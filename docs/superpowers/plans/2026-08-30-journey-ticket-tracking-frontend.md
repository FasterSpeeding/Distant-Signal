# Journey Ticket Tracking Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give journey ticket tracking — `.pkpass`/PDF ticket ingestion, manual ticket entry, and a Delay Repay compensation estimate — a user-facing surface. Today the backend is fully implemented and merged (`crates/api/src/routes/train.rs`'s five `/Train/{trackingId}/tickets...` routes, `crates/api/src/data/delay_repay_rules.rs`, `crates/api/src/data/ticket_extraction.rs`) but there is **zero frontend code anywhere referencing tickets, pkpass, or delay-repay** — confirmed by grepping `frontend/` for `ticket`/`pkpass`/`delay-repay`/`multipart`/`FormData` and finding no matches outside spec/plan docs. This plan closes that gap: a session-gated ticket panel attached to the two existing tracked-train pages, a manual-entry-plus-upload form, and an honest, never-overstating Delay Repay estimate display.

**Architecture:** Pure frontend work in `frontend/` (Next.js App Router), plus one fix to shared infrastructure the new upload routes need: `frontend/app/api/[...path]/route.ts` (the same-origin proxy) currently corrupts any `multipart/form-data` body it forwards — confirmed by direct re-read on 2026-08-30, still present exactly as the spec describes:
- Line 60 hardcodes `const headers: Record<string, string> = { 'Content-Type': 'application/json' };` on every forwarded request, discarding the `multipart/form-data; boundary=...` header a browser's `fetch(url, { body: formData })` sets — axum's `Multipart` extractor needs that exact boundary to parse a file field at all.
- Line 79 does `init.body = await req.text();` for every non-GET/DELETE request, which lossily UTF-8-decodes the body before re-sending it — a `.pkpass` (zip) or PDF's raw bytes are binary and not valid UTF-8 in general, so this corrupts any file upload in transit today, independent of the header problem above.

Task 1 below fixes both, with regression coverage for every *existing* JSON caller (`PinToggle`, `TrackTrainForm`, preferences, the OIDC flow) alongside new coverage for the binary/multipart case — per the design spec's own Open Question 4, this touches shared infrastructure every existing mutation already depends on, not just this feature's new routes.

Every other route this feature needs (`GET .../tickets`, `GET .../tickets/{id}/delay-repay`, `POST .../tickets`, `POST .../tickets/pkpass`, `POST .../tickets/pdf`) is already reachable path-wise through the proxy's existing `/Train/...` allowlist (widened once already by the train-tracking-frontend plan) — no further prefix widening is needed. No task in this plan modifies anything under `crates/`.

**Tech Stack:** Next.js App Router + TypeScript + Mantine v9 (`@mantine/core`, including `FileInput` and `Tabs`, neither used elsewhere in this frontend today), Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`).

**Spec:** `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Correction N" refer to that document. The backend this frontend consumes was itself planned in `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md` and `docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md` (both already implemented and merged — not restated here either).

**Status note:** every backend prerequisite this plan depends on is already live, not merely planned — confirmed by direct inspection while writing this plan. `crates/api/src/routes/train.rs` mounts all five ticket routes on the root router under `/Train/...` (same as the existing train-tracking routes), `crates/api/src/data/delay_repay_rules.rs` and `crates/api/src/data/ticket_extraction.rs` both exist with the exact response shapes the design spec documents. `frontend/lib/types.ts` already has `TrackedTrainState`/`TrackPinRequest`/`TrackPinResponse` and `frontend/lib/api.ts` already has `getTrackedTrainById`/`getTrackedTrainByUidAndDate` (from the train-tracking-frontend plan) — this plan adds alongside them, not instead of them. `frontend/components/PinToggle.tsx` and `frontend/components/TrackTrainForm.tsx` are the established `needsLogin`/401 patterns this plan's own form reuses. `frontend/components/TextLink.tsx` is the app's single link component and needs one small, backward-compatible extension (Task 4) to support the one external, new-tab link this feature introduces.

## A gap in the spec's own hand-written API contract, and how this plan resolves it

The design spec's API/type contract section specifies `getTicketsForTrackedTrain(trackingId): Promise<TrackedTrainTicket[] | null>`, returning `null` on **both** `401` and `404`. But Decision 1 requires a caller to tell those two apart: a `401` (not logged in at all) must render an inline login nudge, while a `404` (logged in, but not the owner) must render **nothing** — the spec is explicit these are different, intentional outcomes, not both "no tickets to show." A single function that collapses both statuses to one `null` value cannot by itself support that branch.

