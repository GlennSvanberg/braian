use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use braian_model::ValidationError;
use braian_rules::DefinitionError;
use braian_store::Error as StoreError;
use serde::Serialize;
use utoipa::ToSchema;

/// One overlapping edit, named with keys where the model still has them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConflictBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure: Option<String>,
    pub version: u64,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = crate::http_schema::FreeObject)]
    pub current: Option<serde_json::Value>,
}

/// `application/problem+json` body. Callers branch on `code`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: u16,
    pub code: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ValidationError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<ConflictBody>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_errors: Vec<String>,
    /// Objects that block a structure or group delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

impl Problem {
    pub fn new(status: StatusCode, code: &str, detail: impl Into<String>) -> Self {
        Self {
            kind: "about:blank".into(),
            title: status.canonical_reason().unwrap_or("error").to_owned(),
            status: status.as_u16(),
            code: code.to_owned(),
            detail: detail.into(),
            errors: Vec::new(),
            warnings: Vec::new(),
            current_version: None,
            conflicts: Vec::new(),
            definition_errors: Vec::new(),
            count: None,
        }
    }

    pub fn occupied(code: &str, detail: impl Into<String>, count: u64) -> Self {
        let mut problem = Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, detail);
        problem.count = Some(count);
        problem
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or unknown token",
        )
    }

    pub fn validation(errors: Vec<ValidationError>, warnings: Vec<ValidationError>) -> Self {
        let mut problem = Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_object",
            "the write failed validation",
        );
        problem.errors = errors;
        problem.warnings = warnings;
        problem
    }

    pub fn definitions(errors: Vec<DefinitionError>) -> Self {
        let mut problem = Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_definitions",
            "the definition set does not compile",
        );
        problem.definition_errors = errors.into_iter().map(|error| error.message).collect();
        problem
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

pub fn store_status(error: StoreError) -> Problem {
    match error {
        StoreError::NotFound(id) => {
            Problem::new(StatusCode::NOT_FOUND, "not_found", id.to_string())
        }
        StoreError::Deleted(id) => Problem::new(StatusCode::GONE, "deleted", id.to_string()),
        StoreError::LinkRejected(message) => {
            Problem::new(StatusCode::UNPROCESSABLE_ENTITY, "link_rejected", message)
        }
        StoreError::LinkCardinality(message) => {
            Problem::new(StatusCode::CONFLICT, "link_cardinality", message)
        }
        StoreError::DuplicateKey { type_id, key } => Problem::new(
            StatusCode::CONFLICT,
            "duplicate_key",
            format!("key {key} already exists for type {type_id}"),
        ),
        StoreError::BaseAhead { current_version } => {
            let mut problem = Problem::new(
                StatusCode::CONFLICT,
                "base_ahead",
                "base version is newer than the stored object",
            );
            problem.current_version = Some(current_version);
            problem
        }
        StoreError::BadRevision { current, got } => Problem::new(
            StatusCode::CONFLICT,
            "revision_conflict",
            format!("definition revision {got} does not follow {current}"),
        ),
        StoreError::NoDefinitions => Problem::new(
            StatusCode::NOT_FOUND,
            "no_definitions",
            "no definitions stored",
        ),
        StoreError::BadKey(id) => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bad_key",
            format!("object {id} has no string key"),
        ),
        StoreError::UnknownType(id) => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_type",
            id.to_string(),
        ),
        StoreError::AgentRequiresBehalf(id) => Problem::new(
            StatusCode::FORBIDDEN,
            "agent_requires_behalf",
            format!("agent {id} requires on_behalf_of"),
        ),
        StoreError::ProposalNotFound(_) => {
            Problem::new(StatusCode::NOT_FOUND, "not_found", "proposal")
        }
        StoreError::ProposalTransition { from, to, .. } => Problem::new(
            StatusCode::CONFLICT,
            "invalid_transition",
            format!("a proposal cannot move from {from} to {to}"),
        ),
        StoreError::ProposalBusy(_) => Problem::new(
            StatusCode::CONFLICT,
            "conflict",
            "this proposal is already being applied",
        ),
        StoreError::ProposalClaimLost(_) => Problem::new(
            StatusCode::CONFLICT,
            "claim_lost",
            "this proposal is no longer held",
        ),
        other => {
            tracing::error!(error = %other, "store request failed");
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "the request could not be completed",
            )
        }
    }
}
