use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use braian_model::{DefinitionSet, ObjectId};
use braian_rules::CompiledModel;
use braian_search::Typesense;
use braian_store::{Error, Store};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, mpsc};
use uuid::Uuid;

const SEQ_SEEN_CAP: usize = 10_000;
const LIVE_QUEUE: usize = 1024;
const WEBHOOK_QUEUE: usize = 256;
const BULK_FLUSH_EVENTS: u64 = 200;
const BULK_FLUSH_EVERY: Duration = Duration::from_millis(200);

use crate::auth::Identity;
use crate::bulk_gate::BulkGate;
use crate::problem::{Problem, store_status};

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub identities: Arc<Vec<Identity>>,
    pub models: ModelCache,
    pub search: Option<Typesense>,
    pub live: broadcast::Sender<Value>,
    pub cookie_secure: bool,
    pub bulk: BulkGate,
    seen: Arc<Mutex<SeqSeen>>,
    live_tx: mpsc::Sender<Value>,
    index_tx: Option<mpsc::Sender<IndexJob>>,
}

/// Remembers recent change seqs and forgets the oldest when the cap is hit.
///
/// Clearing the whole set at 10k made the next replay of those seqs look new.
struct SeqSeen {
    order: VecDeque<i64>,
    set: HashSet<i64>,
    cap: usize,
}

impl SeqSeen {
    fn new(cap: usize) -> Self {
        Self {
            order: VecDeque::new(),
            set: HashSet::new(),
            cap,
        }
    }

    fn insert(&mut self, seq: i64) -> bool {
        if !self.set.insert(seq) {
            return false;
        }
        self.order.push_back(seq);
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }
}

#[derive(Debug, Default)]
struct BulkFold {
    count: u64,
    from_seq: Option<i64>,
    to_seq: Option<i64>,
}

impl BulkFold {
    fn push(&mut self, body: &Value) {
        self.count += 1;
        if let Some(seq) = body.get("seq").and_then(Value::as_i64) {
            if self.from_seq.is_none() {
                self.from_seq = Some(seq);
            }
            self.to_seq = Some(seq);
        }
    }

    fn finish(&mut self) -> Option<Value> {
        if self.count == 0 {
            return None;
        }
        let summary = json!({
            "op": "bulk_summary",
            "count": self.count,
            "from_seq": self.from_seq,
            "to_seq": self.to_seq,
        });
        *self = Self::default();
        Some(summary)
    }
}

fn is_bulk(body: &Value) -> bool {
    body.get("kind").and_then(Value::as_str) == Some("bulk")
        || body.get("op").and_then(Value::as_str) == Some("bulk")
}

/// Interactive events pass through. Bulk events collapse into one summary
/// every [`BULK_FLUSH_EVENTS`] events, or when the next event is interactive.
fn fold_live(fold: &mut BulkFold, body: Value) -> Vec<Value> {
    if is_bulk(&body) {
        fold.push(&body);
        if fold.count >= BULK_FLUSH_EVENTS {
            fold.finish().into_iter().collect()
        } else {
            Vec::new()
        }
    } else {
        let mut out = Vec::new();
        if let Some(summary) = fold.finish() {
            out.push(summary);
        }
        out.push(body);
        out
    }
}

struct IndexJob {
    id: ObjectId,
    seq: i64,
}

