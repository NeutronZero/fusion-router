use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, StrategyIR};
use crate::plugin::PluginRegistry;
use crate::strategies::{Parallelism, Strategy, StrategyDescriptor, StreamingMode};
use crate::types::{RetryPolicy, StrategyKind};
use crate::wasm::runtime::{
    fueled_engine, WasmGuestLimits, WASM_EPOCH_DEADLINE_TICKS, WASM_FUEL_PER_INSTANTIATION,
};

const EXPORT_MEMORY: &str = "memory";
const EXPORT_NAME: &str = "fusion_strategy_name";
const EXPORT_DESCRIPTOR: &str = "fusion_strategy_descriptor";
const EXPORT_LOWER: &str = "fusion_strategy_lower";
const EXPORT_ALLOC: &str = "alloc";

#[derive(Serialize, Deserialize)]
struct WasmLowerInput {
    ir: StrategyIR,
    ctx: CompilationContext,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum WasmLowerOutput {
    Ok { primitives: PrimitiveGraph },
    Error { error: String },
}

pub struct WasmStrategy {
    name: String,
    engine: Engine,
    module: Module,
    descriptor: Mutex<Option<StrategyDescriptor>>,
}

impl WasmStrategy {
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let engine = fueled_engine()?;
        let module = Module::from_file(&engine, path.as_ref())?;
        validate_exports(&engine, &module)?;
        let name = read_name(&engine, &module)?;
        Ok(Self {
            name,
            engine,
            module,
            descriptor: Mutex::new(None),
        })
    }

    /// Builds a strategy from raw wasm bytes. The module always runs on a
    /// freshly built fueled/epoch-interrupted engine: accepting a caller's
    /// arbitrary `Engine` previously allowed unfueled (unbounded) execution.
    pub fn from_binary(bytes: &[u8]) -> anyhow::Result<Self> {
        let engine = fueled_engine()?;
        let module = Module::new(&engine, bytes)?;
        validate_exports(&engine, &module)?;
        let name = read_name(&engine, &module)?;
        Ok(Self {
            name,
            engine,
            module,
            descriptor: Mutex::new(None),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn descriptor_inner(&self) -> anyhow::Result<StrategyDescriptor> {
        let (mut store, instance) = instantiate(&self.engine, &self.module)?;
        let memory = instance
            .get_memory(&mut store, EXPORT_MEMORY)
            .ok_or_else(|| anyhow::anyhow!("missing export: memory"))?;
        let func = instance.get_typed_func::<(), i32>(&mut store, EXPORT_DESCRIPTOR)?;
        let ptr = func.call(&mut store, ())?;
        let json_str = read_string(&store, &memory, ptr)?;
        let descriptor: StrategyDescriptor = serde_json::from_str(&json_str)?;
        Ok(descriptor)
    }

    fn lower_inner(
        &self,
        ir: &StrategyIR,
        ctx: &CompilationContext,
    ) -> anyhow::Result<PrimitiveGraph> {
        let (mut store, instance) = instantiate(&self.engine, &self.module)?;
        let memory = instance
            .get_memory(&mut store, EXPORT_MEMORY)
            .ok_or_else(|| anyhow::anyhow!("missing export: memory"))?;

        let input = WasmLowerInput {
            ir: ir.clone(),
            ctx: ctx.clone(),
        };
        let input_json = serde_json::to_string(&input)?;

        let alloc: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, EXPORT_ALLOC)
            .map_err(|_| anyhow::anyhow!("missing export: alloc"))?;
        let lower: TypedFunc<(i32, i32), i32> =
            instance.get_typed_func(&mut store, EXPORT_LOWER)?;

        let len = input_json.len() as i32;
        let ptr = alloc.call(&mut store, len)?;

        memory.write(&mut store, ptr as usize, input_json.as_bytes())?;

        let result_ptr = lower.call(&mut store, (ptr, len))?;
        let result_str = read_string(&store, &memory, result_ptr)?;

        let output: WasmLowerOutput = serde_json::from_str(&result_str)?;
        match output {
            WasmLowerOutput::Ok { primitives } => Ok(primitives),
            WasmLowerOutput::Error { error } => anyhow::bail!("WASM strategy error: {}", error),
        }
    }
}

impl Strategy for WasmStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        {
            let cached = self.descriptor.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref desc) = *cached {
                return desc.clone();
            }
        }

