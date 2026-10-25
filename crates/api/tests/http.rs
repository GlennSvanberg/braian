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
const AGENT: &str = "token-bot";

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
        Some(r#"{"values":{"name":{"sv":"Två"}}}"#),
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
    assert_eq!(english["values"]["name"]["sv"], "Två");
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
            r#"{"type":"item","groups":{"catalog":"root"},"values":{"sku":"SKU-WARN","name":{"sv":"Stol"},"active":true}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();

    let (status, _, attribute) =
        call(&app, "GET", "/v1/attributes/name", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{attribute}");
    let mut attribute = attribute;
    attribute["kind"]["max_length"] = json!(3);
    let (status, _, saved) = call(
        &app,
        "PUT",
        "/v1/attributes/name",
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
        Some(r#"{"values":{"name":{"sv":"1234"}}}"#),
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
async fn agent_writes_are_attributed_and_delete_returns_gone() {
    let (_db, app) = app().await;
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/objects",
        Some(AGENT),
        &[("content-type", "application/json")],
        Some(r#"{"type":"item","values":{"sku":"SKU-AGENT"}}"#),
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
    assert_eq!(changes[0]["actor"], "bot");
    assert_eq!(changes[0]["on_behalf_of"], "ada");
    assert_eq!(changes[0]["via"], "agent");
    assert_eq!(changes[0]["op"], "create");

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
async fn a_model_command_stays_a_proposal_until_a_person_accepts() {
    let (_db, app) = app().await;
    let (status, _, drafted) = call(
        &app,
        "POST",
        "/v1/operations/model.command",
        Some(HUMAN),
        &[("content-type", "application/json")],
        Some(r#"{ "text": "set up a model for power tools" }"#),
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
    assert!(
        text.contains("Add class Drill on Drills, parent Power tool"),
        "{text}"
    );
    assert!(text.contains("Create attribute voltage"), "{text}");
    let id = drafted["proposal_id"].as_str().unwrap();

    let (status, _, before) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert!(
        !before["structures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["key"] == "power_tools")
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
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
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

    let (status, _, after) = call(&app, "GET", "/v1/definitions", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    let structure = after["structures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "power_tools")
        .expect("structure");
    let drill = structure["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "Drill")
        .expect("drill");
    assert!(drill["parent"].as_str().is_some());
    let voltage = after["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "voltage")
        .expect("voltage");
    assert_eq!(voltage["required"], true);
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
        Some(r#"{ "preset": "power_tools" }"#),
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
            r#"{{"type":"{type_key}","groups":{{"catalog":"root","channel":"flat"}},"values":{{"sku":"{sku}"}}}}"#
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
        "/v1/structures/yard/groups/leaf",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "group_occupied");
    assert_eq!(body["count"], 1);
    assert!(body["detail"].as_str().unwrap().contains('1'));

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
    assert_eq!(status, StatusCode::OK, "{body}");

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
        "/v1/structures/yard",
        Some(HUMAN),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "structure_occupied");
    assert_eq!(body["count"], 1);

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
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, _) = call(&app, "GET", "/v1/structures/yard", Some(HUMAN), &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
