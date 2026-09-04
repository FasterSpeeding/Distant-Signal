# Design: Live Departure Picker for `TrackTrainForm` ("Trip Search," Scoped)

**Status: design proposal, not approved.** This is Part 2 of the input-UX
work `docs/superpowers/specs/2026-09-03-track-a-train-input-ux-research.md`
("the research doc") scoped and sized — but it does **not** build what
that document's Part 2 sketched (a search UX over the CIF SCHEDULE feed).
This document found a different, materially smaller, already-precedented
slice of the same user problem and designs that instead. See "Relationship
to the research doc" below for exactly what changed and why.

## Why this document exists, and what it is not

The research doc's Part 2 asked "could `TrackTrainForm` offer real
service search instead of manual entry" and answered "not now" —
correctly, for the specific approach it evaluated (parsing the CIF
SCHEDULE feed: `BS`/`BX`/`LO`/`LI`/`CR`/`LT` plus STP-overlay resolution,
sized at roughly a month of work by direct comparison to `train-mcp`'s own
equivalent build). This document was written specifically to re-examine
that conclusion given a scale mismatch flagged separately: this repo's
owner has twice recently greenlit *smaller* scaffolding builds ahead of
schedule, and Part 2's CIF estimate is a full order of magnitude bigger
than either of those, so the "not now" call deserved one more honest look
before being accepted at face value.

That re-examination is answered directly in "Re-examining the research
doc's 'no cheap partial version' conclusion," below, and the answer for
the *CIF path specifically* is: **no, the research doc was right, there is
no safe smaller slice of CIF parsing.** Both of the two most obvious
ways to cut the CIF-parsing chain down (skip STP-overlay resolution; skip
`LI` intermediate-calling-point records) were checked against the same
real 2026-08-28 CIF extract this codebase's own prior research already
counted, and both turn out to be wrong often enough to disqualify them for
a "trust this enough to click it" feature, not just theoretically risky.

But a different question — not "how do we shrink CIF parsing" but "is CIF
parsing the only way to answer this" — turned up a real answer: this app
already ingests a different feed (Darwin/LDBWS live departure boards, via
`poller-ldbws`) into `station_samples`, entirely independent of the CIF
SCHEDULE feed, and that data already carries everything
`TrackTrainForm`'s four fields need for departures at the ~286 stations it
covers. Building a picker on top of *that* — not the CIF feed — is small,
real, and was in fact already named as recommended future work in an
earlier design doc that Part 2's research didn't cross-reference (see
below). This document designs that.

## Relationship to the research doc and its adjacent documents

- `docs/superpowers/specs/2026-09-03-track-a-train-input-ux-research.md`
  ("the research doc") — Part 1 (CRS/operator autocomplete) is shipped and
  is this design's baseline (`TrackTrainForm.tsx:62-64` already wires
  `useSuggestions`/`searchStations`/`searchTocs` for all three text
  fields — confirmed directly by reading the file, not assumed from the
  research doc). Part 2 evaluated one data source (CIF SCHEDULE) and
  correctly ruled it out for now; this document evaluates a second,
  different data source the research doc's Part 2 never considered, and
  reaches a different, positive conclusion for that source specifically.
  This document does not overturn the research doc's CIF-path conclusion
  — see "Re-examining..." below, which independently reconfirms it.
- `docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`
  ("the 08-29 doc") — the *original* frontend design for train tracking,
  predating both Part 1 and Part 2 of the research doc. Its Decision 1
  explicitly considered and rejected a per-departure "click to track" flow
  for v1, **for exactly one stated reason**: "no UI or API surface exists
  today to list individual departures" (`2026-08-29-train-tracking-frontend-design.md:216-217`).
  Its "Explicitly out of scope" section names the fix precisely: *"Blocked
  on a new public backend read endpoint exposing
  `station_samples`/`StationDeparture[]` (e.g. `GET /StopPoint/{crs}/Departures`,
  reusing the already-written `latest_station_sample` query function,
  currently internal-only) plus a real departure-board component... recommended
  as the natural fast-follow once this v1 form-based entry point ships and
  usage patterns are known"* (`:539-546`). That v1 shipped. This document
  is that fast-follow, sized and designed concretely rather than merely
  named. The URL shape below deviates from the 08-29 doc's own
  parenthetical suggestion (`/StopPoint/{crs}/Departures`, TfL-shaped) in
  favor of the newer, now-established `/public/stations/{crs}/...`
  convention — see Decision 2.
