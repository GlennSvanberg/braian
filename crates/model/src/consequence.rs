//! What a model step does to stored objects, links, values and selections.
//!
//! Nothing here reads the database. The caller supplies a [`UsageFixture`] in
//! tests, or asks the store to measure the same [`DataPurge`].

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use crate::definition::{AttributeKind, DefinitionSet, UnitFamily};
use crate::id::GroupId;
use crate::language::LanguageTag;
use crate::model_change::{ModelChange, deletes_data};

/// Stored rows a step will remove or clear. Computed from the definition set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DataPurge {
    pub destructive: bool,
    /// Name the person types in the dialog. Empty when the step is not destructive.
    pub confirm_name: String,
    /// Attribute ids whose stored values are removed.
    pub drop_attributes: Vec<Uuid>,
    /// Subset of `drop_attributes` that store object references. Those values count as links.
    pub link_attributes: Vec<Uuid>,
    /// Empty means every type.
    pub limit_types: Vec<Uuid>,
    /// Empty means every group. A non-empty list keeps values on objects in one of these groups.
    pub limit_groups: Vec<Uuid>,
    /// When set, only values that are this option code or this unit are removed.
    pub match_value: Option<ValueMatch>,
    /// Objects of these types are deleted, with their values, links and selections.
    pub delete_types: Vec<Uuid>,
    /// Placements in these groups are removed. The objects stay.
    pub unplace_groups: Vec<Uuid>,
}

/// A value filter for one option code or one unit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueMatch {
    Code(String),
    Unit(String),
}

/// Counts the dialog and `dry_run` show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsequenceCounts {
    /// Objects that are deleted, lose a value, or leave a tree.
    pub objects: i64,
    pub links: i64,
    pub values: i64,
    pub selections: i64,
    /// Objects counted in `objects` because a type delete or a value removal hits them.
    pub removed: i64,
    /// Objects counted in `objects` only because they leave a tree.
    pub unplaced: i64,
}

/// One stored value, for tests of [`tally`].
#[derive(Debug, Clone)]
pub struct StoredValue {
    pub object: Uuid,
    pub attribute: Uuid,
    pub count: i64,
    pub code: Option<String>,
    pub link: bool,
}

/// One live object, for tests of [`tally`].
#[derive(Debug, Clone)]
pub struct StoredObject {
    pub id: Uuid,
    pub type_id: Uuid,
    pub groups: Vec<Uuid>,
}

/// A stand-in for the rows Postgres would count.
#[derive(Debug, Clone, Default)]
pub struct UsageFixture {
    pub objects: Vec<StoredObject>,
    pub values: Vec<StoredValue>,
    /// Relation endpoints.
    pub links: Vec<(Uuid, Uuid)>,
    /// Selections per type id.
    pub selections: BTreeMap<Uuid, i64>,
}

/// The purge for every step, merged. Later steps do not see earlier edits.
pub fn purge_for(defs: &DefinitionSet, changes: &[ModelChange]) -> DataPurge {
    let mut merged = DataPurge::default();
    for change in changes {
        let step = purge_one(defs, change);
        if step.destructive && merged.confirm_name.is_empty() {
            merged.confirm_name = step.confirm_name;
        }
        merged.destructive |= step.destructive;
        push_unique(&mut merged.drop_attributes, step.drop_attributes);
        push_unique(&mut merged.link_attributes, step.link_attributes);
        push_unique(&mut merged.limit_types, step.limit_types);
        push_unique(&mut merged.limit_groups, step.limit_groups);
        push_unique(&mut merged.delete_types, step.delete_types);
        push_unique(&mut merged.unplace_groups, step.unplace_groups);
        if merged.match_value.is_none() {
            merged.match_value = step.match_value;
        }
    }
    // A limit only applies when every step shared it. Mixed steps count everywhere.
    if changes.len() > 1 {
        merged.limit_types.clear();
        merged.limit_groups.clear();
        merged.match_value = None;
    }
    merged
}

