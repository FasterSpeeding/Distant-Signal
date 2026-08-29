# Train Tracking Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give individual train tracking a user-facing surface — a manual tracking form, two read pages (by tracking id and by canonical uid/date), and a station-page shortcut — reusing this app's existing auth-prompt, refresh, and proxy patterns rather than building new ones.

**Architecture:** Pure frontend work in `frontend/` (Next.js App Router). One backend-adjacent change: widening `frontend/app/api/[...path]/route.ts`'s proxy allowlist so a browser-initiated `POST /api/Train/track` can reach the bare-root-mounted `/Train/track` backend route (today the proxy only reaches `/public/*`). Everything else is new pages/components consuming the already-live `GET /Train/{trackingId}` / `GET /Train/by-uid/{uid}/{date}` routes via `lib/api.ts`, and a Client Component form posting through the widened proxy. No backend code changes — `crates/api`'s train-tracking routes, `crates/trust-consumer`, and the account-system SSO session layer are all already implemented and merged (confirmed by direct inspection while writing this plan; see the note below each relevant task).

**Tech Stack:** Next.js App Router + TypeScript + Mantine v9 (`@mantine/core`, `@mantine/dates`' `DateTimePicker`), Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`).

**Spec:** `docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Decision N's table" refer to that document.

**Status note:** every prerequisite this plan depends on is already live, not merely planned. `crates/api/src/routes/train.rs`, `crates/api/src/data/train_tracking.rs`, `crates/api/src/data/eta_blend.rs`, and the account-system session layer (`AuthenticatedUser`, `frontend/components/PinToggle.tsx`, `frontend/components/AuthStatus.tsx`) were all read directly from the working tree while writing this plan and match the spec's description of them exactly, including the three enum `CHECK` constraints in `crates/api/migrations/20260828120000_train_tracking.sql` (`resolution_status`: `'pending'|'resolved'|'unresolved'`, line 72; `status`: `'awaiting_activation'|'en_route'|'cancelled'|'completed'`, line 123; `eta_source`: `'trust-propagated'|'darwin-estimated'`, line 136) and the confirmed gap in `crates/trust-consumer/src/journey.rs`'s `apply_movement` (always sets `status: "en_route"`, `'completed'` is unreachable from any current code path — see that file's own comment, lines ~32-38). No task in this plan is blocked on out-of-band setup.

## Global Constraints

- **No backend changes.** No task may modify anything under `crates/`. `crates/api`'s train-tracking routes are read-only inputs to this plan.
- **URL shapes are fixed, per Decision 2:** `/train/by-id/[trackingId]` (backed by `GET /Train/{tracking_id}`) and `/train/[uid]/[date]` (backed by `GET /Train/by-uid/{train_uid}/{date}`). Do not invent alternate paths.
- **Reads never go through the `/api/*` proxy.** Both new page files call `lib/api.ts` server-side with `cache: 'no-store'`, exactly like `getStopPointDisruption`. Only `POST /Train/track` (a mutation, from a Client Component) goes through the proxy — per Decision 4.
- **Auth UX for pin creation mirrors `PinToggle`'s `needsLogin`/401 pattern exactly, with one deliberate difference that must not be lost:** a 401 does **not** reset the form — all four typed field values stay exactly as entered while the login prompt renders alongside them (Decision 4's "no navigation away" — a form has real input worth protecting, unlike `PinToggle`'s toggle-and-forget click).
- **No new refresh mechanism.** Every new page relies solely on the existing global `AutoRefresh` (`frontend/components/AutoRefresh.tsx`, mounted once in `app/layout.tsx`, `router.refresh()` every 30s). No per-route opt-out, no manual "check now" button — per Decision 5.
- **`'completed'` and the "may have finished" heuristic are provisional, not confirmed, signals.** Per the migration's schema and `journey.rs`'s gap (see Status note above), no current backend code path ever produces `status: "completed"`; the frontend's own inference (`status: "en_route"` with a null `nextCallingPoint`) is a UI guess around that gap, not a real backend assertion. Every task that renders either case must visibly label it as inferred/provisional (e.g. "may have finished" — not "Completed" or "Arrived" stated as fact).
- **`frontend/components/OpenDataAttribution.tsx` already covers this feature's attribution** (its third paragraph, "Live train movement data from Network Rail's open data feeds" — confirmed present, unbranded, and distinct from the TfL/NRE lines above it, verified while writing this plan). No task may add, duplicate, or restyle attribution copy, add a logo, or use "official" framing anywhere in this feature.
- **Testing convention:** colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`) — matching every existing frontend test in this repo. Every task's verification step runs `npm test` and `npm run build` (both from `frontend/`; `next build` type-checks the whole project) and requires both to pass with no new failures.
- **Out of scope, per the spec's own "Explicitly out of scope" section — no task may build any of these:** a per-departure "track this train" action (blocked on a public departures-read endpoint that doesn't exist), a full stop-by-stop journey timeline (blocked on a public `train_movement_events` read endpoint that doesn't exist), a per-route `AutoRefresh` opt-out/interval, persisting/auto-resubmitting form state across the OIDC login redirect round-trip, or a "my tracked trains" list page.

---

### Task 1: Widen the `/api/[...path]/route.ts` proxy to reach `Train/*`

**Files:**
- Modify: `frontend/app/api/[...path]/route.ts`
- Create: `frontend/app/api/[...path]/route.test.ts` (no test file exists for this route today — confirmed by search; this is new coverage, not an extension)

**Interfaces:**
- Produces: a widened `proxy()` that forwards `/api/Train/...` requests to `${API_BASE_URL}/Train/...` (no `/public/` prefix inserted) while every other `/api/...` request still gets `/public/` prepended exactly as before. Still 400s if the *resolved* target pathname starts with neither `/public/` nor `/Train/`.
- Consumed by: Task 5 (`TrackTrainForm` calls `fetch('/api/Train/track', ...)`).

This is the only place `/Train/track` is reachable from the browser — `/Train/{trackingId}` and `/Train/by-uid/{uid}/{date}` (the two read routes) are **not** proxied at all; they're public on the backend and fetched server-side directly (Task 2), per Decision 4.

- [ ] **Step 1: Read the current file to confirm the exact text to change**

`frontend/app/api/[...path]/route.ts`'s `proxy()` function currently builds its target as:

```ts
const target = new URL(`${process.env.API_BASE_URL}/public/${path.join('/')}${req.nextUrl.search}`);
if (!target.pathname.startsWith('/public/')) {
  return new NextResponse('invalid path', { status: 400 });
}
```

- [ ] **Step 2: Replace it with a two-prefix resolver**

Replace the block above with:

```ts
// Backend prefixes this proxy is allowed to reach. `/public/...` is the
// existing, general-purpose authenticated-mutation scope (preferences,
// custom lines, auth). `/Train/...` was added for individual train
// tracking (`POST /Train/track`,
// docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
// Decision 4) -- that route is mounted directly on the backend's root
// router (`crates/api/src/main.rs`'s `.merge(routes::train::router())`),
// not nested under `/public`, the same way `/StopPoint/...`/`/Line/...`
// aren't. Each prefix maps to how the *backend* path is actually built:
// everything else still gets `/public/` prepended (unchanged from
// before this list existed); a `Train/...` request is passed straight
// through with no prefix inserted, since the backend already expects it
// bare.
function resolveTargetPath(path: string[]): string {
  return path[0] === 'Train' ? `/${path.join('/')}` : `/public/${path.join('/')}`;
}

async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  // Build the target as a `URL` and check the *resolved* pathname still
  // lives under one of the two allowed prefixes, rather than trying to
  // reject specific traversal patterns in the raw segments. Next.js
  // decodes catch-all segments before populating `path`, so a raw join
  // could otherwise let `..` (however it got there — literal, `%2e%2e`,
  // an embedded `%2F`, etc.) escape the intended scope and reach other
  // routes on the backend host. Checking the URL parser's actual
  // normalized output is strictly stronger than enumerating every
  // encoding trick that could produce a traversal — same check as
  // before this prefix list existed, just checked against either
  // allowed prefix instead of one.
  const target = new URL(`${process.env.API_BASE_URL}${resolveTargetPath(path)}${req.nextUrl.search}`);
  if (!target.pathname.startsWith('/public/') && !target.pathname.startsWith('/Train/')) {
    return new NextResponse('invalid path', { status: 400 });
  }
```

(The rest of `proxy()` — cookie forwarding, `redirect: 'manual'`, `Set-Cookie` passthrough — is unchanged; only the target-URL construction and the guard condition change.)

- [ ] **Step 3: Write the test file**

Create `frontend/app/api/[...path]/route.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';
import { GET, POST } from './route';

describe('/api/[...path] proxy', () => {
  beforeEach(() => {
    vi.stubEnv('API_BASE_URL', 'http://test-api:8080');
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ ok: true }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      ),
    );
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function makeRequest(pathname: string, init?: RequestInit): NextRequest {
    return new NextRequest(`http://localhost:3000${pathname}`, init);
  }

  it('still forwards an existing /public-scoped route unchanged (regression)', async () => {
    const req = makeRequest('/api/preferences');
    await GET(req, { params: Promise.resolve({ path: ['preferences'] }) });
    const [calledUrl] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/public/preferences');
  });

  it('forwards a Train/track POST to the bare-root backend path, with cookies attached', async () => {
    const req = makeRequest('/api/Train/track', {
      method: 'POST',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ origin_crs: 'WAT' }),
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', 'track'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/Train/track');
    expect((init as RequestInit).method).toBe('POST');
    expect((init as { headers: Record<string, string> }).headers.Cookie).toBe('nr_session=abc123');
  });

  it('a path outside both public/ and Train/ still 400s', async () => {
    // Not reachable through this app's own links today (every catch-all
    // segment this app generates comes from a literal string, never raw
    // user text) -- this is the traversal-safety net Decision 4 said
    // stays "unchanged in kind"; confirm it still rejects a resolved path
    // outside the widened two-prefix allowlist, not just the original
    // single-prefix one.
    const req = makeRequest('/api/../secret');
    const response = await GET(req, { params: Promise.resolve({ path: ['..', 'secret'] }) });
    expect(response.status).toBe(400);
  });
});
```

Note: `response.headers.getSetCookie()` (used elsewhere in `proxy()`, unchanged by this task) requires a fetch `Headers` implementation that supports it — Node's built-in `fetch`/`Headers` (which both `next dev`/`next build` and Vitest's default Node-derived globals use) supports this; if the test environment's global `Response`/`Headers` doesn't, adjust the mock `Response` construction accordingly, but this is pre-existing behavior of the file, not something this task's change introduces.

- [ ] **Step 4: Run the test suite**

Run (from `frontend/`): `npm test -- route.test.ts`
Expected: all three tests PASS.

- [ ] **Step 5: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, no regressions to existing `/api/*` consumers (`PinToggle`, the auth login/callback flow).

- [ ] **Step 6: Commit**

```bash
git add frontend/app/api/[...path]/route.ts frontend/app/api/[...path]/route.test.ts
git commit -m "Widen /api proxy allowlist to reach Train/* alongside public/*"
```

---

### Task 2: `lib/types.ts` and `lib/api.ts` additions

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: types `ResolutionStatus`, `JourneyStatus`, `EtaSource`, `TrackedTrainState`, `TrackPinRequest`, `TrackPinResponse`; functions `getTrackedTrainById(id: number): Promise<TrackedTrainState>`, `getTrackedTrainByUidAndDate(uid: string, date: string): Promise<TrackedTrainState>`.
- Consumed by: Task 4 (`TrainJourney` takes a `TrackedTrainState`), Task 5 (`TrackTrainForm` builds a `TrackPinRequest`, parses a `TrackPinResponse`), Task 8/9 (the two new pages call the two getters).

**Note on field shapes:** `TrackPinRequest`'s wire body is plain `snake_case` (confirmed directly against `crates/common/src/lib.rs`'s `TrackPinRequest` struct — no `#[serde(rename_all = ...)]` on it, unlike every camelCase public-JSON type below). Every other type here mirrors `crates/api`'s camelCase public JSON, confirmed directly against `crates/api/src/data/train_tracking.rs`'s `TrackedTrainState` (`#[serde(rename_all = "camelCase")]`) and its `TRACKED_TRAIN_STATE_SELECT` query, which selects exactly the 14 columns listed below — no more, no less. In particular, it does **not** select `pin_scheduled_departure`, only `service_date` (a date, no time-of-day) — Task 4's `pending`/`unresolved` rendering must not assume a scheduled clock time is available in this struct.

- [ ] **Step 1: Add the types**

Add to `frontend/lib/types.ts`, after `SessionInfo`:

```ts
export type ResolutionStatus = 'pending' | 'resolved' | 'unresolved';
export type JourneyStatus = 'awaiting_activation' | 'en_route' | 'cancelled' | 'completed';
export type EtaSource = 'trust-propagated' | 'darwin-estimated';

/** `GET /Train/{trackingId}` and `GET /Train/by-uid/{uid}/{date}`'s shared
 * response shape (`crates/api/src/data/train_tracking.rs`'s
 * `TrackedTrainState`, camelCase on the wire). `status` and every
 * movement field are `null` until `resolutionStatus` is `'resolved'` and
 * `trust-consumer` has written a `train_current_state` row. Note there is
 * no `scheduledDeparture` field -- the backend's read query does not
 * select `pin_scheduled_departure`, only `serviceDate` (a date). See
 * `components/TrainJourney.tsx` for the full per-state rendering rules. */
export interface TrackedTrainState {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  trainId: string | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
}

/** `POST /Train/track`'s request body (`common::TrackPinRequest`). Plain
 * snake_case on the wire -- unlike every other type in this file, which
 * mirrors `crates/api`'s camelCase public JSON, this one matches
 * `crates/common`'s internal-wire-type convention instead. Sent only from
 * `components/TrackTrainForm.tsx`, via the same-origin `/api/Train/track`
 * proxy (`app/api/[...path]/route.ts`). */
export interface TrackPinRequest {
  service_date: string; // "YYYY-MM-DD"
  origin_crs: string;
  scheduled_departure: string; // RFC3339
  destination_crs?: string;
  operator?: string;
}

/** `POST /Train/track`'s response body -- camelCase, like every other
 * `crates/api` public JSON response (only the request body above is
 * snake_case). `resolutionStatus` is always the literal `'pending'` --
 * a newly-created pin has no `train_uid` bound yet. */
export interface TrackPinResponse {
  trackingId: number;
  resolutionStatus: 'pending';
}
```

- [ ] **Step 2: Add the two read functions**

In `frontend/lib/api.ts`, add `TrackedTrainState` to the existing `import type { ... } from './types';` list, then add after `getDataFreshness`:

```ts
export async function getTrackedTrainById(id: number): Promise<TrackedTrainState> {
  return fetchJson<TrackedTrainState>(`${baseUrl()}/Train/${id}`, { cache: 'no-store' });
}

export async function getTrackedTrainByUidAndDate(uid: string, date: string): Promise<TrackedTrainState> {
  return fetchJson<TrackedTrainState>(`${baseUrl()}/Train/by-uid/${uid}/${date}`, { cache: 'no-store' });
}
```

- [ ] **Step 3: Add tests**

In `frontend/lib/api.test.ts`, add `getTrackedTrainById, getTrackedTrainByUidAndDate` to the existing import list from `./api`, then add:

```ts
it('getTrackedTrainById fetches the correct URL with no caching', async () => {
  await getTrackedTrainById(42);
  expect(fetch).toHaveBeenCalledWith(
    'http://test-api:8080/Train/42',
    expect.objectContaining({ cache: 'no-store' }),
  );
});

it('getTrackedTrainByUidAndDate fetches the correct URL with no caching', async () => {
  await getTrackedTrainByUidAndDate('C21373', '2026-08-28');
  expect(fetch).toHaveBeenCalledWith(
    'http://test-api:8080/Train/by-uid/C21373/2026-08-28',
    expect.objectContaining({ cache: 'no-store' }),
  );
});

it('getTrackedTrainById throws ApiNotFoundError on a 404', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
  await expect(getTrackedTrainById(999)).rejects.toBeInstanceOf(ApiNotFoundError);
});
```

- [ ] **Step 4: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: all tests, including the three new ones, PASS.

- [ ] **Step 5: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add TrackedTrainState/TrackPinRequest types and tracked-train read functions"
```

