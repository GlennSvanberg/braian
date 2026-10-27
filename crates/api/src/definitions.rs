use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use braian_model::{
    Attribute, AttributeId, DefinitionSet, ObjectType, Structure, StructureId, TypeId, UnitFamily,
    UnitFamilyId, Uuid, set_languages, upsert_attribute, upsert_structure, upsert_type,
    upsert_unit_family,
};
use braian_rules::CompiledModel;
use braian_store::{Actor, Error, WriteReceipt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::{Auth, Role};
use crate::headers::{etag, if_match};
use crate::keys::DryQuery;
use crate::platform::require;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct DefinitionWrite {
    pub model_revision: u64,
    pub seq: i64,
}

#[utoipa::path(
    get,
    path = "/v1/definitions",
    tag = "definitions",
    responses(
        (status = 200, description = "Current definition set", body = braian_model::DefinitionSet),
        (status = 404, description = "No definitions stored", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn get_definitions(
    State(state): State<AppState>,
    Auth(_): Auth,
    headers: HeaderMap,
) -> Result<Response, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let yaml = wants_yaml(&headers);
    let body = if yaml {
        serde_norway::to_string(&defs).map_err(|error| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                error.to_string(),
            )
        })?
    } else {
        serde_json::to_string(&defs).map_err(|error| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                error.to_string(),
            )
        })?
    };
    let content_type = if yaml {
        "application/yaml"
    } else {
        "application/json"
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::ETAG, &etag(defs.model_revision)),
        ],
        body,
    )
        .into_response())
}

#[utoipa::path(
    put,
    path = "/v1/definitions",
    tag = "definitions",
    request_body(content = braian_model::DefinitionSet, description = "JSON or YAML definition set"),
    params(("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")),
    responses(
        (status = 200, description = "Imported, or a dry run", body = DefinitionWrite),
        (status = 409, description = "Revision conflict", body = crate::problem::Problem),
        (status = 422, description = "The set does not compile", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn put_definitions(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<crate::deletes::DryRun>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Problem> {
    if !query.dry_run {
        crate::auth::require_commit(&identity)?;
    }
    let mut defs: DefinitionSet = if content_is_yaml(&headers) {
        serde_norway::from_slice(&body).map_err(|error| bad_body(error.to_string()))?
    } else {
        serde_json::from_slice(&body).map_err(|error| bad_body(error.to_string()))?
    };
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    if let Some(matched) = if_match(&headers)?
        && matched != current
    {
        return Err(Problem::new(
            StatusCode::PRECONDITION_FAILED,
            "precondition_failed",
            format!("definitions are at revision {current}"),
        ));
    }
    if query.dry_run {
        let stored = state.store.definition_set().await.map_err(store_status)?;
        let impact = braian_model::model_diff(&stored, &defs);
        return Ok(Json(serde_json::json!({
            "applied": false,
            "lines": impact.lines,
            "warnings": impact.warnings,
        }))
        .into_response());
    }
    defs.model_revision = current + 1;
    CompiledModel::compile(&defs).map_err(Problem::definitions)?;
    let receipt = match state.store.put_definitions(&identity.actor, &defs).await {
        Ok(receipt) => receipt,
        Err(Error::BadRevision { current, got }) => {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "revision_conflict",
                format!("definition revision {got} does not follow {current}"),
            ));
        }
        Err(error) => return Err(store_status(error)),
    };
    state.models.invalidate().await;
    Ok(write_response(defs.model_revision, receipt))
}

#[utoipa::path(
    post,
    path = "/v1/attributes",
    tag = "definitions",
    request_body(content = braian_model::Attribute, description = "Id is optional"),
    responses(
        (status = 200, description = "Stored attribute", body = braian_model::Attribute),
        (status = 422, description = "The set does not compile", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn post_attribute(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(mut body): Json<Value>,
) -> Result<Json<Attribute>, Problem> {
    ensure_id(&mut body);
    let attribute: Attribute =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_attribute(defs, attribute.clone());
        Ok(())
    })
    .await?;
    let stored = defs
        .attributes
        .into_iter()
        .find(|item| item.id == attribute.id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "attribute missing",
            )
        })?;
    Ok(Json(stored))
}

