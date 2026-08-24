#![cfg(feature = "wasm-plugins")]

use fusion_kernel::capability::{CapabilityRegistry, InMemoryCapabilityRegistry};
use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
use fusion_router::events::bus::BroadcastEventBus;
use fusion_router::runtime::host_services::CapabilityHostServices;
use fusion_router::runtime::linker::configure_linker;
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
            reqwest::Client::new(),
            FusionMetrics::instance(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            None,
        ),
    )
}

#[test]
fn test_wasm_calls_emit_event() {
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
    let mut store = Store::new(&engine, host);
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
}

#[test]
fn test_wasm_fetch_secret_permission_denied() {
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
    let mut store = Store::new(&engine, host);
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
}

#[test]
fn test_wasm_http_request_permission_denied() {
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
    let mut store = Store::new(&engine, host);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let invoke = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
        .unwrap();
    let result = invoke.call(&mut store, (0, 0));
    assert!(
        result.is_ok(),
        "http_request should not trap even on denied"
    );
}

#[test]
fn test_wasm_log_no_trap() {
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
    let mut store = Store::new(&engine, host);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let invoke = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "capability_invoke")
        .unwrap();
    let result = invoke.call(&mut store, (0, 0));
    assert!(result.is_ok(), "log should not trap");
}
