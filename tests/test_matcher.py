"""Tests for the segment-aware matcher and aggregator."""

from pathlib import Path

from src.aggregator import aggregate
from src.loader import load_all_lines, load_line
from src.matcher import MatchScope, lines_affected_by
from src.segments import SegmentRegistry
from src.types import IncidentMessage, Severity


LINES_DIR = Path(__file__).parent.parent / "lines"


def all_lines():
    return load_all_lines(LINES_DIR)


def test_excluded_keyword_vetoes_match():
    lines = {
        "wcml": load_line(LINES_DIR / "west-coast-main-line.toml"),
    }
    registry = SegmentRegistry(lines)
    incident = IncidentMessage(
        incident_id="T1",
        summary="Cross Country delays",
        description="Cross Country services are delayed at Rugby.",
        operators=[],
        affected_stations=["RUG"],
    )
    matches = lines_affected_by(incident, lines, registry)
    assert not matches, "excluded keyword should veto match"


def test_keyword_only_match():
    lines = {
        "wcml": load_line(LINES_DIR / "west-coast-main-line.toml"),
    }
    registry = SegmentRegistry(lines)
    incident = IncidentMessage(
        incident_id="T2",
        summary="WCML engineering",
        description="Overnight engineering work on the West Coast Main Line.",
        operators=[],
        affected_stations=[],
    )
    matches = lines_affected_by(incident, lines, registry)
    assert len(matches) == 1
    assert matches[0].scope == MatchScope.KEYWORD_ONLY


def test_swr_shared_trunk_incident_propagates():
    """Woking is on the shared SWR trunk; an incident there must hit all three SWR lines."""
    lines = all_lines()
    registry = SegmentRegistry(lines)
    incident = IncidentMessage(
        incident_id="SWR-1",
        summary="Signal failure at Woking",
        description="Signal failure causing delays to SWR services.",
        operators=["SW"],
        affected_stations=["WOK"],
        severity_hint="major",
    )
    matches = lines_affected_by(incident, lines, registry)
    matched_ids = {m.line.id for m in matches}
    assert "swr-south-west-main" in matched_ids
    assert "swr-portsmouth-direct" in matched_ids
    assert "swr-alton" in matched_ids
    for m in matches:
        if m.line.id.startswith("swr-"):
            assert m.scope == MatchScope.SHARED_SEGMENT, (
                f"{m.line.id} should be SHARED_SEGMENT, got {m.scope}"
            )


def test_swr_exclusive_segment_incident_does_not_propagate():
    """An incident at Alton (exclusive to the Alton line) must NOT hit the others."""
    lines = all_lines()
    registry = SegmentRegistry(lines)
    incident = IncidentMessage(
        incident_id="SWR-2",
        summary="Power supply issue at Alton",
        description="Power supply problem causing delays at Alton.",
        operators=["SW"],
        affected_stations=["AON"],
        severity_hint="minor",
    )
    matches = lines_affected_by(incident, lines, registry)
    matched_ids = {m.line.id for m in matches}
    assert matched_ids == {"swr-alton"}, f"expected only swr-alton, got {matched_ids}"
    assert matches[0].scope == MatchScope.EXCLUSIVE_SEGMENT


def test_aggregator_propagates_severity_through_shared_trunk():
    """The Woking incident should produce a Severe-Delays-class status on every SWR line."""
    lines = all_lines()
    incident = IncidentMessage(
        incident_id="SWR-3",
        summary="Signal failure at Woking",
        description="Severe delays expected on SWR services.",
        operators=["SW"],
        affected_stations=["WOK"],
        severity_hint="major",
    )
    reports = aggregate(lines, [incident], samples={})
    for line_id in ["swr-south-west-main", "swr-portsmouth-direct", "swr-alton"]:
        worst = reports[line_id].worst_severity
        assert int(worst) <= int(Severity.SEVERE_DELAYS), (
            f"{line_id} should have severe-or-worse severity, got {worst}"
        )


def test_aggregator_isolates_exclusive_incident():
    """The Alton incident should only show on swr-alton; siblings stay Good Service."""
    lines = all_lines()
    incident = IncidentMessage(
        incident_id="SWR-4",
        summary="Minor delays on Alton line",
        description="A power supply problem at Alton is causing minor delays.",
        operators=["SW"],
        affected_stations=["AON"],
        severity_hint="minor",
    )
    reports = aggregate(lines, [incident], samples={})
    assert reports["swr-alton"].worst_severity == Severity.MINOR_DELAYS
    assert reports["swr-south-west-main"].worst_severity == Severity.GOOD_SERVICE
    assert reports["swr-portsmouth-direct"].worst_severity == Severity.GOOD_SERVICE


def test_operator_only_match_is_demoted_to_minor():
    """A vague 'operator reporting delays' message must not Suspend an entire line."""
    lines = all_lines()
    incident = IncidentMessage(
        incident_id="OP-1",
        summary="SWR services suspended",
        description="No service on SWR following an earlier incident.",  # would normally => SUSPENDED
        operators=["SW"],
        affected_stations=[],   # no stations -> operator-only
    )
    reports = aggregate(lines, [incident], samples={})
    # SWR lines should be Minor Delays at worst (capped from Suspended).
    for line_id in ["swr-south-west-main", "swr-portsmouth-direct", "swr-alton"]:
        worst = reports[line_id].worst_severity
        assert worst == Severity.MINOR_DELAYS, (
            f"{line_id} should be capped at Minor Delays, got {worst}"
        )


if __name__ == "__main__":
    test_excluded_keyword_vetoes_match()
    test_keyword_only_match()
    test_swr_shared_trunk_incident_propagates()
    test_swr_exclusive_segment_incident_does_not_propagate()
    test_aggregator_propagates_severity_through_shared_trunk()
    test_aggregator_isolates_exclusive_incident()
    test_operator_only_match_is_demoted_to_minor()
    print("All tests passed.")
