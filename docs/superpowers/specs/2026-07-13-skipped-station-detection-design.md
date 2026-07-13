# Skipped-Station Detection Design

## Problem

The system currently accounts for cancelled trains and delayed trains when
computing a line's inferred status severity from LDBWS sample data, but has
no concept of a train that's still running but skipping one or more of its
normally-scheduled calls (e.g. "not calling at Woking today due to a
signalling fault"). Darwin's live departure board feed carries enough
information to detect this, but the current `poller-ldbws` parsing only
looks at each service's final `destination`, `std`/`etd`, and top-level
`isCancelled` flag — it never inspects the calling-points list where a
scheduled-but-skipped stop is actually recorded.

Critically, a service *not calling* at a station because it was never on
that service's route in the first place (fast trains skip stops as a
matter of normal, non-disruptive scheduling) must not be confused with a
service that *was* scheduled to call somewhere and is skipping it today.
Only the latter is a real disruption signal.

## Goals

- Detect genuine skipped-station events (scheduled call, marked cancelled
  on that specific service today) from the Darwin/LDBWS feed.
- Feed a skip rate into a line's severity classification as an
  **independent signal** from cancellation and delay, with its own
  overridable thresholds.
- Surface an aggregate skip count in the existing `RepresentativeInfo`
  display, alongside the delayed/cancelled/avg-delay stats already shown
  there.

## Non-goals (this iteration)

