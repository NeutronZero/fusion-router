#![cfg(feature = "wasm-plugins")]

use fusion_kernel::capability::{CapabilityRegistry, InMemoryCapabilityRegistry};
use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
use fusion_router::events::bus::BroadcastEventBus;
use fusion_router::runtime::host_services::CapabilityHostServices;
use fusion_router::runtime::linker::{configure_linker, WasmHostContext};
use fusion_router::telemetry::metrics::FusionMetrics;
use std::sync::Arc;
use wasmtime::{Config, Engine, Linker, Module, Store};

fn make_engine() -> Engine {
    let config = Config::new();
    Engine::new(&config).unwrap()
}

fn make_host(permissions: Vec<Permission>) -> Arc<dyn CapabilityHostServices> {
    let mut reg = InMemoryCapabilityRegistry::new();
    let contract = CapabilityContract {
        id: CapabilityId::new("test.integration"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "integration test cap".into(),
        inputs_schema: serde_json::json!({}),
        outputs_schema: serde_json::json!({}),
        permissions,
        dependencies: vec![],
        estimated_cost: fusion_core::NanoUSD::ZERO,
        estimated_latency_ms: 0,
        reliability_score: 1.0,
        supports_streaming: false,
        traits: vec![],
    };
    reg.register(contract).unwrap();

    Arc::new(
        fusion_router::runtime::wasmtime_host::WasmtimeCapabilityHost::new(
            Arc::new(reg),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            None,
        ),
    )
}

/// Run `f` with a captured tokio runtime handle on a dedicated OS thread so
/// that the host shims can `block_on` the async host services without requiring
/// a current-thread runtime context (this avoids the old
/// `Handle::try_current().expect(...)` panic that fired inside plain `#[test]`s).
fn with_runtime<F>(f: F)
where
    F: FnOnce(tokio::runtime::Handle) + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();
    std::thread::spawn(move || f(handle))
        .join()
        .expect("runtime thread panicked");
}

#[test]
fn test_wasm_calls_emit_event() {
    with_runtime(|handle| {
        let engine = make_engine();
        let host = make_host(vec![]);
        let mut linker = Linker::new(&engine);
        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "emit_event" (func $emit (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "capability_invoke") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 0
                    call $emit
                    drop
                    i32.const 0
                )
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let mut store = Store::new(&engine, WasmHostContext::new(host, handle));
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
            .unwrap();
        let result = invoke.call(&mut store, (0, 0));
        assert!(
            result.is_ok(),
            "emit_event should not trap: {:?}",
            result.err()
        );
    });
}

#[test]
fn test_wasm_fetch_secret_permission_denied() {
    with_runtime(|handle| {
        let engine = make_engine();
        let host = make_host(vec![Permission::Network]);
        let mut linker = Linker::new(&engine);
        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "fetch_secret" (func $fetch (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "capability_invoke") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    call $fetch
                )
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let mut store = Store::new(&engine, WasmHostContext::new(host, handle));
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
            .unwrap();
        let result = invoke.call(&mut store, (0, 0));
        assert!(
            result.is_ok(),
            "fetch_secret should not trap even on denied: {:?}",
            result.err()
        );
    });
}

#[test]
fn test_wasm_http_request_permission_denied() {
    with_runtime(|handle| {
        let engine = make_engine();
        let host = make_host(vec![Permission::Secrets("x".into())]);
        let mut linker = Linker::new(&engine);
        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "http_request" (func $http (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "capability_invoke") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    call $http
                )
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let mut store = Store::new(&engine, WasmHostContext::new(host, handle));
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
            .unwrap();
        let result = invoke.call(&mut store, (0, 0));
        assert!(
            result.is_ok(),
            "http_request should not trap even on denied"
        );
    });
}

#[test]
fn test_wasm_log_no_trap() {
    with_runtime(|handle| {
        let engine = make_engine();
        let host = make_host(vec![]);
        let mut linker = Linker::new(&engine);
        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "log" (func $log (param i32 i32 i32)))
                (memory (export "memory") 1)
                (func (export "capability_invoke") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    call $log
                    i32.const 0
                )
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let mut store = Store::new(&engine, WasmHostContext::new(host, handle));
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
            .unwrap();
        let result = invoke.call(&mut store, (0, 0));
        assert!(result.is_ok(), "log should not trap");
    });
}

#[test]
fn test_wasm_record_metric_forwards() {
    with_runtime(|handle| {
        let engine = make_engine();
        let host = make_host(vec![]);
        let mut linker = Linker::new(&engine);
        configure_linker(&mut linker).unwrap();

        let wat = r#"
            (module
                (import "host" "record_metric" (func $rm (param i32 i32 f64)))
                (memory (export "memory") 1)
                (data (i32.const 0) "test.metric")
                (func (export "capability_invoke") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 11
                    f64.const 3.5
                    call $rm
                    i32.const 0
                )
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let mut store = Store::new(&engine, WasmHostContext::new(host, handle));
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
            .unwrap();
        let result = invoke.call(&mut store, (0, 0));
        assert!(
            result.is_ok(),
            "record_metric should forward and not trap: {:?}",
            result.err()
        );
    });
}
