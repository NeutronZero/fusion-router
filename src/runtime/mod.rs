pub mod config;
pub mod context;
pub mod host_services;
pub mod policy;
pub mod sandbox_instance;
pub mod sandbox_runtime;
pub mod telemetry_context;

#[cfg(feature = "wasm-plugins")]
pub mod linker;
#[cfg(feature = "wasm-plugins")]
pub mod module_cache;
#[cfg(feature = "wasm-plugins")]
pub mod wasmtime_host;
#[cfg(feature = "wasm-plugins")]
pub mod wasmtime_runtime;

use std::fmt;

#[derive(Debug)]
pub enum RuntimeError {
    CompilationFailed(String),
    OutOfMemory,
    FuelExhausted,
    ExecutionTrap { message: String },
    HostServiceError { service: String, inner: String },
    NotSupported(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::CompilationFailed(msg) => write!(f, "WASM compilation failed: {msg}"),
            RuntimeError::OutOfMemory => write!(f, "out of memory"),
            RuntimeError::FuelExhausted => write!(f, "fuel exhausted"),
            RuntimeError::ExecutionTrap { message } => write!(f, "execution trap: {message}"),
            RuntimeError::HostServiceError { service, inner } => {
                write!(f, "host service error: service={service}, inner={inner}")
            }
            RuntimeError::NotSupported(msg) => write!(f, "not supported: {msg}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

pub use config::SandboxConfig;
pub use context::RuntimeContext;
pub use host_services::CapabilityHostServices;
pub use policy::{check_http_access, check_secret_access};
pub use sandbox_instance::SandboxInstance;
pub use sandbox_runtime::SandboxRuntime;
pub use telemetry_context::TelemetryContext;
pub use fusion_runtime::RuntimeEngine;

#[cfg(feature = "wasm-plugins")]
pub use linker::configure_linker;
#[cfg(feature = "wasm-plugins")]
pub use module_cache::RuntimeModuleCache;
#[cfg(feature = "wasm-plugins")]
pub use wasmtime_host::WasmtimeCapabilityHost;
#[cfg(feature = "wasm-plugins")]
pub use wasmtime_runtime::WasmtimeSandboxRuntime;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn runtime_error_display() {
        let err = RuntimeError::CompilationFailed("bad wasm".into());
        assert_eq!(err.to_string(), "WASM compilation failed: bad wasm");
        let err = RuntimeError::OutOfMemory;
        assert_eq!(err.to_string(), "out of memory");
        let err = RuntimeError::FuelExhausted;
        assert_eq!(err.to_string(), "fuel exhausted");
        let err = RuntimeError::ExecutionTrap { message: "segfault".into() };
        assert_eq!(err.to_string(), "execution trap: segfault");
        let err = RuntimeError::HostServiceError { service: "secret".into(), inner: "denied".into() };
        assert_eq!(err.to_string(), "host service error: service=secret, inner=denied");
        let err = RuntimeError::NotSupported("reset".into());
        assert_eq!(err.to_string(), "not supported: reset");
    }

    #[test]
    fn runtime_context_construction() {
        let _ = RuntimeError::OutOfMemory;
    }

    #[test]
    fn runtime_error_is_debug() {
        let err = RuntimeError::FuelExhausted;
        assert!(format!("{:?}", err).contains("FuelExhausted"));
    }

    #[test]
    fn capabiltiy_host_services_is_object_safe() {
        fn _assert_object_safe(_: Arc<dyn CapabilityHostServices>) {}
    }

    #[test]
    fn sandbox_runtime_trait_object_safe() {
        fn _take(_rt: Box<dyn SandboxRuntime>) {}
    }

    #[test]
    fn sandbox_instance_trait_object_safe() {
        fn _take(_inst: Box<dyn SandboxInstance>) {}
    }

    #[test]
    fn sandbox_runtime_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Box<dyn SandboxRuntime>>();
    }
}
