use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::post;
use braian_api::router;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;

const TOKENS: &str = "ada:human:token-ada;bot:agent@ada:token-bot";
const HUMAN: &str = "token-ada";
const AGENT: &str = "token-bot";
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

type Captured = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let store = db.store().clone();
    let identities = braian_api::identities(&store, TOKENS).await.unwrap();
    (db, router(store, identities))
}

async fn mock_hook(captured: Captured, status: StatusCode) -> String {
    let app = Router::new()
        .route(
            "/hook",
            post(
                move |State(captured): State<Captured>, headers: HeaderMap, body: String| async move {
                    let value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    captured.lock().unwrap().push((headers, value));
                    status
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
        std::env::remove_var("BRAIAN_FEEDBACK_DIR");
    }
}

fn context() -> Value {
    json!({
        "route": "/objects",
        "view": { "id": "view-1", "type": "table", "title": "Articles" },
        "workspace": { "id": "ws-1", "name": "Articles" },
        "object": { "id": "obj-1", "type": "artikel", "title": "Tält" },
        "search": { "filter": "status:draft", "sort": "name" },
        "build": "abc123"
    })
}

#[tokio::test]
async fn feedback_is_off_without_a_webhook_url() {
    let _guard = ENV_LOCK.lock().await;
    clear_feedback_env();
    let (_db, app) = app().await;
    let (status, _, body) = call(&app, "GET", "/v1/me", Some(HUMAN), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["feedback"], false);
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
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/feedback",
        None,
        Some(r#"{"message":"hello","context":{}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn feedback_rejects_a_non_admin() {
    let _guard = ENV_LOCK.lock().await;
    clear_feedback_env();
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let hook = mock_hook(captured.clone(), StatusCode::NO_CONTENT).await;
    unsafe {
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_URL", &hook);
    }
    let (_db, app) = app().await;
    let (status, _, me) = call(&app, "GET", "/v1/me", Some(AGENT), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["feedback"], false);
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/feedback",
        Some(AGENT),
        Some(r#"{"message":"hello","context":{}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(captured.lock().unwrap().is_empty());
    clear_feedback_env();
}

#[tokio::test]
async fn feedback_relays_paths_and_a_full_header_line() {
    let _guard = ENV_LOCK.lock().await;
    clear_feedback_env();
    let dir = std::env::temp_dir().join(format!("braian-feedback-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let hook = mock_hook(captured.clone(), StatusCode::NO_CONTENT).await;
    unsafe {
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_URL", &hook);
        std::env::set_var(
            "BRAIAN_FEEDBACK_WEBHOOK_HEADER",
            "Authorization: Bearer abc:def",
        );
        std::env::set_var("BRAIAN_FEEDBACK_DIR", &dir);
    }
    let (_db, app) = app().await;
    let (status, _, me) = call(&app, "GET", "/v1/me", Some(HUMAN), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["feedback"], true);
    let payload = json!({
        "message": "Found a bug",
        "context": context(),
        "log": [{ "kind": "api", "message": "GET /v1/me 500 12ms" }],
        "screenshot": PNG,
    });
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/feedback",
        Some(HUMAN),
        Some(&payload.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let rows = captured.lock().unwrap();
    assert_eq!(rows.len(), 1);
    let (headers, body) = &rows[0];
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer abc:def")
    );
    assert_eq!(body["message"], "Found a bug");
    assert_eq!(body["route"], "/objects");
    assert_eq!(body["view"]["title"], "Articles");
    assert_eq!(body["workspace"]["name"], "Articles");
    assert_eq!(body["object"]["id"], "obj-1");
    assert_eq!(body["object"]["type"], "artikel");
    assert_eq!(body["object"]["title"], "Tält");
    assert_eq!(body["filter"], "status:draft");
    assert_eq!(body["build"], "abc123");
    assert_eq!(body["user"]["handle"], "ada");
    assert!(body["id"].as_str().is_some());
    let shot = body["screenshot_path"].as_str().unwrap();
    let log = body["log_path"].as_str().unwrap();
    assert!(shot.ends_with(".png"), "{shot}");
    assert!(log.ends_with(".log.json"), "{log}");
    assert!(std::fs::read(shot).unwrap().starts_with(b"\x89PNG"));
    let stored: Value = serde_json::from_slice(&std::fs::read(log).unwrap()).unwrap();
    assert_eq!(stored[0]["message"], "GET /v1/me 500 12ms");
    let wire = body.to_string();
    assert!(!wire.contains(PNG));
    assert!(!wire.contains("GET /v1/me 500"));
    assert!(body.get("screenshot").is_none());
    assert!(body.get("log").is_none());
    clear_feedback_env();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn feedback_reports_a_webhook_failure() {
    let _guard = ENV_LOCK.lock().await;
    clear_feedback_env();
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let hook = mock_hook(captured, StatusCode::INTERNAL_SERVER_ERROR).await;
    unsafe {
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_URL", &hook);
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_HEADER", "X-Braian-Feedback");
        std::env::set_var("BRAIAN_FEEDBACK_WEBHOOK_SECRET", "hook-secret");
    }
    let (_db, app) = app().await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/feedback",
        Some(HUMAN),
        Some(r#"{"message":"still here","context":{"route":"/"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["code"], "feedback_delivery_failed");
    assert!(!body.to_string().contains("hook-secret"));
    clear_feedback_env();
}
