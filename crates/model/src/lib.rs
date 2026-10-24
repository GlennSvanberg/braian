//! Definition vocabulary for OpenPIM.
//!
//! Nothing in this crate performs I/O. A [`DefinitionSet`] describes types,
//! the global attribute pool, unit families and structures. An [`Object`]
//! carries values keyed by attribute id. Validation lives in `openpim-rules`.

mod definition;
mod edit;
mod error;
mod id;
mod language;
mod object;
mod patch;

pub use definition::{
    Attribute, AttributeKind, Class, DefinitionSet, Group, IndexFlags, NumberValue, ObjectType,
    Structure, Unit, UnitFamily, ValueOption,
};
pub use edit::{
    delete_attribute, delete_structure, delete_type, delete_unit_family, set_languages,
    upsert_attribute, upsert_structure, upsert_type, upsert_unit_family,
};
pub use error::{ErrorCode, ValidationError};
pub use id::{AttributeId, ClassId, GroupId, ObjectId, StructureId, TypeId, UnitFamilyId};
pub use language::{LanguageTag, LanguageTagError};
pub use object::Object;
pub use patch::{AttributePatch, ObjectPatch, ValuePatch, apply_object_patch, apply_value_patch};
pub use uuid::Uuid;
