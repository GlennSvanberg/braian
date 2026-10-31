//! Spreadsheet import routes.

pub mod decisions;
mod diff;
mod entity;
mod mapping;
mod parse;
mod save;
mod saved;
mod session;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::auth::{Auth, Identity, Role};
use crate::platform::emit_job;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

use mapping::{Target, targets_for};
use parse::FileKind;
#[derive(Deserialize)]
struct RowEdit {
    col: i32,
    value: String,
}

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
        .route(
            "/v1/imports/{id}/rows/{row_no}",
            axum::routing::patch(patch_row),
        )
        .route("/v1/imports/{id}/failed.xlsx", get(failed_xlsx))
        .route("/v1/import-mappings", get(saved::list).post(saved::create))
        .route(
            "/v1/import-mappings/{id}",
            axum::routing::patch(saved::rename).delete(saved::remove),
        )
}

pub async fn upload(
    State(state): State<AppState>,
    Auth(identity): Auth,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<Value>), Problem> {
    require(&identity, Role::Editor)?;
    let owner = identity.actor.actor.as_uuid();
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
    let check_path = path.clone();
    let readable = tokio::task::spawn_blocking(move || parse::ensure_has_rows(&check_path, kind))
        .await
        .map_err(|error| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "import",
                error.to_string(),
            )
        })?;
    if let Err(error) = readable {
        let _ = state.store.delete_import_session(id).await;
        let _ = std::fs::remove_dir_all(session_dir(id));
        let code = if error.message == "This file has no rows." {
            "empty_file"
        } else {
            "unreadable_file"
        };
        return Err(Problem::new(StatusCode::BAD_REQUEST, code, error.message));
    }
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
    Ok(Json(
        with_draft(
            &state,
            &session,
            session_body(&session, &targets, &cached.defs),
        )
        .await,
    ))
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
    Ok(Json(
        with_draft(
            &state,
            &session,
            session_body(&session, &targets, &cached.defs),
        )
        .await,
    ))
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
    let body_json = session_body(
        &session,
        &targets_for(&cached.defs, &body.type_key),
        &cached.defs,
    );
    Ok(Json(with_draft(&state, &session, body_json).await))
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
    let mut stored = body.clone();
    braian_model::translate_import_mapping(&mut stored, &cached.defs);
    state
        .store
        .update_import_mapping(id, &stored)
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

/// Approves an import draft. The rows were checked when the preview was made;
/// this starts the background write on the bulk lane. `exclude` and `only` are
/// row numbers from the preview, so a person can deselect thousands of rows
/// without sending thousands of ids.
pub(crate) async fn accept_import(
    state: &AppState,
    identity: &Identity,
    session_id: Uuid,
    body: &crate::platform::AcceptBody,
) -> Result<Value, Problem> {
    require(identity, Role::Editor)?;
    crate::auth::require_commit(identity)?;
    let session = state
        .store
        .import_session(session_id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "That import does not exist.",
            )
        })?;
    refresh_if_model_changed(state, &session).await?;
    if session.status != "previewed" {
        let (code, detail) = save_conflict(&session.status)
            .unwrap_or(("not_ready", "The preview is not ready yet."));
        return Err(Problem::new(StatusCode::CONFLICT, code, detail));
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
            format!("{failed} rows cannot be imported and will be skipped."),
        ));
    }
    let is_model = session
        .mapping
        .as_ref()
        .and_then(|mapping| mapping.get("type"))
        .and_then(Value::as_str)
        .is_some_and(entity::is_model);
    if is_model {
        return accept_model(state, identity, &session, body).await;
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
    let claimed = state
        .store
        .claim_import_save(session_id, job_id, &session.counters)
        .await
        .map_err(store_status)?;
    if !claimed {
        let current = state
            .store
            .import_session(session_id)
            .await
            .map_err(store_status)?
            .map(|item| item.status)
            .unwrap_or_default();
        let (code, detail) =
            save_conflict(&current).unwrap_or(("already_saving", "A save is already running."));
        return Err(Problem::new(StatusCode::CONFLICT, code, detail));
    }
    let only = body.only_rows.as_deref();
    let exclude = body.exclude_rows.as_deref().unwrap_or(&[]);
    let remaining = state
        .store
        .exclude_import_rows(session_id, only, exclude)
        .await
        .map_err(store_status)?;
    let counts = state
        .store
        .import_status_counts(session_id)
        .await
        .map_err(store_status)?;
    let mut counters = session.counters.clone();
    for key in ["new", "changed", "failed", "stale", "excluded"] {
        counters[key] = json!(counts.get(key).copied().unwrap_or(0));
    }
    let total = i32::try_from(remaining).unwrap_or(i32::MAX);
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
        .update_import_job(session_id, "saving", Some(job_id), &counters, None)
        .await
        .map_err(store_status)?;
    let actor = identity.actor.clone();
    let job_state = state.clone();
    tokio::spawn(async move {
        reservation
            .finish(save::run(job_state, session_id, job_id, actor))
            .await;
    });
    if let Ok(Some(current)) = state.store.import_session(session_id).await {
        emit_import(state, &current);
    }
    emit_job(state, job_id, "running", 0, total);
    Ok(json!({ "id": job_id, "job_id": job_id, "status": "running", "total": total }))
}

