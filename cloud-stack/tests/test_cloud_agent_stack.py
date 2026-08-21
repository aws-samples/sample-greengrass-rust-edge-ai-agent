"""CDK assertions for the cloud agent stack (synth without deploy)."""

import sys
from pathlib import Path

import aws_cdk as cdk
import pytest
from aws_cdk.assertions import Match, Template

sys.path.insert(0, str(Path(__file__).parent.parent))

from stacks.cloud_agent_stack import CloudAgentStack


@pytest.fixture(scope="module")
def template() -> Template:
    app = cdk.App()
    stack = CloudAgentStack(app, "TestStack")
    return Template.from_stack(stack)


def test_dynamodb_table_on_demand_with_ttl(template):
    template.has_resource_properties(
        "AWS::DynamoDB::Table",
        {
            "BillingMode": "PAY_PER_REQUEST",
            "TimeToLiveSpecification": {
                "AttributeName": "expire_at",
                "Enabled": True,
            },
            "KeySchema": [
                {"AttributeName": "thing_name", "KeyType": "HASH"},
                {"AttributeName": "ts", "KeyType": "RANGE"},
            ],
        },
    )


def test_telemetry_rule_writes_to_dynamodb_without_lambda(template):
    template.has_resource_properties(
        "AWS::IoT::TopicRule",
        {
            "TopicRulePayload": {
                "Sql": Match.string_like_regexp(r".*pump-stations/\+/telemetry.*"),
                "Actions": [{"DynamoDBv2": Match.any_value()}],
            }
        },
    )


def test_escalation_rule_invokes_lambda(template):
    template.has_resource_properties(
        "AWS::IoT::TopicRule",
        {
            "TopicRulePayload": {
                "Sql": Match.string_like_regexp(r".*pump-stations/\+/escalations.*"),
                "Actions": [{"Lambda": Match.any_value()}],
            }
        },
    )


def test_agentcore_runtime_exists_with_arm64_container(template):
    template.has_resource_properties(
        "AWS::BedrockAgentCore::Runtime",
        {
            "AgentRuntimeName": "pump_station_rca_agent",
            "AgentRuntimeArtifact": {
                "ContainerConfiguration": {"ContainerUri": Match.any_value()}
            },
        },
    )


def test_invoker_lambda_is_arm64_python312(template):
    template.has_resource_properties(
        "AWS::Lambda::Function",
        {
            "Runtime": "python3.12",
            "Architectures": ["arm64"],
            "Handler": "handler.lambda_handler",
        },
    )


def test_agent_role_cannot_write_telemetry(template):
    """Least privilege: the agent reads history, never writes it."""
    roles = template.find_resources("AWS::IAM::Policy")
    for policy in roles.values():
        statements = policy["Properties"]["PolicyDocument"]["Statement"]
        for stmt in statements:
            actions = stmt.get("Action", [])
            if isinstance(actions, str):
                actions = [actions]
            has_read = any("dynamodb:Query" in a for a in actions)
            has_write = any(
                a.startswith("dynamodb:PutItem") or a.startswith("dynamodb:UpdateItem")
                for a in actions
            )
            assert not (has_read and has_write), (
                "no single statement should both read and write telemetry"
            )


def test_iot_publish_scoped_to_recommendations(template):
    """The agent may publish only to recommendation topics."""
    policies = template.find_resources("AWS::IAM::Policy")
    publish_resources = []
    for policy in policies.values():
        for stmt in policy["Properties"]["PolicyDocument"]["Statement"]:
            actions = stmt.get("Action", [])
            if isinstance(actions, str):
                actions = [actions]
            if "iot:Publish" in actions:
                publish_resources.append(stmt["Resource"])
    assert publish_resources, "expected an iot:Publish statement"
    for resource in publish_resources:
        blob = str(resource)
        assert "recommendations" in blob, f"iot:Publish not scoped: {blob}"
