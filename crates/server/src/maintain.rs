use braian_store::Store;

/// What every process runs after migrations: the service principal, then the
/// stored-model upgrades. Both upgrades call the same principal upsert.
pub(crate) async fn startup(store: &Store) -> anyhow::Result<()> {
    store.ensure_system_principal().await?;
    store.upgrade_standard_attributes().await?;
    store.upgrade_object_refs().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use braian_store::testing::TestDb;
    use braian_store::{PrincipalKind, SYSTEM_PRINCIPAL};
    use serde_json::json;
    use tokio_postgres::NoTls;
    use uuid::Uuid;

    use super::startup;

    #[tokio::test]
    async fn startup_twice_keeps_one_system_principal() {
        let db = TestDb::create().await;
        startup(db.store()).await.unwrap();
        startup(db.store()).await.unwrap();

        let client = connect(&db.database_url()).await;
        let rows = principal_rows(&client).await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        let system = &rows[0];
        assert_eq!(system.id, system_id(&client).await);
        assert_eq!(system.id, SYSTEM_PRINCIPAL.as_uuid());
        assert_eq!(system.kind, "integration");
        assert_eq!(system.name, "System");
        assert_eq!(system.handle.as_deref(), Some("system"));
        braian_api::stored_principal_cannot_approve_discard_or_delete(db.store())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn legacy_system_principal_is_merged_onto_the_stable_id() {
        let db = TestDb::create().await;
        let client = connect(&db.database_url()).await;
        let legacy = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO principals (id, kind, name, handle)
                 VALUES ($1, 'integration', 'System', 'system')",
                &[&legacy],
            )
            .await
            .unwrap();
        let batch = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO batches (id, actor, kind, source)
                 VALUES ($1, $2, 'interactive', 'test')",
                &[&batch, &legacy],
            )
            .await
            .unwrap();
        let object = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO objects (id, type_id, key, version, model_revision, updated_by)
                 VALUES ($1, $2, 'legacy', 1, 0, $3)",
                &[&object, &Uuid::now_v7(), &legacy],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO changes (batch_id, object_id, version, op, actor)
                 VALUES ($1, $2, 1, 'create', $3)",
                &[&batch, &object, &legacy],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO command_frecency (principal_id, command_id, uses, last_at)
                 VALUES ($1, 'open-object', 3, now())",
                &[&legacy],
            )
            .await
            .unwrap();

        startup(db.store()).await.unwrap();
        startup(db.store()).await.unwrap();

        let rows = principal_rows(&client).await;
        let systems: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == "integration" && row.name == "System")
            .collect();
        assert_eq!(systems.len(), 1, "{rows:?}");
        let system = systems[0];
        assert_eq!(system.id, SYSTEM_PRINCIPAL.as_uuid());
        assert_eq!(system.handle.as_deref(), Some("system"));
        assert!(rows.iter().all(|row| row.id != legacy), "{rows:?}");

        let batch_actor: Uuid = client
            .query_one("SELECT actor FROM batches WHERE id = $1", &[&batch])
            .await
            .unwrap()
            .get(0);
        assert_eq!(batch_actor, SYSTEM_PRINCIPAL.as_uuid());
        let change_actor: Uuid = client
            .query_one("SELECT actor FROM changes WHERE batch_id = $1", &[&batch])
            .await
            .unwrap()
            .get(0);
        assert_eq!(change_actor, SYSTEM_PRINCIPAL.as_uuid());
        let updated_by: Uuid = client
            .query_one("SELECT updated_by FROM objects WHERE id = $1", &[&object])
            .await
            .unwrap()
            .get(0);
        assert_eq!(updated_by, SYSTEM_PRINCIPAL.as_uuid());
        let frecency: Uuid = client
            .query_one(
                "SELECT principal_id FROM command_frecency WHERE command_id = 'open-object'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(frecency, SYSTEM_PRINCIPAL.as_uuid());
        assert!(
            !rows.iter().any(|row| row.id == legacy),
            "legacy System row must be gone after merge: {rows:?}"
        );
        assert_eq!(
            rows
                .iter()
                .filter(|row| row.kind == "integration" && row.name == "System")
                .count(),
            1,
            "{rows:?}"
        );
    }

    #[tokio::test]
    async fn a_person_named_System_is_left_untouched_while_legacy_system_merges() {
        let db = TestDb::create().await;
        let client = connect(&db.database_url()).await;
        let person = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO principals (id, kind, name, handle)
                 VALUES ($1, 'human', 'System', 'nina')",
                &[&person],
            )
            .await
            .unwrap();
        let legacy = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO principals (id, kind, name, handle)
                 VALUES ($1, 'integration', 'System', 'system')",
                &[&legacy],
            )
            .await
            .unwrap();
        let batch = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO batches (id, actor, kind, source)
                 VALUES ($1, $2, 'interactive', 'test')",
                &[&batch, &legacy],
            )
            .await
            .unwrap();

        startup(db.store()).await.unwrap();
        startup(db.store()).await.unwrap();

        let rows = principal_rows(&client).await;
        let human = rows.iter().find(|row| row.id == person).expect("human System");
        assert_eq!(human.kind, "human");
        assert_eq!(human.name, "System");
        assert_eq!(human.handle.as_deref(), Some("nina"));
        assert!(rows.iter().all(|row| row.id != legacy), "{rows:?}");
        let system = rows
            .iter()
            .find(|row| row.id == SYSTEM_PRINCIPAL.as_uuid())
            .expect("stable system");
        assert_eq!(system.kind, "integration");
        assert_eq!(system.name, "System");
        let batch_actor: Uuid = client
            .query_one("SELECT actor FROM batches WHERE id = $1", &[&batch])
            .await
            .unwrap()
            .get(0);
        assert_eq!(batch_actor, SYSTEM_PRINCIPAL.as_uuid());
    }

    #[tokio::test]
    async fn a_person_named_system_keeps_their_row() {
        let db = TestDb::create().await;
        let client = connect(&db.database_url()).await;
        let person = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO principals (id, kind, name, handle)
                 VALUES ($1, 'human', 'Nina', 'system')",
                &[&person],
            )
            .await
            .unwrap();
        let batch = Uuid::now_v7();
        client
            .execute(
                "INSERT INTO batches (id, actor, kind, source)
                 VALUES ($1, $2, 'interactive', 'test')",
                &[&batch, &person],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO definition_revisions (model_revision, definitions, batch_id)
                 VALUES (1, $1, $2)",
                &[&catalog(), &batch],
            )
            .await
            .unwrap();

        startup(db.store()).await.unwrap();
        startup(db.store()).await.unwrap();

        let rows = principal_rows(&client).await;
        assert_eq!(rows.len(), 2, "{rows:?}");
        let nina = rows.iter().find(|row| row.id == person).expect("person");
        assert_eq!(nina.kind, "human");
        assert_eq!(nina.name, "Nina");
        assert_eq!(nina.handle.as_deref(), Some("system"));
        let stable = system_id(&client).await;
        let system = rows.iter().find(|row| row.id == stable).expect("system");
        assert_eq!(system.id, SYSTEM_PRINCIPAL.as_uuid());
        assert_eq!(system.kind, "integration");
        assert_eq!(system.name, "System");
        assert_eq!(system.handle.as_deref(), Some("system-2"));
        let record = db
            .store()
            .principal(SYSTEM_PRINCIPAL)
            .await
            .unwrap()
            .expect("system row");
        assert_eq!(record.kind, PrincipalKind::Integration);
        braian_api::stored_principal_cannot_approve_discard_or_delete(db.store())
            .await
            .unwrap();

        let revision: i64 = client
            .query_one("SELECT max(model_revision) FROM definition_revisions", &[])
            .await
            .unwrap()
            .get(0);
        let document: String = client
            .query_one(
                "SELECT definitions::text FROM definition_revisions
                 WHERE model_revision = $1",
                &[&revision],
            )
            .await
            .unwrap()
            .get(0);
        assert!(
            !document.contains("object_ref"),
            "startup left an object reference in the model: {document}"
        );
        startup(db.store()).await.unwrap();
        let again: i64 = client
            .query_one("SELECT max(model_revision) FROM definition_revisions", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(again, revision);
        let rows = principal_rows(&client).await;
        assert_eq!(rows.len(), 2, "{rows:?}");
        let nina = rows.iter().find(|row| row.id == person).expect("person");
        assert_eq!(nina.kind, "human");
        assert_eq!(nina.name, "Nina");
        assert_eq!(nina.handle.as_deref(), Some("system"));

        client
            .execute(
                "UPDATE principals SET handle = 'nina' WHERE id = $1",
                &[&person],
            )
            .await
            .unwrap();
        startup(db.store()).await.unwrap();
        let rows = principal_rows(&client).await;
        let nina = rows.iter().find(|row| row.id == person).expect("person");
        assert_eq!(nina.kind, "human");
        assert_eq!(nina.name, "Nina");
        assert_eq!(nina.handle.as_deref(), Some("nina"));
        let system = rows
            .iter()
            .find(|row| row.id == SYSTEM_PRINCIPAL.as_uuid())
            .expect("system");
        assert_eq!(system.handle.as_deref(), Some("system"));
        assert_eq!(system.kind, "integration");
    }

    #[derive(Debug)]
    struct PrincipalRow {
        id: Uuid,
        kind: String,
        name: String,
        handle: Option<String>,
    }

    async fn connect(url: &str) -> tokio_postgres::Client {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
    }

    async fn principal_rows(client: &tokio_postgres::Client) -> Vec<PrincipalRow> {
        let rows = client
            .query(
                "SELECT id, kind, name, handle FROM principals ORDER BY created_at, id",
                &[],
            )
            .await
            .unwrap();
        rows.into_iter()
            .map(|row| PrincipalRow {
                id: row.get(0),
                kind: row.get(1),
                name: row.get(2),
                handle: row.get(3),
            })
            .collect()
    }

    async fn system_id(client: &tokio_postgres::Client) -> Uuid {
        client
            .query_one(
                "WITH raw AS (
                     SELECT decode(md5('principal:system'), 'hex') AS bytes
                 )
                 SELECT encode(
                     set_byte(
                         set_byte(bytes, 6, (get_byte(bytes, 6) & 15) | 48),
                         8,
                         (get_byte(bytes, 8) & 63) | 128
                     ),
                     'hex'
                 )::uuid
                 FROM raw",
                &[],
            )
            .await
            .unwrap()
            .get(0)
    }

    fn catalog() -> serde_json::Value {
        let product = "018f0000-0000-7000-8000-000000000200";
        let article = "018f0000-0000-7000-8000-000000000201";
        let number = "018f0000-0000-7000-8000-000000000100";
        let product_attr = "018f0000-0000-7000-8000-00000000011a";
        json!({
            "model_revision": 1,
            "languages": ["en"],
            "unit_families": [],
            "attributes": [
                {
                    "id": number,
                    "key": "number",
                    "labels": {"en": "Number"},
                    "language_dependent": false,
                    "multiple": false,
                    "kind": {"type": "text", "max_length": 64},
                    "index": {"searchable": true, "filterable": true, "sortable": true}
                },
                {
                    "id": product_attr,
                    "key": "product",
                    "labels": {"en": "Product"},
                    "language_dependent": false,
                    "multiple": false,
                    "required": true,
                    "kind": {"type": "object_ref", "type_key": "product"},
                    "index": {"searchable": false, "filterable": false, "sortable": false}
                }
            ],
            "types": [
                {
                    "id": product,
                    "key": "product",
                    "labels": {"en": "Product"},
                    "key_attribute": number,
                    "attributes": [number]
                },
                {
                    "id": article,
                    "key": "article",
                    "labels": {"en": "Article"},
                    "key_attribute": number,
                    "attributes": [number, product_attr]
                }
            ],
            "structures": []
        })
    }
}
