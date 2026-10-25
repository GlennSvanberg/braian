//! The relay publishes `changes` in commit order and does not drop rows when it restarts.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use braian_events::{INTERACTIVE_TOPIC, Subscription};
use braian_model::{DefinitionSet, Object, ObjectId};
use braian_rules::CompiledModel;
use braian_store::testing::TestDb;
use braian_store::{Actor, BatchKind, ObjectOp, PrincipalKind};
use serde_json::json;
use uuid::Uuid;

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
async fn a_thousand_changes_stay_ordered_when_the_relay_restarts() {
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
    let bus = braian_events::Bus::connect(&brokers).expect("kafka");
    let _ = bus.delete_topics().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let store = db.store().clone();
    let principal = store
        .upsert_principal("ada", PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let actor = Actor {
        actor: principal,
        on_behalf_of: None,
        kind: BatchKind::Interactive,
        source: "relay-test".into(),
    };
    let defs: DefinitionSet =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    store.put_definitions(&actor, &defs).await.unwrap();
    let model = CompiledModel::compile(&defs).unwrap();
    let db_name = db.database_url().rsplit('/').next().unwrap().to_owned();
    let slot = format!("s{db_name}");
    let port = 23000
        + db_name
            .bytes()
            .fold(0u16, |acc, byte| acc.wrapping_add(u16::from(byte)))
            % 1000;
    let listen = format!("127.0.0.1:{port}").parse().unwrap();
    let url = db.database_url();
    let mut task = Stop(Some(tokio::spawn({
        let store = store.clone();
        let brokers = brokers.clone();
        let slot = slot.clone();
        async move {
            braian_relay::run(
                braian_relay::Config {
                    database_url: url,
                    brokers,
                    slot,
                    listen,
                    lag_limit_bytes: 1024,
                    heartbeat_interval: braian_relay::HEARTBEAT_INTERVAL,
                },
                store,
            )
            .await
        }
    })));
    let ready = Instant::now();
    loop {
        if store.relay_lag_bytes(&slot).await.unwrap().is_some() {
            break;
        }
        if task.0.as_ref().is_some_and(|handle| handle.is_finished()) {
            match task.0.take().unwrap().await {
                Ok(Err(error)) => panic!("relay stopped before the slot existed: {error}"),
                Err(error) => panic!("relay task panicked: {error}"),
                Ok(Ok(())) => panic!("relay exited before the slot existed"),
            }
        }
        assert!(
            ready.elapsed() < Duration::from_secs(20),
            "replication slot was not created"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    write_items(&store, &actor, &model, &defs, 0, 1000).await;
    let group = format!("g{db_name}");
    let subscription = Subscription::subscribe(&brokers, &group, &[INTERACTIVE_TOPIC]).unwrap();
    let seqs = read_until(&subscription, 1000, Duration::from_secs(25)).await;
    assert_eq!(seqs.len(), 1000, "first wave seqs: {seqs:?}");
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "not ordered: {seqs:?}"
    );

    let confirmed = Instant::now();
    loop {
        if store
            .relay_lag_bytes(&slot)
            .await
            .unwrap()
            .unwrap_or(i64::MAX)
            < 65_536
        {
            break;
        }
        if confirmed.elapsed() > Duration::from_secs(8) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if let Some(handle) = task.0.take() {
        handle.abort();
    }
    let released = Instant::now();
    loop {
        match store.replication_slot_active(&slot).await.unwrap() {
            Some(false) | None => break,
            Some(true) if released.elapsed() > Duration::from_secs(8) => break,
            Some(true) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    write_items(&store, &actor, &model, &defs, 1000, 20).await;
    let url = db.database_url();
    let mut restarted = Stop(Some(tokio::spawn({
        let store = store.clone();
        let brokers = brokers.clone();
        let slot = slot.clone();
        async move {
            braian_relay::run(
                braian_relay::Config {
                    database_url: url,
                    brokers,
                    slot,
                    listen,
                    lag_limit_bytes: 1024,
                    heartbeat_interval: braian_relay::HEARTBEAT_INTERVAL,
                },
                store,
            )
            .await
        }
    })));
    tokio::time::sleep(Duration::from_secs(2)).await;
    if restarted
        .0
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        let error = restarted.0.take().unwrap().await;
        panic!("second relay stopped: {error:?}");
    }
    let last = *seqs.last().expect("first wave");
    let fresh = read_after(&subscription, last, 20, Duration::from_secs(12)).await;
    let expected: Vec<i64> = (last + 1..=last + 20).collect();
    assert_eq!(fresh, expected, "missing rows after restart: {fresh:?}");

    let metrics = tokio::task::spawn_blocking(move || http_get(port))
        .await
        .unwrap();
    assert!(metrics.contains("braian_relay_lag_bytes"), "{metrics}");
    if let Some(handle) = restarted.0.take() {
        handle.abort();
    }
}

struct Stop(Option<tokio::task::JoinHandle<anyhow::Result<()>>>);

impl Drop for Stop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

async fn write_items(
    store: &braian_store::Store,
    actor: &Actor,
    model: &CompiledModel,
    defs: &DefinitionSet,
    start: u32,
    count: u32,
) {
    let sku = model.attribute_by_key("sku").unwrap().id;
    let active = model.attribute_by_key("active").unwrap().id;
    let type_id = model.type_id("item").unwrap();
    let mut ops = Vec::with_capacity(count as usize);
    for index in start..start + count {
        let mut values = BTreeMap::new();
        values.insert(sku, json!(format!("K{index:04}")));
        values.insert(active, json!(true));
        ops.push(ObjectOp::Create(Object {
            id: ObjectId::from_uuid(Uuid::now_v7()),
            type_id,
            model_revision: defs.model_revision,
            groups: BTreeMap::new(),
            values,
        }));
    }
    store.write_batch(actor, ops).await.unwrap();
}

fn http_get(port: u16) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(b"GET /metrics HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body);
    body
}

async fn read_after(
    subscription: &Subscription,
    after: i64,
    want: usize,
    budget: Duration,
) -> Vec<i64> {
    let deadline = Instant::now() + budget;
    let mut seqs = Vec::new();
    while seqs.len() < want && Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), subscription.next()).await {
            Ok(Ok(event)) if event.object_id.is_some() && event.seq > after => seqs.push(event.seq),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("kafka read: {error}"),
            Err(_) => {}
        }
    }
    seqs
}

async fn read_until(subscription: &Subscription, want: usize, budget: Duration) -> Vec<i64> {
    let deadline = Instant::now() + budget;
    let mut seqs = Vec::new();
    while seqs.len() < want && Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), subscription.next()).await {
            Ok(Ok(event)) if event.object_id.is_some() => seqs.push(event.seq),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("kafka read: {error}"),
            Err(_) => {}
        }
    }
    seqs
}
