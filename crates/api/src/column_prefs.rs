//! Per-user column preferences for one view (`view_column_prefs`).
//!
//! # Stale paths on GET
//! A saved path can become unknown after a model change (attribute or link
//! removed, key renamed). GET never returns 500 for that. Each column whose
//! `path` / `path_id` no longer parses or is no longer listed for the view's
//! bound object type is kept in `document.columns` with `"stale": true`, and
//! its text is also listed in top-level `stale_paths`. PUT still rejects
//! unknown paths with 400 so callers cannot save new stale data.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use braian_model::{DefinitionSet, TypeId, list_attribute_paths, parse_path};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::auth::{Auth, Identity, Role};
use crate::platform::require;
use crate::problem::{Problem, store_status};
use crate::state::AppState;
use crate::wire;

const DOCUMENT_LIMIT: usize = 64 * 1024;
const MAX_COLUMNS: usize = 200;

#[derive(Debug, Deserialize)]
pub struct PrefsQuery {
    pub environment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PrefsBody {
    pub environment: Option<String>,
    pub document: Value,
}

fn environment(raw: Option<&str>) -> String {
    let raw = raw.filter(|value| !value.is_empty()).unwrap_or("default");
    let cleaned: String = raw
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "default".to_owned()
    } else {
        cleaned
    }
}

/// Owner of the prefs row: signed-in user, or the user attached to this principal.
async fn prefs_owner(state: &AppState, identity: &Identity) -> Result<Uuid, Problem> {
    if let Some(id) = identity.user_id {
        return Ok(id);
    }
    state
        .store
        .user_id_for_principal(identity.actor.actor.as_uuid())
        .await
        .map_err(store_status)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "no_user",
                "sign in to keep column preferences",
            )
        })
}

async fn require_view(
    state: &AppState,
    view_id: Uuid,
) -> Result<(String, Value), Problem> {
    state
        .store
        .view_row(view_id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "view"))
}

fn bound_type_token(state: &Value) -> Option<&str> {
    state
        .get("type")
        .or_else(|| state.get("type_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn column_path_input(column: &Value) -> Result<Value, Problem> {
    if let Some(path_id) = column.get("path_id") {
        if path_id.is_object() {
            return Ok(path_id.clone());
        }
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "path_id",
            "path_id must be an attribute path object",
        ));
    }
    if let Some(path) = column.get("path").and_then(Value::as_str) {
        return Ok(Value::String(path.to_owned()));
    }
    Err(Problem::new(
        StatusCode::BAD_REQUEST,
        "path",
        "each column needs a path or path_id",
    ))
}

fn path_known(defs: &DefinitionSet, type_id: TypeId, input: &Value) -> Result<String, String> {
    let path = parse_path(input, defs, Some(type_id))?;
    let listed = list_attribute_paths(defs, type_id);
    let Some(row) = listed.iter().find(|item| item.path == path) else {
        return Err("unknown attribute path for this type".into());
    };
    Ok(row.text.clone())
}

/// Validate a PUT document. Returns the normalized document to store.
fn validate_document(
    document: &Value,
    defs: &DefinitionSet,
    type_id: TypeId,
) -> Result<Value, Problem> {
    let size = serde_json::to_vec(document)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > DOCUMENT_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "document",
            "the column preferences document is too large",
        ));
    }
    let object = document.as_object().ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "document",
            "the column preferences document must be an object",
        )
    })?;
    let version = object.get("version").and_then(Value::as_u64).ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "version",
            "document.version must be 1",
        )
    })?;
    if version != 1 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "version",
            "document.version must be 1",
        ));
    }
    let columns = object
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "columns",
                "document.columns must be an array",
            )
        })?;
    if columns.len() > MAX_COLUMNS {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "columns",
            format!("at most {MAX_COLUMNS} columns"),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut normalized = Vec::with_capacity(columns.len());
    for column in columns {
        let input = column_path_input(column)?;
        let text = path_known(defs, type_id, &input).map_err(|message| {
            // parse_path rejects more than one via-step with this wording.
            let code = if message.contains("one link step") {
                "via"
            } else if message.contains("unknown") {
                "path"
            } else {
                "path"
            };
            Problem::new(StatusCode::BAD_REQUEST, code, message)
        })?;
        if !seen.insert(text.clone()) {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "duplicate",
                format!("duplicate path {text}"),
            ));
        }
        let visible = column
            .get("visible")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut entry = Map::new();
        entry.insert("path".into(), Value::String(text));
        entry.insert("visible".into(), Value::Bool(visible));
        if let Some(path_id) = column.get("path_id").filter(|value| value.is_object()) {
            entry.insert("path_id".into(), path_id.clone());
        }
        normalized.push(Value::Object(entry));
    }
    Ok(json!({
        "version": 1,
        "columns": normalized,
    }))
}

