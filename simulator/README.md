# Simulators

Test utilities — no real pump hardware required.

## mqtt_watch.py (terminal MQTT monitor)

Subscribe to pump-station topics from the terminal and pretty-print every
message — a reliable stand-in for the AWS IoT console's MQTT test client,
which often fails to connect on corporate networks (the browser WebSocket
gets blocked by VPN/proxy stacks even when permissions and the endpoint
are fine). Uses the same credentials as the AWS CLI.

```bash
# watch everything on pump-stations/#
python3 mqtt_watch.py

# watch one topic; exit after the first message (scriptable)
python3 mqtt_watch.py --once --timeout 120 \
  --topic 'pump-stations/pump-station-001/recommendations'
```

## sensor_simulator.py (device-side)

Emits synthetic 1 Hz sensor readings as JSON lines, with optional anomaly
injection. Pipe into anything that publishes to the component's local IPC
topic, or use it to eyeball data shapes:

```bash
python3 sensor_simulator.py --seconds 180 --anomaly cavitation --at 60
```

Anomaly patterns (`anomaly_injector.py`) mirror the training data:
`bearing_wear`, `cavitation`, `seal_leak` (multi-sensor → escalate),
`single_sensor_spike` (→ local alert), `erratic` (unknown → escalate).

## fleet_simulator.py (cloud-side)

Requires AWS credentials; targets the deployed cloud stack.

```bash
# Seed 7 days of history so the agent has baselines on day one
python3 fleet_simulator.py seed --stations 3 --days 7

# Publish live telemetry through the IoT Rule -> DynamoDB path
python3 fleet_simulator.py publish --stations 2 --minutes 5

# Fire one synthetic escalation (IoT Rule -> Lambda -> AgentCore agent)
python3 fleet_simulator.py escalate --station pump-station-001
```

Seeding writes ~10K records/station/day directly to DynamoDB (on-demand
billing; a 3-station, 7-day seed is ~30K writes ≈ $0.04).

> ⚠️ **Escalations cost real money** (Lambda + Bedrock agent per message).
> Don't loop `escalate`, and never leave a device/simulator running with
> anomaly injection after a test session — see the cost warning at the top
> of `docs/runbook.md`.
