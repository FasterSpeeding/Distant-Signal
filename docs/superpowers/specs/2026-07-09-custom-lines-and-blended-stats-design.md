# Custom Lines + Blended Delay Stats — Design

Sub-project 1 of 3 (see also: `2026-07-09-frontend-personalization-design.md`,
`2026-07-09-outage-page-redesign-design.md`). This is the foundational
backend piece — the other two depend on the API shape defined here.

## Goals

- Let a user define their own line (name, operators, ordered stations)
  through the `api` service, and have it get real status computation —
  incident matching and LDBWS-inferred delay stats — same pipeline as the
  static `lines/*.toml` catalogue.
- Make sample-derived delay/cancellation stats available as supplementary
  data on a `LineStatus` even when an incident is also active, instead of
  only when there's no incident at all (today's behavior).

## Non-goals

- No auth/ownership on custom lines yet. Unauthenticated CRUD, consistent
  with this app's current "single trusted personal instance" model.
  SSO/multi-user, and gating custom lines per-user, is a known future
  direction, not built here — no placeholder `owner_id` column either
  (add it in the migration that actually adds auth).
- No segment/shared-trunk topology, `match_keywords`/`excluded_keywords`,
  or `severity_overrides` for custom lines in v1. Those exist to encode
  official-line route topology and threshold tuning; a custom line gets
  plain `StationHit`/`OperatorOnly` incident matching and standard-
  threshold LDBWS inference, which is already real, useful status
  computation without that complexity.
- Sample stats never escalate severity above what an active incident
  reports — informational only.

## Data model

New table `custom_lines`:

| column                  | type        | notes                              |
|--------------------------|-------------|-------------------------------------|
| `id`                     | text PK     | slug, e.g. `custom-my-commute`      |
| `name`                   | text        |                                      |
| `operators`               | text[]      | ATOC codes                          |
| `stations`               | jsonb       | ordered list of `{crs}`             |
| `headcode_prefixes`       | text[]      | optional, narrows LDBWS matching    |
| `destination_crs_filter`  | text[]      | optional, narrows LDBWS matching    |
| `created_at`             | timestamptz |                                      |

`mode` is implicitly `"national-rail"` (the only mode this app models).
No `segment` per station, no `match_keywords`/`excluded_keywords`,
no `severity_overrides` — see Non-goals.

## Loading changes

Both `api` and `aggregator` currently parse the static `lines/*.toml`
catalogue once at process startup (`ServiceArguments`/`Config::lines`,
loaded via `clap` at process start) and never re-read it.

- **`aggregator`**: `run_cycle` (`crates/aggregator/src/main.rs`) gains a
  `queries::load_custom_lines(pool)` call each cycle, merged into the
  static `lines: HashMap<String, LineDefinition>` before
  `SegmentRegistry::new(&lines)` and `aggregation::aggregate(...)` run.
  A custom line is converted into a `LineDefinition` with empty
  `stations[].segment`, `match_keywords`, `excluded_keywords`,
  `exclusive_segments`, `severity_overrides` — the existing matcher and
  `infer_from_samples` logic then just works unmodified (confirmed:
  `StationHit` matching only reads `line.stations`/`has_station`;
  segments only *upgrade* an existing `StationHit` to
  `Shared`/`ExclusiveSegment`, they're not required for a match to fire
  at all).
- **`api`**: `GET /sample-stations` (`crates/api/src/routes/samples.rs`)
  currently derives from `app.config.lines` (static). Changes to query
  `custom_lines` from the DB and merge in each request, so
  `poller-ldbws` picks up custom lines' stations for sampling without
  restarting anything.

## Blended stats

`infer_from_samples` (`crates/aggregator/src/aggregation.rs`) currently
only runs in "Layer 2: inference for lines with no incidents" — skipped
entirely once Layer 1 (incidents) has produced any status for a line.

Change: always compute the sample-derived numbers (when
`min_sample_size` is met) regardless of whether an incident exists, and
attach them as a new field:

```rust
pub struct SampleStats {
    pub total: usize,
    pub delayed: usize,
    pub cancelled: usize,
    pub avg_delay_minutes: f64,
}

pub struct LineStatus {
    // ...existing fields...
    pub sample_stats: Option<SampleStats>,
}
```

- No incident: behavior unchanged — the inferred status *is* the status,
  as today, now with `sample_stats` also populated on it.
- Incident active: the incident's severity/reason still wins;
  `sample_stats` is computed independently from the same sample data
  `infer_from_samples` already reads and attached to the incident-derived
  `LineStatus` alongside it. Never used to change `severity`.

This threads through `crates/common` (shared `LineStatus` struct),
`crates/api/src/render.rs` (JSON serialization — new optional
`sampleStats` field), and `frontend/lib/types.ts` (new optional field on
`LineStatus`).

## New API endpoints (all under `/public/lines`, unauthenticated)

- `GET /public/lines` — every line, official + custom, each tagged
  `source: "catalogue" | "custom"`. Backs the "all lines" browse page
  (sub-project 2) without needing a separate enumeration mechanism.
- `POST /public/lines` — create a custom line. Body: `name`, `operators`,
  `stations` (ordered CRS list), optional `headcodePrefixes`/
  `destinationCrsFilter`.
- `DELETE /public/lines/{id}` — custom lines only; 404 (or 400) if `id`
  resolves to a catalogue line rather than a custom one. Cascades to
  remove any `pinned_lines` row referencing it (see sub-project 2) in the
  same transaction.

## Testing

- `aggregation.rs` unit tests: a custom-line-shaped `LineDefinition`
  (no segments/keywords) still gets `StationHit`/`OperatorOnly` incident
  matches and `LdbwsInferred` status from samples — same assertions
  pattern as the existing SWR/WCML tests, just built from a literal
  `LineDefinition` instead of `load_all_lines()`.
- New test: a `LineStatus` with both an incident and qualifying samples
  ends up with the incident's severity *and* a populated `sample_stats`,
  not just one or the other.
- `queries.rs` (aggregator + api): round-trip test for
  `load_custom_lines`/`GET /public/lines`/`POST`/`DELETE` against a real
  Postgres instance (matches existing test patterns using the same test
  DB setup as `upsert_stations` etc.).
