# train-mcp Integration — Landscape Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(the direct structural template for this document — a research pass that
evaluates plausibility, cites what it could and couldn't confirm, and
reaches a ranked recommendation without being an implementation plan).
Nothing here is wired up; no code in this repo was modified or connected
to anything as part of this pass.

## Problem being researched

A separate, already-substantially-built TypeScript project, **train-mcp**,
exists: an MCP (Model Context Protocol) server exposing UK rail data —
`resolve_station`, `get_departures`, `get_arrivals`, `get_service_detail`,
`find_services`, `plan_journey` — as tools for AI assistants. It gets its
data by polling National Rail Darwin/LDBWS directly and by ingesting its
own CIF timetable extract into a SQLite store it built and runs its own
RAPTOR/Connection-Scan-Algorithm (CSA) journey planner over, authenticated
via Discord OAuth. It is entirely independent of Distant Signal today:
separate codebase, separate upstream data licences, separate deployment.

This document asks two concrete questions:

1. Could a *derived* version of train-mcp use Distant Signal's own REST
   API (`crates/api`) as its upstream, instead of train-mcp's current
   direct Darwin/CIF polling — and for each of its six tools, is the data
   DS's API would need to supply already there, partially there, or
   entirely absent?
2. What would "delay aware" concretely mean for such a service, given
   Distant Signal has TRUST-derived individual-train tracking and
   sampled line-level delay data that train-mcp does not have today — and
   is any of that actually new value, or does train-mcp's direct Darwin
   access already give it most of this for free?

## Method

This is a single-agent research pass: one agent reading a provided
`train-mcp.zip` archive (extracted to a scratch location outside this
git worktree, never committed) plus this repository's own source, in one
session. No live external research, no upstream API calls, no contact
with train-mcp's maintainer. Every claim about what Distant Signal's API
does or doesn't expose is backed by a `file:line` citation into
`crates/api/src/routes/*.rs` or the crate it delegates to, read directly
for this pass — not inferred from route names or from train-mcp's own
README. Every claim about train-mcp's own behaviour is backed by a
citation into its design docs or source, also read directly. Two files in
the archive, `.env` and `.env.prod`, were deliberately not opened —
per the task's own instruction, they hold what are likely real
credentials (Discord bot token, RDM API keys) and are irrelevant to a
landscape survey of architecture and data shape.

### A relevant data point: a currently-failing MCP connector

This session's environment lists a configured MCP connector named
"National rail status" that fails to connect with an HTTP 502 (Cloudflare
Bad Gateway) from its own upstream. This could not be inspected further —
no tool available in this pass can see that connector's backend
configuration or logs. But train-mcp's own README documents a live
production deployment at `rail.skyes.lgbt`, "Docker Compose behind
caddy-docker-proxy on the shared `caddy` network, deployed over SSH"
(`README.md:250-266`, train-mcp archive), which is exactly the shape of
service that would sit behind a Cloudflare-fronted domain and return a
gateway error if its origin container were down or its DNS/proxy
misconfigured. This is circumstantial, not confirmed — the connector's
actual backend was never observed — but it raises a real possibility
worth stating plainly: this may not be a hypothetical "should we build
this" question. A derivative of train-mcp (or train-mcp itself) may
already be deployed and simply down right now, which would make this
research relevant to an existing, currently-broken integration rather
than a purely speculative one. Flagged as an open question below, not
assumed further in the rest of this document.

## What train-mcp is today

Four architectural layers, per its own Phase 1 design doc: Transport
(Express `/mcp` endpoint, OAuth bearer gate) → Tools (thin: schema in,
formatted result out) → Domain (Darwin client, station resolution, time
handling — no MCP dependency) → Reference (`stations.json`, baked from a
CORPUS extract) (`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:37-49`,
train-mcp archive). Six tools are registered in `src/server.ts:33-40`
(train-mcp archive): `resolve_station`, `get_departures`/`get_arrivals`
(Darwin LDBWS live boards), `get_service_detail` (Darwin `GetServiceDetails`),
and `find_services`/`plan_journey` (its own CIF-derived SQLite timetable
store, queried via RAPTOR for multi-option answers and Connection Scan
for the single fastest journey — `docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:36-53`).
Auth is Discord OAuth as a resource server (train-mcp verifies tokens,
never issues them), gated by a hand-curated Discord user-ID allowlist
(`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:380-441`).

## Per-tool findings against Distant Signal's current API surface

Distant Signal's full current route surface, read directly from
`crates/api/src/routes/*.rs` (`grep -n '\.route(' crates/api/src/routes/*.rs`):

