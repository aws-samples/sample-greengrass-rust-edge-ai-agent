//! Rust Greengrass component: local ONNX anomaly classification for pump
//! station sensors, with cloud escalation for complex anomalies.
//!
//! Library crate so integration tests and benchmarks can exercise the
//! internals; `main.rs` is a thin binary wrapper.

pub mod communication;
pub mod config;
pub mod inference;
pub mod ingestion;
pub mod orchestrator;
