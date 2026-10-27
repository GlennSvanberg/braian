//! Excel export of the rows the caller is looking at.
//!
//! Postgres is read in chunks on the interactive pool when the file is streamed
//! straight back, and on the bulk pool when the export is a background job.
//! The workbook is a real Excel table: header row, style, autofilter and a
//! frozen header. Column widths come from the header and the first
//! [`WIDTH_SAMPLE`] rows. Cell values keep numbers, dates and booleans, and
//! localized fields use the requested data language only.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use braian_model::{Attribute, AttributeId, AttributeKind, DefinitionSet, LanguageTag};
use braian_rules::CompiledModel;
use braian_store::{Error, ExportObject, ExportOrder, ExportScan, Store};
use futures::stream;
use rust_xlsxwriter::{
    ExcelDateTime, Format, Table, TableColumn, TableStyle, Workbook, Worksheet, XlsxError,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::auth::{Auth, Identity, Role};
use crate::platform::{emit_job, require};
use crate::problem::{Problem, store_status};
use crate::state::AppState;

/// Above this, the export is a job instead of holding the HTTP request open.
const INLINE_ROW_LIMIT: i64 = 10_000;
/// Excel's last data row. Row 1 is the header.
const EXCEL_DATA_ROWS: i64 = 1_048_575;
const MAX_COLUMNS: usize = 512;
/// Header plus this many leading rows decide the column widths.
const WIDTH_SAMPLE: usize = 400;
const MIN_WIDTH: f64 = 8.0;
const MAX_WIDTH: f64 = 48.0;
const WIDTH_PAD: f64 = 2.0;
const EXCEL_TEXT: usize = 32_767;
const XLSX_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const JOB_TYPE: &str = "export.xlsx";

#[derive(Debug)]
struct ExportError {
    message: String,
}

impl ExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportError {}

impl From<XlsxError> for ExportError {
    fn from(error: XlsxError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<std::io::Error> for ExportError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<zip::result::ZipError> for ExportError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Blank,
    Text(String),
    Number(f64),
    Boolean(bool),
    Date { year: u16, month: u8, day: u8 },
}

#[derive(Clone, Copy)]
enum ValueShape {
    Text,
    Number,
    Boolean,
    Date,
    Range,
    List,
}

#[derive(Clone)]
struct AttrColumn {
    id: AttributeId,
    shape: ValueShape,
    multiple: bool,
    language_dependent: bool,
    labels: BTreeMap<String, String>,
}

#[derive(Clone)]
enum ColumnKind {
    Key,
    Type,
    Version,
    Blank,
    Attribute(AttrColumn),
}

#[derive(Clone)]
struct ResolvedColumn {
    header: String,
    kind: ColumnKind,
}

struct Prepared {
    scan: ExportScan,
    columns: Arc<Vec<ResolvedColumn>>,
    types: Arc<HashMap<Uuid, String>>,
    lang: String,
    total: i64,
}

struct Progress {
    state: AppState,
    id: Uuid,
    total: i32,
    done: AtomicI64,
}

impl Progress {
    async fn tick(&self, rows: i64) {
        let done = self.done.fetch_add(rows, Ordering::SeqCst) + rows;
        let done_i = i32::try_from(done).unwrap_or(i32::MAX);
        let note = format!("{done_i} rows");
        let _ = self
            .state
            .store
            .update_job_progress(self.id, done_i, &note, &json!([]))
            .await;
        emit_job(&self.state, self.id, "running", done_i, self.total);
    }
}

#[derive(Deserialize)]
pub(crate) struct ExportBody {
    #[serde(default)]
    columns: Vec<ColumnIn>,
    q: Option<String>,
    lang: Option<String>,
    filter: Option<String>,
    sort: Option<String>,
    ids: Option<Vec<String>>,
    #[serde(rename = "type")]
    r#type: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum ColumnIn {
    Name(String),
    Spec {
        id: String,
        #[serde(default)]
        header: Option<String>,
        #[serde(default)]
        attribute: Option<String>,
    },
}

impl ColumnIn {
    fn parts(&self) -> (&str, Option<&str>, Option<&str>) {
        match self {
            Self::Name(id) => (id, None, None),
            Self::Spec {
                id,
                header,
                attribute,
            } => (id, header.as_deref(), attribute.as_deref()),
        }
    }
}

struct ChunkRows {
    rx: mpsc::Receiver<Vec<Vec<Cell>>>,
    pending: std::vec::IntoIter<Vec<Cell>>,
}

impl Iterator for ChunkRows {
    type Item = Vec<Cell>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(row) = self.pending.next() {
                return Some(row);
            }
            self.pending = self.rx.blocking_recv()?.into_iter();
        }
    }
}

pub(crate) async fn export_xlsx(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<ExportBody>,
) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    let prepared = prepare(&state, &body).await?;
    if prepared.total > EXCEL_DATA_ROWS {
        return Err(Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_too_large",
            format!("an export holds at most {EXCEL_DATA_ROWS} data rows"),
        ));
    }
    if prepared.total > INLINE_ROW_LIMIT {
        return start_job(state, identity, prepared).await;
    }
    std::fs::create_dir_all(export_dir()).map_err(io_problem)?;
    let path = export_dir().join(format!("inline-{}.xlsx", Uuid::now_v7()));
    export_to_file(&state.store, &prepared, &path, None)
        .await
        .map_err(export_problem)?;
    file_response(path, true)
}

