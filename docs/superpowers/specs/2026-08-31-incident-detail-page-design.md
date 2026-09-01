# Design: Incident Detail Page

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(closest precedent — a real frontend-plus-backend feature design, no
implementation plan included; that is a separate, later step in this
repo's process).

## Goal

Every Knowledgebase-sourced disruption already carries a stable identifier
(`Disruption.source`) end to end through this app, but there is no page a
user can land on that shows just that one incident — its full detail, and,
uniquely, how its own record has changed since it was first seen. Today the
only place an incident's detail is ever shown is inline, buried inside an
`Accordion` row on a line's or station's issue list, re-rendered fresh
(and identically) everywhere the same incident happens to appear. This
spec designs a dedicated `/incidents/[id]` page, the backend route it
reads from, and — the explicit, non-negotiable part of the brief — exactly
where existing UI gets a real link to it, not just a page built and left
unreachable.

## Corrections to the brief's assumptions (recorded for posterity)

Following the ticket-tracking-frontend spec's own "Corrections" precedent:
direct inspection of the code turned up things the brief didn't quite get
right, materially affecting the design below.

1. **The brief's claim that "the LDBWS-inferred and TfL paths never
   construct a `Disruption`" is false — both paths do construct one, just
   never one backed by a real `incidents` row.** Three call sites build a
   `common::Disruption` (grepped exhaustively across `crates/`, excluding
   tests):
   - `crates/aggregator/src/aggregation.rs:136` (`status_from_incident`,
     the Knowledgebase path) — `source:
     Some(format!("knowledgebase-incident-{}", incident.incident_id))`.
     **This is the only one backed by a real, persisted `incidents` row.**
   - `crates/aggregator/src/aggregation.rs:799` (`infer_from_samples`, the
     LDBWS-inferred path) — `source: Some("ldbws-sampling".to_string())`,
     a **literal constant**, not an id, attached to `DataQuality::LdbwsInferred`.
   - `crates/poller-tfl/src/schema.rs:136` (`map_status`, the TfL path) —
     `source: Some(format!("tfl-line-status-{line_id}"))`, keyed off the
     **TfL line id**, not an incident id, attached to `DataQuality::Tfl`.

   So the real rule this spec's linkability check must encode is not
   "does `data_quality` say Knowledgebase/Planned" (though that happens to
   be an equivalent proxy, since only `status_from_incident` ever sets
   those two `DataQuality` values) but **"does `disruption.source` start
   with the literal prefix `knowledgebase-incident-`"** — the only
   provenance string in this codebase that names a real `incidents.incident_id`.
   Both the LDBWS and TfL constants would parse as syntactically-plausible
   ids if naively string-split, so the check must be a genuine prefix
   match, not "source is present" or "source contains a dash."
2. **Two unrelated concepts share the word "source", and conflating them
   would silently break the cross-reference query in Decision 3.**
   `crates/api/migrations/20260822120000_line_status_source.sql` added a
   top-level `line_status.source` **column** (`'aggregator' | 'tfl'` —
   which *service* wrote the row). `common::Disruption.source` is a
   **JSONB field nested inside `line_status.statuses`**, one array element
   per status, and is a completely different value (the incident/inference
   provenance string this spec is about). Any query written against this
   table for Decision 3 must reach into the JSONB (`statuses -> ... ->>
   'source'`), never the `line_status.source` column, or it will silently
   match nothing.
3. **`affected_routes` cannot be reconstructed from the `incidents` row
   alone — it is not incident-level data.** `status_from_incident` computes
   it via `routes_from_stations(m.line, &affected_stations)`
   (`aggregation.rs:657`), which sorts the incident's `affected_stations`
   into **that specific line's own station order** — a genuinely per-line
   derivation, re-computed independently for every line the incident
   matches. The `incidents` table itself stores only `affected_stations
   TEXT[]` (CRS codes) — no route/from-to data at all. This bounds what
   the detail page can show: a real `affectedStations` list, but no
   single, line-independent "affected routes" value. See Decision 2.

## Current relevant state (verified 2026-08-31)

**`incidents` table**, current shape after every migration through
`20260822090000_incident_extraction_periods.sql` (grepped
`crates/api/migrations/` for every later touch — `20260822120000` only
touches `line_status`, and nothing after that touches `incidents`):

```
incident_id                    TEXT PRIMARY KEY
summary                        TEXT NOT NULL
description                    TEXT NOT NULL
operators                      TEXT[] NOT NULL          -- ATOC codes
affected_stations               TEXT[] NOT NULL          -- CRS codes
priority                       INTEGER NOT NULL          -- raw RDM int, no documented "major"/"minor" enum
validity_periods               JSONB NOT NULL DEFAULT '[]'
is_planned                     BOOLEAN NOT NULL DEFAULT FALSE
is_cleared                     BOOLEAN NOT NULL DEFAULT FALSE
fetched_at                     TIMESTAMPTZ NOT NULL DEFAULT NOW()
first_seen_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW()  -- this app's own clock, immune to RDM going stale
-- NLP-extraction columns, written by `enricher`, read only by `aggregator` --
-- not part of this page's design; see Decision 2's note.
source_text_hash, extracted_category, extracted_resolution_status,
extracted_schedule_window, extracted_eta, extraction_confidence,
extraction_model_version, extracted_at,           -- deprecated flat shape, no longer written
extracted_severity, extracted_severity_confidence, -- deprecated flat shape, no longer written
extracted_periods              JSONB                      -- current shape, Vec<ExtractionPeriod>
```

Indexes: `incidents_affected_stations_gin` (GIN over `affected_stations`),
`incidents_operators_gin` (GIN over `operators`), `incidents_active`
(partial, `WHERE NOT is_cleared`) — none of these help a by-id lookup, but
`incident_id` is the primary key, so a plain `WHERE incident_id = $1` is
already an index lookup.

**`incident_history` table** — one append-only row per detected change
(`crates/api/src/data/queries.rs::incident_changed`: new, or
summary/description/`validity_periods` differ from the stored row):

```
id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY
incident_id   TEXT NOT NULL
summary       TEXT NOT NULL
description   TEXT NOT NULL
operators     TEXT[] NOT NULL
affected_stations TEXT[] NOT NULL
priority      INTEGER NOT NULL
validity_periods JSONB NOT NULL DEFAULT '[]'
is_planned    BOOLEAN NOT NULL
is_cleared    BOOLEAN NOT NULL
recorded_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
```

Indexed `(incident_id, recorded_at DESC)` — exactly the access pattern a
detail page's timeline needs, no new index required.

**`common::Disruption`** (`crates/common/src/lib.rs:309-321`), no
`#[serde(rename_all)]`, so its JSONB-embedded field names (inside
`line_status.statuses`) are literally `category`, `description`,
`affected_stops`, `affected_routes`, `source`:

```rust
pub struct Disruption {
    pub category: String,          // "RealTime" | "PlannedWork" | "Information"
    pub description: String,
    pub affected_stops: Vec<String>,
    pub affected_routes: Vec<AffectedRoute>,
    pub source: Option<String>,    // see Correction 1 for the three real formats
}
```

**No existing route serves one incident by id** (confirmed:
`crates/api/src/routes/` has no `incidents.rs`, and
`crates/api/src/data/queries.rs` has no `incident_by_id`/similar — the
only incidents-table reads that exist today are internal to
`upsert_incidents`' own diff check). The closest routes,
`crates/api/src/routes/line_status.rs`, only ever return a disruption
embedded inside a line's/station's status array.

**Frontend rendering surfaces — the complete inventory** (grepped
`frontend/` for `Disruption`/`DisruptionDetail`, every match reviewed):

- **`frontend/components/DisruptionDetail.tsx`** — the sole component that
  renders a `Disruption`'s content (description, affected stops, affected
  routes, bare `Source: {string}` text). Owns HTML sanitization: registers
  a module-load-time `DOMPurify.addHook('afterSanitizeAttributes', ...)`
  that forces `target="_blank" rel="noopener"` on every surviving `<a>`,
  and exports (today, unexported — file-local) `sanitizeDescription`
  built from `ALLOWED_TAGS = ['p','br','strong','b','em','i','ul','ol','li','a']`
  / `ALLOWED_ATTR = ['href']`.
- **`frontend/components/IssueList.tsx`** — the only component that
  embeds `<DisruptionDetail disruption={status.disruption} />` (inside
  each `AccordionPanel`, only `if (status.disruption)`, line 383-389).
- **`frontend/app/lines/[id]/page.tsx`** and
  **`frontend/app/stations/[crs]/page.tsx`** — the only two pages that
  render `IssueList`. `app/page.tsx` (the dashboard) fetches disruption
  data too (`getStopPointDisruption`) but only reads it for a worst-severity
  badge and a sample-stats summary line — it never renders `IssueList` or
  `DisruptionDetail` at all, confirmed by grep (no match in that file).
  `app/stations/StationSearchForm.tsx`/`stations/page.tsx` only mention
  "Disruption" in prose/comments, not as a rendered `Disruption` value.

**Net result: exactly one component embeds `DisruptionDetail`
(`IssueList.tsx`), and exactly two pages render `IssueList`
(`/lines/[id]`, `/stations/[crs]`).** Adding the link inside
`DisruptionDetail.tsx` itself, once, covers every current and future call
site with zero changes needed to `IssueList.tsx` or either page — see
Decision 4.

**Public read-route convention** (checked against every existing
`public_router()`-mounted route, `crates/api/src/routes/mod.rs`): every
read this app already serves is unauthenticated — `lines.rs`'s
`GET /lines`/`GET /lines/{id}`/`GET /lines/{id}/definition`,
`reference.rs`'s station/TOC type-ahead, `history_retention.rs`,
`freshness.rs`, and the whole TfL-shaped `line_status.rs` family
(`/Line/...`, `/StopPoint/{crs}/Disruption`, both merged unprefixed
directly in `main.rs`, not nested under `/public`, but still
`require_internal_token`-free). The only session-gated reads anywhere in
this app are genuinely personal data: a user's own `Preferences`, their
own `CustomLineDetail.isOwner` flag, and every train-tracking/ticket route
(`crates/api/src/routes/train.rs`) — data that says "this is *mine*," not
a public transit fact. An incident record is squarely in the public-fact
category, and in fact **is already fully public today** — every field a
detail page would show (`summary`/`description`/`affectedStations`/
`validityPeriods`/`isPlanned`) already flows unauthenticated through
`GET /Line/{ids}/Status?detail=true` and `GET /StopPoint/{crs}/Disruption`
(always `detail=true`) as `disruption.description`/`.affectedStops`/etc.
Gating a dedicated *view* of a subset of already-public data behind auth
would add no real protection and be the one inconsistent read in this
app's whole public surface. `incident_history` is new *exposure* but not a
new *kind* of data — it is the same already-public fields, snapshotted
over time.

## Decisions

### 1. URL and route shape: raw `incident_id`, not the full `source` string

`/incidents/[id]`, matching this app's established one-id-per-segment
convention (`/lines/[id]`, `/stations/[crs]`, `/train/by-id/[trackingId]`)
— always the entity's own primary key in the URL, never a
provenance-prefixed wire value.

The brief asked which is more natural given what's actually available
client-side without extra parsing: `disruption.source` (already present,
zero parsing) vs. the raw `incident_id` (requires stripping the known
`knowledgebase-incident-` prefix). **Decision: raw `incident_id`.**
Reasons:

- It is the table's real primary key — the backend query is a direct
  `WHERE incident_id = $1`, no prefix-stripping needed server-side, and no
  risk of the URL encoding a value (`knowledgebase-incident-12345`) that
  duplicates information already implicit in which route you're on.
- `/incidents/knowledgebase-incident-12345` is a worse URL than
  `/incidents/12345` for no benefit — the path segment already says
  "incidents."
- The "extra parsing" needed client-side is a single, fixed-format prefix
  strip against a format Correction 1 verified is always exactly
  `knowledgebase-incident-{incident_id}` (never bare, never a different
  prefix) — not real parsing, not a guess. It is written **once**, in one
  shared helper (Decision 4), not duplicated at every call site.

### 2. Backend: `GET /public/incidents/{incidentId}`

New file `crates/api/src/routes/incidents.rs`, merged into
`public_router()` in `crates/api/src/routes/mod.rs` (alongside
`lines::router()`/`reference::router()`/`history_retention::router()`),
so the full path is `/public/incidents/{incidentId}` — unauthenticated,
per the public-read-convention finding above.

Three new query functions in `crates/api/src/data/queries.rs`, following
the file's existing style (plain `sqlx::query`/`sqlx::query_as`, no
`query!` macro, matching `line_status_for_ids`/`line_status_history_for_range`):

