#!/usr/bin/env python3
"""Local sensor simulator: publishes synthetic readings for one station.

Publishes 1 Hz JSON readings to the component's local IPC topic via
Greengrass local pub/sub when run as a component helper, or to stdout /
a plain MQTT-style JSON stream for host-side testing of the data shapes.

Baseline values match model/train_model.py; anomalies come from
anomaly_injector.PATTERNS.

Usage:
    # print 120 seconds of normal readings as JSON lines
    python3 sensor_simulator.py --seconds 120

    # inject cavitation starting at t=60
    python3 sensor_simulator.py --seconds 180 --anomaly cavitation --at 60
"""

import argparse
import json
import random
import sys
import time

from anomaly_injector import CHANNELS, PATTERNS

BASELINES = {"flow_rate": 100.0, "pressure": 50.0, "vibration": 1.0, "temperature": 20.0}
NOISE = {"flow_rate": 2.0, "pressure": 1.0, "vibration": 0.05, "temperature": 0.3}


def generate_reading(t: float, anomaly: str | None, anomaly_start: float) -> dict:
    reading = {
        channel: BASELINES[channel] + random.gauss(0, NOISE[channel])
        for channel in CHANNELS
    }
    if anomaly and t >= anomaly_start:
        offsets = PATTERNS[anomaly](t - anomaly_start)
        for channel in CHANNELS:
            reading[channel] += offsets[channel]
    return {channel: round(value, 4) for channel, value in reading.items()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=120)
    parser.add_argument("--anomaly", choices=sorted(PATTERNS), default=None)
    parser.add_argument("--at", type=float, default=60.0, help="anomaly start time (s)")
    parser.add_argument("--seed", type=int, default=None)
    parser.add_argument(
        "--realtime",
        action="store_true",
        help="sleep 1s between readings (default: emit as fast as possible)",
    )
    args = parser.parse_args()

    if args.seed is not None:
        random.seed(args.seed)

    for t in range(args.seconds):
        reading = generate_reading(float(t), args.anomaly, args.at)
        print(json.dumps(reading), flush=True)
        if args.realtime:
            time.sleep(1)

    print(f"emitted {args.seconds} readings", file=sys.stderr)


if __name__ == "__main__":
    main()
