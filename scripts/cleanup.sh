#!/usr/bin/env bash
# Tear down everything the sample created (v2 spec cost constraint:
# nothing may linger after a test deploy).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGION="${1:-us-east-1}"
COMPONENT_NAME="com.example.EdgeAIClassifier"

echo "==> Destroying cloud stack (DynamoDB, IoT Rules, Lambda, AgentCore)"
cd "${REPO_ROOT}/cloud-stack"
CDK_DEFAULT_REGION="${REGION}" cdk destroy --force

echo "==> Deleting Greengrass component versions"
ARNS=$(aws greengrassv2 list-component-versions \
    --arn "arn:aws:greengrass:${REGION}:$(aws sts get-caller-identity --query Account --output text):components:${COMPONENT_NAME}" \
    --query 'componentVersions[].arn' --output text 2>/dev/null || true)
for arn in ${ARNS}; do
    echo "    deleting ${arn}"
    aws greengrassv2 delete-component --arn "${arn}" --region "${REGION}"
done

cat <<'EOF'
==> Manual checks (not automated because they may be shared resources):
    - S3: remove uploaded artifacts (aws s3 rm s3://<bucket>/components/ --recursive)
    - Greengrass deployment: cancel/delete if the thing group still targets it
    - ECR: CDK asset repository keeps the agent image (cdk destroy leaves assets)
EOF
echo "==> Done"
