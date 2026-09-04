# Whole-Network CIF-Derived Departures Fallback — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1 and 2 are independent and can proceed in either order (or in
> parallel).** Neither depends on the other: Task 1 (`schedule-query`) is a
> pure library addition nothing else in this plan calls yet; Task 2
> (`crates/api`) stores the new table's payload as opaque JSON and does
> **not** depend on `schedule-query` at all (see "Corrections to the design
> doc's own sketches" below). **Task 3 (`schedule-reference`) depends on
> both** — it calls Task 1's `departures_by_crs`/`ScheduleDeparture` and
> POSTs to Task 2's new route. **Task 4 (frontend) depends only on Task 2's
> wire shape being final**, not on Task 2 or Task 3 being deployed —
> `TrackTrainForm.test.tsx` mocks `global.fetch` directly, so it can be
> written and tested against Task 2's *documented* JSON shape without a
> live route (mirrors `docs/superpowers/plans/2026-09-03-trip-search-plan.md`'s
> own Task 1/Task 2 relationship). **Task 5 is final verification, after
> everything else lands.**

**Goal:** implement
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md`
("the design doc") end to end — a second, additive CIF-derived departures
picker that `TrackTrainForm.tsx` falls back to only when the existing
LDBWS-backed picker (`docs/superpowers/specs/2026-09-03-trip-search-design.md`,
already shipped) reports `'not-sampled'` for a station. Extends real
station coverage from the ~286 LDBWS-sampled stations to the ~2,500-station
network the CIF `SCHEDULE` feed and `stanox_crs` table already cover, with
zero merging of the two sources for a station that has both.

**Architecture:** one new pure function + type in `crates/schedule-query`
(`resolve::departures_by_crs`, `records::ScheduleDeparture`); a
restructured `crates/schedule-reference::poll_once` that builds one
`ScheduleIndex` per cycle and runs two grouping passes over it (the
existing per-line publish, unchanged in behavior, plus a new per-CRS
publish); one new table, one new private POST-only route, and one new
public GET route in `crates/api`; a second, sequential fetch chained off
`TrackTrainForm.tsx`'s existing departures effect, with its own, distinct
frontend type and rendering treatment.

**Tech Stack:** Rust (the existing `schedule-query`/`schedule-reference`/
`api` stack — axum, sqlx, `common::ingest::post_batch`, `chrono`/`chrono-tz`
for local-time handling), Next.js 16 App Router + TypeScript, Vitest 2 +
Testing Library.

**Spec:**
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md` —
its Decisions section is authoritative for every type/route/wire shape
below; this plan does not repeat the reasoning, only the concrete steps,
plus a short list of corrections found while grounding the design doc's own
sketches against the real, current state of the code (below).

---

## Corrections to the design doc's own sketches

The design doc's code sketches were written to convey shape, not copied
verbatim from a compiling check. Four real mismatches were found while
grounding this plan against the actual current code (all of it already
reflects the already-merged `2026-09-04-option-b-live-consumer-plan.md`,
confirmed by reading `crates/schedule-reference/src/main.rs`,
`crates/schedule-query/src/{records,resolve,lib}.rs`,
`crates/api/src/{app,routes/ingest,routes/departures,render}.rs`,
`crates/api/src/data/queries.rs` in full before writing this plan). Each is
resolved here the same way this session's other plans resolve theirs —
toward this repo's real, established convention, never a new one:

1. **`render.rs`'s new helper must NOT take `&schedule_query::ScheduleDeparture`.**
   The design doc's own Decision 1 text says, for the sibling
   `schedule_line_population` table: *"`population` is stored opaquely;
   `api` never deserializes it into `schedule_query::LinePopulationEntry` —
   only `schedule-reference` (writer) and `full-coverage-consumer` (reader)
   need that shape"* — and `crates/api/Cargo.toml` confirms this is real,
   not aspirational: **`crates/api` has no dependency on `schedule-query` at
   all today.** The design doc's own Decision 2 code sketch for
   `schedule_departure_json(d: &schedule_query::ScheduleDeparture)`
   contradicts that established posture for the *new* table without saying
   so. Corrected here: `schedule_departure_json` takes `&serde_json::Value`
   (the same opaque shape `queries::get_schedule_line_population` already
   returns for its table) and reads `uid`/`scheduled`/`destination_crs` off
   it by key, never adding `schedule-query` as an `api` dependency. See
   Task 2 Step 4.
2. **The new POST-body row struct lives in `queries.rs`, not `ingest.rs`.**
   The design doc's own sketch declares `ScheduleNetworkDeparturesRow` in
   `routes/ingest.rs`. This repo's dependency direction is routes → data,
   never the reverse (confirmed: no `data/queries.rs` function anywhere
   takes a type defined in `routes/`) — a query function accepting a
   route-layer type would invert that. Corrected here: the row struct is
   defined once in `data/queries.rs` (mirroring `StanoxCrsRow`'s own
   "query-scoped row struct lives next to its query" precedent) and
   imported by `routes/ingest.rs`. See Task 2 Steps 2–3.
3. **Local civil-time handling for the `now`-forward filter (the design
   doc's own Open Question 1) is resolved concretely, not left open.**
   `crates/api/src/data/eta_blend.rs::london_to_utc` — the only precedent
   the design doc names — solves the *harder* direction (a **naive** local
   datetime → the UTC instant it names, which is genuinely ambiguous or
   missing across a DST transition, hence its `LocalResult` handling).
   `schedule-reference` needs the *easier* direction instead: a known UTC
   instant (`Utc::now()`) → its Europe/London clock time, via
   `DateTime::with_timezone`, which is always exactly one unambiguous
   answer — no `LocalResult` matching needed at all. See Task 3 Step 3.
4. **The new POST reuses `common::ingest::post_batch` directly** rather
   than the design doc's own bespoke `post_schedule_network_departures`
   sketch. `post_batch<T: Serialize>` already POSTs a `Vec<T>` with bearer
   auth to a URL (it's what `poll_once` already uses for
   `Vec<common::StanoxCrsRecord>`) — `Vec<serde_json::Value>` satisfies
   `T: Serialize` trivially, so a duplicate helper isn't needed. This *is*
   Decision 1's own "one batch-array POST, not one POST per CRS" shape,
   already implemented generically. See Task 3 Step 4.

None of these change any wire shape, table schema, route path, HTTP
method, row cap, or frontend behavior the design doc specifies — only
which file a definition lives in and which existing helper a call site
reuses.

## Open questions this plan deliberately leaves open

The design doc's Open Question 1 is resolved concretely (Correction 3,
Task 3 Step 3). Open Question 2 (real-data timing) gets a best-effort,
non-blocking check (Task 5 Step 4). The rest are **not** addressed by any
task below, on purpose — carried forward exactly as the design doc left
them, not silently dropped:

- **Open Question 3** (client-side filtering of a row whose `scheduled`
  time has already passed by view time): not implemented. The design doc
  calls this "a real, small UX question this document leaves open rather
  than resolving with an unresearched guess" — this plan does the same; a
  stale-looking CIF row is a known, accepted rough edge, not a bug this
  plan is missing.
- **Open Question 4** (how often `destination_crs` resolves to `None` in
  practice): unmeasured by this plan. `departures_by_crs`'s own tests
  (Task 1 Step 4) prove the `None` path is *handled* correctly; they don't
  measure its real-data frequency.
- **Open Question 6 / same-day VSTP amendments**: unresolved, inherited
  from the research doc. Not addressed by any task below.

---

## Non-goals — binding, copied from the design doc's own "Explicitly out of
## scope" section

- **No `BX`/headcode decoding, no operator field anywhere in the new wire
  shape.** `ScheduleDeparture` carries only `uid`/`scheduled`/
  `destination_crs`; `ScheduleDepartureRow` (frontend) mirrors that exactly.
- **No merge/replace of the two sources for an already-LDBWS-sampled
  station.** The CIF fallback only ever fires when the LDBWS fetch returns
  `404`; a non-`404` LDBWS response (success or another error) is
  unchanged from today.
- **No full per-train calling-pattern display.** `ScheduleDeparture` has no
  `calling_points` field — deliberately narrower than
  `LinePopulationEntry`.
- **No resident, permanently-in-memory whole-network index.**
  `schedule-reference`'s `ScheduleIndex` stays stack-local for one cycle,
  exactly as today, then is dropped.
- **No synchronous HTTP call from `api` into `schedule-reference` at
  request time.** `api` reads its own table directly.
- **No pagination, no window wider than the 10-departure, `now`-forward
  cap.**
- **No change to `TrackPinRequest`, `validate_pin`, or `POST
  /Train/track`.**
- **No broadening of `poller-ldbws`'s sampled-station set.**
- **No change to `crates/full-coverage-consumer` or any shadow-mode
  full-coverage code.** Unrelated; not touched by any task below.
- **No change to the LDBWS picker's own behavior when it returns data.**
  Only its `404` branch gains a second fetch.

## Global Constraints

- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests) after every
  task; `DATABASE_URL=<url> cargo test -p api -- --ignored
  --test-threads=1` for the DB-backed tests this plan adds (mirrors
  `.github/workflows/ci.yml`'s existing invocation). Frontend: `npm test`
  (`vitest run`) and `npm run build` (`next build`) from `frontend/`.
- **Row/data caps** (design doc Decision 1/4, binding, verbatim): 10
  departures per station (`MAX_DEPARTURES_PER_STATION`), earliest-first,
  each row `{uid, scheduled, destinationCrs}` only. No other field is ever
  added to this wire shape by this plan.
- **Wire field naming.** The private POST body
  (`schedule-reference` → `api`) uses snake_case Rust field names
  (`crs`, `service_date`, `departures`; and, inside `departures`, `uid`,
  `scheduled`, `destination_crs`) — matching every other `/private/*`
  ingest payload in this codebase. The public GET response
  (`api` → frontend) uses camelCase (`uid`, `scheduled`, `destinationCrs`)
  — matching `station_departure_json`'s own established convention. These
  are two different, deliberately distinct JSON shapes for the same
  logical data, exactly like `StationDeparture`/`station_departure_json`
  already are.
- **No new OAuth group.** The new private route's POST is gated by the
  **already-existing** `internal_oauth_group_schedule_reference` (the same
  credential `POST /private/stanox-crs` and `POST
  /private/schedule-line-population` already use) — confirmed present in
  `crates/api/src/data/config.rs` and every colocated `ServiceArguments`
  test fixture already. The public GET route is unauthenticated, mounted
  via `public_router()` only, identical to `departures::get_station_departures`.
  No config field is added by this plan.
- **File scope.** Modified:
  `crates/schedule-query/src/{records.rs,resolve.rs,lib.rs}`;
  `crates/schedule-reference/{Cargo.toml,src/main.rs,src/config.rs}`;
  `crates/api/src/data/queries.rs`, `crates/api/src/routes/{ingest.rs,departures.rs}`,
  `crates/api/src/{app.rs,render.rs}`;
  `charts/distant-signal/templates/schedulefeed-deployment.yaml`;
  `frontend/components/{TrackTrainForm.tsx,TrackTrainForm.test.tsx}`.
  Created: `crates/api/migrations/20260904110000_schedule_network_departures.sql`.
  No other file changes.

---

## Task 1: `crates/schedule-query` — `ScheduleDeparture` + `departures_by_crs` (pure, tested)

**Files:**
- Modify: `crates/schedule-query/src/records.rs`
- Modify: `crates/schedule-query/src/resolve.rs`
- Modify: `crates/schedule-query/src/lib.rs`

Independent of every other task. No I/O, no new dependency.

- [ ] **Step 1: Add `ScheduleDeparture` to `records.rs`**

Directly after `LinePopulationEntry` (`records.rs:147-169` region):

```rust
/// One CIF-derived departure -- the whole-network trip-search fallback
/// picker's wire shape between `crates/schedule-reference` (writer, via
/// `POST /private/schedule-network-departures`) and `crates/api` (reader,
/// opaque-JSONB storage only -- `api` does NOT depend on this crate, see
/// docs/superpowers/plans/2026-09-04-whole-network-trip-search-plan.md's
/// own Corrections section). Deliberately narrower than
/// [`LinePopulationEntry`]: no `calling_points`, no full stopping pattern
/// -- see
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md's
/// Explicitly out of scope section for why a full pattern per departure
/// per station was ruled out (row-size blowup for a feature this slice
/// doesn't need).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDeparture {
    pub uid: String,
    pub scheduled: NaiveTime,
    pub destination_crs: Option<String>,
}
```

- [ ] **Step 2: Add `departures_by_crs` to `resolve.rs`**

`resolve.rs`'s current imports are `use chrono::{Datelike, NaiveDate};` —
extend to `use chrono::{Datelike, NaiveDate, NaiveTime};`. Directly after
`schedules_touching` (`resolve.rs:77-96` region):

```rust
/// Every non-cancelled, resolved schedule's departure-bearing calling
/// points (`Origin`/`Intermediate`, i.e. `booked_departure.is_some()` --
/// `Terminate` never has one, see [`crate::records::CallingPointKind::Terminate`]'s
/// own doc), bucketed by CRS via `tiploc_to_crs` (normalized-TIPLOC keyed,
/// built by the caller from the SAME cycle's already-resolved
/// `stanox_crs` rows -- no second lookup table, no new parse). A calling
/// point whose TIPLOC has no `tiploc_to_crs` entry is dropped, not guessed
/// at -- a real, if rare, honest gap (see the design doc's Open Question
/// 4), not a silent one: the caller simply never sees that departure
/// rather than seeing it filed under a wrong or fabricated CRS. A calling
/// point that IS kept but whose *destination* TIPLOC has no
/// `tiploc_to_crs` entry gets `destination_crs: None`, not dropped -- see
/// the design doc's Decision 1 wire-type doc comment.
///
/// `now`: only calling points with `booked_departure >= now` are kept --
/// this is what keeps a station's bucket naturally small AND naturally
/// forward-looking without an arbitrary unbounded "whole day" list (see
/// the design doc's Decision 4). One O(all UIDs) resolve pass + O(total
/// calling points) bucketing -- the same complexity class
/// [`schedules_touching`] already pays per line, done once for the whole
/// network instead of once per line.
pub fn departures_by_crs(
    index: &ScheduleIndex,
    date: NaiveDate,
    now: NaiveTime,
    tiploc_to_crs: &HashMap<String, String>,
) -> HashMap<String, Vec<crate::records::ScheduleDeparture>> {
    let mut by_crs: HashMap<String, Vec<crate::records::ScheduleDeparture>> = HashMap::new();

    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        for cp in &resolved.calling_points {
            let Some(departure) = cp.booked_departure else {
                continue;
            };
            if departure < now {
                continue;
            }
            let Some(crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else {
                continue;
            };
            let destination_crs = resolved
                .calling_points
                .last()
                .and_then(|last| tiploc_to_crs.get(normalize_tiploc(&last.tiploc)))
                .cloned();
            by_crs
                .entry(crs.clone())
                .or_default()
                .push(crate::records::ScheduleDeparture {
                    uid: resolved.uid.clone(),
                    scheduled: departure,
                    destination_crs,
                });
        }
    }

    by_crs
}
```

- [ ] **Step 3: Export the new type/function from `lib.rs`**

```rust
pub use parse::parse_schedule_records;
pub use records::{
    BasicSchedule, CallingPoint, CallingPointKind, LinePopulationEntry, RawSchedule,
    ScheduleDeparture, StpIndicator,
};
pub use resolve::{ResolvedSchedule, ScheduleIndex, departures_by_crs, resolve_for_date, schedules_touching};
pub use tiploc::normalize_tiploc;
```

- [ ] **Step 4: Unit tests in `resolve.rs`'s existing `#[cfg(test)] mod tests`**

Add these helpers next to the existing `basic`/`calling_point` fixture
functions:

```rust
fn calling_point_with_departure(
    tiploc: &str,
    kind: CallingPointKind,
    departure: &str,
) -> CallingPoint {
    CallingPoint {
        tiploc: tiploc.to_string(),
        kind,
        booked_arrival: None,
        booked_departure: Some(NaiveTime::parse_from_str(departure, "%H:%M").unwrap()),
        is_half_minute_arrival: false,
        is_half_minute_departure: false,
    }
}

fn tiploc_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(tiploc, crs)| (tiploc.to_string(), crs.to_string()))
        .collect()
}
```

And these tests, reusing the file's existing `basic`/`WEEKDAYS` fixtures:

```rust
#[test]
fn departures_by_crs_buckets_an_origin_departure_under_its_crs_with_destination_resolved() {
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![
            calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
            calling_point("CREWE  ", CallingPointKind::Terminate), // no booked_departure
        ],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);

    assert_eq!(by_crs.len(), 1, "only EUS gets a bucket -- CREWE's Terminate has no booked_departure");
    let euston = &by_crs["EUS"];
    assert_eq!(euston.len(), 1);
    assert_eq!(euston[0].uid, "C11052");
    assert_eq!(euston[0].scheduled, NaiveTime::from_hms_opt(8, 22, 0).unwrap());
    assert_eq!(euston[0].destination_crs, Some("CRE".to_string()));
    assert!(!by_crs.contains_key("CRE"));
}

#[test]
fn departures_by_crs_excludes_a_departure_already_before_now() {
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![calling_point_with_departure(
            "EUSTON ",
            CallingPointKind::Origin,
            "08:22",
        )],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(9, 0, 0).unwrap(); // after 08:22
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]);

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);
    assert!(by_crs.is_empty());
}

#[test]
fn departures_by_crs_excludes_a_cancelled_schedule_even_though_its_time_has_not_passed() {
    // Same real UID/date/days shape as this file's own `c11052_raw` fixture
    // (a base P pattern plus a real STP=C override on 2026-08-31).
    let raw = c11052_with_departures();
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(); // the cancelled date
    let now = NaiveTime::from_hms_opt(0, 0, 0).unwrap(); // well before any booked time
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]);

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);
    assert!(by_crs.is_empty(), "the STP=C override must suppress this date's bucket entirely");
}

#[test]
fn departures_by_crs_drops_a_calling_point_whose_own_tiploc_is_unresolved() {
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![calling_point_with_departure(
            "EUSTON ",
            CallingPointKind::Origin,
            "08:22",
        )],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = HashMap::new(); // EUSTON not resolved at all

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);
    assert!(by_crs.is_empty(), "an unresolved origin TIPLOC drops the whole calling point, never a fabricated CRS");
}

#[test]
fn departures_by_crs_keeps_a_calling_point_with_an_unresolved_destination_as_none() {
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![
            calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
            calling_point("CREWE  ", CallingPointKind::Terminate),
        ],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]); // CREWE deliberately absent

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);
    assert_eq!(by_crs["EUS"][0].destination_crs, None);
}

#[test]
fn departures_by_crs_buckets_an_intermediate_calling_point_departure_under_its_own_crs() {
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![
            calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
            calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
            calling_point("MNCRPIC", CallingPointKind::Terminate),
        ],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

    let by_crs = departures_by_crs(&index, date, now, &tiploc_to_crs);
    assert_eq!(by_crs.len(), 2, "both EUSTON (Origin) and CREWE (Intermediate) get their own bucket entry");
    assert_eq!(by_crs["EUS"][0].scheduled, NaiveTime::from_hms_opt(8, 22, 0).unwrap());
    assert_eq!(by_crs["CRE"][0].scheduled, NaiveTime::from_hms_opt(10, 5, 0).unwrap());
    assert_eq!(by_crs["CRE"][0].destination_crs, Some("MAN".to_string()));
}

/// Same real UID/STP/date-range/days values as this file's own `c11052_raw`
/// (a real Bank Holiday cross-check, see that fixture's own comment), but
/// with a real `booked_departure` added to the base pattern's Origin
/// calling point so `departures_by_crs` has something to (correctly) NOT
/// return on the cancelled date.
fn c11052_with_departures() -> Vec<RawSchedule> {
    vec![
        RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![calling_point_with_departure(
                "EUSTON ",
                CallingPointKind::Origin,
                "08:22",
            )],
        },
        RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Cancellation,
                "2026-08-31",
                "2026-08-31",
                MONDAY_ONLY,
            ),
            calling_points: Vec::new(),
        },
    ]
}
```

- [ ] **Step 5: Test, lint, build**

```bash
cargo fmt --all
cargo clippy -p schedule-query --all-features
cargo test -p schedule-query
```

Expected: all PASS, zero clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-query/src/records.rs crates/schedule-query/src/resolve.rs crates/schedule-query/src/lib.rs
git commit -m "schedule-query: add ScheduleDeparture + departures_by_crs, the whole-network per-CRS grouping pass"
```

---

## Task 2: `crates/api` — `schedule_network_departures` table, queries, private POST route, public GET route

**Files:**
- Create: `crates/api/migrations/20260904110000_schedule_network_departures.sql`
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/ingest.rs`
- Modify: `crates/api/src/routes/departures.rs`
- Modify: `crates/api/src/app.rs`
- Modify: `crates/api/src/render.rs`

Independent of Task 1 — see Corrections item 1: this table is opaque JSONB
end to end, `api` never depends on `schedule-query`.

- [ ] **Step 1: Migration**

```sql
-- crates/api/migrations/20260904110000_schedule_network_departures.sql
-- ---------------------------------------------------------------------
-- One row per (crs, service_date): a station's next-10, `now`-forward-
-- filtered, CIF-SCHEDULE-derived departures for one rail day, published
-- by schedule-reference (POST, its own existing writer credential -- the
-- same `internal_oauth_group_schedule_reference` /stanox-crs and
-- /schedule-line-population already use) and read by `api` itself
-- directly, via a public passthrough route -- unlike
-- schedule_line_population, there is no second private GET pair: the only
-- reader is this same crate's own SQL. `departures` is opaque JSONB here
-- -- a Vec<schedule_query::ScheduleDeparture> -- `api` never deserializes
-- it into that Rust type, only stores/relays it, same "opaque blob"
-- posture schedule_line_population.population already established. See
-- docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
-- Decision 1 and
-- docs/superpowers/plans/2026-09-04-whole-network-trip-search-plan.md
-- Task 2.
-- ---------------------------------------------------------------------

CREATE TABLE schedule_network_departures (
    crs          TEXT        NOT NULL,
    service_date DATE        NOT NULL,
    departures   JSONB       NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (crs, service_date)
);
```

Apply locally, confirm the table exists, commit in isolation
(`git commit -m "Add schedule_network_departures table (no writer/reader yet)"`)
— same isolated-migration-commit convention every prior plan in this
lineage uses.

- [ ] **Step 2: `queries.rs` — row struct, upsert, read**

Directly after `get_schedule_line_population` (`queries.rs:759-`, ends
around line 771 per the current file):

```rust
/// One `POST /private/schedule-network-departures` batch element --
/// query-scoped, deserialized straight off the request body by
/// `routes::ingest::post_schedule_network_departures`. Defined here
/// (the data layer), not in `routes/ingest.rs`, so the data layer never
/// depends on a route-layer type -- same direction as every other
/// dependency between these two files. `departures` stays an opaque
/// `serde_json::Value` -- see this table's own migration comment for why.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleNetworkDeparturesRow {
    pub crs: String,
    pub service_date: chrono::NaiveDate,
    pub departures: serde_json::Value,
}

/// Upserts one cycle's batch of per-station CIF-derived departures --
/// wholesale replaces any existing row for each `(crs, service_date)` (a
/// fresh cycle's grouping pass supersedes the prior one entirely, never
/// merged), same shape as `upsert_full_coverage_line_stats`/
/// `upsert_stanox_crs`: one transaction, one `INSERT ... ON CONFLICT` per
/// row.
pub async fn upsert_schedule_network_departures(
    pool: &PgPool,
    rows: &[ScheduleNetworkDeparturesRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO schedule_network_departures (crs, service_date, departures, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (crs, service_date) DO UPDATE SET
                departures = EXCLUDED.departures,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.crs)
        .bind(row.service_date)
        .bind(&row.departures)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Reads one station's CIF-derived departures for one service date, if
/// published. `None` when no `schedule-reference` cycle has published for
/// this `(crs, service_date)` yet -- either the station never appears in
/// `stanox_crs` at all, or (far more likely in practice) the current
/// service date's cycle just hasn't run yet. The caller
/// (`routes::departures::get_station_schedule_departures`) maps this to a
/// `404`, the same honesty split `get_station_departures` already uses for
/// `station_samples`.
pub async fn latest_schedule_network_departures(
    pool: &PgPool,
    crs: &str,
    service_date: chrono::NaiveDate,
) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT departures FROM schedule_network_departures WHERE crs = $1 AND service_date = $2",
    )
    .bind(crs)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get("departures")).transpose().map_err(Into::into)
}
```

(`Deserialize` needs to already be imported at the top of `queries.rs` --
confirm via `grep -n '^use serde' crates/api/src/data/queries.rs` first; if
not, add `use serde::Deserialize;`.)

- [ ] **Step 3: `routes/ingest.rs` — POST-only route**

Import the new row type:

```rust
use crate::data::queries::{self, ScheduleNetworkDeparturesRow};
```

(Adjust the existing `use crate::data::queries;` line at the top of
`ingest.rs` to this form, or add a second `use` line — whichever keeps
every other `queries::` call site in the file working unchanged.)

`router()`, one more `.route(...)`, **POST only** — deliberately no GET
pair, unlike `/schedule-line-population` (design doc Decision 1: the only
reader is `api` itself):

```rust
.route(
    "/schedule-network-departures",
    axum::routing::post(post_schedule_network_departures),
)
```

Handler, directly after `post_schedule_line_population`/
`get_schedule_line_population`:

```rust
/// `crates/schedule-reference`'s per-cycle batch of CIF-derived per-station
/// departures -- see `queries::upsert_schedule_network_departures`. POST
/// only: unlike `/schedule-line-population`, no service reads this table
/// back over HTTP -- `api` serves it straight off Postgres via
/// `routes::departures::get_station_schedule_departures`. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1.
async fn post_schedule_network_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleNetworkDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_network_departures(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 4: `app.rs` — one route-table entry, POST only**

In `build_internal_oauth_routes`, directly after the `/schedule-line-population`
entries:

```rust
// POST-only: no GET pair, unlike /schedule-line-population -- see
// Corrections/Task 2's own note in
// docs/superpowers/plans/2026-09-04-whole-network-trip-search-plan.md.
// Reuses schedule-reference's EXISTING writer credential, the same one
// /stanox-crs and /schedule-line-population's own POST already use.
(
    "/schedule-network-departures",
    Method::POST,
    vec![config.internal_oauth_group_schedule_reference.clone()],
),
```

- [ ] **Step 5: `render.rs` — `schedule_departure_json`**

Directly after `station_departure_json`:

```rust
/// Hand-built camelCase JSON for one CIF-derived schedule departure entry,
/// backing `GET /public/stations/{crs}/schedule-departures`. Operates on
/// the already-opaque `serde_json::Value` this table stores rather than
/// `schedule_query::ScheduleDeparture` -- `crates/api` deliberately does
/// NOT depend on `schedule-query` (a leaf parsing crate), the same
/// "opaque JSONB, never deserialized into the producer's own Rust type"
/// posture `schedule_line_population` already established (see this
/// plan's own Corrections section for why this departs from the design
/// doc's `&schedule_query::ScheduleDeparture` sketch). Missing/mistyped
/// fields fall back to `Value::Null`/`None` rather than panicking --
/// defensive against this table's producer and this function ever
/// silently drifting out of sync, matching this module's general
/// avoidance of `.unwrap()` on data that crossed a process boundary.
/// `scheduled` is stored as chrono's default `NaiveTime` JSON
/// serialization (`"HH:MM:SS"`); this trims it to `"HH:MM"`, matching the
/// design doc's own documented wire shape for this field.
pub(crate) fn schedule_departure_json(d: &Value) -> Value {
    let scheduled = d
        .get("scheduled")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "destinationCrs": d.get("destination_crs").cloned().unwrap_or(Value::Null),
    })
}
```

- [ ] **Step 6: unit test for `schedule_departure_json` in `render.rs`'s
      existing `#[cfg(test)] mod tests`**