const AGENT_FILE_LIMIT: usize = 2 * 1024 * 1024;

/// The import operations of the registry. An agent starts a draft, maps and previews it,
/// and a person approves it in the changes view. There is no accept operation on purpose.
pub(crate) async fn run_agent_op(
    state: &AppState,
    identity: &Identity,
    name: &str,
    args: &Value,
) -> Result<Value, Problem> {
    let id = || -> Result<Uuid, Problem> {
        args.get("id")
            .and_then(Value::as_str)
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or_else(|| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_id", "Give the import id.")
            })
    };
    match name {
        "import.start" => {
            require(identity, Role::Editor)?;
            let file_name = args
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or("agent.csv");
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            if content.is_empty() || content.len() > AGENT_FILE_LIMIT {
                return Err(Problem::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_body",
                    "Give the file as CSV text, at most 2 MB.",
                ));
            }
            let kind = FileKind::parse(file_name)
                .filter(|kind| *kind == FileKind::Csv)
                .ok_or_else(|| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_file",
                        "An agent can start an import from a .csv text.",
                    )
                })?;
            let owner = identity.actor.actor.as_uuid();
            let revision = state
                .store
                .model_revision()
                .await
                .map_err(store_status)?
                .unwrap_or(0);
            let session_id = Uuid::now_v7();
            state
                .store
                .insert_import_session(
                    session_id,
                    owner,
                    identity.actor.on_behalf_of.map(|item| item.as_uuid()),
                    file_name,
                    kind.as_str(),
                    revision as i64,
                )
                .await
                .map_err(store_status)?;
            let dir = session_dir(session_id);
            let io = |error: std::io::Error| {
                Problem::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "import",
                    error.to_string(),
                )
            };
            std::fs::create_dir_all(&dir).map_err(io)?;
            let path = dir.join(format!("source.{}", kind.extension()));
            std::fs::write(&path, content).map_err(io)?;
            let check = path.clone();
            let readable =
                tokio::task::spawn_blocking(move || parse::ensure_has_rows(&check, kind))
                    .await
                    .map_err(|error| {
                        Problem::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "import",
                            error.to_string(),
                        )
                    })?;
            if let Err(error) = readable {
                let _ = state.store.delete_import_session(session_id).await;
                let _ = std::fs::remove_dir_all(&dir);
                return Err(Problem::new(
                    StatusCode::BAD_REQUEST,
                    "unreadable_file",
                    error.message,
                ));
            }
            state
                .store
                .update_import_upload(session_id, content.len() as i64, "mapping", None)
                .await
                .map_err(store_status)?;
            spawn_prepare(state.clone(), session_id, kind, None);
            Ok(json!({ "id": session_id, "status": "mapping", "file_name": file_name }))
        }
        "import.status" => {
            let Json(body) =
                show(State(state.clone()), Auth(identity.clone()), Path(id()?)).await?;
            Ok(body)
        }
        "import.map" => {
            let mapping = args.get("mapping").cloned().ok_or_else(|| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_body", "Give the mapping.")
            })?;
            put_mapping(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id()?),
                Json(mapping),
            )
            .await?;
            Ok(json!({ "status": "mapped" }))
        }
        "import.preview" => {
            let (_, Json(body)) =
                approve(State(state.clone()), Auth(identity.clone()), Path(id()?)).await?;
            Ok(body)
        }
        "import.discard" => {
            require(identity, Role::Editor)?;
            let id = id()?;
            load(state, identity, id).await?;
            discard_import(state, id).await?;
            Ok(json!({ "status": "discarded" }))
        }
        "import.mappings.list" => {
            let Json(body) = saved::list(State(state.clone()), Auth(identity.clone())).await?;
            Ok(body)
        }
        "import.mappings.rename" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            saved::rename(
                State(state.clone()),
                Auth(identity.clone()),
                Path(id()?),
                Json(saved::RenameBody { name }),
            )
            .await?;
            Ok(json!({ "status": "renamed" }))
        }
        "import.mappings.delete" => {
            saved::remove(State(state.clone()), Auth(identity.clone()), Path(id()?)).await?;
            Ok(json!({ "status": "deleted" }))
        }
        _ => Err(Problem::new(
            StatusCode::NOT_FOUND,
            "unknown_operation",
            name,
        )),
    }
}

