//! Builds the search index from change events.
//!
//! Interactive and bulk topics flush on separate tasks, so a bulk import does
//! not hold an interactive edit. Duplicate ids in one batch collapse to the
//! highest sequence, and the document is the current Postgres row. The search
//! import refuses an older `seq`, so the two lanes can write at the same time.
//!
//! `BRAIAN_INDEXER_BATCH`, `BRAIAN_INDEXER_IMPORT_BATCH` and
//! `BRAIAN_INDEXER_IMPORT_CONCURRENCY` size the interactive lane. That lane
//! reads Postgres through `BRAIAN_INDEXER_POOL` (default 2) and writes through
//! its own HTTP client, so a bulk fetch cannot hold the connection or the upsert.
//!
//! The bulk lane reads `BRAIAN_INDEXER_BULK_BATCH` events on the bulk pool,
//! builds the documents, and imports them in waves capped at 200 documents
//! and 2 calls. The first reads stay at 200 events until two full batches
//! have finished, then grow toward the configured size. A cold read of 2 000
//! events stalled the interactive lane for about 3 s; the warm-up keeps that
//! first import small. The next Redpanda read and document build run while
//! the current import is in flight, and that next batch's offsets stay
//! uncommitted until its own import finishes. An idle wave starts at 100×1
//! and grows only while the interactive topic stays empty. A second Typesense
//! call is used only when the previous call finished inside 50 ms, so an
//! upsert is not stuck behind a long import. When an edit is already queued,
//! the next wave is one batch of at most 100 and the lane waits up to
//! `BRAIAN_INDEXER_BULK_YIELD_MAX_MS` before importing anyway. A 4×500 import
//! held Typesense for the whole batch, and a wait before the flush did
//! nothing once that import was already in flight.
//!
//! Each lane records a tracing span named `indexer` around `fetch`, `build`
//! and `import`, with `lane`, `events` and `elapsed_ms`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use braian_events::{BULK_TOPIC, ChangeEvent, HeldEvent, INTERACTIVE_TOPIC, Subscription};
use braian_model::ObjectId;
use braian_rules::CompiledModel;
use braian_search::{Typesense, document};
use braian_store::{Store, StoredObject};
use futures::stream::{StreamExt, TryStreamExt};
use serde_json::Value;
use tracing::Instrument;
use uuid::Uuid;

const WINDOW: Duration = Duration::from_millis(100);

/// How the indexer batches Postgres reads and Typesense imports.
#[derive(Debug, Clone, Copy)]
pub struct IndexerConfig {
    /// Interactive events coalesced before one flush.
    pub batch_size: usize,
    /// Documents per interactive Typesense `documents/import` call.
    pub import_batch: usize,
    /// Interactive import calls in flight.
    pub import_concurrency: usize,
    /// Bulk events coalesced before one flush.
    pub bulk_batch_size: usize,
    /// Documents per bulk Typesense `documents/import` call.
    pub bulk_import_batch: usize,
    /// Bulk import calls in flight.
    pub bulk_import_concurrency: usize,
    /// How long the bulk lane waits for the interactive topic to drain.
    pub bulk_yield_max: Duration,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            batch_size: 200,
            import_batch: 100,
            import_concurrency: 4,
            bulk_batch_size: 2_000,
            bulk_import_batch: 200,
            bulk_import_concurrency: 2,
            bulk_yield_max: Duration::from_millis(100),
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
            bulk_batch_size: positive_env("BRAIAN_INDEXER_BULK_BATCH", defaults.bulk_batch_size),
            bulk_import_batch: cap_bulk_batch(positive_env(
                "BRAIAN_INDEXER_BULK_IMPORT_BATCH",
                defaults.bulk_import_batch,
            )),
            bulk_import_concurrency: cap_bulk_concurrency(positive_env(
                "BRAIAN_INDEXER_BULK_IMPORT_CONCURRENCY",
                defaults.bulk_import_concurrency,
            )),
            bulk_yield_max: duration_env(
                "BRAIAN_INDEXER_BULK_YIELD_MAX_MS",
                defaults.bulk_yield_max,
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
            bulk_batch = config.bulk_batch_size,
            bulk_import_batch = config.bulk_import_batch,
            bulk_import_concurrency = config.bulk_import_concurrency,
            bulk_yield_max_ms =
                u64::try_from(config.bulk_yield_max.as_millis()).unwrap_or(u64::MAX),
            indexer_pool = store.on_indexer().pool_max(),
            bulk_pool = store.on_bulk().pool_max(),
            warm_batch = WARM_BATCH,
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
        let shared = Arc::new(Shared {
            interactive: self.store.on_indexer(),
            bulk: self.store.on_bulk(),
            search: self.search,
            config: self.config,
            cache: tokio::sync::Mutex::new(None),
        });
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let drain = Arc::new(InteractiveDrain::new());
        let mut interactive = tokio::spawn(run_lane(
            Arc::clone(&shared),
            self.interactive,
            shutdown.clone(),
            stop_rx.clone(),
            LanePace::for_kind(LaneKind::Interactive, self.config),
            Arc::clone(&drain),
        ));
        let mut bulk = tokio::spawn(run_lane(
            shared,
            self.bulk,
            shutdown,
            stop_rx,
            LanePace::for_kind(LaneKind::Bulk, self.config),
            drain,
        ));
        tokio::select! {
            result = &mut interactive => {
                let _ = stop_tx.send(true);
                let interactive_result = result.map_err(|error| anyhow::anyhow!("interactive lane: {error}"))?;
                let bulk_result = bulk.await.map_err(|error| anyhow::anyhow!("bulk lane: {error}"))?;
                interactive_result?;
                bulk_result?;
            }
            result = &mut bulk => {
                let _ = stop_tx.send(true);
                let bulk_result = result.map_err(|error| anyhow::anyhow!("bulk lane: {error}"))?;
                let interactive_result = interactive.await.map_err(|error| anyhow::anyhow!("interactive lane: {error}"))?;
                bulk_result?;
                interactive_result?;
            }
        }
        Ok(())
    }
}

