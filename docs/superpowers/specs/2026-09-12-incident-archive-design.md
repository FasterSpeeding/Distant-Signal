# Design: Cross-Network Incident Archive/Search Page

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md` (closest
precedent — the design for `/incidents/[id]` and its backing route, which
this spec builds directly on top of) and
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(this repo's template for a full frontend-plus-backend design with no
implementation plan included — that is a separate, later step).

## Goal

`/incidents/[id]` (shipped per the 2026-08-31 spec) shows one incident's
full detail, and `/lines/[id]/history`'s Timeline tab shows a line's own
incident-derived status changes — but nothing lets a user browse or search
Knowledgebase incidents **across the whole network**, independent of which
line happened to be looking at them. The 2026-08-31 spec named this gap
explicitly and declined to design it ("Explicitly out of scope... No
backend route supports 'list every incident' either; adding one is a
separate, unscoped feature"). This spec designs that feature: a
`GET /public/incidents` list/search route and a `/incidents` frontend page
that filters by operator, line, date range, priority, and planned/unplanned
status, linking each result to the existing `/incidents/[id]` detail page.

## Corrections to the brief's assumptions

Direct inspection of the schema, the matcher, and this codebase's own
pagination/filter conventions turned up several things the brief's filter
list glossed over, materially affecting the design below.

1. **There is no "line" column on `incidents`, and there never can be one
   the way the brief implies.** Which lines an incident affects is not a
   stored fact — it's a *live computation* the aggregator re-derives every
   cycle (`crates/aggregator/src/matcher.rs::lines_affected_by`), using a
   five-tier scope classification (`ExclusiveSegment`, `SharedSegment`,
   `StationHit`, `KeywordOnly`, `OperatorOnly`) that consults each line's
   station list, the shared-segment registry, keyword lists, and operator
   overlap — none of which the `incidents` row itself carries, and none of
   which is persisted anywhere keyed by `(incident, line)` over time. The
   2026-08-31 spec's own `lines_currently_reporting_incident` query
   (Decision 3) only answers "which lines' **current** `line_status` rows
   happen to embed this exact `disruption.source` string right now" — it
   reads `line_status`, a live-state table with one row per line, not a
   history. For an *archive* of incidents that may be long-cleared, that
   table won't have a matching row at all (a cleared or superseded
   incident's line long since moved on to a different status). **A
   backend "filter by line" cannot re-run the real matcher retroactively**
   (that needs the full `SegmentRegistry` and each line's catalogue
   definition, is aggregator-side logic, and even if ported would be
   re-deriving the matcher's answer for a point in time that has already
   passed and whose keyword/operator dictionaries may since have changed) —
   see Decision 2 for the approximation this spec uses instead, and its
   named limitation.
2. **"Severity" is not a real, filterable concept on this data.**
   `incidents.priority` is documented in this codebase's own code as `//
   raw IncidentPriority integer — no documented enum, do not re-invent
   "major"/"minor"` (`crates/aggregator/src/aggregation.rs`, `LineStatus`'s
   own field comment), and the original `severity_hint TEXT CHECK
   (severity_hint IN ('major', 'minor'))` column from the very first
   migration (`20260510023522_initial.sql`) was dropped and never
   replaced with a documented enum. The 2026-08-31 spec flagged the same
   thing as its own Open Question #1 and left `priority` on the wire "as
   a raw integer... this spec does not resolve how or whether to surface
   it." This spec cannot invent a "Major/Minor/Minor-and-below" filter
   that doesn't exist in the data — see Decision 3 for what it offers
   instead (a raw numeric range, clearly labeled as an unexplained raw
   feed value, not a resolution of that open question).
3. **This codebase already has an established, load-bearing pagination
   convention that isn't cursor-vs-offset ambiguous — it's cursor, and a
   specific cursor shape.** `crates/api/src/routes/trains.rs`'s
   `GET /trains/search` (`get_trains_search`) is the one existing
   general-purpose *search* route (as opposed to a by-id lookup) in this
   codebase, and it uses: an opaque base64url(no-pad)-encoded keyset
   cursor (`encode_cursor`/`decode_cursor`), a `limit` query param clamped
   to a `MAX_SEARCH_LIMIT` constant (never rejected for being too large,
   only for being non-positive or unparseable), an `after` query param
   naming the previous page's cursor, a `nextCursor` field in the response
   that is explicit JSON `null` on the last page (never omitted), and
   `#[serde(deny_unknown_fields)]` on the query-params struct so a
   misspelled filter name 400s instead of silently no-op'ing. This spec
   matches that convention exactly (Decision 4) rather than introducing
   offset/page-number pagination, which exists nowhere else in this
   codebase's read routes.
4. **The brief's "operator/line" filters need a multi-value shape, and
   this codebase already has a real precedent for that which isn't
   repeated query keys.** `crates/api/src/routes/trains.rs`'s own history
   (`stops_at`) shows repeated-key multi-value query params were tried and
   *deliberately reverted* (needed the `axum_extra`/`serde_html_form`
   dependency, then was scoped back down to a single value). The
   convention this codebase actually keeps is a single comma-separated
   value in one query param — `GET /Line/{ids}/Status` takes
   `ids = "line1,line2"` and does `ids.split(',')`
   (`crates/api/src/routes/line_status.rs:191`). Decision 2 follows that
   precedent for `operator`, not repeated keys.
5. **A GIN-indexed array-overlap filter over `incidents.affected_stations`
   is the closest real primitive this schema has to "browse by line", and
   it already exists — no schema change needed for it.** See Decision 2.

