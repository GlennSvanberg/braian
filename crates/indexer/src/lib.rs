//! Builds the search index from change events.
//!
//! Interactive events are drained before bulk events. Duplicate ids in one batch
//! collapse to the highest sequence, and the document is the current Postgres
//! row, so a duplicate or a late event writes the same document. Objects are
//! loaded with one query per batch, then imported to Typesense in parallel chunks.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use braian_events::{BULK_TOPIC, ChangeEvent, INTERACTIVE_TOPIC, Subscription};
use braian_model::ObjectId;
use braian_rules::CompiledModel;
use braian_search::{Typesense, document};
use braian_store::{Store, StoredObject};
use futures::stream::{StreamExt, TryStreamExt};
use serde_json::Value;
use uuid::Uuid;

const WINDOW: Duration = Duration::from_millis(100);

/// How the indexer batches Postgres reads and Typesense imports.
#[derive(Debug, Clone, Copy)]
pub struct IndexerConfig {
    /// Events coalesced before one flush.
    pub batch_size: usize,
    /// Documents per Typesense `documents/import` call.
    pub import_batch: usize,
    /// Import calls in flight.
    pub import_concurrency: usize,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            batch_size: 200,
            import_batch: 100,
            import_concurrency: 4,
        }
    }
}

impl IndexerConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            batch_size: positive_env("BRAIAN_INDEXER_BATCH", defaults.batch_size),
            import_batch: positive_env("BRAIAN_INDEXER_IMPORT_BATCH", defaults.import_batch),
            import_concurrency: positive_env(
                "BRAIAN_INDEXER_IMPORT_CONCURRENCY",
                defaults.import_concurrency,
            ),
        }
    }
}

pub struct Indexer {
    store: Store,
    search: Typesense,
    interactive: Subscription,
    bulk: Subscription,
    config: IndexerConfig,
}

impl Indexer {
    pub fn new(
        store: Store,
        search: Typesense,
        brokers: &str,
        group: &str,
    ) -> anyhow::Result<Self> {
        Self::with_config(store, search, brokers, group, IndexerConfig::from_env())
    }

    pub fn with_config(
        store: Store,
        search: Typesense,
        brokers: &str,
        group: &str,
        config: IndexerConfig,
    ) -> anyhow::Result<Self> {
        let interactive = Subscription::subscribe(
            brokers,
            &format!("{group}-interactive"),
            &[INTERACTIVE_TOPIC],
        )?;
        let bulk = Subscription::subscribe(brokers, &format!("{group}-bulk"), &[BULK_TOPIC])?;
        tracing::info!(
            batch = config.batch_size,
            import_batch = config.import_batch,
            import_concurrency = config.import_concurrency,
            "indexer batching"
        );
        Ok(Self {
            store,
            search,
            interactive,
            bulk,
            config,
        })
    }

    pub async fn run(self, shutdown: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
        let mut pending: HashMap<String, ChangeEvent> = HashMap::new();
        let mut since = Instant::now();
        let mut checked = Instant::now()
            .checked_sub(Duration::from_secs(2))
            .unwrap_or_else(Instant::now);
        loop {
            if *shutdown.borrow() {
                if !pending.is_empty() {
                    self.flush(&pending).await?;
                    self.commit_both()?;
                }
                return Ok(());
            }
            if checked.elapsed() >= Duration::from_secs(1) {
                self.rebuild_if_requested().await?;
                checked = Instant::now();
            }
            let mut interactive_drained = false;
            while pending.len() < self.config.batch_size {
                let wait = if interactive_drained {
                    Duration::ZERO
                } else {
                    Duration::from_millis(5)
                };
                let Some(event) = poll(&self.interactive, wait).await? else {
                    break;
                };
                remember(&mut pending, event);
                interactive_drained = true;
            }
            if read_bulk_this_turn(pending.len(), self.config.batch_size, interactive_drained) {
                let mut first = true;
                while pending.len() < self.config.batch_size {
                    let wait = if first && pending.is_empty() {
                        Duration::from_millis(40)
                    } else if first {
                        Duration::from_millis(5)
                    } else {
                        Duration::ZERO
                    };
                    first = false;
                    let Some(event) = poll(&self.bulk, wait).await? else {
                        break;
                    };
                    remember(&mut pending, event);
                }
            }
            let due = !pending.is_empty()
                && (pending.len() >= self.config.batch_size || since.elapsed() >= WINDOW);
            if due {
                self.flush(&pending).await?;
                self.commit_both()?;
                pending.clear();
                since = Instant::now();
            } else if pending.is_empty() {
                since = Instant::now();
            }
        }
    }

    fn commit_both(&self) -> anyhow::Result<()> {
        self.interactive.commit()?;
        self.bulk.commit()?;
        Ok(())
    }

    async fn rebuild_if_requested(&self) -> anyhow::Result<()> {
        if !self.store.take_reindex_request().await? {
            return Ok(());
        }
        tracing::warn!(
            "replication slot recovery requested a reindex; rebuilding the search alias from Postgres"
        );
        if let Err(error) = reindex_with(&self.store, &self.search, self.config).await {
            let _ = self.store.request_reindex().await;
            return Err(error);
        }
        Ok(())
    }