```rust
pub struct IncidentRow {
    pub incident_id: String,
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,   // Vec<common::ValidityPeriod>
    pub is_planned: bool,
    pub is_cleared: bool,
    pub first_seen_at: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
}

pub async fn incident_by_id(pool: &PgPool, incident_id: &str) -> Result<Option<IncidentRow>> {
    // SELECT incident_id, summary, description, operators, affected_stations,
    //        priority, validity_periods, is_planned, is_cleared,
    //        first_seen_at, fetched_at
    // FROM incidents WHERE incident_id = $1
}

pub struct IncidentHistoryRow {
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub recorded_at: DateTime<Utc>,
}

pub async fn incident_history_for_id(pool: &PgPool, incident_id: &str) -> Result<Vec<IncidentHistoryRow>> {
    // SELECT summary, description, operators, affected_stations, priority,
    //        validity_periods, is_planned, is_cleared, recorded_at
    // FROM incident_history WHERE incident_id = $1 ORDER BY recorded_at DESC
    // -- matches the incident_history_id_time index exactly, no new index needed
}
```

**Deliberately excludes every `extracted_*`/`source_text_hash` column.**
The brief's content list (description, stations/routes, validity,
planned-flag, history timeline, cross-reference) never asks for NLP
extraction output, that data is explicitly scoped "written only by
`enricher`, read only by `aggregator`" per its own migration header
comments (not meant for direct end-user display), and half of the columns
that would carry it are already-deprecated dead weight per the two-step
migration note in `20260822090000_incident_extraction_periods.sql`. Out of
scope — see Explicitly out of scope.

