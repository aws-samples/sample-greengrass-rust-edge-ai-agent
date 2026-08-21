//! Local IPC subscription for sensor readings (FR-1).
//!
//! Sensor gateways publish JSON readings to a local pub/sub topic
//! (`local/sensors/readings` by default). The Greengrass SDK delivers them
//! via a synchronous callback on its receive thread; this module parses
//! them and forwards into a tokio channel for the ingestion task.

use crate::ingestion::sliding_window::SensorReading;
use serde::Deserialize;

/// Wire format of one sensor message on the local IPC topic.
#[derive(Debug, Deserialize)]
pub struct SensorMessage {
    pub flow_rate: f32,
    pub pressure: f32,
    pub vibration: f32,
    pub temperature: f32,
}

impl From<SensorMessage> for SensorReading {
    fn from(msg: SensorMessage) -> Self {
        SensorReading {
            flow_rate: msg.flow_rate,
            pressure: msg.pressure,
            vibration: msg.vibration,
            temperature: msg.temperature,
        }
    }
}

/// Parses a raw IPC payload into a SensorReading. Malformed payloads are
/// reported as errors so the caller can count/log them without crashing
/// the ingestion path.
pub fn parse_sensor_payload(payload: &[u8]) -> Result<SensorReading, serde_json::Error> {
    serde_json::from_slice::<SensorMessage>(payload).map(Into::into)
}

#[cfg(feature = "greengrass")]
pub mod greengrass {
    //! Real IPC subscription backed by `gg_sdk` (Linux / on-device only).

    use super::parse_sensor_payload;
    use crate::communication::mqtt::TransportError;
    use crate::ingestion::sliding_window::SensorReading;
    use gg_sdk::{Sdk, SubscribeToTopicPayload};
    use tokio::sync::mpsc;

    /// Subscribes to the local sensor topic, forwarding parsed readings
    /// into a tokio channel. Callback and subscription are leaked for the
    /// process lifetime (single subscription at startup), matching the
    /// SDK's requirement that the callback outlive the subscription.
    pub fn subscribe_sensors(
        sdk: Sdk,
        topic: &str,
    ) -> Result<mpsc::Receiver<SensorReading>, TransportError> {
        let (tx, rx) = mpsc::channel(256);
        // The SDK requires a Sized `F: Fn` outliving the subscription;
        // leak the concrete closure for the process lifetime.
        let callback = Box::leak(Box::new(
            move |_topic: &str, payload: SubscribeToTopicPayload| {
                let bytes: &[u8] = match &payload {
                    SubscribeToTopicPayload::Binary(bytes) => bytes,
                    // JSON-typed messages are re-serialized upstream as
                    // binary by convention; skip structured payloads.
                    SubscribeToTopicPayload::Json(_) => return,
                };
                if let Ok(reading) = parse_sensor_payload(bytes) {
                    // Drop on backpressure rather than block the SDK thread.
                    let _ = tx.try_send(reading);
                }
            },
        ));
        let subscription = sdk
            .subscribe_to_topic(topic, callback)
            .map_err(|e| TransportError::Fatal(format!("IPC subscribe failed: {e}")))?;
        std::mem::forget(subscription);
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_sensor_json() {
        let payload =
            br#"{"flow_rate": 101.5, "pressure": 49.8, "vibration": 1.02, "temperature": 20.3}"#;
        let reading = parse_sensor_payload(payload).unwrap();
        assert_eq!(reading.flow_rate, 101.5);
        assert_eq!(reading.temperature, 20.3);
    }

    #[test]
    fn rejects_malformed_payload() {
        assert!(parse_sensor_payload(b"not json").is_err());
        assert!(parse_sensor_payload(br#"{"flow_rate": 1.0}"#).is_err());
    }
}
