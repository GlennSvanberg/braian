//! Saved column mappings: shared by everyone who can import, reused when the headers match.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::mapping::ManualLine;
use super::{load, require};
use crate::auth::{Auth, Role};
use crate::problem::{Problem, store_status};
use crate::state::AppState;

/// Header names of a file, normalised and sorted. Two files with the same set of
/// headers get the same signature, whatever the column order.
pub fn signature(headers: &Value) -> String {
    let mut names: Vec<String> = headers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|header| header.get("name").and_then(Value::as_str))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    names.sort();
    names.join("\n")
}

/// What to reuse for a new file: the type, the key column and a manual line per column.
pub struct Reuse {
    pub type_key: String,
    pub id_column: Option<Option<usize>>,
    pub lines: Vec<ManualLine>,
    pub name: String,
}

/// The mapping that fits these headers exactly, if one was saved.
pub async fn find_for(state: &AppState, headers: &Value) -> Option<Reuse> {
    let signature = signature(headers);
    if signature.is_empty() {
        return None;
    }
    let saved = state
        .store
        .mappings_for_signature(&signature)
        .await
        .ok()?
        .into_iter()
        .next()?;
    let index_of = |name: &str| -> Option<usize> {
        headers.as_array()?.iter().find_map(|header| {
            (header.get("name").and_then(Value::as_str) == Some(name))
                .then(|| header.get("index").and_then(Value::as_u64))
                .flatten()
                .map(|index| index as usize)
        })
    };
    let lines = saved
        .mapping
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|column| {
            let name = column.get("header")?.as_str()?;
            Some(ManualLine {
                col: index_of(name)?,
                target: column.get("target")?.as_str()?.to_owned(),
            })
        })
        .collect();
    let id_column = match saved.mapping.get("id_header").and_then(Value::as_str) {
        Some(name) => Some(index_of(name)),
        None => Some(None),
    };
    Some(Reuse {
        type_key: saved.type_key,
        id_column,
        lines,
        name: saved.name,
    })
}

fn view(saved: &braian_store::SavedMapping) -> Value {
    json!({
        "id": saved.id,
        "name": saved.name,
        "type": saved.type_key,
        "headers": saved.headers,
        "columns": saved.mapping.get("columns").cloned().unwrap_or(Value::Null),
        "empty_clears": saved.mapping.get("empty_clears").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub async fn list(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let items = state
        .store
        .list_import_mappings()
        .await
        .map_err(store_status)?;
    Ok(Json(
        json!({ "mappings": items.iter().map(view).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub session_id: Uuid,
    pub name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<SaveBody>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_name",
            "Give the mapping a name of 1 to 120 characters.",
        ));
    }
    let session = load(&state, &identity, body.session_id).await?;
    let Some(mapping) = session.mapping.clone() else {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "not_ready",
            "Save needs a mapping.",
        ));
    };
    let headers = session.headers.as_array().cloned().unwrap_or_default();
    let name_of = |col: u64| -> Option<String> {
        headers.iter().find_map(|header| {
            (header.get("index").and_then(Value::as_u64) == Some(col))
                .then(|| {
                    header
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten()
        })
    };
    let columns: Vec<Value> = mapping
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|column| {
            let target = column.get("target")?.as_str()?;
            if target == "unmapped" {
                return None;
            }
            let header = name_of(column.get("col")?.as_u64()?)?;
            Some(json!({ "header": header, "target": target }))
        })
        .collect();
    let id_header = mapping
        .get("id_column")
        .and_then(Value::as_u64)
        .and_then(name_of);
    let type_key = mapping
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let stored = json!({
        "columns": columns,
        "id_header": id_header,
        "empty_clears": mapping.get("empty_clears").and_then(Value::as_bool).unwrap_or(false),
    });
    let saved = braian_store::SavedMapping {
        id: Uuid::now_v7(),
        name: name.to_owned(),
        type_key,
        header_signature: signature(&session.headers),
        headers: json!(
            headers
                .iter()
                .filter_map(|header| header.get("raw").cloned())
                .collect::<Vec<_>>()
        ),
        mapping: stored,
        created_by: identity.actor.actor.as_uuid(),
    };
    state
        .store
        .save_import_mapping(&saved)
        .await
        .map_err(store_status)?;
    Ok((StatusCode::CREATED, Json(view(&saved))))
}

#[derive(Deserialize)]
pub struct RenameBody {
    pub name: String,
}

pub async fn rename(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameBody>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_name",
            "Give the mapping a name of 1 to 120 characters.",
        ));
    }
    let found = state
        .store
        .rename_import_mapping(id, name)
        .await
        .map_err(store_status)?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That mapping does not exist.",
        ))
    }
}

pub async fn remove(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    let found = state
        .store
        .delete_import_mapping(id)
        .await
        .map_err(store_status)?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That mapping does not exist.",
        ))
    }
}