**This plan resolves the gap by composition, not by changing the spec-pinned function signature:** `components/TicketPanel.tsx` (Task 5) first calls the already-established `getSession()` (existing in `lib/api.ts`, already used by `AuthStatus`) to determine "logged in or not" independently, then calls `getTicketsForTrackedTrain` (which keeps exactly the signature and 401/404-collapsing behavior the spec's contract section specifies) to determine "owns this pin or not" among logged-in viewers. This reuses an existing, already-established primitive instead of inventing new status-passing plumbing or widening the pinned function's return type — but it is this plan's own judgment call about how to fill a real gap in the spec's contract, not something the spec itself resolved, and is flagged here rather than resolved silently.

## Global Constraints

- **No backend changes.** No task may modify anything under `crates/`. Every backend route/type this plan consumes is a read-only input.
- **This feature has no unauthenticated read path at all** (Decision 4) — every one of the five ticket routes requires `AuthenticatedUser`. `TicketPanel`'s own probe (Task 5) *is* the entire "are you logged in" check for read purposes; there is no separate public partial view to fall back to. This is a real, deliberate difference from train tracking's public-read/session-gated-write split.
- **Reads never go through the `/api/*` proxy; mutations never go through `lib/api.ts`.** `getTicketsForTrackedTrain`/`getDelayRepayEstimate` (Task 3) are server-only, cookie-forwarding reads called from `TicketPanel`. `POST .../tickets`, `POST .../tickets/pkpass`, `POST .../tickets/pdf` (all in Task 6's `TicketEntryForm`) are browser-initiated mutations called only via `fetch('/api/Train/...')` through the proxy — this is the same split `TrackTrainForm`/`PinToggle` already establish for the sibling train-tracking feature.
- **Review-before-save is a real, structural frontend property, not just backend-enforced.** Every field pre-filled from a `.pkpass`/PDF upload preview (Task 6) stays a normal, editable input — no "accept as-is" button that skips the CRS-format correction the backend requires anyway. The `source` value carried into the final submit is whichever tier produced the starting point (`manual` / `pkpass-semantics` / `pkpass-heuristic` / `pdf-heuristic`); a subsequent manual edit to any field does **not** reset `source` back to `manual`. Only a user who never touched an upload keeps `source: 'manual'`.
- **The Delay Repay disclaimer is rendered verbatim, never paraphrased.** `DelayRepayEstimate` (Task 4) renders `response.disclaimer` (the top-level field, always populated regardless of `estimate`) exactly as received, in full, every time this section renders — never shortened, never hardcoded as an equivalent-sounding sentence, so a future backend wording change is picked up automatically. `estimate.disclaimer` (present only when `estimate` is non-null, a textually *different* string) is never additionally rendered — two near-duplicate-but-not-identical caveats on screen at once would read as inconsistent, not doubly cautious (Decision 3's own reasoning, flagged there as revisitable if the two strings drift further apart).
- **`claimUrl` is always a real, clickable, external-labelled link — never phrasing that could read as this app performing a claim.** Rendered with `target="_blank" rel="noopener noreferrer"` (the only place in this feature that opens a new tab; every other action stays same-page), labelled to describe leaving the app (e.g. "See how to claim from the operator ↗"), never "Claim now"/"Submit claim"/similar.
- **No new refresh mechanism.** `TicketPanel` and its per-ticket Delay Repay fetches are ordinary `no-store` Server Component reads, covered by the existing global `AutoRefresh` (Decision 5) — no per-route opt-out, no manual "check now" button.
- **Wire-type convention, matching the design spec's own API contract exactly:** `TicketEntryRequest` (the only request body this feature sends) is plain `snake_case`, matching `TrackPinRequest`'s existing convention. `TrackedTrainTicket`, `PartialTicket`, `DelayRepayEstimate`, `DelayRepayEstimateResponse` are camelCase, matching every other `crates/api` public JSON response this frontend already consumes.
- **No established local convention exists for a file-input control** (design spec's Open Question 5) — `FileInput` ships with the already-installed `@mantine/core` but is unused elsewhere in this codebase. Task 6 uses it directly, per the spec's own assumption; this plan does not invent a wrapper component or a new local convention for it.
- **Out of scope, per the spec's own "Explicitly out of scope" section — no task may build any of these:** editing or deleting a saved ticket (no `PUT`/`DELETE` route exists), a "my tickets across all tracked trains" view (no unscoped backend route exists), client-side file-size/type pre-validation beyond what the backend enforces (nice-to-have, not designed here), any UI implying this app can submit a claim or prove travel (a hard constraint, not a gap to fill later), widening `AutoRefresh` for the finished-journey staleness case (inherited from the train-tracking-frontend plan, not reopened here).
- **Testing convention:** colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). Every task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures.
- **Testing an async Server Component is new surface for this repo's test suite.** No existing test in this codebase renders an `async function` component directly (confirmed: neither of the two existing tracked-train pages, nor any other async Server Component, has a colocated test — their synchronous sub-components carry the coverage instead). `TicketPanel` (Task 5) is this frontend's first async component that itself needs direct render-test coverage, per the spec's own Testing section. This plan uses the standard technique for this — call the async function directly and `await` its returned element before handing it to `renderWithMantine` (e.g. `renderWithMantine(await TicketPanel({ trackingId: 1 }))`) — flagged here as genuinely new, not an established pattern being reused.

---

### Task 1: Fix the `/api/[...path]` proxy for binary/multipart passthrough

**Files:**
- Modify: `frontend/app/api/[...path]/route.ts`
- Modify: `frontend/app/api/[...path]/route.test.ts` (already exists, from the train-tracking-frontend plan — extend it, don't replace its existing three tests)

**Interfaces:**
- Produces: a `proxy()` that forwards the incoming request's real `Content-Type` header (falling back to `application/json` only when the incoming request has none — every existing JSON caller already sets this header itself, so this is a no-op for them) and forwards the raw body via `arrayBuffer()` instead of `text()`, so a multipart boundary and binary payload both survive the round trip.
- Consumed by: Task 6 (`TicketEntryForm`'s `.pkpass`/PDF uploads, `fetch('/api/Train/{trackingId}/tickets/pkpass', { method: 'POST', body: formData })` with no explicit `Content-Type` header — the browser sets the correct `multipart/form-data; boundary=...` value itself, and this fix is what lets that boundary and the binary bytes both survive to the backend).

This is genuinely early, load-bearing work — per Correction 2 and Open Question 4 of the design spec, this file is shared infrastructure every existing browser-initiated mutation already depends on, so this task's regression tests must cover the existing JSON callers, not just the new binary path.

- [ ] **Step 1: Confirm the current text to change**

Re-read `frontend/app/api/[...path]/route.ts` and confirm it still reads (as of 2026-08-30, verified while writing this plan):

```ts
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const cookie = req.headers.get('cookie');
  if (cookie) {
    headers.Cookie = cookie;
  }
```

and, further down:

```ts
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    init.body = await req.text();
  }
```

- [ ] **Step 2: Forward the incoming `Content-Type` instead of hardcoding one**

Replace the header block with:

```ts
  // Forward the incoming Content-Type verbatim rather than hardcoding
  // 'application/json' -- a browser's fetch(url, { body: formData }) sets
  // its own 'multipart/form-data; boundary=...' header, and axum's
  // Multipart extractor needs that exact boundary value to parse an
  // uploaded file field at all (the ticket-upload routes this proxy must
  // now support --
  // docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
  // Correction 2). Every existing JSON caller (PinToggle, TrackTrainForm,
  // preferences, the OIDC flow) already sets its own
  // 'Content-Type': 'application/json' header on the request it sends to
  // this proxy, so this is inert for them -- the fallback below only
  // matters for a request that somehow reaches this proxy with no
  // Content-Type header at all.
  const headers: Record<string, string> = {
    'Content-Type': req.headers.get('content-type') ?? 'application/json',
  };
  const cookie = req.headers.get('cookie');
  if (cookie) {
    headers.Cookie = cookie;
  }
```

- [ ] **Step 3: Forward the body as raw bytes instead of decoded text**

Replace the body-forwarding block with:

```ts
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    // arrayBuffer(), not text(): .text() decodes the incoming body as
    // UTF-8 before this function ever sees it, which is LOSSY for
    // non-UTF-8 bytes -- a .pkpass (zip) or PDF's raw bytes are binary and
    // not valid UTF-8 in general, so any invalid byte sequence becomes a
    // U+FFFD replacement character on the way through, silently
    // corrupting the file before it reaches the backend. arrayBuffer()
    // forwards the exact bytes the browser sent, with no decode/re-encode
    // step -- a JSON body round-trips identically (JSON is always valid
    // UTF-8, so this is inert for PinToggle/TrackTrainForm/preferences/
    // auth) and a binary multipart body survives byte-for-byte.
    init.body = await req.arrayBuffer();
  }
```

- [ ] **Step 4: Extend the existing test file**

`frontend/app/api/[...path]/route.test.ts` already has three tests (regression for `/public` passthrough, `/Train/track` forwarding, and the traversal 400). Widen its import to include `PUT`, then add:

```ts
import { GET, POST, PUT } from './route';
```

```ts
  it('forwards a multipart/form-data upload with its original Content-Type (boundary intact)', async () => {
    const boundary = '----testboundary123';
    const req = makeRequest('/api/Train/1/tickets/pkpass', {
      method: 'POST',
      headers: {
        cookie: 'nr_session=abc123',
        'content-type': `multipart/form-data; boundary=${boundary}`,
      },
      body: `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="t.pkpass"\r\n\r\nfake-bytes\r\n--${boundary}--`,
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', '1', 'tickets', 'pkpass'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedHeaders = (init as { headers: Record<string, string> }).headers;
    expect(forwardedHeaders['Content-Type']).toBe(`multipart/form-data; boundary=${boundary}`);
  });

  it('forwards binary body bytes unchanged (does not lossily decode as UTF-8 text)', async () => {
    // A byte sequence that is invalid UTF-8 on its own (0xff is never a
    // valid standalone UTF-8 byte) -- .text() would have replaced it with
    // U+FFFD before this test could ever observe the original bytes;
    // arrayBuffer() must not.
    const rawBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04, 0xff, 0x00, 0x89]);
    const req = new NextRequest('http://localhost:3000/api/Train/1/tickets/pkpass', {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: rawBytes,
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', '1', 'tickets', 'pkpass'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedBody = new Uint8Array((init as { body: ArrayBuffer }).body);
    expect(Array.from(forwardedBody)).toEqual(Array.from(rawBytes));
  });

  it('still forwards a JSON body byte-identically (regression: existing callers unaffected)', async () => {
    const req = makeRequest('/api/preferences/pinned-lines', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(['wcml']),
    });
    await PUT(req, { params: Promise.resolve({ path: ['preferences', 'pinned-lines'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedHeaders = (init as { headers: Record<string, string> }).headers;
    const forwardedBody = new TextDecoder().decode((init as { body: ArrayBuffer }).body);
    expect(forwardedHeaders['Content-Type']).toBe('application/json');
    expect(forwardedBody).toBe(JSON.stringify(['wcml']));
  });
```

- [ ] **Step 5: Run the proxy's own test file**

Run (from `frontend/`): `npm test -- route.test.ts`
Expected: all six tests (three existing + three new) PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS — no regression to any existing `/api/*` consumer (`PinToggle`, `TrackTrainForm`, preferences, the OIDC login/callback flow).

- [ ] **Step 7: Commit**

```bash
git add frontend/app/api/[...path]/route.ts frontend/app/api/[...path]/route.test.ts
git commit -m "Make the /api proxy binary-safe so it can forward multipart ticket uploads"
```

---

### Task 2: `lib/types.ts` additions

**Files:**
- Modify: `frontend/lib/types.ts`

**Interfaces:**
- Produces: `TicketSource`, `TrackedTrainTicket`, `TicketEntryRequest`, `TicketCreatedResponse`, `PartialTicket`, `DelayRepayEstimate`, `DelayRepayEstimateResponse`.
- Consumed by: Task 3 (`lib/api.ts`'s two new read functions), Task 4 (`DelayRepayEstimate` component), Task 5 (`TicketPanel`), Task 6 (`TicketEntryForm`).

These are copied directly from the design spec's own hand-written API/type contract section (already verified there against the live backend response shapes) — this task does not re-derive them.

- [ ] **Step 1: Add the types**

Add to `frontend/lib/types.ts`, after `TrackPinResponse`:

```ts
export type TicketSource = 'manual' | 'pkpass-semantics' | 'pkpass-heuristic' | 'pdf-heuristic';

/** `GET /Train/{trackingId}/tickets`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainTicket`,
 * camelCase). Never includes `userId` -- same posture as
 * `TrackedTrainState`. Nothing caps a tracked train at one ticket; multiple
 * tickets per tracked train are a real, supported case (see
 * `components/TicketPanel.tsx`). */
export interface TrackedTrainTicket {
  id: number;
  trackedTrainId: number;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339
}

/** `POST /Train/{trackingId}/tickets`'s request body
 * (`common::TicketEntryRequest`) -- snake_case, matching `TrackPinRequest`'s
 * own internal-wire-type convention (unlike every other type in this file,
 * which mirrors `crates/api`'s camelCase public JSON). `source` is not
 * optional on this type even though the backend defaults it to `'manual'`
 * -- `components/TicketEntryForm.tsx` always sends it explicitly, since it
 * needs to track the current provenance of the fields it's submitting
 * regardless of which tab produced them. */
export interface TicketEntryRequest {
  operator?: string;
  ticket_type?: string;
  origin_crs?: string;
  destination_crs?: string;
  source: TicketSource;
}

export interface TicketCreatedResponse {
  ticketId: number;
}

/** `POST .../tickets/pkpass` and `POST .../tickets/pdf`'s shared response
 * shape -- every field independently nullable; "not found in this file" is
 * expected, not an error. Never persisted to the database by either upload
 * route -- this is only ever a preview
 * (`components/TicketEntryForm.tsx` pre-fills the manual-entry fields from
 * it and requires a second, separate submit to actually save anything). */
export interface PartialTicket {
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
}

/** Present only inside a non-null `DelayRepayEstimateResponse.estimate`.
 * `disclaimer` here is a DIFFERENT string from
 * `DelayRepayEstimateResponse.disclaimer` (the top-level field) -- see
 * `components/DelayRepayEstimate.tsx`, which renders only the top-level
 * one. */
export interface DelayRepayEstimate {
  scheme: 'DR15' | 'DR30';
  bandMinutes: number;
  percentage: number;
  disclaimer: string;
}

/** `GET .../tickets/{ticketId}/delay-repay`'s response. `claimUrl` and the
 * top-level `disclaimer` are ALWAYS populated, independent of `estimate` --
 * this route never returns a bare percentage with no caveat and no link.
 * `estimate` is `null` whenever any of three things is true (no operator on
 * the ticket, no delay data on the train yet, or a real delay that just
 * didn't clear the matched scheme's lowest band) -- the response gives no
 * signal which of the three applied; see
 * `components/DelayRepayEstimate.tsx` for how this is rendered honestly
 * without inventing a reason the API doesn't give. */
export interface DelayRepayEstimateResponse {
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
}
```

- [ ] **Step 2: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS — additive types, unused by anything yet.

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "Add ticket/delay-repay wire types"
```

---

### Task 3: `lib/api.ts` additions — session-gated ticket reads

**Files:**
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `getTicketsForTrackedTrain(trackingId: number): Promise<TrackedTrainTicket[] | null>`, `getDelayRepayEstimate(trackingId: number, ticketId: number): Promise<DelayRepayEstimateResponse | null>`.
- Consumed by: Task 5 (`TicketPanel`, alongside `getSession()` per this plan's own gap-resolution note above).

Both use the same cookie-forwarding pattern `getSession()`/`getPreferences()` already establish (a Server Component's own `fetch` does not inherit the incoming request's cookies) — no new plumbing pattern, reuse of an existing one.

- [ ] **Step 1: Add the two functions**

Add `TrackedTrainTicket, DelayRepayEstimateResponse` to `frontend/lib/api.ts`'s existing `import type { ... } from './types';` list, then add after `getTrackedTrainByUidAndDate`:

```ts
/** Per-user, session-gated ticket list for one tracked train
 * (`GET /Train/{trackingId}/tickets`). Same cookie-forwarding pattern as
 * `getPreferences`/`getSession` (a Server Component's own fetch does not
 * inherit the incoming request's cookies). Returns `null` on BOTH `401`
 * and `404` -- deliberately not thrown as `ApiNotFoundError`, since "you're
 * not the owner of this pin" is an expected, common outcome for a public,
 * shareable tracked-train page (every non-owner viewer hits this), not an
 * exceptional one. This collapses two different real conditions (not
 * logged in at all vs. logged in but not the owner) into one `null` --
 * `components/TicketPanel.tsx` tells them apart itself by separately
 * calling the existing `getSession()` first, since widening this
 * function's own signature would depart from the design spec's own
 * hand-written contract for it. */
export async function getTicketsForTrackedTrain(trackingId: number): Promise<TrackedTrainTicket[] | null> {
  const url = `${baseUrl()}/Train/${trackingId}/tickets`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401 || response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TrackedTrainTicket[]>;
}

/** Per-ticket Delay Repay estimate
 * (`GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`). Same
 * cookie-forwarding and null-on-401/404 shape as
 * `getTicketsForTrackedTrain` above -- called only from within the "you
 * own this pin" branch `TicketPanel` has already established (see that
 * component), so a `null` here in practice means the specific ticket id
 * didn't resolve under this tracking id, a narrower condition than the
 * top-level list's 401/404 split -- `TicketPanel` treats it as "no
 * estimate to show for this ticket" rather than failing the whole page. */
export async function getDelayRepayEstimate(
  trackingId: number,
  ticketId: number,
): Promise<DelayRepayEstimateResponse | null> {
  const url = `${baseUrl()}/Train/${trackingId}/tickets/${ticketId}/delay-repay`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401 || response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<DelayRepayEstimateResponse>;
}
```

- [ ] **Step 2: Add tests**

Add `getTicketsForTrackedTrain, getDelayRepayEstimate` to `frontend/lib/api.test.ts`'s existing import list from `./api`, then add:

```ts
  it('getTicketsForTrackedTrain fetches the correct URL, forwarding cookies, with no caching', async () => {
    incomingCookies.header = 'distant_signal_session=abc123';
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await getTicketsForTrackedTrain(1);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/1/tickets',
      expect.objectContaining({
        cache: 'no-store',
        headers: { Cookie: 'distant_signal_session=abc123' },
      }),
    );
  });

  it('getTicketsForTrackedTrain returns null on a 401 (not logged in)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toBeNull();
  });

  it('getTicketsForTrackedTrain returns null on a 404 (logged in, not the owner)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toBeNull();
  });

  it('getTicketsForTrackedTrain resolves an empty array as owner-with-no-tickets, not null', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toEqual([]);
  });

  it('getTicketsForTrackedTrain still throws on a non-401/404 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getTicketsForTrackedTrain(1)).rejects.toThrow(/500/);
  });

  it('getDelayRepayEstimate fetches the correct URL with no caching', async () => {
    const sample = { delayMinutes: 45, estimate: null, claimUrl: 'https://example.com', disclaimer: 'x' };
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(sample), { status: 200 })));
    await getDelayRepayEstimate(1, 7);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/1/tickets/7/delay-repay',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getDelayRepayEstimate returns null on a 401 or 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getDelayRepayEstimate(1, 7)).resolves.toBeNull();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getDelayRepayEstimate(1, 7)).resolves.toBeNull();
  });
```

- [ ] **Step 3: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: all tests, including the seven new ones, PASS.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add session-gated ticket and delay-repay read functions"
```

---

### Task 4: `TextLink` external-link support, and the `DelayRepayEstimate` component

**Files:**
- Modify: `frontend/components/TextLink.tsx`
- Modify: `frontend/components/TextLink.test.tsx`
- Create: `frontend/components/DelayRepayEstimate.tsx`
- Create: `frontend/components/DelayRepayEstimate.test.tsx`

**Interfaces:**
- Produces: `TextLink`'s optional `target`/`rel` props (backward-compatible — every existing call site passes neither and gets byte-identical output). `DelayRepayEstimate({ response: DelayRepayEstimateResponse })` — pure presentational, no fetch of its own.
- Consumed by: Task 5 (`TicketPanel` renders one `DelayRepayEstimate` per ticket that has a Delay Repay response).

`claimUrl` is an external, out-of-app URL and this is the only place in the whole feature that needs to open a new tab (per Decision 3's own external-link hygiene note). `TextLink` — "the app's single link component" (confirmed: used throughout, no second link component exists) — has no `target`/`rel` support today; this task extends it rather than introducing a parallel one-off external-link component.

- [ ] **Step 1: Extend `TextLink` with optional `target`/`rel`**

In `frontend/components/TextLink.tsx`, change the prop signature and the render:

```tsx
export function TextLink({
  href,
  children,
  underline = 'hover',
  target,
  rel,
}: {
  href: string;
  children: React.ReactNode;
  underline?: 'hover' | 'always';
  target?: string;
  rel?: string;
}) {
  return (
    <Link href={href} data-text-link={underline} target={target} rel={rel}>
      <Text c="var(--mantine-color-anchor)">{children}</Text>
    </Link>
  );
}
```

`target`/`rel` are `undefined` for every existing call site, so `next/link` renders no `target`/`rel` attribute at all for them — identical output to today.

- [ ] **Step 2: Add a regression test confirming existing call sites are unaffected**

Add to `frontend/components/TextLink.test.tsx`:

```tsx
  it('renders no target/rel by default (regression: every existing call site omits them)', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).not.toHaveAttribute('target');
    expect(link).not.toHaveAttribute('rel');
  });

  it('can opt into target/rel for an external link', () => {
    renderWithMantine(
      <TextLink href="https://example.com" target="_blank" rel="noopener noreferrer">
        External
      </TextLink>,
    );
    const link = screen.getByRole('link', { name: 'External' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  });
```

- [ ] **Step 3: Write `DelayRepayEstimate`**

Create `frontend/components/DelayRepayEstimate.tsx`:

```tsx
import { Alert, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';
import type { DelayRepayEstimateResponse } from '@/lib/types';

/** Renders one ticket's Delay Repay estimate, per
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
 * Decision 3. Pure presentational -- takes an already-fetched response, no
 * fetch of its own (the per-ticket fetch lives in `TicketPanel`).
 *
 * SAFETY-CRITICAL, carried forward verbatim from the backend, not
 * paraphrased: `response.disclaimer` (the TOP-LEVEL field, always
 * populated regardless of `estimate`) is rendered exactly as received, in
 * full, every time this component renders -- never shortened, never
 * hardcoded as an equivalent-sounding sentence, so a future backend
 * wording change is picked up automatically just by rendering the field.
 * `estimate.disclaimer` (present only when `estimate` is non-null, a
 * textually DIFFERENT string from the top-level one) is deliberately never
 * rendered here -- two near-duplicate-but-not-identical caveats on screen
 * at once would read as inconsistent, not doubly cautious (Decision 3's
 * own reasoning; flagged there as revisitable if the two strings ever
 * drift further apart). `claimUrl` is always rendered as a real outbound
 * link, labelled to describe leaving this app -- never phrasing that could
 * read as this app performing a claim itself. */
export function DelayRepayEstimate({ response }: { response: DelayRepayEstimateResponse }) {
  return (
    <Stack gap={4}>
      <EstimateSummary response={response} />
      <Text size="sm">{response.disclaimer}</Text>
      {/* The only place in this feature that opens a new tab -- every
          other action stays same-page. */}
      <TextLink href={response.claimUrl} underline="always" target="_blank" rel="noopener noreferrer">
        See how to claim from the operator ↗
      </TextLink>
    </Stack>
  );
}

function EstimateSummary({ response }: { response: DelayRepayEstimateResponse }) {
  const { estimate, delayMinutes } = response;

  if (estimate) {
    return (
      <Alert color="blue" title="Estimated Delay Repay eligibility" variant="light">
        Estimated compensation: {estimate.percentage}% of your fare ({estimate.scheme}, {estimate.bandMinutes}+
        minute delay). This is an estimate, not a guarantee.
      </Alert>
    );
  }

  if (delayMinutes !== null) {
    // Deliberate: the API gives no way to distinguish "you're genuinely
    // under threshold" from "we don't recognize this operator's scheme"
    // from "some other reason didn't clear a band" -- this copy must not
    // assert a specific one of the three the response doesn't support.
    return (
      <Text size="sm">
        Based on the recorded delay ({delayMinutes} minutes), this operator&apos;s Delay Repay rules may not give
        a payout at that length — but rules vary and this estimate can be wrong, so it&apos;s still worth checking
        directly.
      </Text>
    );
  }

  return (
    <Text size="sm">
      No delay data recorded yet for this journey — if you already know you were delayed, the link below still
      goes straight to the operator.
    </Text>
  );
}
```

- [ ] **Step 4: Write the tests**

Create `frontend/components/DelayRepayEstimate.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { DelayRepayEstimateResponse } from '@/lib/types';

const TOP_LEVEL_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. This app never submits a claim on your behalf -- verify eligibility and claim directly from the operator using the link above.';
const ESTIMATE_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. Always verify eligibility and submit any claim directly with the operator -- this app never submits a claim on your behalf.';

function response(overrides: Partial<DelayRepayEstimateResponse> = {}): DelayRepayEstimateResponse {
  return {
    delayMinutes: null,
    estimate: null,
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: TOP_LEVEL_DISCLAIMER,
    ...overrides,
  };
}

describe('DelayRepayEstimate', () => {
  it('estimate present: shows the scheme/band/percentage, framed as an estimate', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 35,
          estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: ESTIMATE_DISCLAIMER },
        })}
      />,
    );
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByText(/DR30/)).toBeInTheDocument();
    expect(screen.getByText(/estimate, not a guarantee/)).toBeInTheDocument();
  });

  it('estimate null with a real delayMinutes: does not assert a specific reason', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ delayMinutes: 10 })} />);
    expect(screen.getByText(/10 minutes/)).toBeInTheDocument();
    expect(screen.getByText(/rules vary and this estimate can be wrong/)).toBeInTheDocument();
  });

  it('estimate and delayMinutes both null: says no delay data recorded yet', () => {
    renderWithMantine(<DelayRepayEstimate response={response()} />);
    expect(screen.getByText(/No delay data recorded yet/)).toBeInTheDocument();
  });

  it('always renders the top-level disclaimer verbatim, in every branch', () => {
    const cases = [
      response(),
      response({ delayMinutes: 10 }),
      response({ delayMinutes: 35, estimate: { scheme: 'DR15', bandMinutes: 30, percentage: 50, disclaimer: ESTIMATE_DISCLAIMER } }),
    ];
    for (const r of cases) {
      const { unmount } = renderWithMantine(<DelayRepayEstimate response={r} />);
      expect(screen.getByText(TOP_LEVEL_DISCLAIMER)).toBeInTheDocument();
      unmount();
    }
  });

  it('never renders estimate.disclaimer a second time alongside the top-level one', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 60,
          estimate: { scheme: 'DR15', bandMinutes: 60, percentage: 100, disclaimer: ESTIMATE_DISCLAIMER },
        })}
      />,
    );
    expect(screen.queryByText(ESTIMATE_DISCLAIMER)).not.toBeInTheDocument();
  });

  it('always renders claimUrl as an external, new-tab link, never claim-performing language', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ claimUrl: 'https://example.com/claim' })} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', 'https://example.com/claim');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.queryByText(/^Claim now$/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Submit claim$/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test -- TextLink.test.tsx DelayRepayEstimate.test.tsx`
