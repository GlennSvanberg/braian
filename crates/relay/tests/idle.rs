//! An idle relay keeps the slot near the WAL head.
//!
//! WAL that the publication does not cover (here a table outside it) moves
//! `pg_current_wal_lsn()` but sends the relay nothing. Without a heartbeat the
//! slot never confirms past its last event, lag grows, and `/healthz` fails.

use std::time::{Duration, Instant};

use braian_store::admin::{self, PgConn};
use braian_store::testing::TestDb;

fn load_env() {
    let Ok(text) = std::fs::read_to_string("/workspace/.env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if std::env::var_os(key).is_none() {
            let value = value.trim().trim_matches('"');
            unsafe { std::env::set_var(key, value) };
        }
    }
}

#[tokio::test]
async fn an_idle_relay_advances_the_slot_past_unpublished_wal() {
    load_env();
    let Ok(brokers) = std::env::var("KAFKA_BROKERS") else {
        eprintln!("skip: KAFKA_BROKERS is unset");
        return;
    };
    if brokers.is_empty() {
        eprintln!("skip: KAFKA_BROKERS is empty");
        return;
    }
    let db = TestDb::create().await;
    let store = db.store().clone();
    let db_name = db.database_url().rsplit('/').next().unwrap().to_owned();
    let slot = format!("i{db_name}");
    let port = 24000
        + db_name
            .bytes()
            .fold(0u16, |acc, byte| acc.wrapping_add(u16::from(byte)))
            % 1000;
    let limit: i64 = 512 * 1024;
    let relay = tokio::spawn({
        let store = store.clone();
        let url = db.database_url();
        let slot = slot.clone();
        async move {
            braian_relay::run(
                braian_relay::Config {
                    database_url: url,
                    brokers,
                    slot,
                    listen: format!("127.0.0.1:{port}").parse().unwrap(),
                    lag_limit_bytes: limit,
                    heartbeat_interval: Duration::from_millis(500),
                },
                store,
            )
            .await
        }
    });
    let _stop = Abort(relay);

    let ready = Instant::now();
    while store.relay_lag_bytes(&slot).await.unwrap().is_none() {
        assert!(
            ready.elapsed() < Duration::from_secs(20),
            "replication slot was not created"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // About 4 MB of WAL the relay never sees as an event.
    let conn = PgConn::connect(admin::parse_config(&db.database_url()).unwrap())
        .await
        .unwrap();
    conn.client
        .batch_execute(
            "CREATE TABLE idle_noise (id int, pad text);
             INSERT INTO idle_noise SELECT g, repeat('x', 400) FROM generate_series(1, 10000) g;",
        )
        .await
        .unwrap();
    // Without the heartbeat the lag stays at about the size of the noise.
    let started = Instant::now();
    let mut lag = i64::MAX;
    while started.elapsed() < Duration::from_secs(15) {
        lag = store
            .relay_lag_bytes(&slot)
            .await
            .unwrap()
            .unwrap_or(i64::MAX);
        if lag < 64 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        lag < 64 * 1024,
        "idle slot did not advance: lag {lag} bytes after {:?}",
        started.elapsed()
    );
}

struct Abort(tokio::task::JoinHandle<anyhow::Result<()>>);

impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}
