//! Edits of one structure or one group.
//!
//! These helpers keep parent links inside the structure. They do not look at
//! stored objects. The caller refuses a delete while objects still sit in the
//! group, or moves those objects first.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::definition::{DefinitionSet, Group, Structure};
use crate::id::{ClassId, GroupId, StructureId};
use crate::language::LanguageTag;

/// Why a structure edit was refused. `code` is the value clients branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum StructureErrorCode {
    EmptyKey,
    InvalidKey,
    DuplicateKey,
    UnknownStructure,
    UnknownGroup,
    Cycle,
    TargetInSubtree,
    GroupOccupied,
    StructureOccupied,
}

impl StructureErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyKey => "empty_key",
            Self::InvalidKey => "invalid_key",
            Self::DuplicateKey => "duplicate_key",
            Self::UnknownStructure => "unknown_structure",
            Self::UnknownGroup => "unknown_group",
            Self::Cycle => "cycle",
            Self::TargetInSubtree => "target_in_subtree",
            Self::GroupOccupied => "group_occupied",
            Self::StructureOccupied => "structure_occupied",
        }
    }
}

impl std::fmt::Display for StructureErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A structure edit that did not change the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StructureError {
    pub code: StructureErrorCode,
    pub message: String,
}

impl StructureError {
    pub fn new(code: StructureErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StructureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StructureError {}

/// Groups a delete removes, and the group objects should move to when one was named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRemoval {
    pub structure_id: StructureId,
    pub removed: Vec<GroupId>,
    pub move_to: Option<GroupId>,
}

/// Inserts a structure with no groups. An existing key is left as it is when
/// `strict` is false, so a repeated proposal does not fail.
pub fn create_structure(
    defs: &mut DefinitionSet,
    id: StructureId,
    key: &str,
    label: &str,
    strict: bool,
) -> Result<(), StructureError> {
    check_key(key)?;
    if defs.structures.iter().any(|item| same_key(&item.key, key)) {
        if strict {
            return Err(StructureError::new(
                StructureErrorCode::DuplicateKey,
                format!("structure {key} already exists"),
            ));
        }
        return Ok(());
    }
    let mut labels = BTreeMap::new();
    labels.insert(label_tag(defs), label_or_key(label, key).to_owned());
    defs.structures.push(Structure {
        id,
        key: key.to_owned(),
        labels,
        groups: Vec::new(),
        classes: Vec::new(),
    });
    Ok(())
}

/// Sets the display name on the UI language (`en` when the model has it).
pub fn rename_structure(
    defs: &mut DefinitionSet,
    key: &str,
    label: &str,
) -> Result<(), StructureError> {
    let text = require_label(label)?.to_owned();
    let tag = label_tag(defs);
    let structure = structure_mut(defs, key)?;
    structure.labels.insert(tag, text);
    Ok(())
}

/// Replaces the label map. An empty map clears the display name back to the key.
pub fn set_structure_labels(
    defs: &mut DefinitionSet,
    key: &str,
    labels: BTreeMap<LanguageTag, String>,
) -> Result<(), StructureError> {
    let structure = structure_mut(defs, key)?;
    structure.labels = labels;
    Ok(())
}

/// Removes the structure. The returned ids are every group it held.
pub fn delete_structure(
    defs: &mut DefinitionSet,
    key: &str,
) -> Result<Vec<GroupId>, StructureError> {
    let plan = plan_delete_structure(defs, key)?;
    defs.structures.retain(|item| item.id != plan.structure_id);
    Ok(plan.removed)
}

/// Groups that disappear with the structure. Does not change `defs`.
pub fn plan_delete_structure(
    defs: &DefinitionSet,
    key: &str,
) -> Result<GroupRemoval, StructureError> {
    let structure = find_structure(defs, key)?;
    Ok(GroupRemoval {
        structure_id: structure.id,
        removed: structure.groups.iter().map(|group| group.id).collect(),
        move_to: None,
    })
}

/// Inserts a group. `parent` none makes it a root. An existing key stays put
/// when `strict` is false.
pub fn create_group(
    defs: &mut DefinitionSet,
    id: GroupId,
    structure_key: &str,
    key: &str,
    label: &str,
    parent: Option<&str>,
    strict: bool,
) -> Result<(), StructureError> {
    check_key(key)?;
    let text = label_or_key(label, key).to_owned();
    let tag = label_tag(defs);
    let structure = structure_mut(defs, structure_key)?;
    if structure
        .groups
        .iter()
        .any(|group| same_key(&group.key, key))
    {
        if strict {
            return Err(StructureError::new(
                StructureErrorCode::DuplicateKey,
                format!("group {key} already exists in {}", structure.key),
            ));
        }
        return Ok(());
    }
    let parent_id = match parent {
        Some(parent) => Some(find_group(structure, parent)?.id),
        None => None,
    };
    let mut labels = BTreeMap::new();
    labels.insert(tag, text);
    structure.groups.push(Group {
        id,
        key: key.to_owned(),
        labels,
        parent: parent_id,
        attributes: Vec::new(),
    });
    Ok(())
}

pub fn rename_group(
    defs: &mut DefinitionSet,
    structure_key: &str,
    key: &str,
    label: &str,
) -> Result<(), StructureError> {
    let text = require_label(label)?.to_owned();
    let tag = label_tag(defs);
    let structure = structure_mut(defs, structure_key)?;
    let group = group_mut(structure, key)?;
    group.labels.insert(tag, text);
    Ok(())
}

pub fn set_group_labels(
    defs: &mut DefinitionSet,
    structure_key: &str,
    key: &str,
    labels: BTreeMap<LanguageTag, String>,
) -> Result<(), StructureError> {
    let structure = structure_mut(defs, structure_key)?;
    let group = group_mut(structure, key)?;
    group.labels = labels;
    Ok(())
}

/// Moves `group` under `parent`. `parent` none makes it a root.
///
/// A group cannot move under itself or under one of its descendants.
pub fn move_group(
    defs: &mut DefinitionSet,
    structure_key: &str,
    key: &str,
    parent: Option<&str>,
) -> Result<(), StructureError> {
    let structure = structure_mut(defs, structure_key)?;
    let group_id = find_group(structure, key)?.id;
    let parent_id = match parent {
        Some(parent) => Some(find_group(structure, parent)?.id),
        None => None,
    };
    if let Some(parent_id) = parent_id
        && subtree_ids(&structure.groups, group_id).contains(&parent_id)
    {
        return Err(StructureError::new(
            StructureErrorCode::Cycle,
            format!("moving {key} under {} would cycle", parent.unwrap_or(key)),
        ));
    }
    let group = group_mut(structure, key)?;
    group.parent = parent_id;
    Ok(())
}

/// Checks the delete and names the groups it removes. Does not change `defs`.
pub fn plan_delete_group(
    defs: &DefinitionSet,
    structure_key: &str,
    key: &str,
    move_to: Option<&str>,
) -> Result<GroupRemoval, StructureError> {
    let structure = find_structure(defs, structure_key)?;
    let group = find_group(structure, key)?;
    let removed = subtree_ids(&structure.groups, group.id);
    let move_to = match move_to {
        None => None,
        Some(target) => {
            let target = find_group(structure, target)?;
            if removed.contains(&target.id) {
                return Err(StructureError::new(
                    StructureErrorCode::TargetInSubtree,
                    format!("cannot move objects from {key} into {}", target.key),
                ));
            }
            Some(target.id)
        }
    };
    Ok(GroupRemoval {
        structure_id: structure.id,
        removed,
        move_to,
    })
}

/// Removes the group, its descendants, and the classes on those groups.
///
/// A class that remains and pointed at a removed class keeps the nearest
/// surviving ancestor as its parent, so the parent link never dangles.
pub fn delete_group(
    defs: &mut DefinitionSet,
    structure_key: &str,
    key: &str,
    move_to: Option<&str>,
) -> Result<GroupRemoval, StructureError> {
    let plan = plan_delete_group(defs, structure_key, key, move_to)?;
    let structure = structure_mut(defs, structure_key)?;
    let removed: HashSet<GroupId> = plan.removed.iter().copied().collect();
    let removed_classes: HashSet<ClassId> = structure
        .classes
        .iter()
        .filter(|class| removed.contains(&class.group))
        .map(|class| class.id)
        .collect();
    let parent_of: BTreeMap<ClassId, Option<ClassId>> = structure
        .classes
        .iter()
        .map(|class| (class.id, class.parent))
        .collect();
    structure
        .groups
        .retain(|group| !removed.contains(&group.id));
    structure
        .classes
        .retain(|class| !removed_classes.contains(&class.id));
    let alive: HashSet<ClassId> = structure.classes.iter().map(|class| class.id).collect();
    for class in &mut structure.classes {
        class.parent = surviving_parent(class.parent, &parent_of, &removed_classes, &alive);
    }
    Ok(plan)
}

fn surviving_parent(
    start: Option<ClassId>,
    parent_of: &BTreeMap<ClassId, Option<ClassId>>,
    removed: &HashSet<ClassId>,
    alive: &HashSet<ClassId>,
) -> Option<ClassId> {
    let mut current = start;
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            return None;
        }
        if alive.contains(&id) && !removed.contains(&id) {
            return Some(id);
        }
        current = parent_of.get(&id).copied().flatten();
    }
    None
}

