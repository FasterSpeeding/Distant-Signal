# Design: Standalone Ticket Entry Page

**Status: design proposal, not approved.** No implementation plan or code
changes are included — that is a separate, later step in this repo's
process. Written as a direct structural sibling to
`docs/superpowers/specs/2026-09-02-custom-line-creation-page-design.md`
(merged to `main` this session), which designs the exact same shape of
change — an inline, collapsed-by-default form moved to its own dedicated
page, linked via an entry point placed at the top of the page it used to
live at the bottom of — for `CustomLineForm`/`/lines`. This spec follows
that precedent's reasoning and conventions everywhere they transfer, and
says explicitly, in each Decision, where this case differs and why.

## Goal

`frontend/app/track/mine/page.tsx` (the merged "My Trains & Tickets" page,
read in full) renders `<TicketEntryForm label="Add a ticket" />` inline at
the very bottom of the page (line 113), below the tracked-trains list and
the unattached-tickets section — a visitor has to scroll past everything
else on the page to reach it, and today it renders as a single collapsed
`Button` reading "Add a ticket" until clicked. The same page already has a
`<TextLink href="/track">Track a new train</TextLink>` at its top, beside
the page's `<Title order={1}>My Trains &amp; Tickets</Title>`
(`page.tsx:76-79`). This spec moves ticket entry to its own dedicated
route, with a second entry-point link placed next to the existing one, and
works out the route, the entry point's copy and layout, whether the moved
form should render pre-expanded, what happens after a successful save, and
what does and doesn't change on `TicketEntryForm` itself to support the
move.

## Current relevant state (verified 2026-09-02)

