"""Load and validate line definitions from TOML files."""

from pathlib import Path

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ImportError:
        raise ImportError("Python 3.11+ required, or install tomli: pip install tomli")

from .types import LineDefinition, Station


def load_line(path: Path) -> LineDefinition:
    with open(path, "rb") as f:
        data = tomllib.load(f)

    required = {"id", "name", "mode", "category", "operators", "stations"}
    missing = required - data.keys()
    if missing:
        raise ValueError(f"{path.name}: missing required fields: {missing}")

    stations = [
        Station(
            crs=s["crs"],
            tiploc=s.get("tiploc"),
            role=s.get("role", "minor"),
            segment=s.get("segment"),
        )
        for s in data["stations"]
    ]

    sample_stations = data.get("sample_stations") or _default_samples(stations, data["category"])

    return LineDefinition(
        id=data["id"],
        name=data["name"],
        mode=data["mode"],
        category=data["category"],
        operators=data["operators"],
        stations=stations,
        sample_stations=sample_stations,
        match_keywords=data.get("match_keywords", []),
        excluded_keywords=data.get("excluded_keywords", []),
        severity_overrides=data.get("severity_overrides", {}),
        exclusive_segments=data.get("exclusive_segments", []),
        destination_crs_filter=data.get("destination_crs_filter", []),
        headcode_prefixes=data.get("headcode_prefixes", []),
    )


def load_all_lines(directory: Path) -> dict[str, LineDefinition]:
    lines = {}
    for path in sorted(directory.glob("*.toml")):
        line = load_line(path)
        if line.id in lines:
            raise ValueError(f"Duplicate line id: {line.id}")
        lines[line.id] = line
    return lines


def _default_samples(stations: list[Station], category: str) -> list[str]:
    """Pick sensible sampling stations if none specified."""
    if not stations:
        return []
    if len(stations) <= 3:
        return [s.crs for s in stations]

    # Always include endpoints + roughly one sample per "leg"
    if category == "main-line":
        # Origin, ~1/3, ~2/3, destination
        n = len(stations)
        idxs = [0, n // 3, (2 * n) // 3, n - 1]
    elif category == "commuter":
        idxs = [0, len(stations) // 2, len(stations) - 1]
    else:
        idxs = [0, len(stations) // 2, len(stations) - 1]

    seen = set()
    out = []
    for i in idxs:
        crs = stations[i].crs
        if crs not in seen:
            out.append(crs)
            seen.add(crs)
    return out
