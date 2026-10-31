//! Disk spool, parse/diff workers, and live progress events.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use braian_model::AttributeKind;
use braian_store::ImportSession;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::AppState;

use super::decisions::{self, HttpDecider};
use super::diff::{self, MappedColumn, classify_row};
use super::entity;
use super::mapping::{self, HeaderInput, ManualLine, Proposal};
use super::parse::{self, FileKind, HeaderCol, ParseOutcome, RowCursor};

pub fn import_dir() -> PathBuf {
    std::env::var("BRAIAN_IMPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/imports"))
}

pub fn session_dir(id: Uuid) -> PathBuf {
    import_dir().join(id.to_string())
}

pub fn emit_import(state: &AppState, session: &ImportSession) {
    state.emit(json!({
        "op": "import",
        "id": session.id,
        "owner": session.owner,
        "status": session.status,
        "counters": session.counters,
        "rows_parsed": session.rows_parsed,
        "parse_done": session.parse_done,
    }));
}

pub fn spawn_janitor(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60 * 60));
        loop {
            tick.tick().await;
            let Ok(ids) = state.store.purge_import_sessions().await else {
                continue;
            };
            for id in ids {
                let _ = std::fs::remove_dir_all(session_dir(id));
            }
        }
    });
}

struct PrepareGate {
    lock: tokio::sync::Mutex<()>,
    sheet_selected: AtomicBool,
}

fn prepare_gate(id: Uuid) -> Arc<PrepareGate> {
    static GATES: LazyLock<Mutex<HashMap<Uuid, Arc<PrepareGate>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut gates = GATES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    gates
        .entry(id)
        .or_insert_with(|| {
            Arc::new(PrepareGate {
                lock: tokio::sync::Mutex::new(()),
                sheet_selected: AtomicBool::new(false),
            })
        })
        .clone()
}

pub fn spawn_prepare(state: AppState, id: Uuid, kind: FileKind, sheet: Option<usize>) {
    tokio::spawn(async move {
        let gate = prepare_gate(id);
        let _guard = gate.lock.lock().await;
        if gate.sheet_selected.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = prepare(&state, id, kind, sheet).await {
            let _ = state
                .store
                .update_import_upload(id, 0, "failed", Some(&error))
                .await;
            if let Ok(Some(session)) = state.store.import_session(id).await {
                emit_import(&state, &session);
            }
        }
    });
}

/// Re-parses one sheet and waits until mapping has finished.
///
/// The lock is the same one the upload task holds, so a switch that arrives
/// during the first parse runs after that parse and then replaces it.
pub async fn switch_sheet(
    state: &AppState,
    id: Uuid,
    kind: FileKind,
    sheet_index: usize,
    file_bytes: i64,
) -> Result<(), String> {
    let gate = prepare_gate(id);
    gate.sheet_selected.store(true, Ordering::Release);
    let _guard = gate.lock.lock().await;
    let dir = session_dir(id);
    let _ = std::fs::remove_file(dir.join("rows.bin"));
    let _ = std::fs::remove_file(dir.join("rows.idx"));
    let _ = std::fs::remove_file(dir.join("rows.nos"));
    state
        .store
        .delete_import_rows(id)
        .await
        .map_err(|error| error.to_string())?;
    state
        .store
        .update_import_upload(id, file_bytes, "mapping", None)
        .await
        .map_err(|error| error.to_string())?;
    prepare(state, id, kind, Some(sheet_index)).await
}

