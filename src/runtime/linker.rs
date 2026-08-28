use crate::runtime::host_services::CapabilityHostServices;
use std::sync::Arc;
use wasmtime::{Caller, Extern, Linker};

pub trait HostAccess {
    fn host_services(&self) -> &Arc<dyn CapabilityHostServices>;
    fn runtime_handle(&self) -> tokio::runtime::Handle;
}

impl HostAccess for Arc<dyn CapabilityHostServices> {
    fn host_services(&self) -> &Arc<dyn CapabilityHostServices> {
        self
    }
    fn runtime_handle(&self) -> tokio::runtime::Handle {
        tokio::runtime::Handle::try_current()
            .expect("HostAccess host requires a tokio runtime to be available")
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
        |_caller: Caller<'_, T>, _event_ptr: i32, _event_len: i32| -> i32 { 0 },
    )?;

    linker.func_wrap(
        "host",
        "log",
        |_caller: Caller<'_, T>, _level: i32, _msg_ptr: i32, _msg_len: i32| {},
    )?;

    linker.func_wrap(
        "host",
        "record_metric",
        |_caller: Caller<'_, T>, _name_ptr: i32, _name_len: i32, _value: f64| {},
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
                reqwest::Client::new(),
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
            reqwest::Client::new(),
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