- `docs/superpowers/specs/2026-09-03-per-station-stats-design.md` ("the
  per-station-stats doc") — landed *after* the 08-29 doc, and is the
  direct structural precedent this document reuses wholesale: the same
  `latest_station_sample` query, the same 404-vs-`200 []` honesty split,
  the same hand-built-`json!`-to-avoid-nested-snake_case pattern
  (`station_stats.rs`, `render.rs`'s extracted `sample_stats_json`/
  `sample_availability_json`), and the same three-state frontend
  degradation shape. This document's new route sits directly beside
  `station_stats.rs` as a sibling, not a variant of it — different data
  (raw departures, not computed stats) but identical conventions.
- `docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md`
  Decision 5, and `train-mcp.zip` (present at the repo root, a full prior
  build of a comparable CIF-parsing + journey-planning system, previously
  investigated in full by `docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`)
  — both re-confirmed, not re-litigated, in "Re-examining..." below. Not
  extracted or read further this pass; its prior investigation already
  covers everything relevant here.

## Re-examining the research doc's "no cheap partial version" conclusion

Two concrete simplifications of the CIF-parsing path were checked against
the same figures the prior research already gathered from the real
2026-08-28 `timetable_full.zip` sample
(`2026-09-01-schedule-ingest-stanox-crs-table-design.md:501-529`), plus one
external option:

1. **Skip STP-overlay resolution; parse only `P` (Permanent) `BS` records,
   ignore `O`/`N`/`C`.** Fails concretely, not just theoretically: the
   prior research's own count found **81,162 `C`-indicator
   (cancellation-of-permanent) records** in the sample extract
   (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:516-517`) — each
   one exists specifically to say "the permanent schedule with this UID
   does *not* run on this date." Ignoring STP resolution and showing every
   `P` schedule as if it always runs means every one of those 81K+
   suppressions would be silently wrong — the app would list trains as
   departing that Network Rail's own data says are cancelled that day,
   concentrated exactly on the bank holidays and engineering-possession
   weekends when users most need this to be right. Not a rare edge case;
   a large, load-bearing fraction of the schedule set. Ruled out.
2. **Skip `LI` (intermediate calling points); parse only `BS`/`BX`/`LO`/`LT`
   (a schedule's origin and terminus only).** This does cut real parsing
   *volume* substantially — `LI` accounts for roughly 6.8M of the ~8.6M
   `MCA` schedule-record lines the prior research counted
   (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:512-513,519-521`),
   i.e. most of the file. But it fails the feature's actual use case: a
   CIF schedule's `LO`/`LT` only cover the two stations where a train
   *starts* and *ends its journey as one continuous schedule record* —
   the origin/destination search this feature needs is "what departs from
   station X," and for the large majority of real stations (anywhere that
   isn't itself a literal service terminus — every ordinary intermediate
   stop) the departures a user actually wants are `LI` rows, not `LO`/`LT`
   rows. Dropping `LI` doesn't narrow the feature to a smaller-but-honest
   subset the way dropping STP overlays for "permanent-schedule-only, name
   the limitation" might in principle — it silently returns nothing (or
   wildly incomplete results) for most non-terminus stations, which is a
   worse failure mode than the STP-skip option above, not a better one.
   Ruled out.