Expected: all tests PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/components/TextLink.tsx frontend/components/TextLink.test.tsx frontend/components/DelayRepayEstimate.tsx frontend/components/DelayRepayEstimate.test.tsx
git commit -m "Add external-link support to TextLink and the DelayRepayEstimate component"
```

---

### Task 5: `TicketPanel` component — the ownership-gated entry point

**Files:**
- Create: `frontend/components/TicketPanel.tsx`
- Create: `frontend/components/TicketPanel.test.tsx`

**Interfaces:**
- Consumes: `getSession`, `getTicketsForTrackedTrain`, `getDelayRepayEstimate` (Task 3), `DelayRepayEstimate` (Task 4), `TicketEntryForm` (Task 6 — written after this task but referenced by it; see the ordering note below).
- Produces: `async function TicketPanel({ trackingId }: { trackingId: number })`, implementing all four of Decision 1's branches.
- Consumed by: Task 7 (both existing tracked-train pages).

**Ordering note:** this task's component imports `TicketEntryForm` from Task 6, which does not exist yet at this point in the plan. Either implement Task 6 first and this task second, or stub `TicketEntryForm` with a minimal placeholder export here and let Task 6 replace it — either order is fine; this plan lists `TicketPanel` first because it is the simpler, read-only half of the feature and a natural place to validate the auth-branching logic before adding the more involved form. If executing task-by-task strictly in order, write a temporary one-line placeholder (`export function TicketEntryForm() { return null; }` in a throwaway location or an early stub in Task 6's file) so this task's build/tests pass, then replace it in Task 6.

- [ ] **Step 1: Write the component**

Create `frontend/components/TicketPanel.tsx`:

```tsx
import { Divider, Stack, Text } from '@mantine/core';
import { getSession, getTicketsForTrackedTrain, getDelayRepayEstimate } from '@/lib/api';
import { TextLink } from './TextLink';
import { TicketEntryForm } from './TicketEntryForm';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { TrackedTrainTicket } from '@/lib/types';

