//! Seq-ordered read of `changes` with a durable cursor per lane.
//!
//! `changes` is append-only. Workers lease a range, import it, then mark the
//! range done. The lane cursor moves only across a contiguous done prefix, so
//! a crash replays the open range and does not skip it. `LISTEN braian_changes`
//! wakes consumers; a poll still sees rows if a notify is missed. The notify
//! is once per transaction, from a row trigger gated by a transaction-local flag.

use std::sync::Arc;
use std::time::Duration;

use std::future::poll_fn;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_postgres::types::ToSql;
use uuid::Uuid;

use crate::error::Error;
use crate::ids::Actor;
use crate::session::Store;

pub const CHANGES_CHANNEL: &str = "braian_changes";

const MAX_CLAIM: i64 = 5_000;

/// One row from `changes`, in commit order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEvent {
    pub seq: i64,
    pub op: String,
    pub object_id: Option<Uuid>,
    pub version: Option<i64>,
    pub batch_id: Uuid,
    pub kind: String,
    pub at: DateTime<Utc>,
}

/// A leased range of [`LogEvent`]s. `from_seq` is exclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogClaim {
    pub holder: Uuid,
    pub from_seq: i64,
    pub through_seq: i64,
    pub events: Vec<LogEvent>,
}

/// A dedicated session subscribed to [`CHANGES_CHANNEL`].
///
/// Dropping it stops the session. `wait` returns when a statement inserted
/// into `changes`, when the session dies, or when `timeout` elapses.
pub struct ChangeListener {
    client: tokio_postgres::Client,
    wake: Arc<Notify>,
    driver: JoinHandle<()>,
}

impl ChangeListener {
    pub async fn wait(&self, timeout: Duration) {
        let _session = &self.client;
        let notified = self.wake.notified();
        tokio::pin!(notified);
        tokio::select! {
            () = notified => {}
            () = tokio::time::sleep(timeout) => {}
        }
    }
}

impl Drop for ChangeListener {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

enum ClaimStep {
    Events(LogClaim),
    CaughtUp,
    SkippedHole,
}

impl Store {
    /// Opens a `LISTEN` session on a connection of its own.
    pub async fn listen_changes(&self) -> Result<ChangeListener, Error> {
        let (client, mut connection) = self.config.clone().connect(tokio_postgres::NoTls).await?;
        let wake = Arc::new(Notify::new());
        let notify = Arc::clone(&wake);
        let driver = tokio::spawn(async move {
            loop {
                match poll_fn(|cx| connection.poll_message(cx)).await {
                    Some(Ok(tokio_postgres::AsyncMessage::Notification(_))) => {
                        notify.notify_waiters();
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::warn!(%error, "change listener disconnected");
                        notify.notify_waiters();
                        break;
                    }
                    None => {
                        notify.notify_waiters();
                        break;
                    }
                }
            }
        });
        client
            .batch_execute(&format!("LISTEN {CHANGES_CHANNEL}"))
            .await?;
        Ok(ChangeListener {
            client,
            wake,
            driver,
        })
    }

    /// Rows with `seq` greater than `after`, in commit order.
    pub async fn changes_after(&self, after: i64, limit: i64) -> Result<Vec<LogEvent>, Error> {
        let limit = limit.clamp(1, MAX_CLAIM);
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT seq, op, object_id, version, batch_id, kind, at
                 FROM changes
                 WHERE seq > $1
                 ORDER BY seq
                 LIMIT $2",
                &[&after, &limit],
            )
            .await?;
        Ok(rows.iter().map(log_event).collect())
    }

