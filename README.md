# nr-status

A TfL-style line status aggregator for UK National Rail, with first-class
support for operators with multiple parallel routes that share trunk track
(SWR, Southeastern, Northern, etc.).

## What it does

Takes Knowledgebase incidents and LDBWS departure-board samples as input,
and emits a TfL-Unified-API-compatible status report per line. Crucially,
it knows the difference between an incident on a *shared trunk* (which
should propagate to every line using that trunk) and an incident on an
*exclusive segment* (which should not).

## Layout

```
lines/                    TOML line definitions (the curatorial asset)
  SCHEMA.md               Field reference, including segments
  west-coast-main-line.toml
  thameslink-core.toml
  swr-south-west-main.toml
  swr-portsmouth-direct.toml
  swr-alton.toml
src/
  types.py                Domain types (Severity, LineDefinition, etc.)
  config.py               Default thresholds for status derivation
  loader.py               TOML -> LineDefinition
  segments.py             Cross-line segment registry
  matcher.py              Incident -> {line, scope, evidence}
  aggregator.py           The core decision logic
  render.py               -> TfL-shaped JSON
tests/
  test_matcher.py
demo.py                   End-to-end run with three scenarios
```

## Run the demo

```bash
PYTHONPATH=. python demo.py
PYTHONPATH=. python tests/test_matcher.py
```

The demo runs three scenarios:

1. **WCML trespass** between Watford Junction and Milton Keynes — exclusive
   to WCML, no propagation.
2. **SWR signal failure at Woking** — shared trunk, propagates to all three
   SWR lines (South West Main, Portsmouth Direct, Alton).
3. **Power supply problem at Alton** — exclusive segment, stays local to
   the Alton line. Sibling SWR lines stay Good Service.

## How segments work

Each station on a line belongs to a named `segment`. When the same segment
name appears across multiple line definitions, the system treats it as a
shared trunk — incidents there propagate to every line using that segment.

The matcher classifies every incident-to-line match by scope:

- `EXCLUSIVE_SEGMENT` — incident's stations all sit on segments unique to
  this line. Highest confidence.
- `SHARED_SEGMENT` — at least one of the touched segments is shared.
  Status propagates to all lines using that segment, with a "shared trunk"
  annotation in the reason text.
- `STATION_HIT` — line/station overlap but no segment metadata to classify.
- `KEYWORD_ONLY` — line is named in the incident text but no station hits.
  Capped at Severe Delays.
- `OPERATOR_ONLY` — only operator overlap. Capped at Minor Delays, and
  suppressed entirely if a more precise match exists for the same incident.

The last point matters: it's what stops an incident on the Alton branch
from also flagging South West Main and Portsmouth Direct just because all
three share the `SW` operator code.

## Adding a complex operator

For a TOC like SWR with multiple routes:

1. Create one line file per passenger route (`swr-south-west-main.toml`,
   `swr-portsmouth-direct.toml`, etc.).
2. Use the same segment name (e.g. `swr-trunk-waterloo`) on all the lines
   that share trunk track. Junction stations belong to the shared trunk;
   exclusive segments start at the next station.
3. Set `destination_crs_filter` (and/or `headcode_prefixes`) so LDBWS
   inference at shared stations counts only the line's own services.
4. Add `match_keywords` for any colloquial line names ("Portsmouth Direct",
   "Alton line").
5. Run the test suite. Add a scenario that exercises the new line's shared
   trunks and exclusive segments — both shapes of incident must produce
   the right behaviour.

## What's not included

This is the aggregation layer only. To run against live data you need:

1. A **Knowledgebase incidents poller** that fetches the NRE incidents XML,
   parses each incident into an `IncidentMessage`, and feeds them in.
2. An **LDBWS sampler** that calls `GetDepBoardWithDetails` for each
   `sample_stations` CRS in your line set and shapes the results into
   `StationSample` objects.
3. A **scheduler** to run the above every 30-60 seconds and persist results.
4. An **HTTP layer** to serve the rendered JSON. FastAPI works nicely.

## Severity scale

We use TfL's 0-14 scale verbatim where it applies, then add two NR-specific
values (Recovering = 20, Diverted = 21) outside the TfL range to avoid
clashes if TfL adds new codes.

## Design notes

- **Per-line thresholds matter.** A 5-minute delay on a 15-min-frequency
  commuter route is more disruptive than the same delay on an hourly
  long-distance route.
- **Knowledgebase prose is the gold.** When an active KB incident exists,
  prefer its description text as the `reason` over anything we infer.
- **Inference is a fallback, not a primary signal.** Only emit non-Good
  inferred statuses with reasonable sample sizes (`min_sample_size`).
- **Make data quality visible.** Clients should be able to tell whether a
  status came from a curated source or was inferred. We expose this via
  `dataQuality` on every status.
- **Junction stations belong to the shared trunk.** This is the single
  most important rule when authoring line definitions. The exclusive
  segment starts *after* the junction.
