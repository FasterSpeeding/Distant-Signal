# Design: Journey Ticket Tracking Frontend

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md` (the
closest precedent — same team, same session, same backend conventions,
same follow-up relationship to a deferred-frontend backend spec) and, one
level further back, `docs/superpowers/specs/2026-07-07-frontend-design.md`.
No implementation plan is included; that is a separate, later step in this
repo's process.

## Goal

Journey ticket tracking — `.pkpass`/PDF ticket ingestion, manual ticket
entry, and a Delay Repay compensation estimate — has a complete,
safety-reviewed backend
(`crates/api/src/routes/train.rs`, `crates/api/src/data/ticket_extraction.rs`,
`crates/api/src/data/delay_repay_rules.rs`) and **zero user-facing
surface** — confirmed by grepping `frontend/` for `ticket`/`pkpass`/
`delay-repay`/`multipart`/`FormData` and finding no matches anywhere
outside this doc and the design/plan docs it follows. A logged-in user who
has already tracked a train (per
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`) has
no way to record "I had a ticket for this" and no way to see whether
they're likely owed Delay Repay compensation, despite that entire pipeline
already existing and working end to end behind `curl`. This spec designs
that missing frontend: where a user starts attaching a ticket, what the
upload/manual-entry flow looks like including its real failure modes, how
the established login-prompt pattern extends to a session-gated feature
that (unlike train tracking) has **no unauthenticated surface at all**,
and how a Delay Repay estimate is rendered without ever overstating what
this app can do on the user's behalf.

## Corrections to the brief's assumptions (recorded for posterity)

Following `2026-08-29-train-tracking-frontend-design.md`'s own
"Corrections" precedent: this section exists because direct inspection of
the code turned up things the brief (and the backend docs it points to)
didn't establish, materially affecting the design below.

1. **The backend design doc's frontend sketch is one line and is
   misleading about scope.** `2026-08-29-journey-ticket-tracking-design.md`'s
   Architecture section sketches `frontend (upload .pkpass/PDF or fill a
   form)` sending a single `POST /Train/{trackingId}/tickets`. Reading the
   real, merged implementation shows five routes, not one — `POST`/`GET
   .../tickets`, `GET .../tickets/{ticketId}/delay-repay`, `POST
   .../tickets/pkpass`, `POST .../tickets/pdf` — and the upload routes are
   read-only preview endpoints that write nothing; "upload or fill a form"
   undersells that uploading is *never* a save by itself, only a two-step
   preview-then-confirm flow through the same manual-entry endpoint. See
   Current relevant state.
2. **The existing same-origin proxy (`frontend/app/api/[...path]/route.ts`)
   is not safe for file uploads as written, and this is new, load-bearing
   work for this feature, not a detail to wave past.** Its allowlist
   already covers every new route path-wise — everything here sits under
   `/Train/...`, which the proxy already passes through unprefixed (this
   was widened once already, for `POST /Train/track`, by the
   train-tracking-frontend spec). But two things it does today actively
   break a multipart file upload:
   - It hardcodes `'Content-Type': 'application/json'` on every forwarded
     request (`route.ts:60`), which discards the
     `multipart/form-data; boundary=...` header a browser's
     `fetch(url, { body: formData })` sets — axum's `Multipart` extractor
     needs that boundary to parse the file field at all. Forwarding
     `application/json` instead means the backend can't even identify
     where one field ends and the next begins.
   - It reads and re-sends the body via `await req.text()`
     (`route.ts:79`), which decodes the incoming bytes as UTF-8 text
     before re-serializing them. A `.pkpass` (zip) or PDF's raw bytes are
     binary and not valid UTF-8 in general — `.text()` on such a body
     performs *lossy* UTF-8 decoding (invalid byte sequences become
     U+FFFD replacement characters), silently corrupting the file in
     transit. Every upload through this proxy today would arrive at the
     backend already mangled, independent of the Content-Type problem
     above.
   Both are existing behavior of a file this spec's feature must extend,
   not something already fixed elsewhere — see Decision 2.
3. **No file-upload UI exists anywhere in this frontend today.** Confirmed
   by grepping the whole tree for `multipart`, `FormData`, and
   `type="file"` — zero matches. Every other Client Component mutation in
   this app (`PinToggle`, `TrackTrainForm`) sends JSON. This is genuinely
   new interaction surface, not a reuse of an established pattern, unlike
   the rest of this feature (auth UX, refresh, page structure) which
   reuses existing conventions closely.
4. **`TrackedTrainState` never reveals who owns a tracked-train pin, and
   the two existing tracked-train pages render for any visitor.** Both
   `GET /Train/{trackingId}` and `GET /Train/by-uid/{uid}/{date}` are
   public/unauthenticated (confirmed:
   `docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`'s
   own "Current relevant state"), and `TrackedTrainTicket`'s own doc
   comment in `crates/api/src/data/train_tracking.rs` states it "Never
   leaks `user_id`" — same posture as `TrackedTrainState`. This means
   neither existing page can tell, from the state response alone, whether
   the current viewer owns the pin they're looking at — a real constraint
   on how a ticket section can be shown at all. See Decision 1.