impl Shared {
    async fn rebuild_if_requested(&self) -> anyhow::Result<()> {
        if !self.bulk.take_reindex_request().await? {
            return Ok(());
        }
        tracing::warn!(
            "replication slot recovery requested a reindex; rebuilding the search alias from Postgres"
        );
        if let Err(error) = reindex_with(&self.bulk, &self.search, self.config).await {
            let _ = self.bulk.request_reindex().await;
            return Err(error);
        }
        Ok(())
    }

    async fn compiled(&self, store: &Store) -> anyhow::Result<Option<Arc<RevisionCache>>> {
        let Some(revision) = store.model_revision().await? else {
            return Ok(None);
        };
        let mut guard = self.cache.lock().await;
        if let Some(cached) = guard.as_ref()
            && reuse_revision(Some(cached.revision), revision)
            && cached.alias_ready
        {
            return Ok(Some(Arc::clone(cached)));
        }
        let defs = match store.definition_set().await {
            Ok(defs) => defs,
            Err(braian_store::Error::NoDefinitions) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let model = CompiledModel::compile(&defs)
            .map_err(|errors| anyhow::anyhow!("definitions: {}", errors[0].message))?;
        ensure_alias(store, &self.search, &defs).await?;
        let cached = Arc::new(RevisionCache {
            revision: defs.model_revision,
            defs: Arc::new(defs),
            model: Arc::new(model),
            alias_ready: true,
        });
        *guard = Some(Arc::clone(&cached));
        Ok(Some(cached))
    }

    async fn flush(
        &self,
        pending: &HashMap<String, ChangeEvent>,
        pace: &LanePace,
    ) -> anyhow::Result<()> {
        let max_seq = pending.values().map(|event| event.seq).max().unwrap_or(0);
        let Some(cached) = self.compiled(&self.interactive).await? else {
            self.interactive.set_indexer_seq(max_seq).await?;
            return Ok(());
        };
        let ids = object_ids(pending.values().cloned());
        let mut build = Stage::start(pace.kind, "build");
        build.arm(ids.len());
        let stored = self
            .interactive
            .get_objects(&ids)
            .instrument(build.span())
            .await?;
        let docs = build
            .span()
            .in_scope(|| documents(&cached.model, &cached.defs, &stored, pending));
        build.arm(docs.len());
        drop(build);
        if !docs.is_empty() {
            let mut import = Stage::start(pace.kind, "import");
            import.arm(docs.len());
            import_batched(
                &self.search.interactive_writer(),
                self.search.alias(),
                &docs,
                pace.import_batch,
                pace.import_concurrency,
            )
            .instrument(import.span())
            .await?;
            drop(import);
        }
        self.interactive.set_indexer_seq(max_seq).await?;
        Ok(())
    }
}

/// A lane flushes on its own batch and clock. The other lane flushing does not
/// delay it; the Typesense seq guard keeps an older import from winning.
pub(crate) fn lane_due(
    pending: usize,
    batch: usize,
    elapsed: Duration,
    other_lane_flushing: bool,
) -> bool {
    let _ = other_lane_flushing;
    pending > 0 && (pending >= batch || elapsed >= WINDOW)
}

struct Shared {
    interactive: Store,
    bulk: Store,
    search: Typesense,
    config: IndexerConfig,
    cache: tokio::sync::Mutex<Option<Arc<RevisionCache>>>,
}

struct RevisionCache {
    revision: u64,
    defs: Arc<braian_model::DefinitionSet>,
    model: Arc<CompiledModel>,
    alias_ready: bool,
}

/// Same revision keeps the compiled model and the search alias.
pub(crate) fn reuse_revision(cached: Option<u64>, current: u64) -> bool {
    cached == Some(current)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaneKind {
    Interactive,
    Bulk,
}

#[derive(Clone, Copy)]
struct LanePace {
    idle: Duration,
    armed: Duration,
    rebuild: bool,
    kind: LaneKind,
    batch_size: usize,
    import_batch: usize,
    import_concurrency: usize,
    bulk_yield_max: Duration,
}

impl LanePace {
    fn for_kind(kind: LaneKind, config: IndexerConfig) -> Self {
        match kind {
            LaneKind::Interactive => Self {
                idle: Duration::from_millis(5),
                armed: Duration::from_millis(5),
                rebuild: false,
                kind,
                batch_size: config.batch_size,
                import_batch: config.import_batch,
                import_concurrency: config.import_concurrency,
                bulk_yield_max: config.bulk_yield_max,
            },
            LaneKind::Bulk => Self {
                idle: Duration::from_millis(40),
                armed: Duration::from_millis(5),
                rebuild: true,
                kind,
                batch_size: config.bulk_batch_size,
                import_batch: config.bulk_import_batch,
                import_concurrency: config.bulk_import_concurrency,
                bulk_yield_max: config.bulk_yield_max,
            },
        }
    }
}

/// Interactive events the bulk lane should let finish before it imports.
struct InteractiveDrain {
    pending: AtomicUsize,
    flushing: AtomicBool,
    notify: tokio::sync::Notify,
}

impl InteractiveDrain {
    fn new() -> Self {
        Self {
            pending: AtomicUsize::new(0),
            flushing: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn set_pending(&self, pending: usize) {
        self.pending.store(pending, Ordering::Release);
        self.publish();
    }

    fn set_flushing(&self, flushing: bool) {
        self.flushing.store(flushing, Ordering::Release);
        self.publish();
    }

    fn publish(&self) {
        if !self.busy() {
            self.notify.notify_waiters();
        }
    }

    fn busy(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0 || self.flushing.load(Ordering::Acquire)
    }

    #[cfg(test)]
    async fn wait_for_idle(&self, max_wait: Duration) {
        let started = Instant::now();
        loop {
            if !bulk_import_waits(
                self.pending.load(Ordering::Acquire),
                self.flushing.load(Ordering::Acquire),
                started.elapsed(),
                max_wait,
            ) {
                return;
            }
            let left = max_wait.saturating_sub(started.elapsed());
            if left.is_zero() {
                return;
            }
            let notified = self.notify.notified();
            tokio::pin!(notified);
            if notified.as_mut().enable() {
                continue;
            }
            if !bulk_import_waits(
                self.pending.load(Ordering::Acquire),
                self.flushing.load(Ordering::Acquire),
                started.elapsed(),
                max_wait,
            ) {
                return;
            }
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep(left) => return,
            }
        }
    }
}

/// Bulk import waits while the interactive lane still has events or is flushing,
/// and only until `max_wait`. After that it imports anyway.
#[cfg(test)]
pub(crate) fn bulk_import_waits(
    interactive_pending: usize,
    interactive_flushing: bool,
    waited: Duration,
    max_wait: Duration,
) -> bool {
    let busy = interactive_pending > 0 || interactive_flushing;
    busy && waited < max_wait
}

/// One Typesense import wave. `batch` is documents per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImportWave {
    pub batch: usize,
    pub concurrency: usize,
}

/// Largest bulk import the indexer will send, even when the env asks for more.
const BULK_BATCH_CAP: usize = 200;
const BULK_CONCURRENCY_CAP: usize = 2;
/// While an interactive edit is already in the indexer, one wave is a single
/// batch of this size.
const BUSY_BATCH_CAP: usize = 100;
/// Pause while HTTP requests are in flight and the interactive topic is empty.
///
/// Long enough for one pressure-gauge tick (15 ms) to publish a commit, short
/// enough that a click stream does not add 100 ms to every wave.
const PRESSURE_GAP: Duration = Duration::from_millis(15);
/// How long one Typesense call may take before a second call would make an
/// interactive upsert wait. v8's backlog regression was a 200×2 import with
/// both threads busy; a call inside this budget keeps that wait near 50 ms.
const IMPORT_BUDGET: Duration = Duration::from_millis(50);

pub(crate) fn cap_bulk_batch(batch: usize) -> usize {
    batch.clamp(1, BULK_BATCH_CAP)
}

pub(crate) fn cap_bulk_concurrency(concurrency: usize) -> usize {
    concurrency.clamp(1, BULK_CONCURRENCY_CAP)
}

/// Why the next bulk wave is this size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveKind {
    /// The interactive topic has events, or that lane is flushing.
    Events,
    /// Interactive HTTP is in flight. The topic itself is idle.
    Pressure,
    /// Nothing interactive is in flight.
    Idle,
}

pub(crate) fn wave_kind(events_pending: bool, pressure: usize) -> WaveKind {
    if events_pending {
        WaveKind::Events
    } else if pressure > 0 {
        WaveKind::Pressure
    } else {
        WaveKind::Idle
    }
}

/// How long to pause before the wave. The pause never exceeds `yield_max`,
/// so `BRAIAN_INDEXER_BULK_YIELD_MAX_MS=0` still imports immediately.
pub(crate) fn wave_gap(kind: WaveKind, yield_max: Duration) -> Duration {
    let gap = match kind {
        WaveKind::Events => yield_max,
        WaveKind::Pressure => PRESSURE_GAP,
        WaveKind::Idle => Duration::ZERO,
    };
    gap.min(yield_max)
}

/// Idle-wave size. Starts at one short call and grows only while the
/// interactive topic stays empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WaveTune {
    batch: usize,
    concurrency: usize,
}

