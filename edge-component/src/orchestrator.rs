//! Coordinates the three async tasks: ingestion → inference → routing,
//! plus telemetry emission (FR-13), offline queueing (FR-4), and cloud
//! response handling (FR-7).
//!
//! Task layout (all on the tokio runtime):
//! - ingestion: drains the sensor channel into the sliding window
//! - inference: classifies each full window, routes per FR-3
//! - communication: publishes with offline fallback, drains the queue on
//!   reconnect, logs cloud recommendations
//!
//! Inference never blocks on communication: routing decisions are sent
//! over an unbounded-in-practice mpsc channel; the communication task owns
//! the offline queue and all transport I/O.

use crate::communication::message_types::{
    CloudResponse, DeviceMetadata, EscalationMessage, LocalAlert, TelemetryRecord,
};
use crate::communication::mqtt::{MqttTransport, TransportError};
use crate::communication::offline_queue::OfflineQueue;
use crate::config::Config;
use crate::inference::classifier::Classifier;
use crate::inference::types::{AnomalyType, Routing, NUM_CHANNELS, WINDOW_SECONDS};
use crate::ingestion::sliding_window::{SensorReading, SlidingWindow};
use std::io::Write;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Messages flowing from the inference task to the communication task.
#[derive(Debug)]
pub enum Outbound {
    Alert(LocalAlert),
    Escalation(EscalationMessage),
    Telemetry(TelemetryRecord),
}

/// ISO 8601 / RFC 3339 UTC timestamp (spec: all timestamps UTC).
pub fn utc_now_iso8601() -> String {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("zero nanosecond is valid")
        .format(&Rfc3339)
        .expect("RFC3339 formatting of UTC time cannot fail")
}

/// TTL for telemetry rows: now + 90 days, epoch seconds (DynamoDB TTL).
fn telemetry_expire_at() -> i64 {
    (OffsetDateTime::now_utc() + time::Duration::days(90)).unix_timestamp()
}

pub struct Orchestrator<T: MqttTransport + 'static> {
    config: Config,
    thing_name: String,
    classifier: Classifier,
    transport: Arc<T>,
}

impl<T: MqttTransport + 'static> Orchestrator<T> {
    pub fn new(
        config: Config,
        thing_name: String,
        classifier: Classifier,
        transport: Arc<T>,
    ) -> Self {
        Self {
            config,
            thing_name,
            classifier,
            transport,
        }
    }

    /// Runs until the sensor channel closes. `responses` carries raw cloud
    /// response payloads from the MQTT subscription.
    pub async fn run(
        mut self,
        mut sensors: mpsc::Receiver<SensorReading>,
        responses: mpsc::Receiver<Vec<u8>>,
    ) {
        let (outbound_tx, outbound_rx) = mpsc::channel::<Outbound>(256);

        let comms = tokio::spawn(communication_task(
            self.transport.clone(),
            self.config.clone(),
            outbound_rx,
        ));
        let resp = tokio::spawn(response_task(
            self.config.recommendation_log_path.clone(),
            responses,
        ));

        // Ingestion + inference loop. Sequential by design: readings arrive
        // at 1 Hz and inference takes <50 ms, so a dedicated inference task
        // would add channel plumbing without changing behavior. The
        // orchestrator classifies every full window (once per second once
        // warm) and emits telemetry when the window rolls over a 60-sample
        // boundary.
        let mut window = SlidingWindow::new();
        let mut readings_seen: u64 = 0;
        // Cooldown ledger: last reading count at which each anomaly type
        // was emitted. Readings arrive at 1 Hz, so reading count ≈ seconds;
        // counting readings (not wall clock) keeps suppression
        // deterministic and independent of ingestion timing.
        let mut cooldown = CooldownLedger::new(self.config.escalation_cooldown_seconds);

        while let Some(reading) = sensors.recv().await {
            window.push(reading);
            readings_seen += 1;

            if !window.is_full() {
                continue;
            }

            let data = window.get_window();
            match self.classifier.classify(&data) {
                Ok(result) => {
                    let timestamp = utc_now_iso8601();
                    match result.routing(self.config.escalation_threshold) {
                        Routing::Ignore => {}
                        Routing::LocalAlert => {
                            if !cooldown.should_emit(result.anomaly_type, readings_seen) {
                                continue;
                            }
                            let alert = LocalAlert {
                                thing_name: self.thing_name.clone(),
                                timestamp,
                                classification: result,
                            };
                            let _ = outbound_tx.send(Outbound::Alert(alert)).await;
                        }
                        Routing::Escalate => {
                            if !cooldown.should_emit(result.anomaly_type, readings_seen) {
                                continue;
                            }
                            let escalation = EscalationMessage {
                                thing_name: self.thing_name.clone(),
                                timestamp,
                                sensor_window: to_rows(&data),
                                confidence: result.confidence,
                                local_classification: result,
                                device_metadata: DeviceMetadata {
                                    station_id: self.config.device_metadata.station_id.clone(),
                                    installation_date: self
                                        .config
                                        .device_metadata
                                        .installation_date
                                        .clone(),
                                    last_maintenance_date: self
                                        .config
                                        .device_metadata
                                        .last_maintenance_date
                                        .clone(),
                                },
                            };
                            let _ = outbound_tx.send(Outbound::Escalation(escalation)).await;
                        }
                    }
                }
                Err(e) => warn!("inference failed: {e}"),
            }

            // FR-13: one aggregate record per full window interval.
            if readings_seen % (WINDOW_SECONDS as u64) == 0 {
                let means = window.channel_means();
                let telemetry = TelemetryRecord {
                    thing_name: self.thing_name.clone(),
                    ts: utc_now_iso8601(),
                    flow_rate: means[0],
                    pressure: means[1],
                    vibration: means[2],
                    temperature: means[3],
                    expire_at: telemetry_expire_at(),
                };
                let _ = outbound_tx.send(Outbound::Telemetry(telemetry)).await;
            }
        }

        // Sensor channel closed: shut down.
        drop(outbound_tx);
        let _ = comms.await;
        resp.abort();
        info!("orchestrator stopped");
    }
}