---

### Task 3: `EtaBadge` component

**Files:**
- Create: `frontend/components/EtaBadge.tsx`
- Create: `frontend/components/EtaBadge.test.tsx`

**Interfaces:**
- Produces: `EtaBadge({ etaNext, etaSource }: { etaNext: string | null; etaSource: EtaSource | null })` — renders nothing if `etaNext` is null; otherwise renders the formatted time plus a visibly distinct badge per `etaSource`, per Decision 3 ("a visibly distinct treatment for `etaSource`... badge/tooltip, not collapsed into one number, mirroring `StatusBadge`'s existing severity-badge pattern").
- Consumed by: Task 4 (`TrainJourney`'s `en_route`/`cancelled`/`completed` branches).

- [ ] **Step 1: Write the component**

Create `frontend/components/EtaBadge.tsx`:

```tsx
import { Badge, Group, Text, Tooltip } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { EtaSource } from '@/lib/types';

/** Renders nothing when there's no ETA at all (`etaNext` null) -- a
 * tracked train that hasn't been resolved yet, or has no current-state
 * row, has nothing to show here. When there IS an ETA, `etaSource` is
 * always shown as a distinct badge alongside the time, never collapsed
 * into one number -- extending this app's existing `dataQuality`
 * provenance-surfacing philosophy (`StatusBadge`/`LineStatus.dataQuality`)
 * to ETAs, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3. */
export function EtaBadge({ etaNext, etaSource }: { etaNext: string | null; etaSource: EtaSource | null }) {
  if (!etaNext || !etaSource) return null;

  const label = etaSource === 'darwin-estimated' ? 'Live departure board' : 'Network Rail propagated';
  const tooltip =
    etaSource === 'darwin-estimated'
      ? 'Estimated from a live Darwin/National Rail Enquiries departure board sample at the origin station'
      : "Estimated by Network Rail's TRUST movement feed, propagated forward from the train's last reported delay";

  return (
    <Group gap={6} wrap="nowrap">
      <Text size="sm">ETA {formatTime(etaNext)}</Text>
      <Tooltip label={tooltip}>
        <Badge color={etaSource === 'darwin-estimated' ? 'teal' : 'gray'} variant="light">
          {label}
        </Badge>
      </Tooltip>
    </Group>
  );
}
```

- [ ] **Step 2: Write the tests**

Create `frontend/components/EtaBadge.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EtaBadge } from './EtaBadge';

describe('EtaBadge', () => {
  it('renders nothing when there is no ETA', () => {
    const { container } = renderWithMantine(<EtaBadge etaNext={null} etaSource={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('renders nothing if etaSource is somehow missing despite an etaNext value', () => {
    const { container } = renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows a distinct badge for a darwin-estimated ETA', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="darwin-estimated" />);
    expect(screen.getByText('Live departure board')).toBeInTheDocument();
  });

  it('shows a distinct badge for a trust-propagated ETA', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    expect(screen.getByText('Network Rail propagated')).toBeInTheDocument();
  });

  it('the two sources render visibly different badge text', () => {
    const { unmount } = renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="darwin-estimated" />);
    const darwinText = screen.getByText('Live departure board').textContent;
    unmount();
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    const trustText = screen.getByText('Network Rail propagated').textContent;
    expect(darwinText).not.toBe(trustText);
  });
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- EtaBadge.test.tsx`
Expected: all five tests PASS.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/EtaBadge.tsx frontend/components/EtaBadge.test.tsx
git commit -m "Add EtaBadge component for trust-propagated vs darwin-estimated ETAs"
```

---

### Task 4: `TrainJourney` component — shared state-branch renderer

**Files:**
- Create: `frontend/components/TrainJourney.tsx`
- Create: `frontend/components/TrainJourney.test.tsx`

**Interfaces:**
- Consumes: `TrackedTrainState` (Task 2), `EtaBadge` (Task 3).
- Produces: `TrainJourney({ state }: { state: TrackedTrainState })` — renders every `resolutionStatus`/`status` combination from Decision 3's table.
- Consumed by: Task 8 (`/train/by-id/[trackingId]`) and Task 9 (`/train/[uid]/[date]`) — the design's whole point in factoring this out is that both pages differ only in which `lib/api.ts` fetch found the data, never in how it's shown.

**Deviation from the spec's prose, resolved against the actual struct:** Decision 3's table describes the `pending`/`unresolved` panels as showing "origin/destination/scheduled time as pinned." `TrackedTrainState` (Task 2, confirmed against `crates/api/src/data/train_tracking.rs`'s live `TRACKED_TRAIN_STATE_SELECT`) has no scheduled-departure-clock-time field — only `serviceDate` (a date). This task renders origin, destination, and service date; it does not fabricate a clock time the backend doesn't return.

- [ ] **Step 1: Write the component**

Create `frontend/components/TrainJourney.tsx`:

```tsx
import { Alert, Badge, Group, Loader, Stack, Text } from '@mantine/core';
import { EtaBadge } from './EtaBadge';
import { formatDate } from '@/lib/dateFormat';
import type { TrackedTrainState } from '@/lib/types';

/** Renders one `TrackedTrainState` through every state the backend can
 * return, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3's table. Shared by both `/train/by-id/[trackingId]` and
 * `/train/[uid]/[date]`.
 *
 * Note on the pin summary shown for `pending`/`unresolved`: this renders
 * origin, destination, and service *date* only -- `TrackedTrainState` has
 * no scheduled-departure clock-time field (see this component's own
 * module in the implementation plan for why), so this does not claim to
 * show a scheduled time the backend doesn't return. */
export function TrainJourney({ state }: { state: TrackedTrainState }) {
  const pinSummary = (
    <Text size="sm" c="dimmed">
      {state.pinOriginCrs}
      {state.pinDestinationCrs ? ` → ${state.pinDestinationCrs}` : ''} · {formatDate(state.serviceDate)}
    </Text>
  );

  if (state.resolutionStatus === 'pending') {
    return (
      <Stack gap="sm" role="status">
        <Group gap="sm">
          <Loader size="sm" />
          <Text fw={500}>Waiting to hear from Network Rail</Text>
        </Group>
        {pinSummary}
        <Text size="sm" c="dimmed">
          This train hasn&apos;t been matched to a live service yet. This page updates automatically.
        </Text>
      </Stack>
    );
  }

  if (state.resolutionStatus === 'unresolved') {
    return (
      <Stack gap="sm">
        <Text fw={500} c="red">
          Couldn&apos;t be matched to a live service
        </Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Network Rail never reported a matching service for this pin. This won&apos;t resolve on its own
          — try tracking the train again if it was a genuine mistake.
        </Text>
      </Stack>
    );
  }

  // resolutionStatus === 'resolved' from here on -- trainUid is non-null
  // per the backend's own resolution invariant (a tracked train is only
  // ever set to 'resolved' in the same write that sets train_uid), even
  // though the TypeScript type can't express that correlation across two
  // separate optional fields.
  if (state.status === 'awaiting_activation' || state.status === null) {
    return (
      <Stack gap="sm">
        <Text fw={500}>Matched to train {state.trainUid}</Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Waiting for its first movement report.
        </Text>
      </Stack>
    );
  }

  if (state.status === 'cancelled') {
    return (
      <Stack gap="sm">
        <Alert color="red" title="Cancelled">
          This service was cancelled.
        </Alert>
        <Text fw={500}>Train {state.trainUid}</Text>
        {pinSummary}
        <JourneyDetails state={state} />
      </Stack>
    );
  }

  // 'en_route' or 'completed' share the same "current position" rendering
  // -- 'completed' is kept as a real branch even though no current
  // trust-consumer code path produces it yet (see this plan's Global
  // Constraints and Status note), so it's forward-compatible rather than
  // dead code the day journey.rs gets real completion detection.
  const mayHaveFinished =
    state.status === 'completed' || (state.status === 'en_route' && state.nextCallingPoint === null);

  return (
    <Stack gap="sm">
      <Text fw={500}>Train {state.trainUid}</Text>
      {pinSummary}
      {mayHaveFinished && (
        <Alert color="yellow" title="May have finished" variant="light">
          {/* Provisional heuristic, not a confirmed backend status -- see
              this plan's Global Constraints and
              docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
              Open Question 2. Deliberately worded as an inference
              ("may have"), never asserted as fact. */}
          No further calling points have been reported. This journey may have finished, but this is an
          inference, not a confirmed status from Network Rail.
        </Alert>
      )}
      <JourneyDetails state={state} />
    </Stack>
  );
}

function JourneyDetails({ state }: { state: TrackedTrainState }) {
  return (
    <Stack gap={4}>
      {state.lastReportedLocation && (
        <Text size="sm">
          Last reported: {state.lastReportedLocation}
          {state.lastEventType ? ` (${state.lastEventType.toLowerCase()})` : ''}
        </Text>
      )}
      {state.delayMinutes !== null && (
        <Group gap={6}>
          <Text size="sm">Delay:</Text>
          <Badge color={state.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
            {state.delayMinutes > 0 ? `${state.delayMinutes}m late` : 'On time'}
          </Badge>
        </Group>
      )}
      {state.nextCallingPoint && <Text size="sm">Next calling point: {state.nextCallingPoint}</Text>}
      <EtaBadge etaNext={state.etaNext} etaSource={state.etaSource} />
    </Stack>
  );
}
```

- [ ] **Step 2: Write the tests**

Create `frontend/components/TrainJourney.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainJourney } from './TrainJourney';
import type { TrackedTrainState } from '@/lib/types';

function baseState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-08-28',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    resolutionStatus: 'pending',
    trainUid: null,
    trainId: null,
    status: null,
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    ...overrides,
  };
}

