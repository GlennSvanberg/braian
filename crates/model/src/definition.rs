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
    /// Value lists that several attributes can share. An attribute may still
    /// carry its options inline when it does not reference one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_lists: Vec<SharedValueList>,
    pub attributes: Vec<Attribute>,
    pub types: Vec<ObjectType>,
    pub structures: Vec<Structure>,
}

/// A value list defined once and referenced by attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SharedValueList {
    pub id: crate::id::ValueListId,
    pub key: String,
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub labels: BTreeMap<LanguageTag, String>,
    pub options: Vec<ValueOption>,
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
    /// Shared dimension such as `length` or `mass`. Families with the same
    /// dimension convert through their base unit. Empty means the key is the dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    pub base_unit: String,
    pub units: Vec<Unit>,
}

impl UnitFamily {
    pub fn dimension(&self) -> &str {
        self.dimension.as_deref().unwrap_or(self.key.as_str())
    }
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

/// One attribute in the global pool, identified by `key`.
///
/// Rules live only here. A type or a structure node decides that the attribute
/// applies; it does not add a second definition. When the same key arrives from
/// several places the object still has the attribute once. A different type or
/// unit for that key is rejected when the attribute is added.
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
    /// The value must be present when the attribute applies. Absent means optional.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
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
    /// `form` is `short` or `long`. Language dependence is the attribute flag.
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
        #[serde(default, skip_serializing_if = "TextForm::is_short")]
        form: TextForm,
    },
    /// Exact decimal or integer. `min` and `max` are in the base unit when the attribute has one.
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_family: Option<UnitFamilyId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
        min: Option<Decimal>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
        max: Option<Decimal>,
        /// When true the entered value must be a whole number. Storage stays decimal.
        #[serde(default, skip_serializing_if = "is_false")]
        integer: bool,
    },
    /// A closed interval. The value is `{min, max, unit?}`.
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_family: Option<UnitFamilyId>,
    },
    /// Options are inline, or `list` names a [`SharedValueList`] key.
    ValueList {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<ValueOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list: Option<String>,
    },
    Boolean,
    /// Calendar date, stored as `YYYY-MM-DD`.
    Date,
    /// Reference to another object. `type_key` limits the picker; rules check the id shape.
    ObjectRef {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        type_key: Option<String>,
    },
    /// File or media reference. The bytes live outside the object.
    FileRef {
        #[serde(default, skip_serializing_if = "is_false")]
        media: bool,
    },
}

/// Short text is one line. Long text is a multi-line value. Both are plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TextForm {
    #[default]
    Short,
    Long,
}

impl TextForm {
    pub fn is_short(&self) -> bool {
        matches!(self, Self::Short)
    }
}

impl AttributeKind {
    pub fn unit_family_id(&self) -> Option<UnitFamilyId> {
        match self {
            Self::Number { unit_family, .. } | Self::Range { unit_family } => *unit_family,
            _ => None,
        }
    }

    /// Same value type and the same unit family.
    ///
    /// Length, required, and index flags stay on the single definition and are
    /// not a second attribute.
    pub fn same_type_and_unit(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
            && self.unit_family_id() == other.unit_family_id()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
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
    /// Singular display name per language. Empty falls back to labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub singular: BTreeMap<LanguageTag, String>,
    /// Plural display name per language. Empty falls back to an English-style plural of the singular.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = BTreeMap<String, String>))]
    pub plural: BTreeMap<LanguageTag, String>,
    pub key_attribute: AttributeId,
    pub attributes: Vec<AttributeId>,
}

impl ObjectType {
    /// Singular name for `language`, then English, then any stored name, then the key.
    pub fn singular_label(&self, language: &str) -> String {
        stored_label(&self.singular, language)
            .or_else(|| stored_label(&self.labels, language))
            .unwrap_or_else(|| self.key.clone())
    }

