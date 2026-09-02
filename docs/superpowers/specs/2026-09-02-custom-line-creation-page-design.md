# Design: Custom Line Creation Page

**Status: design proposal, not approved.** No implementation plan or code
changes are included — that is a separate, later step in this repo's
process.

## Goal

`frontend/app/lines/page.tsx` (the "All Lines" page, read in full) renders
`<CustomLineForm />` inline at the very bottom of the page, under a "New
Custom Line" `<Title order={2}>` heading, always fully expanded — a visitor
has to scroll past the entire lines table to reach it:

```
frontend/app/lines/page.tsx:17-29
  <Stack p="lg" gap="xl">
    <Stack gap="md">
      <Title order={1}>All Lines</Title>
      <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} tocs={tocs} />
    </Stack>

    <Stack gap="md">
      <Title order={2}>New Custom Line</Title>
      <CustomLineForm />
    </Stack>
  </Stack>
```

This spec moves creation to its own dedicated route, with a link/entry
point placed at the top of `/lines` instead of the form itself, following
the real, already-shipped precedent for this exact shape:
`frontend/app/lines/[id]/edit/page.tsx`, which renders the same
`CustomLineForm` (in edit mode) on its own route wrapped in
`<Center><Stack maw={480}>`, with a `cancelHref` back to the line's detail
page.

## Current relevant state (verified 2026-09-02)

- **`frontend/app/lines/page.tsx`** (read in full, above): two stacked
  sections, "All Lines" (`order={1}`) then "New Custom Line" (`order={2}`
  + inline `<CustomLineForm />`), no `Group`/entry-point pattern at all
  today.
- **`frontend/app/lines/[id]/edit/page.tsx`** (read in full): the exact
  layout precedent this spec follows —

  ```
  frontend/app/lines/[id]/edit/page.tsx:23-35
  <Center>
    <Stack p="lg" gap="md" maw={480} w="100%">
      <Title order={1}>Edit: {line.name}</Title>
      <CustomLineForm existingLine={line} cancelHref={`/lines/${id}`} />
    </Stack>
  </Center>
  ```

  Its own comment (lines 24-27) explains the `maw={480}` is chosen
  specifically to match `CustomLineForm`'s own `maw={480}` (confirmed at
  `frontend/app/lines/CustomLineForm.tsx:165`), so the heading lines up
  with the form's edges. **No colocated `page.test.tsx` exists for this
  route** — `find frontend/app/lines/[id]/edit -type f` returns only
  `page.tsx`; the only test file that exercises this route's rendering is
  `frontend/app/lines/CustomLineForm.test.tsx`, which mounts
  `CustomLineForm` directly (not the page) and stubs `usePathname()` to
  `/lines/my-line/edit` for its 401 test.
