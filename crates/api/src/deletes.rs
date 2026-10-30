//! Direct deletes for a person, and a read-only consequence report.
//!
//! `dry_run=true` counts objects, links, values and selections and writes nothing.
//! An agent or MCP token receives 403 on every delete, including a dry run.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use braian_model::{ModelChange, purge_for};
use braian_rules::CompiledModel;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DryRun {
    #[serde(default)]
    pub dry_run: bool,
}

use crate::auth::{Identity, delete_forbidden};
use crate::model_proposal::model_status;
use crate::problem::{Problem, store_status};
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct ConsequenceReport {
    pub objects: i64,
    pub links: i64,
    pub values: i64,
    pub selections: i64,
    pub destructive: bool,
    pub confirm_name: String,
    pub sentence: String,
    pub detail: Vec<String>,
}

/// Applies one destructive model step, or reports what it would remove.
pub async fn delete_model(
    state: &AppState,
    identity: &Identity,
    dry_run: bool,
    change: ModelChange,
) -> Result<Response, Problem> {
    if !identity.may_commit() {
        return Err(delete_forbidden());
    }
    for _attempt in 0..2 {
        let mut defs = state.store.definition_set().await.map_err(store_status)?;
        let purge = purge_for(&defs, std::slice::from_ref(&change));
        let current = defs.model_revision;
        braian_model::apply_all(&mut defs, std::slice::from_ref(&change)).map_err(model_status)?;
        CompiledModel::compile(&defs).map_err(Problem::definitions)?;
        if dry_run {
            let counts = state
                .store
                .measure_purge(&purge)
                .await
                .map_err(store_status)?;
            let detail = braian_model::consequence_lines(&counts);
            let report = ConsequenceReport {
                objects: counts.objects,
                links: counts.links,
                values: counts.values,
                selections: counts.selections,
                destructive: true,
                confirm_name: purge.confirm_name,
                sentence: detail.first().cloned().unwrap_or_default(),
                detail,
            };
            return Ok(axum::Json(report).into_response());
        }
        defs.model_revision = current + 1;
        match state
            .store
            .put_definitions_purging(&identity.actor, &defs, &purge)
            .await
        {
            Ok(receipt) => {
                state.models.invalidate().await;
                state.emit(serde_json::json!({
                    "seq": receipt.last_seq,
                    "op": "model",
                }));
                return Ok(StatusCode::NO_CONTENT.into_response());
            }
            Err(braian_store::Error::BadRevision { .. }) => continue,
            Err(error) => return Err(store_status(error)),
        }
    }
    Err(Problem::new(
        StatusCode::CONFLICT,
        "revision",
        "the model changed while it was being deleted",
    ))
}
