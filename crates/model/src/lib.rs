//! Definition vocabulary for Braian.
//!
//! Nothing in this crate performs I/O. A [`DefinitionSet`] describes types,
//! the global attribute pool, unit families and structures. An [`Object`]
//! carries values keyed by attribute id. Validation lives in `braian-rules`.

mod definition;
mod edit;
mod error;
mod id;
mod language;
mod model_change;
mod object;
mod patch;
pub mod refs;
pub mod structure;

pub use definition::{
    Attribute, AttributeKind, Class, DefinitionSet, Group, IndexFlags, NumberValue, ObjectType,
    SharedValueList, Structure, TextForm, Unit, UnitFamily, ValueOption, default_plural,
    operation_title,
};
pub use edit::{
    delete_attribute, delete_structure, delete_type, delete_unit_family, set_languages,
    upsert_attribute, upsert_structure, upsert_type, upsert_unit_family,
};
pub use error::{ErrorCode, ValidationError};
pub use id::{
    AttributeId, ClassId, GroupId, ObjectId, StructureId, TypeId, UnitFamilyId, ValueListId,
};
pub use language::{LanguageTag, LanguageTagError};
pub use model_change::{
    ApplyError, ChangeImpact, ModelChange, apply_all, change_needs_reindex, deletes_data,
    diff as model_diff, interpret, preview,
};
pub use object::Object;
pub use patch::{AttributePatch, ObjectPatch, ValuePatch, apply_object_patch, apply_value_patch};
pub use refs::{
    RefKind, ResolvedRef, attribute_index_field, key_rename_warning, resolve,
    translate_assortment_query, translate_import_mapping, translate_view_state,
};
pub use structure::{GroupRemoval, StructureError, StructureErrorCode};
pub use uuid::Uuid;
