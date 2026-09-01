# train-mcp Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `train-mcp` (the existing, already-built, already-tested TypeScript MCP server sitting in this repo as `train-mcp.zip`) into Distant Signal's derived rail-MCP service by forking it as-is into its own repository, then layering exactly two new capabilities onto that working baseline: `resolve_station` re-pointed at DS's own `GET /public/stations`, and delay-aware `plan_journey` output annotated with DS's live line-status/incident data via a new leg→line matching algorithm. Everything else in the fork — boards, `get_service_detail`, the CIF timetable store, RAPTOR/CSA — is untouched. This plan also adds the new derived service's deployment surface to *this* repo (a Helm subchart, a compose service) and reconciles a general "prefer DS's own API" principle against the one place the design spec deliberately did not route through DS (the board tools).

**Architecture:**

```
Task 1: extract train-mcp.zip -> new repo, `vitest run` green, UNMODIFIED baseline commit
                    |
                    v
Task 2: src/ds/client.ts (anonymous DsApiClient) + config.ts DS_API_BASE_URL
        |                                   |
        v                                   v
Task 3: resolve_station -> DS shim    Task 5: src/ds/lineCatalogue.ts (cached /public/lines,
        (src/stations/resolveViaDs.ts)        /public/lines/{id}/definition)
                                               |
        Task 4: src/ds/lineMatch.ts            |
        (pure leg-to-line matcher,             |
         no network, own tests) ---------------+
                    |                          |
                    v                          v
              Task 6: src/ds/annotateLeg.ts wired into plan_journey
              (status fetch, scope check, liveStatus schema, error handling)
                    |
      +-------------+-------------+
      v                           v
Task 7: charts/distant-signal/    Task 8: docker-compose.yml
templates/railmcp-*.yaml          rail-mcp: service
+ values.yaml railMcp: block

Task 9 (flagged follow-on, not executed by this plan): smallest DS route
addition that would let board tools route through DS too.

Task 10 (non-code gate): NRE-attribution legal sign-off before shipping
Decision 3's output.
```

**Tech Stack:** TypeScript (Node >=24, `@modelcontextprotocol/sdk`, `zod`, `express`) inside the forked service's own new repository; `vitest run` for its tests. Helm (this repo's existing `charts/distant-signal/` chart, Go templates) and Docker Compose (this repo's existing `docker-compose.yml`) for the two deployment surfaces that live in *this* repo. No Rust code changes anywhere in Tasks 1-8 or Task 10 — the only task touching `crates/*` is Task 9, and it is explicitly a flagged proposal, not part of this plan's executed critical path.

**Spec:** `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md` — read in full before starting; this plan carries its Decisions into concrete tasks rather than re-deriving them. Cross-references below to "Decision N" refer to that document. It in turn builds on `docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md` ("the research doc").

**Status note — every claim below re-verified directly against the actual extracted `train-mcp.zip` source this session (not trusted from the design spec's or research doc's own summaries), at `/tmp/train-mcp-plan-extract/train-mcp/` (`.env`/`.env.prod` excluded from extraction and never opened, per the task brief).** Three real findings surfaced by this direct verification, not called out by the design spec itself:

1. **The design spec's Decision 3 and Architecture diagram both frame a "Darwin board check (train-mcp's own, unmodified — 3a)" as an existing foundational mechanism inside `plan_journey` that the DS-annotation layer is "layered onto."** Direct inspection of `src/tools/plan-journey.ts` (read in full) and the fork's own docs (`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:32,166` — *"Live disruption applied to a plan. That is Phase 2c."*; `docs/superpowers/specs/2026-07-21-train-mcp-phase2a-timetable-store-design.md:32,394` — *"Live overlay. Cross-checking a plan against Darwin is Phase 2c."*) confirms **no such check exists in the fork today.** `plan_journey`'s own `JOURNEY_FRAME_NOTE` constant (`plan-journey.ts:526-530`) states plainly: *"...carries no live delay, cancellation or platform-change information."* The research doc's own Recommendation 3 independently confirms this is proposed, not built: *"buildable independent of any DS migration... just extending train-mcp's own `plan_journey` to consult Darwin's live boards."* This plan therefore does **not** treat a Darwin check as a pre-existing foundation to layer onto — Task 6 below builds the DS-sourced `liveStatus` annotation as the entire mechanism this plan adds to `plan_journey`, standalone. The 239-minute live-board-horizon *figure* is still reused (it is a fact about Darwin's own API depth, independent of whether `plan_journey` calls it), but as a bare number, not an imported constant or existing check — see Task 6, Step 1.
2. **DS's `/public/stations` route accepts no `limit` query parameter at all.** `crates/api/src/routes/reference.rs`'s `SearchQuery` struct (read directly) is `{ q: String }` only — `SUGGESTION_LIMIT = 20` is a hard, server-side, non-negotiable cap, not something a caller can raise or lower. The design spec's Decision 2 describes the shim as requesting "`limit + 1` from DS, mirroring train-mcp's own existing over-fetch trick" — that literal HTTP-level trick isn't possible against this route shape (there's nothing to send). Task 3 below implements the equivalent *effect* (an honest `truncated` signal) without a param that doesn't exist — see Task 3, Step 4, including the specific edge case DS's hard cap creates when a caller's own requested `limit` reaches the tool's own max of 20 (`resolve-station.ts:21`, `z.number().int().min(1).max(20)`), which exactly equals DS's own cap.
3. **`find-services.ts`'s `resolveOne`** (consumed by both `find_services` and `plan_journey` for their own `from`/`to`/`via`/`avoid` inputs, `find-services.ts:179-181`) **calls `resolveStation`/`findByCrs` from `src/stations/resolve.ts` directly** — the same local-corpus cascade `resolve_station` (the MCP tool) currently sits on top of. Decision 2 only migrates `resolve_station`'s own tool backing (`src/tools/resolve-station.ts`); `src/stations/resolve.ts`'s `resolveStation`/`findByCrs` functions are **not** deleted, deprecated, or modified anywhere in this plan — Task 3 explicitly reuses `findByCrs` (unchanged) for its local tiploc join, and `find-services.ts`/`plan-journey.ts`'s own independent use of the same functions is untouched, matching Decision 1/5's "find_services/plan_journey ... unchanged" boundary precisely.

## Global Constraints

