//! Ids are the stable address. Keys are labels that can change.
//!
//! A token is an id or a key. Stored view state, assortment queries and import
//! mappings keep ids. A key that is still stored is translated when it is read.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::definition::{AttributeKind, DefinitionSet, Structure};
use crate::id::{AttributeId, GroupId, StructureId, TypeId, UnitFamilyId, ValueListId};
use crate::language::LanguageTag;

/// What a caller asked to address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Type,
    Attribute,
    Structure,
    Group,
    ValueList,
    UnitFamily,
}

impl RefKind {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "type" | "types" => Some(Self::Type),
            "attribute" | "attributes" => Some(Self::Attribute),
            "structure" | "structures" => Some(Self::Structure),
            "group" | "groups" => Some(Self::Group),
            "value_list" | "value-list" | "value_lists" | "value-lists" => Some(Self::ValueList),
            "unit_family" | "unit-family" | "unit_families" | "unit-families" => {
                Some(Self::UnitFamily)
            }
            _ => None,
        }
    }
}

/// `{id, key, label}` for one model entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    pub id: String,
    pub key: String,
    pub label: String,
}

/// A key change that external callers still using the old key will notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRename {
    pub kind: &'static str,
    pub old_key: String,
    pub new_key: String,
}

/// Warning shown before a key change is saved, and returned by the API.
pub fn key_rename_warning(old_key: &str) -> String {
    format!("External integrations that use the old key « {old_key} » will stop working")
}

/// Typesense field for an attribute. Hyphens in the id become underscores.
pub fn attribute_index_field(id: AttributeId, language: Option<&str>) -> String {
    let mut name = format!("a_{}", id.as_uuid().to_string().replace('-', "_"));
    if let Some(language) = language.filter(|value| !value.is_empty()) {
        name.push_str("__");
        name.push_str(&language.replace('-', "_"));
    }
    name
}

/// A new key must be non-empty and must not be a UUID.
pub fn check_new_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("key is empty".into());
    }
    if Uuid::parse_str(key).is_ok() {
        return Err(format!(
            "key {key} must not be a UUID, so a value can be addressed by key or id"
        ));
    }
    Ok(())
}

pub fn resolve(
    defs: &DefinitionSet,
    kind: RefKind,
    token: &str,
    structure: Option<&str>,
) -> Result<ResolvedRef, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("missing id or key".into());
    }
    match kind {
        RefKind::Type => defs
            .types
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
            .map(|item| {
                resolved(
                    item.id.as_uuid(),
                    &item.key,
                    &item.singular_label(first_language(defs)),
                )
            })
            .ok_or_else(|| format!("unknown type {token}")),
        RefKind::Attribute => defs
            .attributes
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
            .map(|item| {
                resolved(
                    item.id.as_uuid(),
                    &item.key,
                    &label_of(&item.labels, &item.key, defs),
                )
            })
            .ok_or_else(|| format!("unknown attribute {token}")),
        RefKind::Structure => defs
            .structures
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
            .map(|item| {
                resolved(
                    item.id.as_uuid(),
                    &item.key,
                    &label_of(&item.labels, &item.key, defs),
                )
            })
            .ok_or_else(|| format!("unknown structure {token}")),
        RefKind::Group => resolve_group(defs, token, structure),
        RefKind::ValueList => defs
            .value_lists
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
            .map(|item| {
                resolved(
                    item.id.as_uuid(),
                    &item.key,
                    &label_of(&item.labels, &item.key, defs),
                )
            })
            .ok_or_else(|| format!("unknown value list {token}")),
        RefKind::UnitFamily => defs
            .unit_families
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
            .map(|item| resolved(item.id.as_uuid(), &item.key, &item.key))
            .ok_or_else(|| format!("unknown unit family {token}")),
    }
}