/** Renders on both `/train/by-id/[trackingId]` and `/train/[uid]/[date]`,
 * directly below `<TrainJourney>`. This is a real, session-gated feature
 * with NO unauthenticated read path at all (Decision 4 of
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md)
 * layered onto two PUBLIC pages (any viewer, owner or not, can load either
 * page, per the train-tracking-frontend spec) -- so this component's own
 * ownership probe *is* the entire "is this yours" check for the whole
 * ticket feature (Decision 1).
 *
 * Branches on four real, distinguishable outcomes. `401` and `404` from
 * `GET .../tickets` both collapse to `null` from `getTicketsForTrackedTrain`
 * (see that function's own doc comment in `lib/api.ts`), so this component
 * separately calls the already-established `getSession()` first to tell
 * "not logged in at all" apart from "logged in, but not the owner of this
 * pin" -- the two cases Decision 1 requires rendering completely
 * differently (a login nudge vs. nothing at all). This composition, not a
 * change to `getTicketsForTrackedTrain`'s own spec-pinned signature, is how
 * this plan resolves that gap -- see this plan's own top-level note on it. */
export async function TicketPanel({ trackingId }: { trackingId: number }) {
  const session = await getSession();
  if (!session.authenticated) {
    // Worded as "attach a ticket," not "see your ticket" -- logging in
    // doesn't guarantee this viewer owns this particular pin, so the copy
    // doesn't promise something a subsequent not-the-owner outcome might
    // immediately take back.
    return (
      <Text size="sm">
        <TextLink href="/api/auth/login" underline="always">
          Log in to attach a ticket to this journey
        </TextLink>
      </Text>
    );
  }

  const tickets = await getTicketsForTrackedTrain(trackingId);
  if (tickets === null) {
    // Logged in, but not the owner of this pin (or, redundantly, a
    // tracking id that already 404'd the page itself upstream). Every
    // tracked-train page is public and shareable by design, so this is
    // the overwhelming common case for a page view -- render nothing, not
    // a permanent "this isn't your journey" banner (Decision 1's own
    // reasoning for why this branch stays silent rather than explicit).
    return null;
  }

  if (tickets.length === 0) {
    return <TicketEntryForm trackingId={trackingId} label="Add a ticket for this journey" />;
  }

  // Eager, one fetch per ticket, not a client-triggered "check
  // eligibility" button -- consistent with this app's existing "just
  // refetch, no manual poll control" posture (Decision 5; flagged in the
  // design spec's Open Question 1 as fine for the expected common case of
  // a handful of tickets per tracked train, not resolved further here).
  const withEstimates = await Promise.all(
    tickets.map(async (ticket) => ({
      ticket,
      estimate: await getDelayRepayEstimate(trackingId, ticket.id),
    })),
  );

  return (
    <Stack gap="lg">
      {withEstimates.map(({ ticket, estimate }, index) => (
        <Stack key={ticket.id} gap="xs">
          {index > 0 && <Divider />}
          <TicketSummary ticket={ticket} />
          {estimate && <DelayRepayEstimate response={estimate} />}
        </Stack>
      ))}
      <TicketEntryForm trackingId={trackingId} label="Add another ticket" />
    </Stack>
  );
}

