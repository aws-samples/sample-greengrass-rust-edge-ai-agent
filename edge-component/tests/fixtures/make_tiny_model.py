#!/usr/bin/env python3
"""Generate tiny_model.onnx: a minimal fixture for classifier unit tests.

Input shape [1, 4, 60] (batch, channels, time) and 4-class softmax output
[1, 4] — the same interface as the real model (see "Sensor window memory
layout" in the v2 spec) — but with fixed, hand-chosen weights so tests are
deterministic and the fixture stays under 2 KB.

The weights make the model a simple channel-mean detector:
  logit[c] = mean(input[c]) * 1.0
so a window where channel c has the largest mean classifies as class c.
Tests use this to force each output class deliberately.

Usage: python3 make_tiny_model.py [output_path]
"""

import sys

import numpy as np
import onnx
from onnx import TensorProto, helper


def build_model() -> onnx.ModelProto:
    # Graph: input [1,4,60] -> ReduceMean(axis=2) [1,4] -> Softmax -> output
    input_tensor = helper.make_tensor_value_info(
        "sensor_window", TensorProto.FLOAT, [1, 4, 60]
    )
    output_tensor = helper.make_tensor_value_info(
        "class_probabilities", TensorProto.FLOAT, [1, 4]
    )

    axes = helper.make_tensor("axes", TensorProto.INT64, [1], np.array([2], dtype=np.int64))

    reduce_mean = helper.make_node(
        "ReduceMean",
        inputs=["sensor_window", "axes"],
        outputs=["channel_means"],
        keepdims=0,
    )
    softmax = helper.make_node(
        "Softmax", inputs=["channel_means"], outputs=["class_probabilities"], axis=1
    )

    graph = helper.make_graph(
        [reduce_mean, softmax],
        "tiny_anomaly_classifier_fixture",
        [input_tensor],
        [output_tensor],
        initializer=[axes],
    )
    model = helper.make_model(
        graph,
        opset_imports=[helper.make_opsetid("", 18)],
        producer_name="make_tiny_model",
    )
    model.ir_version = 9
    onnx.checker.check_model(model)
    return model


def main() -> None:
    out = sys.argv[1] if len(sys.argv) > 1 else "tiny_model.onnx"
    model = build_model()
    onnx.save(model, out)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
