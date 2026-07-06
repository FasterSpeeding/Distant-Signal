# Design: LDBWS Sampler Poller

## Goal

Add a fourth poller service, `poller-ldbws`, that samples live departure-board
data (LDBWS — Live Departure Board Web Service, the Darwin real-time feed)
for every station any line's inference logic depends on, and stores it in
the already-existing `station_samples` table. This is the missing input
DESIGN.md's aggregator inference layer (§6.2) needs; without it, that layer
has no data to run on.

This spec is the first of two backend sub-projects that together let a
future frontend consume real line-status data (the second,
`03-aggregator-read-api.md`, covers the matcher/segments/aggregation port
and the read endpoints — brainstormed separately, after this lands).

## Why this, why now

The previous plan (`plans/01-poller-microservices.md`) built three pollers
(Knowledgebase Incidents, Stations reference data, TOCs) plus the ingestion
API, but did not touch LDBWS sampling — it wasn't in that plan's scope.
DESIGN.md's aggregation logic (§6, `aggregate()`) is a two-layer design:
incidents (layer 1, now available) and LDBWS-sample-based inference (layer
2, the fallback for lines with no active incident). Building the aggregator
without this poller would mean every line with no incident silently reports
Good Service with no real signal behind it — which is an acceptable *v1*
simplification (DESIGN.md itself frames inference as a fallback, not
required for a functioning aggregator), but the user has asked for full
aggregation from day one, so this poller is a prerequisite.

## Current relevant state (verified by reading the code, 2026-07-06)

- `common::StationSample` and `common::StationDeparture` already exist
  (`crates/common/src/lib.rs`), unused by anything today.
- The `station_samples` table already exists
  (`crates/api/migrations/20260510023522_initial.sql`): `crs CHAR(3) PRIMARY
  KEY, polled_at TIMESTAMPTZ NOT NULL, departures JSONB NOT NULL DEFAULT
  '[]'`. No new migration needed for storage.
- `common::LineDefinition`/`Station` (loaded from `lines/*.toml`) already
  has a `sample_stations: Vec<String>` field per line (CRS codes to poll)
  — this is the union DESIGN.md's Stage 1 guidance says to deduplicate.
- No `/private/station-samples` ingestion endpoint exists yet; no endpoint
  exposing the deduplicated station list exists yet.
- The other three pollers (`poller-incidents`, `poller-stations`,
  `poller-tocs`) are the structural template: clap+env config, a
  `reqwest::Client` built with a timeout from construction, a
  `tokio::time::interval` poll loop that logs and continues past errors
  rather than crashing, a non-root multi-stage Dockerfile, and — since the
  final whole-branch review of that plan — a shared `common::ingest` module
  holding the internal-auth header constant and a generic `post_batch`
  helper, which this poller should reuse rather than redefine.

## Decisions (from brainstorming)

1. **LDBWS API choice (SOAP vs. the newer REST option): deferred to a
   documentation-discovery research pass during planning**, not decided
   here — same approach used for the Incidents/Stations/TOCs RDM products
   in the previous plan (a subagent researched the actual RDM product
   listing and technical spec before any field name was committed to code).
   Whichever is chosen, the poller must not invent field names or an
   endpoint URL not traceable to a real, cited source — the previous plan's
   "no invented API details" discipline carries over to this one.