3. **Adopt an existing Rust CIF-parsing crate instead of building the
   parser.** Checked directly (not something the research doc looked for):
   [`nr-cif`](https://crates.io/crates/nr-cif) (`lilopkins/nr-cif-rs`,
   MIT-licensed) does exist and does parse the full record set
   (`BS`/`BX`/`LO`/`LI`/`CR`/`LT`/`TI`/`AA`, confirmed by reading
   `src/schedule.rs`/`src/parser.rs` directly from its GitHub repo) into a
   `ScheduleDatabase { schedules: HashMap<UID, Vec<Schedule>> }`. This is a
   real, meaningful find — it would remove a large fraction of the raw
   mechanical parsing labor the "roughly a month" estimate priced in. It
   does **not**, however, change the two dominant remaining costs: (a) its
   own doc comment states plainly that STP resolution is the *caller's*
   job — *"these should be filtered by the validity date... then you
   should get the one at the latest index valid"*
   (`schedule.rs:56-58`, paraphrased from the field doc comment) — the
   genuinely stateful `P`/`O`/`N`/`C` precedence algorithm the prior
   research flagged as the hard part is not solved by this crate, only
   made buildable on top of; and (b) it has no schema, ingest pipeline, or
   query layer at all — it is an in-memory, single-process parsing
   library, not a Postgres-backed service matching this app's own
   convention (`schedule-reference`'s existing shape). Its own maintenance
   signal is also a real adoption risk for a feature this codebase would
   depend on for correctness: last published 2023-12-14 (roughly 2 years
   9 months stale as of this writing), 0 GitHub stars/watchers, ~32
   downloads in the last 90 days — a single-maintainer project with no
   visible ongoing activity. Net effect: real, but doesn't flip the
   month-scale estimate to a subagent-chain-sized one — the schema/ingest/
   STP-resolution/query work, which this design's own reading (above)
   confirms is where the real cost sits, is entirely unaffected by whether
   the record-parsing layer is written in-house or imported.

**Conclusion for the CIF path specifically: the research doc's "no cheap
partial version" holds**, now with concrete evidence (the 81,162-record
STP count; the ~6.8M-of-8.6M-line `LI` share; `nr-cif`'s real but partial
coverage) rather than a restatement of the prior high-level sizing. If CIF
parsing is ever pursued, `nr-cif` is worth a second look as a
parsing-layer accelerant, but it is not a reason to schedule the project
now — see Explicitly out of scope.

**What this document does instead** is answer a different question:
does `TrackTrainForm` need CIF schedule data at all to offer a real
search/pick UX, or does this app already have a different, live data
source that answers the same user need for a meaningfully-sized subset of
stations? It does — see below.

## Current relevant state

- `common::StationDeparture` (`crates/common/src/lib.rs:403-427`) — one
  row per live Darwin/LDBWS service, already storing `service_id`,
  `operator`, `destination_crs`, `scheduled` (`std`, an `"HH:MM"` string),
  `estimated` (`etd`, `"On time"`/`"Cancelled"`/`"HH:MM"`), `is_cancelled`,
  `delay_minutes`, optional `cancel_reason`/`delay_reason`, `headcode`
  (always `None` — confirmed by `poller-ldbws/src/schema.rs:104-105`'s own
  comment: *"confirmed absent from this API's schema entirely"*), and
  `skipped_stations`. This is exactly the field set `TrackPinRequest`'s
  optional `destination_crs`/`operator` and required `scheduled_departure`
  need, minus a date component (handled below, Decision 4).
- `common::StationSample` (`:565-569`) — `{ crs, polled_at,
  departures: Vec<StationDeparture> }`, one row per CRS in `station_samples`,
  wholesale-replaced each poll cycle.
- `queries::latest_station_sample` (`crates/api/src/data/queries.rs:681-697`)
  — proven, shipping, single-CRS on-demand read. Already used twice:
  `station_stats.rs` (aggregated stats) and `train.rs`'s `blend_darwin_eta`
  (`train.rs:512-540`, ETA overlay for an already-tracked pin). This
  document's new route is a third, simpler caller: no aggregation, no
  matching against an existing pin — just the raw list.
