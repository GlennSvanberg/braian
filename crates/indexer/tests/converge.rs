//! A duplicate or a late event re-reads Postgres, so the index matches the current row.

use std::collections::BTreeMap;

use braian_model::{DefinitionSet, Object, ObjectId, ObjectPatch, ValuePatch};
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
async fn stale_events_and_reindex_converge_on_postgres() {
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
        kind: BatchKind::Interactive,
        source: "index-test".into(),
    };
    let defs: DefinitionSet =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    store.put_definitions(&actor, &defs).await.unwrap();
    let model = CompiledModel::compile(&defs).unwrap();
    let sku = model.attribute_by_key("sku").unwrap().id;
    let active = model.attribute_by_key("active").unwrap().id;
    let id = ObjectId::from_uuid(Uuid::now_v7());
    let mut values = BTreeMap::new();
    values.insert(sku, json!("SKU-1"));
    values.insert(active, json!(true));
    store
        .write_batch(
            &actor,
            vec![ObjectOp::Create(Object {
                id,
                type_id: model.type_id("item").unwrap(),
                model_revision: defs.model_revision,
                groups: BTreeMap::new(),
                values,
            })],
        )
        .await
        .unwrap();

    let alias = format!("t{}", db.database_url().rsplit('/').next().unwrap());
    let search = Typesense::new(url, key).with_alias(alias);
    braian_indexer::index_object(&store, &search, id, 1)
        .await
        .unwrap();
    braian_indexer::index_object(&store, &search, id, 1)
        .await
        .unwrap();

    let patch = ObjectPatch {
        values: ValuePatch {
            attributes: BTreeMap::from([(
                active,
                braian_model::AttributePatch::Replace {
                    value: json!(false),
                },
            )]),
        },
        ..ObjectPatch::default()
    };
    store
        .write_batch(
            &actor,
            vec![ObjectOp::Update {
                id,
                patch,
                base_version: 1,
                read_version: 1,
            }],
        )
        .await
        .unwrap();
    // An old sequence must not put the previous value back.
    let doc = braian_indexer::index_object(&store, &search, id, 1)
        .await
        .unwrap();
    assert_eq!(doc["key"], "SKU-1");
    let active_field = braian_model::attribute_index_field(active, None);
    assert_eq!(doc[&active_field], "false");

    let page = search
        .search(&SearchQuery {
            text: "SKU-1".to_owned(),
            language: "sv".into(),
            filter_by: String::new(),
            sort_by: String::new(),
            group_by: String::new(),
            page: 1,
            per_page: 10,
        })
        .await
        .unwrap();
    assert_eq!(page.found, 1);

    let before = search.alias_target().await.unwrap();
    let seq = braian_indexer::reindex(&store, &search).await.unwrap();
    assert!(seq >= 1);
    let after = search.alias_target().await.unwrap();
    assert_ne!(before, after);
    let page = search
        .search(&SearchQuery {
            text: "SKU-1".to_owned(),
            language: "sv".into(),
            ..SearchQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.found, 1);
    assert_eq!(page.hits[0].document[&active_field], "false");
    if let Some(name) = after {
        let _ = search.drop_collection(&name).await;
    }
}
