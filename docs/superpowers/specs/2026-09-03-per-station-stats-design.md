# Design: Per-Station Delay/Cancellation Stats, Scoped by Operator

**Status: design proposal, not approved.** Written directly on top of
`docs/superpowers/specs/2026-09-03-per-station-stats-research.md` ("the
research doc"), which investigated whether/how to add station-level
delay/cancellation stats, concluded it is buildable today against
existing data with no new upstream dependency, and deliberately left
seven questions open (its "Open questions" section). This document
resolves six of those — the ones that are genuinely this pass's to
resolve. Its #7 ("is this worth building at all right now") is a
product-priority call the research doc explicitly declined to make and
this document does not re-litigate either; it is answered implicitly by
the fact that a plan (`docs/superpowers/plans/2026-09-03-per-station-stats-plan.md`)
follows this document. No implementation plan is embedded here — that
plan is the separate, later step.

## Goal

Turn the research doc's Recommendation — Option C (compute on demand at
read time from `station_samples`, reusing `latest_station_sample`), scoped
per **(station, operator)** rather than one unfiltered number per station
— into a concrete, buildable design: an exact wire shape, exact code
reuse/promotion, exact threshold source, exact skip-rate definition, and
exact frontend rendering. Scoped to the ~286 CRS codes that are already
someone's `sample_stations` entry (the research doc's "Polling scope"
finding); broadening `poller-ldbws`'s own reach is explicitly out of
scope (see below).

## Relationship to the two adjacent documents referenced in the research doc

- `docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md`
  ("09-01") — the direct structural precedent for `SampleAvailability`
  (`crates/common/src/lib.rs:731-747`, confirmed shipped). This document
  reuses that type's vocabulary and shape (`NoCoverage`/`BelowThreshold`/
  `Available`) for the per-operator case, with one documented deviation
  (Decision 7, below) on how `NoCoverage` behaves at the per-operator
  granularity.
- `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
  — did not exist when the research doc was written; it exists now
  (confirmed directly: it is present on `main` as of this pass). Read in
  full for this reconciliation. It is a **line-level** migration design
  (what happens to `LineStatus.sample_stats`/`sample_availability` once
  TRUST/Option B ships) and explicitly, by its own words, stays out of
  the station-level question: it deliberately stays scoped to the
  line-level `LineStatus` surfaces and does not attempt to also design
  the station-level transition, to avoid duplicating or preempting this
  parallel work. **No overlap, no conflict** — this document is the
  parallel work it names, and nothing here needs to change because that
  document landed. One item from it is worth carrying forward as a
  constraint here anyway: that document flags `formatSampleSummary`/
  `sampleUnavailableReason`'s eventual rename to a source-agnostic name
  as "a probable need, not committed to a final name here." Decision 9
  below generalizes those functions' *signature* without renaming them,
  so it does not preempt or conflict with that future rename either.

## Current relevant state (not re-derived — see the research doc for full citations)

Summarized only to the extent this document's Decisions depend on it
directly:

- `station_samples` (`crates/api/migrations/20260510023522_initial.sql:52-56`)
  is one row per CRS, wholesale-replaced per poll, already carrying
  per-station `StationDeparture`s (`crates/common/src/lib.rs:378-401`)
  with `operator`, `is_cancelled`, `delay_minutes`, `skipped_stations`.
- `latest_station_sample` (`crates/api/src/data/queries.rs:664-679`) is a
  proven, shipping, single-CRS on-demand read, already used by
  `crates/api/src/routes/train.rs:512-537`'s `blend_darwin_eta` for the
  same "read-time-only, best-effort" pattern this design reuses.
- The counting arithmetic lives inside `crates/aggregator/src/aggregation.rs`,
  crate-private: `relevant_departures` (`:794-810`), `stats_from_departures`
  (`:816-844`), `compute_sample_availability` (`:859-882`),
  `belongs_to_line` (`:983-1000`). None of it is reachable from `crates/api`
  today.
- `common::Defaults` (`crates/common/src/lib.rs:870-902`) and
  `thresholds_for` (`:916-928`) already live in the shared `common` crate;
  `crates/aggregator/src/main.rs:50` calls `Defaults::default()` directly
  — it does **not** read any defaults file (aggregator's own `Config`
  struct, `crates/aggregator/src/config.rs`, has no `defaults_file`
  field at all). `crates/api/src/data/config.rs:186`'s
  `ServiceArguments.defaults_file: Option<Defaults>` field exists
  (a real `--defaults-file`/`DEFAULTS_FILE` CLI arg) but, confirmed by
  grep, is read nowhere in `crates/api` today except test fixtures that
  set it to `None` — it is currently a dead field in the running binary.
  This is a real, pre-existing inconsistency this document does not
  attempt to fix (see Decision 3 and Explicitly out of scope).
- `crates/api/src/render.rs` hand-builds the public JSON shape field by
  field (`status_to_json`, `:48-`), deliberately independent of any
  `#[serde(rename)]` on `common`'s stored types — its own module doc:
  the internal storage representation and the public TfL response shape
  are different concerns. `crates/api/src/routes/incidents.rs:53-59`
  documents the concrete failure mode this exists to avoid: `rename_all =
  "camelCase"` on an outer struct does **not** propagate into a nested
  type, so embedding `common::SampleStats` (itself un-renamed,
  `avg_delay_minutes`) inside a `#[serde(rename_all = "camelCase")]` wire
  struct would silently ship a response that is camelCase at the top
  level and snake_case one level down. This has already bitten this
  codebase once; Decision 7 designs around it deliberately, not by luck.
