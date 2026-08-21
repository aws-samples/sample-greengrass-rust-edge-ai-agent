"""Anomaly pattern injection for the sensor simulator.

Mirrors the failure-mode patterns in model/train_model.py so injected
anomalies reproduce classes the model was trained on. Patterns operate on
a per-second callback basis: given elapsed seconds since injection start,
return additive offsets per channel [flow_rate, pressure, vibration,
temperature].
"""

import math
import random

CHANNELS = ("flow_rate", "pressure", "vibration", "temperature")


def bearing_wear(t: float, duration: float = 300.0) -> dict:
    """Gradual vibration increase with slow pressure drop (multi-sensor)."""
    ramp = min(t / duration, 1.0)
    return {
        "flow_rate": 0.0,
        "pressure": -ramp * 4.0,
        "vibration": ramp * 0.6,
        "temperature": ramp * 0.5,
    }


def cavitation(t: float, duration: float = 120.0) -> dict:
    """Flow oscillation with coincident pressure dips (multi-sensor)."""
    osc = math.sin(t * 0.8 * math.pi)
    return {
        "flow_rate": osc * 8.0,
        "pressure": osc * 3.0 - 1.0,
        "vibration": abs(osc) * 0.15,
        "temperature": 0.0,
    }


def seal_leak(t: float, duration: float = 600.0) -> dict:
    """Steady pressure decline with temperature rise (multi-sensor)."""
    ramp = min(t / duration, 1.0)
    return {
        "flow_rate": -ramp * 2.0,
        "pressure": -ramp * 5.0,
        "vibration": 0.0,
        "temperature": ramp * 2.5,
    }


def single_sensor_spike(t: float, duration: float = 60.0, channel: str = "pressure") -> dict:
    """Sustained single-channel threshold breach (single_sensor_fault)."""
    magnitudes = {"flow_rate": 14.0, "pressure": 7.0, "vibration": 0.35, "temperature": 2.1}
    offsets = {c: 0.0 for c in CHANNELS}
    offsets[channel] = magnitudes[channel]
    return offsets


def erratic(t: float, duration: float = 90.0) -> dict:
    """Random multi-channel spikes matching no trained pattern (unknown)."""
    offsets = {}
    for channel, scale in zip(CHANNELS, (10.0, 5.0, 0.4, 1.5)):
        offsets[channel] = random.gauss(0, scale) if random.random() < 0.2 else 0.0
    return offsets


PATTERNS = {
    "bearing_wear": bearing_wear,
    "cavitation": cavitation,
    "seal_leak": seal_leak,
    "single_sensor_spike": single_sensor_spike,
    "erratic": erratic,
}
