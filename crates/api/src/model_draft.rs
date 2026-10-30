//! A person's model draft, and direct saves that use the same steps.
//!
//! The grid collects steps here. Review and save writes one revision with
//! source `user` and does not open a proposal. An agent never writes this
//! table: the registry turns the same steps into a proposal.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use braian_model::{
    DefinitionSet, ModelChange, ModelDraft, SharedValueList, change_removed_definitions,
    groups_removed_by_structure_upserts, preview, purge_for, restored_definitions, undo_purge,
};
use braian_rules::CompiledModel;
use braian_store::{Actor, BatchKind, Error, PrincipalKind};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::{Auth, Identity};
use crate::deletes::DryRun;
use crate::model_proposal::{consequence, model_status};
use crate::problem::{Problem, store_status};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct DraftBody {
    #[serde(default)]
    #[schema(value_type = Vec<crate::http_schema::ModelStep>)]
    pub steps: Vec<ModelChange>,
    /// Drops the step at this index. Cannot be combined with `steps`.
    #[serde(default)]
    pub remove: Option<usize>,
    /// Operation name from the registry. Converted with the same mapping as a proposal.
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::http_schema::FreeObject>)]
    pub args: Option<Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveBody {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub confirm_name: Option<String>,
}

pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/model/draft", get(get_draft).delete(discard_draft))
        .route("/v1/model/draft/steps", post(post_steps))
        .route("/v1/model/draft/rebase", post(rebase_draft))
        .route("/v1/model/draft/save", post(save_draft))
        .route(
            "/v1/value-lists",
            get(list_value_lists).post(post_value_list),
        )
        .route(
            "/v1/value-lists/{id}",
            get(get_value_list)
                .put(put_value_list)
                .delete(delete_value_list),
        )
}

async fn require_person(state: &AppState, identity: &Identity) -> Result<Uuid, Problem> {
    if identity.kind != PrincipalKind::Human || !identity.may_commit() {
        return Err(crate::auth::commit_forbidden());
    }
    if let Some(id) = identity.user_id {
        return Ok(id);
    }
    state
        .store
        .user_id_for_principal(identity.actor.actor.as_uuid())
        .await
        .map_err(store_status)?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "no_user",
                "sign in to keep a model draft",
            )
        })
}

fn user_actor(identity: &Identity) -> Actor {
    let mut actor = identity.actor.clone();
    actor.kind = BatchKind::Interactive;
    actor.source = "user".into();
    actor
}

struct StoredDraft {
    draft: ModelDraft,
    updated_at: DateTime<Utc>,
}

async fn load_draft(state: &AppState, user_id: Uuid) -> Result<Option<StoredDraft>, Problem> {
    let row = state
        .store
        .model_draft(user_id)
        .await
        .map_err(store_status)?;
    let Some((base, steps, updated_at)) = row else {
        return Ok(None);
    };
    let steps: Vec<ModelChange> = serde_json::from_value(steps).map_err(|error| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "draft",
            error.to_string(),
        )
    })?;
    let base_revision = u64::try_from(base).unwrap_or(0);
    Ok(Some(StoredDraft {
        draft: ModelDraft {
            base_revision,
            steps,
        },
        updated_at,
    }))
}

async fn store_draft(state: &AppState, user_id: Uuid, draft: &ModelDraft) -> Result<(), Problem> {
    let steps = serde_json::to_value(&draft.steps).map_err(|error| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "draft",
            error.to_string(),
        )
    })?;
    let base = i64::try_from(draft.base_revision).unwrap_or(i64::MAX);
    state
        .store
        .put_model_draft(user_id, base, &steps)
        .await
        .map_err(store_status)
}

async fn draft_view(
    state: &AppState,
    draft: Option<&ModelDraft>,
    current: u64,
    updated_at: Option<DateTime<Utc>>,
) -> Result<Value, Problem> {
    let Some(draft) = draft else {
        return Ok(json!({
            "count": 0,
            "steps": [],
            "base_revision": Value::Null,
            "current_revision": current,
            "conflict": false,
        }));
    };
    let conflict = draft.conflicts(current);
    let mut body = json!({
        "count": draft.steps.len(),
        "steps": draft.steps,
        "base_revision": draft.base_revision,
        "current_revision": current,
        "conflict": conflict,
        "updated_at": updated_at.map(|stamp| stamp.to_rfc3339()),
    });
    if conflict {
        return Ok(body);
    }
    let defs = state.store.definition_set().await.map_err(store_status)?;
    match preview(&defs, &draft.steps) {
        Ok((after, impact)) => {
            let report = consequence(state, &defs, &draft.steps, &impact).await?;
            body["lines"] = json!(impact.lines);
            body["warnings"] = json!(impact.warnings);
            body["consequence"] = report;
            body["definitions"] = serde_json::to_value(&after).unwrap_or(Value::Null);
        }
        Err(error) => {
            body["error"] = json!(error.to_string());
        }
    }
    Ok(body)
}

