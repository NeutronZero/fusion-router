#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub fuel_amount: u64,
    pub memory_limit_bytes: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            fuel_amount: 1_000_000,
            memory_limit_bytes: 10 * 1024 * 1024,
        }
    }
}
