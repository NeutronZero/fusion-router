use crate::release::gate::GateError;
use fusion_plugin_api::Permission;

pub fn check_secret_access(permissions: &[Permission], secret_name: &str) -> Result<(), GateError> {
    for perm in permissions {
        match perm {
            Permission::Secrets(pattern) => {
                if glob_match(pattern, secret_name) {
                    return Ok(());
                }
            }
            _ => continue,
        }
    }
    Err(GateError::PermissionDenied(format!(
        "secret '{}' is not in the declared permission set",
        secret_name
    )))
}

pub fn check_http_access(permissions: &[Permission], url: &str) -> Result<(), GateError> {
    for perm in permissions {
        match perm {
            Permission::Http(pattern) => {
                if glob_match(pattern, url) {
                    return Ok(());
                }
            }
            _ => continue,
        }
    }
    Err(GateError::PermissionDenied(format!(
        "URL '{}' is not in the declared permission set",
        url
    )))
}

/// Single, fail-closed gate for reading environment variables. Every live env
/// access (provider API-key interpolation via `{env:VAR}`, the `api_key_env`
/// config field, and any plugin path) must pass through this function.
///
/// Rules, in order:
/// 1. Reject match-all / empty names (`""` or `"*"`).
/// 2. Denylist: never allow the router's own infrastructure secrets to be read
///    back out as a provider credential. This includes `FUSION_MASTER_KEY`
///    explicitly, plus any `FUSION_*` var shaped like a key/secret/password/token.
/// 3. Allowlist: only names shaped like a key/token/secret/password may be read
///    via interpolation. Anything else is rejected (fail-closed).
///
/// Note: `Permission::Environment` (in `fusion_plugin_api`) independently
/// rejects `""` and `"*"` at validation time, so a plugin can never be granted
/// access to a variable that this gate denies.
pub fn check_environment(var_name: &str) -> Result<(), GateError> {
    let var = var_name.trim();

    // 1. Reject match-all / empty names.
    if var.is_empty() || var == "*" {
        return Err(GateError::PermissionDenied(
            "environment variable name must not be empty or '*'".into(),
        ));
    }

    let upper = var.to_ascii_uppercase();

    // 2. Denylist: explicit router infrastructure secrets.
    const DENY_EXACT: &[&str] = &["FUSION_MASTER_KEY"];
    if DENY_EXACT.contains(&upper.as_str()) {
        return Err(GateError::PermissionDenied(format!(
            "reading environment variable '{}' is denied: router infrastructure secret",
            var
        )));
    }

    // 2. Denylist: any router-owned key/secret/password/token.
    if upper.starts_with("FUSION_")
        && (upper.ends_with("_KEY")
            || upper.ends_with("_SECRET")
            || upper.ends_with("_PASSWORD")
            || upper.ends_with("_TOKEN"))
    {
        return Err(GateError::PermissionDenied(format!(
            "reading environment variable '{}' is denied: router infrastructure secret",
            var
        )));
    }

    // 3. Allowlist: only key/token/secret/password-shaped names may be read.
    let shape_ok = upper.ends_with("_KEY")
        || upper.ends_with("_TOKEN")
        || upper.ends_with("_SECRET")
        || upper.ends_with("_PASSWORD");
    if !shape_ok {
        return Err(GateError::PermissionDenied(format!(
            "environment variable '{}' is not an allowed key/token/secret/password name",
            var
        )));
    }

    Ok(())
}

fn glob_match(pattern: &str, candidate: &str) -> bool {
    if pattern == candidate {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return candidate.starts_with(prefix);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::Permission;

    #[test]
    fn test_secret_exact_match_allowed() {
        let perms = vec![Permission::Secrets("db_password".into())];
        assert!(check_secret_access(&perms, "db_password").is_ok());
    }

    #[test]
    fn test_secret_glob_match_allowed() {
        let perms = vec![Permission::Secrets("db_*".into())];
        assert!(check_secret_access(&perms, "db_password").is_ok());
    }

    #[test]
    fn test_secret_no_match_denied() {
        let perms = vec![Permission::Secrets("api_key".into())];
        let result = check_secret_access(&perms, "db_password");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_http_exact_match_allowed() {
        let perms = vec![Permission::Http("https://api.example.com/v1".into())];
        assert!(check_http_access(&perms, "https://api.example.com/v1").is_ok());
    }

    #[test]
    fn test_http_glob_match_allowed() {
        let perms = vec![Permission::Http("https://api.example.com/*".into())];
        assert!(check_http_access(&perms, "https://api.example.com/v1/users").is_ok());
    }

    #[test]
    fn test_http_no_match_denied() {
        let perms = vec![Permission::Http("https://allowed.com/*".into())];
        let result = check_http_access(&perms, "https://evil.com/steal");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_secret_denied_when_only_http_permissions() {
        let perms = vec![Permission::Http("https://example.com/*".into())];
        let result = check_secret_access(&perms, "db_password");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_http_denied_when_only_secret_permissions() {
        let perms = vec![Permission::Secrets("db_*".into())];
        let result = check_http_access(&perms, "https://example.com/api");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_empty_permissions_deny_secret_access() {
        let result = check_secret_access(&[], "db_password");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_empty_permissions_deny_http_access() {
        let result = check_http_access(&[], "https://example.com/api");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_secret_glob_prefix_denied() {
        let perms = vec![Permission::Secrets("db_*".into())];
        let result = check_secret_access(&perms, "api_other_key");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_key_shaped_allowed() {
        assert!(check_environment("OPENAI_API_KEY").is_ok());
    }

    #[test]
    fn test_environment_password_shaped_allowed() {
        assert!(check_environment("DB_PASSWORD").is_ok());
    }

    #[test]
    fn test_environment_non_key_shape_denied() {
        let result = check_environment("HOME");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_wildcard_denied() {
        let result = check_environment("*");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_empty_denied() {
        let result = check_environment("");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_fusion_master_key_denied() {
        let result = check_environment("FUSION_MASTER_KEY");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_fusion_secret_denied() {
        let result = check_environment("FUSION_DB_SECRET");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }
}
