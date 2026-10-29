//! Batch-read attribute and hop path values for the object table.
//!
//! Search hits already carry local indexed fields (including parts such as
//! `weight.unit`). Linked values are not in Typesense; the grid loads them
//! here in one round trip per page instead of one request per cell.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use braian_model::{
    AttributeId, AttributeKind, AttributePath, DefinitionSet, ObjectId, PathPart, PathSide,
    parse_path_text, validate_path,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::{Auth, Role};
use crate::platform::require;
use crate::problem::{Problem, store_status};
use crate::state::AppState;
use crate::wire;

/// Cap on object ids, aligned with [`crate::objects::BATCH_GET_LIMIT`].
pub const PATH_VALUES_ID_LIMIT: usize = 200;
/// Cap on distinct paths in one request.
pub const PATH_VALUES_PATH_LIMIT: usize = 64;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum PathInput {
    Text(String),
    Spec(PathSpec),
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PathSpec {
    pub path: String,
    /// Optional client column id used as the response map key (cardinality/unit/lang).
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub cardinality: Option<String>,
    #[serde(default)]
    pub peer_id: Option<Uuid>,
    /// Target unit code for measure display conversion.
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PathValuesRequest {
    /// Object ids to read. Missing or deleted ids are omitted from the response.
    pub ids: Vec<Uuid>,
    /// Path texts or rich specs (`path` + cardinality / peer / unit).
    pub paths: Vec<PathInput>,
    /// Type key or id used to validate paths (required).
    #[serde(rename = "type")]
    pub type_key: String,
    /// Language for value-list labels. Defaults to `en`.
    #[serde(default)]
    pub lang: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PathValueEntry {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    /// True when a requested unit conversion could not be applied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raw: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PathValuesResponse {
    /// `object_id → path key (column id or path text) → display values`.
    pub values: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Richer entries with peer ids and conversion markers.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub entries: BTreeMap<String, BTreeMap<String, Vec<PathValueEntry>>>,
}

#[utoipa::path(
    post,
    path = "/v1/objects:pathValues",
    tag = "objects",
    request_body(content = PathValuesRequest),
    responses(
        (status = 200, description = "Path values keyed by object id and path text", body = PathValuesResponse),
        (status = 400, description = "Too many ids or paths, or invalid path", body = crate::problem::Problem),
    ),
    security(("bearer" = []))
)]
pub async fn path_values(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<PathValuesRequest>,
) -> Result<Json<PathValuesResponse>, Problem> {
    require(&identity, Role::Viewer)?;
    if body.ids.len() > PATH_VALUES_ID_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "batch_too_large",
            format!("path values accepts at most {PATH_VALUES_ID_LIMIT} ids"),
        ));
    }
    if body.paths.len() > PATH_VALUES_PATH_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "batch_too_large",
            format!("path values accepts at most {PATH_VALUES_PATH_LIMIT} paths"),
        ));
    }
    let type_token = body.type_key.trim();
    if type_token.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "type",
            "type is required so paths can be validated",
        ));
    }
    let lang = body
        .lang
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("en");
    let cached = state.models.get(&state.store).await?;
    let type_id = wire::resolve_type(&cached.model, type_token)?;
    let mut resolved = Vec::with_capacity(body.paths.len());
    for input in &body.paths {
        let (text, response_key, cardinality, peer_id, unit) = match input {
            PathInput::Text(text) => (text.as_str(), None, None, None, None),
            PathInput::Spec(spec) => (
                spec.path.as_str(),
                spec.key.as_deref(),
                spec.cardinality.as_deref(),
                spec.peer_id,
                spec.unit.as_deref(),
            ),
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let path = parse_path_text(text, &cached.defs, Some(type_id))
            .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, "path", message))?;
        validate_path(&path, &cached.defs, type_id)
            .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, "path", message))?;
        let attribute = cached
            .defs
            .attributes
            .iter()
            .find(|item| item.id == path.attribute);
        let language_dependent = attribute.is_some_and(|item| item.language_dependent);
        let unit_family = attribute.and_then(|item| item.kind.unit_family_id());
        let labels = option_labels(&cached.defs, path.attribute, lang);
        resolved.push(ResolvedPath {
            text: text.to_owned(),
            key: response_key.map(str::to_owned),
            path,
            labels,
            language_dependent,
            cardinality: cardinality.map(str::to_owned),
            peer_id,
            unit: unit.map(str::to_owned),
            unit_family,
        });
    }
    if body.ids.is_empty() || resolved.is_empty() {
        return Ok(Json(PathValuesResponse {
            values: BTreeMap::new(),
            entries: BTreeMap::new(),
        }));
    }

    let needs_hops = resolved.iter().any(|item| item.path.via.is_some());
    let needs_local = resolved.iter().any(|item| item.path.via.is_none());

    let hops = if needs_hops {
        state
            .store
            .hop_entries_for(&body.ids)
            .await
            .map_err(store_status)?
    } else {
        BTreeMap::new()
    };

    let locals = if needs_local {
        let object_ids: Vec<ObjectId> = body.ids.iter().copied().map(ObjectId::from_uuid).collect();
        let loaded = state
            .store
            .get_objects(&object_ids)
            .await
            .map_err(store_status)?;
        loaded
            .into_iter()
            .filter(|item| item.deleted_at.is_none())
            .map(|item| (item.object.id.as_uuid(), item.object.values.clone()))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    let mut values: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut entries: BTreeMap<String, BTreeMap<String, Vec<PathValueEntry>>> = BTreeMap::new();
    for id in &body.ids {
        let mut cells = BTreeMap::new();
        let mut rich = BTreeMap::new();
        for item in &resolved {
            let response_key = response_path_key(item);
            let mut list = if let Some(via) = &item.path.via {
                let side = match via.side {
                    PathSide::To => "out",
                    PathSide::From => "in",
                    PathSide::Link => "link",
                };
                let slot = format!(
                    "{}:{side}:{}",
                    via.link_type.as_uuid(),
                    item.path.attribute.as_uuid()
                );
                hops.get(id)
                    .and_then(|map| map.get(&slot))
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| {
                        let (text, raw) = display_measure(
                            &entry.value,
                            item.path.part,
                            &item.labels,
                            lang,
                            item.language_dependent,
                            item.unit.as_deref(),
                            item.unit_family,
                            &cached.defs,
                        );
                        PathValueEntry {
                            text,
                            peer_id: Some(entry.peer_id.to_string()),
                            raw,
                        }
                    })
                    .filter(|entry| !entry.text.is_empty() || entry.raw)
                    .collect::<Vec<_>>()
            } else {
                locals
                    .get(id)
                    .and_then(|map| map.get(&item.path.attribute))
                    .map(|value| {
                        let (text, raw) = display_measure(
                            value,
                            item.path.part,
                            &item.labels,
                            lang,
                            item.language_dependent,
                            item.unit.as_deref(),
                            item.unit_family,
                            &cached.defs,
                        );
                        if text.is_empty() && !raw {
                            Vec::new()
                        } else {
                            vec![PathValueEntry {
                                text,
                                peer_id: None,
                                raw,
                            }]
                        }
                    })
                    .unwrap_or_default()
            };

            // Cardinality filters for hop results.
            if item.path.via.is_some() {
                match item.cardinality.as_deref() {
                    Some("first") => {
                        list.truncate(1);
                    }
                    Some("specific") => {
                        if let Some(peer) = item.peer_id {
                            list.retain(|entry| {
                                entry
                                    .peer_id
                                    .as_deref()
                                    .and_then(|text| Uuid::parse_str(text).ok())
                                    == Some(peer)
                            });
                        } else {
                            list.clear();
                        }
                    }
                    // "all" or none — keep full list
                    _ => {}
                }
            }

            let texts: Vec<String> = list.iter().map(|entry| entry.text.clone()).collect();
            cells.insert(response_key.clone(), texts);
            rich.insert(response_key, list);
        }
        if !cells.is_empty() {
            values.insert(id.to_string(), cells);
            entries.insert(id.to_string(), rich);
        }
    }
    Ok(Json(PathValuesResponse { values, entries }))
}

