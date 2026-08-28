use crate::runtime::config::SandboxConfig;
use crate::runtime::context::RuntimeContext;
use crate::runtime::host_services::CapabilityHostServices;
use crate::runtime::linker::{configure_linker, HostAccess};
use crate::runtime::module_cache::RuntimeModuleCache;
use crate::runtime::sandbox_instance::SandboxInstance;
use crate::runtime::sandbox_runtime::SandboxRuntime;
use crate::runtime::RuntimeError;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wasmtime::{Config, Engine, Linker, Memory, Module, ResourceLimiter, Store, TypedFunc};

/// Hard cap on guest table growth (elements). Tables are host-allocated
/// memory for function references; without a cap a guest can allocate GBs of
/// host memory while staying well within its fuel budget.
const MAX_TABLE_ELEMENTS: usize = 10_000;

const EPOCH_INTERVAL_MS: u64 = 100;

pub struct WasmtimeSandboxRuntime {
    engine: Engine,
    _module_cache: Arc<RuntimeModuleCache>,
    config: SandboxConfig,
}

struct StoreData {
    memory_limit: usize,
    growth_rejected: Arc<AtomicBool>,
    host_services: Arc<dyn CapabilityHostServices>,
    runtime_handle: tokio::runtime::Handle,
}

impl HostAccess for StoreData {
    fn host_services(&self) -> &Arc<dyn CapabilityHostServices> {
        &self.host_services
    }
    fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime_handle.clone()
    }
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
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        // Mirror the memory policy: growth beyond the fixed element cap is
        // rejected so the guest sees a failed grow instead of pinning GBs of
        // host memory.
        Ok(desired <= MAX_TABLE_ELEMENTS)
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
        wasm_config.epoch_interruption(true);

        let engine = Engine::new(&wasm_config)
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

        let ticker_engine = engine.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(EPOCH_INTERVAL_MS));
            ticker_engine.increment_epoch();
        });

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
        ctx: RuntimeContext,
    ) -> Result<Box<dyn SandboxInstance>, RuntimeError> {
        // Compilation + instantiation are pure CPU-bound work (Cranelift);
        // run them off the async worker threads.
        let engine = self.engine.clone();
        let module_bytes = module_bytes.to_vec();
        let memory_limit = self.config.memory_limit_bytes as usize;
        let fuel_amount = self.config.fuel_amount;
        let max_response_bytes = self.config.max_response_bytes;
        let host_services = ctx.host_services.clone();

        let effective_deadline: Option<tokio::time::Instant> = match (ctx.deadline, self.config.timeout_ms) {
            (Some(d), _) => Some(d),
            (None, Some(ms)) => Some(tokio::time::Instant::now() + Duration::from_millis(ms)),
            (None, None) => None,
        };
        let epoch_duration = effective_deadline
            .map(|d| d.saturating_duration_since(tokio::time::Instant::now()));

        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|_| RuntimeError::CompilationFailed("wasmtime sandbox requires a tokio runtime".into()))?;

        let build = tokio::task::spawn_blocking(move || {
            build_instance(
                &engine,
                &module_bytes,
                StoreData {
                    memory_limit,
                    growth_rejected: Arc::new(AtomicBool::new(false)),
                    host_services,
                    runtime_handle,
                },
                fuel_amount,
                max_response_bytes,
                epoch_duration,
            )
        });

        let built = match effective_deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, build)
                .await
                .map_err(|_| RuntimeError::ExecutionTrap {
                    message: "wasm instantiation exceeded the runtime deadline".into(),
                })?,
            None => build.await,
        }
        .map_err(|e| RuntimeError::CompilationFailed(format!("wasm setup task panicked: {e}")))?;

        let wasm_inner = built?;

        Ok(Box::new(WasmtimeSandboxInstance {
            inner: Some(WasmInner {
                deadline: effective_deadline,
                ..wasm_inner
            }),
        }))
    }
}
/// Everything needed to execute a module instance; moved as a unit into
/// `spawn_blocking` closures and back out afterwards.
struct WasmInner {
    store: Store<StoreData>,
    memory: Memory,
    allocate: Option<TypedFunc<i32, i32>>,
    invoke: TypedFunc<(i32, i32), (i32, i32)>,
    fuel_initial: u64,
    growth_rejected: Arc<AtomicBool>,
    max_response_bytes: usize,
    /// Wall-clock deadline captured from the RuntimeContext that produced
    /// this instance; enforced on every blocking join.
    deadline: Option<tokio::time::Instant>,
}