async fn prepare(
    state: &AppState,
    id: Uuid,
    kind: FileKind,
    sheet: Option<usize>,
) -> Result<(), String> {
    let Some(session) = state
        .store
        .import_session(id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let path = session_dir(id).join(format!("source.{}", kind.extension()));
    let spool = session_dir(id);
    let outcome =
        tokio::task::spawn_blocking(move || parse::parse_upload(&path, kind, sheet, &spool))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.message)?;
    let _ = state
        .store
        .update_import_parsed(
            id,
            &sheets_json(&outcome),
            outcome.sheet_index as i32,
            outcome.dialect.as_ref().map(dialect_json).as_ref(),
            &headers_json(&outcome.headers),
            &serde_json::to_value(&outcome.sample).unwrap_or(Value::Null),
            outcome.data_rows as i64,
            true,
            "mapping",
            None,
        )
        .await;
    if let Ok(Some(updated)) = state.store.import_session(id).await {
        emit_import(state, &updated);
    }
    let cached = state
        .models
        .get(&state.store)
        .await
        .map_err(|problem| problem.detail)?;
    let mut headers = HeaderInput::from_parsed(&outcome.headers, &outcome.sample);
    hide_samples_for_ai(&mut headers);
    let sheet_name = outcome
        .sheets
        .iter()
        .find(|sheet| sheet.index == outcome.sheet_index)
        .map(|sheet| sheet.name.clone())
        .unwrap_or_else(|| "Sheet".into());
    if let Some(reuse) = super::saved::find_for(state, &headers_json(&outcome.headers)).await {
        let (mut proposal, usage) = mapping::propose::<HttpDecider>(
            None,
            &cached.defs,
            &headers,
            &session.file_name,
            &sheet_name,
            session.owner,
            Some(&reuse.type_key),
            reuse.id_column,
            &reuse.lines,
        )
        .await;
        proposal.ai = format!("saved: {}", reuse.name);
        let proposal_json = serde_json::to_value(&proposal).map_err(|error| error.to_string())?;
        state
            .store
            .update_import_proposal(id, &proposal_json, &usage, "ready")
            .await
            .map_err(|error| error.to_string())?;
        if let Ok(Some(updated)) = state.store.import_session(id).await {
            emit_import(state, &updated);
        }
        return Ok(());
    }
    let decider = decider_from_env();
    if decider.is_some() {
        // Rules first: the user sees a usable mapping at once while the AI works.
        let (mut quick, usage) = mapping::propose::<HttpDecider>(
            None,
            &cached.defs,
            &headers,
            &session.file_name,
            &sheet_name,
            session.owner,
            None,
            None,
            &[],
        )
        .await;
        quick.ai = "pending".into();
        let quick_json = serde_json::to_value(&quick).map_err(|error| error.to_string())?;
        state
            .store
            .update_import_proposal(id, &quick_json, &usage, "mapping")
            .await
            .map_err(|error| error.to_string())?;
        if let Ok(Some(updated)) = state.store.import_session(id).await {
            emit_import(state, &updated);
        }
    }
    let (proposal, usage) = mapping::propose(
        decider.as_ref(),
        &cached.defs,
        &headers,
        &session.file_name,
        &sheet_name,
        session.owner,
        None,
        None,
        &[],
    )
    .await;
    let proposal_json = serde_json::to_value(&proposal).map_err(|error| error.to_string())?;
    state
        .store
        .update_import_proposal(id, &proposal_json, &usage, "ready")
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(Some(updated)) = state.store.import_session(id).await {
        emit_import(state, &updated);
    }
    Ok(())
}

/// In headers mode no cell values leave the server, so they are not read at all.
fn hide_samples_for_ai(headers: &mut [HeaderInput]) {
    if decisions::AiMode::from_env() == decisions::AiMode::Headers {
        for header in headers {
            header.samples.clear();
        }
    }
}

fn decider_from_env() -> Option<HttpDecider> {
    if decisions::AiMode::from_env() == decisions::AiMode::Off {
        return None;
    }
    let key = decisions::openai_key()?;
    Some(HttpDecider::new(
        decisions::openai_base(),
        key,
        decisions::DecisionPolicy::default(),
    ))
}

