# Frontend Accessibility Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1–3 are trivial, independent and safe to land in any order.**
> **Tasks 4–5 change colour on nearly every screen in the app** and are
> gated behind Task 6's visual pass before they should be considered done.
> Do not fold 4/5 into 1–3's commits — a heading-level diff that has to be
> reverted alongside a palette diff is the failure mode this split exists
> to avoid.

**Goal:** implement fixes for every real finding in
`docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md`
— 0 critical, 6 serious (all `color-contrast`), 5 moderate (missing
`<main>` landmark sitewide, two `heading-order` root causes, missing
page-level `<h1>` on 6 error/not-found templates), 0 minor, 0 static
`eslint-plugin-jsx-a11y` violations. Plus the two contrast pairings that
audit provably could not see (see "Findings the audit could not reach"
below), because fixing four of the six severity-badge colours and leaving
two known-failing siblings behind is not a fix.

**Architecture:** no new components, no new data flow, no backend changes.
Four of the six serious findings collapse into a **single theme-level
change** in `frontend/lib/theme.ts` once the mechanism is chosen correctly
(see Design), so this plan is: one `lib/theme.ts` edit, three scoped CSS
custom-property overrides in `frontend/app/globals.css` (extending the
block that already lives there for `--mantine-color-anchor`), one prop
added to one shared chart component, and ten one-line `Title`/`Container`
prop edits across `app/`.

**Tech Stack:** Next.js 16 App Router + TypeScript + Mantine v9.5.2
(`@mantine/core`, `@mantine/charts`), Vitest 2 + `@testing-library/react`
via this repo's `renderWithMantine` helper (`frontend/test/render.tsx`),
Playwright 1.62 (`frontend/e2e/`, `testDir: './e2e'`, driven against a real
deployment via `E2E_BASE_URL`).

**Specs:**
- `docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md`
  — the audit this plan implements. Its Recommendations section is
  authoritative for *what* to fix; this plan departs from it on *how* in
  two places (Decisions 3 and 5 below), each with the reason recorded.
- `docs/superpowers/specs/2026-08-18-grape-theme-design.md` — the
  half-finished thread Task 4 completes. Its "Primary shade" section
  (lines 124–162) already measured white-on-grape-6 at 4.02:1, already
  rejected `primaryShade: 7` on evidence, already shipped the grape-7
  answer for links only, and already flagged "dimmed grey body text,
  white-on-amber badges" as unmeasured concerns needing "an automated pass
  rather than trust eyeballs" (lines 185–189). The audit *is* that pass.
  Task 4 and Task 5 close both halves it left open.

---

## Measured facts (ground truth for this plan — do not re-derive)

All ratios below are WCAG 2.1 relative-luminance contrast, computed with
the same formula `frontend/app/globals.test.ts:18-27` already encodes,
against Mantine 9.5.2's shipped Open Color palette (shade-6 hexes verified
against the audit's own live axe measurements — they agree to two
decimals, so the palette in this Mantine version is unchanged from the
classic values).

### The six findings the audit measured