#[utoipa::path(
    get,
    path = "/v1/attributes/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Attribute key or id")),
    responses(
        (status = 200, description = "Attribute", body = braian_model::Attribute),
        (status = 404, description = "Unknown attribute", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn get_attribute(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<Attribute>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    defs.attributes
        .into_iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(Json)
        .ok_or_else(|| missing("attribute", &id))
}

#[utoipa::path(
    put,
    path = "/v1/attributes/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Attribute key or id")),
    request_body(content = braian_model::Attribute),
    responses(
        (status = 200, description = "Stored attribute", body = braian_model::Attribute),
        (status = 422, description = "The set does not compile", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn put_attribute(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<Json<Attribute>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let existing = defs
        .attributes
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id));
    let attribute_id = existing
        .map(|item| item.id)
        .or_else(|| Uuid::parse_str(&id).ok().map(AttributeId::from_uuid))
        .ok_or_else(|| missing("attribute", &id))?;
    body["id"] = serde_json::json!(attribute_id);
    let attribute: Attribute =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_attribute(defs, attribute.clone());
        Ok(())
    })
    .await?;
    Ok(Json(
        defs.attributes
            .into_iter()
            .find(|item| item.id == attribute.id)
            .ok_or_else(|| {
                Problem::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "attribute missing",
                )
            })?,
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/attributes/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Attribute key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 200, description = "Consequence analysis", body = crate::deletes::ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = crate::problem::Problem),
        (status = 404, description = "Unknown attribute", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_attribute_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<crate::deletes::DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        braian_model::ModelChange::DeleteAttribute { key: id },
    )
    .await
}

#[utoipa::path(
    post,
    path = "/v1/types",
    tag = "definitions",
    request_body(content = braian_model::ObjectType, description = "Id is optional"),
    responses((status = 200, description = "Stored type", body = braian_model::ObjectType)),
    security(("bearer" = []))
)]
pub async fn post_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(mut body): Json<Value>,
) -> Result<Json<ObjectType>, Problem> {
    ensure_id(&mut body);
    let object_type: ObjectType =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_type(defs, object_type.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_type(defs, object_type.id)?))
}

#[utoipa::path(
    get,
    path = "/v1/types/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Type key or id")),
    responses((status = 200, description = "Type", body = braian_model::ObjectType)),
    security(("bearer" = []))
)]
pub async fn get_type(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<ObjectType>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    defs.types
        .into_iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(Json)
        .ok_or_else(|| missing("type", &id))
}

#[utoipa::path(
    put,
    path = "/v1/types/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Type key or id")),
    request_body(content = braian_model::ObjectType),
    responses((status = 200, description = "Stored type", body = braian_model::ObjectType)),
    security(("bearer" = []))
)]
pub async fn put_type(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<Json<ObjectType>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let type_id = defs
        .types
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .or_else(|| Uuid::parse_str(&id).ok().map(TypeId::from_uuid))
        .ok_or_else(|| missing("type", &id))?;
    body["id"] = serde_json::json!(type_id);
    let object_type: ObjectType =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_type(defs, object_type.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_type(defs, object_type.id)?))
}

#[utoipa::path(
    delete,
    path = "/v1/types/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Type key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted. Objects, values, links and selections of this type are removed."),
        (status = 200, description = "Consequence analysis", body = crate::deletes::ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = crate::problem::Problem),
        (status = 404, description = "Unknown type", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_type_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<crate::deletes::DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        braian_model::ModelChange::DeleteType { key: id },
    )
    .await
}

