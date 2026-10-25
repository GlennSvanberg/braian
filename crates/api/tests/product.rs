//! Login, proposals, bulk jobs and rollback.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::{hash_password, router};
use braian_store::ProposalRow;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn load_env() {
    if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_some() {
        return;
    }
    let Ok(text) = std::fs::read_to_string("/workspace/.env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if std::env::var_os(key).is_none() {
            unsafe { std::env::set_var(key, value.trim().trim_matches('"')) };
        }
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    cookie: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, String, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
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
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, set_cookie, json)
}

#[tokio::test]
async fn a_large_accept_returns_a_job() {
    load_env();
    let db = TestDb::create().await;
    let store = db.store().clone();
    let identities = braian_api::identities(db.store(), "ada:human:token-ada")
        .await
        .unwrap();
    let actor = identities[0].actor.actor;
    let app = router(store.clone(), identities);
    let proposal_id = Uuid::now_v7();
    let rows: Vec<ProposalRow> = (0..8)
        .map(|position| ProposalRow {
            id: Uuid::now_v7(),
            object_id: None,
            object_key: None,
            type_key: None,
            base_version: None,
            patch: serde_json::json!({}),
            status: "pending".into(),
            conflict: false,
            position,
        })
        .collect();
    store
        .create_proposal(
            proposal_id,
            "bulk accept",
            "test",
            actor,
            None,
            false,
            &rows,
        )
        .await
        .unwrap();
    let (status, _, body) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some("token-ada"),
        None,
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["status"], "running");
    assert_eq!(body["total"], 8);
    let job_id = body["id"].as_str().unwrap();
    let mut finished = Value::Null;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let (status, _, job) = call(
            &app,
            "GET",
            &format!("/v1/jobs/{job_id}"),
            Some("token-ada"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{job}");
        if job["status"] != "running" {
            finished = job;
            break;
        }
    }
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["progress_total"], 8);
}

#[tokio::test]
async fn login_proposal_bulk_and_rollback() {
    load_env();
    let db = TestDb::create().await;
    let store = db.store().clone();
    let hash = hash_password("braian-demo").unwrap();
    store
        .create_user("admin@braian.local", &hash, "admin", "Demo")
        .await
        .unwrap();
    let identities =
        braian_api::identities(db.store(), "ada:human:token-ada;bot:agent@ada:token-bot")
            .await
            .unwrap();
    let app = router(store.clone(), identities);
    let defs = include_str!("../../../testdata/rules/definitions.json");
    let (status, _, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some("token-ada"),
        None,
        Some(defs),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, cookie, me) = call(
        &app,
        "POST",
        "/v1/login",
        None,
        None,
        Some(r#"{"email":"admin@braian.local","password":"braian-demo"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert!(cookie.contains("braian_session="), "{cookie}");
    let session = cookie.split(';').next().unwrap();
    let (status, _, me) = call(&app, "GET", "/v1/me", None, Some(session), None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["role"], "admin");

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some("token-ada"),
        None,
        Some(r#"{"type":"item","values":{"sku":"SKU-1","name":{"sv":"Stol"},"active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let seq = created["seq"].as_i64().unwrap();

    let (status, _, bad) = call(
        &app,
        "POST",
        "/v1/objects",
        Some("token-ada"),
        None,
        Some(r#"{"type":"item","values":{"sku":"","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");

    let (status, _, proposal) = call(
        &app,
        "POST",
        "/v1/operations/mass_edit",
        Some("token-ada"),
        None,
        Some(r#"{"type":"item","values":{"active":false},"limit":1000}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposal}");
    assert!(proposal["rows"].as_i64().unwrap() >= 1, "{proposal}");
    let proposal_id = proposal["id"].as_str().unwrap();
    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some("token-ada"),
        None,
        Some(r#"{}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert!(
        accepted["failed"].as_array().unwrap().is_empty(),
        "{accepted}"
    );

    let (status, _, agent) = call(
        &app,
        "POST",
        "/v1/operations/patch_object",
        Some("token-bot"),
        None,
        Some(&format!(
            r#"{{"id":"{id}","values":{{"active":true}},"base_version":2}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{agent}");
    assert_eq!(agent["needs_approval"], true, "{agent}");

    let (status, _, rolled) = call(
        &app,
        "POST",
        &format!("/v1/objects/{id}/rollback"),
        Some("token-ada"),
        None,
        Some(&format!(r#"{{"seq":{seq}}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rolled}");
    let rollback_id = rolled["id"].as_str().unwrap();
    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{rollback_id}/accept"),
        Some("token-ada"),
        None,
        Some(r#"{}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some("token-ada"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["values"]["active"], true, "{object}");

    let (status, _, job) = call(
        &app,
        "POST",
        "/v1/bulk",
        Some("token-ada"),
        None,
        Some(
            r#"{"objects":[
                {"type":"item","values":{"sku":"BULK-1","active":true}},
                {"type":"item","values":{"sku":"","active":true}}
            ]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let job_id = job["id"].as_str().unwrap();
    let mut finished = Value::Null;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (status, _, body) = call(
            &app,
            "GET",
            &format!("/v1/jobs/{job_id}"),
            Some("token-ada"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["status"] != "running" {
            finished = body;
            break;
        }
    }
    assert_ne!(finished["status"], "running", "{finished}");
    assert!(
        !finished["errors"].as_array().unwrap().is_empty(),
        "{finished}"
    );
    assert!(
        finished["progress_done"].as_i64().unwrap() >= 1,
        "{finished}"
    );

    let (status, _, history) = call(
        &app,
        "GET",
        &format!("/v1/history?object={id}"),
        Some("token-ada"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert!(
        history["changes"].as_array().unwrap().len() >= 2,
        "{history}"
    );
}
