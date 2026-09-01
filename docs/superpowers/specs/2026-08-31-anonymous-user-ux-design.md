# Design: Anonymous User UX

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(the closest precedent — a real, code-grounded frontend design covering an
auth-tied feature layered onto public pages) and, one level further back,
`docs/superpowers/specs/2026-07-07-frontend-design.md`. This is a design
document only — no implementation plan, no code changes. That is a
separate, later step in this repo's process.

## Goal

Two concrete problems were flagged:

1. Interactions/pages that require auth or only make sense for a logged-in
   user need to either be hidden or clearly prompt for auth. Named
   examples: pinning ("starring") lines/stations, and train tracking
   pages.
2. The home page (`/`) is currently useless for an anonymous or
   first-time visitor — its "Your Lines"/"Your Stations" sections are
   built entirely from `getPreferences()`, which is empty for anyone who
   has never pinned anything, leaving the page with nothing to show.

This was explicitly flagged as a non-exhaustive list. This doc walks
every route under `frontend/app/`, every component that branches on auth
state, and the relevant backend route handlers, to build a complete,
verified inventory (not a guess) of what happens today for a logged-out
vs. logged-in visitor at each surface — then proposes a single consistent
policy for how this app should treat auth-relevant UI, a concrete home
page redesign, and a reusable pattern so this doesn't get reinvented
differently every time a new auth-tied feature ships.

## Corrections to the brief's assumptions (recorded for posterity)

Following the ticket-tracking-frontend spec's own "Corrections" precedent:
this section exists because direct inspection of the code turned up
things the brief either got wrong, or didn't have — recorded here rather
than silently worked around, so the gap is visible to whoever reads this
next.

### 1. The tracked-trains-list design spec does not exist

The brief names
`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` as an
existing document to read alongside this audit. It is not in the repo:

```
$ find docs/superpowers -iname "*tracked-train*"
(no output)
$ grep -rl "tracked trains list\|TrackedTrainsList" . --include="*.md" -i
(no output)
```

Nothing in the codebase or docs tree references a "tracked trains list"
feature by that or any other name. This audit therefore cannot "account
for it landing" against a real design — there is nothing written down yet
about what that page will show, what route it lives at, or how it queries
tracked trains per-user. What follows instead is grounded in the one
piece of real evidence that does exist: `crates/api/src/data/train_tracking.rs`
and `crates/api/src/routes/train.rs`'s `post_track` handler create tracking
pins scoped to `user.id` (see below), which is enough to know the shape of
the auth problem (a per-user list, no useful anonymous form) without
knowing the feature's UI. §"Nav bar" and §"Policy" below state the general
rule a real tracked-trains-list spec should follow; whoever writes that
spec should point back at this document for its auth section rather than
re-derive one.

### 2. "Starring requires auth" is correct, not a misunderstanding — but the framing needs one addition

The brief asked this audit to verify precisely whether pinning is really
auth-gated, or whether (as a first read of `preferences.rs` might suggest)
it's a separate anonymous, per-browser feature. Read in full:

