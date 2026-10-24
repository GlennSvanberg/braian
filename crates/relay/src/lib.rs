//! Reads `changes` through a logical replication slot and publishes one small
//! event per row, in commit order. The slot is confirmed only after Kafka
//! accepts the event, so a crash replays rather than drops.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use braian_events::{Bus, ChangeEvent};
use braian_store::Store;
use pg_walstream::{
    ChangeEvent as WalEvent, ColumnValue, EventType, LogicalReplicationStream, ReplicationError,
    ReplicationStreamConfig,
};
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
}

pub async fn run(config: Config, store: Store) -> anyhow::Result<()> {
    let bus = Bus::connect(&config.brokers)?;
    bus.ensure_topics().await?;
    let metrics = Arc::new(Metrics {
        lag_bytes: AtomicI64::new(0),
        exceeded: AtomicBool::new(false),
        limit: config.lag_limit_bytes,
    });
    let poll_metrics = Arc::clone(&metrics);
    let poll_store = store.clone();
    let slot = config.slot.clone();
    tokio::spawn(async move {
        loop {
            match poll_store.relay_lag_bytes(&slot).await {
                Ok(Some(lag)) => {
                    poll_metrics.lag_bytes.store(lag, Ordering::Relaxed);
                    let over = lag > poll_metrics.limit;
                    if over && !poll_metrics.exceeded.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            lag,
                            limit = poll_metrics.limit,
                            "replication slot lag exceeded the limit"
                        );
                    }
                    if !over {
                        poll_metrics.exceeded.store(false, Ordering::Relaxed);
                    }
                }
                Ok(None) => poll_metrics.lag_bytes.store(0, Ordering::Relaxed),
                Err(error) => tracing::warn!(%error, "could not read replication lag"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let listen = config.listen;
    let served = Arc::clone(&metrics);
    tokio::spawn(async move {
        let app = Router::new()
            .route("/metrics", get(metrics_text))
            .route("/healthz", get(|| async { "" }))
            .with_state(served);
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
    // Ack only after Kafka accepts the event. The library reports that applied
    // LSN on its feedback interval, from inside the read loop, so the socket
    // keeps draining. A crash replays unconfirmed rows; it does not drop them.
    events
        .for_each_event(move |event| {
            let bus = bus.clone();
            async move {
                if let Some(change) = change_from_wal(&event) {
                    bus.publish(&change)
                        .await
                        .map_err(|error| ReplicationError::Protocol(error.to_string()))?;
                }
                Ok(())
            }
        })
        .await
        .map_err(|error| anyhow::anyhow!("replication stream: {error}"))?;
    Ok(())
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

async fn metrics_text(State(metrics): State<Arc<Metrics>>) -> String {
    let lag = metrics.lag_bytes.load(Ordering::Relaxed);
    let exceeded = i32::from(metrics.exceeded.load(Ordering::Relaxed));
    format!(
        "# HELP braian_relay_lag_bytes WAL bytes the replication slot has not confirmed.\n\
         # TYPE braian_relay_lag_bytes gauge\n\
         braian_relay_lag_bytes {lag}\n\
         # TYPE braian_relay_lag_limit_bytes gauge\n\
         braian_relay_lag_limit_bytes {}\n\
         # TYPE braian_relay_lag_exceeded gauge\n\
         braian_relay_lag_exceeded {exceeded}\n",
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
