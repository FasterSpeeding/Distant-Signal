# Real Operator (TOC) Filtering for Journey Leg Time-Window Candidate Search

**Status: approved for implementation.** Written directly from a same-session
investigation of the real codebase (file/line citations below), not from a
separate design doc. No spec doc exists elsewhere for this feature; this
plan is its own authority, subordinate only to the real, cited evidence in
it.

## Why

`crates/api/src/data/queries.rs::search_journey_leg_candidates` (backs `GET
/Journeys/{journeyId}/legs/{legId}/candidates`, called by
`frontend/components/JourneyLegCandidates.tsx` for the "search a time
window" journey-leg-creation flow) has no way to filter candidate trains by
operator (TOC), because `schedule_destination_departures` — the table it
reads — carries no operator column, because CIF SCHEDULE ingestion
(`schedule-query`/`schedule-reference`) never decodes the one CIF record
that carries it: the `BX` ("Basic Schedule Extra Details") line.
`crates/schedule-query/src/parse.rs` already recognizes a `BX` line (so it
correctly extends the open `BS` block rather than being treated as
malformed) but its own doc comment says plainly: "not decoded -- no real
fixture in this plan's scope needed a `BX` field." This plan closes that
gap end to end: decode the real ATOC/TOC code from `BX`, thread it through
`schedule-query` -> `schedule-reference` -> `api` DB column -> query filter
-> route -> frontend picker.

A prior pass in this same session correctly declined to add a *decorative*
filter control with nothing real behind it. This plan is what makes it
real.

## The real evidence this plan is grounded in

### The BX record's real byte content (already in this repo)

`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`,
"Claim 1" section (line 92), quotes a real, byte-verbatim `BX` line copied
directly from a genuine National Rail full CIF timetable extract
(`RJTTF942MCA.txt`, from `timetable_full.zip`, generated 28/08/2026 — see
that doc's own "Why this exists" section for full provenance):

```
BX         SRYSR408800
```

This exact line (paired with the real `BS` line for UID `W68468`,
`BSNW684682605172610180000001 POO2E88    113560015 EMU    075D     S
O`, also quoted in the same section) is ALREADY used byte-verbatim in
this codebase's own tests:
`crates/schedule-query/src/parse.rs`'s
`a_real_overlay_bs_line_with_full_body_decodes_lo_and_the_o_indicator` test,
and `crates/schedule-query/tests/real_cif_fixtures.rs`'s
`REAL_MIXED_BLOCK` constant / `a_real_mixed_block_parses_end_to_end_through_the_index`
test.

### The byte range for the ATOC/TOC code field

The CIF `BX` record's fixed-width field layout (Network Rail / Rail
Delivery Group "CIF User Spec", document RSPS5046, "Timetable Information
Data Feed Interface Specification" — mirrored on the Open Rail Data Wiki's
Schedule Records page) is, in field order after the 2-byte record identity:
Traction Class (4 chars), UIC Code (5 chars), **ATOC Code (2 chars)**,
Applicable Timetable Code (1 char), Retail Service ID (8 chars), Source (1
char), Spare. Converting the spec's 1-indexed columns to 0-indexed
half-open Rust byte ranges:

| Field | 0-indexed byte range |
|---|---|
| Record Identity (`"BX"`) | `0..2` |
| Traction Class (not decoded) | `2..6` |
| UIC Code (not decoded) | `6..11` |
| **ATOC Code** | **`11..13`** |
| Applicable Timetable Code (not decoded) | `13..14` |
| Retail Service ID (not decoded) | `14..22` |
| Source (not decoded) | `22..23` |

**Independent triangulation against the real quoted line, not just the
published spec table** (this repo's own "no invented byte offsets"
convention — see `crates/schedule-query/src/records.rs`'s module doc
comment — demands checking a real sample, not just trusting a remembered
table):

1. `"BX         SRYSR408800"[11..13]` = `"SR"` — a real, valid ATOC code
   (ScotRail). The schedule this line belongs to (UID `W68468`) calls at
   `BALLOCH`, a real ScotRail-operated Argyle Line station near Glasgow —
   so the decoded operator is exactly the one a real Scottish local service
   should have.
2. The same line's bytes `14..22` (Retail Service ID, NOT decoded by this
   plan, computed here only as a cross-check) = `"SR408800"` — real-world
   RSID convention is for a service's RSID to begin with its own ATOC code,
   and it does here: `"SR"` (ATOC Code) + `"SR408800"` (RSID) agree with
   each other, which would be a coincidence if the byte offset were wrong.
