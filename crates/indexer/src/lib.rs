//! Builds the search index from change events.
//!
//! Interactive events are drained before bulk events. Events for the same object
//! collapse over a short window, and the document is the current Postgres row,
//! so a duplicate or a late event writes the same document.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use braian_events::{BULK_TOPIC, ChangeEvent, INTERACTIVE_TOPIC, Subscription};
use braian_model::ObjectId;
use braian_rules::CompiledModel;
use braian_search::{Typesense, document};
use braian_store::Store;
use serde_json::Value;
use uuid::Uuid;

const WINDOW: Duration = Duration::from_millis(100);
const BATCH: usize = 100;

pub struct Indexer {
    store: Store,
    search: Typesense,
    interactive: Subscription,
    bulk: Subscription,
}

impl Indexer {
    pub fn new(
        store: Store,
        search: Typesense,
        brokers: &str,
        group: &str,
    ) -> anyhow::Result<Self> {
        let interactive = Subscription::subscribe(
            brokers,
            &format!("{group}-interactive"),
            &[INTERACTIVE_TOPIC],
        )?;
        let bulk = Subscription::subscribe(brokers, &format!("{group}-bulk"), &[BULK_TOPIC])?;
        Ok(Self {
            store,
            search,
            interactive,
            bulk,
        })
    }

    pub async fn run(self, shutdown: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
        let mut pending: HashMap<String, ChangeEvent> = HashMap::new();
        let mut since = Instant::now();
        loop {
            if *shutdown.borrow() {
                if !pending.is_empty() {
                    self.flush(&pending).await?;
                    self.interactive.commit()?;
                    self.bulk.commit()?;
                }
                return Ok(());
            }
            let mut drained = false;
            while let Some(event) = poll(&self.interactive, Duration::from_millis(5)).await? {
                pending.insert(event.key(), event);
                drained = true;
                if pending.len() >= BATCH {
                    break;
                }
            }
            if !drained
                && pending.len() < BATCH
                && let Some(event) = poll(&self.bulk, Duration::from_millis(40)).await?
            {
                pending.insert(event.key(), event);
            }
            let due = !pending.is_empty() && (pending.len() >= BATCH || since.elapsed() >= WINDOW);
            if due {
                self.flush(&pending).await?;
                self.interactive.commit()?;
                self.bulk.commit()?;
                pending.clear();
                since = Instant::now();
            } else if pending.is_empty() {
                since = Instant::now();
            }
        }
    }

    async fn flush(&self, pending: &HashMap<String, ChangeEvent>) -> anyhow::Result<()> {
        let defs = match self.store.definition_set().await {
            Ok(defs) => defs,
            Err(braian_store::Error::NoDefinitions) => {
                let seq = pending.values().map(|event| event.seq).max().unwrap_or(0);
                self.store.set_indexer_seq(seq).await?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let model = CompiledModel::compile(&defs)
            .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
        let collection = self.store.indexer_collection().await?;
        if collection.is_empty() {
            let name = format!("braian_{}", Uuid::now_v7().simple());
            self.search.create_collection(&name, &defs).await?;
            self.search.upsert_alias(&name).await?;
            self.store.set_indexer_collection(&name).await?;
        }
        let mut docs = Vec::new();
        let mut max_seq = 0i64;
        for event in pending.values() {
            max_seq = max_seq.max(event.seq);
            let Some(object_id) = event.object_id else {
                continue;
            };
            let Some(stored) = self
                .store
                .get_object(ObjectId::from_uuid(object_id))
                .await?
            else {
                continue;
            };
            let type_key = defs
                .types
                .iter()
                .find(|object_type| object_type.id == stored.object.type_id)
                .map(|object_type| object_type.key.clone())
                .unwrap_or_default();
            docs.push(document(
                &model,
                &defs,
                &stored.object,
                &stored.key,
                &type_key,
                stored.version,
                event.seq,
                stored.deleted_at.is_some(),
            ));
        }
        self.search.import(self.search.alias(), &docs).await?;
        self.store.set_indexer_seq(max_seq).await?;
        Ok(())
    }
}

async fn poll(subscription: &Subscription, wait: Duration) -> anyhow::Result<Option<ChangeEvent>> {
    match tokio::time::timeout(wait, subscription.next()).await {
        Ok(Ok(event)) => Ok(Some(event)),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Ok(None),
    }
}

/// Rebuilds a new collection from Postgres and points the alias at it.
pub async fn reindex(store: &Store, search: &Typesense) -> anyhow::Result<i64> {
    let defs = store.definition_set().await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
    let name = format!("braian_{}", Uuid::now_v7().simple());
    let previous = search.alias_target().await?;
    search.create_collection(&name, &defs).await?;
    let mut after: Option<Uuid> = None;
    let max_seq = store.max_change_seq().await?;
    loop {
        let ids = store.object_ids_after(after, 200).await?;
        if ids.is_empty() {
            break;
        }
        let mut docs = Vec::with_capacity(ids.len());
        for id in &ids {
            let Some(stored) = store.get_object(*id).await? else {
                continue;
            };
            if stored.deleted_at.is_some() {
                continue;
            }
            let type_key = defs
                .types
                .iter()
                .find(|object_type| object_type.id == stored.object.type_id)
                .map(|object_type| object_type.key.clone())
                .unwrap_or_default();
            docs.push(document(
                &model,
                &defs,
                &stored.object,
                &stored.key,
                &type_key,
                stored.version,
                max_seq,
                false,
            ));
        }
        search.import(&name, &docs).await?;
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

/// Used by tests that want the indexed document for one object without Kafka.
pub async fn index_object(
    store: &Store,
    search: &Typesense,
    id: ObjectId,
    seq: i64,
) -> anyhow::Result<Value> {
    let defs = store.definition_set().await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
    if search.alias_target().await?.is_none() {
        let name = format!("braian_{}", Uuid::now_v7().simple());
        search.create_collection(&name, &defs).await?;
        search.upsert_alias(&name).await?;
        store.set_indexer_collection(&name).await?;
    }
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
    let doc = document(
        &model,
        &defs,
        &stored.object,
        &stored.key,
        &type_key,
        stored.version,
        seq,
        stored.deleted_at.is_some(),
    );
    search
        .import(search.alias(), std::slice::from_ref(&doc))
        .await?;
    store.set_indexer_seq(seq).await?;
    Ok(doc)
}
