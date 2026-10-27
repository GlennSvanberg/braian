//! Edits of a definition set that a person confirms as one proposal.
//!
//! Nothing here writes. The caller compiles the result and stores it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::definition::{
    Attribute, AttributeKind, Class, DefinitionSet, Group, ObjectType, Structure, TextForm,
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
        #[serde(alias = "class")]
        class_key: String,
        parent: Option<String>,
    },
    AddAttribute {
        structure: String,
        #[serde(alias = "class")]
        class_key: String,
        attribute: String,
    },
    RemoveAttribute {
        structure: String,
        #[serde(alias = "class")]
        class_key: String,
        attribute: String,
    },
    ReorderAttributes {
        structure: String,
        #[serde(alias = "class")]
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
    /// Group ids whose objects change class applicability.
    pub groups: Vec<String>,
}

fn default_filterable() -> bool {
    true
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
    let languages: &[LanguageTag] = if after.languages.is_empty() {
        &before.languages
    } else {
        &after.languages
    };
    let before_attrs: BTreeMap<&str, &Attribute> = before
        .attributes
        .iter()
        .map(|attribute| (attribute.key.as_str(), attribute))
        .collect();
    for attribute in &after.attributes {
        match before_attrs.get(attribute.key.as_str()) {
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
                    || kind_label(&previous.kind) != kind_label(&attribute.kind)
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
            .find(|item| item.key == structure.key);
        if previous.is_none() {
            lines.push(format!("Add structure {}", structure.key));
        }
        let old_groups = previous.map(|item| &item.groups);
        for group in &structure.groups {
            let existed =
                old_groups.is_some_and(|groups| groups.iter().any(|item| item.key == group.key));
            if !existed {
                let name = caption_of(languages, &group.labels, &group.key);
                let parent = group_caption(languages, structure, group.parent);
                match parent {
                    Some(parent) => lines.push(format!("Add group {name} under {parent}")),
                    None => lines.push(format!("Add group {name}")),
                }
            }
            let old_ids = old_groups
                .and_then(|groups| groups.iter().find(|item| item.key == group.key))
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
                let Some(group) = structure.groups.iter().find(|item| item.key == old.key) else {
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
                            "Add class {} on {group_name}, parent {parent}",
                            class.key
                        )),
                        None => lines.push(format!("Add class {} on {group_name}", class.key)),
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
                            "Move class {} from {old_group} to {group_name}",
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
                            "Class {} parent is {}",
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
        if after.structures.iter().any(|item| item.key == old.key) {
            continue;
        }
        lines.push(format!("Delete structure {}", old.key));
        for group in &old.groups {
            groups.insert(group.id.as_uuid().to_string());
        }
    }
    for object_type in &after.types {
        let old = before.types.iter().find(|item| item.key == object_type.key);
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
        }
    }
    ChangeImpact {
        lines,
        fields: fields.into_iter().collect(),
        usage_fields: usage_fields.into_iter().collect(),
        groups: groups.into_iter().collect(),
    }
}

fn apply_one(defs: &mut DefinitionSet, change: &ModelChange) -> Result<(), ApplyError> {
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
        } => {
            let unit_family = match unit {
                Some(unit) => Some(family_for_unit(defs, unit)?),
                None => None,
            };
            let wanted = attribute_kind(kind, unit_family)?;
            if let Some(existing) = defs.attributes.iter().find(|item| item.key == *key) {
                if !wanted.same_type_and_unit(&existing.kind) {
                    return Err(format!(
                        "attribute {key} already exists with a different type or unit"
                    )
                    .into());
                }
                return Ok(());
            }
            defs.attributes.push(Attribute {
                id: AttributeId::from_uuid(stable_uuid(&format!("attribute:{key}"))),
                key: key.clone(),
                labels: label_map(defs, if label.is_empty() { key } else { label }),
                language_dependent: *languages,
                multiple: *multiple,
                required: *required,
                kind: wanted,
                index: crate::definition::IndexFlags {
                    searchable: *searchable,
                    filterable: *filterable,
                    sortable: *sortable,
                },
            });
            Ok(())
        }
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
        } => update_type(
            defs,
            key,
            language.as_deref(),
            singular.as_deref(),
            plural.as_deref(),
        )
        .map_err(ApplyError::from),
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
        });
    }
    let (structure, class_key) = if let Some(found) = find_class(defs, class_name) {
        found
    } else {
        let structure = "unplaced".to_owned();
        steps.push(ModelChange::CreateStructure {
            key: structure.clone(),
            label: String::new(),
        });
        steps.push(ModelChange::CreateGroup {
            structure: structure.clone(),
            group: "groups".into(),
            parent: None,
            label: String::new(),
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
) -> Result<(), String> {
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
    } = patch;
    let (kind_now, family_now) = {
        let current = defs
            .attributes
            .iter()
            .find(|item| eq_key(&item.key, key))
            .ok_or_else(|| format!("unknown attribute {key}"))?;
        (
            kind_token(&current.kind).to_owned(),
            current.kind.unit_family_id(),
        )
    };
    let next_kind = if kind.is_some() || unit.is_some() || clear_unit {
        let kind_name = kind.clone().unwrap_or(kind_now);
        let unit_family = if clear_unit {
            None
        } else if let Some(unit) = unit {
            Some(family_for_unit(defs, unit)?)
        } else {
            family_now
        };
        Some(attribute_kind(&kind_name, unit_family)?)
    } else {
        None
    };
    let label_tag = defs
        .languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned());
    let attribute = defs
        .attributes
        .iter_mut()
        .find(|item| eq_key(&item.key, key))
        .ok_or_else(|| format!("unknown attribute {key}"))?;
    if let (Some(label), Some(tag)) = (label, label_tag) {
        attribute.labels.insert(tag, label.clone());
    }
    if let Some(kind) = next_kind {
        attribute.kind = kind;
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
        "object" | "object_ref" => Ok(AttributeKind::ObjectRef { type_key: None }),
        other => Err(format!("unknown attribute type {other}")),
    }
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
            vec!["Move class bag from camp to timber".to_owned()]
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
        }
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
}
