#!/usr/bin/env python3
"""Fleet simulator: cloud-side testing without real devices.

Two modes:

seed  — writes N days of historical telemetry directly into DynamoDB so
        the agent has baselines on day one (v2 spec FR-13 / task 35).
publish — publishes live telemetry and/or escalation messages to IoT Core
        over MQTT (boto3 iot-data), exercising the IoT Rules end to end.

Usage:
    python3 fleet_simulator.py seed --stations 3 --days 7
    python3 fleet_simulator.py publish --stations 2 --minutes 5
    python3 fleet_simulator.py escalate --station pump-station-001
"""

import argparse
import json
import random
import time
from datetime import datetime, timedelta, timezone
from decimal import Decimal

import boto3

from anomaly_injector import CHANNELS
from sensor_simulator import BASELINES, NOISE, generate_reading

TABLE = "pump_station_telemetry"


def iso(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def station_name(index: int) -> str:
    return f"pump-station-{index:03d}"


def seed(stations: int, days: int) -> None:
    """Write per-minute aggregates for the trailing N days."""
    table = boto3.resource("dynamodb").Table(TABLE)
    now = datetime.now(timezone.utc).replace(second=0, microsecond=0)
    expire_at = int((now + timedelta(days=90)).timestamp())
    total = 0
    with table.batch_writer() as batch:
        for s in range(1, stations + 1):
            thing = station_name(s)
            # Slight per-station drift so trend statistics are non-trivial.
            drift = {c: random.uniform(-0.01, 0.01) for c in CHANNELS}
            minutes = days * 24 * 60
            for m in range(minutes):
                ts = now - timedelta(minutes=minutes - m)
                item = {"thing_name": thing, "ts": iso(ts), "expire_at": expire_at}
                for channel in CHANNELS:
                    value = (
                        BASELINES[channel]
                        + random.gauss(0, NOISE[channel] * 0.3)
                        + drift[channel] * m / 60.0
                    )
                    item[channel] = Decimal(str(round(value, 4)))
                batch.put_item(Item=item)
                total += 1
    print(f"seeded {total} telemetry records for {stations} station(s), {days} day(s)")


def publish(stations: int, minutes: int) -> None:
    """Publish live 1-minute telemetry aggregates to IoT Core."""
    client = boto3.client("iot-data")
    for minute in range(minutes):
        now = datetime.now(timezone.utc)
        expire_at = int((now + timedelta(days=90)).timestamp())
        for s in range(1, stations + 1):
            thing = station_name(s)
            record = {"thing_name": thing, "ts": iso(now), "expire_at": expire_at}
            for channel in CHANNELS:
                record[channel] = round(
                    BASELINES[channel] + random.gauss(0, NOISE[channel] * 0.3), 4
                )
            client.publish(
                topic=f"pump-stations/{thing}/telemetry",
                qos=1,
                payload=json.dumps(record),
            )
        print(f"minute {minute + 1}/{minutes}: published telemetry for {stations} station(s)")
        if minute < minutes - 1:
            time.sleep(60)


def escalate(station: str) -> None:
    """Publish one synthetic escalation message (triggers the cloud agent)."""
    client = boto3.client("iot-data")
    window = {c: [] for c in CHANNELS}
    for t in range(60):
        reading = generate_reading(float(t), "bearing_wear", 0.0)
        for channel in CHANNELS:
            window[channel].append(reading[channel])
    message = {
        "thing_name": station,
        "timestamp": iso(datetime.now(timezone.utc)),
        "sensor_window": [window[c] for c in CHANNELS],
        "local_classification": {
            "anomaly_type": "multi_sensor_correlation",
            "confidence": 0.55,
            "contributing_sensors": ["vibration", "pressure"],
        },
        "confidence": 0.55,
        "device_metadata": {
            "station_id": station.upper(),
            "installation_date": "2020-03-15",
            "last_maintenance_date": "2026-01-10",
        },
    }
    client.publish(
        topic=f"pump-stations/{station}/escalations",
        qos=1,
        payload=json.dumps(message),
    )
    print(f"published escalation for {station}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)

    p_seed = sub.add_parser("seed", help="seed historical telemetry into DynamoDB")
    p_seed.add_argument("--stations", type=int, default=1)
    p_seed.add_argument("--days", type=int, default=7)

    p_pub = sub.add_parser("publish", help="publish live telemetry to IoT Core")
    p_pub.add_argument("--stations", type=int, default=1)
    p_pub.add_argument("--minutes", type=int, default=5)

    p_esc = sub.add_parser("escalate", help="publish one synthetic escalation")
    p_esc.add_argument("--station", default="pump-station-001")

    args = parser.parse_args()
    if args.mode == "seed":
        seed(args.stations, args.days)
    elif args.mode == "publish":
        publish(args.stations, args.minutes)
    else:
        escalate(args.station)


if __name__ == "__main__":
    main()
