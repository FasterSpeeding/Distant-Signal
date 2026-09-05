# MCP Integration Phase 0+1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Every implementation task below applies to the separate `distant-signal-mcp`
> repository (local clone: `/workspaces/distant-signal-mcp`), not to this
> (`distant-signal`) repository.** This repo needs **zero code changes** for this
> plan's scope — every route Phase 0/1 calls already exists in `crates/api` and is
> already public/unauthenticated (Phase 0) or already fully authorized via the
> existing `mcp-users` access group (Phase 1). This plan document lives in this
> repo (`docs/superpowers/plans/`) only because that is where this repo's other
> cross-repo MCP plans already live (e.g.
> `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`), matching that
> precedent exactly. All file paths in the tasks below, unless explicitly marked
> "(this repo, read-only citation)", are relative to `distant-signal-mcp`'s own
> repository root.

**Goal:** implement Phase 0 (four new MCP tools/resources over already-public
`api` routes `distant-signal-mcp` doesn't call yet) and Phase 1 (the missing
fifth `DsApiClient` method — a session-carrying `GET /Train/by-uid/{uid}/{date}`
call — wired into `annotateLeg.ts`'s existing-but-dormant TRUST-corroboration
tier, plus a standalone tool built on the same method), per the design spec's
own Decisions 1 and 2 and its explicit recommendation to build these two phases
"first, as one batch... the cheap, high-value slice."

**Architecture:** Phase 0 (Tasks 1–6) extends `DsApiClient`
(`src/ds/client.ts`) with six new methods over six already-public `crates/api`
routes, wraps four of them in three new MCP tools and one new MCP resource
(the first resource this codebase has ever registered — `registerResource` is
a real, already-available SDK method, `McpServer.registerResource`, just never
called anywhere in this codebase today; no new scaffolding beyond one new
`src/resources/` directory is needed), and wires all four into
`buildServer`. Phase 1 (Tasks 7–9) adds a seventh `DsApiClient` method that —
unlike every other method on that client — sends the caller's own DS session
cookie (Decision 4's "deliberately anonymous" posture is a default, not an
absolute; this one call is the sole exception, exactly as the integration
doc's revised Decision 3b.6 originally specified), wires it into
`annotateLeg.ts`'s TRUST-corroboration tier (dormant since `src/app.ts` first
started extracting `dsSessionCookieValue`), and exposes the same client method
as a standalone tool, `get_tracked_train_status`, gated by nothing beyond the
whole-server `mcp-users` group every tool already sits behind. Task 10 is a
single full-suite verification pass across both phases.

**Tech Stack:** TypeScript (Node ≥24, `@modelcontextprotocol/sdk` 1.29.0,
`zod`), `vitest run` for tests (`package.json:14`). No new npm dependency
anywhere in this plan — every new file uses only `zod`, the platform `fetch`,
and this codebase's own existing modules.

**Spec:**
`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md` (in
the `distant-signal` repo) — read in full before starting; this plan carries
its Decisions 1, 2, and 7 into concrete tasks. It in turn corrects one
conclusion of `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
revised Decision 3b.6/Decision 4 (also in the `distant-signal` repo): that
document said the TRUST-corroboration tier would become "a general per-request
mechanism," but the mechanism itself was never built — Tasks 7–8 below are
that still-outstanding work, not a re-verification of something already done.

## Global Constraints

- **No `crates/api` (or any other file in the `distant-signal` repo) changes
  anywhere in this plan.** Every route this plan's tasks call already exists,
  already returns the JSON shape cited in each task, and needs no new route,
  no new migration, no new access group. If a task's step appears to require
  a `crates/api` change, that is a signal to stop and re-read the design
  spec's Decision 1/2/7 rather than add one.
- **Every DS call this plan adds is anonymous by default, matching Decision 4
  of the prior integration design — except exactly one.** `DsApiClient.getTrackedTrainStatus`
  (Task 7) is the sole method on this client that ever sends a `Cookie`
  header. Every other new method (Task 1) follows the existing `getJson`
  helper's unmodified "no Authorization, no Cookie" path. `test/ds-client.test.ts`'s
  existing auth smoke test (`'never sends an Authorization header or a
  session cookie on any call'`) must keep passing unmodified for every method
  it already covers, and Task 7 adds a **separate** test asserting the
  Cookie header is sent **only** by `getTrackedTrainStatus`, never leaking
  into a sibling call made from the same `DsApiClient` instance.
- **Module resolution is ESM with explicit `.js` extensions on relative
  imports**, matching every existing file (e.g. `src/tools/resolve-station.ts:3`:
  `from '../ds/client.js'`). Every new `import`/`export` this plan adds
  follows the same `.js`-suffixed-relative-import convention even though the
  source file is `.ts`.
- **Testing convention, unchanged from this codebase's own:** flat
  `test/*.test.ts` at the repo root (not colocated with `src/`), run via
  `vitest run` (`package.json:14`, `"test": "vitest run"`). Every new test
  file this plan adds goes in `test/`, matching the naming convention already
  in use (`test/tools-resolve-station.test.ts` for `src/tools/resolve-station.ts`,
  `test/ds-client.test.ts` for `src/ds/client.ts`).
- **No new npm dependency.** Every new file uses only `zod`, the platform
  `fetch`, and existing modules in this codebase.
- **A DS failure for any of this plan's new tools propagates as an
  uncaught rejection, matching `resolve_station`'s own convention**
  (`src/tools/resolve-station.ts`'s handler has no try/catch around
  `resolveStationViaDs` — an uncaught `DsUnavailableError` becomes the MCP
  SDK's own `isError: true` tool result). None of Phase 0/1's new tools has
  a fallback data source once DS is unreachable, the same posture
  `resolve_station` already committed to. The one exception is a DS **404**
  specifically representing a legitimate "no data for this input" outcome
  (a station absent from CIF-SCHEDULE reference data, a station with zero
  sample data, an untracked `(uid, date)` pair) — those are never errors,
  never thrown, and are represented as a normal, non-error tool result (see
  each task's own handling).
- **Every new tool/resource this plan adds sits under the whole-server
  `mcp-users` gate only, never `mcp-live-boards`.** Per the design spec's
  Decision 7: none of this plan's new routes hit a metered external
  resource (Darwin/LDBWS) the way the four `LIVE_BOARD_TOOLS`
  (`src/oauth/accessGroups.ts:37`) do — every one of them is DS-hosted.
  Concretely: none of Tasks 2–9 ever add an entry to `LIVE_BOARD_TOOLS`, and
  every new tool is registered in `buildServer` **unconditionally** (outside
  the `if (deps.liveBoardsEntitled ?? true)` block that gates
  `registerBoardTools`/`registerServiceDetail`/`registerPlanJourney`).
- **`test/oauth-access-groups.test.ts`'s `ALL_TOOLS` fixture (line 10) and
  `test/app.test.ts`'s two tool-count assertions (`'mcp-users-only caller...
  tools/list omits the four metered board tools'` and `'fully entitled
  caller... tools/list includes all six tools'`, currently asserting a total
  of six tools) must be updated as each new tool is registered — not left
  stale.** These are real, existing, passing assertions this plan's new
  tools change the correct answer to; leaving them unupdated would make them
  silently wrong, not just incomplete.
- **File scope (`distant-signal-mcp` repo).** Modified:
  `src/ds/client.ts`, `src/ds/annotateLeg.ts`, `src/server.ts`,
  `src/tools/plan-journey.ts`, `test/ds-client.test.ts`,
  `test/ds-annotate-leg.test.ts`, `test/tools-plan-journey.test.ts`,
  `test/oauth-access-groups.test.ts`, `test/app.test.ts`. Created:
  `src/tools/line-delay-trend.ts`, `src/tools/national-schedule-departures.ts`,
  `src/tools/station-operator-stats.ts`, `src/tools/tracked-train-status.ts`,
  `src/resources/dataFreshness.ts`, and one test file per new tool/resource
  (`test/tools-line-delay-trend.test.ts`,
  `test/tools-national-schedule-departures.test.ts`,
  `test/tools-station-operator-stats.test.ts`,
  `test/resources-data-freshness.test.ts`,
  `test/tools-tracked-train-status.test.ts`). No file in the `distant-signal`
  repo changes.

## Non-goals (explicitly out of scope)

- **Any Phase 2 or Phase 3 work** (a new `GET /public/lines/{id}/schedule`
  route, a public `stanox_crs` mirror, `train_movement_events.raw_body`,
  `full_coverage_train_state`, or any other new `crates/api` route,
  migration, or microservice change). All of that is separate, later,
  design-and-plan work per the design spec's own phased recommendation.
- **Re-pointing `get_departures`/`get_arrivals`/`get_service_detail` at
  `GET /public/stations/{crs}/departures`.** Named and declined in the
  design spec's Decision 1 and Open questions/risks #1 — not resolved here.
- **Any change to the adapter's OAuth 2.1 authorization-server layer, the
  `mcp-users`/`mcp-live-boards` access-group *mechanism*, or
  `src/oauth/accessGroups.ts`'s `isEntitledToServer`/`requiresLiveBoardsGroup`
  gating logic itself.** This plan only registers new tools under the
  existing `mcp-users` gate (already enforced in `src/app.ts`'s `/mcp`
  handler before `buildServer` ever runs) — it adds no new gate and modifies
  no existing one.
- **A `full_coverage_train_state`-backed `get_train_status(lineId, uid,
  date?)` network-wide tool.** That is Phase 3b, explicitly deferred to its
  own follow-up design per the design spec's own recommendation.
- **Caching for any of Phase 0's four new tools/resource.** Unlike
  `src/ds/lineCatalogue.ts`'s `DsLineCatalogue` (which caches because
  `plan_journey` may fetch the same line's catalogue data many times per
  request across many legs), each of `get_line_delay_trend`,
  `get_national_schedule_departures`, `get_station_operator_stats`, and the
  `data-freshness` resource is called at most once per MCP request with
  caller-supplied parameters that vary per call — there is no repeated
  same-input fetch within one request to amortize, and the freshness
  resource specifically exists to report *how stale* DS's own data is,
  so caching it would risk reporting stale staleness data. No `TtlCache`
  is introduced by this plan.
- **Any change to `src/ldbws/*`, `src/timetable/*`, `src/tools/boards.ts`,
  `src/tools/service-detail.ts`, or `src/tools/find-services.ts`.**
  Untouched, matching every prior MCP plan's own boundary.

---

# Phase 0 — four new tools/resource over already-public routes

## Task 1: `DsApiClient` — six new Phase 0 methods, `getJson` extended for optional headers and a 404-as-null mode

**Files:**
- Modify: `src/ds/client.ts`
- Test: `test/ds-client.test.ts` (extend)