async fn get_draft(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    let user_id = require_person(&state, &identity).await?;
    let draft = load_draft(&state, user_id).await?;
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    Ok(Json(
        draft_view(
            &state,
            draft.as_ref().map(|row| &row.draft),
            current,
            draft.as_ref().map(|row| row.updated_at),
        )
        .await?,
    ))
}

async fn post_steps(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<DraftBody>,
) -> Result<Json<Value>, Problem> {
    let user_id = require_person(&state, &identity).await?;
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    let loaded = load_draft(&state, user_id).await?;
    let mut draft = loaded
        .as_ref()
        .map(|row| row.draft.clone())
        .unwrap_or_else(|| ModelDraft::new(current));
    let mut steps = body.steps;
    if body.remove.is_none()
        && let Some(name) = body.operation.as_deref()
    {
        if name.contains("delete") {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "draft",
                "delete from the dialog; it is not collected in the draft",
            ));
        }
        let defs = state.store.definition_set().await.map_err(store_status)?;
        let args = body.args.unwrap_or(Value::Null);
        steps.extend(crate::model_proposal::changes_from(name, &args, &defs)?);
    }
    if draft.conflicts(current) && (body.remove.is_some() || !steps.is_empty()) {
        return Err(conflict_problem(draft.base_revision, current));
    }
    if let Some(index) = body.remove {
        draft
            .remove(index)
            .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, "draft", message))?;
    } else {
        for step in steps {
            draft.add(step);
        }
    }
    if draft.steps.is_empty() {
        state
            .store
            .delete_model_draft(user_id)
            .await
            .map_err(store_status)?;
        return Ok(Json(draft_view(&state, None, current, None).await?));
    }
    let defs = state.store.definition_set().await.map_err(store_status)?;
    preview(&defs, &draft.steps).map_err(model_status)?;
    store_draft(&state, user_id, &draft).await?;
    Ok(Json(
        draft_view(&state, Some(&draft), current, Some(Utc::now())).await?,
    ))
}

async fn discard_draft(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<StatusCode, Problem> {
    let user_id = require_person(&state, &identity).await?;
    state
        .store
        .delete_model_draft(user_id)
        .await
        .map_err(store_status)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn rebase_draft(
    State(state): State<AppState>,
    Auth(identity): Auth,
) -> Result<Json<Value>, Problem> {
    let user_id = require_person(&state, &identity).await?;
    let mut draft = load_draft(&state, user_id)
        .await?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "draft", "no draft to rebase"))?
        .draft;
    let defs = state.store.definition_set().await.map_err(store_status)?;
    draft.rebase(&defs).map_err(model_status)?;
    store_draft(&state, user_id, &draft).await?;
    Ok(Json(
        draft_view(&state, Some(&draft), defs.model_revision, Some(Utc::now())).await?,
    ))
}

async fn save_draft(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<DryRun>,
    Json(body): Json<SaveBody>,
) -> Result<Json<Value>, Problem> {
    let user_id = require_person(&state, &identity).await?;
    let draft = load_draft(&state, user_id)
        .await?
        .ok_or_else(|| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "empty_draft",
                "the draft has no steps",
            )
        })?
        .draft;
    if draft.steps.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "empty_draft",
            "the draft has no steps",
        ));
    }
    let current = state
        .store
        .model_revision()
        .await
        .map_err(store_status)?
        .unwrap_or(0);
    if draft.conflicts(current) {
        return Err(conflict_problem(draft.base_revision, current));
    }
    let dry_run = query.dry_run || body.dry_run;
    let actor = user_actor(&identity);
    let saved = commit_steps(
        &state,
        &actor,
        &draft.steps,
        dry_run,
        body.confirm_name.as_deref(),
    )
    .await?;
    if !dry_run {
        state
            .store
            .delete_model_draft(user_id)
            .await
            .map_err(store_status)?;
    }
    Ok(Json(saved))
}

fn conflict_problem(base: u64, current: u64) -> Problem {
    Problem::new(
        StatusCode::CONFLICT,
        "draft_conflict",
        format!("the model is at revision {current} and the draft started at {base}"),
    )
}

