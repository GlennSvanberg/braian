//! Edits of a definition set that a person confirms as one proposal.
//!
//! Nothing here writes. The caller compiles the result and stores it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rust_decimal::Decimal;

use crate::definition::{
    Attribute, AttributeKind, Class, DefinitionSet, Group, ObjectType, Structure, TextForm, Unit,
    UnitFamily, ValueOption,
};
use crate::id::{AttributeId, ClassId, GroupId, StructureId, UnitFamilyId};
use crate::language::LanguageTag;
use crate::structure::{self, StructureError};

/// One step in a data-model proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ModelChange {
    MoveClass {
        structure: String,
        #[serde(alias = "class")]
        class_key: String,
        group: String,
    },
    SetParent {
        structure: String,
        #[serde(rename = "group", alias = "class", alias = "class_key")]
        class_key: String,
        parent: Option<String>,
    },
    AddAttribute {
        structure: String,
        #[serde(rename = "group", alias = "class", alias = "class_key")]
        class_key: String,
        attribute: String,
    },
    RemoveAttribute {
        structure: String,
        #[serde(rename = "group", alias = "class", alias = "class_key")]
        class_key: String,
        attribute: String,
    },
    ReorderAttributes {
        structure: String,
        #[serde(rename = "group", alias = "class", alias = "class_key")]
        class_key: String,
        attributes: Vec<String>,
    },
    CreateAttribute {
        key: String,
        #[serde(default)]
        label: String,
        kind: String,
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        multiple: bool,
        #[serde(default)]
        languages: bool,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        searchable: bool,
        /// Proposals written before index flags were sent still mark the attribute filterable.
        #[serde(default = "default_filterable")]
        filterable: bool,
        #[serde(default)]
        sortable: bool,
        /// Min, max, length, text form, value list and unit family set on create.
        #[serde(flatten, default)]
        bounds: AttributeBounds,
    },
    /// Link an existing attribute onto a type or a group. The definition stays one row.
    AttachAttribute {
        attribute: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        type_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structure: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
    },
    DetachAttribute {
        attribute: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        type_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structure: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
    },
    /// Singular and plural display names for one object type, in one language.
    UpdateType {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        singular: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plural: Option<String>,
        /// Replaces the trees the type may use. Empty means the type has no tree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structures: Option<Vec<String>>,
        /// Replaces the type's default values, keyed by attribute key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defaults: Option<BTreeMap<String, String>>,
    },
    UpdateAttribute {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default)]
        clear_unit: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        languages: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        searchable: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filterable: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sortable: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        multiple: Option<bool>,
        #[serde(flatten, default)]
        bounds: AttributeBounds,
    },
    /// A new object type. Standard attributes are attached. An existing key stays put.
    CreateType {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plural: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structures: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defaults: Option<BTreeMap<String, String>>,
    },
    /// A link type. It does not receive number or name.
    CreateLinkType {
        key: String,
        #[serde(default)]
        label: String,
        from_types: Vec<String>,
        to_types: Vec<String>,
        from_key: String,
        #[serde(default)]
        from_label: String,
        to_key: String,
        #[serde(default)]
        to_label: String,
        #[serde(default = "default_many")]
        from_cardinality: String,
        #[serde(default = "default_many")]
        to_cardinality: String,
        /// Attribute keys to attach. A missing key is created as short text.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attributes: Vec<String>,
    },
    UpdateLinkType {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_types: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_types: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_cardinality: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_cardinality: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attributes: Option<Vec<String>>,
    },
    DeleteLinkType {
        key: String,
    },
    CreateValueList {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        options: Vec<NamedOption>,
    },
    UpdateValueList {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// Replaces the options, in this order. Omitted leaves them as they are.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<NamedOption>>,
    },
    CreateUnitFamily {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dimension: Option<String>,
        base_unit: String,
        #[serde(default)]
        units: Vec<UnitSpec>,
    },
    UpdateUnitFamily {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dimension: Option<String>,
        #[serde(default)]
        clear_dimension: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        units: Option<Vec<UnitSpec>>,
    },
    /// Reorders the attributes on a type. Standard attributes stay at the front.
    ReorderTypeAttributes {
        #[serde(rename = "type", alias = "type_key")]
        type_key: String,
        attributes: Vec<String>,
    },
    CreateClass {
        structure: String,
        #[serde(alias = "class")]
        class_key: String,
        group: String,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        attributes: Vec<String>,
    },
    CreateGroup {
        structure: String,
        group: String,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        label: String,
    },
    CreateStructure {
        key: String,
        #[serde(default)]
        label: String,
    },
    RenameStructure {
        structure: String,
        label: String,
    },
    DeleteStructure {
        structure: String,
    },
    RenameGroup {
        structure: String,
        group: String,
        label: String,
    },
    /// `parent` none places the group at the root of the structure.
    MoveGroup {
        structure: String,
        group: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// `move_to` is a group outside the deleted subtree that should receive its objects.
    DeleteGroup {
        structure: String,
        group: String,
        #[serde(default)]
        move_to: Option<String>,
    },
    /// Copies the group node. Children and classes stay on the original.
    DuplicateGroup {
        structure: String,
        group: String,
        name: String,
    },
    /// `key` is the current id or key. The id does not change.
    RenameTypeKey {
        #[serde(alias = "id")]
        key: String,
        new_key: String,
    },
    RenameAttributeKey {
        #[serde(alias = "id")]
        key: String,
        new_key: String,
    },
    RenameStructureKey {
        #[serde(alias = "key", alias = "id")]
        structure: String,
        new_key: String,
    },
    RenameGroupKey {
        #[serde(alias = "id")]
        structure: String,
        group: String,
        new_key: String,
    },
    RenameValueListKey {
        #[serde(alias = "id")]
        key: String,
        new_key: String,
    },
    RenameUnitFamilyKey {
        #[serde(alias = "id")]
        key: String,
        new_key: String,
    },
    /// Removes a type. Objects, their values, links and selections of that type go with it.
    DeleteType {
        #[serde(alias = "id")]
        key: String,
    },
    /// Removes an attribute from the pool and from every type and group.
    DeleteAttribute {
        #[serde(alias = "id")]
        key: String,
    },
    DeleteValueList {
        #[serde(alias = "id")]
        key: String,
    },
    DeleteValueOption {
        list: String,
        option: String,
    },
    DeleteUnitFamily {
        #[serde(alias = "id")]
        key: String,
    },
    DeleteUnit {
        family: String,
        unit: String,
    },
    /// Replaces the trees a type may use. An empty list means the type has no tree.
    SetStructures {
        #[serde(alias = "id")]
        type_key: String,
        structures: Vec<String>,
    },
    /// Replaces one attribute document. A new id inserts it. The id is kept.
    ///
    /// Rule changes use the same checks as [`ModelChange::UpdateAttribute`] and
    /// [`ModelChange::RenameAttributeKey`].
    UpsertAttribute {
        attribute: Box<Attribute>,
    },
    /// Replaces one type document. A new id inserts it. The id is kept.
    ///
    /// A new key uses the same checks as [`ModelChange::CreateType`].
    /// A new type receives the standard attributes from that same step.
    /// An update keeps those ids and does not add a second copy.
    UpsertType {
        object_type: Box<ObjectType>,
    },
    /// Replaces one structure document, including its groups. The id is kept.
    ///
    /// Keys, parents and cycles use the same checks as creating a structure and its groups.
    /// Groups this step drops are listed by [`groups_removed_by_structure_upserts`].
    /// The save refuses the step, including a dry run, when those groups still hold objects.
    UpsertStructure {
        structure: Box<Structure>,
    },
    /// Replaces one unit family document. The id is kept.
    ///
    /// The base unit and unit rows use the same checks as [`ModelChange::CreateUnitFamily`].
    UpsertUnitFamily {
        family: Box<UnitFamily>,
    },
}

/// One option on a value list, in the order the step names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedOption {
    pub code: String,
    #[serde(default)]
    pub label: String,
}

/// One unit in a family. `factor` is a decimal string. The base unit is 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitSpec {
    pub code: String,
    #[serde(default = "factor_one")]
    pub factor: String,
}

fn factor_one() -> String {
    "1".into()
}

/// Extra rules on an attribute. Flattened into create and update steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AttributeBounds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    #[serde(default, skip_serializing_if = "is_false_flag")]
    pub clear_min: bool,
    #[serde(default, skip_serializing_if = "is_false_flag")]
    pub clear_max: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false_flag")]
    pub clear_max_length: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_list: Option<String>,
    #[serde(default, skip_serializing_if = "is_false_flag")]
    pub clear_value_list: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_family: Option<String>,
}

fn is_false_flag(value: &bool) -> bool {
    !*value
}

/// A definition edit that did not apply.
#[derive(Debug)]
pub enum ApplyError {
    Structure(StructureError),
    Other(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structure(error) => formatter.write_str(&error.message),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for ApplyError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for ApplyError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_owned())
    }
}

impl From<StructureError> for ApplyError {
    fn from(error: StructureError) -> Self {
        Self::Structure(error)
    }
}

/// What a person should read before confirming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeImpact {
    pub lines: Vec<String>,
    /// Attribute keys named by the change, including ones a job might read.
    pub fields: Vec<String>,
    /// Attribute keys whose stored values may become invalid.
    pub usage_fields: Vec<String>,
    /// Group ids whose objects change group applicability.
    pub groups: Vec<String>,
    /// Key changes. External integrations that still send the old key stop matching.
    pub warnings: Vec<String>,
}

fn default_filterable() -> bool {
    true
}

fn default_many() -> String {
    "many".to_owned()
}

/// A step that removes stored data or a definition that data hangs on.
///
/// The agent is not offered these, and cannot propose them.
pub fn deletes_data(change: &ModelChange) -> bool {
    matches!(
        change,
        ModelChange::DeleteStructure { .. }
            | ModelChange::DeleteGroup { .. }
            | ModelChange::DeleteType { .. }
            | ModelChange::DeleteLinkType { .. }
            | ModelChange::DeleteAttribute { .. }
            | ModelChange::DeleteValueList { .. }
            | ModelChange::DeleteValueOption { .. }
            | ModelChange::DeleteUnitFamily { .. }
            | ModelChange::DeleteUnit { .. }
            | ModelChange::RemoveAttribute { .. }
            | ModelChange::DetachAttribute { .. }
    )
}

/// Type, structure and group keys are copied into the search document.
pub fn change_needs_reindex(change: &ModelChange) -> bool {
    matches!(
        change,
        ModelChange::RenameTypeKey { .. }
            | ModelChange::RenameStructureKey { .. }
            | ModelChange::RenameGroupKey { .. }
            | ModelChange::SetStructures { .. }
    )
}

/// Applies every step. Later steps see earlier ones. Idempotent creates stay put.
pub fn apply_all(defs: &mut DefinitionSet, changes: &[ModelChange]) -> Result<(), ApplyError> {
    for change in changes {
        apply_one(defs, change)?;
    }
    Ok(())
}

/// Applies the steps on a clone and describes the difference.
pub fn preview(
    defs: &DefinitionSet,
    changes: &[ModelChange],
) -> Result<(DefinitionSet, ChangeImpact), ApplyError> {
    let mut after = defs.clone();
    apply_all(&mut after, changes)?;
    let impact = diff(defs, &after);
    Ok((after, impact))
}

/// A sentence the palette and the agent can both run.
pub fn interpret(text: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename attribute key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::Attribute, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename type key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::Type, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename structure key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::Structure, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename group key ") {
        return rename_group_key_sentence(defs, rest);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename value list key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::ValueList, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename unit family key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::UnitFamily, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename key ") {
        return rename_key_sentence(defs, crate::refs::RefKind::Attribute, rest, None);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "add attribute ") {
        return add_attribute_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "remove attribute ") {
        return remove_attribute_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "add group ") {
        return add_group_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "rename group ") {
        return rename_group_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "move group ") {
        return move_group_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "duplicate group ") {
        return duplicate_group_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "delete group ") {
        return delete_group_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "move class ") {
        return move_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "move ") {
        return move_command(rest, defs);
    }
    None
}

