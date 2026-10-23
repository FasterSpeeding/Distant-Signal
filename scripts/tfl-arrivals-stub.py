#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI: it reports where it listens on stdout
"""Answer TfL's StopPoint arrivals for the frontend's e2e and visual tests.

  scripts/tfl-arrivals-stub.py [--port 8099]

The api fetches live arrivals for the /tfl/stops/{id} page from TfL's
Unified API (crates/api/src/data/tfl_arrivals.rs). CI's frontend-e2e job
points TFL_BASE_URL at this stub instead, so no test reaches TfL and every
run sees the same board:

- `GET /StopPoint/940GZZLUKSX/Arrivals` (King's Cross St. Pancras, in
  frontend/e2e/fixtures/seed.sql) answers four predictions on two
  platforms, shaped like TfL's own. Their expected times are offsets from
  the moment of the request, each 45 s past a whole minute, so the board
  reads the same ("Due", "2 min", "6 min", "10 min") through the api's
  30 s cache;
- any other stop answers 503, as TfL does when it is struggling, which
  the page shows as "Couldn't load live arrivals".

Stdlib only; stops on Ctrl-C or SIGTERM.
"""

import argparse
import json
import signal
import sys
from collections.abc import Sequence
from datetime import UTC, datetime, timedelta
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import FrameType
from typing import NamedTuple, override

STOP = "940GZZLUKSX"
PATH = f"/StopPoint/{STOP}/Arrivals"


class Prediction(NamedTuple):
    """One train on the board."""

    pid: str
    platform: str
    direction: str
    destination: str
    towards: str
    where: str
    secs: int


SOUTH = "Southbound - Platform 5"
NORTH = "Northbound - Platform 6"
BRIXTON = "Brixton Underground Station"
PREDICTIONS = (
    Prediction(
        "-1001", SOUTH, "inbound", BRIXTON, "Brixton", "At Highbury & Islington", 45
    ),
    Prediction(
        "-1002",
        SOUTH,
        "inbound",
        BRIXTON,
        "Brixton",
        "Between Finsbury Park and Highbury & Islington",
        405,
    ),
    Prediction(
        "-1003",
        NORTH,
        "outbound",
        "Walthamstow Central Underground Station",
        "Walthamstow Central",
        "At Euston",
        165,
    ),
    # No destination: the page falls back to `towards`.
    Prediction("-1004", NORTH, "outbound", "", "Seven Sisters", "", 645),
)
PREDICTION_TYPE = (
    "Tfl.Api.Presentation.Entities.Prediction, Tfl.Api.Presentation.Entities"
)


def iso(moment: datetime) -> str:
    """Return `moment` as TfL writes times: UTC with a trailing Z."""
    return moment.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def arrivals(now: datetime) -> list[dict[str, object]]:
    """Return the King's Cross board as TfL would answer it at `now`."""
    rows: list[dict[str, object]] = []
    for row in PREDICTIONS:
        expected = iso(now + timedelta(seconds=row.secs))
        rows.append(
            {
                "$type": PREDICTION_TYPE,
                "id": row.pid,
                "operationType": 1,
                "vehicleId": row.pid.lstrip("-"),
                "naptanId": STOP,
                "stationName": "King's Cross St. Pancras Underground Station",
                "lineId": "victoria",
                "lineName": "Victoria",
                "platformName": row.platform,
                "direction": row.direction,
                "bearing": "",
                "destinationNaptanId": "",
                "destinationName": row.destination,
                "timestamp": iso(now),
                "timeToStation": row.secs,
                "currentLocation": row.where,
                "towards": row.towards,
                "expectedArrival": expected,
                "timeToLive": expected,
                "modeName": "tube",
            },
        )
    return rows


class Handler(BaseHTTPRequestHandler):
    """The stub's one route, and 503 for everything else."""

    def do_GET(self) -> None:
        """Answer the fixture board, or 503."""
        if self.path.split("?", 1)[0] == PATH:
            body = json.dumps(arrivals(datetime.now(UTC))).encode()
            self.send_response(200)
        else:
            body = b'{"message": "The service is unavailable."}'
            self.send_response(503)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    @override
    def log_message(self, format: str, *args: object) -> None:
        """Log each request to stderr, one line."""
        sys.stderr.write(f"tfl-arrivals-stub: {format % args}\n")


def main(argv: Sequence[str] | None = None) -> int:
    """Serve until interrupted."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--port", type=int, default=8099)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args(argv)
    server = ThreadingHTTPServer((args.host, args.port), Handler)

    def stop(_signum: int, _frame: FrameType | None) -> None:
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop)
    print(f"tfl-arrivals-stub listening on http://{args.host}:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
