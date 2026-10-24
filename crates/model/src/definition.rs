use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::id::{AttributeId, ClassId, GroupId, StructureId, TypeId, UnitFamilyId};
use crate::language::LanguageTag;

/// A definition set is the import and export file for the model.
///
/// Authored through the API from phase 3 on, and stored as JSON or YAML in git.
/// `model_revision` increases by one for every change to the set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct DefinitionSet {
    pub model_revision: u64,
    pub languages: Vec<LanguageTag>,
    pub unit_families: Vec<UnitFamily>,
    pub attributes: Vec<Attribute>,
    pub types: Vec<ObjectType>,
    pub structures: Vec<Structure>,
}

/// Units that convert to one base unit by multiplication.
///
/// `factor` turns an entered value into the base unit: 5000 g with factor
/// 0.001 is 5 kg. The base unit itself has factor 1. Filters and sorting use
/// the base-unit value; the entered value and unit are what gets stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct UnitFamily {
    pub id: UnitFamilyId,
    pub key: String,
    pub base_unit: String,
    pub units: Vec<Unit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub code: String,
    /// Multiplier into the base unit. Serialized as a decimal string.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub factor: Decimal,
}

/// One attribute in the global pool.
///
/// Rules live only here. A class decides that the attribute applies to an
/// object; it does not add or override rules. A value is stored once per
/// object even when several structures make the attribute apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    pub id: AttributeId,
    pub key: String,
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub labels: BTreeMap<LanguageTag, String>,
    pub language_dependent: bool,
    pub multiple: bool,
    pub kind: AttributeKind,
    pub index: IndexFlags,
}

/// Which parts of the attribute are copied into the search index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct IndexFlags {
    pub searchable: bool,
    pub filterable: bool,
    pub sortable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttributeKind {
    /// Plain text. Length is counted in Unicode scalar values, not bytes.
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
    },
    /// Exact decimal. `min` and `max` are in the base unit when the attribute has one.
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_family: Option<UnitFamilyId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
        min: Option<Decimal>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
        max: Option<Decimal>,
    },
    ValueList {
        options: Vec<ValueOption>,
    },
    Boolean,
    /// Calendar date, stored as `YYYY-MM-DD`.
    Date,
    /// Reference to a file. The file itself is not modeled in phase 1.
    FileRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ValueOption {
    pub code: String,
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub labels: BTreeMap<LanguageTag, String>,
}

/// A number as entered, before conversion to the base unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct NumberValue {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub value: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// An object type. There are no built-in types such as article or product.
///
/// `key_attribute` is the human key, unique within the type. The database
/// enforces that uniqueness; phase 1 only checks that the attribute is
/// single-valued, language-independent text and that a value is present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ObjectType {
    pub id: TypeId,
    pub key: String,
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub labels: BTreeMap<LanguageTag, String>,
    pub key_attribute: AttributeId,
    pub attributes: Vec<AttributeId>,
}

/// A hierarchy of groups plus the classes attached to those groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Structure {
    pub id: StructureId,
    pub key: String,
    pub groups: Vec<Group>,
    pub classes: Vec<Class>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: GroupId,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<GroupId>,
}

/// A class adds its attributes to one group, and to every group below it.
///
/// That is applicability, not value inheritance: an object placed in a child
/// group uses the ancestor class's attributes, but never its values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub id: ClassId,
    pub key: String,
    pub group: GroupId,
    pub attributes: Vec<AttributeId>,
}
