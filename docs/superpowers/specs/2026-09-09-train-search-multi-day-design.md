# Design: Multi-Day `GET /public/trains/search`

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline) -- same posture
as `2026-09-08-calling-point-train-search-design.md`.** This document turns
the calling-point-first `/trains` search (`2026-09-07-train-listing-page-design.md`,
as revised by `2026-09-07-train-listing-destination-search-sizing-design.md`
and generalized by `2026-09-08-calling-point-train-search-design.md` and
`2026-09-08-destination-arrival-time-filter-design.md`) from "always today,
server-side" into "today, or a nearby day the caller names."

Required reading consumed in full before this document was written: all
four design docs named above; `crates/api/src/routes/trains.rs` (whole
file, including `db_tests`); `crates/api/src/data/queries.rs`'s whole
`schedule_destination_departures` section (`:900-1220`+ its `db_tests`);
`crates/api/migrations/20260907130000_schedule_destination_departures.sql`,
`20260908120000_..._calling_point_search.sql`,
`20260908130000_..._destination_arrival.sql`; `crates/schedule-reference/src/main.rs`
(`publish_cif_derived_products`, `publish_schedule_destination_departures`,
`schedule_destination_departures_rows`, `london_local_time_now`/
`london_local_time_at`); `crates/schedule-query/src/resolve.rs` (all of it,
including `resolve_for_date`, `schedules_touching`, `schedule_for_uid`,
`departures_by_destination_crs`); `crates/aggregator/src/queries.rs::prune_schedule_destination_departures`
and its `Config::schedule_destination_departures_retention_days`;
`frontend/components/TrainSearchForm.tsx` and its test; `frontend/components/TrackTrainForm.tsx`
and `frontend/app/lines/[id]/history/HistoryRangePicker.tsx` (existing
`@mantine/dates` usage); commits `baa4e75` and `8250a9a` in full (`git show`),
for the exact UTC/London date-time skew bug class this document must not
reintroduce.

## 0. The load-bearing finding: this needs no schema change and no index
change, and that is a real design decision, not an oversight

The brief's own framing assumes `from`/`to` become full datetimes, which
would make `service_date` a range predicate and force reordering the
leading index. §5 below picks a different wire shape -- one explicit,
optional `date=YYYY-MM-DD` parameter, layered on top of the existing
`"HH:MM"` `from`/`to`/`destination_from`/`destination_to` pairs, unchanged
in format -- precisely *because* every request this route serves still
names exactly one rail day. Under that shape `service_date` remains an
**equality** predicate in every query, exactly as it is today; it is simply
no longer hardcoded to `chrono::Utc::now()`'s date. §4 shows why that means
`schedule_destination_departures_calling_point_idx`
(`service_date, origin_crs, scheduled, train_uid`) needs no reordering, and
no new column is needed anywhere. The only real code changes are: (1) a
bounded forward-publish loop in `schedule-reference`, (2) threading a
caller-supplied `service_date` through `routes::trains`/`queries.rs` instead
of always computing `today`, and (3) a `date` control in `TrainSearchForm`.
No migration file is part of this plan.

## 1. How many days forward, and how many back

**Recommendation: publish `today..=today+6` (7 calendar days, configurable),
read-accessible back to whatever the existing 2-day retention window still
holds. No change to retention.**

This table's cost profile is not comparable to `schedule_line_population`'s
(the only other data product in this codebase with a "future dates" scoping
document, `2026-09-06-schedule-line-population-future-dates-design.md`,
which reasoned about its 14-30 day recommendation against **109 rows/day**).
`schedule_destination_departures` is **~377,000 rows/day**
(`2026-09-07-train-listing-destination-search-sizing-design.md` §1.2's own
derivation: 25,305 non-cancelled schedules/day × 14.9 mean departure-bearing
calling points/schedule) -- a **~3,459x** larger per-day cost. The
14-30-day range recommended there does not transfer here by analogy; it has
to be re-derived against this table's own numbers.

