//! Synthetic food catalog shaped like Open Food Facts. Safe to run more than once.
//!
//! Hero objects live in `demo/objects.json`. `BRAIAN_SEED_COUNT` (default 240)
//! scales the same deterministic generator. Together with the heroes that is
//! about 250 objects: suppliers, products and articles.

use std::collections::{BTreeMap, HashSet};

use braian_model::{AttributeId, DefinitionSet, Object, ObjectId};
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
        if store.get_object(id).await?.is_some() || store.get_by_key(type_id, key).await?.is_some()
        {
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
        seed_catalog(store, &actor, &model, defs.model_revision, target).await?;
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
                tabs(&["018f2000-0000-7000-8000-000000000202"]),
                tabs(&["018f2000-0000-7000-8000-000000000203"]),
            ),
        ),
        "018f2000-0000-7000-8000-000000000004" => split(
            "horizontal",
            0.62,
            tabs(&["018f2000-0000-7000-8000-000000000401"]),
            tabs(&[
                "018f2000-0000-7000-8000-000000000402",
                "018f2000-0000-7000-8000-000000000403",
            ]),
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
            json!({"columns": ["number", "company_name", "country", "city", "status"], "sort": "number", "object_type": "018f0000-0000-7000-8000-000000000202", "type_key": "supplier"}),
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
                "company_name",
                json!(format!("{} Foods {}", brand_for(index), index + 1)),
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
            if let Some(product) = slot.product {
                put_attr(
                    model,
                    &mut values,
                    "product",
                    json!(synthetic_id(product).as_uuid().to_string()),
                )?;
            }
            if let Some(supplier) = slot.supplier {
                put_attr(
                    model,
                    &mut values,
                    "supplier",
                    json!(synthetic_id(supplier).as_uuid().to_string()),
                )?;
            }
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
    let mut values = fill_values(model, &slot)?;
    if slot.type_key == "article" && index.is_multiple_of(11) {
        let article_n = index - suppliers - products;
        if article_n > 0 {
            let previous = index - 1;
            put_attr(
                model,
                &mut values,
                "replaces",
                json!(synthetic_id(previous).as_uuid().to_string()),
            )?;
        }
    }
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
    model: &CompiledModel,
    revision: u64,
    target: u32,
) -> anyhow::Result<()> {
    let existing: HashSet<String> = store.keys_with_prefix("SYN-").await?.into_iter().collect();
    if existing.len() >= target as usize {
        println!(
            "seed catalog already has {} synthetic objects (target {target})",
            existing.len()
        );
        return Ok(());
    }
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
    println!(
        "seed catalog inserted {inserted} synthetic objects (target {target}, {suppliers} suppliers, {products} products, {articles} articles)"
    );
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
        assert!(
            defs.attributes
                .iter()
                .any(|item| item.key == "product" && item.required)
        );
        assert!(
            defs.attributes
                .iter()
                .any(|item| item.key == "supplier" && item.required)
        );
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
    /// replace a saved view or selection. Renaming an attribute key leaves
    /// those column ids in place.
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

        let mut renamed = defs.clone();
        renamed.model_revision += 1;
        braian_model::apply_all(
            &mut renamed,
            &[braian_model::ModelChange::RenameAttributeKey {
                key: "number".into(),
                new_key: "item_number".into(),
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
        assert!(columns.iter().any(|column| column == &number.id));
        assert!(
            columns
                .iter()
                .all(|column| column != "number" && column != "item_number")
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
    }

    impl SeedEnv {
        fn set(pairs: &[(&'static str, &str)]) -> Self {
            let previous = pairs
                .iter()
                .map(|(key, value)| {
                    let old = std::env::var_os(key);
                    unsafe { std::env::set_var(key, value) };
                    (*key, old)
                })
                .collect();
            Self { previous }
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
