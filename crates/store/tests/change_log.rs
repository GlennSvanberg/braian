//! The change log is `changes`. Workers lease seq ranges; a crash replays them.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use braian_store::admin;
use braian_store::testing::{PageGap, TestDb, append_changes, expire_claims};
use braian_store::{Actor, BatchKind, PrincipalKind, STORAGE_FORM, Store};
use uuid::Uuid;

fn actor(principal: braian_store::PrincipalId, kind: BatchKind) -> Actor {
    Actor {
        actor: principal,
        on_behalf_of: None,
        kind,
        source: "change-log".into(),
    }
}

async fn principal(store: &Store) -> Actor {
    let id = store
        .upsert_principal("ada", PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    actor(id, BatchKind::Bulk)
}

fn ids(count: usize) -> Vec<Option<Uuid>> {
    (0..count).map(|_| Some(Uuid::now_v7())).collect()
}

#[tokio::test]
async fn a_crashed_worker_resumes_from_the_uncommitted_range() {
    let db = TestDb::create().await;
    let store = db.store();
    let who = principal(store).await;
    let seqs = append_changes(store, &who, &ids(6)).await.unwrap();
    let holder = Uuid::now_v7();
    let first = store
        .claim_changes("bulk", holder, 6, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("range");
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        seqs
    );
    expire_claims(store, "bulk").await.unwrap();
    let resumed = store
        .claim_changes("bulk", Uuid::now_v7(), 6, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("replay");
    assert_eq!(
        resumed
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        seqs
    );
    let committed = store
        .commit_claim("bulk", resumed.holder, resumed.from_seq)
        .await
        .unwrap();
    assert_eq!(committed, *seqs.last().unwrap());
    assert!(
        store
            .claim_changes("bulk", Uuid::now_v7(), 6, Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_claim_returns_every_change_for_one_object_in_order() {
    let db = TestDb::create().await;
    let store = db.store();
    let mut who = principal(store).await;
    who.kind = BatchKind::Interactive;
    let object = Uuid::now_v7();
    let seqs = append_changes(store, &who, &[Some(object), Some(object), Some(object)])
        .await
        .unwrap();
    let claim = store
        .claim_changes("interactive", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("claim");
    let seen: Vec<_> = claim.events.iter().map(|event| event.seq).collect();
    assert_eq!(seen, seqs);
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        claim
            .events
            .iter()
            .all(|event| event.object_id == Some(object))
    );
}

#[tokio::test]
async fn interactive_is_claimed_while_a_bulk_range_is_still_open() {
    let db = TestDb::create().await;
    let store = db.store();
    let mut bulk = principal(store).await;
    bulk.kind = BatchKind::Bulk;
    append_changes(store, &bulk, &ids(30)).await.unwrap();
    let mut interactive = bulk.clone();
    interactive.kind = BatchKind::Interactive;
    let edited = append_changes(store, &interactive, &[Some(Uuid::now_v7())])
        .await
        .unwrap();
    let held = store
        .claim_changes("bulk", Uuid::now_v7(), 30, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("bulk");
    assert_eq!(held.events.len(), 30);
    let started = Instant::now();
    let edit = store
        .claim_changes("interactive", Uuid::now_v7(), 5, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("interactive");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "interactive claim waited {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(
        edit.events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        edited
    );
    assert!(edit.events.iter().all(|event| event.kind == "interactive"));
    assert_eq!(store.lane_committed("bulk").await.unwrap(), 0);
}

#[tokio::test]
async fn two_workers_claim_disjoint_ranges() {
    let db = TestDb::create().await;
    let store = db.store();
    let who = principal(store).await;
    let seqs = append_changes(store, &who, &ids(20)).await.unwrap();
    let first = store
        .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("first");
    let second = store
        .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("second");
    let left: HashSet<_> = first.events.iter().map(|event| event.seq).collect();
    let right: HashSet<_> = second.events.iter().map(|event| event.seq).collect();
    assert_eq!(left.len(), 10);
    assert_eq!(right.len(), 10);
    assert!(left.is_disjoint(&right));
    let mut both = left;
    both.extend(right);
    assert_eq!(both, seqs.into_iter().collect());
}

#[tokio::test]
async fn a_commit_on_another_connection_wakes_the_listener() {
    let db = TestDb::create().await;
    let store = db.store().clone();
    let mut config = admin::parse_config(&db.database_url()).unwrap();
    config.dbname(db.database_url().rsplit('/').next().unwrap());
    let other = Store::connect_config(config, STORAGE_FORM).await.unwrap();
    let cursor = store.max_change_seq().await.unwrap();
    let listener = store.listen_changes().await.unwrap();
    let waiting = tokio::spawn(async move {
        let started = Instant::now();
        listener.wait(Duration::from_secs(5)).await;
        started.elapsed()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let who = principal(&other).await;
    let seqs = append_changes(&other, &who, &[Some(Uuid::now_v7())])
        .await
        .unwrap();
    let elapsed = waiting.await.unwrap();
    assert!(
        elapsed < Duration::from_secs(2),
        "LISTEN did not wake the tail in {} ms",
        elapsed.as_millis()
    );
    let rows = store.changes_after(cursor, 10).await.unwrap();
    assert_eq!(rows.iter().map(|row| row.seq).collect::<Vec<_>>(), seqs);
}

/// A lower seq is still open. The higher seq has committed. Neither lane, nor
/// the live tail, may move past the open seq.
async fn assert_open_seq_blocks(store: &Store, cursor: i64) {
    for lane in ["interactive", "bulk"] {
        assert!(
            store
                .claim_changes(lane, Uuid::now_v7(), 10, Duration::from_secs(30))
                .await
                .unwrap()
                .is_none(),
            "{lane} claimed past an open seq"
        );
        assert_eq!(store.lane_committed(lane).await.unwrap(), cursor);
    }
    assert!(
        store.changes_after(cursor, 10).await.unwrap().is_empty(),
        "live tail read past an open seq"
    );
    assert_eq!(store.safe_change_seq().await.unwrap(), cursor);
}

async fn connect(url: &str) -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            panic!("log connection closed: {error}");
        }
    });
    client
}

async fn begin(url: &str) -> tokio_postgres::Client {
    let client = connect(url).await;
    client.batch_execute("BEGIN").await.unwrap();
    client
}

async fn insert_row(client: &tokio_postgres::Client, actor: Uuid, kind: &str) -> i64 {
    let batch = Uuid::now_v7();
    client
        .execute(
            "INSERT INTO batches (id, actor, on_behalf_of, kind, source)
             VALUES ($1, $2, NULL, $3, 'log')",
            &[&batch, &actor, &kind],
        )
        .await
        .unwrap();
    let row = client
        .query_one(
            "INSERT INTO changes
                (batch_id, object_id, version, op, actor, on_behalf_of, diff, kind)
             VALUES ($1, $2, 1, 'update', $3, NULL, NULL, $4)
             RETURNING seq",
            &[&batch, &Uuid::now_v7(), &actor, &kind],
        )
        .await
        .unwrap();
    row.get(0)
}

#[tokio::test]
async fn a_higher_seq_that_commits_first_stays_behind_the_open_one() {
    let db = TestDb::create().await;
    let store = db.store();
    let who = principal(store).await;
    let actor = who.actor.as_uuid();
    let cursor = store.safe_change_seq().await.unwrap();
    let early = begin(&db.database_url()).await;
    let low = insert_row(&early, actor, "bulk").await;
    let late = begin(&db.database_url()).await;
    let high = insert_row(&late, actor, "interactive").await;
    late.batch_execute("COMMIT").await.unwrap();
    assert!(low < high);

    assert_open_seq_blocks(store, cursor).await;

    early.batch_execute("COMMIT").await.unwrap();
    let tail = store.changes_after(cursor, 10).await.unwrap();
    assert_eq!(
        tail.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![low, high]
    );
    assert_eq!(store.safe_change_seq().await.unwrap(), high);

    let interactive = store
        .claim_changes("interactive", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("interactive");
    assert_eq!(
        interactive
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![high]
    );
    let bulk = store
        .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("bulk");
    assert_eq!(
        bulk.events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![low]
    );
}

#[tokio::test]
async fn an_earlier_transaction_can_commit_a_higher_seq_first() {
    let db = TestDb::create().await;
    let store = db.store();
    let who = principal(store).await;
    let actor = who.actor.as_uuid();
    let cursor = store.safe_change_seq().await.unwrap();
    // Lower xid, higher seq: xmin would call this row stable while `low` is open.
    let early = begin(&db.database_url()).await;
    early
        .query_one("SELECT pg_current_xact_id()", &[])
        .await
        .unwrap();
    let later = begin(&db.database_url()).await;
    let low = insert_row(&later, actor, "bulk").await;
    let high = insert_row(&early, actor, "interactive").await;
    early.batch_execute("COMMIT").await.unwrap();
    assert!(low < high);

    assert_open_seq_blocks(store, cursor).await;

    later.batch_execute("COMMIT").await.unwrap();
    let tail = store.changes_after(cursor, 10).await.unwrap();
    assert_eq!(
        tail.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![low, high]
    );
    assert_eq!(store.safe_change_seq().await.unwrap(), high);
    let interactive = store
        .claim_changes("interactive", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("interactive");
    assert_eq!(interactive.events[0].seq, high);
    let bulk = store
        .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("bulk");
    assert_eq!(bulk.events[0].seq, low);
}

#[tokio::test]
async fn an_aborted_seq_is_skipped_once_nothing_else_is_open() {
    let db = TestDb::create().await;
    let store = db.store();
    let who = principal(store).await;
    let actor = who.actor.as_uuid();
    let cursor = store.safe_change_seq().await.unwrap();
    let early = begin(&db.database_url()).await;
    let low = insert_row(&early, actor, "bulk").await;
    let late = begin(&db.database_url()).await;
    let high = insert_row(&late, actor, "interactive").await;
    late.batch_execute("COMMIT").await.unwrap();
    assert_open_seq_blocks(store, cursor).await;

    early.batch_execute("ROLLBACK").await.unwrap();
    // "Nothing else is open" has to hold for the reads below, not just usually.
    let quiet = quiet_database(&db.database_url()).await;
    let tail = store.changes_after(cursor, 10).await.unwrap();
    assert_eq!(
        tail.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![high]
    );
    assert!(tail.iter().all(|row| row.seq != low));
    assert_eq!(store.safe_change_seq().await.unwrap(), high);
    let interactive = store
        .claim_changes("interactive", Uuid::now_v7(), 10, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("interactive");
    assert_eq!(interactive.events[0].seq, high);
    assert!(
        store
            .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
            .await
            .unwrap()
            .is_none(),
        "aborted bulk seq was claimed as a row"
    );
    assert_eq!(store.lane_committed("bulk").await.unwrap(), high);
    quiet.batch_execute("COMMIT").await.unwrap();
}

/// Keeps every other transaction in this database out until the returned
/// session commits.
///
/// Two backends besides the test take an xid here on their own schedule, and
/// the log rightly treats an aborted seq as one that may still commit while
/// either is open:
///
/// - The store's pressure gauge upserts `interactive_pressure` on its own
///   connection every 15 ms. `SHARE` waits for a running upsert to commit and
///   blocks the next one at its table lock, before it takes an xid.
/// - An autovacuum worker that connects to this database also analyzes the
///   shared `pg_database`, which every parallel test churns with `CREATE` and
///   `DROP DATABASE`. Analyze takes an xid. Holding `SHARE UPDATE EXCLUSIVE`
///   waits for a running one and makes autovacuum skip the table. It does not
///   block `CREATE DATABASE`. The `SHARE` lock above also keeps autovacuum off
///   `interactive_pressure`.
///
/// `LOCK TABLE` takes no xid, so this session is not counted as open either.
async fn quiet_database(url: &str) -> tokio_postgres::Client {
    let quiet = begin(url).await;
    quiet
        .batch_execute(
            "LOCK TABLE interactive_pressure IN SHARE MODE;
             LOCK TABLE pg_catalog.pg_database IN SHARE UPDATE EXCLUSIVE MODE",
        )
        .await
        .unwrap();
    let open: Vec<String> = quiet
        .query(
            "SELECT backend_type || ': ' || coalesce(query, '')
             FROM pg_stat_activity
             WHERE datname = current_database()
               AND pid <> pg_backend_pid()
               AND backend_xid IS NOT NULL",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert!(
        open.is_empty(),
        "another transaction is still open: {open:?}"
    );
    quiet
}

/// The blocking transaction starts after the read's first activity check and
/// before the page. A higher seq then commits. The stale activity snapshot
/// would call the lower seq aborted.
///
/// A claim with a leading hole returns before that pause when any other
/// backend already holds an xid. The pressure gauge upserts on its own
/// connection every few milliseconds, so that early return is a race. Retry
/// until the read is actually inside the window.
async fn during_page_gap<T, F, Fut>(url: &str, actor: Uuid, mut read: F) -> (T, i64)
where
    T: Send + 'static,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = T> + Send + 'static,
{
    for attempt in 1..=40 {
        let gap = PageGap::arm();
        let mut task = gap.drive(read());
        let entered = tokio::select! {
            biased;
            _ = gap.wait_entered() => true,
            result = &mut task => {
                result.expect("read task");
                false
            }
            () = tokio::time::sleep(Duration::from_secs(5)) => {
                panic!("the read did not reach the page window");
            }
        };
        if !entered {
            assert!(
                attempt < 40,
                "the read kept stopping before the page window"
            );
            continue;
        }
        let blocker = begin(url).await;
        let low = insert_row(&blocker, actor, "bulk").await;
        let other = begin(url).await;
        let high = insert_row(&other, actor, "interactive").await;
        other.batch_execute("COMMIT").await.unwrap();
        assert!(low < high);
        gap.release();
        let value = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the read stayed in the page window")
            .expect("read task");
        blocker.batch_execute("ROLLBACK").await.unwrap();
        return (value, low);
    }
    unreachable!("the retry loop returns or panics");
}

#[tokio::test]
async fn an_open_seq_that_starts_after_the_activity_check_is_not_skipped() {
    let db = TestDb::create().await;
    let store = db.store().clone();
    let url = db.database_url();
    let who = principal(&store).await;
    let actor = who.actor.as_uuid();
    let cursor = store.safe_change_seq().await.unwrap();

    for lane in ["interactive", "bulk"] {
        let reading = store.clone();
        let (claimed, low) = during_page_gap(&url, actor, move || {
            let reading = reading.clone();
            async move {
                reading
                    .claim_changes(lane, Uuid::now_v7(), 10, Duration::from_secs(30))
                    .await
            }
        })
        .await;
        assert!(
            claimed.unwrap().is_none(),
            "{lane} claimed a seq that was still open"
        );
        assert!(low > cursor);
        assert_eq!(store.lane_committed(lane).await.unwrap(), cursor);
        assert_eq!(store.lane_committed("interactive").await.unwrap(), cursor);
        assert_eq!(store.lane_committed("bulk").await.unwrap(), cursor);
        let claims = connect(&url).await;
        let open_claims: i64 = claims
            .query_one("SELECT count(*)::bigint FROM index_claim", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(open_claims, 0, "{lane} filled a hole over an open seq");
    }

    let reading = store.clone();
    let (tail, low) = during_page_gap(&url, actor, move || {
        let reading = reading.clone();
        async move { reading.changes_after(cursor, 10).await }
    })
    .await;
    let tail = tail.unwrap();
    assert!(
        tail.iter().all(|row| row.seq < low),
        "live tail returned {tail:?} at or above the open seq {low}"
    );

    let reading = store.clone();
    let (safe, low) = during_page_gap(&url, actor, move || {
        let reading = reading.clone();
        async move { reading.safe_change_seq().await }
    })
    .await;
    let safe = safe.unwrap();
    assert!(safe < low, "safe seq {safe} passed the open seq {low}");
    assert_eq!(safe, cursor);
}

#[tokio::test]
async fn a_hole_is_not_filled_when_a_transaction_opens_during_the_page_read() {
    let db = TestDb::create().await;
    let store = db.store().clone();
    let url = db.database_url();
    let who = principal(&store).await;
    let actor = who.actor.as_uuid();
    let cursor = store.lane_committed("bulk").await.unwrap();
    let admin = connect(&url).await;
    admin
        .query_one(
            "SELECT setval(pg_get_serial_sequence('changes', 'seq'), 200)",
            &[],
        )
        .await
        .unwrap();
    let holder = Uuid::now_v7();
    admin
        .execute(
            "INSERT INTO index_claim (lane, from_seq, through_seq, holder, leased_until, done)
             VALUES ('bulk', 50, 51, $1, now() + interval '5 minutes', false)",
            &[&holder],
        )
        .await
        .unwrap();

    let reading = store.clone();
    let (claimed, low) = during_page_gap(&url, actor, move || {
        let reading = reading.clone();
        async move {
            reading
                .claim_changes("bulk", Uuid::now_v7(), 10, Duration::from_secs(30))
                .await
        }
    })
    .await;
    assert!(low > 50);
    assert!(
        claimed.unwrap().is_none(),
        "hole fill claimed past an open seq"
    );
    assert_eq!(store.lane_committed("bulk").await.unwrap(), cursor);
    let done: i64 = admin
        .query_one("SELECT count(*)::bigint FROM index_claim WHERE done", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(done, 0, "hole fill wrote a done claim over an open seq");
}