2. **Station-list source: a new API endpoint, `GET
   /private/sample-stations`**, not the poller parsing `lines/*.toml`
   itself. `api` already owns line-catalogue loading (via
   `ServiceArguments`'s `--lines-dir`); this endpoint computes the
   deduplicated union of every loaded line's `sample_stations` and returns
   it as a CRS list. Gated behind the same internal-token auth as the other
   `/private/*` routes. `poller-ldbws` calls this once at startup (and
   periodically refreshes — exact cadence a planning-time detail, since the
   line catalogue changes rarely) rather than needing its own copy of the
   line-loading/TOML-parsing logic.
3. **New ingestion endpoint: `POST /private/station-samples`**, accepting
   `Vec<common::StationSample>`, upserting into `station_samples` (`ON
   CONFLICT (crs) DO UPDATE`) — no history table, matching the existing
   "most recent sample per station" design and Global Constraint 6's
   precedent from the previous plan (reference/sample data is
   wholesale-replaced, not versioned).
4. **Poll cadence**: default 60s (top of DESIGN.md's "30-60s" range),
   configurable via `POLL_INTERVAL_SECS` env var, matching the other
   pollers' configuration pattern.

## Architecture

```
┌──────────────┐  GET /private/sample-stations   ┌─────┐
│ poller-ldbws │ ───────────────────────────────► │ api │
│              │ ◄─────────────────────────────── │     │
│              │      [CRS, CRS, CRS, ...]         └──┬──┘
│              │                                       │
│              │  N × LDBWS call (one per station)      │
│              │ ───────────────► (RDM LDBWS API)       │
│              │                                       │
│              │  POST /private/station-samples         │
│              │ ───────────────────────────────────►   │
└──────────────┘   Vec<StationSample>              station_samples table
```

`poller-ldbws` is a new crate in the existing Cargo workspace, alongside
the other three pollers, sharing `crates/common` for types and the
`common::ingest` module for the auth-header constant and POST helper.

## Data flow per poll cycle

1. Fetch (or use cached) deduplicated station list from `GET
   /private/sample-stations`.
2. For each CRS in that list, call the LDBWS endpoint for that station
   (exact operation name/shape pending the research pass — conceptually
   "get departures for this station, with delay/cancellation detail").
3. Map each response into a `StationSample { crs, polled_at: now, departures:
   Vec<StationDeparture> }`, where each `StationDeparture` carries
   `service_id`, `operator`, `destination_crs`, `scheduled`, `estimated`,
   `is_cancelled`, `delay_minutes`, optional `cancel_reason`/`delay_reason`,
   optional `headcode` — this shape is already fixed in `common::lib.rs`
   from the previous plan.
4. Batch all successfully-fetched samples from this cycle and POST once to
   `/private/station-samples` via `common::ingest::post_batch`.

## Error handling

- A single station's LDBWS call failing (timeout, 500, malformed response)
  must not abort the whole cycle — log it and continue with the remaining
  stations, POSTing whatever samples succeeded. This is routine, not
  exceptional: DESIGN.md's own inference logic already requires a minimum
  sample size before drawing conclusions, so partial data from a cycle is
  expected and tolerable.
- Failure to reach `/private/sample-stations` at all (e.g. `api` is down)
  should log and skip the entire cycle, matching the other pollers'
  "keep the loop alive" contract.
- Same `reqwest::Client` timeout discipline as the other three pollers
  (30s, applied from construction — not added after the fact this time).

## Testing

- Unit tests mapping a hand-written sample LDBWS response (once the
  research pass confirms the real schema) into `StationSample`/
  `StationDeparture`, following the same pattern as the other three
  pollers' schema tests.
- A test confirming a single failed per-station fetch doesn't prevent the
  successfully-fetched stations' samples from being POSTed.
- Docker build + non-root user verification, matching the established
  pattern.

## Explicitly out of scope for this spec

- The aggregator itself (matcher, segment registry, severity
  classification, `aggregate()`) — spec `03-aggregator-read-api.md`.
- The read endpoints (`/Line/...`, `/StopPoint/...`) and history endpoint —
  same follow-up spec.
- Any change to the existing three pollers or the `incidents`/`stations`/
  `tocs` ingestion paths.
- Docker Compose / Kubernetes deployment changes beyond adding this one
  service to the existing `docker-compose.yml` (mirrors the other three).

## Open questions for the planning phase (not blocking this design)

- Exact LDBWS product/endpoint/auth mechanism (SOAP vs REST, base URL,
  auth header) — resolve via documentation discovery, same rigor as the
  previous plan's RDM research.
- Exact refresh cadence for the station list (once at startup vs.
  periodic re-fetch) — low-stakes detail, decide during planning.
- Rate/pace of the N per-station calls within one poll cycle (sequential vs.
  bounded-concurrency) — a real API-etiquette question once the actual
  RDM product's rate limits (if any) are known from the research pass.
