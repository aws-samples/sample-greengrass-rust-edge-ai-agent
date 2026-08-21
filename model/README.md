# Anomaly classification model

A small 1D-CNN with channel attention that classifies one 60-second sensor
window into four classes: `normal`, `single_sensor_fault`,
`multi_sensor_correlation`, `unknown`.

## Interface

| | Shape | Notes |
|---|---|---|
| Input `sensor_window` | `[1, 4, 60]` f32 | Channel-major: flow_rate, pressure, vibration, temperature; oldest sample first. Raw sensor units — normalization is baked into the graph. |
| Output `class_probabilities` | `[1, 4]` f32 | Softmax over `[normal, single_sensor_fault, multi_sensor_correlation, unknown]`. |

## Architecture

`(x - baseline) / noise` normalization → Conv1d(4→32, k5) → MaxPool →
Conv1d(32→64, k5) → MaxPool → squeeze-and-excitation channel attention →
global average pool → Linear(64→4) → Softmax. ~25k parameters; int8-quantized
export is ~23 KB.

> The blog's use case describes a ~12 MB production model (10M parameters).
> This sample model is deliberately tiny so the repo stays lightweight and
> training takes seconds on a laptop; the component code paths (loading,
> hash verification, inference, routing) are identical regardless of model
> size.

## Files

- `train_model.py` — generates synthetic pump-station data (same failure
  patterns as `simulator/anomaly_injector.py`), trains, exports fp32 ONNX
- `quantize_model.py` — int8 dynamic quantization + writes `model_hash.sha256`
- `sample_model.onnx` — pre-built quantized model (committed so the sample
  works without a training step)
- `model_hash.sha256` — expected SHA-256, consumed by the component's FR-8
  integrity check and the recipe artifact digest

## Regenerate

```bash
python3 train_model.py --epochs 15 --out model_fp32.onnx
python3 quantize_model.py --in model_fp32.onnx --out sample_model.onnx
```