- **`frontend/app/track/mine/page.tsx`** (read in full):
  - `MyTrackedTrainsPage` (lines 46-58): calls
    `Promise.all([getMyTrackedTrains(), getMyTickets()])`; if `trains ===
    null` (not logged in — `getMyTrackedTrains()`'s complete 401 signal,
    per that function's own doc comment and this page's own doc comment,
    lines 37-40), returns **immediately** with just `<Title>` + a
    `LoginLink` login nudge — no `Group`, no `TextLink`s, no ticket form at
    all render for a logged-out visitor.
  - The entry-point `Group` (lines 76-79) and everything below it
    (including line 113's `<TicketEntryForm />`) only exist on the
    non-`null` (logged-in) branch. **The existing "Track a new train" link
    is therefore already fully auth-gated for free** — an anonymous
    visitor never sees it, with no separate session probe on this page
    beyond the one `getMyTrackedTrains()` already performs.
  - The empty-state text (lines 80-84, `nothingToShow`) mentions both
    trains and tickets in prose ("You haven't tracked any trains or added
    any tickets yet") but links only to `/track`; unaffected by this spec.
- **`frontend/components/TicketEntryForm.tsx`** (read in full):
  - Signature (line 53): `{ trackingId, label }: { trackingId?: number;
    label: string }`. `open` (line 55) defaults to `useState(false)` —
    collapsed by default, unconditionally, for every call site today.
  - Doc comment (lines 13-52) states the collapse mechanism is this
    component's own self-contained choice ("kept self-contained here so
    `TicketPanel` stays a plain, server-renderable async function with no
    interactive state of its own") — not something a design spec upstream
    of it dictated. Nothing about the mechanism is tied to `trackingId`
    being present or absent.
  - `handleSubmit`'s success branch (lines 197-215): when `trackingId ===
    undefined` (standalone ticket, lines 199-211), it does **not** call
    `setOpen(false)` — it sets `savedStandaloneTicket`, resets the field
    state, and calls `router.refresh()`, then returns. When `trackingId` is
    given (lines 212-215), it collapses (`setOpen(false)`) and refreshes.
  - The `savedStandaloneTicket` render branch (lines 233-273) is checked
    **before** the `if (!open)` collapsed-button check (line 275) — so once
    a standalone save succeeds, the "Ticket saved" `Alert` fully replaces
    whatever `open` would otherwise render, regardless of its value. Its
    "Done for now" button (lines 259-268) clears `savedStandaloneTicket`
    **and** calls `setOpen(false)`, returning the component to its
    collapsed `label` button — i.e., today's standalone flow already
    supports "save, decline to track a train right now, and the entry
    point re-collapses so another ticket can be added from the same
    place" — this is existing behavior, not something this spec needs to
    add.
  - No `maw`/width constraint anywhere on the component's own root
    `<Stack>` (line 284) or its collapsed `<Button>` (line 277) — unlike
    `CustomLineForm.tsx:165`'s own internal `maw={480}`, which is what the
    custom-line precedent's dedicated-page layout (`<Center><Stack
    maw={480}>`) was explicitly sized to match.
- **`frontend/components/TicketPanel.tsx`** (read in full): two call
  sites, both `trackingId`-scoped —
  `<TicketEntryForm trackingId={trackingId} label="Add a ticket for this
  journey" />` (line 74, zero-tickets branch) and
  `<TicketEntryForm trackingId={trackingId} label="Add another ticket" />`
  (line 98, has-tickets branch). Both render on `/train/by-id/[trackingId]`
  and `/train/[uid]/[date]`, directly under `<TrainJourney>`, for a
  *specific already-tracked train*. `TicketPanel` also has its own
  proactive `getSession()` check (lines 37-42, with a `.catch()` fallback
  to `{ authenticated: false, ... }`) and login-nudge branch (lines 43-60)
  — a different, existing gating mechanism from `/track/mine`'s
  `getMyTrackedTrains()`-null approach, kept for reference below (Decision
  3) since it's the closer local precedent for a proactive session check
  that exists purely to decorate an otherwise-reactive form.
- **`frontend/next.config.mjs`** (lines 18-31): a config-level
  `redirects()` entry, `source: '/track/tickets'` (exact string, no
  wildcard) → `destination: '/track/mine'`, `permanent: true` — a leftover
  redirect stub from when `/track/tickets` was its own page, kept working
  for old bookmarks. Its own comment states it is deliberately a
  config-level redirect, not a rendered stub page — no
  `frontend/app/track/tickets/` directory exists in the tree at all
  (confirmed: `find frontend/app/track -type f` returns only
  `page.tsx`, `mine/page.tsx`, `mine/page.test.tsx`). Because the redirect
  `source` is an exact string rather than a `:path*` prefix, it would not
  intercept a hypothetical `/track/tickets/new` — but reusing the
  `tickets` segment for a *new* real route would still put a live page one
  level under a path that already means "dead redirect stub" one level up,
  which is a legibility cost even though not a routing conflict (Decision
  1).
- **`frontend/app/track/page.tsx`** (read in full): the existing,
  **pre-existing** `/track` route — `<Title order={1}>Track a Train</Title>`
  + `<TrackTrainForm .../>` in a plain, unconstrained `<Stack p="lg"
  gap="md">` (no `Center`, no `maw`). This is what "Track a new train"
  already links to (`page.tsx:78`) — it predates the `/track/mine` merge
  entirely (per `/track/mine`'s own doc comment on the history of the
  merge) and was never created *for* the entry-point-link pattern; ticket
  entry has no equivalent pre-existing top-level route to point at instead.
- **`frontend/app/layout.tsx`** (lines 104-118): `TrackedTrainsNavItem`,
  an always-visible (when logged in) nav-bar link,
  `<TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>`
  (line 118) — an existing, global way back to `/track/mine` from anywhere
  in the app, independent of anything this spec adds.
- **`frontend/components/TextLink.tsx`** (read in full): the app's
  standard in-page link, `underline="hover"` default, whose own doc
  comment recommends that default for "a right-aligned action beside a
  section heading" — exactly this spec's proposed placement, same as the
  custom-line precedent's own citation of it.
- **`frontend/app/track/mine/page.test.tsx`** (read in full): the one
  existing assertion this spec's move invalidates is `'renders the "Add a
  ticket" entry point'` (lines 209-214), which asserts `screen.getByRole('
  button', { name: 'Add a ticket' })` — a `button` query, because today's
  entry point *is* `TicketEntryForm`'s own collapsed button rendered
  in-place. No existing test in this file asserts the "Track a new train"
  link's `href` at all (`grep` for that string only matches the unrelated,
  per-ticket "Track a new train for this ticket" links inside
  `UnattachedTicketRow`, lines 143/158/169).
- **`frontend/components/TicketEntryForm.test.tsx`** (read in full):
  `'starts collapsed, showing only the entry-point button'` (lines 52-57)
  locks in today's unconditional `open` default of `false`. A `describe('
  with no trackingId (standalone ticket)', ...)` block (lines 210-267)
  already exercises the exact no-`trackingId` instance this spec relocates
  — `openStandaloneForm()` (lines 211-214) renders `<TicketEntryForm
  label="Add a ticket" />` with no `trackingId`, matching
  `page.tsx:113`'s call exactly. This block's tests (flat-route POSTs, the
  "find or track" next-step link, its href with/without an origin) are
  about `TicketEntryForm`'s own behavior, not about which page renders it
  — nothing in them depends on `/track/mine` specifically.

## Decisions

### 1. New route: `/track/mine/add-ticket`

**Chosen.** `frontend/app/track/mine/add-ticket/page.tsx`, nested under
the same page that hosts the entry point — following the custom-line
precedent's own pattern literally (`/lines` → `/lines/new`, the
destination nested directly under the page carrying the link), not
inventing a new top-level `/track/*` sibling.

