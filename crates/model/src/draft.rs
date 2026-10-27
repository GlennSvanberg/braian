//! One person's unsaved model steps.
//!
//! Nothing here writes. The store keeps the row; this module adds, removes,
//! detects a conflict and rebases the steps onto a newer revision.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::consequence::DataPurge;
use crate::definition::DefinitionSet;
use crate::model_change::{ApplyError, ChangeImpact, ModelChange, preview};

/// Steps collected since `base_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDraft {
    pub base_revision: u64,
    pub steps: Vec<ModelChange>,
}

impl ModelDraft {
    pub fn new(base_revision: u64) -> Self {
        Self {
            base_revision,
            steps: Vec::new(),
        }
    }

    pub fn add(&mut self, step: ModelChange) {
        self.steps.push(step);
    }

    pub fn remove(&mut self, index: usize) -> Result<ModelChange, String> {
        if index >= self.steps.len() {
            return Err(format!("no step {index}"));
        }
        Ok(self.steps.remove(index))
    }

    /// The model moved on after the draft was started.
    pub fn conflicts(&self, current_revision: u64) -> bool {
        self.base_revision != current_revision
    }

    /// Keeps the steps and points them at `defs`, which must already be the latest revision.
    ///
    /// The steps have to apply on that revision. A step that does not apply leaves the draft as it was.
    pub fn rebase(&mut self, defs: &DefinitionSet) -> Result<ChangeImpact, ApplyError> {
        let (_, impact) = preview(defs, &self.steps)?;
        self.base_revision = defs.model_revision;
        Ok(impact)
    }
}

/// The saved change removed a definition. That cannot be undone.
pub fn change_removed_definitions(before: &DefinitionSet, after: &DefinitionSet) -> bool {
    missing_ids(
        before.types.iter().map(|item| item.id.as_uuid()),
        &after.types.iter().map(|item| item.id.as_uuid()).collect(),
    ) || missing_ids(
        before.attributes.iter().map(|item| item.id.as_uuid()),
        &after
            .attributes
            .iter()
            .map(|item| item.id.as_uuid())
            .collect(),
    ) || missing_ids(
        before.structures.iter().map(|item| item.id.as_uuid()),
        &after
            .structures
            .iter()
            .map(|item| item.id.as_uuid())
            .collect(),
    ) || missing_ids(
        before.value_lists.iter().map(|item| item.id.as_uuid()),
        &after
            .value_lists
            .iter()
            .map(|item| item.id.as_uuid())
            .collect(),
    ) || missing_ids(
        before.unit_families.iter().map(|item| item.id.as_uuid()),
        &after
            .unit_families
            .iter()
            .map(|item| item.id.as_uuid())
            .collect(),
    ) || groups_or_units_removed(before, after)
}

fn missing_ids(before: impl Iterator<Item = Uuid>, after: &BTreeSet<Uuid>) -> bool {
    before.into_iter().any(|id| !after.contains(&id))
}

fn groups_or_units_removed(before: &DefinitionSet, after: &DefinitionSet) -> bool {
    for structure in &before.structures {
        let Some(next) = after.structures.iter().find(|item| item.id == structure.id) else {
            continue;
        };
        let ids: BTreeSet<Uuid> = next.groups.iter().map(|group| group.id.as_uuid()).collect();
        if structure
            .groups
            .iter()
            .any(|group| !ids.contains(&group.id.as_uuid()))
        {
            return true;
        }
    }
    for family in &before.unit_families {
        let Some(next) = after.unit_families.iter().find(|item| item.id == family.id) else {
            continue;
        };
        let codes: BTreeSet<&str> = next.units.iter().map(|unit| unit.code.as_str()).collect();
        if family
            .units
            .iter()
            .any(|unit| !codes.contains(unit.code.as_str()))
        {
            return true;
        }
    }
    for list in &before.value_lists {
        let Some(next) = after.value_lists.iter().find(|item| item.id == list.id) else {
            continue;
        };
        let codes: BTreeSet<&str> = next
            .options
            .iter()
            .map(|option| option.code.as_str())
            .collect();
        if list
            .options
            .iter()
            .any(|option| !codes.contains(option.code.as_str()))
        {
            return true;
        }
    }
    false
}

/// Definitions the undo would drop. The caller refuses when those rows hold data.
pub fn undo_purge(before: &DefinitionSet, after: &DefinitionSet) -> DataPurge {
    let mut purge = DataPurge::default();
    let old_types: BTreeSet<Uuid> = before.types.iter().map(|item| item.id.as_uuid()).collect();
    for item in &after.types {
        let id = item.id.as_uuid();
        if !old_types.contains(&id) {
            purge.delete_types.push(id);
            purge.destructive = true;
        }
    }
    let old_attributes: BTreeSet<Uuid> = before
        .attributes
        .iter()
        .map(|item| item.id.as_uuid())
        .collect();
    for item in &after.attributes {
        let id = item.id.as_uuid();
        if !old_attributes.contains(&id) {
            purge.drop_attributes.push(id);
            purge.destructive = true;
        }
    }
    for structure in &after.structures {
        let old = before
            .structures
            .iter()
            .find(|item| item.id == structure.id);
        let old_groups: BTreeSet<Uuid> = old
            .map(|item| item.groups.iter().map(|group| group.id.as_uuid()).collect())
            .unwrap_or_default();
        for group in &structure.groups {
            let id = group.id.as_uuid();
            if !old_groups.contains(&id) {
                purge.unplace_groups.push(id);
                purge.destructive = true;
            }
        }
    }
    purge
}