- **Public, unauthenticated** (`crates/api/src/routes/mod.rs:20-42`):
  `/public/health`, `/public/freshness`, `/public/history-retention`,
  `/public/incidents/{incidentId}` (`incidents.rs:18`), `/public/lines`,
  `/public/lines/{id}/definition` (`lines.rs:27-32`), `/public/stations`,
  `/public/tocs`, `/public/tocs/all` (`reference.rs:20-22`),
  `/public/preferences*`, `/public/auth/*`.
- **Top-level, unauthenticated, TfL-response-shaped** (mounted directly
  in `crates/api/src/main.rs:57-60`, not under `/public`, so existing TfL
  API clients work unchanged): `/Line/Mode/{mode}/Status`,
  `/Line/{ids}/Status`, `/StopPoint/{crs}/Disruption`,
  `/Line/{id}/Status/{from}/to/{to}`, `/Line/{id}/Stats/{from}/to/{to}`
  (`line_status.rs:38-42`).
- **Top-level, requires a real user session** (`AuthenticatedUser`,
  `train.rs:1-13`): `/Train/track`, `/Train/mine`, `/Train/tickets/mine`,
  `/Train/{tracking_id}`, `/Train/by-uid/{train_uid}/{date}`, plus
  ticket/delay-repay sub-routes (`train.rs:28-36`).
- **`/private/*`**, gated by a shared-secret header
  (`crates/api/src/auth.rs:1-33`), reachable only from other services in
  the deployment, not from any external client: station-sample ingest,
  TOC/schedule-feed ingest, tracked-train reference reload.

### `resolve_station` — DS has a direct, cheap fit

`GET /public/stations?q=` (`reference.rs:18-42`) is a type-ahead search
over reference data, matching train-mcp's own `resolve_station` cascade
in shape (name/code query in, ranked candidates out) and is exactly what
`frontend/lib/suggestions.ts:9-14`'s `searchStations` already calls
through the frontend's own `/api/stations` proxy. train-mcp's version
additionally returns `tiploc` and a `matchType` confidence tag
(`src/tools/resolve-station.ts:5-10`, train-mcp archive) that DS's
`Suggestion` shape was not confirmed to carry (not read this pass — the
`Suggestion` struct itself, in `crates/api/src/data/reference.rs`, was
not opened). **Verdict: cheap.** If DS's `Suggestion` lacks `tiploc` or a
match-confidence field, that's a small additive change to an
already-existing, already-public route — not a new capability.

### `get_departures` / `get_arrivals` — DS ingests the right data but exposes none of it publicly

This is the most consequential finding in this document. train-mcp's
board tools return a live per-station departure/arrival board: every
service's scheduled/expected time, platform, operator, and a rich set of
per-service disruption facts extracted straight from Darwin
(`futureCancellation`, `futureDelay`, `currentDestinations`,
`rerouteDelay`, `cancelReason`, `delayReason`, `nrccMessages`, etc. — see
`src/ldbws/map.ts:322-341`, train-mcp archive) — surfaced in rendered
text as an indented `⚠` line (`src/tools/boards.ts:410`, train-mcp
archive) and confirmed in the design doc's rendering rule
("flags are rendered only when true" — `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:368-372`).

Distant Signal genuinely ingests the raw material for this:
`poller-ldbws` "samples live departure-board data for every station any
line's inference logic depends on" (`crates/poller-ldbws/src/main.rs:1-2`)
and POSTs the parsed result to `/private/station-samples`
(`crates/api/src/routes/ingest.rs:39-41`, wired through
`upsert_station_samples`, `crates/api/src/data/queries.rs:258`), landing
in the `station_samples` table. But three things separate this from what
`get_departures`/`get_arrivals` need:

1. **No public route reads `station_samples` at all.** The only reader
   is `latest_station_sample` (`crates/api/src/data/queries.rs:566-582`),
   called from exactly one place — `train.rs`'s `blend_darwin_eta`
   (`crates/api/src/routes/train.rs:239-261`) — to overlay a *single*
   destination-matching departure onto an already-authenticated,
   already-owned tracked-train's ETA. There is no route that returns "the
   board for station X."
2. **DS only samples a curated subset of stations, not an arbitrary one a
   caller might ask about.** `poller-ldbws` learns which CRS codes to
   poll from `GET /private/sample-stations` (`crates/poller-ldbws/src/main.rs:14-16`,
   backed by `crates/api/src/routes/samples.rs:17`'s
   `get_sample_stations`) — the stations DS's own line-status inference
   needs, not a general "any station in the country" set. A caller asking
   `get_departures` for a station DS doesn't currently sample for line
   inference would get nothing, regardless of a new route being added.
