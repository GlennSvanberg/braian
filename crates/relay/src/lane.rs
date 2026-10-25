//! Interactive and bulk publishes run on separate channels so an interactive
//! event is not waiting on a bulk produce. The replication slot still advances
//! only through the oldest event that Kafka has accepted.

use std::collections::VecDeque;

use tokio::sync::mpsc;

/// How many lane batches may sit in each publisher queue.
pub const INTERACTIVE_QUEUE: usize = 8;
pub const BULK_QUEUE: usize = 4;

/// Events pulled off the replication socket before the relay yields to publish.
pub const FETCH_LIMIT: usize = 1_000;
pub const BULK_BATCH: usize = 400;
pub const INTERACTIVE_BATCH: usize = 32;

#[derive(Debug)]
pub struct LaneBatch<T> {
    pub items: Vec<T>,
    pub tokens: Vec<u64>,
}

pub struct LaneSenders<T> {
    interactive: mpsc::Sender<LaneBatch<T>>,
    bulk: mpsc::Sender<LaneBatch<T>>,
}

impl<T: Send> LaneSenders<T> {
    pub fn pair(interactive: mpsc::Sender<LaneBatch<T>>, bulk: mpsc::Sender<LaneBatch<T>>) -> Self {
        Self { interactive, bulk }
    }

    /// Queues interactive work first. A full bulk queue cannot hold that send.
    pub async fn submit(&self, interactive: LaneBatch<T>, bulk: LaneBatch<T>) -> Result<(), ()> {
        if !interactive.items.is_empty() {
            self.interactive.send(interactive).await.map_err(|_| ())?;
        }
        if !bulk.items.is_empty() {
            self.bulk.send(bulk).await.map_err(|_| ())?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct AckSlot {
    token: u64,
    lsn: u64,
    done: bool,
}

/// In-order acknowledgement of replication LSNs.
#[derive(Debug, Default)]
pub struct AckQueue {
    next_token: u64,
    slots: VecDeque<AckSlot>,
}

impl AckQueue {
    pub fn push(&mut self, lsn: u64) -> u64 {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        self.slots.push_back(AckSlot {
            token,
            lsn,
            done: false,
        });
        token
    }

    /// Marks `token` done and returns the LSN the slot may now confirm, if any.
    pub fn finish(&mut self, token: u64) -> Option<u64> {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.token == token) {
            slot.done = true;
        }
        let mut applied = None;
        while self.slots.front().is_some_and(|slot| slot.done) {
            applied = self.slots.pop_front().map(|slot| slot.lsn);
        }
        applied
    }
}

/// Postgres drops a slot with `wal_removed` once `max_slot_wal_keep_size` is exceeded.
pub fn slot_invalidated_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("invalidat")
        || message.contains("wal_removed")
        || message.contains("wal was removed")
        || message.contains("no longer get changes")
        || message.contains("exceeded the maximum reserved")
}

/// Prometheus code for `pg_replication_slots.wal_status`.
/// 0 missing, 1 reserved, 2 extended, 3 unreserved, 4 lost, 5 other.
pub fn wal_status_code(status: Option<&str>) -> u8 {
    match status {
        None => 0,
        Some("reserved") => 1,
        Some("extended") => 2,
        Some("unreserved") => 3,
        Some("lost") => 4,
        Some(_) => 5,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;

    #[test]
    fn applied_lsn_stops_at_the_oldest_unacked_event() {
        let mut queue = AckQueue::default();
        let bulk = queue.push(10);
        let interactive = queue.push(20);
        assert_eq!(queue.finish(interactive), None);
        assert_eq!(queue.finish(bulk), Some(20));
    }

    #[test]
    fn finishing_the_oldest_event_confirms_only_that_lsn() {
        let mut queue = AckQueue::default();
        let bulk = queue.push(10);
        let _interactive = queue.push(20);
        assert_eq!(queue.finish(bulk), Some(10));
    }

    #[test]
    fn slot_loss_messages_are_recognized() {
        assert!(slot_invalidated_message(
            "slot braian_relay was invalidated because the required WAL was removed"
        ));
        assert!(slot_invalidated_message(
            "can no longer get changes from replication slot \"braian_relay\""
        ));
        assert!(slot_invalidated_message("invalidation_reason wal_removed"));
        assert!(!slot_invalidated_message("connection reset by peer"));
    }

    #[test]
    fn wal_status_codes_match_the_metric() {
        assert_eq!(wal_status_code(None), 0);
        assert_eq!(wal_status_code(Some("reserved")), 1);
        assert_eq!(wal_status_code(Some("extended")), 2);
        assert_eq!(wal_status_code(Some("unreserved")), 3);
        assert_eq!(wal_status_code(Some("lost")), 4);
    }

    #[tokio::test]
    async fn interactive_is_delivered_while_bulk_is_still_publishing() {
        let (interactive_tx, mut interactive_rx) = mpsc::channel(INTERACTIVE_QUEUE);
        let (bulk_tx, mut bulk_rx) = mpsc::channel(1);
        let senders = LaneSenders::pair(interactive_tx, bulk_tx);
        let order = Arc::new(Mutex::new(Vec::new()));

        let bulk_order = Arc::clone(&order);
        let bulk_worker = tokio::spawn(async move {
            while let Some(batch) = bulk_rx.recv().await {
                tokio::time::sleep(Duration::from_millis(150)).await;
                bulk_order
                    .lock()
                    .expect("order")
                    .push(("bulk", batch.items[0]));
            }
        });
        let interactive_order = Arc::clone(&order);
        let interactive_worker = tokio::spawn(async move {
            while let Some(batch) = interactive_rx.recv().await {
                interactive_order
                    .lock()
                    .expect("order")
                    .push(("interactive", batch.items[0]));
            }
        });

        senders
            .submit(
                LaneBatch {
                    items: Vec::new(),
                    tokens: Vec::new(),
                },
                LaneBatch {
                    items: vec![1u32],
                    tokens: vec![1],
                },
            )
            .await
            .expect("bulk submit");
        // Let the bulk worker pick up the batch and enter its slow publish.
        tokio::time::sleep(Duration::from_millis(20)).await;
        senders
            .submit(
                LaneBatch {
                    items: vec![2u32],
                    tokens: vec![2],
                },
                LaneBatch {
                    items: Vec::new(),
                    tokens: Vec::new(),
                },
            )
            .await
            .expect("interactive submit");

        let deadline = std::time::Instant::now() + Duration::from_millis(80);
        let mut saw_interactive = false;
        while std::time::Instant::now() < deadline {
            saw_interactive = order
                .lock()
                .expect("order")
                .iter()
                .any(|(lane, _)| *lane == "interactive");
            if saw_interactive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let seen = order.lock().expect("order").clone();
        assert!(saw_interactive, "interactive waited behind bulk: {seen:?}");
        assert!(
            seen.iter().all(|(lane, _)| *lane != "bulk"),
            "bulk finished before the interactive event: {seen:?}"
        );

        drop(senders);
        let _ = bulk_worker.await;
        let _ = interactive_worker.await;
    }
}