describe('TrainJourney', () => {
  it('pending: shows a waiting panel with the pinned origin/destination/date', () => {
    renderWithMantine(<TrainJourney state={baseState()} />);
    expect(screen.getByText('Waiting to hear from Network Rail')).toBeInTheDocument();
    expect(screen.getByText(/WAT/)).toBeInTheDocument();
    expect(screen.getByText(/WOK/)).toBeInTheDocument();
  });

  it('unresolved: shows a terminal, non-retrying message', () => {
    renderWithMantine(<TrainJourney state={baseState({ resolutionStatus: 'unresolved' })} />);
    expect(screen.getByText("Couldn't be matched to a live service")).toBeInTheDocument();
    expect(screen.getByText(/won't resolve on its own/)).toBeInTheDocument();
  });

  it('resolved + awaiting_activation: names the matched train, no movement data', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({ resolutionStatus: 'resolved', trainUid: 'C21373', status: 'awaiting_activation' })}
      />,
    );
    expect(screen.getByText('Matched to train C21373')).toBeInTheDocument();
    expect(screen.getByText('Waiting for its first movement report.')).toBeInTheDocument();
  });

  it('resolved + en_route: shows location, delay, next calling point, and ETA', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'en_route',
          lastReportedLocation: 'Clapham Junction',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'Woking',
          etaNext: '2026-08-28T18:41:00Z',
          etaSource: 'trust-propagated',
        })}
      />,
    );
    expect(screen.getByText(/Clapham Junction/)).toBeInTheDocument();
    expect(screen.getByText('4m late')).toBeInTheDocument();
    expect(screen.getByText('Next calling point: Woking')).toBeInTheDocument();
    expect(screen.getByText(/ETA/)).toBeInTheDocument();
    expect(screen.queryByText('May have finished')).not.toBeInTheDocument();
  });

  it('resolved + en_route with no next calling point: shows the provisional "may have finished" note', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          nextCallingPoint: null,
        })}
      />,
    );
    expect(screen.getByText('May have finished')).toBeInTheDocument();
    expect(screen.getByText(/this is an inference, not a confirmed status/)).toBeInTheDocument();
  });

  it('resolved + cancelled: shows a cancelled banner and retains last known location', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'cancelled',
          lastReportedLocation: 'Surbiton',
        })}
      />,
    );
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
    expect(screen.getByText(/Surbiton/)).toBeInTheDocument();
  });

  it('resolved + completed: shows the same arrived treatment as the no-next-stop en_route case', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'completed',
          lastReportedLocation: 'Woking',
        })}
      />,
    );
    expect(screen.getByText('May have finished')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- TrainJourney.test.tsx`
Expected: all seven tests PASS.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrainJourney.tsx frontend/components/TrainJourney.test.tsx
git commit -m "Add TrainJourney shared state-branch renderer"
```

---

### Task 5: `TrackTrainForm` component

**Files:**
- Create: `frontend/components/TrackTrainForm.tsx`
- Create: `frontend/components/TrackTrainForm.test.tsx`

**Interfaces:**
- Consumes: `TrackPinRequest`, `TrackPinResponse` (Task 2), the widened proxy (Task 1, functionally — at end-to-end runtime, not at compile time).
- Produces: `TrackTrainForm({ initialOrigin }: { initialOrigin?: string })`. On a `200` response, calls `router.push('/train/by-id/{trackingId}')`.
- Consumed by: Task 6 (`/track` page).

Mirrors `PinToggle.tsx`'s `needsLogin` 401 pattern (see `frontend/components/PinToggle.tsx` and its test file for the precedent), with the one deliberate difference called out in this plan's Global Constraints: a 401 does not reset any field.

- [ ] **Step 1: Write the component**

Create `frontend/components/TrackTrainForm.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, TextInput } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import { TextLink } from './TextLink';
import type { TrackPinRequest, TrackPinResponse } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;

/** The v1 entry point for individual train tracking -- a manual form, not
 * a per-departure "track this train" action, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 1 (no public API exposes individual departures today, so a
 * departure-row action can't be built). `initialOrigin` is set by
 * `/track`'s page when arriving via the "Track a train from here" link on
 * `/stations/[crs]` (Decision 1's honest station-page shortcut).
 *
 * Submits through the same-origin `/api/Train/track` proxy (Client
 * Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`). Mirrors
 * `PinToggle`'s `needsLogin` 401 pattern, with one deliberate difference:
 * a 401 here does NOT reset the form. `PinToggle` can afford to forget its
 * click (there was no typed input to lose); a four-field form has real
 * input worth protecting, so all four fields stay exactly as typed while
 * the login prompt renders alongside them (Decision 4, "no navigation
 * away"). */
export function TrackTrainForm({ initialOrigin = '' }: { initialOrigin?: string }) {
  const router = useRouter();
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState('');
  const [operator, setOperator] = useState('');
  const [scheduledDeparture, setScheduledDeparture] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [needsLogin, setNeedsLogin] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);

  const originValid = CRS_PATTERN.test(originCrs.trim());
  const canSubmit = originValid && scheduledDeparture !== null && !submitting;

  async function handleSubmit() {
    if (!canSubmit || scheduledDeparture === null) return;
    setSubmitting(true);
    setNeedsLogin(false);
    setFieldError(null);
    try {
      const departure = new Date(scheduledDeparture);
      const body: TrackPinRequest = {
        service_date: departure.toISOString().slice(0, 10),
        origin_crs: originCrs.trim().toUpperCase(),
        scheduled_departure: departure.toISOString(),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
        ...(operator.trim() ? { operator: operator.trim() } : {}),
      };

      const response = await fetch('/api/Train/track', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result: TrackPinResponse = await response.json();
        router.push(`/train/by-id/${result.trackingId}`);
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setFieldError(await response.text());
        return;
      }
      setFieldError("Couldn't create the tracking pin. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="md">
      <TextInput
        label="Origin CRS code"
        placeholder="e.g. WAT"
        value={originCrs}
        onChange={(event) => setOriginCrs(event.currentTarget.value)}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <DateTimePicker
        label="Scheduled departure"
        placeholder="Pick date and time"
        value={scheduledDeparture}
        onChange={setScheduledDeparture}
        // The backend rejects a departure more than 6 hours in the past
        // (`crates/api/src/data/train_tracking.rs`'s `MAX_PIN_AGE`) --
        // this hint is here so a rejection is rare rather than the
        // user's first encounter with the rule, per Decision 1.
        description="Must be within the last 6 hours, or any time in the future"
        required
      />
      <TextInput
        label="Destination CRS code (optional)"
        placeholder="e.g. WOK"
        value={destinationCrs}
        onChange={(event) => setDestinationCrs(event.currentTarget.value)}
      />
      <TextInput
        label="Operator (optional)"
        placeholder="e.g. SW"
        value={operator}
        onChange={(event) => setOperator(event.currentTarget.value)}
      />
      {fieldError && (
        <Alert color="red" title="Couldn't track this train">
          {fieldError}
        </Alert>
      )}
      <Group>
        <Button onClick={handleSubmit} disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
        {needsLogin && (
          <TextLink href="/api/auth/login" underline="always">
            Log in to track this train
          </TextLink>
        )}
      </Group>
    </Stack>
  );
}
```

**Implementation-time verification note:** `value`/`onChange` above assume `DateTimePicker` (from `@mantine/dates` v9) takes/emits a `string | null`, matching the convention `HistoryRangePicker.tsx` already establishes for `DatePickerInput` in this codebase. Confirm this against the installed `@mantine/dates` version while implementing this step; adjust the state type if it differs.

- [ ] **Step 2: Write the tests**

Create `frontend/components/TrackTrainForm.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackTrainForm } from './TrackTrainForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
}));

