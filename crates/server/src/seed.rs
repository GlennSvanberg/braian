//! Synthetic outdoor and building-materials catalog. Safe to run more than once.

use std::collections::BTreeMap;

use braian_model::{DefinitionSet, Object, ObjectId};
use braian_rules::CompiledModel;
use braian_search::Typesense;
use braian_store::{Actor, BatchKind, Error, ObjectOp, Store};
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

    let workspaces = [
        (
            "018f2000-0000-7000-8000-000000000001",
            "Sätt upp systemet",
            true,
            "braian-setup",
        ),
        (
            "018f2000-0000-7000-8000-000000000002",
            "Datamodell",
            false,
            "braian-model",
        ),
        (
            "018f2000-0000-7000-8000-000000000003",
            "Berika objekt",
            false,
            "braian-enrich",
        ),
        (
            "018f2000-0000-7000-8000-000000000004",
            "Granska",
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
            "Tält",
            &json!({ "type": "artikel", "text": "talt", "group": "sortiment:talt" }),
        )
        .await?;
    store
        .register_job_type(
            "dq.status",
            "Statuskontroll",
            &["status".into()],
            Some(Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1")?),
        )
        .await?;
    store
        .register_view_type(
            "leverantor",
            "Leverantör",
            Some("https://example.invalid/embed?object={object}&token={token}"),
        )
        .await?;
    if let Some(search) = search {
        braian_indexer::reindex(store, search).await?;
    }
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
            "Översikt",
            "main",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000102",
            "018f2000-0000-7000-8000-000000000001",
            "data_model",
            "Att göra",
            "main",
            1,
            json!({"checklist": ["languages", "units", "types", "import"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000201",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Struktur",
            "left",
            0,
            json!({"mode": "tree"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000202",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Klass",
            "right",
            0,
            json!({"mode": "editor"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000203",
            "018f2000-0000-7000-8000-000000000002",
            "data_model",
            "Konsekvens",
            "right",
            1,
            json!({"mode": "impact"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000301",
            "018f2000-0000-7000-8000-000000000003",
            "structure_tree",
            "Sortiment",
            "left",
            0,
            json!({"expanded": ["friluft", "bygg"]}),
        ),
        (
            "018f2000-0000-7000-8000-000000000302",
            "018f2000-0000-7000-8000-000000000003",
            "object_table",
            "Objekt",
            "center",
            0,
            json!({"columns": ["nummer", "namn", "type", "status", "farg", "vikt", "langd"], "sort": "nummer"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000303",
            "018f2000-0000-7000-8000-000000000003",
            "object_detail",
            "Detalj",
            "right",
            0,
            json!({"language": "sv"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000304",
            "018f2000-0000-7000-8000-000000000003",
            "graph",
            "Graf",
            "right",
            1,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000305",
            "018f2000-0000-7000-8000-000000000003",
            "history",
            "Historik",
            "right",
            2,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000401",
            "018f2000-0000-7000-8000-000000000004",
            "proposal",
            "Förslag",
            "left",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000402",
            "018f2000-0000-7000-8000-000000000004",
            "batch_log",
            "Batchlogg",
            "right",
            0,
            json!({}),
        ),
        (
            "018f2000-0000-7000-8000-000000000403",
            "018f2000-0000-7000-8000-000000000004",
            "job_history",
            "Jobb",
            "right",
            1,
            json!({"type": "bulk.write"}),
        ),
        (
            "018f2000-0000-7000-8000-000000000404",
            "018f2000-0000-7000-8000-000000000003",
            "embed",
            "Externt",
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
