use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse::{Parse, ParseStream}, Token, LitStr, Path};

/// Represents a single `#[permission(...)]` attribute value.
/// Maps to the typed `Permission` enum in `fusion_plugin_api`.
#[derive(Debug, Clone)]
pub enum PermissionAttr {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
    Environment(String),
}

impl PermissionAttr {
    /// Emits a typed `::fusion_plugin_api::Permission::*` token stream.
    pub fn to_permission_token_stream(&self) -> TokenStream {
        match self {
            PermissionAttr::Network => quote! { ::fusion_plugin_api::Permission::Network },
            PermissionAttr::Filesystem(path) => {
                quote! { ::fusion_plugin_api::Permission::Filesystem(#path.into()) }
            }
            PermissionAttr::Http(endpoint) => {
                quote! { ::fusion_plugin_api::Permission::Http(#endpoint.into()) }
            }
            PermissionAttr::Secrets(name) => {
                quote! { ::fusion_plugin_api::Permission::Secrets(#name.into()) }
            }
            PermissionAttr::Environment(name) => {
                quote! { ::fusion_plugin_api::Permission::Environment(#name.into()) }
            }
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
            "Environment" => {
                let content;
                syn::parenthesized!(content in input);
                let name: LitStr = content.parse()?;
                Ok(PermissionAttr::Environment(name.value()))
            }
            _ => Err(syn::Error::new_spanned(&path, format!("unknown permission variant: {name}")))
        }
    }
}

/// Parses `#[permission(...)]` from struct attributes.
pub fn parse_permission_attrs(attrs: &[syn::Attribute]) -> Result<Vec<PermissionAttr>, syn::Error> {
    let mut permissions = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("permission")) {
        permissions.push(attr.parse_args::<PermissionAttr>()?);
    }
    Ok(permissions)
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

    #[test]
    fn parse_environment() {
        let attr: PermissionAttr = parse_quote!(Environment("HOME"));
        assert!(matches!(attr, PermissionAttr::Environment(p) if p == "HOME"));
    }
}