describe('TrackTrainForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('pre-fills the origin field from initialOrigin', () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    expect(screen.getByLabelText(/Origin CRS code/)).toHaveValue('WAT');
  });

  it('disables submit until the origin is a valid 3-letter code and a departure is picked', () => {
    renderWithMantine(<TrackTrainForm />);
    expect(screen.getByRole('button', { name: /Track this train/ })).toBeDisabled();
  });

  it('shows a field error for a non-3-letter origin code', () => {
    renderWithMantine(<TrackTrainForm />);
    fireEvent.change(screen.getByLabelText(/Origin CRS code/), { target: { value: 'WATERLOO' } });
    expect(screen.getByText('Must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it('on success, POSTs to /api/Train/track and redirects to /train/by-id/{trackingId}', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ trackingId: 42, resolutionStatus: 'pending' }), { status: 200 }),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), { target: { value: '2026-08-28T18:32' } });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/track', expect.objectContaining({ method: 'POST' }));
    });
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/train/by-id/42');
    });
  });

  it('on a 401, shows the login prompt and preserves the typed field values', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Destination CRS code/), { target: { value: 'WOK' } });
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), { target: { value: '2026-08-28T18:32' } });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to track this train' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login');
    // Unlike PinToggle's toggle-and-forget click, the form's own input
    // must survive a 401 -- Decision 4's explicit "preserve typed values"
    // call.
    expect(screen.getByLabelText(/Origin CRS code/)).toHaveValue('WAT');
    expect(screen.getByLabelText(/Destination CRS code/)).toHaveValue('WOK');
  });

  it('on a 400, shows the server error message inline', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response('scheduled_departure is too far in the past to track', { status: 400 }),
    );

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), { target: { value: '2026-08-28T18:32' } });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText('scheduled_departure is too far in the past to track')).toBeInTheDocument();
  });

  it('on a 500, shows a generic error message', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('internal error', { status: 500 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), { target: { value: '2026-08-28T18:32' } });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText("Couldn't create the tracking pin. Try again.")).toBeInTheDocument();
  });
});
```

**Implementation-time verification note:** driving `DateTimePicker` via `fireEvent.change` on its labelled input assumes direct text entry works the way it does for a plain, non-`readOnly` Mantine dates input. This repo's one existing dates-component test (`HistoryRangePicker.test.tsx`) never actually exercises typing into a picker — it only asserts display values and button state — so there is no direct precedent to copy here. Confirm this interaction fires `onChange` as expected while implementing this step; if it doesn't, use Testing Library's guidance for custom form controls instead (e.g. querying and clicking the calendar's rendered day/time controls, or asserting via a thin test-only wrapper around the component's exposed props).

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- TrackTrainForm.test.tsx`
Expected: all seven tests PASS. If the `DateTimePicker` interaction note above required a different approach, all tests still PASS with the adjusted interaction.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "Add TrackTrainForm with PinToggle-style needsLogin 401 handling"
```

---

### Task 6: `/track` page

**Files:**
- Create: `frontend/app/track/page.tsx`

**Interfaces:**
- Consumes: `TrackTrainForm` (Task 5).
- Produces: default-exported `TrackPage`, reading `searchParams.origin` and passing it through as `initialOrigin`.
- Consumed by: Task 7 (the station-page shortcut links to `/track?origin={crs}`), Task 10 (the nav link points here).

- [ ] **Step 1: Write the page**

Create `frontend/app/track/page.tsx`:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { TrackTrainForm } from '@/components/TrackTrainForm';

export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{ origin?: string }>;
}) {
  const { origin } = await searchParams;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Train</Title>
      <Text c="dimmed">
        Pin a specific train to see its live position, delay and next calling point as Network Rail
        reports it.
      </Text>
      <TrackTrainForm initialOrigin={origin?.toUpperCase()} />
    </Stack>
  );
}
```

