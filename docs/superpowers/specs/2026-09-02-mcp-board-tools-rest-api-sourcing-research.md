# Should `distant-signal-mcp`'s live-board tools source data from `crates/api` instead of LDBWS directly? — Research

**Status: research only, not an approved design. No code in either
repository was modified as part of this pass.** This document re-examines,
with fresh evidence from both repositories as they stand on 2026-09-02, a
question the `train-mcp` integration work already looked at once
(`docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`,
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`
Decision 5). Both prior documents concluded "leave the board tools calling
LDBWS directly, no DS work now." This pass asks whether anything that has
changed since then — `distant-signal-mcp` actually existing as a live
sibling repo now (rather than a `train-mcp.zip` archive), the poller-ldbws
`numRows` resilience fix, and the concurrent internal-service-OAuth2
work — changes that answer. **It does not.** The reasoning below is new
(fresh citations against the current state of both repos), but the
conclusion is the same, and this document says so plainly rather than
re-deriving suspense.

## Scope and method

Read directly, this session:

- `/workspaces/distant-signal-mcp/src/ldbws/client.ts`,
  `src/tools/boards.ts`, `src/tools/service-detail.ts`, `src/ds/client.ts`,
  `src/config.ts`, `src/stations/resolveViaDs.ts` — every place the MCP
  server touches live-departure, arrival, or per-service data, and its own
  `DsApiClient` (the thing that already talks to `crates/api`).
- `crates/api/src/routes/{mod,ingest,train,line_status,lines,reference}.rs`,
  `crates/api/src/data/{queries,train_tracking}.rs`, `crates/common/src/lib.rs`
  (`StationSample`/`StationDeparture` shapes) — DS's actual current route
  surface and what it stores.
- `crates/poller-ldbws/src/{main,config}.rs` — the sampler that is DS's
  only source of LDBWS-derived data, including the `numRows` retry-fallback
  fix (commit `821abaf`, merged `c69bae7`, 2026-09-02).
- `docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md`
  and `docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md`
  — the two pieces of concurrent/recent work this task asked to be factored
  in as context, not modified.

No LDBWS/RDM API calls were made. No MCP tool was invoked. This is a static
read of both codebases.

## Part 1 — What `distant-signal-mcp` calls today, and with what credentials

The MCP server has **two independent upstream HTTP clients**, calling two
different services, for two different purposes. This split is real and
deliberate, not an oversight the codebase is unaware of — both clients
carry comments explaining it.

### 1a. `LdbwsClient` (`src/ldbws/client.ts`) — direct Darwin/LDBWS, metered operator keys

`get_departures`, `get_arrivals` (`src/tools/boards.ts:513-568`) and
`get_service_detail` (`src/tools/service-detail.ts:125-166`) all go through
`LdbwsClient`, which calls Rail Data Marketplace's `GetDepBoardWithDetails`,
`GetArrBoardWithDetails`, and `GetServiceDetails` operations directly
(`src/ldbws/client.ts:50-63`), authenticated with an `x-apikey` header
(`src/ldbws/client.ts:118-120`). Three **separate** RDM products/keys are
configured — `LDBWS_DEPARTURES_URL`/`_KEY`, `LDBWS_ARRIVALS_URL`/`_KEY`,
`LDBWS_SERVICE_URL`/`_KEY` (`src/config.ts:93-116`, `195-199`) — because "the
keys are not interchangeable: a departures key returns 401 against the
arrivals product" (`src/config.ts:107-109`). Responses are cached client-side
for 30s (`src/ldbws/client.ts:47`, `cacheTtlMs` default).

This is the operator's *own* metered LDBWS credential, separate from and
unrelated to whatever `poller-ldbws` uses server-side in the main repo — a
fact `src/config.ts:88-90`'s comment on `mcpLiveBoardsGroup` states
explicitly ("four tools hitting metered Darwin/LDBWS keys directly"), and
which the access-groups design doc treats as the entire reason a separate
`mcp-live-boards` gate exists at all
(`docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md:365-416`,
"required, in addition [to `mcp-users`]... outside DS's own infrastructure
onto the operator's metered LDBWS").

### 1b. `DsApiClient` (`src/ds/client.ts`) — anonymous calls to DS's own public API

Station search (`resolveStationViaDs`, `src/stations/resolveViaDs.ts`),
line catalogue/definition lookups, and line-status annotation
(`getLines`, `getLineDefinition`, `getLineStatus`) go through a second
client that calls `crates/api`'s `/public/stations`, `/public/lines`,
`/public/lines/{id}/definition`, and `/Line/{id}/Status?detail=true`
(`src/ds/client.ts:96-121`). This client is explicitly, permanently
unauthenticated by design — "this client never sends an `Authorization`
header or a session cookie" (`src/ds/client.ts:67-77`), confirmed against
`crates/api/src/routes/mod.rs:23-42`'s `public_router()`, which mounts
exactly those routes with no auth layer.

**Neither client currently authenticates to `crates/api` with any
credential at all.** `DsApiClient` calls only what's already public;
`LdbwsClient` never calls `crates/api` in the first place. The
internal-service-OAuth2 refactor this task named as concurrent work would,
once landed, give `DsApiClient` (or a new client alongside it) the ability
to call `crates/api`'s `/private/*` routes the way `poller-ldbws` and
friends do (`crates/common/src/oauth_client.rs`,
`crates/api/src/auth.rs`'s `require_internal_oauth`) — but that only
matters if there is a route worth calling with it. Part 2 checks whether
one exists for board data.

## Part 2 — What `crates/api` actually exposes today, re-verified

Confirmed by grepping every `.route(` call across
`crates/api/src/routes/*.rs`, 2026-09-02:

- **`/public/*`** (`mod.rs:23-42`): health, freshness, history-retention,
  incidents, `/public/lines`, `/public/lines/{id}/definition`,
  `/public/stations`, `/public/tocs`, `/public/tocs/all`, preferences,
  auth, chatbot. **No board/departures/arrivals route.**
- **Top-level, TfL-shaped, unauthenticated** (`line_status.rs:38-42`):
  `/Line/{ids}/Status`, plus (per `main.rs`, not re-read this session but
  cited unchanged by the 2026-09-01 design doc) `/Line/Mode/{mode}/Status`,
  `/StopPoint/{crs}/Disruption`, `/Line/{id}/Status/{from}/to/{to}`,
  `/Line/{id}/Stats/{from}/to/{to}`. These are **aggregate line-level
  status** (severity, reason, `dataQuality`, sample stats) — never a
  per-service, per-station board.
- **`/Train/*`, session-gated** (`train.rs:26-89`): `/Train/track`,
  `/Train/mine`, `/Train/{tracking_id}`, `/Train/by-uid/{train_uid}/{date}`,
  ticket/delay-repay sub-routes. These require an already-created,
  user-owned tracking pin (`AuthenticatedUser`, ownership check returning
  404 for both "doesn't exist" and "not yours" — `train.rs:1-13`). There is
  no route that answers "what's on the board at station X right now" for
  an arbitrary station.
- **`/private/*`**, internal-only (`ingest.rs:27-61`): `station-samples`
  (GET returns only a last-fetched timestamp — `ingest.rs:97-104` — POST is
  the poller's own ingest). The one place `station_samples` rows are ever
  *read* is `queries::latest_station_sample` (`queries.rs:664-681`), called
  from exactly one call site (`train.rs:524`, inside the "Darwin overlay"
  that corroborates a user's own tracked-train ETA against a fresh sample
  for its origin station — `crates/api/src/data/eta_blend.rs`). **No route,
  public or private, returns a station's sampled departures to a general
  caller.**

This matches the 2026-09-01 research doc's finding
(`2026-09-01-train-mcp-integration-research.md:154-160`) exactly, and
nothing added to `crates/api`'s routes since then changes it — the routes
added most recently (`chatbot.rs`, `notifications.rs`,
`history_retention.rs`, per `mod.rs`'s `pub mod` list and file
mtimes) are unrelated to board data.

### What `station_samples` actually contains, checked against what a board tool needs

Re-verifying the research doc's point 3 with the full struct
(`crates/common/src/lib.rs:376-401`), `StationDeparture` — the one row DS
stores per sampled station, replaced wholesale each poll
(`queries.rs:562-563`) — carries: `service_id`, `operator`,
`destination_crs`, `scheduled`, `estimated`, `is_cancelled`,
`delay_minutes`, `cancel_reason`, `delay_reason`, `headcode`,
`skipped_stations` (CRS list only). Missing, relative to what
`get_departures`/`get_arrivals`/`get_service_detail` return today
(`src/ldbws/types.ts`'s `BoardService`/`ServiceDetail`, mirrored in
`boards.ts:80-99`'s `serviceShape`): **platform**, **origin** (only
`destination_crs` is stored — the struct is departures-only in shape),
**full calling points** (only skipped-station CRS codes, not scheduled/
expected/actual times or cancel/delay reasons per stop), **`serviceKey`**
(the stable human-quotable handle `boards.ts` tells callers to prefer),
**`rsid`**, and the entire `ServiceDisruption` object
(`futureCancellation`, `futureDelay`, `affectedByDiversion`,
`rerouteDelay`, `currentDestinations`, `isReverseFormation`,
`isCircularRoute`, etc. — `boards.ts:66-78`) and `nrccMessages`. And
`poller-ldbws` only ever calls `GetDepBoardWithDetails`
(`crates/poller-ldbws/src/main.rs:1-16`'s own module doc) — **there is no
arrivals-board sampling at all**, so even a hypothetical read route over
`station_samples` could never back `get_arrivals`.

None of this is a criticism of `poller-ldbws` — it was built to answer "is
this line currently disrupted," a coarser question than "list me this
station's next ten trains with platforms and calling points," and its data
shape is exactly sufficient for that job and no more.

## Part 3 — What changed since the 2026-09-01 research/design docs, and whether it moves the needle

Three things this task specifically asked to be weighed:

**1. The `numRows` retry-fallback fix (`821abaf`/`c69bae7`, merged
2026-09-02).** `poller-ldbws::fetch_departures` now retries a busy
station's `GetDepBoardWithDetails` call with a halved `numRows` (10 → 5 →
2 → 1, `numrows_step_down`, `crates/poller-ldbws/src/main.rs:205-210`) up
to `MAX_NUMROWS_ATTEMPTS` (4) times, 500ms apart, and only on a genuine
5xx (`should_retry_with_smaller_rows`) — fixing "500s at busy termini"
(the commit's own subject). This is a real, well-tested resilience
improvement. **But it is not something an MCP-server-via-REST-API
architecture would let the MCP server inherit "for free," because it
lives in the wrong layer for that:** it is background-sampler resilience
(tolerate a bad response, keep the aggregate line-status pipeline moving,
retry on the *next* scheduled cycle if this one still fails) applied
*before* the data ever reaches Postgres — not request-time resilience a
synchronous MCP tool call could delegate to. An MCP tool calling
`crates/api` for a *live* board would still need the request to succeed
*right now*, in response to *this* call — `poller-ldbws`'s "skip this
station, log it, try again next cycle" failure mode (`main.rs:141-144`)
is exactly wrong for a synchronous tool response, which has no next
cycle to fall back to.

  What *is* true: the retry-with-smaller-`numRows` **strategy** (not the
  code) is directly portable to `distant-signal-mcp`'s own
  `LdbwsClient.board()` (`src/ldbws/client.ts:65-109`), which today makes
  exactly one attempt and surfaces a bare `LdbwsUnavailableError` on any
  non-2xx (`src/ldbws/client.ts:127-144`). Busy-terminus 500s are a
  property of LDBWS/RDM's `GetDepBoardWithDetails` operation itself, not
  of who's calling it — `distant-signal-mcp`'s own direct calls to the
  same operation are exposed to the identical failure mode `poller-ldbws`
  was fixed for, and would benefit from the same fix, implemented natively
  in TypeScript (a bounded halving-retry loop is perhaps 30-40 lines,
  the same shape as `numrows_step_down`/`fetch_departures` already are in
  Rust). This is a genuine, low-cost improvement worth doing — see
  Recommendation below — but it argues for *duplicating a small, well-
  isolated retry strategy*, not for *routing through `crates/api` to reuse
  Rust code*.

**2. The internal-service-OAuth2 refactor (concurrent work, not touched by
this pass).** Once landed, it removes the one real *mechanical* blocker
that exists today — `DsApiClient`'s anonymous-only design (`src/ds/client.ts:67-77`)
literally cannot call anything under `/private/*` — by giving the MCP
server the same `Authorization: Bearer` machine-credential path
`poller-ldbws` et al. use (`crates/common/src/oauth_client.rs`,
`crates/api/src/auth.rs`'s `require_internal_oauth`). This is a real
enabler *if and when* a board-shaped route exists to call. **It does not
by itself create such a route, and it does not change anything found in
Part 2**: today there is still nothing behind `/private/*` (or anywhere
else) that answers "give me station X's live board" for an arbitrary
station. An auth upgrade with nothing new to authenticate to closes zero
of the gaps this document found. It is worth noting for the future,
though: if `crates/api` ever does grow a board-shaped route (see Part 4),
gating it behind internal-service-OAuth2 rather than a new bespoke
mechanism would be the obviously consistent choice, and the refactor
landing first means that choice is already available rather than needing
its own design pass later.

**3. `distant-signal-mcp` now exists as a real, live sibling repo** (this
session read its actual current source, not a `train-mcp.zip` archive as
the 2026-09-01 docs did). The architecture the 2026-09-01 docs designed
against an archive is confirmed, unchanged, live: `mcpLiveBoardsGroup`
(`src/config.ts:88-90`), the three separate LDBWS product credentials
(`src/config.ts:93-116`), and `DsApiClient`'s deliberately-anonymous
`/public/*`-only calls (`src/ds/client.ts:67-77`) are all exactly what
Decision 5 (`2026-09-01-train-mcp-integration-design.md:760-784`)
anticipated and left alone. Nothing in the live repo has drifted from or
outgrown that decision.

## Part 4 — If this were pursued anyway: what `crates/api` would need

Characterized precisely, for the record, without recommending it (see
Recommendation):

**Option A — narrow: expose the curated `station_samples` set read-only.**
A new route, e.g. `GET /public/station-samples/{crs}` (or under
`/private/*` if internal-service-OAuth2 is the intended caller — a public
route open to arbitrary internet callers wasn't designed with
per-caller cost in mind, since it's LDBWS-derived data one caller's own
poll cycle paid for once). Cheap in engineering terms — the data already
sits in Postgres, `queries.rs:664-681` already has the read query, the
only missing piece is a handler and a `Serialize` response shape. But it
answers a materially narrower question than the MCP tools ask today:
only the fixed set of stations `poller-ldbws` already samples for line
inference (`GET /private/sample-stations`, `samples.rs:17`), one current
snapshot per station rather than a `timeOffset`/`timeWindow`/`callingAt`-
parameterized window, departures only (never arrivals, since
`poller-ldbws` never calls `GetArrBoardWithDetails`), and missing
platform/full-calling-points/`serviceKey`/`rsid`/disruption-object fields
(Part 2). It would not actually let `get_departures`/`get_arrivals`/
`get_service_detail` be re-implemented against it without a real drop in
what they can answer.

**Option B — broad: a genuine on-demand board proxy in `crates/api`.**
`crates/api` makes its own live `GetDepBoardWithDetails`/
`GetArrBoardWithDetails`/`GetServiceDetails` call per request, for any
station, with `filterCrs`/`timeOffset`/`timeWindow` passed through — i.e.
`crates/api` re-implements what `LdbwsClient` already does, behind a new
route, using its *own* separately-provisioned LDBWS credential (reusing
`poller-ldbws`'s existing sampling key would multiply that key's
metered-request volume by however many ad-hoc MCP calls come in, on top
of its existing 60s-cycle background load — `poll_interval_secs` default
60, `crates/poller-ldbws/src/config.rs:79-80`). This would need its own
resilience logic (the `numRows` retry-fallback from Part 3, likely worth
extracting from `poller-ldbws::main` into `crates/common` if two call
sites end up needing it), and its own rate-limiting story distinct from
the background sampler's. This is, in substance, standing up a second
implementation of `LdbwsClient` inside `crates/api` — not a thin
pass-through, a comparable amount of new code to what already exists in
`distant-signal-mcp`, just relocated and duplicated rather than removed.

Neither option is small-and-obviously-worth-it. A is cheap but doesn't
close the gap; B closes the gap but isn't cheap, and doesn't remove
LDBWS-calling code from the system — it just moves which process holds
the credential and does the calling, while adding a network hop and a
second implementation to keep in sync with RDM's own quirks (the
`UPSTREAM_WINDOW_HORIZON_MINUTES`/`MAX_ROWS` probed-live constants
`boards.ts:22-39` already had to reverse-engineer once).

## Part 5 — Tradeoffs, stated directly

- **Latency.** Direct LDBWS today: one hop, MCP server → RDM, ~30s client
  cache (`src/ldbws/client.ts:47`). Via `crates/api` (Option B): two hops,
  MCP server → `crates/api` → RDM, plus whatever auth overhead the
  internal-service-OAuth2 token exchange adds (cached per
  `crates/common/src/oauth_client.rs`'s `REFRESH_MARGIN`, so amortized
  low). A proxy adds latency without adding correctness — it doesn't make
  the answer more accurate, just slower to obtain, for zero present
  benefit against Option A's narrower data.
- **Staleness.** Via `station_samples` (Option A): bounded by
  `poller-ldbws`'s 60s cycle for the curated station set — actually
  comparable to `LdbwsClient`'s own 30-60s effective staleness (its cache
  TTL plus whatever gap since the last cache miss) for those stations
  specifically. Staleness is *not* the argument against Option A; data
  shape and station coverage are.
- **Rate-limiting two independent clients against RDM.** This is real and
  already acknowledged by the codebase's own design — it's why
  `mcp-live-boards` exists as a separate access gate at all
  (`2026-09-02-mcp-server-oauth-access-groups-design.md:365-416`): the
  operator is *already* running two independent LDBWS consumers
  (`poller-ldbws` server-side, `distant-signal-mcp` for its own tools)
  against what may or may not be the same RDM account, and has already
  chosen to manage that via visibility/access control (a human-curated
  group gate) rather than by merging the two call paths. Consolidating
  onto one caller would reduce total RDM request volume in principle, but
  only by giving up per-service, full-fidelity board data — see Option A's
  gap — which is a real cost, not a free consolidation.
- **Some use cases need real LDBWS fidelity a curated snapshot can't
  give.** `callingAt` filtering (server-side LDBWS parameter,
  `2026-07-21-train-mcp-phase1-design.md:374-378` per the earlier research
  doc), arrivals boards (not sampled at all), platform numbers, and the
  full `ServiceDisruption` object the design doc calls "the most
  consequential" disruption signal
  (`2026-07-21-train-mcp-phase1-design.md:360-362`) are all things a
  passenger-facing MCP tool needs and `station_samples` structurally
  cannot provide without `poller-ldbws` itself being widened into
  something closer to Option B.

## Recommendation

**Do not refactor the board tools to source from `crates/api`, now or as a
near-term follow-up.** This reaffirms Decision 5
(`2026-09-01-train-mcp-integration-design.md:760-784`) with fresh, current
evidence rather than assuming it still holds: the data gap it found is
unchanged, the `numRows` fix strengthens the case for `distant-signal-mcp`
adopting the *same resilience pattern independently* rather than for
routing through `crates/api`, and the internal-service-OAuth2 refactor
removes an auth blocker but not the underlying data-shape and station-
coverage gaps — there is still nothing behind any DS route to call for
general live-board data.

Two small, independent, non-blocking things worth doing regardless of this
recommendation, neither requiring any `crates/api` change:

1. **Port the `numRows` halving-retry strategy into `distant-signal-mcp`'s
   own `LdbwsClient.board()`** (`src/ldbws/client.ts:65-109`), mirroring
   `poller-ldbws::fetch_departures`/`numrows_step_down`
   (`crates/poller-ldbws/src/main.rs:205-338`) but with an actually-failed
   request surfaced to the caller after retries are exhausted (unlike the
   background sampler, an MCP tool call has no "try again next cycle" to
   fall back to). This directly fixes the same class of busy-terminus 500s
   `poller-ldbws` was just fixed for, in the layer that actually serves
   these tools, with no dependency on `crates/api` at all.
2. **If `crates/api`'s own frontend ever grows a general "any station's
   live board" feature for its own reasons** (unrelated to MCP), build
   that as the research doc already suggested
   (`2026-09-01-train-mcp-integration-research.md:781-784`) and revisit
   this question then — at that point Option B's cost would already be
   paid for a different reason, and reusing it for the MCP tools becomes
   the cheap case rather than the expensive one this document found today.

Everything else in this document — the route-surface citations, the
`StationDeparture` field gap, the two characterized options in Part 4 — is
preserved here so that a future revisit (should DS's own roadmap create
the "for free" case above) doesn't have to re-derive it.