    /// Plural name for `language`. A missing plural is derived from the singular.
    ///
    /// Another language's plural is not reused. English is the only cross-language fallback.
    pub fn plural_label(&self, language: &str) -> String {
        plural_stored(&self.plural, language)
            .unwrap_or_else(|| default_plural(&self.singular_label(language)))
    }
}

/// English-style plural used when a type has no plural label.
///
/// `Product` becomes `Products`, `Category` becomes `Categories`.
pub fn default_plural(singular: &str) -> String {
    let word = singular.trim();
    if word.is_empty() {
        return "Objects".to_owned();
    }
    let lower = word.to_lowercase();
    if lower.ends_with("ch")
        || lower.ends_with("sh")
        || lower.ends_with('s')
        || lower.ends_with('x')
        || lower.ends_with('z')
    {
        return format!("{word}es");
    }
    let chars: Vec<char> = word.chars().collect();
    if chars.len() > 1 && matches!(chars.last().copied(), Some('y' | 'Y')) {
        let previous = chars[chars.len() - 2].to_ascii_lowercase();
        if !matches!(previous, 'a' | 'e' | 'i' | 'o' | 'u') {
            let stem: String = chars[..chars.len() - 1].iter().collect();
            return format!("{stem}ies");
        }
    }
    format!("{word}s")
}

fn plural_stored(map: &BTreeMap<LanguageTag, String>, language: &str) -> Option<String> {
    let exact = map.iter().find_map(|(tag, value)| {
        (tag.as_str() == language && !value.is_empty()).then(|| value.clone())
    });
    if exact.is_some() {
        return exact;
    }
    if language != "en" {
        return map.iter().find_map(|(tag, value)| {
            (tag.as_str() == "en" && !value.is_empty()).then(|| value.clone())
        });
    }
    None
}

fn stored_label(map: &BTreeMap<LanguageTag, String>, language: &str) -> Option<String> {
    let exact = map.iter().find_map(|(tag, value)| {
        (tag.as_str() == language && !value.is_empty()).then(|| value.clone())
    });
    if exact.is_some() {
        return exact;
    }
    if language != "en" {
        let english = map.iter().find_map(|(tag, value)| {
            (tag.as_str() == "en" && !value.is_empty()).then(|| value.clone())
        });
        if english.is_some() {
            return english;
        }
    }
    map.values().find(|value| !value.is_empty()).cloned()
}

/// Operation titles shown to a person.
///
/// One configured type replaces the words `object` and `objects`. Several types
/// keep those words, because the operation is not about one type.
pub fn operation_title(title: &str, types: &[ObjectType], language: &str) -> String {
    let Some(object_type) = types.first().filter(|_| types.len() == 1) else {
        return title.to_owned();
    };
    let singular = object_type.singular_label(language);
    let plural = object_type.plural_label(language);
    title
        .split_inclusive(char::is_whitespace)
        .map(|word| {
            let trimmed = word.trim_end();
            let space = &word[trimmed.len()..];
            let replaced = match trimmed {
                "objects" | "Objects" => plural.as_str(),
                "object" | "Object" => singular.as_str(),
                _ => trimmed,
            };
            format!("{replaced}{space}")
        })
        .collect()
}

/// A hierarchy of groups. Nodes are not tied to an object type.
///
/// `classes` remains so older revisions still load. New applicability is
/// `Group::attributes`; the compiler also honours class lists already stored.
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
    /// Attributes defined on this node. Ancestor nodes apply as well.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeId>,
}

/// Legacy applicability stored on older models.
///
/// A class lists attributes for one group. Those attributes apply to that group
/// and, through the group tree, to every group below it. The editor writes
/// [`Group::attributes`] instead. Values are never copied from a parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub id: ClassId,
    pub key: String,
    pub group: GroupId,
    /// Attribute inheritance. Values are never copied from the parent class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ClassId>,
    pub attributes: Vec<AttributeId>,
}

#[cfg(test)]
mod label_tests {
    use super::{ObjectType, default_plural, operation_title};
    use crate::id::{AttributeId, TypeId};
    use crate::language::LanguageTag;

    fn object_type(key: &str, singular: &str, plural: Option<&str>) -> ObjectType {
        let mut singular_map = std::collections::BTreeMap::new();
        singular_map.insert(LanguageTag::parse("en").unwrap(), singular.to_owned());
        let mut plural_map = std::collections::BTreeMap::new();
        if let Some(plural) = plural {
            plural_map.insert(LanguageTag::parse("en").unwrap(), plural.to_owned());
        }
        ObjectType {
            id: TypeId::from_uuid(uuid::Uuid::from_u128(1)),
            key: key.to_owned(),
            labels: std::collections::BTreeMap::new(),
            singular: singular_map,
            plural: plural_map,
            key_attribute: AttributeId::from_uuid(uuid::Uuid::from_u128(2)),
            attributes: vec![],
        }
    }

    #[test]
    fn plural_defaults_and_operation_titles() {
        assert_eq!(default_plural("Product"), "Products");
        assert_eq!(default_plural("Article"), "Articles");
        assert_eq!(default_plural("Category"), "Categories");
        assert_eq!(default_plural("Bus"), "Buses");
        let product = object_type("produkt", "Product", None);
        assert_eq!(product.singular_label("en"), "Product");
        assert_eq!(product.plural_label("en"), "Products");
        assert_eq!(
            operation_title("Open object", std::slice::from_ref(&product), "en"),
            "Open Product"
        );
        assert_eq!(
            operation_title("Search objects", std::slice::from_ref(&product), "en"),
            "Search Products"
        );
        let article = object_type("artikel", "Article", Some("Articles"));
        assert_eq!(
            operation_title("Open object", &[product, article], "en"),
            "Open object"
        );
    }
}
