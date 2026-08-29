# Design: Train Tracking Frontend

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-07-07-frontend-design.md` (the precedent this
doc follows in structure) and
`docs/superpowers/specs/2026-08-28-train-tracking-design.md` (the backend
design this doc is the deferred frontend follow-up to — see that doc's
"Frontend" section and its "Frontend UI design" non-goal). No
implementation plan is included; that is a separate, later step in this
repo's process.

## Goal

Individual train tracking has a complete, working backend
(`crates/api/src/routes/train.rs`, `crates/trust-consumer/`) and **zero
user-facing surface** — confirmed by grepping the whole `frontend/` tree
for "train"/"journey" and finding no matches anywhere outside this doc.
A user cannot create a tracking pin, cannot view a tracked train's journey,
and has no way to discover the feature exists. This spec designs that
missing frontend: where a user starts tracking a train, what the tracking
page shows, how the required-login/no-login-required split in the backend
surfaces in the UI, and how the page stays live.

## Corrections to the brief's assumptions (recorded for posterity)

Following `2026-07-07-frontend-design.md`'s own "Naming correction"
precedent: this section exists because the brief handed to this design
pass assumed two things about the current frontend that turned out, on
direct inspection of the code, not to be true. Both materially change the
design below, so they're recorded rather than silently worked around.

1. **`/stations/[crs]` is not a departure board.** The brief describes it
   as "the existing per-station departure UI" and expects a "track this
   train" action on an individual departure row. Reading
   `frontend/app/stations/[crs]/page.tsx` shows it renders
   `getStopPointDisruption(crs)` — a list of **lines** serving that
   station and their current aggregate `LineStatus` (severity, sample
   stats, disruption text) — not a list of individual trains. There is no
   component anywhere in `frontend/` that renders one row per
   `StationDeparture` (service id, scheduled/estimated time, headcode).
   The backend design doc's own sketch ("reachable from a 'track this
   train' action on the existing per-station departure UI") describes UI
   that does not exist.
2. **No public API exposes individual departures at all**, so that UI
   couldn't be built today even if this spec wanted to. `station_samples`
   (`StationDeparture[]` per CRS, written by `poller-ldbws`) is read via
   `crates/api/src/data/queries.rs::latest_station_sample`, but the only
   caller of that function is `crates/api/src/routes/train.rs`'s own
   `blend_darwin_eta` — an internal read for the ETA overlay, never
   returned to a client. `crates/api/src/routes/mod.rs`'s route list has
   no `GET .../Departures`-shaped endpoint; `ingest.rs`'s
   `/station-samples` route is POST-only and sits behind
   `private_router()`'s `X-Internal-Token` gate, unreachable from a
   browser.

Practical consequence: this spec cannot make "click an individual
departure to track it" the v1 entry point, because the data it would
render doesn't reach the frontend today. See **Entry points** below for
what v1 does instead, and **Explicitly out of scope** for the new
public-departures endpoint this would need as a fast-follow.

A third, smaller correction: the brief describes
`frontend/app/api/[...path]/route.ts` as ready to carry the tracking
POST because it "handles cookie forwarding for authenticated routes."
That's true of its cookie handling, but its path-scoping is narrower than
the brief implies — see **Auth UX** below for why it needs a one-line
extension, not just reuse as-is.

## Current relevant state (verified 2026-08-29)

**Backend (`crates/api`):**

- `POST /Train/track` (`crates/api/src/routes/train.rs:24`) — requires
  `AuthenticatedUser` (an unauthenticated request gets a bare
  `StatusCode::UNAUTHORIZED`, no JSON body — confirmed in
  `crates/api/src/auth.rs`'s `FromRequestParts` impl, `Err(StatusCode::UNAUTHORIZED)`
  at line 34, same shape `PinToggle.tsx` already special-cases). Body is
  `common::TrackPinRequest` (`crates/common/src/lib.rs:514-522`):
  `serviceDate` (date), `originCrs` (must be exactly 3 letters, checked
  server-side by `train_tracking::validate_pin`), `scheduledDeparture`
  (ISO datetime, must be within `MAX_PIN_AGE` = 6 hours in the past, or
  any time in the future — `crates/api/src/data/train_tracking.rs:20-32`),
  optional `destinationCrs`, optional `operator`. Returns
  `{trackingId: number, resolutionStatus: "pending"}` (always `"pending"`
  literally — `crates/api/src/routes/train.rs:47`).
- `GET /Train/{tracking_id}` and `GET /Train/by-uid/{train_uid}/{date}`
  (`train.rs:25-26`) — both public, both return
  `train_tracking::TrackedTrainState`, both 404 (`{status, body: string}`,
  not JSON) when no matching row exists. **Both routes are merged directly
  onto the root router in `crates/api/src/main.rs:59`
  (`.merge(routes::train::router())`), the same way `line_status::router()`
  is (`main.rs:58`) — not nested under `/public` the way
  `preferences`/`reference`/`auth`/`lines` are (`main.rs:60`,
  `.nest("/public", routes::public_router())`).** This matters for the
  frontend proxy — see Auth UX.
- `TrackedTrainState` (`crates/api/src/data/train_tracking.rs:191-208`,
  camelCase on the wire):
  ```
  id: number
  serviceDate: string            // NaiveDate, "YYYY-MM-DD"
  pinOriginCrs: string
  pinDestinationCrs: string | null
  resolutionStatus: string       // see enum below
  trainUid: string | null
  trainId: string | null
  status: string | null          // see enum below; null until resolved
  lastReportedLocation: string | null
  lastEventType: string | null   // "ARRIVAL" | "DEPARTURE" | "PASS", or null
  delayMinutes: number | null
  nextCallingPoint: string | null
  etaNext: string | null         // DateTime<Utc>, RFC3339
  etaSource: string | null       // see enum below
  ```
- **`resolutionStatus` is a closed, fully-confirmed 3-value enum** — not
  just observed in code, but enforced by a Postgres `CHECK` constraint:
  `'pending' | 'resolved' | 'unresolved'`
  (`crates/api/migrations/20260828120000_train_tracking.sql:71-72`).
  `'pending'`: just created, no `train_uid` yet.  `'resolved'`: bound to a
  TRUST Activation, `trainUid`/`trainId` populated.  `'unresolved'`: no
  Activation was ever matched — and this is **terminal**, confirmed by
  `crates/trust-consumer/src/process.rs:35`'s own comment: "it stays that
  way indefinitely: nothing re-attempts the match."
- **`status` (journey status) is also a closed, fully-confirmed 4-value
  enum**, also `CHECK`-constrained:
  `'awaiting_activation' | 'en_route' | 'cancelled' | 'completed'`
  (`train_tracking.sql:122-123`, matching
  `crates/trust-consumer/src/journey.rs:10`'s `DerivedState.status`
  doc comment exactly). `status` is `null` only while `resolutionStatus`
  is still `'pending'`/`'unresolved'` (no `train_current_state` row
  exists yet — the `LEFT JOIN` in `TRACKED_TRAIN_STATE_SELECT`,
  `train_tracking.rs:210-216`, yields `NULL`s).
  **Caveat found while reading `journey.rs` closely, not to be glossed
  over: `'completed'` is declared in the schema and the type comment, but
  no code path in `crates/trust-consumer/src/journey.rs` ever produces
  it.** `apply_movement` (`journey.rs:28-46`) always sets `'en_route'`
  regardless of `event_type`, with an explicit comment: *""PASS" doesn't
  complete the journey; only the last scheduled location's
  ARRIVAL/DEPARTURE would, and this crate has no scheduled calling-point
  list to know which location is "last"... status stays en_route
  regardless of event_type until an explicit Cancellation ends it. A
  future CIF-backed pass is the natural place to add real completion
  detection."* So today, a train that finishes its journey normally (not
  cancelled) will sit at `status: "en_route"` indefinitely, with whatever
  `nextCallingPoint` was last written (which may now be stale/already
  passed) — there is no current signal that distinguishes "still running"
  from "finished, nothing further will arrive." See **Open questions**.
- `etaSource` is `'trust-propagated' | 'darwin-estimated'`
  (`train_tracking.sql:136` `CHECK`, matching
  `common::TrainMovementEventMessage`'s doc comment,
  `crates/common/src/lib.rs:573`) whenever `etaNext` is non-null.
  `'trust-propagated'`: naive forward delay propagation, written by
  `trust-consumer`. `'darwin-estimated'`: a same-request-only overlay from
  a live Darwin/LDBWS sample at the train's origin station, applied by
  `blend_darwin_eta` (`train.rs:86-100`) and never written back to the
  database — it can appear on one response and be absent (falling back to
  `'trust-propagated'`) on the next, if the underlying Darwin sample
  changes or a matching departure stops being visible on the board. The
  overlay's own matching logic (`crates/api/src/data/eta_blend.rs`) only
  yields a concrete ETA for an `"HH:MM"`-shaped Darwin estimate on a
  non-cancelled matching departure — a `"On time"` Darwin string
  deliberately yields nothing rather than fabricating a time from the
  schedule (`eta_blend.rs:148-152`'s own test says so explicitly).
- Attribution: `frontend/components/OpenDataAttribution.tsx` **already**
  carries the required, distinct-from-NRE, unbranded Network Rail line —
  *"Live train movement data from Network Rail's open data feeds"*
  (`OpenDataAttribution.tsx:69-71`), rendered in the root layout's footer
  on every page already. This spec adds no attribution work; it must not
  duplicate or replace this line.

**Frontend conventions this spec reuses (verified in the current tree):**

- Next.js App Router + TypeScript + Mantine v9 (incl. `@mantine/dates`,
  confirmed actually used — `DatePickerInput` in
  `frontend/app/lines/[id]/history/HistoryRangePicker.tsx:5` — not just
  installed).
- `frontend/lib/api.ts` — server-side fetch helpers, one per read
  endpoint, each hitting `${API_BASE_URL}` directly at whatever path the
  backend actually mounts it at (`getStopPointDisruption` calls
  `${baseUrl()}/StopPoint/${crs}/Disruption` — the bare root path, *not*
  `/public/...`, exactly matching where `/Train/...` is mounted). Errors
  become `ApiNotFoundError` on a 404, letting the calling page call
  `notFound()`.
- `frontend/app/api/[...path]/route.ts` — the browser-facing proxy for
  *mutations* from Client Components (`PinToggle.tsx` uses it for
  `PUT /api/preferences/...`). **Scoped specifically to `/public/*` on the
  backend**: it builds the target as
  `` `${API_BASE_URL}/public/${path.join('/')}` `` and 400s if the
  resolved pathname doesn't start with `/public/` (route.ts:35-38) — a
  deliberate traversal guard, not an incidental detail. It forwards
  `Cookie` in, `Set-Cookie` out, and preserves 3xx redirects unfollowed
  (needed for the OIDC login/callback flow it also carries).
- `frontend/components/PinToggle.tsx` — the precedent for "this mutation
  needs login": a `needsLogin` boolean, set when the proxied request comes
  back 401 (never parsed as JSON on 401 — the backend doesn't send a JSON
  body for it), rendered as an inline `<TextLink href="/api/auth/login">`
  prompt next to the disabled control, not a separate error page.
- `frontend/components/AuthStatus.tsx` / `frontend/lib/api.ts`'s
  `getSession()` — reads `SessionInfo` (`authenticated`, nullable
  `id`/`email`/`name`) via a cookie-forwarding server fetch to
  `/public/auth/session`, which never 401s itself.
- `frontend/components/AutoRefresh.tsx` — a side-effect-only Client
  Component mounted once in `app/layout.tsx`, calling `router.refresh()`
  every 30s via `useInterval`, applied globally to every route (no
  per-route opt-out mechanism exists today).
- Route-segment conventions: one root `app/error.tsx`; per-segment
  `not-found.tsx` exists for `app/lines/[id]/` and
  `app/stations/[crs]/`, triggered by catching `ApiNotFoundError` and
  calling `notFound()`.
- Colocated `*.test.tsx` with `@testing-library/react` and a
  `renderWithMantine` helper (`frontend/test/render.tsx`).

## Decisions

### 1. Entry points: a manual tracking form is the v1 entry point, not a departure-row action

Per the corrections above, no UI or API surface exists today to list
individual departures, so "click a departure to track it" isn't buildable
in this pass. v1 instead ships:

- **A new top-level nav link**, "Track a Train", next to the existing
  "Station Lookup" link in `app/layout.tsx`'s nav (`TextLink href="/track"`
  alongside the existing `<TextLink href="/stations">Station Lookup</TextLink>`
  at `layout.tsx:99`), pointing at a new form page.
- **A form page at `/track`** (`app/track/page.tsx`), mirroring the
  existing `/stations` search-page pattern (`app/stations/page.tsx`
  renders a heading + a client form component). Fields, matching
  `TrackPinRequest` exactly: Origin CRS (text input, 3-letter validation
  client-side, matching the pattern `/stations/[crs]/page.tsx` already
  uses — `CRS_PATTERN = /^[A-Za-z]{3}$/`), Date + scheduled departure time
  (a single `@mantine/dates` `DateTimePicker`, following the precedent of
  `DatePickerInput` already used on the history page), optional
  Destination CRS, optional Operator. Client-side hint text for the
  6-hour-past/any-future validity window backend enforces
  (`validate_pin`), so a rejection is rare rather than the first time the
  user learns about it.
- **A weaker, honest version of "reachable from the station page":** a
  single `TextLink` on `/stations/[crs]/page.tsx`, near the existing
  `PinToggle` in that page's header `Group`, reading "Track a train from
  here" and linking to `/track?origin={crs}` — the form page reads
  `origin` from `searchParams` and pre-fills the Origin CRS field. This is
  the closest honest equivalent to the backend design doc's sketch given
  what's actually rendered there today: it's a shortcut into the manual
  form, pre-scoped to the station the user was already looking at, not a
  per-departure action (there is no per-departure row to attach one to).
- On successful submission, redirect to `/train/by-id/{trackingId}` (see
  URL shapes below) so the user immediately sees their new pin's
  (pending) state.

The richer per-departure entry point (a `GET .../Departures` public
endpoint plus a real departure-board component with a "Track" button per
row) is recommended future work — see **Explicitly out of scope**.

### 2. Tracking page URL shapes: two routes, one per backend lookup key

The backend design doc only sketched `/Train/{uid}/{date}` at the
*backend* level, and explicitly left the frontend URL open. This app's
existing frontend routes are already lowercase and don't mirror the
backend's PascalCase TfL-style paths verbatim (`/stations/[crs]` vs.
`GET /StopPoint/{crs}/Disruption`; `/lines/[id]` vs.
`GET /Line/{id}/Status`) — different servers, different origins, so there
is no literal routing collision either way, but following the existing
lowercase convention rather than reusing `/Train/...` verbatim keeps this
consistent with every other page in the app:

- **`/train/by-id/[trackingId]`** — looked up via
  `GET /Train/{tracking_id}`. This is the only URL reachable immediately
  after creating a pin, since the tracking id is the only identifier the
  create response returns; `train_uid`/`service_date` aren't known yet
  while `resolutionStatus` is still `'pending'`.
- **`/train/[uid]/[date]`** — looked up via
  `GET /Train/by-uid/{train_uid}/{date}`. This is the canonical,
  shareable/bookmarkable URL once a pin has resolved — it names the real
  service (`train_uid` + `service_date`), not this particular user's pin
  row, so two different users' pins on the same real train converge on
  the same link.
- Once `/train/by-id/[trackingId]` observes `resolutionStatus: "resolved"`
  with a non-null `trainUid`, it renders a `TextLink` "View the canonical
  link for this train" to `/train/{trainUid}/{serviceDate}` — a
  same-page nudge, not an automatic redirect (an automatic redirect would
  silently break "I bookmarked the URL right after tracking, before it
  resolved" for a user who didn't want to wait).
- A non-numeric `trackingId` segment, or an id/uid+date pair the backend
  404s, both call `notFound()` via `ApiNotFoundError`, rendering a new
  `app/train/by-id/[trackingId]/not-found.tsx` /
  `app/train/[uid]/[date]/not-found.tsx`, following the existing
  `lines/[id]/not-found.tsx` / `stations/[crs]/not-found.tsx` pattern.

### 3. What the tracking page renders, by state

Both URL variants render through one shared presentational component
(`components/TrainJourney.tsx`) taking a `TrackedTrainState`, so the two
page files differ only in which `lib/api.ts` fetch they call. State
branches, using the fully-enumerated values confirmed above:

| `resolutionStatus` | `status` | Rendered |
|---|---|---|
| `pending` | (always null) | "Waiting to hear from Network Rail" panel: origin/destination/scheduled time as pinned, a `Loader` or static waiting graphic, no journey timeline. No claim about *when* this will resolve — see Open questions on real-world Activation timing, which this research pass could not confirm. |
| `unresolved` | (always null) | Terminal "Couldn't be matched to a live service" panel — phrased as final, not "still trying," matching `process.rs`'s confirmed no-retry behavior. Still shows the original pin criteria (origin/date/time) for reference. |
| `resolved` | `awaiting_activation` | "Matched to train {trainUid} — waiting for its first movement report" — `trainUid` now shown, but no location/delay data yet (no `train_current_state` row's movement fields populated beyond the defaults). |
| `resolved` | `en_route` | The main view: last reported location + event type (arrival/departure/pass) + delay in minutes, next calling point, ETA with a visibly distinct treatment for `etaSource` (`trust-propagated` vs `darwin-estimated` — badge/tooltip, not collapsed into one number, mirroring `StatusBadge`'s existing severity-badge pattern and directly extending this app's established `dataQuality` provenance-surfacing philosophy per the backend design doc's ETA section). **Given the `'completed'` gap above:** if `nextCallingPoint` is null while `status` is still `en_route`, render a soft "no further calling points reported — this journey may have finished" note rather than claiming a real "Completed" state the backend doesn't actually assert. This is presented as a heuristic inference, visually distinct from the `cancelled` case below, not as equivalent certainty. |
| `resolved` | `cancelled` | Clear cancelled banner, last known location/event retained and shown (matches `apply_cancellation`'s explicit preservation of prior state, confirmed in `journey.rs:48-50` and its own test `cancellation_preserves_last_known_location`). |
| `resolved` | `completed` | Same "arrived" treatment described in the `en_route`+no-next-stop row above, kept as a real branch in the component even though no current backend code path produces it — cheap to keep and turns from dead code into forward-compatible code the day `journey.rs` gets real completion detection (flagged in that file's own comment as a "future...pass"). |

No journey **timeline** (a full stop-by-stop list) is renderable from
`TrackedTrainState` alone — that struct is the denormalized *current*
state row only (`last_reported_location`, not a list). A real timeline
needs `train_movement_events`, which has no public read route today
(only written internally by `upsert_train_event`, and read back nowhere).
**This spec descopes the "journey timeline" part of the backend design
doc's UI sketch** to "current state only" for v1, and flags the missing
events-list endpoint as necessary follow-up work — see **Explicitly out
of scope**.

### 4. Auth UX: extend the existing proxy, follow `PinToggle`'s login prompt exactly

The tracking-creation form is a Client Component (it needs interactive
validation and a `needsLogin` state, matching `PinToggle`'s pattern per
the brief's explicit instruction to reuse it) submitting
`POST /Train/track`. Per the corrections section, the existing
`app/api/[...path]/route.ts` proxy cannot reach it as written — `/Train/*`
is mounted at the bare root on `api`, not under `/public`, and the proxy
hard-checks for a resolved `/public/` prefix.

**Decision: widen the proxy's allowlist from a single fixed `/public/`
prefix to an explicit short list of allowed backend prefixes, `['public/',
'Train/']`**, rather than adding a second bespoke proxy route file. This
keeps the one place that does cookie-forwarding + redirect-passthrough +
traversal-safe path resolution single-sourced (avoiding the drift risk of
a hand-copied second proxy), while keeping the widened surface
minimal and explicit — not a general "forward anything" passthrough. The
traversal check (verifying the *resolved* `URL`'s pathname, not the raw
segments) is unchanged in kind, just checked against either allowed
prefix instead of one.

Flow, mirroring `PinToggle.tsx` exactly:

1. Form submit calls `fetch('/api/Train/track', { method: 'POST', ... })`.
2. `200` → parse `{trackingId, resolutionStatus}`, `router.push` to
   `/train/by-id/{trackingId}`.
3. `401` → set `needsLogin` (same flag name/shape as `PinToggle`), render
   the same inline `<TextLink href="/api/auth/login">Log in to track this
   train</TextLink>` pattern next to the (not-yet-submitted) form — the
   user's already-typed field values are preserved (no navigation away),
   unlike `PinToggle`'s toggle-and-forget click, since a form has real
   input to protect from being silently discarded. After logging in and
   returning, the user re-submits manually (no attempt to auto-resubmit
   across the OIDC redirect round-trip — that would need persisting form
   state across a full-page navigation, which nothing else in this app
   does today and isn't justified for a four-field form).
4. `400` (validation failure, e.g. stale `scheduledDeparture`) → render
   the server's plain-text error message inline, same tier as any other
   client-side field error.
5. Anything else (`500`, network failure) → a generic "couldn't create
   the tracking pin, try again" message — this app's existing tolerance
   for "let the specific case be handled, everything else gets a generic
   fallback" (`fetchJson`'s `errorForResponse`, `PinToggle`'s unconditional
   `return` on any non-ok PUT).

Reads (`GET /train/by-id/...`, `GET /train/[uid]/[date]/...`) need **no**
proxy involvement at all — they're public/unauthenticated on the backend,
so both new page files call `lib/api.ts` server-side, hitting
`${API_BASE_URL}/Train/...` directly, exactly like
`getStopPointDisruption` already does for the (also root-mounted, also
public) `/StopPoint/...` route. Two new `lib/api.ts` functions:
`getTrackedTrainById(id: number)` and
`getTrackedTrainByUidAndDate(uid: string, date: string)`, both
`cache: 'no-store'` (matching every other live-data fetch in that file),
both throwing `ApiNotFoundError` on a 404 for the pages' `notFound()`
handling.

### 5. Data refresh: reuse the existing global `AutoRefresh`, don't build a second mechanism

`AutoRefresh` already runs on every route via the root layout, refreshing
every 30s using `cache: 'no-store'` Server Component fetches — exactly the
fetch mode the two new `lib/api.ts` functions above use. **No new
refresh mechanism is needed**; the tracking pages get live updates for
free the same way every other dynamic page in this app does, by simply
being a normal Server Component page with `no-store` fetches.

One tension acknowledged, not solved: `AutoRefresh` has no per-route
opt-out, so a `cancelled`/genuinely-finished tracking page keeps
re-fetching every 30s even though its data will never change again. This
spec accepts that as-is for v1 — it's the same blunt-but-simple posture
this app already has everywhere else (no page currently pauses
`AutoRefresh` for its own "nothing left to change" cases either, e.g. a
fully-elapsed history range), and a per-route refresh override would be a
real `AutoRefresh` API change affecting every existing page, not a
train-tracking-scoped decision to make unilaterally in this doc. Flagged
under Open questions if it's worth revisiting once real usage exists.

A `pending`-state-specific manual "Check now" button was considered and
rejected for v1: since real-world Activation timing relative to scheduled
departure isn't confirmed anywhere in this codebase's research (see Open
questions), a tighter poll wouldn't reliably make the wait feel shorter,
only add a control whose usefulness can't be verified from what's known
today. The global 30s refresh already covers it without extra surface.

## API/type contract

Hand-written, matching the verified Rust shapes above (not generated —
consistent with `frontend/lib/types.ts`'s existing convention):

```ts
// frontend/lib/types.ts additions

export type ResolutionStatus = 'pending' | 'resolved' | 'unresolved';
export type JourneyStatus = 'awaiting_activation' | 'en_route' | 'cancelled' | 'completed';
export type EtaSource = 'trust-propagated' | 'darwin-estimated';

export interface TrackedTrainState {
  id: number;
  serviceDate: string;              // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  trainId: string | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null;     // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null;           // RFC3339
  etaSource: EtaSource | null;
}

export interface TrackPinRequest {
  serviceDate: string;              // "YYYY-MM-DD"
  originCrs: string;
  scheduledDeparture: string;       // RFC3339
  destinationCrs?: string;
  operator?: string;
}

export interface TrackPinResponse {
  trackingId: number;
  resolutionStatus: 'pending';
}
```

```ts
// frontend/lib/api.ts additions

export async function getTrackedTrainById(id: number): Promise<TrackedTrainState> {
  return fetchJson<TrackedTrainState>(`${baseUrl()}/Train/${id}`, { cache: 'no-store' });
}

export async function getTrackedTrainByUidAndDate(
  uid: string,
  date: string,
): Promise<TrackedTrainState> {
  return fetchJson<TrackedTrainState>(`${baseUrl()}/Train/by-uid/${uid}/${date}`, {
    cache: 'no-store',
  });
}
```

`POST /Train/track` itself is called only from the Client Component form,
via `fetch('/api/Train/track', ...)` through the widened proxy — it does
not go through `lib/api.ts` at all, matching `PinToggle.tsx`'s existing
split (server-only reads in `lib/api.ts`, browser-initiated mutations via
`/api/*`).

## Architecture

```
┌───────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                     │
│                                                                       │
│  app/track/page.tsx              "Track a train" form (Client Comp) │
│  app/train/by-id/[trackingId]/page.tsx     GET /Train/{id}          │
│  app/train/by-id/[trackingId]/not-found.tsx                         │
│  app/train/[uid]/[date]/page.tsx           GET /Train/by-uid/{uid}/{date}
│  app/train/[uid]/[date]/not-found.tsx                               │
│                                                                       │
│  components/TrainJourney.tsx     shared state-branch renderer        │
│  components/TrackTrainForm.tsx   Client Comp, needsLogin pattern     │
│  components/EtaBadge.tsx         trust-propagated vs darwin-estimated│
│                                                                       │
│  lib/api.ts   + getTrackedTrainById, getTrackedTrainByUidAndDate     │
│  lib/types.ts + TrackedTrainState, TrackPinRequest, TrackPinResponse │
│                                                                       │
│  app/api/[...path]/route.ts   allowlist widened: ['public/','Train/']│
└──────────────────────────┬──────────────────────┬───────────────────┘
        server-side fetch  │                       │ browser fetch,
        (reads, no-store)  │                       │ via /api/Train/track
                            ▼                       ▼
              ┌─────────────────────────────────────────┐
              │ api crate (existing, no backend changes  │
              │ needed for this spec)                    │
              │  GET  /Train/{tracking_id}                │
              │  GET  /Train/by-uid/{train_uid}/{date}     │
              │  POST /Train/track   (needs session cookie)│
              └─────────────────────────────────────────┘
```

## Error handling

- Unknown `trackingId` / unresolved `(uid, date)` pair → backend 404 →
  `ApiNotFoundError` → `notFound()` → the new route-specific
  `not-found.tsx`, same pattern as `lines/[id]` and `stations/[crs]`.
- API unreachable/5xx on either read → no dedicated `error.tsx` for this
  segment; falls through to the existing root `app/error.tsx`, same as
  every other page that has no segment-specific one today (e.g.
  `app/lines/[id]/history`).
- Malformed URL segments (a non-numeric `trackingId`, a `date` segment
  that doesn't parse) → validated before the fetch fires, calling
  `notFound()` directly rather than letting a malformed request reach the
  backend and rely on its error shape.
- Tracking-creation errors → handled entirely within the form component
  per the Auth UX flow above (401 / 400 / generic), never a route-level
  crash.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest):

- `lib/types.ts`/`lib/api.ts`: unit tests for `getTrackedTrainById` /
  `getTrackedTrainByUidAndDate` URL construction and 404→`ApiNotFoundError`
  mapping, mirroring the existing tests for `getLineStatus` etc.
- `components/TrainJourney.tsx`: render tests for every row of the
  state table above (`pending`, `unresolved`, `resolved`+each of the four
  `status` values including the no-`nextCallingPoint` heuristic branch),
  confirming the right message/data appears and that `etaSource` renders
  visibly distinctly per value.
- `components/TrackTrainForm.tsx`: render/interaction tests for the
  three-way submit outcome (success → redirect call, 401 → `needsLogin`
  prompt appears with fields preserved, 400 → inline error text),
  mirroring `PinToggle.test.tsx`'s existing 401-path test shape.
- `app/api/[...path]/route.ts`: extend its existing test coverage (if
  any exists today — verify at planning time) to cover the widened
  allowlist: a `/api/Train/track` request forwards to
  `${API_BASE_URL}/Train/track` with cookies attached, and a path outside
  both allowed prefixes still 400s.

## Explicitly out of scope for this spec

- **A real per-departure "track this train" action**, i.e. the backend
  design doc's original sketch. Blocked on a new public backend read
  endpoint exposing `station_samples`/`StationDeparture[]` (e.g.
  `GET /StopPoint/{crs}/Departures`, reusing the already-written
  `latest_station_sample` query function, currently internal-only) plus a
  real departure-board component on `/stations/[crs]`. Both are backend
  and frontend work beyond a spec-only pass; recommended as the natural
  fast-follow once this v1 form-based entry point ships and usage
  patterns are known.
- **A full journey timeline** (stop-by-stop history, not just current
  state). Blocked on a public read endpoint over `train_movement_events`,
  which has no route today (write-only, via `upsert_train_event`,
  triggered by `trust-consumer`'s internal POST). `TrackedTrainState`
  alone cannot render this.
- **Widening `AutoRefresh` with a per-route opt-out or interval**, for the
  `cancelled`/finished-journey staleness case noted under Decision 5.
  A real change to a component every other page also depends on;
  out of scope for a train-tracking-scoped design.
- **Persisting/auto-resubmitting form state across the OIDC login
  redirect round-trip.** Nothing else in this app does this; the 401 flow
  here is "show the prompt, let the user manually resubmit," matching
  `PinToggle`'s posture of leaving state visibly unsaved rather than
  building a resume mechanism.
- **A "my tracked trains" list page** (browsing a user's own pins,
  analogous to `pinnedLines`/`pinnedStations` in `Preferences`). The
  backend has no such read route today either (`list_active_tracked_trains`
  is `trust-consumer`-facing only, unauthenticated-caller-scoped, and
  lives behind the private router) — a real feature, not sketched by the
  backend design doc, and not designed here.
- Legal sign-off on the exact Network Rail attribution wording — already
  flagged as outstanding in `OpenDataAttribution.tsx`'s own TODO comment
  and the backend design doc's Open Questions; this spec doesn't touch
  that wording, just confirms it exists and shouldn't be duplicated.

## Open questions / risks

1. **Real-world time-to-resolution for a `pending` pin is unknown from
   this codebase alone.** How soon after a pin is created does TRUST's
   Activation message typically arrive relative to a train's scheduled
   departure? This governs how the `pending`-state copy should set user
   expectations ("check back in a few minutes" vs. "check back once the
   train is due to leave") and whether Decision 5's "no manual refresh
   button" call is right. Not resolvable by reading code — needs either
   the backend design doc's own research notes (none found beyond the
   general TRUST message-cadence figures already in that doc, which are
   national-feed volume stats, not per-train Activation lag) or real
   production observation once `trust-consumer` is live.
2. **The `'completed'` status gap** (Decision 3's table): the schema
   allows it, `TrackedTrainState.status` is typed to allow it, but no
   current `trust-consumer` code path emits it. The heuristic fallback
   proposed here (infer "may have finished" from a null
   `nextCallingPoint` while still `en_route`) is a frontend guess around a
   real backend gap, not a confirmed signal — flagged in-page as
   provisional per Decision 3, but worth resolving properly in
   `journey.rs` before this ships, not just papered over in the UI copy.
3. **Whether historical/finished journeys should remain viewable, and for
   how long**, is genuinely unresolved — the backend design doc's own
   Open Questions #7 already flags retention as unresearched ("no
   existing retention policy elsewhere in this repo... the 90-day figure
   is a starting proposal, not a researched one"). This spec doesn't
   invent a frontend-side answer beyond "the page renders whatever the
   read routes currently return" — if/when a prune job starts deleting
   old `tracked_trains`/`train_current_state` rows, a previously-shared
   `/train/[uid]/[date]` link will start 404ing, which this spec's
   `not-found.tsx` already handles gracefully, but the UX of "your old
   link stopped working" isn't otherwise addressed here.
4. **Darwin ETA blend behavior under real load** (cache/latency of
   `latest_station_sample`, how often a `darwin-estimated` overlay is
   actually available vs. falling back to `trust-propagated` in
   practice) is unverifiable by static reading — noted so the `EtaBadge`
   component's design (Decision 3) isn't assumed to see a particular mix
   of the two values in practice.
5. **Whether `/track`'s manual-entry origin CRS should validate against
   the real station reference set** (`getStationName`/`/public/stations`
   type-ahead, already used elsewhere) rather than just a 3-letter regex,
   was not resolved here — likely yes, for consistency with how
   `/stations` search already works, but left as a planning-time detail
   rather than a design-level decision, the same way `2026-07-07`'s doc
   deferred exact Mantine component choices.