- `crates/api/src/routes/mod.rs:22-53` (`public_router`) is where every
  non-TfL-shaped, non-`/private` read endpoint (`reference.rs`,
  `lines.rs`, `freshness.rs`, …) is mounted, nested under `/public` by
  `crates/api/src/main.rs:61`. `get_stop_point_disruption`
  (`crates/api/src/routes/line_status.rs:278-320`) establishes the
  "distinguish a real coverage gap from a genuinely-empty-but-covered
  answer via 404-vs-200-`[]`, not a shape change" precedent this design
  reuses at Decision 7.

## Decisions

### 1. Scope: one entry per (station, operator), not one unfiltered per-station number — resolves research doc Open Question 1

The research doc measured this directly: 53 of 286 sample stations
(~19%) are served by lines with genuinely different operators (Edinburgh,
Liverpool Lime Street, Newcastle, etc.), and those are disproportionately
the busiest stations most likely to actually clear `min_sample_size`. An
unfiltered per-station number would blend, say, CrossCountry's punctuality
with a local TransPennine stopping service's, under one figure, at exactly
the stations where it matters most.

This design commits to **operator-scoped only** — no unfiltered
"whole-station" number is computed or shown at all, not even alongside
the per-operator breakdown. Rationale: a flat number that's honest at
81% of stations and actively misleading at the busiest 19% is worse than
no flat number; a per-operator list degrades gracefully to "one row" at
every single-operator station (the majority case), so nothing is lost for
the common case and nothing misleading is added for the shared-station
case.

UX shape: a station page renders **one row per operator that currently
has departures at this station**, not a picker and not a single blended
figure. This mirrors how the same page already renders "one row per
line" for disruptions (`frontend/app/stations/[crs]/page.tsx:140-159`) —
no new UI paradigm, the same list-of-rows shape this page already uses,
just keyed by operator instead of by line.

### 2. Where computed: Option C, read-time in `crates/api`, no new table — resolves the research doc's core Recommendation, formally

No new migration, no new aggregator write path. `crates/api`'s new code
calls the existing `latest_station_sample` for the requested CRS and
computes stats from whatever `StationSample.departures` it returns, at
request time.

