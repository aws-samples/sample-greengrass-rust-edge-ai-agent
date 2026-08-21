use crate::inference::types::AnomalyResult;
use serde::{Deserialize, Serialize};

/// Static device information included in escalations (FR-5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub station_id: String,
    pub installation_date: String,
    pub last_maintenance_date: String,
}

/// Escalation message published to `pump-stations/<thing>/escalations`
/// (FR-5). `sensor_window` is 4x60, channel-major, oldest sample first —
/// the same layout the model consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationMessage {
    pub thing_name: String,
    /// ISO 8601 UTC timestamp of when the anomaly was detected (original
    /// detection time, preserved through offline queueing per FR-4).
    pub timestamp: String,
    pub sensor_window: Vec<Vec<f32>>,
    pub local_classification: AnomalyResult,
    pub confidence: f32,
    pub device_metadata: DeviceMetadata,
}

/// Local alert published to `pump-stations/<thing>/alerts` for confident
/// single-sensor faults (FR-3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAlert {
    pub thing_name: String,
    pub timestamp: String,
    pub classification: AnomalyResult,
}

/// 60-second aggregate published to `pump-stations/<thing>/telemetry`
/// (FR-13). An IoT Rule writes these to DynamoDB to build the historical
/// baseline the cloud agent queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub thing_name: String,
    /// ISO 8601 UTC timestamp of the window close. Doubles as the
    /// DynamoDB sort key (`ts`).
    pub ts: String,
    pub flow_rate: f32,
    pub pressure: f32,
    pub vibration: f32,
    pub temperature: f32,
    /// DynamoDB TTL attribute: epoch seconds, now + 90 days.
    pub expire_at: i64,
}

/// Recommendation received from the cloud agent on
/// `pump-stations/<thing>/recommendations` (FR-6, FR-7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudResponse {
    pub severity: String,
    pub probable_cause: String,
    pub recommended_action: String,
    pub evidence: String,
    #[serde(default)]
    pub estimated_time_to_failure: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::types::{AnomalyResult, AnomalyType};

    #[test]
    fn escalation_message_serializes_to_spec_shape() {
        let msg = EscalationMessage {
            thing_name: "pump-station-042".into(),
            timestamp: "2026-07-17T10:30:00Z".into(),
            sensor_window: vec![vec![0.0; 60]; 4],
            local_classification: AnomalyResult {
                anomaly_type: AnomalyType::MultiSensorCorrelation,
                confidence: 0.55,
                contributing_sensors: vec!["pressure".into(), "vibration".into()],
            },
            confidence: 0.55,
            device_metadata: DeviceMetadata {
                station_id: "STN-042".into(),
                installation_date: "2020-03-15".into(),
                last_maintenance_date: "2026-01-10".into(),
            },
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(json["thing_name"], "pump-station-042");
        assert_eq!(
            json["local_classification"]["anomaly_type"],
            "multi_sensor_correlation"
        );
        assert_eq!(json["sensor_window"].as_array().unwrap().len(), 4);
        assert_eq!(json["sensor_window"][0].as_array().unwrap().len(), 60);
    }

    #[test]
    fn cloud_response_deserializes_agent_json() {
        let json = r#"{
            "severity": "HIGH",
            "probable_cause": "BEARING_WEAR",
            "recommended_action": "SCHEDULE_MAINTENANCE",
            "evidence": "Vibration trending up 12% over 7 days while pressure dropped.",
            "estimated_time_to_failure": "5 days"
        }"#;
        let response: CloudResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.severity, "HIGH");
        assert_eq!(response.probable_cause, "BEARING_WEAR");
        assert_eq!(
            response.estimated_time_to_failure.as_deref(),
            Some("5 days")
        );
    }

    #[test]
    fn cloud_response_tolerates_missing_ttf() {
        let json = r#"{
            "severity": "LOW",
            "probable_cause": "UNKNOWN",
            "recommended_action": "CONTINUE_MONITORING",
            "evidence": "Pattern ambiguous."
        }"#;
        let response: CloudResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.estimated_time_to_failure, None);
    }
}
