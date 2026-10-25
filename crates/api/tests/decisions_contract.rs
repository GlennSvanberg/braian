//! Mock of the documented Decisions contract. This file lives outside
//! `src/import` so the acceptance search stays limited to the client.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use braian_api::import::decisions::{
    Answer, ChoiceOption, ContentPart, DECISIONS_MODEL, DECISIONS_PATH, DEVELOPER_TEXT, Decider,
    DecisionError, DecisionPolicy, DecisionRequest, DecisionResponse, HttpDecider, choice_question,
    contract_problems, decode_json, explicit_cache, openai_base, post_decision, shared_input,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Seen {
    paths: Vec<String>,
    inputs: Vec<Value>,
    saw_authorization: bool,
    problems: Vec<String>,
}

fn fixture(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/decisions")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn sample_request(model_json: Value, file_json: Value, name: &str) -> DecisionRequest {
    DecisionRequest {
        model: DECISIONS_MODEL,
        prompt_cache_key: "braian-import-0123456789abcdef".into(),
        safety_identifier: "a".repeat(32),
        prompt_cache_options: explicit_cache(),
        input: shared_input(model_json, file_json),
        questions: vec![choice_question(
            name,
            "Which field?",
            vec![
                ChoiceOption {
                    value: "type:name".into(),
                    description: None,
                },
                ChoiceOption {
                    value: "unmapped".into(),
                    description: None,
                },
            ],
        )],
    }
}

fn file_json() -> Value {
    let samples: Vec<String> = (0..20).map(|index| format!("row-{index}")).collect();
    json!({
        "file_name": "items.csv",
        "sheet": "CSV",
        "columns": [{ "id": "col:1", "header": "Name", "samples": samples }]
    })
}

fn policy() -> DecisionPolicy {
    DecisionPolicy {
        timeout: Duration::from_secs(2),
        retries: 1,
        max_retry_after: Duration::from_millis(20),
    }
}

