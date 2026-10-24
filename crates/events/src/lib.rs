//! Change events published by the relay and consumed by the indexer, the
//! realtime stream and webhooks.
//!
//! A message is "object X changed", not the values. Topics have one partition
//! so commit order survives a crash. A replay can deliver the same `seq` twice.

use std::time::Duration;

use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::error::KafkaError;
use rdkafka::message::Message;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const INTERACTIVE_TOPIC: &str = "braian.changes.interactive";
pub const BULK_TOPIC: &str = "braian.changes.bulk";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub seq: i64,
    pub op: String,
    pub object_id: Option<Uuid>,
    pub version: Option<i64>,
    pub batch_id: Uuid,
    pub kind: String,
    pub at: String,
}

impl ChangeEvent {
    pub fn topic(&self) -> &'static str {
        if self.kind == "bulk" {
            BULK_TOPIC
        } else {
            INTERACTIVE_TOPIC
        }
    }

    pub fn key(&self) -> String {
        self.object_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("seq-{}", self.seq))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("kafka: {0}")]
    Kafka(String),
    #[error("event json: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<KafkaError> for Error {
    fn from(error: KafkaError) -> Self {
        Self::Kafka(error.to_string())
    }
}

#[derive(Clone)]
pub struct Bus {
    producer: FutureProducer,
    brokers: String,
}

impl Bus {
    pub fn connect(brokers: &str) -> Result<Self, Error> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "10000")
            .set("acks", "all")
            .set("enable.idempotence", "true")
            .create()?;
        Ok(Self {
            producer,
            brokers: brokers.to_owned(),
        })
    }

    pub async fn ensure_topics(&self) -> Result<(), Error> {
        let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
            .set("bootstrap.servers", &self.brokers)
            .create()?;
        let topics = [
            NewTopic::new(INTERACTIVE_TOPIC, 1, TopicReplication::Fixed(1)),
            NewTopic::new(BULK_TOPIC, 1, TopicReplication::Fixed(1)),
        ];
        let results = admin
            .create_topics(&topics, &AdminOptions::new())
            .await
            .map_err(|error| Error::Kafka(error.to_string()))?;
        for result in results {
            match result {
                Ok(_) => {}
                Err((_, error)) if error.to_string().contains("already exists") => {}
                Err((name, error)) => {
                    return Err(Error::Kafka(format!("create topic {name}: {error}")));
                }
            }
        }
        Ok(())
    }

    /// Drops both change topics so a test starts from an empty log.
    /// Missing topics are not an error. Works against Redpanda and Apache Kafka.
    pub async fn delete_topics(&self) -> Result<(), Error> {
        let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
            .set("bootstrap.servers", &self.brokers)
            .create()?;
        let names = [INTERACTIVE_TOPIC, BULK_TOPIC];
        let results = admin
            .delete_topics(&names, &AdminOptions::new())
            .await
            .map_err(|error| Error::Kafka(error.to_string()))?;
        for result in results {
            match result {
                Ok(_) => {}
                Err((_, error))
                    if error.to_string().contains("Unknown")
                        || error.to_string().contains("does not exist") => {}
                Err((name, error)) => {
                    return Err(Error::Kafka(format!("delete topic {name}: {error}")));
                }
            }
        }
        Ok(())
    }

    pub async fn publish(&self, event: &ChangeEvent) -> Result<(), Error> {
        let payload = serde_json::to_vec(event)?;
        let key = event.key();
        let record = FutureRecord::to(event.topic())
            .key(&key)
            .payload(payload.as_slice());
        self.producer
            .send(record, Duration::from_secs(10))
            .await
            .map_err(|(error, _)| Error::Kafka(error.to_string()))?;
        Ok(())
    }
}

pub struct Subscription {
    consumer: StreamConsumer,
}

impl Subscription {
    pub fn subscribe(brokers: &str, group: &str, topics: &[&str]) -> Result<Self, Error> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("group.id", group)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", "earliest")
            .set("session.timeout.ms", "10000")
            .create()?;
        consumer.subscribe(topics)?;
        Ok(Self { consumer })
    }

    pub async fn next(&self) -> Result<ChangeEvent, Error> {
        loop {
            let message = self.consumer.recv().await?;
            let Some(payload) = message.payload() else {
                continue;
            };
            let event: ChangeEvent = serde_json::from_slice(payload)?;
            self.consumer.store_offset_from_message(&message)?;
            return Ok(event);
        }
    }

    pub fn commit(&self) -> Result<(), Error> {
        match self.consumer.commit_consumer_state(CommitMode::Sync) {
            Ok(()) => Ok(()),
            // A subscription that saw no records in this window has nothing to commit.
            // Failing the process there stops indexing after the first interactive batch.
            Err(error) if error.to_string().contains("NoOffset") => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
