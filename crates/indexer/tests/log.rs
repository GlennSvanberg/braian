//! Coalescing happens on the events one claim returns, before Typesense.

use braian_events::ChangeEvent;
use braian_indexer::coalesce_events;
use braian_store::testing::{TestDb, append_changes};
use braian_store::{Actor, BatchKind, PrincipalKind};
use uuid::Uuid;

#[tokio::test]
async fn one_claim_coalesces_to_the_highest_seq_for_an_object() {
    let db = TestDb::create().await;
    let store = db.store();
    let principal = store
        .upsert_principal("ada", PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let who = Actor {
        actor: principal,
        on_behalf_of: None,
        kind: BatchKind::Interactive,
        source: "coalesce".into(),
    };
    let object = Uuid::now_v7();
    let seqs = append_changes(
        store,
        &who,
        &[Some(object), Some(object), Some(Uuid::now_v7())],
    )
    .await
    .unwrap();
    let claim = store
        .claim_changes(
            "interactive",
            Uuid::now_v7(),
            10,
            std::time::Duration::from_secs(30),
        )
        .await
        .unwrap()
        .expect("claim");
    let events: Vec<ChangeEvent> = claim
        .events
        .iter()
        .map(|event| ChangeEvent {
            seq: event.seq,
            op: event.op.clone(),
            object_id: event.object_id,
            version: event.version,
            batch_id: event.batch_id,
            kind: event.kind.clone(),
            at: event.at.to_rfc3339(),
        })
        .collect();
    let pending = coalesce_events(events);
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending.get(&object.to_string()).expect("object").seq,
        seqs[1]
    );
}