- **`poller-ldbws`'s scope, confirmed directly**: requests `num_rows: 10`
  departures per station by default (`crates/poller-ldbws/src/config.rs:45-46`),
  polls on a `poll_interval_secs` cycle (default 60s per that file), and
  only covers the ~286 CRS codes that are some line's `sample_stations`
  entry (`docs/superpowers/specs/2026-09-03-per-station-stats-research.md`'s
  "Polling scope" finding, reused verbatim by the per-station-stats
  design's Goal section — not re-measured here, same figure). Every other
  GB station has no row in `station_samples` at all.
- **`parse_departures` never sorts** (`crates/poller-ldbws/src/schema.rs:106-131`)
  — departures are stored in whatever order RDM's `GetDepBoardWithDetails`
  response returned them (conventionally chronological, per how every
  Darwin-derived departure board on the actual National Rail site
  renders), not re-sorted by this app. This document's new route
  preserves that order rather than inventing a new one — see Decision 2.
- **`TrackPinRequest`** (`crates/common/src/lib.rs:580-588`) — `service_date:
  NaiveDate`, `origin_crs: String`, `scheduled_departure: DateTime<Utc>`,
  optional `destination_crs`/`operator`. Its own doc comment is direct
  prior-art confirmation for this document's whole approach: *"the pinned
  service is only ever known by what a departure-board view already has...
  never by a durable train identity at pin time"* (`:571-578`) — this
  schema was **already designed** around being filled from a live
  departure-board row, not a CIF schedule lookup. Nothing about
  `TrackPinRequest` needs to change for this design.
- **`TrackTrainForm.tsx`, exactly as it stands today** (Part 1 already
  shipped): `originCrs`/`destinationCrs`/`operator` are `Autocomplete`
  fields already wired to `useSuggestions`/`searchStations`/`searchTocs`
  (`:54-64,144-211`). `scheduledDeparture` is a `DateTimePicker` plus a
  "Now" button (`:159-186`) that sets it via
  `dayjs().format('YYYY-MM-DD HH:mm:ss')` — the exact local-wall-clock
  string shape `handleSubmit`'s own comment explains in detail
  (`:77-97`): the picker's value is a local-time string, not ISO 8601, and
  `service_date` is read as that string's first 10 characters specifically
  to avoid an around-local-midnight day-off-by-one. This exact string
  shape is what Decision 4 below reuses for a picked departure, instead of
  re-deriving date/timezone handling.
- **Client-side fetches from `TrackTrainForm` (a Client Component) already
  go through the same-origin `/api/*` catch-all proxy**
  (`frontend/lib/suggestions.ts:3-8`'s own comment: *"Client Components
  can't read the server-only `API_BASE_URL`"*), which auto-prepends
  `/public/` to any path segment other than `Train` (`app/api/[...path]/route.ts`'s
  `resolveTargetPath`, confirmed by reading the file). A new
  `/public/stations/{crs}/departures` backend route therefore needs **zero**
  proxy changes — `fetch('/api/stations/{crs}/departures')` already reaches
  it, exactly as `searchStations`'s `fetch('/api/stations?q=...')` already
  reaches `/public/stations`.
- **`crates/api/src/render.rs`'s hand-built-JSON convention** and the
  documented reason for it (`incidents.rs:53-59`'s nested-snake_case
  pitfall) — reused verbatim, not re-derived; see Decision 2.

## Goal

Add a live, real-departures picker to the existing `/track` form: once a
user has entered a valid Origin CRS (already autocomplete-assisted by Part
1), show today's next scheduled departures from that station — if it's one
of the ~286 stations this app already samples — and let picking one
auto-fill Destination, Operator, and Scheduled departure, leaving the user
to review and submit. Every other station falls back to exactly today's
manual-entry experience, unchanged. No new data pipeline, no new table, no
CIF parsing, no journey-planning algorithm — this reuses `station_samples`
precisely as it already exists.

## Decisions

### 1. Scope: the ~286 already-sampled stations only; every other station gets an honest, unchanged manual-entry fallback

No broadening of `poller-ldbws`'s polling scope (a materially separate,
unsized piece of work the per-station-stats research doc already named
and declined to size — RDM rate limits, per-cycle time budget). This
document inherits that same boundary rather than re-opening it. A station
outside the sampled set is not an error state — it's simply "no picker
available here," rendered as a plain sentence, with the existing manual
`Autocomplete`/`DateTimePicker` fields fully functional beneath it exactly
as they are today. This mirrors the per-station-stats design's own
Decision 1 posture (an honest partial-coverage feature, not a fake
full-coverage one) rather than blocking on broadening coverage first.

### 2. New endpoint: `GET /public/stations/{crs}/departures`, raw `StationDeparture[]`, 404-vs-`200 []` honesty, hand-built camelCase JSON

```rust
// crates/api/src/routes/departures.rs (new file)

//! `GET /public/stations/{crs}/departures`: today's live departure board
//! for `crs`, straight from `station_samples`, no aggregation. Backs the
//! trip-search picker on `/track`
//! (docs/superpowers/specs/2026-09-03-trip-search-design.md). Sibling to
//! `station_stats.rs`, same `latest_station_sample` read, same honesty
//! split, deliberately not merged into that file: this returns raw rows
//! for a picker, not computed per-operator stats -- different callers,
//! different wire shapes, no shared logic beyond the DB read itself.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::queries;
use crate::render::station_departure_json;

pub fn router() -> Router {
    Router::new().route(
        "/stations/{crs}/departures",
        axum::routing::get(get_station_departures),
    )
}

/// 404 when `station_samples` has no row for `crs` at all -- identical
/// honesty split to `station_stats.rs::get_station_sample_stats`. `200 []`
/// is the same "row exists, board is genuinely empty right now" fact that
/// route already draws.
async fn get_station_departures(
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

    // Order preserved exactly as stored -- `parse_departures` never
    // re-sorts (poller-ldbws/src/schema.rs), and RDM's own board is
    // already chronological by convention. No new sort introduced here.
    Ok(Json(sample.departures.iter().map(station_departure_json).collect()))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "station departures query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}
```

Registered in `crates/api/src/routes/mod.rs`: add `pub mod departures;`
and `.merge(departures::router())` inside `public_router()`, alongside
`station_stats::router()`.

**Wire shape deliberately mirrors `StationDeparture`'s own fields,
camelCased, hand-built** — same rationale as `station_stats.rs`'s
`sample_stats_json`/`sample_availability_json` (`render.rs:435-453`): a
`#[derive(Serialize)] #[serde(rename_all = "camelCase")]` wrapper around
`common::StationDeparture` directly would still emit `StationDeparture`'s
own un-renamed field names one level down (`incidents.rs:53-59`'s
documented pitfall). New extracted helper:

```rust
// crates/api/src/render.rs -- new function, alongside sample_stats_json/
// sample_availability_json

pub(crate) fn station_departure_json(d: &common::StationDeparture) -> serde_json::Value {
    serde_json::json!({
        "serviceId": d.service_id,
        "operator": d.operator,
        "destinationCrs": d.destination_crs,
        "scheduled": d.scheduled,
        "estimated": d.estimated,
        "isCancelled": d.is_cancelled,
        "delayMinutes": d.delay_minutes,
        "cancelReason": d.cancel_reason,
        "delayReason": d.delay_reason,
        "skippedStations": d.skipped_stations,
    })
}
```

`headcode` is deliberately omitted — always `None` at the source
(`poller-ldbws/src/schema.rs:104-105`), and `TrackPinRequest` has no field
for it anyway, so there is nothing to carry through.

Example response, `GET /public/stations/WAT/departures`:

```json
[
  { "serviceId": "abc123", "operator": "SW", "destinationCrs": "WOK",
    "scheduled": "14:32", "estimated": "On time", "isCancelled": false,
    "delayMinutes": 0, "cancelReason": null, "delayReason": null,
    "skippedStations": [] },
  { "serviceId": "def456", "operator": "SW", "destinationCrs": "BSK",
    "scheduled": "14:40", "estimated": "14:47", "isCancelled": false,
    "delayMinutes": 7, "cancelReason": null, "delayReason": "signalling problem",
    "skippedStations": [] }
]
```

### 3. Cancelled departures: shown, not filtered, but not selectable

A cancelled `StationDeparture` is real information a user browsing "what's
departing from here" should see (it explains a gap, matches what the
actual station board shows), but tracking a cancelled service makes no
sense as "the trip I want to follow." The endpoint returns cancelled
departures unfiltered (no server-side opinion baked into the wire shape);
the frontend picker (Decision 4) renders them with a "Cancelled" badge and
disables the pick action for that row specifically, rather than hiding
them or letting a user select a service that has already been called off.

