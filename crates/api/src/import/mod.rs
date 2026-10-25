//! Spreadsheet import routes.

pub mod decisions;
mod diff;
mod mapping;
mod parse;
mod save;
mod session;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use braian_store::PrincipalKind;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::auth::{Auth, Identity, Role};
use crate::platform::emit_job;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

use mapping::{Target, targets_for};
use parse::FileKind;
use session::{emit_import, manual_from_proposal, session_dir, spawn_diff, spawn_prepare};

const XLS_LIMIT: u64 = 200 * 1024 * 1024;

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/imports/{id}", get(show).delete(remove))
        .route("/v1/imports/{id}/sheet", axum::routing::post(sheet))
        .route("/v1/imports/{id}/remap", axum::routing::post(remap))
        .route("/v1/imports/{id}/mapping", axum::routing::put(put_mapping))
        .route("/v1/imports/{id}/approve", axum::routing::post(approve))
        .route("/v1/imports/{id}/rows", get(rows))
        .route("/v1/imports/{id}/save", axum::routing::post(save_import))
        .route("/v1/imports/{id}/failed.xlsx", get(failed_xlsx))
}

pub async fn upload(
    State(state): State<AppState>,
    Auth(identity): Auth,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    let owner = identity.actor.actor.as_uuid();
    let active = state
        .store
        .count_active_imports(owner)
        .await
        .map_err(store_status)?;
    if active >= 3 {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_imports",
            "Three imports are already in progress.",
        ));
    }
    let file_name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(percent_decode)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "upload.csv".into());
    let Some(kind) = FileKind::parse(&file_name) else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "unsupported_file",
            "Only .xlsx, .xls and .csv can be imported.",
        ));
    };
    if kind == FileKind::Xls
        && headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|len| len > XLS_LIMIT)
    {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "file_too_large",
            "This .xls file is larger than 200 MB.",
        ));
    }
    let revision = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    let id = Uuid::now_v7();
    state
        .store
        .insert_import_session(
            id,
            owner,
            identity.actor.on_behalf_of.map(|item| item.as_uuid()),
            &file_name,
            kind.as_str(),
            revision as i64,
        )
        .await
        .map_err(store_status)?;
    let dir = session_dir(id);
    std::fs::create_dir_all(&dir).map_err(|error| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "import",
            error.to_string(),
        )
    })?;
    let path = dir.join(format!("source.{}", kind.extension()));
    let mut file = tokio::fs::File::create(&path).await.map_err(|error| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "import",
            error.to_string(),
        )
    })?;
    let mut stream = body.into_data_stream();
    let mut bytes = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "import", error.to_string()))?;
        bytes += chunk.len() as u64;
        if kind == FileKind::Xls && bytes > XLS_LIMIT {
            let _ = state
                .store
                .update_import_upload(
                    id,
                    bytes as i64,
                    "failed",
                    Some("This .xls file is larger than 200 MB."),
                )
                .await;
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "file_too_large",
                "This .xls file is larger than 200 MB.",
            ));
        }
        file.write_all(&chunk).await.map_err(|error| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "import",
                error.to_string(),
            )
        })?;
    }
    file.flush().await.ok();
    state
        .store
        .update_import_upload(id, bytes as i64, "mapping", None)
        .await
        .map_err(store_status)?;
    if decisions::openai_key().is_none() {
        decisions::log_missing_key_once();
    }
    spawn_prepare(state, id, kind, None);
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "id": id, "status": "mapping", "file_name": file_name, "file_kind": kind.as_str() }),
        ),
    ))
}

async fn show(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let session = load(&state, &identity, id).await?;
    let cached = state.models.get(&state.store).await?;
    let type_key = session
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.pointer("/type/value"))
        .and_then(Value::as_str)
        .or_else(|| cached.defs.types.first().map(|item| item.key.as_str()));
    let targets = type_key
        .map(|key| targets_for(&cached.defs, key))
        .unwrap_or_default();
    Ok(Json(session_body(&session, &targets, &cached.defs)))
}