fn resolve_group(
    defs: &DefinitionSet,
    token: &str,
    structure_token: Option<&str>,
) -> Result<ResolvedRef, String> {
    if let Some(structure_token) = structure_token.filter(|value| !value.is_empty()) {
        let structure = defs
            .structures
            .iter()
            .find(|item| same(&item.id.as_uuid().to_string(), &item.key, structure_token))
            .ok_or_else(|| format!("unknown structure {structure_token}"))?;
        let group = structure
            .groups
            .iter()
            .find(|group| same(&group.id.as_uuid().to_string(), &group.key, token))
            .ok_or_else(|| format!("unknown group {token} in {}", structure.key))?;
        return Ok(resolved(
            group.id.as_uuid(),
            &group.key,
            &label_of(&group.labels, &group.key, defs),
        ));
    }
    if let Ok(id) = Uuid::parse_str(token) {
        let group_id = GroupId::from_uuid(id);
        for structure in &defs.structures {
            if let Some(group) = structure.groups.iter().find(|group| group.id == group_id) {
                return Ok(resolved(
                    group.id.as_uuid(),
                    &group.key,
                    &label_of(&group.labels, &group.key, defs),
                ));
            }
        }
        return Err(format!("unknown group {token}"));
    }
    let mut found: Vec<ResolvedRef> = Vec::new();
    for structure in &defs.structures {
        for group in &structure.groups {
            if group.key.eq_ignore_ascii_case(token) {
                found.push(resolved(
                    group.id.as_uuid(),
                    &group.key,
                    &label_of(&group.labels, &group.key, defs),
                ));
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("unknown group {token}")),
        _ => Err(format!(
            "group {token} is used in more than one structure; pass structure"
        )),
    }
}

/// Key changes between two revisions of the same entities.
pub fn key_renames(before: &DefinitionSet, after: &DefinitionSet) -> Vec<KeyRename> {
    let mut found = Vec::new();
    for item in &after.attributes {
        if let Some(old) = before.attributes.iter().find(|old| old.id == item.id)
            && old.key != item.key
        {
            found.push(KeyRename {
                kind: "attribute",
                old_key: old.key.clone(),
                new_key: item.key.clone(),
            });
        }
    }
    for item in &after.types {
        if let Some(old) = before.types.iter().find(|old| old.id == item.id)
            && old.key != item.key
        {
            found.push(KeyRename {
                kind: "type",
                old_key: old.key.clone(),
                new_key: item.key.clone(),
            });
        }
    }
    for item in &after.structures {
        if let Some(old) = before.structures.iter().find(|old| old.id == item.id)
            && old.key != item.key
        {
            found.push(KeyRename {
                kind: "structure",
                old_key: old.key.clone(),
                new_key: item.key.clone(),
            });
        }
        let Some(old_structure) = before.structures.iter().find(|old| old.id == item.id) else {
            continue;
        };
        for group in &item.groups {
            if let Some(old) = old_structure.groups.iter().find(|old| old.id == group.id)
                && old.key != group.key
            {
                found.push(KeyRename {
                    kind: "group",
                    old_key: old.key.clone(),
                    new_key: group.key.clone(),
                });
            }
        }
    }
    for item in &after.value_lists {
        if let Some(old) = before.value_lists.iter().find(|old| old.id == item.id)
            && old.key != item.key
        {
            found.push(KeyRename {
                kind: "value list",
                old_key: old.key.clone(),
                new_key: item.key.clone(),
            });
        }
    }
    for item in &after.unit_families {
        if let Some(old) = before.unit_families.iter().find(|old| old.id == item.id)
            && old.key != item.key
        {
            found.push(KeyRename {
                kind: "unit family",
                old_key: old.key.clone(),
                new_key: item.key.clone(),
            });
        }
    }
    found
}

/// True when the search document stores this key, so a rename needs a reindex.
pub fn rename_needs_reindex(kind: &str) -> bool {
    matches!(kind, "type" | "structure" | "group")
}

pub fn translate_view_state(state: &mut Value, defs: &DefinitionSet) {
    let Some(object) = state.as_object_mut() else {
        return;
    };
    if let Some(columns) = object.get_mut("columns").and_then(Value::as_array_mut) {
        for column in columns {
            if let Some(text) = column.as_str() {
                *column = Value::String(translate_column(text, defs));
            }
        }
    }
    if let Some(text) = object.get("sort").and_then(Value::as_str) {
        let next = translate_sort_token(text, defs);
        object.insert("sort".into(), Value::String(next));
    }
    if let Some(text) = object.get("type").and_then(Value::as_str) {
        let next = id_of(defs, RefKind::Type, text).unwrap_or_else(|| text.to_owned());
        object.insert("type".into(), Value::String(next));
    }
    if let Some(text) = object.get("type_key").and_then(Value::as_str) {
        let next = id_of(defs, RefKind::Type, text).unwrap_or_else(|| text.to_owned());
        object.insert("type_key".into(), Value::String(next));
    }
    if let Some(expanded) = object.get_mut("expanded").and_then(Value::as_array_mut) {
        for item in expanded {
            if let Some(text) = item.as_str() {
                *item = Value::String(translate_group_token(text, defs));
            }
        }
    }
    if let Some(table) = object.get_mut("table").and_then(Value::as_object_mut) {
        translate_table(table, defs);
    }
}

pub fn translate_assortment_query(query: &mut Value, defs: &DefinitionSet) {
    let Some(object) = query.as_object_mut() else {
        return;
    };
    if let Some(text) = object.get("type").and_then(Value::as_str) {
        let next = id_of(defs, RefKind::Type, text).unwrap_or_else(|| text.to_owned());
        object.insert("type".into(), Value::String(next));
    }
    if let Some(text) = object.get("group").and_then(Value::as_str) {
        let next = translate_group_token(text, defs);
        object.insert("group".into(), Value::String(next));
    }
    if let Some(filter) = object.get("filter").and_then(Value::as_str) {
        let next = translate_filter(filter, defs);
        object.insert("filter".into(), Value::String(next));
    }
}

pub fn translate_import_mapping(mapping: &mut Value, defs: &DefinitionSet) {
    if mapping.is_null() {
        return;
    }
    let Some(object) = mapping.as_object_mut() else {
        return;
    };
    if let Some(text) = object.get("type").and_then(Value::as_str) {
        let next = id_of(defs, RefKind::Type, text).unwrap_or_else(|| text.to_owned());
        object.insert("type".into(), Value::String(next));
    }
    if let Some(columns) = object.get_mut("columns").and_then(Value::as_array_mut) {
        for column in columns {
            let Some(target) = column.get("target").and_then(Value::as_str) else {
                continue;
            };
            let next = translate_import_target(target, defs);
            if let Some(object) = column.as_object_mut() {
                object.insert("target".into(), Value::String(next));
            }
        }
    }
}

fn translate_table(table: &mut Map<String, Value>, defs: &DefinitionSet) {
    for key in ["sort", "groupBy", "filter"] {
        if let Some(text) = table.get(key).and_then(Value::as_str) {
            let next = match key {
                "filter" => translate_filter(text, defs),
                _ => translate_sort_token(text, defs),
            };
            table.insert(key.into(), Value::String(next));
        }
    }
    if let Some(order) = table.get_mut("order").and_then(Value::as_array_mut) {
        for item in order {
            if let Some(text) = item.as_str() {
                *item = Value::String(translate_column(text, defs));
            }
        }
    }
}

fn translate_column(token: &str, defs: &DefinitionSet) -> String {
    // Built-in table columns stay as they are. `name` and `status` are
    // translated when they are attributes.
    if matches!(token, "key" | "type" | "version") {
        return token.to_owned();
    }
    id_of(defs, RefKind::Attribute, token).unwrap_or_else(|| token.to_owned())
}

fn translate_sort_token(token: &str, defs: &DefinitionSet) -> String {
    let (field, direction) = token
        .rsplit_once(':')
        .filter(|(_, direction)| {
            direction.eq_ignore_ascii_case("asc") || direction.eq_ignore_ascii_case("desc")
        })
        .unwrap_or((token, ""));
    let field = translate_index_field(field, defs);
    if direction.is_empty() {
        field
    } else {
        format!("{field}:{direction}")
    }
}

fn translate_index_field(field: &str, defs: &DefinitionSet) -> String {
    if matches!(
        field,
        "key" | "type_key" | "version" | "seq" | "deleted" | ""
    ) {
        return field.to_owned();
    }
    let (body, language) = field
        .rsplit_once("__")
        .map(|(body, language)| (body, Some(language)))
        .unwrap_or((field, None));
    let Some(token) = body.strip_prefix("a_") else {
        return id_of(defs, RefKind::Attribute, field).unwrap_or_else(|| field.to_owned());
    };
    if let Some(attribute) = attribute_from_index_token(defs, token) {
        return attribute_index_field(attribute, language);
    }
    field.to_owned()
}

fn translate_filter(filter: &str, defs: &DefinitionSet) -> String {
    filter
        .split(" && ")
        .map(|part| translate_filter_part(part.trim(), defs))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn translate_filter_part(part: &str, defs: &DefinitionSet) -> String {
    let Some((field, rest)) = part.split_once(":=") else {
        return part.to_owned();
    };
    if field == "type_key" {
        let id = id_of(defs, RefKind::Type, rest).unwrap_or_else(|| rest.to_owned());
        return format!("type_key:={id}");
    }
    if field == "groups" {
        return format!("groups:={}", translate_group_token(rest, defs));
    }
    if field.starts_with("a_") {
        let (body, language) = field
            .rsplit_once("__")
            .map(|(body, language)| (body, Some(language)))
            .unwrap_or((field, None));
        let token = body.strip_prefix("a_").unwrap_or(body);
        if let Some(attribute) = attribute_from_index_token(defs, token) {
            return format!("{}:={rest}", attribute_index_field(attribute, language));
        }
    }
    part.to_owned()
}

fn translate_group_token(token: &str, defs: &DefinitionSet) -> String {
    if let Some((structure, group)) = token.split_once(':') {
        let structure_id = id_of(defs, RefKind::Structure, structure);
        let group_id = resolve(defs, RefKind::Group, group, Some(structure))
            .ok()
            .map(|item| item.id);
        return match (structure_id, group_id) {
            (Some(structure_id), Some(group_id)) => format!("{structure_id}:{group_id}"),
            _ => token.to_owned(),
        };
    }
    id_of(defs, RefKind::Group, token).unwrap_or_else(|| token.to_owned())
}

fn translate_import_target(target: &str, defs: &DefinitionSet) -> String {
    if matches!(target, "action" | "unmapped" | "") {
        return target.to_owned();
    }
    let (body, language) = target
        .rsplit_once('@')
        .filter(|(_, language)| !language.is_empty() && !language.contains(':'))
        .unwrap_or((target, ""));
    let next = if let Some(rest) = body.strip_prefix("membership:") {
        id_of(defs, RefKind::Structure, rest)
            .map(|id| format!("membership:{id}"))
            .unwrap_or_else(|| body.to_owned())
    } else if let Some(rest) = body.strip_prefix("type:") {
        id_of(defs, RefKind::Attribute, rest)
            .map(|id| format!("type:{id}"))
            .unwrap_or_else(|| body.to_owned())
    } else if let Some(rest) = body.strip_prefix("group:") {
        translate_group_target(rest, defs).unwrap_or_else(|| body.to_owned())
    } else {
        body.to_owned()
    };
    if language.is_empty() {
        next
    } else {
        format!("{next}@{language}")
    }
}

fn translate_group_target(rest: &str, defs: &DefinitionSet) -> Option<String> {
    let (path, attribute) = rest.rsplit_once(':')?;
    let (structure, group) = path.split_once('/')?;
    let structure_id = id_of(defs, RefKind::Structure, structure)?;
    let group_id = resolve(defs, RefKind::Group, group, Some(structure))
        .ok()?
        .id;
    let attribute_id = id_of(defs, RefKind::Attribute, attribute)?;
    Some(format!("group:{structure_id}/{group_id}:{attribute_id}"))
}

fn id_of(defs: &DefinitionSet, kind: RefKind, token: &str) -> Option<String> {
    resolve(defs, kind, token, None).ok().map(|item| item.id)
}

fn attribute(defs: &DefinitionSet, token: &str) -> Option<AttributeId> {
    resolve(defs, RefKind::Attribute, token, None)
        .ok()
        .and_then(|item| Uuid::parse_str(&item.id).ok())
        .map(AttributeId::from_uuid)
}

fn attribute_from_index_token(defs: &DefinitionSet, token: &str) -> Option<AttributeId> {
    if let Some(id) = uuid_from_index_token(token) {
        let attribute = AttributeId::from_uuid(id);
        if defs.attributes.iter().any(|item| item.id == attribute) {
            return Some(attribute);
        }
    }
    attribute(defs, token)
}

fn uuid_from_index_token(token: &str) -> Option<Uuid> {
    if Uuid::parse_str(token).is_ok() {
        return Uuid::parse_str(token).ok();
    }
    if token.len() == 36 && token.as_bytes().get(8) == Some(&b'_') {
        return Uuid::parse_str(&token.replace('_', "-")).ok();
    }
    None
}

fn same(id: &str, key: &str, token: &str) -> bool {
    id.eq_ignore_ascii_case(token) || key.eq_ignore_ascii_case(token)
}

fn resolved(id: Uuid, key: &str, label: &str) -> ResolvedRef {
    ResolvedRef {
        id: id.to_string(),
        key: key.to_owned(),
        label: label.to_owned(),
    }
}

fn first_language(defs: &DefinitionSet) -> &str {
    defs.languages
        .first()
        .map(LanguageTag::as_str)
        .unwrap_or("en")
}

fn label_of(
    labels: &BTreeMap<LanguageTag, String>,
    fallback: &str,
    defs: &DefinitionSet,
) -> String {
    let language = first_language(defs);
    labels
        .iter()
        .find(|(tag, value)| tag.as_str() == language && !value.is_empty())
        .or_else(|| {
            labels
                .iter()
                .find(|(tag, value)| tag.as_str() == "en" && !value.is_empty())
        })
        .map(|(_, value)| value.clone())
        .or_else(|| labels.values().find(|value| !value.is_empty()).cloned())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Point value-list and object-reference keys at the key that was just set.
pub fn retarget_key_strings(defs: &mut DefinitionSet, kind: RefKind, old_key: &str, new_key: &str) {
    match kind {
        RefKind::ValueList => {
            for attribute in &mut defs.attributes {
                if let AttributeKind::ValueList {
                    list: Some(list), ..
                } = &mut attribute.kind
                    && list == old_key
                {
                    *list = new_key.to_owned();
                }
            }
        }
        RefKind::Type => {
            for attribute in &mut defs.attributes {
                if let AttributeKind::ObjectRef {
                    type_key: Some(type_key),
                } = &mut attribute.kind
                    && type_key == old_key
                {
                    *type_key = new_key.to_owned();
                }
            }
        }
        _ => {}
    }
}

pub fn duplicate_key(
    defs: &DefinitionSet,
    kind: RefKind,
    key: &str,
    structure: Option<&str>,
) -> bool {
    match kind {
        RefKind::Attribute => defs
            .attributes
            .iter()
            .any(|item| item.key.eq_ignore_ascii_case(key)),
        RefKind::Type => defs
            .types
            .iter()
            .any(|item| item.key.eq_ignore_ascii_case(key)),
        RefKind::Structure => defs
            .structures
            .iter()
            .any(|item| item.key.eq_ignore_ascii_case(key)),
        RefKind::UnitFamily => defs
            .unit_families
            .iter()
            .any(|item| item.key.eq_ignore_ascii_case(key)),
        RefKind::ValueList => defs
            .value_lists
            .iter()
            .any(|item| item.key.eq_ignore_ascii_case(key)),
        RefKind::Group => structure_of(defs, structure.unwrap_or(""))
            .into_iter()
            .flat_map(|item| item.groups.iter())
            .any(|group| group.key.eq_ignore_ascii_case(key)),
    }
}

fn structure_of<'a>(defs: &'a DefinitionSet, token: &str) -> Option<&'a Structure> {
    defs.structures
        .iter()
        .find(|item| same(&item.id.as_uuid().to_string(), &item.key, token))
}

/// Used by rename steps that need the concrete id newtypes.
pub fn attribute_id(defs: &DefinitionSet, token: &str) -> Result<AttributeId, String> {
    let resolved = resolve(defs, RefKind::Attribute, token, None)?;
    Ok(AttributeId::from_uuid(
        Uuid::parse_str(&resolved.id).expect("attribute id"),
    ))
}

pub fn type_id(defs: &DefinitionSet, token: &str) -> Result<TypeId, String> {
    let resolved = resolve(defs, RefKind::Type, token, None)?;
    Ok(TypeId::from_uuid(
        Uuid::parse_str(&resolved.id).expect("type id"),
    ))
}

pub fn structure_id(defs: &DefinitionSet, token: &str) -> Result<StructureId, String> {
    let resolved = resolve(defs, RefKind::Structure, token, None)?;
    Ok(StructureId::from_uuid(
        Uuid::parse_str(&resolved.id).expect("structure id"),
    ))
}

pub fn value_list_id(defs: &DefinitionSet, token: &str) -> Result<ValueListId, String> {
    let resolved = resolve(defs, RefKind::ValueList, token, None)?;
    Ok(ValueListId::from_uuid(
        Uuid::parse_str(&resolved.id).expect("value list id"),
    ))
}

pub fn unit_family_id(defs: &DefinitionSet, token: &str) -> Result<UnitFamilyId, String> {
    let resolved = resolve(defs, RefKind::UnitFamily, token, None)?;
    Ok(UnitFamilyId::from_uuid(
        Uuid::parse_str(&resolved.id).expect("unit family id"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> DefinitionSet {
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap()
    }

    #[test]
    fn id_and_key_resolve_to_the_same_attribute() {
        let defs = defs();
        let by_key = resolve(&defs, RefKind::Attribute, "sku", None).unwrap();
        let by_id = resolve(&defs, RefKind::Attribute, &by_key.id, None).unwrap();
        assert_eq!(by_key, by_id);
        assert_eq!(by_key.key, "sku");
        assert!(!by_key.label.is_empty());
    }

    #[test]
    fn view_state_and_import_targets_keep_ids_when_read_again() {
        let defs = defs();
        let sku = resolve(&defs, RefKind::Attribute, "sku", None).unwrap();
        let mut state = serde_json::json!({
            "columns": ["sku", "name", "type"],
            "sort": "sku",
            "type_key": "item",
            "table": { "sort": "a_sku:asc", "groupBy": "a_active", "filter": "a_sku:=SKU-1" }
        });
        translate_view_state(&mut state, &defs);
        translate_view_state(&mut state, &defs);
        assert_eq!(state["columns"][0], sku.id);
        assert_eq!(state["columns"][2], "type");
        assert_eq!(state["sort"], sku.id);
        let item = resolve(&defs, RefKind::Type, "item", None).unwrap();
        assert_eq!(state["type_key"], item.id);
        let field = attribute_index_field(
            AttributeId::from_uuid(Uuid::parse_str(&sku.id).unwrap()),
            None,
        );
        assert_eq!(state["table"]["sort"], format!("{field}:asc"));
        assert_eq!(state["table"]["filter"], format!("{field}:=SKU-1"));

        let mut mapping = serde_json::json!({
            "type": "item",
            "columns": [
                { "col": 0, "target": "type:sku" },
                { "col": 1, "target": "type:name@en-GB" },
                { "col": 2, "target": "action" }
            ]
        });
        translate_import_mapping(&mut mapping, &defs);
        translate_import_mapping(&mut mapping, &defs);
        assert_eq!(mapping["type"], item.id);
        assert_eq!(mapping["columns"][0]["target"], format!("type:{}", sku.id));
        assert!(
            mapping["columns"][1]["target"]
                .as_str()
                .unwrap()
                .ends_with("@en-GB")
        );
        assert_eq!(mapping["columns"][2]["target"], "action");
    }

    #[test]
    fn the_warning_names_the_old_key() {
        assert_eq!(
            key_rename_warning("brand"),
            "External integrations that use the old key « brand » will stop working"
        );
    }
}
