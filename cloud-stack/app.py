#!/usr/bin/env python3
"""CDK app: cloud side of the Greengrass Rust edge AI sample."""

import aws_cdk as cdk
from stacks.cloud_agent_stack import CloudAgentStack

app = cdk.App()
CloudAgentStack(
    app,
    "GreengrassRustEdgeAiCloudStack",
    description="Cloud agent stack for sample-greengrass-rust-edge-ai-agent "
    "(IoT Rules, escalation Lambda, DynamoDB telemetry, AgentCore agent)",
)
app.synth()