/// Per-anomaly-type emission cooldown.
///
/// A persistent fault classifies identically on every window (1 Hz), so
/// without suppression one stuck anomaly produces ~86K alerts/escalations
/// per device per day — each escalation fanning out to Lambda + Bedrock
/// agent invocations. This ledger emits the first occurrence immediately
/// and suppresses repeats of the *same* anomaly type until the cooldown
/// has elapsed; a different anomaly type emits immediately (a new,
/// distinct problem should never wait behind an old one's cooldown).
struct CooldownLedger {
    cooldown: u64,
    last_emitted: std::collections::HashMap<AnomalyType, u64>,
}

impl CooldownLedger {
    fn new(cooldown_seconds: u64) -> Self {
        Self {
            cooldown: cooldown_seconds,
            last_emitted: std::collections::HashMap::new(),
        }
    }

    /// Returns true (and records the emission) if this anomaly type is
    /// outside its cooldown window. `now` is the reading counter, which
    /// advances at ~1/second.
    fn should_emit(&mut self, anomaly: AnomalyType, now: u64) -> bool {
        if self.cooldown == 0 {
            return true;
        }
        match self.last_emitted.get(&anomaly) {
            Some(&last) if now.saturating_sub(last) < self.cooldown => {
                debug!(
                    ?anomaly,
                    suppressed_for = self.cooldown - (now - last),
                    "suppressing repeat emission (cooldown)"
                );
                false
            }
            _ => {
                self.last_emitted.insert(anomaly, now);
                true
            }
        }
    }
}

/// Converts the flat channel-major window into the 4x60 nested array of
/// the escalation JSON (FR-5).
fn to_rows(window: &[f32]) -> Vec<Vec<f32>> {
    (0..NUM_CHANNELS)
        .map(|c| window[c * WINDOW_SECONDS..(c + 1) * WINDOW_SECONDS].to_vec())
        .collect()
}

/// Publishes outbound messages; queues escalations while offline (FR-4).
/// Alerts and telemetry are fire-and-forget — only escalations are queued,
/// per the spec.
async fn communication_task<T: MqttTransport>(
    transport: Arc<T>,
    config: Config,
    mut outbound: mpsc::Receiver<Outbound>,
) {
    let mut queue: OfflineQueue<EscalationMessage> = OfflineQueue::new(config.offline_queue_size);

    while let Some(message) = outbound.recv().await {
        // Any successful publish means we're online: drain queued
        // escalations first, in FIFO order, before the new message.
        match message {
            Outbound::Alert(alert) => {
                let payload = serde_json::to_vec(&alert).expect("alert serializes");
                if let Err(e) = transport.publish(&config.mqtt_alert_topic, &payload) {
                    warn!("alert publish failed (not queued): {e}");
                }
            }
            Outbound::Telemetry(record) => {
                let payload = serde_json::to_vec(&record).expect("telemetry serializes");
                if let Err(e) = transport.publish(&config.mqtt_telemetry_topic, &payload) {
                    warn!("telemetry publish failed (not queued): {e}");
                } else {
                    drain_queue(&*transport, &config, &mut queue);
                }
            }
            Outbound::Escalation(escalation) => {
                drain_queue(&*transport, &config, &mut queue);
                let payload = serde_json::to_vec(&escalation).expect("escalation serializes");
                match transport.publish(&config.mqtt_escalation_topic, &payload) {
                    Ok(()) => {}
                    Err(TransportError::Offline(reason)) => {
                        info!("offline ({reason}); queueing escalation");
                        if !queue.push(escalation) {
                            warn!(
                                "offline queue overflow: oldest escalation dropped ({} dropped total)",
                                queue.dropped_count()
                            );
                        }
                    }
                    Err(TransportError::Fatal(reason)) => {
                        warn!("escalation publish failed permanently: {reason}");
                    }
                }
            }
        }
    }
}

/// Attempts to publish queued escalations in FIFO order, stopping at the
/// first failure (still offline).
fn drain_queue<T: MqttTransport + ?Sized>(
    transport: &T,
    config: &Config,
    queue: &mut OfflineQueue<EscalationMessage>,
) {
    while let Some(escalation) = queue.peek() {
        let payload = serde_json::to_vec(escalation).expect("escalation serializes");
        if let Err(e) = transport.publish(&config.mqtt_escalation_topic, &payload) {
            info!("drain stopped, still offline: {e}");
            break;
        }
        queue.pop();
    }
}

/// Receives cloud recommendations, parses, and appends to the HMI log
/// file (FR-7).
async fn response_task(log_path: String, mut responses: mpsc::Receiver<Vec<u8>>) {
    while let Some(payload) = responses.recv().await {
        match serde_json::from_slice::<CloudResponse>(&payload) {
            Ok(response) => {
                info!(
                    severity = %response.severity,
                    cause = %response.probable_cause,
                    action = %response.recommended_action,
                    "cloud recommendation received"
                );
                let line = format!(
                    "{} {}\n",
                    utc_now_iso8601(),
                    serde_json::to_string(&response).expect("response re-serializes")
                );
                if let Err(e) = append_line(&log_path, &line) {
                    warn!("failed to write recommendation log: {e}");
                }
            }
            Err(e) => warn!("unparseable cloud response: {e}"),
        }
    }
}

fn append_line(path: &str, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}