- **`frontend/app/lines/CustomLineForm.tsx`** (read in full):
  - `cancelHref?: string` prop (line 30): when given, renders a `Cancel`
    `<Link>`+`<Button variant="default">` beside the submit button in a
    `<Group justify="flex-end">` (lines 249-267); when omitted, the submit
    button alone keeps the `Stack`'s full width (comment, lines 17-18) —
    exactly the two shapes create-page and edit-page respectively need,
    with no new prop work required.
  - `existingLine` absent = create mode: `POST /api/lines`; present = edit
    mode: `PUT /api/lines/{id}` (lines 108-109).
  - Already has full `useNeedsLogin`/`LoginLink` 401 handling (line 41,
    120-121, 244-248) — this landed as the fix for
    §Correction 3/Policy Tier 2 item 2 of
    `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md`
    (confirmed: `useNeedsLogin.ts`'s own doc comment cites that spec by
    name and says `CustomLineForm.tsx`/`DeleteLineButton.tsx` are two of
    the hook's three call sites). Nothing about this spec's move changes
    that — the form keeps 100% of its own submit-time auth handling
    regardless of what route it's rendered on.
  - **The create-mode success branch** (lines 128-157) is the "stuck
    loading" workaround this session already shipped, with its own
    detailed comment explaining why: creating a line calls
    `router.push('/lines')` — **the same route the form is already
    rendered on today** — so App Router doesn't remount the component and
    `submitting` is manually reset (`setSubmitting(false)`) and every
    field is manually cleared (lines 149-155) to leave the form usable for
    another entry, "matching what a remount would have produced anyway."
    The **edit-mode** branch (lines 129-133) needs no such workaround
    because it already navigates to a *different* route
    (`/lines/${existingLine.id}`), which remounts for free — its own
    comment says so explicitly.
  - `frontend/app/lines/CustomLineForm.test.tsx:264-296`,
    `'a successful create resets the submit button out of its loading
    state and clears the form'`, is the regression test locking in that
    exact workaround. Its own comment (lines 264-276) states the premise
    plainly: *"creating a line navigates back to `/lines`, the very route
    this form is already rendered on... unlike the edit flow... nothing
    here ever reset `submitting` back to `false`"* — a premise this spec's
    route move invalidates for the create path (see Decision 4).
- **`crates/api/src/routes/lines.rs`** (read in full) and
  **`crates/api/src/data/custom_lines.rs`** (`slugify`, read in full):
  `create_line` (lines 213-258) derives a custom line's `id` via
  `custom_lines::insert_custom_line`, which slugifies the submitted
  `name`. `slugify` (lines 20-36) is unconditional about its prefix:

  ```
  crates/api/src/data/custom_lines.rs:35
  format!("custom-{slug}")
  ```

  Every custom line id is therefore always `custom-<slug>` — there is no
  code path, for any submitted name, that produces a bare id with no
  `custom-` prefix. A custom line with the literal id `"new"` is not
  constructible through this route. (`create_line` does separately reject
  a name that slugifies to an empty body — `slugify(&req.name) ==
  "custom-"` at line 230 — which is an unrelated guard against
  all-punctuation names, not a collision concern.)
- **App Router structure** (`find frontend/app -maxdepth 2 -type d`):
  `frontend/app/lines/` today contains only `page.tsx` (static) and
  `[id]/` (dynamic segment) — no `new/` directory exists yet, so adding
  one introduces no naming conflict with anything already in the tree.
  This app already has one directly analogous static-vs-dynamic-segment
  situation at the same nesting depth: `frontend/app/track/mine/` (a
  literal, static segment) sits alongside no dynamic sibling under
  `/track` today, so there's no existing in-repo precedent for a static
  segment literally shadowing a dynamic one at the same level — but
  Next.js's App Router routing precedence (a literal path segment always
  matches before a sibling dynamic segment, e.g. `/blog/about` resolves to
  a static `app/blog/about/page.tsx` over a dynamic
  `app/blog/[slug]/page.tsx` if both exist) is standard, documented
  framework behavior, not something specific to this codebase that needed
  verifying by reading vendored Next.js source. Combined with the
  `custom-` prefix finding above (no real custom line can ever have id
  `"new"`), a `/lines/new` static route is safe on both fronts: the
  framework would always route it to the static page even if a colliding
  dynamic id existed, and no colliding id can exist in the first place.
- **`frontend/app/track/mine/page.tsx`** (read in full): the entry-point
  placement precedent —

  ```
  frontend/app/track/mine/page.tsx:76-79
  <Group justify="space-between" align="baseline">
    <Title order={1}>My Trains &amp; Tickets</Title>
    <TextLink href="/track">Track a new train</TextLink>
  </Group>
  ```