No colocated test file for this page — matching this codebase's existing convention: `app/stations/page.tsx` (the closest precedent, a heading + a form component) has no `page.test.tsx` of its own either; the form component it wraps (`StationSearchForm.test.tsx`) carries the test coverage, exactly as `TrackTrainForm.test.tsx` (Task 5) does here.

- [ ] **Step 2: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. `next build` will surface any type error in the `searchParams` shape or the `TrackTrainForm` prop wiring.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/track/page.tsx
git commit -m "Add /track page"
```

---

### Task 7: "Track a train from here" shortcut on `/stations/[crs]`

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`

**Interfaces:**
- Produces: a `TextLink` reading "Track a train from here", linking to `/track?origin={crs}`, next to the existing `PinToggle` in the page's header `Group`.

Per Decision 1: "the closest honest equivalent to the backend design doc's sketch given what's actually rendered there today: it's a shortcut into the manual form, pre-scoped to the station the user was already looking at, not a per-departure action."

- [ ] **Step 1: Modify the header `Group`**

In `frontend/app/stations/[crs]/page.tsx`, `TextLink` is already imported. Replace:

```tsx
      <Group justify="space-between">
        <Title order={1}>Disruptions at {heading}</Title>
        <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
      </Group>
```

with:

```tsx
      <Group justify="space-between">
        <Title order={1}>Disruptions at {heading}</Title>
        <Group gap="md">
          <TextLink href={`/track?origin=${crs}`}>Track a train from here</TextLink>
          <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
        </Group>
      </Group>
```

