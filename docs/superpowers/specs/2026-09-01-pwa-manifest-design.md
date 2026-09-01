# Design: PWA Manifest and Icons

**Status: design proposal, not approved.** Builds directly on
`docs/superpowers/specs/2026-09-01-pwa-support-research.md`
(research/survey only, merged to `main`, no code changed) — this spec does
not re-run that research; it takes its Recommendation (Option A: manifest +
existing icons, no service worker) as given and works out exactly how to
implement it: the manifest's actual content, the two new icon assets, the
`theme-color`/viewport question, which (if any) Apple-specific meta tags
still earn their place, and what's actually worth testing. No
implementation plan is included; that is a separate, later step in this
repo's process. Written to the same rigor as
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md` and
`docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md`
(this document's closest structural precedent — same "research doc landed,
now turn its one recommendation into an implementable design" shape).

## Relationship to prior specs

Two prior specs, both merged/authored the same day as this one, are
directly relevant and neither is contradicted here:

- **`2026-09-01-pwa-support-research.md`** is this spec's entire premise —
  see above. Re-confirmed as still accurate: `frontend/app/layout.tsx`
  (read in full this session) still has no `viewport` export, no
  `manifest.ts` exists anywhere in `frontend/`, and `<head>` still contains
  only `<ColorSchemeScript>` (line 94) — no drift from the research's own
  findings.
- **`2026-09-01-dynamic-color-scheme-meta-design.md`** (also unimplemented
  as of this writing — `frontend/components/ColorSchemeMeta.tsx` does not
  exist, confirmed by directory listing, and `layout.tsx` mounts no such
  component) explicitly scoped `theme-color` **out**: *"`theme-color` meta
  tag (`Viewport.themeColor`) or any other visual-metadata field — the
  research and this spec are scoped narrowly to `color-scheme` only; a
  `theme-color` tag is a different feature with its own design questions
  (a literal colour per scheme, browser chrome tinting on mobile) not
  touched here."* This spec is where that explicitly-deferred question
  gets picked up (Decision 3) — not a correction, a planned follow-on.
  Where this spec's `theme-color` design touches the same `Viewport`
  export that sibling spec would add `colorScheme` to, both specs simply
  add fields to the same object; there's no conflict, and neither depends
  on the other landing first.

## Current relevant state (verified 2026-09-01, this session)

- **`frontend/app/layout.tsx:16-20`**: `Metadata` export has only `title:
  'Distant Signal'` and a long `description` (`"A personal UK rail
  companion: TfL-style line status, live train tracking, and ticket/
  Delay-Repay support — with first-class handling of operators whose
  routes share trunk track, so an incident is only ever flagged on the
  lines it actually affects."`) — no `viewport` export anywhere in the
  file. `<head>` (lines 92-95) contains only `<ColorSchemeScript
  defaultColorScheme="auto" />`.
- **`frontend/lib/theme.ts:27`**: `export const theme = createTheme({
  primaryColor: 'grape' })` — the app's single documented brand-colour
  decision (comment cites `docs/superpowers/specs/2026-08-18-grape-theme-design.md`).
- **`frontend/app/icon.svg`** (read in full): a 32×32 viewBox favicon.
  Lines 22-23: gradient stops `#be4bdb` (Mantine grape-6) → `#9c36b5`
  (grape-8), with a comment confirming these were verified against
  `@mantine/core`'s `default-colors.mjs`. The signal arm is Mantine
  yellow-4 (`#ffd43b`). The background rect (`line 27`) is `rx="8"` inset
  1px from the 32×32 viewBox — i.e. rounded corners with a ~1px
  transparent margin, not a full-bleed edge-to-edge fill.
- **`frontend/app/apple-icon.png`**: confirmed exactly **180×180** by
  direct PNG-header read this session (`struct.unpack('>II', ...)` on the
  IHDR chunk). Its real provenance, from `git log`/`git show 35cc693`
  (`"Add a favicon: a fishtail-notched distant-signal arm in grape"`):
  *"Also add app/apple-icon.png (180x180, opaque, generated from an
  equivalent full-bleed SVG via sharp, which is already present as a
  transitive dependency)"* — i.e. this file was produced by a **one-off
  `sharp` conversion script**, not committed as a build step and not a new
  project dependency (`sharp` is `optional: true` in
  `frontend/package-lock.json`'s `node_modules/sharp` entry — it's Next.js's
  own optional dependency for self-hosted image optimization, already
  present in `node_modules` without this repo adding it directly). A
  *different, full-bleed* SVG variant was used for this specific file,
  because iOS composites alpha onto black and wants an opaque icon —
  `icon.svg`'s own 1px transparent margin/rounded corners wouldn't have
  been correct here.