/// Counts `fixture` the way the store counts a database.
pub fn tally(purge: &DataPurge, fixture: &UsageFixture) -> ConsequenceCounts {
    let delete_types: BTreeSet<Uuid> = purge.delete_types.iter().copied().collect();
    let drop: BTreeSet<Uuid> = purge.drop_attributes.iter().copied().collect();
    let links_attrs: BTreeSet<Uuid> = purge.link_attributes.iter().copied().collect();
    let unplace: BTreeSet<Uuid> = purge.unplace_groups.iter().copied().collect();
    let mut removed_objects = BTreeSet::new();
    let mut unplaced_objects = BTreeSet::new();
    let mut values = 0i64;
    let mut link_values = 0i64;

    for object in &fixture.objects {
        if delete_types.contains(&object.type_id) {
            removed_objects.insert(object.id);
        }
    }
    for value in &fixture.values {
        let Some(object) = fixture.objects.iter().find(|item| item.id == value.object) else {
            continue;
        };
        if delete_types.contains(&object.type_id) {
            values += value.count;
            if value.link {
                link_values += value.count;
            }
            continue;
        }
        if !drop.contains(&value.attribute) || !within_limits(purge, object) {
            continue;
        }
        if !value_matches_filter(value, purge.match_value.as_ref()) {
            continue;
        }
        removed_objects.insert(object.id);
        values += value.count;
        if value.link || links_attrs.contains(&value.attribute) {
            link_values += value.count;
        }
    }
    for object in &fixture.objects {
        if removed_objects.contains(&object.id) {
            continue;
        }
        if object.groups.iter().any(|group| unplace.contains(group)) {
            unplaced_objects.insert(object.id);
        }
    }
    let mut relation_links = 0i64;
    for (from, to) in &fixture.links {
        let hits = fixture.objects.iter().any(|object| {
            delete_types.contains(&object.type_id) && (object.id == *from || object.id == *to)
        });
        if hits {
            relation_links += 1;
        }
    }
    let mut selections = 0i64;
    for type_id in &purge.delete_types {
        selections += fixture.selections.get(type_id).copied().unwrap_or(0);
    }
    let removed = i64::try_from(removed_objects.len()).unwrap_or(i64::MAX);
    let unplaced = i64::try_from(unplaced_objects.len()).unwrap_or(i64::MAX);
    ConsequenceCounts {
        objects: removed + unplaced,
        links: relation_links + link_values,
        values,
        selections,
        removed,
        unplaced,
    }
}

/// Plain-English lines for a destructive step. The first line is the one the dialog leads with.
pub fn consequence_lines(counts: &ConsequenceCounts) -> Vec<String> {
    let mut lines = Vec::new();
    if counts.values > 0 || counts.removed > 0 {
        lines.push(format!(
            "{} on {} will be deleted",
            noun(counts.values, "value", "values"),
            noun(counts.removed, "object", "objects")
        ));
    }
    if counts.unplaced > 0 {
        lines.push(format!(
            "{} will be removed from the tree",
            noun(counts.unplaced, "object", "objects")
        ));
    }
    if counts.links > 0 {
        lines.push(format!(
            "{} will be deleted",
            noun(counts.links, "link", "links")
        ));
    }
    if counts.selections > 0 {
        lines.push(format!(
            "{} will be deleted",
            noun(counts.selections, "selection", "selections")
        ));
    }
    if lines.is_empty() {
        lines.push("0 values on 0 objects will be deleted".to_owned());
    }
    lines
}

pub fn consequence_sentence(counts: &ConsequenceCounts) -> String {
    consequence_lines(counts)
        .into_iter()
        .next()
        .unwrap_or_else(|| "0 values on 0 objects will be deleted".to_owned())
}

/// `1,240` for the dialog. Negative numbers keep the sign.
pub fn grouped(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, character) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(character);
    }
    let mut text: String = out.chars().rev().collect();
    if negative {
        text.insert(0, '-');
    }
    text
}

