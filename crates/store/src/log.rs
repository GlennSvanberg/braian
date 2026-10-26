//! Seq-ordered read of `changes` with a durable cursor per lane.
//!
//! `changes` is append-only. `seq` is allocated at insert, so a transaction
//! that commits later can hold a lower seq than one that already committed.
//! A claim, a hole fill, and the live tail move forward only across a prefix
//! whose missing seqs are guaranteed aborted. While any other transaction is
//! in progress, a missing seq stops the prefix. Workers lease a range, import
//! it, then mark the range done. The lane cursor moves only across a
//! contiguous done prefix, so a crash replays the open range and does not
//! skip it. `LISTEN braian_changes` wakes consumers; a poll still sees rows
//! if a notify is missed. The notify is once per transaction, from a row
//! trigger gated by a transaction-local flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use std::future::poll_fn;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_postgres::IsolationLevel;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::ToSql;
use uuid::Uuid;

use crate::error::Error;
use crate::ids::Actor;
use crate::session::Store;

pub const CHANGES_CHANNEL: &str = "braian_changes";

const MAX_CLAIM: i64 = 5_000;

/// True when some other backend in this database holds a transaction id.
///
/// `pg_snapshot_xip` does not list that transaction when it is the oldest
/// active xid: the snapshot collapses to `xmin:xmax:` with an empty list, on
/// Postgres 16, even after the other backend has inserted. `backend_xid` is
/// the signal that matches an open insert. Transactions in other databases
/// are ignored; they cannot write this log.
///
/// `pg_stat_activity` is a snapshot taken on the first read in a transaction.
/// A later statement sees the same `backend_xid` set until
/// `pg_stat_clear_snapshot()` runs. Each check clears it in its own statement,
/// then reads. The check that treats a hole as aborted runs after the page
/// (or `max(seq)`) has been read. A check before that read is stale: a
/// transaction can allocate a lower seq in between and still be open.
///
/// That window includes the insert itself. The first write of a loader is the
/// `changes` insert. Identity `nextval` assigns `seq` inside that statement,
/// and `backend_xid` appears when the statement assigns the transaction id.
/// A read that finishes before the insert can miss the backend. The remaining
/// race is that single statement, not the time between the activity read and
/// the page.
const OTHERS_OPEN: &str = "SELECT EXISTS (
    SELECT 1
    FROM pg_stat_activity
    WHERE datname = current_database()
      AND pid <> pg_backend_pid()
      AND backend_xid IS NOT NULL
) OR EXISTS (
    SELECT 1
    FROM pg_prepared_xacts
    WHERE database = current_database()
)";

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

    /// Rows with `seq` greater than `after`, in seq order, stopping before a
    /// gap that an open transaction might still fill.
    pub async fn changes_after(&self, after: i64, limit: i64) -> Result<Vec<LogEvent>, Error> {
        let limit = limit.clamp(1, MAX_CLAIM);
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        // The first activity read only opens the window. `visible_page`
        // decides after the rows are loaded, with a fresh snapshot.
        let _stale = others_open(&tx).await?;
        before_page().await;
        let page = load_page(&tx, after, i64::MAX, limit).await?;
        let events = visible_page(&tx, after, i64::MAX, limit, &page).await?;
        tx.commit().await?;
        Ok(events)
    }

    /// Highest `seq` a live tail may start at without skipping a row that can
    /// still commit.
    ///
    /// With no other transaction in progress, that is `max(seq)`: every gap
    /// below it is an aborted insert. Otherwise it is the last seq of the
    /// contiguous prefix, and the tail waits in front of the first hole.
    pub async fn safe_change_seq(&self) -> Result<i64, Error> {
        let mut client = self.pool().get().await?;
        let tx = client.transaction().await?;
        // Same window as a claim: the bool from before the tip read is stale.
        let _stale = others_open(&tx).await?;
        before_page().await;
        let tip = max_seq(&tx).await?;
        if !others_open(&tx).await? && max_seq(&tx).await? == tip {
            tx.commit().await?;
            return Ok(tip);
        }
        tx.commit().await?;
        let tx = client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await?;
        let tip = max_seq(&tx).await?;
        if tip == 0 {
            tx.commit().await?;
            return Ok(0);
        }
        let mut lo = 0i64;
        let mut hi = tip;
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            let row = tx
                .query_one(
                    "SELECT count(*)::bigint FROM changes WHERE seq > $1 AND seq <= $2",
                    &[&lo, &mid],
                )
                .await?;
            let present: i64 = row.get(0);
            if present == mid - lo {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        tx.commit().await?;
        Ok(lo)
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
        let mut skips = 0;
        let mut conflicts = 0;
        while skips < 8 && conflicts < 8 {
            match claim_once(&mut client, lane, holder, limit, lease).await {
                Ok(ClaimStep::Events(claim)) => return Ok(Some(claim)),
                Ok(ClaimStep::CaughtUp) => return Ok(None),
                Ok(ClaimStep::SkippedHole) => skips += 1,
                Err(error) if serialization_failure(&error) => conflicts += 1,
                Err(error) => return Err(error),
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
    lock_lane(&tx, lane).await?;
    tx.execute(
        "DELETE FROM index_claim
         WHERE lane = $1 AND NOT done AND leased_until < now()",
        &[&lane],
    )
    .await?;
    let mut cursor = fold_lane(&tx, lane).await?;
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
    let head = next_seq(&tx, cursor, upper).await?;
    let leading_hole = head.is_none_or(|seq| seq != cursor.saturating_add(1));
    // An open COPY already holds lower seqs. Stay put without reading a page.
    // A quiet result here is not permission to skip: the snapshot is stale
    // the moment it returns, and a transaction can take the next seq after it.
    if leading_hole && others_open(&tx).await? {
        if head.is_some() {
            tracing::debug!(
                lane,
                cursor,
                head,
                "change log stopped at a seq an open transaction can still fill"
            );
        }
        tx.commit().await?;
        return Ok(ClaimStep::CaughtUp);
    }
    before_page().await;
    // `head` was read before the window. A seq allocated in that window is
    // only on this page.
    let mut page = load_page(&tx, cursor, upper, MAX_CLAIM).await?;
    if page.is_empty() {
        page = load_page(&tx, cursor, upper, MAX_CLAIM).await?;
        if page.is_empty() {
            // Both reads missed every seq up to the ceiling. Close the hole
            // only when a fresh activity read, after those reads, is quiet.
            if !others_open(&tx).await?
                && let Some(end) = ceiling
            {
                close_hole(&tx, lane, holder, cursor, end).await?;
                tx.commit().await?;
                return Ok(ClaimStep::SkippedHole);
            }
            tx.commit().await?;
            return Ok(ClaimStep::CaughtUp);
        }
    }
    let prefix = if safe_prefix(cursor, &page, true).len() != page.len() {
        visible_page(&tx, cursor, upper, MAX_CLAIM, &page).await?
    } else {
        page
    };
    let mut lane_events = Vec::new();
    let mut through = cursor;
    for event in prefix {
        through = event.seq;
        if event.kind == lane {
            lane_events.push(event);
            if lane_events.len() as i64 >= limit {
                break;
            }
        }
    }
    if through <= cursor {
        tx.commit().await?;
        return Ok(ClaimStep::CaughtUp);
    }
    if lane_events.is_empty() {
        close_hole(&tx, lane, holder, cursor, through).await?;
        tx.commit().await?;
        return Ok(ClaimStep::SkippedHole);
    }
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
        events: lane_events,
    }))
}