/// Approves a data model sheet: every selected row becomes a model change, all in one revision.
async fn accept_model(
    state: &AppState,
    identity: &Identity,
    session: &braian_store::ImportSession,
    body: &crate::platform::AcceptBody,
) -> Result<Value, Problem> {
    let session_id = session.id;
    let only = body.only_rows.as_deref();
    let exclude = body.exclude_rows.as_deref().unwrap_or(&[]);
    state
        .store
        .exclude_import_rows(session_id, only, exclude)
        .await
        .map_err(store_status)?;
    let mut rows = Vec::new();
    for status in ["new", "changed"] {
        let mut after = 0;
        loop {
            let page = state
                .store
                .scan_import_rows(session_id, status, after, 500)
                .await
                .map_err(store_status)?;
            let Some(last) = page.last() else { break };
            after = last.row_no;
            rows.extend(page);
        }
    }
    rows.sort_by_key(|row| row.row_no);
    let mut steps = Vec::with_capacity(rows.len());
    for row in &rows {
        let patch = row.patch.clone().unwrap_or(Value::Null);
        let step = serde_json::from_value::<braian_model::ModelChange>(patch).map_err(|error| {
            Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_row",
                format!("Row {} is not a valid model change: {error}", row.row_no),
            )
        })?;
        steps.push(step);
    }
    let reply =
        crate::model_draft::commit_steps(state, &identity.actor, &steps, false, None).await?;
    let counts = state
        .store
        .import_status_counts(session_id)
        .await
        .map_err(store_status)?;
    let mut counters = session.counters.clone();
    for key in ["new", "changed", "failed", "stale", "excluded"] {
        counters[key] = json!(counts.get(key).copied().unwrap_or(0));
    }
    counters["saved"] = json!(steps.len());
    state
        .store
        .update_import_job(session_id, "saved", None, &counters, None)
        .await
        .map_err(store_status)?;
    state
        .store
        .close_import_proposal(session_id, "accepted", Some(&counters))
        .await
        .map_err(store_status)?;
    if let Ok(Some(current)) = state.store.import_session(session_id).await {
        emit_import(state, &current);
    }
    state.emit(json!({ "op": "proposal", "session": session_id }));
    Ok(json!({ "status": "saved", "total": steps.len(), "revision": reply }))
}

/// Throws an import draft away: the rows, the spooled file and the proposal.
pub(crate) async fn discard_import(state: &AppState, session_id: Uuid) -> Result<(), Problem> {
    let Some(session) = state
        .store
        .import_session(session_id)
        .await
        .map_err(store_status)?
    else {
        state
            .store
            .close_import_proposal(session_id, "discarded", None)
            .await
            .map_err(store_status)?;
        return Ok(());
    };
    if session.status == "saving" {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "saving",
            "The draft is being written. It cannot be discarded now.",
        ));
    }
    state
        .store
        .close_import_proposal(session_id, "discarded", None)
        .await
        .map_err(store_status)?;
    state
        .store
        .delete_import_session(session_id)
        .await
        .map_err(store_status)?;
    let _ = std::fs::remove_dir_all(session_dir(session_id));
    state.emit(json!({ "op": "proposal", "session": session_id }));
    Ok(())
}

