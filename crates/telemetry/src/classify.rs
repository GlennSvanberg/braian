//! Optional task-segment classifier. One Decisions API call per closed
//! segment, never per event. Off unless `BRAIAN_TELEMETRY_CLASSIFIER=1` and
//! `OPENAI_API_KEY` is set. The key is read from the environment and is not
//! logged or written to the repo.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::rules::TASK_TYPES;

pub const MODEL: &str = "gpt-6-luna";

#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    pub task_type: String,
    pub new_task: bool,
    pub probability: f64,
}

#[derive(Debug, Clone)]
pub struct SegmentSummary {
    pub rule_type: String,
    pub operations: Vec<String>,
    pub actor_counts: BTreeMap<String, u32>,
    pub object_count: u32,
    pub duration_ms: i64,
    pub ambiguous: bool,
}

pub struct DecisionsConfig {
    api_key: String,
    base: String,
    pub threshold: f64,
}

impl DecisionsConfig {
    pub fn new(api_key: String, base: String, threshold: f64) -> Self {
        Self {
            api_key,
            base: base.trim_end_matches('/').to_owned(),
            threshold,
        }
    }
}

pub enum ClassifierMode {
    Off,
    Decisions(DecisionsConfig),
    /// Test double. Production uses [`ClassifierMode::from_env`].
    Fixed(Classified),
}

pub fn enabled_by(flag: &str, key: &str) -> bool {
    flag == "1" && !key.trim().is_empty()
}

impl ClassifierMode {
    pub fn from_env() -> Self {
        let flag = std::env::var("BRAIAN_TELEMETRY_CLASSIFIER").unwrap_or_default();
        let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        if !enabled_by(&flag, &key) {
            return Self::Off;
        }
        let threshold = std::env::var("BRAIAN_TELEMETRY_CLASSIFIER_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| (0.0..=1.0).contains(value))
            .unwrap_or(0.7);
        let base = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_owned());
        Self::Decisions(DecisionsConfig::new(key, base, threshold))
    }

    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn threshold(&self) -> f64 {
        match self {
            Self::Decisions(config) => config.threshold,
            Self::Fixed(_) => 0.7,
            Self::Off => 1.0,
        }
    }

    pub async fn classify(&self, segment: &SegmentSummary) -> Option<Classified> {
        match self {
            Self::Off => None,
            Self::Fixed(value) => Some(value.clone()),
            Self::Decisions(config) => decide(config, segment).await,
        }
    }
}

pub fn decision_body(segment: &SegmentSummary) -> Value {
    let choices: Vec<Value> = TASK_TYPES
        .iter()
        .map(|value| {
            json!({
                "value": value,
                "description": task_description(value),
            })
        })
        .collect();
    json!({
        "model": MODEL,
        "input": [{
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": "Classify this task segment. The evidence is operation names, counts, and durations. There is no field content and no person."
                },
                {
                    "type": "input_json",
                    "data": {
                        "rule_type": segment.rule_type,
                        "operations": segment.operations,
                        "actor_counts": segment.actor_counts,
                        "object_count": segment.object_count,
                        "duration_ms": segment.duration_ms,
                        "ambiguous": segment.ambiguous
                    }
                }
            ]
        }],
        "questions": [
            {
                "type": "choice",
                "name": "task_type",
                "instructions": "Which task type is this segment?",
                "choices": choices
            },
            {
                "type": "predicate",
                "name": "new_task",
                "instructions": "Does the ambiguous hop inside this segment start a new task rather than continue the earlier operations?"
            }
        ]
    })
}

fn task_description(value: &str) -> &'static str {
    match value {
        "import" => "A bulk import or job, and follow-up work on those objects.",
        "mass_edit" => "A mass edit across many objects.",
        "enrich" => "Search or open followed by edits on those objects.",
        "search" => "Search and filter without edits.",
        "navigate" => "View, layout, and column changes.",
        "review" => "History, proposals, impact, or restore.",
        "model" => "Data model changes.",
        "agent" => "A command run for the person by the agent.",
        _ => "Does not fit the other types.",
    }
}

