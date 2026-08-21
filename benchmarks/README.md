# Benchmarks

## Methodology

- **Latency** (NFR-2): Criterion benchmark `classify_window` in
  `edge-component/benches/bench_inference.rs`, run against the real
  quantized model via `scripts/run_benchmarks.sh`. Criterion reports the
  mean with 95% confidence bounds; p50/p95/p99 are in the HTML report
  under `edge-component/target/criterion/`.
- **Memory** (NFR-1): peak RSS of a process that loads the model and runs
  the inference test suite, measured with `/usr/bin/time -l` (macOS) or
  `/usr/bin/time -v` (Linux). Criterion measures time, not memory, so RSS
  deliberately lives in the script (v2 spec change #11).

Numbers below are **host measurements** (Apple M-series, macOS). The NFR
gates are defined for the ARM Cortex-A53 target — re-run
`scripts/run_benchmarks.sh` on the device and update this table before
quoting numbers in the blog.

## Results

| Metric | Target (spec) | Host measurement (M-series Mac) | Cortex-A53 (TODO) |
|---|---|---|---|
| Inference latency (mean) | < 50 ms | 63.4 µs | — |
| Peak RSS (model + inference) | < 30 MB | 22.1 MB | — |
| Stripped binary size | < 10 MB | **22 MB** (see note) | n/a |

Notes:
- The sample model is ~23 KB (deliberately tiny; see `model/README.md`).
  The blog's 12 MB / 10M-parameter scenario will have proportionally
  higher latency and RSS — the 50 ms / 30 MB budgets are for that size,
  and the Cortex-A53 column is the one that matters.
- Peak RSS includes the ONNX Runtime static initialization (~20 MB), which
  dominates at this model size.
- **Binary size (22 MB) exceeds the spec's 10 MB target** because the full
  ONNX Runtime is statically linked (the prebuilt library isn't built with
  a minimal-ops config). Options to shrink it: a custom ONNX Runtime
  minimal build with only the ops this model needs (Conv/MatMul/etc.), or
  `load-dynamic` with a shared `libonnxruntime.so` artifact. Memory
  footprint, not disk, is the binding constraint on the target (NFR-1),
  so the sample keeps the simpler static-link build.