| # | Pairing | Where | Measured | Threshold | Root cause |
|---|---|---|---|---|---|
| S1 | white on yellow-6 `#fab005` | `components/StatusBadge.tsx:11`, `GROUP_COLOR.mild` | **1.86:1** | 4.5 | `variant="filled"` + white text |
| S2 | white on green-6 `#40c057` | same, `GROUP_COLOR.good` | **2.36:1** | 4.5 | same |
| S3 | white on orange-6 `#fd7e14` | `app/incidents/[id]/page.tsx:58` (no `variant` → Mantine's `filled` default) | **2.57:1** | 4.5 | same |
| S4 | white on red-6 `#fa5252` | `StatusBadge.tsx:11`, `GROUP_COLOR.severe` | **3.28:1** | 4.5 | same |
| S5 | gray-6 `#868e96` on white | every `<Text c="dimmed">` — **72 call sites** (`grep -rn 'c="dimmed"' app components --include=*.tsx \| wc -l`) | **3.32:1** | 4.5 | `--mantine-color-dimmed` = gray-6 in the light scheme |
| S6 | white on grape-6 `#be4bdb` | `lib/theme.ts:27` `primaryColor: 'grape'` — filled `Button`s, active `Chip`s, date-preset buttons | **4.02:1** | 4.5 | primary filled shade |

**All six need 4.5:1, not 3:1.** WCAG's 3:1 large-text allowance needs
≥18.66px bold or ≥24px; Mantine's `Badge` text is ~11px bold and `Button`
`sm` is 14px, and `Text c="dimmed"` call sites here are `md`/`sm`/`xs`
(≤16px). The audit reaches the same conclusion ("axe itself confirms need
4.5:1"). WCAG 1.4.11's separate 3:1 *non-text* rule (badge fill vs page
background) is **not** what any of these six are — it was not measured by
the audit and is out of scope here.

### Findings the audit could not reach (same root causes, same fix)

The audit's own Open Question 3 flags that components gated behind data
the disposable test account didn't have "went untested dynamically."
Computing the same formula over the rest of `GROUP_COLOR` and the rest of
the app's filled surfaces shows four more instances of the *same three*
root causes:

| Pairing | Where | Ratio | Why axe never saw it |
|---|---|---|---|
| white on blue-6 `#228be6` | `StatusBadge`, `GROUP_COLOR.planned` | **3.56:1** | no line was in a Planned Closure / Part Closure state during the audit |
| white on gray-6 `#868e96` | `StatusBadge`, `GROUP_COLOR.informational` | **3.32:1** | no line was in an informational state (Special Service / Exit Only / …) |
| white on blue-6 `#228be6` | `app/incidents/[id]/page.tsx:58`, the `isPlanned` branch of the same `Badge` | **3.56:1** | the scanned incident was `!isPlanned`, so only the orange branch rendered |
| white on red-6 `#fa5252` | `color="red"` filled `Button`s in `DeleteLineButton.tsx:78`, `DeleteTicketButton.tsx:93`, `DeleteTrainButton.tsx:88` | **3.28:1** | those buttons only exist inside an opened confirm `Modal` |
| white on grape-6 `#be4bdb` | `app/lines/CustomLineForm.tsx:194` — a `Badge` with neither `color` nor `variant`, so primary + filled | **4.02:1** | same pairing as S6, different component |

These are **not scope creep**: every one is the identical white-on-shade-6
pairing already being fixed for its siblings, in the same component or the
same theme knob. Fixing S1–S4 while leaving blue and gray severity badges
at 3.56/3.32 would ship a `StatusBadge` that is AA-compliant for three of
its five semantic states.

### The autoContrast threshold derivation (the load-bearing number)

Mantine's `autoContrast` picks black or white text per surface by
comparing the background's relative luminance `L` against
`theme.luminanceThreshold` (default `0.3`). Solving WCAG's 4.5:1 for each
branch:

- black text clears 4.5:1 ⟺ `(L + 0.05) / 0.05 ≥ 4.5` ⟺ **`L ≥ 0.1750`**
- white text clears 4.5:1 ⟺ `1.05 / (L + 0.05) ≥ 4.5` ⟺ **`L ≤ 0.1833`**

The two conditions overlap, so **any threshold in `[0.1750, 0.1833]`
makes `autoContrast` provably AA-correct for *every* background colour,
present or future** — there is no colour it can get wrong. Mantine's
default `0.3` sits far above that window, which is precisely why yellow
and green flip correctly today but red, blue and gray do not.

`luminanceThreshold: 0.179` is this plan's choice: the balance point where
both branches floor at ≈4.58:1 (black-branch floor `(0.179+0.05)/0.05 =
4.580`; white-branch floor `1.05/(0.179+0.05) = 4.585`), i.e. maximum
margin on both sides simultaneously. Verified exhaustively across all
eight palette colours this app uses (gray, red, green, blue, yellow,
orange, grape, teal) at shades 6, 7 and 8 — **24 of 24 pass**, minimum
4.65:1 (dark-scheme red-8, black text).

Resulting ratios at `luminanceThreshold: 0.179`, light scheme (filled =
shade 6):

| Colour | L | autoContrast picks | Ratio | was |
|---|---|---|---|---|
| yellow-6 `#fab005` | 0.5139 | black | **11.28:1** | 1.86 |
| green-6 `#40c057` | 0.3948 | black | **8.90:1** | 2.36 |
| orange-6 `#fd7e14` | 0.3585 | black | **8.17:1** | 2.57 |
| red-6 `#fa5252` | 0.2697 | black | **6.39:1** | 3.28 |
| gray-6 `#868e96` | 0.2662 | black | **6.32:1** | 3.32 |
| blue-6 `#228be6` | 0.2452 | black | **5.90:1** | 3.56 |
| grape-6 `#be4bdb` | 0.2109 | black | 5.22:1 | 4.02 |
| grape-7 `#ae3ec9` | 0.1666 | **white** | **4.85:1** | — |

Dark scheme (filled = shade 8), unchanged by this plan except through the
same knob: yellow-8 black 8.46, green-8 black 6.10, orange-8 black 5.87,
red-8 black **4.65** (the tightest number in this plan — see Risks),
blue-8 white 5.02, gray-8 white 11.51, grape-8 white 5.82.

### Dimmed text

- Light: `--mantine-color-dimmed` resolves to gray-6 `#868e96` = 3.32:1 on
  white. gray-7 `#495057` = **8.18:1**. There is no intermediate Mantine
  gray; gray-7 is the only shade on the scale that clears AA, and the
  grape spec's own Non-goals forbid hand-mixed shades.
- Dark: `--mantine-color-dimmed` resolves to dark-2 `#a6a7ab`, which is
  **6.46:1** on the dark body `#242424` — already passing. Leave it alone,
  exactly as the existing `--mantine-color-anchor` override left the dark
  half alone (`app/globals.css:25-26`, and the assertion at
  `app/globals.test.ts:50` that no `data-mantine-color-scheme='dark'`
  selector exists in that file).

---

## Design

### Decision 1 — `<main>` is genuinely a one-line change, but verify Container's polymorphism first

The audit's top recommendation is `component="main"` on the content
`Container` at `app/layout.tsx:184` (**re-verified during planning: line
184 is still `<Container size="lg" px={0}>`, immediately followed by
`{children}` at :185**). Mantine's polymorphic `component` prop swaps only
the rendered tag; `Container`'s own `size`/`px` handling, its
`--container-size` CSS variable and its `mantine-Container-root` class all
come from its `useStyles`/`Box` layer and are tag-independent, so this
should be a zero-pixel diff.

**"Should be" is not "is", and this repo's own convention is to confirm
against the installed library rather than the docs** (the audit itself did
this five separate times; `LoginPromptModal.tsx:42-46` records the same
habit). So Task 1 Step 1 reads the installed `Container` source and
confirms it is built on `Box`/`polymorphicFactory` before the prop is
added, and Task 1 Step 4 diffs a screenshot. If `Container` turns out not
to be polymorphic, the fallback is the audit's own parenthetical: wrap
`{children}` in a plain `<main>` inside the `Container`.

One consequence worth stating: `app/error.tsx` and every `not-found.tsx`
render *inside* this `Container`, so they gain the `<main>` landmark for
free — which is why Task 3 only has to fix their heading level, not their
landmarking.

### Decision 2 — `autoContrast` + a derived `luminanceThreshold`, not per-colour shade surgery

Three mechanisms were on the table for the badge/button contrast failures.

**(a) Darken each failing colour's filled background, keep white text.**
Rejected. It would need yellow-6→yellow-9, green-6→green-9,
red-6→red-8/9, blue-6→blue-8 — i.e. rewriting `GROUP_COLOR` in
`lib/severity.ts` with shade suffixes. That directly violates the grape
spec's standing Non-goal ("No change to `lib/severity.ts`'s `GROUP_COLOR`
map… green/gray/blue/yellow/red carry meaning users already read at a
glance"), and a yellow-9 `#e67700` badge does not read as yellow — it
reads as brown. It also fixes only the colours enumerated today.

**(b) Global `primaryShade: 7`.** Already rejected on evidence in
`lib/theme.ts:10-26` and `2026-08-18-grape-theme-design.md:130-142`:
`getPrimaryShade()` feeds *every* colour's `-filled` variable, not just
the primary's. That objection still holds and is independently confirmed
by the numbers here — at shade 7, white text on red-7 is 3.84:1 and on
blue-7 is 4.20:1, so a `primaryShade` bump does not even fix the problem
it would be breaking the palette to fix.

**(c) `autoContrast: true` with `luminanceThreshold: 0.179`. Chosen.**
One theme knob; provably AA for every background colour (see the
derivation above, not a tuned magic number); keeps every hue in
`GROUP_COLOR` byte-identical, satisfying the grape spec's Non-goal in
letter and spirit — a yellow badge stays yellow, it just gains black text.
That flip is not a side effect to apologise for: the grape design doc
already names it as *"an improvement the original review actually asked
for"* (line 160). It also covers the four pairings the audit could not
reach and any colour added later.

The grape spec parked `autoContrast` because it "would also flip
`StatusBadge`'s text colour… [which] needs a deliberate decision against
the Non-goals" (lines 156–162). **This plan is that deliberate decision,
and the audit is the evidence it was waiting for:** white text on the
severity palette fails AA for *all five* groups, not just the amber one
the review guessed at. The Non-goal protects the *hues*, and the hues do
not change.

### Decision 3 — leave `app/incidents/[id]/page.tsx:58` as a filled `Badge` (departs from audit recommendation #3)

The audit recommends giving that badge `variant="light"` or
`variant="outline"`, matching `DisruptionDetail.tsx:16`. **Not doing
that.** Under Decision 2 the badge is already fixed in place at 8.17:1
(orange) / 5.90:1 (blue) with no call-site change at all, and the audit's
own Open Question 4 says the contrast of this app's `light`/`outline`
pairings "wasn't exhaustively checked… that's not a guarantee every
light/outline pairing everywhere clears AA." Swapping a now-measured-
passing variant for an unmeasured one is a downgrade in certainty for a
purely stylistic gain. Task 6 Step 5 measures the `light`/`outline`
pairings so that open question gets closed on evidence rather than traded
against.

### Decision 4 — grape filled surfaces go to grape-7 with pinned white text, completing the recorded thread

`lib/theme.ts:22-24` already names the fix: *"revisit this as a scoped
`variantColorResolver` override (grape-only) rather than a global
`primaryShade` bump."* `2026-08-18-grape-theme-design.md:144-154` already
shipped the same answer's other half — `--mantine-color-anchor:
var(--mantine-color-grape-7)` scoped to the light scheme in
`app/globals.css:25-26`, measured at 4.85:1, asserted in
`app/globals.test.ts:40-51`. This plan finishes it: **filled grape gets
the same grape-7 the links already got**, so the light scheme has one
grape rather than two.

Mechanically this needs *both* layers, and the reason is a real footgun
worth writing down:

- **CSS layer** (`globals.css`, inside the existing light-scheme block):
  redirect `--mantine-color-grape-filled` → grape-7 and
  `--mantine-color-grape-filled-hover` → grape-8. Same one-line, same
  scheme-scoped, same-selector pattern as the anchor override sitting two
  lines above it.
- **JS layer** (`lib/theme.ts`): a `variantColorResolver` that wraps
  `defaultVariantColorsResolver` and, for `color === 'grape' && variant
  === 'filled'` only, pins `color` to `var(--mantine-color-white)`.

**Why both:** `autoContrast` is computed in JS from
`theme.colors.grape[primaryShade]` — it cannot see a CSS-variable
substitution. Left alone it would evaluate grape-**6** (L 0.2109 > 0.179),
choose **black**, and paint it on a grape-**7** background = 4.29:1, which
*fails*. The resolver is what keeps the two layers from disagreeing. The
same reasoning applies to the theme-level `--mantine-primary-color-contrast`
variable, which `autoContrast` also drives — Task 4 Step 4 overrides it to
white in the same light-scheme block and Task 6 Step 4 checks for any
surface that still picks up black.

**Recorded fallback, already measured:** if that three-part coupling
proves awkward in practice, dropping the grape override entirely also
clears AA — `autoContrast` alone gives black-on-grape-6 at **5.22:1**.
It is rejected as the primary answer only because it puts black labels on
the brand's primary buttons and leaves the light scheme with grape-6
buttons beside grape-7 links, i.e. it answers the grape question
*differently* from the answer already on record rather than completing it.
Do not reach for it without recording why.

### Decision 5 — dimmed text is its own task, not part of the filled-surface task

The audit lists S5 (dimmed gray) alongside the five filled-surface
failures because axe groups them by rule, but they share no mechanism:
S1–S4 and S6 are all one `autoContrast` decision about *text on a
saturated fill*, while S5 is `--mantine-color-dimmed` resolving to a
too-light *foreground on the page background*. `autoContrast` does not
touch it.

They are also very differently shaped risks. The filled-surface change
touches ~8 rendered elements' text colour; the dimmed change touches **72
call sites** across virtually every page, and gray-6→gray-7 is a large
enough jump (3.32 → 8.18) that "dimmed" text becomes noticeably darker.
Hence Task 4 and Task 5 are separate commits with separate revert
surfaces. (This is the badge-vs-button split the audit's shape suggests,
redrawn along the line the *code* actually divides on: the badge and
button fixes are literally the same theme edit and cannot be usefully
split from each other.)

### Decision 6 — `TrendsCharts` takes a required `order`, and `order` is not defaulted

`TrendsCharts` hardcodes `<Title order={4}>` at `:86` and `:124`. Its two
callers sit at different depths, so any default is wrong for one of them:

| Caller | Page | Enclosing heading | Correct `order` |
|---|---|---|---|
| `HalfHourlyTrendsResults.tsx:76` | `/lines/[id]` | `h1` line name (`page.tsx:110`) → `h2` "Recent trends (last 24 hours)" (`page.tsx:162`) | **3** |
| `TrendsResults.tsx:86` | `/lines/[id]/history`, Trends tab | `h1` "History: {name}" (`page.tsx:74`), nothing between | **2** |

So `order` is a **required** prop typed as Mantine's `TitleOrder`, with no
default — a future third caller must state its own depth rather than
silently inherit a wrong one. Both `Title`s keep `size="h6"`, so the
rendered font size does not change at either call site.

### Decision 7 — the whole-app heading hierarchy, verified end to end

The audit fixes two local skips; this plan confirms the result is
globally correct rather than locally patched. Verified during planning
that **no component under `frontend/components/` renders a heading at
all** (`grep -rn "<Title" app components` returns hits only under `app/`;
`grep -rn "<h[1-6]\b" app components` returns nothing; the audit
separately confirmed `IssueList`'s `Accordion` contributes no heading
because it passes no `order`). That makes the per-route hierarchy fully
determined by the `app/**/page.tsx` files:

| Route | Headings, in DOM order | After this plan |
|---|---|---|
| `/` (anonymous branch) | h1 "Distant Signal" → h2 "Right now" | ✅ unchanged |
| `/` (authenticated branch) | h1 "Your Lines" → h2 "Your Stations" → h2 "Your Tracked Trains" | ✅ unchanged |
| `/lines`, `/lines/new`, `/lines/[id]/edit`, `/stations`, `/stations/[crs]`, `/track`, `/track/mine/add-ticket`, `/train/**`, `/chat`, `/chat/callback`, `/connect-claude`, `/incidents/[id]` | single h1 | ✅ unchanged |
| `/track/mine` | h1 → h2 "Tickets not yet attached to a train" | ✅ unchanged |
| `/lines/[id]` | h1 → h2 "Recent trends" → **h4** ×2 | h1 → h2 → **h3** ×2 ✅ |
| `/lines/[id]/history` | h1 → **h3** per-day ×N → **h4** ×2 | h1 → **h2** per-day ×N → **h2** ×2 ✅ |
| 5 × `not-found.tsx`, `app/error.tsx` | **h2** only, no h1 | **h1** ✅ |

The two `h1`s on `app/page.tsx` (`:109` and `:202`) are in mutually
exclusive `session.authenticated` branches — verified, not a defect.

On the history page both `TabsPanel`s are in the DOM simultaneously
(Mantine's `Tabs` `keepMounted` defaults to `true`), so the two fixes have
to be correct *together*: Timeline's per-day headers and Trends' chart
headers both land at `h2` under the single `h1`, in that DOM order, with
no skip on either tab or across them.

**Out of scope, noted not fixed:** several `<Text fw={500}>` elements read
as section headings visually but are not headings semantically
(`app/incidents/[id]/page.tsx:74` "Validity", `app/lines/[id]/page.tsx:157`
"TfL also reports:"). axe does not flag these and the audit did not
either — promoting them is a semantics change with real visual
consequences, and belongs to its own piece of work.

### Decision 8 — `<Title order={1}>` needs `size="h2"` on the error/not-found templates

The audit says changing `order={2}` → `order={1}` on the six templates has
"no visual impact if `size` is set independently" — but on those six files
`size` is **not** set (verified: all six are a bare `<Title order={2}>`).
Mantine's `Title` uses `order` for both the tag *and* the default font
size (`size` only overrides the latter), so a bare flip would visibly
enlarge every 404 heading. Task 3 writes `<Title order={1} size="h2">`,
keeping the rendered size byte-identical while fixing the tag. The
history page's per-day headers already pass `size="h5"` independently, so
they need no such addition.

---

## Global Constraints

- **No change to `lib/severity.ts`.** `GROUP_COLOR`'s five hue names are
  untouched by every task here — the grape spec's standing Non-goal. If a
  task appears to need a shade suffix (`'red.8'`) in that file, the
  mechanism has been chosen wrong; re-read Decision 2.
- **No hand-mixed hexes.** Every colour value introduced by this plan is
  a `var(--mantine-color-*)` reference to a shipped Mantine shade. The
  existing assertion at `app/globals.test.ts:113` (`expect(rule[0]).not.toMatch(/#[0-9a-f]{3,8}/i)`)
  encodes this habit for the body wash; keep it true for the new rules.
- **Light scheme only for the CSS overrides.** `app/globals.test.ts:50`
  asserts `expect(css).not.toContain("data-mantine-color-scheme='dark'")`
  — that assertion must still pass after Tasks 4 and 5. Every dark-scheme
  pairing measured above already clears AA, so there is nothing to fix
  there.
- **Do not add `eslint-config-next` or any dependency in Tasks 1–6.**
  Task 7 is the only task allowed to touch `package.json`, and it is
  explicitly optional.
- **No backend changes.** Nothing in `crates/` is touched; `cargo test` is
  not part of this plan's verification.
- **Testing:** Vitest via `npm test` from `frontend/` (`"test": "vitest
  run"`, so `npm test` and `npx vitest run` are equivalent). Playwright
  via `npm run test:e2e`, which per `playwright.config.ts` needs either a
  local `npm run dev` or an `E2E_BASE_URL` pointing at a real deployment —
  `e2e/chat.spec.ts:5` records that this suite targets a real deployment
  and "does not stand up its own" backend.
- **File scope.** Modified: `frontend/app/layout.tsx`,
  `frontend/app/layout.test.tsx`, `frontend/app/lines/[id]/history/TrendsCharts.tsx`,
  `frontend/app/lines/[id]/history/TrendsResults.tsx`,
  `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx`,
  `frontend/app/lines/[id]/history/page.tsx`, `frontend/app/error.tsx`,
  the five `not-found.tsx` files, `frontend/lib/theme.ts`,
  `frontend/lib/theme.test.tsx`, `frontend/app/globals.css`,
  `frontend/app/globals.test.ts`, plus the colocated test files named per
  task. Created: `frontend/e2e/accessibility.spec.ts` (Task 6).

### Verification already done during planning (do not redo)

- `app/layout.tsx:184` is still `<Container size="lg" px={0}>`, with
  `{children}` at `:185` and `</Container>` at `:186`. The nav landmark
  (`:144-147`, `<Box component="nav">`) and the footer landmark
  (`components/OpenDataAttribution.tsx:75-79`, `component="footer"`) are
  both correct and out of scope.
- Exactly **one** explicit `variant="filled"` exists in the whole app
  (`StatusBadge.tsx:11`). The other filled surfaces are filled-by-default:
  `Badge` at `app/incidents/[id]/page.tsx:58` and
  `app/lines/CustomLineForm.tsx:194`, and 18 `<Button>`s with no `variant`
  (15 primary/grape, 3 `color="red"` delete confirms). Every `Alert` in
  the app either passes `variant="light"` or relies on `Alert`'s own
  non-filled default, and every other `Badge` passes `variant="light"` or
  `variant="outline"`. This is the complete blast radius of Decision 2.
- `severityColor()` is consumed by exactly one component
  (`StatusBadge.tsx:2`) plus its own tests — nothing else in the app maps
  severity to colour, so there is no second call site to keep in sync.
- No component under `frontend/components/` renders any heading; all
  `<Title>` call sites are under `frontend/app/`. Full per-route
  hierarchy table in Decision 7.
- Test precedent exists for both assertion styles this plan needs:
  `getByRole('heading', { name, level })` at `app/lines/new/page.test.tsx:23`,
  `app/track/mine/add-ticket/page.test.tsx:41`, `app/lines/page.test.tsx:60`;
  landmark-by-tag at `components/OpenDataAttribution.test.tsx:52`
  (`container.querySelector('footer')`).
- `~/.cache/ms-playwright/` currently holds `chromium-1234` and
  `chromium_headless_shell-1234` (the grape spec's Verification section
  refers to the older `chromium-1228` — update that path when following
  its screenshot recipe).
- `frontend/node_modules/` is **not installed** in this worktree. Every
  claim in this plan about Mantine internals is therefore derived from
  version-pinned behaviour and must be confirmed against the installed
  source before being relied on — see the checklist in Task 4 Step 1.

---

### Task 1: Add the `<main>` landmark

**Files:**
- Modify: `frontend/app/layout.tsx`, `frontend/app/layout.test.tsx`

Fixes the audit's #1 recommendation: `landmark-one-main` plus the
overwhelming majority of `region` violations, on every page in the app, in
one line. Independent of every other task.

- [ ] **Step 1: Confirm `Container` is polymorphic against the installed source**

```bash
cd frontend && npm ci
grep -rn "polymorphicFactory\|defaultComponent\|component" node_modules/@mantine/core/cjs/components/Container/Container.cjs
```

Expected: `Container` is created through Mantine's polymorphic/`Box`
factory and forwards an unknown `component` prop to the rendered element.
If it is **not** polymorphic, use the fallback from Decision 1 (wrap
`{children}` in a plain `<main>` inside the `Container`) and note the
deviation in the commit message.

- [ ] **Step 2: Add the prop**

`frontend/app/layout.tsx:184` — change:

```tsx
          <Container size="lg" px={0}>
```

to:

```tsx
          {/* `component="main"`: Mantine's Container renders a plain
              <div> by default, which left every page's actual content
              outside any landmark -- axe's `landmark-one-main` fired on
              every route tested, and `region` fired once per unlandmarked
              node (487 on /lines alone). See
              docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md.
              The nav (:144) and footer (OpenDataAttribution.tsx) were
              already landmarked; only the middle was not. Polymorphic
              `component` swaps the tag only -- size/px/class output is
              unchanged. */}
          <Container component="main" size="lg" px={0}>