3. A second real-world fragment surfaced independently while researching
   this plan (Open Rail Data Wiki sample text, UID `G82885`) shows the same
   `"BX ... SR Y ..."` shape (ATOC Code then Applicable Timetable Code
   `'Y'` immediately after) at the same relative position.

All three agree. Byte range `11..13` is the ATOC Code field.

### The table is fully rebuilt every publish cycle (no backfill concern)

`crates/api/src/data/queries.rs::upsert_schedule_destination_departures`
(the only writer of `schedule_destination_departures`) does, in one
transaction: `DELETE FROM schedule_destination_departures WHERE
service_date = ANY(...)` over the batch's distinct service dates, then one
bulk `INSERT ... SELECT * FROM UNNEST(...)`. Confirmed also at the ingest
route (`crates/api/src/routes/ingest.rs::post_schedule_destination_departures`,
which calls that same function with no other logic) and by the table's own
migration doc comment
(`crates/api/migrations/20260907130000_schedule_destination_departures.sql`):
"an ingest wholesale-replaces a whole service_date in one transaction... no
`updated_at`... per-row write timestamps would all be identical." A new
nullable column added by migration needs no backfill: every existing row
is deleted and rewritten by the very next CIF delivery's publish cycle
(daily), and until then a `NULL` operator on old rows is correct — that
data genuinely wasn't decoded yet.

## Naming decisions (binding — use these exact names)