- **`frontend/app/lines/[id]/page.tsx`** (read in full): the other
  relevant placement precedent — Edit/Delete controls sit in a
  `<Group justify="space-between">` beside the page's own `Title
  order={1}` (lines 108-138), not below any content.
- **`frontend/components/TextLink.tsx`** (read in full): the app's
  standard in-page link component — plain `<Link>` wrapping Mantine
  `Text`, server-renderable, `underline="hover"` default (suited to a
  link "whose position already identifies them... a right-aligned action
  beside a section heading" — its own doc comment's exact words, matching
  this spec's proposed placement).
- **`frontend/components/useNeedsLogin.ts`** and
  **`frontend/components/LoginLink.tsx`** (both read in full): the shared
  Tier-2 401-handling pattern `CustomLineForm` already uses, extracted per
  `2026-08-31-anonymous-user-ux-design.md`'s own "Reusable pattern"
  recommendation.
- **`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md`**
  (read in full): its per-surface table (§Current relevant state) already
  classifies `/lines`' custom-line form as **Tier 2** ("public entry,
  gated completion") and its Policy section states the recommended
  default explicitly: *"keep the reactive pattern where session isn't
  already being fetched (it's honest and costs nothing extra), but where a
  page/component already has session in hand... prefer showing the prompt
  proactively."* `/lines` does not call `getSession()` today (confirmed:
  no such call appears in `app/lines/page.tsx`), so per this policy the
  reactive-only default applies. This governs Decision 3 below.

## Decisions

### 1. New route: `/lines/new`

**Chosen.** `frontend/app/lines/new/page.tsx`, a Server Component with no
`getSession()` call (see Decision 3), rendering `CustomLineForm` inside
the same `<Center><Stack p="lg" gap="md" maw={480} w="100%">` chrome
`[id]/edit/page.tsx` already uses, with `<Title order={1}>New custom
line</Title>` and `cancelHref="/lines"` (parallel to the edit page's
`cancelHref={`/lines/${id}`}`, sending Cancel back to the list rather than
a single line's detail).

No real alternative path was seriously considered: `/lines/new` is the
only candidate that reads naturally next to the existing `/lines/[id]` and
`/lines/[id]/edit` siblings, and Current relevant state above confirms it
is safe on both the routing-precedence axis (standard Next.js behavior)
and the id-collision axis (`custom-` prefix is unconditional, so no real
line can ever hold id `"new"`).

### 2. Entry point on `/lines`: a `TextLink` beside the `Title order={1}`, in a `Group justify="space-between"`

**Chosen**, following `/track/mine`'s own established pattern verbatim
(Current relevant state, above) rather than inventing a new placement
convention:

```tsx
<Group justify="space-between" align="baseline">
  <Title order={1}>All Lines</Title>
  <TextLink href="/lines/new">New custom line</TextLink>