#[utoipa::path(
    post,
    path = "/v1/structures",
    tag = "definitions",
    request_body(content = braian_model::Structure, description = "Id is optional. Groups and classes belong to the structure."),
    responses((status = 200, description = "Stored structure", body = braian_model::Structure)),
    security(("bearer" = []))
)]
pub async fn post_structure(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(mut body): Json<Value>,
) -> Result<Json<Structure>, Problem> {
    ensure_id(&mut body);
    let structure: Structure =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_structure(defs, structure.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_structure(defs, structure.id)?))
}

#[utoipa::path(
    get,
    path = "/v1/structures/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Structure key or id")),
    responses((status = 200, description = "Structure", body = braian_model::Structure)),
    security(("bearer" = []))
)]
pub async fn get_structure(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<Structure>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    defs.structures
        .into_iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(Json)
        .ok_or_else(|| missing("structure", &id))
}

#[utoipa::path(
    put,
    path = "/v1/structures/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Structure key or id")),
    request_body(content = braian_model::Structure),
    responses((status = 200, description = "Stored structure", body = braian_model::Structure)),
    security(("bearer" = []))
)]
pub async fn put_structure(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<Json<Structure>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_id = defs
        .structures
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .or_else(|| Uuid::parse_str(&id).ok().map(StructureId::from_uuid))
        .ok_or_else(|| missing("structure", &id))?;
    body["id"] = serde_json::json!(structure_id);
    let structure: Structure =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    if let Some(previous) = defs.structures.iter().find(|item| item.id == structure_id) {
        let removed: Vec<_> = previous
            .groups
            .iter()
            .filter(|group| !structure.groups.iter().any(|item| item.id == group.id))
            .map(|group| group.id)
            .collect();
        crate::structures::reject_removed_groups(&state, &removed).await?;
    }
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_structure(defs, structure.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_structure(defs, structure.id)?))
}

#[utoipa::path(
    delete,
    path = "/v1/structures/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Structure key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted. Objects placed in the structure leave the tree."),
        (status = 200, description = "Consequence analysis", body = crate::deletes::ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = crate::problem::Problem),
        (status = 404, description = "Unknown structure", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_structure_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<crate::deletes::DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        braian_model::ModelChange::DeleteStructure { structure: id },
    )
    .await
}

#[utoipa::path(
    post,
    path = "/v1/unit-families",
    tag = "definitions",
    request_body(content = braian_model::UnitFamily, description = "Id is optional"),
    responses((status = 200, description = "Stored unit family", body = braian_model::UnitFamily)),
    security(("bearer" = []))
)]
pub async fn post_unit_family(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(mut body): Json<Value>,
) -> Result<Json<UnitFamily>, Problem> {
    ensure_id(&mut body);
    let family: UnitFamily =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_unit_family(defs, family.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_family(defs, family.id)?))
}

#[utoipa::path(
    get,
    path = "/v1/unit-families/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Unit family key or id")),
    responses((status = 200, description = "Unit family", body = braian_model::UnitFamily)),
    security(("bearer" = []))
)]
pub async fn get_unit_family(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<UnitFamily>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    defs.unit_families
        .into_iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(Json)
        .ok_or_else(|| missing("unit family", &id))
}

#[utoipa::path(
    put,
    path = "/v1/unit-families/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Unit family key or id")),
    request_body(content = braian_model::UnitFamily),
    responses((status = 200, description = "Stored unit family", body = braian_model::UnitFamily)),
    security(("bearer" = []))
)]
pub async fn put_unit_family(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<Json<UnitFamily>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let family_id = defs
        .unit_families
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .or_else(|| Uuid::parse_str(&id).ok().map(UnitFamilyId::from_uuid))
        .ok_or_else(|| missing("unit family", &id))?;
    body["id"] = serde_json::json!(family_id);
    let family: UnitFamily =
        serde_json::from_value(body).map_err(|error| bad_body(error.to_string()))?;
    let (defs, _) = commit(&state, &identity.actor, |defs| {
        upsert_unit_family(defs, family.clone());
        Ok(())
    })
    .await?;
    Ok(Json(find_family(defs, family.id)?))
}

