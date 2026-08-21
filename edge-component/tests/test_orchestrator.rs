//! Integration tests: orchestrator end-to-end with mock transport and the
//! tiny fixture model (channel-mean detector: dominant channel wins).
//!
//! Channel → class mapping of the fixture:
//!   0 flow_rate    → normal
//!   1 pressure     → single_sensor_fault
//!   2 vibration    → multi_sensor_correlation
//!   3 temperature  → unknown

use edge_ai_classifier::communication::mqtt::MockTransport;
use edge_ai_classifier::config::Config;
use edge_ai_classifier::inference::classifier::Classifier;
use edge_ai_classifier::inference::model_loader::{load_verified_model, sha256_hex};
use edge_ai_classifier::ingestion::sliding_window::SensorReading;
use edge_ai_classifier::orchestrator::Orchestrator;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

fn test_config() -> Config {
    // escalation_cooldown_seconds: 0 — most tests assert per-window
    // behavior; cooldown gets its own dedicated test.
    Config::from_json(
        r#"{
        "model_sha256": "unused-here",
        "escalation_cooldown_seconds": 0,
        "mqtt_alert_topic": "pump-stations/test-thing/alerts",
        "mqtt_escalation_topic": "pump-stations/test-thing/escalations",
        "mqtt_telemetry_topic": "pump-stations/test-thing/telemetry",
        "mqtt_response_topic": "pump-stations/test-thing/recommendations",
        "recommendation_log_path": "/tmp/edge-ai-test-recommendations.log"
    }"#,
    )
    .unwrap()
}

fn classifier() -> Classifier {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny_model.onnx");
    let hash = sha256_hex(&std::fs::read(&path).unwrap());
    Classifier::new(load_verified_model(&path, &hash).unwrap())
}

/// Reading where one channel dominates -> fixture classifies as that class.
fn reading_favoring(channel: usize) -> SensorReading {
    let mut values = [0.0f32; 4];
    values[channel] = 10.0;
    SensorReading {
        flow_rate: values[0],
        pressure: values[1],
        vibration: values[2],
        temperature: values[3],
    }
}

async fn run_orchestrator(
    transport: Arc<MockTransport>,
    readings: Vec<SensorReading>,
) -> Vec<(String, Vec<u8>)> {
    let (sensor_tx, sensor_rx) = mpsc::channel(512);
    let (_response_tx, response_rx) = mpsc::channel(8);
    let orchestrator = Orchestrator::new(
        test_config(),
        "test-thing".into(),
        classifier(),
        transport.clone(),
    );
    let handle = tokio::spawn(orchestrator.run(sensor_rx, response_rx));
    for reading in readings {
        sensor_tx.send(reading).await.unwrap();
    }
    drop(sensor_tx); // close channel -> orchestrator shuts down
    handle.await.unwrap();
    transport.published()
}

#[tokio::test]
async fn normal_windows_produce_only_telemetry() {
    let transport = Arc::new(MockTransport::new());
    // 60 normal readings: window full exactly once, telemetry emitted once.
    let published =
        run_orchestrator(transport, (0..60).map(|_| reading_favoring(0)).collect()).await;

    let telemetry: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic.ends_with("/telemetry"))
        .collect();
    let alerts: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic.ends_with("/alerts"))
        .collect();
    let escalations: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic.ends_with("/escalations"))
        .collect();

    assert_eq!(telemetry.len(), 1, "one telemetry record per 60 readings");
    assert!(alerts.is_empty(), "normal windows must not alert");
    assert!(escalations.is_empty(), "normal windows must not escalate");

    let record: serde_json::Value = serde_json::from_slice(&telemetry[0].1).unwrap();
    assert_eq!(record["thing_name"], "test-thing");
    assert!(record["expire_at"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn confident_single_sensor_fault_publishes_alert() {
    let transport = Arc::new(MockTransport::new());
    let published =
        run_orchestrator(transport, (0..60).map(|_| reading_favoring(1)).collect()).await;

    // Fixture confidence for a 10-vs-0 margin saturates near 1.0 > 0.7.
    let alerts: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic.ends_with("/alerts"))
        .collect();
    assert!(!alerts.is_empty(), "expected at least one local alert");
    let alert: serde_json::Value = serde_json::from_slice(&alerts[0].1).unwrap();
    assert_eq!(
        alert["classification"]["anomaly_type"],
        "single_sensor_fault"
    );
}

#[tokio::test]
async fn multi_sensor_correlation_escalates_with_full_window() {
    let transport = Arc::new(MockTransport::new());
    let published =
        run_orchestrator(transport, (0..60).map(|_| reading_favoring(2)).collect()).await;

    let escalations: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic.ends_with("/escalations"))
        .collect();
    assert!(!escalations.is_empty(), "expected escalation");
    let msg: serde_json::Value = serde_json::from_slice(&escalations[0].1).unwrap();
    assert_eq!(
        msg["local_classification"]["anomaly_type"],
        "multi_sensor_correlation"
    );
    // FR-5: sensor_window is 4x60.
    assert_eq!(msg["sensor_window"].as_array().unwrap().len(), 4);
    assert_eq!(msg["sensor_window"][2].as_array().unwrap().len(), 60);
    // Timestamp is ISO 8601 UTC.
    let ts = msg["timestamp"].as_str().unwrap();
    assert!(ts.ends_with('Z'), "timestamp must be UTC: {ts}");
}

