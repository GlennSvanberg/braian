//! A database that already applied main's migrations must accept this branch.
//!
//! refinery 0.8 aborts when an applied migration's checksum changes. CI points
//! `BRAIAN_BASELINE_MIGRATIONS` at main's `crates/store/migrations`,
//! applies those, then applies the migrations in this checkout.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use braian_store::admin::{self, PgConn};
use refinery::{Migration, Runner, load_sql_migrations};
use uuid::Uuid;

/// Checksum refinery recorded for V3 on the VPS, which matches main.
const MAIN_V3_CHECKSUM: u64 = 10_177_840_271_409_145_164;

struct EmptyDb {
    name: String,
    admin_url: String,
}

impl EmptyDb {
    async fn create() -> Self {
        let admin_url = env::var("BRAIAN_TEST_DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "BRAIAN_TEST_DATABASE_URL is not set. Point it at the maintenance database, for example postgres://braian:secret@localhost:5433/postgres"
            )
        });
        let name = format!("m{:032x}", Uuid::now_v7().as_u128());
        let mut config = admin::parse_config(&admin_url).unwrap();
        config.dbname("postgres");
        let admin = PgConn::connect(config).await.unwrap();
        admin::create_database(&admin.client, &name).await.unwrap();
        Self { name, admin_url }
    }

    async fn connect(&self) -> PgConn {
        PgConn::connect(self.config()).await.unwrap()
    }

    fn config(&self) -> tokio_postgres::Config {
        let mut config = admin::parse_config(&self.admin_url).unwrap();
        config.dbname(&self.name);
        config
    }
}

impl Drop for EmptyDb {
    fn drop(&mut self) {
        let url = self.admin_url.clone();
        let name = self.name.clone();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("migration test cleanup");
            runtime.block_on(async {
                for _ in 0..20 {
                    let Ok(mut config) = admin::parse_config(&url) else {
                        return;
                    };
                    config.dbname("postgres");
                    let Ok(admin) = PgConn::connect(config).await else {
                        return;
                    };
                    if admin::drop_database(&admin.client, &name).await.is_ok() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        })
        .join();
    }
}

fn branch_migrations() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn baseline_migrations() -> PathBuf {
    if let Ok(dir) = env::var("BRAIAN_BASELINE_MIGRATIONS") {
        let path = PathBuf::from(&dir);
        assert!(
            path.join("V3__app.sql").is_file(),
            "BRAIAN_BASELINE_MIGRATIONS={dir} has no V3__app.sql"
        );
        let baseline_max = max_migration_version(&load(&path));
        let branch_max = max_migration_version(&load(&branch_migrations()));
        assert!(
            baseline_max <= branch_max,
            "BRAIAN_BASELINE_MIGRATIONS={dir} goes up to V{baseline_max} but this checkout only has V{branch_max}"
        );
        return path;
    }
    panic!(
        "BRAIAN_BASELINE_MIGRATIONS is not set. Point it at main's crates/store/migrations before the branch."
    );
}

fn load(dir: &Path) -> Vec<Migration> {
    load_sql_migrations(dir).unwrap_or_else(|error| panic!("load {}: {error}", dir.display()))
}

fn v3_checksum(migrations: &[Migration]) -> u64 {
    migrations
        .iter()
        .find(|migration| migration.version() == 3)
        .unwrap_or_else(|| panic!("V3 is missing"))
        .checksum()
}

fn migration_version(file: &str) -> Option<u32> {
    let rest = file.strip_prefix('V')?;
    let (number, _) = rest.split_once('_')?;
    number.parse().ok()
}

fn max_migration_version(migrations: &[Migration]) -> i32 {
    migrations
        .iter()
        .map(|migration| migration.version() as i32)
        .max()
        .expect("at least one migration")
}

async fn migrate(client: &mut tokio_postgres::Client, dir: &Path) {
    let migrations = load(dir);
    Runner::new(&migrations)
        .run_async(client)
        .await
        .unwrap_or_else(|error| panic!("migrate {}: {error}", dir.display()));
}

async fn wal_level(client: &tokio_postgres::Client) -> String {
    let row = client.query_one("SHOW wal_level", &[]).await.unwrap();
    row.get(0)
}

async fn publication_exists(client: &tokio_postgres::Client) -> bool {
    let row = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'braian_changes')",
            &[],
        )
        .await
        .unwrap();
    row.get(0)
}

