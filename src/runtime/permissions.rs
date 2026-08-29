//! AD-002: Fine-grained WASM syscall permissions.
//! Coarse fuel/memory limits remain; this layer adds capability-scoped import
//! allowlisting so a guest can only call the host functions its declared
//! `CapabilityContract.permissions` grant.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WasmPermission {
    /// Allow `host.log` / `host.print`
    Log,
    /// Allow `host.http_request` (maps to `http_request` tool)
    Http,
    /// Allow `host.shell_exec` (maps to `shell_command` tool)
    Shell,
    /// Allow file read via `host.file_read`
    FileRead,
    /// Allow capability invocation via `host.capability_call`
    CapabilityCall,
    /// Custom permission string (forward-compatible)
    Custom(String),
}

impl WasmPermission {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Log => "log",
            Self::Http => "http",
            Self::Shell => "shell",
            Self::FileRead => "file_read",
            Self::CapabilityCall => "capability_call",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "log" => Self::Log,
            "http" => Self::Http,
            "shell" => Self::Shell,
            "file_read" => Self::FileRead,
            "capability_call" => Self::CapabilityCall,
            other => Self::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WasmPermissions {
    /// Set of allowed host imports. Empty = deny-all (fail-closed).
    /// `{"*": true}` style wildcard is deliberately unsupported; every import
    /// must be explicitly granted.
    pub allowed: HashSet<String>,

    /// Whether to allow all `host.*` imports (coarse mode, legacy).
    /// When true, fine-grained checks are bypassed and only fuel/memory limits apply.
    #[serde(default)]
    pub allow_all_host: bool,
}

impl WasmPermissions {
    pub fn deny_all() -> Self {
        Self {
            allowed: HashSet::new(),
            allow_all_host: false,
        }
    }

    pub fn allow_all() -> Self {
        Self {
            allowed: HashSet::new(),
            allow_all_host: true,
        }
    }

    pub fn from_permissions(perms: &[String]) -> Self {
        let mut allowed = HashSet::new();
        for p in perms {
            // Each `CapabilityContract.permission` maps 1:1 to a host import name.
            // Example: `shell.exec` → `host.shell_exec`, `http.request` → `host.http_request`
            let host_import = match p.as_str() {
                "shell.exec" => "host.shell_exec",
                "http.request" => "host.http_request",
                "file.read" => "host.file_read",
                "log.write" => "host.log",
                other => other,
            };
            allowed.insert(host_import.to_string());
            allowed.insert(p.clone());
        }
        // Always allow logging — least-privilege but not silent
        allowed.insert("host.log".into());
        allowed.insert("log".into());
        Self {
            allowed,
            allow_all_host: false,
        }
    }

    pub fn is_allowed(&self, import: &str) -> bool {
        if self.allow_all_host {
            return true;
        }
        self.allowed.contains(import)
    }

    pub fn check_import(&self, module: &str, field: &str) -> Result<(), String> {
        let fq = format!("{module}.{field}");
        if self.is_allowed(&fq) || self.is_allowed(field) {
            Ok(())
        } else {
            Err(format!(
                "WASM import '{fq}' denied by fine-grained permission policy; allowed: {:?}",
                self.allowed
            ))
        }
    }
}

/// Registry mapping capability IDs to their required WASM permissions.
// Used at scheduling time to build a least-privilege `WasmPermissions` per node.
#[derive(Debug, Default)]
pub struct PermissionRegistry {
    map: HashMap<String, WasmPermissions>,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, capability_id: &str, perms: WasmPermissions) {
        self.map.insert(capability_id.to_string(), perms);
    }

    pub fn for_capability(&self, cap_id: &str) -> WasmPermissions {
        self.map
            .get(cap_id)
            .cloned()
            .unwrap_or_else(WasmPermissions::deny_all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_all_rejects_everything() {
        let perms = WasmPermissions::deny_all();
        assert!(perms.check_import("host", "http_request").is_err());
        assert!(perms.check_import("host", "log").is_err());
    }

    #[test]
    fn allow_all_permits_everything() {
        let perms = WasmPermissions::allow_all();
        assert!(perms.check_import("host", "http_request").is_ok());
        assert!(perms.check_import("env", "anything").is_ok());
    }

    #[test]
    fn from_permissions_maps_shell_and_http() {
        let perms = WasmPermissions::from_permissions(&["shell.exec".into(), "http.request".into()]);
        assert!(perms.is_allowed("host.shell_exec"));
        assert!(perms.is_allowed("host.http_request"));
        assert!(perms.is_allowed("host.log"));
        assert!(!perms.is_allowed("host.file_read"));
    }

    #[test]
    fn custom_permission_preserved() {
        let perms = WasmPermissions::from_permissions(&["custom.cap".into()]);
        assert!(perms.is_allowed("custom.cap"));
    }
}