- **Prefer DS's own REST API wherever this derived service can feasibly use it — a general implementation principle for the whole service, not something satisfied only by Decisions 2 and 3's two scoped capabilities.** Every new outbound HTTP call this plan's tasks add defaults to a DS route unless DS genuinely has no equivalent public data source. This currently applies concretely to: station lookup (Task 3, DS's `/public/stations`), line/incident status (Task 6, DS's `/Line/{id}/Status`, `/public/lines`, `/public/lines/{id}/definition`). It does **not** apply to the CIF-timetable-store/RAPTOR/CSA engine (`find_services`/`plan_journey`'s own planned-timetable data) — DS has no queryable timetable store at all (confirmed by the design spec's Current relevant state and re-confirmed by this plan's own reading of `crates/api`'s route surface), so there is nothing to route through yet; that boundary is unchanged from Decision 1/5. It also, as of this plan, does not yet apply to the three live-board tools (`get_departures`/`get_arrivals`/`get_service_detail`) — Decision 5 of the design spec deliberately keeps those on direct Darwin/LDBWS access because DS does not today expose a public per-station board route. **This plan does not silently carry that scope-out forward as permanently settled**: Task 9 below is a real, file-level-detailed follow-on proposal for the smallest DS route addition that would close this gap, explicitly flagged as *not* part of this plan's own executed critical path (Tasks 1-8, 10) — see Task 9's own header for why it is scoped this way rather than folded into the main sequence.
- **No `.env`/`.env.prod` file from the extracted zip is ever opened, read, or referenced in any task below.** Both are excluded from Task 1's extraction outright.
- **Task 1 is the load-bearing "fork, don't rewrite" task.** Every later task modifies or extends the fork's own existing files; no task in this plan re-implements any part of `src/timetable/*`, `src/ldbws/*`, `src/auth/*`, `src/tools/boards.ts`, `src/tools/service-detail.ts`, or `src/tools/find-services.ts`. If a later task's step appears to require duplicating logic that already exists in one of those files, that is a signal to re-read this plan's Status note above and Task 1's own commit, not to write a parallel implementation.
- **Testing convention, unchanged from the fork's own:** colocated-nowhere, flat `test/*.test.ts` at the repo root, `vitest run` (`package.json:14-15`), `vitest.config.ts`'s `include: ['test/**/*.test.ts']`. Every new test file this plan adds goes in `test/`, not colocated with `src/`.
- **Module resolution is ESM with explicit `.js` extensions on relative imports**, matching every existing file in the fork (e.g. `src/tools/resolve-station.ts:3`: `from '../stations/resolve.js'`). Every new `import`/`export` this plan adds follows the same `.js`-suffixed-relative-import convention, even though the source file is `.ts`.
- **Every DS call this plan adds is anonymous — no `Authorization` header, no session cookie, ever** (Decision 4, Auth Option 1). This is a hard constraint, not a default that can silently regress: Task 2's own test suite includes a smoke test asserting no such header is ever sent, and Task 6's status-fetch call reuses the same `DsApiClient` rather than hand-rolling a second HTTP call path that could drift from it.
- **Every DS call inside the `plan_journey` delay-annotation loop (Task 6) is best-effort.** A DS timeout, non-2xx, or malformed-JSON response for one leg's line/definition/status fetch must never throw out of the tool handler and must never suppress or delay the schedule-only plan `plan_journey` already produces today — it degrades to `liveStatus: { source: 'unavailable', matchConfidence: 'none' }` for that leg only. This mirrors `crates/api/src/data/eta_blend.rs`'s own "deliberately NOT a guaranteed join" posture, cited directly by the design spec's Error handling section.
- **Ambiguous line matches (`matchConfidence: 'ambiguous'`) are a correct, reportable outcome, not an error and not something to silently resolve by picking one candidate.** Any implementation that picks a single line id when more than one candidate's station membership matches both of a leg's CRS codes is a defect in that task, not an acceptable simplification — this is the same posture `resolve_station` already takes for ambiguous station names.
- **Reuse `src/cache.ts`'s existing `TtlCache<V>` for Task 5's catalogue cache. Do not introduce a second caching mechanism, a `setInterval`-driven background refresh loop, or a new npm dependency for this.** The fork has exactly one caching idiom today (`src/ldbws/client.ts`'s board cache: lazy per-key TTL, sweep-on-write, no timer) and Task 5 follows it — see Task 5's own note on why this satisfies Decision 3d's "refreshes on an interval" without adding a mechanism this codebase doesn't already have.
- **No new npm dependency anywhere in Tasks 1-8.** Every new module this plan adds (`src/ds/client.ts`, `src/ds/lineMatch.ts`, `src/ds/lineCatalogue.ts`, `src/ds/annotateLeg.ts`, `src/stations/resolveViaDs.ts`) uses only what `package.json` already lists (`zod`, the platform `fetch`, `node:*` builtins) plus the fork's own existing modules.
- **The derived service's own source code lives in its own new repository (created by Task 1), never inside this Distant Signal repo.** Only Tasks 7 and 8 touch files inside this repo (`charts/distant-signal/`, `docker-compose.yml`), matching Decision 1's explicit "deployment lives here, source doesn't" split. Task 9, if ever executed, is the one exception in the opposite direction — it touches `crates/api` inside this repo, and is exactly why it is scoped as a separate, unexecuted, flagged proposal rather than folded into Tasks 1-8's TypeScript-fork sequence.
- **This plan does not resolve the NRE-attribution "presentation" question the design spec's own Licensing note raises.** Task 10 is a non-code gate that must complete — with an explicit yes/no human legal answer — before Task 6's `liveStatus`-carrying output (which surfaces DS's Knowledgebase-derived `reason`/`disruption.description` text through an MCP tool) ships to real users. Nothing in Tasks 1-9 is blocked on Task 10 for *development*; Task 10 blocks *deployment/rollout* of Task 6's output.
- **Parallelizable tasks:** Task 4 depends only on Task 1 (it is pure, fixture-driven, no DS client) and can run in parallel with Tasks 2/3/5. Tasks 7 and 8 depend only on Task 2 (they need the `DS_API_BASE_URL` env var name settled) and touch disjoint files from every TypeScript task — dispatch them in parallel with Tasks 3-6 once Task 2 lands. Task 9 has no dependency on any other task in this plan (it is Rust-side, inside this repo, independent of the TypeScript fork entirely) and Task 10 has no code dependency at all — both can be started at any point, but Task 10 must complete before Task 6's output goes live.

---

### Task 1: Stand up the fork baseline — extract, verify, checkpoint, BEFORE any Distant-Signal-specific change

**Files:**
- Create: a new directory outside this git worktree (this plan uses `distant-signal-mcp/` as a working name throughout — substitute the real target path/repository name at execution time; Decision 1 only specifies "its own repository," not a name or host).
- Read only, never modified in this task: `/workspaces/github-com-fasterspeeding-network-rail-status/train-mcp.zip`.

**Interfaces:**
- Produces: a working, standalone git repository containing the fork's own unmodified `src/`, `test/`, `docs/`, `.superpowers/`, `scripts/`, `Dockerfile`, `docker-compose.yml`, `docker-compose.prod.yml`, `package.json`, `tsconfig*.json`, `vitest.config.ts`, `README.md`, `TODO.md`, `.gitignore` — everything the zip contains except `.env`/`.env.prod` — with its own `vitest run` passing (249 tests, per the fork's own `TODO.md:35`) and its own `git log` showing a clean checkpoint commit every later task builds on top of.
- Consumed by: every other task in this plan. **Nothing in Task 2 onward may re-derive, re-implement, or bypass anything this task extracts** — see Global Constraints.
- **Depends on:** nothing. This is the foundational task.

This is the task where "prefer deriving from the existing system over rebuilding it" is load-bearing, not decorative: the fork is a real, substantial, already-tested codebase (six working MCP tools, a CIF timetable store, RAPTOR/CSA/connections/interchange journey planning, Discord auth — confirmed directly this session, not assumed from either prior doc's summary). Every later task in this plan is a diff on top of the exact commit this task produces, never a parallel rewrite.

- [ ] **Step 1: Extract the zip to a scratch location, confirm no `.env*` present**

```bash
mkdir -p /tmp/train-mcp-plan-extract
unzip -q /workspaces/github-com-fasterspeeding-network-rail-status/train-mcp.zip -d /tmp/train-mcp-plan-extract
find /tmp/train-mcp-plan-extract -iname ".env*" -delete
find /tmp/train-mcp-plan-extract -iname ".env*"   # must print nothing
```

Confirm the extracted layout is `/tmp/train-mcp-plan-extract/train-mcp/` (one nested top-level directory), with `src/`, `test/`, `docs/`, `.superpowers/`, `scripts/`, `Dockerfile`, `docker-compose.yml`, `docker-compose.prod.yml`, `package.json` all present directly under it.

- [ ] **Step 2: Copy the extracted content into the new repository's working directory**

```bash
mkdir -p distant-signal-mcp
cp -r /tmp/train-mcp-plan-extract/train-mcp/. distant-signal-mcp/
cd distant-signal-mcp
```

The fork's own `.gitignore` (already present, unmodified) already excludes `node_modules/`, `dist/`, `.env`, `.env.prod`, `.playwright-mcp/`, and `data/`/`*.zip` (the CIF timetable extract/store) — no changes needed to it for this step.

- [ ] **Step 3: `git init`, commit the unmodified baseline**

```bash
git init
git add -A
git status   # confirm no .env*, node_modules/, dist/, or data/ staged
git commit -m "Initial fork of train-mcp (unmodified baseline)

Forked from the already-built, already-tested train-mcp codebase
(README.md, TODO.md, .superpowers/sdd/ progress logs) rather than
rebuilt from scratch. Every later commit on this branch is a diff on
top of this exact checkpoint."
```

- [ ] **Step 4: Install and verify the baseline builds and tests pass, unmodified**

```bash
npm install
npm run build       # tsc -- must succeed with zero errors
npm run typecheck   # tsc --noEmit + tsc --noEmit -p tsconfig.test.json
npm test            # vitest run
```

Expected: `npm run build` and `npm run typecheck` both exit 0. `npm test` reports all suites passing (the fork's own `TODO.md:35` states "249 tests, typecheck clean" as of its last recorded state — a materially different count here is not automatically a failure, since the checked-out zip may be a slightly later or earlier snapshot than that note, but any *failing* test is a hard stop: do not proceed to Task 2 with a red baseline).

- [ ] **Step 5 (only if Step 4 finds a genuine, pre-existing failure): fix it as its own commit, isolated from every later task**

If `npm test`/`npm run build`/`npm run typecheck` fail on the untouched baseline, fix only what is necessary to make them pass, commit that fix separately (`git commit -m "Fix pre-existing <X> to get the unmodified baseline green"`), and note in the commit body that this is baseline hygiene, not a Distant-Signal-specific change. Do not combine this with any step from Task 2 onward.

- [ ] **Step 6: Rename the fork's own advertised identity for this derivative**

Two small, deliberate renames — not part of "unmodified baseline," so this is its own commit after Step 3/5's checkpoint(s):

`package.json`:
```json
{
  "name": "distant-signal-mcp",
  ...
}
```
(leave `version`, `engines`, every dependency untouched — only the `name` field changes)

`src/server.ts:33` (inside `buildServer`):
```ts
const server = new McpServer({ name: 'distant-signal-mcp', version: '0.1.0' });
```

This matches the design spec's own Architecture diagram naming ("derived MCP service (`distant-signal-mcp`)") and is what an MCP client actually sees on tool discovery — worth getting right before any DS-specific tool logic is added, not as an afterthought later.

- [ ] **Step 7: Re-run the full suite after the rename, commit**

```bash
grep -rn "'train-mcp'" src/ test/   # confirm only intentional remaining references, if any (e.g. package-lock.json's own name field is fine to leave)
npm run build && npm run typecheck && npm test
git add package.json src/server.ts
git commit -m "Rename fork's advertised identity to distant-signal-mcp"
```

Expected: PASS, identical to Step 4's result.

---

### Task 2: DS API client + anonymous auth wiring (Decision 4)

**Files:**
- Create (in `distant-signal-mcp/`): `src/ds/client.ts`, `src/ds/errors.ts`
- Modify: `src/config.ts`
- Test: `test/config.test.ts` (extend), `test/ds-client.test.ts` (new)

**Interfaces:**
- Produces: `DsApiClient` class (`searchStations(query, limit?)`, `getLines()`, `getLineDefinition(id)`, `getLineStatus(id)`), `DsUnavailableError`, `Config.ds: { baseUrl: string; catalogueTtlMs: number }`.
- Consumed by: Task 3 (`searchStations`), Task 5 (`getLines`/`getLineDefinition`), Task 6 (`getLineStatus`).
- **Depends on:** Task 1 (needs the checkpointed baseline to modify).

This models `src/ds/client.ts` directly on `src/ldbws/client.ts`'s own shape (constructor takes `fetchImpl` for testability, base URL normalisation, a `get`-style private helper that maps non-2xx/parse failures to a typed error) — the fork's own established pattern for "one HTTP upstream, one typed client," reused rather than invented fresh.

- [ ] **Step 1: `src/ds/errors.ts`**

```ts
/** Thrown for any failure reaching or parsing a response from Distant Signal's public API — network error, non-2xx, or a body that isn't JSON. Every caller in this codebase treats this uniformly as "DS is unavailable right now," per Decision 3's Error handling: never a reason to fail the surrounding tool call, only a reason to degrade that one piece of DS-sourced data. */
export class DsUnavailableError extends Error {
    constructor(message: string, options?: ErrorOptions) {
        super(message, options);
        this.name = 'DsUnavailableError';
    }
}
```

- [ ] **Step 2: `src/ds/client.ts`**

```ts
import { DsUnavailableError } from './errors.js';

export interface DsClientOptions {
    baseUrl: string;
    fetchImpl?: typeof fetch;
}

export interface DsStationSuggestion {
    code: string;
    name: string;
}

export interface DsLineSummary {
    id: string;
    name: string;
    category: string;
    operators: string[];
    source: string;
}

export interface DsLineDefinition {
    stations: string[];
    operators: string[];
}

export interface DsAffectedRoute {
    from: string;
    to: string;
}

export interface DsDisruption {
    category: string;
    description: string;
    affectedStops: string[];
    affectedRoutes: DsAffectedRoute[];
    source: string | null;
}

export interface DsSampleStats {
    total: number;
    delayed: number;
    cancelled: number;
    skipped: number;
    avgDelayMinutes: number;
}

export interface DsLineStatus {
    statusSeverity: number;
    statusSeverityDescription: string;
    reason: string;
    dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
    validityPeriods: Array<{ fromDate: string; toDate: string | null; isNow: boolean }>;
    sampleStats?: DsSampleStats;
    disruption?: DsDisruption;
}

/** Shape of `crates/api/src/render.rs::to_tfl_shape`'s JSON, one report per requested line id. `GET /Line/{ids}/Status` (`crates/api/src/routes/line_status.rs::get_line_status`) always returns an ARRAY of these, even for a single id — confirmed directly against that handler's `Json<Vec<Value>>` return type. */
export interface DsLineStatusReport {
    id: string;
    name: string;
    modeName: string;
    operators: string[];
    computedAt: string;
    lineStatuses: DsLineStatus[];
}

/**
 * Anonymous HTTP client to Distant Signal's own public API — Decision 4:
 * every route this client calls (`/public/stations`, `/public/lines`,
 * `/public/lines/{id}/definition`, `/Line/{id}/Status`) is confirmed
 * unauthenticated (`crates/api/src/routes/mod.rs::public_router()`, and
 * `line_status.rs`'s routes taking `OptionalAuthenticatedUser` purely to
 * decide whether to include a caller's own private custom lines — never
 * gating catalogue-line data). This client never sends an `Authorization`
 * header or a session cookie — see test/ds-client.test.ts's auth smoke
 * test, which is the regression guard for that never regressing by
 * accident later.
 */
export class DsApiClient {
    private readonly baseUrl: string;
    private readonly fetchImpl: typeof fetch;

    constructor(options: DsClientOptions) {
        this.baseUrl = options.baseUrl.replace(/\/+$/, '');
        this.fetchImpl = options.fetchImpl ?? fetch;
    }

    /**
     * `GET /public/stations?q=`. DS's own `SearchQuery` struct
     * (`crates/api/src/routes/reference.rs`) accepts `q` only — there is
     * no `limit` parameter to send; `SUGGESTION_LIMIT = 20` is a hard,
     * server-side cap this client cannot raise or lower. See Task 3 for
     * how the caller turns this into an honest `truncated` signal without
     * a request-side over-fetch trick.
     */
    async searchStations(query: string): Promise<DsStationSuggestion[]> {
        const url = new URL(`${this.baseUrl}/public/stations`);
        url.searchParams.set('q', query);
        return this.getJson(url.toString()) as Promise<DsStationSuggestion[]>;
    }

    /** `GET /public/lines`. Includes every source (`'catalogue'`, `'custom-*'` never appears here per `list_lines`'s own doc, `'tfl'`) — callers that only want National Rail catalogue lines filter `source !== 'tfl'` themselves (Task 5). */
    async getLines(): Promise<DsLineSummary[]> {
        return this.getJson(`${this.baseUrl}/public/lines`) as Promise<DsLineSummary[]>;
    }

    /** `GET /public/lines/{id}/definition`. Unauthenticated for a catalogue id specifically (`crates/api/src/routes/lines.rs::get_line_definition`). */
    async getLineDefinition(id: string): Promise<DsLineDefinition> {
        return this.getJson(`${this.baseUrl}/public/lines/${encodeURIComponent(id)}/definition`) as Promise<DsLineDefinition>;
    }

    /** `GET /Line/{id}/Status?detail=true` for exactly one line id — cheaper than the `/Line/Mode/{mode}/Status` bulk form for a single leg's match (Decision 3b step 4). */
    async getLineStatus(id: string): Promise<DsLineStatusReport> {
        const url = `${this.baseUrl}/Line/${encodeURIComponent(id)}/Status?detail=true`;
        const rows = (await this.getJson(url)) as DsLineStatusReport[];
        const report = rows[0];
        if (!report) {
            throw new DsUnavailableError(`Distant Signal returned no status report for line "${id}"`);
        }
        return report;
    }

    private async getJson(url: string): Promise<unknown> {
        let response: Response;
        try {
            // Deliberately no Authorization/session header — Decision 4.
            response = await this.fetchImpl(url, { headers: { Accept: 'application/json' } });
        } catch (cause) {
            throw new DsUnavailableError(`Could not reach Distant Signal at ${url}`, { cause });
        }
        if (!response.ok) {
            throw new DsUnavailableError(`Distant Signal returned ${response.status} for ${url}`);
        }
        try {
            return await response.json();
        } catch (cause) {
            throw new DsUnavailableError(`Distant Signal returned a body that was not JSON for ${url}`, { cause });
        }
    }
}
```

- [ ] **Step 3: Extend `src/config.ts`**

Add to the `Config` interface, alongside the existing `ldbws`/`timetableDbPath` fields:

```ts
    ds: {
        baseUrl: string;
        /** How long Task 5's catalogue cache trusts one fetch of GET /public/lines / GET /public/lines/{id}/definition before refetching. Decision 3d's own figure (15 minutes) is explicitly flagged there as an unresearched starting point, not benchmarked — same posture as this app's other unresearched constants. */
        catalogueTtlMs: number;
    };
```

Add to `loadConfig`'s return, using the existing `required`/`positiveInteger` helpers already in this file:

```ts
        ds: {
            baseUrl: required(env, 'DS_API_BASE_URL').replace(/\/+$/, ''),
            catalogueTtlMs: positiveInteger(env, 'DS_LINE_CATALOGUE_TTL_MS', 15 * 60 * 1000)
        },
```

- [ ] **Step 4: Extend `test/config.test.ts`**

Add `DS_API_BASE_URL: 'https://ds.example.com'` to the `valid` fixture object at the top of the file, and add:

```ts
    it('parses the DS API base URL and strips a trailing slash', () => {
        const config = loadConfig({ ...valid, DS_API_BASE_URL: 'https://ds.example.com/' });
        expect(config.ds.baseUrl).toBe('https://ds.example.com');
    });

    it('names DS_API_BASE_URL when it is missing', () => {
        const { DS_API_BASE_URL, ...rest } = valid;
        expect(() => loadConfig(rest)).toThrow(/DS_API_BASE_URL/);
    });

    it('defaults the DS line catalogue TTL to 15 minutes', () => {
        expect(loadConfig(valid).ds.catalogueTtlMs).toBe(15 * 60 * 1000);
    });

    it('accepts an explicit DS_LINE_CATALOGUE_TTL_MS', () => {
        expect(loadConfig({ ...valid, DS_LINE_CATALOGUE_TTL_MS: '60000' }).ds.catalogueTtlMs).toBe(60_000);
    });
```

- [ ] **Step 5: `test/ds-client.test.ts` — including the auth smoke test**

```ts
import { describe, expect, it, vi } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { DsUnavailableError } from '../src/ds/errors.js';

function jsonResponse(body: unknown, status = 200): Response {
    return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

describe('DsApiClient', () => {
    it('never sends an Authorization header or a session cookie on any call', async () => {
        const calls: RequestInit[] = [];
        const fetchImpl = (vi.fn(async (_url: string, init?: RequestInit) => {
            calls.push(init ?? {});
            return jsonResponse([]);
        }) as unknown) as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });

        await client.searchStations('bath');
        await client.getLines();
        await client.getLineDefinition('c2c');
        try {
            await client.getLineStatus('c2c');
        } catch {
            // getLineStatus throws on an empty array response above; the
            // request headers were already captured before that point.
        }

        for (const init of calls) {
            const headers = new Headers(init.headers);
            expect(headers.has('Authorization')).toBe(false);
            expect(headers.has('Cookie')).toBe(false);
        }
    });

    it('searchStations sends q and no limit parameter', async () => {
        let requestedUrl = '';
        const fetchImpl = (async (url: string) => {
            requestedUrl = url;
            return jsonResponse([{ code: 'BTH', name: 'Bath Spa' }]);
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });

        const results = await client.searchStations('bath');
        expect(results).toEqual([{ code: 'BTH', name: 'Bath Spa' }]);
        const url = new URL(requestedUrl);
        expect(url.searchParams.get('q')).toBe('bath');
        expect(url.searchParams.has('limit')).toBe(false);
    });

    it('getLineStatus unwraps the single-element array DS always returns', async () => {
        const report = { id: 'c2c', name: 'c2c', modeName: 'rail', operators: ['CC'], computedAt: '2026-09-01T00:00:00Z', lineStatuses: [] };
        const fetchImpl = (async () => jsonResponse([report])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getLineStatus('c2c')).toEqual(report);
    });

    it('throws DsUnavailableError on a non-2xx response', async () => {
        const fetchImpl = (async () => jsonResponse({ error: 'nope' }, 500)) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getLines()).rejects.toBeInstanceOf(DsUnavailableError);
    });

    it('throws DsUnavailableError on a network failure', async () => {
        const fetchImpl = (async () => {
            throw new Error('ECONNREFUSED');
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getLines()).rejects.toBeInstanceOf(DsUnavailableError);
    });

    it('throws DsUnavailableError on a body that is not JSON', async () => {
        const fetchImpl = (async () => new Response('<html>gateway error</html>', { status: 200 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getLines()).rejects.toBeInstanceOf(DsUnavailableError);
    });

    it('throws DsUnavailableError when getLineStatus gets an empty array', async () => {
        const fetchImpl = (async () => jsonResponse([])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getLineStatus('dangling-id')).rejects.toBeInstanceOf(DsUnavailableError);
    });
});
```

- [ ] **Step 6: Run the tests**

Run: `npm test`
Expected: PASS, including every existing suite untouched by this task.

- [ ] **Step 7: Commit**

```bash
git add src/ds/client.ts src/ds/errors.ts src/config.ts test/config.test.ts test/ds-client.test.ts
git commit -m "Add anonymous DsApiClient and DS_API_BASE_URL config (Decision 4)"
```

---

### Task 3: `resolve_station` migration onto DS's `/public/stations` (Decision 2)

**Files:**
- Create: `src/stations/resolveViaDs.ts`
- Modify: `src/tools/resolve-station.ts`, `src/server.ts`, `src/app.ts`
- Test: `test/ds-resolve-station.test.ts` (new, pure shim logic), `test/tools-resolve-station.test.ts` (rewrite the DS-facing parts)

**Interfaces:**
- Produces: `resolveStationViaDs(client: DsApiClient, query: string, limit: number): Promise<{ matches: DsStationMatch[]; truncated: boolean }>`. `DsStationMatch = { crs: string; name: string; tiploc?: string; matchType: 'exact_crs' | 'exact_name' | 'substring' }` — note: **not** the same `MatchType` union `src/stations/resolve.ts` exports (`'alias' | 'fuzzy'` are dropped — this shim never produces them, per Decision 2's "downgrade to the closest honest category" ruling) and **`tiploc` is optional**, not required (see Step 2's note).
- Consumed by: `src/tools/resolve-station.ts`'s tool handler (replaces its call into `resolveStation` from `src/stations/resolve.ts` — that function itself is untouched, see this plan's Status note #3).
- **Depends on:** Task 2 (`DsApiClient`).

- [ ] **Step 1: `src/stations/resolveViaDs.ts` — matchType and truncation, no network beyond one DS call**

```ts
import type { DsApiClient, DsStationSuggestion } from '../ds/client.js';
import { findByCrs } from './resolve.js';

export type DsMatchType = 'exact_crs' | 'exact_name' | 'substring';

export interface DsStationMatch {
    crs: string;
    name: string;
    /** Omitted, never fabricated, when the CRS DS returns isn't in this fork's own bundled stations.json (Decision 2). */
    tiploc?: string;
    matchType: DsMatchType;
}

/** DS's own hard server-side cap (`crates/api/src/routes/reference.rs`'s `SUGGESTION_LIMIT`) — this client sends no `limit` parameter because DS's route accepts none; see Task 2's client and this plan's Status note #2. */
const DS_SUGGESTION_LIMIT = 20;

function classify(suggestion: DsStationSuggestion, query: string): DsMatchType {
    const trimmedQuery = query.trim();
    if (suggestion.code.toUpperCase() === trimmedQuery.toUpperCase()) {
        return 'exact_crs';
    }
    if (suggestion.name.trim().toLowerCase() === trimmedQuery.toLowerCase()) {
        return 'exact_name';
    }
    // DS's own query is always a substring match by construction (`%q%` in
    // `search_stations`'s SQL) — 'alias'/'fuzzy' are never produced here,
    // since DS has no alias table and no fuzzy/edit-distance scoring to
    // report a confidence for. Downgrading to the closest honest category
    // rather than fabricating either is Decision 2's own explicit ruling.
    return 'substring';
}

function toMatch(suggestion: DsStationSuggestion, query: string): DsStationMatch {
    const local = findByCrs(suggestion.code);
    return {
        crs: suggestion.code,
        name: suggestion.name,
        ...(local ? { tiploc: local.tiploc } : {}),
        matchType: classify(suggestion, query)
    };
}

/**
 * DS-backed replacement for `resolve_station`'s candidate resolution.
 * `src/stations/resolve.ts`'s own `resolveStation`/`findByCrs` are NOT
 * modified by this function — `findByCrs` is reused unchanged for the
 * local tiploc join (a pure local lookup against this fork's own bundled
 * `stations.json`, no second network call), and `resolveStation` itself
 * stays exactly as-is for `find-services.ts`'s `resolveOne`, which this
 * function has no relationship to.
 *
 * `truncated`: DS's own `SUGGESTION_LIMIT` (20) gives no over-fetch signal
 * a caller-requested `limit` smaller than 20 can rely on alone — but when
 * DS's response hits its own hard cap exactly, this function treats that
 * as truncated too, even if the caller's own `limit` is also 20 (the
 * tool's own max, `resolve-station.ts`'s `z.number().max(20)`): DS
 * returning exactly its own ceiling is indistinguishable from "there were
 * 21+ and DS silently dropped the rest," and reporting `truncated: false`
 * in that case would be exactly the false certainty this whole tool
 * exists to avoid (`resolve-station.ts`'s own description string).
 */
export async function resolveStationViaDs(
    client: DsApiClient,
    query: string,
    limit: number
): Promise<{ matches: DsStationMatch[]; truncated: boolean }> {
    const suggestions = await client.searchStations(query);
    const truncated = suggestions.length > limit || suggestions.length === DS_SUGGESTION_LIMIT;
    const matches = suggestions.slice(0, limit).map(suggestion => toMatch(suggestion, query));
    return { matches, truncated };
}
```

- [ ] **Step 2: Rewrite `src/tools/resolve-station.ts`**

Replace the file's content. `tiploc` becomes optional in the Zod shape (matching `DsStationMatch.tiploc?`), `matchType`'s enum drops `'alias'`/`'fuzzy'`, and the handler takes a `DsApiClient` dependency instead of calling the local `resolveStation` cascade:

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { DsApiClient } from '../ds/client.js';
import { resolveStationViaDs } from '../stations/resolveViaDs.js';

const matchShape = {
    crs: z.string(),
    name: z.string(),
    tiploc: z.string().optional(),
    matchType: z.enum(['exact_crs', 'exact_name', 'substring'])
};

export function registerResolveStation(server: McpServer, ds: DsApiClient): void {
    server.registerTool(
        'resolve_station',
        {
            title: 'Resolve a UK station name',
            description:
                'Resolve a UK railway station name or code to its CRS code, sourced from Distant Signal\'s own station reference data. Call this before any board tool when you have a station name rather than a three-letter CRS code. Ambiguous or partial names return several candidates — ask the user which they mean rather than assuming. matchType indicates confidence: exact_crs and exact_name are certain, substring means the query only partially matched.',
            inputSchema: {
                query: z.string().describe('Station name or CRS code, e.g. "Bath Spa" or "BTH"'),
                limit: z.number().int().min(1).max(20).optional().describe('Maximum candidates, default 5')
            },
            outputSchema: {
                matches: z.array(z.object(matchShape)),
                truncated: z
                    .boolean()
                    .describe(
                        'True when more stations match than were returned. Ask the user to narrow the query rather than assuming the list is complete.'
                    )
            }
        },
        async ({ query, limit }) => {
            const requested = limit ?? 5;
            const { matches, truncated } = await resolveStationViaDs(ds, query, requested);

            const summary =
                matches.length === 0
                    ? `No UK station matched "${query}".`
                    : [
                          ...matches.map(m => `${m.name} (${m.crs}) [${m.matchType}]`),
                          ...(truncated
                              ? [
                                    `More than ${requested} stations match "${query}". Ask the user to be more specific, or retry with a higher limit.`
                                ]
                              : [])
                      ].join('\n');
            return {
                content: [{ type: 'text', text: summary }],
                structuredContent: { matches, truncated }
            };
        }
    );
}
```

Note the handler no longer catches a DS failure — per the design spec's Error handling: *"a DS failure ... surfaces as a tool-level error ... since this tool has no fallback data source once DS is its only upstream."* Letting `resolveStationViaDs`'s rejection (a `DsUnavailableError`) propagate is the correct behavior here, not a gap — the MCP SDK's own tool-call error handling turns an uncaught rejection into an `isError: true` result, matching the fork's existing convention for a failed Domain-layer call (no bespoke try/catch existed in the original `resolve-station.ts` either).

- [ ] **Step 3: Wire `DsApiClient` into `server.ts` and `app.ts`**

`src/server.ts` — extend `ServerDeps`, following the existing `timetable?`/`plan?` optional-with-default idiom so tests that don't touch `resolve_station`/`plan_journey` need not change:

```ts
import { DsApiClient } from './ds/client.js';
// ... existing imports ...

interface DsDeps {
    client: DsApiClient;
}

/** Default used only when a caller (existing tests unrelated to DS-backed tools) omits `ds` entirely. Never actually invoked by those tests — resolve_station/plan_journey are the only consumers, and Task 3/Task 6's own tests always supply a real (mocked) `ds`. */
const DEFAULT_DS_DEPS: DsDeps = { client: new DsApiClient({ baseUrl: 'http://ds.invalid' }) };

export interface ServerDeps {
    ldbws: LdbwsClient;
    timetable?: FindServicesDeps;
    plan?: PlanJourneyDeps;
    /** Optional so callers that never exercise resolve_station/plan_journey's DS-sourced behavior need not supply it — see DEFAULT_DS_DEPS. */
    ds?: DsDeps;
}

export function buildServer(deps: ServerDeps): McpServer {
    const server = new McpServer({ name: 'distant-signal-mcp', version: '0.1.0' });
    const ds = deps.ds ?? DEFAULT_DS_DEPS;
    registerResolveStation(server, ds.client);
    registerBoardTools(server, deps.ldbws);
    registerServiceDetail(server, deps.ldbws);
    registerFindServices(server, deps.timetable ?? DEFAULT_TIMETABLE_DEPS);
    registerPlanJourney(server, deps.plan ?? DEFAULT_PLAN_DEPS);
    return server;
}
```

`src/app.ts` — extend `AppOptions` and thread `ds` through to `buildServer` inside the `/mcp` handler:

```ts
export interface AppOptions {
    config: Config;
    verifier: OAuthTokenVerifier;
    ldbws: LdbwsClient;
    ds: DsApiClient;
}
```

Inside `buildApp`'s `/mcp` handler, where `buildServer` is currently called:

```ts
            const server = buildServer({
                ldbws,
                ds: { client: ds },
                timetable: { dbPath: config.timetableDbPath, maxResults: config.timetableMaxResults },
                plan: { dbPath: config.timetableDbPath, maxChanges: config.planMaxChanges, maxResults: config.planMaxResults }
            });
```

(`ds` here refers to the `AppOptions.ds` parameter destructured at the top of `buildApp`, alongside the existing `ldbws` destructure.)

- [ ] **Step 4: Wire `DsApiClient` into `src/index.ts`**

```ts
import { DsApiClient } from './ds/client.js';
// ... existing imports ...

const ds = new DsApiClient({ baseUrl: config.ds.baseUrl });

// ... existing ldbws credential probe stays unchanged ...

buildApp({ config, verifier, ldbws, ds }).listen(config.port, () => {
    console.log(`distant-signal-mcp listening on port ${config.port} (public URL ${config.publicUrl})`);
});
```

No boot-time DS probe is added here — unlike LDBWS's credentials, DS's routes need no credential to validate, so there is nothing analogous to fail loudly on at startup; a DS outage at runtime is handled entirely by Task 2's `DsUnavailableError` path.

- [ ] **Step 5: `test/ds-resolve-station.test.ts` — pure shim tests, mocked `DsApiClient`**

```ts
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { resolveStationViaDs } from '../src/stations/resolveViaDs.js';

function clientReturning(suggestions: Array<{ code: string; name: string }>): DsApiClient {
    const fetchImpl = (async () => new Response(JSON.stringify(suggestions), { status: 200 })) as unknown as typeof fetch;
    return new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
}

describe('resolveStationViaDs', () => {
    it('classifies an exact CRS match', async () => {
        const client = clientReturning([{ code: 'BTH', name: 'Bath Spa' }]);
        const { matches } = await resolveStationViaDs(client, 'bth', 5);
        expect(matches[0]).toMatchObject({ crs: 'BTH', matchType: 'exact_crs' });
    });

    it('classifies an exact name match case-insensitively', async () => {
        const client = clientReturning([{ code: 'BTH', name: 'Bath Spa' }]);
        const { matches } = await resolveStationViaDs(client, 'bath spa', 5);
        expect(matches[0]).toMatchObject({ crs: 'BTH', matchType: 'exact_name' });
    });

    it('degrades a partial match to substring, never alias or fuzzy', async () => {
        const client = clientReturning([{ code: 'BTH', name: 'Bath Spa' }]);
        const { matches } = await resolveStationViaDs(client, 'bath', 5);
        expect(matches[0]!.matchType).toBe('substring');
    });

    it('omits tiploc rather than fabricating it when the CRS is not in the local corpus', async () => {
        const client = clientReturning([{ code: 'ZZZ', name: 'Not A Real Station' }]);
        const { matches } = await resolveStationViaDs(client, 'zzz', 5);
        expect(matches[0]).not.toHaveProperty('tiploc');
    });

    it('looks up a real tiploc from the bundled corpus by CRS', async () => {
        const client = clientReturning([{ code: 'BTH', name: 'Bath Spa' }]);
        const { matches } = await resolveStationViaDs(client, 'bth', 5);
        expect(matches[0]!.tiploc).toBe('BATHSPA');
    });

    it('flags truncated when DS returns more than the requested limit', async () => {
        const client = clientReturning([
            { code: 'PAD', name: 'London Paddington' },
            { code: 'PAR', name: 'Paris Gare du Nord' }
        ]);
        const { matches, truncated } = await resolveStationViaDs(client, 'pa', 1);
        expect(matches).toHaveLength(1);
        expect(truncated).toBe(true);
    });

    it('does not flag truncated when everything fits under both DS\'s own cap and the requested limit', async () => {
        const client = clientReturning([{ code: 'BTH', name: 'Bath Spa' }]);
        const { truncated } = await resolveStationViaDs(client, 'bath spa', 5);
        expect(truncated).toBe(false);
    });

    it('flags truncated when DS returns exactly its own 20-result hard cap, even at the tool\'s own max limit of 20', async () => {
        const twenty = Array.from({ length: 20 }, (_, i) => ({ code: `S${i}`, name: `Station ${i}` }));
        const client = clientReturning(twenty);
        const { matches, truncated } = await resolveStationViaDs(client, 's', 20);
        expect(matches).toHaveLength(20);
        expect(truncated).toBe(true);
    });
});
```

- [ ] **Step 6: Rewrite `test/tools-resolve-station.test.ts`'s DS-facing parts**

Replace the file's `testClient`/`connect` setup to supply a mocked `DsApiClient` alongside the existing `LdbwsClient`:

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { LdbwsClient } from '../src/ldbws/client.js';
import { buildServer } from '../src/server.js';

const testProduct = (name: string) => ({
    baseUrl: `https://api.example.com/${name}/LDBWS/api/20220120`,
    apiKey: `${name}-key`
});

function dsClientReturning(suggestions: Array<{ code: string; name: string }>): DsApiClient {
    const fetchImpl = (async () => new Response(JSON.stringify(suggestions), { status: 200 })) as unknown as typeof fetch;
    return new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
}

async function connect(dsSuggestions: Array<{ code: string; name: string }>) {
    const ldbws = new LdbwsClient({
        departures: testProduct('dep'),
        arrivals: testProduct('arr'),
        serviceDetail: testProduct('svc'),
        fetchImpl: (async () => new Response('')) as typeof fetch,
        cacheTtlMs: 30_000
    });
    const server = buildServer({ ldbws, ds: { client: dsClientReturning(dsSuggestions) } });
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

describe('resolve_station tool', () => {
    it('is advertised by the server', async () => {
        const client = await connect([{ code: 'BTH', name: 'Bath Spa' }]);
        const { tools } = await client.listTools();
        expect(tools.map(tool => tool.name)).toContain('resolve_station');
    });

    it('resolves a station name to structured candidates sourced from DS', async () => {
        const client = await connect([{ code: 'BTH', name: 'Bath Spa' }]);
        const result = await client.callTool({ name: 'resolve_station', arguments: { query: 'bath spa' } });
        const structured = result.structuredContent as { matches: Array<{ crs: string; matchType: string }> };
        expect(structured.matches[0]?.crs).toBe('BTH');
        expect(structured.matches[0]?.matchType).toBe('exact_name');
    });

    it('flags truncation rather than silently dropping candidates', async () => {
        const client = await connect([
            { code: 'PAD', name: 'London Paddington' },
            { code: 'PAX', name: 'London Paxton (fixture)' }
        ]);
        const result = await client.callTool({ name: 'resolve_station', arguments: { query: 'london', limit: 1 } });
        const structured = result.structuredContent as { matches: unknown[]; truncated: boolean };
        expect(structured.matches).toHaveLength(1);
        expect(structured.truncated).toBe(true);
        expect(JSON.stringify(result.content)).toMatch(/more specific/i);
    });

    it('reports no matches without erroring', async () => {
        const client = await connect([]);
        const result = await client.callTool({ name: 'resolve_station', arguments: { query: 'zzzzqqqq' } });
        expect(result.isError).toBeFalsy();
        expect((result.structuredContent as { matches: unknown[] }).matches).toEqual([]);
    });

    it('surfaces a DS failure as a tool-level error, with no fallback', async () => {
        const failingFetch = (async () => new Response('boom', { status: 503 })) as unknown as typeof fetch;
        const failingDs = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl: failingFetch });
        const server = buildServer({
            ldbws: new LdbwsClient({
                departures: testProduct('dep'),
                arrivals: testProduct('arr'),
                serviceDetail: testProduct('svc'),
                fetchImpl: (async () => new Response('')) as typeof fetch
            }),
            ds: { client: failingDs }
        });
        const client = new Client({ name: 'test', version: '1.0.0' });
        const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
        await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);

        const result = await client.callTool({ name: 'resolve_station', arguments: { query: 'bath spa' } });
        expect(result.isError).toBeTruthy();
    });
});
```

- [ ] **Step 7: Run the full suite, confirm no unrelated fallout**

```bash
npm run typecheck && npm test
```

Expected: PASS. `test/tools-boards.test.ts`, `test/tools-service-detail.test.ts`, `test/tools-find-services.test.ts`, `test/app.test.ts`, `test/discord-auth.test.ts`, `test/resolve.test.ts` are unmodified and must still pass unchanged — they never touch `resolve_station` or `plan_journey`, so `ServerDeps.ds`'s default (`DEFAULT_DS_DEPS`) is never exercised by them. If any of those files fail to compile because `buildServer(...)`/`buildApp(...)` now requires a field they don't supply, that is a signal Step 3 made `ds` required rather than optional — re-check against the snippet above (`ds?: DsDeps`, not `ds: DsDeps`) before touching any of those five files.

- [ ] **Step 8: Commit**

```bash
git add src/stations/resolveViaDs.ts src/tools/resolve-station.ts src/server.ts src/app.ts src/index.ts test/ds-resolve-station.test.ts test/tools-resolve-station.test.ts
git commit -m "Migrate resolve_station onto DS's /public/stations, with a local tiploc/matchType shim (Decision 2)"
```

---

### Task 4: Leg → DS-line matching algorithm (Decision 3b) — pure, no network

**Files:**
- Create: `src/ds/lineMatch.ts`
- Test: `test/ds-line-match.test.ts` (new)

**Interfaces:**
- Produces: `MatchConfidence` type, `matchLegToLine(operator, fromCrs, toCrs, candidateLines, definitions): LineMatchResult`.
- Consumed by: Task 6's orchestration layer (`src/ds/annotateLeg.ts`), which supplies the pre-fetched `candidateLines`/`definitions` this function needs and never fetches anything itself.
- **Depends on:** Task 1 only. Independent of Task 2/3/5 — this task can be dispatched in parallel with any of them.

Mirrors DS's own `tfl_ids_to_overlay`/`overlay_for` precedent the design spec's Testing section cites — "pure so it's testable without a database" — applied here as "pure so it's testable without a network call."

- [ ] **Step 1: `src/ds/lineMatch.ts`**

```ts
import type { DsLineDefinition, DsLineSummary } from './client.js';

