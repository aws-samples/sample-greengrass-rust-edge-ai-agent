#!/usr/bin/env bash
# Cross-compile the edge component for aarch64 and extract the binary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${REPO_ROOT}/edge-component/dist"

echo "==> Building aarch64 binary via Docker (this compiles ort + gg_sdk; first run takes a while)"
docker build \
    -f "${REPO_ROOT}/edge-component/Dockerfile" \
    -t edge-ai-classifier-build \
    --target output \
    --output "type=local,dest=${OUT_DIR}" \
    "${REPO_ROOT}/edge-component"

BINARY="${OUT_DIR}/edge-ai-classifier"
[[ -f "${BINARY}" ]] || { echo "ERROR: binary not produced"; exit 1; }

SIZE_MB=$(du -m "${BINARY}" | cut -f1)
echo "==> Binary: ${BINARY} (${SIZE_MB} MB)"
if (( SIZE_MB >= 10 )); then
    echo "WARNING: stripped binary is >= 10 MB (spec target: < 10 MB)"
fi

file "${BINARY}" || true
echo "==> Done. Deploy with scripts/deploy_component.sh"