3. **`station_samples` is a single current snapshot, not a board.** Per
   its own query-layer doc comment, the table is "wholesale-replaced per
   poll (one row per station, no history" — `crates/api/src/data/queries.rs:562-563`)
   — one row per station holding whatever `StationDeparture[]` LDBWS last
   returned (`crates/common/src/lib.rs:362-385`), not distinguished by
   `timeOffset`/`timeWindow`/`callingAt`/`rows` the way train-mcp's tool
   parameters expect.

**Gap characterization: partial, and the cheap part and the expensive
part are different.** Exposing what DS already samples as a new
`GET /public/station-samples/{crs}`-shaped read route is cheap — the
data is already in Postgres, already parsed into `StationDeparture`,
already carrying `delay_minutes`/`cancel_reason`/`delay_reason`/
`skipped_stations` (`crates/common/src/lib.rs:362-385`). But it would only
ever answer for the curated set of stations DS already samples for line
inference (currently on the order of the stations its ~20-100-line
catalogue needs, per DESIGN.md's own scope, not read this pass but
referenced by the other-uk-transit-networks research doc). Widening
`poller-ldbws` to sample *any* station on demand — which is what a
general-purpose MCP board tool implies — is a materially different
(and ongoing-cost) change: more RDM quota consumption, a cache-and-poll
strategy per arbitrary station rather than a fixed curated list, and
losing the "we only poll what the aggregator needs" invariant the
current design leans on. `callingAt` filtering (which train-mcp's own
design doc confirms is a genuine LDBWS server-side capability, not
something the client computes — `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:374-378`)
would also need to be added to `poller-ldbws`'s own request shape, since
DS's poller does not appear (not verified against `poller-ldbws/src/config.rs`
in full) to request it today for its own line-inference purposes.

### `get_service_detail` — DS's individual-train data is real but scoped to a different problem

train-mcp's `get_service_detail` takes an opaque, short-lived Darwin
`serviceId` (handed out by a prior board call) and returns full calling
points for *any* service a board surfaced
(`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:274-296`).
Distant Signal's individual-train-tracking routes
(`/Train/{tracking_id}`, `/Train/by-uid/{train_uid}/{date}`,
`train.rs:31-32`) look superficially similar — both resolve to "detail
about one specific train" — but are a fundamentally different shape:

- **Ownership-gated, not open lookup.** Both read routes require
  `AuthenticatedUser` and check `tracked_train_owner(...) == user.id`,
  returning `404` (not `403`) for both "doesn't exist" and "exists but
  isn't yours" (`train.rs:200-217`, `train.rs:219-237`, per the module's
  own doc comment at `train.rs:1-13`). There is no route that returns
  detail for an arbitrary train a caller hasn't first "pinned."
- **The underlying TRUST data is scoped to tracked trains only, not
  every running service.** `trust-consumer`'s own module doc states it is
  "a persistent Kafka consumer for Network Rail's TRUST Train Movements
  feed... filtered to exactly the currently user-tracked `(train_uid,
  date)` set" and explicitly "NOT a cron-style poller" over everything
  running (`crates/trust-consumer/src/main.rs:1-5`). DS does not, today,
  have TRUST-derived state for trains nobody has pinned.
