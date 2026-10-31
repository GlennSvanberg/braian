//! Background save through the bulk lane.

use braian_model::{Object, ObjectId, ObjectPatch};
use braian_store::{Actor, BatchKind, Error, ImportRowOut, ObjectOp};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::platform::emit_job;
use crate::state::AppState;

use super::session::emit_import;

const NEW_CHUNK: i64 = 5_000;
const CHANGE_CHUNK: i64 = 500;

pub async fn run(state: AppState, session_id: Uuid, job_id: Uuid, mut actor: Actor) {
    actor.kind = BatchKind::Bulk;
    actor.source = "import".into();
    let store = state.store.on_bulk();
    let Some(session) = (match state.store.import_session(session_id).await {
        Ok(session) => session,
        Err(error) => {
            finish(
                &state,
                &store,
                session_id,
                job_id,
                "failed",
                0,
                0,
                &error.to_string(),
                &json!([]),
            )
            .await;
            return;
        }
    }) else {
        return;
    };
    let counters = session.counters.clone();
    let total = counter(&counters, "new") + counter(&counters, "changed");
    let total_i = i32::try_from(total).unwrap_or(i32::MAX);
    emit_job(&state, job_id, "running", 0, total_i);
    let batch = match store.open_batch(&actor).await {
        Ok(batch) => batch,
        Err(error) => {
            finish(
                &state,
                &store,
                session_id,
                job_id,
                "failed",
                0,
                total_i,
                &error.to_string(),
                &json!([]),
            )
            .await;
            return;
        }
    };
    let mut done = 0i32;
    let mut errors = Vec::new();
    if !write_status(
        &state,
        &store,
        session_id,
        job_id,
        batch,
        &actor,
        "new",
        NEW_CHUNK,
        &mut done,
        total_i,
        &mut errors,
    )
    .await
    {
        return;
    }
    if !write_status(
        &state,
        &store,
        session_id,
        job_id,
        batch,
        &actor,
        "changed",
        CHANGE_CHUNK,
        &mut done,
        total_i,
        &mut errors,
    )
    .await
    {
        return;
    }
    let status = if cancelled(&state, session_id).await {
        "cancelled"
    } else {
        "succeeded"
    };
    let session_status = if status == "cancelled" {
        "cancelled"
    } else {
        "saved"
    };
    let _ = store
        .finish_job_run(
            job_id,
            status,
            done,
            &format!("{done} rows saved"),
            &Value::Array(errors.clone()),
        )
        .await;
    emit_job(&state, job_id, status, done, total_i);
    let mut final_counters = counters.clone();
    if let Ok(counts) = state.store.import_status_counts(session_id).await {
        for key in ["new", "changed", "failed", "stale", "excluded"] {
            final_counters[key] = json!(counts.get(key).copied().unwrap_or(0));
        }
        final_counters["written"] = json!(done);
    }
    let _ = state
        .store
        .update_import_job(
            session_id,
            session_status,
            Some(job_id),
            &final_counters,
            None,
        )
        .await;
    let _ = state
        .store
        .close_import_proposal(
            session_id,
            if status == "cancelled" {
                "discarded"
            } else {
                "accepted"
            },
            Some(&final_counters),
        )
        .await;
    if let Ok(Some(session)) = state.store.import_session(session_id).await {
        emit_import(&state, &session);
    }
    state.emit(json!({ "op": "proposal", "session": session_id }));
}

#[allow(clippy::too_many_arguments)]
async fn write_status(
    state: &AppState,
    store: &braian_store::Store,
    session_id: Uuid,
    job_id: Uuid,
    batch: braian_store::BatchId,
    actor: &Actor,
    status: &str,
    chunk: i64,
    done: &mut i32,
    total: i32,
    errors: &mut Vec<Value>,
) -> bool {
    let mut after = 0i32;
    loop {
        if cancelled(state, session_id).await {
            let _ = store
                .finish_job_run(
                    job_id,
                    "cancelled",
                    *done,
                    "cancelled",
                    &Value::Array(errors.clone()),
                )
                .await;
            emit_job(state, job_id, "cancelled", *done, total);
            return false;
        }
        let rows = match store
            .scan_import_rows(session_id, status, after, chunk)
            .await
        {
            Ok(rows) => rows,
            Err(error) => {
                let message = error.to_string();
                errors.push(json!({ "message": message }));
                finish(
                    state,
                    store,
                    session_id,
                    job_id,
                    "failed",
                    *done,
                    total,
                    &message,
                    &Value::Array(errors.clone()),
                )
                .await;
                return false;
            }
        };
        if rows.is_empty() {
            return true;
        }
        after = rows.last().map(|row| row.row_no).unwrap_or(after);
        if status == "new" {
            write_new(state, store, session_id, batch, actor, &rows, errors).await;
        } else {
            write_ops(state, store, session_id, batch, actor, &rows, errors).await;
        }
        *done += i32::try_from(rows.len()).unwrap_or(0);
        let _ = store
            .update_job_progress(
                job_id,
                *done,
                &format!("{done} rows saved"),
                &Value::Array(errors.clone()),
            )
            .await;
        emit_job(state, job_id, "running", *done, total);
        // `import_chunk` and `write_ops` already split and pause on this lane.
        // This wait covers the gap before the next scan while interactive
        // requests are in flight: the pause grows and stops at the bulk cap.
        store.yield_between_bulk_batches().await;
    }
}

