# Design: Frontend (Next.js + Mantine)

## Goal

Build a Next.js + TypeScript frontend, styled with Mantine, that consumes
the now-complete read API (`GET /Line/Mode/{mode}/Status`, `GET
/Line/{ids}/Status`, `GET /StopPoint/{crs}/Disruption`, `GET
/Line/{id}/Status/{from}/to/{to}`) and presents it as a TfL-style line
status dashboard. This is the third and final piece of the original
three-part decomposition (backend aggregator+read-API, LDBWS sampler
poller, frontend) — both backend pieces are done and merged.

## Naming correction (recorded for posterity)

The original request named "TapTap" as the design system. This turned out
to be a mishearing of **Tiptap** (`@tiptap/react` — a headless rich-text
editor toolkit, not a component/design system), surfaced by fetching
Tiptap's actual Next.js docs during brainstorming. Tiptap would only make
sense here for an admin content-authoring feature (freeform editorial
notes/advisories) — which doesn't exist in this system today and has no
backend support (no storage, no endpoint). That feature is explicitly
**out of scope**, deferred to a later spec if wanted. The actual design
system for this spec is **Mantine v9** (`@mantine/core`, `@mantine/hooks`,
`@mantine/dates`), confirmed against Mantine's current Next.js setup guide
during brainstorming, not assumed from training data.

## Current relevant state (verified 2026-07-07)

- No frontend code exists anywhere in this repo (confirmed: no
  `package.json`/`next.config.*` anywhere in the tree).
- The read API is live and unauthenticated on the existing `api` crate
  (`crates/api/src/routes/line_status.rs`, `crates/api/src/render.rs`),
  merged and tested. Exact response shape verified by reading `render.rs`
  directly (not assumed) — see "API Contract" below.
- `crates/api/Cargo.toml` already declares `tower-http`'s `"cors"` feature,
  but **no `CorsLayer` is applied anywhere** in `crates/api/src/main.rs`
  today (confirmed by grep) — this spec is what first wires it up.
- `docker-compose.yml` runs `api` on `${API_HOST_PORT:-8080}`, mapped to
  the host — the frontend's local dev / docker-compose config points at
  this.

## Decisions (from brainstorming)