- No per-service or per-station skip detail anywhere in the UI (no "which
  train, which stop" breakdown) — aggregate counts only, matching how
  cancellations are handled today.
- No station-detail-page view of skips (e.g. "N services not calling here
  today") — out of scope for this iteration.
- No new `dataQuality` tier — skip-driven inference stays `ldbws-inferred`,
  same as delay/cancellation-driven inference.

## Calling-point field names

The existing `crates/poller-ldbws/src/schema.rs` field list (`destination`,
`std`, `etd`, `isCancelled`, `cancelReason`, `delayReason`) was transcribed
from a Swagger 2.0 spec fetched during that poller's original planning (see
`docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md`). That pass
recorded only the fields actually used at the time — it never transcribed
the calling-points structure, and no swagger/OpenAPI spec file for this
feed is checked into the repo.

This gap is now resolved by cross-referencing the official Darwin OpenLDBWS
XSD (`rtti_2021-11-01_ldb_types.xsd`, the SOAP/XML schema RDM's REST/JSON
product wraps). Its `CallingPoint` complexType declares (in order):
`locationName`, `crs`, `st`, `et`, `at`, `isCancelled`, `length`,
`detachFront`, `formation`, `adhocAlerts`, `cancelReason`, `delayReason`,
`affectedByDiversion`, `rerouteDelay`, `uncertainty`, `affectedBy`. Every
field this codebase's existing `RdmServiceItem` already uses
(`isCancelled`, `cancelReason`, `delayReason`) matches this XSD's element
names verbatim, which is strong corroboration that RDM's JSON is a direct
camelCase rendering of the same underlying schema, unwrapped from the SOAP
envelope — the same confidence tier the existing code's own doc comment
already claims for its other fields.

`subsequentCallingPoints`/`previousCallingPoints` are of XSD type
`ArrayOfArrayOfCallingPoints`: an array of `callingPointList` elements
(type `ArrayOfCallingPoints`), each containing `callingPoint` elements
(type `CallingPoint`). The nesting exists to support service splits/joins
(multiple associations); this app doesn't care about split-service
semantics and flattens across every `callingPointList` into one flat
skipped-CRS list per service. Expected RDM JSON shape:

```json
"subsequentCallingPoints": [
  {
    "callingPoint": [
      { "locationName": "Woking", "crs": "WOK", "st": "10:15", "isCancelled": true },
      { "locationName": "Basingstoke", "crs": "BSK", "st": "10:32", "isCancelled": false }
    ]
  }
]
```

## Design

### 1. Ingestion (`crates/poller-ldbws/src/schema.rs`)

- Extend `RdmServiceItem` to deserialize the `subsequentCallingPoints` list
  (field names confirmed above).
- Extract every calling point marked cancelled into a flat `Vec<String>`
  of CRS codes.
- Add `skipped_stations: Vec<String>` to `common::StationDeparture`
  (defaults to empty when a service reports no skipped calls).
- `poller-ldbws` has no line-topology awareness (it only samples a flat
  CRS list learned from `GET /private/sample-stations`), so it captures
  *every* skipped calling point Darwin reports for a service, unfiltered.
  Filtering down to "does this skip matter to line X" happens downstream,
  in the aggregator.

### 2. Aggregation & severity (`crates/aggregator/src/aggregation.rs`)

- For each departure already deemed relevant to a line (existing
  `belongs_to_line` + `sample_stations` filtering, unchanged), check
  whether `departure.skipped_stations` intersects the line's own
  `stations` list (by CRS). A non-empty intersection counts as one "skip
  event" for that line's sample.
- `common::SampleStats` gains `skipped: usize`, computed the same way
  `cancelled` is today. `skip_rate = skipped / total`.
- `common::Defaults` (the thresholds struct) gains two new
  `#[serde_inline_default]` fields, overridable per-line via the existing
  `severity_overrides` TOML mechanism exactly like `minor_delays_pct` etc.
  already are:
  - `minor_delays_skip_pct` (default `0.25`, matching `minor_delays_pct`)
  - `severe_delays_skip_pct` (default `0.50`, matching `severe_delays_pct`)
- `classify()` changes: after the existing cancel-rate checks
  (`PartSuspended`/`ReducedService` — unchanged, still highest priority),
  compute two candidate severities at the milder tier:
  - a delay-rate candidate against `minor_delays_pct`/`severe_delays_pct`
    (existing logic, unchanged)
  - a skip-rate candidate against the two new skip thresholds
  Take whichever candidate is **more severe** (lower `Severity` ordinal —
  `SevereDelays` = 6 outranks `MinorDelays` = 9). If skip is the deciding
  factor, the reason text reads `"{skipped} of {total} sampled services
  skipping a scheduled stop."` in place of the delayed-count message. If
  both candidates land on the same tier, combine into one message
  mentioning both counts.
  `data_quality` remains `LdbwsInferred` in all cases.
  `min_sample_size` gating is unchanged (applies to the same relevant-set
  size as before).

### 3. API & frontend exposure

- `crates/api/src/render.rs` serializes `sampleStats.skipped` alongside
  `sampleStats.cancelled` — same pattern, no new endpoint.
- `frontend/lib/types.ts`'s `SampleStats` gains `skipped: number`.
- `frontend/components/RepresentativeInfo.tsx` extends the existing
  aggregate-stats line (which currently shows delayed/cancelled/avg-delay)
  to also show the skip count, e.g.:
  `"142 of 160 sampled services delayed, 3 cancelled, 2 skipping stops, avg 12.4 min late."`
- No other frontend changes: no new filter, no new badge type, no
  per-service/per-station breakdown anywhere. `IssueList`'s existing
  `dataQuality` badges are unaffected since skip-driven and delay-driven
  statuses both still report `ldbws-inferred`.

## Testing plan

- `poller-ldbws/src/schema.rs`: parse tests for calling-point extraction
  (once real field names are confirmed via the discovery task), including
  a service with no skipped calls (empty `skipped_stations`) and a service
  with one or more genuinely skipped calls.
- `aggregator/src/aggregation.rs`: `classify()` tests covering skip-rate
  crossing the minor/severe thresholds independently of delay-rate, a
  combined case where both delay and skip candidates fire at the same
  tier, and confirmation that cancel-rate still takes priority over both
  milder-tier candidates.
- `RepresentativeInfo.test.tsx`: extend existing fixtures with a `skipped`
  value and assert it renders in the aggregate stats line, following the
  same pattern just used to add `cancelled` to this component.

## Open items carried into the implementation plan

- Whether `minor_delays_skip_pct`/`severe_delays_skip_pct` defaults
  (0.25/0.50) need per-line overrides in any existing `lines/*.toml` file,
  or whether the global defaults are fine for the initial rollout — no
  existing line currently overrides `minor_delays_pct`/`severe_delays_pct`,
  so defaulting to the same values keeps behavior conservative out of the
  gate.
