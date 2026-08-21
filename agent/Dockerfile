# Strands agent container for Amazon Bedrock AgentCore Runtime (ARM64).
FROM --platform=linux/arm64 public.ecr.aws/docker/library/python:3.12-slim

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY agent.py .
COPY tools/ tools/

# AgentCore Runtime expects the app to listen on 8080.
EXPOSE 8080
CMD ["python", "agent.py"]