/// Annotate unknown paths for GET. Never errors on stale data.
fn annotate_stale(document: &Value, defs: &DefinitionSet, type_id: Option<TypeId>) -> (Value, Vec<String>) {
    let Some(columns) = document.get("columns").and_then(Value::as_array) else {
        return (document.clone(), Vec::new());
    };
    let mut stale_paths = Vec::new();
    let mut next_columns = Vec::with_capacity(columns.len());
    for column in columns {
        let mut entry = column.as_object().cloned().unwrap_or_default();
        let input = column
            .get("path_id")
            .filter(|value| value.is_object())
            .cloned()
            .or_else(|| {
                column
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|path| Value::String(path.to_owned()))
            });
        let mut stale = false;
        let label = column
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        match (input, type_id) {
            (Some(input), Some(type_id)) => match path_known(defs, type_id, &input) {
                Ok(_) => {}
                Err(_) => {
                    stale = true;
                    let text = if label.is_empty() {
                        input
                            .as_str()
                            .unwrap_or("unknown")
                            .to_owned()
                    } else {
                        label.clone()
                    };
                    stale_paths.push(text);
                }
            },
            (Some(input), None) => {
                // No bound type: treat every stored path as stale so the client can clean up.
                stale = true;
                stale_paths.push(if label.is_empty() {
                    input.as_str().unwrap_or("unknown").to_owned()
                } else {
                    label
                });
            }
            (None, _) => {
                stale = true;
                stale_paths.push(if label.is_empty() {
                    "unknown".into()
                } else {
                    label
                });
            }
        }
        if stale {
            entry.insert("stale".into(), Value::Bool(true));
        } else {
            entry.remove("stale");
        }
        next_columns.push(Value::Object(entry));
    }
    let mut out = document.as_object().cloned().unwrap_or_default();
    out.insert("columns".into(), Value::Array(next_columns));
    (Value::Object(out), stale_paths)
}

async fn resolve_type_id(
    state: &AppState,
    view_state: &Value,
) -> Result<Option<TypeId>, Problem> {
    let Some(token) = bound_type_token(view_state) else {
        return Ok(None);
    };
    let cached = state.models.get(&state.store).await?;
    match wire::resolve_type(&cached.model, token) {
        Ok(id) => Ok(Some(id)),
        Err(_) => Ok(None),
    }
}