- Rust field name everywhere in the backend: **`operator_atoc: Option<String>`**
  (on `BasicSchedule`, `ResolvedSchedule`, `DestinationDeparture`,
  `ScheduleDestinationDeparturesRow`). `None` when a schedule has no `BX`
  line at all (a `Cancellation`-indicator `BS` line has no body, see
  `StpIndicator::Cancellation`'s doc comment) or when the ATOC Code field
  itself is blank/space-filled in a real record.
- DB column: **`operator_atoc TEXT`** (nullable, on `schedule_destination_departures`).
- JSON wire key between `schedule-reference` and `api`'s ingest route (the
  `ScheduleDestinationDeparturesRow` field, `#[serde(default)]` for
  rolling-deploy tolerance, matching `true_origin_crs`'s own precedent):
  **`"operator_atoc"`**.
- Snake_case key inside the intermediate `serde_json::Value` rows built by
  `queries::search_journey_leg_candidates` /
  `queries::search_schedule_calling_point_departures`: **`"operator_atoc"`**.
- Public wire (camelCase) key emitted by `render::calling_point_departure_json`
  (shared by `GET /public/trains/search` AND `GET
  /Journeys/{id}/legs/{id}/candidates`): **`"operator"`** — matching the
  existing precedent for a single ATOC code on a departure row,
  `frontend/components/TrackTrainForm.tsx`'s own `DepartureRow.operator:
  string` (LDBWS rows).
- Filter query param on `GET
  /Journeys/{journeyId}/legs/{legId}/candidates`: **`operator`**, a
  comma-separated list of ATOC codes, parsed to `Option<Vec<String>>` —
  copy `crates/api/src/routes/incidents.rs::search_incidents`'s existing
  parsing block verbatim (lines ~218-226 as of this writing): split on
  `,`, trim each, drop empties, `None` if the resulting list is empty.
- Rust query-function parameter name: **`operators: Option<Vec<String>>`**
  (matches `queries::search_incidents`'s own parameter name for the same
  shape).
- SQL predicate: **`AND ($N::text[] IS NULL OR main.operator_atoc = ANY($N))`**
  — the exact shape `queries::search_incidents` already uses for
  `operators && $1` is an *array-overlap* (multi-valued column); here the
  column is single-valued per row, so the correct mirror is `= ANY($N)`,
  not `&&`.
- Frontend field: **`CandidateRow.operator: string | null`** (in
  `frontend/components/JourneyLegCandidates.tsx` — NOT
  `frontend/lib/types.ts`; `CandidateRow` is a locally-defined interface
  in that component file, not a shared type. Treat this plan's original
  phrasing "types.ts" as referring to that local interface).

## Scope boundaries (ruled on up front, not open questions)

- **`crates/schedule-query`'s `schedule_network_departures` /
  `ScheduleDeparture` product (a different table,
  `schedule_network_departures`, different publish function) is OUT OF
  SCOPE.** Only `DestinationDeparture` / `schedule_destination_departures`
  (the table `search_journey_leg_candidates` actually reads) carries the
  new field. `crates/api/src/render.rs::schedule_departure_json`'s own doc
  comment ("deliberately NOT `DepartureRow`: no `operator`... because the
  CIF SCHEDULE feed genuinely has none of that") becomes imprecise once
  this plan ships (CIF *does* carry it now, this product just still
  doesn't plumb it) — Task 5 updates that one sentence to say so, without
  changing that function's behavior.
- **`crates/notifier/src/queries.rs::schedule_candidates_for_leg`** (the
  documented near-duplicate of `search_journey_leg_candidates`, used only
  by the automatic `match_mode = 'auto'` template sweep) is **NOT
  extended**. Nothing in this codebase gives an auto-match journey
  template an operator preference to filter by, so adding an unused column
  or an unused filter parameter there is speculative complexity with no
  consumer. Ledger this as a ruling, do not silently skip it.
- **`crates/api/src/data/queries.rs::search_schedule_calling_point_departures`**
  (backs the general-purpose `GET /public/trains/search`) gets the
  **read-through only** (the `operator_atoc` column added to its SELECT and
  its output JSON, flowing through the same shared
  `render::calling_point_departure_json` renderer this plan changes) for
  display consistency with the leg-candidates list, since both render
  through the same function. It does **NOT** get a new filter query
  parameter or new frontend UI on `TrainSearchForm.tsx` — that page's own
  doc comment ("CIF rows carry no operator field at all") names exactly
  the gap this plan closes at the *data* level, but wiring a new filter
  control into that separate, already-large form is unrequested scope this
  plan does not take on. A natural follow-up, not this plan's job.
- No new index on `schedule_destination_departures`. The operator filter
  is an additional `AND` predicate evaluated after the existing covering
  index (`schedule_destination_departures_calling_point_idx` on
  `service_date, origin_crs, scheduled, train_uid`) has already narrowed to
  one leg's rows — the same "no speculative index" posture the table's own
  migrations already established for `calling_point_arrival`.

## Global Constraints (bind every task)

- Every DB-touching Rust query/struct change in `crates/api` must come with
  updates to every existing call site/struct literal that would otherwise
  fail to compile (grep counts as of this writing, cite these in your
  brief-following, they may drift slightly by the time you run them):
  `BasicSchedule { ... }` literals: 4 (schedule-query). `ResolvedSchedule {
  ... }` literals: 3 (schedule-query). `DestinationDeparture { ... }`
  literals: 2 (schedule-query). `ScheduleDestinationDeparturesRow { ...
  }` literals: 12 (api, mostly test fixture builders in
  `crates/api/src/data/queries.rs`, plus 2 in
  `crates/api/src/routes/journeys.rs`). `search_journey_leg_candidates(`
  call sites: 4 (1 route + 3 tests, all in `crates/api`).
- Every new struct field is additive: `Option<String>` everywhere, never a
  required field with no sensible default, so rolling deploy / partial
  data never hard-fails.
- Match this repo's own established doc-comment rigor: every new byte
  range, table column, and struct field gets a real doc comment citing
  either the real fixture/spec evidence (schedule-query layer) or the
  established sibling precedent it mirrors (`true_origin_crs`,
  `calling_point_arrival`) — do not leave new public fields undocumented.
- DB-gated tests in `crates/api` follow the file's own established
  convention exactly: `#[ignore = "requires a live database; run with
  `DATABASE_URL=... cargo test -p api <test_name> -- --ignored
  --test-threads=1`"]`, a `test_pool()` helper, and a `delete_day(&pool,
  date)` cleanup call before and after. Use a distant-future `service_date`
  (this file's established sentinel is 2099, or 2067 where a real CIF
  `%y`-parsed date near the ceiling is needed — see
  `search_journey_leg_candidates_includes_every_real_operator_calling_at_a_shared_station`'s
  own doc comment for why 2067, not 2099, when building fixtures through
  the REAL parser).
- Do not touch `crates/schedule-query`'s fuzz targets or seed corpus
  (`crates/schedule-query/fuzz/`) — out of scope, no fuzz-target signature
  changes needed for an additive parse feature that only adds a new match
  arm and a new optional field.
- `cargo fmt --all`, then `cargo clippy --workspace --all-features
  --all-targets -- -D warnings`, then `cargo test --workspace -- --skip
  incident_search_query_tests` must be clean after every task (the
  `incident_search_query_tests` module is a known pre-existing unrelated
  sandbox hang — skip it, do not investigate it).
- Frontend: `npx tsc --noEmit` and `npm run lint` clean after the frontend
  task. Vitest via `mise exec node@22 -- npx vitest run` (the ambient
  sandbox Node has a known unrelated `localStorage`/jsdom incompatibility —
  if a FRESH `origin/main` checkout shows the same failures, they are not
  this plan's regression).

## Task 1 — Decode the ATOC/TOC code from `BX` (`schedule-query` parse layer)

Files: `crates/schedule-query/src/records.rs`, `crates/schedule-query/src/parse.rs`.

1. `records.rs`: add `pub operator_atoc: Option<String>` to `BasicSchedule`
   (after `days_of_week`). Doc comment: cite the real quoted `BX` line, the
   byte range `11..13`, and the module's own header comment's "every
   `BX`-record field... left undecoded" sentence needs updating too (it is
   no longer fully true — say so precisely: the ATOC Code field is now
   decoded, every other `BX` field remains undecoded and why).
2. `parse.rs`:
   - Add `const MIN_BX_LEN: usize = 13;` (needs bytes `0..13`: record
     identity through the ATOC code) next to the existing `MIN_*_LEN`
     constants, with a doc comment in that file's own established style.
   - Add `fn parse_bx_operator(line: &str) -> Option<String>`: guard with
     `is_fixed_width_decodable(line, MIN_BX_LEN)`, then read `&line[11..13]`,
     trim it, and return `None` if the trimmed result is empty (a
     space-filled ATOC Code field, if that ever occurs in real data),
     `Some(trimmed.to_string())` otherwise.
   - In `parse_schedule_records`'s match on `line.as_bytes()`, add a new
     arm `[b'X', ..] if ...` — no: add the arm as `[b'B', b'X', ..] =>
     { ... }` (it currently falls through the `_ => {}` catch-all). Body:
     if `current` is `Some`, set `current.as_mut().unwrap().basic.operator_atoc
     = parse_bx_operator(line);` (a malformed/short BX line just leaves it
     `None`, exactly like today's silent-skip posture for every other
     malformed line in this module — do not introduce a new error path).
     If `current` is `None` (a stray BX with no open block), do nothing —
     mirrors how a stray LO/LI/LT is already handled.
   - Update `parse_basic_schedule`'s `Some(BasicSchedule { ... })`
     constructor to include `operator_atoc: None` (real value is filled in
     later by the BX arm, if a BX line follows).
3. Real-fixture test (in `parse.rs`'s own `#[cfg(test)] mod tests`,
   alongside the existing `a_real_overlay_bs_line_with_full_body_decodes_lo_and_the_o_indicator`
   test): extend that same test (or add a sibling using the identical
   `BS_W68468_OVERLAY` + `"BX         SRYSR408800"` real-quoted text) to
   assert `schedules[0].basic.operator_atoc == Some("SR".to_string())`.
   Doc comment cites this plan's own "real evidence" section above (or
   restate the same citation inline: real line quoted in
   `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`,
   "Claim 1" section).
