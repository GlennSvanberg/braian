use std::sync::Arc;
use std::time::Duration;

use deadpool_postgres::{Manager, Pool};
use tokio::sync::RwLock;
use tokio_postgres::NoTls;

use crate::error::Error;
use crate::gauge::{self, Gauge};
use crate::pressure::{self, InteractiveGuard, InteractiveLoad, YieldLimits};
use crate::split::StorageForm;
use crate::types::CachedDefs;

use refinery::embed_migrations;
embed_migrations!("migrations");

/// Which connection pool a cloned [`Store`] checks out from.
///
/// Interactive reads and edits use [`DbLane::Interactive`]. Bulk import and
/// bulk jobs use [`DbLane::Bulk`], whose pool is capped so a large COPY cannot
/// take every connection. The indexer's interactive lane uses [`DbLane::Indexer`],
/// a small pool that a bulk fetch does not check out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbLane {
    Interactive,
    Bulk,
    Indexer,
}

/// Pool sizes and the pause between bulk batches.
///
/// [`PoolConfig::from_env`] is what `braian` uses. Tests and the benches call
/// [`PoolConfig::default`] through [`Store::connect_config`], so a developer
/// shell cannot change their pool sizes by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
    pub interactive: usize,
    /// Connections for import, bulk jobs and the indexer's bulk lane.
    ///
    /// Eight matches the bench's default loader count without taking the
    /// interactive pool. `BRAIAN_POOL_BULK=12` is the quiet-host size for an
    /// import that should use more of the machine; the interactive gauge is
    /// what keeps that from sitting on a click.
    pub bulk: usize,
    /// Connections for the indexer's interactive lane.
    ///
    /// Two is enough for one object read plus the seq update, and it stays off
    /// [`Self::bulk`] so a cold 2 000-object fetch cannot queue an edit.
    /// `BRAIAN_INDEXER_POOL` overrides it.
    pub indexer: usize,
    /// Sleep after each idle bulk slice, on top of a scheduler yield.
    /// Zero still yields the task.
    pub bulk_yield: Duration,
    /// Added to the pause for each in-flight interactive request.
    pub bulk_yield_step: Duration,
    /// Upper bound on the pause while interactive requests are running.
    /// The import continues after this, so a click stream cannot park it.
    pub bulk_yield_max: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            interactive: 24,
            bulk: 8,
            indexer: 2,
            bulk_yield: Duration::from_millis(1),
            bulk_yield_step: Duration::from_millis(4),
            bulk_yield_max: Duration::from_millis(25),
        }
    }
}

impl PoolConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            interactive: positive_env("BRAIAN_POOL_INTERACTIVE", defaults.interactive),
            bulk: positive_env("BRAIAN_POOL_BULK", defaults.bulk),
            indexer: positive_env("BRAIAN_INDEXER_POOL", defaults.indexer),
            bulk_yield: duration_env("BRAIAN_BULK_YIELD_MS", defaults.bulk_yield),
            bulk_yield_step: duration_env("BRAIAN_BULK_YIELD_STEP_MS", defaults.bulk_yield_step),
            bulk_yield_max: duration_env("BRAIAN_BULK_YIELD_MAX_MS", defaults.bulk_yield_max),
        }
    }

    fn yield_limits(self) -> YieldLimits {
        YieldLimits {
            base: self.bulk_yield,
            step: self.bulk_yield_step,
            max_pause: self.bulk_yield_max,
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
    indexer: Pool,
    lane: DbLane,
    yield_limits: YieldLimits,
    load: InteractiveLoad,
    /// Keeps the pressure publisher running until the last store clone is dropped.
    #[expect(dead_code)]
    gauge: Gauge,
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

        let load = InteractiveLoad::new();
        let gauge = gauge::start(config.clone(), load.clone());
        let interactive = Pool::builder(Manager::new(config.clone(), NoTls))
            .max_size(pools.interactive.max(1))
            .build()?;
        let indexer = Pool::builder(Manager::new(config.clone(), NoTls))
            .max_size(pools.indexer.max(1))
            .build()?;
        let bulk = Pool::builder(Manager::new(config, NoTls))
            .max_size(pools.bulk.max(1))
            .build()?;
        Ok(Self {
            interactive,
            indexer,
            bulk,
            lane: DbLane::Interactive,
            yield_limits: pools.yield_limits(),
            load,
            gauge,
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

    /// Same store, checking out connections from the interactive pool.
    pub fn on_interactive(&self) -> Self {
        let mut store = self.clone();
        store.lane = DbLane::Interactive;
        store
    }

    /// Same store, checking out connections from the indexer's interactive pool.
    ///
    /// Bulk indexing stays on [`Self::on_bulk`]. The two lanes then do not wait
    /// on each other's Postgres connections.
    pub fn on_indexer(&self) -> Self {
        let mut store = self.clone();
        store.lane = DbLane::Indexer;
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

    /// Counts one in-flight interactive read or write.
    ///
    /// The guard is held for the request. [`Self::on_bulk`] sees the same
    /// counter and pauses the next chunk while it is above zero.
    pub fn enter_interactive(&self) -> InteractiveGuard {
        self.load.enter()
    }

    /// How many interactive reads and writes are inside this process.
    pub fn interactive_inflight(&self) -> usize {
        self.load.inflight()
    }

    /// Interactive requests this process should yield for.
    ///
    /// The count is this process plus fresh rows other processes have
    /// published to `interactive_pressure`. A crashed process drops out
    /// after a few hundred milliseconds.
    pub fn interactive_pressure(&self) -> usize {
        self.load.total()
    }

    /// Rows to take from a remaining bulk slice.
    ///
    /// Interactive stores return `remaining`. Bulk stores shrink it while
    /// [`Self::interactive_pressure`] is above zero.
    pub(crate) fn bulk_take(&self, remaining: usize) -> usize {
        if remaining == 0 || self.lane != DbLane::Bulk {
            return remaining;
        }
        pressure::bulk_pace(self.load.total(), remaining, self.yield_limits)
            .chunk
            .min(remaining)
            .max(1)
    }

    /// Gives the scheduler a turn before the next bulk slice, then waits.
    ///
    /// Interactive stores return immediately. With interactive requests in
    /// flight, here or in another process, the wait grows and stops at
    /// [`PoolConfig::bulk_yield_max`], so the import continues. An idle bulk
    /// lane waits [`PoolConfig::bulk_yield`].
    pub async fn yield_between_bulk_batches(&self) {
        if self.lane != DbLane::Bulk {
            return;
        }
        pressure::pause_for_bulk(&self.load, self.yield_limits).await;
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
            DbLane::Indexer => &self.indexer,
        }
    }

    /// The interactive pool, ignoring this store's lane.
    ///
    /// List pages use this so a bulk COPY cannot queue them.
    pub(crate) fn interactive_pool(&self) -> &Pool {
        &self.interactive
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

#[cfg(test)]
mod tests {
    use super::PoolConfig;

    #[test]
    fn the_indexer_pool_is_small_and_separate_from_bulk() {
        let pools = PoolConfig::default();
        assert_eq!(pools.indexer, 2);
        assert!(pools.indexer < pools.bulk);
        assert!(pools.bulk < pools.interactive);
    }
}