```

Leave the existing `px={0}`/`size` comment block above the nav
`Container` (`:136-143`) untouched — it documents a different element.

- [ ] **Step 3: Add the regression test**

`RootLayout` itself renders `<html>`/`<body>` and is not usefully
renderable under `@testing-library/react`, so assert on the source the
same way `app/globals.test.ts` asserts on `globals.css` — a deliberate,
already-established pattern in this repo for things the DOM harness can't
reach. Append to `frontend/app/layout.test.tsx`:

```tsx
import { readFileSync } from 'node:fs';

describe('page content landmark', () => {
  // RootLayout renders <html>/<body>, which @testing-library/react can't
  // mount into a <div> container, so this asserts on the source rather
  // than the DOM -- the same tactic app/globals.test.ts uses for CSS
  // rules that only exist at the stylesheet level. The live-DOM check for
  // this lives in e2e/accessibility.spec.ts instead.
  it('renders page content inside a <main> landmark, not a bare Container div', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<Container\s+component="main"/);
  });
});
```

(Vitest runs with `frontend/` as its root — see `vitest.config.ts` and
`app/globals.test.ts:4-5` — so the relative path resolves.)

- [ ] **Step 4: Verify no visual change**

Run the app (`npm run dev`, or against a deployment) and diff a screenshot
of `/lines` and `/lines/[id]` before/after. Expected: byte-identical
layout; the only DOM difference is `div` → `main` on the content wrapper.

- [ ] **Step 5: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Give page content a <main> landmark (axe landmark-one-main, sitewide)"
```

