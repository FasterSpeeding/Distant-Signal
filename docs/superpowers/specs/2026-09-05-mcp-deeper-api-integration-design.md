# MCP Deeper `api` Integration — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`
("the integration doc"), `docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md`
("the access-groups doc"), and
`docs/superpowers/specs/2026-09-02-mcp-server-first-party-hosting-design.md`
("the hosting doc") — all three read in full this session, along with
`docs/superpowers/plans/2026-09-01-train-mcp-integration.md` ("the
plan"). No implementation plan is included; that is separate, later
work. `distant-signal-mcp` itself was read directly at
`/workspaces/distant-signal-mcp` (read-only, a separate repository — see
the hosting doc's Decision 1/2, reused unchanged here, not
re-litigated).

## Relationship to prior docs — what's already settled, what's new here

The three prior docs, plus roughly twenty commits on `main` in both
repos since they were written, have already fully shipped:

- **The DS-line-catalogue-annotated `plan_journey`** (integration doc
  Decision 3): `distant-signal-mcp`'s `src/ds/lineMatch.ts`/
  `lineCatalogue.ts`/`annotateLeg.ts` call `GET /public/lines`,
  `GET /public/lines/{id}/definition`, and `GET /Line/{id}/Status?detail=true`
  exactly as designed.
- **`resolve_station`'s DS-backed shim** (integration doc Decision 2):
  live, calling `GET /public/stations?q=`.
- **The adapter's own OAuth 2.1 authorization server** (integration doc
  Decision 4d) and **the `mcp-users`/`mcp-live-boards` access-group
  gating** (the access-groups doc, all of it): both fully implemented on
  *both* sides — `crates/api/src/auth/oidc.rs`'s `AccessGroupClaims`,
  `crates/api/migrations/20260902160000_user_access_groups.sql`'s
  `users.groups` column, `GET /public/auth/session`'s `groups` field
  (`crates/api/src/routes/auth.rs:259-292`), and
  `distant-signal-mcp/src/oauth/accessGroups.ts`'s `isEntitledToServer`/
  `requiresLiveBoardsGroup` gates, enforced in `src/app.ts`'s `/mcp`
  handler. Discord auth is fully gone (confirmed: no `DISCORD_*`
  reference anywhere in `distant-signal-mcp`'s current code, and its
  README/TODO were already corrected — commit `3c0600d`, "Drop the
  train-mcp fork framing and document the real OAuth/access-group
  auth" — so the hosting doc's §4 staleness finding no longer holds;
  noted so this document doesn't re-flag it as live drift).
- **The repo-split/fork-lineage calls** (the hosting doc, both
  decisions): unchanged, not revisited here. `distant-signal-mcp` stays
  a separate repository; this document proposes only new `api` routes
  and MCP-side tool/client work, consistent with that boundary.

**What is genuinely new, and what this document is actually about.**
The task that produced this document is broader than any of the three
prior docs: `api` has grown substantially since 2026-09-01 (real-time
TRUST movement-event processing, per-line daily/half-hourly stats and
their full-coverage siblings, per-station full-coverage samples,
CIF-SCHEDULE-derived line population and per-station departures) —
none of which the prior MCP docs designed against, because none of it
existed yet when they were written. `distant-signal-mcp`'s own `api`
client (`src/ds/client.ts`) has exactly four methods and has not grown
since Decision 3 shipped. This document closes that gap: what `api` can
now do that MCP doesn't use, what `api`/Postgres still can't do at all,
and what's worth building first.

**One correction to the integration doc's own Decision 4/3b.6, found this
session.** The integration doc's revised Decision 4 and 3b.6 stated the
TRUST-corroboration tier (`GET /Train/by-uid/{uid}/{date}`, using the
caller's held DS session) would become "a general per-request mechanism,
not opportunistic," once the adapter held a real session. **The
plumbing for this is live but the mechanism itself was never built.**
`src/app.ts:158-167` extracts `dsSessionCookieValue` from the verified
bearer token specifically for this purpose, and its own comment says so
explicitly: *"DsApiClient itself has no method that sends a Cookie
header yet (every DS call it makes today is deliberately anonymous,
Decision 4) — using this for annotateLeg.ts's TRUST-corroboration tier
is a follow-up to the sibling train-mcp-integration plan's own Task 6,
not executed by this change."* `DsApiClient` (`src/ds/client.ts:79-140`)
has exactly four methods — `searchStations`, `getLines`,
`getLineDefinition`, `getLineStatus` — all four routed through the same
`getJson` helper that "*Deliberately [sends] no Authorization/session
header*" (`client.ts:126`). No fifth, session-carrying method exists.
This is not a new finding this document invents; it is the most
directly actionable, already-fully-specified gap in the whole
investigation, and it anchors Decision 2 below.

## Goal

Investigate the actual, current gap between what `distant-signal-mcp`
can query today and what `crates/api` can now provide, and produce a
concrete, phased proposal — new/changed `api` routes, new Postgres
persistence where the data genuinely doesn't exist yet, microservice
changes where raw detail is being discarded before it ever reaches
`api`, and the new MCP tools each change unlocks.

## Current relevant state (verified 2026-09-05)

### A. What `distant-signal-mcp` calls today — the complete list

`DsApiClient` (`distant-signal-mcp/src/ds/client.ts:79-140`) calls
exactly these four `api` endpoints, all anonymously:

| Endpoint | MCP caller | Tool(s) |
|---|---|---|
| `GET /public/stations?q=` | `src/stations/resolveViaDs.ts` | `resolve_station` |
| `GET /public/lines` | `src/ds/lineCatalogue.ts` | `plan_journey` (line matching) |
| `GET /public/lines/{id}/definition` | `src/ds/lineCatalogue.ts` | `plan_journey` (line matching) |
| `GET /Line/{id}/Status?detail=true` | `src/ds/annotateLeg.ts` | `plan_journey` (live-status overlay) |

Six MCP tools exist total (`distant-signal-mcp/src/tools/*.ts`,
registered in `src/server.ts`): `resolve_station`, `get_departures`,
`get_arrivals`, `get_service_detail` (all three Darwin/LDBWS directly,
no DS call), `find_services` (own CIF/SQLite store, no DS call), and
`plan_journey` (own CIF/RAPTOR planner plus the DS annotation above).
No MCP **resources** exist, only tools. `distant-signal-mcp/TODO.md`
tracks LDBWS-side gaps only (the Disruptions Experience API, RDM key
rotation, a Network Rail CORPUS account) — nothing about deeper `api`
integration, train tracking, TRUST events, or schedule data appears in
either `TODO.md` or `README.md`. The only place in the MCP repo that
names this exact gap is the `app.ts:158-167` comment quoted above.

### B. `api` routes that exist today and have no MCP caller at all

All confirmed by reading `crates/api/src/routes/mod.rs`,
`crates/api/src/main.rs:58-62`, and each route file directly.
`public_router()` (`routes/mod.rs:26-52`) is nested at `/public` with no
auth layer of any kind; `line_status.rs`'s five `/Line/...`/
`/StopPoint/...` routes are merged unprefixed onto the top-level router
(`main.rs:59`), also unauthenticated. **Every route in this table is
already public and unauthenticated; none of it needs an access-group
change to be reachable.**

| Route | Auth | Returns | Cited |
|---|---|---|---|
| `GET /Line/{id}/Stats/{from}/to/{to}` | none | Per-line **daily** rollup of `SampleStats` (`total`/`delayed`/`cancelled`/`skipped`/`avgDelayMinutes`) over a date range | `line_status.rs:414-422`, `queries::daily_stats_for_range` |
| `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` | none | Same, half-hourly buckets | `line_status.rs:458-467`, `queries::half_hourly_stats_for_range` |
| `GET /Line/{id}/Stats/Coverage/...` (daily + half-hourly) | none | Full-coverage sibling of the above — **always returns `[]` today**, no writer populates it yet | `line_status.rs:65-79`, migration `20260903200000`/`20260903200001` |
| `GET /public/stations/{crs}/departures` | none | Raw live departure board rows for `crs`, straight from `station_samples` — same underlying LDBWS data `poller-ldbws` already polls | `departures.rs:35-59` |
| `GET /public/stations/{crs}/schedule-departures` | none | Today's CIF-SCHEDULE-derived departures for `crs` — covers the **whole national network** (any station in `stanox_crs`), not just DS's curated `sample_stations` subset | `departures.rs:71-88` |
| `GET /public/stations/{crs}/sample-stats` | none | Per-operator delay/cancellation stats for `crs`, blending LDBWS-sample and full-coverage sources | `station_stats.rs:31-92` |
| `GET /public/freshness` | none | Last-fetched timestamp for 5 upstream sources (stations, tocs, incidents, tfl, schedule feed) | `freshness.rs:26-58` |
| `GET /public/history-retention` | none | Static config echo of retained history days | `history_retention.rs:23-40` |
| `POST /Train/track`, `GET /Train/mine`, `GET/DELETE /Train/{trackingId}`, `GET /Train/by-uid/{uid}/{date}`, ticket routes | `AuthenticatedUser` (real DS session), ownership-scoped | Individual TRUST-tracked-train state, tickets, delay-repay estimates | `train.rs:26-98` |

`GET /public/stations/{crs}/departures`'s station coverage is bounded
by the curated `sample_stations` list in `lines/*.toml`
(`routes/samples.rs:16-29`'s `/sample-stations` route lists exactly
this set) — the same "ongoing RDM-quota/architecture cost" limitation
the integration doc's Decision 5 already declined to widen. It is
**not** a drop-in replacement for MCP's direct LDBWS client, which can
query any station via train-mcp's own product key. `schedule-departures`
has no such limit (national coverage) but carries only scheduled times
and `destinationCrs`, no live estimate/delay — a materially different,
complementary capability, not a substitute.

### C. Data `api`/Postgres already persists but exposes to no one outside its own producer/consumer pair

Two tables hold real, non-trivial data behind `private_router()`
(`routes/mod.rs:55-77`, gated by `require_internal_oauth` — a
service-to-service credential, not a user session), reachable only by
the specific microservice(s) whose credential is scoped to that route
in `build_internal_oauth_routes` (`crates/api/src/app.rs:55-220`):

- **`schedule_line_population`** (migration `20260904090000`): one row
  per `(line_id, service_date)`, `population` an opaque JSONB
  `Vec<schedule_query::LinePopulationEntry>` — and `LinePopulationEntry`
  (`crates/schedule-query/src/records.rs:157-160`) is genuinely rich:
  `{uid, calling_points: [{tiploc, kind, booked_arrival, booked_departure,
  is_half_minute_arrival, is_half_minute_departure}]}` — i.e. **the full
  CIF-derived stopping pattern for every service on a line, for one rail
  day, already sits in Postgres.** `POST` is `schedule-reference`'s
  writer credential (`internal_oauth_group_schedule_reference`); `GET`
  is scoped to `internal_oauth_group_full_coverage`
  (`crates/api/src/app.rs:187-196`) — `full-coverage-consumer`'s own
  reload, not a second, different reader. **No public or user-facing
  route exists over this table at all.** This is exactly the data a
  "what's the full timetable for line X today" MCP tool would need, and
  it's already computed and stored — the gap is a missing route, not
  missing persistence.
- **`stanox_crs`** (migration `20260901150000`): the live
  STANOX→CRS→TIPLOC→station-name reference table, one row per STANOX,
  refreshed wholesale by `schedule-reference` from the CIF feed's own
  `TI` records. `GET /private/stanox-crs` returns the **entire current
  table** (`ingest.rs:304-311`, "returns the full current table, not a
  freshness timestamp"), gated to `svc-trust-consumer`/
  `svc-full-coverage-consumer` (`app.rs:160-167`). This is precisely the
  live, richer alternative to the bundled `stations.json` the
  integration doc's Decision 2 used for `resolve_station`'s `tiploc`
  join, and precisely the CRS↔TIPLOC mapping the integration doc's own
  Decision 2 declined to add to DS's public `Suggestion` type "for this
  pass," noting it only as "a possible future follow-up." That follow-up
  is now cheap: the data source already exists, it just isn't public.

### D. Data that *looks* persisted but the writer never actually populates it

- **`train_movement_events.raw_body`** (migration `20260828120000`) is
  documented as *"full message body, verbatim, for anything this schema
  doesn't model explicitly"* — but `trust-consumer` never sends it.
  `crates/trust-consumer/src/process.rs:104-107`'s own module doc states
  plainly: *"`raw_body` is always `serde_json::json!({})`.
  `schema::parse_envelope` deserializes each envelope's body into a
  typed struct and drops the original `serde_json::Value`, so there is
  no raw body left to forward by the time a message reaches this
  module."* Confirmed at every construction site that builds a
  `common::TrainMovementEventMessage`: `process.rs:521`, `:567`, `:628`
  all hard-code `raw_body: serde_json::json!({})`. So
  `train_movement_events` **is** a real, per-event, append-only,
  queryable-by-train log (one row per TRUST message matched to a
  tracked train, `dedup_key`-deduplicated, `tracked_train_id`-indexed —
  a genuinely good foundation for "show me this specific train's full
  movement history") — but every row's structured columns
  (`event_type`, `loc_crs`, `variation_status`, etc.) are the *only*
  detail available; the schema's own "everything else, verbatim" escape
  hatch is dead code today, because the raw envelope is discarded one
  layer up, in `crates/trust-schema/src/schema.rs`'s `parse_envelope`
  (confirmed: that function returns only the parsed
  `TrustMessage`/typed-struct value, never the original
  `serde_json::Value` it parsed from — `schema.rs:22` shows the
  intermediate envelope's own `body: serde_json::Value` field exists
  transiently during parsing but nothing carries it forward).
- **Full-coverage-consumer computes real per-train derived state for
  *every* scheduled service on a full-coverage-enabled line, then
  throws it all away except one aggregated row per line.**
  `crates/full-coverage-consumer/src/correlate.rs`'s `CorrelationState`
  holds `derived: HashMap<(String, String), DerivedState>` — one
  `DerivedState` (`status`, `last_reported_location`, `delay_minutes`,
  `next_calling_point`, the *exact same shape* `train_current_state`
  already persists for user-pinned trains) **per `(line_id, uid)` pair**,
  computed by reusing `trust_schema::journey`'s derivation logic
  directly against the line's full CIF `Population`
  (`correlate.rs:1-27`) — i.e. this consumer already knows the live
  status of every train on a full-coverage line, not just ones a user
  happens to have pinned. But the only thing it ever writes back to
  `api` is `POST /private/full-coverage-stats`
  (`crates/full-coverage-consumer/src/stats.rs`, `ingest.rs:385-393`), a
  single aggregated `SampleStats` row per line
  (`full_coverage_line_stats`, migration `20260904100000`). The
  per-`(line_id, uid)` `DerivedState` map never leaves the consumer's
  own memory. This is the single biggest untapped data source this
  investigation found: **individual-train tracking, network-wide, for
  every full-coverage-enabled line, already computed and simply
  discarded before persistence** — not gated behind auth, not
  unbuilt, just never written down.

### E. Auth/access-group implications for the routes above

None of Section B's routes need any access-group change — they're
already public. Section C's two tables are gated by
`internal_oauth_*` **service** credentials
(`crates/api/src/app.rs:55-220`), a mechanism the sibling hosting doc's
own §6e citation of `2026-09-01-internal-service-accounts-design.md`'s
Decision 4 already argues is the *wrong* fit for a human-facing,
per-user caller like `distant-signal-mcp` ("service identities are
static and deploy-time... human-facing access is inherently dynamic").
Threading a `distant-signal-mcp`-specific internal-oauth service
credential through the adapter to reach `/private/schedule-line-population`
or `/private/stanox-crs` would be exactly the conflation that design
already argued against — MCP calls are made *as* a specific DS user, not
as a static backend service. The correct shape (Decisions 3/4 below) is
new `/public/*` routes, matching the posture every other reference-data
route in this app already takes (`/public/lines`, `/public/stations`),
not a new internal-oauth grant. `GET /Train/by-uid/{uid}/{date}`
(Decision 2) needs no *new* group either — it already requires
`AuthenticatedUser`, which the adapter already holds via
`dsSessionCookieValue`; the only open question (Decision 6) is which of
the MCP-side groups (`mcp-users` vs. `mcp-live-boards`) a *new* tool
built on it should require.

### F. `distant-signal-mcp`'s own TODO.md / README.md

Read in full. Neither mentions train tracking, TRUST, schedule data, or
any of this document's subject matter — see §A. Nothing here duplicates
already-tracked work.

## Decisions

### 1. Phase 0 — wire existing, already-public `api` routes into new MCP tools: zero `api` changes

**Chosen: add MCP tools/resources for the five Section B routes DS
already serves publicly and unauthenticated, with no `api`-side work at
all.** These are genuinely free wins relative to everything else in
this document — the routes exist, are stable (some have HTTP-layer
tests already, e.g. `departures.rs`'s and `station_stats.rs`'s
`db_tests` modules), and need only a client method + tool wrapper on
the MCP side:

- `get_line_delay_trend(lineId, from, to, granularity: 'daily' |
  'half-hourly')` → `GET /Line/{id}/Stats/{from}/to/{to}` or its
  `HalfHourly` sibling. Answers exactly the "historical delay trend for
  line Y" capability named in this task's brief.
- `get_national_schedule_departures(crs)` → `GET
  /public/stations/{crs}/schedule-departures`. Unlike the existing
  Darwin-backed board tools, this reaches **any** station in
  `stanox_crs` (the whole GB network), not just DS's curated
  `sample_stations` — a genuinely new "any station" capability the
  integration doc's Decision 5 explicitly declined to build via
  `poller-ldbws` widening, but which already exists via the CIF-derived
  path for free.
- `get_station_operator_stats(crs)` → `GET
  /public/stations/{crs}/sample-stats`. Per-operator delay/cancellation
  breakdown DS's own frontend already renders; no MCP tool surfaces it
  today.
- `get_data_freshness()` → `GET /public/freshness` +
  `GET /public/history-retention`. Low-value on its own, but cheap, and
  useful as a "how current is what you're about to tell me" caveat for
  an LLM consumer — arguably better as an MCP **resource** than a tool,
  since it's not something a user asks for directly.

**Not choosing** to route `get_departures`/`get_arrivals` through `GET
/public/stations/{crs}/departures` instead of Darwin/LDBWS directly —
considered, rejected for this document (see Open questions/risks #1):
it would trade "no metered external call, no `mcp-live-boards` gate
needed" for "silently narrower station coverage" (curated
`sample_stations` only, vs. Darwin's arbitrary-CRS reach), a real
behavior change to two existing tools this document isn't the right
place to decide unilaterally.

### 2. Phase 1 — finish the already-designed TRUST-corroboration tier: MCP-side only, zero `api` changes

**Chosen: implement the missing fifth `DsApiClient` method** (a
session-cookie-carrying `GET /Train/by-uid/{train_uid}/{date}` call)
**and wire it into `annotateLeg.ts`**, exactly as the integration doc's
revised Decision 3b.6 already specified and `app.ts:158-167`'s own
comment already flags as outstanding. This needs no `crates/api` change
at all — the route, the `AuthenticatedUser` gate, and the
`TrackedTrainState` shape all already exist and already match what the
design calls for (`train.rs:471-500`). It is the cheapest, most
directly load-bearing item in this document, because it's the one place
prior design work already fully specified the shape and simply wasn't
executed. Recommending it as Phase 1 (immediately after Phase 0) rather
than folding it into "already done" is deliberate: it's real,
outstanding MCP-repo work, not something this document can mark
complete by fiat.

This also unlocks a standalone tool, not just an annotation:
`get_tracked_train_status(uid, date)` for any `(uid, date)` the calling
DS user has personally pinned via `POST /Train/track` — independent of
`plan_journey`. See Decision 6 for which access group should gate it.

### 3. Phase 2a — new public route over `schedule_line_population`: closes a real, already-persisted data gap

**Chosen: add `GET /public/lines/{id}/schedule?date=` (naming
placeholder), unauthenticated, reading `schedule_line_population`
directly** (`queries::get_schedule_line_population`, already exists,
`crates/api/src/data/queries.rs:760-796`) — no new persistence, no
microservice change, no new writer. This is purely a new read path over
data `schedule-reference` already publishes daily. **Rejected
alternative: reuse the existing internal-oauth-gated
`GET /private/schedule-line-population` by granting `distant-signal-mcp`
a service credential.** Rejected per §E above — wrong mechanism for a
per-user-acting caller, and it would also require the adapter to hold a
*second* kind of credential (a static service token) alongside its
per-user DS session, a real, avoidable complexity increase for no
benefit over a plain public route matching every other reference-data
route in this app.

Response shape: pass the existing `LinePopulationEntry[]` JSON straight
through (`api` never deserializes it into a Rust type today — this
route shouldn't start; keep the "opaque blob, `Value`-typed" posture
`schedule_network_departures`'s own route already established,
`data/queries.rs:836-855`/`routes/departures.rs:71-88`). Unlike
`/private/schedule-line-population`'s query-param `line_id`/
`service_date` pair, the public route takes `line_id` from the path
(matching `/public/lines/{id}/definition`'s own convention) and
`service_date` as an optional query param defaulting to today
(matching `schedule-departures`'s own "always now unless told
otherwise" pattern, `departures.rs:71-75`).

**Unlocks**: `get_line_timetable(lineId, date?)` — a genuine "what's the
full timetable for line X today" MCP tool, including full stopping
patterns (tiploc-level calling points with booked times), not just
population counts.

### 4. Phase 2b — surface `stanox_crs` publicly, closing the integration doc's own declined follow-up

**Chosen: widen `GET /public/stations` (or add a new
`GET /public/stations/all`, TBD at implementation time) to include
`tiploc`, sourced from `stanox_crs` rather than the static
`stations.json` an operator's `crates/schedule-reference` process
already refreshes daily.** This is precisely the option the integration
doc's Decision 2 named and declined "for this pass" — *"Widen DS's
`Suggestion`/`search_stations` to add `tiploc` and a match-confidence
field... Rejected for this pass: it is a change to DS's own Rust code...
more importantly, it couples the shim's correctness to DS's schema
evolving in a specific way DS has no independent reason to want"* — but
the second half of that reasoning ("DS has no independent reason to
want it") is weaker now that `stanox_crs` exists as a live,
already-computed table for an unrelated reason (trust-consumer/
full-coverage-consumer STANOX resolution), not a schema change invented
solely to serve MCP.

**Two real shapes, weighed:**
- **(a) Add `tiploc: Option<String>` directly to the existing
  `Suggestion`/`GET /public/stations?q=` response.** Simple, but
  `search_stations`'s query (`data/reference.rs:23-35`) searches the
  `stations` table (RDM reference data), not `stanox_crs` — joining
  across two independently-refreshed tables (`stations.crs` vs.
  `stanox_crs.crs`) inside the existing hot-path type-ahead query adds
  real complexity to a route DS's own frontend also depends on for
  its own unrelated purpose (station search-as-you-type), for a field
  only MCP needs.
- **(b) A separate `GET /public/stations/{crs}/tiploc` or a bulk
  `GET /public/stanox-crs`-style public mirror**, read-through to
  `stanox_crs` directly, entirely independent of `search_stations`.
  **Chosen.** Keeps DS's existing type-ahead route untouched (no risk
  to the frontend's own working path) and matches this document's
  general posture of adding narrow, purpose-built public routes rather
  than widening shared ones. A bulk form is more useful to MCP than a
  per-CRS one, since `resolve_station`'s shim currently does its
  `tiploc` join against a bundled, potentially-stale `stations.json`
  snapshot for every result — a live bulk table lets it do the same
  join against current data instead, still locally (no per-request
  network cost beyond what the periodic cache refresh already costs
  DS's other MCP-facing catalogue calls, mirroring the integration
  doc's own Decision 3d TTL-cache pattern).

**Unlocks**: replaces `resolve_station`'s bundled-`stations.json`
`tiploc` join with a live DS source; also enables a `resolve_tiploc`-
shaped tool/resource for `distant-signal-mcp`'s own timetable code,
which already needs TIPLOC↔CRS resolution (`store.crsForTiploc`,
integration doc's Current relevant state) and currently sources it
from its own bundled data independent of DS entirely.

### 5. Phase 3a — make `train_movement_events.raw_body` actually carry data

**Chosen: thread the original TRUST envelope's `serde_json::Value`
through `trust_schema::schema::parse_envelope` alongside the typed
`TrustMessage`, and have `trust-consumer`'s `process_message`
(`process.rs`) attach it to `TrainMovementEventMessage.raw_body` instead
of the hardcoded `serde_json::json!({})`.** No new table, no migration —
the column has existed since `20260828120000` and has simply never been
populated. This is real, non-trivial code work (not a config flag):
`parse_envelope`'s current signature returns only the parsed message;
it needs to additionally carry (or a sibling function needs to
separately expose) the pre-parse `Value` for each surviving envelope,
and every one of `process.rs`'s four `TrainMovementEventMessage`
construction sites (`:521`, `:567`, `:628`, plus the
`passthrough_event` helper) needs to stop hardcoding the empty object.

**Reasoning for doing this at all**: today, `train_movement_events`
already answers "give me this tracked train's event history" using only
its typed columns (`event_type`, `loc_crs`, `variation_status`,
`planned_timestamp`/`actual_timestamp`) — a real, useful, per-event
history an MCP tool like `get_train_movement_history(trackingId)` could
already build on **without this change**. This phase is about
completeness and forward-compatibility (TRUST fields this schema
doesn't model explicitly — platform numbers, correction indicators,
etc. — becoming visible without a further migration), not about
unblocking that tool's basic version. It is real work for a real but
secondary gain, which is why it's placed in Phase 3, not Phase 1.

**Unlocks**: a genuinely fuller `get_train_movement_history(trackingId)`
tool once built (see Decision 2's tracked-train tooling), where
`raw_body` becomes a legitimate "here's everything TRUST said, in case
the summarized fields don't answer your question" fallback for an LLM
consumer, rather than a permanently-empty documented-but-dead field.

### 6. Phase 3b — persist and expose per-train state for every full-coverage-enabled line, not just user-pinned trains

**The single biggest, highest-value, and most involved change this
document proposes.** Per §D, `full-coverage-consumer` already computes
a `DerivedState` (status/location/delay/next-calling-point) for every
`(line_id, uid)` pair on any full-coverage-enabled line, then discards
all but a one-row-per-line aggregate. This is the concrete answer to
"is train X currently tracked and what's its live status," for *any*
scheduled service on a rolled-out line — not gated behind a user having
separately pinned it.

**New persistence**: a new table, `full_coverage_train_state` — one row
per `(line_id, uid, service_date)`, column-for-column mirroring
`train_current_state`'s existing shape (`status`,
`last_reported_location`, `last_event_type`, `delay_minutes`,
`next_calling_point`, `updated_at`), following the same "live snapshot,
wholesale-replaced per cycle, not an append log" posture
`station_full_coverage_samples` and `full_coverage_line_stats` already
established for this same producer chain (their own migration
comments' explicit reasoning, `20260904070000`/`20260904100000`) — a
genuinely new table, not a repurposing of `train_current_state`
(that table's `tracked_train_id` FK ties every row to a user-owned
pin; this data has no owner at all, the same distinction `SampleStats`-
by-population vs. `SampleStats`-by-sample-station draws in the
full-coverage-metrics-transition design's own Decision 4, cited
verbatim in that migration's header).

**New `api` routes**:
- `POST /private/full-coverage-train-state` — new ingest route, gated
  to `internal_oauth_group_full_coverage` (the same credential
  `full-coverage-consumer` already holds for `/full-coverage-stats` and
  `/station-full-coverage-samples` — no new service identity needed).
- `GET /public/lines/{id}/trains/{uid}?date=` (naming placeholder) —
  **public, unauthenticated**, not gated behind any user session at
  all. This is a deliberate departure from `/Train/by-uid/{uid}/{date}`'s
  own `AuthenticatedUser` gate: that route is ownership-scoped because
  it's answering on behalf of a specific user's pin (a private
  artifact — the pin, its tickets, its delay-repay claim); this route
  answers a plain physical fact about a scheduled public train service,
  with no owner to scope to, the same posture `/Line/{id}/Status`
  already takes for line-level facts. Making it public also sidesteps
  the entire access-group question for this specific data — it doesn't
  need `mcp-users` or a new group at all, any more than `/public/lines`
  does.

**Unlocks**: `get_train_status(lineId, uid, date?)` — the network-wide
generalization of Decision 2's pin-scoped tool, answering "what's this
specific service doing right now" for any train on a full-coverage
line, without requiring the calling DS user to have tracked anything
first. This is the concrete, buildable-now answer to the task's own
example ("is train X currently tracked and what's its live status") for
the subset of the network full-coverage rollout actually covers — see
Open questions/risks #2 for the coverage caveat.

### 7. Access-group placement for the new per-user-relevant routes

**Chosen: `GET /Train/by-uid/{uid}/{date}`-backed tools (Decision 2) sit
under `mcp-users` only, not `mcp-live-boards`.** The access-groups
doc's own Decision 3 scoped `mcp-live-boards` specifically to "the four
tools that hit a real, external, metered resource" (Darwin/LDBWS,
operator product keys) — `/Train/by-uid` is DS-hosted, not
Darwin-metered (its `blend_darwin_eta` overlay is best-effort and
already folded into `TrackedTrainState`'s existing shape, not a
separate metered call the *tool* makes). All of this document's other
new tools (Decisions 1, 3, 4, 6) are equally DS-hosted, non-metered, and
should sit under `mcp-users` alone for the same reason. **No new access
group is proposed anywhere in this document** — every new route is
either fully public (Decisions 1, 3, 4, 6's read route) or already
covered by the existing `mcp-users` gate (Decision 2). This is a
deliberate minimality call: the access-groups doc's own Decision 3
explicitly reserved finer splitting for "real usage data on which tools
actually need separate gating in practice" (its Open question 5) — none
of this document's findings constitute that evidence.

## New MCP tools/resources this unlocks — summary

| Tool/resource | Backing route | New `api` work? | Phase |
|---|---|---|---|
| `get_line_delay_trend` | `GET /Line/{id}/Stats(/HalfHourly)/{from}/to/{to}` | None | 0 |
| `get_national_schedule_departures` | `GET /public/stations/{crs}/schedule-departures` | None | 0 |
| `get_station_operator_stats` | `GET /public/stations/{crs}/sample-stats` | None | 0 |
| `data-freshness` resource | `GET /public/freshness` + `/history-retention` | None | 0 |
| `get_tracked_train_status(uid, date)` + `plan_journey`'s TRUST tier | `GET /Train/by-uid/{uid}/{date}` | None (MCP-side only) | 1 |
| `get_line_timetable(lineId, date?)` | new `GET /public/lines/{id}/schedule` | New public route only | 2a |
| `resolve_station` tiploc via live data; `resolve_tiploc` | new public `stanox_crs` mirror | New public route only | 2b |
| `get_train_movement_history(trackingId)` (fuller version) | existing `train_movement_events` + fixed `raw_body` | Microservice fix, no schema change | 3a |
| `get_train_status(lineId, uid, date?)` (network-wide) | new `full_coverage_train_state` table + 2 new routes | New table + new ingest/read routes + consumer change | 3b |

## Recommended phased scope

**Phase 0 + 1 first, as one batch.** Both are zero-`api`-change,
MCP-repo-only work, already fully specified (Phase 1 by the integration
doc itself, over a year — in-story — of prior design work already
paid for). This is the cheap, high-value slice the task's own framing
asks for: real new capability, no new persistence, no new microservice
risk, no new access-group design. Recommend doing this before anything
else in this document, independent of whether the rest is ever built.

**Phase 2 (2a + 2b) second.** Both are narrow, additive, new-route-only
`api` changes over data that already exists and is already correctly
computed — no microservice changes, no new tables, no new failure
modes beyond "a new route can 500/404 like any other." Real but bounded
`api`-side work (new route handlers, new response DTOs, new route
tests following this crate's own `db_tests` convention throughout every
file read this session).

**Phase 3 (3a + 3b) last, and 3b specifically should be scoped and
reviewed as its own follow-up design, not built directly from this
document's sketch.** 3a is a real but self-contained fix confined to
`trust-schema`/`trust-consumer`. 3b is the biggest lift by a wide
margin: a new table, a new producer-side aggregation change inside
`full-coverage-consumer` (which today is architected around "reduce to
one row per line," per its own `stats.rs`), a new ingest route, a new
public read route, and — unlike everything else in this document — a
real new write-volume question (one row per `(line_id, uid,
service_date)` per full-coverage-enabled line, not one row per line,
which is a materially larger table depending on how many lines full-
coverage rollout eventually covers). It is also the one item in this
document whose value is bounded by full-coverage rollout's own current
scope (see Open questions/risks #2) — building it ahead of that rollout
maturing risks a table sized for a rollout that may not be there yet.
**Recommend a short, dedicated design pass for 3b alone** before
implementation, scoped like `2026-09-04-option-b-live-consumer-design.md`
was for its own piece of this same producer chain — this document's
sketch establishes that it's worth doing and roughly what shape it
takes, not final field-level detail.

## Non-goals (explicitly out of scope)

- **Any change to `distant-signal-mcp`'s adapter authentication or
  access-group *mechanism*.** Both are fully implemented and match the
  access-groups doc exactly (Current relevant state, above) — this
  document only asks which existing gate (if any) new tools sit behind
  (Decision 7), never how the gates themselves work.
- **Re-litigating the hosting doc's repo-split or fork-lineage
  decisions.** Both stand unchanged. Nothing here proposes moving
  `distant-signal-mcp`'s source into this repo.
- **Rewriting or migrating `find_services`/`plan_journey`'s own
  CIF/RAPTOR/CSA engine.** Unchanged from the integration doc's own
  Decision 1/5 — still entirely inside `distant-signal-mcp`, untouched.
- **Widening `poller-ldbws`'s curated `sample_stations` set**, or
  routing `get_departures`/`get_arrivals` through DS instead of Darwin
  directly. Named as a real, weighed option in Decision 1 and
  deliberately not chosen here — see Open questions/risks #1.
- **Ireland/Northern Ireland rail data of any kind.** Confirmed this
  session: `docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md`
  and its NI sibling are design specs only, not implemented — no
  migration, no code, exists for either jurisdiction anywhere in this
  repo as of this session (`crates/api/migrations/`'s full listing,
  read this session, has no Ireland/NI-related entry; the `stations`
  table, `crates/api/migrations/20260706004003_reference_data.sql:10-18`,
  has no network/country/jurisdiction column of any kind). The task
  brief's own "what stations exist on the Ireland network (once that
  lands)" is explicitly hypothetical/future-tense; this document does
  not design for it.
- **The NRE/Darwin-data "presentation" licensing question.** Already
  flagged, unresolved, and explicitly not re-litigated by the
  integration doc's own Licensing note. New MCP tools built on this
  document's new routes carry the same open question as every existing
  DS-sourced tool output; nothing here narrows or widens it.
- **Precise field-level API design for Phase 2/3's new routes**
  (exact JSON shapes beyond "reuses the existing opaque-JSONB/typed-row
  posture already established for sibling routes"). Left to
  implementation-plan depth, consistent with how the integration doc
  itself treated its own Decision 3's caching sketch.
- **A full design for Phase 3b specifically** — deliberately deferred
  to its own follow-up document, per the phased-scope recommendation
  above.

## Open questions/risks

1. **Whether `get_departures`/`get_arrivals` should ever be re-pointed
   at `GET /public/stations/{crs}/departures` instead of Darwin/LDBWS
   directly.** Named and declined in Decision 1: it would trade a
   metered external call (and the `mcp-live-boards` gate) for silently
   narrower station coverage (curated `sample_stations` only). This is
   a real product tradeoff a future document should make deliberately,
   not something this investigation resolves.
2. **Decision 6 (3b)'s value is bounded by how many lines full-coverage
   rollout actually covers today**, which this document did not
   measure (`full_coverage_enabled` is a per-line catalogue flag,
   `common::LineDefinition::full_coverage_enabled`, and a global
   `full_coverage_enabled_default` override exists in config — neither
   was counted this session). A `get_train_status` tool answering "no
   data" for most of the network until rollout matures further is
   honest but may look sparse; worth measuring before committing to the
   dedicated Phase 3b design pass this document recommends.
3. **Phase 3a's `raw_body` fix requires touching `trust_schema::schema`'s
   `parse_envelope` signature**, a function this document did not fully
   trace every caller of — `trust-consumer` is confirmed to be its only
   current consumer (via `process.rs`'s imports), but a signature change
   should be re-verified against `crates/trust-schema`'s own test suite
   before being treated as a purely-additive change.
4. **Naming for the two new public routes in Decisions 3 and 4/6** (`GET
   /public/lines/{id}/schedule`, `GET /public/lines/{id}/trains/{uid}`,
   the `stanox_crs` public mirror's exact path) are this document's own
   placeholders, not researched against a URL-naming convention beyond
   "follows this app's existing `/public/lines/{id}/...` shape" — an
   implementation plan should confirm no existing route collides before
   committing to exact paths.
5. **Whether a bulk public `stanox_crs` mirror (Decision 4) has any
   real cost DS should weigh** — it's reference data, not sensitive, but
   this document did not measure the table's current row count/response
   size the way the integration doc flagged (and left unmeasured) DS's
   line-catalogue size in its own Open questions/risks #1. Likely small
   (one row per GB TIPLOC), not verified this session.
6. **`train_movement_events`/`tracked_trains` have no retention or
   pruning job of any kind, confirmed directly in the code's own
   comment**: `crates/api/src/data/train_tracking.rs:24-38` states
   plainly *"No retention or pruning job exists anywhere in this
   codebase for `tracked_trains` (grepped for `DELETE FROM
   tracked_trains`/`prune`/`expire`/`retention` -- only `ON DELETE
   CASCADE` foreign keys and unrelated matches turned up), so this table
   grows without bound"* — `list_tracked_trains_for_user` already caps
   its own response at `MINE_LIST_LIMIT = 100` (`:38`) for exactly this
   reason, and the child `train_movement_events` table shares the same
   unbounded-growth profile (cascade-deleted only if the owning pin is
   itself deleted). This is a real, pre-existing gap this document
   didn't create, but it's directly load-bearing for Phase 3a's
   `get_train_movement_history(trackingId)` tool: that route's own query
   should take the same defensive posture (an explicit `LIMIT`,
   most-recent-first) rather than assuming an unbounded per-event log is
   already a solved problem elsewhere in this codebase — it isn't.
