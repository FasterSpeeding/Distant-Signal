# "Stops at" single-station calling-point filter -- design note

Replaces the single-valued "Terminating at" filter
(`destination`/`destination_crs`) on `GET /public/trains/search`, addendum
to docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md
and docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.

**Scope note.** An earlier version of this filter accepted zero or more
repeated `stops_at` entries and matched ALL-of-N (relational division): a
schedule had to call at EVERY named station somewhere on its route. That
shipped, and was then deliberately scoped back down to exactly one station
-- this document now describes the single-station shape only. The
underlying semantic change from "Terminating at" (below) is NOT being
reverted; only the multi-station mechanism is gone.

## Problem

"Terminating at" answered "is this CRS the schedule's TRUE final calling
point" -- a narrower question than most riders actually have ("does this
train call at Reading", regardless of whether Reading is where the
schedule actually ends). This replaces it with "Stops at (optional)": a
single station the schedule must call at somewhere along its route.

## Query parameter: `stops_at` (single-valued)

`?stops_at=RDG`. Plain `axum::extract::Query` (`serde_urlencoded`) is
sufficient for this: it's an ordinary optional `String` field, no repeated
key handling needed. (The multi-station version of this filter needed a
new `axum_extra`/`serde_html_form`-backed extractor to group repeated keys
into a `Vec<String>`; that dependency was removed along with the
multi-station mechanism.) The value is validated with the same
`normalize_crs` helper `station`/`origin` already use (3-letter CRS,
uppercased); an invalid value 400s, naming the field, matching this
route's existing "malformed input 400s" posture.

**A plain membership test, not relational division.** Matched via a
correlated `EXISTS` against `origin_crs` in
`queries::search_schedule_calling_point_departures` -- the same equality
check `station_crs` already does, just for a second, independent CRS.
Matches against the same `origin_crs` column `station_crs` matches
against, so (consistent with that field's own pre-existing behavior, not a
new gap) a `stops_at` value naming a schedule's TRUE terminating calling
point never matches: that calling point has no `booked_departure` and so
never gets its own row in `schedule_destination_departures` at all.
**Both halves of that paragraph were later revised** -- a call at the
station `station` itself named now has to come LATER in the journey, and
the true terminus now DOES match. See the "Loop services" addendum at the
foot of this document.

**This is a genuine behavior change from `destination`, not just a
rename.** `destination=X` meant "X IS this schedule's true final stop";
`stops_at=X` means "this schedule calls at X, anywhere on its route" -- a
real capability loss for a caller who specifically wanted "true
destination equals X" with no "or intermediate stop" leak. No replacement
filter for that narrower question ships alongside this one; flagged here
rather than silently dropped.

## Query parameters: `arrival_from` / `arrival_to`

Kept, not removed. Apply only when `stops_at` is set at all (a `400`
otherwise, same "ambiguous input" reasoning `destination_from`/
`destination_to` originally used for "`destination` must be set"): with no
`stops_at` value there is no calling point to scope "arrival" to. (The
multi-station version of this filter required `stops_at` to name EXACTLY
ONE station, since 2+ entries also had no single well-defined arrival to
bind to; with `stops_at` back down to a single value, that "exactly one"
check collapsed to a plain "is it set" check -- the same simplification
this whole document reflects.)

**Not the same value as `destination_arrival`.** That column is the
schedule's TRUE destination's own arrival, computed once per schedule and
copied onto every row. `stops_at`'s value is frequently an INTERMEDIATE
calling point, which has no row of its own in the destination-arrival
sense. This needed a genuinely new, per-calling-point column:
`calling_point_arrival` (nullable `TIME`, migration
`20260910100000_schedule_destination_departures_calling_point_arrival.sql`),
populated in `schedule_query::resolve::departures_by_destination_crs` from
each calling point's own `booked_arrival` (`None` for the schedule's true
origin, `Some` for a genuine intermediate stop -- mirrors
`true_origin_crs`/`destination_arrival`'s existing nullable-column
precedent, but varies per entry instead of being copied schedule-wide).
This column and its population are unchanged by the multi-station-to-
single-station scope-down: they were always keyed on ONE calling point
per row and needed no adjustment.

