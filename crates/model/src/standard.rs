//! Attributes every regular type carries, and which trees it may use.
//!
//! The ids are fixed so a saved view can point at Number or Name after a key
//! rename. Labels can change. The attributes themselves cannot be removed.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use uuid::Uuid;

use crate::definition::{
    Attribute, AttributeKind, DefinitionSet, IndexFlags, ObjectType, TextForm,
};
use crate::id::{AttributeId, StructureId, TypeId};
use crate::language::LanguageTag;

const NUMBER: &str = "018f0000-0000-7000-8000-000000000100";
const NAME: &str = "018f0000-0000-7000-8000-000000000101";
const CREATED_AT: &str = "018f0000-0000-7000-8000-00000000a003";
const UPDATED_AT: &str = "018f0000-0000-7000-8000-00000000a004";
const UPDATED_BY: &str = "018f0000-0000-7000-8000-00000000a005";

#[derive(Debug, Clone, Copy)]
pub struct StandardIds {
    pub number: AttributeId,
    pub name: AttributeId,
    pub created_at: AttributeId,
    pub updated_at: AttributeId,
    pub updated_by: AttributeId,
}

pub fn standard_ids() -> StandardIds {
    StandardIds {
        number: id(NUMBER),
        name: id(NAME),
        created_at: id(CREATED_AT),
        updated_at: id(UPDATED_AT),
        updated_by: id(UPDATED_BY),
    }
}

pub fn number_id() -> AttributeId {
    id(NUMBER)
}

pub fn name_id() -> AttributeId {
    id(NAME)
}

pub fn is_standard_attribute(id: AttributeId) -> bool {
    let ids = standard_ids();
    id == ids.number
        || id == ids.name
        || id == ids.created_at
        || id == ids.updated_at
        || id == ids.updated_by
}

/// A standard attribute that already carries its canonical key.
///
/// The fixed ids are also used by catalogs that have not been upgraded yet.
/// Those rows keep their own keys, such as `sku`, until the upgrade renames them.
pub fn is_locked_standard(defs: &DefinitionSet, id: AttributeId) -> bool {
    let Some(attribute) = defs.attributes.iter().find(|item| item.id == id) else {
        return false;
    };
    let ids = standard_ids();
    (id == ids.number && attribute.key == "number")
        || (id == ids.name && attribute.key == "name")
        || (id == ids.created_at && attribute.key == "created_at")
        || (id == ids.updated_at && attribute.key == "updated_at")
        || (id == ids.updated_by && attribute.key == "updated_by")
}

fn id(text: &str) -> AttributeId {
    AttributeId::from_uuid(Uuid::parse_str(text).expect("standard attribute id"))
}

/// Adds the five standard attributes and attaches them to every type.
///
/// `number` becomes the key attribute. An existing attribute with that id keeps
/// its label. Timestamp attributes are created when they are missing. They are
/// not stored on the object; the columns are.
pub fn install_standard_attributes(defs: &mut DefinitionSet) {
    let ids = standard_ids();
    let language = defs
        .languages
        .iter()
        .find(|tag| tag.as_str() == "en")
        .cloned()
        .or_else(|| defs.languages.first().cloned());
    ensure_row(
        defs, ids.number, "number", "Number", true, false, &language, true,
    );
    ensure_row(defs, ids.name, "name", "Name", true, true, &language, true);
    ensure_row(
        defs,
        ids.created_at,
        "created_at",
        "Created",
        false,
        false,
        &language,
        false,
    );
    ensure_row(
        defs,
        ids.updated_at,
        "updated_at",
        "Updated",
        false,
        false,
        &language,
        false,
    );
    ensure_row(
        defs,
        ids.updated_by,
        "updated_by",
        "Updated by",
        false,
        false,
        &language,
        false,
    );
    let order = [
        ids.number,
        ids.name,
        ids.created_at,
        ids.updated_at,
        ids.updated_by,
    ];
    for object_type in &mut defs.types {
        object_type.key_attribute = ids.number;
        let mut attributes = Vec::with_capacity(object_type.attributes.len() + order.len());
        for id in order {
            if !attributes.contains(&id) {
                attributes.push(id);
            }
        }
        for id in &object_type.attributes {
            if !attributes.contains(id) {
                attributes.push(*id);
            }
        }
        object_type.attributes = attributes;
    }
}