fn noun(count: i64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

fn value_matches_filter(value: &StoredValue, filter: Option<&ValueMatch>) -> bool {
    match filter {
        None => true,
        Some(ValueMatch::Code(code) | ValueMatch::Unit(code)) => {
            value.code.as_deref() == Some(code)
        }
    }
}

fn within_limits(purge: &DataPurge, object: &StoredObject) -> bool {
    if !purge.limit_types.is_empty() && !purge.limit_types.contains(&object.type_id) {
        return false;
    }
    if !purge.limit_groups.is_empty()
        && !object
            .groups
            .iter()
            .any(|group| purge.limit_groups.contains(group))
    {
        return false;
    }
    true
}

fn push_unique(into: &mut Vec<Uuid>, values: Vec<Uuid>) {
    for value in values {
        if !into.contains(&value) {
            into.push(value);
        }
    }
}

fn purge_one(defs: &DefinitionSet, change: &ModelChange) -> DataPurge {
    if !deletes_data(change) {
        return DataPurge::default();
    }
    match change {
        ModelChange::DeleteType { key } => type_purge(defs, key),
        ModelChange::DeleteAttribute { key } => attribute_purge(defs, key, None, None),
        ModelChange::DeleteValueList { key } => value_list_purge(defs, key, None),
        ModelChange::DeleteValueOption { list, option } => {
            value_list_purge(defs, list, Some(option))
        }
        ModelChange::DeleteUnitFamily { key } => unit_family_purge(defs, key, None),
        ModelChange::DeleteUnit { family, unit } => unit_family_purge(defs, family, Some(unit)),
        ModelChange::DeleteStructure { structure } => structure_purge(defs, structure),
        ModelChange::DeleteGroup {
            structure,
            group,
            move_to,
        } => group_purge(defs, structure, group, move_to.as_deref()),
        ModelChange::RemoveAttribute {
            structure,
            class_key,
            attribute,
        } => {
            let groups = group_tree(defs, structure, class_key);
            attribute_purge(defs, attribute, None, Some(groups))
        }
        ModelChange::DetachAttribute {
            attribute,
            type_key,
            structure,
            group,
        } => {
            let types = type_key
                .as_deref()
                .and_then(|token| find_type(defs, token))
                .map(|item| vec![item.id.as_uuid()]);
            let groups = match (structure.as_deref(), group.as_deref()) {
                (Some(structure), Some(group)) => Some(group_tree(defs, structure, group)),
                _ => None,
            };
            attribute_purge(defs, attribute, types, groups)
        }
        _ => DataPurge {
            destructive: true,
            confirm_name: String::new(),
            ..DataPurge::default()
        },
    }
}

fn type_purge(defs: &DefinitionSet, token: &str) -> DataPurge {
    let Some(object_type) = find_type(defs, token) else {
        return missing(token);
    };
    let link_attributes = defs
        .attributes
        .iter()
        .filter(|attribute| matches!(attribute.kind, AttributeKind::ObjectRef { .. }))
        .map(|attribute| attribute.id.as_uuid())
        .collect();
    DataPurge {
        destructive: true,
        confirm_name: object_type.singular_label("en"),
        link_attributes,
        delete_types: vec![object_type.id.as_uuid()],
        ..DataPurge::default()
    }
}

fn attribute_purge(
    defs: &DefinitionSet,
    token: &str,
    limit_types: Option<Vec<Uuid>>,
    limit_groups: Option<Vec<Uuid>>,
) -> DataPurge {
    let Some(attribute) = find_attribute(defs, token) else {
        return missing(token);
    };
    let link = matches!(attribute.kind, AttributeKind::ObjectRef { .. });
    DataPurge {
        destructive: true,
        confirm_name: caption(defs, &attribute.labels, &attribute.key),
        drop_attributes: vec![attribute.id.as_uuid()],
        link_attributes: if link {
            vec![attribute.id.as_uuid()]
        } else {
            Vec::new()
        },
        limit_types: limit_types.unwrap_or_default(),
        limit_groups: limit_groups.unwrap_or_default(),
        ..DataPurge::default()
    }
}

fn value_list_purge(defs: &DefinitionSet, token: &str, option: Option<&str>) -> DataPurge {
    let Some(list) = defs
        .value_lists
        .iter()
        .find(|item| same(token, &item.key, &item.id.as_uuid()))
    else {
        return missing(option.unwrap_or(token));
    };
    let confirm_name = match option {
        Some(option) => list
            .options
            .iter()
            .find(|item| same(option, &item.code, &Uuid::nil()))
            .map(|item| caption(defs, &item.labels, &item.code))
            .unwrap_or_else(|| option.to_owned()),
        None => caption(defs, &list.labels, &list.key),
    };
    let drop_attributes = defs
        .attributes
        .iter()
        .filter(|attribute| uses_list(attribute, &list.key))
        .map(|attribute| attribute.id.as_uuid())
        .collect();
    DataPurge {
        destructive: true,
        confirm_name,
        drop_attributes,
        match_value: option.map(|code| ValueMatch::Code(code.to_owned())),
        ..DataPurge::default()
    }
}

fn unit_family_purge(defs: &DefinitionSet, token: &str, unit: Option<&str>) -> DataPurge {
    let Some(family) = find_family(defs, token) else {
        return missing(unit.unwrap_or(token));
    };
    let confirm_name = match unit {
        Some(unit) => unit.to_owned(),
        None => caption(defs, &BTreeMap::new(), &family.key),
    };
    let drop_attributes = defs
        .attributes
        .iter()
        .filter(|attribute| attribute.kind.unit_family_id() == Some(family.id))
        .map(|attribute| attribute.id.as_uuid())
        .collect();
    DataPurge {
        destructive: true,
        confirm_name,
        drop_attributes,
        match_value: unit.map(|code| ValueMatch::Unit(code.to_owned())),
        ..DataPurge::default()
    }
}

fn structure_purge(defs: &DefinitionSet, token: &str) -> DataPurge {
    let Some(structure) = find_structure(defs, token) else {
        return missing(token);
    };
    DataPurge {
        destructive: true,
        confirm_name: caption(defs, &structure.labels, &structure.key),
        unplace_groups: structure
            .groups
            .iter()
            .map(|group| group.id.as_uuid())
            .collect(),
        ..DataPurge::default()
    }
}

fn group_purge(
    defs: &DefinitionSet,
    structure: &str,
    group: &str,
    move_to: Option<&str>,
) -> DataPurge {
    let Some(structure) = find_structure(defs, structure) else {
        return missing(group);
    };
    let Some(group) = structure
        .groups
        .iter()
        .find(|item| same(group, &item.key, &item.id.as_uuid()))
    else {
        return missing(group);
    };
    let name = caption(defs, &group.labels, &group.key);
    if move_to.is_some() {
        return DataPurge {
            destructive: true,
            confirm_name: name,
            ..DataPurge::default()
        };
    }
    let mut ids = Vec::new();
    collect_descendants(&structure.groups, group.id, &mut ids);
    DataPurge {
        destructive: true,
        confirm_name: name,
        unplace_groups: ids.into_iter().map(GroupId::as_uuid).collect(),
        ..DataPurge::default()
    }
}

fn group_tree(defs: &DefinitionSet, structure: &str, group: &str) -> Vec<Uuid> {
    let Some(structure) = find_structure(defs, structure) else {
        return Vec::new();
    };
    let Some(group) = structure
        .groups
        .iter()
        .find(|item| same(group, &item.key, &item.id.as_uuid()))
    else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    collect_descendants(&structure.groups, group.id, &mut ids);
    ids.into_iter().map(GroupId::as_uuid).collect()
}

fn collect_descendants(
    groups: &[crate::definition::Group],
    root: GroupId,
    into: &mut Vec<GroupId>,
) {
    into.push(root);
    for group in groups {
        if group.parent == Some(root) {
            collect_descendants(groups, group.id, into);
        }
    }
}

fn uses_list(attribute: &crate::definition::Attribute, list_key: &str) -> bool {
    match &attribute.kind {
        AttributeKind::ValueList { list, .. } => list.as_deref() == Some(list_key),
        _ => false,
    }
}

fn missing(name: &str) -> DataPurge {
    DataPurge {
        destructive: true,
        confirm_name: name.to_owned(),
        ..DataPurge::default()
    }
}

fn find_type<'a>(
    defs: &'a DefinitionSet,
    token: &str,
) -> Option<&'a crate::definition::ObjectType> {
    defs.types
        .iter()
        .find(|item| same(token, &item.key, &item.id.as_uuid()))
}