---

### Task 2: Fix the two heading-order skips

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`,
  `frontend/app/lines/[id]/history/TrendsResults.tsx`,
  `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx`,
  `frontend/app/lines/[id]/history/page.tsx`, and the colocated test files
  `TrendsResults.test.tsx`, `HalfHourlyTrendsResults.test.tsx`,
  `app/lines/[id]/history/page.test.tsx`

Independent of Tasks 1, 3, 4 and 5.

- [ ] **Step 1: Give `TrendsCharts` a required `order` prop**

`TrendsCharts.tsx:77` — change the signature to take `order`, typed as
Mantine's `TitleOrder`, with **no default** (Decision 6):

```tsx
export function TrendsCharts({
  points,
  granularity,
  order,
}: {
  points: ChartPoint[];
  granularity: 'day' | 'halfHour';
  /** Heading level for the two chart titles. Required, deliberately
   * undefaulted: this component is mounted at two different depths
   * (h2 on /lines/[id]/history's Trends tab, h3 under "Recent trends" on
   * /lines/[id]), so any default would render a heading-order skip on one
   * of them -- which is exactly the axe `heading-order` defect this
   * replaced (both titles were a hardcoded `order={4}`). `size="h6"` stays
   * pinned at both call sites, so changing the level changes the tag only,
   * never the rendered font size. */
  order: TitleOrder;
}) {
```

Import `TitleOrder`: `import { Stack, Title, type TitleOrder } from '@mantine/core';`

Then at `:86` and `:124` replace `<Title order={4} size="h6">` with
`<Title order={order} size="h6">` (both occurrences; keep `size="h6"`).

- [ ] **Step 2: Pass the right level from each caller**

- `HalfHourlyTrendsResults.tsx:76` → `<TrendsCharts points={points} granularity="halfHour" order={3} />`
  (sits under `/lines/[id]`'s `h1` → `h2` "Recent trends (last 24 hours)").
- `TrendsResults.tsx:86` → `<TrendsCharts points={points} granularity="day" order={2} />`
  (sits directly under `/lines/[id]/history`'s only `h1`).

- [ ] **Step 3: Bump the Timeline tab's per-day headers to `h2`**

`frontend/app/lines/[id]/history/page.tsx:167` — change:

```tsx
          <Title order={3} size="h5">
```

to:

```tsx
          {/* order={2}, not 3: this page's only other heading is the
              `History: {name}` h1 at :74 -- there is no h2 between them,
              so an h3 here skipped a level (axe `heading-order`).
              `size="h5"` is unchanged, so this is a tag-only change with
              no visual effect. Both TabsPanels are mounted at once
              (Mantine Tabs keepMounted defaults to true), so this and the
              Trends tab's chart headings both have to land at h2 for the
              document to be skip-free either way the tabs are read. */}
          <Title order={2} size="h5">
```

- [ ] **Step 4: Add heading-level assertions**

Following `app/lines/new/page.test.tsx:23`'s existing
`getByRole('heading', { name, level })` pattern:

- In `TrendsResults.test.tsx`: assert the two chart titles render at
  level 2 — `expect(screen.getByRole('heading', { name: 'Delay / cancellation / skip rate', level: 2 })).toBeInTheDocument()`
  and the same for `'Average delay (minutes)'`.
- In `HalfHourlyTrendsResults.test.tsx`: the same two assertions at
  **level 3**.
- In `app/lines/[id]/history/page.test.tsx`: assert a per-day header
  renders at level 2 (use whatever date string that file's existing
  fixtures already produce — do not invent a new fixture).

Each assertion should carry a one-line comment naming the enclosing `h1`/
`h2` it is levelled against, so a future reader can tell the number is
derived, not arbitrary.

- [ ] **Step 5: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. `npm run build` matters here specifically — `order`
being required will fail type-checking at any call site that was missed.

- [ ] **Step 6: Commit**

```bash
git add "frontend/app/lines/[id]/history"
git commit -m "Fix heading-order skips on the line and line-history pages (axe heading-order)"
```

---

### Task 3: Give every error/not-found template a page-level `<h1>`

**Files:**
- Modify: `frontend/app/error.tsx:14`,
  `frontend/app/lines/[id]/not-found.tsx:7`,
  `frontend/app/incidents/[id]/not-found.tsx:7`,
  `frontend/app/stations/[crs]/not-found.tsx:7`,
  `frontend/app/train/by-id/[trackingId]/not-found.tsx:7`,
  `frontend/app/train/[uid]/[date]/not-found.tsx:7`

Independent of every other task.

- [ ] **Step 1: Change all six headings, preserving the rendered size**

In each of the six files, change the bare `<Title order={2}>` to
`<Title order={1} size="h2">` (Decision 8 — `size` is **not** currently
set on any of them, so a bare `order` flip would visibly enlarge the
heading). Text content is unchanged in all six.

Add this comment once, above the `app/error.tsx` heading, and a one-line
pointer to it in the five `not-found.tsx` files:

```tsx
      {/* order={1}, size="h2": this is the page's top-level heading, so
          it must be the h1 -- a 404/500 with no h1 at all fired axe's
          `page-has-heading-one` on every not-found template in the app
          (docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md).
          `size="h2"` keeps the rendered size exactly as it was; only the
          tag changes. These render inside the root layout's <main>
          Container, so they need no landmarking of their own. */}
```

- [ ] **Step 2: Add or update assertions**

For each of the six, add a `getByRole('heading', { name: '<text>', level: 1 })`
assertion to its colocated test file if one exists. If a given
`not-found.tsx` has **no** colocated test file today, do not create one —
instead assert it in that route's existing `page.test.tsx` if that file
already exercises the not-found branch, and otherwise leave it to Task 6's
live axe run. (Do not add five new near-empty test files for six one-line
diffs.)

- [ ] **Step 3: Grep for stragglers**

```bash
cd frontend && grep -rn "Title order={2}" app --include="not-found.tsx" --include="error.tsx"
```

Expected: zero matches.

- [ ] **Step 4: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/error.tsx frontend/app/**/not-found.tsx
git commit -m "Make error and not-found templates' top heading an h1 (axe page-has-heading-one)"
```

---

### Task 4: Fix filled-surface contrast — badges, buttons, chips

**Files:**
- Modify: `frontend/lib/theme.ts`, `frontend/lib/theme.test.tsx`,
  `frontend/app/globals.css`, `frontend/app/globals.test.ts`

This is the largest and riskiest change in the plan. It resolves S1, S2,
S3, S4 and S6, plus the five unmeasured pairings in "Findings the audit
could not reach", through one theme decision (Decision 2) and one scoped
grape override (Decision 4). **Do not start it until Tasks 1–3 are
landed** — keeping the palette diff isolated is the whole point of the
sequencing.

- [ ] **Step 1: Confirm the Mantine API surface against the installed source**

`node_modules/` is not installed in this worktree, so every assumption
below is unverified. Run `npm ci` from `frontend/` first, then confirm all
seven, and **stop and re-plan if any is false**:

```bash
cd frontend
grep -rn "autoContrast\|luminanceThreshold" node_modules/@mantine/core/cjs/core/MantineProvider/ | head -40
grep -rn "defaultVariantColorsResolver\|VariantColorsResolver" node_modules/@mantine/core/cjs/core/MantineProvider/ | head -20
grep -rn "mantine-color-dimmed" node_modules/@mantine/core/styles.css | head
grep -rn "primary-color-contrast" node_modules/@mantine/core/ --include=*.cjs --include=*.css | head
```

1. `createTheme` accepts `autoContrast`, `luminanceThreshold` and
   `variantColorResolver`.
2. `defaultVariantColorsResolver` is exported from `@mantine/core` under
   that exact name, and the `VariantColorsResolver` type with it.
3. `autoContrast` affects the `filled` variant's text colour (and which
   other variants — record the answer; if it also affects `light`, Task 6
   Step 5's measurements become load-bearing rather than informational).
4. The resolver input object exposes `color` and `variant` as plain
   strings for the branch in Step 3 to test.
5. `--mantine-color-dimmed` resolves to `var(--mantine-color-gray-6)` in
   the light scheme (this is Task 5's premise, confirm it here while the
   tree is installed).
6. `--mantine-color-grape-filled` / `--mantine-color-grape-filled-hover`
   are the variables the `filled` variant's background/hover actually
   read.
7. Which components consume `--mantine-primary-color-contrast` directly
   rather than going through the resolver — that set is what Step 4's
   override exists for and what Task 6 Step 4 has to eyeball.

- [ ] **Step 2: Set `autoContrast` and the derived threshold in `lib/theme.ts`**

Replace `lib/theme.ts:10-27`'s comment-and-export with the new theme.
**Keep the existing `primaryShade` reasoning paragraph** — it explains a
still-live rejected alternative (Decision 2b) and must not be deleted —
but update its trailing sentence, which currently points forward to this
work as future.

```ts
export const theme = createTheme({
  primaryColor: 'grape',

  // `autoContrast` picks black or white text per filled surface by
  // comparing the background's WCAG relative luminance against
  // `luminanceThreshold`. Mantine's default threshold (0.3) is too high to
  // be safe: it correctly flips yellow and green badges to black text but
  // leaves red (3.28:1), gray (3.32:1), blue (3.56:1) and orange (2.57:1
  // before the flip) on white text, all short of AA's 4.5:1.
  //
  // 0.179 is derived, not tuned. Solving 4.5:1 for each branch:
  //   black text clears AA  <=>  (L + 0.05)/0.05 >= 4.5  <=>  L >= 0.1750
  //   white text clears AA  <=>  1.05/(L + 0.05) >= 4.5  <=>  L <= 0.1833
  // Those windows overlap, so ANY threshold in [0.1750, 0.1833] makes
  // autoContrast provably AA-correct for every possible background --
  // there is no colour it can get wrong. 0.179 is the balance point where
  // both branches floor at ~4.58:1. Verified across all eight palette
  // colours this app uses at shades 6/7/8: 24 of 24 pass, minimum 4.65:1.
  // See app/globals.test.ts, which asserts this exhaustively.
  //
  // This is the "deliberate decision against the Non-goals" that
  // docs/superpowers/specs/2026-08-18-grape-theme-design.md:156-162 parked.
  // The evidence it was waiting for is
  // docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md:
  // white text fails AA on ALL FIVE of GROUP_COLOR's hues, not just the
  // amber one that review guessed at. lib/severity.ts is untouched -- a
  // yellow badge is still yellow, it just has readable text now.
  autoContrast: true,
  luminanceThreshold: 0.179,

  variantColorResolver,
});
```

- [ ] **Step 3: Add the grape-only resolver (above the `createTheme` call)**

```ts
import { createTheme, defaultVariantColorsResolver, type VariantColorsResolver } from '@mantine/core';

/** Pins filled grape surfaces to white text, overriding `autoContrast`
 * for this one colour.
 *
 * Necessary because the light scheme substitutes grape 7 for grape 6 as
 * the filled background at the CSS layer (`app/globals.css` --
 * `--mantine-color-grape-filled`, the same grape-7 the anchor colour was
 * moved to in the grape-theme spec, so the light scheme has one grape
 * rather than two). `autoContrast` runs in JS against
 * `theme.colors.grape[6]` and cannot see that substitution: unaided it
 * would evaluate grape 6 (L 0.2109, above the 0.179 threshold), choose
 * BLACK, and paint it on a grape-7 background for 4.29:1 -- a fail, and
 * the wrong colour. White on grape 7 is 4.85:1, and white on grape 8
 * (the dark scheme's filled shade, not overridden) is 5.82:1, so pinning
 * white is correct in both schemes.
 *
 * Scoped to grape + filled deliberately: every other colour must keep
 * going through `autoContrast`, which is what fixes the severity badges
 * and the red delete buttons. */
const variantColorResolver: VariantColorsResolver = (input) => {
  const resolved = defaultVariantColorsResolver(input);
  if (input.color === 'grape' && input.variant === 'filled') {
    return { ...resolved, color: 'var(--mantine-color-white)' };
  }
  return resolved;
};
```

- [ ] **Step 4: Redirect the grape filled variables in `app/globals.css`**

Extend the **existing** `html:root[data-mantine-color-scheme='light']`
block at `app/globals.css:25` (do not add a second block — the assertion
at `app/globals.test.ts:47` matches that exact selector, and the one at
`:50` forbids a dark-scheme selector):

```css
html:root[data-mantine-color-scheme='light'] {
  --mantine-color-anchor: var(--mantine-color-grape-7);
  /* Same grape 7 as the anchor above, for the same reason and with the
     same measurement: white on grape 6 is 4.02:1, white on grape 7 is
     4.85:1. The anchor half of this shipped with the grape theme; this is
     the filled half it left open (see lib/theme.ts's variantColorResolver
     comment, which explains why the JS side has to be pinned in step with
     this). Hover follows one shade further down, as Mantine's own filled
     hover does. Dark is untouched: grape 8 with white is already 5.82:1. */
  --mantine-color-grape-filled: var(--mantine-color-grape-7);
  --mantine-color-grape-filled-hover: var(--mantine-color-grape-8);
  /* `autoContrast` also drives this theme-level variable from grape 6's
     luminance and would set it to black; anything reading it directly
     rather than through the variantColorResolver would then paint black
     on the grape-7 fill above (4.29:1, fail). */
  --mantine-primary-color-contrast: var(--mantine-color-white);
}
```

- [ ] **Step 5: Add exhaustive contrast assertions to `app/globals.test.ts`**

That file already has the WCAG formula (`:18-27`) and the `AA_BODY_TEXT =
4.5` constant (`:29`). Add a new `describe` block that is **table-driven
over the whole palette**, not over the specific colours that failed —
the point is that the threshold is provably correct, so the test should
fail if a future palette or threshold change breaks *any* colour:

```ts
describe('filled-surface contrast under autoContrast', () => {
  // Mantine's shipped palette for the eight colours this app renders
  // filled, at the two shades `filled` resolves to (6 light, 8 dark).
  // ... PALETTE table ...

  // Mirrors Mantine's own autoContrast rule: black text when the
  // background's relative luminance exceeds theme.luminanceThreshold,
  // white otherwise. Kept as an independent reimplementation rather than
  // importing Mantine's -- the point is to check the THRESHOLD choice, and
  // a test that borrowed Mantine's implementation could only ever agree
  // with itself.
  function autoContrastText(bg: string, threshold: number): string {
    return luminance(bg) > threshold ? '#000000' : '#ffffff';
  }

  it.each(/* every colour x shade 6 and 8 */)(
    '%s-%i clears AA for body text with the autoContrast-chosen label colour',
    (name, shade, hex) => {
      expect(contrast(hex, autoContrastText(hex, theme.luminanceThreshold!)))
        .toBeGreaterThanOrEqual(AA_BODY_TEXT);
    },
  );

  it('keeps the threshold inside the window where BOTH branches clear AA', () => {
    // The real guarantee: any threshold in [0.1750, 0.1833] is AA-correct
    // for every possible background, so this asserts the derivation, not
    // just today's palette. See lib/theme.ts for the algebra.
    expect(theme.luminanceThreshold!).toBeGreaterThanOrEqual((4.5 * 0.05) - 0.05);
    expect(theme.luminanceThreshold!).toBeLessThanOrEqual((1.05 / 4.5) - 0.05);
  });

  it('pins filled grape to white text on the grape 7 the light scheme substitutes', () => {
    expect(contrast(GRAPE_7, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT);
    // The failure this guards: black on grape 7 is 4.33:1, which is what
    // autoContrast would choose unaided -- see lib/theme.ts's resolver.
    expect(contrast(GRAPE_7, '#000000')).toBeLessThan(AA_BODY_TEXT);
  });

  it('redirects the light scheme\'s grape filled background and hover to grape 7/8', () => {
    const rule = css.match(/html:root\[data-mantine-color-scheme=['"]light['"]\]\s*\{[^}]*\}/);
    expect(rule![0]).toContain('--mantine-color-grape-filled: var(--mantine-color-grape-7)');
    expect(rule![0]).toContain('--mantine-color-grape-filled-hover: var(--mantine-color-grape-8)');
    expect(rule![0]).toContain('--mantine-primary-color-contrast: var(--mantine-color-white)');
  });
});
```

Import `theme` from `@/lib/theme` so the threshold under test is the real
one, not a copy. **Also update the two existing link-colour tests'
comments** at `:32-38` — they currently read as "the shade the light
scheme was moved off"; that framing now covers filled surfaces too.

- [ ] **Step 6: Assert the theme knobs are actually threaded through, in `lib/theme.test.tsx`**

That file's existing posture ("colour is largely not unit-testable… what
IS worth locking down here is that the provider actually *receives* this
theme") is the right one. Add, in the same spirit:

```tsx
it('sets autoContrast and the AA-derived luminance threshold', () => {
  expect(theme.autoContrast).toBe(true);
  expect(theme.luminanceThreshold).toBe(0.179);
});

it('resolves filled grape to white text rather than autoContrast\'s black', () => {
  // Calls the resolver directly -- reading the rendered colour back out of
  // jsdom isn't reliable for Mantine's CSS variables (see this file's
  // existing note about :root-scoped custom properties).
  const resolved = theme.variantColorResolver!({ color: 'grape', variant: 'filled', theme: /* ... */ });
  expect(resolved.color).toBe('var(--mantine-color-white)');
});

it('leaves every other colour to autoContrast', () => {
  const resolved = theme.variantColorResolver!({ color: 'red', variant: 'filled', theme: /* ... */ });
  expect(resolved.color).not.toBe('var(--mantine-color-white)');
});
```

Build the resolver's `theme` argument from `MantineProvider`'s own
default-theme merge (`DEFAULT_THEME` merged with `theme`) rather than
hand-rolling one — if that turns out to be awkward to obtain, drop the
last two assertions and rely on Task 6's live axe run for that half,
rather than asserting against a fake theme that could diverge from the
real one.

- [ ] **Step 7: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. `app/globals.test.ts:50` (no dark-scheme selector)
and `:113` (no hardcoded hex in the wash rule) must both still pass.

- [ ] **Step 8: Commit — but do not consider this done until Task 6**

```bash
git add frontend/lib/theme.ts frontend/lib/theme.test.tsx frontend/app/globals.css frontend/app/globals.test.ts
git commit -m "Fix filled-surface contrast: autoContrast at an AA-derived threshold, grape 7 for filled brand surfaces"
```

---

### Task 5: Fix dimmed body-text contrast

**Files:**
- Modify: `frontend/app/globals.css`, `frontend/app/globals.test.ts`

Resolves S5. Separate from Task 4 because it shares no mechanism with it
and has a much wider blast radius — **72 `c="dimmed"` call sites** across
virtually every page (Decision 5).

- [ ] **Step 1: Redirect `--mantine-color-dimmed` for the light scheme**

Add to the same existing `html:root[data-mantine-color-scheme='light']`
block:

```css
  /* Mantine resolves `--mantine-color-dimmed` to gray 6 (#868e96) in the
     light scheme: 3.32:1 on white, short of AA's 4.5:1 for body text, and
     this app has 72 `c="dimmed"` call sites -- the single most widespread
     contrast defect the accessibility audit found
     (docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md).
     Gray 7 (#495057) is 8.18:1 and is the only shade on Mantine's gray
     scale that clears AA; a hand-mixed intermediate is ruled out by the
     grape spec's "no custom colour tuples" Non-goal. Dimmed text still
     reads as dimmed because body text here is black (21:1), so the
     hierarchy survives -- but it IS visibly darker than before; see this
     plan's Task 6. Dark is untouched: `--mantine-color-dimmed` resolves to
     dark 2 (#a6a7ab) there, already 6.46:1 on the #242424 body. */
  --mantine-color-dimmed: var(--mantine-color-gray-7);
```

- [ ] **Step 2: Assert it**

In `app/globals.test.ts`, extend the light-scheme-block test (or add a
sibling in the same style):

```ts
it('overrides --mantine-color-dimmed to gray 7 for the light scheme only', () => {
  const rule = css.match(/html:root\[data-mantine-color-scheme=['"]light['"]\]\s*\{[^}]*\}/);
  expect(rule![0]).toContain('--mantine-color-dimmed: var(--mantine-color-gray-7)');
});

it('confirms the dimmed shade the light scheme was moved off actually failed AA', () => {
  expect(contrast(GRAY_6, WHITE)).toBeLessThan(AA_BODY_TEXT);   // 3.32:1
  expect(contrast(GRAY_7, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 8.18:1
});

it('leaves the dark scheme\'s dimmed colour alone, where it already clears AA', () => {
  expect(contrast(DARK_2, DARK_7)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 6.46:1
});
```

Add `GRAY_6 = '#868e96'`, `GRAY_7 = '#495057'`, `DARK_2 = '#a6a7ab'` to
that file's existing hex constants block at `:9-13`, matching its
"only the shades these assertions actually name are listed" comment.

- [ ] **Step 3: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 4: Commit — again, not done until Task 6**

```bash
git add frontend/app/globals.css frontend/app/globals.test.ts
git commit -m "Fix dimmed body-text contrast: gray 7 in the light scheme (3.32:1 -> 8.18:1)"
```

---

### Task 6: Visual and live-axe verification of Tasks 4 and 5

**Files:**
- Create: `frontend/e2e/accessibility.spec.ts`
- Modify: `frontend/package.json` (dev dependency only — the one
  exception to the Global Constraint, see Step 6)

**Depends on:** Tasks 4 and 5.

**This is the real risk in this plan, and it is a bigger lift than every
other task combined.** Tasks 4 and 5 change text colour on the single
most-repeated UI element in the product (`StatusBadge` renders on `/`,
`/lines/[id]`, `/lines/[id]/history`, `/stations/[crs]`, inside every
`IssueList` row and every `LineStatusCard`) and on 72 dimmed-text call
sites. A contrast-ratio calculator saying "8.90:1" tells you nothing about
whether a black-on-green badge beside a black-on-red badge still reads as
a *severity scale*. That judgement can only be made by looking.

- [ ] **Step 1: Build a temporary all-states preview route**

Live data will not obligingly render all five severity groups at once —
which is exactly why the audit never measured blue or gray. Create a
throwaway route (the audit used the same tactic with its temporary
`app/__a11ytest__/` scratch file, then deleted it) at
`frontend/app/__a11y-preview__/page.tsx` rendering, in one view:

- `<StatusBadge severity={n} />` for one severity per group — `10` (good,
  green), `0` (informational, gray), `4` (planned, blue), `9` (mild,
  yellow), `6` (severe, red).
- The incident `Badge` in both branches: `<Badge color="blue">Planned Work</Badge>`
  and `<Badge color="orange">Real-Time</Badge>`.
- A default `<Button>` (grape filled), a `<Button color="red">` (the
  delete-confirm pairing), and a `<Chip defaultChecked>`.
- `<Text c="dimmed">` at `md`, `sm` and `xs`, beside an undimmed `<Text>`
  for the hierarchy comparison.

**This file must not be committed.** Delete it before Step 7.

- [ ] **Step 2: Screenshot before/after, both schemes**

Follow the recipe in `docs/superpowers/specs/2026-08-18-grape-theme-design.md:204-226`
— `playwright-core` against the bundled Chromium, whose path is now
`~/.cache/ms-playwright/chromium-1234/` (that doc says `1228`; update it
in passing). Capture at `git stash`-ed (before) and current (after) state:

- `/__a11y-preview__` — light and dark.
- The three routes that doc already nominates: `/lines/[id]` (worst-status
  badge beside the "View history" link and `IssueList`'s per-issue badges
  beside gray data-quality badges), `/lines/[id]/history` (per-entry
  badges beside "Back to line"), and `/` in both light and dark.
- Add `/lines/new` and `/track/mine/add-ticket` — the grape-filled
  submit button and active tab, i.e. S6's actual live surfaces.
- Add `/incidents/[id]` — S3's badge.

- [ ] **Step 3: Judge the four things a ratio cannot tell you**

Look at the after-shots and answer explicitly, in the commit message or a
short addendum to this file:

1. **Does the severity scale still read as a scale?** Every badge now has
   black text; the hue is the only remaining channel, as it was before.
   Confirm the green/yellow/red progression still scans at a glance in a
   dense `IssueList`.
2. **Black-on-red delete buttons** (`DeleteLineButton`/`DeleteTicketButton`/
   `DeleteTrainButton`) are the most unusual-looking consequence of
   Decision 2 — 6.39:1, up from 3.28:1, but visually unlike most design
   systems' destructive buttons. If rejected: the recorded, measured
   escape hatch is a second `variantColorResolver` branch pinning
   `red` + `filled` to white on `var(--mantine-color-red-9)` (`#c92a2a`,
   **5.46:1**), *not* raising `luminanceThreshold` — that would break the
   derived guarantee in Decision 2.
3. **Dark-scheme red-8 badges at 4.65:1** are the tightest pairing this
   plan ships. Passing, but with almost no margin; look at them
   specifically in the dark preview shot.
4. **Dimmed text at gray-7** is a large jump (3.32 → 8.18). Confirm the
   footer, the "Rates shown count each distinct train once per day…"
   honesty copy, and the `Category:`/`Operators:` lines on `/lines/[id]`
   still read as secondary rather than as body text.

- [ ] **Step 4: Hunt for surfaces that still pick up black-on-grape**

Per Decision 4, `--mantine-primary-color-contrast` is overridden in CSS
but `autoContrast` computes it from grape-6 in JS. Using the Step 1
preview page plus `/lines/new` and `/track/mine/add-ticket`, check every
grape-filled surface (`Button`, checked `Chip`, active `Tabs` tab, date
presets in `HistoryRangePicker`) for a black label. Expected: none — all
white. Any that are black are reading the variable through a path the
resolver doesn't cover; fix by extending the CSS override, and record
which component it was.

- [ ] **Step 5: Close the audit's Open Question 4 while the tooling is up**

Measure the `light`/`outline` badge pairings the audit explicitly did not
verify — `DisruptionDetail.tsx:16` (orange light), `IssueList.tsx:357`
(orange light) / `:362,:373` (gray outline), `EtaBadge.tsx:27` (teal
light), `app/page.tsx:331-344` and `app/track/mine/page.tsx:246-264`
(`TrackedTrainStatusBadge`, which the audit could not render at all
because its test account had no tracked trains), and
`TicketSummary.tsx:54`. Record the numbers. If any fail, they are **new
findings, not this plan's scope** — file them, don't fold them in.

- [ ] **Step 6: Commit a repeatable live axe check**

Add `@axe-core/playwright` as a devDependency and create
`frontend/e2e/accessibility.spec.ts`, asserting zero `color-contrast`,
`landmark-one-main`, `region`, `heading-order` and `page-has-heading-one`
violations across the anonymous-reachable routes (`/`, `/lines`,
`/lines/[a real line]`, `/lines/[id]/history` on both tabs, `/stations`,
`/stations/PAD`, `/incidents/[a real id]`, and a deliberate 404).

This reproduces the audit's own method (`axe.run` per page) as a committed
test instead of an ad-hoc `browser_evaluate` injection. Two honest limits
to write into the file's doc comment: it only sees whatever severities are
live at run time (which is why the deterministic palette assertions in
Task 4 Step 5 are the primary net and this is the secondary one), and per
`e2e/chat.spec.ts:5` this suite needs a real deployment via
`E2E_BASE_URL` — it does not stand up its own backend.

Run: `E2E_BASE_URL=<deployment> npm run test:e2e`

- [ ] **Step 7: Delete the preview route and commit**

```bash
rm -r frontend/app/__a11y-preview__
git status --short   # expected: no __a11y-preview__ entry
git add frontend/e2e/accessibility.spec.ts frontend/package.json frontend/package-lock.json
git commit -m "Add a committed axe-core e2e accessibility check"
```

---

### Task 7 (optional): Install `eslint-plugin-jsx-a11y` as regression insurance

**Files:**
- Modify: `frontend/package.json`, `frontend/package-lock.json`
- Create: `frontend/eslint.config.mjs`

**Explicitly optional and lowest priority.** This is audit recommendation
#7, and it is **not a finding** — the audit ran both the `recommended` and
`strict` rulesets across all 122 `.tsx` files and got **zero violations**,
after validating the setup against a deliberately-broken scratch file so
the zero wasn't a silent misconfiguration. Nothing is broken today.

The case for doing it anyway is regression insurance: the reason the
linter found nothing is a set of disciplines this codebase currently holds
by convention (no raw `<input>`/`<button>` anywhere, every `ActionIcon`
carrying a state-reflecting `aria-label`), which a future contributor
could break without anything catching it.

- [ ] **Step 1:** Install `eslint-config-next` (the standard route for a
  Next.js app; it bundles `eslint-plugin-jsx-a11y` plus Next's other
  recommended rules) as a devDependency.
- [ ] **Step 2:** Add a flat config at `frontend/eslint.config.mjs`. The
  audit's scratch configs are gone, but its Method section records exactly
  what worked: `jsx-a11y`'s `flatConfigs.recommended`, with
  `@typescript-eslint/parser` for `.tsx` resolution.
- [ ] **Step 3:** Add a `"lint": "next lint"` (or `eslint .`) script and
  wire it into whatever CI runs `npm test` for `frontend/`.
- [ ] **Step 4:** Run it and confirm zero violations across
  `app/**/*.tsx` and `components/**/*.tsx` — reproducing the audit's
  result now that its temporary tooling is gone. If anything *does* fire,
  it was introduced by Tasks 1–6 and must be fixed before landing.
- [ ] **Step 5:** Commit.

---

### Task 8: Final verification

**Files:** none — verification only.

- [ ] **Step 1: Full frontend suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 2: Live axe re-run against every route the audit covered**

Run the Task 6 spec against a real deployment, and additionally re-check
the logged-in-only routes the anonymous spec can't reach (`/track/mine`,
`/track/mine/add-ticket`, `/chat`, `/lines/new`) by the audit's own method
— injecting axe-core 4.10.2 via the browser and calling
`window.axe.run(document, { resultTypes: ['violations'] })` per page.

