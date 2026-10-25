//! Usage telemetry. A separate Redpanda topic and a separate consumer.
//!
//! The change topics (`braian.changes.interactive`, `braian.changes.bulk`) are
//! not read or written here. Publish is fire-and-forget: a full queue or a
//! slow broker drops the batch.

mod classify;
mod consume;
mod rules;

pub use classify::{
    ClassifierMode, SegmentSummary, decision_body, enabled_by, parse_answers, plan_split,
};
pub use consume::run;
pub use rules::{
    ActorKind, Assigner, Cursor, Explicit, Observe, apply, canonical_task_type, is_change,
};

use std::time::Duration;

use chrono::{DateTime, Utc};
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, error::TrySendError};
use uuid::Uuid;

pub const TOPIC: &str = "braian.telemetry.ui";
pub const GROUP: &str = "braian-telemetry";
const QUEUE: usize = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_type: String,
    pub task_id: Uuid,
    pub task_type: String,
    pub session_id: Uuid,
    pub operation: String,
    pub object_ids: Vec<Uuid>,
    pub object_count: u32,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_batch_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<Uuid>,
    #[serde(default)]
    pub ambiguous: bool,
}

/// Starts a background publisher. The sender never waits on the broker.
/// A full or disconnected queue drops the batch.
pub fn spawn_publisher(brokers: &str) -> mpsc::Sender<Vec<UsageEvent>> {
    let (tx, mut rx) = mpsc::channel::<Vec<UsageEvent>>(QUEUE);
    let brokers = brokers.to_owned();
    tokio::spawn(async move {
        let producer = match producer(&brokers) {
            Ok(producer) => producer,
            Err(error) => {
                tracing::warn!(%error, "telemetry producer is off; events will be dropped");
                while rx.recv().await.is_some() {}
                return;
            }
        };
        if let Err(error) = ensure_topic(&brokers).await {
            tracing::warn!(%error, "telemetry topic was not created");
        }
        while let Some(batch) = rx.recv().await {
            if let Err(error) = publish(&producer, &batch).await {
                tracing::debug!(%error, "dropped a telemetry batch");
            }
        }
    });
    tx
}

/// How many events were dropped. `None` drops the whole batch.
pub fn enqueue(tx: Option<&mpsc::Sender<Vec<UsageEvent>>>, events: Vec<UsageEvent>) -> usize {
    let Some(tx) = tx else {
        return events.len();
    };
    if events.is_empty() {
        return 0;
    }
    match tx.try_send(events) {
        Ok(()) => 0,
        Err(TrySendError::Full(events) | TrySendError::Closed(events)) => events.len(),
    }
}

fn producer(brokers: &str) -> Result<FutureProducer, rdkafka::error::KafkaError> {
    ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("message.timeout.ms", "250")
        .set("acks", "1")
        .set("linger.ms", "20")
        .set("queue.buffering.max.messages", "1000")
        .set("compression.codec", "lz4")
        .create()
}

pub(crate) async fn ensure_topic(brokers: &str) -> Result<(), String> {
    let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .create()
        .map_err(|error| error.to_string())?;
    let topic = NewTopic::new(TOPIC, 1, TopicReplication::Fixed(1));
    let results = admin
        .create_topics(&[topic], &AdminOptions::new())
        .await
        .map_err(|error| error.to_string())?;
    for result in results {
        match result {
            Ok(_) => {}
            Err((_, error)) if error.to_string().contains("already exists") => {}
            Err((name, error)) => return Err(format!("create topic {name}: {error}")),
        }
    }
    Ok(())
}

async fn publish(producer: &FutureProducer, events: &[UsageEvent]) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }
    let payload = serde_json::to_vec(events).map_err(|error| error.to_string())?;
    let key = events[0].session_id.to_string();
    let record = FutureRecord::to(TOPIC).key(&key).payload(&payload);
    producer
        .send(record, Duration::from_millis(400))
        .await
        .map_err(|(error, _)| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_queue_drops_the_batch() {
        let (tx, _rx) = mpsc::channel(1);
        let first = sample();
        assert_eq!(enqueue(Some(&tx), vec![first.clone()]), 0);
        assert_eq!(enqueue(Some(&tx), vec![first]), 1);
    }

    #[test]
    fn a_missing_publisher_drops_everything() {
        let event = sample();
        assert_eq!(enqueue(None, vec![event]), 1);
    }

    #[test]
    fn field_content_is_not_part_of_the_event() {
        let raw = r#"{
            "id": "018f2000-0000-7000-8000-0000000000aa",
            "occurred_at": "2026-10-25T12:00:00Z",
            "actor_type": "human",
            "task_id": "018f2000-0000-7000-8000-0000000000bb",
            "task_type": "enrich",
            "session_id": "018f2000-0000-7000-8000-0000000000cc",
            "operation": "patch_object",
            "object_ids": [],
            "object_count": 1,
            "result": "ok",
            "values": {"namn": "hemligt"},
            "email": "ada@example.com"
        }"#;
        let event: UsageEvent = serde_json::from_str(raw).expect("event");
        let back = serde_json::to_string(&event).expect("json");
        assert!(!back.contains("hemligt"));
        assert!(!back.contains("ada@"));
        assert!(!back.contains("values"));
    }

    fn sample() -> UsageEvent {
        UsageEvent {
            id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            actor_type: "human".into(),
            task_id: Uuid::now_v7(),
            task_type: "search".into(),
            session_id: Uuid::now_v7(),
            operation: "search".into(),
            object_ids: Vec::new(),
            object_count: 0,
            result: "ok".into(),
            duration_ms: None,
            audit_batch_id: None,
            audit_seq: None,
            actor_id: None,
            on_behalf_of: None,
            ambiguous: false,
        }
    }
}
