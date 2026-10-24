use std::sync::Arc;

use deadpool_postgres::{Manager, Pool};
use tokio::sync::RwLock;
use tokio_postgres::NoTls;

use crate::error::Error;
use crate::split::StorageForm;
use crate::types::CachedDefs;

use refinery::embed_migrations;
embed_migrations!("migrations");

#[derive(Clone)]
pub struct Store {
    pool: Pool,
    form: StorageForm,
    definitions: Arc<RwLock<Option<Arc<CachedDefs>>>>,
}

impl Store {
    pub async fn connect(database_url: &str) -> Result<Self, Error> {
        Self::connect_form(database_url, crate::STORAGE_FORM).await
    }

    pub async fn connect_form(database_url: &str, form: StorageForm) -> Result<Self, Error> {
        let config = parse_config(database_url)?;
        Self::connect_config(config, form).await
    }

    pub async fn connect_config(
        config: tokio_postgres::Config,
        form: StorageForm,
    ) -> Result<Self, Error> {
        let migrate_with = config.clone();
        let (mut client, connection) = migrate_with.connect(NoTls).await?;
        let handle = tokio::spawn(async move {
            let _ = connection.await;
        });
        migrations::runner().run_async(&mut client).await?;
        client
            .batch_execute("SELECT ensure_change_partitions(3)")
            .await?;
        crate::ops::backfill_attribute_parts(&mut client).await?;
        drop(client);
        handle.abort();

        let manager = Manager::new(config, NoTls);
        let pool = Pool::builder(manager).max_size(32).build()?;
        Ok(Self {
            pool,
            form,
            definitions: Arc::new(RwLock::new(None)),
        })
    }

    pub fn form(&self) -> StorageForm {
        self.form
    }

    pub(crate) async fn defs(&self) -> Result<Arc<CachedDefs>, Error> {
        if let Some(cached) = self.definitions.read().await.clone() {
            return Ok(cached);
        }
        self.refresh_defs().await
    }

    pub(crate) async fn refresh_defs(&self) -> Result<Arc<CachedDefs>, Error> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1",
                &[],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::NoDefinitions);
        };
        let revision: i64 = row.get(0);
        let document: serde_json::Value = row.get(1);
        let set: openpim_model::DefinitionSet = serde_json::from_value(document)
            .map_err(|error| Error::Definitions(error.to_string()))?;
        if i64::try_from(set.model_revision).ok() != Some(revision) {
            return Err(Error::Definitions(
                "stored revision does not match the document".into(),
            ));
        }
        let group_rows = client
            .query("SELECT attribute_id, part_group FROM attribute_parts", &[])
            .await?;
        let mut groups = std::collections::HashMap::new();
        for row in group_rows {
            let group: i32 = row.get("part_group");
            groups.insert(
                openpim_model::AttributeId::from_uuid(row.get("attribute_id")),
                usize::try_from(group).unwrap_or(0),
            );
        }
        let groups = crate::split::SplitContext::assign_groups(&set, &groups);
        let cached = Arc::new(CachedDefs::from_groups(set, groups));
        *self.definitions.write().await = Some(Arc::clone(&cached));
        Ok(cached)
    }

    pub(crate) async fn remember(&self, cached: Arc<CachedDefs>) {
        *self.definitions.write().await = Some(cached);
    }

    pub(crate) fn pool(&self) -> &Pool {
        &self.pool
    }
}

pub fn parse_config(database_url: &str) -> Result<tokio_postgres::Config, Error> {
    database_url
        .parse::<tokio_postgres::Config>()
        .map_err(|error| Error::Url(error.to_string()))
}

pub struct PgConn {
    pub client: tokio_postgres::Client,
}

impl PgConn {
    pub async fn connect(config: tokio_postgres::Config) -> Result<Self, Error> {
        let (client, connection) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Self { client })
    }
}

pub async fn create_database(client: &tokio_postgres::Client, name: &str) -> Result<(), Error> {
    let name = crate::ids::plain_name(name)?;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await?;
    Ok(())
}

pub async fn drop_database(client: &tokio_postgres::Client, name: &str) -> Result<(), Error> {
    let name = crate::ids::plain_name(name)?;
    client
        .batch_execute(&format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{name}' AND pid <> pg_backend_pid()"
        ))
        .await?;
    client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name}"))
        .await?;
    Ok(())
}
