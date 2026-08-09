use std::sync::Arc;
use wasmtime::Linker;
use crate::runtime::host_services::CapabilityHostServices;

pub fn configure_linker(
    linker: &mut Linker<Arc<dyn CapabilityHostServices>>,
) -> Result<(), anyhow::Error> {
    linker.func_wrap("host", "emit_event", |_event_ptr: i32, _event_len: i32| -> i32 {
        0
    })?;

    linker.func_wrap("host", "log", |_level: i32, _msg_ptr: i32, _msg_len: i32| {
    })?;

    linker.func_wrap("host", "fetch_secret", |_name_ptr: i32, _name_len: i32, _out_ptr: i32, _out_len: i32| -> i32 {
        -1
    })?;

    linker.func_wrap("host", "http_request", |_req_ptr: i32, _req_len: i32, _resp_ptr: i32, _resp_len: i32| -> i32 {
        -1
    })?;

    linker.func_wrap("host", "record_metric", |_name_ptr: i32, _name_len: i32, _value: f64| {
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Config, Engine, Module};
    use crate::runtime::host_services::CapabilityHostServices;
    use crate::events::bus::BroadcastEventBus;
    use crate::telemetry::metrics::FusionMetrics;
    use std::sync::Arc;

    #[test]
    fn test_linker_import_signatures_match() {
        let config = Config::new();
        let engine = Engine::new(&config).unwrap();

        let host: Arc<dyn CapabilityHostServices> = Arc::new(
            crate::runtime::wasmtime_host::WasmtimeCapabilityHost::new(
                Arc::new(crate::capability::InMemoryCapabilityRegistry::new()),
                Arc::new(BroadcastEventBus::new(16)),
                reqwest::Client::new(),
                FusionMetrics::instance(),
                uuid::Uuid::new_v4(),
                uuid::Uuid::new_v4(),
                None,
            )
        );

        let mut linker = Linker::new(&engine);
        let mut store = wasmtime::Store::new(&engine, host.clone());

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
        assert!(instance.is_ok(), "linker should satisfy all 5 imports: {:?}", instance.err());
    }
}