**Option B (a `station_status_daily_stats` table) is explicitly deferred**
— not rejected, deferred — per the research doc's own sequencing
recommendation: building durable per-station history now risks being
built against the smaller (LDBWS-only, 286-station) of two possible
future datasets, if TRUST/Option B (the separate, currently-"not yet"
schedule-adherence line of work) eventually ships and supersedes the
286-station ceiling with TIPLOC-based coverage. This document does not
re-derive that reasoning — see the research doc's "Relationship to the
TRUST/full-schedule line of work" section for the full argument. Nothing
in this design forecloses building Option B later; the per-operator
scoping decision (Decision 1) is schema-shape-relevant either way, so it
carries forward unchanged if/when that happens.

### 3. Threshold source: `common::Defaults::default()`, no per-station/per-operator override concept — resolves research doc Open Question 2

No per-station equivalent of `LineDefinition.severity_overrides` exists
anywhere in this codebase, and this document does not invent one. The new
read-time computation uses `common::Defaults::default()` directly — the
exact same construction `crates/aggregator/src/main.rs:50` already uses
as its own baseline for any line without a specific override. This is a
deliberate, minimal choice: it makes the per-station computation use
*the* global defaults, not *a new, api-crate-specific* defaults source.

This deliberately does **not** wire up `ServiceArguments.defaults_file`
(`crates/api/src/data/config.rs:186`) — doing so would make this feature
the first-ever real consumer of a config field that today has zero effect
on the running `api` binary, and would leave `aggregator` (hardcoded
`Defaults::default()`, no file) and `api` (a newly-live `--defaults-file`)
disagreeing about where "the" defaults come from for what is conceptually
one shared threshold. Reconciling that pre-existing inconsistency is a
real, separate cleanup this document flags but does not do — see
Explicitly out of scope.

### 4. Skip-rate definition: "did this departure skip calling at *this* station" — resolves research doc Open Question 3

The line-level `stats_from_departures` (`aggregation.rs:816-844`) checks
whether a departure's `skipped_stations` intersects `line.stations` — "did
this train skip a stop *somewhere on the route*." The research doc
flagged the per-station-natural reading as different and mechanically
simpler: "did this train skip calling *at the specific station being
asked about*" — a direct `skipped_stations.contains(this_crs)` check.

This design picks the per-station reading, explicitly, as a genuine
semantic divergence from the line-level number, not a port of it. A
station's own skip rate answering "how often do trains that would
otherwise call here skip this station specifically" is the more useful,
more literal fact for a station page to show, and is exactly what
`skipped_stations` already encodes with no further derivation needed.

### 5. Shared arithmetic: promote a generalized `common::compute_sample_stats`, refactor aggregator to delegate — resolves research doc's "What's directly reusable" #3

`stats_from_departures`'s core loop (count cancelled, count delayed above
threshold, count skipped, average delay over non-cancelled departures) has
no `aggregator`-specific dependency once the skip-check question
(Decision 4) is settled — it operates on a flat `&[&StationDeparture]`
slice already. Promoted to `common` as:

```rust
// crates/common/src/lib.rs, next to `thresholds_for`

/// Shared delayed/cancelled/skipped/avg-delay arithmetic underlying every
/// `SampleStats` computation in this app. `is_skip` is a caller-supplied
/// predicate rather than a fixed membership check, because "skip" means
/// two different, both legitimate things depending on the caller: the
/// line-level caller means "skips a stop somewhere on the line's route"
/// (`line.stations`); the per-(station, operator) caller
/// (docs/superpowers/specs/2026-09-03-per-station-stats-design.md
/// Decision 4) means "skips calling at this specific station"
/// (`skipped_stations.contains(this_crs)`). Only ever evaluated for a
/// non-cancelled departure, matching every existing caller.
pub fn compute_sample_stats(
    departures: &[&StationDeparture],
    delay_threshold_minutes: i64,
    is_skip: impl Fn(&StationDeparture) -> bool,
) -> SampleStats {
    let total = departures.len();
    let cancelled = departures.iter().filter(|d| d.is_cancelled).count();
    let delayed = departures
        .iter()
        .filter(|d| !d.is_cancelled && d.delay_minutes as i64 >= delay_threshold_minutes)
        .count();
    let skipped = departures
        .iter()
        .filter(|d| !d.is_cancelled && is_skip(d))
        .count();
    let running: Vec<&&StationDeparture> = departures.iter().filter(|d| !d.is_cancelled).collect();
    let avg_delay_minutes = if running.is_empty() {
        0.0
    } else {
        running.iter().map(|d| d.delay_minutes as f64).sum::<f64>() / running.len() as f64
    };

    SampleStats { total, delayed, cancelled, skipped, avg_delay_minutes }
}
```

