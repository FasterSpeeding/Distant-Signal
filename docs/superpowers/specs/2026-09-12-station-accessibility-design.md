# Design: Station Accessibility & Facilities Section

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-03-per-station-stats-design.md` (closest
precedent — a real, small frontend-plus-backend feature design layered onto
an existing station page, no implementation plan embedded here; that is a
separate, later step in this repo's process). No code is changed by this
document.

## Goal

`stations.accessibility` (a `JSONB NOT NULL DEFAULT '{}'` column, populated
wholesale on every `poller-stations` poll) already carries real per-station
facility data from the RDM Stations feed — step-free access, staff
assistance, toilets, lifts, car parks, cycling facilities, transport links,
and more — but no route in `crates/api` exposes it and no frontend page
renders it. This document designs the smallest honest slice of that data
worth showing on `/stations/[crs]`: which fields get surfaced, the new
route that has to exist to serve them, and how the frontend renders a
payload whose exact field-level shape this codebase cannot currently cite
with confidence (see Correction 2).

## Corrections to the brief's assumptions (recorded for posterity)

Following the incident-detail-page spec's own "Corrections" precedent:
direct inspection of the code turned up things the brief didn't quite get
right, materially affecting the design below.

1. **The brief's claim that "the station page only shows departures today"
   is false.** `frontend/app/stations/[crs]/page.tsx` shows no departure
   board at all. It renders two independent sections: per-line disruption
   status (`fetchStationDisruptions`, backed by `GET
   /StopPoint/{crs}/Disruption`) and per-operator sample stats
   (`fetchStationSampleStats`, backed by `GET
   /public/stations/{crs}/sample-stats`, added by the per-station-stats
   design this document mirrors). Live departures for a station are a
   different feature entirely (`station_samples`/LDBWS, surfaced via
   `/track`, not this page). This matters because the new section below is
   the page's **third** independent block, not a replacement or extension
   of a departures list that doesn't exist here.

2. **The brief's field list ("step-free access, staff assistance, toilets,
   lifts, car parks, cycling facilities, transport links, etc.") names real
   top-level keys, but this codebase has never recorded their internal
   shape.** `crates/poller-stations/src/schema.rs`'s module doc (lines
   17-30) enumerates the unmodeled `Station` fields collected verbatim via
   `#[serde(flatten)]` — `stationAccessibility`, `staffAssistance`,
   `toiletsAndChanging`, `transportLinks`, `lifts`, `ticketBuying`,
   `loungesAndWaiting`, `stationFacilities`, `helpAndSupport`,
   `platformFacilities`, `cycling`, `dropOffPickUp`, `carParks`,
   `changeHistory`, `slug`, `sixteenCharacterName`, `nationalLocationCode`,
   `minimumConnectionTime`, `address`, `stationAlerts`, `stationMap`,
   `staffingLevel`, `informationServices` — but that doc comment (and the
   OpenAPI spec it cites) only names the keys; nothing in this repo
   transcribes what's *inside* e.g. `stationAccessibility` (a boolean? an
   object of sub-flags? a free-text description?). The schema.rs test
   fixture (`SAMPLE_JSON`, lines 113-143) only exercises `slug` and
   `changeHistory` round-tripping — it does not include a single one of the
   accessibility-relevant keys. This is not a gap this document can close
   by reading more code; it changes the frontend design from "one component
   per known field" to "a generic, depth-limited, crash-proof renderer over
   values of genuinely unknown shape" (Decision 6).

3. **This is not a pure frontend change.** `crates/api/src/routes/
   reference.rs`'s `/public/stations` and `/public/tocs` — the only routes
   that read the `stations`/`tocs` tables today — return `Suggestion {
   code, name }` only (`crates/api/src/data/reference.rs:14-18`). No route
   anywhere returns `latitude`, `longitude`, `station_operator`, or
   `accessibility`. `crates/api/src/routes/mod.rs:79-90`'s own comment
   confirms `StationReference::accessibility` is currently *ingested*
   (private `/private` POST from `poller-stations`, capped at 100MB for
   exactly this reason) but never read back out through any public route.
   A new backend route is required; this cannot be built by only touching
   `frontend/`.

