//! Import session over Postgres: upload, map, diff, save, and the live events.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use braian_api::AppState;
use braian_store::testing::TestDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower::ServiceExt;

const TOKENS: &str = "ada:human:token-ada;bot:agent@ada:token-bot;feed:integration:token-import";
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
    token: &str,
    extra: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(match body {
            Some(body) => Body::from(body),
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

async fn harness() -> (TestDb, AppState, axum::Router, broadcast::Receiver<Value>) {
    load_env();
    let db = TestDb::create().await;
    let identities = braian_api::identities(db.store(), TOKENS).await.unwrap();
    let state = AppState::new(db.store().clone(), identities, None);
    let live = state.live.subscribe();
    let app = braian_api::into_router(state.clone());
    let defs = include_str!("../../../testdata/rules/definitions.json");
    let (status, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        HUMAN,
        &[("content-type", "application/json")],
        Some(defs.as_bytes().to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (db, state, app, live)
}

async fn until(app: &axum::Router, id: &str, want: &[&str]) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = Value::Null;
    while Instant::now() < deadline {
        let (status, body) = call(app, "GET", &format!("/v1/imports/{id}"), HUMAN, &[], None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        last = body;
        let current = last["status"].as_str().unwrap_or("");
        if want.contains(&current) {
            return last;
        }
        if current == "failed" {
            panic!("import failed: {last}");
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("timed out waiting for {want:?}: {last}");
}

fn csv() -> Vec<u8> {
    b"sku,name\nNEW-1,Chair\nOLD-1,Updated\nOLD-2,Keep\nBAD-1,WayTooLongX\n".to_vec()
}

/// The open draft of an import session, as the staging area lists it.
async fn draft_of(app: &axum::Router, session: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = Value::Null;
    while Instant::now() < deadline {
        let (status, list) = call(
            app,
            "GET",
            "/v1/proposals?status=open&source=import",
            HUMAN,
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        last = list.clone();
        if let Some(draft) = list["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["session_id"] == session)
        {
            return draft.clone();
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("no open draft for {session}: {last}");
}

async fn accept_draft(
    app: &axum::Router,
    token: &str,
    draft: &str,
    body: Value,
) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/v1/proposals/{draft}/accept"),
        token,
        &[("content-type", "application/json")],
        Some(body.to_string().into_bytes()),
    )
    .await
}

async fn object_name(app: &axum::Router, id: &str) -> Value {
    let (status, object) = call(app, "GET", &format!("/v1/objects/{id}"), HUMAN, &[], None).await;
    assert_eq!(status, StatusCode::OK, "{object}");
    object["values"]["name"]["sv"].clone()
}

async fn create_object(app: &axum::Router, sku: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/v1/objects",
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({
                "type": "item",
                "groups": { "catalog": "root", "channel": "flat" },
                "values": { "sku": sku, "name": { "sv": "Old" }, "active": true }
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn upload_maps_diffs_and_saves_with_progress_events() {
    let (_db, _state, app, mut live) = harness().await;
    let old_changed = create_object(&app, "OLD-1").await;
    let old_excluded = create_object(&app, "OLD-2").await;
    let (status, uploaded) = call(
        &app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", "items.csv"),
        ],
        Some(csv()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let id = uploaded["id"].as_str().unwrap().to_owned();
    let ready = until(&app, &id, &["ready"]).await;
    assert!(ready["parse_done"].as_bool().unwrap_or(false));
    assert!(ready["counters"].get("delete").is_none());
    assert!(
        !ready["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["id"] == "action"),
        "imports never delete, so there is no action target"
    );
    let mapping = json!({
        "type": "item",
        "id_column": 0,
        "columns": [
            { "col": 0, "target": "type:sku" },
            { "col": 1, "target": "type:name" }
        ]
    });
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/v1/imports/{id}/mapping"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(mapping.to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/imports/{id}/approve"),
        HUMAN,
        &[],
        Some(b"{}".to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let preview = until(&app, &id, &["previewed"]).await;
    assert!(preview["counters"]["new"].as_u64().unwrap_or(0) >= 1);
    assert!(preview["counters"]["changed"].as_u64().unwrap_or(0) >= 1);
    assert!(preview["counters"].get("delete").is_none());
    assert!(preview["counters"]["failed"].as_u64().unwrap_or(0) >= 1);
    // The preview is a draft in the staging area. Nothing is written yet.
    let draft = draft_of(&app, &id).await;
    let draft_id = draft["id"].as_str().unwrap().to_owned();
    assert_eq!(draft["source"], "import");
    assert_eq!(draft["counters"]["new"], preview["counters"]["new"]);
    assert_eq!(object_name(&app, &old_changed).await, "Old");
    let (status, page) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{draft_id}/rows?limit=2&offset=1"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["kind"], "import");
    assert_eq!(page["rows"].as_array().unwrap().len(), 2);
    assert!(page["total"].as_i64().unwrap() >= 4);
    assert_eq!(page["columns"].as_array().unwrap().len(), 2);
    let (status, rows) = call(
        &app,
        "GET",
        &format!("/v1/imports/{id}/rows?limit=20&status=all"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    let statuses: BTreeMap<String, String> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["object_key"].as_str().unwrap_or("").to_owned(),
                row["status"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    assert_eq!(statuses.get("NEW-1").map(String::as_str), Some("new"));
    assert_eq!(statuses.get("OLD-1").map(String::as_str), Some("changed"));
    assert_eq!(statuses.get("OLD-2").map(String::as_str), Some("changed"));
    let excluded_row = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["object_key"] == "OLD-2")
        .map(|row| row["row_no"].as_i64().unwrap())
        .unwrap();
    let (status, denied) = accept_draft(&app, HUMAN, &draft_id, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{denied}");
    assert_eq!(denied["code"], "has_failures");
    for token in [AGENT, "token-import"] {
        let (status, forbidden) =
            accept_draft(&app, token, &draft_id, json!({ "skip_failed": true })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{token} {forbidden}");
        assert_eq!(forbidden["code"], "agent_cannot_accept");
    }
    // Nothing was written by the refused attempts.
    assert_eq!(object_name(&app, &old_changed).await, "Old");
    let (status, saved) = accept_draft(
        &app,
        HUMAN,
        &draft_id,
        json!({ "skip_failed": true, "exclude_rows": [excluded_row] }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{saved}");
    let job_id = saved["job_id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut job = Value::Null;
    while Instant::now() < deadline {
        let (status, body) =
            call(&app, "GET", &format!("/v1/jobs/{job_id}"), HUMAN, &[], None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        job = body;
        if job["status"] == "succeeded" || job["status"] == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert_eq!(job["status"], "succeeded", "{job}");
    assert!(job["progress_done"].as_i64().unwrap_or(0) >= 1);
    let done = until(&app, &id, &["saved"]).await;
    assert_eq!(done["status"], "saved");
    assert_eq!(done["counters"]["excluded"], 1);
    let (status, closed) = call(
        &app,
        "GET",
        &format!("/v1/proposals/{draft_id}"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "accepted");
    let (status, created) = call(
        &app,
        "GET",
        &format!("/v1/imports/{id}/rows?status=new&limit=10"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let new_id = created["rows"][0]["object_id"].as_str().unwrap();
    let (status, object) = call(
        &app,
        "GET",
        &format!("/v1/objects/{new_id}"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{object}");
    assert_eq!(object["values"]["sku"], "NEW-1");
    assert_eq!(object["values"]["name"]["sv"], "Chair");
    assert_eq!(object_name(&app, &old_changed).await, "Updated");
    // The deselected row stays as it was, and an import never deletes.
    assert_eq!(object_name(&app, &old_excluded).await, "Old");

    let mut saw_import = false;
    let mut saw_job = false;
    let drain = Instant::now() + Duration::from_millis(500);
    while Instant::now() < drain && !(saw_import && saw_job) {
        match live.try_recv() {
            Ok(value) => match value.get("op").and_then(Value::as_str) {
                Some("import") => saw_import = true,
                Some("job") => saw_job = true,
                _ => {}
            },
            Err(broadcast::error::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {}
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
    assert!(saw_import, "missing import progress event");
    assert!(saw_job, "missing job progress event");
}

#[tokio::test]
async fn a_changed_model_blocks_the_save() {
    let (_db, _state, app, _live) = harness().await;
    let (status, uploaded) = call(
        &app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", "items.csv"),
        ],
        Some(b"sku,name\nNEW-9,Chair\n".to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let id = uploaded["id"].as_str().unwrap();
    until(&app, id, &["ready"]).await;
    let mut defs: Value =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    defs["model_revision"] = json!(2);
    let (status, body) = call(
        &app,
        "PUT",
        "/v1/definitions",
        HUMAN,
        &[("content-type", "application/json")],
        Some(defs.to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, blocked) = call(
        &app,
        "PUT",
        &format!("/v1/imports/{id}/mapping"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({
                "type": "item",
                "id_column": 0,
                "columns": [
                    { "col": 0, "target": "type:sku" },
                    { "col": 1, "target": "type:name" }
                ]
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "model_changed");
}

#[tokio::test]
async fn approve_restarts_when_the_mapping_changes() {
    let (_db, _state, app, _live) = harness().await;
    create_object(&app, "OLD-1").await;
    create_object(&app, "OLD-2").await;
    let (status, uploaded) = call(
        &app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", "items.csv"),
        ],
        Some(csv()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let id = uploaded["id"].as_str().unwrap();
    until(&app, id, &["ready"]).await;
    put_mapping(&app, id, true).await;
    let (status, approved) = approve(&app, id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{approved}");
    let preview = until(&app, id, &["previewed"]).await;
    assert!(preview["counters"]["failed"].as_u64().unwrap_or(0) >= 1);

    let (status, again) = approve(&app, id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["status"], "previewed");
    let (status, still) = call(&app, "GET", &format!("/v1/imports/{id}"), HUMAN, &[], None).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert_eq!(still["status"], "previewed");
    assert_eq!(still["counters"]["failed"], preview["counters"]["failed"]);

    put_mapping(&app, id, false).await;
    let (status, restarted) = approve(&app, id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{restarted}");
    assert_eq!(restarted["status"], "diffing");
    let next = until(&app, id, &["previewed"]).await;
    assert_eq!(next["counters"]["failed"].as_u64().unwrap_or(1), 0);
    assert!(next["counters"]["unchanged"].as_u64().unwrap_or(0) >= 2);
    let (status, rows) = call(
        &app,
        "GET",
        &format!("/v1/imports/{id}/rows?limit=20&status=all"),
        HUMAN,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    let bad = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["object_key"] == "BAD-1")
        .unwrap();
    assert_eq!(bad["status"], "new");
}

#[tokio::test]
async fn sheet_switch_waits_until_mapping_finishes() {
    let (_db, _state, app, _live) = harness().await;
    let (status, uploaded) = call(
        &app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", "book.xlsx"),
        ],
        Some(two_sheets()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let id = uploaded["id"].as_str().unwrap();
    let ready = until(&app, id, &["ready"]).await;
    assert_eq!(ready["headers"][0]["raw"], "sku");
    let (status, switched) = call(
        &app,
        "POST",
        &format!("/v1/imports/{id}/sheet"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(br#"{"sheet_index":1}"#.to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_ne!(switched["status"], "mapping");
    assert_eq!(switched["status"], "ready");
    assert_eq!(switched["sheet_index"], 1);
    assert_eq!(switched["headers"][0]["raw"], "code");
}

async fn approve(app: &axum::Router, id: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/v1/imports/{id}/approve"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(b"{}".to_vec()),
    )
    .await
}

async fn put_mapping(app: &axum::Router, id: &str, with_name: bool) {
    let mut columns = vec![json!({ "col": 0, "target": "type:sku" })];
    if with_name {
        columns.push(json!({ "col": 1, "target": "type:name" }));
    }
    let (status, body) = call(
        app,
        "PUT",
        &format!("/v1/imports/{id}/mapping"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({
                "type": "item",
                "id_column": 0,
                "columns": columns,
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

fn two_sheets() -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("braian-import-sheets-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("book.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("First").unwrap();
    first.write_string(0, 0, "sku").unwrap();
    first.write_string(1, 0, "A-1").unwrap();
    first.set_active(true);
    let second = workbook.add_worksheet();
    second.set_name("Second").unwrap();
    second.write_string(0, 0, "code").unwrap();
    second.write_string(1, 0, "B-1").unwrap();
    workbook.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

#[tokio::test]
async fn an_empty_upload_is_rejected_up_front() {
    let (_db, _state, app, _live) = harness().await;
    for (name, body) in [
        ("empty.csv", Vec::new()),
        ("header.csv", b"sku,name\n".to_vec()),
    ] {
        let (status, uploaded) = call(
            &app,
            "POST",
            "/v1/imports",
            HUMAN,
            &[
                ("content-type", "application/octet-stream"),
                ("x-file-name", name),
            ],
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name} {uploaded}");
        assert_eq!(uploaded["code"], "empty_file");
        assert_eq!(uploaded["detail"], "This file has no rows.");
    }
}

#[tokio::test]
async fn saving_again_while_a_save_is_running_is_already_saving() {
    let (_db, state, app, _live) = harness().await;
    let (status, uploaded) = call(
        &app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", "items.csv"),
        ],
        Some(csv()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let id = uploaded["id"].as_str().unwrap();
    let uuid = uuid::Uuid::parse_str(id).unwrap();
    // Mapping keeps writing status until it is ready. Set saving only after that.
    let _ = until(&app, id, &["ready"]).await;
    let draft = state
        .store
        .upsert_import_proposal(uuid, "Import items.csv", &json!({ "new": 1, "failed": 0 }))
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .update_import_job(uuid, "saving", None, &json!({}), None)
        .await
        .unwrap();
    let (status, body) = accept_draft(&app, HUMAN, &draft.to_string(), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "already_saving");
    assert_eq!(body["detail"], "A save is already running.");
}

async fn upload_csv(app: &axum::Router, name: &str, body: &str) -> String {
    let (status, uploaded) = call(
        app,
        "POST",
        "/v1/imports",
        HUMAN,
        &[
            ("content-type", "application/octet-stream"),
            ("x-file-name", name),
        ],
        Some(body.as_bytes().to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    uploaded["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn editing_a_failing_cell_rechecks_the_row_and_the_counters() {
    let (_db, _state, app, _live) = harness().await;
    let id = upload_csv(
        &app,
        "items.csv",
        "sku,name\nNEW-1,Chair\nBAD-1,WayTooLongX\n",
    )
    .await;
    until(&app, &id, &["ready"]).await;
    put_mapping(&app, &id, true).await;
    let (status, approved) = approve(&app, &id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{approved}");
    let preview = until(&app, &id, &["previewed"]).await;
    assert_eq!(preview["counters"]["failed"], 1);
    let (_, rows) = call(
        &app,
        "GET",
        &format!("/v1/imports/{id}/rows?limit=20&status=failed"),
        HUMAN,
        &[],
        None,
    )
    .await;
    let row_no = rows["rows"][0]["row_no"].as_u64().expect("row_no");
    let (status, edited) = call(
        &app,
        "PATCH",
        &format!("/v1/imports/{id}/rows/{row_no}"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({ "col": 1, "value": "Short" })
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["counters"]["failed"], 0);
    assert_eq!(edited["row"]["status"], "new");
    // The edit survives a new check of the whole file.
    let (_, again) = approve(&app, &id).await;
    assert_eq!(again["status"], "previewed");
}

#[tokio::test]
async fn a_saved_mapping_is_reused_for_the_same_headers() {
    let (_db, _state, app, _live) = harness().await;
    let first = upload_csv(&app, "a.csv", "sku,name\nNEW-1,Chair\n").await;
    until(&app, &first, &["ready"]).await;
    put_mapping(&app, &first, true).await;
    let (status, saved) = call(
        &app,
        "POST",
        "/v1/import-mappings",
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({ "session_id": first, "name": "Items" })
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert!(status.is_success(), "{saved}");
    let second = upload_csv(&app, "b.csv", "sku,name\nNEW-2,Table\n").await;
    let ready = until(&app, &second, &["ready"]).await;
    assert_eq!(ready["proposal"]["ai"], "saved: Items");
    assert!(ready["proposal"]["type"]["value"].is_string());
}

#[tokio::test]
async fn a_model_sheet_becomes_one_revision_when_approved() {
    let (_db, _state, app, _live) = harness().await;
    let id = upload_csv(
        &app,
        "attributes.csv",
        "key,label,kind,required\nmaterial,Material,text,yes\nweight_g,Weight,number,no\nBad Key,X,text,\n",
    )
    .await;
    until(&app, &id, &["ready"]).await;
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/v1/imports/{id}/mapping"),
        HUMAN,
        &[("content-type", "application/json")],
        Some(
            json!({
                "type": "model:attribute",
                "id_column": 0,
                "columns": [
                    { "col": 0, "target": "model:attribute:key" },
                    { "col": 1, "target": "model:attribute:label" },
                    { "col": 2, "target": "model:attribute:kind" },
                    { "col": 3, "target": "model:attribute:required" },
                ],
            })
            .to_string()
            .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, revision_before) = call(&app, "GET", "/v1/definitions", HUMAN, &[], None).await;
    let (status, approved) = approve(&app, &id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{approved}");
    let preview = until(&app, &id, &["previewed"]).await;
    assert_eq!(preview["counters"]["new"], 2);
    assert_eq!(preview["counters"]["failed"], 1);
    let draft = draft_of(&app, &id).await;
    let (status, accepted) = accept_draft(
        &app,
        HUMAN,
        draft["id"].as_str().unwrap(),
        json!({ "skip_failed": true }),
    )
    .await;
    assert!(status.is_success(), "{accepted}");
    let done = until(&app, &id, &["saved"]).await;
    assert_eq!(done["status"], "saved");
    let (_, after) = call(&app, "GET", "/v1/definitions", HUMAN, &[], None).await;
    let text = after.to_string();
    assert!(
        text.contains("weight_g") && text.contains("material"),
        "{text}"
    );
    let before = revision_before["model_revision"].as_i64().unwrap_or(0);
    assert_eq!(after["model_revision"].as_i64().unwrap_or(0), before + 1);
}
