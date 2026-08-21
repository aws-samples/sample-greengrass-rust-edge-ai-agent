"""Strands tool: publish the agent's recommendation back to the device."""

import json

import boto3
from strands import tool


@tool
def publish_response(thing_name: str, response: dict) -> dict:
    """Publish a maintenance recommendation to the device via IoT Core MQTT.

    Args:
        thing_name: The pump station identifier (IoT thing name).
        response: The recommendation JSON with keys: severity,
            probable_cause, recommended_action, evidence, and optionally
            estimated_time_to_failure.

    Returns:
        Confirmation with the topic published to.
    """
    topic = f"pump-stations/{thing_name}/recommendations"
    client = boto3.client("iot-data")
    client.publish(topic=topic, qos=1, payload=json.dumps(response))
    return {"published": True, "topic": topic}