Applied via a correlated `EXISTS` against `stop.origin_crs = $stops_at`,
not against the outer query's own row -- the `station_crs` row and the
`stops_at`-arrival row are frequently two different calling points of the
same schedule.

## Frontend: single-station autocomplete

Structurally the same single-station `Autocomplete` the pre-existing
"Terminating at" field used (composed with the same `useSuggestions`/
`searchStations` pair every other station field in this form uses), just
renamed "Stops at (optional)" and carrying the new any-calling-point
semantic instead of true-destination-only. (An intermediate version of
this field was a chip-style multi-entry `TagsInput`; that shipped and was
then reverted back to a plain single-value `Autocomplete` along with the
backend's scope-down.) The existing "Earliest/Latest arrival" `TextInput`
pair is gated on `stopsAt.trim() !== ''`, mirroring the reveal condition
the original "Terminating at" field used for the same pair, and is cleared
from the request (though not from local state) when Stops at is emptied.

## Wire shape

No change: `destinationCrs`/`destinationArrival`/
`destinationArrivalDayOffset` remain on every row exactly as before --
still the schedule's true destination and its arrival, still purely
informational display fields, unrelated to which filter parameters were
supplied.

## Addendum (2026-09-17): loop services, and what "stops at" excludes

### The bug

"Departing from" = `WAT`, "Stops at" = `WAT` returned every train out of
Waterloo. Not "most of them", not "the wrong ones as well" -- the exact
same result set, in the same order, as supplying no `stops_at` at all
(confirmed against a real Postgres, not inferred from the SQL). The
membership test above is a correlated `EXISTS` over the schedule's own
rows, and the row being tested is a member of its own calling-point list,
so the predicate was true by construction for every candidate.

That is not a harmless no-op, because naming one station in both fields is
exactly how a rider asks for a LOOP: "leaves Waterloo, comes back to
Waterloo". South Western Railway's Kingston Loop (train L82877,
2026-09-14: Waterloo 07:27, round via Clapham Junction, Kingston and
Richmond, terminating back at Waterloo 08:46) is the shape -- the working whose
live timeline was fixed earlier the same day by
`journey::assign_events_to_stops`, and the same root mistake in a second
place: **a CRS code does not identify one position in a journey.**

### The fix, in two halves

**Superseded (2026-09-22).** The second paragraph of item 1 below -- "a
call at any OTHER station is untouched ... including before `station`" --
describes behavior this document's own foot no longer has. See "Addendum
(2026-09-22): the same-station rule stops being same-station-only" at the
end of this document for what changed, and why the paragraph is kept
rather than deleted.

1. **A call at the station `station` itself named counts only if it comes
   LATER in the journey**, compared as `(day_offset, scheduled)`. A call at
   any OTHER station is untouched and still counts wherever in the route it
   falls, including before `station` -- `stops_at` remains "calls at X
   somewhere on its route" and deliberately does not quietly become "and
   you can get there from where you searched". So every search naming two
   different stations returns exactly what it returned before this half of
   the change: `stop.origin_crs = $stops_at` and `main.origin_crs =
   $station` are then different values and the rule cannot fire.

   **LATER, not merely OTHER**, and the distinction is not academic. The
   first draft of this fix excluded only the searched row itself, which is
   enough for a service that terminates back where it started but wrong for
   one that passes back through and carries on (`WAT -> ... -> WAT -> ... ->
   SOU`). Such a working offers two Waterloo departures and only the FIRST
   comes back; "some other row at this CRS exists" returns both, so half the
   results would be trains a rider boards expecting a return that never
   happens. An independent review caught this before the change landed.

   Ordering by `(day_offset, scheduled)` rather than `scheduled` alone is
   required, not belt-and-braces: an overnight working crosses midnight and
   its later calls carry a SMALLER clock time (which is the whole reason
   `day_offset` exists -- see its own migration). Two same-station calls
   can never TIE under that comparison, and the guarantee is structural
   rather than a claim about timetabling: the table's primary key covers
   `(service_date, destination_crs, scheduled, train_uid, origin_crs)`,
   `destination_crs` is constant per `(service_date, train_uid)`, and the
   ingest is `ON CONFLICT DO NOTHING`, so two same-station calls sharing a
   `scheduled` cannot both be rows at all. (The key does not carry
   `day_offset`, so a revisit at the same clock minute exactly a day later
   loses a row at ingest -- pre-existing, vanishingly rare, and named here
   because it is the actual boundary of the invariant this rule leans on.)
   Nothing comparable holds ACROSS stations, where two calling points
   genuinely can share a booked minute -- the practical second reason to
   scope the rule to same-station calls, the semantic one above being the
   first.

   This rule is also the only thing in that query that reads `day_offset`
   at all. The `from`/`to` bounds, the `ORDER BY` and the keyset cursor
   remain plain wall-clock comparisons on `scheduled`; widening those is a
   separate question about how a midnight-crossing rail day should
   paginate, deliberately not answered here.

   The rule is per-row, so a station called at three times still matches
   from each departure that has a later call at that station -- two out of
   three, in that example -- rather than all-or-nothing per train.

