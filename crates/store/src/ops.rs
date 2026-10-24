use std::collections::BTreeSet;
use std::sync::Arc;

use futures::StreamExt;
use openpim_model::{DefinitionSet, Object, ObjectId, TypeId};
use serde_json::{Map, Value};
use tokio_postgres::Transaction;
use tokio_postgres::binary_copy::BinaryCopyInWriter;
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{Actor, BatchId, ChangeOp, PrincipalId, PrincipalKind};
use crate::session::Store;
use crate::split::{self, diff_created, diff_from_patch};
use crate::types::{
    BulkReport, BulkRowError, CachedDefs, ChangeRow, ObjectOp, StoredObject, WriteReceipt,
};

impl Store {
    pub async fn create_principal(
        &self,
        kind: PrincipalKind,
        name: &str,
    ) -> Result<PrincipalId, Error> {
        let id = PrincipalId::new();
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO principals (id, kind, name) VALUES ($1, $2, $3)",
                &[&id.as_uuid(), &kind.as_str(), &name],
            )
            .await?;
        Ok(id)
    }

    pub async fn put_definitions(
        &self,
        actor: &Actor,
        defs: &DefinitionSet,
    ) -> Result<WriteReceipt, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        self.ensure_actor(&tx, actor).await?;
        let current = tx
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1 FOR UPDATE",
                &[],
            )
            .await?;
        let current_revision = current
            .as_ref()
            .map(|row| row.get::<_, i64>(0) as u64)
            .unwrap_or(0);
        if defs.model_revision != current_revision + 1 {
            return Err(Error::BadRevision {
                current: current_revision,
                got: defs.model_revision,
            });
        }
        let batch = insert_batch(&tx, actor).await?;
        let document =
            serde_json::to_value(defs).map_err(|error| Error::Definitions(error.to_string()))?;
        tx.execute(
            "INSERT INTO definition_revisions (model_revision, definitions, batch_id) VALUES ($1, $2, $3)",
            &[&as_i64(defs.model_revision)?, &document, &batch.as_uuid()],
        )
        .await?;
        let mut diff = Map::new();
        if let Some(row) = &current {
            diff.insert("before".to_owned(), row.get("definitions"));
        }
        diff.insert("after".to_owned(), document);
        let seq = insert_change(
            &tx,
            batch,
            None,
            None,
            ChangeOp::Definitions,
            actor,
            &Value::Object(diff),
        )
        .await?;
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_set(defs.clone())))
            .await;
        Ok(WriteReceipt {
            batch_id: batch,
            last_seq: seq,
            versions: Vec::new(),
        })
    }

    pub async fn open_batch(&self, actor: &Actor) -> Result<BatchId, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        self.ensure_actor(&tx, actor).await?;
        let batch = insert_batch(&tx, actor).await?;
        tx.commit().await?;
        Ok(batch)
    }

    pub async fn write_batch(
        &self,
        actor: &Actor,
        ops: Vec<ObjectOp>,
    ) -> Result<WriteReceipt, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        self.ensure_actor(&tx, actor).await?;
        let batch = insert_batch(&tx, actor).await?;
        let defs = self.defs().await?;
        let receipt = apply_ops(&tx, self.form(), &defs, batch, actor, &ops).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// Writes `ops` into a batch opened with [`Store::open_batch`].
    ///
    /// One call is one transaction. A bulk job calls this once per chunk.
    pub async fn write_ops(
        &self,
        batch: BatchId,
        actor: &Actor,
        ops: Vec<ObjectOp>,
    ) -> Result<WriteReceipt, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let defs = self.defs().await?;
        let receipt = apply_ops(&tx, self.form(), &defs, batch, actor, &ops).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn import_chunk(
        &self,
        batch: BatchId,
        actor: &Actor,
        objects: &[Object],
    ) -> Result<(), Error> {
        if objects.is_empty() {
            return Ok(());
        }
        let defs = self.defs().await?;
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        copy_creates(&tx, self.form(), &defs, batch, actor, objects).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn bulk_create(
        &self,
        actor: &Actor,
        mut objects: impl futures::Stream<Item = Object> + Unpin,
        chunk: usize,
    ) -> Result<BulkReport, Error> {
        if chunk == 0 {
            return Err(Error::EmptyChunk);
        }
        let _ = self.defs().await?;
        let batch = self.open_batch(actor).await?;
        let mut written = 0u64;
        let mut errors = Vec::new();
        let mut buffer = Vec::with_capacity(chunk);
        while let Some(object) = objects.next().await {
            buffer.push(object);
            if buffer.len() == chunk {
                self.flush_chunk(batch, actor, &mut buffer, &mut written, &mut errors)
                    .await?;
            }
        }
        if !buffer.is_empty() {
            self.flush_chunk(batch, actor, &mut buffer, &mut written, &mut errors)
                .await?;
        }
        let last_seq = self.max_seq(batch).await?;
        Ok(BulkReport {
            batch_id: batch,
            written,
            last_seq,
            errors,
        })
    }

    pub async fn get_object(&self, id: ObjectId) -> Result<Option<StoredObject>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = object_row(&**client, id).await? else {
            return Ok(None);
        };
        let stored = self.assemble(&**client, row).await?;
        Ok(Some(stored))
    }

    pub async fn get_by_key(
        &self,
        type_id: TypeId,
        key: &str,
    ) -> Result<Option<StoredObject>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, type_id, key, version, model_revision, deleted_at FROM objects WHERE type_id = $1 AND key = $2 AND deleted_at IS NULL",
                &[&type_id.as_uuid(), &key],
            )
            .await?
        else {
            return Ok(None);
        };
        let stored = self.assemble(&**client, row).await?;
        Ok(Some(stored))
    }

    pub async fn changes_for(&self, id: ObjectId) -> Result<Vec<ChangeRow>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT seq, at, batch_id, object_id, version, op, actor, on_behalf_of, diff FROM changes WHERE object_id = $1 ORDER BY seq",
                &[&id.as_uuid()],
            )
            .await?;
        rows.iter().map(change_from_row).collect()
    }

    /// Monthly partition that holds `seq`, for example `changes_2026_10`.
    pub async fn change_partition(&self, seq: i64) -> Result<String, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT tableoid::regclass::text FROM changes WHERE seq = $1",
                &[&seq],
            )
            .await?;
        Ok(row.get(0))
    }

    async fn flush_chunk(
        &self,
        batch: BatchId,
        actor: &Actor,
        buffer: &mut Vec<Object>,
        written: &mut u64,
        errors: &mut Vec<BulkRowError>,
    ) -> Result<(), Error> {
        let chunk = std::mem::take(buffer);
        match self.import_chunk(batch, actor, &chunk).await {
            Ok(()) => {
                *written += chunk.len() as u64;
                Ok(())
            }
            Err(error) if error.is_duplicate() && chunk.len() > 1 => {
                for object in chunk {
                    match self
                        .import_chunk(batch, actor, std::slice::from_ref(&object))
                        .await
                    {
                        Ok(()) => *written += 1,
                        Err(error) if error.is_duplicate() => {
                            errors.push(row_error(&object, &error));
                        }
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            }
            Err(error) if error.is_duplicate() => {
                errors.push(row_error(&chunk[0], &error));
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn max_seq(&self, batch: BatchId) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT COALESCE(MAX(seq), 0) FROM changes WHERE batch_id = $1",
                &[&batch.as_uuid()],
            )
            .await?;
        Ok(row.get(0))
    }

    async fn assemble(
        &self,
        client: &impl tokio_postgres::GenericClient,
        row: tokio_postgres::Row,
    ) -> Result<StoredObject, Error> {
        let id = ObjectId::from_uuid(row.get("id"));
        let groups = client
            .query(
                "SELECT structure_id, group_id FROM object_structures WHERE object_id = $1",
                &[&id.as_uuid()],
            )
            .await?;
        let parts = client
            .query(
                "SELECT part, \"values\" FROM object_values WHERE object_id = $1",
                &[&id.as_uuid()],
            )
            .await?;
        let encoded = parts
            .iter()
            .map(|part| (part.get("part"), part.get("values")))
            .collect();
        let values = split::join_parts(self.form(), &encoded)
            .map_err(|message| Error::Corrupt(id, message))?;
        let version: i64 = row.get("version");
        Ok(StoredObject {
            object: Object {
                id,
                type_id: TypeId::from_uuid(row.get("type_id")),
                model_revision: u64_from_i64(row.get("model_revision"))?,
                groups: groups
                    .iter()
                    .map(|group| {
                        (
                            openpim_model::StructureId::from_uuid(group.get("structure_id")),
                            openpim_model::GroupId::from_uuid(group.get("group_id")),
                        )
                    })
                    .collect(),
                values,
            },
            key: row.get("key"),
            version: u64_from_i64(version)?,
            deleted_at: row.get("deleted_at"),
        })
    }

    async fn ensure_actor(&self, tx: &Transaction<'_>, actor: &Actor) -> Result<(), Error> {
        let row = tx
            .query_opt(
                "SELECT kind FROM principals WHERE id = $1",
                &[&actor.actor.as_uuid()],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::UnknownPrincipal(actor.actor));
        };
        let kind = PrincipalKind::parse(row.get("kind"))?;
        if kind == PrincipalKind::Agent && actor.on_behalf_of.is_none() {
            return Err(Error::AgentRequiresBehalf(actor.actor));
        }
        if let Some(behalf) = actor.on_behalf_of {
            let found = tx
                .query_opt(
                    "SELECT 1 FROM principals WHERE id = $1",
                    &[&behalf.as_uuid()],
                )
                .await?;
            if found.is_none() {
                return Err(Error::UnknownPrincipal(behalf));
            }
        }
        Ok(())
    }
}

impl Error {
    fn is_duplicate(&self) -> bool {
        matches!(self, Self::DuplicateKey { .. })
    }
}

async fn object_row(
    client: &impl tokio_postgres::GenericClient,
    id: ObjectId,
) -> Result<Option<tokio_postgres::Row>, Error> {
    Ok(client
        .query_opt(
            "SELECT id, type_id, key, version, model_revision, deleted_at FROM objects WHERE id = $1",
            &[&id.as_uuid()],
        )
        .await?)
}

async fn apply_ops(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    ops: &[ObjectOp],
) -> Result<WriteReceipt, Error> {
    let mut versions = Vec::with_capacity(ops.len());
    let mut last_seq = 0i64;
    for op in ops {
        let (id, version, seq) = match op {
            ObjectOp::Create(object) => {
                let (version, seq) = insert_object(tx, form, defs, batch, actor, object).await?;
                (object.id, version, seq)
            }
            ObjectOp::Update { id, set } => {
                let (version, seq) = update_object(tx, form, defs, batch, actor, *id, set).await?;
                (*id, version, seq)
            }
            ObjectOp::Delete { id } => {
                let (version, seq) = delete_object(tx, batch, actor, *id).await?;
                (*id, version, seq)
            }
        };
        versions.push((id, version));
        last_seq = seq;
    }
    Ok(WriteReceipt {
        batch_id: batch,
        last_seq,
        versions,
    })
}

async fn insert_object(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    object: &Object,
) -> Result<(u64, i64), Error> {
    let key = defs.key_of(object)?;
    let revision = as_i64(object.model_revision)?;
    tx.execute(
        "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES ($1, $2, $3, 1, $4)",
        &[
            &object.id.as_uuid(),
            &object.type_id.as_uuid(),
            &key,
            &revision,
        ],
    )
    .await
    .map_err(|error| map_unique(error, object.type_id, &key))?;
    let parts = defs.split.split(form, &object.values);
    for (part, document) in &parts {
        let value = Value::Object(document.clone());
        tx.execute(
            "INSERT INTO object_values (object_id, part, \"values\") VALUES ($1, $2, $3)",
            &[&object.id.as_uuid(), part, &value],
        )
        .await?;
    }
    for (structure, group) in &object.groups {
        tx.execute(
            "INSERT INTO object_structures (object_id, structure_id, group_id) VALUES ($1, $2, $3)",
            &[&object.id.as_uuid(), &structure.as_uuid(), &group.as_uuid()],
        )
        .await?;
    }
    let diff = diff_created(&object.values);
    let seq = insert_change(
        tx,
        batch,
        Some(object.id),
        Some(1),
        ChangeOp::Create,
        actor,
        &diff,
    )
    .await?;
    Ok((1, seq))
}

async fn update_object(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    id: ObjectId,
    patch: &openpim_model::ValuePatch,
) -> Result<(u64, i64), Error> {
    let row = tx
        .query_opt(
            "SELECT version, deleted_at FROM objects WHERE id = $1 FOR UPDATE",
            &[&id.as_uuid()],
        )
        .await?;
    let Some(row) = row else {
        return Err(Error::NotFound(id));
    };
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row.get("deleted_at");
    if deleted_at.is_some() {
        return Err(Error::Deleted(id));
    }
    let affected = defs.split.affected_parts(form, patch);
    let loaded_rows = tx
        .query(
            "SELECT part, \"values\" FROM object_values WHERE object_id = $1 AND part = ANY($2) FOR UPDATE",
            &[&id.as_uuid(), &affected],
        )
        .await?;
    let mut parts = split::documents_from_rows(
        loaded_rows
            .iter()
            .map(|row| (row.get("part"), row.get("values"))),
    )
    .map_err(|message| Error::Corrupt(id, message))?;
    let before = parts.clone();
    defs.split.apply_patch(form, &mut parts, patch);
    let existing: BTreeSet<String> = before.keys().cloned().collect();
    for part in &existing {
        if !parts.contains_key(part) {
            tx.execute(
                "DELETE FROM object_values WHERE object_id = $1 AND part = $2",
                &[&id.as_uuid(), part],
            )
            .await?;
        }
    }
    for (part, document) in &parts {
        let value = Value::Object(document.clone());
        if existing.contains(part) {
            tx.execute(
                "UPDATE object_values SET \"values\" = $3 WHERE object_id = $1 AND part = $2",
                &[&id.as_uuid(), part, &value],
            )
            .await?;
        } else {
            tx.execute(
                "INSERT INTO object_values (object_id, part, \"values\") VALUES ($1, $2, $3)",
                &[&id.as_uuid(), part, &value],
            )
            .await?;
        }
    }
    let version_row = tx
        .query_one(
            "UPDATE objects SET version = version + 1, updated_at = now() WHERE id = $1 RETURNING version",
            &[&id.as_uuid()],
        )
        .await?;
    let version = u64_from_i64(version_row.get(0))?;
    let diff = diff_from_patch(&defs.split, form, &before, patch);
    let seq = insert_change(
        tx,
        batch,
        Some(id),
        Some(as_i64(version)?),
        ChangeOp::Update,
        actor,
        &diff,
    )
    .await?;
    Ok((version, seq))
}

async fn delete_object(
    tx: &Transaction<'_>,
    batch: BatchId,
    actor: &Actor,
    id: ObjectId,
) -> Result<(u64, i64), Error> {
    let row = tx
        .query_opt(
            "SELECT deleted_at FROM objects WHERE id = $1 FOR UPDATE",
            &[&id.as_uuid()],
        )
        .await?;
    let Some(row) = row else {
        return Err(Error::NotFound(id));
    };
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row.get(0);
    if deleted_at.is_some() {
        return Err(Error::Deleted(id));
    }
    let version_row = tx
        .query_one(
            "UPDATE objects SET version = version + 1, updated_at = now(), deleted_at = now() WHERE id = $1 RETURNING version",
            &[&id.as_uuid()],
        )
        .await?;
    let version = u64_from_i64(version_row.get(0))?;
    let seq = tx
        .query_one(
            "INSERT INTO changes (batch_id, object_id, version, op, actor, on_behalf_of, diff) VALUES ($1, $2, $3, $4, $5, $6, NULL) RETURNING seq",
            &[
                &batch.as_uuid(),
                &id.as_uuid(),
                &as_i64(version)?,
                &ChangeOp::Delete.as_str(),
                &actor.actor.as_uuid(),
                &actor.on_behalf_of.map(PrincipalId::as_uuid),
            ],
        )
        .await?;
    Ok((version, seq.get(0)))
}

async fn copy_creates(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    objects: &[Object],
) -> Result<(), Error> {
    let mut object_rows = Vec::with_capacity(objects.len());
    let mut value_rows = Vec::new();
    let mut structure_rows = Vec::new();
    let mut change_rows = Vec::with_capacity(objects.len());
    for object in objects {
        let key = defs.key_of(object)?;
        object_rows.push((
            object.id.as_uuid(),
            object.type_id.as_uuid(),
            key,
            1i64,
            as_i64(object.model_revision)?,
        ));
        for (part, document) in defs.split.split(form, &object.values) {
            value_rows.push((object.id.as_uuid(), part, Value::Object(document)));
        }
        for (structure, group) in &object.groups {
            structure_rows.push((object.id.as_uuid(), structure.as_uuid(), group.as_uuid()));
        }
        change_rows.push((object.id.as_uuid(), diff_created(&object.values)));
    }

    let sink = tx
        .copy_in(
            "COPY objects (id, type_id, key, version, model_revision) FROM STDIN WITH (FORMAT binary)",
        )
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[Type::UUID, Type::UUID, Type::TEXT, Type::INT8, Type::INT8],
    ));
    for (id, type_id, key, version, revision) in &object_rows {
        writer
            .as_mut()
            .write(&[id as &(dyn ToSql + Sync), type_id, key, version, revision])
            .await
            .map_err(|error| map_unique(error, TypeId::from_uuid(*type_id), key))?;
    }
    writer
        .finish()
        .await
        .map_err(|error| map_unique(error, TypeId::from_uuid(Uuid::nil()), ""))?;

    let sink = tx
        .copy_in("COPY object_values (object_id, part, \"values\") FROM STDIN WITH (FORMAT binary)")
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[Type::UUID, Type::TEXT, Type::JSONB],
    ));
    for (id, part, value) in &value_rows {
        writer
            .as_mut()
            .write(&[id as &(dyn ToSql + Sync), part, value])
            .await?;
    }
    writer.finish().await?;

    let sink = tx
        .copy_in(
            "COPY object_structures (object_id, structure_id, group_id) FROM STDIN WITH (FORMAT binary)",
        )
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[Type::UUID, Type::UUID, Type::UUID],
    ));
    for (id, structure, group) in &structure_rows {
        writer
            .as_mut()
            .write(&[id as &(dyn ToSql + Sync), structure, group])
            .await?;
    }
    writer.finish().await?;

    let op = ChangeOp::Create.as_str();
    let version = 1i64;
    let actor_id = actor.actor.as_uuid();
    let behalf = actor.on_behalf_of.map(PrincipalId::as_uuid);
    let batch_id = batch.as_uuid();
    let sink = tx
        .copy_in(
            "COPY changes (batch_id, object_id, version, op, actor, on_behalf_of, diff) FROM STDIN WITH (FORMAT binary)",
        )
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[
            Type::UUID,
            Type::UUID,
            Type::INT8,
            Type::TEXT,
            Type::UUID,
            Type::UUID,
            Type::JSONB,
        ],
    ));
    for (object_id, diff) in &change_rows {
        writer
            .as_mut()
            .write(&[
                &batch_id as &(dyn ToSql + Sync),
                object_id,
                &version,
                &op,
                &actor_id,
                &behalf,
                diff,
            ])
            .await?;
    }
    writer.finish().await?;
    Ok(())
}

