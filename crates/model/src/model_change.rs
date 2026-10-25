//! Edits of a definition set that a person confirms as one proposal.
//!
//! Nothing here writes. The caller compiles the result and stores it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::definition::{
    Attribute, AttributeKind, Class, DefinitionSet, Group, Structure, TextForm,
};
use crate::id::{AttributeId, ClassId, GroupId, StructureId, UnitFamilyId};
use crate::language::LanguageTag;

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
    },
    CreateStructure {
        key: String,
    },
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

/// Applies every step. Later steps see earlier ones. Idempotent creates stay put.
pub fn apply_all(defs: &mut DefinitionSet, changes: &[ModelChange]) -> Result<(), String> {
    for change in changes {
        apply_one(defs, change)?;
    }
    Ok(())
}

/// Applies the steps on a clone and describes the difference.
pub fn preview(
    defs: &DefinitionSet,
    changes: &[ModelChange],
) -> Result<(DefinitionSet, ChangeImpact), String> {
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
    let lower = trimmed.to_lowercase();
    if lower.contains("power tool")
        && (lower.contains("model")
            || lower.contains("set up")
            || lower.contains("setup")
            || lower.contains("draft"))
    {
        return Some(power_tools(defs));
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "add attribute ") {
        return add_attribute_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "remove attribute ") {
        return remove_attribute_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "move class ") {
        return move_command(rest, defs);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "move ") {
        return move_command(rest, defs);
    }
    None
}

/// A small power-tool model: structure, groups, parent class, drill, weight and voltage.
pub fn power_tools(defs: &DefinitionSet) -> Vec<ModelChange> {
    let mut steps = vec![
        ModelChange::CreateStructure {
            key: "power_tools".into(),
        },
        ModelChange::CreateGroup {
            structure: "power_tools".into(),
            group: "tools".into(),
            parent: None,
        },
        ModelChange::CreateGroup {
            structure: "power_tools".into(),
            group: "drills".into(),
            parent: Some("tools".into()),
        },
        ModelChange::CreateClass {
            structure: "power_tools".into(),
            class_key: "Power tool".into(),
            group: "tools".into(),
            parent: None,
            attributes: vec![],
        },
        ModelChange::CreateClass {
            structure: "power_tools".into(),
            class_key: "Drill".into(),
            group: "drills".into(),
            parent: Some("Power tool".into()),
            attributes: vec![],
        },
        ModelChange::CreateAttribute {
            key: "weight".into(),
            label: "Weight".into(),
            kind: "number".into(),
            unit: Some("kg".into()),
            multiple: false,
            languages: false,
            required: false,
        },
        ModelChange::AddAttribute {
            structure: "power_tools".into(),
            class_key: "Power tool".into(),
            attribute: "weight".into(),
        },
        ModelChange::CreateAttribute {
            key: "voltage".into(),
            label: "Voltage".into(),
            kind: "number".into(),
            unit: None,
            multiple: false,
            languages: false,
            required: true,
        },
        ModelChange::AddAttribute {
            structure: "power_tools".into(),
            class_key: "Drill".into(),
            attribute: "voltage".into(),
        },
    ];
    if defs
        .attributes
        .iter()
        .any(|attribute| attribute.key == "status")
    {
        steps.push(ModelChange::AddAttribute {
            structure: "power_tools".into(),
            class_key: "Drill".into(),
            attribute: "status".into(),
        });
    }
    steps
}

pub fn diff(before: &DefinitionSet, after: &DefinitionSet) -> ChangeImpact {
    let mut lines = Vec::new();
    let mut fields = BTreeSet::new();
    let mut usage_fields = BTreeSet::new();
    let mut groups = BTreeSet::new();
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
                let parent = group_key(structure, group.parent);
                match parent {
                    Some(parent) => lines.push(format!("Add group {} under {parent}", group.key)),
                    None => lines.push(format!("Add group {}", group.key)),
                }
            }
        }
        let old_classes = previous.map(|item| &item.classes);
        for class in &structure.classes {
            let old =
                old_classes.and_then(|classes| classes.iter().find(|item| item.key == class.key));
            let group_name = group_key(structure, Some(class.group)).unwrap_or_else(|| "?".into());
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
                    let old_group =
                        group_key(old_structure, Some(old.group)).unwrap_or_else(|| "?".into());
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
    ChangeImpact {
        lines,
        fields: fields.into_iter().collect(),
        usage_fields: usage_fields.into_iter().collect(),
        groups: groups.into_iter().collect(),
    }
}