export type MatchConfidence = 'operatorAndBothStations' | 'operatorAndOneStation' | 'ambiguous' | 'none';

export interface LineMatchResult {
    matchConfidence: MatchConfidence;
    lineId?: string;
    /** Set only when matchConfidence === 'ambiguous'. */
    candidateLineIds?: string[];
}

/**
 * Resolves one train leg (operator + origin/destination CRS) to a DS
 * catalogue line — Decision 3b. Pure: takes the already-filtered
 * (`source !== 'tfl'`) candidate line list and their already-fetched
 * definitions as plain data, so this function never touches the network
 * or Task 5's cache directly — see src/ds/annotateLeg.ts (Task 6) for the
 * orchestration that supplies both.
 *
 * A leg's own operator/fromCrs/toCrs can each independently be `null`
 * (`JourneyLegDetails.operator`, `StationRef.crs` in plan-journey.ts are
 * all nullable) — every null case degrades to 'none' rather than guessing.
 */
export function matchLegToLine(
    operator: string | null,
    fromCrs: string | null,
    toCrs: string | null,
    candidateLines: DsLineSummary[],
    definitions: Map<string, DsLineDefinition>
): LineMatchResult {
    if (!operator) {
        return { matchConfidence: 'none' };
    }

    const byOperator = candidateLines.filter(line => line.operators.includes(operator));
    if (byOperator.length === 0) {
        return { matchConfidence: 'none' };
    }

    const bothStations: string[] = [];
    const oneStation: string[] = [];
    for (const line of byOperator) {
        const definition = definitions.get(line.id);
        if (!definition) {
            continue;
        }
        const hasFrom = fromCrs !== null && definition.stations.includes(fromCrs);
        const hasTo = toCrs !== null && definition.stations.includes(toCrs);
        if (hasFrom && hasTo) {
            bothStations.push(line.id);
        } else if (hasFrom || hasTo) {
            oneStation.push(line.id);
        }
    }

    if (bothStations.length === 1) {
        return { matchConfidence: 'operatorAndBothStations', lineId: bothStations[0] };
    }
    if (bothStations.length > 1) {
        // The real, documented lines/c2c.toml Barking/Upminster overlap:
        // two catalogue lines with the same operator whose station lists
        // both cover this leg's own origin and destination. Never guess
        // which one the leg actually ran on — report every candidate.
        return { matchConfidence: 'ambiguous', candidateLineIds: bothStations };
    }

    // Plan-level extension beyond the design spec's literal 3b text: the
    // spec's "only one CRS present" case describes attaching "the single
    // matching line" without addressing what happens when more than one
    // candidate independently matches only one of the leg's two stations.
    // Treating that as ambiguous too is the same "never guess past what
    // the data supports" posture the spec applies to the two-station case
    // — flagged here explicitly since it extends, rather than quotes, the
    // spec's own text.
    if (oneStation.length === 1) {
        return { matchConfidence: 'operatorAndOneStation', lineId: oneStation[0] };
    }
    if (oneStation.length > 1) {
        return { matchConfidence: 'ambiguous', candidateLineIds: oneStation };
    }

    return { matchConfidence: 'none' };
}
```

- [ ] **Step 2: `test/ds-line-match.test.ts`**

Fixtures modeled directly on the real `lines/c2c.toml`/`lines/overground-suffragette.toml`/`lines/overground-liberty.toml` overlap cited by the design spec's Current relevant state (CRS `BKG` in both `c2c` and `overground-suffragette`; CRS `UPM` in both `c2c` and `overground-liberty`):

```ts
import { describe, expect, it } from 'vitest';
import type { DsLineDefinition, DsLineSummary } from '../src/ds/client.js';
import { matchLegToLine } from '../src/ds/lineMatch.js';

