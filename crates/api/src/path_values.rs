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
pub struct PathValuesRequest {
    /// Object ids to read. Missing or deleted ids are omitted from the response.
    pub ids: Vec<Uuid>,
    /// Path texts such as `product>brand` or `weight.unit`.
    pub paths: Vec<String>,
    /// Optional type key or id used to validate paths.
    #[serde(default, rename = "type")]
    pub type_key: Option<String>,
    /// Language for value-list labels. Defaults to `en`.
    #[serde(default)]
    pub lang: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PathValuesResponse {
    /// `object_id → path_text → display values` (one entry per linked peer when a hop has many).
    pub values: BTreeMap<String, BTreeMap<String, Vec<String>>>,
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
    let lang = body
        .lang
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("en");
    let cached = state.models.get(&state.store).await?;
    let type_id = match body.type_key.as_deref() {
        Some(token) if !token.is_empty() => Some(wire::resolve_type(&cached.model, token)?),
        _ => None,
    };
    let mut resolved = Vec::with_capacity(body.paths.len());
    for text in &body.paths {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let path = parse_path_text(text, &cached.defs, type_id)
            .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, "path", message))?;
        if let Some(type_id) = type_id {
            validate_path(&path, &cached.defs, type_id)
                .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, "path", message))?;
        }
        let attribute = cached
            .defs
            .attributes
            .iter()
            .find(|item| item.id == path.attribute);
        let language_dependent = attribute.is_some_and(|item| item.language_dependent);
        let labels = option_labels(&cached.defs, path.attribute, lang);
        resolved.push(ResolvedPath {
            text: text.to_owned(),
            path,
            labels,
            language_dependent,
        });
    }
    if body.ids.is_empty() || resolved.is_empty() {
        return Ok(Json(PathValuesResponse {
            values: BTreeMap::new(),
        }));
    }

    let needs_hops = resolved.iter().any(|item| item.path.via.is_some());
    let needs_local = resolved.iter().any(|item| item.path.via.is_none());

    let hops = if needs_hops {
        state
            .store
            .hop_values_for(&body.ids)
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
    for id in &body.ids {
        let mut cells = BTreeMap::new();
        for item in &resolved {
            let texts = if let Some(via) = &item.path.via {
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
                    .map(|list| {
                        list.iter()
                            .map(|value| {
                                display_value(
                                    value,
                                    item.path.part,
                                    &item.labels,
                                    lang,
                                    item.language_dependent,
                                )
                            })
                            .filter(|text| !text.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                locals
                    .get(id)
                    .and_then(|map| map.get(&item.path.attribute))
                    .map(|value| {
                        let text = display_value(
                            value,
                            item.path.part,
                            &item.labels,
                            lang,
                            item.language_dependent,
                        );
                        if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![text]
                        }
                    })
                    .unwrap_or_default()
            };
            cells.insert(item.text.clone(), texts);
        }
        if !cells.is_empty() {
            values.insert(id.to_string(), cells);
        }
    }
    Ok(Json(PathValuesResponse { values }))
}

struct ResolvedPath {
    text: String,
    path: AttributePath,
    labels: BTreeMap<String, String>,
    language_dependent: bool,
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
