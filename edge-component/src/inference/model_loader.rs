//! ONNX model loading with SHA-256 integrity verification (FR-8).

use ort::session::{builder::GraphOptimizationLevel, Session};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum ModelLoadError {
    Io(std::io::Error),
    /// Computed hash did not match the expected value from configuration.
    HashMismatch {
        expected: String,
        actual: String,
    },
    Ort(ort::Error),
}

impl fmt::Display for ModelLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelLoadError::Io(e) => write!(f, "failed to read model file: {e}"),
            ModelLoadError::HashMismatch { expected, actual } => write!(
                f,
                "model integrity check failed: expected sha256 {expected}, got {actual}"
            ),
            ModelLoadError::Ort(e) => write!(f, "failed to create ort session: {e}"),
        }
    }
}

impl std::error::Error for ModelLoadError {}

/// Computes the hex-encoded SHA-256 of a byte buffer.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Loads the ONNX model, verifying its SHA-256 against `expected_sha256`
/// (hex, case-insensitive) before handing the bytes to onnxruntime.
///
/// Verification happens on the exact bytes passed to the runtime
/// (commit_from_memory), so there is no read-then-reopen TOCTOU gap.
pub fn load_verified_model(
    model_path: &Path,
    expected_sha256: &str,
) -> Result<Session, ModelLoadError> {
    let bytes = std::fs::read(model_path).map_err(ModelLoadError::Io)?;

    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(ModelLoadError::HashMismatch {
            expected: expected_sha256.trim().to_string(),
            actual,
        });
    }

    build_session(&bytes).map_err(ModelLoadError::Ort)
}

fn build_session(bytes: &[u8]) -> Result<Session, ort::Error> {
    Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        // Single intra-op thread: the model is tiny and the Cortex-A53
        // target shares cores with other processes (NFR-1/NFR-2).
        .with_intra_threads(1)?
        .commit_from_memory(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
