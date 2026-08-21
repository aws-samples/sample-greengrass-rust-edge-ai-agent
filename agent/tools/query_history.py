"""Strands tool: query 7-day historical telemetry from DynamoDB."""

import os
from datetime import datetime, timedelta, timezone

import boto3
from boto3.dynamodb.conditions import Key
from strands import tool

TABLE_NAME = os.environ.get("TELEMETRY_TABLE", "pump_station_telemetry")

CHANNELS = ("flow_rate", "pressure", "vibration", "temperature")


def _stats(values: list[float], timestamps: list[float]) -> dict:
    """Mean, stddev, and least-squares trend (slope per hour) for one channel."""
    n = len(values)
    if n == 0:
        return {"mean": None, "stddev": None, "trend_per_hour": None}
    mean = sum(values) / n
    if n == 1:
        return {"mean": round(mean, 4), "stddev": 0.0, "trend_per_hour": 0.0}
    variance = sum((v - mean) ** 2 for v in values) / (n - 1)
    # Least-squares slope of value vs. time (hours since first record).
    t0 = timestamps[0]
    hours = [(t - t0) / 3600.0 for t in timestamps]
    mean_h = sum(hours) / n
    denom = sum((h - mean_h) ** 2 for h in hours)
    slope = (
        sum((h - mean_h) * (v - mean) for h, v in zip(hours, values)) / denom
        if denom > 0
        else 0.0
    )
    return {
        "mean": round(mean, 4),
        "stddev": round(variance**0.5, 4),
        "trend_per_hour": round(slope, 6),
    }


@tool
def query_history(thing_name: str, hours: int = 168) -> dict:
    """Query historical sensor telemetry for a pump station.

    Args:
        thing_name: The pump station identifier (IoT thing name).
        hours: Lookback window in hours (default 168 = 7 days).

    Returns:
        Per-channel statistics over the window: mean, stddev, and
        trend_per_hour (least-squares slope), plus record_count.
    """
    table = boto3.resource("dynamodb").Table(TABLE_NAME)
    now = datetime.now(timezone.utc)
    start = (now - timedelta(hours=hours)).strftime("%Y-%m-%dT%H:%M:%SZ")
    end = now.strftime("%Y-%m-%dT%H:%M:%SZ")

    items = []
    kwargs = {
        "KeyConditionExpression": Key("thing_name").eq(thing_name)
        & Key("ts").between(start, end)
    }
    while True:
        page = table.query(**kwargs)
        items.extend(page.get("Items", []))
        last_key = page.get("LastEvaluatedKey")
        if not last_key:
            break
        kwargs["ExclusiveStartKey"] = last_key

    timestamps = [
        datetime.strptime(item["ts"], "%Y-%m-%dT%H:%M:%SZ")
        .replace(tzinfo=timezone.utc)
        .timestamp()
        for item in items
    ]
    result = {"thing_name": thing_name, "hours": hours, "record_count": len(items)}
    for channel in CHANNELS:
        values = [float(item[channel]) for item in items if channel in item]
        result[channel] = _stats(values, timestamps[: len(values)])
    return result
