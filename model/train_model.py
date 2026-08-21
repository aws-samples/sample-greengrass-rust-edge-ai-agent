#!/usr/bin/env python3
"""Train the pump-station anomaly classifier and export to ONNX.

Architecture: 1D-CNN + channel attention over a [batch, 4, 60] input
(channel-major sensor window per the v2 spec) with a 4-class output:
[normal, single_sensor_fault, multi_sensor_correlation, unknown].

Training data is synthetic, generated with the same pattern logic as
simulator/anomaly_injector.py so the edge simulator can reproduce every
class the model knows.

Usage:
    python3 train_model.py --epochs 20 --out model_fp32.onnx
"""

import argparse

import numpy as np
import torch
import torch.nn as nn
from torch.utils.data import DataLoader, TensorDataset

NUM_CHANNELS = 4
WINDOW = 60
NUM_CLASSES = 4

# Baseline operating point for a healthy pump station.
BASELINES = np.array([100.0, 50.0, 1.0, 20.0], dtype=np.float32)  # flow, press, vib, temp
NOISE = np.array([2.0, 1.0, 0.05, 0.3], dtype=np.float32)


def make_normal(rng: np.random.Generator) -> np.ndarray:
    window = BASELINES[:, None] + rng.normal(0, 1, (NUM_CHANNELS, WINDOW)).astype(
        np.float32
    ) * NOISE[:, None]
    return window


def make_single_sensor_fault(rng: np.random.Generator) -> np.ndarray:
    """One channel breaches threshold; others stay at baseline."""
    window = make_normal(rng)
    channel = rng.integers(0, NUM_CHANNELS)
    onset = rng.integers(10, 40)
    magnitude = rng.uniform(4, 8) * NOISE[channel] * rng.choice([-1, 1])
    window[channel, onset:] += magnitude
    return window


def make_multi_sensor_correlation(rng: np.random.Generator) -> np.ndarray:
    """Correlated drift across 2+ channels (bearing wear / cavitation / seal leak)."""
    window = make_normal(rng)
    pattern = rng.integers(0, 3)
    ramp = np.linspace(0, 1, WINDOW, dtype=np.float32)
    if pattern == 0:  # bearing wear: vibration up, pressure down
        window[2] += ramp * rng.uniform(0.3, 0.8)
        window[1] -= ramp * rng.uniform(2, 5)
    elif pattern == 1:  # cavitation: flow oscillates, pressure dips coincide
        osc = np.sin(np.linspace(0, rng.uniform(6, 12) * np.pi, WINDOW)).astype(
            np.float32
        )
        window[0] += osc * rng.uniform(5, 10)
        window[1] += osc * rng.uniform(2, 4) - 1.0
    else:  # seal leak: pressure declines, temperature rises
        window[1] -= ramp * rng.uniform(3, 6)
        window[3] += ramp * rng.uniform(1.5, 3)
    return window


def make_unknown(rng: np.random.Generator) -> np.ndarray:
    """Erratic multi-channel noise matching no trained failure mode."""
    window = make_normal(rng)
    for channel in range(NUM_CHANNELS):
        if rng.random() < 0.6:
            spikes = rng.random(WINDOW) < 0.15
            window[channel] += spikes * rng.normal(0, 8 * NOISE[channel], WINDOW).astype(
                np.float32
            )
    return window


GENERATORS = [make_normal, make_single_sensor_fault, make_multi_sensor_correlation, make_unknown]


def make_dataset(rng: np.random.Generator, per_class: int) -> TensorDataset:
    windows, labels = [], []
    for label, gen in enumerate(GENERATORS):
        for _ in range(per_class):
            windows.append(gen(rng))
            labels.append(label)
    x = torch.from_numpy(np.stack(windows))
    y = torch.tensor(labels, dtype=torch.long)
    return TensorDataset(x, y)


class ChannelAttention(nn.Module):
    """Squeeze-and-excitation style attention over sensor channels."""

    def __init__(self, channels: int, reduction: int = 2):
        super().__init__()
        self.fc = nn.Sequential(
            nn.Linear(channels, channels // reduction),
            nn.ReLU(),
            nn.Linear(channels // reduction, channels),
            nn.Sigmoid(),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        weights = self.fc(x.mean(dim=2))
        return x * weights.unsqueeze(2)


class AnomalyClassifier(nn.Module):
    def __init__(self):
        super().__init__()
        # Input normalization baked into the graph: (x - baseline) / noise
        # per channel, so the component feeds raw sensor units and the
        # model is self-contained (buffers export as ONNX constants).
        self.register_buffer("baseline", torch.from_numpy(BASELINES).view(1, NUM_CHANNELS, 1))
        self.register_buffer("scale", torch.from_numpy(NOISE).view(1, NUM_CHANNELS, 1))
        self.features = nn.Sequential(
            nn.Conv1d(NUM_CHANNELS, 32, kernel_size=5, padding=2),
            nn.ReLU(),
            nn.MaxPool1d(2),
            nn.Conv1d(32, 64, kernel_size=5, padding=2),
            nn.ReLU(),
            nn.MaxPool1d(2),
        )
        self.attention = ChannelAttention(64, reduction=8)
        self.head = nn.Sequential(
            nn.AdaptiveAvgPool1d(1),
            nn.Flatten(),
            nn.Linear(64, NUM_CLASSES),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = (x - self.baseline) / self.scale
        x = self.features(x)
        x = self.attention(x)
        logits = self.head(x)
        return torch.softmax(logits, dim=1)


def train(model: nn.Module, dataset: TensorDataset, epochs: int) -> None:
    loader = DataLoader(dataset, batch_size=64, shuffle=True)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
    # CrossEntropyLoss on log of softmax output == NLL; model emits
    # probabilities directly so the ONNX graph needs no post-processing.
    criterion = nn.NLLLoss()
    model.train()
    for epoch in range(epochs):
        total, correct, loss_sum = 0, 0, 0.0
        for x, y in loader:
            optimizer.zero_grad()
            probs = model(x)
            loss = criterion(torch.log(probs + 1e-9), y)
            loss.backward()
            optimizer.step()
            loss_sum += loss.item() * len(y)
            correct += (probs.argmax(dim=1) == y).sum().item()
            total += len(y)
        print(f"epoch {epoch + 1}/{epochs}  loss={loss_sum / total:.4f}  acc={correct / total:.3f}")


def export_onnx(model: nn.Module, path: str) -> None:
    model.eval()
    dummy = torch.zeros(1, NUM_CHANNELS, WINDOW)
    torch.onnx.export(
        model,
        dummy,
        path,
        input_names=["sensor_window"],
        output_names=["class_probabilities"],
        opset_version=18,
        dynamo=False,
    )
    print(f"exported {path}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--epochs", type=int, default=20)
    parser.add_argument("--per-class", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--out", default="model_fp32.onnx")
    args = parser.parse_args()

    torch.manual_seed(args.seed)
    rng = np.random.default_rng(args.seed)

    model = AnomalyClassifier()
    dataset = make_dataset(rng, args.per_class)
    train(model, dataset, args.epochs)

    # Hold-out check on freshly generated data.
    model.eval()
    holdout = make_dataset(np.random.default_rng(args.seed + 1), 200)
    x, y = holdout.tensors
    with torch.no_grad():
        acc = (model(x).argmax(dim=1) == y).float().mean().item()
    print(f"holdout accuracy: {acc:.3f}")

    export_onnx(model, args.out)


if __name__ == "__main__":
    main()
