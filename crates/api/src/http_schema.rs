//! JSON shapes for routes whose handlers still build `serde_json::Value`.
//!
//! The structs are the OpenAPI contract. Handlers keep their current JSON
//! so a schema change cannot silently drop a field the client already reads.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::Serialize;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{AdditionalProperties, ObjectBuilder, Schema, SchemaType, Type};
use utoipa::{PartialSchema, ToSchema};
use uuid::Uuid;

/// JSON object whose keys are data, not a fixed field list.
pub struct FreeObject;

impl PartialSchema for FreeObject {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::Object)
            .description(Some("JSON object. Keys depend on the stored document."))
            .additional_properties(Some(AdditionalProperties::FreeForm(true)))
            .into()
    }
}

impl ToSchema for FreeObject {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("FreeObject")
    }
}

impl Serialize for FreeObject {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        serializer.serialize_map(Some(0))?.end()
    }
}

/// Any JSON value, including arrays and scalars.
pub struct JsonValue;

impl PartialSchema for JsonValue {
    fn schema() -> RefOr<Schema> {
        // AnyValue omits `type` so the schema accepts objects, arrays, and scalars.
        ObjectBuilder::new()
            .schema_type(SchemaType::AnyValue)
            .description(Some("Any JSON value."))
            .into()
    }
}

impl ToSchema for JsonValue {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("JsonValue")
    }
}

impl Serialize for JsonValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

/// One model-draft step. `op` selects the change; other fields depend on it.
pub struct ModelStep;

impl PartialSchema for ModelStep {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::Object)
            .description(Some(
                "One model change. `op` is the step name; remaining fields depend on `op`.",
            ))
            .property("op", ObjectBuilder::new().schema_type(Type::String).build())
            .required("op")
            .additional_properties(Some(AdditionalProperties::FreeForm(true)))
            .into()
    }
}

impl ToSchema for ModelStep {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("ModelStep")
    }
}

impl Serialize for ModelStep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut step = serializer.serialize_struct("ModelStep", 1)?;
        step.serialize_field("op", "")?;
        step.end()
    }
}

