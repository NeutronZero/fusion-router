//! Security primitives shared across subsystems.
//!
//! Law 10: path validation must canonicalize within its trust root â€” see
//! `paths` for the canonicalization helpers used by file tools, the shell
//! argument policy, and plugin extraction.

pub mod openat2;
pub mod paths;
pub mod secrets;
