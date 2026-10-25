//! Task records for usage. Raw events keep the audit link. Summaries do not
//! carry a person, a session, or an actor id.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Error;
use crate::session::Store;

#[derive(Debug, Clone)]
pub struct UsageEventRow {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_type: String,
    pub task_id: Uuid,
    pub task_type: String,
    pub session_id: Uuid,
    pub operation: String,
    pub object_ids: Vec<Uuid>,
    pub object_count: i32,
    pub result: String,
    pub duration_ms: Option<i32>,
    pub audit_batch_id: Option<Uuid>,
    pub audit_seq: Option<i64>,
    pub actor_id: Option<Uuid>,
    pub on_behalf_of: Option<Uuid>,
    pub ambiguous: bool,
    pub is_change: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSettings {
    pub raw_retention_days: i32,
    pub inactivity_gap_seconds: i32,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            raw_retention_days: 30,
            inactivity_gap_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageByType {
    pub task_type: String,
    pub tasks: i64,
    pub human_time_ms: i64,
    pub changes_human: i64,
    pub changes_agent: i64,
    pub changes_integration: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTrend {
    pub day: String,
    pub task_type: String,
    pub tasks: i64,
    pub human_time_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSummary {
    pub by_type: Vec<UsageByType>,
    pub trend: Vec<UsageTrend>,
}

#[derive(Debug, Clone)]
pub struct SegmentEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub ambiguous: bool,
    pub task_type: String,
    pub operation: String,
    pub actor_type: String,
    pub object_count: i32,
    pub duration_ms: Option<i32>,
}

impl Store {
    pub async fn usage_settings(&self) -> Result<UsageSettings, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT raw_retention_days, inactivity_gap_seconds FROM telemetry_settings WHERE singleton",
                &[],
            )
            .await?;
        Ok(match row {
            Some(row) => UsageSettings {
                raw_retention_days: row.get(0),
                inactivity_gap_seconds: row.get(1),
            },
            None => UsageSettings::default(),
        })
    }

    pub async fn put_usage_settings(&self, settings: &UsageSettings) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE telemetry_settings
                 SET raw_retention_days = $1, inactivity_gap_seconds = $2
                 WHERE singleton",
                &[
                    &settings.raw_retention_days,
                    &settings.inactivity_gap_seconds,
                ],
            )
            .await?;
        Ok(())
    }

    /// Inserts events and refreshes the task rows they belong to.
    ///
    /// Duplicate ids are ignored so a consumer retry does not double-count.
    pub async fn insert_usage_events(&self, events: &[UsageEventRow]) -> Result<(), Error> {
        if events.is_empty() {
            return Ok(());
        }
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        for event in events {
            tx.execute(
                "INSERT INTO telemetry_events (
                    id, occurred_at, actor_type, task_id, task_type, session_id, operation,
                    object_ids, object_count, result, duration_ms, audit_batch_id, audit_seq,
                    actor_id, on_behalf_of, ambiguous, is_change
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12, $13,
                    $14, $15, $16, $17
                 )
                 ON CONFLICT (id) DO NOTHING",
                &[
                    &event.id,
                    &event.occurred_at,
                    &event.actor_type,
                    &event.task_id,
                    &event.task_type,
                    &event.session_id,
                    &event.operation,
                    &event.object_ids,
                    &event.object_count,
                    &event.result,
                    &event.duration_ms,
                    &event.audit_batch_id,
                    &event.audit_seq,
                    &event.actor_id,
                    &event.on_behalf_of,
                    &event.ambiguous,
                    &event.is_change,
                ],
            )
            .await?;
        }
        let task_ids: Vec<Uuid> = events.iter().map(|event| event.task_id).collect();
        tx.execute(RECOMPUTE, &[&task_ids]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn close_idle_tasks(&self, gap_seconds: i32) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE usage_tasks
                 SET open = false
                 WHERE open
                   AND ended_at < now() - ($1::int * interval '1 second')",
                &[&gap_seconds],
            )
            .await?;
        Ok(())
    }

    pub async fn purge_usage_events(&self, retention_days: i32) -> Result<u64, Error> {
        let client = self.pool().get().await?;
        let deleted = client
            .execute(
                "DELETE FROM telemetry_events
                 WHERE occurred_at < now() - ($1::int * interval '1 day')",
                &[&retention_days],
            )
            .await?;
        Ok(deleted)
    }

    /// Closed tasks the rule engine marked ambiguous and a classifier has not
    /// answered yet.
    pub async fn tasks_to_classify(&self) -> Result<Vec<Uuid>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id FROM usage_tasks
                 WHERE open = false AND classifier = 'rules'
                   AND (ambiguous OR task_type = 'other')
                 ORDER BY ended_at
                 LIMIT 20",
                &[],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }

    pub async fn segment_events(&self, task_id: Uuid) -> Result<Vec<SegmentEvent>, Error> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT id, occurred_at, ambiguous, task_type, operation, actor_type, object_count, duration_ms
                 FROM telemetry_events
                 WHERE task_id = $1
                 ORDER BY occurred_at, id",
                &[&task_id],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| SegmentEvent {
                id: row.get(0),
                occurred_at: row.get(1),
                ambiguous: row.get(2),
                task_type: row.get(3),
                operation: row.get(4),
                actor_type: row.get(5),
                object_count: row.get(6),
                duration_ms: row.get(7),
            })
            .collect())
    }

    pub async fn set_task_classification(
        &self,
        task_id: Uuid,
        task_type: &str,
    ) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE usage_tasks
                 SET task_type = $2, classifier = 'decisions'
                 WHERE id = $1",
                &[&task_id, &task_type],
            )
            .await?;
        Ok(())
    }

    /// Moves a tail of a segment onto a new task. The audit columns stay.
    pub async fn retask_events(
        &self,
        event_ids: &[Uuid],
        task_id: Uuid,
        task_type: &str,
    ) -> Result<(), Error> {
        if event_ids.is_empty() {
            return Ok(());
        }
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE telemetry_events
                 SET task_id = $2, task_type = $3
                 WHERE id = ANY($1)",
                &[&event_ids, &task_id, &task_type],
            )
            .await?;
        Ok(())
    }

    pub async fn recompute_usage_tasks(&self, task_ids: &[Uuid]) -> Result<(), Error> {
        if task_ids.is_empty() {
            return Ok(());
        }
        let client = self.pool().get().await?;
        client.execute(RECOMPUTE, &[&task_ids]).await?;
        Ok(())
    }

    pub async fn usage_summary(&self, days: i32) -> Result<UsageSummary, Error> {
        let client = self.pool().get().await?;
        let by_type_rows = client
            .query(
                "SELECT task_type,
                        count(*)::bigint,
                        coalesce(sum(human_time_ms), 0)::bigint,
                        coalesce(sum(changes_human), 0)::bigint,
                        coalesce(sum(changes_agent), 0)::bigint,
                        coalesce(sum(changes_integration), 0)::bigint
                 FROM usage_tasks
                 WHERE started_at >= now() - ($1::int * interval '1 day')
                 GROUP BY task_type
                 ORDER BY coalesce(sum(human_time_ms), 0) DESC, task_type",
                &[&days],
            )
            .await?;
        let trend_rows = client
            .query(
                "SELECT to_char(date_trunc('day', started_at), 'YYYY-MM-DD'),
                        task_type,
                        count(*)::bigint,
                        coalesce(sum(human_time_ms), 0)::bigint
                 FROM usage_tasks
                 WHERE started_at >= now() - ($1::int * interval '1 day')
                 GROUP BY 1, 2
                 ORDER BY 1, 2",
                &[&days],
            )
            .await?;
        Ok(UsageSummary {
            by_type: by_type_rows
                .into_iter()
                .map(|row| UsageByType {
                    task_type: row.get(0),
                    tasks: row.get(1),
                    human_time_ms: row.get(2),
                    changes_human: row.get(3),
                    changes_agent: row.get(4),
                    changes_integration: row.get(5),
                })
                .collect(),
            trend: trend_rows
                .into_iter()
                .map(|row| UsageTrend {
                    day: row.get(0),
                    task_type: row.get(1),
                    tasks: row.get(2),
                    human_time_ms: row.get(3),
                })
                .collect(),
        })
    }

    /// Synthetic rows so the usage view has something to show. No person columns.
    pub async fn seed_usage_demo(&self) -> Result<(), Error> {
        let client = self.pool().get().await?;
        client
            .execute(
                "INSERT INTO usage_tasks (
                    id, task_type, started_at, ended_at, human_time_ms,
                    changes_human, changes_agent, changes_integration,
                    object_count, event_count, open, ambiguous, classifier
                 ) VALUES
                    ('018f2000-0000-7000-8000-000000000d01', 'enrich', now() - interval '1 day', now() - interval '1 day' + interval '42 minutes', 2520000, 18, 7, 0, 12, 31, false, false, 'rules'),
                    ('018f2000-0000-7000-8000-000000000d02', 'import', now() - interval '2 days', now() - interval '2 days' + interval '15 minutes', 900000, 0, 0, 1, 1000000, 1, false, false, 'rules'),
                    ('018f2000-0000-7000-8000-000000000d03', 'search', now() - interval '2 days', now() - interval '2 days' + interval '6 minutes', 360000, 0, 0, 0, 40, 4, false, false, 'rules'),
                    ('018f2000-0000-7000-8000-000000000d04', 'mass_edit', now() - interval '4 days', now() - interval '4 days' + interval '3 minutes', 180000, 1, 0, 0, 240, 1, false, false, 'rules'),
                    ('018f2000-0000-7000-8000-000000000d05', 'review', now() - interval '6 days', now() - interval '6 days' + interval '11 minutes', 660000, 2, 1, 0, 8, 6, false, false, 'rules'),
                    ('018f2000-0000-7000-8000-000000000d06', 'enrich', now() - interval '6 days', now() - interval '6 days' + interval '20 minutes', 1200000, 9, 2, 0, 5, 14, false, false, 'rules')
                 ON CONFLICT (id) DO UPDATE SET
                    started_at = EXCLUDED.started_at,
                    ended_at = EXCLUDED.ended_at,
                    human_time_ms = EXCLUDED.human_time_ms,
                    changes_human = EXCLUDED.changes_human,
                    changes_agent = EXCLUDED.changes_agent,
                    changes_integration = EXCLUDED.changes_integration,
                    object_count = EXCLUDED.object_count,
                    event_count = EXCLUDED.event_count",
                &[],
            )
            .await?;
        Ok(())
    }

    pub async fn usage_event_audit(
        &self,
        id: Uuid,
    ) -> Result<Option<(Option<Uuid>, Option<i64>)>, Error> {
        let client = self.pool().get().await?;
        let row = client
            .query_opt(
                "SELECT audit_batch_id, audit_seq FROM telemetry_events WHERE id = $1",
                &[&id],
            )
            .await?;
        Ok(row.map(|row| (row.get(0), row.get(1))))
    }
}