1. **Design system: Mantine v9** (`@mantine/core`, `@mantine/hooks`, plus
   `@mantine/dates` for the history page's date-range picker), Next.js App
   Router, TypeScript. Setup follows Mantine's official Next.js guide
   verbatim (PostCSS config with `postcss-preset-mantine` +
   `postcss-simple-vars`, `MantineProvider` + `ColorSchemeScript` in
   `app/layout.tsx`, `'use client'` on interactive components since
   Mantine components require context).
2. **CORS: enabled on the backend** (`crates/api/src/main.rs` gets a
   `CorsLayer`), not proxied through Next.js. Since all four read
   endpoints are already unauthenticated, read-only GETs of public data —
   mirroring TfL's own publicly-CORS-enabled API — the policy is
   permissive (`Any` origin, GET only), not a configurable allowlist:
   there is no credential/cookie exposure to protect against, and an
   allowlist would add config surface for no real benefit. This also
   matches DESIGN.md's stated compatibility goal more literally than a
   proxy would (TfL's own API is directly browser-callable).
3. **Frontend lives at `frontend/`** in this same repo, alongside
   `crates/`, `lines/`, `docs/` — not a separate repository.
4. **v1 scope covers all four endpoints**: dashboard, per-line detail,
   station-disruption lookup, and history. No admin/authoring features
   (explicitly deferred, see naming-correction section above).
5. **Data freshness**: dashboard and line-detail pages use Next.js
   segment-level revalidation (`fetch(url, { next: { revalidate: 30 } })`)
   to auto-refresh roughly in step with the aggregator's 60s recompute
   cadence, without client-side polling JS. Station lookup and history are
   on-demand/user-triggered (a CRS code or a date range is user input by
   nature), no auto-refresh.
6. **Testing**: Vitest + React Testing Library for pure logic (severity
   mapping, response-shape handling) and component rendering. No E2E
   tooling (Playwright etc.) for v1 — more than this scope needs.

## API Contract

Hand-written TypeScript types matching `render.rs`'s actual JSON output —
verified against the real Rust source during brainstorming, not
generated (no OpenAPI spec exists) and not assumed from the domain types'
Rust field names (which are snake_case internally; the wire format is
camelCase, built by hand in `render.rs` specifically so the two concerns
stay decoupled):

```ts
export interface ValidityPeriod {
  fromDate: string;   // RFC3339
  toDate: string | null;
  isNow: boolean;
}

export interface AffectedRoute {
  from: string;
  to: string;
}

export interface Disruption {
  category: string;   // "RealTime" | "PlannedWork" | "Information"
  description: string;
  affectedStops: string[];
  affectedRoutes: AffectedRoute[];
  source: string | null;
}

export interface LineStatus {
  statusSeverity: number;              // 0-14, 20, 21 — see severity.ts
  statusSeverityDescription: string;
  reason: string;
  dataQuality: "knowledgebase" | "ldbws-inferred" | "trust-inferred" | "planned";
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;             // present only when detail=true was requested and one exists
}

export interface LineStatusReport {
  $type: "NRStatus.LineStatusReport";
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
}

// GET /Line/{id}/Status/{from}/to/{to} adds one extra field per entry,
// on top of the same LineStatusReport shape (crates/api/src/routes/line_status.rs's
// get_line_status_history handler stamps this in after calling to_tfl_shape):
export interface LineStatusHistoryEntry extends LineStatusReport {
  computedAt: string;  // RFC3339
}
```

Four thin fetch wrappers in `frontend/lib/api.ts`, one per endpoint, each
returning the typed shape above (or throwing on non-2xx, letting the
calling Server Component's error boundary / `notFound()` handle it):
`getLineStatusForMode(mode)`, `getLineStatus(ids, detail)`,
`getStopPointDisruption(crs)`, `getLineStatusHistory(id, from, to)`.

## Severity Display

`frontend/lib/severity.ts` maps all 17 `Severity` values (fixed, small
enum — worth covering exhaustively rather than a fallback-heavy partial
map) to a Mantine color + short label, grouped by what a passenger
actually cares about:

| Group | Values | Color |
|---|---|---|
| Fine | `GoodService` (10) | green |
| Informational | `SpecialService` (0), `ExitOnly` (12), `NoStepFree` (13) | gray |
| Planned | `PlannedClosure` (4), `PartClosure` (5) | blue |
| Mild disruption | `MinorDelays` (9), `ReducedService` (7), `ChangeOfFrequency` (14), `Recovering` (20) | yellow |
| Severe disruption | `SevereDelays` (6), `Suspended` (2), `PartSuspended` (3), `Closed` (1), `PartClosed` (11), `BusService` (8), `Diverted` (21) | red |

`components/StatusBadge.tsx` renders one `LineStatus` as a colored Mantine
`Badge` using this table, driven off `statusSeverity` (the numeric field),
not `statusSeverityDescription` (the string field is displayed as the
badge's label, but the color decision uses the stable numeric enum, not a
string match).

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router, TypeScript, Mantine v9)        │
│                                                                 │
│  app/layout.tsx        MantineProvider, ColorSchemeScript, nav │
│  app/page.tsx          dashboard — GET /Line/Mode/{mode}/Status│
│  app/lines/[id]/page.tsx        line detail — GET /Line/{id}/Status?detail=true
│  app/lines/[id]/history/page.tsx  history — GET /Line/{id}/Status/{from}/to/{to}
│  app/stations/[crs]/page.tsx    station lookup — GET /StopPoint/{crs}/Disruption
│                                                                 │
│  lib/api.ts       thin fetch wrappers, one per endpoint        │
│  lib/types.ts     hand-written TS contract (see above)         │
│  lib/severity.ts  severity -> {color, label} table             │
│                                                                 │
│  components/StatusBadge.tsx                                    │
│  components/LineStatusCard.tsx     one line, used on dashboard │
│  components/DisruptionDetail.tsx   renders a Disruption object │
└───────────────────────────┬────────────────────────────────────┘
                             │ server-side fetch (Server Components),
                             │ API_BASE_URL env var
                             ▼
              ┌─────────────────────────────┐
              │ api crate (existing, this   │
              │ spec adds a CorsLayer only) │
              │  GET /Line/Mode/{mode}/Status│
              │  GET /Line/{ids}/Status      │
              │  GET /StopPoint/{crs}/Disruption
              │  GET /Line/{id}/Status/{from}/to/{to}
              └─────────────────────────────┘
```

Each page is a Server Component doing its own server-side fetch (no
client-side data-fetching library needed for v1 — Next.js's built-in
`fetch` + `revalidate` covers the dashboard/line-detail freshness need,
and station lookup/history are one-shot, user-triggered fetches). Server
Components use a server-only `API_BASE_URL` env var (e.g. `http://api:8080`
inside docker-compose, `http://localhost:8080` for local dev) — since CORS
is enabled per Decision 2, a future client-side feature *could* call the
API directly from the browser too, but nothing in v1 needs to.

## Error Handling

- A line ID that doesn't exist → the API 404s → the page calls Next.js's
  `notFound()`, rendering the framework's standard not-found UI.
- The API unreachable/erroring → caught at the page level, rendering a
  friendly "couldn't load status data" message via an `error.tsx` boundary
  per route segment — not an unhandled crash.
- An invalid/malformed CRS code or date range on the user-input pages →
  client-side form validation before the fetch fires (Mantine's form
  handling), not a raw API error surfaced to the user.

## Testing

- `lib/severity.ts`: unit tests covering all 17 severity values map to
  the correct color/label, plus an explicit test that an unrecognized
  numeric value doesn't crash (falls back to a defined default, e.g. gray
  + "Unknown").
- `lib/types.ts`/`lib/api.ts`: unit tests for the fetch wrappers using a
  mocked `fetch`, confirming correct URL construction (including comma-join
  for `/Line/{ids}/Status` and correct RFC3339 formatting for the history
  range) and correct error handling on non-2xx responses.
- `components/StatusBadge.tsx`, `LineStatusCard.tsx`, `DisruptionDetail.tsx`:
  React Testing Library render tests confirming the right text/color
  appears for representative severity values (at least one from each group
  in the table above) and that `detail`-gated fields (disruption) render
  only when present.

## Explicitly out of scope for this spec

- Admin content authoring / editorial notes (the original Tiptap
  misunderstanding) — no backend storage/endpoint exists for this; a
  future spec if wanted.
- Station name → CRS code lookup/autocomplete (the backend only accepts
  raw CRS codes today; DESIGN.md's own "Known gaps" section already flags
  CRS-from-free-text resolution as unbuilt).
- Client-side live polling / WebSocket push updates (segment revalidation
  is sufficient for this data's actual freshness cadence).
- E2E test tooling (Playwright, Cypress, etc.).
- Deployment/hosting configuration beyond local dev + docker-compose (a
  production hosting target, e.g. Vercel vs. self-hosted, is a separate
  decision not made here).

## Open questions for the planning phase (not blocking this design)

- Exact Mantine component choices for each page's layout (e.g.
  `SimpleGrid` vs `Stack` for the dashboard's line cards) — a
  planning-time detail, not a design one.
- Whether `docker-compose.yml` gets a `frontend` service in this same plan
  or a follow-up — the design doesn't depend on this being decided yet.
