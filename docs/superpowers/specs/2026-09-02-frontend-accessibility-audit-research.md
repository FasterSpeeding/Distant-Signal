# Frontend Accessibility Audit — Research

**Status: research/audit only, not an approved fix plan.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(structural template: Goal, Method, findings with `file:line` citations, a
ranked Recommendation, explicit out-of-scope and open-questions sections).
No source file under `frontend/` (outside two temporary, now-deleted
scratch ESLint configs used only to run the linter, and a temporary
`node_modules` install) was left modified while producing this document.
This is an audit; a follow-up implementation plan should design and apply
the fixes.

## Goal

Run a real accessibility audit of Distant Signal's Next.js/Mantine
frontend (`frontend/`), combining **static** analysis (source code) and
**dynamic** analysis (the live rendered app at
`https://konata.fox-prometheus.ts.net`), since the two catch different
classes of defects — static tools catch missing ARIA attributes and
markup-pattern mistakes before anything renders; dynamic tools catch
computed-style problems (contrast, focus order, DOM landmark structure)
that only exist once the browser has actually painted the page. Deliver a
severity-ranked findings report with file:line citations and prioritized
recommendations, no code changes.

## Method

### Part 1 — Static analysis

**`eslint-plugin-jsx-a11y` was not present in this project at all** —
confirmed by reading `frontend/package.json` (no `eslint`,
`eslint-config-next`, or `eslint-plugin-jsx-a11y` in `dependencies` or
`devDependencies`), grepping `frontend/package-lock.json` for `jsx-a11y`
(zero hits), and finding no `.eslintrc*`/`eslint.config.*` anywhere in the
repo. `next.config.mjs` has no `eslint` block either. This is a real gap,
not a false negative: `eslint-config-next` (which does bundle
`eslint-plugin-jsx-a11y` by default) is simply never installed here, so no
a11y linting has ever run against this codebase, in CI or locally.

