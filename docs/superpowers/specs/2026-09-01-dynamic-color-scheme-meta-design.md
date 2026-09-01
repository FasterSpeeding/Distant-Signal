# Design: A Dynamic `<meta name="color-scheme">` Tag

**Status: design proposal, not approved.** Builds directly on
`docs/superpowers/specs/2026-09-01-dark-reader-color-scheme-signal-research.md`
(research/survey only, merged to `main`, no code changed) — this spec does
not re-run that research; it takes its findings as given and works out how
to actually implement the one concrete recommendation that research made.
No implementation plan is included; that is a separate, later step in this
repo's process.

## Goal

The Dark Reader browser extension double-dark-themes this app for users
who have it installed, even when the app is already rendering its own real
dark theme. The prior research pass established, against primary sources
(MDN, Dark Reader's own repo), that:

- This app already emits `color-scheme: light dark` unconditionally (via
  Mantine's base stylesheet), and that is specifically the form Dark
  Reader's own bug tracker shows has an unreliable history as an opt-out
  signal.
- The one confirmed-current, evidence-grounded signal is a **single-value**
  `<meta name="color-scheme" content="dark">` (or `content="light"`) tag —
  per Dark Reader maintainer `alexanderby`'s 2026-03-20 reply in
  `darkreader/darkreader` discussion #15128.
- No such tag exists anywhere in this codebase today.
- A static tag would be wrong roughly half the time on a toggleable site —
  it has to track this app's actual resolved theme.

This spec works out exactly where that tag's value comes from on first
paint, how it gets kept in sync with every subsequent theme change, why
that doesn't reintroduce a hydration-mismatch bug, what a test for it
would assert, and whether building it is actually worth it given the
research found no *guaranteed* fix.

## Current relevant state (verified 2026-09-01, this session)

- **`frontend/app/layout.tsx`** (read in full): `RootLayout` is a Server
  Component. It exports `metadata: Metadata` (lines 16–20, `title` and
  `description` only — no `viewport` export exists anywhere in this file,
  confirmed by `grep -n "viewport"` turning up nothing but an unrelated
  code comment). Inside `<head>` it renders `<ColorSchemeScript
  defaultColorScheme="auto" />` (line 94); the `<html>` tag carries
  `{...mantineHtmlProps}` (line 92). Inside `<body>`,
  `<MantineProvider theme={theme} defaultColorScheme="auto">` (line 97)
  wraps everything, with `<AutoRefresh />` (line 98) mounted as the first
  child — a side-effect-only Client Component that renders `null`.
  `ThemeToggle`/`PrideToggle` are mounted further down, inside the nav
  `Group` (lines 140–141). Confirmed against `main` (`git diff main --
  frontend/app/layout.tsx` is empty in this worktree) — no drift from the
  research doc's premise.
- **`frontend/components/ThemeToggle.tsx`** (read in full): a Client
  Component. Reads `useMantineColorScheme()` for the raw
  `colorScheme`/`setColorScheme` and separately
  `useComputedColorScheme('light')` for the *resolved* light/dark value
  (line 36) — the fallback `'light'` matters: it's what "auto with no
  known preference" resolves to before mount. Gates its own display with
  `useMounted()` from `@mantine/hooks` (line 37): `displayedScheme`/
  `displayedComputedScheme` are hardcoded to `'auto'`/`'light'` until
  `mounted` flips true, specifically so the client's first (pre-hydration)
  render matches the server-rendered output byte-for-byte. Its own comment
  (lines 28–33) states why: "Mantine's `colorScheme` reads localStorage
  synchronously (even on the client's first, pre-hydration render), so it
  can already disagree with the server-rendered 'auto' default before
  React ever gets to diff the tree." Confirmed by `ThemeToggle.test.tsx`'s
  own SSR test (`'server-rendered output ignores localStorage, avoiding a
  hydration mismatch'`), which sets `localStorage['mantine-color-scheme-value']
  = 'dark'` and asserts `renderToString` still shows `"Theme: auto"` /
  `☀️`, not `"Theme: dark"` / `🌙` — i.e. Mantine's real persistence key is
  `localStorage['mantine-color-scheme-value']`, not a cookie; there is no
  server-visible signal of the stored preference in this app today, only a
  client-side, post-mount one.
- **`frontend/components/PrideToggle.tsx`** (read in full): a directly
  analogous existing pattern for "keep a DOM attribute the component
  itself doesn't render in JSX in sync with client-only preference state."
  Lines 130–133:
  ```
  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = mode;
  }, [mode, mounted]);
  ```
  This imperatively sets `document.body.dataset.pride` from inside a
  `useEffect`, gated on the same `useMounted()` pattern — `body` isn't an
  element this component renders via JSX, so React's hydration
  reconciliation never compares this attribute; the mutation happens
  strictly after hydration completes. This is the load-bearing precedent
  for this spec's own mechanism (Decision 2 below).
- **`frontend/components/AutoRefresh.tsx`** (read in full): "Side-effect-
  only component (renders nothing) mounted once in the root layout" (its
  own doc comment, line 8) — the established shape in this codebase for
  "one global Client Component, mounted once inside `MantineProvider`,
  that does something imperative and returns `null`."
  `frontend/components/LastUpdated.tsx` (lines 16–24) documents the same
  `useMounted()`-gated, server/client-byte-identical-first-render pattern
  a third time, for a different reason (a `Date.now()`-derived string) —
  the repo explicitly calls this "the same class of bug fixed in
  `ThemeToggle`."
- **`frontend/app/globals.css:1`**: `@import '@mantine/core/styles.css';`
  — confirmed still present, unmodified. Per the research doc's Finding 2,
  this stylesheet declares `:root { color-scheme: var(--mantine-color-scheme); }`,
  which compiles to the static default `light dark` — present
  unconditionally, not dynamically switched, and untouched by anything in
  this spec (see Decision 3).
- **Next.js's `Viewport`/`Metadata` types**, verified directly against the
  installed package (`frontend/node_modules/next@16.2.10`'s
  `dist/lib/metadata/types/metadata-interface.d.ts`, since this worktree
  has no local `node_modules` — checked against the sibling checkout that
  does): `Metadata.colorScheme` (line 163) is annotated
  `@deprecated Use the new viewport configuration (\`export const viewport: Viewport = { ... }\`) instead`.
  The live, non-deprecated field is `Viewport.colorScheme` (lines 622–629),
  whose own doc comment gives the exact mapping:
  ```
  colorScheme: "dark"
  // Renders <meta name="color-scheme" content="dark" />
  ```
  `dist/lib/metadata/default-metadata.js`'s `createDefaultViewport()`
  (lines 22–30) confirms Next fills in `width: 'device-width'`,
  `initialScale: 1` as baseline defaults merged with whatever a layout's
  own `viewport` export supplies — so adding a `viewport` export with only
  `colorScheme` set does not need to also restate `width`/`initialScale`
  to avoid losing them.
  `grep -rln "export const metadata\|export function generateMetadata\|export const viewport\|export function generateViewport" frontend/app/` returns only `frontend/app/layout.tsx` — no route in this app defines its own `metadata` or `viewport` today, so there is nothing downstream that could override or merge against a `viewport` export added at the root layout.
- **`frontend/app/layout.test.tsx`** (read in full): tests only the
  exported `TrackedTrainsNavItem` function; nothing in this codebase
  currently tests the `metadata` object's contents directly (title/
  description are untested). There is no existing precedent either way for
  testing a `viewport` export's value — Decision 5/Testing below sets one.

## Recommendation

**Build it.** The implementation this spec describes is small (one new
`viewport` export of a few lines, one new ~20-line side-effect-only Client
Component mounted once, following three separate patterns — `AutoRefresh`,
`PrideToggle`, `ThemeToggle` — this codebase already uses elsewhere for
exactly this shape of problem), adds no new dependency, and changes
nothing about how the app looks or behaves for a visitor who isn't running
Dark Reader.

Against that low cost: the benefit is real but **not guaranteed**, and this
spec does not pretend otherwise. What it *does* guarantee: this app will
emit the one specific, current, primary-source-confirmed signal
(`<meta name="color-scheme" content="dark|light">`, kept honestly in sync
with the resolved theme) that Dark Reader's own maintainer named, by name,
as recently as six months before this research, as "the best way to tell
Dark Reader about a dark theme." What it does **not** guarantee:

- A visitor who has manually force-enabled Dark Reader for this site
  (a real, common override — Dark Reader exposes exactly this per-site
  toggle) will still get double-themed; no page-side signal changes a
  user's own explicit per-site override.
- Dark Reader's own docs describe this signal as one input to a heuristic,
  not a hard contract enforced by a spec — the research's own Finding 3
  found open Dark Reader issues arguing this exact behavior *should* be
  more reliable, which is itself evidence it currently isn't perfectly
  reliable.
- Every *other* dark-mode-forcing extension or browser feature (this
  research only ever examined Dark Reader) may or may not read this tag
  the same way.

Given the cost is genuinely small and reuses only patterns already proven
out in this codebase (no novel architecture, no new hydration-safety class
to invent), a best-effort, non-guaranteed improvement is still worth
shipping here — the alternative (do nothing) leaves on the table a
specific, dated, named-maintainer confirmation of what to do, for
single-digit lines of new code. See "Do-nothing option" below for the full
weighing.

## Decisions

### 1. Where the initial value comes from: a new `viewport` export, not `metadata`, not a second blocking script

**Chosen: add `export const viewport: Viewport = { colorScheme: 'light' }`
to `frontend/app/layout.tsx`**, alongside (not replacing) the existing
`metadata` export. This is a first-class Next.js 16 mechanism for
precisely this tag (`Viewport.colorScheme` → `<meta name="color-scheme"
content="...">`, confirmed above) — using it is the same kind of choice
this file already made for `title`/`description` via `metadata`, not a new
category of thing.

**`'light'` is the right static default**, for the same reason
`ThemeToggle.tsx`'s pre-mount render is hardcoded to `'auto'`/`'light'`
rather than trying to guess: the server cannot know a visitor's system
preference or their stored `localStorage` choice (Current relevant state,
above — this app has no cookie-based persistence, only
`localStorage['mantine-color-scheme-value']`), so any SSR-time value has to
be a fixed default, and `'light'` is the one this app's own existing
theme system already uses as its deterministic pre-mount fallback
(`useComputedColorScheme('light')`). Using the same constant means this
tag's SSR value is never a *new*, third opinion about what "unknown" means
— it agrees with the one the rest of the page already commits to.

**Considered and rejected: a second blocking inline script in `<head>`**
(mirroring what `ColorSchemeScript` itself does — reading
`localStorage['mantine-color-scheme-value']` synchronously before paint and
setting the meta tag's `content` immediately, closing the narrow gap
described in Open questions/risks below). Rejected because it is exactly
the "second, separate mechanism that could disagree with the first" this
spec was explicitly asked not to invent: it would need to reimplement
Mantine's own `auto` resolution logic (stored value, else
`matchMedia('(prefers-color-scheme: dark)')`) a second time, by hand, and
any future change to Mantine's own resolution logic (a version bump,
a changed storage key) would silently desync the two. The static-default
approach has a real but narrow, well-understood limitation (below)
instead of an open-ended maintenance-drift risk.

