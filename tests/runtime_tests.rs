#![cfg(feature = "wasm-plugins")]

use std::sync::Arc;
use uuid::Uuid;
use fusion_router::events::ExecutionEvent;
use fusion_router::release::gate::GateError;
use fusion_router::runtime::{
    CapabilityHostServices, RuntimeContext, RuntimeError, RuntimeModuleCache, SandboxConfig,
    SandboxRuntime, TelemetryContext, WasmtimeSandboxRuntime,
};

struct MockHostServices;

#[async_trait::async_trait]
impl CapabilityHostServices for MockHostServices {
    async fn emit_event(&self, _event: ExecutionEvent) -> Result<(), GateError> { Ok(()) }
    async fn log(&self, _level: tracing::Level, _message: &str) {}
    async fn fetch_secret(&self, _secret_name: &str) -> Result<String, GateError> { Err(GateError::PermissionDenied("mock denied".into())) }
    async fn http_request(&self, _req: reqwest::Request) -> Result<reqwest::Response, GateError> { Err(GateError::PermissionDenied("mock denied".into())) }
    fn record_metric(&self, _name: &str, _value: f64) {}
}

struct MockTelemetryContext {
    execution_id: Uuid,
}

impl MockTelemetryContext {
    fn new() -> Self {
        Self {
            execution_id: Uuid::new_v4(),
        }
    }
}

impl TelemetryContext for MockTelemetryContext {
    fn execution_id(&self) -> &Uuid {
        &self.execution_id
    }
    fn record_counter(&self, _name: &str, _value: u64) {}
}

fn test_context() -> RuntimeContext {
    RuntimeContext {
        execution_id: Uuid::new_v4(),
        host_services: Arc::new(MockHostServices),
        deadline: None,
        telemetry: Arc::new(MockTelemetryContext::new()),
    }
}

#[tokio::test]
async fn full_lifecycle() {
    let config = SandboxConfig::default();
    let cache = Arc::new(RuntimeModuleCache::new());

    let runtime = WasmtimeSandboxRuntime::new(config.clone(), Arc::clone(&cache)).unwrap();
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32)
                i32.const 0
            )
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                local.get 0
                local.get 1
            )
        )
    "#;

    let mut instance1 = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();
    let output1 = instance1.invoke(b"first call").await.unwrap();
    assert_eq!(output1, b"first call");

    let runtime2 = WasmtimeSandboxRuntime::new(config, cache).unwrap();
    let mut instance2 = runtime2
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();
    let output2 = instance2.invoke(b"second call").await.unwrap();
    assert_eq!(output2, b"second call");
}

#[tokio::test]
async fn fuel_exhaustion_returns_proper_error() {
    let config = SandboxConfig {
        fuel_amount: 10,
        ..Default::default()
    };
    let cache = Arc::new(RuntimeModuleCache::new());
    let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32)
                i32.const 0
            )
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                (loop
                    br 0
                )
                local.get 0
                local.get 1
            )
        )
    "#;

    let mut instance = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();

    let result = instance.invoke(b"x").await;
    assert!(
        matches!(result, Err(RuntimeError::FuelExhausted)),
        "expected FuelExhausted, got {result:?}"
    );
}

#[tokio::test]
async fn trap_handling_returns_proper_error() {
    let config = SandboxConfig::default();
    let cache = Arc::new(RuntimeModuleCache::new());
    let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32)
                i32.const 0
            )
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                unreachable
            )
        )
    "#;

    let mut instance = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();

    let result = instance.invoke(b"x").await;
    assert!(
        matches!(result, Err(RuntimeError::ExecutionTrap { .. })),
        "expected ExecutionTrap, got {result:?}"
    );
}

#[tokio::test]
async fn module_without_memory_export_fails_on_instantiate() {
    let config = SandboxConfig::default();
    let cache = Arc::new(RuntimeModuleCache::new());
    let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

    let wat = r#"
        (module
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                local.get 0
                local.get 1
            )
        )
    "#;

    match runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
    {
        Err(RuntimeError::CompilationFailed(_)) => {}
        Err(other) => panic!("expected CompilationFailed, got {other:?}"),
        Ok(_) => panic!("expected instantiation to fail for module without memory"),
    }
}

#[tokio::test]
async fn multiple_instances_from_same_module_are_independent() {
    let config = SandboxConfig::default();
    let cache = Arc::new(RuntimeModuleCache::new());
    let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32)
                i32.const 0
            )
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                local.get 0
                local.get 1
            )
        )
    "#;

    let mut inst_a = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();
    let mut inst_b = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();

    let out_a = inst_a.invoke(b"alpha").await.unwrap();
    let out_b = inst_b.invoke(b"beta").await.unwrap();
    assert_eq!(out_a, b"alpha");
    assert_eq!(out_b, b"beta");
}

#[tokio::test]
async fn metrics_reporting_memory_and_fuel() {
    let config = SandboxConfig::default();
    let cache = Arc::new(RuntimeModuleCache::new());
    let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32)
                i32.const 0
            )
            (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                local.get 0
                local.get 1
            )
        )
    "#;

    let instance = runtime
        .instantiate(wat.as_bytes(), test_context())
        .await
        .unwrap();

    assert!(instance.memory_usage() >= 65536);
    assert!(instance.fuel_consumed() < 100);
}