    async fn flush(&self, pending: &HashMap<String, ChangeEvent>) -> anyhow::Result<()> {
        let max_seq = pending.values().map(|event| event.seq).max().unwrap_or(0);
        let defs = match self.store.definition_set().await {
            Ok(defs) => defs,
            Err(braian_store::Error::NoDefinitions) => {
                self.store.set_indexer_seq(max_seq).await?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let model = CompiledModel::compile(&defs)
            .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
        ensure_alias(&self.store, &self.search, &defs).await?;
        let ids = object_ids(pending.values().cloned());
        let stored = self.store.get_objects(&ids).await?;
        let docs = documents(&model, &defs, &stored, pending);
        import_batched(
            &self.search,
            self.search.alias(),
            &docs,
            self.config.import_batch,
            self.config.import_concurrency,
        )
        .await?;
        self.store.set_indexer_seq(max_seq).await?;
        Ok(())
    }
}

/// Bulk is read only when this turn did not already take an interactive event.
fn read_bulk_this_turn(pending: usize, batch: usize, interactive_drained: bool) -> bool {
    !interactive_drained && pending < batch
}

fn remember(pending: &mut HashMap<String, ChangeEvent>, event: ChangeEvent) {
    let key = event.key();
    if pending
        .get(&key)
        .is_some_and(|existing| existing.seq > event.seq)
    {
        return;
    }
    pending.insert(key, event);
}

fn object_ids(events: impl IntoIterator<Item = ChangeEvent>) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        let Some(id) = event.object_id else {
            continue;
        };
        if seen.insert(id) {
            ids.push(ObjectId::from_uuid(id));
        }
    }
    ids
}

fn documents(
    model: &CompiledModel,
    defs: &braian_model::DefinitionSet,
    stored: &[StoredObject],
    pending: &HashMap<String, ChangeEvent>,
) -> Vec<Value> {
    let type_keys: HashMap<_, _> = defs
        .types
        .iter()
        .map(|object_type| (object_type.id, object_type.key.as_str()))
        .collect();
    let mut docs = Vec::with_capacity(stored.len());
    for row in stored {
        let key = row.object.id.as_uuid().to_string();
        let seq = pending.get(&key).map(|event| event.seq).unwrap_or(0);
        let type_key = type_keys.get(&row.object.type_id).copied().unwrap_or("");
        docs.push(document(
            model,
            defs,
            &row.object,
            &row.key,
            type_key,
            row.version,
            seq,
            row.deleted_at.is_some(),
        ));
    }
    docs
}

async fn import_batched(
    search: &Typesense,
    collection: &str,
    docs: &[Value],
    batch: usize,
    concurrency: usize,
) -> anyhow::Result<()> {
    if docs.is_empty() {
        return Ok(());
    }
    let batch = batch.max(1);
    let concurrency = concurrency.max(1);
    // Own each chunk. The import futures then stay independent of the caller's
    // buffer, which is what `Indexer::run` needs when it is spawned.
    let jobs: Vec<Vec<Value>> = docs.chunks(batch).map(<[Value]>::to_vec).collect();
    let search = search.clone();
    let collection = collection.to_owned();
    futures::stream::iter(jobs)
        .map(move |chunk| {
            let search = search.clone();
            let collection = collection.clone();
            async move { search.import(&collection, &chunk).await }
        })
        .buffer_unordered(concurrency)
        .try_collect::<Vec<()>>()
        .await
        .map_err(|error| anyhow::anyhow!("typesense import: {error}"))?;
    Ok(())
}

async fn ensure_alias(
    store: &Store,
    search: &Typesense,
    defs: &braian_model::DefinitionSet,
) -> anyhow::Result<()> {
    let collection = store.indexer_collection().await?;
    if collection.is_empty() {
        let name = format!("braian_{}", Uuid::now_v7().simple());
        search.create_collection(&name, defs).await?;
        search.upsert_alias(&name).await?;
        store.set_indexer_collection(&name).await?;
    }
    Ok(())
}

async fn poll(subscription: &Subscription, wait: Duration) -> anyhow::Result<Option<ChangeEvent>> {
    match tokio::time::timeout(wait, subscription.next()).await {
        Ok(Ok(event)) => Ok(Some(event)),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Ok(None),
    }
}

fn positive_env(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(value) => value.parse().ok().filter(|parsed: &usize| *parsed > 0),
        Err(_) => None,
    }
    .unwrap_or(default)
}

/// Rebuilds a new collection from Postgres and points the alias at it.
pub async fn reindex(store: &Store, search: &Typesense) -> anyhow::Result<i64> {
    reindex_with(store, search, IndexerConfig::from_env()).await
}