## Current relevant state (verified 2026-09-12)

**`incidents` table** (unchanged since the 2026-08-31 spec's own inventory
— re-verified against every migration through `20260828120000_train_tracking.sql`,
nothing later touches `incidents`):

```
incident_id       TEXT PRIMARY KEY
summary           TEXT NOT NULL
description       TEXT NOT NULL
operators         TEXT[] NOT NULL          -- ATOC codes
affected_stations TEXT[] NOT NULL          -- CRS codes
priority          INTEGER NOT NULL         -- raw RDM int, no documented enum
validity_periods  JSONB NOT NULL DEFAULT '[]'
is_planned        BOOLEAN NOT NULL DEFAULT FALSE
is_cleared        BOOLEAN NOT NULL DEFAULT FALSE
fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
first_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()   -- this app's own clock
-- extracted_* / source_text_hash NLP columns — internal, not surfaced here either
```

**Existing indexes** (`20260510023522_initial.sql`,
`20260706004003_reference_data.sql`):

- `incidents_affected_stations_gin` — `GIN (affected_stations)`. Supports
  the `&&` (overlap) operator efficiently — **this is exactly what
  Decision 2's line/station filter needs, unchanged.**
- `incidents_operators_gin` — `GIN (operators)`. Same `&&` support —
  **exactly what Decision 2's operator filter needs, unchanged.**
- `incidents_active` — `(incident_id) WHERE NOT is_cleared`, a *partial*
  index over the primary key column itself, added to fix an earlier
  version that predicated on a since-removed `valid_from`/`valid_to`
  pair. It carries no orderable column and was built for a narrower
  "does this specific still-active id exist" check, not "give me active
  incidents newest-first" — **it does not help this feature's default
  listing order.** See Decision 5 for the one new index this spec adds.
- No index at all on `first_seen_at`, `fetched_at`, or `priority`.

**`incident_history`**: unchanged, `(incident_id, recorded_at DESC)`
indexed, not read by this feature (the archive lists incidents, not their
per-incident change history — that stays on the detail page).

**Retention/pruning — checked directly, still absent today.**
`crates/aggregator/src/config.rs` defines retention knobs for exactly six
things: `history_retention_days` (`line_status_history`),
`daily_stats_retention_days`, `half_hourly_stats_retention_hours`,
`trust_event_backlog_retention_days`, `trains_retention_days` /
`untracked_trains_retention_days`, and
`schedule_destination_departures_retention_days`. **`incidents` and
`incident_history` are not among them.** `crates/aggregator/src/queries.rs`
has a `prune_*` function for every one of those six (`prune_history`,
`prune_daily_stats`, `prune_half_hourly_stats`,
`prune_trust_event_backlog`, `prune_trains`,
`prune_schedule_destination_departures`), every one of which is called
unconditionally from `run_cycle` in `crates/aggregator/src/main.rs`. There
is no `prune_incidents`/`prune_incident_history` function anywhere in the
crate, and no call site references either table in a pruning context.
**This confirms the 2026-08-31 spec's flagged gap is still fully open as
of this writing** — nothing has closed it since. See "Retention and
unbounded growth" below for how this spec treats that.

**Public read-route convention**: unchanged from the 2026-08-31 spec's own
finding — every read in `public_router()` is unauthenticated, and every
field this feature would expose (`summary`, `operators`,
`affectedStations`, `priority`, `isPlanned`, `isCleared`, `firstSeenAt`,
`fetchedAt`) is already public today via the by-id detail route and via
`GET /Line/{ids}/Status?detail=true`. This new list route inherits the
same posture: unauthenticated.

**Catalogue lines are in-memory, not a DB table.**
`crates/api/src/routes/lines.rs::get_line_definition` resolves a catalogue
line's station list from `app.config.lines` (loaded from the `lines/*.toml`
catalogue at startup), each entry carrying `.stations` (CRS list) and
`.operators`. Custom lines are separate, DB-backed, and **private** —
`get_line_definition` only resolves one for its authenticated owner. This
matters for Decision 2's `line` filter: resolving it against
`app.config.lines` only (never against custom lines) keeps this new route
fully public with no session/ownership check needed, and sidesteps any
question about whether an unauthenticated caller could use an incidents
search as an oracle to learn something about a private custom line's
station list.

## Decisions

### 1. Backend route: `GET /public/incidents`