fn subtree_ids(groups: &[Group], root: GroupId) -> Vec<GroupId> {
    let mut children: BTreeMap<GroupId, Vec<GroupId>> = BTreeMap::new();
    for group in groups {
        if let Some(parent) = group.parent {
            children.entry(parent).or_default().push(group.id);
        }
    }
    let mut out = vec![root];
    let mut index = 0;
    while index < out.len() {
        if let Some(kids) = children.get(&out[index]) {
            out.extend(kids.iter().copied());
        }
        index += 1;
    }
    out
}

fn check_key(key: &str) -> Result<(), StructureError> {
    if key.trim().is_empty() {
        return Err(StructureError::new(
            StructureErrorCode::EmptyKey,
            "key is empty",
        ));
    }
    if key.contains(':') || key.chars().any(char::is_whitespace) {
        return Err(StructureError::new(
            StructureErrorCode::InvalidKey,
            format!("key {key} cannot contain spaces or ':'"),
        ));
    }
    Ok(())
}

fn require_label(label: &str) -> Result<&str, StructureError> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(StructureError::new(
            StructureErrorCode::EmptyKey,
            "name is empty",
        ));
    }
    Ok(trimmed)
}

fn label_or_key<'a>(label: &'a str, key: &'a str) -> &'a str {
    let trimmed = label.trim();
    if trimmed.is_empty() { key } else { trimmed }
}

