use edge_ai_classifier::communication::offline_queue::OfflineQueue;

#[test]
fn drains_in_fifo_order() {
    let mut queue = OfflineQueue::new(10);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.pop(), Some(2));
    assert_eq!(queue.pop(), Some(3));
    assert_eq!(queue.pop(), None);
}

#[test]
fn drops_oldest_on_overflow() {
    let mut queue = OfflineQueue::new(3);
    assert!(queue.push(1));
    assert!(queue.push(2));
    assert!(queue.push(3));
    assert!(queue.is_full());
    // Overflow: 1 is dropped, push reports it.
    assert!(!queue.push(4));
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.dropped_count(), 1);
    assert_eq!(queue.pop(), Some(2));
    assert_eq!(queue.pop(), Some(3));
    assert_eq!(queue.pop(), Some(4));
}

#[test]
fn spec_capacity_thousand_messages() {
    let mut queue = OfflineQueue::new(1000);
    for i in 0..1500 {
        queue.push(i);
    }
    assert_eq!(queue.len(), 1000);
    assert_eq!(queue.dropped_count(), 500);
    // Oldest surviving message is 500.
    assert_eq!(queue.pop(), Some(500));
}

#[test]
fn empty_and_len_track_state() {
    let mut queue: OfflineQueue<u8> = OfflineQueue::new(2);
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
    queue.push(9);
    assert!(!queue.is_empty());
    assert_eq!(queue.len(), 1);
}
