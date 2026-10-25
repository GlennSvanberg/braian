//! Reads `changes` through a logical replication slot and publishes one small
//! event per row, in commit order.
//!
//! Interactive and bulk events are produced on separate tasks, so an interactive
//! change is not waiting on a bulk ack. The slot is confirmed only up to the
//! oldest event Kafka has accepted. A crash replays that tail. When Postgres
//! invalidates the slot, the relay drops it, asks the indexer to reindex, and
//! creates a new slot.

mod lane;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use braian_events::{Bus, ChangeEvent};
use braian_store::Store;
use lane::{
    AckQueue, BULK_BATCH, BULK_QUEUE, FETCH_LIMIT, INTERACTIVE_BATCH, INTERACTIVE_QUEUE, LaneBatch,
    LaneSenders, slot_invalidated_message, wal_status_code,
};
use pg_walstream::{
    ChangeEvent as WalEvent, ColumnValue, EventType, LogicalReplicationStream, ReplicationError,
    ReplicationStreamConfig,
};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct Config {
    pub database_url: String,
    pub brokers: String,
    pub slot: String,
    pub listen: SocketAddr,
    pub lag_limit_bytes: i64,
}

struct Metrics {
    lag_bytes: AtomicI64,
    exceeded: AtomicBool,
    limit: i64,
    /// See [`wal_status_code`].
    wal_status: AtomicU8,
    invalidated: AtomicBool,
    slot_missing: AtomicBool,
    recoveries: AtomicU64,
    wal_warned: AtomicBool,
}

