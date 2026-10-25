use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use braian_model::{DefinitionSet, Object, ObjectId, TypeId};
use futures::StreamExt;
use serde_json::{Map, Value};
use tokio_postgres::Transaction;
use tokio_postgres::binary_copy::BinaryCopyInWriter;
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{Actor, BatchId, ChangeOp, PrincipalId, PrincipalKind};
use crate::session::Store;
use crate::split::{self, diff_from_patch};
use crate::types::{
    BulkReport, BulkRowError, CachedDefs, ChangeRow, ObjectOp, PrincipalRecord, StoredObject,
    WriteReceipt,
};

impl Store {
    /// Inserts a principal with a stable handle, or updates kind and name when the handle exists.
    ///
    /// Dev tokens use the handle so a restart does not mint a new principal.
    pub async fn upsert_principal(
        &self,
        handle: &str,
        kind: PrincipalKind,
        name: &str,
    ) -> Result<PrincipalId, Error> {
        if handle.is_empty() {
            return Err(Error::Data("principal handle is empty".into()));
        }
        let id = PrincipalId::new();
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "INSERT INTO principals (id, kind, name, handle) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (handle) DO UPDATE SET kind = EXCLUDED.kind, name = EXCLUDED.name
                 RETURNING id",
                &[&id.as_uuid(), &kind.as_str(), &name, &handle],
            )
            .await?;
        Ok(PrincipalId::from_uuid(row.get(0)))
    }

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
        let previous = match &current {
            Some(row) => Some(
                serde_json::from_value(row.get("definitions"))
                    .map_err(|error| Error::Definitions(error.to_string()))?,
            ),
            None => None,
        };
        let diff = definition_diff(previous.as_ref(), defs);
        let seq =
            insert_change(&tx, batch, None, None, ChangeOp::Definitions, actor, &diff).await?;
        let groups = sync_attribute_parts(&tx, defs).await?;
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_groups(defs.clone(), groups)))
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
        let defs = self.defs_for(&tx).await?;
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
        // A chunk belongs to an open batch that the caller can resume, so it
        // does not need to wait for its own WAL flush. Order and atomicity are
        // unchanged; a crash can only lose chunks that were never acknowledged
        // as durable, never half a chunk.
        tx.batch_execute("SET LOCAL synchronous_commit = off")
            .await?;
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

    /// Loads many objects in three pipelined queries (`objects`, `object_structures`, `object_values`).
    ///
    /// Ids that are missing are omitted. The result follows `ids`, without duplicates.
    pub async fn get_objects(&self, ids: &[ObjectId]) -> Result<Vec<StoredObject>, Error> {
        self.get_objects_projected(ids, &crate::types::ObjectProjection::full())
            .await
    }

    /// Same as [`Self::get_objects`], skipping groups, values, or value parts that
    /// the caller did not ask for.
    pub async fn get_objects_projected(
        &self,
        ids: &[ObjectId],
        projection: &crate::types::ObjectProjection,
    ) -> Result<Vec<StoredObject>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let client = self.pool().get().await?;
        let params: [&(dyn ToSql + Sync); 1] = [&uuids];
        let rows_fut = client.query(
            "SELECT id, type_id, key, version, model_revision, deleted_at
             FROM objects WHERE id = ANY($1)",
            &params,
        );
        let groups_fut = async {
            if !projection.groups {
                return Ok(Vec::new());
            }
            client
                .query(
                    "SELECT object_id, structure_id, group_id
                     FROM object_structures WHERE object_id = ANY($1)",
                    &[&uuids],
                )
                .await
        };
        let parts_fut = async {
            if !projection.values {
                return Ok(Vec::new());
            }
            if let Some(parts) = &projection.parts {
                client
                    .query(
                        "SELECT DISTINCT ON (object_id, part) object_id, part, \"values\"
                         FROM object_values
                         WHERE object_id = ANY($1) AND part = ANY($2)
                         ORDER BY object_id, part, version DESC",
                        &[&uuids, parts],
                    )
                    .await
            } else {
                client
                    .query(
                        "SELECT DISTINCT ON (object_id, part) object_id, part, \"values\"
                         FROM object_values WHERE object_id = ANY($1)
                         ORDER BY object_id, part, version DESC",
                        &[&uuids],
                    )
                    .await
            }
        };
        let (rows, group_rows, part_rows) =
            futures::future::try_join3(rows_fut, groups_fut, parts_fut).await?;
        let mut groups: HashMap<Uuid, Vec<(Uuid, Uuid)>> = HashMap::new();
        for row in &group_rows {
            groups
                .entry(row.get("object_id"))
                .or_default()
                .push((row.get("structure_id"), row.get("group_id")));
        }
        let mut parts: HashMap<Uuid, BTreeMap<String, Value>> = HashMap::new();
        for row in &part_rows {
            parts
                .entry(row.get("object_id"))
                .or_default()
                .insert(row.get("part"), row.get("values"));
        }
        let mut by_id = HashMap::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let stored = self.assemble_loaded(
                row,
                groups.get(&id).map(Vec::as_slice).unwrap_or(&[]),
                parts.get(&id),
            )?;
            by_id.insert(id, stored);
        }
        let mut seen = HashSet::new();
        let mut stored = Vec::with_capacity(ids.len());
        for id in ids {
            if seen.insert(*id)
                && let Some(object) = by_id.remove(&id.as_uuid())
            {
                stored.push(object);
            }
        }
        Ok(stored)
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

    /// Raw part documents for one object. A definition change must not move a value between parts.
    pub async fn stored_parts(&self, id: ObjectId) -> Result<BTreeMap<String, Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT DISTINCT ON (part) part, \"values\" FROM object_values
                 WHERE object_id = $1 ORDER BY part, version DESC",
                &[&id.as_uuid()],
            )
            .await?;
        let mut parts = BTreeMap::new();
        for row in rows {
            parts.insert(row.get("part"), row.get("values"));
        }
        Ok(parts)
    }

    /// The current definition set, or [`Error::NoDefinitions`] when nothing has been stored.
    pub async fn definition_set(&self) -> Result<braian_model::DefinitionSet, Error> {
        Ok(self.defs().await?.set.clone())
    }

    /// Latest stored model revision. `None` when no definitions have been stored.
    pub async fn model_revision(&self) -> Result<Option<u64>, Error> {
        match self.defs().await {
            Ok(defs) => Ok(Some(defs.set.model_revision)),
            Err(Error::NoDefinitions) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn principal(&self, id: PrincipalId) -> Result<Option<PrincipalRecord>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT kind, name, handle FROM principals WHERE id = $1",
                &[&id.as_uuid()],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(PrincipalRecord {
            id,
            kind: PrincipalKind::parse(row.get("kind"))?,
            name: row.get("name"),
            handle: row.get("handle"),
        }))
    }

    /// Loads many principals in one query. Unknown ids are omitted.
    pub async fn principals(&self, ids: &[PrincipalId]) -> Result<Vec<PrincipalRecord>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, kind, name, handle FROM principals WHERE id = ANY($1)",
                &[&uuids],
            )
            .await?;
        rows.iter()
            .map(|row| {
                Ok(PrincipalRecord {
                    id: PrincipalId::from_uuid(row.get("id")),
                    kind: PrincipalKind::parse(row.get("kind"))?,
                    name: row.get("name"),
                    handle: row.get("handle"),
                })
            })
            .collect()
    }

    /// Change log for one object. A create row is rebuilt from `object_values`
    /// version 1 so the stored diff can stay a marker.
    pub async fn changes_for(&self, id: ObjectId) -> Result<Vec<ChangeRow>, Error> {
        let client = self.pool().get().await?;
        let mut rows = stored_changes(&**client, id).await?;
        self.expand_change_rows(&**client, &mut rows).await?;
        Ok(rows)
    }

    /// Change rows as written. A create diff is a `$created` marker, not the values.
    pub async fn stored_changes(&self, id: ObjectId) -> Result<Vec<ChangeRow>, Error> {
        let client = self.pool().get().await?;
        stored_changes(&**client, id).await
    }

    /// Values from one `object_values` version. Version 1 is the create snapshot
    /// and is never rewritten.
    pub async fn values_at_version(
        &self,
        id: ObjectId,
        version: u64,
    ) -> Result<BTreeMap<braian_model::AttributeId, Value>, Error> {
        let client = self.pool().get().await?;
        let version = as_i64(version)?;
        let rows = client
            .query(
                "SELECT part, \"values\" FROM object_values WHERE object_id = $1 AND version = $2",
                &[&id.as_uuid(), &version],
            )
            .await?;
        let encoded = rows
            .iter()
            .map(|row| (row.get("part"), row.get("values")))
            .collect();
        split::join_parts(self.form(), &encoded).map_err(|message| Error::Corrupt(id, message))
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
                for object in &chunk {
                    match self
                        .import_chunk(batch, actor, std::slice::from_ref(object))
                        .await
                    {
                        Ok(()) => *written += 1,
                        Err(error) if error.is_duplicate() => {
                            errors.push(row_error(object, &error));
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
        }?;
        drop(chunk);
        // Heavy chunks allocate a large JSON tree per object. glibc keeps that
        // high-water mark, so RSS grew by about a chunk on every batch.
        release_heap();
        Ok(())
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
                "SELECT DISTINCT ON (part) part, \"values\" FROM object_values
                 WHERE object_id = $1 ORDER BY part, version DESC",
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
                            braian_model::StructureId::from_uuid(group.get("structure_id")),
                            braian_model::GroupId::from_uuid(group.get("group_id")),
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

    fn assemble_loaded(
        &self,
        row: &tokio_postgres::Row,
        groups: &[(Uuid, Uuid)],
        parts: Option<&BTreeMap<String, Value>>,
    ) -> Result<StoredObject, Error> {
        let id = ObjectId::from_uuid(row.get("id"));
        let empty = BTreeMap::new();
        let encoded = parts.unwrap_or(&empty);
        let values = split::join_parts(self.form(), encoded)
            .map_err(|message| Error::Corrupt(id, message))?;
        let version: i64 = row.get("version");
        Ok(StoredObject {
            object: Object {
                id,
                type_id: TypeId::from_uuid(row.get("type_id")),
                model_revision: u64_from_i64(row.get("model_revision"))?,
                groups: groups
                    .iter()
                    .map(|(structure, group)| {
                        (
                            braian_model::StructureId::from_uuid(*structure),
                            braian_model::GroupId::from_uuid(*group),
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

    /// Definitions visible to this transaction.
    ///
    /// A write reloads when another connection has committed a newer revision
    /// than the one this process cached.
    async fn defs_for(&self, tx: &Transaction<'_>) -> Result<Arc<CachedDefs>, Error> {
        let row = tx
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1",
                &[],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::NoDefinitions);
        };
        let revision: i64 = row.get(0);
        if let Ok(cached) = self.defs().await
            && i64::try_from(cached.set.model_revision).ok() == Some(revision)
        {
            return Ok(cached);
        }
        let document: Value = row.get(1);
        let set: braian_model::DefinitionSet = serde_json::from_value(document)
            .map_err(|error| Error::Definitions(error.to_string()))?;
        let groups = load_groups(tx).await?;
        let groups = split::SplitContext::assign_groups(&set, &groups);
        let cached = Arc::new(CachedDefs::from_groups(set, groups));
        self.remember(Arc::clone(&cached)).await;
        Ok(cached)
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
            ObjectOp::Update {
                id,
                patch,
                base_version,
                read_version,
            } => {
                let (version, seq) = update_object(
                    tx,
                    form,
                    defs,
                    batch,
                    actor,
                    *id,
                    patch,
                    *base_version,
                    *read_version,
                )
                .await?;
                (*id, version, seq)
            }
            ObjectOp::Delete { id, base_version } => {
                let (version, seq) = delete_object(tx, batch, actor, *id, *base_version).await?;
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
            "INSERT INTO object_values (object_id, part, version, \"values\") VALUES ($1, $2, 1, $3)",
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
    let diff = split::create_marker(&object.groups);
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

#[allow(clippy::too_many_arguments)]
async fn update_object(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    id: ObjectId,
    patch: &braian_model::ObjectPatch,
    base_version: u64,
    read_version: u64,
) -> Result<(u64, i64), Error> {
    let row = tx
        .query_opt(
            "SELECT type_id, version, deleted_at FROM objects WHERE id = $1 FOR UPDATE",
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
    let current_version = u64_from_i64(row.get("version"))?;
    if current_version != read_version {
        return Err(Error::StaleRead { current_version });
    }
    if base_version > current_version {
        return Err(Error::BaseAhead { current_version });
    }
    let type_id = braian_model::TypeId::from_uuid(row.get("type_id"));
    let mut parts = load_parts_for_update(tx, form, defs, id, &patch.values).await?;
    if base_version < current_version {
        let history = changes_since(tx, id, base_version).await?;
        let conflicts = crate::conflict::conflicts_for(&history, patch, &defs.split, form, &parts);
        if !conflicts.is_empty() {
            return Err(Error::Conflict {
                current_version,
                conflicts,
            });
        }
    }
    let before = parts.clone();
    defs.split.apply_patch(form, &mut parts, &patch.values);
    let version_i64 = as_i64(current_version + 1)?;
    for part in before.keys() {
        if parts.contains_key(part) {
            continue;
        }
        let empty = Value::Object(serde_json::Map::new());
        tx.execute(
            "INSERT INTO object_values (object_id, part, version, \"values\") VALUES ($1, $2, $3, $4)",
            &[&id.as_uuid(), part, &version_i64, &empty],
        )
        .await?;
    }
    for (part, document) in &parts {
        if before.get(part) == Some(document) {
            continue;
        }
        let value = Value::Object(document.clone());
        tx.execute(
            "INSERT INTO object_values (object_id, part, version, \"values\") VALUES ($1, $2, $3, $4)",
            &[&id.as_uuid(), part, &version_i64, &value],
        )
        .await?;
    }
    let placed = apply_groups(tx, id, patch).await?;
    let revision = as_i64(defs.set.model_revision)?;
    let key_attribute = defs
        .set
        .types
        .iter()
        .find(|object_type| object_type.id == type_id)
        .map(|object_type| object_type.key_attribute);
    let version_row = if key_attribute
        .is_some_and(|attribute| patch.values.attributes.contains_key(&attribute))
    {
        let key = new_key(id, patch, key_attribute.expect("checked above"))?;
        tx.query_one(
            "UPDATE objects SET version = version + 1, updated_at = now(), model_revision = $2, key = $3 WHERE id = $1 RETURNING version",
            &[&id.as_uuid(), &revision, &key],
        )
        .await
        .map_err(|error| map_unique(error, type_id, &key))?
    } else {
        tx.query_one(
            "UPDATE objects SET version = version + 1, updated_at = now(), model_revision = $2 WHERE id = $1 RETURNING version",
            &[&id.as_uuid(), &revision],
        )
        .await?
    };
    let version = u64_from_i64(version_row.get(0))?;
    let mut diff = diff_from_patch(&defs.split, form, &before, &patch.values);
    if !placed.is_empty() {
        diff.as_object_mut()
            .expect("diff is an object")
            .insert("$groups".to_owned(), Value::Object(placed));
    }
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
    base_version: Option<u64>,
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
    let current_version = u64_from_i64(row.get("version"))?;
    if let Some(base_version) = base_version {
        if base_version > current_version {
            return Err(Error::BaseAhead { current_version });
        }
        if base_version < current_version {
            let history = changes_since(tx, id, base_version).await?;
            return Err(Error::Conflict {
                current_version,
                conflicts: crate::conflict::all_conflicts(&history),
            });
        }
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
            "INSERT INTO changes (batch_id, object_id, version, op, actor, on_behalf_of, diff, kind) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7) RETURNING seq",
            &[
                &batch.as_uuid(),
                &id.as_uuid(),
                &as_i64(version)?,
                &ChangeOp::Delete.as_str(),
                &actor.actor.as_uuid(),
                &actor.on_behalf_of.map(PrincipalId::as_uuid),
                &actor.kind.as_str(),
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
    // Stream each COPY. Holding every part and every diff for the whole chunk
    // kept a second and third copy of each heavy object alive at once.
    let sink = tx
        .copy_in(
            "COPY objects (id, type_id, key, version, model_revision) FROM STDIN WITH (FORMAT binary)",
        )
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[Type::UUID, Type::UUID, Type::TEXT, Type::INT8, Type::INT8],
    ));
    for object in objects {
        let key = defs.key_of(object)?;
        let id = object.id.as_uuid();
        let type_id = object.type_id.as_uuid();
        let version = 1i64;
        let revision = as_i64(object.model_revision)?;
        writer
            .as_mut()
            .write(&[
                &id as &(dyn ToSql + Sync),
                &type_id,
                &key,
                &version,
                &revision,
            ])
            .await
            .map_err(|error| map_unique(error, object.type_id, &key))?;
    }
    writer
        .finish()
        .await
        .map_err(|error| map_unique(error, TypeId::from_uuid(Uuid::nil()), ""))?;

    let sink = tx
        .copy_in(
            "COPY object_values (object_id, part, version, \"values\") FROM STDIN WITH (FORMAT binary)",
        )
        .await?;
    let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
        sink,
        &[Type::UUID, Type::TEXT, Type::INT8, Type::JSONB],
    ));
    let value_version = 1i64;
    for object in objects {
        let id = object.id.as_uuid();
        for (part, document) in defs.split.split_ref(form, &object.values) {
            let value = tokio_postgres::types::Json(&document);
            writer
                .as_mut()
                .write(&[&id as &(dyn ToSql + Sync), &part, &value_version, &value])
                .await?;
        }
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
    for object in objects {
        let id = object.id.as_uuid();
        for (structure, group) in &object.groups {
            let structure = structure.as_uuid();
            let group = group.as_uuid();
            writer
                .as_mut()
                .write(&[&id as &(dyn ToSql + Sync), &structure, &group])
                .await?;
        }
    }
    writer.finish().await?;

    let op = ChangeOp::Create.as_str();
    let version = 1i64;
    let actor_id = actor.actor.as_uuid();
    let behalf = actor.on_behalf_of.map(PrincipalId::as_uuid);
    let batch_id = batch.as_uuid();
    let kind = actor.kind.as_str();
    let sink = tx
        .copy_in(
            "COPY changes (batch_id, object_id, version, op, actor, on_behalf_of, diff, kind) FROM STDIN WITH (FORMAT binary)",
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
            Type::TEXT,
        ],
    ));
    for object in objects {
        let object_id = object.id.as_uuid();
        let diff = tokio_postgres::types::Json(split::create_marker(&object.groups));
        writer
            .as_mut()
            .write(&[
                &batch_id as &(dyn ToSql + Sync),
                &object_id,
                &version,
                &op,
                &actor_id,
                &behalf,
                &diff,
                &kind,
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
            "INSERT INTO changes (batch_id, object_id, version, op, actor, on_behalf_of, diff, kind) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING seq",
            &[
                &batch.as_uuid(),
                &object.map(ObjectId::as_uuid),
                &version,
                &op.as_str(),
                &actor.actor.as_uuid(),
                &actor.on_behalf_of.map(PrincipalId::as_uuid),
                &diff,
                &actor.kind.as_str(),
            ],
        )
        .await?;
    Ok(row.get(0))
}

async fn stored_changes(
    client: &impl tokio_postgres::GenericClient,
    id: ObjectId,
) -> Result<Vec<ChangeRow>, Error> {
    let rows = client
        .query(
            "SELECT seq, at, batch_id, object_id, version, op, actor, on_behalf_of, diff FROM changes WHERE object_id = $1 ORDER BY seq",
            &[&id.as_uuid()],
        )
        .await?;
    rows.iter().map(change_from_row).collect()
}

impl Store {
    pub(crate) async fn expand_history_diffs(
        &self,
        client: &impl tokio_postgres::GenericClient,
        entries: &mut [crate::product::HistoryEntry],
    ) -> Result<(), Error> {
        let mut ids = Vec::new();
        for entry in entries.iter() {
            if entry.diff.as_ref().is_some_and(split::is_create_marker)
                && let Some(id) = entry.object_id
            {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return Ok(());
        }
        ids.sort();
        ids.dedup();
        let created = created_values(self, client, &ids).await?;
        let empty = BTreeMap::new();
        for entry in entries.iter_mut() {
            let Some(diff) = entry.diff.as_ref() else {
                continue;
            };
            if !split::is_create_marker(diff) {
                continue;
            }
            let values = entry
                .object_id
                .and_then(|id| created.get(&id))
                .unwrap_or(&empty);
            let expanded = split::expand_create_marker(diff, values);
            entry.diff = Some(expanded);
        }
        Ok(())
    }

    async fn expand_change_rows(
        &self,
        client: &impl tokio_postgres::GenericClient,
        rows: &mut [ChangeRow],
    ) -> Result<(), Error> {
        let mut ids = Vec::new();
        for row in rows.iter() {
            if row.diff.as_ref().is_some_and(split::is_create_marker)
                && let Some(id) = row.object_id
            {
                ids.push(id.as_uuid());
            }
        }
        if ids.is_empty() {
            return Ok(());
        }
        ids.sort();
        ids.dedup();
        let created = created_values(self, client, &ids).await?;
        let empty = BTreeMap::new();
        for row in rows.iter_mut() {
            let Some(diff) = row.diff.as_ref() else {
                continue;
            };
            if !split::is_create_marker(diff) {
                continue;
            }
            let values = row
                .object_id
                .and_then(|id| created.get(&id.as_uuid()))
                .unwrap_or(&empty);
            let expanded = split::expand_create_marker(diff, values);
            row.diff = Some(expanded);
        }
        Ok(())
    }
}

async fn created_values(
    store: &Store,
    client: &impl tokio_postgres::GenericClient,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, BTreeMap<braian_model::AttributeId, Value>>, Error> {
    let rows = client
        .query(
            "SELECT object_id, part, \"values\" FROM object_values WHERE object_id = ANY($1) AND version = 1",
            &[&ids],
        )
        .await?;
    let mut parts: HashMap<Uuid, BTreeMap<String, Value>> = HashMap::new();
    for row in &rows {
        parts
            .entry(row.get("object_id"))
            .or_default()
            .insert(row.get("part"), row.get("values"));
    }
    let mut values = HashMap::with_capacity(parts.len());
    for (id, encoded) in parts {
        let joined = split::join_parts(store.form(), &encoded)
            .map_err(|message| Error::Corrupt(ObjectId::from_uuid(id), message))?;
        values.insert(id, joined);
    }
    Ok(values)
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

async fn load_parts_for_update(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    id: ObjectId,
    patch: &braian_model::ValuePatch,
) -> Result<BTreeMap<String, serde_json::Map<String, Value>>, Error> {
    let stale = defs.split.stale_parts(form, patch);
    let rows = match form {
        crate::split::StorageForm::Document => {
            tx.query(
                "SELECT v.part, v.\"values\"
                 FROM object_values v
                 JOIN (
                    SELECT part, MAX(version) AS version
                    FROM object_values WHERE object_id = $1 GROUP BY part
                 ) latest ON v.object_id = $1 AND v.part = latest.part AND v.version = latest.version
                 WHERE v.part = ANY($2)
                 FOR UPDATE OF v",
                &[&id.as_uuid(), &stale.destinations],
            )
            .await?
        }
        crate::split::StorageForm::PerLanguage => {
            tx.query(
                "SELECT v.part, v.\"values\"
                 FROM object_values v
                 JOIN (
                    SELECT part, MAX(version) AS version
                    FROM object_values WHERE object_id = $1 GROUP BY part
                 ) latest ON v.object_id = $1 AND v.part = latest.part AND v.version = latest.version
                 WHERE (
                    v.part = ANY($2)
                    OR (v.part = '' AND v.\"values\" ?| $3::text[])
                    OR (v.part <> '' AND v.\"values\" ?| $4::text[])
                 )
                 FOR UPDATE OF v",
                &[
                    &id.as_uuid(),
                    &stale.destinations,
                    &stale.localized_keys,
                    &stale.plain_keys,
                ],
            )
            .await?
        }
        crate::split::StorageForm::PerLanguageGroup => {
            tx.query(
                "SELECT v.part, v.\"values\"
                 FROM object_values v
                 JOIN (
                    SELECT part, MAX(version) AS version
                    FROM object_values WHERE object_id = $1 GROUP BY part
                 ) latest ON v.object_id = $1 AND v.part = latest.part AND v.version = latest.version
                 WHERE (
                    v.part = ANY($2)
                    OR (v.part LIKE '#%' AND split_part(v.part, '#', 2) = ANY($3::text[]) AND v.\"values\" ?| $4::text[])
                    OR (v.part NOT LIKE '#%' AND split_part(v.part, '#', 2) = ANY($5::text[]) AND v.\"values\" ?| $6::text[])
                 )
                 FOR UPDATE OF v",
                &[
                    &id.as_uuid(),
                    &stale.destinations,
                    &stale.localized_groups,
                    &stale.localized_keys,
                    &stale.plain_groups,
                    &stale.plain_keys,
                ],
            )
            .await?
        }
    };
    split::documents_from_rows(rows.iter().map(|row| (row.get("part"), row.get("values"))))
        .map_err(|message| Error::Corrupt(id, message))
}

async fn load_groups(
    tx: &Transaction<'_>,
) -> Result<HashMap<braian_model::AttributeId, usize>, Error> {
    let rows = tx
        .query("SELECT attribute_id, part_group FROM attribute_parts", &[])
        .await?;
    let mut groups = HashMap::new();
    for row in rows {
        let group: i32 = row.get(1);
        groups.insert(
            braian_model::AttributeId::from_uuid(row.get(0)),
            usize::try_from(group).unwrap_or(0),
        );
    }
    Ok(groups)
}

async fn sync_attribute_parts(
    tx: &Transaction<'_>,
    defs: &braian_model::DefinitionSet,
) -> Result<HashMap<braian_model::AttributeId, usize>, Error> {
    let rows = tx
        .query("SELECT attribute_id, part_group FROM attribute_parts", &[])
        .await?;
    let mut existing = HashMap::new();
    for row in rows {
        let group: i32 = row.get(1);
        existing.insert(
            braian_model::AttributeId::from_uuid(row.get(0)),
            usize::try_from(group).unwrap_or(0),
        );
    }
    let assigned = split::SplitContext::assign_groups(defs, &existing);
    for attribute in &defs.attributes {
        let Some(group) = assigned.get(&attribute.id).copied() else {
            continue;
        };
        let stored = i32::try_from(group)
            .map_err(|_| Error::Data(format!("part group {group} does not fit in integer")))?;
        tx.execute(
            "INSERT INTO attribute_parts (attribute_id, part_group) VALUES ($1, $2) ON CONFLICT (attribute_id) DO NOTHING",
            &[&attribute.id.as_uuid(), &stored],
        )
        .await?;
    }
    Ok(assigned)
}

pub(crate) async fn backfill_attribute_parts(
    client: &mut tokio_postgres::Client,
) -> Result<(), Error> {
    let row = client
        .query_one(
            "SELECT
                EXISTS (SELECT 1 FROM attribute_parts) AS has_parts,
                (SELECT definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1) AS definitions",
            &[],
        )
        .await?;
    if row.get::<_, bool>("has_parts") {
        return Ok(());
    }
    let Some(document) = row.get::<_, Option<Value>>("definitions") else {
        return Ok(());
    };
    let set: braian_model::DefinitionSet =
        serde_json::from_value(document).map_err(|error| Error::Definitions(error.to_string()))?;
    let groups = split::SplitContext::sorted_groups(&set);
    for (id, group) in groups {
        let stored = i32::try_from(group)
            .map_err(|_| Error::Data(format!("part group {group} does not fit in integer")))?;
        client
            .execute(
                "INSERT INTO attribute_parts (attribute_id, part_group) VALUES ($1, $2) ON CONFLICT (attribute_id) DO NOTHING",
                &[&id.as_uuid(), &stored],
            )
            .await?;
    }
    Ok(())
}

async fn apply_groups(
    tx: &Transaction<'_>,
    id: ObjectId,
    patch: &braian_model::ObjectPatch,
) -> Result<Map<String, Value>, Error> {
    if patch.groups.is_empty() {
        return Ok(Map::new());
    }
    let rows = tx
        .query(
            "SELECT structure_id, group_id FROM object_structures WHERE object_id = $1 FOR UPDATE",
            &[&id.as_uuid()],
        )
        .await?;
    let mut current = HashMap::new();
    for row in &rows {
        current.insert(
            braian_model::StructureId::from_uuid(row.get("structure_id")),
            braian_model::GroupId::from_uuid(row.get("group_id")),
        );
    }
    let mut diff = Map::new();
    for (structure, group) in &patch.groups {
        let before = current.get(structure).copied();
        match group {
            Some(group) => {
                tx.execute(
                    "INSERT INTO object_structures (object_id, structure_id, group_id) VALUES ($1, $2, $3)
                     ON CONFLICT (object_id, structure_id) DO UPDATE SET group_id = EXCLUDED.group_id",
                    &[&id.as_uuid(), &structure.as_uuid(), &group.as_uuid()],
                )
                .await?;
            }
            None => {
                tx.execute(
                    "DELETE FROM object_structures WHERE object_id = $1 AND structure_id = $2",
                    &[&id.as_uuid(), &structure.as_uuid()],
                )
                .await?;
            }
        }
        let mut entry = Map::new();
        if let Some(before) = before {
            entry.insert(
                "before".to_owned(),
                Value::String(before.as_uuid().to_string()),
            );
        }
        entry.insert(
            "after".to_owned(),
            match group {
                Some(group) => Value::String(group.as_uuid().to_string()),
                None => Value::Null,
            },
        );
        diff.insert(structure.as_uuid().to_string(), Value::Object(entry));
    }
    Ok(diff)
}

fn new_key(
    id: ObjectId,
    patch: &braian_model::ObjectPatch,
    attribute: braian_model::AttributeId,
) -> Result<String, Error> {
    match patch.values.attributes.get(&attribute) {
        Some(braian_model::AttributePatch::Replace { value }) => value
            .as_str()
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .ok_or(Error::BadKey(id)),
        _ => Err(Error::BadKey(id)),
    }
}

async fn changes_since(
    tx: &Transaction<'_>,
    id: ObjectId,
    base_version: u64,
) -> Result<Vec<crate::conflict::ChangeView>, Error> {
    let rows = tx
        .query(
            "SELECT version, actor, on_behalf_of, diff FROM changes WHERE object_id = $1 AND version > $2 ORDER BY version",
            &[&id.as_uuid(), &as_i64(base_version)?],
        )
        .await?;
    let mut history = Vec::new();
    for row in rows {
        let Some(diff) = row.get::<_, Option<Value>>("diff") else {
            continue;
        };
        let version: i64 = row.get("version");
        history.push(crate::conflict::ChangeView {
            version: u64_from_i64(version)?,
            actor: PrincipalId::from_uuid(row.get("actor")),
            on_behalf_of: row
                .get::<_, Option<Uuid>>("on_behalf_of")
                .map(PrincipalId::from_uuid),
            diff,
        });
    }
    Ok(history)
}

fn definition_diff(before: Option<&DefinitionSet>, after: &DefinitionSet) -> Value {
    let empty = DefinitionSet {
        model_revision: 0,
        languages: Vec::new(),
        unit_families: Vec::new(),
        value_lists: Vec::new(),
        attributes: Vec::new(),
        types: Vec::new(),
        structures: Vec::new(),
    };
    let before = before.unwrap_or(&empty);
    let mut entities = Vec::new();
    diff_entities(
        &mut entities,
        "value_list",
        &before.value_lists,
        &after.value_lists,
        |item| item.id.as_uuid(),
    );
    diff_entities(
        &mut entities,
        "attribute",
        &before.attributes,
        &after.attributes,
        |item| item.id.as_uuid(),
    );
    diff_entities(&mut entities, "type", &before.types, &after.types, |item| {
        item.id.as_uuid()
    });
    diff_entities(
        &mut entities,
        "structure",
        &before.structures,
        &after.structures,
        |item| item.id.as_uuid(),
    );
    diff_entities(
        &mut entities,
        "unit_family",
        &before.unit_families,
        &after.unit_families,
        |item| item.id.as_uuid(),
    );
    if before.languages != after.languages {
        entities.push(serde_json::json!({
            "kind": "languages",
            "op": "update",
        }));
    }
    serde_json::json!({ "entities": entities })
}

fn diff_entities<T: PartialEq>(
    entities: &mut Vec<Value>,
    kind: &str,
    before: &[T],
    after: &[T],
    id_of: impl Fn(&T) -> Uuid,
) {
    let before_ids: HashMap<Uuid, &T> = before.iter().map(|item| (id_of(item), item)).collect();
    let after_ids: HashMap<Uuid, &T> = after.iter().map(|item| (id_of(item), item)).collect();
    let mut ids: Vec<Uuid> = before_ids
        .keys()
        .copied()
        .chain(after_ids.keys().copied())
        .collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let op = match (before_ids.get(&id), after_ids.get(&id)) {
            (None, Some(_)) => "create",
            (Some(_), None) => "delete",
            (Some(left), Some(right)) if *left != *right => "update",
            _ => continue,
        };
        entities.push(serde_json::json!({
            "kind": kind,
            "id": id.to_string(),
            "op": op,
        }));
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

fn release_heap() {
    #[cfg(target_os = "linux")]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        unsafe {
            malloc_trim(0);
        }
    }
}

fn as_i64(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Data(format!("{value} does not fit in bigint")))
}

fn u64_from_i64(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Data(format!("negative stored integer {value}")))
}