New handler in the same file as the existing detail route,
`crates/api/src/routes/incidents.rs`, registered alongside it:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::get(search_incidents))
        .route("/incidents/{incidentId}", axum::routing::get(get_incident))
}
```

Full path: `GET /public/incidents` — same prefix, same file, same
`public_router()` merge point as `/public/incidents/{incidentId}`, per the
brief's own instruction to pick a path consistent with the existing route.
Unauthenticated, per the Public read-route convention above.

### 2. Filters: operator (comma-separated), line (station-overlap
approximation), date range, planned/unplanned, cleared/active, priority range

```rust
/// `#[serde(deny_unknown_fields)]` for the same reason
/// `trains.rs::TrainSearchParams` has it: a misspelled filter name must
/// 400, not silently search unfiltered. See Correction 3.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentSearchParams {
    /// Optional. Comma-separated ATOC codes, e.g. `operator=SW,VT` —
    /// matches this codebase's existing multi-value convention
    /// (`GET /Line/{ids}/Status`'s `ids`), not repeated query keys. See
    /// Correction 4. Matches if the incident's `operators` array overlaps
    /// this set at all (`operators && $1`) — an "any of" filter, not
    /// "incident is scoped to exactly this operator."
    operator: Option<String>,
    /// Optional. A single catalogue line id (`app.config.lines`, never a
    /// custom line — see Current relevant state). Resolved server-side to
    /// that line's own station list, then applied as
    /// `affected_stations && $stations` — see the approximation note
    /// below. An id that doesn't resolve in `app.config.lines` is a `400`
    /// ("unknown line"), not a 404 or a silently-empty result — this is a
    /// malformed filter value, the same posture `trains.rs` takes for
    /// every other unrecognized filter input.
    line: Option<String>,
    /// Optional, RFC3339. Inclusive lower bound on `first_seen_at` (this
    /// app's own ingest clock, not any Knowledgebase-supplied timestamp —
    /// same reason the detail page prefers it, see the 2026-08-31 spec).
    from: Option<String>,
    /// Optional, RFC3339. Inclusive upper bound on `first_seen_at`.
    to: Option<String>,
    /// Optional. `true` = planned works only, `false` = unplanned only,
    /// omitted = either.
    planned: Option<bool>,
    /// Optional. `true` = cleared only, `false` = active (not yet
    /// cleared) only, omitted = either. Deliberately NOT a hidden default
    /// filter on the backend — see "Avoiding the spamminess research
    /// document's lesson" below for why an explicit, visible filter is
    /// the right shape here, not an implicit one.
    cleared: Option<bool>,
    /// Optional. Inclusive lower bound on the raw `priority` integer.
    /// No documented "major"/"minor" mapping exists (Correction 2) — this
    /// is a raw numeric range over an unexplained feed value, not a
    /// severity filter with real semantic tiers.
    priority_min: Option<i32>,
    /// Optional. Inclusive upper bound. A `400` if both bounds are given
    /// and `priority_min > priority_max` — same "malformed input 400s"
    /// posture as everywhere else in this file.
    priority_max: Option<i32>,
    /// Optional page size, 1..=`MAX_INCIDENT_SEARCH_LIMIT` (mirrors
    /// `trains.rs`'s `MAX_SEARCH_LIMIT`/`DEFAULT_SEARCH_LIMIT` shape and
    /// values exactly — no reason for this route to pick different
    /// numbers). Over-large is clamped, not rejected; non-positive or
    /// unparseable is a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`.
    after: Option<String>,
}
```

**The `line` filter is a named approximation, not matcher parity — this
must be visible to the user, not just documented here.** Per Correction 1,
this reuses the matcher's own `StationHit` tier only (an incident matches
if it shares at least one affected station with the line), and skips
`ExclusiveSegment`/`SharedSegment` (shared-trunk incidents that never
literally list the line's own stations in `affected_stations`, only a
station on the shared segment — which *is* one of the line's own stations,
so this case is actually still caught), `KeywordOnly` (an incident matched
by route-name keywords in its free text, with no station overlap at all —
**not** caught), and `OperatorOnly` (matched purely because it shares an
operator with the line, with no station or keyword hit — **not** caught).
So this filter will under-count relative to what a line's own live status
page might have shown while an incident was active (a `KeywordOnly` or
`OperatorOnly` match on that line is invisible to it), and can very rarely
over-count in the opposite direction (an incident whose only realistic
scope was a shared *segment* elsewhere, but which happens to also
independently list a CRS this line calls at, would still show up under
this line — genuinely correct, not a bug, since station overlap on that
one CRS is real, just not the same as saying the *whole* incident affects
this whole line). Decision 6 requires the frontend to word this filter as
"Incidents affecting stations on this line," not "Incidents on this
line," specifically so it doesn't imply a promise this data can't back.

### 3. Priority stays a raw, unexplained range filter — not resolved into a severity tier here

Per Correction 2, this spec does not invent a severity taxonomy `priority`
doesn't have. `priority_min`/`priority_max` are plain integer bounds; the
frontend (Decision 6) labels the control honestly ("Priority (raw feed
value — no documented meaning)") rather than pretending it maps to
Major/Minor. This is consistent with, not a resolution of, the 2026-08-31
spec's own Open Question #1.

### 4. Pagination: keyset cursor on `(first_seen_at DESC, incident_id DESC)`, matching `trains_search` exactly

Per Correction 3, this is not a new pagination design — it is the existing
one, applied to a new table. `MAX_INCIDENT_SEARCH_LIMIT` /
`DEFAULT_INCIDENT_SEARCH_LIMIT` are declared as their own constants in
`incidents.rs` (not shared with `trains.rs`'s, since the two routes have
no reason to be coupled), but with the **same numeric values**
`trains.rs` uses today, absent any reason for this feature's page size to
differ.

```rust
struct IncidentSearchCursor {
    first_seen_at: chrono::DateTime<chrono::Utc>,
    incident_id: String,
}

/// base64url(no-pad) of `"{RFC3339 first_seen_at}|{incident_id}"` —
/// same opaque-token shape as `trains.rs::encode_cursor`. Not signed, for
/// the same reason: it names a public row on an unauthenticated route.
fn encode_cursor(cursor: &IncidentSearchCursor) -> String { /* ... */ }