pub fn diff(before: &DefinitionSet, after: &DefinitionSet) -> ChangeImpact {
    let mut lines = Vec::new();
    let mut fields = BTreeSet::new();
    let mut usage_fields = BTreeSet::new();
    let mut groups = BTreeSet::new();
    let mut warnings = Vec::new();
    for rename in crate::refs::key_renames(before, after) {
        lines.push(format!(
            "Rename {} key {} to {}",
            rename.kind, rename.old_key, rename.new_key
        ));
        warnings.push(crate::refs::key_rename_warning(&rename.old_key));
        if rename.kind == "attribute" {
            fields.insert(rename.new_key.clone());
        }
    }
    let languages: &[LanguageTag] = if after.languages.is_empty() {
        &before.languages
    } else {
        &after.languages
    };
    let before_attrs: BTreeMap<AttributeId, &Attribute> = before
        .attributes
        .iter()
        .map(|attribute| (attribute.id, attribute))
        .collect();
    for attribute in &after.attributes {
        match before_attrs.get(&attribute.id) {
            None => {
                lines.push(format!(
                    "Create attribute {} ({})",
                    attribute.key,
                    kind_label(&attribute.kind)
                ));
                fields.insert(attribute.key.clone());
            }
            Some(previous) => {
                if previous.required != attribute.required
                    || previous.multiple != attribute.multiple
                    || previous.language_dependent != attribute.language_dependent
                    || previous.index != attribute.index
                    || previous.kind != attribute.kind
                    || previous.labels != attribute.labels
                {
                    lines.push(format!("Update attribute {}", attribute.key));
                    fields.insert(attribute.key.clone());
                }
            }
        }
    }
    for structure in &after.structures {
        let previous = before
            .structures
            .iter()
            .find(|item| item.id == structure.id);
        if previous.is_none() {
            lines.push(format!("Add structure {}", structure.key));
        }
        let old_groups = previous.map(|item| &item.groups);
        for group in &structure.groups {
            let existed =
                old_groups.is_some_and(|groups| groups.iter().any(|item| item.id == group.id));
            if !existed {
                let name = caption_of(languages, &group.labels, &group.key);
                let parent = group_caption(languages, structure, group.parent);
                match parent {
                    Some(parent) => lines.push(format!("Add group {name} under {parent}")),
                    None => lines.push(format!("Add group {name}")),
                }
            }
            let old_ids = old_groups
                .and_then(|groups| groups.iter().find(|item| item.id == group.id))
                .map(|item| item.attributes.as_slice())
                .unwrap_or(&[]);
            let old_keys = attribute_keys(before, old_ids);
            let new_keys = attribute_keys(after, &group.attributes);
            let name = caption_of(languages, &group.labels, &group.key);
            for key in new_keys.difference(&old_keys) {
                lines.push(format!("Add attribute {key} to group {name}"));
                fields.insert(key.clone());
            }
            for key in old_keys.difference(&new_keys) {
                lines.push(format!("Remove attribute {key} from group {name}"));
                fields.insert(key.clone());
                usage_fields.insert(key.clone());
            }
        }
        if let Some(previous) = previous {
            if previous.labels != structure.labels {
                lines.push(format!(
                    "Rename structure {} to {}",
                    structure.key,
                    display_label(&structure.labels, &structure.key)
                ));
            }
            for old in &previous.groups {
                let Some(group) = structure.groups.iter().find(|item| item.id == old.id) else {
                    let name = caption_of(languages, &old.labels, &old.key);
                    lines.push(format!("Delete group {name}"));
                    groups.insert(old.id.as_uuid().to_string());
                    continue;
                };
                if old.parent != group.parent {
                    let name = caption_of(languages, &group.labels, &group.key);
                    let parent = group_caption(languages, structure, group.parent)
                        .unwrap_or_else(|| "the root".into());
                    lines.push(format!("Move group {name} under {parent}"));
                    groups.insert(old.id.as_uuid().to_string());
                }
                if old.labels != group.labels {
                    let from = caption_of(languages, &old.labels, &old.key);
                    let mut to = caption_of(languages, &group.labels, &group.key);
                    if from == to {
                        to = display_label(&group.labels, &group.key);
                    }
                    if from != to {
                        lines.push(format!("Rename group {from} to {to}"));
                    }
                }
            }
        }
        let old_classes = previous.map(|item| &item.classes);
        for class in &structure.classes {
            let old =
                old_classes.and_then(|classes| classes.iter().find(|item| item.key == class.key));
            let group_name = group_caption(languages, structure, Some(class.group))
                .unwrap_or_else(|| "?".into());
            match old {
                None => {
                    let parent = class_key(structure, class.parent);
                    match parent {
                        Some(parent) => lines.push(format!(
                            "Add group {} on {group_name}, parent {parent}",
                            class.key
                        )),
                        None => lines.push(format!("Add group {} on {group_name}", class.key)),
                    }
                    for key in attribute_keys(after, &class.attributes) {
                        lines.push(format!("Add attribute {key} to {}", class.key));
                        fields.insert(key);
                    }
                }
                Some(old) => {
                    let old_structure = previous.expect("class implies structure");
                    let old_group = group_caption(languages, old_structure, Some(old.group))
                        .unwrap_or_else(|| "?".into());
                    if old.group != class.group {
                        lines.push(format!(
                            "Move group {} from {old_group} to {group_name}",
                            class.key
                        ));
                        groups.insert(old.group.as_uuid().to_string());
                        groups.insert(class.group.as_uuid().to_string());
                        for key in attribute_keys(after, &old.attributes) {
                            fields.insert(key.clone());
                            usage_fields.insert(key);
                        }
                    }
                    let old_parent = class_key(old_structure, old.parent);
                    let new_parent = class_key(structure, class.parent);
                    if old_parent != new_parent {
                        lines.push(format!(
                            "Group {} parent is {}",
                            class.key,
                            new_parent.unwrap_or_else(|| "none".into())
                        ));
                    }
                    let old_keys = attribute_keys(before, &old.attributes);
                    let new_keys = attribute_keys(after, &class.attributes);
                    for key in new_keys.difference(&old_keys) {
                        lines.push(format!("Add attribute {key} to {}", class.key));
                        fields.insert(key.clone());
                    }
                    for key in old_keys.difference(&new_keys) {
                        lines.push(format!("Remove attribute {key} from {}", class.key));
                        fields.insert(key.clone());
                        usage_fields.insert(key.clone());
                        groups.insert(old.group.as_uuid().to_string());
                    }
                    if old_keys == new_keys && old.attributes != class.attributes {
                        lines.push(format!("Reorder attributes on {}", class.key));
                    }
                }
            }
        }
    }
    for old in &before.structures {
        if after.structures.iter().any(|item| item.id == old.id) {
            continue;
        }
        lines.push(format!("Delete structure {}", old.key));
        for group in &old.groups {
            groups.insert(group.id.as_uuid().to_string());
        }
    }
    for list in &after.value_lists {
        match before.value_lists.iter().find(|item| item.id == list.id) {
            None => lines.push(format!("Create value list {}", list.key)),
            Some(previous)
                if previous.options != list.options || previous.labels != list.labels =>
            {
                lines.push(format!("Update value list {}", list.key));
            }
            Some(_) => {}
        }
    }
    for old in &before.value_lists {
        if after.value_lists.iter().all(|item| item.id != old.id) {
            lines.push(format!("Delete value list {}", old.key));
        }
    }
    for family in &after.unit_families {
        match before
            .unit_families
            .iter()
            .find(|item| item.id == family.id)
        {
            None => lines.push(format!("Create unit family {}", family.key)),
            Some(previous)
                if previous.units != family.units
                    || previous.base_unit != family.base_unit
                    || previous.dimension != family.dimension =>
            {
                lines.push(format!("Update unit family {}", family.key));
            }
            Some(_) => {}
        }
    }
    for old in &before.unit_families {
        if after.unit_families.iter().all(|item| item.id != old.id) {
            lines.push(format!("Delete unit family {}", old.key));
        }
    }
    for object_type in &after.types {
        let old = before.types.iter().find(|item| item.id == object_type.id);
        if old.is_none() {
            if object_type.is_link() {
                lines.push(format!(
                    "Create link type {}",
                    object_type.singular_label("en")
                ));
            } else {
                lines.push(format!("Create type {}", object_type.key));
            }
        }
        let old_keys = attribute_keys(
            before,
            old.map(|item| item.attributes.as_slice()).unwrap_or(&[]),
        );
        let new_keys = attribute_keys(after, &object_type.attributes);
        for key in new_keys.difference(&old_keys) {
            lines.push(format!("Add attribute {key} to type {}", object_type.key));
            fields.insert(key.clone());
        }
        for key in old_keys.difference(&new_keys) {
            lines.push(format!(
                "Remove attribute {key} from type {}",
                object_type.key
            ));
            fields.insert(key.clone());
            usage_fields.insert(key.clone());
        }
        if let Some(previous) = old {
            if previous.link != object_type.link || previous.kind != object_type.kind {
                lines.push(format!(
                    "Update link type {}",
                    object_type.singular_label("en")
                ));
            }
            if previous.singular != object_type.singular || previous.labels != object_type.labels {
                lines.push(format!(
                    "Type {} singular is {}",
                    object_type.key,
                    object_type.singular_label("en")
                ));
            }
            if previous.plural != object_type.plural {
                lines.push(format!(
                    "Type {} plural is {}",
                    object_type.key,
                    object_type.plural_label("en")
                ));
            }
            if previous.defaults != object_type.defaults {
                lines.push(format!("Type {} defaults changed", object_type.key));
            }
            if previous.structures != object_type.structures {
                let listed = object_type.structures.as_ref().map(|ids| {
                    ids.iter()
                        .map(|id| {
                            after
                                .structures
                                .iter()
                                .find(|item| item.id == *id)
                                .map(|item| item.key.as_str())
                                .unwrap_or("unknown")
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                match listed {
                    None => {}
                    Some(text) if text.is_empty() => {
                        lines.push(format!("Type {} uses no tree", object_type.key));
                    }
                    Some(text) => {
                        lines.push(format!("Type {} may use {text}", object_type.key));
                    }
                }
            }
        }
    }
    for old in &before.types {
        if after.types.iter().all(|item| item.id != old.id) {
            if old.is_link() {
                lines.push(format!("Delete link type {}", old.singular_label("en")));
            } else {
                lines.push(format!("Delete type {}", old.key));
            }
        }
    }
    ChangeImpact {
        lines,
        fields: fields.into_iter().collect(),
        usage_fields: usage_fields.into_iter().collect(),
        groups: groups.into_iter().collect(),
        warnings,
    }
}

fn apply_one(defs: &mut DefinitionSet, change: &ModelChange) -> Result<(), ApplyError> {
    if crate::consequence::apply_destructive(defs, change).map_err(ApplyError::from)? {
        return Ok(());
    }
    match change {
        ModelChange::CreateStructure { key, label } => {
            let id = StructureId::from_uuid(stable_uuid(&format!("structure:{key}")));
            structure::create_structure(defs, id, key, label, false)?;
            Ok(())
        }
        ModelChange::RenameStructure { structure, label } => {
            structure::rename_structure(defs, structure, label)?;
            Ok(())
        }
        ModelChange::DeleteStructure { structure } => {
            structure::delete_structure(defs, structure)?;
            Ok(())
        }
        ModelChange::CreateGroup {
            structure,
            group,
            parent,
            label,
        } => {
            let id = GroupId::from_uuid(stable_uuid(&format!("group:{structure}:{group}")));
            structure::create_group(defs, id, structure, group, label, parent.as_deref(), false)?;
            Ok(())
        }
        ModelChange::RenameGroup {
            structure,
            group,
            label,
        } => {
            structure::rename_group(defs, structure, group, label)?;
            Ok(())
        }
        ModelChange::MoveGroup {
            structure,
            group,
            parent,
        } => {
            structure::move_group(defs, structure, group, parent.as_deref())?;
            Ok(())
        }
        ModelChange::DeleteGroup {
            structure,
            group,
            move_to,
        } => {
            structure::delete_group(defs, structure, group, move_to.as_deref())?;
            Ok(())
        }
        ModelChange::DuplicateGroup {
            structure,
            group,
            name,
        } => duplicate_group(defs, structure, group, name),
        ModelChange::RenameTypeKey { key, new_key } => {
            rename_entity(defs, crate::refs::RefKind::Type, key, new_key, None)
        }
        ModelChange::RenameAttributeKey { key, new_key } => {
            rename_entity(defs, crate::refs::RefKind::Attribute, key, new_key, None)
        }
        ModelChange::RenameStructureKey { structure, new_key } => rename_entity(
            defs,
            crate::refs::RefKind::Structure,
            structure,
            new_key,
            None,
        ),
        ModelChange::RenameGroupKey {
            structure,
            group,
            new_key,
        } => rename_entity(
            defs,
            crate::refs::RefKind::Group,
            group,
            new_key,
            Some(structure),
        ),
        ModelChange::RenameValueListKey { key, new_key } => {
            rename_entity(defs, crate::refs::RefKind::ValueList, key, new_key, None)
        }
        ModelChange::RenameUnitFamilyKey { key, new_key } => {
            rename_entity(defs, crate::refs::RefKind::UnitFamily, key, new_key, None)
        }
        ModelChange::CreateClass {
            structure,
            class_key,
            group,
            parent,
            attributes,
        } => {
            let attribute_ids = attributes
                .iter()
                .map(|key| attribute_id(defs, key))
                .collect::<Result<Vec<_>, _>>()?;
            let structure = structure_mut(defs, structure)?;
            if structure
                .classes
                .iter()
                .any(|item| eq_key(&item.key, class_key))
            {
                return Ok(());
            }
            let group_id = group_id(structure, group)?;
            let parent_id = match parent {
                Some(key) => Some(class_id(structure, key)?),
                None => None,
            };
            let class = Class {
                id: ClassId::from_uuid(stable_uuid(&format!(
                    "class:{}:{class_key}",
                    structure.key
                ))),
                key: class_key.clone(),
                group: group_id,
                parent: parent_id,
                attributes: attribute_ids,
            };
            structure.classes.push(class);
            Ok(())
        }
        ModelChange::CreateAttribute {
            key,
            label,
            kind,
            unit,
            multiple,
            languages,
            required,
            searchable,
            filterable,
            sortable,
            bounds,
        } => create_attribute(
            defs,
            AttributeDraft {
                key,
                label,
                kind,
                unit: unit.as_deref(),
                multiple: *multiple,
                languages: *languages,
                required: *required,
                searchable: *searchable,
                filterable: *filterable,
                sortable: *sortable,
                bounds,
            },
        ),
        ModelChange::MoveClass {
            structure,
            class_key,
            group,
        } => {
            let structure = structure_mut(defs, structure)?;
            let group_id = group_id(structure, group)?;
            let class = class_mut(structure, class_key)?;
            class.group = group_id;
            Ok(())
        }
        ModelChange::SetParent {
            structure,
            class_key,
            parent,
        } => {
            let structure = structure_mut(defs, structure)?;
            let parent_id = match parent {
                Some(key) => Some(class_id(structure, key)?),
                None => None,
            };
            let class = class_mut(structure, class_key)?;
            if parent_id == Some(class.id) {
                return Err(format!("class {} cannot parent itself", class.key).into());
            }
            class.parent = parent_id;
            Ok(())
        }
        ModelChange::AddAttribute {
            structure,
            class_key,
            attribute,
        } => {
            let attribute_id = attribute_id(defs, attribute)?;
            let structure = structure_mut(defs, structure)?;
            let class = class_mut(structure, class_key)?;
            if !class.attributes.contains(&attribute_id) {
                class.attributes.push(attribute_id);
            }
            Ok(())
        }
        ModelChange::RemoveAttribute {
            structure,
            class_key,
            attribute,
        } => {
            let attribute_id = attribute_id(defs, attribute)?;
            let structure = structure_mut(defs, structure)?;
            let class = class_mut(structure, class_key)?;
            class.attributes.retain(|id| *id != attribute_id);
            Ok(())
        }
        ModelChange::ReorderAttributes {
            structure,
            class_key,
            attributes,
        } => {
            let ids = attributes
                .iter()
                .map(|key| attribute_id(defs, key))
                .collect::<Result<Vec<_>, _>>()?;
            let structure = structure_mut(defs, structure)?;
            let class = class_mut(structure, class_key)?;
            if ids.len() != class.attributes.len()
                || ids.iter().any(|id| !class.attributes.contains(id))
            {
                return Err(
                    format!("reorder of {} must list the same attributes", class.key).into(),
                );
            }
            class.attributes = ids;
            Ok(())
        }
        ModelChange::AttachAttribute {
            attribute,
            type_key,
            structure,
            group,
        } => link_attribute(defs, attribute, type_key, structure, group, true)
            .map_err(ApplyError::from),
        ModelChange::DetachAttribute {
            attribute,
            type_key,
            structure,
            group,
        } => link_attribute(defs, attribute, type_key, structure, group, false)
            .map_err(ApplyError::from),
        ModelChange::UpdateType {
            key,
            language,
            singular,
            plural,
            structures,
            defaults,
        } => update_type(
            defs,
            key,
            language.as_deref(),
            singular.as_deref(),
            plural.as_deref(),
            structures.as_deref(),
            defaults.as_ref(),
        )
        .map_err(ApplyError::from),
        ModelChange::CreateType {
            key,
            label,
            plural,
            structures,
            defaults,
        } => create_type(
            defs,
            key,
            label,
            plural.as_deref(),
            structures.as_deref(),
            defaults.as_ref(),
        ),
        ModelChange::CreateLinkType {
            key,
            label,
            from_types,
            to_types,
            from_key,
            from_label,
            to_key,
            to_label,
            from_cardinality,
            to_cardinality,
            attributes,
        } => create_link_type(
            defs,
            key,
            label,
            from_types,
            to_types,
            from_key,
            from_label,
            to_key,
            to_label,
            from_cardinality,
            to_cardinality,
            attributes,
        ),
        ModelChange::UpdateLinkType {
            key,
            label,
            from_types,
            to_types,
            from_key,
            from_label,
            to_key,
            to_label,
            from_cardinality,
            to_cardinality,
            attributes,
        } => update_link_type(
            defs,
            key,
            label.as_deref(),
            from_types.as_deref(),
            to_types.as_deref(),
            from_key.as_deref(),
            from_label.as_deref(),
            to_key.as_deref(),
            to_label.as_deref(),
            from_cardinality.as_deref(),
            to_cardinality.as_deref(),
            attributes.as_deref(),
        ),
        ModelChange::DeleteLinkType { .. } => Ok(()),
        ModelChange::CreateValueList {
            key,
            label,
            options,
        } => create_value_list(defs, key, label, options),
        ModelChange::UpdateValueList {
            key,
            label,
            options,
        } => update_value_list(defs, key, label.as_deref(), options.as_deref()),
        ModelChange::CreateUnitFamily {
            key,
            dimension,
            base_unit,
            units,
        } => create_unit_family(defs, key, dimension.as_deref(), base_unit, units),
        ModelChange::UpdateUnitFamily {
            key,
            dimension,
            clear_dimension,
            base_unit,
            units,
        } => update_unit_family(
            defs,
            key,
            dimension.as_deref(),
            *clear_dimension,
            base_unit.as_deref(),
            units.as_deref(),
        ),
        ModelChange::ReorderTypeAttributes {
            type_key,
            attributes,
        } => reorder_type_attributes(defs, type_key, attributes),
        ModelChange::DeleteType { .. }
        | ModelChange::DeleteAttribute { .. }
        | ModelChange::DeleteValueList { .. }
        | ModelChange::DeleteValueOption { .. }
        | ModelChange::DeleteUnitFamily { .. }
        | ModelChange::DeleteUnit { .. } => Ok(()),
        ModelChange::SetStructures {
            type_key,
            structures,
        } => set_structures(defs, type_key, structures).map_err(ApplyError::from),
        ModelChange::UpsertAttribute { attribute } => upsert_posted_attribute(defs, attribute),
        ModelChange::UpsertType { object_type } => upsert_posted_type(defs, object_type),
        ModelChange::UpsertStructure { structure } => upsert_posted_structure(defs, structure),
        ModelChange::UpsertUnitFamily { family } => upsert_posted_family(defs, family),
        ModelChange::UpdateAttribute {
            key,
            label,
            kind,
            unit,
            clear_unit,
            languages,
            required,
            searchable,
            filterable,
            sortable,
            multiple,
            bounds,
        } => update_attribute(
            defs,
            AttributeUpdate {
                key,
                label,
                kind,
                unit,
                clear_unit: *clear_unit,
                languages: *languages,
                required: *required,
                searchable: *searchable,
                filterable: *filterable,
                sortable: *sortable,
                multiple: *multiple,
                bounds,
            },
        )
        .map_err(ApplyError::from),
    }
}

fn add_attribute_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (spec, class_name) = split_ci(rest, " to ")?;
    let (key, unit) = parse_attribute_spec(spec);
    if key.is_empty() || class_name.is_empty() {
        return None;
    }
    let mut steps = Vec::new();
    if !defs.attributes.iter().any(|item| eq_key(&item.key, &key)) {
        steps.push(ModelChange::CreateAttribute {
            key: key.clone(),
            label: key.clone(),
            kind: if unit.is_some() { "number" } else { "text" }.into(),
            unit,
            multiple: false,
            languages: false,
            required: false,
            searchable: false,
            filterable: true,
            sortable: false,
            bounds: AttributeBounds::default(),
        });
    }
    let (structure, class_key) = if let Some(found) = find_class(defs, class_name) {
        found
    } else {
        let structure = "unplaced".to_owned();
        steps.push(ModelChange::CreateStructure {
            key: structure.clone(),
            label: "Unplaced".into(),
        });
        steps.push(ModelChange::CreateGroup {
            structure: structure.clone(),
            group: "groups".into(),
            parent: None,
            label: "Groups".into(),
        });
        steps.push(ModelChange::CreateClass {
            structure: structure.clone(),
            class_key: class_name.to_owned(),
            group: "groups".into(),
            parent: None,
            attributes: vec![],
        });
        (structure, class_name.to_owned())
    };
    steps.push(ModelChange::AddAttribute {
        structure,
        class_key,
        attribute: key,
    });
    Some(steps)
}

fn add_group_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (name, parent) = match split_ci(rest, " under ") {
        Some((name, parent)) => (name, Some(parent)),
        None => (rest, None),
    };
    if name.is_empty() {
        return None;
    }
    let (structure, parent) = if let Some(parent_key) = parent {
        let (structure, group) = find_named_group(defs, parent_key)?;
        (structure.key.clone(), Some(group.key.clone()))
    } else {
        (sole_structure(defs)?.to_owned(), None)
    };
    Some(vec![ModelChange::CreateGroup {
        structure,
        group: name.to_owned(),
        parent,
        label: name.to_owned(),
    }])
}

fn rename_group_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (group, name) = split_ci(rest, " to ")?;
    let (structure, group) = find_named_group(defs, group)?;
    if name.is_empty() {
        return None;
    }
    Some(vec![ModelChange::RenameGroup {
        structure: structure.key.clone(),
        group: group.key.clone(),
        label: name.to_owned(),
    }])
}

fn move_group_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (group, parent) = split_ci(rest, " to ").or_else(|| split_ci(rest, " under "))?;
    let (structure, group) = find_named_group(defs, group)?;
    let parent = if parent.eq_ignore_ascii_case("root") {
        None
    } else {
        Some(find_named_group(defs, parent)?.1.key.clone())
    };
    Some(vec![ModelChange::MoveGroup {
        structure: structure.key.clone(),
        group: group.key.clone(),
        parent,
    }])
}

