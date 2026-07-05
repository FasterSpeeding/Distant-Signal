"""
Decide which lines a Knowledgebase incident affects, AND classify the
scope of each match so the aggregator can size the response appropriately.

Match scopes:

- `EXCLUSIVE_SEGMENT` — incident's affected stations all sit on a segment
  that belongs only to this line. Highest confidence; this incident is
  this line's problem and nobody else's.
- `SHARED_SEGMENT` — incident's affected stations sit on a trunk segment
  shared with other lines. Every line using that segment gets a status,
  with the shared nature flagged.
- `STATION_HIT` — incident references stations on the line but no segment
  metadata is available. Falls back to the v1 behaviour.
- `KEYWORD_ONLY` — the incident text mentions the line by name (or one of
  its match_keywords) but doesn't pinpoint stations.
- `OPERATOR_ONLY` — operator overlap only, no station or keyword evidence.
  Treated as a possible network-wide event for that operator.

Excluded keywords still veto matches at any scope.
"""

from dataclasses import dataclass
from enum import Enum

from .segments import SegmentRegistry
from .types import IncidentMessage, LineDefinition


class MatchScope(str, Enum):
    EXCLUSIVE_SEGMENT = "exclusive-segment"
    SHARED_SEGMENT = "shared-segment"
    STATION_HIT = "station-hit"
    KEYWORD_ONLY = "keyword-only"
    OPERATOR_ONLY = "operator-only"


@dataclass
class Match:
    line: LineDefinition
    scope: MatchScope
    evidence: dict


def lines_affected_by(
    incident: IncidentMessage,
    lines: dict[str, LineDefinition],
    registry: SegmentRegistry,
) -> list[Match]:
    """Return all lines the incident could plausibly affect, classified."""
    out: list[Match] = []
    haystack = (incident.summary + " " + incident.description).lower()

    for line in lines.values():
        if _is_excluded(line, haystack):
            continue

        match = _match_one(line, incident, registry, haystack)
        if match:
            out.append(match)

    # If we have any precise match (segment-classified, station-hit, or
    # keyword), drop operator-only matches — they're almost certainly
    # false positives where another line on the same operator is the
    # actual target. This is what stops an incident on the Alton branch
    # from also flagging South West Main and Portsmouth Direct just
    # because they all share the SW operator code.
    has_precise = any(m.scope != MatchScope.OPERATOR_ONLY for m in out)
    if has_precise:
        out = [m for m in out if m.scope != MatchScope.OPERATOR_ONLY]

    return out


def _match_one(
    line: LineDefinition,
    incident: IncidentMessage,
    registry: SegmentRegistry,
    haystack: str,
) -> Match | None:
    operator_overlap = set(line.operators) & set(incident.operators)
    station_hits = [crs for crs in incident.affected_stations if line.has_station(crs)]
    keyword_hits = [kw for kw in line.match_keywords if kw.lower() in haystack]

    # Tier 1: station hits — try to classify by segment.
    if station_hits:
        segments = registry.segments_touched_by(line, station_hits)
        evidence = {
            "stations": station_hits,
            "segments": list(segments),
            "operators": list(operator_overlap) or None,
            "keywords": keyword_hits or None,
        }

        # All touched segments exclusive to this line -> definitely this line.
        if segments and all(registry.is_exclusive_to(s, line.id) for s in segments):
            return Match(line, MatchScope.EXCLUSIVE_SEGMENT, evidence)

        # At least one touched segment is shared -> shared trunk match.
        if segments and any(registry.is_shared(s) for s in segments):
            return Match(line, MatchScope.SHARED_SEGMENT, evidence)

        # Stations on the line but no segment metadata to classify with.
        return Match(line, MatchScope.STATION_HIT, evidence)

    # Tier 2: keyword match (the incident names the line).
    if keyword_hits:
        return Match(
            line,
            MatchScope.KEYWORD_ONLY,
            {"keywords": keyword_hits, "operators": list(operator_overlap) or None},
        )

    # Tier 3: operator only — softest signal, used for "TOC reporting widespread delays".
    if operator_overlap:
        return Match(
            line,
            MatchScope.OPERATOR_ONLY,
            {"operators": list(operator_overlap)},
        )

    return None


def _is_excluded(line: LineDefinition, haystack: str) -> bool:
    return any(kw.lower() in haystack for kw in line.excluded_keywords)