**Publish-payload arithmetic, extending the calling-point-search design's
own "~80 bytes, not ~55" per-entry estimate** (`main.rs`'s own doc comment
on `publish_schedule_destination_departures`, post-`true_origin_crs`/
`destination_arrival`):

| N (forward days published per cycle) | Rows/cycle | Payload/cycle (≈80B/row) | Steady-state table rows (N forward + 2 backward) |
|---|---|---|---|
| 1 (today only, current behavior) | 377,000 | ~30MB | ~1,131,000 (3 days) |
| 3 | 1,131,000 | ~90.5MB | ~1,885,000 (5 days) |
| 7 (recommended) | 2,639,000 | ~211MB | ~3,393,000 (9 days) |
| 14 | 5,278,000 | ~422MB | ~6,032,000 (16 days) |
| 30 | 11,310,000 | ~905MB | ~12,064,000 (32 days) |

Table+index storage, using the sizing addendum's own "~40-60MB of
table+index per service date" figure: at N=7, roughly `9 × 50MB ≈ 450MB`
steady-state -- comfortably small for Postgres, and not the binding
constraint. **The real cost that scales with N is publish-time write
traffic** (the DELETE+UNNEST-INSERT pair, once per CIF delivery, i.e.
roughly once a day -- `2026-09-07-...-sizing-design.md` §1.3), not storage.

Why 7, not 14 or 30: the product need named in this feature's brief is
"search a different day," i.e. short-term trip planning (checking next
Tuesday, this weekend) -- a materially different use case from the
tracked-train-pin feature the 14-30 day sibling document was scoped for,
where a pin can legitimately be created arbitrarily far in advance and
needs to eventually resolve no matter how far out it was made. A search UI
has no equivalent "must eventually work, however far out" obligation: a
user planning a trip a month away can simply search again closer to the
date. Seven days is a full week of lookahead at roughly a third of N=14's
or a twelfth of N=30's publish-write cost, and -- per this section's own
honesty convention -- is a reasoned choice pending real usage data, not a
derived one. It is a plain config value
(`Config::schedule_destination_departures_forward_days`, default `7`,
mirroring `schedule_destination_departures_retention_days`'s own shape),
changeable without a migration or a code change if real usage ever argues
for more.

**Backward: no change to `schedule_destination_departures_retention_days`
(default 2).** The existing backward-pruning job already retains today and
roughly the prior two days; nothing in this feature needs to extend that
window. Critically, extending *read* access to that window costs nothing
new and carries none of `2026-09-06-schedule-line-population-past-dates-design.md`'s
risk: that document's "Regime B" danger (reconstructing a date from a later,
possibly-already-corrected CIF extract, silently losing an expired STP
overlay) applies only to *computing* a past date's data after the fact.
This feature does no such reconstruction -- it only exposes, via `date`,
rows that `schedule-reference` already wrote **on that day itself**, the
same "Regime A" case that document already found "provably as accurate as
present-day resolution... no new correctness risk to document." A caller
asking for `date` further back than the retention window simply gets this
route's existing, unmodified 404 ("no CIF publish has landed for that day"),
same as a future date beyond the forward window -- see §2 and §5.

## 2. Proactive N-day publish loop, not on-demand compute -- the existing
"publish-then-poll, never synchronous" posture is kept, not reopened

The module doc's "no date parameter... publish-then-poll, never
synchronous" stance (`trains.rs` lines 14-19, 34-35) was written for the
single-day case. Re-examined here for a *bounded* forward window, it holds
just as strongly, for reasons that are if anything sharper than the
sibling's:

- **On-demand compute for a far-future date would require `api` to gain a
  dependency it does not have today.** `2026-09-06-schedule-line-population-future-dates-design.md`'s
  Approach B already identified this cost precisely (for a ~109-row/day
  product) and rejected it: either `api` links `schedule_query` and rebuilds
  a `ScheduleIndex` from the raw ~707MB CIF delivery itself (new I/O, new
  deployment dependency, duplicated responsibility), or `schedule-reference`
  grows a synchronous "compute this date now" HTTP endpoint it has never had
  (it has no inbound HTTP server at all today -- confirmed, it only ever
  POSTs out). Both costs apply unchanged here, and apply *worse*: this
  product's resolve pass touches the whole network (~25,305 schedules),
  not one line's subset, so an on-demand path would be markedly more
  expensive per request, against an **unauthenticated public route** with
  no existing rate limiting beyond `MAX_SEARCH_LIMIT`'s clamp.