### 4. Frontend: an inline picker on `/track`, reusing the exact date/time-string construction the "Now" button already establishes

New state and effect in `TrackTrainForm.tsx`, gated on the origin field
resolving to a syntactically valid CRS (`originValid`, already computed):

```tsx
// New: fetch departures whenever originCrs becomes/stays a valid 3-letter
// code, via the same same-origin proxy pattern searchStations/searchTocs
// already use (client-safe, no baseUrl() import).
const [departures, setDeparturesState] = useState<DepartureRow[] | 'not-sampled' | null>(null);

useEffect(() => {
  if (!originValid) {
    setDeparturesState(null);
    return;
  }
  const controller = new AbortController();
  fetch(`/api/stations/${originCrs.trim().toUpperCase()}/departures`, { signal: controller.signal })
    .then((res) => {
      if (res.status === 404) return setDeparturesState('not-sampled');
      if (!res.ok) return setDeparturesState(null);
      return res.json().then(setDeparturesState);
    })
    .catch(() => {}); // aborted or network blip -- leave prior state, same posture as useSuggestions
  return () => controller.abort();
}, [originCrs, originValid]);
```

Rendered as a new block directly below the Origin field, three states
(mirroring the per-station-stats design's Decision 9 three-state
convention, applied here instead to a picker rather than a stats
summary):

