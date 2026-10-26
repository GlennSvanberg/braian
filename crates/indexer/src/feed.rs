//! Postgres-backed [`ChangeSource`]. Claims are seq ranges on `changes`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use braian_events::{ChangeEvent, ChangeSource, Lane, LaneClaim};
use braian_store::{ChangeListener, LogEvent, Store};
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(30);
const RENEW_EVERY: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct PgSource {
    store: Store,
    lane: Lane,
    holder: Uuid,
    wake: Arc<ChangeListener>,
}

impl PgSource {
    pub(crate) fn new(store: Store, lane: Lane, wake: Arc<ChangeListener>) -> Self {
        Self {
            store,
            lane,
            holder: Uuid::now_v7(),
            wake,
        }
    }

    /// Keeps `claims` leased until the guard is dropped.
    pub(crate) fn keep_leased(&self, claims: &[i64]) -> LeaseGuard {
        let source = self.clone();
        let claims = claims.to_vec();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(RENEW_EVERY).await;
                for from_seq in &claims {
                    if let Err(error) = ChangeSource::renew(&source, *from_seq).await {
                        tracing::warn!(%error, from_seq, "could not renew index claim");
                    }
                }
            }
        });
        LeaseGuard { task }
    }

    pub(crate) async fn commit_many(&self, claims: &[i64]) -> anyhow::Result<()> {
        for from_seq in claims {
            ChangeSource::commit(self, *from_seq).await?;
        }
        Ok(())
    }
}

pub(crate) struct LeaseGuard {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ChangeSource for PgSource {
    type Error = braian_store::Error;

    async fn pending(&self, limit: usize) -> Result<usize, Self::Error> {
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let pending = self.store.lane_pending(self.lane.as_str(), limit).await?;
        Ok(usize::try_from(pending).unwrap_or(usize::MAX))
    }

    async fn claim(&self, limit: usize) -> Result<Option<LaneClaim>, Self::Error> {
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let Some(claimed) = self
            .store
            .claim_changes(self.lane.as_str(), self.holder, limit, LEASE)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(LaneClaim {
            holder: claimed.holder,
            from_seq: claimed.from_seq,
            through_seq: claimed.through_seq,
            events: claimed.events.into_iter().map(change_event).collect(),
        }))
    }

    async fn commit(&self, from_seq: i64) -> Result<(), Self::Error> {
        match self
            .store
            .commit_claim(self.lane.as_str(), self.holder, from_seq)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.is_claim_lost() => {
                tracing::warn!(
                    lane = self.lane.as_str(),
                    from_seq,
                    "index claim expired; another worker will retry the range"
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn renew(&self, from_seq: i64) -> Result<(), Self::Error> {
        self.store
            .renew_claim(self.lane.as_str(), self.holder, from_seq, LEASE)
            .await
    }

    async fn wait(&self, timeout: Duration) {
        self.wake.wait(timeout).await;
    }
}

fn change_event(event: LogEvent) -> ChangeEvent {
    ChangeEvent {
        seq: event.seq,
        op: event.op,
        object_id: event.object_id,
        version: event.version,
        batch_id: event.batch_id,
        kind: event.kind,
        at: event.at.to_rfc3339(),
    }
}

/// Events pulled from one lane, plus the claims that have not been committed.
pub(crate) struct Inflight {
    source: PgSource,
    buffer: VecDeque<ChangeEvent>,
    claims: Vec<i64>,
}

impl Inflight {
    pub(crate) fn new(source: PgSource) -> Self {
        Self {
            source,
            buffer: VecDeque::new(),
            claims: Vec::new(),
        }
    }

    pub(crate) fn source(&self) -> &PgSource {
        &self.source
    }

    /// The next event. `limit` is how many new rows a fresh claim may take.
    ///
    /// An empty lane waits for a notify or `wait`, whichever comes first, and
    /// does not write a lease.
    pub(crate) async fn next(
        &mut self,
        wait: Duration,
        limit: usize,
    ) -> anyhow::Result<Option<ChangeEvent>> {
        if let Some(event) = self.buffer.pop_front() {
            return Ok(Some(event));
        }
        if ChangeSource::pending(&self.source, 1).await? == 0 {
            self.source.wait(wait).await;
            if ChangeSource::pending(&self.source, 1).await? == 0 {
                return Ok(None);
            }
        }
        let Some(claim) = ChangeSource::claim(&self.source, limit).await? else {
            self.source.wait(wait).await;
            return Ok(None);
        };
        tracing::debug!(
            lane = self.source.lane.as_str(),
            from_seq = claim.from_seq,
            through_seq = claim.through_seq,
            events = claim.events.len(),
            "claimed change range"
        );
        self.claims.push(claim.from_seq);
        self.buffer.extend(claim.events);
        Ok(self.buffer.pop_front())
    }

    pub(crate) fn take_claims(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.claims)
    }

    /// Events already leased. Does not ask Postgres for more.
    pub(crate) fn pop_buffered(&mut self) -> Option<ChangeEvent> {
        self.buffer.pop_front()
    }
}