/// Inverse. A malformed cursor is a `400`, never silently dropped —
/// dropping it would restart the caller at page 1 while their UI appended
/// the response as page 2, duplicating rows. Same reasoning as
/// `trains.rs::decode_cursor`.
fn decode_cursor(raw: &str) -> Result<IncidentSearchCursor, (StatusCode, String)> { /* ... */ }
```

New query function in `crates/api/src/data/queries.rs`, modelled directly
on `search_schedule_calling_point_departures`'s existing
"`fetch = limit + 1`, one extra row to detect `has_more`,
`($n::type IS NULL OR condition)` per optional filter, keyset tuple
comparison in `WHERE`, matching `ORDER BY`" shape:

```sql
SELECT incident_id, summary, operators, affected_stations, priority,
       is_planned, is_cleared, first_seen_at, fetched_at
FROM incidents
WHERE ($1::text[]        IS NULL OR operators && $1)
  AND ($2::text[]        IS NULL OR affected_stations && $2)
  AND ($3::boolean       IS NULL OR is_planned = $3)
  AND ($4::boolean       IS NULL OR is_cleared = $4)
  AND ($5::integer       IS NULL OR priority >= $5)
  AND ($6::integer       IS NULL OR priority <= $6)
  AND ($7::timestamptz   IS NULL OR first_seen_at >= $7)
  AND ($8::timestamptz   IS NULL OR first_seen_at <= $8)
  AND ($9::timestamptz IS NULL
       OR (first_seen_at, incident_id) < ($9, $10))
ORDER BY first_seen_at DESC, incident_id DESC
LIMIT $11
```

(`$1` is the parsed `operator` list; `$2` is the resolved line's station
list, or `NULL` when no `line` filter was given; `$9`/`$10` are the
decoded cursor's two fields, `NULL` on the first page.) No `IncidentRow`
reuse from the detail route: a list row is deliberately lighter — see
Decision 7 for why `description`/`validity_periods` are excluded here.

**No `is_cleared`-only default filter, no implicit "recent" floor baked
into the query itself.** Every filter above is opt-in; an unfiltered
request returns every incident ever ingested, newest-first, one page at a
time. Any "start with a sane recent window" behavior is a **frontend**
default (Decision 6), not a backend one — the backend route answers
exactly what it's asked, matching every other search route in this
codebase (`trains_search` doesn't default to "today only" either; that's
the frontend's `TrainSearchForm` initial state).

### 5. One new index: `incidents_first_seen_at_id` — everything else is already GIN-covered

Per Correction 5 / the brief's ask #5: the operator and line/station
filters need **no new index** — `incidents_operators_gin` and
`incidents_affected_stations_gin` already support the `&&` overlap
operator this query uses, unchanged from how the detail page's matcher
already relies on the equivalent GIN indexes existing.

What's genuinely missing is an index that supports this feature's own
**default order and keyset cursor** — `incidents_active`'s partial index
has no orderable column, and nothing else touches `first_seen_at` at all.
New migration:

```sql
CREATE INDEX incidents_first_seen_at_id
    ON incidents (first_seen_at DESC, incident_id DESC);
