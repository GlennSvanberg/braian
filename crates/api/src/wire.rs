use std::collections::BTreeMap;

use braian_model::{
    AttributeId, DefinitionSet, GroupId, ObjectId, ObjectPatch, StructureId, TypeId, ValuePatch,
};
use braian_rules::CompiledModel;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::problem::Problem;

pub fn resolve_attribute(model: &CompiledModel, token: &str) -> Result<AttributeId, Problem> {
    if let Ok(id) = Uuid::parse_str(token) {
        let attribute = AttributeId::from_uuid(id);
        if model.attribute(attribute).is_some() {
            return Ok(attribute);
        }
        return Err(unknown("attribute", token));
    }
    model
        .attribute_by_key(token)
        .map(|attribute| attribute.id)
        .ok_or_else(|| unknown("attribute", token))
}

pub fn resolve_type(model: &CompiledModel, token: &str) -> Result<TypeId, Problem> {
    if let Ok(id) = Uuid::parse_str(token) {
        let type_id = TypeId::from_uuid(id);
        if model.key_attribute(type_id).is_some() {
            return Ok(type_id);
        }
        return Err(unknown("type", token));
    }
    model.type_id(token).ok_or_else(|| unknown("type", token))
}

pub fn resolve_structure(
    model: &CompiledModel,
    defs: &DefinitionSet,
    token: &str,
) -> Result<StructureId, Problem> {
    if let Ok(id) = Uuid::parse_str(token) {
        let structure = StructureId::from_uuid(id);
        if defs.structures.iter().any(|item| item.id == structure) {
            return Ok(structure);
        }
        return Err(unknown("structure", token));
    }
    model
        .structure_id(token)
        .ok_or_else(|| unknown("structure", token))
}

pub fn resolve_group(
    defs: &DefinitionSet,
    structure: StructureId,
    token: &str,
) -> Result<GroupId, Problem> {
    let Some(structure) = defs.structures.iter().find(|item| item.id == structure) else {
        return Err(unknown("structure", &structure.to_string()));
    };
    if let Ok(id) = Uuid::parse_str(token) {
        let group = GroupId::from_uuid(id);
        if structure.groups.iter().any(|item| item.id == group) {
            return Ok(group);
        }
        return Err(unknown("group", token));
    }
    structure
        .groups
        .iter()
        .find(|group| group.key == token)
        .map(|group| group.id)
        .ok_or_else(|| unknown("group", token))
}

pub fn object_id(given: Option<Uuid>) -> Result<ObjectId, Problem> {
    match given {
        None => Ok(ObjectId::from_uuid(Uuid::now_v7())),
        Some(id) if id.get_version_num() == 7 => Ok(ObjectId::from_uuid(id)),
        Some(id) => Err(Problem::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_id",
            format!("object id {id} must be a UUIDv7"),
        )),
    }
}

/// Full values for a create. `null` omits the attribute. A null language is omitted.
pub fn create_values(
    model: &CompiledModel,
    values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<AttributeId, Value>, Problem> {
    let mut stored = BTreeMap::new();
    for (token, value) in values {
        if value.is_null() {
            continue;
        }
        let id = resolve_attribute(model, token)?;
        let attribute = model
            .attribute(id)
            .ok_or_else(|| unknown("attribute", token))?;
        let value = if attribute.language_dependent {
            strip_null_languages(value)
        } else {
            value.clone()
        };
        stored.insert(id, value);
    }
    Ok(stored)
}

pub fn patch_from_wire(
    model: &CompiledModel,
    defs: &DefinitionSet,
    values: &BTreeMap<String, Value>,
    groups: &BTreeMap<String, Option<String>>,
) -> Result<ObjectPatch, Problem> {
    let mut attributes = BTreeMap::new();
    for (token, value) in values {
        let id = resolve_attribute(model, token)?;
        let attribute = model
            .attribute(id)
            .ok_or_else(|| unknown("attribute", token))?;
        let patch = if value.is_null() {
            braian_model::AttributePatch::Remove
        } else if attribute.language_dependent {
            let Some(map) = value.as_object() else {
                attributes.insert(
                    id,
                    braian_model::AttributePatch::Replace {
                        value: value.clone(),
                    },
                );
                continue;
            };
            let mut languages = BTreeMap::new();
            for (language, inner) in map {
                let tag = braian_model::LanguageTag::parse(language).map_err(|_| {
                    Problem::new(
                        axum::http::StatusCode::BAD_REQUEST,
                        "invalid_language",
                        format!("invalid language tag {language}"),
                    )
                })?;
                if inner.is_null() {
                    languages.insert(tag, None);
                } else {
                    languages.insert(tag, Some(inner.clone()));
                }
            }
            braian_model::AttributePatch::Languages { values: languages }
        } else {
            braian_model::AttributePatch::Replace {
                value: value.clone(),
            }
        };
        attributes.insert(id, patch);
    }
    let mut placed = BTreeMap::new();
    for (structure, group) in groups {
        let structure = resolve_structure(model, defs, structure)?;
        let group = match group {
            Some(group) => Some(resolve_group(defs, structure, group)?),
            None => None,
        };
        placed.insert(structure, group);
    }
    Ok(ObjectPatch {
        values: ValuePatch { attributes },
        groups: placed,
    })
}

pub fn values_to_wire(
    model: &CompiledModel,
    applicable: &[AttributeId],
    values: &BTreeMap<AttributeId, Value>,
    include_inapplicable: bool,
) -> BTreeMap<String, Value> {
    let mut wire = BTreeMap::new();
    for (id, value) in values {
        let applies = applicable.contains(id);
        if !applies && !include_inapplicable {
            continue;
        }
        let key = model
            .attribute(*id)
            .map(|attribute| attribute.key.clone())
            .unwrap_or_else(|| id.as_uuid().to_string());
        wire.insert(key, value.clone());
    }
    wire
}

pub fn groups_to_wire(
    defs: &DefinitionSet,
    groups: &BTreeMap<StructureId, GroupId>,
) -> BTreeMap<String, String> {
    let mut wire = BTreeMap::new();
    for (structure_id, group_id) in groups {
        let structure = defs.structures.iter().find(|item| item.id == *structure_id);
        let structure_key = structure
            .map(|item| item.key.clone())
            .unwrap_or_else(|| structure_id.as_uuid().to_string());
        let group_key = structure
            .and_then(|item| item.groups.iter().find(|group| group.id == *group_id))
            .map(|group| group.key.clone())
            .unwrap_or_else(|| group_id.as_uuid().to_string());
        wire.insert(structure_key, group_key);
    }
    wire
}

pub fn type_key(defs: &DefinitionSet, id: TypeId) -> String {
    defs.types
        .iter()
        .find(|object_type| object_type.id == id)
        .map(|object_type| object_type.key.clone())
        .unwrap_or_else(|| id.as_uuid().to_string())
}

fn strip_null_languages(value: &Value) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };
    let mut kept = Map::new();
    for (language, inner) in map {
        if !inner.is_null() {
            kept.insert(language.clone(), inner.clone());
        }
    }
    Value::Object(kept)
}

fn unknown(kind: &str, token: &str) -> Problem {
    Problem::new(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "unknown_reference",
        format!("unknown {kind} {token}"),
    )
}
