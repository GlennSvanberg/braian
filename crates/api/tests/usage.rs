use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::{AppState, into_router, router};
use braian_store::UsageEventRow;
use braian_store::testing::TestDb;
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKENS: &str = "ada:human:token-ada";
const HUMAN: &str = "token-ada";

fn load_env() {
    if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_some() {
        return;
    }
    let Ok(start) = std::env::current_dir() else {
        return;
    };
    let mut dir = Some(start);
    for _ in 0..6 {
        let Some(current) = dir else { return };
        let path = current.join(".env");
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe { std::env::set_var(key, value.trim_matches('"')) };
                }
            }
            return;
        }
        dir = current.parent().map(std::path::Path::to_path_buf);
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {HUMAN}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(match body {
            Some(body) => Body::from(body.to_owned()),
            None => Body::empty(),
        })
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

#[tokio::test]
async fn ingest_drops_instead_of_writing_postgres() {
    load_env();
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), TOKENS).await.unwrap();
    let mut state = AppState::new(db.store().clone(), identities, None);
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    tx.try_send(Vec::new()).unwrap();
    state.usage_tx = Some(tx);
    let app = into_router(state);
    let event_id = Uuid::now_v7();
    let task_id = Uuid::now_v7();
    let body = json!({
        "events": [{
            "id": event_id,
            "occurred_at": "2026-10-25T12:00:00Z",
            "actor_type": "human",
            "task_id": task_id,
            "task_type": "enrich",
            "session_id": Uuid::now_v7(),
            "operation": "patch_object",
            "object_ids": [],
            "object_count": 1,
            "result": "ok",
            "values": { "namn": "secret-value" }
        }]
    });
    let (status, response) = call(&app, "POST", "/v1/telemetry", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{response}");
    assert_eq!(response["accepted"], 0);
    assert!(response["dropped"].as_u64().unwrap_or(0) >= 1);
    assert!(!response.to_string().contains("secret-value"));
    assert!(
        db.store()
            .usage_event_audit(event_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(db.store().history(None, None, 20).await.unwrap().is_empty());
}

#[tokio::test]
async fn usage_keeps_the_audit_link_and_hides_the_person() {
    load_env();
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), TOKENS).await.unwrap();
    let app = router(db.store().clone(), identities);
    let actor = Uuid::now_v7();
    let batch = Uuid::now_v7();
    let task = Uuid::now_v7();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let start = Utc::now();
    let row = |id, at, operation: &str, task_type: &str, change| UsageEventRow {
        id,
        occurred_at: at,
        actor_type: "human".into(),
        task_id: task,
        task_type: task_type.into(),
        session_id: Uuid::now_v7(),
        operation: operation.into(),
        object_ids: vec![],
        object_count: 1,
        result: "ok".into(),
        duration_ms: None,
        audit_batch_id: Some(batch),
        audit_seq: Some(9),
        actor_id: Some(actor),
        on_behalf_of: Some(actor),
        ambiguous: false,
        is_change: change,
    };
    db.store()
        .insert_usage_events(&[
            row(first, start, "search", "search", false),
            row(
                second,
                start + Duration::seconds(5),
                "patch_object",
                "enrich",
                true,
            ),
        ])
        .await
        .unwrap();
    let audit = db.store().usage_event_audit(first).await.unwrap();
    assert_eq!(audit, Some((Some(batch), Some(9))));
    let (status, body) = call(&app, "GET", "/v1/usage?days=30", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = body.to_string();
    assert!(text.contains("Work is measured per task, not per person."));
    assert!(!text.contains(&actor.to_string()));
    assert!(!text.contains(&batch.to_string()));
    assert!(!text.contains("session"));
    assert!(!text.contains("on_behalf"));
    let enrich = body["by_type"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["task_type"] == "enrich")
        .expect("enrich");
    assert_eq!(enrich["changes_human"], 1);
    assert!(enrich["human_time_ms"].as_i64().unwrap() >= 5_000);
    assert_eq!(enrich["tasks"], 1);
}
