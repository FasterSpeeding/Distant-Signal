"""
Domain types for the National Rail status aggregator.

Response shapes mirror TfL's Unified API where it makes sense, so any
client already built against TfL can largely be reused.
"""

from dataclasses import dataclass, field
from datetime import datetime
from enum import IntEnum
from typing import Optional


class Severity(IntEnum):
    """
    Status severity scale. Mirrors TfL's `statusSeverity` codes 0-14
    where the meanings carry over, with a few additions for NR-specific
    cases. Lower is worse, except 0 (Special Service) and 10 (Good Service)
    which are the canonical "fine" states. Sort by severity for UI ordering
    of disrupted lines first.
    """
    SPECIAL_SERVICE = 0
    CLOSED = 1
    SUSPENDED = 2
    PART_SUSPENDED = 3
    PLANNED_CLOSURE = 4
    PART_CLOSURE = 5
    SEVERE_DELAYS = 6
    REDUCED_SERVICE = 7
    BUS_SERVICE = 8           # rail replacement
    MINOR_DELAYS = 9
    GOOD_SERVICE = 10
    PART_CLOSED = 11
    EXIT_ONLY = 12
    NO_STEP_FREE = 13
    CHANGE_OF_FREQUENCY = 14
    # NR-specific extensions, kept separate from TfL's range to avoid clashes
    RECOVERING = 20            # post-incident catch-up
    DIVERTED = 21              # services running but on alternative route


SEVERITY_DESCRIPTIONS = {
    Severity.SPECIAL_SERVICE: "Special Service",
    Severity.CLOSED: "Closed",
    Severity.SUSPENDED: "Suspended",
    Severity.PART_SUSPENDED: "Part Suspended",
    Severity.PLANNED_CLOSURE: "Planned Closure",
    Severity.PART_CLOSURE: "Part Closure",
    Severity.SEVERE_DELAYS: "Severe Delays",
    Severity.REDUCED_SERVICE: "Reduced Service",
    Severity.BUS_SERVICE: "Rail Replacement",
    Severity.MINOR_DELAYS: "Minor Delays",
    Severity.GOOD_SERVICE: "Good Service",
    Severity.PART_CLOSED: "Part Closed",
    Severity.EXIT_ONLY: "Exit Only",
    Severity.NO_STEP_FREE: "No Step Free Access",
    Severity.CHANGE_OF_FREQUENCY: "Change of Frequency",
    Severity.RECOVERING: "Recovering",
    Severity.DIVERTED: "Diverted",
}


class DataQuality(str):
    """How confident are we in this status?"""
    KNOWLEDGEBASE = "knowledgebase"   # human-curated NR incident message
    LDBWS_INFERRED = "ldbws-inferred" # derived from departure board sampling
    TRUST_INFERRED = "trust-inferred" # derived from movement events
    PLANNED = "planned"               # from engineering works data


@dataclass
class Station:
    """
    A station as it appears on one specific line.

    `segment` groups consecutive stations into a named section of track.
    Segments shared between lines (same name appearing in multiple line
    definitions) represent shared trunks; segments unique to a line are
    that line's exclusive sections. The matcher and aggregator use this
    to decide whether an incident propagates to other lines.
    """
    crs: str                 # 3-letter National Rail station code
    tiploc: Optional[str] = None
    role: str = "minor"      # terminus | major | minor | junction
    segment: Optional[str] = None   # e.g. "trunk-waterloo", "portsmouth-direct"


@dataclass
class LineDefinition:
    """A user-facing 'line' the aggregator reports status for."""
    id: str
    name: str
    mode: str
    category: str
    operators: list[str]
    stations: list[Station]
    sample_stations: list[str] = field(default_factory=list)
    match_keywords: list[str] = field(default_factory=list)
    excluded_keywords: list[str] = field(default_factory=list)
    severity_overrides: dict = field(default_factory=dict)
    # Segments this line considers exclusive (not shared with other lines).
    # Optional: if omitted, the matcher derives exclusivity by comparing
    # segment usage across all loaded lines.
    exclusive_segments: list[str] = field(default_factory=list)
    # Optional service-pattern hints — destination CRS or headcode prefix
    # filters used during LDBWS inference to distinguish this line's
    # services from sibling services at shared stations.
    destination_crs_filter: list[str] = field(default_factory=list)
    headcode_prefixes: list[str] = field(default_factory=list)

    def has_station(self, crs: str) -> bool:
        return any(s.crs == crs for s in self.stations)

    def segment_for(self, crs: str) -> Optional[str]:
        for s in self.stations:
            if s.crs == crs:
                return s.segment
        return None

    def segments(self) -> set[str]:
        return {s.segment for s in self.stations if s.segment}

    def stations_between(self, from_crs: str, to_crs: str) -> list[str]:
        """Return CRS codes between two stations inclusive, in order."""
        crs_list = [s.crs for s in self.stations]
        if from_crs not in crs_list or to_crs not in crs_list:
            return []
        i, j = crs_list.index(from_crs), crs_list.index(to_crs)
        if i > j:
            i, j = j, i
        return crs_list[i:j + 1]


@dataclass
class ValidityPeriod:
    from_date: datetime
    to_date: Optional[datetime]
    is_now: bool


@dataclass
class AffectedRoute:
    from_crs: str
    to_crs: str


@dataclass
class Disruption:
    category: str            # 'RealTime' | 'PlannedWork' | 'Information'
    description: str
    affected_stops: list[str] = field(default_factory=list)
    affected_routes: list[AffectedRoute] = field(default_factory=list)
    source: Optional[str] = None  # e.g. "knowledgebase-incident-12345"


@dataclass
class LineStatus:
    """One status entry on a line. A line may have several simultaneously."""
    severity: Severity
    reason: str
    validity: ValidityPeriod
    disruption: Optional[Disruption] = None
    data_quality: str = DataQuality.KNOWLEDGEBASE


@dataclass
class LineStatusReport:
    """Top-level object returned by the API for one line."""
    id: str
    name: str
    mode_name: str
    operators: list[str]
    statuses: list[LineStatus]

    @property
    def worst_severity(self) -> Severity:
        """Lowest numeric severity is the most disruptive."""
        if not self.statuses:
            return Severity.GOOD_SERVICE
        return min(s.severity for s in self.statuses)


# --- Inputs the aggregator consumes ---

@dataclass
class IncidentMessage:
    """A parsed Knowledgebase incident."""
    incident_id: str
    summary: str
    description: str
    operators: list[str]      # ATOC codes mentioned
    affected_stations: list[str]   # CRS codes parsed from message
    severity_hint: Optional[str] = None  # 'major' | 'minor' if NR tagged it
    valid_from: Optional[datetime] = None
    valid_to: Optional[datetime] = None
    is_planned: bool = False


@dataclass
class StationDeparture:
    """One service from an LDBWS departure board."""
    service_id: str
    operator: str
    destination_crs: str
    scheduled: str            # 'std' field
    estimated: str            # 'etd' — may be 'On time', 'Cancelled', or HH:MM
    is_cancelled: bool
    delay_minutes: int        # 0 if on time
    cancel_reason: Optional[str] = None
    delay_reason: Optional[str] = None
    headcode: Optional[str] = None      # e.g. "1P23", from Darwin's `trainid`/`rid`


@dataclass
class StationSample:
    """An LDBWS poll result for one station along a line."""
    crs: str
    polled_at: datetime
    departures: list[StationDeparture]