pub(crate) async fn download_export(
    State(state): State<AppState>,
    Auth(identity): Auth,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Response, Problem> {
    require(&identity, Role::Viewer)?;
    let Some(job) = state.store.job_run(id).await.map_err(store_status)? else {
        return Err(Problem::new(StatusCode::NOT_FOUND, "not_found", "export"));
    };
    if job.get("type_key").and_then(Value::as_str) != Some(JOB_TYPE) {
        return Err(Problem::new(StatusCode::NOT_FOUND, "not_found", "export"));
    }
    match job.get("status").and_then(Value::as_str) {
        Some("succeeded") => {}
        Some("failed") => {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "export_failed",
                "export failed",
            ));
        }
        _ => {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "export_running",
                "export is still running",
            ));
        }
    }
    let path = export_dir().join(format!("{id}.xlsx"));
    if !path.is_file() {
        return Err(Problem::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "export file is missing",
        ));
    }
    file_response(path, false)
}

async fn start_job(
    state: AppState,
    identity: Identity,
    prepared: Prepared,
) -> Result<Response, Problem> {
    let Some(reservation) = state.bulk.try_reserve() else {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "bulk_busy",
            "too many bulk jobs are already queued",
        ));
    };
    state
        .store
        .register_job_type(JOB_TYPE, "Export", &[], None)
        .await
        .map_err(store_status)?;
    let id = Uuid::now_v7();
    let total = i32::try_from(prepared.total).unwrap_or(i32::MAX);
    state
        .store
        .create_job_run(
            id,
            JOB_TYPE,
            None,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            total,
        )
        .await
        .map_err(store_status)?;
    tokio::spawn(async move {
        reservation
            .finish(run_export_job(state, id, prepared, total))
            .await;
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "mode": "job",
            "id": id,
            "status": "running",
            "total": total,
        })),
    )
        .into_response())
}

async fn run_export_job(state: AppState, id: Uuid, prepared: Prepared, total: i32) {
    let path = export_dir().join(format!("{id}.xlsx"));
    if let Err(error) = std::fs::create_dir_all(export_dir()) {
        fail(&state, id, total, &error.to_string()).await;
        return;
    }
    sweep_exports(&export_dir());
    emit_job(&state, id, "running", 0, total);
    let store = state.store.on_bulk();
    let progress = Arc::new(Progress {
        state: state.clone(),
        id,
        total,
        done: AtomicI64::new(0),
    });
    match export_to_file(&store, &prepared, &path, Some(progress)).await {
        Ok(written) => {
            let done = i32::try_from(written).unwrap_or(i32::MAX);
            let total = total.max(done);
            let note = format!("{done} rows");
            let _ = state
                .store
                .finish_job_run(id, "succeeded", done, &note, &json!([]))
                .await;
            emit_job(&state, id, "succeeded", done, total);
        }
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            fail(&state, id, total, &error.message).await;
        }
    }
}

async fn fail(state: &AppState, id: Uuid, total: i32, message: &str) {
    let _ = state
        .store
        .finish_job_run(id, "failed", 0, message, &json!([{ "message": message }]))
        .await;
    emit_job(state, id, "failed", 0, total);
}

async fn prepare(state: &AppState, body: &ExportBody) -> Result<Prepared, Problem> {
    let cached = state.models.get(&state.store).await?;
    let lang = body
        .lang
        .as_deref()
        .map(str::trim)
        .filter(|lang| !lang.is_empty())
        .unwrap_or("en")
        .to_owned();
    let columns = resolve_columns(&cached.model, &cached.defs, &body.columns, &lang)?;
    let scan = scan_from(&cached.model, body)?;
    let total = state
        .store
        .count_export(&scan)
        .await
        .map_err(store_status)?;
    let types = type_labels(&cached.defs, &lang);
    Ok(Prepared {
        scan,
        columns: Arc::new(columns),
        types: Arc::new(types),
        lang,
        total,
    })
}

async fn export_to_file(
    store: &Store,
    prepared: &Prepared,
    path: &Path,
    progress: Option<Arc<Progress>>,
) -> Result<i64, ExportError> {
    let (tx, rx) = mpsc::channel(1);
    let headers: Vec<String> = prepared
        .columns
        .iter()
        .map(|column| column.header.clone())
        .collect();
    let path_buf = path.to_path_buf();
    let writer = tokio::task::spawn_blocking(move || {
        write_xlsx(
            &headers,
            ChunkRows {
                rx,
                pending: Vec::new().into_iter(),
            },
            &path_buf,
        )
    });
    let columns = Arc::clone(&prepared.columns);
    let types = Arc::clone(&prepared.types);
    let lang = prepared.lang.clone();
    let send = tx.clone();
    let progress_flag = progress.clone();
    let scan_result = store
        .for_each_export_chunk(&prepared.scan, move |chunk| {
            let n = i64::try_from(chunk.len()).unwrap_or(i64::MAX);
            let columns = Arc::clone(&columns);
            let types = Arc::clone(&types);
            let lang = lang.clone();
            let send = send.clone();
            let progress_flag = progress_flag.clone();
            let cells = chunk
                .iter()
                .map(|object| cells_for(object, &columns, &types, &lang))
                .collect::<Vec<_>>();
            async move {
                send.send(cells)
                    .await
                    .map_err(|_| Error::Data("export writer stopped".into()))?;
                if let Some(progress) = progress_flag {
                    progress.tick(n).await;
                }
                Ok(())
            }
        })
        .await;
    drop(tx);
    let written = match writer.await {
        Ok(result) => result,
        Err(error) => return Err(ExportError::new(error.to_string())),
    };
    if let Err(error) = scan_result {
        let _ = std::fs::remove_file(path);
        return Err(ExportError::new(error.to_string()));
    }
    let data_rows = written?;
    Ok(i64::from(data_rows))
}

