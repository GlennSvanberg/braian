//! Granular structure and group edits.
//!
//! A delete that would drop a group out from under live objects is refused
//! unless the caller names a group outside that subtree to receive them.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use braian_model::structure::{
    create_group, create_structure, move_group, plan_delete_group, plan_delete_structure,
    rename_group, rename_structure, set_group_labels, set_structure_labels,
};
use braian_model::{
    GroupId, LanguageTag, ModelChange, Structure, StructureError, StructureId, Uuid,
};
use braian_store::Actor;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::Auth;
use crate::definitions;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct StructureCreate {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// Replaces the single `label` when present.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StructureRename {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupCreate {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// Parent group key. Absent means a root group.
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupRename {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupMove {
    /// Parent group key. `null` makes the group a root.
    pub parent: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MoveToQuery {
    /// Group key that receives objects from the deleted group and its descendants.
    #[serde(default)]
    pub move_to: Option<String>,
    /// When true, count what the delete would remove and change nothing.
    #[serde(default)]
    #[param(required = false)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlacementCount {
    /// Live objects in the group and its descendants, or in the whole structure.
    pub count: u64,
}

pub async fn guard_model_change(
    state: &AppState,
    actor: &Actor,
    change: &ModelChange,
    apply: bool,
) -> Result<(), Problem> {
    match change {
        ModelChange::DeleteGroup {
            structure,
            group,
            move_to,
        } => guard_group(state, actor, structure, group, move_to.as_deref(), apply).await,
        ModelChange::DeleteStructure { structure } => guard_structure(state, structure).await,
        _ => Ok(()),
    }
}

#[utoipa::path(
    post,
    path = "/v1/structures/{id}/groups",
    tag = "definitions",
    params(("id" = String, Path, description = "Structure key or id")),
    request_body = GroupCreate,
    responses(
        (status = 200, description = "Structure after the group was added", body = braian_model::Structure),
        (status = 404, description = "Unknown structure or parent", body = Problem),
        (status = 422, description = "empty_key, invalid_key, or duplicate_key", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn post_group(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(body): Json<GroupCreate>,
) -> Result<Json<Structure>, Problem> {
    let parent = body.parent.clone();
    let (defs, _) = definitions::commit(&state, &identity.actor, |defs| {
        let structure_key = structure_key(defs, &id)?;
        create_group(
            defs,
            GroupId::from_uuid(Uuid::now_v7()),
            &structure_key,
            &body.key,
            &body.label,
            parent.as_deref(),
            true,
        )
        .map_err(structure_status)
    })
    .await?;
    Ok(Json(stored_structure(defs, &id)?))
}

#[utoipa::path(
    patch,
    path = "/v1/structures/{id}/groups/{group}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Structure key or id"),
        ("group" = String, Path, description = "Group key")
    ),
    request_body = GroupRename,
    responses(
        (status = 200, description = "Structure after the rename", body = braian_model::Structure),
        (status = 404, description = "Unknown structure or group", body = Problem),
        (status = 422, description = "empty_key", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn rename_group_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path((id, group)): Path<(String, String)>,
    Json(body): Json<GroupRename>,
) -> Result<Json<Structure>, Problem> {
    let (defs, _) = definitions::commit(&state, &identity.actor, |defs| {
        let structure_key = structure_key(defs, &id)?;
        apply_group_rename(defs, &structure_key, &group, &body)
    })
    .await?;
    Ok(Json(stored_structure(defs, &id)?))
}

#[utoipa::path(
    post,
    path = "/v1/structures/{id}/groups/{group}/parent",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Structure key or id"),
        ("group" = String, Path, description = "Group key")
    ),
    request_body = GroupMove,
    responses(
        (status = 200, description = "Structure after the move", body = braian_model::Structure),
        (status = 404, description = "Unknown structure or group", body = Problem),
        (status = 422, description = "cycle", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn move_group_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path((id, group)): Path<(String, String)>,
    Json(body): Json<GroupMove>,
) -> Result<Json<Structure>, Problem> {
    let parent = body.parent.clone();
    let (defs, _) = definitions::commit(&state, &identity.actor, |defs| {
        let structure_key = structure_key(defs, &id)?;
        move_group(defs, &structure_key, &group, parent.as_deref()).map_err(structure_status)
    })
    .await?;
    Ok(Json(stored_structure(defs, &id)?))
}

#[utoipa::path(
    get,
    path = "/v1/structures/{id}/groups/{group}/usage",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Structure key or id"),
        ("group" = String, Path, description = "Group key")
    ),
    responses(
        (status = 200, description = "Objects in the group and its descendants", body = PlacementCount),
        (status = 404, description = "Unknown structure or group", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn group_usage(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path((id, group)): Path<(String, String)>,
) -> Result<Json<PlacementCount>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_key = structure_key(&defs, &id)?;
    let plan = plan_delete_group(&defs, &structure_key, &group, None).map_err(structure_status)?;
    let count = count_groups(&state, &plan.removed).await?;
    Ok(Json(PlacementCount { count }))
}

#[utoipa::path(
    delete,
    path = "/v1/structures/{id}/groups/{group}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Structure key or id"),
        ("group" = String, Path, description = "Group key"),
        MoveToQuery
    ),
    responses(
        (status = 204, description = "Group and descendants removed. Objects leave the tree unless move_to names another group."),
        (status = 200, description = "Consequence analysis", body = crate::deletes::ConsequenceReport),
        (status = 403, description = "An agent or MCP token cannot delete", body = Problem),
        (status = 404, description = "Unknown structure or group", body = Problem),
        (status = 422, description = "target_in_subtree when move_to is inside the deleted subtree", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn delete_group_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path((id, group)): Path<(String, String)>,
    Query(query): Query<MoveToQuery>,
) -> Result<axum::response::Response, Problem> {
    guard_group(
        &state,
        &identity.actor,
        &id,
        &group,
        query.move_to.as_deref(),
        !query.dry_run,
    )
    .await?;
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        ModelChange::DeleteGroup {
            structure: id,
            group,
            move_to: query.move_to,
        },
    )
    .await
}

#[utoipa::path(
    post,
    path = "/v1/structures:create",
    tag = "definitions",
    request_body = StructureCreate,
    responses(
        (status = 200, description = "Empty structure. Add groups afterwards.", body = braian_model::Structure),
        (status = 422, description = "empty_key, invalid_key, or duplicate_key", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn create_structure_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<StructureCreate>,
) -> Result<Json<Structure>, Problem> {
    let key = body.key.clone();
    let (defs, _) = definitions::commit(&state, &identity.actor, |defs| {
        create_structure(
            defs,
            StructureId::from_uuid(Uuid::now_v7()),
            &body.key,
            &body.label,
            true,
        )
        .map_err(structure_status)?;
        if !body.labels.is_empty() {
            set_structure_labels(defs, &body.key, parse_labels(&body.labels)?)
                .map_err(structure_status)?;
        }
        Ok(())
    })
    .await?;
    Ok(Json(stored_structure(defs, &key)?))
}

#[utoipa::path(
    patch,
    path = "/v1/structures/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Structure key or id")),
    request_body = StructureRename,
    responses(
        (status = 200, description = "Renamed structure. The key is unchanged.", body = braian_model::Structure),
        (status = 404, description = "Unknown structure", body = Problem),
        (status = 422, description = "empty_key", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn rename_structure_route(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Json(body): Json<StructureRename>,
) -> Result<Json<Structure>, Problem> {
    let (defs, _) = definitions::commit(&state, &identity.actor, |defs| {
        let structure_key = structure_key(defs, &id)?;
        apply_structure_rename(defs, &structure_key, &body)
    })
    .await?;
    Ok(Json(stored_structure(defs, &id)?))
}

#[utoipa::path(
    get,
    path = "/v1/structures/{id}/usage",
    tag = "definitions",
    params(("id" = String, Path, description = "Structure key or id")),
    responses(
        (status = 200, description = "Objects placed anywhere in the structure", body = PlacementCount),
        (status = 404, description = "Unknown structure", body = Problem)
    ),
    security(("bearer" = []))
)]
pub async fn structure_usage(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<PlacementCount>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_key = structure_key(&defs, &id)?;
    let plan = plan_delete_structure(&defs, &structure_key).map_err(structure_status)?;
    let count = count_groups(&state, &plan.removed).await?;
    Ok(Json(PlacementCount { count }))
}

/// Refuses a whole-structure replace that drops groups objects still use.
pub async fn reject_removed_groups(state: &AppState, removed: &[GroupId]) -> Result<(), Problem> {
    let count = count_groups(state, removed).await?;
    if count == 0 {
        return Ok(());
    }
    Err(Problem::occupied(
        "group_occupied",
        format!("{count} objects are still in groups this save removes"),
        count,
    ))
}

async fn guard_structure(state: &AppState, id: &str) -> Result<(), Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_key = structure_key(&defs, id)?;
    plan_delete_structure(&defs, &structure_key).map_err(structure_status)?;
    Ok(())
}