#[utoipa::path(
    get,
    path = "/v1/views/{view_id}/column-prefs",
    tag = "platform",
    params(
        ("view_id" = Uuid, Path, description = "View id"),
        ("environment" = Option<String>, Query, description = "Layout environment, default `default`")
    ),
    responses(
        (status = 200, description = "Own column preferences. Stale paths are flagged, never a 500."),
    ),
    security(("bearer" = []))
)]
pub async fn get_column_prefs(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(view_id): Path<Uuid>,
    Query(query): Query<PrefsQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let owner = prefs_owner(&state, &identity).await?;
    let env = environment(query.environment.as_deref());
    let (_type_key, view_state) = require_view(&state, view_id).await?;
    let stored = state
        .store
        .view_column_prefs(owner, &env, view_id)
        .await
        .map_err(store_status)?;
    let type_id = resolve_type_id(&state, &view_state).await?;
    let cached = state.models.get(&state.store).await?;
    let (document, updated_at, stale_paths) = match stored {
        Some((document, updated_at)) => {
            let (annotated, stale) = annotate_stale(&document, &cached.defs, type_id);
            (Some(annotated), Some(updated_at.to_rfc3339()), stale)
        }
        None => (None, None, Vec::new()),
    };
    Ok(Json(json!({
        "environment": env,
        "document": document,
        "updated_at": updated_at,
        "stale_paths": stale_paths,
    })))
}

#[utoipa::path(
    put,
    path = "/v1/views/{view_id}/column-prefs",
    tag = "platform",
    params(("view_id" = Uuid, Path, description = "View id")),
    responses(
        (status = 200, description = "Saved own column preferences"),
        (status = 400, description = "Invalid document, unknown path, duplicate, or too large"),
    ),
    security(("bearer" = []))
)]
pub async fn put_column_prefs(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(view_id): Path<Uuid>,
    Json(body): Json<PrefsBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let owner = prefs_owner(&state, &identity).await?;
    let env = environment(body.environment.as_deref());
    let (_type_key, view_state) = require_view(&state, view_id).await?;
    let type_id = resolve_type_id(&state, &view_state).await?.ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "type",
            "the view has no object type for attribute paths",
        )
    })?;
    let cached = state.models.get(&state.store).await?;
    let document = validate_document(&body.document, &cached.defs, type_id)?;
    let updated = state
        .store
        .put_view_column_prefs(owner, &env, view_id, &document)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "environment": env,
        "document": document,
        "updated_at": updated.to_rfc3339(),
    })))
}

#[utoipa::path(
    delete,
    path = "/v1/views/{view_id}/column-prefs",
    tag = "platform",
    params(
        ("view_id" = Uuid, Path, description = "View id"),
        ("environment" = Option<String>, Query, description = "Layout environment, default `default`")
    ),
    responses((status = 204, description = "Own column preferences removed")),
    security(("bearer" = []))
)]
pub async fn delete_column_prefs(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(view_id): Path<Uuid>,
    Query(query): Query<PrefsQuery>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Viewer)?;
    let owner = prefs_owner(&state, &identity).await?;
    let env = environment(query.environment.as_deref());
    require_view(&state, view_id).await?;
    state
        .store
        .delete_view_column_prefs(owner, &env, view_id)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Agent / operations entry for GET.
pub async fn get_view_column_prefs_op(
    state: &AppState,
    identity: &Identity,
    args: &Value,
) -> Result<Value, Problem> {
    let view_id = args
        .get("view")
        .or_else(|| args.get("view_id"))
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "view",
                "view id is required",
            )
        })?;
    let query = PrefsQuery {
        environment: args
            .get("environment")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    Ok(get_column_prefs(
        State(state.clone()),
        Auth(identity.clone()),
        Path(view_id),
        Query(query),
    )
    .await?
    .0)
}

/// Agent / operations entry for PUT. Always the caller's own user row.
pub async fn put_view_column_prefs_op(
    state: &AppState,
    identity: &Identity,
    args: &Value,
) -> Result<Value, Problem> {
    let view_id = args
        .get("view")
        .or_else(|| args.get("view_id"))
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "view",
                "view id is required",
            )
        })?;
    let document = args.get("document").cloned().ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "document",
            "document is required",
        )
    })?;
    let body = PrefsBody {
        environment: args
            .get("environment")
            .and_then(Value::as_str)
            .map(str::to_owned),
        document,
    };
    Ok(put_column_prefs(
        State(state.clone()),
        Auth(identity.clone()),
        Path(view_id),
        Json(body),
    )
    .await?
    .0)
}