/// The previous definition set, stored as the next revision.
pub fn restored_definitions(previous: &DefinitionSet, current: &DefinitionSet) -> DefinitionSet {
    let mut next = previous.clone();
    next.model_revision = current.model_revision.saturating_add(1);
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{Attribute, AttributeKind, IndexFlags, ObjectType, TextForm};
    use crate::id::{AttributeId, TypeId};
    use crate::language::LanguageTag;
    use crate::model_change::apply_all;

    fn empty(revision: u64) -> DefinitionSet {
        DefinitionSet {
            model_revision: revision,
            languages: vec![LanguageTag::parse("en").unwrap()],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![Attribute {
                id: AttributeId::from_uuid(Uuid::from_u128(1)),
                key: "name".into(),
                labels: Default::default(),
                language_dependent: false,
                multiple: false,
                required: true,
                kind: AttributeKind::Text {
                    max_length: None,
                    form: TextForm::Short,
                },
                index: IndexFlags::default(),
            }],
            types: vec![ObjectType {
                id: TypeId::from_uuid(Uuid::from_u128(2)),
                key: "item".into(),
                labels: Default::default(),
                singular: Default::default(),
                plural: Default::default(),
                key_attribute: AttributeId::from_uuid(Uuid::from_u128(1)),
                attributes: vec![AttributeId::from_uuid(Uuid::from_u128(1))],
                structures: Some(vec![]),
                defaults: Default::default(),
            }],
            structures: vec![],
        }
    }

    fn step(key: &str) -> ModelChange {
        ModelChange::CreateType {
            key: key.into(),
            label: key.into(),
            plural: None,
            structures: None,
            defaults: None,
        }
    }

    #[test]
    fn adding_and_removing_a_step() {
        let mut draft = ModelDraft::new(4);
        draft.add(step("recipe"));
        draft.add(step("meal"));
        assert_eq!(draft.steps.len(), 2);
        let removed = draft.remove(0).expect("step");
        assert!(matches!(removed, ModelChange::CreateType { key, .. } if key == "recipe"));
        assert_eq!(draft.steps.len(), 1);
        assert!(draft.remove(3).is_err());
    }

    #[test]
    fn a_moved_revision_is_a_conflict_until_rebase() {
        let mut draft = ModelDraft::new(4);
        draft.add(step("recipe"));
        assert!(draft.conflicts(5));
        assert!(!draft.conflicts(4));
        let mut defs = empty(7);
        let impact = draft.rebase(&defs).expect("rebase");
        assert!(impact.lines.iter().any(|line| line.contains("recipe")));
        assert_eq!(draft.base_revision, 7);
        assert!(!draft.conflicts(7));
        apply_all(&mut defs, &draft.steps).expect("apply");
        assert!(defs.types.iter().any(|item| item.key == "recipe"));
    }

    #[test]
    fn rebase_refuses_a_step_that_does_not_apply() {
        let mut draft = ModelDraft::new(1);
        draft.add(ModelChange::UpdateType {
            key: "missing".into(),
            language: None,
            singular: Some("Missing".into()),
            plural: None,
            structures: None,
            defaults: None,
        });
        let defs = empty(3);
        assert!(draft.rebase(&defs).is_err());
        assert_eq!(draft.base_revision, 1);
        assert!(draft.conflicts(3));
    }

    #[test]
    fn a_delete_cannot_be_undone_and_a_label_can() {
        let before = empty(1);
        let mut added = before.clone();
        added.model_revision = 2;
        apply_all(&mut added, &[step("recipe")]).unwrap();
        assert!(!change_removed_definitions(&before, &added));
        let purge = undo_purge(&before, &added);
        assert!(purge.delete_types.len() == 1);
        let mut renamed = before.clone();
        renamed.types[0]
            .labels
            .insert(LanguageTag::parse("en").unwrap(), "Item".into());
        renamed.model_revision = 2;
        assert!(!change_removed_definitions(&before, &renamed));
        assert!(!undo_purge(&before, &renamed).destructive);
        let mut deleted = before.clone();
        deleted.types.clear();
        assert!(change_removed_definitions(&before, &deleted));
        let restored = restored_definitions(&before, &added);
        assert_eq!(restored.model_revision, 3);
        assert!(restored.types.iter().all(|item| item.key != "recipe"));
    }
}