async fn sheet(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<SheetBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let session = load(&state, &identity, id).await?;
    let kind = FileKind::parse(&format!("file.{}", session.file_kind)).unwrap_or(FileKind::Csv);
    let file_bytes = session.file_bytes;
    if let Err(error) = session::switch_sheet(&state, id, kind, body.sheet_index, file_bytes).await
    {
        state
            .store
            .update_import_upload(id, file_bytes, "failed", Some(&error))
            .await
            .map_err(store_status)?;
        if let Ok(Some(current)) = state.store.import_session(id).await {
            emit_import(&state, &current);
        }
    }
    let session = load(&state, &identity, id).await?;
    let cached = state.models.get(&state.store).await?;
    let type_key = session
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.pointer("/type/value"))
        .and_then(Value::as_str)
        .or_else(|| cached.defs.types.first().map(|item| item.key.as_str()));
    let targets = type_key
        .map(|key| targets_for(&cached.defs, key))
        .unwrap_or_default();
    Ok(Json(session_body(&session, &targets, &cached.defs)))
}

async fn remap(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<RemapBody>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let session = load(&state, &identity, id).await?;
    ensure_revision(&state, &session).await?;
    let manual = body
        .columns
        .map(|columns| {
            columns
                .into_iter()
                .map(|column| mapping::ManualLine {
                    col: column.col,
                    target: column.target,
                })
                .collect()
        })
        .unwrap_or_else(|| {
            session
                .proposal
                .as_ref()
                .map(manual_from_proposal)
                .unwrap_or_default()
        });
    let id_column = match body.id_column.as_deref() {
        Some("none") => Some(None),
        Some(value) => Some(
            value
                .strip_prefix("col:")
                .and_then(|text| text.parse().ok()),
        ),
        None => None,
    };
    session::remap(&state, &session, &body.type_key, id_column, &manual)
        .await
        .map_err(|detail| Problem::new(StatusCode::BAD_REQUEST, "import", detail))?;
    let session = load(&state, &identity, id).await?;
    if let Ok(Some(current)) = state.store.import_session(id).await {
        emit_import(&state, &current);
    }
    let cached = state.models.get(&state.store).await?;
    Ok(Json(session_body(
        &session,
        &targets_for(&cached.defs, &body.type_key),
        &cached.defs,
    )))
}

async fn put_mapping(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    let session = load(&state, &identity, id).await?;
    ensure_revision(&state, &session).await?;
    let cached = state.models.get(&state.store).await?;
    let type_key = body.get("type").and_then(Value::as_str).ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_mapping",
            "Choose an object type.",
        )
    })?;
    let targets = targets_for(&cached.defs, type_key);
    validate_mapping(&body, &targets, &cached)?;
    state
        .store
        .update_import_mapping(id, &body)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn approve(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    let session = load(&state, &identity, id).await?;
    ensure_revision(&state, &session).await?;
    let Some(mapping) = session.mapping.clone() else {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "not_ready",
            "Approve needs a saved mapping.",
        ));
    };
    if state
        .store
        .import_mapping_in_progress(id, &mapping)
        .await
        .map_err(store_status)?
    {
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "status": session.status })),
        ));
    }
    let generation = state
        .store
        .restart_import_diff(id)
        .await
        .map_err(store_status)?;
    spawn_diff(state.clone(), id, generation);
    if let Ok(Some(current)) = state.store.import_session(id).await {
        emit_import(&state, &current);
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "diffing" }))))
}

async fn rows(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<RowQuery>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Viewer)?;
    let _ = load(&state, &identity, id).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let status = query.status.as_deref().filter(|status| *status != "all");
    let total = state
        .store
        .count_import_rows(id, status)
        .await
        .map_err(store_status)?;
    let rows = state
        .store
        .list_import_rows(id, status, offset, limit)
        .await
        .map_err(store_status)?;
    Ok(Json(json!({
        "total": total,
        "rows": rows.into_iter().map(|row| json!({
            "row_no": row.row_no,
            "status": row.status,
            "object_id": row.object_id,
            "object_key": row.object_key,
            "cells": row.cells,
            "errors": row.errors,
        })).collect::<Vec<_>>(),
    })))
}