- `crates/api/src/routes/preferences.rs`'s own module doc says it
  outright: *"`/public/preferences`: which lines/stations are pinned to
  the home page... Fully session-gated, both read and write... pinned
  lines/stations are per-user state with no useful anonymous reading."*
  All three handlers (`get_preferences`, `put_pinned_lines`,
  `put_pinned_stations`) take an `AuthenticatedUser` extractor, which
  rejects with `401` + a plain-text body (`"no session"` or `"session
  expired or unknown"`) when there's no valid session cookie
  (`crates/api/src/auth.rs`'s `FromRequestParts` impl, lines ~131–147).
- `frontend/lib/api.ts`'s `getPreferences()` calls `/public/preferences`
  directly (with the incoming request's cookies forwarded — a Server
  Component's own `fetch` doesn't inherit them automatically) and, per its
  own doc comment, treats a `401` as "no preferences" rather than an
  error: it returns `{ pinnedLines: [], pinnedStations: [] }`. This is
  deliberate and correct — every page that calls it (`/`, `/lines`,
  `/stations/[crs]`) must still render for an anonymous visitor.
- `components/PinToggle.tsx`'s `toggle()` does the real end-to-end
  probe: it `fetch('/api/preferences')` first (this also 401s for an
  anonymous visitor, since the client-side proxy forwards status codes
  verbatim), and on a `401` sets a `needsLogin` flag rather than
  attempting the `PUT`. The same `needsLogin` handling wraps the `PUT`
  response. When set, it renders `<TextLink href="/api/auth/login">Log in
  to pin</TextLink>` next to the star.

So: **pinning does require a login, exactly as flagged** — this is not a
misreading of the code, and correcting the brief here would be wrong.
What the brief's framing under-states is that this is *not* a broken or
silently-no-op feature today — `PinToggle` already fails gracefully and
tells the visitor why, and `getPreferences()` already degrades every page
that reads it. The real, narrower gap is that this handling is
**reactive-only**: a logged-out visitor sees a perfectly normal-looking
star icon, has no reason to expect it needs an account, and only learns
that after clicking it and watching the request round-trip and fail. The
policy below (§"Policy", tier 2 refinement) proposes closing that specific
gap — not "fixing" pinning, which already works as designed.

### 3. A third auth-relevant surface exists that the brief didn't name: custom-line authoring

Auditing every page (per the brief's own instruction to find what it
didn't name) turned up a real, unflagged bug of the same shape as
starring, but *without* starring's graceful handling.

`crates/api/src/routes/lines.rs` mounts `create_line`, `update_line`, and
`delete_line` all behind `AuthenticatedUser` — added in a real, findable
commit:

```
$ git log --oneline -- crates/api/src/routes/lines.rs | tail -5
...
cd7e37f Scope custom-line authoring to the authenticated user
```

That commit also scoped ownership at the query level —
`crates/api/src/data/custom_lines.rs`'s `update_custom_line` and
`delete_custom_line` both run `WHERE id = $1 AND user_id = $2`, so even a
*logged-in* user editing/deleting a line they don't own gets `0` rows
affected, which the route maps to a `404`.

None of this reached the frontend:

- `frontend/app/lines/CustomLineForm.tsx` (used both for "New Custom
  Line" on `/lines` and for editing on `/lines/[id]/edit`) has no
  `needsLogin`/401 branch at all. Its generic handler does
  `const message = await response.text(); setError(message || ...)` — for
  an anonymous visitor that renders the raw backend rejection text, `"no
  session"`, in a red `<Text c="red">`. For a logged-in non-owner editing
  someone else's line, it would show whatever `update_line`'s 404 body
  says.
- `frontend/components/DeleteLineButton.tsx` has the identical gap — same
  generic `response.text()` fallback, no login prompt, no ownership
  awareness.
- `frontend/app/lines/[id]/page.tsx` renders the Edit/Delete buttons
  purely from `isCustom` (whether `getCustomLine(id)` 404s) — not from
  session state, and not from ownership. Every viewer, logged in or not,
  owner or not, sees live-looking Edit/Delete controls on every custom
  line.
- The module-level doc comment at the top of `lines.rs` is now stale:
  *"`/public/lines`: enumerate official + custom lines, and create/delete
  custom ones. **Unauthenticated** — see
  `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
  Non-goals for why."* That referenced doc's Non-goals section says *"No
  auth/ownership on custom lines **yet**... SSO/multi-user... is a known
  future direction, not built here."* The "yet" happened (`cd7e37f`); the
  module doc above it was never updated to match. `GET /public/lines` and
  `GET /public/lines/{id}` (reads) are still genuinely unauthenticated —
  only the doc's blanket "Unauthenticated" claim is wrong now.

This is folded into the policy and per-surface table below as a third
named problem alongside the two in the brief, since the brief explicitly
asked for anything else the audit turned up.

## Current relevant state (verified 2026-08-31)

Every `page.tsx` under `frontend/app/`, read in full, plus every
component that branches on session state (`grep -rn "getSession" frontend/`
found exactly three call sites: `app/layout.tsx`, `components/TicketPanel.tsx`,
and this doc's own new proposal touches a fourth for the home page).

| Surface | Anonymous today | Logged-in today | Backend auth | Category (see Policy) |
|---|---|---|---|---|
| `/` home dashboard | Renders "Your Lines"/"Your Stations" as empty-state text only (`getPreferences()` degrades to `{[], []}`). No other content. **The flagged problem.** | Pinned lines/stations render normally. | N/A (page itself has no gate; `getPreferences` degrades) | Redesign — see §Home page |
| `/lines` (All Lines table) | Table renders fully (public read, `/public/lines`). Pin star present per row; clicking it reactively shows "Log in to pin" (§Correction 2). "New Custom Line" form always rendered; submitting it as anonymous shows raw `"no session"` error text (§Correction 3). | Table + working pins. Custom-line form works. | Reads: none. Create: `AuthenticatedUser`. | Read = Tier 1. Pin = Tier 2 (existing pattern). Create form = Tier 2, currently broken (no login prompt) — needs the same treatment as PinToggle. |
| `/lines/[id]` | Read renders fully (public). If `isCustom`, Edit/Delete buttons render unconditionally and fail with a raw error string if clicked (§Correction 3). | Same read. Edit/Delete work only if the viewer owns the line; otherwise same silent-fail-with-raw-text as anonymous. | Read: none. Edit/Delete: `AuthenticatedUser` + ownership. | Read = Tier 1. Edit/Delete = Tier 3 (should not render for non-owners at all — see Policy). |
| `/lines/[id]/edit` | Page itself loads (`getCustomLine` is a public read). Submitting "Save changes" fails the same way as Correction 3. | Works if owner; fails the same way if not. | Read: none. Write: `AuthenticatedUser` + ownership. | Same as above. |
| `/lines/[id]/history` | Fully public, no auth surface anywhere on this page. | Identical. | None. | Tier 1 — no change needed. |
| `/stations`, `/stations/[crs]` | Search and disruption reads are fully public. `PinToggle` on the station page has the exact same reactive "Log in to pin" behavior as on `/lines` (same component). | Same read; pin works. | Reads: none. Pin: same as `/preferences`. | Tier 1 (reads) + Tier 2 (pin, existing pattern) — no change needed beyond the general Tier 2 refinement in Policy. |
| `/track` (search form) | Page and form always render (`TrackTrainForm`). Submitting posts to `/api/Train/track`, which 401s anonymously — the form **already** has a `needsLogin` branch matching PinToggle's pattern, and deliberately does *not* clear the four fields on a 401 (`TrackTrainForm.tsx`'s own comment: "a four-field form has real input worth protecting"). | Submitting creates a pin and redirects to `/train/by-id/{id}`. | `post_track` requires `AuthenticatedUser` (`crates/api/src/routes/train.rs`, confirmed: `user: AuthenticatedUser` param). | Tier 2 — already the reference-quality implementation of this tier. No change needed. |
| `/train/[uid]/[date]`, `/train/by-id/[trackingId]` | Both **public reads** — `get_by_tracking_id`/`get_by_uid_and_date` in `train.rs` take no `AuthenticatedUser` param at all. Reachable directly by URL; `TrainJourney` renders the live journey for anyone. `TicketPanel` beneath it: shows "Log in to attach a ticket" if `!session.authenticated`; renders nothing if authenticated-but-not-owner (`getTicketsForTrackedTrain` returns `null` on both 401 and 404, disambiguated by a separate `getSession()` call); renders the real ticket/Delay-Repay UI only for the owner. | Same reads. `TicketPanel` shows real content if owner, nothing if not. | Read: none. Ticket routes (`post_ticket`, `get_tickets`, `get_delay_repay_estimate`): `AuthenticatedUser`. | Read = Tier 1. `TicketPanel` = Tier 2/3 hybrid, and is the **reference pattern** this doc generalizes — see §Reusable pattern. |
| Nav bar (`app/layout.tsx`) | "All Lines" / "Station Lookup" / "Track a Train" always shown. `AuthNavItem` (Suspense-wrapped, `getSession().catch(...)`) renders `AuthStatus`, which shows a plain "Log in" link when `!session.authenticated`. | Same three links; `AuthStatus` shows name/email + a logout button. | N/A | No structural gate needed for the three links (all Tier 1/2 destinations) — see §Nav bar. |
| Tracked-trains-list | Does not exist yet (§Correction 1). | — | Inferred: per-`user.id`, same as `post_track`. | Will be Tier 3 once built — see §Nav bar and §Policy. |

## Policy

A single blanket rule ("always hide" or "always prompt") doesn't fit —
the cases genuinely differ in whether there's anything useful to show
before login. Three tiers, matching what's already partially built:

### Tier 1 — Public, no gate

Anything that's a *read* of shared, non-personal data: line/station
status (current or historical), the all-lines catalogue, station lookup,
a specific tracked train's live journey (any tracked train is a public,
shareable URL by design — confirmed by `get_by_tracking_id`/
`get_by_uid_and_date` taking no `AuthenticatedUser`). This is the app's
entire reason to exist and must never be gated. No change proposed to any
Tier 1 surface — they already work exactly like this.

### Tier 2 — Public entry, gated completion ("show, then prompt on the real 401")

Actions that are worth *offering* to an anonymous visitor even though
their effect is inherently per-user: pinning a line/station, submitting
the track-a-train form, attaching a ticket to a train you already own the
pin for. The control/form renders for everyone; the actual login
requirement is enforced by the backend and surfaced only when hit, via an
inline, honest prompt next to the control — never a silent no-op, never a
generic error string.

This is not a new idea — `PinToggle` and `TrackTrainForm` already do
exactly this, and it works well. The gap this audit found is twofold:

1. **Reactive-only, not proactive** (Correction 2). Where a page already
   knows the session state server-side before it renders (i.e., a page
   that could cheaply call `getSession()` alongside its other data), it
   should show the login affordance *before* the first click, not only
   after a failed one — e.g. a small "Log in to pin" hint rendered beside
   an unauthenticated visitor's star from the first paint, instead of the
   star looking identical to a logged-in visitor's until clicked. This is
   a genuine trade-off (see §Open questions) — `/lines` and
   `/stations/[crs]` don't fetch session today, and adding it purely for
   this purpose is new cost. Recommended default: keep the reactive
   pattern where session isn't already being fetched (it's honest and
   costs nothing extra), but where a page/component already has session
   in hand (e.g. any future page built with the pattern in
   §Reusable pattern below), prefer showing the prompt proactively.
2. **Not applied to custom-line authoring** (Correction 3). This is the
   one concrete Tier-2 surface that currently fails the tier's own
   contract — it shows a live-looking control and fails with a raw
   backend string instead of a login prompt. Fixing this means giving
   `CustomLineForm.tsx` and `DeleteLineButton.tsx` the same `needsLogin`
   branch `PinToggle`/`TrackTrainForm` already have: catch a `401`
   specifically, set a flag, render `<TextLink href="/api/auth/login">Log
   in to create/edit/delete a line</TextLink>` next to the control instead
   of (or in addition to) the generic error text.

### Tier 3 — Fully gated (hide entirely, or replace with an explicit login state)

Content with **no meaningful anonymous or non-owner form at all**: a list
of "my tracked trains", a list of "my tickets", and — this audit's one
addition to the brief's model — editing/deleting a *specific* custom line
the current viewer doesn't own (regardless of whether they're logged in
as someone else or not logged in at all). These should never render a
working-looking interactive control that then fails on click. Two
sub-cases, both already exemplified in this codebase:

- **Not logged in at all** → a clear, explicit login prompt in place of
  the content (`TicketPanel`'s `!session.authenticated` branch: `"Log in
  to attach a ticket to this journey"`).
- **Logged in, but not the owner** → render nothing, silently
  (`TicketPanel`'s `tickets === null` branch, deliberately not a "this
  isn't yours" banner, since every tracked-train page is public and this
  is the overwhelming common case for a page view from someone who isn't
  the owner).

Applying this to custom-line Edit/Delete requires one small, real backend
addition — not invented speculatively, but confirmed necessary: today
`GET /public/lines/{id}` (`CustomLineDetail` in `frontend/lib/types.ts`)
returns no ownership signal at all (`id`, `name`, `operators`, `stations`,
`headcodePrefixes`, `destinationCrsFilter` — no `owner`/`isOwner`/
`userId` field), so the frontend has no way to know, even for a
logged-in visitor, whether to render Edit/Delete. Recommendation: add a
boolean (`isOwner` or similar, computed server-side from
`AuthenticatedUser`/`OptionalAuthenticatedUser` vs. the stored
`user_id`) to that response, and gate the buttons on it — hidden
entirely for both anonymous visitors and non-owner logged-in visitors,
exactly like `TicketPanel`'s non-owner branch. This is the only backend
change this doc proposes; everything else below uses data that already
exists.

## Home page redesign

### What's there today

`frontend/app/page.tsx` already fetches everything needed for a useful
anonymous view — it just doesn't use most of it for that purpose:

```ts
const preferences = await getPreferences();
const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM); // ALL lines, every mode
```

`allReports` is the full status of every displayed line (national rail +
TfL), fetched unconditionally on every load. Today it's immediately
filtered down to `pinnedLineReports` and the rest is discarded. For an
anonymous visitor, `pinnedLineReports` is always empty, so the entire
fetch's result is thrown away and the page shows only two lines of
placeholder text ("You haven't pinned any lines yet... Browse all lines
to pin some.").

### Proposal

Branch the page on session state (one `getSession()` call, same
defensive `.catch()` fallback `app/layout.tsx` and `TicketPanel` already
use — an auth glitch degrades to "treat as anonymous", not a broken
homepage):

**Anonymous visitor** sees, in place of the two empty pinned sections:

1. A short explainer of what the app does (one line — this app has no
   onboarding anywhere else either, and a first-time visitor currently
   gets no explanation at all before hitting an empty dashboard).
2. **"Right now" system status** — built entirely from `allReports`,
   which is already being fetched on this exact page today. No new
   endpoint. Reuse `severityRank`/`worstStatus` (already imported here)
   to compute, e.g.: a count of lines currently not at "Good Service", and
   a short list of the worst-affected lines (same sort the pinned section
   already does: worst-first, then alphabetical), each linking to
   `/lines/[id]`. This gives a real, live, useful homepage to someone who
   has never touched the app, using data it already pulls on every load.
3. A CTA row: `Browse all lines` / `Look up a station` (both already
   exist as links elsewhere on this page) plus, new, a proactive `Log in
   to pin your lines and stations` link (`/api/auth/login`) — replacing
   the passive "you haven't pinned anything" text with an actual next
   step, consistent with the Tier 2 proactive-prompt preference above
   (this page already fetches session for this branch, so there's no
   additional cost to being proactive here specifically).

**Logged-in visitor** keeps today's behavior unchanged — pinned lines and
stations render exactly as now. Optionally (not required to close the
brief's flagged issue, but worth noting): a logged-in visitor with zero
pins currently sees the same "you haven't pinned anything" text as
before; that's arguably fine for them specifically since they've already
been prompted to log in and can act on it immediately via `/lines`/
`/stations`, so no separate empty-state redesign is proposed for that
narrower case.

Explicitly not proposed: a "most-disrupted stations" widget. Unlike
lines, there's no existing bulk endpoint that returns disruption status
for many stations at once — `getStopPointDisruption(crs)` is per-station,
and `/public/stations` (used by `getStationName`) is a name/search
lookup, not a disruption feed. Building that would need a genuine new
backend endpoint, which this doc's brief explicitly says not to invent
without confirming the data doesn't already exist — it doesn't, so it's
named here as a real, separate, future idea, not folded into this
redesign.

## Reusable pattern

The three working Tier-2 implementations (`PinToggle`, `TrackTrainForm`)
and the one working Tier-2/3 hybrid (`TicketPanel`) already converged
independently on the same shape without a shared component — which is
exactly how `CustomLineForm`/`DeleteLineButton` ended up *not* having it
(§Correction 3). Two distinct sub-patterns exist and should both be named
and reused explicitly, rather than each new feature re-deriving its own
version:

**1. Client-side "needsLogin" 401 handling** (`PinToggle`, `TrackTrainForm`,
and the proposed fix for `CustomLineForm`/`DeleteLineButton`): catch a
`401` specifically from a same-origin `/api/*` proxy call, set a boolean,
render `<TextLink href="/api/auth/login" underline="always">Log in to
{verb}</TextLink>` beside the control, and — this is the part that's easy
to drop, as `CustomLineForm`/`DeleteLineButton` show — never let a
non-401 failure or a 401 body get treated as generic text shown to the
user. Worth extracting as one shared piece (e.g. a `useNeedsLogin()` hook
returning `{ needsLogin, run(fn) }`, or a small `<LoginPrompt verb="pin"
/>` component) purely so the *shape* can't drift per-feature the way it
already has once. Not proposed as a mandatory refactor of the two
existing correct implementations (`PinToggle`/`TrackTrainForm`) — but any
new Tier-2 control, and the fix to `CustomLineForm`/`DeleteLineButton`,
should use it rather than hand-roll a fourth slightly-different version.

**2. Server-side "ownership probe" composition** (`TicketPanel`): a
Server Component that (a) calls `getSession()` with the same defensive
`.catch()` fallback as `app/layout.tsx`, (b) branches to a login prompt if
unauthenticated, (c) otherwise fetches the owner-scoped data and branches
again on `null` (not-owner, render nothing) vs. real content. This is
already documented in `TicketPanel.tsx`'s own comment as deliberate
composition rather than widening `getTicketsForTrackedTrain`'s signature.
The same three-way branch (unauthenticated → prompt; authenticated,
not-owner → nothing; authenticated, owner → content) is exactly what a
future tracked-trains-list page and the proposed custom-line
Edit/Delete-visibility fix both need. Recommend documenting this
explicitly as "the ownership-probe pattern" (in code comments, the way
`TicketPanel.tsx` already half does) so the next auth-tied feature copies
it on purpose rather than reinventing a fourth branching scheme.

Both patterns already exist in working form in this codebase; the
recommendation is consolidation and consistent reuse, not new
architecture.

## Nav bar changes

- **"Track a Train"**: keep visible unconditionally. It's Tier 2 by
  policy — the search form itself has value to demonstrate even before
  login, and `TrackTrainForm` already implements the gate correctly at
  submit time. Hiding the nav link would just relocate the same
  information a click away for no benefit, and contradicts the form's own
  documented design intent ("Decision 4, no navigation away").
- **Tracked-trains-list** (once it exists — §Correction 1): Tier 3, no
  anonymous value at all (a list of trains *you* tracked). Should render
  only for an authenticated session, following `AuthNavItem`'s existing
  `Suspense`-wrapped pattern in `app/layout.tsx` — reuse the *same*
  `getSession()` call already made for `AuthStatus` (e.g. render it from
  inside `AuthNavItem` alongside `AuthStatus`, or lift session into a
  shared server component both read from) rather than a second,
  independent session fetch just for this link.
- No other nav changes are implied by this audit. `AuthStatus` already
  does the right thing (login link vs. name+logout), and `/lines`/
  `/stations` both stay unconditionally visible (Tier 1 reads).

## Open questions / risks

1. **Proactive vs. reactive Tier-2 prompting cost.** Making the
   login-prompt proactive everywhere (not just reactive-after-401) means
   fetching session on pages that don't today (`/lines`,
   `/stations/[crs]`) purely to decorate a star icon. Recommended default
   above is "proactive only where session is already in hand" rather than
   adding new session fetches solely for this — but this is a judgment
   call for whoever implements it, not settled definitively here.
2. **The `isOwner` backend addition** (§Policy, Tier 3) is small but real
   — needs its own review of exactly how ownership is computed for an
   anonymous request (should be `false`, not an error) and whether it
   uses `AuthenticatedUser` (which 401s) or `OptionalAuthenticatedUser`
   (which doesn't — already exists in `crates/api/src/auth.rs` for
   exactly this "never reject, just tell me who if anyone" shape, used
   today only by `GET /auth/session`). `OptionalAuthenticatedUser` is the
   correct extractor for this — `GET /public/lines/{id}` must keep
   working for anonymous readers.
3. **The tracked-trains-list spec doesn't exist** (§Correction 1) — this
   doc describes the *policy* it should follow, not its actual design
   (route, list rendering, pagination, etc.), which is out of scope here
   and belongs in that spec once written.
4. **`PinToggle`'s documented last-write-wins race** (its own comment:
   concurrent pin clicks against the whole-list `PUT` can silently drop
   an earlier change) is a real, already-known issue but orthogonal to
   auth UX — not addressed by this doc, flagged only so it isn't
   conflated with the changes proposed here.

## Non-goals

- No implementation — this is a design document only, per the brief.
- No new backend endpoints beyond the one identified, minimal addition
  (an ownership flag on `GET /public/lines/{id}`, using the existing
  `OptionalAuthenticatedUser` extractor) — every other recommendation
  here (the home page's "right now" widget in particular) is grounded in
  data already fetched by an existing route.
- No redesign of the ticket-tracking or train-tracking flows themselves —
  `TicketPanel`/`TrackTrainForm` are treated here as the established,
  working reference pattern, not something this doc revises.
- No attempt to design the tracked-trains-list feature itself (route
  shape, pagination, etc.) — only the auth policy it should inherit.