pub async fn remap(
    state: &AppState,
    session: &ImportSession,
    type_key: &str,
    id_column: Option<Option<usize>>,
    manual: &[ManualLine],
) -> Result<Proposal, String> {
    let cached = state
        .models
        .get(&state.store)
        .await
        .map_err(|problem| problem.detail)?;
    let headers: Vec<HeaderCol> =
        serde_json::from_value(session.headers.clone()).unwrap_or_default();
    let sample: Vec<Vec<String>> =
        serde_json::from_value(session.sample.clone()).unwrap_or_default();
    let mut headers = HeaderInput::from_parsed(&headers, &sample);
    hide_samples_for_ai(&mut headers);
    let sheet = session
        .sheets
        .as_array()
        .and_then(|sheets| {
            sheets.iter().find(|sheet| {
                sheet.get("index").and_then(Value::as_i64) == Some(i64::from(session.sheet_index))
            })
        })
        .and_then(|sheet| sheet.get("name").and_then(Value::as_str))
        .unwrap_or("Sheet");
    let decider = decider_from_env();
    let (proposal, usage) = mapping::propose(
        decider.as_ref(),
        &cached.defs,
        &headers,
        &session.file_name,
        sheet,
        session.owner,
        Some(type_key),
        id_column,
        manual,
    )
    .await;
    let proposal_json = serde_json::to_value(&proposal).map_err(|error| error.to_string())?;
    state
        .store
        .update_import_proposal(session.id, &proposal_json, &usage, "ready")
        .await
        .map_err(|error| error.to_string())?;
    Ok(proposal)
}

enum DiffError {
    Superseded,
    Failed(String),
}

impl From<String> for DiffError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

pub fn spawn_diff(state: AppState, id: Uuid, generation: i64) {
    tokio::spawn(async move {
        match diff_session(&state, id, generation).await {
            Ok(()) => {}
            Err(DiffError::Superseded) => {}
            Err(DiffError::Failed(error)) => {
                let failed = state
                    .store
                    .fail_import_diff_if_current(id, generation, &error)
                    .await
                    .unwrap_or(false);
                if failed && let Ok(Some(session)) = state.store.import_session(id).await {
                    emit_import(&state, &session);
                }
            }
        }
    });
}

async fn diff_session(state: &AppState, id: Uuid, generation: i64) -> Result<(), DiffError> {
    let Some(session) = state
        .store
        .import_session(id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let mapping = session
        .mapping
        .clone()
        .ok_or_else(|| "Choose a mapping first.".to_owned())?;
    let type_token = mapping
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Choose an object type.".to_owned())?
        .to_owned();
    let id_column = mapping
        .get("id_column")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let columns = mapped_columns(&mapping);
    if !state
        .store
        .import_diff_current(id, generation)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err(DiffError::Superseded);
    }
    let cached = state
        .models
        .get(&state.store)
        .await
        .map_err(|problem| problem.detail)?;
    if entity::is_model(&type_token) {
        return diff_model_session(
            state,
            id,
            generation,
            &session,
            &cached.defs,
            id_column,
            &columns,
        )
        .await;
    }
    let targets = mapping::targets_for(&cached.defs, &type_token);
    let object_type = cached
        .defs
        .types
        .iter()
        .find(|item| {
            item.key == type_token
                || item
                    .id
                    .as_uuid()
                    .to_string()
                    .eq_ignore_ascii_case(&type_token)
        })
        .ok_or_else(|| "That object type does not exist.".to_owned())?;
    // The saved mapping stores the type id. Classification looks the type up by key.
    let type_id = object_type.id;
    let type_key = object_type.key.clone();
    let overrides = state
        .store
        .import_overrides(id)
        .await
        .map_err(|error| error.to_string())?;
    let mut cursor = RowCursor::open(&session_dir(id)).map_err(|error| error.message)?;
    let mut seen = std::collections::HashMap::new();
    let mut counters = Counter::default();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut buffer = Vec::new();
    loop {
        let next = cursor.next_row().map_err(|error| error.message)?;
        let Some((row_no, cells)) = next else { break };
        buffer.push((row_no, cells));
        if buffer.len() >= diff::DIFF_CHUNK {
            flush_chunk(
                state,
                id,
                generation,
                &cached.model,
                &cached.defs,
                &targets,
                &type_key,
                type_id,
                &columns,
                id_column,
                &mut buffer,
                &mut seen,
                &mut counters,
                &mut last_emit,
                &overrides,
            )
            .await?;
        }
    }
    if !buffer.is_empty() {
        flush_chunk(
            state,
            id,
            generation,
            &cached.model,
            &cached.defs,
            &targets,
            &type_key,
            type_id,
            &columns,
            id_column,
            &mut buffer,
            &mut seen,
            &mut counters,
            &mut last_emit,
            &overrides,
        )
        .await?;
    }
    let counters = counters.json(cursor.len() as u64);
    finish_preview(state, id, generation, &session, counters).await
}

