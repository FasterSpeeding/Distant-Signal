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
   `day_offset` exists -- see its own migration). Conversely, ordering two
   calls at ONE station by their booked departures is safe precisely
   because they cannot be at the same minute, which is another reason to
   scope the rule to same-station calls and no further: two calls at
   DIFFERENT stations can and do share a booked minute, so the same
   comparison would be unreliable there.

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
