use syn::{parse::{Parse, ParseStream}, Token, LitStr, Path};

/// Represents a single `#[permission(...)]` attribute value.
/// Maps to the typed `Permission` enum planned for Sprint O2.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PermissionAttr {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
}

#[allow(dead_code)]
impl PermissionAttr {
    /// Returns the permission variant name (e.g. "Network", "Filesystem").
    pub fn variant_name(&self) -> &'static str {
        match self {
            PermissionAttr::Network => "Network",
            PermissionAttr::Filesystem(_) => "Filesystem",
            PermissionAttr::Http(_) => "Http",
            PermissionAttr::Secrets(_) => "Secrets",
        }
    }

    /// Returns the string representation used in `CapabilityContract.permissions`.
    /// Single-arg variants include the value: `"Http(https://...)"`.
    pub fn to_permission_string(&self) -> String {
        match self {
            PermissionAttr::Network => "Network".to_string(),
            PermissionAttr::Filesystem(path) => format!("Filesystem({path})"),
            PermissionAttr::Http(url) => format!("Http({url})"),
            PermissionAttr::Secrets(name) => format!("Secrets({name})"),
        }
    }
}

impl Parse for PermissionAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: Path = input.parse()?;
        let ident = path.get_ident()
            .ok_or_else(|| input.error("expected identifier"))?;
        let name = ident.to_string();

        match name.as_str() {
            "Network" => {
                if input.peek(Token![,]) || input.is_empty() {
                    Ok(PermissionAttr::Network)
                } else {
                    Err(input.error("Network takes no arguments"))
                }
            }
            "Filesystem" => {
                let content;
                syn::parenthesized!(content in input);
                let path: LitStr = content.parse()?;
                Ok(PermissionAttr::Filesystem(path.value()))
            }
            "Http" => {
                let content;
                syn::parenthesized!(content in input);
                let url: LitStr = content.parse()?;
                Ok(PermissionAttr::Http(url.value()))
            }
            "Secrets" => {
                let content;
                syn::parenthesized!(content in input);
                let name: LitStr = content.parse()?;
                Ok(PermissionAttr::Secrets(name.value()))
            }
            _ => Err(syn::Error::new_spanned(&path, format!("unknown permission variant: {name}")))
        }
    }
}

/// Parses `#[permission(...)]` from struct attributes.
#[allow(dead_code)]
pub fn parse_permission_attrs(attrs: &[syn::Attribute]) -> Vec<PermissionAttr> {
    attrs.iter()
        .filter(|a| a.path().is_ident("permission"))
        .filter_map(|a| a.parse_args::<PermissionAttr>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parse_network() {
        let attr: PermissionAttr = parse_quote!(Network);
        assert!(matches!(attr, PermissionAttr::Network));
    }

    #[test]
    fn parse_filesystem() {
        let attr: PermissionAttr = parse_quote!(Filesystem("/tmp"));
        assert!(matches!(attr, PermissionAttr::Filesystem(p) if p == "/tmp"));
    }

    #[test]
    fn parse_http() {
        let attr: PermissionAttr = parse_quote!(Http("https://api.example.com"));
        assert!(matches!(attr, PermissionAttr::Http(u) if u == "https://api.example.com"));
    }

    #[test]
    fn parse_secrets() {
        let attr: PermissionAttr = parse_quote!(Secrets("OPENAI_API_KEY"));
        assert!(matches!(attr, PermissionAttr::Secrets(k) if k == "OPENAI_API_KEY"));
    }
}