impl WaveTune {
    pub(crate) fn new() -> Self {
        Self {
            batch: BUSY_BATCH_CAP,
            concurrency: 1,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    /// Records one finished wave.
    ///
    /// A partial tail does not retune: a 17-document remainder is fast for the
    /// wrong reason. An edit resets the tune, so the following idle wave is
    /// 100×1 again. Pressure keeps the configured 200×1 and does not grow a
    /// second call. Idle grows the batch while one Typesense thread stays
    /// free, and adds the second call only after a full wave finished inside
    /// [`IMPORT_BUDGET`].
    pub(crate) fn observe(&mut self, kind: WaveKind, docs: usize, elapsed: Duration) {
        if kind == WaveKind::Events {
            self.reset();
            return;
        }
        if kind != WaveKind::Idle || docs == 0 {
            return;
        }
        let planned = self.batch.saturating_mul(self.concurrency).max(1);
        if docs < planned {
            return;
        }
        let over_budget = elapsed > IMPORT_BUDGET;
        if self.concurrency > 1 && over_budget {
            let per_call = docs.div_ceil(self.concurrency);
            self.concurrency = 1;
            self.batch = fit_docs(per_call, elapsed, IMPORT_BUDGET).clamp(1, BUSY_BATCH_CAP);
            return;
        }
        if self.batch < BULK_BATCH_CAP {
            self.batch = grow_batch(self.batch).min(BULK_BATCH_CAP);
            return;
        }
        if !over_budget && self.concurrency < BULK_CONCURRENCY_CAP {
            self.concurrency += 1;
        }
    }
}

fn grow_batch(batch: usize) -> usize {
    batch.saturating_add(batch / 2).max(batch.saturating_add(1))
}

/// Cold bulk reads stay at this many events. The first configured read of
/// 2 000 stalled the interactive lane for about 3 s.
const WARM_BATCH: usize = 200;
/// Full cold reads before the bulk read size starts growing.
const WARMUP_BATCHES: usize = 2;

/// Bulk read size. Cold reads stay at [`WARM_BATCH`]. After [`WARMUP_BATCHES`]
/// full reads the size grows toward the configured batch. A short tail does
/// not count: the lane is warm only once a read filled the cap it was offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatchRamp {
    phase: RampPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RampPhase {
    Warmup { done: usize },
    Ramping { limit: usize },
}

impl BatchRamp {
    pub(crate) fn new() -> Self {
        Self {
            phase: RampPhase::Warmup { done: 0 },
        }
    }

    /// Events to pull on the next bulk read.
    pub(crate) fn batch_size(&self, configured: usize) -> usize {
        let configured = configured.max(1);
        match self.phase {
            RampPhase::Warmup { .. } => WARM_BATCH.min(configured),
            RampPhase::Ramping { limit } => limit.min(configured).max(1),
        }
    }

    /// One finished read. `offered` is the cap that read was built under.
    pub(crate) fn observe(&mut self, configured: usize, events: usize, offered: usize) {
        if offered == 0 || events < offered {
            return;
        }
        let configured = configured.max(1);
        match self.phase {
            RampPhase::Warmup { done } => {
                let done = done.saturating_add(1);
                if done < WARMUP_BATCHES {
                    self.phase = RampPhase::Warmup { done };
                    return;
                }
                self.phase = RampPhase::Ramping {
                    limit: WARM_BATCH.min(configured),
                };
                self.grow(configured);
            }
            RampPhase::Ramping { .. } => self.grow(configured),
        }
    }

    fn grow(&mut self, configured: usize) {
        let RampPhase::Ramping { limit } = self.phase else {
            return;
        };
        if limit >= configured {
            self.phase = RampPhase::Ramping { limit: configured };
            return;
        }
        self.phase = RampPhase::Ramping {
            limit: grow_batch(limit).min(configured),
        };
    }
}

/// Documents that would fill `budget` at the rate of `docs` per `elapsed`.
fn fit_docs(docs: usize, elapsed: Duration, budget: Duration) -> usize {
    if elapsed.is_zero() {
        return docs.max(1);
    }
    let fit = (docs as u128).saturating_mul(budget.as_nanos()) / elapsed.as_nanos().max(1);
    usize::try_from(fit).unwrap_or(docs).max(1)
}

/// The next bulk wave.
///
/// Pressure keeps a single call of the configured size, so one Typesense
/// thread stays free while a bulk COPY is in flight. Events shrink to one
/// short batch, because that edit is about to import. Idle uses `tune`, which
/// starts at 100×1.
pub(crate) fn bulk_import_wave(
    batch: usize,
    concurrency: usize,
    kind: WaveKind,
    tune: &WaveTune,
) -> ImportWave {
    let batch = cap_bulk_batch(batch);
    let concurrency = cap_bulk_concurrency(concurrency);
    match kind {
        WaveKind::Events => ImportWave {
            batch: batch.min(BUSY_BATCH_CAP),
            concurrency: 1,
        },
        WaveKind::Pressure => ImportWave {
            batch,
            concurrency: 1,
        },
        WaveKind::Idle => ImportWave {
            batch: tune.batch.min(batch).max(1),
            concurrency: tune.concurrency.min(concurrency).max(1),
        },
    }
}

pub(crate) fn wave_end(offset: usize, docs: usize, wave: ImportWave) -> usize {
    let span = wave.batch.saturating_mul(wave.concurrency).max(1);
    offset.saturating_add(span).min(docs)
}

struct WavePlan {
    batch: usize,
    concurrency: usize,
    yield_max: Duration,
}

fn current_kind(drain: Option<&InteractiveDrain>, store: &Store) -> WaveKind {
    wave_kind(
        drain.is_some_and(InteractiveDrain::busy),
        store.interactive_pressure(),
    )
}

async fn wait_for_gap(drain: Option<&InteractiveDrain>, store: &Store, max_wait: Duration) {
    let started = Instant::now();
    loop {
        let gap = wave_gap(current_kind(drain, store), max_wait);
        if started.elapsed() >= gap {
            return;
        }
        let left = gap
            .saturating_sub(started.elapsed())
            .min(Duration::from_millis(5));
        if left.is_zero() {
            return;
        }
        tokio::time::sleep(left).await;
    }
}

async fn import_waves(
    search: &Typesense,
    store: &Store,
    drain: Option<&InteractiveDrain>,
    collection: &str,
    docs: &[Value],
    plan: WavePlan,
    tune: &mut WaveTune,
) -> anyhow::Result<()> {
    if docs.is_empty() {
        return Ok(());
    }
    let mut offset = 0;
    while offset < docs.len() {
        let mut kind = current_kind(drain, store);
        if kind == WaveKind::Events {
            tune.reset();
        }
        wait_for_gap(drain, store, plan.yield_max).await;
        kind = current_kind(drain, store);
        if kind == WaveKind::Events {
            tune.reset();
        }
        let wave = bulk_import_wave(plan.batch, plan.concurrency, kind, tune);
        let end = wave_end(offset, docs.len(), wave);
        let started = Instant::now();
        import_batched(
            search,
            collection,
            &docs[offset..end],
            wave.batch,
            wave.concurrency,
        )
        .await?;
        let imported = end - offset;
        let after = current_kind(drain, store);
        if kind == WaveKind::Events || after == WaveKind::Events {
            tune.reset();
        } else {
            tune.observe(kind, imported, started.elapsed());
        }
        offset = end;
    }
    Ok(())
}

async fn run_lane(
    shared: Arc<Shared>,
    subscription: Subscription,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    mut stop: tokio::sync::watch::Receiver<bool>,
    pace: LanePace,
    drain: Arc<InteractiveDrain>,
) -> anyhow::Result<()> {
    if pace.kind == LaneKind::Bulk {
        return run_bulk_lane(shared, subscription, shutdown, stop, pace, drain).await;
    }
    let mut pending: HashMap<String, ChangeEvent> = HashMap::new();
    let mut since = Instant::now();
    let mut checked = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    loop {
        if *shutdown.borrow() || *stop.borrow() {
            commit_pending(&shared, &subscription, &mut pending, &pace, &drain).await?;
            return Ok(());
        }
        if pace.rebuild && checked.elapsed() >= Duration::from_secs(1) {
            shared.rebuild_if_requested().await?;
            checked = Instant::now();
        }
        let mut fetch = Stage::start(pace.kind, "fetch");
        let fetch_span = fetch.span();
        let before = pending.len();
        let read = async {
            let mut first = true;
            while pending.len() < pace.batch_size {
                let wait = if pending.is_empty() && first {
                    pace.idle
                } else if first {
                    pace.armed
                } else {
                    Duration::ZERO
                };
                first = false;
                let Some(event) = poll(&subscription, wait).await? else {
                    break;
                };
                remember(&mut pending, event);
            }
            Ok::<(), anyhow::Error>(())
        }
        .instrument(fetch_span)
        .await;
        let read_now = pending.len().saturating_sub(before);
        if read_now > 0 {
            fetch.arm(read_now);
        }
        drop(fetch);
        read?;
        if pace.kind == LaneKind::Interactive {
            drain.set_pending(pending.len());
        }
        if lane_due(pending.len(), pace.batch_size, since.elapsed(), false) {
            commit_pending(&shared, &subscription, &mut pending, &pace, &drain).await?;
            since = Instant::now();
        } else if pending.is_empty() {
            since = Instant::now();
        } else {
            // The other lane is a separate task. This one only waits out its window.
            let wait = WINDOW.saturating_sub(since.elapsed());
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                result = shutdown.wait_for(|flag| *flag) => { let _ = result; }
                result = stop.wait_for(|flag| *flag) => { let _ = result; }
            }
        }
    }
}

struct BuiltBatch {
    held: Vec<HeldEvent>,
    docs: Vec<Value>,
    max_seq: i64,
    /// Coalesced events in this read.
    events: usize,
    /// Cap the read was built under. A short tail is `events < offered`.
    offered: usize,
}

/// Bulk lane. The next batch is fetched and built while Typesense imports the
/// current one. Offsets for the next batch stay uncommitted, so a crash
/// redelivers it instead of skipping it.
async fn run_bulk_lane(
    shared: Arc<Shared>,
    subscription: Subscription,
    shutdown: tokio::sync::watch::Receiver<bool>,
    stop: tokio::sync::watch::Receiver<bool>,
    pace: LanePace,
    drain: Arc<InteractiveDrain>,
) -> anyhow::Result<()> {
    let mut tune = WaveTune::new();
    let mut ramp = BatchRamp::new();
    let mut prefetched: Option<BuiltBatch> = None;
    let mut checked = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    loop {
        if *shutdown.borrow() || *stop.borrow() {
            if let Some(batch) = prefetched.take() {
                finish_batch(&shared, &subscription, &batch, &drain, &mut tune, &pace).await?;
            }
            return Ok(());
        }
        if pace.rebuild && checked.elapsed() >= Duration::from_secs(1) {
            shared.rebuild_if_requested().await?;
            checked = Instant::now();
        }
        let current = if let Some(batch) = prefetched.take() {
            batch
        } else {
            let Some(batch) = build_batch(
                &shared,
                &subscription,
                shutdown.clone(),
                stop.clone(),
                pace,
                &ramp,
            )
            .await?
            else {
                continue;
            };
            batch
        };
        if *shutdown.borrow() || *stop.borrow() {
            finish_batch(&shared, &subscription, &current, &drain, &mut tune, &pace).await?;
            return Ok(());
        }
        let imported = import_built(&shared, &current, &drain, &mut tune, &pace);
        let fetched = build_batch(
            &shared,
            &subscription,
            shutdown.clone(),
            stop.clone(),
            pace,
            &ramp,
        );
        let (imported, fetched) = tokio::join!(imported, fetched);
        imported?;
        ramp.observe(pace.batch_size, current.events, current.offered);
        subscription.commit_held(&current.held)?;
        prefetched = fetched?;
    }
}

async fn finish_batch(
    shared: &Shared,
    subscription: &Subscription,
    batch: &BuiltBatch,
    drain: &InteractiveDrain,
    tune: &mut WaveTune,
    pace: &LanePace,
) -> anyhow::Result<()> {
    import_built(shared, batch, drain, tune, pace).await?;
    subscription.commit_held(&batch.held)?;
    Ok(())
}

async fn import_built(
    shared: &Shared,
    batch: &BuiltBatch,
    drain: &InteractiveDrain,
    tune: &mut WaveTune,
    pace: &LanePace,
) -> anyhow::Result<()> {
    if !batch.docs.is_empty() {
        let mut import = Stage::start(pace.kind, "import");
        import.arm(batch.docs.len());
        let result = import_waves(
            &shared.search,
            &shared.bulk,
            Some(drain),
            shared.search.alias(),
            &batch.docs,
            WavePlan {
                batch: pace.import_batch,
                concurrency: pace.import_concurrency,
                yield_max: pace.bulk_yield_max,
            },
            tune,
        )
        .instrument(import.span())
        .await;
        drop(import);
        result?;
    }
    shared.bulk.set_indexer_seq(batch.max_seq).await?;
    Ok(())
}

async fn build_batch(
    shared: &Shared,
    subscription: &Subscription,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    mut stop: tokio::sync::watch::Receiver<bool>,
    pace: LanePace,
    ramp: &BatchRamp,
) -> anyhow::Result<Option<BuiltBatch>> {
    let batch_size = ramp.batch_size(pace.batch_size);
    let mut pending: HashMap<String, ChangeEvent> = HashMap::new();
    let mut held = Vec::new();
    let since = Instant::now();
    loop {
        if *shutdown.borrow() || *stop.borrow() {
            break;
        }
        let mut fetch = Stage::start(pace.kind, "fetch");
        let fetch_span = fetch.span();
        let before = pending.len();
        let read = async {
            let mut first = true;
            while pending.len() < batch_size {
                let wait = if pending.is_empty() && first {
                    pace.idle
                } else if first {
                    pace.armed
                } else {
                    Duration::ZERO
                };
                first = false;
                let Some(event) = poll_held(subscription, wait, &mut shutdown, &mut stop).await?
                else {
                    break;
                };
                remember(&mut pending, event.event.clone());
                held.push(event);
            }
            Ok::<(), anyhow::Error>(())
        }
        .instrument(fetch_span)
        .await;
        let read_now = pending.len().saturating_sub(before);
        if read_now > 0 {
            fetch.arm(read_now);
        }
        drop(fetch);
        read?;
        if lane_due(pending.len(), batch_size, since.elapsed(), false) {
            break;
        }
        if pending.is_empty() {
            return Ok(None);
        }
        let wait = WINDOW.saturating_sub(since.elapsed());
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            result = shutdown.wait_for(|flag| *flag) => { let _ = result; }
            result = stop.wait_for(|flag| *flag) => { let _ = result; }
        }
    }
    if pending.is_empty() {
        return Ok(None);
    }
    let events = pending.len();
    let max_seq = pending.values().map(|event| event.seq).max().unwrap_or(0);
    let Some(cached) = shared.compiled(&shared.bulk).await? else {
        return Ok(Some(BuiltBatch {
            held,
            docs: Vec::new(),
            max_seq,
            events,
            offered: batch_size,
        }));
    };
    let ids = object_ids(pending.values().cloned());
    let mut build = Stage::start(pace.kind, "build");
    build.arm(ids.len());
    let stored = shared
        .bulk
        .get_objects(&ids)
        .instrument(build.span())
        .await?;
    let docs = build
        .span()
        .in_scope(|| documents(&cached.model, &cached.defs, &stored, &pending));
    build.arm(docs.len());
    drop(build);
    Ok(Some(BuiltBatch {
        held,
        docs,
        max_seq,
        events,
        offered: batch_size,
    }))
}