To actually run it: installed `eslint@9`, `eslint-plugin-jsx-a11y@latest`,
and `@typescript-eslint/parser@latest` into `frontend/node_modules` via
`npm install --no-save` (not saved to `package.json`/lockfile — scratch
tooling only), plus ran a plain `npm ci` first to get the project's real
dependency tree so the parser could resolve JSX/TSX correctly. Wrote a
temporary flat-config file at `frontend/eslint.config.a11y-audit.mjs`
(`jsx-a11y`'s `flatConfigs.recommended`) and a second at
`eslint.config.a11y-strict.mjs` (`flatConfigs.strict`), then ran:

```
npx eslint -c eslint.config.a11y-audit.mjs "app/**/*.tsx" "components/**/*.tsx"
npx eslint -c eslint.config.a11y-strict.mjs "app/**/*.tsx" "components/**/*.tsx"
```

covering all 122 real `.tsx` files under `app/` and `components/`
(confirmed via `find . -name "*.tsx"`; there are zero `.jsx` files in this
project). **Both rulesets returned zero violations.** To rule out a
silent misconfiguration (e.g., the plugin loading but its rules object
being empty), the setup was validated against a deliberately broken
scratch file (`<img src=... />` with no `alt`, a `<div onClick>` with no
keyboard handler, an `<a>` with no `href`) placed temporarily under
`app/__a11ytest__/` — it correctly produced 4 errors across
`jsx-a11y/alt-text`, `jsx-a11y/click-events-have-key-events`,
`jsx-a11y/no-static-element-interactions`, and `jsx-a11y/anchor-is-valid`
— then the scratch file was deleted. Both temporary ESLint config files
and the extra `node_modules` install were removed/left untracked before
finishing this audit; `frontend/node_modules/` is gitignored, so nothing
from this setup is committed.

Supplemented with a manual/grep-assisted pass (no tool exists for these
specific Mantine-shaped questions) covering: icon-only buttons/`ActionIcon`
labeling, form-input/label association, color-only status indicators,
missing `alt`/decorative-icon handling, keyboard operability of
`PinToggle`/`Dropzone`/`LoginPromptModal`, and heading hierarchy across
`app/**/*.tsx` (every `<Title order={...}>` call site, cross-referenced
against literal `<h3>`/`<h4>` usage and Mantine component internals where
a component — e.g. `Accordion`, `Title` — controls the rendered heading
level itself). Confirmed a few claims against the *installed* library
source (`node_modules/@mantine/core`, `node_modules/@mantine/dropzone`)
rather than assuming Mantine's documented defaults still hold at the
pinned `9.5.2`/`9.5.2` versions.

### Part 2 — Dynamic analysis

Used the Playwright MCP browser tools already available in this session
against the real deployment (`https://konata.fox-prometheus.ts.net`),
logged in as the supplied disposable test account
(`ds-test-2026-09-02`). No `@axe-core/playwright` package or a way to run
real Playwright *test* code was available in this session (MCP only
exposes browser-automation tool calls, not a Node test runner) — so
**axe-core was injected directly into the live page via
`browser_evaluate`**, loading `axe-core@4.10.2` from
`cdnjs.cloudflare.com` (the CSP-allowlisted CDN) with a `<script>` tag,
then calling `window.axe.run(document, { resultTypes: ['violations'] })`
per page. This is a legitimate, standard way to run axe-core outside a
test harness and is what axe's own browser extension does internally.

**A real tooling limitation hit and worked around:** a sibling agent was
concurrently taking screenshots of the same app in the same session, and
the Playwright MCP server turned out to route browser actions to a single
shared "current" page object rather than one page per calling agent —
opening a second tab did not isolate the two agents' navigations, and
several `browser_navigate`/`browser_evaluate` calls landed on whatever URL
the other agent had just navigated to mid-flight. Worked around by having
every axe invocation report back `location.href` in its own result and
re-running any call whose returned URL didn't match the intended route
(most results below still cite an exact URL for this reason, and a few
races incidentally produced *bonus* coverage of routes not in the
required list — noted where relevant). One planned session-state action
(logging out to test anonymous behavior, then logging back in) was done
once, deliberately, using the explicitly-disposable test account, and
restored immediately afterward via the same SSO session (no credential
re-entry needed) to minimize disruption to the concurrent sibling agent.

Routes covered, logged in unless noted (`(anon)` marks a logged-out run):
`/` (both), `/lines`, `/lines/new`, `/lines/gwr-main-line` (real line),
`/lines/gwr-main-line/history` (both Timeline and Trends tabs),
`/incidents/E879EB6C791C470AB6C2A7458AE68C3B` (real, live Knowledgebase
incident, found by expanding an `IssueList` accordion row to its
`DisruptionDetail` "View full incident details" link), `/stations`,
`/stations/PAD` (both, plus with `LoginPromptModal` open anonymously),
`/track/mine`, `/track/mine/add-ticket`, `/chat`, `/connect-claude`, and
(bonus, from race conditions) `/lines/custom-ds-test-custom-line`,
`/lines/custom-ds-test-custom-line/edit`, `/track` (both), and a live 404
(`/lines/nonexistent-line-slug`). `/connect-claude` reproducibly 500'd
(see Findings) — only its `error.tsx` fallback state could be scanned, not
the intended page content.

## Summary — findings by severity

| Severity | Count (distinct findings) | Issue types |
|---|---|---|
| Critical | 0 | — |
| Serious | 6 | Color-contrast failures (6 distinct color pairings, each recurring across most pages) |
| Moderate | 5 | Missing `<main>` landmark (1, sitewide) · content outside landmarks (1, same root cause) · heading-level skips (2 distinct root causes) · missing page-level `<h1>` on error/not-found states (1, 6 files) |
| Minor | 0 | — |
| **Static (`eslint-plugin-jsx-a11y`)** | **0** | Zero violations, recommended *and* strict rulesets, 122 files |

No axe-core rule of `critical` impact fired on any of the ~16 page states
scanned. The static linter — once actually installed and run — found
nothing at all; every real defect below was found either by manual
source review (heading hierarchy) or by axe-core's live DOM/computed-style
checks (contrast, landmarks), which is exactly the "different classes of
issues" split this audit was meant to demonstrate.

## Findings

### Serious

#### Color-contrast failures (WCAG 1.4.3, axe rule `color-contrast`)

Found on nearly every page carrying a `StatusBadge`, the incident-category
`Badge`, dimmed body text, or a `primaryColor="grape"` filled
button/active-tab — i.e., most of the app. All six pairings below use
Mantine's *default* shade-6 palette values at small/bold badge or button
text (11px/14px), which axe itself confirms need 4.5:1 and get nowhere
close for three of the six:

| Pairing | Component / file:line | Measured ratio | Where seen live |
|---|---|---|---|
| White on yellow `#fab005` (severity group `mild`, e.g. "Minor Delays", "Reduced Service") | `components/StatusBadge.tsx:9-15` (`variant="filled"`) via `lib/severity.ts:5-40` `GROUP_COLOR.mild` | **1.86:1** | `/lines`, `/lines/gwr-main-line` (worst of all measured) |
| White on green `#40c057` (severity group `good`) | same `StatusBadge.tsx:11`, `GROUP_COLOR.good` | **2.36:1** | `/`, `/lines`, `/lines/custom-ds-test-custom-line` |
| White on orange `#fd7e14` (`Badge` for `!incident.isPlanned`, "Real-Time") | `app/incidents/[id]/page.tsx:58` — no `variant` prop, so Mantine's `Badge` default (`variant: "filled"`, confirmed at `node_modules/@mantine/core/cjs/components/Badge/Badge.cjs:17`) applies | **2.57:1** | `/incidents/[id]` |
| White on red `#fa5252` (severity group `severe`) | `StatusBadge.tsx:11`, `GROUP_COLOR.severe` | **3.28:1** | `/`, `/stations/PAD` |
| Gray `#868e96` dimmed text on white | Every `<Text c="dimmed">` call site (e.g. `app/lines/[id]/page.tsx:139-140`, `components/RepresentativeInfo.tsx`, dozens more) | **3.32:1** | virtually every page |
| White on grape `#be4bdb` (`primaryColor`, filled buttons / active `Tabs`/date-preset buttons) | `frontend/lib/theme.ts:27` (`primaryColor: 'grape'`) | **4.02:1** | `/lines/new`, `/lines/[id]/history` (date presets), `/track/mine/add-ticket` (active tab) |

**The grape-6 gap is already known and deliberately, explicitly deferred**
— it is not a new discovery. `frontend/lib/theme.ts:10-26` documents the
exact 4.02:1 measurement and explains why the obvious fix
(`primaryShade: 7`) was rejected: `primaryShade` isn't scoped to
`primaryColor` in Mantine, so raising it would also have shifted every
`StatusBadge` filled shade, breaking a stated non-goal. The recommended
fix already on record is a **scoped `variantColorResolver` override for
grape only** (grape 7 = `#ae3ec9` = 4.85:1, clears AA), not implemented
yet. `docs/superpowers/specs/2026-08-18-grape-theme-design.md:156-162`
tracks this as "still open." That same design doc, at lines 187-189,
explicitly flagged **"dimmed grey body text, white-on-amber badges" as
unmeasured concerns** and said "this is a good moment to run an automated
pass rather than trust eyeballs" — this axe run is, as far as this
research can tell, the first time that pass has actually been run. It
confirms both flagged concerns fail AA, and additionally reveals the
green/red/orange badge pairings were never flagged at all and also fail
(red only narrowly).

**Test coverage reality check:** `frontend/app/globals.test.ts:31-56` unit
tests the WCAG contrast formula, but only for the grape/anchor-link color
— not for any `StatusBadge` shade or the dimmed-gray text. The severity
badges (the single most-repeated colored UI element in the entire app)
have zero contrast test coverage today.

Severity badges do, as design intent requires, always carry a text label
alongside color (`StatusBadge.tsx:12`, `severityLabel(severity)`) — so
this is purely a **contrast** defect (can the text be read at all), not a
**color-only-meaning** defect (WCAG 1.4.1, which this app already handles
correctly — see Moderate/Minor section below).

### Moderate

#### Missing `<main>` landmark, sitewide (WCAG 1.3.1 / 2.4.1, axe rules `landmark-one-main` + `region`)

Fired on **every single page tested**, logged in or out — `/`, `/lines`,
`/lines/new`, `/lines/gwr-main-line`, `/lines/gwr-main-line/history`
(both tabs), `/incidents/[id]`, `/stations`, `/stations/PAD`,
`/track/mine`, `/track/mine/add-ticket`, `/chat`, `/connect-claude`
(error state), `/track`. Root cause, confirmed by reading the layout:
`app/layout.tsx:184` renders all page content as
`<Container size="lg" px={0}>{children}</Container>` — Mantine's
`Container` defaults to a plain `<div>`, not `<main>`. The nav bar is
correctly landmarked (`<Box component="nav">`, `app/layout.tsx:145`) and
the footer is correctly landmarked (`component="footer"`,
`components/OpenDataAttribution.tsx:77`) — only the actual page content in
between has no landmark at all. This is a single-line root cause behind
two separate axe rules and, on content-heavy pages, hundreds of
"not contained by landmarks" node reports (487 on `/lines`'s full line
table alone) that are really one bug, not 487.

#### Heading-level skips (WCAG 1.3.1, axe rule `heading-order`)

Two distinct root causes, both confirmed live by axe's `heading-order`
rule as well as by static reading of the `Title`/heading call sites
(`Title`'s `order` prop sets the actual rendered tag — confirmed against
`node_modules/@mantine/core/cjs/components/Title/Title.cjs:35-53`,
`component: \`h${order}\``, `size` only changes font-size, not tag):

1. **`TrendsCharts` hardcodes `<Title order={4}>` regardless of caller
   depth.** `app/lines/[id]/history/TrendsCharts.tsx:86` and `:124`
   ("Delay / cancellation / skip rate", "Average delay (minutes)") always
   render literal `<h4>`, but neither page that mounts this shared
   component ever has an `<h3>` on the page at all:
   - `/lines/[id]` (e.g. `/lines/gwr-main-line`): `app/lines/[id]/page.tsx:110`
     is `<h1>` (line name), `:162` is `<h2 size="h4">` ("Recent trends") —
     then `TrendsCharts`' literal `<h4>` follows directly, skipping `h3`.
   - `/lines/[id]/history` Trends tab: `app/lines/[id]/history/page.tsx:74`
     is the page's only `<h1>` ("History: {name}") — no `h2` or `h3`
     anywhere on this tab, so `TrendsCharts`' `<h4>` skips both levels at
     once.
2. **The Timeline tab's per-day headers skip `h2`.**
   `app/lines/[id]/history/page.tsx:74` (`<h1>`) is followed directly by
   `:167`'s `<Title order={3} size="h5">{formatDate(...)}</Title>` (one
   per day group) — no `h2` exists on this tab either.