const c2c: DsLineSummary = { id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' };
const suffragette: DsLineSummary = { id: 'overground-suffragette', name: 'Suffragette line', category: 'rail', operators: ['LO'], source: 'catalogue' };
const liberty: DsLineSummary = { id: 'overground-liberty', name: 'Liberty line', category: 'rail', operators: ['LO'], source: 'catalogue' };
const thameslink: DsLineSummary = { id: 'thameslink-core', name: 'Thameslink', category: 'rail', operators: ['TL'], source: 'catalogue' };
const tflBus: DsLineSummary = { id: 'tfl-victoria', name: 'Victoria line', category: 'tube', operators: ['LU'], source: 'tfl' };

const definitions = new Map<string, DsLineDefinition>([
    ['c2c', { operators: ['CC'], stations: ['FEN', 'BKG', 'UPM', 'SOE'] }],
    ['overground-suffragette', { operators: ['LO'], stations: ['GHY', 'BKG', 'WCHE'] }],
    ['overground-liberty', { operators: ['LO'], stations: ['UPM', 'ROM', 'EMR'] }],
    ['thameslink-core', { operators: ['TL'], stations: ['STP', 'BFR'] }]
]);

describe('matchLegToLine', () => {
    it('is confident when exactly one candidate line has both stations (operatorAndBothStations)', () => {
        const result = matchLegToLine('CC', 'FEN', 'SOE', [c2c, thameslink, tflBus], definitions);
        expect(result).toEqual({ matchConfidence: 'operatorAndBothStations', lineId: 'c2c' });
    });

    it('the real Barking/c2c/Suffragette overlap is reported as ambiguous, not guessed', () => {
        // A leg wouldn't genuinely have both stations on both lines in
        // real data (c2c doesn't serve GHY, Suffragette doesn't serve
        // SOE) -- this fixture constructs the overlap deliberately at the
        // single shared station, BKG, to exercise the documented
        // real-world case: two same-CRS-serving lines under operator
        // codes that both legitimately claim it.
        const overlapLines: DsLineSummary[] = [
            { id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC', 'LO'], source: 'catalogue' },
            { id: 'overground-suffragette', name: 'Suffragette line', category: 'rail', operators: ['CC', 'LO'], source: 'catalogue' }
        ];
        const overlapDefinitions = new Map<string, DsLineDefinition>([
            ['c2c', { operators: ['CC', 'LO'], stations: ['FEN', 'BKG'] }],
            ['overground-suffragette', { operators: ['CC', 'LO'], stations: ['GHY', 'BKG', 'FEN'] }]
        ]);
        const result = matchLegToLine('LO', 'FEN', 'BKG', overlapLines, overlapDefinitions);
        expect(result.matchConfidence).toBe('ambiguous');
        expect(result.candidateLineIds?.sort()).toEqual(['c2c', 'overground-suffragette']);
    });

    it('attaches the single matching line, flagged weaker, when only one CRS is a member (operatorAndOneStation)', () => {
        // c2c's own definition doesn't include a hypothetical far-end
        // station this leg's wider working continues to.
        const result = matchLegToLine('CC', 'FEN', 'ZZZ', [c2c, thameslink], definitions);
        expect(result).toEqual({ matchConfidence: 'operatorAndOneStation', lineId: 'c2c' });
    });

    it('reports ambiguous, not a guess, when more than one candidate matches only one station each', () => {
        const twoWeak: DsLineSummary[] = [
            { id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' },
            { id: 'other-cc-line', name: 'Other CC line', category: 'rail', operators: ['CC'], source: 'catalogue' }
        ];
        const twoWeakDefinitions = new Map<string, DsLineDefinition>([
            ['c2c', { operators: ['CC'], stations: ['FEN', 'BKG'] }],
            ['other-cc-line', { operators: ['CC'], stations: ['FEN', 'SOE'] }]
        ]);
        const result = matchLegToLine('CC', 'FEN', 'ZZZ', twoWeak, twoWeakDefinitions);
        expect(result.matchConfidence).toBe('ambiguous');
        expect(result.candidateLineIds?.sort()).toEqual(['c2c', 'other-cc-line']);
    });

    it('is none when the operator matches but neither CRS is a member of any candidate', () => {
        const result = matchLegToLine('CC', 'ZZZ', 'YYY', [c2c], definitions);
        expect(result).toEqual({ matchConfidence: 'none' });
    });

    it('is none when no candidate line has this operator at all', () => {
        const result = matchLegToLine('XX', 'FEN', 'SOE', [c2c, thameslink], definitions);
        expect(result).toEqual({ matchConfidence: 'none' });
    });

    it('is none when the leg has no operator', () => {
        const result = matchLegToLine(null, 'FEN', 'SOE', [c2c], definitions);
        expect(result).toEqual({ matchConfidence: 'none' });
    });

    it('is none when neither CRS is known (both null)', () => {
        const result = matchLegToLine('CC', null, null, [c2c], definitions);
        expect(result).toEqual({ matchConfidence: 'none' });
    });

    it('skips a candidate whose definition failed to fetch (not present in the map) rather than throwing', () => {
        const emptyDefinitions = new Map<string, DsLineDefinition>();
        const result = matchLegToLine('CC', 'FEN', 'SOE', [c2c], emptyDefinitions);
        expect(result).toEqual({ matchConfidence: 'none' });
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- ds-line-match`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ds/lineMatch.ts test/ds-line-match.test.ts
git commit -m "Add pure leg-to-DS-line matching algorithm, including the real Barking/c2c overlap case (Decision 3b)"
```

---

### Task 5: DS line catalogue caching layer (Decision 3d)

**Files:**
- Create: `src/ds/lineCatalogue.ts`
- Test: `test/ds-line-catalogue.test.ts` (new)

**Interfaces:**
- Produces: `DsLineCatalogue` class (`catalogueLines(): Promise<DsLineSummary[]>`, `definition(lineId): Promise<DsLineDefinition | null>`).
- Consumed by: Task 6's orchestration layer.
- **Depends on:** Task 2 (`DsApiClient`, and reuses `src/cache.ts`'s existing `TtlCache`).

Per Global Constraints: this reuses the fork's own existing `TtlCache` idiom (lazy per-key TTL, sweep-on-write, no timer — exactly `src/ldbws/client.ts`'s own board-cache technique) rather than adding a `setInterval` loop, which nothing in this codebase uses anywhere today. The effective behavior Decision 3d asks for — the catalogue is refetched at most once per `catalogueTtlMs` (default 15 minutes), not on every `plan_journey` call — is fully satisfied by this; only the literal "on an interval" *mechanism* differs from Decision 3d's wording, and only because a background timer would be a new pattern this fork doesn't otherwise have, not because the lazy form is functionally weaker.

- [ ] **Step 1: `src/ds/lineCatalogue.ts`**

```ts
import { TtlCache } from '../cache.js';
import type { DsApiClient, DsLineDefinition, DsLineSummary } from './client.js';

export interface DsLineCatalogueOptions {
    client: DsApiClient;
    ttlMs: number;
    now?: () => number;
}

const LINES_CACHE_KEY = 'catalogue-lines';

/**
 * Cached view of DS's National Rail catalogue (Decision 3d): the full
 * `GET /public/lines` list, filtered to `source !== 'tfl'` (TfL-mode rows
 * have no relevance to matching a National Rail journey plan's own
 * operator codes — Decision 3b step 2), and each catalogue line's
 * `GET /public/lines/{id}/definition`. Refetched at most once per
 * `ttlMs` — see this task's own header note on why this uses the same
 * lazy TtlCache idiom src/ldbws/client.ts already uses, not a background
 * timer.
 */
export class DsLineCatalogue {
    private readonly client: DsApiClient;
    private readonly linesCache: TtlCache<DsLineSummary[]>;
    private readonly definitionCache: TtlCache<DsLineDefinition>;

    constructor(options: DsLineCatalogueOptions) {
        this.client = options.client;
        this.linesCache = new TtlCache<DsLineSummary[]>(options.ttlMs, options.now);
        this.definitionCache = new TtlCache<DsLineDefinition>(options.ttlMs, options.now);
    }

    /** Non-TfL catalogue lines. Propagates a DS failure to the caller (Task 6 treats an uncaught rejection here the same as any other DS-call failure — `liveStatus: { source: 'unavailable', matchConfidence: 'none' }`). */
    async catalogueLines(): Promise<DsLineSummary[]> {
        const cached = this.linesCache.get(LINES_CACHE_KEY);
        if (cached) {
            return cached;
        }
        const all = await this.client.getLines();
        const catalogue = all.filter(line => line.source !== 'tfl');
        this.linesCache.set(LINES_CACHE_KEY, catalogue);
        return catalogue;
    }

    /**
     * One catalogue line's station/operator definition. Returns `null`
     * (never throws) on a DS failure or a dangling id — Decision 3d's
     * "cached catalogue going stale ... degrades to a missed match ...
     * treated the same as any other DS-call failure above, not a crash."
     * This is the one method on this class that swallows its own errors,
     * deliberately: a single candidate line's definition failing to fetch
     * should not abort matching against every OTHER candidate for the
     * same leg.
     */
    async definition(lineId: string): Promise<DsLineDefinition | null> {
        const cached = this.definitionCache.get(lineId);
        if (cached) {
            return cached;
        }
        try {
            const def = await this.client.getLineDefinition(lineId);
            this.definitionCache.set(lineId, def);
            return def;
        } catch {
            return null;
        }
    }
}
```

- [ ] **Step 2: `test/ds-line-catalogue.test.ts`**

```ts
import { describe, expect, it, vi } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { DsLineCatalogue } from '../src/ds/lineCatalogue.js';

function jsonResponse(body: unknown, status = 200): Response {
    return new Response(JSON.stringify(body), { status });
}

describe('DsLineCatalogue', () => {
    it('filters out tfl-sourced lines from catalogueLines', async () => {
        const fetchImpl = (async () =>
            jsonResponse([
                { id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' },
                { id: 'tfl-victoria', name: 'Victoria line', category: 'tube', operators: ['LU'], source: 'tfl' }
            ])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const lines = await catalogue.catalogueLines();
        expect(lines.map(l => l.id)).toEqual(['c2c']);
    });

    it('refetches at most once per ttlMs', async () => {
        let now = 0;
        const fetchImpl = vi.fn(async () => jsonResponse([{ id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' }])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 1000, now: () => now });

        await catalogue.catalogueLines();
        await catalogue.catalogueLines();
        expect(fetchImpl).toHaveBeenCalledTimes(1);

        now = 2000;
        await catalogue.catalogueLines();
        expect(fetchImpl).toHaveBeenCalledTimes(2);
    });

    it('caches a definition per line id independently', async () => {
        const fetchImpl = vi.fn(async (url: string) => {
            const stations = url.includes('c2c') ? ['FEN', 'BKG'] : ['GHY', 'BKG'];
            return jsonResponse({ operators: ['CC'], stations });
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });

        const c2cDef = await catalogue.definition('c2c');
        const suffragetteDef = await catalogue.definition('overground-suffragette');
        await catalogue.definition('c2c'); // cached, no extra fetch

        expect(c2cDef?.stations).toEqual(['FEN', 'BKG']);
        expect(suffragetteDef?.stations).toEqual(['GHY', 'BKG']);
        expect(fetchImpl).toHaveBeenCalledTimes(2);
    });

    it('returns null, not a throw, when a definition fetch fails', async () => {
        const fetchImpl = (async () => new Response('nope', { status: 500 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        await expect(catalogue.definition('c2c')).resolves.toBeNull();
    });

    it('propagates a catalogueLines failure to the caller (no silent swallow at this layer)', async () => {
        const fetchImpl = (async () => new Response('nope', { status: 500 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        await expect(catalogue.catalogueLines()).rejects.toThrow();
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- ds-line-catalogue`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ds/lineCatalogue.ts test/ds-line-catalogue.test.ts
git commit -m "Add TTL-cached DS line catalogue, reusing src/cache.ts's existing TtlCache (Decision 3d)"
```

---

### Task 6: Wire delay-aware annotation into `plan_journey` (Decisions 3a, 3c, error handling)

**Files:**
- Create: `src/ds/annotateLeg.ts`
- Modify: `src/tools/plan-journey.ts`, `src/server.ts`
- Test: `test/ds-annotate-leg.test.ts` (new, orchestration + scope-check unit tests), `test/tools-plan-journey.test.ts` (extend with end-to-end DS-annotation and failure-fallback cases)

**Interfaces:**
- Produces: `annotateLeg(leg: RenderedTrainLeg, deps: AnnotateLegDeps, now: Date): Promise<LiveStatus | undefined>`, the `LiveStatus` output-schema type, `LIVE_BOARD_HORIZON_MINUTES = 239`.
- Consumes: Task 4's `matchLegToLine`, Task 5's `DsLineCatalogue`, Task 2's `DsApiClient`.
- **Depends on:** Task 4, Task 5 (transitively Task 2).

**Two necessary resolutions of gaps in the design spec's own `liveStatus` schema (3c), not literal quotes from it — both flagged explicitly since the schema as written is ambiguous on exactly these two points:**

1. **`source`'s three named values (`'ds_line_status' | 'ds_tracked_train' | 'unavailable'`) have no value for "DS answered successfully but no line could be matched"** (`matchConfidence: 'none'` or `'ambiguous'`, no status fetch attempted at all). Using `'unavailable'` there would be actively wrong — DS didn't fail, there was simply nothing to fetch a status for. This task adds a fourth value, `'no_match'`, for exactly that case.
2. **`asOf` is written as always-present (`asOf: string`) but there is no computed_at to report when no status fetch happened** (the `'no_match'`/`'unavailable'` cases). This task makes it `asOf?: string`, omitted in both of those cases.

- [ ] **Step 1: `src/ds/annotateLeg.ts`**

```ts
import type { DsApiClient, DsDisruption } from './client.js';
import type { DsLineCatalogue } from './lineCatalogue.js';
import { matchLegToLine, type MatchConfidence } from './lineMatch.js';

/**
 * How far ahead Darwin's own live boards can see — train-mcp's own
 * measured figure (`docs/superpowers/specs/2026-07-21-train-mcp-phase1-design.md:340-341`:
 * "about 239 minutes — just under four hours"), reused here as a bare
 * number. No such constant exists anywhere in this fork's own code today
 * (confirmed by grep) — this is a fresh definition, not an import,
 * despite Decision 3a's "reused, not re-derived" framing referring only
 * to the *figure*, not an existing symbol. Applied here as a general "is
 * this leg near enough to now for current line-status data to be
 * meaningfully informative" gate, not because DS's own line-status data
 * source is itself horizon-limited the way an individual train's live
 * board is — an active incident's current severity says nothing
 * meaningful about a leg three days out, so the same bound is reused for
 * that reason.
 */
export const LIVE_BOARD_HORIZON_MINUTES = 239;

export type LiveStatusSource = 'ds_line_status' | 'ds_tracked_train' | 'unavailable' | 'no_match';

export interface LiveStatus {
    matchConfidence: MatchConfidence;
    lineId?: string;
    candidateLineIds?: string[];
    severity?: string;
    reason?: string;
    dataQuality?: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
    sampleStats?: { total: number; delayed: number; cancelled: number; skipped: number; avgDelayMinutes: number };
    legInDisruptionScope?: boolean;
    asOf?: string;
    source: LiveStatusSource;
}

export interface AnnotateLegDeps {
    client: DsApiClient;
    catalogue: DsLineCatalogue;
}

/** Minimal shape this function needs from plan-journey.ts's own `RenderedTrainLeg` — see that file's Step 2 for how the real type satisfies this. */
export interface AnnotatableLeg {
    operator: string | null;
    fromCrs: string | null;
    toCrs: string | null;
    departureAt: string | null;
}

function legInDisruptionScope(disruption: DsDisruption | undefined, fromCrs: string | null, toCrs: string | null): boolean | undefined {
    if (!disruption) {
        return undefined;
    }
    if (fromCrs === null && toCrs === null) {
        return undefined;
    }
    const stops = new Set(disruption.affectedStops);
    const inStops = (fromCrs !== null && stops.has(fromCrs)) || (toCrs !== null && stops.has(toCrs));
    const inRoutes = disruption.affectedRoutes.some(
        route => (fromCrs !== null && (route.from === fromCrs || route.to === fromCrs)) || (toCrs !== null && (route.from === toCrs || route.to === toCrs))
    );
    return inStops || inRoutes;
}

/**
 * Best-effort DS-sourced annotation for one TrainLeg (Decision 3). Never
 * throws — every internal failure degrades to `{ source: 'unavailable',
 * matchConfidence: 'none' }`, per this plan's Global Constraints. Returns
 * `undefined` (no `liveStatus` field at all, rather than a degraded
 * object) only for the two cases that are not failures: the leg is
 * outside the live-board horizon, or `departureAt` could not be resolved
 * at all.
 */
export async function annotateLeg(leg: AnnotatableLeg, deps: AnnotateLegDeps, now: Date): Promise<LiveStatus | undefined> {
    if (leg.departureAt === null) {
        return undefined;
    }
    const minutesUntilDeparture = (Date.parse(leg.departureAt) - now.getTime()) / 60_000;
    if (minutesUntilDeparture > LIVE_BOARD_HORIZON_MINUTES) {
        return undefined;
    }

    try {
        const candidateLines = await deps.catalogue.catalogueLines();
        const byOperator = leg.operator ? candidateLines.filter(line => line.operators.includes(leg.operator!)) : [];
        const definitionEntries = await Promise.all(
            byOperator.map(async line => [line.id, await deps.catalogue.definition(line.id)] as const)
        );
        const definitions = new Map(definitionEntries.filter((entry): entry is [string, NonNullable<(typeof entry)[1]>] => entry[1] !== null));

        const match = matchLegToLine(leg.operator, leg.fromCrs, leg.toCrs, byOperator, definitions);

        if (match.matchConfidence === 'none' || match.matchConfidence === 'ambiguous') {
            return { ...match, source: 'no_match' };
        }

        const report = await deps.client.getLineStatus(match.lineId!);
        const status = report.lineStatuses[0];
        if (!status) {
            return { ...match, source: 'no_match' };
        }

        return {
            ...match,
            severity: status.statusSeverityDescription,
            reason: status.reason,
            dataQuality: status.dataQuality,
            sampleStats: status.sampleStats,
            legInDisruptionScope: legInDisruptionScope(status.disruption, leg.fromCrs, leg.toCrs),
            asOf: report.computedAt,
            source: 'ds_line_status'
        };
    } catch {
        return { matchConfidence: 'none', source: 'unavailable' };
    }
}
```

- [ ] **Step 2: Wire the annotation into `src/tools/plan-journey.ts`**

Add `liveStatus` to `RenderedTrainLeg` and thread an `AnnotateLegDeps` through the render path. `RenderedTrainLeg` already carries `operator: string | null` and `from.crs`/`to.crs: string | null` (`StationRef.crs`, produced by `stationRef`'s call to `store.crsForTiploc()`) — this task reuses those existing fields as the `AnnotatableLeg` input, per this plan's Status note #1: no new CRS-resolution step is added, since `enrichLeg` already computes it.

Extend the interface (`plan-journey.ts`, near line 157):

```ts
interface RenderedTrainLeg {
    kind: 'train';
    from: StationRef;
    to: StationRef;
    departure: string;
    arrival: string;
    departureAt: string | null;
    arrivalAt: string | null;
    departurePlatform: string | null;
    arrivalPlatform: string | null;
    operator: string | null;
    identity: string | null;
    uid: string;
    originTiploc: string;
    originName: string | null;
    destinationTiploc: string;
    destinationName: string | null;
    /** Best-effort DS-sourced delay annotation (Decision 3) — absent when the leg is outside the live-board horizon, or when annotation was never attempted (a TransferLeg never reaches this field's assignment at all). */
    liveStatus?: LiveStatus;
}
```

Add the import at the top of the file:

```ts
import { annotateLeg, type AnnotateLegDeps, type LiveStatus } from '../ds/annotateLeg.js';
```

`enrichLeg` becomes `async` and takes `deps: AnnotateLegDeps` (the function currently returns `RenderedLeg` synchronously — `plan-journey.ts:214-261`):

```ts
async function enrichLeg(
    store: TimetableStore,
    leg: JourneyLeg,
    dayAnchor: Date,
    previousArrival: Date | null,
    deps: AnnotateLegDeps
): Promise<RenderedLeg> {
    const reference = previousArrival ?? dayAnchor;

    if (leg.kind === 'transfer') {
        // ... unchanged transfer-leg branch, exactly as today ...
        // (TransferLegs are never annotated — Decision 3a — so this
        // branch's return value gains no new field.)
    }

    const details: JourneyLegDetails = store.journeyLegDetails(leg);
    const departureAt = toInstant(leg.departure, reference);
    const arrivalAt = departureAt ? toInstant(leg.arrival, new Date(departureAt)) : toInstant(leg.arrival, reference);
    const from = stationRef(store, leg.fromTiploc, details.fromName);
    const to = stationRef(store, leg.toTiploc, details.toName);
    const liveStatus = await annotateLeg(
        { operator: details.operator, fromCrs: from.crs, toCrs: to.crs, departureAt },
        deps,
        new Date()
    );
    return {
        kind: 'train',
        from,
        to,
        departure: leg.departure,
        arrival: leg.arrival,
        departureAt,
        arrivalAt,
        departurePlatform: details.fromPlatform,
        arrivalPlatform: details.toPlatform,
        operator: details.operator,
        identity: details.identity,
        uid: leg.uid,
        originTiploc: details.originTiploc,
        originName: details.originName,
        destinationTiploc: details.destinationTiploc,
        destinationName: details.destinationName,
        ...(liveStatus ? { liveStatus } : {})
    };
}
```

`enrichLegs` becomes `async` and awaits each leg in order (kept sequential, matching its existing sequential `previousArrival`-chaining dependency — do not parallelize this loop, since each leg's instant resolution depends on the previous one's):

```ts
async function enrichLegs(store: TimetableStore, legs: JourneyLeg[], date: string, deps: AnnotateLegDeps): Promise<RenderedLeg[]> {
    const dayAnchor = new Date(`${date}T12:00:00.000Z`);
    const rendered: RenderedLeg[] = [];
    let previousArrival: Date | null = null;
    for (const leg of legs) {
        const enriched = await enrichLeg(store, leg, dayAnchor, previousArrival, deps);
        rendered.push(enriched);
        previousArrival = enriched.arrivalAt ? new Date(enriched.arrivalAt) : previousArrival;
    }
    return rendered;
}
```

`renderJourneyBody` becomes `async` and threads `deps` through (`plan-journey.ts:560-571`):

```ts
async function renderJourneyBody(store: TimetableStore, legs: JourneyLeg[], date: string, deps: AnnotateLegDeps): Promise<{ text: string; rendered: RenderedLeg[] }> {
    const rendered = await enrichLegs(store, legs, date, deps);
    // ... rest of the function body is unchanged (the lines/interchange assembly loop doesn't touch DS at all) ...
}
```

`renderFastest` and `renderOptions` (and their shared caller `handlePlanJourney`) each gain an `await` at their existing `renderJourneyBody(...)` call sites and thread `deps` through their own argument objects (`FastestArgs`/`OptionsArgs` each gain a `deps: AnnotateLegDeps` field). `handlePlanJourney` itself gains a `deps: AnnotateLegDeps` parameter, and `registerPlanJourney` passes it through:

```ts
export interface PlanJourneyDeps {
    dbPath: string;
    maxChanges: number;
    maxResults: number;
    ds: AnnotateLegDeps;
}
```

```ts
async function handlePlanJourney(deps: PlanJourneyDeps, request: PlanJourneyRequest) {
    // ... unchanged up to the renderFastest/renderOptions call sites ...
    if (results === 'fastest') {
        return renderFastest({ /* ...existing fields..., */ deps: deps.ds });
    }
    // ...
    return renderOptions({ /* ...existing fields..., */ deps: deps.ds });
}
```

Update `JOURNEY_FRAME_NOTE` (`plan-journey.ts:526-530`) — its current text is now inaccurate once `liveStatus` can be present:

```ts
const JOURNEY_FRAME_NOTE =
    'Times above are local (Europe/London) timetable clock times, not UTC instants. An arrival on a later ' +
    'calendar date than the request is marked, e.g. "(+1 day)". A journey plan is built from the planned ' +
    'timetable, exactly as find_services is. Some train legs departing soon carry a best-effort liveStatus ' +
    'field sourced from Distant Signal\'s own live line-status data — absent when no confident match or no ' +
    'current disruption exists, and never a guarantee: it reflects a line\'s overall current status, checked ' +
    'against this leg\'s own stations where possible (see legInDisruptionScope), not a per-train running check.';
```

Finally, extend the `trainLegShape` Zod output schema (`plan-journey.ts`, near line 1132-1149) with the new field:

```ts
const liveStatusShape = z.object({
    matchConfidence: z.enum(['operatorAndBothStations', 'operatorAndOneStation', 'ambiguous', 'none']),
    lineId: z.string().optional(),
    candidateLineIds: z.array(z.string()).optional(),
    severity: z.string().optional(),
    reason: z.string().optional(),
    dataQuality: z.enum(['knowledgebase', 'ldbws-inferred', 'trust-inferred', 'planned', 'tfl']).optional(),
    sampleStats: z
        .object({ total: z.number(), delayed: z.number(), cancelled: z.number(), skipped: z.number(), avgDelayMinutes: z.number() })
        .optional(),
    legInDisruptionScope: z.boolean().optional(),
    asOf: z.string().optional(),
    source: z.enum(['ds_line_status', 'ds_tracked_train', 'unavailable', 'no_match'])
});

const trainLegShape = {
    // ...every existing field, unchanged...
    liveStatus: liveStatusShape.optional()
};
```

- [ ] **Step 3: Extend `ServerDeps`/`server.ts`'s `DEFAULT_PLAN_DEPS` to carry `ds`**

```ts
const DEFAULT_PLAN_DEPS: PlanJourneyDeps = {
    dbPath: './data/timetable.sqlite',
    maxChanges: 4,
    maxResults: 5,
    ds: { client: new DsApiClient({ baseUrl: 'http://ds.invalid' }), catalogue: new DsLineCatalogue({ client: new DsApiClient({ baseUrl: 'http://ds.invalid' }), ttlMs: 15 * 60 * 1000 }) }
};
```

And in `buildServer`, when a caller supplies `deps.ds` (Task 3's `DsDeps`), build a matching `DsLineCatalogue` for `plan_journey` to share:

```ts
export function buildServer(deps: ServerDeps): McpServer {
    const server = new McpServer({ name: 'distant-signal-mcp', version: '0.1.0' });
    const ds = deps.ds ?? DEFAULT_DS_DEPS;
    const catalogue = new DsLineCatalogue({ client: ds.client, ttlMs: deps.dsCatalogueTtlMs ?? 15 * 60 * 1000 });
    registerResolveStation(server, ds.client);
    registerBoardTools(server, deps.ldbws);
    registerServiceDetail(server, deps.ldbws);
    registerFindServices(server, deps.timetable ?? DEFAULT_TIMETABLE_DEPS);
    registerPlanJourney(server, {
        ...(deps.plan ?? { dbPath: DEFAULT_TIMETABLE_DEPS.dbPath, maxChanges: 4, maxResults: 5 }),
        ds: { client: ds.client, catalogue }
    });
    return server;
}
```

Add `dsCatalogueTtlMs?: number` to `ServerDeps` (threaded from `Config.ds.catalogueTtlMs` in `app.ts`/`index.ts`, matching how `config.timetableDbPath` already reaches `DEFAULT_TIMETABLE_DEPS`'s override).

- [ ] **Step 4: `test/ds-annotate-leg.test.ts` — orchestration + scope-check unit tests**

```ts
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { DsLineCatalogue } from '../src/ds/lineCatalogue.js';
import { annotateLeg, LIVE_BOARD_HORIZON_MINUTES } from '../src/ds/annotateLeg.js';

function clientReturning(routes: Record<string, unknown>): DsApiClient {
    const fetchImpl = (async (url: string) => {
        for (const [fragment, body] of Object.entries(routes)) {
            if (url.includes(fragment)) {
                return new Response(JSON.stringify(body), { status: 200 });
            }
        }
        return new Response('not found', { status: 404 });
    }) as unknown as typeof fetch;
    return new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
}

const now = new Date('2026-09-01T12:00:00Z');

describe('annotateLeg', () => {
    it('returns undefined (not a degraded object) for a leg outside the live-board horizon', async () => {
        const client = clientReturning({});
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const farDeparture = new Date(now.getTime() + (LIVE_BOARD_HORIZON_MINUTES + 1) * 60_000).toISOString();
        const result = await annotateLeg({ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: farDeparture }, { client, catalogue }, now);
        expect(result).toBeUndefined();
    });

    it('returns undefined when departureAt could not be resolved at all', async () => {
        const client = clientReturning({});
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg({ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: null }, { client, catalogue }, now);
        expect(result).toBeUndefined();
    });

    it('annotates a confident match with source ds_line_status inside the horizon', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({
            '/public/lines/': { operators: ['CC'], stations: ['FEN', 'BKG'] },
            '/public/lines': [{ id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' }],
            '/Line/c2c/Status': [
                {
                    id: 'c2c',
                    name: 'c2c',
                    modeName: 'rail',
                    operators: ['CC'],
                    computedAt: '2026-09-01T11:55:00Z',
                    lineStatuses: [
                        {
                            statusSeverity: 5,
                            statusSeverityDescription: 'MinorDelays',
                            reason: 'Minor delays due to a signalling fault',
                            dataQuality: 'knowledgebase',
                            validityPeriods: [{ fromDate: '2026-09-01T10:00:00Z', toDate: null, isNow: true }],
                            disruption: { category: 'RealTime', description: 'Signal fault at Barking', affectedStops: ['BKG'], affectedRoutes: [], source: 'knowledgebase-incident-1' }
                        }
                    ]
                }
            ]
        });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg({ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture }, { client, catalogue }, now);
        expect(result).toMatchObject({
            matchConfidence: 'operatorAndBothStations',
            lineId: 'c2c',
            source: 'ds_line_status',
            severity: 'MinorDelays',
            legInDisruptionScope: true,
            asOf: '2026-09-01T11:55:00Z'
        });
    });

    it('legInDisruptionScope is false when the leg\'s own stations are outside the disruption\'s named scope', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({
            '/public/lines/': { operators: ['CC'], stations: ['FEN', 'BKG', 'SOE'] },
            '/public/lines': [{ id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' }],
            '/Line/c2c/Status': [
                {
                    id: 'c2c', name: 'c2c', modeName: 'rail', operators: ['CC'], computedAt: '2026-09-01T11:55:00Z',
                    lineStatuses: [
                        {
                            statusSeverity: 5, statusSeverityDescription: 'MinorDelays', reason: 'Delays near Southend',
                            dataQuality: 'knowledgebase', validityPeriods: [{ fromDate: '2026-09-01T10:00:00Z', toDate: null, isNow: true }],
                            disruption: { category: 'RealTime', description: 'Fault near Southend', affectedStops: ['SOE'], affectedRoutes: [], source: 'knowledgebase-incident-2' }
                        }
                    ]
                }
            ]
        });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg({ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture }, { client, catalogue }, now);
        expect(result?.legInDisruptionScope).toBe(false);
        expect(result?.severity).toBe('MinorDelays'); // the line's overall severity is still attached regardless
    });

    it('degrades to source unavailable, never throws, on a DS failure anywhere in the loop', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const failingFetch = (async () => new Response('boom', { status: 500 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl: failingFetch });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg({ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture }, { client, catalogue }, now);
        expect(result).toEqual({ matchConfidence: 'none', source: 'unavailable' });
    });

    it('reports source no_match, not unavailable, when DS answers fine but no line matches', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({ '/public/lines': [] });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg({ operator: 'ZZ', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture }, { client, catalogue }, now);
        expect(result).toEqual({ matchConfidence: 'none', source: 'no_match' });
    });
});
```

- [ ] **Step 5: Extend `test/tools-plan-journey.test.ts`**

Add an end-to-end case that a `plan_journey` call carries `liveStatus` on a matched leg, and a fallback case confirming a DS outage never fails the whole plan. Both reuse this file's existing `twoLegFixture`/`namedStations`/timetable-writer setup — only the `buildServer({...})` call sites need a `ds` dep added:

```ts
import { DsApiClient } from '../src/ds/client.js';
import { DsLineCatalogue } from '../src/ds/lineCatalogue.js';
// ... alongside the file's existing imports ...

function dsDeps(routes: Record<string, unknown> = {}): { client: DsApiClient } {
    const fetchImpl = (async (url: string) => {
        for (const [fragment, body] of Object.entries(routes)) {
            if (url.includes(fragment)) {
                return new Response(JSON.stringify(body), { status: 200 });
            }
        }
        return new Response(JSON.stringify([]), { status: 200 });
    }) as unknown as typeof fetch;
    return { client: new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }) };
}
```

```ts
    it('annotates a matched train leg with a DS-sourced liveStatus', async () => {
        // ... reuse this file's existing twoLegFixture()/writer setup to seed `path` ...
        const server = buildServer({
            ldbws: /* this file's existing stub LdbwsClient construction */,
            ds: dsDeps({
                '/public/lines/': { operators: ['GW'], stations: ['PAD', 'RDG'] },
                '/public/lines': [{ id: 'gwr-main', name: 'GWR Main Line', category: 'rail', operators: ['GW'], source: 'catalogue' }],
                '/Line/gwr-main/Status': [
                    { id: 'gwr-main', name: 'GWR Main Line', modeName: 'rail', operators: ['GW'], computedAt: '2026-01-01T00:00:00Z', lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'GoodService', reason: 'Good Service', dataQuality: 'ldbws-inferred', validityPeriods: [] }] }
                ]
            }),
            plan: { dbPath: path, maxChanges: 4, maxResults: 5 }
        });
        // ... connect an MCP client, call plan_journey exactly as this file's existing "finds a two-leg journey" test does ...
        // assert structuredContent.journeys[0].legs[0].liveStatus?.matchConfidence === 'operatorAndBothStations'
    });

    it('a DS outage never fails the surrounding plan_journey call', async () => {
        const failingFetch = (async () => new Response('boom', { status: 503 })) as unknown as typeof fetch;
        const server = buildServer({
            ldbws: /* stub as above */,
            ds: { client: new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl: failingFetch }) },
            plan: { dbPath: path, maxChanges: 4, maxResults: 5 }
        });
        // ... call plan_journey as above ...
        // assert result.isError is falsy, journeys[0] is still present with legs,
        // and journeys[0].legs[0].liveStatus is either absent or { matchConfidence: 'none', source: 'unavailable' }
    });
```

(These two new tests are written against this file's own existing fixture-setup conventions — `mkdtempSync`, `TimetableWriter`, `namedStations`, `twoLegFixture` — already present earlier in the file; wire the DS deps in exactly as sketched above, matching the existing tests' own `buildServer({...})` call shape.)

- [ ] **Step 6: Run the full suite**

```bash
npm run typecheck && npm test
```

Expected: PASS, including every existing `plan_journey`/`find_services` test unmodified in behavior (only their `buildServer(...)` call sites gain a `ds` dep where the file already constructs a server directly, per Step 5).

- [ ] **Step 7: Commit**

```bash
git add src/ds/annotateLeg.ts src/tools/plan-journey.ts src/server.ts test/ds-annotate-leg.test.ts test/tools-plan-journey.test.ts
git commit -m "Wire best-effort DS-sourced liveStatus annotation into plan_journey (Decision 3)"
```

---

### Task 7: Helm subchart deployment — `charts/distant-signal/templates/railmcp-*.yaml` (Decision 6, first half)

**Files (all inside THIS repo, `/workspaces/github-com-fasterspeeding-network-rail-status/`):**
- Create: `charts/distant-signal/templates/railmcp-deployment.yaml`, `charts/distant-signal/templates/railmcp-service.yaml`
- Modify: `charts/distant-signal/values.yaml`

**Interfaces:**
- Produces: an opt-in (`railMcp.enabled`, default `false`) Deployment + ClusterIP Service for the derived MCP service, following `devauthentik-server-deployment.yaml`'s external-image pattern (simpler than `schedulefeed-deployment.yaml`'s multi-container pod — this service has no paired verifier container).
- **Depends on:** Task 2 (needs the `DS_API_BASE_URL` env var name settled).

- [ ] **Step 1: Add a `railMcp:` block to `charts/distant-signal/values.yaml`**

Insert after the existing `scheduleFeed:` block (`values.yaml:629-...`), following `devAuthentik.image`'s external-registry-path pattern (`values.yaml:409-416`) exactly — a full registry path with `{repository, tag, pullPolicy}`, not a bare `distant-signal/<name>` short name, since this repo's own CI does not build this image (Decision 1):

```yaml
railMcp:
  # -- Opt-in; false by default, matching scheduleFeed.enabled's own
  # pattern. This chart never builds this image itself -- its source
  # lives in the derived service's own repository (Task 1), forked from
  # train-mcp -- see docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md
  # Decision 1.
  enabled: false
  image:
    # -- Full external registry path, like devAuthentik.image.repository
    # (ghcr.io/goauthentik/server) -- NOT a bare distant-signal/<name>
    # short name, since this repo's own CI/CD does not build or publish
    # this image (Decision 1/6).
    repository: ghcr.io/CHANGE-ME/distant-signal-mcp
    tag: ""
    pullPolicy: IfNotPresent
  service:
    port: 3000
  # -- The MCP service's own PUBLIC_URL (its OAuth discovery metadata
  # needs a real, reachable origin -- see the fork's own config.ts). Left
  # blank by default; an operator enabling railMcp must set this to
  # wherever they expose the railmcp Service (an Ingress/LoadBalancer is
  # deliberately not sketched by this chart -- see this plan's Task 7,
  # matching Decision 6's own shallow deployment depth).
  publicUrl: ""
  # -- The Discord application this MCP server's OAuth resource-server
  # verification checks tokens against -- the fork's own DISCORD_CLIENT_ID/
  # DISCORD_ALLOWED_USER_IDS. Neither has a sensible chart-wide default;
  # both are required by the fork's own config.ts at boot.
  discord:
    clientId: ""
    allowedUserIds: ""
  # -- LDBWS/Rail Data Marketplace credentials the fork's board/service-
  # detail tools still call directly (Decision 5 -- unchanged by this
  # integration). Six required values, unchanged from the fork's own
  # README.md Configuration table.
  ldbws:
    departuresUrl: ""
    departuresKey: ""
    arrivalsUrl: ""
    arrivalsKey: ""
    serviceUrl: ""
    serviceKey: ""
  # -- How long Task 5's DS line-catalogue cache trusts one fetch before
  # refetching -- DS_LINE_CATALOGUE_TTL_MS, an unresearched starting
  # figure per Decision 3d's own Open questions/risks entry.
  lineCatalogueTtlMs: 900000
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

- [ ] **Step 2: `charts/distant-signal/templates/railmcp-deployment.yaml`**

```yaml
{{/*
The derived MCP service (Task 1: forked from train-mcp, own repository,
own CI/tests -- see docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md
Decision 1). ONE container, unlike schedulefeed-deployment.yaml's paired
sftp/ingest pod -- this service has no verifier counterpart.
*/}}
{{- if .Values.railMcp.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "distant-signal.railMcpFullname" . }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "railmcp") | nindent 4 }}
spec:
  replicas: 1
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "railmcp") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "railmcp") | nindent 8 }}
      {{- with .Values.railMcp.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "distant-signal.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "distant-signal.podSecurityContext" (dict "override" .Values.railMcp.podSecurityContext) | nindent 8 }}
      containers:
        - name: railmcp
          image: {{ printf "%s:%s" .Values.railMcp.image.repository (default .Chart.AppVersion .Values.railMcp.image.tag) | quote }}
          imagePullPolicy: {{ .Values.railMcp.image.pullPolicy }}
          securityContext:
            # This is a third-party-built image this chart's own security
            # posture cannot verify at render time -- same
            # readOnlyRootFilesystem: false stance devauthentik-server-
            # deployment.yaml takes for ghcr.io/goauthentik/server, for
            # the same reason (the fork's own Node process may write
            # beyond what this chart controls; not independently verified
            # against a real running container this pass).
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" false) | nindent 12 }}
          ports:
            - name: http
              containerPort: {{ .Values.railMcp.service.port }}
              protocol: TCP
          env:
            - name: PORT
              value: {{ .Values.railMcp.service.port | quote }}
            - name: PUBLIC_URL
              value: {{ .Values.railMcp.publicUrl | quote }}
            # In-cluster DNS name for this chart's own `api` Service --
            # no new DS-side route or auth needed (Decision 4: every DS
            # call this service makes is anonymous).
            - name: DS_API_BASE_URL
              value: {{ include "distant-signal.apiBaseUrl" . | quote }}
            - name: DS_LINE_CATALOGUE_TTL_MS
              value: {{ .Values.railMcp.lineCatalogueTtlMs | quote }}
            - name: DISCORD_CLIENT_ID
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: discord-client-id
            - name: DISCORD_ALLOWED_USER_IDS
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: discord-allowed-user-ids
            - name: LDBWS_DEPARTURES_URL
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-departures-url }
            - name: LDBWS_DEPARTURES_KEY
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-departures-key }
            - name: LDBWS_ARRIVALS_URL
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-arrivals-url }
            - name: LDBWS_ARRIVALS_KEY
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-arrivals-key }
            - name: LDBWS_SERVICE_URL
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-service-url }
            - name: LDBWS_SERVICE_KEY
              valueFrom:
                secretKeyRef: { name: {{ include "distant-signal.railMcpSecretName" . }}, key: ldbws-service-key }
          {{- with .Values.railMcp.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.railMcp.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.railMcp.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.railMcp.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

Note: `railmcp-secret.yaml` (a chart-rendered `Secret` holding `discord-client-id`/`discord-allowed-user-ids`/the six LDBWS values, mirroring `secret.yaml`'s own pattern for genuinely-external credentials — these are all externally-assigned values this chart cannot generate, unlike `scheduleFeed.sftp.password`) is deliberately **not** designed in full here — sketch its shape as `secret.yaml`'s own existing external-credential half already does (an `existingSecret` escape hatch, keys passed through from `.Values.railMcp.*` when unset), matching this plan's stated deployment depth (Decision 6 itself: "not designed further here" for anything beyond the Deployment/Service/values shape). Referencing `include "distant-signal.railMcpSecretName" .`/`railMcpFullname` above assumes `_helpers.tpl` gains those two `define` blocks, following the exact naming pattern `devAuthentikSecretName`/`devAuthentikFullname` already use (`_helpers.tpl:284-307`) — add them as part of this step.

- [ ] **Step 3: `charts/distant-signal/templates/railmcp-service.yaml`**

```yaml
{{- if .Values.railMcp.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "distant-signal.railMcpFullname" . }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "railmcp") | nindent 4 }}
spec:
  type: ClusterIP
  selector:
    {{- include "distant-signal.selectorLabels" (dict "root" . "component" "railmcp") | nindent 4 }}
  ports:
    - name: http
      port: {{ .Values.railMcp.service.port }}
      targetPort: http
{{- end }}
```

No Ingress/TLS is added here — Decision 6 and this plan's Explicitly out of scope section both carry that forward as a deliberate non-goal for this pass, matching this chart's own existing shallow depth for every other integration spec's deployment section.

- [ ] **Step 4: Add `distant-signal.railMcpFullname`/`distant-signal.railMcpSecretName` to `_helpers.tpl`**

Following the exact pattern `distant-signal.devAuthentikFullname`/`distant-signal.devAuthentikSecretName` already use (`_helpers.tpl:284,303`):

```
{{- define "distant-signal.railMcpFullname" -}}
{{- printf "%s-railmcp" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end }}