- `departures === null` (origin not yet valid, or a transient fetch
  failure): nothing rendered — the manual fields below are the whole
  story, exactly as today.
- `departures === 'not-sampled'`: *"Live departures aren't available to
  browse for this station — enter the details below."* — an explicit,
  honest statement of Decision 1's scope boundary, not a blank gap.
- `departures` is an array: a scrollable list, one row per departure —
  `scheduled`, `destinationCrs` (resolved to a name via the same
  `originSuggestions`-shaped station lookup already in scope, falling back
  to the bare code), `operator`, and a delay/cancelled badge
  (`isCancelled` → "Cancelled" in red; `delayMinutes > 0` → "+N min" in
  amber; otherwise "On time"). An empty array renders *"No live
  departures currently on the board for this station right now."*

Picking a non-cancelled row fills the existing fields, **without
submitting**, so the user can still review/edit before tracking:

```tsx
function pickDeparture(row: DepartureRow) {
  setDestinationCrs(row.destinationCrs);
  setOperator(row.operator);
  // Same local-wall-clock string shape the "Now" button already produces
  // (dayjs().format('YYYY-MM-DD HH:mm:ss')) -- combines *today's* browser-
  // local date with the departure's "HH:MM" scheduled time. This accepts
  // the same browser-local-date assumption the "Now" button already makes
  // (not Europe/London specifically) -- not a new limitation this design
  // introduces. See Open questions for the narrow near-midnight edge case.
  const [hh, mm] = row.scheduled.split(':');
  const today = dayjs().format('YYYY-MM-DD');
  setScheduledDeparture(`${today} ${hh}:${mm}:00`);
}
```