**Considered and rejected: reusing the deprecated `Metadata.colorScheme`
field.** Functionally very similar, but Next's own type declares it
deprecated in favor of `Viewport.colorScheme` specifically — no reason to
add new code against a field Next itself says not to use.

### 2. Where it gets updated: a new side-effect-only Client Component, mounted once, imperatively mutating the DOM — not a second render of the `<meta>` tag

**Chosen: a new Client Component (e.g. `ColorSchemeMeta`), mounted once
inside `<MantineProvider>` in `app/layout.tsx`, alongside `<AutoRefresh
/>`.** It renders `null`. Its logic:

```
const computedColorScheme = useComputedColorScheme('light'); // same hook,
                                                                // same fallback,
                                                                // as ThemeToggle
const mounted = useMounted();

useEffect(() => {
  if (!mounted) return;
  let tag = document.querySelector('meta[name="color-scheme"]');
  if (!tag) {
    tag = document.createElement('meta');
    tag.setAttribute('name', 'color-scheme');
    document.head.appendChild(tag);
  }
  tag.setAttribute('content', computedColorScheme);
}, [mounted, computedColorScheme]);
```

This is not new architecture for this codebase — it is `PrideToggle.tsx`'s
own `document.body.dataset.pride = mode` pattern (lines 130–133, Current
relevant state above), applied to a `<meta>` tag instead of a `body`
dataset attribute, with the same `useMounted()` gate and the same
`useComputedColorScheme('light')` hook/fallback `ThemeToggle.tsx` already
uses — no new hook, no new gating pattern, no new fallback constant.