const RECOMPUTE: &str = "INSERT INTO usage_tasks AS t (
                id, task_type, started_at, ended_at, human_time_ms,
                changes_human, changes_agent, changes_integration,
                object_count, event_count, open, ambiguous, classifier
             )
             SELECT
                task_id,
                (array_agg(task_type ORDER BY occurred_at DESC))[1],
                min(occurred_at),
                max(occurred_at),
                GREATEST(
                    (EXTRACT(EPOCH FROM (max(occurred_at) - min(occurred_at))) * 1000)::bigint,
                    coalesce(max(duration_ms), 0)::bigint
                ),
                (count(*) FILTER (WHERE is_change AND actor_type = 'human'))::int,
                (count(*) FILTER (WHERE is_change AND actor_type = 'agent'))::int,
                (count(*) FILTER (WHERE is_change AND actor_type = 'integration'))::int,
                max(object_count)::int,
                count(*)::int,
                true,
                bool_or(ambiguous),
                'rules'
             FROM telemetry_events
             WHERE task_id = ANY($1)
             GROUP BY task_id
             ON CONFLICT (id) DO UPDATE SET
                task_type = CASE
                    WHEN t.classifier = 'decisions' THEN t.task_type
                    ELSE EXCLUDED.task_type
                END,
                started_at = EXCLUDED.started_at,
                ended_at = EXCLUDED.ended_at,
                human_time_ms = EXCLUDED.human_time_ms,
                changes_human = EXCLUDED.changes_human,
                changes_agent = EXCLUDED.changes_agent,
                changes_integration = EXCLUDED.changes_integration,
                object_count = EXCLUDED.object_count,
                event_count = EXCLUDED.event_count,
                ambiguous = EXCLUDED.ambiguous,
                classifier = t.classifier";
