use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::routing::post;
use braian_api::router;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;

const TOKENS: &str = "ada:human:token-ada";
const HUMAN: &str = "token-ada";

type Captured = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, BTreeMap<String, String>, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = builder
        .body(match body {
            Some(body) => Body::from(body.to_owned()),
            None => Body::empty(),
        })
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers() {
        if let Ok(text) = value.to_str() {
            headers.insert(name.as_str().to_owned(), text.to_owned());
        }
    }
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, headers, json)
}

async fn app() -> (TestDb, axum::Router) {
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), TOKENS).await.unwrap();
    (db, router(db.store().clone(), identities))
}

async fn mock_hook(captured: Captured) -> String {
    let app = Router::new()
        .route(
            "/hook",
            post(
                |State(captured): State<Captured>, headers: HeaderMap, body: String| async move {
                    let value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    captured.lock().unwrap().push((headers, value));
                    StatusCode::NO_CONTENT
                },
            ),
        )
        .with_state(captured);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}/hook")
}

fn clear_feedback_env() {
    unsafe {
        std::env::remove_var("BRAIAN_FEEDBACK_WEBHOOK_URL");
        std::env::remove_var("BRAIAN_FEEDBACK_WEBHOOK_HEADER");
        std::env::remove_var("BRAIAN_FEEDBACK_WEBHOOK_SECRET");
    }
}

#[tokio::test]
async fn feedback_status_is_off_without_webhook_url() {
    clear_feedback_env();
    let (_db, app) = app().await;
    let (status, _, body) = call(&app, "GET", "/v1/feedback", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
}

#[tokio::test]
async fn feedback_post_is_disabled_without_webhook_url() {
    clear_feedback_env();
    let (_db, app) = app().await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/feedback",
        Some(HUMAN),
        Some(r#"{"message":"hello","context":{}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "feedback_disabled");
}

#[tokio::test]
async fn feedback_post_relays_json_with_configured_header() {
    clear_feedback_env();
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let hook = mock_hook(captured.clone()).await;
    unsafe {
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_URL", &hook);
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_HEADER", "X-Braian-Feedback");
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_SECRET", "hook-secret");
    }
    let (_db, app) = app().await;
    let (status, _, _) = call(
        &app,
        "GET",
        "/v1/feedback",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let payload = r#"{"message":"Found a bug","context":{"route":"/"},"log":[{"kind":"api","message":"GET /v1/me 500 12ms"}],"screenshot":"aGVsbG8="}"#;
    let (status, _, _) = call(&app, "POST", "/v1/feedback", Some(HUMAN), Some(payload)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for _ in 0..40 {
        if !captured.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let rows = captured.lock().unwrap();
    assert_eq!(rows.len(), 1);
    let (headers, body) = &rows[0];
    assert_eq!(
        headers
            .get("x-braian-feedback")
            .and_then(|value| value.to_str().ok()),
        Some("hook-secret")
    );
    assert_eq!(body["message"], "Found a bug");
    assert_eq!(body["context"]["route"], "/");
    assert_eq!(body["log"][0]["kind"], "api");
    assert_eq!(body["screenshot"], "aGVsbG8=");
    assert_eq!(body["user"]["handle"], "ada");
    assert!(body.get("submitted_at").and_then(Value::as_str).is_some());
    clear_feedback_env();
}

#[tokio::test]
async fn feedback_rejects_an_oversized_screenshot() {
    clear_feedback_env();
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let hook = mock_hook(captured).await;
    unsafe {
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_URL", &hook);
    }
    let (_db, app) = app().await;
    let huge = "a".repeat(1_500_001);
    let payload = format!(r#"{{"message":"x","context":{{}},"screenshot":"{huge}"}}"#);
    let (status, _, body) = call(&app, "POST", "/v1/feedback", Some(HUMAN), Some(&payload)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_feedback");
    clear_feedback_env();
}
