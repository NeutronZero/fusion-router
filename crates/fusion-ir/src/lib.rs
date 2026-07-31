//! # fusion-ir
//!
//! Canonical provider-independent Workflow IR (v0.13 contract 2).
//!
//! ## Canonical graph invariant
//!
//! `WorkflowIR` is the canonical immutable graph representation. There is
//! intentionally no separate `WorkflowGraph` type. All graph operations are
//! performed directly on `WorkflowIR`. Additional graph views may be
//! introduced in future versions without changing the `WorkflowIR` contract.
//!
//! ## Provider-free law
//!
//! Provider-identifying configuration fields reserved by the architecture
//! (`model`, `provider`, `endpoint`) are rejected by validation. The list may
//! be expanded without redefining the law.
//!
//! ## Dependency rule
//!
//! This crate is a leaf: it depends on nothing in the FusionRouter stack.
//! Everything else in the stack depends on this crate.

mod builder;
mod edge;
mod node;
mod validate;
mod version;
mod workflow;

pub use builder::WorkflowBuilder;
pub use edge::{WorkflowEdge, WorkflowEdgeKind};
pub use node::{WorkflowNode, WorkflowNodeKind};
pub use validate::{ValidationError, ValidationIssue, ValidationReport};
pub use version::WORKFLOW_IR_VERSION;
pub use workflow::{WorkflowIR, WorkflowMetadata};