Two real alternatives were weighed and rejected:

- **`/track/tickets/new`** (or similar, reusing the `tickets` segment).
  **Rejected.** Not a literal routing conflict (Current relevant state
  above: the `/track/tickets` redirect is an exact-match `source`, not a
  `:path*` prefix, so a real page under `/track/tickets/*` would still
  resolve to the filesystem route). But it's a real legibility cost: this
  tree has zero `frontend/app/track/tickets/` directory today, purely a
  dead-stub redirect target one segment above where a real, permanent page
  would then live — a future reader hitting that combination has to
  untangle "this segment both 308s at its root and hosts a live child" for
  no benefit over a name that avoids the overlap entirely.
- **`/track/mine/new`**, mirroring `/lines/new` most literally.
  **Rejected**, specifically because `/track/mine` is not analogous to
  `/lines` on this axis: `/lines` only ever creates one kind of thing (a
  line), so an unqualified `new` is unambiguous there. `/track/mine` hosts
  *two* kinds of content (trains and tickets) and already has a working,
  pre-existing route for "new train" that lives entirely outside this
  page's own tree (`/track`, Current relevant state above) — an
  unqualified `/track/mine/new` sitting next to that existing pattern
  would read as if it were the train-creation route's sibling, when it
  is not.
- **`/track/mine/add-ticket`**. **Chosen.** Unambiguous by construction
  (no other "new" resource lives at this exact path), reuses the feature's
  own already-established name verbatim (the collapsed button's own
  `label="Add a ticket"` at `page.tsx:113` today) rather than inventing new
  phrasing — the same "reuse the existing name for both the link and the
  destination" reasoning the custom-line precedent's Decision 2 already
  used for "New custom line." Kebab-case for a multi-word segment already
  has precedent in this app (`/train/by-id/[trackingId]`).

No dynamic-segment collision risk: `frontend/app/track/mine/` today
contains only `page.tsx` and `page.test.tsx` (Current relevant state,
`find frontend/app/track -type f`) — no existing children, dynamic or
otherwise, for a new static `add-ticket/` directory to shadow or be
shadowed by.

### 2. Entry point: a second `TextLink` in the same `Group`, reusing "Add a ticket" verbatim

**Chosen:**

```tsx
<Group justify="space-between" align="baseline">
  <Title order={1}>My Trains &amp; Tickets</Title>
  <Group gap="md">
    <TextLink href="/track">Track a new train</TextLink>
    <TextLink href="/track/mine/add-ticket">Add a ticket</TextLink>
  </Group>
</Group>
```

The existing `justify="space-between" align="baseline"` `Group` and its
first `TextLink` are unchanged in position and props — only a second
`TextLink`, wrapped with the first in an inner `Group gap="md"` so both
sit together as one right-aligned cluster rather than the outer
`space-between` trying to place two independent items against the title.
Same component, same `underline="hover"` default, same visual weight as
the existing link — no new visual language, matching this codebase's
consistent practice of not inventing a second style tier for what is
functionally the same kind of action (a page-level entry point beside a
heading).

