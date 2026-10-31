//! Synthetic food catalog shaped like Open Food Facts. Safe to run more than once.
//!
//! Hero objects live in `demo/objects.json`. `BRAIAN_SEED_COUNT` (default 240)
//! scales the same deterministic generator. Together with the heroes that is
//! about 250 objects: suppliers, products and articles.

use std::collections::{BTreeMap, HashSet};

use braian_model::{AttributeId, Cardinality, DefinitionSet, Object, ObjectId, TypeId};
use braian_rules::CompiledModel;
use braian_search::Typesense;
use braian_store::{Actor, BatchKind, Error, ObjectOp, PrincipalId, ProposalRow, Store};
use serde_json::{Value, json};
use uuid::Uuid;

const DEFINITIONS: &str = include_str!("../demo/definitions.json");
const OBJECTS: &str = include_str!("../demo/objects.json");

pub async fn run(store: &Store, search: Option<&Typesense>) -> anyhow::Result<()> {
    let email = std::env::var("BRAIAN_DEMO_EMAIL").map_err(|_| {
        anyhow::anyhow!(
            "BRAIAN_DEMO_EMAIL is not set. Copy .env.example to .env; the demo login is documented there."
        )
    })?;
    let password = std::env::var("BRAIAN_DEMO_PASSWORD").map_err(|_| {
        anyhow::anyhow!(
            "BRAIAN_DEMO_PASSWORD is not set. See .env.example, do not invent a password."
        )
    })?;
    let user = match store.user_by_email(&email).await? {
        Some(user) => user,
        None => {
            let hash = braian_api::hash_password(&password)
                .map_err(|error| anyhow::anyhow!("password hash: {error}"))?;
            store
                .create_user(&email, &hash, "admin", "Demo Admin")
                .await?
        }
    };
    if let Ok(token) = std::env::var("BRAIAN_DEMO_TOKEN")
        && !token.is_empty()
    {
        let hashed = braian_api::token_hash(&token);
        if store.api_token(&hashed).await?.is_none() {
            store
                .insert_api_token(&user, "demo", "api", &hashed)
                .await?;
        }
    }
    println!(
        "demo login comes from BRAIAN_DEMO_EMAIL and BRAIAN_DEMO_PASSWORD in .env (see .env.example), not from the source tree"
    );

    let actor = Actor {
        actor: user.principal_id,
        on_behalf_of: None,
        kind: BatchKind::Interactive,
        source: "seed".into(),
    };
    // Views and selections are translated against the live definition set.
    // Install it first so the first seed can store ids, not keys.
    let defs = ensure_definitions(store, &actor).await?;
    seed_demo_surface(store, &user, &defs).await?;
    let model =
        CompiledModel::compile(&defs).map_err(|errors| anyhow::anyhow!("{}", errors[0].message))?;
    let links = LinkSeed {
        model: &model,
        defs: &defs,
        revision: defs.model_revision,
    };
    let rows: Vec<Value> = serde_json::from_str(OBJECTS)?;
    for row in &rows {
        let type_key = row["type"].as_str().unwrap_or("");
        let type_id = model
            .type_id(type_key)
            .ok_or_else(|| anyhow::anyhow!("unknown type {type_key}"))?;
        let key = row["values"]["number"].as_str().unwrap_or("");
        let id = ObjectId::from_uuid(
            Uuid::parse_str(row["id"].as_str().unwrap_or_default())
                .map_err(|error| anyhow::anyhow!("object id: {error}"))?,
        );
        // A previous run, or an edit that changed the business key, still owns this id.
        let present = store.get_object(id).await?.is_some()
            || store.get_by_key(type_id, key).await?.is_some();
        if present {
            seed_row_links(store, &actor, &links, row).await?;
            continue;
        }
        let groups = match row["group"].as_str().filter(|key| !key.is_empty()) {
            Some(group_key) => {
                let structure = model
                    .structure_id("food")
                    .ok_or_else(|| anyhow::anyhow!("food is missing"))?;
                let group = model
                    .group_id("food", group_key)
                    .ok_or_else(|| anyhow::anyhow!("unknown group {group_key}"))?;
                BTreeMap::from([(structure, group)])
            }
            None => BTreeMap::new(),
        };
        let mut values = BTreeMap::new();
        let Some(raw) = row["values"].as_object() else {
            continue;
        };
        for (name, value) in raw {
            let attribute = model
                .attribute_by_key(name)
                .ok_or_else(|| anyhow::anyhow!("unknown attribute {name}"))?;
            values.insert(attribute.id, value.clone());
        }
        let object = Object {
            id,
            type_id,
            model_revision: defs.model_revision,
            groups,
            values,
        };
        let errors = model.validate(&object);
        if !errors.is_empty() {
            return Err(anyhow::anyhow!(
                "seed {} failed validation: {}",
                key,
                errors[0].message
            ));
        }
        store
            .write_batch(&actor, vec![ObjectOp::Create(object)])
            .await?;
        seed_row_links(store, &actor, &links, row).await?;
    }

    // The seeded proposal points at ART-CHED-400. Heroes are written above;
    // seed_demo_surface runs before any of those rows exist, so a first seed
    // against an empty database would otherwise skip the proposal.
    seed_proposal(store, user.principal_id).await?;

    let target = std::env::var("BRAIAN_SEED_COUNT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SEED_COUNT);
    if target > 0 {
        seed_catalog(store, &actor, &links, target).await?;
    }

    if let Some(search) = search {
        braian_indexer::reindex(store, search).await?;
    }
    Ok(())
}

async fn ensure_definitions(store: &Store, actor: &Actor) -> anyhow::Result<DefinitionSet> {
    match store.definition_set().await {
        Ok(defs) => Ok(defs),
        Err(Error::NoDefinitions) => {
            let defs: DefinitionSet = serde_json::from_str(DEFINITIONS)?;
            CompiledModel::compile(&defs)
                .map_err(|errors| anyhow::anyhow!("{}", errors[0].message))?;
            store.put_definitions(actor, &defs).await?;
            Ok(defs)
        }
        Err(error) => Err(error.into()),
    }
}

async fn seed_demo_surface(
    store: &Store,
    user: &braian_store::UserRecord,
    defs: &DefinitionSet,
) -> anyhow::Result<()> {
    // Removed demo workspace: drop it from databases seeded before.
    store
        .delete_workspace(Uuid::parse_str("018f2000-0000-7000-8000-000000000001")?)
        .await?;
    let workspaces = [
        (
            "018f2000-0000-7000-8000-000000000002",
            "Data model",
            false,
            "braian-model",
        ),
        (
            "018f2000-0000-7000-8000-000000000003",
            "Products",
            false,
            "braian-type-product",
        ),
        (
            "018f2000-0000-7000-8000-000000000005",
            "Articles",
            false,
            "braian-type-article",
        ),
        (
            "018f2000-0000-7000-8000-000000000007",
            "Suppliers",
            false,
            "braian-type-supplier",
        ),
        (
            "018f2000-0000-7000-8000-000000000004",
            "Review",
            false,
            "braian-review",
        ),
    ];
    for (id, name, locked, share) in workspaces {
        store
            .save_workspace(
                Uuid::parse_str(id)?,
                user.id,
                name,
                locked,
                Some(share),
                &layout(id),
            )
            .await?;
    }
    // Removed demo views: drop them from databases seeded before. View first, then its type.
    store
        .delete_view(Uuid::parse_str("018f2000-0000-7000-8000-000000000404")?)
        .await?;
    store.delete_view_type("leverantor").await?; // english-check: allow-line
    save_views(store, defs).await?;
    // Selections belong to a type. The article Cheese keeps the food:cheese group.
    // Organic is assigned to article: the generator marks more articles than
    // products with organic_eu, and this seed runs before those objects exist.
    // A second Cheese is the product example, so each type workspace has one.
    let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let product = Uuid::parse_str("018f0000-0000-7000-8000-000000000200")?;
    save_seed_assortment(
        store,
        defs,
        Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1")?,
        user.id,
        article,
        "Cheese",
        json!({ "group": "food:cheese" }),
    )
    .await?;
    save_seed_assortment(
        store,
        defs,
        Uuid::parse_str("018f2000-0000-7000-8000-0000000000a2")?,
        user.id,
        article,
        "Organic",
        organic_assortment_query(),
    )
    .await?;
    save_seed_assortment(
        store,
        defs,
        Uuid::parse_str("018f2000-0000-7000-8000-0000000000a3")?,
        user.id,
        product,
        "Cheese",
        json!({ "group": "food:cheese" }),
    )
    .await?;
    // Status check is a registered stub, not a runner. Drop it from databases seeded before.
    store.delete_job_type("dq.status").await?;
    store
        .register_job_type("bulk.write", "Bulk write", &["status".into()], None)
        .await?;
    seed_job_run(
        store,
        "018f2000-0000-7000-8000-0000000000b2",
        "bulk.write",
        user.principal_id,
    )
    .await?;
    Ok(())
}

/// Saved query for the Organic assortment.
///
/// Membership is the filterable `labels` value `organic_eu`. The search filter
/// is `a_labels:=organic_eu`. A text query for "Organic" is not used.
fn organic_assortment_query() -> Value {
    json!({ "labels": "organic_eu" })
}

fn catalog_layout(tree: &str, assortments: &str, table: &str, right: &[&str]) -> Value {
    split(
        "horizontal",
        0.22,
        tabs(&[tree, assortments]),
        split("horizontal", 0.62, tabs(&[table]), tabs(right)),
    )
}

fn tabs(ids: &[&str]) -> Value {
    json!({ "type": "tabs", "views": ids, "active": 0 })
}

fn split(dir: &str, first_ratio: f64, first: Value, second: Value) -> Value {
    json!({
        "type": "split",
        "dir": dir,
        "ratios": [first_ratio, 1.0 - first_ratio],
        "children": [first, second],
    })
}

fn layout(workspace: &str) -> Value {
    match workspace {
        "018f2000-0000-7000-8000-000000000003" => catalog_layout(
            "018f2000-0000-7000-8000-000000000301",
            "018f2000-0000-7000-8000-000000000308",
            "018f2000-0000-7000-8000-000000000302",
            &[
                "018f2000-0000-7000-8000-000000000303",
                "018f2000-0000-7000-8000-000000000304",
                "018f2000-0000-7000-8000-000000000305",
                "018f2000-0000-7000-8000-000000000309",
            ],
        ),
        "018f2000-0000-7000-8000-000000000005" => catalog_layout(
            "018f2000-0000-7000-8000-000000000501",
            "018f2000-0000-7000-8000-000000000508",
            "018f2000-0000-7000-8000-000000000306",
            &[
                "018f2000-0000-7000-8000-000000000503",
                "018f2000-0000-7000-8000-000000000504",
                "018f2000-0000-7000-8000-000000000505",
                "018f2000-0000-7000-8000-000000000509",
            ],
        ),
        "018f2000-0000-7000-8000-000000000007" => split(
            "horizontal",
            0.22,
            tabs(&["018f2000-0000-7000-8000-000000000701"]),
            split(
                "horizontal",
                0.62,
                tabs(&["018f2000-0000-7000-8000-000000000702"]),
                tabs(&[
                    "018f2000-0000-7000-8000-000000000703",
                    "018f2000-0000-7000-8000-000000000704",
                    "018f2000-0000-7000-8000-000000000705",
                ]),
            ),
        ),
        "018f2000-0000-7000-8000-000000000002" => split(
            "horizontal",
            0.22,
            tabs(&["018f2000-0000-7000-8000-000000000201"]),
            split(
                "horizontal",
                0.58,
                tabs(&[
                    "018f2000-0000-7000-8000-000000000202",
                    "018f2000-0000-7000-8000-000000000204",
                ]),
                tabs(&["018f2000-0000-7000-8000-000000000203"]),
            ),
        ),
        "018f2000-0000-7000-8000-000000000004" => split(
            "horizontal",
            0.3,
            tabs(&["018f2000-0000-7000-8000-000000000405"]),
            split(
                "horizontal",
                0.74,
                tabs(&["018f2000-0000-7000-8000-000000000401"]),
                tabs(&[
                    "018f2000-0000-7000-8000-000000000402",
                    "018f2000-0000-7000-8000-000000000403",
                ]),
            ),
        ),
        _ => tabs(&[]),
    }
}

/// Inserts a missing selection with ids in `query`.
///
/// An existing row keeps its name, type and query. Key tokens still stored in
/// that query are rewritten to ids; the rest of the document stays.
async fn save_seed_assortment(
    store: &Store,
    defs: &DefinitionSet,
    id: Uuid,
    owner: Uuid,
    type_id: Uuid,
    name: &str,
    mut query: Value,
) -> anyhow::Result<()> {
    if let Some(existing) = store.assortment(id).await? {
        let stored = existing.get("query").cloned().unwrap_or(Value::Null);
        let mut repaired = stored.clone();
        braian_model::translate_assortment_query(&mut repaired, defs);
        if repaired != stored {
            store.update_assortment_query(id, &repaired).await?;
        }
        return Ok(());
    }
    braian_model::translate_assortment_query(&mut query, defs);
    store
        .save_assortment(id, owner, type_id, name, &query)
        .await?;
    Ok(())
}

async fn save_views(store: &Store, defs: &DefinitionSet) -> anyhow::Result<()> {
    let views = [
        (
            "018f2000-0000-7000-8000-000000000201",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Model",
            "left",
            0,
            json!({"mode": "tree"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000202",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Attributes",
            "center",
            0,
            json!({"mode": "classes"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000203",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Preview",
            "right",
            0,
            json!({"mode": "card"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000204",
            "018f2000-0000-7000-8000-000000000002",
            "attribute_definitions",
            "Attribute definitions",
            "center",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000301",
            "018f2000-0000-7000-8000-000000000003",
            "structure_tree",
            "Food",
            "left",
            0,
            json!({"expanded": ["dairy", "beverages"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000308",
            "018f2000-0000-7000-8000-000000000003",
            "assortment_list",
            "Selections",
            "left",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000302",
            "018f2000-0000-7000-8000-000000000003",
            "object_table",
            "Products",
            "center",
            0,
            json!({"columns": ["number", "name", "brand", "nutriscore", "status"], "sort": "number", "object_type": "018f0000-0000-7000-8000-000000000200", "type_key": "product"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000303",
            "018f2000-0000-7000-8000-000000000003",
            "object_detail",
            "Detail",
            "right",
            0,
            json!({"language": "en"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000304",
            "018f2000-0000-7000-8000-000000000003",
            "graph",
            "Graph",
            "right",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000305",
            "018f2000-0000-7000-8000-000000000003",
            "history",
            "History",
            "right",
            2,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000401",
            "018f2000-0000-7000-8000-000000000004",
            "proposal",
            "Proposal",
            "center",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000405",
            "018f2000-0000-7000-8000-000000000004",
            "proposal_list",
            "Proposals",
            "left",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000402",
            "018f2000-0000-7000-8000-000000000004",
            "batch_log",
            "Batch log",
            "right",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000403",
            "018f2000-0000-7000-8000-000000000004",
            "job_history",
            "Jobs",
            "right",
            1,
            json!({"type": "bulk.write"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000306",
            "018f2000-0000-7000-8000-000000000005",
            "object_table",
            "Articles",
            "center",
            1,
            json!({"columns": ["number", "name", "gtin", "storage", "status"], "sort": "number", "object_type": "018f0000-0000-7000-8000-000000000201", "type_key": "article"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000702",
            "018f2000-0000-7000-8000-000000000007",
            "object_table",
            "Suppliers",
            "center",
            0,
            json!({"columns": ["018f0000-0000-7000-8000-000000000100", "018f0000-0000-7000-8000-000000000101", "018f0000-0000-7000-8000-00000000012b", "018f0000-0000-7000-8000-00000000012c", "018f0000-0000-7000-8000-000000000103"], "sort": "number", "object_type": "018f0000-0000-7000-8000-000000000202", "type_key": "supplier"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000309",
            "018f2000-0000-7000-8000-000000000003",
            "class_attributes",
            "Group attributes",
            "right",
            3,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000501",
            "018f2000-0000-7000-8000-000000000005",
            "structure_tree",
            "Food",
            "left",
            0,
            json!({"expanded": ["dairy", "beverages"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000508",
            "018f2000-0000-7000-8000-000000000005",
            "assortment_list",
            "Selections",
            "left",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000503",
            "018f2000-0000-7000-8000-000000000005",
            "object_detail",
            "Detail",
            "right",
            0,
            json!({"language": "en"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000504",
            "018f2000-0000-7000-8000-000000000005",
            "graph",
            "Graph",
            "right",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000505",
            "018f2000-0000-7000-8000-000000000005",
            "history",
            "History",
            "right",
            2,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000509",
            "018f2000-0000-7000-8000-000000000005",
            "class_attributes",
            "Group attributes",
            "right",
            3,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000701",
            "018f2000-0000-7000-8000-000000000007",
            "assortment_list",
            "Selections",
            "left",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000703",
            "018f2000-0000-7000-8000-000000000007",
            "object_detail",
            "Detail",
            "right",
            0,
            json!({"language": "en"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000704",
            "018f2000-0000-7000-8000-000000000007",
            "graph",
            "Graph",
            "right",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000705",
            "018f2000-0000-7000-8000-000000000007",
            "history",
            "History",
            "right",
            2,
            json!({}),
        ),
    ];
    for (id, workspace, kind, title, pane, position, mut state) in views {
        let id = Uuid::parse_str(id)?;
        // A later boot must not put the template back over a saved layout.
        // Key tokens left in an existing document are still rewritten to ids.
        if let Some(stored) = store.view_state(id).await? {
            let mut repaired = stored.clone();
            braian_model::translate_view_state(&mut repaired, defs);
            if repaired != stored {
                store.update_view_state(id, &repaired).await?;
            }
            continue;
        }
        braian_model::translate_view_state(&mut state, defs);
        store
            .save_view(
                id,
                Uuid::parse_str(workspace)?,
                kind,
                title,
                pane,
                position,
                &state,
            )
            .await?;
    }
    Ok(())
}

fn synthetic_id(index: u32) -> ObjectId {
    let mut bytes = [
        0x01, 0x8f, 0x30, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    let be = index.to_be_bytes();
    bytes[12..16].copy_from_slice(&be);
    ObjectId::from_uuid(Uuid::from_bytes(bytes))
}

const DEFAULT_SEED_COUNT: u32 = 240;

const LEAVES: [&str; 17] = [
    "milk",
    "cheese",
    "yoghurt",
    "juice",
    "soft_drinks",
    "coffee_tea",
    "plant_drinks",
    "pasta_rice",
    "cereals",
    "canned",
    "spices",
    "bread",
    "chips",
    "chocolate",
    "fruit_vegetables",
    "meat_fish",
    "frozen",
];

/// Suppliers, products and articles for a generator target.
///
/// The ratio matches the demo shape (about 15 / 60 / 175 at 250 objects,
/// heroes included separately). Every product receives 1 to 5 articles
/// whenever the target is large enough to hold that.
fn catalog_plan(target: u32) -> (u32, u32, u32) {
    if target == 0 {
        return (0, 0, 0);
    }
    let mut suppliers = target * 13 / DEFAULT_SEED_COUNT;
    let mut products = target * 57 / DEFAULT_SEED_COUNT;
    if target >= 3 {
        suppliers = suppliers.max(1);
        products = products.max(1);
    } else if target == 2 {
        suppliers = 1;
        products = 1;
    } else {
        return (1, 0, 0);
    }
    if suppliers + products > target {
        products = target.saturating_sub(suppliers).min(products);
        if suppliers > target {
            suppliers = target;
            products = 0;
        }
    }
    let mut articles = target - suppliers - products;
    if products > 0 && articles > 0 && articles < products {
        products = articles;
        articles = target - suppliers - products;
    }
    (suppliers, products, articles)
}

fn leaf_for(index: u32) -> &'static str {
    LEAVES[(index as usize) % LEAVES.len()]
}

fn uses_volume(group: &str) -> bool {
    matches!(group, "milk" | "juice" | "soft_drinks" | "plant_drinks")
}

fn storage_for(group: &str) -> &'static str {
    match group {
        "milk" | "cheese" | "yoghurt" | "meat_fish" => "chilled",
        "frozen" => "frozen",
        _ => "ambient",
    }
}

fn food_name(group: &str, index: u32) -> String {
    let words: &[&str] = match group {
        "milk" => &[
            "Whole milk",
            "Semi-skimmed milk",
            "Skimmed milk",
            "Buttermilk",
        ],
        "cheese" => &["Cheddar", "Gouda", "Mozzarella", "Feta"],
        "yoghurt" => &[
            "Natural yoghurt",
            "Vanilla yoghurt",
            "Greek-style yoghurt",
            "Skyr",
        ],
        "juice" => &["Apple juice", "Orange juice", "Pear juice", "Grape juice"],
        "soft_drinks" => &["Lemon soda", "Cola", "Ginger ale", "Orange soda"],
        "coffee_tea" => &["Filter coffee", "Espresso", "Black tea", "Green tea"],
        "plant_drinks" => &["Oat drink", "Soya drink", "Almond drink", "Rice drink"],
        "pasta_rice" => &["Spaghetti", "Penne", "Jasmine rice", "Basmati rice"],
        "cereals" => &["Oat flakes", "Muesli", "Corn flakes", "Puffed rice"],
        "canned" => &["Chopped tomatoes", "Chickpeas", "Red lentils", "Sweetcorn"],
        "spices" => &["Black pepper", "Cinnamon", "Paprika", "Cumin"],
        "bread" => &[
            "Sourdough loaf",
            "Rye bread",
            "Wholemeal roll",
            "Seeded loaf",
        ],
        "chips" => &[
            "Salted chips",
            "Paprika chips",
            "Lentil chips",
            "Potato crisps",
        ],
        "chocolate" => &[
            "Dark chocolate",
            "Milk chocolate",
            "Hazelnut chocolate",
            "Cocoa nibs",
        ],
        "fruit_vegetables" => &["Apples", "Bananas", "Carrots", "Tomatoes"],
        "meat_fish" => &[
            "Minced beef",
            "Chicken fillet",
            "Salmon fillet",
            "Bean burger",
        ],
        "frozen" => &[
            "Frozen peas",
            "Frozen berries",
            "Fish fingers",
            "Vegetable mix",
        ],
        _ => &["Pantry item", "Grocery item", "Store item", "Kitchen item"],
    };
    let organic = if index.is_multiple_of(4) {
        "Organic "
    } else {
        ""
    };
    format!(
        "{organic}{} {}",
        words[(index as usize) % words.len()],
        index + 1
    )
}

fn brand_for(index: u32) -> &'static str {
    const BRANDS: [&str; 8] = [
        "Northmill",
        "Harbor & Rye",
        "Little Kiln",
        "Meadow Route",
        "Bright Tin",
        "Orchard Lane",
        "Copper Kettle",
        "Field Note",
    ];
    BRANDS[(index as usize) % BRANDS.len()]
}

fn status_code(index: u32) -> &'static str {
    match index % 5 {
        0 => "draft",
        4 => "discontinued",
        _ => "ok",
    }
}

fn country_code(index: u32) -> &'static str {
    const CODES: [&str; 8] = ["se", "dk", "fi", "de", "fr", "it", "nl", "es"];
    CODES[(index as usize) % CODES.len()]
}

fn allergen_for(group: &str) -> &'static str {
    match group {
        "milk" | "cheese" | "yoghurt" => "milk",
        "bread" | "pasta_rice" | "cereals" => "gluten",
        "meat_fish" => "fish",
        "chocolate" | "chips" => "soybeans",
        "plant_drinks" => "gluten",
        _ => "mustard",
    }
}

fn put_attr(
    model: &CompiledModel,
    values: &mut BTreeMap<AttributeId, Value>,
    key: &str,
    value: Value,
) -> anyhow::Result<()> {
    let attribute = model
        .attribute_by_key(key)
        .ok_or_else(|| anyhow::anyhow!("unknown attribute {key}"))?;
    values.insert(attribute.id, value);
    Ok(())
}

fn grams(amount: u32) -> Value {
    json!({ "value": amount.to_string(), "unit": "g" })
}

fn whole(amount: u32) -> Value {
    json!({ "value": amount.to_string() })
}

struct Slot {
    index: u32,
    type_key: &'static str,
    group_key: Option<&'static str>,
    product: Option<u32>,
    supplier: Option<u32>,
}

fn slot_for(index: u32, suppliers: u32, products: u32) -> Slot {
    if index < suppliers {
        return Slot {
            index,
            type_key: "supplier",
            group_key: None,
            product: None,
            supplier: None,
        };
    }
    if index < suppliers + products {
        return Slot {
            index,
            type_key: "product",
            group_key: Some(leaf_for(index)),
            product: None,
            supplier: None,
        };
    }
    let article = index - suppliers - products;
    let product = suppliers + (article % products.max(1));
    Slot {
        index,
        type_key: "article",
        group_key: Some(leaf_for(product)),
        product: Some(product),
        supplier: Some(article % suppliers.max(1)),
    }
}

fn fill_values(model: &CompiledModel, slot: &Slot) -> anyhow::Result<BTreeMap<AttributeId, Value>> {
    let index = slot.index;
    let key = format!("SYN-{index:06}");
    let mut values = BTreeMap::new();
    put_attr(model, &mut values, "number", json!(key))?;
    put_attr(model, &mut values, "status", json!(status_code(index)))?;
    match slot.type_key {
        "supplier" => {
            put_attr(
                model,
                &mut values,
                "active",
                json!(!index.is_multiple_of(17)),
            )?;
            put_attr(
                model,
                &mut values,
                "name",
                json!({"en": format!("{} Foods {}", brand_for(index), index + 1)}),
            )?;
            put_attr(
                model,
                &mut values,
                "org_number",
                json!(format!(
                    "556{:03}-{:04}",
                    index % 1000,
                    1000 + (index % 9000)
                )),
            )?;
            put_attr(
                model,
                &mut values,
                "gln",
                json!(format!(
                    "{:013}",
                    730_000_000_000u64 + u64::from(index % 1_000_000)
                )),
            )?;
            put_attr(model, &mut values, "country", json!(country_code(index)))?;
            let cities = ["York", "Bristol", "Aberdeen", "Leeds", "Norwich", "Dover"];
            put_attr(
                model,
                &mut values,
                "city",
                json!(cities[(index as usize) % cities.len()]),
            )?;
            put_attr(
                model,
                &mut values,
                "website",
                json!(format!("https://supplier{index}.example")),
            )?;
            let certs = match index % 4 {
                0 => json!(["brc", "krav"]),
                1 => json!(["ifs"]),
                2 => json!(["msc", "fairtrade"]),
                _ => json!(["brc"]),
            };
            put_attr(model, &mut values, "certifications", certs)?;
            put_attr(
                model,
                &mut values,
                "lead_time_days",
                whole(2 + (index % 12)),
            )?;
        }
        "product" => {
            let group = slot.group_key.unwrap_or("bread");
            let name = food_name(group, index);
            put_attr(model, &mut values, "name", json!({ "en": name }))?;
            put_attr(
                model,
                &mut values,
                "description",
                json!({ "en": format!("{name}. Synthetic grocery recipe for the demo.") }),
            )?;
            put_attr(model, &mut values, "brand", json!(brand_for(index)))?;
            put_attr(
                model,
                &mut values,
                "ingredients",
                json!({ "en": format!("Water, {group} ingredients, salt.") }),
            )?;
            put_attr(
                model,
                &mut values,
                "allergens",
                json!([allergen_for(group)]),
            )?;
            put_attr(model, &mut values, "traces", json!([]))?;
            let mut labels = Vec::new();
            if index.is_multiple_of(4) {
                labels.push("organic_eu");
            }
            if matches!(group, "plant_drinks" | "fruit_vegetables" | "pasta_rice") {
                labels.push("vegan");
            }
            if group == "meat_fish" && index.is_multiple_of(3) {
                labels.push("msc");
            }
            put_attr(model, &mut values, "labels", json!(labels))?;
            let scores = ["a", "b", "c", "d", "e"];
            put_attr(
                model,
                &mut values,
                "nutriscore",
                json!(scores[(index as usize) % scores.len()]),
            )?;
            put_attr(
                model,
                &mut values,
                "nova",
                json!(((index % 4) + 1).to_string()),
            )?;
            put_attr(
                model,
                &mut values,
                "country_of_origin",
                json!(country_code(index)),
            )?;
            let energy = 80 + (index % 40) * 50;
            put_attr(
                model,
                &mut values,
                "energy",
                json!({ "value": energy.to_string(), "unit": "kJ" }),
            )?;
            let fat = index % 36;
            put_attr(model, &mut values, "fat", grams(fat))?;
            put_attr(model, &mut values, "saturated_fat", grams(fat / 2))?;
            put_attr(model, &mut values, "carbohydrates", grams(4 + (index % 30)))?;
            put_attr(model, &mut values, "sugars", grams(index % 20))?;
            put_attr(model, &mut values, "fiber", grams(index % 12))?;
            put_attr(model, &mut values, "protein", grams(index % 25))?;
            put_attr(
                model,
                &mut values,
                "salt",
                json!({ "value": format!("0.{}", index % 10), "unit": "g" }),
            )?;
            fill_group_measures(model, &mut values, group, index)?;
        }
        "article" => {
            let group = slot.group_key.unwrap_or("bread");
            let name = food_name(group, index);
            put_attr(
                model,
                &mut values,
                "active",
                json!(!index.is_multiple_of(17)),
            )?;
            put_attr(model, &mut values, "name", json!({ "en": name }))?;
            let mut labels = Vec::new();
            let product_organic = slot
                .product
                .is_some_and(|product| product.is_multiple_of(4));
            if index.is_multiple_of(4) || product_organic {
                labels.push("organic_eu");
            }
            put_attr(model, &mut values, "labels", json!(labels))?;
            put_attr(
                model,
                &mut values,
                "gtin",
                json!(format!("{:014}", 7_300_000_000_000u64 + u64::from(index))),
            )?;
            put_attr(model, &mut values, "units_per_pack", whole(1 + (index % 6)))?;
            if uses_volume(group) {
                let ml = 250 + (index % 4) * 250;
                put_attr(
                    model,
                    &mut values,
                    "net_volume",
                    json!({ "value": ml.to_string(), "unit": "ml" }),
                )?;
                put_attr(model, &mut values, "packaging", json!("bottle"))?;
                put_attr(
                    model,
                    &mut values,
                    "packaging_material",
                    json!(["glass", "plastic"]),
                )?;
                put_attr(
                    model,
                    &mut values,
                    "deposit",
                    json!(index.is_multiple_of(2)),
                )?;
                put_attr(model, &mut values, "gross_weight", grams(ml + 40))?;
            } else {
                let grams_net = 100 + (index % 8) * 50;
                put_attr(model, &mut values, "net_weight", grams(grams_net))?;
                let pack = if group == "canned" { "can" } else { "bag" };
                put_attr(model, &mut values, "packaging", json!(pack))?;
                let material = if group == "canned" { "steel" } else { "paper" };
                put_attr(model, &mut values, "packaging_material", json!([material]))?;
                put_attr(model, &mut values, "deposit", json!(false))?;
                put_attr(model, &mut values, "gross_weight", grams(grams_net + 15))?;
            }
            put_attr(model, &mut values, "storage", json!(storage_for(group)))?;
            put_attr(
                model,
                &mut values,
                "shelf_life_days",
                whole(14 + (index % 50) * 7),
            )?;
            put_attr(
                model,
                &mut values,
                "height",
                json!({ "value": (40 + index % 180).to_string(), "unit": "mm" }),
            )?;
            put_attr(
                model,
                &mut values,
                "width",
                json!({ "value": (40 + index % 120).to_string(), "unit": "mm" }),
            )?;
            put_attr(
                model,
                &mut values,
                "depth",
                json!({ "value": (20 + index % 90).to_string(), "unit": "mm" }),
            )?;
            let dates = [
                "2023-03-01",
                "2023-09-15",
                "2024-04-01",
                "2024-11-20",
                "2025-05-12",
                "2026-01-08",
            ];
            put_attr(
                model,
                &mut values,
                "launched",
                json!(dates[(index as usize) % dates.len()]),
            )?;
            put_attr(
                model,
                &mut values,
                "co2_footprint",
                json!({ "value": format!("{}.{}", 1 + index % 5, index % 10), "unit": "kgCO2e" }),
            )?;
            fill_group_measures(model, &mut values, group, index)?;
        }
        _ => {}
    }
    Ok(values)
}

fn fill_group_measures(
    model: &CompiledModel,
    values: &mut BTreeMap<AttributeId, Value>,
    group: &str,
    index: u32,
) -> anyhow::Result<()> {
    if matches!(group, "milk" | "cheese" | "yoghurt") {
        put_attr(
            model,
            values,
            "fat_content",
            json!({ "value": format!("{}", 1 + (index % 40)) }),
        )?;
    }
    if group == "chocolate" {
        put_attr(
            model,
            values,
            "cocoa_content",
            json!({ "value": format!("{}", 40 + (index % 50)) }),
        )?;
    }
    if group == "coffee_tea" {
        put_attr(
            model,
            values,
            "caffeine",
            json!({ "value": format!("{}", 5 + (index % 80)) }),
        )?;
    }
    Ok(())
}

fn synthetic_object(
    model: &CompiledModel,
    revision: u64,
    index: u32,
    suppliers: u32,
    products: u32,
) -> anyhow::Result<Object> {
    let slot = slot_for(index, suppliers, products);
    let type_id = model
        .type_id(slot.type_key)
        .ok_or_else(|| anyhow::anyhow!("unknown type {}", slot.type_key))?;
    let groups = if let Some(group_key) = slot.group_key {
        let structure = model
            .structure_id("food")
            .ok_or_else(|| anyhow::anyhow!("food is missing"))?;
        let group = model
            .group_id("food", group_key)
            .ok_or_else(|| anyhow::anyhow!("unknown group {group_key}"))?;
        BTreeMap::from([(structure, group)])
    } else {
        BTreeMap::new()
    };
    let values = fill_values(model, &slot)?;
    Ok(Object {
        id: synthetic_id(index),
        type_id,
        model_revision: revision,
        groups,
        values,
    })
}

async fn seed_catalog(
    store: &Store,
    actor: &Actor,
    links: &LinkSeed<'_>,
    target: u32,
) -> anyhow::Result<()> {
    let model = links.model;
    let revision = links.revision;
    let existing: HashSet<String> = store.keys_with_prefix("SYN-").await?.into_iter().collect();
    let (suppliers, products, articles) = catalog_plan(target);
    if suppliers + products + articles != target {
        return Err(anyhow::anyhow!(
            "seed plan {suppliers}+{products}+{articles} does not add up to {target}"
        ));
    }
    let bulk = Actor {
        kind: BatchKind::Bulk,
        source: "seed-catalog".into(),
        ..actor.clone()
    };
    let store = store.on_bulk();
    let batch = store.open_batch(&bulk).await?;
    let mut pending = Vec::new();
    let mut inserted = 0u32;
    if existing.len() < target as usize {
        for index in 0..target {
            let key = format!("SYN-{index:06}");
            if existing.contains(&key) {
                continue;
            }
            let object = synthetic_object(model, revision, index, suppliers, products)?;
            let errors = model.validate(&object);
            if !errors.is_empty() {
                return Err(anyhow::anyhow!(
                    "seed {key} failed validation: {}",
                    errors[0].message
                ));
            }
            pending.push(ObjectOp::Create(object));
            if pending.len() == 400 {
                let chunk = std::mem::take(&mut pending);
                inserted += u32::try_from(chunk.len()).unwrap_or(0);
                store.write_ops(batch, &bulk, chunk).await?;
            }
        }
        if !pending.is_empty() {
            inserted += u32::try_from(pending.len()).unwrap_or(0);
            store.write_ops(batch, &bulk, pending).await?;
        }
    }
    println!(
        "seed catalog inserted {inserted} synthetic objects (target {target}, {suppliers} suppliers, {products} products, {articles} articles)"
    );
    seed_catalog_links(&store, &bulk, batch, links, (suppliers, products, articles)).await?;
    Ok(())
}

/// The live model the seed writes links against.
struct LinkSeed<'a> {
    model: &'a CompiledModel,
    defs: &'a DefinitionSet,
    revision: u64,
}

struct PlannedLink {
    op: ObjectOp,
    type_id: TypeId,
    from_id: ObjectId,
    to_id: ObjectId,
    from_one: bool,
    to_one: bool,
}

/// Pairs and cardinality-one ends already chosen in this batch.
struct LinkSeen {
    pairs: HashSet<(TypeId, ObjectId, ObjectId)>,
    froms: HashSet<(TypeId, ObjectId)>,
    tos: HashSet<(TypeId, ObjectId)>,
}

impl LinkSeen {
    fn blocks(&self, plan: &PlannedLink) -> bool {
        self.pairs
            .contains(&(plan.type_id, plan.from_id, plan.to_id))
            || (plan.from_one && self.froms.contains(&(plan.type_id, plan.from_id)))
            || (plan.to_one && self.tos.contains(&(plan.type_id, plan.to_id)))
    }

    fn remember(&mut self, plan: &PlannedLink) {
        self.pairs.insert((plan.type_id, plan.from_id, plan.to_id));
        if plan.from_one {
            self.froms.insert((plan.type_id, plan.from_id));
        }
        if plan.to_one {
            self.tos.insert((plan.type_id, plan.to_id));
        }
    }
}

async fn seed_row_links(
    store: &Store,
    actor: &Actor,
    links: &LinkSeed<'_>,
    row: &Value,
) -> anyhow::Result<()> {
    let Some(rows) = row.get("links").and_then(Value::as_array) else {
        return Ok(());
    };
    let to_id = ObjectId::from_uuid(Uuid::parse_str(row["id"].as_str().unwrap_or_default())?);
    let mut seen = LinkSeen {
        pairs: HashSet::new(),
        froms: HashSet::new(),
        tos: HashSet::new(),
    };
    for link in rows {
        let id = ObjectId::from_uuid(Uuid::parse_str(link["id"].as_str().unwrap_or_default())?);
        if store.get_object(id).await?.is_some() {
            continue;
        }
        let Some(plan) = plan_link(
            links,
            id,
            link["type"].as_str().unwrap_or(""),
            ObjectId::from_uuid(Uuid::parse_str(link["from"].as_str().unwrap_or_default())?),
            to_id,
            link.get("sort_order"),
        ) else {
            continue;
        };
        if link_already_held(store, &plan, &seen).await? {
            seen.remember(&plan);
            continue;
        }
        seen.remember(&plan);
        store.write_batch(actor, vec![plan.op]).await?;
    }
    Ok(())
}

/// A link the current model can store. An upgraded demo has the link types and
/// not `sort_order`; a missing type or attribute is skipped instead of failing boot.
fn plan_link(
    links: &LinkSeed<'_>,
    id: ObjectId,
    type_key: &str,
    from_id: ObjectId,
    to_id: ObjectId,
    sort_order: Option<&Value>,
) -> Option<PlannedLink> {
    let object_type = links
        .defs
        .types
        .iter()
        .find(|item| item.key == type_key && item.is_link())?;
    let spec = object_type.link.as_ref()?;
    let mut values = BTreeMap::new();
    if let Some(order) = sort_order.and_then(Value::as_str)
        && let Some(attribute) = links.model.attribute_by_key("sort_order")
        && object_type.attributes.contains(&attribute.id)
    {
        values.insert(attribute.id, json!({ "value": order }));
    }
    Some(PlannedLink {
        op: ObjectOp::CreateLink {
            object: Object {
                id,
                type_id: object_type.id,
                model_revision: links.revision,
                groups: BTreeMap::new(),
                values,
            },
            from_id,
            to_id,
        },
        type_id: object_type.id,
        from_id,
        to_id,
        from_one: spec.from_cardinality == Cardinality::One,
        to_one: spec.to_cardinality == Cardinality::One,
    })
}

async fn link_already_held(
    store: &Store,
    plan: &PlannedLink,
    seen: &LinkSeen,
) -> anyhow::Result<bool> {
    if seen.blocks(plan) {
        return Ok(true);
    }
    Ok(store
        .link_blocks(
            plan.type_id,
            plan.from_id,
            plan.to_id,
            plan.from_one,
            plan.to_one,
        )
        .await?)
}

fn synthetic_link_id(kind: u8, index: u32) -> ObjectId {
    let mut bytes = [
        0x01, 0x8f, 0x31, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    bytes[11] = kind;
    let be = index.to_be_bytes();
    bytes[12..16].copy_from_slice(&be);
    ObjectId::from_uuid(Uuid::from_bytes(bytes))
}

async fn seed_catalog_links(
    store: &Store,
    actor: &Actor,
    batch: braian_store::BatchId,
    links: &LinkSeed<'_>,
    counts: (u32, u32, u32),
) -> anyhow::Result<()> {
    let (suppliers, products, articles) = counts;
    let mut pending = Vec::new();
    let mut seen = LinkSeen {
        pairs: HashSet::new(),
        froms: HashSet::new(),
        tos: HashSet::new(),
    };
    for article_n in 0..articles {
        let index = suppliers + products + article_n;
        let slot = slot_for(index, suppliers, products);
        let to_id = synthetic_id(index);
        if let Some(product) = slot.product {
            let order = json!(((article_n / products.max(1)) + 1).to_string());
            queue_link(
                store,
                links,
                &mut seen,
                &mut pending,
                LinkRequest {
                    id: synthetic_link_id(1, index),
                    type_key: "product_articles",
                    from_id: synthetic_id(product),
                    to_id,
                    sort_order: Some(&order),
                },
            )
            .await?;
        }
        if let Some(supplier) = slot.supplier {
            queue_link(
                store,
                links,
                &mut seen,
                &mut pending,
                LinkRequest {
                    id: synthetic_link_id(2, index),
                    type_key: "supplier_articles",
                    from_id: synthetic_id(supplier),
                    to_id,
                    sort_order: None,
                },
            )
            .await?;
        }
        if index.is_multiple_of(11) && article_n > 0 {
            queue_link(
                store,
                links,
                &mut seen,
                &mut pending,
                LinkRequest {
                    id: synthetic_link_id(3, index),
                    type_key: "replaces",
                    from_id: synthetic_id(index - 1),
                    to_id,
                    sort_order: None,
                },
            )
            .await?;
        }
        if pending.len() >= 400 {
            let chunk = std::mem::take(&mut pending);
            store.write_ops(batch, actor, chunk).await?;
        }
    }
    if !pending.is_empty() {
        store.write_ops(batch, actor, pending).await?;
    }
    Ok(())
}

struct LinkRequest<'a> {
    id: ObjectId,
    type_key: &'a str,
    from_id: ObjectId,
    to_id: ObjectId,
    sort_order: Option<&'a Value>,
}

async fn queue_link(
    store: &Store,
    links: &LinkSeed<'_>,
    seen: &mut LinkSeen,
    pending: &mut Vec<ObjectOp>,
    request: LinkRequest<'_>,
) -> anyhow::Result<()> {
    if store.get_object(request.id).await?.is_some() {
        return Ok(());
    }
    let Some(plan) = plan_link(
        links,
        request.id,
        request.type_key,
        request.from_id,
        request.to_id,
        request.sort_order,
    ) else {
        return Ok(());
    };
    if link_already_held(store, &plan, seen).await? {
        seen.remember(&plan);
        return Ok(());
    }
    seen.remember(&plan);
    pending.push(plan.op);
    Ok(())
}

async fn seed_job_run(
    store: &Store,
    id: &str,
    type_key: &str,
    actor: PrincipalId,
) -> anyhow::Result<()> {
    let id = Uuid::parse_str(id)?;
    if store.job_run(id).await?.is_some() {
        store
            .finish_job_run(id, "succeeded", 48, "synthetic run", &json!([]))
            .await?;
        return Ok(());
    }
    store
        .create_job_run(id, type_key, None, actor, None, 48)
        .await?;
    store
        .finish_job_run(id, "succeeded", 48, "synthetic run", &json!([]))
        .await?;
    Ok(())
}

async fn seed_proposal(store: &Store, actor: PrincipalId) -> anyhow::Result<()> {
    let id = Uuid::parse_str("018f2000-0000-7000-8000-0000000000c1")?;
    if store.proposal(id).await?.is_some() {
        store.set_proposal_title(id, "Accept ART-CHED-400").await?;
        store.set_proposal_source(id, "agent").await?;
        return Ok(());
    }
    let object_id = ObjectId::from_uuid(Uuid::parse_str("018f1000-0000-7000-8000-000000000023")?);
    let Some(stored) = store.get_object(object_id).await? else {
        return Ok(());
    };
    let version = i64::try_from(stored.version).unwrap_or(1);
    store
        .create_proposal(
            id,
            "Accept ART-CHED-400",
            "agent",
            actor,
            None,
            false,
            &[ProposalRow {
                id: Uuid::parse_str("018f2000-0000-7000-8000-0000000000c2")?,
                object_id: Some(object_id.as_uuid()),
                object_key: Some(stored.key),
                type_key: Some("article".into()),
                base_version: Some(version),
                patch: json!({ "values": { "status": "ok" } }),
                status: "pending".into(),
                conflict: false,
                position: 0,
            }],
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn model() -> (DefinitionSet, CompiledModel) {
        let defs: DefinitionSet = serde_json::from_str(DEFINITIONS).expect("definitions");
        let compiled = CompiledModel::compile(&defs).expect("demo definitions compile");
        (defs, compiled)
    }

    #[test]
    fn demo_model_and_labels_are_english_only() {
        let raw: Value = serde_json::from_str(DEFINITIONS).expect("definitions");
        assert_eq!(raw["languages"], json!(["en"]));
        let mut foreign = Vec::new();
        collect_foreign_languages(&raw, "definitions", &mut foreign);
        assert!(
            foreign.is_empty(),
            "demo labels must use en only: {}",
            foreign.join(", ")
        );
    }

    fn collect_foreign_languages(value: &Value, path: &str, found: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                let name = path.rsplit(['.', '[']).next().unwrap_or("");
                if matches!(name, "labels" | "singular" | "plural") {
                    for key in map.keys() {
                        if key != "en" && !key.starts_with("en-") {
                            found.push(format!("{path}.{key}"));
                        }
                    }
                }
                for (key, child) in map {
                    collect_foreign_languages(child, &format!("{path}.{key}"), found);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    collect_foreign_languages(child, &format!("{path}[{index}]"), found);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn food_catalog_compiles_without_the_outdoor_model() {
        let (defs, _) = model();
        assert!(defs.types.iter().any(|item| item.key == "product"));
        assert!(defs.types.iter().any(|item| item.key == "article"));
        assert!(defs.types.iter().any(|item| item.key == "supplier"));
        assert!(defs.types.iter().all(|item| item.key != "variant"));
        assert!(defs.structures.iter().all(|item| item.key != "environment"));
        let food = defs
            .structures
            .iter()
            .find(|item| item.key == "food")
            .expect("food");
        assert!(food.labels.values().any(|label| label == "Food"));
        assert!(food.groups.iter().any(|group| group.key == "chocolate"));
        assert!(
            defs.unit_families
                .iter()
                .any(|family| family.key == "volume")
        );
        assert!(
            defs.unit_families
                .iter()
                .any(|family| family.key == "energy")
        );
        assert!(defs.value_lists.iter().any(|list| list.key == "allergens"));
        assert!(defs.value_lists.iter().all(|list| {
            list.key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        }));
        assert!(defs.value_lists.iter().all(|list| list.key != "eco_label"));
        let product_articles = defs
            .types
            .iter()
            .find(|item| item.key == "product_articles")
            .expect("product articles");
        assert!(product_articles.is_link());
        let spec = product_articles.link.as_ref().expect("ends");
        assert_eq!(spec.from_label.key, "product");
        assert_eq!(spec.to_label.key, "articles");
        assert!(spec.to_cardinality == braian_model::Cardinality::One);
        assert!(
            defs.types
                .iter()
                .any(|item| item.key == "supplier_articles" && item.is_link())
        );
        assert!(defs.attributes.iter().any(|item| item.key == "sort_order"));
        assert!(defs.attributes.iter().all(|item| item.key != "product"));
    }

    #[test]
    fn generator_is_deterministic_and_keeps_one_to_five_articles() {
        let (defs, compiled) = model();
        let (suppliers, products, articles) = catalog_plan(DEFAULT_SEED_COUNT);
        assert_eq!((suppliers, products, articles), (13, 57, 170));
        let mut per_product: HashMap<u32, u32> = HashMap::new();
        let mut first = Vec::new();
        for index in 0..DEFAULT_SEED_COUNT {
            let object =
                synthetic_object(&compiled, defs.model_revision, index, suppliers, products)
                    .expect("object");
            let again =
                synthetic_object(&compiled, defs.model_revision, index, suppliers, products)
                    .expect("again");
            assert_eq!(object, again);
            let errors = compiled.validate(&object);
            assert!(
                errors.is_empty(),
                "SYN-{index:06}: {}",
                errors
                    .first()
                    .map(|error| error.message.as_str())
                    .unwrap_or("")
            );
            let slot = slot_for(index, suppliers, products);
            if slot.type_key == "supplier" {
                assert!(object.groups.is_empty());
            }
            if slot.type_key == "article" {
                assert!(object.values.values().any(|value| value.as_str().is_some()));
                let product = slot.product.expect("product");
                *per_product.entry(product).or_default() += 1;
            }
            if index < 3 {
                first.push(object.id);
            }
        }
        assert!(per_product.values().all(|count| (1..=5).contains(count)));
        assert_eq!(per_product.len(), products as usize);
        let rows: Vec<Value> = serde_json::from_str(OBJECTS).expect("heroes");
        assert!(rows.len() >= 10 && rows.len() <= 12);
        for row in rows {
            let type_key = row["type"].as_str().unwrap();
            let type_id = compiled.type_id(type_key).expect(type_key);
            let id = ObjectId::from_uuid(Uuid::parse_str(row["id"].as_str().unwrap()).unwrap());
            let groups = match row["group"].as_str() {
                Some(group_key) => {
                    let structure = compiled.structure_id("food").unwrap();
                    let group = compiled.group_id("food", group_key).unwrap();
                    BTreeMap::from([(structure, group)])
                }
                None => BTreeMap::new(),
            };
            let mut values = BTreeMap::new();
            for (name, value) in row["values"].as_object().unwrap() {
                let attribute = compiled.attribute_by_key(name).expect(name);
                values.insert(attribute.id, value.clone());
            }
            let object = Object {
                id,
                type_id,
                model_revision: defs.model_revision,
                groups,
                values,
            };
            let errors = compiled.validate(&object);
            assert!(
                errors.is_empty(),
                "{}: {}",
                row["values"]["number"],
                errors
                    .first()
                    .map(|error| error.message.as_str())
                    .unwrap_or("")
            );
        }
        assert_eq!(first.len(), 3);
    }

    #[test]
    fn organic_assortment_contains_only_objects_with_the_label() {
        let (defs, compiled) = model();
        let query = organic_assortment_query();
        assert_eq!(query["labels"], "organic_eu");
        assert!(query.get("text").is_none());
        let (suppliers, products, _) = catalog_plan(DEFAULT_SEED_COUNT);
        let mut objects = Vec::new();
        for index in 0..DEFAULT_SEED_COUNT {
            objects.push(
                synthetic_object(&compiled, defs.model_revision, index, suppliers, products)
                    .expect("object"),
            );
        }
        for row in serde_json::from_str::<Vec<Value>>(OBJECTS).expect("heroes") {
            objects.push(hero_object(&compiled, &defs, &row));
            if row["values"]["number"] == "ART-OAT-1L" {
                assert!(row["values"].get("image").is_none());
            }
        }
        let mut products_in = 0;
        let mut articles_in = 0;
        let mut products_out = 0;
        let mut articles_out = 0;
        for object in &objects {
            let labels = label_codes(&compiled, object);
            let included = assortment_includes(&query, &labels);
            assert_eq!(
                included,
                labels.iter().any(|label| label == "organic_eu"),
                "assortment membership must be the organic_eu label"
            );
            let type_key = defs
                .types
                .iter()
                .find(|item| item.id == object.type_id)
                .map(|item| item.key.as_str())
                .unwrap_or("");
            let name = english_name(&compiled, object);
            if name.starts_with("Organic ") {
                assert!(
                    included,
                    "{name} is named Organic but has no organic_eu label"
                );
            }
            match type_key {
                "product" if included => products_in += 1,
                "product" => products_out += 1,
                "article" if included => articles_in += 1,
                "article" => articles_out += 1,
                _ => assert!(!included, "suppliers are not in the organic assortment"),
            }
        }
        assert!(products_in > 0 && articles_in > 0);
        assert!(products_out > 0 && articles_out > 0);
    }

    /// First seed on an empty database creates "Accept ART-CHED-400".
    /// A second seed updates that row and does not insert another.
    #[tokio::test]
    async fn first_seed_creates_the_ched_proposal_once() {
        load_test_database_url();
        let _restore = SeedEnv::set(&[
            ("BRAIAN_DEMO_EMAIL", "seed-test@example.com"),
            ("BRAIAN_DEMO_PASSWORD", "seed-test-password"),
            // Heroes are enough for the proposal. Skip the synthetic bulk.
            ("BRAIAN_SEED_COUNT", "0"),
        ]);
        let db = braian_store::testing::TestDb::create().await;
        let proposal_id = Uuid::parse_str("018f2000-0000-7000-8000-0000000000c1").unwrap();

        run(db.store(), None).await.expect("first seed");
        let proposal = db
            .store()
            .proposal(proposal_id)
            .await
            .expect("proposal query")
            .expect("proposal after the first seed");
        assert_eq!(proposal["title"], "Accept ART-CHED-400");
        assert_eq!(proposal["source"], "agent");
        let rows = proposal["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["object_key"], "ART-CHED-400");
        assert_eq!(matching_proposals(db.store()).await, 1);

        run(db.store(), None).await.expect("second seed");
        let again = db
            .store()
            .proposal(proposal_id)
            .await
            .expect("proposal query")
            .expect("proposal after the second seed");
        assert_eq!(again["title"], "Accept ART-CHED-400");
        assert_eq!(again["source"], "agent");
        assert_eq!(matching_proposals(db.store()).await, 1);
    }

    /// The seed stores ids. A second run repairs leftover keys and does not
    /// replace a saved view or selection. Renaming the brand key leaves the
    /// stored column id in place. `number` is a locked standard attribute, so
    /// this uses brand, which the product view already stores by id.
    #[tokio::test]
    async fn seeded_views_keep_ids_across_a_second_seed_and_an_attribute_rename() {
        load_test_database_url();
        let _restore = SeedEnv::set(&[
            ("BRAIAN_DEMO_EMAIL", "seed-ids@example.com"),
            ("BRAIAN_DEMO_PASSWORD", "seed-test-password"),
            ("BRAIAN_SEED_COUNT", "0"),
        ]);
        let db = braian_store::testing::TestDb::create().await;
        let store = db.store();

        run(store, None).await.expect("first seed");
        let defs = store.definition_set().await.expect("definitions");
        let number = braian_model::resolve(&defs, braian_model::RefKind::Attribute, "number", None)
            .expect("number");
        let company = braian_model::resolve(
            &defs,
            braian_model::RefKind::Attribute,
            "company_name",
            None,
        )
        .expect("company_name");
        let supplier = braian_model::resolve(&defs, braian_model::RefKind::Type, "supplier", None)
            .expect("supplier");
        let food =
            braian_model::resolve(&defs, braian_model::RefKind::Structure, "food", None).unwrap();
        let cheese_group =
            braian_model::resolve(&defs, braian_model::RefKind::Group, "cheese", Some("food"))
                .unwrap();
        let cheese_ref = format!("{}:{}", food.id, cheese_group.id);

        let products = stored_view(store, PRODUCTS_WORKSPACE, PRODUCTS_VIEW).await;
        assert_eq!(products["state"]["columns"][0], number.id);
        assert_eq!(products["state"]["sort"], number.id);
        assert_eq!(
            products["state"]["type_key"],
            "018f0000-0000-7000-8000-000000000200"
        );
        assert_refs_are_ids(&defs, &products["state"], &json!({}));
        let cheese = store
            .assortment(id(CHEESE_ARTICLE))
            .await
            .expect("query")
            .expect("cheese");
        assert_eq!(cheese["query"]["group"], cheese_ref);
        assert!(cheese["query"].get("type").is_none());
        assert_refs_are_ids(&defs, &json!({}), &cheese["query"]);

        let user = store
            .user_by_email("seed-ids@example.com")
            .await
            .expect("user")
            .expect("demo user");
        let suppliers = stored_view(store, SUPPLIERS_WORKSPACE, SUPPLIERS_VIEW).await;
        store
            .save_view(
                id(SUPPLIERS_VIEW),
                id(SUPPLIERS_WORKSPACE),
                suppliers["type"].as_str().unwrap(),
                "Supplier grid",
                suppliers["pane"].as_str().unwrap(),
                suppliers["position"].as_i64().unwrap() as i32,
                &json!({
                    "columns": ["company_name"],
                    "sort": "company_name",
                    "object_type": "018f0000-0000-7000-8000-000000000202",
                    "type_key": "supplier"
                }),
            )
            .await
            .expect("narrow the supplier view");
        store
            .save_assortment(
                id(CHEESE_ARTICLE),
                user.id,
                id("018f0000-0000-7000-8000-000000000201"),
                "Aged cheese",
                &json!({ "group": "food:cheese" }),
            )
            .await
            .expect("put a key back into the selection");

        run(store, None).await.expect("second seed");
        let defs = store.definition_set().await.expect("definitions");
        let suppliers = stored_view(store, SUPPLIERS_WORKSPACE, SUPPLIERS_VIEW).await;
        assert_eq!(suppliers["title"], "Supplier grid");
        assert_eq!(suppliers["state"]["columns"], json!([company.id]));
        assert_eq!(suppliers["state"]["sort"], company.id);
        assert_eq!(suppliers["state"]["type_key"], supplier.id);
        assert_refs_are_ids(&defs, &suppliers["state"], &json!({}));
        let products = stored_view(store, PRODUCTS_WORKSPACE, PRODUCTS_VIEW).await;
        assert_eq!(products["state"]["columns"][0], number.id);
        assert_eq!(products["title"], "Products");
        for workspace in [PRODUCTS_WORKSPACE, ARTICLES_WORKSPACE, SUPPLIERS_WORKSPACE] {
            let body = store
                .workspace(id(workspace))
                .await
                .expect("workspace")
                .expect("workspace row");
            for view in body["views"].as_array().expect("views") {
                assert_refs_are_ids(&defs, &view["state"], &json!({}));
            }
        }
        let cheese = store
            .assortment(id(CHEESE_ARTICLE))
            .await
            .expect("query")
            .expect("cheese");
        assert_eq!(cheese["name"], "Aged cheese");
        assert_eq!(cheese["query"]["group"], cheese_ref);
        assert_refs_are_ids(&defs, &json!({}), &cheese["query"]);
        let product_cheese = store
            .assortment(id(CHEESE_PRODUCT))
            .await
            .expect("query")
            .expect("product cheese");
        assert_eq!(product_cheese["query"]["group"], cheese_ref);
        let organic = store
            .assortment(id(ORGANIC))
            .await
            .expect("query")
            .expect("organic");
        assert_eq!(organic["query"]["labels"], "organic_eu");
        assert_refs_are_ids(&defs, &json!({}), &organic["query"]);

        let brand = braian_model::resolve(&defs, braian_model::RefKind::Attribute, "brand", None)
            .expect("brand");
        let mut renamed = defs.clone();
        renamed.model_revision += 1;
        braian_model::apply_all(
            &mut renamed,
            &[braian_model::ModelChange::RenameAttributeKey {
                key: "brand".into(),
                new_key: "brand_label".into(),
            }],
        )
        .expect("rename");
        let actor = Actor {
            actor: user.principal_id,
            on_behalf_of: None,
            kind: BatchKind::Interactive,
            source: "test".into(),
        };
        store
            .put_definitions(&actor, &renamed)
            .await
            .expect("save the renamed key");

        let products = stored_view(store, PRODUCTS_WORKSPACE, PRODUCTS_VIEW).await;
        let columns = products["state"]["columns"].as_array().expect("columns");
        assert!(columns.iter().any(|column| column == &brand.id));
        assert!(columns.iter().any(|column| column == &number.id));
        assert!(
            columns
                .iter()
                .all(|column| column != "brand" && column != "brand_label" && column != "number")
        );
        assert_eq!(products["state"]["sort"], number.id);
        let articles = stored_view(store, ARTICLES_WORKSPACE, ARTICLES_VIEW).await;
        let article_columns = articles["state"]["columns"].as_array().expect("columns");
        assert!(article_columns.iter().any(|column| column == &number.id));
        assert_refs_are_ids(&renamed, &products["state"], &json!({}));
        assert_refs_are_ids(&renamed, &articles["state"], &json!({}));
    }

    const PRODUCTS_WORKSPACE: &str = "018f2000-0000-7000-8000-000000000003";
    const ARTICLES_WORKSPACE: &str = "018f2000-0000-7000-8000-000000000005";
    const SUPPLIERS_WORKSPACE: &str = "018f2000-0000-7000-8000-000000000007";
    const PRODUCTS_VIEW: &str = "018f2000-0000-7000-8000-000000000302";
    const ARTICLES_VIEW: &str = "018f2000-0000-7000-8000-000000000306";
    const SUPPLIERS_VIEW: &str = "018f2000-0000-7000-8000-000000000702";
    const CHEESE_ARTICLE: &str = "018f2000-0000-7000-8000-0000000000a1";
    const CHEESE_PRODUCT: &str = "018f2000-0000-7000-8000-0000000000a3";
    const ORGANIC: &str = "018f2000-0000-7000-8000-0000000000a2";

    fn id(text: &str) -> Uuid {
        Uuid::parse_str(text).expect("uuid")
    }

    async fn stored_view(store: &Store, workspace: &str, view: &str) -> Value {
        let body = store
            .workspace(id(workspace))
            .await
            .expect("workspace")
            .expect("workspace row");
        body["views"]
            .as_array()
            .expect("views")
            .iter()
            .find(|row| row["id"].as_str() == Some(view))
            .unwrap_or_else(|| panic!("missing view {view}"))
            .clone()
    }

    fn assert_refs_are_ids(defs: &DefinitionSet, state: &Value, query: &Value) {
        let mut state_again = state.clone();
        braian_model::translate_view_state(&mut state_again, defs);
        assert_eq!(&state_again, state, "view state still stores keys: {state}");
        let mut query_again = query.clone();
        braian_model::translate_assortment_query(&mut query_again, defs);
        assert_eq!(
            &query_again, query,
            "selection query still stores keys: {query}"
        );
    }

    async fn matching_proposals(store: &Store) -> usize {
        store
            .list_proposals()
            .await
            .expect("list proposals")
            .iter()
            .filter(|item| item["title"] == "Accept ART-CHED-400")
            .count()
    }

    /// V22 demo rows (object refs and a relation) migrate, then the seed runs twice.
    ///
    /// The converted link types have no `sort_order`, and their link ids are not
    /// the ids in `objects.json`. Both seed runs have to finish, and the second
    /// must not add a link.
    #[tokio::test]
    async fn upgraded_v22_demo_seeds_links_twice_and_system_cannot_commit() {
        load_test_database_url();
        let _restore = SeedEnv::set(&[
            ("BRAIAN_DEMO_EMAIL", "upgraded-seed@example.com"),
            ("BRAIAN_DEMO_PASSWORD", "seed-test-password"),
            ("BRAIAN_SEED_COUNT", "12"),
        ]);
        let db = UpgradeDb::create().await;
        let source =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../store/migrations");
        let dir = std::env::temp_dir().join(format!("braian-v22-seed-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let Some(version) = name
                .strip_prefix('V')
                .and_then(|rest| rest.split_once('_'))
                .and_then(|(number, _)| number.parse::<u32>().ok())
            else {
                continue;
            };
            if version <= 22 {
                std::fs::copy(entry.path(), dir.join(name)).unwrap();
            }
        }
        let mut setup = db.connect().await;
        let migrations = refinery::load_sql_migrations(&dir).unwrap();
        refinery::Runner::new(&migrations)
            .run_async(&mut setup.client)
            .await
            .unwrap();
        insert_v22_demo(&setup.client).await;
        drop(setup);

        let store = braian_store::Store::connect_config(db.config(), braian_store::STORAGE_FORM)
            .await
            .unwrap();
        assert!(store.upgrade_object_refs().await.unwrap());
        let defs = store.definition_set().await.unwrap();
        assert!(defs.types.iter().any(|item| item.key == "product_articles"));
        assert!(defs.attributes.iter().all(|item| item.key != "sort_order"));
        braian_api::stored_principal_cannot_approve_discard_or_delete(&store)
            .await
            .unwrap();
        let before = link_census(&db).await;
        assert!(before.0 > 0, "upgrade should have written links");
        assert_eq!(before.1, 0, "upgrade wrote duplicate pairs");

        run(&store, None).await.expect("first seed");
        let after_first = link_census(&db).await;
        assert_eq!(after_first.0, before.0, "the first seed added links");
        assert_eq!(after_first.1, 0, "the first seed duplicated a pair");

        run(&store, None).await.expect("second seed");
        let after_second = link_census(&db).await;
        assert_eq!(after_second.0, after_first.0);
        assert_eq!(after_second.1, 0);
        braian_api::stored_principal_cannot_approve_discard_or_delete(&store)
            .await
            .unwrap();
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn link_census(db: &UpgradeDb) -> (i64, i64) {
        let conn = db.connect().await;
        let links: i64 = conn
            .client
            .query_one(
                "SELECT count(*) FROM links l
                 JOIN objects object ON object.id = l.object_id AND object.deleted_at IS NULL",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        let duplicates: i64 = conn
            .client
            .query_one(
                "SELECT count(*) FROM (
                    SELECT 1
                    FROM links l
                    JOIN objects object ON object.id = l.object_id AND object.deleted_at IS NULL
                    GROUP BY l.link_type_id, l.from_id, l.to_id
                    HAVING count(*) > 1
                 ) pairs",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        (links, duplicates)
    }

    async fn insert_v22_demo(client: &tokio_postgres::Client) {
        let product = Uuid::parse_str("018f0000-0000-7000-8000-000000000200").unwrap();
        let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap();
        let supplier = Uuid::parse_str("018f0000-0000-7000-8000-000000000202").unwrap();
        let number = "018f0000-0000-7000-8000-000000000100";
        let product_attr = "018f0000-0000-7000-8000-00000000011a";
        let supplier_attr = "018f0000-0000-7000-8000-00000000011b";
        let replaces_attr = "018f0000-0000-7000-8000-000000000107";
        let principal = Uuid::now_v7();
        let batch = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
                &[&principal as &(dyn tokio_postgres::types::ToSql + Sync)],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO batches (id, actor, kind, source) VALUES ($1, $2, 'interactive', 'test')",
                &[
                    &batch as &(dyn tokio_postgres::types::ToSql + Sync),
                    &principal,
                ],
            )
            .await
            .unwrap();
        let text = json!({
            "type": "text",
            "max_length": 64
        });
        let index = json!({"searchable": true, "filterable": true, "sortable": true});
        let definitions = json!({
            "model_revision": 1,
            "languages": ["en"],
            "unit_families": [],
            "attributes": [
                attribute_json(number, "number", text.clone(), false, index.clone()),
                attribute_json(product_attr, "product", json!({"type": "object_ref", "type_key": "product"}), true, index.clone()),
                attribute_json(supplier_attr, "supplier", json!({"type": "object_ref", "type_key": "supplier"}), false, index.clone()),
                attribute_json(replaces_attr, "replaces", json!({"type": "object_ref", "type_key": "article"}), false, index)
            ],
            "types": [
                type_json(product, "product", "Product", number, json!([number])),
                type_json(article, "article", "Article", number, json!([number, product_attr, supplier_attr, replaces_attr])),
                type_json(supplier, "supplier", "Supplier", number, json!([number]))
            ],
            "structures": []
        });
        client
            .execute(
                "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES (1, $1, $2)",
                &[
                    &definitions as &(dyn tokio_postgres::types::ToSql + Sync),
                    &batch,
                ],
            )
            .await
            .unwrap();

        let heroes = [
            (
                "018f1000-0000-7000-8000-000000000001",
                supplier,
                "SUP-NORTH",
            ),
            (
                "018f1000-0000-7000-8000-000000000002",
                supplier,
                "SUP-COAST",
            ),
            ("018f1000-0000-7000-8000-000000000011", product, "PROD-OAT"),
            (
                "018f1000-0000-7000-8000-000000000012",
                product,
                "PROD-CHEDDAR",
            ),
            (
                "018f1000-0000-7000-8000-000000000013",
                product,
                "PROD-COFFEE",
            ),
            ("018f1000-0000-7000-8000-000000000014", product, "PROD-CHOC"),
            (
                "018f1000-0000-7000-8000-000000000021",
                article,
                "ART-OAT-1L",
            ),
            (
                "018f1000-0000-7000-8000-000000000022",
                article,
                "ART-CHED-200",
            ),
            (
                "018f1000-0000-7000-8000-000000000023",
                article,
                "ART-CHED-400",
            ),
            (
                "018f1000-0000-7000-8000-000000000024",
                article,
                "ART-COFFEE-250",
            ),
            (
                "018f1000-0000-7000-8000-000000000025",
                article,
                "ART-CHOC-100",
            ),
        ];
        for (id, type_id, key) in heroes {
            let id = Uuid::parse_str(id).unwrap();
            client
                .execute(
                    "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, 1)",
                    &[
                        &id as &(dyn tokio_postgres::types::ToSql + Sync),
                        &type_id,
                        &key,
                    ],
                )
                .await
                .unwrap();
        }
        let refs = [
            (
                "018f1000-0000-7000-8000-000000000021",
                "018f1000-0000-7000-8000-000000000011",
                "018f1000-0000-7000-8000-000000000001",
                None,
            ),
            (
                "018f1000-0000-7000-8000-000000000022",
                "018f1000-0000-7000-8000-000000000012",
                "018f1000-0000-7000-8000-000000000002",
                None,
            ),
            (
                "018f1000-0000-7000-8000-000000000023",
                "018f1000-0000-7000-8000-000000000012",
                "018f1000-0000-7000-8000-000000000002",
                Some("018f1000-0000-7000-8000-000000000022"),
            ),
            (
                "018f1000-0000-7000-8000-000000000024",
                "018f1000-0000-7000-8000-000000000013",
                "018f1000-0000-7000-8000-000000000001",
                None,
            ),
            (
                "018f1000-0000-7000-8000-000000000025",
                "018f1000-0000-7000-8000-000000000014",
                "018f1000-0000-7000-8000-000000000002",
                None,
            ),
        ];
        for (holder, product_id, supplier_id, replaces) in refs {
            let mut values = serde_json::Map::new();
            values.insert(product_attr.into(), json!(product_id));
            values.insert(supplier_attr.into(), json!(supplier_id));
            if let Some(replaces) = replaces {
                values.insert(replaces_attr.into(), json!(replaces));
            }
            client
                .execute(
                    "INSERT INTO object_values (object_id, part, \"values\", version, is_latest) VALUES ($1, '', $2, 1, true)",
                    &[
                        &Uuid::parse_str(holder).unwrap() as &(dyn tokio_postgres::types::ToSql + Sync),
                        &Value::Object(values),
                    ],
                )
                .await
                .unwrap();
        }
        client
            .execute(
                "INSERT INTO relations (from_id, to_id, kind) VALUES ($1, $2, 'article_of')",
                &[
                    &Uuid::parse_str("018f1000-0000-7000-8000-000000000021").unwrap()
                        as &(dyn tokio_postgres::types::ToSql + Sync),
                    &Uuid::parse_str("018f1000-0000-7000-8000-000000000011").unwrap(),
                ],
            )
            .await
            .unwrap();

        let (suppliers, products, _) = catalog_plan(12);
        for index in 0..12 {
            let slot = slot_for(index, suppliers, products);
            let type_id = match slot.type_key {
                "supplier" => supplier,
                "product" => product,
                _ => article,
            };
            let id = synthetic_id(index).as_uuid();
            let key = format!("SYN-{index:06}");
            client
                .execute(
                    "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, 1)",
                    &[
                        &id as &(dyn tokio_postgres::types::ToSql + Sync),
                        &type_id,
                        &key,
                    ],
                )
                .await
                .unwrap();
            if slot.type_key != "article" {
                continue;
            }
            let mut values = serde_json::Map::new();
            if let Some(product_index) = slot.product {
                values.insert(
                    product_attr.into(),
                    json!(synthetic_id(product_index).as_uuid().to_string()),
                );
            }
            if let Some(supplier_index) = slot.supplier {
                values.insert(
                    supplier_attr.into(),
                    json!(synthetic_id(supplier_index).as_uuid().to_string()),
                );
            }
            let article_n = index - suppliers - products;
            if index.is_multiple_of(11) && article_n > 0 {
                values.insert(
                    replaces_attr.into(),
                    json!(synthetic_id(index - 1).as_uuid().to_string()),
                );
            }
            client
                .execute(
                    "INSERT INTO object_values (object_id, part, \"values\", version, is_latest) VALUES ($1, '', $2, 1, true)",
                    &[
                        &id as &(dyn tokio_postgres::types::ToSql + Sync),
                        &Value::Object(values),
                    ],
                )
                .await
                .unwrap();
        }
    }

    fn attribute_json(id: &str, key: &str, kind: Value, required: bool, index: Value) -> Value {
        json!({
            "id": id,
            "key": key,
            "labels": {"en": key},
            "language_dependent": false,
            "multiple": false,
            "required": required,
            "kind": kind,
            "index": index
        })
    }

    fn type_json(id: Uuid, key: &str, label: &str, number: &str, attributes: Value) -> Value {
        json!({
            "id": id,
            "key": key,
            "labels": {"en": label},
            "key_attribute": number,
            "attributes": attributes,
            "structures": []
        })
    }

    struct UpgradeDb {
        name: String,
        admin_url: String,
    }

    impl UpgradeDb {
        async fn create() -> Self {
            let admin_url = std::env::var("BRAIAN_TEST_DATABASE_URL").expect("database url");
            let name = format!("u{:032x}", Uuid::now_v7().as_u128());
            let mut config = braian_store::admin::parse_config(&admin_url).unwrap();
            config.dbname("postgres");
            let admin = braian_store::admin::PgConn::connect(config).await.unwrap();
            braian_store::admin::create_database(&admin.client, &name)
                .await
                .unwrap();
            Self { name, admin_url }
        }

        fn config(&self) -> tokio_postgres::Config {
            let mut config = braian_store::admin::parse_config(&self.admin_url).unwrap();
            config.dbname(&self.name);
            config
        }

        async fn connect(&self) -> braian_store::admin::PgConn {
            braian_store::admin::PgConn::connect(self.config())
                .await
                .unwrap()
        }
    }

    impl Drop for UpgradeDb {
        fn drop(&mut self) {
            let url = self.admin_url.clone();
            let name = self.name.clone();
            let _ = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("upgrade test cleanup");
                runtime.block_on(async {
                    for _ in 0..20 {
                        let Ok(mut config) = braian_store::admin::parse_config(&url) else {
                            return;
                        };
                        config.dbname("postgres");
                        let Ok(admin) = braian_store::admin::PgConn::connect(config).await else {
                            return;
                        };
                        if braian_store::admin::drop_database(&admin.client, &name)
                            .await
                            .is_ok()
                        {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                });
            })
            .join();
        }
    }

    fn load_test_database_url() {
        if std::env::var_os("BRAIAN_TEST_DATABASE_URL").is_some() {
            return;
        }
        let mut candidates = vec![std::path::PathBuf::from(".env")];
        candidates.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.env"));
        for path in candidates {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                if key == "BRAIAN_TEST_DATABASE_URL" && std::env::var_os(key).is_none() {
                    unsafe { std::env::set_var(key, value.trim_matches('"')) };
                }
            }
        }
    }

    struct SeedEnv {
        previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn seed_env_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        &LOCK
    }

    impl SeedEnv {
        fn set(pairs: &[(&'static str, &str)]) -> Self {
            let _lock = seed_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = pairs
                .iter()
                .map(|(key, value)| {
                    let old = std::env::var_os(key);
                    unsafe { std::env::set_var(key, value) };
                    (*key, old)
                })
                .collect();
            Self { previous, _lock }
        }
    }

    impl Drop for SeedEnv {
        fn drop(&mut self) {
            for (key, value) in self.previous.drain(..) {
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    fn hero_object(compiled: &CompiledModel, defs: &DefinitionSet, row: &Value) -> Object {
        let type_key = row["type"].as_str().unwrap();
        let type_id = compiled.type_id(type_key).expect(type_key);
        let id = ObjectId::from_uuid(Uuid::parse_str(row["id"].as_str().unwrap()).unwrap());
        let groups = match row["group"].as_str() {
            Some(group_key) => {
                let structure = compiled.structure_id("food").unwrap();
                let group = compiled.group_id("food", group_key).unwrap();
                BTreeMap::from([(structure, group)])
            }
            None => BTreeMap::new(),
        };
        let mut values = BTreeMap::new();
        for (name, value) in row["values"].as_object().unwrap() {
            let attribute = compiled.attribute_by_key(name).expect(name);
            values.insert(attribute.id, value.clone());
        }
        let object = Object {
            id,
            type_id,
            model_revision: defs.model_revision,
            groups,
            values,
        };
        let errors = compiled.validate(&object);
        assert!(
            errors.is_empty(),
            "{}: {}",
            row["values"]["number"],
            errors
                .first()
                .map(|error| error.message.as_str())
                .unwrap_or("")
        );
        object
    }

    fn label_codes(model: &CompiledModel, object: &Object) -> Vec<String> {
        let Some(attribute) = model.attribute_by_key("labels") else {
            return Vec::new();
        };
        let Some(value) = object.values.get(&attribute.id) else {
            return Vec::new();
        };
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// V23-shaped filters and import targets are rewritten when the database
    /// opens, then startup and seed both run twice without adding rows.
    #[tokio::test]
    async fn old_shape_data_survives_startup_and_a_second_seed() {
        load_test_database_url();
        let _restore = SeedEnv::set(&[
            ("BRAIAN_DEMO_EMAIL", "path-upgrade@example.com"),
            ("BRAIAN_DEMO_PASSWORD", "seed-test-password"),
            ("BRAIAN_SEED_COUNT", "0"),
        ]);
        let admin_url = std::env::var("BRAIAN_TEST_DATABASE_URL").expect("database url");
        let name = format!("p{:032x}", Uuid::now_v7().as_u128());
        let _cleanup = DbGuard {
            admin_url: admin_url.clone(),
            name: name.clone(),
        };
        let mut admin_config = braian_store::admin::parse_config(&admin_url).unwrap();
        admin_config.dbname("postgres");
        let admin = braian_store::admin::PgConn::connect(admin_config)
            .await
            .unwrap();
        braian_store::admin::create_database(&admin.client, &name)
            .await
            .unwrap();
        drop(admin);

        let mut config = braian_store::admin::parse_config(&admin_url).unwrap();
        config.dbname(&name);
        let mut conn = braian_store::admin::PgConn::connect(config).await.unwrap();
        let source =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../store/migrations");
        let dir = std::env::temp_dir().join(format!("braian-v23-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let file = entry.file_name().into_string().unwrap();
            // V23 is the last shape this fixture inserts. Later files, including
            // the claim lease, have to stay out or they apply ahead of V24.
            let Some(version) = file
                .strip_prefix('V')
                .and_then(|rest| rest.split_once('_'))
                .and_then(|(number, _)| number.parse::<u32>().ok())
            else {
                continue;
            };
            if version <= 23 {
                std::fs::copy(entry.path(), dir.join(file)).unwrap();
            }
        }
        let migrations = refinery::load_sql_migrations(&dir).unwrap();
        refinery::Runner::new(&migrations)
            .run_async(&mut conn.client)
            .await
            .unwrap();

        let principal = Uuid::now_v7();
        let user = Uuid::now_v7();
        let batch = Uuid::now_v7();
        let workspace = Uuid::now_v7();
        let view = Uuid::parse_str("018f2000-0000-7000-8000-00000000f010").unwrap();
        let import_id = Uuid::parse_str("018f2000-0000-7000-8000-00000000f011").unwrap();
        let assortment = Uuid::parse_str("018f2000-0000-7000-8000-00000000f012").unwrap();
        let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap();
        let weight = "018f0000-0000-7000-8000-00000000011c";
        let defs: Value = serde_json::from_str(include_str!("../demo/definitions.json")).unwrap();
        conn.client
            .execute(
                "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
                &[&principal],
            )
            .await
            .unwrap();
        conn.client
            .execute(
                "INSERT INTO users (id, principal_id, email, password_hash, role, display_name)
                 VALUES ($1, $2, 'legacy-path@example.com', 'x', 'admin', 'Ada')",
                &[&user, &principal],
            )
            .await
            .unwrap();
        conn.client
            .execute(
                "INSERT INTO batches (id, actor, kind, source) VALUES ($1, $2, 'interactive', 'test')",
                &[&batch, &principal],
            )
            .await
            .unwrap();
        conn.client
            .execute(
                "INSERT INTO definition_revisions (model_revision, definitions, batch_id)
                 VALUES (1, $1::jsonb, $2)",
                &[&defs, &batch],
            )
            .await
            .unwrap();
        conn.client
            .execute(
                "INSERT INTO workspaces (id, owner_id, name, share_token) VALUES ($1, $2, 'Legacy', $3)",
                &[&workspace, &user, &format!("legacy-{name}")],
            )
            .await
            .unwrap();
        let state: Value = serde_json::from_str(&format!(
            r#"{{"table":{{"filter":"a_{underscored}:=500","sort":"a_{underscored}:asc"}}}}"#,
            underscored = weight.replace('-', "_")
        ))
        .unwrap();
        conn.client
            .execute(
                "INSERT INTO views (id, workspace_id, type_key, title, state)
                 VALUES ($1, $2, 'object_table', 'Legacy filter', $3::jsonb)",
                &[&view, &workspace, &state],
            )
            .await
            .unwrap();
        let mapping: Value = serde_json::json!({
            "type": "article",
            "columns": [
                { "col": 0, "target": "type:net_weight" },
                { "col": 1, "target": "type:net_weight.unit" }
            ]
        });
        conn.client
            .execute(
                "INSERT INTO import_sessions (id, owner, file_name, file_kind, status, model_revision, mapping)
                 VALUES ($1, $2, 'old.csv', 'csv', 'ready', 1, $3::jsonb)",
                &[&import_id, &principal, &mapping],
            )
            .await
            .unwrap();
        let query: Value = serde_json::from_str(&format!(
            r#"{{"filter":"a_{underscored}__unit:=g"}}"#,
            underscored = weight.replace('-', "_")
        ))
        .unwrap();
        conn.client
            .execute(
                "INSERT INTO assortments (id, owner_id, name, query, type_id)
                 VALUES ($1, $2, 'Legacy selection', $3::jsonb, $4)",
                &[&assortment, &user, &query, &article],
            )
            .await
            .unwrap();
        drop(conn);

        let url = {
            let base = admin_url.trim_end_matches('/');
            match base.rsplit_once('/') {
                Some((prefix, _)) => format!("{prefix}/{name}"),
                None => format!("{base}/{name}"),
            }
        };
        let store = braian_store::Store::connect(&url).await.expect("startup");
        crate::maintain::startup(&store).await.expect("maintain");
        let mut read_config = braian_store::admin::parse_config(&url).unwrap();
        read_config.dbname(&name);
        let read = braian_store::admin::PgConn::connect(read_config)
            .await
            .unwrap();
        let view_state = read
            .client
            .query_one("SELECT state FROM views WHERE id = $1", &[&view])
            .await
            .unwrap()
            .get::<_, Value>("state");
        assert_eq!(view_state["table"]["filter"], format!("{weight}:=500"));
        assert_eq!(view_state["table"]["sort"], format!("{weight}:asc"));
        let stored_mapping = read
            .client
            .query_one(
                "SELECT mapping FROM import_sessions WHERE id = $1",
                &[&import_id],
            )
            .await
            .unwrap()
            .get::<_, Value>("mapping");
        assert_eq!(stored_mapping["columns"][0]["target"], weight);
        assert_eq!(
            stored_mapping["columns"][1]["target"],
            format!("{weight}.unit")
        );
        let stored_query = read
            .client
            .query_one(
                "SELECT query FROM assortments WHERE id = $1",
                &[&assortment],
            )
            .await
            .unwrap()
            .get::<_, Value>("query");
        assert_eq!(stored_query["filter"], format!("{weight}.unit:=g"));
        let requested: bool = read
            .client
            .query_one(
                "SELECT reindex_requested FROM indexer_progress WHERE id = 1",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert!(requested);

        run(&store, None).await.expect("first seed");
        let after_first = row_counts(&read.client).await;
        let legacy_view: i64 = read
            .client
            .query_one("SELECT count(*) FROM views WHERE id = $1", &[&view])
            .await
            .unwrap()
            .get(0);
        let legacy_state: Value = read
            .client
            .query_one("SELECT state FROM views WHERE id = $1", &[&view])
            .await
            .unwrap()
            .get("state");
        crate::maintain::startup(&store)
            .await
            .expect("second maintain");
        run(&store, None).await.expect("second seed");
        let after_second = row_counts(&read.client).await;
        let imports: i64 = read
            .client
            .query_one(
                "SELECT count(*) FROM import_sessions WHERE id = $1",
                &[&import_id],
            )
            .await
            .unwrap()
            .get(0);
        let legacy_view_after: i64 = read
            .client
            .query_one("SELECT count(*) FROM views WHERE id = $1", &[&view])
            .await
            .unwrap()
            .get(0);
        let legacy_state_after: Value = read
            .client
            .query_one("SELECT state FROM views WHERE id = $1", &[&view])
            .await
            .unwrap()
            .get("state");
        assert_eq!(legacy_view, 1);
        assert_eq!(legacy_view_after, 1);
        assert_eq!(legacy_state_after, legacy_state);
        assert_eq!(imports, 1);
        assert_eq!(
            after_first, after_second,
            "seed added rows on the second run"
        );
        assert!(after_second.objects > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RowCounts {
        objects: i64,
        object_values: i64,
        links: i64,
        views: i64,
        workspaces: i64,
        users: i64,
        import_sessions: i64,
        assortments: i64,
        definition_revisions: i64,
        principals: i64,
        proposals: i64,
    }

    async fn row_counts(client: &tokio_postgres::Client) -> RowCounts {
        async fn count(client: &tokio_postgres::Client, table: &str) -> i64 {
            client
                .query_one(&format!("SELECT count(*)::bigint FROM {table}"), &[])
                .await
                .unwrap()
                .get(0)
        }
        RowCounts {
            objects: count(client, "objects").await,
            object_values: count(client, "object_values").await,
            links: count(client, "links").await,
            views: count(client, "views").await,
            workspaces: count(client, "workspaces").await,
            users: count(client, "users").await,
            import_sessions: count(client, "import_sessions").await,
            assortments: count(client, "assortments").await,
            definition_revisions: count(client, "definition_revisions").await,
            principals: count(client, "principals").await,
            proposals: count(client, "proposals").await,
        }
    }

    struct DbGuard {
        admin_url: String,
        name: String,
    }

    impl Drop for DbGuard {
        fn drop(&mut self) {
            let url = self.admin_url.clone();
            let name = self.name.clone();
            let _ = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("cleanup");
                runtime.block_on(async {
                    let mut config = braian_store::admin::parse_config(&url).unwrap();
                    config.dbname("postgres");
                    if let Ok(admin) = braian_store::admin::PgConn::connect(config).await {
                        let _ = braian_store::admin::drop_database(&admin.client, &name).await;
                    }
                });
            })
            .join();
        }
    }

    fn english_name(model: &CompiledModel, object: &Object) -> String {
        let Some(attribute) = model.attribute_by_key("name") else {
            return String::new();
        };
        object
            .values
            .get(&attribute.id)
            .and_then(|value| value.get("en"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    }

    fn assortment_includes(query: &Value, labels: &[String]) -> bool {
        match query.get("labels") {
            Some(Value::String(code)) => labels.iter().any(|label| label == code),
            Some(Value::Array(codes)) => codes.iter().all(|code| {
                code.as_str()
                    .is_some_and(|code| labels.iter().any(|label| label == code))
            }),
            _ => false,
        }
    }
}