async fn save_import(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<SaveBody>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    if identity.kind == PrincipalKind::Agent {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "needs_approval",
            "An agent files a proposal instead of saving an import.",
        ));
    }
    let session = load(&state, &identity, id).await?;
    ensure_revision(&state, &session).await?;
    if session.status != "previewed" && session.status != "saved" {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "not_ready",
            "The preview is not ready yet.",
        ));
    }
    let failed = session
        .counters
        .get("failed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if failed > 0 && !body.skip_failed {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "has_failures",
            format!("{failed} rows cannot be imported."),
        ));
    }
    let Some(reservation) = state.bulk.try_reserve() else {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "bulk_busy",
            "Too many bulk jobs are already queued.",
        ));
    };
    state
        .store
        .register_job_type("import.write", "Import", &[], None)
        .await
        .map_err(store_status)?;
    let job_id = Uuid::now_v7();
    let total = i32::try_from(
        session
            .counters
            .get("new")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + session
                .counters
                .get("changed")
                .and_then(Value::as_u64)
                .unwrap_or(0)
            + session
                .counters
                .get("delete")
                .and_then(Value::as_u64)
                .unwrap_or(0),
    )
    .unwrap_or(i32::MAX);
    state
        .store
        .create_job_run(
            job_id,
            "import.write",
            None,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            total,
        )
        .await
        .map_err(store_status)?;
    state
        .store
        .update_import_job(id, "saving", Some(job_id), &session.counters, None)
        .await
        .map_err(store_status)?;
    let actor = identity.actor.clone();
    let job_state = state.clone();
    tokio::spawn(async move {
        reservation
            .finish(save::run(job_state, id, job_id, actor))
            .await;
    });
    if let Ok(Some(current)) = state.store.import_session(id).await {
        emit_import(&state, &current);
    }
    emit_job(&state, job_id, "running", 0, total);
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

async fn failed_xlsx(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    let session = load(&state, &identity, id).await?;
    let rows = state
        .store
        .list_import_rows(id, Some("failed"), 0, 100_000)
        .await
        .map_err(store_status)?;
    let bytes = failed_workbook(&session.headers, &rows)
        .map_err(|error| Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "import", error))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"failed.xlsx\"",
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

async fn remove(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, Problem> {
    require(&identity, Role::Editor)?;
    let session = load(&state, &identity, id).await?;
    if session.status == "saving" && !query.force.unwrap_or(false) {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "saving",
            "A save is still running. Pass force to cancel it.",
        ));
    }
    if session.status == "saving" {
        state
            .store
            .update_import_job(
                id,
                "cancelled",
                session.job_id,
                &session.counters,
                Some("cancelled"),
            )
            .await
            .map_err(store_status)?;
        if let Some(job_id) = session.job_id {
            let _ = state
                .store
                .finish_job_run(job_id, "cancelled", 0, "cancelled", &json!([]))
                .await;
            emit_job(&state, job_id, "cancelled", 0, 0);
        }
    }
    state
        .store
        .delete_import_session(id)
        .await
        .map_err(store_status)?;
    let _ = std::fs::remove_dir_all(session_dir(id));
    Ok(StatusCode::NO_CONTENT)
}