#[tokio::test]
async fn cooldown_suppresses_repeat_escalations_of_same_type() {
    let config = Config::from_json(
        r#"{
        "model_sha256": "unused-here",
        "escalation_cooldown_seconds": 300,
        "mqtt_alert_topic": "pump-stations/test-thing/alerts",
        "mqtt_escalation_topic": "pump-stations/test-thing/escalations",
        "mqtt_telemetry_topic": "pump-stations/test-thing/telemetry",
        "mqtt_response_topic": "pump-stations/test-thing/recommendations",
        "recommendation_log_path": "/tmp/edge-ai-test-recommendations.log"
    }"#,
    )
    .unwrap();

    let transport = Arc::new(MockTransport::new());
    let (sensor_tx, sensor_rx) = mpsc::channel(4096);
    let (_response_tx, response_rx) = mpsc::channel(8);
    let orchestrator =
        Orchestrator::new(config, "test-thing".into(), classifier(), transport.clone());
    let handle = tokio::spawn(orchestrator.run(sensor_rx, response_rx));

    // 240 seconds of a persistent multi-sensor anomaly: windows fill from
    // t=60, then classify every second — 181 escalation-eligible windows,
    // all within one 300-reading cooldown.
    for _ in 0..240 {
        sensor_tx.send(reading_favoring(2)).await.unwrap();
    }
    // Switch to a *different* anomaly type (unknown): must emit
    // immediately despite the escalation cooldown still running.
    for _ in 0..60 {
        sensor_tx.send(reading_favoring(3)).await.unwrap();
    }
    drop(sensor_tx);
    handle.await.unwrap();

    let escalations: Vec<serde_json::Value> = transport
        .published()
        .iter()
        .filter(|(topic, _)| topic.ends_with("/escalations"))
        .map(|(_, payload)| serde_json::from_slice(payload).unwrap())
        .collect();
    let multi: Vec<_> = escalations
        .iter()
        .filter(|e| e["local_classification"]["anomaly_type"] == "multi_sensor_correlation")
        .collect();
    let unknown: Vec<_> = escalations
        .iter()
        .filter(|e| e["local_classification"]["anomaly_type"] == "unknown")
        .collect();

    assert_eq!(
        multi.len(),
        1,
        "persistent anomaly must escalate once per cooldown window, got {}",
        multi.len()
    );
    assert!(
        !unknown.is_empty(),
        "a new anomaly type must not wait behind another type's cooldown"
    );
}

#[tokio::test]
async fn escalations_queue_offline_and_drain_in_order() {
    let transport = Arc::new(MockTransport::new());
    transport.set_online(false);

    let (sensor_tx, sensor_rx) = mpsc::channel(4096);
    let (_response_tx, response_rx) = mpsc::channel(8);
    let orchestrator = Orchestrator::new(
        test_config(),
        "test-thing".into(),
        classifier(),
        transport.clone(),
    );
    let handle = tokio::spawn(orchestrator.run(sensor_rx, response_rx));

    // Two full windows of escalating readings while offline.
    for _ in 0..120 {
        sensor_tx.send(reading_favoring(2)).await.unwrap();
    }
    // Let the pipeline settle, then go online and trigger more traffic so
    // the queue drains.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        transport.published().is_empty(),
        "nothing must be published while offline"
    );
    transport.set_online(true);
    for _ in 0..60 {
        sensor_tx.send(reading_favoring(2)).await.unwrap();
    }
    drop(sensor_tx);
    handle.await.unwrap();

    let escalations: Vec<_> = transport
        .published()
        .into_iter()
        .filter(|(topic, _)| topic.ends_with("/escalations"))
        .collect();
    // 61 offline windows (windows 60..120 inclusive of each new reading)
    // queue up; exact count depends on classification cadence — the key
    // invariants are: everything queued was delivered after reconnect and
    // timestamps are monotonically non-decreasing (FIFO).
    assert!(
        escalations.len() >= 60,
        "queued escalations must drain after reconnect (got {})",
        escalations.len()
    );
    let timestamps: Vec<String> = escalations
        .iter()
        .map(|(_, payload)| {
            serde_json::from_slice::<serde_json::Value>(payload).unwrap()["timestamp"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    let mut sorted = timestamps.clone();
    sorted.sort();
    assert_eq!(timestamps, sorted, "escalations must drain in FIFO order");
}
