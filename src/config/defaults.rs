pub fn default_host() -> String { "127.0.0.1".to_string() }
pub fn default_port() -> u16 { 8080 }
pub fn default_shutdown_timeout() -> u64 { 30 }

pub fn default_cors_origins() -> Vec<String> { vec![] }
pub fn default_cors_methods() -> Vec<String> { vec!["GET".into(), "POST".into(), "PUT".into(), "DELETE".into(), "OPTIONS".into()] }
pub fn default_cors_headers() -> Vec<String> { vec!["content-type".into(), "authorization".into(), "x-api-key".into(), "x-request-id".into()] }

pub fn default_auth_enabled() -> bool { true }

pub fn default_rate_limiting_enabled() -> bool { true }
pub fn default_rpm() -> u64 { 60 }
pub fn default_burst() -> u32 { 10 }
pub fn default_cleanup_interval() -> u64 { 300 }

pub fn default_log_format() -> String { "text".into() }
pub fn default_log_level() -> String { "info".into() }

pub fn default_max_concurrent_nodes() -> u32 { 16 }
pub fn default_concurrent() -> u32 { 5 }

pub fn default_transport() -> String { "openai-chat".to_string() }

pub fn default_consensus_count() -> u32 { 3 }
pub fn default_failure_threshold() -> u32 { 5 }
pub fn default_cooldown_secs() -> u64 { 30 }

pub fn default_allowed_shell_commands() -> Vec<String> { vec![] }
pub fn default_shell_timeout_secs() -> u64 { 10 }
pub fn default_allowed_read_directories() -> Vec<String> { vec![".".into()] }
pub fn default_enable_http_tool() -> bool { false }
pub fn default_allow_auto_exec() -> bool { false }
pub fn default_allow_unrestricted_args() -> bool { false }
