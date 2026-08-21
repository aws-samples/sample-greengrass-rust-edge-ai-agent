# Component recipe

`recipe.yaml` defines the Greengrass component. Field notes:

| Field | Why it looks the way it does |
|---|---|
| `accessControl` under `DefaultConfiguration` | This is where Greengrass reads IPC authorization policies from; a top-level `AccessControl` key fails validation. |
| `{iot:thingName}` | Greengrass recipe variable, interpolated per device at deployment (requires Nucleus ≥ 2.6 for use inside configuration). Scopes MQTT topics and IPC permissions to the individual device — least privilege. |
| Two `accessControl` blocks | `mqttproxy` for IoT Core publish/subscribe, `pubsub` for the local sensor topic. |
| `--config-json '{configuration:/}'` | Interpolates the merged component configuration as JSON into the run command; the binary parses it at startup (no config file on disk). |
| Model artifact `Digest` | Greengrass verifies the S3 artifact hash at deployment; the binary re-verifies at startup against `model_sha256` in configuration (defense in depth, FR-8). |
| No `install` step | `Permission: Execute: OWNER` already makes the binary executable; the artifact directory is read-only to the component. |

`BUCKET` and `MODEL_HASH` are placeholders substituted by
`scripts/deploy_component.sh`.