```

This also directly serves the `from`/`to` date-range filter (leading
column of the same index). **Deliberately no new index for
`priority_min`/`priority_max` or `is_planned`/`is_cleared`** — following
this repo's own established posture (the 2026-08-31 spec's
`lines_currently_reporting_incident` and this table's own
`line_status.source` column both went without an index on the stated
reasoning "cheap at this scale, revisit only if it measurably isn't"): a
low-selectivity boolean or a range over a low-cardinality integer column,
scanned via the new `first_seen_at` index's row order, is not expected to
be a measured problem at today's data volume. No row count for `incidents`
was directly measured for this spec (see "Retention and unbounded growth"
below for why that's a real gap, not just a convenient assumption) — this
is a reasoned-cheap call, not a benchmarked one, exactly the same caveat
the 2026-08-31 spec attached to its own no-new-index calls.

### 6. Frontend: `/incidents` page, filter form, results list — following `TrainSearchForm`'s established client-search pattern, not a server-searchParams one

**This is the load-bearing frontend precedent, and it is not the
`HistoryRangePicker`/server-searchParams pattern** used by
`/lines/[id]/history`. The real precedent for "an interactive filter form
over a cursor-paginated backend search route" already exists in this
codebase: `frontend/components/TrainSearchForm.tsx` (backing `/trains`),
which is a **Client Component** holding its own filter state, calling
`fetch('/api/trains/search?...')` (the same-origin proxy — see below for
why this route also needs it), storing `{ rows, nextCursor }` in
`useState`, and rendering a "Load more" button that appends the next
page's rows to the existing list rather than replacing them or navigating.
This spec's `/incidents` page follows that shape exactly, not the
range-picker's URL-driven server-refetch shape, because the filter set
here (six independent optional filters) is a closer match to
`TrainSearchForm`'s multi-filter interactive search than to the
range-picker's single from/to control.

**`frontend/app/incidents/page.tsx`** — thin shell, mirrors
`app/trains/page.tsx` structure: reads initial filter values out of
`searchParams` (so a filtered link is shareable, matching `/trains?station=...`),
passes them as `initial*` props into the client form, same pattern as
`TrainsPage` → `TrainSearchForm`.

**New `frontend/components/IncidentSearchForm.tsx`** (client component):

- **Filter controls**:
  - Operator: Mantine `MultiSelect`, options from `getAllTocs()` (already
    fetched this same way on `/lines`'s page — `frontend/app/lines/page.tsx`'s
    `getAllTocs().catch(() => [])` "hour-cached reference data, degrades to
    an empty list on failure" pattern, reused verbatim here). Selected
    codes joined with `,` into the `operator` query param — mirrors the
    backend's comma-separated convention (Correction 4), not one param per
    selection.
  - Line: Mantine `Select` (single-value), options from `getAllLines()`
    (already fetched the same way on `/lines`'s page) — **catalogue lines
    only**, matching Decision 2's backend scoping; nothing in this list
    ever includes a custom line, so there's no way to even attempt
    filtering by one.
  - Date range: reuses the exact `DatePickerInput` + `new Date(start).toISOString()`
    pattern `HistoryRangePicker.tsx` already established, feeding `from`/
    `to` as RFC3339 timestamps — not a new date-handling convention.
  - Planned/unplanned: Mantine `SegmentedControl`, three options
    (`All` / `Planned work` / `Real-time`), mapping to `planned` unset/
    `true`/`false`.
  - Active/cleared: Mantine `SegmentedControl`, three options (`All` /
    `Active` / `Cleared`), mapping to `cleared` unset/`false`/`true`.
  - Priority: two plain `NumberInput`s labeled "Priority (raw feed value —
    meaning undocumented)" with a short `Text c="dimmed"` caveat below
    them, matching Decision 3's stance directly — this UI must not imply a
    severity scale that doesn't exist.
  - **Default filter state on first load (no `searchParams`): `from` = 30
    days ago, everything else unset.** See "Avoiding the spamminess
    research document's lesson" below for why an unfiltered-by-default
    view is the wrong initial state for this specific page, and why 30
    days (not `/lines/[id]/history`'s 7-day default) is the right initial
    window for a whole-network view. Quick-preset buttons (`7 days` /
    `30 days` / `90 days` / `All time`) mirror `HistoryRangePicker`'s own
    preset-button pattern exactly, including its filled-vs-light selected
    state.
- **Results**: a `Stack` of rows (not a dense table — a summary line, two
  badge rows, and a timestamp per incident is too much for a data-table
  column model, and this mirrors `IssueList`'s own accordion-row density
  rather than `AllLinesTable`'s dense grid). Each row:
  - `summary` rendered as a `TextLink` to `/incidents/${incidentId}` —
    reuses the existing `TextLink` component and the existing detail page,
    no new link-building logic (the raw `incidentId` this route returns is
    already the correct path segment, per the 2026-08-31 spec's Decision
    1 — no `knowledgebase-incident-` prefix stripping needed here, unlike
    `DisruptionDetail.tsx`, because this route was never built from a
    `Disruption.source` string in the first place).
  - A `Badge` for Planned/Real-time (`isPlanned`), matching the detail
    page's own badge.
  - A `Badge` for Active/Cleared (`isCleared`) — the archive's whole point
    is browsing history, so a cleared incident is a normal, expected row,
    not something hidden or de-emphasized by default (see below).
  - Operator and affected-station badges, reusing whatever badge-list
    component `DisruptionDetail.tsx`/`IssueList.tsx` already render for
    the equivalent fields (no new badge-list component).
  - `firstSeenAt`, formatted via the existing `formatDateTime` helper,
    low-emphasis styling — same convention `IncidentDetail`'s own
    "First seen" line uses.
  - **No inline description/HTML rendering in the list** — see Decision 7.
- **"Load more"**: identical mechanics to `TrainSearchForm`'s own
  `handleLoadMore` — append `body.results` to existing `rows` state,
  replace `nextCursor`, disable/spin the button while a request is in
  flight, don't touch it at all once `nextCursor` comes back `null`.
- **Empty state**: "No incidents match these filters" — a normal, expected
  outcome (e.g. a narrow date range with nothing in it), not an error;
  matches this codebase's established "an empty result set is not a
  failure" posture (`trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array`).
- **Error state**: any non-2xx or network failure → a plain inline error
  message with a retry affordance, same shape as `TrainSearchForm`'s own
  `results === 'error'` branch — no new error-handling pattern introduced.

**Why this goes through `/api/incidents` (the same-origin proxy), not a
server-side `fetchJson` straight to `API_BASE_URL`.** Unlike
`/incidents/[id]`'s own `getIncident` (a Server Component read, no proxy
needed — see the 2026-08-31 spec's own reasoning), this page's search is
**interactive**: the filter form re-queries on every change and "Load
more" click without a full page navigation, exactly like
`TrainSearchForm`. That means the request originates in the browser, which
cannot read `API_BASE_URL` (server-only env var). `resolveTargetPath` in
`frontend/app/api/[...path]/route.ts` already maps any path other than its
one special-cased `Train/...` prefix to `/public/${path}` — `/api/incidents`
resolves to `/public/incidents` automatically, and the pathname check
(`starts with /public/ or /Train/`) already passes. **No proxy allowlist
change is needed for this feature.**

**Linking in from elsewhere**: add one `TextLink` to `/incidents` from the
main nav (wherever `/trains`/`/track` links already live) and from the
`/lines` page's header area (next to the existing "New custom line" link)
— this is a real, user-reachable page, not a dead end, matching this
repo's own repeatedly-stated posture (per the 2026-08-31 spec's own
framing: "a page built and left unreachable" is the thing to avoid). No
change is made to `DisruptionDetail.tsx`'s existing incident link (it
still points at the single-incident detail page, which remains the
correct target for "tell me about the thing on my line's own status
page" — the archive is a separate, complementary entry point, for
"show me things across the network," not a replacement).

### 7. List rows exclude `description`/`validityPeriods`/history — deliberately lighter than the detail response

The detail route's response (2026-08-31 spec) includes raw HTML
`description`, every `validityPeriods` entry, `currentlyAffectsLines`, and
the full `history` array — all reasonable for a single incident's detail
page, all wasteful for a list that may render dozens of rows per page.
This route's `IncidentSummaryRow` carries only: `incidentId`, `summary`,
`operators`, `affectedStations`, `priority`, `isPlanned`, `isCleared`,
`firstSeenAt`, `fetchedAt`. Two consequences worth being explicit about:

- **No HTML sanitization is needed anywhere in this feature's frontend
  code** — `summary` is already plain text everywhere else it's rendered
  (the detail page's own heading, per the 2026-08-31 spec's Decision 6),
  unlike `description`, which is the one field that needs
  `sanitizeDescription`. This route never returns `description` at all, so
  `frontend/lib/sanitizeHtml.ts` is not touched by this feature.
- A user who wants the full description has to open the detail page —
  which is exactly the linking model this feature is built around (list →
  detail), not a design gap.

## Avoiding the spamminess research document's lesson

`docs/superpowers/specs/2026-09-02-line-history-list-spamminess-research.md`
investigated why `/lines/[id]/history`'s Timeline tab reads as "spammy,"
and its root cause was specific to that feature's data shape: unnormalized,
per-poll-cycle-varying LDBWS-sample-derived `reason` text defeating both
the write-side "insert only if changed" guard and the read-side
`collapseDay` grouping, producing many near-duplicate entries for what was
really one ongoing situation. **That specific mechanism does not carry
over to this feature** — this archive lists rows straight from the
`incidents` table, which is upserted one row per real `incident_id` (no
per-poll-cycle proliferation: `upsert_incidents`'s `ON CONFLICT
(incident_id) DO UPDATE` means there is exactly one `incidents` row per
real incident, full stop, and a materially-changed one gets a new
`incident_history` row rather than a new `incidents` row — this feature
never touches `incident_history` at all). So there is no reason-text-churn
identity problem here to inherit.

What *does* carry over is the more general lesson that document's own
"Possible directions" section names but frames independently of its
primary root cause: **direction 5, "UI-side collapsing/pagination as a
backstop... for lines that legitimately see many distinct real
incidents... even a fully-normalized identity scheme will still show a
real, possibly long, list."** A cross-network archive is exactly that
case, at a larger scale than any single line — this is the one part of
that document's findings genuinely applicable here, and this spec answers
it two ways, both already built into the decisions above rather than
bolted on after:

1. **Cursor pagination with a bounded page size** (Decision 4) — the
   backend never hands back an unbounded response regardless of how many
   incidents match a given filter set.
2. **A non-empty default filter on first load** (Decision 6: 30 days,
   not "all time") — an *unfiltered* archive of every Knowledgebase
   incident this app has ever ingested, with no date floor, is the direct
   equivalent of the Timeline tab's "87 incidents… newest first" wall the
   research document diagnosed as unhelpfully spammy, just multiplied
   across every line at once instead of one. The fix here is upstream of
   pagination: don't let the *default* view be "everything," while still
   leaving "everything" one filter-clear away for a user who genuinely
   wants the full archive (an explicit `All time` preset, not a hidden
   ceiling).

## API/type contract

```ts
// frontend/lib/types.ts additions