        let name = self.name.clone();
        let desc = match self.descriptor_inner() {
            Ok(d) => StrategyDescriptor { name, ..d },
            Err(_) => StrategyDescriptor {
                name,
                parallelism: Parallelism::Sequential,
                requires_barrier: false,
                supports_streaming: StreamingMode::None,
                retry_policy: RetryPolicy {
                    max_retries: 0,
                    backoff_ms: 0,
                },
                expected_outputs: vec![],
            },
        };

        *self.descriptor.lock().unwrap_or_else(|e| e.into_inner()) = Some(desc.clone());
        desc
    }

    fn lower(
        &self,
        ir: &StrategyIR,
        ctx: &CompilationContext,
    ) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        // Map trap-class failures (fuel exhaustion, epoch timeout) onto
        // typed messages before they become compiler diagnostics.
        self.lower_inner(ir, ctx)
            .map_err(map_guest_error)
            .map_err(|e| {
                CompilerDiagnostic::error(
                    "WASM_STRATEGY_LOWER",
                    format!("WASM lower failed: {}", e),
                )
            })
    }
}

fn validate_exports(_engine: &Engine, module: &Module) -> anyhow::Result<()> {
    for name in &[EXPORT_MEMORY, EXPORT_NAME, EXPORT_DESCRIPTOR, EXPORT_LOWER] {
        if module.get_export(name).is_none() {
            anyhow::bail!("WASM strategy module missing required export: {}", name);
        }
    }
    Ok(())
}

fn instantiate(
    engine: &Engine,
    module: &Module,
) -> anyhow::Result<(Store<WasmGuestLimits>, Instance)> {
    let linker = Linker::new(engine);
    let mut store = Store::new(engine, WasmGuestLimits::default());
    // Resource ceiling: memory growth capped, table growth capped (DoS).
    store.limiter(|limits: &mut WasmGuestLimits| limits as &mut dyn wasmtime::ResourceLimiter);
    // CPU ceiling: fuel metering turns infinite loops into OutOfFuel traps.
    store.set_fuel(WASM_FUEL_PER_INSTANTIATION)?;
    // Wall-clock ceiling: the engine is epoch-interrupted (see fueled_engine)
    // and this store traps when its epoch budget elapses.
    store.set_epoch_deadline(WASM_EPOCH_DEADLINE_TICKS);
    let instance = linker.instantiate(&mut store, module)?;
    Ok((store, instance))
}

/// Maps a guest failure onto a typed, actionable error message: fuel
/// exhaustion and epoch (wall-clock) interruption are called out explicitly
/// instead of surfacing as opaque traps.
fn map_guest_error(err: anyhow::Error) -> anyhow::Error {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
        return match trap {
            wasmtime::Trap::OutOfFuel => anyhow::anyhow!(
                "WASM strategy exceeded its fuel budget ({WASM_FUEL_PER_INSTANTIATION})"
            ),
            wasmtime::Trap::Interrupt => anyhow::anyhow!(
                "WASM strategy execution timed out (epoch deadline of {WASM_EPOCH_DEADLINE_TICKS} ticks)"
            ),
            other => anyhow::anyhow!("WASM strategy trapped: {other}"),
        };
    }
    err
}

