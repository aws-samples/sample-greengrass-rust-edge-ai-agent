//! MQTT to AWS IoT Core via the Greengrass IPC MQTT proxy.
//!
//! Per the v2 spec, the component holds no credentials and opens no direct
//! MQTT connection: publish/subscribe goes through the Nucleus using the
//! `aws.greengrass.ipc.mqttproxy` operations authorized in the recipe.
//!
//! The Greengrass SDK (`gg_sdk`) is synchronous and callback-based, so this
//! module exposes a small sync `MqttTransport` trait the async orchestrator
//! calls from its communication task, and bridges incoming subscription
//! callbacks into a tokio channel. The real implementation is gated behind
//! the `greengrass` feature (the SDK's C bindings only build on Linux);
//! tests use `MockTransport`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// MQTT/IPC connection unavailable — callers should queue and retry.
    Offline(String),
    /// Non-retryable failure (bad topic, unauthorized, payload too large).
    Fatal(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Offline(msg) => write!(f, "transport offline: {msg}"),
            TransportError::Fatal(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Publish-side abstraction over the Greengrass MQTT proxy.
pub trait MqttTransport: Send + Sync {
    /// Publishes `payload` to `topic` at QoS 1 (AtLeastOnce).
    fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), TransportError>;
}

/// In-memory transport for tests: records publishes and can simulate
/// connectivity loss.
pub struct MockTransport {
    published: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
    online: std::sync::atomic::AtomicBool,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            published: std::sync::Mutex::new(Vec::new()),
            online: std::sync::atomic::AtomicBool::new(true),
        }
    }

    pub fn set_online(&self, online: bool) {
        self.online
            .store(online, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn published(&self) -> Vec<(String, Vec<u8>)> {
        self.published.lock().unwrap().clone()
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MqttTransport for MockTransport {
    fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), TransportError> {
        if !self.online.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TransportError::Offline("mock offline".into()));
        }
        self.published
            .lock()
            .unwrap()
            .push((topic.to_string(), payload.to_vec()));
        Ok(())
    }
}

#[cfg(feature = "greengrass")]
pub mod greengrass {
    //! Real transport backed by `gg_sdk` (Linux / on-device only).

    use super::{MqttTransport, TransportError};
    use gg_sdk::{Qos, Sdk};
    use tokio::sync::mpsc;

    pub struct GreengrassTransport {
        sdk: Sdk,
    }

    impl GreengrassTransport {
        /// Connects to the Nucleus IPC socket using the SVCUID and
        /// AWS_GG_NUCLEUS_DOMAIN_SOCKET_FILEPATH_FOR_COMPONENT environment
        /// variables Greengrass injects into the component process.
        pub fn connect() -> Result<Self, TransportError> {
            let sdk = Sdk::init();
            sdk.connect()
                .map_err(|e| TransportError::Fatal(format!("IPC connect failed: {e}")))?;
            Ok(Self { sdk })
        }

        pub fn sdk(&self) -> Sdk {
            self.sdk
        }

        /// Subscribes to the cloud response topic, forwarding raw payloads
        /// into a tokio channel the orchestrator consumes.
        ///
        /// The SDK invokes callbacks from its own receive thread and the
        /// callback must outlive the subscription, so both are leaked for
        /// the life of the process (the component subscribes exactly once
        /// at startup).
        pub fn subscribe_responses(
            &self,
            topic: &str,
        ) -> Result<mpsc::Receiver<Vec<u8>>, TransportError> {
            let (tx, rx) = mpsc::channel(64);
            // The SDK requires a Sized `F: Fn` that outlives the
            // subscription; leak the concrete closure for the process
            // lifetime (subscribed exactly once at startup).
            let callback = Box::leak(Box::new(move |_topic: &str, payload: &[u8]| {
                // try_send: if the orchestrator is behind, drop rather than
                // block the SDK's receive thread.
                let _ = tx.try_send(payload.to_vec());
            }));
            let subscription = self
                .sdk
                .subscribe_to_iot_core(topic, Qos::AtLeastOnce, callback)
                .map_err(|e| TransportError::Fatal(format!("subscribe failed: {e}")))?;
            std::mem::forget(subscription);
            Ok(rx)
        }
    }

    impl MqttTransport for GreengrassTransport {
        fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), TransportError> {
            self.sdk
                .publish_to_iot_core(topic, payload, Qos::AtLeastOnce)
                .map_err(|e| match e {
                    gg_sdk::Error::Noconn | gg_sdk::Error::Retry | gg_sdk::Error::Timeout => {
                        TransportError::Offline(e.to_string())
                    }
                    other => TransportError::Fatal(other.to_string()),
                })
        }
    }
}
