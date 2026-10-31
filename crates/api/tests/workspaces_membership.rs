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

async fn save_ws(db: &TestDb, owner: Uuid, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    db.store()
        .save_workspace(id, owner, name, false, None, &json!({}))
        .await
        .unwrap();
    id
}

async fn save_view(db: &TestDb, workspace: Uuid, title: &str) -> Uuid {
    db.store()
        .register_view_type("table", "Table", None)
        .await
        .unwrap();
    let id = Uuid::now_v7();
    db.store()
        .save_view(id, workspace, "table", title, "main", 0, &json!({}))
        .await
        .unwrap();
    id
}

struct Pair {
    #[allow(dead_code)]
    a: Uuid,
    cookie_a: String,
    #[allow(dead_code)]
    b: Uuid,
    cookie_b: String,
    b_ws: Uuid,
    b_view: Uuid,
}

async fn pair(app: &axum::Router, db: &TestDb, tag: &str) -> Pair {
    let (a, cookie_a) = login(app, db, &format!("mem-a-{tag}@braian.local"), "secret").await;
    let (b, cookie_b) = login(app, db, &format!("mem-b-{tag}@braian.local"), "secret").await;
    let _a_ws = save_ws(db, a, "A ws").await;
    let b_ws = save_ws(db, b, "B ws").await;
    let b_view = save_view(db, b_ws, "B view").await;
    Pair {
        a,
        cookie_a,
        b,
        cookie_b,
        b_ws,
        b_view,
    }
}

#[tokio::test]
async fn get_workspace_foreign_is_not_found() {
    let (db, app) = app().await;
    let p = pair(&app, &db, "get").await;
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}", p.b_ws),
        Some(&p.cookie_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}", p.b_ws),
        Some(&p.cookie_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "B ws");
}

#[tokio::test]
async fn save_workspace_cannot_overwrite_foreign() {
    let (db, app) = app().await;
    let p = pair(&app, &db, "save").await;
    let (status, body) = call(
        &app,
        "POST",
        "/v1/workspaces",
        Some(&p.cookie_a),
        Some(
            &json!({
                "id": p.b_ws,
                "name": "Hijacked",
                "locked": true,
                "layout": {}
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}", p.b_ws),
        Some(&p.cookie_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "B ws");
    assert_eq!(body["locked"], false);
}

#[tokio::test]
async fn workspace_state_foreign_is_not_found() {
    let (db, app) = app().await;
    let p = pair(&app, &db, "state").await;
    let doc = json!({ "environment": "default", "document": { "tabs": [] } }).to_string();

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/v1/workspaces/{}/state", p.b_ws),
        Some(&p.cookie_a),
        Some(&doc),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}/state", p.b_ws),
        Some(&p.cookie_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/v1/workspaces/{}/state", p.b_ws),
        Some(&p.cookie_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // B can write and read their own state.
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/v1/workspaces/{}/state", p.b_ws),
        Some(&p.cookie_b),
        Some(&doc),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}/state", p.b_ws),
        Some(&p.cookie_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["document"].is_object());
}

#[tokio::test]
async fn save_view_foreign_workspace_is_not_found() {
    let (db, app) = app().await;
    let p = pair(&app, &db, "view").await;
    let (status, body) = call(
        &app,
        "POST",
        "/v1/views",
        Some(&p.cookie_a),
        Some(
            &json!({
                "workspace_id": p.b_ws,
                "type": "table",
                "title": "Stolen",
                "pane": "main",
                "position": 1,
                "state": {}
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Overwrite existing foreign view id also 404.
    let (status, body) = call(
        &app,
        "POST",
        "/v1/views",
        Some(&p.cookie_a),
        Some(
            &json!({
                "id": p.b_view,
                "workspace_id": p.b_ws,
                "type": "table",
                "title": "Renamed by A",
                "pane": "main",
                "position": 0,
                "state": {}
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{}", p.b_ws),
        Some(&p.cookie_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let views = body["views"].as_array().cloned().unwrap_or_default();
    assert!(
        views
            .iter()
            .any(|view| view["id"] == p.b_view.to_string() && view["title"] == "B view"),
        "{views:?}"
    );
}

#[tokio::test]
async fn column_prefs_foreign_view_is_not_found() {
    let (db, app) = app().await;
    let p = pair(&app, &db, "prefs").await;
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/views/{}/column-prefs", p.b_view),
        Some(&p.cookie_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/v1/views/{}/column-prefs", p.b_view),
        Some(&p.cookie_a),
        Some(
            &json!({ "environment": "default", "document": { "version": 2, "columns": [] } })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/v1/views/{}/column-prefs", p.b_view),
        Some(&p.cookie_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/views/{}/column-prefs", p.b_view),
        Some(&p.cookie_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
