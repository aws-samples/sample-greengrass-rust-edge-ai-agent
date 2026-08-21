#!/usr/bin/env bash
# Upload artifacts to S3, create the component version, and deploy to a
# thing group. Requires: aws CLI credentials, scripts/build.sh already run.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BUCKET="${1:?usage: deploy_component.sh <s3-bucket> <thing-group> [region]}"
THING_GROUP="${2:?usage: deploy_component.sh <s3-bucket> <thing-group> [region]}"
REGION="${3:-us-east-1}"

COMPONENT_NAME="com.example.EdgeAIClassifier"
VERSION="1.0.1"
BINARY="${REPO_ROOT}/edge-component/dist/edge-ai-classifier"
MODEL="${REPO_ROOT}/model/sample_model.onnx"
MODEL_HASH="$(tr -d '[:space:]' < "${REPO_ROOT}/model/model_hash.sha256")"
S3_PREFIX="components/${COMPONENT_NAME}/${VERSION}"

[[ -f "${BINARY}" ]] || { echo "ERROR: run scripts/build.sh first"; exit 1; }

echo "==> Uploading artifacts to s3://${BUCKET}/${S3_PREFIX}/"
aws s3 cp "${BINARY}" "s3://${BUCKET}/${S3_PREFIX}/edge-ai-classifier" --region "${REGION}"
aws s3 cp "${MODEL}" "s3://${BUCKET}/${S3_PREFIX}/sample_model.onnx" --region "${REGION}"

echo "==> Rendering recipe"
RECIPE="$(mktemp)"
sed -e "s/BUCKET/${BUCKET}/g" -e "s/MODEL_HASH/${MODEL_HASH}/g" \
    "${REPO_ROOT}/component-recipe/recipe.yaml" > "${RECIPE}"

echo "==> Creating component version ${COMPONENT_NAME}@${VERSION}"
aws greengrassv2 create-component-version \
    --inline-recipe "fileb://${RECIPE}" \
    --region "${REGION}"

echo "==> Waiting for component to be deployable"
aws greengrassv2 wait 2>/dev/null || sleep 10

ACCOUNT_ID="$(aws sts get-caller-identity --query Account --output text)"
TARGET_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thinggroup/${THING_GROUP}"

echo "==> Creating deployment to ${THING_GROUP}"
aws greengrassv2 create-deployment \
    --target-arn "${TARGET_ARN}" \
    --deployment-name "edge-ai-classifier-${VERSION}" \
    --components "{\"${COMPONENT_NAME}\":{\"componentVersion\":\"${VERSION}\"}}" \
    --region "${REGION}"

echo "==> Done. Check status: aws greengrassv2 list-effective-deployments --core-device-thing-name <thing>"
