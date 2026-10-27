//! Mechanical edits of a [`DefinitionSet`].
//!
//! These helpers do not check references or rules. The caller compiles the
//! result and rejects a set that does not compile.

use crate::definition::{Attribute, DefinitionSet, ObjectType, Structure, UnitFamily};
use crate::id::{AttributeId, StructureId, TypeId, UnitFamilyId};
use crate::language::LanguageTag;

/// Inserts `attribute` or replaces the attribute with the same id.
pub fn upsert_attribute(defs: &mut DefinitionSet, attribute: Attribute) {
    upsert(&mut defs.attributes, attribute, |item| item.id);
}

/// Removes the attribute. Returns whether it was present.
///
/// References from types and classes are left in place. Compiling the set
/// reports them.
pub fn delete_attribute(defs: &mut DefinitionSet, id: AttributeId) -> bool {
    delete(&mut defs.attributes, |item| item.id == id)
}

/// Inserts `object_type` or replaces the type with the same id.
pub fn upsert_type(defs: &mut DefinitionSet, object_type: ObjectType) {
    upsert(&mut defs.types, object_type, |item| item.id);
}

/// Removes the type. Returns whether it was present.
pub fn delete_type(defs: &mut DefinitionSet, id: TypeId) -> bool {
    delete(&mut defs.types, |item| item.id == id)
}

/// Inserts `structure` or replaces the structure with the same id.
///
/// Groups and classes travel with the structure. Granular edits live in
/// [`crate::structure`].
pub fn upsert_structure(defs: &mut DefinitionSet, structure: Structure) {
    upsert(&mut defs.structures, structure, |item| item.id);
}

/// Removes the structure. Returns whether it was present.
pub fn delete_structure(defs: &mut DefinitionSet, id: StructureId) -> bool {
    delete(&mut defs.structures, |item| item.id == id)
}

/// Inserts `family` or replaces the unit family with the same id.
pub fn upsert_unit_family(defs: &mut DefinitionSet, family: UnitFamily) {
    upsert(&mut defs.unit_families, family, |item| item.id);
}

/// Removes the unit family. Returns whether it was present.
pub fn delete_unit_family(defs: &mut DefinitionSet, id: UnitFamilyId) -> bool {
    delete(&mut defs.unit_families, |item| item.id == id)
}

/// Replaces the language list. Order is the order callers read back.
pub fn set_languages(defs: &mut DefinitionSet, languages: Vec<LanguageTag>) {
    defs.languages = languages;
}

fn upsert<T, I: PartialEq>(items: &mut Vec<T>, item: T, id_of: impl Fn(&T) -> I) {
    let id = id_of(&item);
    if let Some(slot) = items.iter_mut().find(|existing| id_of(existing) == id) {
        *slot = item;
    } else {
        items.push(item);
    }
}

fn delete<T>(items: &mut Vec<T>, matches: impl Fn(&T) -> bool) -> bool {
    let before = items.len();
    items.retain(|item| !matches(item));
    items.len() != before
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uuid::Uuid;

    use crate::definition::{Attribute, AttributeKind, DefinitionSet, IndexFlags, ObjectType};
    use crate::id::{AttributeId, TypeId};
    use crate::language::LanguageTag;

    use super::*;

    fn empty() -> DefinitionSet {
        DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("sv").unwrap()],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![],
            types: vec![],
            structures: vec![],
        }
    }

    fn text(id: u128, key: &str) -> Attribute {
        Attribute {
            id: AttributeId::from_uuid(Uuid::from_u128(id)),
            key: key.to_owned(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Text {
                max_length: None,
                form: crate::definition::TextForm::Short,
            },
            index: IndexFlags::default(),
        }
    }

    #[test]
    fn upsert_inserts_then_replaces_by_id() {
        let mut defs = empty();
        let mut attribute = text(1, "sku");
        upsert_attribute(&mut defs, attribute.clone());
        assert_eq!(defs.attributes.len(), 1);
        attribute.key = "article".into();
        upsert_attribute(&mut defs, attribute);
        assert_eq!(defs.attributes.len(), 1);
        assert_eq!(defs.attributes[0].key, "article");
    }

    #[test]
    fn delete_reports_whether_the_entity_existed() {
        let mut defs = empty();
        let attribute = text(1, "sku");
        let id = attribute.id;
        assert!(!delete_attribute(&mut defs, id));
        upsert_attribute(&mut defs, attribute);
        assert!(delete_attribute(&mut defs, id));
        assert!(defs.attributes.is_empty());
    }

    #[test]
    fn set_languages_replaces_the_list() {
        let mut defs = empty();
        set_languages(
            &mut defs,
            vec![
                LanguageTag::parse("en-GB").unwrap(),
                LanguageTag::parse("sv").unwrap(),
            ],
        );
        assert_eq!(
            defs.languages
                .iter()
                .map(LanguageTag::as_str)
                .collect::<Vec<_>>(),
            vec!["en-GB", "sv"]
        );
    }

    #[test]
    fn upsert_type_replaces_by_id() {
        let mut defs = empty();
        let attribute = text(1, "sku");
        let id = TypeId::from_uuid(Uuid::from_u128(2));
        upsert_type(
            &mut defs,
            ObjectType {
                id,
                key: "item".into(),
                labels: BTreeMap::new(),
                singular: Default::default(),
                plural: Default::default(),
                key_attribute: attribute.id,
                attributes: vec![attribute.id],
                structures: None,
                defaults: BTreeMap::new(),

                kind: Default::default(),
                link: None,
            },
        );
        upsert_type(
            &mut defs,
            ObjectType {
                id,
                key: "product".into(),
                labels: BTreeMap::new(),
                singular: Default::default(),
                plural: Default::default(),
                key_attribute: attribute.id,
                attributes: vec![attribute.id],
                structures: None,
                defaults: BTreeMap::new(),

                kind: Default::default(),
                link: None,
            },
        );
        assert_eq!(defs.types.len(), 1);
        assert_eq!(defs.types[0].key, "product");
    }
}