No backend timezone conversion is needed for this — `eta_blend.rs`'s
`london_to_utc` (`crates/api/src/data/eta_blend.rs:44-56`) is proven prior
art that a server-side HH:MM+date→UTC conversion is safe and already
solved in this codebase *if* a future caller needs one, but this design
doesn't need it: the picker only ever fills the form's own state, and
`handleSubmit`'s existing, unmodified logic (`TrackTrainForm.tsx:77-97`)
does the local-string→ISO conversion exactly as it already does for a
manually-typed or "Now"-button-set value. Zero changes to `handleSubmit`.

### 5. Naming

- Rust: `routes::departures::router`/`get_station_departures` (new route
  module), `render::station_departure_json` (new extracted helper,
  alongside the existing `sample_stats_json`/`sample_availability_json`).
- Wire: `serviceId`, `operator`, `destinationCrs`, `scheduled`,
  `estimated`, `isCancelled`, `delayMinutes`, `cancelReason`,
  `delayReason`, `skippedStations` — camelCase versions of
  `StationDeparture`'s own field names, unchanged in meaning.
- Frontend: `DepartureRow` (type, mirrors the wire shape), `departures`/
  `setDeparturesState` (component state), `pickDeparture` (the fill
  handler).

## Architecture

```
/track form, Origin CRS becomes a valid 3-letter code
        │
        ▼
fetch('/api/stations/{crs}/departures')   (same-origin proxy, unchanged)
        │
        ▼
GET /public/stations/{crs}/departures      (crates/api/src/routes/departures.rs, NEW)
        │
        ▼
queries::latest_station_sample(pool, crs)  ← already shipping, unmodified
        │  (Option<StationSample>)
        ▼
sample.departures.iter().map(station_departure_json)   ← render.rs, NEW helper
        │
        ▼
frontend: departures list rendered below Origin field
        │  user clicks a non-cancelled row
        ▼
pickDeparture() sets destinationCrs/operator/scheduledDeparture state
        │  (existing handleSubmit, UNCHANGED, fires on user's own submit)
        ▼
POST /Train/track  (existing route, existing TrackPinRequest shape, unchanged)
```

No write path is added anywhere. `poller-ldbws`, `aggregator`, and every
existing route are untouched except the new route file and its
registration in `mod.rs`.

## Error handling

- **Database error**: `500`, logged — identical shape to
  `station_stats.rs`'s sibling route.
- **No `station_samples` row for `crs`**: `404` → frontend's "not
  available to browse" state, manual fields unaffected.
- **Row exists, zero departures**: `200 []` → frontend's "no live
  departures right now" state, manual fields unaffected.
- **Frontend fetch failure that isn't a 404** (network blip, aborted by a
  fast-typed origin change): `departures` state left at its prior value
  (or `null` on first failure) — the manual fields are always usable
  regardless, so there is no broken/blocked state this can produce, only
  "the picker doesn't show up this time."
- **User picks a departure, then keeps editing the Origin field**: the
  effect's cleanup (`AbortController`) cancels the in-flight fetch for the
  old origin; already-filled Destination/Operator/Scheduled-departure
  values are **not** cleared automatically — same posture as every other
  field in this form today (nothing auto-clears a field the user has
  already set), left as a user-visible "you picked from a station you've
  now changed" state rather than surprising the user with a silent reset.

## Testing

- **`crates/api`**: a `oneshot`-probe path test plus `#[ignore]`-gated
  DB-backed tests for `GET /public/stations/{crs}/departures` — 404 for no
  row, `200 []` for an empty row, and a populated-row test asserting exact
  camelCase field names and a cancelled entry passing through unfiltered
  — following `station_stats.rs`'s exact test-fixture pattern
  (`INSERT INTO station_samples ... ON CONFLICT`, `delete_fixture`
  cleanup, `#[ignore = "requires a live database..."]`).
- **`crates/api/src/render.rs`**: a unit test for `station_departure_json`
  asserting the exact camelCase field mapping, including the `null`
  encoding of `cancel_reason: None`/`delay_reason: None`.
