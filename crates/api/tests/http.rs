use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::router;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKENS: &str = "ada:human:token-ada;bot:agent@ada:token-bot;feed:integration:token-import";
const HUMAN: &str = "token-ada";
const AGENT: &str = "token-bot";
const IMPORT: &str = "token-import";

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

fn codes(body: &Value) -> Vec<String> {
    body["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|error| error["code"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn auth_rejects_a_missing_or_unknown_token() {
    let (_db, app) = app().await;
    let (status, _, body) = call(&app, "GET", "/v1/me", None, &[], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, _, _) = call(&app, "GET", "/v1/me", Some("nope"), &[], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, body) = call(&app, "GET", "/v1/me", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["handle"], "ada");
    assert_eq!(body["kind"], "human");
    let (status, _, body) = call(&app, "GET", "/v1/me", Some(AGENT), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["on_behalf_of"], "ada");
}

#[tokio::test]
async fn an_agent_token_without_a_person_is_rejected_at_startup() {
    load_env();
    let db = TestDb::create().await;
    let error = braian_api::identities(db.store(), "ada:agent:secret")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("behalf") || error.to_string().contains("@behalf"));
}

#[tokio::test]
async fn objects_round_trip_by_key_or_uuid_and_answer_with_keys() {
    let (_db, app) = app().await;
    let (status, headers, body) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "root", "channel": "flat" },
                "values": { "sku": "SKU-1", "name": { "sv": "Stol" }, "active": true }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["type"], "item");
    assert_eq!(body["groups"]["catalog"], "root");
    assert_eq!(body["values"]["sku"], "SKU-1");
    assert_eq!(body["version"], 1);
    assert_eq!(headers.get("etag").map(String::as_str), Some("\"1\""));
    let id = body["id"].as_str().unwrap();

    let (status, headers, got) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["values"]["name"]["sv"], "Stol");
    assert_eq!(headers.get("etag").map(String::as_str), Some("\"1\""));

    let (status, _, by_id) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "018f0000-0000-7000-8000-000000000200",
                "groups": {
                    "018f0000-0000-7000-8000-000000000300": "018f0000-0000-7000-8000-000000000400"
                },
                "values": { "018f0000-0000-7000-8000-000000000100": "SKU-UUID" }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_id}");
    assert_eq!(by_id["type"], "item");
    assert_eq!(by_id["values"]["sku"], "SKU-UUID");
    assert!(by_id["groups"]["catalog"].is_string());
}