- **Correctness without new invalidation machinery.** The future-dates
  sibling's Dimension 4 finding transfers directly: a proactive,
  recompute-the-whole-window-every-cycle design gets VSTP/STP-overlay
  correction pickup for free (every in-window date is re-derived from the
  newest `ScheduleIndex` on every processed delivery, exactly like `today`
  already is), with no separate "did this date change" check. An on-demand
  or cache-on-first-use path would not get this for free and would need to
  reinvent the same "recompute everything, every cycle" logic just to stay
  correct -- more moving parts for no benefit.
- **Minimal, mechanical extension of already-correct code.** `departures_by_destination_crs`
  (`resolve.rs`) already takes a bare `NaiveDate` parameter and has no
  date-dependent assumption beyond what `resolve_for_date` (also already
  date-agnostic, confirmed by reading it -- see §4) provides. Extending the
  publish loop to call it N times instead of once is not new architecture.

**Decision: extend `publish_schedule_destination_departures` to be called
once per date in `today..=today+forward_days-1`**, each call scoped to
exactly one `service_date`, **not** one combined multi-day call. This is a
deliberate shape choice, not an afterthought: because
`upsert_schedule_destination_departures`'s DELETE-then-UNNEST-INSERT is
already, today, scoped to "the batch's own distinct service dates" (a
single `ANY($dates)` DELETE already generalized to a set, per its own doc
comment), publishing **one date per POST** means every cycle's publish
payload stays at the already-measured ~30MB/date regardless of how large
`forward_days` is -- **the 50,000-row chunking escape hatch the sizing
addendum names as its fallback is never needed**, because no single POST
ever exceeds one day's ~377,000 rows. This is strictly simpler than
chunking a combined N-day batch and needs zero changes to the ingest
route (`routes::ingest::post_schedule_destination_departures`) or to
`upsert_schedule_destination_departures` itself -- both already operate
correctly on a single date's rows, which is exactly what each of the N
calls now sends.

Dates beyond the forward window remain genuinely unsupported -- a search for
`date` 30 days out gets the same honest 404 as today's "not published yet,"
with no special-cased rejection message. This mirrors the future-dates
sibling's own honestly-stated limit: "a pin for a date beyond the window is
no better off than today... every approach considered here shares some
version of this limit."

## 3. Retention/pruning: no new job, and here is why explicitly

**Forward days need no pruning job.** A date that ages out of the forward
window (yesterday's `today+6` becomes today's `today+5`, etc.) does not need
active deletion the way a sibling table's trailing edge does -- it simply
*becomes* a nearer date next cycle and keeps being recomputed like any other
in-window date, until it eventually becomes "today" itself and then ages
into the **backward** retention window, which already has an active pruning
job (`prune_schedule_destination_departures`, unchanged, `(default 2)` days).
Concretely: no date is ever skipped, double-published, or left to rot --
every date from `today-2` (retained) through `today+6` (published) is
covered by exactly one of the two existing mechanisms (backward: delete
after 2 days; forward: recompute fresh every cycle until it becomes
"today"), and nothing new needs to be built to connect them. This directly
answers the brief's own hint ("probably not [needed] -- future days age
into 'today' naturally -- but say so explicitly").

**No change to `prune_schedule_destination_departures` itself.** Its
`WHERE service_date < CURRENT_DATE - ($1 || ' days')::interval` predicate is
already independent of how many *forward* days exist in the table at any
moment -- it only ever looks backward from `CURRENT_DATE`. Extending the
forward publish window does not change what that job considers "old."

## 4. Index/query shape: unchanged, because `service_date` stays an equality
predicate