fn read_name(engine: &Engine, module: &Module) -> anyhow::Result<String> {
    let (mut store, instance) = instantiate(engine, module)?;
    let memory = instance
        .get_memory(&mut store, EXPORT_MEMORY)
        .ok_or_else(|| anyhow::anyhow!("missing export: memory"))?;
    let func = instance.get_typed_func::<(), i32>(&mut store, EXPORT_NAME)?;
    let ptr = func.call(&mut store, ())?;
    read_string(&store, &memory, ptr)
}

fn read_string<T>(store: &Store<T>, memory: &Memory, ptr: i32) -> anyhow::Result<String> {
    let data = memory.data(store);
    let start = ptr as usize;
    if start >= data.len() {
        return Err(anyhow::anyhow!(
            "read_string: pointer {ptr} out of bounds (memory len {})",
            data.len()
        ));
    }
    let mut end = start;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    let bytes = &data[start..end];
    Ok(String::from_utf8(bytes.to_vec())?)
}

pub fn load_and_register_wasm_strategy(
    registry: &mut PluginRegistry,
    path: &Path,
    kind: Option<StrategyKind>,
) -> anyhow::Result<()> {
    let strategy = WasmStrategy::from_file(path)?;
    let name = strategy.name().to_string();
    let strategy_kind = kind.unwrap_or(StrategyKind::Custom(name));
    registry.register_strategy(strategy_kind, Box::new(strategy));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::runtime::fueled_engine;
    use wasmtime::MemoryType;

    #[test]
    fn test_validate_exports_rejects_module_without_strategy_exports() {
        let engine = fueled_engine().unwrap();
        let wat = r#"(module (func (export "dummy") (result i32) i32.const 42))"#;
        let module = Module::new(&engine, wat).unwrap();
        let result = validate_exports(&engine, &module);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing required export"));
    }

    #[test]
    fn test_read_name_from_module() {
        let engine = fueled_engine().unwrap();
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "fusion_strategy_name") (result i32)
                    i32.const 0
                )
                (func (export "fusion_strategy_descriptor") (result i32)
                    i32.const 0
                )
                (func (export "fusion_strategy_lower") (param i32 i32) (result i32)
                    i32.const 0
                )
                (data (i32.const 0) "test-strategy\00")
            )
        "#;
        let module = Module::new(&engine, wat).unwrap();
        let name = read_name(&engine, &module).unwrap();
        assert_eq!(name, "test-strategy");
    }

    #[test]
    fn test_from_file_rejects_file_not_found() {
        let result = WasmStrategy::from_file("nonexistent.wasm");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_string_rejects_out_of_bounds_pointer() {
        let engine = fueled_engine().unwrap();
        let mut store = Store::new(&engine, ());
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(1))).unwrap();

        let negative = read_string(&store, &memory, -1);
        assert!(negative.is_err(), "negative pointer must error, not panic");

        let past_end = read_string(&store, &memory, 65537);
        assert!(
            past_end.is_err(),
            "pointer past memory end must error, not panic"
        );
    }

    #[test]
    fn test_lower_fuel_exhaustion_maps_to_typed_error() {
        // fusion_strategy_lower loops forever: the fuel budget must turn the
        // infinite loop into a typed, human-readable failure instead of
        // hanging the compiler pass.
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) i32.const 0)
                (func (export "fusion_strategy_name") (result i32) i32.const 0)
                (func (export "fusion_strategy_descriptor") (result i32) i32.const 0)
                (func (export "fusion_strategy_lower") (param i32 i32) (result i32)
                    (loop (result i32) (br 0))
                )
            )
        "#;
        let strategy = WasmStrategy::from_binary(wat.as_bytes()).expect("module must load");
        let diag = strategy
            .lower(&StrategyIR::Single, &CompilationContext::new())
            .expect_err("infinite loop must fail");
        assert_eq!(diag.code, "WASM_STRATEGY_LOWER");
        assert!(
            diag.message.contains("fuel budget"),
            "fuel exhaustion must be named explicitly, got: {}",
            diag.message
        );
    }
}