/// One page of an import draft, in the shape the review table renders: the
/// mapped columns once, then rows keyed by file column index.
pub(crate) async fn draft_rows(
    state: &AppState,
    session_id: Uuid,
    status: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Value, Problem> {
    let session = state
        .store
        .import_session(session_id)
        .await
        .map_err(store_status)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "That import does not exist.",
            )
        })?;
    let cached = state.models.get(&state.store).await?;
    let mut mapping = session.mapping.clone().unwrap_or(Value::Null);
    braian_model::translate_import_mapping(&mut mapping, &cached.defs);
    let type_key = mapping
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let targets = targets_for(&cached.defs, &type_key);
    let headers = session.headers.as_array().cloned().unwrap_or_default();
    let columns: Vec<Value> = mapping
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|column| {
            let col = column.get("col").and_then(Value::as_u64)?;
            let target = column.get("target").and_then(Value::as_str)?;
            if target == "unmapped" || target == "action" {
                return None;
            }
            let header = headers
                .iter()
                .find(|item| item.get("index").and_then(Value::as_u64) == Some(col))
                .and_then(|item| item.get("raw").and_then(Value::as_str))
                .unwrap_or_default();
            let label = mapping::find_target(&targets, target)
                .map(|item| item.label.clone())
                .unwrap_or_else(|| target.to_owned());
            let options = mapping::find_target(&targets, target)
                .filter(|item| item.kind == "value_list")
                .and_then(|item| item.attribute_id.as_deref())
                .map(|id| value_list_options(&cached.defs, id));
            Some(json!({
                "col": col,
                "header": header,
                "target": target,
                "label": label,
                "options": options,
            }))
        })
        .collect();
    let total = state
        .store
        .count_import_rows(session_id, status)
        .await
        .map_err(store_status)?;
    let rows = state
        .store
        .list_import_rows(session_id, status, offset, limit)
        .await
        .map_err(store_status)?;
    Ok(json!({
        "kind": "import",
        "total": total,
        "session_id": session_id,
        "session_status": session.status,
        "skip_invalid": mapping.get("skip_invalid").and_then(Value::as_bool).unwrap_or(false),
        "type": type_key,
        "columns": columns,
        "counters": session.counters,
        "rows": rows.into_iter().map(|row| json!({
            "row_no": row.row_no,
            "status": row.status,
            "object_id": row.object_id,
            "object_key": row.object_key,
            "cells": row.cells,
            "errors": row.errors,
        })).collect::<Vec<_>>(),
    }))
}

/// The choices of a value list attribute: shared list first, then inline options.
/// The label is what the cell shows and what an edit sends back.
fn value_list_options(defs: &braian_model::DefinitionSet, attribute_id: &str) -> Vec<Value> {
    let Some(attribute) = defs.attributes.iter().find(|item| {
        item.id
            .as_uuid()
            .to_string()
            .eq_ignore_ascii_case(attribute_id)
    }) else {
        return Vec::new();
    };
    let braian_model::AttributeKind::ValueList { options, list } = &attribute.kind else {
        return Vec::new();
    };
    let shared = list
        .as_deref()
        .and_then(|key| defs.value_lists.iter().find(|item| item.key == key))
        .map(|item| item.options.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    shared
        .into_iter()
        .chain(options.iter())
        .map(|option| {
            let label = option
                .labels
                .iter()
                .find(|(tag, _)| tag.as_str() == "en")
                .or_else(|| option.labels.iter().next())
                .map(|(_, text)| text.clone())
                .unwrap_or_else(|| option.code.clone());
            json!({ "value": label, "label": label, "code": option.code })
        })
        .collect()
}

async fn patch_row(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path((id, row_no)): Path<(Uuid, i32)>,
    Json(body): Json<RowEdit>,
) -> Result<Json<Value>, Problem> {
    require(&identity, Role::Editor)?;
    let _ = load(&state, &identity, id).await?;
    if row_no < 0 || body.col < 0 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_row",
            "That row or column does not exist.",
        ));
    }
    let (row, counters) =
        session::edit_cell(&state, id, row_no as u32, body.col as usize, &body.value)
            .await
            .map_err(|detail| Problem::new(StatusCode::CONFLICT, "cannot_edit", detail))?;
    Ok(Json(json!({
        "counters": counters,
        "row": row.map(|row| json!({
            "row_no": row.row_no,
            "status": row.status,
            "object_id": row.object_id,
            "object_key": row.object_key,
            "cells": row.cells,
            "errors": row.errors,
        })),
    })))
}
/// When the model moved on since the preview, check the rows again against the
/// current model instead of writing rows that were validated against an old one.
async fn refresh_if_model_changed(
    state: &AppState,
    session: &braian_store::ImportSession,
) -> Result<(), Problem> {
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0) as i64;
    if session.model_revision == current {
        return Ok(());
    }
    state
        .store
        .set_import_model_revision(session.id, current)
        .await
        .map_err(store_status)?;
    if session.mapping.is_some() && session.status == "previewed" {
        let generation = state
            .store
            .restart_import_diff(session.id)
            .await
            .map_err(store_status)?;
        session::spawn_diff(state.clone(), session.id, generation);
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "model_changed",
        "The data model changed. Re-checked against the current model; review it again.",
    ))
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
    if !identity.may_commit() {
        return Err(crate::auth::delete_forbidden());
    }
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
        .close_import_proposal(id, "discarded", None)
        .await
        .map_err(store_status)?;
    state
        .store
        .delete_import_session(id)
        .await
        .map_err(store_status)?;
    let _ = std::fs::remove_dir_all(session_dir(id));
    state.emit(json!({ "op": "proposal", "session": id }));
    Ok(StatusCode::NO_CONTENT)
}