4. Fix every `BasicSchedule { ... }` struct literal elsewhere in this
   crate that this addition breaks (grep for `BasicSchedule {` inside
   `crates/schedule-query/src/`) by adding `operator_atoc: None,` (or a
   real test value where the surrounding test is specifically about this
   field).
5. Run `cargo test -p schedule-query` and confirm the new test and every
   pre-existing test in this crate still pass.

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary, concerns.

## Task 2 — Thread `operator_atoc` through resolution (`schedule-query` resolve layer)

Files: `crates/schedule-query/src/resolve.rs`, `crates/schedule-query/src/records.rs`,
`crates/schedule-query/tests/real_cif_fixtures.rs`.

Depends on Task 1 (needs `BasicSchedule::operator_atoc` to exist).

1. `records.rs`: add `pub operator_atoc: Option<String>` to
   `ResolvedSchedule`'s sibling struct `DestinationDeparture` (this plan's
   chosen carrier — see "Naming decisions" above for why not
   `ScheduleDeparture`/`schedule_network_departures`). Doc comment:
   mirror `true_origin_crs`'s own doc comment structure exactly — "computed
   once per schedule... attached unchanged to every entry that schedule
   contributes."
2. `resolve.rs`:
   - Add `pub operator_atoc: Option<String>` to `ResolvedSchedule` (next to
     `stp_indicator`/`cancelled`). In `resolve_for_date`, populate it from
     `winner.basic.operator_atoc.clone()` (same place `uid`/`stp_indicator`
     are read off `winner.basic`).
   - In `departures_by_destination_crs`, alongside the existing
     `true_origin_crs`/`destination_arrival`/`destination_arrival_day_offset`
     "computed once per schedule" block, add:
     `let operator_atoc = resolved.operator_atoc.clone();` and include
     `operator_atoc: operator_atoc.clone(),` in the
     `crate::records::DestinationDeparture { ... }` construction inside the
     per-calling-point loop.
   - `departures_by_crs` / `ScheduleDeparture` (the
     `schedule_network_departures` sibling): **do not touch** — out of
     scope, see Scope Boundaries above.