#[allow(clippy::too_many_arguments)]
fn ensure_row(
    defs: &mut DefinitionSet,
    id: AttributeId,
    key: &str,
    label: &str,
    required: bool,
    language_dependent: bool,
    language: &Option<LanguageTag>,
    indexed: bool,
) {
    if let Some(existing) = defs.attributes.iter_mut().find(|item| item.id == id) {
        if existing.key != key {
            existing.key = key.to_owned();
        }
        if required {
            existing.required = true;
        }
        return;
    }
    // A different attribute already owns the key. Keep the standard id and
    // leave the old key for the value move to read.
    if defs.attributes.iter().any(|item| item.key == key) {
        let mut taken = 2u32;
        let mut renamed = format!("{key}_{taken}");
        while defs.attributes.iter().any(|item| item.key == renamed) {
            taken += 1;
            renamed = format!("{key}_{taken}");
        }
        if let Some(existing) = defs.attributes.iter_mut().find(|item| item.key == key) {
            existing.key = renamed;
        }
    }
    let mut labels = BTreeMap::new();
    if let Some(language) = language {
        labels.insert(language.clone(), label.to_owned());
    }
    defs.attributes.insert(
        0,
        Attribute {
            id,
            key: key.to_owned(),
            labels,
            language_dependent,
            multiple: false,
            required,
            kind: AttributeKind::Text {
                max_length: Some(if key == "number" { 64 } else { 160 }),
                form: TextForm::Short,
            },
            index: IndexFlags {
                searchable: indexed,
                filterable: key == "number",
                sortable: indexed,
            },
        },
    );
}

/// `None` allows every tree. An empty list allows none.
pub fn placement_allowed(object_type: &ObjectType, structure: StructureId) -> bool {
    match &object_type.structures {
        None => true,
        Some(allowed) => allowed.contains(&structure),
    }
}

/// Fills missing values from the type's `defaults`.
pub fn apply_defaults(
    defs: &DefinitionSet,
    type_id: TypeId,
    values: &mut BTreeMap<AttributeId, Value>,
) {
    let Some(object_type) = defs.types.iter().find(|item| item.id == type_id) else {
        return;
    };
    for (key, text) in &object_type.defaults {
        let Some(attribute) = defs.attributes.iter().find(|item| item.key == *key) else {
            continue;
        };
        if values.contains_key(&attribute.id) {
            continue;
        }
        values.insert(attribute.id, Value::String(text.clone()));
    }
}

/// Copies `name` and `company_name` onto the standard name attribute.
///
/// A plain string becomes `{en: string}` when the standard name is language
/// dependent. The old values are removed. The object key fills name when both
/// sources are empty, so a required name is present after the upgrade.
pub fn move_values(defs: &DefinitionSet, key: &str, values: &mut BTreeMap<AttributeId, Value>) {
    let ids = standard_ids();
    let name_attr = defs.attributes.iter().find(|item| item.id == ids.name);
    let sources: Vec<AttributeId> = defs
        .attributes
        .iter()
        .filter(|item| item.id != ids.name && matches!(item.key.as_str(), "name" | "company_name"))
        .map(|item| item.id)
        .collect();
    if !values.contains_key(&ids.name) {
        let mut taken = None;
        for source in &sources {
            if let Some(value) = values.get(source).cloned() {
                taken = Some(value);
                if defs
                    .attributes
                    .iter()
                    .any(|item| item.id == *source && item.key == "name")
                {
                    break;
                }
            }
        }
        let taken = taken.or_else(|| (!key.is_empty()).then(|| Value::String(key.to_owned())));
        if let Some(value) = taken {
            let value = match name_attr {
                Some(attribute) if attribute.language_dependent && value.is_string() => {
                    let mut map = serde_json::Map::new();
                    map.insert("en".into(), value);
                    Value::Object(map)
                }
                _ => value,
            };
            values.insert(ids.name, value);
        }
    }
    for source in sources {
        values.remove(&source);
    }
}

/// Trees each type's objects already sit in. Types with no rows get an empty list.
pub fn structures_from_usage(
    defs: &mut DefinitionSet,
    usage: &BTreeMap<TypeId, BTreeSet<StructureId>>,
) {
    for object_type in &mut defs.types {
        if object_type.structures.is_some() {
            continue;
        }
        let mut ids: Vec<StructureId> = usage
            .get(&object_type.id)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default();
        ids.sort_by_key(|id| id.as_uuid());
        object_type.structures = Some(ids);
    }
}