**Why this doesn't race Mantine's own hydration**: `useMounted()` only
flips `true` in an effect that runs after the component has mounted —
i.e. strictly after React's hydration pass has already reconciled the
server-rendered tree against the client's first render. The `content`
mutation above only ever runs inside that post-`mounted` branch, so it
cannot fire before hydration completes, and it cannot run at all during
SSR (`useEffect` bodies never execute server-side). There is nothing here
that touches Mantine's own `ColorSchemeScript`, its `data-mantine-color-
scheme` attribute, or its own hydration handling — this component reads
`useComputedColorScheme` (a value Mantine itself computes) but never
writes anything Mantine owns.

**Placement**: alongside `AutoRefresh`, not alongside `ThemeToggle`/
`PrideToggle` in the nav `Group`. It has no UI, so it doesn't need to be
inside the nav at all — `AutoRefresh`'s "one global, invisible, mount-once
inside `MantineProvider`" slot is the correct precedent, not the visible
toggle buttons' slot.

**Considered and rejected: rendering the `<meta>` tag directly in this
component's own JSX** (e.g. returning
`<meta name="color-scheme" content={displayedComputedScheme} />` the way
`ThemeToggle` returns its icon-bearing button). Rejected because this
would reintroduce exactly the hydration-mismatch class of bug this
codebase has already fixed three times over (`ThemeToggle`, `PrideToggle`,
`LastUpdated`): React tracks and diffs an element it renders itself, so a
`<meta>` tag rendered by *this* component would need its own
`displayedScheme`-style pre-mount/post-mount split, *and* it would
conflict with the tag Next's `viewport` export already renders at the same
`<head>` position — two different code paths trying to own the same DOM
node. The imperative, JSX-free `useEffect` mutation avoids both problems
at once: there is exactly one thing that renders the tag (Next's
`viewport` export, at SSR time and on every subsequent server-rendered
navigation), and exactly one thing that ever mutates its `content`
post-mount (this component), and the two never contend for the same React
node.

### 3. Coexistence with Mantine's own `color-scheme: light dark` CSS: leave it untouched, don't override or suppress it

**Chosen: do nothing to `@mantine/core/styles.css`'s `:root { color-
scheme: light dark; }` rule.** The two signals are not actually in
contradiction, once what each one is *for* is kept straight:

- Per MDN (research Finding 1, re-confirmed above), the CSS `color-scheme`
  property governs **native browser UI** — scrollbars, form controls,
  spellcheck squiggles — for the element it's set on. Mantine's `light
  dark` on `:root` is a completely standard, correct use of that property:
  "this document supports rendering natively in either scheme, browser's
  choice." Changing or removing it would be a real regression to native
  UI chrome for zero benefit — the research never found evidence that the
  CSS property itself, as opposed to the meta tag, needed to change.
- Per the HTML spec's own precedence rules, an explicit `color-scheme` CSS
  property on the root element takes priority over the meta tag for
  *that* native-UI purpose anyway — so even if the meta tag and the CSS
  property "disagreed" in the sense of stating different values, the
  browser's own native-UI rendering would still follow the CSS property,
  unaffected by the meta tag either way.
- Dark Reader's own maintainer (Finding 3) named the **meta tag**
  specifically, with a **single value**, as the thing Dark Reader checks —
  not "whatever the CSS property currently resolves to." The two are
  different signals aimed at different consumers (native browser chrome
  vs. Dark Reader's own detection heuristic), so there is nothing to
  reconcile between them.

**Considered and rejected: overriding Mantine's CSS declaration to track
the same single resolved value** (e.g. a client effect additionally
setting `document.documentElement.style.colorScheme = computedColorScheme`,
or a `globals.css` rule fighting Mantine's own injected `:root` rule the
way `html:root[data-mantine-color-scheme='light'] { --mantine-color-
anchor: ... }` already has to do for the anchor-colour fix at the top of
that file). Rejected on two grounds: first, the research found no evidence
this is *needed* — the confirmed signal is the meta tag, not the CSS
property, and Dark Reader's own currently-open feature requests to treat
the CSS property as a general opt-out (Finding 3's last bullet) are
proposals, not shipped behavior, so building against them would be
speculative. Second, this codebase already has one on-the-record example
(`globals.css`'s anchor-colour override, and its own comment explaining
why) of how fiddly it is to win a specificity fight against a rule
Mantine's own stylesheet injects — taking on that fight a second time, for
a change the evidence doesn't call for, is not worth it.

### 4. Single value, not `light dark`, in the new tag — always exactly what's currently resolved

The new tag's `content` is always exactly one of `'light'`/`'dark'` — the
current `useComputedColorScheme('light')` result — never `'light dark'`.
Using the multi-value form here would just recreate, in a second location,
the exact form Finding 3 found unreliable; the entire point of adding a
second, purpose-built tag is to use the form that's actually confirmed to
work.

### 5. Testing: colocated, following `ThemeToggle.test.tsx`'s existing shape where it applies, plain object assertions where JSX rendering doesn't apply

Two different things need covering, and they need two different test
shapes because — unlike `ThemeToggle`'s own text/icon output — the static
half of this feature (the `viewport` export) is not something React ever
renders in a component tree a test can mount:

- **The static default** (`frontend/app/layout.tsx`'s new `viewport`
  export): a plain assertion in `frontend/app/layout.test.tsx` (which
  already imports named exports from `./layout`, e.g.
  `TrackedTrainsNavItem`) — `expect(viewport.colorScheme).toBe('light')`.
  No rendering needed; this is a data assertion, matching how this file
  already isn't rendering `metadata`'s `title`/`description` in a test
  either — the value just needs to exist and be right.
- **The runtime-sync behaviour** (`ColorSchemeMeta`, new colocated
  `ColorSchemeMeta.test.tsx`, `renderWithMantine` per this repo's
  established helper): assert that after mount the tag exists with
  `content="light"` under the default (`matchMedia` polyfilled to no-dark-
  preference, per `vitest.setup.ts`), and that when the surrounding
  `MantineProvider`'s stored scheme changes to dark (either by rendering
  `ColorSchemeMeta` alongside `ThemeToggle` and firing clicks the way
  `ThemeToggle.test.tsx` does, or by pre-seeding
  `localStorage['mantine-color-scheme-value'] = 'dark'` before render, the
  same key `ThemeToggle.test.tsx`'s own SSR test already uses) the tag's
  `content` updates to `"dark"`. A further test confirms no *second*
  `<meta name="color-scheme">` tag is created on re-render/re-mount — the
  "create if absent" branch only ever adds one tag, it doesn't duplicate
  an existing one.
- **Explicitly not attempted**: a `renderToString`-based SSR/hydration-
  parity test in the shape of `ThemeToggle.test.tsx`'s own
  `'server-rendered output ignores localStorage'` case. That pattern works
  for `ThemeToggle` because the differing text is part of *its own*
  returned JSX; `viewport.colorScheme` is resolved by Next's separate
  metadata pipeline, not by rendering `RootLayout`'s function body under
  `renderToString`, so there's no equivalent tree to diff in a unit test.
  The hydration-safety property this spec relies on (Decision 2) is
  structural — no JSX renders the tag from more than one place — not
  something a render-diff test could add further confidence to beyond
  what's already covered above.

## Architecture

```
                         ┌───────────────────────────────────────────┐
                         │ frontend/app/layout.tsx                   │
                         │                                           │
                         │ export const viewport: Viewport = {       │
                         │   colorScheme: 'light',   // NEW           │
                         │ };                                        │
                         └───────────────────┬───────────────────────┘
                                              │ Next.js metadata pipeline
                                              │ (server-rendered, every
                                              │  request/navigation)
                                              ▼
                         <meta name="color-scheme" content="light">
                                              │
                                              │ present in initial HTML,
                                              │ byte-identical pre/post
                                              │ hydration (nothing reads
                                              │ client state to render it)
                                              ▼
   ┌──────────────────────────────────────────────────────────────────┐
   │ <MantineProvider>                                                 │
   │   <AutoRefresh />           existing, unchanged                   │
   │   <ColorSchemeMeta />       NEW -- renders null                   │
   │     useComputedColorScheme('light')  (same hook/fallback as       │
   │     ThemeToggle) + useMounted()                                   │
   │     useEffect: after mount, on every resolved-scheme change,      │
   │       document.querySelector('meta[name="color-scheme"]')        │
   │         .setAttribute('content', computedColorScheme)             │
   │   ...ThemeToggle, PrideToggle, nav, etc. (unchanged)               │
   │ </MantineProvider>                                                 │
   └──────────────────────────────────────────────────────────────────┘

   @mantine/core/styles.css: `:root { color-scheme: light dark; }`
   -- left untouched throughout (Decision 3); governs native browser UI
   only, not read by Dark Reader's own confirmed detection path.
