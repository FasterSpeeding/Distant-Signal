# Design: Aggregator + Read API

## Goal

Turn the raw data already flowing into Postgres (Knowledgebase incidents,
station reference data, TOC reference data, LDBWS departure-board samples —
all built and working per the two prior plans) into the TfL-shaped line
status API DESIGN.md describes, so a future frontend has something real to
consume. This is the second of two backend sub-projects identified when
originally scoping the frontend request; the first (the LDBWS sampler
poller) is done and merged.

Concretely: a new `aggregator` service that periodically computes each
line's status from incidents + samples, and four new read endpoints on the
existing `api` crate serving that computed status in TfL's response shape.

## Current relevant state (verified by reading the code, 2026-07-06)

- **Storage is already in place and unused.** `crates/api/migrations/20260510023522_initial.sql`
  already defines `line_status` (`line_id TEXT PRIMARY KEY, name, mode_name,
  operators TEXT[], statuses JSONB, computed_at`) and `line_status_history`
  (`id BIGINT IDENTITY PRIMARY KEY, line_id, statuses JSONB, computed_at`,
  indexed on `(line_id, computed_at DESC)`), with the original migration's
  own comment already anticipating a pruning job ("Pruned by a periodic job
  (e.g. keep 7 days)") that was never built.
- **The domain types are already ported to Rust**, in `crates/common/src/lib.rs`:
  `Severity` (0-14 + 20/21 NR extensions), `DataQuality`, `ValidityPeriod`,
  `AffectedRoute`, `Disruption`, `LineStatus`, `LineStatusReport` (with
  `worst_severity()`), `LineDefinition`/`Station` (TOML-loaded, with
  `sample_stations`, `match_keywords`, `excluded_keywords`,
  `severity_overrides: HashMap<String, f64>` — just fixed in the prior
  plan, `exclusive_segments`, `destination_crs_filter`, `headcode_prefixes`),
  `IncidentMessage`, `StationSample`/`StationDeparture`, `StationReference`,
  `TocReference`. None of the matching/aggregation *logic* exists in Rust
  yet — only the data shapes.
- **`api` already loads the line catalogue at startup** (`--lines-dir`/
  `LINES_DIR`, defaulting to `/app/lines` baked into its Docker image, via
  the `LineCatalogue` newtype in `crates/api/src/data/config.rs`) and
  already serves it back partially via `GET /private/sample-stations`
  (internal-only, computes a deduplicated CRS list). The read endpoints
  this spec adds are the first *public* consumers of that same loaded
  catalogue.
- **The reference implementation is a complete, tested Python prototype**
  (`src/types.py`, `segments.py`, `matcher.py`, `aggregator.py`, `render.py`,
  `config.py` — 833 lines total) with `tests/test_matcher.py` (157 lines)
  covering exclusive-segment, shared-trunk, keyword-only, and both
  operator-only cases (with and without a precise match suppressing it).
  This is the algorithm to port faithfully — see "Adapting the reference
  implementation" below for the two places reality has diverged from what
  the Python code assumes.
- **`Defaults`** (the threshold struct: `delay_threshold_minutes`,
  `minor_delays_pct`, `severe_delays_pct`, `reduced_service_pct`,
  `part_suspended_pct`, `knowledgebase_severity_floor`, `min_sample_size`)
  is already a faithful Rust port of Python's `config.py` `DEFAULTS` dict,
  but currently lives private to `crates/api/src/data/config.rs`, wired to
  an unused `--defaults-file` CLI arg. It needs to move to `common` so the
  new `aggregator` crate can share it.
- Four pollers already populate `incidents`, `stations`, `tocs`, and
  `station_samples`. This spec is a pure consumer of that data — it adds
  no new poller and touches no ingestion endpoint.

## Decisions (from brainstorming)

1. **`aggregator` is a separate binary crate** (new workspace member,
   `crates/aggregator`), matching the pollers' "one concern per service"
   pattern rather than a background task inside `api`. It runs on its own
   schedule, independently loads the line catalogue, and only touches
   Postgres (no HTTP surface of its own).
2. **Default poll/recompute cadence: 60 seconds**, configurable via env
   (matching every poller's `POLL_INTERVAL_SECS` pattern) — the
   conservative end of DESIGN.md's documented 30-60s target.
3. **`line_status_history` pruning is in scope**, not deferred. The
   aggregator deletes history rows older than a configurable retention
   window (default 7 days, per the original migration's own comment) once
   per cycle, after writing the current cycle's data.
4. **The four read endpoints go on `public_router()`**, unauthenticated —
   this mirrors TfL's own public API, which is the entire reason the
   response shape matches it in the first place. This is a deliberate
   contrast with the `/private/*` ingestion endpoints, not an oversight.
5. **`/StopPoint/{crs}/Disruption` looks up station-to-line membership
   in-memory from the already-loaded `LineDefinition`s**, not a
   denormalized column on `line_status`. `lines/*.toml` stays the single
   source of truth for which stations belong to which lines.
6. **Future direction, explicitly flagged, not built now:** the line
   catalogue currently lives in static TOML files loaded at each service's
   startup. A later migration should move it into Postgres (e.g. `lines`/
   `line_stations` tables), admin-editable and hot-reloadable without a
   redeploy, while preserving `LineDefinition`'s current shape as the
   stable contract every consumer (`api`, `aggregator`, and any future
   poller needing `sample_stations`) already depends on. Nothing in this
   spec should make that migration harder than it already would be —
   in particular, keep line-catalogue loading behind a narrow interface
   (a function returning `Vec<LineDefinition>`) in both `api` and
   `aggregator`, so swapping the TOML loader for a Postgres query later is
   a localized change, not a rewrite.

## Adapting the reference implementation

The Python prototype's `aggregator.py` assumes an `IncidentMessage` shape
that doesn't match what `common::IncidentMessage` actually is (the real
one was built against confirmed RDM schema facts in an earlier plan; the
Python prototype predates that research). Two concrete divergences, both
requiring a deliberate adaptation rather than a literal port:

1. **No `severity_hint`.** Python's `_severity_from_incident` checks
   `incident.severity_hint == "major"` / `"minor"` as one of its
   classification branches. The real `IncidentMessage` has `priority: i32`
   instead — a raw `IncidentPriority` integer whose meaning is a
   documented, still-unresolved RDM gap (no enum, no confirmed mapping).
   The Rust port **drops this branch entirely** and relies solely on the
   keyword-text classification already present in the same function
   (`"suspended"`, `"rail replacement"`, `"lines blocked"`, `"severe
   delays"`, `"diverted"`, `"minor delays"`, otherwise `MinorDelays`) —
   consistent with this project's established discipline of never
   inventing meaning for an unconfirmed RDM field.
2. **Multiple validity periods, one output slot.** Python's
   `IncidentMessage.valid_from`/`valid_to` is a single optional pair,
   copied directly into the output `LineStatus.validity: ValidityPeriod`
   (also singular). The real `IncidentMessage.validity: Vec<ValidityPeriod>`
   can hold more than one period (the real RDM schema allows repeated
   `ValidityPeriod` elements — confirmed in an earlier plan's research).
   Since the output type is still singular, the Rust port picks **the
   currently-active period** — the first one where `from_date <= now &&
   (to_date.is_none() || to_date > now)` — falling back to the first
   period in the list if none are currently active (e.g. all in the
   future, or the incident is stale). This is a new rule with no Python
   equivalent to port, so it gets its own new test, not a translated one.

Everything else in `matcher.py`/`segments.py`/`aggregator.py`/`render.py`
ports faithfully: `SegmentRegistry`, the `MatchScope` enum and its five
variants, `lines_affected_by`/`_match_one` (including the "drop
operator-only matches when any precise match exists" rule — caught by a
named regression test in the Python suite, must not be lost), `aggregate`'s
two-layer structure (incidents first, inference fallback second),
`_infer_from_samples`/`_belongs_to_line`/`_classify`, and `render.py`'s
exact TfL JSON field names (`$type`, `statusSeverity`,
`statusSeverityDescription`, `validityPeriods`, etc.).

## Architecture

```
┌─────────────┐   every 60s    ┌──────────────────────────────┐
│ aggregator   │───────────────▶│ read incidents +            │
│ (new binary) │                │ station_samples from Postgres│
└──────┬───────┘                └──────────────┬───────────────┘
       │ loads lines/*.toml                     │
       │ at startup                             ▼
       │                          ┌───────────────────────────┐
       │                          │ aggregate() — ported       │
       │                          │ matcher + segments +       │
       │                          │ severity/inference logic   │
       │                          └──────────────┬─────────────┘
       │                                          ▼
       │                          upsert line_status (always),
       │                          insert line_status_history
       │                          (only if changed vs last cycle),
       │                          prune history > retention window
       │                                          │
       ▼                                          ▼
                              Postgres: line_status, line_status_history
                                          ▲
                                          │ read-only queries
                              ┌───────────┴────────────┐
                              │ api (existing crate)    │
                              │ public_router() adds:   │
                              │  GET /Line/Mode/{m}/Status
                              │  GET /Line/{ids}/Status  │
                              │  GET /StopPoint/{crs}/Disruption
                              │  GET /Line/{id}/Status/{from}/to/{to}
                              └─────────────────────────┘
```

`aggregator` is read-only with respect to `incidents`/`station_samples`/
`stations`/`tocs` (all written by the four existing pollers) and
write-only with respect to `line_status`/`line_status_history`. `api`'s
new endpoints are read-only with respect to both status tables. No
component writes and reads the same table in this design, keeping the data
flow one-directional and easy to reason about.

## Data flow per aggregator cycle

1. Load (once, at startup, not per cycle) `lines/*.toml` → build the
   `SegmentRegistry`.
2. Each cycle: query all rows from `incidents` and `station_samples`.
3. Run `aggregate(lines, incidents, samples, registry)` → `HashMap<line_id,
   LineStatusReport>`.
4. For each line: compare the new `Vec<LineStatus>` (serialized) against
   the currently-stored `line_status` row for that `line_id`. Upsert
   `line_status` unconditionally (keeps `computed_at` fresh even when
   nothing changed, so a client can tell staleness from freshness). Insert
   into `line_status_history` only when the serialized statuses differ
   from what's currently stored — mirroring `incident_changed`'s existing
   "only on real change" convention, not writing a history row every cycle
   regardless of content.
5. Delete `line_status_history` rows where `computed_at < now() -
   retention_window` (default 7 days, configurable).

## Read endpoints

All four added to `crates/api/src/routes/`, mounted under `public_router()`
(no internal-token auth), rendering `LineStatusReport`/`LineStatus` in
`render.py`'s exact TfL JSON shape (`$type`, `id`, `name`, `modeName`,
`operators`, `lineStatuses[]`; each status has `statusSeverity` (int),
`statusSeverityDescription`, `reason`, `dataQuality`, `validityPeriods[]`
(`fromDate`/`toDate`/`isNow`), and `disruption` only when `?detail=true`
was passed and the status actually has one).

- `GET /Line/Mode/{mode}/Status` — validates `mode == "national-rail"`
  (400 otherwise; DESIGN.md defines no other mode), returns every line's
  current `line_status` row.
- `GET /Line/{ids}/Status?detail=true` — `{ids}` splits on commas (TfL's
  own convention for batch requests); queries `line_status WHERE line_id =
  ANY($1)`; 404s only if none of the requested IDs match any row.
- `GET /StopPoint/{crs}/Disruption` — scans the loaded `LineDefinition`s
  for ones containing `crs`, queries `line_status` for those line IDs,
  filters out any status whose `severity == GoodService`, returns the
  (possibly empty) remainder.
- `GET /Line/{id}/Status/{from}/to/{to}` — parses `{from}`/`{to}` as
  RFC 3339 timestamps, queries `line_status_history WHERE line_id = $1 AND
  computed_at BETWEEN $2 AND $3 ORDER BY computed_at`, renders each row.

## Testing

- **Matcher/segments/aggregation**: one Rust test per existing Python test
  in `tests/test_matcher.py` (exclusive-segment match, shared-trunk
  propagation, keyword-only, operator-only with no precise match,
  operator-only suppressed by a precise match, excluded-keyword veto),
  plus two new tests for the two adaptations above (priority-less
  severity classification producing the same result the keyword text
  alone would; multi-period validity selection picking the
  currently-active period, and falling back to the first when none are
  active).
- **Threshold merging**: unit tests for `thresholds_for` covering no
  overrides, partial overrides, and every field being overridden.
- **Read endpoints**: integration-style tests that seed `line_status`/
  `line_status_history` directly (bypassing the aggregator) and assert
  the rendered JSON matches `render.py`'s shape exactly, including the
  `detail=true`/`false` disruption-inclusion behavior and the
  multi-ID/empty-result/404 cases for `/Line/{ids}/Status`.
- **Aggregator's change-detection**: a test confirming an unchanged cycle
  upserts `line_status` (fresh `computed_at`) but does *not* insert a new
  `line_status_history` row, and a changed cycle does both.

## Explicitly out of scope for this spec

- Moving the line catalogue into Postgres (the flagged future direction
  above — noted, not built).
- Any new poller or ingestion endpoint (all four already exist).
- Caching of read-endpoint responses (unnecessary at this data scale).
- LDBWS-inference refinements beyond what `_infer_from_samples`/
  `_belongs_to_line`/`_classify` already do (destination/headcode
  filtering, minimum sample size, etc. — all ported as-is).
- The original frontend request (Next.js + TapTap) — the third piece in
  the original decomposition, picked up once this spec's implementation
  lands.

## Open questions for the planning phase (not blocking this design)

- Exact Rust module layout inside `crates/aggregator` (mirroring Python's
  `matcher.rs`/`segments.rs`/`aggregator.rs`/`render.rs`/`config.rs` split,
  or consolidating — a planning-time file-structure decision, not a design
  one).
- Whether `thresholds_for`'s f64→i64/i8 casting for
  `delay_threshold_minutes`/`min_sample_size`/`knowledgebase_severity_floor`
  needs saturating/truncating semantics specified precisely, or plain `as`
  casts suffice given these are always whole numbers in practice — a
  planning-time detail.