**Copy: "Add a ticket," not "Add a new ticket."** Two options weighed,
mirroring the custom-line precedent's own copy decision (Decision 2
there):

- **Match "Track a new train"'s exact grammatical shape** ("Track a new
  train" → "Add a new ticket"). Considered, but rejected for the same
  reason the precedent rejected the equivalent option: this feature
  already has an established name — the collapsed button today reads
  exactly "Add a ticket" (`page.tsx:113`), and `TicketPanel`'s own two
  call sites use the same "Add a[nother] ticket" family of phrasing
  (`TicketPanel.tsx:74,98`). Introducing a second, differently-worded name
  for the identical feature at the exact moment it gets a dedicated page
  would be gratuitous churn.
- **Reuse "Add a ticket" verbatim, for both the link and the new page's
  `<Title order={1}>`.** **Chosen** — same "link text names its
  destination" pairing the precedent already established
  (Edit → "Edit: {line.name}"; "New custom line" → "New custom line").

Order: "Track a new train" stays first (unchanged), "Add a ticket" second
— matches the existing page's own content order (trains listed before
tickets, `page.tsx:87-93` before `94-110`), so the two entry points read
left-to-right in the same priority order the rest of the page already
uses.

### 3. Auth-gating the new page: a proactive `getSession()` check, not reactive-only

**Chosen: `/track/mine/add-ticket` calls `getSession()` server-side and
renders the same login-nudge shape `/track/mine` itself uses (`<Title>` +
`LoginLink`) when `!session.authenticated`, before rendering the form at
all.**

This is a real departure from the custom-line precedent's own Decision 3
(no proactive check on `/lines`, reactive-only via `CustomLineForm`'s
existing `useNeedsLogin`/`LoginLink` handling) — worth stating plainly
rather than copying that decision blindly, because the two host pages sit
in different tiers:

- `/lines` is classified Tier 2 ("public entry, gated completion") by
  `2026-08-31-anonymous-user-ux-design.md`'s own per-surface table, and
  critically *does not fetch session at all today* — the precedent's
  Decision 3 leaned on that specific, already-adopted policy default
  ("keep the reactive pattern where session isn't already being fetched").
- `/track/mine` is not a public-entry page at all: `trains === null`
  makes the **entire page** — including the entry-point `Group` itself —
  render nothing but a login nudge (Current relevant state, above). A
  visitor only ever *sees* the "Add a ticket" link after already being
  known to be logged in. Sending that same visitor to a *laxer* destination
  page (one that renders the real form and only discovers "actually,
  you're not logged in" reactively at submit time) would be an
  inconsistency within this one page family, not a considered policy
  choice — the entry point's own gating already promises "you're logged
  in past this point," and the destination should keep that promise.

The check uses `getSession()` directly (the same call `TicketPanel.tsx:37`
already makes for an identical purpose — knowing whether *anyone* is
logged in, not fetching data scoped to a specific train), not a second
call to `getMyTrackedTrains()` purely for its side-channel 401 signal —
reusing that heavier, list-fetching call here would fetch data this page
never uses, just to borrow its auth signal.

**What does not change:** `TicketEntryForm`'s own existing reactive
401-on-submit handling (`needsLogin` state, `LoginLink` at
`TicketEntryForm.tsx:375-379`) stays exactly as-is and unmodified — the
proactive page-level check is a courtesy that avoids showing the form to
someone who obviously can't use it (e.g. a session that expired between
loading `/track/mine` and clicking through), not a replacement for the
form's own defense against a session that expires mid-interaction on this
page itself.

### 4. Collapse-vs-expanded default: pre-expanded on the new page, via a new optional `defaultOpen` prop

**Chosen: `TicketEntryForm` gains a new optional prop, `defaultOpen?:
boolean` (default `false`), used only to seed `open`'s initial value
(`useState(defaultOpen ?? false)` in place of today's hardcoded
`useState(false)`). `/track/mine/add-ticket` is the only call site that
passes `defaultOpen`.**

Real call, not a copy of the precedent: `CustomLineForm` has no collapse
mechanism at all to preserve or discard, so the custom-line spec never
faced this question. Two options were weighed here:

