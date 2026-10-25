//! Disk spool, parse/diff workers, and live progress events.

use std::collections::HashMap;
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
    let headers = HeaderInput::from_parsed(&outcome.headers, &outcome.sample);
    let sheet_name = outcome
        .sheets
        .iter()
        .find(|sheet| sheet.index == outcome.sheet_index)
        .map(|sheet| sheet.name.clone())
        .unwrap_or_else(|| "Sheet".into());
    let decider = decider_from_env();
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

fn decider_from_env() -> Option<HttpDecider> {
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
    let headers = HeaderInput::from_parsed(&headers, &sample);
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
    let type_key = mapping
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
    let targets = mapping::targets_for(&cached.defs, &type_key);
    let type_id = cached
        .model
        .type_id(&type_key)
        .ok_or_else(|| "That object type does not exist.".to_owned())?;
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
        )
        .await?;
    }
    let counters = counters.json(cursor.len() as u64);
    let current = state
        .store
        .update_import_diff_if_current(id, generation, "previewed", &counters, None)
        .await
        .map_err(|error| error.to_string())?;
    if !current {
        return Err(DiffError::Superseded);
    }
    if let Ok(Some(session)) = state.store.import_session(id).await {
        emit_import(state, &session);
    }
    Ok(())
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
) -> Result<(), DiffError> {
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
    let refs = preload_refs(state, model, targets, columns, buffer).await?;
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
        );
        match classified.outcome.status.as_str() {
            "new" => counters.new += 1,
            "changed" => counters.changed += 1,
            "delete" => counters.delete += 1,
            "failed" => counters.failed += 1,
            "unchanged" => {
                counters.unchanged += 1;
                continue;
            }
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

async fn preload_refs(
    state: &AppState,
    model: &braian_rules::CompiledModel,
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
            Some(MappedColumn {
                index: column.get("col")?.as_u64()? as usize,
                target: column.get("target")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

#[derive(Default)]
struct Counter {
    new: u64,
    changed: u64,
    delete: u64,
    failed: u64,
    unchanged: u64,
    processed: u64,
}

impl Counter {
    fn json(&self, total: u64) -> Value {
        json!({
            "new": self.new,
            "changed": self.changed,
            "delete": self.delete,
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
