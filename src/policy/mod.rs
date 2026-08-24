//! Policy Compilation Subsystem Module (`src/policy/mod.rs`)

pub mod ast;
pub mod bridge;
pub mod diagnostics;
pub mod ir;
pub mod policy_registry;
pub mod precedence;
pub mod trace;

pub use policy_registry::PolicyRegistry;

#[allow(unused_imports)]
pub use ast::{PolicyAST, PolicyDeclaration, PolicyParser};
#[allow(unused_imports)]
pub use diagnostics::{DiagnosticSeverity, PolicyDiagnostic};
#[allow(unused_imports)]
pub use ir::{PolicyAction, PolicyCondition, PolicyEffect, PolicyIR, PolicyRule};
#[allow(unused_imports)]
pub use precedence::PolicyPrecedenceEngine;
#[allow(unused_imports)]
pub use trace::{PolicyMatchEvent, PolicyTrace};