fn apply_one(defs: &mut DefinitionSet, change: &ModelChange) -> Result<(), String> {
    match change {
        ModelChange::CreateStructure { key } => {
            if key.is_empty() {
                return Err("structure key is empty".into());
            }
            if defs.structures.iter().any(|item| item.key == *key) {
                return Ok(());
            }
            let structure_id = StructureId::from_uuid(stable_uuid(&format!("structure:{key}")));
            defs.structures.push(Structure {
                id: structure_id,
                key: key.clone(),
                groups: vec![Group {
                    id: GroupId::from_uuid(stable_uuid(&format!("group:{key}:root"))),
                    key: "root".into(),
                    parent: None,
                }],
                classes: vec![],
            });
            Ok(())
        }
        ModelChange::CreateGroup {
            structure,
            group,
            parent,
        } => {
            let structure = structure_mut(defs, structure)?;
            if structure.groups.iter().any(|item| item.key == *group) {
                return Ok(());
            }
            let parent_id = match parent {
                Some(key) => Some(group_id(structure, key)?),
                None => None,
            };
            structure.groups.push(Group {
                id: GroupId::from_uuid(stable_uuid(&format!("group:{}:{group}", structure.key))),
                key: group.clone(),
                parent: parent_id,
            });
            Ok(())
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
        } => {
            if defs.attributes.iter().any(|item| item.key == *key) {
                return Ok(());
            }
            let unit_family = match unit {
                Some(unit) => Some(family_for_unit(defs, unit)?),
                None => None,
            };
            let kind = attribute_kind(kind, unit_family)?;
            defs.attributes.push(Attribute {
                id: AttributeId::from_uuid(stable_uuid(&format!("attribute:{key}"))),
                key: key.clone(),
                labels: label_map(defs, if label.is_empty() { key } else { label }),
                language_dependent: *languages,
                multiple: *multiple,
                required: *required,
                kind,
                index: crate::definition::IndexFlags {
                    searchable: false,
                    filterable: true,
                    sortable: false,
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
                return Err(format!("class {} cannot parent itself", class.key));
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
                return Err(format!(
                    "reorder of {} must list the same attributes",
                    class.key
                ));
            }
            class.attributes = ids;
            Ok(())
        }
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
        });
    }
    let (structure, class_key) = if let Some(found) = find_class(defs, class_name) {
        found
    } else {
        let structure = "power_tools".to_owned();
        steps.push(ModelChange::CreateStructure {
            key: structure.clone(),
        });
        steps.push(ModelChange::CreateGroup {
            structure: structure.clone(),
            group: "tools".into(),
            parent: None,
        });
        steps.push(ModelChange::CreateClass {
            structure: structure.clone(),
            class_key: class_name.to_owned(),
            group: "tools".into(),
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

fn structure_mut<'a>(defs: &'a mut DefinitionSet, key: &str) -> Result<&'a mut Structure, String> {
    defs.structures
        .iter_mut()
        .find(|item| eq_key(&item.key, key))
        .ok_or_else(|| format!("unknown structure {key}"))
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

fn group_key(structure: &Structure, id: Option<GroupId>) -> Option<String> {
    let id = id?;
    structure
        .groups
        .iter()
        .find(|group| group.id == id)
        .map(|group| group.key.clone())
}

fn class_key(structure: &Structure, id: Option<ClassId>) -> Option<String> {
    let id = id?;
    structure
        .classes
        .iter()
        .find(|class| class.id == id)
        .map(|class| class.key.clone())
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
                key_attribute: weight,
                attributes: vec![weight],
            }],
            structures: vec![Structure {
                id: structure,
                key: "sortiment".into(),
                groups: vec![
                    Group {
                        id: tents,
                        key: "talt".into(),
                        parent: None,
                    },
                    Group {
                        id: timber,
                        key: "bygg".into(),
                        parent: None,
                    },
                ],
                classes: vec![Class {
                    id: class,
                    key: "sovsack".into(),
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
    fn moving_a_class_names_both_groups() {
        let defs = base();
        let steps = interpret("move sovsack to bygg", &defs).expect("command");
        let (_after, impact) = preview(&defs, &steps).expect("preview");
        assert_eq!(
            impact.lines,
            vec!["Move class sovsack from talt to bygg".to_owned()]
        );
        assert!(impact.usage_fields.iter().any(|field| field == "weight"));
        assert_eq!(impact.groups.len(), 2);
    }

    #[test]
    fn power_tools_is_a_tree_the_person_can_read() {
        let defs = base();
        let steps = interpret("set up a model for power tools", &defs).expect("command");
        let (after, impact) = preview(&defs, &steps).expect("preview");
        let text = impact.lines.join("\n");
        assert!(text.contains("Add structure power_tools"));
        assert!(text.contains("Add group drills under tools"));
        assert!(text.contains("Add class Drill on drills, parent Power tool"));
        assert!(text.contains("Add attribute weight to Power tool"));
        assert!(text.contains("Create attribute voltage"));
        assert!(text.contains("status"));
        let drill = after
            .structures
            .iter()
            .find(|item| item.key == "power_tools")
            .expect("structure")
            .classes
            .iter()
            .find(|item| item.key == "Drill")
            .expect("drill");
        assert!(drill.parent.is_some());
        let voltage = after
            .attributes
            .iter()
            .find(|item| item.key == "voltage")
            .expect("voltage");
        assert!(voltage.required);
    }

    #[test]
    fn removing_an_attribute_is_a_usage_change() {
        let defs = base();
        let steps = interpret("remove attribute weight from sovsack", &defs).expect("command");
        let (_after, impact) = preview(&defs, &steps).expect("preview");
        assert_eq!(
            impact.lines,
            vec!["Remove attribute weight from sovsack".to_owned()]
        );
        assert_eq!(impact.usage_fields, vec!["weight".to_owned()]);
    }
}
