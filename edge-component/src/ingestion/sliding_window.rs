use crate::inference::types::{NUM_CHANNELS, WINDOW_SECONDS, WINDOW_SIZE};

/// One sensor reading: all four channels sampled at the same second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorReading {
    pub flow_rate: f32,
    pub pressure: f32,
    pub vibration: f32,
    pub temperature: f32,
}

impl SensorReading {
    fn channel(&self, index: usize) -> f32 {
        match index {
            0 => self.flow_rate,
            1 => self.pressure,
            2 => self.vibration,
            3 => self.temperature,
            _ => unreachable!("channel index out of range"),
        }
    }
}

/// Circular buffer holding the last 60 seconds of readings for 4 channels.
///
/// `get_window` returns the channel-major layout the ONNX model expects
/// (see "Sensor window memory layout" in the v2 spec):
/// `[flow_rate[0..60], pressure[0..60], vibration[0..60], temperature[0..60]]`,
/// oldest sample first within each channel.
pub struct SlidingWindow {
    readings: [SensorReading; WINDOW_SECONDS],
    /// Index of the next slot to write.
    head: usize,
    /// Number of readings pushed, capped at WINDOW_SECONDS.
    len: usize,
}

impl SlidingWindow {
    pub fn new() -> Self {
        Self {
            readings: [SensorReading {
                flow_rate: 0.0,
                pressure: 0.0,
                vibration: 0.0,
                temperature: 0.0,
            }; WINDOW_SECONDS],
            head: 0,
            len: 0,
        }
    }

    /// Appends a reading, evicting the oldest once the window is full.
    pub fn push(&mut self, reading: SensorReading) {
        self.readings[self.head] = reading;
        self.head = (self.head + 1) % WINDOW_SECONDS;
        self.len = (self.len + 1).min(WINDOW_SECONDS);
    }

    /// True once 60 readings have been received.
    pub fn is_full(&self) -> bool {
        self.len == WINDOW_SECONDS
    }

    /// Returns the window in channel-major order, oldest sample first.
    ///
    /// Callers should check `is_full()` first; a partially filled window
    /// zero-pads the missing (oldest) samples.
    pub fn get_window(&self) -> [f32; WINDOW_SIZE] {
        let mut window = [0.0f32; WINDOW_SIZE];
        // Oldest reading position: when full, it's `head` (the slot about
        // to be overwritten); when partial, readings start at index 0.
        let start = if self.len == WINDOW_SECONDS {
            self.head
        } else {
            0
        };
        for t in 0..self.len {
            let reading = self.readings[(start + t) % WINDOW_SECONDS];
            // Zero-pad at the front so the most recent sample is always last.
            let slot = WINDOW_SECONDS - self.len + t;
            for c in 0..NUM_CHANNELS {
                window[c * WINDOW_SECONDS + slot] = reading.channel(c);
            }
        }
        window
    }

    /// Per-channel means over the current contents (for telemetry, FR-13).
    /// Returns `[flow_rate, pressure, vibration, temperature]` means.
    pub fn channel_means(&self) -> [f32; NUM_CHANNELS] {
        let mut means = [0.0f32; NUM_CHANNELS];
        if self.len == 0 {
            return means;
        }
        let start = if self.len == WINDOW_SECONDS {
            self.head
        } else {
            0
        };
        for t in 0..self.len {
            let reading = self.readings[(start + t) % WINDOW_SECONDS];
            for (c, mean) in means.iter_mut().enumerate() {
                *mean += reading.channel(c);
            }
        }
        for mean in &mut means {
            *mean /= self.len as f32;
        }
        means
    }
}

impl Default for SlidingWindow {
    fn default() -> Self {
        Self::new()
    }
}
