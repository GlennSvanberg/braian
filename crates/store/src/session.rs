use std::sync::Arc;
use std::time::Duration;

use deadpool_postgres::{Manager, Pool};
use tokio::sync::RwLock;
use tokio_postgres::NoTls;

use crate::error::Error;
use crate::split::StorageForm;
use crate::types::CachedDefs;

use refinery::embed_migrations;
embed_migrations!("migrations");

/// Which connection pool a cloned [`Store`] checks out from.
///
/// Interactive reads and edits use [`DbLane::Interactive`]. Bulk import,
/// bulk jobs and the indexer use [`DbLane::Bulk`], whose pool is capped so a
/// large COPY cannot take every connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbLane {
    Interactive,
    Bulk,
}

/// Pool sizes and the pause between bulk batches.
///
/// [`PoolConfig::from_env`] is what `braian` uses. Tests and the benches call
/// [`PoolConfig::default`] through [`Store::connect_config`], so a developer
/// shell cannot change their pool sizes by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
    pub interactive: usize,
    pub bulk: usize,
    /// Sleep after each bulk batch, on top of a scheduler yield.
    /// Zero still yields the task.
    pub bulk_yield: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            interactive: 24,
            bulk: 4,
            bulk_yield: Duration::from_millis(1),
        }
    }
}

impl PoolConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            interactive: positive_env("BRAIAN_POOL_INTERACTIVE", defaults.interactive),
            bulk: positive_env("BRAIAN_POOL_BULK", defaults.bulk),
            bulk_yield: duration_env("BRAIAN_BULK_YIELD_MS", defaults.bulk_yield),
        }
    }
}

fn positive_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

fn duration_env(name: &str, default: Duration) -> Duration {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map(Duration::from_millis)
            .unwrap_or(default),
        Err(_) => default,
    }
}

#[derive(Clone)]
pub struct Store {
    interactive: Pool,
    bulk: Pool,
    lane: DbLane,
    bulk_yield: Duration,
    form: StorageForm,
    definitions: Arc<RwLock<Option<Arc<CachedDefs>>>>,
}

/// One checked-out connection. Drop it to return the connection to the pool.
pub struct HeldConnection {
    _connection: deadpool_postgres::Object,
}

impl Store {
    pub async fn connect(database_url: &str) -> Result<Self, Error> {
        let config = parse_config(database_url)?;
        Self::connect_with(config, crate::STORAGE_FORM, PoolConfig::from_env()).await
    }

    pub async fn connect_form(database_url: &str, form: StorageForm) -> Result<Self, Error> {
        let config = parse_config(database_url)?;
        Self::connect_with(config, form, PoolConfig::from_env()).await
    }

    pub async fn connect_config(
        config: tokio_postgres::Config,
        form: StorageForm,
    ) -> Result<Self, Error> {
        Self::connect_with(config, form, PoolConfig::default()).await
    }

    pub async fn connect_with(
        config: tokio_postgres::Config,
        form: StorageForm,
        pools: PoolConfig,
    ) -> Result<Self, Error> {
        let migrate_with = config.clone();
        let (mut client, connection) = migrate_with.connect(NoTls).await?;
        let handle = tokio::spawn(async move {
            let _ = connection.await;
        });
        migrations::runner().run_async(&mut client).await?;
        client
            .batch_execute("SELECT ensure_change_partitions(3)")
            .await?;
        crate::ops::backfill_attribute_parts(&mut client).await?;
        drop(client);
        handle.abort();

        let interactive = Pool::builder(Manager::new(config.clone(), NoTls))
            .max_size(pools.interactive.max(1))
            .build()?;
        let bulk = Pool::builder(Manager::new(config, NoTls))
            .max_size(pools.bulk.max(1))
            .build()?;
        Ok(Self {
            interactive,
            bulk,
            lane: DbLane::Interactive,
            bulk_yield: pools.bulk_yield,
            form,
            definitions: Arc::new(RwLock::new(None)),
        })
    }

    /// Same store, checking out connections from the capped bulk pool.
    pub fn on_bulk(&self) -> Self {
        let mut store = self.clone();
        store.lane = DbLane::Bulk;
        store
    }

    pub fn lane(&self) -> DbLane {
        self.lane
    }

    /// Connections this lane may hold at once.
    pub fn pool_max(&self) -> usize {
        self.pool().status().max_size
    }

    /// Connections this lane can check out right now.
    pub fn pool_available(&self) -> usize {
        self.pool().status().available
    }