Handler (`incidents.rs`), same shape as `lines.rs::get_line_definition`/
`line_status.rs::get_line_status_history` — `Path(String)` extraction,
`(StatusCode, String)` error type, `404` via a `None`/empty match, no
special-casing an incident with `is_cleared = true` (a cleared incident is
still a real, fully valid detail page — arguably more useful, since that's
exactly when someone wants to check "did this ever affect my line," and
the timeline is most interesting once an incident is finished evolving):

```rust
async fn get_incident(
    State(app): State<App>,
    Path(incident_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(incident) = queries::incident_by_id(&app.database, &incident_id)
        .await.map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, "incident not found".to_string()));
    };
    let history = queries::incident_history_for_id(&app.database, &incident_id)
        .await.map_err(internal_error)?;
    // Reconstruct the exact `Disruption.source` string this incident would
    // carry on a live LineStatus row -- see Correction 1's verified format.
    let source = format!("knowledgebase-incident-{incident_id}");
    let lines = queries::lines_currently_reporting_incident(&app.database, &source)
        .await.map_err(internal_error)?;
    Ok(Json(to_incident_detail_json(incident, history, lines)))
}
```

Response shape, camelCase, matching every other `crates/api` public JSON
response:

```jsonc
{
  "incidentId": "12345",
  "summary": "Signal failure at Woking",
  "description": "<p>...</p>",              // raw HTML, NOT pre-sanitized server-side
  "operators": ["VT", "SW"],
  "affectedStations": ["WOK", "WAT"],
  "priority": 3,                             // raw RDM int, no documented meaning -- shown as-is or omitted, see Open questions
  "validityPeriods": [{ "fromDate": "...", "toDate": null, "isNow": true }],
  "isPlanned": false,
  "isCleared": false,
  "firstSeenAt": "2026-08-30T09:00:00Z",
  "fetchedAt": "2026-08-31T10:15:00Z",
  "currentlyAffectsLines": [{ "id": "south-western", "name": "South Western Main Line" }],
  "history": [
    { "summary": "...", "description": "...", "operators": [...],
      "affectedStations": [...], "priority": 2, "validityPeriods": [...],
      "isPlanned": false, "isCleared": false, "recordedAt": "2026-08-30T09:00:00Z" }
  ]
}
```