fn duplicate_group_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (group, name) = split_ci(rest, " as ")?;
    let (structure, group) = find_named_group(defs, group)?;
    if name.is_empty() {
        return None;
    }
    Some(vec![ModelChange::DuplicateGroup {
        structure: structure.key.clone(),
        group: group.key.clone(),
        name: name.to_owned(),
    }])
}

fn delete_group_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (structure, group) = find_named_group(defs, rest)?;
    Some(vec![ModelChange::DeleteGroup {
        structure: structure.key.clone(),
        group: group.key.clone(),
        move_to: None,
    }])
}

fn sole_structure(defs: &DefinitionSet) -> Option<&str> {
    let mut keys = defs.structures.iter().map(|item| item.key.as_str());
    let first = keys.next()?;
    if keys.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn find_named_group<'a>(defs: &'a DefinitionSet, key: &str) -> Option<(&'a Structure, &'a Group)> {
    let mut found = None;
    for structure in &defs.structures {
        for group in &structure.groups {
            if eq_key(&group.key, key) {
                if found.is_some() {
                    return None;
                }
                found = Some((structure, group));
            }
        }
    }
    found
}

fn duplicate_group(
    defs: &mut DefinitionSet,
    structure_key: &str,
    group: &str,
    name: &str,
) -> Result<(), ApplyError> {
    if name.is_empty() {
        return Err("group key is empty".into());
    }
    let labels = label_map(defs, name);
    let structure = structure_mut(defs, structure_key)?;
    if structure.groups.iter().any(|item| eq_key(&item.key, name)) {
        return Err(format!("group {name} already exists in {}", structure.key).into());
    }
    let source = structure
        .groups
        .iter()
        .find(|item| eq_key(&item.key, group))
        .cloned()
        .ok_or_else(|| format!("unknown group {group} in {}", structure.key))?;
    let structure_key = structure.key.clone();
    structure.groups.push(Group {
        id: GroupId::from_uuid(stable_uuid(&format!("group:{structure_key}:{name}:copy"))),
        key: name.to_owned(),
        labels,
        parent: source.parent,
        attributes: source.attributes,
    });
    Ok(())
}