    /// Open connections on this lane, idle or checked out.
    pub fn pool_size(&self) -> usize {
        self.pool().status().size
    }

    /// Checks out one connection on this store's lane.
    pub async fn hold_connection(&self) -> Result<HeldConnection, Error> {
        Ok(HeldConnection {
            _connection: self.pool().get().await?,
        })
    }

    /// Gives the scheduler a turn after a bulk batch, then waits
    /// [`PoolConfig::bulk_yield`] when this store is on the bulk lane.
    ///
    /// Interactive stores return immediately. A bulk COPY that never yields
    /// keeps the runtime on the import task while a GET sits in the queue.
    pub async fn yield_between_bulk_batches(&self) {
        if self.lane != DbLane::Bulk {
            return;
        }
        tokio::task::yield_now().await;
        if !self.bulk_yield.is_zero() {
            tokio::time::sleep(self.bulk_yield).await;
        }
    }

    pub fn form(&self) -> StorageForm {
        self.form
    }

    pub(crate) async fn defs(&self) -> Result<Arc<CachedDefs>, Error> {
        if let Some(cached) = self.definitions.read().await.clone() {
            return Ok(cached);
        }
        self.refresh_defs().await
    }

    pub(crate) async fn refresh_defs(&self) -> Result<Arc<CachedDefs>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1",
                &[],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::NoDefinitions);
        };
        let revision: i64 = row.get(0);
        let document: serde_json::Value = row.get(1);
        let set: braian_model::DefinitionSet = serde_json::from_value(document)
            .map_err(|error| Error::Definitions(error.to_string()))?;
        if i64::try_from(set.model_revision).ok() != Some(revision) {
            return Err(Error::Definitions(
                "stored revision does not match the document".into(),
            ));
        }
        let group_rows = client
            .query("SELECT attribute_id, part_group FROM attribute_parts", &[])
            .await?;
        let mut groups = std::collections::HashMap::new();
        for row in group_rows {
            let group: i32 = row.get("part_group");
            groups.insert(
                braian_model::AttributeId::from_uuid(row.get("attribute_id")),
                usize::try_from(group).unwrap_or(0),
            );
        }
        let groups = crate::split::SplitContext::assign_groups(&set, &groups);
        let cached = Arc::new(CachedDefs::from_groups(set, groups));
        *self.definitions.write().await = Some(Arc::clone(&cached));
        Ok(cached)
    }

    pub(crate) async fn remember(&self, cached: Arc<CachedDefs>) {
        *self.definitions.write().await = Some(cached);
    }

    pub(crate) fn pool(&self) -> &Pool {
        match self.lane {
            DbLane::Interactive => &self.interactive,
            DbLane::Bulk => &self.bulk,
        }
    }
}

pub fn parse_config(database_url: &str) -> Result<tokio_postgres::Config, Error> {
    database_url
        .parse::<tokio_postgres::Config>()
        .map_err(|error| Error::Url(error.to_string()))
}

pub struct PgConn {
    pub client: tokio_postgres::Client,
}

impl PgConn {
    pub async fn connect(config: tokio_postgres::Config) -> Result<Self, Error> {
        let (client, connection) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Self { client })
    }
}

pub async fn create_database(client: &tokio_postgres::Client, name: &str) -> Result<(), Error> {
    let name = crate::ids::plain_name(name)?;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await?;
    Ok(())
}

pub async fn drop_database(client: &tokio_postgres::Client, name: &str) -> Result<(), Error> {
    let name = crate::ids::plain_name(name)?;
    client
        .batch_execute(&format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{name}' AND pid <> pg_backend_pid()"
        ))
        .await?;
    let _ = client
        .batch_execute(&format!(
            "SELECT pg_terminate_backend(active_pid) FROM pg_replication_slots \
             WHERE database = '{name}' AND active_pid IS NOT NULL"
        ))
        .await;
    let slots = client
        .query(
            "SELECT slot_name, active FROM pg_replication_slots WHERE database = $1",
            &[&name],
        )
        .await?;
    for slot in slots {
        let slot_name: String = slot.get(0);
        let active: bool = slot.get(1);
        if active || crate::ids::plain_name(&slot_name).is_err() {
            continue;
        }
        let _ = client
            .batch_execute(&format!("SELECT pg_drop_replication_slot('{slot_name}')"))
            .await;
    }
    let _ = client.batch_execute("SET statement_timeout = '3s'").await;
    client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name}"))
        .await?;
    Ok(())
}