fn response_path_key(item: &ResolvedPath) -> String {
    // Prefer the client column id so all/first/specific/unit do not collide on path text.
    item.key.clone().filter(|key| !key.is_empty()).unwrap_or_else(|| item.text.clone())
}

struct ResolvedPath {
    text: String,
    key: Option<String>,
    path: AttributePath,
    labels: BTreeMap<String, String>,
    language_dependent: bool,
    cardinality: Option<String>,
    peer_id: Option<Uuid>,
    unit: Option<String>,
    unit_family: Option<braian_model::UnitFamilyId>,
}

fn option_labels(
    defs: &DefinitionSet,
    attribute: AttributeId,
    lang: &str,
) -> BTreeMap<String, String> {
    let Some(attribute) = defs.attributes.iter().find(|item| item.id == attribute) else {
        return BTreeMap::new();
    };
    let options = match &attribute.kind {
        AttributeKind::ValueList { options, list } => list
            .as_ref()
            .and_then(|key| defs.value_lists.iter().find(|item| &item.key == key))
            .map(|item| item.options.as_slice())
            .unwrap_or(options.as_slice()),
        _ => return BTreeMap::new(),
    };
    options
        .iter()
        .filter_map(|option| {
            option
                .labels
                .iter()
                .find(|(tag, _)| tag.as_str() == lang)
                .map(|(_, label)| (option.code.clone(), label.clone()))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn display_measure(
    value: &Value,
    part: PathPart,
    labels: &BTreeMap<String, String>,
    lang: &str,
    language_dependent: bool,
    target_unit: Option<&str>,
    unit_family: Option<braian_model::UnitFamilyId>,
    defs: &DefinitionSet,
) -> (String, bool) {
    let value = if language_dependent {
        value
            .as_object()
            .and_then(|object| object.get(lang).or_else(|| object.values().next()))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        value.clone()
    };

    if let Some(target) = target_unit.filter(|_| matches!(part, PathPart::Whole | PathPart::Value))
        && let Some((number, from_unit)) = read_measure(&value)
    {
        let converted = unit_family
            .and_then(|family_id| convert_unit(defs, family_id, number, &from_unit, target));
        if let Some(converted) = converted {
            let display = if converted.fract() == 0.0 {
                format!("{converted:.0}")
            } else {
                format!("{converted}")
            };
            return (format!("{display} {target}"), false);
        }
        // Missing factor — raw value with own unit; client shows a discrete marker via raw=true.
        let raw_text = if from_unit.is_empty() {
            number.to_string()
        } else {
            format!("{number} {from_unit}")
        };
        return (raw_text, true);
    }

    let projected = project_part(&value, part, labels);
    let text = match projected {
        Value::Null => String::new(),
        Value::String(text) => text,
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| display_value(item, PathPart::Whole, labels, lang, false))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => other
            .get("value")
            .map(|inner| {
                let unit = other.get("unit").and_then(Value::as_str).unwrap_or("");
                let base = display_value(inner, PathPart::Whole, labels, lang, false);
                if unit.is_empty() || base.is_empty() {
                    base
                } else {
                    format!("{base} {unit}")
                }
            })
            .filter(|text| !text.is_empty())
            .or_else(|| other.as_str().map(str::to_owned))
            .unwrap_or_default(),
    };
    (text, false)
}

fn read_measure(value: &Value) -> Option<(f64, String)> {
    let object = value.as_object()?;
    let raw = object.get("value")?;
    let number = match raw {
        Value::Number(n) => n.as_f64()?,
        Value::String(text) => text.parse::<f64>().ok()?,
        _ => return None,
    };
    let unit = object
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Some((number, unit))
}

fn convert_unit(
    defs: &DefinitionSet,
    family_id: braian_model::UnitFamilyId,
    value: f64,
    from_unit: &str,
    to_unit: &str,
) -> Option<f64> {
    let family = defs
        .unit_families
        .iter()
        .find(|item| item.id == family_id)?;
    let from = family
        .units
        .iter()
        .find(|unit| unit.code.eq_ignore_ascii_case(from_unit))?;
    let to = family
        .units
        .iter()
        .find(|unit| unit.code.eq_ignore_ascii_case(to_unit))?;
    let from_f = from.factor.to_string().parse::<f64>().ok()?;
    let to_f = to.factor.to_string().parse::<f64>().ok()?;
    if to_f == 0.0 {
        return None;
    }
    // value × from.factor = base; base / to.factor = display
    Some((value * from_f) / to_f)
}

fn display_value(
    value: &Value,
    part: PathPart,
    labels: &BTreeMap<String, String>,
    lang: &str,
    language_dependent: bool,
) -> String {
    let value = if language_dependent {
        value
            .as_object()
            .and_then(|object| object.get(lang).or_else(|| object.values().next()))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        value.clone()
    };
    let projected = project_part(&value, part, labels);
    match projected {
        Value::Null => String::new(),
        Value::String(text) => text,
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| display_value(item, PathPart::Whole, labels, lang, false))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => other
            .get("value")
            .map(|inner| display_value(inner, PathPart::Whole, labels, lang, false))
            .filter(|text| !text.is_empty())
            .or_else(|| other.as_str().map(str::to_owned))
            .unwrap_or_default(),
    }
}

fn project_part(value: &Value, part: PathPart, labels: &BTreeMap<String, String>) -> Value {
    match part {
        PathPart::Whole | PathPart::Code => value.clone(),
        PathPart::Value => value.get("value").cloned().unwrap_or_else(|| value.clone()),
        PathPart::Unit => value.get("unit").cloned().unwrap_or(Value::Null),
        PathPart::Min => value.get("min").cloned().unwrap_or(Value::Null),
        PathPart::Max => value.get("max").cloned().unwrap_or(Value::Null),
        PathPart::Label => {
            let code = value.as_str().unwrap_or("");
            Value::String(labels.get(code).cloned().unwrap_or_else(|| code.to_owned()))
        }
    }
}