/// Applies model steps directly. `dry_run` returns the consequence and writes nothing.
pub(crate) async fn commit_steps(
    state: &AppState,
    actor: &Actor,
    changes: &[ModelChange],
    dry_run: bool,
    confirm_name: Option<&str>,
) -> Result<Value, Problem> {
    for _attempt in 0..2 {
        let defs = match state.store.definition_set().await {
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
        // One check for the write and for dry_run. A structure upsert that drops
        // a group still holding objects fails here and leaves the model as it was.
        let removed = groups_removed_by_structure_upserts(&defs, changes);
        if !removed.is_empty() {
            crate::structures::reject_removed_groups(state, &removed).await?;
        }
        let (mut after, impact) = preview(&defs, changes).map_err(model_status)?;
        CompiledModel::compile(&after).map_err(Problem::definitions)?;
        let purge = purge_for(&defs, changes);
        let report = consequence(state, &defs, changes, &impact).await?;
        if dry_run {
            return Ok(json!({
                "applied": false,
                "source": actor.source,
                "lines": impact.lines,
                "warnings": impact.warnings,
                "consequence": report,
            }));
        }
        if purge.destructive
            && !purge.confirm_name.is_empty()
            && confirm_name != Some(purge.confirm_name.as_str())
        {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "confirm_name",
                format!("type the name {} to confirm", purge.confirm_name),
            ));
        }
        let current = defs.model_revision;
        after.model_revision = current + 1;
        let write = if purge.destructive {
            state
                .store
                .put_definitions_purging(actor, &after, &purge)
                .await
        } else {
            state.store.put_definitions(actor, &after).await
        };
        match write {
            Ok(receipt) => {
                state.models.invalidate().await;
                let source = state
                    .store
                    .definition_batch_source(after.model_revision)
                    .await
                    .map_err(store_status)?
                    .unwrap_or_else(|| actor.source.clone());
                state.emit(json!({ "seq": receipt.last_seq, "op": "model" }));
                return Ok(json!({
                    "applied": true,
                    "model_revision": after.model_revision,
                    "seq": receipt.last_seq,
                    "source": source,
                    "lines": impact.lines,
                    "proposal": false,
                }));
            }
            Err(Error::BadRevision { .. }) => continue,
            Err(error) => return Err(store_status(error)),
        }
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "revision_conflict",
        "definition revision changed",
    ))
}

pub async fn undo_model(state: &AppState, identity: &Identity) -> Result<Value, Problem> {
    if identity.kind != PrincipalKind::Human || !identity.may_commit() {
        return Err(crate::auth::delete_forbidden());
    }
    let recent = state
        .store
        .recent_definitions(2)
        .await
        .map_err(store_status)?;
    let Some(current) = recent.first() else {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "undo",
            "there is no model change to undo",
        ));
    };
    let Some(previous) = recent.get(1) else {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "undo",
            "there is no earlier model revision",
        ));
    };
    if change_removed_definitions(previous, current) {
        return Err(Problem::new(
            StatusCode::CONFLICT,
            "undo",
            "a destructive model change cannot be undone",
        ));
    }
    let purge = undo_purge(previous, current);
    if purge.destructive {
        let counts = state
            .store
            .measure_purge(&purge)
            .await
            .map_err(store_status)?;
        if counts.objects > 0 || counts.values > 0 || counts.links > 0 {
            return Err(Problem::new(
                StatusCode::CONFLICT,
                "undo",
                "this model change has data and cannot be undone",
            ));
        }
    }
    let restored = restored_definitions(previous, current);
    CompiledModel::compile(&restored).map_err(Problem::definitions)?;
    let actor = user_actor(identity);
    let receipt = state
        .store
        .put_definitions(&actor, &restored)
        .await
        .map_err(store_status)?;
    state.models.invalidate().await;
    let source = state
        .store
        .definition_batch_source(restored.model_revision)
        .await
        .map_err(store_status)?
        .unwrap_or_else(|| actor.source.clone());
    Ok(json!({
        "applied": true,
        "model_revision": restored.model_revision,
        "seq": receipt.last_seq,
        "source": source,
        "undone": true,
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ValueListBody {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub options: Vec<BTreeMap<String, String>>,
}

fn list_change(body: &ValueListBody, update: bool) -> Result<ModelChange, Problem> {
    let options = body
        .options
        .iter()
        .map(|item| {
            let code = item
                .get("code")
                .or_else(|| item.get("key"))
                .cloned()
                .ok_or_else(|| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "value_list",
                        "option code is required",
                    )
                })?;
            Ok(braian_model::NamedOption {
                code,
                label: item.get("label").cloned().unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, Problem>>()?;
    if update {
        Ok(ModelChange::UpdateValueList {
            key: body.key.clone(),
            label: if body.label.is_empty() {
                None
            } else {
                Some(body.label.clone())
            },
            options: Some(options),
        })
    } else {
        Ok(ModelChange::CreateValueList {
            key: body.key.clone(),
            label: body.label.clone(),
            options,
        })
    }
}

#[utoipa::path(
    get,
    path = "/v1/value-lists",
    tag = "definitions",
    responses((status = 200, description = "Shared value lists", body = Vec<SharedValueList>)),
    security(("bearer" = []))
)]
pub(crate) async fn list_value_lists(
    State(state): State<AppState>,
    Auth(_): Auth,
) -> Result<Json<Vec<SharedValueList>>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    Ok(Json(defs.value_lists))
}

#[utoipa::path(
    post,
    path = "/v1/value-lists",
    tag = "definitions",
    params(("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")),
    request_body = ValueListBody,
    responses((status = 200, description = "Stored, or a dry run")),
    security(("bearer" = []))
)]
pub(crate) async fn post_value_list(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<DryRun>,
    Json(body): Json<ValueListBody>,
) -> Result<Response, Problem> {
    let step = list_change(&body, false)?;
    let saved = crate::definitions::commit_model_change_allowing_import(
        &state,
        &identity,
        step,
        query.dry_run,
    )
    .await?;
    if query.dry_run {
        return Ok(Json(saved).into_response());
    }
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let stored = defs
        .value_lists
        .into_iter()
        .find(|item| item.key == body.key)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "value_list",
                "list missing",
            )
        })?;
    Ok(Json(stored).into_response())
}

