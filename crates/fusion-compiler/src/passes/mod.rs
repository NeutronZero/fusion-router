//! Compiler passes — budget optimisation and future passes.

pub mod budget;

pub use budget::{BudgetError, BudgetOptimisationPass};
