"""Thin escalation invoker: IoT Rule event -> AgentCore InvokeAgentRuntime.

All agent logic lives in the AgentCore-hosted Strands agent (see /agent).
This function only forwards the escalation payload and logs the result.
"""

import json
import logging
import os
import uuid

import boto3

logger = logging.getLogger()
logger.setLevel(logging.INFO)

AGENT_RUNTIME_ARN = os.environ["AGENT_RUNTIME_ARN"]

client = boto3.client("bedrock-agentcore")


def lambda_handler(event: dict, context) -> dict:
    """`event` is the escalation JSON (IoT Rule SELECT * plus thing_name)."""
    thing_name = event.get("thing_name", "unknown")
    logger.info(
        "escalation from %s: classification=%s confidence=%s",
        thing_name,
        event.get("local_classification", {}).get("anomaly_type"),
        event.get("confidence"),
    )

    response = client.invoke_agent_runtime(
        agentRuntimeArn=AGENT_RUNTIME_ARN,
        # One session per escalation: analyses are independent.
        runtimeSessionId=f"escalation-{thing_name}-{uuid.uuid4()}",
        payload=json.dumps({"escalation": event}).encode(),
    )

    body = response["response"].read().decode()
    # Log metadata only; full analysis may contain sensitive operational details.
    import json as _json
    try:
        analysis = _json.loads(body)
        logger.info(
            "agent analysis for %s: severity=%s cause=%s",
            thing_name,
            analysis.get("severity", "unknown"),
            analysis.get("probable_cause", "unknown"),
        )
    except (ValueError, KeyError):
        logger.info("agent analysis for %s: completed (non-JSON response)", thing_name)
    return {"statusCode": 200, "analysis": body}