`crates/aggregator/src/aggregation.rs::stats_from_departures` becomes a
thin, behavior-preserving wrapper:

```rust
pub(crate) fn stats_from_departures(
    departures: &[&StationDeparture],
    line: &LineDefinition,
    thresholds: &Defaults,
) -> SampleStats {
    let line_stations: HashSet<&str> = line.stations.iter().map(|s| s.crs.as_str()).collect();
    common::compute_sample_stats(departures, thresholds.delay_threshold_minutes, |d| {
        d.skipped_stations.iter().any(|crs| line_stations.contains(crs.as_str()))
    })
}
```

This is a small, mechanical, behavior-preserving refactor — `aggregation.rs`'s
existing `mod tests` block (`:1165` onward, dozens of `#[test]`s including
direct coverage of `stats_from_departures`/`compute_sample_availability`)
is the regression safety net and should pass unchanged; no new aggregator
behavior is introduced by this decision.

### 6. New pure logic module in `crates/api`, mirroring `eta_blend.rs`'s precedent

```rust
// crates/api/src/data/station_stats.rs (new file)

//! Per-(station, operator) `SampleStats`, computed on demand from
//! `station_samples` at read time -- Option C from
//! docs/superpowers/specs/2026-09-03-per-station-stats-research.md.
//! No new table, no new aggregator write path: reuses
//! `queries::latest_station_sample` (already shipping, backs
//! `eta_blend.rs`) and `common::compute_sample_stats` (promoted for this
//! purpose -- design doc Decision 5).

use std::collections::BTreeSet;

use common::{Defaults, SampleAvailability, StationDeparture, StationSample};

/// One operator's sample-derived stats at one station. `NoCoverage` never
/// appears here by construction -- the caller (the route handler) only
/// invokes this once it already knows `station_samples` has a row for
/// this CRS at all (design doc Decision 7's 404 gate covers the
/// no-row-at-all case one level up).
pub struct OperatorSampleStats {
    pub operator: String,
    pub availability: SampleAvailability,
}

/// One entry per distinct `operator` value observed in `sample`'s current
/// departures -- not every ATOC code this app knows about, only the ones
/// with at least one departure on today's board right now. An operator
/// with zero current departures has nothing to report and is not listed,
/// the same way a line with no `sample_stations` row for a CRS wouldn't
/// invent a `BelowThreshold { observed: 0, .. }` entry for a station it
/// doesn't cover. Sorted alphabetically by ATOC code (via `BTreeSet`) for
/// deterministic wire output, mirroring `dedup_sample_stations`'s
/// (`crates/api/src/data/samples.rs:11-23`) own rationale.
pub fn compute_station_operator_stats(
    sample: &StationSample,
    defaults: &Defaults,
) -> Vec<OperatorSampleStats> {
    let operators: BTreeSet<&str> = sample.departures.iter().map(|d| d.operator.as_str()).collect();

    operators
        .into_iter()
        .map(|operator| {
            let relevant: Vec<&StationDeparture> =
                sample.departures.iter().filter(|d| d.operator == operator).collect();
            let availability = if (relevant.len() as i64) < defaults.min_sample_size {
                SampleAvailability::BelowThreshold {
                    observed: relevant.len(),
                    required: defaults.min_sample_size,
                }
            } else {
                let stats = common::compute_sample_stats(
                    &relevant,
                    defaults.delay_threshold_minutes,
                    |d| d.skipped_stations.iter().any(|crs| crs == &sample.crs),
                );
                SampleAvailability::Available(stats)
            };
            OperatorSampleStats { operator: operator.to_string(), availability }
        })
        .collect()
}
```

