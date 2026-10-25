//! Low-priority reader of `braian.telemetry.ui`.
//!
//! It does not subscribe to the change topics. A read or a database error is
//! logged and retried; it does not feed back into the API.

use std::collections::BTreeMap;
use std::time::Duration;

use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use uuid::Uuid;

use crate::classify::{ClassifierMode, SegmentRef, SegmentSummary, plan_split};
use crate::rules::canonical_task_type;
use crate::{GROUP, TOPIC, UsageEvent};
use braian_store::{Store, UsageEventRow};

pub async fn run(store: Store, brokers: &str, classifier: ClassifierMode) -> Result<(), String> {
    crate::ensure_topic(brokers).await?;
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", GROUP)
        .set("enable.auto.commit", "false")
        .set("enable.auto.offset.store", "false")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "10000")
        .set("fetch.wait.max.ms", "1000")
        .create()
        .map_err(|error| error.to_string())?;
    consumer
        .subscribe(&[TOPIC])
        .map_err(|error| error.to_string())?;
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                maintain(&store, &classifier).await;
            }
            message = consumer.recv() => {
                match message {
                    Ok(message) => {
                        let payload = message.payload().unwrap_or_default();
                        let events = decode(payload);
                        if !events.is_empty()
                            && let Err(error) = store_events(&store, &events).await
                        {
                            tracing::warn!(%error, "telemetry insert failed; the batch stays uncommitted");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                        if let Err(error) = consumer.store_offset_from_message(&message) {
                            tracing::debug!(%error, "telemetry offset was not stored");
                        }
                        if let Err(error) = consumer.commit_consumer_state(CommitMode::Async) {
                            tracing::debug!(%error, "telemetry commit skipped");
                        }
                        tokio::task::yield_now().await;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "telemetry read failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

fn decode(payload: &[u8]) -> Vec<UsageEvent> {
    if let Ok(events) = serde_json::from_slice::<Vec<UsageEvent>>(payload) {
        return events;
    }
    serde_json::from_slice::<UsageEvent>(payload)
        .map(|event| vec![event])
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "dropped a telemetry message");
            Vec::new()
        })
}

async fn store_events(store: &Store, events: &[UsageEvent]) -> Result<(), braian_store::Error> {
    let rows = events
        .iter()
        .map(|event| UsageEventRow {
            id: event.id,
            occurred_at: event.occurred_at,
            actor_type: event.actor_type.clone(),
            task_id: event.task_id,
            task_type: canonical_task_type(&event.task_type).to_owned(),
            session_id: event.session_id,
            operation: event.operation.clone(),
            object_ids: event.object_ids.clone(),
            object_count: i32::try_from(event.object_count).unwrap_or(i32::MAX),
            result: event.result.clone(),
            duration_ms: event.duration_ms,
            audit_batch_id: event.audit_batch_id,
            audit_seq: event.audit_seq,
            actor_id: event.actor_id,
            on_behalf_of: event.on_behalf_of,
            ambiguous: event.ambiguous,
            is_change: crate::is_change(&event.operation),
        })
        .collect::<Vec<_>>();
    store.insert_usage_events(&rows).await
}

async fn maintain(store: &Store, classifier: &ClassifierMode) {
    let settings = match store.usage_settings().await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::debug!(%error, "telemetry settings were not read");
            return;
        }
    };
    if let Err(error) = store
        .close_idle_tasks(settings.inactivity_gap_seconds)
        .await
    {
        tracing::debug!(%error, "telemetry idle close failed");
    }
    if let Err(error) = store.purge_usage_events(settings.raw_retention_days).await {
        tracing::debug!(%error, "telemetry retention failed");
    }
    if classifier.enabled() {
        classify_closed(store, classifier).await;
    }
}

async fn classify_closed(store: &Store, classifier: &ClassifierMode) {
    let tasks = match store.tasks_to_classify().await {
        Ok(tasks) => tasks,
        Err(error) => {
            tracing::debug!(%error, "telemetry classify list failed");
            return;
        }
    };
    for task_id in tasks {
        let events = match store.segment_events(task_id).await {
            Ok(events) => events,
            Err(error) => {
                tracing::debug!(%error, "telemetry segment was not read");
                continue;
            }
        };
        if events.is_empty() {
            continue;
        }
        let summary = summary_of(&events);
        let Some(classified) = classifier.classify(&summary).await else {
            continue;
        };
        let refs = events
            .iter()
            .map(|event| SegmentRef {
                id: event.id,
                ambiguous: event.ambiguous,
            })
            .collect::<Vec<_>>();
        let plan = plan_split(&refs, &classified, classifier.threshold());
        if plan.move_ids.is_empty() {
            if let Err(error) = store
                .set_task_classification(task_id, &plan.task_type)
                .await
            {
                tracing::debug!(%error, "telemetry classification was not stored");
            }
            continue;
        }
        let fresh = Uuid::now_v7();
        if let Err(error) = store
            .retask_events(&plan.move_ids, fresh, &plan.task_type)
            .await
        {
            tracing::debug!(%error, "telemetry split was not stored");
            continue;
        }
        let mut ids = vec![task_id, fresh];
        ids.sort();
        ids.dedup();
        if let Err(error) = store.recompute_usage_tasks(&ids).await {
            tracing::debug!(%error, "telemetry recompute failed");
            continue;
        }
        let _ = store.set_task_classification(fresh, &plan.task_type).await;
        let _ = store
            .set_task_classification(task_id, summary.rule_type.as_str())
            .await;
    }
}

fn summary_of(events: &[braian_store::SegmentEvent]) -> SegmentSummary {
    let mut actor_counts = BTreeMap::new();
    let mut operations = Vec::new();
    let mut object_count = 0u32;
    let mut duration_ms = 0i64;
    let mut ambiguous = false;
    let rule_type = events
        .last()
        .map(|event| canonical_task_type(&event.task_type).to_owned())
        .unwrap_or_else(|| "other".to_owned());
    for event in events {
        operations.push(event.operation.clone());
        *actor_counts.entry(event.actor_type.clone()).or_insert(0) += 1;
        object_count = object_count.max(u32::try_from(event.object_count).unwrap_or(0));
        if let Some(duration) = event.duration_ms {
            duration_ms = duration_ms.max(i64::from(duration));
        }
        ambiguous |= event.ambiguous;
    }
    if let (Some(first), Some(last)) = (events.first(), events.last()) {
        duration_ms = duration_ms.max(
            (last.occurred_at - first.occurred_at)
                .num_milliseconds()
                .max(0),
        );
    }
    SegmentSummary {
        rule_type,
        operations,
        actor_counts,
        object_count,
        duration_ms,
        ambiguous,
    }
}