fn label_tag(defs: &DefinitionSet) -> LanguageTag {
    defs.languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned())
        .unwrap_or_else(|| LanguageTag::parse("en").expect("en is a language tag"))
}

fn same_key(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn find_structure<'a>(defs: &'a DefinitionSet, key: &str) -> Result<&'a Structure, StructureError> {
    defs.structures
        .iter()
        .find(|item| same_key(&item.key, key))
        .ok_or_else(|| {
            StructureError::new(
                StructureErrorCode::UnknownStructure,
                format!("unknown structure {key}"),
            )
        })
}

fn structure_mut<'a>(
    defs: &'a mut DefinitionSet,
    key: &str,
) -> Result<&'a mut Structure, StructureError> {
    defs.structures
        .iter_mut()
        .find(|item| same_key(&item.key, key))
        .ok_or_else(|| {
            StructureError::new(
                StructureErrorCode::UnknownStructure,
                format!("unknown structure {key}"),
            )
        })
}

fn find_group<'a>(structure: &'a Structure, key: &str) -> Result<&'a Group, StructureError> {
    structure
        .groups
        .iter()
        .find(|group| same_key(&group.key, key))
        .ok_or_else(|| {
            StructureError::new(
                StructureErrorCode::UnknownGroup,
                format!("unknown group {key} in {}", structure.key),
            )
        })
}

