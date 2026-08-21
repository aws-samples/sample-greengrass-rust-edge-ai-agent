//! Local ONNX inference (FR-2): classify one sensor window.

use crate::inference::types::{
    AnomalyResult, AnomalyType, NUM_CHANNELS, SENSOR_CHANNELS, WINDOW_SECONDS, WINDOW_SIZE,
};
use ort::session::Session;
use ort::value::Tensor;

/// Wraps an ort session and maps raw model output to AnomalyResult.
pub struct Classifier {
    session: Session,
}

impl Classifier {
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// Runs the model on one channel-major window (see spec: layout is
    /// `[flow_rate[0..60], pressure[0..60], vibration[0..60],
    /// temperature[0..60]]`, matching the model's `[1, 4, 60]` input).
    pub fn classify(&mut self, window: &[f32; WINDOW_SIZE]) -> Result<AnomalyResult, ort::Error> {
        let input = Tensor::from_array(([1usize, NUM_CHANNELS, WINDOW_SECONDS], window.to_vec()))?;
        let outputs = self.session.run(ort::inputs![input])?;
        let (_shape, probs) = outputs[0].try_extract_tensor::<f32>()?;

        let (winner, confidence) = probs
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("model output tensor is non-empty");

        let anomaly_type = AnomalyType::from_class_index(winner);
        let contributing_sensors = if anomaly_type == AnomalyType::Normal {
            Vec::new()
        } else {
            contributing_sensors(window)
        };

        Ok(AnomalyResult {
            anomaly_type,
            confidence,
            contributing_sensors,
        })
    }
}

/// Heuristic for the informational `contributing_sensors` field: channels
/// whose peak deviation from their own window mean exceeds 2x the average
/// deviation across all channels (normalized per channel).
fn contributing_sensors(window: &[f32; WINDOW_SIZE]) -> Vec<String> {
    let mut deviations = [0.0f32; NUM_CHANNELS];
    for (c, deviation) in deviations.iter_mut().enumerate() {
        let channel = &window[c * WINDOW_SECONDS..(c + 1) * WINDOW_SECONDS];
        let mean = channel.iter().sum::<f32>() / WINDOW_SECONDS as f32;
        let spread = channel.iter().map(|v| (v - mean).abs()).fold(0.0, f32::max);
        // Normalize by mean magnitude so channels with different units
        // are comparable; guard against zero baselines.
        *deviation = spread / mean.abs().max(1e-6);
    }
    let avg = deviations.iter().sum::<f32>() / NUM_CHANNELS as f32;
    SENSOR_CHANNELS
        .iter()
        .zip(deviations.iter())
        .filter(|(_, &d)| d > 2.0 * avg && d > 1e-3)
        .map(|(name, _)| name.to_string())
        .collect()
}
