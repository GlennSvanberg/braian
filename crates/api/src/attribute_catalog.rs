//! Read-only catalog of attribute definitions and where their values are stored.
//!
//! The same rows drive the Attribute definitions view and the agent tools
//! `model.list_attribute_definitions` and `model.attribute_usage`.

use std::collections::{BTreeMap, BTreeSet};

use axum::http::StatusCode;
use braian_model::{
    Attribute, AttributeId, AttributeKind, DefinitionSet, LanguageTag, is_locked_standard,
};
use braian_store::AttributeUsageCount;
use serde_json::{Value, json};

use crate::problem::{Problem, store_status};
use crate::state::AppState;
use crate::wire;

pub async fn list(state: &AppState) -> Result<Value, Problem> {
    let cached = state.models.get(&state.store).await?;
    let counts = state
        .store
        .attribute_usage_counts()
        .await
        .map_err(store_status)?;
    Ok(json!({
        "attributes": catalog(&cached.defs, &counts),
    }))
}

pub async fn usage(state: &AppState, args: &Value) -> Result<Value, Problem> {
    let token = args
        .get("attribute")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "missing_attribute",
                "name the attribute",
            )
        })?;
    let cached = state.models.get(&state.store).await?;
    let attribute_id = wire::resolve_attribute(&cached.model, token)?;
    let objects = state
        .store
        .count_attribute_usage(attribute_id.as_uuid())
        .await
        .map_err(store_status)?;
    let counts = state
        .store
        .attribute_usage_counts()
        .await
        .map_err(store_status)?;
    let attribute = cached
        .defs
        .attributes
        .iter()
        .find(|item| item.id == attribute_id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("attribute {token}"),
            )
        })?;
    let mut body = attribute_json(&cached.defs, attribute, &counts);
    body["used_in"]["objects"] = json!(objects);
    Ok(body)
}

fn catalog(defs: &DefinitionSet, counts: &[AttributeUsageCount]) -> Vec<Value> {
    let mut rows: Vec<Value> = defs
        .attributes
        .iter()
        .map(|attribute| attribute_json(defs, attribute, counts))
        .collect();
    rows.sort_by(|left, right| {
        label_of(left)
            .cmp(&label_of(right))
            .then_with(|| key_of(left).cmp(&key_of(right)))
    });
    rows
}

fn attribute_json(
    defs: &DefinitionSet,
    attribute: &Attribute,
    counts: &[AttributeUsageCount],
) -> Value {
    let (kind, unit_family, value_list) = kind_fields(defs, &attribute.kind);
    let standard = is_locked_standard(defs, attribute.id);
    json!({
        "id": attribute.id.as_uuid(),
        "key": attribute.key,
        "label": english_label(&attribute.labels, &attribute.key),
        "kind": kind,
        "unit_family": unit_family,
        "value_list": value_list,
        "standard": standard,
        "flags": {
            "required": attribute.required,
            "multiple": attribute.multiple,
            "languages": attribute.language_dependent,
            "searchable": attribute.index.searchable,
            "filterable": attribute.index.filterable,
            "sortable": attribute.index.sortable,
            "standard": standard,
        },
        "used_in": used_in(defs, attribute.id, counts),
    })
}

fn used_in(defs: &DefinitionSet, attribute: AttributeId, counts: &[AttributeUsageCount]) -> Value {
    let id = attribute.as_uuid();
    let objects = counts
        .iter()
        .find(|row| {
            row.attribute_id == id
                && row.type_id.is_none()
                && row.structure_id.is_none()
                && row.group_id.is_none()
        })
        .map(|row| row.objects)
        .unwrap_or(0);
    let mut types = Vec::new();
    let mut seen_types = BTreeSet::new();
    for object_type in &defs.types {
        if !object_type.attributes.contains(&attribute) {
            continue;
        }
        let type_id = object_type.id.as_uuid();
        seen_types.insert(type_id);
        types.push(json!({
            "id": type_id,
            "key": object_type.key,
            "label": object_type.singular_label("en"),
            "objects": count_for_type(counts, id, type_id),
        }));
    }
    for row in counts.iter().filter(|row| row.attribute_id == id) {
        let Some(type_id) = row.type_id else {
            continue;
        };
        if !seen_types.insert(type_id) {
            continue;
        }
        let object_type = defs.types.iter().find(|item| item.id.as_uuid() == type_id);
        types.push(json!({
            "id": type_id,
            "key": object_type.map(|item| item.key.clone()).unwrap_or_default(),
            "label": object_type
                .map(|item| item.singular_label("en"))
                .unwrap_or_else(|| type_id.to_string()),
            "objects": row.objects,
        }));
    }
    types.sort_by(|left, right| {
        label_of(left)
            .cmp(&label_of(right))
            .then_with(|| key_of(left).cmp(&key_of(right)))
    });
    let mut groups = Vec::new();
    let mut seen_groups = BTreeSet::new();
    for structure in &defs.structures {
        for group in &structure.groups {
            let on_class = structure
                .classes
                .iter()
                .any(|class| class.group == group.id && class.attributes.contains(&attribute));
            if !group.attributes.contains(&attribute) && !on_class {
                continue;
            }
            let structure_id = structure.id.as_uuid();
            let group_id = group.id.as_uuid();
            seen_groups.insert((structure_id, group_id));
            groups.push(group_place(
                structure_id,
                &structure.key,
                &english_label(&structure.labels, &structure.key),
                group_id,
                &group.key,
                &english_label(&group.labels, &group.key),
                count_for_group(counts, id, structure_id, group_id),
            ));
        }
    }
    for row in counts.iter().filter(|row| row.attribute_id == id) {
        let (Some(structure_id), Some(group_id)) = (row.structure_id, row.group_id) else {
            continue;
        };
        if !seen_groups.insert((structure_id, group_id)) {
            continue;
        }
        let structure = defs
            .structures
            .iter()
            .find(|item| item.id.as_uuid() == structure_id);
        let group = structure.and_then(|structure| {
            structure
                .groups
                .iter()
                .find(|item| item.id.as_uuid() == group_id)
        });
        groups.push(group_place(
            structure_id,
            structure.map(|item| item.key.as_str()).unwrap_or(""),
            &structure
                .map(|item| english_label(&item.labels, &item.key))
                .unwrap_or_else(|| structure_id.to_string()),
            group_id,
            group.map(|item| item.key.as_str()).unwrap_or(""),
            &group
                .map(|item| english_label(&item.labels, &item.key))
                .unwrap_or_else(|| group_id.to_string()),
            row.objects,
        ));
    }
    groups.sort_by(|left, right| {
        label_of(left).cmp(&label_of(right)).then_with(|| {
            text_field(left, "structure_label").cmp(&text_field(right, "structure_label"))
        })
    });
    json!({
        "objects": objects,
        "types": types,
        "groups": groups,
    })
}