### 3. Cross-reference: "which lines currently report this incident"

Real, queryable, no new column or index needed — but genuinely new
territory for this codebase (grepped every `crates/api`/`crates/aggregator`
query for `jsonb_array_elements`/`->>`/`@>`: none exist yet). **Cannot use
JSONB containment (`@>`)**: Postgres's array-containment semantics require
each array element to match *structurally in full*, not partially — `'[{"a":1,"b":2}]'::jsonb @> '[{"a":1}]'::jsonb`
is `false` — so a `statuses @> '[{"disruption":{"source":"..."}}]'` query
would silently never match a real row (every stored status object also
carries `severity`/`reason`/`validity`/`data_quality`). The correct tool
is `jsonb_array_elements` to unnest the array, then a plain path
comparison:

```sql
SELECT DISTINCT line_status.line_id, line_status.name
FROM line_status, jsonb_array_elements(statuses) AS s
WHERE s -> 'disruption' ->> 'source' = $1
ORDER BY line_status.name
```

(`$1` is the full reconstructed `knowledgebase-incident-{id}` string, not
the bare id — that's the literal value actually stored in the JSONB.) No
index needed for the same reason the sibling `line_status.source` column
went without one (per its own migration comment): this table is one row
per line, tens of rows total — a full scan plus unnest is cheap at this
scale, and adding a GIN expression index here ahead of any measured need
would be exactly the "cargo cult" the existing migration comment already
warns against.

This answers a real question the incident's own row can't: an incident
can match zero, one, or several lines depending on the matcher/segment
registry (`crates/aggregator/src/matcher.rs`), and that set can change
between aggregation cycles independent of the incident's own text — so
this is computed fresh per request, not stored.

### 4. Wiring the link: inside `DisruptionDetail.tsx`, not `IssueList.tsx`

**New shared helper, `frontend/lib/incidents.ts`:**

```ts
const KNOWLEDGEBASE_INCIDENT_PREFIX = 'knowledgebase-incident-';

/** The only place that "parses" `Disruption.source` — see
 * docs/superpowers/specs/2026-08-31-incident-detail-page-design.md
 * Correction 1 for why this exact prefix, and why the LDBWS ("ldbws-sampling")
 * and TfL ("tfl-line-status-{lineId}") source values must NOT resolve to a
 * link (neither names a real `incidents` row). */
export function incidentIdFromSource(source: string | null | undefined): string | null {
  if (!source || !source.startsWith(KNOWLEDGEBASE_INCIDENT_PREFIX)) return null;
  return source.slice(KNOWLEDGEBASE_INCIDENT_PREFIX.length);
}
```

**`DisruptionDetail.tsx` gets one addition**, right after its existing
`Source: {disruption.source}` line:

```tsx
{incidentId && (
  <TextLink href={`/incidents/${incidentId}`} underline="always">
    View full incident details
  </TextLink>
)}
```

with `const incidentId = incidentIdFromSource(disruption.source);` computed
at the top of the component. `TextLink` is this app's single established
link component (`frontend/components/TextLink.tsx`, `underline="always"`
for WCAG 1.4.1 body-flow links, already used throughout).

**Why here and not in `IssueList.tsx`:** per the Current relevant state
inventory, `DisruptionDetail` is the *only* place a `Disruption` is
rendered anywhere in this frontend, and it is already the natural
"description + metadata" unit — the link belongs beside the data it links
out from, not hoisted into its caller. This one change automatically
reaches both real render sites (`/lines/[id]` and `/stations/[crs]`, both
via `IssueList`) with no changes to either page or to `IssueList.tsx`
itself, and reaches any future caller of `DisruptionDetail` for free.

**Why it renders conditionally, not always:** `incidentIdFromSource`
returns `null` for the two non-incident-backed provenance strings
(`ldbws-sampling`, `tfl-line-status-{lineId}`) — Correction 1's whole
point. An LDBWS-inferred or TfL status still renders its
`Source: ldbws-sampling` text exactly as today, just with no dead link
appended.

### 5. Reusing sanitization, not reimplementing it

`DisruptionDetail.tsx` today keeps `ALLOWED_TAGS`/`ALLOWED_ATTR`/the
`DOMPurify.addHook` registration/`sanitizeDescription` all file-local
(none exported). **Extract all four into a new
`frontend/lib/sanitizeHtml.ts`**, exporting `sanitizeDescription(html: string): string`.
`DisruptionDetail.tsx` imports it instead of defining it; the new
`app/incidents/[id]/page.tsx` imports the same function for the
incident's own `description` field. ES modules are singletons, so moving
the `DOMPurify.addHook(...)` call into the shared module still runs it
exactly once regardless of how many places import
`sanitizeDescription` — no duplicate-hook risk. This is a pure move, not a
behavior change: the allowed-tags/attrs list and the forced
`target="_blank" rel="noopener"` hook are carried over verbatim.

### 6. Page content and layout

`frontend/app/incidents/[id]/page.tsx` — async Server Component,
`export const revalidate = 0` (same rationale as every other dynamic
route in this app — `next build` cannot prerender against a database that
only exists on the compose network at runtime), `notFound()` on
`ApiNotFoundError` (same pattern as `/lines/[id]`):

- **Heading**: the incident's `summary`. A `Badge` for Planned vs.
  Real-time (`isPlanned`), matching `Disruption.category`'s
  `"PlannedWork"` vs `"RealTime"` distinction already shown elsewhere.
- **Description**: `dangerouslySetInnerHTML={{ __html: sanitizeDescription(description) }}`
  — identical rendering to `DisruptionDetail`, reused via Decision 5, not
  reimplemented.
- **Affected stations**: the same `Badge` row `DisruptionDetail` already
  renders for `affectedStops`, fed from `incident.affectedStations`
  directly (this field genuinely is incident-level — unlike routes, per
  Correction 3).
- **No top-level "affected routes" section.** Per Correction 3, there is
  no single, line-independent value to show — `affected_routes` is
  computed per-line by the matcher, and every line that currently reports
  this incident already shows its own version of that (as part of its own
  `IssueList`/`DisruptionDetail` rendering) on that line's own page. Not a
  gap: nothing is lost, since the "currently affects" section below
  (next bullet) already links out to exactly those pages.
- **Validity period(s)**, using the existing `formatFullValidity`-style
  rendering `IssueList.tsx` already has for a single validity range,
  extended to render every entry in `validityPeriods` (the incident row
  can carry more than one — `IssueList`'s own `LineStatus.validityPeriods`
  is always a one-element array by the time it reaches the frontend,
  since `validity_for_output` already collapsed it; the incident's own
  `validity_periods` column has not been collapsed, so this page is the
  first frontend surface that needs to render more than one).
- **"Currently affects" section**: `currentlyAffectsLines`, each rendered
  as a `TextLink` to `/lines/${line.id}` (same as the existing per-line
  links on the station page, `app/stations/[crs]/page.tsx:96`). Empty
  state: "Not currently reported on any tracked line" — a real, expected
  outcome for a cleared or superseded incident, not an error.
- **History timeline**: `history`, newest-first (already ordered that way
  by the query). Rendered as a plain grouped `Stack`/`Divider` list, one
  entry per `IncidentHistoryRow`, each showing `recordedAt` (via the
  existing `formatDateTime` helper) plus which fields changed from the
  entry below it in the list (summary/description/priority/validity/
  planned/cleared — a simple textual diff summary, e.g. "Priority changed
  from 2 to 3", not a full field dump every time, since most snapshots
  differ in only one or two fields). **Not** Mantine's `Timeline`
  component — it ships with `@mantine/core` (already a dependency) but is
  unused anywhere in this codebase today, so there is no local convention
  for its styling; this spec instead follows the visual convention
  `/lines/[id]/history` already established for an analogous "changes
  over time" view (`groupHistoryByDay`-style plain list), the closer
  precedent. Flagged as an open styling choice either way — see Open
  questions.
- **First seen / last fetched**: `firstSeenAt` (this app's own,
  RDM-independent clock) and `fetchedAt`, both rendered as plain
  `formatDateTime` text near the bottom, same low-emphasis styling
  `IssueList`'s "Valid:" line already uses.

## API/type contract

```ts
// frontend/lib/types.ts additions

export interface IncidentLineRef {
  id: string;
  name: string;
}

export interface IncidentHistoryEntry {
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  recordedAt: string; // RFC3339
}

/** `GET /public/incidents/{incidentId}`'s response
 * (`crates/api/src/routes/incidents.rs`). `description` is raw HTML --
 * sanitize with `sanitizeDescription` (`frontend/lib/sanitizeHtml.ts`)
 * before rendering, same as `DisruptionDetail`. `currentlyAffectsLines`
 * is computed fresh per request (see Decision 3) -- can be empty for a
 * cleared or no-longer-matched incident, which is a normal outcome, not
 * an error. */
export interface IncidentDetail {
  incidentId: string;
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string; // RFC3339
  currentlyAffectsLines: IncidentLineRef[];
  history: IncidentHistoryEntry[];
}
```

```ts
// frontend/lib/api.ts addition -- public, unauthenticated read, so no
// cookie-forwarding needed (unlike getTicketsForTrackedTrain/getSession) --
// same plain fetchJson pattern as getLineStatus/getCustomLine.

export async function getIncident(incidentId: string): Promise<IncidentDetail> {
  return fetchJson<IncidentDetail>(`${baseUrl()}/public/incidents/${incidentId}`, {
    cache: 'no-store',
  });
  // Throws ApiNotFoundError on 404 (via errorForResponse), same as every
  // other fetchJson caller -- app/incidents/[id]/page.tsx catches it and
  // calls notFound(), identical to /lines/[id]'s existing pattern.
}
```

```ts
// frontend/lib/sanitizeHtml.ts -- NEW, extracted from DisruptionDetail.tsx
// (Decision 5), no behavior change from what DisruptionDetail already does.

export function sanitizeDescription(html: string): string { /* moved verbatim */ }
```

```ts
// frontend/lib/incidents.ts -- NEW (Decision 4)

export function incidentIdFromSource(source: string | null | undefined): string | null { /* ... */ }
```

`getIncident` is called only from `app/incidents/[id]/page.tsx`, a Server
Component read straight to `API_BASE_URL` — no same-origin proxy involved
(matching every other unauthenticated Server Component read in this app;
the proxy at `frontend/app/api/[...path]/route.ts` exists for
browser-initiated mutations, not server-side GETs), so **no proxy
allowlist change is needed for this feature**.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                          │
│                                                                            │
│  app/incidents/[id]/page.tsx     NEW -- async Server Component,          │
│                                     getIncident(id), notFound() on 404   │
│                                                                            │
│  components/DisruptionDetail.tsx  MODIFIED -- adds a conditional          │
│                                     "View full incident details" link,    │
│                                     computed via incidentIdFromSource     │
│                                     (Decision 4); sanitizeDescription now │
│                                     imported from lib/sanitizeHtml.ts     │
│                                     instead of defined locally (Dec. 5)  │
│                                                                            │
│  lib/incidents.ts     NEW -- incidentIdFromSource                        │
│  lib/sanitizeHtml.ts  NEW -- sanitizeDescription, DOMPurify hook (moved) │
│  lib/api.ts           + getIncident                                     │
│  lib/types.ts         + IncidentDetail, IncidentHistoryEntry,           │
│                          IncidentLineRef                                │
│                                                                            │
│  components/IssueList.tsx, app/lines/[id]/page.tsx,                     │
│  app/stations/[crs]/page.tsx   UNCHANGED -- reach the new page only     │
│                                  through DisruptionDetail's new link      │
└──────────────────────────┬────────────────────────────────────────────┘
     server-side fetch, no-store, no auth needed
                            ▼
┌────────────────────────────────────────────────────────────────────────┐
│ api crate                                                                │
│  routes/incidents.rs   NEW -- GET /public/incidents/{incidentId}        │
│                          merged into public_router() alongside          │
│                          lines::router()/reference::router()            │
│  data/queries.rs       + incident_by_id, incident_history_for_id,       │
│                          lines_currently_reporting_incident              │
│                          (jsonb_array_elements over line_status.statuses,│
│                           NOT the unrelated line_status.source column   │
│                           -- see Correction 2)                          │
└──────────────────────────┬────────────────────────────────────────────┘
                            ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Postgres: incidents, incident_history (both pre-existing, no schema    │
│ change needed), line_status (read-only, JSONB unnest)                   │
└────────────────────────────────────────────────────────────────────────┘
```

## Error handling

- **`404` (unknown `incidentId`)** — `app/incidents/[id]/page.tsx` catches
  `ApiNotFoundError` and calls `notFound()`, identical to `/lines/[id]`'s
  existing pattern. This is the expected outcome for a mistyped id, a
  very old incident this app never actually ingested (the `incidents`
  table has no retention/prune job today — a real gap, but out of scope
  for this spec), or a link constructed from a stale/copied source string.
- **Any other non-ok status** (5xx, network failure) — falls through to
  the existing root `app/error.tsx`, same as every other page section
  with no segment-specific error boundary today.
- **`currentlyAffectsLines` empty** — not an error; rendered as an
  explicit "not currently reported on any tracked line" line, per
  Decision 6.
- **`history` a single entry (just the incident's first-seen snapshot)**
  — not an error; the timeline section still renders, just with one row,
  rather than being hidden (a brand-new incident is exactly when a user
  is most likely to check "is this new" via this section).

## Testing

Following this repo's existing convention (colocated `*.test.tsx`/
`*.test.ts`, `renderWithMantine`, Vitest; Rust side's existing
`#[cfg(test)]` module-per-file convention):

- **`crates/api/src/data/queries.rs`**: unit/integration tests for
  `incident_by_id` (found / not found), `incident_history_for_id`
  (ordering, empty case), and `lines_currently_reporting_incident`
  (matches only the exact `disruption.source` string, confirms it does
  **not** false-positive against `ldbws-sampling`/`tfl-line-status-*`
  rows or against the unrelated `line_status.source` column — the
  concrete regression test for Correction 2).
- **`crates/api/src/routes/incidents.rs`**: handler tests mirroring
  `line_status.rs`'s existing style — 404 on unknown id, full shape on a
  found one, empty `currentlyAffectsLines`/`history` handled without
  panicking.
- **`frontend/lib/incidents.test.ts`**: `incidentIdFromSource` — real
  prefix strips correctly; `null`/`undefined`/`ldbws-sampling`/
  `tfl-line-status-northern` all return `null` (this is the direct test
  for Correction 1's scope boundary).
- **`frontend/lib/sanitizeHtml.test.ts`**: move `DisruptionDetail.test.tsx`'s
  existing HTML-tag/script-stripping/link-hardening assertions here
  (they test `sanitizeDescription`'s behavior, not anything
  `DisruptionDetail`-specific) — confirms the extraction in Decision 5 is
  byte-for-byte behavior-preserving.
- **`frontend/components/DisruptionDetail.test.tsx`**: add cases for the
  new link — renders "View full incident details" linking to
  `/incidents/{id}` when `source` is `knowledgebase-incident-{id}`;
  renders no such link when `source` is `ldbws-sampling`,
  `tfl-line-status-*`, or `null`.
- **`frontend/app/incidents/[id]/page.test.tsx`** (or equivalent): 404
  path, full-content render including the history list and the
  "currently affects" links, and the "not currently reported anywhere"
  empty state.

## Explicitly out of scope

- **A detail page (or anything resembling one) for LDBWS-inferred or TfL
  statuses.** Per Correction 1, neither path's `Disruption.source` names a
  real, persisted incident record — `ldbws-sampling` is a shared literal
  constant across every LDBWS-inferred status on every line, and
  `tfl-line-status-{lineId}` names a *line*, not an incident. There is no
  comparable row to look up for either — an LDBWS-inferred "3 of 11
  sampled services delayed" reading is a live computation re-derived every
  aggregation cycle, not a stored fact with its own identity or history.
  `incidentIdFromSource` returning `null` for both is the enforcement
  mechanism; no route or page is designed for either case.
- **Exposing NLP-extraction fields** (`extracted_category`,
  `extracted_severity`, `extracted_periods`, etc.) on the detail page. Not
  requested by the brief's content list, and these columns are explicitly
  documented as an internal `enricher`→`aggregator` channel, not
  end-user-facing data.
- **Pruning/retention for `incidents`/`incident_history`.** Neither table
  has a pruning job today (unlike `line_status_history`, which
  `HISTORY_RETENTION_DAYS` explicitly bounds) — a real, separate gap this
  spec doesn't attempt to close.
- **Editing an incident, or any write path on this route.** `GET` only;
  Knowledgebase incidents are poller-written, not user-editable, matching
  every other reference-data table in this app.
- **A "browse all incidents" index/listing page.** Only a by-id detail
  page is designed here — reaching it always starts from a link on an
  already-rendered `Disruption` (Decision 4) or a directly-typed/shared
  URL, never a catalogue browse. No backend route supports "list every
  incident" either; adding one is a separate, unscoped feature.

## Open questions / risks

1. **`priority` (raw RDM integer, no documented "major"/"minor" meaning)**
   — included in the API contract for completeness (it's a real incident
   field with no other place to see it), but this spec does not resolve
   how or whether to surface it on the page itself. Showing a bare integer
   with no legend risks looking like a bug; omitting it entirely means the
   history timeline's "priority changed from 2 to 3" entries lose their
   only anchor. Left as an implementation-time call.
2. **History timeline visual treatment** — Decision 6 recommends a plain
   grouped list (following `/lines/[id]/history`'s precedent) over
   Mantine's unused `Timeline` component, but this is a real design
   choice with no strong forcing function either way; worth a second look
   once the page is actually built and there's real data to look at.
3. **No retention on `incidents`/`incident_history`** (see Explicitly out
   of scope) means this page's 404 behavior for a very old incident is
   currently indistinguishable from "never existed" — both read as a
   plain 404. If retention is ever added, this page's error copy may want
   to special-case "this incident's record has been pruned" vs. "no such
   incident," which today's schema can't tell apart either way.
4. **`lines_currently_reporting_incident`'s full-scan-plus-unnest query
   has no measured cost yet** — reasoned to be cheap given `line_status`'s
   current size (tens of rows), matching this repo's own stated rationale
   for leaving the sibling `line_status.source` column unindexed. Worth
   revisiting only if `line_status` ever grows by orders of magnitude
   (e.g. many more custom lines), not assumed to be a problem today.
