//! Validation rules for Braian definitions and objects.
//!
//! The same crate compiles to `wasm32-unknown-unknown`. It does no I/O.
//! Rules live only on attributes. A type, and every structure group an object
//! belongs to, including ancestor groups, decide which attributes apply.
//! Older class lists still contribute the attributes of their group.

mod compile;
mod date;
mod validate;

pub use compile::{AttributeSource, CompiledModel, DefinitionError, EffectiveAttribute};
pub use validate::PatchReport;
