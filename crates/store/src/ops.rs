use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use braian_model::{AttributeId, DefinitionSet, GroupId, Object, ObjectId, StructureId, TypeId};
use futures::StreamExt;
use serde_json::{Map, Value};
use tokio_postgres::Transaction;
use tokio_postgres::binary_copy::BinaryCopyInWriter;
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{Actor, BatchId, ChangeOp, PrincipalId, PrincipalKind};
use crate::session::{DbLane, Store};
use crate::split::{self, diff_from_patch};
use crate::types::{
    BulkReport, BulkRowError, CachedDefs, ChangeRow, ObjectAudit, ObjectOp, PrincipalRecord,
    StoredObject, WriteReceipt,
};

/// What an accepted proposal row writes, in the same transaction as its status change.
pub enum ProposalWrite {
    /// The row is rejected. Nothing else is written.
    None,
    Objects {
        ops: Vec<ObjectOp>,
        /// For an update, the object must still be at this version or the transaction rolls back.
        expect_version: Option<(ObjectId, u64)>,
    },
    Assortment {
        id: Uuid,
        owner: Uuid,
        type_id: Uuid,
        name: String,
        query: Value,
    },
    Definitions {
        defs: DefinitionSet,
        purge: Box<braian_model::DataPurge>,
    },
}

struct CommittedProposalRow {
    receipt: WriteReceipt,
    definitions: Option<(DefinitionSet, HashMap<AttributeId, usize>)>,
}

/// Holds `proposals` so a second accept waits, then sees the rows are no longer pending.
pub struct ProposalLock {
    client: Option<deadpool_postgres::Object>,
}

impl ProposalLock {
    /// Releases the lock. The row writes have already committed on their own.
    pub async fn release(mut self) -> Result<(), Error> {
        let Some(client) = self.client.take() else {
            return Ok(());
        };
        client.batch_execute("ROLLBACK").await?;
        Ok(())
    }
}