#### No page-level `<h1>` on not-found/error states (WCAG 1.3.1 best practice, axe rule `page-has-heading-one`)

Confirmed live on a real 404 (`/lines/nonexistent-line-slug`) and on the
`/connect-claude` 500's `error.tsx` fallback (see below) — both have a
top-level heading, but at `order={2}`, so the page never has an `<h1>` at
all. The same pattern repeats by static grep across every not-found
template in the app, all using `<Title order={2}>`, never `order={1}`:
`app/lines/[id]/not-found.tsx:7`, `app/incidents/[id]/not-found.tsx:7`,
`app/stations/[crs]/not-found.tsx:7`,
`app/train/by-id/[trackingId]/not-found.tsx:7`,
`app/train/[uid]/[date]/not-found.tsx:7`, and the shared `app/error.tsx:14`.

### Minor

None found via either tool. (`TrackedTrainStatusBadge`'s `variant="light"`
on-time/late badges on the home page could not be exercised — the test
account has no tracked trains — see Open questions.)

### Positive findings — deliberately verified, not just "no news"

Worth recording explicitly since the task asked for a genuine audit, not
a one-sided defect list:

- **Icon-only buttons are correctly labeled.** Every `ActionIcon` in the
  app (`ThemeToggle.tsx:59-65`, `PrideToggle.tsx:139-150`,
  `PinToggle.tsx:106-124`, `LineDefinitionTooltip.tsx:34`,
  `DataFreshnessInfo.tsx:48`) carries a real, state-reflecting
  `aria-label` (e.g. `PinToggle`'s label states both the action *and* the
  current state — "Unpin (currently pinned)" — specifically so a
  screen-reader user isn't relying on icon fill color, per that
  component's own comment at `PinToggle.tsx:107-109`). Purely decorative
  SVGs (`components/InfoIcon.tsx:22`) and decorative badge overlays
  (`ThemeToggle.tsx`'s "A" indicator, `attributes: { indicator: {
  'aria-hidden': 'true' } }`) are correctly `aria-hidden`.
- **No raw HTML form elements anywhere.** `grep -rn "<input\|<select\|<textarea\|<button"` across `app/` and
  `components/` returns nothing — every form control goes through
  Mantine's wrapped components (`TextInput`, `Select`, etc.), which
  auto-generate paired `label`/`htmlFor`/`id`. This is the likely reason
  `jsx-a11y/label-has-associated-control` never had anything to flag.
- **`Dropzone` (file upload, `TicketEntryForm.tsx:418-441`) is keyboard-
  operable by default.** Confirmed by reading the installed
  `@mantine/dropzone@9.5.2` source: `activateOnKeyboard` defaults to
  `true` and is never overridden here, so `noKeyboard` (passed to the
  underlying `react-dropzone`) is `false` — Enter/Space on a focused
  dropzone opens the file picker.
- **`LoginPromptModal` focus management verified live, not just by
  reading Mantine's docs.** Triggered it anonymously from
  `/stations/PAD`'s pin button; `document.activeElement` after open was
  the modal's own close button (Mantine `Modal`'s default `trapFocus`
  moving focus in), and axe reported zero violations with the modal open.
  Its explicit `closeButtonProps={{ 'aria-label': 'Close' }}`
  (`LoginPromptModal.tsx:57`) is a deliberate, already-documented fix for
  a real gap in Mantine's own `CloseButton` (no default accessible name —
  `LoginPromptModal.tsx:42-46`'s comment records this was confirmed
  against the installed library source, not assumed).
- **Color is never the only channel for severity.** Every `StatusBadge`
  carries a text label (`severityLabel`) alongside its color; every
  `IssueList` row additionally shows an `impactType` text badge ("Rail
  Replacement Bus", "Diversion") where extracted. The color-blindness risk
  from the grape/red palette collision was explicitly considered and
  accepted at design time for exactly this reason
  (`docs/superpowers/specs/2026-08-18-grape-theme-design.md:176-183`).

## Recommendations (prioritized)

1. **Add `component="main"` to the content `Container` in
   `app/layout.tsx:184`** (or wrap `{children}` in a plain `<main>`). One
   line, zero visual change, fixes `landmark-one-main` and the
   overwhelming majority of `region` violations on every page in the app
   simultaneously — the single highest-leverage fix in this whole audit.
2. **Fix the `StatusBadge` yellow/green (and, more mildly, red) filled
   contrast**, since these are the most-repeated colored element in the
   product and the worst-measured ratios found (1.86:1, 2.36:1). The
   `variantColorResolver`-override approach already scoped out for grape
   in `docs/superpowers/specs/2026-08-18-grape-theme-design.md:156-162`
   is the right shape for this too — a scoped override (or `autoContrast`,
   which that same doc notes would flip yellow badges to black text and
   was flagged as "an improvement the original review actually asked
   for") rather than a global `primaryShade` bump that would ripple into
   unrelated colors. Add ratio assertions for these shades to
   `app/globals.test.ts` alongside the existing grape/anchor ones, so this
   doesn't silently regress again.
3. **Fix the `Badge` at `app/incidents/[id]/page.tsx:58`** (2.57:1) by
   giving it an explicit `variant="light"` or `variant="outline"` (the
   pattern `DisruptionDetail.tsx` already uses for its own badges) instead
   of relying on the implicit `filled` default.
4. **Give `TrendsCharts` a caller-supplied heading `order` instead of a
   hardcoded 4**, and make the Timeline tab's per-day headers `order={2}`
   (dropping `size="h5"` to keep the same visual size while fixing the
   semantic level). Both call sites already pass a `size` prop
   independent of `order`, so this is a plumbing change, not a redesign —
   thread an `order` prop through `TrendsCharts` from each of its two
   callers.
5. **Change every `not-found.tsx`'s and `error.tsx`'s top heading from
   `order={2}` to `order={1}`.** Six files, one-character diffs each; no
   visual impact if `size` is set independently.
6. Ship the already-designed, already-deferred grape-6 button fix (scoped
   `variantColorResolver` override to grape 7) — lower priority than #2
   only because it's a smaller, single-known gap (4.02 vs. 4.5) versus the
   badges' much larger gaps, and because it was already scoped and just
   needs implementing.
7. Install `eslint-plugin-jsx-a11y` for real (via `eslint-config-next`,
   which is the standard, lowest-effort route for a Next.js app and pulls
   in the rest of Next's recommended lint rules too) and wire it into CI,
   even though it found nothing this pass — it's cheap regression
   insurance against exactly the kind of "no raw `<input>`,
   properly-labeled `ActionIcon`s" discipline this codebase already has,
   which a future contributor unfamiliar with that discipline could
   easily break without a linter catching it.

Recommendations 1–5 are all small, mechanical, low-risk diffs (a Container
prop, a few `Title`/`Badge` prop changes) that collectively resolve every
distinct issue *type* found — prioritized above by how many pages/how much
of the DOM each one touches, not by how hard each is to fix (none of them
are hard).

## Explicitly out of scope

- **No code was changed.** This is an audit; implementing any of the
  Recommendations above is separate, future work.
- **`/connect-claude` reproducibly returns HTTP 500** (minified React
  error #130 — an invalid/undefined element type — surfaced through
  `app/error.tsx`'s boundary), reproduced twice cleanly. This is a
  **functional bug, not an accessibility defect**, and is out of scope to
  diagnose or fix here — noted only because it blocked testing the real
  page's accessibility (only the generic error-boundary fallback could be
  scanned, and that fallback's own a11y gaps are already covered under
  "No page-level `<h1>`" above). Flagged as an open risk below.
- **WCAG conformance beyond what `jsx-a11y`'s rulesets and axe-core's
  default ruleset check.** Neither tool is a substitute for a full manual
  WCAG 2.2 AA audit (e.g., full keyboard-trap testing across every
  interactive widget, screen-reader-software testing with a real AT like
  NVDA/VoiceOver, reduced-motion/reflow/text-spacing testing beyond the
  `prefers-reduced-motion` handling already spot-checked for `PrideToggle`
  sparkles). This audit is deliberately scoped to what these two
  categories of tooling actually surface.
- **`TrackedTrainStatusBadge`'s contrast** (home page's per-train delay
  badges, `app/page.tsx:322-350`, `variant="light"`) was not dynamically
  measured — the disposable test account has no tracked trains, so this
  component never rendered during this session. Flagged, not measured;
  see Open questions.
- **Dark mode** was not separately audited. All dynamic testing ran under
  the default light appearance; `docs/superpowers/specs/2026-08-18-grape-theme-design.md:185-189`
  already flags dark-mode grape contrast as a separate, previously-open
  question this document doesn't re-litigate.
- **Any redesign of the severity color palette itself** (e.g. revisiting
  whether `planned` should stay blue, per that same design doc's own
  still-open question at lines 228-236) — out of scope; this audit only
  measures contrast against the *existing* palette choices.

## Open questions / risks

1. **`/connect-claude`'s HTTP 500 needs its own investigation.** It
   reproduced identically twice in this session (not a one-off glitch)
   and prevented any real accessibility testing of that route's actual
   content — only its generic error fallback was auditable. Whoever picks
   this up should check first whether it's a genuine regression or
   environment-specific to this deployment.
2. **The shared-browser-session race with the concurrent screenshot
   agent** means a small number of results above were opportunistic
   (captured because a race happened to land on a useful URL, e.g. the
   live 404 and `/track` in both auth states) rather than deliberately
   sequenced. Every dynamic finding cited above was verified against its
   own `location.href` at capture time, so none of the *findings*
   themselves are suspect — but the *route coverage* wasn't as evenly
   deliberate as a dedicated, isolated session would have produced.
3. **`TrackedTrainStatusBadge` and any other component gated behind real
   user data this test account doesn't have** (a populated ticket list,
   an actual tracked train mid-journey, an upcoming-vs-active incident
   filter with real "Upcoming" results) went untested dynamically. Static
   review didn't flag anything Mantine-pattern-wise for these, but they
   weren't run through axe.
4. **Whether `variant="light"`/`variant="outline"` Badge/Button contrast
   pairings elsewhere in the app (outside the six `filled` pairings
   measured above) also fail** wasn't exhaustively checked — axe only
   flags what actually renders on a scanned page, and several
   `variant="light"`/`"outline"` badges (e.g. `DisruptionDetail.tsx`'s
   `affectedStops` outline badges, the `impactType` orange-light badge)
   happened to be present on scanned pages and did *not* get flagged, but
   that's not a guarantee every light/outline pairing everywhere clears
   AA — light/outline variants are generally lower-risk than filled
   (colored text on a near-white background, not white-on-saturated), but
   this wasn't independently re-derived from first principles.

## References

- `frontend/package.json`, `frontend/package-lock.json` — confirmed no
  `eslint`/`eslint-config-next`/`eslint-plugin-jsx-a11y` present anywhere.
- `frontend/app/layout.tsx:145` (nav landmark, correct), `:184` (missing
  main landmark — root cause), `:119-192` (full layout).
- `frontend/components/OpenDataAttribution.tsx:77` (footer landmark,
  correct).
- `frontend/lib/severity.ts:5-40` (`SEVERITY_TABLE`/`GROUP_COLOR`).
- `frontend/components/StatusBadge.tsx:9-15`.
- `frontend/app/incidents/[id]/page.tsx:58`.
- `frontend/lib/theme.ts:1-27` (grape primary color, documented 4.02:1
  gap and rejected-fix reasoning).
- `frontend/app/globals.test.ts:1-56` (only contrast pairing under test
  today).
- `docs/superpowers/specs/2026-08-18-grape-theme-design.md:126-236`
  (primary-shade history, explicitly-flagged-but-unmeasured contrast
  concerns, colorblindness risk accepted by design, open question on the
  `planned` severity color).
- `frontend/app/lines/[id]/page.tsx:106-193` (heading structure,
  `Title order={2} size="h4"` at :162).
- `frontend/app/lines/[id]/history/page.tsx:69-146` (heading structure,
  `<h1>` at :74, per-day `<h3>` at :167).
- `frontend/app/lines/[id]/history/TrendsCharts.tsx:77-135` (hardcoded
  `<h4>` at :86, :124).
- `frontend/app/lines/[id]/not-found.tsx:7`,
  `frontend/app/incidents/[id]/not-found.tsx:7`,
  `frontend/app/stations/[crs]/not-found.tsx:7`,
  `frontend/app/train/by-id/[trackingId]/not-found.tsx:7`,
  `frontend/app/train/[uid]/[date]/not-found.tsx:7`,
  `frontend/app/error.tsx:14` (all `Title order={2}`, no page `<h1>`).
- `frontend/components/PinToggle.tsx:95-130`,
  `frontend/components/ThemeToggle.tsx:1-67`,
  `frontend/components/PrideToggle.tsx:100-160`,
  `frontend/components/LineDefinitionTooltip.tsx:1-40`,
  `frontend/components/DataFreshnessInfo.tsx:1-55`,
  `frontend/components/InfoIcon.tsx:1-30` (icon-button/decorative-icon
  labeling, all correct).
- `frontend/components/LoginPromptModal.tsx:1-67` (focus/close-button
  fix, verified live).
- `frontend/components/TicketEntryForm.tsx:401-457` (`Dropzone` usage).
- `node_modules/@mantine/core/cjs/components/Title/Title.cjs:35-53`
  (`order` prop controls the rendered heading tag).
- `node_modules/@mantine/core/cjs/components/Badge/Badge.cjs:12-26`
  (`Badge` default `variant: "filled"`).
- `node_modules/@mantine/core/cjs/components/Accordion/Accordion.cjs`,
  `.../AccordionControl/AccordionControl.cjs` (Accordion only wraps its
  control in a heading when an explicit `order` prop is passed; this
  app's `IssueList.tsx` Accordion usage passes none, so it contributes no
  heading-order risk of its own).
- `node_modules/@mantine/dropzone/cjs/Dropzone.cjs:16,42,72`
  (`activateOnKeyboard` default `true`).
- Live axe-core 4.10.2 runs (this session, `browser_evaluate` injection)
  against: `/`, `/lines`, `/lines/new`, `/lines/gwr-main-line`,
  `/lines/gwr-main-line/history` (Timeline + Trends tabs),
  `/incidents/E879EB6C791C470AB6C2A7458AE68C3B`, `/stations`,
  `/stations/PAD` (incl. anonymous + `LoginPromptModal` open),
  `/track/mine`, `/track/mine/add-ticket`, `/chat`, `/connect-claude`
  (error state), plus incidental coverage of
  `/lines/custom-ds-test-custom-line`,
  `/lines/custom-ds-test-custom-line/edit`, `/track` (both auth states),
  and a live 404 at `/lines/nonexistent-line-slug`.