type Script = VecDeque<(&'static str, StatusCode)>;
type MockState = (Arc<Mutex<Seen>>, Arc<Mutex<Script>>);

async fn serve(seen: Arc<Mutex<Seen>>, script: Arc<Mutex<Script>>) -> String {
    let app = Router::new()
        .route(DECISIONS_PATH, post(decide))
        .route("/v1/responses", post(reject))
        .route("/v1/chat/completions", post(reject))
        .with_state((seen, script));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

async fn decide(
    State((seen, script)): State<MockState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut seen = seen.lock().expect("seen");
    seen.paths.push(DECISIONS_PATH.to_owned());
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if authorization.starts_with("Bearer ") && authorization.len() > "Bearer ".len() {
        seen.saw_authorization = true;
    }
    if value.get("tools").is_some()
        || value.get("functions").is_some()
        || value.get("response_format").is_some()
    {
        seen.problems.push("extra".into());
    }
    if value["model"] != DECISIONS_MODEL || value["model"] != "gpt-6-luna" {
        seen.problems.push("model".into());
    }
    let questions = value["questions"].as_array().cloned().unwrap_or_default();
    if questions.is_empty() || questions.len() > 400 {
        seen.problems.push("questions".into());
    }
    for question in &questions {
        if question["type"] != "choice"
            || question.get("instructions").is_none()
            || question.get("question").is_some()
        {
            seen.problems.push("question".into());
        }
        let count = question["choices"].as_array().map(Vec::len).unwrap_or(0);
        if !(2..=255).contains(&count) {
            seen.problems.push("choices".into());
        }
    }
    let input = value["input"].as_array().cloned().unwrap_or_default();
    let developer = input
        .first()
        .and_then(|item| item["content"][0]["text"].as_str());
    if developer != Some(DEVELOPER_TEXT) {
        seen.problems.push("developer".into());
    }
    let parts = input
        .get(1)
        .and_then(|item| item["content"].as_array())
        .cloned()
        .unwrap_or_default();
    let model_at = parts.iter().position(|part| part["type"] == "input_json");
    let break_at = parts
        .iter()
        .position(|part| part["text"] == "END OF DATA MODEL.");
    let file_at = parts.iter().rposition(|part| part["type"] == "input_json");
    if model_at.is_none()
        || break_at.is_none()
        || file_at.is_none()
        || model_at >= break_at
        || break_at >= file_at
    {
        seen.problems.push("order".into());
    }
    let samples = parts
        .iter()
        .rev()
        .find(|part| part["type"] == "input_json")
        .and_then(|part| part["data"]["columns"][0]["samples"].as_array())
        .map(Vec::len)
        .unwrap_or(0);
    if samples > 20 {
        seen.problems.push("samples".into());
    }
    seen.inputs.push(value["input"].clone());
    drop(seen);
    let next = script
        .lock()
        .expect("script")
        .pop_front()
        .unwrap_or(("call2_response.json", StatusCode::OK));
    let mut response = Response::builder().status(next.1);
    if next.1 == StatusCode::TOO_MANY_REQUESTS {
        response = response.header(header::RETRY_AFTER, "0");
    }
    response
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(fixture(next.0)))
        .unwrap()
}

async fn reject(headers: HeaderMap) -> StatusCode {
    let _ = headers;
    StatusCode::NOT_FOUND
}

#[test]
fn openai_base_url_overrides_the_host_for_tests() {
    let previous = std::env::var("OPENAI_BASE_URL").ok();
    unsafe { std::env::set_var("OPENAI_BASE_URL", "http://127.0.0.1:9") };
    assert_eq!(openai_base(), "http://127.0.0.1:9");
    match previous {
        Some(value) => unsafe { std::env::set_var("OPENAI_BASE_URL", value) },
        None => unsafe { std::env::remove_var("OPENAI_BASE_URL") },
    }
}

#[tokio::test]
async fn decisions_mock_matches_the_documented_contract() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let script = Arc::new(Mutex::new(VecDeque::from([
        ("call1_response.json", StatusCode::OK),
        ("call2_response.json", StatusCode::OK),
    ])));
    let base = serve(seen.clone(), script).await;
    let model = json!({"types": ["item"]});
    let file = file_json();
    let first = sample_request(model.clone(), file.clone(), "type");
    let second = sample_request(model, file, "col:1");
    assert!(
        contract_problems(&first).is_empty(),
        "{:?}",
        contract_problems(&first)
    );
    match &first.input[1].content[2] {
        ContentPart::Text {
            text,
            prompt_cache_breakpoint,
        } => {
            assert_eq!(text, "END OF DATA MODEL.");
            assert!(prompt_cache_breakpoint.is_some());
        }
        ContentPart::Json { .. } => panic!("the cache breakpoint is a text part"),
    }
    match first.input[1].content.last() {
        Some(ContentPart::Text {
            text,
            prompt_cache_breakpoint,
        }) => {
            assert_eq!(text, "END OF FILE.");
            assert!(prompt_cache_breakpoint.is_some());
        }
        _ => panic!("the file sits inside the cached prefix"),
    }
    assert!(matches!(
        first.input[1].content.first(),
        Some(ContentPart::Text { text, .. }) if text == "DATA MODEL"
    ));
    let client = HttpDecider::new(&base, "test-key", policy());
    let left = client.decide(&first).await.unwrap();
    let right = client.decide(&second).await.unwrap();
    assert_eq!(left.model, "gpt-6-luna");
    assert_eq!(right.model, DECISIONS_MODEL);
    let seen = seen.lock().unwrap();
    assert!(seen.problems.is_empty(), "{:?}", seen.problems);
    assert!(seen.saw_authorization);
    assert_eq!(seen.paths, vec![DECISIONS_PATH, DECISIONS_PATH]);
    assert!(seen.paths.iter().all(|path| path == "/v1/decisions"));
    assert!(
        !seen
            .paths
            .iter()
            .any(|path| path.contains("responses") || path.contains("chat"))
    );
    assert_eq!(seen.inputs[0], seen.inputs[1]);
}

#[tokio::test]
async fn decisions_retries_429_then_reads_the_fixture() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let script = Arc::new(Mutex::new(VecDeque::from([
        ("429.json", StatusCode::TOO_MANY_REQUESTS),
        ("call1_response.json", StatusCode::OK),
    ])));
    let base = serve(seen.clone(), script).await;
    let request = sample_request(json!({"types": ["item"]}), file_json(), "type");
    let response = post_decision(&http(), &base, "test-key", &request, &policy())
        .await
        .unwrap();
    assert_eq!(response.model, DECISIONS_MODEL);
    assert_eq!(seen.lock().unwrap().paths.len(), 2);
}