No case-normalization on `operator`/`crs` comparisons — matches the
existing, established convention throughout this codebase
(`belongs_to_line`, `has_station`, `stats_from_departures`'s
`line_stations` check) of comparing CRS/operator codes with exact `==`,
relying on upstream data already being canonically uppercase. Not a new
assumption this document introduces.

### 7. API shape: `GET /public/stations/{crs}/sample-stats`, 404-vs-`[]` honesty, hand-built JSON to avoid the documented nested-rename pitfall

New route, new file, mounted in `public_router()` (not TfL-shaped — there
is no TfL precedent for this data, so no reason to mimic TfL's URL
scheme or `/StopPoint/…` shape the way `line_status.rs` must):

```rust
// crates/api/src/routes/station_stats.rs (new file)

//! `GET /public/stations/{crs}/sample-stats`: per-(station, operator)
//! delay/cancellation stats, computed on demand from `station_samples`.
//! Unauthenticated, same `public_router()` pattern as `reference.rs`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::station_stats::compute_station_operator_stats;
use crate::render::{sample_availability_json, sample_stats_json};

pub fn router() -> Router {
    Router::new().route(
        "/stations/{crs}/sample-stats",
        axum::routing::get(get_station_sample_stats),
    )
}

/// 404s when `station_samples` has no row for `crs` at all -- this app
/// has never once polled it -- mirroring `get_stop_point_disruption`'s
/// existing "not covered" honesty precedent
/// (`crates/api/src/routes/line_status.rs:278-295`). `200 []` is a
/// different, equally real fact: the row exists but genuinely has zero
/// departures right now (a quiet board).
async fn get_station_sample_stats(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let Some(sample) = queries::latest_station_sample(&app.database, &crs)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for station: {crs}"),
        ));
    };

    // No per-station override mechanism exists (Decision 3) --
    // `Defaults::default()` is the same baseline `aggregator` uses for
    // any line without its own `severity_overrides`.
    let defaults = common::Defaults::default();
    let stats = compute_station_operator_stats(&sample, &defaults);

    Ok(Json(
        stats
            .into_iter()
            .map(|s| {
                let mut out = json!({
                    "operator": s.operator,
                    "sampleAvailability": sample_availability_json(&s.availability),
                });
                if let Some(stats) = s.availability.sample_stats() {
                    out["sampleStats"] = sample_stats_json(&stats);
                }
                out
            })
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "station sample-stats query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}
```