async fn poll_held(
    subscription: &Subscription,
    wait: Duration,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Option<HeldEvent>> {
    if *shutdown.borrow() || *stop.borrow() {
        return Ok(None);
    }
    tokio::select! {
        biased;
        result = tokio::time::timeout(wait, subscription.next_held()) => {
            match result {
                Ok(Ok(event)) => Ok(Some(event)),
                Ok(Err(error)) => Err(error.into()),
                Err(_) => Ok(None),
            }
        }
        result = shutdown.wait_for(|flag| *flag) => {
            let _ = result;
            Ok(None)
        }
        result = stop.wait_for(|flag| *flag) => {
            let _ = result;
            Ok(None)
        }
    }
}

async fn commit_pending(
    shared: &Shared,
    subscription: &Subscription,
    pending: &mut HashMap<String, ChangeEvent>,
    pace: &LanePace,
    drain: &InteractiveDrain,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        if pace.kind == LaneKind::Interactive {
            drain.set_pending(0);
            drain.set_flushing(false);
        }
        return Ok(());
    }
    if pace.kind == LaneKind::Interactive {
        drain.set_pending(pending.len());
        drain.set_flushing(true);
    }
    shared.flush(pending, pace).await?;
    subscription.commit()?;
    pending.clear();
    if pace.kind == LaneKind::Interactive {
        drain.set_pending(0);
        drain.set_flushing(false);
    }
    Ok(())
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

fn lane_label(kind: LaneKind) -> &'static str {
    match kind {
        LaneKind::Interactive => "interactive",
        LaneKind::Bulk => "bulk",
    }
}