impl AppState {
    pub fn new(store: Store, identities: Vec<Identity>, search: Option<Typesense>) -> Self {
        let (live, _) = broadcast::channel(1024);
        let (live_tx, mut live_rx) = mpsc::channel::<Value>(LIVE_QUEUE);
        let (hook_tx, mut hook_rx) = mpsc::channel::<Value>(WEBHOOK_QUEUE);
        let hook_store = store.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            while let Some(body) = hook_rx.recv().await {
                crate::platform::deliver_webhooks(&hook_store, &client, &body).await;
            }
        });
        let broadcast = live.clone();
        tokio::spawn(async move {
            let mut fold = BulkFold::default();
            let mut tick = tokio::time::interval(BULK_FLUSH_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    incoming = live_rx.recv() => {
                        let Some(body) = incoming else { break };
                        for message in fold_live(&mut fold, body) {
                            publish(&broadcast, &hook_tx, message);
                        }
                    }
                    _ = tick.tick() => {
                        if let Some(summary) = fold.finish() {
                            publish(&broadcast, &hook_tx, summary);
                        }
                    }
                }
            }
        });
        let cookie_secure = std::env::var("BRAIAN_COOKIE_SECURE").ok().as_deref() == Some("1");
        let index_tx = search.as_ref().map(|search| {
            let (tx, mut rx) = mpsc::channel::<IndexJob>(64);
            let store = store.clone();
            let search = search.clone();
            tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    if let Err(error) =
                        braian_indexer::index_written(&store, &search, &[job.id], job.seq).await
                    {
                        tracing::warn!(%error, "search write-through failed");
                    }
                }
            });
            tx
        });
        Self {
            store,
            identities: Arc::new(identities),
            models: ModelCache::default(),
            search,
            live,
            cookie_secure,
            bulk: BulkGate::from_env(),
            seen: Arc::new(Mutex::new(SeqSeen::new(SEQ_SEEN_CAP))),
            live_tx,
            index_tx,
        }
    }

    /// Best-effort search update after an interactive commit. A full queue is
    /// dropped; the Redpanda indexer still writes the document.
    pub fn index_after_commit(&self, id: Uuid, seq: i64) {
        let Some(tx) = &self.index_tx else {
            return;
        };
        if tx
            .try_send(IndexJob {
                id: ObjectId::from_uuid(id),
                seq,
            })
            .is_err()
        {
            tracing::debug!(%id, seq, "search write-through queue is full");
        }
    }

    /// Queues one change for the live stream and webhooks.
    ///
    /// A repeated `seq` is ignored so a local write and the Kafka consumer do
    /// not notify twice. Bulk events are folded by the worker. A full queue is
    /// dropped; the change is still in Postgres and Redpanda.
    pub fn emit(&self, body: Value) {
        if let Some(seq) = body.get("seq").and_then(Value::as_i64) {
            let mut seen = self
                .seen
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if !seen.insert(seq) {
                return;
            }
        }
        if self.live_tx.try_send(body).is_err() {
            tracing::debug!("live queue is full");
        }
    }
}

fn publish(live: &broadcast::Sender<Value>, hooks: &mpsc::Sender<Value>, message: Value) {
    let _ = live.send(message.clone());
    if hooks.try_send(message).is_err() {
        tracing::debug!("webhook queue is full");
    }
}

pub struct CachedModel {
    pub revision: u64,
    pub defs: DefinitionSet,
    pub model: CompiledModel,
}

#[derive(Clone, Default)]
pub struct ModelCache {
    inner: Arc<RwLock<Option<Arc<CachedModel>>>>,
}

impl ModelCache {
    pub async fn get(&self, store: &Store) -> Result<Arc<CachedModel>, Problem> {
        let Some(revision) = store.model_revision().await.map_err(store_status)? else {
            return Err(store_status(Error::NoDefinitions));
        };
        if let Some(cached) = self.inner.read().await.clone()
            && cached.revision == revision
        {
            return Ok(cached);
        }
        let defs = store.definition_set().await.map_err(store_status)?;
        let model = CompiledModel::compile(&defs).map_err(Problem::definitions)?;
        let cached = Arc::new(CachedModel {
            revision: defs.model_revision,
            defs,
            model,
        });
        *self.inner.write().await = Some(Arc::clone(&cached));
        Ok(cached)
    }

    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{BulkFold, SeqSeen, fold_live};
    use serde_json::json;

    #[test]
    fn seq_seen_forgets_the_oldest_seq_and_keeps_the_rest() {
        let mut seen = SeqSeen::new(2);
        assert!(seen.insert(1));
        assert!(seen.insert(2));
        assert!(!seen.insert(1));
        assert!(seen.insert(3));
        assert!(!seen.insert(2));
        assert!(seen.insert(1));
        assert!(!seen.insert(3));
    }

    #[test]
    fn bulk_events_collapse_and_an_interactive_event_flushes_them() {
        let mut fold = BulkFold::default();
        assert!(fold_live(&mut fold, json!({"op": "create", "kind": "bulk", "seq": 4})).is_empty());
        assert!(fold_live(&mut fold, json!({"op": "create", "kind": "bulk", "seq": 5})).is_empty());
        let flushed = fold_live(&mut fold, json!({"op": "update", "seq": 6}));
        assert_eq!(flushed.len(), 2);
        assert_eq!(flushed[0]["op"], "bulk_summary");
        assert_eq!(flushed[0]["count"], 2);
        assert_eq!(flushed[0]["from_seq"], 4);
        assert_eq!(flushed[0]["to_seq"], 5);
        assert_eq!(flushed[1]["op"], "update");
        assert!(fold.finish().is_none());
    }

    #[test]
    fn two_hundred_bulk_events_become_one_summary() {
        let mut fold = BulkFold::default();
        for seq in 1..200 {
            assert!(
                fold_live(&mut fold, json!({"op": "bulk", "seq": seq})).is_empty(),
                "{seq}"
            );
        }
        let summary = fold_live(&mut fold, json!({"op": "bulk", "seq": 200}));
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0]["count"], 200);
        assert_eq!(summary[0]["from_seq"], 1);
        assert_eq!(summary[0]["to_seq"], 200);
    }
}