pub fn parse_answers(body: &Value, threshold: f64) -> Option<Classified> {
    let answers = body.get("answers")?.as_array()?;
    let mut task_type = None;
    let mut probability = 0.0;
    let mut new_task = false;
    for answer in answers {
        let name = answer.get("name").and_then(Value::as_str).unwrap_or("");
        let kind = answer.get("type").and_then(Value::as_str).unwrap_or("");
        if name == "task_type" && kind == "choice" {
            let choice = answer.get("choice").and_then(Value::as_str)?;
            if TASK_TYPES.contains(&choice) {
                task_type = Some(choice.to_owned());
            }
        }
        if name == "new_task" && kind == "predicate" {
            probability = answer
                .get("probability")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            new_task = probability >= threshold;
        }
    }
    Some(Classified {
        task_type: task_type?,
        new_task,
        probability,
    })
}

async fn decide(config: &DecisionsConfig, segment: &SegmentSummary) -> Option<Classified> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;
    let response = client
        .post(format!("{}/v1/decisions", config.base))
        .bearer_auth(&config.api_key)
        .json(&decision_body(segment))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "telemetry classifier skipped a segment");
        return None;
    }
    let body: Value = response.json().await.ok()?;
    parse_answers(&body, config.threshold)
}

/// Where a classifier answer splits a segment. The tail moves; the head stays.
#[derive(Debug, PartialEq, Eq)]
pub struct SplitPlan {
    pub task_type: String,
    pub move_ids: Vec<uuid::Uuid>,
}

#[derive(Debug, Clone, Copy)]
pub struct SegmentRef {
    pub id: uuid::Uuid,
    pub ambiguous: bool,
}

pub fn plan_split(events: &[SegmentRef], classified: &Classified, threshold: f64) -> SplitPlan {
    let move_ids = if classified.new_task && classified.probability >= threshold {
        let split_at = events.iter().position(|event| event.ambiguous);
        match split_at {
            Some(index) if index > 0 => events[index..].iter().map(|event| event.id).collect(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    SplitPlan {
        task_type: classified.task_type.clone(),
        move_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn body_asks_choice_and_predicate_without_field_content() {
        let body = decision_body(&SegmentSummary {
            rule_type: "search".into(),
            operations: vec!["search".into(), "patch_object".into()],
            actor_counts: BTreeMap::from([("human".into(), 2)]),
            object_count: 1,
            duration_ms: 4_000,
            ambiguous: true,
        });
        assert_eq!(body["model"], "gpt-6-luna");
        let questions = body["questions"].as_array().expect("questions");
        assert_eq!(questions[0]["type"], "choice");
        assert_eq!(questions[0]["name"], "task_type");
        assert_eq!(questions[1]["type"], "predicate");
        assert_eq!(questions[1]["name"], "new_task");
        let text = body.to_string();
        assert!(text.contains("input_json"));
        assert!(!text.contains("values"));
        assert!(!text.contains("@"));
    }

    #[test]
    fn parses_a_decision() {
        let body = json!({
            "answers": [
                {"type": "choice", "name": "task_type", "choice": "enrich", "confidence": 0.8},
                {"type": "predicate", "name": "new_task", "probability": 0.91}
            ]
        });
        let parsed = parse_answers(&body, 0.7).expect("parsed");
        assert_eq!(parsed.task_type, "enrich");
        assert!(parsed.new_task);
    }

    #[test]
    fn a_low_probability_does_not_split() {
        let body = json!({
            "answers": [
                {"type": "choice", "name": "task_type", "choice": "import"},
                {"type": "predicate", "name": "new_task", "probability": 0.4}
            ]
        });
        let parsed = parse_answers(&body, 0.7).expect("parsed");
        assert!(!parsed.new_task);
    }

    #[test]
    fn split_starts_at_the_first_ambiguous_event() {
        let first = Uuid::now_v7();
        let hop = Uuid::now_v7();
        let tail = Uuid::now_v7();
        let events = [
            SegmentRef {
                id: first,
                ambiguous: false,
            },
            SegmentRef {
                id: hop,
                ambiguous: true,
            },
            SegmentRef {
                id: tail,
                ambiguous: false,
            },
        ];
        let plan = plan_split(
            &events,
            &Classified {
                task_type: "enrich".into(),
                new_task: true,
                probability: 0.9,
            },
            0.7,
        );
        assert_eq!(plan.move_ids, vec![hop, tail]);
        assert_eq!(plan.task_type, "enrich");
    }

    #[test]
    fn the_classifier_stays_off_without_a_flag_and_a_key() {
        assert!(!enabled_by("1", ""));
        assert!(!enabled_by("0", "sk-test"));
        assert!(enabled_by("1", "sk-test"));
    }
}
