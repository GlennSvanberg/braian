//! Tables that sit on the log: people, workspaces, jobs, proposals, relations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Error;
use crate::ids::{PrincipalId, PrincipalKind};
use crate::session::Store;
use braian_model::ObjectId;

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: Uuid,
    pub principal_id: PrincipalId,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub batch_id: Uuid,
    pub object_id: Option<Uuid>,
    pub version: Option<i64>,
    pub op: String,
    pub source: String,
    pub kind: String,
    pub actor: Uuid,
    pub on_behalf_of: Option<Uuid>,
    pub actor_handle: Option<String>,
    pub behalf_handle: Option<String>,
    pub diff: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ProposalRow {
    pub id: Uuid,
    pub object_id: Option<Uuid>,
    pub object_key: Option<String>,
    pub type_key: Option<String>,
    pub base_version: Option<i64>,
    pub patch: Value,
    pub status: String,
    pub conflict: bool,
    pub position: i32,
}

impl Store {
    pub async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        role: &str,
        display_name: &str,
    ) -> Result<UserRecord, Error> {
        let principal = self
            .upsert_principal(email, PrincipalKind::Human, display_name)
            .await?;
        let id = Uuid::now_v7();
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO users (id, principal_id, email, password_hash, role, display_name)
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[
                    &id,
                    &principal.as_uuid(),
                    &email,
                    &password_hash,
                    &role,
                    &display_name,
                ],
            )
            .await?;
        Ok(UserRecord {
            id,
            principal_id: principal,
            email: email.to_owned(),
            password_hash: password_hash.to_owned(),
            role: role.to_owned(),
            display_name: display_name.to_owned(),
        })
    }

    pub async fn user_by_email(&self, email: &str) -> Result<Option<UserRecord>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, principal_id, email, password_hash, role, display_name FROM users WHERE email = $1",
                &[&email],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(user_from_row(&row)))
    }

    pub async fn user_by_id(&self, id: Uuid) -> Result<Option<UserRecord>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, principal_id, email, password_hash, role, display_name FROM users WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(user_from_row(&row)))
    }

    pub async fn create_session(
        &self,
        user_id: Uuid,
        token_hash: &[u8],
        expires_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
                &[&token_hash, &user_id, &expires_at],
            )
            .await?;
        Ok(())
    }

    pub async fn user_for_session(&self, token_hash: &[u8]) -> Result<Option<UserRecord>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT u.id, u.principal_id, u.email, u.password_hash, u.role, u.display_name
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = $1 AND s.expires_at > now()",
                &[&token_hash],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(user_from_row(&row)))
    }

    pub async fn delete_session(&self, token_hash: &[u8]) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM sessions WHERE token_hash = $1", &[&token_hash])
            .await?;
        Ok(())
    }

    pub async fn insert_api_token(
        &self,
        owner: &UserRecord,
        name: &str,
        kind: &str,
        token_hash: &[u8],
    ) -> Result<PrincipalId, Error> {
        let principal_kind = if kind == "agent" {
            PrincipalKind::Agent
        } else {
            PrincipalKind::Integration
        };
        let principal = self
            .upsert_principal(
                &format!("{kind}:{name}:{}", owner.email),
                principal_kind,
                name,
            )
            .await?;
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO api_tokens (token_hash, principal_id, owner_user_id, name, kind)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&token_hash, &principal.as_uuid(), &owner.id, &name, &kind],
            )
            .await?;
        Ok(principal)
    }

    pub async fn api_token(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<(PrincipalId, UserRecord, String)>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT t.principal_id, t.kind, u.id, u.principal_id, u.email, u.password_hash, u.role, u.display_name
                 FROM api_tokens t JOIN users u ON u.id = t.owner_user_id
                 WHERE t.token_hash = $1",
                &[&token_hash],
            )
            .await?
        else {
            return Ok(None);
        };
        let principal = PrincipalId::from_uuid(row.get(0));
        let kind: String = row.get(1);
        Ok(Some((principal, user_from_row_at(&row, 2), kind)))
    }

    pub async fn save_workspace(
        &self,
        id: Uuid,
        owner_id: Uuid,
        name: &str,
        locked: bool,
        share_token: Option<&str>,
        layout: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO workspaces (id, owner_id, name, locked, share_token, layout)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, locked = EXCLUDED.locked,
                   share_token = EXCLUDED.share_token, layout = EXCLUDED.layout",
                &[&id, &owner_id, &name, &locked, &share_token, layout],
            )
            .await?;
        client
            .execute(
                "INSERT INTO workspace_members (workspace_id, user_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
                &[&id, &owner_id],
            )
            .await?;
        Ok(())
    }

    pub async fn assign_workspace(&self, workspace_id: Uuid, user_id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO workspace_members (workspace_id, user_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
                &[&workspace_id, &user_id],
            )
            .await?;
        Ok(())
    }

    pub async fn list_workspaces(&self, user_id: Uuid) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT w.id, w.name, w.locked, w.share_token, w.layout, w.owner_id
                 FROM workspaces w
                 JOIN workspace_members m ON m.workspace_id = w.id
                 WHERE m.user_id = $1
                 ORDER BY w.created_at",
                &[&user_id],
            )
            .await?;
        Ok(rows.iter().map(workspace_json).collect())
    }

    pub async fn list_all_workspaces(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, name, locked, share_token, layout, owner_id FROM workspaces ORDER BY created_at",
                &[],
            )
            .await?;
        Ok(rows.iter().map(workspace_json).collect())
    }

    pub async fn workspace(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, name, locked, share_token, layout, owner_id FROM workspaces WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        let mut body = workspace_json(&row);
        let views = client
            .query(
                "SELECT id, type_key, title, pane, position, state FROM views WHERE workspace_id = $1 ORDER BY pane, position",
                &[&id],
            )
            .await?;
        let views = views
            .iter()
            .map(|view| {
                serde_json::json!({
                    "id": view.get::<_, Uuid>("id"),
                    "type": view.get::<_, String>("type_key"),
                    "title": view.get::<_, String>("title"),
                    "pane": view.get::<_, String>("pane"),
                    "position": view.get::<_, i32>("position"),
                    "state": view.get::<_, Value>("state"),
                })
            })
            .collect::<Vec<_>>();
        body.as_object_mut()
            .expect("workspace json")
            .insert("views".into(), Value::Array(views));
        Ok(Some(body))
    }

    pub async fn workspace_by_share(&self, token: &str) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id FROM workspaces WHERE share_token = $1",
                &[&token],
            )
            .await?
        else {
            return Ok(None);
        };
        self.workspace(row.get("id")).await
    }

    pub async fn workspace_locked(&self, id: Uuid) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one("SELECT locked FROM workspaces WHERE id = $1", &[&id])
            .await?;
        Ok(row.get(0))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_view(
        &self,
        id: Uuid,
        workspace_id: Uuid,
        type_key: &str,
        title: &str,
        pane: &str,
        position: i32,
        state: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO views (id, workspace_id, type_key, title, pane, position, state)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, pane = EXCLUDED.pane,
                   position = EXCLUDED.position, state = EXCLUDED.state, type_key = EXCLUDED.type_key",
                &[&id, &workspace_id, &type_key, &title, &pane, &position, state],
            )
            .await?;
        Ok(())
    }

    pub async fn list_view_types(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT key, title, builtin, embed_url FROM view_types ORDER BY key",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "key": row.get::<_, String>("key"),
                    "title": row.get::<_, String>("title"),
                    "builtin": row.get::<_, bool>("builtin"),
                    "embed_url": row.get::<_, Option<String>>("embed_url"),
                })
            })
            .collect())
    }

    pub async fn register_view_type(
        &self,
        key: &str,
        title: &str,
        embed_url: Option<&str>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO view_types (key, title, builtin, embed_url) VALUES ($1, $2, false, $3)
                 ON CONFLICT (key) DO UPDATE SET title = EXCLUDED.title, embed_url = EXCLUDED.embed_url",
                &[&key, &title, &embed_url],
            )
            .await?;
        Ok(())
    }

    pub async fn save_assortment(
        &self,
        id: Uuid,
        owner_id: Uuid,
        name: &str,
        query: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO assortments (id, owner_id, name, query) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, query = EXCLUDED.query",
                &[&id, &owner_id, &name, query],
            )
            .await?;
        Ok(())
    }

    pub async fn list_assortments(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query("SELECT id, name, query FROM assortments ORDER BY name", &[])
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>("id"),
                    "name": row.get::<_, String>("name"),
                    "query": row.get::<_, Value>("query"),
                })
            })
            .collect())
    }

    pub async fn assortment(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, name, query FROM assortments WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::json!({
            "id": row.get::<_, Uuid>("id"),
            "name": row.get::<_, String>("name"),
            "query": row.get::<_, Value>("query"),
        })))
    }

    pub async fn register_job_type(
        &self,
        key: &str,
        title: &str,
        reads_fields: &[String],
        assortment_id: Option<Uuid>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO job_types (key, title, reads_fields, assortment_id) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (key) DO UPDATE SET title = EXCLUDED.title, reads_fields = EXCLUDED.reads_fields,
                   assortment_id = EXCLUDED.assortment_id",
                &[&key, &title, &reads_fields, &assortment_id],
            )
            .await?;
        Ok(())
    }

    pub async fn list_job_types(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT key, title, reads_fields, assortment_id FROM job_types ORDER BY key",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "key": row.get::<_, String>("key"),
                    "title": row.get::<_, String>("title"),
                    "reads_fields": row.get::<_, Vec<String>>("reads_fields"),
                    "assortment_id": row.get::<_, Option<Uuid>>("assortment_id"),
                })
            })
            .collect())
    }

    pub async fn jobs_touching(&self, fields: &[String]) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT key, title, reads_fields FROM job_types WHERE reads_fields && $1",
                &[&fields],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "key": row.get::<_, String>("key"),
                    "title": row.get::<_, String>("title"),
                    "reads_fields": row.get::<_, Vec<String>>("reads_fields"),
                })
            })
            .collect())
    }

    pub async fn create_job_run(
        &self,
        id: Uuid,
        type_key: &str,
        assortment_id: Option<Uuid>,
        actor: PrincipalId,
        on_behalf_of: Option<PrincipalId>,
        total: i32,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO job_runs (id, type_key, status, assortment_id, actor, on_behalf_of, progress_total)
                 VALUES ($1, $2, 'running', $3, $4, $5, $6)",
                &[
                    &id,
                    &type_key,
                    &assortment_id,
                    &actor.as_uuid(),
                    &on_behalf_of.map(PrincipalId::as_uuid),
                    &total,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn update_job_progress(
        &self,
        id: Uuid,
        done: i32,
        log: &str,
        errors: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE job_runs SET progress_done = $2, log = $3, errors = $4 WHERE id = $1",
                &[&id, &done, &log, errors],
            )
            .await?;
        Ok(())
    }

    pub async fn finish_job_run(
        &self,
        id: Uuid,
        status: &str,
        done: i32,
        log: &str,
        errors: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE job_runs SET status = $2, progress_done = $3, log = $4, errors = $5, finished_at = now()
                 WHERE id = $1",
                &[&id, &status, &done, &log, errors],
            )
            .await?;
        Ok(())
    }

    pub async fn job_run(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, type_key, status, progress_done, progress_total, log, errors, created_at, finished_at
                 FROM job_runs WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(job_json(&row)))
    }

    pub async fn job_runs(&self, type_key: &str) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, type_key, status, progress_done, progress_total, log, errors, created_at, finished_at
                 FROM job_runs WHERE type_key = $1 ORDER BY created_at DESC LIMIT 100",
                &[&type_key],
            )
            .await?;
        Ok(rows.iter().map(job_json).collect())
    }

    pub async fn list_proposals(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, title, source, status, conflict, created_at FROM proposals ORDER BY created_at DESC LIMIT 50",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>("id"),
                    "title": row.get::<_, String>("title"),
                    "source": row.get::<_, String>("source"),
                    "status": row.get::<_, String>("status"),
                    "conflict": row.get::<_, bool>("conflict"),
                    "created_at": row.get::<_, DateTime<Utc>>("created_at"),
                })
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_proposal(
        &self,
        id: Uuid,
        title: &str,
        source: &str,
        actor: PrincipalId,
        on_behalf_of: Option<PrincipalId>,
        conflict: bool,
        rows: &[ProposalRow],
    ) -> Result<(), Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        tx.execute(
            "INSERT INTO proposals (id, title, source, status, actor, on_behalf_of, conflict)
             VALUES ($1, $2, $3, 'open', $4, $5, $6)",
            &[
                &id,
                &title,
                &source,
                &actor.as_uuid(),
                &on_behalf_of.map(PrincipalId::as_uuid),
                &conflict,
            ],
        )
        .await?;
        for row in rows {
            tx.execute(
                "INSERT INTO proposal_rows (id, proposal_id, object_id, object_key, type_key, base_version, patch, status, conflict, position)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &row.id,
                    &id,
                    &row.object_id,
                    &row.object_key,
                    &row.type_key,
                    &row.base_version,
                    &row.patch,
                    &row.status,
                    &row.conflict,
                    &row.position,
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn proposal(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, title, source, status, conflict, actor, on_behalf_of, created_at FROM proposals WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        let rows = client
            .query(
                "SELECT id, object_id, object_key, type_key, base_version, patch, status, conflict, position
                 FROM proposal_rows WHERE proposal_id = $1 ORDER BY position",
                &[&id],
            )
            .await?;
        let mut body = serde_json::json!({
            "id": row.get::<_, Uuid>("id"),
            "title": row.get::<_, String>("title"),
            "source": row.get::<_, String>("source"),
            "status": row.get::<_, String>("status"),
            "conflict": row.get::<_, bool>("conflict"),
            "actor": row.get::<_, Uuid>("actor"),
            "on_behalf_of": row.get::<_, Option<Uuid>>("on_behalf_of"),
            "created_at": row.get::<_, DateTime<Utc>>("created_at"),
        });
        let items = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>("id"),
                    "object_id": row.get::<_, Option<Uuid>>("object_id"),
                    "object_key": row.get::<_, Option<String>>("object_key"),
                    "type_key": row.get::<_, Option<String>>("type_key"),
                    "base_version": row.get::<_, Option<i64>>("base_version"),
                    "patch": row.get::<_, Value>("patch"),
                    "status": row.get::<_, String>("status"),
                    "conflict": row.get::<_, bool>("conflict"),
                    "position": row.get::<_, i32>("position"),
                })
            })
            .collect::<Vec<_>>();
        body.as_object_mut()
            .expect("proposal")
            .insert("rows".into(), Value::Array(items));
        Ok(Some(body))
    }

    pub async fn set_proposal_rows(
        &self,
        proposal_id: Uuid,
        row_ids: &[Uuid],
        status: &str,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        if row_ids.is_empty() {
            client
                .execute(
                    "UPDATE proposal_rows SET status = $2 WHERE proposal_id = $1 AND status = 'pending'",
                    &[&proposal_id, &status],
                )
                .await?;
        } else {
            client
                .execute(
                    "UPDATE proposal_rows SET status = $3 WHERE proposal_id = $1 AND id = ANY($2)",
                    &[&proposal_id, &row_ids, &status],
                )
                .await?;
        }
        Ok(())
    }

    pub async fn set_proposal_status(&self, id: Uuid, status: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE proposals SET status = $2 WHERE id = $1",
                &[&id, &status],
            )
            .await?;
        Ok(())
    }

    pub async fn set_proposal_title(&self, id: Uuid, title: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE proposals SET title = $2 WHERE id = $1",
                &[&id, &title],
            )
            .await?;
        Ok(())
    }

    pub async fn set_relation(
        &self,
        from_id: ObjectId,
        to_id: ObjectId,
        kind: &str,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO relations (from_id, to_id, kind) VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
                &[&from_id.as_uuid(), &to_id.as_uuid(), &kind],
            )
            .await?;
        Ok(())
    }

    pub async fn relations_for(&self, id: ObjectId) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT r.kind, r.to_id, o.key, o.type_id
                 FROM relations r JOIN objects o ON o.id = r.to_id
                 WHERE r.from_id = $1
                 UNION ALL
                 SELECT r.kind, r.from_id, o.key, o.type_id
                 FROM relations r JOIN objects o ON o.id = r.from_id
                 WHERE r.to_id = $1",
                &[&id.as_uuid()],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "kind": row.get::<_, String>(0),
                    "object_id": row.get::<_, Uuid>(1),
                    "key": row.get::<_, String>(2),
                    "type_id": row.get::<_, Uuid>(3),
                })
            })
            .collect())
    }

    pub async fn history(
        &self,
        object_id: Option<Uuid>,
        actor: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<HistoryEntry>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT c.seq, c.at, c.batch_id, c.object_id, c.version, c.op, c.diff,
                        c.actor, c.on_behalf_of, b.source, b.kind,
                        pa.handle, pb.handle
                 FROM changes c
                 JOIN batches b ON b.id = c.batch_id
                 LEFT JOIN principals pa ON pa.id = c.actor
                 LEFT JOIN principals pb ON pb.id = c.on_behalf_of
                 WHERE ($1::uuid IS NULL OR c.object_id = $1)
                   AND ($2::uuid IS NULL OR c.actor = $2 OR c.on_behalf_of = $2)
                 ORDER BY c.seq DESC
                 LIMIT $3",
                &[&object_id, &actor, &limit],
            )
            .await?;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let version: Option<i64> = row.get("version");
            entries.push(HistoryEntry {
                seq: row.get("seq"),
                at: row.get("at"),
                batch_id: row.get("batch_id"),
                object_id: row.get("object_id"),
                version,
                op: row.get("op"),
                source: row.get("source"),
                kind: row.get("kind"),
                actor: row.get("actor"),
                on_behalf_of: row.get("on_behalf_of"),
                actor_handle: row.get(11),
                behalf_handle: row.get(12),
                diff: row.get("diff"),
            });
        }
        Ok(entries)
    }

    pub async fn keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT key FROM objects WHERE starts_with(key, $1) AND deleted_at IS NULL",
                &[&prefix],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }

    pub async fn count_attribute_usage(&self, attribute_id: Uuid) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(DISTINCT object_id)::bigint FROM object_values WHERE \"values\" ? $1",
                &[&attribute_id.to_string()],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn objects_with_attribute(
        &self,
        attribute_id: Uuid,
        limit: i64,
    ) -> Result<Vec<ObjectId>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT DISTINCT object_id FROM object_values WHERE \"values\" ? $1 LIMIT $2",
                &[&attribute_id.to_string(), &limit],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| ObjectId::from_uuid(row.get(0)))
            .collect())
    }

    pub async fn object_ids_after(
        &self,
        after: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<ObjectId>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id FROM objects
                 WHERE deleted_at IS NULL AND ($1::uuid IS NULL OR id > $1)
                 ORDER BY id
                 LIMIT $2",
                &[&after, &limit],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| ObjectId::from_uuid(row.get(0)))
            .collect())
    }

    pub async fn max_change_seq(&self) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one("SELECT COALESCE(MAX(seq), 0) FROM changes", &[])
            .await?;
        Ok(row.get(0))
    }

    pub async fn indexer_seq(&self) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one("SELECT seq FROM indexer_progress WHERE id = 1", &[])
            .await?;
        Ok(row.get(0))
    }

    pub async fn set_indexer_seq(&self, seq: i64) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE indexer_progress SET seq = GREATEST(seq, $1), updated_at = now() WHERE id = 1",
                &[&seq],
            )
            .await?;
        Ok(())
    }

    pub async fn indexer_collection(&self) -> Result<String, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one("SELECT collection FROM indexer_progress WHERE id = 1", &[])
            .await?;
        Ok(row.get(0))
    }

    pub async fn set_indexer_collection(&self, name: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE indexer_progress SET collection = $1, updated_at = now() WHERE id = 1",
                &[&name],
            )
            .await?;
        Ok(())
    }

    pub async fn create_webhook(
        &self,
        id: Uuid,
        url: &str,
        secret: &str,
        assortment_id: Option<Uuid>,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO webhooks (id, url, secret, assortment_id) VALUES ($1, $2, $3, $4)",
                &[&id, &url, &secret, &assortment_id],
            )
            .await?;
        Ok(())
    }

    pub async fn list_webhooks(&self) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, url, assortment_id, enabled FROM webhooks ORDER BY created_at",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>("id"),
                    "url": row.get::<_, String>("url"),
                    "assortment_id": row.get::<_, Option<Uuid>>("assortment_id"),
                    "enabled": row.get::<_, bool>("enabled"),
                })
            })
            .collect())
    }

    pub async fn enabled_webhooks(
        &self,
    ) -> Result<Vec<(Uuid, String, String, Option<Uuid>)>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, url, secret, assortment_id FROM webhooks WHERE enabled",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .collect())
    }

    pub async fn relay_lag_bytes(&self, slot: &str) -> Result<Option<i64>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), confirmed_flush_lsn)::bigint
                 FROM pg_replication_slots WHERE slot_name = $1",
                &[&slot],
            )
            .await?
        else {
            return Ok(None);
        };
        let lag: Option<i64> = row.get(0);
        Ok(lag)
    }

    pub async fn replication_slot_active(&self, slot: &str) -> Result<Option<bool>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT active FROM pg_replication_slots WHERE slot_name = $1",
                &[&slot],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(row.get(0)))
    }

    pub async fn list_object_heads(
        &self,
        limit: i64,
    ) -> Result<Vec<(Uuid, String, Uuid, i64)>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, key, type_id, version FROM objects
                 WHERE deleted_at IS NULL
                 ORDER BY key
                 LIMIT $1",
                &[&limit],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get::<_, i64>(3)))
            .collect())
    }
}