/** One row from `GET /public/incidents`. Deliberately lighter than
 * `IncidentDetail` (no description, no validityPeriods, no history, no
 * currentlyAffectsLines) -- see Decision 7. */
export interface IncidentSummary {
  incidentId: string;
  summary: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string;   // RFC3339
}

export interface IncidentSearchResponse {
  results: IncidentSummary[];
  nextCursor: string | null;
}
```

`IncidentSearchForm.tsx` calls `fetch('/api/incidents?...')` directly
(browser-initiated, through the proxy), the same way `TrainSearchForm.tsx`
calls `/api/trains/search` — **no new function is added to
`frontend/lib/api.ts`** for this feature's search itself, since
`lib/api.ts` is this codebase's convention for **server-side**
`fetchJson` calls (every existing export there is `async function get*`
called from a Server Component), and this is deliberately a client-side
interactive search instead (Decision 6). `getAllTocs()`/`getAllLines()`
(both already exported from `lib/api.ts`) are reused as-is for the filter
dropdowns' option lists, fetched server-side in `app/incidents/page.tsx`
and passed down as props — the same "reference data fetched once by the
page, passed to the client form" shape `TrainsPage` doesn't need (it has
no dropdown reference data) but `AllLinesPage`/`AllLinesTable` already
establishes for `tocs`.

## Architecture

```
┌───────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                         │
│                                                                          │
│  app/incidents/page.tsx        NEW -- thin Server Component shell,     │
│                                   fetches getAllTocs()/getAllLines()    │
│                                   for filter options, reads searchParams │
│                                   for shareable initial filter state,   │
│                                   renders IncidentSearchForm            │
│                                                                          │
│  components/IncidentSearchForm.tsx   NEW -- Client Component, mirrors  │
│                                   TrainSearchForm.tsx's fetch/useState/ │
│                                   Load-more shape exactly               │
│                                                                          │
│  lib/types.ts   + IncidentSummary, IncidentSearchResponse              │
│                                                                          │
│  app/incidents/[id]/page.tsx, components/DisruptionDetail.tsx  UNCHANGED│
└──────────────────────────┬──────────────────────────────────────────────┘
     browser fetch, same-origin proxy (no allowlist change needed)
                            ▼