3. Fix the 3 `ResolvedSchedule { ... }` and 2 `DestinationDeparture { ...
   }` struct literals this breaks (add `operator_atoc: None,` unless the
   surrounding test specifically wants a real value).
4. Real end-to-end test: extend
   `crates/schedule-query/tests/real_cif_fixtures.rs`'s existing
   `a_real_mixed_block_parses_end_to_end_through_the_index` test (which
   already resolves the real `REAL_MIXED_BLOCK`, containing the same real
   `BX         SRYSR408800` line, for UID `W68468`) to also assert
   `resolved.operator_atoc == Some("SR".to_string())` — the real byte
   decode now proven all the way through `ScheduleIndex::from_text` ->
   `schedule_for_uid`.
5. Run `cargo test -p schedule-query` (whole crate, both unit and
   integration tests) and confirm everything passes.

Report file contract: same as Task 1.

## Task 3 — Publish `operator_atoc` (`schedule-reference`)

File: `crates/schedule-reference/src/main.rs`.

Depends on Task 2.

1. In `schedule_destination_departures_rows` (~line 749), add
   `"operator_atoc": d.operator_atoc,` to the `serde_json::json!({...})`
   object it builds per departure (alongside the existing
   `"true_origin_crs"`/`"calling_point_arrival"`/`"destination_arrival"`
   keys). Update that function's own doc comment's "budget ~100 bytes per
   entry" sizing note if you judge the extra field meaningfully changes it
   (a 2-char nullable string does not — a one-sentence note saying so is
   enough, not a re-measurement).
2. Existing unit tests for this function
   (`schedule_destination_departures_rows_*`, same file) build
   `schedule_query::DestinationDeparture { ... }` literals directly — fix
   every one Task 2 broke (add `operator_atoc: None` or a real value).
   Extend `schedule_destination_departures_rows_produces_one_flat_row_per_departure_carrying_its_destination`
   (or add a sibling) to assert the emitted row's `"operator_atoc"` key
   round-trips a `Some("SR".to_string())` input correctly.
3. `cargo test -p schedule-reference`.

Report file contract: same as Task 1.

## Task 4 — DB column + wire row + upsert (`crates/api` data layer)

Files: new migration under `crates/api/migrations/`,
`crates/api/src/data/queries.rs` (`ScheduleDestinationDeparturesRow`,
`upsert_schedule_destination_departures`).

Depends on Task 3 for the wire JSON key name agreement (no compile-time
dependency between crates — `api` never depends on `schedule-reference` or
`schedule-query`, per this repo's own documented crate-boundary
convention. This task can be implemented independently of Tasks 1-3
actually landing, but must use the exact same JSON key,
`"operator_atoc"`, decided above).

1. New migration file
   `crates/api/migrations/20260924120000_schedule_destination_departures_operator_atoc.sql`
   (pick the next available timestamp after `20260923110000_unlisted_links.sql`
   if that has since changed): `ALTER TABLE schedule_destination_departures
   ADD COLUMN operator_atoc TEXT;` with a doc comment mirroring
   `20260910100000_schedule_destination_departures_calling_point_arrival.sql`'s
   own style: what real CIF field it carries (the `BX` record's ATOC
   Code), why nullable (no `BX` line for a schedule, or a real gap-filled
   field), and an explicit "no new index" sentence with the same
   reasoning as this plan's own Scope Boundaries section.
