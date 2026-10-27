#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI whose output (the figures) is stdout
"""Measure /Trips/plan with and without transit networks (DS-164, phase M4).

Usage (from the repo root)::

  uv run scripts/measure-transit-trip-planning.py measure --api URL
      --date D --label off --out off.json [--transit] [--pid PID]
      [--queries FILE] [--repeat N]
  uv run scripts/measure-transit-trip-planning.py compare off.json on.json

The spec's budget (docs/superpowers/specs/2026-10-24-transit-trip-planning-
design.md section 10): the api's resident memory at most 25 MB more with the
networks on, and each search at most 10% slower. Run it against a release
build of the api on a COPY of production (never production itself), twice:
once with API_TRIP_PLAN_TRANSIT_NETWORKS empty ("off") and once with it set
("on", with --transit), each from a fresh start. docs/transit-networks.md,
"Trip planning", has the steps.

`measure` warms the graph cache for the date and the next one (the two
dates GraphCache keeps), then times every query REPEAT times for
`results=fastest` and `results=options`, all with `live=false` so the
figures do not depend on the hour; with --transit also with `transit=true`.
It records each query's latencies, the SHA-256 of each response, and with
--pid the api's VmRSS and VmHWM (from /proc, so run it on the same host).

`compare` prints the deltas and exits 1 when a budget is broken, or when a
response without `transit=true` differs between the two runs (the golden
rule: with the switch on, a request that does not opt in is unchanged).
Only figures go into a PR or a card; the JSON files hold hashes, not
responses.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import statistics
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import TypedDict

MEMORY_BUDGET_BYTES = 25 * 1024 * 1024
LATENCY_BUDGET = 1.10
MB = 1024 * 1024

# Origin and destination pairs through and around Manchester, plus a few
# elsewhere: every search sweeps the whole day either way.
DEFAULT_QUERIES: list[tuple[str, str, str]] = [
    ("SPT", "LDS", "08:00"),
    ("MAN", "PRE", "08:30"),
    ("MIA", "BPN", "07:45"),
    ("ALT", "HUD", "09:10"),
    ("WGN", "SHF", "17:20"),
    ("EUS", "MAN", "07:00"),
    ("KGX", "EDB", "09:00"),
    ("BHM", "BRI", "12:00"),
    ("LIV", "NCL", "16:30"),
    ("MCV", "YRK", "18:05"),
]


class Sample(TypedDict):
    """One query in one mode: its latencies and last response."""

    query: str
    mode: str
    seconds: list[float]
    status: int
    sha256: str


class Memory(TypedDict, total=False):
    """The api's VmRSS and VmHWM after the warm-up and after the run."""

    warm_rss_bytes: int
    warm_hwm_bytes: int
    end_rss_bytes: int
    end_hwm_bytes: int


class Result(TypedDict):
    """One `measure` run."""

    label: str
    date: str
    transit: bool
    samples: list[Sample]
    memory: Memory


@dataclass(frozen=True)
class Response:
    """One HTTP response and how long it took."""

    status: int
    body: bytes
    seconds: float


@dataclass(frozen=True)
class Query:
    """One plan request."""

    origin: str
    destination: str
    at: str


def fetch(url: str, timeout: float) -> Response:
    """GET `url` (http or https only)."""
    if urllib.parse.urlsplit(url).scheme not in {"http", "https"}:
        msg = f"not an http URL: {url}"
        raise ValueError(msg)
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(url, timeout=timeout) as reply:  # noqa: S310  # scheme checked above
            body = reply.read()
            status = int(reply.status)
    except urllib.error.HTTPError as err:
        body = err.read()
        status = err.code
    return Response(status, body, time.perf_counter() - started)


def plan_url(api: str, query: Query, date: str, extra: dict[str, str]) -> str:
    """Build the /Trips/plan URL for `query` on `date`, timetable only."""
    params = {
        "origin": query.origin,
        "destination": query.destination,
        "date": date,
        "departAfter": query.at,
        "live": "false",
        **extra,
    }
    return f"{api.rstrip('/')}/Trips/plan?{urllib.parse.urlencode(params)}"


def proc_memory(pid: int) -> tuple[int, int]:
    """`pid`'s VmRSS and VmHWM, in bytes."""
    values: dict[str, int] = {}
    status = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    for line in status.splitlines():
        name, _, value = line.partition(":")
        if name in {"VmRSS", "VmHWM"}:
            values[name] = int(value.split()[0]) * 1024
    return values.get("VmRSS", 0), values.get("VmHWM", 0)


def read_queries(path: Path | None) -> list[Query]:
    """Read `path`'s `origin,destination,HH:MM` lines, or use the set."""
    if path is None:
        return [Query(*row) for row in DEFAULT_QUERIES]
    queries = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            origin, destination, at = (part.strip() for part in line.split(","))
            queries.append(Query(origin, destination, at))
    return queries


