use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use openpim_model::{
    Attribute, AttributeId, DefinitionSet, ObjectType, Structure, StructureId, TypeId, UnitFamily,
    UnitFamilyId, Uuid, delete_attribute, delete_structure, delete_type, delete_unit_family,
    set_languages, upsert_attribute, upsert_structure, upsert_type, upsert_unit_family,
};
use openpim_rules::CompiledModel;
use openpim_store::{Actor, Error, WriteReceipt};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::Auth;
use crate::headers::{etag, if_match};
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
        (status = 200, description = "Current definition set", body = openpim_model::DefinitionSet),
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
    request_body(content = openpim_model::DefinitionSet, description = "JSON or YAML definition set"),
    responses(
        (status = 200, description = "Imported", body = DefinitionWrite),
        (status = 409, description = "Revision conflict", body = crate::problem::Problem),
        (status = 422, description = "The set does not compile", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn put_definitions(
    State(state): State<AppState>,
    Auth(identity): Auth,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Problem> {
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
    request_body(content = openpim_model::Attribute, description = "Id is optional"),
    responses(
        (status = 200, description = "Stored attribute", body = openpim_model::Attribute),
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
        (status = 200, description = "Attribute", body = openpim_model::Attribute),
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
    request_body(content = openpim_model::Attribute),
    responses(
        (status = 200, description = "Stored attribute", body = openpim_model::Attribute),
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
    params(("id" = String, Path, description = "Attribute key or id")),
    responses(
        (status = 200, description = "Deleted", body = DefinitionWrite),
        (status = 404, description = "Unknown attribute", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_attribute_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
) -> Result<Json<DefinitionWrite>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let attribute_id = defs
        .attributes
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .ok_or_else(|| missing("attribute", &id))?;
    let (defs, receipt) = commit(&state, &identity.actor, |defs| {
        if !delete_attribute(defs, attribute_id) {
            return Err(missing("attribute", &id));
        }
        Ok(())
    })
    .await?;
    Ok(Json(DefinitionWrite {
        model_revision: defs.model_revision,
        seq: receipt.last_seq,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/types",
    tag = "definitions",
    request_body(content = openpim_model::ObjectType, description = "Id is optional"),
    responses((status = 200, description = "Stored type", body = openpim_model::ObjectType)),
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
    responses((status = 200, description = "Type", body = openpim_model::ObjectType)),
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
    request_body(content = openpim_model::ObjectType),
    responses((status = 200, description = "Stored type", body = openpim_model::ObjectType)),
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
    params(("id" = String, Path, description = "Type key or id")),
    responses((status = 200, description = "Deleted", body = DefinitionWrite)),
    security(("bearer" = []))
)]
pub async fn delete_type_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
) -> Result<Json<DefinitionWrite>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let type_id = defs
        .types
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .ok_or_else(|| missing("type", &id))?;
    let (defs, receipt) = commit(&state, &identity.actor, |defs| {
        if !delete_type(defs, type_id) {
            return Err(missing("type", &id));
        }
        Ok(())
    })
    .await?;
    Ok(Json(DefinitionWrite {
        model_revision: defs.model_revision,
        seq: receipt.last_seq,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/structures",
    tag = "definitions",
    request_body(content = openpim_model::Structure, description = "Id is optional. Groups and classes belong to the structure."),
    responses((status = 200, description = "Stored structure", body = openpim_model::Structure)),
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
    responses((status = 200, description = "Structure", body = openpim_model::Structure)),
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
    request_body(content = openpim_model::Structure),
    responses((status = 200, description = "Stored structure", body = openpim_model::Structure)),
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
    params(("id" = String, Path, description = "Structure key or id")),
    responses((status = 200, description = "Deleted", body = DefinitionWrite)),
    security(("bearer" = []))
)]
pub async fn delete_structure_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
) -> Result<Json<DefinitionWrite>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_id = defs
        .structures
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .ok_or_else(|| missing("structure", &id))?;
    let (defs, receipt) = commit(&state, &identity.actor, |defs| {
        if !delete_structure(defs, structure_id) {
            return Err(missing("structure", &id));
        }
        Ok(())
    })
    .await?;
    Ok(Json(DefinitionWrite {
        model_revision: defs.model_revision,
        seq: receipt.last_seq,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/unit-families",
    tag = "definitions",
    request_body(content = openpim_model::UnitFamily, description = "Id is optional"),
    responses((status = 200, description = "Stored unit family", body = openpim_model::UnitFamily)),
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
    responses((status = 200, description = "Unit family", body = openpim_model::UnitFamily)),
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
    request_body(content = openpim_model::UnitFamily),
    responses((status = 200, description = "Stored unit family", body = openpim_model::UnitFamily)),
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
    params(("id" = String, Path, description = "Unit family key or id")),
    responses((status = 200, description = "Deleted", body = DefinitionWrite)),
    security(("bearer" = []))
)]
pub async fn delete_unit_family_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
) -> Result<Json<DefinitionWrite>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let family_id = defs
        .unit_families
        .iter()
        .find(|item| same_ref(&item.id.as_uuid().to_string(), &item.key, &id))
        .map(|item| item.id)
        .ok_or_else(|| missing("unit family", &id))?;
    let (defs, receipt) = commit(&state, &identity.actor, |defs| {
        if !delete_unit_family(defs, family_id) {
            return Err(missing("unit family", &id));
        }
        Ok(())
    })
    .await?;
    Ok(Json(DefinitionWrite {
        model_revision: defs.model_revision,
        seq: receipt.last_seq,
    }))
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
    Json(languages): Json<Vec<openpim_model::LanguageTag>>,
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

async fn commit(
    state: &AppState,
    actor: &Actor,
    edit: impl Fn(&mut DefinitionSet) -> Result<(), Problem>,
) -> Result<(DefinitionSet, WriteReceipt), Problem> {
    for attempt in 0..2 {
        let mut defs = match state.store.definition_set().await {
            Ok(defs) => defs,
            Err(Error::NoDefinitions) => DefinitionSet {
                model_revision: 0,
                languages: Vec::new(),
                unit_families: Vec::new(),
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