- **`frontend`**: a unit test for the new fetch effect (mock `fetch`,
  assert the three states — not-sampled/empty/populated — render the
  right copy), and a test for `pickDeparture` asserting it produces the
  exact `'YYYY-MM-DD HH:mm:ss'` string shape `handleSubmit` expects
  (constructed via a fixed/mocked `dayjs()` "now," same technique this
  file's existing "Now"-button test, if any, already uses — check
  `TrackTrainForm.test.tsx` for the established pattern before adding a
  new one).

## Explicitly out of scope

- **Any CIF SCHEDULE-feed parsing, STP-overlay resolution, or a
  `schedules`/`calling_points` schema.** Re-examined this pass (see
  above) and reconfirmed not worth it now — a real, separate,
  month-scale project if ever pursued, unaffected by this document.
- **Browsing future dates, or any date other than "the live board as it
  exists right now."** `station_samples` only ever holds the current poll
  cycle's board. A user wanting to track a train departing tomorrow still
  uses the manual fields, exactly as today.
- **Headcode search** (research doc Part 2's shape 2). Not just
  deprioritized — genuinely unbuildable from this data source, since
  `headcode` is always `None` at the LDBWS source
  (`poller-ldbws/src/schema.rs:104-105`).
- **Broadening `poller-ldbws`'s polling scope beyond the current ~286
  `sample_stations`-derived list.** Named but not sized by the
  per-station-stats research doc; unaffected by this document, which
  stays scoped to whatever's already sampled (Decision 1).
- **A standalone departures endpoint/component on `/stations/[crs]`
  itself**, distinct from the `/track` picker. The 08-29 doc's own
  "fast-follow" language named a station-page departure board as part of
  the eventual richer picture; this document deliberately ships only the
  `/track`-form picker first, since that's the smaller, more directly
  useful slice for the stated goal (fewer typed fields when tracking), not
  a general-purpose departure-board feature. The new `GET
  /public/stations/{crs}/departures` route is generic enough that a future
  station-page board could reuse it unchanged, but building that UI is not
  this document's job.
- **Pagination or a wider time window than `poller-ldbws`'s own
  `num_rows` cap (10 by default).** Whatever the poller already fetches is
  what this picker shows — no new querying/windowing logic invented on
  top of what's already stored.
- **Sorting/re-ordering `station_samples.departures`.** Preserved in
  storage order, per Decision 2 — no new sort key introduced.
- **Any change to `TrackPinRequest`, `validate_pin`, or `POST
  /Train/track`.** This document only ever fills the existing form's
  existing fields; the backend pin-creation contract is untouched.

## Open questions / risks

1. **Near-midnight day boundary.** `pickDeparture` combines a departure's
   `"HH:MM"` with the browser's *current local calendar date*. A
   departure shown as, say, `"00:12"` fetched at 23:58 local time belongs
   to the *next* calendar day, but this construction would date it today,
   producing a `scheduled_departure` roughly 24 hours off. This is the
   same class of risk the existing "Now" button already accepts (per
   `handleSubmit`'s own comment on browser-local-date, not
   Europe/London-specific), and the practical window is narrow —
   `poller-ldbws`'s ~10-departure, ~60-second-refreshed board rarely
   straddles midnight with a picked row that far off from "now" — but it
   is a real, unquantified risk this document flags rather than
   engineers around, matching this codebase's own posture on
   similarly-narrow, previously-accepted edge cases (e.g. `MAX_PIN_AGE`'s
   own unresearched-constant precedent).
2. **Whether `station_departure_json`'s wire shape should be reused for a
   future station-page departure board** (out of scope above) is left
   open — this document's shape is picker-appropriate (enough to fill
   `TrackPinRequest`) but a general departure-board UI might want more
   (e.g. platform, if `poller-ldbws` ever captures it — it currently
   doesn't, unconfirmed either way this pass).
3. **Whether cancelled departures should still be selectable with a
   confirmation** ("track this train even though it's showing cancelled
   right now, in case that changes") rather than fully disabled (Decision
   3) is a real product question this document doesn't resolve — disabled
   is the safer, simpler default and what's designed above, but a case
   could be made either way and isn't researched further here.
