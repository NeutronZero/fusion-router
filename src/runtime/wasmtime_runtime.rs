use crate::runtime::config::SandboxConfig;
use crate::runtime::context::RuntimeContext;
use crate::runtime::module_cache::RuntimeModuleCache;
use crate::runtime::sandbox_instance::SandboxInstance;
use crate::runtime::sandbox_runtime::SandboxRuntime;
use crate::runtime::RuntimeError;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wasmtime::{Config, Engine, Linker, Memory, Module, ResourceLimiter, Store, TypedFunc};

pub struct WasmtimeSandboxRuntime {
    engine: Engine,
    _module_cache: Arc<RuntimeModuleCache>,
    config: SandboxConfig,
}

struct StoreData {
    memory_limit: usize,
    growth_rejected: Arc<AtomicBool>,
}

impl ResourceLimiter for StoreData {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        if desired > self.memory_limit {
            self.growth_rejected.store(true, Ordering::SeqCst);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        Ok(true)
    }
}

impl WasmtimeSandboxRuntime {
    pub fn new(
        config: SandboxConfig,
        module_cache: Arc<RuntimeModuleCache>,
    ) -> Result<Self, RuntimeError> {
        let mut wasm_config = Config::new();
        wasm_config.wasm_bulk_memory(true);
        wasm_config.wasm_multi_value(true);
        wasm_config.consume_fuel(true);

        let engine = Engine::new(&wasm_config)
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

        Ok(Self {
            engine,
            _module_cache: module_cache,
            config,
        })
    }
}

#[async_trait]
impl SandboxRuntime for WasmtimeSandboxRuntime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    async fn instantiate(
        &self,
        module_bytes: &[u8],
        _ctx: RuntimeContext,
    ) -> Result<Box<dyn SandboxInstance>, RuntimeError> {
        let module = Module::new(&self.engine, module_bytes)
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

        let growth_rejected = Arc::new(AtomicBool::new(false));
        let store_data = StoreData {
            memory_limit: self.config.memory_limit_bytes as usize,
            growth_rejected: growth_rejected.clone(),
        };

        let mut store = Store::new(&self.engine, store_data);
        store
            .set_fuel(self.config.fuel_amount)
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

        store.limiter(move |data: &mut StoreData| data as &mut dyn ResourceLimiter);

        let linker = Linker::new(&self.engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| RuntimeError::CompilationFailed("memory export not found".into()))?;

        let allocate = instance
            .get_typed_func::<i32, i32>(&mut store, "allocate")
            .ok();

        let invoke = instance
            .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, "capability_invoke")
            .map_err(|e| {
                RuntimeError::CompilationFailed(format!("capability_invoke export not found: {e}"))
            })?;

        let fuel_initial = store.get_fuel().unwrap_or(0);

        Ok(Box::new(WasmtimeSandboxInstance {
            store,
            memory,
            allocate,
            invoke,
            fuel_initial,
            growth_rejected,
            max_response_bytes: self.config.max_response_bytes,
        }))
    }
}

pub struct WasmtimeSandboxInstance {
    store: Store<StoreData>,
    memory: Memory,
    allocate: Option<TypedFunc<i32, i32>>,
    invoke: TypedFunc<(i32, i32), (i32, i32)>,
    fuel_initial: u64,
    growth_rejected: Arc<AtomicBool>,
    max_response_bytes: usize,
}

impl WasmtimeSandboxInstance {
    fn map_trap_error(err: wasmtime::Error) -> RuntimeError {
        if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
            return match trap {
                wasmtime::Trap::OutOfFuel => RuntimeError::FuelExhausted,
                _ => RuntimeError::ExecutionTrap {
                    message: err.to_string(),
                },
            };
        }
        RuntimeError::ExecutionTrap {
            message: err.to_string(),
        }
    }
}

#[async_trait]
impl SandboxInstance for WasmtimeSandboxInstance {
    async fn invoke(&mut self, input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        self.growth_rejected.store(false, Ordering::SeqCst);

        let len = input.len() as i32;

        let ptr = if let Some(ref alloc) = self.allocate {
            alloc
                .call(&mut self.store, len)
                .map_err(Self::map_trap_error)?
        } else {
            0
        };

        self.memory
            .write(&mut self.store, ptr as usize, input)
            .map_err(|_| RuntimeError::OutOfMemory)?;

        let (out_ptr, out_len) = self
            .invoke
            .call(&mut self.store, (ptr, len))
            .map_err(Self::map_trap_error)?;

        if self.growth_rejected.load(Ordering::SeqCst) {
            return Err(RuntimeError::OutOfMemory);
        }

        // Reject guest-claimed output lengths that exceed the configured cap
        // BEFORE allocating a host buffer: the guest controls `out_len`, so an
        // unbounded allocation here is an OOM denial-of-service vector.
        if out_len as usize > self.max_response_bytes {
            return Err(RuntimeError::OutOfMemory);
        }

        let mut output = vec![0u8; out_len as usize];
        self.memory
            .read(&self.store, out_ptr as usize, &mut output)
            .map_err(|_| RuntimeError::OutOfMemory)?;

        Ok(output)
    }