async fn finish_preview(
    state: &AppState,
    id: Uuid,
    generation: i64,
    session: &ImportSession,
    counters: Value,
) -> Result<(), DiffError> {
    let current = state
        .store
        .update_import_diff_if_current(id, generation, "previewed", &counters, None)
        .await
        .map_err(|error| error.to_string())?;
    if !current {
        return Err(DiffError::Superseded);
    }
    // The draft exists as soon as the preview is ready. It stays in the staging
    // area until someone approves or discards it.
    let sheet = session
        .sheets
        .as_array()
        .and_then(|sheets| {
            sheets.iter().find(|sheet| {
                sheet.get("index").and_then(Value::as_i64) == Some(i64::from(session.sheet_index))
            })
        })
        .and_then(|sheet| sheet.get("name").and_then(Value::as_str))
        .filter(|name| session.file_kind != "csv" && !name.is_empty());
    let title = match sheet {
        Some(name) => format!("Import {} ({name})", session.file_name),
        None => format!("Import {}", session.file_name),
    };
    state
        .store
        .upsert_import_proposal(id, &title, &counters)
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(Some(session)) = state.store.import_session(id).await {
        emit_import(state, &session);
    }
    state.emit(json!({ "op": "proposal", "session": id }));
    Ok(())
}

/// Diff of a data model sheet: the rows become model changes. Model sheets are small, so the
/// rows are held in memory and written in one go.
async fn diff_model_session(
    state: &AppState,
    id: Uuid,
    generation: i64,
    session: &ImportSession,
    defs: &braian_model::DefinitionSet,
    id_column: Option<usize>,
    columns: &[MappedColumn],
) -> Result<(), DiffError> {
    let overrides = state
        .store
        .import_overrides(id)
        .await
        .map_err(|error| error.to_string())?;
    let mut cursor = RowCursor::open(&session_dir(id)).map_err(|error| error.message)?;
    let mut counters = Counter::default();
    let mut rows = Vec::new();
    while let Some((row_no, mut cells)) = cursor.next_row().map_err(|error| error.message)? {
        apply_overrides(&overrides, row_no, &mut cells);
        let outcome = entity::classify_attribute(defs, columns, id_column, &cells);
        match outcome.status.as_str() {
            "new" => counters.new += 1,
            "changed" => counters.changed += 1,
            "failed" => counters.failed += 1,
            _ => counters.unchanged += 1,
        }
        rows.push(braian_store::ImportRowIn {
            session_id: id,
            row_no: row_no as i32,
            status: outcome.status,
            object_id: None,
            object_key: outcome.object_key,
            base_version: None,
            cells: outcome.cells,
            patch: outcome.patch,
            errors: outcome.errors,
        });
    }
    counters.processed = rows.len() as u64;
    let kept = state
        .store
        .copy_import_rows(id, generation, &rows)
        .await
        .map_err(|error| error.to_string())?;
    if !kept {
        return Err(DiffError::Superseded);
    }
    finish_preview(
        state,
        id,
        generation,
        session,
        counters.json(cursor.len() as u64),
    )
    .await
}
#[allow(clippy::too_many_arguments)]
async fn flush_chunk(
    state: &AppState,
    id: Uuid,
    generation: i64,
    model: &braian_rules::CompiledModel,
    defs: &braian_model::DefinitionSet,
    targets: &[mapping::Target],
    type_key: &str,
    type_id: braian_model::TypeId,
    columns: &[MappedColumn],
    id_column: Option<usize>,
    buffer: &mut Vec<(u32, Vec<Option<String>>)>,
    seen: &mut std::collections::HashMap<String, u32>,
    counters: &mut Counter,
    last_emit: &mut Instant,
    overrides: &Value,
) -> Result<(), DiffError> {
    for (row_no, cells) in buffer.iter_mut() {
        apply_overrides(overrides, *row_no, cells);
    }
    if !state
        .store
        .import_diff_current(id, generation)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err(DiffError::Superseded);
    }
    let keys: Vec<String> = buffer
        .iter()
        .filter_map(|(_, cells)| {
            id_column.and_then(|index| {
                cells
                    .get(index)
                    .and_then(|cell| cell.clone())
                    .map(|text| text.trim().to_owned())
            })
        })
        .filter(|key| !key.is_empty())
        .collect();
    let existing = state
        .store
        .get_by_keys(type_id, &keys)
        .await
        .map_err(|error| error.to_string())?;
    let ids: Vec<Uuid> = existing
        .values()
        .map(|item| item.object.id.as_uuid())
        .collect();
    let peer_index = state
        .store
        .link_peer_index(&ids)
        .await
        .map_err(|error| error.to_string())?;
    let refs = preload_refs(state, model, defs, targets, columns, buffer).await?;
    let mut rows = Vec::new();
    for (row_no, cells) in buffer.drain(..) {
        let key = id_column.and_then(|index| {
            cells
                .get(index)
                .and_then(|cell| cell.clone())
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        });
        let duplicate = key
            .as_ref()
            .and_then(|key| diff::seen_key(seen, key, row_no));
        let found = key.as_ref().and_then(|key| existing.get(key));
        let mut resolve = |ref_type: &str, ref_key: &str| {
            refs.get(&(ref_type.to_owned(), ref_key.to_owned()))
                .copied()
        };
        let classified = classify_row(
            model,
            defs,
            targets,
            type_key,
            columns,
            id_column,
            row_no,
            &cells,
            found,
            duplicate,
            &mut resolve,
            &current_link_sets(defs, found, &peer_index),
        );
        match classified.outcome.status.as_str() {
            "new" => counters.new += 1,
            "changed" => counters.changed += 1,
            "failed" => counters.failed += 1,
            "unchanged" => counters.unchanged += 1,
            _ => {}
        }
        rows.push(braian_store::ImportRowIn {
            session_id: id,
            row_no: row_no as i32,
            status: classified.outcome.status,
            object_id: classified.outcome.object_id,
            object_key: classified.outcome.object_key,
            base_version: classified.outcome.base_version,
            cells: classified.outcome.cells,
            patch: classified.outcome.patch,
            errors: classified.outcome.errors,
        });
    }
    let kept = state
        .store
        .copy_import_rows(id, generation, &rows)
        .await
        .map_err(|error| error.to_string())?;
    if !kept {
        return Err(DiffError::Superseded);
    }
    counters.processed += rows.len() as u64;
    if last_emit.elapsed() >= Duration::from_millis(250) {
        let value = counters.json(0);
        let current = state
            .store
            .update_import_diff_if_current(id, generation, "diffing", &value, None)
            .await
            .map_err(|error| error.to_string())?;
        if !current {
            return Err(DiffError::Superseded);
        }
        if let Ok(Some(session)) = state.store.import_session(id).await {
            emit_import(state, &session);
        }
        *last_emit = Instant::now();
    }
    Ok(())
}