/// Times one lane stage. Unarmed stages are idle polls and stay quiet.
struct Stage {
    lane: &'static str,
    stage: &'static str,
    span: tracing::Span,
    started: Instant,
    events: usize,
    armed: bool,
}

impl Stage {
    fn start(lane: LaneKind, stage: &'static str) -> Self {
        let label = lane_label(lane);
        Self {
            lane: label,
            stage,
            span: tracing::info_span!(
                "indexer",
                lane = label,
                stage,
                events = tracing::field::Empty,
                elapsed_ms = tracing::field::Empty,
            ),
            started: Instant::now(),
            events: 0,
            armed: false,
        }
    }

    fn span(&self) -> tracing::Span {
        self.span.clone()
    }

    fn arm(&mut self, events: usize) {
        self.events = events;
        self.armed = true;
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.span.record("events", self.events);
        self.span.record("elapsed_ms", elapsed_ms);
        let lane = self.lane;
        let stage = self.stage;
        let events = self.events;
        self.span.in_scope(|| {
            tracing::info!(lane, stage, events, elapsed_ms, "indexer stage");
        });
    }
}

fn positive_env(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(value) => value.parse().ok().filter(|parsed: &usize| *parsed > 0),
        Err(_) => None,
    }
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
    let page = i64::try_from(config.bulk_import_batch.max(1).saturating_mul(2)).unwrap_or(1_000);
    let mut tune = WaveTune::new();
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
        import_waves(
            search,
            store,
            None,
            &name,
            &docs,
            WavePlan {
                batch: config.bulk_import_batch,
                concurrency: config.bulk_import_concurrency,
                yield_max: config.bulk_yield_max,
            },
            &mut tune,
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
    index_objects_in(store, search, ids, seq, true, false).await
}