fn scan_from(model: &CompiledModel, body: &ExportBody) -> Result<ExportScan, Problem> {
    let mut scan = ExportScan::default();
    if let Some(ids) = body.ids.as_ref().filter(|ids| !ids.is_empty()) {
        scan.ids = Some(parse_ids(ids)?);
        scan.order = ExportOrder::Selection;
        return Ok(scan);
    }
    if let Some(type_key) = body
        .r#type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let Some(type_id) = model.type_id(type_key) else {
            return Err(Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_type",
                "unknown type",
            ));
        };
        scan.type_id = Some(type_id.as_uuid());
    }
    let (text, exact) = search_text(body.q.as_deref());
    scan.text = text;
    scan.exact_key = exact;
    if let Some((structure, group)) = group_filter(model, body.filter.as_deref())? {
        scan.structure_id = structure;
        scan.group_id = Some(group);
    }
    scan.order = sort_order(body.sort.as_deref());
    Ok(scan)
}

fn parse_ids(ids: &[String]) -> Result<Vec<Uuid>, Problem> {
    ids.iter()
        .map(|id| {
            Uuid::parse_str(id).map_err(|_| {
                Problem::new(StatusCode::BAD_REQUEST, "invalid_id", "invalid object id")
            })
        })
        .collect()
}

fn search_text(q: Option<&str>) -> (Option<String>, bool) {
    let Some(q) = q.map(str::trim).filter(|q| !q.is_empty() && *q != "*") else {
        return (None, false);
    };
    if let Some(exact) = q.strip_prefix('=') {
        let exact = exact.trim();
        if exact.is_empty() {
            return (None, false);
        }
        return (Some(exact.to_owned()), true);
    }
    (Some(q.to_owned()), false)
}

fn group_filter(
    model: &CompiledModel,
    filter: Option<&str>,
) -> Result<Option<(Option<Uuid>, Uuid)>, Problem> {
    let Some((structure, group)) = split_group_filter(filter.unwrap_or(""))? else {
        return Ok(None);
    };
    if let Ok(id) = Uuid::parse_str(&group) {
        let structure_id = structure
            .as_deref()
            .and_then(|key| model.structure_id(key))
            .map(|id| id.as_uuid());
        return Ok(Some((structure_id, id)));
    }
    let Some(structure_key) = structure else {
        return Err(Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_group",
            "unknown group",
        ));
    };
    let Some(group_id) = model.group_id(&structure_key, &group) else {
        return Err(Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_group",
            "unknown group",
        ));
    };
    let structure_id = model.structure_id(&structure_key).map(|id| id.as_uuid());
    Ok(Some((structure_id, group_id.as_uuid())))
}

fn split_group_filter(filter: &str) -> Result<Option<(Option<String>, String)>, Problem> {
    let mut found = None;
    for part in filter.split("&&") {
        let part = part.trim();
        if part.is_empty() || part.starts_with("deleted:") {
            continue;
        }
        let Some(value) = part.strip_prefix("groups:=") else {
            return Err(Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_filter",
                "unknown filter",
            ));
        };
        let value = value.trim().trim_matches(['`', '"']);
        found = Some(match value.split_once(':') {
            Some((structure, group)) => (Some(structure.to_owned()), group.to_owned()),
            None => (None, value.to_owned()),
        });
    }
    Ok(found)
}

fn sort_order(sort: Option<&str>) -> ExportOrder {
    let sort = sort.unwrap_or("").trim();
    let (field, dir) = sort.split_once(':').unwrap_or((sort, "asc"));
    let desc = dir.eq_ignore_ascii_case("desc");
    match (field, desc) {
        ("version", true) => ExportOrder::VersionDesc,
        ("version", false) => ExportOrder::VersionAsc,
        ("key" | "", true) => ExportOrder::KeyDesc,
        _ => ExportOrder::KeyAsc,
    }
}

fn resolve_columns(
    model: &CompiledModel,
    defs: &DefinitionSet,
    requested: &[ColumnIn],
    lang: &str,
) -> Result<Vec<ResolvedColumn>, Problem> {
    let requested = if requested.is_empty() {
        vec![ColumnIn::Name("key".into()), ColumnIn::Name("type".into())]
    } else {
        requested.to_vec()
    };
    if requested.len() > MAX_COLUMNS {
        return Err(Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export",
            "too many columns",
        ));
    }
    Ok(requested
        .iter()
        .map(|column| resolve_column(model, defs, column, lang))
        .collect())
}