</Group>
```

This replaces the current bare `<Title order={1}>All Lines</Title>` line
(`page.tsx:20`) and removes the entire second `Stack` (lines 24-27,
the "New Custom Line" heading + inline form). `AllLinesTable` and
everything else on the page is unchanged.

**Copy: "New custom line", not "Create a line" or "Track a new
train"-style phrasing.** Two real options were weighed:

- **Match `/track/mine`'s exact grammatical shape** ("Track a new
  train" → "Create a new line"). Considered, but rejected: this app
  already has an established, exact name for this feature — the
  removed inline heading literally read "New Custom Line" — and
  `/lines/[id]/edit`'s own "Edit" button pairs one-to-one with its
  destination page's heading ("Edit" → "Edit: {line.name}"). Reusing the
  feature's existing name for both the link and the destination page's
  `<Title order={1}>` continues that same "link text names its
  destination" pairing rather than introducing a second, differently-
  phrased name for the same feature.
- **Reuse "New custom line" for both the link and the destination
  `<Title order={1}>`** (sentence case, matching this page's own "Edit:
  {line.name}" and this spec's other headings' capitalization). **Chosen**
  — minimal copy invention, and mirrors the Edit-button/Edit-page-heading
  pairing already live in this exact page family.

`variant="space-between"`/`align="baseline"` on the `Group` matches
`/track/mine`'s own exact props, not a new layout choice.

### 3. Auth-gating the entry point: always show the link; keep all login-prompt handling inside the form on `/lines/new`

**Chosen: no proactive session check on `/lines`, no gating on the
`TextLink` itself.** Two options were weighed against
`2026-08-31-anonymous-user-ux-design.md`'s existing, already-adopted
policy (Current relevant state, above):

- **Gate the link with a session probe** (e.g. `getSession()` on
  `/lines`, showing `<LoginLink>New custom line</LoginLink>` instead of a
  working link for an anonymous visitor). **Rejected.** The anonymous-
  user-ux spec's own Policy section is explicit that the *recommended
  default* for a Tier-2 surface is the reactive pattern specifically
  *because* `/lines` doesn't fetch session today — proactive gating is
  only preferred "where a page/component already has session in hand."
  Adding a new `getSession()` call to `/lines` purely to decorate this one
  link would be new cost this policy document already reasoned through
  and declined by default, and would apply only to this one entry point
  while `/lines`' pin-star affordances (also Tier 2, also on this same
  page) stay reactive-only — introducing an inconsistency within the same
  page for no stated benefit.
- **Always render the working link; `CustomLineForm` on `/lines/new`
  handles a real 401 with its existing `useNeedsLogin`/`LoginLink`
  behavior, unchanged.** **Chosen.** This is exactly today's behavior
  (the inline form is unconditionally rendered right now, with the same
  reactive 401 handling) carried forward unchanged to the new route — no
  new auth surface is introduced by moving the form, only its position on
  the page changes. It also matches the anonymous-user-ux spec's own
  per-surface table entry for this exact form ("Create form = Tier 2,
  currently broken (no login prompt) — needs the same treatment as
  PinToggle" — a gap this session's `useNeedsLogin` work already closed at
  the form level, not the entry-link level).

### 4. The create-mode submit-reset workaround becomes dead code for its one remaining call site — remove it

This is the one place this spec revisits an existing, working piece of
code rather than only adding new files, so it gets its own decision.

**The facts, verified above:** `CustomLineForm`'s create-mode success
branch (`CustomLineForm.tsx:134-157`) manually resets `submitting` and
clears every field specifically because, *today*, `POST`-then-navigate
targets `/lines` — the same route the form already lives on — so App
Router doesn't remount the component. The edit-mode branch needs no such
workaround because it already targets a *different* route
(`/lines/${existingLine.id}`) and gets a free remount. Once this spec's
route move ships, the create form's only remaining home is
`/lines/new` (Decision 1) — a route strictly different from `/lines`,
the success target. Create mode's `router.push('/lines')` therefore
becomes, for the first time, a genuine cross-route navigation: App Router
remounts the component on the way there, exactly like edit mode already
does, for free.

**Two options:**

- **Leave the manual reset as defensive redundancy.** It's harmless in
  the sense that resetting state on a component about to unmount has no
  visible effect either way. **Rejected as the wrong default for this
  codebase specifically**, not as a blanket rule: this repo's own recent
  design work (`2026-09-01-tracked-trains-home-page-design.md`'s Decision
  3, rejecting a `?limit=` parameter added "on a hypothesis rather than a
  measured cost") repeatedly declines speculative complexity kept "just in
  case," in favor of removing what a change has made genuinely
  unnecessary. More importantly, the code isn't just inert here — its
  *comment* becomes actively wrong: the create-mode comment
  (`CustomLineForm.tsx:135-148`) states as fact that success "navigates
  back to `/lines` — the same route this form already lives on," which
  would no longer be true once the form only ever renders at
  `/lines/new`. A false comment left in place to justify now-dead code is
  worse than removing both.
- **Remove the manual reset for the create-mode success path, and
  collapse create/edit onto one identical, comment-free navigation.**
  **Chosen.** Concretely: replace the `if (existingLine) { router.push(...)
  } else { setSubmitting(false); setName(''); ...; router.push('/lines'); }`
  branch with a single
  `router.push(existingLine ? `/lines/${existingLine.id}` : '/lines')`,
  removing the six manual reset calls (lines 149-155) and the now-stale
  comment explaining them (lines 134-148), replacing it with a short note
  that both modes now navigate cross-route and rely on the resulting
  remount, mirroring what the edit-mode comment already says.

  This also means removing (not merely leaving red)
  `CustomLineForm.test.tsx:264-296`, `'a successful create resets the
  submit button out of its loading state and clears the form'` — its own
  comment states its premise is specifically the same-route,
  non-remounting scenario, which stops being true for this form's real
  route once this spec ships. Its no-op `useRouter().push` mock (top of
  test file) cannot distinguish "same route, no remount" from "different
  route, remounts" — a unit test rendering `CustomLineForm` in isolation
  has no way to exercise App Router's actual remount behavior either way,
  so keeping this test after removing the code it locks in would either
  fail outright (nothing resets state anymore) or, if kept passing by
  leaving the workaround in, would keep enforcing dead code by
  construction. Removing the test alongside the code is the honest
  option, not a coverage regression: nothing then asserts a
  same-route-no-remount behavior because that scenario no longer exists
  in the app.

  **What is safe to leave in place:** `existingLine`'s own edit-mode
  reasoning, the `cancelHref` behavior, and every other test in that file
  — none of them depend on the removed branch.

