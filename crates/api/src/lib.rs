//! HTTP API in front of the store.
//!
//! `Problem` is the response body, so handlers return it by value.
#![allow(clippy::result_large_err)]
//!
//! Definitions can be written one entity at a time or imported as a whole set.
//! Objects are created, patched, read and deleted with validation always on.

mod auth;
mod definitions;
mod headers;
mod objects;
mod problem;
mod state;
mod wire;

use std::sync::Arc;

use axum::Json;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use openpim_store::Store;
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::{Auth, Identity, identities_from};
use crate::state::{AppState, ModelCache};

pub use crate::auth::{IdentitySpec, parse_dev_tokens};

const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Router for one process. `identities` come from [`identities_from`].
pub fn router(store: Store, identities: Vec<Identity>) -> axum::Router {
    let state = AppState {
        store,
        identities: Arc::new(identities),
        models: ModelCache::default(),
    };
    axum::Router::new()
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
        .route("/v1/structures", post(definitions::post_structure))
        .route(
            "/v1/structures/{id}",
            get(definitions::get_structure)
                .put(definitions::put_structure)
                .delete(definitions::delete_structure_route),
        )
        .route("/v1/unit-families", post(definitions::post_unit_family))
        .route(
            "/v1/unit-families/{id}",
            get(definitions::get_unit_family)
                .put(definitions::put_unit_family)
                .delete(definitions::delete_unit_family_route),
        )
        .route("/v1/languages", put(definitions::put_languages))
        .route("/v1/objects", post(objects::create_object))
        .route(
            "/v1/objects/{id}",
            get(objects::get_object)
                .patch(objects::patch_object)
                .delete(objects::delete_object),
        )
        .route("/v1/objects/{id}/changes", get(objects::object_changes))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

pub async fn identities(store: &Store, raw: &str) -> Result<Vec<Identity>, openpim_store::Error> {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf_of: Option<String>,
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
        on_behalf_of: identity.behalf_handle,
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

mod openapi;