fn resolve_column(
    model: &CompiledModel,
    defs: &DefinitionSet,
    column: &ColumnIn,
    lang: &str,
) -> ResolvedColumn {
    let (id, header, attribute) = column.parts();
    let header_of = |fallback: &str| {
        header
            .filter(|text| !text.is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    match id {
        "key" => ResolvedColumn {
            header: header_of("Key"),
            kind: ColumnKind::Key,
        },
        "type" | "type_key" => ResolvedColumn {
            header: header_of("Type"),
            kind: ColumnKind::Type,
        },
        "version" => ResolvedColumn {
            header: header_of("Version"),
            kind: ColumnKind::Version,
        },
        _ => {
            let key = attribute_name(id, attribute);
            let Some(attribute) = attribute_for_column(model, &key) else {
                return ResolvedColumn {
                    header: header_of(&key),
                    kind: ColumnKind::Blank,
                };
            };
            let fallback =
                label_in(&attribute.labels, lang).unwrap_or_else(|| attribute.key.clone());
            ResolvedColumn {
                header: header_of(&fallback),
                kind: ColumnKind::Attribute(AttrColumn {
                    id: attribute.id,
                    shape: shape_of(&attribute.kind),
                    multiple: attribute.multiple,
                    language_dependent: attribute.language_dependent,
                    labels: option_labels(defs, attribute, lang),
                }),
            }
        }
    }
}

fn attribute_for_column<'a>(
    model: &'a CompiledModel,
    token: &str,
) -> Option<&'a braian_model::Attribute> {
    if let Some(attribute) = model.attribute_by_key(token) {
        return Some(attribute);
    }
    let uuid_text = if token.len() == 36 && token.as_bytes().get(8) == Some(&b'_') {
        token.replace('_', "-")
    } else {
        token.to_owned()
    };
    let id = uuid::Uuid::parse_str(&uuid_text).ok()?;
    model.attribute(braian_model::AttributeId::from_uuid(id))
}

fn attribute_name(id: &str, attribute: Option<&str>) -> String {
    if let Some(attribute) = attribute.map(str::trim).filter(|value| !value.is_empty()) {
        return attribute.to_owned();
    }
    if id == "name" {
        return "name".to_owned();
    }
    if let Some(rest) = id.strip_prefix("a_") {
        return rest.split("__").next().unwrap_or(rest).to_owned();
    }
    id.to_owned()
}

fn shape_of(kind: &AttributeKind) -> ValueShape {
    match kind {
        AttributeKind::Number { .. } => ValueShape::Number,
        AttributeKind::Boolean => ValueShape::Boolean,
        AttributeKind::Date => ValueShape::Date,
        AttributeKind::Range { .. } => ValueShape::Range,
        AttributeKind::ValueList { .. } => ValueShape::List,
        _ => ValueShape::Text,
    }
}

fn option_labels(
    defs: &DefinitionSet,
    attribute: &Attribute,
    lang: &str,
) -> BTreeMap<String, String> {
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
            label_in(&option.labels, lang).map(|label| (option.code.clone(), label))
        })
        .collect()
}

fn type_labels(defs: &DefinitionSet, lang: &str) -> HashMap<Uuid, String> {
    defs.types
        .iter()
        .map(|object_type| {
            let label =
                label_in(&object_type.labels, lang).unwrap_or_else(|| object_type.key.clone());
            (object_type.id.as_uuid(), label)
        })
        .collect()
}

fn label_in(labels: &BTreeMap<LanguageTag, String>, lang: &str) -> Option<String> {
    labels
        .iter()
        .find(|(tag, _)| tag.as_str() == lang)
        .map(|(_, text)| text.clone())
}

fn cells_for(
    object: &ExportObject,
    columns: &[ResolvedColumn],
    types: &HashMap<Uuid, String>,
    lang: &str,
) -> Vec<Cell> {
    columns
        .iter()
        .map(|column| match &column.kind {
            ColumnKind::Key => text_cell(&object.key),
            ColumnKind::Type => {
                text_cell(types.get(&object.type_id).map(String::as_str).unwrap_or(""))
            }
            ColumnKind::Version => Cell::Number(object.version as f64),
            ColumnKind::Blank => Cell::Blank,
            ColumnKind::Attribute(attr) => map_attribute(object.values.get(&attr.id), attr, lang),
        })
        .collect()
}

fn map_attribute(value: Option<&Value>, attr: &AttrColumn, lang: &str) -> Cell {
    let Some(value) = value else {
        return Cell::Blank;
    };
    let Some(value) = language_value(value, attr.language_dependent, lang) else {
        return Cell::Blank;
    };
    if attr.multiple {
        let Some(items) = value.as_array() else {
            return scalar_one(value, attr);
        };
        let joined = items
            .iter()
            .map(|item| scalar_text(item, attr))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        return text_cell(joined);
    }
    scalar_one(value, attr)
}

