use std::time::Duration;

use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{Actor, plain_name};
pub use crate::log::PageGap;
use crate::session::{self, PgConn, Store};
use crate::split::StorageForm;

/// One migrated database per test, cloned from a template built once per process.
pub struct TestDb {
    store: Option<Store>,
    name: String,
    admin_url: String,
}

impl TestDb {
    /// Panics with a setup message when `BRAIAN_TEST_DATABASE_URL` is missing.
    ///
    /// The URL has to reach the maintenance database (`postgres`), because the
    /// test creates and drops its own database.
    pub async fn create() -> Self {
        let admin_url = std::env::var("BRAIAN_TEST_DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "BRAIAN_TEST_DATABASE_URL is not set. Point it at the maintenance database, for example postgres://braian:secret@localhost:5433/postgres"
            )
        });
        Self::try_create(&admin_url)
            .await
            .unwrap_or_else(|error| panic!("could not create the test database: {error}"))
    }

    pub fn store(&self) -> &Store {
        self.store.as_ref().expect("test database is open")
    }

    /// Connection string for this test database, including the role password.
    pub fn database_url(&self) -> String {
        let base = self.admin_url.trim_end_matches('/');
        match base.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{}", self.name),
            None => format!("{base}/{}", self.name),
        }
    }

    async fn try_create(admin_url: &str) -> Result<Self, Error> {
        let template = template_database(admin_url).await?;
        let name = format!("t{:032x}", Uuid::now_v7().as_u128());
        plain_name(&name)?;
        let admin = admin_conn(admin_url).await?;
        let mut created = false;
        let mut last_error = None;
        for _ in 0..10 {
            match admin
                .client
                .batch_execute(&format!("CREATE DATABASE {name} TEMPLATE {template}"))
                .await
            {
                Ok(()) => {
                    created = true;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        drop(admin);
        if !created {
            return Err(Error::Db(last_error.expect("create database error")));
        }
        let mut config = session::parse_config(admin_url)?;
        config.dbname(&name);
        let store = Store::connect_config(config, crate::STORAGE_FORM).await?;
        Ok(Self {
            store: Some(store),
            name,
            admin_url: admin_url.to_owned(),
        })
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        self.store.take();
        let url = self.admin_url.clone();
        let name = self.name.clone();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test cleanup runtime");
            runtime.block_on(async {
                for _ in 0..20 {
                    let Ok(admin) = admin_conn(&url).await else {
                        return;
                    };
                    if session::drop_database(&admin.client, &name).await.is_ok() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        })
        .join();
    }
}

async fn template_database(admin_url: &str) -> Result<&'static str, Error> {
    static TEMPLATE: OnceCell<String> = OnceCell::const_new();
    let name = TEMPLATE
        .get_or_try_init(|| create_template(admin_url.to_owned()))
        .await?;
    Ok(name.as_str())
}

async fn create_template(admin_url: String) -> Result<String, Error> {
    let name = template_name();
    let admin = admin_conn(&admin_url).await?;
    admin
        .client
        .batch_execute("SELECT pg_advisory_lock(84210424)")
        .await?;
    let exists = admin
        .client
        .query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&name])
        .await?;
    if exists.is_none() {
        session::create_database(&admin.client, &name).await?;
        let mut config = session::parse_config(&admin_url)?;
        config.dbname(&name);
        let store = Store::connect_config(config, StorageForm::Document).await?;
        drop(store);
        admin
            .client
            .batch_execute(&format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{name}' AND pid <> pg_backend_pid()"
            ))
            .await?;
    }
    admin
        .client
        .batch_execute("SELECT pg_advisory_unlock(84210424)")
        .await?;
    Ok(name)
}

fn template_name() -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let sqls = [
        include_str!("../migrations/V1__storage.sql"),
        include_str!("../migrations/V2__api.sql"),
        include_str!("../migrations/V3__app.sql"),
        include_str!("../migrations/V4__frecency.sql"),
        include_str!("../migrations/V5__workspace_state.sql"),
        include_str!("../migrations/V6__reindex_request.sql"),
        include_str!("../migrations/V7__value_versions.sql"),
        include_str!("../migrations/V8__current_values.sql"),
        include_str!("../migrations/V9__object_list.sql"),
        include_str!("../migrations/V10__imports.sql"),
        include_str!("../migrations/V11__interactive_pressure.sql"),
        include_str!("../migrations/V12__structure_labels.sql"),
        include_str!("../migrations/V13__assortment_view.sql"),
        include_str!("../migrations/V14__class_attributes_view.sql"),
        include_str!("../migrations/V15__change_log.sql"),
        include_str!("../migrations/V16__english_view_types.sql"),
        include_str!("../migrations/V17__selection_type.sql"),
        include_str!("../migrations/V18__ids_and_group_attributes.sql"),
        include_str!("../migrations/V19__updated_by.sql"),
        include_str!("../migrations/V20__model_drafts.sql"),
        include_str!("../migrations/V21__attribute_definitions_view.sql"),
        include_str!("../migrations/V22__links.sql"),
        include_str!("../migrations/V23__links_replace_relations.sql"),
        include_str!("../migrations/V24__attribute_paths.sql"),
        include_str!("../migrations/V25__proposal_claim.sql"),
        include_str!("../migrations/V26__view_column_prefs.sql"),
        include_str!("../migrations/V27__model_table_view_types.sql"),
        include_str!("../migrations/V28__import_drafts.sql"),
        include_str!("../migrations/V29__import_overrides_and_mappings.sql"),
    ];
    for sql in sqls {
        for byte in sql.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("tpl_{hash:016x}")
}

/// Appends one `changes` row per id, on `actor`'s lane.
pub async fn append_changes(
    store: &Store,
    actor: &Actor,
    object_ids: &[Option<Uuid>],
) -> Result<Vec<i64>, Error> {
    store.append_log(actor, object_ids).await
}

/// Expires open leases so the next claim replays them.
pub async fn expire_claims(store: &Store, lane: &str) -> Result<(), Error> {
    store.expire_claims(lane).await
}

/// Expires one proposal lease so the next claim can take it without waiting.
pub async fn expire_proposal_claim(store: &Store, id: Uuid) -> Result<(), Error> {
    store.expire_proposal_claim(id).await
}

async fn admin_conn(admin_url: &str) -> Result<PgConn, Error> {
    let mut config = session::parse_config(admin_url)?;
    config.dbname("postgres");
    PgConn::connect(config).await
}