    fn reset(&mut self) -> Result<(), RuntimeError> {
        Err(RuntimeError::NotSupported("reset not supported".into()))
    }

    fn memory_usage(&self) -> u64 {
        self.memory.data_size(&self.store) as u64
    }

    fn fuel_consumed(&self) -> u64 {
        self.fuel_initial
            .saturating_sub(self.store.get_fuel().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::config::SandboxConfig;
    use crate::runtime::context::RuntimeContext;
    use crate::runtime::host_services::CapabilityHostServices;
    use crate::runtime::module_cache::RuntimeModuleCache;
    use crate::runtime::telemetry_context::TelemetryContext;
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockHostServices;

    #[async_trait]
    impl CapabilityHostServices for MockHostServices {
        async fn emit_event(
            &self,
            _event: crate::events::payload::ExecutionEvent,
        ) -> Result<(), crate::release::gate::GateError> {
            Ok(())
        }
        async fn log(&self, _level: tracing::Level, _message: &str) {}
        async fn fetch_secret(
            &self,
            _secret_name: &str,
        ) -> Result<String, crate::release::gate::GateError> {
            Err(crate::release::gate::GateError::PermissionDenied(
                "mock denied".into(),
            ))
        }
        async fn http_request(
            &self,
            _req: reqwest::Request,
        ) -> Result<reqwest::Response, crate::release::gate::GateError> {
            Err(crate::release::gate::GateError::PermissionDenied(
                "mock denied".into(),
            ))
        }
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
    async fn instantiate_and_invoke_echo() {
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

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        let input = b"hello world";
        let output = instance.invoke(input).await.unwrap();
        assert_eq!(output, input);
    }

    #[tokio::test]
    async fn fuel_exhaustion_kills_instance() {
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

        let result = instance.invoke(b"data").await;
        match result {
            Err(RuntimeError::FuelExhausted) => {}
            Err(other) => panic!("expected FuelExhausted, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn trap_during_execution_returns_error() {
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
        match result {
            Err(RuntimeError::ExecutionTrap { .. }) => {}
            Err(other) => panic!("expected ExecutionTrap, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn memory_limit_enforced() {
        let config = SandboxConfig {
            memory_limit_bytes: 1024 * 1024,
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
                    (memory.grow (i32.const 65536))
                    drop
                    local.get 0
                    local.get 1
                )
            )
        "#;

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        let result = instance.invoke(b"data").await;
        assert!(
            matches!(
                &result,
                Err(RuntimeError::OutOfMemory) | Err(RuntimeError::ExecutionTrap { .. })
            ),
            "expected memory error, got {result:?}"
        );
    }

    #[test]
    fn runtime_name_is_static_str() {
        let config = SandboxConfig::default();
        let cache = Arc::new(RuntimeModuleCache::new());
        let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();
        assert!(!runtime.name().is_empty());
    }

    #[tokio::test]
    async fn memory_usage_returns_non_zero() {
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

        assert!(instance.memory_usage() > 0, "should report memory usage");
    }

    #[tokio::test]
    async fn fuel_consumed_after_invoke() {
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

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        let fuel_before = instance.fuel_consumed();
        let _ = instance.invoke(b"hello").await.unwrap();
        let fuel_after = instance.fuel_consumed();
        assert!(fuel_after >= fuel_before, "fuel should not decrease");
    }

    #[tokio::test]
    async fn invalid_wasm_returns_compilation_error() {
        let config = SandboxConfig::default();
        let cache = Arc::new(RuntimeModuleCache::new());
        let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

        let result = runtime.instantiate(b"not valid wasm", test_context()).await;

        match result {
            Err(RuntimeError::CompilationFailed(_)) => {}
            Err(other) => panic!("expected CompilationFailed, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn reset_returns_not_supported() {
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

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        match instance.reset() {
            Err(RuntimeError::NotSupported(_)) => {}
            Err(other) => panic!("expected NotSupported, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
