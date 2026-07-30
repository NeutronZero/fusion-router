mod permission;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct};

struct CapabilityArgs {
    id: String,
    description: String,
    version: String,
}

impl syn::parse::Parse for CapabilityArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut id = None;
        let mut description = None;
        let mut version = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let value: syn::LitStr = input.parse()?;

            match key.to_string().as_str() {
                "id" => id = Some(value.value()),
                "description" => description = Some(value.value()),
                "version" => version = Some(value.value()),
                other => {
                    return Err(syn::Error::new_spanned(&key, format!("unknown capability attribute: {other}")));
                }
            }

            if !input.is_empty() {
                let _: syn::Token![,] = input.parse()?;
            }
        }

        Ok(CapabilityArgs {
            id: id.ok_or_else(|| input.error("missing required attribute: id"))?,
            description: description.ok_or_else(|| input.error("missing required attribute: description"))?,
            version: version.ok_or_else(|| input.error("missing required attribute: version"))?,
        })
    }
}

fn validate_semver(version: &str) -> Result<::semver::Version, String> {
    ::semver::Version::parse(version).map_err(|e| format!("invalid semver version '{version}': {e}"))
}

#[proc_macro_attribute]
pub fn capability(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as ItemStruct);
    let struct_name = &item_struct.ident;

    let args = match syn::parse::<CapabilityArgs>(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };

    let id = &args.id;
    let description = &args.description;
    let version = &args.version;

    if let Err(e) = validate_semver(version) {
        return syn::Error::new_spanned(&item_struct, e).to_compile_error().into();
    }

    let permissions = match permission::parse_permission_attrs(&item_struct.attrs) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };
    let permission_tokens: Vec<proc_macro2::TokenStream> = permissions.iter().map(|p| p.to_permission_token_stream()).collect();

    let expanded = quote! {
        #item_struct

        impl ::fusion_plugin_api::Plugin for #struct_name {
            fn metadata(&self) -> ::fusion_plugin_api::PluginMetadata {
                ::fusion_plugin_api::PluginMetadata {
                    name: #id.to_string(),
                    version: ::fusion_capability_sdk::__reexports::semver::Version::parse(#version).unwrap(),
                    api_version: ::fusion_capability_sdk::__reexports::semver::Version::parse(::fusion_plugin_api::CAPABILITY_ABI_VERSION).unwrap(),
                    min_compiler_version: ::fusion_capability_sdk::__reexports::semver::Version::parse("0.11.0").unwrap(),
                    capabilities: vec![::fusion_plugin_api::CapabilityId::new(#id)],
                }
            }
        }

        impl ::fusion_plugin_api::CapabilityPlugin for #struct_name {
            fn capabilities(&self) -> Vec<::fusion_plugin_api::CapabilityContract> {
                vec![
                    ::fusion_plugin_api::CapabilityContract {
                        id: ::fusion_plugin_api::CapabilityId::new(#id),
                        version: ::fusion_capability_sdk::__reexports::semver::Version::parse(#version).unwrap(),
                        description: #description.to_string(),
                        inputs_schema: ::fusion_capability_sdk::__reexports::serde_json::Value::Object(Default::default()),
                        outputs_schema: ::fusion_capability_sdk::__reexports::serde_json::Value::Object(Default::default()),
                        permissions: vec![#(#permission_tokens),*],
                        dependencies: vec![],
                        estimated_cost_usd: 0.0,
                        estimated_latency_ms: 0,
                        reliability_score: 1.0,
                        supports_streaming: false,
                    }
                ]
            }
        }
    };

    TokenStream::from(expanded)
}