pub async fn run(config: Config, store: Store) -> anyhow::Result<()> {
    let bus = Bus::connect(&config.brokers)?;
    bus.ensure_topics().await?;
    let metrics = Arc::new(Metrics {
        lag_bytes: AtomicI64::new(0),
        exceeded: AtomicBool::new(false),
        limit: config.lag_limit_bytes,
        wal_status: AtomicU8::new(0),
        invalidated: AtomicBool::new(false),
        slot_missing: AtomicBool::new(true),
        recoveries: AtomicU64::new(0),
        wal_warned: AtomicBool::new(false),
    });
    let lost = Arc::new(AtomicBool::new(false));
    spawn_lag_poll(
        store.clone(),
        config.slot.clone(),
        Arc::clone(&metrics),
        Arc::clone(&lost),
    );
    spawn_metrics(config.listen, Arc::clone(&metrics));

    loop {
        match stream_once(&config, &bus, &lost).await {
            Ok(()) => return Ok(()),
            Err(error) if should_recover(&error, &lost) => {
                tracing::error!(
                    slot = %config.slot,
                    %error,
                    "replication slot invalidated; dropping it, requesting a full reindex, and creating a new slot"
                );
                recover(&config, &store, &metrics).await?;
                lost.store(false, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn should_recover(error: &anyhow::Error, lost: &AtomicBool) -> bool {
    lost.load(Ordering::Relaxed) || slot_invalidated_message(&error.to_string())
}

fn spawn_lag_poll(store: Store, slot: String, metrics: Arc<Metrics>, lost: Arc<AtomicBool>) {
    tokio::spawn(async move {
        loop {
            match store.relay_slot_status(&slot).await {
                Ok(Some(status)) => {
                    let lag = status.lag_bytes.unwrap_or(0);
                    metrics.lag_bytes.store(lag, Ordering::Relaxed);
                    metrics.slot_missing.store(false, Ordering::Relaxed);
                    let code = wal_status_code(Some(status.wal_status.as_str()));
                    metrics.wal_status.store(code, Ordering::Relaxed);
                    let invalidated = status.invalidated();
                    if invalidated && !metrics.invalidated.swap(true, Ordering::Relaxed) {
                        tracing::error!(
                            slot,
                            wal_status = %status.wal_status,
                            reason = ?status.invalidation_reason,
                            lag,
                            "replication slot is invalidated"
                        );
                        lost.store(true, Ordering::Relaxed);
                    }
                    if !invalidated {
                        metrics.invalidated.store(false, Ordering::Relaxed);
                    }
                    let over = lag > metrics.limit;
                    if over && !metrics.exceeded.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            lag,
                            limit = metrics.limit,
                            "replication slot lag exceeded the limit"
                        );
                    }
                    if !over {
                        metrics.exceeded.store(false, Ordering::Relaxed);
                    }
                    let retaining = matches!(status.wal_status.as_str(), "extended" | "unreserved");
                    if retaining && !metrics.wal_warned.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            slot,
                            wal_status = %status.wal_status,
                            lag,
                            "replication slot is retaining WAL past max_wal_size"
                        );
                    }
                    if !retaining {
                        metrics.wal_warned.store(false, Ordering::Relaxed);
                    }
                }
                Ok(None) => {
                    metrics.lag_bytes.store(0, Ordering::Relaxed);
                    metrics.slot_missing.store(true, Ordering::Relaxed);
                    metrics.wal_status.store(0, Ordering::Relaxed);
                    metrics.invalidated.store(false, Ordering::Relaxed);
                }
                Err(error) => tracing::warn!(%error, "could not read replication lag"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn spawn_metrics(listen: SocketAddr, metrics: Arc<Metrics>) {
    tokio::spawn(async move {
        let app = Router::new()
            .route("/metrics", get(metrics_text))
            .route("/healthz", get(health))
            .with_state(metrics);
        let listener = match tokio::net::TcpListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%error, %listen, "relay metrics listener failed");
                return;
            }
        };
        tracing::info!(%listen, "relay metrics listening");
        let _ = axum::serve(listener, app).await;
    });
}

async fn recover(config: &Config, store: &Store, metrics: &Metrics) -> anyhow::Result<()> {
    let mut last_error = None;
    for attempt in 1..=5 {
        match store.drop_replication_slot(&config.slot).await {
            Ok(()) => {
                last_error = None;
                break;
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    attempt,
                    slot = %config.slot,
                    "waiting to drop the invalidated replication slot"
                );
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    if let Some(error) = last_error {
        return Err(error.into());
    }
    store.request_reindex().await?;
    metrics.recoveries.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        slot = %config.slot,
        "reindex requested; the indexer rebuilds the search alias from Postgres"
    );
    Ok(())
}

async fn stream_once(config: &Config, bus: &Bus, lost: &AtomicBool) -> anyhow::Result<()> {
    let url = replication_url(&config.database_url);
    let stream_config = ReplicationStreamConfig::builder(&config.slot, "braian_changes")
        .with_protocol_version(1)
        .with_feedback_interval(Duration::from_millis(200));
    let mut stream = LogicalReplicationStream::new(&url, stream_config)
        .await
        .map_err(|error| anyhow::anyhow!("replication connect: {error}"))?;
    stream
        .start(None)
        .await
        .map_err(|error| anyhow::anyhow!("replication start: {error}"))?;
    let cancel = pg_walstream::CancellationToken::new();
    let mut events = stream.into_stream(cancel);
    publish_stream(bus, &mut events, lost).await
}

enum LaneAck {
    Done(Vec<u64>),
    Failed(String),
}

async fn lane_worker(
    mut rx: mpsc::Receiver<LaneBatch<ChangeEvent>>,
    bus: Bus,
    ack_tx: mpsc::UnboundedSender<LaneAck>,
) {
    while let Some(batch) = rx.recv().await {
        let result = bus.publish_many(&batch.items).await;
        let failed = result.is_err();
        let ack = match result {
            Ok(()) => LaneAck::Done(batch.tokens),
            Err(error) => LaneAck::Failed(error.to_string()),
        };
        if ack_tx.send(ack).is_err() || failed {
            break;
        }
    }
}

async fn publish_stream(
    bus: &Bus,
    events: &mut pg_walstream::EventStream,
    lost: &AtomicBool,
) -> anyhow::Result<()> {
    let (interactive_tx, interactive_rx) = mpsc::channel(INTERACTIVE_QUEUE);
    let (bulk_tx, bulk_rx) = mpsc::channel(BULK_QUEUE);
    let senders = LaneSenders::pair(interactive_tx, bulk_tx);
    let (ack_tx, mut ack_rx) = mpsc::unbounded_channel();
    let interactive_worker = tokio::spawn(lane_worker(interactive_rx, bus.clone(), ack_tx.clone()));
    let bulk_worker = tokio::spawn(lane_worker(bulk_rx, bus.clone(), ack_tx.clone()));
    drop(ack_tx);

    let mut acks = AckQueue::default();
    let mut interactive = Vec::new();
    let mut interactive_tokens = Vec::new();
    let mut bulk = Vec::new();
    let mut bulk_tokens = Vec::new();
    let mut fetched = 0usize;

    let outcome = async {
        loop {
            if lost.load(Ordering::Relaxed) {
                anyhow::bail!("replication slot invalidated");
            }
            if let Some(lsn) = drain_acks(&mut ack_rx, &mut acks)? {
                events.update_applied_lsn(lsn);
            }
            let buffered = !interactive.is_empty() || !bulk.is_empty();
            let wait = if buffered {
                Duration::from_millis(2)
            } else {
                Duration::from_millis(50)
            };
            match tokio::time::timeout(wait, events.next_event()).await {
                Ok(Ok(event)) => {
                    if let Some(lsn) = stage_event(
                        event,
                        &mut acks,
                        &mut interactive,
                        &mut interactive_tokens,
                        &mut bulk,
                        &mut bulk_tokens,
                        &mut fetched,
                        &senders,
                    )
                    .await?
                    {
                        events.update_applied_lsn(lsn);
                    }
                }
                Ok(Err(ReplicationError::Cancelled(_) | ReplicationError::StreamStopped(_))) => {
                    return Ok(());
                }
                Ok(Err(error)) => {
                    anyhow::bail!("replication stream: {error}");
                }
                Err(_timeout) => {
                    flush_lanes(
                        &senders,
                        &mut interactive,
                        &mut interactive_tokens,
                        &mut bulk,
                        &mut bulk_tokens,
                    )
                    .await?;
                    fetched = 0;
                }
            }
        }
    }
    .await;

    if outcome.is_err() {
        interactive_worker.abort();
        bulk_worker.abort();
        return outcome;
    }

    if let Err(error) = flush_lanes(
        &senders,
        &mut interactive,
        &mut interactive_tokens,
        &mut bulk,
        &mut bulk_tokens,
    )
    .await
    {
        interactive_worker.abort();
        bulk_worker.abort();
        return Err(error);
    }
    drop(senders);
    while let Some(ack) = ack_rx.recv().await {
        if let Some(lsn) = apply_ack(ack, &mut acks)? {
            events.update_applied_lsn(lsn);
        }
    }
    interactive_worker
        .await
        .map_err(|error| anyhow::anyhow!("interactive publisher: {error}"))?;
    bulk_worker
        .await
        .map_err(|error| anyhow::anyhow!("bulk publisher: {error}"))?;
    Ok(())
}

fn drain_acks(
    ack_rx: &mut mpsc::UnboundedReceiver<LaneAck>,
    acks: &mut AckQueue,
) -> anyhow::Result<Option<u64>> {
    let mut applied = None;
    while let Ok(ack) = ack_rx.try_recv() {
        if let Some(lsn) = apply_ack(ack, acks)? {
            applied = Some(lsn);
        }
    }
    Ok(applied)
}

fn apply_ack(ack: LaneAck, acks: &mut AckQueue) -> anyhow::Result<Option<u64>> {
    match ack {
        LaneAck::Done(tokens) => {
            let mut applied = None;
            for token in tokens {
                if let Some(lsn) = acks.finish(token) {
                    applied = Some(lsn);
                }
            }
            Ok(applied)
        }
        LaneAck::Failed(message) => anyhow::bail!("publish: {message}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn stage_event(
    event: WalEvent,
    acks: &mut AckQueue,
    interactive: &mut Vec<ChangeEvent>,
    interactive_tokens: &mut Vec<u64>,
    bulk: &mut Vec<ChangeEvent>,
    bulk_tokens: &mut Vec<u64>,
    fetched: &mut usize,
    senders: &LaneSenders<ChangeEvent>,
) -> anyhow::Result<Option<u64>> {
    let lsn = event.lsn.value();
    let token = acks.push(lsn);
    let Some(change) = change_from_wal(&event) else {
        return Ok(acks.finish(token));
    };
    if change.kind == "bulk" {
        if !interactive.is_empty() {
            senders
                .submit(take_batch(interactive, interactive_tokens), empty_batch())
                .await
                .map_err(|()| anyhow::anyhow!("interactive publisher stopped"))?;
        }
        bulk.push(change);
        bulk_tokens.push(token);
    } else {
        interactive.push(change);
        interactive_tokens.push(token);
    }
    *fetched += 1;
    let interactive_full = interactive.len() >= INTERACTIVE_BATCH;
    let bulk_full = bulk.len() >= BULK_BATCH || *fetched >= FETCH_LIMIT;
    if interactive_full || bulk_full {
        flush_lanes(senders, interactive, interactive_tokens, bulk, bulk_tokens).await?;
        if bulk_full {
            *fetched = 0;
        }
    }
    Ok(None)
}

async fn flush_lanes(
    senders: &LaneSenders<ChangeEvent>,
    interactive: &mut Vec<ChangeEvent>,
    interactive_tokens: &mut Vec<u64>,
    bulk: &mut Vec<ChangeEvent>,
    bulk_tokens: &mut Vec<u64>,
) -> anyhow::Result<()> {
    senders
        .submit(
            take_batch(interactive, interactive_tokens),
            take_batch(bulk, bulk_tokens),
        )
        .await
        .map_err(|()| anyhow::anyhow!("lane publisher stopped"))
}

fn take_batch(items: &mut Vec<ChangeEvent>, tokens: &mut Vec<u64>) -> LaneBatch<ChangeEvent> {
    LaneBatch {
        items: std::mem::take(items),
        tokens: std::mem::take(tokens),
    }
}

fn empty_batch() -> LaneBatch<ChangeEvent> {
    LaneBatch {
        items: Vec::new(),
        tokens: Vec::new(),
    }
}

fn replication_url(database_url: &str) -> String {
    let joiner = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{joiner}replication=database&sslmode=disable")
}

fn change_from_wal(event: &WalEvent) -> Option<ChangeEvent> {
    let EventType::Insert { table, data, .. } = &event.event_type else {
        return None;
    };
    if table.as_ref() != "changes" && !table.starts_with("changes_") {
        return None;
    }
    let seq = data
        .get("seq")
        .and_then(ColumnValue::as_str)?
        .parse()
        .ok()?;
    let op = data.get("op").and_then(ColumnValue::as_str)?.to_owned();
    let batch_id = Uuid::parse_str(data.get("batch_id").and_then(ColumnValue::as_str)?).ok()?;
    let kind = data
        .get("kind")
        .and_then(ColumnValue::as_str)
        .unwrap_or("interactive")
        .to_owned();
    let object_id = data
        .get("object_id")
        .and_then(ColumnValue::as_str)
        .and_then(|text| Uuid::parse_str(text).ok());
    let version = data
        .get("version")
        .and_then(ColumnValue::as_str)
        .and_then(|text| text.parse().ok());
    let at = data
        .get("at")
        .and_then(ColumnValue::as_str)
        .unwrap_or("")
        .to_owned();
    Some(ChangeEvent {
        seq,
        op,
        object_id,
        version,
        batch_id,
        kind,
        at,
    })
}

async fn health(State(metrics): State<Arc<Metrics>>) -> StatusCode {
    if unhealthy(metrics.as_ref()) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NO_CONTENT
    }
}

fn unhealthy(metrics: &Metrics) -> bool {
    let wal = metrics.wal_status.load(Ordering::Relaxed);
    metrics.exceeded.load(Ordering::Relaxed)
        || metrics.invalidated.load(Ordering::Relaxed)
        || metrics.slot_missing.load(Ordering::Relaxed)
        || wal == 3
        || wal == 4
}

async fn metrics_text(State(metrics): State<Arc<Metrics>>) -> String {
    let lag = metrics.lag_bytes.load(Ordering::Relaxed);
    let exceeded = i32::from(metrics.exceeded.load(Ordering::Relaxed));
    let invalidated = i32::from(metrics.invalidated.load(Ordering::Relaxed));
    let missing = i32::from(metrics.slot_missing.load(Ordering::Relaxed));
    let wal = metrics.wal_status.load(Ordering::Relaxed);
    let recoveries = metrics.recoveries.load(Ordering::Relaxed);
    format!(
        "# HELP braian_relay_lag_bytes WAL bytes the replication slot has not confirmed.\n\
         # TYPE braian_relay_lag_bytes gauge\n\
         braian_relay_lag_bytes {lag}\n\
         # TYPE braian_relay_lag_limit_bytes gauge\n\
         braian_relay_lag_limit_bytes {}\n\
         # TYPE braian_relay_lag_exceeded gauge\n\
         braian_relay_lag_exceeded {exceeded}\n\
         # HELP braian_relay_slot_wal_status 0 missing, 1 reserved, 2 extended, 3 unreserved, 4 lost, 5 other.\n\
         # TYPE braian_relay_slot_wal_status gauge\n\
         braian_relay_slot_wal_status {wal}\n\
         # HELP braian_relay_slot_invalidated 1 when Postgres has invalidated the slot.\n\
         # TYPE braian_relay_slot_invalidated gauge\n\
         braian_relay_slot_invalidated {invalidated}\n\
         # TYPE braian_relay_slot_missing gauge\n\
         braian_relay_slot_missing {missing}\n\
         # TYPE braian_relay_slot_recoveries_total counter\n\
         braian_relay_slot_recoveries_total {recoveries}\n",
        metrics.limit
    )
}

#[cfg(test)]
mod tests {
    use super::replication_url;

    #[test]
    fn replication_url_adds_the_slot_parameters() {
        let url = replication_url("postgres://braian:secret@localhost:5433/braian");
        assert!(url.contains("replication=database"));
        assert!(url.contains("sslmode=disable"));
        assert!(!url.contains("secretsecret"));
    }
}