#[utoipa::path(
    delete,
    path = "/v1/unit-families/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Unit family key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 200, description = "Consequence analysis", body = crate::deletes::ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = crate::problem::Problem),
        (status = 404, description = "Unknown unit family", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_unit_family_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<crate::deletes::DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        braian_model::ModelChange::DeleteUnitFamily { key: id },
    )
    .await
}

#[utoipa::path(
    put,
    path = "/v1/languages",
    tag = "definitions",
    request_body(content = Vec<String>, description = "BCP 47 language tags, in order"),
    responses((status = 200, description = "Languages replaced", body = DefinitionWrite)),
    security(("bearer" = []))
)]
pub async fn put_languages(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(languages): Json<Vec<braian_model::LanguageTag>>,
) -> Result<Json<DefinitionWrite>, Problem> {
    let (defs, receipt) = commit(&state, &identity.actor, |defs| {
        set_languages(defs, languages.clone());
        Ok(())
    })
    .await?;
    Ok(Json(DefinitionWrite {
        model_revision: defs.model_revision,
        seq: receipt.last_seq,
    }))
}

pub(crate) async fn commit(
    state: &AppState,
    actor: &Actor,
    edit: impl Fn(&mut DefinitionSet) -> Result<(), Problem>,
) -> Result<(DefinitionSet, WriteReceipt), Problem> {
    ensure_person(state, actor).await?;
    for attempt in 0..2 {
        let mut defs = match state.store.definition_set().await {
            Ok(defs) => defs,
            Err(Error::NoDefinitions) => DefinitionSet {
                model_revision: 0,
                languages: Vec::new(),
                unit_families: Vec::new(),
                value_lists: Vec::new(),
                attributes: Vec::new(),
                types: Vec::new(),
                structures: Vec::new(),
            },
            Err(error) => return Err(store_status(error)),
        };
        let current = defs.model_revision;
        edit(&mut defs)?;
        defs.model_revision = current + 1;
        CompiledModel::compile(&defs).map_err(Problem::definitions)?;
        match state.store.put_definitions(actor, &defs).await {
            Ok(receipt) => {
                state.models.invalidate().await;
                return Ok((defs, receipt));
            }
            Err(Error::BadRevision { .. }) if attempt == 0 => continue,
            Err(error) => return Err(store_status(error)),
        }
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "revision_conflict",
        "definition revision changed",
    ))
}

async fn ensure_person(state: &AppState, actor: &Actor) -> Result<(), Problem> {
    let human = match state
        .store
        .principal(actor.actor)
        .await
        .map_err(store_status)?
    {
        Some(record) => record.kind == braian_store::PrincipalKind::Human,
        None => false,
    };
    if human {
        Ok(())
    } else {
        Err(crate::auth::commit_forbidden())
    }
}

fn write_response(revision: u64, receipt: WriteReceipt) -> Response {
    (
        StatusCode::OK,
        [(header::ETAG, etag(revision))],
        Json(DefinitionWrite {
            model_revision: revision,
            seq: receipt.last_seq,
        }),
    )
        .into_response()
}

fn ensure_id(body: &mut Value) {
    if body.get("id").is_none() || body.get("id").is_some_and(Value::is_null) {
        body["id"] = serde_json::json!(Uuid::now_v7());
    }
}

fn same_ref(id: &str, key: &str, token: &str) -> bool {
    token == id || token == key
}

fn missing(kind: &str, id: &str) -> Problem {
    Problem::new(
        StatusCode::NOT_FOUND,
        "not_found",
        format!("unknown {kind} {id}"),
    )
}

fn bad_body(detail: String) -> Problem {
    Problem::new(StatusCode::BAD_REQUEST, "bad_body", detail)
}

fn wants_yaml(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("yaml"))
}

fn content_is_yaml(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("yaml"))
}

fn find_type(defs: DefinitionSet, id: TypeId) -> Result<ObjectType, Problem> {
    defs.types
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "type missing",
            )
        })
}

fn find_structure(defs: DefinitionSet, id: StructureId) -> Result<Structure, Problem> {
    defs.structures
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "structure missing",
            )
        })
}