fn language_value<'a>(value: &'a Value, dependent: bool, lang: &str) -> Option<&'a Value> {
    if !dependent {
        return Some(value);
    }
    value.as_object()?.get(lang)
}

fn scalar_one(value: &Value, attr: &AttrColumn) -> Cell {
    match attr.shape {
        ValueShape::Number => number_cell(value),
        ValueShape::Boolean => match value {
            Value::Bool(flag) => Cell::Boolean(*flag),
            _ => text_cell(string_of(value)),
        },
        ValueShape::Date => date_cell(value),
        ValueShape::Range => text_cell(range_text(value)),
        ValueShape::List => text_cell(list_text(value, attr)),
        ValueShape::Text => text_cell(string_of(value)),
    }
}

fn scalar_text(value: &Value, attr: &AttrColumn) -> String {
    match scalar_one(value, attr) {
        Cell::Blank => String::new(),
        Cell::Text(text) => text,
        Cell::Number(number) => number_text(number),
        Cell::Boolean(flag) => flag.to_string(),
        Cell::Date { year, month, day } => format!("{year:04}-{month:02}-{day:02}"),
    }
}

fn number_cell(value: &Value) -> Cell {
    let Some(text) = decimal_string(value) else {
        return text_cell(string_of(value));
    };
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() => Cell::Number(number),
        _ => text_cell(text),
    }
}

fn decimal_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Object(map) => map.get("value").and_then(decimal_string),
        _ => None,
    }
}

fn date_cell(value: &Value) -> Cell {
    let Some(text) = value.as_str() else {
        return text_cell(string_of(value));
    };
    match parse_date(text) {
        Some((year, month, day)) => Cell::Date { year, month, day },
        None => text_cell(text),
    }
}

fn parse_date(text: &str) -> Option<(u16, u8, u8)> {
    let mut parts = text.split('-');
    let year: u16 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn list_text(value: &Value, attr: &AttrColumn) -> String {
    let Some(code) = value.as_str() else {
        return string_of(value);
    };
    attr.labels
        .get(code)
        .cloned()
        .unwrap_or_else(|| code.to_owned())
}

fn range_text(value: &Value) -> String {
    let Some(map) = value.as_object() else {
        return string_of(value);
    };
    let min = map.get("min").map(string_of).unwrap_or_default();
    let max = map.get("max").map(string_of).unwrap_or_default();
    let mut text = format!("{min}–{max}");
    if let Some(unit) = map.get("unit").and_then(Value::as_str)
        && !unit.is_empty()
    {
        text.push(' ');
        text.push_str(unit);
    }
    text
}

fn string_of(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items
            .iter()
            .map(string_of)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

fn text_cell(text: impl AsRef<str>) -> Cell {
    let text = text.as_ref();
    if text.is_empty() {
        return Cell::Blank;
    }
    if text.chars().count() <= EXCEL_TEXT {
        return Cell::Text(text.to_owned());
    }
    Cell::Text(text.chars().take(EXCEL_TEXT).collect())
}

fn column_widths<R: AsRef<[Cell]>>(headers: &[String], sample: &[R]) -> Vec<f64> {
    let sample = &sample[..sample.len().min(WIDTH_SAMPLE)];
    headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let mut units = text_units(header);
            for row in sample {
                if let Some(cell) = row.as_ref().get(index) {
                    units = units.max(cell_units(cell));
                }
            }
            (units + WIDTH_PAD).clamp(MIN_WIDTH, MAX_WIDTH)
        })
        .collect()
}

fn cell_units(cell: &Cell) -> f64 {
    match cell {
        Cell::Blank => 0.0,
        Cell::Text(text) => text_units(text),
        Cell::Number(number) => text_units(&number_text(*number)),
        Cell::Boolean(true) => text_units("TRUE"),
        Cell::Boolean(false) => text_units("FALSE"),
        Cell::Date { .. } => 10.0,
    }
}

fn number_text(number: f64) -> String {
    if !number.is_finite() {
        return String::new();
    }
    if number.fract() == 0.0 && number.abs() < 1e15 {
        return format!("{}", number as i64);
    }
    format!("{number}")
}

fn text_units(text: &str) -> f64 {
    text.split(['\n', '\r'])
        .map(|line| {
            line.chars()
                .map(|ch| if is_wide(ch) { 2.0 } else { 1.0 })
                .sum::<f64>()
        })
        .fold(0.0, f64::max)
}

fn is_wide(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{115F}'
            | '\u{2329}'..='\u{232A}'
            | '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE10}'..='\u{FE19}'
            | '\u{FE30}'..='\u{FE6F}'
            | '\u{FF00}'..='\u{FF60}'
            | '\u{FFE0}'..='\u{FFE6}'
            | '\u{1F300}'..='\u{1F64F}'
            | '\u{1F900}'..='\u{1F9FF}'
            | '\u{20000}'..='\u{3FFFD}'
    )
}

fn unique_headers(headers: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    headers
        .iter()
        .map(|header| {
            let base = table_header(header);
            let count = seen.entry(base.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                base
            } else {
                format!("{base} {count}")
            }
        })
        .collect()
}