{{- define "distant-signal.railMcpSecretName" -}}
{{- printf "%s-railmcp" (include "distant-signal.fullname" .) -}}
{{- end }}
```

- [ ] **Step 5: Lint the chart**

```bash
helm lint charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://railmcp.example.com --set railMcp.discord.clientId=x --set railMcp.discord.allowedUserIds=1 --set railMcp.ldbws.departuresUrl=x --set railMcp.ldbws.departuresKey=x --set railMcp.ldbws.arrivalsUrl=x --set railMcp.ldbws.arrivalsKey=x --set railMcp.ldbws.serviceUrl=x --set railMcp.ldbws.serviceKey=x
helm template charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://railmcp.example.com --set railMcp.discord.clientId=x --set railMcp.discord.allowedUserIds=1 --set railMcp.ldbws.departuresUrl=x --set railMcp.ldbws.departuresKey=x --set railMcp.ldbws.arrivalsUrl=x --set railMcp.ldbws.arrivalsKey=x --set railMcp.ldbws.serviceUrl=x --set railMcp.ldbws.serviceKey=x > /dev/null
```

Expected: both exit 0. Also confirm `helm lint charts/distant-signal` (with **no** `railMcp.*` overrides, `railMcp.enabled` at its default `false`) still passes — the new templates must not render or fail when the feature is off.

- [ ] **Step 6: Commit**

```bash
git add charts/distant-signal/templates/railmcp-deployment.yaml charts/distant-signal/templates/railmcp-service.yaml charts/distant-signal/templates/_helpers.tpl charts/distant-signal/values.yaml
git commit -m "Add opt-in railMcp Helm subchart for the derived MCP service (Decision 6)"
```

---

### Task 8: `docker-compose.yml` service entry (Decision 6, second half)

**Files:**
- Modify: `/workspaces/github-com-fasterspeeding-network-rail-status/docker-compose.yml`

**Interfaces:**
- Produces: a `rail-mcp` compose service, `depends_on: [api]`, referencing an externally-built image (not a `build:` context into this repo), matching `schedule-sftp`'s own precedent for a third-party/externally-sourced image.
- **Depends on:** Task 2 (env var names).

- [ ] **Step 1: Add the service block**

Insert after the existing `schedule-ingest:` service (following `docker-compose.yml`'s established `depends_on`/`environment` style seen at the `api:`/`schedule-sftp:` entries):

```yaml
  rail-mcp:
    # Externally-built image -- this repo's own CI does not build the
    # derived MCP service's source (it lives in its own repository, per
    # docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md
    # Decision 1) -- same shape as schedule-sftp's drakkan/sftpgo entry
    # above, not a `build:` context into this repo.
    image: ${RAIL_MCP_IMAGE:-ghcr.io/CHANGE-ME/distant-signal-mcp:latest}
    restart: unless-stopped
    depends_on:
      api:
        condition: service_started
    ports:
      - "${RAIL_MCP_PORT:-3100}:3000"
    environment:
      PORT: "3000"
      PUBLIC_URL: ${RAIL_MCP_PUBLIC_URL:-http://localhost:3100}
      # In-network hostname every other compose service already uses to
      # reach `api` -- see the api: service's own BIND_URL above.
      DS_API_BASE_URL: http://api:8080
      DS_LINE_CATALOGUE_TTL_MS: ${DS_LINE_CATALOGUE_TTL_MS:-900000}

      DISCORD_CLIENT_ID: ${RAIL_MCP_DISCORD_CLIENT_ID}
      DISCORD_ALLOWED_USER_IDS: ${RAIL_MCP_DISCORD_ALLOWED_USER_IDS}

      LDBWS_DEPARTURES_URL: ${LDBWS_DEPARTURES_URL}
      LDBWS_DEPARTURES_KEY: ${LDBWS_DEPARTURES_KEY}
      LDBWS_ARRIVALS_URL: ${LDBWS_ARRIVALS_URL}
      LDBWS_ARRIVALS_KEY: ${LDBWS_ARRIVALS_KEY}
      LDBWS_SERVICE_URL: ${LDBWS_SERVICE_URL}
      LDBWS_SERVICE_KEY: ${LDBWS_SERVICE_KEY}
```

`docker-compose.dev.yml`'s exact local-dev story (bind-mount of a sibling checkout of the derived service's own repo, for live-reloading, vs. this pre-built-image entry) is deliberately left to a later implementation pass — Decision 6 sketches this as a real option without designing it further, and this task does not resolve it either.

- [ ] **Step 2: Validate the compose file**

```bash
docker compose config --quiet
```

Expected: exits 0 with no error (confirms YAML validity and variable interpolation, without requiring the referenced image to actually exist or be pulled).

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "Add rail-mcp compose service entry for local/dev use (Decision 6)"
```

---

### Task 9 (flagged follow-on — NOT executed by this plan's own critical path): smallest DS route addition that would let board tools route through DS too

**Why this task exists, and why it is scoped this way:** the new "prefer DS's own API wherever feasible" instruction this plan's Global Constraints state has a real implication for Decision 5 of the design spec, which keeps `get_departures`/`get_arrivals`/`get_service_detail` on direct Darwin/LDBWS access, reasoned on "DS does not today expose a public per-station board route" plus a real RDM-quota/`poller-ldbws`-widening cost. This plan does **not** silently drop that tension. It also does not, on its own authority, overturn Decision 5 — that decision was reached with real, considered alternatives (this plan's own re-reading of `crates/api`'s route surface confirms DS genuinely has no such route today, and confirms *why* one would be structurally limited even if added — see below) inside an approved-shape design this plan is implementing, not re-litigating. This task is therefore written as a concrete, file-level-detailed **proposal**, sketched to the depth a reader could pick it up and implement it, but explicitly **not** part of Tasks 1-8's executed sequence and **not** something this plan's own verification steps (Task 1-8's `npm test`/`helm lint` runs) cover.