async fn index_objects_in(
    store: &Store,
    search: &Typesense,
    ids: &[ObjectId],
    seq: i64,
    advance_seq: bool,
    interactive: bool,
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
    let writer = if interactive {
        search.interactive_writer()
    } else {
        search.clone()
    };
    import_batched(
        &writer,
        writer.alias(),
        &docs,
        config.import_batch,
        config.import_concurrency,
    )
    .await?;
    if advance_seq {
        store.set_indexer_seq(seq).await?;
    }
    Ok(())
}

/// Indexes without moving the indexer watermark.
///
/// Interactive writes use this so a seq that landed ahead of the bulk lane
/// does not mark every earlier change as indexed. Redpanda still repairs the
/// document.
pub async fn index_written(
    store: &Store,
    search: &Typesense,
    ids: &[ObjectId],
    seq: i64,
) -> anyhow::Result<()> {
    index_objects_in(store, search, ids, seq, false, true).await
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
    fn a_compiled_model_is_reused_until_the_revision_changes() {
        assert!(reuse_revision(Some(4), 4));
        assert!(!reuse_revision(Some(4), 5));
        assert!(!reuse_revision(None, 1));
    }

    #[test]
    fn interactive_lane_flushes_while_bulk_is_flushing() {
        assert!(lane_due(1, 200, WINDOW, true));
        assert!(lane_due(200, 200, Duration::ZERO, true));
        assert!(!lane_due(1, 200, Duration::from_millis(10), true));
        assert!(!lane_due(0, 200, WINDOW, true));
    }

    #[tokio::test]
    async fn the_interactive_task_is_not_queued_behind_bulk() {
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (started, start_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel::<()>();
        let bulk_order = std::sync::Arc::clone(&order);
        let bulk = tokio::spawn(async move {
            bulk_order.lock().expect("order").push("bulk-start");
            let _ = started.send(());
            let _ = release_rx.await;
            bulk_order.lock().expect("order").push("bulk-end");
        });
        start_rx.await.expect("bulk started");
        let interactive_order = std::sync::Arc::clone(&order);
        let interactive = tokio::spawn(async move {
            interactive_order.lock().expect("order").push("interactive");
            let _ = release.send(());
        });
        interactive.await.expect("interactive");
        bulk.await.expect("bulk");
        assert_eq!(
            order.lock().expect("order").as_slice(),
            ["bulk-start", "interactive", "bulk-end"]
        );
    }

    #[test]
    fn bulk_import_waits_until_the_interactive_lane_is_idle_or_the_cap_hits() {
        let max = Duration::from_millis(100);
        assert!(!bulk_import_waits(0, false, Duration::ZERO, max));
        assert!(bulk_import_waits(1, false, Duration::ZERO, max));
        assert!(bulk_import_waits(0, true, Duration::from_millis(99), max));
        assert!(!bulk_import_waits(4, true, max, max));
        assert!(!bulk_import_waits(4, true, Duration::from_millis(150), max));
    }

    #[test]
    fn cold_bulk_reads_stay_at_200_until_the_lane_is_warm() {
        let mut ramp = BatchRamp::new();
        let configured = 2_000;
        assert_eq!(ramp.batch_size(configured), 200);
        ramp.observe(configured, 200, 200);
        assert_eq!(
            ramp.batch_size(configured),
            200,
            "one full read is not warm yet"
        );
        ramp.observe(configured, 17, 200);
        assert_eq!(
            ramp.batch_size(configured),
            200,
            "a short tail warmed the lane"
        );
        ramp.observe(configured, 200, 200);
        let stepped = ramp.batch_size(configured);
        assert!(
            (201..configured).contains(&stepped),
            "warm lane did not ramp: {stepped}"
        );
        let mut guard = 0;
        while ramp.batch_size(configured) < configured {
            let offered = ramp.batch_size(configured);
            let previous = offered;
            ramp.observe(configured, offered, offered);
            let next = ramp.batch_size(configured);
            assert!(next > previous, "ramp stalled at {previous}");
            guard += 1;
            assert!(guard < 20, "ramp did not reach {configured}");
        }
        assert_eq!(ramp.batch_size(configured), configured);
        ramp.observe(configured, configured, configured);
        assert_eq!(ramp.batch_size(configured), configured);
    }

    #[test]
    fn a_configured_batch_under_the_warm_cap_does_not_grow() {
        let mut ramp = BatchRamp::new();
        assert_eq!(ramp.batch_size(80), 80);
        ramp.observe(80, 80, 80);
        ramp.observe(80, 80, 80);
        assert_eq!(ramp.batch_size(80), 80);
    }

    #[test]
    fn stage_spans_name_the_lane_and_record_elapsed() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::{Arc, Mutex};

        use tracing::field::{Field, Visit};
        use tracing::span::{Attributes, Id, Record};
        use tracing::{Event, Metadata, Subscriber};

        struct Inner {
            spans: Mutex<Vec<String>>,
            records: Mutex<Vec<String>>,
            events: Mutex<Vec<String>>,
            next: AtomicU64,
        }

        struct Hit(Arc<Inner>);

        struct Text<'a>(&'a mut String);

        impl Visit for Text<'_> {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={:?};", field.name(), value);
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={value};", field.name());
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={value};", field.name());
            }

            fn record_i64(&mut self, field: &Field, value: i64) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={value};", field.name());
            }
        }

        impl Subscriber for Hit {
            fn enabled(&self, _: &Metadata<'_>) -> bool {
                true
            }

            fn new_span(&self, attrs: &Attributes<'_>) -> Id {
                let mut text = String::new();
                attrs.record(&mut Text(&mut text));
                self.0
                    .spans
                    .lock()
                    .expect("spans")
                    .push(format!("{} {text}", attrs.metadata().name()));
                let id = self.0.next.fetch_add(1, Ordering::Relaxed) + 1;
                Id::from_u64(id)
            }

            fn record(&self, _: &Id, values: &Record<'_>) {
                let mut text = String::new();
                values.record(&mut Text(&mut text));
                self.0.records.lock().expect("records").push(text);
            }

            fn record_follows_from(&self, _: &Id, _: &Id) {}

            fn event(&self, event: &Event<'_>) {
                let mut text = String::new();
                event.record(&mut Text(&mut text));
                self.0.events.lock().expect("events").push(text);
            }

            fn enter(&self, _: &Id) {}

            fn exit(&self, _: &Id) {}
        }

        let inner = Arc::new(Inner {
            spans: Mutex::new(Vec::new()),
            records: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
        });
        tracing::subscriber::with_default(Hit(Arc::clone(&inner)), || {
            for (lane, stage, events) in [
                (LaneKind::Interactive, "fetch", 3usize),
                (LaneKind::Bulk, "build", 200),
                (LaneKind::Bulk, "import", 200),
            ] {
                let mut timed = Stage::start(lane, stage);
                timed.arm(events);
                drop(timed);
            }
            drop(Stage::start(LaneKind::Interactive, "fetch"));
        });

        let spans = inner.spans.lock().expect("spans");
        let joined = spans.join("\n");
        assert!(joined.contains("indexer"), "span name missing: {joined}");
        assert!(
            joined.contains("lane=interactive"),
            "lane missing: {joined}"
        );
        assert!(joined.contains("lane=bulk"), "bulk lane missing: {joined}");
        assert!(joined.contains("stage=fetch"), "fetch missing: {joined}");
        assert!(joined.contains("stage=build"), "build missing: {joined}");
        assert!(joined.contains("stage=import"), "import missing: {joined}");
        let records = inner.records.lock().expect("records").join("\n");
        assert!(records.contains("events=3"), "events missing: {records}");
        assert!(
            records.contains("events=200"),
            "bulk events missing: {records}"
        );
        assert!(
            records.contains("elapsed_ms="),
            "elapsed missing: {records}"
        );
        let events = inner.events.lock().expect("events");
        assert_eq!(events.len(), 3, "an idle stage was logged: {events:?}");
        assert!(
            events.iter().all(|line| line.contains("indexer stage")),
            "stage event missing: {events:?}"
        );
    }

    #[test]
    fn bulk_import_defaults_are_short_waves_inside_a_larger_read() {
        let config = IndexerConfig::default();
        let tune = WaveTune::new();
        assert!(config.bulk_batch_size > config.batch_size);
        assert_eq!(config.bulk_import_batch, 200);
        assert_eq!(config.bulk_import_concurrency, 2);
        assert!(config.bulk_batch_size > config.bulk_import_batch);
        assert!((100..=200).contains(&config.bulk_import_batch));
        assert_eq!(
            bulk_import_wave(
                config.bulk_import_batch,
                config.bulk_import_concurrency,
                WaveKind::Idle,
                &tune
            ),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(
                config.bulk_import_batch,
                config.bulk_import_concurrency,
                WaveKind::Pressure,
                &tune
            ),
            ImportWave {
                batch: 200,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(
                config.bulk_import_batch,
                config.bulk_import_concurrency,
                WaveKind::Events,
                &tune
            ),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
    }

    #[test]
    fn a_configured_4_by_500_import_is_capped_and_shrinks_when_interactive_is_pending() {
        let tune = WaveTune::new();
        assert_eq!(cap_bulk_batch(500), 200);
        assert_eq!(cap_bulk_concurrency(4), 2);
        assert_eq!(
            bulk_import_wave(500, 4, WaveKind::Idle, &tune),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(500, 4, WaveKind::Pressure, &tune),
            ImportWave {
                batch: 200,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(500, 4, WaveKind::Events, &tune),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(150, 1, WaveKind::Idle, &tune),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
    }

    #[test]
    fn an_idle_wave_grows_and_an_edit_drops_it_to_one_short_call() {
        let mut tune = WaveTune::new();
        let slow = Duration::from_millis(180);
        let fast = Duration::from_millis(40);
        tune.observe(WaveKind::Idle, 100, slow);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune),
            ImportWave {
                batch: 150,
                concurrency: 1
            }
        );
        tune.observe(WaveKind::Idle, 150, slow);
        assert_eq!(bulk_import_wave(200, 2, WaveKind::Idle, &tune).batch, 200);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune).concurrency,
            1,
            "a slow call must not take the second Typesense thread"
        );
        tune.observe(WaveKind::Idle, 200, fast);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune),
            ImportWave {
                batch: 200,
                concurrency: 2
            }
        );
        tune.observe(WaveKind::Idle, 400, Duration::from_millis(80));
        let shrunk = bulk_import_wave(200, 2, WaveKind::Idle, &tune);
        assert_eq!(shrunk.concurrency, 1);
        assert_eq!(shrunk.batch, 100);
        tune.observe(WaveKind::Events, 100, fast);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Events, &tune),
            ImportWave {
                batch: 100,
                concurrency: 1
            }
        );
        tune.observe(WaveKind::Idle, 100, slow);
        tune.observe(WaveKind::Pressure, 200, slow);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune).batch,
            150,
            "pressure does not reset a wave that already grew"
        );
        tune.observe(WaveKind::Idle, 17, fast);
        assert_eq!(
            bulk_import_wave(200, 2, WaveKind::Idle, &tune).batch,
            150,
            "a short tail does not retune"
        );
    }

    #[test]
    fn bulk_docs_are_split_so_the_next_wave_can_wait() {
        let wave = ImportWave {
            batch: 150,
            concurrency: 1,
        };
        let total = 1_000;
        let mut offset = 0;
        let mut waves = 0;
        while offset < total {
            let end = wave_end(offset, total, wave);
            assert!(end > offset);
            assert!(end - offset <= wave.batch * wave.concurrency);
            offset = end;
            waves += 1;
        }
        assert!(waves > 1, "the flush was one import");
        assert_eq!(waves, 7);
    }

    #[test]
    fn pressure_pauses_briefly_and_queued_events_use_the_full_cap() {
        let max = Duration::from_millis(100);
        assert_eq!(wave_kind(false, 0), WaveKind::Idle);
        assert_eq!(wave_kind(false, 2), WaveKind::Pressure);
        assert_eq!(wave_kind(true, 0), WaveKind::Events);
        assert_eq!(wave_kind(true, 4), WaveKind::Events);
        assert_eq!(wave_gap(WaveKind::Idle, max), Duration::ZERO);
        assert_eq!(wave_gap(WaveKind::Pressure, max), Duration::from_millis(15));
        assert_eq!(wave_gap(WaveKind::Events, max), max);
        assert_eq!(wave_gap(WaveKind::Pressure, Duration::ZERO), Duration::ZERO);
        assert_eq!(
            wave_gap(WaveKind::Events, Duration::from_millis(5)),
            Duration::from_millis(5)
        );
    }

    #[tokio::test]
    async fn bulk_lane_wakes_when_the_interactive_backlog_clears() {
        let drain = Arc::new(InteractiveDrain::new());
        drain.set_pending(3);
        let waiting = Arc::clone(&drain);
        let task = tokio::spawn(async move {
            waiting.wait_for_idle(Duration::from_millis(200)).await;
        });
        tokio::time::sleep(Duration::from_millis(15)).await;
        drain.set_pending(0);
        let started = Instant::now();
        task.await.expect("wait");
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "bulk import kept waiting after the interactive topic drained"
        );
    }

    #[tokio::test]
    async fn bulk_lane_imports_after_the_cap_while_interactive_is_still_busy() {
        let drain = InteractiveDrain::new();
        drain.set_flushing(true);
        let started = Instant::now();
        drain.wait_for_idle(Duration::from_millis(40)).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(30),
            "bulk import did not wait for the interactive flush: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "bulk import waited past the cap: {elapsed:?}"
        );
    }
}
