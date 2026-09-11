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
