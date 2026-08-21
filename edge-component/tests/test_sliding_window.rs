use edge_ai_classifier::ingestion::sliding_window::{SensorReading, SlidingWindow};

fn reading(value: f32) -> SensorReading {
    SensorReading {
        flow_rate: value,
        pressure: value + 100.0,
        vibration: value + 200.0,
        temperature: value + 300.0,
    }
}

#[test]
fn empty_window_is_not_full() {
    let window = SlidingWindow::new();
    assert!(!window.is_full());
}

#[test]
fn window_fills_after_sixty_readings() {
    let mut window = SlidingWindow::new();
    for i in 0..59 {
        window.push(reading(i as f32));
        assert!(
            !window.is_full(),
            "should not be full at {} readings",
            i + 1
        );
    }
    window.push(reading(59.0));
    assert!(window.is_full());
}

#[test]
fn get_window_is_channel_major_oldest_first() {
    let mut window = SlidingWindow::new();
    for i in 0..60 {
        window.push(reading(i as f32));
    }
    let data = window.get_window();

    // Channel 0 (flow_rate): values 0..60, oldest first.
    assert_eq!(data[0], 0.0);
    assert_eq!(data[59], 59.0);
    // Channel 1 (pressure): offset +100.
    assert_eq!(data[60], 100.0);
    assert_eq!(data[119], 159.0);
    // Channel 2 (vibration): offset +200.
    assert_eq!(data[120], 200.0);
    // Channel 3 (temperature): offset +300.
    assert_eq!(data[180], 300.0);
    assert_eq!(data[239], 359.0);
}

#[test]
fn window_evicts_oldest_reading() {
    let mut window = SlidingWindow::new();
    // Push 70 readings; the first 10 should be evicted.
    for i in 0..70 {
        window.push(reading(i as f32));
    }
    let data = window.get_window();
    assert_eq!(data[0], 10.0, "oldest surviving flow_rate should be 10");
    assert_eq!(data[59], 69.0, "newest flow_rate should be 69");
    assert!(window.is_full());
}

#[test]
fn partial_window_zero_pads_the_front() {
    let mut window = SlidingWindow::new();
    window.push(reading(5.0));
    window.push(reading(6.0));
    let data = window.get_window();
    // First 58 slots of channel 0 are padding.
    assert_eq!(data[0], 0.0);
    assert_eq!(data[57], 0.0);
    assert_eq!(data[58], 5.0);
    assert_eq!(data[59], 6.0);
}

#[test]
fn channel_means_average_current_contents() {
    let mut window = SlidingWindow::new();
    window.push(reading(0.0));
    window.push(reading(10.0));
    let means = window.channel_means();
    assert_eq!(means[0], 5.0); // flow_rate: (0 + 10) / 2
    assert_eq!(means[1], 105.0); // pressure: (100 + 110) / 2
    assert_eq!(means[3], 305.0); // temperature
}