fn find_family(defs: DefinitionSet, id: UnitFamilyId) -> Result<UnitFamily, Problem> {
    defs.unit_families
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "unit family missing",
            )
        })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StructuresBody {
    pub structures: Vec<String>,
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StructuresResponse {
    pub structures: Vec<String>,
    pub applied: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnplacedQuery {
    pub structure: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnplacedCount {
    pub structure: String,
    pub count: i64,
}

#[utoipa::path(
    put,
    path = "/v1/types/{id}/structures",
    tag = "definitions",
    params(("id" = String, Path, description = "Type id or key")),
    request_body = StructuresBody,
    responses(
        (status = 200, description = "Allowed trees replaced, or a dry run", body = StructuresResponse),
        (status = 202, description = "Filed as a model proposal", body = Value)
    ),
    security(("bearer" = []))
)]
pub async fn put_type_structures(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<DryQuery>,
    Json(body): Json<StructuresBody>,
) -> Result<Response, Problem> {
    let dry_run = body.dry_run.unwrap_or(false) || query.dry_run.unwrap_or(false);
    if dry_run {
        require(&identity, Role::Viewer)?;
    } else {
        require(&identity, Role::Editor)?;
    }
    let change = braian_model::ModelChange::SetStructures {
        type_key: id.clone(),
        structures: body.structures.clone(),
    };
    let cached = state.models.get(&state.store).await?;
    let (after, impact) = braian_model::preview(&cached.defs, std::slice::from_ref(&change))
        .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "model", error.to_string()))?;
    let object_type = after
        .types
        .iter()
        .find(|item| item.key == id || item.id.as_uuid().to_string() == id)
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "unknown_type", id.clone()))?;
    let listed = object_type
        .structures
        .clone()
        .unwrap_or_default()
        .iter()
        .filter_map(|structure| {
            after
                .structures
                .iter()
                .find(|item| item.id == *structure)
                .map(|item| item.key.clone())
        })
        .collect::<Vec<_>>();
    if dry_run {
        return Ok(Json(StructuresResponse {
            structures: listed,
            applied: false,
            warnings: impact.lines,
        })
        .into_response());
    }
    if !identity.may_commit() {
        let response = crate::model_proposal::propose(
            &state,
            &identity,
            "model.set_structures",
            &serde_json::json!({ "type": id, "structures": body.structures }),
        )
        .await?;
        return Ok((StatusCode::ACCEPTED, Json(response)).into_response());
    }
    let (_defs, _receipt) = commit(&state, &identity.actor, |defs| {
        braian_model::apply_all(defs, std::slice::from_ref(&change))
            .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "model", error.to_string()))
    })
    .await?;
    if impact
        .lines
        .iter()
        .any(|line| line.contains("may use") || line.contains("no tree"))
    {
        let _ = state.store.request_reindex().await;
    }
    Ok(Json(StructuresResponse {
        structures: listed,
        applied: true,
        warnings: impact.lines,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/v1/types/{id}/unplaced",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Type id or key"),
        ("structure" = String, Query, description = "Structure id or key")
    ),
    responses((status = 200, description = "Objects of this type with no group in the tree", body = UnplacedCount)),
    security(("bearer" = []))
)]
pub async fn unplaced_count(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<UnplacedQuery>,
) -> Result<Json<UnplacedCount>, Problem> {
    require(&identity, Role::Viewer)?;
    let cached = state.models.get(&state.store).await?;
    let type_id = crate::wire::resolve_type(&cached.model, &id)?;
    let structure = crate::wire::resolve_structure(&cached.model, &cached.defs, &query.structure)?;
    let count = state
        .store
        .unplaced_count(type_id, structure)
        .await
        .map_err(store_status)?;
    let key = cached
        .defs
        .structures
        .iter()
        .find(|item| item.id == structure)
        .map(|item| item.key.clone())
        .unwrap_or(query.structure);
    Ok(Json(UnplacedCount {
        structure: key,
        count,
    }))
}