async fn guard_group(
    state: &AppState,
    actor: &Actor,
    structure: &str,
    group: &str,
    move_to: Option<&str>,
    apply: bool,
) -> Result<(), Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let structure_key = structure_key(&defs, structure)?;
    let plan =
        plan_delete_group(&defs, &structure_key, group, move_to).map_err(structure_status)?;
    if apply && let Some(target) = plan.move_to {
        state
            .store
            .reassign_members(actor, plan.structure_id, &plan.removed, target)
            .await
            .map_err(store_status)?;
    }
    Ok(())
}

async fn count_groups(state: &AppState, groups: &[GroupId]) -> Result<u64, Problem> {
    let ids: Vec<Uuid> = groups.iter().map(|id| id.as_uuid()).collect();
    let count = state
        .store
        .count_in_groups(&ids)
        .await
        .map_err(store_status)?;
    u64::try_from(count).map_err(|_| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "object count overflowed",
        )
    })
}

fn apply_structure_rename(
    defs: &mut braian_model::DefinitionSet,
    key: &str,
    body: &StructureRename,
) -> Result<(), Problem> {
    if !body.labels.is_empty() {
        return set_structure_labels(defs, key, parse_labels(&body.labels)?)
            .map_err(structure_status);
    }
    let Some(label) = body.label.as_deref() else {
        return Err(structure_status(StructureError::new(
            braian_model::StructureErrorCode::EmptyKey,
            "name is empty",
        )));
    };
    rename_structure(defs, key, label).map_err(structure_status)
}