fn rename_entity(
    defs: &mut DefinitionSet,
    kind: crate::refs::RefKind,
    token: &str,
    new_key: &str,
    structure: Option<&str>,
) -> Result<(), ApplyError> {
    let new_key = new_key.trim();
    crate::refs::check_new_key(new_key)?;
    let current = crate::refs::resolve(defs, kind, token, structure)?;
    if current.key.eq_ignore_ascii_case(new_key) {
        return Ok(());
    }
    if crate::refs::duplicate_key(defs, kind, new_key, structure) {
        return Err(format!("{} key {new_key} already exists", kind_word(kind)).into());
    }
    let old_key = current.key.clone();
    match kind {
        crate::refs::RefKind::Attribute => {
            let id = crate::refs::attribute_id(defs, token)?;
            if crate::standard::is_locked_standard(defs, id) {
                return Err(locked_attribute(token).into());
            }
            let attribute = defs
                .attributes
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| format!("unknown attribute {token}"))?;
            attribute.key = new_key.to_owned();
        }
        crate::refs::RefKind::Type => {
            let id = crate::refs::type_id(defs, token)?;
            {
                let object_type = defs
                    .types
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or_else(|| format!("unknown type {token}"))?;
                object_type.key = new_key.to_owned();
            }
            crate::refs::retarget_key_strings(defs, kind, &old_key, new_key);
        }
        crate::refs::RefKind::Structure => {
            let id = crate::refs::structure_id(defs, token)?;
            let structure = defs
                .structures
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| format!("unknown structure {token}"))?;
            structure.key = new_key.to_owned();
        }
        crate::refs::RefKind::Group => {
            let structure_token = structure.ok_or("missing structure")?;
            let resolved = crate::refs::resolve(defs, kind, token, Some(structure_token))?;
            let group_id = GroupId::from_uuid(
                uuid::Uuid::parse_str(&resolved.id)
                    .map_err(|_| format!("unknown group {token}"))?,
            );
            let structure = structure_mut(defs, structure_token)?;
            let group = structure
                .groups
                .iter_mut()
                .find(|group| group.id == group_id)
                .ok_or_else(|| format!("unknown group {token}"))?;
            group.key = new_key.to_owned();
        }
        crate::refs::RefKind::ValueList => {
            let id = crate::refs::value_list_id(defs, token)?;
            {
                let list = defs
                    .value_lists
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or_else(|| format!("unknown value list {token}"))?;
                list.key = new_key.to_owned();
            }
            crate::refs::retarget_key_strings(defs, kind, &old_key, new_key);
        }
        crate::refs::RefKind::UnitFamily => {
            let id = crate::refs::unit_family_id(defs, token)?;
            let family = defs
                .unit_families
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| format!("unknown unit family {token}"))?;
            family.key = new_key.to_owned();
        }
    }
    Ok(())
}

fn kind_word(kind: crate::refs::RefKind) -> &'static str {
    match kind {
        crate::refs::RefKind::Type => "type",
        crate::refs::RefKind::Attribute => "attribute",
        crate::refs::RefKind::Structure => "structure",
        crate::refs::RefKind::Group => "group",
        crate::refs::RefKind::ValueList => "value list",
        crate::refs::RefKind::UnitFamily => "unit family",
    }
}

fn rename_key_sentence(
    defs: &DefinitionSet,
    kind: crate::refs::RefKind,
    rest: &str,
    structure: Option<&str>,
) -> Option<Vec<ModelChange>> {
    let (token, new_key) = split_ci(rest, " to ")?;
    if token.is_empty() || new_key.is_empty() {
        return None;
    }
    if crate::refs::resolve(defs, kind, token, structure).is_err() {
        return None;
    }
    let step = match kind {
        crate::refs::RefKind::Attribute => ModelChange::RenameAttributeKey {
            key: token.to_owned(),
            new_key: new_key.to_owned(),
        },
        crate::refs::RefKind::Type => ModelChange::RenameTypeKey {
            key: token.to_owned(),
            new_key: new_key.to_owned(),
        },
        crate::refs::RefKind::Structure => ModelChange::RenameStructureKey {
            structure: token.to_owned(),
            new_key: new_key.to_owned(),
        },
        crate::refs::RefKind::ValueList => ModelChange::RenameValueListKey {
            key: token.to_owned(),
            new_key: new_key.to_owned(),
        },
        crate::refs::RefKind::UnitFamily => ModelChange::RenameUnitFamilyKey {
            key: token.to_owned(),
            new_key: new_key.to_owned(),
        },
        crate::refs::RefKind::Group => return None,
    };
    Some(vec![step])
}

fn rename_group_key_sentence(defs: &DefinitionSet, rest: &str) -> Option<Vec<ModelChange>> {
    let (left, new_key) = split_ci(rest, " to ")?;
    if new_key.is_empty() {
        return None;
    }
    let (structure, group) = if let Some((structure, group)) = split_ci(left, " ") {
        if crate::refs::resolve(defs, crate::refs::RefKind::Group, group, Some(structure)).is_ok() {
            (structure.to_owned(), group.to_owned())
        } else {
            (String::new(), left.to_owned())
        }
    } else {
        (String::new(), left.to_owned())
    };
    let (structure, group) = if structure.is_empty() {
        let resolved =
            crate::refs::resolve(defs, crate::refs::RefKind::Group, group.as_str(), None).ok()?;
        let structure = defs.structures.iter().find(|item| {
            item.groups
                .iter()
                .any(|candidate| candidate.id.as_uuid().to_string() == resolved.id)
        })?;
        (structure.key.clone(), resolved.key)
    } else {
        (structure, group)
    };
    Some(vec![ModelChange::RenameGroupKey {
        structure,
        group,
        new_key: new_key.to_owned(),
    }])
}

fn remove_attribute_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (attribute, class_name) = split_ci(rest, " from ")?;
    let (structure, class_key) = find_class(defs, class_name)?;
    Some(vec![ModelChange::RemoveAttribute {
        structure,
        class_key,
        attribute: attribute.to_owned(),
    }])
}

fn move_command(rest: &str, defs: &DefinitionSet) -> Option<Vec<ModelChange>> {
    let (class_name, group) = split_ci(rest, " to ")?;
    let (structure, class_key) = find_class(defs, class_name)?;
    Some(vec![ModelChange::MoveClass {
        structure,
        class_key,
        group: group.to_owned(),
    }])
}

fn parse_attribute_spec(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();
    if let Some(open) = spec.rfind('(') {
        let key = spec[..open].trim().to_owned();
        let unit = spec[open + 1..]
            .trim()
            .trim_end_matches(')')
            .trim()
            .to_owned();
        if unit.is_empty() {
            (key, None)
        } else {
            (key, Some(unit))
        }
    } else {
        (spec.to_owned(), None)
    }
}

fn find_class(defs: &DefinitionSet, name: &str) -> Option<(String, String)> {
    for structure in &defs.structures {
        if let Some(class) = structure
            .classes
            .iter()
            .find(|class| eq_key(&class.key, name))
        {
            return Some((structure.key.clone(), class.key.clone()));
        }
    }
    None
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let lower = text.to_lowercase();
    let prefix = prefix.to_lowercase();
    if lower.starts_with(&prefix) {
        Some(text[prefix.len()..].trim())
    } else {
        None
    }
}

fn split_ci<'a>(text: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let index = lower.find(&needle_lower)?;
    Some((text[..index].trim(), text[index + needle.len()..].trim()))
}

fn eq_key(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn type_mut<'a>(defs: &'a mut DefinitionSet, key: &str) -> Result<&'a mut ObjectType, String> {
    defs.types
        .iter_mut()
        .find(|item| eq_key(&item.key, key))
        .ok_or_else(|| format!("unknown type {key}"))
}

fn structure_mut<'a>(defs: &'a mut DefinitionSet, key: &str) -> Result<&'a mut Structure, String> {
    defs.structures
        .iter_mut()
        .find(|item| eq_key(&item.key, key))
        .ok_or_else(|| format!("unknown structure {key}"))
}

fn group_mut<'a>(structure: &'a mut Structure, key: &str) -> Result<&'a mut Group, String> {
    let structure_key = structure.key.clone();
    structure
        .groups
        .iter_mut()
        .find(|group| eq_key(&group.key, key))
        .ok_or_else(|| format!("unknown group {key} in {structure_key}"))
}

fn group_id(structure: &Structure, key: &str) -> Result<GroupId, String> {
    structure
        .groups
        .iter()
        .find(|group| eq_key(&group.key, key))
        .map(|group| group.id)
        .ok_or_else(|| format!("unknown group {key} in {}", structure.key))
}

fn class_id(structure: &Structure, key: &str) -> Result<ClassId, String> {
    structure
        .classes
        .iter()
        .find(|class| eq_key(&class.key, key))
        .map(|class| class.id)
        .ok_or_else(|| format!("unknown class {key} in {}", structure.key))
}

fn class_mut<'a>(structure: &'a mut Structure, key: &str) -> Result<&'a mut Class, String> {
    let structure_key = structure.key.clone();
    structure
        .classes
        .iter_mut()
        .find(|class| eq_key(&class.key, key))
        .ok_or_else(|| format!("unknown class {key} in {structure_key}"))
}

fn attribute_id(defs: &DefinitionSet, key: &str) -> Result<AttributeId, String> {
    defs.attributes
        .iter()
        .find(|attribute| eq_key(&attribute.key, key))
        .map(|attribute| attribute.id)
        .ok_or_else(|| format!("unknown attribute {key}"))
}

fn family_for_unit(defs: &DefinitionSet, unit: &str) -> Result<UnitFamilyId, String> {
    if let Some(family) = defs
        .unit_families
        .iter()
        .find(|family| eq_key(&family.key, unit) || eq_key(&family.base_unit, unit))
    {
        return Ok(family.id);
    }
    defs.unit_families
        .iter()
        .find(|family| family.units.iter().any(|item| eq_key(&item.code, unit)))
        .map(|family| family.id)
        .ok_or_else(|| format!("unknown unit {unit}"))
}

fn kind_token(kind: &AttributeKind) -> &'static str {
    match kind {
        AttributeKind::Text { .. } => "text",
        AttributeKind::Number { .. } => "number",
        AttributeKind::Range { .. } => "range",
        AttributeKind::ValueList { .. } => "value_list",
        AttributeKind::Boolean => "boolean",
        AttributeKind::Date => "date",
        AttributeKind::ObjectRef { .. } => "object",
        AttributeKind::FileRef { .. } => "file",
    }
}

fn link_attribute(
    defs: &mut DefinitionSet,
    attribute: &str,
    type_key: &Option<String>,
    structure_key: &Option<String>,
    group_key: &Option<String>,
    attach: bool,
) -> Result<(), String> {
    let attribute_id = attribute_id(defs, attribute)?;
    if type_key.is_none() && (structure_key.is_none() || group_key.is_none()) {
        return Err("name a type, or a structure and a group".into());
    }
    if !attach && crate::standard::is_locked_standard(defs, attribute_id) && type_key.is_some() {
        return Err(format!("attribute {attribute} is a standard attribute"));
    }
    if let Some(type_key) = type_key {
        let object_type = type_mut(defs, type_key)?;
        if !attach && object_type.key_attribute == attribute_id {
            return Err(format!(
                "attribute {attribute} is the key of type {}",
                object_type.key
            ));
        }
        if attach {
            if !object_type.attributes.contains(&attribute_id) {
                object_type.attributes.push(attribute_id);
            }
        } else {
            object_type.attributes.retain(|id| *id != attribute_id);
        }
    }
    if let (Some(structure_key), Some(group_key)) = (structure_key, group_key) {
        let structure = structure_mut(defs, structure_key)?;
        let group = group_mut(structure, group_key)?;
        if attach {
            if !group.attributes.contains(&attribute_id) {
                group.attributes.push(attribute_id);
            }
        } else {
            group.attributes.retain(|id| *id != attribute_id);
        }
    }
    Ok(())
}

fn update_type(
    defs: &mut DefinitionSet,
    key: &str,
    language: Option<&str>,
    singular: Option<&str>,
    plural: Option<&str>,
    structures: Option<&[String]>,
    defaults: Option<&BTreeMap<String, String>>,
) -> Result<(), String> {
    if let Some(structures) = structures {
        set_structures(defs, key, structures)?;
    }
    let object_type = type_mut(defs, key)?;
    let tag = LanguageTag::parse(language.unwrap_or("en")).map_err(|error| error.to_string())?;
    if let Some(singular) = singular {
        let text = singular.trim();
        if text.is_empty() {
            object_type.singular.remove(&tag);
            object_type.labels.remove(&tag);
        } else {
            object_type.singular.insert(tag.clone(), text.to_owned());
            object_type.labels.insert(tag.clone(), text.to_owned());
        }
    }
    if let Some(plural) = plural {
        let text = plural.trim();
        if text.is_empty() {
            object_type.plural.remove(&tag);
        } else {
            object_type.plural.insert(tag, text.to_owned());
        }
    }
    if let Some(defaults) = defaults {
        object_type.defaults.clone_from(defaults);
    }
    Ok(())
}

struct AttributeUpdate<'a> {
    key: &'a str,
    label: &'a Option<String>,
    kind: &'a Option<String>,
    unit: &'a Option<String>,
    clear_unit: bool,
    languages: Option<bool>,
    required: Option<bool>,
    searchable: Option<bool>,
    filterable: Option<bool>,
    sortable: Option<bool>,
    multiple: Option<bool>,
    bounds: &'a AttributeBounds,
}

