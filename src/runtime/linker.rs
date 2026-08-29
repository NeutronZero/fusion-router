use crate::events::payload::ExecutionEvent;
use crate::runtime::host_services::CapabilityHostServices;
use std::sync::Arc;
use tracing::Level;
use wasmtime::{Caller, Extern, Linker};

pub trait HostAccess {
    fn host_services(&self) -> &Arc<dyn CapabilityHostServices>;
    fn runtime_handle(&self) -> tokio::runtime::Handle;
}

/// A safe [`HostAccess`] implementation that holds both the host services and
/// an explicitly captured tokio runtime handle. Unlike the previous
/// `Arc<dyn CapabilityHostServices>` impl (which called
/// `Handle::try_current().expect(...)` and panicked outside a runtime), this
/// never depends on a current-thread runtime context and is therefore safe to
/// use from plain `#[test]`s or any blocking context.
pub struct WasmHostContext {
    host: Arc<dyn CapabilityHostServices>,
    runtime: tokio::runtime::Handle,
}

impl WasmHostContext {
    pub fn new(host: Arc<dyn CapabilityHostServices>, runtime: tokio::runtime::Handle) -> Self {
        Self { host, runtime }
    }
}

impl HostAccess for WasmHostContext {
    fn host_services(&self) -> &Arc<dyn CapabilityHostServices> {
        &self.host
    }
    fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.clone()
    }
}

fn read_memory<T: HostAccess>(
    caller: &mut Caller<'_, T>,
    ptr: i32,
    len: i32,
) -> Option<Vec<u8>> {
    if ptr < 0 || len < 0 || len as usize > (1 << 20) {
        return None;
    }
    let mem = caller.get_export("memory").and_then(Extern::into_memory)?;
    let mut buf = vec![0u8; len as usize];
    mem.read(&mut *caller, ptr as usize, &mut buf).ok()?;
    Some(buf)
}