**Files (a future, separately-scoped pass would touch):**
- Modify: `crates/api/src/routes/reference.rs` (or a new `crates/api/src/routes/board.rs`, mounted the same way `reference.rs`'s router is — `public_router()`, `crates/api/src/routes/mod.rs:20-51`)
- Modify: `crates/api/src/data/queries.rs` (a thin public wrapper around the already-existing `latest_station_sample(pool, crs)`, `queries.rs:568-583` — confirmed already implemented and already used internally by `crates/api/src/data/eta_blend.rs` for Darwin/TRUST correlation, so the read path this route would need already exists; nothing here requires new schema or a new poller)

**The concrete sketch:**

A new unauthenticated route, `GET /public/stations/{crs}/board`, returning the same `StationSample` shape `latest_station_sample` already produces (`{crs, polledAt, departures: [...]}`), 404 if no row exists for that CRS. This is a small, additive Rust change — a route handler plus a `pub` re-export of a query that already exists internally — not a new subsystem.

**The real limit this sketch does not, and cannot, resolve:** `station_samples` only ever has a row for a station `crates/poller-ldbws` actually polls — a **curated set**, fetched at startup from `config.api_sample_stations_url` (`crates/poller-ldbws/src/main.rs:80,117`), not "every UK station." Adding this route makes DS's *already-polled* stations reachable by CRS instead of only by the aggregator's own internal use — genuinely useful for board tools calling through DS for **that curated subset** — but does **not** achieve "arbitrary station" board coverage on its own. Closing that second gap requires widening `poller-ldbws`'s curated station-set config, which is exactly the RDM-quota/cost Decision 5 flagged and neither this task nor this plan resolves — restated here, not silently dropped, matching Decision 5's own reasoning and the design spec's own Open Question 4 (DS's curated catalogue size/coverage is not measured by any pass so far, this one included).