async fn write_new(
    state: &AppState,
    store: &braian_store::Store,
    session_id: Uuid,
    batch: braian_store::BatchId,
    actor: &Actor,
    rows: &[ImportRowOut],
    errors: &mut Vec<Value>,
) {
    let mut objects = Vec::new();
    let mut row_nos = Vec::new();
    for row in rows {
        match row
            .patch
            .as_ref()
            .and_then(|patch| serde_json::from_value::<Object>(patch.clone()).ok())
        {
            Some(object) => {
                objects.push(object);
                row_nos.push(row.row_no);
            }
            None => {
                errors.push(json!({ "row": row.row_no, "code": "corrupt", "message": "The prepared row could not be read." }));
                let _ = state.store.mark_import_row(session_id, row.row_no, "failed", &json!([{ "code": "corrupt", "message": "The prepared row could not be read." }])).await;
            }
        }
    }
    if objects.is_empty() {
        return;
    }
    if store.import_chunk(batch, actor, &objects).await.is_ok() {
        apply_imported_links(state, store, batch, actor, rows, errors).await;
        return;
    }
    for (object, row_no) in objects.into_iter().zip(row_nos) {
        if let Err(error) = store.import_chunk(batch, actor, &[object]).await {
            errors.push(json!({ "row": row_no, "message": error.to_string() }));
            let _ = state
                .store
                .mark_import_row(
                    session_id,
                    row_no,
                    "failed",
                    &json!([{ "code": "write_failed", "message": error.to_string() }]),
                )
                .await;
        }
    }
    apply_imported_links(state, store, batch, actor, rows, errors).await;
}

async fn write_ops(
    state: &AppState,
    store: &braian_store::Store,
    session_id: Uuid,
    batch: braian_store::BatchId,
    actor: &Actor,
    rows: &[ImportRowOut],
    errors: &mut Vec<Value>,
) {
    let mut ops = Vec::new();
    for row in rows {
        match op_for(row) {
            Some(op) => ops.push((row.row_no, op)),
            None if row
                .cells
                .get("links")
                .and_then(Value::as_array)
                .is_some_and(|links| !links.is_empty()) => {}
            None => {
                errors.push(json!({ "row": row.row_no, "code": "corrupt", "message": "The prepared row could not be read." }));
            }
        }
    }
    let plain: Vec<ObjectOp> = ops.iter().map(|(_, op)| op.clone()).collect();
    if !plain.is_empty() {
        match store.write_ops(batch, actor, plain).await {
            Ok(_) => {}
            Err(error) if is_conflict(&error) => {
                for (row_no, op) in ops {
                    if let Err(error) = store.write_ops(batch, actor, vec![op]).await {
                        let code = if is_conflict(&error) {
                            "stale"
                        } else {
                            "write_failed"
                        };
                        errors.push(
                            json!({ "row": row_no, "code": code, "message": error.to_string() }),
                        );
                        let _ = state
                            .store
                            .mark_import_row(
                                session_id,
                                row_no,
                                if code == "stale" { "stale" } else { "failed" },
                                &json!([{ "code": code, "message": error.to_string() }]),
                            )
                            .await;
                    }
                }
            }
            Err(error) => errors.push(json!({ "message": error.to_string() })),
        }
    }
    apply_imported_links(state, store, batch, actor, rows, errors).await;
}

fn op_for(row: &ImportRowOut) -> Option<ObjectOp> {
    let id = ObjectId::from_uuid(row.object_id?);
    let patch = serde_json::from_value::<ObjectPatch>(row.patch.clone()?).ok()?;
    let version = u64::try_from(row.base_version?).ok()?;
    Some(ObjectOp::Update {
        id,
        patch,
        base_version: version,
        read_version: version,
    })
}

