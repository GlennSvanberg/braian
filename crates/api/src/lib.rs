//! HTTP API in front of the store.
//!
//! `Problem` is the response body, so handlers return it by value.
#![allow(clippy::result_large_err)]
//!
//! Definitions can be written one entity at a time or imported as a whole set.
//! Objects are created, patched, read and deleted with validation always on.

mod auth;
mod bulk_gate;
mod definitions;
mod deletes;
mod export;
mod feedback;
mod headers;
pub mod import;
mod keys;
mod model_proposal;
mod objects;
mod platform;
mod problem;
mod state;
mod structures;
mod wire;

use axum::Json;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use braian_store::Store;
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::{Auth, Identity, identities_from};

pub use crate::auth::{
    IdentitySpec, hash_password, parse_dev_tokens, random_token, token_hash, verify_password,
};
pub use crate::feedback::check_feedback_config;
pub use crate::state::AppState;

const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Router for one process. `identities` come from [`identities_from`].
pub fn router(store: Store, identities: Vec<Identity>) -> axum::Router {
    into_router(AppState::new(store, identities, None))
}

pub fn into_router(state: AppState) -> axum::Router {
    import::spawn_janitor(state.clone());
    let limited = axum::Router::new()
        .route("/healthz", get(health))
        .route("/v1/me", get(me))
        .route("/v1/openapi.json", get(openapi_json))
        .route(
            "/v1/definitions",
            get(definitions::get_definitions).put(definitions::put_definitions),
        )
        .route("/v1/attributes", post(definitions::post_attribute))
        .route(
            "/v1/attributes/{id}",
            get(definitions::get_attribute)
                .put(definitions::put_attribute)
                .delete(definitions::delete_attribute_route),
        )
        .route("/v1/types", post(definitions::post_type))
        .route(
            "/v1/types/{id}",
            get(definitions::get_type)
                .put(definitions::put_type)
                .delete(definitions::delete_type_route),
        )
        .route(
            "/v1/types/{id}/structures",
            put(definitions::put_type_structures),
        )
        .route("/v1/types/{id}/unplaced", get(definitions::unplaced_count))
        .route("/v1/structures", post(definitions::post_structure))
        .route(
            "/v1/structures:create",
            post(structures::create_structure_route),
        )
        .route(
            "/v1/structures/{id}",
            get(definitions::get_structure)
                .put(definitions::put_structure)
                .patch(structures::rename_structure_route)
                .delete(definitions::delete_structure_route),
        )
        .route(
            "/v1/structures/{id}/usage",
            get(structures::structure_usage),
        )
        .route("/v1/structures/{id}/groups", post(structures::post_group))
        .route(
            "/v1/structures/{id}/groups/{group}",
            patch(structures::rename_group_route).delete(structures::delete_group_route),
        )
        .route(
            "/v1/structures/{id}/groups/{group}/parent",
            post(structures::move_group_route),
        )
        .route(
            "/v1/structures/{id}/groups/{group}/usage",
            get(structures::group_usage),
        )
        .route("/v1/unit-families", post(definitions::post_unit_family))
        .route(
            "/v1/unit-families/{id}",
            get(definitions::get_unit_family)
                .put(definitions::put_unit_family)
                .delete(definitions::delete_unit_family_route),
        )
        .route("/v1/languages", put(definitions::put_languages))
        .route("/v1/resolve/{kind}/{id_or_key}", get(keys::resolve_ref))
        .route("/v1/model/keys", post(keys::rename_key))
        .route("/v1/objects", post(objects::create_object))
        .route("/v1/objects:batchGet", post(objects::batch_get))
        .route(
            "/v1/objects/{id}",
            get(objects::get_object)
                .patch(objects::patch_object)
                .delete(objects::delete_object),
        )
        .route("/v1/objects/{id}/changes", get(objects::object_changes))
        .merge(feedback::routes())
        .merge(platform::routes())
        .merge(import::routes())
        .layer(DefaultBodyLimit::max(BODY_LIMIT));
    let upload = axum::Router::new()
        .route("/v1/imports", post(import::upload))
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024));
    upload
        .merge(limited)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            track_interactive,
        ))
        .with_state(state)
}

/// Routes that read or write catalog data count as interactive.
///
/// Health checks, the OpenAPI document, the long-lived SSE stream and the
/// bulk submission itself do not. Counting the stream would pause every
/// import for as long as a browser tab stayed open.
pub(crate) fn counts_as_interactive(path: &str) -> bool {
    if matches!(
        path,
        "/healthz" | "/v1/openapi.json" | "/v1/stream" | "/v1/bulk"
    ) {
        return false;
    }
    // Queuing an import save only reserves the bulk gate. The job runs on the
    // bulk lane and must not look like an interactive request while it waits.
    if let Some(rest) = path.strip_prefix("/v1/imports/")
        && rest.ends_with("/save")
    {
        return false;
    }
    true
}

async fn track_interactive(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !counts_as_interactive(request.uri().path()) {
        return next.run(request).await;
    }
    let _guard = state.store.enter_interactive();
    next.run(request).await
}

pub async fn identities(store: &Store, raw: &str) -> Result<Vec<Identity>, braian_store::Error> {
    identities_from(store, raw).await
}

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "meta",
    responses((status = 204, description = "The process is up"))
)]
async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Serialize, ToSchema)]
struct Me {
    handle: String,
    kind: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<uuid::Uuid>,
    feedback: bool,
}

#[utoipa::path(
    get,
    path = "/v1/me",
    tag = "meta",
    responses(
        (status = 200, description = "The token's principal", body = Me),
        (status = 401, description = "Missing or unknown token", body = crate::problem::Problem)
    ),
    security(("bearer" = []))
)]
async fn me(Auth(identity): Auth) -> Json<Me> {
    Json(Me {
        handle: identity.handle,
        kind: identity.kind.as_str().to_owned(),
        role: identity.role.as_str().to_owned(),
        on_behalf_of: identity.behalf_handle,
        user_id: identity.user_id,
        feedback: crate::feedback::available_for(identity.role),
    })
}

#[utoipa::path(
    get,
    path = "/v1/openapi.json",
    tag = "meta",
    responses((status = 200, description = "This OpenAPI document"))
)]
async fn openapi_json(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::openapi::spec())
}

pub fn openapi_spec() -> utoipa::openapi::OpenApi {
    crate::openapi::spec()
}

#[cfg(test)]
mod tests {
    use super::counts_as_interactive;

    #[test]
    fn catalog_routes_count_and_long_lived_ones_do_not() {
        assert!(counts_as_interactive("/v1/objects/abc"));
        assert!(counts_as_interactive("/v1/search"));
        assert!(!counts_as_interactive("/healthz"));
        assert!(!counts_as_interactive("/v1/openapi.json"));
        assert!(!counts_as_interactive("/v1/stream"));
        assert!(!counts_as_interactive("/v1/bulk"));
        assert!(!counts_as_interactive(
            "/v1/imports/00000000-0000-0000-0000-000000000000/save"
        ));
        assert!(counts_as_interactive(
            "/v1/imports/00000000-0000-0000-0000-000000000000"
        ));
        assert!(counts_as_interactive(
            "/v1/imports/00000000-0000-0000-0000-000000000000/approve"
        ));
    }
}

mod openapi;
