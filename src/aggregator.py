"""
The aggregator: combine Knowledgebase incidents and LDBWS samples into
one status report per line.

This version is segment-aware, so it correctly handles operators with
multiple parallel routes that share trunk sections (e.g. SWR's network
out of Waterloo). Three things change vs v1:

1. The matcher now classifies each match by scope (exclusive segment,
   shared trunk, etc.). The aggregator uses scope to decide whether to
   propagate a status to sibling lines, and how confident to be.

2. Inference filters services not just by operator but optionally by
   destination CRS or headcode prefix. This lets a line like the
   Portsmouth Direct see only its own services at shared stations
   like Woking.

3. Severity is gently demoted for weaker match scopes — an operator-only
   match never produces "Severe Delays" on its own, only "Minor Delays",
   because the evidence is too thin to justify more.
"""

from datetime import datetime, timezone

from .config import thresholds_for
from .matcher import Match, MatchScope, lines_affected_by
from .segments import SegmentRegistry
from .types import (
    AffectedRoute,
    DataQuality,
    Disruption,
    IncidentMessage,
    LineDefinition,
    LineStatus,
    LineStatusReport,
    Severity,
    SEVERITY_DESCRIPTIONS,
    StationDeparture,
    StationSample,
    ValidityPeriod,
)


def aggregate(
    lines: dict[str, LineDefinition],
    incidents: list[IncidentMessage],
    samples: dict[str, StationSample],
    registry: SegmentRegistry | None = None,
) -> dict[str, LineStatusReport]:
    """Build a status report per line."""
    if registry is None:
        registry = SegmentRegistry(lines)

    reports: dict[str, LineStatusReport] = {
        line.id: LineStatusReport(
            id=line.id,
            name=line.name,
            mode_name=line.mode,
            operators=line.operators,
            statuses=[],
        )
        for line in lines.values()
    }

    # Layer 1: incidents.
    for incident in incidents:
        for match in lines_affected_by(incident, lines, registry):
            status = _status_from_incident(match, incident)
            reports[match.line.id].statuses.append(status)

    # Layer 2: inference for lines with no incidents.
    for line in lines.values():
        if reports[line.id].statuses:
            continue
        inferred = _infer_from_samples(line, samples)
        reports[line.id].statuses.append(inferred or _good_service())

    return reports


# --- Incident path ---------------------------------------------------

def _status_from_incident(match: Match, incident: IncidentMessage) -> LineStatus:
    line = match.line
    base_severity = _severity_from_incident(incident)
    severity = _demote_for_scope(base_severity, match.scope)

    affected_stations = list(match.evidence.get("stations") or [])
    affected_routes = _routes_from_stations(line, affected_stations)

    reason = incident.summary
    if match.scope == MatchScope.SHARED_SEGMENT:
        reason = f"{reason} (shared trunk — also affects other lines)"
    elif match.scope == MatchScope.OPERATOR_ONLY:
        reason = f"{reason} (operator-wide report)"

    disruption = Disruption(
        category="PlannedWork" if incident.is_planned else "RealTime",
        description=incident.description or incident.summary,
        affected_stops=affected_stations,
        affected_routes=affected_routes,
        source=f"knowledgebase-incident-{incident.incident_id}",
    )

    validity = ValidityPeriod(
        from_date=incident.valid_from or _now(),
        to_date=incident.valid_to,
        is_now=True,
    )

    return LineStatus(
        severity=severity,
        reason=reason,
        validity=validity,
        disruption=disruption,
        data_quality=DataQuality.PLANNED if incident.is_planned else DataQuality.KNOWLEDGEBASE,
    )


def _severity_from_incident(incident: IncidentMessage) -> Severity:
    if incident.is_planned:
        return Severity.PLANNED_CLOSURE

    text = (incident.summary + " " + incident.description).lower()
    if "suspended" in text or "no service" in text:
        return Severity.SUSPENDED
    if "rail replacement" in text or "replacement bus" in text:
        return Severity.BUS_SERVICE
    if "lines blocked" in text or "all lines blocked" in text:
        return Severity.PART_SUSPENDED
    if "severe delays" in text or "major disruption" in text:
        return Severity.SEVERE_DELAYS
    if incident.severity_hint == "major":
        return Severity.SEVERE_DELAYS
    if "diverted" in text:
        return Severity.DIVERTED
    if "minor delays" in text or incident.severity_hint == "minor":
        return Severity.MINOR_DELAYS
    return Severity.MINOR_DELAYS


