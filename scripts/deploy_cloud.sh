#!/usr/bin/env bash
# Deploy the cloud stack (DynamoDB, IoT Rules, Lambda, AgentCore agent).
# The CDK Docker image asset builds and pushes the agent container.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGION="${1:-us-east-1}"

cd "${REPO_ROOT}/cloud-stack"

echo "==> Installing CDK app dependencies"
python3 -m pip install -q -r requirements.txt

echo "==> Deploying (region: ${REGION})"
CDK_DEFAULT_REGION="${REGION}" cdk deploy --require-approval never

echo "==> Done. Seed history next: python3 simulator/fleet_simulator.py seed --stations 1 --days 7"
