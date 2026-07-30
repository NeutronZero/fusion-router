//! Runtime permission helpers for policy evaluation and convenience.
//!
//! The `Permission` type itself lives in `fusion-plugin-api` (the ABI crate).
//! This module provides runtime-specific utilities.

// Imported here for runtime helpers; currently used only in tests.
#[allow(unused_imports)]
use fusion_plugin_api::{Permission, PermissionError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_display_round_trips() {
        let cases = vec![
            Permission::Network,
            Permission::Filesystem("/data".into()),
            Permission::Http("https://example.com".into()),
            Permission::Secrets("API_KEY".into()),
            Permission::Environment("HOME".into()),
        ];
        for p in cases {
            let s = p.to_string();
            let back: Permission = s.parse().unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn validate_allows_valid() {
        assert!(Permission::Network.validate().is_ok());
        assert!(Permission::Filesystem("/tmp".into()).validate().is_ok());
        assert!(Permission::Http("https://example.com".into()).validate().is_ok());
        assert!(Permission::Secrets("API_KEY".into()).validate().is_ok());
        assert!(Permission::Environment("HOME".into()).validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(Permission::Filesystem("".into()).validate().is_err());
        assert!(Permission::Http("".into()).validate().is_err());
        assert!(Permission::Secrets("".into()).validate().is_err());
        assert!(Permission::Environment("".into()).validate().is_err());
    }
}
