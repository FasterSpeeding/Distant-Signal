"""Default thresholds for status derivation. Lines may override per-line."""

DEFAULTS = {
    # When inferring from LDBWS sampling
    "delay_threshold_minutes": 5,        # service is "delayed" above this
    "minor_delays_pct": 0.25,            # >25% of services delayed -> Minor Delays
    "severe_delays_pct": 0.50,           # >50% of services delayed -> Severe Delays
    "reduced_service_pct": 0.25,         # >25% cancelled -> Reduced Service
    "part_suspended_pct": 0.60,          # >60% cancelled -> Part Suspended

    # Knowledgebase incident handling
    "knowledgebase_severity_floor": 9,   # an active KB incident is at least Minor Delays

    # Sample sizing
    "min_sample_size": 3,                # below this many services, don't infer alone
}


def thresholds_for(line_overrides: dict) -> dict:
    """Merge defaults with per-line overrides."""
    merged = DEFAULTS.copy()
    merged.update(line_overrides or {})
    return merged
