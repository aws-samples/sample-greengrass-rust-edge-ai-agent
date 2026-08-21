use std::collections::VecDeque;

/// Bounded FIFO queue for escalation messages while MQTT is unavailable
/// (FR-4). Drop-oldest on overflow so the freshest anomalies survive a
/// long outage.
///
/// Not lock-free by design (v2 spec): the orchestrator owns the queue and
/// the inference task never blocks on it — non-blocking behavior comes
/// from task structure, not from the data structure.
pub struct OfflineQueue<T> {
    items: VecDeque<T>,
    capacity: usize,
    /// Total messages dropped due to overflow, for observability.
    dropped: u64,
}

impl<T> OfflineQueue<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "queue capacity must be positive");
        Self {
            items: VecDeque::with_capacity(capacity),
            capacity,
            dropped: 0,
        }
    }

    /// Enqueues a message. Returns `false` if the oldest message was
    /// dropped to make room.
    pub fn push(&mut self, item: T) -> bool {
        let overflowed = self.items.len() == self.capacity;
        if overflowed {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);
        !overflowed
    }

    /// Dequeues the oldest message (FIFO drain order per FR-4).
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// Borrows the oldest message without removing it. Lets the drain loop
    /// publish first and pop only on success, preserving FIFO order when
    /// the transport is still offline.
    pub fn peek(&self) -> Option<&T> {
        self.items.front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.items.len() == self.capacity
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }
}