#[derive(Serialize, ToSchema)]
pub struct IdBody {
    pub id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct SessionUser {
    pub email: String,
    pub role: String,
    pub display_name: String,
    pub user_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedToken {
    pub token: String,
    pub kind: String,
    pub name: String,
    pub owner: String,
}

#[derive(Serialize, ToSchema)]
pub struct OperationInfo {
    pub name: String,
    pub title: String,
    pub permission: String,
    pub needs_approval: bool,
}

#[derive(Serialize, ToSchema)]
pub struct OperationList {
    pub operations: Vec<OperationInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct AgentTool {
    pub name: JsonValue,
    pub result: JsonValue,
}

#[derive(Serialize, ToSchema)]
pub struct AgentReply {
    pub available: bool,
    pub model: String,
    pub reply: String,
    pub tools: Vec<AgentTool>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchHit {
    pub id: String,
    pub document: FreeObject,
}

#[derive(Serialize, ToSchema)]
pub struct SearchGroup {
    pub key: String,
    pub found: u32,
    pub hits: Vec<SearchHit>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchResponse {
    pub found: u32,
    pub hits: Vec<SearchHit>,
    pub groups: Vec<SearchGroup>,
    pub indexed_seq: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectHead {
    pub id: Uuid,
    pub key: String,
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub version: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectList {
    pub objects: Vec<ObjectHead>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct NamedObject {
    pub id: Uuid,
    pub key: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct NamedObjectList {
    pub objects: Vec<NamedObject>,
}

#[derive(Serialize, ToSchema)]
pub struct TypeCount {
    #[serde(rename = "type")]
    pub object_type: String,
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    pub objects: usize,
    pub by_type: Vec<TypeCount>,
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceView {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub view_type: String,
    pub title: String,
    pub pane: String,
    pub position: i32,
    pub state: FreeObject,
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceRow {
    pub id: Uuid,
    pub name: String,
    pub locked: bool,
    pub share_token: Option<String>,
    pub layout: FreeObject,
    pub owner_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<Vec<WorkspaceView>>,
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceList {
    pub workspaces: Vec<WorkspaceRow>,
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceState {
    pub environment: String,
    pub document: Option<FreeObject>,
    pub updated_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceStateSaved {
    pub environment: String,
    pub updated_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ViewTypeRow {
    pub key: String,
    pub title: String,
    pub builtin: bool,
    pub embed_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ViewTypeList {
    pub view_types: Vec<ViewTypeRow>,
}

#[derive(Serialize, ToSchema)]
pub struct AssortmentRow {
    pub id: Uuid,
    pub name: String,
    pub type_id: Uuid,
    pub query: FreeObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub object_type: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AssortmentList {
    pub assortments: Vec<AssortmentRow>,
}

#[derive(Serialize, ToSchema)]
pub struct JobTypeRow {
    pub key: String,
    pub title: String,
    pub reads_fields: Vec<String>,
    pub assortment_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct JobTypeList {
    pub job_types: Vec<JobTypeRow>,
}

#[derive(Serialize, ToSchema)]
pub struct JobRun {
    pub id: Uuid,
    pub type_key: String,
    pub status: String,
    pub progress_done: i32,
    pub progress_total: i32,
    pub log: String,
    pub errors: JsonValue,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct JobRunList {
    pub runs: Vec<JobRun>,
}

#[derive(Serialize, ToSchema)]
pub struct RunningJob {
    pub id: Uuid,
    pub status: String,
    pub total: i32,
}

#[derive(Serialize, ToSchema)]
pub struct ExportJob {
    pub mode: String,
    pub id: Uuid,
    pub status: String,
    pub total: i32,
}

#[derive(Serialize, ToSchema)]
pub struct FrecencyEntry {
    pub uses: i32,
    pub last: i64,
}

#[derive(Serialize, ToSchema)]
pub struct FrecencyList {
    pub entries: BTreeMap<String, FrecencyEntry>,
}

#[derive(Serialize, ToSchema)]
pub struct FrecencyTouch {
    pub id: String,
    pub uses: i32,
    pub last: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PendingObject {
    pub object_id: Uuid,
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PendingSummary {
    pub total: i64,
    pub objects: Vec<PendingObject>,
}

#[derive(Serialize, ToSchema)]
pub struct ProposalListItem {
    pub id: Uuid,
    pub title: String,
    pub source: String,
    pub status: String,
    pub conflict: bool,
    pub created_at: String,
    /// Set for import drafts: the import session that holds the rows.
    pub session_id: Option<Uuid>,
    /// Import drafts: new, changed, failed, stale and excluded row counts.
    pub counters: Option<JsonValue>,
    pub actor_name: Option<String>,
    pub behalf_name: Option<String>,
    /// Rows stored in `proposal_rows`. Import drafts keep theirs elsewhere.
    pub row_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ProposalList {
    pub proposals: Vec<ProposalListItem>,
    /// Proposals that match the filter, not only this page.
    pub total: i64,
    pub pending: PendingSummary,
}

#[derive(Serialize, ToSchema)]
pub struct DraftColumn {
    pub col: u64,
    pub header: String,
    pub target: String,
    pub label: String,
}

/// One page of a draft. Import drafts add the mapped columns and counters.
#[derive(Serialize, ToSchema)]
pub struct DraftRowsPage {
    /// `import` or `rows`.
    pub kind: String,
    pub total: i64,
    pub rows: Vec<JsonValue>,
    pub session_id: Option<Uuid>,
    pub session_status: Option<String>,
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub columns: Option<Vec<DraftColumn>>,
    pub counters: Option<JsonValue>,
}

#[derive(Serialize, ToSchema)]
pub struct ProposalRowView {
    pub id: Uuid,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub type_key: Option<String>,
    pub base_version: Option<i64>,
    pub patch: FreeObject,
    pub status: String,
    pub conflict: bool,
    pub position: i32,
}

#[derive(Serialize, ToSchema)]
pub struct SampleRef {
    pub id: Uuid,
    pub key: String,
}

#[derive(Serialize, ToSchema)]
pub struct JobTouch {
    pub key: String,
    pub title: String,
    pub reads_fields: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct DraftConsequence {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub fields: Vec<String>,
    pub affected_objects: i64,
    pub samples: Vec<SampleRef>,
    pub jobs: Vec<JobTouch>,
    pub objects: i64,
    pub links: i64,
    pub values: i64,
    pub selections: i64,
    pub destructive: bool,
    pub confirm_name: String,
    pub sentence: String,
    pub detail: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ProposalView {
    pub id: Uuid,
    pub title: String,
    pub source: String,
    pub status: String,
    pub conflict: bool,
    pub actor: Uuid,
    pub on_behalf_of: Option<Uuid>,
    pub actor_name: Option<String>,
    pub behalf_name: Option<String>,
    pub created_at: String,
    pub session_id: Option<Uuid>,
    pub counters: Option<JsonValue>,
    pub rows: Vec<ProposalRowView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consequence: Option<DraftConsequence>,
}

#[derive(Serialize, ToSchema)]
pub struct AcceptResult {
    pub accepted: Vec<JsonValue>,
    pub failed: Vec<JsonValue>,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryChange {
    pub seq: i64,
    pub at: String,
    pub batch_id: Uuid,
    pub object_id: Option<Uuid>,
    pub version: Option<i64>,
    pub op: String,
    pub source: String,
    pub kind: String,
    pub actor: String,
    pub on_behalf_of: Option<String>,
    pub via: String,
    pub diff: Option<FreeObject>,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryList {
    pub changes: Vec<HistoryChange>,
}

#[derive(Serialize, ToSchema)]
pub struct AsOfView {
    pub seq: i64,
    pub deleted: bool,
    pub values: FreeObject,
}

#[derive(Serialize, ToSchema)]
pub struct GraphLink {
    pub id: Uuid,
    pub link_type: String,
    pub label: String,
    pub object_id: Uuid,
    pub key: String,
    pub type_id: Uuid,
    pub direction: String,
}

#[derive(Serialize, ToSchema)]
pub struct GraphSource {
    pub from: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    pub structure: Option<String>,
    pub class: Option<String>,
    pub group: Option<String>,
    pub inherited: bool,
}

#[derive(Serialize, ToSchema)]
pub struct GraphAttribute {
    pub key: String,
    pub label: String,
    pub sources: Vec<GraphSource>,
    pub changed_at: Option<String>,
    pub changed_by: Option<String>,
    pub seq: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectGraph {
    pub id: Uuid,
    pub key: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub groups: BTreeMap<String, String>,
    pub links: Vec<GraphLink>,
    pub attributes: Vec<GraphAttribute>,
}

#[derive(Serialize, ToSchema)]
pub struct ImpactReport {
    pub affected_objects: i64,
    pub sampled_objects: i64,
    pub samples: Vec<SampleRef>,
    pub invalid_objects: i64,
    pub jobs: Vec<JobTouch>,
    pub definition_errors: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct WebhookRow {
    pub id: Uuid,
    pub url: String,
    pub assortment_id: Option<Uuid>,
    pub enabled: bool,
}

#[derive(Serialize, ToSchema)]
pub struct WebhookList {
    pub webhooks: Vec<WebhookRow>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedWebhook {
    pub id: Uuid,
    pub secret: String,
}

#[derive(Serialize, ToSchema)]
pub struct ColumnPrefsView {
    pub environment: String,
    pub document: Option<FreeObject>,
    pub updated_at: Option<String>,
    pub stale_paths: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ColumnPrefsSaved {
    pub environment: String,
    pub document: FreeObject,
    pub updated_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportTypeOption {
    pub id: Uuid,
    pub key: String,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportTarget {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub key: bool,
    pub source: String,
    pub language: Option<String>,
    pub unit: Option<String>,
    pub attribute_id: Option<String>,
    pub structure_id: Option<String>,
    pub group_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportCreated {
    pub id: Uuid,
    pub status: String,
    pub file_name: String,
    pub file_kind: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportSessionView {
    pub types: Vec<ImportTypeOption>,
    pub id: Uuid,
    pub status: String,
    pub file_name: String,
    pub file_kind: String,
    pub sheets: JsonValue,
    pub sheet_index: i32,
    pub headers: JsonValue,
    pub sample: JsonValue,
    pub rows_parsed: i64,
    pub parse_done: bool,
    pub model_revision: i64,
    pub targets: Vec<ImportTarget>,
    pub proposal: Option<JsonValue>,
    pub mapping: Option<JsonValue>,
    pub counters: JsonValue,
    pub job_id: Option<Uuid>,
    pub error: Option<String>,
    /// The open draft (proposal) of this import, once it is previewed.
    pub draft_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportStatus {
    pub status: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportPreviewRow {
    pub row_no: i32,
    pub status: String,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub cells: JsonValue,
    pub errors: JsonValue,
}

#[derive(Serialize, ToSchema)]
pub struct ImportPreviewPage {
    pub total: i64,
    pub rows: Vec<ImportPreviewRow>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportRowEdit {
    pub col: i32,
    pub value: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportRowEdited {
    pub counters: JsonValue,
    pub row: Option<ImportPreviewRow>,
}

#[derive(Serialize, ToSchema)]
pub struct SavedMappingBody {
    pub session_id: Uuid,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct SavedMappingRename {
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct SavedMappingView {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub headers: JsonValue,
    pub columns: JsonValue,
    pub empty_clears: bool,
}

#[derive(Serialize, ToSchema)]
pub struct SavedMappingList {
    pub mappings: Vec<SavedMappingView>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportJob {
    pub job_id: Uuid,
}

/// Column in an export request: a path string, or an id with a header.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ExportColumn {
    Name(String),
    Spec {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        header: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        attribute: Option<String>,
    },
}

#[derive(Serialize, ToSchema)]
pub struct ImportMappingColumn {
    pub col: u64,
    pub target: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportMappingBody {
    #[serde(rename = "type")]
    pub object_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_column: Option<u64>,
    /// An empty cell removes the stored value instead of leaving it as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_clears: Option<bool>,
    /// A cell that does not validate is left out instead of failing its row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_invalid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ImportMappingColumn>>,
}

#[derive(Serialize, ToSchema)]
pub struct DraftView {
    pub count: usize,
    pub steps: Vec<ModelStep>,
    pub base_revision: Option<u64>,
    pub current_revision: u64,
    pub conflict: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consequence: Option<DraftConsequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definitions: Option<braian_model::DefinitionSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ModelSaveResult {
    pub applied: bool,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consequence: Option<DraftConsequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undone: Option<bool>,
}