Registered in `crates/api/src/routes/mod.rs`: add `pub mod station_stats;`
and `.merge(station_stats::router())` inside `public_router()` (line
~49-53's list). Reachable as `/public/stations/{crs}/sample-stats` once
`main.rs:61` nests it, same as every other `public_router()` entry.

**The hand-built-`json!` choice is deliberate, not incidental.**
`crates/api/src/routes/incidents.rs:53-59`'s own comment documents exactly
the failure mode a `#[derive(Serialize)] #[serde(rename_all =
"camelCase")]` wire struct embedding `common::SampleStats` directly would
hit: the outer struct's fields would render camelCase, but
`SampleStats`'s own un-renamed fields (`avg_delay_minutes`) would not,
producing an inconsistently-cased response. `sample_stats_json`/
`sample_availability_json` are extracted from `render.rs::status_to_json`'s
existing inline blocks (currently `crates/api/src/render.rs:63-78`) into
two small `pub(crate)` functions, reused by both the existing
`status_to_json` (unchanged output — `render.rs`'s existing tests,
`sample_stats_included_when_present`/`sample_availability_below_threshold_shape`/
etc., `render.rs:228-289`, cover this and should pass unchanged) and this
new route:

```rust
// crates/api/src/render.rs — extracted, not new behavior

pub(crate) fn sample_stats_json(stats: &common::SampleStats) -> Value {
    json!({
        "total": stats.total,
        "delayed": stats.delayed,
        "cancelled": stats.cancelled,
        "skipped": stats.skipped,
        "avgDelayMinutes": stats.avg_delay_minutes,
    })
}

pub(crate) fn sample_availability_json(availability: &common::SampleAvailability) -> Value {
    match availability {
        common::SampleAvailability::NoCoverage => json!({ "state": "no-coverage" }),
        common::SampleAvailability::BelowThreshold { observed, required } => {
            json!({ "state": "below-threshold", "observed": observed, "required": required })
        }
        common::SampleAvailability::Available(_) => json!({ "state": "available" }),
    }
}
```

Example response, `GET /public/stations/EDB/sample-stats`:

```json
[
  { "operator": "GR", "sampleAvailability": { "state": "available" },
    "sampleStats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.4 } },
  { "operator": "SR", "sampleAvailability": { "state": "below-threshold", "observed": 1, "required": 3 } }
]
```

Note `common::SampleAvailability`'s `NoCoverage` state is unreachable
through this route by construction (the route already 404s one level up
when there is no row at all) — this is a documented invariant of this
call site, not a type-level guarantee; reusing the existing enum wholesale
was chosen over inventing a narrower one specifically so the frontend can
reuse `sampleUnavailableReason`/`formatSampleSummary` (Decision 9) without
a second, parallel `SampleAvailability`-shaped type on the wire.

### 8. No `internal_oauth_routes` entry needed

The new route lives entirely in `public_router()`, mounted the same way
as `reference.rs`/`lines.rs`/etc. — unauthenticated, same as
`get_stop_point_disruption`. `AppState::internal_oauth_routes`
(`crates/api/src/app.rs:47`) only governs `/private/*` routes behind
`require_internal_oauth`; this route needs no entry there, matching every
other `public_router()` member.

### 9. Frontend: an independent "Sample stats by operator" block on the station page, three honest states mirroring the page's own existing disruption-coverage pattern

`frontend/lib/types.ts` — new type:

```ts
export interface StationOperatorSampleStats {
  operator: string;
  sampleAvailability: SampleAvailability;
  sampleStats?: SampleStats;
}
```

`frontend/lib/api.ts` — new call, same `no-store` convention as
`getStopPointDisruption`:

```ts
export async function getStationSampleStats(crs: string): Promise<StationOperatorSampleStats[]> {
  return fetchJson<StationOperatorSampleStats[]>(`${baseUrl()}/public/stations/${crs}/sample-stats`, {
    cache: 'no-store',
  });
}
```

`frontend/lib/sampleStats.ts` — generalize `sampleUnavailableReason`/
`formatSampleSummary`'s input type so both the existing per-line callers
and the new per-operator entries can share them, without a rename (per
the reconciliation with the full-coverage-metrics design's flagged future
rename, above — this widens the signature, it does not rename the
functions):

```ts
type SampleStatsCarrier = {
  sampleStats?: SampleStats;
  sampleAvailability: SampleAvailability;
  dataQuality?: LineStatus['dataQuality'];
};

export function sampleUnavailableReason(status: SampleStatsCarrier): string | null {
  if (status.sampleStats) return null;
  if (status.dataQuality === 'tfl') {
    return "Not measured by this app — status is TfL's own.";
  }
  if (status.sampleAvailability.state === 'no-coverage') {
    return 'No live departure data received for this line yet.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}

export function formatSampleSummary(status: SampleStatsCarrier | undefined): string {
  // body unchanged
}
```

`StationOperatorSampleStats` satisfies `SampleStatsCarrier` structurally
(no `dataQuality`, which is `undefined` — the `'tfl'` branch simply never
fires for it, and `'no-coverage'` never fires either per Decision 7's
documented invariant, so only the final, already-source-agnostic fallback
string is ever reachable for a per-operator entry; no wording change
needed to the existing, already-tested strings).

`frontend/app/stations/[crs]/page.tsx` — a new, independent fetch and
section, structurally mirroring the page's own existing
`fetchStationDisruptions`/`coverage: 'covered' | 'none'` pattern
(`page.tsx:58-70`) exactly, because it is the same shape of fact (a
different, orthogonal coverage question — LDBWS sampling coverage, not
line-status-catalogue coverage — so a station can independently be
`covered`/`none` for disruptions and `sampled`/`not-sampled` for stats):

```tsx
type StationSampleStatsResult =
  | { coverage: 'not-sampled' }
  | { coverage: 'sampled'; operatorStats: StationOperatorSampleStats[] };

async function fetchStationSampleStats(crs: string): Promise<StationSampleStatsResult> {
  try {
    const operatorStats = await withStaleFallback(`stationSampleStats:${crs}`, () => getStationSampleStats(crs));
    return { coverage: 'sampled', operatorStats };
  } catch (err) {
    if (err instanceof ApiNotFoundError) return { coverage: 'not-sampled' };
    throw err;
  }
}
```

Fetched alongside the page's existing `Promise.all` (disruptions,
preferences), plus `getAllTocs()` (already the established pattern for
resolving an ATOC code to a display name, `frontend/app/lines/page.tsx:28-31`
— `.catch(() => [])`, degrading the name column rather than the page).
Rendered as its own block, independent of the disruption section above
it, with three explicit states (mirroring the page's existing
`coverage === 'none'` / `coverage === 'covered' && reports.length === 0`
/ non-empty three-way split immediately above it in the same file):

