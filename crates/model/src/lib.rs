//! Definition vocabulary for Braian.
//!
//! Nothing in this crate performs I/O. A [`DefinitionSet`] describes types,
//! the global attribute pool, unit families and structures. An [`Object`]
//! carries values keyed by attribute id. Validation lives in `braian-rules`.

mod consequence;
mod definition;
mod draft;
mod edit;
mod error;
mod id;
mod language;
mod link;
mod model_change;
mod object;
mod patch;
pub mod path;
pub mod refs;
pub mod standard;
pub mod structure;

pub use consequence::{
    ConsequenceCounts, DataPurge, UsageFixture, ValueMatch, consequence_lines,
    consequence_sentence, grouped, purge_for, tally,
};
pub use definition::{
    Attribute, AttributeKind, Cardinality, Class, DefinitionSet, Group, IndexFlags, LinkLabel,
    LinkSpec, NumberValue, ObjectType, SharedValueList, Structure, TextForm, TypeKind, Unit,
    UnitFamily, ValueOption, default_plural, operation_title,
};
pub use draft::{ModelDraft, change_removed_definitions, restored_definitions, undo_purge};
pub use edit::{
    delete_attribute, delete_structure, delete_type, delete_unit_family, set_languages,
    upsert_attribute, upsert_structure, upsert_type, upsert_unit_family,
};
pub use error::{ErrorCode, ValidationError};
pub use id::{
    AttributeId, ClassId, GroupId, ObjectId, StructureId, TypeId, UnitFamilyId, ValueListId,
};
pub use language::{LanguageTag, LanguageTagError};
pub use link::{RefConversion, RefLinkPlan, convert_object_refs, explain_link};
pub use model_change::{
    ApplyError, AttributeBounds, ChangeImpact, ModelChange, NamedOption, UnitSpec, apply_all,
    change_needs_reindex, deletes_data, diff as model_diff, groups_removed_by_structure_upserts,
    interpret, preview,
};
pub use object::Object;
pub use patch::{AttributePatch, ObjectPatch, ValuePatch, apply_object_patch, apply_value_patch};
pub use path::{
    AttributePath, ListedPath, PathPart, PathSide, PathVia, extra_parts, format_path_id,
    format_path_id_text, format_path_text, list_attribute_paths, parse_path, parse_path_id,
    parse_path_text, part_fits, path_index_field, path_label, stored_field_as_path,
    translate_filter_expression, translate_sort_expression, validate_path,
};
pub use refs::{
    RefKind, ResolvedRef, attribute_index_field, key_rename_warning, resolve,
    translate_assortment_query, translate_import_mapping, translate_view_state,
};
pub use standard::{
    StandardIds, apply_defaults, detach_name_aliases, install_standard_attributes,
    is_locked_standard, is_standard_attribute, move_values, name_id, number_id, placement_allowed,
    standard_ids, structures_from_usage,
};
pub use structure::{GroupRemoval, StructureError, StructureErrorCode};
pub use uuid::Uuid;