┌───────────────────────────────────────────────────────────────────────┐
│ frontend/app/api/[...path]/route.ts   UNCHANGED -- /api/incidents      │
│   already resolves to /public/incidents under the existing catch-all  │
└──────────────────────────┬──────────────────────────────────────────────┘
                            ▼
┌───────────────────────────────────────────────────────────────────────┐
│ api crate                                                                │
│  routes/incidents.rs   + GET /incidents (search_incidents), alongside  │
│                          the existing GET /incidents/{incidentId}      │
│  data/queries.rs       + search_incidents (keyset-paginated, dynamic   │
│                          optional filters -- modelled on               │
│                          search_schedule_calling_point_departures)     │
│  routes/lines.rs / app.config.lines  READ ONLY -- resolves `line` to a │
│                          catalogue line's station list, never a custom │
│                          line                                          │
└──────────────────────────┬──────────────────────────────────────────────┘
                            ▼
┌───────────────────────────────────────────────────────────────────────┐
│ Postgres migration: + incidents_first_seen_at_id index (Decision 5).   │
│ incidents_operators_gin / incidents_affected_stations_gin reused       │
│ as-is, no change. incident_history NOT read by this feature.           │
└───────────────────────────────────────────────────────────────────────┘
```

## Retention and unbounded growth

**Scoped out of this spec, named explicitly, per the task's own allowance
for that outcome — not silently ignored.**

- **The gap is real and still open** (verified above): no pruning job
  exists for `incidents`/`incident_history` today, unlike every other
  time-series table this app writes.
- **This feature does not make the gap worse in a way that blocks
  shipping it.** The new route is cursor-paginated with a bounded page
  size (Decision 4) and its filters are backed by existing GIN indexes
  plus one new btree-style composite index (Decision 5) — its own
  per-request cost does not scale with total table size the way an
  unindexed full scan would. Whether `incidents` has ten thousand rows or
  ten million, one page of this route's response costs roughly the same.
- **What this feature genuinely does is make the *existence* of that
  unbounded table more visible to a real person**, exactly as the task
  brief anticipated: today, an ever-growing `incidents` table is purely an
  internal storage/query-planning concern nobody outside the aggregator
  crate has reason to think about. Once `/incidents` ships, "how far back
  does this archive go" becomes a real, user-facing question with a real,
  user-visible answer ("all the way to whenever this deployment started
  ingesting Knowledgebase incidents, however long ago that was, with no
  cutoff") — which is a materially different situation than before this
  feature existed, even though this feature's own query performance
  doesn't degrade because of it.
- **No row count for `incidents` was measured for this spec** (no access
  to a live database with production-scale data was exercised here,
  mirroring the same caveat the 2026-08-31 spec and the spamminess
  research document both explicitly flagged about their own unmeasured
  assumptions) — so "is this a problem yet" is reasoned qualitatively, not
  benchmarked. Given a UK-wide Knowledgebase feed's real-world incident
  rate (almost certainly low thousands to tens of thousands per year, not
  millions), and that every other read this feature performs is bounded
  and indexed, this spec's own judgment is that **current volume does not
  block shipping this feature**, but that judgment should not be read as
  "the retention gap is fine" — see the next point.
- **Recommendation, not a design**: this spec explicitly recommends that
  designing real pruning/retention for `incidents`/`incident_history` (its
  own dedicated spec, following the shape
  `docs/superpowers/plans/2026-09-01-ldbws-data-retention.md` already
  established for the other six tables) should move up in priority now
  that this feature exists, precisely because the 404-vs-pruned ambiguity
  the 2026-08-31 spec flagged in its own Open Question #3 ("this page's
  404 behavior for a very old incident is currently indistinguishable
  from 'never existed'") becomes something a real user browsing an
  archive is now likely to actually hit by clicking an old row, rather
  than something only reachable via a stale/shared link. **This spec does
  not design that pruning job** — it is a separate, unscoped feature, the
  same way the 2026-08-31 spec scoped it out of the detail page.

## Explicitly out of scope

- **Full-text search over `summary`/`description`.** The brief's filter
  list (operator/line/date/severity/planned) never asked for it, no
  `tsvector`/`pg_trgm` index exists on either column today, and adding one
  is a separately-scoped indexing decision, not a natural extension of the
  structured filters this spec designs.
- **Retroactively re-running the real matcher** (`ExclusiveSegment`/
  `SharedSegment`/`KeywordOnly`/`OperatorOnly`) to build a historically
  exact "which lines did this incident really affect" filter. Per
  Correction 1, this is aggregator-side logic operating on live inputs
  (the segment registry, each line's current catalogue definition, live
  keyword/operator dictionaries) that isn't meaningfully re-derivable for
  an arbitrary past incident without also snapshotting all of those
  inputs as they were at the time — a much larger feature than this spec's
  brief asked for. Decision 2's station-overlap approximation is the
  answer this spec gives instead, with its limitation stated plainly in
  the UI copy (Decision 6), not hidden.
- **Filtering by a private custom line.** Decision 2 resolves `line` only
  against `app.config.lines` (catalogue lines); a custom line id simply
  doesn't resolve, the same 400 an unknown/mistyped id gets. No
  authentication is added to this route for this reason — see Current
  relevant state's note on why that would also avoid a minor
  information-oracle concern.
- **Pruning/retention for `incidents`/`incident_history`.** See "Retention
  and unbounded growth" above — named as a real, currently-unaddressed gap
  this feature surfaces more visibly, with a concrete recommendation to
  design it as its own follow-up spec, but not designed here.
- **A resolved answer to the 2026-08-31 spec's Open Question #1**
  (whether/how to give `priority` real semantic meaning). This spec
  surfaces `priority` as a raw range filter, consistent with that
  question staying open, not an attempt to close it.
- **Editing, deleting, or any write path on this route.** `GET` only,
  matching the detail route and every other Knowledgebase-sourced,
  poller-written reference table in this app.
- **Total result counts.** Matching `trains_search`'s own convention, the
  response carries `results`/`nextCursor` only — no `totalCount`, which a
  keyset-paginated query can't cheaply compute anyway without a separate
  `COUNT(*)` query this spec sees no strong need for.
- **Any change to `/lines/[id]/history`'s Timeline tab or its underlying
  data.** This is a genuinely separate feature reading a genuinely
  separate table (`incidents`, not `line_status_history`); the spamminess
  research document's own root cause and fixes are untouched by anything
  here — see "Avoiding the spamminess research document's lesson" above
  for exactly what does and doesn't carry over.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`/
