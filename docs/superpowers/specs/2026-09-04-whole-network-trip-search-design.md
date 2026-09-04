# Design: Whole-Network CIF-Derived Departures Fallback for `TrackTrainForm`

**Status: design proposal, not approved.** This is the concrete, buildable
spec the research doc
(`docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md`,
"the research doc") asked for — turning its Recommendation section into
exact wire shapes, table schemas, and code sketches. It is a **second,
additive extension** of the already-shipped LDBWS picker
(`docs/superpowers/specs/2026-09-03-trip-search-design.md`, "the LDBWS
design doc"), not a replacement or a rewrite of it. Per the research doc's
own call: ship as **fallback-only** — a CIF-derived picker appears only
where the LDBWS picker already returns "not sampled," extending real
coverage from ~286 LDBWS-sampled stations to the ~2,500-station national
network the CIF `SCHEDULE` feed and `stanox_crs` table already cover, with
zero reconciliation of two live sources for the same station.

Required reading consumed in full before this document was written (beyond
the research doc itself, already fully consumed): `crates/schedule-reference/src/main.rs`,
`config.rs`; `crates/schedule-query/src/records.rs`, `resolve.rs`;
`crates/api/src/routes/departures.rs`, `ingest.rs`, `app.rs`;
`crates/api/src/data/queries.rs` (the `stanox_crs`/`schedule_line_population`
sections); `crates/api/src/render.rs`; `frontend/components/TrackTrainForm.tsx`
in full; `docs/superpowers/specs/2026-09-03-trip-search-design.md` in full;
`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`'s
Decision 2h; `docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md`.

## Current relevant state, grounded directly in code

- **`schedule-reference`'s real per-cycle shape** (`main.rs:98-170`,
  `poll_once`): reads `TI`/`A` records, resolves `Vec<common::StanoxCrsRecord>`
  (`main.rs:116-149`, bound to `records`), POSTs it to `/private/stanox-crs`
  (`main.rs:151-158`), then — only after that POST succeeds and
  `last_processed_delivery` advances (`main.rs:165`) — calls
  `publish_schedule_line_population(client, config, &delivery.mca_path, internal_oauth)`
  (`main.rs:167`), which does its **own**, separate
  `read_prefixed_lines_multi`/`ScheduleIndex::from_text`
  (`main.rs:190-201`) and iterates `lines_to_publish` (`main.rs:210,241-247`),
  calling `schedules_touching` once per line (`main.rs:216`) and POSTing
  each line's population individually, one object per POST
  (`post_schedule_line_population`, `main.rs:252-273`), not a batch array.
- **The already-resolved `records: Vec<common::StanoxCrsRecord>` is real,
  in scope, and thrown away** at the end of `poll_once` — never passed into
  `publish_schedule_line_population`, which has no TIPLOC→CRS mapping
  available to it at all today, only TIPLOC→line membership via
  `config.lines`.