type BuiltInstance = Result<WasmInner, RuntimeError>;

fn build_instance(
    engine: &Engine,
    module_bytes: &[u8],
    store_data: StoreData,
    fuel_amount: u64,
    max_response_bytes: usize,
    epoch_deadline: Option<Duration>,
) -> BuiltInstance {
    let growth_rejected = store_data.growth_rejected.clone();

    let module = Module::new(engine, module_bytes)
        .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

    let mut store = Store::new(engine, store_data);
    store
        .set_fuel(fuel_amount)
        .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))?;

    store.limiter(move |data: &mut StoreData| data as &mut dyn ResourceLimiter);

    if let Some(d) = epoch_deadline {
        let epochs = ((d.as_millis() / EPOCH_INTERVAL_MS as u128).max(1)) as u64 + 1;
        store.set_epoch_deadline(epochs);
    }

    let mut linker = Linker::new(engine);
    configure_linker(&mut linker)
        .map_err(|e| RuntimeError::CompilationFailed(format!("linker setup failed: {e}")))?;
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

    Ok(WasmInner {
        store,
        memory,
        allocate,
        invoke,
        fuel_initial,
        growth_rejected,
        max_response_bytes,
        // Overwritten by the caller with the RuntimeContext deadline.
        deadline: None,
    })
}

pub struct WasmtimeSandboxInstance {
    inner: Option<WasmInner>,
}

impl WasmtimeSandboxInstance {
    fn inner(&self) -> &WasmInner {
        self.inner
            .as_ref()
            .expect("sandbox instance inner state is only absent inside invoke()")
    }

    fn map_trap_error(err: wasmtime::Error) -> RuntimeError {
        if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
            return match trap {
                wasmtime::Trap::OutOfFuel => RuntimeError::FuelExhausted,
                wasmtime::Trap::Interrupt => RuntimeError::ExecutionTrap {
                    message: "wasm exceeded its wall-clock epoch deadline".into(),
                },
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
        // Move the whole execution environment onto a blocking thread:
        // guest code can burn seconds of CPU under a full fuel budget.
        let deadline = self
            .inner
            .as_ref()
            .and_then(|wasm_inner| wasm_inner.deadline);
        let mut inner = self
            .inner
            .take()
            .ok_or_else(|| RuntimeError::NotSupported("invoke already in flight".into()))?;
        let input = input.to_vec();

        let exec = tokio::task::spawn_blocking(move || {
            inner.growth_rejected.store(false, Ordering::SeqCst);

            let len = input.len() as i32;

            let ptr = if let Some(ref alloc) = inner.allocate {
                alloc
                    .call(&mut inner.store, len)
                    .map_err(Self::map_trap_error)?
            } else {
                0
            };

            inner
                .memory
                .write(&mut inner.store, ptr as usize, &input)
                .map_err(|_| RuntimeError::OutOfMemory)?;

            let (out_ptr, out_len) = inner
                .invoke
                .call(&mut inner.store, (ptr, len))
                .map_err(Self::map_trap_error)?;

            if inner.growth_rejected.load(Ordering::SeqCst) {
                return Err(RuntimeError::OutOfMemory);
            }

            // Reject guest-claimed output lengths that exceed the configured
            // cap BEFORE allocating a host buffer: the guest controls
            // `out_len`, so an unbounded allocation here is an OOM
            // denial-of-service vector.
            if out_len as usize > inner.max_response_bytes {
                return Err(RuntimeError::OutOfMemory);
            }

            let mut output = vec![0u8; out_len as usize];
            inner
                .memory
                .read(&inner.store, out_ptr as usize, &mut output)
                .map_err(|_| RuntimeError::OutOfMemory)?;

            Ok((inner, output))
        });

        let completed = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, exec).await.map_err(|_| {
                RuntimeError::ExecutionTrap {
                    message: "wasm invocation exceeded the runtime deadline".into(),
                }
            })?,
            None => exec.await,
        }
        .map_err(|e| RuntimeError::ExecutionTrap {
            message: format!("wasm invocation task panicked: {e}"),
        })?;

