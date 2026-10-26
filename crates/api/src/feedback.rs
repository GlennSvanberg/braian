//! In-app feedback. Files stay on disk. The webhook only receives paths.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use base64::Engine;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::{Auth, Role};
use crate::problem::Problem;
use crate::state::AppState;

const TIMEOUT: Duration = Duration::from_secs(10);
const MESSAGE_MAX: usize = 8_000;
const SCREENSHOT_MAX: usize = 1_200_000;
const LOG_MAX: usize = 256_000;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX: usize = 8;

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/v1/feedback", post(submit_feedback))
}

pub fn configured() -> bool {
    std::env::var("BRAIAN_FEEDBACK_WEBHOOK_URL")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

/// True only for an admin while the webhook URL is set.
pub fn available_for(role: Role) -> bool {
    role == Role::Admin && configured()
}

pub fn feedback_dir() -> PathBuf {
    std::env::var("BRAIAN_FEEDBACK_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data/feedback"))
}

/// Checked when the API process starts. The error text never includes the header value.
pub fn check_feedback_config() -> Result<(), String> {
    load_webhook()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct WebhookHeader {
    name: String,
    value: String,
}

struct WebhookConfig {
    url: String,
    header: Option<WebhookHeader>,
}

impl std::fmt::Debug for WebhookConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebhookConfig")
            .field("url", &self.url)
            .field(
                "header_name",
                &self.header.as_ref().map(|header| header.name.as_str()),
            )
            .field("header_value", &self.header.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

#[derive(Debug)]
struct ConfigError;

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BRAIAN_FEEDBACK_WEBHOOK_HEADER is not a valid HTTP header")
    }
}

fn load_webhook() -> Result<Option<WebhookConfig>, ConfigError> {
    let Some(url) = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let raw = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_HEADER").unwrap_or_default();
    let secret = std::env::var("BRAIAN_FEEDBACK_WEBHOOK_SECRET").ok();
    let header =
        parse_header(&raw, secret.as_deref())?.map(|(name, value)| WebhookHeader { name, value });
    Ok(Some(WebhookConfig { url, header }))
}

/// A colon means `Name: value` (split on the first colon). Otherwise the
/// variable is the header name and `BRAIAN_FEEDBACK_WEBHOOK_SECRET` is the value.
fn parse_header(raw: &str, secret: Option<&str>) -> Result<Option<(String, String)>, ConfigError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let (name, value) = if let Some((name, value)) = raw.split_once(':') {
        (name.trim(), value.trim())
    } else {
        let Some(secret) = secret.map(str::trim).filter(|value| !value.is_empty()) else {
            validate_header_name(raw)?;
            return Ok(None);
        };
        (raw, secret)
    };
    validate_header(name, value)?;
    Ok(Some((name.to_owned(), value.to_owned())))
}

fn validate_header_name(name: &str) -> Result<(), ConfigError> {
    reqwest::header::HeaderName::from_bytes(name.as_bytes())
        .map(|_| ())
        .map_err(|_| ConfigError)
}