## Architecture

Before:

```
/lines                       Server Component
  ├─ All Lines table
  └─ "New Custom Line" heading + inline <CustomLineForm /> (always expanded)
```

After:

```
/lines                       Server Component
  ├─ Group: Title "All Lines"  +  TextLink "New custom line" -> /lines/new
  └─ All Lines table                                          (unchanged)

/lines/new                   Server Component, NEW
  └─ Center > Stack maw=480
       Title "New custom line"
       <CustomLineForm cancelHref="/lines" />   (create mode, unmodified props)

/lines/[id]/edit              unchanged (existing precedent this spec copies)
```

`CustomLineForm` itself changes in exactly one place: the create-mode
success branch loses its manual reset (Decision 4) and gains the same
one-line `router.push` shape edit mode already has. No new props, no new
component.

## Error handling

- **Form-level 401 on submit** (both modes): unchanged — `CustomLineForm`'s
  existing `useNeedsLogin`/`LoginLink` handling (Current relevant state)
  is reused verbatim at the new route; nothing about this move touches it.
- **Non-401 submit failures** (validation errors, 5xx): unchanged —
  rendered via the form's existing `error`/`<Text c="red">` path, same as
  today, at either route.
- **`/lines/new` itself has no failure mode to design**: unlike
  `/lines/[id]/edit`, it takes no dynamic segment and fetches nothing
  server-side before rendering — there is no `getCustomLine`/`notFound()`
  equivalent needed (contrast with `[id]/edit/page.tsx`'s `try/catch` +
  `notFound()` around `getCustomLine`, which exists only because that page
  has an id to fail to resolve).
- **A visitor navigating directly to `/lines/new` without ever having
  come from `/lines`**: renders exactly the same page either way — nothing
  on `/lines/new` depends on referrer or navigation history, matching how
  `/lines/[id]/edit` already behaves for a direct link.

## Testing

- **`frontend/app/lines/page.tsx` currently has no colocated
  `page.test.tsx`** (confirmed: `find frontend/app/lines -maxdepth 1
  -name "*.test.tsx"` returns nothing) — so this move has no existing
  inline-form assertions to remove from a page test; only
  `CustomLineForm.test.tsx` exercises the form's own behavior today
  (mounting the component directly, not the page), and that file is
  unaffected by the route move except for Decision 4's removed test. If a
  `frontend/app/lines/page.test.tsx` is added at implementation time (not
  mandated by this spec, but a reasonable place to assert the new entry
  point), it should cover: the `TextLink` "New custom line" renders,
  points at `/lines/new`, and sits beside the `Title` rather than below
  the table; and that no `CustomLineForm` markup (e.g. a "Name" field)
  renders on `/lines` any more.
