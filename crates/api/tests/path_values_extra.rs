//! Extra pathValues coverage: all + first must not share one response key.
use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::router;
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

#[tokio::test]
async fn path_values_keys_all_and_first_separately() {
    let (_db, app) = app().await;
    let (status, _, host) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"HOST-AF","name":{"sv":"Host"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{host}");
    let mut peer_ids = Vec::new();
    for (sku, grams) in [("P1", "400"), ("P2", "450"), ("P3", "500")] {
        let (status, _, peer) = call(
            &app,
            "POST",
            "/v1/objects",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(
                r#"{{"type":"item","values":{{"sku":"{sku}","name":{{"sv":"{sku}"}},"weight":{{"value":"{grams}","unit":"g"}}}}}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{peer}");
        peer_ids.push(peer["id"].as_str().unwrap().to_owned());
    }
    let (status, _, link_type) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"af_peer","label":"AF","from_types":["item"],"to_types":["item"],"from_key":"articles","from_label":"Articles","to_key":"products","to_label":"Products","from_cardinality":"many","to_cardinality":"many"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{link_type}");
    let host_id = host["id"].as_str().unwrap();
    for peer_id in &peer_ids {
        let (status, _, linked) = call(
            &app,
            "POST",
            "/v1/links",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(
                r#"{{"type":"af_peer","from_id":"{host_id}","to_id":"{peer_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{linked}");
    }
    let all_key = "products>weight#cardinality:all|unit:kg";
    let first_key = "products>weight#cardinality:first|unit:kg";
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects:pathValues",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"ids":["{host_id}"],"paths":[
              {{"path":"products>weight","key":"{all_key}","cardinality":"all","unit":"kg"}},
              {{"path":"products>weight","key":"{first_key}","cardinality":"first","unit":"kg"}}
            ],"type":"item","lang":"en"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let all_cells = body["values"][host_id][all_key].as_array().unwrap();
    let first_cells = body["values"][host_id][first_key].as_array().unwrap();
    assert_eq!(all_cells.len(), 3, "all must keep every peer: {body}");
    assert_eq!(first_cells.len(), 1, "first must truncate: {body}");
    assert_ne!(all_cells, first_cells, "{body}");
}