        match completed {
            Ok((returned, output)) => {
                self.inner = Some(returned);
                Ok(output)
            }
            Err(e) => Err(e),
        }
    }

    fn reset(&mut self) -> Result<(), RuntimeError> {
        Err(RuntimeError::NotSupported("reset not supported".into()))
    }

    fn memory_usage(&self) -> u64 {
        let inner = self.inner();
        inner.memory.data_size(&inner.store) as u64
    }

    fn fuel_consumed(&self) -> u64 {
        let inner = self.inner();
        inner
            .fuel_initial
            .saturating_sub(inner.store.get_fuel().unwrap_or(0))
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
    async fn table_growth_beyond_cap_is_rejected() {
        let config = SandboxConfig::default();
        let cache = Arc::new(RuntimeModuleCache::new());
        let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

        // The raw table.grow result (-1 on rejection) is stored to memory so
        // it survives the host-side response-length check.
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (table $t 1 funcref)
                (func (export "allocate") (param i32) (result i32)
                    i32.const 0
                )
                (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                    (i32.store8 (i32.const 0)
                        (table.grow $t (ref.null func) (i32.const 20000)))
                    i32.const 0
                    i32.const 1
                )
            )
        "#;

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        let output = instance.invoke(b"t").await.unwrap();
        assert_eq!(
            output[0], 255,
            "table growth beyond MAX_TABLE_ELEMENTS must be rejected (-1)"
        );
    }

    #[tokio::test]
    async fn table_growth_within_cap_is_allowed() {
        let config = SandboxConfig::default();
        let cache = Arc::new(RuntimeModuleCache::new());
        let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

        let wat = r#"
            (module
                (memory (export "memory") 1)
                (table $t 1 funcref)
                (func (export "allocate") (param i32) (result i32)
                    i32.const 0
                )
                (func (export "capability_invoke") (param i32 i32) (result i32 i32)
                    (i32.store8 (i32.const 0)
                        (table.grow $t (ref.null func) (i32.const 5)))
                    i32.const 0
                    i32.const 1
                )
            )
        "#;

        let mut instance = runtime
            .instantiate(wat.as_bytes(), test_context())
            .await
            .unwrap();

        let output = instance.invoke(b"t").await.unwrap();
        assert_eq!(
            output[0], 1,
            "small table growth must succeed (previous size returned)"
        );
    }

    #[tokio::test]
    async fn expired_deadline_aborts_instantiation() {
        let config = SandboxConfig::default();
        let cache = Arc::new(RuntimeModuleCache::new());
        let runtime = WasmtimeSandboxRuntime::new(config, cache).unwrap();

        let mut ctx = test_context();
        // Deadline already in the past: the blocking join must be cut short
        // and mapped onto ExecutionTrap, not run to completion.
        ctx.deadline = Some(tokio::time::Instant::now() - std::time::Duration::from_secs(1));

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

        let result = runtime.instantiate(wat.as_bytes(), ctx).await;
        match result {
            Err(RuntimeError::ExecutionTrap { message }) => {
                assert!(message.contains("deadline"), "{message}");
            }
            Err(other) => panic!("expected deadline ExecutionTrap, got {other:?}"),
            Ok(_) => panic!("expired deadline must not produce an instance"),
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