def _demote_for_scope(severity: Severity, scope: MatchScope) -> Severity:
    """
    Weaker evidence -> milder reported status. Prevents a vague
    operator-only message from suspending an entire line.

    Lower severity numbers are more disruptive (Suspended=2, Good Service=10),
    so capping "at Minor Delays or milder" means `max(severity, MINOR_DELAYS)`.
    """
    if scope in (MatchScope.EXCLUSIVE_SEGMENT, MatchScope.STATION_HIT):
        return severity
    if scope == MatchScope.SHARED_SEGMENT:
        # Shared trunk: usually keep severity; trunk problems really do affect everyone.
        return severity
    if scope == MatchScope.KEYWORD_ONLY:
        # Reasonable signal but no station precision — cap at Severe Delays.
        return Severity(max(int(severity), int(Severity.SEVERE_DELAYS)))
    if scope == MatchScope.OPERATOR_ONLY:
        # Softest evidence — never report worse than Minor Delays from this alone.
        return Severity(max(int(severity), int(Severity.MINOR_DELAYS)))
    return severity


def _routes_from_stations(line: LineDefinition, stations: list[str]) -> list[AffectedRoute]:
    if len(stations) < 2:
        return []
    line_order = [s.crs for s in line.stations]
    in_order = sorted(stations, key=lambda c: line_order.index(c) if c in line_order else 999)
    return [AffectedRoute(from_crs=in_order[0], to_crs=in_order[-1])]


# --- Inference path --------------------------------------------------

def _infer_from_samples(
    line: LineDefinition,
    samples: dict[str, StationSample],
) -> LineStatus | None:
    thresholds = thresholds_for(line.severity_overrides)
    delay_threshold = thresholds["delay_threshold_minutes"]

    relevant: list[StationDeparture] = []
    for crs in line.sample_stations:
        if crs in samples:
            for dep in samples[crs].departures:
                if _belongs_to_line(dep, line):
                    relevant.append(dep)

    if len(relevant) < thresholds["min_sample_size"]:
        return None

    total = len(relevant)
    cancelled = sum(1 for d in relevant if d.is_cancelled)
    delayed = sum(
        1 for d in relevant
        if not d.is_cancelled and d.delay_minutes >= delay_threshold
    )
    cancel_rate = cancelled / total
    delay_rate = delayed / total

    severity, reason = _classify(cancel_rate, delay_rate, thresholds, total, cancelled, delayed)
    if severity == Severity.GOOD_SERVICE:
        return _good_service()

    reasons = [d.delay_reason or d.cancel_reason for d in relevant if d.delay_reason or d.cancel_reason]
    if reasons:
        reason += f" (most cited: {_most_common(reasons)})"

    return LineStatus(
        severity=severity,
        reason=reason,
        validity=ValidityPeriod(from_date=_now(), to_date=None, is_now=True),
        disruption=Disruption(
            category="RealTime",
            description=reason,
            affected_stops=list(samples.keys()),
            source="ldbws-sampling",
        ),
        data_quality=DataQuality.LDBWS_INFERRED,
    )


def _belongs_to_line(dep: StationDeparture, line: LineDefinition) -> bool:
    """
    Decide whether a sampled departure represents one of this line's services.

    Operator filter is mandatory. Then optionally narrow by destination CRS
    or headcode prefix — useful when a sample station is on a shared trunk
    and would otherwise pull in sibling-line services.
    """
    if dep.operator not in line.operators:
        return False

    if line.destination_crs_filter:
        if dep.destination_crs not in line.destination_crs_filter:
            return False

    if line.headcode_prefixes:
        if not dep.headcode:
            return False  # can't classify; exclude to avoid false positives
        if not any(dep.headcode.startswith(p) for p in line.headcode_prefixes):
            return False

    return True


def _classify(cancel_rate, delay_rate, thresholds, total, cancelled, delayed):
    if cancel_rate >= thresholds["part_suspended_pct"]:
        return Severity.PART_SUSPENDED, f"{cancelled} of {total} sampled services cancelled."
    if cancel_rate >= thresholds["reduced_service_pct"]:
        return Severity.REDUCED_SERVICE, f"{cancelled} of {total} sampled services cancelled."
    if delay_rate >= thresholds["severe_delays_pct"]:
        return Severity.SEVERE_DELAYS, f"{delayed} of {total} sampled services delayed."
    if delay_rate >= thresholds["minor_delays_pct"]:
        return Severity.MINOR_DELAYS, f"{delayed} of {total} sampled services delayed."
    return Severity.GOOD_SERVICE, "Good Service"


def _good_service() -> LineStatus:
    return LineStatus(
        severity=Severity.GOOD_SERVICE,
        reason=SEVERITY_DESCRIPTIONS[Severity.GOOD_SERVICE],
        validity=ValidityPeriod(from_date=_now(), to_date=None, is_now=True),
        disruption=None,
        data_quality=DataQuality.LDBWS_INFERRED,
    )


def _most_common(items):
    return max(set(items), key=items.count)


def _now() -> datetime:
    return datetime.now(timezone.utc)
