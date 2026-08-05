//! Security primitives shared across subsystems.
//!
//! Law 10: path validation must canonicalize within its trust root — see
//! `paths` for the canonicalization helpers used by file tools, the shell
//! argument policy, and plugin extraction.

pub mod paths;
