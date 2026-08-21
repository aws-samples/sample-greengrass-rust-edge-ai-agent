#!/usr/bin/env python3
"""Terminal MQTT watcher: subscribe to pump-station topics and pretty-print
every message. A reliable stand-in for the AWS IoT console's MQTT test
client, which can fail to connect on corporate networks (its browser
WebSocket is often blocked by VPN/proxy stacks even when the endpoint and
IAM permissions are fine).

Connects over WebSockets with SigV4 using your default AWS credentials —
the same auth chain as the AWS CLI, so if `aws sts get-caller-identity`
works, this works.

Usage:
    # watch everything (default: pump-stations/#)
    python3 mqtt_watch.py

    # watch one station's recommendations only
    python3 mqtt_watch.py --topic 'pump-stations/pump-station-001/recommendations'

    # exit after the first message (useful in scripts/CI)
    python3 mqtt_watch.py --once --timeout 120

Requires: pip install awsiotsdk
"""

import argparse
import json
import sys
import threading

import boto3
from awscrt import auth, mqtt5
from awsiot import mqtt5_client_builder


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--topic", default="pump-stations/#", help="topic filter")
    parser.add_argument("--region", default="us-east-1")
    parser.add_argument(
        "--endpoint",
        default=None,
        help="IoT data endpoint (default: discovered via describe-endpoint)",
    )
    parser.add_argument(
        "--once", action="store_true", help="exit after the first message"
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=0,
        help="with --once: give up after N seconds (0 = wait forever)",
    )
    args = parser.parse_args()

    endpoint = args.endpoint or boto3.client("iot", region_name=args.region)\
        .describe_endpoint(endpointType="iot:Data-ATS")["endpointAddress"]

    got_message = threading.Event()

    def on_message(data):
        packet = data.publish_packet
        print(f"\n{'=' * 70}\nTOPIC: {packet.topic}\n{'=' * 70}")
        try:
            print(json.dumps(json.loads(packet.payload), indent=2))
        except (ValueError, UnicodeDecodeError):
            print(packet.payload[:1000])
        got_message.set()

    client = mqtt5_client_builder.websockets_with_default_aws_signing(
        endpoint=endpoint,
        region=args.region,
        credentials_provider=auth.AwsCredentialsProvider.new_default_chain(),
        on_publish_received=on_message,
        client_id="mqtt-watch-terminal",
    )
    client.start()
    subscribe_future = client.subscribe(
        subscribe_packet=mqtt5.SubscribePacket(
            subscriptions=[
                mqtt5.Subscription(
                    topic_filter=args.topic, qos=mqtt5.QoS.AT_LEAST_ONCE
                )
            ]
        )
    )
    subscribe_future.result(timeout=30)
    print(f"✅ connected to {endpoint}")
    print(f"✅ subscribed to {args.topic}")

    if args.once:
        print(f"waiting for first message{f' (max {args.timeout}s)' if args.timeout else ''}...")
        if not got_message.wait(timeout=args.timeout or None):
            print("TIMEOUT: no message received", file=sys.stderr)
            client.stop()
            sys.exit(1)
        client.stop()
    else:
        print("watching (Ctrl-C to stop)...")
        try:
            threading.Event().wait()
        except KeyboardInterrupt:
            client.stop()


if __name__ == "__main__":
    main()