function TicketSummary({ ticket }: { ticket: TrackedTrainTicket }) {
  const route =
    ticket.originCrs || ticket.destinationCrs ? `${ticket.originCrs ?? '?'} → ${ticket.destinationCrs ?? '?'}` : null;
  return (
    <Stack gap={2}>
      <Text fw={500}>
        {ticket.operator ?? 'Ticket'}
        {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
      </Text>
      {route && <Text size="sm">{route}</Text>}
    </Stack>
  );
}
```

- [ ] **Step 2: Write the tests**

Create `frontend/components/TicketPanel.test.tsx`. This mocks `@/lib/api` entirely (unlike every synchronous component test elsewhere in this codebase) and calls the async component function directly, `await`-ing its returned element before handing it to `renderWithMantine` — the new testing technique flagged in this plan's Global Constraints, since no existing test in this repo exercises an async Server Component directly.

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketPanel } from './TicketPanel';
import * as api from '@/lib/api';

vi.mock('@/lib/api');

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'user-1' : null, email: null, name: null };
}

describe('TicketPanel', () => {
  beforeEach(() => {
    vi.mocked(api.getDelayRepayEstimate).mockResolvedValue(null);
  });

  it('401 (not logged in): shows a login nudge to attach a ticket', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByRole('link', { name: 'Log in to attach a ticket to this journey' })).toHaveAttribute(
      'href',
      '/api/auth/login',
    );
  });

  it('404 (logged in, not the owner): renders nothing', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue(null);
    const element = await TicketPanel({ trackingId: 1 });
    const { container } = renderWithMantine(element);
    expect(container).toBeEmptyDOMElement();
  });

  it('200 with an empty array (owner, no ticket yet): shows the add-a-ticket entry point', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([]);
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
  });

  it('200 with tickets: renders each ticket and its own delay-repay estimate, plus an add-another affordance', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      {
        id: 1,
        trackedTrainId: 1,
        operator: 'LNER',
        ticketType: 'single',
        originCrs: 'KGX',
        destinationCrs: 'EDB',
        source: 'manual',
        createdAt: '2026-08-29T12:00:00Z',
      },
    ]);
    vi.mocked(api.getDelayRepayEstimate).mockResolvedValue({
      delayMinutes: 45,
      estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'x' },
      claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
      disclaimer: 'y',
    });
    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add another ticket' })).toBeInTheDocument();
  });

  it('multiple tickets: fetches a delay-repay estimate per ticket, not just the first', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      { id: 1, trackedTrainId: 1, operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null, source: 'manual', createdAt: '2026-08-29T12:00:00Z' },
      { id: 2, trackedTrainId: 1, operator: 'CrossCountry', ticketType: null, originCrs: null, destinationCrs: null, source: 'manual', createdAt: '2026-08-29T13:00:00Z' },
    ]);
    await TicketPanel({ trackingId: 1 });
    expect(api.getDelayRepayEstimate).toHaveBeenCalledWith(1, 1);
    expect(api.getDelayRepayEstimate).toHaveBeenCalledWith(1, 2);
  });
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- TicketPanel.test.tsx`
Expected: all five tests PASS (once Task 6's real `TicketEntryForm` exists, or against the temporary placeholder if built out of order per this task's ordering note).

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TicketPanel.tsx frontend/components/TicketPanel.test.tsx
git commit -m "Add TicketPanel: ownership-gated ticket list and delay-repay display"
```

---

### Task 6: `TicketEntryForm` component — manual entry plus two upload accelerants

**Files:**
- Create: `frontend/components/TicketEntryForm.tsx`
- Create: `frontend/components/TicketEntryForm.test.tsx`

**Interfaces:**
- Consumes: `TicketEntryRequest`, `PartialTicket`, `TicketSource` (Task 2), the proxy fixed in Task 1 (functionally, at end-to-end runtime).
- Produces: `TicketEntryForm({ trackingId, label }: { trackingId: number; label: string })`. Collapsed by default (renders only a `label`-captioned button); expands into the full manual/upload form on click, per Decision 1's "entry point that opens the form."
- Consumed by: Task 5 (`TicketPanel`, both the "no tickets yet" and "has tickets" branches, differing only in `label`).

This is genuinely new interaction surface for this codebase (Correction 3: no file-upload UI exists anywhere in this frontend today; `FileInput`/`Tabs` are both unused elsewhere). Mirrors `TrackTrainForm`'s `needsLogin` 401 pattern for its own final submit and, per Decision 4, needs that same defensive handling at the upload step too — a session can lapse between `TicketPanel`'s server-side probe and either client-side action.

- [ ] **Step 1: Write the component**

Create `frontend/components/TicketEntryForm.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Badge, Button, FileInput, Group, Stack, Tabs, TextInput } from '@mantine/core';
import { TextLink } from './TextLink';
import type { PartialTicket, TicketEntryRequest, TicketSource } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
type Tab = 'manual' | 'pkpass' | 'pdf';

