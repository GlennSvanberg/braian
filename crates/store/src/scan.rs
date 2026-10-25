//! Chunked reads for export. One chunk is checked out, assembled, then dropped.
//!
//! The caller picks the lane: [`Store::on_bulk`] for a background export, the
//! interactive store for a download the person is waiting on. A cursor keeps
//! the snapshot stable. [`Store::yield_between_bulk_batches`] runs after each
//! chunk so a bulk export does not pin the runtime.

use std::collections::BTreeMap;
use std::future::Future;

use braian_model::{AttributeId, ObjectId};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Error;
use crate::session::Store;
use crate::split::{self, StorageForm};

/// Rows fetched per round trip. The workbook sizes columns from a sample of
/// about this many rows, so a chunk is also the unit that stays in memory.
pub const EXPORT_CHUNK: usize = 400;

/// How to order an export. `Selection` follows the id list in `$1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOrder {
    KeyAsc,
    KeyDesc,
    VersionAsc,
    VersionDesc,
    Selection,
}

/// Filters pushed into the object query. `None` means that predicate is off.
///
/// `text` is a raw key. `exact_key` compares it with `=`. Otherwise it is the
/// same case-insensitive contains match as `Store::list_objects_page`.
#[derive(Debug, Clone)]
pub struct ExportScan {
    pub ids: Option<Vec<Uuid>>,
    pub type_id: Option<Uuid>,
    pub text: Option<String>,
    pub exact_key: bool,
    pub structure_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub order: ExportOrder,
}

impl Default for ExportScan {
    fn default() -> Self {
        Self {
            ids: None,
            type_id: None,
            text: None,
            exact_key: false,
            structure_id: None,
            group_id: None,
            order: ExportOrder::KeyAsc,
        }
    }
}

/// One object, values joined back from the part rows. The chunk that owns it
/// is the only copy.
#[derive(Debug)]
pub struct ExportObject {
    pub id: Uuid,
    pub key: String,
    pub type_id: Uuid,
    pub version: i64,
    pub values: BTreeMap<AttributeId, Value>,
}

const WHERE: &str = r#"
o.deleted_at IS NULL
AND ($1::uuid[] IS NULL OR o.id = ANY($1))
AND ($2::uuid IS NULL OR o.type_id = $2)
AND (
    $3::text IS NULL
    OR ($4::bool AND o.key = $3)
    OR (NOT $4::bool AND o.key ILIKE $3 ESCAPE E'\\')
)
AND (
    $5::uuid IS NULL
    OR EXISTS (
        SELECT 1 FROM object_structures s
        WHERE s.object_id = o.id
          AND s.group_id = $5
          AND ($6::uuid IS NULL OR s.structure_id = $6)
    )
)
"#;

const PARTS: &str = r#"
SELECT DISTINCT ON (object_id, part) object_id, part, "values"
FROM object_values
WHERE object_id = ANY($1)
ORDER BY object_id, part, version DESC
"#;

fn order_sql(order: ExportOrder) -> &'static str {
    match order {
        ExportOrder::KeyAsc => "o.key ASC, o.id ASC",
        ExportOrder::KeyDesc => "o.key DESC, o.id ASC",
        ExportOrder::VersionAsc => "o.version ASC, o.id ASC",
        ExportOrder::VersionDesc => "o.version DESC, o.id ASC",
        ExportOrder::Selection => "array_position($1, o.id), o.id",
    }
}

impl Store {
    /// How many objects match. Uses this store's lane and returns the connection.
    pub async fn count_export(&self, scan: &ExportScan) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let sql = format!("SELECT count(*) FROM objects o WHERE {WHERE}");
        let pattern = key_param(scan);
        let params = binds(scan, &pattern);
        let row = client.query_one(&sql, &params).await?;
        Ok(row.get(0))
    }

    /// Calls `visit` once per chunk. The chunk is dropped when `visit` returns,
    /// and the cursor connection stays on this store's lane for the whole scan.
    pub async fn for_each_export_chunk<F, Fut>(
        &self,
        scan: &ExportScan,
        mut visit: F,
    ) -> Result<u64, Error>
    where
        F: FnMut(Vec<ExportObject>) -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        let mut client = self.pool().get().await?;
        let tx = client.build_transaction().read_only(true).start().await?;
        let declare = format!(
            "DECLARE export_cur NO SCROLL CURSOR FOR \
             SELECT o.id, o.key, o.type_id, o.version \
             FROM objects o WHERE {WHERE} ORDER BY {order}",
            order = order_sql(scan.order),
        );
        let pattern = key_param(scan);
        let params = binds(scan, &pattern);
        tx.execute(&declare, &params).await?;
        let fetch = format!("FETCH {EXPORT_CHUNK} FROM export_cur");
        let mut total = 0u64;
        loop {
            let heads = tx.query(&fetch, &[]).await?;
            if heads.is_empty() {
                break;
            }
            let ids: Vec<Uuid> = heads.iter().map(|row| row.get(0)).collect();
            let parts = tx.query(PARTS, &[&ids]).await?;
            let chunk = assemble(self.form(), heads, parts)?;
            total += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            visit(chunk).await?;
            self.yield_between_bulk_batches().await;
        }
        tx.commit().await?;
        Ok(total)
    }
}