#[tokio::test]
async fn a_fresh_database_migrates_when_wal_level_is_not_logical() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let level = wal_level(&conn.client).await;
    assert_ne!(
        level, "logical",
        "CREATE PUBLICATION has to be exercised where it only warns"
    );
    migrate(&mut conn.client, &branch_migrations()).await;
    assert!(
        !publication_exists(&conn.client).await,
        "V15 should drop the publication V3 created"
    );
    let lanes: i64 = conn
        .client
        .query_one("SELECT COUNT(*) FROM index_lane", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(lanes, 2);
}

#[tokio::test]
async fn view_types_are_english_and_have_no_dashboard() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    migrate(&mut conn.client, &branch_migrations()).await;
    let rows = conn
        .client
        .query("SELECT key, title FROM view_types", &[])
        .await
        .unwrap();
    let mut titles = BTreeMap::new();
    for row in rows {
        titles.insert(row.get::<_, String>(0), row.get::<_, String>(1));
    }
    assert!(!titles.contains_key("dashboard"), "{titles:?}");
    let dashboards: i64 = conn
        .client
        .query_one(
            "SELECT COUNT(*) FROM views WHERE type_key = 'dashboard'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(dashboards, 0);
    assert_eq!(titles["structure_tree"], "Structure");
    assert_eq!(titles["object_table"], "Objects");
    assert_eq!(titles["object_detail"], "Detail");
    assert_eq!(titles["graph"], "Graph");
    assert_eq!(titles["data_model"], "Data model");
    assert_eq!(titles["history"], "History");
    assert_eq!(titles["batch_log"], "Batch log");
    assert_eq!(titles["job_history"], "Jobs");
    assert_eq!(titles["proposal"], "Proposals");
    assert_eq!(titles["embed"], "Embedded view");
    assert_eq!(titles["assortment_list"], "Selections");
    assert_eq!(titles["class_attributes"], "Group attributes");
    assert_eq!(titles["attribute_definitions"], "Attribute definitions");
}

#[tokio::test]
async fn attribute_definitions_view_is_added_to_an_existing_data_model() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    migrate(&mut conn.client, &branch_migrations()).await;
    let user = Uuid::now_v7();
    let principal = Uuid::now_v7();
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
             VALUES ($1, $2, 'ada@example.com', 'x', 'admin', 'Ada')",
            &[&user, &principal],
        )
        .await
        .unwrap();
    let workspace = Uuid::parse_str("018f2000-0000-7000-8000-000000000002").unwrap();
    let layout = serde_json::json!({
        "type": "split",
        "dir": "horizontal",
        "ratios": [0.22, 0.78],
        "children": [
            { "type": "tabs", "views": ["018f2000-0000-7000-8000-000000000201"], "active": 0 },
            {
                "type": "split",
                "dir": "horizontal",
                "ratios": [0.58, 0.42],
                "children": [
                    { "type": "tabs", "views": ["018f2000-0000-7000-8000-000000000202"], "active": 0 },
                    { "type": "tabs", "views": ["018f2000-0000-7000-8000-000000000203"], "active": 0 }
                ]
            }
        ]
    });
    conn.client
        .execute(
            "INSERT INTO workspaces (id, owner_id, name, locked, share_token, layout)
             VALUES ($1, $2, 'Data model', false, 'braian-model', $3)",
            &[&workspace, &user, &layout],
        )
        .await
        .unwrap();
    conn.client
        .batch_execute(include_str!(
            "../migrations/V21__attribute_definitions_view.sql"
        ))
        .await
        .unwrap();
    let views: i64 = conn
        .client
        .query_one(
            "SELECT COUNT(*) FROM views
             WHERE workspace_id = $1 AND type_key = 'attribute_definitions'",
            &[&workspace],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(views, 1);
    let stored: serde_json::Value = conn
        .client
        .query_one("SELECT layout FROM workspaces WHERE id = $1", &[&workspace])
        .await
        .unwrap()
        .get(0);
    assert!(
        stored
            .to_string()
            .contains("018f2000-0000-7000-8000-000000000204"),
        "{stored}"
    );
    conn.client
        .batch_execute(include_str!(
            "../migrations/V21__attribute_definitions_view.sql"
        ))
        .await
        .unwrap();
    let views: i64 = conn
        .client
        .query_one(
            "SELECT COUNT(*) FROM views
             WHERE workspace_id = $1 AND type_key = 'attribute_definitions'",
            &[&workspace],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        views, 1,
        "running the migration again must not add a second view"
    );
}

