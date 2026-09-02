# Design: Modal Login Prompt

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` (the
policy this spec revises one presentation layer of) and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(the closest recent precedent for a spec that both extends and knowingly
corrects a prior decision — see Corrections below). No implementation
plan is included; that is a separate, later step in this repo's process.

## Goal

Two related but distinct changes, both requested together because the
second depends on the first:

1. **Replace the inline `LoginLink` text prompt with a modal dialog** for
   Tier-2 interactions that fail with a real `401` — the anonymous-user-ux
   spec's own named examples of this pattern (pinning a line, creating a
   custom line) plus train tracking and ticket saving. Not every existing
   `LoginLink` call site fits this — this spec inventories all twelve real
   ones and makes a call, per site, on whether a modal is an improvement
   or a mismatch.
2. **Reclassify "My Trains & Tickets" from Tier 3 (hidden entirely for an
   anonymous session) to a Tier-2-shaped surface**: the nav link becomes
   always-visible, "to advertise the feature" to a logged-out visitor,
   with the new modal appearing when they actually click through and hit
   the real `401`. This directly reverses a specific, named prior
   decision — see Corrections.

## Corrections / relationship to prior specs

Following this repo's established "Corrections" precedent
(`2026-08-29-train-tracking-frontend-design.md`,
`2026-08-31-tracked-trains-list-design.md`,
`2026-09-01-tracked-trains-home-page-design.md`): one prior decision is
being knowingly reversed here, not overlooked.

**`2026-08-31-tracked-trains-list-design.md`'s Decision 4 (lines 467–478)
already considered and explicitly rejected "always-visible-but-login-gated"
for this exact nav link**, choosing "hidden entirely for a logged-out
visitor" instead, with this reasoning (quoted in full since the reversal
turns on it): *"not always-visible-but-login-gated (the way `TicketPanel`
degrades a section of an already-public page), because this is a full
nav-bar entry point to a page whose entire content is private to the
viewer; showing it to every visitor and having it always resolve to a
login nudge would be dead weight in the nav bar for the (likely common)
case of an anonymous visitor."* That reasoning is reproduced verbatim in
the shipped code's own comment, `frontend/app/layout.tsx:88–101`, directly
above `TrackedTrainsNavItem` (confirmed by reading the file: *"this is a
full nav-bar entry point to a page whose entire content is private to the
viewer, not a section of an already-public page (the `TicketPanel`
pattern), so showing it to every visitor and having it always resolve to
a login nudge would be dead weight... for the common case of an anonymous
visitor"*), and `TrackedTrainsNavItem` (`frontend/app/layout.tsx:108–119`)
implements exactly that: `if (!session.authenticated) { return null; }`.
Three tests in `frontend/app/layout.test.tsx:13–35` pin this behavior
down explicitly (`'hides "My Trains & Tickets" when logged out'`,
`'shows... when logged in'`, `'degrades to hidden... when getSession
rejects'`).

**What's changing, and why this doc treats the original reasoning as
correct-at-the-time rather than wrong:** the original decision's concern
was real — an always-visible link to a fully private page *would* be
"dead weight" if hitting it just silently failed or dumped a bare `401`
page with no recourse. That concern is answered, not dismissed, by change
1 above: once the anonymous 401 outcome is a real, actionable modal
("here's why, here's the button to fix it") rather than a plain inline
sentence at the top of an otherwise-empty page, "dead weight" is no
longer an accurate description of what a logged-out click produces. The
user's own stated reason for wanting the reversal is discoverability —
"to advertise the feature" — which the original decision never weighed
against, because at the time (a) the feature had less to advertise, being
the first tracked-train UI in the app, and (b) no modal-quality login
prompt existed yet to make the always-visible version non-dead-weight.
Both conditions have changed: the app now also has custom lines, tickets,
and a home-page tracked-trains section (per
`2026-09-01-tracked-trains-home-page-design.md`) that all benefit from
the same discoverability argument, and this spec is what supplies the
modal. This is not "the original spec was wrong" — it's "the tradeoff it
correctly evaluated at the time comes out differently now that one of its
own inputs (login-prompt quality) has changed." See Decision 6 below for
the concrete mechanism.

