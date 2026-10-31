use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::router;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

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
    cookie: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, json)
}

async fn call_bearer(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
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
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, json)
}

async fn app() -> (TestDb, axum::Router) {
    load_env();
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), "ada:human:token-ada")
        .await
        .unwrap();
    let app = router(db.store().clone(), identities);
    let defs = include_str!("../../../testdata/rules/definitions.json");
    let (status, body) = call_bearer(&app, "PUT", "/v1/definitions", "token-ada", Some(defs)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (db, app)
}

async fn login(app: &axum::Router, db: &TestDb, email: &str, password: &str) -> (Uuid, String) {
    let hash = braian_api::hash_password(password).unwrap();
    let user = db
        .store()
        .create_user(email, &hash, "editor", "Editor")
        .await
        .unwrap();
    let builder = Request::builder()
        .method("POST")
        .uri("/v1/login")
        .header("content-type", "application/json");
    let request = builder
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_owned();
    (user.id, cookie)
}

async fn save_ws(db: &TestDb, owner: Uuid, name: &str, locked: bool) -> Uuid {
    let id = Uuid::now_v7();
    db.store()
        .save_workspace(id, owner, name, locked, None, &json!({}))
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn delete_workspace_ok_leaves_objects() {
    let (db, app) = app().await;
    let (user, cookie) = login(&app, &db, "del-ok@braian.local", "secret-del").await;
    let keep = save_ws(&db, user, "Keep", false).await;
    let remove = save_ws(&db, user, "Remove", false).await;

    // Object not tied to workspace layout
    let builder = Request::builder()
        .method("POST")
        .uri("/v1/objects")
        .header("cookie", &cookie)
        .header("content-type", "application/json");
    let request = builder
        .body(Body::from(
            r#"{"type":"item","groups":{"catalog":"root"},"values":{"sku":"DEL-1","name":{"en-GB":"Keep me"}}}"#
                .to_owned(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::OK, "{created}");
    let object_id = created["id"].as_str().unwrap().to_owned();

    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/v1/workspaces/{remove}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{remove}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{keep}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/objects/{object_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], object_id);
}

#[tokio::test]
async fn delete_workspace_rejects_locked_and_last() {
    let (db, app) = app().await;
    let (user, cookie) = login(&app, &db, "del-lock@braian.local", "secret-del").await;
    let only = save_ws(&db, user, "Only", false).await;
    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/v1/workspaces/{only}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "last_workspace");

    let locked = save_ws(&db, user, "Locked", true).await;
    let _spare = save_ws(&db, user, "Spare", false).await;
    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/v1/workspaces/{locked}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "locked");
}

#[tokio::test]
async fn delete_workspace_rejects_agent() {
    load_env();
    let db = TestDb::create().await;
    let identities =
        braian_api::identities(db.store(), "ada:human:token-ada;bot:agent@ada:token-bot")
            .await
            .unwrap();
    let app = router(db.store().clone(), identities);
    let hash = braian_api::hash_password("secret-agent").unwrap();
    let user = db
        .store()
        .create_user("del-agent@braian.local", &hash, "editor", "Editor")
        .await
        .unwrap();
    let id = save_ws(&db, user.id, "A", false).await;
    let _ = save_ws(&db, user.id, "B", false).await;
    let builder = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/workspaces/{id}"))
        .header("authorization", "Bearer token-bot");
    let request = builder.body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
