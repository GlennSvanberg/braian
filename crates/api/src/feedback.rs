//! In-app feedback relayed to a configured webhook.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Auth;
use crate::problem::Problem;
use crate::state::AppState;

const TIMEOUT: Duration = Duration::from_secs(15);
/// PNG base64 for a screenshot downscaled to about 1600 px.
const SCREENSHOT_MAX: usize = 1_500_000;
const LOG_MAX: usize = 512_000;
const MESSAGE_MAX: usize = 8_000;

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/feedback", get(feedback_status).post(submit_feedback))
}

pub fn enabled() -> bool {
    std::env::var("BRAIAN_FEEDBACK_WEBHOOK_URL")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn webhook_config() -> Option<(String, String, String)> {
    let url = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    let header = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_HEADER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "authorization".into());
    let secret = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_SECRET").unwrap_or_default();
    Some((url, header, secret))
}

async fn feedback_status() -> Json<Value> {
    Json(json!({ "enabled": enabled() }))
}

#[derive(Debug, Deserialize)]
struct FeedbackBody {
    message: String,
    #[serde(default)]
    context: Value,
    #[serde(default)]
    log: Option<Value>,
    #[serde(default)]
    screenshot: Option<String>,
}

async fn submit_feedback(
    State(_state): State<AppState>,
    Auth(identity): Auth,
    Json(body): Json<FeedbackBody>,
) -> Result<StatusCode, Problem> {
    let Some((url, header_name, secret)) = webhook_config() else {
        return Err(Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "feedback_disabled",
            "feedback is not configured",
        ));
    };
    let message = body.message.trim();
    if message.is_empty() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_feedback",
            "message is required",
        ));
    }
    if message.len() > MESSAGE_MAX {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_feedback",
            "message is too long",
        ));
    }
    if let Some(log) = &body.log {
        let size = serde_json::to_string(log).map(|text| text.len()).unwrap_or(LOG_MAX + 1);
        if size > LOG_MAX {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_feedback",
                "log is too large",
            ));
        }
    }
    if let Some(screenshot) = &body.screenshot {
        let payload = screenshot.strip_prefix("data:image/png;base64,").unwrap_or(screenshot);
        if payload.len() > SCREENSHOT_MAX {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_feedback",
                "screenshot is too large",
            ));
        }
    }
    let payload = json!({
        "message": message,
        "context": body.context,
        "log": body.log,
        "screenshot": body.screenshot,
        "user": {
            "handle": identity.handle,
            "kind": identity.kind.as_str(),
            "role": identity.role.as_str(),
            "user_id": identity.user_id,
        },
        "submitted_at": Utc::now().to_rfc3339(),
    });
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut request = client.post(&url).json(&payload);
    if !secret.is_empty() {
        request = request.header(&header_name, secret.as_str());
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "feedback webhook request failed");
            return Err(Problem::new(
                StatusCode::BAD_GATEWAY,
                "feedback_delivery_failed",
                "could not deliver feedback",
            ));
        }
    };
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "feedback webhook rejected the payload");
        return Err(Problem::new(
            StatusCode::BAD_GATEWAY,
            "feedback_delivery_failed",
            "feedback was rejected by the webhook",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