#[utoipa::path(
    get,
    path = "/v1/value-lists/{id}",
    tag = "definitions",
    params(("id" = String, Path, description = "Value list key or id")),
    responses((status = 200, description = "Value list", body = SharedValueList)),
    security(("bearer" = []))
)]
pub(crate) async fn get_value_list(
    State(state): State<AppState>,
    Auth(_): Auth,
    Path(id): Path<String>,
) -> Result<Json<SharedValueList>, Problem> {
    let defs = state.store.definition_set().await.map_err(store_status)?;
    defs.value_lists
        .into_iter()
        .find(|item| item.key == id || item.id.as_uuid().to_string() == id)
        .map(Json)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("unknown value list {id}"),
            )
        })
}

#[utoipa::path(
    put,
    path = "/v1/value-lists/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Value list key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    request_body = ValueListBody,
    responses((status = 200, description = "Stored, or a dry run")),
    security(("bearer" = []))
)]
pub(crate) async fn put_value_list(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<DryRun>,
    Json(mut body): Json<ValueListBody>,
) -> Result<Response, Problem> {
    if body.key.is_empty() {
        body.key = id.clone();
    }
    let step = list_change(&body, true)?;
    let saved = crate::definitions::commit_model_change_allowing_import(
        &state,
        &identity,
        step,
        query.dry_run,
    )
    .await?;
    if query.dry_run {
        return Ok(Json(saved).into_response());
    }
    let defs = state.store.definition_set().await.map_err(store_status)?;
    let stored = defs
        .value_lists
        .into_iter()
        .find(|item| item.key == body.key || item.id.as_uuid().to_string() == id)
        .ok_or_else(|| {
            Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("unknown value list {id}"),
            )
        })?;
    Ok(Json(stored).into_response())
}

#[utoipa::path(
    delete,
    path = "/v1/value-lists/{id}",
    tag = "definitions",
    params(
        ("id" = String, Path, description = "Value list key or id"),
        ("dry_run" = Option<bool>, Query, description = "Return the consequence analysis and change nothing")
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 200, description = "Consequence analysis"),
        (status = 403, description = "An agent or MCP token cannot delete")
    ),
    security(("bearer" = []))
)]
pub(crate) async fn delete_value_list(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path(id): Path<String>,
    Query(query): Query<DryRun>,
) -> Result<Response, Problem> {
    crate::deletes::delete_model(
        &state,
        &identity,
        query.dry_run,
        ModelChange::DeleteValueList { key: id },
    )
    .await
}

/// Direct model steps for REST callers. `dry_run` reports the consequence.
#[allow(dead_code)]
pub async fn apply_direct(
    state: &AppState,
    identity: &Identity,
    changes: Vec<ModelChange>,
    dry_run: bool,
) -> Result<Value, Problem> {
    if !identity.may_commit() {
        if changes.iter().any(braian_model::deletes_data) {
            return Err(crate::auth::delete_forbidden());
        }
        return Err(crate::auth::commit_forbidden());
    }
    commit_steps(state, &identity.actor, &changes, dry_run, None).await
}
