use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use openpim_model::{Object, ObjectId, Uuid, ValidationError};
use openpim_store::{Conflict, Error, ObjectOp, PrincipalId, StoredObject, WriteReceipt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::Auth;
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

#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeResponse {
    pub seq: i64,
    pub version: Option<u64>,
    pub op: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Object)]
    pub diff: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IncludeQuery {
    include: Option<String>,
}

#[utoipa::path(
    post,
    path = "/v1/objects",
    tag = "objects",
    request_body(content = CreateObject),
    responses(
        (status = 200, description = "Created object", body = ObjectResponse),
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
    Ok(object_response(&cached, &stored, receipt.last_seq, false))
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
    let seq = latest_seq(&state, id).await?;
    Ok(object_response(
        &cached,
        &stored,
        seq,
        includes_inapplicable(&query),
    ))
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
                openpim_model::apply_object_patch(&mut object, &patch);
                let stored = StoredObject {
                    key: String::new(),
                    version: version_of(&receipt, id),
                    deleted_at: None,
                    object,
                };
                return Ok(object_response(&cached, &stored, receipt.last_seq, false));
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
    Ok(Json(DeletedResponse {
        version: version_of(&receipt, id),
        seq: receipt.last_seq,
    }))
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
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        changes.push(ChangeResponse {
            seq: row.seq,
            version: row.version,
            op: row.op.as_str().to_owned(),
            actor: principal_name(&state, row.actor).await?,
            on_behalf_of: match row.on_behalf_of {
                Some(id) => Some(principal_name(&state, id).await?),
                None => None,
            },
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
) -> Response {
    let applicable = cached
        .model
        .applicable_ids(stored.object.type_id, &stored.object.groups)
        .unwrap_or_default();
    let warnings = cached.model.validate(&stored.object);
    let body = ObjectResponse {
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
    };
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

async fn latest_seq(state: &AppState, id: ObjectId) -> Result<i64, Problem> {
    let changes = state.store.changes_for(id).await.map_err(store_status)?;
    Ok(changes.last().map(|change| change.seq).unwrap_or(0))
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
    query
        .include
        .as_deref()
        .is_some_and(|value| value.split(',').any(|item| item.trim() == "inapplicable"))
}

async fn conflict_problem(
    state: &AppState,
    cached: &CachedModel,
    current_version: u64,
    conflicts: Vec<Conflict>,
) -> Problem {
    let mut body = Vec::with_capacity(conflicts.len());
    for conflict in conflicts {
        let actor = principal_name(state, conflict.actor)
            .await
            .unwrap_or_else(|_| conflict.actor.to_string());
        let on_behalf_of = match conflict.on_behalf_of {
            Some(id) => principal_name(state, id).await.ok(),
            None => None,
        };
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

async fn principal_name(state: &AppState, id: PrincipalId) -> Result<String, Problem> {
    match state.store.principal(id).await.map_err(store_status)? {
        Some(record) => Ok(record.handle.unwrap_or_else(|| record.id.to_string())),
        None => Ok(id.to_string()),
    }
}
