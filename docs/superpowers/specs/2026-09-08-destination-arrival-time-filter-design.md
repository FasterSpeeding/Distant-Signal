# Destination-arrival time filter -- design note

Addendum to
docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md, not
a rethink of it. That document fixed the data model (one row per
departure-bearing calling point, keyed by `origin_crs` = the calling point
searched on); this note only adds one nullable column and two request
parameters on top of it.

## Problem

`GET /public/trains/search`'s existing `from`/`to` bound `scheduled`, which
is the row's own calling-point time -- i.e. the time at the searched
`station`. There is no way to ask "trains calling at READING, terminating
at WATERLOO, arriving at Waterloo between 09:00 and 09:30" -- only "calling
at Reading between 09:00 and 09:30" (a different, already-correct
question). This adds that second, independent time filter, scoped to the
`destination` filter's own arrival time.

## Column: `destination_arrival`

Added to `schedule_destination_departures` as a nullable `TIME`. Mirrors
`true_origin_crs`'s existing pattern exactly: computed once per schedule in
`departures_by_destination_crs` (`crates/schedule-query/src/resolve.rs`)
from `resolved.calling_points.last()` -- the schedule's `Terminate` calling
point -- and stamped unchanged onto every departure-bearing row that
schedule contributes. Unlike `true_origin_crs` (which reads
`booked_departure` from the FIRST calling point, an `Origin` record that
only ever carries a departure), this reads `booked_arrival` from the LAST
calling point, because `CallingPointKind::Terminate` is "arrival only, no
departure" (`crates/schedule-query/src/records.rs`). `None` when the
terminating calling point's own `booked_arrival` is absent -- a
filter-field degrade, not a dropped row, same posture as an unresolved
`true_origin_crs`.

Named `destination_arrival`, not `destination_scheduled`: the existing
`scheduled` column is deliberately a departure ("the row's own calling
point time", i.e. when the train leaves `station`), and reusing that word
for an arrival at a *different* calling point would blur the exact
distinction this whole file's row doc comment already goes out of its way
to draw between calling-point-local and schedule-level times.

## Query parameters: `destination_from` / `destination_to`

Snake_case, not camelCase, matching this codebase's other multi-word
*query-string* parameters (e.g. `crates/api/src/routes/ingest.rs`'s
`SchedulePopulationParams { line_id, service_date }`) -- camelCase in this
app is a JSON-body/response convention (`render.rs`'s `stationCrs` etc.),
not a query-string one.

**Requires `destination` to be set; a `400` if either is present without
it.** Same reasoning trains.rs's own doc comment already gives for why
`from`/`to`/`destination`/`origin`/`station` 400 on malformed input instead
of being silently ignored (lines 68-76): an arrival-time filter with
nothing named to arrive AT is ambiguous input, not a wider search. Silently
ignoring it would return MORE rows than the caller's query implies a
`destination_from`/`destination_to` pair should leave in, under a filter
the caller believes is still active -- exactly the "reads as a broken
search" failure mode that comment already rejects for the other fields.

Both are inclusive bounds, `"HH:MM"`, parsed with the same `normalize_time`
helper already used for `from`/`to`. They are a SEPARATE range from
`from`/`to` -- both pairs can be supplied at once and are evaluated
independently (`from`/`to` against `scheduled` at `station`,
`destination_from`/`destination_to` against `destination_arrival` at
`destination`). Neither implies or widens the other.

## Index

**No new index.** `destination_arrival_from`/`destination_arrival_to`
become two more `AND` predicates evaluated against rows already narrowed by
`schedule_destination_departures_calling_point_idx`'s equality/range scan
on `(service_date, origin_crs, scheduled, [train_uid])` -- the same shape
`destination_crs` already has, and the prior migration's own comment ("Do
not add a second index without a measured reason") applies unchanged: a
per-row filter layered on an already-narrow scan doesn't need its own
index, and this filter can only ever be used together with `destination`
(enforced by the `400` above), which is itself already a non-leading
predicate on the same scan. If a future measurement shows this predicate
alone is selective enough to be worth a
`(service_date, destination_crs, destination_arrival)` index independent of
`origin_crs`, that is a new, separately-measured decision -- not something
this change should pre-empt speculatively.

## Wire shape

New nullable JSON field `destinationArrival` (`"HH:MM"` or `null`) on each
`GET /public/trains/search` result row, added by
`crates/api/src/render.rs::calling_point_departure_json` next to
`destinationCrs`, following that function's existing "explicit null, never
omitted" convention.

## Frontend

`TrainSearchForm` gets a second `TextInput` pair, "Arrival from
(optional)"/"Arrival to (optional)", rendered only when `destinationCrs` is
non-empty -- matching this component's existing `results.nextCursor !==
null && (...)` conditional-render convention, and side-stepping the 400
above by construction: the fields don't exist for the caller to fill in
until a `destination` exists to scope them to. `searchParams()` gates
sending `destination_from`/`destination_to` on `destinationCrs.trim()`
being non-empty too, so a value typed while `destination` was set and then
cleared is never sent stale.