def measure(args: argparse.Namespace) -> int:
    """Time the queries against one running api and write the result."""
    date = dt.date.fromisoformat(args.date)
    queries = read_queries(args.queries)
    for day in (date, date + dt.timedelta(days=1)):
        warm = fetch(plan_url(args.api, queries[0], day.isoformat(), {}), args.timeout)
        print(f"warm-up {day}: {warm.status} in {warm.seconds:.2f} s (graph build)")
    memory: Memory = {}
    if args.pid is not None:
        memory["warm_rss_bytes"], memory["warm_hwm_bytes"] = proc_memory(args.pid)
    modes: list[tuple[str, dict[str, str]]] = [
        ("fastest", {}),
        ("options", {"results": "options"}),
    ]
    if args.transit:
        modes += [
            ("fastest+transit", {"transit": "true"}),
            ("options+transit", {"results": "options", "transit": "true"}),
        ]
    samples: list[Sample] = []
    for query in queries:
        for mode, extra in modes:
            url = plan_url(args.api, query, date.isoformat(), extra)
            responses = [fetch(url, args.timeout) for _ in range(args.repeat)]
            seconds = [response.seconds for response in responses]
            name = f"{query.origin}-{query.destination}@{query.at}"
            samples.append(
                {
                    "query": name,
                    "mode": mode,
                    "seconds": seconds,
                    "status": responses[-1].status,
                    "sha256": hashlib.sha256(responses[-1].body).hexdigest(),
                }
            )
            median_ms = statistics.median(seconds) * 1000
            print(f"{name} {mode}: {responses[-1].status}, median {median_ms:.0f} ms")
    if args.pid is not None:
        memory["end_rss_bytes"], memory["end_hwm_bytes"] = proc_memory(args.pid)
    result: Result = {
        "label": args.label,
        "date": date.isoformat(),
        "transit": args.transit,
        "samples": samples,
        "memory": memory,
    }
    args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out}")
    return 0


def load(path: Path) -> Result:
    """Load a `measure` result file."""
    data: Result = json.loads(path.read_text(encoding="utf-8"))
    return data


def medians(result: Result, mode: str) -> list[float]:
    """Each query's median latency in `mode`."""
    return [
        statistics.median(s["seconds"]) for s in result["samples"] if s["mode"] == mode
    ]


def golden_failures(off: Result, on: Result) -> list[str]:
    """Responses without `transit=true` that differ between the runs."""
    on_hashes = {(s["query"], s["mode"]): s["sha256"] for s in on["samples"]}
    return [
        f"{s['query']} {s['mode']}: the response differs without transit=true"
        for s in off["samples"]
        if on_hashes.get((s["query"], s["mode"]), s["sha256"]) != s["sha256"]
    ]


def latency_failures(off: Result, on: Result) -> list[str]:
    """Modes slower than the budget; prints every mode's figures."""
    failures = []
    for mode in ("fastest", "options"):
        base = medians(off, mode)
        if not base:
            continue
        before = statistics.median(base)
        for label in (mode, f"{mode}+transit"):
            figures = medians(on, label)
            if not figures:
                continue
            after = statistics.median(figures)
            change = (after / before - 1) * 100
            print(
                f"{label}: {before * 1000:.0f} ms off, {after * 1000:.0f} ms on "
                f"({change:+.1f}%)"
            )
            if after / before > LATENCY_BUDGET:
                failures.append(f"{label} is {change:.1f}% slower (budget 10%)")
    return failures


def memory_failures(off: Result, on: Result) -> list[str]:
    """Report RSS growth past the budget, printing the figures."""
    off_rss = off["memory"].get("end_rss_bytes")
    on_rss = on["memory"].get("end_rss_bytes")
    if off_rss is None or on_rss is None:
        return []
    delta = on_rss - off_rss
    print(
        f"RSS: {off_rss / MB:.1f} MB off, {on_rss / MB:.1f} MB on "
        f"({delta / MB:+.1f} MB, budget 25 MB)"
    )
    if delta > MEMORY_BUDGET_BYTES:
        return [f"RSS grew {delta / MB:.1f} MB (budget 25 MB)"]
    return []


def compare(args: argparse.Namespace) -> int:
    """Check two runs against the budget and the golden rule."""
    off = load(args.off)
    on = load(args.on)
    failures = (
        golden_failures(off, on) + latency_failures(off, on) + memory_failures(off, on)
    )
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    return 1 if failures else 0


def main() -> int:
    """Run `measure` or `compare`."""
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("measure", help="time the queries against one api")
    run.add_argument("--api", required=True, help="e.g. http://127.0.0.1:8080")
    run.add_argument("--date", required=True, help="a weekday, YYYY-MM-DD")
    run.add_argument("--label", required=True, help="off or on")
    run.add_argument("--out", required=True, type=Path)
    run.add_argument("--transit", action="store_true", help="also transit=true")
    run.add_argument("--pid", type=int, help="the api's pid, for VmRSS and VmHWM")
    run.add_argument("--queries", type=Path, help="origin,destination,HH:MM lines")
    run.add_argument("--repeat", type=int, default=7)
    run.add_argument("--timeout", type=float, default=120.0)
    check = commands.add_parser("compare", help="check two runs against the budget")
    check.add_argument("off", type=Path)
    check.add_argument("on", type=Path)
    args = parser.parse_args()
    if args.command == "measure":
        return measure(args)
    return compare(args)


if __name__ == "__main__":
    sys.exit(main())