- **`schedule_query::records::CallingPoint`** (`records.rs:137-145`):
  `{ tiploc: String, kind: CallingPointKind, booked_arrival: Option<NaiveTime>,
  booked_departure: Option<NaiveTime>, is_half_minute_arrival: bool,
  is_half_minute_departure: bool }`. No CRS, no operator, no headcode —
  confirmed exhaustively by the research doc's Part 1 and independently
  reconfirmed here by reading the same file. `CallingPointKind::Origin`/
  `Intermediate` carry `booked_departure`; `Terminate` never does
  (`records.rs:98-107`'s own doc: "Terminate — Arrival only, no departure").
- **`schedules_touching`** (`resolve.rs:77-96`) already does one
  `index.by_uid.iter().filter_map(resolve_for_date).filter(!cancelled)` pass
  — O(all UIDs) — then filters to schedules touching a *specific* TIPLOC
  set. `resolve_for_date` (`resolve.rs:42-57`) is the STP `min_by_key`
  resolution the whole crate is built around; both are pure, no I/O,
  reusable unchanged.
- **`/public/stations/{crs}/departures`** (`crates/api/src/routes/departures.rs:1-54`):
  `queries::latest_station_sample` → 404 if no `station_samples` row at
  all, `200 []` if the row exists but is empty, else
  `sample.departures.iter().map(station_departure_json)`
  (`render.rs:140-153`). This exact 404-vs-`200 []` honesty split is the
  pattern the new endpoint reuses verbatim.
- **`TrackTrainForm.tsx`'s existing effect** (`:94-108`): fetches
  `/api/stations/{crs}/departures` whenever `originValid` is true; `404` →
  `setDepartures('not-sampled')`; non-OK → `setDepartures(null)`; else
  `res.json().then(setDepartures)`. The `'not-sampled'` branch renders a
  plain sentence (`:215-219`) and nothing else — today, a dead end. `DepartureRow`
  (`:21-32`) has `operator`, `estimated`, `isCancelled`, `delayMinutes`,
  `cancelReason`, `delayReason`, `skippedStations` — none of which a
  CIF-derived row can honestly populate.
- **Scale precedent already running in production at almost exactly this
  document's target scale**: `stanox_crs` is a real **~3,100-row** table
  (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:521`), rebuilt
  and fully upserted **every single `schedule-reference` cycle** (30 min)
  via `queries::upsert_stanox_crs` (`queries.rs:648-678`) — a `for` loop of
  individual `INSERT ... ON CONFLICT` statements inside one transaction,
  driven by one batch-array POST (`common::ingest::post_batch`,
  `main.rs:151-158`). **This means the research doc's "~23x more
  per-cycle rows" scale-up (~2,500 vs ~110) is not a hypothetical new
  order of magnitude for this system — it is the same order of magnitude
  as a table this exact service already writes in full, every cycle,
  today**, addressed concretely in Decision 4.
- **Decision 2h's precedent** (`2026-09-04-option-b-live-consumer-design.md:436-518`):
  the already-approved pattern for "a second, differently-keyed output
  computed from the same underlying pass, not a second parse or a second
  consumer" — there, `full-coverage-consumer` buckets the *same* matched
  TRUST events by `(crs, toc_id)` alongside its existing `(line_id, uid)`
  bucketing. This document's Decision 1 is the direct structural analogue,
  applied to `schedule-reference`'s existing CIF parse instead of
  `full-coverage-consumer`'s TRUST correlation: one already-built
  `ScheduleIndex`, one additional grouping pass, a second published
  product.

## Goal

Once `TrackTrainForm`'s existing LDBWS-backed picker reports `'not-sampled'`
for a station, attempt a second fetch against a new CIF-derived endpoint
before falling back to the plain manual-entry sentence. On success, show a
scheduled-departures picker — visibly and honestly distinct from the LDBWS
picker — that can auto-fill Destination and Scheduled-departure (never
Operator, which the data doesn't have). No change to the LDBWS picker
itself, no merge of the two sources for a station that has both, no
resident whole-network index anywhere, no journey-planning algorithm.

## Decisions

### 1. Where the CIF-derived per-station view gets computed: a second grouping pass over the same already-built `ScheduleIndex`, publishing a second, differently-keyed product

**Not a new service, not a resident index, not a second parse.** Restructure
`poll_once`/`publish_schedule_line_population` so the whole-network
`ScheduleIndex` build and the already-resolved `records: Vec<common::StanoxCrsRecord>`
are both shared, not independently rebuilt:

```rust
// crates/schedule-reference/src/main.rs -- sketch

// poll_once, after the existing stanox/crs resolve+POST (main.rs:116-158)
// and *before* `last_processed_delivery` advances at :165, `records` is
// already in scope, already fully resolved, and today only used once.
// This document's one change to `poll_once` itself: pass it through.

publish_cif_derived_products(client, config, &delivery.mca_path, internal_oauth, &records).await;

// New wrapper, replacing today's `publish_schedule_line_population` as the
// single entry point for BOTH CIF-derived publishes -- one read, one
// index build, two grouping passes, two publishes. Mirrors Decision 2h's
// framing exactly: "one pass over the feed producing two outputs."
async fn publish_cif_derived_products(
    client: &Client,
    config: &Config,
    mca_path: &Path,
    internal_oauth: &OAuthTokenCache,
    stanox_crs_records: &[common::StanoxCrsRecord],
) {
    let mca_schedule_text = match read_prefixed_lines_multi(mca_path, &["BS", "BX", "LO", "LI", "CR", "LT"]) {
        Ok(text) => text,
        Err(err) => { tracing::error!(error = ?err, "..."); return; }
    };
    let index = schedule_query::ScheduleIndex::from_text(&mca_schedule_text); // ONE build
    let today = chrono::Utc::now().date_naive();

    publish_schedule_line_population(client, config, &index, today, internal_oauth).await; // existing, unchanged logic, now takes &index instead of rebuilding it
    publish_schedule_network_departures(client, config, &index, today, stanox_crs_records, internal_oauth).await; // new
}
```

`publish_schedule_line_population` keeps its existing per-line loop and
per-line individual-object POST unchanged (Decision 4 explains why this new
publish deliberately does *not* copy that POST shape) — only its own
internal `read_prefixed_lines_multi`/`ScheduleIndex::from_text` calls move
up into the shared wrapper.

**The new grouping function itself lives in `schedule-query`, pure, no
I/O** — same crate, same convention `schedules_touching` already
establishes:

```rust
// crates/schedule-query/src/resolve.rs -- new function

/// Every non-cancelled, resolved schedule's departure-bearing calling
/// points (`Origin`/`Intermediate`, i.e. `booked_departure.is_some()` --
/// `Terminate` never has one, `records.rs:98-107`), bucketed by CRS via
/// `tiploc_to_crs` (normalized-TIPLOC keyed, built by the caller from the
/// SAME cycle's already-resolved `stanox_crs` rows -- no second lookup
/// table, no new parse). A calling point whose TIPLOC has no `tiploc_to_crs`
/// entry is dropped, not guessed at -- see this function's own return type
/// doc for why that's an honest, bounded gap, not a silent one.
///
/// `now`: only calling points with `booked_departure >= now` are kept --
/// this is what keeps a station's bucket naturally small AND naturally
/// forward-looking without an arbitrary unbounded "whole day" list, see
/// the design doc's Decision 4. One O(all UIDs) resolve pass +
/// O(total calling points) bucketing -- the same complexity class
/// `schedules_touching` already pays per line, done once for the whole
/// network instead of once per line.
pub fn departures_by_crs(
    index: &ScheduleIndex,
    date: NaiveDate,
    now: NaiveTime,
    tiploc_to_crs: &HashMap<String, String>,
) -> HashMap<String, Vec<ScheduleDeparture>> {
    let mut by_crs: HashMap<String, Vec<ScheduleDeparture>> = HashMap::new();
    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else { continue };
        if resolved.cancelled { continue; }
        for cp in &resolved.calling_points {
            let Some(departure) = cp.booked_departure else { continue };
            if departure < now { continue; }
            let Some(crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else { continue };
            let destination_crs = resolved
                .calling_points
                .last()
                .and_then(|last| tiploc_to_crs.get(normalize_tiploc(&last.tiploc)))
                .cloned();
            by_crs.entry(crs.clone()).or_default().push(ScheduleDeparture {
                uid: resolved.uid.clone(),
                scheduled: departure,
                destination_crs,
            });
        }
    }
    by_crs
}

/// One CIF-derived departure -- deliberately narrower than
/// `LinePopulationEntry` (no `calling_points`, no full pattern; see
/// Decision 2). Wire type shared between `schedule-reference` (producer)
/// and `crates/api` (consumer, opaque-JSONB storage only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleDeparture {
    pub uid: String,
    pub scheduled: NaiveTime,
    pub destination_crs: Option<String>,
}
```

`schedule-reference`'s own publish function caps each bucket, then batches
the whole cycle's rows into **one array POST**, not one POST per CRS:

```rust
// crates/schedule-reference/src/main.rs -- sketch

const MAX_DEPARTURES_PER_STATION: usize = 10; // mirrors poller-ldbws's own
                                                // num_rows=10 default,
                                                // config.rs:45-46

async fn publish_schedule_network_departures(
    client: &Client, config: &Config, index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate, stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &OAuthTokenCache,
) {
    let tiploc_to_crs: HashMap<String, String> = stanox_crs_records.iter()
        .map(|r| (schedule_query::normalize_tiploc(&r.tiploc).to_string(), r.crs.clone()))
        .collect();
    let now = local_london_time_now(); // see Open Questions -- CIF times are
                                        // local civil time, not UTC; naive
                                        // Utc::now().time() would be wrong
                                        // during BST
    let mut by_crs = schedule_query::departures_by_crs(index, today, now, &tiploc_to_crs);

    let rows: Vec<serde_json::Value> = by_crs.drain().map(|(crs, mut departures)| {
        departures.sort_by_key(|d| d.scheduled); // earliest-first
        departures.truncate(MAX_DEPARTURES_PER_STATION);
        serde_json::json!({ "crs": crs, "service_date": today, "departures": departures })
    }).collect();

    if let Err(err) = post_schedule_network_departures(client, &config.schedule_network_departures_url, internal_oauth, &rows).await {
        tracing::error!(error = ?err, "failed to publish schedule-derived network departures; will retry next cycle");
    }
}
```

**New migration**, copy-adjacent to `schedule_line_population`'s own
(`crates/api/migrations/20260904090000_schedule_line_population.sql`):

```sql
-- crates/api/migrations/20260904120000_schedule_network_departures.sql
CREATE TABLE schedule_network_departures (
    crs          TEXT        NOT NULL,
    service_date DATE        NOT NULL,
    departures   JSONB       NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (crs, service_date)
);
```

**One new private route, POST only** — deliberately **not** a POST/GET
pair like `/schedule-line-population`. That route needs a GET because a
*different service* (`full-coverage-consumer`) reads it back over HTTP; here,
the only reader is `crates/api` itself, serving the public passthrough
directly off Postgres via `queries::latest_schedule_network_departures`,
exactly how `station_samples` already works for `/departures` — no
service-to-service GET round trip is needed at all. This is one route and
one credential fewer than the research doc's Part 3(c) sketch, found by
reading `app.rs`'s real OAuth-route table (`app.rs:130-193`) rather than
assuming the `/schedule-line-population` pair generalizes:

```rust
// crates/api/src/routes/ingest.rs -- new, batch-array POST like /stanox-crs
// (main.rs:151-158's `post_batch`), NOT the one-object-per-POST shape
// `/schedule-line-population` uses -- see Decision 4 for why.
#[derive(Debug, Deserialize)]
struct ScheduleNetworkDeparturesRow {
    crs: String,
    service_date: chrono::NaiveDate,
    departures: serde_json::Value,
}

async fn post_schedule_network_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleNetworkDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_network_departures(&app.database, &rows).await.map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

`app.rs`'s route table gains one entry, reusing `schedule-reference`'s
*existing* writer credential — no new OAuth group:

```rust
(
    "/schedule-network-departures",
    Method::POST,
    vec![config.internal_oauth_group_schedule_reference.clone()],
),
```

**One new public passthrough**, copy-adjacent to `departures.rs`
(`crates/api/src/routes/departures.rs:1-54`), same 404-vs-`200 []` split,
reading the new table directly (today's date, server-side — the picker
never asks for a different day, mirroring the LDBWS route's "always now"
posture):

```rust
// crates/api/src/routes/departures.rs -- new route in the same file,
// sibling to get_station_departures
async fn get_station_schedule_departures(
    State(app): State<App>, Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let today = chrono::Utc::now().date_naive();
    let Some(row) = queries::latest_schedule_network_departures(&app.database, &crs, today)
        .await.map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, format!("no CIF-derived schedule data for station: {crs}")));
    };
    Ok(Json(row.departures.iter().map(schedule_departure_json).collect()))
}
```

Registered at `GET /public/stations/{crs}/schedule-departures`.

### 2. Wire shape: what a CIF-derived "departure" honestly contains, and how the picker signals the difference

`ScheduleDeparture` (Decision 1) only ever carries `uid`, `scheduled`
(`NaiveTime`), and `destination_crs: Option<String>` — genuinely all
`schedule_query` can produce today (no operator field on `BasicSchedule`
at all; no headcode; `destination_crs` itself is `None`, not a fallback
string, when the terminating TIPLOC isn't in `stanox_crs`). `render.rs`
gets one new hand-built-JSON helper, alongside `station_departure_json`
(`render.rs:140-153`):

```rust
pub(crate) fn schedule_departure_json(d: &schedule_query::ScheduleDeparture) -> Value {
    json!({
        "uid": d.uid,
        "scheduled": d.scheduled.format("%H:%M").to_string(),
        "destinationCrs": d.destination_crs,
    })
}
```

**Frontend: a distinct type, not a field-optional variant of `DepartureRow`.**
Silently unifying the two shapes (e.g. giving `DepartureRow` optional
`operator?`/`delayMinutes?`) would let a CIF-derived row render through the
*same* badge logic that currently promises "On time"/"+N min"/"Cancelled"
— exactly the dishonesty Part 5 below rules out. Instead:

```tsx
// frontend/components/TrackTrainForm.tsx -- new type, sibling to DepartureRow

/** Wire shape of `GET /public/stations/{crs}/schedule-departures`
 * (`crates/api/src/render.rs::schedule_departure_json`) -- deliberately
 * NOT `DepartureRow`: no `operator`, no live running-status fields at all
 * (`isCancelled`/`delayMinutes`/`estimated`/`cancelReason`/`delayReason`),
 * because the CIF SCHEDULE feed genuinely has none of that -- see design
 * doc Decision 2/5. `destinationCrs` is nullable: `None` when the
 * terminating TIPLOC has no `stanox_crs` row (a real, if rare, gap). */
interface ScheduleDepartureRow {
  uid: string;
  scheduled: string;
  destinationCrs: string | null;
}

type Picker =
  | { source: 'ldbws'; rows: DepartureRow[] }
  | { source: 'cif'; rows: ScheduleDepartureRow[] }
  | 'unavailable'
  | null;
```

Rendering distinguishes the two `source` values at the top of the block,
not per-row: the CIF branch reuses the same list layout (`scheduled ·
destinationCrs`) but renders **no status badge at all** (there is nothing
honest to badge — see Decision 5 for the copy that replaces it) and omits
the `· {operator}` segment entirely rather than showing a blank. Clicking a
CIF row calls a `pickCifDeparture` sibling to `pickDeparture` (`:118-124`)
that sets `destinationCrs`/`scheduledDeparture` only — **`operator` is left
exactly as the user already typed it**, never cleared, never guessed.

### 3. Fallback logic: a second fetch, on 404 only, chained off the existing effect

`TrackTrainForm.tsx`'s existing effect (`:94-108`) is extended, not
replaced — the LDBWS fetch is unchanged and still wins outright whenever it
succeeds or returns a populated/empty `200`; only its `404` branch grows a
second, sequential fetch:

```tsx
// frontend/components/TrackTrainForm.tsx -- extends the existing effect
const [picker, setPicker] = useState<Picker>(null);

useEffect(() => {
  if (!originValid) { setPicker(null); return; }
  const controller = new AbortController();
  const crs = originCrs.trim().toUpperCase();

  fetch(`/api/stations/${crs}/departures`, { signal: controller.signal })
    .then((res) => {
      if (res.status === 404) {
        // Fallback ONLY on 404 -- an LDBWS network blip or 500 must NOT
        // silently swap in the CIF picker; `!res.ok` still maps to `null`
        // exactly as today, leaving the picker absent rather than
        // switching sources on an error condition.
        return fetch(`/api/stations/${crs}/schedule-departures`, { signal: controller.signal })
          .then((cifRes) => {
            if (cifRes.status === 404) return setPicker('unavailable');
            if (!cifRes.ok) return setPicker(null);
            return cifRes.json().then((rows) => setPicker({ source: 'cif', rows }));
          });
      }
      if (!res.ok) return setPicker(null);
      return res.json().then((rows) => setPicker({ source: 'ldbws', rows }));
    })
    .catch(() => {}); // aborted or network blip -- leave prior state, unchanged posture
  return () => controller.abort();
}, [originCrs, originValid]);
```

The existing `'not-sampled'` terminal state is renamed `'unavailable'` and
its copy changes to reflect that *neither* source had data (Decision 5)
rather than only naming the LDBWS gap. A CIF `200 []` (station known to
`stanox_crs`, but zero upcoming departures survived the `now`-forward
filter — e.g. requested very late in the service day) renders the same
"no departures right now" sentence the LDBWS empty-array branch already
uses, reused verbatim rather than duplicated.

### 4. Scale/cost: the ~23x row-count figure holds; the cost it implied does not

The research doc flagged two distinct things under one "~23x" number, and
they resolve differently:

- **Compute cost — does NOT scale 23x.** The research doc's own analysis
  already ruled out the naive alternative (calling `schedules_touching`
  once per CRS, ~2,500× instead of ~110×, ~587M comparisons). Decision 1's
  `departures_by_crs` is the single grouping pass the research doc
  recommended precisely to avoid that: O(all UIDs) resolve + O(total
  calling points) bucketing, run **once**, independent of how many
  stations the result is later sliced into. This is the same complexity
  class `schedule-reference` already pays once per cycle for its
  `schedules_touching`-based line publish, not a new one.
- **Row-count — DOES scale ~23x, and that's fine, not a new risk class.**
  ~2,500 `schedule_network_departures` rows per cycle vs. ~110
  `schedule_line_population` rows is a real, confirmed ~22.7x row-count
  increase. But `schedule-reference` **already fully rewrites a ~3,100-row
  table every cycle today** — `stanox_crs`
  (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:521`,
  `queries::upsert_stanox_crs`, `queries.rs:648-678`) — via exactly the
  pattern this document reuses: one batch-array POST, one transaction, a
  loop of individual `INSERT ... ON CONFLICT` statements. **The absolute
  row count this document adds (~2,500) is smaller than a table this exact
  service already writes in full every 30 minutes.** The "23x" comparison
  looked alarming only because it was measured against
  `schedule_line_population`'s ~110 rows, not against this service's own
  real write-volume ceiling.
- **Row size — the new rows are deliberately much smaller than
  `schedule_line_population`'s, not proportionally larger.** A
  `schedule_line_population` row stores every UID touching a line, each
  with its **full, uncapped `calling_points` array** — a busy line's row
  can be large. A `schedule_network_departures` row is capped to 10
  `{uid, scheduled, destinationCrs}` entries (Decision 1) — roughly
  60-80 bytes each, ~700 bytes-1KB per row including JSONB/column
  overhead. At ~2,500 rows, that's on the order of **2-3MB of JSONB
  written per 30-minute cycle**, in one transaction, alongside a `stanox_crs`
  rewrite that already touches a comparable row count today. Not
  bounded arbitrarily — bounded by construction, from the `now`-forward
  filter plus the 10-row cap, regardless of how many departures a busy
  interchange schedules in a full day.
- **Resolution: no additional capping mechanism is needed beyond what
  Decision 1 already specifies** (the `now`-forward filter + the 10-row
  cap + the batch-array POST). The research doc's flagged "worth a
  capacity sanity-check before shipping" is satisfied by direct comparison
  to `stanox_crs`'s already-running write pattern, not by a new
  measurement this document invents.

### 5. Freshness/staleness honesty: distinct copy, distinct visual treatment, no borrowed liveness cues

The CIF-derived picker must never look like a live board. Concretely,
distinct from the LDBWS picker's framing at every point of contact:

- **No status badge.** The LDBWS picker's per-row badge
  (`TrackTrainForm.tsx:230-236`: "Cancelled"/"+N min"/"On time") encodes
  *live* running information this source does not have — STP=C
  cancellations are already filtered out server-side (`schedules_touching`
  never returns a cancelled schedule, `resolve.rs:88`), so every row shown
  is, as far as this source is concerned, "scheduled to run" — showing
  "On time" for that would imply live confirmation that does not exist.
  The CIF row renders with **no badge**.
  - a persistent, non-dismissable sentence directly above the CIF list,
  distinct in wording from the LDBWS `'unavailable'` sentence and never
  reused for it:

  > *"Live departure boards aren't available for this station. Showing the
  > scheduled timetable instead — this is not live running information and
  > may be up to 30 minutes out of date."*

  This appears **only** in the `source: 'cif'` branch — the LDBWS branch's
  existing rendering is untouched, so a user who *does* get the live
  picker never sees this caveat at all.
- **No operator shown, and no operator auto-filled.** Already covered in
  Decision 2 — restated here because it's also a staleness-adjacent
  honesty point: a stale-but-plausible operator guess would be worse than
  none.
- **The `now`-forward publish-time filter (Decision 1) bounds staleness in
  one direction (no five-hour-stale rows shown at 3pm) but not the other**:
  a row published at cycle-start could itself be up to 30 minutes stale by
  the time a user views it, and nothing prevents a row's `scheduled` time
  from having already passed by then. This is flagged, not engineered
  around in this pass — see Open Questions.

## Explicitly out of scope

- **`BX`/headcode decoding.** Deferred exactly per the research doc's
  Recommendation #3 — ship honestly missing Operator, don't block this
  slice on new byte-offset verification work.
- **Merge/replace of the two sources for an already-LDBWS-sampled
  station.** LDBWS wins outright wherever it has data; this document's
  fallback only ever fires on a 404, never blending or second-guessing an
  LDBWS `200` response, per the research doc's Recommendation #2.
- **Full per-train calling-pattern display** (research doc Part 3b, shape
  2). `LinePopulationEntry` can carry a full pattern "for free" *within
  `schedule_line_population`'s per-line rows*, but `schedule_network_departures`
  deliberately does **not** — including a full `calling_points` array per
  departure, per station, would multiply row size by the pattern length
  and defeat Decision 4's size bound for no feature this slice needs. A
  future "show full stopping pattern" UI is a separate, later addition
  (e.g. a UID-keyed lookup against a still-published line population, for
  stations that happen to sit on a catalogued line) — not designed here.
- **Any resident, permanently-in-memory whole-network index anywhere.**
  `schedule-reference` stays a lightweight, restart-safe poller — the
  `ScheduleIndex` built in Decision 1 is stack-local for one cycle, exactly
  like today's line-population build, then dropped.
- **A synchronous HTTP call from `api` into `schedule-reference` at
  request time.** Ruled out for the same reason the research doc's Part 3a
  rules it out — this app has no synchronous cross-service request pattern
  anywhere else; this document doesn't introduce one.
- **Pagination, or any window wider than the 10-departure, `now`-forward
  cap.** Whatever Decision 1 publishes is what the picker shows.
- **Any change to `TrackPinRequest`, `validate_pin`, or `POST
  /Train/track`.** The CIF picker only ever fills existing form state.
- **Broadening `poller-ldbws`'s own sampled-station set.** Unrelated,
  already-declined-elsewhere scope (per the LDBWS design doc's own
  Decision 1) — this document's whole point is covering the *gap* that
  leaves, not shrinking it a different way.
- **Same-day VSTP-style urgent CIF amendments.** Inherited, unresolved,
  from the research doc's Open Question 1 — this document does not answer
  whether `schedule-ingest`'s delivery pipeline ever carries these.

## Open questions / risks

1. **Local civil-time handling for the `now`-forward filter, inside
   `schedule-reference` specifically.** CIF `booked_departure` times are
   local (Europe/London) civil time, not UTC — comparing against a naive
   `chrono::Utc::now()` would be wrong by an hour during BST. This
   codebase has proven local-time handling (`crates/api/src/data/eta_blend.rs:44-56`'s
   `london_to_utc`), but it lives in `api`, not `schedule-reference`, and
   this document does not resolve whether to duplicate a small helper,
   extract a shared one, or take a different, `schedule-reference`-local
   approach — flagged for the implementation-plan stage, not decided here.
2. **How large `by_crs` (the un-truncated grouping-pass output,
   ~7.6M calling points total per the research doc's own estimate) gets
   transiently, and whether the extra grouping pass measurably changes
   `schedule-reference`'s own cycle duration.** Unmeasured — inherits the
   research doc's own Open Question 2 (never-profiled `ScheduleIndex`
   build) and extends it to the new pass; worth a cheap real check (timing
   `departures_by_crs` against the real `timetable_full.zip` extract)
   before committing to this shape in an implementation plan.
3. **A row's `scheduled` time can go stale mid-cycle** (Decision 5's last
   point) — whether the frontend should client-side-filter out rows whose
   `scheduled` time has already passed (comparing against the browser's
   own clock, same posture as `pickDeparture`'s existing browser-local-date
   assumption) is a real, small UX question this document leaves open
   rather than resolving with an unresearched guess.
4. **How often `destination_crs` actually resolves to `None` in practice**
   (a terminating TIPLOC absent from `stanox_crs`) is unmeasured — the
   research doc's Part 2 confirms `stanox_crs` is whole-network by
   construction, so this is expected to be rare, but "expected to be rare"
   is not "confirmed rare against real data," and this document does not
   run that check.
5. **Whether `MAX_DEPARTURES_PER_STATION = 10` is the right cap**, chosen
   here purely by precedent-matching against `poller-ldbws`'s own
   `num_rows` default (`crates/poller-ldbws/src/config.rs:45-46`), not by
   any CIF-specific measurement of typical per-station departure density
   within a `now`-forward window.
6. **Same-day VSTP amendments** (research doc Open Question 1, restated
   above under Explicitly out of scope) — still unresolved, still directly
   relevant to how confidently this feature's "scheduled, not live" framing
   (Decision 5) can be marketed as anything more than "what CIF said as of
   the last delivery."