```rust
#[test]
fn schedule_departure_json_maps_snake_case_to_camel_case_and_trims_seconds() {
    let raw = serde_json::json!({
        "uid": "C11052",
        "scheduled": "08:22:00",
        "destination_crs": "CRE",
    });
    let json = schedule_departure_json(&raw);
    assert_eq!(
        json,
        serde_json::json!({ "uid": "C11052", "scheduled": "08:22", "destinationCrs": "CRE" })
    );
    assert!(json.get("destination_crs").is_none(), "no stray snake_case field");
}

#[test]
fn schedule_departure_json_null_destination_crs_stays_null() {
    let raw = serde_json::json!({
        "uid": "C99999",
        "scheduled": "14:05:00",
        "destination_crs": null,
    });
    let json = schedule_departure_json(&raw);
    assert!(json["destinationCrs"].is_null());
}
```

- [ ] **Step 7: `routes/departures.rs` — public GET route**

Add the import and route entry:

```rust
use crate::render::{schedule_departure_json, station_departure_json};
```

```rust
pub fn router() -> Router {
    Router::new()
        .route(
            "/stations/{crs}/departures",
            axum::routing::get(get_station_departures),
        )
        .route(
            "/stations/{crs}/schedule-departures",
            axum::routing::get(get_station_schedule_departures),
        )
}
```