fn current_link_sets(
    defs: &braian_model::DefinitionSet,
    found: Option<&braian_store::StoredObject>,
    peer_index: &HashMap<Uuid, BTreeMap<(Uuid, bool), BTreeSet<String>>>,
) -> BTreeMap<(String, bool), BTreeSet<String>> {
    let Some(stored) = found else {
        return BTreeMap::new();
    };
    let Some(sides) = peer_index.get(&stored.object.id.as_uuid()) else {
        return BTreeMap::new();
    };
    let mut sets = BTreeMap::new();
    for ((type_id, outgoing), keys) in sides {
        let type_key = defs
            .types
            .iter()
            .find(|item| item.id.as_uuid() == *type_id)
            .map(|item| item.key.clone())
            .unwrap_or_default();
        sets.insert((type_key, *outgoing), keys.clone());
    }
    sets
}

async fn preload_refs(
    state: &AppState,
    model: &braian_rules::CompiledModel,
    defs: &braian_model::DefinitionSet,
    targets: &[mapping::Target],
    columns: &[MappedColumn],
    buffer: &[(u32, Vec<Option<String>>)],
) -> Result<std::collections::HashMap<(String, String), Uuid>, String> {
    let mut wanted: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (_, cells) in buffer {
        for column in columns {
            let Some(target) = mapping::find_target(targets, &column.target) else {
                continue;
            };
            if target.kind == "link" {
                let Some(rest) = target.id.strip_prefix("link:") else {
                    continue;
                };
                let Some((type_key, direction)) = rest.rsplit_once(':') else {
                    continue;
                };
                let Some(link) = defs
                    .types
                    .iter()
                    .find(|item| item.is_link() && item.key == type_key)
                else {
                    continue;
                };
                let Some(spec) = link.link.as_ref() else {
                    continue;
                };
                let opposite = if direction == "out" {
                    &spec.to_types
                } else {
                    &spec.from_types
                };
                let Some(text) = cells.get(column.index).and_then(|cell| cell.clone()) else {
                    continue;
                };
                for token in text.split(',') {
                    let token = token.trim();
                    if token.is_empty() || Uuid::parse_str(token).is_ok() {
                        continue;
                    }
                    for type_id in opposite {
                        let Some(object_type) = defs.types.iter().find(|item| item.id == *type_id)
                        else {
                            continue;
                        };
                        wanted
                            .entry(object_type.key.clone())
                            .or_default()
                            .push(token.to_owned());
                    }
                }
                continue;
            }
            let Some(attribute_key) = target.attribute_key.as_deref() else {
                continue;
            };
            let Some(attribute) = model.attribute_by_key(attribute_key) else {
                continue;
            };
            let AttributeKind::ObjectRef {
                type_key: Some(ref_type),
            } = &attribute.kind
            else {
                continue;
            };
            let Some(text) = cells.get(column.index).and_then(|cell| cell.clone()) else {
                continue;
            };
            if Uuid::parse_str(text.trim()).is_ok() {
                continue;
            }
            wanted
                .entry(ref_type.clone())
                .or_default()
                .push(text.trim().to_owned());
        }
    }
    let mut resolved = std::collections::HashMap::new();
    for (ref_type, keys) in wanted {
        let Some(type_id) = model.type_id(&ref_type) else {
            continue;
        };
        let found = state
            .store
            .get_by_keys(type_id, &keys)
            .await
            .map_err(|error| error.to_string())?;
        for (key, stored) in found {
            resolved.insert((ref_type.clone(), key), stored.object.id.as_uuid());
        }
    }
    Ok(resolved)
}

