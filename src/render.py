"""Render LineStatusReports as TfL-shaped JSON dictionaries."""

from .types import LineStatusReport, SEVERITY_DESCRIPTIONS


def to_tfl_shape(report: LineStatusReport, detail: bool = False) -> dict:
    """
    TfL-compatible dictionary. The `detail` flag controls whether affected stops
    and routes are included, matching TfL's `?detail=true` semantics.
    """
    return {
        "$type": "NRStatus.LineStatusReport",
        "id": report.id,
        "name": report.name,
        "modeName": report.mode_name,
        "operators": report.operators,
        "lineStatuses": [_status_to_dict(s, detail) for s in report.statuses],
    }


def _status_to_dict(status, detail: bool) -> dict:
    out = {
        "statusSeverity": int(status.severity),
        "statusSeverityDescription": SEVERITY_DESCRIPTIONS[status.severity],
        "reason": status.reason,
        "dataQuality": status.data_quality,
        "validityPeriods": [
            {
                "fromDate": status.validity.from_date.isoformat(),
                "toDate": status.validity.to_date.isoformat() if status.validity.to_date else None,
                "isNow": status.validity.is_now,
            }
        ],
    }
    if status.disruption and detail:
        out["disruption"] = {
            "category": status.disruption.category,
            "description": status.disruption.description,
            "affectedStops": status.disruption.affected_stops,
            "affectedRoutes": [
                {"from": r.from_crs, "to": r.to_crs}
                for r in status.disruption.affected_routes
            ],
            "source": status.disruption.source,
        }
    return out