/** The upload/manual-entry flow for one journey, per
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
 * Decision 2. Collapsed by default -- `label` (either "Add a ticket for
 * this journey" or "Add another ticket", set by the caller) is the entry
 * point that expands this into the real form, matching Decision 1's
 * "entry point that opens TicketEntryForm" -- the exact collapse mechanism
 * is this plan's own choice, since the spec doesn't detail it further,
 * kept self-contained here so `TicketPanel` (Task 5) stays a plain,
 * server-renderable async function with no interactive state of its own.
 *
 * Three ways to arrive at the same underlying field set and the same final
 * submit: manual entry (default, always available, every field optional),
 * `.pkpass` upload, and PDF upload. Both uploads are read-only PREVIEWS --
 * a `200` from either upload route pre-fills the manual-entry fields and
 * switches back to the manual view; it never bypasses that view or offers
 * a one-click accept. `source` is whatever tier produced the current
 * starting point and is NOT reset to 'manual' by a later manual edit --
 * only a user who never touched an upload keeps `source: 'manual'`.
 *
 * Every request here goes through the same-origin `/api/Train/...` proxy
 * (Client Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`/`TrackTrainForm`),
 * fixed for binary uploads by this plan's own Task 1. */
export function TicketEntryForm({ trackingId, label }: { trackingId: number; label: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>('manual');

  const [operator, setOperator] = useState('');
  const [ticketType, setTicketType] = useState('');
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [source, setSource] = useState<TicketSource>('manual');
  const [autoFilled, setAutoFilled] = useState<Set<string>>(new Set());

  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [needsLogin, setNeedsLogin] = useState(false);

  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const destinationValid = destinationCrs.trim() === '' || CRS_PATTERN.test(destinationCrs.trim());

  function resetFields() {
    setOperator('');
    setTicketType('');
    setOriginCrs('');
    setDestinationCrs('');
    setSource('manual');
    setAutoFilled(new Set());
    setTab('manual');
    setUploadError(null);
    setSubmitError(null);
  }

  function clearAutoFilled(field: string) {
    setAutoFilled((current) => {
      const next = new Set(current);
      next.delete(field);
      return next;
    });
  }

  function applyPreview(preview: PartialTicket) {
    const filled = new Set<string>();
    if (preview.operator) {
      setOperator(preview.operator);
      filled.add('operator');
    }
    if (preview.ticketType) {
      setTicketType(preview.ticketType);
      filled.add('ticketType');
    }
    if (preview.originCrs) {
      setOriginCrs(preview.originCrs);
      filled.add('originCrs');
    }
    if (preview.destinationCrs) {
      setDestinationCrs(preview.destinationCrs);
      filled.add('destinationCrs');
    }
    setSource(preview.source);
    setAutoFilled(filled);
    setTab('manual');
  }

  async function handleUpload(file: File | null, kind: 'pkpass' | 'pdf') {
    if (!file) return;
    setUploading(true);
    setUploadError(null);
    setNeedsLogin(false);
    try {
      const formData = new FormData();
      formData.append('file', file);
      // No explicit Content-Type header -- the browser sets the correct
      // 'multipart/form-data; boundary=...' value itself for a FormData
      // body, and Task 1's proxy fix is what lets that boundary survive to
      // the backend.
      const response = await fetch(`/api/Train/${trackingId}/tickets/${kind}`, {
        method: 'POST',
        body: formData,
      });

      if (response.ok) {
        applyPreview((await response.json()) as PartialTicket);
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setUploadError("That doesn't look like a valid upload — try again or fill in the form manually");
        return;
      }
      if (response.status === 422) {
        // Backend's own message is already human-readable, e.g. "could
        // not read this as a train .pkpass: ..." -- safe to surface
        // directly per Decision 2's table.
        setUploadError(await response.text());
        return;
      }
      if (response.status === 504) {
        setUploadError('That file took too long to read — try a smaller or simpler PDF, or fill in the details manually');
        return;
      }
      if (response.status === 413) {
        setUploadError('That file is too large (8 MB limit). Try filling in the details manually');
        return;
      }
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } catch {
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } finally {
      setUploading(false);
    }
  }

  async function handleSubmit() {
    setSubmitting(true);
    setSubmitError(null);
    setNeedsLogin(false);
    try {
      const body: TicketEntryRequest = {
        source,
        ...(operator.trim() ? { operator: operator.trim() } : {}),
        ...(ticketType.trim() ? { ticket_type: ticketType.trim() } : {}),
        ...(originCrs.trim() ? { origin_crs: originCrs.trim().toUpperCase() } : {}),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
      };
      const response = await fetch(`/api/Train/${trackingId}/tickets`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        setOpen(false);
        resetFields();
        router.refresh();
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setSubmitError(await response.text());
        return;
      }
      setSubmitError("Couldn't save this ticket. Try again.");
    } catch {
      setSubmitError("Couldn't save this ticket. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  if (!open) {
    return (
      <Button variant="light" onClick={() => setOpen(true)}>
        {label}
      </Button>
    );
  }

  return (
    <Stack gap="md">
      <Tabs value={tab} onChange={(value) => setTab((value as Tab) ?? 'manual')}>
        <Tabs.List>
          <Tabs.Tab value="manual">Manual entry</Tabs.Tab>
          <Tabs.Tab value="pkpass">Upload .pkpass</Tabs.Tab>
          <Tabs.Tab value="pdf">Upload PDF e-ticket</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="manual" pt="md">
          <Stack gap="sm">
            <TextInput
              label="Operator"
              value={operator}
              onChange={(event) => {
                setOperator(event.currentTarget.value);
                clearAutoFilled('operator');
              }}
              rightSection={autoFilled.has('operator') ? <Badge size="xs">auto-filled</Badge> : undefined}
            />
            <TextInput
              label="Ticket type"
              value={ticketType}
              onChange={(event) => {
                setTicketType(event.currentTarget.value);
                clearAutoFilled('ticketType');
              }}
              rightSection={autoFilled.has('ticketType') ? <Badge size="xs">auto-filled</Badge> : undefined}
            />
            <TextInput
              label="Origin CRS code"
              value={originCrs}
              onChange={(event) => {
                setOriginCrs(event.currentTarget.value);
                clearAutoFilled('originCrs');
              }}
              error={!originValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('originCrs') ? 'Auto-filled — please check this is a real 3-letter CRS code' : undefined
              }
            />
            <TextInput
              label="Destination CRS code"
              value={destinationCrs}
              onChange={(event) => {
                setDestinationCrs(event.currentTarget.value);
                clearAutoFilled('destinationCrs');
              }}
              error={!destinationValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('destinationCrs')
                  ? 'Auto-filled — please check this is a real 3-letter CRS code'
                  : undefined
              }
            />
          </Stack>
        </Tabs.Panel>

        <Tabs.Panel value="pkpass" pt="md">
          <UploadPanel kind="pkpass" accept=".pkpass" uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>

        <Tabs.Panel value="pdf" pt="md">
          <UploadPanel kind="pdf" accept="application/pdf" uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>
      </Tabs>

      {submitError && (
        <Alert color="red" title="Couldn't save this ticket">
          {submitError}
        </Alert>
      )}

      <Group>
        <Button onClick={handleSubmit} disabled={submitting || !originValid || !destinationValid}>
          {submitting ? 'Saving…' : 'Save ticket'}
        </Button>
        <Button variant="subtle" onClick={() => setOpen(false)}>
          Cancel
        </Button>
        {needsLogin && (
          <TextLink href="/api/auth/login" underline="always">
            Log in to save this ticket
          </TextLink>
        )}
      </Group>
    </Stack>
  );
}

function UploadPanel({
  kind,
  accept,
  uploading,
  error,
  onFile,
  onFallback,
}: {
  kind: 'pkpass' | 'pdf';
  accept: string;
  uploading: boolean;
  error: string | null;
  onFile: (file: File | null, kind: 'pkpass' | 'pdf') => void;
  onFallback: () => void;
}) {
  return (
    <Stack gap="sm">
      <FileInput
        label={kind === 'pkpass' ? 'Apple Wallet .pkpass file' : 'PDF e-ticket'}
        accept={accept}
        disabled={uploading}
        onChange={(file) => onFile(file, kind)}
      />
      {error && (
        <Alert color="red">
          <Stack gap={4}>
            <span>{error}</span>
            {/* Always reachable, per Decision 2's table -- the manual form
                is right there and always usable regardless of why the
                upload failed. */}
            <Button variant="subtle" size="xs" onClick={onFallback} style={{ alignSelf: 'flex-start' }}>
              or fill in the details manually
            </Button>
          </Stack>
        </Alert>
      )}
    </Stack>
  );
}
```

- [ ] **Step 2: Write the tests**

Create `frontend/components/TicketEntryForm.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketEntryForm } from './TicketEntryForm';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
}));