- [ ] **Step 2: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS — no existing test asserts the exact contents of this page's header `Group` (confirmed: no `page.test.tsx` exists for `app/stations/[crs]/`), so this is additive with no expected regressions.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/stations/[crs]/page.tsx
git commit -m "Add 'Track a train from here' shortcut to the station disruption page"
```

---

### Task 8: `/train/by-id/[trackingId]` page

**Files:**
- Create: `frontend/app/train/by-id/[trackingId]/page.tsx`
- Create: `frontend/app/train/by-id/[trackingId]/not-found.tsx`

**Interfaces:**
- Consumes: `getTrackedTrainById` (Task 2), `TrainJourney` (Task 4), `ApiNotFoundError` (existing, `lib/api.ts`).
- Produces: the post-creation landing page (Decision 1's redirect target), including the same-page canonical-link nudge from Decision 2.

Per Decision 2, this is a same-page `TextLink` nudge, **not** an automatic redirect: "an automatic redirect would silently break 'I bookmarked the URL right after tracking, before it resolved' for a user who didn't want to wait." Do not implement this as a redirect.

- [ ] **Step 1: Write the not-found page**

Create `frontend/app/train/by-id/[trackingId]/not-found.tsx`:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function TrackedTrainByIdNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Tracked train not found</Title>
      <Text c="dimmed">No tracking pin matches that id.</Text>
      <TextLink href="/track" underline="always">
        Track a train
      </TextLink>
    </Stack>
  );
}
```