pub fn configure_linker<T: HostAccess + 'static>(
    linker: &mut Linker<T>,
) -> Result<(), anyhow::Error> {
    linker.func_wrap(
        "host",
        "emit_event",
        |mut caller: Caller<'_, T>, event_ptr: i32, event_len: i32| -> i32 {
            let bytes = match read_memory(&mut caller, event_ptr, event_len) {
                Some(b) => b,
                None => return -1,
            };
            let event: ExecutionEvent = match serde_json::from_slice(&bytes) {
                Ok(e) => e,
                Err(_) => return -1,
            };
            let host = caller.data().host_services().clone();
            let result = caller.data().runtime_handle().block_on(host.emit_event(event));
            match result {
                Ok(()) => 0,
                Err(_) => -1,
            }
        },
    )?;

    linker.func_wrap(
        "host",
        "log",
        |mut caller: Caller<'_, T>, level: i32, msg_ptr: i32, msg_len: i32| {
            let bytes = match read_memory(&mut caller, msg_ptr, msg_len) {
                Some(b) => b,
                None => return,
            };
            let message = String::from_utf8_lossy(&bytes).into_owned();
            let level = match level {
                0 => Level::ERROR,
                1 => Level::WARN,
                2 => Level::INFO,
                3 => Level::DEBUG,
                4 => Level::TRACE,
                _ => Level::INFO,
            };
            let host = caller.data().host_services().clone();
            caller.data().runtime_handle().block_on(host.log(level, &message));
        },
    )?;

    linker.func_wrap(
        "host",
        "record_metric",
        |mut caller: Caller<'_, T>, name_ptr: i32, name_len: i32, value: f64| {
            let bytes = match read_memory(&mut caller, name_ptr, name_len) {
                Some(b) => b,
                None => return,
            };
            let name = String::from_utf8_lossy(&bytes).into_owned();
            let host = caller.data().host_services().clone();
            host.record_metric(&name, value);
        },
    )?;

    linker.func_wrap(
        "host",
        "fetch_secret",
        |mut caller: Caller<'_, T>,
         name_ptr: i32,
         name_len: i32,
         out_ptr: i32,
         out_len: i32|
         -> i32 {
            let bytes = match read_memory(&mut caller, name_ptr, name_len) {
                Some(b) => b,
                None => return -1,
            };
             let name = String::from_utf8_lossy(&bytes).into_owned();
             let host = caller.data().host_services().clone();
             let result = caller.data().runtime_handle().block_on(host.fetch_secret(&name));
             match result {
                Ok(val) => {
                    let data = val.as_bytes();
                    if (out_len as usize) < data.len() {
                        return -1;
                    }
                    match caller.get_export("memory").and_then(Extern::into_memory) {
                        Some(mem) => {
                            if mem.write(&mut caller, out_ptr as usize, data).is_ok() {
                                0
                            } else {
                                -1
                            }
                        }
                        None => -1,
                    }
                }
                Err(_) => -1,
            }
        },
    )?;

    linker.func_wrap(
        "host",
        "http_request",
        |mut caller: Caller<'_, T>,
         req_ptr: i32,
         req_len: i32,
         _resp_ptr: i32,
         _resp_len: i32|
         -> i32 {
            let bytes = match read_memory(&mut caller, req_ptr, req_len) {
                Some(b) => b,
                None => return -1,
            };
            let url_str = match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };
            let url = match reqwest::Url::parse(&url_str) {
                Ok(u) => u,
                Err(_) => return -1,
            };
            let req = reqwest::Request::new(reqwest::Method::GET, url);
             let host = caller.data().host_services().clone();
             let result = caller.data().runtime_handle().block_on(host.http_request(req));
             match result {
                Ok(_) => 0,
                Err(_) => -1,
            }
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::bus::BroadcastEventBus;
    use crate::runtime::host_services::CapabilityHostServices;
    use fusion_plugin_api::Permission;
    use crate::telemetry::metrics::FusionMetrics;
    use std::sync::Arc;
    use wasmtime::{Config, Engine, Module, Store};

    struct TestStore {
        host: Arc<dyn CapabilityHostServices>,
        runtime: tokio::runtime::Handle,
    }

    impl HostAccess for TestStore {
        fn host_services(&self) -> &Arc<dyn CapabilityHostServices> {
            &self.host
        }
        fn runtime_handle(&self) -> tokio::runtime::Handle {
            self.runtime.clone()
        }
    }

    #[test]
    fn test_linker_import_signatures_match() {
        let config = Config::new();
        let engine = Engine::new(&config).unwrap();

        let host: Arc<dyn CapabilityHostServices> =
            Arc::new(crate::runtime::wasmtime_host::WasmtimeCapabilityHost::new(
                Arc::new(crate::capability::InMemoryCapabilityRegistry::new()),
                Arc::new(BroadcastEventBus::new(16)),
                FusionMetrics::instance(),
                uuid::Uuid::new_v4(),
                uuid::Uuid::new_v4(),
                None,
            ));

        let mut linker: Linker<TestStore> = Linker::new(&engine);
        let mut store = Store::new(
            &engine,
            TestStore {
                host,
                runtime: tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .handle()
                    .clone(),
            },
        );

        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "emit_event" (func (param i32 i32) (result i32)))
                (import "host" "log" (func (param i32 i32 i32)))
                (import "host" "fetch_secret" (func (param i32 i32 i32 i32) (result i32)))
                (import "host" "http_request" (func (param i32 i32 i32 i32) (result i32)))
                (import "host" "record_metric" (func (param i32 i32 f64)))
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let instance = linker.instantiate(&mut store, &module);
        assert!(
            instance.is_ok(),
            "linker should satisfy all 5 imports: {:?}",
            instance.err()
        );
    }

    fn make_host(perms: Option<Vec<Permission>>) -> Arc<dyn CapabilityHostServices> {
        let base = crate::runtime::wasmtime_host::WasmtimeCapabilityHost::new(
            Arc::new(crate::capability::InMemoryCapabilityRegistry::new()),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            None,
        );
        let base = if let Some(p) = perms {
            base.with_caller_permissions(p)
        } else {
            base
        };
        Arc::new(base)
    }

    const GUEST_WAT: &str = r#"
        (module
            (import "host" "fetch_secret" (func $fs (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "db_password")
            (func (export "run") (result i32)
                (call $fs (i32.const 0) (i32.const 11) (i32.const 0) (i32.const 64))
            )
        )
    "#;

    #[tokio::test]
    async fn test_guest_fetch_secret_denied_without_permissions() {
        let rt = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let host = make_host(None);
            let engine = Engine::new(&Config::new()).unwrap();
            let mut linker: Linker<TestStore> = Linker::new(&engine);
            let mut store = Store::new(&engine, TestStore { host, runtime: rt });
            configure_linker(&mut linker).unwrap();
            let module = Module::new(&engine, GUEST_WAT).unwrap();
            let instance = linker.instantiate(&mut store, &module).unwrap();
            let run = instance
                .get_typed_func::<(), i32>(&mut store, "run")
                .unwrap();
            run.call(&mut store, ()).unwrap()
        })
        .await
        .unwrap();
        assert_eq!(
            result, -1,
            "fetch_secret must be denied (fail-closed) without caller permissions"
        );
    }

    #[tokio::test]
    async fn test_guest_fetch_secret_allowed_with_permission() {
        std::env::set_var("db_password", "s3cr3t");
        let rt = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let host = make_host(Some(vec![Permission::Secrets("db_password".into())]));
            let engine = Engine::new(&Config::new()).unwrap();
            let mut linker: Linker<TestStore> = Linker::new(&engine);
            let mut store = Store::new(&engine, TestStore { host, runtime: rt });
            configure_linker(&mut linker).unwrap();
            let module = Module::new(&engine, GUEST_WAT).unwrap();
            let instance = linker.instantiate(&mut store, &module).unwrap();
            let run = instance
                .get_typed_func::<(), i32>(&mut store, "run")
                .unwrap();
            run.call(&mut store, ()).unwrap()
        })
        .await
        .unwrap();
        std::env::remove_var("db_password");
        assert_eq!(
            result, 0,
            "fetch_secret must succeed when the caller holds the matching Secrets permission"
        );
    }
}