async fn others_open(tx: &tokio_postgres::Transaction<'_>) -> Result<bool, Error> {
    // Its own statement. Folding the clear into `OTHERS_OPEN` would still see
    // the snapshot that statement started with.
    tx.batch_execute("SELECT pg_stat_clear_snapshot()").await?;
    let row = tx.query_one(OTHERS_OPEN, &[]).await?;
    Ok(row.get(0))
}

tokio::task_local! {
    static PAGE_GATE: Arc<PageGate>;
}

struct PageGate {
    entered: AtomicBool,
    released: AtomicBool,
    entered_wake: Notify,
    release_wake: Notify,
}

/// Pauses one change-log read after its first activity check and before the
/// page, on the task that polls the read.
///
/// Integration tests build this crate without `cfg(test)`. The gate is
/// task-local so a pause in one test does not stop another database's claim.
pub struct PageGap {
    gate: Arc<PageGate>,
}

impl PageGap {
    pub fn arm() -> Self {
        Self {
            gate: Arc::new(PageGate {
                entered: AtomicBool::new(false),
                released: AtomicBool::new(false),
                entered_wake: Notify::new(),
                release_wake: Notify::new(),
            }),
        }
    }

    /// Polls `read` on a new task with this gate armed.
    pub fn drive<T: Send + 'static>(
        &self,
        read: impl std::future::Future<Output = T> + Send + 'static,
    ) -> JoinHandle<T> {
        let gate = Arc::clone(&self.gate);
        tokio::spawn(async move { PAGE_GATE.scope(gate, read).await })
    }

    pub async fn wait_entered(&self) {
        loop {
            if self.gate.entered.load(Ordering::SeqCst) {
                return;
            }
            let notified = self.gate.entered_wake.notified();
            tokio::pin!(notified);
            if self.gate.entered.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    pub fn release(&self) {
        self.gate.released.store(true, Ordering::SeqCst);
        self.gate.release_wake.notify_waiters();
    }
}

impl Drop for PageGap {
    fn drop(&mut self) {
        self.release();
    }
}

/// Test hook between the first activity read and the page. Production calls
/// return immediately. A gate pauses only the first time it is entered.
async fn before_page() {
    let Some(gate) = PAGE_GATE.try_with(Arc::clone).ok() else {
        return;
    };
    if gate.entered.swap(true, Ordering::SeqCst) {
        return;
    }
    gate.entered_wake.notify_waiters();
    loop {
        if gate.released.load(Ordering::SeqCst) {
            return;
        }
        let notified = gate.release_wake.notified();
        tokio::pin!(notified);
        if gate.released.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

async fn max_seq(tx: &tokio_postgres::Transaction<'_>) -> Result<i64, Error> {
    let row = tx
        .query_one("SELECT COALESCE(MAX(seq), 0) FROM changes", &[])
        .await?;
    Ok(row.get(0))
}

async fn next_seq(
    tx: &tokio_postgres::Transaction<'_>,
    cursor: i64,
    upper: i64,
) -> Result<Option<i64>, Error> {
    let rows = tx
        .query(
            "SELECT seq FROM changes WHERE seq > $1 AND seq <= $2 ORDER BY seq LIMIT 1",
            &[&cursor, &upper],
        )
        .await?;
    Ok(rows.first().map(|row| row.get(0)))
}

async fn load_page(
    tx: &tokio_postgres::Transaction<'_>,
    cursor: i64,
    upper: i64,
    limit: i64,
) -> Result<Vec<LogEvent>, Error> {
    let rows = tx
        .query(
            "SELECT seq, op, object_id, version, batch_id, kind, at
             FROM changes
             WHERE seq > $1 AND seq <= $2
             ORDER BY seq
             LIMIT $3",
            &[&cursor, &upper, &limit],
        )
        .await?;
    Ok(rows.iter().map(log_event).collect())
}

/// `page` was just read. A hole in it is aborted only when this database stays
/// quiet and a second read returns the same seqs. A transaction that starts
/// later can only take a seq above those already allocated.
async fn visible_page(
    tx: &tokio_postgres::Transaction<'_>,
    cursor: i64,
    upper: i64,
    limit: i64,
    page: &[LogEvent],
) -> Result<Vec<LogEvent>, Error> {
    let contiguous = safe_prefix(cursor, page, true);
    if contiguous.len() == page.len() {
        return Ok(contiguous.to_vec());
    }
    if others_open(tx).await? {
        return Ok(contiguous.to_vec());
    }
    let again = load_page(tx, cursor, upper, limit).await?;
    if others_open(tx).await? || !same_seqs(page, &again) {
        return Ok(safe_prefix(cursor, &again, true).to_vec());
    }
    Ok(safe_prefix(cursor, &again, false).to_vec())
}

fn same_seqs(left: &[LogEvent], right: &[LogEvent]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.seq == b.seq)
}

async fn close_hole(
    tx: &tokio_postgres::Transaction<'_>,
    lane: &str,
    holder: Uuid,
    cursor: i64,
    end: i64,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO index_claim
            (lane, from_seq, through_seq, holder, leased_until, done)
         VALUES ($1, $2, $3, $4, now(), true)",
        &[&lane, &cursor, &end, &holder],
    )
    .await?;
    fold_lane(tx, lane).await?;
    Ok(())
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

fn serialization_failure(error: &Error) -> bool {
    matches!(
        error,
        Error::Db(inner) if inner.code() == Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    )
}

/// Prefix of `rows` the cursor may advance through.
///
/// `rows` is ordered by seq. A missing seq is aborted only when no other
/// transaction is in progress. Identity values are not reused, and a new
/// transaction can only take a seq above every seq already allocated, so a
/// quiet snapshot cannot grow a hole backwards.
fn safe_prefix(cursor: i64, rows: &[LogEvent], others_active: bool) -> &[LogEvent] {
    let mut expected = cursor.saturating_add(1);
    let mut end = 0;
    for event in rows {
        if event.seq != expected && (others_active || expected == i64::MAX) {
            break;
        }
        end += 1;
        expected = event.seq.saturating_add(1);
    }
    &rows[..end]
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn event(seq: i64) -> LogEvent {
        LogEvent {
            seq,
            op: "update".into(),
            object_id: None,
            version: Some(1),
            batch_id: Uuid::nil(),
            kind: "interactive".into(),
            at: Utc.timestamp_opt(0, 0).single().expect("epoch"),
        }
    }

    #[test]
    fn an_open_gap_stops_the_prefix() {
        let rows = vec![event(4), event(5)];
        assert!(safe_prefix(0, &rows, true).is_empty());
        assert_eq!(safe_prefix(3, &rows, true).len(), 2);
        let gapped = vec![event(1), event(4)];
        assert_eq!(safe_prefix(0, &gapped, true).len(), 1);
    }

    #[test]
    fn a_quiet_snapshot_crosses_an_aborted_gap() {
        let rows = vec![event(1), event(4), event(5)];
        let prefix = safe_prefix(0, &rows, false);
        assert_eq!(
            prefix.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![1, 4, 5]
        );
    }
}
