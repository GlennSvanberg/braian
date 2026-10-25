//! How many bulk jobs may run, and how many may wait.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Semaphore;

/// Caps concurrent bulk jobs and the number waiting for a slot.
///
/// A full gate refuses the request. The job does not start and then sit on
/// the database. Interactive requests use a different Postgres pool; this
/// gate is the second limit, on the work itself.
#[derive(Clone)]
pub struct BulkGate {
    inner: Arc<Inner>,
}

struct Inner {
    permits: Semaphore,
    inflight: AtomicUsize,
    max_inflight: usize,
    chunk: usize,
}

/// A reserved slot. Drop it to free the slot, including when the task is cancelled.
pub struct BulkReservation {
    gate: BulkGate,
}

impl BulkGate {
    pub fn new(max_jobs: usize, queue: usize, chunk: usize) -> Self {
        let max_jobs = max_jobs.max(1);
        let chunk = chunk.max(1);
        Self {
            inner: Arc::new(Inner {
                permits: Semaphore::new(max_jobs),
                inflight: AtomicUsize::new(0),
                max_inflight: max_jobs.saturating_add(queue),
                chunk,
            }),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            positive_env("BRAIAN_BULK_JOBS", 1),
            positive_env("BRAIAN_BULK_QUEUE", 4),
            positive_env("BRAIAN_BULK_CHUNK", 25),
        )
    }

    pub fn chunk(&self) -> usize {
        self.inner.chunk
    }

    /// Reserves a queue slot. `None` means the queue is already full.
    pub fn try_reserve(&self) -> Option<BulkReservation> {
        let mut current = self.inner.inflight.load(Ordering::Acquire);
        loop {
            if current >= self.inner.max_inflight {
                return None;
            }
            match self.inner.inflight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(BulkReservation { gate: self.clone() });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl BulkReservation {
    /// Waits for a running slot, then runs `job`.
    pub async fn finish<F>(self, job: F)
    where
        F: Future<Output = ()>,
    {
        let permit = self
            .gate
            .inner
            .permits
            .acquire()
            .await
            .expect("bulk gate stays open");
        job.await;
        drop(permit);
    }
}

impl Drop for BulkReservation {
    fn drop(&mut self) {
        self.gate.inner.inflight.fetch_sub(1, Ordering::AcqRel);
    }
}

fn positive_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::BulkGate;

    #[tokio::test]
    async fn one_running_job_queues_one_more_and_rejects_the_next() {
        let gate = BulkGate::new(1, 1, 25);
        let first = gate.try_reserve().expect("running slot");
        let second = gate.try_reserve().expect("queue slot");
        assert!(gate.try_reserve().is_none());

        let started = Arc::new(AtomicUsize::new(0));
        let flag = Arc::clone(&started);
        let running = tokio::spawn(async move {
            first
                .finish(async move {
                    flag.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(40)).await;
                })
                .await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(started.load(Ordering::SeqCst), 1);

        let queued = Arc::clone(&started);
        let waiting = tokio::spawn(async move {
            second
                .finish(async move {
                    queued.fetch_add(1, Ordering::SeqCst);
                })
                .await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "the queued job ran beside the first"
        );
        running.await.unwrap();
        waiting.await.unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert!(gate.try_reserve().is_some());
    }
}