Expected: zero `color-contrast`, zero `landmark-one-main`, zero `region`,
zero `heading-order`, zero `page-has-heading-one` across all of them.

**Two known non-regressions to expect and not chase:**
- `/connect-claude` still 500s (minified React error #130). The audit
  scoped this out as a functional bug, not an accessibility defect; its
  `error.tsx` fallback now has an `h1` (Task 3) and a `<main>` (Task 1),
  which is all this plan owes it.
- Routes gated behind data a test account doesn't have (a populated
  ticket list, a live tracked train) still won't render. Their badge
  colours are covered deterministically by Task 4 Step 5's palette
  assertions instead.

- [ ] **Step 3: Confirm the source specs are updated, not left stale**

Two documents now describe resolved state as open:
- `docs/superpowers/specs/2026-08-18-grape-theme-design.md:156-162`
  ("**Still open:** white-on-grape-6 *filled buttons* remain at 4.02:1")
  and `:185-189` (dimmed grey / white-on-amber flagged as unmeasured).
  Mark both resolved, pointing at this plan and at the audit.
- `frontend/lib/theme.ts`'s comment — already rewritten in Task 4 Step 2;
  verify the "recommendation to revisit this… later" sentence is gone.

- [ ] **Step 4: No backend verification required**

Per Global Constraints, this plan makes no backend changes; `cargo test`
is not part of its verification.