fn find_attribute<'a>(
    defs: &'a DefinitionSet,
    token: &str,
) -> Option<&'a crate::definition::Attribute> {
    defs.attributes
        .iter()
        .find(|item| same(token, &item.key, &item.id.as_uuid()))
}

fn find_structure<'a>(
    defs: &'a DefinitionSet,
    token: &str,
) -> Option<&'a crate::definition::Structure> {
    defs.structures
        .iter()
        .find(|item| same(token, &item.key, &item.id.as_uuid()))
}

fn find_family<'a>(defs: &'a DefinitionSet, token: &str) -> Option<&'a UnitFamily> {
    defs.unit_families
        .iter()
        .find(|item| same(token, &item.key, &item.id.as_uuid()))
}

fn same(token: &str, key: &str, id: &Uuid) -> bool {
    token.eq_ignore_ascii_case(key) || token == id.to_string()
}

fn caption(defs: &DefinitionSet, labels: &BTreeMap<LanguageTag, String>, fallback: &str) -> String {
    let pick = |tag: &str| {
        labels
            .iter()
            .find(|(item, text)| item.as_str() == tag && !text.is_empty())
            .map(|(_, text)| text.clone())
    };
    if let Some(tag) = defs.languages.first()
        && let Some(text) = pick(tag.as_str())
    {
        return text;
    }
    if let Some(text) = pick("en") {
        return text;
    }
    labels
        .values()
        .find(|text| !text.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// Applies a destructive definition edit. Returns whether `change` was one of them.
pub fn apply_destructive(defs: &mut DefinitionSet, change: &ModelChange) -> Result<bool, String> {
    match change {
        ModelChange::DeleteType { key } => {
            let id = find_type(defs, key)
                .map(|item| item.id)
                .ok_or_else(|| format!("unknown type {key}"))?;
            defs.types.retain(|item| item.id != id);
            Ok(true)
        }
        ModelChange::DeleteAttribute { key } => {
            let id = find_attribute(defs, key)
                .map(|item| item.id)
                .ok_or_else(|| format!("unknown attribute {key}"))?;
            if crate::standard::is_standard_attribute(id) {
                return Err(format!("attribute {key} is a standard attribute"));
            }
            if defs.types.iter().any(|item| item.key_attribute == id) {
                return Err(format!(
                    "attribute {key} is a type key and cannot be deleted"
                ));
            }
            defs.attributes.retain(|item| item.id != id);
            for object_type in &mut defs.types {
                object_type.attributes.retain(|item| *item != id);
            }
            for structure in &mut defs.structures {
                for group in &mut structure.groups {
                    group.attributes.retain(|item| *item != id);
                }
                for class in &mut structure.classes {
                    class.attributes.retain(|item| *item != id);
                }
            }
            Ok(true)
        }
        ModelChange::DeleteValueList { key } => {
            let list_key = defs
                .value_lists
                .iter()
                .find(|item| same(key, &item.key, &item.id.as_uuid()))
                .map(|item| item.key.clone())
                .ok_or_else(|| format!("unknown value list {key}"))?;
            defs.value_lists
                .retain(|item| !same(key, &item.key, &item.id.as_uuid()));
            for attribute in &mut defs.attributes {
                if let AttributeKind::ValueList { list, .. } = &mut attribute.kind
                    && list.as_deref() == Some(list_key.as_str())
                {
                    *list = None;
                }
            }
            Ok(true)
        }
        ModelChange::DeleteValueOption { list, option } => {
            let list = defs
                .value_lists
                .iter_mut()
                .find(|item| same(list, &item.key, &item.id.as_uuid()))
                .ok_or_else(|| format!("unknown value list {list}"))?;
            let before = list.options.len();
            list.options
                .retain(|item| !item.code.eq_ignore_ascii_case(option));
            if list.options.len() == before {
                return Err(format!("unknown option {option}"));
            }
            Ok(true)
        }
        ModelChange::DeleteUnitFamily { key } => {
            let id = find_family(defs, key)
                .map(|item| item.id)
                .ok_or_else(|| format!("unknown unit family {key}"))?;
            defs.unit_families.retain(|item| item.id != id);
            for attribute in &mut defs.attributes {
                clear_family(&mut attribute.kind, id);
            }
            Ok(true)
        }
        ModelChange::DeleteUnit { family, unit } => {
            let family = defs
                .unit_families
                .iter_mut()
                .find(|item| same(family, &item.key, &item.id.as_uuid()))
                .ok_or_else(|| format!("unknown unit family {family}"))?;
            if family.base_unit.eq_ignore_ascii_case(unit) {
                return Err(format!("unit {unit} is the base unit of {}", family.key));
            }
            let before = family.units.len();
            family
                .units
                .retain(|item| !item.code.eq_ignore_ascii_case(unit));
            if family.units.len() == before {
                return Err(format!("unknown unit {unit}"));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn clear_family(kind: &mut AttributeKind, family: crate::id::UnitFamilyId) {
    match kind {
        AttributeKind::Number { unit_family, .. } | AttributeKind::Range { unit_family }
            if *unit_family == Some(family) =>
        {
            *unit_family = None;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uuid::Uuid;

    use crate::definition::{
        Attribute, AttributeKind, DefinitionSet, Group, IndexFlags, ObjectType, SharedValueList,
        Structure, TextForm, Unit, UnitFamily, ValueOption,
    };
    use crate::id::{AttributeId, GroupId, StructureId, TypeId, UnitFamilyId, ValueListId};
    use crate::language::LanguageTag;
    use crate::model_change::{ModelChange, deletes_data};

    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn label(text: &str) -> BTreeMap<LanguageTag, String> {
        let mut labels = BTreeMap::new();
        labels.insert(LanguageTag::parse("en").expect("en"), text.to_owned());
        labels
    }

    fn text_attr(n: u128, key: &str, name: &str) -> Attribute {
        Attribute {
            id: AttributeId::from_uuid(id(n)),
            key: key.to_owned(),
            labels: label(name),
            language_dependent: false,
            multiple: false,
            required: false,
            kind: AttributeKind::Text {
                max_length: None,
                form: TextForm::Short,
            },
            index: IndexFlags::default(),
        }
    }

    fn sample() -> (DefinitionSet, UsageFixture) {
        let product = id(1);
        let article = id(2);
        let name = id(10);
        let brand = id(11);
        let weight = id(12);
        let color = id(13);
        let mass = id(20);
        let colors = id(21);
        let food = id(30);
        let dairy = id(31);
        let cheese = id(32);
        let o1 = id(101);
        let o2 = id(102);
        let o3 = id(103);
        let name_attr = text_attr(10, "name", "Name");
        let mut brand_attr = text_attr(11, "brand", "Brand");
        brand_attr.kind = AttributeKind::ObjectRef { type_key: None };
        let mut weight_attr = text_attr(12, "weight", "Weight");
        weight_attr.kind = AttributeKind::Number {
            unit_family: Some(UnitFamilyId::from_uuid(mass)),
            min: None,
            max: None,
            integer: false,
        };
        let mut color_attr = text_attr(13, "color", "Color");
        color_attr.kind = AttributeKind::ValueList {
            options: Vec::new(),
            list: Some("color".into()),
        };
        let defs = DefinitionSet {
            model_revision: 1,
            languages: vec![LanguageTag::parse("en").expect("en")],
            unit_families: vec![UnitFamily {
                id: UnitFamilyId::from_uuid(mass),
                key: "mass".into(),
                dimension: None,
                base_unit: "kg".into(),
                units: vec![
                    Unit {
                        code: "kg".into(),
                        factor: "1".parse().expect("factor"),
                    },
                    Unit {
                        code: "g".into(),
                        factor: "0.001".parse().expect("factor"),
                    },
                ],
            }],
            value_lists: vec![SharedValueList {
                id: ValueListId::from_uuid(colors),
                key: "color".into(),
                labels: label("Color"),
                options: vec![ValueOption {
                    code: "red".into(),
                    labels: label("Red"),
                }],
            }],
            attributes: vec![name_attr, brand_attr, weight_attr, color_attr],
            types: vec![
                ObjectType {
                    id: TypeId::from_uuid(product),
                    key: "product".into(),
                    labels: label("Product"),
                    singular: label("Product"),
                    plural: label("Products"),
                    key_attribute: AttributeId::from_uuid(name),
                    attributes: vec![
                        AttributeId::from_uuid(name),
                        AttributeId::from_uuid(brand),
                        AttributeId::from_uuid(weight),
                        AttributeId::from_uuid(color),
                    ],
                },
                ObjectType {
                    id: TypeId::from_uuid(article),
                    key: "article".into(),
                    labels: label("Article"),
                    singular: label("Article"),
                    plural: label("Articles"),
                    key_attribute: AttributeId::from_uuid(name),
                    attributes: vec![AttributeId::from_uuid(name)],
                },
            ],
            structures: vec![Structure {
                id: StructureId::from_uuid(food),
                key: "food".into(),
                labels: label("Food"),
                groups: vec![
                    Group {
                        id: GroupId::from_uuid(dairy),
                        key: "dairy".into(),
                        labels: label("Dairy"),
                        parent: None,
                        attributes: vec![AttributeId::from_uuid(name)],
                    },
                    Group {
                        id: GroupId::from_uuid(cheese),
                        key: "cheese".into(),
                        labels: label("Cheese"),
                        parent: Some(GroupId::from_uuid(dairy)),
                        attributes: Vec::new(),
                    },
                ],
                classes: Vec::new(),
            }],
        };
        let fixture = UsageFixture {
            objects: vec![
                StoredObject {
                    id: o1,
                    type_id: product,
                    groups: vec![cheese],
                },
                StoredObject {
                    id: o2,
                    type_id: product,
                    groups: vec![dairy],
                },
                StoredObject {
                    id: o3,
                    type_id: article,
                    groups: Vec::new(),
                },
            ],
            values: vec![
                StoredValue {
                    object: o1,
                    attribute: name,
                    count: 1,
                    code: None,
                    link: false,
                },
                StoredValue {
                    object: o1,
                    attribute: brand,
                    count: 1,
                    code: None,
                    link: true,
                },
                StoredValue {
                    object: o1,
                    attribute: weight,
                    count: 1,
                    code: Some("kg".into()),
                    link: false,
                },
                StoredValue {
                    object: o1,
                    attribute: color,
                    count: 1,
                    code: Some("red".into()),
                    link: false,
                },
                StoredValue {
                    object: o2,
                    attribute: name,
                    count: 1,
                    code: None,
                    link: false,
                },
            ],
            links: vec![(o1, o3)],
            selections: BTreeMap::from([(product, 2), (article, 1)]),
        };
        (defs, fixture)
    }

    fn counts(
        defs: &DefinitionSet,
        change: ModelChange,
        fixture: &UsageFixture,
    ) -> (bool, ConsequenceCounts) {
        let purge = purge_for(defs, &[change]);
        let counts = tally(&purge, fixture);
        (purge.destructive, counts)
    }

    #[test]
    fn counts_follow_each_step() {
        let (defs, fixture) = sample();
        let steps = vec![
            ModelChange::CreateAttribute {
                key: "note".into(),
                label: "Note".into(),
                kind: "text".into(),
                unit: None,
                multiple: false,
                languages: false,
                required: false,
                searchable: false,
                filterable: false,
                sortable: false,
            },
            ModelChange::UpdateAttribute {
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
            },
            ModelChange::UpdateType {
                key: "product".into(),
                language: Some("en".into()),
                singular: Some("Product".into()),
                plural: None,
            },
            ModelChange::RenameTypeKey {
                key: "product".into(),
                new_key: "products".into(),
            },
            ModelChange::RenameAttributeKey {
                key: "name".into(),
                new_key: "title".into(),
            },
            ModelChange::RenameStructureKey {
                structure: "food".into(),
                new_key: "foods".into(),
            },
            ModelChange::RenameGroupKey {
                structure: "food".into(),
                group: "cheese".into(),
                new_key: "cheeses".into(),
            },
            ModelChange::RenameValueListKey {
                key: "color".into(),
                new_key: "colours".into(),
            },
            ModelChange::RenameUnitFamilyKey {
                key: "mass".into(),
                new_key: "weight".into(),
            },
            ModelChange::AddAttribute {
                structure: "food".into(),
                class_key: "dairy".into(),
                attribute: "name".into(),
            },
            ModelChange::AttachAttribute {
                attribute: "name".into(),
                type_key: Some("article".into()),
                structure: None,
                group: None,
            },
            ModelChange::ReorderAttributes {
                structure: "food".into(),
                class_key: "dairy".into(),
                attributes: vec!["name".into()],
            },
            ModelChange::MoveGroup {
                structure: "food".into(),
                group: "cheese".into(),
                parent: None,
            },
            ModelChange::CreateGroup {
                structure: "food".into(),
                group: "butter".into(),
                parent: None,
                label: "Butter".into(),
            },
            ModelChange::CreateStructure {
                key: "aisle".into(),
                label: "Aisle".into(),
            },
            ModelChange::RenameStructure {
                structure: "food".into(),
                label: "Foods".into(),
            },
            ModelChange::RenameGroup {
                structure: "food".into(),
                group: "cheese".into(),
                label: "Cheeses".into(),
            },
            ModelChange::DuplicateGroup {
                structure: "food".into(),
                group: "cheese".into(),
                name: "cheese_copy".into(),
            },
            ModelChange::MoveClass {
                structure: "food".into(),
                class_key: "dairy".into(),
                group: "cheese".into(),
            },
            ModelChange::SetParent {
                structure: "food".into(),
                class_key: "cheese".into(),
                parent: Some("dairy".into()),
            },
            ModelChange::CreateClass {
                structure: "food".into(),
                class_key: "soft".into(),
                group: "cheese".into(),
                parent: None,
                attributes: Vec::new(),
            },
        ];
        for step in steps {
            let (destructive, counts) = counts(&defs, step.clone(), &fixture);
            assert!(!destructive, "{step:?} is not a delete");
            assert!(!deletes_data(&step));
            assert_eq!(counts, ConsequenceCounts::default(), "{step:?}");
        }

        let (destructive, counted) = counts(
            &defs,
            ModelChange::DeleteType {
                key: "product".into(),
            },
            &fixture,
        );
        assert!(destructive);
        assert_eq!(counted.removed, 2);
        assert_eq!(counted.values, 5);
        assert_eq!(counted.links, 2, "one relation and one object reference");
        assert_eq!(counted.selections, 2);
        assert_eq!(
            consequence_sentence(&counted),
            "5 values on 2 objects will be deleted"
        );

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteAttribute { key: "name".into() },
            &fixture,
        );
        assert_eq!(counted.removed, 2);
        assert_eq!(counted.values, 2);
        assert_eq!(counted.links, 0);

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteAttribute {
                key: "brand".into(),
            },
            &fixture,
        );
        assert_eq!(counted.values, 1);
        assert_eq!(counted.links, 1);

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteValueList {
                key: "color".into(),
            },
            &fixture,
        );
        assert_eq!(counted.values, 1);
        assert_eq!(
            purge_for(
                &defs,
                &[ModelChange::DeleteValueList {
                    key: "color".into()
                }]
            )
            .confirm_name,
            "Color"
        );

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteValueOption {
                list: "color".into(),
                option: "red".into(),
            },
            &fixture,
        );
        assert_eq!(counted.values, 1);
        assert_eq!(
            purge_for(
                &defs,
                &[ModelChange::DeleteValueOption {
                    list: "color".into(),
                    option: "red".into(),
                }]
            )
            .confirm_name,
            "Red"
        );

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteUnitFamily { key: "mass".into() },
            &fixture,
        );
        assert_eq!(counted.values, 1);

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteUnit {
                family: "mass".into(),
                unit: "kg".into(),
            },
            &fixture,
        );
        assert_eq!(counted.values, 1);
        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteUnit {
                family: "mass".into(),
                unit: "g".into(),
            },
            &fixture,
        );
        assert_eq!(counted.values, 0);
        assert!(deletes_data(&ModelChange::DeleteUnit {
            family: "mass".into(),
            unit: "g".into(),
        }));

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteStructure {
                structure: "food".into(),
            },
            &fixture,
        );
        assert_eq!(counted.unplaced, 2);
        assert_eq!(counted.values, 0);
        assert_eq!(
            consequence_sentence(&counted),
            "2 objects will be removed from the tree"
        );

        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteGroup {
                structure: "food".into(),
                group: "dairy".into(),
                move_to: None,
            },
            &fixture,
        );
        assert_eq!(counted.unplaced, 2);
        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteGroup {
                structure: "food".into(),
                group: "cheese".into(),
                move_to: None,
            },
            &fixture,
        );
        assert_eq!(counted.unplaced, 1);
        let (_, counted) = counts(
            &defs,
            ModelChange::DeleteGroup {
                structure: "food".into(),
                group: "cheese".into(),
                move_to: Some("dairy".into()),
            },
            &fixture,
        );
        assert_eq!(counted.unplaced, 0, "a move keeps the objects in the tree");
        assert!(deletes_data(&ModelChange::DeleteGroup {
            structure: "food".into(),
            group: "cheese".into(),
            move_to: Some("dairy".into()),
        }));

        let (_, counted) = counts(
            &defs,
            ModelChange::RemoveAttribute {
                structure: "food".into(),
                class_key: "cheese".into(),
                attribute: "name".into(),
            },
            &fixture,
        );
        assert_eq!(counted.removed, 1);
        assert_eq!(counted.values, 1);

        let (_, counted) = counts(
            &defs,
            ModelChange::DetachAttribute {
                attribute: "name".into(),
                type_key: Some("product".into()),
                structure: None,
                group: None,
            },
            &fixture,
        );
        assert_eq!(counted.removed, 2);
        assert_eq!(counted.values, 2);

        assert_eq!(grouped(1240), "1,240");
        assert_eq!(grouped(248), "248");
        assert_eq!(
            consequence_sentence(&ConsequenceCounts {
                objects: 248,
                links: 0,
                values: 1240,
                selections: 0,
                removed: 248,
                unplaced: 0,
            }),
            "1,240 values on 248 objects will be deleted"
        );
    }

    #[test]
    fn deleting_an_attribute_drops_it_from_types_and_groups() {
        let (mut defs, _) = sample();
        apply_destructive(
            &mut defs,
            &ModelChange::DeleteAttribute {
                key: "brand".into(),
            },
        )
        .expect("delete");
        assert!(defs.attributes.iter().all(|item| item.key != "brand"));
        assert!(defs.types.iter().all(|item| {
            item.attributes
                .iter()
                .all(|attribute| attribute.as_uuid() != id(11))
        }));
    }

    #[test]
    fn deleting_a_type_removes_its_selections_from_the_plan() {
        let (defs, fixture) = sample();
        let purge = purge_for(
            &defs,
            &[ModelChange::DeleteType {
                key: "article".into(),
            }],
        );
        let counts = tally(&purge, &fixture);
        assert_eq!(counts.selections, 1);
        assert_eq!(counts.removed, 1);
        assert_eq!(counts.links, 1);
        assert_eq!(purge.confirm_name, "Article");
    }
}