4. **"Accessibility" is already a heavily-used, differently-scoped word in
   this codebase**, referring to WCAG/web accessibility: `e2e/
   accessibility.spec.ts` (an axe-core sweep of anonymous-reachable routes,
   including `/stations/{crs}` itself), `docs/superpowers/specs/
   2026-09-02-frontend-accessibility-audit-research.md`, and comments across
   `frontend/components/PinToggle.tsx`, `ThemeToggle.tsx`, `layout.tsx`. The
   RDM feed's use of "accessibility" (physical/wheelchair/step-free access)
   is an unrelated, coincidentally-named concern. This document keeps the
   existing Rust-side vocabulary (`stations.accessibility`,
   `StationReference.accessibility`) unchanged — renaming a shipped DB
   column and struct field is real, unrelated churn this document declines
   to start — but picks deliberately distinct names on the frontend and in
   product copy (Decision 8) so a future `grep -ri accessibility` across
   `frontend/` does not silently conflate a WCAG regression with a station
   facilities bug.

5. **The column is `NOT NULL DEFAULT '{}'`, so "genuinely absent" and
   "present but empty" are two different, already-representable database
   states, not one.** `crates/api/migrations/20260706004003_reference_data.sql:16`.
   `accessibility` is never SQL `NULL` for a row that exists — but a CRS
   can also have **no row in `stations` at all** (e.g. covered by this
   app's static line-status catalogue but never present in the RDM
   Stations feed, or not yet polled). Those are the two honest absence
   states this design's 404-vs-`200 {}` split (Decision 5) is built around
   — deliberately the same shape as the per-station-stats design's
   404-vs-`200 []` precedent (that design's Decision 7), not a new pattern.

## Current relevant state (not re-derived beyond what's cited above)

- `common::StationReference` (`crates/common/src/lib.rs:834-842`):
  `crs: String`, `name: String`, `latitude: Option<f64>`, `longitude:
  Option<f64>`, `station_operator: Option<String>`, `accessibility:
  serde_json::Value` (doc comment: "JSONB passthrough — schema not modeled
  further here").
- `crates/poller-stations/src/schema.rs::RdmStation` captures every
  unlisted `Station` field via `#[serde(flatten)] pub rest:
  serde_json::Value`, mapped 1:1 into `StationReference.accessibility`
  (`schema.rs:81`). Global Constraint 7
  (`docs/superpowers/plans/01-poller-microservices.md:40-42`): "Don't
  hand-model every Stations-JSON accessibility sub-field as a typed Rust
  struct field" — this document does not violate that constraint (Decision
  1 curates by top-level key name only, never by sub-field).
- `crates/api/src/data/queries.rs::upsert_stations` (lines 225-252) is the
  only place that writes the column; there is no existing read function for
  it anywhere in `crates/api`.
- `crates/api/src/data/reference.rs` is the module that already owns
  `stations`/`tocs` type-ahead reads (`search_stations`, `search_tocs`,
  `get_all_tocs`), all returning the narrow `Suggestion` shape; `crates/api/
  src/routes/reference.rs` mounts them into `public_router()`.
- `crates/api/src/routes/station_stats.rs` (added by the per-station-stats
  design) is a *separate* module because it computes derived stats from
  `station_samples` at read time — a different table and a different kind
  of data (live sampling, not static reference). This document's data is a
  straight column read off `stations`, the same table `reference.rs`
  already owns, so the new route belongs in `reference.rs`'s module, not a
  new sibling of `station_stats.rs`. That's a real, cited design choice —
  see Decision 3.
- `frontend/app/stations/[crs]/page.tsx` (277 lines, read in full): fetches
  disruptions, preferences, sample stats and `tocs` in one `Promise.all`
  (lines 164-176), then renders three sections in order — heading/pin/share,
  disruptions, sample-stats-by-operator — each with its own three-state
  honesty split and its own `<Divider/>` + `<Title order={2} size="h4">`.
  The new section is a fourth, independent block following the same shape.
- `frontend/app/stations/[crs]/page.test.tsx` mocks each `lib/api.ts`
  function individually via `vi.mock('@/lib/api', ...)` and drives the page
  through `renderWithMantine`; the per-operator-stats `describe` block
  (lines 167-251) is the closest structural precedent for the new section's
  tests.
- `crates/poller-stations/src/main.rs:25-26`: the RDM Stations feed's
  documented recommended poll interval is 24 hours — this data changes on
  the order of months/years in practice, same "reference data" class as
  `getStationName`/`getAllTocs`, which both use Next's `revalidate: 3600`
  rather than the `cache: 'no-store'` used for live disruption/sample data
  (`frontend/lib/api.ts:139-145`, `:357-361`).
- `frontend/e2e/accessibility.spec.ts:28,72-74` already runs an axe-core
  sweep of `/stations/${REAL_STATION_CRS}` (default `PAD`) for
  `color-contrast`/`landmark-one-main`/`region`/`heading-order`/
  `page-has-heading-one` — the new section lands inside that page and is
  automatically covered by this existing sweep; see Decision 9 and
  Explicitly out of scope.

## Decisions

### 1. Scope: forward a curated allowlist of top-level keys, not the whole `accessibility` blob

Twelve of the twenty-two flattened `Station` fields named in `schema.rs`'s
module doc are plausibly "accessibility or amenities" in the brief's sense:

```
stationAccessibility, staffAssistance, toiletsAndChanging, lifts,
transportLinks, cycling, carParks, dropOffPickUp, platformFacilities,
stationFacilities, helpAndSupport, loungesAndWaiting
```

Excluded, with reasons:

- `ticketBuying`, `staffingLevel`, `informationServices` — ticketing/staffing
  concerns, not facility/accessibility ones; a plausible future section,
  not this one.
- `address`, `stationMap`, `stationAlerts` — address/map/alerts are their
  own display concerns (a map image, a live-alerts feed) with their own
  honesty questions this document doesn't want to silently inherit by
  bundling them in.
- `slug`, `sixteenCharacterName`, `nationalLocationCode`,
  `minimumConnectionTime`, `changeHistory` — pure metadata, not
  user-facing facility data.

This is a deliberate, named allowlist maintained as a Rust `const` in the
new backend code (Decision 3), not a blanket passthrough of everything
`rest` collected. It does **not** violate Global Constraint 7: the
constraint is about not hand-modeling *sub-fields*, and this allowlist only
selects which *top-level* keys are forwarded — each selected value's
internal shape is still passed through as an opaque `serde_json::Value`,
untouched, exactly as `accessibility` itself is stored.

Rationale for curating at all, rather than shipping the whole `rest` blob
to the browser: `crates/api/src/routes/mod.rs:83-89` measured the full
~2,600-station feed at ~55MB raw, precisely because it carries fields like
`ticketBuying`/`carParks`/`address` a station page has no use for; there is
no reason to ship a single station's copy of that same unrelated data to
every visitor of `/stations/[crs]` just because it happens to live in the
same JSONB column.

### 2. No new migration, no new write path

`poller-stations`/`upsert_stations` are entirely unmodified. This is a new
**read** only, off data already captured today.

### 3. New route lives in `reference.rs`, not a new sibling module

`crates/api/src/data/reference.rs` already owns every read of the
`stations` table (`search_stations`, `get_all_tocs`, etc.); this is another
read of the same table, not a derived computation over a different table
the way `station_stats.rs` is over `station_samples`. Add:

```rust
// crates/api/src/data/reference.rs

/// Top-level `stations.accessibility` keys considered "accessibility or
/// amenities" data for the station page's facilities section — see
/// docs/superpowers/specs/2026-09-12-station-accessibility-design.md
/// Decision 1 for why this list and not the full RDM `Station` object.
/// Each value is forwarded completely unexamined: this only filters by
/// key name, it does not decompose or validate what's inside (Global
/// Constraint 7, docs/superpowers/plans/01-poller-microservices.md:40-42).
const ACCESSIBILITY_KEYS: &[&str] = &[
    "stationAccessibility",
    "staffAssistance",
    "toiletsAndChanging",
    "lifts",
    "transportLinks",
    "cycling",
    "carParks",
    "dropOffPickUp",
    "platformFacilities",
    "stationFacilities",
    "helpAndSupport",
    "loungesAndWaiting",
];

/// Returns `None` when `stations` has no row for `crs` at all (this app
/// has never captured reference data for it) -- distinct from `Some` of an
/// empty object, which means the row exists but none of
/// `ACCESSIBILITY_KEYS` were present in its `accessibility` JSONB. Exact
/// `crs = $1` match, no case normalization -- same convention
/// `latest_station_sample` (`crates/api/src/data/queries.rs:1459-1463`)
/// already uses for a single-CRS lookup; this document does not introduce
/// or fix case-sensitivity handling either way (see Open questions/risks).
pub async fn station_accessibility(pool: &PgPool, crs: &str) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query("SELECT accessibility FROM stations WHERE crs = $1")
        .bind(crs)
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else { return Ok(None) };
    let full: serde_json::Value = row.try_get("accessibility")?;

    let mut filtered = serde_json::Map::new();
    if let Some(obj) = full.as_object() {
        for key in ACCESSIBILITY_KEYS {
            if let Some(value) = obj.get(*key) {
                if !value.is_null() {
                    filtered.insert(key.to_string(), value.clone());
                }
            }
        }
    }
    Ok(Some(serde_json::Value::Object(filtered)))
}
```

### 4. New route: `GET /public/stations/{crs}/accessibility`

Mounted from `crates/api/src/routes/reference.rs`'s existing `router()`
(same file, same `public_router()` membership as `/stations` and `/tocs`):

```rust
// crates/api/src/routes/reference.rs

Router::new()
    .route("/stations", axum::routing::get(search_stations))
    .route("/stations/{crs}/accessibility", axum::routing::get(get_station_accessibility))
    .route("/tocs", axum::routing::get(search_tocs))
    .route("/tocs/all", axum::routing::get(list_all_tocs))

async fn get_station_accessibility(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match reference::station_accessibility(&app.database, &crs)
        .await
        .map_err(internal_error)?
    {
        Some(data) => Ok(Json(data)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("no station reference data for: {crs}"),
        )),
    }
}
```

No hand-built `json!()` reshaping is needed here (unlike the sample-stats
route) — there is no nested `common` struct being embedded that could hit
the camelCase/snake_case pitfall `crates/api/src/routes/incidents.rs:53-59`
documents, because every value forwarded is already the RDM feed's own
camelCase JSON, untouched. The response body **is** the filtered object,
not an envelope — this mirrors returning `Json<Vec<Value>>` directly in
`station_stats.rs` rather than wrapping in a named struct.

Response shape, `GET /public/stations/EUS/accessibility`:

```json
{
  "stationAccessibility": { "...": "whatever the RDM feed put here" },
  "lifts": { "...": "..." },
  "carParks": [ { "...": "..." } ]
}
```

`GET /public/stations/ZZZ/accessibility` (row exists, no allowlisted keys
present): `200 {}`. `GET /public/stations/NOTREAL/accessibility` (no
`stations` row at all): `404`.

### 5. Frontend type: intentionally loose, `unknown`-valued

```ts
// frontend/lib/types.ts

/** `GET /public/stations/{crs}/accessibility`'s response -- a filtered
 * passthrough of `stations.accessibility` (see design spec Decision 1 for
 * the exact key allowlist). Every value is `unknown`, not a nested
 * interface, because this codebase has never recorded the RDM feed's
 * field-level shape for any of these keys (design spec Correction 2) --
 * typing them more precisely here would be inventing a contract this app
 * cannot actually verify. Keys are present only when non-null in the
 * source data; a key with no data is simply absent, not `null`. */
export interface StationAccessibilityData {
  stationAccessibility?: unknown;
  staffAssistance?: unknown;
  toiletsAndChanging?: unknown;
  lifts?: unknown;
  transportLinks?: unknown;
  cycling?: unknown;
  carParks?: unknown;
  dropOffPickUp?: unknown;
  platformFacilities?: unknown;
  stationFacilities?: unknown;
  helpAndSupport?: unknown;
  loungesAndWaiting?: unknown;
}
```

`frontend/lib/api.ts`:

```ts
/** `GET /public/stations/{crs}/accessibility` -- filtered RDM station
 * facilities/accessibility data (design spec Decision 3-4). Cached for an
 * hour, same convention as `getStationName`/`getAllTocs`: the underlying
 * feed's own documented poll interval is 24 hours
 * (`crates/poller-stations/src/main.rs:25-26`), so this is reference data,
 * not a live feed, and does not warrant `cache: 'no-store'`. Throws
 * `ApiNotFoundError` on a 404 (no `stations` row for this CRS at all) via
 * `errorForResponse`, same as every other `fetchJson` caller.
 */
export async function getStationAccessibility(crs: string): Promise<StationAccessibilityData> {
  return fetchJson<StationAccessibilityData>(`${baseUrl()}/public/stations/${crs}/accessibility`, {
    next: { revalidate: 3600 },
  });
}
```

### 6. Rendering: a generic, depth-limited, crash-proof value renderer — not one component per known field

Because Correction 2 established that no field's internal shape is known
with confidence, the renderer cannot assume e.g. `stationAccessibility` is
a boolean, an object, or an array. It must render *whatever comes back*
without throwing, and degrade gracefully for shapes that don't fit a
simple label/value model.

A single small helper, `frontend/lib/stationAccessibility.ts`:

- **`string | number | boolean`** → rendered as-is (`String(value)`,
  booleans as "Yes"/"No").
- **`null` / `undefined`** → the key is skipped entirely (this should
  already be filtered server-side by Decision 3, but the frontend does not
  trust that as a hard guarantee — defense in depth).
- **Array of primitives** → comma-joined text.
- **Array of objects, or any array mixing types** → rendered as "`N`
  items", each item independently recursed one level *inside a `<Spoiler>`*
  rather than inlined, so a large or deeply nested array (e.g. `carParks`
  as an array of car-park objects) doesn't produce a wall of text.
- **Plain object, depth 1** → one label/value row per own key, keys
  humanized (`stepFreeAccess` → "Step free access") via a simple
  camelCase-to-title-case transform — no hardcoded per-field dictionary,
  since the field set is unverified (Correction 2); a wrong or ugly label
  from an unanticipated key is an acceptable, non-crashing degradation.
- **Anything deeper than one level of nesting, or a shape the above rules
  don't cover cleanly** → falls back to a collapsed, monospace
  `JSON.stringify(value, null, 2)` inside a Mantine `<Spoiler
  maxHeight={0}>`/`<Code block>`, so the raw data is still inspectable
  without ever crashing the page or hiding the fact that something was
  there.

This is the same "never trust upstream shape, degrade instead of crash"
posture `frontend/app/stations/[crs]/page.tsx` already takes at the
coverage level (404-vs-`[]`) — this decision applies it one level deeper,
to values inside a single already-200'd response.

### 7. Category display order and labels

Fixed order (not alphabetical, not the order keys happen to appear in
JSON — a station page reader scans top-to-bottom for the thing they care
about, most-asked-about first):

```
1. Step-free access & assistance  → stationAccessibility, staffAssistance
2. Facilities                     → toiletsAndChanging, lifts, loungesAndWaiting
3. Platform & station facilities  → platformFacilities, stationFacilities, helpAndSupport
4. Getting here                   → transportLinks, carParks, dropOffPickUp, cycling
```

Each group heading renders only if at least one of its keys is present in
the response; an empty group is not shown (same "don't invent a row for
data that isn't there" precedent as `compute_station_operator_stats` not
listing an operator with zero departures).

### 8. UI placement and naming — a fourth independent section, deliberately non-colliding names

A new block at the bottom of `frontend/app/stations/[crs]/page.tsx`,
fetched alongside the existing `Promise.all` (disruptions, preferences,
sample stats, tocs) and rendered after the "Sample stats by operator"
section, with its own `<Divider/>` and `<Title order={2} size="h4">`.
Heading copy: **"Accessibility & facilities"** — deliberately not just
"Accessibility" alone, so it reads unambiguously as physical-access
information even out of context, and so a future contributor searching
this codebase's frontend for "accessibility" (meaning WCAG, per Correction
4) is less likely to land here expecting a color-contrast fix.