2. `ScheduleDestinationDeparturesRow`: add
   `#[serde(default)] pub operator_atoc: Option<String>,` (same
   `#[serde(default)]` rolling-deploy-tolerance convention as
   `true_origin_crs`/`day_offset` on this same struct).
3. `upsert_schedule_destination_departures`: add the new column to every
   one of its three UNNEST-bound stages — the `Vec<Option<&str>>`
   collection (alongside `true_origin_crs`'s own identical
   `.iter().map(|r| r.operator_atoc.as_deref()).collect()`), the INSERT
   column list, the `UNNEST(...)` type list (`$11::text[]` — this bumps
   every subsequent `$N` placeholder in that one INSERT by one; there is
   no `LIMIT`/cursor placeholder in THIS particular query to worry about,
   unlike the SELECT queries in Task 5), and the final `.bind(&...)` call.
4. Fix every `ScheduleDestinationDeparturesRow { ... }` struct literal this
   breaks (12 sites per the Global Constraints count — grep to confirm the
   current count, it may have drifted) with `operator_atoc: None,` unless
   the surrounding test wants a real value.
5. Tests: extend this file's own
   `upsert_schedule_destination_departures`-covering tests (search for
   `mod` tests near that function, e.g. around line 5658-5900) to prove a
   row with `operator_atoc: Some("SR".to_string())` round-trips through
   the DB and back out. This is DB-gated (needs a live Postgres) — use the
   file's own `#[ignore = "requires a live database; ..."]` convention,
   `test_pool()`, `delete_day`.
6. Run `cargo test -p api --lib` (non-DB tests) and, if a live Postgres is
   reachable (see this plan's own final verification section for how to
   check), the new DB-gated test(s) specifically.

Report file contract: same as Task 1, PLUS explicitly state whether you
ran the DB-gated test against a live database or not, and why.

## Task 5 — Operator filter in the query layer + route + shared renderer (`crates/api`)