async fn insert_batch(tx: &Transaction<'_>, actor: &Actor) -> Result<BatchId, Error> {
    let id = BatchId::new();
    tx.execute(
        "INSERT INTO batches (id, actor, on_behalf_of, kind, source) VALUES ($1, $2, $3, $4, $5)",
        &[
            &id.as_uuid(),
            &actor.actor.as_uuid(),
            &actor.on_behalf_of.map(PrincipalId::as_uuid),
            &actor.kind.as_str(),
            &actor.source,
        ],
    )
    .await?;
    Ok(id)
}

async fn insert_change(
    tx: &Transaction<'_>,
    batch: BatchId,
    object: Option<ObjectId>,
    version: Option<i64>,
    op: ChangeOp,
    actor: &Actor,
    diff: &Value,
) -> Result<i64, Error> {
    let row = tx
        .query_one(
            "INSERT INTO changes (batch_id, object_id, version, op, actor, on_behalf_of, diff) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING seq",
            &[
                &batch.as_uuid(),
                &object.map(ObjectId::as_uuid),
                &version,
                &op.as_str(),
                &actor.actor.as_uuid(),
                &actor.on_behalf_of.map(PrincipalId::as_uuid),
                &diff,
            ],
        )
        .await?;
    Ok(row.get(0))
}

