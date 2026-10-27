//! Link types and the links between two objects.
//!
//! A person saves a link type through the same model step as Review and save.
//! An agent files a proposal for a link, and cannot save or delete a link type.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use braian_model::{ModelChange, Object, ObjectId, ObjectPatch, Uuid};
use braian_store::{LinkView, ObjectOp, StoredObject};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::auth::{Auth, delete_forbidden};
use crate::deletes::{ConsequenceReport, DryRun};
use crate::headers::require_if_match;
use crate::objects::{
    SuggestionDraft, file_suggestion, load_object, note_write, object_response, version_of,
};
use crate::problem::{Problem, store_status};
use crate::state::{AppState, CachedModel};
use crate::wire::{create_values, object_id, patch_from_wire, resolve_type, type_key};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateLink {
    pub id: Option<Uuid>,
    #[serde(rename = "type")]
    pub link_type: String,
    pub from_id: Uuid,
    pub to_id: Uuid,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchLink {
    #[serde(default)]
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
    pub from_id: Option<Uuid>,
    pub to_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkTypeBody {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub from_types: Vec<String>,
    pub to_types: Vec<String>,
    pub from_key: String,
    #[serde(default)]
    pub from_label: String,
    pub to_key: String,
    #[serde(default)]
    pub to_label: String,
    #[serde(default = "many")]
    pub from_cardinality: String,
    #[serde(default = "many")]
    pub to_cardinality: String,
    #[serde(default)]
    pub attributes: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkTypePatch {
    pub label: Option<String>,
    pub from_types: Option<Vec<String>>,
    pub to_types: Option<Vec<String>>,
    pub from_key: Option<String>,
    pub from_label: Option<String>,
    pub to_key: Option<String>,
    pub to_label: Option<String>,
    pub from_cardinality: Option<String>,
    pub to_cardinality: Option<String>,
    pub attributes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct LinksQuery {
    pub object: Option<Uuid>,
}

fn many() -> String {
    "many".to_owned()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LinkSide {
    pub link_type: String,
    pub label: String,
    pub direction: String,
    pub links: Vec<LinkItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LinkItem {
    pub id: Uuid,
    pub peer_id: Uuid,
    pub peer_key: String,
    pub peer_type: String,
    pub has_values: bool,
}

pub fn sides(defs: &braian_model::DefinitionSet, views: &[LinkView]) -> Vec<LinkSide> {
    let mut order = Vec::new();
    let mut grouped: BTreeMap<(String, bool), Vec<LinkItem>> = BTreeMap::new();
    for view in views {
        let key = type_key(defs, view.link_type_id);
        let entry = grouped.entry((key.clone(), view.outgoing)).or_default();
        if entry.is_empty() {
            order.push((key, view.outgoing, view.link_type_id));
        }
        entry.push(LinkItem {
            id: view.id.as_uuid(),
            peer_id: view.peer_id.as_uuid(),
            peer_key: view.peer_key.clone(),
            peer_type: type_key(defs, view.peer_type_id),
            has_values: view.has_values,
        });
    }
    order
        .into_iter()
        .map(|(key, outgoing, type_id)| {
            let label = defs
                .types
                .iter()
                .find(|item| item.id == type_id)
                .and_then(|item| item.link.as_ref())
                .map(|spec| {
                    if outgoing {
                        spec.to_label.text("en")
                    } else {
                        spec.from_label.text("en")
                    }
                })
                .unwrap_or_else(|| key.clone());
            LinkSide {
                link_type: key,
                label,
                direction: if outgoing { "out" } else { "in" }.to_owned(),
                links: grouped
                    .remove(&(type_key(defs, type_id), outgoing))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

pub async fn load_sides(
    state: &AppState,
    defs: &braian_model::DefinitionSet,
    id: ObjectId,
) -> Result<Vec<LinkSide>, Problem> {
    let views = state.store.links_for(id).await.map_err(store_status)?;
    Ok(sides(defs, &views))
}

#[utoipa::path(
    get,
    path = "/v1/links",
    tag = "objects",
    params(("object" = Option<Uuid>, Query, description = "Object whose links are listed, from both directions")),
    responses((status = 200, description = "Links grouped by type and direction", body = Vec<LinkSide>)),
    security(("bearer" = []))
)]
pub async fn list_links(
    State(state): State<AppState>,
    Auth(_): Auth,
    Query(query): Query<LinksQuery>,
) -> Result<Json<Vec<LinkSide>>, Problem> {
    let Some(object) = query.object else {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "missing_id",
            "name the object",
        ));
    };
    let cached = state.models.get(&state.store).await?;
    Ok(Json(
        load_sides(&state, &cached.defs, ObjectId::from_uuid(object)).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/links",
    tag = "objects",
    request_body(content = CreateLink),
    responses(
        (status = 200, description = "Created link", body = crate::objects::ObjectResponse),
        (status = 202, description = "Filed as a suggestion", body = crate::objects::Suggestion),
        (status = 409, description = "Cardinality", body = crate::problem::Problem),
        (status = 422, description = "Not allowed", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn post_link(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<CreateLink>,
) -> Result<Response, Problem> {
    let cached = state.models.get(&state.store).await?;
    let op = link_create(&cached, &body)?;
    let ObjectOp::CreateLink { object, .. } = &op else {
        unreachable!("link_create returns CreateLink");
    };
    let created = object.clone();
    if !identity.may_commit() {
        return file_suggestion(
            &state,
            &identity,
            SuggestionDraft {
                title: "Create link",
                object_id: None,
                object_key: None,
                type_key: Some(body.link_type.clone()),
                base_version: None,
                patch: json!({ "link": link_json(&body) }),
            },
        )
        .await;
    }
    let receipt = state
        .store
        .write_batch(&identity.actor, vec![op])
        .await
        .map_err(store_status)?;
    let stored = StoredObject {
        key: String::new(),
        version: version_of(&receipt, created.id),
        deleted_at: None,
        object: created,
    };
    note_write(
        &state,
        "create",
        stored.object.id.as_uuid(),
        stored.version,
        receipt.last_seq,
        &body.link_type,
    );
    Ok(object_response(&state, &cached, &stored, receipt.last_seq, false, true).await)
}

#[utoipa::path(
    patch,
    path = "/v1/links/{id}",
    tag = "objects",
    params(
        ("id" = Uuid, Path, description = "Link id"),
        ("If-Match" = u64, Header, description = "Link version the edit started from")
    ),
    request_body(content = PatchLink),
    responses(
        (status = 200, description = "Updated link", body = crate::objects::ObjectResponse),
        (status = 202, description = "Filed as a suggestion", body = crate::objects::Suggestion),
        (status = 428, description = "If-Match is required", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn patch_link(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<PatchLink>,
) -> Result<Response, Problem> {
    let base = require_if_match(&headers)?;
    let id = ObjectId::from_uuid(id);
    let cached = state.models.get(&state.store).await?;
    let stored = load_object(&state, id).await?;
    let patch = patch_from_wire(&cached.model, &cached.defs, &body.values, &BTreeMap::new())?;
    let report = cached.model.validate_patch(&stored.object, &patch);
    if !report.errors.is_empty() {
        return Err(Problem::validation(report.errors, report.warnings));
    }
    if !identity.may_commit() {
        return file_suggestion(
            &state,
            &identity,
            SuggestionDraft {
                title: "Edit link",
                object_id: Some(id.as_uuid()),
                object_key: None,
                type_key: Some(type_key(&cached.defs, stored.object.type_id)),
                base_version: Some(i64::try_from(base).unwrap_or(i64::MAX)),
                patch: json!({ "link": body_values(&body) }),
            },
        )
        .await;
    }
    let receipt = state
        .store
        .write_batch(
            &identity.actor,
            vec![ObjectOp::UpdateLink {
                id,
                patch: patch.clone(),
                base_version: base,
                read_version: stored.version,
                from_id: body.from_id.map(ObjectId::from_uuid),
                to_id: body.to_id.map(ObjectId::from_uuid),
            }],
        )
        .await
        .map_err(store_status)?;
    let mut object = stored.object;
    braian_model::apply_object_patch(&mut object, &patch);
    let stored = StoredObject {
        key: String::new(),
        version: version_of(&receipt, id),
        deleted_at: None,
        object,
    };
    note_write(
        &state,
        "update",
        id.as_uuid(),
        stored.version,
        receipt.last_seq,
        &type_key(&cached.defs, stored.object.type_id),
    );
    Ok(object_response(&state, &cached, &stored, receipt.last_seq, false, true).await)
}

#[utoipa::path(
    delete,
    path = "/v1/links/{id}",
    tag = "objects",
    params(
        ("id" = Uuid, Path, description = "Link id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 200, description = "Deleted, or the consequence analysis when dry_run is set", body = ConsequenceReport),
        (status = 403, description = "An agent or import cannot delete", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_link(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<DryRun>,
) -> Result<Response, Problem> {
    if !identity.may_commit() {
        return Err(delete_forbidden());
    }
    let id = ObjectId::from_uuid(id);
    if query.dry_run {
        let _stored = load_object(&state, id).await?;
        let report = ConsequenceReport {
            objects: 1,
            links: 1,
            values: 0,
            selections: 0,
            destructive: true,
            confirm_name: String::new(),
            sentence: "Removes 1 link".to_owned(),
            detail: vec!["Removes 1 link".to_owned()],
        };
        return Ok(Json(report).into_response());
    }
    let receipt = state
        .store
        .write_batch(
            &identity.actor,
            vec![ObjectOp::Delete {
                id,
                base_version: None,
            }],
        )
        .await
        .map_err(store_status)?;
    note_write(
        &state,
        "delete",
        id.as_uuid(),
        version_of(&receipt, id),
        receipt.last_seq,
        "",
    );
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/v1/link-types",
    tag = "definitions",
    params(("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")),
    request_body(content = LinkTypeBody),
    responses(
        (status = 204, description = "Link type saved"),
        (status = 200, description = "Consequence analysis")
    ),
    security(("bearer" = []))
)]
pub async fn post_link_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<DryRun>,
    Json(body): Json<LinkTypeBody>,
) -> Result<Response, Problem> {
    let saved = crate::definitions::commit_model_change(
        &state,
        &identity,
        create_change(&body)?,
        query.dry_run,
    )
    .await?;
    if query.dry_run {
        return Ok(Json(saved).into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    patch,
    path = "/v1/link-types/{key}",
    tag = "definitions",
    params(
        ("key" = String, Path, description = "Link type key"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    request_body(content = LinkTypePatch),
    responses(
        (status = 204, description = "Link type updated"),
        (status = 200, description = "Consequence analysis")
    ),
    security(("bearer" = []))
)]
pub async fn patch_link_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(key): Path<String>,
    Query(query): Query<DryRun>,
    Json(body): Json<LinkTypePatch>,
) -> Result<Response, Problem> {
    let saved = crate::definitions::commit_model_change(
        &state,
        &identity,
        ModelChange::UpdateLinkType {
            key,
            label: body.label,
            from_types: body.from_types,
            to_types: body.to_types,
            from_key: body.from_key,
            from_label: body.from_label,
            to_key: body.to_key,
            to_label: body.to_label,
            from_cardinality: body.from_cardinality,
            to_cardinality: body.to_cardinality,
            attributes: body.attributes,
        },
        query.dry_run,
    )
    .await?;
    if query.dry_run {
        return Ok(Json(saved).into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    delete,
    path = "/v1/link-types/{key}",
    tag = "definitions",
    params(
        ("key" = String, Path, description = "Link type key"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 200, description = "Consequence analysis", body = ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_link_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(key): Path<String>,
    Query(query): Query<DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        ModelChange::DeleteLinkType { key },
    )
    .await
}

pub fn link_create(cached: &CachedModel, body: &CreateLink) -> Result<ObjectOp, Problem> {
    let type_id = resolve_type(&cached.model, &body.link_type)?;
    let values = create_values(&cached.model, &body.values)?;
    let object = Object {
        id: object_id(body.id)?,
        type_id,
        model_revision: cached.revision,
        groups: BTreeMap::new(),
        values,
    };
    let errors = cached.model.validate(&object);
    if !errors.is_empty() {
        return Err(Problem::validation(errors, Vec::new()));
    }
    Ok(ObjectOp::CreateLink {
        object,
        from_id: ObjectId::from_uuid(body.from_id),
        to_id: ObjectId::from_uuid(body.to_id),
    })
}

pub fn link_update(
    cached: &CachedModel,
    stored: &StoredObject,
    body: &PatchLink,
    base: u64,
) -> Result<ObjectOp, Problem> {
    let patch = patch_from_wire(&cached.model, &cached.defs, &body.values, &BTreeMap::new())?;
    checked_patch(cached, stored, &patch)?;
    Ok(ObjectOp::UpdateLink {
        id: stored.object.id,
        patch,
        base_version: base,
        read_version: stored.version,
        from_id: body.from_id.map(ObjectId::from_uuid),
        to_id: body.to_id.map(ObjectId::from_uuid),
    })
}

fn checked_patch(
    cached: &CachedModel,
    stored: &StoredObject,
    patch: &ObjectPatch,
) -> Result<(), Problem> {
    let report = cached.model.validate_patch(&stored.object, patch);
    if report.errors.is_empty() {
        Ok(())
    } else {
        Err(Problem::validation(report.errors, report.warnings))
    }
}

fn create_change(body: &LinkTypeBody) -> Result<ModelChange, Problem> {
    if body.key.trim().is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "model",
            "missing key",
        ));
    }
    Ok(ModelChange::CreateLinkType {
        key: body.key.clone(),
        label: body.label.clone(),
        from_types: body.from_types.clone(),
        to_types: body.to_types.clone(),
        from_key: body.from_key.clone(),
        from_label: body.from_label.clone(),
        to_key: body.to_key.clone(),
        to_label: body.to_label.clone(),
        from_cardinality: body.from_cardinality.clone(),
        to_cardinality: body.to_cardinality.clone(),
        attributes: body.attributes.clone(),
    })
}

fn link_json(body: &CreateLink) -> Value {
    json!({
        "type": body.link_type,
        "from_id": body.from_id,
        "to_id": body.to_id,
        "values": body.values,
    })
}

fn body_values(body: &PatchLink) -> Value {
    json!({
        "values": body.values,
        "from_id": body.from_id,
        "to_id": body.to_id,
    })
}

pub fn create_body(args: &Value) -> Result<CreateLink, Problem> {
    serde_json::from_value(args.clone())
        .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string()))
}

pub fn patch_body(args: &Value) -> Result<PatchLink, Problem> {
    serde_json::from_value(args.clone())
        .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string()))
}