impl Drop for ProposalLock {
    fn drop(&mut self) {
        let Some(client) = self.client.take() else {
            return;
        };
        // A cancelled accept must not return the pooled connection mid-transaction.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = client.batch_execute("ROLLBACK").await;
            });
        }
    }
}

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
        let (receipt, groups) = self.write_definitions_in(&tx, actor, defs).await?;
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_groups(defs.clone(), groups)))
            .await;
        Ok(receipt)
    }

    /// Saves definitions and, in the same batch, removes the data the purge names.
    pub async fn put_definitions_purging(
        &self,
        actor: &Actor,
        defs: &DefinitionSet,
        purge: &braian_model::DataPurge,
    ) -> Result<WriteReceipt, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let (mut receipt, groups) = self.write_definitions_in(&tx, actor, defs).await?;
        let seq = crate::purge::apply(&tx, actor, receipt.batch_id, purge).await?;
        if seq > receipt.last_seq {
            receipt.last_seq = seq;
        }
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_groups(defs.clone(), groups)))
            .await;
        Ok(receipt)
    }

    pub async fn measure_purge(
        &self,
        purge: &braian_model::DataPurge,
    ) -> Result<braian_model::ConsequenceCounts, Error> {
        let client = self.pool().get().await?;
        crate::purge::measure(&client, purge).await
    }

    /// Gives every type its standard attributes and the trees its objects already use.
    ///
    /// Runs once, when a stored type has no `structures` field. Value moves and the
    /// new definition revision share one transaction. Search is asked to rebuild.
    pub async fn upgrade_standard_attributes(&self) -> Result<bool, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let row = tx
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1 FOR UPDATE",
                &[],
            )
            .await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let revision: i64 = row.get(0);
        let document: Value = row.get(1);
        let pending = document
            .get("types")
            .and_then(Value::as_array)
            .is_some_and(|types| types.iter().any(|item| item.get("structures").is_none()));
        if !pending {
            return Ok(false);
        }
        let mut defs: DefinitionSet = serde_json::from_value(document.clone())
            .map_err(|error| Error::Definitions(error.to_string()))?;
        let usage_rows = tx
            .query(
                "SELECT DISTINCT object.type_id, placement.structure_id
                 FROM objects object
                 JOIN object_structures placement ON placement.object_id = object.id
                 WHERE object.deleted_at IS NULL",
                &[],
            )
            .await?;
        let mut usage: std::collections::BTreeMap<TypeId, std::collections::BTreeSet<StructureId>> =
            std::collections::BTreeMap::new();
        for row in &usage_rows {
            usage
                .entry(TypeId::from_uuid(row.get(0)))
                .or_default()
                .insert(StructureId::from_uuid(row.get(1)));
        }
        braian_model::install_standard_attributes(&mut defs);
        braian_model::structures_from_usage(&mut defs, &usage);
        let existing = load_groups(&tx).await?;
        let assigned = split::SplitContext::assign_groups(&defs, &existing);
        let context = split::SplitContext::with_groups(&defs, &assigned);
        rewrite_standard_values(&tx, self.form(), &context, &defs).await?;
        braian_model::detach_name_aliases(&mut defs);
        defs.model_revision = u64::try_from(revision)
            .map_err(|_| Error::Data("definition revision does not fit".into()))?
            + 1;
        let actor_id: Uuid = tx
            .query_one(
                "INSERT INTO principals (id, kind, name, handle) VALUES ($1, 'integration', 'System', 'system')
                 ON CONFLICT (handle) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind
                 RETURNING id",
                &[&Uuid::now_v7()],
            )
            .await?
            .get(0);
        let actor = Actor {
            actor: PrincipalId::from_uuid(actor_id),
            on_behalf_of: None,
            kind: crate::ids::BatchKind::Bulk,
            source: "standard-attributes".into(),
        };
        let (_receipt, groups) = self.write_definitions_in(&tx, &actor, &defs).await?;
        tx.execute(
            "UPDATE indexer_progress SET reindex_requested = true, updated_at = now() WHERE id = 1",
            &[],
        )
        .await?;
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_groups(defs, groups)))
            .await;
        Ok(true)
    }

    /// Turns stored object references into links and drops those attributes from the latest model.
    ///
    /// Relation rows are already links by the time this runs. A second call finds no
    /// `object_ref` attributes and does nothing. Search is asked to rebuild because
    /// those values leave the object documents.
    pub async fn upgrade_object_refs(&self) -> Result<bool, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let row = tx
            .query_opt(
                "SELECT model_revision, definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT 1 FOR UPDATE",
                &[],
            )
            .await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let revision: i64 = row.get(0);
        let document: Value = row.get(1);
        let defs: DefinitionSet = serde_json::from_value(document)
            .map_err(|error| Error::Definitions(error.to_string()))?;
        let ref_ids: Vec<AttributeId> = defs
            .attributes
            .iter()
            .filter(|attribute| {
                matches!(
                    attribute.kind,
                    braian_model::AttributeKind::ObjectRef { .. }
                )
            })
            .map(|attribute| attribute.id)
            .collect();
        if ref_ids.is_empty() {
            return Ok(false);
        }
        let value_rows = tx
            .query(
                "SELECT object_id, \"values\" FROM object_values WHERE is_latest",
                &[],
            )
            .await?;
        let object_rows = tx.query("SELECT id, type_id FROM objects", &[]).await?;
        let mut object_types: HashMap<Uuid, Uuid> = HashMap::new();
        for row in &object_rows {
            object_types.insert(row.get(0), row.get(1));
        }
        let mut by_object: HashMap<Uuid, Vec<Value>> = HashMap::new();
        for row in &value_rows {
            by_object.entry(row.get(0)).or_default().push(row.get(1));
        }
        let mut max_values: BTreeMap<AttributeId, i64> = BTreeMap::new();
        let mut stored: Vec<(Uuid, AttributeId, Vec<Uuid>)> = Vec::new();
        for (object_id, parts) in &by_object {
            for attribute in &ref_ids {
                let mut targets = Vec::new();
                for part in parts {
                    if let Some(value) = part.get(attribute.as_uuid().to_string()) {
                        collect_uuids(value, &mut targets);
                    }
                }
                targets.sort();
                targets.dedup();
                if targets.is_empty() {
                    continue;
                }
                let count = i64::try_from(targets.len()).unwrap_or(i64::MAX);
                let slot = max_values.entry(*attribute).or_insert(0);
                if count > *slot {
                    *slot = count;
                }
                stored.push((*object_id, *attribute, targets));
            }
        }
        let Some(converted) = braian_model::convert_object_refs(&defs, &max_values) else {
            return Ok(false);
        };
        let mut defs = converted.defs;
        defs.model_revision = u64::try_from(revision)
            .map_err(|_| Error::Data("definition revision does not fit".into()))?
            + 1;
        let actor_id: Uuid = tx
            .query_one(
                "INSERT INTO principals (id, kind, name, handle) VALUES ($1, 'integration', 'System', 'system')
                 ON CONFLICT (handle) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind
                 RETURNING id",
                &[&Uuid::now_v7()],
            )
            .await?
            .get(0);
        let actor = Actor {
            actor: PrincipalId::from_uuid(actor_id),
            on_behalf_of: None,
            kind: crate::ids::BatchKind::Bulk,
            source: "object-refs-to-links".into(),
        };
        let (receipt, groups) = self.write_definitions_in(&tx, &actor, &defs).await?;
        for plan in &converted.plans {
            let Some(link_type) = defs.types.iter().find(|item| item.id == plan.link_type_id)
            else {
                continue;
            };
            let Some(spec) = link_type.link.as_ref() else {
                continue;
            };
            let from_one = spec.from_cardinality == braian_model::Cardinality::One;
            let to_one = spec.to_cardinality == braian_model::Cardinality::One;
            for (holder, attribute, targets) in &stored {
                if *attribute != plan.attribute_id {
                    continue;
                }
                let Some(holder_type) = object_types.get(holder).copied() else {
                    continue;
                };
                if !spec
                    .to_types
                    .iter()
                    .any(|type_id| type_id.as_uuid() == holder_type)
                {
                    continue;
                }
                for target in targets {
                    if target == holder {
                        continue;
                    }
                    let Some(target_type) = object_types.get(target).copied() else {
                        continue;
                    };
                    if !spec
                        .from_types
                        .iter()
                        .any(|type_id| type_id.as_uuid() == target_type)
                    {
                        continue;
                    }
                    let link_id = ref_link_id(attribute.as_uuid(), *target, *holder);
                    let exists: bool = tx
                        .query_one(
                            "SELECT EXISTS (SELECT 1 FROM objects WHERE id = $1)",
                            &[&link_id],
                        )
                        .await?
                        .get(0);
                    if exists {
                        continue;
                    }
                    tx.execute(
                        "INSERT INTO objects (id, type_id, key, version, model_revision, updated_by)
                         VALUES ($1, $2, NULL, 1, $3, $4)",
                        &[
                            &link_id,
                            &plan.link_type_id.as_uuid(),
                            &as_i64(defs.model_revision)?,
                            &actor_id,
                        ],
                    )
                    .await?;
                    let marker = split::create_marker(&BTreeMap::new());
                    insert_change(
                        &tx,
                        receipt.batch_id,
                        Some(ObjectId::from_uuid(link_id)),
                        Some(1),
                        ChangeOp::Create,
                        &actor,
                        &marker,
                    )
                    .await?;
                    tx.execute(
                        "INSERT INTO links (object_id, link_type_id, from_id, to_id, from_one, to_one)
                         VALUES ($1, $2, $3, $4, $5, $6)",
                        &[
                            &link_id,
                            &plan.link_type_id.as_uuid(),
                            &target,
                            &holder,
                            &from_one,
                            &to_one,
                        ],
                    )
                    .await?;
                }
            }
        }
        for attribute in &converted.strip {
            let key = attribute.as_uuid().to_string();
            tx.execute(
                "UPDATE object_values SET \"values\" = \"values\" - $1 WHERE is_latest AND \"values\" ? $1",
                &[&key],
            )
            .await?;
        }
        tx.execute(
            "UPDATE indexer_progress SET reindex_requested = true, updated_at = now() WHERE id = 1",
            &[],
        )
        .await?;
        tx.commit().await?;
        self.remember(Arc::new(CachedDefs::from_groups(defs, groups)))
            .await;
        Ok(true)
    }

    /// Objects of this type with no group in the structure.
    pub async fn unplaced_count(
        &self,
        type_id: TypeId,
        structure_id: StructureId,
    ) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(*)
                 FROM objects object
                 WHERE object.type_id = $1
                   AND object.deleted_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM object_structures placement
                       WHERE placement.object_id = object.id
                         AND placement.structure_id = $2
                   )",
                &[&type_id.as_uuid(), &structure_id.as_uuid()],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Created, updated, and who last wrote each object.
    pub async fn object_audits(&self, ids: &[ObjectId]) -> Result<Vec<ObjectAudit>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT object.id, object.created_at, object.updated_at, object.updated_by,
                        principal.name, principal.kind
                 FROM objects object
                 LEFT JOIN principals principal ON principal.id = object.updated_by
                 WHERE object.id = ANY($1)",
                &[&keys],
            )
            .await?;
        rows.iter()
            .map(|row| {
                let kind: Option<String> = row.get(5);
                Ok(ObjectAudit {
                    id: ObjectId::from_uuid(row.get(0)),
                    created_at: row.get(1),
                    updated_at: row.get(2),
                    updated_by: row.get::<_, Option<Uuid>>(3).map(PrincipalId::from_uuid),
                    updated_by_name: row.get(4),
                    updated_by_kind: kind.as_deref().map(PrincipalKind::parse).transpose()?,
                })
            })
            .collect()
    }

    /// Locks the proposal until [`ProposalLock::release`]. Row writes use their own transactions.
    pub async fn lock_proposal(&self, id: Uuid) -> Result<ProposalLock, Error> {
        let client = self.pool().get().await?;
        client.batch_execute("BEGIN").await?;
        let locked = client
            .query_opt("SELECT id FROM proposals WHERE id = $1 FOR UPDATE", &[&id])
            .await;
        let locked = match locked {
            Ok(row) => row,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(error.into());
            }
        };
        if locked.is_none() {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(Error::Data(format!("proposal {id} was not found")));
        }
        Ok(ProposalLock {
            client: Some(client),
        })
    }

    /// Sets the row status and applies `write` in one transaction.
    ///
    /// `Ok(None)` means the row was not `pending`: another accept owns it or already finished it.
    /// A write error rolls the status change back, so a crash or a failed write leaves `pending`
    /// and does not keep the object change.
    pub async fn commit_proposal_row(
        &self,
        actor: &Actor,
        proposal_id: Uuid,
        row_id: Uuid,
        status: &str,
        write: ProposalWrite,
    ) -> Result<Option<WriteReceipt>, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let outcome = self
            .commit_proposal_row_in(&tx, actor, proposal_id, row_id, status, write)
            .await;
        match outcome {
            Ok(value) => {
                tx.commit().await?;
                if let Some(committed) = value.as_ref()
                    && let Some((defs, groups)) = committed.definitions.as_ref()
                {
                    self.remember(Arc::new(CachedDefs::from_groups(
                        defs.clone(),
                        groups.clone(),
                    )))
                    .await;
                }
                Ok(value.map(|committed| committed.receipt))
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    async fn commit_proposal_row_in(
        &self,
        tx: &Transaction<'_>,
        actor: &Actor,
        proposal_id: Uuid,
        row_id: Uuid,
        status: &str,
        write: ProposalWrite,
    ) -> Result<Option<CommittedProposalRow>, Error> {
        let claimed = tx
            .query_opt(
                "UPDATE proposal_rows SET status = $3
                 WHERE proposal_id = $1 AND id = $2 AND status = 'pending'
                 RETURNING id",
                &[&proposal_id, &row_id, &status],
            )
            .await?;
        if claimed.is_none() {
            return Ok(None);
        }
        let receipt = match write {
            ProposalWrite::None => WriteReceipt {
                batch_id: BatchId::from_uuid(Uuid::nil()),
                last_seq: 0,
                versions: Vec::new(),
            },
            ProposalWrite::Objects {
                ops,
                expect_version,
            } => {
                if let Some((id, expected)) = expect_version {
                    self.ensure_object_version(tx, id, expected).await?;
                }
                self.ensure_actor(tx, actor).await?;
                let batch = insert_batch(tx, actor).await?;
                let defs = self.defs_for(tx).await?;
                apply_ops(tx, self.form(), &defs, batch, actor, &ops).await?
            }
            ProposalWrite::Assortment {
                id,
                owner,
                type_id,
                name,
                query,
            } => {
                tx.execute(
                    "INSERT INTO assortments (id, owner_id, type_id, name, query)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, query = EXCLUDED.query,
                       type_id = EXCLUDED.type_id",
                    &[&id, &owner, &type_id, &name, &query],
                )
                .await?;
                WriteReceipt {
                    batch_id: BatchId::from_uuid(Uuid::nil()),
                    last_seq: 0,
                    versions: Vec::new(),
                }
            }
            ProposalWrite::Definitions { defs, purge } => {
                let (mut receipt, groups) = self.write_definitions_in(tx, actor, &defs).await?;
                let seq = crate::purge::apply(tx, actor, receipt.batch_id, purge.as_ref()).await?;
                if seq > receipt.last_seq {
                    receipt.last_seq = seq;
                }
                return Ok(Some(CommittedProposalRow {
                    receipt,
                    definitions: Some((defs, groups)),
                }));
            }
        };
        Ok(Some(CommittedProposalRow {
            receipt,
            definitions: None,
        }))
    }

    async fn ensure_object_version(
        &self,
        tx: &Transaction<'_>,
        id: ObjectId,
        expected: u64,
    ) -> Result<(), Error> {
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
        let version = u64_from_i64(row.get("version"))?;
        if version > expected {
            return Err(Error::Conflict {
                current_version: version,
                conflicts: Vec::new(),
            });
        }
        if version < expected {
            return Err(Error::BaseAhead {
                current_version: version,
            });
        }
        Ok(())
    }

    async fn write_definitions_in(
        &self,
        tx: &Transaction<'_>,
        actor: &Actor,
        defs: &DefinitionSet,
    ) -> Result<(WriteReceipt, HashMap<AttributeId, usize>), Error> {
        self.ensure_actor(tx, actor).await?;
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
        let batch = insert_batch(tx, actor).await?;
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
        let seq = insert_change(tx, batch, None, None, ChangeOp::Definitions, actor, &diff).await?;
        let groups = sync_attribute_parts(tx, defs).await?;
        crate::links::sync_cardinality(tx, defs).await?;
        Ok((
            WriteReceipt {
                batch_id: batch,
                last_seq: seq,
                versions: Vec::new(),
            },
            groups,
        ))
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
    /// The interactive lane commits the slice in one transaction. The bulk
    /// lane pauses first, and while interactive requests are still running it
    /// commits several smaller transactions. Each of those is atomic. A later
    /// slice can fail after an earlier one has committed.
    pub async fn write_ops(
        &self,
        batch: BatchId,
        actor: &Actor,
        mut ops: Vec<ObjectOp>,
    ) -> Result<WriteReceipt, Error> {
        if ops.is_empty() {
            return Ok(WriteReceipt {
                batch_id: batch,
                last_seq: 0,
                versions: Vec::new(),
            });
        }
        self.yield_between_bulk_batches().await;
        let defs = self.defs().await?;
        let mut versions = Vec::with_capacity(ops.len());
        loop {
            let take = self.bulk_take(ops.len());
            let chunk: Vec<_> = ops.drain(..take).collect();
            let receipt = self.write_ops_tx(batch, actor, &defs, chunk).await?;
            let last_seq = receipt.last_seq;
            versions.extend(receipt.versions);
            if ops.is_empty() {
                return Ok(WriteReceipt {
                    batch_id: batch,
                    last_seq,
                    versions,
                });
            }
            if self.lane() == DbLane::Bulk {
                tokio::task::yield_now().await;
            }
            self.yield_between_bulk_batches().await;
        }
    }

    async fn write_ops_tx(
        &self,
        batch: BatchId,
        actor: &Actor,
        defs: &CachedDefs,
        ops: Vec<ObjectOp>,
    ) -> Result<WriteReceipt, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        if self.lane() == DbLane::Bulk {
            // The batch is already open. The slice commits or rolls back
            // together, and it does not wait for its own WAL flush.
            tx.batch_execute("SET LOCAL synchronous_commit = off")
                .await?;
        }
        let receipt = apply_ops(&tx, self.form(), defs, batch, actor, &ops).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// Imports `objects` into an open batch.
    ///
    /// One transaction when the lane is idle. Under interactive load the bulk
    /// lane pauses, then commits a smaller slice and repeats. A duplicate key
    /// still fails the call; rows from an earlier slice of that call stay
    /// committed.
    pub async fn import_chunk(
        &self,
        batch: BatchId,
        actor: &Actor,
        objects: &[Object],
    ) -> Result<(), Error> {
        if objects.is_empty() {
            return Ok(());
        }
        let mut offset = 0;
        while offset < objects.len() {
            self.yield_between_bulk_batches().await;
            let take = self.bulk_take(objects.len() - offset);
            let end = offset + take;
            self.import_slice(batch, actor, &objects[offset..end])
                .await?;
            offset = end;
        }
        Ok(())
    }

    async fn import_slice(
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
        // A slice belongs to an open batch that the caller can resume, so it
        // does not need to wait for its own WAL flush. The slice is atomic.
        // A crash can lose a slice that was never flushed, not half of one.
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
                        "SELECT object_id, part, \"values\"
                         FROM object_values
                         WHERE object_id = ANY($1) AND is_latest AND part = ANY($2)",
                        &[&uuids, parts],
                    )
                    .await
            } else {
                client
                    .query(
                        "SELECT object_id, part, \"values\"
                         FROM object_values
                         WHERE object_id = ANY($1) AND is_latest",
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

    /// Loads every live object of `type_id` whose key is in `keys`.
    ///
    /// One query finds the ids; the objects are loaded with [`Store::get_objects`].
    /// Keys that are missing are omitted. Duplicate keys in `keys` collapse to one row.
    pub async fn get_by_keys(
        &self,
        type_id: TypeId,
        keys: &[String],
    ) -> Result<HashMap<String, StoredObject>, Error> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, key FROM objects
                 WHERE type_id = $1 AND key = ANY($2) AND deleted_at IS NULL",
                &[&type_id.as_uuid(), &keys],
            )
            .await?;
        let mut id_of = HashMap::with_capacity(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let key: String = row.get("key");
            if id_of.insert(id, key).is_none() {
                ids.push(ObjectId::from_uuid(id));
            }
        }
        drop(client);
        let stored = self.get_objects(&ids).await?;
        let mut found = HashMap::with_capacity(stored.len());
        for object in stored {
            if let Some(key) = id_of.remove(&object.object.id.as_uuid()) {
                found.insert(key, object);
            }
        }
        Ok(found)
    }

    /// Raw part documents for one object. A definition change must not move a value between parts.
    pub async fn stored_parts(&self, id: ObjectId) -> Result<BTreeMap<String, Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT part, \"values\" FROM object_values
                 WHERE object_id = $1 AND is_latest",
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

    /// Moves live objects from `from` onto `to` in one structure and writes a change per object.
    ///
    /// The indexer reloads the object from Postgres, so the search document follows the new group.
    pub async fn reassign_members(
        &self,
        actor: &Actor,
        structure: StructureId,
        from: &[GroupId],
        to: GroupId,
    ) -> Result<u64, Error> {
        if from.is_empty() {
            return Ok(0);
        }
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        self.ensure_actor(&tx, actor).await?;
        let batch = insert_batch(&tx, actor).await?;
        let from_ids: Vec<Uuid> = from.iter().map(|id| id.as_uuid()).collect();
        let rows = tx
            .query(
                "SELECT o.id, s.group_id
                 FROM objects o
                 JOIN object_structures s ON s.object_id = o.id
                 WHERE o.deleted_at IS NULL AND s.structure_id = $1 AND s.group_id = ANY($2)
                 FOR UPDATE OF o",
                &[&structure.as_uuid(), &from_ids],
            )
            .await?;
        let mut moved = 0u64;
        for row in &rows {
            let object_id: Uuid = row.get(0);
            let before: Uuid = row.get(1);
            tx.execute(
                "UPDATE object_structures SET group_id = $3 WHERE object_id = $1 AND structure_id = $2",
                &[&object_id, &structure.as_uuid(), &to.as_uuid()],
            )
            .await?;
            let version_row = tx
                .query_one(
                    "UPDATE objects SET version = version + 1, updated_at = now(), updated_by = $2 WHERE id = $1 RETURNING version",
                    &[&object_id, &actor.actor.as_uuid()],
                )
                .await?;
            let version: i64 = version_row.get(0);
            let diff = serde_json::json!({
                "$groups": {
                    structure.as_uuid().to_string(): {
                        "before": before.to_string(),
                        "after": to.as_uuid().to_string(),
                    }
                }
            });
            insert_change(
                &tx,
                batch,
                Some(ObjectId::from_uuid(object_id)),
                Some(version),
                ChangeOp::Update,
                actor,
                &diff,
            )
            .await?;
            moved += 1;
        }
        tx.commit().await?;
        Ok(moved)
    }

    /// The newest definition revisions, latest first. Used to undo a model save.
    pub async fn recent_definitions(
        &self,
        limit: i64,
    ) -> Result<Vec<braian_model::DefinitionSet>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT definitions FROM definition_revisions ORDER BY model_revision DESC LIMIT $1",
                &[&limit],
            )
            .await?;
        let mut sets = Vec::with_capacity(rows.len());
        for row in rows {
            let document: serde_json::Value = row.get(0);
            sets.push(
                serde_json::from_value(document)
                    .map_err(|error| Error::Definitions(error.to_string()))?,
            );
        }
        Ok(sets)
    }

    /// Source recorded on the batch that wrote a definition revision.
    pub async fn definition_batch_source(&self, revision: u64) -> Result<Option<String>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT batches.source
                 FROM definition_revisions
                 JOIN batches ON batches.id = definition_revisions.batch_id
                 WHERE definition_revisions.model_revision = $1",
                &[&as_i64(revision)?],
            )
            .await?;
        Ok(row.map(|row| row.get(0)))
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

    /// Highest change sequence for one object. This does not load the diff.
    pub async fn latest_seq(&self, id: ObjectId) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT seq FROM changes WHERE object_id = $1 ORDER BY seq DESC LIMIT 1",
                &[&id.as_uuid()],
            )
            .await?;
        Ok(row.map(|row| row.get(0)).unwrap_or(0))
    }

    /// Highest change sequence for each id that has one. Missing ids are omitted.
    pub async fn latest_seqs(&self, ids: &[ObjectId]) -> Result<HashMap<ObjectId, i64>, Error> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT DISTINCT ON (object_id) object_id, seq
                 FROM changes
                 WHERE object_id = ANY($1)
                 ORDER BY object_id, seq DESC",
                &[&uuids],
            )
            .await?;
        let mut seqs = HashMap::with_capacity(rows.len());
        for row in &rows {
            seqs.insert(ObjectId::from_uuid(row.get("object_id")), row.get("seq"));
        }
        Ok(seqs)
    }

    /// Parts that store `ids`. `languages` limits localized attributes.
    pub async fn parts_for_attributes(
        &self,
        ids: &[braian_model::AttributeId],
        languages: Option<&[String]>,
    ) -> Result<Vec<String>, Error> {
        let defs = self.defs().await?;
        Ok(defs.split.parts_covering(self.form(), ids, languages))
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
        while !buffer.is_empty() {
            self.yield_between_bulk_batches().await;
            let take = self.bulk_take(buffer.len());
            let slice: Vec<_> = buffer.drain(..take).collect();
            match self.import_slice(batch, actor, &slice).await {
                Ok(()) => *written += slice.len() as u64,
                Err(error) if error.is_duplicate() && slice.len() > 1 => {
                    for object in &slice {
                        match self
                            .import_slice(batch, actor, std::slice::from_ref(object))
                            .await
                        {
                            Ok(()) => *written += 1,
                            Err(error) if error.is_duplicate() => {
                                errors.push(row_error(object, &error));
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
                Err(error) if error.is_duplicate() => {
                    errors.push(row_error(&slice[0], &error));
                }
                Err(error) => return Err(error),
            }
        }
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
                "SELECT part, \"values\" FROM object_values
                 WHERE object_id = $1 AND is_latest",
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
            key: row.get::<_, Option<String>>("key").unwrap_or_default(),
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
            key: row.get::<_, Option<String>>("key").unwrap_or_default(),
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
            ObjectOp::CreateLink {
                object,
                from_id,
                to_id,
            } => {
                let (version, seq) =
                    insert_link(tx, form, defs, batch, actor, object, *from_id, *to_id).await?;
                (object.id, version, seq)
            }
            ObjectOp::UpdateLink {
                id,
                patch,
                base_version,
                read_version,
                from_id,
                to_id,
            } => {
                let (version, seq) = update_link(
                    tx,
                    form,
                    defs,
                    batch,
                    actor,
                    *id,
                    patch,
                    *base_version,
                    *read_version,
                    *from_id,
                    *to_id,
                )
                .await?;
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
    insert_head(tx, actor, object, Some(key)).await?;
    let parts = defs.split.split(form, &object.values);
    for (part, document) in &parts {
        let value = Value::Object(document.clone());
        insert_part_version(tx, object.id.as_uuid(), part, 1, &value).await?;
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

async fn insert_head(
    tx: &Transaction<'_>,
    actor: &Actor,
    object: &Object,
    key: Option<String>,
) -> Result<(), Error> {
    let revision = as_i64(object.model_revision)?;
    tx.execute(
        "INSERT INTO objects (id, type_id, key, version, model_revision, updated_by) VALUES ($1, $2, $3, 1, $4, $5)",
        &[
            &object.id.as_uuid(),
            &object.type_id.as_uuid(),
            &key,
            &revision,
            &actor.actor.as_uuid(),
        ],
    )
    .await
    .map_err(|error| map_unique(error, object.type_id, key.as_deref().unwrap_or("")))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_link(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    object: &Object,
    from_id: ObjectId,
    to_id: ObjectId,
) -> Result<(u64, i64), Error> {
    let link_type = defs
        .set
        .types
        .iter()
        .find(|item| item.id == object.type_id)
        .ok_or(Error::UnknownType(object.type_id))?;
    if !link_type.is_link() {
        return Err(Error::LinkRejected(format!(
            "{} is not a link type",
            link_type.key
        )));
    }
    let spec = link_type
        .link
        .as_ref()
        .ok_or_else(|| Error::LinkRejected(format!("link type {} has no ends", link_type.key)))?;
    if from_id == to_id {
        return Err(Error::LinkRejected(
            "a link cannot point at itself".to_owned(),
        ));
    }
    let from_type = crate::links::endpoint_type(tx, from_id).await?;
    let to_type = crate::links::endpoint_type(tx, to_id).await?;
    let from_key = type_key(&defs.set, from_type)?;
    let to_key = type_key(&defs.set, to_type)?;
    let (from_count, to_count) =
        crate::links::existing_counts(tx, link_type.id, from_id, to_id, None).await?;
    braian_model::explain_link(
        &defs.set,
        &link_type.key,
        &from_key,
        &to_key,
        from_count,
        to_count,
    )
    .map_err(Error::LinkRejected)?;
    insert_head(tx, actor, object, None).await?;
    let parts = defs.split.split(form, &object.values);
    for (part, document) in &parts {
        let value = Value::Object(document.clone());
        insert_part_version(tx, object.id.as_uuid(), part, 1, &value).await?;
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
    let from_one = spec.from_cardinality == braian_model::Cardinality::One;
    let to_one = spec.to_cardinality == braian_model::Cardinality::One;
    tx.execute(
        "INSERT INTO links (object_id, link_type_id, from_id, to_id, from_one, to_one) VALUES ($1, $2, $3, $4, $5, $6)",
        &[
            &object.id.as_uuid(),
            &link_type.id.as_uuid(),
            &from_id.as_uuid(),
            &to_id.as_uuid(),
            &from_one,
            &to_one,
        ],
    )
    .await
    .map_err(|error| crate::links::map_cardinality(error, &link_type.key))?;
    Ok((1, seq))
}

#[allow(clippy::too_many_arguments)]
async fn update_link(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    defs: &CachedDefs,
    batch: BatchId,
    actor: &Actor,
    id: ObjectId,
    patch: &braian_model::ObjectPatch,
    base_version: u64,
    read_version: u64,
    from_id: Option<ObjectId>,
    to_id: Option<ObjectId>,
) -> Result<(u64, i64), Error> {
    let (version, seq) = update_object(
        tx,
        form,
        defs,
        batch,
        actor,
        id,
        patch,
        base_version,
        read_version,
    )
    .await?;
    if from_id.is_none() && to_id.is_none() {
        return Ok((version, seq));
    }
    let row = tx
        .query_opt(
            "SELECT link_type_id, from_id, to_id FROM links WHERE object_id = $1",
            &[&id.as_uuid()],
        )
        .await?;
    let Some(row) = row else {
        return Err(Error::LinkRejected(format!("{id} is not a link")));
    };
    let link_type_id = TypeId::from_uuid(row.get("link_type_id"));
    let current_from = ObjectId::from_uuid(row.get("from_id"));
    let current_to = ObjectId::from_uuid(row.get("to_id"));
    let next_from = from_id.unwrap_or(current_from);
    let next_to = to_id.unwrap_or(current_to);
    if next_from == current_from && next_to == current_to {
        return Ok((version, seq));
    }
    let link_type = defs
        .set
        .types
        .iter()
        .find(|item| item.id == link_type_id)
        .ok_or(Error::UnknownType(link_type_id))?;
    let spec = link_type
        .link
        .as_ref()
        .ok_or_else(|| Error::LinkRejected(format!("link type {} has no ends", link_type.key)))?;
    if next_from == next_to {
        return Err(Error::LinkRejected(
            "a link cannot point at itself".to_owned(),
        ));
    }
    let from_type = crate::links::endpoint_type(tx, next_from).await?;
    let to_type = crate::links::endpoint_type(tx, next_to).await?;
    let (from_count, to_count) =
        crate::links::existing_counts(tx, link_type.id, next_from, next_to, Some(id)).await?;
    braian_model::explain_link(
        &defs.set,
        &link_type.key,
        &type_key(&defs.set, from_type)?,
        &type_key(&defs.set, to_type)?,
        from_count,
        to_count,
    )
    .map_err(Error::LinkRejected)?;
    let from_one = spec.from_cardinality == braian_model::Cardinality::One;
    let to_one = spec.to_cardinality == braian_model::Cardinality::One;
    tx.execute(
        "UPDATE links SET from_id = $2, to_id = $3, from_one = $4, to_one = $5 WHERE object_id = $1",
        &[
            &id.as_uuid(),
            &next_from.as_uuid(),
            &next_to.as_uuid(),
            &from_one,
            &to_one,
        ],
    )
    .await
    .map_err(|error| crate::links::map_cardinality(error, &link_type.key))?;
    Ok((version, seq))
}

async fn cascade_links(
    tx: &Transaction<'_>,
    batch: BatchId,
    actor: &Actor,
    id: ObjectId,
) -> Result<(), Error> {
    let ids = crate::links::touching_ids(tx, id).await?;
    if ids.is_empty() {
        return Ok(());
    }
    crate::links::delete_rows(tx, &ids).await?;
    for link_id in ids {
        if link_id == id.as_uuid() {
            continue;
        }
        let row = tx
            .query_opt(
                "SELECT version, deleted_at FROM objects WHERE id = $1 FOR UPDATE",
                &[&link_id],
            )
            .await?;
        let Some(row) = row else {
            continue;
        };
        let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row.get("deleted_at");
        if deleted_at.is_some() {
            continue;
        }
        let version_row = tx
            .query_one(
                "UPDATE objects SET version = version + 1, updated_at = now(), deleted_at = now(), updated_by = $2 WHERE id = $1 RETURNING version",
                &[&link_id, &actor.actor.as_uuid()],
            )
            .await?;
        let version = u64_from_i64(version_row.get(0))?;
        insert_change(
            tx,
            batch,
            Some(ObjectId::from_uuid(link_id)),
            Some(as_i64(version)?),
            ChangeOp::Delete,
            actor,
            &serde_json::json!({ "link_removed": true }),
        )
        .await?;
    }
    Ok(())
}

fn type_key(defs: &braian_model::DefinitionSet, id: TypeId) -> Result<String, Error> {
    defs.types
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.key.clone())
        .ok_or(Error::UnknownType(id))
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
        insert_part_version(tx, id.as_uuid(), part, version_i64, &empty).await?;
    }
    for (part, document) in &parts {
        if before.get(part) == Some(document) {
            continue;
        }
        let value = Value::Object(document.clone());
        insert_part_version(tx, id.as_uuid(), part, version_i64, &value).await?;
    }
    let placed = apply_groups(tx, id, patch).await?;
    let revision = as_i64(defs.set.model_revision)?;
    let object_type = defs
        .set
        .types
        .iter()
        .find(|object_type| object_type.id == type_id);
    let is_link = object_type.is_some_and(|object_type| object_type.is_link());
    let key_attribute = object_type.map(|object_type| object_type.key_attribute);
    let version_row = if !is_link
        && key_attribute.is_some_and(|attribute| patch.values.attributes.contains_key(&attribute))
    {
        let key = new_key(id, patch, key_attribute.expect("checked above"))?;
        tx.query_one(
            "UPDATE objects SET version = version + 1, updated_at = now(), model_revision = $2, key = $3, updated_by = $4 WHERE id = $1 RETURNING version",
            &[&id.as_uuid(), &revision, &key, &actor.actor.as_uuid()],
        )
        .await
        .map_err(|error| map_unique(error, type_id, &key))?
    } else {
        tx.query_one(
            "UPDATE objects SET version = version + 1, updated_at = now(), model_revision = $2, updated_by = $3 WHERE id = $1 RETURNING version",
            &[&id.as_uuid(), &revision, &actor.actor.as_uuid()],
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
    cascade_links(tx, batch, actor, id).await?;
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
            "UPDATE objects SET version = version + 1, updated_at = now(), deleted_at = now(), updated_by = $2 WHERE id = $1 RETURNING version",
            &[&id.as_uuid(), &actor.actor.as_uuid()],
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

pub(crate) async fn insert_change(
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

async fn insert_part_version(
    tx: &Transaction<'_>,
    id: Uuid,
    part: &str,
    version: i64,
    value: &Value,
) -> Result<(), Error> {
    if version > 1 {
        tx.execute(
            "UPDATE object_values SET is_latest = false
             WHERE object_id = $1 AND part = $2 AND is_latest",
            &[&id, &part],
        )
        .await?;
    }
    tx.execute(
        "INSERT INTO object_values (object_id, part, version, \"values\", is_latest)
         VALUES ($1, $2, $3, $4, true)",
        &[&id, &part, &version, value],
    )
    .await?;
    Ok(())
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
                 WHERE v.object_id = $1 AND v.is_latest AND v.part = ANY($2)
                 FOR UPDATE OF v",
                &[&id.as_uuid(), &stale.destinations],
            )
            .await?
        }
        crate::split::StorageForm::PerLanguage => {
            tx.query(
                "SELECT v.part, v.\"values\"
                 FROM object_values v
                 WHERE v.object_id = $1 AND v.is_latest AND (
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
                 WHERE v.object_id = $1 AND v.is_latest AND (
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

async fn rewrite_standard_values(
    tx: &Transaction<'_>,
    form: crate::split::StorageForm,
    context: &split::SplitContext,
    defs: &DefinitionSet,
) -> Result<(), Error> {
    let rows = tx
        .query(
            "SELECT object.id, object.key, value.part, value.\"values\"
             FROM objects object
             JOIN object_values value ON value.object_id = object.id AND value.is_latest
             WHERE object.deleted_at IS NULL
             ORDER BY object.id",
            &[],
        )
        .await?;
    struct Bucket {
        key: String,
        parts: BTreeMap<String, Value>,
    }
    let mut objects: BTreeMap<Uuid, Bucket> = BTreeMap::new();
    for row in &rows {
        let id: Uuid = row.get(0);
        let entry = objects.entry(id).or_insert_with(|| Bucket {
            key: row.get(1),
            parts: BTreeMap::new(),
        });
        entry.parts.insert(row.get(2), row.get(3));
    }
    for (id, bucket) in objects {
        let mut values = split::join_parts(form, &bucket.parts)
            .map_err(|message| Error::Corrupt(ObjectId::from_uuid(id), message))?;
        let before = values.clone();
        braian_model::move_values(defs, &bucket.key, &mut values);
        if values == before {
            continue;
        }
        let next = context.split(form, &values);
        for (part, document) in &next {
            let value = Value::Object(document.clone());
            let updated = tx
                .execute(
                    "UPDATE object_values SET \"values\" = $3
                     WHERE object_id = $1 AND part = $2 AND is_latest",
                    &[&id, part, &value],
                )
                .await?;
            if updated == 0 {
                tx.execute(
                    "INSERT INTO object_values (object_id, part, version, \"values\", is_latest)
                     VALUES ($1, $2, 1, $3, true)",
                    &[&id, part, &value],
                )
                .await?;
            }
        }
        for part in bucket.parts.keys() {
            if next.contains_key(part) {
                continue;
            }
            let empty = Value::Object(Map::new());
            tx.execute(
                "UPDATE object_values SET \"values\" = $3
                 WHERE object_id = $1 AND part = $2 AND is_latest",
                &[&id, part, &empty],
            )
            .await?;
        }
        if let Some(number) = values
            .get(&braian_model::number_id())
            .and_then(Value::as_str)
            && number != bucket.key
        {
            tx.execute(
                "UPDATE objects SET key = $2 WHERE id = $1 AND deleted_at IS NULL",
                &[&id, &number],
            )
            .await?;
        }
    }
    Ok(())
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

fn collect_uuids(value: &Value, out: &mut Vec<Uuid>) {
    match value {
        Value::String(text) => {
            if let Ok(id) = Uuid::parse_str(text.trim()) {
                out.push(id);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_uuids(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_uuids(item, out);
            }
        }
        _ => {}
    }
}

fn ref_link_id(attribute: Uuid, from_id: Uuid, to_id: Uuid) -> Uuid {
    let label = format!("object-ref-link:{attribute}:{from_id}:{to_id}");
    let mut hash = 0xcbf29ce484222325u128;
    for byte in label.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut bytes = hash.to_le_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn as_i64(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Data(format!("{value} does not fit in bigint")))
}

fn u64_from_i64(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Data(format!("negative stored integer {value}")))
}
