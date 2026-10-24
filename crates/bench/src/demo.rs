use std::time::Instant;

use openpim_store::admin::{self, PgConn};
use openpim_store::{Actor, BatchKind, ObjectOp, PrincipalKind, STORAGE_FORM, Store};
use openpim_synth::{EditKind, Tier, definitions, edits, objects_iter};

pub async fn run(database_url: &str, tier: Tier, count: usize, seed: u64) -> anyhow::Result<()> {
    let database = "openpim_demo";
    let mut admin_config = admin::parse_config(database_url)?;
    admin_config.dbname("postgres");
    let admin = PgConn::connect(admin_config).await?;
    admin::drop_database(&admin.client, database).await.ok();
    admin::create_database(&admin.client, database).await?;
    drop(admin);

    let mut config = admin::parse_config(database_url)?;
    config.dbname(database);
    let store = Store::connect_config(config.clone(), STORAGE_FORM).await?;
    let defs = definitions(tier, seed);
    let human = store.create_principal(PrincipalKind::Human, "Ada").await?;
    let agent = store
        .create_principal(PrincipalKind::Agent, "Scout")
        .await?;
    let bulk = Actor {
        actor: agent,
        on_behalf_of: Some(human),
        kind: BatchKind::Bulk,
        source: "demo-2".into(),
    };
    store.put_definitions(&bulk, &defs).await?;

    let started = Instant::now();
    let report = store
        .bulk_create(
            &bulk,
            futures::stream::iter(objects_iter(&defs, tier, seed, 0..count)),
            500,
        )
        .await?;
    let elapsed = started.elapsed();
    if !report.errors.is_empty() {
        anyhow::bail!("{} rows failed: {:?}", report.errors.len(), report.errors);
    }
    let per_second = count as f64 / elapsed.as_secs_f64();
    println!(
        "demo-2: loaded {count} {} objects into {database} as {} in {:.1}s ({per_second:.0}/s)",
        tier.name(),
        STORAGE_FORM.name(),
        elapsed.as_secs_f64()
    );

    let type_id = defs.types[0].id;
    let loaded = store
        .get_by_key(type_id, "SKU-00000000")
        .await?
        .ok_or_else(|| anyhow::anyhow!("SKU-00000000 was not stored"))?;
    println!(
        "fetched {} key {} version {}",
        loaded.object.id, loaded.key, loaded.version
    );

    let edit = edits(&defs, tier, seed, EditKind::OneLanguage, 0..1)
        .next()
        .expect("edit");
    let interactive = Actor {
        kind: BatchKind::Interactive,
        ..bulk.clone()
    };
    let updated = store
        .write_batch(
            &interactive,
            vec![ObjectOp::Update {
                id: loaded.object.id,
                patch: openpim_model::ObjectPatch {
                    values: edit.patch,
                    groups: Default::default(),
                },
                base_version: loaded.version,
                read_version: loaded.version,
            }],
        )
        .await?;
    println!(
        "agent {} on behalf of {} updated {} to version {}",
        agent, human, loaded.object.id, updated.versions[0].1
    );

    let other = objects_iter(&defs, tier, seed, 1..2)
        .next()
        .expect("second object");
    store
        .write_batch(
            &interactive,
            vec![ObjectOp::Delete {
                id: other.id,
                base_version: None,
            }],
        )
        .await?;
    println!("deleted {}", other.id);

    println!("changes for {}:", loaded.object.id);
    for change in store.changes_for(loaded.object.id).await? {
        let behalf = change
            .on_behalf_of
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "  seq {} {} actor {} on behalf of {behalf} at {} version {:?}",
            change.seq,
            change.op.as_str(),
            change.actor,
            change.at,
            change.version
        );
        if let Some(diff) = change.diff {
            println!("  diff {}", serde_json::to_string(&diff)?);
        }
    }
    let meter = PgConn::connect(config).await?;
    let row = meter
        .client
        .query_one(
            "SELECT pg_total_relation_size('object_values') + pg_total_relation_size('changes')",
            &[],
        )
        .await?;
    let bytes: i64 = row.get(0);
    println!(
        "object_values + changes: {:.1} MB",
        bytes as f64 / 1_048_576.0
    );
    Ok(())
}