2. **The schedule's TRUE terminating calling point now matches**,
   via `main.destination_crs`, reversing the gap the section above
   flagged. Two reasons, one principled and one forcing. The principled
   one: that gap contradicts this document's own problem statement
   ("`does this train call at Reading`, regardless of whether Reading is
   where the schedule actually ends") and was a data-model artifact
   (an arrival-only calling point has no `booked_departure` and so no row)
   rather than a decision. The forcing one: a loop's return call IS its
   terminus, so without this branch the corrected `EXISTS` in (1) finds
   nothing and the Kingston Loop stays unfindable. The terminus needs no
   ordering test of its own -- it is downstream of every departure-bearing
   row by construction. This widens ordinary searches too -- `stops_at`
   naming any schedule's true destination now matches it -- which is
   intended, and which restores (as a by-product, not as a replacement
   filter) the "true destination equals X" answer the
   `destination`-to-`stops_at` change had removed, still mixed in with
   intermediate-stop matches.

`arrival_from`/`arrival_to` mirror the same two branches, same same-station
ordering rule and all, so that the bounds are asked about the very calling
point `stops_at` matched on: `calling_point_arrival` on the `EXISTS` branch
as before, and the terminus's own `destination_arrival` on the new branch
-- where it is not a schedule-level stand-in but literally the arrival at
the calling point the caller named. Day offsets are consulted by neither
ARRIVAL comparison, matching the original bound's own wall-clock behavior.

**A NULL arrival never satisfies a bound, on either branch.** Both columns
are genuinely nullable in published data, so setting an arrival bound drops
schedules whose named calling point has no booked arrival at all. That was
already true of `calling_point_arrival`; the terminus branch matches it
deliberately rather than inventing a "NULL passes" rule for one branch
only, and there is a test pinning it. It is the one place the arrival pair
narrows what `stops_at` matched rather than merely filtering it.

### What did NOT change

The wire shape, the single-valued parameter, the `normalize_crs`
validation, the `400` for an arrival bound with no `stops_at`, and the
`origin`/`station`/time filters. On the frontend only the "Stops at"
field's `description` changed, to say that the named station is another
one the train calls at, its destination included, and that naming the same
station as "Departing from" finds loop services that come back to it.

**This description of the frontend copy is itself superseded by the
2026-09-22 addendum below** -- the field description changed again, and no
longer needs to call out the loop case specially.

## Addendum (2026-09-22): the same-station rule stops being same-station-only

### The reversal

**Product decision, not a bug fix.** Item 1 of the 2026-09-17 addendum
above deliberately scoped the "must come later in the journey" ordering
rule to the case where `stops_at` repeats `station` -- a call at any OTHER
named station was left as a plain, unordered "calls at X somewhere on its
route", including before `station`. That was a considered choice at the
time (see the paragraph's own reasoning), not an oversight. It has now
been reversed by explicit product decision: **"stops at X" should always
mean X comes later in the journey than the search origin station, for
every station, not just the loop case.**

The reasoning: a rider typing "stops at Reading" into this search wants
trains that genuinely continue on to Reading from where they searched --
that they can actually board at the search origin and ride to Reading.
"This train calls at Reading at some point on its overall route, possibly
hours before it ever reached the station you searched" answers a
different, less useful question, and silently returning those trains mixed
in with the ones a rider can actually use was the gap: `stops_at` never
promised "and you can get there from here" in the old wording specifically
so that it would NOT make that promise, but making it was always what a
searcher actually wanted. There is no longer a semantic reason for the
same-station (loop) case and the different-station case to disagree with
each other on this question, so they no longer do.

### The fix

One line: the `EXISTS` subquery's ordering test,

```sql
AND (stop.origin_crs <> main.origin_crs
     OR (stop.day_offset, stop.scheduled)
        > (main.day_offset, main.scheduled))
```

loses its `stop.origin_crs <> main.origin_crs OR` escape hatch, on BOTH
branches that carry it (the plain membership `EXISTS` and its
`arrival_from`/`arrival_to` mirror), leaving

```sql
AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
```

unconditional. The ordering test already worked correctly for the
same-station case (that is the whole 2026-09-17 fix); this simply extends
it to every case, same comparison and same `(day_offset, scheduled)`
reasoning (overnight midnight-crossing services, tie-impossibility from the
table's primary key) as before -- none of that reasoning was scoped to
same-station calls in the first place, it just was not being applied
elsewhere.

The terminus branch (`main.destination_crs = $stops_at`) is UNCHANGED: it
never carried an ordering test (the terminus is downstream of every
departure-bearing row of the same schedule by construction, per item 2 of
the 2026-09-17 addendum), and still does not need one.

### What this actually changes for a caller

For `stops_at` naming the SAME station as `station` (the loop case):
nothing -- that case was already fully ordered by the 2026-09-17 fix.

For `stops_at` naming a DIFFERENT station: a schedule that calls at that
station BEFORE ever reaching `station` no longer matches. Worked example
from this fix's own test fixture (`crates/api/src/data/queries.rs`'s
`loop_fixture_rows`): `station=CLJ&stops_at=WAT` used to match both
`L82877` (Waterloo 07:27, Clapham Junction 07:40) and `P00001` (Waterloo
07:30, Clapham Junction 07:55), because both call WAT somewhere on their
route. Both calls are BEFORE Clapham Junction, so neither train is
reachable at WAT from a rider standing on the Clapham Junction platform;
the search now correctly returns neither.

### What did NOT change (again)

Same-valued CRS as `station` still finds loop services exactly as before.
The terminus-matching branch, the arrival-bound pair's NULL-never-satisfies
rule, the single-valued parameter shape, and the `400` for an arrival bound
with no `stops_at` are all untouched. Only the ordering test's scope
widened.

### Tests

`crates/api/src/data/queries.rs`: `loop_fixture_rows`'s Clapham Junction
calling point (present on `L82877` and `P00001`, true origin of neither)
now doubles as the fixture for the reversed direction --
`search_calling_point_stops_at_excludes_a_different_station_reached_before_the_searched_one`
pins `station=CLJ&stops_at=WAT` returning empty. The old "calls at WAT
before CLJ still matches" half of
`search_calling_point_stops_at_still_matches_a_genuine_intermediate_stop`
was removed from that test (it pinned the now-reversed behavior) and that
test is now scoped to its still-true forward-direction case only.

`crates/api/src/routes/trains.rs`:
`trains_search_station_and_stops_at_naming_one_station_finds_loop_services`'s
`station=ZRB&stops_at=KNG` assertion is updated -- `T53003`'s SECOND ZRB
departure used to match (its only KNG call is BEFORE it) and is now
excluded, dropping the expected match count from four rows to three. A new
dedicated test,
`trains_search_stops_at_a_different_station_excludes_a_call_before_the_search_origin`,
isolates the reversal on its own minimal fixture, independent of the loop
fixture's other cases.

### Frontend copy

`frontend/components/TrainSearchForm.tsx`'s "Stops at" field `description`
no longer needs to carve out the loop case as a special example, now that
the rule is uniform: it reads "A station this train reaches later in its
journey than Station, its destination included." (was: "Another station
this train calls at, its destination included. Enter the same station as
Departing from to find loop services that come back to it.") The
surrounding doc comments in that file and in `frontend/app/trains/page.tsx`
that explained (and, in one case, warned future editors NOT to write) the
old "a stop earlier than the searched station matches too" behavior are
updated to describe the new, uniform rule instead.