- [ ] **Step 2: Write the page**

Create `frontend/app/train/by-id/[trackingId]/page.tsx`:

```tsx
import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getTrackedTrainById, ApiNotFoundError } from '@/lib/api';
import { TrainJourney } from '@/components/TrainJourney';
import { TextLink } from '@/components/TextLink';

export default async function TrackedTrainByIdPage({
  params,
}: {
  params: Promise<{ trackingId: string }>;
}) {
  const { trackingId } = await params;

  // Validated before the fetch fires, per
  // docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
  // Error handling section -- a malformed segment 404s directly rather
  // than reaching the backend and relying on its error shape.
  if (!/^\d+$/.test(trackingId)) {
    notFound();
  }

  let state;
  try {
    state = await getTrackedTrainById(Number(trackingId));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Tracking Train {trackingId}</Title>
      <TrainJourney state={state} />
      {/* A same-page nudge, not an automatic redirect -- Decision 2's
          explicit reasoning: a redirect would silently break "I
          bookmarked the URL right after tracking, before it resolved"
          for a user who didn't want to wait. */}
      {state.resolutionStatus === 'resolved' && state.trainUid && (
        <TextLink href={`/train/${state.trainUid}/${state.serviceDate}`} underline="always">
          View the canonical link for this train
        </TextLink>
      )}
    </Stack>
  );
}
```

No colocated test file — matching this codebase's existing convention for data-fetching page files (`app/lines/[id]/page.tsx`, `app/stations/[crs]/page.tsx` have none either; their sub-components carry the test coverage, which `TrainJourney.test.tsx` (Task 4) already does for the state-rendering logic this page delegates to).

- [ ] **Step 3: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add "frontend/app/train/by-id/[trackingId]/page.tsx" "frontend/app/train/by-id/[trackingId]/not-found.tsx"
git commit -m "Add /train/by-id/[trackingId] page with canonical-link nudge"
```

---

### Task 9: `/train/[uid]/[date]` page

**Files:**
- Create: `frontend/app/train/[uid]/[date]/page.tsx`
- Create: `frontend/app/train/[uid]/[date]/not-found.tsx`

**Interfaces:**
- Consumes: `getTrackedTrainByUidAndDate` (Task 2), `TrainJourney` (Task 4), `ApiNotFoundError` (existing).
- Produces: the canonical, shareable/bookmarkable URL once a pin has resolved, per Decision 2.

- [ ] **Step 1: Write the not-found page**

Create `frontend/app/train/[uid]/[date]/not-found.tsx`:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function TrackedTrainByUidNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Tracked train not found</Title>
      <Text c="dimmed">No resolved tracked train matches that train and date.</Text>
      <TextLink href="/track" underline="always">
        Track a train
      </TextLink>
    </Stack>
  );
}
```