#[tokio::test]
async fn invalid_writes_are_422_and_a_patch_needs_if_match() {
    let (_db, app) = app().await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "root" },
                "values": {
                    "sku": "SKU-BAD",
                    "name": { "sv": "12345678901" },
                    "color": "green"
                }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let found = codes(&body);
    assert!(found.iter().any(|code| code == "text_too_long"), "{body}");
    assert!(
        found.iter().any(|code| code == "value_not_in_list"),
        "{body}"
    );
    assert!(
        body["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| { error["code"] == "text_too_long" && error["language"] == "sv" }),
        "{body}"
    );

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-OK"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, missing) = call(
        &app,
        "PATCH",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"values":{"active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED, "{missing}");
    assert_eq!(missing["code"], "if_match_required");
}

#[tokio::test]
async fn patches_merge_per_language_and_conflict_on_the_same_one() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-MERGE","name":{"sv":"Ett"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let path = format!("/v1/objects/{id}");
    let headers = [("content-type", "application/json"), ("if-match", "1")];
    let (status, _, swedish) = call(
        &app,
        "PATCH",
        &path,
        Some(HUMAN),
        &headers,
        Some(r#"{"values":{"name":{"sv":"Two"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{swedish}");
    let (status, _, english) = call(
        &app,
        "PATCH",
        &path,
        Some(HUMAN),
        &headers,
        Some(r#"{"values":{"name":{"en-GB":"Two"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{english}");
    assert_eq!(english["values"]["name"]["sv"], "Two");
    assert_eq!(english["values"]["name"]["en-GB"], "Two");
    assert_eq!(english["version"], 3);

    let (status, _, again) = call(
        &app,
        "PATCH",
        &path,
        Some(HUMAN),
        &headers,
        Some(r#"{"values":{"name":{"sv":"Tre"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "conflict");
    assert_eq!(again["conflicts"][0]["attribute"], "name");
    assert_eq!(again["conflicts"][0]["language"], "sv");
    assert_eq!(again["conflicts"][0]["actor"], "ada");
}

#[tokio::test]
async fn old_errors_warn_and_inapplicable_values_stay_hidden() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"type":"item","groups":{"catalog":"root"},"values":{"sku":"SKU-WARN","name":{"sv":"Stol"},"active":true,"tags":["abcd"]}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();

    let (status, _, attribute) =
        call(&app, "GET", "/v1/attributes/tags", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{attribute}");
    let mut attribute = attribute;
    attribute["kind"]["max_length"] = json!(3);
    let (status, _, saved) = call(
        &app,
        "PUT",
        "/v1/attributes/tags",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&attribute.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");

    let (status, _, got) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert!(
        got["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "text_too_long"),
        "{got}"
    );

    let (status, _, edited) = call(
        &app,
        "PATCH",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("content-type", "application/json"), ("if-match", "1")],
        Some(r#"{"values":{"color":"red"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["values"]["color"], "red");
    assert!(
        edited["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "text_too_long"),
        "{edited}"
    );

    let (status, _, blocked) = call(
        &app,
        "PATCH",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("content-type", "application/json"), ("if-match", "2")],
        Some(r#"{"values":{"tags":["1234"]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{blocked}");
    assert!(codes(&blocked).iter().any(|code| code == "text_too_long"));

    let (status, _, object_type) =
        call(&app, "GET", "/v1/types/item", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{object_type}");
    let mut object_type = object_type;
    let attributes = object_type["attributes"].as_array_mut().unwrap();
    attributes.retain(|id| id != "018f0000-0000-7000-8000-000000000105");
    let (status, _, _) = call(
        &app,
        "PUT",
        "/v1/types/item",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&object_type.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, hidden) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert!(hidden["values"].get("active").is_none(), "{hidden}");
    let (status, _, shown) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}?include=inapplicable"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["values"]["active"], true);
}

#[tokio::test]
async fn yaml_import_and_compile_errors() {
    let (_db, app) = app().await;
    let defs: Value =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    let yaml = serde_norway::to_string(&defs).unwrap();
    let (status, _, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some(HUMAN),
        &[("content-type", "application/yaml")],
        Some(&yaml),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["model_revision"], 2);

    let (status, headers, listed) = call(
        &app,
        "GET",
        "/v1/definitions",
        Some(HUMAN),
        &[("accept", "application/yaml")],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers.get("content-type").unwrap().contains("yaml"),
        "{headers:?}"
    );
    assert!(listed.as_str().unwrap_or("").contains("sku") || listed["attributes"].is_array());

    let mut broken = defs;
    broken["attributes"][0]["key"] = json!("018f0000-0000-7000-8000-000000000999");
    let (status, _, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&broken.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "invalid_definitions");
}

#[tokio::test]
async fn a_person_save_is_attributed_and_delete_returns_gone() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-HUMAN","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, changes) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/changes"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changes}");
    assert_eq!(changes[0]["actor"], "ada");
    assert_eq!(changes[0]["op"], "create");
    assert!(changes[0].get("via").is_none(), "{changes}");

    let (status, _, deleted) = call(
        &app,
        "DELETE",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("if-match", "1")],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    let (status, _, gone) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{gone}");
    assert_eq!(gone["code"], "deleted");
}

#[tokio::test]
async fn an_agent_or_import_cannot_commit_and_a_person_save_still_does() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-GUARD","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let path = format!("/v1/objects/{id}");

    let (status, _, suggested) = call(
        &app,
        "PATCH",
        &path,
        Some(AGENT),
        &[("content-type", "application/json"), ("if-match", "1")],
        Some(r#"{"values":{"active":false}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{suggested}");
    assert_eq!(suggested["needs_approval"], true, "{suggested}");
    assert_eq!(suggested["source"], "agent");
    let proposal_id = suggested["proposal_id"].as_str().unwrap();

    let (status, _, imported) = call(
        &app,
        "PATCH",
        &path,
        Some(IMPORT),
        &[("content-type", "application/json"), ("if-match", "1")],
        Some(r#"{"values":{"active":false}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{imported}");
    assert_eq!(imported["source"], "import");

    let (status, _, current) = call(&app, "GET", &path, Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["version"], 1, "{current}");
    assert_eq!(current["values"]["active"], true, "{current}");

    let (status, _, denied) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(AGENT),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    let (status, _, denied) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert!(
        accepted["failed"].as_array().unwrap().is_empty(),
        "{accepted}"
    );
    let (status, _, saved) = call(&app, "GET", &path, Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["values"]["active"], false, "{saved}");
    assert_eq!(saved["version"], 2, "{saved}");

    let (status, _, human) = call(
        &app,
        "PATCH",
        &path,
        Some(HUMAN),
        &[("content-type", "application/json"), ("if-match", "2")],
        Some(r#"{"values":{"active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{human}");
    assert_eq!(human["version"], 3, "{human}");
    assert_eq!(human["values"]["active"], true, "{human}");

    let (status, _, blocked) = call(
        &app,
        "DELETE",
        &path,
        Some(AGENT),
        &[("if-match", "3")],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{blocked}");
    assert_eq!(blocked["code"], "agent_cannot_delete");
    let (status, _, still) = call(&app, "GET", &path, Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert_eq!(still["version"], 3);

    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-ONLY-AGENT","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{drafted}");
    let (status, _, listed) = call(
        &app,
        "GET",
        "/v1/objects?q=SKU-ONLY-AGENT",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["objects"].as_array().unwrap().len(), 0, "{listed}");

    let (status, _, op) = call(
        &app,
        "POST",
        "/v1/operations/patch_object",
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"id":"{id}","values":{{"active":false}},"base_version":3}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{op}");
    assert_eq!(op["needs_approval"], true, "{op}");
    let (status, _, after_op) = call(&app, "GET", &path, Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after_op}");
    assert_eq!(after_op["version"], 3, "{after_op}");
    assert_eq!(after_op["values"]["active"], true, "{after_op}");

    for token in [AGENT, IMPORT] {
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/bulk",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"objects":[{"type":"item","values":{"sku":"SKU-BULK","active":true}}]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {body}");
        assert_eq!(body["code"], "needs_approval");
        let (status, _, body) = call(
            &app,
            "PUT",
            "/v1/languages",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"["sv","en"]"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/assortments",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"name":"Unattended","query":{"type":"item"}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/proposals/018f1000-0000-7000-8000-000000000099/accept",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"skip_failed":true}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/structures:create",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"key":"unattended","label":"Unattended"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {body}");
    }

    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    assert!(
        !defs["structures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["key"] == "unattended"),
        "{defs}"
    );
    let (status, _, assortments) =
        call(&app, "GET", "/v1/assortments", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{assortments}");
    assert_eq!(assortments["assortments"].as_array().unwrap().len(), 0);

    for token in [AGENT, IMPORT] {
        let (status, _, ops) = call(&app, "GET", "/v1/operations", Some(token), &[], None).await;
        assert_eq!(status, StatusCode::OK, "{ops}");
        let names = ops["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|op| op["name"].as_str())
            .collect::<Vec<_>>();
        assert!(!names.contains(&"accept_proposal"), "{token} {ops}");
        assert!(!names.contains(&"discard_proposal"), "{token} {ops}");
        assert!(names.contains(&"mass_edit"), "{token} {ops}");
        let (status, _, denied) = call(
            &app,
            "POST",
            "/v1/operations/accept_proposal",
            Some(token),
            &[("content-type", "application/json")],
            Some(&format!(r#"{{"id":"{proposal_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {denied}");
        assert_eq!(denied["code"], "agent_cannot_accept");
    }
    let (status, _, human_ops) = call(&app, "GET", "/v1/operations", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{human_ops}");
    let human_names = human_ops["operations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|op| op["name"].as_str())
        .collect::<Vec<_>>();
    assert!(human_names.contains(&"accept_proposal"), "{human_ops}");
    assert!(human_names.contains(&"discard_proposal"), "{human_ops}");
}

#[tokio::test]
async fn a_person_keeps_one_field_and_an_agent_relation_stays_a_proposal() {
    let (_db, app) = app().await;
    let (status, _, left) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-FIELD","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{left}");
    let id = left["id"].as_str().unwrap();
    let (status, _, right) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-OTHER","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{right}");
    let other = right["id"].as_str().unwrap();

    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Agent draft","source":"agent","rows":[{{"object_id":"{id}","object_key":"SKU-FIELD","type_key":"item","base_version":1,"patch":{{"values":{{"sku":"SKU-CHANGED","active":false}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    let row_id = drafted["rows"][0]["id"].as_str().unwrap();

    let (status, _, kept) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"rows":["{row_id}"],"patches":{{"{row_id}":{{"values":{{"active":false}}}}}}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert!(kept["failed"].as_array().unwrap().is_empty(), "{kept}");
    let (status, _, saved) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["values"]["active"], false, "{saved}");
    assert_eq!(saved["values"]["sku"], "SKU-FIELD", "{saved}");

    let (status, _, rejected) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Import draft","source":"import","rows":[{{"object_id":"{id}","object_key":"SKU-FIELD","type_key":"item","base_version":2,"patch":{{"values":{{"sku":"SKU-IMPORT"}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    let import_id = rejected["id"].as_str().unwrap();
    let import_row = rejected["rows"][0]["id"].as_str().unwrap();
    let (status, _, dropped) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{import_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"rows":["{import_row}"],"patches":{{"{import_row}":{{"values":{{}}}}}}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dropped}");
    let (status, _, still) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert_eq!(still["values"]["sku"], "SKU-FIELD", "{still}");
    let (status, _, proposal) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{import_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposal}");
    assert_eq!(proposal["status"], "discarded", "{proposal}");
    assert_eq!(proposal["source"], "import");
    assert!(proposal["actor_name"].is_string(), "{proposal}");

    let (status, _, link_type) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"related","label":"Related","from_types":["item"],"to_types":["item"],"from_key":"related_by","from_label":"Related by","to_key":"related","to_label":"Related","from_cardinality":"many","to_cardinality":"many"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{link_type}");
    let (status, _, missing) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/relations"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, _, relation) = call(
        &app,
        "POST",
        "/v1/operations/create_link",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"type":"related","from_id":"{id}","to_id":"{other}"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{relation}");
    assert_eq!(relation["needs_approval"], true, "{relation}");
    let (status, _, graph) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/graph"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    assert!(graph["links"].as_array().unwrap().is_empty(), "{graph}");

    let (status, _, listed) = call(&app, "GET", "/v1/proposals", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let pending = &listed["pending"];
    assert!(pending["total"].as_i64().unwrap() >= 1, "{listed}");
    let relation_id = relation["proposal_id"].as_str().expect("link proposal");
    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{relation_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert!(
        accepted["failed"].as_array().unwrap().is_empty(),
        "{accepted}"
    );
    let (status, _, linked) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/graph"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["links"].as_array().unwrap().len(), 1, "{linked}");
}

#[tokio::test]
async fn a_model_command_stays_a_proposal_until_a_person_accepts() {
    let (_db, app) = app().await;
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/operations/model.command",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{ "text": "add attribute voltage to Drill" }"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["needs_approval"], true);
    assert_eq!(drafted["source"], "user");
    let lines = drafted["consequence"]["lines"].as_array().expect("lines");
    let text = lines
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Drill"), "{text}");
    assert!(text.contains("voltage"), "{text}");
    let id = drafted["proposal_id"].as_str().unwrap();

    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert!(
        !before["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["key"] == "voltage")
    );

    let (status, _, denied) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{id}/accept"),
        Some(AGENT),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert!(
        accepted["failed"].as_array().unwrap().is_empty(),
        "{accepted}"
    );

    let (status, _, after) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    let structure = after["structures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "unplaced")
        .expect("structure");
    let drill = structure["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "Drill")
        .expect("drill");
    assert!(drill["parent"].is_null());
    let voltage = after["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "voltage")
        .expect("voltage");
    assert_eq!(voltage["key"], "voltage");
}

#[tokio::test]
async fn an_agent_drafts_the_same_model_tree() {
    let (_db, app) = app().await;
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/operations/model.draft",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{ "text": "add attribute voltage to Drill" }"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["source"], "agent");
    let id = drafted["id"].as_str().unwrap();
    let (status, _, body) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "agent");
    assert!(body["consequence"]["lines"].as_array().unwrap().len() > 3);
}

#[tokio::test]
async fn frecency_is_stored_for_the_person() {
    let (_db, app) = app().await;
    let (status, _, touched) = call(
        &app,
        "POST",
        "/v1/frecency",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{ "id": "model.command" }"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{touched}");
    assert_eq!(touched["uses"], 1);
    let (status, _, again) = call(
        &app,
        "POST",
        "/v1/frecency",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{ "id": "model.command" }"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["uses"], 2);
    let (status, _, listed) = call(&app, "GET", "/v1/frecency", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["entries"]["model.command"]["uses"], 2);
}

#[tokio::test]
async fn personal_layout_round_trips_and_layout_commands_are_ui() {
    let (db, app) = app().await;
    let hash = braian_api::hash_password("secret-layout").unwrap();
    let user = db
        .store()
        .create_user("layout@braian.local", &hash, "admin", "Layout")
        .await
        .unwrap();
    let id = uuid::Uuid::now_v7();
    db.store()
        .save_workspace(
            id,
            user.id,
            "Layout bench",
            false,
            None,
            &json!({ "type": "tabs", "views": ["v1"], "active": 0 }),
        )
        .await
        .unwrap();
    let (status, headers, logged) = call(
        &app,
        "POST",
        "/v1/login",
        None,
        &[("content-type", "application/json")],
        Some(r#"{"email":"layout@braian.local","password":"secret-layout"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    let cookie = headers
        .get("set-cookie")
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_owned();
    let (status, _, empty) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{id}/state?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert!(empty["document"].is_null());
    let document = json!({
        "id": id,
        "title": "Layout bench",
        "layout": { "type": "tabs", "views": ["v1"], "active": 0 },
        "focus": "v1"
    });
    let (status, _, saved) = call(
        &app,
        "PUT",
        &format!("/v1/workspaces/{id}/state"),
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(
            &serde_json::to_string(&json!({ "environment": "default", "document": document }))
                .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, _, loaded) = call(
        &app,
        "GET",
        &format!("/v1/workspaces/{id}/state?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{loaded}");
    assert_eq!(loaded["document"]["focus"], "v1");
    let (status, _, snapshot) = call(
        &app,
        "POST",
        "/v1/operations/get_layout",
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(&format!(
            r#"{{"workspace":"{id}","environment":"default"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["focus"], "v1");
    assert_eq!(snapshot["views"][0], "v1");
    let (status, _, opened) = call(
        &app,
        "POST",
        "/v1/operations/open_view",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"view":"Detail","zone":"right"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(opened["ui"], true);
    assert_eq!(opened["name"], "open_view");
    let (status, _, _) = call(
        &app,
        "DELETE",
        &format!("/v1/workspaces/{id}/state?environment=default"),
        None,
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn batch_get_defaults_to_heads_and_can_project_fields() {
    let (_db, app) = app().await;
    let mut ids = Vec::new();
    for sku in ["A", "B"] {
        let (status, _, created) = call(
            &app,
            "POST",
            "/v1/objects",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(
                r#"{{"type":"item","groups":{{"catalog":"root"}},"values":{{"sku":"{sku}","name":{{"sv":"Stol {sku}","en-GB":"Chair {sku}"}}}}}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        ids.push(created["id"].as_str().unwrap().to_owned());
    }
    let (status, _, lean) = call(
        &app,
        "POST",
        "/v1/objects:batchGet",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(r#"{{"ids":["{}","{}"]}}"#, ids[0], ids[1])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lean}");
    let objects = lean["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0]["id"], ids[0]);
    assert_eq!(objects[1]["id"], ids[1]);
    assert!(
        objects[0]["values"].as_object().unwrap().is_empty(),
        "{lean}"
    );
    assert_eq!(objects[0]["groups"]["catalog"], "root");
    assert!(objects[0]["version"].as_u64().unwrap() >= 1);

    let (status, _, projected) = call(
        &app,
        "POST",
        "/v1/objects:batchGet",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"ids":["{}"],"fields":["name"],"languages":["sv"]}}"#,
            ids[0]
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{projected}");
    let one = &projected["objects"][0];
    assert_eq!(one["values"]["name"]["sv"], "Stol A");
    assert!(one["values"].get("sku").is_none(), "{projected}");
    assert!(one["values"]["name"].get("en-GB").is_none(), "{projected}");

    let (status, _, missing) = call(
        &app,
        "POST",
        "/v1/objects:batchGet",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"ids":[],"fields":["no-such-field"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{missing}");

    let too_many = (0..201)
        .map(|_| format!("\"{}\"", ids[0]))
        .collect::<Vec<_>>()
        .join(",");
    let (status, _, limited) = call(
        &app,
        "POST",
        "/v1/objects:batchGet",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(r#"{{"ids":[{too_many}]}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{limited}");
    assert_eq!(limited["code"], "batch_too_large");
}
#[tokio::test]
async fn list_filters_before_limit_and_pages_with_a_cursor() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "key": "produkt",
                "labels": {},
                "key_attribute": "018f0000-0000-7000-8000-000000000100",
                "attributes": ["018f0000-0000-7000-8000-000000000100"]
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");

    for (type_key, sku) in [
        ("item", "aaa-1"),
        ("item", "aaa-2"),
        ("item", "aaa-3"),
        ("item", "100%_x"),
        ("item", "100abcx"),
        ("produkt", "zzz-1"),
        ("produkt", "zzz-2"),
        ("produkt", "zzz-3"),
    ] {
        let body = format!(
            r#"{{"type":"{type_key}","groups":{{"catalog":"root","channel":"flat"}},"values":{{"number":"{sku}","name":{{"sv":"Namn"}}}}}}"#
        );
        let (status, _, response) = call(
            &app,
            "POST",
            "/v1/objects",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sku} {response}");
    }

    let (status, _, first) = call(
        &app,
        "GET",
        "/v1/objects?type=produkt&limit=2",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let page = first["objects"].as_array().expect("objects");
    assert_eq!(page.len(), 2, "{first}");
    assert!(page.iter().all(|row| row["type"] == "produkt"), "{first}");
    assert!(first["next_cursor"].is_string(), "{first}");

    let produkts = pages(&app, "/v1/objects?type=produkt&limit=2").await;
    assert_eq!(produkts.len(), 3, "{produkts:?}");
    let mut keys: Vec<_> = produkts
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["zzz-1", "zzz-2", "zzz-3"]);

    let found = pages(&app, "/v1/objects?q=zzz&limit=1").await;
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .all(|row| row["key"].as_str().unwrap().contains("zzz"))
    );

    let upper = pages(&app, "/v1/objects?q=ZZZ&limit=10").await;
    assert_eq!(upper.len(), 3, "{upper:?}");

    let literal = pages(&app, "/v1/objects?q=100%25_&limit=10").await;
    assert_eq!(literal.len(), 1, "{literal:?}");
    assert_eq!(literal[0]["key"], "100%_x");

    let (status, _, missing) = call(
        &app,
        "GET",
        "/v1/objects?type=missing&limit=2",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{missing}");
    assert!(missing["objects"].as_array().unwrap().is_empty());
    assert!(missing["next_cursor"].is_null());

    let (status, _, bad) = call(
        &app,
        "GET",
        "/v1/objects?cursor=not-a-cursor",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    assert_eq!(bad["code"], "invalid_cursor");

    let (status, _, op) = call(
        &app,
        "POST",
        "/v1/operations/list_objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"produkt","limit":2,"q":"zzz"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{op}");
    assert_eq!(op["objects"].as_array().unwrap().len(), 2, "{op}");
    let cursor = op["next_cursor"].as_str().expect("cursor");
    let (status, _, next) = call(
        &app,
        "POST",
        "/v1/operations/list_objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"type":"produkt","limit":2,"q":"zzz","cursor":"{cursor}"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{next}");
    assert_eq!(next["objects"].as_array().unwrap().len(), 1, "{next}");
    assert!(next["next_cursor"].is_null(), "{next}");

    let all = pages(&app, "/v1/objects?limit=2").await;
    assert_eq!(all.len(), 8, "{all:?}");
    let ids: Vec<&str> = all.iter().map(|row| row["id"].as_str().unwrap()).collect();
    let mut ordered = ids.clone();
    ordered.sort();
    assert_eq!(ids, ordered, "pages follow id order");
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len()
    );
}

#[tokio::test]
async fn graph_marks_an_attribute_inherited_from_an_ancestor_group() {
    let (_db, app) = app().await;
    let (status, _, child) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "child", "channel": "flat" },
                "values": { "sku": "SKU-CHILD", "color": "red", "finish": "matte" }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{child}");
    let id = child["id"].as_str().unwrap();
    let (status, _, graph) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/graph"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    let color = attr(&graph, "color");
    assert_eq!(color["sources"].as_array().unwrap().len(), 1, "{color}");
    assert_eq!(color["sources"][0]["from"], "group");
    assert_eq!(color["sources"][0]["structure"], "catalog");
    assert_eq!(color["sources"][0]["group"], "root");
    assert_eq!(color["sources"][0]["class"], "root");
    assert_eq!(color["sources"][0]["inherited"], true);

    let finish = attr(&graph, "finish");
    assert_eq!(finish["sources"][0]["group"], "child");
    assert_eq!(finish["sources"][0]["class"], "child");
    assert_eq!(finish["sources"][0]["inherited"], false);

    let weight = attr(&graph, "weight");
    assert_eq!(weight["sources"][0]["from"], "type");
    assert_eq!(weight["sources"][0]["type"], "item");
    assert_eq!(weight["sources"][0]["inherited"], false);

    let (status, _, root) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "root", "channel": "flat" },
                "values": { "sku": "SKU-ROOT", "color": "blue" }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{root}");
    let id = root["id"].as_str().unwrap();
    let (status, _, graph) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}/graph"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    let color = attr(&graph, "color");
    assert_eq!(color["sources"][0]["group"], "root");
    assert_eq!(color["sources"][0]["inherited"], false);
    assert!(
        graph["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key"] != "finish"),
        "{graph}"
    );
}

fn attr<'a>(body: &'a Value, key: &str) -> &'a Value {
    body["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == key)
        .unwrap_or_else(|| panic!("missing {key} in {body}"))
}

async fn pages(app: &axum::Router, path: &str) -> Vec<Value> {
    let mut path = path.to_owned();
    let mut rows = Vec::new();
    for _ in 0..20 {
        let (status, _, body) = call(app, "GET", &path, Some(HUMAN), &[], None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let page = body["objects"].as_array().expect("objects");
        rows.extend(page.iter().cloned());
        match body["next_cursor"].as_str() {
            Some(cursor) => {
                assert!(!page.is_empty(), "{body}");
                let base = path
                    .split("&cursor=")
                    .next()
                    .unwrap()
                    .split("?cursor=")
                    .next()
                    .unwrap();
                let join = if base.contains('?') { '&' } else { '?' };
                path = format!("{base}{join}cursor={cursor}");
            }
            None => {
                assert!(body["next_cursor"].is_null(), "{body}");
                return rows;
            }
        }
    }
    panic!("cursor did not end");
}

#[tokio::test]
async fn structures_rename_move_and_delete_keep_parents_and_objects() {
    let (_db, app) = app().await;
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/structures:create",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"yard","label":"Yard"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["key"], "yard");
    assert_eq!(body["labels"]["sv"], "Yard");

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/structures:create",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"yard","label":"Again"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "duplicate_key");

    let (status, _, body) = call(
        &app,
        "PATCH",
        "/v1/structures/yard",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"label":"Garden"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["key"], "yard");
    assert_eq!(body["labels"]["sv"], "Garden");

    for (key, parent) in [("root", None), ("leaf", Some("root")), ("kept", None)] {
        let payload = match parent {
            Some(parent) => json!({"key": key, "label": key, "parent": parent}),
            None => json!({"key": key, "label": key}),
        };
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/structures/yard/groups",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&payload.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/structures/yard/groups/root/parent",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"parent":"leaf"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "cycle");

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/structures/yard/groups",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"bad key","label":"Bad"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "invalid_key");

    let (status, _, mut structure) =
        call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{structure}");
    let group_id = |key: &str| {
        structure["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| group["key"] == key)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let root_id = group_id("root");
    let leaf_id = group_id("leaf");
    let kept_id = group_id("kept");
    structure["classes"] = json!([
        {"id": "018f0000-0000-7000-8000-000000000611", "key": "root", "group": root_id, "attributes": []},
        {"id": "018f0000-0000-7000-8000-000000000612", "key": "leaf", "group": leaf_id, "parent": "018f0000-0000-7000-8000-000000000611", "attributes": []},
        {"id": "018f0000-0000-7000-8000-000000000613", "key": "kept", "group": kept_id, "parent": "018f0000-0000-7000-8000-000000000612", "attributes": []}
    ]);
    let (status, _, body) = call(
        &app,
        "PUT",
        "/v1/structures/yard",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&structure.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _, object) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "root", "channel": "flat", "yard": "leaf" },
                "values": { "sku": "YARD-1", "name": { "sv": "Kruka" }, "active": true }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    let object_id = object["id"].as_str().unwrap();

    let (status, _, body) = call(
        &app,
        "DELETE",
        "/v1/structures/yard/groups/leaf?dry_run=true",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["objects"], 1);
    assert_eq!(body["destructive"], true);
    assert!(
        body["sentence"].as_str().unwrap().contains("will be"),
        "{body}"
    );
    let (status, _, still) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert!(
        still["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["key"] == "leaf")
    );

    let (status, _, body) = call(
        &app,
        "DELETE",
        "/v1/structures/yard/groups/leaf?move_to=leaf",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "target_in_subtree");

    let (status, _, usage) = call(
        &app,
        "GET",
        "/v1/structures/yard/groups/leaf/usage",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{usage}");
    assert_eq!(usage["count"], 1);

    let (status, _, body) = call(
        &app,
        "DELETE",
        "/v1/structures/yard/groups/leaf?move_to=kept",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{object_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["groups"]["yard"], "kept");

    let (status, _, structure) =
        call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{structure}");
    let groups = structure["groups"].as_array().unwrap();
    let group_ids: Vec<&str> = groups
        .iter()
        .filter_map(|group| group["id"].as_str())
        .collect();
    assert!(groups.iter().all(|group| group["key"] != "leaf"));
    for group in groups {
        if let Some(parent) = group["parent"].as_str() {
            assert!(
                group_ids.contains(&parent),
                "dangling group parent {parent}"
            );
        }
    }
    let classes = structure["classes"].as_array().unwrap();
    let class_ids: Vec<&str> = classes
        .iter()
        .filter_map(|class| class["id"].as_str())
        .collect();
    assert!(classes.iter().all(|class| class["key"] != "leaf"));
    for class in classes {
        let group = class["group"].as_str().unwrap();
        assert!(
            group_ids.contains(&group),
            "class points at a missing group"
        );
        if let Some(parent) = class["parent"].as_str() {
            assert!(
                class_ids.contains(&parent),
                "dangling class parent {parent}"
            );
        }
    }
    let kept = classes.iter().find(|class| class["key"] == "kept").unwrap();
    assert_eq!(kept["parent"], "018f0000-0000-7000-8000-000000000611");

    let (status, _, body) = call(
        &app,
        "DELETE",
        "/v1/structures/yard?dry_run=true",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["objects"].as_i64().unwrap() >= 1, "{body}");
    let (status, _, still) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");

    let (status, _, body) = call(
        &app,
        "DELETE",
        &format!("/v1/objects/{object_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, body) = call(
        &app,
        "DELETE",
        "/v1/structures/yard",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _, _) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_proposal_needs_base_version_and_a_stale_one_does_not_overwrite() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-STALE","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();

    let (status, _, missing) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"No base","source":"someone","rows":[{{"object_id":"{id}","type_key":"item","patch":{{"values":{{"active":false}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{missing}");
    assert_eq!(missing["code"], "base_version_required");

    let (status, _, op) = call(
        &app,
        "POST",
        "/v1/operations/patch_object",
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some(&format!(r#"{{"id":"{id}","values":{{"active":false}}}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{op}");
    assert_eq!(op["code"], "base_version_required");

    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Stale","rows":[{{"object_id":"{id}","type_key":"item","base_version":1,"patch":{{"values":{{"active":false}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["source"], "agent", "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();

    let (status, _, saved) = call(
        &app,
        "PATCH",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("content-type", "application/json"), ("if-match", "1")],
        Some(r#"{"values":{"sku":"SKU-NEWER"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 2, "{saved}");

    let (status, _, conflict) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "conflict");
    assert_eq!(conflict["current_version"], 2, "{conflict}");
    assert!(
        conflict["detail"].as_str().unwrap().contains("older"),
        "{conflict}"
    );
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["version"], 2, "{object}");
    assert_eq!(object["values"]["sku"], "SKU-NEWER", "{object}");
    assert_eq!(object["values"]["active"], true, "{object}");
    let (status, _, pending) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{proposal_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pending}");
    assert_eq!(pending["status"], "open", "{pending}");
    assert_eq!(pending["rows"][0]["status"], "pending", "{pending}");
}

#[tokio::test]
async fn a_second_accept_does_not_apply_the_proposal_again() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-ONCE","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Once","rows":[{{"object_id":"{id}","type_key":"item","base_version":1,"patch":{{"values":{{"active":false}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/proposals/{proposal_id}/accept");
    let first = app.clone();
    let second = app.clone();
    let (left, right) = tokio::join!(
        call(
            &first,
            "POST",
            &path,
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some("{}"),
        ),
        call(
            &second,
            "POST",
            &path,
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some("{}"),
        ),
    );
    let ok = [left.0, right.0]
        .into_iter()
        .filter(|status| *status == StatusCode::OK)
        .count();
    let conflict = [left.0, right.0]
        .into_iter()
        .filter(|status| *status == StatusCode::CONFLICT)
        .count();
    assert_eq!(ok, 1, "{left:?} {right:?}");
    assert_eq!(conflict, 1, "{left:?} {right:?}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["version"], 2, "{object}");
    assert_eq!(object["values"]["active"], false, "{object}");
    let (status, _, again) = call(
        &app,
        "POST",
        &path,
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "conflict");
    assert!(
        again["detail"]
            .as_str()
            .unwrap()
            .contains("no longer pending"),
        "{again}"
    );

    let (status, _, create) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(
            r#"{"title":"Create once","source":"import","rows":[{"type_key":"item","patch":{"values":{"sku":"SKU-CREATE-ONCE","active":true}}}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{create}");
    assert_eq!(create["source"], "agent", "{create}");
    let create_id = create["id"].as_str().unwrap();
    let create_path = format!("/v1/proposals/{create_id}/accept");
    let (status, _, accepted) = call(
        &app,
        "POST",
        &create_path,
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let (status, _, duplicate) = call(
        &app,
        "POST",
        &create_path,
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{duplicate}");
    let (status, _, listed) = call(
        &app,
        "GET",
        "/v1/objects?q=SKU-CREATE-ONCE",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["objects"].as_array().unwrap().len(), 1, "{listed}");
}

#[tokio::test]
async fn a_large_accept_applies_each_patched_row_once() {
    let (_db, app) = app().await;
    let mut rows = Vec::new();
    for index in 0..8 {
        let sku = format!("SKU-BG-{index}");
        let (status, _, created) = call(
            &app,
            "POST",
            "/v1/objects",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(
                r#"{{"type":"item","values":{{"sku":"{sku}","active":true}}}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let id = created["id"].as_str().unwrap();
        rows.push(format!(
            r#"{{"object_id":"{id}","object_key":"{sku}","type_key":"item","base_version":1,"patch":{{"values":{{"sku":"{sku}-NEW","active":false}}}}}}"#
        ));
    }
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Background","source":"agent","rows":[{}]}}"#,
            rows.join(",")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["source"], "user", "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    let mut patches = serde_json::Map::new();
    for row in drafted["rows"].as_array().unwrap() {
        patches.insert(
            row["id"].as_str().unwrap().to_owned(),
            json!({ "values": { "active": false } }),
        );
    }
    let body = serde_json::to_string(&json!({ "patches": patches })).unwrap();
    let path = format!("/v1/proposals/{proposal_id}/accept");
    let (status, _, accepted) = call(
        &app,
        "POST",
        &path,
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let (status, _, during) = call(
        &app,
        "POST",
        &path,
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{during}");
    assert_eq!(during["code"], "conflict");
    let job_id = accepted["id"].as_str().unwrap();
    let mut finished = Value::Null;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (status, _, job) = call(
            &app,
            "GET",
            &format!("/v1/jobs/{job_id}"),
            Some(HUMAN),
            &[],
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
    for index in 0..8 {
        let sku = format!("SKU-BG-{index}");
        let (status, _, object) = call(
            &app,
            "GET",
            &format!("/v1/objects?q={sku}"),
            Some(HUMAN),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{object}");
        let row = object["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == sku)
            .unwrap_or_else(|| panic!("{sku} missing in {object}"));
        let id = row["id"].as_str().unwrap();
        let (status, _, object) = call(
            &app,
            "GET",
            &format!("/v1/objects/{id}"),
            Some(HUMAN),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{object}");
        assert_eq!(object["version"], 2, "{sku} {object}");
        assert_eq!(object["values"]["active"], false, "{sku} {object}");
        assert_eq!(object["values"]["sku"], sku, "{sku} {object}");
    }
}

#[tokio::test]
async fn an_accept_patch_cannot_introduce_a_field_or_value() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-PATCH","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Fields","source":"user","rows":[{{"object_id":"{id}","type_key":"item","base_version":1,"patch":{{"values":{{"active":false,"sku":"SKU-OTHER","name":{{"sv":"Hej","en-GB":"Hello"}},"tags":["aa","bb"]}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["source"], "agent", "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    let row_id = drafted["rows"][0]["id"].as_str().unwrap();
    for patch in [
        r#"{"values":{"active":true}}"#,
        r#"{"values":{"color":"red"}}"#,
        r#"{"values":{"name":{"sv":"Hej","de":"Hi"}}}"#,
        r#"{"values":{"tags":["aa","zz"]}}"#,
    ] {
        let (status, _, rejected) = call(
            &app,
            "POST",
            &format!("/v1/proposals/{proposal_id}/accept"),
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&format!(r#"{{"patches":{{"{row_id}":{patch}}}}}"#)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{patch} {rejected}"
        );
        assert_eq!(rejected["code"], "patch_not_proposed", "{patch} {rejected}");
    }
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["version"], 1, "{object}");
    assert_eq!(object["values"]["sku"], "SKU-PATCH", "{object}");
    let (status, _, kept) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"patches":{{"{row_id}":{{"values":{{"active":false,"name":{{"sv":"Hej"}},"tags":["aa"]}}}}}}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert!(kept["failed"].as_array().unwrap().is_empty(), "{kept}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["values"]["active"], false, "{object}");
    assert_eq!(object["values"]["sku"], "SKU-PATCH", "{object}");
    assert_eq!(object["values"]["name"]["sv"], "Hej", "{object}");
    assert!(object["values"]["name"].get("en-GB").is_none(), "{object}");
    assert_eq!(object["values"]["tags"], json!(["aa"]), "{object}");
}

#[tokio::test]
async fn proposal_status_errors_reach_the_api() {
    let (db, app) = app().await;
    let missing = uuid::Uuid::now_v7();
    let (status, _, body) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{missing}/discard"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "not_found");

    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"title":"Close me","source":"agent","rows":[{"type_key":"item","patch":{"values":{"sku":"SKU-STATUS","active":true}}}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["source"], "user", "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let (status, _, transition) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/discard"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{transition}");
    assert_eq!(transition["code"], "invalid_transition", "{transition}");
    let (status, _, stored) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{proposal_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["status"], "accepted", "{stored}");

    let (status, _, open) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(
            r#"{"title":"Held","source":"user","rows":[{"type_key":"item","patch":{"values":{"sku":"SKU-HELD","active":true}}}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{open}");
    assert_eq!(open["source"], "agent", "{open}");
    let held_id = uuid::Uuid::parse_str(open["id"].as_str().unwrap()).unwrap();
    let lock = db.store().claim_proposal(held_id).await.unwrap();
    let (status, _, busy) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{held_id}/discard"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{busy}");
    assert_eq!(busy["code"], "conflict", "{busy}");
    let (status, _, during) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{held_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{during}");
    assert_eq!(during["code"], "conflict", "{during}");
    let (status, _, unchanged) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{held_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["status"], "open", "{unchanged}");
    assert_eq!(unchanged["rows"][0]["status"], "pending", "{unchanged}");
    lock.release().await.unwrap();
    let (status, _, _) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{held_id}/discard"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_agent_cannot_discard_a_proposal_and_a_person_can() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-DISCARD","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, blocked) = call(
        &app,
        "DELETE",
        &format!("/v1/objects/{id}"),
        Some(AGENT),
        &[("if-match", "1")],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{blocked}");
    assert_eq!(blocked["code"], "agent_cannot_delete");
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Discard me","rows":[{{"object_id":"{id}","type_key":"item","base_version":1,"patch":{{"values":{{"active":false}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    for token in [AGENT, IMPORT] {
        let (status, _, denied) = call(
            &app,
            "POST",
            &format!("/v1/proposals/{proposal_id}/discard"),
            Some(token),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {denied}");
        assert_eq!(denied["code"], "agent_cannot_discard", "{denied}");
        let (status, _, accept) = call(
            &app,
            "POST",
            &format!("/v1/proposals/{proposal_id}/accept"),
            Some(token),
            &[("content-type", "application/json")],
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {accept}");
        assert_eq!(accept["code"], "agent_cannot_accept", "{accept}");
    }
    let (status, _, pending) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{proposal_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pending}");
    assert_eq!(pending["status"], "open", "{pending}");
    assert_eq!(pending["rows"][0]["status"], "pending", "{pending}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["version"], 1, "{object}");
    assert_eq!(object["values"]["active"], true, "{object}");
    let (status, _, _) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/discard"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, discarded) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{proposal_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{discarded}");
    assert_eq!(discarded["status"], "discarded", "{discarded}");
    assert_eq!(discarded["rows"][0]["status"], "rejected", "{discarded}");
}

#[tokio::test]
async fn a_failed_apply_leaves_the_row_pending_and_writes_nothing() {
    let (_db, app) = app().await;
    let (status, _, taken) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-TAKEN","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{taken}");
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-KEEP","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/proposals",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"title":"Duplicate sku","rows":[{{"object_id":"{id}","type_key":"item","base_version":1,"patch":{{"values":{{"sku":"SKU-TAKEN"}}}}}}]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drafted}");
    let proposal_id = drafted["id"].as_str().unwrap();
    let (status, _, failed) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{failed}");
    assert!(
        failed["accepted"].as_array().unwrap().is_empty(),
        "{failed}"
    );
    assert_eq!(failed["failed"][0]["code"], "duplicate_key", "{failed}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["version"], 1, "{object}");
    assert_eq!(object["values"]["sku"], "SKU-KEEP", "{object}");
    assert_eq!(object["values"]["active"], true, "{object}");
    let (status, _, pending) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{proposal_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pending}");
    assert_eq!(pending["status"], "open", "{pending}");
    assert_eq!(pending["rows"][0]["status"], "pending", "{pending}");
}

#[tokio::test]
async fn a_selection_cannot_be_used_with_another_type() {
    let (db, app) = app().await;
    let hash = braian_api::hash_password("secret-selection").unwrap();
    db.store()
        .create_user("selection@braian.local", &hash, "admin", "Selection")
        .await
        .unwrap();
    let (status, headers, logged) = call(
        &app,
        "POST",
        "/v1/login",
        None,
        &[("content-type", "application/json")],
        Some(r#"{"email":"selection@braian.local","password":"secret-selection"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    let cookie = headers
        .get("set-cookie")
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_owned();
    let (status, _, widget) = call(
        &app,
        "POST",
        "/v1/types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "key": "widget",
                "labels": {},
                "key_attribute": "018f0000-0000-7000-8000-000000000100",
                "attributes": ["018f0000-0000-7000-8000-000000000100"]
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{widget}");

    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/assortments",
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(r#"{"name":"Cheese","type":"item","query":{"group":"catalog:root"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["type"], "item", "{saved}");
    assert!(saved["query"].get("type").is_none(), "{saved}");
    let id = saved["id"].as_str().unwrap();

    let (status, _, wrong) = call(
        &app,
        "GET",
        &format!("/v1/assortments/{id}?type=widget"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{wrong}");
    assert_eq!(wrong["code"], "wrong_type", "{wrong}");

    let (status, _, ok) = call(
        &app,
        "GET",
        &format!("/v1/assortments/{id}?type=item"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ok}");
    assert_eq!(ok["name"], "Cheese", "{ok}");

    let (status, _, listed) = call(
        &app,
        "GET",
        "/v1/assortments?type=item",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["assortments"].as_array().unwrap().len(),
        1,
        "{listed}"
    );

    let item_id = "018f0000-0000-7000-8000-000000000200";
    let (status, _, by_id) = call(
        &app,
        "GET",
        &format!("/v1/assortments?type={item_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_id}");
    assert_eq!(by_id["assortments"][0]["id"], id, "{by_id}");

    let (status, _, widgets) = call(
        &app,
        "GET",
        "/v1/assortments?type=widget",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{widgets}");
    assert_eq!(
        widgets["assortments"].as_array().unwrap().len(),
        0,
        "{widgets}"
    );

    let (status, _, conflict) = call(
        &app,
        "POST",
        "/v1/assortments",
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(r#"{"name":"Mixed","type":"item","query":{"type":"widget"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{conflict}");
    assert_eq!(conflict["code"], "wrong_type", "{conflict}");

    let (status, _, moved) = call(
        &app,
        "POST",
        "/v1/assortments",
        None,
        &[("cookie", &cookie), ("content-type", "application/json")],
        Some(&format!(
            r#"{{"id":"{id}","name":"Cheese","type":"widget","query":{{}}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{moved}");
    assert_eq!(moved["code"], "wrong_type", "{moved}");

    let (status, _, unknown) = call(
        &app,
        "GET",
        "/v1/assortments?type=nope",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{unknown}");
    assert_eq!(unknown["code"], "unknown_reference", "{unknown}");
}

#[tokio::test]
async fn id_and_key_resolve_to_the_same_attribute_and_a_rename_applies() {
    let (_db, app) = app().await;
    let (status, _, by_key) = call(
        &app,
        "GET",
        "/v1/resolve/attribute/sku",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_key}");
    let id = by_key["id"].as_str().unwrap();
    let (status, _, by_id) = call(
        &app,
        "GET",
        &format!("/v1/resolve/attribute/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_id}");
    assert_eq!(by_key, by_id);

    let (status, _, dry) = call(
        &app,
        "POST",
        "/v1/model/keys?dry_run=true",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"kind":"attribute","ref":"sku","key":"article_number"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["applied"], false, "{dry}");
    assert_eq!(dry["id"], id);
    assert_eq!(dry["key"], "article_number", "{dry}");
    assert_eq!(
        dry["warnings"][0],
        "External integrations that use the old key « sku » will stop working"
    );
    let (status, _, unchanged) = call(
        &app,
        "GET",
        "/v1/resolve/attribute/sku",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["key"], "sku", "{unchanged}");

    let (status, _, renamed) = call(
        &app,
        "POST",
        "/v1/model/keys",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"kind":"attribute","ref":"sku","key":"article_number"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["applied"], true, "{renamed}");
    assert_eq!(renamed["id"], id);
    assert_eq!(renamed["key"], "article_number");
    let (status, _, gone) = call(
        &app,
        "GET",
        "/v1/resolve/attribute/sku",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{gone}");
    let (status, _, found) = call(
        &app,
        "GET",
        "/v1/attributes/article_number",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["id"], id);
    assert_eq!(found["key"], "article_number");
}

#[tokio::test]
async fn an_agent_rename_is_a_proposal_and_delete_is_refused() {
    let (_db, app) = app().await;
    let (status, _, proposed) = call(
        &app,
        "POST",
        "/v1/operations/model.rename_attribute_key",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"key":"sku","new_key":"article_number"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["needs_approval"], true, "{proposed}");
    assert_eq!(
        proposed["consequence"]["warnings"][0],
        "External integrations that use the old key « sku » will stop working"
    );
    let (status, _, deleted) = call(
        &app,
        "POST",
        "/v1/operations/model.command",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"text":"delete group flat"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{deleted}");
}

#[tokio::test]
async fn deleting_a_type_counts_then_removes_objects_links_and_selections() {
    let (db, app) = app().await;
    let (status, _, first) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-DEL-1","active":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, _, second) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-DEL-2"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let (status, _, link_type) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"related","label":"Related","from_types":["item"],"to_types":["item"],"from_key":"related_by","from_label":"Related by","to_key":"related","to_label":"Related","from_cardinality":"many","to_cardinality":"many"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{link_type}");
    let (status, _, linked) = call(
        &app,
        "POST",
        "/v1/links",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"type":"related","from_id":"{first_id}","to_id":"{second_id}"}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let user = db
        .store()
        .create_user("del@example.com", "x", "editor", "Del")
        .await
        .unwrap();
    db.store()
        .save_assortment(
            uuid::Uuid::now_v7(),
            user.id,
            uuid::Uuid::parse_str("018f0000-0000-7000-8000-000000000200").unwrap(),
            "Spring",
            &json!({ "q": "sku" }),
        )
        .await
        .unwrap();

    let (status, _, dry) = call(
        &app,
        "DELETE",
        "/v1/types/item?dry_run=true",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["objects"], 3, "{dry}");
    assert_eq!(dry["values"], 3, "{dry}");
    assert_eq!(dry["links"], 1, "{dry}");
    assert_eq!(dry["selections"], 1, "{dry}");
    assert_eq!(dry["destructive"], true);
    assert!(
        dry["sentence"]
            .as_str()
            .unwrap()
            .contains("will be deleted"),
        "{dry}"
    );
    let (status, _, still) = call(&app, "GET", "/v1/types/item", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    let (status, _, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{first_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");

    for token in [AGENT, IMPORT] {
        let (status, _, body) = call(
            &app,
            "DELETE",
            "/v1/types/item?dry_run=true",
            Some(token),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "agent_cannot_delete");
        let (status, _, body) =
            call(&app, "DELETE", "/v1/types/item", Some(token), &[], None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/operations/model.delete_type",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"type":"item"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, _, body) = call(
            &app,
            "DELETE",
            &format!("/v1/objects/{first_id}"),
            Some(token),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/operations/delete_object",
            Some(token),
            &[("content-type", "application/json")],
            Some(&format!(r#"{{"id":"{first_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/operations/remove_link",
            Some(token),
            &[("content-type", "application/json")],
            Some(r#"{"id":"018f0000-0000-7000-8000-000000000099"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }

    let (status, _, body) = call(&app, "DELETE", "/v1/types/item", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _, _) = call(&app, "GET", "/v1/types/item", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, gone) = call(
        &app,
        "GET",
        &format!("/v1/objects/{first_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{gone}");
    let left = db.store().list_assortments(None).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
}

#[tokio::test]
async fn a_person_saves_a_model_draft_and_an_agent_gets_a_proposal() {
    let (db, app) = app().await;
    let principal = db
        .store()
        .principal_by_handle("ada")
        .await
        .unwrap()
        .expect("ada");
    db.store()
        .attach_user(principal, "ada@example.com", "x", "admin", "Ada")
        .await
        .unwrap();

    let (status, _, empty) = call(&app, "GET", "/v1/model/draft", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["count"], 0);

    let (status, _, draft) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[
                {"op":"create_value_list","key":"difficulty","label":"Difficulty","options":[{"code":"easy","label":"Easy"}]},
                {"op":"create_type","key":"recipe","label":"Recipe","plural":"Recipes"},
                {"op":"create_attribute","key":"servings","label":"Servings","kind":"number","unit":"kg","min":"1","max":"12"},
                {"op":"attach_attribute","attribute":"servings","type_key":"recipe"}
            ]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["count"], 4, "{draft}");
    assert_eq!(draft["conflict"], false);

    let (status, _, removed) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"remove":0}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(removed["count"], 3);

    let (status, _, added) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[{"op":"create_value_list","key":"difficulty","label":"Difficulty","options":[{"code":"easy","label":"Easy"}]}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added["count"], 4);

    let (status, _, spice) = call(
        &app,
        "POST",
        "/v1/value-lists",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"spice","label":"Spice","options":[{"code":"salt","label":"Salt"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{spice}");
    let (status, _, conflicted) =
        call(&app, "GET", "/v1/model/draft", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{conflicted}");
    assert_eq!(conflicted["conflict"], true, "{conflicted}");
    let (status, _, blocked) = call(
        &app,
        "POST",
        "/v1/model/draft/save",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    let (status, _, rebased) = call(
        &app,
        "POST",
        "/v1/model/draft/rebase",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rebased}");
    assert_eq!(rebased["conflict"], false, "{rebased}");

    let (status, _, dry) = call(
        &app,
        "POST",
        "/v1/model/draft/save?dry_run=true",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["applied"], false);

    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/model/draft/save",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["applied"], true);
    assert_eq!(saved["source"], "user", "{saved}");
    assert_eq!(saved["proposal"], false);
    let revision = saved["model_revision"].as_u64().unwrap();
    let source = db
        .store()
        .definition_batch_source(revision)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source, "user");

    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    let types = defs["types"].as_array().unwrap();
    assert!(types.iter().any(|item| item["key"] == "recipe"), "{defs}");
    let attributes = defs["attributes"].as_array().unwrap();
    let servings = attributes
        .iter()
        .find(|item| item["key"] == "servings")
        .expect("servings");
    assert_eq!(servings["kind"]["min"], "1");
    assert_eq!(servings["kind"]["max"], "12");

    let (status, _, proposals) = call(&app, "GET", "/v1/proposals", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{proposals}");
    let before = proposals["proposals"]
        .as_array()
        .map(|rows| rows.len())
        .unwrap_or(0);

    let (status, _, proposed) = call(
        &app,
        "POST",
        "/v1/operations/model.create_attribute",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"key":"note","label":"Note","kind":"text"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["source"], "agent", "{proposed}");
    assert_eq!(proposed["needs_approval"], true);
    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    assert!(
        defs["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key"] != "note"),
        "{defs}"
    );
    let (status, _, proposals) = call(&app, "GET", "/v1/proposals", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{proposals}");
    let after = proposals["proposals"]
        .as_array()
        .map(|rows| rows.len())
        .unwrap_or(0);
    assert!(after > before, "{proposals}");

    let (status, _, denied) =
        call(&app, "DELETE", "/v1/types/recipe", Some(AGENT), &[], None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    let (status, _, denied) = call(
        &app,
        "POST",
        "/v1/operations/model.delete_type",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"type":"recipe"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (status, _, cleared) =
        call(&app, "DELETE", "/v1/model/draft", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{cleared}");
}

#[tokio::test]
async fn an_agent_dry_run_of_definitions_writes_nothing_and_a_save_is_forbidden() {
    let (_db, app) = app().await;
    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let revision = before["model_revision"].clone();
    let mut posted = before.clone();
    let label = posted["attributes"][0]["labels"]["sv"]
        .as_str()
        .unwrap_or("Artikelnummer");
    posted["attributes"][0]["labels"]["sv"] = json!(format!("{label} changed"));
    let payload = posted.to_string();

    let (status, _, dry) = call(
        &app,
        "PUT",
        "/v1/definitions?dry_run=true",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["applied"], false, "{dry}");

    let (status, _, after_dry) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after_dry}");
    assert_eq!(after_dry["model_revision"], revision, "{after_dry}");
    assert_eq!(after_dry, before, "{after_dry}");

    let (status, _, denied) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&payload),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (status, _, after_deny) =
        call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after_deny}");
    assert_eq!(after_deny["model_revision"], revision, "{after_deny}");
    assert_eq!(after_deny, before, "{after_deny}");
}

#[tokio::test]
async fn attribute_definitions_list_usage_and_a_draft_step() {
    let (db, app) = app().await;
    let principal = db
        .store()
        .principal_by_handle("ada")
        .await
        .unwrap()
        .expect("ada");
    db.store()
        .attach_user(principal, "ada@example.com", "x", "admin", "Ada")
        .await
        .unwrap();

    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    let (status, _, listed) = call(
        &app,
        "POST",
        "/v1/operations/model.list_attribute_definitions",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["attributes"].as_array().map(Vec::len),
        defs["attributes"].as_array().map(Vec::len),
        "{listed}"
    );

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"type":"item","groups":{"catalog":"root"},"values":{"sku":"SKU-USED","name":{"sv":"Rod"},"color":"red"}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let (status, _, usage) = call(
        &app,
        "POST",
        "/v1/operations/model.attribute_usage",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"attribute":"color"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{usage}");
    let color = uuid::Uuid::parse_str("018f0000-0000-7000-8000-000000000104").unwrap();
    let stored = db.store().count_attribute_usage(color).await.unwrap();
    assert_eq!(
        usage["used_in"]["objects"].as_i64(),
        Some(stored),
        "{usage}"
    );
    assert!(stored >= 1, "{stored}");
    let item = usage["used_in"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "item")
        .expect("type");
    assert_eq!(item["objects"], 1, "{usage}");
    let root = usage["used_in"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "root" && row["structure_key"] == "catalog")
        .expect("group");
    assert_eq!(root["objects"], 1, "{usage}");

    let (status, _, draft) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"operation":"model.attach_attribute","args":{"attribute":"secret","type":"item"}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    let steps = draft["steps"].as_array().expect("steps");
    assert!(
        steps.iter().any(|step| step["op"] == "attach_attribute"),
        "{draft}"
    );

    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let (status, _, proposed) = call(
        &app,
        "POST",
        "/v1/operations/model.attach_attribute",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"attribute":"secret","type":"item"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["needs_approval"], true, "{proposed}");
    let (status, _, after) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["model_revision"], before["model_revision"]);
    assert_eq!(
        after["types"][0]["attributes"],
        before["types"][0]["attributes"]
    );

    let (status, _, agent_list) = call(
        &app,
        "POST",
        "/v1/operations/model.list_attribute_definitions",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{agent_list}");
    assert!(!agent_list["attributes"].as_array().unwrap().is_empty());
    let (status, _, agent_usage) = call(
        &app,
        "POST",
        "/v1/operations/model.attribute_usage",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"attribute":"color"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{agent_usage}");
    assert_eq!(agent_usage["used_in"]["objects"].as_i64(), Some(stored));

    let demo = include_str!("../../../crates/server/demo/definitions.json");
    let (status, _, replaced) = call(
        &app,
        "PUT",
        "/v1/definitions",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(demo),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    let (status, _, demo_list) = call(
        &app,
        "POST",
        "/v1/operations/model.list_attribute_definitions",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{demo_list}");
    assert_eq!(
        demo_list["attributes"].as_array().map(Vec::len),
        Some(52),
        "the demo has 52 attributes after the standard attributes"
    );
}

#[tokio::test]
async fn links_round_trip_and_an_agent_cannot_delete_them() {
    let (_db, app) = app().await;
    let make = |sku: &str| format!(r#"{{"type":"item","values":{{"sku":"{sku}","active":true}}}}"#);
    let mut ids = Vec::new();
    for sku in ["LINK-A", "LINK-B", "LINK-C"] {
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/objects",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&make(sku)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"pairs_with","label":"Pairs with","from_types":["item"],"to_types":["item"],"from_key":"pairs_with","from_label":"Pairs with","to_key":"paired_with","to_label":"Pairs with","from_cardinality":"one","to_cardinality":"many","attributes":["note"]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{created}");

    let (status, _, denied) = call(
        &app,
        "DELETE",
        "/v1/link-types/pairs_with?dry_run=true",
        Some(AGENT),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let proposal = format!(
        r#"{{"type":"pairs_with","from_id":"{}","to_id":"{}","values":{{"note":"aged"}}}}"#,
        ids[0], ids[1]
    );
    let (status, _, suggested) = call(
        &app,
        "POST",
        "/v1/operations/create_link",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&proposal),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{suggested}");
    assert_eq!(suggested["needs_approval"], true, "{suggested}");
    let proposal_id = suggested["proposal_id"].as_str().unwrap();
    let (status, _, accepted) = call(
        &app,
        "POST",
        &format!("/v1/proposals/{proposal_id}/accept"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert!(
        accepted["failed"].as_array().unwrap().is_empty(),
        "{accepted}"
    );

    let (status, _, left) = call(
        &app,
        "GET",
        &format!("/v1/objects/{}", ids[0]),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{left}");
    assert_eq!(left["links"][0]["direction"], "out", "{left}");
    assert_eq!(left["links"][0]["links"][0]["peer_id"], ids[1], "{left}");
    let (status, _, right) = call(
        &app,
        "GET",
        &format!("/v1/objects/{}", ids[1]),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{right}");
    assert_eq!(right["links"][0]["direction"], "in", "{right}");
    assert_eq!(right["links"][0]["links"][0]["peer_id"], ids[0], "{right}");

    let again = format!(
        r#"{{"type":"pairs_with","from_id":"{}","to_id":"{}"}}"#,
        ids[0], ids[2]
    );
    let (status, _, blocked) = call(
        &app,
        "POST",
        "/v1/links",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&again),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{blocked}");

    let (status, _, dry) = call(
        &app,
        "DELETE",
        &format!("/v1/objects/{}?dry_run=true", ids[0]),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert!(dry["links"].as_i64().unwrap() >= 1, "{dry}");
    let (status, _, agent_dry) = call(
        &app,
        "DELETE",
        &format!("/v1/objects/{}?dry_run=true", ids[0]),
        Some(AGENT),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{agent_dry}");

    let link_id = left["links"][0]["links"][0]["id"].as_str().unwrap();
    let (status, _, agent_link) = call(
        &app,
        "DELETE",
        &format!("/v1/links/{link_id}?dry_run=true"),
        Some(AGENT),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{agent_link}");
    let (status, _, _) = call(
        &app,
        "DELETE",
        &format!("/v1/links/{link_id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, after) = call(
        &app,
        "GET",
        &format!("/v1/objects/{}", ids[1]),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert!(after.get("links").is_none() || after["links"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rest_upserts_match_the_draft_and_land_in_history() {
    let (db, app) = app().await;
    let principal = db
        .store()
        .principal_by_handle("ada")
        .await
        .unwrap()
        .expect("ada");
    db.store()
        .attach_user(principal, "ada@example.com", "x", "admin", "Ada")
        .await
        .unwrap();

    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let revision = before["model_revision"].clone();

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"steps":[{"op":"update_attribute","key":"name","kind":"number"}]}"#),
    )
    .await;
    let (status, _, attribute) =
        call(&app, "GET", "/v1/attributes/name", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{attribute}");
    let mut attribute = attribute;
    attribute["kind"] = json!({"type": "number"});
    let (rest_status, _, rest_body) = call(
        &app,
        "PUT",
        "/v1/attributes/name",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&attribute.to_string()),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(
        rest_body["code"], draft_body["code"],
        "{draft_body} {rest_body}"
    );
    assert_eq!(
        rest_body["detail"], draft_body["detail"],
        "{draft_body} {rest_body}"
    );
    assert_eq!(rest_body["code"], "model");

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[{"op":"create_type","key":"018f0000-0000-7000-8000-000000000999","label":"Nope"}]}"#,
        ),
    )
    .await;
    let (rest_status, _, rest_body) = call(
        &app,
        "POST",
        "/v1/types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"018f0000-0000-7000-8000-000000000999","labels":{},"key_attribute":"018f0000-0000-7000-8000-000000000100","attributes":[]}"#,
        ),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(rest_body["code"], draft_body["code"]);
    assert_eq!(rest_body["detail"], draft_body["detail"]);

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"steps":[{"op":"create_structure","key":"bad key","label":"Bad"}]}"#),
    )
    .await;
    let (rest_status, _, rest_body) = call(
        &app,
        "POST",
        "/v1/structures",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"bad key","groups":[],"classes":[]}"#),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(rest_body["code"], draft_body["code"]);
    assert_eq!(rest_body["detail"], draft_body["detail"]);
    assert_eq!(rest_body["code"], "invalid_key");

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[{"op":"create_unit_family","key":"portion","base_unit":"","units":[]}]}"#,
        ),
    )
    .await;
    let (rest_status, _, rest_body) = call(
        &app,
        "POST",
        "/v1/unit-families",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"portion","base_unit":"","units":[]}"#),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(rest_body["code"], draft_body["code"]);
    assert_eq!(rest_body["detail"], draft_body["detail"]);

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[{"op":"create_value_list","key":"spice2","label":"Spice","options":[{"code":"","label":"Salt"}]}]}"#,
        ),
    )
    .await;
    let (rest_status, _, rest_body) = call(
        &app,
        "POST",
        "/v1/value-lists",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"spice2","label":"Spice","options":[{"code":"","label":"Salt"}]}"#),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(rest_body["code"], draft_body["code"]);
    assert_eq!(rest_body["detail"], draft_body["detail"]);

    let (draft_status, _, draft_body) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"steps":[{"op":"create_link_type","key":"bundled","label":"Bundled","from_types":["item"],"to_types":["item"],"from_key":"same","from_label":"Same","to_key":"same","to_label":"Same"}]}"#,
        ),
    )
    .await;
    let (rest_status, _, rest_body) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"bundled","label":"Bundled","from_types":["item"],"to_types":["item"],"from_key":"same","from_label":"Same","to_key":"same","to_label":"Same"}"#,
        ),
    )
    .await;
    assert_eq!(rest_status, draft_status, "{draft_body} {rest_body}");
    assert_eq!(rest_body["code"], draft_body["code"]);
    assert_eq!(rest_body["detail"], draft_body["detail"]);

    let (status, _, unchanged) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["model_revision"], revision);

    let (status, _, tags) = call(&app, "GET", "/v1/attributes/tags", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{tags}");
    let mut gloss = tags;
    gloss.as_object_mut().unwrap().remove("id");
    gloss["key"] = json!("gloss");
    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/attributes",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&gloss.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["key"], "gloss");
    let gloss_id = saved["id"].as_str().unwrap().to_owned();

    let (status, _, saved_type) = call(
        &app,
        "POST",
        "/v1/types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"fixture","labels":{},"key_attribute":"018f0000-0000-7000-8000-000000000100","attributes":["018f0000-0000-7000-8000-000000000100"]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved_type}");
    assert_eq!(saved_type["key"], "fixture");
    let type_id = saved_type["id"].as_str().unwrap().to_owned();

    let (status, _, saved_structure) = call(
        &app,
        "POST",
        "/v1/structures",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"shelf","groups":[],"classes":[]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved_structure}");
    assert_eq!(saved_structure["key"], "shelf");
    let structure_id = saved_structure["id"].as_str().unwrap().to_owned();

    let (status, _, saved_family) = call(
        &app,
        "POST",
        "/v1/unit-families",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"count","base_unit":"pc","units":[{"code":"pc","factor":"1"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved_family}");
    assert_eq!(saved_family["key"], "count");
    let family_id = saved_family["id"].as_str().unwrap().to_owned();

    let (status, _, saved_list) = call(
        &app,
        "POST",
        "/v1/value-lists",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"shade","label":"Shade","options":[{"code":"grey","label":"Grey"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved_list}");
    assert_eq!(saved_list["key"], "shade");

    let (status, _, saved_link) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"bundled_with","label":"Bundled with","from_types":["item"],"to_types":["item"],"from_key":"bundles","from_label":"Bundles","to_key":"bundled_in","to_label":"Bundled in"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{saved_link}");

    let (status, _, history) =
        call(&app, "GET", "/v1/history?limit=20", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let changes = history["changes"].as_array().unwrap();
    let logged = |kind: &str, id: &str| {
        changes.iter().any(|row| {
            row["op"] == "definitions"
                && row["source"] == "api"
                && row["diff"]["entities"].as_array().is_some_and(|entities| {
                    entities.iter().any(|entity| {
                        entity["kind"] == kind && entity["id"] == id && entity["op"] == "create"
                    })
                })
        })
    };
    assert!(logged("attribute", &gloss_id), "{history}");
    assert!(logged("type", &type_id), "{history}");
    assert!(logged("structure", &structure_id), "{history}");
    assert!(logged("unit_family", &family_id), "{history}");
    assert!(
        changes.iter().any(|row| {
            row["op"] == "definitions"
                && row["diff"]["entities"].as_array().is_some_and(|entities| {
                    entities
                        .iter()
                        .any(|entity| entity["kind"] == "value_list" && entity["op"] == "create")
                })
        }),
        "{history}"
    );
    assert!(
        changes.iter().any(|row| {
            row["op"] == "definitions"
                && row["diff"]["entities"].as_array().is_some_and(|entities| {
                    entities
                        .iter()
                        .any(|entity| entity["kind"] == "type" && entity["op"] == "create")
                })
        }),
        "{history}"
    );

    let (status, _, denied) = call(
        &app,
        "POST",
        "/v1/attributes",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&gloss.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(denied["code"], "needs_approval");
    let (status, _, dry) = call(
        &app,
        "PUT",
        &format!("/v1/attributes/{gloss_id}?dry_run=true"),
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&saved.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["applied"], false, "{dry}");
    let (status, _, denied) = call(
        &app,
        "POST",
        "/v1/link-types",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"agent_link","label":"Agent","from_types":["item"],"to_types":["item"],"from_key":"out","from_label":"Out","to_key":"in","to_label":"In"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(denied["code"], "needs_approval");
    let (status, _, denied) = call(
        &app,
        "POST",
        "/v1/value-lists",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"key":"agent_list","label":"Agent","options":[]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(denied["code"], "needs_approval");
    let (status, _, still) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert!(
        still["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key"] != "agent_link"),
        "{still}"
    );
}

#[tokio::test]
async fn an_import_writes_a_value_list_and_is_refused_on_the_other_upserts() {
    let (_db, app) = app().await;
    let refused = [
        (
            "POST",
            "/v1/attributes",
            r#"{"key":"import_extra","labels":{"sv":"Extra"},"language_dependent":false,"multiple":false,"kind":{"type":"text","max_length":16},"index":{"searchable":false,"filterable":false,"sortable":false}}"#,
        ),
        (
            "POST",
            "/v1/types",
            r#"{"key":"import_type","labels":{},"key_attribute":"018f0000-0000-7000-8000-000000000100","attributes":["018f0000-0000-7000-8000-000000000100"]}"#,
        ),
        (
            "POST",
            "/v1/structures",
            r#"{"key":"import_shelf","groups":[],"classes":[]}"#,
        ),
        (
            "POST",
            "/v1/unit-families",
            r#"{"key":"import_count","base_unit":"pc","units":[{"code":"pc","factor":"1"}]}"#,
        ),
        (
            "POST",
            "/v1/link-types",
            r#"{"key":"import_link","label":"Import","from_types":["item"],"to_types":["item"],"from_key":"out","from_label":"Out","to_key":"in","to_label":"In"}"#,
        ),
    ];
    for (method, path, body) in refused {
        let (status, _, denied) = call(
            &app,
            method,
            path,
            Some(IMPORT),
            &[("content-type", "application/json")],
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} {denied}");
        assert_eq!(denied["code"], "needs_approval", "{path} {denied}");
    }

    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/value-lists",
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"import_shade","label":"Shade","options":[{"code":"grey","label":"Grey"}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["key"], "import_shade");

    let (status, _, updated) = call(
        &app,
        "PUT",
        "/v1/value-lists/import_shade",
        Some(IMPORT),
        &[("content-type", "application/json")],
        Some(
            r#"{"key":"import_shade","label":"Shade updated","options":[{"code":"grey","label":"Grey"}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert!(
        updated["labels"]
            .as_object()
            .unwrap()
            .values()
            .any(|label| label == "Shade updated"),
        "{updated}"
    );

    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    assert!(
        defs["value_lists"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["key"] == "import_shade"),
        "{defs}"
    );
    assert!(
        defs["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key"] != "import_extra"),
        "{defs}"
    );
    assert!(
        defs["types"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["key"] != "import_link" && item["key"] != "import_type"),
        "{defs}"
    );
}

#[tokio::test]
async fn a_structure_dry_run_refuses_a_group_that_still_holds_objects() {
    let (_db, app) = app().await;
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/structures:create",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"key":"yard","label":"Yard"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for key in ["kept", "bin"] {
        let (status, _, body) = call(
            &app,
            "POST",
            "/v1/structures/yard/groups",
            Some(HUMAN),
            &[("content-type", "application/json")],
            Some(&json!({"key": key, "label": key}).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, _, object) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            r#"{
                "type": "item",
                "groups": { "catalog": "root", "channel": "flat", "yard": "bin" },
                "values": { "sku": "YARD-1", "name": { "sv": "Kruka" }, "active": true }
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");

    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let revision = before["model_revision"].clone();
    let (status, _, mut structure) =
        call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{structure}");
    let groups = structure["groups"].as_array().unwrap().clone();
    assert!(groups.iter().any(|group| group["key"] == "bin"));
    structure["groups"] = json!(
        groups
            .into_iter()
            .filter(|group| group["key"] != "bin")
            .collect::<Vec<_>>()
    );
    let payload = structure.to_string();

    let (dry_status, _, dry_body) = call(
        &app,
        "PUT",
        "/v1/structures/yard?dry_run=true",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&payload),
    )
    .await;
    assert_eq!(dry_status, StatusCode::UNPROCESSABLE_ENTITY, "{dry_body}");
    assert_eq!(dry_body["code"], "group_occupied");

    let (status, _, after_dry) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after_dry}");
    assert_eq!(after_dry["model_revision"], revision);
    let (status, _, still) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert!(
        still["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["key"] == "bin"),
        "{still}"
    );

    let (write_status, _, write_body) = call(
        &app,
        "PUT",
        "/v1/structures/yard",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&payload),
    )
    .await;
    assert_eq!(write_status, dry_status, "{write_body}");
    assert_eq!(write_body["code"], dry_body["code"], "{write_body}");
    assert_eq!(write_body["detail"], dry_body["detail"], "{write_body}");
    let (status, _, still) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert!(
        still["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["key"] == "bin"),
        "{still}"
    );
    let (status, _, after_write) =
        call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after_write}");
    assert_eq!(after_write["model_revision"], revision);
}

fn standard_attribute_ids(defs: &Value) -> Vec<String> {
    ["number", "name", "created_at", "updated_at", "updated_by"]
        .into_iter()
        .map(|key| {
            let matches: Vec<_> = defs["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|attribute| attribute["key"] == key)
                .map(|attribute| attribute["id"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(matches.len(), 1, "{key} in {defs}");
            matches[0].clone()
        })
        .collect()
}

fn type_attribute_ids(object_type: &Value) -> Vec<String> {
    object_type["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_type_from_rest_gets_the_same_standard_attributes_as_the_draft() {
    let (db, app) = app().await;
    let principal = db
        .store()
        .principal_by_handle("ada")
        .await
        .unwrap()
        .expect("ada");
    db.store()
        .attach_user(principal, "ada@example.com", "x", "admin", "Ada")
        .await
        .unwrap();

    let (status, _, draft) = call(
        &app,
        "POST",
        "/v1/model/draft/steps",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"steps":[{"op":"create_type","key":"recipe","label":"Recipe","plural":"Recipes"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    let (status, _, saved) = call(
        &app,
        "POST",
        "/v1/model/draft/save",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["applied"], true);

    let (status, _, item) = call(&app, "GET", "/v1/types/item", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{item}");
    let key_attribute = item["key_attribute"].as_str().unwrap();
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/types",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            &json!({
                "key": "dish",
                "labels": {"sv": "Dish"},
                "key_attribute": key_attribute,
                "attributes": []
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");

    let fresh = "018f0000-0000-7000-8000-000000000777";
    let (status, _, put_created) = call(
        &app,
        "PUT",
        &format!("/v1/types/{fresh}"),
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(
            &json!({
                "key": "pantry",
                "labels": {"sv": "Pantry"},
                "key_attribute": key_attribute,
                "attributes": []
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put_created}");

    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    let standard = standard_attribute_ids(&defs);
    let recipe = defs["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "recipe")
        .expect("recipe");
    let dish = defs["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "dish")
        .expect("dish");
    let pantry = defs["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "pantry")
        .expect("pantry");
    assert_eq!(type_attribute_ids(recipe), standard);
    assert_eq!(type_attribute_ids(dish), standard);
    assert_eq!(type_attribute_ids(pantry), standard);
    assert_eq!(recipe["key_attribute"], standard[0]);
    assert_eq!(dish["key_attribute"], standard[0]);
    assert_eq!(pantry["key_attribute"], standard[0]);

    let mut update = dish.clone();
    update["labels"] = json!({"sv": "Dish updated"});
    update["attributes"] = json!([standard[0], standard[0]]);
    update["key_attribute"] = json!(standard[0]);
    let (status, _, updated) = call(
        &app,
        "PUT",
        "/v1/types/dish",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(&update.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(type_attribute_ids(&updated), standard);
    assert_eq!(updated["key_attribute"], standard[0]);
    assert_eq!(updated["labels"]["sv"], "Dish updated");
    let (status, _, defs) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{defs}");
    assert_eq!(standard_attribute_ids(&defs), standard);
    let dish = defs["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "dish")
        .expect("dish");
    assert_eq!(type_attribute_ids(dish), standard);
}

#[tokio::test]
async fn attribute_paths_filter_and_an_agent_patch_use_the_same_path() {
    let (_db, app) = app().await;
    let (status, _, listed) = call(
        &app,
        "GET",
        "/v1/types/item/attribute-paths",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let texts: Vec<_> = listed["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["text"].as_str())
        .collect();
    assert!(texts.contains(&"weight.unit"), "{texts:?}");
    assert!(texts.contains(&"weight.value"), "{texts:?}");

    let (status, _, paths) = call(
        &app,
        "POST",
        "/v1/operations/list_attribute_paths",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paths}");
    assert!(
        paths["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["text"] == "weight.unit"),
        "{paths}"
    );

    let (status, _, denied) = call(
        &app,
        "GET",
        "/v1/search?filter=missing%3Ebrand:=x",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{denied}");
    assert_eq!(denied["code"], "invalid_path");

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-PATH","weight":{"value":"1","unit":"kg"}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let (status, _, proposed) = call(
        &app,
        "POST",
        "/v1/operations/patch_object",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(&format!(
            r#"{{"id":"{id}","base_version":1,"values":{{"weight.value":"500","weight.unit":"g"}}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proposed}");
    assert_eq!(proposed["needs_approval"], true, "{proposed}");
    let (status, _, after) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["values"]["weight"]["value"], "1", "{after}");
    assert_eq!(after["values"]["weight"]["unit"], "kg", "{after}");

    let (status, _, patched) = call(
        &app,
        "PATCH",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[("content-type", "application/json"), ("if-match", "\"1\"")],
        Some(r#"{"values":{"weight.value":"500","weight.unit":"g"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    let (status, _, after) = call(
        &app,
        "GET",
        &format!("/v1/objects/{id}"),
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(
        after["values"]["weight"],
        json!({"value": "500", "unit": "g"})
    );
}