async fn with_draft(
    state: &AppState,
    session: &braian_store::ImportSession,
    mut body: Value,
) -> Value {
    let draft = state.store.session_draft(session.id).await.ok().flatten();
    body["draft_id"] = json!(draft);
    body
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
                "id": object_type.id.as_uuid(),
                "key": object_type.key,
                "label": object_type.singular_label("en"),
            })
        })
        .chain(
            entity::options()
                .into_iter()
                .map(|(token, label)| json!({ "id": token, "key": token, "label": label })),
        )
        .collect::<Vec<_>>();
    let mut mapping = session.mapping.clone();
    if let Some(mapping) = mapping.as_mut() {
        braian_model::translate_import_mapping(mapping, defs);
    }
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
        "mapping": mapping,
        "counters": session.counters,
        "job_id": session.job_id,
        "error": session.error,
    })
}

fn validate_mapping(
    body: &Value,
    targets: &[Target],
    _cached: &crate::state::CachedModel,
) -> Result<(), Problem> {
    let mut used = std::collections::BTreeSet::new();
    let columns = body
        .get("columns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for column in &columns {
        let target = column.get("target").and_then(Value::as_str).unwrap_or("");
        if target == "unmapped" {
            continue;
        }
        if target == "action" {
            // Old saved mappings may still carry the action column. Imports
            // never delete, so the column is ignored.
            continue;
        }
        if mapping::find_target(targets, target).is_none() {
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
    if let Some(index) = body.get("id_column").and_then(Value::as_u64) {
        let mapped = columns
            .iter()
            .find(|column| column.get("col").and_then(Value::as_u64) == Some(index));
        let target = mapped
            .and_then(|column| column.get("target"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !mapping::find_target(targets, target).is_some_and(|item| item.key_attribute) {
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct SheetBody {
    sheet_index: usize,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RemapBody {
    #[serde(rename = "type")]
    type_key: String,
    id_column: Option<String>,
    columns: Option<Vec<ColumnBody>>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ColumnBody {
    col: usize,
    target: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct RowQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct DeleteQuery {
    force: Option<bool>,
}

pub fn spawn_janitor(state: AppState) {
    session::spawn_janitor(state);
}

fn save_conflict(status: &str) -> Option<(&'static str, &'static str)> {
    match status {
        "saving" => Some(("already_saving", "A save is already running.")),
        "previewed" | "saved" => None,
        _ => Some(("not_ready", "The preview is not ready yet.")),
    }
}

#[cfg(test)]
mod tests {
    use super::save_conflict;

    #[test]
    fn a_running_save_is_not_described_as_a_missing_preview() {
        assert_eq!(
            save_conflict("saving"),
            Some(("already_saving", "A save is already running."))
        );
        assert_eq!(
            save_conflict("diffing").map(|item| item.0),
            Some("not_ready")
        );
        assert_eq!(save_conflict("previewed"), None);
    }
}
