use wasmtime::{Config, Engine, Instance, Linker, Module, ResourceLimiter, Store, Val};

/// Hard cap on guest table growth (elements). Tables allocate host memory for
/// function references; without a cap a guest can exhaust the host within its
/// fuel budget. Mirrors `memory_growing` logic.
pub const WASM_MAX_TABLE_ELEMENTS: usize = 10_000;

/// Default per-instantiation guest memory ceiling.
pub const WASM_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// Fuel budget granted to each instantiation of a module.
pub const WASM_FUEL_PER_INSTANTIATION: u64 = 1_000_000;

/// Interval between engine epoch increments for registered engines.
const EPOCH_TICK_MS: u64 = 20;

/// Epoch ticks a plugin call may consume before being interrupted. With a
/// 20ms tick this bounds a call to roughly one second of wall time.
pub const WASM_EPOCH_DEADLINE_TICKS: u64 = 50;

static EPOCH_ENGINES: std::sync::OnceLock<std::sync::Mutex<Vec<Engine>>> =
    std::sync::OnceLock::new();

/// Registers an epoch-interrupted engine with the global ticker so
/// `set_epoch_deadline` on stores built from it actually fires. The ticker
/// thread is spawned once and lives for the process lifetime; engines are
/// retained by it (plugin runtimes are long-lived by design).
fn register_epoch_engine(engine: Engine) {
    let registry = EPOCH_ENGINES.get_or_init(|| {
        std::thread::Builder::new()
            .name("wasm-epoch-ticker".into())
            .spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_millis(EPOCH_TICK_MS));
                if let Some(registry) = EPOCH_ENGINES.get() {
                    if let Ok(engines) = registry.lock() {
                        for engine in engines.iter() {
                            engine.increment_epoch();
                        }
                    }
                }
            })
            .expect("failed to spawn wasm epoch ticker thread");
        std::sync::Mutex::new(Vec::new())
    });
    registry
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(engine);
}

/// Builds a fueled, epoch-interrupted engine. Infallible construction paths
/// must go through [`WasmRuntime::new`], which surfaces engine errors instead
/// of silently swapping in a weaker default.
pub fn fueled_engine() -> anyhow::Result<Engine> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine = Engine::new(&config)?;
    register_epoch_engine(engine.clone());
    Ok(engine)
}

/// Shared guest resource limits: memory ceiling plus table-growth cap.
/// Used directly as store state (`Store<WasmGuestLimits>`) or embedded in
/// larger store-data structs.
#[derive(Clone)]
pub struct WasmGuestLimits {
    pub memory_limit_bytes: usize,
}

impl Default for WasmGuestLimits {
    fn default() -> Self {
        Self {
            memory_limit_bytes: WASM_MEMORY_LIMIT_BYTES,
        }
    }
}

impl ResourceLimiter for WasmGuestLimits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        Ok(desired <= self.memory_limit_bytes)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        Ok(desired <= WASM_MAX_TABLE_ELEMENTS)
    }
}

pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    /// Explicit, fallible construction. There is deliberately no `Default`
    /// impl: an engine without fuel metering would silently remove the
    /// infinite-loop/OOM defenses from every module executed through it.
    pub fn new() -> anyhow::Result<Self> {
        let engine = fueled_engine()?;
        Ok(Self { engine })
    }

    pub fn load_module(&self, bytes: &[u8]) -> anyhow::Result<WasmModule> {
        let module = Module::new(&self.engine, bytes)?;
        Ok(WasmModule { module })
    }

    fn engine(&self) -> &Engine {
        &self.engine
    }
}

pub struct WasmModule {
    module: Module,
}

impl WasmModule {
    pub fn instantiate(&self, runtime: &WasmRuntime) -> anyhow::Result<WasmInstance> {
        let linker = Linker::new(runtime.engine());
        let mut store = Store::new(runtime.engine(), WasmGuestLimits::default());
        store.limiter(|limits: &mut WasmGuestLimits| limits as &mut dyn ResourceLimiter);
        store.set_fuel(WASM_FUEL_PER_INSTANTIATION)?;
        // Epoch-interrupted engines trap stores whose deadline is unset
        // (default 0 == already elapsed). Fuel is the primary CPU bound;
        // give this generic path a generous wall-clock ceiling (~10 min at
        // the global 20ms tick) purely as a hang guard.
        store.set_epoch_deadline(RUNTIME_EPOCH_DEADLINE_TICKS);
        let instance = linker.instantiate(&mut store, &self.module)?;
        Ok(WasmInstance { instance, store })
    }
}

