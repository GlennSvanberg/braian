//! Synthetic outdoor and building-materials catalog. Safe to run more than once.

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
        let key = row["values"]["nummer"].as_str().unwrap_or("");
        let id = ObjectId::from_uuid(
            Uuid::parse_str(row["id"].as_str().unwrap_or_default())
                .map_err(|error| anyhow::anyhow!("object id: {error}"))?,
        );
        // A previous run, or an edit that changed the business key, still owns this id.
        if store.get_object(id).await?.is_some() || store.get_by_key(type_id, key).await?.is_some()
        {
            continue;
        }
        let structure = model
            .structure_id("sortiment")
            .ok_or_else(|| anyhow::anyhow!("sortiment is missing"))?;
        let group_key = row["group"].as_str().unwrap_or("");
        let group = model
            .group_id("sortiment", group_key)
            .ok_or_else(|| anyhow::anyhow!("unknown group {group_key}"))?;
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
            groups: BTreeMap::from([(structure, group)]),
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
        if let Some(links) = row.get("links").and_then(Value::as_array) {
            for link in links {
                let to = Uuid::parse_str(link["to"].as_str().unwrap_or_default())?;
                let kind = link["kind"].as_str().unwrap_or("related");
                store
                    .set_relation(id, ObjectId::from_uuid(to), kind)
                    .await?;
            }
        }
    }

    let target = std::env::var("BRAIAN_SEED_COUNT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(3_000);
    if target > 0 {
        seed_catalog(store, &actor, &model, defs.model_revision, target).await?;
    }

    if let Some(search) = search {
        braian_indexer::reindex(store, search).await?;
    }
    Ok(())
}