/// Exact key, or the list-filter contains pattern. `None` leaves the predicate off.
fn key_param(scan: &ExportScan) -> Option<String> {
    let text = scan.text.as_ref()?;
    if scan.exact_key {
        Some(text.clone())
    } else {
        Some(crate::product::like_contains(text))
    }
}

fn binds<'a>(
    scan: &'a ExportScan,
    key: &'a Option<String>,
) -> [&'a (dyn tokio_postgres::types::ToSql + Sync); 6] {
    [
        &scan.ids,
        &scan.type_id,
        key,
        &scan.exact_key,
        &scan.group_id,
        &scan.structure_id,
    ]
}

fn assemble(
    form: StorageForm,
    heads: Vec<tokio_postgres::Row>,
    parts: Vec<tokio_postgres::Row>,
) -> Result<Vec<ExportObject>, Error> {
    let mut by_id: BTreeMap<Uuid, BTreeMap<String, Value>> = BTreeMap::new();
    for row in &parts {
        let id: Uuid = row.get(0);
        let part: String = row.get(1);
        let values: Value = row.get(2);
        by_id.entry(id).or_default().insert(part, values);
    }
    let mut objects = Vec::with_capacity(heads.len());
    for row in heads {
        let id: Uuid = row.get(0);
        let encoded = by_id.remove(&id).unwrap_or_default();
        let values = split::join_parts(form, &encoded)
            .map_err(|message| Error::Corrupt(ObjectId::from_uuid(id), message))?;
        objects.push(ExportObject {
            id,
            key: row.get(1),
            type_id: row.get(2),
            version: row.get(3),
            values,
        });
    }
    Ok(objects)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{EXPORT_CHUNK, ExportScan};
    use crate::session::DbLane;
    use crate::testing::TestDb;

    #[tokio::test]
    async fn export_scan_yields_chunks_on_the_bulk_pool() {
        let db = TestDb::create().await;
        let store = db.store();
        let client = store.pool().get().await.unwrap();
        let type_id = uuid::Uuid::now_v7();
        client
            .batch_execute(&format!(
                "INSERT INTO objects (id, type_id, key, version, model_revision)
                 SELECT (
                     '018f0000-0000-7000-8000-' || lpad(to_hex(g), 12, '0')
                 )::uuid,
                 '{type_id}'::uuid,
                 'row-' || lpad(g::text, 5, '0'),
                 1,
                 1
                 FROM generate_series(1, 1000) AS g"
            ))
            .await
            .unwrap();
        drop(client);

        let bulk = store.on_bulk();
        assert_eq!(bulk.lane(), DbLane::Bulk);
        let scan = ExportScan::default();
        let found = bulk.count_export(&scan).await.unwrap();
        assert_eq!(found, 1000);

        let chunks = Arc::new(AtomicUsize::new(0));
        let max_chunk = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let chunks_flag = Arc::clone(&chunks);
        let max_flag = Arc::clone(&max_chunk);
        let total_flag = Arc::clone(&total);
        bulk.for_each_export_chunk(&scan, move |chunk| {
            let chunks_flag = Arc::clone(&chunks_flag);
            let max_flag = Arc::clone(&max_flag);
            let total_flag = Arc::clone(&total_flag);
            async move {
                chunks_flag.fetch_add(1, Ordering::SeqCst);
                max_flag.fetch_max(chunk.len(), Ordering::SeqCst);
                total_flag.fetch_add(chunk.len(), Ordering::SeqCst);
                assert!(chunk.len() <= EXPORT_CHUNK);
                Ok(())
            }
        })
        .await
        .unwrap();

        assert_eq!(total.load(Ordering::SeqCst), 1000);
        assert!(chunks.load(Ordering::SeqCst) > 1);
        assert!(max_chunk.load(Ordering::SeqCst) <= EXPORT_CHUNK);
        assert!(max_chunk.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn export_key_filter_matches_the_list_contains_filter() {
        let db = TestDb::create().await;
        let store = db.store();
        let client = store.pool().get().await.unwrap();
        let type_id = uuid::Uuid::now_v7();
        client
            .batch_execute(&format!(
                "INSERT INTO objects (id, type_id, key, version, model_revision) VALUES
                 ('018f0000-0000-7000-8000-0000000000a1', '{type_id}', '100%_x', 1, 1),
                 ('018f0000-0000-7000-8000-0000000000a2', '{type_id}', '100abcx', 1, 1),
                 ('018f0000-0000-7000-8000-0000000000a3', '{type_id}', 'other', 1, 1)"
            ))
            .await
            .unwrap();
        drop(client);

        let listed = store
            .list_objects_page(Some(type_id), Some("100%_"), None, 10)
            .await
            .unwrap();
        let scan = ExportScan {
            type_id: Some(type_id),
            text: Some("100%_".into()),
            ..ExportScan::default()
        };
        let found = store.count_export(&scan).await.unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(found, 1);
        assert_eq!(listed[0].1, "100%_x");

        let exact = ExportScan {
            text: Some("100abcx".into()),
            exact_key: true,
            ..ExportScan::default()
        };
        assert_eq!(store.count_export(&exact).await.unwrap(), 1);
    }
}
