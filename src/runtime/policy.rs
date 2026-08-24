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
}
