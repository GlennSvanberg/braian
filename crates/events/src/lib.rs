//! Change events the indexer, the live stream and webhooks consume.
//!
//! A message is "object X changed", not the values. The same `seq` can be
//! delivered twice after a crash. Consumers re-read the current Postgres row
//! and ignore an older sequence.
//!
//! [`ChangeSource`] is the transport seam. The only implementation reads the
//! `changes` table by sequence and stores its cursor in Postgres. A different
//! transport would claim and commit the same way. This crate does not ship one.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which stream a change belongs to. Interactive work is not queued behind bulk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Interactive,
    Bulk,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Bulk => "bulk",
        }
    }

    pub fn matches(self, kind: &str) -> bool {
        kind == self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub seq: i64,
    pub op: String,
    pub object_id: Option<Uuid>,
    pub version: Option<i64>,
    pub batch_id: Uuid,
    pub kind: String,
    pub at: String,
}

impl ChangeEvent {
    pub fn lane(&self) -> Lane {
        if self.kind == "bulk" {
            Lane::Bulk
        } else {
            Lane::Interactive
        }
    }

    pub fn key(&self) -> String {
        self.object_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("seq-{}", self.seq))
    }
}

/// One leased seq range. `from_seq` is exclusive and `through_seq` is inclusive.
///
/// The range stays uncommitted until [`ChangeSource::commit`]. A crash drops
/// the lease and the next worker claims the same sequences again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneClaim {
    pub holder: Uuid,
    pub from_seq: i64,
    pub through_seq: i64,
    pub events: Vec<ChangeEvent>,
}

/// Durable, lane-scoped read of the change log.
///
/// Claiming a range must not double-assign sequences that another live worker
/// already holds. Committing is monotonic per lane: a later range does not
/// hide an earlier one that is still open. `pending` is a cheap read used to
/// decide whether a claim is worth taking. `wait` returns when new rows may
/// exist, or when `timeout` elapses.
pub trait ChangeSource {
    type Error: std::error::Error + Send + Sync + 'static;

    fn pending(&self, limit: usize) -> impl Future<Output = Result<usize, Self::Error>> + Send;

    fn claim(
        &self,
        limit: usize,
    ) -> impl Future<Output = Result<Option<LaneClaim>, Self::Error>> + Send;

    fn commit(&self, from_seq: i64) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn renew(&self, from_seq: i64) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn wait(&self, timeout: Duration) -> impl Future<Output = ()> + Send;
}
