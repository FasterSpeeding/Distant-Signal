# Stale Incident Handling Design

## Problem

Two related gaps let a Knowledgebase incident keep influencing a line's
displayed status long after it stops being representative of reality.

**The dead-code gap.** `incidents.is_cleared` (RDM's `ClearedIncident` flag)
and each incident's `validity_periods` are stored and even indexed
(`incidents_active ON incidents (incident_id) WHERE NOT is_cleared`), but
`aggregator::queries::load_incidents` loads every row with no `WHERE`
clause, and `aggregator::aggregation::aggregate()` never checks `is_cleared`
or whether any validity period actually covers "now" — it just picks a
period to *display* (`validity_for_output`), falling back to the first one
even if it's fully in the past. Combined with upsert-only ingestion (no
`DELETE` anywhere in the codebase), a properly-cleared incident still keeps
producing a `LineStatus` forever.

**The SWR-shaped gap, which the above fix doesn't solve.** In practice,
South Western Railway incidents are often updated in their free-text
summary/description to say the underlying issue has been resolved, while
`is_cleared` stays `false` and `validity_periods` is never narrowed —
sometimes for days or months. Residual delays for a while after an
incident's root cause is fixed are genuinely representative in the short
term, but an incident that's still "active" per RDM's structured fields
weeks later never was and never will be representative again. No amount of
respecting `is_cleared`/validity fixes this, because RDM's own structured
data never changes for these incidents — only the human-authored text does.

## Goals

- Respect `is_cleared` and validity-period expiry when deciding whether an
  incident contributes to a line's current status (currently ignored
  entirely).
- Cap how long any non-planned (real-time) incident can keep influencing a
  line's status, independent of whether RDM's own fields ever get updated —
  expire it at the next UK rail "traffic day" boundary (02:00 Europe/London)
  after we first saw it, per Network Rail's timetable convention.
- Exempt planned engineering work (`is_planned = true`) from the rail-day
  cutoff — its own `validity_periods` are the legitimate source of truth
  for how long it should show, and those spans are meant to run multiple
  days/weeks by design.
- When an incident is filtered out (cleared, expired validity, or aged past
  the rail-day cutoff), the line falls back to the same "no incident" path
  used today: sample-derived inference, or Good Service.

## Non-goals (this iteration)

- No deletion/pruning of old rows from `incidents` — table growth is a
  separate housekeeping concern from displaying wrong statuses, and
  `incident_history` already retains a full audit trail regardless of what
  happens to the live table.
- No "hasn't been refreshed in N poll cycles ⇒ implicitly cleared" signal —
  a different, unconfirmed failure mode (incidents vanishing from the feed
  entirely) from the one being fixed here (incidents whose structured
  fields simply never get updated while still present in the feed).
- No configurable rail-day boundary hour, and no per-line override — 02:00
  Europe/London is a fixed industry convention, not a tunable.
- No change to `IncidentMessage`'s wire shape (poller ↔ API contract) —
  `first_seen_at` is purely a database-side fact, never sent by or read by
  a poller.

## Design

### 1. Schema (`crates/api/migrations`)

- New migration adds `first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()` to
  `incidents`.
- `crates/api/src/data/queries.rs::upsert_incidents`: add `first_seen_at` to
  the `INSERT` column list, bound to `NOW()`, but deliberately omit it from
  the `ON CONFLICT DO UPDATE SET` clause. It is stamped once, on first
  insert, and never touched by any later edit — including the exact
  summary/description/priority/validity edits that leave `is_cleared`
  stale. This is what makes it immune to RDM's own data-quality gap: it's
  our clock, not theirs.

### 2. Loading incidents (`crates/aggregator/src/queries.rs::load_incidents`)

- Add `WHERE NOT is_cleared` to the query. Cheap, and finally gives the
  existing `incidents_active` partial index a purpose — cleared incidents
  never reach the aggregator's matching logic at all.
- Select `first_seen_at` alongside the existing columns.
- Return type changes from `Vec<IncidentMessage>` to `Vec<LoadedIncident>`,
  a small new struct local to the aggregator crate:
  `struct LoadedIncident { message: IncidentMessage, first_seen_at: DateTime<Utc> }`.
  `IncidentMessage` itself is untouched, keeping the poller-facing wire type
  unaware of a fact only the aggregator's database concerns itself with —
  the same separation `computed_at` got in the last-updated-indicators
  feature.

