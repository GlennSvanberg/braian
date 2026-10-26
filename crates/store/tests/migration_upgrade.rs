//! A database that already applied main's migrations must accept this branch.
//!
//! refinery 0.8 aborts when an applied migration's checksum changes. CI points
//! `BRAIAN_BASELINE_MIGRATIONS` at main's `crates/store/migrations`, applies
//! those, then applies the migrations in this checkout.

use std::env;
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
        let mut config = admin::parse_config(&self.admin_url).unwrap();
        config.dbname(&self.name);
        PgConn::connect(config).await.unwrap()
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
        return path;
    }
    panic!(
        "BRAIAN_BASELINE_MIGRATIONS is not set. Point it at main's crates/store/migrations so this test applies main before the branch."
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

    let baseline_stops_before_v15 = !baseline.join("V15__change_log.sql").is_file();
    let latest_branch = branch_set
        .iter()
        .map(|migration| migration.version())
        .max()
        .expect("branch migrations");
    let db = EmptyDb::create().await;
    let mut conn = db.connect().await;
    let level = wal_level(&conn.client).await;
    assert_ne!(level, "logical");
    migrate(&mut conn.client, &baseline).await;
    if baseline_stops_before_v15 {
        assert!(
            publication_exists(&conn.client).await,
            "main's V3 creates braian_changes even when wal_level is {level}"
        );
    }
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
    assert_eq!(u32::try_from(latest).unwrap(), latest_branch);
}