Handler, directly after `get_station_departures`:

```rust
/// `GET /public/stations/{crs}/schedule-departures`: today's CIF
/// SCHEDULE-derived scheduled departures for `crs` -- the whole-network
/// trip-search fallback picker's backing route, see
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1. Reads `schedule_network_departures` directly for today's
/// date, server-side -- same "always now" posture as
/// `get_station_departures` above, and the same 404-vs-`200 []` honesty
/// split: 404 when no row exists for `(crs, today)` at all (this station
/// isn't in `stanox_crs`, or today's cycle simply hasn't published yet);
/// `200 []` when a row exists but its `now`-forward filter left nothing.
async fn get_station_schedule_departures(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let today = chrono::Utc::now().date_naive();
    let Some(departures) =
        queries::latest_schedule_network_departures(&app.database, &crs, today)
            .await
            .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule data for station: {crs}"),
        ));
    };

    let rows = departures.as_array().cloned().unwrap_or_default();
    Ok(Json(rows.iter().map(schedule_departure_json).collect()))
}
```

- [ ] **Step 8: DB-backed tests, `routes/ingest.rs`'s `db_tests` module**

Following the existing `schedule_line_population` tests' shape in the same
file:

```rust
async fn delete_network_departures_fixture(pool: &PgPool, crs: &str) {
    sqlx::query("DELETE FROM schedule_network_departures WHERE crs = $1")
        .bind(crs)
        .execute(pool)
        .await
        .expect("cleanup fixture schedule_network_departures rows");
}

fn network_departures_body(crs: &str, service_date: &str) -> Value {
    json!([{
        "crs": crs,
        "service_date": service_date,
        "departures": [{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}],
    }])
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_network_departures -- --ignored --test-threads=1`"]
async fn post_schedule_network_departures_upserts_the_row() {
    let pool = connect().await;
    delete_network_departures_fixture(&pool, "ZQV").await;

    let router: axum::Router = crate::app::Router::new()
        .merge(router())
        .with_state(test_app(pool.clone()));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/schedule-network-departures")
                .header("content-type", "application/json")
                .body(Body::from(
                    network_departures_body("ZQV", "2026-09-04").to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!({"upserted": 1}));

    let departures: serde_json::Value = sqlx::query_scalar(
        "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQV' AND service_date = '2026-09-04'",
    )
    .fetch_one(&pool)
    .await
    .expect("row landed");
    assert_eq!(departures[0]["uid"], "C11052");

    delete_network_departures_fixture(&pool, "ZQV").await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_network_departures -- --ignored --test-threads=1`"]
async fn a_second_post_for_the_same_key_wholesale_replaces_not_merges() {
    let pool = connect().await;
    delete_network_departures_fixture(&pool, "ZQW").await;

    queries::upsert_schedule_network_departures(
        &pool,
        &[ScheduleNetworkDeparturesRow {
            crs: "ZQW".to_string(),
            service_date: "2026-09-04".parse().unwrap(),
            departures: serde_json::json!([{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}]),
        }],
    )
    .await
    .expect("seed first row");
    queries::upsert_schedule_network_departures(
        &pool,
        &[ScheduleNetworkDeparturesRow {
            crs: "ZQW".to_string(),
            service_date: "2026-09-04".parse().unwrap(),
            departures: serde_json::json!([{"uid": "C99999", "scheduled": "09:00:00", "destination_crs": null}]),
        }],
    )
    .await
    .expect("seed second row");

    let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
        "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQW' AND service_date = '2026-09-04'",
    )
    .fetch_all(&pool)
    .await
    .expect("select fixture rows");
    assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
    assert_eq!(rows[0].0[0]["uid"], "C99999");

    delete_network_departures_fixture(&pool, "ZQW").await;
}
```

- [ ] **Step 9: DB-backed tests, `routes/departures.rs`'s `db_tests` module**

```rust
async fn delete_schedule_departures_fixture(pool: &PgPool, crs: &str) {
    sqlx::query("DELETE FROM schedule_network_departures WHERE crs = $1")
        .bind(crs)
        .execute(pool)
        .await
        .expect("cleanup fixture schedule_network_departures rows");
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_departures -- --ignored --test-threads=1`"]
async fn schedule_departures_no_row_for_crs_today_is_404_naming_the_crs() {
    let pool = connect().await;
    delete_schedule_departures_fixture(&pool, "ZQX").await;

    let router: axum::Router = crate::app::Router::new()
        .merge(router())
        .with_state(test_app(pool.clone()));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/stations/ZQX/schedule-departures")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8(body.to_vec()).unwrap().contains("ZQX"));

    delete_schedule_departures_fixture(&pool, "ZQX").await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_departures -- --ignored --test-threads=1`"]
async fn schedule_departures_a_row_only_for_a_different_date_is_still_404_today() {
    // Proves the route's "always today, server-side" date scoping -- a
    // stale row from a different service_date must never leak through.
    let pool = connect().await;
    delete_schedule_departures_fixture(&pool, "ZQY").await;

    let yesterday = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    sqlx::query(
        "INSERT INTO schedule_network_departures (crs, service_date, departures) VALUES ('ZQY', $1, '[]')",
    )
    .bind(yesterday)
    .execute(&pool)
    .await
    .expect("seed a stale fixture row");

    let router: axum::Router = crate::app::Router::new()
        .merge(router())
        .with_state(test_app(pool.clone()));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/stations/ZQY/schedule-departures")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    delete_schedule_departures_fixture(&pool, "ZQY").await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_departures -- --ignored --test-threads=1`"]
async fn schedule_departures_a_row_for_today_with_empty_departures_is_200_empty_array() {
    let pool = connect().await;
    delete_schedule_departures_fixture(&pool, "ZQZ").await;

    let today = chrono::Utc::now().date_naive();
    sqlx::query(
        "INSERT INTO schedule_network_departures (crs, service_date, departures) VALUES ('ZQZ', $1, '[]')",
    )
    .bind(today)
    .execute(&pool)
    .await
    .expect("seed empty-departures fixture row");

    let router: axum::Router = crate::app::Router::new()
        .merge(router())
        .with_state(test_app(pool.clone()));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/stations/ZQZ/schedule-departures")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!([]));

    delete_schedule_departures_fixture(&pool, "ZQZ").await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            schedule_departures -- --ignored --test-threads=1`"]
async fn schedule_departures_two_rows_render_camel_case_with_trimmed_time() {
    let pool = connect().await;
    delete_schedule_departures_fixture(&pool, "ZRA").await;

    let today = chrono::Utc::now().date_naive();
    let departures = serde_json::json!([
        {"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"},
        {"uid": "C99999", "scheduled": "09:00:00", "destination_crs": null},
    ]);
    sqlx::query(
        "INSERT INTO schedule_network_departures (crs, service_date, departures) VALUES ('ZRA', $1, $2)",
    )
    .bind(today)
    .bind(departures)
    .execute(&pool)
    .await
    .expect("seed two-departure fixture row");

    let router: axum::Router = crate::app::Router::new()
        .merge(router())
        .with_state(test_app(pool.clone()));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/stations/ZRA/schedule-departures")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json.as_array().unwrap().len(), 2);
    assert_eq!(json[0]["uid"], "C11052");
    assert_eq!(json[0]["scheduled"], "08:22");
    assert_eq!(json[0]["destinationCrs"], "CRE");
    assert!(json[1]["destinationCrs"].is_null());
    assert!(json[0].get("destination_crs").is_none(), "no stray snake_case field");

    delete_schedule_departures_fixture(&pool, "ZRA").await;
}
```

(`ZQV`/`ZQW`/`ZQX`/`ZQY`/`ZQZ`/`ZRA` continue this file's own reserved
fixture-CRS sequence after `ZQT`/`ZQU`, staying clear of `station_stats.rs`'s
`ZQQ`/`ZQR`/`ZQS`.)

- [ ] **Step 10: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api schedule_network_departures -- --ignored --test-threads=1
DATABASE_URL=<url> cargo test -p api schedule_departures -- --ignored --test-threads=1
```

Expected: all PASS, zero clippy warnings.

- [ ] **Step 11: Commit**

```bash
git add crates/api/migrations/20260904110000_schedule_network_departures.sql \
        crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs \
        crates/api/src/routes/departures.rs crates/api/src/app.rs crates/api/src/render.rs
git commit -m "Add POST /private/schedule-network-departures and GET /public/stations/{crs}/schedule-departures"
```

---

## Task 3: `crates/schedule-reference` — restructure `poll_once`, publish whole-network departures

**Files:**
- Modify: `crates/schedule-reference/Cargo.toml`
- Modify: `crates/schedule-reference/src/main.rs`
- Modify: `crates/schedule-reference/src/config.rs`
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml`

Depends on Task 1 (`departures_by_crs`/`ScheduleDeparture`) and Task 2 (the
route it POSTs to — not required to compile, only for a real end-to-end
cycle to succeed).

**This is a working, already-deployed production service. The
behavior-preserving constraint below is non-negotiable**: after this task,
`publish_schedule_line_population`'s own per-line loop, its filtering
predicate (`lines_to_publish`, untouched), its JSON body shape
(`line_id`/`service_date`/`population`), and its individual-object POST
(`post_schedule_line_population`, untouched) must be **byte-for-byte
identical in behavior** to before this task — only *how* it obtains
`index`/`today` changes (caller-supplied instead of self-computed). Step 5
below is a mechanical diff-based check of exactly this.

- [ ] **Step 1: Add `chrono-tz` dependency**

`crates/schedule-reference/Cargo.toml`, in `[dependencies]` (alongside the
existing `chrono = { version = "0.4.44", features = ["serde"] }` line):

```toml
chrono-tz = "0.10"
```

(Matches the version every other crate in this workspace already pins for
`chrono-tz` — `common`, `api`, `aggregator`, `poller-tfl`,
`schedule-ingest` — confirm still current at implementation time.)

- [ ] **Step 2: Add the new config field**

`crates/schedule-reference/src/config.rs`, directly after
`schedule_line_population_url`:

```rust
/// The `api` crate's ingestion endpoint for this service's third
/// responsibility: the whole-network trip-search fallback's per-CRS,
/// CIF-derived "next 10 scheduled departures" publish. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1. POST-only, no GET pair -- see that decision's own note on
/// why this differs from `schedule_line_population_url`'s route shape.
#[arg(
    long,
    env,
    default_value = "http://api:8080/private/schedule-network-departures"
)]
pub schedule_network_departures_url: String,
```

- [ ] **Step 3: Restructure `main.rs` — shared index build**

Replace the tail of `poll_once` (currently ending with
`publish_schedule_line_population(client, config, &delivery.mca_path, internal_oauth).await;`)
with:

```rust
    publish_cif_derived_products(client, config, &delivery.mca_path, internal_oauth, &records).await;

    Ok(())
}
```

(`records: Vec<common::StanoxCrsRecord>`, already in scope from the
existing stanox/crs resolve above — this is Decision 1's one change to
`poll_once` itself: pass it through instead of discarding it.)

Replace the entire existing `publish_schedule_line_population` function
with these three functions (the shared wrapper, the modified per-line
publish, and the new per-station publish), in this order:

```rust
/// Task 3's (whole-network-trip-search plan) shared wrapper: builds the
/// whole-network `ScheduleIndex` ONCE from this delivery's `BS`/`BX`/`LO`/
/// `LI`/`CR`/`LT` records, then runs BOTH CIF-derived publishes off that
/// one index/`today` pair -- the per-line publish this crate already had
/// (Task 7 of the option-b-live-consumer plan, UNCHANGED below beyond its
/// own signature: same per-line loop, same individual-object POST, same
/// line-filtering predicate) and the new per-station whole-network publish
/// this plan adds. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1.
async fn publish_cif_derived_products(
    client: &Client,
    config: &Config,
    mca_path: &std::path::Path,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    stanox_crs_records: &[common::StanoxCrsRecord],
) {
    let mca_schedule_text =
        match read_prefixed_lines_multi(mca_path, &["BS", "BX", "LO", "LI", "CR", "LT"]) {
            Ok(text) => text,
            Err(err) => {
                tracing::error!(error = ?err, "failed to read CIF SCHEDULE records from delivery; skipping this cycle's CIF-derived publishes");
                return;
            }
        };

    let index = schedule_query::ScheduleIndex::from_text(&mca_schedule_text);
    // schedule-reference has no rail-day concept of its own yet --
    // publishing against the plain calendar date is deliberate and
    // sufficient here, UNCHANGED from before this restructuring:
    // `schedules_touching`/`departures_by_crs` both resolve STP overlays
    // per calendar date already, and `full-coverage-consumer`'s OWN
    // rail-day gating is what decides Pending/Available for the line
    // population, not this publish step.
    let today = chrono::Utc::now().date_naive();

    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
}

/// UNCHANGED per-line publish logic (per-line loop, per-line individual
/// POST) -- only its own signature changed: `index`/`today` are now
/// shared, caller-supplied inputs (built once by
/// `publish_cif_derived_products`) rather than rebuilt here on every call.
/// Behavior-preserving: the JSON body shape (`line_id`/`service_date`/
/// `population`), the per-line filtering predicate (`lines_to_publish`,
/// untouched), and the individual-object POST (`post_schedule_line_population`,
/// untouched) are all byte-for-byte the same as before this plan's Task 3.
async fn publish_schedule_line_population(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    for line in lines_to_publish(&config.lines) {
        let tiplocs: Vec<&str> = line
            .stations
            .iter()
            .filter_map(|s| s.tiploc.as_deref())
            .collect();
        let resolved = schedule_query::schedules_touching(index, &tiplocs, today);
        let population: Vec<schedule_query::LinePopulationEntry> =
            resolved.into_iter().map(Into::into).collect();
        let body = serde_json::json!({
            "line_id": line.id,
            "service_date": today,
            "population": population,
        });
        if let Err(err) = post_schedule_line_population(
            client,
            &config.schedule_line_population_url,
            internal_oauth,
            &body,
        )
        .await
        {
            tracing::error!(error = ?err, line_id = %line.id, "failed to publish schedule line population; will retry next cycle");
        }
    }
}

/// The whole-network trip-search design doc's Decision 1: every
/// non-cancelled schedule's departure-bearing calling points, bucketed by
/// CRS via this cycle's already-resolved `stanox_crs_records`, capped to
/// the earliest `MAX_DEPARTURES_PER_STATION` per station, published as ONE
/// batch-array POST (not one POST per CRS, unlike the per-line publish
/// above -- see the design doc's Decision 1 for why: this route has one
/// reader, `api` itself, storing every row from one cycle in one
/// transaction).
const MAX_DEPARTURES_PER_STATION: usize = 10; // mirrors poller-ldbws's own
                                                // num_rows=10 default,
                                                // crates/poller-ldbws/src/config.rs:45-46

async fn publish_schedule_network_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = stanox_crs_records
        .iter()
        .map(|r| {
            (
                schedule_query::normalize_tiploc(&r.tiploc).to_string(),
                r.crs.clone(),
            )
        })
        .collect();
    let now = london_local_time_now();

    let by_crs = schedule_query::departures_by_crs(index, today, now, &tiploc_to_crs);
    let rows = schedule_network_departures_rows(by_crs, today);

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_network_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived network departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish schedule-derived network departures; will retry next cycle");
    }
}

/// Pure sort/cap/JSON-shaping logic, split out of
/// `publish_schedule_network_departures` purely so it's unit-testable
/// without a mock HTTP server -- same "pure logic separated from I/O"
/// convention `lines_to_publish`/`read_prefixed_lines_multi` already
/// establish in this file.
fn schedule_network_departures_rows(
    mut by_crs: std::collections::HashMap<String, Vec<schedule_query::ScheduleDeparture>>,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_crs
        .drain()
        .map(|(crs, mut departures)| {
            departures.sort_by_key(|d| d.scheduled);
            departures.truncate(MAX_DEPARTURES_PER_STATION);
            serde_json::json!({ "crs": crs, "service_date": today, "departures": departures })
        })
        .collect()
}

/// The CIF `booked_departure`/`booked_arrival` fields
/// (`schedule_query::records::CallingPoint`'s own doc) are Europe/London
/// LOCAL civil time, not UTC -- comparing them against a naive
/// `chrono::Utc::now().time()` would be wrong by an hour for the ~7 months
/// of British Summer Time. Resolves the design doc's Open Question 1:
/// unlike `crates/api/src/data/eta_blend.rs::london_to_utc` (which resolves
/// a NAIVE local datetime to UTC, and therefore has to handle the
/// ambiguous-hour/nonexistent-hour DST edge cases via `LocalResult`), this
/// goes the other way -- FROM a known UTC instant TO its local
/// Europe/London clock time via `DateTime::with_timezone`, which is always
/// exactly one unambiguous answer.
fn london_local_time_at(instant: chrono::DateTime<chrono::Utc>) -> chrono::NaiveTime {
    instant.with_timezone(&chrono_tz::Europe::London).time()
}

fn london_local_time_now() -> chrono::NaiveTime {
    london_local_time_at(chrono::Utc::now())
}
```

- [ ] **Step 4: Tests**

Add to `main.rs`'s existing `#[cfg(test)] mod poll_once_tests`:

```rust
#[test]
fn london_local_time_at_a_summer_instant_is_one_hour_ahead_of_utc() {
    // 2026-07-15 13:00:00 UTC is 14:00:00 BST (July is daylight saving).
    let instant: chrono::DateTime<chrono::Utc> = "2026-07-15T13:00:00Z".parse().unwrap();
    assert_eq!(
        london_local_time_at(instant),
        chrono::NaiveTime::from_hms_opt(14, 0, 0).unwrap()
    );
}

#[test]
fn london_local_time_at_a_winter_instant_matches_utc() {
    // 2026-01-15 13:00:00 UTC is 13:00:00 GMT (January is not daylight saving).
    let instant: chrono::DateTime<chrono::Utc> = "2026-01-15T13:00:00Z".parse().unwrap();
    assert_eq!(
        london_local_time_at(instant),
        chrono::NaiveTime::from_hms_opt(13, 0, 0).unwrap()
    );
}

#[test]
fn schedule_network_departures_rows_sorts_earliest_first_and_caps_at_ten() {
    let mut by_crs = std::collections::HashMap::new();
    let departures: Vec<schedule_query::ScheduleDeparture> = (0..12)
        .rev() // deliberately out of order
        .map(|hour| schedule_query::ScheduleDeparture {
            uid: format!("U{hour:05}"),
            scheduled: chrono::NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
            destination_crs: None,
        })
        .collect();
    by_crs.insert("EUS".to_string(), departures);

    let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
    let rows = schedule_network_departures_rows(by_crs, today);

    assert_eq!(rows.len(), 1);
    let row_departures = rows[0]["departures"].as_array().unwrap();
    assert_eq!(row_departures.len(), MAX_DEPARTURES_PER_STATION);
    assert_eq!(row_departures[0]["uid"], "U00000", "earliest-first after sort");
    assert_eq!(row_departures[9]["uid"], "U00009", "capped at 10, entries 10 and 11 dropped");
}

#[test]
fn schedule_network_departures_rows_produces_one_row_per_crs_key() {
    let mut by_crs = std::collections::HashMap::new();
    by_crs.insert(
        "EUS".to_string(),
        vec![schedule_query::ScheduleDeparture {
            uid: "U1".to_string(),
            scheduled: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            destination_crs: Some("CRE".to_string()),
        }],
    );
    by_crs.insert(
        "WAT".to_string(),
        vec![schedule_query::ScheduleDeparture {
            uid: "U2".to_string(),
            scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            destination_crs: None,
        }],
    );

    let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
    let rows = schedule_network_departures_rows(by_crs, today);

    assert_eq!(rows.len(), 2);
    let crs_values: Vec<&str> = rows.iter().map(|r| r["crs"].as_str().unwrap()).collect();
    assert!(crs_values.contains(&"EUS"));
    assert!(crs_values.contains(&"WAT"));
    for row in &rows {
        assert_eq!(row["service_date"], "2026-09-04");
    }
}
```

The existing `lines_to_publish_includes_...`/`lines_to_publish_excludes_...`
tests are untouched — `lines_to_publish` itself did not change.

- [ ] **Step 5: Behavior-preservation check (manual, not a test)**

```bash
git diff -- crates/schedule-reference/src/main.rs
```

Confirm the diff on the old `publish_schedule_line_population` body touches
only: (a) the function signature (`mca_path: &Path` → `index: &ScheduleIndex,
today: NaiveDate`), (b) the removed internal `read_prefixed_lines_multi`/
`ScheduleIndex::from_text`/`let today = ...` lines (now in
`publish_cif_derived_products` instead). The `for line in
lines_to_publish(...)` loop body itself — every line inside it — must be
byte-identical to `git show HEAD~1:crates/schedule-reference/src/main.rs`'s
version. If it isn't, stop and diagnose; that means this task changed
production per-line-publish behavior, which it must not do.

- [ ] **Step 6: Test, lint, build**

```bash
cargo fmt --all
cargo clippy -p schedule-reference --all-features
cargo test -p schedule-reference
```

Expected: all PASS (including the pre-existing `poll_once_tests`), zero
clippy warnings.

- [ ] **Step 7: Helm — env var for the new URL**

`charts/distant-signal/templates/schedulefeed-deployment.yaml`, directly
after the existing `SCHEDULE_LINE_POPULATION_URL` entry:

```yaml
            # Whole-network-trip-search plan: this crate's third
            # responsibility, alongside stanox/crs and per-line CIF
            # SCHEDULE population.
            - name: SCHEDULE_NETWORK_DEPARTURES_URL
              value: {{ printf "%s/private/schedule-network-departures" (include "distant-signal.apiBaseUrl" .) | quote }}
```

Also update that block's leading comment (currently listing
`storage_dir, poll_interval_secs, api_ingest_url,
schedule_line_population_url, lines (LINES_DIR), ...`) to add
`schedule_network_departures_url` to the list.

```bash
helm template charts/distant-signal > /dev/null
```

- [ ] **Step 8: Commit**

```bash
git add crates/schedule-reference/Cargo.toml crates/schedule-reference/src/main.rs \
        crates/schedule-reference/src/config.rs \
        charts/distant-signal/templates/schedulefeed-deployment.yaml
git commit -m "schedule-reference: share one ScheduleIndex per cycle, publish per-station CIF-derived network departures"
```

---

## Task 4: Frontend — CIF fallback picker on `TrackTrainForm`

**Files:**
- Modify: `frontend/components/TrackTrainForm.tsx`
- Modify: `frontend/components/TrackTrainForm.test.tsx`

Depends on Task 2's wire shape being final (this task's mocked JSON must
match Task 2's actual field names exactly), not on Task 2 or Task 3 being
deployed.

- [ ] **Step 1: Add `ScheduleDepartureRow` and the `Picker` union type**

Directly after the existing `DepartureRow` interface:

```tsx
/** Wire shape of `GET /public/stations/{crs}/schedule-departures`
 * (`crates/api/src/render.rs::schedule_departure_json`) -- deliberately
 * NOT `DepartureRow`: no `operator`, no live running-status fields at all
 * (`isCancelled`/`delayMinutes`/`estimated`/`cancelReason`/`delayReason`),
 * because the CIF SCHEDULE feed genuinely has none of that -- see
 * docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
 * Decision 2/5. `destinationCrs` is nullable: `null` when the terminating
 * TIPLOC has no `stanox_crs` row (a real, if rare, gap). */
interface ScheduleDepartureRow {
  uid: string;
  scheduled: string;
  destinationCrs: string | null;
}

/** `'unavailable'` replaces the old `'not-sampled'` name: it now means
 * neither the LDBWS live board NOR the CIF-derived timetable had data for
 * this station -- see Decision 3/5. */
type Picker =
  | { source: 'ldbws'; rows: DepartureRow[] }
  | { source: 'cif'; rows: ScheduleDepartureRow[] }
  | 'unavailable'
  | null;
```

Replace the existing state line:

```tsx
const [picker, setPicker] = useState<Picker>(null);
```

(removing the old `const [departures, setDepartures] = useState<DepartureRow[]
| 'not-sampled' | null>(null);` line entirely).

- [ ] **Step 2: Replace the fetch effect**

```tsx
useEffect(() => {
  if (!originValid) {
    setPicker(null);
    return;
  }
  const controller = new AbortController();
  const crs = originCrs.trim().toUpperCase();

  fetch(`/api/stations/${crs}/departures`, { signal: controller.signal })
    .then((res) => {
      if (res.status === 404) {
        // Fallback ONLY on 404 -- an LDBWS network blip or 500 must NOT
        // silently swap in the CIF picker; `!res.ok` still maps to `null`
        // exactly as today, leaving the picker absent rather than
        // switching sources on an error condition. Per
        // docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
        // Decision 3.
        return fetch(`/api/stations/${crs}/schedule-departures`, { signal: controller.signal }).then(
          (cifRes) => {
            if (cifRes.status === 404) return setPicker('unavailable');
            if (!cifRes.ok) return setPicker(null);
            return cifRes.json().then((rows: ScheduleDepartureRow[]) => setPicker({ source: 'cif', rows }));
          },
        );
      }
      if (!res.ok) return setPicker(null);
      return res.json().then((rows: DepartureRow[]) => setPicker({ source: 'ldbws', rows }));
    })
    .catch(() => {}); // aborted or network blip -- leave prior state, same posture as useSuggestions
  return () => controller.abort();
}, [originCrs, originValid]);
```

- [ ] **Step 3: Add `pickCifDeparture`**

Directly after the existing `pickDeparture`:

```tsx
/** CIF-derived sibling of `pickDeparture` -- fills only
 * Destination/Scheduled-departure. `operator` is left exactly as the user
 * already typed it, never cleared, never guessed -- the CIF SCHEDULE feed
 * has no operator field at all (Decision 2). If `row.destinationCrs` is
 * `null` (the terminating TIPLOC has no `stanox_crs` row), the existing
 * Destination field is left untouched too, for the same "never guess,
 * never clobber with a blank" reason. */
function pickCifDeparture(row: ScheduleDepartureRow) {
  if (row.destinationCrs !== null) setDestinationCrs(row.destinationCrs);
  const [hh, mm] = row.scheduled.split(':');
  const today = dayjs().format('YYYY-MM-DD');
  setScheduledDeparture(`${today} ${hh}:${mm}:00`);
}
```

- [ ] **Step 4: Replace the picker rendering block**

Replace the existing three `{departures === 'not-sampled' && ...}` /
`{Array.isArray(departures) && departures.length === 0 && ...}` /
`{Array.isArray(departures) && departures.length > 0 && ...}` blocks with:

```tsx
{picker === 'unavailable' && (
  <Text size="sm" c="dimmed">
    No departure information is available for this station — enter the details below.
  </Text>
)}
{picker !== null && picker !== 'unavailable' && picker.rows.length === 0 && (
  <Text size="sm" c="dimmed">
    No live departures currently on the board for this station right now.
  </Text>
)}
{picker !== null && picker !== 'unavailable' && picker.rows.length > 0 && picker.source === 'ldbws' && (
  <ScrollArea mah={220} offsetScrollbars>
    <Stack gap="xs">
      {picker.rows.map((row) => {
        const clickable = !row.isCancelled;
        const badge = row.isCancelled ? (
          <Badge color="red">Cancelled</Badge>
        ) : row.delayMinutes > 0 ? (
          <Badge color="orange">+{row.delayMinutes} min</Badge>
        ) : (
          <Badge color="green">On time</Badge>
        );
        return (
          <Group
            key={row.serviceId}
            justify="space-between"
            wrap="nowrap"
            role={clickable ? 'button' : undefined}
            tabIndex={clickable ? 0 : undefined}
            onClick={clickable ? () => pickDeparture(row) : undefined}
            onKeyDown={
              clickable
                ? (event) => {
                    if (event.key === 'Enter' || event.key === ' ') pickDeparture(row);
                  }
                : undefined
            }
            style={{ cursor: clickable ? 'pointer' : 'default', opacity: clickable ? 1 : 0.6 }}
          >
            <Text size="sm">
              {row.scheduled} · {row.destinationCrs} · {row.operator}
            </Text>
            {badge}
          </Group>
        );
      })}
    </Stack>
  </ScrollArea>
)}
{picker !== null && picker !== 'unavailable' && picker.rows.length > 0 && picker.source === 'cif' && (
  <>
    <Text size="sm" c="dimmed">
      Live departure boards aren&apos;t available for this station. Showing the scheduled timetable
      instead — this is not live running information and may be up to 30 minutes out of date.
    </Text>
    <ScrollArea mah={220} offsetScrollbars>
      <Stack gap="xs">
        {picker.rows.map((row) => (
          <Group
            key={row.uid}
            justify="space-between"
            wrap="nowrap"
            role="button"
            tabIndex={0}
            onClick={() => pickCifDeparture(row)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') pickCifDeparture(row);
            }}
            style={{ cursor: 'pointer' }}
          >
            <Text size="sm">
              {row.scheduled}
              {row.destinationCrs ? ` · ${row.destinationCrs}` : ''}
            </Text>
          </Group>
        ))}
      </Stack>
    </ScrollArea>
  </>
)}
```

(No status badge in the CIF branch at all — Decision 5: every row shown
already had cancelled schedules filtered out server-side by
`schedules_touching`, so "On time" would imply live confirmation that
doesn't exist. Every CIF row is clickable, unlike the LDBWS branch, which
disables cancelled rows — there is no cancelled state to disable here.)

- [ ] **Step 5: Update `TrackTrainForm.test.tsx`'s `mockFetchByUrl` helper**

Replace the existing single-argument helper:

```tsx
/** Routes a mocked `fetch` call by URL: `/api/stations/{crs}/departures`
 * (LDBWS), `/api/stations/{crs}/schedule-departures` (the CIF fallback,
 * this task), suggestion fetches, and everything else (the track-submit
 * call). `departures` defaults to an inert empty-array 200 so a test only
 * needs to override the branch it actually cares about; `scheduleDeparatures`
 * has no default -- a test that expects the CIF fallback to fire but
 * doesn't configure it will throw loudly rather than silently returning
 * something misleading, since most tests never expect a 404 from the
 * `departures` fetch at all. */
function mockFetchByUrl(
  options: {
    departures?: () => Response;
    scheduleDepartures?: () => Response;
  } = {},
) {
  const { departures = () => new Response(JSON.stringify([]), { status: 200 }), scheduleDepartures } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (/\/api\/stations\/[A-Za-z]{3}\/schedule-departures$/.test(url)) {
      if (!scheduleDepartures) {
        throw new Error(`unexpected schedule-departures fetch for ${url} -- this test did not configure one`);
      }
      return Promise.resolve(scheduleDepartures());
    }
    if (/\/api\/stations\/[A-Za-z]{3}\/departures$/.test(url)) {
      return Promise.resolve(departures());
    }
    if (url.startsWith('/api/stations?') || url.startsWith('/api/tocs?')) {
      return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    }
    return Promise.resolve(new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }));
  });
}
```

Update every existing call site in the file to the new options-object
form:

- `'typing a valid origin CRS triggers a departures fetch to /api/stations/{ORIGIN}/departures'`:
  `mockFetchByUrl({ departures: () => new Response(JSON.stringify([]), { status: 200 }) })`
- `'a 404 response renders the "not available to browse" text'`: **rename**
  to `'a 404 from both LDBWS and CIF renders the "no departure information" unavailable text'`
  and use
  `mockFetchByUrl({ departures: () => new Response('not found', { status: 404 }), scheduleDepartures: () => new Response('not found', { status: 404 }) })`;
  update the assertion text to `'No departure information is available for this station — enter the details below.'`.
- `'a 200 [] response renders the "no live departures right now" text'`:
  `mockFetchByUrl({ departures: () => new Response(JSON.stringify([]), { status: 200 }) })`
  (unchanged — LDBWS succeeds directly, no fallback fires).
- `'renders a cancelled and an on-time departure, with the cancelled row not clickable'`:
  `mockFetchByUrl({ departures: () => new Response(JSON.stringify(departures), { status: 200 }) })`.
- `'clicking a non-cancelled row fills destinationCrs/operator/scheduledDeparture'`:
  same as above.
- `'changing the origin away from a previously-picked value does not clear already-filled fields'`:
  same as above.

- [ ] **Step 6: Add new tests for the CIF fallback**

Add inside the existing `describe('live departures picker', ...)` block:

```tsx
const scheduleDepartures: { uid: string; scheduled: string; destinationCrs: string | null }[] = [
  { uid: 'C11052', scheduled: '08:22', destinationCrs: 'CRE' },
  { uid: 'C99999', scheduled: '09:00', destinationCrs: null },
];

it('a 404 from LDBWS followed by a CIF 200 renders the CIF picker with its staleness disclaimer, no badges', async () => {
  const fetchMock = mockFetchByUrl({
    departures: () => new Response('not found', { status: 404 }),
    scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
  });
  vi.stubGlobal('fetch', fetchMock);

  renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

  expect(
    await screen.findByText(
      /Live departure boards aren't available for this station\. Showing the scheduled timetable/,
    ),
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: /08:22/ })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: /09:00/ })).toBeInTheDocument();
  expect(screen.queryByText('On time')).not.toBeInTheDocument();
  expect(screen.queryByText('Cancelled')).not.toBeInTheDocument();
});

it('a 404 from LDBWS followed by a CIF 200 [] renders the shared "no live departures right now" text', async () => {
  const fetchMock = mockFetchByUrl({
    departures: () => new Response('not found', { status: 404 }),
    scheduleDepartures: () => new Response(JSON.stringify([]), { status: 200 }),
  });
  vi.stubGlobal('fetch', fetchMock);

  renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

  expect(
    await screen.findByText('No live departures currently on the board for this station right now.'),
  ).toBeInTheDocument();
});

it('a non-404, non-ok LDBWS response does not fall back to CIF at all', async () => {
  const fetchMock = mockFetchByUrl({ departures: () => new Response('server error', { status: 500 }) });
  vi.stubGlobal('fetch', fetchMock);

  renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);

  await waitFor(() => expect(fetchMock).toHaveBeenCalled());
  expect(screen.queryByText('No departure information is available for this station — enter the details below.')).not.toBeInTheDocument();
  expect(screen.queryByText(/Showing the scheduled timetable/)).not.toBeInTheDocument();
  expect(screen.queryByText('No live departures currently on the board for this station right now.')).not.toBeInTheDocument();
});

it('clicking a CIF row with a real destinationCrs fills destination and scheduled departure, leaving operator untouched', async () => {
  const fetchMock = mockFetchByUrl({
    departures: () => new Response('not found', { status: 404 }),
    scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
  });
  vi.stubGlobal('fetch', fetchMock);

  renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
  const row = await screen.findByRole('button', { name: /08:22/ });
  const today = dayjs().format('YYYY-MM-DD');
  fireEvent.click(row);

  expect(screen.getByRole('combobox', { name: /Destination CRS code/ })).toHaveValue('CRE');
  expect(screen.getByRole('combobox', { name: /Operator/ })).toHaveValue('');
  const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
  expect(picker.value).toBe(`${today} 08:22:00`);
});

it('clicking a CIF row with a null destinationCrs leaves any existing destination untouched', async () => {
  const fetchMock = mockFetchByUrl({
    departures: () => new Response('not found', { status: 404 }),
    scheduleDepartures: () => new Response(JSON.stringify(scheduleDepartures), { status: 200 }),
  });
  vi.stubGlobal('fetch', fetchMock);

  renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
  const destinationField = screen.getByRole('combobox', { name: /Destination CRS code/ });
  fireEvent.change(destinationField, { target: { value: 'EXISTING' } });

  const row = await screen.findByRole('button', { name: /09:00/ });
  fireEvent.click(row);

  expect(destinationField).toHaveValue('EXISTING');
  const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
  expect(picker.value).toMatch(/09:00:00$/);
});
```

- [ ] **Step 7: Test and build**

```bash
cd frontend
npm test
npm run build
```

Expected: all PASS, build succeeds.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "Add a CIF-derived scheduled-departures fallback picker to /track, shown only when LDBWS has no board"
```

---

## Task 5: Final verification

- [ ] **Step 1: Full workspace verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -- --ignored --test-threads=1
```

- [ ] **Step 2: Full frontend verification**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 3: Helm template check**

```bash
helm template charts/distant-signal > /dev/null
```

- [ ] **Step 4: Cheap real-data timing sanity check (design doc Open
      Question 2, if `timetable_full.zip` or an equivalent real MCA extract
      is available locally)**

Not a CI-gated test (no real CIF extract is available in CI) — a manual,
best-effort check per the design doc's own framing ("worth a cheap real
check... before committing to this shape"). If a real extract is at hand,
time `schedule_query::departures_by_crs` against it (e.g. via a throwaway
`cargo run --example` or a `#[test]` gated behind an env var, mirroring
`schedule-query/examples/inspect.rs`'s own "explicitly labeled dev-only
tool, not part of `cargo test`" convention) and note the wall-clock time
alongside `schedule-reference`'s own cycle-duration metric
(`schedule_reference_cycle_duration_seconds`). If unavailable, skip this
step and leave Open Question 2 flagged exactly as the design doc left it —
do not fabricate a number.

- [ ] **Step 5: Manual smoke check against a real deployment (if available)**

`POST /private/schedule-network-departures` cannot be smoke-tested
directly (private, requires a service credential) — instead, after one
real `schedule-reference` cycle has run against a live deployment:
`GET /public/stations/{crs}/schedule-departures` for a CRS known to be in
`stanox_crs` but NOT in `poller-ldbws`'s sampled set — confirm a populated
array with `uid`/`scheduled`/`destinationCrs` fields, no `operator`. Load
`/track?origin={that crs}` in a browser and confirm: the LDBWS picker's own
404 triggers the CIF fallback, the staleness disclaimer renders, no status
badges appear, and clicking a row fills Destination/Scheduled-departure
(and only those two fields) correctly, leaving the form still submittable.
Separately, load `/track?origin={a real LDBWS-sampled station}` and confirm
the existing live picker's behavior is completely unchanged (no disclaimer,
badges present, `operator` fills as before).

- [ ] **Step 6: Confirm no stray edits outside this plan's file scope**

```bash
git diff --stat main...HEAD
```

Compare against this plan's Global Constraints "File scope" list — flag
anything unexpected before considering the branch done.

## Testing

Summarized (see each task's own steps for the authoritative detail):

- **`crates/schedule-query`**: unit tests for `departures_by_crs` covering
  origin-departure bucketing + destination resolution, the `now`-forward
  filter, STP=C cancellation exclusion, an unresolved origin TIPLOC being
  dropped vs. an unresolved destination TIPLOC becoming `None`, and
  Intermediate calling points each getting their own bucket entry.
- **`crates/api`**: unit tests for `schedule_departure_json` (snake_case →
  camelCase mapping, seconds trimmed, `null` destination preserved as
  `null`); `#[ignore]`-gated DB tests for the POST route (upsert, wholesale
  replace) and the GET route (404 for no row, 404 for a row on a different
  `service_date`, `200 []`, `200` with two rows including a `null`
  destination), following `station_stats.rs`'s/`departures.rs`'s existing
  fixture-and-`oneshot` convention with reserved `ZQV`–`ZRA` fixture CRS
  codes.
- **`crates/schedule-reference`**: unit tests for the new
  `london_local_time_at` (a BST instant and a GMT instant) and
  `schedule_network_departures_rows` (sort-and-cap, one row per CRS); the
  pre-existing `lines_to_publish`/`read_prefixed_lines`/
  `embedded_sequence_number` tests are untouched; a manual diff-based
  behavior-preservation check (Task 3 Step 5) in place of a redundant
  regression test, since the per-line publish logic itself did not change.
- **`frontend`**: new/updated tests in `TrackTrainForm.test.tsx` covering
  the 404→CIF-fallback chain (populated, empty, both-404), the "no
  fallback on a non-404 error" case, both `pickCifDeparture` branches
  (real and `null` `destinationCrs`), and confirming every pre-existing
  LDBWS-picker test still passes unchanged against the updated
  `mockFetchByUrl` helper.
- **CI**: every new DB-backed test runs under the existing `api` crate's
  `--ignored` CI job — no new CI job needed.