The brief poses this conditionally: *if* `from`/`to` become datetimes
spanning multiple `service_date` values, a range-scan-friendly,
station-first `(origin_crs, service_date, scheduled, train_uid)` ordering
would be needed instead of the current
`(service_date, origin_crs, scheduled, train_uid)`. Worked through
directly, for completeness, because the brief asks for the reasoning rather
than an assertion:

- With `service_date` leading and made a **range** (not equality), Postgres
  can range-scan across dates but cannot use `origin_crs` as a second
  index-level equality seek within that range under the classic single-seek
  btree plan this codebase's other indexes rely on -- it would fall back to
  filtering `origin_crs` row-by-row across every station's rows for every
  date in range, which is exactly the "Waterloo costs the same as Bootle
  Oriel Road" property the current design deliberately built and measured
  for (`2026-09-07-...-sizing-design.md` §3, Approach C) being lost for the
  new multi-day case.
- Reordering to `(origin_crs, service_date, scheduled, train_uid)` would fix
  that (equality-seek on `origin_crs`, then an ordered range walk across
  `(service_date, scheduled)`) **without regressing the single-day case**:
  for a same-day query, both orderings are leading-equality-then-equality,
  which Postgres treats identically regardless of which equality column
  comes first.

**This reasoning is moot under this document's actual wire-shape decision
(§5): `date` selects exactly one `service_date` per request, never a range,
so `service_date` stays an equality predicate in every query this route
ever issues, identical in shape to today's.** The existing index needs no
reordering and no migration. This is the direct payoff of choosing a single
`date` parameter over full datetime fields -- it was evaluated, not assumed,
and the conditional case above is recorded so a future change to the wire
shape (if `from`/`to` ever genuinely need to span days) knows exactly what
index change that would require.

`resolve_for_date`/`schedules_touching`/`schedule_for_uid`
(`crates/schedule-query/src/resolve.rs:42-70, 77-96, 361-364`) are confirmed,
by direct reading, to already be fully date-parametric with no "today"
special-casing anywhere -- `resolve_for_date`'s own filter is a bare
`date_from <= date && date <= date_to && days_of_week[weekday]` comparison,
identical in cost and behavior for any date within a schedule's validity
window. No `schedule_query` change is needed for a bounded forward window,
consistent with `2026-09-06-schedule-line-population-future-dates-design.md`'s
identical finding for that sibling product.

## 5. Wire/API shape: one optional `date=YYYY-MM-DD`, not full datetimes

**Decision: add a single optional query parameter, `date`, in the same
`YYYY-MM-DD` form already used by `/train/{uid}/{date}`'s path segment.
`from`/`to`/`destination_from`/`destination_to` are unchanged --
`"HH:MM"`, scoped to `date` exactly as they are scoped to "today" now.**
Confirming the brief's own alternative framing: yes, this is simpler and
less disruptive than turning every time field into an independent datetime.
Reasons, concretely:

- **All four existing time fields already implicitly share one day.** Origin
  time (`from`/`to`) and destination-arrival time (`destination_from`/
  `destination_to`) are both, today, scoped to the same single rail day.
  Turning each into an independent datetime would mean parsing and
  validating four datetimes per request for information ("which day") that
  is identical across all four -- pure duplication, and four more
  opportunities to disagree with each other (e.g. a caller who sets `from`'s
  date to one day and `destination_from`'s to another, a case with no
  sensible meaning this route would then have to reject).
