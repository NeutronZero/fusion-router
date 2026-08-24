//! Phase 4A1 — `PolicyAST` & `PolicyParser` (`src/policy/ast.rs`)
//!
//! Abstract Syntax Tree representing declarative user-facing policy definitions.

use crate::policy::diagnostics::PolicyDiagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDeclaration {
    pub name: String,
    #[serde(default)]
    pub priority: u32,
    pub match_target: String,
    pub effect: String, // "deny", "approval", "allow"
    #[serde(default)]
    pub conditions: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub annotations: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAST {
    pub version: String,
    pub declarations: Vec<PolicyDeclaration>,
}

pub struct PolicyParser;

impl PolicyParser {
    pub fn parse_json(json_str: &str) -> Result<(PolicyAST, Vec<PolicyDiagnostic>), String> {
        let ast: PolicyAST = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse Policy AST JSON: {}", e))?;

        let mut diagnostics = Vec::new();
        for decl in &ast.declarations {
            if !["deny", "approval", "allow"].contains(&decl.effect.as_str()) {
                diagnostics.push(PolicyDiagnostic::error(
                    format!("declaration '{}'", decl.name),
                    Some(decl.name.clone()),
                    format!(
                        "Invalid effect '{}'. Expected one of: deny, approval, allow",
                        decl.effect
                    ),
                ));
            }
        }

        Ok((ast, diagnostics))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_policy_ast() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "require-shell-approval",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "approval",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, diagnostics) = PolicyParser::parse_json(json_raw).unwrap();
        assert_eq!(ast.declarations.len(), 1);
        assert!(diagnostics.is_empty());
    }
}
