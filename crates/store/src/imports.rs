//! Import sessions and the disposable diff rows.

use serde_json::Value;
use tokio_postgres::binary_copy::BinaryCopyInWriter;
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

use crate::error::Error;
use crate::session::Store;

#[derive(Debug, Clone)]
pub struct ImportSession {
    pub id: Uuid,
    pub owner: Uuid,
    pub on_behalf_of: Option<Uuid>,
    pub file_name: String,
    pub file_kind: String,
    pub file_bytes: i64,
    pub status: String,
    pub sheets: Value,
    pub sheet_index: i32,
    pub csv_dialect: Option<Value>,
    pub headers: Value,
    pub sample: Value,
    pub rows_parsed: i64,
    pub parse_done: bool,
    pub model_revision: i64,
    pub proposal: Option<Value>,
    pub mapping: Option<Value>,
    pub counters: Value,
    pub job_id: Option<Uuid>,
    pub error: Option<String>,
    pub decisions_usage: Value,
}

#[derive(Debug, Clone)]
pub struct ImportRowIn {
    pub session_id: Uuid,
    pub row_no: i32,
    pub status: String,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub base_version: Option<i64>,
    pub cells: Value,
    pub patch: Option<Value>,
    pub errors: Value,
}

#[derive(Debug, Clone)]
pub struct ImportRowOut {
    pub row_no: i32,
    pub status: String,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub base_version: Option<i64>,
    pub cells: Value,
    pub patch: Option<Value>,
    pub errors: Value,
}

impl Store {
    pub async fn insert_import_session(
        &self,
        id: Uuid,
        owner: Uuid,
        on_behalf_of: Option<Uuid>,
        file_name: &str,
        file_kind: &str,
        model_revision: i64,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO import_sessions (id, owner, on_behalf_of, file_name, file_kind, status, model_revision)
                 VALUES ($1, $2, $3, $4, $5, 'uploading', $6)",
                &[
                    &id,
                    &owner,
                    &on_behalf_of,
                    &file_name,
                    &file_kind,
                    &model_revision,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn count_active_imports(&self, owner: Uuid) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(*)::bigint FROM import_sessions
                 WHERE owner = $1 AND status NOT IN ('saved', 'failed', 'cancelled')",
                &[&owner],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn import_session(&self, id: Uuid) -> Result<Option<ImportSession>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, owner, on_behalf_of, file_name, file_kind, file_bytes, status,
                        sheets, sheet_index, csv_dialect, headers, sample, rows_parsed, parse_done,
                        model_revision, proposal, mapping, counters, job_id, error, decisions_usage
                 FROM import_sessions WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(ImportSession {
            id: row.get("id"),
            owner: row.get("owner"),
            on_behalf_of: row.get("on_behalf_of"),
            file_name: row.get("file_name"),
            file_kind: row.get("file_kind"),
            file_bytes: row.get("file_bytes"),
            status: row.get("status"),
            sheets: row.get("sheets"),
            sheet_index: row.get("sheet_index"),
            csv_dialect: row.get("csv_dialect"),
            headers: row.get("headers"),
            sample: row.get("sample"),
            rows_parsed: row.get("rows_parsed"),
            parse_done: row.get("parse_done"),
            model_revision: row.get("model_revision"),
            proposal: row.get("proposal"),
            mapping: row.get("mapping"),
            counters: row.get("counters"),
            job_id: row.get("job_id"),
            error: row.get("error"),
            decisions_usage: row.get("decisions_usage"),
        }))
    }

    pub async fn update_import_upload(
        &self,
        id: Uuid,
        file_bytes: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET file_bytes = $2, status = $3, error = $4, updated_at = now()
                 WHERE id = $1",
                &[&id, &file_bytes, &status, &error],
            )
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_import_parsed(
        &self,
        id: Uuid,
        sheets: &Value,
        sheet_index: i32,
        csv_dialect: Option<&Value>,
        headers: &Value,
        sample: &Value,
        rows_parsed: i64,
        parse_done: bool,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET sheets = $2, sheet_index = $3, csv_dialect = $4, headers = $5, sample = $6,
                     rows_parsed = $7, parse_done = $8, status = $9, error = $10, updated_at = now()
                 WHERE id = $1",
                &[
                    &id,
                    sheets,
                    &sheet_index,
                    &csv_dialect,
                    headers,
                    sample,
                    &rows_parsed,
                    &parse_done,
                    &status,
                    &error,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn update_import_progress(
        &self,
        id: Uuid,
        rows_parsed: i64,
        parse_done: bool,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET rows_parsed = $2, parse_done = $3, updated_at = now()
                 WHERE id = $1",
                &[&id, &rows_parsed, &parse_done],
            )
            .await?;
        Ok(())
    }

    pub async fn update_import_proposal(
        &self,
        id: Uuid,
        proposal: &Value,
        usage: &Value,
        status: &str,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET proposal = $2, decisions_usage = $3, status = $4, updated_at = now()
                 WHERE id = $1",
                &[&id, proposal, usage, &status],
            )
            .await?;
        Ok(())
    }