`*.test.ts`, Vitest for the frontend; `#[cfg(test)]` module-per-file, plus
`#[ignore]`d live-database integration tests run explicitly, for the Rust
side):

- **`crates/api/src/data/queries.rs`** (mirroring
  `search_calling_point_*`'s own test suite style):
  - `search_incidents` with no filters returns every seeded row,
    newest-`first_seen_at`-first, ties broken by `incident_id` descending.
  - Each filter in isolation: `operator` (comma-parsed, overlap not exact
    match — an incident with `{VT, SW}` matches a request for `SW` alone),
    `line` (resolves via a stubbed/fixture catalogue and matches on
    station overlap only — including the explicit regression case "an
    incident matched only via `KeywordOnly`/`OperatorOnly` in the real
    matcher is correctly absent from this filter's results," proving the
    approximation's documented limitation is real and intentional, not an
    accidental gap), `from`/`to` (inclusive boundaries), `planned`,
    `cleared`, `priority_min`/`priority_max`.
  - Filters combined (AND semantics across different filter kinds).
  - Keyset pagination: pages without gaps or repeats across a
    `first_seen_at` tie, mirroring
    `search_calling_point_keyset_cursor_pages_without_gaps_or_repeats_and_breaks_ties_on_train_uid`
    directly.
  - No matches at all → `Ok` with an empty `Vec`, never a `None`/404 shape
    — unlike `search_schedule_calling_point_departures`, there is no
    "unpublished day" concept here to distinguish from "published but
    empty," so this route never 404s on a well-formed, merely-empty query.
- **`crates/api/src/routes/incidents.rs`**:
  - 400 on an unknown `line` id, on `priority_min > priority_max`, on a
    malformed `after` cursor, and on an unrecognized query parameter name
    (mirroring `trains_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it`
    directly).
  - `limit` clamping/rejection behavior, mirroring
    `trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one`.
  - `nextCursor` explicit `null` on the last page (never omitted),
    mirroring `trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one`.
  - Full response shape camelCase, no leaked snake_case field names —
    same category of regression test `to_incident_detail_json`'s own
    tests already run for the detail route.
- **`frontend/components/IncidentSearchForm.test.tsx`** (mirroring
  `TrainSearchForm.test.tsx`'s structure):
  - Builds the correct query string from a given filter combination,
    including the comma-joined `operator` value.
  - "Load more" appends rows rather than replacing them, and is hidden
    once `nextCursor` is `null`.
  - Empty-results and error states render their respective messages, not
    a blank screen or a thrown error.
  - Default initial state (no `searchParams`) applies the 30-day `from`
    floor described in Decision 6.
- **`frontend/app/incidents/page.test.tsx`** (or equivalent): renders with
  `getAllTocs()`/`getAllLines()` fixture data, passes `searchParams`-derived
  initial values through to the form correctly, degrades sanely if either
  reference-data fetch fails (matching `AllLinesPage`'s own
  `.catch(() => [])` posture).

## Self-review notes

- Placeholder scan: no `TODO`/`TBD`/bracketed placeholder text remains.
- Every decision that names a specific file, function, index, or constant
  was checked directly against this repository's current state on
  2026-09-12 (`crates/api/src/routes/incidents.rs`,
  `crates/api/src/data/queries.rs`, `crates/aggregator/src/config.rs`,
  `crates/aggregator/src/queries.rs`, `crates/aggregator/src/main.rs`,
  `crates/aggregator/src/matcher.rs`, `crates/api/src/routes/trains.rs`,
  `crates/api/src/routes/reference.rs`, `crates/api/src/routes/lines.rs`,
  `frontend/components/TrainSearchForm.tsx`,
  `frontend/app/trains/page.tsx`, `frontend/app/lines/page.tsx`,
  `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`,
  `frontend/app/api/[...path]/route.ts`, and every migration under
  `crates/api/migrations/` touching `incidents`/`incident_history`).
- Internal consistency: the `line` filter's documented limitation
  (Correction 1 / Decision 2) is carried through consistently into the
  frontend copy (Decision 6), the out-of-scope list, and a named test
  case, rather than only appearing once.
- Scope check against the brief: operator, line, date range, and
  planned/unplanned are all designed as real filters; "severity" is
  addressed by explaining why it can't be a real severity filter and
  designing the honest substitute (raw priority range) instead of
  quietly dropping it. Pagination shape matches this codebase's own
  existing convention rather than introducing a new one. The retention
  question is answered with a stated decision (scope out, ship as-is,
  name the risk, recommend a follow-up) rather than left ambiguous.