/// Wall-clock hang-guard for generic runtime instances (see instantiate):
/// 30_000 ticks x 20ms tick = ~10 minutes; fuel still bounds CPU first.
const RUNTIME_EPOCH_DEADLINE_TICKS: u64 = 30_000;

pub struct WasmInstance {
    instance: Instance,
    store: Store<WasmGuestLimits>,
}

impl WasmInstance {
    pub fn call_func(&mut self, name: &str, params: &[Val]) -> anyhow::Result<Vec<Val>> {
        let func = self
            .instance
            .get_func(&mut self.store, name)
            .ok_or_else(|| anyhow::anyhow!("exported function '{}' not found", name))?;

        let ty = func.ty(&self.store);
        let results_count = ty.results().len();
        let mut results = vec![Val::I32(0); results_count];
        func.call(&mut self.store, params, &mut results)?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_call_add() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let mut instance = module.instantiate(&runtime).unwrap();

        let result = instance
            .call_func("add", &[Val::I32(2), Val::I32(3)])
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].i32(), Some(5));
    }

    #[test]
    fn test_fuel_metering() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (func (export "loop_forever") (result i32)
                    (loop (result i32)
                        br 0
                    )
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let linker = wasmtime::Linker::new(runtime.engine());
        let mut store = wasmtime::Store::new(runtime.engine(), ());
        store.set_fuel(10).unwrap();
        let instance = linker.instantiate(&mut store, &module.module).unwrap();
        let func = instance.get_func(&mut store, "loop_forever").unwrap();
        let mut results = [wasmtime::Val::I32(0)];
        let result = func.call(&mut store, &[], &mut results);
        assert!(
            result.is_err(),
            "expected trap on infinite loop with limited fuel"
        );
    }

    #[test]
    fn test_load_invalid_wasm() {
        let runtime = WasmRuntime::new().unwrap();
        let invalid_bytes = b"not a valid wasm module\x00\x01\x02";
        let result = runtime.load_module(invalid_bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_module_empty_bytes_fails() {
        let runtime = WasmRuntime::new().unwrap();
        let result = runtime.load_module(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fails_on_unresolved_import() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (import "env" "missing_func" (func))
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();

        let result = module.instantiate(&runtime);
        assert!(
            result.is_err(),
            "instantiate must fail for unresolved imports"
        );
    }

    #[test]
    fn test_call_func_missing_export_fails() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let mut instance = module.instantiate(&runtime).unwrap();

        let result = instance.call_func("nonexistent", &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_call_func_wrong_signature_fails() {
        let runtime = WasmRuntime::new().unwrap();
        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let mut instance = module.instantiate(&runtime).unwrap();

        let result = instance.call_func("add", &[Val::I32(1)]);
        assert!(result.is_err(), "calling with wrong arity must fail");
    }

    #[test]
    fn test_table_growth_is_capped() {
        let runtime = WasmRuntime::new().unwrap();
        // Grow the table past WASM_MAX_TABLE_ELEMENTS: table.grow returns -1
        // when the limiter rejects.
        let wat = r#"
            (module
                (table $t 1 funcref)
                (func (export "grow_small") (result i32)
                    (table.grow $t (ref.null func) (i32.const 5))
                )
                (func (export "grow_huge") (result i32)
                    (table.grow $t (ref.null func) (i32.const 20000))
                )
            )
        "#;
        let module = runtime.load_module(wat.as_bytes()).unwrap();
        let mut instance = module.instantiate(&runtime).unwrap();

        let ok = instance.call_func("grow_small", &[]).unwrap();
        assert_eq!(ok[0].i32(), Some(1), "small growth within cap succeeds");

        let rejected = instance.call_func("grow_huge", &[]).unwrap();
        assert_eq!(
            rejected[0].i32(),
            Some(-1),
            "growth beyond the table cap must be rejected by the limiter"
        );
    }

    #[test]
    fn test_guest_limits_reject_memory_over_ceiling() {
        use wasmtime::ResourceLimiter as _;
        let mut limits = WasmGuestLimits::default();
        assert!(limits
            .memory_growing(0, WASM_MEMORY_LIMIT_BYTES, None)
            .unwrap());
        assert!(!limits
            .memory_growing(0, WASM_MEMORY_LIMIT_BYTES + 1, None)
            .unwrap());
        assert!(limits
            .table_growing(0, WASM_MAX_TABLE_ELEMENTS, None)
            .unwrap());
        assert!(!limits
            .table_growing(0, WASM_MAX_TABLE_ELEMENTS + 1, None)
            .unwrap());
    }
}
