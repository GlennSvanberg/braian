//! Tables that sit on the log: people, workspaces, jobs, proposals, relations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio_postgres::types::ToSql;
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

/// One usage count. Empty place ids mean the attribute's total object count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeUsageCount {
    pub attribute_id: Uuid,
    pub type_id: Option<Uuid>,
    pub structure_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub objects: i64,
}

fn usage_attribute_id(text: String) -> Option<Uuid> {
    Uuid::parse_str(&text).ok()
}

impl Store {
    pub async fn model_draft(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(i64, serde_json::Value, DateTime<Utc>)>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT base_revision, steps, updated_at FROM model_drafts WHERE user_id = $1",
                &[&user_id],
            )
            .await?;
        Ok(row.map(|row| (row.get(0), row.get(1), row.get(2))))
    }

    pub async fn put_model_draft(
        &self,
        user_id: Uuid,
        base_revision: i64,
        steps: &serde_json::Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO model_drafts (user_id, base_revision, steps, updated_at)
                 VALUES ($1, $2, $3, now())
                 ON CONFLICT (user_id) DO UPDATE
                 SET base_revision = EXCLUDED.base_revision,
                     steps = EXCLUDED.steps,
                     updated_at = now()",
                &[&user_id, &base_revision, steps],
            )
            .await?;
        Ok(())
    }

    pub async fn delete_model_draft(&self, user_id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM model_drafts WHERE user_id = $1", &[&user_id])
            .await?;
        Ok(())
    }

    pub async fn principal_by_handle(&self, handle: &str) -> Result<Option<PrincipalId>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt("SELECT id FROM principals WHERE handle = $1", &[&handle])
            .await?;
        Ok(row.map(|row| PrincipalId::from_uuid(row.get(0))))
    }

    pub async fn attach_user(
        &self,
        principal: PrincipalId,
        email: &str,
        password_hash: &str,
        role: &str,
        display_name: &str,
    ) -> Result<Uuid, Error> {
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
        Ok(id)
    }

    pub async fn user_id_for_principal(&self, principal: Uuid) -> Result<Option<Uuid>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT id FROM users WHERE principal_id = $1",
                &[&principal],
            )
            .await?;
        Ok(row.map(|row| row.get(0)))
    }

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

    pub async fn workspace_is_member(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
                &[&workspace_id, &user_id],
            )
            .await?;
        Ok(row.is_some())
    }

    pub async fn delete_workspace(&self, id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM workspaces WHERE id = $1", &[&id])
            .await?;
        Ok(())
    }

    pub async fn workspace_count_for_user(&self, user_id: Uuid) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*)::bigint AS n
                 FROM workspace_members
                 WHERE user_id = $1",
                &[&user_id],
            )
            .await?;
        Ok(row.get("n"))
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

    pub async fn workspace_state(
        &self,
        user_id: Uuid,
        environment: &str,
        workspace_id: Uuid,
    ) -> Result<Option<(Value, DateTime<Utc>)>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT document, updated_at FROM workspace_states
                 WHERE user_id = $1 AND environment = $2 AND workspace_id = $3",
                &[&user_id, &environment, &workspace_id],
            )
            .await?;
        Ok(row.map(|row| (row.get(0), row.get(1))))
    }

    pub async fn put_workspace_state(
        &self,
        user_id: Uuid,
        environment: &str,
        workspace_id: Uuid,
        document: &Value,
    ) -> Result<DateTime<Utc>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "INSERT INTO workspace_states (user_id, environment, workspace_id, document, updated_at)
                 VALUES ($1, $2, $3, $4, now())
                 ON CONFLICT (user_id, environment, workspace_id)
                 DO UPDATE SET document = EXCLUDED.document, updated_at = now()
                 RETURNING updated_at",
                &[&user_id, &environment, &workspace_id, document],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn delete_workspace_state(
        &self,
        user_id: Uuid,
        environment: &str,
        workspace_id: Uuid,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "DELETE FROM workspace_states
                 WHERE user_id = $1 AND environment = $2 AND workspace_id = $3",
                &[&user_id, &environment, &workspace_id],
            )
            .await?;
        Ok(())
    }

    /// View type key and state document, when the view exists.
    pub async fn view_workspace_id(&self, id: Uuid) -> Result<Option<Uuid>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt("SELECT workspace_id FROM views WHERE id = $1", &[&id])
            .await?;
        Ok(row.map(|row| row.get(0)))
    }

    pub async fn view_row(&self, id: Uuid) -> Result<Option<(String, Value)>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt("SELECT type_key, state FROM views WHERE id = $1", &[&id])
            .await?;
        Ok(row.map(|row| (row.get(0), row.get(1))))
    }

    pub async fn view_column_prefs(
        &self,
        user_id: Uuid,
        environment: &str,
        view_id: Uuid,
    ) -> Result<Option<(Value, DateTime<Utc>)>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT document, updated_at FROM view_column_prefs
                 WHERE user_id = $1 AND environment = $2 AND view_id = $3",
                &[&user_id, &environment, &view_id],
            )
            .await?;
        Ok(row.map(|row| (row.get(0), row.get(1))))
    }

    pub async fn put_view_column_prefs(
        &self,
        user_id: Uuid,
        environment: &str,
        view_id: Uuid,
        document: &Value,
    ) -> Result<DateTime<Utc>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "INSERT INTO view_column_prefs (user_id, environment, view_id, document, updated_at)
                 VALUES ($1, $2, $3, $4, now())
                 ON CONFLICT (user_id, environment, view_id)
                 DO UPDATE SET document = EXCLUDED.document, updated_at = now()
                 RETURNING updated_at",
                &[&user_id, &environment, &view_id, document],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn delete_view_column_prefs(
        &self,
        user_id: Uuid,
        environment: &str,
        view_id: Uuid,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "DELETE FROM view_column_prefs
                 WHERE user_id = $1 AND environment = $2 AND view_id = $3",
                &[&user_id, &environment, &view_id],
            )
            .await?;
        Ok(())
    }

    /// Drop every saved column layout for one user (all views / environments).
    pub async fn delete_all_view_column_prefs_for_user(&self, user_id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "DELETE FROM view_column_prefs WHERE user_id = $1",
                &[&user_id],
            )
            .await?;
        Ok(())
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
                 ON CONFLICT (id) DO UPDATE SET workspace_id = EXCLUDED.workspace_id,
                   title = EXCLUDED.title, pane = EXCLUDED.pane, position = EXCLUDED.position,
                   state = EXCLUDED.state, type_key = EXCLUDED.type_key",
                &[
                    &id,
                    &workspace_id,
                    &type_key,
                    &title,
                    &pane,
                    &position,
                    state,
                ],
            )
            .await?;
        Ok(())
    }

    /// Stored view document, without the seed template.
    pub async fn view_state(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt("SELECT state FROM views WHERE id = $1", &[&id])
            .await?;
        Ok(row.map(|row| row.get(0)))
    }

    /// Rewrites `views.state` and leaves title, pane and position alone.
    pub async fn update_view_state(&self, id: Uuid, state: &Value) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("UPDATE views SET state = $2 WHERE id = $1", &[&id, state])
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

    pub async fn delete_view(&self, id: Uuid) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM views WHERE id = $1", &[&id])
            .await?;
        Ok(())
    }

    /// Deletes a registered (non-builtin) view type that no view uses.
    pub async fn delete_view_type(&self, key: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "DELETE FROM view_types WHERE key = $1 AND builtin = false
                 AND NOT EXISTS (SELECT 1 FROM views WHERE type_key = $1)",
                &[&key],
            )
            .await?;
        Ok(())
    }

    pub async fn save_assortment(
        &self,
        id: Uuid,
        owner_id: Uuid,
        type_id: Uuid,
        name: &str,
        query: &Value,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO assortments (id, owner_id, type_id, name, query) VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, query = EXCLUDED.query, type_id = EXCLUDED.type_id",
                &[&id, &owner_id, &type_id, &name, query],
            )
            .await?;
        Ok(())
    }

    /// Rewrites `assortments.query` and leaves the name and type alone.
    pub async fn update_assortment_query(&self, id: Uuid, query: &Value) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE assortments SET query = $2 WHERE id = $1",
                &[&id, query],
            )
            .await?;
        Ok(())
    }

    pub async fn list_assortments(&self, type_id: Option<Uuid>) -> Result<Vec<Value>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, name, type_id, query FROM assortments
                 WHERE $1::uuid IS NULL OR type_id = $1
                 ORDER BY name",
                &[&type_id],
            )
            .await?;
        Ok(rows.iter().map(assortment_json).collect())
    }

    pub async fn assortment(&self, id: Uuid) -> Result<Option<Value>, Error> {
        let client = self.pool().get().await?;
        let Some(row) = client
            .query_opt(
                "SELECT id, name, type_id, query FROM assortments WHERE id = $1",
                &[&id],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(assortment_json(&row)))
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

    /// Removes a job type and its runs. Used when seed drops a type that is not a real runner.
    pub async fn delete_job_type(&self, key: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute("DELETE FROM job_runs WHERE type_key = $1", &[&key])
            .await?;
        client
            .execute("DELETE FROM job_types WHERE key = $1", &[&key])
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

    pub async fn count_in_groups(&self, groups: &[Uuid]) -> Result<i64, Error> {
        if groups.is_empty() {
            return Ok(0);
        }
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(DISTINCT o.id)::bigint
                 FROM objects o
                 JOIN object_structures s ON s.object_id = o.id
                 WHERE o.deleted_at IS NULL AND s.group_id = ANY($1)",
                &[&groups],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn sample_in_groups(&self, groups: &[Uuid], limit: i64) -> Result<Vec<Value>, Error> {
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT DISTINCT o.id, o.key
                 FROM objects o
                 JOIN object_structures s ON s.object_id = o.id
                 WHERE o.deleted_at IS NULL AND s.group_id = ANY($1)
                 ORDER BY o.key
                 LIMIT $2",
                &[&groups, &limit],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>(0),
                    "key": row.get::<_, String>(1),
                })
            })
            .collect())
    }

    pub async fn frecency(&self, principal: Uuid) -> Result<Value, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT command_id, uses, last_at FROM command_frecency WHERE principal_id = $1",
                &[&principal],
            )
            .await?;
        let mut entries = serde_json::Map::new();
        for row in rows {
            let id: String = row.get(0);
            let uses: i32 = row.get(1);
            let last: DateTime<Utc> = row.get(2);
            entries.insert(
                id,
                serde_json::json!({
                    "uses": uses,
                    "last": last.timestamp_millis(),
                }),
            );
        }
        Ok(Value::Object(entries))
    }

    pub async fn touch_frecency(&self, principal: Uuid, command_id: &str) -> Result<Value, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "INSERT INTO command_frecency (principal_id, command_id, uses, last_at)
                 VALUES ($1, $2, 1, now())
                 ON CONFLICT (principal_id, command_id)
                 DO UPDATE SET uses = command_frecency.uses + 1, last_at = now()
                 RETURNING uses, last_at",
                &[&principal, &command_id],
            )
            .await?;
        let uses: i32 = row.get(0);
        let last: DateTime<Utc> = row.get(1);
        Ok(serde_json::json!({
            "id": command_id,
            "uses": uses,
            "last": last.timestamp_millis(),
        }))
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

    /// One page of proposals, newest first. `status` and `source` filter when set.
    /// Returns the total that matches, so a table can size its scroll area.
    pub async fn list_proposals_page(
        &self,
        status: Option<&str>,
        source: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(i64, Vec<Value>), Error> {
        let client = self.pool().get().await?;
        let total: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM proposals
                 WHERE ($1::text IS NULL OR status = $1) AND ($2::text IS NULL OR source = $2)",
                &[&status, &source],
            )
            .await?
            .get(0);
        let rows = client
            .query(
                "SELECT p.id, p.title, p.source, p.status, p.conflict, p.created_at,
                        p.session_id, p.counters, p.on_behalf_of,
                        a.name AS actor_name, b.name AS behalf_name,
                        (SELECT count(*)::bigint FROM proposal_rows r WHERE r.proposal_id = p.id) AS row_count
                 FROM proposals p
                 LEFT JOIN principals a ON a.id = p.actor
                 LEFT JOIN principals b ON b.id = p.on_behalf_of
                 WHERE ($1::text IS NULL OR p.status = $1) AND ($2::text IS NULL OR p.source = $2)
                 ORDER BY p.created_at DESC
                 OFFSET $3 LIMIT $4",
                &[&status, &source, &offset, &limit],
            )
            .await?;
        let items = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<_, Uuid>("id"),
                    "title": row.get::<_, String>("title"),
                    "source": row.get::<_, String>("source"),
                    "status": row.get::<_, String>("status"),
                    "conflict": row.get::<_, bool>("conflict"),
                    "created_at": row.get::<_, DateTime<Utc>>("created_at"),
                    "session_id": row.get::<_, Option<Uuid>>("session_id"),
                    "counters": row.get::<_, Option<Value>>("counters"),
                    "actor_name": row.get::<_, Option<String>>("actor_name"),
                    "behalf_name": row.get::<_, Option<String>>("behalf_name"),
                    "row_count": row.get::<_, i64>("row_count"),
                })
            })
            .collect();
        Ok((total, items))
    }

    /// One page of a proposal's own rows, in position order.
    pub async fn proposal_rows_page(
        &self,
        id: Uuid,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(i64, Vec<Value>), Error> {
        let client = self.pool().get().await?;
        let total: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM proposal_rows
                 WHERE proposal_id = $1 AND ($2::text IS NULL OR status = $2)",
                &[&id, &status],
            )
            .await?
            .get(0);
        let rows = client
            .query(
                "SELECT id, object_id, object_key, type_key, base_version, patch, status, conflict, position
                 FROM proposal_rows
                 WHERE proposal_id = $1 AND ($2::text IS NULL OR status = $2)
                 ORDER BY position
                 OFFSET $3 LIMIT $4",
                &[&id, &status, &offset, &limit],
            )
            .await?;
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
            .collect();
        Ok((total, items))
    }

    /// Open proposals still waiting for a person: a total, and a count per object.
    pub async fn pending_summary(&self) -> Result<Value, Error> {
        let client = self.pool().get().await?;
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*)::bigint
                 FROM proposal_rows r
                 JOIN proposals p ON p.id = r.proposal_id
                 WHERE r.status = 'pending' AND p.status = 'open'",
                &[],
            )
            .await?
            .get(0);
        let rows = client
            .query(
                "SELECT r.object_id, COUNT(*)::bigint
                 FROM proposal_rows r
                 JOIN proposals p ON p.id = r.proposal_id
                 WHERE r.status = 'pending' AND p.status = 'open' AND r.object_id IS NOT NULL
                 GROUP BY r.object_id",
                &[],
            )
            .await?;
        let objects = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "object_id": row.get::<_, Uuid>(0),
                    "count": row.get::<_, i64>(1),
                })
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "total": total, "objects": objects }))
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
                "SELECT p.id, p.title, p.source, p.status, p.conflict, p.actor, p.on_behalf_of, p.created_at,
                        p.session_id, p.counters,
                        a.name AS actor_name, b.name AS behalf_name
                 FROM proposals p
                 LEFT JOIN principals a ON a.id = p.actor
                 LEFT JOIN principals b ON b.id = p.on_behalf_of
                 WHERE p.id = $1",
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
            "actor_name": row.get::<_, Option<String>>("actor_name"),
            "behalf_name": row.get::<_, Option<String>>("behalf_name"),
            "created_at": row.get::<_, DateTime<Utc>>("created_at"),
            "session_id": row.get::<_, Option<Uuid>>("session_id"),
            "counters": row.get::<_, Option<Value>>("counters"),
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

    /// Moves an open proposal to `accepted` or `discarded` while `claim` still holds it.
    ///
    /// A missing row, a lease that is not `claim`, and any other current status
    /// are errors. When `reject_pending` is set, rows still `pending` become
    /// `rejected` in the same transaction, so a failed status write leaves them
    /// pending. The claim is cleared when the new status commits.
    pub async fn set_proposal_status(
        &self,
        id: Uuid,
        status: &str,
        claim: Uuid,
        reject_pending: bool,
    ) -> Result<(), Error> {
        if status != "accepted" && status != "discarded" {
            return Err(Error::ProposalTransition {
                id,
                from: "open".into(),
                to: status.to_owned(),
            });
        }
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let row = tx
            .query_opt(
                "SELECT status, claimed_by, (claimed_until > now()) AS live
                 FROM proposals WHERE id = $1 FOR UPDATE",
                &[&id],
            )
            .await?;
        let Some(row) = row else {
            return Err(Error::ProposalNotFound(id));
        };
        let current: String = row.get(0);
        let holder: Option<Uuid> = row.get(1);
        let live: Option<bool> = row.get(2);
        if current != "open" {
            return Err(Error::ProposalTransition {
                id,
                from: current,
                to: status.to_owned(),
            });
        }
        if holder != Some(claim) || live != Some(true) {
            return Err(Error::ProposalClaimLost(id));
        }
        if reject_pending {
            tx.execute(
                "UPDATE proposal_rows SET status = 'rejected'
                 WHERE proposal_id = $1 AND status = 'pending'",
                &[&id],
            )
            .await?;
        }
        let updated = tx
            .execute(
                "UPDATE proposals
                 SET status = $2, claimed_by = NULL, claimed_until = NULL
                 WHERE id = $1 AND claimed_by = $3",
                &[&id, &status, &claim],
            )
            .await?;
        if updated != 1 {
            return Err(Error::ProposalClaimLost(id));
        }
        tx.commit().await?;
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

    pub async fn set_proposal_source(&self, id: Uuid, source: &str) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE proposals SET source = $2 WHERE id = $1",
                &[&id, &source],
            )
            .await?;
        Ok(())
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
        self.expand_history_diffs(&**client, &mut entries).await?;
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

    /// Objects, types and groups that store a value for each attribute.
    ///
    /// A row with `type_id`, `structure_id` and `group_id` all empty is the
    /// total. That total uses the same predicate as [`Self::count_attribute_usage`].
    pub async fn attribute_usage_counts(&self) -> Result<Vec<AttributeUsageCount>, Error> {
        let client = self.pool().get().await?;
        let totals = client
            .query(
                "SELECT keys.attr, count(DISTINCT v.object_id)::bigint
                 FROM object_values v
                 CROSS JOIN LATERAL jsonb_object_keys(v.\"values\") AS keys(attr)
                 WHERE v.is_latest
                 GROUP BY keys.attr",
                &[],
            )
            .await?;
        let types = client
            .query(
                "SELECT keys.attr, o.type_id, count(DISTINCT v.object_id)::bigint
                 FROM object_values v
                 JOIN objects o ON o.id = v.object_id
                 CROSS JOIN LATERAL jsonb_object_keys(v.\"values\") AS keys(attr)
                 WHERE v.is_latest
                 GROUP BY keys.attr, o.type_id",
                &[],
            )
            .await?;
        let groups = client
            .query(
                "SELECT keys.attr, s.structure_id, s.group_id, count(DISTINCT v.object_id)::bigint
                 FROM object_values v
                 JOIN object_structures s ON s.object_id = v.object_id
                 CROSS JOIN LATERAL jsonb_object_keys(v.\"values\") AS keys(attr)
                 WHERE v.is_latest
                 GROUP BY keys.attr, s.structure_id, s.group_id",
                &[],
            )
            .await?;
        let mut counts = Vec::new();
        for row in totals {
            let Some(attribute_id) = usage_attribute_id(row.get(0)) else {
                continue;
            };
            counts.push(AttributeUsageCount {
                attribute_id,
                type_id: None,
                structure_id: None,
                group_id: None,
                objects: row.get(1),
            });
        }
        for row in types {
            let Some(attribute_id) = usage_attribute_id(row.get(0)) else {
                continue;
            };
            counts.push(AttributeUsageCount {
                attribute_id,
                type_id: Some(row.get(1)),
                structure_id: None,
                group_id: None,
                objects: row.get(2),
            });
        }
        for row in groups {
            let Some(attribute_id) = usage_attribute_id(row.get(0)) else {
                continue;
            };
            counts.push(AttributeUsageCount {
                attribute_id,
                type_id: None,
                structure_id: Some(row.get(1)),
                group_id: Some(row.get(2)),
                objects: row.get(3),
            });
        }
        Ok(counts)
    }

    pub async fn count_attribute_usage(&self, attribute_id: Uuid) -> Result<i64, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT count(DISTINCT v.object_id)::bigint
                 FROM object_values v
                 WHERE v.is_latest AND v.\"values\" ? $1",
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
                "SELECT DISTINCT v.object_id
                 FROM object_values v
                 WHERE v.is_latest AND v.\"values\" ? $1
                 LIMIT $2",
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
                 WHERE deleted_at IS NULL
                   AND NOT EXISTS (SELECT 1 FROM links WHERE links.object_id = objects.id)
                   AND ($1::uuid IS NULL OR id > $1)
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

    /// Asks the indexer to rebuild the search alias from Postgres.
    pub async fn request_reindex(&self) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE indexer_progress SET reindex_requested = true, updated_at = now() WHERE id = 1",
                &[],
            )
            .await?;
        Ok(())
    }

    /// Clears a pending reindex request. `true` when this caller won the flag.
    pub async fn take_reindex_request(&self) -> Result<bool, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "UPDATE indexer_progress
                 SET reindex_requested = false, updated_at = now()
                 WHERE id = 1 AND reindex_requested
                 RETURNING id",
                &[],
            )
            .await?;
        Ok(row.is_some())
    }

    pub async fn list_object_heads(
        &self,
        limit: i64,
    ) -> Result<Vec<(Uuid, String, Uuid, i64)>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, key, type_id, version FROM objects
                 WHERE deleted_at IS NULL AND key IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM links WHERE links.object_id = objects.id)
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

    /// One page of active object heads, filtered before `limit`.
    ///
    /// `key_contains` is a literal substring, matched case-insensitively.
    /// `after_id` is the last id of the previous page. Rows are ordered by id.
    /// The query always checks out the interactive pool, even on a bulk-lane store.
    pub async fn list_objects_page(
        &self,
        type_id: Option<Uuid>,
        key_contains: Option<&str>,
        after_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<(Uuid, String, Uuid, i64)>, Error> {
        let client = self.interactive_pool().get().await?;
        let pattern = key_contains.map(like_contains);
        let mut sql = String::from(
            "SELECT id, key, type_id, version FROM objects WHERE deleted_at IS NULL AND key IS NOT NULL AND NOT EXISTS (SELECT 1 FROM links WHERE links.object_id = objects.id)",
        );
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
        if let Some(type_id) = type_id.as_ref() {
            push_pred(&mut sql, params.len(), " AND type_id = $");
            params.push(type_id);
        }
        if let Some(pattern) = pattern.as_ref() {
            push_pred(&mut sql, params.len(), " AND key ILIKE $");
            // E'\\' is one backslash: the LIKE escape character.
            sql.push_str(" ESCAPE E'\\\\'");
            params.push(pattern);
        }
        if let Some(after_id) = after_id.as_ref() {
            push_pred(&mut sql, params.len(), " AND id > $");
            params.push(after_id);
        }
        push_pred(&mut sql, params.len(), " ORDER BY id LIMIT $");
        params.push(&limit);
        let rows = client.query(&sql, &params).await?;
        Ok(rows
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get::<_, i64>(3)))
            .collect())
    }
}

fn assortment_json(row: &tokio_postgres::Row) -> Value {
    serde_json::json!({
        "id": row.get::<_, Uuid>("id"),
        "name": row.get::<_, String>("name"),
        "type_id": row.get::<_, Uuid>("type_id"),
        "query": row.get::<_, Value>("query"),
    })
}

fn push_pred(sql: &mut String, params_len: usize, clause: &str) {
    sql.push_str(clause);
    sql.push_str(&params_len.saturating_add(1).to_string());
}

/// A contains-pattern for `ILIKE ... ESCAPE '\'`. `%`, `_` and `\` are literal.
pub(crate) fn like_contains(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len().saturating_add(2));
    pattern.push('%');
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
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

#[cfg(test)]
mod tests {
    use super::like_contains;

    #[test]
    fn key_search_escapes_like_metacharacters() {
        assert_eq!(like_contains("tent"), "%tent%");
        assert_eq!(like_contains("100%_"), "%100\\%\\_%");
        assert_eq!(like_contains("a\\b"), "%a\\\\b%");
    }
}