async fn seed_demo_surface(
    store: &Store,
    user: &braian_store::UserRecord,
) -> anyhow::Result<()> {
    let workspaces = [
        (
            "018f2000-0000-7000-8000-000000000001",
            "Set up the system",
            true,
            "braian-setup",
        ),
        (
            "018f2000-0000-7000-8000-000000000002",
            "Data model",
            false,
            "braian-model",
        ),
        (
            "018f2000-0000-7000-8000-000000000003",
            "Enrich objects",
            false,
            "braian-enrich",
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
    save_views(store).await?;
    store
        .save_assortment(
            Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1")?,
            user.id,
            "Tents",
            &json!({ "type": "artikel", "group": "sortiment:talt" }),
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
            "Timber",
            &json!({ "type": "artikel", "group": "sortiment:virke" }),
        )
        .await?;
    store
        .register_view_type(
            "leverantor",
            "Supplier",
            Some("https://example.invalid/embed?object={object}&token={token}"),
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

fn layout(workspace: &str) -> Value {
    match workspace {
        "018f2000-0000-7000-8000-000000000003" => json!({
            "direction": "row",
            "ratio": 0.22,
            "first": { "tabs": ["018f2000-0000-7000-8000-000000000301"] },
            "second": {
                "direction": "row",
                "ratio": 0.62,
                "first": { "tabs": ["018f2000-0000-7000-8000-000000000302"] },
                "second": { "tabs": [
                    "018f2000-0000-7000-8000-000000000303",
                    "018f2000-0000-7000-8000-000000000304",
                    "018f2000-0000-7000-8000-000000000305"
                ] }
            }
        }),
        "018f2000-0000-7000-8000-000000000002" => json!({
            "direction": "row",
            "ratio": 0.34,
            "first": { "tabs": ["018f2000-0000-7000-8000-000000000201"] },
            "second": { "tabs": ["018f2000-0000-7000-8000-000000000202", "018f2000-0000-7000-8000-000000000203"] }
        }),
        "018f2000-0000-7000-8000-000000000004" => json!({
            "direction": "row",
            "ratio": 0.62,
            "first": { "tabs": ["018f2000-0000-7000-8000-000000000401"] },
            "second": { "tabs": ["018f2000-0000-7000-8000-000000000402", "018f2000-0000-7000-8000-000000000403"] }
        }),
        _ => json!({
            "direction": "row",
            "ratio": 0.5,
            "first": { "tabs": ["018f2000-0000-7000-8000-000000000101"] },
            "second": { "tabs": ["018f2000-0000-7000-8000-000000000102"] }
        }),
    }
}

async fn save_views(store: &Store) -> anyhow::Result<()> {
    let views = [
        (
            "018f2000-0000-7000-8000-000000000101",
            "018f2000-0000-7000-8000-000000000001",
            "dashboard",
            "Overview",
            "main",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000102",
            "018f2000-0000-7000-8000-000000000001",
            "data_model",
            "To do",
            "main",
            1,
            json!({"checklist": ["languages", "units", "types", "import"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000201",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Structure",
            "left",
            0,
            json!({"mode": "tree"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000202",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Class",
            "right",
            0,
            json!({"mode": "editor"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000203",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Impact",
            "right",
            1,
            json!({"mode": "impact"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000301",
            "018f2000-0000-7000-8000-000000000003",
            "structure_tree",
            "Assortment",
            "left",
            0,
            json!({"expanded": ["friluft", "bygg"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000302",
            "018f2000-0000-7000-8000-000000000003",
            "object_table",
            "Objects",
            "center",
            0,
            json!({"columns": ["nummer", "namn", "type", "status", "farg", "vikt", "langd"], "sort": "nummer"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000303",
            "018f2000-0000-7000-8000-000000000003",
            "object_detail",
            "Detail",
            "right",
            0,
            json!({"language": "sv"}),
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
            "018f2000-0000-7000-8000-000000000404",
            "018f2000-0000-7000-8000-000000000003",
            "embed",
            "External",
            "right",
            3,
            json!({"url": "/embed/preview?object={object}&key={key}"}),
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

fn seed_target_kind(index: u32) -> (&'static str, &'static str) {
    match index % 10 {
        0 => ("produkt", "friluft"),
        1 => ("produkt", "bygg"),
        2 => ("variant", "talt"),
        3 => ("variant", "sovsack"),
        _ => match index % 3 {
            0 => ("artikel", "talt"),
            1 => ("artikel", "sovsack"),
            _ => ("artikel", "virke"),
        },
    }
}

fn product_name(group: &str, index: u32) -> (String, String, String) {
    let family = match group {
        "talt" => ["Kupoltält", "Tunneltält", "Familjetält", "Vindskydd"],
        "sovsack" => ["Dunsovsäck", "Syntetsäck", "Sommarsäck", "Vintersäck"],
        "virke" => ["Regel", "Råspont", "Läkt", "Fasadpanel"],
        "bygg" => ["Gipsskiva", "Isolering", "Plywood", "Trallskruv"],
        _ => ["Ryggsäck", "Stormkök", "Liggunderlag", "Vattenflaska"],
    };
    let en = match group {
        "talt" => ["Dome tent", "Tunnel tent", "Family tent", "Tarp"],
        "sovsack" => ["Down bag", "Synthetic bag", "Summer bag", "Winter bag"],
        "virke" => ["Stud", "Tongue and groove", "Batten", "Cladding"],
        "bygg" => ["Plasterboard", "Insulation", "Plywood", "Deck screw"],
        _ => ["Backpack", "Stove", "Sleeping pad", "Bottle"],
    };
    let de = match group {
        "talt" => ["Kuppelzelt", "Tunnelzelt", "Familienzelt", "Tarp"],
        "sovsack" => [
            "Daunenschlafsack",
            "Kunstfaserschlafsack",
            "Sommerschlafsack",
            "Winterschlafsack",
        ],
        "virke" => ["Kantholz", "Profilbrett", "Latte", "Fassadenprofil"],
        "bygg" => ["Gipskarton", "Dämmung", "Sperrholz", "Terrassenschraube"],
        _ => ["Rucksack", "Kocher", "Isomatte", "Flasche"],
    };
    let slot = (index as usize) % 4;
    let n = index + 1;
    (
        format!("{} {n}", family[slot]),
        format!("{} {n}", en[slot]),
        format!("{} {n}", de[slot]),
    )
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
    let structure = model
        .structure_id("sortiment")
        .ok_or_else(|| anyhow::anyhow!("sortiment is missing"))?;
    let bulk = Actor {
        kind: BatchKind::Bulk,
        source: "seed-catalog".into(),
        ..actor.clone()
    };
    let batch = store.open_batch(&bulk).await?;
    let mut pending = Vec::new();
    let mut inserted = 0u32;
    for index in 0..target {
        let key = format!("SYN-{index:06}");
        if existing.contains(&key) {
            continue;
        }
        let (type_key, group_key) = seed_target_kind(index);
        let type_id = model
            .type_id(type_key)
            .ok_or_else(|| anyhow::anyhow!("unknown type {type_key}"))?;
        let group = model
            .group_id("sortiment", group_key)
            .ok_or_else(|| anyhow::anyhow!("unknown group {group_key}"))?;
        let (sv, en, de) = product_name(group_key, index);
        let mut values = BTreeMap::new();
        put_attr(model, &mut values, "nummer", json!(key))?;
        put_attr(
            model,
            &mut values,
            "namn",
            json!({ "sv": sv, "en": en, "de": de }),
        )?;
        put_attr(
            model,
            &mut values,
            "status",
            json!(match index % 5 {
                0 => "utkast",
                4 => "slut",
                _ => "ok",
            }),
        )?;
        put_attr(model, &mut values, "aktiv", json!(index % 17 != 0))?;
        if type_key != "variant" {
            put_attr(
                model,
                &mut values,
                "beskrivning",
                json!({
                    "sv": format!("{sv} för bygg och friluft. Syntetiskt demoobjekt."),
                    "en": format!("{en} for building and outdoor use. Synthetic demo object."),
                    "de": format!("{de} für Bau und Outdoor. Synthetisches Demo-Objekt."),
                }),
            )?;
        }
        if type_key != "produkt" {
            let color = match index % 4 {
                0 => "svart",
                1 => "gra",
                2 => "gron",
                _ => "bla",
            };
            put_attr(model, &mut values, "farg", json!([color]))?;
        }
        if type_key == "artikel" {
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
                "lanserad",
                json!(dates[(index as usize) % dates.len()]),
            )?;
            if index % 5 == 0 {
                put_attr(
                    model,
                    &mut values,
                    "ersatter",
                    json!("018f1000-0000-7000-8000-000000000011"),
                )?;
            }
        }
        if matches!(group_key, "talt" | "sovsack" | "virke") {
            put_attr(
                model,
                &mut values,
                "vikt",
                json!({ "value": format!("{}", 400 + (index % 80) * 25), "unit": "g" }),
            )?;
        }
        if matches!(group_key, "talt" | "virke") {
            put_attr(
                model,
                &mut values,
                "langd",
                json!({ "value": format!("{}", 80 + (index % 40) * 5), "unit": "cm" }),
            )?;
        }
        if group_key == "talt" {
            put_attr(
                model,
                &mut values,
                "spann",
                json!({ "min": "180", "max": "240", "unit": "cm" }),
            )?;
        }
        let object = Object {
            id: synthetic_id(index),
            type_id,
            model_revision: revision,
            groups: BTreeMap::from([(structure, group)]),
            values,
        };
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
    println!("seed catalog inserted {inserted} synthetic objects (target {target})");
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
        store.set_proposal_title(id, "Accept ART-TALT-3").await?;
        return Ok(());
    }
    let object_id = ObjectId::from_uuid(Uuid::parse_str("018f1000-0000-7000-8000-000000000012")?);
    let Some(stored) = store.get_object(object_id).await? else {
        return Ok(());
    };
    let version = i64::try_from(stored.version).unwrap_or(1);
    store
        .create_proposal(
            id,
            "Accept ART-TALT-3",
            "user",
            actor,
            None,
            false,
            &[ProposalRow {
                id: Uuid::parse_str("018f2000-0000-7000-8000-0000000000c2")?,
                object_id: Some(object_id.as_uuid()),
                object_key: Some(stored.key),
                type_key: Some("artikel".into()),
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