fn update_attribute(defs: &mut DefinitionSet, patch: AttributeUpdate<'_>) -> Result<(), String> {
    let AttributeUpdate {
        key,
        label,
        kind,
        unit,
        clear_unit,
        languages,
        required,
        searchable,
        filterable,
        sortable,
        multiple,
        bounds,
    } = patch;
    let mut next_kind = {
        let current = defs
            .attributes
            .iter()
            .find(|item| eq_key(&item.key, key))
            .ok_or_else(|| format!("unknown attribute {key}"))?;
        current.kind.clone()
    };
    if let Some(kind_name) = kind
        && kind_token(&next_kind) != kind_name.to_lowercase().as_str()
    {
        next_kind = attribute_kind(kind_name, next_kind.unit_family_id())?;
    }
    if clear_unit {
        set_unit_family(&mut next_kind, None)?;
    } else if let Some(unit) = unit {
        set_unit_family(&mut next_kind, Some(family_for_unit(defs, unit)?))?;
    }
    apply_bounds(&mut next_kind, bounds, defs)?;
    let kind_changed = {
        let current = defs
            .attributes
            .iter()
            .find(|item| eq_key(&item.key, key))
            .ok_or_else(|| format!("unknown attribute {key}"))?;
        current.kind != next_kind
    };
    let label_tag = defs
        .languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned());
    let locked = defs
        .attributes
        .iter()
        .find(|item| eq_key(&item.key, key))
        .map(|item| crate::standard::is_locked_standard(defs, item.id))
        .unwrap_or(false);
    let bounds_set = bounds != &AttributeBounds::default();
    if locked
        && (kind_changed
            || languages.is_some()
            || required.is_some()
            || searchable.is_some()
            || filterable.is_some()
            || sortable.is_some()
            || multiple.is_some()
            || bounds_set)
    {
        return Err(locked_attribute(key));
    }
    let attribute = defs
        .attributes
        .iter_mut()
        .find(|item| eq_key(&item.key, key))
        .ok_or_else(|| format!("unknown attribute {key}"))?;
    if let (Some(label), Some(tag)) = (label, label_tag) {
        attribute.labels.insert(tag, label.clone());
    }
    attribute.kind = next_kind;
    if let Some(value) = multiple {
        attribute.multiple = value;
    }
    if let Some(value) = languages {
        attribute.language_dependent = value;
    }
    if let Some(value) = required {
        attribute.required = value;
    }
    if let Some(value) = searchable {
        attribute.index.searchable = value;
    }
    if let Some(value) = filterable {
        attribute.index.filterable = value;
    }
    if let Some(value) = sortable {
        attribute.index.sortable = value;
    }
    Ok(())
}

struct AttributeDraft<'a> {
    key: &'a str,
    label: &'a str,
    kind: &'a str,
    unit: Option<&'a str>,
    multiple: bool,
    languages: bool,
    required: bool,
    searchable: bool,
    filterable: bool,
    sortable: bool,
    bounds: &'a AttributeBounds,
}

fn create_attribute(defs: &mut DefinitionSet, draft: AttributeDraft<'_>) -> Result<(), ApplyError> {
    let AttributeDraft {
        key,
        label,
        kind,
        unit,
        multiple,
        languages,
        required,
        searchable,
        filterable,
        sortable,
        bounds,
    } = draft;
    let unit_family = match unit {
        Some(unit) => Some(family_for_unit(defs, unit)?),
        None => None,
    };
    let mut wanted = attribute_kind(kind, unit_family)?;
    apply_bounds(&mut wanted, bounds, defs)?;
    if let Some(existing) = defs.attributes.iter().find(|item| item.key == key) {
        if !wanted.same_type_and_unit(&existing.kind) {
            return Err(
                format!("attribute {key} already exists with a different type or unit").into(),
            );
        }
        return Ok(());
    }
    defs.attributes.push(Attribute {
        id: AttributeId::from_uuid(stable_uuid(&format!("attribute:{key}"))),
        key: key.to_owned(),
        labels: label_map(defs, if label.is_empty() { key } else { label }),
        language_dependent: languages,
        multiple,
        required,
        kind: wanted,
        index: crate::definition::IndexFlags {
            searchable,
            filterable,
            sortable,
        },
    });
    Ok(())
}

fn create_type(
    defs: &mut DefinitionSet,
    key: &str,
    label: &str,
    plural: Option<&str>,
    structures: Option<&[String]>,
    defaults: Option<&BTreeMap<String, String>>,
) -> Result<(), ApplyError> {
    let key = key.trim();
    crate::refs::check_new_key(key)?;
    if defs.types.iter().any(|item| eq_key(&item.key, key)) {
        return Ok(());
    }
    crate::standard::ensure_standard_rows(defs);
    let mut ids = Vec::new();
    if let Some(structures) = structures {
        for token in structures {
            let id = crate::refs::structure_id(defs, token)
                .map_err(|_| format!("unknown structure {token}"))?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let caption = if label.trim().is_empty() {
        key
    } else {
        label.trim()
    };
    let mut object_type = ObjectType {
        id: crate::id::TypeId::from_uuid(stable_uuid(&format!("type:{key}"))),
        key: key.to_owned(),
        labels: label_map(defs, caption),
        singular: label_map(defs, caption),
        plural: plural
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| label_map(defs, text))
            .unwrap_or_default(),
        key_attribute: crate::standard::standard_ids().number,
        attributes: Vec::new(),
        structures: Some(ids),
        defaults: defaults.cloned().unwrap_or_default(),
        kind: Default::default(),
        link: None,
    };
    crate::standard::attach_standard_attributes(&mut object_type);
    defs.types.push(object_type);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create_link_type(
    defs: &mut DefinitionSet,
    key: &str,
    label: &str,
    from_types: &[String],
    to_types: &[String],
    from_key: &str,
    from_label: &str,
    to_key: &str,
    to_label: &str,
    from_cardinality: &str,
    to_cardinality: &str,
    attributes: &[String],
) -> Result<(), ApplyError> {
    let key = key.trim();
    crate::refs::check_new_key(key)?;
    if defs.types.iter().any(|item| eq_key(&item.key, key)) {
        return Ok(());
    }
    let from_key = side_key(from_key)?;
    let to_key = side_key(to_key)?;
    if from_key.eq_ignore_ascii_case(&to_key) {
        return Err("the two side keys have to differ".into());
    }
    let from_ids = resolve_entity_types(defs, from_types, "from")?;
    let to_ids = resolve_entity_types(defs, to_types, "to")?;
    let from_cardinality = crate::link::parse_cardinality(from_cardinality)?;
    let to_cardinality = crate::link::parse_cardinality(to_cardinality)?;
    ensure_text_attributes(defs, attributes)?;
    let mut attached = Vec::new();
    for attribute in attributes {
        let attribute = attribute.trim();
        if let Some(found) = defs
            .attributes
            .iter()
            .find(|item| eq_key(&item.key, attribute) && !attached.contains(&item.id))
        {
            attached.push(found.id);
        }
    }
    let caption = if label.trim().is_empty() {
        key
    } else {
        label.trim()
    };
    crate::standard::ensure_standard_rows(defs);
    let mut object_type = ObjectType {
        id: crate::id::TypeId::from_uuid(stable_uuid(&format!("type:{key}"))),
        key: key.to_owned(),
        labels: label_map(defs, caption),
        singular: label_map(defs, caption),
        plural: label_map(defs, caption),
        key_attribute: crate::standard::standard_ids().created_at,
        attributes: attached,
        structures: Some(Vec::new()),
        defaults: BTreeMap::new(),
        kind: crate::definition::TypeKind::Link,
        link: Some(crate::definition::LinkSpec {
            from_types: from_ids,
            to_types: to_ids,
            from_label: side_label(defs, &from_key, from_label),
            to_label: side_label(defs, &to_key, to_label),
            from_cardinality,
            to_cardinality,
        }),
    };
    crate::standard::attach_standard_attributes(&mut object_type);
    defs.types.push(object_type);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn update_link_type(
    defs: &mut DefinitionSet,
    key: &str,
    label: Option<&str>,
    from_types: Option<&[String]>,
    to_types: Option<&[String]>,
    from_key: Option<&str>,
    from_label: Option<&str>,
    to_key: Option<&str>,
    to_label: Option<&str>,
    from_cardinality: Option<&str>,
    to_cardinality: Option<&str>,
    attributes: Option<&[String]>,
) -> Result<(), ApplyError> {
    let id = crate::refs::type_id(defs, key)?;
    let current = defs
        .types
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("unknown type {key}"))?;
    if !current.is_link() {
        return Err(format!("{key} is not a link type").into());
    }
    let mut spec = current
        .link
        .clone()
        .ok_or_else(|| format!("link type {key} has no ends"))?;
    if let Some(types) = from_types {
        spec.from_types = resolve_entity_types(defs, types, "from")?;
    }
    if let Some(types) = to_types {
        spec.to_types = resolve_entity_types(defs, types, "to")?;
    }
    if let Some(value) = from_key {
        spec.from_label.key = side_key(value)?;
    }
    if let Some(value) = to_key {
        spec.to_label.key = side_key(value)?;
    }
    if spec.from_label.key.eq_ignore_ascii_case(&spec.to_label.key) {
        return Err("the two side keys have to differ".into());
    }
    if let Some(value) = from_label {
        spec.from_label.labels = label_map(defs, value.trim());
    }
    if let Some(value) = to_label {
        spec.to_label.labels = label_map(defs, value.trim());
    }
    if let Some(value) = from_cardinality {
        spec.from_cardinality = crate::link::parse_cardinality(value)?;
    }
    if let Some(value) = to_cardinality {
        spec.to_cardinality = crate::link::parse_cardinality(value)?;
    }
    let attached = if let Some(attributes) = attributes {
        ensure_text_attributes(defs, attributes)?;
        let mut ids = Vec::new();
        for attribute in attributes {
            let attribute = attribute.trim();
            if let Some(found) = defs
                .attributes
                .iter()
                .find(|item| eq_key(&item.key, attribute))
                && !ids.contains(&found.id)
            {
                ids.push(found.id);
            }
        }
        Some(ids)
    } else {
        None
    };
    let renamed = label
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|label| label_map(defs, label));
    let object_type = defs
        .types
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("unknown type {key}"))?;
    object_type.link = Some(spec);
    if let Some(labels) = renamed {
        object_type.labels = labels.clone();
        object_type.singular = labels.clone();
        object_type.plural = labels;
    }
    if let Some(ids) = attached {
        let standard = [
            crate::standard::standard_ids().created_at,
            crate::standard::standard_ids().updated_at,
            crate::standard::standard_ids().updated_by,
        ];
        object_type.attributes.retain(|id| standard.contains(id));
        for id in ids {
            if !object_type.attributes.contains(&id) {
                object_type.attributes.push(id);
            }
        }
    }
    crate::standard::attach_standard_attributes(object_type);
    Ok(())
}

fn side_key(key: &str) -> Result<String, ApplyError> {
    let key = key.trim();
    crate::refs::check_new_key(key)?;
    Ok(key.to_owned())
}

fn side_label(defs: &DefinitionSet, key: &str, label: &str) -> crate::definition::LinkLabel {
    let text = if label.trim().is_empty() {
        key
    } else {
        label.trim()
    };
    crate::definition::LinkLabel {
        key: key.to_owned(),
        labels: label_map(defs, text),
    }
}

fn resolve_entity_types(
    defs: &DefinitionSet,
    tokens: &[String],
    side: &str,
) -> Result<Vec<crate::id::TypeId>, ApplyError> {
    let mut ids = Vec::new();
    for token in tokens {
        let id = crate::refs::type_id(defs, token)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    crate::link::check_ends(defs, &ids, side)?;
    Ok(ids)
}

fn ensure_text_attributes(defs: &mut DefinitionSet, keys: &[String]) -> Result<(), ApplyError> {
    let bounds = AttributeBounds::default();
    for key in keys {
        let key = key.trim();
        if key.is_empty() {
            return Err("attribute key is empty".into());
        }
        if defs.attributes.iter().any(|item| eq_key(&item.key, key)) {
            continue;
        }
        create_attribute(
            defs,
            AttributeDraft {
                key,
                label: key,
                kind: "text",
                unit: None,
                multiple: false,
                languages: false,
                required: false,
                searchable: false,
                filterable: false,
                sortable: false,
                bounds: &bounds,
            },
        )?;
    }
    Ok(())
}

fn create_value_list(
    defs: &mut DefinitionSet,
    key: &str,
    label: &str,
    options: &[NamedOption],
) -> Result<(), ApplyError> {
    let key = key.trim();
    crate::refs::check_new_key(key)?;
    if defs.value_lists.iter().any(|item| eq_key(&item.key, key)) {
        return Ok(());
    }
    let caption = if label.trim().is_empty() {
        key
    } else {
        label.trim()
    };
    defs.value_lists.push(crate::definition::SharedValueList {
        id: crate::id::ValueListId::from_uuid(stable_uuid(&format!("value_list:{key}"))),
        key: key.to_owned(),
        labels: label_map(defs, caption),
        options: named_options(defs, options)?,
    });
    Ok(())
}

fn update_value_list(
    defs: &mut DefinitionSet,
    key: &str,
    label: Option<&str>,
    options: Option<&[NamedOption]>,
) -> Result<(), ApplyError> {
    let id = crate::refs::value_list_id(defs, key)?;
    let built = options
        .map(|options| named_options(defs, options))
        .transpose()?;
    let list = defs
        .value_lists
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("unknown value list {key}"))?;
    if let Some(label) = label.map(str::trim).filter(|text| !text.is_empty()) {
        let tag = list
            .labels
            .keys()
            .next()
            .cloned()
            .or_else(|| defs.languages.first().cloned());
        if let Some(tag) = tag {
            list.labels.insert(tag, label.to_owned());
        }
    }
    if let Some(options) = built {
        list.options = options;
    }
    Ok(())
}

