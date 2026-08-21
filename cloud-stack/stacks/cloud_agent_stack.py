"""Cloud agent stack.

Everything is serverless / on-demand (v2 spec cost constraint):
- DynamoDB on-demand table for telemetry (TTL: expire_at)
- IoT Rule: telemetry topic -> DynamoDBv2 direct write (no Lambda)
- IoT Rule: escalation topic -> escalation_invoker Lambda
- Lambda: thin invoker calling bedrock-agentcore InvokeAgentRuntime
- AgentCore Runtime hosting the Strands agent (ARM64 container from ECR)

The agent container is built from ../agent by a CDK Docker image asset,
so `cdk deploy` handles the ECR push. The AgentCore Runtime itself is
provisioned via the L1 CfnResource for AWS::BedrockAgentCore::Runtime.
"""

from pathlib import Path

import aws_cdk as cdk
from aws_cdk import (
    Duration,
    RemovalPolicy,
    Stack,
    aws_dynamodb as dynamodb,
    aws_ecr_assets as ecr_assets,
    aws_iam as iam,
    aws_iot as iot,
    aws_kms as kms,
    aws_lambda as lambda_,
    aws_logs as logs,
)
from constructs import Construct

AGENT_DIR = str(Path(__file__).parent.parent.parent / "agent")
LAMBDA_DIR = str(Path(__file__).parent.parent / "lambda" / "escalation_invoker")