async fn reindex_with(
    store: &Store,
    search: &Typesense,
    config: IndexerConfig,
) -> anyhow::Result<i64> {
    let defs = store.definition_set().await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
    let name = format!("braian_{}", Uuid::now_v7().simple());
    let previous = search.alias_target().await?;
    search.create_collection(&name, &defs).await?;
    let mut after: Option<Uuid> = None;
    let max_seq = store.max_change_seq().await?;
    let page = i64::try_from(config.import_batch.max(1) * 2).unwrap_or(200);
    loop {
        let ids = store.object_ids_after(after, page).await?;
        if ids.is_empty() {
            break;
        }
        let stored = store.get_objects(&ids).await?;
        let type_keys: HashMap<_, _> = defs
            .types
            .iter()
            .map(|object_type| (object_type.id, object_type.key.as_str()))
            .collect();
        let docs: Vec<Value> = stored
            .iter()
            .filter(|row| row.deleted_at.is_none())
            .map(|row| {
                let type_key = type_keys.get(&row.object.type_id).copied().unwrap_or("");
                document(
                    &model,
                    &defs,
                    &row.object,
                    &row.key,
                    type_key,
                    row.version,
                    max_seq,
                    false,
                )
            })
            .collect();
        import_batched(
            search,
            &name,
            &docs,
            config.import_batch,
            config.import_concurrency,
        )
        .await?;
        after = ids.last().map(|id| id.as_uuid());
    }
    search.upsert_alias(&name).await?;
    if let Some(previous) = previous
        && previous != name
    {
        search.drop_collection(&previous).await?;
    }
    store.set_indexer_collection(&name).await?;
    store.set_indexer_seq(max_seq).await?;
    Ok(max_seq)
}

/// Indexes `ids` in one Postgres read and one Typesense import plan.
///
/// Duplicate ids are read once. Tests use this without Kafka.
pub async fn index_objects(
    store: &Store,
    search: &Typesense,
    ids: &[ObjectId],
    seq: i64,
) -> anyhow::Result<()> {
    let defs = store.definition_set().await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
    ensure_alias(store, search, &defs).await?;
    let unique = object_ids(ids.iter().map(|id| ChangeEvent {
        seq,
        op: "upsert".into(),
        object_id: Some(id.as_uuid()),
        version: None,
        batch_id: Uuid::nil(),
        kind: "interactive".into(),
        at: String::new(),
    }));
    let stored = store.get_objects(&unique).await?;
    let mut pending = HashMap::new();
    for id in &unique {
        pending.insert(
            id.as_uuid().to_string(),
            ChangeEvent {
                seq,
                op: "upsert".into(),
                object_id: Some(id.as_uuid()),
                version: None,
                batch_id: Uuid::nil(),
                kind: "interactive".into(),
                at: String::new(),
            },
        );
    }
    let config = IndexerConfig::from_env();
    let docs = documents(&model, &defs, &stored, &pending);
    import_batched(
        search,
        search.alias(),
        &docs,
        config.import_batch,
        config.import_concurrency,
    )
    .await?;
    store.set_indexer_seq(seq).await?;
    Ok(())
}

/// Used by tests that want the indexed document for one object without Kafka.
pub async fn index_object(
    store: &Store,
    search: &Typesense,
    id: ObjectId,
    seq: i64,
) -> anyhow::Result<Value> {
    index_objects(store, search, &[id], seq).await?;
    let defs = store.definition_set().await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
    let stored = store
        .get_object(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("object {id} was not found"))?;
    let type_key = defs
        .types
        .iter()
        .find(|object_type| object_type.id == stored.object.type_id)
        .map(|object_type| object_type.key.clone())
        .unwrap_or_default();
    Ok(document(
        &model,
        &defs,
        &stored.object,
        &stored.key,
        &type_key,
        stored.version,
        seq,
        stored.deleted_at.is_some(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn event(seq: i64, id: Uuid, kind: &str) -> ChangeEvent {
        ChangeEvent {
            seq,
            op: "update".into(),
            object_id: Some(id),
            version: Some(seq),
            batch_id: Uuid::nil(),
            kind: kind.into(),
            at: String::new(),
        }
    }

    #[test]
    fn coalesce_keeps_the_highest_seq_per_object() {
        let id = Uuid::now_v7();
        let other = Uuid::now_v7();
        let mut pending = HashMap::new();
        remember(&mut pending, event(1, id, "bulk"));
        remember(&mut pending, event(4, id, "interactive"));
        remember(&mut pending, event(3, id, "bulk"));
        remember(&mut pending, event(2, other, "bulk"));
        assert_eq!(pending.len(), 2);
        assert_eq!(pending.get(&id.to_string()).expect("id").seq, 4);
        assert_eq!(pending.get(&other.to_string()).expect("other").seq, 2);
    }

    #[test]
    fn duplicate_ids_are_fetched_once() {
        let id = Uuid::now_v7();
        let ids = object_ids([event(1, id, "bulk"), event(2, id, "interactive")]);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].as_uuid(), id);
    }

    #[test]
    fn interactive_events_block_bulk_in_the_same_turn() {
        assert!(read_bulk_this_turn(0, 200, false));
        assert!(!read_bulk_this_turn(1, 200, true));
        assert!(!read_bulk_this_turn(0, 200, true));
        assert!(!read_bulk_this_turn(200, 200, false));
    }
}