fn table_header(header: &str) -> String {
    let cleaned: String = header
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Column".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn write_xlsx<I, R>(headers: &[String], rows: I, path: &Path) -> Result<u32, ExportError>
where
    I: Iterator<Item = R>,
    R: AsRef<[Cell]>,
{
    if headers.is_empty() {
        return Err(ExportError::new("export needs a column"));
    }
    let headers = unique_headers(headers);
    let mut rows = rows;
    let mut sample = Vec::new();
    while sample.len() < WIDTH_SAMPLE {
        let Some(row) = rows.next() else {
            break;
        };
        sample.push(row);
    }
    let widths = column_widths(&headers, &sample);
    let mut workbook = Workbook::new();
    let date_format = Format::new().set_num_format("yyyy-mm-dd");
    let mut data_rows = 0u32;
    let last_col = u16::try_from(headers.len().saturating_sub(1)).unwrap_or(0);
    {
        let worksheet = workbook.add_worksheet_with_constant_memory();
        for (index, width) in widths.iter().enumerate() {
            let col = u16::try_from(index).unwrap_or(u16::MAX);
            worksheet.set_column_width(col, *width)?;
        }
        worksheet.set_freeze_panes(1, 0)?;
        // A real table needs a header row and at least one data row. The
        // library records every cell of the table range, so the range added
        // here is only the header plus the first data row. The saved table
        // XML is widened to the real range afterwards, without holding the
        // rest of the cells.
        if sample.is_empty() {
            for (index, header) in headers.iter().enumerate() {
                let col = u16::try_from(index).unwrap_or(u16::MAX);
                worksheet.write_string(0, col, header)?;
            }
        } else {
            let columns: Vec<TableColumn> = headers
                .iter()
                .map(|header| TableColumn::new().set_header(header))
                .collect();
            let table = Table::new()
                .set_name("Export")
                .set_style(TableStyle::Medium2)
                .set_columns(&columns);
            worksheet.add_table(0, 0, 1, last_col, &table)?;
        }
        for row in &sample {
            data_rows += 1;
            write_cells(worksheet, data_rows, row.as_ref(), &date_format)?;
        }
        drop(sample);
        for row in rows {
            if i64::from(data_rows) >= EXCEL_DATA_ROWS {
                return Err(ExportError::new("too many rows"));
            }
            data_rows += 1;
            write_cells(worksheet, data_rows, row.as_ref(), &date_format)?;
        }
    }
    workbook.save(path)?;
    if data_rows > 1 {
        widen_table_ref(path, data_rows, headers.len())?;
    }
    Ok(data_rows)
}

/// Rewrites only the small table part. The worksheet entry is copied compressed.
fn widen_table_ref(path: &Path, data_rows: u32, columns: usize) -> Result<(), ExportError> {
    let letter = column_letter(u16::try_from(columns.saturating_sub(1)).unwrap_or(0));
    let from = format!("A1:{letter}2");
    let to = format!("A1:{letter}{}", data_rows + 1);
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let tmp = path.with_extension("part");
    let out = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(out);
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.name() == "xl/tables/table1.xml" {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut entry, &mut text)?;
            drop(entry);
            if !text.contains(&from) {
                let _ = std::fs::remove_file(&tmp);
                return Err(ExportError::new("table range was not the sample row"));
            }
            let text = text.replace(&from, &to);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("xl/tables/table1.xml", options)?;
            std::io::Write::write_all(&mut writer, text.as_bytes())?;
            found = true;
        } else {
            writer.raw_copy_file(entry)?;
        }
    }
    writer.finish()?;
    if !found {
        let _ = std::fs::remove_file(&tmp);
        return Err(ExportError::new("workbook has no table"));
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn column_letter(mut index: u16) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, char::from(b'A' + (index % 26) as u8));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name
}

fn write_cells(
    worksheet: &mut Worksheet,
    row: u32,
    cells: &[Cell],
    date_format: &Format,
) -> Result<(), ExportError> {
    for (index, cell) in cells.iter().enumerate() {
        let Ok(col) = u16::try_from(index) else {
            break;
        };
        match cell {
            Cell::Blank => {}
            Cell::Text(text) => {
                worksheet.write_string(row, col, text)?;
            }
            Cell::Number(number) if number.is_finite() => {
                worksheet.write_number(row, col, *number)?;
            }
            Cell::Number(_) => {}
            Cell::Boolean(value) => {
                worksheet.write_boolean(row, col, *value)?;
            }
            Cell::Date { year, month, day } => {
                let date = ExcelDateTime::from_ymd(*year, *month, *day)?;
                worksheet.write_datetime_with_format(row, col, &date, date_format)?;
            }
        }
    }
    Ok(())
}

fn export_dir() -> PathBuf {
    std::env::var_os("BRAIAN_EXPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("braian-exports"))
}

fn sweep_exports(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let Some(cutoff) = SystemTime::now().checked_sub(Duration::from_secs(24 * 60 * 60)) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn file_response(path: PathBuf, delete: bool) -> Result<Response, Problem> {
    let len = std::fs::metadata(&path).map_err(io_problem)?.len();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, XLSX_TYPE)
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"braian.xlsx\"",
        )
        .header(header::CONTENT_LENGTH, len.to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-accel-buffering", "no")
        .body(stream_file(path, delete))
        .map_err(|error| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "export",
                error.to_string(),
            )
        })
}

