//! A duplicate or a late event re-reads Postgres, so the index matches the current row.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use braian_model::{
    Cardinality, DefinitionSet, LinkLabel, LinkSpec, Object, ObjectId, ObjectPatch, ObjectType,
    TypeId, TypeKind, ValuePatch,
};
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

#[tokio::test]
async fn a_link_is_absent_from_search_and_lists_after_indexing_and_reindex() {
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
        source: "link-index".into(),
    };
    let mut defs: DefinitionSet =
        serde_json::from_str(include_str!("../../../testdata/rules/definitions.json")).unwrap();
    store.put_definitions(&actor, &defs).await.unwrap();
    let sku = defs
        .attributes
        .iter()
        .find(|attribute| attribute.key == "sku")
        .unwrap()
        .id;
    let active = defs
        .attributes
        .iter()
        .find(|attribute| attribute.key == "active")
        .unwrap()
        .id;
    let item_type = defs
        .types
        .iter()
        .find(|object_type| object_type.key == "item")
        .unwrap()
        .id;
    let link_type = TypeId::from_uuid(Uuid::now_v7());
    defs.types.push(ObjectType {
        id: link_type,
        key: "pairs_with".into(),
        labels: BTreeMap::new(),
        singular: BTreeMap::new(),
        plural: BTreeMap::new(),
        key_attribute: sku,
        attributes: vec![sku],
        structures: Some(Vec::new()),
        defaults: BTreeMap::new(),
        kind: TypeKind::Link,
        link: Some(LinkSpec {
            from_types: vec![item_type],
            to_types: vec![item_type],
            from_label: LinkLabel {
                key: "pairs_with".into(),
                labels: BTreeMap::new(),
            },
            to_label: LinkLabel {
                key: "paired_from".into(),
                labels: BTreeMap::new(),
            },
            from_cardinality: Cardinality::Many,
            to_cardinality: Cardinality::Many,
        }),
    });
    defs.model_revision += 1;
    store.put_definitions(&actor, &defs).await.unwrap();
    CompiledModel::compile(&defs).unwrap();
    let left = ObjectId::from_uuid(Uuid::now_v7());
    let right = ObjectId::from_uuid(Uuid::now_v7());
    let link = ObjectId::from_uuid(Uuid::now_v7());
    let item = |id, number: &str| {
        let mut values = BTreeMap::new();
        values.insert(sku, json!(number));
        values.insert(active, json!(true));
        ObjectOp::Create(Object {
            id,
            type_id: item_type,
            model_revision: defs.model_revision,
            groups: BTreeMap::new(),
            values,
        })
    };
    store
        .write_batch(&actor, vec![item(left, "LINK-A"), item(right, "LINK-B")])
        .await
        .unwrap();
    store
        .write_batch(
            &actor,
            vec![ObjectOp::CreateLink {
                object: Object {
                    id: link,
                    type_id: link_type,
                    model_revision: defs.model_revision,
                    groups: BTreeMap::new(),
                    values: BTreeMap::new(),
                },
                from_id: left,
                to_id: right,
            }],
        )
        .await
        .unwrap();

    let alias = format!("l{}", db.database_url().rsplit('/').next().unwrap());
    let search = Typesense::new(url, key).with_alias(alias);
    braian_indexer::index_objects(&store, &search, &[left, right], 1)
        .await
        .unwrap();
    assert_has_items(&search, left, right).await;
    plant_link(&search, link).await;
    assert!(
        search_ids(&search)
            .await
            .contains(&link.as_uuid().to_string()),
        "planted link document should be searchable before the indexer runs"
    );

    let indexer = braian_indexer::Indexer::connect(store.clone(), search.clone())
        .await
        .unwrap();
    let (shutdown, watch) = tokio::sync::watch::channel(false);
    let running = tokio::spawn(async move { indexer.run(watch).await });
    wait_until(&search, |ids| {
        ids.contains(&left.as_uuid().to_string())
            && ids.contains(&right.as_uuid().to_string())
            && !ids.contains(&link.as_uuid().to_string())
    })
    .await;
    assert_has_items(&search, left, right).await;
    assert_list(&store, left, right, link).await;
    shutdown.send(true).unwrap();
    running.await.unwrap().unwrap();

    plant_link(&search, link).await;
    assert!(
        search_ids(&search)
            .await
            .contains(&link.as_uuid().to_string())
    );
    braian_indexer::index_objects(&store, &search, &[link], 50)
        .await
        .unwrap();
    assert_has_items(&search, left, right).await;
    assert!(
        !search_ids(&search)
            .await
            .contains(&link.as_uuid().to_string())
    );

    plant_link(&search, link).await;
    assert!(
        search_ids(&search)
            .await
            .contains(&link.as_uuid().to_string())
    );
    braian_indexer::reindex(&store, &search).await.unwrap();
    assert_has_items(&search, left, right).await;
    assert!(
        !search_ids(&search)
            .await
            .contains(&link.as_uuid().to_string()),
        "reindex left the link in the alias"
    );
    assert_list(&store, left, right, link).await;
    if let Some(name) = search.alias_target().await.unwrap() {
        let _ = search.drop_collection(&name).await;
    }
}

async fn search_ids(search: &Typesense) -> Vec<String> {
    let page = search
        .search(&SearchQuery {
            text: String::new(),
            language: "sv".into(),
            filter_by: String::new(),
            sort_by: String::new(),
            group_by: String::new(),
            page: 1,
            per_page: 20,
        })
        .await
        .unwrap();
    page.hits.into_iter().map(|hit| hit.id).collect()
}

async fn wait_until(search: &Typesense, ready: impl Fn(&[String]) -> bool) {
    let started = Instant::now();
    loop {
        let ids = search_ids(search).await;
        if ready(&ids) {
            return;
        }
        if started.elapsed() > Duration::from_secs(20) {
            panic!("search still shows {ids:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn assert_has_items(search: &Typesense, left: ObjectId, right: ObjectId) {
    let ids = search_ids(search).await;
    assert!(
        ids.contains(&left.as_uuid().to_string()),
        "missing {left} in {ids:?}"
    );
    assert!(
        ids.contains(&right.as_uuid().to_string()),
        "missing {right} in {ids:?}"
    );
    assert_eq!(ids.len(), 2, "search returned {ids:?}");
}

async fn assert_list(store: &braian_store::Store, left: ObjectId, right: ObjectId, link: ObjectId) {
    let heads = store.list_objects_page(None, None, None, 20).await.unwrap();
    let ids: Vec<Uuid> = heads.iter().map(|row| row.0).collect();
    assert!(ids.contains(&left.as_uuid()));
    assert!(ids.contains(&right.as_uuid()));
    assert!(!ids.contains(&link.as_uuid()), "list returned the link");
    assert_eq!(heads.len(), 2);
}

async fn plant_link(search: &Typesense, link: ObjectId) {
    search
        .import(
            search.alias(),
            &[json!({
                "id": link.as_uuid().to_string(),
                "key": "",
                "type_key": "pairs_with",
                "seq": 1,
                "version": 1,
                "deleted": false,
                "groups": []
            })],
        )
        .await
        .unwrap();
}
