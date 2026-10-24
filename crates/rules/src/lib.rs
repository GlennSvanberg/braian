//! Validation rules for Braian definitions and objects.
//!
//! The same crate compiles to `wasm32-unknown-unknown`. It does no I/O.
//! Rules live only on attributes. A class decides which attributes apply,
//! including the classes of every ancestor group.

mod compile;
mod date;
mod validate;

pub use compile::{CompiledModel, DefinitionError};
pub use validate::PatchReport;