#[tokio::test]
async fn branch_migrations_apply_after_main() {
    let baseline = baseline_migrations();
    let branch = branch_migrations();
    let baseline_set = load(&baseline);
    let branch_set = load(&branch);
    assert_eq!(
        v3_checksum(&baseline_set),
        MAIN_V3_CHECKSUM,
        "baseline V3 is not the file the VPS already applied"
    );
    assert_eq!(
        v3_checksum(&branch_set),
        MAIN_V3_CHECKSUM,
        "this branch changed V3__app.sql; refinery will refuse to start on the existing database"
    );

    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let level = wal_level(&conn.client).await;
    assert_ne!(level, "logical");
    migrate(&mut conn.client, &baseline).await;
    assert!(
        !publication_exists(&conn.client).await,
        "main drops braian_changes in V15 even when wal_level is {level}"
    );
    migrate(&mut conn.client, &branch).await;
    assert!(!publication_exists(&conn.client).await);
    let stored: String = conn
        .client
        .query_one(
            "SELECT checksum FROM refinery_schema_history WHERE version = 3",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(stored, MAIN_V3_CHECKSUM.to_string());
    let latest: i32 = conn
        .client
        .query_one("SELECT MAX(version) FROM refinery_schema_history", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(latest, max_migration_version(&branch_set));
}

/// V17 reads `query.type` when it is set, and otherwise assigns the type with
/// the most matching live objects. A selection that already names a type keeps it.
#[tokio::test]
async fn selection_type_comes_from_the_query_or_the_most_hits() {
    let source = branch_migrations();
    let dir = env::temp_dir().join(format!("braian-v17-{}", Uuid::now_v7().simple()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let file = name.to_string_lossy();
        // V17 fills type_id. Later files (V18 rewrites query ids, V19 adds
        // updated_by) must not run until that column exists.
        if migration_version(&file).is_some_and(|version| version >= 17) {
            continue;
        }
        fs::copy(entry.path(), dir.join(&name)).unwrap();
    }

    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    migrate(&mut conn.client, &dir).await;

    let product = Uuid::parse_str("018f0000-0000-7000-8000-000000000200").unwrap();
    let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap();
    let principal = Uuid::now_v7();
    let batch = Uuid::now_v7();
    let user = Uuid::now_v7();
    let workspace = Uuid::now_v7();
    conn.client
        .execute(
            "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
            &[&principal],
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
            "INSERT INTO users (id, principal_id, email, password_hash, role, display_name)
             VALUES ($1, $2, 'ada@example.com', 'x', 'admin', 'Ada')",
            &[&user, &principal],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'Products')",
            &[&workspace, &user],
        )
        .await
        .unwrap();
    let definitions = serde_json::json!({
        "model_revision": 1,
        "languages": ["en"],
        "types": [
            {"id": product, "key": "product"},
            {"id": article, "key": "article"}
        ],
        "structures": [{
            "id": "018f0000-0000-7000-8000-000000000300",
            "key": "food",
            "groups": [{"id": "018f0000-0000-7000-8000-000000000400", "key": "cheese"}]
        }]
    });
    conn.client
        .execute(
            "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES (1, $1, $2)",
            &[&definitions, &batch],
        )
        .await
        .unwrap();

    for index in 0..6 {
        insert_object(&conn.client, product, &format!("P{index}"), index == 0).await;
    }
    for index in 0..3 {
        insert_object(&conn.client, article, &format!("A{index}"), true).await;
    }

    let cheese = Uuid::parse_str("018f2000-0000-7000-8000-0000000000a1").unwrap();
    let organic = Uuid::parse_str("018f2000-0000-7000-8000-0000000000a2").unwrap();
    let typed = Uuid::parse_str("018f2000-0000-7000-8000-0000000000a3").unwrap();
    let missing = Uuid::parse_str("018f2000-0000-7000-8000-0000000000a4").unwrap();
    let cheese_query = serde_json::json!({"type": "article", "group": "food:cheese"});
    let typed_query = serde_json::json!({"type": product.to_string()});
    let organic_query = serde_json::json!({"labels": "organic_eu"});
    let missing_query = serde_json::json!({"labels": "no-such"});
    for (id, name, query) in [
        (cheese, "Cheese", cheese_query),
        (organic, "Organic", organic_query),
        (typed, "Named", typed_query),
        (missing, "Empty", missing_query),
    ] {
        conn.client
            .execute(
                "INSERT INTO assortments (id, owner_id, name, query) VALUES ($1, $2, $3, $4)",
                &[&id, &user, &name, &query],
            )
            .await
            .unwrap();
    }
    let kept = Uuid::now_v7();
    let renamed = Uuid::now_v7();
    conn.client
        .execute(
            "INSERT INTO views (id, workspace_id, type_key, title) VALUES
             ($1, $3, 'assortment_list', 'Assortments'),
             ($2, $3, 'assortment_list', 'My picks')",
            &[&kept, &renamed, &workspace],
        )
        .await
        .unwrap();

    fs::copy(
        source.join("V17__selection_type.sql"),
        dir.join("V17__selection_type.sql"),
    )
    .unwrap();
    migrate(&mut conn.client, &dir).await;

    let row = conn
        .client
        .query_one(
            "SELECT type_id, query FROM assortments WHERE id = $1",
            &[&cheese],
        )
        .await
        .unwrap();
    let type_id: Uuid = row.get(0);
    let query: serde_json::Value = row.get(1);
    assert_eq!(type_id, article);
    assert_eq!(query["group"], "food:cheese");
    assert!(query.get("type").is_none(), "{query}");

    let row = conn
        .client
        .query_one(
            "SELECT type_id, query FROM assortments WHERE id = $1",
            &[&typed],
        )
        .await
        .unwrap();
    let type_id: Uuid = row.get(0);
    let query: serde_json::Value = row.get(1);
    assert_eq!(type_id, product);
    assert_eq!(query, serde_json::json!({}));

    let row = conn
        .client
        .query_one(
            "SELECT type_id, query FROM assortments WHERE id = $1",
            &[&organic],
        )
        .await
        .unwrap();
    let type_id: Uuid = row.get(0);
    let query: serde_json::Value = row.get(1);
    assert_eq!(
        type_id, article,
        "three organic articles beat one organic product"
    );
    assert_eq!(query["labels"], "organic_eu");

    let type_id: Uuid = conn
        .client
        .query_one("SELECT type_id FROM assortments WHERE id = $1", &[&missing])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        type_id, product,
        "no label hits, and products are the larger set"
    );

    let title: String = conn
        .client
        .query_one(
            "SELECT title FROM view_types WHERE key = 'assortment_list'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(title, "Selections");
    let title: String = conn
        .client
        .query_one("SELECT title FROM views WHERE id = $1", &[&kept])
        .await
        .unwrap()
        .get(0);
    assert_eq!(title, "Selections");
    let title: String = conn
        .client
        .query_one("SELECT title FROM views WHERE id = $1", &[&renamed])
        .await
        .unwrap()
        .get(0);
    assert_eq!(title, "My picks");

    fs::copy(
        source.join("V18__ids_and_group_attributes.sql"),
        dir.join("V18__ids_and_group_attributes.sql"),
    )
    .unwrap();
    migrate(&mut conn.client, &dir).await;
    let query: serde_json::Value = conn
        .client
        .query_one("SELECT query FROM assortments WHERE id = $1", &[&cheese])
        .await
        .unwrap()
        .get(0);
    assert!(query.get("type").is_none(), "{query}");
    assert_eq!(
        query["group"],
        "018f0000-0000-7000-8000-000000000300:018f0000-0000-7000-8000-000000000400"
    );
    let reindex: bool = conn
        .client
        .query_one(
            "SELECT reindex_requested FROM indexer_progress WHERE id = 1",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(reindex);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn relations_and_object_refs_become_links() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let source = branch_migrations();
    let dir = std::env::temp_dir().join(format!("braian-v22-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let Some(version) = migration_version(&name) else {
            continue;
        };
        if version <= 22 {
            fs::copy(entry.path(), dir.join(name)).unwrap();
        }
    }
    migrate(&mut conn.client, &dir).await;
    let relations: String = conn
        .client
        .query_one("SELECT to_regclass('public.relations')::text", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(relations, "relations");

    let product = Uuid::parse_str("018f0000-0000-7000-8000-000000000200").unwrap();
    let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap();
    let number = "018f0000-0000-7000-8000-000000000100";
    let product_attr = "018f0000-0000-7000-8000-00000000011a";
    let principal = Uuid::now_v7();
    let batch = Uuid::now_v7();
    conn.client
        .execute(
            "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
            &[&principal],
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
    let definitions = serde_json::json!({
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
                "attributes": [number],
                "structures": []
            },
            {
                "id": article,
                "key": "article",
                "labels": {"en": "Article"},
                "key_attribute": number,
                "attributes": [number, product_attr],
                "structures": []
            }
        ],
        "structures": []
    });
    conn.client
        .execute(
            "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES (1, $1, $2)",
            &[&definitions, &batch],
        )
        .await
        .unwrap();
    let product_id = Uuid::now_v7();
    let article_id = Uuid::now_v7();
    for (id, type_id, key) in [(product_id, product, "P1"), (article_id, article, "A1")] {
        conn.client
            .execute(
                "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, 1)",
                &[&id, &type_id, &key],
            )
            .await
            .unwrap();
    }
    let values = serde_json::json!({ product_attr: product_id.to_string() });
    conn.client
        .execute(
            "INSERT INTO object_values (object_id, part, \"values\", version, is_latest) VALUES ($1, '', $2, 1, true)",
            &[&article_id, &values],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO relations (from_id, to_id, kind) VALUES ($1, $2, 'article_of'), ($1, $1, 'related')",
            &[&article_id, &product_id],
        )
        .await
        .unwrap();

    migrate(&mut conn.client, &source).await;
    let gone: Option<String> = conn
        .client
        .query_one("SELECT to_regclass('public.relations')::text", &[])
        .await
        .unwrap()
        .get(0);
    assert!(gone.is_none(), "{gone:?}");
    let relation_links: i64 = conn
        .client
        .query_one(
            "SELECT count(*) FROM links WHERE from_id = $1 AND to_id = $2",
            &[&article_id, &product_id],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        relation_links, 1,
        "the article_of row is a link and the self-link is dropped"
    );
    let objects_before: i64 = conn
        .client
        .query_one("SELECT count(*) FROM objects WHERE key IS NOT NULL", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(objects_before, 2);

    drop(conn);
    let store =
        braian_store::Store::connect_config(db.config(), braian_store::StorageForm::Document)
            .await
            .unwrap();
    assert!(store.upgrade_object_refs().await.unwrap());
    assert!(!store.upgrade_object_refs().await.unwrap());
    let conn = db.connect().await;
    let links: i64 = conn
        .client
        .query_one("SELECT count(*) FROM links", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(links, 2);
    let still: i64 = conn
        .client
        .query_one(
            "SELECT count(*) FROM objects WHERE id = $1 OR id = $2",
            &[&product_id, &article_id],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(still, 2);
    let value: serde_json::Value = conn
        .client
        .query_one(
            "SELECT \"values\" FROM object_values WHERE object_id = $1 AND is_latest",
            &[&article_id],
        )
        .await
        .unwrap()
        .get(0);
    assert!(value.get(product_attr).is_none(), "{value}");
    let document: serde_json::Value = conn
        .client
        .query_one(
            "SELECT definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let encoded = document.to_string();
    assert!(!encoded.contains("object_ref"), "{document}");
    assert!(encoded.contains("product_articles"), "{document}");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_taken_relation_key_becomes_a_link_that_can_be_removed() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let source = branch_migrations();
    let dir = std::env::temp_dir().join(format!("braian-v22-key-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let Some(version) = migration_version(&name) else {
            continue;
        };
        if version <= 22 {
            fs::copy(entry.path(), dir.join(name)).unwrap();
        }
    }
    migrate(&mut conn.client, &dir).await;

    let article = Uuid::parse_str("018f0000-0000-7000-8000-000000000201").unwrap();
    let related_entity = Uuid::parse_str("018f0000-0000-7000-8000-0000000002c0").unwrap();
    let number = "018f0000-0000-7000-8000-000000000100";
    let principal = Uuid::now_v7();
    let batch = Uuid::now_v7();
    conn.client
        .execute(
            "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
            &[&principal],
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
    let definitions = serde_json::json!({
        "model_revision": 1,
        "languages": ["en"],
        "unit_families": [],
        "attributes": [{
            "id": number,
            "key": "number",
            "labels": {"en": "Number"},
            "language_dependent": false,
            "multiple": false,
            "kind": {"type": "text", "max_length": 64},
            "index": {"searchable": true, "filterable": true, "sortable": true}
        }],
        "types": [
            {
                "id": article,
                "key": "article",
                "labels": {"en": "Article"},
                "key_attribute": number,
                "attributes": [number],
                "structures": []
            },
            {
                "id": related_entity,
                "key": "related",
                "labels": {"en": "Related item"},
                "key_attribute": number,
                "attributes": [number],
                "structures": []
            }
        ],
        "structures": []
    });
    conn.client
        .execute(
            "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES (1, $1, $2)",
            &[&definitions, &batch],
        )
        .await
        .unwrap();
    let left = Uuid::now_v7();
    let right = Uuid::now_v7();
    for (id, key) in [(left, "A1"), (right, "A2")] {
        conn.client
            .execute(
                "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, 1)",
                &[&id, &article, &key],
            )
            .await
            .unwrap();
    }
    conn.client
        .execute(
            "INSERT INTO relations (from_id, to_id, kind) VALUES ($1, $2, 'related'), ($1, $2, 'article_of')",
            &[&left, &right],
        )
        .await
        .unwrap();

    migrate(&mut conn.client, &source).await;

    let related_link_type = Uuid::parse_str("018f0000-0000-7000-8000-0000000002b3").unwrap();
    let article_of_type = Uuid::parse_str("018f0000-0000-7000-8000-0000000002b1").unwrap();
    let document: serde_json::Value = conn
        .client
        .query_one(
            "SELECT definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let types = document["types"].as_array().unwrap();
    let entity = types
        .iter()
        .find(|item| item["key"] == "related")
        .expect("the ordinary type keeps its key");
    assert_ne!(entity["kind"], "link");
    assert_eq!(entity["id"], related_entity.to_string());
    let link_type = types
        .iter()
        .find(|item| item["key"] == "related_link")
        .expect("the relation becomes related_link");
    assert_eq!(link_type["kind"], "link");
    assert_eq!(link_type["id"], related_link_type.to_string());

    let related_link: Uuid = conn
        .client
        .query_one(
            "SELECT object_id FROM links WHERE link_type_id = $1",
            &[&related_link_type],
        )
        .await
        .unwrap()
        .get(0);
    let article_of: Uuid = conn
        .client
        .query_one(
            "SELECT object_id FROM links WHERE link_type_id = $1",
            &[&article_of_type],
        )
        .await
        .unwrap()
        .get(0);
    assert_ne!(related_link, related_entity);
    let created: serde_json::Value = conn
        .client
        .query_one(
            "SELECT diff FROM changes WHERE object_id = $1 AND op = 'create'",
            &[&related_link],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(created["$created"]["version"], 1, "{created}");
    let value_rows: i64 = conn
        .client
        .query_one(
            "SELECT count(*) FROM object_values WHERE object_id = $1",
            &[&related_link],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(value_rows, 0, "an empty link stores no value version");

    drop(conn);
    let store =
        braian_store::Store::connect_config(db.config(), braian_store::StorageForm::Document)
            .await
            .unwrap();
    let link_id = braian_model::ObjectId::from_uuid(related_link);
    let opened = store.get_object(link_id).await.unwrap().expect("detail");
    assert_eq!(opened.version, 1);
    assert!(opened.object.values.is_empty());
    let history = store.stored_changes(link_id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].op, braian_store::ChangeOp::Create);
    let actor = braian_store::Actor {
        actor: braian_store::PrincipalId::from_uuid(principal),
        on_behalf_of: None,
        kind: braian_store::BatchKind::Interactive,
        source: "test".into(),
    };
    store
        .write_batch(
            &actor,
            vec![braian_store::ObjectOp::Delete {
                id: link_id,
                base_version: Some(1),
            }],
        )
        .await
        .unwrap();
    let after = store.stored_changes(link_id).await.unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[1].op, braian_store::ChangeOp::Delete);
    assert!(
        store
            .links_for(braian_model::ObjectId::from_uuid(left))
            .await
            .unwrap()
            .iter()
            .all(|link| link.id != link_id)
    );

    store
        .write_batch(
            &actor,
            vec![braian_store::ObjectOp::Delete {
                id: braian_model::ObjectId::from_uuid(left),
                base_version: Some(1),
            }],
        )
        .await
        .unwrap();
    let cascaded = store
        .get_object(braian_model::ObjectId::from_uuid(article_of))
        .await
        .unwrap()
        .unwrap();
    assert!(cascaded.deleted_at.is_some());
    let remaining: i64 = {
        let conn = db.connect().await;
        let count: i64 = conn
            .client
            .query_one("SELECT count(*) FROM links", &[])
            .await
            .unwrap()
            .get(0);
        count
    };
    assert_eq!(remaining, 0);
    let cascade_history = store
        .stored_changes(braian_model::ObjectId::from_uuid(article_of))
        .await
        .unwrap();
    assert_eq!(cascade_history[0].op, braian_store::ChangeOp::Create);
    assert_eq!(
        cascade_history.last().unwrap().op,
        braian_store::ChangeOp::Delete
    );
    let _ = fs::remove_dir_all(&dir);
}

async fn insert_object(client: &tokio_postgres::Client, type_id: Uuid, key: &str, organic: bool) {
    let id = Uuid::now_v7();
    client
        .execute(
            "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, 1)",
            &[&id, &type_id, &key],
        )
        .await
        .unwrap();
    let values = if organic {
        serde_json::json!({"labels": ["organic_eu"]})
    } else {
        serde_json::json!({})
    };
    client
        .execute(
            "INSERT INTO object_values (object_id, part, \"values\", version, is_latest) VALUES ($1, 'main', $2, 1, true)",
            &[&id, &values],
        )
        .await
        .unwrap();
}