- **`frontend/app/globals.css:755-761`**: `body`'s actual background is
  **not** solid grape — it's the scheme-default background plus a subtle
  wash: `background-image: linear-gradient(180deg, color-mix(in srgb,
  var(--mantine-color-grape-6) 6%, transparent) 0%, transparent 70vh)`.
  `frontend/app/globals.test.ts:13` records the real resolved colours this
  computes against: light scheme's `--mantine-color-body` is white
  (`#ffffff`, the CSS default with nothing overriding it), dark scheme's
  is `#242424` (`DARK_7`, confirmed by that file's own comment and used in
  its contrast assertions).
- **No `public/` directory exists anywhere in `frontend/`** — confirmed by
  listing the top-level tree. Every static asset this app serves today
  (`icon.svg`, `apple-icon.png`) lives under `app/` via Next's
  file-convention metadata system, not as a plain static file. This
  matters for where new raster icons can live (Decision 2).
- **`frontend/components/AutoRefresh.tsx`** (read in full): 30s
  `router.refresh()` polling, unrelated to this spec — nothing here
  introduces a cache for it to interact with (no service worker, per the
  research's own recommendation, carried forward unchanged).
- **`frontend/app/layout.test.tsx`** (read in full): tests only
  `TrackedTrainsNavItem`; nothing tests `metadata`'s contents.
  **`frontend/app/globals.test.ts`** (read in full) is this repo's one
  existing precedent for testing a non-component, data-shaped file
  directly (plain `readFileSync` + assertions on CSS content, no
  rendering) — the closest existing pattern to what a `manifest.ts` test
  would look like.
- **Next.js type definitions**, read directly from the sibling checkout's
  `node_modules/next@16.2.10` (this worktree has no local `node_modules`,
  same situation `2026-09-01-dynamic-color-scheme-meta-design.md` already
  noted and worked around the same way):
  - `dist/lib/metadata/types/manifest-types.d.ts`: `app/manifest.ts`'s
    return type (`MetadataRoute.Manifest`) is a snake_case object mirroring
    the Web App Manifest spec directly — confirmed field names
    `background_color`, `description`, `display` (`'standalone'` is a
    valid literal), `icons` (`{ src, type?, sizes?, purpose? }[]`), `name`,
    `short_name`, `start_url`, `theme_color` all exist as typed, optional
    fields.
  - `dist/lib/metadata/types/metadata-interface.d.ts:157,619`: `Metadata.
    themeColor` is `@deprecated` in favour of `Viewport.themeColor`, which
    is **not** deprecated and explicitly supports a single colour, one
    `{ media, color }` descriptor, or an array of them — its own doc
    comment's worked example (lines 605-618) is literally light/dark
    `prefers-color-scheme` pair, rendering as two separate `<meta
    name="theme-color" media="...">` tags.
  - `dist/lib/metadata/metadata.js:627-660`: `Metadata.appleWebApp`'s three
    sub-fields render **independently** — `capable` alone produces `<meta
    name="mobile-web-app-capable" content="yes">`; `title` alone produces
    `apple-mobile-web-app-title`; `statusBarStyle` alone produces
    `apple-mobile-web-app-status-bar-style`. Setting only `statusBarStyle`
    does not implicitly render the other two (relevant to Decision 4).
  - `Metadata.manifest` (line 253) is for linking an **external**
    manifest URL/string; it is not needed for the native `app/manifest.ts`
    file-convention route, which Next auto-discovers and links the same
    way it already does for `icon.svg`/`apple-icon.png` — confirmed by the
    research doc's own §3 finding, not re-litigated here.
- **Fresh platform findings, verified via web search this session** (not
  covered by the merged research doc, which didn't investigate these
  specific questions):
  - `short_name`'s commonly-cited platform guidance is **~12 characters**
    to avoid truncation, corroborated across web.dev, MDN, and Chrome's
    own extension-manifest docs (all independently converge on the same
    number).
  - **iOS Safari has never implemented `display: minimal-ui`** — Apple's
    own developer forums list it explicitly as one of the unimplemented
    Web App Manifest fields, a 2021 W3C mailing-list thread says the same,
    and MDN currently flags `display` overall as "Limited availability."
    2026-dated practical guides for iOS PWAs discuss only `standalone` as
    the workable value.
  - **iOS 26 Safari (shipped 2026) dropped `<meta name="theme-color">`
    support for in-tab/toolbar tinting entirely** — multiple independent,
    dated sources (Ben Frain, Ben Nasedkin, a marked-accepted answer on
    Apple's own developer forums) confirm Safari 26 now derives toolbar
    colour from the CSS `background-color` of fixed/sticky elements near
    the viewport edges, falling back to `<body>`'s background-colour, and
    ignores the meta tag's value even though it still parses without
    erroring.
  - **iOS has never read the manifest's `theme_color`/`background_color`
    for an installed PWA's *running* status bar** — only briefly during
    the splash-screen phase before the app's own CSS has loaded. The only
    mechanism that controls the status bar colour of an already-running,
    home-screen-installed iOS PWA is the separate
    `apple-mobile-web-app-status-bar-style` meta tag (`default` / `black`
    / `black-translucent`), where `black-translucent` specifically means
    "take the tint from `<body>`'s own `background-color`."
  - **`apple-mobile-web-app-capable` (and its generic
    `mobile-web-app-capable` form) is now explicitly discouraged**: current
    guidance (citing web.dev) states using these pre-manifest-era tags
    today "is no longer recommended and may harm the installation
    experience when the browser can't load the manifest properly," since
    they produce a degraded standalone mode lacking `scope`/`start_url`.
    iOS 26 additionally now defaults every home-screen add to a
    standalone-like experience regardless of any tag or manifest at all
    (per the merged research doc's own §2 finding), making the legacy tag
    doubly unnecessary.

## Decisions

### 1. Manifest content

```ts
// frontend/app/manifest.ts (illustrative — design only, not written here)
{
  name: 'Distant Signal',
  short_name: 'Distant Signal',
  description: 'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.',
  start_url: '/',
  display: 'standalone',
  background_color: '#ffffff',
  theme_color: '#be4bdb',
  icons: [
    { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
    { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
  ],
}
```

- **`name`/`short_name`: both `"Distant Signal"`, no abbreviation.** A
  repo-wide grep for any existing shorthand (`"DS"`, `"Dist. Signal"`, or
  similar) found none — the app's name is used verbatim in exactly three
  places (`layout.tsx:17`'s `title`, `layout.tsx:128`'s nav-bar text,
  `page.tsx:107`'s home-page `<Title order={1}>`), never abbreviated.
  `"Distant Signal"` is 14 characters, two over the ~12-character platform
  guidance found above. **Considered and rejected: inventing a shorter
  form** (e.g. `"DistSignal"`, `"D. Signal"`) — a two-character overage on
  an already-short, two-word name is a mild, common case most launchers
  handle by wrapping to a second line rather than truncating mid-word;
  inventing new brand nomenclature with zero precedent anywhere in this
  codebase, to shave two characters, is a bigger and more speculative
  decision than this spec's scope. Flagged honestly in Open questions —
  accepted, not solved.
- **`description`**: a trimmed version of the real `layout.tsx:18-19`
  description's opening clause, dropping the trailing "first-class
  handling of operators whose routes share trunk track..." clause — that
  detail is a technical/SEO differentiator relevant to a search snippet,
  not to a compact install-dialog description. The research doc's own §2
  sketch offered materially this same trimmed text as an *illustrative
  paraphrase only, explicitly not a proposal*; this spec adopts it as the
  actual value, now that turning the sketch into a real decision is
  exactly this document's job.
- **`start_url: '/'`**, plain. **Considered and rejected: a
  `?source=pwa`-style query param** for future install-source analytics —
  this app has no analytics/telemetry infrastructure anywhere in
  `frontend/` today (nothing found in `package.json`'s dependency list or
  anywhere else this session), so a param with nothing to consume it would
  be speculative.
- **`display: 'standalone'`** — see Decision on iOS/Android behaviour
  below; not `minimal-ui` (unsupported on iOS, confirmed above) and not
  `fullscreen` (research's §2 already characterizes this as the outlier,
  not asked for, and iOS doesn't distinguish it from `standalone` in
  practice per the same forum thread).
- **`background_color: '#ffffff'`** — the manifest's `background_color`
  field is a **static, single value**; this session's web search confirmed
  there is still no shipped, standardized per-`prefers-color-scheme` array
  form for it inside `manifest.json` itself (unlike `Viewport.themeColor`,
  see Decision 3) — a real W3C proposal for exactly that (`theme_colors`
  array, `media_overrides`) exists but hasn't shipped. Given it has to be
  one fixed value and the manifest can't know the visitor's stored/system
  preference at parse time, `'#ffffff'` mirrors this app's own light-mode
  default background and its own established convention for "what to show
  when the scheme is unknown" — `ThemeToggle.tsx`'s pre-mount fallback is
  already hardcoded to `'light'` for the same reason (documented in
  `2026-09-01-dynamic-color-scheme-meta-design.md`'s Current relevant
  state). This is a splash-screen colour, not a live theme signal — it
  will be visibly wrong for roughly half of dark-mode-preferring visitors
  for the brief pre-CSS-load flash, the same known, accepted limitation
  the sibling spec already named for its own static fallback.
- **`theme_color: '#be4bdb'` (grape-6)** — the app's one documented brand
  colour, matching `icon.svg`'s gradient start and `theme.ts:27`'s
  `primaryColor`. This is the manifest's splash-screen/Android
  task-switcher-card colour — **deliberately not** matched to the page's
  actual near-white/near-black body background the way the *live*
  in-page `theme-color` tag is (Decision 3): a splash/switcher moment is a
  branding opportunity in a way a live browser-chrome blend isn't, and the
  research doc's own §2 sketch already reasoned identically ("matches the
  icon's dominant colour... not the near-white body background").
- **`icons`**: exactly the two files Decision 2 adds, `192×192` and
  `512×512`, `type: 'image/png'`, no `purpose` field (i.e. the spec
  default, `"any"`) — matches the task's own framing of "the two new icon
  assets" as the installability floor. No maskable-purpose entry; see
  Decision 2's last paragraph.
- **No `apple-icon.png` entry added to `icons`.** iOS never reads the
  manifest's `icons` array for its home-screen icon — it exclusively uses
  the dedicated `apple-touch-icon` `<link>` that `apple-icon.png`'s
  existing file convention already produces (research §2). The two
  mechanisms are genuinely non-overlapping; cross-referencing one from the
  other would add nothing for either platform.
- **No `scope`, `id`, `orientation`, or `display_override` fields.** This
  is a single-origin, single-scope app with one manifest and no plan to
  change `start_url`'s meaning over time, and it isn't a
  graphics/orientation-sensitive app — adding fields nothing here needs
  would be the kind of unrequested complexity this repo's existing
  minimal-config files (`next.config.mjs`, `package.json`'s short
  dependency list) consistently avoid elsewhere.

### 2. Icon generation: two static PNGs via the same one-off `sharp` conversion already used for `apple-icon.png` — not Next's `ImageResponse` convention

**Chosen: hand-generate `icon-192.png` and `icon-512.png` once, from
`icon.svg`'s existing artwork, using the same `sharp`-based one-off script
`apple-icon.png` was already produced with** (commit `35cc693`, Current
relevant state above) — not committed as a build-time step, not a new
project dependency (`sharp` is already present, transitively, as Next's
own optional dependency).

**Unlike `apple-icon.png`, no separate full-bleed SVG variant is needed.**
`apple-icon.png` needed one because iOS composites an icon's alpha channel
onto black, and `icon.svg`'s rounded rect (`rx="8"`, 1px transparent
margin) would have shown as a visible black ring on iOS. Chrome's manifest
icons (default `purpose: "any"`) have no such opacity requirement — a
`purpose: "any"` icon is expected to be shown as the app itself intends,
transparency and all, the same way a favicon is. So `icon-192.png`/
`icon-512.png` can be a **direct rasterization of `icon.svg` exactly as
authored** — same rounded-square background, same signal-arm mark — just
re-exported at two larger fixed sizes, not a redesign.

**Considered: Next's native `ImageResponse`/`icon.tsx` dynamic-generation
convention**, since the task explicitly asked whether generating these
programmatically from `icon.svg` at request time is preferable to two more
static exports. **Rejected**, for two concrete reasons:

1. **`ImageResponse` (Satori-based) doesn't natively re-render this SVG.**
   Satori interprets a constrained HTML/CSS-like subset of JSX, not
   arbitrary SVG markup — `icon.svg`'s `<linearGradient>` def and its
   rotated `<polygon>` fishtail arm (`icon.svg:34-38`) can't be pasted in
   as-is. The only way to reuse the *exact* source art would be embedding
   the raw SVG as a base64 `data:` URI inside an `<img>` element for
   Satori to rasterize as an image (technically workable, since Satori
   does support image elements) — but that's a materially different,
   more fragile mechanism than "the file already exists as an image,"
   not a simplification.
2. **This content has no reason to be generated per-request.** These two
   icons are 100% static — nothing about them varies by request, query
   string, or auth state (the same limitation the research doc's §3
   already noted for `manifest.ts` itself: it "doesn't receive the
   incoming request"). Introducing a runtime Satori/`resvg` render path
   for content that never changes would add real, ongoing render cost
   (every cold start or every cache-miss) for zero functional benefit
   over a file generated once and committed — and it would be a wholly
   new pattern for this codebase (no `next/og` usage exists anywhere
   today), when a working, already-precedented pattern
   (`apple-icon.png`'s own `sharp` export) already covers exactly this
   need.

**File location: `frontend/public/icon-192.png` and
`frontend/public/icon-512.png` — this repo's first `public/`
directory.** Next.js's App Router only serves an arbitrary static file at
a predictable URL from `public/`; files placed directly under `app/` are
only served when they match one of Next's own recognized special
filenames (`page.tsx`, `route.ts`, `icon.(svg|png|jpg|ico)`, numbered
`icon0`/`icon1` variants, etc.) — a hyphenated name like `icon-192.png`
isn't part of that recognized pattern and wouldn't be reachable at all if
placed in `app/`. Introducing `public/` is new to this repo but is an
entirely standard, zero-risk Next.js convention, not a novel one.
`manifest.ts`'s `icons` array then references them as plain root-relative
paths, `/icon-192.png` and `/icon-512.png` — matching the research doc's
own illustrative sketch verbatim.

**Considered and rejected: a maskable (`purpose: "any maskable"`) third
icon.** The research's §2 already characterized this as "recommended, not
required," and its own Open question 3 left Chrome's exact current
strictness about `purpose` unresolved. A maskable icon needs a genuinely
different, full-bleed "safe zone" composition (no existing rounded-corner
padding, since the launcher applies its own mask) — a real design task in
its own right, not a re-export of the existing artwork the way the two
required icons are. The task's own framing ("the two new icon assets")
matches treating this as out of scope for this pass; noted in Explicitly
out of scope.

### 3. `theme-color` meta tag: add via `layout.tsx`'s new `viewport` export, split light/dark to match the app's real backgrounds — separate from, not redundant with, the manifest's brand-grape `theme_color`

**Chosen:**

```ts
export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#242424' },
  ],
};
```

`Viewport.themeColor` natively supports this exact `{ media, color }[]`
array form (Current relevant state above — it's the worked example in
Next's own type doc comment), rendering two separate `<meta
name="theme-color" media="...">` tags resolved entirely by the browser's
native CSS media-query evaluation — no client JavaScript, no hydration
concern, nothing to keep in sync at runtime for the common case.

**Values chosen to match this app's actual rendered background**
(`#ffffff` light / `#242424` dark, `globals.test.ts:13`/`globals.css:755-761`),
**not** the manifest's `#be4bdb` brand grape. This is a deliberate,
reasoned split, not an oversight: the manifest's `theme_color` paints a
splash screen / Android task-switcher card — a branding moment, where a
bold, deliberate colour is a normal, common choice (Decision 1). This
`viewport.themeColor` tag instead tints the *live* browser toolbar/status
bar while the page is actually open — its entire purpose is to make the
chrome blend with what's already on screen. Since this app's own body
background is never solid grape (only a ~6%-opacity wash over the
scheme's real background, confined to the top 70vh), tinting the toolbar a
fully-saturated grape would create a visible colour seam right where the
toolbar meets the page, not a blend. **Not redundant with `theme_color`**:
the two fields serve different runtime contexts (live in-page/standalone
chrome vs. static splash/switcher) and, per this session's manifest-spec
research, there is no shipped mechanism for the manifest's own field to
vary by scheme the way `Viewport.themeColor` can — so picking different,
purpose-appropriate values for each is the correct call, not an
inconsistency to resolve.

**Honest limitation, not solved here**: because these two tags are gated
by `prefers-color-scheme` media queries evaluated natively by the browser,
they track the visitor's **system** preference correctly and automatically
— but this app also lets a visitor manually force light or dark via
`ThemeToggle`, independent of system preference (stored in
`localStorage['mantine-color-scheme-value']`, per
`2026-09-01-dynamic-color-scheme-meta-design.md`'s own findings). A
visitor whose OS is in light mode but who has manually switched the app to
dark will see a `theme-color` tag that still says light, until/unless
something rewrites it client-side. The natural fix, if this gap is ever
judged worth closing, is extending that same sibling spec's (currently
unimplemented) `ColorSchemeMeta` component to also own these tags
post-mount, via `useComputedColorScheme` — the identical mechanism it
already designs for the `color-scheme` tag. **Not designed further here**
— see Explicitly out of scope.

**iOS caveat, freshly verified this session**: on iOS 26 Safari
specifically, this tag no longer affects the in-tab toolbar tint at all —
Safari 26 dropped the behaviour entirely in favour of deriving toolbar
colour from CSS background-colour of fixed/sticky elements or `<body>`
(Current relevant state above). This spec still adds the tag — it's free,
harmless, and still functions on Android Chrome (both tab and standalone)
and on pre-26 iOS — but its near-term real-world payoff for this app's
**primary**, iOS-focused audience is smaller than it would have been a
year ago, and shouldn't be oversold as an iOS win in particular.

### 4. iOS-specific meta tags: add exactly one (`apple-mobile-web-app-status-bar-style`), reject the other two

**Chosen: `metadata.appleWebApp = { statusBarStyle: 'black-translucent' }`**
in `layout.tsx`'s existing `Metadata` export. Per
`metadata.js:627-660`'s independent per-field rendering (Current relevant
state above), this produces exactly one new tag —
`<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">`
— without also emitting `mobile-web-app-capable` or
`apple-mobile-web-app-title`.

- **Why this one is worth adding**: verified this session (Current
  relevant state) that iOS never reads the manifest's `theme_color`/
  `background_color` for an *already-running* installed PWA's status bar
  — only briefly during the splash phase — so nothing else this spec adds
  covers that gap. `apple-mobile-web-app-status-bar-style` is the one
  real, current, working mechanism for it. **`black-translucent` is
  chosen specifically because it makes the status bar take its tint from
  `<body>`'s own `background-color`** — which, in this app, is already
  scheme-aware (white light / `#242424` dark, per Decision 3's citations).
  That means the status bar auto-corrects for light/dark exactly the way
  the rest of the page already does, with a single static meta-tag value,
  rather than needing a second static compromise colour the way
  `background_color` (Decision 1) does.
- **Why `mobile-web-app-capable`/`apple-mobile-web-app-capable` is
  rejected**: verified this session that current guidance explicitly
  discourages it — "may harm the installation experience when the browser
  can't load the manifest properly," since the legacy tag alone produces a
  degraded standalone mode missing `scope`/`start_url`. The manifest's own
  `display: 'standalone'` (Decision 1) is the modern, recommended
  mechanism for the same outcome, and iOS 26 already defaults every
  home-screen add to a standalone-like experience regardless of any tag.
  Adding a discouraged, redundant legacy tag has a real (if small)
  downside and no verified upside — a clean rejection, not an oversight.
- **Why `apple-mobile-web-app-title` is rejected**: it would carry exactly
  the same string as `name`/`short_name` (`"Distant Signal"`) — a
  repo-wide grep confirmed **no route in this app defines its own
  page-level `<title>`** (`layout.tsx`'s single, static title applies
  everywhere), so there is no scenario in which a distinct Apple-specific
  home-screen title would ever differ from what the manifest's own
  `short_name` already provides. Adding it would be pure duplication with
  no observable effect.

This directly answers the task's own framing: this session's fresh
verification (not the merged research doc, which didn't investigate these
specific tags) supports exactly one addition and specifically argues
against the other two — not a blanket "add the usual Apple meta tags"
move.

### 5. Testing: minimal and honest — most of this is manual

- **`frontend/app/manifest.test.ts`** (new, colocated): following
  `globals.test.ts`'s existing precedent for testing a non-component,
  static/data-shaped file directly — plain assertions against the
  returned object (`icons` has exactly the two expected `sizes`/`type`
  entries, `display`/`start_url`/`name`/`short_name` hold the values
  Decision 1 specifies). Low-value relative to just reading the file
  (same conclusion the research doc's own §4 already reached), but
  stylistically consistent with an established pattern this codebase
  already uses elsewhere, so worth the few lines.
- **`frontend/app/layout.test.tsx`** (extend, doesn't already import
  `viewport`/`metadata`): plain assertions on the new `viewport.themeColor`
  array (both `media`/`color` pairs correct) and
  `metadata.appleWebApp.statusBarStyle`, mirroring exactly the pattern
  `2026-09-01-dynamic-color-scheme-meta-design.md`'s own Decision 5/
  Testing section already sets for that spec's sibling `viewport.
  colorScheme` field — no rendering needed, these are data assertions.
- **Everything else is manual, and this spec says so plainly rather than
  inventing tests to paper over it**: whether "Add to Home Screen"
  actually produces a correctly-iconed, correctly-titled, chrome-less
  launch on a real iOS device (Simulator or hardware); whether Chrome
  DevTools'/Lighthouse's installability audit actually passes; whether the
  two new icons render correctly at real launcher/install-dialog sizes on
  Android; whether the installed iOS PWA's status bar actually
  auto-adapts light/dark as Decision 4 intends. None of this is
  unit-testable in this codebase's existing Vitest/jsdom setup (the
  research doc's own §4 already established Vitest doesn't host a
  meaningful PWA-install environment), and this spec doesn't propose
  Playwright coverage for it either — a real device/emulator check is the
  only thing that actually verifies an install flow, and that's a manual
  step for whoever implements this, not a gap this spec tries to close
  with the wrong tool.

## Architecture

```
frontend/
├── app/
│   ├── layout.tsx           MODIFIED: adds `viewport` export
│   │                          (themeColor light/dark pair) and extends
│   │                          `metadata` with `appleWebApp.statusBarStyle`
│   ├── manifest.ts           NEW: MetadataRoute.Manifest, Decision 1's
│   │                          object literal — Next auto-links this into
│   │                          <head> the same way icon.svg/apple-icon.png
│   │                          already are, no manual <link> needed
│   ├── icon.svg               unchanged — remains the sole favicon
│   ├── apple-icon.png         unchanged — already 180×180, already correct
│   └── manifest.test.ts      NEW: shape assertions (Decision 5)
└── public/                    NEW directory (first in this repo)
    ├── icon-192.png           NEW: rasterized from icon.svg via sharp,
    │                            one-off script (Decision 2)
    └── icon-512.png           NEW: same

Resulting <head> additions (illustrative, not exhaustive):
  <link rel="manifest" href="/manifest.webmanifest">
  <meta name="theme-color" media="(prefers-color-scheme: light)" content="#ffffff">
  <meta name="theme-color" media="(prefers-color-scheme: dark)" content="#242424">
  <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">
```

No change anywhere to `AutoRefresh.tsx`, `app/api/[...path]/route.ts`, or
any data-fetching path — this entire feature is static configuration and
two static image files, with no runtime behaviour of its own.

## Error handling

This is almost entirely static configuration, so there's little to design
here:

- **A broken or missing icon file** (e.g. a bad path in `manifest.ts`'s
  `icons` array) degrades silently — Chrome/Android just falls back to no
  icon or a generic placeholder for the install prompt; there's no
  page-visible error and no error boundary to add, the same "quiet
  degrade" character `icon.svg`/`apple-icon.png` already have today.
- **`manifest.ts` itself has no failure mode to guard against**: per the
  research doc's own §3 finding, this file convention doesn't receive the
  incoming request and can't vary by query string or auth state — it's a
  static object literal, not a data fetch, so there's no async/error path
  the way there is for, say, `getDataFreshness()` elsewhere in
  `layout.tsx`.
- **No interaction with the `AutoRefresh`/`/api` proxy caching-correctness
  concerns** the research doc's §1 raised at length — nothing in this
  spec introduces a cache of any kind (no service worker, unchanged from
  the research's recommendation), so that entire risk class doesn't apply
  here.

## Explicitly out of scope

- **Any service worker, offline caching, or push notifications** —
  carried forward unchanged from the research doc's own explicit
  out-of-scope list; not re-litigated here.
- **A maskable icon variant** — "recommended, not required" per research
  §2; would need a genuinely new full-bleed "safe zone" composition, not
  a re-export of existing artwork (Decision 2).
- **An iOS "how to install" UI nudge** (since iOS has no
  `beforeinstallprompt` to hook a native prompt off) — the research doc's
  own Open question 2, still unanswered; a product/UX decision, not
  addressed by a manifest-and-icons spec.
- **Extending `ColorSchemeMeta` (or any new component) to post-mount-sync
  the `theme-color` tags for a manual `ThemeToggle` override** — named as
  the natural follow-up in Decision 3, not designed further here.
- **Any change to `icon.svg`'s or `apple-icon.png`'s actual artwork** —
  both are reused exactly as they exist today; `icon-192.png`/
  `icon-512.png` are re-exports of the same source, not a redesign.
- **Chrome's exact current strictness around `beforeinstallprompt`'s
  service-worker requirement, or `purpose: "any maskable"` format
  strictness** — both already flagged as unresolved by the research doc's
  own Open questions; unchanged by this spec.

## Open questions/risks

1. **`display: 'standalone'` removes the browser's back-button chrome,
   and several of this app's inner pages have no in-app back link to
   compensate.** `app/lines/[id]/page.tsx` and `app/stations/[crs]/page.tsx`
   — both directly reachable from the home page's pinned-lines/pinned-
   stations lists — were checked this session and have no "back to home"
   or "back to list" link anywhere in their own markup; only three pages
   in this whole app do (`app/lines/[id]/history/page.tsx`,
   `app/lines/[id]/not-found.tsx`, `app/incidents/[id]/not-found.tsx`).
   `minimal-ui` isn't a real fix (unsupported on iOS, confirmed above),
   so there is no display-mode choice that solves this — an iOS PWA user
   who drills into a line or station detail page has no way back except
   whatever in-app links that specific page happens to have. Not fixed
   here; worth flagging to whoever implements this as a real, current
   navigation gap this feature would newly expose for standalone users,
   independent of anything this spec adds.
2. **`theme-color`'s per-scheme accuracy is system-preference-only**, not
   manual-override-aware, until/unless a `ColorSchemeMeta`-style client
   sync is added later (Decision 3) — a known, accepted gap, not a bug to
   fix in this pass.
3. **iOS 26 dropped in-tab `theme-color` support entirely** — this
   decision's near-term real value skews toward Android and older iOS
   versions; worth knowing so nobody implementing or reviewing this is
   surprised the toolbar doesn't visibly tint on a fresh iOS 26 device.
4. **`short_name` at 14 characters is a couple over the ~12-character
   platform guidance** — accepted, not solved (Decision 1); could
   truncate on some launchers.
5. **Maskable-icon support and Chrome's exact current strictness about
   `purpose`/format** remain open per the research doc's own Open
   questions — unchanged, not re-investigated here.
6. **Nothing in this spec substitutes for a real-device check.** Whoever
   implements this should manually verify "Add to Home Screen" on actual
   iOS hardware (or Simulator) and Chrome's installability audit before
   considering the feature done — Testing (above) says this plainly
   rather than proposing automated coverage that wouldn't actually catch
   a platform-specific regression.
