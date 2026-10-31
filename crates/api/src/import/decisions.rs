//! HTTP client for the Decisions API.
//!
//! The request uses the reference field names: question text is `instructions`,
//! and an `input_json` part carries `data`. `prompt_cache_options.mode` is
//! `explicit` so the implicit breakpoint does not swallow the file that follows
//! the marked prefix.

use std::future::Future;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DECISIONS_MODEL: &str = "gpt-6-luna";
pub const DECISIONS_PATH: &str = "/v1/decisions";

pub const DEVELOPER_TEXT: &str = "You map spreadsheet files onto a product information model. Only use the provided choices. Prefer 'none' or 'unmapped' over a weak guess.";

const MAX_QUESTIONS: usize = 400;
const MIN_CHOICES: usize = 2;
const MAX_CHOICES: usize = 255;

#[derive(Debug, Clone)]
pub struct DecisionPolicy {
    pub timeout: Duration,
    pub retries: u32,
    pub max_retry_after: Duration,
}

impl Default for DecisionPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(8),
            retries: 1,
            max_retry_after: Duration::from_secs(2),
        }
    }
}

#[derive(Debug)]
pub enum DecisionError {
    Status { code: u16 },
    Timeout,
    Transport(String),
    Decode(String),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status { code } => write!(formatter, "decisions status {code}"),
            Self::Timeout => formatter.write_str("decisions timed out"),
            Self::Transport(message) | Self::Decode(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DecisionError {}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionRequest {
    pub model: &'static str,
    pub prompt_cache_key: String,
    pub safety_identifier: String,
    pub prompt_cache_options: PromptCacheOptions,
    pub input: Vec<InputMessage>,
    pub questions: Vec<ChoiceQuestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptCacheOptions {
    pub mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputMessage {
    pub role: &'static str,
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "input_text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt_cache_breakpoint: Option<Breakpoint>,
    },
    #[serde(rename = "input_json")]
    Json { data: Value },
}

#[derive(Debug, Clone, Serialize)]
pub struct Breakpoint {
    pub mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChoiceQuestion {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub name: String,
    pub instructions: String,
    pub choices: Vec<ChoiceOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChoiceOption {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DecisionResponse {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub answers: Vec<Answer>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Answer {
    Choice {
        name: Option<String>,
        choice: ChoiceValue,
        #[serde(default)]
        confidence: f64,
        #[serde(default)]
        probabilities: Vec<Probability>,
    },
    Refusal {
        name: Option<String>,
    },
    Predicate {
        name: Option<String>,
        #[serde(default)]
        probability: f64,
    },
    Score {
        name: Option<String>,
        #[serde(default)]
        confidence: f64,
    },
}

impl Answer {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Choice { name, .. }
            | Self::Refusal { name }
            | Self::Predicate { name, .. }
            | Self::Score { name, .. } => name.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ChoiceValue {
    Text(String),
    Bool(bool),
}

impl ChoiceValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Bool(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Probability {
    pub value: ChoiceValue,
    #[serde(default)]
    pub probability: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub input_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenDetails {
    #[serde(default)]
    pub cached_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
}

pub trait Decider {
    fn decide(
        &self,
        request: &DecisionRequest,
    ) -> impl Future<Output = Result<DecisionResponse, DecisionError>> + Send;
}

#[derive(Clone)]
pub struct HttpDecider {
    client: reqwest::Client,
    base: String,
    key: String,
    policy: DecisionPolicy,
}

impl HttpDecider {
    pub fn new(base: impl Into<String>, key: impl Into<String>, policy: DecisionPolicy) -> Self {
        let client = reqwest::Client::builder()
            .timeout(policy.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            base: base.into().trim_end_matches('/').to_owned(),
            key: key.into(),
            policy,
        }
    }
}

impl Decider for HttpDecider {
    fn decide(
        &self,
        request: &DecisionRequest,
    ) -> impl Future<Output = Result<DecisionResponse, DecisionError>> + Send {
        post_decision(&self.client, &self.base, &self.key, request, &self.policy)
    }
}

pub fn openai_base() -> String {
    std::env::var("OPENAI_BASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.openai.com".to_owned())
}

/// What the AI mapper may see, from BRAIAN_IMPORT_AI: off (rules only),
/// headers (column names only) or samples (names plus a few cell values, default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    Off,
    Headers,
    Samples,
}

impl AiMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value
            .map(|text| text.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("off") => Self::Off,
            Some("headers") => Self::Headers,
            _ => Self::Samples,
        }
    }

    pub fn from_env() -> Self {
        Self::parse(std::env::var("BRAIAN_IMPORT_AI").ok().as_deref())
    }
}

pub fn openai_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
}

pub fn log_missing_key_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if LOGGED
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        tracing::info!("import mapping has no API key; column mapping stays deterministic");
    }
}

pub async fn post_decision(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    request: &DecisionRequest,
    policy: &DecisionPolicy,
) -> Result<DecisionResponse, DecisionError> {
    let problems = contract_problems(request);
    if !problems.is_empty() {
        return Err(DecisionError::Decode(problems.join(", ")));
    }
    let url = format!("{}{DECISIONS_PATH}", base.trim_end_matches('/'));
    let mut attempt = 0u32;
    loop {
        let started = Instant::now();
        let sent = client
            .post(&url)
            .bearer_auth(key)
            .json(request)
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                if attempt >= policy.retries {
                    return Err(DecisionError::Timeout);
                }
                attempt += 1;
                continue;
            }
            Err(error) => {
                if attempt >= policy.retries {
                    return Err(DecisionError::Transport(error.to_string()));
                }
                attempt += 1;
                continue;
            }
        };
        let status = response.status();
        if status.is_success() {
            let text = response
                .text()
                .await
                .map_err(|error| DecisionError::Decode(error.to_string()))?;
            let parsed: DecisionResponse = decode_json(&text)?;
            log_usage(
                request.questions.len(),
                started.elapsed(),
                parsed.model.as_str(),
                parsed.usage.as_ref(),
            );
            return Ok(parsed);
        }
        let retry = status.as_u16() == 429 || status.is_server_error();
        let wait = retry_after(response.headers()).min(policy.max_retry_after);
        if retry && attempt < policy.retries {
            attempt += 1;
            tokio::time::sleep(wait).await;
            continue;
        }
        tracing::info!(
            questions = request.questions.len(),
            status = status.as_u16(),
            ms = started.elapsed().as_millis() as u64,
            "decisions call failed"
        );
        return Err(DecisionError::Status {
            code: status.as_u16(),
        });
    }
}

fn log_usage(questions: usize, elapsed: Duration, model: &str, usage: Option<&Usage>) {
    let input_tokens = usage.map(|item| item.input_tokens).unwrap_or(0);
    let output_tokens = usage.map(|item| item.output_tokens).unwrap_or(0);
    let total_tokens = usage.map(|item| item.total_tokens).unwrap_or(0);
    let _ = (model, output_tokens, total_tokens);
    let cached_tokens = usage
        .and_then(|item| item.input_tokens_details.as_ref())
        .map(|item| item.cached_tokens)
        .unwrap_or(0);
    let cache_write_tokens = usage
        .and_then(|item| item.input_tokens_details.as_ref())
        .map(|item| item.cache_write_tokens)
        .unwrap_or(0);
    tracing::info!(
        questions,
        ms = elapsed.as_millis() as u64,
        input_tokens,
        cached_tokens,
        cache_write_tokens,
        "decisions call"
    );
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Duration {
    let Some(value) = headers.get(reqwest::header::RETRY_AFTER) else {
        return Duration::from_millis(200);
    };
    let Ok(text) = value.to_str() else {
        return Duration::from_millis(200);
    };
    text.parse::<u64>()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_millis(200))
}

pub fn explicit_cache() -> PromptCacheOptions {
    PromptCacheOptions { mode: "explicit" }
}

pub fn breakpoint() -> Breakpoint {
    Breakpoint { mode: "explicit" }
}

/// Stable prefix first, then the file.
///
/// The data model is identical for every file at one revision, so it leads
/// and ends on an explicit breakpoint. The file is identical across the two
/// calls of one import. A second breakpoint after the file lets that shared
/// prefix hit when the model alone is shorter than the cache minimum.
/// Questions stay outside `input`, so they never sit in front of the prefix.
pub fn shared_input(model_json: Value, file_json: Value) -> Vec<InputMessage> {
    vec![
        InputMessage {
            role: "developer",
            content: vec![ContentPart::Text {
                text: DEVELOPER_TEXT.to_owned(),
                prompt_cache_breakpoint: None,
            }],
        },
        InputMessage {
            role: "user",
            content: vec![
                ContentPart::Text {
                    text: "DATA MODEL".to_owned(),
                    prompt_cache_breakpoint: None,
                },
                ContentPart::Json { data: model_json },
                ContentPart::Text {
                    text: "END OF DATA MODEL.".to_owned(),
                    prompt_cache_breakpoint: Some(breakpoint()),
                },
                ContentPart::Text {
                    text: "FILE: header row and the first 20 data rows, column-oriented."
                        .to_owned(),
                    prompt_cache_breakpoint: None,
                },
                ContentPart::Json { data: file_json },
                ContentPart::Text {
                    text: "END OF FILE.".to_owned(),
                    prompt_cache_breakpoint: Some(breakpoint()),
                },
            ],
        },
    ]
}

pub fn choice_question(
    name: impl Into<String>,
    instructions: impl Into<String>,
    choices: Vec<ChoiceOption>,
) -> ChoiceQuestion {
    ChoiceQuestion {
        kind: "choice",
        name: name.into(),
        instructions: instructions.into(),
        choices,
    }
}

/// Problems that would make a request violate the documented contract.
pub fn contract_problems(request: &DecisionRequest) -> Vec<String> {
    let mut problems = Vec::new();
    if request.model != DECISIONS_MODEL {
        problems.push("model".into());
    }
    if request.prompt_cache_key.chars().count() > 64 {
        problems.push("prompt_cache_key length".into());
    }
    if request.safety_identifier.chars().count() > 128 {
        problems.push("safety_identifier length".into());
    }
    if request.prompt_cache_options.mode != "explicit" {
        problems.push("prompt cache mode".into());
    }
    if request.questions.is_empty() || request.questions.len() > MAX_QUESTIONS {
        problems.push("question count".into());
    }
    for question in &request.questions {
        if question.kind != "choice" {
            problems.push(format!("question {} kind", question.name));
        }
        if question.choices.len() < MIN_CHOICES || question.choices.len() > MAX_CHOICES {
            problems.push(format!("question {} choices", question.name));
        }
        let mut seen = std::collections::BTreeSet::new();
        for choice in &question.choices {
            if !seen.insert(choice.value.clone()) {
                problems.push(format!(
                    "question {} duplicate {}",
                    question.name, choice.value
                ));
            }
        }
    }
    let Ok(value) = serde_json::to_value(request) else {
        problems.push("serialize".into());
        return problems;
    };
    if value.get("tools").is_some()
        || value.get("functions").is_some()
        || value.get("response_format").is_some()
    {
        problems.push("extra fields".into());
    }
    let Some(input) = value.get("input").and_then(Value::as_array) else {
        problems.push("input".into());
        return problems;
    };
    let Some(developer) = input
        .first()
        .and_then(|item| item.get("content"))
        .and_then(Value::as_array)
    else {
        problems.push("developer".into());
        return problems;
    };
    let developer_text = developer
        .first()
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str);
    if developer_text != Some(DEVELOPER_TEXT) {
        problems.push("developer text".into());
    }
    let Some(user) = input
        .get(1)
        .and_then(|item| item.get("content"))
        .and_then(Value::as_array)
    else {
        problems.push("user".into());
        return problems;
    };
    let json_before_break = user
        .iter()
        .position(|part| part.get("type").and_then(Value::as_str) == Some("input_json"));
    let breakpoint_at = user.iter().position(|part| {
        part.get("text").and_then(Value::as_str) == Some("END OF DATA MODEL.")
            && part
                .get("prompt_cache_breakpoint")
                .and_then(|item| item.get("mode"))
                .and_then(Value::as_str)
                == Some("explicit")
    });
    let file_at = user
        .iter()
        .rposition(|part| part.get("type").and_then(Value::as_str) == Some("input_json"));
    let file_break = user.iter().rposition(|part| {
        part.get("text").and_then(Value::as_str) == Some("END OF FILE.")
            && part
                .get("prompt_cache_breakpoint")
                .and_then(|item| item.get("mode"))
                .and_then(Value::as_str)
                == Some("explicit")
    });
    match (json_before_break, breakpoint_at, file_at, file_break) {
        (Some(model_at), Some(break_at), Some(file_at), Some(file_break))
            if model_at < break_at && break_at < file_at && file_at < file_break => {}
        _ => problems.push("cache order".into()),
    }
    if let Some(file) = file_at.and_then(|index| user.get(index)) {
        let samples = file
            .pointer("/data/columns")
            .and_then(Value::as_array)
            .map(|columns| {
                columns
                    .iter()
                    .filter_map(|column| column.get("samples").and_then(Value::as_array))
                    .map(Vec::len)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        if samples > 20 {
            problems.push("sample rows".into());
        }
    }
    problems
}

pub fn usage_record(
    call: &str,
    questions: usize,
    elapsed: Duration,
    usage: Option<&Usage>,
) -> Value {
    let details = usage.and_then(|item| item.input_tokens_details.as_ref());
    serde_json::json!({
        "call": call,
        "questions": questions,
        "input_tokens": usage.map(|item| item.input_tokens).unwrap_or(0),
        "cached_tokens": details.map(|item| item.cached_tokens).unwrap_or(0),
        "cache_write_tokens": details.map(|item| item.cache_write_tokens).unwrap_or(0),
        "ms": elapsed.as_millis() as u64,
    })
}

pub fn json_limits(value: &Value) -> (usize, usize, usize) {
    let bytes = serde_json::to_vec(value)
        .map(|buf| buf.len())
        .unwrap_or(usize::MAX);
    (json_nodes(value), bytes, json_depth(value))
}

pub fn within_json_limits(value: &Value) -> bool {
    let (nodes, bytes, depth) = json_limits(value);
    nodes <= 10_000 && bytes <= 256 * 1024 && depth <= 32
}

fn json_nodes(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(json_nodes).sum::<usize>(),
        Value::Object(map) => 1 + map.values().map(json_nodes).sum::<usize>(),
        _ => 1,
    }
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

pub fn decode_json<T: DeserializeOwned>(text: &str) -> Result<T, DecisionError> {
    serde_json::from_str(text).map_err(|error| DecisionError::Decode(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Json;
    use axum::http::StatusCode;
    use axum::routing::post;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample_request() -> DecisionRequest {
        DecisionRequest {
            model: DECISIONS_MODEL,
            prompt_cache_key: "braian-import-0123456789abcdef".into(),
            safety_identifier: "abc".into(),
            prompt_cache_options: explicit_cache(),
            input: shared_input(
                serde_json::json!({ "model_revision": 1, "types": [] }),
                serde_json::json!({
                    "file_name": "prices.csv",
                    "sheet": "CSV",
                    "columns": [{ "id": "col:0", "header": "Artnr", "samples": ["A-1"] }]
                }),
            ),
            questions: vec![choice_question(
                "type",
                "Which object type does each row of this file describe?",
                vec![
                    ChoiceOption {
                        value: "item".into(),
                        description: Some("Item".into()),
                    },
                    ChoiceOption {
                        value: "other".into(),
                        description: None,
                    },
                ],
            )],
        }
    }

    #[test]
    fn request_matches_the_documented_contract() {
        let request = sample_request();
        assert!(
            contract_problems(&request).is_empty(),
            "{:?}",
            contract_problems(&request)
        );
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["model"], DECISIONS_MODEL);
        assert_eq!(
            value["questions"][0]["instructions"],
            "Which object type does each row of this file describe?"
        );
        assert!(value["questions"][0].get("question").is_none());
        let part = &value["input"][1]["content"][1];
        assert_eq!(part["type"], "input_json");
        assert!(part.get("data").is_some());
        assert!(part.get("json").is_none());
        assert_eq!(value["prompt_cache_options"]["mode"], "explicit");
    }

    #[test]
    fn refusal_and_choice_deserialize() {
        let refusal: DecisionResponse = decode_json(
            r#"{"model":"m","answers":[{"type":"refusal","name":"type"}],"usage":{"input_tokens":3,"input_tokens_details":{"cached_tokens":1,"cache_write_tokens":2},"output_tokens":0,"total_tokens":3}}"#,
        )
        .unwrap();
        assert!(matches!(refusal.answers[0], Answer::Refusal { .. }));
        assert_eq!(
            refusal
                .usage
                .unwrap()
                .input_tokens_details
                .unwrap()
                .cached_tokens,
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retries_once_on_429_then_reads_the_answer() {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let app = axum::Router::new().route(
            DECISIONS_PATH,
            post(move |Json(body): Json<Value>| {
                let counter = Arc::clone(&counter);
                async move {
                    let n = counter.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["model"], DECISIONS_MODEL);
                    if n == 0 {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [("retry-after", "0")],
                            axum::Json(serde_json::json!({})),
                        )
                    } else {
                        (
                            StatusCode::OK,
                            [("retry-after", "0")],
                            axum::Json(serde_json::json!({
                                "model": DECISIONS_MODEL,
                                "answers": [{
                                    "type": "choice",
                                    "name": "type",
                                    "choice": "item",
                                    "confidence": 0.91,
                                    "probabilities": [{ "value": "item", "probability": 0.91 }]
                                }]
                            })),
                        )
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let response = post_decision(
            &client,
            &format!("http://{address}"),
            "test-key",
            &sample_request(),
            &DecisionPolicy {
                timeout: Duration::from_secs(2),
                retries: 1,
                max_retry_after: Duration::from_secs(2),
            },
        )
        .await
        .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        match &response.answers[0] {
            Answer::Choice { choice, .. } => assert_eq!(choice.as_text(), Some("item")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn json_limit_counts_nodes_bytes_and_depth() {
        let small = serde_json::json!({"a": 1});
        assert!(within_json_limits(&small));
        let mut nested = serde_json::json!(1);
        for _ in 0..40 {
            nested = serde_json::json!([nested]);
        }
        assert!(!within_json_limits(&nested));
    }
}