#[tokio::test]
async fn refusal_decodes_and_a_hung_call_times_out() {
    let decoded: DecisionResponse = decode_json(&fixture("refusal.json")).unwrap();
    assert!(matches!(decoded.answers[0], Answer::Refusal { .. }));
    let unknown: DecisionResponse = decode_json(&fixture("call2_response.json")).unwrap();
    let Answer::Choice { choice, .. } = &unknown.answers[0] else {
        panic!("choice");
    };
    assert_eq!(choice.as_text(), Some("type:name"));

    let hang = Router::new().route(
        DECISIONS_PATH,
        post(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, hang).await.unwrap();
    });
    let policy = DecisionPolicy {
        timeout: Duration::from_millis(80),
        retries: 0,
        max_retry_after: Duration::from_millis(10),
    };
    let request = sample_request(json!({"types": ["item"]}), file_json(), "col:1");
    let client = reqwest::Client::builder()
        .timeout(policy.timeout)
        .build()
        .unwrap();
    let error = post_decision(
        &client,
        &format!("http://{address}"),
        "test-key",
        &request,
        &policy,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DecisionError::Timeout | DecisionError::Transport(_)
    ));
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}

#[tokio::test]
#[ignore = "set BRAIAN_LIVE_DECISIONS=1 and OPENAI_API_KEY"]
async fn import_decisions_live() {
    if std::env::var("BRAIAN_LIVE_DECISIONS").ok().as_deref() != Some("1") {
        eprintln!(
            "skipped import_decisions_live: set BRAIAN_LIVE_DECISIONS=1 and OPENAI_API_KEY to run it"
        );
        return;
    }
    let key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("BRAIAN_LIVE_DECISIONS=1 but OPENAI_API_KEY is missing"));
    let request = DecisionRequest {
        model: DECISIONS_MODEL,
        prompt_cache_key: "braian-import-0123456789abcdef".into(),
        safety_identifier: "a".repeat(32),
        prompt_cache_options: explicit_cache(),
        input: shared_input(
            json!({
                "model_revision": 1,
                "types": [{
                    "key": "item",
                    "label": "Item",
                    "attributes": [
                        {"id": "type:sku", "kind": "text", "label": "SKU"},
                        {"id": "type:name", "kind": "text", "label": "Name"}
                    ]
                }]
            }),
            json!({
                "file_name": "items.csv",
                "sheet": "CSV",
                "columns": [
                    {"id": "col:0", "header": "sku", "samples": ["A-1"]},
                    {"id": "col:1", "header": "name", "samples": ["Chair"]}
                ]
            }),
        ),
        questions: vec![choice_question(
            "col:0",
            "Which data model field should column \"sku\" be imported into?",
            vec![
                ChoiceOption {
                    value: "type:sku".into(),
                    description: Some("SKU, the key attribute".into()),
                },
                ChoiceOption {
                    value: "type:name".into(),
                    description: Some("Name".into()),
                },
                ChoiceOption {
                    value: "unmapped".into(),
                    description: Some("Do not import this column".into()),
                },
            ],
        )],
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let response = post_decision(
        &client,
        "https://api.openai.com",
        &key,
        &request,
        &DecisionPolicy {
            timeout: Duration::from_secs(20),
            retries: 1,
            max_retry_after: Duration::from_secs(2),
        },
    )
    .await
    .expect("live decisions call");
    assert_eq!(response.model, "gpt-6-luna");
    assert!(
        response
            .usage
            .as_ref()
            .is_some_and(|usage| usage.input_tokens > 0)
    );
    let choice = response
        .answers
        .iter()
        .find_map(|answer| match answer {
            Answer::Choice { choice, .. } => choice.as_text().map(str::to_owned),
            _ => None,
        })
        .expect("a choice for column sku");
    assert_eq!(
        choice, "type:sku",
        "column sku with sample A-1 must map to the sku attribute, not {choice}"
    );
}