fn user_from_row(row: &tokio_postgres::Row) -> UserRecord {
    user_from_row_at(row, 0)
}

fn user_from_row_at(row: &tokio_postgres::Row, start: usize) -> UserRecord {
    UserRecord {
        id: row.get(start),
        principal_id: PrincipalId::from_uuid(row.get(start + 1)),
        email: row.get(start + 2),
        password_hash: row.get(start + 3),
        role: row.get(start + 4),
        display_name: row.get(start + 5),
    }
}

fn workspace_json(row: &tokio_postgres::Row) -> Value {
    serde_json::json!({
        "id": row.get::<_, Uuid>("id"),
        "name": row.get::<_, String>("name"),
        "locked": row.get::<_, bool>("locked"),
        "share_token": row.get::<_, Option<String>>("share_token"),
        "layout": row.get::<_, Value>("layout"),
        "owner_id": row.get::<_, Uuid>("owner_id"),
    })
}

fn job_json(row: &tokio_postgres::Row) -> Value {
    serde_json::json!({
        "id": row.get::<_, Uuid>("id"),
        "type_key": row.get::<_, String>("type_key"),
        "status": row.get::<_, String>("status"),
        "progress_done": row.get::<_, i32>("progress_done"),
        "progress_total": row.get::<_, i32>("progress_total"),
        "log": row.get::<_, String>("log"),
        "errors": row.get::<_, Value>("errors"),
        "created_at": row.get::<_, DateTime<Utc>>("created_at"),
        "finished_at": row.get::<_, Option<DateTime<Utc>>>("finished_at"),
    })
}
