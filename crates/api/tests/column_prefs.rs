use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::{hash_password, router, token_hash};
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKENS: &str = "ada:human:token-ada;bot:agent@ada:token-bot;feed:integration:token-import";
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
    token: Option<&str>,
    extra: &[(&str, &str)],
    body: Option<&str>,
) -> (StatusCode, BTreeMap<String, String>, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
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
    load_env();
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), TOKENS).await.unwrap();
    let app = router(db.store().clone(), identities);
    let defs = include_str!("../../../testdata/rules/definitions.json");
    let (status, _, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(defs),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (db, app)
}

async fn login_cookie(app: &axum::Router, email: &str, password: &str) -> String {
    let (status, headers, body) = call(
        app,
        "POST",
        "/v1/login",
        None,
        &[("content-type", "application/json")],
        Some(&format!(r#"{{"email":"{email}","password":"{password}"}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    headers
        .get("set-cookie")
        .and_then(|value| value.split(';').next())
        .expect("session cookie")
        .to_owned()
}

async fn seed_object_table_view(db: &TestDb, owner: Uuid) -> Uuid {
    let workspace = Uuid::now_v7();
    db.store()
        .save_workspace(
            workspace,
            owner,
            "Prefs bench",
            false,
            None,
            &json!({ "type": "tabs", "views": ["v1"], "active": 0 }),
        )
        .await
        .unwrap();
    let view = Uuid::now_v7();
    db.store()
        .save_view(
            view,
            workspace,
            "object_table",
            "Items",
            "center",
            0,
            &json!({ "type": "item" }),
        )
        .await
        .unwrap();
    view
}

#[tokio::test]
async fn column_prefs_round_trip_for_a_viewer() {
    let (db, app) = app().await;
    let hash = hash_password("secret-prefs").unwrap();
    let user = db
        .store()
        .create_user("viewer-prefs@braian.local", &hash, "viewer", "Viewer")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    let cookie = login_cookie(&app, "viewer-prefs@braian.local", "secret-prefs").await;

    let (status, _, empty) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert!(empty["document"].is_null(), "{empty}");

    let document = json!({
        "version": 2,
        "columns": [
            { "path": "sku", "visible": true },
            { "path": "weight.unit", "visible": false }
        ]
    });
    let (status, _, saved) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({ "environment": "default", "document": document }))
                .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["document"]["columns"][0]["path"], "sku");
    assert_eq!(saved["document"]["columns"][1]["visible"], false);

    let (status, _, loaded) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{loaded}");
    let loaded_paths: Vec<&str> = loaded["document"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["path"].as_str().unwrap())
        .collect();
    assert_eq!(loaded_paths, ["sku", "weight.unit", "type", "version"]);
    assert!(loaded["stale_paths"].as_array().unwrap().is_empty());

    let (status, _, _) = call(
        &app,
        "DELETE",
        &format!("/v1/views/{view}/column-prefs?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, again) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(again["document"].is_null());
}

#[tokio::test]
async fn column_prefs_are_private_to_the_owner() {
    let (db, app) = app().await;
    let hash = hash_password("secret-a").unwrap();
    let alice = db
        .store()
        .create_user("alice-prefs@braian.local", &hash, "editor", "Alice")
        .await
        .unwrap();
    let bob = db
        .store()
        .create_user(
            "bob-prefs@braian.local",
            &hash_password("secret-b").unwrap(),
            "editor",
            "Bob",
        )
        .await
        .unwrap();
    let view = seed_object_table_view(&db, alice.id).await;
    let alice_cookie = login_cookie(&app, "alice-prefs@braian.local", "secret-a").await;
    let bob_cookie = login_cookie(&app, "bob-prefs@braian.local", "secret-b").await;

    let document = json!({
        "version": 2,
        "columns": [{ "path": "sku", "visible": true }]
    });
    let (status, _, _) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[
            ("cookie", &alice_cookie),
            ("content-type", "application/json"),
        ],
        Some(&serde_json::to_string(&json!({ "document": document })).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, bob_get) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &bob_cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bob_get}");
    assert!(bob_get["document"].is_null(), "{bob_get}");

    let other = json!({
        "version": 2,
        "columns": [{ "path": "name", "visible": true }]
    });
    let (status, _, _) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[
            ("cookie", &bob_cookie),
            ("content-type", "application/json"),
        ],
        Some(&serde_json::to_string(&json!({ "document": other })).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, alice_get) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &alice_cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{alice_get}");
    assert_eq!(alice_get["document"]["columns"][0]["path"], "sku");

    // Bob cannot delete Alice's row by targeting the same view — delete only clears Bob's.
    let (status, _, _) = call(
        &app,
        "DELETE",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &bob_cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, alice_still) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &alice_cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{alice_still}");
    assert_eq!(alice_still["document"]["columns"][0]["path"], "sku");
    let _ = bob;
}

#[tokio::test]
async fn column_prefs_put_rejects_invalid_documents() {
    let (db, app) = app().await;
    let hash = hash_password("secret-bad").unwrap();
    let user = db
        .store()
        .create_user("bad-prefs@braian.local", &hash, "viewer", "Bad")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    let cookie = login_cookie(&app, "bad-prefs@braian.local", "secret-bad").await;

    let cases = [
        (json!({ "version": 1, "columns": [] }), "version"),
        (
            json!({ "version": 2, "columns": [{ "path": "no_such_attr", "visible": true }] }),
            "path",
        ),
        (
            json!({
                "version": 2,
                "columns": [
                    { "path": "sku", "visible": true },
                    { "path": "sku", "visible": false }
                ]
            }),
            "duplicate",
        ),
        (
            json!({
                "version": 2,
                "columns": [{ "path": "a>b>c", "visible": true }]
            }),
            "via",
        ),
    ];
    for (document, code) in cases {
        let (status, _, body) = call(
            &app,
            "PUT",
            &format!("/v1/views/{view}/column-prefs"),
            None,
            &[("cookie", &cookie), ("content-type", "application/json")],
            Some(&serde_json::to_string(&json!({ "document": document })).unwrap()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code} => {body}");
        assert_eq!(body["code"], code, "{body}");
    }

    let huge_columns: Vec<Value> = (0..201)
        .map(|i| json!({ "path": format!("sku{i}"), "visible": true }))
        .collect();
    // Unknown paths also 400; oversized array of duplicates-of-unknown still hits columns limit first
    // when we use unique fake paths — path validation runs per column after length check.
    let (status, _, body) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({
                "document": { "version": 2, "columns": huge_columns }
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "columns", "{body}");

    // Payload too large: many long path strings under the column cap.
    let fat: Vec<Value> = (0..50)
        .map(|i| {
            json!({
                "path": format!("x{}", "y".repeat(2000)),
                "visible": true,
                "note": format!("{i}"),
            })
        })
        .collect();
    let (status, _, body) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({
                "document": { "version": 2, "columns": fat }
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["code"] == "document" || body["code"] == "path",
        "{body}"
    );
}

#[tokio::test]
async fn column_prefs_get_flags_stale_paths_without_500() {
    let (db, app) = app().await;
    let hash = hash_password("secret-stale").unwrap();
    let user = db
        .store()
        .create_user("stale-prefs@braian.local", &hash, "viewer", "Stale")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    // Bypass PUT validation to simulate a path that later became invalid.
    db.store()
        .put_view_column_prefs(
            user.id,
            "default",
            view,
            &json!({
                "version": 2,
                "columns": [
                    { "path": "sku", "visible": true },
                    { "path": "removed_attr", "visible": true }
                ]
            }),
        )
        .await
        .unwrap();
    let cookie = login_cookie(&app, "stale-prefs@braian.local", "secret-stale").await;
    let (status, _, body) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let columns = body["document"]["columns"].as_array().unwrap();
    let stale_paths_order: Vec<&str> = columns
        .iter()
        .map(|column| column["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        stale_paths_order,
        ["sku", "removed_attr", "type", "version"]
    );
    assert!(columns[0].get("stale").is_none() || columns[0]["stale"] == false);
    assert_eq!(columns[1]["stale"], true);
    assert!(columns[2].get("stale").is_none() || columns[2]["stale"] == false);
    assert!(
        body["stale_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "removed_attr"),
        "{body}"
    );
}

#[tokio::test]
async fn column_prefs_agent_ops_use_owner_and_skip_approval() {
    let (db, app) = app().await;
    let hash = hash_password("secret-agent").unwrap();
    let user = db
        .store()
        .create_user("agent-prefs@braian.local", &hash, "editor", "AgentOwner")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    let raw = "agent-prefs-token-1";
    db.store()
        .insert_api_token(&user, "prefs-bot", "agent", &token_hash(raw))
        .await
        .unwrap();

    let document = json!({
        "version": 2,
        "columns": [{ "path": "sku", "visible": true }]
    });
    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/operations/put_view_column_prefs",
        Some(raw),
        &[("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({
                "view": view,
                "environment": "default",
                "document": document
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved.get("proposal_id").is_none(), "{saved}");
    assert_eq!(saved["document"]["columns"][0]["path"], "sku");

    let (status, _, loaded) = call(
        &app,
        "POST",
        "/v1/operations/get_view_column_prefs",
        Some(raw),
        &[("content-type", "application/json")],
        Some(&serde_json::to_string(&json!({ "view": view })).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{loaded}");
    let agent_paths: Vec<&str> = loaded["document"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["path"].as_str().unwrap())
        .collect();
    assert_eq!(agent_paths, ["sku", "type", "version"]);
}

#[tokio::test]
async fn column_prefs_rows_cascade_when_view_or_user_is_removed() {
    let (db, app) = app().await;
    let hash = hash_password("secret-cascade").unwrap();
    let owner = db
        .store()
        .create_user("cascade-owner@braian.local", &hash, "viewer", "Owner")
        .await
        .unwrap();
    let other = db
        .store()
        .create_user(
            "cascade-prefs@braian.local",
            &hash_password("secret-cascade-2").unwrap(),
            "viewer",
            "PrefsUser",
        )
        .await
        .unwrap();
    let view = seed_object_table_view(&db, owner.id).await;
    db.store()
        .put_view_column_prefs(
            other.id,
            "default",
            view,
            &json!({ "version": 2, "columns": [{ "path": "sku", "visible": true }] }),
        )
        .await
        .unwrap();
    assert!(
        db.store()
            .view_column_prefs(other.id, "default", view)
            .await
            .unwrap()
            .is_some()
    );
    db.store().delete_view(view).await.unwrap();
    assert!(
        db.store()
            .view_column_prefs(other.id, "default", view)
            .await
            .unwrap()
            .is_none()
    );

    let view2 = seed_object_table_view(&db, owner.id).await;
    db.store()
        .put_view_column_prefs(
            other.id,
            "default",
            view2,
            &json!({ "version": 2, "columns": [{ "path": "sku", "visible": true }] }),
        )
        .await
        .unwrap();
    // Delete the prefs owner; rows must go with ON DELETE CASCADE on users.
    {
        let (client, connection) =
            tokio_postgres::connect(&db.database_url(), tokio_postgres::NoTls)
                .await
                .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .execute("DELETE FROM users WHERE id = $1", &[&other.id])
            .await
            .unwrap();
    }
    assert!(
        db.store()
            .view_column_prefs(other.id, "default", view2)
            .await
            .unwrap()
            .is_none()
    );
    let _ = app;
}

#[tokio::test]
async fn column_prefs_migration_sql_is_idempotent() {
    load_env();
    let db = TestDb::create().await;
    let sql = include_str!("../../store/migrations/V26__view_column_prefs.sql");
    let (client, connection) = tokio_postgres::connect(&db.database_url(), tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client.batch_execute(sql).await.expect("first apply");
    client.batch_execute(sql).await.expect("second apply");
    let exists: bool = client
        .query_one(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_name = 'view_column_prefs'
            )",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(exists);
}

#[tokio::test]
async fn column_prefs_accept_system_view_columns() {
    let (db, app) = app().await;
    let hash = hash_password("secret-sys").unwrap();
    let user = db
        .store()
        .create_user("sys-prefs@braian.local", &hash, "viewer", "Sys")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    let cookie = login_cookie(&app, "sys-prefs@braian.local", "secret-sys").await;
    let document = json!({
        "version": 2,
        "columns": [
            { "path": "sku", "visible": true },
            { "path": "type", "visible": true },
            { "path": "version", "visible": false }
        ]
    });
    let (status, _, saved) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({ "environment": "default", "document": document }))
                .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["document"]["columns"][1]["path"], "type");
    assert_eq!(saved["document"]["columns"][2]["visible"], false);

    let (status, _, body) = call(
        &app,
        "PUT",
        &format!("/v1/views/{view}/column-prefs"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({
                "environment": "default",
                "document": {
                    "version": 2,
                    "columns": [
                        { "path": "type", "visible": true },
                        { "path": "type", "visible": false }
                    ]
                }
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn column_prefs_get_injects_missing_system_columns() {
    let (db, app) = app().await;
    let hash = hash_password("secret-inject").unwrap();
    let user = db
        .store()
        .create_user("inject-prefs@braian.local", &hash, "viewer", "Inject")
        .await
        .unwrap();
    let view = seed_object_table_view(&db, user.id).await;
    db.store()
        .put_view_column_prefs(
            user.id,
            "default",
            view,
            &json!({
                "version": 2,
                "columns": [
                    { "path": "number", "visible": true },
                    { "path": "name", "visible": true },
                    { "path": "status", "visible": true }
                ]
            }),
        )
        .await
        .unwrap();
    let cookie = login_cookie(&app, "inject-prefs@braian.local", "secret-inject").await;
    let (status, _, body) = call(
        &app,
        "GET",
        &format!("/v1/views/{view}/column-prefs?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let columns = body["document"]["columns"].as_array().unwrap();
    let paths: Vec<&str> = columns
        .iter()
        .map(|column| column["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["number", "name", "type", "status", "version"]);
    assert_eq!(columns[2]["visible"], true);
    assert_eq!(columns[4]["visible"], true);
}