## Current relevant state (verified 2026-09-02)

### The two shared primitives

- **`useNeedsLogin()`** (`frontend/components/useNeedsLogin.ts:21–31`, read
  in full): `{ needsLogin, reset, markNeedsLogin }` — a bare `useState`
  wrapper, nothing else. Its own doc comment (lines 5–20) states the
  design intent explicitly: *"Deliberately minimal: does not wrap the
  fetch call itself... Just the state a caller resets at the start of
  every attempt and sets when a response comes back 401."*
- **`LoginLink`** (`frontend/components/LoginLink.tsx:28–44`, read in
  full): a Client Component wrapping `TextLink` that appends the current
  `usePathname()`/`useSearchParams()` as a `return_to` query parameter on
  `/api/auth/login`, per
  `docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md`.
  Takes `children` as the link's visible text (no `verb` prop — that
  shape was retired) and an optional `underline` prop passed through to
  `TextLink`.
- **`LoginPrompt.tsx`** (the `verb`-prop wrapper `useNeedsLogin`'s own doc
  comment still references) **no longer exists.** Confirmed via `git log`:
  introduced in `7d04aed` ("Extract shared useNeedsLogin/LoginPrompt,
  retrofit CustomLineForm and DeleteLineButton"), deleted in `584e1d1`
  ("Consolidate remaining login prompts onto LoginLink") earlier this
  session, which moved every remaining call site directly onto `LoginLink`
  with call-site-specific `children` text instead. `LoginPromptModal` (the
  name this spec uses for the new component — see Decision 1) does not
  collide with anything currently in the tree.

### Full call-site inventory: every `LoginLink`/`useNeedsLogin` usage in `frontend/`

`grep -rl "useNeedsLogin"` (excluding the hook's own definition and test
file) returns exactly three files: `DeleteTrainButton.tsx`,
`DeleteLineButton.tsx`, `app/lines/CustomLineForm.tsx`. Three other
Tier-2 components — `PinToggle.tsx`, `TrackTrainForm.tsx`,
`TicketEntryForm.tsx` — still hand-roll their own local `useState(false)`
for the identical `needsLogin` boolean rather than using the shared hook;
the anonymous-user-ux spec's §Reusable pattern explicitly declined to
force that migration on the two that predated the hook (`PinToggle`,
`TrackTrainForm`), so this is pre-existing, known drift, not a new
finding.

`grep -rl "LoginLink"` (excluding `LoginLink.tsx` itself and every
`*.test.tsx`) returns twelve real call sites. All twelve, read in full
this session:

| # | Call site | Kind | Trigger | Current copy |
|---|---|---|---|---|
| 1 | `components/PinToggle.tsx:125` | Client, hand-rolled `useState` | Reactive 401 on pin/unpin `PUT` | "Log in to pin" |
| 2 | `app/lines/CustomLineForm.tsx:244–248` | Client, `useNeedsLogin` | Reactive 401 on create/edit `POST`/`PUT` | "Log in to {create/edit} a line" |
| 3 | `components/TrackTrainForm.tsx:194–198` | Client, hand-rolled `useState` | Reactive 401 on track `POST` | "Log in to track this train" |
| 4 | `components/TicketEntryForm.tsx:375–379` | Client, hand-rolled `useState` | Reactive 401 on ticket save/upload | "Log in to save this ticket" |
| 5 | `components/DeleteLineButton.tsx:69–73` | Client, `useNeedsLogin`, **inside an existing `Modal`** | Reactive 401 on delete, from within the confirm dialog | "Log in to delete a line" |
| 6 | `components/DeleteTrainButton.tsx:79–83` | Client, `useNeedsLogin`, **inside an existing `Modal`** | Reactive 401 on delete, from within the confirm dialog | "Log in to delete this tracked train" |
| 7 | `components/TicketPanel.tsx:55–59` | Server Component | Proactive: `getSession()` already fetched, unauthenticated branch replaces the whole panel | "Log in to attach a ticket to this journey" |
| 8 | `app/train/by-id/[trackingId]/page.tsx:41–43` | Server Component | `ApiUnauthorizedError` catch, replaces the whole page body | "Log in to view this tracked train" |
| 9 | `app/train/[uid]/[date]/page.tsx:42–44` | Server Component | Same as #8, sibling route | "Log in to view this tracked train" |
| 10 | `app/track/mine/page.tsx:53–55` | Server Component | `getMyTrackedTrains()` returns `null` on 401, replaces the whole page body | "Log in to see the trains and tickets you're tracking" |
| 11 | `app/page.tsx:147` | Server Component | Proactive: `getSession()` already fetched (home-page redesign), rendered unconditionally in a CTA `Group` alongside "Browse all lines"/"Look up a station" | "Log in to pin your lines and stations" |
| 12 | `components/AuthStatus.tsx:21` | Server Component | Unconditional: the nav bar's permanent, static "you are not logged in" affordance, not tied to any failed action | "Log in" |

Two Mantine `Modal` usages already exist in this codebase as the direct
precedent for this spec's own modal:
`components/DeleteLineButton.tsx:29,66–83` and
`components/DeleteTrainButton.tsx:39,76–93`, both read in full. Both use
`useDisclosure(false)` for `opened`/`open`/`close`, a controlled `<Modal
opened={opened} onClose={close} title="...">`, a `Group justify="end"
mt="md"` footer with a `variant="default"` Cancel button and a primary
action button, and — load-bearing for #5/#6 above — **the existing
`needsLogin` `LoginLink` already renders as plain inline text *inside*
that same `Modal`'s body**, not as a separate dialog of its own.

## Decisions

### 1. New component: `LoginPromptModal`, a thin presentational wrapper — the hook itself is unchanged

Per `useNeedsLogin`'s own documented minimalism ("just the state a caller
resets/sets"), **the hook's shape does not change.** `needsLogin` is
already exactly the boolean a controlled `Modal`'s `opened` prop needs;
inventing a second, modal-specific piece of state alongside it would be
pure duplication for no new information. `reset()` is already exactly
what a `Modal`'s `onClose` needs to call. Nothing about the hook's
`{ needsLogin, reset, markNeedsLogin }` contract requires touching.

What's new is a small, purely presentational Client Component,
`LoginPromptModal`, that a call site renders unconditionally (mirroring
`DeleteLineButton`/`DeleteTrainButton`'s own `<Modal opened={opened}
onClose={close}>` — Mantine's `Modal` already no-ops visually when
`opened` is `false`, so call sites don't need their own `{needsLogin &&
...}` conditional the way the inline `LoginLink` version required):

```
<LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
  Log in to pin this line.
</LoginPromptModal>
```

Internally it renders a Mantine `Modal` with a fixed title ("Log in
required" — see Decision 2 for why the title doesn't vary), the
`children` as body prose, and a primary "Log in" button whose `href` is
built the same way `LoginLink` already builds one — `usePathname()` +
`useSearchParams()` joined into `return_to`
(`frontend/components/LoginLink.tsx:35–38`). To avoid that three-line
calculation existing in two places and drifting (the exact failure mode
`LoginLink`'s own doc comment already worries about for a different
reason — capturing the *current* page reliably), this spec recommends
extracting it into a tiny shared `useLoginHref()` hook that both
`LoginLink` and `LoginPromptModal` call, rather than either duplicating
it or `LoginPromptModal` importing/rendering a `LoginLink` styled to look
like a button (which it structurally isn't — Mantine's `Modal` primary
action here is a `Button`, and this codebase already has an established,
different pattern for "a button that navigates," per Decision 3).

This keeps the churn at existing call sites to exactly what it needs to
be: swap the conditional `LoginLink` block for an unconditional
`LoginPromptModal` block, no change to how `needsLogin`/`reset`/
`markNeedsLogin` are obtained or called.

### 2. Message shape: fixed generic title, call-site-specific body text via `children` — not a `verb` prop

Two options were weighed:

- **A generic modal with a customizable `verb`/`reason` prop** (reviving
  something close to the retired `LoginPrompt`'s `verb` prop). Rejected:
  this session already deliberately retired that exact shape in favor of
  plain `children` (`584e1d1`, "Consolidate remaining login prompts onto
  LoginLink") specifically because a bare verb couldn't express every
  call site's actual phrasing (`CustomLineForm`'s "edit" vs. "create" a
  line is itself already a ternary inside the string, not a single
  fixed verb). Re-introducing a narrower prop shape than `children`
  would be a step backward from a change made deliberately, this same
  session.
- **`children` as prose body text, exactly mirroring `LoginLink`'s
  existing flexibility, with one fixed modal `title`.** **Chosen.** Every
  migrating call site's existing `LoginLink` text becomes the modal's
  body sentence with minimal rewording (see the table in Decision 4);
  the modal's `title` is a single constant, `"Log in required"`, not a
  prop — a second per-call-site string alongside `children` would just
  restate the same information the body sentence already carries (an
  itemized survey of the five migrating messages found no case where a
  distinct title would add information a reader doesn't already get from
  the body line), so it's one fewer thing every call site has to supply,
  consistent with `DeleteLineButton`/`DeleteTrainButton`'s own modals,
  which likewise use a fixed, situation-describing `title` string per
  component rather than a templated one.

### 3. The modal's login button: `Link`-wrapping-`Button`, not a Mantine `component` prop

`CustomLineForm.tsx:258–263`'s existing Cancel button is the established
precedent for "a Mantine `Button` that navigates": a plain `<Link
href={cancelHref} style={{ textDecoration: 'none' }}><Button
type="button">Cancel</Button></Link>`, not `component={Link}` on the
`Button` itself. `app/layout.tsx`'s own comment (referenced by
`TextLink.tsx`'s doc comment) explains why: passing the `Link` component
reference into a Mantine polymorphic `component` prop from a Server
Component previously broke `next build`'s Server/Client boundary
serialization check. `LoginPromptModal` is a Client Component, so that
specific failure mode may not apply here — but `CustomLineForm` is
*already* a Client Component and still uses the wrapping form, so this is
the established convention regardless of component type, not a
workaround scoped only to Server Components. `LoginPromptModal`'s login
button follows the same wrapping pattern: `<Link href={loginHref}
style={{ textDecoration: 'none' }}><Button>Log in</Button></Link>`.

### 4. Migration call: four client-side reactive controls move to the modal

Reasoning per site, grouped by why they do or don't fit:

**Migrate — small/dense control, or a multi-field form where inline text
disrupts layout:**

- **`PinToggle.tsx`** (#1). The star renders inline inside table rows
  (`/lines`) and card rows (`/stations/[crs]`, the home page's pinned
  sections). Inline text next to a star in a dense row either wraps
  awkwardly or pushes the row taller, and gives very little room for a
  real call-to-action. A modal keeps every row's height stable regardless
  of auth state and gives the prompt real space. Body text: `"Log in to
  pin this {kind}."` (`kind` is already a `PinKind`, `'line' | 'station'`,
  threaded through the existing component).
- **`CustomLineForm.tsx`** (#2). Named explicitly by the brief as a
  target. Today's inline `LoginLink` (line 244) sits between the error
  text and the Save/Cancel row inside a `maw={480}` form — a modal removes
  a conditionally-appearing element from that vertical flow entirely
  rather than shifting the buttons below it up and down as `needsLogin`
  toggles. Body text: `"Log in to {edit/create} a custom line."`
- **`TrackTrainForm.tsx`** (#3). Same shape as `CustomLineForm`: a
  `Button` + conditional `LoginLink` inside one `Group` (line 190–199).
  The component's own doc comment stresses "no navigation away" (Decision
  4 of the train-tracking-frontend spec) specifically so the four typed
  fields aren't lost — a modal satisfies this at least as well as the
  inline text does: it overlays the page without unmounting the form or
  its state, and Mantine's `Modal` doesn't navigate anywhere on its own.
  Migrating this alongside `CustomLineForm` also keeps the two
  structurally-identical "Button + inline prompt in a `Group`" call sites
  consistent with each other rather than one becoming a modal and the
  other staying text for no principled reason. Body text: `"Log in to
  track this train."`
- **`TicketEntryForm.tsx`** (#4). Same `Button`/`Group` shape again
  (lines 368–380), same reasoning as `TrackTrainForm`. Body text: `"Log
  in to save this ticket."`

**Stay inline — already inside a `Modal`, or not a reactive failure at
all:**

- **`DeleteLineButton.tsx` / `DeleteTrainButton.tsx`** (#5, #6). **Not
  migrated — deliberately.** Both already render their `needsLogin`
  prompt *inside* an existing confirm `Modal` (Current relevant state,
  above). Nesting a second `Modal` inside an already-open one is real,
  avoidable complexity (Mantine has no built-in two-modal stacking here;
  it would need `zIndex` management or the separate `ModalsStack` API,
  neither used anywhere in this codebase today) to solve a problem that
  doesn't exist at these two sites — the "dense row, no room" justification
  driving the four migrations above doesn't apply inside a `Modal`, which
  already has generous space. Left exactly as-is: inline `LoginLink` text
  inside the existing confirm dialog.
- **`TicketPanel.tsx`** (#7), **`train/by-id/[trackingId]/page.tsx`**
  (#8), **`train/[uid]/[date]/page.tsx`** (#9). All three are Server
  Component, whole-content-replacement prompts — Tier 3's "replace with
  an explicit login state" sub-case, not Tier 2's "control that just
  failed." There is no separate control on these pages that "worked" and
  then didn't; the entire returned content *is already* the prompt, on
  its own otherwise-blank page/section. A modal on top of a page whose
  only content is already the login message adds a dismiss/backdrop
  interaction with nothing behind it to reveal, which is strictly worse
  than the plain heading + inline sentence these three already show. None
  of the three were named in the brief's target list, and none share
  `PinToggle`/`CustomLineForm`/`TrackTrainForm`/`TicketEntryForm`'s "modal
  keeps a busy layout stable" justification. Left unchanged.
- **`app/page.tsx`** (#11). Not a reactive 401 at all — it's a proactive
  advertisement rendered because the home page already fetches session
  (per the anonymous-user-ux redesign), sitting in a `Group` next to
  "Browse all lines"/"Look up a station" as a peer nav-style link, not as
  a response to a failed click. Popping a modal for a link nobody clicked
  yet would be actively wrong UX. Left unchanged.
- **`AuthStatus.tsx`** (#12). The brief's own example of what shouldn't
  migrate, confirmed correct on inspection: this is the nav bar's
  permanent "Log in" link, rendered because `!session.authenticated`, not
  because anything was attempted and rejected. A modal here would have no
  triggering action to explain itself — clicking it *is* the intended
  action (go log in), not a thing that failed. Left unchanged.

**Special case — reclassified, see Decision 6:**

- **`app/track/mine/page.tsx`** (#10). Not part of the four "control
  fits a modal better" migrations above (it's a Server Component
  whole-page prompt, structurally identical to #7–#9) — but it migrates
  anyway, for a different reason tied to the Tier 3→2 reclassification of
  the nav link that leads here. See Decision 6.

Net: **5 of 12** sites move to `LoginPromptModal` (4 client controls +
`/track/mine`'s page-level prompt, migrated for a distinct reason); **7
of 12** stay exactly as they are today, each for a stated, call-site-
specific reason rather than a blanket "leave the rest alone."

### 5. Opportunistic cleanup: the three hand-rolled `useState` sites adopt `useNeedsLogin()` while they're being touched anyway

`PinToggle.tsx`, `TrackTrainForm.tsx`, `TicketEntryForm.tsx` each still
hand-roll their own `needsLogin`/`setNeedsLogin` instead of using the
shared hook (Current relevant state). The anonymous-user-ux spec's own
§Reusable pattern explicitly declined to force this migration as a
standalone change — but this spec's migration already requires editing
every line in all three files that reads or sets `needsLogin` (to
render `LoginPromptModal` instead of the inline conditional), so
finishing the hook consolidation at the same time costs nothing extra
and closes a gap that doc named but left open. Not treated as
load-bearing for this spec's actual goal (the modal) — if an
implementation plan finds a reason to defer it, dropping it doesn't
change anything else in this design.

### 6. `/track/mine`'s Tier 3→2 reclassification: nav link always visible, page navigates normally, page-level prompt becomes an auto-opened modal

Two mechanisms were weighed for how an anonymous click reaches the
login prompt:

- **Client-side interception before navigation** — the nav link becomes
  a Client Component that checks session state (or just always shows a
  modal-trigger) and only navigates once "logged in" is confirmed.
  **Rejected.** This app's nav bar is deliberately Server-Component-first
  — `TrackedTrainsNavItem` today is `async`, reads `getSession()` once,
  server-side (`app/layout.tsx:108–119`), matching the sibling
  `AuthNavItem`/`DataFreshnessNavItem` pattern in the same file. Turning
  the link itself into a client-side gate would mean fetching session
  *twice* for the same nav bar render (once here, once in `AuthNavItem`)
  or threading session state between two independent `Suspense`
  boundaries — real new plumbing to avoid one otherwise-ordinary page
  navigation.
- **Nav link always renders a plain `<TextLink href="/track/mine">`;
  `/track/mine`'s own existing server-side 401 branch (`getMyTrackedTrains()`
  returning `null`) is what actually decides whether to show the
  prompt**, exactly as it already does today for anyone reaching the URL
  directly. **Chosen.** `TrackedTrainsNavItem` (`app/layout.tsx:108–119`)
  loses its `if (!session.authenticated) return null;` branch and its
  `getSession()` call entirely — since the page itself already does the
  real gating via `getMyTrackedTrains()`'s `null`-on-401 return
  (`app/track/mine/page.tsx:47,49`), the nav item has no remaining reason
  to fetch session at all. It becomes an unconditional `<TextLink
  href="/track/mine">My Trains &amp; Tickets</TextLink>`, and — since it no
  longer awaits anything — it no longer needs its own `<Suspense>`
  boundary either (`app/layout.tsx:166–168`'s `<Suspense
  fallback={null}><TrackedTrainsNavItem /></Suspense>` collapses to a
  plain synchronous child, matching how `"All Lines"`/`"Station Lookup"`
  already render as plain `TextLink`s two lines above it).

  This is simpler, not just more consistent: no new client-side auth
  check, no duplicate `getSession()` call, and it reuses a 401 branch
  that already exists and is already tested
  (`app/track/mine/page.test.tsx`, per the tracked-trains-list spec's own
  Testing section).

`/track/mine`'s existing 401 branch (`app/track/mine/page.tsx:49–57`)
then upgrades from a plain inline `LoginLink` to `LoginPromptModal` —
but pre-opened, not click-triggered, since there is no client-side click
event on a Server Component to open it from. This needs one small new
piece: a local Client Component wrapper (scoped to this page, the same
way `TrackedTrainListRow`/`RowStatusBadge` are already local,
non-exported helpers inside this same file) that holds `useState(true)`
for `opened` and passes `onClose={() => setOpened(false)}` — i.e.
`LoginPromptModal` itself stays fully controlled (matching
`DeleteLineButton`/`DeleteTrainButton`'s controlled-`Modal` convention
exactly), and this page supplies the one piece of "starts open" behavior
itself, rather than `LoginPromptModal` growing a `defaultOpened` prop
that only one of its five call sites would ever use.

Closing the modal (Escape, backdrop, or its own close control — all
handled by Mantine's `Modal` for free, see Decision 7) leaves the page
showing just its `"My Trains & Tickets"` heading and nothing else — no
worse than today's "heading + inline sentence" once the modal is
dismissed, and the modal is trivially reachable again via the nav link
(a fresh navigation to `/track/mine` reopens it, since `opened` re-
initializes to `true` on every fresh mount of the page).

## Architecture

```
Migrating (5 sites) ─────────────────────────────────────────────
  PinToggle / CustomLineForm / TrackTrainForm / TicketEntryForm
    useNeedsLogin() ──► { needsLogin, reset, markNeedsLogin }  (UNCHANGED)
    <LoginPromptModal opened={needsLogin} onClose={reset}>
      {contextual body text}
    </LoginPromptModal>

  app/track/mine/page.tsx (Server Component)
    getMyTrackedTrains() -> null on 401  (UNCHANGED, existing gate)
      -> local client wrapper, useState(true)
           <LoginPromptModal opened onClose={() => setOpened(false)}>
             "Log in to see the trains and tickets you're tracking."
           </LoginPromptModal>

LoginPromptModal (NEW, frontend/components/)
  'use client'
  Mantine <Modal title="Log in required" opened onClose>
    {children}                      <- prose, per call site
    <Link href={useLoginHref()}>    <- shared with LoginLink, NEW extraction
      <Button>Log in</Button>
    </Link>
  </Modal>

Staying inline (7 sites) ─────────────────────────────────────────
  DeleteLineButton / DeleteTrainButton   (LoginLink text inside their OWN existing Modal)
  TicketPanel / train/by-id / train/uid-date   (Tier-3 whole-content-replace prompts)
  app/page.tsx home CTA                  (proactive nav-style link, not reactive)
  AuthStatus.tsx                         (static "Log in", not tied to any action)

app/layout.tsx ────────────────────────────────────────────────────
  TrackedTrainsNavItem: loses getSession()/null-branch/Suspense,
  becomes an unconditional <TextLink href="/track/mine">
```

## Error handling

- No new error paths are introduced anywhere. Every migrating site's
  `401` detection, `reset()`/`markNeedsLogin()` call, and non-401 error
  handling (the generic `response.text()` fallback in `CustomLineForm`/
  `DeleteLineButton`/`DeleteTrainButton`, the field-error `Alert`s in
  `TrackTrainForm`/`TicketEntryForm`) is unchanged — only the rendering
  of the already-existing `needsLogin`/`null` outcome changes, from
  inline text to a modal.
- `LoginPromptModal` itself has no fetch or fallible logic of its own —
  it is presentation only, same posture as `useNeedsLogin`'s own
  "deliberately minimal" design (Decision 1).
- `/track/mine`'s auto-opened modal has one new, real edge case worth
  naming: if the visitor closes it without logging in, the page behind
  it is bare (a heading, no body content, no fallback sentence). This is
  a deliberate simplification (Decision 6) rather than an oversight, but
  it's the one place this spec accepts a slightly emptier resting state
  than today's page has — flagged again in Open questions/risks.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest, `useDisclosure`/`Modal` testing precedent
already established in `DeleteLineButton.test.tsx`/
`DeleteTrainButton.test.tsx`):

- **New `LoginPromptModal.test.tsx`**: renders nothing meaningfully
  interactive when `opened={false}` (mirrors how `DeleteLineButton.test.tsx`
  presumably already asserts its own `Modal`'s closed state, to be
  confirmed at implementation time); when `opened={true}`, the title
  ("Log in required"), the passed `children` body text, and a "Log in"
  link/button with the correct `return_to`-bearing `href` (same URL-
  encoding assertions `LoginLink.test.tsx:14–52` already makes, reused
  for the extracted `useLoginHref()`); calls `onClose` when the built-in
  close affordance fires.
- **`useNeedsLogin.test.ts`**: unchanged — the hook's contract doesn't
  change (Decision 1), so its three existing assertions
  (`useNeedsLogin.test.ts:6–19`) need no edits.
- **`PinToggle.test.tsx` / `TrackTrainForm.test.tsx` /
  `TicketEntryForm.test.tsx`**: any existing assertion of the form
  `expect(screen.getByRole('link', { name: 'Log in to ...' }))` (matching
  the old inline `LoginLink` text) is replaced with an assertion that
  `LoginPromptModal` is rendered `opened` with the right body text after
  a 401 — plus, if Decision 5's hook migration lands alongside this, an
  update to however each file's local `needsLogin` state was previously
  mocked/asserted, to instead exercise `useNeedsLogin()`'s real
  `reset`/`markNeedsLogin`.
- **`CustomLineForm.test.tsx`**: same shape of change as above; already
  uses `useNeedsLogin`, so no hook-usage test changes, only the rendered-
  output assertion.
- **`DeleteLineButton.test.tsx` / `DeleteTrainButton.test.tsx`**: **no
  change** — these two are explicitly not migrating (Decision 4), so
  their existing inline-`LoginLink`-inside-`Modal` assertions stay valid
  as-is.
- **`app/layout.test.tsx`**: the three `TrackedTrainsNavItem` tests
  (`layout.test.tsx:13–35`) change meaningfully — `'hides ... when logged
  out'` and `'degrades to hidden ... when getSession rejects'` no longer
  apply once the component stops calling `getSession()` at all (Decision
  6); both should be replaced with a single assertion that the link
  renders unconditionally, and the `vi.mock('@/lib/api')`/
  `api.getSession` mocking this describe block currently sets up
  (`layout.test.tsx:7,15,25,31`) can likely be dropped entirely for this
  component once it no longer calls that function.
- **`app/track/mine/page.test.tsx`**: the existing `null`-from-
  `getMyTrackedTrains()` test (whatever currently asserts the inline
  `LoginLink`, per the tracked-trains-list spec's own Testing section
  citing "login nudge") updates to assert the new local client wrapper
  renders `LoginPromptModal` pre-`opened`, with the page's contextual
  body text.

## Explicitly out of scope

- **Any change to `useNeedsLogin`'s public shape.** Decision 1 keeps it
  exactly as documented; this spec adds a consumer, not a new capability.
- **Migrating `DeleteLineButton`/`DeleteTrainButton`'s login prompt to a
  nested modal.** Decision 4 explicitly keeps these two inline, inside
  their existing confirm dialog.
- **Migrating `TicketPanel`, `train/by-id/[trackingId]`, or
  `train/[uid]/[date]`'s whole-page/whole-panel 401 prompts to a modal.**
  Decision 4 keeps all three as Tier-3 "explicit login state" page
  content, unchanged.
- **The `isOwner` backend addition and custom-line Edit/Delete
  visibility gating** — a real, separate, already-recommended change from
  the anonymous-user-ux spec's own Policy §Tier 3, orthogonal to this
  spec's modal work and not touched here.
- **Any redesign of the home page's anonymous "Right now" widget or CTA
  row** (`app/page.tsx`'s existing proactive `LoginLink`, #11 in the
  inventory) beyond confirming it correctly stays inline. Not a target of
  either requested change.
- **A general "confirm you want to leave `/track/mine` unauthenticated"
  flow, or any redirect-away behavior for the auto-opened modal's closed
  state.** Decision 6 accepts a bare page behind the closed modal as a
  deliberate simplification, not a gap this spec tries to close further.

## Open questions / risks

1. **The bare-page-behind-closed-modal state on `/track/mine`
   (Decision 6)** is a real, if minor, regression from today's "heading +
   permanent inline sentence" — a visitor who dismisses the modal has
   strictly less visible content than before, until they navigate again.
   Worth revisiting if this turns out to read as broken in practice; a
   fallback inline sentence rendered *behind* the modal (so closing it
   reveals the old text) was considered and set aside here as unnecessary
   complexity for a first pass, not ruled out permanently.
2. **`useLoginHref()`'s extraction (Decision 1/3) is new, if small,
   shared surface area** — `LoginLink.tsx` and `LoginPromptModal.tsx`
   both depending on it means a bug in the `return_to` calculation now
   affects both instead of just one. This is the same trade every shared-
   hook extraction in this codebase already makes (see `useNeedsLogin`
   itself, or `useSuggestions`) and isn't treated as a new category of
   risk, but is worth testing the extracted hook directly (per Testing,
   above) rather than only indirectly through each consumer.
3. **Whether Decision 5's opportunistic `useNeedsLogin()` migration for
   `PinToggle`/`TrackTrainForm`/`TicketEntryForm` should actually be
   bundled into the same implementation pass as the modal, or deferred as
   its own smaller follow-up.** Not blocking either way (Decision 5), but
   flagged as a real scoping choice for whoever writes the implementation
   plan, since it touches the same lines for an unrelated reason.
4. **This spec does not re-verify Mantine's focus-trap/return-focus
   defaults from source** — it relies on Mantine's own current docs
   (`trapFocus`/`returnFocus`/`closeOnEscape`, all recommended-on by
   default, fetched from mantine.dev/core/modal/ this session) plus this
   codebase's own two years-old-pattern working `Modal` usages
   (`DeleteLineButton`/`DeleteTrainButton`) as evidence it already works
   correctly here. If Mantine's actual shipped defaults in `9.5.2`
   (the version pinned in `frontend/package.json`) ever diverge from its
   current docs, that's a pre-existing risk this spec inherits rather
   than introduces — no new accessibility work is scoped here beyond
   what `Modal` already provides for free.
