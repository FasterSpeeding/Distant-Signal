"""
Segment registry: cross-line view of which segments are shared and which
are exclusive, derived from the full set of loaded line definitions.

Built once at startup (or on line-definition reload) and consulted by the
matcher and aggregator. Keeps the per-line TOML simple — authors don't
have to know what other lines exist.
"""

from collections import defaultdict

from .types import LineDefinition


class SegmentRegistry:
    """Indexes segment usage across all known lines."""

    def __init__(self, lines: dict[str, LineDefinition]):
        # segment -> ordered list of unique line IDs that include it
        self._segment_lines: dict[str, list[str]] = defaultdict(list)
        # (line_id, crs) -> segment
        self._station_segments: dict[tuple[str, str], str] = {}

        for line in lines.values():
            for station in line.stations:
                if station.segment:
                    if line.id not in self._segment_lines[station.segment]:
                        self._segment_lines[station.segment].append(line.id)
                    self._station_segments[(line.id, station.crs)] = station.segment

    def lines_for_segment(self, segment: str) -> list[str]:
        """Every line ID that includes this segment, in load order."""
        return list(self._segment_lines.get(segment, []))

    def is_shared(self, segment: str) -> bool:
        """A segment is shared if more than one line uses it."""
        return len(self._segment_lines.get(segment, [])) > 1

    def is_exclusive_to(self, segment: str, line_id: str) -> bool:
        """True if `line_id` is the only line using this segment."""
        users = self._segment_lines.get(segment, [])
        return users == [line_id]

    def segment_at(self, line_id: str, crs: str) -> str | None:
        return self._station_segments.get((line_id, crs))

    def segments_touched_by(
        self,
        line: LineDefinition,
        affected_stations: list[str],
    ) -> set[str]:
        """Which of this line's segments are touched by these stations."""
        out = set()
        for crs in affected_stations:
            seg = self._station_segments.get((line.id, crs))
            if seg:
                out.add(seg)
        return out