fn session_body(
    session: &braian_store::ImportSession,
    targets: &[Target],
    defs: &braian_model::DefinitionSet,
) -> Value {
    let types = defs
        .types
        .iter()
        .map(|object_type| {
            json!({
                "key": object_type.key,
                "label": object_type.labels.iter().next().map(|(_, label)| label.clone()).unwrap_or_else(|| object_type.key.clone()),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "types": types,
        "id": session.id,
        "status": session.status,
        "file_name": session.file_name,
        "file_kind": session.file_kind,
        "sheets": session.sheets,
        "sheet_index": session.sheet_index,
        "headers": session.headers,
        "sample": session.sample,
        "rows_parsed": session.rows_parsed,
        "parse_done": session.parse_done,
        "model_revision": session.model_revision,
        "targets": mapping::target_wire(targets),
        "proposal": session.proposal,
        "mapping": session.mapping,
        "counters": session.counters,
        "job_id": session.job_id,
        "error": session.error,
    })
}

fn validate_mapping(
    body: &Value,
    targets: &[Target],
    cached: &crate::state::CachedModel,
) -> Result<(), Problem> {
    let type_key = body.get("type").and_then(Value::as_str).unwrap_or("");
    let mut used = std::collections::BTreeSet::new();
    let columns = body
        .get("columns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut action = 0;
    for column in &columns {
        let target = column.get("target").and_then(Value::as_str).unwrap_or("");
        if target == "unmapped" {
            continue;
        }
        if target == "action" {
            action += 1;
            continue;
        }
        if !targets.iter().any(|item| item.id == target) {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_mapping",
                format!("Unknown field {target}."),
            ));
        }
        if !used.insert(target.to_owned()) {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_mapping",
                format!("{target} is mapped twice."),
            ));
        }
    }
    if action > 1 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_mapping",
            "Only one action column is allowed.",
        ));
    }
    if let Some(index) = body.get("id_column").and_then(Value::as_u64) {
        let key = cached
            .model
            .type_id(type_key)
            .and_then(|type_id| cached.model.key_attribute(type_id));
        let key_name = key
            .and_then(|id| cached.model.attribute(id))
            .map(|attribute| attribute.key.as_str());
        let mapped = columns
            .iter()
            .find(|column| column.get("col").and_then(Value::as_u64) == Some(index));
        let target = mapped
            .and_then(|column| column.get("target"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let expected = key_name
            .map(|name| format!("type:{name}"))
            .unwrap_or_default();
        if target != expected {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_mapping",
                "The id column has to be the key attribute.",
            ));
        }
    }
    Ok(())
}

async fn load(
    state: &AppState,
    identity: &Identity,
    id: Uuid,
) -> Result<braian_store::ImportSession, Problem> {
    let session = state
        .store
        .import_session(id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "That import does not exist.",
            )
        })?;
    if session.owner != identity.actor.actor.as_uuid() && identity.role != Role::Admin {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That import does not exist.",
        ));
    }
    Ok(session)
}

async fn ensure_revision(
    state: &AppState,
    session: &braian_store::ImportSession,
) -> Result<(), Problem> {
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    if session.model_revision != current as i64 {
        Err(Problem::new(
            StatusCode::CONFLICT,
            "model_changed",
            "The data model changed. Re-checking the mapping…",
        ))
    } else {
        Ok(())
    }
}

fn require(identity: &Identity, role: Role) -> Result<(), Problem> {
    if identity.role.allows(role) {
        Ok(())
    } else {
        Err(Problem::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "The role does not allow this.",
        ))
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|text| u8::from_str_radix(text, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn failed_workbook(
    headers: &Value,
    rows: &[braian_store::ImportRowOut],
) -> Result<Vec<u8>, String> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    let names: Vec<String> = headers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|header| header.get("raw").and_then(Value::as_str).map(str::to_owned))
        .collect();
    for (index, name) in names.iter().enumerate() {
        sheet
            .write_string(0, index as u16, name)
            .map_err(|error| error.to_string())?;
    }
    sheet
        .write_string(0, names.len() as u16, "Error")
        .map_err(|error| error.to_string())?;
    for (row_index, row) in rows.iter().enumerate() {
        for (index, _) in names.iter().enumerate() {
            let value = row
                .cells
                .get(index.to_string())
                .and_then(|cell| cell.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");
            sheet
                .write_string(row_index as u32 + 1, index as u16, value)
                .map_err(|error| error.to_string())?;
        }
        let message = row
            .errors
            .as_array()
            .and_then(|errors| errors.first())
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        sheet
            .write_string(row_index as u32 + 1, names.len() as u16, message)
            .map_err(|error| error.to_string())?;
    }
    workbook.save_to_buffer().map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct SheetBody {
    sheet_index: usize,
}

#[derive(Deserialize)]
struct RemapBody {
    #[serde(rename = "type")]
    type_key: String,
    id_column: Option<String>,
    columns: Option<Vec<ColumnBody>>,
}

#[derive(Deserialize)]
struct ColumnBody {
    col: usize,
    target: String,
}

#[derive(Deserialize)]
struct RowQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct SaveBody {
    #[serde(default)]
    skip_failed: bool,
}

#[derive(Deserialize)]
struct DeleteQuery {
    force: Option<bool>,
}

pub fn spawn_janitor(state: AppState) {
    session::spawn_janitor(state);
}
