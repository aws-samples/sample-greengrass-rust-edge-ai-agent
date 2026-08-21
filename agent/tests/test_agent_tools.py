"""Unit tests for the Strands agent tools (mocked boto3 clients)."""

import json
import sys
from pathlib import Path
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).parent.parent))

from tools.publish_response import publish_response
from tools.query_history import _stats, query_history


def _invoke(tool_fn, **kwargs):
    """Call the undecorated function behind a @tool."""
    inner = getattr(tool_fn, "original_function", None) or getattr(
        tool_fn, "_tool_func", None
    )
    if inner is None:  # plain function or unknown strands version
        inner = tool_fn
    return inner(**kwargs)


class TestStats:
    def test_empty_series(self):
        assert _stats([], []) == {
            "mean": None,
            "stddev": None,
            "trend_per_hour": None,
        }

    def test_single_value(self):
        result = _stats([5.0], [0.0])
        assert result["mean"] == 5.0
        assert result["stddev"] == 0.0
        assert result["trend_per_hour"] == 0.0

    def test_constant_series_has_zero_trend(self):
        values = [10.0] * 24
        timestamps = [i * 3600.0 for i in range(24)]
        result = _stats(values, timestamps)
        assert result["mean"] == 10.0
        assert result["stddev"] == 0.0
        assert result["trend_per_hour"] == 0.0

    def test_linear_increase_recovers_slope(self):
        # value = 2.0 * hours -> slope 2.0/hour
        timestamps = [i * 3600.0 for i in range(48)]
        values = [2.0 * i for i in range(48)]
        result = _stats(values, timestamps)
        assert abs(result["trend_per_hour"] - 2.0) < 1e-6


class TestQueryHistory:
    @patch("tools.query_history.boto3")
    def test_computes_stats_from_query_results(self, mock_boto3):
        table = MagicMock()
        mock_boto3.resource.return_value.Table.return_value = table
        table.query.return_value = {
            "Items": [
                {
                    "ts": "2026-07-16T00:00:00Z",
                    "flow_rate": "100.0",
                    "pressure": "50.0",
                    "vibration": "1.0",
                    "temperature": "20.0",
                },
                {
                    "ts": "2026-07-16T01:00:00Z",
                    "flow_rate": "102.0",
                    "pressure": "49.0",
                    "vibration": "1.2",
                    "temperature": "21.0",
                },
            ]
        }
        result = _invoke(query_history, thing_name="pump-station-001", hours=168)
        assert result["record_count"] == 2
        assert result["flow_rate"]["mean"] == 101.0
        assert result["pressure"]["mean"] == 49.5
        # pressure declining 1.0/hour
        assert abs(result["pressure"]["trend_per_hour"] + 1.0) < 1e-6

    @patch("tools.query_history.boto3")
    def test_paginates_through_results(self, mock_boto3):
        table = MagicMock()
        mock_boto3.resource.return_value.Table.return_value = table
        page1 = {
            "Items": [{"ts": "2026-07-16T00:00:00Z", "flow_rate": "1.0"}],
            "LastEvaluatedKey": {"thing_name": "x", "ts": "y"},
        }
        page2 = {"Items": [{"ts": "2026-07-16T01:00:00Z", "flow_rate": "3.0"}]}
        table.query.side_effect = [page1, page2]
        result = _invoke(query_history, thing_name="pump-station-001")
        assert result["record_count"] == 2
        assert result["flow_rate"]["mean"] == 2.0
        assert table.query.call_count == 2

    @patch("tools.query_history.boto3")
    def test_empty_table_returns_none_stats(self, mock_boto3):
        table = MagicMock()
        mock_boto3.resource.return_value.Table.return_value = table
        table.query.return_value = {"Items": []}
        result = _invoke(query_history, thing_name="pump-station-404")
        assert result["record_count"] == 0
        assert result["flow_rate"]["mean"] is None


class TestPublishResponse:
    @patch("tools.publish_response.boto3")
    def test_publishes_to_recommendations_topic(self, mock_boto3):
        client = MagicMock()
        mock_boto3.client.return_value = client
        recommendation = {
            "severity": "HIGH",
            "probable_cause": "BEARING_WEAR",
            "recommended_action": "SCHEDULE_MAINTENANCE",
            "evidence": "Vibration trend +12%/day.",
        }
        result = _invoke(
            publish_response, thing_name="pump-station-007", response=recommendation
        )
        assert result == {
            "published": True,
            "topic": "pump-stations/pump-station-007/recommendations",
        }
        client.publish.assert_called_once()
        call = client.publish.call_args.kwargs
        assert call["topic"] == "pump-stations/pump-station-007/recommendations"
        assert call["qos"] == 1
        assert json.loads(call["payload"]) == recommendation