- **`frontend/app/lines/new/page.tsx`** needs its own new test file.
  `frontend/app/lines/[id]/edit/page.tsx` has **no existing
  `page.test.tsx`** to mirror (Current relevant state, above) — so there
  is no existing test shape for a page-level wrapper in this route family
  to copy structurally; the only realistic precedent is
  `CustomLineForm.test.tsx`'s own `renderWithProvider`/`renderWithMantine`
  approach, applied to the new page component instead of the bare form.
  At minimum: the page renders `<Title>New custom line</Title>`,
  `CustomLineForm` mounts with `cancelHref="/lines"` (assert the Cancel
  link's `href`, same assertion style as
  `CustomLineForm.test.tsx:198-209`), and the page requires no server-side
  fetch/param resolution to fail (no `notFound()` path exists to test,
  per Error handling above).
- **`CustomLineForm.test.tsx` changes** (Decision 4): remove
  `'a successful create resets the submit button out of its loading state
  and clears the form'` (lines 277-296) and its now-inapplicable framing
  in the block comment above it (lines 264-276); update the create-mode
  401 test (lines 242-262, which sets `mockUsePathname.mockReturnValue(
  '/lines')`) to `'/lines/new'` so the pathname mock — used to build
  `LoginLink`'s `return_to`, per `LoginLink.tsx`'s own logic — matches the
  form's real new location; update the assertion at line 261
  (`return_to=%2Flines`) to the encoded `/lines/new` accordingly. No other
  existing test in this file depends on the form's create-mode route.

## Explicitly out of scope

- **Any change to `CustomLineForm`'s edit-mode behavior, props, or
  `[id]/edit` route.** Untouched by this spec — it is the precedent being
  copied, not something being redesigned.
- **A session-aware, proactive login prompt on the `/lines` entry
  point.** Decision 3 explicitly chooses the reactive default per the
  already-adopted anonymous-user-ux policy; a future change to that
  policy (e.g. if `/lines` starts fetching session for an unrelated
  reason) could revisit this, but is not designed here.
- **Any backend change.** `POST /api/lines`, `custom_lines::slugify`, and
  everything in `crates/api/src/routes/lines.rs` are read only to confirm
  the id-collision question (Decision 1) — nothing here proposes touching
  them.
- **Redesigning `DeleteLineButton`** or any other Tier-2/3 auth surface
  named in `2026-08-31-anonymous-user-ux-design.md`. Out of scope; this
  spec only moves where the create form lives.
- **A `frontend/app/lines/page.test.tsx` file's full contents.** Testing
  above states what a new test *should* cover if one is added, but
  writing that file is implementation, not design.

## Open questions / risks

1. **Copy bikeshedding.** "New custom line" (Decision 2) is a defensible,
   low-invention choice, but it is still a judgment call between that and
   a `/track/mine`-style verb phrase ("Create a new line"); either reads
   fine and neither was validated with real users.
2. **Whether to add a `frontend/app/lines/page.test.tsx` at all.** No such
   file exists today (Current relevant state), so adding one is new
   test-infrastructure for this route, not a modification of an existing
   file — worth confirming at implementation time that this repo wants one
   rather than continuing to rely on `AllLinesTable.test.tsx` +
   `CustomLineForm.test.tsx` covering the page's two halves separately, as
   it effectively does today.
3. **Decision 4's test removal shrinks explicit regression coverage for
   the original "stuck loading" bug.** The bug can no longer occur for
   `CustomLineForm`'s only remaining create-mode call site (route move
   makes it structurally impossible, not just less likely), so this is a
   deliberate, reasoned trade — but it does mean nothing in the test suite
   would catch a *future* regression if some later change reintroduced a
   same-route create-then-navigate pattern elsewhere. Not mitigated here;
   flagged for whoever implements this to judge whether a narrower,
   route-agnostic test (e.g. "create mode always navigates to a route
   different from its own current pathname") is worth adding in its
   place — not designed further in this spec.