describe('TicketEntryForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function openForm() {
    renderWithMantine(<TicketEntryForm trackingId={1} label="Add a ticket for this journey" />);
    fireEvent.click(screen.getByRole('button', { name: 'Add a ticket for this journey' }));
  }

  it('starts collapsed, showing only the entry-point button', () => {
    renderWithMantine(<TicketEntryForm trackingId={1} label="Add a ticket for this journey" />);
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Operator')).not.toBeInTheDocument();
  });

  it('expands into the manual-entry tab by default when opened', () => {
    openForm();
    expect(screen.getByLabelText('Operator')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Manual entry', selected: true })).toBeInTheDocument();
  });

  it('manual submit: on success, saves, collapses, and refreshes the page', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    openForm();
    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(
        '/api/Train/1/tickets',
        expect.objectContaining({ method: 'POST' }),
      );
    });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({ operator: 'LNER', source: 'manual' });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Add a ticket for this journey' })).toBeInTheDocument();
  });

  it('manual submit: on a 401, shows the login prompt and preserves typed fields', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to save this ticket' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login');
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
  });

  it('manual submit: on a 400, shows the backend message inline', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('origin_crs must be a 3-letter CRS code', { status: 400 }));
    openForm();
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));
    expect(await screen.findByText('origin_crs must be a 3-letter CRS code')).toBeInTheDocument();
  });

  it.each([
    [400, "That doesn't look like a valid upload — try again or fill in the form manually"],
    [422, 'could not read this as a train .pkpass: not a zip file'],
    [504, 'That file took too long to read — try a smaller or simpler PDF, or fill in the details manually'],
    [413, 'That file is too large (8 MB limit). Try filling in the details manually'],
    [500, "Couldn't read this file. Try filling in the details manually"],
  ])('pkpass upload: a %i response shows the mapped inline message', async (status, expectedSubstring) => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(status === 422 ? 'could not read this as a train .pkpass: not a zip file' : 'error', { status }),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(screen.getByLabelText('Apple Wallet .pkpass file'), { target: { files: [file] } });

    expect(await screen.findByText(expectedSubstring)).toBeInTheDocument();
    // The manual form must stay reachable regardless of why the upload
    // failed.
    expect(screen.getByRole('button', { name: 'or fill in the details manually' })).toBeInTheDocument();
  });

  it('pkpass upload: on a 200, pre-fills manual fields, marks them auto-filled, and switches to the manual tab', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({
          operator: 'LNER',
          ticketType: null,
          originCrs: 'Kings Cross',
          destinationCrs: 'Edinburgh',
          source: 'pkpass-semantics',
        }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(screen.getByLabelText('Apple Wallet .pkpass file'), { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Manual entry', selected: true })).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
    expect(screen.getByLabelText('Origin CRS code')).toHaveValue('Kings Cross');
    // "Kings Cross" is not a 3-letter CRS code -- the pre-filled value
    // stays editable and is flagged for review, not silently accepted.
    expect(screen.getByText('Auto-filled — please check this is a real 3-letter CRS code')).toBeInTheDocument();
    expect(screen.getByLabelText('Origin CRS code')).not.toBeDisabled();
  });

  it('editing an auto-filled field does not reset source back to manual', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify({ operator: 'LNER', ticketType: null, originCrs: 'Kings Cross', destinationCrs: null, source: 'pkpass-heuristic' }),
        { status: 200 },
      ),
    );
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(screen.getByLabelText('Apple Wallet .pkpass file'), { target: { files: [file] } });
    await screen.findByLabelText('Origin CRS code');

    // Correct the auto-filled station name into a real CRS code -- this is
    // exactly the review-before-save edit the CRS-format check exists to
    // force.
    fireEvent.change(screen.getByLabelText('Origin CRS code'), { target: { value: 'KGX' } });

    vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      const [, init] = vi.mocked(fetch).mock.calls.at(-1)!;
      const body = JSON.parse((init as RequestInit).body as string);
      expect(body.source).toBe('pkpass-heuristic');
      expect(body.origin_crs).toBe('KGX');
    });
  });

  it('a 401 during upload shows the login prompt, same as the final-submit 401 handling', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(screen.getByLabelText('Apple Wallet .pkpass file'), { target: { files: [file] } });
    expect(await screen.findByRole('link', { name: 'Log in to save this ticket' })).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- TicketEntryForm.test.tsx`
Expected: all tests PASS.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. If Task 5 was implemented first with a placeholder `TicketEntryForm`, delete the placeholder now and confirm `TicketPanel.test.tsx` still passes against the real component.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TicketEntryForm.tsx frontend/components/TicketEntryForm.test.tsx
git commit -m "Add TicketEntryForm: manual entry plus .pkpass/PDF upload preview-and-confirm"
```

---

### Task 7: Wire `TicketPanel` into both existing tracked-train pages

**Files:**
- Modify: `frontend/app/train/by-id/[trackingId]/page.tsx`
- Modify: `frontend/app/train/[uid]/[date]/page.tsx`

**Interfaces:**
- Consumes: `TicketPanel` (Task 5).
- Produces: both pages rendering `<TicketPanel trackingId={state.id} />` directly below their existing `<TrainJourney state={state} />`, per the design spec's own Architecture diagram. `state.id` (the tracking id) is present and correct on both page variants regardless of which lookup resolved the state — no bespoke plumbing needed on either page.

- [ ] **Step 1: Modify `frontend/app/train/by-id/[trackingId]/page.tsx`**

Add the import and render call:

```tsx
import { TicketPanel } from '@/components/TicketPanel';
```

Replace:

```tsx
      <TrainJourney state={state} />
      {/* A same-page nudge, not an automatic redirect -- Decision 2's
```

with:

```tsx
      <TrainJourney state={state} />
      <TicketPanel trackingId={state.id} />
      {/* A same-page nudge, not an automatic redirect -- Decision 2's
```

(This is the train-tracking-frontend spec's own Decision 2 comment already in the file, referring to the canonical-link nudge below it — unaffected, kept as-is; `TicketPanel` is inserted directly above it.)

- [ ] **Step 2: Modify `frontend/app/train/[uid]/[date]/page.tsx`**

Add the import and render call:

```tsx
import { TicketPanel } from '@/components/TicketPanel';
```

Replace:

```tsx
      <TrainJourney state={state} />
    </Stack>
  );
}
```

with:

```tsx
      <TrainJourney state={state} />
      <TicketPanel trackingId={state.id} />
    </Stack>
  );
}
```

No colocated test file for either page — matching this codebase's existing convention (neither page has one today; their sub-components carry the test coverage, which `TrainJourney.test.tsx` already does for journey rendering and `TicketPanel.test.tsx`/`TicketEntryForm.test.tsx` (Tasks 5–6) now do for the ticket section).

- [ ] **Step 3: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add "frontend/app/train/by-id/[trackingId]/page.tsx" "frontend/app/train/[uid]/[date]/page.tsx"
git commit -m "Render TicketPanel on both tracked-train pages"
```

---

### Task 8: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, no regressions anywhere in the frontend.

- [ ] **Step 2: Confirm the proxy fix didn't regress any existing mutation caller**

Run (from `frontend/`): `npm test -- route.test.ts api.test.ts PinToggle.test.tsx TrackTrainForm.test.tsx`
Expected: all PASS — the single riskiest change in this plan (Task 1, shared infrastructure) re-verified in isolation against every existing consumer, not just the new one.

- [ ] **Step 3: Re-confirm, by grep, that mutation routes are only ever called from the proxy path, never from `lib/api.ts`**

```bash
grep -n "tickets" frontend/lib/api.ts
```

Expected: only `getTicketsForTrackedTrain`/`getDelayRepayEstimate` (both `GET`s) appear — no `POST` to any `.../tickets` variant anywhere in `lib/api.ts`. This is the literal, checkable form of this plan's own Global Constraint that every mutation goes through `TicketEntryForm`'s `fetch('/api/Train/...')` calls, never through the server-only client.

- [ ] **Step 4: Re-confirm the disclaimer/claim-link safety property by grep, not just by memory**

```bash
grep -n "disclaimer" frontend/components/DelayRepayEstimate.tsx
```

Expected: `response.disclaimer` (the top-level field) is read and rendered; `estimate.disclaimer` never appears as a separate render call anywhere in the file.

- [ ] **Step 5: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree.

---

## Explicitly out of scope for this plan (carried forward from the spec, not resolved here)

Per the spec's own "Explicitly out of scope" and "Open questions / risks" sections — none of the following is invented or silently decided by any task above:

- Editing or deleting a saved ticket — no `PUT`/`DELETE` route exists anywhere in the ticket family.
- A "my tickets across all tracked trains" view — no backend route supports this query.
- Client-side file-size/type pre-validation beyond what the backend enforces (the `8 MiB` cap and format checks are all server-side) — a nice-to-have, not designed or built here.
- Any UI implying this app can submit a claim or prove travel — a hard constraint every task above is built around, not a gap to fill later.
- Widening `AutoRefresh` for the same finished-journey staleness case the train-tracking-frontend plan already declined to solve.
- Real-world hit rate of the `.pkpass` `semantics` vs. positional-fallback path, and of the PDF regex heuristic — unconfirmed against real sample tickets by either the backend plan or this one; `TicketEntryForm`'s "auto-filled, please check" copy is written to be honest regardless of the real hit rate, but this plan cannot predict how often a user sees an empty vs. populated pre-fill.

## Self-review notes

- **Spec coverage:** entry point/ownership gating (Task 5), the three-tier entry flow (Task 6), Delay Repay rendering rules and safety-critical disclaimer/link handling (Task 4), auth UX at both the read gate and each mutation point (Tasks 5–6), the proxy binary-safety fix (Task 1), the type/API contract (Tasks 2–3), page wiring (Task 7), and the testing convention (every task) are each covered by exactly one task above.
- **The one real gap in the spec's own hand-written contract** — `getTicketsForTrackedTrain` collapsing `401`/`404` into one `null` despite Decision 1 needing to tell them apart — is resolved by composition with the existing `getSession()` in Task 5, flagged both in this plan's own header section and inline in the affected code's doc comments, not silently decided.
- **Type consistency check:** `TrackedTrainTicket`/`TicketEntryRequest`/`PartialTicket`/`DelayRepayEstimate(Response)`/`TicketSource` (Task 2) are used with identical field names in Task 3 (`lib/api.ts`), Task 4 (`DelayRepayEstimate`), Task 5 (`TicketPanel`), and Task 6 (`TicketEntryForm`). `getTicketsForTrackedTrain`/`getDelayRepayEstimate` (Task 3) are called with matching signatures in Task 5. `TicketEntryForm`'s props (`trackingId`, `label`) match how Task 5 calls it in both of its two call sites.
