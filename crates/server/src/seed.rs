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
    seed_demo_surface(store, &user).await?;
    match store.definition_set().await {
        Err(Error::NoDefinitions) => {
            let defs: DefinitionSet = serde_json::from_str(DEFINITIONS)?;
            CompiledModel::compile(&defs)
                .map_err(|errors| anyhow::anyhow!("{}", errors[0].message))?;
            store.put_definitions(&actor, &defs).await?;
        }
        Ok(_) => {}
        Err(error) => return Err(error.into()),
    }
    let defs = store.definition_set().await?;
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

async fn seed_demo_surface(store: &Store, user: &braian_store::UserRecord) -> anyhow::Result<()> {
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
    store.delete_view_type("leverantor").await?;
    save_views(store).await?;
    store
        .save_assortment(
            Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1")?,
            user.id,
            "Cheese",
            &json!({ "type": "article", "group": "food:cheese" }),
        )
        .await?;
    store
        .register_job_type(
            "dq.status",
            "Status check",
            &["status".into()],
            Some(Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1")?),
        )
        .await?;
    store
        .save_assortment(
            Uuid::parse_str("018f2000-0000-7000-8000-0000000000a2")?,
            user.id,
            "Organic",
            &json!({ "type": "article", "text": "Organic" }),
        )
        .await?;
    store
        .register_job_type("bulk.write", "Bulk write", &["status".into()], None)
        .await?;
    seed_job_run(
        store,
        "018f2000-0000-7000-8000-0000000000b1",
        "dq.status",
        user.principal_id,
    )
    .await?;
    seed_job_run(
        store,
        "018f2000-0000-7000-8000-0000000000b2",
        "bulk.write",
        user.principal_id,
    )
    .await?;
    seed_proposal(store, user.principal_id).await?;
    Ok(())
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

async fn save_views(store: &Store) -> anyhow::Result<()> {
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
            "Assortments",
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
            "Classes",
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
            "Assortments",
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
            "Classes",
            "right",
            3,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000701",
            "018f2000-0000-7000-8000-000000000007",
            "assortment_list",
            "Assortments",
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
    for (id, workspace, kind, title, pane, position, state) in views {
        store
            .save_view(
                Uuid::parse_str(id)?,
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
            "user",
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
        assert!(defs.value_lists.iter().all(|list| list.key != "farg"));
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
}