fn create_unit_family(
    defs: &mut DefinitionSet,
    key: &str,
    dimension: Option<&str>,
    base_unit: &str,
    units: &[UnitSpec],
) -> Result<(), ApplyError> {
    let key = key.trim();
    crate::refs::check_new_key(key)?;
    if defs.unit_families.iter().any(|item| eq_key(&item.key, key)) {
        return Ok(());
    }
    let base_unit = base_unit.trim();
    reject_empty_base(base_unit)?;
    defs.unit_families.push(crate::definition::UnitFamily {
        id: UnitFamilyId::from_uuid(stable_uuid(&format!("unit_family:{key}"))),
        key: key.to_owned(),
        dimension: dimension
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        base_unit: base_unit.to_owned(),
        units: unit_specs(base_unit, units)?,
    });
    Ok(())
}

fn update_unit_family(
    defs: &mut DefinitionSet,
    key: &str,
    dimension: Option<&str>,
    clear_dimension: bool,
    base_unit: Option<&str>,
    units: Option<&[UnitSpec]>,
) -> Result<(), ApplyError> {
    let id = crate::refs::unit_family_id(defs, key)?;
    let family = defs
        .unit_families
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("unknown unit family {key}"))?;
    if clear_dimension {
        family.dimension = None;
    }
    if let Some(dimension) = dimension.map(str::trim).filter(|text| !text.is_empty()) {
        family.dimension = Some(dimension.to_owned());
    }
    if let Some(base_unit) = base_unit.map(str::trim).filter(|text| !text.is_empty()) {
        family.base_unit = base_unit.to_owned();
    }
    if let Some(units) = units {
        let base = family.base_unit.clone();
        family.units = unit_specs(&base, units)?;
    }
    if !family
        .units
        .iter()
        .any(|unit| unit.code == family.base_unit)
    {
        family.units.insert(
            0,
            Unit {
                code: family.base_unit.clone(),
                factor: Decimal::ONE,
            },
        );
    }
    Ok(())
}

fn locked_attribute(key: &str) -> String {
    format!("attribute {key} is a standard attribute")
}

fn reject_empty_base(base_unit: &str) -> Result<(), ApplyError> {
    if base_unit.trim().is_empty() {
        Err("base unit is empty".into())
    } else {
        Ok(())
    }
}

fn duplicate_key_message(kind: crate::refs::RefKind, key: &str) -> String {
    format!("{} key {key} already exists", kind_word(kind))
}

fn rules_changed(before: &Attribute, after: &Attribute) -> bool {
    before.required != after.required
        || before.multiple != after.multiple
        || before.language_dependent != after.language_dependent
        || before.index != after.index
        || before.kind != after.kind
}

fn upsert_posted_attribute(defs: &mut DefinitionSet, posted: &Attribute) -> Result<(), ApplyError> {
    if let Some(existing) = defs
        .attributes
        .iter()
        .find(|item| item.id == posted.id)
        .cloned()
    {
        if !eq_key(&existing.key, &posted.key) {
            rename_entity(
                defs,
                crate::refs::RefKind::Attribute,
                &existing.key,
                &posted.key,
                None,
            )?;
        }
        if rules_changed(&existing, posted)
            && crate::standard::is_locked_standard(defs, existing.id)
        {
            return Err(locked_attribute(&existing.key).into());
        }
    } else if defs
        .attributes
        .iter()
        .any(|item| eq_key(&item.key, &posted.key))
    {
        return Err(duplicate_key_message(crate::refs::RefKind::Attribute, &posted.key).into());
    }
    crate::edit::upsert_attribute(defs, posted.clone());
    Ok(())
}

fn upsert_posted_type(defs: &mut DefinitionSet, posted: &ObjectType) -> Result<(), ApplyError> {
    let new_type = !defs.types.iter().any(|item| item.id == posted.id);
    if let Some(existing) = defs.types.iter().find(|item| item.id == posted.id).cloned() {
        if !eq_key(&existing.key, &posted.key) {
            rename_entity(
                defs,
                crate::refs::RefKind::Type,
                &existing.key,
                &posted.key,
                None,
            )?;
        }
    } else {
        crate::refs::check_new_key(&posted.key)?;
        if defs.types.iter().any(|item| eq_key(&item.key, &posted.key)) {
            return Err(duplicate_key_message(crate::refs::RefKind::Type, &posted.key).into());
        }
    }
    if posted.is_link() {
        let spec = posted
            .link
            .as_ref()
            .ok_or_else(|| format!("link type {} has no ends", posted.key))?;
        if spec.from_label.key.eq_ignore_ascii_case(&spec.to_label.key) {
            return Err("the two side keys have to differ".into());
        }
        side_key(&spec.from_label.key)?;
        side_key(&spec.to_label.key)?;
        crate::link::check_ends(defs, &spec.from_types, "from")?;
        crate::link::check_ends(defs, &spec.to_types, "to")?;
    }
    let mut object_type = posted.clone();
    if new_type {
        // Same install as [`create_type`]: rows with the stable ids, then pinned on the type.
        crate::standard::ensure_standard_rows(defs);
        crate::standard::attach_standard_attributes(&mut object_type);
    } else if standard_rows_present(defs) {
        // The rows already have their ids. Pin them again so an update cannot drop or repeat them.
        crate::standard::attach_standard_attributes(&mut object_type);
    }
    crate::edit::upsert_type(defs, object_type);
    Ok(())
}

fn standard_rows_present(defs: &DefinitionSet) -> bool {
    let ids = crate::standard::standard_ids();
    [
        ids.number,
        ids.name,
        ids.created_at,
        ids.updated_at,
        ids.updated_by,
    ]
    .into_iter()
    .all(|id| defs.attributes.iter().any(|attribute| attribute.id == id))
}

/// Groups a structure upsert removes from a structure that already exists.
///
/// Delete steps are omitted. Those unplace objects. The save path counts the
/// groups returned here and refuses the upsert, dry run included, when any
/// still hold objects.
pub fn groups_removed_by_structure_upserts(
    defs: &DefinitionSet,
    changes: &[ModelChange],
) -> Vec<GroupId> {
    let mut removed = Vec::new();
    for change in changes {
        let ModelChange::UpsertStructure { structure } = change else {
            continue;
        };
        let Some(previous) = defs.structures.iter().find(|item| item.id == structure.id) else {
            continue;
        };
        for group in &previous.groups {
            if structure.groups.iter().any(|item| item.id == group.id) {
                continue;
            }
            if !removed.contains(&group.id) {
                removed.push(group.id);
            }
        }
    }
    removed
}

fn upsert_posted_structure(defs: &mut DefinitionSet, posted: &Structure) -> Result<(), ApplyError> {
    crate::structure::check_posted_structure(posted)?;
    if let Some(existing) = defs
        .structures
        .iter()
        .find(|item| item.id == posted.id)
        .cloned()
    {
        if !eq_key(&existing.key, &posted.key) {
            rename_entity(
                defs,
                crate::refs::RefKind::Structure,
                &existing.key,
                &posted.key,
                None,
            )?;
        }
    } else if defs
        .structures
        .iter()
        .any(|item| eq_key(&item.key, &posted.key))
    {
        return Err(duplicate_key_message(crate::refs::RefKind::Structure, &posted.key).into());
    }
    crate::edit::upsert_structure(defs, posted.clone());
    Ok(())
}

fn upsert_posted_family(defs: &mut DefinitionSet, posted: &UnitFamily) -> Result<(), ApplyError> {
    if let Some(existing) = defs
        .unit_families
        .iter()
        .find(|item| item.id == posted.id)
        .cloned()
    {
        if !eq_key(&existing.key, &posted.key) {
            rename_entity(
                defs,
                crate::refs::RefKind::UnitFamily,
                &existing.key,
                &posted.key,
                None,
            )?;
        }
    } else {
        crate::refs::check_new_key(&posted.key)?;
        if defs
            .unit_families
            .iter()
            .any(|item| eq_key(&item.key, &posted.key))
        {
            return Err(
                duplicate_key_message(crate::refs::RefKind::UnitFamily, &posted.key).into(),
            );
        }
    }
    reject_empty_base(&posted.base_unit)?;
    let specs = posted
        .units
        .iter()
        .map(|unit| UnitSpec {
            code: unit.code.clone(),
            factor: unit.factor.to_string(),
        })
        .collect::<Vec<_>>();
    unit_specs(posted.base_unit.trim(), &specs)?;
    crate::edit::upsert_unit_family(defs, posted.clone());
    Ok(())
}

fn reorder_type_attributes(
    defs: &mut DefinitionSet,
    type_key: &str,
    attributes: &[String],
) -> Result<(), ApplyError> {
    let ids = crate::standard::standard_ids();
    let pinned = [
        ids.number,
        ids.name,
        ids.created_at,
        ids.updated_at,
        ids.updated_by,
    ];
    let wanted = attributes
        .iter()
        .map(|key| attribute_id(defs, key))
        .collect::<Result<Vec<_>, _>>()?;
    let object_type = type_mut(defs, type_key)?;
    let mut ordered = Vec::new();
    for id in pinned {
        if object_type.attributes.contains(&id) {
            ordered.push(id);
        }
    }
    for id in wanted {
        if ordered.contains(&id) {
            continue;
        }
        if !object_type.attributes.contains(&id) {
            return Err(format!("attribute is not on type {}", object_type.key).into());
        }
        ordered.push(id);
    }
    for id in &object_type.attributes {
        if !ordered.contains(id) {
            ordered.push(*id);
        }
    }
    object_type.attributes = ordered;
    Ok(())
}

fn named_options(
    defs: &DefinitionSet,
    options: &[NamedOption],
) -> Result<Vec<ValueOption>, String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for option in options {
        let code = option.code.trim();
        if code.is_empty() {
            return Err("option code is empty".into());
        }
        if !seen.insert(code.to_owned()) {
            return Err(format!("option {code} is repeated"));
        }
        out.push(ValueOption {
            code: code.to_owned(),
            labels: if option.label.trim().is_empty() {
                label_map(defs, code)
            } else {
                label_map(defs, option.label.trim())
            },
        });
    }
    Ok(out)
}

fn unit_specs(base_unit: &str, units: &[UnitSpec]) -> Result<Vec<Unit>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for spec in units {
        let code = spec.code.trim();
        if code.is_empty() {
            return Err("unit code is empty".into());
        }
        if !seen.insert(code.to_owned()) {
            return Err(format!("unit {code} is repeated"));
        }
        out.push(Unit {
            code: code.to_owned(),
            factor: parse_decimal(&spec.factor)?,
        });
    }
    if !seen.contains(base_unit) {
        out.insert(
            0,
            Unit {
                code: base_unit.to_owned(),
                factor: Decimal::ONE,
            },
        );
    }
    Ok(out)
}

fn set_unit_family(kind: &mut AttributeKind, family: Option<UnitFamilyId>) -> Result<(), String> {
    match kind {
        AttributeKind::Number { unit_family, .. } | AttributeKind::Range { unit_family } => {
            *unit_family = family;
            Ok(())
        }
        _ if family.is_none() => Ok(()),
        _ => Err("a unit family applies to a number or a range".into()),
    }
}

fn apply_bounds(
    kind: &mut AttributeKind,
    bounds: &AttributeBounds,
    defs: &DefinitionSet,
) -> Result<(), String> {
    if bounds == &AttributeBounds::default() {
        return Ok(());
    }
    if let Some(token) = &bounds.unit_family {
        let id = crate::refs::unit_family_id(defs, token)?;
        set_unit_family(kind, Some(id))?;
    }
    if bounds.clear_min
        || bounds.min.is_some()
        || bounds.clear_max
        || bounds.max.is_some()
        || bounds.integer.is_some()
    {
        let AttributeKind::Number {
            min, max, integer, ..
        } = kind
        else {
            return Err("min and max apply to a number".into());
        };
        if bounds.clear_min {
            *min = None;
        }
        if let Some(text) = &bounds.min {
            *min = Some(parse_decimal(text)?);
        }
        if bounds.clear_max {
            *max = None;
        }
        if let Some(text) = &bounds.max {
            *max = Some(parse_decimal(text)?);
        }
        if let Some(value) = bounds.integer {
            *integer = value;
        }
    }
    if bounds.clear_max_length || bounds.max_length.is_some() || bounds.text_form.is_some() {
        let AttributeKind::Text { max_length, form } = kind else {
            return Err("length applies to text".into());
        };
        if bounds.clear_max_length {
            *max_length = None;
        }
        if let Some(value) = bounds.max_length {
            *max_length = Some(value);
        }
        if let Some(form_name) = &bounds.text_form {
            *form = match form_name.to_lowercase().as_str() {
                "short" => TextForm::Short,
                "long" => TextForm::Long,
                other => return Err(format!("unknown text form {other}")),
            };
        }
    }
    if bounds.clear_value_list || bounds.value_list.is_some() {
        let AttributeKind::ValueList { list, .. } = kind else {
            return Err("a value list applies to a value list attribute".into());
        };
        if bounds.clear_value_list {
            *list = None;
        }
        if let Some(token) = &bounds.value_list {
            let _ = crate::refs::value_list_id(defs, token)?;
            *list = Some(token.clone());
        }
    }
    Ok(())
}