fn stream_file(path: PathBuf, delete: bool) -> Body {
    let (tx, rx) = mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        let streamed = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::open(&path)?;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = std::io::Read::read(&mut file, &mut buf)?;
                if n == 0 {
                    break;
                }
                let chunk = Bytes::copy_from_slice(&buf[..n]);
                if tx.blocking_send(Ok(chunk)).is_err() {
                    break;
                }
            }
            Ok(())
        })();
        if let Err(error) = streamed {
            let _ = tx.blocking_send(Err(error));
        }
        if delete {
            let _ = std::fs::remove_file(&path);
        }
    });
    Body::from_stream(stream::unfold(rx, |mut rx| async move {
        let item = rx.recv().await?;
        Some((item, rx))
    }))
}

fn io_problem(error: std::io::Error) -> Problem {
    Problem::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "export",
        error.to_string(),
    )
}

fn export_problem(error: ExportError) -> Problem {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "export", error.message)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        AttrColumn, Cell, ValueShape, WIDTH_SAMPLE, column_widths, is_wide, map_attribute,
        search_text, split_group_filter, text_units, write_xlsx,
    };
    use serde_json::json;

    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("braian-xlsx-{}.xlsx", uuid::Uuid::now_v7())))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn zip_text(path: &std::path::Path, suffix: &str) -> String {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            names.push(entry.name().to_owned());
            if entry.name().ends_with(suffix) {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
                return text;
            }
        }
        panic!("missing {suffix} in {names:?}");
    }

    fn attr(shape: ValueShape, multiple: bool, localized: bool) -> AttrColumn {
        AttrColumn {
            id: AttributeId::from_uuid(uuid::Uuid::nil()),
            shape,
            multiple,
            language_dependent: localized,
            labels: [("ok".into(), "Klar".into())].into(),
        }
    }

    use braian_model::AttributeId;

    #[test]
    fn values_keep_excel_types_and_the_requested_language() {
        let number = attr(ValueShape::Number, false, false);
        assert_eq!(
            map_attribute(Some(&json!({"value": "12.5", "unit": "kg"})), &number, "sv"),
            Cell::Number(12.5)
        );
        let flag = attr(ValueShape::Boolean, false, false);
        assert_eq!(
            map_attribute(Some(&json!(true)), &flag, "sv"),
            Cell::Boolean(true)
        );
        let date = attr(ValueShape::Date, false, false);
        assert_eq!(
            map_attribute(Some(&json!("2024-03-15")), &date, "sv"),
            Cell::Date {
                year: 2024,
                month: 3,
                day: 15
            }
        );
        let many = attr(ValueShape::Text, true, false);
        assert_eq!(
            map_attribute(Some(&json!(["a", "b"])), &many, "sv"),
            Cell::Text("a, b".into())
        );
        let name = attr(ValueShape::Text, false, true);
        let localized = json!({"sv": "Hej", "en": "Hi"});
        assert_eq!(
            map_attribute(Some(&localized), &name, "sv"),
            Cell::Text("Hej".into())
        );
        assert_eq!(
            map_attribute(Some(&localized), &name, "en"),
            Cell::Text("Hi".into())
        );
        assert_eq!(map_attribute(Some(&localized), &name, "de"), Cell::Blank);
        let list = attr(ValueShape::List, false, false);
        assert_eq!(
            map_attribute(Some(&json!("ok")), &list, "sv"),
            Cell::Text("Klar".into())
        );
        assert_eq!(
            map_attribute(Some(&json!("other")), &list, "sv"),
            Cell::Text("other".into())
        );
    }

    #[test]
    fn column_widths_use_a_sample_and_wide_characters() {
        assert!(is_wide('字'));
        assert!(!is_wide('a'));
        assert!((text_units("字") - 2.0).abs() < f64::EPSILON);
        let narrow = column_widths(&["n".into()], &[vec![Cell::Text("a".repeat(10))]]);
        let wide = column_widths(&["n".into()], &[vec![Cell::Text("字".repeat(10))]]);
        assert!(wide[0] > narrow[0], "{wide:?} vs {narrow:?}");

        let tiny = column_widths(&["A".into()], &[vec![Cell::Text("a".into())]]);
        assert_eq!(tiny[0], super::MIN_WIDTH);
        let huge = column_widths(&["A".into()], &[vec![Cell::Text("x".repeat(500))]]);
        assert_eq!(huge[0], super::MAX_WIDTH);

        let mut rows = Vec::new();
        for _ in 0..WIDTH_SAMPLE {
            rows.push(vec![Cell::Text("short".into())]);
        }
        rows.push(vec![Cell::Text("x".repeat(300))]);
        let widths = column_widths(&["Code".into()], &rows);
        assert!(
            widths[0] < 20.0,
            "later rows widened the column: {}",
            widths[0]
        );
    }

    #[test]
    fn workbook_is_a_table_with_typed_cells_and_sample_widths() {
        let file = TempFile::new();
        let headers = ["Name".into(), "Count".into(), "On".into(), "When".into()];
        let rows = [
            vec![
                Cell::Text("Hej".into()),
                Cell::Number(12.5),
                Cell::Boolean(true),
                Cell::Date {
                    year: 2024,
                    month: 3,
                    day: 15,
                },
            ],
            vec![
                Cell::Text("a, b".into()),
                Cell::Number(3.0),
                Cell::Boolean(false),
                Cell::Date {
                    year: 2020,
                    month: 1,
                    day: 2,
                },
            ],
        ];
        write_xlsx(&headers, rows.into_iter(), &file.0).unwrap();
        let table = zip_text(&file.0, "xl/tables/table1.xml");
        assert!(table.contains("ref=\"A1:D3\""), "{table}");
        assert!(table.contains("autoFilter"), "{table}");
        assert!(table.contains("name=\"Name\""), "{table}");
        assert!(table.contains("name=\"Count\""), "{table}");
        assert!(table.contains("name=\"When\""), "{table}");
        let sheet = zip_text(&file.0, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("ySplit=\"1\""), "{sheet}");
        assert!(sheet.contains("frozen"), "{sheet}");
        assert!(sheet.contains("t=\"b\""), "{sheet}");
        assert!(
            sheet.contains(">12.5<") || sheet.contains(">12.5</v>"),
            "{sheet}"
        );
        assert!(sheet.contains("Hej"), "{sheet}");
        let styles = zip_text(&file.0, "xl/styles.xml");
        assert!(styles.contains("yyyy-mm-dd"), "{styles}");

        let sized = TempFile::new();
        let mut sample = Vec::new();
        for _ in 0..WIDTH_SAMPLE {
            sample.push(vec![
                Cell::Text("short".into()),
                Cell::Text("字".repeat(10)),
            ]);
        }
        sample.push(vec![
            Cell::Text("x".repeat(300)),
            Cell::Text("字".repeat(10)),
        ]);
        write_xlsx(
            &["Code".into(), "Wide".into()],
            sample.into_iter(),
            &sized.0,
        )
        .unwrap();
        let sheet = zip_text(&sized.0, "xl/worksheets/sheet1.xml");
        let widths = col_widths(&sheet);
        assert!(widths.len() >= 2, "{sheet}");
        assert!(widths[0] <= 12.0, "sample ignored: {widths:?}");
        assert!(widths[1] >= 18.0, "wide chars ignored: {widths:?}");
        assert!(widths[1] > widths[0]);
    }

    #[test]
    fn a_large_export_streams_rows_without_keeping_them_all() {
        const ROWS: usize = 5_000;
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let live_rows = Arc::clone(&live);
        let peak_rows = Arc::clone(&peak);
        let rows = (0..ROWS).map(move |index| {
            let current = live_rows.fetch_add(1, Ordering::SeqCst) + 1;
            peak_rows.fetch_max(current, Ordering::SeqCst);
            Held {
                cells: vec![Cell::Number(index as f64)],
                live: Arc::clone(&live_rows),
            }
        });
        let file = TempFile::new();
        let written = write_xlsx(&["N".into()], rows, &file.0).unwrap();
        assert_eq!(written, u32::try_from(ROWS).unwrap());
        assert_eq!(live.load(Ordering::SeqCst), 0);
        let high = peak.load(Ordering::SeqCst);
        assert!(
            high <= WIDTH_SAMPLE + 2,
            "held {high} rows at once, sample is {WIDTH_SAMPLE}"
        );
        let table = zip_text(&file.0, "xl/tables/table1.xml");
        assert!(
            table.contains(&format!("ref=\"A1:A{}\"", ROWS + 1)),
            "{table}"
        );
    }

    struct Held {
        cells: Vec<Cell>,
        live: Arc<AtomicUsize>,
    }

    impl AsRef<[Cell]> for Held {
        fn as_ref(&self) -> &[Cell] {
            &self.cells
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn col_widths(xml: &str) -> Vec<f64> {
        let mut widths = Vec::new();
        let mut rest = xml;
        while let Some(start) = rest.find("<col ") {
            rest = &rest[start..];
            let Some(end) = rest.find("/>") else {
                break;
            };
            let tag = &rest[..end];
            if let Some(index) = tag.find("width=\"") {
                let after = &tag[index + "width=\"".len()..];
                let num: String = after
                    .chars()
                    .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
                    .collect();
                if let Ok(width) = num.parse() {
                    widths.push(width);
                }
            }
            rest = &rest[end + 2..];
        }
        widths
    }

    #[test]
    fn search_text_and_group_filters_match_the_view() {
        assert_eq!(search_text(Some("*")), (None, false));
        assert_eq!(search_text(Some("=SKU-1")), (Some("SKU-1".into()), true));
        assert_eq!(search_text(Some("100%_")), (Some("100%_".into()), false));
        let (pattern, exact) = search_text(Some("tent"));
        assert!(!exact);
        assert_eq!(pattern.as_deref(), Some("tent"));
        assert_eq!(
            split_group_filter("groups:=food:cheese").unwrap(),
            Some((Some("food".into()), "cheese".into()))
        );
        assert!(split_group_filter("color:=red").is_err());
    }
}
