use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use braian_model::{DefinitionSet, ObjectId};
use braian_rules::CompiledModel;
use braian_search::Typesense;
use braian_store::{Error, Store};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, mpsc};
use uuid::Uuid;

use crate::auth::Identity;
use crate::problem::{Problem, store_status};

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub identities: Arc<Vec<Identity>>,
    pub models: ModelCache,
    pub search: Option<Typesense>,
    pub live: broadcast::Sender<Value>,
    pub cookie_secure: bool,
    seen: Arc<Mutex<HashSet<i64>>>,
    index_tx: Option<mpsc::Sender<IndexJob>>,
}

struct IndexJob {
    id: ObjectId,
    seq: i64,
}

impl AppState {
    pub fn new(store: Store, identities: Vec<Identity>, search: Option<Typesense>) -> Self {
        let (live, _) = broadcast::channel(1024);
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
            seen: Arc::new(Mutex::new(HashSet::new())),
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

    /// Broadcasts one change and delivers webhooks. A repeated `seq` is ignored
    /// so a local write and the Kafka consumer do not notify twice.
    pub fn emit(&self, body: Value) {
        if let Some(seq) = body.get("seq").and_then(Value::as_i64) {
            let mut seen = self
                .seen
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if !seen.insert(seq) {
                return;
            }
            if seen.len() > 10_000 {
                seen.clear();
                seen.insert(seq);
            }
        }
        let _ = self.live.send(body.clone());
        let state = self.clone();
        tokio::spawn(async move {
            crate::platform::deliver_webhooks(&state, &body).await;
        });
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