class CloudAgentStack(Stack):
    def __init__(self, scope: Construct, construct_id: str, **kwargs) -> None:
        super().__init__(scope, construct_id, **kwargs)

        # --- Telemetry table (FR-13 / FR-6) ---
        telemetry_key = kms.Key(
            self,
            "TelemetryTableKey",
            description="CMK for pump_station_telemetry DynamoDB table encryption",
            enable_key_rotation=True,
            removal_policy=RemovalPolicy.DESTROY,
        )

        table = dynamodb.Table(
            self,
            "TelemetryTable",
            table_name="pump_station_telemetry",
            partition_key=dynamodb.Attribute(
                name="thing_name", type=dynamodb.AttributeType.STRING
            ),
            sort_key=dynamodb.Attribute(name="ts", type=dynamodb.AttributeType.STRING),
            billing_mode=dynamodb.BillingMode.PAY_PER_REQUEST,
            time_to_live_attribute="expire_at",
            point_in_time_recovery=True,
            encryption=dynamodb.TableEncryption.CUSTOMER_MANAGED,
            encryption_key=telemetry_key,
            # Sample repo: destroy cleanly on `cdk destroy` / cleanup.sh.
            removal_policy=RemovalPolicy.DESTROY,
        )

        # --- Telemetry ingestion rule: MQTT -> DynamoDB, no Lambda ---
        telemetry_rule_role = iam.Role(
            self,
            "TelemetryRuleRole",
            assumed_by=iam.ServicePrincipal("iot.amazonaws.com"),
        )
        table.grant_write_data(telemetry_rule_role)

        iot.CfnTopicRule(
            self,
            "TelemetryIngestRule",
            rule_name="pump_station_telemetry_ingest",
            topic_rule_payload=iot.CfnTopicRule.TopicRulePayloadProperty(
                sql="SELECT *, topic(2) as thing_name FROM 'pump-stations/+/telemetry'",
                aws_iot_sql_version="2016-03-23",
                actions=[
                    iot.CfnTopicRule.ActionProperty(
                        dynamo_d_bv2=iot.CfnTopicRule.DynamoDBv2ActionProperty(
                            put_item=iot.CfnTopicRule.PutItemInputProperty(
                                table_name=table.table_name
                            ),
                            role_arn=telemetry_rule_role.role_arn,
                        )
                    )
                ],
            ),
        )

        # --- Strands agent on AgentCore Runtime ---
        agent_image = ecr_assets.DockerImageAsset(
            self,
            "AgentImage",
            directory=AGENT_DIR,
            platform=ecr_assets.Platform.LINUX_ARM64,
        )

        agent_role = iam.Role(
            self,
            "AgentRuntimeRole",
            assumed_by=iam.ServicePrincipal("bedrock-agentcore.amazonaws.com"),
            description="Execution role for the pump-station RCA Strands agent",
        )
        table.grant_read_data(agent_role)
        agent_role.add_to_policy(
            iam.PolicyStatement(
                sid="PublishRecommendations",
                actions=["iot:Publish"],
                resources=[
                    self.format_arn(
                        service="iot",
                        resource="topic",
                        resource_name="pump-stations/*/recommendations",
                    )
                ],
            )
        )
        agent_role.add_to_policy(
            iam.PolicyStatement(
                sid="InvokeFoundationModel",
                actions=["bedrock:InvokeModel", "bedrock:InvokeModelWithResponseStream"],
                # Cross-region inference profile: model resources in all
                # us regions plus the profile itself.
                resources=[
                    f"arn:{self.partition}:bedrock:*::foundation-model/"
                    "anthropic.claude-haiku-4-5*",
                    f"arn:{self.partition}:bedrock:{self.region}:{self.account}:"
                    "inference-profile/us.anthropic.claude-haiku-4-5*",
                ],
            )
        )
        agent_role.add_to_policy(
            iam.PolicyStatement(
                sid="AgentCoreLogging",
                actions=[
                    "logs:CreateLogGroup",
                    "logs:CreateLogStream",
                    "logs:PutLogEvents",
                ],
                resources=[
                    self.format_arn(
                        service="logs",
                        resource="log-group",
                        resource_name="/aws/bedrock-agentcore/pump_station_rca_agent*",
                        arn_format=cdk.ArnFormat.COLON_RESOURCE_NAME,
                    )
                ],
            )
        )
        agent_role.add_to_policy(
            iam.PolicyStatement(
                sid="PullAgentImage",
                actions=[
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:BatchGetImage",
                ],
                resources=[agent_image.repository.repository_arn],
            )
        )
        agent_role.add_to_policy(
            iam.PolicyStatement(
                sid="EcrAuth",
                actions=["ecr:GetAuthorizationToken"],
                resources=["*"],
            )
        )

        # AWS::BedrockAgentCore::Runtime via L1 CfnResource (no L2 yet).
        agent_runtime = cdk.CfnResource(
            self,
            "AgentRuntime",
            type="AWS::BedrockAgentCore::Runtime",
            properties={
                "AgentRuntimeName": "pump_station_rca_agent",
                "AgentRuntimeArtifact": {
                    "ContainerConfiguration": {
                        "ContainerUri": agent_image.image_uri,
                    }
                },
                "NetworkConfiguration": {"NetworkMode": "PUBLIC"},
                "RoleArn": agent_role.role_arn,
                "EnvironmentVariables": {
                    "TELEMETRY_TABLE": table.table_name,
                },
            },
        )
        # AgentCore validates the role's ECR permissions at create time;
        # without this the Runtime races the role's inline policy.
        agent_runtime.node.add_dependency(agent_role)
        for policy in agent_role.node.children:
            if isinstance(policy, iam.Policy):
                agent_runtime.node.add_dependency(policy)

        # --- Escalation invoker Lambda (thin) ---
        invoker = lambda_.Function(
            self,
            "EscalationInvoker",
            runtime=lambda_.Runtime.PYTHON_3_12,
            architecture=lambda_.Architecture.ARM_64,
            handler="handler.lambda_handler",
            code=lambda_.Code.from_asset(LAMBDA_DIR),
            timeout=Duration.minutes(5),
            memory_size=256,
            log_group=logs.LogGroup(
                self,
                "EscalationInvokerLogs",
                retention=logs.RetentionDays.ONE_WEEK,
                removal_policy=RemovalPolicy.DESTROY,
            ),
            environment={
                "AGENT_RUNTIME_ARN": agent_runtime.get_att("AgentRuntimeArn").to_string(),
            },
        )
        invoker.add_to_role_policy(
            iam.PolicyStatement(
                sid="InvokeAgentRuntime",
                actions=["bedrock-agentcore:InvokeAgentRuntime"],
                resources=[
                    agent_runtime.get_att("AgentRuntimeArn").to_string(),
                    agent_runtime.get_att("AgentRuntimeArn").to_string() + "/*",
                ],
            )
        )

        # --- Escalation rule: MQTT -> Lambda ---
        escalation_rule = iot.CfnTopicRule(
            self,
            "EscalationRule",
            rule_name="pump_station_escalations",
            topic_rule_payload=iot.CfnTopicRule.TopicRulePayloadProperty(
                sql="SELECT *, topic(2) as thing_name FROM 'pump-stations/+/escalations'",
                aws_iot_sql_version="2016-03-23",
                actions=[
                    iot.CfnTopicRule.ActionProperty(
                        lambda_=iot.CfnTopicRule.LambdaActionProperty(
                            function_arn=invoker.function_arn
                        )
                    )
                ],
            ),
        )
        invoker.add_permission(
            "AllowIotInvoke",
            principal=iam.ServicePrincipal("iot.amazonaws.com"),
            source_arn=escalation_rule.attr_arn,
        )

        cdk.CfnOutput(self, "TelemetryTableName", value=table.table_name)
        cdk.CfnOutput(
            self,
            "AgentRuntimeArn",
            value=agent_runtime.get_att("AgentRuntimeArn").to_string(),
        )
        cdk.CfnOutput(self, "EscalationInvokerName", value=invoker.function_name)