    /// How many uncommitted rows this lane can see, capped at `limit`.
    ///
    /// This is a read. It does not lease anything, so an idle poll does not
    /// write the cursor.
    pub async fn lane_pending(&self, lane: &str, limit: i64) -> Result<i64, Error> {
        check_lane(lane)?;
        let limit = limit.clamp(1, MAX_CLAIM);
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*)::bigint FROM (
                     SELECT 1 FROM changes
                     WHERE kind = $1
                       AND seq > (SELECT committed_seq FROM index_lane WHERE lane = $1)
                     ORDER BY seq
                     LIMIT $2
                 ) pending",
                &[&lane, &limit],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Last seq this lane has finished. Open claims sit above it.
    pub async fn lane_committed(&self, lane: &str) -> Result<i64, Error> {
        check_lane(lane)?;
        let client = self.pool().get().await?;
        let row = client
            .query_one(
                "SELECT committed_seq FROM index_lane WHERE lane = $1",
                &[&lane],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Leases the next free seq range on `lane`.
    ///
    /// `None` when the lane is caught up. Two callers cannot hold the same
    /// range: the lane row is locked for the claim, and a later worker starts
    /// after every live lease. An expired lease is deleted first, so a dead
    /// worker's range is claimed again.
    pub async fn claim_changes(
        &self,
        lane: &str,
        holder: Uuid,
        limit: i64,
        lease: Duration,
    ) -> Result<Option<LogClaim>, Error> {
        check_lane(lane)?;
        let limit = limit.clamp(1, MAX_CLAIM);
        let mut client = self.pool().get().await?;
        for _ in 0..8 {
            match claim_once(&mut client, lane, holder, limit, lease).await? {
                ClaimStep::Events(claim) => return Ok(Some(claim)),
                ClaimStep::CaughtUp => return Ok(None),
                ClaimStep::SkippedHole => continue,
            }
        }
        Ok(None)
    }

    /// Marks the claim done and advances the cursor across the finished prefix.
    pub async fn commit_claim(
        &self,
        lane: &str,
        holder: Uuid,
        from_seq: i64,
    ) -> Result<i64, Error> {
        check_lane(lane)?;
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        lock_lane(&tx, lane).await?;
        let updated = tx
            .execute(
                "UPDATE index_claim
                 SET done = true
                 WHERE lane = $1 AND from_seq = $2 AND holder = $3 AND NOT done",
                &[&lane, &from_seq, &holder],
            )
            .await?;
        if updated == 0 {
            return Err(Error::ClaimLost {
                lane: lane.to_owned(),
                from_seq,
            });
        }
        let committed = fold_lane(&tx, lane).await?;
        tx.commit().await?;
        Ok(committed)
    }

    /// Extends a live lease. `ClaimLost` when the range was taken back.
    pub async fn renew_claim(
        &self,
        lane: &str,
        holder: Uuid,
        from_seq: i64,
        lease: Duration,
    ) -> Result<(), Error> {
        check_lane(lane)?;
        let millis = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX).max(1);
        let client = self.pool().get().await?;
        let updated = client
            .execute(
                "UPDATE index_claim
                 SET leased_until = now() + ($4::bigint * interval '1 millisecond')
                 WHERE lane = $1 AND from_seq = $2 AND holder = $3 AND NOT done",
                &[&lane, &from_seq, &holder, &millis],
            )
            .await?;
        if updated == 0 {
            return Err(Error::ClaimLost {
                lane: lane.to_owned(),
                from_seq,
            });
        }
        Ok(())
    }

    /// Makes every open lease on `lane` look expired. The next claim replays it.
    pub(crate) async fn expire_claims(&self, lane: &str) -> Result<(), Error> {
        check_lane(lane)?;
        let client = self.pool().get().await?;
        client
            .execute(
                "UPDATE index_claim
                 SET leased_until = now() - interval '1 second'
                 WHERE lane = $1 AND NOT done",
                &[&lane],
            )
            .await?;
        Ok(())
    }

    /// Appends one audit row per id. Tests use this to fill a lane.
    pub(crate) async fn append_log(
        &self,
        actor: &Actor,
        object_ids: &[Option<Uuid>],
    ) -> Result<Vec<i64>, Error> {
        if object_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        let batch = Uuid::now_v7();
        let behalf = actor.on_behalf_of.map(|id| id.as_uuid());
        let kind = actor.kind.as_str();
        tx.execute(
            "INSERT INTO batches (id, actor, on_behalf_of, kind, source)
             VALUES ($1, $2, $3, $4, 'log')",
            &[&batch, &actor.actor.as_uuid(), &behalf, &kind],
        )
        .await?;
        let mut seqs = Vec::with_capacity(object_ids.len());
        for object_id in object_ids {
            let row = tx
                .query_one(
                    "INSERT INTO changes
                        (batch_id, object_id, version, op, actor, on_behalf_of, diff, kind)
                     VALUES ($1, $2, 1, 'update', $3, $4, NULL, $5)
                     RETURNING seq",
                    &[&batch, object_id, &actor.actor.as_uuid(), &behalf, &kind],
                )
                .await?;
            seqs.push(row.get(0));
        }
        tx.commit().await?;
        Ok(seqs)
    }
}

fn check_lane(lane: &str) -> Result<(), Error> {
    if lane == "interactive" || lane == "bulk" {
        Ok(())
    } else {
        Err(Error::Data(format!("unknown change lane {lane}")))
    }
}

async fn lock_lane(tx: &tokio_postgres::Transaction<'_>, lane: &str) -> Result<i64, Error> {
    let row = tx
        .query_one(
            "SELECT committed_seq FROM index_lane WHERE lane = $1 FOR UPDATE",
            &[&lane],
        )
        .await?;
    Ok(row.get(0))
}

async fn claim_once(
    client: &mut tokio_postgres::Client,
    lane: &str,
    holder: Uuid,
    limit: i64,
    lease: Duration,
) -> Result<ClaimStep, Error> {
    let tx = client.transaction().await?;
    let mut cursor = lock_lane(&tx, lane).await?;
    tx.execute(
        "DELETE FROM index_claim
         WHERE lane = $1 AND NOT done AND leased_until < now()",
        &[&lane],
    )
    .await?;
    let held = tx
        .query(
            "SELECT from_seq, through_seq
             FROM index_claim
             WHERE lane = $1
             ORDER BY from_seq",
            &[&lane],
        )
        .await?;
    let mut ceiling: Option<i64> = None;
    for row in &held {
        let from: i64 = row.get(0);
        let through: i64 = row.get(1);
        if from > cursor {
            ceiling = Some(from);
            break;
        }
        if through > cursor {
            cursor = through;
        }
    }
    let upper = ceiling.unwrap_or(i64::MAX);
    if cursor >= upper {
        tx.commit().await?;
        return Ok(ClaimStep::CaughtUp);
    }
    let rows = tx
        .query(
            "SELECT seq, op, object_id, version, batch_id, kind, at
             FROM changes
             WHERE kind = $1 AND seq > $2 AND seq <= $3
             ORDER BY seq
             LIMIT $4",
            &[&lane, &cursor, &upper, &limit],
        )
        .await?;
    if rows.is_empty() {
        if let Some(end) = ceiling {
            tx.execute(
                "INSERT INTO index_claim
                    (lane, from_seq, through_seq, holder, leased_until, done)
                 VALUES ($1, $2, $3, $4, now(), true)",
                &[&lane, &cursor, &end, &holder],
            )
            .await?;
            fold_lane(&tx, lane).await?;
            tx.commit().await?;
            return Ok(ClaimStep::SkippedHole);
        }
        tx.commit().await?;
        return Ok(ClaimStep::CaughtUp);
    }
    let events: Vec<LogEvent> = rows.iter().map(log_event).collect();
    let through = events.last().map(|event| event.seq).unwrap_or(cursor);
    let millis = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX).max(1);
    let params: [&(dyn ToSql + Sync); 5] = [&lane, &cursor, &through, &holder, &millis];
    tx.execute(
        "INSERT INTO index_claim
            (lane, from_seq, through_seq, holder, leased_until, done)
         VALUES ($1, $2, $3, $4, now() + ($5::bigint * interval '1 millisecond'), false)",
        &params,
    )
    .await?;
    tx.commit().await?;
    Ok(ClaimStep::Events(LogClaim {
        holder,
        from_seq: cursor,
        through_seq: through,
        events,
    }))
}

