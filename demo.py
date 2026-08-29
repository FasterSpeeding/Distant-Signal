"""
End-to-end demo. Three scenarios run in sequence:

  1. WCML trespass between Watford Junction and Milton Keynes (exclusive
     to WCML — no propagation).
  2. SWR signal failure at Woking (a shared SWR trunk station — propagates
     to South West Main, Portsmouth Direct, and Alton).
  3. SWR power supply problem at Alton (an exclusive segment — only the
     Alton line gets a status).

For each scenario we print the resulting status per line, plus the
match scope so you can see the segment logic working.

Run:
    PYTHONPATH=. python demo.py
"""

from datetime import datetime, timedelta, timezone
from pathlib import Path

from src.aggregator import aggregate
from src.loader import load_all_lines
from src.render import to_tfl_shape
from src.types import IncidentMessage, StationDeparture, StationSample


def main():
    lines = load_all_lines(Path(__file__).parent / "lines")
    print(f"Loaded {len(lines)} lines:")
    for line_id, line in lines.items():
        segs = sorted(line.segments())
        print(f"  - {line_id}: {len(line.stations)} stations, segments {segs}")
    print()

    now = datetime.now(timezone.utc)
    empty_samples: dict[str, StationSample] = {}

    # --- Scenario 1: WCML exclusive-segment incident ---
    print("=" * 70)
    print("Scenario 1: WCML trespass between Watford Junction and Milton Keynes")
    print("=" * 70)
    wcml_incident = IncidentMessage(
        incident_id="DEMO-WCML-001",
        summary="Major disruption between Watford Junction and Milton Keynes Central",
        description="Lines blocked between Watford Junction and Milton Keynes "
                    "Central due to a trespass incident. Severe delays expected.",
        operators=["VT", "LM"],
        affected_stations=["WFJ", "MKC"],
        severity_hint="major",
        valid_from=now - timedelta(minutes=20),
    )
    summarise(aggregate(lines, [wcml_incident], empty_samples))

    # --- Scenario 2: SWR shared-trunk incident at Woking ---
    print("=" * 70)
    print("Scenario 2: SWR signal failure at Woking (shared trunk)")
    print("=" * 70)
    woking_incident = IncidentMessage(
        incident_id="DEMO-SWR-001",
        summary="Signal failure at Woking",
        description="Signalling problem at Woking is causing severe delays to "
                    "South Western Railway services through the area.",
        operators=["SW"],
        affected_stations=["WOK"],
        severity_hint="major",
        valid_from=now - timedelta(minutes=10),
    )
    summarise(aggregate(lines, [woking_incident], empty_samples))

    # --- Scenario 3: SWR exclusive-segment incident at Alton ---
    print("=" * 70)
    print("Scenario 3: Power supply problem on Alton branch")
    print("=" * 70)
    alton_incident = IncidentMessage(
        incident_id="DEMO-SWR-002",
        summary="Minor delays on the Alton line",
        description="A power supply problem at Alton is causing minor delays "
                    "to services on the Alton branch.",
        operators=["SW"],
        affected_stations=["AON", "BTL"],
        severity_hint="minor",
        valid_from=now - timedelta(minutes=5),
    )
    summarise(aggregate(lines, [alton_incident], empty_samples))


def summarise(reports):
    """One-line summary of each line's status."""
    for report in reports.values():
        worst = report.statuses[0] if report.statuses else None
        if worst is None:
            continue
        sev = worst.severity
        desc = worst.reason
        quality = worst.data_quality
        print(f"  {report.id:30s} sev={int(sev):2d}  {desc[:60]}  [{quality}]")
    print()


if __name__ == "__main__":
    main()