    pub async fn update_import_mapping(&self, id: Uuid, mapping: &Value) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions SET mapping = $2, updated_at = now() WHERE id = $1",
                &[&id, mapping],
            )
            .await?;
        Ok(())
    }

    pub async fn update_import_diff(
        &self,
        id: Uuid,
        status: &str,
        counters: &Value,
        error: Option<&str>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET status = $2, counters = $3, error = $4, updated_at = now()
                 WHERE id = $1",
                &[&id, &status, counters, &error],
            )
            .await?;
        Ok(())
    }

    /// `true` when a diff or preview is already using this exact mapping.
    pub async fn import_mapping_in_progress(
        &self,
        id: Uuid,
        mapping: &Value,
    ) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT (status IN ('diffing', 'previewed') AND diff_mapping = $2)
                 FROM import_sessions WHERE id = $1",
                &[&id, mapping],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn import_diff_current(&self, id: Uuid, generation: i64) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT diff_generation = $2 FROM import_sessions WHERE id = $1",
                &[&id, &generation],
            )
            .await?
        else {
            return Ok(false);
        };
        Ok(row.get(0))
    }

    /// Starts a diff for the mapping stored on the session.
    ///
    /// The generation bumps in the same transaction that clears preview rows, so
    /// a worker still writing the previous mapping cannot append after this.
    pub async fn restart_import_diff(&self, id: Uuid) -> Result<i64, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let counters = empty_import_counters();
        let row = tx
            .query_one(
                "UPDATE import_sessions
                 SET diff_generation = diff_generation + 1,
                     diff_mapping = mapping,
                     status = 'diffing',
                     counters = $2,
                     error = NULL,
                     updated_at = now()
                 WHERE id = $1
                 RETURNING diff_generation",
                &[&id, &counters],
            )
            .await?;
        tx.execute("DELETE FROM import_rows WHERE session_id = $1", &[&id])
            .await?;
        let generation: i64 = row.get(0);
        tx.commit().await?;
        Ok(generation)
    }

    pub async fn update_import_diff_if_current(
        &self,
        id: Uuid,
        generation: i64,
        status: &str,
        counters: &Value,
        error: Option<&str>,
    ) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let updated = client
            .execute(
                "UPDATE import_sessions
                 SET status = $3, counters = $4, error = $5, updated_at = now()
                 WHERE id = $1 AND diff_generation = $2",
                &[&id, &generation, &status, counters, &error],
            )
            .await?;
        Ok(updated == 1)
    }

    pub async fn fail_import_diff_if_current(
        &self,
        id: Uuid,
        generation: i64,
        error: &str,
    ) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let updated = client
            .execute(
                "UPDATE import_sessions
                 SET status = 'failed', error = $3, updated_at = now()
                 WHERE id = $1 AND diff_generation = $2 AND status = 'diffing'",
                &[&id, &generation, &error],
            )
            .await?;
        Ok(updated == 1)
    }

    pub async fn update_import_job(
        &self,
        id: Uuid,
        status: &str,
        job_id: Option<Uuid>,
        counters: &Value,
        error: Option<&str>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_sessions
                 SET status = $2, job_id = $3, counters = $4, error = $5, updated_at = now()
                 WHERE id = $1",
                &[&id, &status, &job_id, counters, &error],
            )
            .await?;
        Ok(())
    }

    pub async fn delete_import_session(&self, id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM import_sessions WHERE id = $1", &[&id])
            .await?;
        Ok(())
    }

    /// Sessions older than 24h that are not saving, and saved sessions an hour after they finished.
    pub async fn purge_import_sessions(&self) -> Result<Vec<Uuid>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "DELETE FROM import_sessions
                 WHERE (status <> 'saving' AND status <> 'saved' AND created_at < now() - interval '24 hours')
                    OR (status = 'saved' AND updated_at < now() - interval '1 hour')
                 RETURNING id",
                &[],
            )
            .await?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    pub async fn mark_import_row(
        &self,
        session_id: Uuid,
        row_no: i32,
        status: &str,
        errors: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE import_rows SET status = $3, errors = $4
                 WHERE session_id = $1 AND row_no = $2",
                &[&session_id, &row_no, &status, errors],
            )
            .await?;
        Ok(())
    }

    pub async fn delete_import_rows(&self, session_id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "DELETE FROM import_rows WHERE session_id = $1",
                &[&session_id],
            )
            .await?;
        Ok(())
    }

    /// Copies preview rows only while `generation` is still the session's diff.
    ///
    /// `false` means a newer approve replaced this diff. The caller stops
    /// without marking the session failed.
    pub async fn copy_import_rows(
        &self,
        session_id: Uuid,
        generation: i64,
        rows: &[ImportRowIn],
    ) -> Result<bool, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let Some(row) = tx
            .query_opt(
                "SELECT diff_generation FROM import_sessions WHERE id = $1 FOR UPDATE",
                &[&session_id],
            )
            .await?
        else {
            tx.rollback().await?;
            return Ok(false);
        };
        let current: i64 = row.get(0);
        if current != generation {
            tx.rollback().await?;
            return Ok(false);
        }
        if rows.is_empty() {
            tx.commit().await?;
            return Ok(true);
        }
        let sink = tx
            .copy_in(
                "COPY import_rows (
                    session_id, row_no, status, object_id, object_key, base_version, cells, patch, errors
                 ) FROM STDIN WITH (FORMAT binary)",
            )
            .await?;
        let mut writer = std::pin::pin!(BinaryCopyInWriter::new(
            sink,
            &[
                Type::UUID,
                Type::INT4,
                Type::TEXT,
                Type::UUID,
                Type::TEXT,
                Type::INT8,
                Type::JSONB,
                Type::JSONB,
                Type::JSONB,
            ],
        ));
        for row in rows {
            writer
                .as_mut()
                .write(&[
                    &row.session_id as &(dyn ToSql + Sync),
                    &row.row_no,
                    &row.status,
                    &row.object_id,
                    &row.object_key,
                    &row.base_version,
                    &row.cells,
                    &row.patch,
                    &row.errors,
                ])
                .await?;
        }
        writer.finish().await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn count_import_rows(
        &self,
        session_id: Uuid,
        status: Option<&str>,
    ) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(*)::bigint FROM import_rows
                 WHERE session_id = $1 AND ($2::text IS NULL OR status = $2)",
                &[&session_id, &status],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn list_import_rows(
        &self,
        session_id: Uuid,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<ImportRowOut>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT row_no, status, object_id, object_key, base_version, cells, patch, errors
                 FROM import_rows
                 WHERE session_id = $1 AND ($2::text IS NULL OR status = $2)
                 ORDER BY row_no
                 OFFSET $3 LIMIT $4",
                &[&session_id, &status, &offset, &limit],
            )
            .await?;
        Ok(rows.iter().map(row_out).collect())
    }

    pub async fn scan_import_rows(
        &self,
        session_id: Uuid,
        status: &str,
        after_row: i32,
        limit: i64,
    ) -> Result<Vec<ImportRowOut>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT row_no, status, object_id, object_key, base_version, cells, patch, errors
                 FROM import_rows
                 WHERE session_id = $1 AND status = $2 AND row_no > $3
                 ORDER BY row_no
                 LIMIT $4",
                &[&session_id, &status, &after_row, &limit],
            )
            .await?;
        Ok(rows.iter().map(row_out).collect())
    }
}

fn empty_import_counters() -> Value {
    serde_json::json!({
        "new": 0,
        "changed": 0,
        "unchanged": 0,
        "failed": 0,
        "delete": 0,
        "processed": 0,
        "total": 0,
    })
}

fn row_out(row: &tokio_postgres::Row) -> ImportRowOut {
    ImportRowOut {
        row_no: row.get("row_no"),
        status: row.get("status"),
        object_id: row.get("object_id"),
        object_key: row.get("object_key"),
        base_version: row.get("base_version"),
        cells: row.get("cells"),
        patch: row.get("patch"),
        errors: row.get("errors"),
    }
}