fn apply_group_rename(
    defs: &mut braian_model::DefinitionSet,
    structure: &str,
    group: &str,
    body: &GroupRename,
) -> Result<(), Problem> {
    if !body.labels.is_empty() {
        return set_group_labels(defs, structure, group, parse_labels(&body.labels)?)
            .map_err(structure_status);
    }
    let Some(label) = body.label.as_deref() else {
        return Err(structure_status(StructureError::new(
            braian_model::StructureErrorCode::EmptyKey,
            "name is empty",
        )));
    };
    rename_group(defs, structure, group, label).map_err(structure_status)
}

fn structure_key(defs: &braian_model::DefinitionSet, token: &str) -> Result<String, Problem> {
    defs.structures
        .iter()
        .find(|item| item.key == token || item.id.as_uuid().to_string() == token)
        .map(|item| item.key.clone())
        .ok_or_else(|| {
            structure_status(StructureError::new(
                braian_model::StructureErrorCode::UnknownStructure,
                format!("unknown structure {token}"),
            ))
        })
}

fn stored_structure(defs: braian_model::DefinitionSet, token: &str) -> Result<Structure, Problem> {
    defs.structures
        .into_iter()
        .find(|item| item.key == token || item.id.as_uuid().to_string() == token)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "structure missing",
            )
        })
}

fn parse_labels(
    labels: &BTreeMap<String, String>,
) -> Result<BTreeMap<LanguageTag, String>, Problem> {
    let mut parsed = BTreeMap::new();
    for (tag, label) in labels {
        let tag = LanguageTag::parse(tag).map_err(|error| {
            Problem::new(StatusCode::BAD_REQUEST, "bad_body", error.to_string())
        })?;
        parsed.insert(tag, label.clone());
    }
    Ok(parsed)
}

fn structure_status(error: StructureError) -> Problem {
    let status = match error.code {
        braian_model::StructureErrorCode::UnknownStructure
        | braian_model::StructureErrorCode::UnknownGroup => StatusCode::NOT_FOUND,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    Problem::new(status, error.code.as_str(), error.message)
}
