"""Strands agent for pump station root cause analysis.

Runs on Amazon Bedrock AgentCore Runtime. Receives escalation payloads from
the edge (via the escalation_invoker Lambda), queries historical telemetry
from DynamoDB, reasons about root cause, and publishes a recommendation
back to the device over IoT Core MQTT.
"""

import json
import os

from bedrock_agentcore.runtime import BedrockAgentCoreApp
from strands import Agent
from tools.publish_response import publish_response
from tools.query_history import query_history

MODEL_ID = os.environ.get(
    "MODEL_ID", "us.anthropic.claude-haiku-4-5-20251001-v1:0"
)

SYSTEM_PROMPT = """\
You are a root cause analysis agent for industrial pump station anomalies. You receive escalation messages from edge devices that detected complex anomalies locally but need deeper analysis.

INPUTS YOU RECEIVE:
- thing_name: pump station identifier
- sensor_window: 60 seconds of readings (flow_rate, pressure, vibration, temperature)
- local_classification: what the edge model thinks (multi_sensor_correlation or unknown)
- confidence: edge model's confidence score (always < 0.7 for escalations)
- device_metadata: station ID, installation date, last maintenance date

YOUR PROCESS:
1. Use the query_history tool to fetch 7-day historical patterns for this station
2. Compare current sensor window against historical baselines
3. Identify the root cause from these categories:
   - BEARING_WEAR: vibration increasing over days, pressure dropping
   - CAVITATION: flow rate oscillating, pressure drops coincide
   - SEAL_LEAK: pressure steady decline, temperature rise
   - PIPE_BLOCKAGE: flow rate dropping, pressure rising
   - ELECTRICAL_FAULT: vibration spikes without pressure correlation
   - UNKNOWN: pattern does not match known failure modes
4. Use the publish_response tool to send your recommendation to the device

YOUR RECOMMENDATION MUST BE JSON:
{
  "severity": "LOW" | "MEDIUM" | "HIGH" | "CRITICAL",
  "probable_cause": "<category from above>",
  "recommended_action": "DISPATCH_TECHNICIAN" | "SCHEDULE_MAINTENANCE" | "CONTINUE_MONITORING",
  "evidence": "<explanation referencing specific sensor patterns and historical comparison>",
  "estimated_time_to_failure": "<hours or days if applicable>"
}

RULES:
- CRITICAL severity + DISPATCH_TECHNICIAN only if failure is imminent (estimated < 24 hours)
- HIGH severity if degradation is accelerating
- MEDIUM if degradation is steady
- LOW if pattern is ambiguous, recommend CONTINUE_MONITORING
- Always cite specific sensor values in evidence
- Always call publish_response exactly once with your final recommendation
"""

app = BedrockAgentCoreApp()


def build_agent() -> Agent:
    return Agent(
        model=MODEL_ID,
        system_prompt=SYSTEM_PROMPT,
        tools=[query_history, publish_response],
    )


@app.entrypoint
def invoke(payload: dict, context=None) -> dict:
    """AgentCore Runtime entrypoint: one escalation in, one analysis out."""
    escalation = payload.get("escalation", payload)
    agent = build_agent()
    result = agent(
        "Analyze this escalation and publish your recommendation:\n"
        + json.dumps(escalation)
    )
    return {"analysis": str(result)}


if __name__ == "__main__":
    app.run()