- `coverage === 'not-sampled'` → *"This station isn't part of our live
  departure sampling."*
- `coverage === 'sampled' && operatorStats.length === 0` → *"No live
  departures currently recorded at this station."* (a genuinely quiet
  board, not a coverage gap — same distinction the disruption section
  already draws for its own two absences)
- otherwise → one row per operator, `tocs`-resolved display name (falling
  back to the bare ATOC code when not found in the hourly-cached list,
  same fallback shape `AllLinesTable.tsx:81` already establishes), plus
  `formatSampleSummary(entry)` for the trailing dimmed text — reusing
  the exact same rendering call already used for the per-line list
  directly above it on this same page (`page.tsx:145-148`).

### 10. Naming

- Rust: `common::compute_sample_stats` (shared arithmetic),
  `station_stats::{OperatorSampleStats, compute_station_operator_stats}`
  (api-crate-local, station-shaped gate), `routes::station_stats::router`
  (new route module).
- Wire: `operator`, `sampleAvailability`, `sampleStats` — deliberately the
  same field names `LineStatus` already uses on the wire, so a frontend
  reader recognizes the shape immediately.
- Frontend: `StationOperatorSampleStats` (type), `getStationSampleStats`
  (fetcher), `fetchStationSampleStats` (page-local coverage wrapper,
  named identically in shape to the existing `fetchStationDisruptions`).

## Architecture

```
station page request
        │
        ▼
GET /public/stations/{crs}/sample-stats  (crates/api/src/routes/station_stats.rs)
        │
        ▼
queries::latest_station_sample(pool, crs)   ← already shipping, unmodified
        │  (Option<StationSample>)
        ▼
station_stats::compute_station_operator_stats(sample, Defaults::default())
        │  groups sample.departures by `operator`, gates each group via
        │  common::compute_sample_stats (shared with aggregator)
        ▼
Vec<OperatorSampleStats>  →  hand-built JSON via render.rs's extracted helpers
        │
        ▼
frontend: getStationSampleStats → fetchStationSampleStats → page section
```

No write path is added anywhere. `aggregator`'s existing per-cycle line
pass is entirely unmodified except for the internal `stats_from_departures`
refactor (Decision 5), which is behavior-preserving.

## Error handling

- **Database error** (connection failure, etc.): `internal_error` →
  `500`, logged — same shape as every other route in this file's
  siblings (`reference.rs`, `line_status.rs`).
