//! Publishes this process's interactive count and reads everyone else's.
//!
//! The in-process counter is immediate for `POST /v1/bulk` in the API. A
//! loader in another process, such as `braian-bench perf-load`, only sees a
//! request once it is written here. Rows older than [`STALE`] are ignored.

use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio_postgres::NoTls;
use uuid::Uuid;

use crate::pressure::InteractiveLoad;

const TICK: Duration = Duration::from_millis(15);
const STALE_MS: u64 = 400;

const UPSERT: &str = "\
INSERT INTO interactive_pressure (instance_id, inflight, updated_at)
VALUES ($1, $2, clock_timestamp())
ON CONFLICT (instance_id) DO UPDATE
SET inflight = EXCLUDED.inflight, updated_at = clock_timestamp()";

fn others_query() -> String {
    format!(
        "SELECT COALESCE(SUM(inflight), 0)::bigint
         FROM interactive_pressure
         WHERE instance_id <> $1
           AND updated_at > clock_timestamp() - interval '{STALE_MS} milliseconds'"
    )
}

const CLEAR: &str = "DELETE FROM interactive_pressure WHERE instance_id = $1";

struct Keep {
    instance: Uuid,
    load: InteractiveLoad,
}

/// Holds the publisher alive for as long as a [`crate::Store`] clone exists.
#[derive(Clone)]
pub(crate) struct Gauge {
    _keep: Arc<Keep>,
}

pub(crate) fn start(config: tokio_postgres::Config, load: InteractiveLoad) -> Gauge {
    let keep = Arc::new(Keep {
        instance: Uuid::now_v7(),
        load,
    });
    let weak = Arc::downgrade(&keep);
    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(20);
        loop {
            let Some(keep) = weak.upgrade() else {
                return;
            };
            let instance = keep.instance;
            let load = keep.load.clone();
            drop(keep);
            match serve(&config, instance, &load, &weak).await {
                Serve::Stop => return,
                Serve::Retry => {
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(2));
                }
            }
        }
    });
    Gauge { _keep: keep }
}

enum Serve {
    Stop,
    Retry,
}

async fn serve(
    config: &tokio_postgres::Config,
    instance: Uuid,
    load: &InteractiveLoad,
    weak: &Weak<Keep>,
) -> Serve {
    let Ok((client, connection)) = config.connect(NoTls).await else {
        return Serve::Retry;
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });
    loop {
        if weak.upgrade().is_none() {
            let _ = client.execute(CLEAR, &[&instance]).await;
            return Serve::Stop;
        }
        if publish(&client, instance, load).await.is_err() {
            return Serve::Retry;
        }
    }
}

async fn publish(
    client: &tokio_postgres::Client,
    instance: Uuid,
    load: &InteractiveLoad,
) -> Result<(), tokio_postgres::Error> {
    let notified = load.notified();
    tokio::pin!(notified);
    let pending = notified.as_mut().enable();
    let local = i32::try_from(load.inflight()).unwrap_or(i32::MAX);
    client.execute(UPSERT, &[&instance, &local]).await?;
    let others: i64 = client
        .query_one(&others_query(), &[&instance])
        .await?
        .get(0);
    load.set_remote(usize::try_from(others).unwrap_or(0));
    if load.inflight() != usize::try_from(local).unwrap_or(usize::MAX) || pending {
        return Ok(());
    }
    tokio::select! {
        () = notified => {}
        () = tokio::time::sleep(TICK) => {}
    }
    Ok(())
}

/// How long a published row still counts. The SQL window is [`STALE_MS`].
#[cfg(test)]
fn pressure_row_is_fresh(age: Duration) -> bool {
    age < Duration::from_millis(STALE_MS)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::pressure_row_is_fresh;
    use crate::pressure::PRESSURE_CHUNK_CAP;
    use crate::testing::TestDb;
    use crate::{STORAGE_FORM, Store};

    #[test]
    fn a_row_older_than_the_window_does_not_count() {
        assert!(pressure_row_is_fresh(Duration::from_millis(399)));
        assert!(!pressure_row_is_fresh(Duration::from_millis(400)));
        assert!(!pressure_row_is_fresh(Duration::from_secs(2)));
    }

    #[tokio::test]
    async fn a_loader_in_another_connection_yields_for_the_api_and_ignores_a_stale_row() {
        let db = TestDb::create().await;
        let api = db.store().clone();
        let config = crate::session::parse_config(&db.database_url()).expect("url");
        let loader = Store::connect_config(config, STORAGE_FORM)
            .await
            .expect("loader");
        let (client, connection) =
            tokio_postgres::connect(&db.database_url(), tokio_postgres::NoTls)
                .await
                .expect("client");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let stale = uuid::Uuid::now_v7();
        client
            .execute(
                "INSERT INTO interactive_pressure (instance_id, inflight, updated_at)
                 VALUES ($1, 9, clock_timestamp() - interval '2 seconds')",
                &[&stale],
            )
            .await
            .expect("stale row");

        let bulk = loader.on_bulk();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            bulk.interactive_pressure(),
            0,
            "a stale row counted as interactive pressure"
        );
        assert_eq!(bulk.bulk_take(250), 250);

        let guard = api.enter_interactive();
        assert_eq!(api.interactive_pressure(), 1);
        let saw = wait_until(Duration::from_secs(2), || bulk.interactive_pressure() >= 1).await;
        assert!(saw, "the loader never saw the api's interactive request");
        assert_eq!(
            api.interactive_pressure(),
            1,
            "the api counted its own published row twice"
        );
        let slice = bulk.bulk_take(250);
        assert!(
            slice <= PRESSURE_CHUNK_CAP,
            "bulk slice stayed large under remote pressure: {slice}"
        );

        drop(guard);
        let cleared = wait_until(Duration::from_secs(2), || {
            api.interactive_inflight() == 0 && bulk.interactive_pressure() == 0
        })
        .await;
        assert!(cleared, "pressure stayed set after the request finished");
        assert_eq!(bulk.bulk_take(250), 250);
    }

    async fn wait_until(limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if ready() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ready()
    }
}