fn validate_header(name: &str, value: &str) -> Result<(), ConfigError> {
    if name.is_empty() || value.is_empty() {
        return Err(ConfigError);
    }
    validate_header_name(name)?;
    reqwest::header::HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|_| ConfigError)
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
    if identity.role != Role::Admin {
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "feedback is limited to admins",
        ));
    }
    let config = match load_webhook() {
        Ok(Some(config)) => config,
        Ok(None) => {
            return Err(Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "feedback_disabled",
                "feedback is not configured",
            ));
        }
        Err(error) => {
            tracing::warn!(%error, "feedback webhook header is not valid");
            return Err(Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "feedback_misconfigured",
                "feedback webhook header is not valid",
            ));
        }
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
    let key = identity
        .user_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| identity.handle.clone());
    if !allow_rate(&key) {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "feedback_rate_limited",
            "too many feedback reports, try again shortly",
        ));
    }

    let screenshot = match body
        .screenshot
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(raw) => Some(decode_png(raw)?),
        None => None,
    };
    let log_bytes = match &body.log {
        Some(Value::Null) | None => None,
        Some(log) => {
            let bytes = serde_json::to_vec(log).map_err(|_| {
                Problem::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_feedback",
                    "log is not json",
                )
            })?;
            if bytes.len() > LOG_MAX {
                return Err(Problem::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_feedback",
                    "log is too large",
                ));
            }
            Some(bytes)
        }
    };

    let id = Uuid::now_v7();
    let dir = feedback_dir();
    let screenshot_path = screenshot.as_ref().map(|_| dir.join(format!("{id}.png")));
    let log_path = log_bytes
        .as_ref()
        .map(|_| dir.join(format!("{id}.log.json")));
    if let Err(error) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!(%error, "feedback directory could not be created");
        return Err(Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "feedback_store_failed",
            "could not store feedback",
        ));
    }
    if let (Some(path), Some(bytes)) = (&screenshot_path, &screenshot)
        && let Err(error) = tokio::fs::write(path, bytes).await
    {
        tracing::warn!(%error, "feedback screenshot could not be stored");
        return Err(Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "feedback_store_failed",
            "could not store feedback",
        ));
    }
    if let (Some(path), Some(bytes)) = (&log_path, &log_bytes)
        && let Err(error) = tokio::fs::write(path, bytes).await
    {
        tracing::warn!(%error, "feedback log could not be stored");
        return Err(Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "feedback_store_failed",
            "could not store feedback",
        ));
    }

    let payload = json!({
        "id": id,
        "message": message,
        "route": field(&body.context, "route"),
        "link": field(&body.context, "link"),
        "view": field(&body.context, "view"),
        "workspace": field(&body.context, "workspace"),
        "object": field(&body.context, "object"),
        "filter": body.context.get("search").and_then(|search| search.get("filter")).cloned().unwrap_or(Value::Null),
        "sort": body.context.get("search").and_then(|search| search.get("sort")).cloned().unwrap_or(Value::Null),
        "build": field(&body.context, "build"),
        "user": {
            "handle": identity.handle,
            "kind": identity.kind.as_str(),
            "role": identity.role.as_str(),
            "user_id": identity.user_id,
        },
        "screenshot_path": screenshot_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "log_path": log_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "submitted_at": Utc::now().to_rfc3339(),
    });

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut request = client.post(&config.url).json(&payload);
    if let Some(header) = &config.header {
        let name = reqwest::header::HeaderName::from_bytes(header.name.as_bytes())
            .expect("header name was validated");
        let value = reqwest::header::HeaderValue::from_str(&header.value)
            .expect("header value was validated");
        request = request.header(name, value);
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

fn field(context: &Value, name: &str) -> Value {
    context.get(name).cloned().unwrap_or(Value::Null)
}

fn decode_png(raw: &str) -> Result<Vec<u8>, Problem> {
    let encoded = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    if encoded.len() > SCREENSHOT_MAX * 2 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_feedback",
            "screenshot is too large",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded))
        .map_err(|_| {
            Problem::new(
                StatusCode::BAD_REQUEST,
                "invalid_feedback",
                "screenshot is not a png",
            )
        })?;
    if bytes.len() > SCREENSHOT_MAX {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_feedback",
            "screenshot is too large",
        ));
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_feedback",
            "screenshot is not a png",
        ));
    }
    Ok(bytes)
}

fn allow_rate(key: &str) -> bool {
    static HITS: Mutex<Option<HashMap<String, VecDeque<Instant>>>> = Mutex::new(None);
    let mut guard = HITS.lock().unwrap_or_else(|poison| poison.into_inner());
    let hits = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let queue = hits.entry(key.to_owned()).or_default();
    while queue
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) > RATE_WINDOW)
    {
        queue.pop_front();
    }
    if queue.len() >= RATE_MAX {
        return false;
    }
    queue.push_back(now);
    true
}

#[cfg(test)]
mod tests {
    use super::{WebhookConfig, WebhookHeader, parse_header};

    #[test]
    fn header_line_splits_on_the_first_colon_and_trims() {
        let (name, value) = parse_header("  Authorization: Bearer abc:def  ", None)
            .unwrap()
            .unwrap();
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer abc:def");
    }

    #[test]
    fn header_without_a_colon_uses_the_secret() {
        let (name, value) = parse_header("X-Braian-Feedback", Some("  hook-secret  "))
            .unwrap()
            .unwrap();
        assert_eq!(name, "X-Braian-Feedback");
        assert_eq!(value, "hook-secret");
    }

    #[test]
    fn invalid_header_error_omits_the_value() {
        let error = parse_header("Bad Name: Bearer abc:def", None).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("BRAIAN_FEEDBACK_WEBHOOK_HEADER"));
        assert!(!text.contains("abc"));
        assert!(!text.contains("Bearer"));
        assert!(!text.contains("Bad Name"));
    }

    #[test]
    fn debug_redacts_the_header_value() {
        let config = WebhookConfig {
            url: "http://hooks.example/feedback".into(),
            header: Some(WebhookHeader {
                name: "Authorization".into(),
                value: "Bearer abc:def".into(),
            }),
        };
        let text = format!("{config:?}");
        assert!(text.contains("[redacted]"));
        assert!(text.contains("Authorization"));
        assert!(!text.contains("abc:def"));
        assert!(!text.contains("Bearer"));
    }
}
