use serde::{Deserialize, Serialize};

/// Sensor channels, in the fixed order used throughout the component.
///
/// This order defines the channel-major layout of the model input tensor
/// `[1, 4, 60]`: flow_rate, pressure, vibration, temperature.
pub const SENSOR_CHANNELS: [&str; 4] = ["flow_rate", "pressure", "vibration", "temperature"];

/// Number of sensor channels.
pub const NUM_CHANNELS: usize = 4;

/// Sliding window length in seconds (one reading per second per channel).
pub const WINDOW_SECONDS: usize = 60;

/// Total datapoints in one model input: 4 channels x 60 seconds.
pub const WINDOW_SIZE: usize = NUM_CHANNELS * WINDOW_SECONDS;

/// Classification produced by the local ONNX model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyType {
    Normal,
    SingleSensorFault,
    MultiSensorCorrelation,
    Unknown,
}

impl AnomalyType {
    /// Maps a model output class index to an anomaly type.
    ///
    /// The model's output layer is `[normal, single_sensor_fault,
    /// multi_sensor_correlation, unknown]`; anything out of range is
    /// treated as `Unknown` so a model/binary version skew degrades to
    /// escalation rather than a panic.
    pub fn from_class_index(index: usize) -> Self {
        match index {
            0 => AnomalyType::Normal,
            1 => AnomalyType::SingleSensorFault,
            2 => AnomalyType::MultiSensorCorrelation,
            _ => AnomalyType::Unknown,
        }
    }
}

/// Result of running the classifier on one sensor window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyResult {
    pub anomaly_type: AnomalyType,
    /// Softmax probability of the winning class, in [0, 1].
    pub confidence: f32,
    /// Channels whose readings deviate most from the window mean;
    /// informational, included in alerts and escalations.
    pub contributing_sensors: Vec<String>,
}

/// Routing decision derived from an AnomalyResult per FR-3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    /// anomaly_type == normal: do nothing.
    Ignore,
    /// single_sensor_fault with confidence > threshold: local alert.
    LocalAlert,
    /// multi_sensor_correlation OR unknown OR confidence < threshold.
    Escalate,
}

impl AnomalyResult {
    /// FR-3 routing: normal → ignore; single_sensor_fault above the
    /// escalation threshold → local alert; multi_sensor_correlation,
    /// unknown, or low confidence → escalate to the cloud agent.
    pub fn routing(&self, escalation_threshold: f32) -> Routing {
        match self.anomaly_type {
            AnomalyType::Normal => Routing::Ignore,
            AnomalyType::SingleSensorFault if self.confidence > escalation_threshold => {
                Routing::LocalAlert
            }
            _ => Routing::Escalate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_is_ignored_regardless_of_confidence() {
        let result = AnomalyResult {
            anomaly_type: AnomalyType::Normal,
            confidence: 0.1,
            contributing_sensors: vec![],
        };
        assert_eq!(result.routing(0.7), Routing::Ignore);
    }

    #[test]
    fn confident_single_sensor_fault_alerts_locally() {
        let result = AnomalyResult {
            anomaly_type: AnomalyType::SingleSensorFault,
            confidence: 0.9,
            contributing_sensors: vec!["pressure".into()],
        };
        assert_eq!(result.routing(0.7), Routing::LocalAlert);
    }

    #[test]
    fn low_confidence_single_sensor_fault_escalates() {
        let result = AnomalyResult {
            anomaly_type: AnomalyType::SingleSensorFault,
            confidence: 0.5,
            contributing_sensors: vec![],
        };
        assert_eq!(result.routing(0.7), Routing::Escalate);
    }

    #[test]
    fn multi_sensor_correlation_escalates_even_when_confident() {
        let result = AnomalyResult {
            anomaly_type: AnomalyType::MultiSensorCorrelation,
            confidence: 0.99,
            contributing_sensors: vec![],
        };
        assert_eq!(result.routing(0.7), Routing::Escalate);
    }

    #[test]
    fn unknown_escalates_even_when_confident() {
        // FR-3 (v2): unknown escalates regardless of confidence.
        let result = AnomalyResult {
            anomaly_type: AnomalyType::Unknown,
            confidence: 0.95,
            contributing_sensors: vec![],
        };
        assert_eq!(result.routing(0.7), Routing::Escalate);
    }

    #[test]
    fn out_of_range_class_index_maps_to_unknown() {
        assert_eq!(AnomalyType::from_class_index(7), AnomalyType::Unknown);
    }
}