fn count_for_type(
    counts: &[AttributeUsageCount],
    attribute: uuid::Uuid,
    type_id: uuid::Uuid,
) -> i64 {
    counts
        .iter()
        .find(|row| {
            row.attribute_id == attribute && row.type_id == Some(type_id) && row.group_id.is_none()
        })
        .map(|row| row.objects)
        .unwrap_or(0)
}

fn count_for_group(
    counts: &[AttributeUsageCount],
    attribute: uuid::Uuid,
    structure_id: uuid::Uuid,
    group_id: uuid::Uuid,
) -> i64 {
    counts
        .iter()
        .find(|row| {
            row.attribute_id == attribute
                && row.structure_id == Some(structure_id)
                && row.group_id == Some(group_id)
        })
        .map(|row| row.objects)
        .unwrap_or(0)
}

fn group_place(
    structure_id: uuid::Uuid,
    structure_key: &str,
    structure_label: &str,
    group_id: uuid::Uuid,
    key: &str,
    label: &str,
    objects: i64,
) -> Value {
    json!({
        "id": group_id,
        "key": key,
        "label": label,
        "structure_id": structure_id,
        "structure_key": structure_key,
        "structure_label": structure_label,
        "objects": objects,
    })
}

fn kind_fields(defs: &DefinitionSet, kind: &AttributeKind) -> (String, Value, Value) {
    match kind {
        AttributeKind::Text { .. } => ("text".into(), Value::Null, Value::Null),
        AttributeKind::Number { unit_family, .. } => (
            "number".into(),
            family_json(defs, *unit_family),
            Value::Null,
        ),
        AttributeKind::Range { unit_family } => {
            ("range".into(), family_json(defs, *unit_family), Value::Null)
        }
        AttributeKind::ValueList { list, .. } => (
            "value_list".into(),
            Value::Null,
            list_json(defs, list.as_deref()),
        ),
        AttributeKind::Boolean => ("boolean".into(), Value::Null, Value::Null),
        AttributeKind::Date => ("date".into(), Value::Null, Value::Null),
        AttributeKind::ObjectRef { .. } => ("object_ref".into(), Value::Null, Value::Null),
        AttributeKind::FileRef { .. } => ("file_ref".into(), Value::Null, Value::Null),
    }
}

fn family_json(defs: &DefinitionSet, id: Option<braian_model::UnitFamilyId>) -> Value {
    let Some(id) = id else {
        return Value::Null;
    };
    let Some(family) = defs.unit_families.iter().find(|item| item.id == id) else {
        return json!({ "id": id.as_uuid(), "key": "", "label": id.as_uuid().to_string() });
    };
    json!({
        "id": family.id.as_uuid(),
        "key": family.key,
        "label": family.key,
    })
}

fn list_json(defs: &DefinitionSet, key: Option<&str>) -> Value {
    let Some(key) = key else {
        return Value::Null;
    };
    let Some(list) = defs.value_lists.iter().find(|item| item.key == key) else {
        return json!({ "key": key, "label": key });
    };
    json!({
        "key": list.key,
        "label": english_label(&list.labels, &list.key),
    })
}

fn english_label(labels: &BTreeMap<LanguageTag, String>, key: &str) -> String {
    labels
        .iter()
        .find(|(tag, _)| tag.as_str() == "en")
        .map(|(_, label)| label.clone())
        .or_else(|| labels.values().next().cloned())
        .unwrap_or_else(|| key.to_owned())
}

fn label_of(value: &Value) -> String {
    text_field(value, "label")
}

fn key_of(value: &Value) -> String {
    text_field(value, "key")
}

fn text_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}