/// Drops name aliases from types once their values have moved.
pub fn detach_name_aliases(defs: &mut DefinitionSet) {
    let name = name_id();
    let aliases: Vec<AttributeId> = defs
        .attributes
        .iter()
        .filter(|item| item.id != name && matches!(item.key.as_str(), "name" | "company_name"))
        .map(|item| item.id)
        .collect();
    for object_type in &mut defs.types {
        object_type.attributes.retain(|id| !aliases.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{AttributeKind, IndexFlags, TextForm};
    use crate::id::TypeId;
    use crate::model_change::{ModelChange, apply_all};

    fn bare_type() -> DefinitionSet {
        let sku = AttributeId::from_uuid(Uuid::from_u128(9));
        DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("en").unwrap()],
            unit_families: vec![],
            value_lists: vec![],
            attributes: vec![Attribute {
                id: sku,
                key: "sku".into(),
                labels: BTreeMap::new(),
                language_dependent: false,
                multiple: false,
                required: false,
                kind: AttributeKind::Text {
                    max_length: Some(32),
                    form: TextForm::Short,
                },
                index: IndexFlags::default(),
            }],
            types: vec![ObjectType {
                id: TypeId::from_uuid(Uuid::from_u128(3)),
                key: "item".into(),
                labels: BTreeMap::new(),
                singular: BTreeMap::new(),
                plural: BTreeMap::new(),
                key_attribute: sku,
                attributes: vec![sku],
                structures: None,
                defaults: BTreeMap::new(),
            }],
            structures: vec![],
        }
    }

    #[test]
    fn every_type_receives_standard_attributes_that_cannot_be_removed() {
        let mut defs = bare_type();
        install_standard_attributes(&mut defs);
        let ids = standard_ids();
        for object_type in &defs.types {
            for id in [
                ids.number,
                ids.name,
                ids.created_at,
                ids.updated_at,
                ids.updated_by,
            ] {
                assert!(object_type.attributes.contains(&id), "{id}");
            }
            assert_eq!(object_type.key_attribute, ids.number);
        }
        let error = apply_all(
            &mut defs,
            &[ModelChange::DetachAttribute {
                attribute: "name".into(),
                type_key: Some("item".into()),
                structure: None,
                group: None,
            }],
        );
        assert!(error.is_err(), "{error:?}");
        assert!(defs.types[0].attributes.contains(&ids.name));
        let renamed = apply_all(
            &mut defs,
            &[ModelChange::UpdateAttribute {
                key: "name".into(),
                label: Some("Title".into()),
                kind: None,
                unit: None,
                clear_unit: false,
                languages: None,
                required: None,
                searchable: None,
                filterable: None,
                sortable: None,
            }],
        );
        assert!(renamed.is_ok(), "{renamed:?}");
        let name = defs
            .attributes
            .iter()
            .find(|item| item.id == ids.name)
            .unwrap();
        assert_eq!(
            name.labels.values().next().map(String::as_str),
            Some("Title")
        );
        let key_change = apply_all(
            &mut defs,
            &[ModelChange::RenameAttributeKey {
                key: "name".into(),
                new_key: "title".into(),
            }],
        );
        assert!(key_change.is_err(), "{key_change:?}");
        let deleted = apply_all(
            &mut defs,
            &[ModelChange::DeleteAttribute { key: "name".into() }],
        );
        assert!(deleted.is_err(), "{deleted:?}");
        assert!(defs.attributes.iter().any(|item| item.id == ids.name));
    }

    #[test]
    fn company_name_moves_onto_the_standard_name() {
        let mut defs = bare_type();
        let company = AttributeId::from_uuid(Uuid::from_u128(8));
        defs.attributes.push(Attribute {
            id: company,
            key: "company_name".into(),
            labels: BTreeMap::new(),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Text {
                max_length: Some(80),
                form: TextForm::Short,
            },
            index: IndexFlags::default(),
        });
        install_standard_attributes(&mut defs);
        let mut values = BTreeMap::from([(company, Value::String("Northmill".into()))]);
        move_values(&defs, "SUP-1", &mut values);
        let name = values.get(&name_id()).unwrap();
        assert_eq!(name["en"], "Northmill");
        assert!(!values.contains_key(&company));
    }

    #[test]
    fn an_empty_structure_list_rejects_placement() {
        let mut defs = bare_type();
        let structure = StructureId::from_uuid(Uuid::from_u128(4));
        defs.types[0].structures = Some(vec![]);
        assert!(!placement_allowed(&defs.types[0], structure));
        defs.types[0].structures = Some(vec![structure]);
        assert!(placement_allowed(&defs.types[0], structure));
        defs.types[0].structures = None;
        assert!(placement_allowed(&defs.types[0], structure));
    }
}
