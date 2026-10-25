//! One batch read indexes every object, including when the same id is listed twice.

use std::collections::BTreeMap;

use braian_model::{DefinitionSet, Object, ObjectId};
use braian_rules::CompiledModel;
use braian_search::{SearchQuery, Typesense};
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
            unsafe { std::env::set_var(key, value.trim().trim_matches('"')) };
        }
    }
}

#[tokio::test]
async fn a_batch_of_objects_is_indexed_together() {
    load_env();
    let Ok(url) = std::env::var("TYPESENSE_URL") else {
        eprintln!("skip: TYPESENSE_URL is unset");
        return;
    };
    let Ok(key) = std::env::var("TYPESENSE_API_KEY") else {
        eprintln!("skip: TYPESENSE_API_KEY is unset");
        return;
    };
    if url.is_empty() {
        eprintln!("skip: TYPESENSE_URL is empty");
        return;
    }
    let db = TestDb::create().await;
    let store = db.store().clone();
    let principal = store
        .upsert_principal("ada", PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let actor = Actor {
        actor: principal,
        on_behalf_of: None,
        kind: BatchKind::Bulk,
        source: "batch-index".into(),
    };
    let defs: DefinitionSet =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    store.put_definitions(&actor, &defs).await.unwrap();
    let model = CompiledModel::compile(&defs).unwrap();
    let sku = model.attribute_by_key("sku").unwrap().id;
    let active = model.attribute_by_key("active").unwrap().id;
    let mut ids = Vec::new();
    let mut ops = Vec::new();
    for index in 0..4 {
        let id = ObjectId::from_uuid(Uuid::now_v7());
        ids.push(id);
        let mut values = BTreeMap::new();
        values.insert(sku, json!(format!("BATCH-{index}")));
        values.insert(active, json!(true));
        ops.push(ObjectOp::Create(Object {
            id,
            type_id: model.type_id("item").unwrap(),
            model_revision: defs.model_revision,
            groups: BTreeMap::new(),
            values,
        }));
    }
    store.write_batch(&actor, ops).await.unwrap();

    let mut requested = ids.clone();
    requested.push(ids[0]);
    let alias = format!("b{}", db.database_url().rsplit('/').next().unwrap());
    let search = Typesense::new(url, key).with_alias(alias);
    braian_indexer::index_objects(&store, &search, &requested, 7)
        .await
        .unwrap();
    assert!(store.indexer_seq().await.unwrap() >= 7);

    for index in 0..4 {
        let page = search
            .search(&SearchQuery {
                text: format!("BATCH-{index}"),
                language: "sv".into(),
                ..SearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(page.found, 1, "BATCH-{index}");
        assert_eq!(page.hits[0].document["key"], format!("BATCH-{index}"));
    }
    if let Some(name) = search.alias_target().await.unwrap() {
        let _ = search.drop_collection(&name).await;
    }
}
