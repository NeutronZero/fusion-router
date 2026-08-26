pub(crate) mod runtime;

#[allow(unused_imports)]
pub use runtime::{WasmInstance, WasmModule, WasmRuntime};

#[allow(unused_imports)]
pub(crate) use runtime::{
    fueled_engine, WasmGuestLimits, WASM_EPOCH_DEADLINE_TICKS, WASM_FUEL_PER_INSTANTIATION,
    WASM_MAX_TABLE_ELEMENTS,
};