Naming actually used:

- Type: `StationAccessibilityData` (frontend/lib/types.ts) — kept aligned
  with the wire/Rust name rather than invented ("amenities", "facilities")
  because it *is* exactly `StationReference.accessibility`, filtered; a
  different frontend name for the same data as the backend would be its
  own, worse confusion.
- Fetcher: `getStationAccessibility` (frontend/lib/api.ts), matching every
  other `getStationX` fetcher already on this page.
- Component: `frontend/components/StationAccessibilitySection.tsx` — a new,
  single-purpose section component (this page's other two sections are
  inlined directly in `page.tsx`; this one is pulled into its own file
  specifically so the value-rendering logic in Decision 6 has a home that
  isn't `page.tsx` itself, which is already 277 lines).
- Page-local coverage wrapper: `fetchStationAccessibility`, returning a
  three-state result exactly mirroring `fetchStationDisruptions`/
  `fetchStationSampleStats`'s existing shape:

```ts
type StationAccessibilityResult =
  | { coverage: 'unavailable' }
  | { coverage: 'empty' }
  | { coverage: 'present'; data: StationAccessibilityData };

async function fetchStationAccessibility(crs: string): Promise<StationAccessibilityResult> {
  try {
    const data = await withStaleFallback(`stationAccessibility:${crs}`, () => getStationAccessibility(crs));
    return Object.keys(data).length === 0 ? { coverage: 'empty' } : { coverage: 'present', data };
  } catch (err) {
    if (err instanceof ApiNotFoundError) return { coverage: 'unavailable' };
    throw err;
  }
}
```

### 9. Three honest states, mirroring the page's own established pattern exactly

- `coverage === 'unavailable'` (404 — no `stations` row for this CRS at
  all) → *"We don't have station reference data for this station yet."*
- `coverage === 'empty'` (`200 {}` — row exists, no allowlisted facility
  data present) → *"No accessibility or facilities details have been
  published for this station."* — a genuinely quiet answer, not a coverage
  gap, same "quiet, not missing" distinction the sample-stats section
  already draws one field over on this same page.
- `coverage === 'present'` → the grouped sections from Decision 7.

A non-404 fetch failure (network blip) is served stale via
`withStaleFallback`, identical to every other section on this page — no
new caching mechanism introduced.

## Architecture

```
station page request
        │
        ▼
GET /public/stations/{crs}/accessibility   (crates/api/src/routes/reference.rs)
        │
        ▼
reference::station_accessibility(pool, crs)   (crates/api/src/data/reference.rs, new)
        │  SELECT accessibility FROM stations WHERE crs = $1
        │  → None (no row) | Some(full JSONB)
        ▼
filter to ACCESSIBILITY_KEYS, drop nulls
        │
        ▼
Json<serde_json::Value>  (200 {…filtered…} | 200 {} | 404)
        │
        ▼
frontend: getStationAccessibility → fetchStationAccessibility → StationAccessibilitySection
        │
        ▼
generic depth-limited renderer (Decision 6) inside four fixed category groups (Decision 7)
```

No write path is touched anywhere; `poller-stations`/`upsert_stations` are
unmodified.

## Error handling

- **Database error**: `internal_error` → `500`, logged — same shape as
  every other route in `reference.rs`.
- **No `stations` row for `crs`**: `404`, mirrors the
  `get_stop_point_disruption`/`get_station_sample_stats` "not covered"
  precedent. Frontend renders a plain sentence, not a page-level error.
- **Row exists, no allowlisted keys present**: `200 {}`, rendered as
  "quiet, not missing."
- **A present key's value has an unexpected internal shape**: never an
  error at any layer — Decision 6's renderer degrades to raw JSON rather
  than throwing, and no backend validation rejects or reshapes unexpected
  shapes (Global Constraint 7).
- **Frontend fetch failure that isn't a 404**: served stale via
  `withStaleFallback`, identical to the page's other two sections.

## Non-goals / explicitly out of scope

- **No new data source or change to `poller-stations`'s ingestion.** This
  is a read of data already captured today.
- **No accessibility-based search, filtering, or sorting anywhere else in
  the app** (e.g. "show me step-free stations on this line"). A real,
  plausible future feature, not this one — this document surfaces the data
  on one station's own page only.
- **No decomposition of the JSONB into typed Rust structs.** Global
  Constraint 7 stands; Decision 1's allowlist filters by key name only.
- **No rendering of `stationMap`, `address`, `stationAlerts`,
  `ticketBuying`, `staffingLevel`, or `informationServices`.** Named and
  excluded in Decision 1 as separate concerns with their own display
  questions.
- **No new `DataFreshness` banner integration.** This is slow-changing
  reference data cached the same way `getStationName`/`getAllTocs` already
  are; it does not participate in this app's live-data freshness signaling.
- **No renaming of `stations.accessibility` / `StationReference.
  accessibility`.** Correction 4 flags the naming collision with WCAG
  "accessibility" but this document does not rename a shipped DB column and
  struct field to resolve it — only the new frontend-facing names are
  chosen to avoid the ambiguity going forward.
- **No new Playwright/axe-core test.** `e2e/accessibility.spec.ts` already
  sweeps `/stations/{crs}` for the WCAG rule set this app tracks; the new
  section renders inside that same page and is covered by the existing
  test without any addition (see Decision 9's precedent and Testing
  approach below).
- **No i18n of category labels or humanized field names.**
- **No editing, moderation, or admin curation UI** for this data — it is
  read-only, exactly as ingested.
- **No fix for the pre-existing lack of CRS case-normalization** on
  single-station lookups (`latest_station_sample`, and now
  `station_accessibility`, both do exact `crs = $1` matching with no
  `UPPER()`). This document's new query matches the established, already-
  shipping precedent rather than fixing or diverging from it.

## Testing approach

- **Backend, non-DB unit test**: `ACCESSIBILITY_KEYS` filtering logic
  (`station_accessibility`'s in-memory filter step) can be tested against a
  hand-built `serde_json::Value` without a database — cover: an allowlisted
  key present and forwarded, a non-allowlisted key present and dropped, a
  `null`-valued allowlisted key dropped, and an accessibility object that
  is `{}` producing `{}` out.
- **Backend, `#[ignore]`d DB test** (matching `crates/api/src/data/
  reference.rs`'s own `db_tests` module convention, including its
  reserved-`Z…`-CRS-namespace fixture discipline): seed one `stations` row
  with a mix of allowlisted/non-allowlisted/null keys in `accessibility`
  and assert `station_accessibility` returns exactly the filtered set; seed
  no row for a second CRS and assert `None`.
- **Frontend, Vitest, pure renderer functions** (`frontend/lib/
  stationAccessibility.ts` — new, colocated `*.test.ts`): one case per
  Decision 6 branch — primitive, array of primitives, array of objects,
  shallow object, and a deliberately malformed/unexpected shape (e.g. a
  deeply nested array-of-arrays-of-objects) asserting it falls back to the
  raw-JSON `<Spoiler>` path rather than throwing. This is the test that
  actually defends the "never crashes on unknown JSONB shape" claim in
  Correction 2/Decision 6 — it should be written first, before the
  component, per this codebase's TDD convention.
- **Frontend, Vitest, page-level** (extending `frontend/app/stations/[crs]/
  page.test.tsx`, new `describe('StationDisruptionPage -- accessibility &
  facilities')` block mirroring the existing "sample stats by operator"
  block at lines 167-251): mock `getStationAccessibility` for each of the
  three states in Decision 9 (`ApiNotFoundError` → unavailable copy; `{}` →
  empty copy; a populated object → grouped section headings and at least
  one rendered value) plus one case reusing the malformed-shape fixture
  from the pure-function tests end-to-end through the real component tree,
  matching this same test file's existing "prefers fullCoverageStats …
  end to end through the real component tree" precedent (lines 225-250).
- **No new e2e/Playwright spec.** `e2e/accessibility.spec.ts`'s existing
  `/stations/${REAL_STATION_CRS}` sweep already exercises whatever this
  section renders for that live station at test-run time; adding a
  second, feature-specific e2e spec would duplicate that coverage for a
  section with no live-refresh or interactive behavior of its own to test.

## Open questions / risks

1. **The exact real-world shape of every allowlisted key is unverified
   against a live RDM payload.** Correction 2 is the reason this design
   leans on a generic renderer rather than typed components; that renderer
   is a hedge against the unknown, not a substitute for eventually
   fetching one real station's full `Station` object and checking it by
   hand. Worth doing before or shortly after implementation, not resolved
   by this document.
2. **Per-station payload size is unmeasured.** Only the full ~2,600-station
   aggregate (~55MB) is measured (`routes/mod.rs:83-89`); a single verbose
   station's `carParks`/`transportLinks` entries could still be large
   enough to matter for a synchronously-rendered server component. Not
   sized here.
3. **Whether the twelve-key allowlist (Decision 1) is the right cut, or
   product wants some of the excluded fields (`ticketBuying`,
   `stationMap`) surfaced later** — a real, separate product-priority call
   this document does not make.
4. **The `accessibility` naming collision with WCAG audits (Correction 4)
   is flagged, not structurally resolved** — a future contributor grepping
   `frontend/` for "accessibility" will still get both kinds of result
   mixed together for the parts of this feature that keep the wire/type
   name (`StationAccessibilityData`, `getStationAccessibility`). The UI
   copy and component filename are the only mitigations this document
   applies.
5. **No case-normalization on the new route's `crs` lookup**, consistent
   with `latest_station_sample`'s existing precedent — a pre-existing,
   shared rough edge across this and sibling single-station routes, not
   introduced or fixed here.