fn change_from_row(row: &tokio_postgres::Row) -> Result<ChangeRow, Error> {
    let op =
        ChangeOp::parse(row.get("op")).ok_or_else(|| Error::Data("unknown change op".into()))?;
    let version: Option<i64> = row.get("version");
    Ok(ChangeRow {
        seq: row.get("seq"),
        at: row.get("at"),
        batch_id: BatchId::from_uuid(row.get("batch_id")),
        object_id: row
            .get::<_, Option<Uuid>>("object_id")
            .map(ObjectId::from_uuid),
        version: version.map(u64_from_i64).transpose()?,
        op,
        actor: PrincipalId::from_uuid(row.get("actor")),
        on_behalf_of: row
            .get::<_, Option<Uuid>>("on_behalf_of")
            .map(PrincipalId::from_uuid),
        diff: row.get("diff"),
    })
}

fn row_error(object: &Object, error: &Error) -> BulkRowError {
    let key = match error {
        Error::DuplicateKey { key, .. } => key.clone(),
        _ => String::new(),
    };
    BulkRowError {
        object_id: object.id,
        key,
        message: error.to_string(),
    }
}

fn map_unique(error: tokio_postgres::Error, type_id: TypeId, key: &str) -> Error {
    if let Some(db) = error.as_db_error()
        && db.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
    {
        let constraint = db.constraint();
        if constraint.is_none() || constraint == Some("objects_type_id_key_active") {
            return Error::DuplicateKey {
                type_id,
                key: key.to_owned(),
            };
        }
    }
    Error::Db(error)
}

fn as_i64(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Data(format!("{value} does not fit in bigint")))
}

fn u64_from_i64(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Data(format!("negative stored integer {value}")))
}
