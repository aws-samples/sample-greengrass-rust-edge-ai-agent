#!/usr/bin/env bash
# Latency benchmarks (Criterion) + peak-RSS measurement.
#
# Criterion measures time, not memory (v2 spec change #11), so RSS is
# captured here with /usr/bin/time. Run on the target aarch64 device for
# NFR-1/NFR-2 numbers; host runs give relative numbers only.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}/edge-component"

MODEL="${REPO_ROOT}/model/sample_model.onnx"
HASH="$(tr -d '[:space:]' < "${REPO_ROOT}/model/model_hash.sha256")"

echo "==> Inference latency (Criterion, real quantized model)"
MODEL_PATH="${MODEL}" MODEL_SHA256="${HASH}" cargo bench 2>&1 | grep -A3 'classify_window'

echo ""
echo "==> Peak RSS (model load + 1000 inferences via the test harness)"
# gtime = GNU time from brew coreutils/gnu-time on macOS; /usr/bin/time -v on Linux.
if command -v gtime >/dev/null; then TIME_CMD=(gtime -v);
elif /usr/bin/time -v true 2>/dev/null; then TIME_CMD=(/usr/bin/time -v);
else TIME_CMD=(/usr/bin/time -l); fi

"${TIME_CMD[@]}" cargo test --release --test test_classifier 2>&1 \
    | grep -iE 'maximum resident|peak memory' || true

echo ""
echo "==> Record results in benchmarks/README.md"