fn is_conflict(error: &Error) -> bool {
    matches!(
        error,
        Error::Conflict { .. }
            | Error::StaleRead { .. }
            | Error::BaseAhead { .. }
            | Error::DuplicateKey { .. }
    )
}

fn counter(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

async fn cancelled(state: &AppState, id: Uuid) -> bool {
    state
        .store
        .import_session(id)
        .await
        .ok()
        .flatten()
        .is_some_and(|session| session.status == "cancelled")
}

#[allow(clippy::too_many_arguments)]
async fn finish(
    state: &AppState,
    store: &braian_store::Store,
    session_id: Uuid,
    job_id: Uuid,
    status: &str,
    done: i32,
    total: i32,
    log: &str,
    errors: &Value,
) {
    let _ = store
        .finish_job_run(job_id, status, done, log, errors)
        .await;
    emit_job(state, job_id, status, done, total);
    let _ = state
        .store
        .update_import_job(session_id, "failed", Some(job_id), &json!({}), Some(log))
        .await;
    let _ = state
        .store
        .close_import_proposal(session_id, "discarded", None)
        .await;
    if let Ok(Some(session)) = state.store.import_session(session_id).await {
        emit_import(state, &session);
    }
}

async fn apply_imported_links(
    state: &AppState,
    store: &braian_store::Store,
    batch: braian_store::BatchId,
    actor: &Actor,
    rows: &[ImportRowOut],
    errors: &mut Vec<Value>,
) {
    let Ok(cached) = state.models.get(&state.store).await else {
        return;
    };
    for row in rows {
        let Some(plans) = row.cells.get("links").and_then(Value::as_array) else {
            continue;
        };
        let Some(holder) = row.object_id else {
            continue;
        };
        let holder = ObjectId::from_uuid(holder);
        let Ok(existing) = store.links_for(holder).await else {
            continue;
        };
        let mut ops = Vec::new();
        for plan in plans {
            let Some(type_key) = plan.get("type").and_then(Value::as_str) else {
                continue;
            };
            let outgoing = plan
                .get("outgoing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(link_type) = cached
                .defs
                .types
                .iter()
                .find(|item| item.is_link() && item.key == type_key)
            else {
                continue;
            };
            let Some(spec) = link_type.link.as_ref() else {
                continue;
            };
            let peers: Vec<Uuid> = plan
                .get("peers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().and_then(|text| Uuid::parse_str(text).ok()))
                .collect();
            let one = if outgoing {
                spec.from_cardinality == braian_model::Cardinality::One
            } else {
                spec.to_cardinality == braian_model::Cardinality::One
            };
            let mine: Vec<_> = existing
                .iter()
                .filter(|link| link.link_type_id == link_type.id && link.outgoing == outgoing)
                .collect();
            let create = |peer: Uuid| {
                crate::links::link_create(
                    &cached,
                    &crate::links::CreateLink {
                        id: None,
                        link_type: type_key.to_owned(),
                        from_id: if outgoing { holder.as_uuid() } else { peer },
                        to_id: if outgoing { peer } else { holder.as_uuid() },
                        values: std::collections::BTreeMap::new(),
                    },
                )
            };
            if one {
                let Some(peer) = peers.first().copied() else {
                    continue;
                };
                if let Some(current) = mine.first() {
                    if current.peer_id.as_uuid() == peer {
                        continue;
                    }
                    let Ok(Some(stored)) = store.get_object(current.id).await else {
                        continue;
                    };
                    ops.push(ObjectOp::UpdateLink {
                        id: current.id,
                        patch: ObjectPatch {
                            values: braian_model::ValuePatch {
                                attributes: std::collections::BTreeMap::new(),
                            },
                            groups: std::collections::BTreeMap::new(),
                        },
                        base_version: stored.version,
                        read_version: stored.version,
                        from_id: if outgoing {
                            None
                        } else {
                            Some(ObjectId::from_uuid(peer))
                        },
                        to_id: if outgoing {
                            Some(ObjectId::from_uuid(peer))
                        } else {
                            None
                        },
                    });
                } else if let Ok(op) = create(peer) {
                    ops.push(op);
                }
            } else {
                for peer in peers {
                    if mine.iter().any(|link| link.peer_id.as_uuid() == peer) {
                        continue;
                    }
                    if let Ok(op) = create(peer) {
                        ops.push(op);
                    }
                }
            }
        }
        if ops.is_empty() {
            continue;
        }
        if let Err(error) = store.write_ops(batch, actor, ops).await {
            errors.push(json!({ "row": row.row_no, "message": error.to_string() }));
        }
    }
}
