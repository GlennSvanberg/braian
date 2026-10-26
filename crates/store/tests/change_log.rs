//! The change log is `changes`. Workers lease seq ranges; a crash replays them.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use braian_store::admin;
use braian_store::testing::{TestDb, append_changes, expire_claims};
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