**Interfaces:**
- Produces: `DsApiClient.getLineDailyStats(lineId, from, to)`,
  `.getLineHalfHourlyStats(lineId, from, to)`, `.getScheduleDepartures(crs)`,
  `.getStationOperatorStats(crs)`, `.getFreshness()`, `.getHistoryRetention()`,
  plus the new types `DsDailyStats`, `DsHalfHourlyStats`, `DsScheduleDeparture`,
  `DsSampleStats` (already exists, reused), `DsSampleAvailability`,
  `DsFullCoverageAvailability`, `DsStationOperatorStats`, `DsFreshness`,
  `DsHistoryRetention`.
- Consumed by: Tasks 2–5 (one method each, except Task 5 which uses both
  `getFreshness`/`getHistoryRetention`).
- **Depends on:** nothing (Task 1 is foundational for Phase 0, same role
  `src/ds/client.ts`'s original four methods played for the prior plan).

Every method below is modeled directly on the existing four methods'
established shape (`src/ds/client.ts:88-121`): a `baseUrl`-relative URL, a
call through the shared `getJson` private helper, no `Authorization`/`Cookie`
header. Response shapes below are taken directly from the real `crates/api`
route handlers and their JSON-rendering helpers (this repo's own citations,
read this session), not from the design spec's summaries.

- [ ] **Step 1: Extend `getJson`'s signature** (currently
  `src/ds/client.ts:123-139`) to accept optional extra request headers and an
  optional "treat 404 as `null`, not an error" mode — needed by
  `getScheduleDepartures`/`getStationOperatorStats` below (both 404 for a
  legitimate "no data for this input" case per their own `crates/api` route
  doc comments) and by Task 7's session-carrying call:

```ts
    private async getJson(
        url: string,
        extraHeaders?: Record<string, string>,
        options?: { notFoundIsNull?: boolean }
    ): Promise<unknown> {
        let response: Response;
        try {
            // Deliberately no Authorization header on any call, and no
            // Cookie header unless extraHeaders explicitly supplies one --
            // only getTrackedTrainStatus (Task 7) ever does. Decision 4.
            response = await this.fetchImpl(url, { headers: { Accept: 'application/json', ...extraHeaders } });
        } catch (cause) {
            throw new DsUnavailableError(`Could not reach Distant Signal at ${url}`, { cause });
        }
        if (response.status === 404 && options?.notFoundIsNull) {
            return null;
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
```

  Every existing call site (`searchStations`, `getLines`, `getLineDefinition`,
  `getLineStatus`'s inner call) passes neither new parameter — both are
  optional, so `{ Accept: 'application/json', ...undefined }` still evaluates
  to `{ Accept: 'application/json' }` and every existing behavior is
  unchanged. Confirm this by re-running the existing `ds-client.test.ts`
  suite unmodified after this step (Step 8 below).

- [ ] **Step 2: New interfaces**, added after the existing `DsLineStatusReport`
  interface (`client.ts:58-65`):

```ts
/** `GET /Line/{id}/Stats/{from}/to/{to}` -- daily rollup. Shape from `crates/api/src/routes/line_status.rs::daily_stats_to_json` (`line_status.rs:386-412`). `day` is `YYYY-MM-DD`. */
export interface DsDailyStats {
    day: string;
    sampleCycles: number;
    total: number;
    delayed: number;
    cancelled: number;
    skipped: number;
    avgDelayMinutes: number;
    delayRate: number;
    cancellationRate: number;
    skipRate: number;
}

/** `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` -- half-hourly sibling. `half_hourly_stats_to_json` (`line_status.rs:430-456`). `halfHourStart` is an ISO instant. */
export interface DsHalfHourlyStats {
    halfHourStart: string;
    sampleCycles: number;
    total: number;
    delayed: number;
    cancelled: number;
    skipped: number;
    avgDelayMinutes: number;
    delayRate: number;
    cancellationRate: number;
    skipRate: number;
}

/** `GET /public/stations/{crs}/schedule-departures`. Shape from `crates/api/src/render.rs::schedule_departure_json` (`render.rs:171-181`) -- every field nullable, defensively, since that function itself falls back to `Value::Null` for a missing/mistyped source field rather than panicking. `scheduled` is trimmed to `HH:MM`. */
export interface DsScheduleDeparture {
    uid: string | null;
    scheduled: string | null;
    destinationCrs: string | null;
}

/** `GET /public/stations/{crs}/sample-stats`'s `sampleAvailability`/`fullCoverageAvailability` shape. `render.rs::sample_availability_json`/`full_coverage_availability_json` (`render.rs:107-128`). `observed`/`required` are set only for `state: 'below-threshold'`. */
export interface DsSampleAvailability {
    state: 'no-coverage' | 'below-threshold' | 'available';
    observed?: number;
    required?: number;
}

/** Same route's `fullCoverageAvailability` -- a separate, smaller state set (no `below-threshold`, since full-coverage has no sample-count threshold concept). */
export interface DsFullCoverageAvailability {
    state: 'not-enabled' | 'pending' | 'available';
}

/** One entry of `GET /public/stations/{crs}/sample-stats`'s array response. `crates/api/src/routes/station_stats.rs::get_station_sample_stats` (`station_stats.rs:72-91`). `sampleStats`/`fullCoverageStats` reuse the existing `DsSampleStats` shape (`client.ts`, unchanged). */
export interface DsStationOperatorStats {
    operator: string;
    sampleAvailability: DsSampleAvailability;
    sampleStats?: DsSampleStats;
    fullCoverageStats?: DsSampleStats;
    fullCoverageAvailability: DsFullCoverageAvailability;
}

/**
 * `GET /public/freshness`. Shape from `crates/api/src/routes/freshness.rs::DataFreshness`
 * (`freshness.rs:25-39`) -- every field an ISO instant or `null`.
 * **`schedule_feed` is deliberately snake_case here, not a typo**: that
 * struct has no `#[serde(rename_all = "camelCase")]` attribute (unlike
 * every other hand-built response this client models, and unlike this
 * same route's own sibling `history-retention`, which does have the
 * attribute) -- confirmed directly against `freshness.rs`'s own test,
 * `round_trips_a_present_timestamp`, which asserts `json["schedule_feed"]`
 * (not `json["scheduleFeed"]`). Task 5's resource read renames this to
 * `scheduleFeed` at the MCP boundary, the same "translate DS's wire shape
 * into this adapter's own consistent camelCase convention" precedent
 * `src/stations/resolveViaDs.ts` already established for `resolve_station`.
 */
export interface DsFreshness {
    stations: string | null;
    tocs: string | null;
    incidents: string | null;
    tfl: string | null;
    schedule_feed: string | null;
}

/** `GET /public/history-retention`. `crates/api/src/routes/history_retention.rs::HistoryRetention` -- this one DOES carry `#[serde(rename_all = "camelCase")]` (`history_retention.rs:30-34`), so `historyRetentionDays` is already camelCase on the wire. */
export interface DsHistoryRetention {
    historyRetentionDays: number;
}
```

- [ ] **Step 3: Six new methods**, added after the existing `getLineStatus`
  method (`client.ts:112-121`), before the private `getJson` helper:

```ts
    /** `GET /Line/{id}/Stats/{from}/to/{to}` -- `from`/`to` are `YYYY-MM-DD` calendar dates (a `chrono::NaiveDate` path segment server-side, `line_status.rs:414-423`). */
    async getLineDailyStats(lineId: string, from: string, to: string): Promise<DsDailyStats[]> {
        const url = `${this.baseUrl}/Line/${encodeURIComponent(lineId)}/Stats/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`;
        return this.getJson(url) as Promise<DsDailyStats[]>;
    }

    /** `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` -- `from`/`to` are full RFC3339 instants (a `chrono::DateTime<Utc>` path segment server-side, `line_status.rs:458-469`), unlike the daily sibling above. */
    async getLineHalfHourlyStats(lineId: string, from: string, to: string): Promise<DsHalfHourlyStats[]> {
        const url = `${this.baseUrl}/Line/${encodeURIComponent(lineId)}/Stats/HalfHourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`;
        return this.getJson(url) as Promise<DsHalfHourlyStats[]>;
    }

    /** `GET /public/stations/{crs}/schedule-departures`. Returns `null` (never throws) on a 404 -- `departures.rs::get_station_schedule_departures`'s own doc comment: 404 means "this station isn't in stanox_crs, or today's cycle simply hasn't published yet," a legitimate answer, not a DS outage. */
    async getScheduleDepartures(crs: string): Promise<DsScheduleDeparture[] | null> {
        const url = `${this.baseUrl}/public/stations/${encodeURIComponent(crs)}/schedule-departures`;
        return this.getJson(url, undefined, { notFoundIsNull: true }) as Promise<DsScheduleDeparture[] | null>;
    }

    /** `GET /public/stations/{crs}/sample-stats`. Returns `null` (never throws) on a 404 -- `station_stats.rs::get_station_sample_stats`'s own doc comment: 404 means no `station_samples` AND no `station_full_coverage_samples` row exists for this CRS at all. */
    async getStationOperatorStats(crs: string): Promise<DsStationOperatorStats[] | null> {
        const url = `${this.baseUrl}/public/stations/${encodeURIComponent(crs)}/sample-stats`;
        return this.getJson(url, undefined, { notFoundIsNull: true }) as Promise<DsStationOperatorStats[] | null>;
    }

    /** `GET /public/freshness`. Never 404s (always returns a full object, with `null` fields for anything never fetched -- `freshness.rs:41-59`). */
    async getFreshness(): Promise<DsFreshness> {
        return this.getJson(`${this.baseUrl}/public/freshness`) as Promise<DsFreshness>;
    }

    /** `GET /public/history-retention`. A static config echo, never 404s (`history_retention.rs:36-39`). */
    async getHistoryRetention(): Promise<DsHistoryRetention> {
        return this.getJson(`${this.baseUrl}/public/history-retention`) as Promise<DsHistoryRetention>;
    }