async fn fold_lane(tx: &tokio_postgres::Transaction<'_>, lane: &str) -> Result<i64, Error> {
    loop {
        let row = tx
            .query_opt(
                "SELECT c.from_seq, c.through_seq
                 FROM index_claim c
                 JOIN index_lane l ON l.lane = c.lane
                 WHERE c.lane = $1 AND c.done AND c.from_seq = l.committed_seq
                 FOR UPDATE OF c",
                &[&lane],
            )
            .await?;
        let Some(row) = row else {
            break;
        };
        let from: i64 = row.get(0);
        let through: i64 = row.get(1);
        tx.execute(
            "UPDATE index_lane SET committed_seq = $2 WHERE lane = $1",
            &[&lane, &through],
        )
        .await?;
        tx.execute(
            "DELETE FROM index_claim WHERE lane = $1 AND from_seq = $2",
            &[&lane, &from],
        )
        .await?;
    }
    let row = tx
        .query_one(
            "SELECT committed_seq FROM index_lane WHERE lane = $1",
            &[&lane],
        )
        .await?;
    Ok(row.get(0))
}

fn log_event(row: &tokio_postgres::Row) -> LogEvent {
    LogEvent {
        seq: row.get(0),
        op: row.get(1),
        object_id: row.get(2),
        version: row.get(3),
        batch_id: row.get(4),
        kind: row.get(5),
        at: row.get(6),
    }
}
