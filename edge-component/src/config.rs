use serde::Deserialize;

/// Component configuration, populated from the Greengrass component
/// configuration (recipe `DefaultConfiguration`, overridable per
/// deployment). The recipe passes it to the process as a JSON document
/// via the GG_CONFIG environment variable or a --config-json CLI argument.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "defaults::escalation_threshold")]
    pub escalation_threshold: f32,
    /// Minimum seconds between repeat alerts/escalations for the same
    /// anomaly type. A persistent fault classifies on every window (1 Hz),
    /// so without this a single stuck anomaly floods the cloud agent —
    /// at ~1 escalation/second that is ~86K Bedrock-bound messages per
    /// device per day. 0 disables suppression (tests only).
    #[serde(default = "defaults::escalation_cooldown_seconds")]
    pub escalation_cooldown_seconds: u64,
    #[serde(default = "defaults::offline_queue_size")]
    pub offline_queue_size: usize,
    #[serde(default = "defaults::sensor_window_seconds")]
    pub sensor_window_seconds: usize,
    /// Expected SHA-256 of the model file (FR-8), hex-encoded.
    pub model_sha256: String,
    pub mqtt_alert_topic: String,
    pub mqtt_escalation_topic: String,
    pub mqtt_telemetry_topic: String,
    pub mqtt_response_topic: String,
    /// Local IPC topic carrying raw sensor readings.
    #[serde(default = "defaults::sensor_ipc_topic")]
    pub sensor_ipc_topic: String,
    /// File where cloud recommendations are appended for the HMI (FR-7).
    #[serde(default = "defaults::recommendation_log_path")]
    pub recommendation_log_path: String,
    #[serde(default)]
    pub device_metadata: DeviceMetadataConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceMetadataConfig {
    #[serde(default)]
    pub station_id: String,
    #[serde(default)]
    pub installation_date: String,
    #[serde(default)]
    pub last_maintenance_date: String,
}

mod defaults {
    pub fn escalation_threshold() -> f32 {
        0.7
    }
    pub fn escalation_cooldown_seconds() -> u64 {
        300
    }
    pub fn offline_queue_size() -> usize {
        1000
    }
    pub fn sensor_window_seconds() -> usize {
        60
    }
    pub fn sensor_ipc_topic() -> String {
        "local/sensors/readings".into()
    }
    pub fn recommendation_log_path() -> String {
        "/tmp/edge-ai-recommendations.log".into()
    }
}

impl Config {
    /// Parses configuration from a JSON string (the component's merged
    /// Greengrass configuration document).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
        "model_sha256": "abc123",
        "mqtt_alert_topic": "pump-stations/t1/alerts",
        "mqtt_escalation_topic": "pump-stations/t1/escalations",
        "mqtt_telemetry_topic": "pump-stations/t1/telemetry",
        "mqtt_response_topic": "pump-stations/t1/recommendations"
    }"#;

    #[test]
    fn minimal_config_gets_spec_defaults() {
        let config = Config::from_json(MINIMAL).unwrap();
        assert_eq!(config.escalation_threshold, 0.7);
        assert_eq!(config.offline_queue_size, 1000);
        assert_eq!(config.sensor_window_seconds, 60);
        assert_eq!(config.sensor_ipc_topic, "local/sensors/readings");
    }

    #[test]
    fn explicit_values_override_defaults() {
        let json = r#"{
            "escalation_threshold": 0.8,
            "offline_queue_size": 50,
            "model_sha256": "def",
            "mqtt_alert_topic": "a",
            "mqtt_escalation_topic": "b",
            "mqtt_telemetry_topic": "c",
            "mqtt_response_topic": "d"
        }"#;
        let config = Config::from_json(json).unwrap();
        assert_eq!(config.escalation_threshold, 0.8);
        assert_eq!(config.offline_queue_size, 50);
    }

    #[test]
    fn missing_required_field_is_an_error() {
        assert!(Config::from_json(r#"{"escalation_threshold": 0.5}"#).is_err());
    }
}