- **Keep collapsed-by-default on the new page too**, relocating
  `page.tsx:113`'s call unmodified (`<TicketEntryForm label="Add a
  ticket" />`, zero changes to `TicketEntryForm` itself).
  **Rejected.** On `/track/mine`, collapsing serves a real purpose: it
  keeps a form that isn't every visitor's reason for being on that page
  from cluttering a page that already shows a trains list and an
  unattached-tickets section (the form's own doc comment says as much,
  Current relevant state above). A dedicated page whose *entire* reason
  for existing is "add a ticket," with a `<Title order={1}>Add a
  ticket</Title>` already saying so, has no competing content to protect
  — forcing a click through a button reading the identical words the page
  heading already committed to is friction with nothing behind it.
- **Add `defaultOpen`, pre-expand only on the new page.** **Chosen.**
  Small, additive, backward-compatible: every existing call site
  (`TicketPanel.tsx:74,98`, and this page's own now-removed inline call)
  keeps `useState(false)`'s exact prior behavior since the prop is
  optional and defaults to `false` — this is infrastructure enabling the
  move, not new business logic, matching the task's own framing that this
  should be "a straightforward move, not a new capability."

**The collapsed-button `label` prop stays meaningfully used even with
`defaultOpen`:** the form's existing Cancel button (`TicketEntryForm.tsx:
372-374`, `onClick={() => setOpen(false)}`) and the standalone
success path's "Done for now" button (lines 259-268, also calls
`setOpen(false)`) both already return the component to its collapsed
`label`-button state regardless of how it started. On
`/track/mine/add-ticket`, a visitor who cancels, or who saves one ticket
and clicks "Done for now," sees the same "Add a ticket" button reappear in
place — letting them add a second ticket from the same page visit without
re-navigating. This falls out of `TicketEntryForm`'s existing code
unmodified; `defaultOpen` only changes the *first* render.

### 5. Post-save behavior: no change to `TicketEntryForm`'s own logic; an explicit page-level "Back" link, not a forced redirect

**Chosen: `TicketEntryForm`'s standalone success branch
(`TicketEntryForm.tsx:199-211`) is reused completely unmodified** — same
"Ticket saved" `Alert`, same "Find or track the train this ticket is for"
link (→ `/track?origin=...&ticketId=...`), same "Done for now" button. No
`router.push` is added anywhere in this path.

This is a materially different situation from the custom-line precedent's
Decision 4, which had to revisit `CustomLineForm`'s create-mode navigation
specifically *because* that form's success path did a same-route
`router.push('/lines')` that the route move turned into a genuine
cross-route navigation, making a manual state-reset workaround newly dead.
**`TicketEntryForm`'s standalone success path never navigates away at
all** — it deliberately stays in place and shows a next-step nudge,
because (per its own doc comment) extraction can never recover a
date/time, so this app has no way to guess which tracked train a
standalone ticket belongs to. That reasoning is about the *ticket's data*,
not about which route hosts the form — it holds identically whether the
form lives at `/track/mine` (today) or `/track/mine/add-ticket` (this
spec). No workaround exists here to become dead code; Decision 4's
situation from the precedent simply doesn't recur.

**What the dedicated page adds, at the page level, not inside
`TicketEntryForm`:** a static "Back to My Trains &amp; Tickets"
`TextLink` (`href="/track/mine"`) in the new page's own chrome, near its
`<Title>`. Two options were weighed for "how does a user leave once
they're done":

- **Rely solely on the always-visible nav-bar link**
  (`TrackedTrainsNavItem`, `app/layout.tsx:118`, already links to
  `/track/mine` whenever logged in). Considered sufficient functionally,
  but **rejected as the only affordance**: it's global and indirect,
  whereas this codebase's own established local convention for "how do I
  leave a create/edit form" is a page-level, adjacent link —
  `CustomLineForm`'s `cancelHref` prop, rendered right beside its own
  submit button (`[id]/edit/page.tsx:51`, `CustomLineForm.tsx:249-267`).
  Leaving this page with no local equivalent at all would be a visible gap
  next to that precedent.
- **A plain page-level `TextLink`, not a new `TicketEntryForm` prop.**
  **Chosen.** `TicketEntryForm` has no existing `cancelHref`-style prop
  to reuse, and adding one purely for this one call site would mean
  either threading it through as unused on `TicketPanel`'s two call
  sites or adding conditional logic there — neither buys anything a
  static link at the page's own top, outside the form entirely, doesn't
  already achieve more cheaply. `TicketEntryForm`'s own internal Cancel
  button (which collapses back to the entry-point label, Decision 4)
  already covers "I want to back out of the form itself but stay on this
  page"; the new page-level link covers the separate "I'm done with this
  page entirely" case.

No forced redirect after a successful save was considered and rejected:
it would remove the "add a second ticket in one visit" capability
Decision 4 already gets for free, and would contradict the
already-designed "Done for now" button's own implication that staying put
is a deliberate, supported choice, not an oversight.

### 6. Page layout: an unconstrained `Stack`, not `Center`+`maw={480}`

**Chosen: `<Stack p="lg" gap="md">` (no `Center`, no `maw`), matching
`/track/page.tsx`'s own existing layout** for the sibling form
(`TrackTrainForm`) this page sits directly beside conceptually.

The custom-line precedent's `<Center><Stack maw={480}>` chrome
(`[id]/edit/page.tsx:23-35`) was not an arbitrary layout choice to copy —
that spec's own Current relevant state is explicit that the `480` was
picked specifically to match `CustomLineForm`'s own internal `maw={480}`
(`CustomLineForm.tsx:165`), so the page's heading lines up with the form's
edges. `TicketEntryForm` sets no `maw`/width constraint anywhere on its
own root `<Stack>` or collapsed `<Button>` (Current relevant state,
above) — there is no internal width for a page-level `maw` to line up
with, and this app's other page hosting a comparable standalone
train/ticket-entry form (`/track`, for `TrackTrainForm`) already renders
that form full-width with no `Center`/`maw` wrapper. Copying the
`Center`+`maw` shell here would be importing a detail that solved a
problem specific to `CustomLineForm`, onto a component that doesn't have
that problem.

## Architecture

Before:

```
/track/mine                  Server Component
  ├─ Group: Title "My Trains & Tickets"  +  TextLink "Track a new train" -> /track
  ├─ trains list / empty state
  ├─ unattached-tickets section
  └─ <TicketEntryForm label="Add a ticket" />   (inline, collapsed by default, always at the bottom)
```

After:

```
/track/mine                  Server Component
  ├─ Group: Title "My Trains & Tickets"
  │         + Group: TextLink "Track a new train" -> /track
  │                   TextLink "Add a ticket"     -> /track/mine/add-ticket   NEW
  ├─ trains list / empty state                                                (unchanged)
  └─ unattached-tickets section                                               (unchanged)
       (no ticket-entry form rendered on this page any more)

/track/mine/add-ticket        Server Component, NEW
  ├─ getSession()                                                             NEW, proactive gate
  ├─ if !authenticated: Title + LoginLink                                     (mirrors /track/mine's own null-branch shape)
  └─ else:
       Stack p="lg" gap="md"                                                  (unconstrained, matches /track/page.tsx)
         Title "Add a ticket"
         TextLink "Back to My Trains & Tickets" -> /track/mine                NEW
         <TicketEntryForm label="Add a ticket" defaultOpen />                 (defaultOpen is NEW; everything else reused)

TicketPanel.tsx (two trackingId-scoped call sites)                            UNCHANGED
  <TicketEntryForm trackingId={...} label="Add a ticket for this journey" />
  <TicketEntryForm trackingId={...} label="Add another ticket" />
```

`TicketEntryForm` itself changes in exactly one place: a new optional
`defaultOpen?: boolean` prop seeds `open`'s initial value (Decision 4).
No other prop, no change to `handleSubmit`, no change to the
`savedStandaloneTicket` render branch, no change to the flat
`Train/tickets` vs. `Train/{trackingId}/tickets` routing logic.

## Error handling

- **The new page's own `getSession()` failure mode**: matches
  `TicketPanel.tsx:37`'s existing defensive posture — a
  `.catch(() => ({ authenticated: false, ... }))` fallback, so an
  auth-status glitch degrades to the login nudge rather than throwing and
  taking down the page via the root `app/error.tsx` boundary.
- **Form-level 401 on submit** (once past the proactive gate — e.g. a
  session that expires mid-visit): unchanged, `TicketEntryForm`'s own
  existing `needsLogin`/`LoginLink` handling (`TicketEntryForm.tsx:
  217-219, 375-379`) is reused verbatim, exactly as it already behaves on
  every other call site today.
- **Non-401 submit/upload failures** (400, 422, 504, 413, network error):
  unchanged — the form's existing per-status branches
  (`TicketEntryForm.tsx:148-176, 220-227`) are untouched by this move.
- **`/track/mine/add-ticket` itself has no `notFound()`/dynamic-segment
  failure mode to design**, same as the custom-line precedent's
  equivalent note for `/lines/new`: it takes no dynamic segment and, once
  past the session check, fetches nothing else server-side before
  rendering.
- **A visitor navigating directly to `/track/mine/add-ticket`** (bookmark,
  typed URL, or the fact that the redirect analysis above confirms no
  routing conflict exists): renders exactly the same page either way,
  gated only by the proactive session check (Decision 3) — no dependency
  on having arrived via the `/track/mine` entry-point link specifically.

## Testing

- **`frontend/app/track/mine/page.test.tsx`**: the existing `'renders the
  "Add a ticket" entry point'` test (lines 209-214) currently asserts
  `screen.getByRole('button', { name: 'Add a ticket' })` — this must
  change to a `link` query (`screen.getByRole('link', { name: 'Add a
  ticket' })`) asserting `href="/track/mine/add-ticket"`, since the entry
  point is no longer `TicketEntryForm`'s own collapsed button rendered
  in-place. Reusing the same test case's setup (empty trains/tickets) also
  gives a natural place to add the previously-missing assertion for
  "Track a new train"'s own `href="/track"` (no existing test covers it —
  Current relevant state, above — a small, free addition while this test
  is already being touched, not mandated separately). No other existing
  test in this file depends on the inline form's presence.
- **`frontend/components/TicketEntryForm.test.tsx`**: the standalone
  `describe('with no trackingId (standalone ticket)', ...)` block
  (lines 210-267) needs **no changes** — it exercises the component
  directly with the same props (`trackingId` omitted, `label="Add a
  ticket"`) regardless of which page renders it. One new test should be
  added for `defaultOpen`: rendering `<TicketEntryForm label="Add a
  ticket" defaultOpen />` (no `trackingId`, matching the new page's real
  call) shows the expanded manual-entry tab immediately, with no
  collapsed-button click required — the mirror image of the existing
  `'starts collapsed, showing only the entry-point button'` test
  (lines 52-57), confirming the new prop actually does what Decision 4
  claims and that the existing default (`false`) is preserved when the
  prop is omitted.
- **`frontend/app/track/mine/add-ticket/page.test.tsx`** (new file — no
  colocated test exists for this route today because the route doesn't
  exist yet; there is likewise no `frontend/app/lines/new/page.test.tsx`
  to compare against, since that sibling work's own spec explicitly left
  writing that page's test file as a later, unmandated implementation
  choice — this spec makes its own call independently rather than
  depending on that file existing). At minimum, following
  `page.test.tsx`'s existing `renderWithMantine` + mocked `next/navigation`
  convention (same `useRouter`/`usePathname`/`useSearchParams` stub
  `/track/mine/page.test.tsx:16-20` already uses, since this page also
  renders `TicketEntryForm`):
  - `getSession()` resolving `{ authenticated: false }`: renders the
    login-nudge shape, no `TicketEntryForm` markup present.
  - `getSession()` resolving `{ authenticated: true, ... }`: renders
    `<Title>Add a ticket</Title>`, the "Back to My Trains &amp; Tickets"
    link with `href="/track/mine"`, and `TicketEntryForm`'s expanded
    manual-entry fields **immediately visible with no click** (asserting
    `defaultOpen` is actually wired through from this call site, not just
    unit-tested in isolation on the component).
  - The rendered `TicketEntryForm` has no `trackingId` — assert (or reuse
    an existing helper from `TicketEntryForm.test.tsx`'s own standalone
    block's expectations) that a save posts to the flat `/api/Train/tickets`
    route, not a `trackingId`-scoped one, confirming this page's instance
    really is the standalone one and not an accidental regression to a
    scoped call.

## Explicitly out of scope

- **`TicketPanel.tsx`'s two `trackingId`-scoped `TicketEntryForm`
  instances** (`TicketPanel.tsx:74` and `:98`). Untouched by this spec in
  every respect: no route change (they stay embedded on
  `/train/by-id/[trackingId]` and `/train/[uid]/[date]`), no
  `defaultOpen` (they keep today's collapsed-by-default behavior, since
  the new prop defaults to `false` and neither call site passes it), no
  change to `TicketPanel`'s own `getSession()`-based gating. This is a
  narrower, different context — adding a ticket to an *already-tracked,
  specific* train, on a page full of other content about that one train —
  from `/track/mine/add-ticket`'s "I don't yet know which train this
  ticket is for" standalone case. This spec's `defaultOpen` addition is
  available to `TicketPanel` in principle but is deliberately not applied
  there; that would be a separate design decision about a page this spec
  doesn't touch.
- **Any change to `TicketEntryForm`'s upload logic, manual-entry fields,
  400/401/422/504/413 handling, or the flat-vs-scoped `ticketsBasePath`
  routing.** All reused completely unmodified.
- **Any change to `getMyTrackedTrains()`, `getMyTickets()`, or any backend
  route.** This is a frontend-only route/layout move plus one small,
  additive component prop; nothing in `crates/api` is touched.
- **The custom-line-creation-page-design spec's own `/lines/new` work.**
  Referenced throughout as precedent, not modified or depended on for
  correctness — this spec's decisions stand on their own even if that
  work's own open questions (e.g. whether `frontend/app/lines/page.test.
  tsx` gets added) resolve differently.
- **A session-aware treatment of the entry-point link itself on
  `/track/mine`.** Not applicable here the way it was a real decision for
  `/lines` (Decision 3, precedent): `/track/mine`'s entry-point `Group`
  (both links) is already fully hidden for a logged-out visitor via the
  page's own existing `trains === null` early return — no new gating
  logic is needed or proposed for the link itself, only for the
  destination page (Decision 3, this spec).

## Open questions / risks

1. **The new proactive `getSession()` check on `/track/mine/add-ticket`
   (Decision 3) is a real behavioral divergence from the custom-line
   precedent's reactive-only default**, chosen here because `/track/mine`
   itself is a stricter tier than `/lines`. If a future, broader policy
   pass unifies how Tier-2-and-stricter surfaces handle proactive
   session checks across this app, this page's specific choice should be
   revisited alongside it rather than treated as a permanently bespoke
   case.
2. **Copy bikeshedding.** "Add a ticket" (Decision 2) is a defensible,
   low-invention choice reusing an already-established name, but "Add a
   new ticket" (matching "Track a new train"'s grammar) is a reasonable
   alternative that was not validated with real users, same posture the
   precedent's own Open Questions took for its equivalent copy call.
3. **`defaultOpen`'s naming and default value are this spec's own
   invention** — no existing prop on `TicketEntryForm` or a sibling
   component in this codebase establishes a naming convention for
   "should this interactive component start expanded." Worth a second
   look at implementation time against how this app names comparable
   toggles elsewhere, if any turn up.
4. **The "Back to My Trains & Tickets" link duplicates, in a more
   specific form, what the global nav bar's `TrackedTrainsNavItem`
   already offers** (Decision 5). This is an intentional, precedented
   redundancy (mirroring `CustomLineForm`'s own `cancelHref`), not an
   oversight — but it is one more small link for a maintainer to keep in
   sync with `/track/mine`'s route if that ever changes.
5. **Whether `TicketPanel`'s own two instances should eventually also get
   `defaultOpen`** (e.g. "Add another ticket" pre-expanded, since a
   visitor clicking that specific button already committed to the
   action) is a real, adjacent question this spec's scope boundary
   deliberately leaves untouched (Explicitly out of scope, above) — flagged
   here as a plausible, independent follow-up, not designed further.