- **What the data itself contains is genuinely richer where it exists.**
  `TrackedTrainState` (`crates/api/src/data/train_tracking.rs:293-308`)
  carries `status`, `last_reported_location`, `last_event_type`,
  `delay_minutes`, `next_calling_point`, `eta_next`, `eta_source` — TRUST
  movement-derived facts train-mcp's Darwin-only `get_service_detail`
  does not have (Darwin gives scheduled/estimated times per calling
  point, not "last reported location" from an actual berth/TD event).
  `eta_next` can additionally be blended live against a fresh Darwin
  board sample for the same origin station
  (`crates/api/src/data/eta_blend.rs:1-25`, applied in
  `train.rs:239-261`'s `blend_darwin_eta`), a genuine corroboration
  train-mcp has no equivalent of.

**Gap characterization: expensive, and expensive in a specific way — not
"DS doesn't have the data" but "DS's data requires a user-owned tracking
pin to exist first."** A `get_service_detail`-equivalent MCP tool that
takes any Darwin `serviceId` or headcode/UID and returns detail
regardless of whether anyone has pinned it would need either (a) a new,
unscoped read path over TRUST data DS doesn't currently retain outside
the tracked-train set — expanding `trust-consumer`'s scope from
"currently-tracked trains" to "everything," a materially larger
ingestion/matching problem the crate's own doc comment identifies as a
deliberate design boundary, not an oversight — or (b) falling back to
Darwin's own `GetServiceDetails` directly for untracked trains, which is
exactly train-mcp's current behaviour and adds nothing DS-specific. The
realistic near-term shape is narrower: expose read access to a train
*that the MCP caller has themselves pinned via `POST /Train/track`*,
which is cheap (the route and ownership model already exist) but answers
a different question than train-mcp's tool does today ("tell me about
the train I'm tracking," not "tell me about any train on this board").

### `find_services` / `plan_journey` — DS has no queryable timetable store at all today

train-mcp's own CIF timetable store is a real, substantial piece of
engineering: parsing a ~600MB fixed-width CIF extract (26,848 schedules,
316,362 calling points, ~289,514 connections on a measured weekday —
`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:62-65`)
into a SQLite store with STP-precedence resolution, then running RAPTOR
(for Pareto-optimal multi-change options) or Connection Scan (for
single-fastest) over it, with route constraints (`via`/`avoid`/
`viaStop`/`avoidStop`), interchange-time modelling from MSN and ALF fixed
links, and differential testing between the two algorithms as its
primary correctness mechanism
(`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:36-53`).
This was described in train-mcp's own design doc as "roughly a month of
work" (`docs/superpowers/specs/2026-07-21-train-mcp-phase2a-timetable-store-design.md:51-53`).

Distant Signal's `schedule-ingest` crate, despite the name, does **not**
build anything like this today. Its own doc comment states its scope
precisely: it "watches a locally mounted directory for a pushed CIF
SCHEDULE feed delivery from Network Rail/RDG, verifies completeness
against the delivery's own manifest, and forwards completed sequences to
the `api` crate's ingestion endpoint" (`crates/schedule-ingest/src/main.rs:1-3`).
What lands in `api` is delivery *metadata* only — the
`schedule_feed_ingests` table stores `sequence`, `ingested_at`, and
`files` (a JSON array of filenames and byte sizes,
`crates/api/migrations/20260901130000_schedule_feed_ingests.sql:12-16`,
matching `ScheduleFeedFile { name, bytes }` in
`crates/api/src/routes/ingest.rs`'s handler) — never the CIF content
itself, parsed or otherwise. This is confirmed explicitly, not inferred:
"What `api` does with the files once they're on disk is still out of
scope" (`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md:149`).
There is no `schedules`/`calling_points` table, no TIPLOC-to-CRS
timetable join, nothing queryable by "what runs between A and B."

**Gap characterization: expensive — comparable to porting a real chunk
of train-mcp's own ingestion, not a route addition.** This is the
starkest finding in this document. `find_services` and `plan_journey`
are not "DS has the data, needs a new endpoint" — DS currently receives
and verifies the raw CIF *files* (a real, working, recently-built
capability: `schedule-ingest` and its SFTP delivery path are live per
`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` and the
`b5cbdfe`/`cbbd59a`/`3f60263` commits visible in this repo's own recent
history) but parses none of their content into a store. Building that
store — CIF record parsing, STP resolution, a schema, RAPTOR/CSA over
it — is substantively the same engineering investment train-mcp's own
Phase 2a/2b already represent, done once already in a different
language, for a different codebase. The cheapest realistic path to these
two tools is **not** "add a DS API route and reuse train-mcp's planner
code against it" — it is either (a) DS builds its own timetable store
(a multi-week project, duplicating train-mcp's own build), or (b) the
derived MCP service keeps train-mcp's *existing* CIF ingestion/store/
planner code entirely as-is and only swaps its four other tools
(`resolve_station`, the two board tools, `get_service_detail`) onto DS's
API — i.e., a genuinely partial migration, not a clean "MCP service
calls only DS's API" architecture for all six tools.

## Sketch: where would a derived service live

Two shapes, not fully designed here:

**A. A new Rust crate in `crates/`, MCP-native.** Distant Signal's
existing per-service pattern — one crate per concern, one
`docker-compose.yml` service, one Helm `*-deployment.yaml` template
(`ls charts/distant-signal/templates/` shows this pattern held
consistently: `aggregator-deployment.yaml`, `enricher-deployment.yaml`,
`poller-deployments.yaml`, etc.) — would suggest a `crates/rail-mcp` (or
similar) crate implementing an MCP server directly in Rust, calling
DS's own internal data-access layer (`crates/api/src/data/*`) in-process
or over the same internal-token-gated pattern the other services already
use. This fits the repo's Rust-workspace and single-deployment-story
conventions cleanly, and would let it reuse `common`'s types directly
rather than re-deriving them from JSON. **But** the MCP TypeScript SDK
(`@modelcontextprotocol/sdk`, what train-mcp is built on) is
considerably more mature than the Rust MCP ecosystem as of this
research, and none of train-mcp's substantial existing timetable/planner
code (the expensive part, per the previous section) is reusable in Rust
without a full rewrite.

**B. Keep it a separate TypeScript MCP server, closer to train-mcp's
current shape minus its own Darwin/CIF polling.** train-mcp's own
architecture already draws exactly the seam this would need: its
"Domain" layer (`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:37-49`)
is already isolated from its Darwin client behind plain typed functions.
A derived service could keep the MCP transport, tool definitions,
station-name resolution, rendering, and (crucially) the entire CIF
timetable store and RAPTOR/CSA planner **unchanged**, and only replace
`src/ldbws/client.ts`'s HTTP calls with calls to DS's `/public/*`
routes for the tools DS can actually serve today
(`resolve_station`→`/public/stations`, and any future
`get_service_detail` narrowed to a caller's own tracked train). This
keeps train-mcp's already-substantial, already-tested CIF/planner
investment intact rather than discarding it, at the cost of the service
remaining outside this repo's Rust workspace and Helm chart pattern (it
would need its own Docker image and its own chart template, mirroring
`schedulefeed-*.yaml`'s pattern for a non-Rust auxiliary service already
in this chart). **This shape is the more honest one given the actual
gap analysis above**: since two of six tools (`find_services`,
`plan_journey`) cannot be served by DS's API without DS itself building
a timetable store DS doesn't have, "MCP service calls only DS's REST
API" is not achievable today regardless of which language the service is
written in — the real near-term choice is between "derived service, four
tools swapped to DS's API, two tools keep polling/ingesting for
themselves" (shape B) or "do nothing until DS has its own timetable
store" (defer).

## Delay-awareness

**train-mcp's board tools are already reasonably delay-aware from Darwin
directly — this is the honest baseline, not a gap.** `src/ldbws/map.ts`
extracts `delayMinutes`, `cancelReason`, `delayReason`,
`futureCancellation`, `futureDelay`, `rerouteDelay`,
`currentDestinations`, `affectedByDiversion`, `serviceChangeRequired` and
more directly from Darwin's own response
(`src/ldbws/map.ts:322-341`, train-mcp archive), and renders them as an
explicit `⚠` warning line the design doc calls "the most consequential"
disruption fact it surfaces, specifically because "a service reading 'on
time' at the queried station and cancelled further along is precisely
the trap a connection plan walks into"
(`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:360-362`).
Any claim that DS's data would make `get_departures`/`get_arrivals`
"delay aware" for the first time would overstate the case: Darwin's own
live boards already carry per-service delay minutes, reasons, and
future-disruption flags.

**Where DS's data would add something genuinely new to the board tools:
corroboration, not a new capability.** DS's TRUST-derived
`last_reported_location`/`last_event_type`
(`crates/api/src/data/train_tracking.rs:302-303`) comes from a different
upstream system (actual train-describer/berth movement reporting via
TRUST) than Darwin's own predictive `etd`/`eta` estimates, which are
themselves partly modelled/interpolated. `crates/api/src/data/eta_blend.rs`
already exists as a working precedent for exactly this kind of
cross-checking — it blends a fresh Darwin board sample against
TRUST-derived tracked-train state at read time, deliberately "NOT a
guaranteed join" (`eta_blend.rs:1-7`). A derived MCP tool surfacing "TRUST
last reported this train at X, Darwin's board says Y minutes late" would
be a real, if narrow, improvement over either source alone — but only
for the (currently: user-tracked) subset of trains DS has TRUST data
for, per the `get_service_detail` findings above.

**Where DS's data would add genuinely novel value: `plan_journey`
factoring in currently-known live delays.** train-mcp's own Phase 2b
design doc explicitly scoped this out and previewed it as Phase 2c:
"Cross-checking a plan against live data... `get_departures` knows what
is running now, and Phase 1.2 surfaced `futureCancellation` and
curtailment per service — but a plan is only worth re-checking within
the roughly two-hour window a board can see, so the overlap is narrow"
(`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:166-172`).
This is the single most valuable "delay aware" capability available
here, and it's novel precisely because standalone journey planners are
almost always pure-schedule — train-mcp's own `find_services`/
`plan_journey` explicitly carry "provisional"/"provenance" caveats
(`README.md:35-77`, train-mcp archive) that are schedule-confidence
caveats, not live-delay caveats. A plan that says "board at platform 4 at
14:32, 6 minutes to change at Reading" is silently wrong the moment the
first leg is running 8 minutes late — something Darwin's own live board
for the first leg's origin station would already show, and something
DS's TRUST-derived `delay_minutes` would show too, for a leg DS happens
to be tracking. **What this would concretely require**, sketched, not
designed: for each leg of a candidate plan whose departure falls inside
Darwin's live board horizon (per train-mcp's own finding, "about 239
minutes — just under four hours,"
`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:340-341`),
look up the live board for that leg's origin station (train-mcp's own
Darwin call today, or DS's sampled-station data where DS happens to
cover that station) and annotate the leg with its current delay/
cancellation status, then re-evaluate whether the plan's stated
interchange time still holds against the live estimate rather than the
scheduled one. This is real, scoped work (train-mcp's own doc calls the
overlap with a plan's live-checkable window "narrow," correctly) but it
is the one place in this whole survey where DS's data — or even just
Darwin's own live data, applied to train-mcp's *existing* planner output
— would give a capability neither DS nor train-mcp has today in
isolation.

**Line-level status/`SampleStats` (`crates/aggregator`) is the weakest
fit of DS's delay infrastructure for these tools.** `LineStatus` carries
`data_quality` (`Knowledgebase | LdbwsInferred | TrustInferred | Planned
| Tfl`) and an optional `sample_stats` (`total`, `delayed`, `cancelled`,
`skipped`, `avg_delay_minutes` — `crates/common/src/lib.rs:324-335,
675-681`), which is genuinely useful for "how is this whole line running
right now" — a question none of train-mcp's six tools ask. It doesn't
map onto any of the per-service or per-journey questions train-mcp's
tools answer; it would be a plausible *seventh* tool ("line health
summary"), not an enhancement to the existing six.

## Licensing

train-mcp's own README and design docs record no dedicated licensing
analysis — its setup instructions simply say to "Register for a Darwin
OpenLDBWS token at the National Rail Data Portal" and "Register a Network
Rail data feeds account" (`README.md:109-111`, train-mcp archive),
without engaging with the attribution/branding conditions those licences
carry. Distant Signal, by contrast, has done real, cited work on exactly
this: `frontend/components/OpenDataAttribution.tsx`'s doc comment
distinguishes three separate licence families this app already holds —
TfL's modified OGL v2.0 (fixed "Powered by TfL Open Data" wording,
required), National Rail Enquiries' Terms & Conditions v3.0 covering all
four of DS's RDM feeds (Knowledgebase, LDBWS, Stations, TOCs — fixed
"Powered by National Rail Enquiries" wording, required), and Network
Rail Infrastructure Limited's own separate terms covering the TRUST feed,
which explicitly *prohibit* NR/NRE/TOC branding and the word "official"
(`OpenDataAttribution.tsx:1-45`; further detailed in
`docs/superpowers/specs/2026-08-28-train-tracking-design.md:255-297`,
quoting Network Rail's open-data-feeds terms page directly: "You may not
use our brand or logo or those of any of our partners including National
Rail and the train companies").

**Is holding two independent NRE/Darwin permits for the same underlying
data a conflict?** No evidence found that it would be — RDM licence terms
are per-Data-Consumer, not exclusive
(`docs/superpowers/specs/2026-08-28-train-tracking-design.md:257-259`
quotes RDM's own terms: licensing "depend[s] on the Data Publisher's
approach," with RDG itself "not a party to the Data Sharing Agreement" —
nothing here implies exclusivity or a conflict between two consumers).
It is **redundant**, not a problem: train-mcp holds its own separate RDM
LDBWS subscription (`README.md:144-146`'s `LDBWS_DEPARTURES_URL`/`_KEY`
etc., train-mcp archive) entirely independent of DS's own `poller-ldbws`
subscription to the same product family. This redundancy is precisely
the argument in favour of the "MCP service calls DS's API" architecture
over "keep train-mcp's own polling": a derived service riding on DS's
existing NRE licence for LDBWS/Stations/TOCs data would not need its own
separate LDBWS subscription or its own attribution obligation for that
data — DS already carries it. It would, however, still need its own
attribution surface if it presents data to end users directly (an MCP
tool's rendered text reaching a chat client is arguably "presenting"
NRE-derived data, though whether MCP tool output counts as the kind of
public-facing presentation NRE's attribution clause is aimed at is a
genuinely unresolved question, not addressed by either project's
existing documentation and not resolved by this research pass).

## Auth

train-mcp is an OAuth 2.0 resource server delegating to Discord, gated
by a hand-curated Discord user-ID allowlist
(`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:380-441`).
Distant Signal has two entirely different, purpose-built auth systems,
neither of which is Discord-shaped: an OIDC relying-party client for
real end-user SSO sessions (`crates/api/src/auth/oidc.rs:1-6`, cookie-
based per `train.rs`'s own test-fixture `seed_session`/
`distant_signal_session` cookie pattern), and a single shared-secret
header (`X-Internal-Token`) for service-to-service calls between DS's own
crates (`crates/api/src/auth.rs:1-6`). Neither is designed for a
third-party MCP client to authenticate a human end user through.

Three options, sketched, not designed:

1. **Keep Discord auth, unchanged.** The derived service still gates
   access the way train-mcp does today; it just calls DS's public API
   routes as an anonymous (or internal-token-bearing, for anything
   currently `/private`) HTTP client, same as any other consumer of DS's
   public API. Cheapest, and decouples "who's allowed to use this AI
   tool" from "who has a Distant Signal account" — plausible if the MCP
   service's intended audience (Discord-authenticated individuals) isn't
   the same population as DS's own SSO users.
2. **Reuse DS's OIDC SSO**, making the derived service another OIDC
   relying party against the same identity provider DS's frontend uses.
   This is the more "first-class feature of this repo" framing the task
   description asks about, but it's a real scope increase: MCP clients
   (per train-mcp's own README) commonly assume Dynamic Client
   Registration (RFC 7591), which Discord doesn't support and which would
   need to be checked against DS's own OIDC provider too
   (`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:174-178,
   434-441` documents this exact limitation for Discord; whether DS's
   OIDC provider supports DCR was not checked this pass).
3. **No auth at all for read-only tools**, matching `/public/*`'s own
   existing unauthenticated-read convention (`reference.rs:1-4`'s doc
   comment: "Unauthenticated, read-only — same `public_router()`
   pattern"). Plausible for the four tools that map to genuinely public
   DS data (`resolve_station`, boards, line status), not plausible for
   anything touching a user's own tracked trains.

No option here is free of a real design decision; this section is a
sketch of the shape of that decision, not a recommendation between them.

## Cost/benefit framing

Unlike the other-UK-transit-networks research (where every candidate
network was small relative to DS's existing scope), train-mcp is a
mature, tested, already-in-production system covering the *same*
National Rail data ecosystem DS already operates in — the comparison
here is not "is this worth adding" in the abstract, it's "does routing
train-mcp's tools through DS's API instead of its own polling pay for
itself." The answer differs sharply by tool:

- **`resolve_station`**: cheap, low-risk, genuine dedup — DS's
  `/public/stations` is already close to a drop-in replacement.
- **Board tools**: partial win. DS's data is real but scoped to a
  curated station set DS already samples for its own line inference; a
  general-purpose board tool needs that widened, which is an ongoing
  RDM-quota and polling-architecture cost, not a one-time route addition.
- **`get_service_detail`**: narrow win at best. DS's TRUST data only
  exists for trains someone has already pinned via the tracked-trains
  feature; a general "any service" lookup gets nothing DS-specific DS
  doesn't already have from Darwin directly (which train-mcp already
  polls itself).
- **`find_services`/`plan_journey`**: no win available cheaply. DS has no
  queryable timetable store today; building one is comparable in scope
  to train-mcp's own multi-week Phase 2a/2b investment, already done
  once. Migrating these two tools onto DS's API is not currently
  possible without DS first replicating work train-mcp has already done.

The genuinely novel capability this whole exercise surfaces —
delay-aware journey planning, annotating a schedule-based plan's legs
with live delay/cancellation status where the departure falls inside a
board's live horizon — is real and valuable, but it does not require
migrating train-mcp onto DS's API to build. It requires only Darwin's
own live boards (which train-mcp already has direct access to) plus
train-mcp's own existing plan output; DS's TRUST data would add
corroboration for the subset of legs DS happens to be tracking, not a
capability unavailable without DS.

## Recommendation

**Not a "yes, migrate all six tools" case today — but three narrow,
independently worthwhile follow-ups are visible, ranked below, and the
door should stay open pending the connector question.**

1. **Resolve the "is this already deployed and just down" question
   first, before any design work.** This is the cheapest possible next
   step (asking the person who configured the "National rail status" MCP
   connector, or checking `rail.skyes.lgbt`'s status directly) and its
   answer changes the shape of everything else — a currently-broken
   production integration is a different priority than a hypothetical
   one.
2. **`resolve_station` via `/public/stations` — worth doing, low
   effort, whenever a derived service exists at all.** No architectural
   blocker found; the only open item is confirming `Suggestion`'s exact
   field shape against what `resolve_station`'s callers need.
3. **Delay-aware journey planning (the Phase 2c idea) is the highest-
   value single capability surfaced by this research, and is buildable
   independent of any DS migration.** It requires no new DS route at
   all — just extending train-mcp's own `plan_journey` to consult
   Darwin's live boards (which it already has credentials and client
   code for) for legs inside the live horizon. Recommended as the
   standalone highest-priority follow-up regardless of what happens with
   the rest of this document's questions.
4. **Board tools and `get_service_detail` — watch, don't build yet.**
   Both need DS to widen scope (sampled-station coverage; TRUST coverage
   beyond tracked trains) before a DS-backed version would offer more
   than train-mcp's own direct Darwin access already does. Revisit if
   DS's own roadmap independently grows either of those (e.g. if DS ever
   wants a general "any station's live board" feature for its own
   frontend, at which point the same new route would serve both DS and
   a derived MCP tool for free).
5. **`find_services`/`plan_journey` migration to a DS-backed store — not
   recommended now, in either direction.** Neither "port train-mcp's
   timetable store into DS" nor "make DS's `schedule-ingest` grow one" is
   justified by this research alone; it's a multi-week, high-risk (STP
   resolution "fails silently when wrong," per train-mcp's own design
   doc) project whose benefit is "same capability train-mcp already has,
   running on different infrastructure," not new capability. Worth
   revisiting only if DS decides it wants a timetable store for its own
   reasons (e.g. richer planned-vs-actual comparison for tracked trains),
   at which point train-mcp's own design docs are a genuinely useful
   reference implementation to build against.

If any of items 2-4 are pursued, the next step per this repo's own
convention is a dedicated design-spec pass at the depth
`docs/superpowers/specs/2026-08-28-train-tracking-design.md` or
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
gave their respective features — not attempted here.

## Open questions (explicit, not resolved here)

1. **Is the failing "National rail status" MCP connector train-mcp (or a
   derivative) already deployed?** Genuinely unresolved by this pass —
   no tool available here could inspect the connector's backend. This
   should be checked directly (ask the user, or check
   `rail.skyes.lgbt`'s live status) before any further design work, since
   it changes whether this is "should we build" or "should we fix/adopt
   an existing broken thing."
2. **Does DS's `Suggestion` type (`crates/api/src/data/reference.rs`, not
   read this pass) carry `tiploc` and a match-confidence signal
   equivalent to train-mcp's `matchType`?** If not, a small additive
   change; unconfirmed either way.
3. **Does `poller-ldbws` request `callingAt`-style filtering from RDM's
   LDBWS product today?** Not verified against `crates/poller-ldbws/src/config.rs`
   in full this pass — matters for whether board-tool filtering is a
   poller change or purely a query-layer change.
4. **What is DS's actual current curated station-sample set's size and
   coverage?** Not measured this pass (would require reading
   `crates/aggregator`'s line definitions/`lines/*.toml` in full) — this
   number is the real ceiling on how useful a DS-backed board tool would
   be without widening `poller-ldbws`'s scope.
5. **Does MCP tool-rendered text count as "presentation" under NRE's
   attribution clause, obligating a derived service to carry its own
   "Powered by National Rail Enquiries" text somewhere a user might see
   it (e.g. in a chat client's tool-result rendering)?** Not resolved by
   either project's existing documentation; a genuine open legal/product
   question, not merely an engineering one.
6. **Does DS's OIDC identity provider support Dynamic Client
   Registration (RFC 7591)?** Relevant only if Auth option 2 (reuse DS's
   SSO) is pursued; not checked this pass.
7. **What RDM quota/cost would widening `poller-ldbws` to arbitrary
   stations actually consume?** Not measured — DS's own LDBWS product
   subscription terms and current utilization were not read this pass.

## References

- train-mcp archive (extracted to a scratch location outside this git
  worktree for this research pass; not committed):
  `README.md`, `TODO.md`,
  `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md`,
  `docs/superpowers/specs/2026-07-21-train-mcp-phase2a-timetable-store-design.md`,
  `docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md`,
  `src/server.ts`, `src/config.ts`, `src/ldbws/map.ts`,
  `src/tools/resolve-station.ts`, `src/tools/boards.ts`,
  `src/tools/find-services.ts`, `src/tools/plan-journey.ts`.
  (`.env` and `.env.prod` were present in the archive and deliberately
  not opened — likely real credentials, irrelevant to this survey.)
- `crates/api/src/routes/mod.rs`, `main.rs`, `routes/reference.rs`,
  `routes/train.rs`, `routes/line_status.rs`, `routes/incidents.rs`,
  `routes/ingest.rs`, `routes/samples.rs`, `auth.rs`, `auth/oidc.rs`,
  `data/queries.rs`, `data/eta_blend.rs`, `data/train_tracking.rs`
  (this repository).
- `crates/poller-ldbws/src/main.rs`, `crates/trust-consumer/src/main.rs`,
  `crates/schedule-ingest/src/main.rs`, `crates/aggregator/src/main.rs`,
  `crates/common/src/lib.rs` (this repository).
- `crates/api/migrations/20260901130000_schedule_feed_ingests.sql`
  (this repository).
- `frontend/lib/suggestions.ts`, `frontend/components/OpenDataAttribution.tsx`
  (this repository).
- `docs/superpowers/specs/2026-08-28-train-tracking-design.md`,
  `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md`,
  `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
  (this document's structural template) (this repository).
- `charts/distant-signal/templates/` and `docker-compose.yml` service
  lists (this repository) — used only to characterize this app's
  existing per-service deployment pattern, not for any new claim about
  train-mcp.
