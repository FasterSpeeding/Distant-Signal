# train-mcp Integration — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` (data-
integration structure, decisions with real alternatives, an explicit
"out of scope" ledger) and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(section conventions, "Current relevant state" cited to real code, "Open
questions/risks" as a first-class section). Builds directly on
`docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`
(hereafter "the research doc"), which is treated as source of truth for
train-mcp's own architecture and the per-tool gap analysis — cited, not
re-derived, except where a specific claim is re-verified below because
this design leans on it.

**Correction to the research doc's own Open Questions.** Open question 1
there flagged this session's failing "National rail status" MCP connector
(HTTP 502) as possibly evidence of an existing broken production
deployment. The user has since confirmed that connector **is expected to
be down right now** — it is not a signal of anything, and this design
does not treat it as one. It is dropped from consideration entirely, not
revisited below.

## Goal

Design a concrete, buildable shape for a *derived* train-mcp — an MCP
server exposing UK rail tools to AI assistants, sourcing as much of its
data as sensibly possible from Distant Signal's (DS's) own REST API
rather than duplicating DS's own live-status/incident ingestion — and
work out, in real implementation-ready detail, the one capability the
research doc identified as the standout, buildable-now, high-value
finding: **delay-aware journey planning**, where a schedule-based plan's
legs get annotated with DS's live line-status/incident data. This spec
also nails down the near-drop-in `resolve_station` migration, makes an
explicit scope call on the board tools, and sketches deployment. It does
not design or approve `find_services`/`plan_journey`'s own CIF/timetable
engine (unchanged, per the research doc's own recommendation) or any
change to DS's ingestion pipelines.

## Corrections / relationship to prior specs

Following this repo's established "Corrections" precedent
(`2026-09-01-tracked-trains-home-page-design.md`'s "Corrections /
relationship to prior specs" section): two decisions in this document are
being knowingly reversed here, not overlooked or quietly patched —
**Decision 4 (Auth)** and **Decision 6 (Deployment)**, both replaced in
place below rather than left as stale text next to a note. This section
states what changed and why; the revised decisions carry the real
reasoning and citations.

**What changed.** This design's original Decision 4 chose *"Option 1: keep
train-mcp's existing Discord OAuth as the gate on who may call the MCP
server at all; call DS's public API as a plain anonymous HTTP client — no
session, no shared identity"* — specifically because every capability this
spec needed worked over DS's already-unauthenticated `/public/*` and
`/Line/.../Status` routes, and adopting DS's own OIDC SSO was judged *"a
real scope increase... unjustified by what this spec actually needs."* Its
original Decision 6 chose a `ClusterIP`-only `Service` with *"no external
ingress designed here."* Both are now real, merged, shipped chart state,
not just this document's proposal —
`charts/distant-signal/templates/railmcp-deployment.yaml`,
`railmcp-service.yaml` (`type: ClusterIP`, its own top comment: *"no
external Ingress/TLS is sketched here, matching Decision 6's own shallow
deployment depth"*), and `values.yaml:772` (`railMcp.enabled: false`) —
all read directly this session, confirming the original decisions actually
shipped as designed.

**Why it's being reversed.** The user has explicitly directed the
opposite shape: the derived MCP service should be publicly reachable, with
access gated by **Distant Signal's own user-authentication system**
(`crates/api/src/auth.rs`'s existing OIDC-SSO-backed session mechanism,
`AuthenticatedUser`) rather than by train-mcp's own, DS-unrelated Discord
allowlist. This is not a walk-back of the original reasoning — Option 1
was and remains the right call for "an internal, opt-in convenience,
reachable only inside the cluster or by whoever the operator's Discord
allowlist admits." It is a real, deliberate change in what's being asked
for: from that, to "a service DS's own users can point their own AI
assistant at directly, using the identity they already have with this
app." The load-bearing research behind the revised decisions below — MCP's
actual current remote-server auth requirements, Authentik's actual current
Dynamic Client Registration support — is new to this pass, cited to what
was fetched this session, not carried over from either prior document's
own citations (the research doc explicitly left DS's OIDC provider's DCR
support unchecked; that gap is closed below, since it's now load-bearing
rather than hypothetical).

**What does not change.** Decisions 1, 2, and 5 (where the derived
service's code lives, the `resolve_station` shim, and the board-tools
scope call) are structural and orthogonal to who is allowed to call the
service or how it's exposed — none of that reasoning is affected by this
reversal, and none of it is touched below. Decision 3's core leg-matching
algorithm (3a–3d) is unchanged; only §3b.6's conclusion about the
TRUST-corroboration tier's reachability is revised, because that
conclusion was explicitly conditioned on the original Decision 4's now-
superseded auth posture, not because the matching logic itself changed.

## Current relevant state (verified 2026-09-01)

Everything below was re-read directly this session (not trusted from the
research doc's citations alone), because this design leans on it for a
load-bearing decision.

### DS's route surface (`crates/api`)

- **`GET /public/stations?q=`** (`crates/api/src/routes/reference.rs:18-42`)
  — unauthenticated type-ahead, `SUGGESTION_LIMIT = 20`
  (`reference.rs:16`), delegates to
  `crates/api/src/data/reference.rs::search_stations`. That function's
  query is `SELECT crs AS code, name FROM stations WHERE crs ILIKE $1 OR
  name ILIKE $1 ORDER BY name LIMIT $2` (`data/reference.rs:23-35`), and
  `Suggestion` is confirmed, by reading it directly this session, to be
  **exactly** `{ code: String, name: String }` (`data/reference.rs:15-18`)
  — no `tiploc` field, no match-type/confidence field, no signal that a
  result set was truncated. This resolves the research doc's Open
  Question 2 definitively: DS's `Suggestion` does **not** carry what
  train-mcp's own tool contract expects.
- **`crates/api/src/routes/mod.rs::public_router()`** (`mod.rs:20-51`)
  merges `health`, `freshness`, `history_retention`, `incidents`, `lines`,
  `preferences`, `reference`, `auth` — all nested under `/public` in
  `main.rs:60` — and is confirmed unauthenticated: no
  `require_internal_token` layer is applied to it anywhere (that layer is
  applied only inside `private_router()`, confirmed by reading
  `crates/api/src/auth.rs:1-6,18-34`'s own doc comment: "Applied only to
  `private_router()` — `public_router()` never sees this"), and `main.rs`
  applies no other auth middleware ahead of the merge
  (`main.rs:57-88` shows only `metrics_layer`/`cors`/`TraceLayer`, no auth
  layer, wrapping the whole router).
- **`crates/api/src/routes/line_status.rs`**'s five TfL-shaped routes
  (`/Line/Mode/{mode}/Status`, `/Line/{ids}/Status`,
  `/StopPoint/{crs}/Disruption`, `/Line/{id}/Status/{from}/to/{to}`,
  `/Line/{id}/Stats/{from}/to/{to}` — `line_status.rs:36-43`) are merged
  directly onto the top-level router (`main.rs:58`), unprefixed, also with
  no `require_internal_token` layer. Handlers take
  `OptionalAuthenticatedUser` (`line_status.rs:31`, e.g. `get_line_status`
  at line 176-180) purely to decide whether to include the caller's own
  *private custom lines* (`filter_private_custom_rows`,
  `line_status.rs:90-113` — "`user` is `None` for an anonymous caller —
  every custom-line row is dropped for them"); catalogue-line rows are
  never gated by this. **Confirmed: fully usable as a plain anonymous HTTP
  client for catalogue-line data**, matching the research doc's Auth
  §3 sketch.
- **`crates/api/src/render.rs::to_tfl_shape`/`status_to_json`**
  (`render.rs:14-88`, read in full) is the actual JSON `GET
  /Line/.../Status` returns: `statusSeverity` (int),
  `statusSeverityDescription`, `reason`, `dataQuality`, `validityPeriods`,
  and, when present, `sampleStats: {total, delayed, cancelled, skipped,
  avgDelayMinutes}` (`render.rs:62-70`). With `?detail=true`, it also adds
  `disruption: {category, description, affectedStops, affectedRoutes:
  [{from, to}], source}` (`render.rs:72-85`) — `affectedStops` and
  `affectedRoutes[].{from,to}` are CRS codes (`common::Disruption`,
  `crates/common/src/lib.rs:310-321`: `affected_stops: Vec<String>`,
  `affected_routes: Vec<AffectedRoute>`, `source: Option<String>` e.g.
  `"knowledgebase-incident-12345"`).
- **`GET /public/incidents/{incidentId}`**
  (`crates/api/src/routes/incidents.rs:17-19`) is a separate, deeper read
  of one Knowledgebase incident's full detail (description, every
  validity period, change history) — unauthenticated, same
  `public_router()` pattern. Its own doc comment (`incidents.rs:1-7`)
  states every field it returns is already reachable via `GET
  /Line/{ids}/Status?detail=true`; this design treats it as an **optional
  deeper fetch**, not a required one — see Decision 3 below.
- **`GET /public/lines`** (`crates/api/src/routes/lines.rs::list_lines`,
  unauthenticated) returns, per catalogue line, `{id, name, category,
  operators, source}` (`LineSummary`, `lines.rs:35-42`). **`GET
  /public/lines/{id}/definition`** (`lines.rs:78-93`, unauthenticated via
  `OptionalAuthenticatedUser` for catalogue ids specifically — "a catalogue
  id is a completely valid, sessionless request", `lines.rs:96-98`)
  returns `{stations: Vec<crs>, operators: Vec<String>}`
  (`LineDefinitionSummary`, `lines.rs:78-81`) for a catalogue line, sourced
  from `app.config.lines` (i.e. the `lines/*.toml` catalogue). This is the
  concrete data DS's own line catalogue exposes for matching an
  operator+station pair to a line (see Decision 2).
- **A real, documented example of the matching ambiguity this design has
  to handle**: `lines/c2c.toml`'s own comment block records that CRS
  `BKG` (Barking) appears in both `c2c.toml` and
  `overground-suffragette.toml`, and CRS `UPM` (Upminster) appears in both
  `c2c.toml` and `overground-liberty.toml` — "a *station-level* overlap
  only, not a shared trunk" (`lines/c2c.toml`, read directly). Two
  different operators serving the same station is exactly the situation a
  naive "match by station membership alone" approach would get wrong; the
  file itself is proof this isn't a hypothetical edge case in DS's
  catalogue.
- **`/Train/*`** (`crates/api/src/routes/train.rs`, read in full):
  mounted unprefixed (`main.rs:59`), every route except pin creation takes
  `AuthenticatedUser` and ownership-checks. `get_by_uid_and_date`
  (`train.rs:219-237`) — the route that would answer "is this specific
  `(uid, date)` currently tracked, and what does TRUST say about it" —
  requires a real session and 404s for anyone but the pin's owner
  (`train.rs:231-234`). `TrackedTrainState`
  (`crates/api/src/data/train_tracking.rs:293-308`, re-read this session,
  matches the research doc's citation exactly) carries `status`,
  `last_reported_location`, `last_event_type`, `delay_minutes`,
  `next_calling_point`, `eta_next`, `eta_source`. `eta_blend.rs`'s own doc
  comment (`crates/api/src/data/eta_blend.rs:1-7`, re-read) confirms this
  cross-source correlation is "deliberately NOT a guaranteed join" —
  precedent cited in Decision 4 below.

### train-mcp's own tool/data shapes (`train-mcp.zip`, re-extracted this
session to `/tmp/claude-1000/.../scratchpad/train-mcp-extract/`, outside
this git worktree, `.env`/`.env.prod` excluded from extraction and not
opened, per the research doc's own handling)

- **`resolve_station`**'s real Zod schema
  (`src/tools/resolve-station.ts:5-30`, read in full): input `{query,
  limit?}`; output `{matches: [{crs, name, tiploc, matchType: 'exact_crs'
  | 'exact_name' | 'alias' | 'substring' | 'fuzzy'}], truncated: boolean}`.
  `truncated` is computed by asking the resolver for one more result than
  requested (`resolve-station.ts:38-39`) specifically so the tool never
  silently drops candidates — "the exact false certainty this tool exists
  to avoid" (the file's own description string, `resolve-station.ts:18`).
- **A `TrainLeg`** in train-mcp's planner
  (`src/timetable/plan/csa.ts:12-26`, read in full) carries `scheduleId`,
  `sourceScheduleId`, `uid`, `fromTiploc`, `toTiploc`, `departure`/
  `arrival` (`HH:MM`), `departureMin`/`arrivalMin`. It deliberately does
  **not** carry operator or headcode — that's enriched separately.
- **`JourneyLegDetails`**
  (`src/timetable/store/query.ts:134-144`, read in full) is exactly that
  enrichment: `{operator: string | null, identity: string | null
  (headcode), fromName, toName, fromPlatform, toPlatform, originTiploc,
  originName, destinationTiploc, destinationName}`. `operator` is a TOC
  code (e.g. `"GW"`, `"SW"`) — the same code family as DS's
  `LineSummary.operators`/`LineDefinitionSummary.operators`.
- **TIPLOC→CRS resolution already exists inside train-mcp's own store**:
  `store.crsForTiploc(tiploc)` (referenced at
  `src/timetable/plan/csa.ts:278`, `src/timetable/plan/interchange.ts:113`)
  — so turning a leg's `fromTiploc`/`toTiploc` into CRS codes for matching
  against DS's line definitions requires no new capability on train-mcp's
  side, just a call already made elsewhere in its own code.
- **The live-board horizon** train-mcp's own Phase 1 design doc measured:
  "about 239 minutes — just under four hours"
  (cited by the research doc from
  `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:340-341`,
  train-mcp archive — not re-opened this session, taken as-is from the
  research doc since it's a measurement about Darwin's own API, not about
  DS).
- **Testing convention**: `test/*.test.ts` at the repo root (not
  colocated with `src/`), run via `vitest run` (`package.json:14-15`, read
  this session).

### This repo's deployment conventions (`charts/distant-signal/`,
`docker-compose.yml`)

- `charts/distant-signal/templates/` (`ls`, read this session) follows a
  one-component-per-file pattern for Rust-workspace services
  (`aggregator-deployment.yaml`, `enricher-deployment.yaml`,
  `trust-consumer-deployment.yaml`, `poller-deployments.yaml`, `api-*.yaml`,
  `frontend-*.yaml`) plus one pattern for a **non-Rust, externally-image
  service**: `schedulefeed-*.yaml`, whose own top comment
  (`schedulefeed-deployment.yaml:1-8`) describes it as pairing "an SFTP
  receiver (the `sftp` container, a third-party image DTD pushes files
  into)" with this app's own `crates/schedule-ingest` container. The
  `sftp` container's image is a configurable `repository`/`tag` pair in
  `values.yaml` (`values.yaml:629-632` area, read this session), the same
  shape `devauthentik-server-deployment.yaml` uses for
  `ghcr.io/goauthentik/server:2026.8.0` (`values.yaml:409-415`) — an
  externally-built, externally-versioned image this repo's own CI does not
  build, referenced by full registry path. By contrast every
  Rust-workspace service uses a bare `distant-signal/<crate>` repository
  name with an empty default `tag` (`values.yaml:146-148, 285-288,
  476-478, 528-530, 588-590` etc.), filled in by this repo's own CI/CD.
- `docker-compose.yml`'s service list (`grep -n '^  [a-z-]*:'
  docker-compose.yml`, read this session): `postgres`, `redis`, `api`,
  four `poller-*`, `aggregator`, `enricher`, `trust-consumer`,
  `schedule-feed-volume-permissions`, `schedule-sftp`, `schedule-ingest`,
  `frontend`. `schedule-sftp` is the compose-level sibling of the
  `sftp`-container-in-`schedulefeed-deployment.yaml` pattern above: a
  third-party image, not built from this repo's own Dockerfiles.

## Decisions

### 1. Overall shape: a separate, externally-built TypeScript service (train-mcp's own shape, forked), not a new Rust crate

**Chosen: the derived service stays a fork/derivative of train-mcp's own
codebase, living in its own repository, deployed alongside DS via a new
Helm subchart template and compose service that reference an externally-
built image — mirroring `schedulefeed`'s SFTP container and
`devauthentik-server`'s pattern, not a new `crates/*` member.**

Two shapes were weighed, per the research doc's own sketch (its "Sketch:
where would a derived service live" section):

- **(A) A new Rust crate (`crates/rail-mcp` or similar), MCP-native,
  calling `crates/api/src/data/*` in-process or over the internal-token
  pattern.** Rejected. The forcing fact is scope, not language
  preference: `find_services`/`plan_journey` — two of train-mcp's six
  tools — are explicitly **not** being migrated by this spec (see
  Decision 5 and Explicitly out of scope), because the research doc found
  DS has no queryable timetable store at all and building one is
  "comparable in scope to train-mcp's own multi-week Phase 2a/2b
  investment, already done once." That CIF-parsing/STP-resolution/
  RAPTOR/CSA engine (26,848 schedules, 316,362 calling points on a
  measured weekday, per the research doc) is real, tested TypeScript code
  that a Rust crate cannot reuse without a full, independent reimplementation
  — which is precisely the multi-week duplication this spec's own scope
  (Decision 5) declines to take on. A Rust crate would therefore either
  (a) not implement two of the six tools at all, leaving the MCP server's
  tool surface incomplete relative to what exists today, or (b) itself
  make outbound calls to the *existing* TypeScript service for just those
  two tools — two MCP-adjacent services cooperating across a process and
  language boundary for no benefit over just keeping one service. Neither
  is better than shape (B).
- **(B) Keep it TypeScript, train-mcp's own architecture, calling DS's
  public API as an HTTP client for the parts DS can serve.** **Chosen.**
  train-mcp's own layering already isolates its Darwin client behind a
  plain typed "Domain" boundary (the research doc's citation,
  `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:37-49`,
  train-mcp archive) — swapping what backs `resolve_station` and adding a
  new DS-backed annotation step to `plan_journey` (Decisions 2 and 3
  below) touches that boundary without touching the CIF store, the
  planner, or the other four tools' existing code at all. This keeps
  train-mcp's already-substantial, already-tested investment intact.

**Where the derived service's *code* lives is a separate question from
where its *deployment* lives, and the two get different answers.** The
service's source stays in its own repository (a fork/derivative of
train-mcp, not vendored into this repo) — importing an entire second
language's tests, tooling, and CI matrix (`test/*.test.ts`, `vitest`,
whatever train-mcp's own CI does) into a Rust-workspace-centric repo whose
own CI already spans Rust (`crates/`) and one Next.js app (`frontend/`)
would grow this repo's own build surface for a service this repo's CI
doesn't need to build — this repo doesn't build `ghcr.io/goauthentik/server`
either, and that's the closest existing precedent for "a service this
deployment depends on but doesn't own the source of." Its **deployment**
artifacts (a new Helm subchart template, a new compose service), by
contrast, belong in this repo, following `schedulefeed-*.yaml`'s exact
precedent for a non-Rust, externally-built, externally-versioned
component: reference its image by full registry path
(`repository`/`tag` in `values.yaml`, no bare `distant-signal/<name>`
short name, since this repo's own CI does not build or publish it) — see
Decision 6 for the concrete sketch.

This does **not** contradict the task's framing of "a first-class feature
of this repo" — first-class here means *this repo's deployment story
includes and manages it* (chart, compose, values, documented in this
spec), the same way authentik (an entirely separate upstream project) is
already a first-class, chart-managed part of this deployment without its
source living here.

### 2. `resolve_station` migration: swap the HTTP call, keep a compatibility shim — not a straight drop-in

**Chosen: `resolve_station`'s Domain-layer implementation
(`src/stations/resolve.ts` in train-mcp, not re-read in full this session
— out of scope to re-derive, the research doc's characterization of it as
a "cascade" is accepted) is replaced with a call to `GET
/public/stations?q=<query>&limit=<n>`, followed by a translation step that
fabricates the two fields DS's `Suggestion` doesn't carry.**

This is **not** a straight swap, confirmed by the re-verification above:
train-mcp's tool contract requires `tiploc` and `matchType` per match
(`resolve-station.ts:5-10`); DS's `Suggestion` is `{code, name}` only
(`data/reference.rs:15-18`). Two real options for the gap:

- **Widen DS's `Suggestion`/`search_stations` to add `tiploc` and a
  match-confidence field.** Considered. This is a small, additive change
  to DS's own query (`stations` table would need a `tiploc` column
  exposed, and `search_stations`'s single `ILIKE` pattern would need to
  be split into tiered queries — exact CRS match, exact name match,
  substring — to produce a genuine `matchType`, not just relabel a flat
  `ILIKE` result as one confidence level). **Rejected for this pass**: it
  is a change to DS's own Rust code, which this spec's brief scopes to
  "a new derived MCP service," not a DS backend change; more importantly,
  it couples the shim's correctness to DS's schema evolving in a specific
  way DS has no independent reason to want (DS's own frontend
  `frontend/lib/suggestions.ts` doesn't need `tiploc` or `matchType` —
  the research doc confirms this, and this session's re-read of
  `reference.rs`/`data/reference.rs` found nothing that would benefit from
  either field for DS's own purposes). Noted as a possible future
  follow-up, not designed further here.
- **A translation shim inside the derived service.** **Chosen.**
  `tiploc`: looked up locally from train-mcp's own already-bundled
  `stations.json` CORPUS extract (the same reference data
  `resolveStation`'s cascade already consults, per the research doc's
  citation of `docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:37-49`'s
  "Reference" layer) by CRS code returned from DS — a pure local join, no
  second network call. `matchType`: DS's single `ILIKE`-based query gives
  no tiered confidence signal at all, so the shim computes it **client-side**
  from the already-known `query` string against each returned `code`/`name`:
  `code.toUpperCase() === query.trim().toUpperCase()` → `exact_crs`;
  case-insensitive exact `name` match → `exact_name`; otherwise
  `substring` (DS's query is always a substring match by construction,
  per `data/reference.rs:23-35`'s `%q%` pattern) — `alias` and `fuzzy`
  are never produced by this shim, since DS's query has no alias table
  and no fuzzy/edit-distance matching to report confidence for; downgrading
  to the closest honest category (`substring`) rather than fabricating
  `alias`/`fuzzy` is the same "never fabricate false certainty" posture
  `resolve-station.ts:18`'s own description commits to.
  `truncated`: DS's route has no over-fetch signal (`SUGGESTION_LIMIT =
  20` is a hard `LIMIT`, `reference.rs:16,38`) — the shim requests
  `limit + 1` from DS (mirroring train-mcp's own existing over-fetch
  trick, `resolve-station.ts:38-39`) and sets `truncated = results.length
  > limit` before slicing, exactly reproducing the existing behavior
  against the new upstream.

**Verdict, restated precisely (correcting the research doc's
looser "cheap" framing): cheap engineering, but not literally a drop-in
— it is a real, if small, translation layer, not a bare URL swap.**

### 3. Delay-aware journey planning: DS-sourced per-leg annotation, layered onto (not replacing) train-mcp's own live-board check

The research doc's highest-priority recommendation is annotating
`plan_journey`'s output legs with live delay/cancellation status. It
sketched this as extending train-mcp's own Darwin board check
(Phase 2c) and separately noted DS's TRUST data would add
"corroboration... for the subset of legs DS happens to be tracking, not a
capability unavailable without DS." This spec designs the DS-sourced side
concretely, as the layer this task asked for; train-mcp's own Darwin-board
check (no DS dependency at all) is retained unmodified as the foundational
mechanism and is not re-designed here — see Explicitly out of scope.

#### 3a. What gets annotated, and when

For each `TrainLeg` (`kind: 'train'`) in a candidate plan whose
`departure` falls inside the live-board horizon (train-mcp's own existing
~239-minute figure — reused, not re-derived), the derived service
attempts a DS-sourced annotation as a **best-effort addition**, never a
precondition for returning the plan. `TransferLeg`s (walk/tube/bus/ferry
hops, `kind: 'transfer'`, `src/timetable/plan/csa.ts:41-51`) are never
annotated — they carry no `uid`/operator, and DS has no data source that
applies to a hop with no train on it.

#### 3b. The leg → DS-line matching problem

A `TrainLeg` is train-shaped (`uid`, `operator` from `JourneyLegDetails`,
`fromTiploc`/`toTiploc`); DS's line-status data is line-shaped (one row
per curated catalogue line, e.g. `"c2c"`, `"thameslink-core"`). Resolving
one to the other is a real matching problem, not a lookup, and this
design treats false-confidence the same way `resolve_station` already
does — never guess past what the data actually supports.

**Algorithm:**

1. **Resolve CRS.** Convert the leg's `fromTiploc`/`toTiploc` to CRS
   codes via `store.crsForTiploc()` — already exists in train-mcp, no new
   capability (Current relevant state, above).
2. **Candidate lines by operator.** Fetch (and cache — see 3d)
   `GET /public/lines`; filter to catalogue lines (`source` not `"tfl"`,
   `"custom-"`-prefixed rows never appear here since they're private —
   `list_lines`'s own doc comment) whose `operators` array contains the
   leg's TOC code. Zero candidates → `matchConfidence: 'none'`, stop.
3. **Narrow by station membership.** For each candidate, fetch (cached)
   `GET /public/lines/{id}/definition` and check whether the leg's origin
   CRS and destination CRS are members of `stations`.
   - **Both CRS present, exactly one candidate line** →
     `matchConfidence: 'operatorAndBothStations'`, the confident case.
   - **Both CRS present, more than one candidate line** (the
     `lines/c2c.toml` Barking/Upminster situation, confirmed real in
     Current relevant state) → cannot pick one without guessing which
     operator's own route the leg actually ran on when both serve the
     same pair of stations; report `matchConfidence: 'ambiguous'` with
     `candidateLineIds: string[]` listing all of them, rather than
     silently picking one — same posture `resolve_station` already takes
     for ambiguous names ("ask... rather than assuming",
     `resolve-station.ts:18`).
   - **Only one CRS present** (a through-service whose other end DS's
     catalogue line definition doesn't reach, e.g. a leg that continues
     past a branch line's defined extent) →
     `matchConfidence: 'operatorAndOneStation'`, attached with the single
     matching line but flagged as weaker.
   - **Operator matches but neither CRS is a member of any candidate's
     station list** → `matchConfidence: 'none'`.
4. **Fetch status.** For a confident or weak match, `GET
   /Line/{id}/Status?detail=true` (single line — cheaper than the
   `/Line/Mode/{mode}/Status` bulk form for one leg; see 3d for the
   whole-itinerary case) for the matched line id(s).
5. **Scope-precision check, using data already in hand — no second
   incident fetch required.** `?detail=true`'s own `disruption.affectedStops`/
   `affectedRoutes[].{from,to}` (confirmed real fields, `render.rs:72-85`,
   `common::Disruption`, `crates/common/src/lib.rs:310-321`) are checked
   against the leg's own origin/destination CRS. A line-wide severity
   driven by an incident on a different branch (again, exactly what
   `lines/c2c.toml`'s documented overlap makes plausible for an adjacent
   line) is visible directly from these fields — a line's *overall*
   severity is attached regardless, but the annotation additionally
   reports whether the leg's own two stations are inside the disruption's
   named scope, so a consuming LLM (or a human reading the tool's
   rendered text) isn't misled into thinking "the whole line is down" when
   only an unrelated segment is. `GET /public/incidents/{incidentId}`
   (`disruption.source`) is available as an **optional** deeper fetch for
   a fuller incident narrative (full validity history) but is not required
   for this check — its own doc comment already states its fields
   duplicate what `?detail=true` returns (`incidents.rs:1-7`).
6. **Individual-train (TRUST) corroboration — reachable as a general
   per-request mechanism under the revised Decision 4, not just a narrow,
   opportunistic case.** *(Revised by the Corrections above — this bullet
   originally concluded the opposite.)* If (and only if) the leg's `(uid,
   date)` corresponds to a train the *calling identity* has separately
   pinned in DS via `POST /Train/track`, `GET
   /Train/by-uid/{train_uid}/{date}` (`train.rs:219-237`) returns the
   richer `TrackedTrainState` (`status`, `last_reported_location`,
   `delay_minutes`, `eta_next`). **This path requires a real DS session**
   (`AuthenticatedUser`, `train.rs:221`) — under the *original* Decision 4
   (the derived service calling DS anonymously), the derived service held
   no DS session at all, so this tier was reachable only in the narrow
   case of a human who happened to independently, separately, already hold
   one. **Under the revised Decision 4 below, this blocker is resolved by
   construction**: the adapter auth layer in front of the derived service
   only ever completes a `plan_journey` call for a caller who has already
   gone through DS's own OIDC login (that's now the gate on calling the
   MCP server at all), and holds that caller's real DS session server-side
   for exactly this purpose. So for *every* `plan_journey` call, not just
   an opportunistic subset, step 6 attaches the caller's own held DS
   session to `GET /Train/by-uid/{uid}/{date}` and gets a real answer
   whenever `(uid, date)` happens to be a train that specific person has
   tracked. **The remaining scoping is inherent to the feature, not an
   auth limitation**: this tier still only ever fires for the subset of
   legs that happen to already be a DS-tracked train for the calling
   identity — that's what "corroboration from a train you're personally
   tracking" means — but it is no longer gated behind a second,
   independent precondition (the derived service separately holding a
   session at all) the way it was before.

#### 3c. Output schema

A concrete addition to a rendered `TrainLeg`'s `structuredContent`,
following `resolve_station`'s own precedent (a typed `outputSchema` field
alongside rendered text, `resolve-station.ts:23-30`):

```ts
liveStatus?: {
  matchConfidence: 'operatorAndBothStations' | 'operatorAndOneStation'
                  | 'ambiguous' | 'none',
  lineId?: string,                 // set when matchConfidence isn't 'none'/'ambiguous'
  candidateLineIds?: string[],     // set only when matchConfidence === 'ambiguous'
  severity?: string,               // Severity variant name, e.g. "MinorDelays"
  reason?: string,                 // LineStatus.reason, verbatim
  dataQuality?: 'knowledgebase' | 'ldbwsInferred' | 'trustInferred'
              | 'planned' | 'tfl',   // DS's own common::DataQuality, passed through unchanged
  sampleStats?: { total: number, delayed: number, cancelled: number,
                  skipped: number, avgDelayMinutes: number },
  legInDisruptionScope?: boolean,  // computed from disruption.affectedStops/affectedRoutes
                                    // vs. this leg's own origin/destination CRS; absent
                                    // when there's no active disruption to check against
  trackedTrainState?: {            // present only for the narrow 3b.6 case
    status: string | null, lastReportedLocation: string | null,
    delayMinutes: number | null, etaNext: string | null,
  },
  asOf: string,                    // DS's own computed_at for the matched line row
  source: 'ds_line_status' | 'ds_tracked_train' | 'unavailable',
}
```

`source: 'unavailable'` (see Error handling) covers a DS API failure —
distinct from `matchConfidence: 'none'`, which means DS answered
successfully but no line could be matched.

#### 3d. Caching, to keep the annotation loop cheap

DS's line catalogue is small (on the order of the `lines/*.toml` count —
not measured this session, matching the research doc's own Open Question
4, which this design does not resolve either) and changes rarely (a
config-file catalogue, not per-request data). The derived service caches
`GET /public/lines` and every catalogue line's `GET
/public/lines/{id}/definition` at startup and refreshes on an interval
(candidate: every 15 minutes — an arbitrary, unresearched starting figure,
flagged in Open questions/risks, same posture this repo's own specs take
for unresearched constants like `MINE_LIST_LIMIT`). Only the per-line
**status** fetch (step 4) happens per `plan_journey` call, and only for
lines actually matched by legs in that specific itinerary — not the whole
catalogue.

### 4. Auth: DS's own OIDC SSO becomes the gate, via an adapter authorization-server layer in front of the derived service — reversed from the original anonymous-client choice (see Corrections above)

**This decision is a full reversal, not a tweak, of the original Decision
4** (which chose Option 1: train-mcp's own Discord OAuth as the incoming
gate, plain anonymous HTTP calls to DS). Per the Corrections section, the
user has directed that the derived service be publicly reachable and
gated behind DS's own user auth instead. This section works out
concretely what that requires, using this session's own research into
MCP's real remote-server auth spec and Authentik's real DCR support — not
invented.

#### 4a. What MCP actually requires of a remote server's auth (researched this session)

Fetched directly from the official spec, `modelcontextprotocol.io/
specification/2025-06-18/basic/authorization` (2026-09-01):

- Authorization is **OPTIONAL** for MCP in general, but *"implementations
  using an HTTP-based transport SHOULD conform to this specification"* —
  this design's own server is exactly that case (a publicly reachable HTTP
  transport), so it's the specification this design has to meet, not an
  optional extra.
- The spec is built on four real IETF specs, used together: **OAuth 2.1**
  (`draft-ietf-oauth-v2-1-13`), **OAuth 2.0 Authorization Server Metadata**
  (RFC 8414), **OAuth 2.0 Dynamic Client Registration Protocol** (RFC
  7591), and **OAuth 2.0 Protected Resource Metadata** (RFC 9728).
- Three roles: the **MCP server** acts as the OAuth 2.1 **resource
  server**; the **MCP client** (Claude.ai, Claude Desktop, or any other
  MCP-speaking assistant a user points at this server) acts as the OAuth
  **client**; the **authorization server** issues tokens and *"may be
  hosted with the resource server or a separate entity"* — its own
  implementation details are explicitly *"beyond the scope of this
  specification."* This matters directly below: the spec does not require
  DS's own Authentik instance to be the authorization server MCP clients
  talk to; it only requires *some* OAuth-2.1-compliant AS to exist, and
  it's this design's call which entity plays that role.
- **MUST** requirements that bind concretely: the MCP server **MUST**
  implement RFC 9728 Protected Resource Metadata
  (`/.well-known/oauth-protected-resource`) and **MUST** return it via a
  `WWW-Authenticate` header on a `401`; the authorization server **MUST**
  provide RFC 8414 metadata; the client **MUST** implement PKCE (OAuth 2.1
  §7.5.2); the client **MUST** send an `Authorization: Bearer <token>`
  header on every request (never a query string, never a raw cookie); the
  server **MUST** validate the token's audience was issued specifically
  for it (RFC 8707 resource indicators) and **MUST NOT** accept or pass
  through a token issued for some other resource.
- **Dynamic Client Registration is a SHOULD, not a MUST**: *"MCP clients
  and authorization servers SHOULD support the OAuth 2.0 Dynamic Client
  Registration Protocol (RFC 7591)... Any authorization servers that do
  not support Dynamic Client Registration need to provide alternative
  ways to obtain a client ID... 1. Hardcode a client ID... specifically
  for the MCP client to use... or 2. Present a UI to users that allows
  them to enter these details, after registering an OAuth client
  themselves."* The spec explicitly anticipates and designs around
  authorization servers that don't cleanly support open DCR — this is not
  a hypothetical edge case this design has to invent a fallback for from
  scratch.

#### 4b. Does Authentik (DS's own OIDC provider) support DCR? Researched, resolved concretely — yes, but not the *open* flavor MCP clients need at first contact

Fetched directly from Authentik's own docs this session
(`docs.goauthentik.io/add-secure-apps/providers/oauth2/
dynamic-client-registration/` and `docs.goauthentik.io/releases/2026.8/`,
2026-09-01) — this resolves the open question the embedded-chatbot
research doc explicitly left unchecked, now load-bearing rather than
hypothetical:

- **Authentik does implement RFC 7591 DCR, introduced in release
  2026.8.0.** Notably, `charts/distant-signal/values.yaml:415` already
  pins `devAuthentik.image.tag: "2026.8.0"` for this chart's opt-in local
  dev IdP — the exact release that shipped DCR, a real coincidence worth
  flagging, but not proof a given *production* deployment's `api.sso.
  issuerUrl` (`values.yaml:311-315`, any OIDC-compliant issuer an operator
  points at, unversioned and unpinned by this chart at all) runs 2026.8.0
  or later. Whether a specific deployment's real, production IdP is new
  enough is an operator-environment fact this design cannot assert.
- **Critically, Authentik's DCR is gated, not open registration.** Its own
  docs state plainly: *"The client authenticates to the registration
  endpoint using a Bearer access token that includes the required DCR
  scope"* and *"A successful registration is therefore not an anonymous or
  open registration unless the configured policies explicitly allow that
  behavior."* A registering client must already hold a valid bearer token
  carrying Authentik's own `goauthentik.io/oidc/dcr` scope before it can
  call `/register` at all — and each successful registration mints a
  brand-new Application+Provider pair inside Authentik, one per
  registering client, evaluated against admin-configured policy bindings.
- **This breaks the exact flow MCP clients need DCR for.** The whole
  point of DCR, per the spec quote above, is a client with *no prior
  relationship to this authorization server* self-registering *before any
  user has authenticated anywhere* — that's what "Claude.ai discovers a
  new remote MCP server and connects" looks like on first contact. A
  first-contact MCP client has no bearer token to present to Authentik's
  DCR endpoint, so it cannot get through Authentik's gate at the moment
  it would need to. Authentik's DCR is real and correctly RFC-7591-shaped,
  but it's designed for an already-authenticated caller provisioning a new
  *downstream* application (its own original feature request was
  multi-tenant SaaS apps self-provisioning), not for an anonymous MCP
  client's zero-touch first handshake.

#### 4c. Three real options, weighed

- **(a) Delegate entirely: point MCP clients' DCR/AS-discovery directly at
  DS's Authentik.** **Rejected as the primary mechanism**, precisely
  because of 4b: an unauthenticated first-contact MCP client cannot
  complete Authentik's own DCR flow, so "delegate entirely" doesn't
  actually satisfy what MCP clients expect from an AS that advertises DCR
  support. Even setting that aside, Authentik's one-Application-
  per-registration model doesn't fit this chart's existing provisioning
  story either: DS's real production SSO client and the dev-only
  `devAuthentik` client are both **one fixed, blueprint/operator-
  provisioned application** (`values.yaml:311-330`,
  `charts/distant-signal/files/devauthentik-blueprints/
  oauth2-client.yaml`), not a pattern built for accumulating one
  throwaway Application+Provider per connecting AI assistant instance.
- **(b) An intermediate adapter auth layer, in front of the derived
  service.** **Chosen** — detailed in 4d below.
- **(c) The MCP server validates a raw DS session cookie/token a user's
  own browser-based MCP client presents directly, reusing
  `AuthenticatedUser`'s exact mechanism with no new machinery.**
  **Rejected as the MCP-client-facing mechanism**, for two concrete
  reasons: it isn't spec-compliant (4a's MUST requirements are an
  `Authorization: Bearer` header issued by a real AS, not an app-specific
  cookie a human has to manually extract and paste into a third-party AI
  assistant's own credential store), and no mainstream MCP client
  (Claude.ai, Claude Desktop) has a "paste a raw cookie" connection
  mechanism — they implement the OAuth flow in 4a's sequence diagram, not
  an ad hoc alternative. It's also a materially worse blast radius on
  leak: a live `distant_signal_session` cookie is full account access
  (train tracking, custom lines, tickets — everything `AuthenticatedUser`
  gates), stored inside a third-party product DS doesn't control, versus
  a scoped MCP access token. **However — and this is not hand-waved — the
  underlying mechanism is exactly right for a different part of this
  design**: see 4d, where the adapter itself, once *it* has completed a
  real login, holds and reuses `AuthenticatedUser`'s exact cookie-based
  session for its own outbound calls to DS. Option (c) is wrong as the
  MCP-client-facing contract; its plumbing is right as the adapter's
  internal mechanism.

#### 4d. The chosen shape: an adapter that is its own minimal OAuth 2.1 authorization server to MCP clients, backed by DS's existing, unmodified OIDC login for the actual human authentication step

Per 4a, the spec explicitly allows the authorization server role to be
*"hosted with the resource server"* — so the derived service (or a thin
companion component deployed alongside it; not designed to a specific
process boundary here, since that's an implementation-time call, not a
design-time one) plays **both** roles MCP defines, rather than trying to
make DS's Authentik play the AS role directly:

- **Facing MCP clients, it is a real, spec-compliant AS**: serves
  `/.well-known/oauth-protected-resource` (RFC 9728) and
  `/.well-known/oauth-authorization-server` (RFC 8414), implements **its
  own** RFC 7591 DCR endpoint — genuinely open, since the adapter (not
  Authentik) controls this policy and has every reason to allow exactly
  the zero-touch self-registration 4b found Authentik's own DCR doesn't
  give a first-contact client — and issues MCP-scoped Bearer access
  tokens after a real user completes authorization, with PKCE and the
  `resource` parameter handled per 4a's MUSTs.
- **For the actual human-authentication step**, the adapter does not
  reimplement login — it drives DS's own **existing, unmodified** OIDC
  flow (`crates/api/src/auth/oidc.rs`, `routes/auth.rs`), the same
  authorization-code-redirect-callback sequence DS's frontend already uses
  today, via **one statically pre-registered Authentik client** — mirroring
  `api.sso.clientId`/`clientSecret`'s existing static-registration pattern
  (`values.yaml:311-330`), not Authentik's own gated DCR. The human still
  authenticates through their real browser, redirected through Authentik
  exactly as they would logging into DS's own frontend; nothing about
  DS's login UX, Authentik configuration, or `crates/api`'s auth code
  changes.
- **The adapter holds the resulting DS session server-side**, keyed to
  the MCP access token it hands back to the connecting MCP client — the
  MCP client never sees the raw `distant_signal_session` cookie, only its
  own MCP-scoped Bearer token. Every subsequent MCP tool call the derived
  service makes to DS on that user's behalf reuses that held session via
  `AuthenticatedUser`'s exact existing mechanism (Option (c)'s plumbing,
  applied where it actually belongs) — this is what resolves Decision
  3b.6 above: the derived service now genuinely operates *as* the calling
  identity, on every request, not opportunistically.
- **Because the incoming gate is now "has this caller completed DS's own
  login," not a per-tool check, it applies to every tool the derived
  service exposes** — including `get_departures`/`get_arrivals`/
  `get_service_detail` (Decision 5, unchanged, still calling Darwin/LDBWS
  directly, not DS), which had no auth requirement at all under Discord's
  allowlist model beyond being on that allowlist. This is a natural
  consequence of the reversal, not a separate design point requiring its
  own justification: the user directed DS's own auth as the gate on the
  MCP server as a whole.
- **Discord OAuth (train-mcp's own original incoming gate, retained
  unmodified by the original Decision 4) is superseded, not stacked
  alongside DS's OIDC.** The already-shipped `railMcp.discord.*` chart
  values (`values.yaml:790-796`, consumed by `railmcp-deployment.yaml`)
  are not resolved by this document — whether they're retired, repurposed,
  or kept as a secondary gate is a chart-and-implementation-plan-level
  decision, out of scope for a design-document edit per this task's own
  constraints (see Open questions/risks).

This is deliberately not a full protocol-level design of the adapter's own
endpoints (token format, storage of the token→session mapping, refresh
handling) — that belongs in an implementation plan, the same depth this
document already keeps for e.g. Decision 2's shim or Decision 3's caching
sketch, not a from-scratch OAuth server specification.

### 5. Board tools and `get_service_detail`: left entirely alone, no DS work in this spec

Per the research doc's ranked recommendation ("watch, don't build yet"),
and re-affirmed by this design directly: `get_departures`, `get_arrivals`,
and `get_service_detail` keep polling Darwin/LDBWS directly, exactly as
train-mcp does today, with **no new DS route proposed**. Two reasons,
beyond simply deferring to the research doc's ranking:

1. **The delay-aware annotation design above (Decision 3) doesn't need
   DS's `station_samples` at all.** Its live-board check (3a) is
   train-mcp's own existing Darwin client, unmodified; its DS-sourced
   layer (3b) reads line-level/incident data, not station boards. Nothing
   in this spec's actual scope creates pressure to widen `poller-ldbws`.
2. **The research doc's own gap characterization still holds under this
   session's re-verification of `data/reference.rs` and `render.rs`**:
   nothing read this session found a public route over `station_samples`
   for arbitrary stations, and widening `poller-ldbws`'s curated
   sample-station set remains an ongoing RDM-quota/architecture cost, not
   a route addition — this spec has no new information that would change
   that calculus.

If DS's own roadmap ever grows a general "any station's live board"
route for its own frontend (the research doc's own suggestion), the same
route would serve both DS and a future board-tool migration for free — not
designed further here.

### 6. Deployment: `ClusterIP` stays, but is no longer the whole story — a public `Ingress` rule, following this chart's own existing pattern, not a new mechanism

**Reversed from the original Decision 6** (see Corrections above), which
stopped at `ClusterIP` with "no external ingress designed here." Two
things changed the calculus: the user's explicit direction that the
service be publicly reachable, and the fact that — unlike when the
original Decision 6 was written — this chart now genuinely has real
`Ingress` precedent to follow rather than inventing one from scratch.

#### 6a. What's unchanged from the original sketch

`railmcp-deployment.yaml`/`railmcp-service.yaml` (both already shipped,
read directly this session) keep the same shape the original Decision 6
described and this session confirmed matches what's live: one container,
image referenced by full external registry path
(`ghcr.io/CHANGE-ME/distant-signal-mcp`, `values.yaml:774-780`, matching
`devAuthentik.image.repository`'s `ghcr.io/goauthentik/server` precedent),
`DS_API_BASE_URL` pointed at the in-cluster `api` Service, secrets mounted
the way `schedulefeed-secret.yaml` mounts SFTP host-key material. The
`Service` itself (`railmcp-service.yaml`) stays `ClusterIP` — an `Ingress`
rule sits in front of it and routes to it, exactly the same relationship
`ingress.yaml` already has with `frontend-service.yaml`/`api-service.yaml`
today; `ClusterIP` was never wrong, it was just previously the *only*
exposure this chart offered.

#### 6b. Real precedent already exists in this chart — this is not the first `Ingress`

`charts/distant-signal/templates/ingress.yaml` already exists and is not
new: one `Ingress` object, gated by `ingress.enabled`, with per-component
sub-blocks — `ingress.frontend.{enabled, host}` (`enabled` defaults
`true`) and `ingress.api.{enabled, host}` (`enabled` defaults `false`,
carrying its own explicit inline warning: *"SECURITY: enabling this
exposes `/private/*` to the internet as well. Those endpoints are
protected ONLY by the `X-Internal-Token` shared secret... If you do not
need external API access, leave this off"*, `values.yaml:895-903`) — each
rendering its own `host` rule, sharing one `ingress.className`, one
`ingress.annotations` map, and one `ingress.tls` array
(`values.yaml:882-910`). The file's own header comment explains why it's
**separate hostnames, not path-splitting one host**: the api serves
unprefixed, TfL-compatible routes (`/Line/...`) that would collide with
Next.js's own routes or break TfL-client compatibility under path
splitting (`values.yaml:876-881`).

**Chosen: a third sub-block, `ingress.railMcp.{enabled, host}`, and a
matching third rule in `ingress.yaml`, shaped exactly like
`ingress.api`'s** — its own hostname, `enabled` defaulting `false` (not
`ingress.frontend`'s default-`true` shape), same `{{- fail ... }}`-style
guard `ingress.yaml:5-7` already uses to reject `enabled: true` with an
empty `host`. Not path-split under `frontend`'s or `api`'s existing host
either, for the same reasoning `values.yaml:876-881` already gives against
path-splitting generally, plus one MCP-specific reason: the well-known
discovery documents 4a's research found MCP servers **MUST** serve
(`/.well-known/oauth-protected-resource`, `/.well-known/
oauth-authorization-server`) live at a fixed, convention-defined path off
the document root — they shouldn't have to coexist there with either of
the other two services' own top-level route namespaces.

**TLS reuses the existing shared `ingress.tls` array verbatim** — no new
TLS mechanism; same cert-manager-shaped, issuer-agnostic pass-through
already documented at `values.yaml:904-910`.

**The security posture is the inverse of `ingress.api`'s own warning, and
worth stating with the same directness**: `ingress.api`'s comment warns
that route is protected *only* by a shared secret meant for internal
callers. `ingress.railMcp`, by contrast, is safe to expose specifically
*because* Decision 4's adapter auth layer gates every request behind a
real, individual DS user login — not a shared secret at all. That
distinction is the entire justification for why this reversal is
defensible; it should be stated in the chart comment when this is actually
implemented, not left implicit.

**Dependency between the two flags, not designed further here**:
`ingress.railMcp.enabled: true` is meaningless unless `railMcp.enabled:
true` too (the same implicit relationship `ingress.frontend.enabled`
already has with the `frontend` Deployment always being on) — an operator
wanting a public MCP endpoint has to flip both. Enforcing that
relationship (most likely another `{{- fail ... }}` guard, following
`ingress.yaml:1-7`'s existing pattern) is chart-file work, out of scope
for this design-document edit.

#### 6c. `railMcp.enabled` stays `false` by default — more firmly than before, not less

The original Decision 6 defaulted `railMcp.enabled: false` as a plain
"opt-in extra service" call. That reasoning still holds, but the reversal
adds two independent reasons it should stay `false` **more emphatically**,
not be reconsidered now that the feature is more capable:

1. **A public endpoint is a materially bigger default footprint than an
   internal-only one**, even gated behind real auth — every publicly
   reachable service is something an operator has to actively decide to
   expose, not something a chart should default toward for anyone running
   `helm install` without reading the values file closely.
2. **Turning it on now requires real, non-trivial operator setup before
   the service even functions**: a statically registered Authentik client
   for the adapter's own login step (4d), the adapter component itself
   deployed and configured, and (per the Licensing note, revised below) a
   legal sign-off this reversal makes more urgent, not less. This mirrors
   `api.sso.*`'s own posture exactly — `values.yaml:299-310`'s comment
   states SSO is "REQUIRED... no anonymous fallback" and fails the render
   rather than deploy a pod that can't start; `railMcp` should fail the
   same way if enabled without its own prerequisites met, not silently
   degrade to something less than what was asked for.

`railMcp.publicUrl` (`values.yaml:783-789`) already exists in the shipped
chart, already documented as *"the MCP service's own PUBLIC_URL... an
operator enabling railMcp must set this to wherever they expose the
railmcp Service. No Ingress/TLS is sketched by this chart for it"* — i.e.
the shipped chart's own field naming and comment already anticipated
exactly this eventual public-exposure need, even though the original
Decision 6 chose not to build the `Ingress` rule for it yet. This revision
is the natural completion of a gap the chart already flagged in its own
comments, not a new idea grafted on from outside.

#### 6d. Rate-limiting: a genuine new gap this reversal introduces, not solved here

**No rate-limiting infrastructure exists anywhere in this repo** —
confirmed this session by grepping every `.rs`/`.toml`/`.yaml` file for
`rate.?limit`/`governor`/`tower_governor`/`throttle`: zero matches. That
was a defensible gap for a chart with no publicly-reachable, unauthenticated-
by-default surface; a public `Ingress` rule (even one gated by real auth)
changes that calculus, since an authenticated-but-malicious or simply
buggy MCP client can still hammer `plan_journey`'s DS-annotation loop
(Decision 3) at whatever rate its own retry logic chooses.

At the same shallow depth this chart already gives deployment concerns it
doesn't fully design (`networkPolicy.enabled` defaults `false` and is its
own explicitly undesigned opt-in, per its own `values.yaml` comment): the
cheapest available lever is an ingress-controller-level annotation, which
this chart already threads through unmodified via `ingress.annotations`
(`values.yaml:887-889`) — e.g. an nginx-ingress `limit-rps`-style
annotation, requiring no new chart mechanism at all. **But this has a real
limitation, not resolved here**: `ingress.yaml` renders exactly **one**
`Ingress` object shared across all three host rules (`frontend`, `api`,
and now `railMcp`), and `ingress.annotations` applies to that one object
chart-wide — there is no way, in the shipped file's current shape, to
apply a rate-limit annotation to only the `railMcp` host without also
applying it to `frontend`/`api`. Giving each rule its own annotation set
would be a bigger structural change to `ingress.yaml` than this pass's
scope; flagged in Open questions/risks rather than designed around here.

#### 6e. Not the same axis as the parallel internal-service-accounts design — noted, not conflated

`docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md`
(read this session) redesigns **service-to-service** auth — how
`poller-ldbws`, `trust-consumer`, etc. authenticate to `api`'s
`/private/*` routes, moving from one shared `X-Internal-Token` to
per-service tokens with a static, code-defined route-scoping table. That
document's own Decision 4 explicitly declines to share its mechanism with
a future *human*-roles feature, reasoning that service identities are
"static and deploy-time" while human-facing access is "inherently
dynamic" — assignable to an existing identity at runtime, not fixed at
deploy time. **That exact reasoning is why this design doesn't reuse that
primitive either**: a human connecting an MCP client to `distant-signal-
mcp` is precisely the dynamic, per-user case that other document's own
Decision 4 contrasts against its own static table. Decision 4 above
already reflects this — it reuses DS's full user-identity OIDC/session
mechanism, not an `X-Internal-Token`-shaped credential — so there's no
conflation to resolve; this note exists because the task asked the
relationship be stated explicitly, not because the two designs actually
overlap. The one thing genuinely shared between them is a *pattern*, not a
primitive: both this design's adapter (Decision 4) and that document's
Decision 1 (per-service tokens) follow the same "the app that consumes the
credential mints it and tells the caller what it is" posture already
established by `schedulefeed-secret.yaml`'s corrected comment — worth
noting as a shared taste, not a shared mechanism.

`docker-compose.yml`: a new `rail-mcp:` service entry, `depends_on: [api]`,
`environment: DS_API_BASE_URL=http://api:<port>` (the same in-network
hostname every other compose service already uses to reach `api`), image
referenced the same way `schedule-sftp`'s entry references its third-party
image (not a `build:` context pointing into this repo, since the source
doesn't live here) — unchanged by this reversal, since local dev has no
analogous "publicly reachable" concept to begin with.
`docker-compose.dev.yml`: left to a later implementation pass to decide
whether local dev wants a bind-mount of a sibling checkout of the derived
service's own repo — unchanged, sketched as a real option, not designed
further here.

## Architecture

**Revised from the original diagram** (see Corrections above) to reflect
Decision 4/6's reversal: the incoming gate is DS's own OIDC login via an
adapter AS layer, not Discord OAuth; the derived service now calls DS
*as* the authenticated caller for the train-tracking tier, not anonymously
throughout; and the whole thing sits behind a public `Ingress`, not
`ClusterIP`-only.

```
                    AI assistant / MCP client (Claude.ai, Claude Desktop, ...)
                                    │
                    OAuth 2.1: PRM discovery → DCR (open, adapter's own)
                    → PKCE authorization request → browser redirect
                                    │
                                    ▼
┌───────────────────────────────────────────────────────────────────────┐
│ adapter auth layer (Decision 4d) — "distant-signal-mcp"'s own minimal  │
│ OAuth 2.1 authorization server, facing MCP clients                     │
│  • /.well-known/oauth-protected-resource, oauth-authorization-server   │
│  • own open RFC 7591 DCR endpoint                                      │
│  • drives DS's EXISTING oidc.rs login (one static Authentik client) ───┼──► Authentik
│  • holds the resulting DS session server-side, keyed to the MCP        │    (DS's OIDC
│    access token it issues back to the MCP client                      │     provider)
└───────────────────────────┬─────────────────────────────────────────┘
                             │ Authorization: Bearer <MCP-scoped token>
                             ▼
┌───────────────────────────────────────────────────────────────────────┐
│ derived MCP service ("distant-signal-mcp")                             │
│ TypeScript, forked from train-mcp, own repo, own CI/tests              │
│ public Ingress (Decision 6b) ── ClusterIP Service unchanged behind it   │
│                                                                          │
│  resolve_station ─────► GET /public/stations?q=  ──► shim adds         │
│                          (DS, still anonymous —       tiploc (local    │
│                          this route needs no session) stations.json)  │
│                                                        + matchType     │
│                                                        (client-side)   │
│                                                                          │
│  get_departures/       Darwin LDBWS direct (train-mcp's own,           │
│  get_arrivals/         unchanged) ─── NOT routed through DS            │
│  get_service_detail                   (Decision 5) ── now also gated   │
│                                        on the adapter's DS login (4d)  │
│                                                                          │
│  find_services/        own CIF SQLite store + RAPTOR/CSA               │
│  plan_journey          (train-mcp's own, unchanged) ─── NOT migrated   │
│         │                                              (Decision 1/5)  │
│         │ for each TrainLeg inside the live-board horizon:             │
│         ▼                                                               │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ delay annotation (NEW, this spec's core)                        │  │
│  │  1. Darwin board check (train-mcp's own, unmodified — 3a)       │  │
│  │  2. crsForTiploc(leg) → operator+CRS candidate line lookup      │  │
│  │     against a cached GET /public/lines + /lines/{id}/definition │  │
│  │  3. GET /Line/{id}/Status?detail=true (DS, still anonymous)     │  │
│  │  4. scope check: disruption.affectedStops/affectedRoutes        │  │
│  │     vs. leg's own origin/destination CRS                        │  │
│  │  5. GET /Train/by-uid/{uid}/{date} — using the adapter's HELD    │  │
│  │     DS session for the calling identity (3b.6, revised: general, │  │
│  │     not opportunistic), whenever that train is one they track   │  │
│  └─────────────────────────────────────────────────────────────────┘  │
└───────────────────────────┬──────────────────────────────────────────┘
                             │ HTTP — anonymous for /public/*, /Line/*/Status;
                             │ carries the caller's held DS session for
                             │ /Train/by-uid/* (Decision 4d)
                             ▼
                  Distant Signal `api` (crates/api)
                  /public/*, /Line/*/Status  (unauthenticated, unchanged)
                  /Train/by-uid/*  (requires DS session — reachable for
                                     every call now, see 3b.6 revised)
```

## Error handling

- **Every DS call in the annotation loop (Decision 3) is best-effort.**
  A DS timeout, 5xx, or malformed response for one leg's status/definition
  fetch produces `liveStatus: { source: 'unavailable', matchConfidence:
  'none' }` (or omits `liveStatus` entirely) for that leg only — it never
  fails the whole `plan_journey` call, and never blocks or delays the
  schedule-only plan train-mcp already produces today. This mirrors
  `eta_blend.rs`'s own established posture in DS itself ("deliberately
  NOT a guaranteed join", `eta_blend.rs:1-7`) and this repo's TfL-merge
  spec's own overlay-fetch design (optional side-channel, absent when
  unavailable, primary data untouched — `2026-08-22-tfl-service-metrics-v2-design.md`,
  Area 1 §3).
- **`resolve_station`'s DS-backed path**: a DS failure (network error,
  non-2xx) on `GET /public/stations` surfaces as a tool-level error
  (train-mcp's own existing error-handling convention for a failed
  Domain-layer call — not re-derived here, `src/tools/resolve-station.ts`
  itself was not observed to have a bespoke failure path distinct from
  letting the call throw), since this tool has no fallback data source
  once DS is its only upstream for station search.
- **Ambiguous line matches (`matchConfidence: 'ambiguous'`) are not an
  error** — they're a real, correctly-reported outcome (Decision 3b),
  surfaced to the calling LLM/user rather than resolved by guessing.
- **The cached line catalogue (3d) going stale** (DS adds/removes a
  catalogue line between refresh cycles) degrades gracefully to a missed
  match (`matchConfidence: 'none'` for a genuinely new line, or a
  dangling `lineId` reference that a subsequent status fetch 404s —
  treated the same as any other DS-call failure above), not a crash.

## Testing

Following train-mcp's own established convention (`test/*.test.ts`,
`vitest run`, `package.json:14-15`):

- **Leg → DS-line matching (3b), pure unit tests, no network** — mirroring
  DS's own `tfl_ids_to_overlay`/`overlay_for` precedent for "pure so it's
  testable without a database" (`crates/api/src/routes/line_status.rs:63-79`):
  given a fixed, fixture-backed catalogue (`operators`/`stations` per
  line, modeled directly on the real `lines/c2c.toml` /
  `lines/overground-suffragette.toml` / `lines/overground-liberty.toml`
  overlap confirmed in Current relevant state), assert each of
  `matchConfidence`'s four outcomes, explicitly including a test named
  after the real Barking/`c2c`/Suffragette overlap so this exact
  documented ambiguity case can't silently regress to "pick one and
  guess."
- **`resolve_station` shim**: a fixture test asserting `matchType`
  degrades to `substring` (never fabricates `alias`/`fuzzy`) for a
  non-exact DS match, and a `tiploc`-lookup-miss case (a CRS DS returns
  that isn't in the bundled `stations.json` — should this be possible,
  the field is simply omitted, not fabricated).
- **`truncated` computation**: fixture test confirming the `limit + 1`
  over-fetch/slice logic reproduces train-mcp's existing behavior exactly
  against a mocked DS response shape (`{code, name}[]`), not the
  original Darwin-cascade shape.
- **DS-call-failure fallback**: mocked DS 5xx / timeout / malformed-JSON
  cases for each of the three annotation-loop calls (lines, definition,
  status) each independently produce `source: 'unavailable'` for the
  affected leg only, without failing the surrounding `plan_journey` call
  — a direct regression test for the Error handling section's core claim.
- **Scope-precision check (3b.5)**: fixture asserting a leg whose own
  origin/destination CRS are *not* in a matched line's current
  `disruption.affectedStops`/`affectedRoutes` gets `legInDisruptionScope:
  false` alongside the line's overall (still-attached) severity — guards
  against the "whole line reads as disrupted" overstatement this design
  exists partly to avoid.
- **Auth (Decision 4, revised)**: the inverse of the original test —
  a table-driven test partitioning DS calls by whether they should carry
  the caller's held session: `/public/stations`, `/Line/.../Status`,
  `/public/lines`, `/public/lines/{id}/definition` must carry **no**
  `Authorization`/session cookie header (these routes stay anonymous-
  compatible, unchanged by the reversal — Decision 3's annotation loop has
  no reason to force them through auth), while `/Train/by-uid/{uid}/
  {date}` **must** carry the calling identity's held DS session on every
  call, not just opportunistically — a regression guard specifically for
  Decision 3b.6's revised "general mechanism, not narrow/opportunistic"
  claim. A second test: an MCP request with no valid MCP-issued Bearer
  token (never completed the adapter's OAuth flow) is rejected before any
  DS call is attempted at all.
- Nothing in DS's own Rust test suite needs to change — this spec makes
  no DS code changes, so no new DS-side tests are proposed.

## Explicitly out of scope

- **`find_services`/`plan_journey`'s own CIF-timetable-store/RAPTOR/CSA
  engine.** Stays entirely inside the derived service, unmodified, per
  Decision 1 and Decision 5, and per this repo's own carried-forward
  scope boundary from the research doc. Not migrated to DS in either
  direction.
- **Building a queryable DS timetable store from `schedule-ingest`'s CIF
  data.** Carried forward unchanged from the research doc's own
  recommendation 5 (not justified today).
- **The currently-down "National rail status" MCP connector.** Per the
  Context correction above: expected to be down, not investigated
  further, not evidence of anything.
- **Widening `poller-ldbws` to arbitrary stations, or any new DS route
  for `get_departures`/`get_arrivals`.** Decision 5, above — deliberately
  not designed in this pass.
- **A general-purpose "get detail for any service" route on DS.**
  Decision 5's reasoning extends to `get_service_detail` too: still
  scoped to a caller's own tracked train if it were ever built, not
  designed here since no tool in this spec's scope needs it.
- **DS's `Suggestion` type gaining `tiploc`/`matchType` fields.**
  Considered and rejected in Decision 2 for this pass; noted as a
  possible independent follow-up, not designed.
- ~~Reusing DS's OIDC SSO as the derived service's own auth (research
  doc's Option 2). Rejected for this pass in Decision 4.~~ **Superseded —
  see Corrections above and the revised Decision 4.** This is no longer
  out of scope; it is the chosen mechanism, and Authentik's actual Dynamic
  Client Registration support (the exact open question this bullet
  originally deferred) is now researched and cited in Decision 4b.
- **NRE/Network-Rail-branding attribution requirements for MCP
  tool-rendered output.** Genuinely unresolved (see Open questions/risks)
  — this spec does not resolve it, only flags it, and the Licensing note
  below states plainly that this reversal raises the stakes on getting it
  resolved before shipping.
- **Network-policy/autoscaling for the new `railMcp` chart component, and
  precise per-host rate-limiting.** Ingress/TLS itself is now designed
  (Decision 6, revised) — the reversal specifically required resolving
  that part. Network policy and autoscaling stay sketched at the same
  shallow depth this repo's other integration specs give deployment;
  precise rate-limiting is flagged as a real, unresolved gap in Decision
  6d and Open questions/risks, not designed to completion here.
- **The adapter auth layer's own concrete protocol details** (exact token
  format, the token→DS-session mapping's storage, refresh-token handling,
  whether it's a separate deployable component or code inside the derived
  service). Decision 4d states its required shape and responsibilities but
  deliberately doesn't design it to implementation depth — that's
  follow-up plan work, not this document's job.
- **Retiring, repurposing, or keeping `railMcp.discord.*`'s already-shipped
  chart values** (`values.yaml:790-796`) now that Decision 4 supersedes
  Discord OAuth as the incoming gate. A chart-and-plan-level decision, not
  resolved by this design-document edit — flagged in Open questions/risks.
- **`docker-compose.dev.yml`'s exact local-dev story** (bind-mount vs.
  pre-built image for a sibling checkout of the derived service). Flagged
  as a real, deferred choice in Decision 6, not resolved here.

## Licensing note

The research doc's own Licensing section already establishes that DS
holds a real, cited, separately-documented NRE/Darwin licence posture
(`frontend/components/OpenDataAttribution.tsx`) and that a derived
service riding DS's existing NRE licence for LDBWS/Stations/TOCs data
(via `resolve_station`, Decision 2) would not need its own separate
subscription for that specific data — reducing, not adding, redundant
licensing. This design adds one new consideration the research doc raised
but did not resolve, worth restating plainly rather than re-litigating: **serving
DS-derived line-status/incident data (Decision 3's annotation output)
out through an MCP tool that an AI assistant renders to an end user is
arguably a new "presentation" surface for NRE-sourced data** (the
Knowledgebase-derived `reason`/`disruption.description` text DS's own
`line_status`/`incidents` routes serve), distinct from DS's own frontend,
which already carries NRE's required "Powered by National Rail Enquiries"
attribution. Whether an MCP tool's `structuredContent`/rendered-text output,
consumed inside a third-party AI assistant's chat UI, counts as
"presentation" under NRE's Terms & Conditions v3.0 attribution clause the
same way a web page does is **not resolved by this spec** — it's a
licensing/legal question, not an engineering one, and the research doc
already correctly declined to resolve it (its own Open Question 5). This
design's only obligation here is to flag it plainly for a human legal
look before this ships, not to guess at an answer: if attribution is
required, the natural implementation point is the derived service's own
tool-result rendering (a fixed "Powered by National Rail Enquiries" line
appended wherever DS-sourced `reason`/`disruption` text is rendered),
analogous to `OpenDataAttribution.tsx`'s existing fixed-wording component
— not designed further here since the underlying legal question is still
open.

**This reversal raises the stakes on that open question — stated plainly,
not re-litigated.** Everything above was written against a design where
the derived service was reachable only inside the cluster or by whoever
held a Discord-allowlisted account on the operator's own server — a small,
operator-known population. Under the reversal (Decisions 4 and 6 above),
the same NRE-sourced `reason`/`disruption.description` text is now served
to **any DS user who authenticates and connects any MCP client to a
publicly reachable endpoint** — a materially larger, materially less
controlled exposure surface than an opt-in internal-only deployment, even
though every caller is now a real, individually authenticated DS user
rather than an anonymous one. Authentication answers "who can reach this,"
not "is redistributing NRE-sourced text through a third-party AI
assistant's chat UI a licensed 'presentation,'" so it does not narrow the
open legal question at all — it only widens who would be affected by
getting the answer wrong. **This makes the legal sign-off gate (Task 10 of
the implementation plan, already flagged there as blocking deployment)
more urgent, not less**: this design's own recommendation is that
`railMcp.enabled`/`ingress.railMcp.enabled` should not be flipped to
`true` in any real deployment ahead of that sign-off, a stronger statement
than the original document made, because the original document's
deployment shape (internal-only, Discord-gated) was a meaningfully lower-
stakes exposure to ship ahead of a legal answer than a public one is.

## Open questions / risks

1. **DS's curated catalogue size/coverage isn't measured** (carried
   forward from the research doc's own Open Question 4) — this bounds how
   often Decision 3b's matching step finds *any* candidate line at all for
   an arbitrary leg. A leg on a line DS doesn't curate (not every TOC
   service is necessarily represented in `lines/*.toml`) legitimately
   resolves to `matchConfidence: 'none'`, and this design has no fallback
   for that case beyond train-mcp's own existing Darwin-board check (3a).
2. **The 15-minute catalogue-cache refresh interval (3d) is an
   unresearched starting figure**, same posture this repo's other specs
   take for unmeasured constants (`MINE_LIST_LIMIT`, `MAX_PIN_AGE`) — not
   benchmarked against how often DS's catalogue actually changes or how
   much staleness is tolerable.
3. **~~The TRUST-corroboration tier (3b.6) is reachable only in a narrow,
   opportunistic case~~ — resolved by the Decision 4 reversal**, see 3b.6
   as revised: the tier is now a general per-request mechanism, gated only
   by whether the specific `(uid, date)` happens to be a train the calling
   identity actually tracks, not by whether the derived service separately
   holds a session at all. What's genuinely still open is narrower: the
   adapter auth layer's own concrete implementation (4d, and the
   "Explicitly out of scope" bullet above) isn't designed to protocol
   depth, so this conclusion holds for the *shape* Decision 4 commits to,
   not for a specific untested implementation of it.
4. **The NRE-attribution "presentation" question (Licensing note) is
   unresolved, and the reversal makes it more urgent to resolve before
   `railMcp`/`ingress.railMcp` are ever enabled in a real deployment** —
   see the Licensing note's own revised closing paragraph. Not assumed
   either way here.
5. **`lines/*.toml`'s station lists are the ground truth for Decision
   3b's matching, but their curation is aimed at DS's own incident-scope
   inference, not at exhaustively representing every through-route a real
   train might run** — a leg that legitimately runs beyond what a DS line
   definition's `stations` array covers (e.g. a through-service onto
   track DS doesn't curate for its own purposes) will read as
   `matchConfidence: 'operatorAndOneStation'` or `'none'`, which is
   correct behavior (not overclaiming), but means match coverage is
   bounded by DS's own curation scope, not by what train-mcp's CIF data
   could in principle support. Not a defect to fix here — a real,
   inherent boundary of building this on top of DS's curated catalogue
   rather than a from-scratch operator/route database.
6. **Whether train-mcp's live production deployment (if the "National
   rail status" connector's underlying service is in fact it, or a
   derivative) already differs from what this spec assumes about its
   current architecture** is not re-checked here — per the Context
   correction, that connector is expected to be down right now and is out
   of scope to investigate for this pass.
7. **Whether a given production deployment's real Authentik (or other
   OIDC provider) instance is new enough to matter for DCR, and whether
   that matters at all under the chosen design.** Decision 4b resolved
   *that Authentik supports DCR, and how* (2026.8.0+, gated behind an
   existing DCR-scoped bearer token) — but Decision 4d's chosen adapter
   design doesn't actually depend on DS's own IdP supporting DCR at all,
   since the adapter's own DCR endpoint (facing MCP clients) is separate
   from the static, one-time client registration it uses against
   Authentik for the human-login step. This is flagged only because an
   implementer could reasonably wonder whether Authentik's IdP-side DCR
   feature is required infrastructure here — it is not.
8. **`railMcp.discord.*`'s already-shipped chart values are left dangling
   by this reversal** (`values.yaml:790-796`) — whether they're retired,
   kept as a secondary/defense-in-depth gate alongside DS's OIDC, or
   repurposed is explicitly not decided by this document (see Explicitly
   out of scope); a follow-up plan revision needs to make that call before
   implementation.
9. **Rate-limiting a now-publicly-reachable `railMcp` endpoint has no
   design beyond "the ingress-controller annotation lever exists but
   currently applies chart-wide, not per-host"** (Decision 6d) — a real,
   unresolved gap this reversal introduces, not present in the original
   internal-only shape (where the threat model was "whoever's inside the
   cluster or on the Discord allowlist," not "anyone on the public
   internet who completes DS's own login").