Files: `crates/api/src/data/queries.rs`
(`search_journey_leg_candidates`, `search_schedule_calling_point_departures`),
`crates/api/src/render.rs` (`calling_point_departure_json`,
`schedule_departure_json`'s doc comment), `crates/api/src/routes/journeys.rs`
(`CandidatesParams`, `get_leg_candidates`).

Depends on Task 4 (needs the `operator_atoc` column to exist).

1. **`render::calling_point_departure_json`** (~line 279): add
   `"operator": d.get("operator_atoc").cloned().unwrap_or(Value::Null),` to
   its output `json!({...})`, matching the existing null-tolerant
   `"originCrs"`/`"destinationCrs"` style exactly (both source keys are
   simply absent-or-null tolerant via `.cloned().unwrap_or(Value::Null)`).
   This is the SINGLE choke point both `GET /public/trains/search` and `GET
   /Journeys/{id}/legs/{id}/candidates` render through — one change
   surfaces `operator` on both.
2. **`render::schedule_departure_json`**'s doc comment (~line 202-218):
   update the sentence "deliberately NOT `DepartureRow`: no `operator`...
   because the CIF SCHEDULE feed genuinely has none of that" — it is no
   longer accurate that CIF has none; say instead that CIF now carries it
   (via `BX`, see `schedule_query::records::BasicSchedule::operator_atoc`)
   but this particular product (`schedule_network_departures`) was not
   extended to carry it, out of scope for that plan. Do not change this
   function's behavior, only its doc comment's accuracy.
3. **`search_journey_leg_candidates`** (~line 1943):
   - Add a new parameter `operators: Option<Vec<String>>` (position: after
     `arrive_before`, before `after` — i.e. the same relative position
     `search_incidents` puts its own array-filter params, grouped with the
     other filter args before pagination args).
   - Add `main.operator_atoc` to the SELECT list.
   - Add the new predicate `AND ($N::text[] IS NULL OR main.operator_atoc
     = ANY($N))` to the WHERE clause (bind it wherever convenient in the
     positional sequence — remember sqlx binds by CALL ORDER, not by which
     literal `$N` appears where in the SQL text, so you may bind
     `operators` right after `arrive_before` and before the cursor/limit
     binds, then simply renumber every `$N` in the SQL text from that
     point onward by one).
   - Extend the row tuple type (currently 8 elements: `String, String,
     Option<String>, NaiveTime, Option<NaiveTime>, i16, Option<NaiveTime>,
     Option<i16>`) with a 9th `Option<String>` for `operator_atoc`, in
     whichever position matches the SELECT list's new column order. Update
     every destructuring closure (the `next_cursor` closure and the
     `departures` map closure) to match the new arity, and add
     `"operator_atoc": operator_atoc,` to the json! object the map closure
     builds.
   - Update all 4 call sites (1 route, 3 tests — see Global Constraints)
     to pass `None` for the new parameter, EXCEPT where you add new test
     coverage below.
   - New DB-gated test: the existing
     `search_journey_leg_candidates_includes_every_real_operator_calling_at_a_shared_station`
     test (~line 7525) already seeds TWO real UIDs (`C17798` "Avanti West
     Coast" shape, `C18017` "London Northwestern" shape) calling at the
     same real station pair (EUS/MKC) — but neither of its `AVANTI_EUS_MKC`
     / `LNR_EUS_MKC_INTERMEDIATE` fixture blocks includes a `BX` line, so
     both currently resolve to `operator_atoc: None`. Add a NEW sibling
     test (do not mutate the existing one, which is proving a different,
     already-fixed bug) that appends a real-byte-position but
     clearly-labeled-synthetic `BX` line to each block (following this
     crate's own established "synthetic value, real byte layout" fixture
     convention — see `real_cif_fixtures.rs`'s own
     `SYNTHETIC_MINIMAL_BLOCK`'s `BX         SRYSR000000` line for the
     precedent of exactly this shape), giving the two schedules two
     DIFFERENT operator codes, and asserts that querying with
     `operators: Some(vec!["<code>".to_string()])` returns only the
     matching UID.
4. **`search_schedule_calling_point_departures`** (~line 1735): read-through
   only, per this plan's Scope Boundaries. Add `main.operator_atoc` to the
   SELECT list, extend its own (separate, smaller) row tuple type by one
   `Option<String>`, update its own destructuring closures, add
   `"operator_atoc": operator_atoc,` to its own json! output. Do NOT add a
   new parameter or new predicate to this function. All existing call
   sites keep the same argument list — you're only widening what each row
   carries, not what the function accepts.
5. **`routes::journeys::CandidatesParams`** (~line 1060): add `operator:
   Option<String>` field.
6. **`routes::journeys::get_leg_candidates`** (~line 1073): parse
   `params.operator` into `Option<Vec<String>>` using the EXACT same
   comma-split/trim/drop-empty/empty-list-becomes-None logic as
   `routes::incidents::search_incidents` (copy that block, do not
   generalize it into a shared helper — this repo's own established
   convention is to duplicate this exact ~8-line block per route, see
   `line_status.rs`/`trips.rs` doing the same independently). Pass the
   result as the new `operators` argument to
   `queries::search_journey_leg_candidates`.
7. Run `cargo test -p api --lib` and the new/existing DB-gated tests
   against a live Postgres if reachable.

Report file contract: same as Task 4 (state DB-gated test status
explicitly).

## Task 6 — Frontend: operator field + picker UI

Files: `frontend/components/JourneyLegCandidates.tsx`,
`frontend/components/JourneyLegCandidates.test.tsx`.

Depends on Task 5 (the wire contract — `"operator"` key on each row,
`?operator=` query param on the candidates endpoint — must be stable).

1. **`CandidateRow`** interface: add `operator: string | null;` with a doc
   comment citing `render::calling_point_departure_json`'s new `"operator"`
   key (Task 5).
2. **Operator picker**, reusing the EXACT established pattern from
   `frontend/components/TrackTrainForm.tsx` (its window-mode Operator
   field, ~lines 350-393 and ~1310-1325) and the shared helpers it already
   uses — do not invent a new pattern:
   - `import { useSuggestions } from '@/lib/useSuggestions';`
   - `import { searchTocs } from '@/lib/suggestions';`
   - `import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';`
   - Local state: `const [operator, setOperator] = useState('');`
   - `const { suggestions: operatorSuggestions, loading: operatorSuggestionsLoading } =
     useSuggestions(operator, searchTocs);`
   - Render an `Autocomplete label="Operator (optional)" value={operator}
     onChange={setOperator} {...suggestionAutocompleteProps(operatorSuggestions,
     { query: operator, loading: operatorSuggestionsLoading, noMatchMessage:
     'No matching operators' })} />` above the results list (inside the
     `Stack` this component already returns, before the summary `Text`
     line — but AFTER the loading/error/empty-state early returns, exactly
     like every other filter control in this app that only shows once
     there's something to filter... actually place it so it is visible
     during 'loading' too, since a user filtering by operator likely wants
     to set it before the first page even loads. Use your judgment on
     exact placement, document the choice in a one-line comment).
3. **Debounced, committed filter value, re-fetch on change**: this
   component currently `useEffect`s once per `[journeyId, legId]` with no
   filter params. Add a small local debounce so a fast typist doesn't fire
   one request per keystroke — mirror `useSuggestions.ts`'s own
   `DEBOUNCE_MS = 250` constant (cite it in a comment, do not import it,
   it isn't exported): a `useEffect` that watches `operator` and, after
   250ms of no further change, updates a separate `committedOperator`
   state. The main data-fetch `useEffect` (and `handleLoadMore`'s
   pagination) then depends on/includes `committedOperator`, and its
   fetch URL includes `?operator=<trimmed value>` (uppercase not required
   — the backend already does a plain uppercase-insensitive-free ANY()
   match... actually check Task 5's SQL: `operator_atoc = ANY($N)` is
   case-SENSITIVE by default in Postgres `TEXT` comparison, and real ATOC
   codes are always stored/decoded upper-case already (CIF is upper-case
   ASCII) — so uppercase the typed value client-side before sending it,
   matching `TrackTrainForm.tsx`'s own `matchesOperator`'s
   `.toUpperCase()` precedent) when non-empty, omitted when empty. Changing
   `committedOperator` must reset pagination (drop any existing
   `nextCursor`/rows) exactly like a fresh `journeyId`/`legId` mount does
   today.
4. Render the operator on each row, in `CandidateRowView`'s subtitle
   (alongside the existing `Train {uid} · {originCrs} → {destinationCrs}`
   line), only when `row.operator` is non-null — e.g. append ` ·
   {row.operator}`. One-line change, use your judgment on exact
   placement/wording.
5. Tests in `JourneyLegCandidates.test.tsx`: add `operator` to the fixture
   rows (some `null`, some a real-shaped 2-letter code) and cover: (a) the
   operator renders on a row that has one and is absent on a row that
   doesn't; (b) typing into the Operator field and letting the debounce
   settle triggers a re-fetch whose URL includes `operator=<value>`
   (uppercased); (c) the Operator field's own suggestion dropdown uses the
   SAME shared `suggestionAutocompleteProps`/`useSuggestions(...,
   searchTocs)` plumbing already covered elsewhere in this app's test
   suite for display + search-matching consistency — do not re-test
   `searchTocs`'s own matching logic here (that's `TrackTrainForm.test.tsx`'s
   job), just prove this component wires the shared helper correctly
   (mock `searchTocs`/`useSuggestions` the same way an existing test in
   this file or `TrackTrainForm.test.tsx` already does, if any does).
6. Run `npx tsc --noEmit`, `npm run lint`, and `mise exec node@22 -- npx
   vitest run frontend/components/JourneyLegCandidates.test.tsx` (plus a
   full `vitest run` pass to confirm no regressions elsewhere, noting any
   pre-existing unrelated jsdom/localStorage failures per this plan's
   Global Constraints).

Report file contract: same as Task 1, plus explicit tsc/lint/vitest output
summaries.

## Final Verification (after all tasks, before final review)

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-features --all-targets -- -D warnings`
- `cargo test --workspace -- --skip incident_search_query_tests`
- Check for a live Postgres: try `psql "$DATABASE_URL" -c 'select 1'` (or
  `pg_isready`) before claiming DB-gated tests ran or didn't. If reachable:
  create a scratch DB, `sqlx migrate run --source crates/api/migrations`,
  then `DATABASE_URL=postgres://postgres:postgres@localhost:5432/<db>
  cargo test -p api -- --ignored --test-threads=1 --skip
  incident_search_query_tests`. If unreachable, say so explicitly in the
  final report — do not claim DB-gated coverage that didn't run.
- `npx tsc --noEmit`, `npm run lint`,
  `mise exec node@22 -- npx vitest run` (frontend directory).
