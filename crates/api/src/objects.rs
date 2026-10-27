use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use braian_model::{Object, ObjectId, Uuid, ValidationError};
use braian_store::{
    Conflict, Error, ObjectOp, ObjectProjection, PrincipalId, PrincipalKind, ProposalRow,
    StoredObject, WriteReceipt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::{Auth, Identity};
use crate::headers::{etag, if_match, require_if_match};
use crate::problem::{ConflictBody, Problem, store_status};
use crate::state::{AppState, CachedModel};
use crate::wire::{
    create_values, groups_to_wire, object_id, patch_from_wire, resolve_group, resolve_structure,
    resolve_type, type_key, values_to_wire,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateObject {
    pub id: Option<Uuid>,
    #[serde(rename = "type")]
    pub object_type: String,
    #[serde(default)]
    pub groups: BTreeMap<String, String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
}

/// How many ids one batch read accepts.
pub const BATCH_GET_LIMIT: usize = 200;

#[derive(Debug, Deserialize, ToSchema)]
pub struct BatchGetRequest {
    pub ids: Vec<Uuid>,
    /// Defaults to loading groups. They are small and decide which values apply.
    #[serde(default)]
    pub groups: Option<bool>,
    /// Defaults to false. `fields` or `parts` load those value documents anyway.
    #[serde(default)]
    pub values: Option<bool>,
    /// Storage part names, such as `sv#0`.
    #[serde(default)]
    pub parts: Option<Vec<String>>,
    /// Attribute keys. The server maps them to the parts that store them.
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    /// Limits localized `fields` to these language tags.
    #[serde(default)]
    pub languages: Option<Vec<String>>,
    #[serde(default)]
    pub include: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchGetResponse {
    pub objects: Vec<ObjectResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchObject {
    #[serde(default)]
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
    #[serde(default)]
    pub groups: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObjectResponse {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub object_type: String,
    pub groups: BTreeMap<String, String>,
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
    pub version: u64,
    pub seq: i64,
    pub model_revision: u64,
    pub warnings: Vec<ValidationError>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeletedResponse {
    pub version: u64,
    pub seq: i64,
}

/// A write that was filed for a person to accept. Nothing was saved.
#[derive(Debug, Serialize, ToSchema)]
pub struct Suggestion {
    pub proposal_id: Uuid,
    pub needs_approval: bool,
    pub source: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeResponse {
    pub seq: i64,
    pub at: String,
    pub version: Option<u64>,
    pub op: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// `agent` when the actor wrote for a person, `import` for an integration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub diff: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IncludeQuery {
    pub(crate) include: Option<String>,
}

#[utoipa::path(
    post,
    path = "/v1/objects",
    tag = "objects",
    request_body(content = CreateObject),
    responses(
        (status = 200, description = "Created object", body = ObjectResponse),
        (status = 202, description = "Filed as a suggestion. An agent or import does not save.", body = Suggestion),
        (status = 409, description = "Duplicate key", body = crate::problem::Problem),
        (status = 422, description = "Validation failed", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn create_object(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<CreateObject>,
) -> Result<Response, Problem> {
    let cached = state.models.get(&state.store).await?;
    let type_id = resolve_type(&cached.model, &body.object_type)?;
    let mut groups = BTreeMap::new();
    for (structure, group) in &body.groups {
        let structure = resolve_structure(&cached.model, &cached.defs, structure)?;
        groups.insert(structure, resolve_group(&cached.defs, structure, group)?);
    }
    let object = Object {
        id: object_id(body.id)?,
        type_id,
        model_revision: cached.revision,
        groups,
        values: create_values(&cached.model, &body.values)?,
    };
    let errors = cached.model.validate(&object);
    if !errors.is_empty() {
        return Err(Problem::validation(errors, Vec::new()));
    }
    if !identity.may_commit() {
        return file_suggestion(
            &state,
            &identity,
            SuggestionDraft {
                title: "Create object",
                object_id: None,
                object_key: wire_key(&cached.model, &body.object_type, &body.values),
                type_key: Some(body.object_type.clone()),
                base_version: None,
                patch: serde_json::json!({ "values": body.values, "groups": body.groups }),
            },
        )
        .await;
    }
    let receipt = state
        .store
        .write_batch(&identity.actor, vec![ObjectOp::Create(object.clone())])
        .await
        .map_err(store_status)?;
    let stored = StoredObject {
        key: String::new(),
        version: version_of(&receipt, object.id),
        deleted_at: None,
        object,
    };
    note(
        &state,
        "create",
        stored.object.id.as_uuid(),
        stored.version,
        receipt.last_seq,
        &body.object_type,
    );
    Ok(object_response(
        &cached,
        &stored,
        receipt.last_seq,
        false,
        true,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/objects/{id}",
    tag = "objects",
    params(
        ("id" = Uuid, Path, description = "Object id"),
        ("include" = Option<String>, Query, description = "Pass `inapplicable` to include values that no longer apply")
    ),
    responses(
        (status = 200, description = "Object", body = ObjectResponse),
        (status = 404, description = "Not found", body = crate::problem::Problem),
        (status = 410, description = "Deleted", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn get_object(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<IncludeQuery>,
) -> Result<Response, Problem> {
    let cached = state.models.get(&state.store).await?;
    let id = ObjectId::from_uuid(id);
    let stored = load(&state, id).await?;
    let seq = state.store.latest_seq(id).await.map_err(store_status)?;
    Ok(object_response(
        &cached,
        &stored,
        seq,
        includes_inapplicable(&query),
        true,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/objects:batchGet",
    tag = "objects",
    request_body(content = BatchGetRequest),
    responses(
        (status = 200, description = "Objects in request order. Missing and deleted ids are omitted.", body = BatchGetResponse),
        (status = 400, description = "Too many ids", body = crate::problem::Problem),
        (status = 422, description = "Unknown field", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn batch_get(
    State(state): State<AppState>,
    Auth(_): Auth,
    Json(body): Json<BatchGetRequest>,
) -> Result<Json<BatchGetResponse>, Problem> {
    if body.ids.len() > BATCH_GET_LIMIT {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "batch_too_large",
            format!("batch get accepts at most {BATCH_GET_LIMIT} ids"),
        ));
    }
    let cached = state.models.get(&state.store).await?;
    let projection = projection_for(&state, &cached, &body).await?;
    let full_values = projection.values && projection.parts.is_none();
    let ids: Vec<ObjectId> = body.ids.iter().copied().map(ObjectId::from_uuid).collect();
    let loaded = state
        .store
        .get_objects_projected(&ids, &projection)
        .await
        .map_err(store_status)?;
    let seqs = state.store.latest_seqs(&ids).await.map_err(store_status)?;
    let include = includes_inapplicable_text(body.include.as_deref());
    let mut objects = Vec::with_capacity(loaded.len());
    for stored in loaded {
        if stored.deleted_at.is_some() {
            continue;
        }
        let seq = seqs.get(&stored.object.id).copied().unwrap_or(0);
        objects.push(object_body(&cached, &stored, seq, include, full_values));
    }
    Ok(Json(BatchGetResponse { objects }))
}

#[utoipa::path(
    patch,
    path = "/v1/objects/{id}",
    tag = "objects",
    params(
        ("id" = Uuid, Path, description = "Object id"),
        ("If-Match" = u64, Header, description = "Object version the edit started from")
    ),
    request_body(content = PatchObject),
    responses(
        (status = 200, description = "Updated object", body = ObjectResponse),
        (status = 202, description = "Filed as a suggestion. An agent or import does not save.", body = Suggestion),
        (status = 409, description = "Conflict", body = crate::problem::Problem),
        (status = 422, description = "Validation failed", body = crate::problem::Problem),
        (status = 428, description = "If-Match is required", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn patch_object(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<PatchObject>,
) -> Result<Response, Problem> {
    let base = require_if_match(&headers)?;
    let id = ObjectId::from_uuid(id);
    for _ in 0..3 {
        let cached = state.models.get(&state.store).await?;
        let stored = load(&state, id).await?;
        let patch = patch_from_wire(&cached.model, &cached.defs, &body.values, &body.groups)?;
        let report = cached.model.validate_patch(&stored.object, &patch);
        if !report.errors.is_empty() {
            return Err(Problem::validation(report.errors, report.warnings));
        }
        if !identity.may_commit() {
            return file_suggestion(
                &state,
                &identity,
                SuggestionDraft {
                    title: "Edit object",
                    object_id: Some(id.as_uuid()),
                    object_key: Some(stored.key.clone()),
                    type_key: Some(type_key(&cached.defs, stored.object.type_id)),
                    base_version: Some(i64::try_from(base).unwrap_or(i64::MAX)),
                    patch: serde_json::json!({ "values": body.values, "groups": body.groups }),
                },
            )
            .await;
        }
        match state
            .store
            .write_batch(
                &identity.actor,
                vec![ObjectOp::Update {
                    id,
                    patch: patch.clone(),
                    base_version: base,
                    read_version: stored.version,
                }],
            )
            .await
        {
            Ok(receipt) => {
                let mut object = stored.object;
                braian_model::apply_object_patch(&mut object, &patch);
                let stored = StoredObject {
                    key: String::new(),
                    version: version_of(&receipt, id),
                    deleted_at: None,
                    object,
                };
                note(
                    &state,
                    "update",
                    id.as_uuid(),
                    stored.version,
                    receipt.last_seq,
                    &type_key(&cached.defs, stored.object.type_id),
                );
                return Ok(object_response(
                    &cached,
                    &stored,
                    receipt.last_seq,
                    false,
                    true,
                ));
            }
            Err(Error::StaleRead { .. }) => continue,
            Err(Error::Conflict {
                current_version,
                conflicts,
            }) => {
                return Err(conflict_problem(&state, &cached, current_version, conflicts).await);
            }
            Err(error) => return Err(store_status(error)),
        }
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "stale_read",
        "the object changed while the write was being checked",
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/objects/{id}",
    tag = "objects",
    params(
        ("id" = Uuid, Path, description = "Object id"),
        ("If-Match" = Option<u64>, Header, description = "When set, any newer change is a conflict")
    ),
    responses(
        (status = 200, description = "Deleted", body = DeletedResponse),
        (status = 403, description = "An agent or import cannot delete", body = crate::problem::Problem),
        (status = 409, description = "Conflict", body = crate::problem::Problem),
        (status = 410, description = "Already deleted", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_object(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<DeletedResponse>, Problem> {
    if !identity.may_commit() {
        return Err(crate::auth::delete_forbidden());
    }
    let id = ObjectId::from_uuid(id);
    let base = if_match(&headers)?;
    let receipt = match state
        .store
        .write_batch(
            &identity.actor,
            vec![ObjectOp::Delete {
                id,
                base_version: base,
            }],
        )
        .await
    {
        Ok(receipt) => receipt,
        Err(Error::Conflict {
            current_version,
            conflicts,
        }) => {
            let cached = state.models.get(&state.store).await?;
            return Err(conflict_problem(&state, &cached, current_version, conflicts).await);
        }
        Err(error) => return Err(store_status(error)),
    };
    let version = version_of(&receipt, id);
    note(
        &state,
        "delete",
        id.as_uuid(),
        version,
        receipt.last_seq,
        "",
    );
    Ok(Json(DeletedResponse {
        version,
        seq: receipt.last_seq,
    }))
}

/// Business key shown on a create suggestion.
///
/// The name comes from the type's key attribute. `number` is only used when
/// that type is not in the model.
fn wire_key(
    model: &braian_rules::CompiledModel,
    type_key: &str,
    values: &BTreeMap<String, Value>,
) -> Option<String> {
    match model.type_id(type_key) {
        Some(type_id) => {
            let attribute_id = model.key_attribute(type_id)?;
            let name = model.attribute(attribute_id)?.key.as_str();
            values.get(name).and_then(Value::as_str).map(str::to_owned)
        }
        None => values
            .get("number")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestion_key_follows_the_type_key_attribute() {
        let defs: braian_model::DefinitionSet =
            serde_json::from_str(include_str!("../../server/demo/definitions.json"))
                .expect("demo definitions");
        let model = braian_rules::CompiledModel::compile(&defs).expect("compile");
        let values = BTreeMap::from([
            ("number".into(), Value::String("ART-1".into())),
            ("sku".into(), Value::String("OLD".into())),
        ]);
        assert_eq!(
            wire_key(&model, "article", &values).as_deref(),
            Some("ART-1")
        );
        let only_old = BTreeMap::from([("sku".into(), Value::String("OLD".into()))]);
        assert_eq!(wire_key(&model, "article", &only_old), None);
        assert_eq!(wire_key(&model, "missing", &only_old), None);
        let numbered = BTreeMap::from([("number".into(), Value::String("ART-9".into()))]);
        assert_eq!(
            wire_key(&model, "missing", &numbered).as_deref(),
            Some("ART-9")
        );
    }
}

struct SuggestionDraft {
    title: &'static str,
    object_id: Option<Uuid>,
    object_key: Option<String>,
    type_key: Option<String>,
    base_version: Option<i64>,
    patch: Value,
}

async fn file_suggestion(
    state: &AppState,
    identity: &Identity,
    draft: SuggestionDraft,
) -> Result<Response, Problem> {
    let id = Uuid::now_v7();
    let source = crate::auth::suggestion_source(identity.kind);
    let row = ProposalRow {
        id: Uuid::now_v7(),
        object_id: draft.object_id,
        object_key: draft.object_key,
        type_key: draft.type_key,
        base_version: draft.base_version,
        patch: draft.patch,
        status: "pending".into(),
        conflict: false,
        position: 0,
    };
    state
        .store
        .create_proposal(
            id,
            draft.title,
            source,
            identity.actor.actor,
            identity.actor.on_behalf_of,
            false,
            &[row],
        )
        .await
        .map_err(store_status)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(Suggestion {
            proposal_id: id,
            needs_approval: true,
            source: source.to_owned(),
        }),
    )
        .into_response())
}

fn note(state: &AppState, op: &str, id: Uuid, version: u64, seq: i64, type_key: &str) {
    state.emit(serde_json::json!({
        "seq": seq,
        "op": op,
        "object_id": id,
        "version": version,
        "type": type_key,
    }));
    state.index_after_commit(id, seq);
}

#[utoipa::path(
    get,
    path = "/v1/objects/{id}/changes",
    tag = "objects",
    params(("id" = Uuid, Path, description = "Object id")),
    responses((status = 200, description = "Change log", body = Vec<ChangeResponse>)),
    security(("bearer" = []))
)]
pub async fn object_changes(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ChangeResponse>>, Problem> {
    let id = ObjectId::from_uuid(id);
    let stored = state.store.get_object(id).await.map_err(store_status)?;
    if stored.is_none() {
        return Err(store_status(Error::NotFound(id)));
    }
    let rows = state.store.changes_for(id).await.map_err(store_status)?;
    let mut ids = Vec::new();
    for row in &rows {
        ids.push(row.actor);
        if let Some(id) = row.on_behalf_of {
            ids.push(id);
        }
    }
    ids.sort_by_key(|id| id.as_uuid());
    ids.dedup();
    let names = principal_labels(&state, &ids).await?;
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let actor_face = names.get(&row.actor);
        let behalf = row.on_behalf_of.and_then(|id| names.get(&id));
        let via = if behalf.is_some() {
            Some("agent".to_owned())
        } else if actor_face.is_some_and(|face| face.kind == PrincipalKind::Integration) {
            Some("import".to_owned())
        } else {
            None
        };
        changes.push(ChangeResponse {
            seq: row.seq,
            at: row.at.to_rfc3339(),
            version: row.version,
            op: row.op.as_str().to_owned(),
            actor: principal_label(&names, row.actor),
            on_behalf_of: behalf.map(|face| face.label.clone()),
            via,
            diff: row.diff,
        });
    }
    Ok(Json(changes))
}

fn object_response(
    cached: &CachedModel,
    stored: &StoredObject,
    seq: i64,
    include_inapplicable: bool,
    validate: bool,
) -> Response {
    let body = object_body(cached, stored, seq, include_inapplicable, validate);
    (
        StatusCode::OK,
        [(header::ETAG, etag(stored.version))],
        Json(body),
    )
        .into_response()
}

async fn load(state: &AppState, id: ObjectId) -> Result<StoredObject, Problem> {
    match state.store.get_object(id).await.map_err(store_status)? {
        Some(stored) if stored.deleted_at.is_some() => Err(store_status(Error::Deleted(id))),
        Some(stored) => Ok(stored),
        None => Err(store_status(Error::NotFound(id))),
    }
}

fn object_body(
    cached: &CachedModel,
    stored: &StoredObject,
    seq: i64,
    include_inapplicable: bool,
    validate: bool,
) -> ObjectResponse {
    let applicable = cached
        .model
        .applicable_ids(stored.object.type_id, &stored.object.groups)
        .unwrap_or_default();
    // A partial projection is missing attributes on purpose. Validating it
    // would report those gaps as if the stored object were incomplete.
    let warnings = if validate {
        cached.model.validate(&stored.object)
    } else {
        Vec::new()
    };
    ObjectResponse {
        id: stored.object.id.as_uuid(),
        object_type: type_key(&cached.defs, stored.object.type_id),
        groups: groups_to_wire(&cached.defs, &stored.object.groups),
        values: values_to_wire(
            &cached.model,
            &applicable,
            &stored.object.values,
            include_inapplicable,
        ),
        version: stored.version,
        seq,
        model_revision: stored.object.model_revision,
        warnings,
    }
}

async fn projection_for(
    state: &AppState,
    cached: &CachedModel,
    body: &BatchGetRequest,
) -> Result<ObjectProjection, Problem> {
    let groups = body.groups.unwrap_or(true);
    if let Some(parts) = &body.parts {
        return Ok(ObjectProjection {
            groups,
            values: true,
            parts: Some(parts.clone()),
        });
    }
    if let Some(fields) = &body.fields
        && !fields.is_empty()
    {
        let mut ids = Vec::with_capacity(fields.len());
        for field in fields {
            ids.push(crate::wire::resolve_attribute(&cached.model, field)?);
        }
        let parts = state
            .store
            .parts_for_attributes(&ids, body.languages.as_deref())
            .await
            .map_err(store_status)?;
        return Ok(ObjectProjection {
            groups,
            values: true,
            parts: Some(parts),
        });
    }
    Ok(ObjectProjection {
        groups,
        values: body.values.unwrap_or(false),
        parts: None,
    })
}

fn version_of(receipt: &WriteReceipt, id: ObjectId) -> u64 {
    receipt
        .versions
        .iter()
        .find(|(object, _)| *object == id)
        .map(|(_, version)| *version)
        .unwrap_or(0)
}

fn includes_inapplicable(query: &IncludeQuery) -> bool {
    includes_inapplicable_text(query.include.as_deref())
}

fn includes_inapplicable_text(include: Option<&str>) -> bool {
    include.is_some_and(|value| value.split(',').any(|item| item.trim() == "inapplicable"))
}

async fn conflict_problem(
    state: &AppState,
    cached: &CachedModel,
    current_version: u64,
    conflicts: Vec<Conflict>,
) -> Problem {
    let mut ids = Vec::new();
    for conflict in &conflicts {
        ids.push(conflict.actor);
        if let Some(id) = conflict.on_behalf_of {
            ids.push(id);
        }
    }
    ids.sort_by_key(|id| id.as_uuid());
    ids.dedup();
    let names = principal_labels(state, &ids).await.unwrap_or_default();
    let mut body = Vec::with_capacity(conflicts.len());
    for conflict in conflicts {
        let actor = principal_label(&names, conflict.actor);
        let on_behalf_of = conflict.on_behalf_of.map(|id| principal_label(&names, id));
        body.push(ConflictBody {
            attribute: conflict.attribute.map(|id| {
                cached
                    .model
                    .attribute(id)
                    .map(|attribute| attribute.key.clone())
                    .unwrap_or_else(|| id.as_uuid().to_string())
            }),
            language: conflict.language.map(|tag| tag.to_string()),
            structure: conflict.structure.map(|id| {
                cached
                    .defs
                    .structures
                    .iter()
                    .find(|structure| structure.id == id)
                    .map(|structure| structure.key.clone())
                    .unwrap_or_else(|| id.as_uuid().to_string())
            }),
            version: conflict.version,
            actor,
            on_behalf_of,
            current: conflict.current,
        });
    }
    let mut problem = Problem::new(
        StatusCode::CONFLICT,
        "conflict",
        "the edit overlaps a newer change",
    );
    problem.current_version = Some(current_version);
    problem.conflicts = body;
    problem
}

struct PrincipalFace {
    label: String,
    kind: PrincipalKind,
}

async fn principal_labels(
    state: &AppState,
    ids: &[PrincipalId],
) -> Result<HashMap<PrincipalId, PrincipalFace>, Problem> {
    let mut names = HashMap::new();
    for record in state.store.principals(ids).await.map_err(store_status)? {
        let id = record.id;
        names.insert(
            id,
            PrincipalFace {
                label: record.handle.clone().unwrap_or_else(|| id.to_string()),
                kind: record.kind,
            },
        );
    }
    Ok(names)
}

fn principal_label(names: &HashMap<PrincipalId, PrincipalFace>, id: PrincipalId) -> String {
    names
        .get(&id)
        .map(|face| face.label.clone())
        .unwrap_or_else(|| id.to_string())
}
