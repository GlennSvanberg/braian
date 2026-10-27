//! Address a model entity by id or key, and rename a key.
//!
//! A rename from this route is applied at once. `dry_run` returns the warning
//! and the resulting `{id, key, label}` without writing.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use braian_model::{ModelChange, RefKind, change_needs_reindex, preview, resolve};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::{Auth, Role};
use crate::definitions::commit;
use crate::platform::require;
use crate::problem::Problem;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveQuery {
    /// Structure id or key. Required when a group key is used in more than one structure.
    #[serde(default)]
    pub structure: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResolvedBody {
    pub id: String,
    pub key: String,
    pub label: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KeyRenameBody {
    /// `type`, `attribute`, `structure`, `group`, `value_list` or `unit_family`.
    pub kind: String,
    /// Current id or key.
    #[serde(rename = "ref", alias = "id", alias = "token")]
    pub current: String,
    /// The key to store. The id does not change.
    pub key: String,
    #[serde(default)]
    pub structure: Option<String>,
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DryQuery {
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct KeyRenameResponse {
    pub id: String,
    pub key: String,
    pub label: String,
    pub applied: bool,
    pub warnings: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/v1/resolve/{kind}/{id_or_key}",
    tag = "definitions",
    params(
        ("kind" = String, Path, description = "type, attribute, structure, group, value_list or unit_family"),
        ("id_or_key" = String, Path, description = "Entity id or key"),
        ("structure" = Option<String>, Query, description = "Structure id or key, for a group")
    ),
    responses(
        (status = 200, description = "The same entity for an id or a key", body = ResolvedBody),
        (status = 404, description = "Unknown entity", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn resolve_ref(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Path((kind, token)): Path<(String, String)>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<ResolvedBody>, Problem> {
    require(&identity, Role::Viewer)?;
    let defs = state.models.get(&state.store).await?.defs.clone();
    let kind = parse_kind(&kind)?;
    let resolved = resolve(&defs, kind, &token, query.structure.as_deref()).map_err(unknown)?;
    Ok(Json(ResolvedBody {
        id: resolved.id,
        key: resolved.key,
        label: resolved.label,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/model/keys",
    tag = "definitions",
    params(("dry_run" = Option<bool>, Query, description = "Return the warning without writing")),
    request_body = KeyRenameBody,
    responses(
        (status = 200, description = "Key renamed, or the consequences of a dry run", body = KeyRenameResponse),
        (status = 400, description = "The key was rejected", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
pub async fn rename_key(
    State(state): State<AppState>,
    Auth(identity): Auth,
    Query(query): Query<DryQuery>,
    Json(body): Json<KeyRenameBody>,
) -> Result<Json<KeyRenameResponse>, Problem> {
    let dry_run = body.dry_run.unwrap_or(false) || query.dry_run.unwrap_or(false);
    if dry_run {
        require(&identity, Role::Viewer)?;
    } else {
        require(&identity, Role::Editor)?;
    }
    let kind = parse_kind(&body.kind)?;
    let change = change_for(kind, &body)?;
    let cached = state.models.get(&state.store).await?;
    let (after, impact) = preview(&cached.defs, std::slice::from_ref(&change))
        .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "model", error.to_string()))?;
    let resolved = resolve(&after, kind, &body.key, body.structure.as_deref()).map_err(unknown)?;
    let warnings = impact.warnings;
    if dry_run {
        return Ok(Json(KeyRenameResponse {
            id: resolved.id,
            key: resolved.key,
            label: resolved.label,
            applied: false,
            warnings,
        }));
    }
    let reindex = change_needs_reindex(&change);
    let (_defs, _receipt) = commit(&state, &identity.actor, |defs| {
        braian_model::apply_all(defs, std::slice::from_ref(&change))
            .map_err(|error| Problem::new(StatusCode::BAD_REQUEST, "model", error.to_string()))
    })
    .await?;
    if reindex {
        state
            .store
            .request_reindex()
            .await
            .map_err(crate::problem::store_status)?;
    }
    Ok(Json(KeyRenameResponse {
        id: resolved.id,
        key: resolved.key,
        label: resolved.label,
        applied: true,
        warnings,
    }))
}

fn parse_kind(token: &str) -> Result<RefKind, Problem> {
    RefKind::parse(token).ok_or_else(|| {
        Problem::new(
            StatusCode::BAD_REQUEST,
            "model",
            format!("unknown kind {token}"),
        )
    })
}

fn unknown(message: String) -> Problem {
    let status = if message.starts_with("unknown") || message.contains("unknown") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    Problem::new(status, "model", message)
}

fn change_for(kind: RefKind, body: &KeyRenameBody) -> Result<ModelChange, Problem> {
    let current = body.current.trim().to_owned();
    let new_key = body.key.trim().to_owned();
    if current.is_empty() || new_key.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "model",
            "missing id or key",
        ));
    }
    Ok(match kind {
        RefKind::Type => ModelChange::RenameTypeKey {
            key: current,
            new_key,
        },
        RefKind::Attribute => ModelChange::RenameAttributeKey {
            key: current,
            new_key,
        },
        RefKind::Structure => ModelChange::RenameStructureKey {
            structure: current,
            new_key,
        },
        RefKind::Group => ModelChange::RenameGroupKey {
            structure: body
                .structure
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    Problem::new(StatusCode::BAD_REQUEST, "model", "missing structure")
                })?,
            group: current,
            new_key,
        },
        RefKind::ValueList => ModelChange::RenameValueListKey {
            key: current,
            new_key,
        },
        RefKind::UnitFamily => ModelChange::RenameUnitFamilyKey {
            key: current,
            new_key,
        },
    })
}