fn group_mut<'a>(structure: &'a mut Structure, key: &str) -> Result<&'a mut Group, StructureError> {
    let structure_key = structure.key.clone();
    structure
        .groups
        .iter_mut()
        .find(|group| same_key(&group.key, key))
        .ok_or_else(|| {
            StructureError::new(
                StructureErrorCode::UnknownGroup,
                format!("unknown group {key} in {structure_key}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::definition::Class;
    use crate::id::{AttributeId, ClassId};
    use uuid::Uuid;

    fn empty() -> DefinitionSet {
        DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("en").unwrap()],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![],
            types: vec![],
            structures: vec![],
        }
    }

    fn sid(n: u128) -> StructureId {
        StructureId::from_uuid(Uuid::from_u128(n))
    }

    fn gid(n: u128) -> GroupId {
        GroupId::from_uuid(Uuid::from_u128(n))
    }

    fn cid(n: u128) -> ClassId {
        ClassId::from_uuid(Uuid::from_u128(n))
    }

    fn tree(defs: &mut DefinitionSet) {
        create_structure(defs, sid(1), "catalog", "Catalog", true).unwrap();
        create_group(defs, gid(2), "catalog", "root", "Root", None, true).unwrap();
        create_group(
            defs,
            gid(3),
            "catalog",
            "child",
            "Child",
            Some("root"),
            true,
        )
        .unwrap();
        create_group(defs, gid(4), "catalog", "leaf", "Leaf", Some("child"), true).unwrap();
        let structure = defs.structures.iter_mut().next().unwrap();
        structure.classes.push(Class {
            id: cid(10),
            key: "root".into(),
            group: gid(2),
            parent: None,
            attributes: vec![AttributeId::from_uuid(Uuid::from_u128(90))],
        });
        structure.classes.push(Class {
            id: cid(11),
            key: "child".into(),
            group: gid(3),
            parent: Some(cid(10)),
            attributes: vec![],
        });
        structure.classes.push(Class {
            id: cid(12),
            key: "other".into(),
            group: gid(2),
            parent: Some(cid(11)),
            attributes: vec![],
        });
    }

    #[test]
    fn create_and_rename_keep_the_key_and_set_the_label() {
        let mut defs = empty();
        create_structure(&mut defs, sid(1), "sortiment", "eCom", true).unwrap();
        assert_eq!(defs.structures[0].key, "sortiment");
        assert_eq!(
            defs.structures[0]
                .labels
                .values()
                .next()
                .map(String::as_str),
            Some("eCom")
        );
        rename_structure(&mut defs, "sortiment", "Shop").unwrap();
        assert_eq!(defs.structures[0].key, "sortiment");
        assert_eq!(
            defs.structures[0]
                .labels
                .values()
                .next()
                .map(String::as_str),
            Some("Shop")
        );
    }

    #[test]
    fn a_repeated_create_is_idempotent_until_the_caller_asks_for_strict() {
        let mut defs = empty();
        create_structure(&mut defs, sid(1), "catalog", "", false).unwrap();
        create_structure(&mut defs, sid(2), "catalog", "", false).unwrap();
        assert_eq!(defs.structures.len(), 1);
        let error = create_structure(&mut defs, sid(3), "catalog", "", true).unwrap_err();
        assert_eq!(error.code, StructureErrorCode::DuplicateKey);
    }

    #[test]
    fn moving_a_group_under_its_descendant_is_a_cycle() {
        let mut defs = empty();
        tree(&mut defs);
        let error = move_group(&mut defs, "catalog", "root", Some("leaf")).unwrap_err();
        assert_eq!(error.code, StructureErrorCode::Cycle);
        move_group(&mut defs, "catalog", "leaf", Some("root")).unwrap();
        let leaf = defs.structures[0]
            .groups
            .iter()
            .find(|group| group.key == "leaf")
            .unwrap();
        assert_eq!(leaf.parent, Some(gid(2)));
    }

    #[test]
    fn deleting_a_group_removes_descendants_and_repairs_class_parents() {
        let mut defs = empty();
        tree(&mut defs);
        let removed = delete_group(&mut defs, "catalog", "child", None).unwrap();
        assert_eq!(removed.removed, vec![gid(3), gid(4)]);
        let structure = &defs.structures[0];
        let keys: BTreeSet<_> = structure
            .groups
            .iter()
            .map(|group| group.key.as_str())
            .collect();
        assert_eq!(keys, BTreeSet::from(["root"]));
        assert!(structure.groups.iter().all(|group| {
            group
                .parent
                .is_none_or(|parent| structure.groups.iter().any(|item| item.id == parent))
        }));
        let class_keys: BTreeSet<_> = structure
            .classes
            .iter()
            .map(|class| class.key.as_str())
            .collect();
        assert_eq!(class_keys, BTreeSet::from(["root", "other"]));
        let other = structure
            .classes
            .iter()
            .find(|class| class.key == "other")
            .unwrap();
        assert_eq!(other.parent, Some(cid(10)));
        assert!(structure.classes.iter().all(|class| {
            class
                .parent
                .is_none_or(|parent| structure.classes.iter().any(|item| item.id == parent))
                && structure.groups.iter().any(|group| group.id == class.group)
        }));
    }

    #[test]
    fn moving_objects_into_the_deleted_subtree_is_refused() {
        let mut defs = empty();
        tree(&mut defs);
        let error = plan_delete_group(&defs, "catalog", "root", Some("leaf")).unwrap_err();
        assert_eq!(error.code, StructureErrorCode::TargetInSubtree);
        let plan = plan_delete_group(&defs, "catalog", "child", Some("root")).unwrap();
        assert_eq!(plan.move_to, Some(gid(2)));
        assert!(
            defs.structures[0]
                .groups
                .iter()
                .any(|group| group.key == "child")
        );
    }
}