fn mapped_columns(mapping: &Value) -> Vec<MappedColumn> {
    mapping
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|column| {
            let target = column.get("target")?.as_str()?;
            // Old mappings may carry the removed action column.
            if target == "action" {
                return None;
            }
            Some(MappedColumn {
                index: column.get("col")?.as_u64()? as usize,
                target: target.to_owned(),
                clear_empty: mapping
                    .get("empty_clears")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                skip_invalid: mapping
                    .get("skip_invalid")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

#[derive(Default)]
struct Counter {
    new: u64,
    changed: u64,
    failed: u64,
    unchanged: u64,
    processed: u64,
}

impl Counter {
    fn json(&self, total: u64) -> Value {
        json!({
            "new": self.new,
            "changed": self.changed,
            "stale": 0,
            "excluded": 0,
            "failed": self.failed,
            "unchanged": self.unchanged,
            "processed": self.processed,
            "total": total,
        })
    }
}

fn headers_json(headers: &[HeaderCol]) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|header| {
                json!({
                    "index": header.index,
                    "raw": header.raw,
                    "name": header.name,
                    "empty_ratio": header.empty_ratio,
                    "duplicate_of": header.duplicate_of,
                })
            })
            .collect(),
    )
}

fn sheets_json(outcome: &ParseOutcome) -> Value {
    Value::Array(
        outcome
            .sheets
            .iter()
            .map(|sheet| {
                json!({
                    "index": sheet.index,
                    "name": sheet.name,
                    "hidden": sheet.hidden,
                    "empty": sheet.empty,
                    "rows_estimate": sheet.rows_estimate,
                })
            })
            .collect(),
    )
}

fn dialect_json(dialect: &parse::CsvDialect) -> Value {
    json!({
        "delimiter": dialect.delimiter,
        "quote": dialect.quote,
        "encoding": dialect.encoding,
        "bom": dialect.bom,
    })
}

pub fn manual_from_proposal(proposal: &Value) -> Vec<ManualLine> {
    proposal
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|column| column.get("source").and_then(Value::as_str) == Some("manual"))
        .filter_map(|column| {
            Some(ManualLine {
                col: column.get("col")?.as_u64()? as usize,
                target: column.get("target")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

fn apply_overrides(overrides: &Value, row_no: u32, cells: &mut Vec<Option<String>>) {
    let Some(edits) = overrides.get(row_no.to_string()).and_then(Value::as_object) else {
        return;
    };
    for (col, value) in edits {
        let Ok(index) = col.parse::<usize>() else {
            continue;
        };
        if index >= cells.len() {
            cells.resize(index + 1, None);
        }
        cells[index] = value
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
    }
}

/// Edits one cell of a draft row, checks the row again and updates the counters.
pub async fn edit_cell(
    state: &AppState,
    id: Uuid,
    row_no: u32,
    col: usize,
    value: &str,
) -> Result<(Option<braian_store::ImportRowOut>, Value), String> {
    let session = state
        .store
        .import_session(id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That import does not exist.".to_owned())?;
    if session.status != "previewed" {
        return Err("The draft cannot be edited right now.".into());
    }
    let before = state
        .store
        .import_row(id, row_no as i32)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "That row is not part of the draft.".to_owned())?;
    let mapping = session
        .mapping
        .clone()
        .ok_or_else(|| "Choose a mapping first.".to_owned())?;
    let type_token = mapping
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Choose an object type.".to_owned())?
        .to_owned();
    let id_column = mapping
        .get("id_column")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let columns = mapped_columns(&mapping);
    let cached = state
        .models
        .get(&state.store)
        .await
        .map_err(|problem| problem.detail)?;
    if entity::is_model(&type_token) {
        let row_overrides = state
            .store
            .set_import_override(id, row_no as i32, col as i32, value)
            .await
            .map_err(|error| error.to_string())?;
        let mut cursor = RowCursor::open(&session_dir(id)).map_err(|error| error.message)?;
        let index = (0..cursor.len())
            .find(|index| cursor.row_no(*index) == Some(row_no))
            .ok_or_else(|| "That row is not in the file.".to_owned())?;
        let (_, mut cells) = cursor.read_at(index).map_err(|error| error.message)?;
        apply_overrides(
            &json!({ row_no.to_string(): row_overrides }),
            row_no,
            &mut cells,
        );
        let outcome = entity::classify_attribute(&cached.defs, &columns, id_column, &cells);
        return store_edited(state, id, row_no, &session, &before, outcome).await;
    }
    let targets = mapping::targets_for(&cached.defs, &type_token);
    let object_type = cached
        .defs
        .types
        .iter()
        .find(|item| {
            item.key == type_token
                || item
                    .id
                    .as_uuid()
                    .to_string()
                    .eq_ignore_ascii_case(&type_token)
        })
        .ok_or_else(|| "That object type does not exist.".to_owned())?;
    let type_id = object_type.id;
    let type_key = object_type.key.clone();
    let row_overrides = state
        .store
        .set_import_override(id, row_no as i32, col as i32, value)
        .await
        .map_err(|error| error.to_string())?;
    let mut cursor = RowCursor::open(&session_dir(id)).map_err(|error| error.message)?;
    let index = (0..cursor.len())
        .find(|index| cursor.row_no(*index) == Some(row_no))
        .ok_or_else(|| "That row is not in the file.".to_owned())?;
    let (_, mut cells) = cursor.read_at(index).map_err(|error| error.message)?;
    apply_overrides(
        &json!({ row_no.to_string(): row_overrides }),
        row_no,
        &mut cells,
    );
    let key = id_column.and_then(|position| {
        cells
            .get(position)
            .and_then(|cell| cell.clone())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    });
    let keys: Vec<String> = key.iter().cloned().collect();
    let existing = state
        .store
        .get_by_keys(type_id, &keys)
        .await
        .map_err(|error| error.to_string())?;
    let ids: Vec<Uuid> = existing
        .values()
        .map(|item| item.object.id.as_uuid())
        .collect();
    let peer_index = state
        .store
        .link_peer_index(&ids)
        .await
        .map_err(|error| error.to_string())?;
    let buffer = vec![(row_no, cells.clone())];
    let refs = preload_refs(
        state,
        &cached.model,
        &cached.defs,
        &targets,
        &columns,
        &buffer,
    )
    .await
    .map_err(|error| error.to_string())?;
    let found = key.as_ref().and_then(|key| existing.get(key));
    let mut resolve = |ref_type: &str, ref_key: &str| {
        refs.get(&(ref_type.to_owned(), ref_key.to_owned()))
            .copied()
    };
    let classified = classify_row(
        &cached.model,
        &cached.defs,
        &targets,
        &type_key,
        &columns,
        id_column,
        row_no,
        &cells,
        found,
        None,
        &mut resolve,
        &current_link_sets(&cached.defs, found, &peer_index),
    );
    store_edited(state, id, row_no, &session, &before, classified.outcome).await
}

async fn store_edited(
    state: &AppState,
    id: Uuid,
    row_no: u32,
    session: &ImportSession,
    _before: &braian_store::ImportRowOut,
    outcome: diff::RowOutcome,
) -> Result<(Option<braian_store::ImportRowOut>, Value), String> {
    let status = outcome.status.clone();
    let row = braian_store::ImportRowIn {
        session_id: id,
        row_no: row_no as i32,
        status: status.clone(),
        object_id: outcome.object_id,
        object_key: outcome.object_key,
        base_version: outcome.base_version,
        cells: outcome.cells,
        patch: outcome.patch,
        errors: outcome.errors,
    };
    state
        .store
        .replace_import_row(id, row_no as i32, Some(&row))
        .await
        .map_err(|error| error.to_string())?;
    let counts = state
        .store
        .import_status_counts(id)
        .await
        .map_err(|error| error.to_string())?;
    let mut counters = session.counters.clone();
    if let Some(map) = counters.as_object_mut() {
        for name in ["new", "changed", "failed", "excluded", "stale", "unchanged"] {
            map.insert(name.into(), json!(counts.get(name).copied().unwrap_or(0)));
        }
    }
    state
        .store
        .set_import_counters(id, &counters)
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(Some(updated)) = state.store.import_session(id).await {
        emit_import(state, &updated);
    }
    state.emit(json!({ "op": "proposal", "session": id }));
    let stored = state
        .store
        .import_row(id, row_no as i32)
        .await
        .map_err(|error| error.to_string())?;
    Ok((stored, counters))
}
