# MCP Schedule-Data Follow-Up: Closing the Line-Level Live-Status Gap

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline) — same posture
`2026-09-08-calling-point-train-search-design.md`,
`2026-09-08-destination-arrival-time-filter-design.md` and
`2026-09-09-trains-search-multi-day-design.md` already took for adjacent
work on this same table family.**

Triggered by a persistent project-memory note left after
`2026-09-06-shared-train-identity-design.md` shipped: *"MCP server +
schedule follow-up — spec MCP improvements tying into shared schedule
data, after shared-train-identity work lands."* This document is that
follow-up. It re-investigates the exact question
`2026-09-05-mcp-deeper-api-integration-design.md` ("the September 5
document") asked, against everything that has shipped since — most of
which the September 5 document's own recommended phases anticipated, and
most of which turns out to have *already been built*, on both sides of the
repo split, in the four days since.

## 0. Which situation this is — confirmed before any design work

The task brief's own instructions required confirming, before designing
anything, whether the MCP server lives in this repo or is a separate
service this API merely gets called by — "it changes the shape of the
deliverable substantially."

**Confirmed: separate repository.** `distant-signal-mcp` is a standalone
project (present in this environment, read-only, at
`/workspaces/distant-signal-mcp`), deployed by this repo's own Helm chart
as `charts/distant-signal/templates/railmcp-{deployment,service}.yaml`.
The deployment manifest's own header comment states plainly: *"forked from
train-mcp, own repository, own CI/tests."* Nothing under this repo's
`crates/` implements an MCP server. **Scope, per the task brief's own
branch for this situation: new/changed `api` routes only. No MCP-repo
code (tool definitions, `DsApiClient` methods, `src/tools/*.ts`) is
written by this document or its implementation.** Anything that is purely
"a new MCP tool wrapping an already-public route, zero `api` change" is
named below as a finding, not built.

## 1. Required reading consumed in full before this document was written

`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md`
(the prior scoping document this task explicitly asked to check for reuse
— read in full; see §2 for what it already settled);
`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md` (the
stated prerequisite — read in full; see §3 for confirmation it shipped);
`docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`,
`2026-09-08-destination-arrival-time-filter-design.md`,
`2026-09-09-trains-search-multi-day-design.md`,
`2026-09-08-journey-timetable-overlay-design.md` (the four specs the task
brief named as recent schedule-data infrastructure to check MCP
reachability against — see §4); `crates/api/src/routes/{trains,train,
lines,stanox_crs}.rs` (whole files, including `db_tests`);
`crates/api/src/data/{trains,queries}.rs` (whole files);
`crates/api/src/data/trust_event_backlog.rs`
(`ingest_shared_movements_batch`, the broad TRUST-ingestion writer — see
§5.2); `crates/schedule-query/src/records.rs`
(`LinePopulationEntry`/`CallingPoint`); `crates/api/migrations/` (full
directory listing, to confirm shared-train-identity's migrations landed);
`distant-signal-mcp/src/ds/client.ts` (whole file), `distant-signal-mcp/src/tools/*.ts`
(every tool), `distant-signal-mcp/TODO.md`, `distant-signal-mcp/README.md`,
and `distant-signal-mcp`'s own git log (confirms which of the September 5
document's phases already shipped there — see §2).

## 2. What the September 5 document already settled — and what shipped since

The September 5 document proposed four phases. Re-checked against both
repos' current `main`/this session's starting point, **all four have
shipped, on both sides**:

| Phase | `api` side | `distant-signal-mcp` side |
|---|---|---|
| 0 (four free tools + a resource over already-public routes) | N/A — no `api` change was ever needed | Shipped: commits `1d38405`..`a1591d6`, "Phase 0+1 of the deeper Distant-Signal api integration" |
| 1 (TRUST-corroboration tier, `GET /Train/by-uid`) | N/A | Shipped: `e20e007`, `ec40f27`, `4bcd0ce` |
| 2a (`GET /public/lines/{id}/schedule`) | Shipped: `c58d8f5` "api: add GET /public/lines/{id}/schedule" | Not yet consumed — no `getLineSchedule` method on `DsApiClient` today |
| 2b (public `stanox_crs` mirror) | Shipped: `37b4f8f` "api: add GET /public/stanox-crs" | Not yet consumed |

(`distant-signal-mcp` also independently renamed `get_tracked_train_status`
to `get_train_status` and reworked it to call the now-public, unscoped
`GET /Train/by-uid/{uid}/{date}` — commit `c0cd24f` — once the shared-
train-identity work below made that route's contract change real.)

Phases 2a/2b's own `api` routes exist, are tested (`db_tests` in
`lines.rs`/`stanox_crs.rs`), and are not yet called by any MCP tool. That
is real, but it is **MCP-repo work** (a `getLineSchedule`/`getStanoxCrs`
client method plus a tool wrapper) — out of this document's scope per §0,
and not a reason for this document to touch `api` again: the routes
already exist exactly as Decision 3/4 of the September 5 document
specified them.

**Conclusion: the entire recommended scope of the September 5 document is
either already shipped or is now purely MCP-repo work with zero `api`
gap.** This document does not repeat that investigation. It instead asks
the question the September 5 document explicitly could not: what's the
gap **now**, against everything that has landed since (most of it after
September 5).

## 3. Shared-train-identity: confirmed landed

The task brief asked this to be confirmed, not assumed. Confirmed directly
against `crates/api/migrations/`: `20260906100000_trains.sql` (the new
`trains` table), `20260906110000_train_movement_trains_id.sql`,
`20260906120000_train_movement_events_nullable_tracked_train_id.sql`,
`20260906130000_nullable_pin_columns.sql`,
`20260906140000_drop_legacy_columns.sql`,
`20260907090000_notifier_forward_queue.sql`, and
`20260907100000_rename_tracked_trains.sql` (the `tracked_trains` →
`train_subscriptions` rename, the design's own final, "late, cosmetic"
step) are all present on `main`. `GET /Train/by-uid/{train_uid}/{date}`
(`crates/api/src/routes/train.rs::get_by_uid_and_date`) is confirmed
public and unscoped — no `AuthenticatedUser` extractor, reading only
`trains`/`train_current_state` via `crate::data::trains::get_public_train_state`
— exactly as that design's §4 specified. The ground this task's own
memory note was waiting on is ready.

## 4. What's new since September 5, and whether MCP can already reach it

The four specs the task brief named (calling-point search, destination-
arrival filter, multi-day search, journey-timetable overlay) all shipped
onto routes that are, without exception, **already public and
unauthenticated**:

- `GET /public/trains/search` (`crates/api/src/routes/trains.rs`) —
  calling-point-first, multi-day (`date=`, ±the configured window),
  origin/destination filters, destination-arrival-time filter
  (`destination_from`/`destination_to`). Fully live, fully tested,
  keyset-paginated. **No MCP tool calls it today** — confirmed against
  `distant-signal-mcp/src/ds/client.ts`'s complete method list (§2's
  table; no `searchTrains`/`search_trains`-shaped method exists). This is
  a genuinely free win for a future MCP-repo task: the route needs no
  `api` change to become MCP-reachable, only a client method and a tool
  wrapper — exactly the September 5 document's own Phase-0 pattern,
  repeated. Not built here, per §0.
- Journey-timetable overlay (`JourneyStop[]`, `crates/api/src/data/journey.rs`)
  — attached automatically to `GET /Train/by-uid/{uid}/{date}`'s response
  as `journeyStops` (`routes/train.rs::attach_journey_stops_public`,
  confirmed called unconditionally in `get_by_uid_and_date`). MCP's
  existing `getPublicTrainState`/`get_train_status` already calls this
  exact route — but `distant-signal-mcp/src/ds/client.ts`'s
  `DsPublicTrainState` interface (read in full) has no `journeyStops`
  field, so the richer per-stop overlay this route already returns is
  silently dropped at the MCP boundary today. Again a **zero-`api`-change,
  MCP-repo-only** gap (add the field to the existing TS interface/zod
  shape) — named here so the pipeline that eventually does MCP-side work
  doesn't have to re-derive it, not built here.

**So: every one of the four named specs' new schedule capabilities is
already reachable via an existing, public, unauthenticated route.** This
document does not add a new route for any of them. That is a real,
load-bearing finding, not a negative result — it is exactly the outcome
the task brief's own "what good looks like" section flagged as the
likely-sufficient case ("check whether that's sufficient before assuming
new backend endpoints are needed").

## 5. The one genuine gap this document found

### 5.1 The gap: no aggregate "what's every train on this line doing right now" read

`GET /public/lines/{id}/schedule?date=` (Phase 2a) returns the full
CIF-derived stopping pattern for every service on a line, for one day —
but **only the schedule**, no live status, because `schedule_line_population`
carries none. `GET /Train/by-uid/{uid}/{date}` returns full schedule +
live status + per-stop overlay — but **only for one train at a time**,
identified by a UID the caller must already have. There is no route
between these two: a caller who wants "what's currently running on the
West Coast Main Line, and how's each one doing" has to fetch the line's
population (one call), then issue one `GET /Train/by-uid` call per UID —
for a line with a few dozen to a few hundred services in a day, that is
tens to hundreds of tool calls for a single LLM-agent question, each with
its own round trip and its own chunk of an agent's context window. This is
exactly the "genuinely missing data" / "need to combine multiple existing
routes' data" case the task brief's own scope section names as the
correct bar for a new `api` route — not a shape mismatch, a real N+1.

This is also, functionally, most of what the September 5 document's own
deferred **Phase 3b** ("persist and expose per-train state for every
full-coverage-enabled line") was reaching for — "is train X currently
tracked and what's its live status," for any scheduled service on a line,
without requiring a prior subscription. Phase 3b proposed solving it with
a new table (`full_coverage_train_state`), a new producer-side aggregation
change inside `full-coverage-consumer`, and a new ingest route — real,
substantial work the September 5 document itself flagged as needing "a
short, dedicated design pass... before implementation." §5.2 below is why
that heavier build is very likely no longer the right shape.

### 5.2 Why Phase 3b's premise is now mostly overtaken: `trains`/`train_current_state` are already populated network-wide, not gated to full-coverage lines

This is the load-bearing finding of this document, confirmed by reading
`crates/api/src/data/trust_event_backlog.rs::ingest_shared_movements_batch`
(the function `POST /private/trust-event-backlog` calls, i.e.
`trust-backlog-consumer`'s own write path — see
`2026-09-06-shared-train-identity-design.md` §3, "`trust-backlog-consumer`
becomes the primary movement-event writer" for these exact tables).

That function batches `crate::data::trains::find_or_create_trains_batch`
across **every distinct `(train_uid, service_date)` pair present in the
inbound event batch that carries a known `train_uid`** — no line-catalogue
check, no full-coverage-enabled check, no subscription check anywhere in
this call path. It then writes `train_current_state` for each one the same
way. Full-coverage-consumer's own `full_coverage_line_stats`
aggregate-only posture (which Phase 3b's own §D correctly diagnosed as
"computes real per-train derived state... then throws it all away") was
never the only path into a per-train live-status table — `trust-backlog-
consumer` already writes one, unconditionally, for the whole network, as
long as TRUST reports an Activation carrying that `train_uid`. This is a
different, independent pipeline than `full-coverage-consumer`'s, and it
already covers dramatically more of the network than "full-coverage-
enabled lines only."

**Consequence for this document's design**: a `trains`/`train_current_state`
join, batched across a line's scheduled UIDs, is very likely to already
have real live data for most trains that have actually run — not the
sparse, mostly-null picture the September 5 document worried about ("a
`get_train_status` tool answering 'no data' for most of the network until
rollout matures further" — a full-coverage-only concern that doesn't apply
to this pipeline). Coverage is bounded instead by whatever
`trust_event_backlog`/`trains` retention already is (30 days, per the
shared-train-identity design's own §5) and by whether TRUST has activated
a given service yet at query time (a train scheduled for later today
genuinely has no live state yet — an honest, expected gap, not a bug).

### 5.3 Decision: `GET /public/lines/{id}/trains?date=` — a batched read, no new table, no new writer

**Chosen.** A new public, unauthenticated route, joining data two already-
existing tables already carry, with no migration and no producer-side
change:

1. Read `schedule_line_population` for `(line_id, date)` exactly as
   `get_line_schedule` already does (`queries::get_schedule_line_population`,
   unchanged, reused as-is).
2. Pluck `uid` out of each population entry (`serde_json::Value::get("uid")`
   — a single known-field extraction, not a full deserialize into
   `schedule_query::LinePopulationEntry`, matching this route's own sibling
   `get_line_schedule`'s explicit reasoning for staying undeserialized:
   "`api` has no dependency on the `schedule-query` crate... a raw
   pass-through survives schema growth... with zero changes needed here."
   This document keeps that same posture rather than reopening it).
3. One new batched query, `trains::get_public_train_states_for_line`,
   structurally identical to the existing `get_public_train_state` (same
   `trains`/`train_current_state`/`stations` join, same `PublicTrainState`
   struct, reused unchanged) but keyed by `WHERE tr.train_uid = ANY($1) AND
   tr.service_date = $2` instead of a single UID — one round trip for the
   whole line, not one per train. `journey_stops` is deliberately **not**
   attached per row (that would reintroduce the exact N+1 this route
   exists to remove, and per-stop overlays for potentially hundreds of
   trains at once has no clear consumer need yet); a caller that wants a
   specific train's full stop-by-stop overlay still calls `GET
   /Train/by-uid/{uid}/{date}` for that one train, same as today.
4. Merge: response is a JSON array, one entry per population entry, each
   carrying that entry's own `uid` and unprocessed `callingPoints` (same
   raw pass-through `get_line_schedule` already gives, unchanged shape) plus
   a `liveStatus` field — `null` if no `trains` row exists yet for that
   `(uid, date)`, otherwise an object with the same fields
   `PublicTrainState` already exposes minus `callingPoints`/`journeyStops`
   (`trainsId`, `trainId`, `originCrs`, `originName`, `destinationCrs`,
   `destinationName`, `scheduledDeparture`, `status`,
   `lastReportedLocation`, `lastEventType`, `delayMinutes`,
   `nextCallingPoint`, `etaNext`, `etaSource`).

**No write side effect.** Unlike `get_by_uid_and_date`'s read-triggered
`find_or_create_train` upsert (needed there because a single by-uid lookup
is the *only* place a freshly-searched, never-tracked train's identity
could be minted), this route does not call `find_or_create_train`/
`find_or_create_trains_batch` for population entries with no existing
`trains` row. Two reasons: (a) per §5.2, real coverage is already expected
to be good via `trust-backlog-consumer`'s own broad ingestion — this route
doesn't need to manufacture rows to be useful; (b) a GET that fans out to
insert up to a whole line's population worth of rows on every call is a
real, avoidable write-amplification/DoS-shaped risk for a public,
unauthenticated, unmetered route that this design doesn't need to take on
to deliver its value. A `trains` row for a not-yet-activated service will
appear naturally, on its own, once TRUST or a real lookup creates one —
this route just reads whatever already exists at request time.

**Auth/access group**: none, matching every sibling reference-data route
in this app (`/public/lines`, `/public/lines/{id}/schedule`,
`/public/trains/search`, `/public/stanox-crs`) and per the September 5
document's own Decision 7 reasoning (DS-hosted, non-metered data needs no
access-group gate at all).

**No pagination/cap.** `get_line_schedule`, this route's direct sibling
over the exact same source table, already returns a line's entire
population uncapped — this route rides the same precedent and introduces
no new unbounded-response risk beyond what already ships today. Line
population sizes were not measured in this session (see Open questions).

**Naming**: `GET /public/lines/{id}/trains?date=`, deliberately the
plural, no-uid-segment form. This nests cleanly if a future document ever
wants `GET /public/lines/{id}/trains/{uid}` for something not already
covered by `GET /Train/by-uid/{uid}/{date}` — a collection route and an
eventual item sub-route under it are not a naming conflict, standard REST
nesting. Not reusing the September 5 document's own Phase-3b placeholder
path verbatim, since this route's data source and semantics differ from
what that document sketched (§5.2).

## 6. What this unlocks (for a future, separate MCP-repo pipeline)

A `get_line_trains(lineId, date?)`-shaped tool becomes buildable with zero
further `api` work: "what's running on line X today, and how's each
service doing" in one call, materially cheaper (in both round trips and
LLM context) than looping `get_train_status` once per UID. Combined with
§4's two already-public, not-yet-wired capabilities (`search_trains` over
`GET /public/trains/search`, and `journeyStops` becoming visible on
`get_train_status`'s existing response), this leaves the MCP-repo side
with three well-scoped, zero-`api`-dependency follow-up tools — named here
so a future MCP-focused pipeline doesn't have to re-derive them, not
committed to or scheduled by this document.

## 7. Non-goals (explicitly out of scope)

- **Any `distant-signal-mcp` code.** Per §0 — separate repository, not
  touched by this document or its implementation.
- **Re-shipping Phase 0/1/2a/2b of the September 5 document.** Confirmed
  already shipped, §2 — not repeated.
- **Phase 3a (`train_movement_events.raw_body`).** Still open, still
  cross-crate (`trust-schema`/`trust-consumer`), still not "schedule
  data" in the sense this follow-up's own memory note named — left
  exactly where the September 5 document left it.
- **Building the full Phase 3b (`full_coverage_train_state` table,
  `full-coverage-consumer` producer change).** Superseded in large part by
  §5.2's finding; not resurrected here. If a future document finds real
  gaps §5.3's read-only route doesn't cover (e.g., needing history rather
  than a live snapshot, or needing coverage for services TRUST hasn't
  activated yet), that is its own follow-up, not this one.
- **Per-stop (`journeyStops`) data in the new line-trains route.**
  Deliberately excluded from the batched response — see §5.3, Decision
  point 3.
- **Any change to `GET /Train/by-uid/{uid}/{date}`, `GET
  /public/lines/{id}/schedule`, or `GET /public/trains/search`.** All
  three are reused exactly as they exist today.
- **Metrics/per-station stats work.** Named in project memory as a
  separate, not-yet-started thread; untouched here.
- **Ireland/Northern Ireland rail data.** Same posture as the September 5
  document — design-only, not implemented, not in scope.

## 8. Open questions/risks

1. **Line population size, unmeasured.** Same gap the September 5
   document flagged and left open for `stanox_crs`'s row count — this
   document did not measure a typical `schedule_line_population.population`
   array's length. `get_line_schedule` already returns this uncapped
   today, so this route introduces no *new* risk, but if a real-world
   count turns out to be surprisingly large, both routes would want the
   same fix at the same time, not this route alone.
2. **`liveStatus` coverage still depends on TRUST having activated a
   service by request time.** A train scheduled later today, or one
   `trust_event_backlog`'s 30-day retention window has already aged
   `trains` data out for, will show `liveStatus: null` — correct and
   intentional (§5.3's "no write side effect" decision), but worth
   flagging plainly in the route's own doc comment so a future MCP tool's
   prompt/description sets that expectation rather than promising
   complete network coverage.
3. **§5.2's "network-wide, not full-coverage-gated" finding was derived
   by reading `ingest_shared_movements_batch`'s code, not by querying real
   production `trains`/`train_current_state` row counts.** The
   *mechanism* is confirmed broad; how *much* of a given line's population
   actually has a matching `trains` row at any given moment was not
   measured this session. If a future check finds coverage is thinner
   than this document expects, that's new information for whether Phase
   3b-proper is still worth building — not a defect in this document's own
   reasoning about the mechanism.