- [ ] **Step 2: Write the page**

Create `frontend/app/train/[uid]/[date]/page.tsx`:

```tsx
import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getTrackedTrainByUidAndDate, ApiNotFoundError } from '@/lib/api';
import { TrainJourney } from '@/components/TrainJourney';

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export default async function TrackedTrainByUidPage({
  params,
}: {
  params: Promise<{ uid: string; date: string }>;
}) {
  const { uid, date } = await params;

  // Validated before the fetch fires, per the same "malformed URL segment
  // 404s directly" rule as the by-id page (Task 8) --
  // docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
  // Error handling section.
  if (!DATE_PATTERN.test(date)) {
    notFound();
  }

  let state;
  try {
    state = await getTrackedTrainByUidAndDate(uid, date);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Train {uid}</Title>
      <TrainJourney state={state} />
    </Stack>
  );
}
```

No colocated test file, for the same reason given in Task 8.

- [ ] **Step 3: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add "frontend/app/train/[uid]/[date]/page.tsx" "frontend/app/train/[uid]/[date]/not-found.tsx"
git commit -m "Add canonical /train/[uid]/[date] page"
```

---

### Task 10: Nav link, and confirming the `OpenDataAttribution` non-goal

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Produces: a `TextLink` to `/track` in the root nav, next to the existing "Station Lookup" link, per Decision 1 ("A new top-level nav link, 'Track a Train'... `layout.tsx:99`").

- [ ] **Step 1: Add the nav link**

In `frontend/app/layout.tsx`, replace:

```tsx
                  <TextLink href="/stations">Station Lookup</TextLink>
```

with:

```tsx
                  <TextLink href="/stations">Station Lookup</TextLink>
                  <TextLink href="/track">Track a Train</TextLink>
```

- [ ] **Step 2: Confirm the `OpenDataAttribution` non-goal — verification only, no code change**

Open `frontend/components/OpenDataAttribution.tsx` and confirm its third `<Text>` block still reads "Live train movement data from Network Rail's open data feeds" (unbranded, no logo, no "official" framing), and that it is rendered once by `app/layout.tsx`'s existing `<OpenDataAttribution />` call (already present — not something this task adds). **Do not edit this file.** This plan adds no new attribution copy anywhere in the train-tracking feature; this step exists only to confirm the spec's claim ("this spec adds no attribution work") still holds against the current tree before this feature's frontend ships, rather than assuming it from the spec's prose alone.

- [ ] **Step 3: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. Check that no existing test asserts the exact set/order of nav `TextLink`s in a way this addition would break (none does, based on this plan's research — `app/layout.tsx` has no colocated test file).

- [ ] **Step 4: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Add 'Track a Train' nav link"
```

---

## Explicitly out of scope for this plan (carried forward from the spec, not resolved here)

Per the spec's own "Explicitly out of scope" and "Open questions / risks" sections — none of the following is invented or silently decided by any task above:

- A real per-departure "track this train" action (blocked on a new public `GET .../Departures`-shaped backend endpoint that doesn't exist today).
- A full stop-by-stop journey timeline (blocked on a public read endpoint over `train_movement_events`, which is write-only today).
- A per-route `AutoRefresh` opt-out or interval, for the `cancelled`/finished-journey staleness case.
- Persisting or auto-resubmitting form state across the OIDC login redirect round-trip.
- A "my tracked trains" list page.
- Real-world time-to-resolution for a `pending` pin (how soon Activation typically arrives) — unresolved by this codebase alone; Task 4's `pending` copy deliberately makes no claim about timing.
- Whether historical/finished journeys should remain viewable, and for how long (retention policy) — Task 8/9's `not-found.tsx` pages already handle a pruned row gracefully, but the UX of "your old link stopped working" is not otherwise addressed.
- Darwin ETA blend behavior under real production load (how often `darwin-estimated` actually appears vs. falling back to `trust-propagated`) — Task 3's `EtaBadge` renders whichever value the backend sends, without assuming a particular mix.
- Validating the `/track` form's origin CRS against the real station reference set (type-ahead) rather than a 3-letter regex — Task 5 uses the same regex-only validation `/stations/[crs]/page.tsx` already uses (`CRS_PATTERN`), matching the spec's own explicit deferral of this choice.

## Self-review notes

- **Spec coverage:** entry points (Tasks 5–7, 10), URL shapes (Tasks 8–9), per-state rendering table (Task 4), auth UX (Tasks 1, 5), refresh (no new task — Global Constraints states the existing global `AutoRefresh` covers it, per Decision 5), API/type contract (Task 2), testing convention (every task), attribution non-goal (Task 10) are each covered by exactly one task above.
- **Deviation from the spec worth flagging explicitly (per this plan's brief):** Decision 3's table describes the `pending`/`unresolved` panel as showing "scheduled time as pinned." Reading the live `TrackedTrainState` struct and its backing SQL (`crates/api/src/data/train_tracking.rs`'s `TRACKED_TRAIN_STATE_SELECT`) shows this field is never actually selected — the response has `serviceDate` (a date) but no scheduled clock time. Task 4 renders what the API actually returns rather than a value the wire type doesn't carry; documented inline in both Task 2 and Task 4 rather than silently reconciled.
- **Type consistency check:** `TrackedTrainState`/`TrackPinRequest`/`TrackPinResponse` (defined Task 2) are used with identical field names in Task 4 (`TrainJourney`), Task 5 (`TrackTrainForm`), Task 8/9 (the two pages). `getTrackedTrainById`/`getTrackedTrainByUidAndDate` (Task 2) are called with matching signatures in Task 8/9. `EtaBadge` (Task 3)'s props (`etaNext`, `etaSource`) match how Task 4 calls it.