fn parse_decimal(text: &str) -> Result<Decimal, String> {
    text.trim()
        .parse::<Decimal>()
        .map_err(|_| format!("invalid number {text}"))
}

fn attribute_kind(kind: &str, unit_family: Option<UnitFamilyId>) -> Result<AttributeKind, String> {
    match kind.to_lowercase().as_str() {
        "text" => Ok(AttributeKind::Text {
            max_length: Some(200),
            form: TextForm::Short,
        }),
        "number" => Ok(AttributeKind::Number {
            unit_family,
            min: None,
            max: None,
            integer: false,
        }),
        "boolean" => Ok(AttributeKind::Boolean),
        "date" => Ok(AttributeKind::Date),
        "value_list" => Ok(AttributeKind::ValueList {
            options: vec![],
            list: None,
        }),
        "range" => Ok(AttributeKind::Range { unit_family }),
        "file" | "file_ref" => Ok(AttributeKind::FileRef { media: false }),
        "object" | "object_ref" => Err("object references are link types".into()),
        other => Err(format!("unknown attribute type {other}")),
    }
}

fn set_structures(
    defs: &mut DefinitionSet,
    type_key: &str,
    structures: &[String],
) -> Result<(), String> {
    let mut ids = Vec::with_capacity(structures.len());
    for token in structures {
        let id = crate::refs::structure_id(defs, token)
            .map_err(|_| format!("unknown structure {token}"))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let object_type = type_mut(defs, type_key)?;
    object_type.structures = Some(ids);
    Ok(())
}

fn label_map(defs: &DefinitionSet, label: &str) -> BTreeMap<LanguageTag, String> {
    let tag = defs
        .languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned());
    match tag {
        Some(tag) => BTreeMap::from([(tag, label.to_owned())]),
        None => BTreeMap::new(),
    }
}

fn group_caption(
    languages: &[LanguageTag],
    structure: &Structure,
    id: Option<GroupId>,
) -> Option<String> {
    let id = id?;
    structure
        .groups
        .iter()
        .find(|group| group.id == id)
        .map(|group| caption_of(languages, &group.labels, &group.key))
}