- **No `station_samples` row for `crs`**: `404`, distinct from every other
  failure — mirrors `get_stop_point_disruption`'s existing precedent.
  Frontend renders this as a plain, honest sentence, not a page-level
  error (the page already has a `notFound()`-vs-`unavailable` split at
  the top for "is this even a real station," Decision 9's states are a
  layer beneath that, for "does this real station have live sampling").
- **Row exists, zero departures**: `200 []`, rendered as "quiet, not
  missing" — same honesty distinction the disruption section already
  makes one field over.
- **Frontend fetch failure that isn't a 404** (network blip, `withStaleFallback`
  in play): served stale per `withStaleFallback`'s existing behavior,
  identical to how the disruption section already handles it — no new
  caching mechanism.

## Explicitly out of scope

- **Option B — a `station_status`/`station_status_daily_stats` table, or
  any per-station history/Trends-equivalent.** Deferred per the research
  doc's sequencing recommendation until either a concrete product need
  for per-station *history* is confirmed, or TRUST/Option B's own Task 8
  reaches a verdict (see Decision 2).
- **Broadening `poller-ldbws`'s polling scope beyond the current 286
  `sample_stations`-derived list.** A materially larger, separate piece
  of work (RDM rate limits, per-cycle time budget) the research doc names
  but does not size — unaffected by this document, which stays scoped to
  the 286 stations that already have data.
- **Any per-station/per-operator threshold override mechanism.** Decision
  3 uses global `Defaults::default()` only; inventing a station-keyed
  `severity_overrides` analogue is real, separate work with no current
  demand established.
- **Reconciling `ServiceArguments.defaults_file`'s current dead-field
  status with `aggregator`'s hardcoded `Defaults::default()`.** A real,
  pre-existing inconsistency this document surfaces (Decision 3) but does
  not fix — this feature deliberately does not become the first thing to
  depend on `defaults_file` actually working.
- **Any severity/badge/incident-adjacent treatment of station-operator
  stats.** This is purely informational, like `LineStatus.sample_stats`
  already is — never feeds a severity classification, never appears on a
  `StatusBadge`.
- **A combined or "all operators" summary figure at a shared station.**
  Decision 1 is deliberate: no unfiltered number is shown, ever, even as
  a secondary figure alongside the per-operator rows.
- **A UI picker, tab, or filter for stations with many operators.**
  Decision 1's plain list-of-rows is judged sufficient; the busiest
  stations in the 53/286 set top out around 8 operators, well within
  "just list them" territory the existing per-line list on the same page
  already handles.

## Open questions / risks

1. **Is 286 stations, list-of-rows UI, and no history the right increment
   to ship, or should the frontend section wait for a richer treatment?**
   Not resolved here — a genuine product-priority call, same as the
   research doc's own Open Question 7. This document assumes "yes, ship
   the honest minimal version now" without re-litigating it.
2. **Operator display names for ATOC codes absent from `getAllTocs()`'s
   result** (a code that appears on a live departure board but isn't yet
   in the `tocs` reference table) fall back to the bare code — untested
   against real data for how often this actually happens; a real risk,
   not just a theoretical one, since `tocs` and `station_samples` are
   fed by two entirely independent pollers with no cross-validation.
3. **Whether the per-operator `min_sample_size` gate (Decision 3's global
   `Defaults::default().min_sample_size`, currently 3) is too low/high
   for a single station's typically much smaller departure count than a
   line's pooled multi-station sample** — the line-level number pools
   across 2-5 stations before gating; a per-station number never pools at
   all, so it may cross `BelowThreshold` far more often in practice.
   Worth watching once shipped, not resolved by this document.
4. **The unreachable-`NoCoverage`-through-this-route invariant (Decision
   7) is documented, not type-enforced** — a future change to
   `compute_station_operator_stats` that somehow constructs `NoCoverage`
   would silently produce a `sampleAvailability.state === 'no-coverage'`
   entry the frontend's `sampleUnavailableReason` would render with the
   line-specific "No live departure data received for this line yet"
   string, which would read oddly for a station/operator context. Low
   risk given the current code shape, flagged for whoever touches this
   code next.
