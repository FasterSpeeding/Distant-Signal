# "Stops at" multi-station calling-point filter -- design note

Replaces the single-valued "Terminating at" filter
(`destination`/`destination_crs`) on `GET /public/trains/search`, addendum
to docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md
and docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.

## Problem

"Terminating at" answered "is this CRS the schedule's TRUE final calling
point" -- a narrower question than most riders actually have ("does this
train call at Reading AND Oxford", regardless of which one, if either, is
where the schedule actually ends). This replaces it with "Stops at
(optional)": zero or more stations, ALL of which the schedule must call at
somewhere along its route.

## Query parameter: `stops_at` (repeated)

`?stops_at=RDG&stops_at=OXF`. Plain `axum::extract::Query`
(`serde_urlencoded`) turned out NOT to support this: it cannot
deserialize a repeated query key into a `Vec<String>` field at all, even
for two occurrences of the same key -- a real, verified `serde_urlencoded`
limitation, not a configuration gap. This route was switched to
`axum_extra::extract::Query` (`serde_html_form`), added as a new
dependency (`axum-extra`, `query` feature only), which groups repeated
keys correctly. Each entry is validated with the same `normalize_crs`
helper `station`/`origin` already use (3-letter CRS, uppercased) and then
deduped (a raw, not distinct, element count would otherwise let a
duplicate entry silently zero out every match in the ALL-of-N query
below); any invalid entry 400s, naming the field, matching this route's
existing "malformed input 400s" posture.

**ALL-of-N, not ANY-of-N.** Matched via `train_uid IN (SELECT train_uid ...
WHERE origin_crs = ANY($stops_at) GROUP BY train_uid HAVING
COUNT(DISTINCT origin_crs) = <n>)` in
`queries::search_schedule_calling_point_departures` -- relational division,
generalizing `station_crs`'s own single-CRS equality check to N required
calling points. Matches against the same `origin_crs` column `station_crs`
matches against, so (consistent with that field's own pre-existing
behavior, not a new gap) a `stops_at` entry naming a schedule's TRUE
terminating calling point never matches: that calling point has no
`booked_departure` and so never gets its own row in
`schedule_destination_departures` at all.

**This is a genuine behavior change from `destination`, not just a
rename.** `destination=X` meant "X IS this schedule's true final stop";
`stops_at=X` (one entry) means "this schedule calls at X, anywhere on its
route" -- a real capability loss for a caller who specifically wanted
"true destination equals X" with no "or intermediate stop" leak. No
replacement filter for that narrower question ships alongside this one;
flagged here rather than silently dropped.

## Query parameters: `arrival_from` / `arrival_to`

Kept, not removed, but re-scoped: apply only when `stops_at` names EXACTLY
ONE station (a `400` otherwise, same "ambiguous input" reasoning
`destination_from`/`destination_to` originally used for "`destination`
must be set"). With 2+ `stops_at` entries there is no single well-defined
"arrival" any more -- which of several stops? -- so the fields simply
don't apply.

**Not the same value as `destination_arrival`.** That column is the
schedule's TRUE destination's own arrival, computed once per schedule and
copied onto every row. `stops_at`'s single entry is frequently an
INTERMEDIATE calling point, which has no row of its own in the
destination-arrival sense. This needed a genuinely new, per-calling-point
column: `calling_point_arrival` (nullable `TIME`, migration
`20260910100000_schedule_destination_departures_calling_point_arrival.sql`),
populated in `schedule_query::resolve::departures_by_destination_crs` from
each calling point's own `booked_arrival` (`None` for the schedule's true
origin, `Some` for a genuine intermediate stop -- mirrors
`true_origin_crs`/`destination_arrival`'s existing nullable-column
precedent, but varies per entry instead of being copied schedule-wide).

Applied via a correlated `EXISTS` against `stop.origin_crs =
(stops_at::text[])[1]`, not against the outer query's own row -- the
`station_crs` row and the `stops_at`-arrival row are frequently two
different calling points of the same schedule.

## Frontend: chip-style multi-entry autocomplete

Mantine's `TagsInput` (flat named export, not a compound `.SubPart` API --
no server/client-boundary risk) composed with the existing
`useSuggestions`/`searchStations` pair `Autocomplete` already uses for
"Station"/"Departing from". Each attempted addition is checked against the
live suggestion list for the in-progress search text; only a match becomes
a chip; free text that doesn't resolve to a real station is dropped. The
existing "Earliest/Latest arrival" `TextInput` pair is now gated on
`stopsAt.length === 1` instead of `destinationCrs` being set, and is
cleared when a second station is added or the list is emptied.

## Wire shape

No change: `destinationCrs`/`destinationArrival`/
`destinationArrivalDayOffset` remain on every row exactly as before --
still the schedule's true destination and its arrival, still purely
informational display fields, unrelated to which filter parameters were
supplied.