```

## Error handling

- **`document.querySelector` finds no tag** (e.g. some future page-level
  `viewport` override strips it, or a test environment doesn't render
  Next's metadata pipeline at all): `ColorSchemeMeta` creates the tag
  itself before setting `content`, rather than throwing or silently doing
  nothing — the effect's job is "the tag reflects the current scheme,"
  not "the tag Next rendered specifically gets edited."
- **`useComputedColorScheme` itself never throws** in normal operation —
  it's only ever called from inside `MantineProvider`, same as
  `ThemeToggle`'s existing, unguarded call to the same hook; no new error
  boundary is needed around `ColorSchemeMeta` that `ThemeToggle` doesn't
  already need.
- **No SSR-side failure mode**: the entire runtime-sync half of this
  feature is a `useEffect` body, which never executes during server
  rendering — there is nothing here that can fail a server render or a
  static build the way a data fetch could.
- **Multiple tabs**: each tab's `ColorSchemeMeta` only ever mutates that
  tab's own `document.head` — there is no cross-tab state to
  desynchronize, the same characteristic `ThemeToggle`/`PrideToggle`
  already have (each tab independently reads the same `localStorage` key
  on its own mount/storage-event cycle; this spec adds no new cross-tab
  concern beyond what already exists).

## Explicitly out of scope

- **Any change to `@mantine/core/styles.css`'s injected `color-scheme:
  light dark` rule**, or to `globals.css` to override/suppress it —
  Decision 3 covers why this isn't needed.
- **A blocking inline script** to close the narrow initial-paint gap
  described in Open questions/risks below — Decision 1 covers why this
  was rejected as inventing a second, driftable mechanism.
- **`theme-color` meta tag** (`Viewport.themeColor`) or any other
  visual-metadata field — the research and this spec are scoped narrowly
  to `color-scheme` only; a `theme-color` tag is a different feature with
  its own design questions (a literal colour per scheme, browser chrome
  tinting on mobile) not touched here.
- **Any extension other than Dark Reader.** The research only examined
  Dark Reader; this spec's Recommendation section already flags that other
  force-dark extensions/browser features may or may not read this tag the
  same way, and that isn't investigated further here.
- **The `darkreader-lock` opt-out.** Already ruled out by the research
  (wrong shape for a toggleable site) and not revisited here.

## Do-nothing option, weighed explicitly

**Do nothing** is the honest alternative, and it isn't a strawman: this
app already looks correct to every visitor who isn't running Dark Reader,
and even for one who is, the fix described here is best-effort, not a
guarantee (Recommendation, above). The case *for* doing nothing: zero
implementation cost, zero risk of any hydration-safety mistake (however
small), and the research itself found genuinely open, unresolved Dark
Reader issues arguing the ecosystem's own handling of this signal should
improve — waiting costs nothing but a working extension-detection
heuristic that this app doesn't fully control anyway.

The case *for* building it, which this spec recommends: the cost is not
speculative-large, it's concretely small (three small, additive changes,
all following patterns already proven in this exact codebase), and the
benefit — while not guaranteed — is grounded in the single most current,
specific, named-source confirmation the research turned up, not a guess.
Given how cheap and low-risk the implementation is, "wait for a guarantee
that will likely never come" isn't obviously better than "ship the
best-effort signal Dark Reader's own maintainer described as the best
current option." This spec's recommendation is to build it, precisely
because the cost/benefit is this lopsided — not because the benefit is
large in absolute terms.

## Open questions/risks

1. **A narrow, real timing gap between the page's own visual theme
   correcting (pre-paint, via Mantine's blocking `ColorSchemeScript`) and
   this meta tag correcting (post-hydration, via `ColorSchemeMeta`'s
   `useEffect`).** For a visitor whose stored/system preference is dark,
   the page itself never visibly flashes light (Mantine's script runs
   before first paint), but the new meta tag briefly still says `"light"`
   until React finishes hydrating and this component's effect runs.
   Decision 1 explains why closing this gap with a second blocking script
   was rejected (it would duplicate Mantine's own resolution logic by
   hand); the practical mitigation is that Dark Reader's own detection
   runs against the loaded DOM, not synchronously against paint timing, so
   this gap is unlikely to matter for Dark Reader's own behavior
   specifically — but it's a real, honest gap, not a fully closed one, and
   is not measured here.
2. **No way to verify against a real installed Dark Reader instance from
   within this design pass** — this spec (like the research it builds on)
   is grounded in Dark Reader's own public documentation and maintainer
   statements, not an empirical before/after test against the extension
   itself. Whoever implements this should ideally do a manual check with
   Dark Reader installed, in both this app's light and dark modes, before
   considering the feature validated.
3. **If a future route ever adds its own `metadata`/`viewport` export**
   (none does today — confirmed above), Next's per-route metadata merge
   could produce a different `color-scheme` meta tag on that route's
   server-rendered `<head>`, and `ColorSchemeMeta`'s effect (mounted once,
   dependent only on `[mounted, computedColorScheme]`, not on route
   changes) would not automatically re-fire just because the route
   changed — it would only correct the tag again the next time the
   resolved scheme itself changes. Not a problem today (no route defines
   either export), but worth flagging for whoever adds the first one.
