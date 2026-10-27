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
    let system_kind: String = conn
        .client
        .query_one("SELECT kind FROM principals WHERE handle = 'system'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        system_kind, "integration",
        "the migration principal must not be a person"
    );
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

/// Saved filters, sorts and import targets from every shape a V23 database can
/// hold become attribute paths. Column settings stay. An unknown token stays.
#[tokio::test]
async fn attribute_paths_rewrite_saved_shapes_and_keep_unknown_tokens() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let source = branch_migrations();
    let dir = std::env::temp_dir().join(format!("braian-v23-paths-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let Some(version) = migration_version(&name) else {
            continue;
        };
        if version <= 23 {
            fs::copy(entry.path(), dir.join(name)).unwrap();
        }
    }
    migrate(&mut conn.client, &dir).await;

    let name_id = "018f0000-0000-7000-8000-000000000101";
    let name_en_id = "018f0000-0000-7000-8000-0000000001e1";
    let weight_id = "018f0000-0000-7000-8000-00000000011c";
    let size_id = "018f0000-0000-7000-8000-000000000140";
    let size_min_id = "018f0000-0000-7000-8000-0000000001c1";
    let brand_id = "018f0000-0000-7000-8000-000000000142";
    let article_id = "018f0000-0000-7000-8000-000000000201";
    let product_id = "018f0000-0000-7000-8000-000000000200";
    let link_id = "018f0000-0000-7000-8000-000000000210";
    let food_id = "018f0000-0000-7000-8000-000000000300";
    let cheese_id = "018f0000-0000-7000-8000-000000000400";
    let missing_id = "018f0000-0000-7000-8000-000000000099";
    let name_us = name_id.replace('-', "_");
    let weight_us = weight_id.replace('-', "_");
    let size_us = size_id.replace('-', "_");
    let missing_us = missing_id.replace('-', "_");

    let definitions = serde_json::json!({
        "model_revision": 1,
        "languages": ["en", "sv", "en-GB"],
        "attributes": [
            {"id": name_id, "key": "name"},
            {"id": name_en_id, "key": "name__en"},
            {"id": weight_id, "key": "net_weight"},
            {"id": size_id, "key": "size"},
            {"id": size_min_id, "key": "size__min"},
            {"id": brand_id, "key": "brand"}
        ],
        "types": [
            {"id": article_id, "key": "article"},
            {"id": product_id, "key": "product"},
            {"id": link_id, "key": "articles"}
        ],
        "structures": [{
            "id": food_id,
            "key": "food",
            "groups": [{"id": cheese_id, "key": "cheese"}]
        }]
    });
    let principal = Uuid::now_v7();
    let user = Uuid::now_v7();
    let batch = Uuid::now_v7();
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
             VALUES ($1, $2, 'ada-paths@example.com', 'x', 'admin', 'Ada')",
            &[&user, &principal],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'Legacy')",
            &[&workspace, &user],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES (1, $1, $2)",
            &[&definitions, &batch],
        )
        .await
        .unwrap();

    let filter_in = format!(
        "net_weight:=500 && net_weight=500 && a_{weight_us}:>1 && a_{name_us}__sv:=Ost && a_{name_us}__en_GB:=Cheese && a_{weight_us}__unit:=g && a_{size_us}__min:>=1 && a_{size_us}__max:<=9 && a_{name_us}__sv__unit:=g && size__min:=4 && name__en:=Ada && name__en__sv:=Hej && groups:=food:cheese && type_key:=article && product>brand:=Meadow && articles>link.sort_order:=1 && retired_attr:=x && a_{missing_us}:=z && a_{missing_us}__sv:=q && a_{missing_us}__unit:=g && groups:=food:nope && type_key:=no_such"
    );
    let filter_out = format!(
        "{weight_id}:=500 && {weight_id}:=500 && {weight_id}:>1 && {name_id}@sv:=Ost && {name_id}@en-GB:=Cheese && {weight_id}.unit:=g && {size_id}.min:>=1 && {size_id}.max:<=9 && {name_id}.unit@sv:=g && {size_min_id}:=4 && {name_en_id}:=Ada && {name_en_id}@sv:=Hej && groups:={food_id}:{cheese_id} && type_key:={article_id} && product>brand:=Meadow && articles>link.sort_order:=1 && retired_attr:=x && a_{missing_us}:=z && a_{missing_us}__sv:=q && a_{missing_us}__unit:=g && groups:=food:nope && type_key:=no_such"
    );
    let columns = serde_json::json!([
        "net_weight",
        format!("a_{weight_us}"),
        format!("a_{name_us}__sv"),
        format!("a_{name_us}__en_GB"),
        format!("a_{weight_us}__unit"),
        format!("a_{size_us}__min"),
        format!("a_{size_us}__max"),
        "name__en",
        "size__min",
        "product>brand",
        "articles>link.sort_order",
        "retired_attr",
        format!("a_{missing_us}"),
        format!("a_{missing_us}__sv"),
        "key",
        "name",
        "type",
        "status",
        "version"
    ]);
    let order = serde_json::json!([
        "name__en",
        "size__min",
        format!("a_{size_us}__min"),
        "product>brand",
        "retired_attr",
        "key"
    ]);
    let sizes = serde_json::json!({"name__en": 120, "size__min": 80, "retired_attr": 40});
    let hidden = serde_json::json!(["retired_attr", "name__en", "product>brand"]);
    let column_state = serde_json::json!({
        "columns": columns,
        "table": {"order": order, "sizes": sizes, "hidden": hidden}
    });

    let cases = [
        (
            "018f2000-0000-7000-8000-00000000f101",
            serde_json::json!({"filter": filter_in, "table": {"filter": filter_in}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f102",
            serde_json::json!({"sort": "net_weight:asc", "table": {"sort": format!("a_{name_us}__sv:desc")}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f103",
            serde_json::json!({"sort": format!("a_{name_us}__en_GB:asc"), "table": {"sort": format!("a_{weight_us}__unit:asc")}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f104",
            serde_json::json!({"sort": format!("a_{size_us}__min:desc"), "table": {"sort": format!("a_{size_us}__max:asc")}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f105",
            serde_json::json!({"sort": "size__min:asc", "table": {"sort": "name__en:desc", "groupBy": "size__min"}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f106",
            serde_json::json!({"sort": "product>brand:asc", "table": {"sort": "retired_attr:desc", "groupBy": format!("a_{missing_us}__sv")}}),
        ),
        (
            "018f2000-0000-7000-8000-00000000f107",
            serde_json::json!({"sort": "key:asc", "type": "article", "type_key": "product", "table": {"groupBy": "name__en"}}),
        ),
        ("018f2000-0000-7000-8000-00000000f108", column_state.clone()),
    ];
    for (id, state) in &cases {
        let id = Uuid::parse_str(id).unwrap();
        conn.client
            .execute(
                "INSERT INTO views (id, workspace_id, type_key, title, state)
                 VALUES ($1, $2, 'object_table', 'Legacy', $3)",
                &[&id, &workspace, state],
            )
            .await
            .unwrap();
    }

    let mapping = serde_json::json!({
        "type": "article",
        "columns": [
            {"col": 0, "target": "type:net_weight"},
            {"col": 1, "target": "type:net_weight.unit"},
            {"col": 2, "target": "type:net_weight.value"},
            {"col": 3, "target": "type:name@sv"},
            {"col": 4, "target": format!("type:{weight_us}")},
            {"col": 5, "target": "type:name__en"},
            {"col": 6, "target": "type:size__min"},
            {"col": 7, "target": "type:size__min.unit"},
            {"col": 8, "target": "type:size.min"},
            {"col": 9, "target": "net_weight"},
            {"col": 10, "target": "net_weight.unit"},
            {"col": 11, "target": "name__en"},
            {"col": 12, "target": "name__en@sv"},
            {"col": 13, "target": "name@en-GB"},
            {"col": 14, "target": "product>brand"},
            {"col": 15, "target": "articles>link.sort_order"},
            {"col": 16, "target": "type:retired_attr"},
            {"col": 17, "target": "retired_attr"},
            {"col": 18, "target": format!("a_{missing_us}")},
            {"col": 19, "target": format!("a_{missing_us}__unit")},
            {"col": 20, "target": "link:articles:in"},
            {"col": 21, "target": "link:articles:out"},
            {"col": 22, "target": "link:no_such:in"},
            {"col": 23, "target": "membership:food"},
            {"col": 24, "target": "membership:no_such"},
            {"col": 25, "target": "group:food/cheese:brand"},
            {"col": 26, "target": "group:food/cheese:brand.unit"},
            {"col": 27, "target": "group:food/cheese:name__en"},
            {"col": 28, "target": "group:food/cheese:retired_attr"},
            {"col": 29, "target": "group:nope/missing:brand"},
            {"col": 30, "target": "group:food/cheese:brand@sv"},
            {"col": 31, "target": "unmapped"},
            {"col": 32, "target": "action"},
            {"col": 33, "target": ""}
        ]
    });
    let diff_mapping = serde_json::json!({
        "type": "product",
        "columns": [
            {"col": 0, "target": "type:size__min"},
            {"col": 1, "target": "retired_attr"}
        ]
    });
    let import_id = Uuid::parse_str("018f2000-0000-7000-8000-00000000f110").unwrap();
    conn.client
        .execute(
            "INSERT INTO import_sessions (id, owner, file_name, file_kind, status, model_revision, mapping, diff_mapping)
             VALUES ($1, $2, 'old.csv', 'csv', 'ready', 1, $3, $4)",
            &[&import_id, &principal, &mapping, &diff_mapping],
        )
        .await
        .unwrap();
    let assortment = Uuid::parse_str("018f2000-0000-7000-8000-00000000f111").unwrap();
    let query = serde_json::json!({
        "type": "article",
        "group": "food:cheese",
        "filter": format!("a_{weight_us}__unit:=g && name__en:=Ada && size__min:=1 && groups:=food:cheese && type_key:=article && product>brand:=X && retired_attr:=1 && groups:=nope:missing")
    });
    conn.client
        .execute(
            "INSERT INTO assortments (id, owner_id, name, query, type_id) VALUES ($1, $2, 'Legacy', $3, $4)",
            &[
                &assortment,
                &user,
                &query,
                &Uuid::parse_str(article_id).unwrap(),
            ],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "UPDATE indexer_progress SET reindex_requested = false WHERE id = 1",
            &[],
        )
        .await
        .unwrap();

    fs::copy(
        source.join("V24__attribute_paths.sql"),
        dir.join("V24__attribute_paths.sql"),
    )
    .unwrap();
    migrate(&mut conn.client, &dir).await;

    async fn view_state(client: &tokio_postgres::Client, id: &str) -> serde_json::Value {
        let id = Uuid::parse_str(id).unwrap();
        client
            .query_one("SELECT state FROM views WHERE id = $1", &[&id])
            .await
            .unwrap()
            .get("state")
    }

    let filters = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f101").await;
    assert_eq!(filters["filter"], filter_out);
    assert_eq!(filters["table"]["filter"], filter_out);

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f102").await;
    assert_eq!(sorts["sort"], format!("{weight_id}:asc"));
    assert_eq!(sorts["table"]["sort"], format!("{name_id}@sv:desc"));

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f103").await;
    assert_eq!(sorts["sort"], format!("{name_id}@en-GB:asc"));
    assert_eq!(sorts["table"]["sort"], format!("{weight_id}.unit:asc"));

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f104").await;
    assert_eq!(sorts["sort"], format!("{size_id}.min:desc"));
    assert_eq!(sorts["table"]["sort"], format!("{size_id}.max:asc"));

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f105").await;
    assert_eq!(sorts["sort"], format!("{size_min_id}:asc"));
    assert_eq!(sorts["table"]["sort"], format!("{name_en_id}:desc"));
    assert_eq!(sorts["table"]["groupBy"], size_min_id);

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f106").await;
    assert_eq!(sorts["sort"], "product>brand:asc");
    assert_eq!(sorts["table"]["sort"], "retired_attr:desc");
    assert_eq!(sorts["table"]["groupBy"], format!("a_{missing_us}__sv"));

    let sorts = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f107").await;
    assert_eq!(sorts["sort"], "key:asc");
    assert_eq!(sorts["type"], article_id);
    assert_eq!(sorts["type_key"], product_id);
    assert_eq!(sorts["table"]["groupBy"], name_en_id);

    let stored_columns = view_state(&conn.client, "018f2000-0000-7000-8000-00000000f108").await;
    assert_eq!(stored_columns, column_state);

    let stored_mapping: serde_json::Value = conn
        .client
        .query_one(
            "SELECT mapping FROM import_sessions WHERE id = $1",
            &[&import_id],
        )
        .await
        .unwrap()
        .get("mapping");
    let targets: Vec<&str> = stored_mapping["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["target"].as_str().unwrap())
        .collect();
    let expected_targets = [
        weight_id.to_owned(),
        format!("{weight_id}.unit"),
        format!("{weight_id}.value"),
        format!("{name_id}@sv"),
        weight_id.to_owned(),
        name_en_id.to_owned(),
        size_min_id.to_owned(),
        format!("{size_min_id}.unit"),
        format!("{size_id}.min"),
        weight_id.to_owned(),
        format!("{weight_id}.unit"),
        name_en_id.to_owned(),
        format!("{name_en_id}@sv"),
        format!("{name_id}@en-GB"),
        "product>brand".to_owned(),
        "articles>link.sort_order".to_owned(),
        "type:retired_attr".to_owned(),
        "retired_attr".to_owned(),
        format!("a_{missing_us}"),
        format!("a_{missing_us}__unit"),
        format!("link:{link_id}:in"),
        format!("link:{link_id}:out"),
        "link:no_such:in".to_owned(),
        format!("membership:{food_id}"),
        "membership:no_such".to_owned(),
        format!("group:{food_id}/{cheese_id}:{brand_id}"),
        format!("group:{food_id}/{cheese_id}:{brand_id}.unit"),
        format!("group:{food_id}/{cheese_id}:{name_en_id}"),
        format!("group:{food_id}/{cheese_id}:retired_attr"),
        format!("group:nope/missing:{brand_id}"),
        format!("group:{food_id}/{cheese_id}:{brand_id}@sv"),
        "unmapped".to_owned(),
        "action".to_owned(),
        String::new(),
    ];
    assert_eq!(
        targets,
        expected_targets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(stored_mapping["type"], article_id);
    let stored_diff: serde_json::Value = conn
        .client
        .query_one(
            "SELECT diff_mapping FROM import_sessions WHERE id = $1",
            &[&import_id],
        )
        .await
        .unwrap()
        .get("diff_mapping");
    assert_eq!(stored_diff["type"], product_id);
    assert_eq!(stored_diff["columns"][0]["target"], size_min_id);
    assert_eq!(stored_diff["columns"][1]["target"], "retired_attr");

    let stored_query: serde_json::Value = conn
        .client
        .query_one(
            "SELECT query FROM assortments WHERE id = $1",
            &[&assortment],
        )
        .await
        .unwrap()
        .get("query");
    assert!(stored_query.get("type").is_none(), "{stored_query}");
    assert_eq!(stored_query["group"], format!("{food_id}:{cheese_id}"));
    assert_eq!(
        stored_query["filter"],
        format!(
            "{weight_id}.unit:=g && {name_en_id}:=Ada && {size_min_id}:=1 && groups:={food_id}:{cheese_id} && type_key:={article_id} && product>brand:=X && retired_attr:=1 && groups:=nope:missing"
        )
    );

    let views: i64 = conn
        .client
        .query_one("SELECT count(*) FROM views", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(views, cases.len() as i64);
    let imports: i64 = conn
        .client
        .query_one("SELECT count(*) FROM import_sessions", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(imports, 1);
    let requested: bool = conn
        .client
        .query_one(
            "SELECT reindex_requested FROM indexer_progress WHERE id = 1",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(requested);

    let _ = fs::remove_dir_all(&dir);
}

/// V25 adds the claim columns. A proposal stored before that migration still
/// opens, and two startups (migrate, then clear only expired leases) leave it
/// claimable. The demo seed inserts proposals through the same `create_proposal`
/// path; that path has to work after the upgrade.
#[tokio::test]
async fn an_old_proposal_survives_the_claim_columns_and_two_startups() {
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let source = branch_migrations();
    let dir = std::env::temp_dir().join(format!("braian-v25-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if migration_version(&name).is_some_and(|version| version < 25) {
            fs::copy(entry.path(), dir.join(name)).unwrap();
        }
    }
    migrate(&mut conn.client, &dir).await;

    let actor = Uuid::now_v7();
    let proposal_id = Uuid::now_v7();
    let row_id = Uuid::now_v7();
    conn.client
        .execute(
            "INSERT INTO principals (id, kind, name) VALUES ($1, 'human', 'Ada')",
            &[&actor],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO proposals (id, title, source, status, actor, conflict)
             VALUES ($1, 'Before the lease', 'user', 'open', $2, false)",
            &[&proposal_id, &actor],
        )
        .await
        .unwrap();
    conn.client
        .execute(
            "INSERT INTO proposal_rows (id, proposal_id, object_key, type_key, patch, status, position)
             VALUES ($1, $2, 'SKU-OLD', 'item', '{\"values\":{\"sku\":\"SKU-OLD\"}}', 'pending', 0)",
            &[&row_id, &proposal_id],
        )
        .await
        .unwrap();

    migrate(&mut conn.client, &source).await;
    let claimed_by: Option<Uuid> = conn
        .client
        .query_one(
            "SELECT claimed_by FROM proposals WHERE id = $1",
            &[&proposal_id],
        )
        .await
        .unwrap()
        .get(0);
    assert!(claimed_by.is_none());
    let stale = Uuid::now_v7();
    conn.client
        .execute(
            "UPDATE proposals
             SET claimed_by = $2, claimed_until = now() - interval '1 minute'
             WHERE id = $1",
            &[&proposal_id, &stale],
        )
        .await
        .unwrap();
    drop(conn);

    let first = braian_store::Store::connect_config(db.config(), braian_store::STORAGE_FORM)
        .await
        .unwrap();
    drop(first);
    let second = braian_store::Store::connect_config(db.config(), braian_store::STORAGE_FORM)
        .await
        .unwrap();
    let stored = second.proposal(proposal_id).await.unwrap().unwrap();
    assert_eq!(stored["status"], "open", "{stored}");
    assert_eq!(stored["rows"][0]["status"], "pending", "{stored}");
    let lock = second.claim_proposal(proposal_id).await.unwrap();
    let busy = second.claim_proposal(proposal_id).await.unwrap_err();
    assert!(
        matches!(busy, braian_store::Error::ProposalBusy(_)),
        "{busy}"
    );
    lock.release().await.unwrap();

    let created = Uuid::now_v7();
    second
        .create_proposal(
            created,
            "After the upgrade",
            "user",
            braian_store::PrincipalId::from_uuid(actor),
            None,
            false,
            &[braian_store::ProposalRow {
                id: Uuid::now_v7(),
                object_id: None,
                object_key: Some("SKU-NEW".into()),
                type_key: Some("item".into()),
                base_version: None,
                patch: serde_json::json!({ "values": { "sku": "SKU-NEW" } }),
                status: "pending".into(),
                conflict: false,
                position: 0,
            }],
        )
        .await
        .unwrap();
    second
        .create_proposal(
            Uuid::now_v7(),
            "After the upgrade, again",
            "agent",
            braian_store::PrincipalId::from_uuid(actor),
            None,
            false,
            &[],
        )
        .await
        .unwrap();
    let again = second.proposal(created).await.unwrap().unwrap();
    assert_eq!(again["title"], "After the upgrade");
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