- **Avoids repeating the DST-parsing hazard four times.** Parsing a
  caller-supplied *local* datetime string into a civil instant is exactly
  the class of operation `crates/api/src/data/eta_blend.rs::london_to_utc`
  exists for, specifically because it is ambiguous/nonexistent across a BST
  transition (`LocalResult`-handling, cited directly in this route's own
  sibling function `london_local_time_at`'s doc comment). A bare calendar
  `date` has no such ambiguity -- `NaiveDate::parse_from_str` is a total,
  unambiguous function. Four independent datetime fields would need this
  handling four times over; one `date` field needs it zero times.
- **Matches this codebase's existing convention for "which rail day."**
  `/train/{uid}/{date}`, `GET /public/lines/{id}/schedule?date=`, and the
  aggregator's own `service_date`-keyed tables all already treat "which
  day" as a bare calendar date, never a datetime. `date=YYYY-MM-DD` is the
  same established shape, not a new one.
- **No effect on §4's index analysis** -- covered above.

**Breaking change, deliberately, no compatibility shim.** Same posture this
route's own predecessor docs already took
(`2026-09-08-calling-point-train-search-design.md` §6, `2026-09-07-...-sizing-design.md`
§5 Task 7 row): `GET /public/trains/search` has exactly one caller in this
repository, `TrainSearchForm.tsx`, updated in lockstep by this same plan.
Nothing about this change is actually breaking in the wire-format sense,
though -- `date` is purely additive and optional, defaulting to today
exactly as today's hardcoded behavior already does. The only real
behavioral change for an *existing* caller that omits `date` is none at
all: identical request, identical response.

**Cursor/pagination shape: unchanged, two parts, `"HH:MM:SS|train_uid"`.**
`encode_cursor`/`decode_cursor` (`trains.rs`) do not need a third component.
The brief's own concern -- "a cursor that only carries a time, not a date,
will misbehave once results can span multiple days" -- does not apply under
this wire shape: one request, and therefore one page and its cursor, are
always scoped to exactly one `date`. A "Load more" request reuses the same
`date` query parameter (via `TrainSearchForm`'s existing `searchParams()` +
`after` pattern, which already re-sends every filter including the new
`date`), so the cursor never needs to disambiguate which day it belongs to
-- the request it's attached to already says so.

**404-vs-200[] semantics: unchanged, now keyed on the caller's `date`
instead of always today's.** `schedule_destination_departures_published_for`
already takes `service_date` as a real parameter; passing through the
caller's resolved date (defaulted or explicit) instead of always `today`
is the entire change needed there.

## 6. Timezone correctness: preserving the `baa4e75`/`8250a9a` invariant

**The exact bug class `baa4e75` fixed**: `today` and `now` must never be
derived from two independent `Utc::now()` reads, because during British
Summer Time a UTC-truncated date and a London-converted time-of-day can
disagree about which calendar day it is in the 23:00-00:00 UTC window,
silently reclassifying yesterday's departed trains as upcoming. The fix was
to read `Utc::now().with_timezone(&Europe::London)` exactly once and derive
both `today` and `now` from that single value. `8250a9a` then proved the
existing test suite could not actually detect a regression of this fix (all
13 pre-existing fixtures sit 30-60 minutes from the correct `now`, far
outside the one-hour BST gap) and added a fixture seeded *inside* that gap.

**This feature's new code must preserve the same single-read invariant, and
extend it with one new comparison rather than a second clock read.** The
route already computes `london_now`/`today`/`now` from one reading
(`trains.rs`, unchanged by this feature). The only new logic is:

```rust
let service_date = match params.date {
    Some(raw) => parse_date("date", raw)?,   // NaiveDate::parse_from_str, total/unambiguous
    None => today,                            // the SAME `today` already derived above
};

let scheduled_from = if service_date == today {
    // Exactly today's existing behavior, unchanged.
    match from_time { Some(from) => std::cmp::max(now, from), None => now }
} else {
    // A named day that isn't "right now" has no now-forward boundary to
    // apply at all -- `from`/`to` alone bound the window.
    from_time.unwrap_or(chrono::NaiveTime::MIN)
};
```

The load-bearing property: `service_date == today` compares the caller's
parsed date against the **same** `today` value already produced by the
single `london_now` read that `baa4e75` introduced -- there is no second,
independent "is this today" computation anywhere, and therefore no new
opportunity for the two clocks to disagree. A caller explicitly passing
`date=<today's own date, spelled out>` must behave byte-for-byte identically
to omitting `date` entirely, because both paths resolve to the identical
comparison.

**New tests required, mirroring `8250a9a`'s own rigor:**

1. `trains_search_with_an_explicit_date_equal_to_today_behaves_identically_to_omitting_date`
   -- seeds the existing BST-gap fixture from `8250a9a`
   (`trains_search_hides_a_departure_inside_the_utc_vs_london_gap`'s own
   20-minutes-before-`now` row) and asserts the SAME exclusion when `date=`
   is passed explicitly as today's date, proving the two code paths are one
   path, not two that happen to agree today and could silently diverge
   later.
2. `trains_search_with_a_future_date_does_not_apply_the_now_forward_floor`
   -- seeds a row at `00:05` on `today+1` and asserts it IS returned
   regardless of what `now` currently is in London, proving the floor does
   not leak across the day boundary into a different `service_date`.
3. `trains_search_with_a_past_date_within_retention_returns_that_days_rows`
   -- seeds `today-1` (still inside the existing 2-day retention window)
   and confirms `date=<today-1>` returns it, proving retained backward data
   is genuinely reachable once `date` exists, not merely retained-but-unused
   (today's route has no way to read it at all).
4. `trains_search_with_a_date_outside_the_published_window_returns_404` --
   a syntactically valid but never-published date (e.g. `today+30`) gets the
   existing "no CIF publish has landed for that day" 404, proving no special
   out-of-window rejection logic needed to be invented (§2).
5. `trains_search_rejects_a_malformed_date` -- `date=not-a-date` is a `400`,
   matching every other malformed-input field's existing posture on this
   route (`trains.rs`'s own doc comment on why malformed input 400s rather
   than being silently ignored, applied here too: silently ignoring a bad
   `date` would search "today" under a filter the caller believes names a
   different day -- the same "reads as a broken search" failure mode
   already rejected for `from`/`to`/`destination`/`origin`/`station`).

All five are `#[tokio::test] #[ignore]`, DB-backed, run the same way the
existing `trains_search_*` suite already is:
`DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`.

## 7. `schedule-reference` changes, concretely

- `Config`: new field `schedule_destination_departures_forward_days: i64`,
  default `7`, doc comment mirroring
  `schedule_destination_departures_retention_days`'s own style ("1
  reproduces today's existing single-day behavior; raising this multiplies
  per-cycle publish-write cost roughly linearly -- see
  `2026-09-09-train-search-multi-day-design.md` §1 before raising it").
  Wired through the same Helm-values/env-var mechanism as the retention
  field.
- `publish_cif_derived_products`: after computing `today` (unchanged), loop
  `for offset in 0..config.schedule_destination_departures_forward_days { let service_date = today + chrono::Duration::days(offset); publish_schedule_destination_departures(client, config, &index, service_date, stanox_crs_records, internal_oauth).await; }`
  -- replacing the single current call. `schedule_line_population` and
  `schedule_network_departures` publishes are **untouched** -- this spec
  does not extend either (the future-dates sibling document explicitly
  recommends against extending `schedule_network_departures`, and
  `schedule_line_population`'s own future-dates work remains its own
  separate, not-yet-implemented scoping document this spec does not
  implement).
- `publish_schedule_destination_departures`/`schedule_destination_departures_rows`:
  rename the `today: NaiveDate` parameter to `service_date: NaiveDate` for
  clarity (it no longer always means "today"); no behavioral change to
  either function's body. `now = NaiveTime::MIN` stays exactly as today --
  unrelated to this change, still means "publish the whole rail day,
  uncapped," unaffected by which day is being published.
- Each of the N per-cycle calls keeps the existing log-and-continue error
  posture (`tracing::error!` + "will retry next cycle") independently --
  one date's POST failing does not block the others in the same cycle.

## 8. Frontend (`TrainSearchForm.tsx`)

- New optional field, **Date**, using `@mantine/dates`'s `DatePickerInput`
  (already used by `HistoryRangePicker.tsx`; `DateTimePicker` is the wrong
  component here -- this route takes a bare calendar date, not a
  datetime). No `minDate`/`maxDate` constraint, matching this codebase's
  existing "trust the server's honest 404 over a client-side guess at the
  valid range" posture (`TrackTrainForm`'s own unconstrained
  `DateTimePicker`, cited in the future-dates sibling document). Label
  "Date (optional)", description "Defaults to today. Supports a roughly
  week-ahead window, plus the last couple of days."
- New state `date: string | null` (string form, `YYYY-MM-DD`, matching
  `HistoryRangePicker`'s own `DateStringValue` convention rather than a
  `Date` object).
- `searchParams()`: `if (date) params.set('date', date);` -- gated on
  presence exactly like every other optional field on this form.
- **Correctness fix, not purely additive**: every result row today links to
  a hardcoded `const today = dayjs().format('YYYY-MM-DD')` for both
  `/train/{uid}/{today}` and `TrackThisTrainButton`'s `date` prop, because
  results were always for today. Once `date` can be any day, **both call
  sites must use the actually-searched date, not always today's** -- the
  effective date is `date || dayjs().format('YYYY-MM-DD')`, computed once
  and threaded into both. This is a real bug this feature would otherwise
  introduce (linking a tomorrow search's results back to today's train
  page), not a hypothetical -- flagged explicitly because it is easy to
  miss, since the existing code already "happens to work" by virtue of only
  ever searching today.
- 404 ("unpublished") copy: reword to name "that date" rather than always
  "Today's," e.g. "That date's scheduled timetable data isn't available --
  it may be too far ahead or behind, or it may not have been published
  yet." Matches `baa4e75`'s own precedent of keeping this copy honest about
  what the 404 actually means.
- New tests: omitting `date` behaves identically to today (regression);
  picking a date threads through to both the search request and the result
  rows' links/track buttons (the correctness fix above); clearing `date`
  after picking one returns to "today" semantics.

## 9. Explicitly out of scope

- **Any change to `schedule_line_population` or `schedule_network_departures`**,
  their own future-dates questions, or `full-coverage-consumer`'s "today +
  tomorrow" fetch. All untouched; `schedule_line_population`'s own
  future-dates scoping document remains unimplemented and separate.
- **Reconstructing a past date from today's CIF extract** (the past-dates
  sibling's "Regime B"). This feature only ever reads what was retained
  from the day it was originally published; nothing is recomputed after the
  fact for a past date.
- **Extending `schedule_destination_departures_retention_days`.** Left at
  its current default (2); a future product ask for a longer backward
  window is a one-line config change with proportional, well-understood
  storage cost, not bundled here.
- **A `destination_from`/`destination_to` question about rail-day
  crossing** (an overnight service whose arrival nominally falls on the
  next calendar day). Unaffected by and unrelated to this change --
  `destination_arrival`'s existing semantics (a plain `TIME` scoped to the
  same `service_date` as everything else on that row) are untouched.