**If this task is ever picked up as its own plan:** it would need (1) the Rust route + query wrapper sketched above, with real tests following `crates/api/src/routes/line_status.rs`'s own `#[cfg(test)]` convention; (2) a design-level decision on whether `get_departures`/`get_arrivals` in the derived MCP service should try DS first and fall back to direct LDBWS for stations DS doesn't curate (a real, non-trivial fallback-ordering question this sketch does not answer); (3) a real measurement of `poller-ldbws`'s current curated-station-set size and RDM quota headroom before deciding whether to widen it — none of which this plan attempts, consistent with its own scope.

- [ ] **This task has no steps to execute as part of this plan.** Its only action item is: when a future pass takes this up, start from the file-level sketch above rather than from scratch, and re-verify `latest_station_sample`'s exact current signature and `poller-ldbws`'s exact current curated-station-set mechanism directly against source at that time (both may have changed since this plan's own verification pass).

---

### Task 10: NRE-attribution licensing/legal sign-off (non-code gate)

**Files:** none. This is a human decision, not an engineering task.

**Interfaces:** none produced. **Gates:** Task 6's `liveStatus` output going live for real users (development and testing of Task 6 is not blocked on this).

- **Depends on:** nothing code-wise; blocks deployment of Task 6's output, not its development.

The design spec's own Licensing note raises a genuinely unresolved question, restated here rather than silently dropped or resolved by this plan: **does serving DS-derived line-status/incident data (the Knowledgebase-derived `reason`/`disruption.description` text Task 6's `liveStatus` field carries) out through an MCP tool's `structuredContent`/rendered text, consumed inside a third-party AI assistant's chat UI, count as a "presentation" surface under NRE's Terms & Conditions v3.0 attribution clause — the same way DS's own frontend (which already carries a "Powered by National Rail Enquiries" attribution via `frontend/components/OpenDataAttribution.tsx`) does?**