## Current relevant state (verified 2026-08-29)

**Backend (`crates/api`)**, all five routes mounted under `/Train/...` on
the root router (same as the existing train-tracking routes, not under
`/public`) — `crates/api/src/routes/train.rs`:

- **`POST /Train/{trackingId}/tickets`** — session-gated
  (`AuthenticatedUser`; unauthenticated → bare `401`, no body, same shape
  as `POST /Train/track`), owner-scoped. Body is `common::TicketEntryRequest`,
  plain snake_case (`crates/common/src/lib.rs:535-546`):
  ```
  { operator?: string, ticket_type?: string, origin_crs?: string,
    destination_crs?: string, source: string }  // source defaults "manual"
  ```
  Server-side validation (`train_tracking::validate_ticket_entry`): `source`
  must be exactly one of `manual | pkpass-semantics | pkpass-heuristic |
  pdf-heuristic` (matching the migration's own `CHECK` constraint);
  `origin_crs`/`destination_crs`, if present, must be exactly 3 characters
  — **this 3-letter check is the actual mechanism forcing a human to
  correct a `.pkpass`/PDF-prefilled station *name* (e.g. "Kings Cross")
  into a real CRS code before anything can be saved**, since neither
  upload format can ever produce one. A `400` (validation) or the
  ownership check failing both return **plain text**, not JSON (the error
  type is `(StatusCode, String)`, matching every other error path in this
  file). Ownership: `tracked_train_owner(pool, trackingId)` must resolve
  to the caller's own `user_id`, else `404` — identical for "tracking id
  belongs to someone else" and "tracking id doesn't exist," per this
  app's established "never `403`" convention
  (`docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md`'s Global
  Constraints). Success returns `Json({ ticketId: number })` with **no
  explicit status set — this is `200 OK`, not `201`** (the handler returns
  `Ok(Json(...))` directly; axum's default `Json<T>` response is `200`).
- **`GET /Train/{trackingId}/tickets`** — same session-gate/ownership-check/
  `404` shape as the `POST` above. Returns `TrackedTrainTicket[]`,
  camelCase, never includes `user_id`:
  ```
  { id: number, trackedTrainId: number, operator: string|null,
    ticketType: string|null, originCrs: string|null,
    destinationCrs: string|null, source: string, createdAt: string }
  ```
  **The `404`-vs-`200 []` split is itself the "is this your pin"
  signal** — a `404` means "not logged in as the owner (or the tracking
  id doesn't exist)"; a `200` with an empty array means "you own this pin,
  you just haven't added a ticket yet." The frontend must treat these as
  semantically different, not both as "no tickets to show." Nothing in
  the schema or route caps a tracked train at one ticket — multiple
  tickets per tracked train are a real, supported case.
- **`GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`** —
  session-gated. Loads the ticket scoped to the caller
  (`get_ticket_owned`), then separately checks the loaded ticket's own
  `trackedTrainId` matches the path's `trackingId` (`404` if not — a real
  ticket id that just isn't under this tracking id). Returns
  `DelayRepayEstimateResponse`, camelCase:
  ```
  {
    delayMinutes: number | null,
    estimate: null | { scheme: "DR15" | "DR30", bandMinutes: number,
                        percentage: number, disclaimer: string },
    claimUrl: string,   // ALWAYS a real, non-empty URL
    disclaimer: string  // ALWAYS populated, independent of `estimate`
  }
  ```
  **This route never returns a bare number with no caveat and no link** —
  `claimUrl` and the top-level `disclaimer` are unconditional. `estimate`
  is `null` whenever *any* of three things is true: the ticket has no
  `operator`, the tracked train has no `delayMinutes` yet, or the delay
  didn't clear the matched scheme's lowest band (e.g. 20 minutes on a
  DR30 operator, or any delay under 15 minutes at all) — **the response
  gives no signal which of the three applied**; see Decision 3 for how
  this is rendered honestly without inventing a reason the API doesn't
  give. **Two different disclaimer strings exist and must not be
  confused** — see Decision 3, which quotes both verbatim.
- **`POST /Train/{trackingId}/tickets/pkpass`** and
  **`POST /Train/{trackingId}/tickets/pdf`** — session-gated (login
  required — no anonymous file-parsing endpoint) but **not**
  ownership-checked against `trackingId`: both handlers' `Path` extraction
  is prefixed `_` (`_tracking_id`) and never touched inside the function —
  the id in the URL is only there for routing symmetry with the other
  ticket routes; the client's own confirm step (the `POST .../tickets`
  above) is what actually needs to be scoped. Both expect
  `multipart/form-data` with exactly one field named `"file"`
  (`read_single_file_field`). Both return `ticket_extraction::PartialTicket`
  on success (camelCase):
  ```
  { operator: string|null, ticketType: string|null,
    originCrs: string|null, destinationCrs: string|null, source: string }
  ```
  `source` is fixed per route, never user-chosen at this stage: the
  `pkpass` route yields `"pkpass-semantics"` (Apple's standardised
  `semantics` dictionary matched) or `"pkpass-heuristic"` (fell back to
  the two-entry `primaryFields` positional convention); the `pdf` route
  always yields `"pdf-heuristic"`. **Every field is independently
  nullable — "this file didn't contain X" is the normal, expected
  outcome for a lot of real uploads, not an error state.** Distinct,
  real failure modes the frontend must design for, not glossed over:
  - `400` — malformed multipart itself (no `"file"` field present, or the
    multipart body itself doesn't parse) — plain text.
  - `422 Unprocessable Entity` — the file was read fine but isn't a valid
    ticket of that type: not a real zip / no `pass.json` inside it / not
    a `PKTransitTypeTrain` boarding pass (pkpass route); missing the
    `%PDF-` magic header, or the third-party PDF-text-extraction call
    itself failed (pdf route). Message body is a human-readable string
    (e.g. `"could not read this as a train .pkpass: ..."`), safe to
    surface directly.
  - `504 Gateway Timeout` — **PDF route only** — text extraction exceeded
    a 10-second wall-clock budget (`PDF_PARSE_TIMEOUT`); the message
    explicitly suggests "try a smaller or simpler file."
  - `500` — the PDF parser panicked internally (untrusted third-party
    crate over malformed bytes).
  - `413` — the whole `/Train/...` router (including these two routes)
    has an `8 MiB` body cap (`DefaultBodyLimit::max`) layered on it; a
    too-large upload gets axum's own `413` before either handler runs.
  **No upload route performs any database write** (verified: neither
  handler's body contains an `sqlx::query`/`INSERT` call) — a `200` here
  is only ever a preview. Turning it into a saved ticket requires a
  second, separate `POST /Train/{trackingId}/tickets` call carrying
  whatever the user reviewed/edited from the preview, `source` set to
  whichever tier produced it.

**Schema** (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql`)
— `tracked_train_tickets`: `id`, `tracked_train_id` (FK →
`tracked_trains`, `ON DELETE CASCADE`), `user_id`, `operator` / `ticket_type`
/ `origin_crs` / `destination_crs` (all nullable `TEXT`), `source`
(`CHECK`-constrained, default `'manual'`), `created_at`. The migration's own
header comment states this table must **never** gain a payment/price,
barcode, ITSO, passenger-name, or uploaded-file column. Two direct
consequences for this spec: (a) there is nothing more to ever render about
a ticket than the six fields listed above — no price, no passenger name,
no barcode; (b) **the uploaded `.pkpass`/PDF file is never retained past
the single preview request**, so the frontend cannot show "here's the file
you uploaded" anywhere after the preview step — only the extracted fields
survive.

**Frontend infra already reusable (confirmed in code):**

- `frontend/app/api/[...path]/route.ts` — the proxy. Path-prefix-wise,
  every new route here is already reachable (all five sit under
  `/Train/...`, already in the allowlist from the train-tracking-frontend
  spec's own widening) — **no further prefix widening needed**. It does,
  however, need the two binary/multipart fixes from Correction 2 as part
  of this spec's own work.
- `frontend/components/PinToggle.tsx` / `frontend/components/TrackTrainForm.tsx`
  — the established `needsLogin` 401 pattern: a boolean set on a `401`,
  rendered as an inline `<TextLink href="/api/auth/login">`, with the
  form's already-typed field values left untouched (`TrackTrainForm`'s
  deliberate choice over `PinToggle`'s "forget the click" — the precedent
  this spec follows for any form with real input to protect).
- `frontend/lib/api.ts`'s `getSession()` / `getPreferences()` — the
  established "per-user Server Component read" pattern: forward
  `(await cookies()).toString()` as a `Cookie` header on a server-side
  `fetch`, since a Server Component's own `fetch` does not automatically
  inherit the incoming request's cookies. This is the exact mechanism
  needed to make the session-gated `GET /Train/{trackingId}/tickets` call
  from a Server Component page (see Decision 1) — no new plumbing
  pattern to invent, just reuse.
- `frontend/app/train/by-id/[trackingId]/page.tsx` and
  `frontend/app/train/[uid]/[date]/page.tsx` — both already shipped
  (train-tracking-frontend spec's own work), both render the shared,
  synchronous, non-async `components/TrainJourney.tsx` with a fetched
  `TrackedTrainState`. Critically, `state.id` (the tracking id) is present
  and correct on **both** page variants regardless of which lookup
  resolved it — this is what lets one new ticket component serve both
  pages without either page needing bespoke trackingId plumbing.
- `frontend/components/TextLink.tsx` — the app's single link component,
  `underline="always"` for body-flow links (WCAG 1.4.1), used throughout.
- Mantine's `FileInput` component ships with the already-installed
  `@mantine/core` package but is not used anywhere in this frontend today
  — there is no existing local convention for it to follow; see Open
  questions.

## Decisions

### 1. Entry point: attach to the existing tracked-train pages, gated by a server-side ownership probe — no standalone upload page

The schema settles this: `tracked_train_tickets` has no life independent
of a `tracked_train_id`, every ticket route is scoped by `trackingId`, and
there is no "list all my tickets across every tracked train" route at
all. A standalone `/tickets` page would have nothing backend-side to read
from. The two existing tracked-train pages are the only real integration
point.

But per Correction 4, those pages are public and can't tell who's
viewing. **Decision: a new async Server Component,
`components/TicketPanel.tsx`, taking `trackingId: number`, rendered by
both page files directly below their existing `<TrainJourney state={state} />`,
passed `state.id`.** It performs the same cookie-forwarding fetch as
`getSession()`/`getPreferences()` against `GET /Train/{trackingId}/tickets`,
and branches on the real, distinguishable outcomes documented above:

- **`401`** (not logged in at all) → a single inline nudge: "Log in to
  attach a ticket to this journey" (`TextLink` to `/api/auth/login`).
  Worded as "attach a ticket," not "see your ticket" — logging in doesn't
  guarantee this viewer owns this particular pin, so the copy doesn't
  promise something a subsequent `404` might immediately take back.
- **`404`** (logged in, but not the owner — or, redundantly, a tracking
  id that's already 404'd the page itself upstream) → **render nothing**.
  No ticket section appears at all for a non-owner viewer. Considered an
  explicit "this isn't your journey" message; rejected — every tracked-
  train page is public and shareable by design (per the train-tracking
  spec), so the overwhelming majority of page views are non-owners, and a
  permanent "not yours" banner on every one of those would be dead
  weight, not information.
- **`200` with `[]`** (owner, no ticket yet) → render an "Add a ticket for
  this journey" entry point that opens `TicketEntryForm` (Decision 2).
- **`200` with tickets** → render each `TrackedTrainTicket` plus its own
  Delay Repay estimate (Decision 3), and an "Add another ticket"
  affordance beneath them (multiple tickets per tracked train are a real,
  supported case per the schema).

Ticket-adding is **not** gated on `resolutionStatus`/`status` — nothing in
the backend restricts ticket creation to a `resolved` pin (confirmed:
`post_ticket` only checks ownership and field validation, never reads
`train_current_state`), so `TicketPanel` offers the "add a ticket" entry
point regardless of whether the pin has resolved yet. This matches a real
use case: a user tracking a `pending` pin already knows they hold a
ticket for it and shouldn't have to wait for TRUST to resolve the train
before recording that.

### 2. The upload/manual-entry flow: one form, two optional accelerants, always ending the same way

**`components/TicketEntryForm.tsx`** (Client Component — needs interactive
state and the `needsLogin` pattern). Three tabs/sections, all funneling
into the same underlying field set and the same final submit:

1. **Manual entry** (default, always available): plain text inputs for
   Operator, Ticket type, Origin CRS, Destination CRS — all optional,
   mirroring `TicketEntryRequest`'s own all-optional shape. Origin/
   Destination CRS get the same client-side `CRS_PATTERN = /^[A-Za-z]{3}$/`
   hint `TrackTrainForm` already uses, so a rejection is rare rather than
   the user's first encounter with the rule — but this is a *hint*, not a
   gate on submission the way it is server-side, since a blank field is
   valid (both are optional).
2. **Upload `.pkpass`**: a Mantine `FileInput` (`accept=".pkpass"`) plus
   an "Extract details" button. On click, builds a `FormData` with one
   `"file"` field and `fetch('/api/Train/{trackingId}/tickets/pkpass', { method: 'POST', body: formData })`
   — **no explicit `Content-Type` header set on this call**; the browser
   sets the correct `multipart/form-data; boundary=...` value itself when
   given a `FormData` body, and the proxy fix in Correction 2 is what lets
   that boundary survive to the backend.
3. **Upload PDF e-ticket**: identical shape, `accept="application/pdf"`,
   posting to the `pdf` route instead.

Both upload paths, on a `200`, **pre-fill the manual-entry fields from the
returned `PartialTicket` and switch back to the manual-entry view** — they
never bypass it. Each pre-filled field is visually marked as "auto-filled,
please check" (e.g. a subtle badge or helper text under the field), and
every field, pre-filled or not, stays a normal editable input — this is
what makes "review before save" real on the frontend, not just a backend
property: nothing about the upload step disables editing or offers a
one-click "accept as-is" that skips the CRS-format correction the backend
requires anyway. The hidden `source` value carried into the final submit
is whatever the upload response's own `source` field said
(`"pkpass-semantics"` / `"pkpass-heuristic"` / `"pdf-heuristic"`) — a
subsequent manual edit to any field does **not** reset `source` back to
`"manual"`; the provenance tag describes where the *starting point* came
from, matching the backend's own `source` semantics
(`crates/api/src/data/train_tracking.rs`'s doc comment: "confirmation, not
the parse itself, is what makes the row trustworthy"). Only a user who
started from the manual tab and never touched an upload keeps `source:
"manual"`.

Upload-step error handling, mapped from the real statuses documented
above:

| Status | Shown as |
|---|---|
| `200` | Pre-fill manual fields, switch to manual view, mark auto-filled fields |
| `400` | Inline: "That doesn't look like a valid upload — try again or fill in the form manually" |
| `422` | Inline, using the backend's own message text (already human-readable) — e.g. "could not read this as a train .pkpass: ..." — with a visible "or fill in the details manually" fallback right below it, since the manual form is still right there and always usable |
| `504` (PDF only) | "That file took too long to read — try a smaller or simpler PDF, or fill in the details manually" |
| `500` | Generic: "Couldn't read this file. Try filling in the details manually" |
| `413` | "That file is too large (8 MB limit). Try filling in the details manually" |
| `401` | Same `needsLogin` pattern as the final submit below — see next paragraph |

**Final submit** (from either the manual tab directly, or after an upload
pre-fill): `POST /api/Train/{trackingId}/tickets` with the current field
values and current `source`. Outcome handling mirrors `TrackTrainForm`
exactly: `200` → close the form, refresh the ticket list (`router.refresh()`,
consistent with `PinToggle`'s own post-write refresh); `401` → set
`needsLogin`, preserve every typed/pre-filled field, render the inline
login `TextLink` beside the form (this can legitimately happen here even
though `TicketPanel`'s own gate already confirmed the viewer was logged in
at page-load time — a session can expire between page load and form
submit, same defensive posture `TrackTrainForm` already has for its own
single-step submit); `400` → inline validation text using the backend's
own message (covers both the `source` and CRS-format checks); anything
else → generic "Couldn't save this ticket. Try again."

### 3. Displaying a ticket and its Delay Repay estimate

For each `TrackedTrainTicket` `TicketPanel` renders, it also
server-side-fetches `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
(same cookie-forwarding pattern) and renders the combined result. This is
eager, one fetch per ticket, not a client-triggered "check eligibility"
button — consistent with this app's existing "just refetch, no manual
poll control" posture (`AutoRefresh` already covers the whole page every
30s; per-ticket count is expected to be small, per Decision 1's "multiple
tickets" case being real but not the common case).

**Rendering rules, derived directly from the response shape documented
above, not from any additional signal the API doesn't provide:**

- `estimate` is `Some` → show the operator's scheme (`DR15`/`DR30`), the
  band it cleared, and the percentage, e.g. "Estimated compensation: 50%
  of your fare (DR30, 30+ minute delay)." This is explicitly labelled
  "estimated," never "your compensation is" or any phrasing implying a
  guarantee.
- `estimate` is `null` but `delayMinutes` is a real number → "Based on the
  recorded delay ({delayMinutes} minutes), this operator's Delay Repay
  rules may not give a payout at that length — but rules vary and this
  estimate can be wrong, so it's still worth checking directly." **This
  phrasing is deliberate**: the API gives no way to distinguish "you're
  genuinely under threshold" from "we don't recognize this operator's
  scheme" from "some other reason didn't clear a band" (documented above
  as a real, unresolved ambiguity in the response), so the copy must not
  claim a specific reason the data doesn't actually support.
- `estimate` is `null` and `delayMinutes` is also `null` → "No delay data
  recorded yet for this journey" — still followed by the claim link and
  disclaimer below, unconditionally (see next point) — a user who already
  knows they were delayed shouldn't be blocked from going straight to the
  operator's own page just because this app's own delay tracking hasn't
  caught up.

**Safety-critical, carried forward verbatim from the backend, not
paraphrased:** every one of the three cases above is followed,
unconditionally, by the response's `claimUrl` rendered as a real outbound
link and its top-level `disclaimer` string rendered as visible body text
— never a tooltip, never collapsed behind a "details" toggle, never
smaller/lighter styling than the estimate itself. Concretely:

- **Render `response.disclaimer` (the top-level field) verbatim, in
  full, every time this section renders regardless of whether `estimate`
  is `Some` or `null`.** Its exact current text (`DELAY_REPAY_ROUTE_DISCLAIMER`,
  `crates/api/src/routes/train.rs`) is:
  > "This is a rough, community-sourced estimate, not a guarantee of
  > compensation and not proof you travelled. This app never submits a
  > claim on your behalf — verify eligibility and claim directly from the
  > operator using the link above."

  **Do not paraphrase, shorten, or drop this string.** The frontend's job
  here is to surface exactly what the backend sends, not to write its own
  version of the same idea — if the backend's wording ever changes, the
  frontend picks it up automatically only if it renders the field
  directly rather than hardcoding an equivalent-sounding sentence.
- **`estimate.disclaimer` (present only when `estimate` is `Some`) is a
  second, textually different string** — `DISCLAIMER` in
  `crates/api/src/data/delay_repay_rules.rs`:
  > "This is a rough, community-sourced estimate, not a guarantee of
  > compensation and not proof you travelled. Always verify eligibility
  > and submit any claim directly with the operator — this app never
  > submits a claim on your behalf."

  Both strings carry the same core caveat but are not identical text, and
  only the top-level one is guaranteed present in every response.
  **Decision: render only the top-level `response.disclaimer`; do not
  additionally render `estimate.disclaimer`.** Showing both would put two
  near-duplicate but not-quite-matching caveats on screen at once, which
  reads as inconsistent rather than doubly cautious. The top-level field
  is the one the backend itself treats as unconditional (Task 5's own
  comment: "this route must never leave a caller with a bare percentage
  and no caveat"), so it's the canonical one to surface.
- **`claimUrl` is rendered as a real, clickable outbound link, labelled to
  describe an external action, never an in-app one.** E.g. "Go to
  [operator]'s Delay Repay page ↗" or, for the generic National Rail
  fallback, "See National Rail's compensation information ↗" — never
  "Claim now," "Submit claim," or any phrasing that could read as this
  app performing the claim. Standard external-link hygiene
  (`target="_blank" rel="noopener noreferrer"`) applies; this is the only
  place in this feature that opens a new tab, since every other action
  stays same-page.
- The estimate/disclaimer/claim-link block is visually one unit — the
  disclaimer and link must not be positioned or styled in a way that
  invites reading only the percentage and skipping the rest (e.g. no
  large bold percentage followed by fine-print caveat below the fold).

### 4. Auth UX: this feature has no unauthenticated read path at all — a real difference from train tracking

Train tracking's frontend spec could split "public reads, session-gated
writes." **Every single ticket-related route requires
`AuthenticatedUser`** (verified individually against each of the five
route handlers in `crates/api/src/routes/train.rs`, not assumed by analogy
— the brief explicitly warned not to assume symmetry with train-tracking's
auth posture, and this is the concrete confirmation that ticket data does
*not* inherit train-tracking's "state read is a public transit fact"
reasoning; a ticket record is "I personally had a ticket for this,"
inherently non-public). This means:

- `TicketPanel`'s own ownership probe (Decision 1) *is* this feature's
  entire "are you logged in" check for read purposes — there is no
  separate public partial view to fall back to.
- Every mutation (`TicketEntryForm`'s uploads and final submit) still
  needs its own independent 401 handling (Decision 2), because a session
  can lapse between the page's initial server-side probe and a later
  client-side action — same defensive layering `TrackTrainForm` already
  has for its single-step submit, just applied at two points here instead
  of one (upload step, save step).

No further proxy allowlist change is needed beyond Correction 2's
binary-safety fix — every route here is already under the `/Train/`
prefix the proxy allows.

### 5. Data refresh: reuse `AutoRefresh`, same as train tracking

`TicketPanel` and the per-ticket Delay Repay fetches are ordinary
`no-store` Server Component reads, refreshed automatically the same way
every other dynamic section of these pages already is — no new refresh
mechanism, same acknowledged tension `2026-08-29-train-tracking-frontend-design.md`
already accepted (a `cancelled`/finished journey's ticket section keeps
re-fetching every 30s even once its data can't change, which is this
app's existing blunt-but-simple posture everywhere, not a new tradeoff
introduced here).

## API/type contract

Hand-written, matching the verified shapes above — consistent with
`frontend/lib/types.ts`'s existing convention of not generating types from
the Rust source:

```ts
// frontend/lib/types.ts additions

export type TicketSource = 'manual' | 'pkpass-semantics' | 'pkpass-heuristic' | 'pdf-heuristic';

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
 * own internal-wire-type convention. */
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
 * expected, not an error. */
export interface PartialTicket {
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
}

export interface DelayRepayEstimate {
  scheme: 'DR15' | 'DR30';
  bandMinutes: number;
  percentage: number;
  disclaimer: string; // present only inside a non-null `estimate` -- see Decision 3
}

/** `GET .../tickets/{ticketId}/delay-repay`'s response. `claimUrl` and the
 * top-level `disclaimer` are ALWAYS populated, independent of `estimate`
 * -- see Decision 3 for the exact, current disclaimer text and why only
 * this field (not `estimate.disclaimer`) is rendered. */
export interface DelayRepayEstimateResponse {
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
}
```

```ts
// frontend/lib/api.ts additions -- per-viewer, session-gated reads, so
// each needs the same cookie-forwarding pattern getSession()/getPreferences()
// already use (a Server Component's own fetch does not inherit the
// incoming request's cookies).

export async function getTicketsForTrackedTrain(trackingId: number): Promise<TrackedTrainTicket[] | null> {
  // Returns null on 401/404 (see Decision 1's TicketPanel branching) --
  // deliberately not thrown as ApiNotFoundError, since "not yours" here
  // is an expected, common outcome (every non-owner viewer of a public
  // tracked-train page hits this), not an exceptional one.
}

export async function getDelayRepayEstimate(
  trackingId: number,
  ticketId: number,
): Promise<DelayRepayEstimateResponse | null> {
  // Same null-on-401/404 shape as above.
}
```

`POST .../tickets`, `POST .../tickets/pkpass`, `POST .../tickets/pdf` are
called only from `TicketEntryForm`, via `fetch('/api/Train/...', ...)`
through the (fixed, per Decision 2/Correction 2) proxy — never through
`lib/api.ts`, matching the existing split between server-only reads and
browser-initiated mutations `TrackTrainForm`/`PinToggle` already
establish.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                          │
│                                                                            │
│  app/train/by-id/[trackingId]/page.tsx        (existing, unmodified     │
│  app/train/[uid]/[date]/page.tsx               beyond adding <TicketPanel│
│                                                 trackingId={state.id}/>) │
│                                                                            │
│  components/TicketPanel.tsx      NEW -- async Server Comp, cookie-fwd    │
│                                    GET .../tickets [+ per-ticket          │
│                                    GET .../tickets/{id}/delay-repay]      │
│  components/TicketEntryForm.tsx  NEW -- Client Comp: manual tab +        │
│                                    .pkpass/PDF upload tabs, needsLogin    │
│                                                                            │
│  lib/api.ts    + getTicketsForTrackedTrain, getDelayRepayEstimate        │
│  lib/types.ts  + TrackedTrainTicket, TicketEntryRequest, PartialTicket,  │
│                   DelayRepayEstimate(Response), TicketSource             │
│                                                                            │
│  app/api/[...path]/route.ts   FIXED (Correction 2): preserve incoming    │
│                                 Content-Type; forward body via            │
│                                 arrayBuffer() not text() -- binary-safe   │
└──────────────────────────┬─────────────────────────┬─────────────────────┘
     server-side fetch     │                          │ browser fetch,
     (reads, cookie-fwd,   │                          │ via /api/Train/...
     no-store)             ▼                          ▼
                 ┌──────────────────────────────────────────────┐
                 │ api crate (existing, no backend changes        │
                 │ needed for this spec)                          │
                 │  GET  /Train/{id}/tickets                      │
                 │  GET  /Train/{id}/tickets/{ticketId}/delay-repay│
                 │  POST /Train/{id}/tickets            (JSON)    │
                 │  POST /Train/{id}/tickets/pkpass     (multipart)│
                 │  POST /Train/{id}/tickets/pdf        (multipart)│
                 └──────────────────────────────────────────────┘
```

## Error handling

- `TicketPanel`'s own `401`/`404` branching is not an error path — see
  Decision 1; both are expected, common, first-class outcomes rendered
  intentionally, not caught exceptions.
- Any other non-ok status from `getTicketsForTrackedTrain`/
  `getDelayRepayEstimate` (5xx, network failure) falls through to the
  existing root `app/error.tsx`, same as every other page section with no
  segment-specific error boundary today.
- `TicketEntryForm`'s upload-step and final-submit errors are handled
  entirely within the component, per the tables in Decision 2 — never a
  route-level crash, matching `TrackTrainForm`'s existing posture.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest):

- `lib/api.ts`: unit tests for `getTicketsForTrackedTrain`/
  `getDelayRepayEstimate` returning `null` on `401`/`404` vs. resolving
  normally on `200`, mirroring `getPreferences`'s existing 401-tolerant
  test shape.
- `components/TicketPanel.tsx`: render tests for all four Decision-1
  branches (`401` → login nudge, `404` → renders nothing, `200 []` → "add
  a ticket" entry point, `200` with tickets → each ticket + its estimate
  block rendered).
- `components/TicketEntryForm.tsx`: render/interaction tests covering (a)
  manual-entry submit success/401/400, mirroring `TrackTrainForm.test.tsx`'s
  existing shape; (b) each upload-step status in Decision 2's table,
  confirming the right inline message and the "fill in manually" fallback
  remains reachable; (c) confirming a pre-filled field from an upload stays
  editable and that editing it doesn't discard the carried-forward
  `source` value.
- Delay Repay estimate rendering: a render test per Decision 3 branch
  (`estimate` present, `estimate` null with a real `delayMinutes`,
  both null) confirming the top-level `disclaimer` string and `claimUrl`
  render in every case, and that `estimate.disclaimer` is never rendered
  a second time alongside it.
- `app/api/[...path]/route.ts`: extend existing coverage (verify what
  exists today at planning time) to confirm a multipart request forwards
  with its original `Content-Type` (boundary intact) and that binary body
  bytes round-trip unchanged through the proxy — the concrete regression
  test for Correction 2's fix, since a JSON-only test suite would not have
  caught either bug.

## Explicitly out of scope for this spec

- **Editing or deleting a saved ticket.** No `PUT`/`DELETE` route exists
  anywhere in `crates/api/src/routes/train.rs`'s ticket family (confirmed:
  `router()` only wires `.post`/`.get` for `/tickets`, a bare `.get` for
  the delay-repay route, and `.post` for each upload route) — this is a
  real backend gap, not an oversight in this pass, and no delete/edit UI
  is designed here as a result.
- **A "my tickets across all tracked trains" view.** No backend route
  supports this query (see Decision 1) — would need a new, unscoped
  `GET /tickets` (or similar) route, out of scope for a frontend-only
  spec.
- **Client-side file-size/type pre-validation beyond what the backend
  enforces.** The `8 MiB` cap and format checks are all server-side;
  adding a client-side size check before the upload even fires would give
  snappier feedback but isn't designed here — flagged as a nice-to-have,
  not a blocker.
- **Any UI implying this app can submit a claim or prove travel.** Not a
  gap to fill in later — a hard constraint this entire spec is built
  around (Decision 3), carried forward from
  `2026-08-29-journey-ticket-tracking-design.md`'s Non-goals and the
  Delay Repay Sniper precedent it cites.
- **Widening `AutoRefresh`** for the same finished-journey staleness case
  the train-tracking-frontend spec already declined to solve — this
  spec inherits that same open item, doesn't reopen it.

## Open questions / risks

1. **The Delay Repay estimate is fetched eagerly, once per ticket, on
   every page load.** Fine for the expected common case (one, maybe a
   handful, of tickets per tracked train), but nothing in the schema caps
   ticket count — a pathological number of tickets on one tracked train
   would mean a pathological number of server-side fetches per page view.
   Not resolved here; likely fine in practice, worth revisiting only if
   real usage shows otherwise.
2. **Whether `estimate.disclaimer` should ever be shown was a real
   judgment call, not something the backend resolves for the frontend.**
   Decision 3 chose to render only the top-level `response.disclaimer`
   and never `estimate.disclaimer`, reasoning that two near-duplicate
   caveat strings on screen at once reads as inconsistent rather than
   extra-safe. If the two strings drift further apart in wording in the
   future, this decision should be revisited — this spec picked the field
   that's unconditionally present, not the "more specific to the actual
   estimate" one.
3. **The `estimate: null` case has three possible real causes the API
   response doesn't distinguish** (no operator on the ticket, no delay
   data on the train yet, or a real delay that just didn't clear the
   scheme's threshold) — Decision 3's copy for that case was written
   carefully to avoid asserting a specific one of the three. If this
   turns out to read as unhelpfully vague in practice, the fix has to be
   a backend change (the route would need to expose which reason
   applied), not something the frontend can improve alone.
4. **The proxy fix (Correction 2) touches shared infrastructure every
   existing mutation already depends on**, not just this feature's new
   upload routes. Switching `req.text()` to `req.arrayBuffer()` should be
   a byte-identical round-trip for the existing JSON callers (`PinToggle`,
   `TrackTrainForm`, preferences, auth), and preserving the incoming
   Content-Type instead of hardcoding `application/json` should be inert
   for those same callers (they already send that header themselves) —
   but this needs real regression coverage across every existing proxy
   caller at implementation time, not just the new multipart path, since
   a mistake here would be a silent, cross-feature regression.
5. **No established local convention exists for a file-input control** —
   Mantine ships `FileInput`, unused elsewhere in this codebase today.
   This spec assumes it's the natural choice (matches the rest of the
   app's Mantine-first component sourcing) but flags that there's no
   existing local pattern to copy for its styling/error-display
   conventions, unlike every other input `TicketEntryForm`'s manual tab
   uses.
6. **Real-world hit rate of the `.pkpass` `semantics` vs. positional-
   fallback path, and of the PDF regex heuristic, is unconfirmed** — both
   design and plan docs already flag this as untested against real sample
   tickets (design doc's Open Question 1; plan's `ROUTE_PATTERN` comment).
   This spec's copy ("auto-filled, please check") is written to be honest
   regardless of the real hit rate, but the *frequency* with which a user
   sees an empty vs. populated pre-fill can't be predicted from this
   research pass — worth observing once this ships.
