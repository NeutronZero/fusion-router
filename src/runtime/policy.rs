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

pub fn check_environment(
    permissions: &[Permission],
    var_name: &str,
) -> Result<(), GateError> {
    if var_name.is_empty() || var_name == "*" {
        return Err(GateError::PermissionDenied(
            "environment variable name must not be empty or '*'".into(),
        ));
    }
    for perm in permissions {
        match perm {
            Permission::Environment(pattern) => {
                if pattern == "*" {
                    continue;
                }
                if let Some(prefix) = pattern.strip_suffix('*') {
                    if var_name.starts_with(prefix) {
                        return Ok(());
                    }
                } else if pattern == var_name {
                    return Ok(());
                }
            }
            _ => continue,
        }
    }
    Err(GateError::PermissionDenied(format!(
        "environment variable '{}' is not in the declared permission set",
        var_name
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

    #[test]
    fn test_environment_exact_match_allowed() {
        let perms = vec![Permission::Environment("HOME".into())];
        assert!(check_environment(&perms, "HOME").is_ok());
    }

    #[test]
    fn test_environment_glob_match_allowed() {
        let perms = vec![Permission::Environment("DB_*".into())];
        assert!(check_environment(&perms, "DB_PASSWORD").is_ok());
    }

    #[test]
    fn test_environment_no_match_denied() {
        let perms = vec![Permission::Environment("HOME".into())];
        let result = check_environment(&perms, "PATH");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_wildcard_denied() {
        let perms = vec![Permission::Environment("*".into())];
        let result = check_environment(&perms, "HOME");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[test]
    fn test_environment_network_only_denied() {
        let perms = vec![Permission::Network];
        let result = check_environment(&perms, "HOME");
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }
}