- [ ] **Step 1: Get an explicit human/legal answer to the question above before Task 6's output is deployed to serve real traffic.** Neither "yes, attribution is required" nor "no, it isn't" is assumed by this plan — this step's only job is producing that answer, not guessing at one.
- [ ] **Step 2: If the answer is "yes, attribution is required":** implement it at the derived service's own tool-result rendering layer — a fixed "Powered by National Rail Enquiries" line appended wherever `liveStatus.reason`/DS-sourced disruption text is rendered in `plan_journey`'s text output (`src/tools/plan-journey.ts`'s `renderLeg`/`renderJourneyBody`, Task 6), analogous to `OpenDataAttribution.tsx`'s existing fixed-wording component in this repo's own frontend. This is a small, mechanical follow-up once the legal question is actually answered — not designed further here, since the underlying question is what gates it, not the implementation.
- [ ] **Step 3: If the answer is "no, attribution is not required":** record that decision (and who made it) somewhere durable — a commit message on the derived service's own repository, or this plan's own tracking, whichever this org's convention prefers — so the question does not silently resurface unanswered at a later audit.

---

## Explicitly out of scope

Carried forward from the design spec's own "Explicitly out of scope" section — restated here, not silently dropped, so a reader of this plan alone has the full picture:

- **`find_services`/`plan_journey`'s own CIF-timetable-store/RAPTOR/CSA engine.** Stays entirely inside the forked service, unmodified, per Decision 1/5. No task in this plan touches `src/timetable/*`.
- **Building a queryable DS timetable store from `schedule-ingest`'s CIF data.** Not justified today, per the research doc's own recommendation 5, carried forward unchanged.
- **The currently-down "National rail status" MCP connector.** Confirmed expected to be down (design spec's own Context correction); not investigated by this plan either. This plan's Status note additionally confirms the fork's own `docker-compose.prod.yml`/`Dockerfile` deploy to `rail.skyes.lgbt` via Caddy — plausibly that connector's actual backend — but re-confirming that identification is out of scope here too, exactly as the design spec scoped it.
- **Widening `poller-ldbws` to arbitrary stations, or any new DS route for `get_departures`/`get_arrivals`, as part of this plan's executed tasks.** Decision 5's boundary stays this plan's own scope for Tasks 1-8; Task 9 above is the explicitly-flagged, explicitly-unexecuted exception, not a silent contradiction of this line.
- **A general-purpose "get detail for any service" route on DS**, for `get_service_detail`. Not designed here, matching Decision 5's own reasoning.
- **DS's `Suggestion` type gaining `tiploc`/`matchType` fields.** Considered and rejected in Decision 2; Task 3's shim exists specifically because this was rejected.
- **Reusing DS's OIDC SSO as the derived service's own auth (research doc's Option 2).** Rejected in Decision 4; Task 2 implements Option 1 (anonymous DS calls, unchanged Discord gating on the MCP tools themselves) instead.
- **Ingress/TLS/network-policy/autoscaling for the new `railMcp` chart component.** Sketched at the same shallow depth this chart's other integration components get; not designed further in Task 7.
- **`docker-compose.dev.yml`'s exact local-dev bind-mount story.** Flagged as a real, deferred choice in both the design spec and Task 8 above; not resolved here.
- **The TRUST-corroboration tier (Decision 3b step 6, `trackedTrainState`)** — reachable only in the narrow, opportunistic case of a caller independently holding a DS session and having already tracked the exact same train, which Decision 4's Option 1 (this plan's chosen auth posture) does not make generally reachable. No task in this plan attempts to populate `LiveStatus.trackedTrainState`; the `source: 'ds_tracked_train'` value in Task 6's type exists to name the case, not to be produced by any code this plan writes.

## Self-review notes

- **Spec coverage:** Decision 1 (fork shape) → Task 1. Decision 2 (`resolve_station` shim) → Task 3. Decision 3a-3d (delay annotation, matching, schema, caching) → Tasks 4, 5, 6. Decision 4 (auth) → Task 2. Decision 5 (board tools left alone) → Global Constraints + Task 9's explicit reconciliation. Decision 6 (deployment) → Tasks 7, 8. Testing section → distributed across Tasks 2-6's own test steps (auth smoke test: Task 2 Step 5; leg-matching incl. the named Barking/c2c/Suffragette case: Task 4 Step 2; `resolve_station` shim incl. `matchType` degradation and tiploc-miss: Task 3 Step 5; `truncated` computation: Task 3 Steps 1/5; DS-call-failure fallback for each of the three annotation-loop calls: Task 6 Step 4; scope-precision check: Task 6 Step 4). Licensing note → Task 10.
- **New steer coverage (prefer DS's own API generally):** stated as a Global Constraint, and its one real tension with the merged design spec (Decision 5's board-tools boundary) is reconciled explicitly via Task 9 rather than silently dropped or silently used to unilaterally expand this plan's own scope.
- **Type consistency check:** `DsStationMatch` (Task 3) vs. `matchShape`'s Zod schema (Task 3, same task) — `tiploc?: string` matches `z.string().optional()`; `matchType`'s three-value union matches the Zod enum. `LineMatchResult`/`MatchConfidence` (Task 4) match `liveStatus`'s `matchConfidence` field and Task 6's `LiveStatus` interface exactly — both were written from the same enum definition, not independently re-typed. `AnnotatableLeg` (Task 6) matches the subset of `RenderedTrainLeg` fields Task 6 Step 2 actually threads into it (`operator`, `from.crs`→`fromCrs`, `to.crs`→`toCrs`, `departureAt`).
