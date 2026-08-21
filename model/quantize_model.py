#!/usr/bin/env python3
"""Quantize the fp32 ONNX model to int8 and emit its SHA-256 hash file.

Dynamic post-training quantization via onnxruntime; writes
sample_model.onnx and model_hash.sha256 (consumed by the component's
FR-8 integrity check and the recipe's artifact digest).

Usage:
    python3 quantize_model.py --in model_fp32.onnx --out sample_model.onnx
"""

import argparse
import hashlib
from pathlib import Path

from onnxruntime.quantization import QuantType, quantize_dynamic


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--in", dest="input", default="model_fp32.onnx")
    parser.add_argument("--out", dest="output", default="sample_model.onnx")
    args = parser.parse_args()

    quantize_dynamic(
        model_input=args.input,
        model_output=args.output,
        weight_type=QuantType.QInt8,
    )

    digest = hashlib.sha256(Path(args.output).read_bytes()).hexdigest()
    hash_path = Path(args.output).with_name("model_hash.sha256")
    hash_path.write_text(digest + "\n")

    size_kb = Path(args.output).stat().st_size / 1024
    print(f"wrote {args.output} ({size_kb:.0f} KB)")
    print(f"sha256: {digest} -> {hash_path}")


if __name__ == "__main__":
    main()