- **Any operator filter, LDBWS/live-board multi-day search, or resident
  whole-network index.** All remain binding non-goals from every
  predecessor document in this chain.
- **A migration file.** Per §0, none is needed.
- **Rate limiting or auth on `GET /public/trains/search`.** Unchanged;
  `MAX_SEARCH_LIMIT`'s existing clamp is this route's only such control,
  untouched by this feature.

## 10. Open questions / risks

1. **`forward_days = 7` is a reasoned default, not a measured one** -- same
   honesty posture as the future-dates sibling's own 14-30-day range. No
   metric exists in this codebase for how far ahead users actually search
   (distinct from how far ahead they *pin*, which the sibling document
   already flags as unmeasured too). Worth revisiting with real usage data
   once this ships.
2. **Per-cycle publish-write cost at N=7 (~211MB total writes, once daily,
   across 7 independent ~30MB POSTs) is not independently benchmarked** --
   bounded by the same reasoning the sizing addendum already applied to
   N=1, but a real `schedule_reference_cycle_duration_seconds` comparison
   before/after this ships is cheap to obtain and should gate raising
   `forward_days` further.
3. **Whether product ever wants backward search beyond the existing 2-day
   retention.** Named as a non-goal (§9), not ruled out for later -- a
   simple, independent config change if asked for.