/// The model's first language, then English, then any stored label, then the key.
fn caption_of(
    languages: &[LanguageTag],
    labels: &BTreeMap<LanguageTag, String>,
    fallback: &str,
) -> String {
    let stored = |tag: &str| {
        labels
            .iter()
            .find(|(item, text)| item.as_str() == tag && !text.is_empty())
            .map(|(_, text)| text.clone())
    };
    if let Some(tag) = languages.first()
        && let Some(text) = stored(tag.as_str())
    {
        return text;
    }
    if let Some(text) = stored("en") {
        return text;
    }
    labels
        .values()
        .find(|text| !text.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

fn class_key(structure: &Structure, id: Option<ClassId>) -> Option<String> {
    let id = id?;
    structure
        .classes
        .iter()
        .find(|class| class.id == id)
        .map(|class| class.key.clone())
}

fn display_label(labels: &BTreeMap<LanguageTag, String>, fallback: &str) -> String {
    labels
        .iter()
        .find(|(tag, _)| tag.as_str() == "en")
        .or_else(|| labels.iter().next())
        .map(|(_, label)| label.clone())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn attribute_keys(defs: &DefinitionSet, ids: &[AttributeId]) -> BTreeSet<String> {
    ids.iter()
        .filter_map(|id| {
            defs.attributes
                .iter()
                .find(|attribute| attribute.id == *id)
                .map(|attribute| attribute.key.clone())
        })
        .collect()
}

fn kind_label(kind: &AttributeKind) -> String {
    match kind {
        AttributeKind::Text { .. } => "text".into(),
        AttributeKind::Number { .. } => "number".into(),
        AttributeKind::Range { .. } => "range".into(),
        AttributeKind::ValueList { .. } => "value list".into(),
        AttributeKind::Boolean => "boolean".into(),
        AttributeKind::Date => "date".into(),
        AttributeKind::ObjectRef { .. } => "object".into(),
        AttributeKind::FileRef { .. } => "file".into(),
    }
}

fn stable_uuid(label: &str) -> Uuid {
    let mut hash = 0xcbf29ce484222325u128;
    for byte in label.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut bytes = hash.to_le_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{IndexFlags, ObjectType};
    use crate::id::TypeId;

    fn base() -> DefinitionSet {
        let weight = AttributeId::from_uuid(Uuid::from_u128(1));
        let status = AttributeId::from_uuid(Uuid::from_u128(2));
        let family = UnitFamilyId::from_uuid(Uuid::from_u128(3));
        let structure = StructureId::from_uuid(Uuid::from_u128(4));
        let tents = GroupId::from_uuid(Uuid::from_u128(5));
        let timber = GroupId::from_uuid(Uuid::from_u128(6));
        let class = ClassId::from_uuid(Uuid::from_u128(7));
        DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("en").unwrap()],
            unit_families: vec![crate::definition::UnitFamily {
                id: family,
                key: "mass".into(),
                dimension: Some("mass".into()),
                base_unit: "kg".into(),
                units: vec![crate::definition::Unit {
                    code: "kg".into(),
                    factor: rust_decimal::Decimal::ONE,
                }],
            }],
            value_lists: vec![],
            attributes: vec![
                Attribute {
                    id: weight,
                    key: "weight".into(),
                    labels: BTreeMap::new(),
                    language_dependent: false,
                    multiple: false,
                    required: false,
                    kind: AttributeKind::Number {
                        unit_family: Some(family),
                        min: None,
                        max: None,
                        integer: false,
                    },
                    index: IndexFlags::default(),
                },
                Attribute {
                    id: status,
                    key: "status".into(),
                    labels: BTreeMap::new(),
                    language_dependent: false,
                    multiple: false,
                    required: false,
                    kind: AttributeKind::Text {
                        max_length: None,
                        form: TextForm::Short,
                    },
                    index: IndexFlags::default(),
                },
            ],
            types: vec![ObjectType {
                id: TypeId::from_uuid(Uuid::from_u128(8)),
                key: "item".into(),
                labels: BTreeMap::new(),
                singular: Default::default(),
                plural: Default::default(),
                key_attribute: weight,
                attributes: vec![weight],
                structures: None,
                defaults: BTreeMap::new(),

                kind: Default::default(),
                link: None,
            }],
            structures: vec![Structure {
                id: structure,
                key: "assortment".into(),
                labels: BTreeMap::new(),
                groups: vec![
                    Group {
                        id: tents,
                        key: "camp".into(),
                        labels: BTreeMap::new(),
                        parent: None,
                        attributes: vec![],
                    },
                    Group {
                        id: timber,
                        key: "timber".into(),
                        labels: BTreeMap::new(),
                        parent: None,
                        attributes: vec![],
                    },
                ],
                classes: vec![Class {
                    id: class,
                    key: "bag".into(),
                    group: tents,
                    parent: None,
                    attributes: vec![weight],
                }],
            }],
        }
    }

    #[test]
    fn add_attribute_sentence_drafts_a_class_when_it_is_missing() {
        let defs = base();
        let steps = interpret("add attribute weight (kg) to Drill", &defs).expect("command");
        let (_after, impact) = preview(&defs, &steps).expect("preview");
        assert!(impact.lines.iter().any(|line| line.contains("Drill")));
        assert!(impact.lines.iter().any(|line| line.contains("weight")));
        assert!(impact.fields.iter().any(|field| field == "weight"));
    }

    #[test]
    fn group_commands_rename_move_duplicate_and_delete() {
        let defs = base();
        let renamed = interpret("rename group camp to canopy", &defs).expect("rename");
        let (after, impact) = preview(&defs, &renamed).expect("preview");
        assert_eq!(impact.lines, vec!["Rename group camp to canopy".to_owned()]);
        let tents = after.structures[0]
            .groups
            .iter()
            .find(|group| group.key == "camp")
            .expect("camp");
        assert!(tents.labels.values().any(|label| label == "canopy"));

        let moved = interpret("move group bag to timber", &defs);
        assert!(moved.is_none(), "a class move must not become a group move");
        let moved = interpret("move group camp to timber", &defs).expect("move");
        let (after, impact) = preview(&defs, &moved).expect("preview");
        assert!(
            impact
                .lines
                .iter()
                .any(|line| line.contains("Move group camp"))
        );
        let camp = after.structures[0]
            .groups
            .iter()
            .find(|group| group.key == "camp")
            .expect("camp");
        let timber = after.structures[0]
            .groups
            .iter()
            .find(|group| group.key == "timber")
            .expect("timber");
        assert_eq!(camp.parent, Some(timber.id));

        let copied = interpret("duplicate group camp as spare", &defs).expect("copy");
        let (after, _) = preview(&defs, &copied).expect("preview");
        assert!(
            after.structures[0]
                .groups
                .iter()
                .any(|group| group.key == "spare")
        );

        let removed = interpret("delete group camp", &defs).expect("delete");
        let (after, impact) = preview(&defs, &removed).expect("preview");
        assert_eq!(impact.lines, vec!["Delete group camp".to_owned()]);
        assert!(
            !after.structures[0]
                .groups
                .iter()
                .any(|group| group.key == "camp")
        );
        assert!(impact.groups.iter().any(|id| id.ends_with("000000000005")));
    }

    #[test]
    fn moving_a_class_names_both_groups() {
        let defs = base();
        let steps = interpret("move bag to timber", &defs).expect("command");
        let (_after, impact) = preview(&defs, &steps).expect("preview");
        assert_eq!(
            impact.lines,
            vec!["Move group bag from camp to timber".to_owned()]
        );
        assert!(impact.usage_fields.iter().any(|field| field == "weight"));
        assert_eq!(impact.groups.len(), 2);
    }

    #[test]
    fn add_group_diff_uses_the_caption_not_the_key() {
        let mut defs = base();
        let camp = defs.structures[0]
            .groups
            .iter_mut()
            .find(|group| group.key == "camp")
            .expect("camp");
        camp.labels
            .insert(LanguageTag::parse("en").unwrap(), "Tents".into());
        let (_after, impact) = preview(
            &defs,
            &[ModelChange::CreateGroup {
                structure: "assortment".into(),
                group: "canopy".into(),
                parent: Some("camp".into()),
                label: "Canopy".into(),
            }],
        )
        .expect("preview");
        assert_eq!(
            impact.lines,
            vec!["Add group Canopy under Tents".to_owned()]
        );
    }

    #[test]
    fn removing_an_attribute_is_a_usage_change() {
        let defs = base();
        let steps = interpret("remove attribute weight from bag", &defs).expect("command");
        let (_after, impact) = preview(&defs, &steps).expect("preview");
        assert_eq!(
            impact.lines,
            vec!["Remove attribute weight from bag".to_owned()]
        );
        assert_eq!(impact.usage_fields, vec!["weight".to_owned()]);
    }

    #[test]
    fn a_type_a_value_list_and_a_number_preview_together() {
        let defs = base();
        let steps = vec![
            ModelChange::CreateValueList {
                key: "difficulty".into(),
                label: "Difficulty".into(),
                options: vec![
                    NamedOption {
                        code: "easy".into(),
                        label: "Easy".into(),
                    },
                    NamedOption {
                        code: "hard".into(),
                        label: "Hard".into(),
                    },
                ],
            },
            ModelChange::CreateUnitFamily {
                key: "portion".into(),
                dimension: Some("count".into()),
                base_unit: "pc".into(),
                units: vec![UnitSpec {
                    code: "pc".into(),
                    factor: "1".into(),
                }],
            },
            ModelChange::CreateType {
                key: "recipe".into(),
                label: "Recipe".into(),
                plural: Some("Recipes".into()),
                structures: None,
                defaults: None,
            },
            ModelChange::CreateAttribute {
                key: "servings".into(),
                label: "Servings".into(),
                kind: "number".into(),
                unit: Some("pc".into()),
                multiple: false,
                languages: false,
                required: false,
                searchable: false,
                filterable: true,
                sortable: true,
                bounds: AttributeBounds {
                    min: Some("1".into()),
                    max: Some("12".into()),
                    ..AttributeBounds::default()
                },
            },
            ModelChange::AttachAttribute {
                attribute: "servings".into(),
                type_key: Some("recipe".into()),
                structure: None,
                group: None,
            },
        ];
        let (after, impact) = preview(&defs, &steps).expect("preview");
        assert!(
            impact
                .lines
                .iter()
                .any(|line| line.contains("Create type recipe"))
        );
        assert!(
            impact
                .lines
                .iter()
                .any(|line| line.contains("Difficulty") || line.contains("difficulty"))
        );
        let recipe = after
            .types
            .iter()
            .find(|item| item.key == "recipe")
            .expect("recipe");
        assert!(recipe.attributes.len() > 1);
        let servings = after
            .attributes
            .iter()
            .find(|item| item.key == "servings")
            .expect("servings");
        match &servings.kind {
            AttributeKind::Number {
                min,
                max,
                unit_family,
                ..
            } => {
                assert!(min.is_some() && max.is_some());
                assert!(unit_family.is_some());
            }
            other => panic!("expected a number, got {other:?}"),
        }
    }

    fn create(key: &str, kind: &str, unit: Option<&str>) -> ModelChange {
        ModelChange::CreateAttribute {
            key: key.into(),
            label: key.into(),
            kind: kind.into(),
            unit: unit.map(str::to_owned),
            multiple: false,
            languages: false,
            required: false,
            searchable: false,
            filterable: false,
            sortable: false,
            bounds: AttributeBounds::default(),
        }
    }

    #[test]
    fn an_object_reference_cannot_be_created() {
        let mut defs = base();
        let error = apply_all(&mut defs, &[create("brand", "object_ref", None)]).unwrap_err();
        assert!(error.to_string().contains("link types"), "{error}");
        assert!(defs.attributes.iter().all(|item| item.key != "brand"));
    }

    #[test]
    fn a_second_definition_with_a_different_type_or_unit_is_blocked() {
        let defs = base();
        let text = apply_all(&mut defs.clone(), &[create("weight", "text", None)]);
        assert!(
            text.unwrap_err()
                .to_string()
                .contains("different type or unit")
        );
        let bare = apply_all(&mut defs.clone(), &[create("weight", "number", None)]);
        assert!(
            bare.unwrap_err()
                .to_string()
                .contains("different type or unit"),
            "weight is already a number of family mass"
        );
        let mut same = defs.clone();
        apply_all(&mut same, &[create("weight", "number", Some("kg"))]).expect("same definition");
        assert_eq!(
            same.attributes
                .iter()
                .filter(|item| item.key == "weight")
                .count(),
            1
        );
    }

    #[test]
    fn linking_an_existing_key_does_not_create_a_second_attribute() {
        let mut defs = base();
        apply_all(
            &mut defs,
            &[
                ModelChange::AttachAttribute {
                    attribute: "status".into(),
                    type_key: Some("item".into()),
                    structure: None,
                    group: None,
                },
                ModelChange::AttachAttribute {
                    attribute: "status".into(),
                    type_key: None,
                    structure: Some("assortment".into()),
                    group: Some("camp".into()),
                },
            ],
        )
        .expect("link");
        assert_eq!(
            defs.attributes
                .iter()
                .filter(|item| item.key == "status")
                .count(),
            1
        );
        assert_eq!(defs.types[0].attributes.len(), 2);
        assert_eq!(defs.structures[0].groups[0].attributes.len(), 1);
    }

    #[test]
    fn renaming_an_attribute_key_keeps_the_id_and_warns() {
        let defs = base();
        let weight = defs.attributes[0].id;
        let steps = interpret("rename attribute key weight to mass_kg", &defs).expect("rename");
        let (after, impact) = preview(&defs, &steps).expect("preview");
        assert_eq!(after.attributes[0].id, weight);
        assert_eq!(after.attributes[0].key, "mass_kg");
        assert_eq!(
            impact.warnings,
            vec![
                "External integrations that use the old key « weight » will stop working"
                    .to_owned()
            ]
        );
        assert!(impact.lines.iter().any(|line| line.contains("mass_kg")));
        let mut state = serde_json::json!({ "columns": ["weight"], "sort": "weight" });
        crate::refs::translate_view_state(&mut state, &defs);
        assert_eq!(state["columns"][0], weight.as_uuid().to_string());
        assert_eq!(state["sort"], weight.as_uuid().to_string());
        crate::refs::translate_view_state(&mut state, &after);
        assert_eq!(state["columns"][0], weight.as_uuid().to_string());
    }

    #[test]
    fn a_link_type_has_no_number_or_name_and_an_entity_keeps_both() {
        let mut defs = base();
        apply_all(
            &mut defs,
            &[ModelChange::CreateLinkType {
                key: "pairs_with".into(),
                label: "Pairs with".into(),
                from_types: vec!["item".into()],
                to_types: vec!["item".into()],
                from_key: "pairs_with".into(),
                from_label: "Pairs with".into(),
                to_key: "paired_with".into(),
                to_label: "Pairs with".into(),
                from_cardinality: "many".into(),
                to_cardinality: "one".into(),
                attributes: vec!["note".into()],
            }],
        )
        .expect("link type");
        let link = defs
            .types
            .iter()
            .find(|item| item.key == "pairs_with")
            .unwrap();
        assert!(link.is_link());
        assert!(!link.attributes.contains(&crate::standard::number_id()));
        assert!(!link.attributes.contains(&crate::standard::name_id()));
        assert!(
            link.attributes
                .contains(&crate::standard::standard_ids().created_at)
        );
        assert!(link.attributes.iter().any(|id| {
            defs.attributes
                .iter()
                .any(|attribute| attribute.id == *id && attribute.key == "note")
        }));
        let entity = defs.types.iter().find(|item| item.key == "item").unwrap();
        assert!(!entity.is_link());
        assert_eq!(entity.attributes.len(), 1);
    }

    #[test]
    fn a_posted_document_fails_the_same_check_as_the_step() {
        let mut defs = base();
        crate::standard::install_standard_attributes(&mut defs);
        let update: ModelChange = serde_json::from_value(serde_json::json!({
            "op": "update_attribute",
            "key": "name",
            "kind": "number"
        }))
        .unwrap();
        let update_error = preview(&defs, std::slice::from_ref(&update))
            .unwrap_err()
            .to_string();
        let mut name = defs
            .attributes
            .iter()
            .find(|item| item.key == "name")
            .unwrap()
            .clone();
        name.kind = AttributeKind::Number {
            unit_family: None,
            min: None,
            max: None,
            integer: false,
        };
        let upsert_error = preview(
            &defs,
            &[ModelChange::UpsertAttribute {
                attribute: Box::new(name),
            }],
        )
        .unwrap_err()
        .to_string();
        assert_eq!(update_error, upsert_error);

        let key = "018f0000-0000-7000-8000-000000000999";
        let create_error = preview(
            &defs,
            &[ModelChange::CreateType {
                key: key.into(),
                label: "Nope".into(),
                plural: None,
                structures: None,
                defaults: None,
            }],
        )
        .unwrap_err()
        .to_string();
        let posted = ObjectType {
            id: TypeId::from_uuid(Uuid::from_u128(99)),
            key: key.into(),
            labels: BTreeMap::new(),
            singular: BTreeMap::new(),
            plural: BTreeMap::new(),
            key_attribute: defs.attributes[0].id,
            attributes: vec![],
            structures: None,
            defaults: BTreeMap::new(),
            kind: Default::default(),
            link: None,
        };
        let upsert_type = preview(
            &defs,
            &[ModelChange::UpsertType {
                object_type: Box::new(posted),
            }],
        )
        .unwrap_err()
        .to_string();
        assert_eq!(create_error, upsert_type);

        let structure_error = preview(
            &defs,
            &[ModelChange::CreateStructure {
                key: "bad key".into(),
                label: "Bad".into(),
            }],
        )
        .unwrap_err()
        .to_string();
        let posted_structure = Structure {
            id: StructureId::from_uuid(Uuid::from_u128(50)),
            key: "bad key".into(),
            labels: BTreeMap::new(),
            groups: Vec::new(),
            classes: Vec::new(),
        };
        let upsert_structure = preview(
            &defs,
            &[ModelChange::UpsertStructure {
                structure: Box::new(posted_structure),
            }],
        )
        .unwrap_err()
        .to_string();
        assert_eq!(structure_error, upsert_structure);

        let family_error = preview(
            &defs,
            &[ModelChange::CreateUnitFamily {
                key: "portion".into(),
                dimension: None,
                base_unit: String::new(),
                units: Vec::new(),
            }],
        )
        .unwrap_err()
        .to_string();
        let posted_family = UnitFamily {
            id: UnitFamilyId::from_uuid(Uuid::from_u128(51)),
            key: "portion".into(),
            dimension: None,
            base_unit: String::new(),
            units: Vec::new(),
        };
        let upsert_family = preview(
            &defs,
            &[ModelChange::UpsertUnitFamily {
                family: Box::new(posted_family),
            }],
        )
        .unwrap_err()
        .to_string();
        assert_eq!(family_error, upsert_family);
    }

    #[test]
    fn a_posted_type_gets_the_same_standard_attributes_as_create_type() {
        let mut via_step = base();
        apply_all(
            &mut via_step,
            &[ModelChange::CreateType {
                key: "recipe".into(),
                label: "Recipe".into(),
                plural: Some("Recipes".into()),
                structures: None,
                defaults: None,
            }],
        )
        .expect("create type");
        let mut via_rest = base();
        let posted = ObjectType {
            id: TypeId::from_uuid(Uuid::from_u128(40)),
            key: "dish".into(),
            labels: BTreeMap::new(),
            singular: BTreeMap::new(),
            plural: BTreeMap::new(),
            key_attribute: via_rest.attributes[0].id,
            attributes: vec![],
            structures: None,
            defaults: BTreeMap::new(),
            kind: Default::default(),
            link: None,
        };
        apply_all(
            &mut via_rest,
            &[ModelChange::UpsertType {
                object_type: Box::new(posted),
            }],
        )
        .expect("upsert type");
        let ids = crate::standard::standard_ids();
        let pinned = vec![
            ids.number,
            ids.name,
            ids.created_at,
            ids.updated_at,
            ids.updated_by,
        ];
        let recipe = via_step
            .types
            .iter()
            .find(|item| item.key == "recipe")
            .expect("recipe");
        let dish = via_rest
            .types
            .iter()
            .find(|item| item.key == "dish")
            .expect("dish");
        assert_eq!(recipe.attributes, pinned);
        assert_eq!(dish.attributes, recipe.attributes);
        assert_eq!(recipe.key_attribute, ids.number);
        assert_eq!(dish.key_attribute, ids.number);
        for id in &pinned {
            assert_eq!(
                via_step
                    .attributes
                    .iter()
                    .filter(|attribute| attribute.id == *id)
                    .count(),
                1
            );
            assert_eq!(
                via_rest
                    .attributes
                    .iter()
                    .filter(|attribute| attribute.id == *id)
                    .count(),
                1
            );
        }

        let weight = via_rest
            .attributes
            .iter()
            .find(|attribute| attribute.key == "weight")
            .expect("weight")
            .id;
        let mut updated = dish.clone();
        updated
            .labels
            .insert(LanguageTag::parse("en").expect("en"), "Dish".into());
        updated.key_attribute = weight;
        updated.attributes = vec![ids.number, ids.number, weight];
        apply_all(
            &mut via_rest,
            &[ModelChange::UpsertType {
                object_type: Box::new(updated),
            }],
        )
        .expect("update type");
        let dish = via_rest
            .types
            .iter()
            .find(|item| item.key == "dish")
            .expect("dish");
        assert_eq!(dish.attributes, {
            let mut kept = pinned.clone();
            kept.push(weight);
            kept
        });
        assert_eq!(dish.key_attribute, ids.number);
        assert_eq!(
            dish.attributes
                .iter()
                .filter(|id| **id == ids.number)
                .count(),
            1
        );
        for id in &pinned {
            assert_eq!(
                via_rest
                    .attributes
                    .iter()
                    .filter(|attribute| attribute.id == *id)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn a_structure_upsert_lists_only_the_groups_it_drops() {
        let defs = base();
        let timber = defs.structures[0].groups[1].id;
        let mut posted = defs.structures[0].clone();
        posted.groups.retain(|group| group.id != timber);
        let removed = groups_removed_by_structure_upserts(
            &defs,
            &[ModelChange::UpsertStructure {
                structure: Box::new(posted),
            }],
        );
        assert_eq!(removed, vec![timber]);
        let kept = groups_removed_by_structure_upserts(
            &defs,
            &[ModelChange::UpsertStructure {
                structure: Box::new(defs.structures[0].clone()),
            }],
        );
        assert!(kept.is_empty());
        let deleted = groups_removed_by_structure_upserts(
            &defs,
            &[ModelChange::DeleteGroup {
                structure: "assortment".into(),
                group: "timber".into(),
                move_to: None,
            }],
        );
        assert!(deleted.is_empty());
    }
}
