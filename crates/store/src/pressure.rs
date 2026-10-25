//! Bulk work yields while interactive reads and writes are in flight.
//!
//! The bulk pool can stay large enough for an import. A click still gets the
//! CPU, because the next chunk waits — up to a cap, so the import cannot sit
//! out the whole click stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Largest slice a bulk lane starts while an interactive request is still running.
///
/// Idle imports keep the caller's chunk. A multi-hundred-row COPY that starts
/// beside a PATCH is what pushed interactive latency back up when the pool grew.
pub(crate) const PRESSURE_CHUNK_CAP: usize = 32;

/// How long a bulk chunk waits, and how many rows it may start with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BulkPace {
    pub pause: Duration,
    pub chunk: usize,
}

/// Knobs for [`bulk_pace`]. `max_pause` bounds the wait while interactive
/// requests are running. The idle pause is [`YieldLimits::base`] and is not
/// pulled down by that cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct YieldLimits {
    pub base: Duration,
    pub step: Duration,
    pub max_pause: Duration,
}

/// In-flight interactive reads and writes in this process.
///
/// Cloned stores share the counter, including [`crate::Store::on_bulk`].
#[derive(Clone, Debug)]
pub struct InteractiveLoad {
    inflight: Arc<AtomicUsize>,
}

impl InteractiveLoad {
    pub(crate) fn new() -> Self {
        Self {
            inflight: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn enter(&self) -> InteractiveGuard {
        self.inflight.fetch_add(1, Ordering::AcqRel);
        InteractiveGuard {
            inflight: Arc::clone(&self.inflight),
        }
    }

    pub(crate) fn inflight(&self) -> usize {
        self.inflight.load(Ordering::Acquire)
    }
}

/// Drops the interactive count, including when the request task is cancelled.
#[must_use = "the interactive count drops when the guard is dropped"]
pub struct InteractiveGuard {
    inflight: Arc<AtomicUsize>,
}

impl Drop for InteractiveGuard {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Pause and chunk size for one bulk slice.
///
/// `configured_chunk` is the caller's cap. With nothing interactive in flight
/// the pause is `limits.base` and the chunk stays at that cap. Each in-flight
/// request adds `limits.step`, and the pause stops at `limits.max_pause` so a
/// steady click stream slows the import instead of parking it.
pub(crate) fn bulk_pace(inflight: usize, configured_chunk: usize, limits: YieldLimits) -> BulkPace {
    let configured = configured_chunk.max(1);
    if inflight == 0 {
        return BulkPace {
            pause: limits.base,
            chunk: configured,
        };
    }
    let steps = u32::try_from(inflight).unwrap_or(u32::MAX);
    let pause = limits
        .base
        .saturating_add(limits.step.saturating_mul(steps))
        .min(limits.max_pause);
    let shrunk = configured / inflight.saturating_add(1);
    BulkPace {
        pause,
        chunk: shrunk.clamp(1, PRESSURE_CHUNK_CAP),
    }
}

/// `true` once a bulk loop has waited long enough for this interactive load.
pub(crate) fn bulk_wait_done(inflight: usize, elapsed: Duration, limits: YieldLimits) -> bool {
    elapsed >= bulk_pace(inflight, 1, limits).pause
}

/// Yields, then sleeps until the interactive count drops or the cap is hit.
///
/// The sleep is polled in short slices so a request that finishes early
/// releases the bulk lane without waiting out the whole pause.
pub(crate) async fn pause_for_bulk(load: &InteractiveLoad, limits: YieldLimits) {
    tokio::task::yield_now().await;
    let started = Instant::now();
    loop {
        let inflight = load.inflight();
        if bulk_wait_done(inflight, started.elapsed(), limits) {
            return;
        }
        let target = bulk_pace(inflight, 1, limits).pause;
        let slice = target
            .saturating_sub(started.elapsed())
            .min(Duration::from_millis(2));
        if slice.is_zero() {
            return;
        }
        tokio::time::sleep(slice).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> YieldLimits {
        YieldLimits {
            base: Duration::from_millis(1),
            step: Duration::from_millis(4),
            max_pause: Duration::from_millis(25),
        }
    }

    #[test]
    fn idle_bulk_keeps_the_configured_chunk_and_the_base_pause() {
        let pace = bulk_pace(0, 250, limits());
        assert_eq!(pace.chunk, 250);
        assert_eq!(pace.pause, Duration::from_millis(1));
    }

    #[test]
    fn one_interactive_request_shrinks_a_large_slice_and_adds_one_step() {
        let pace = bulk_pace(1, 250, limits());
        assert_eq!(pace.chunk, PRESSURE_CHUNK_CAP);
        assert_eq!(pace.pause, Duration::from_millis(5));
    }

    #[test]
    fn a_modest_chunk_shrinks_without_hitting_the_cap() {
        let pace = bulk_pace(1, 25, limits());
        assert_eq!(pace.chunk, 12);
        assert!(pace.chunk < PRESSURE_CHUNK_CAP);
    }

    #[test]
    fn pause_never_exceeds_the_cap_while_interactive_work_is_in_flight() {
        let pace = bulk_pace(10_000, 250, limits());
        assert_eq!(pace.pause, limits().max_pause);
        assert_eq!(pace.chunk, 1);
    }

    #[test]
    fn the_cap_does_not_shorten_an_idle_pause() {
        let limits = YieldLimits {
            base: Duration::from_millis(100),
            step: Duration::ZERO,
            max_pause: Duration::from_millis(25),
        };
        assert_eq!(bulk_pace(0, 10, limits).pause, Duration::from_millis(100));
        assert_eq!(bulk_pace(1, 10, limits).pause, Duration::from_millis(25));
    }

    #[test]
    fn a_zero_cap_does_not_wait_but_still_shrinks_the_chunk() {
        let limits = YieldLimits {
            base: Duration::from_millis(1),
            step: Duration::from_millis(4),
            max_pause: Duration::ZERO,
        };
        let pace = bulk_pace(3, 40, limits);
        assert_eq!(pace.pause, Duration::ZERO);
        assert_eq!(pace.chunk, 10);
    }

    #[test]
    fn waiting_is_done_at_the_cap_and_when_the_load_drops() {
        let limits = limits();
        assert!(!bulk_wait_done(5, Duration::from_millis(10), limits));
        assert!(bulk_wait_done(5, Duration::from_millis(25), limits));
        assert!(bulk_wait_done(0, Duration::from_millis(1), limits));
        assert!(!bulk_wait_done(0, Duration::ZERO, limits));
    }

    #[test]
    fn dropping_the_guard_clears_the_shared_gauge() {
        let load = InteractiveLoad::new();
        let first = load.enter();
        let second = load.enter();
        let cloned = load.clone();
        assert_eq!(cloned.inflight(), 2);
        drop(first);
        assert_eq!(cloned.inflight(), 1);
        drop(second);
        assert_eq!(load.inflight(), 0);
    }

    #[tokio::test]
    async fn bulk_pause_ends_when_the_interactive_request_finishes() {
        let load = InteractiveLoad::new();
        let guard = load.enter();
        let limits = YieldLimits {
            base: Duration::from_millis(1),
            step: Duration::from_millis(4),
            max_pause: Duration::from_millis(200),
        };
        let waiting = load.clone();
        let task = tokio::spawn(async move {
            pause_for_bulk(&waiting, limits).await;
        });
        tokio::time::sleep(Duration::from_millis(15)).await;
        drop(guard);
        let started = Instant::now();
        task.await.expect("pause task");
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "bulk kept waiting after the interactive request finished"
        );
    }

    #[tokio::test]
    async fn bulk_pause_is_bounded_when_interactive_work_does_not_finish() {
        let load = InteractiveLoad::new();
        let _guard = load.enter();
        let limits = YieldLimits {
            base: Duration::ZERO,
            step: Duration::from_secs(1),
            max_pause: Duration::from_millis(40),
        };
        let started = Instant::now();
        pause_for_bulk(&load, limits).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(30),
            "pause returned before the cap: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "pause ignored the cap: {elapsed:?}"
        );
    }
}