### 3. Rail-day boundary math (`crates/aggregator/src/aggregation.rs`)

- Add `chrono-tz` as a new dependency of the `aggregator` crate (not
  currently used anywhere in the workspace) — a fixed UTC offset would be
  wrong roughly half the year across the BST/GMT transition.
- `fn next_rail_day_boundary(first_seen_at: DateTime<Utc>) -> DateTime<Utc>`:
  converts `first_seen_at` to Europe/London local time. If the local
  time-of-day is before 02:00, it belongs to the previous calendar day's
  rail day, so the boundary is 02:00 on that same local calendar day;
  otherwise the boundary is 02:00 on the next local calendar day. The
  resulting local 02:00 is converted back to UTC.
- DST edge cases, handled explicitly via `chrono-tz`'s `LocalResult` rather
  than left to an unwrap:
  - **Autumn (clocks back, local 02:00 occurs twice)** →
    `LocalResult::Ambiguous(earliest, latest)`: pick `earliest`, so the
    incident goes stale at the first occurrence of 02:00 rather than
    waiting for the second.
  - **Spring (clocks forward, 01:00 → 02:00 never happens)** →
    `LocalResult::None`: treat the boundary as the instant of the jump
    itself (what would have been 02:00 GMT arrives simultaneously with
    01:00 BST that night).

### 4. The filtering predicate (`crates/aggregator/src/aggregation.rs`)

New pure function:

```rust
fn is_active(incident: &IncidentMessage, first_seen_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let validity_ok = incident.validity.is_empty()
        || incident.validity.iter().any(|p| {
            p.from_date <= now && p.to_date.map(|to| to > now).unwrap_or(true)
        });
    let age_ok = incident.is_planned || now < next_rail_day_boundary(first_seen_at);
    validity_ok && age_ok
}
```

`is_cleared` isn't rechecked here — `load_incidents` already excludes
cleared rows at the SQL layer, so by the time an incident reaches
`aggregate()` it's already known not to be cleared.

- `aggregate()` computes `let now = Utc::now();` once at the top of the
  function (rather than each interior call reading the live clock
  separately, as `validity_for_output`/`good_service()`/etc. do today), and
  filters its `incidents: &[LoadedIncident]` parameter through `is_active`
  before the existing matching loop. A filtered-out incident never enters
  `lines_affected_by`, so the affected line falls through to the existing
  sample-derived/Good-Service path, unchanged.
- `aggregate()`'s signature changes from `incidents: &[IncidentMessage]` to
  `incidents: &[LoadedIncident]`. `status_from_incident` and everything
  downstream keep taking `&IncidentMessage` (via `&loaded.message`), so the
  rest of the file — severity classification, matching, disruption
  rendering — is unaffected.

## Testing plan

- `next_rail_day_boundary`: a plain midweek case; a BST→GMT transition
  night (ambiguous local 02:00, asserts the earliest occurrence wins); a
  GMT→BST transition night (nonexistent local 02:00, asserts the jump
  instant is used); a `first_seen_at` just before vs. just after local
  02:00, to confirm which rail day it's assigned to.
- `is_active`: empty validity (active); a validity period currently
  covering `now` (active); a validity period that's already elapsed
  (inactive); a non-planned incident first seen earlier in the current
  rail day (active); a non-planned incident first seen before the last
  rail-day boundary (inactive); a planned incident aged past the same
  boundary (still active — confirms the exemption).
- `aggregate()`: extend the existing integration-style tests with a stale
  non-planned incident, asserting the affected line falls back to
  sample-derived/Good-Service status rather than showing the stale
  incident, and a planned-work incident aged the same amount, asserting it
  still shows.
- Migration: confirm `first_seen_at` defaults sensibly (`NOW()`) for any
  pre-existing rows at migration time — a deliberate, safe under-count of
  true age, rather than guessing a false-earlier timestamp that would
  mass-expire the entire table the moment the migration runs.

## Open items carried into the implementation plan

- Exact placement of `next_rail_day_boundary`/`is_active` (inline in
  `aggregation.rs` vs. a small new module) — implementation detail, doesn't
  affect behavior.
- Whether this codebase has an existing pattern for DB-backed integration
  tests of query functions like the `load_incidents` SQL change, or whether
  it's thin enough to cover via the migration plus manual verification only
  — confirm against existing test conventions during planning.