```

- [ ] **Step 4: Extend `test/ds-client.test.ts`** with one `describe` block
  per new method:

```ts
describe('getLineDailyStats', () => {
    it('fetches the daily stats range and returns it unchanged', async () => {
        const row = { day: '2026-08-01', sampleCycles: 48, total: 100, delayed: 12, cancelled: 1, skipped: 0, avgDelayMinutes: 3.2, delayRate: 0.12, cancellationRate: 0.01, skipRate: 0 };
        let requestedUrl = '';
        const fetchImpl = (async (url: string) => {
            requestedUrl = url;
            return jsonResponse([row]);
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const result = await client.getLineDailyStats('c2c', '2026-08-01', '2026-08-07');
        expect(result).toEqual([row]);
        expect(requestedUrl).toBe('https://ds.example.com/Line/c2c/Stats/2026-08-01/to/2026-08-07');
    });
});

describe('getLineHalfHourlyStats', () => {
    it('fetches the half-hourly range with RFC3339 from/to', async () => {
        let requestedUrl = '';
        const fetchImpl = (async (url: string) => {
            requestedUrl = url;
            return jsonResponse([]);
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await client.getLineHalfHourlyStats('c2c', '2026-08-01T00:00:00Z', '2026-08-01T12:00:00Z');
        expect(requestedUrl).toBe('https://ds.example.com/Line/c2c/Stats/HalfHourly/2026-08-01T00%3A00%3A00Z/to/2026-08-01T12%3A00%3A00Z');
    });
});

describe('getScheduleDepartures', () => {
    it('returns the array unchanged on success', async () => {
        const fetchImpl = (async () => jsonResponse([{ uid: 'C11052', scheduled: '08:22', destinationCrs: 'CRE' }])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getScheduleDepartures('PAD')).toEqual([{ uid: 'C11052', scheduled: '08:22', destinationCrs: 'CRE' }]);
    });

    it('returns null, not a thrown error, on a 404', async () => {
        const fetchImpl = (async () => new Response('not found', { status: 404 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getScheduleDepartures('ZZZ')).toBeNull();
    });

    it('still throws DsUnavailableError on a 500', async () => {
        const fetchImpl = (async () => new Response('boom', { status: 500 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getScheduleDepartures('PAD')).rejects.toBeInstanceOf(DsUnavailableError);
    });
});

describe('getStationOperatorStats', () => {
    it('returns the array unchanged on success', async () => {
        const row = { operator: 'GW', sampleAvailability: { state: 'available' }, sampleStats: { total: 10, delayed: 2, cancelled: 0, skipped: 0, avgDelayMinutes: 4.1 }, fullCoverageAvailability: { state: 'not-enabled' } };
        const fetchImpl = (async () => jsonResponse([row])) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getStationOperatorStats('PAD')).toEqual([row]);
    });

    it('returns null, not a thrown error, on a 404', async () => {
        const fetchImpl = (async () => new Response('not found', { status: 404 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getStationOperatorStats('ZZZ')).toBeNull();
    });
});

describe('getFreshness', () => {
    it('returns the freshness object unchanged, including the snake_case schedule_feed field', async () => {
        const body = { stations: '2026-09-01T00:00:00Z', tocs: null, incidents: null, tfl: null, schedule_feed: '2026-09-04T06:00:00Z' };
        const fetchImpl = (async () => jsonResponse(body)) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getFreshness()).toEqual(body);
    });
});

describe('getHistoryRetention', () => {
    it('returns the retention-days object unchanged', async () => {
        const fetchImpl = (async () => jsonResponse({ historyRetentionDays: 7 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getHistoryRetention()).toEqual({ historyRetentionDays: 7 });
    });
});
```

- [ ] **Step 5: Run the tests**

Run: `npm test -- ds-client`
Expected: PASS, including every pre-existing test in this file unmodified (the
`getJson` signature change is additive-only — Step 1's note on why).

- [ ] **Step 6: Typecheck**

Run: `npm run typecheck`
Expected: exits 0.

- [ ] **Step 7: Commit**

```bash
git add src/ds/client.ts test/ds-client.test.ts
git commit -m "DsApiClient: add six Phase 0 methods (line stats, national schedule departures, station operator stats, freshness, history retention)"
```

---

## Task 2: `get_line_delay_trend` tool

**Files:**
- Create: `src/tools/line-delay-trend.ts`
- Test: `test/tools-line-delay-trend.test.ts`

**Interfaces:**
- Produces: `registerLineDelayTrend(server: McpServer, ds: DsApiClient): void`.
- Consumed by: Task 6 (`server.ts`'s `buildServer`).
- **Depends on:** Task 1 (`getLineDailyStats`/`getLineHalfHourlyStats`).

Answers "historical delay trend for line Y" (this task's own brief) by
wrapping `GET /Line/{id}/Stats(/HalfHourly)/{from}/to/{to}` — Decision 1's
first bullet. `granularity` selects which of the two DS routes is called and
therefore which `from`/`to` format is required; validated client-side before
any DS call, since DS's own error for a malformed date/timestamp path segment
is a bare axum path-rejection 400 with no field-specific message.

- [ ] **Step 1: `src/tools/line-delay-trend.ts`**

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { DsApiClient, DsDailyStats, DsHalfHourlyStats } from '../ds/client.js';
import { errorResult } from './rendering.js';

const DAILY_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
const RFC3339_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/;

const trendPointShape = {
    periodStart: z.string().describe('Day (YYYY-MM-DD) for daily granularity, or an ISO instant for half-hourly.'),
    sampleCycles: z.number(),
    total: z.number(),
    delayed: z.number(),
    cancelled: z.number(),
    skipped: z.number(),
    avgDelayMinutes: z.number(),
    delayRate: z.number(),
    cancellationRate: z.number(),
    skipRate: z.number()
};

interface StatsRow {
    sampleCycles: number;
    total: number;
    delayed: number;
    cancelled: number;
    skipped: number;
    avgDelayMinutes: number;
    delayRate: number;
    cancellationRate: number;
    skipRate: number;
}

function toTrendPoint(row: StatsRow, periodStart: string) {
    return {
        periodStart,
        sampleCycles: row.sampleCycles,
        total: row.total,
        delayed: row.delayed,
        cancelled: row.cancelled,
        skipped: row.skipped,
        avgDelayMinutes: row.avgDelayMinutes,
        delayRate: row.delayRate,
        cancellationRate: row.cancellationRate,
        skipRate: row.skipRate
    };
}

function summarize(lineId: string, granularity: 'daily' | 'half-hourly', points: ReturnType<typeof toTrendPoint>[]): string {
    if (points.length === 0) {
        return `No ${granularity} stats found for line "${lineId}" in that range.`;
    }
    const totalRuns = points.reduce((sum, p) => sum + p.total, 0);
    const totalDelayed = points.reduce((sum, p) => sum + p.delayed, 0);
    const totalCancelled = points.reduce((sum, p) => sum + p.cancelled, 0);
    const weightedAvgDelay = totalRuns > 0 ? points.reduce((sum, p) => sum + p.avgDelayMinutes * p.total, 0) / totalRuns : 0;
    return (
        `${lineId} (${granularity}), ${points.length} period${points.length === 1 ? '' : 's'}: ` +
        `${totalRuns} services observed, ${totalDelayed} delayed, ${totalCancelled} cancelled, ` +
        `avg delay ${weightedAvgDelay.toFixed(1)} min.`
    );
}

export function registerLineDelayTrend(server: McpServer, ds: DsApiClient): void {
    server.registerTool(
        'get_line_delay_trend',
        {
            title: 'Historical delay/cancellation trend for a line',
            description:
                'Get a line\'s historical delay, cancellation and skip-rate trend over a date range, sourced ' +
                'from Distant Signal\'s own sample-derived stats. granularity "daily" takes from/to as calendar ' +
                'dates (YYYY-MM-DD) and returns one row per day; "half-hourly" takes from/to as full ISO ' +
                'instants (e.g. "2026-08-01T00:00:00Z") and returns finer-grained buckets. lineId is a Distant ' +
                'Signal catalogue line id (e.g. "c2c", "thameslink-core"), not a TOC code or free-text line name.',
            inputSchema: {
                lineId: z.string().min(1).describe('DS catalogue line id, e.g. "c2c" or "thameslink-core".'),
                from: z.string().describe('Range start -- YYYY-MM-DD for daily, or an ISO instant for half-hourly.'),
                to: z.string().describe('Range end -- YYYY-MM-DD for daily, or an ISO instant for half-hourly.'),
                granularity: z.enum(['daily', 'half-hourly']).describe('"daily" (one row per day) or "half-hourly" (finer-grained buckets).')
            },
            outputSchema: {
                lineId: z.string(),
                granularity: z.enum(['daily', 'half-hourly']),
                points: z.array(z.object(trendPointShape))
            }
        },
        async ({ lineId, from, to, granularity }) => {
            if (granularity === 'daily' && (!DAILY_DATE_PATTERN.test(from) || !DAILY_DATE_PATTERN.test(to))) {
                return errorResult(`"daily" granularity needs from/to as YYYY-MM-DD -- got from="${from}", to="${to}".`);
            }
            if (granularity === 'half-hourly' && (!RFC3339_PATTERN.test(from) || !RFC3339_PATTERN.test(to))) {
                return errorResult(
                    `"half-hourly" granularity needs from/to as full ISO instants (e.g. "2026-08-01T00:00:00Z") -- got from="${from}", to="${to}".`
                );
            }

            const points: ReturnType<typeof toTrendPoint>[] =
                granularity === 'daily'
                    ? (await ds.getLineDailyStats(lineId, from, to)).map((row: DsDailyStats) => toTrendPoint(row, row.day))
                    : (await ds.getLineHalfHourlyStats(lineId, from, to)).map((row: DsHalfHourlyStats) => toTrendPoint(row, row.halfHourStart));

            return {
                content: [{ type: 'text' as const, text: summarize(lineId, granularity, points) }],
                structuredContent: { lineId, granularity, points }
            };
        }
    );
}
```

- [ ] **Step 2: `test/tools-line-delay-trend.test.ts`**

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { registerLineDelayTrend } from '../src/tools/line-delay-trend.js';

async function connect(fetchImpl: typeof fetch) {
    const server = new McpServer({ name: 'test', version: '1.0.0' });
    registerLineDelayTrend(server, new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }));
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

describe('get_line_delay_trend tool', () => {
    it('is advertised by the server', async () => {
        const client = await connect((async () => new Response('[]', { status: 200 })) as unknown as typeof fetch);
        const { tools } = await client.listTools();
        expect(tools.map(t => t.name)).toContain('get_line_delay_trend');
    });

    it('fetches daily stats and summarizes them', async () => {
        const row = { day: '2026-08-01', sampleCycles: 48, total: 100, delayed: 12, cancelled: 1, skipped: 0, avgDelayMinutes: 3.2, delayRate: 0.12, cancellationRate: 0.01, skipRate: 0 };
        const client = await connect((async () => new Response(JSON.stringify([row]), { status: 200 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_line_delay_trend', arguments: { lineId: 'c2c', from: '2026-08-01', to: '2026-08-01', granularity: 'daily' } });
        const structured = result.structuredContent as { points: Array<{ periodStart: string; total: number }> };
        expect(structured.points[0]?.periodStart).toBe('2026-08-01');
        expect(structured.points[0]?.total).toBe(100);
        expect(JSON.stringify(result.content)).toMatch(/100 services observed/);
    });

    it('fetches half-hourly stats via the HalfHourly route when asked', async () => {
        let requestedUrl = '';
        const fetchImpl = (async (url: string) => {
            requestedUrl = url;
            return new Response('[]', { status: 200 });
        }) as unknown as typeof fetch;
        const client = await connect(fetchImpl);
        await client.callTool({ name: 'get_line_delay_trend', arguments: { lineId: 'c2c', from: '2026-08-01T00:00:00Z', to: '2026-08-01T12:00:00Z', granularity: 'half-hourly' } });
        expect(requestedUrl).toContain('/Stats/HalfHourly/');
    });

    it('rejects a daily-shaped date under half-hourly granularity without calling DS', async () => {
        let called = false;
        const fetchImpl = (async () => {
            called = true;
            return new Response('[]', { status: 200 });
        }) as unknown as typeof fetch;
        const client = await connect(fetchImpl);
        const result = await client.callTool({ name: 'get_line_delay_trend', arguments: { lineId: 'c2c', from: '2026-08-01', to: '2026-08-07', granularity: 'half-hourly' } });
        expect(result.isError).toBeTruthy();
        expect(called).toBe(false);
    });

    it('surfaces a DS failure as a tool-level error, with no fallback', async () => {
        const client = await connect((async () => new Response('boom', { status: 500 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_line_delay_trend', arguments: { lineId: 'c2c', from: '2026-08-01', to: '2026-08-07', granularity: 'daily' } });
        expect(result.isError).toBeTruthy();
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- tools-line-delay-trend`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/tools/line-delay-trend.ts test/tools-line-delay-trend.test.ts
git commit -m "Add get_line_delay_trend tool over GET /Line/{id}/Stats(/HalfHourly)"
```

---

## Task 3: `get_national_schedule_departures` tool

**Files:**
- Create: `src/tools/national-schedule-departures.ts`
- Test: `test/tools-national-schedule-departures.test.ts`

**Interfaces:**
- Produces: `registerNationalScheduleDepartures(server: McpServer, ds: DsApiClient): void`.
- Consumed by: Task 6.
- **Depends on:** Task 1 (`getScheduleDepartures`).

Reaches **any** station in `stanox_crs` (the whole GB network), not just DS's
curated `sample_stations` set the existing `get_departures`/`get_arrivals`
tools' underlying Darwin/LDBWS boards are limited to on the operator side —
Decision 1's second bullet. Scheduled times only, no live estimate/delay.

- [ ] **Step 1: `src/tools/national-schedule-departures.ts`**

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { DsApiClient } from '../ds/client.js';

const departureShape = {
    uid: z.string().nullable(),
    scheduled: z.string().nullable().describe('HH:MM local scheduled departure time.'),
    destinationCrs: z.string().nullable()
};

export function registerNationalScheduleDepartures(server: McpServer, ds: DsApiClient): void {
    server.registerTool(
        'get_national_schedule_departures',
        {
            title: "Today's CIF-scheduled departures for any GB station",
            description:
                "Get today's CIF SCHEDULE-derived departures for a station, anywhere on the whole GB rail " +
                "network -- not limited to Distant Signal's curated live-board station set the way " +
                'get_departures/get_arrivals are. Scheduled times only: no live estimate, delay, platform or ' +
                'cancellation data. Use get_departures for a live board at a station DS actively polls; use ' +
                'this tool when you need national coverage without live running data.',
            inputSchema: {
                crs: z.string().length(3).describe('Three-letter CRS station code, e.g. "PAD".')
            },
            outputSchema: {
                crs: z.string(),
                available: z
                    .boolean()
                    .describe(
                        'False when this station has no CIF schedule data at all -- not in the national STANOX/TIPLOC reference, or today\'s feed cycle has not published yet. This is a normal outcome for an unrecognised or very minor CRS, not an error.'
                    ),
                departures: z.array(z.object(departureShape))
            }
        },
        async ({ crs }) => {
            const departures = await ds.getScheduleDepartures(crs);
            if (departures === null) {
                return {
                    content: [
                        {
                            type: 'text' as const,
                            text: `No CIF schedule data for station "${crs}" -- it may not be in the national reference data, or today's feed hasn't published yet.`
                        }
                    ],
                    structuredContent: { crs, available: false, departures: [] }
                };
            }
            const text =
                departures.length === 0
                    ? `No more scheduled departures today for "${crs}".`
                    : departures.map(d => `${d.scheduled ?? '??:??'} -> ${d.destinationCrs ?? '??'} [${d.uid ?? '??'}]`).join('\n');
            return {
                content: [{ type: 'text' as const, text }],
                structuredContent: { crs, available: true, departures }
            };
        }
    );
}
```

- [ ] **Step 2: `test/tools-national-schedule-departures.test.ts`**

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { registerNationalScheduleDepartures } from '../src/tools/national-schedule-departures.js';

async function connect(fetchImpl: typeof fetch) {
    const server = new McpServer({ name: 'test', version: '1.0.0' });
    registerNationalScheduleDepartures(server, new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }));
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

describe('get_national_schedule_departures tool', () => {
    it('is advertised by the server', async () => {
        const client = await connect((async () => new Response('[]', { status: 200 })) as unknown as typeof fetch);
        const { tools } = await client.listTools();
        expect(tools.map(t => t.name)).toContain('get_national_schedule_departures');
    });

    it('returns departures for a covered station', async () => {
        const body = [{ uid: 'C11052', scheduled: '08:22', destinationCrs: 'CRE' }];
        const client = await connect((async () => new Response(JSON.stringify(body), { status: 200 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_national_schedule_departures', arguments: { crs: 'PAD' } });
        const structured = result.structuredContent as { available: boolean; departures: unknown[] };
        expect(structured.available).toBe(true);
        expect(structured.departures).toEqual(body);
    });

    it('reports available: false, not an error, for a 404', async () => {
        const client = await connect((async () => new Response('not found', { status: 404 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_national_schedule_departures', arguments: { crs: 'ZZZ' } });
        expect(result.isError).toBeFalsy();
        expect((result.structuredContent as { available: boolean }).available).toBe(false);
    });

    it('surfaces a genuine DS failure as a tool-level error', async () => {
        const client = await connect((async () => new Response('boom', { status: 500 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_national_schedule_departures', arguments: { crs: 'PAD' } });
        expect(result.isError).toBeTruthy();
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- tools-national-schedule-departures`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/tools/national-schedule-departures.ts test/tools-national-schedule-departures.test.ts
git commit -m "Add get_national_schedule_departures tool over GET /public/stations/{crs}/schedule-departures"
```

---

## Task 4: `get_station_operator_stats` tool

**Files:**
- Create: `src/tools/station-operator-stats.ts`
- Test: `test/tools-station-operator-stats.test.ts`

**Interfaces:**
- Produces: `registerStationOperatorStats(server: McpServer, ds: DsApiClient): void`.
- Consumed by: Task 6.
- **Depends on:** Task 1 (`getStationOperatorStats`).

Per-operator delay/cancellation breakdown DS's own frontend already renders —
Decision 1's third bullet.

- [ ] **Step 1: `src/tools/station-operator-stats.ts`**

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { DsApiClient } from '../ds/client.js';

const sampleStatsShape = z.object({
    total: z.number(),
    delayed: z.number(),
    cancelled: z.number(),
    skipped: z.number(),
    avgDelayMinutes: z.number()
});

const operatorStatsShape = {
    operator: z.string(),
    sampleAvailability: z.object({
        state: z.enum(['no-coverage', 'below-threshold', 'available']),
        observed: z.number().optional(),
        required: z.number().optional()
    }),
    sampleStats: sampleStatsShape.optional(),
    fullCoverageStats: sampleStatsShape.optional(),
    fullCoverageAvailability: z.object({ state: z.enum(['not-enabled', 'pending', 'available']) })
};

export function registerStationOperatorStats(server: McpServer, ds: DsApiClient): void {
    server.registerTool(
        'get_station_operator_stats',
        {
            title: 'Per-operator delay/cancellation stats for a station',
            description:
                'Get per-operator delay, cancellation and sample-availability stats for a station, blending ' +
                "live-board-sample and full-coverage data sources, sourced from Distant Signal's own computed " +
                'stats -- the same breakdown its own frontend renders.',
            inputSchema: { crs: z.string().length(3).describe('Three-letter CRS station code, e.g. "PAD".') },
            outputSchema: {
                crs: z.string(),
                available: z.boolean().describe('False when Distant Signal has no sample data at all for this station.'),
                operators: z.array(z.object(operatorStatsShape))
            }
        },
        async ({ crs }) => {
            const operators = await ds.getStationOperatorStats(crs);
            if (operators === null) {
                return {
                    content: [{ type: 'text' as const, text: `No sample data collected yet for station "${crs}".` }],
                    structuredContent: { crs, available: false, operators: [] }
                };
            }
            const text =
                operators.length === 0
                    ? `No per-operator stats available for "${crs}" right now.`
                    : operators
                          .map(
                              o =>
                                  `${o.operator}: ${o.sampleAvailability.state}` +
                                  (o.sampleStats ? `, avg delay ${o.sampleStats.avgDelayMinutes.toFixed(1)} min, ${o.sampleStats.cancelled} cancelled` : '')
                          )
                          .join('\n');
            return {
                content: [{ type: 'text' as const, text }],
                structuredContent: { crs, available: true, operators }
            };
        }
    );
}
```

- [ ] **Step 2: `test/tools-station-operator-stats.test.ts`**

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { registerStationOperatorStats } from '../src/tools/station-operator-stats.js';

async function connect(fetchImpl: typeof fetch) {
    const server = new McpServer({ name: 'test', version: '1.0.0' });
    registerStationOperatorStats(server, new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }));
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

describe('get_station_operator_stats tool', () => {
    it('is advertised by the server', async () => {
        const client = await connect((async () => new Response('[]', { status: 200 })) as unknown as typeof fetch);
        const { tools } = await client.listTools();
        expect(tools.map(t => t.name)).toContain('get_station_operator_stats');
    });

    it('returns per-operator stats for a covered station', async () => {
        const row = {
            operator: 'GW',
            sampleAvailability: { state: 'available' },
            sampleStats: { total: 10, delayed: 2, cancelled: 1, skipped: 0, avgDelayMinutes: 4.1 },
            fullCoverageAvailability: { state: 'not-enabled' }
        };
        const client = await connect((async () => new Response(JSON.stringify([row]), { status: 200 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_station_operator_stats', arguments: { crs: 'PAD' } });
        const structured = result.structuredContent as { available: boolean; operators: unknown[] };
        expect(structured.available).toBe(true);
        expect(structured.operators).toEqual([row]);
        expect(JSON.stringify(result.content)).toMatch(/GW: available/);
    });

    it('reports available: false, not an error, for a 404', async () => {
        const client = await connect((async () => new Response('not found', { status: 404 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_station_operator_stats', arguments: { crs: 'ZZZ' } });
        expect(result.isError).toBeFalsy();
        expect((result.structuredContent as { available: boolean }).available).toBe(false);
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- tools-station-operator-stats`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/tools/station-operator-stats.ts test/tools-station-operator-stats.test.ts
git commit -m "Add get_station_operator_stats tool over GET /public/stations/{crs}/sample-stats"
```

---

## Task 5: `data-freshness` MCP resource — first resource this codebase registers

**Files:**
- Create: `src/resources/dataFreshness.ts`
- Test: `test/resources-data-freshness.test.ts`

**Interfaces:**
- Produces: `registerDataFreshnessResource(server: McpServer, ds: DsApiClient): void`.
- Consumed by: Task 6.
- **Depends on:** Task 1 (`getFreshness`/`getHistoryRetention`).

**No new MCP resource *mechanism* needs building.** `McpServer.registerResource`
(`node_modules/@modelcontextprotocol/sdk/dist/esm/server/mcp.d.ts:102`,
`registerResource(name, uriOrTemplate: string, config: ResourceMetadata,
readCallback: ReadResourceCallback): RegisteredResource`) already exists in
the SDK version this codebase depends on (`1.29.0`) — it has simply never
been called anywhere in `src/` today (confirmed:
`grep -rn "registerResource" src/ test/` finds nothing). This task is the
first resource, not new scaffolding for a missing capability; `src/resources/`
is a new directory but not a new pattern to invent — one file, following the
exact `registerXxx(server, deps)` shape every tool file already uses. Chosen
as a **resource, not a tool**, per Decision 1's own reasoning: "not something
a user asks for directly," better suited to being read once per session as
ambient context (an LLM client can list and read resources without an
explicit user request the way a tool call needs one).

`ReadResourceCallback`'s return type is `ReadResourceResult = { contents:
Array<{ uri: string; mimeType?: string; text: string } | { uri: string;
mimeType?: string; blob: string }> }` (`types.d.ts`'s
`TextResourceContentsSchema`/`BlobResourceContentsSchema`) — this resource
always returns exactly one `TextResourceContents` entry, JSON-encoded.

- [ ] **Step 1: `src/resources/dataFreshness.ts`**

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { DsApiClient } from '../ds/client.js';

/** Static URI -- this resource takes no parameters, so a plain string (not a ResourceTemplate) is the right registration form, matching `registerResource`'s "For static resources, use a URI string" own doc comment. */
export const DATA_FRESHNESS_URI = 'ds://data-freshness';

export function registerDataFreshnessResource(server: McpServer, ds: DsApiClient): void {
    server.registerResource(
        'data-freshness',
        DATA_FRESHNESS_URI,
        {
            title: 'Distant Signal data freshness',
            description:
                "How current Distant Signal's upstream data sources are right now: last-fetched timestamps " +
                'for station/TOC reference data, incidents, TfL line status, and the CIF SCHEDULE feed, plus ' +
                'how many days of line-status history are retained. Read this before relying heavily on a ' +
                'tool\'s output as "live" -- a source can be stale during an upstream outage without any tool ' +
                'call itself failing or erroring.',
            mimeType: 'application/json'
        },
        async uri => {
            const [freshness, historyRetention] = await Promise.all([ds.getFreshness(), ds.getHistoryRetention()]);
            return {
                contents: [
                    {
                        uri: uri.href,
                        mimeType: 'application/json',
                        text: JSON.stringify({
                            stations: freshness.stations,
                            tocs: freshness.tocs,
                            incidents: freshness.incidents,
                            tfl: freshness.tfl,
                            // Renamed from DS's own inconsistent `schedule_feed`
                            // wire field -- see DsFreshness's own doc comment
                            // in src/ds/client.ts.
                            scheduleFeed: freshness.schedule_feed,
                            historyRetentionDays: historyRetention.historyRetentionDays
                        })
                    }
                ]
            };
        }
    );
}
```

- [ ] **Step 2: `test/resources-data-freshness.test.ts`**

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { DATA_FRESHNESS_URI, registerDataFreshnessResource } from '../src/resources/dataFreshness.js';

function routedFetch(routes: Record<string, unknown>): typeof fetch {
    return (async (url: string) => {
        for (const [fragment, body] of Object.entries(routes)) {
            if (url.includes(fragment)) {
                return new Response(JSON.stringify(body), { status: 200 });
            }
        }
        return new Response('not found', { status: 404 });
    }) as unknown as typeof fetch;
}

async function connect(fetchImpl: typeof fetch) {
    const server = new McpServer({ name: 'test', version: '1.0.0' });
    registerDataFreshnessResource(server, new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }));
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

describe('data-freshness resource', () => {
    it('is advertised by resources/list', async () => {
        const client = await connect(routedFetch({}));
        const { resources } = await client.listResources();
        expect(resources.map(r => r.uri)).toContain(DATA_FRESHNESS_URI);
    });

    it('reads freshness + history-retention and renames schedule_feed to scheduleFeed', async () => {
        const client = await connect(
            routedFetch({
                '/public/freshness': { stations: '2026-09-01T00:00:00Z', tocs: null, incidents: null, tfl: null, schedule_feed: '2026-09-04T06:00:00Z' },
                '/public/history-retention': { historyRetentionDays: 7 }
            })
        );
        const result = await client.readResource({ uri: DATA_FRESHNESS_URI });
        const body = JSON.parse(result.contents[0]!.text as string);
        expect(body.stations).toBe('2026-09-01T00:00:00Z');
        expect(body.scheduleFeed).toBe('2026-09-04T06:00:00Z');
        expect(body.historyRetentionDays).toBe(7);
        expect(body.schedule_feed).toBeUndefined();
    });
});
```

- [ ] **Step 3: Run the tests**

Run: `npm test -- resources-data-freshness`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/resources/dataFreshness.ts test/resources-data-freshness.test.ts
git commit -m "Add data-freshness MCP resource over GET /public/freshness + /public/history-retention"
```

---

## Task 6: Wire Phase 0 into `buildServer`, update tool-count fixtures, full Phase 0 verification

**Files:**
- Modify: `src/server.ts`, `test/oauth-access-groups.test.ts`, `test/app.test.ts`

**Interfaces:**
- Consumes: `registerLineDelayTrend`, `registerNationalScheduleDepartures`,
  `registerStationOperatorStats`, `registerDataFreshnessResource` (Tasks 2–5).
- **Depends on:** Tasks 2–5.

- [ ] **Step 1: Register all four in `buildServer`** (`src/server.ts:76-91`).
  Add the four imports at the top of the file, then register them
  **unconditionally** — before the `if (deps.liveBoardsEntitled ?? true)`
  block, alongside `registerResolveStation`/`registerFindServices` — since
  none require `mcp-live-boards` (this plan's own Global Constraints):

```ts
import { registerLineDelayTrend } from './tools/line-delay-trend.js';
import { registerNationalScheduleDepartures } from './tools/national-schedule-departures.js';
import { registerStationOperatorStats } from './tools/station-operator-stats.js';
import { registerDataFreshnessResource } from './resources/dataFreshness.js';
// ... existing imports unchanged ...

export function buildServer(deps: ServerDeps): McpServer {
    const server = new McpServer({ name: 'distant-signal-mcp', version: '0.1.0' });
    const ds = deps.ds ?? DEFAULT_DS_DEPS;
    const catalogue = new DsLineCatalogue({ client: ds.client, ttlMs: deps.dsCatalogueTtlMs ?? DEFAULT_DS_CATALOGUE_TTL_MS });
    registerResolveStation(server, ds.client);
    registerFindServices(server, deps.timetable ?? DEFAULT_TIMETABLE_DEPS);
    registerLineDelayTrend(server, ds.client);
    registerNationalScheduleDepartures(server, ds.client);
    registerStationOperatorStats(server, ds.client);
    registerDataFreshnessResource(server, ds.client);
    if (deps.liveBoardsEntitled ?? true) {
        registerBoardTools(server, deps.ldbws);
        registerServiceDetail(server, deps.ldbws);
        registerPlanJourney(server, {
            ...(deps.plan ?? DEFAULT_PLAN_DEPS),
            ds: { client: ds.client, catalogue }
        });
    }
    return server;
}
```

- [ ] **Step 2: Update `test/oauth-access-groups.test.ts`'s `ALL_TOOLS`
  fixture** (line 10) to include the three new tools (the resource is not a
  tool, and `filterToolsForGroups`/`ALL_TOOLS` only ever describe tools):

```ts
const ALL_TOOLS = [
    'resolve_station',
    'get_departures',
    'get_arrivals',
    'get_service_detail',
    'find_services',
    'plan_journey',
    'get_line_delay_trend',
    'get_national_schedule_departures',
    'get_station_operator_stats'
];
```

  Re-run this file's existing assertions (`'{mcp-users} only: tools/list
  returns exactly resolve_station/find_services'` at line 42 and its
  neighbor at line 39) and confirm they now also list the three new tool
  names in their expected output — `filterToolsForGroups(ALL_TOOLS, [],
  'mcp-live-boards')` and `filterToolsForGroups(ALL_TOOLS, ['mcp-users'],
  'mcp-live-boards')` must both now equal `['resolve_station',
  'find_services', 'get_line_delay_trend', 'get_national_schedule_departures',
  'get_station_operator_stats']` (order matching `ALL_TOOLS`'s own order,
  since `filterToolsForGroups` preserves input order via `Array.filter`).
  Update both `.toEqual(...)` assertions to match.

- [ ] **Step 3: Update `test/app.test.ts`'s two tool-count assertions.**
  The `'mcp-users-only caller (no mcp-live-boards): tools/list omits the
  four metered board tools'` test (line 271) needs three new
  `expect(response.text).toContain('"name":"..."')` lines for the three new
  tools, alongside the existing `resolve_station`/`find_services` lines
  (285-286):

```ts
            expect(response.text).toContain('"name":"resolve_station"');
            expect(response.text).toContain('"name":"find_services"');
            expect(response.text).toContain('"name":"get_line_delay_trend"');
            expect(response.text).toContain('"name":"get_national_schedule_departures"');
            expect(response.text).toContain('"name":"get_station_operator_stats"');
            expect(response.text).not.toContain('"name":"get_departures"');
            expect(response.text).not.toContain('"name":"get_arrivals"');
            expect(response.text).not.toContain('"name":"get_service_detail"');
            expect(response.text).not.toContain('"name":"plan_journey"');
```

  The `'fully entitled caller (mcp-users + mcp-live-boards): tools/list
  includes all six tools'` test (line 312) needs its title and loop updated
  — it now covers nine tools:

```ts
        it('fully entitled caller (mcp-users + mcp-live-boards): tools/list includes all nine tools', async () => {
            // ... unchanged setup ...
            for (const tool of [
                'resolve_station',
                'find_services',
                'get_departures',
                'get_arrivals',
                'get_service_detail',
                'plan_journey',
                'get_line_delay_trend',
                'get_national_schedule_departures',
                'get_station_operator_stats'
            ]) {
                expect(response.text).toContain(tool);
            }
        });
```

  `resources/list` is a separate MCP method from `tools/list` — the
  `data-freshness` resource does not appear in either assertion above and
  needs no change to these two tests; Task 5's own test already covers its
  `resources/list` visibility.

- [ ] **Step 4: Run the full suite**

```bash
npm run typecheck
npm test
```

Expected: every test passes, including the newly-updated assertions in
`test/oauth-access-groups.test.ts` and `test/app.test.ts`.

- [ ] **Step 5: Commit**

```bash
git add src/server.ts test/oauth-access-groups.test.ts test/app.test.ts
git commit -m "Wire Phase 0 tools/resource into buildServer; update tool-count fixtures to nine tools"
```

**Phase 0 is complete after this task** — four new capabilities, zero
`crates/api` changes, verified end to end.

---

# Phase 1 — finish the TRUST-corroboration tier

## Task 7: `DsApiClient.getTrackedTrainStatus` — the missing fifth (now: eleventh) client method, the one that sends a session cookie

**Files:**
- Modify: `src/ds/client.ts`
- Test: `test/ds-client.test.ts` (extend)

**Interfaces:**
- Produces: `DsApiClient.getTrackedTrainStatus(trainUid: string, date: string,
  sessionCookieValue: string): Promise<DsTrackedTrainState | null>`, and the
  `DsTrackedTrainState` interface.
- Consumed by: Task 8 (`annotateLeg.ts`), Task 9 (`get_tracked_train_status`
  tool).
- **Depends on:** Task 1 (reuses the `getJson(url, extraHeaders, options)`
  signature Task 1 already extended — no further `getJson` change needed).

This is the design spec's own most directly actionable finding: `GET
/Train/by-uid/{train_uid}/{date}` (`crates/api/src/routes/train.rs:71-73`
route registration, handler at `train.rs:471-500`) already requires
`AuthenticatedUser` (a real DS session — `train.rs:473`) and already 404s
uniformly for both "no resolved tracked train for that uid/date" (`train.rs:479-484`)
and "resolved, but owned by someone else" (`train.rs:486-497`) — the exact
same 404-for-both-outcomes honesty convention this client's `notFoundIsNull`
mode (Task 1) already models. `AuthenticatedUser`'s session cookie is named
`distant_signal_session` (`crates/api/src/auth.rs:126`,
`SESSION_COOKIE_NAME`), matching exactly what
`src/oauth/accessGroups.ts::lookupDsGroups` already sends
(`accessGroups.ts:103-107`) for the same purpose.

- [ ] **Step 1: New interface**, added alongside `DsHistoryRetention`:

```ts
/**
 * `GET /Train/by-uid/{uid}/{date}`'s response shape.
 * `crates/api/src/data/train_tracking.rs::TrackedTrainState`
 * (`train_tracking.rs:371-395`) -- `#[serde(rename_all = "camelCase")]`
 * (`train_tracking.rs:371`), so every field below is already camelCase on
 * the wire.
 */
export interface DsTrackedTrainState {
    id: number;
    serviceDate: string;
    pinOriginCrs: string;
    pinDestinationCrs: string | null;
    pinOriginName: string | null;
    pinDestinationName: string | null;
    resolutionStatus: string;
    trainUid: string | null;
    trainId: string | null;
    status: string | null;
    lastReportedLocation: string | null;
    lastEventType: string | null;
    delayMinutes: number | null;
    nextCallingPoint: string | null;
    etaNext: string | null;
    etaSource: string | null;
}
```

- [ ] **Step 2: New method**, added after `getLineStatus`:

```ts
    /**
     * `GET /Train/by-uid/{uid}/{date}` -- the ONE method on this client
     * that sends a `Cookie` header. Every other method is deliberately
     * anonymous (Decision 4 of the prior integration design); this route
     * requires `AuthenticatedUser` server-side and answers on behalf of a
     * specific DS user's own tracked-train pin, so there is no anonymous
     * equivalent to fall back to. `sessionCookieValue` is the caller's own
     * raw `distant_signal_session` cookie value, already resolved by
     * src/app.ts's `/mcp` handler before any tool/resource handler runs
     * (`req.auth.extra.dsSessionCookieValue`) -- this method never derives
     * or caches a session itself.
     *
     * Returns `null` (never throws) on a 404 -- DS 404s uniformly for "not
     * tracked by anyone" and "tracked, but not by this session's user"
     * (`train.rs:479-497`), both of which are ordinary, expected outcomes
     * for the great majority of `(uid, date)` pairs a caller might ask
     * about, not a DS failure.
     */
    async getTrackedTrainStatus(trainUid: string, date: string, sessionCookieValue: string): Promise<DsTrackedTrainState | null> {
        const url = `${this.baseUrl}/Train/by-uid/${encodeURIComponent(trainUid)}/${encodeURIComponent(date)}`;
        return this.getJson(url, { Cookie: `distant_signal_session=${sessionCookieValue}` }, { notFoundIsNull: true }) as Promise<DsTrackedTrainState | null>;
    }
```

- [ ] **Step 3: Extend `test/ds-client.test.ts`**

```ts
describe('getTrackedTrainStatus', () => {
    it('sends the session cookie and returns the state on success', async () => {
        const state = {
            id: 1, serviceDate: '2026-09-04', pinOriginCrs: 'PAD', pinDestinationCrs: 'BRI',
            pinOriginName: 'London Paddington', pinDestinationName: 'Bristol Temple Meads',
            resolutionStatus: 'resolved', trainUid: 'C11052', trainId: null,
            status: 'running', lastReportedLocation: 'Reading', lastEventType: 'DEPARTURE',
            delayMinutes: 3, nextCallingPoint: 'Swindon', etaNext: '2026-09-04T09:15:00Z', etaSource: 'trust'
        };
        let sentCookie: string | null = null;
        const fetchImpl = (async (_url: string, init?: RequestInit) => {
            sentCookie = new Headers(init?.headers).get('Cookie');
            return jsonResponse(state);
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const result = await client.getTrackedTrainStatus('C11052', '2026-09-04', 'abc123');
        expect(result).toEqual(state);
        expect(sentCookie).toBe('distant_signal_session=abc123');
    });

    it('returns null, not a thrown error, on a 404 (untracked or not-owned)', async () => {
        const fetchImpl = (async () => new Response('not found', { status: 404 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        expect(await client.getTrackedTrainStatus('NOSUCHUID', '2026-09-04', 'abc123')).toBeNull();
    });

    it('still throws DsUnavailableError on a 500', async () => {
        const fetchImpl = (async () => new Response('boom', { status: 500 })) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await expect(client.getTrackedTrainStatus('C11052', '2026-09-04', 'abc123')).rejects.toBeInstanceOf(DsUnavailableError);
    });

    it('never sends this Cookie header from a sibling anonymous call on the same client instance', async () => {
        const calls: RequestInit[] = [];
        const fetchImpl = (async (_url: string, init?: RequestInit) => {
            calls.push(init ?? {});
            return jsonResponse([]);
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        await client.getLines();
        for (const init of calls) {
            expect(new Headers(init.headers).has('Cookie')).toBe(false);
        }
    });
});
```

- [ ] **Step 4: Run the tests, typecheck**

```bash
npm test -- ds-client
npm run typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ds/client.ts test/ds-client.test.ts
git commit -m "DsApiClient: add getTrackedTrainStatus, the one session-cookie-carrying call (GET /Train/by-uid)"
```

---

## Task 8: Wire the TRUST-corroboration tier into `annotateLeg.ts` and `plan-journey.ts`

**Files:**
- Modify: `src/ds/annotateLeg.ts`, `src/server.ts`, `src/tools/plan-journey.ts`
- Test: `test/ds-annotate-leg.test.ts` (extend), `test/tools-plan-journey.test.ts` (extend)

**Interfaces:**
- Consumes: `DsApiClient.getTrackedTrainStatus` (Task 7).
- Produces: `LiveStatus.trackedTrainState` (new optional field),
  `AnnotateLegDeps.dsSessionCookieValue` (new optional field),
  `AnnotatableLeg.uid`/`AnnotatableLeg.date` (new required fields).
- **Depends on:** Task 7.

This closes the gap `src/app.ts`'s own comment names explicitly
(`app.ts:157-166`): `dsSessionCookieValue` is already extracted from the
verified bearer token and already threaded as far as `server.ts`'s `DsDeps`
interface (`server.ts:22-31`) — but `buildServer` never forwards it into
`PlanJourneyDeps.ds` (`AnnotateLegDeps`, `annotateLeg.ts:37-40`), and
`AnnotateLegDeps` itself carries no field for it at all. This task closes
both gaps and adds the actual TRUST-lookup call.

- [ ] **Step 1: Add `dsSessionCookieValue` to `AnnotateLegDeps` and
  `trackedTrainState` to `LiveStatus`** (`src/ds/annotateLeg.ts:24-40`):

```ts
export interface LiveStatus {
    matchConfidence: MatchConfidence;
    lineId?: string;
    candidateLineIds?: string[];
    severity?: string;
    reason?: string;
    dataQuality?: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
    sampleStats?: { total: number; delayed: number; cancelled: number; skipped: number; avgDelayMinutes: number };
    legInDisruptionScope?: boolean;
    /** Individual-train TRUST corroboration (Decision 2, the design's Phase 1) -- present only when the calling DS user has personally tracked this leg's own (uid, date) via POST /Train/track. Additional corroboration alongside whatever the line-status tier (above) already found, never a replacement for it. */
    trackedTrainState?: {
        status: string | null;
        lastReportedLocation: string | null;
        delayMinutes: number | null;
        etaNext: string | null;
    };
    asOf?: string;
    source: LiveStatusSource;
}

export interface AnnotateLegDeps {
    client: DsApiClient;
    catalogue: DsLineCatalogue;
    /** The calling DS user's own session cookie value, threaded from src/app.ts's `/mcp` handler via server.ts's `DsDeps` -- see this field's own doc comment there. Undefined only for a caller/test that never went through Gate 1's `mcp-users` check (Gate 1 always resolves a real session before buildServer runs in production, so this is effectively always set at runtime -- see get_tracked_train_status's own defensive-only null check, src/tools/tracked-train-status.ts). */
    dsSessionCookieValue?: string;
}

export interface AnnotatableLeg {
    operator: string | null;
    fromCrs: string | null;
    toCrs: string | null;
    departureAt: string | null;
    /** The leg's own schedule UID -- always present for a TrainLeg (csa.ts/raptor.ts's own JourneyLeg.uid). */
    uid: string;
    /** The whole journey's own requested service date (YYYY-MM-DD) -- matches what train_tracking::get_by_uid_and_date resolves against server-side; a UK rail schedule's "date" is the traffic day the service started, not necessarily the calendar date of any individual leg's departureAt instant, so this is the plan_journey request's own `date` input, not derived per-leg. */
    date: string;
}
```

- [ ] **Step 2: Restructure `annotateLeg` to attempt the TRUST tier after
  the line-status tier, merging rather than replacing** (replaces the whole
  function body, `annotateLeg.ts:74-116`):

```ts
export async function annotateLeg(leg: AnnotatableLeg, deps: AnnotateLegDeps, now: Date): Promise<LiveStatus | undefined> {
    if (leg.departureAt === null) {
        return undefined;
    }
    const minutesUntilDeparture = (Date.parse(leg.departureAt) - now.getTime()) / 60_000;
    if (minutesUntilDeparture > LIVE_BOARD_HORIZON_MINUTES) {
        return undefined;
    }

    let result: LiveStatus;
    try {
        const candidateLines = await deps.catalogue.catalogueLines();
        const byOperator = leg.operator ? candidateLines.filter(line => line.operators.includes(leg.operator!)) : [];
        const definitionEntries = await Promise.all(
            byOperator.map(async line => [line.id, await deps.catalogue.definition(line.id)] as const)
        );
        const definitions = new Map(definitionEntries.filter((entry): entry is [string, NonNullable<(typeof entry)[1]>] => entry[1] !== null));

        const match = matchLegToLine(leg.operator, leg.fromCrs, leg.toCrs, byOperator, definitions);

        if (match.matchConfidence === 'none' || match.matchConfidence === 'ambiguous') {
            result = { ...match, source: 'no_match' };
        } else {
            const report = await deps.client.getLineStatus(match.lineId!);
            const status = report.lineStatuses[0];
            result = status
                ? {
                      ...match,
                      severity: status.statusSeverityDescription,
                      reason: status.reason,
                      dataQuality: status.dataQuality,
                      sampleStats: status.sampleStats,
                      legInDisruptionScope: legInDisruptionScope(status.disruption, leg.fromCrs, leg.toCrs),
                      asOf: report.computedAt,
                      source: 'ds_line_status'
                  }
                : { ...match, source: 'no_match' };
        }
    } catch {
        result = { matchConfidence: 'none', source: 'unavailable' };
    }

    // Individual-train (TRUST) corroboration -- Decision 2 of the Phase 0/1
    // design, the integration doc's originally-specified 3b.6, now finally
    // wired up. Best-effort and additive: a failure here never discards the
    // line-status tier's own result above, and a successful lookup never
    // overwrites a confident ds_line_status result's severity/reason -- it
    // only ever ADDS trackedTrainState, taking over `source` solely when
    // the line-status tier itself found nothing to say.
    if (deps.dsSessionCookieValue) {
        try {
            const trackedState = await deps.client.getTrackedTrainStatus(leg.uid, leg.date, deps.dsSessionCookieValue);
            if (trackedState) {
                result = {
                    ...result,
                    trackedTrainState: {
                        status: trackedState.status,
                        lastReportedLocation: trackedState.lastReportedLocation,
                        delayMinutes: trackedState.delayMinutes,
                        etaNext: trackedState.etaNext
                    },
                    source: result.source === 'ds_line_status' ? result.source : 'ds_tracked_train'
                };
            }
        } catch {
            // Best-effort, same posture as the line-status tier above.
        }
    }

    return result;
}
```

- [ ] **Step 3: Thread `date`/`uid` through `plan-journey.ts`'s `enrichLeg`/
  `enrichLegs`** (`src/tools/plan-journey.ts:219-288`). `enrichLeg` gains a
  `date: string` parameter, passed by `enrichLegs`, and the `annotateLeg`
  call site (`plan-journey.ts:251-255`) adds `uid`/`date`:

```ts
async function enrichLeg(
    store: TimetableStore,
    leg: JourneyLeg,
    dayAnchor: Date,
    previousArrival: Date | null,
    deps: AnnotateLegDeps,
    date: string
): Promise<RenderedLeg> {
    // ... unchanged transfer-leg branch ...

    const details: JourneyLegDetails = store.journeyLegDetails(leg);
    const departureAt = toInstant(leg.departure, reference);
    const arrivalAt = departureAt ? toInstant(leg.arrival, new Date(departureAt)) : toInstant(leg.arrival, reference);
    const from = stationRef(store, leg.fromTiploc, details.fromName);
    const to = stationRef(store, leg.toTiploc, details.toName);
    const liveStatus = await annotateLeg(
        { operator: details.operator, fromCrs: from.crs, toCrs: to.crs, departureAt, uid: leg.uid, date },
        deps,
        new Date()
    );
    // ... unchanged return ...
}

async function enrichLegs(store: TimetableStore, legs: JourneyLeg[], date: string, deps: AnnotateLegDeps): Promise<RenderedLeg[]> {
    const dayAnchor = new Date(`${date}T12:00:00.000Z`);
    const rendered: RenderedLeg[] = [];
    let previousArrival: Date | null = null;
    for (const leg of legs) {
        const enriched = await enrichLeg(store, leg, dayAnchor, previousArrival, deps, date);
        rendered.push(enriched);
        previousArrival = enriched.arrivalAt ? new Date(enriched.arrivalAt) : previousArrival;
    }
    return rendered;
}
```

- [ ] **Step 4: Add `trackedTrainState` to `plan-journey.ts`'s
  `liveStatusShape`** (`plan-journey.ts:1155-1168`):

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
    trackedTrainState: z
        .object({
            status: z.string().nullable(),
            lastReportedLocation: z.string().nullable(),
            delayMinutes: z.number().nullable(),
            etaNext: z.string().nullable()
        })
        .optional(),
    asOf: z.string().optional(),
    source: z.enum(['ds_line_status', 'ds_tracked_train', 'unavailable', 'no_match'])
});
```

- [ ] **Step 5: Thread `dsSessionCookieValue` from `server.ts`'s `DsDeps`
  into `PlanJourneyDeps.ds`** (`src/server.ts:76-91`) — the one-line fix
  `app.ts`'s own comment names as outstanding:

```ts
        registerPlanJourney(server, {
            ...(deps.plan ?? DEFAULT_PLAN_DEPS),
            ds: { client: ds.client, catalogue, dsSessionCookieValue: ds.dsSessionCookieValue }
        });
```

- [ ] **Step 6: Extend `test/ds-annotate-leg.test.ts`.** Every existing
  test's `AnnotatableLeg` object literal now needs `uid`/`date` fields added
  (e.g. `{ operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: ...,
  uid: 'X00001', date: '2026-09-01' }`) — update all six existing test cases
  first, confirm they still pass unmodified in behavior, then add:

```ts
describe('annotateLeg -- TRUST corroboration tier', () => {
    it('attaches trackedTrainState and keeps source ds_line_status when the line-status tier already succeeded', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({
            '/public/lines/': { operators: ['CC'], stations: ['FEN', 'BKG'] },
            '/public/lines': [{ id: 'c2c', name: 'c2c', category: 'rail', operators: ['CC'], source: 'catalogue' }],
            '/Line/c2c/Status': [
                { id: 'c2c', name: 'c2c', modeName: 'rail', operators: ['CC'], computedAt: '2026-09-01T11:55:00Z', lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '', dataQuality: 'ldbws-inferred', validityPeriods: [] }] }
            ],
            '/Train/by-uid/': { id: 1, serviceDate: '2026-09-01', pinOriginCrs: 'FEN', pinDestinationCrs: 'BKG', pinOriginName: null, pinDestinationName: null, resolutionStatus: 'resolved', trainUid: 'X00001', trainId: null, status: 'running', lastReportedLocation: 'Stratford', lastEventType: 'DEPARTURE', delayMinutes: 2, nextCallingPoint: 'BKG', etaNext: '2026-09-01T12:10:00Z', etaSource: 'trust' }
        });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg(
            { operator: 'CC', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture, uid: 'X00001', date: '2026-09-01' },
            { client, catalogue, dsSessionCookieValue: 'abc123' },
            now
        );
        expect(result?.source).toBe('ds_line_status');
        expect(result?.trackedTrainState).toEqual({ status: 'running', lastReportedLocation: 'Stratford', delayMinutes: 2, etaNext: '2026-09-01T12:10:00Z' });
    });

    it('reports source ds_tracked_train when the line-status tier found no match but TRUST corroboration succeeded', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({
            '/public/lines': [],
            '/Train/by-uid/': { id: 1, serviceDate: '2026-09-01', pinOriginCrs: 'FEN', pinDestinationCrs: 'BKG', pinOriginName: null, pinDestinationName: null, resolutionStatus: 'resolved', trainUid: 'X00001', trainId: null, status: 'running', lastReportedLocation: 'Stratford', lastEventType: 'DEPARTURE', delayMinutes: 0, nextCallingPoint: 'BKG', etaNext: null, etaSource: null }
        });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg(
            { operator: 'ZZ', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture, uid: 'X00001', date: '2026-09-01' },
            { client, catalogue, dsSessionCookieValue: 'abc123' },
            now
        );
        expect(result?.source).toBe('ds_tracked_train');
        expect(result?.trackedTrainState?.status).toBe('running');
    });

    it('never calls getTrackedTrainStatus when no dsSessionCookieValue is supplied', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        let calledTrustRoute = false;
        const fetchImpl = (async (url: string) => {
            if (url.includes('/Train/by-uid/')) {
                calledTrustRoute = true;
            }
            return new Response('[]', { status: 200 });
        }) as unknown as typeof fetch;
        const client = new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl });
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        await annotateLeg({ operator: 'ZZ', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture, uid: 'X00001', date: '2026-09-01' }, { client, catalogue }, now);
        expect(calledTrustRoute).toBe(false);
    });

    it('degrades silently (no trackedTrainState field, no thrown error) when the TRUST lookup itself fails', async () => {
        const soonDeparture = new Date(now.getTime() + 30 * 60_000).toISOString();
        const client = clientReturning({ '/public/lines': [] }); // /Train/by-uid/ falls through to the 404 default
        const catalogue = new DsLineCatalogue({ client, ttlMs: 60_000 });
        const result = await annotateLeg(
            { operator: 'ZZ', fromCrs: 'FEN', toCrs: 'BKG', departureAt: soonDeparture, uid: 'X00001', date: '2026-09-01' },
            { client, catalogue, dsSessionCookieValue: 'abc123' },
            now
        );
        expect(result).toEqual({ matchConfidence: 'none', source: 'no_match' });
        expect(result?.trackedTrainState).toBeUndefined();
    });
});
```

- [ ] **Step 7: Extend `test/tools-plan-journey.test.ts`** with one test
  confirming a `plan_journey` call whose `PlanJourneyDeps.ds` carries a
  `dsSessionCookieValue` produces a `liveStatus.trackedTrainState` on a leg
  whose `uid` the mocked DS client recognizes as tracked — model this test
  directly on that file's own existing DS-mocking helper (`clientReturning`-
  style, matching `ds-annotate-leg.test.ts`'s pattern) rather than
  duplicating a second one; read that file's own existing DS-mock helper
  first and reuse it.

- [ ] **Step 8: Run the tests, typecheck**

```bash
npm test -- ds-annotate-leg tools-plan-journey
npm run typecheck
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ds/annotateLeg.ts src/server.ts src/tools/plan-journey.ts test/ds-annotate-leg.test.ts test/tools-plan-journey.test.ts
git commit -m "Wire the TRUST-corroboration tier into annotateLeg.ts/plan_journey (Decision 2, integration doc's original 3b.6)"
```

---

## Task 9: `get_tracked_train_status(uid, date)` standalone tool

**Files:**
- Create: `src/tools/tracked-train-status.ts`
- Modify: `src/server.ts`, `test/oauth-access-groups.test.ts`, `test/app.test.ts`
- Test: `test/tools-tracked-train-status.test.ts`

**Interfaces:**
- Produces: `registerTrackedTrainStatus(server: McpServer, ds: DsApiClient,
  dsSessionCookieValue: string | undefined): void`.
- Consumed by: `server.ts`'s `buildServer`.
- **Depends on:** Task 7 (`getTrackedTrainStatus`).

Per Decision 7: sits under the whole-server `mcp-users` gate only, exactly
like every other tool this plan adds — **not** `mcp-live-boards`, and no new
access group. `/Train/by-uid` is DS-hosted, not Darwin-metered; its
`blend_darwin_eta` overlay (`train.rs:499`) is a best-effort addition already
folded into `TrackedTrainState` server-side, not a separate metered call this
tool itself makes.

- [ ] **Step 1: `src/tools/tracked-train-status.ts`**

```ts
import type { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { DsApiClient } from '../ds/client.js';

const trackedTrainStateShape = {
    id: z.number(),
    serviceDate: z.string(),
    pinOriginCrs: z.string(),
    pinDestinationCrs: z.string().nullable(),
    pinOriginName: z.string().nullable(),
    pinDestinationName: z.string().nullable(),
    resolutionStatus: z.string(),
    trainUid: z.string().nullable(),
    trainId: z.string().nullable(),
    status: z.string().nullable(),
    lastReportedLocation: z.string().nullable(),
    lastEventType: z.string().nullable(),
    delayMinutes: z.number().nullable(),
    nextCallingPoint: z.string().nullable(),
    etaNext: z.string().nullable(),
    etaSource: z.string().nullable()
};

export function registerTrackedTrainStatus(server: McpServer, ds: DsApiClient, dsSessionCookieValue: string | undefined): void {
    server.registerTool(
        'get_tracked_train_status',
        {
            title: 'Live status of a train you have personally tracked',
            description:
                'Get the live TRUST-derived status of a specific scheduled train service (by its schedule UID ' +
                "and service date) that YOU have personally pinned via Distant Signal's Track a Train feature. " +
                'Returns tracked: false (not an error) when this (uid, date) pair is not one you have tracked ' +
                "-- this tool never reveals another user's tracked trains, and never guesses.",
            inputSchema: {
                uid: z.string().min(1).describe('The schedule UID of the train, e.g. "C11052".'),
                date: z.string().describe('The service date, ISO YYYY-MM-DD, e.g. "2026-09-04".')
            },
            outputSchema: {
                tracked: z.boolean(),
                state: z.object(trackedTrainStateShape).optional()
            }
        },
        async ({ uid, date }) => {
            if (!dsSessionCookieValue) {
                // Defense in depth only -- src/app.ts's /mcp handler already
                // requires a valid, group-checked DS session to pass Gate 1
                // (mcp-users) before buildServer is ever called, so this
                // branch should be unreachable in production. Never a
                // fallback DS call of any kind.
                return {
                    content: [{ type: 'text' as const, text: 'No Distant Signal session available for this request.' }],
                    structuredContent: { tracked: false }
                };
            }
            const state = await ds.getTrackedTrainStatus(uid, date, dsSessionCookieValue);
            if (!state) {
                return {
                    content: [{ type: 'text' as const, text: `You have not tracked train ${uid} on ${date} (or it hasn't resolved yet).` }],
                    structuredContent: { tracked: false }
                };
            }
            const text =
                `${uid} on ${date}: ${state.status ?? 'unknown status'}` +
                (state.lastReportedLocation ? `, last reported at ${state.lastReportedLocation}` : '') +
                (state.delayMinutes !== null ? `, ${state.delayMinutes} min delay` : '');
            return {
                content: [{ type: 'text' as const, text }],
                structuredContent: { tracked: true, state }
            };
        }
    );
}
```

- [ ] **Step 2: Register unconditionally in `buildServer`** (`src/server.ts`),
  alongside the Phase 0 registrations from Task 6:

```ts
import { registerTrackedTrainStatus } from './tools/tracked-train-status.js';
// ...
    registerDataFreshnessResource(server, ds.client);
    registerTrackedTrainStatus(server, ds.client, ds.dsSessionCookieValue);
```

- [ ] **Step 3: `test/tools-tracked-train-status.test.ts`**

```ts
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { describe, expect, it } from 'vitest';
import { DsApiClient } from '../src/ds/client.js';
import { registerTrackedTrainStatus } from '../src/tools/tracked-train-status.js';

async function connect(fetchImpl: typeof fetch, sessionCookieValue: string | undefined = 'abc123') {
    const server = new McpServer({ name: 'test', version: '1.0.0' });
    registerTrackedTrainStatus(server, new DsApiClient({ baseUrl: 'https://ds.example.com', fetchImpl }), sessionCookieValue);
    const client = new Client({ name: 'test', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
    return client;
}

const trackedState = {
    id: 1, serviceDate: '2026-09-04', pinOriginCrs: 'PAD', pinDestinationCrs: 'BRI',
    pinOriginName: 'London Paddington', pinDestinationName: 'Bristol Temple Meads',
    resolutionStatus: 'resolved', trainUid: 'C11052', trainId: null,
    status: 'running', lastReportedLocation: 'Reading', lastEventType: 'DEPARTURE',
    delayMinutes: 3, nextCallingPoint: 'Swindon', etaNext: '2026-09-04T09:15:00Z', etaSource: 'trust'
};

describe('get_tracked_train_status tool', () => {
    it('is advertised by the server', async () => {
        const client = await connect((async () => new Response('not found', { status: 404 })) as unknown as typeof fetch);
        const { tools } = await client.listTools();
        expect(tools.map(t => t.name)).toContain('get_tracked_train_status');
    });

    it('returns tracked: true with the state for a tracked train', async () => {
        const client = await connect((async () => new Response(JSON.stringify(trackedState), { status: 200 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_tracked_train_status', arguments: { uid: 'C11052', date: '2026-09-04' } });
        const structured = result.structuredContent as { tracked: boolean; state?: typeof trackedState };
        expect(structured.tracked).toBe(true);
        expect(structured.state).toEqual(trackedState);
        expect(JSON.stringify(result.content)).toMatch(/running/);
    });

    it('returns tracked: false, not an error, for an untracked (uid, date)', async () => {
        const client = await connect((async () => new Response('not found', { status: 404 })) as unknown as typeof fetch);
        const result = await client.callTool({ name: 'get_tracked_train_status', arguments: { uid: 'NOSUCHUID', date: '2026-09-04' } });
        expect(result.isError).toBeFalsy();
        expect((result.structuredContent as { tracked: boolean }).tracked).toBe(false);
    });

    it('sends the Cookie header carrying the caller\'s own session', async () => {
        let sentCookie: string | null = null;
        const fetchImpl = (async (_url: string, init?: RequestInit) => {
            sentCookie = new Headers(init?.headers).get('Cookie');
            return new Response(JSON.stringify(trackedState), { status: 200 });
        }) as unknown as typeof fetch;
        const client = await connect(fetchImpl, 'my-session-value');
        await client.callTool({ name: 'get_tracked_train_status', arguments: { uid: 'C11052', date: '2026-09-04' } });
        expect(sentCookie).toBe('distant_signal_session=my-session-value');
    });

    it('reports tracked: false, not a DS call, when no session is available (defense in depth)', async () => {
        let called = false;
        const fetchImpl = (async () => {
            called = true;
            return new Response('boom', { status: 500 });
        }) as unknown as typeof fetch;
        const client = await connect(fetchImpl, undefined);
        const result = await client.callTool({ name: 'get_tracked_train_status', arguments: { uid: 'C11052', date: '2026-09-04' } });
        expect(result.isError).toBeFalsy();
        expect((result.structuredContent as { tracked: boolean }).tracked).toBe(false);
        expect(called).toBe(false);
    });
});
```

- [ ] **Step 4: Update `test/oauth-access-groups.test.ts`'s `ALL_TOOLS`**
  (extended again in Task 6) to also include `'get_tracked_train_status'`,
  and re-check its two `filterToolsForGroups` assertions include it in their
  expected output (it requires only `mcp-users`, same as the three Phase 0
  tools).

- [ ] **Step 5: Update `test/app.test.ts`'s two tool-count assertions again**
  — add `'"name":"get_tracked_train_status"'` to the mcp-users-only test's
  `toContain` list, and add `'get_tracked_train_status'` to the fully-
  entitled test's loop array (now ten tools total; update that test's own
  title from "all nine tools" to "all ten tools").

- [ ] **Step 6: Run the full suite**

```bash
npm run typecheck
npm test
```

Expected: every test passes.

- [ ] **Step 7: Commit**

```bash
git add src/tools/tracked-train-status.ts src/server.ts test/tools-tracked-train-status.test.ts test/oauth-access-groups.test.ts test/app.test.ts
git commit -m "Add get_tracked_train_status tool over DsApiClient.getTrackedTrainStatus, gated by mcp-users only (Decision 7)"
```

**Phase 1 is complete after this task.**

---

## Task 10: Full Phase 0+1 verification pass

**Files:** none (verification only).

**Depends on:** Tasks 1–9.

- [ ] **Step 1: Full build, typecheck, test**

```bash
npm run build
npm run typecheck
npm test
```

Expected: `build`/`typecheck` exit 0; every test in the suite passes,
including all nine new/updated test files this plan added and the three
existing files it modified (`test/ds-client.test.ts`,
`test/ds-annotate-leg.test.ts`, `test/tools-plan-journey.test.ts`,
`test/oauth-access-groups.test.ts`, `test/app.test.ts`).

- [ ] **Step 2: Confirm the anonymous-by-default constraint held**

```bash
npm test -- ds-client 2>&1 | grep -A2 "never sends an Authorization header"
```

Expected: that pre-existing test (unmodified by this plan) still passes,
confirming Task 1's `getJson` extension and Task 7's new Cookie-sending
method did not regress the anonymity guarantee for every other method.

- [ ] **Step 3: Confirm no stray `crates/api`/other-repo file was touched**

```bash
cd /workspaces/distant-signal-mcp && git status --porcelain
```

Expected: only files listed in this plan's own Global Constraints "File
scope" section appear as modified/new. (Run from `/workspaces/distant-signal-mcp`,
not this repo — this plan's own changes live entirely there.)

- [ ] **Step 4: Final summary commit note (optional, informational only)** —
  no code change; if the executing agent/human wants a marker commit noting
  Phase 0+1 completion (e.g. for a PR description), it can be added here,
  but is not required by this plan.

Phase 0 and Phase 1 are now both complete: four new tools/resource over
already-public routes, and the TRUST-corroboration tier the integration
doc originally specified over a prior session's design work. Phase 2
(`GET /public/lines/{id}/schedule`, a public `stanox_crs` mirror) and Phase 3
(`train_movement_events.raw_body`, `full_coverage_train_state`) remain
separate, later, not-yet-planned work per the design spec's own
recommendation.
