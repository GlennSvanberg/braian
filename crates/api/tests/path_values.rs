use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::{hash_password, router};
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKENS: &str = "ada:human:token-ada;bot:agent@ada:token-bot";
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

async fn seed_linked_items(app: &axum::Router) -> (String, String, String) {
    let (status, _, first) = call(
        app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"HOST","name":{"sv":"Host"},"weight":{"value":"500","unit":"g"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, _, peer_a) = call(
        app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"PEER-A","name":{"sv":"Alpha"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{peer_a}");
    let (status, _, peer_b) = call(
        app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"PEER-B","name":{"sv":"Beta"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{peer_b}");
    let (status, _, link_type) = call(
        app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"bundled_with","label":"Bundled with","from_types":["item"],"to_types":["item"],"from_key":"bundles","from_label":"Bundles","to_key":"bundled_in","to_label":"Bundled in","from_cardinality":"many","to_cardinality":"many"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{link_type}");
    let host = first["id"].as_str().unwrap();
    let a = peer_a["id"].as_str().unwrap();
    let b = peer_b["id"].as_str().unwrap();
    for peer in [a, b] {
        let (status, _, linked) = call(
            app,
            "POST",
            "/v1/links",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(
                r#"{{"type":"bundled_with","from_id":"{host}","to_id":"{peer}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{linked}");
    }
    (host.to_owned(), a.to_owned(), b.to_owned())
}

#[tokio::test]
async fn path_values_return_hop_and_part_paths() {
    let (_db, app) = app().await;
    let (host, _a, _b) = seed_linked_items(&app).await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"ids":["{host}"],"paths":["bundled_in>name","weight.unit"],"type":"item","lang":"sv"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let cells = &body["values"][&host];
    let names = cells["bundled_in>name"].as_array().unwrap();
    assert_eq!(names.len(), 2, "{body}");
    let texts: Vec<&str> = names.iter().map(|item| item.as_str().unwrap()).collect();
    assert!(texts.contains(&"Alpha"), "{texts:?}");
    assert!(texts.contains(&"Beta"), "{texts:?}");
    assert_eq!(cells["weight.unit"], json!(["g"]), "{body}");
}

#[tokio::test]
async fn path_values_reject_over_limit_and_require_auth() {
    let (_db, app) = app().await;
    let ids: Vec<String> = (0..201).map(|_| uuid::Uuid::now_v7().to_string()).collect();
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&json!({ "ids": ids, "paths": ["weight.unit"] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch_too_large");

    let paths: Vec<String> = (0..65).map(|i| format!("weight.unit{i}")).collect();
    // 65 paths — even invalid ones are counted before parse? We count body.paths.len first.
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&json!({ "ids": [uuid::Uuid::now_v7()], "paths": paths }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch_too_large");

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        None,
        &[("content-type", "application/json")],
        Some(r#"{"ids":[],"paths":[]}"#),
    )
    .await;
    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN,
        "{status} {body}"
    );
}

#[tokio::test]
async fn path_values_viewer_may_read() {
    let (db, app) = app().await;
    let hash = hash_password("secret-viewer").unwrap();
    let _user = db
        .store()
        .create_user("viewer-paths@braian.local", &hash, "viewer", "Viewer")
        .await
        .unwrap();
    let (status, headers, body) = call(
        &app,
        "POST",
        "/v1/login",
        None,
        &[("content-type", "application/json")],
        Some(r#"{"email":"viewer-paths@braian.local","password":"secret-viewer"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let cookie = headers
        .get("set-cookie")
        .and_then(|value| value.split(';').next())
        .expect("cookie");
    let (host, _, _) = seed_linked_items(&app).await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        None,
        &[("cookie", cookie), ("content-type", "application/json")],
        Some(&format!(
            r#"{{"ids":["{host}"],"paths":["bundled_in>name"],"type":"item"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["values"][&host]["bundled_in>name"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn path_values_invalid_path_is_400() {
    let (_db, app) = app().await;
    let (host, _, _) = seed_linked_items(&app).await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"ids":["{host}"],"paths":["nope>missing"],"type":"item"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "path");
}
