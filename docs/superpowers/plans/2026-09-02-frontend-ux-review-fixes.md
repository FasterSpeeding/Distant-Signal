# Frontend UX Review Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1–6 are independent of each other and of everything else** — land
> them in any order, one commit each. **Tasks 7, 8 and 9 are not
> independent:** 7 and 9 both edit `frontend/app/page.tsx`, and 9 cannot
> compile until 8 has shipped the fields it renders. Do 7 → 8 → 9 in that
> order.
>
> **This plan overlaps four files with the unimplemented
> `2026-09-02-frontend-accessibility-fixes.md`.** Neither plan has landed;
> whichever goes second must rebase rather than re-derive. The exact
> collision list is in "Interaction with the accessibility-fixes plan"
> below — read it before touching `app/error.tsx`, `app/globals.css`,
> `app/globals.test.ts` or `app/page.tsx`.

**Goal:** implement fixes for the six highest-ranked findings in
`docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md` — F1 (station
search ranking), F2 (blank logged-in home page), F3 (raw CRS codes as
labels), F4 (no pinning at mobile width), F5 (internal strings leaking into
user-facing copy, three separate root causes), and F7 (unthemed blue links
in incident bodies).

**The review's own posture is carried into this plan.** That document
reviewed 73 screenshots against this app's design specs while explicitly
*not* treating those specs as binding, and three of these six findings
recommend going past or against a spec decision:

- **F1** overrides `2026-07-11-operator-station-autocomplete-design.md`'s
  Non-goal (*"plain substring `ILIKE` on code or name is enough for a list
  this size"*, lines 23–24). It is not enough; Task 1 changes it.
- **F2** overrides `2026-08-31-anonymous-user-ux-design.md`'s explicit
  decision that the logged-in zero-pin case is *"arguably fine for them
  specifically"*. Task 7 reverses that call.
- **F3** goes past `2026-09-01-tracked-trains-home-page-design.md`
  Decision 1, which permits a bare origin. Tasks 8–9 show names anyway.

Where this plan departs from the *review* as well, the reason is recorded
(see "Where code investigation corrected the review", below). Findings F6,
F8–F14 and D1–D6 from that review are **out of scope here** — see
"Explicitly out of scope".

**Architecture:** two backend changes and six frontend changes, no new
services, no new dependencies, no migrations.

- **Backend (`crates/api`)**: one `ORDER BY` rewrite in
  `data/reference.rs` (Task 1); station-name `LEFT JOIN`s added to four
  read models in `data/train_tracking.rs` (Task 8); four validation
  strings rewritten as human copy in the same file (Task 5).
- **Frontend**: two `visibleFrom` props deleted (Task 2); one `data-*`
  attribute plus one `globals.css` rule (Task 3); one rewritten
  `app/error.tsx` (Task 4); one new label map (Task 6); one call-site move
  plus one extracted component in `app/page.tsx` (Task 7); station-name
  rendering across six sites (Task 9).

**Task-to-finding map**, since the tasks are ordered by dependency rather
than by the review's ranking:

| Task | Finding | Layer |
|---|---|---|
| 1 | F1 station search ranking | **backend** (`crates/api`) |
| 2 | F4 missing Pin column at mobile | frontend |
| 3 | F7 blue links in incident bodies | frontend |
| 4 | F5b `app/error.tsx` prints `error.message` | frontend |
| 5 | F5a snake_case validation copy | **backend** + one frontend test |
| 6 | F5c 32-hex source ID in body copy | frontend |
| 7 | F2 blank logged-in home page | frontend |
| 8 | F3a station names on the API rows | **backend** (`crates/api`) |
| 9 | F3b render names instead of codes | frontend |
| 10 | final verification | — |

**Tech Stack:** Rust 1.x + Axum + `sqlx` 0.8 runtime-checked `query_as`
(the workspace deliberately carries no `.sqlx` cache — see
`crates/api/src/data/reference.rs:5-8`); Postgres. Next.js 16 App Router +
TypeScript + Mantine v9.5.2, Vitest 2 + `@testing-library/react` via
`frontend/test/render.tsx`'s `renderWithMantine`.

**Specs:**
- `docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md` — the review
  this plan implements. Its Findings section is authoritative for *what* to
  fix; this plan departs from it on *how* and on *blast radius* in five
  places, each recorded below.
- `docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md`
  — the spec F1 overrides. Its Non-goals (lines 16–27) and its verbatim
  query shape (lines 45–48) are what Task 1 replaces.
- `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` — the
  spec F2 overrides.
- `docs/superpowers/specs/2026-08-18-grape-theme-design.md` — the
  half-finished thread Task 3 extends to sanitized HTML.

---

## Verified facts (ground truth for this plan — do not re-derive)

Everything below was read out of the working tree during planning. File and
line numbers are as of this branch's HEAD.

### F1 — station search ranking

- `crates/api/src/data/reference.rs:23-35` (`search_stations`) is:
  ```sql
  SELECT crs AS code, name FROM stations
  WHERE crs ILIKE $1 OR name ILIKE $1
  ORDER BY name LIMIT $2
  ```
  with `$1` bound to `%{q}%` at `:24`. `search_tocs` at `:40-52` is the
  identical shape over `tocs`/`atoc_code`. **`ORDER BY name` is the whole
  defect: there is no ranking at all.** Confirmed.
- `crates/api/src/routes/reference.rs:16` caps every response at
  `SUGGESTION_LIMIT = 20`. `sanitize_query` (`:77-84`) only trims; there is
  no minimum length, which is why a single character returns 20 rows that
  look like an unfiltered A–Z list.
- The `stations` table is `crs CHAR(3) PRIMARY KEY, name TEXT NOT NULL, …`
  (`crates/api/migrations/20260706004003_reference_data.sql:10-18`).
- **Two consumers depend on the ordering for correctness, not just
  display** — see "Where code investigation corrected the review", items 2
  and 3.

### F2 — the logged-in home page

- `frontend/app/page.tsx` is a single server component, `DashboardPage` at
  `:73`, with an early-return anonymous branch at `:103-159` and the
  authenticated branch at `:161-267`. `export const revalidate = 0` at
  `:27`.
- **`allReports` is genuinely fetched unconditionally**, at `:95-101`:
  ```ts
  const [preferences, allReports, myTrackedTrains] = await Promise.all([
    getPreferences(), getLineStatusForMode(DISPLAYED_MODES_PARAM), getMyTrackedTrains(),
  ]);
  ```
  above the branch at `:103`. A logged-in user already pays for this fetch
  and, with zero pinned lines, discards all of it. The review's claim is
  correct.
- **The "Right now" logic is already extracted** as
  `notGoodServiceSummary` at `:59-71` (module scope, pure, takes
  `allReports`). It is called from exactly one place, `:104`. Making it
  serve both branches is a call-site move, not a refactor.
- The "Right now" *rendering* is **not** `LineStatusCard` — it is a
  hand-rolled `Link > Card > Group` with `report.name` + `StatusBadge` at
  `:132-141`, inside the anonymous branch's JSX.
- With zero pins the authenticated branch renders: an empty
  `Group justify="flex-end"` (`:197-199`), the "Your Lines" empty
  one-liner (`:206-208`), the "Your Stations" empty one-liner
  (`:224-226`), and nothing else — the tracked-trains section at `:253-265`
  is gated on `trackedTrains.length > 0`.
- Existing tests: `frontend/app/page.test.tsx` (195 lines), using
  `renderWithMantine(await DashboardPage())` with `vi.mock('@/lib/api')`
  (`:8`) and a `next/navigation` stub (`:12-15`). **`page.test.tsx:98`
  asserts `queryByText(/Right now/)` is absent for a logged-in user — but
  that test pins `['central']`, so it still passes under the rule this plan
  adopts.** No test covers "logged in with zero pins" as a whole-page
  outcome.

### F3 — CRS codes as labels

Every site the review names, verified:

| Site | File:line | Field |
|---|---|---|
| Home "Your Tracked Trains" row | `frontend/app/page.tsx:290` (built), `:297` (rendered) | `pinOriginCrs` / `pinDestinationCrs` |
| `/track/mine` train rows | `frontend/app/track/mine/page.tsx:132`, `:145` | same |
| Attach-ticket `Select` options | `frontend/components/AttachTicketAction.tsx:66-68` | same |
| Ticket route line ("PAD → RDG") | `frontend/components/TicketSummary.tsx:41`, `:48` | `originCrs` / `destinationCrs` |
| `/train/by-id` + `/train/[uid]/[date]` pin summary | `frontend/components/TrainJourney.tsx:20-21` (rendered at `:32,45,64,79,96`) | `pinOriginCrs` / `pinDestinationCrs` |
| `/lines/new` + `/lines/[id]/edit` station chips | `frontend/app/lines/CustomLineForm.tsx:193-197` | local `stations: string[]` |

- **No station-name field exists on any of these API responses.**
  `TrackedTrainState` (`crates/api/src/data/train_tracking.rs:307-320`),
  `TrackedTrainListItem` (`:344-357`), `TrackedTrainTicket` (`:592-601`)
  and `TicketListItem` (`:741-762`) all carry CRS only, and none of their
  queries (`:322-331`, `:374-383`, `:603-606`, `:826-838`) joins
  `stations`. The names *are* in the database — `data/reference.rs:26`
  reads them — but nothing puts them on these rows.
- `pin_origin_crs` is `TEXT NOT NULL`
  (`crates/api/migrations/20260828120000_train_tracking.sql:61`) and
  `validate_pin` (`crates/api/src/data/train_tracking.rs:40-51`) **does
  not upper-case it**, while `stations.crs` is `CHAR(3)`. Any join must go
  through `UPPER(...)` — see Decision 3.
- `frontend/app/lines/CustomLineForm.tsx:58-67` already maintains a
  `nameByCode` cache and already uses it for `title=` on the operator pill
  (`:152`) and the destination-CRS pill (`:213`). The station badges at
  `:193-197` simply don't. **That one is a pure-frontend, two-line fix with
  the data already in hand.**
- Precedent for the display format already exists in this app, twice:
  `frontend/app/page.tsx:236` renders pinned stations as
  `` `${name} (${crs})` `` and `frontend/app/stations/[crs]/page.tsx:55`
  renders the heading the same way. The comment at `page.tsx:179-182`
  already argues F3's case in this repo's own words: *"The station detail
  page already shows 'London Kings Cross (KGX)'; there is no reason for the
  dashboard to show a bare code."*
- `getStationName(crs)` (`frontend/lib/api.ts:115-121`) exists, is
  1-hour-cached, and is used at `frontend/app/page.tsx:183` and
  `frontend/app/stations/[crs]/page.tsx:31`. **It is rejected as the
  mechanism for this plan** — Decision 3.
- There is **no** bulk stations endpoint. `crates/api/src/routes/reference.rs:18-23`
  exposes `/stations`, `/tocs` and `/tocs/all`; the `all` variant is
  TOCs-only.

### F4 — the missing Pin column

- Mechanism: two Mantine `visibleFrom="sm"` props —
  `frontend/app/lines/AllLinesTable.tsx:210` (the `<TableTh>Pin</TableTh>`)
  and `:260` (the `<TableTd>` wrapping `<PinToggle>` at `:261`).
  `visibleFrom` emits Mantine's static `mantine-visible-from-sm`
  (`display: none`) class, so the element is in the DOM but not rendered,
  and is out of the tab order and the accessibility tree.
- Breakpoint is Mantine's default `sm` = `48em` = 768px:
  `frontend/lib/theme.ts:27` is `createTheme({ primaryColor: 'grape' })`
  with no `breakpoints` override. Matches the observed 390px-hidden /
  768px-visible behaviour exactly.
- The mobile fold at `:225-227` re-surfaces **numbers only**
  (`formatSampleSummary`, `frontend/lib/sampleStats.ts:45-53`). Nothing
  pin-related. There is no other pinned-state indicator on the row:
  `pinnedSet` (`:80`) is referenced exactly once, at `:261`.
- **`AllLinesTable.test.tsx:352-357` is a test that actively locks in the
  bug** (`it('hides the Pin column below the sm breakpoint too', …)`). Task
  2 must invert it.
- **Provenance: this is a one-day-old deliberate regression.** Commit
  `bd4d739` "Hide the Pin column on mobile in the all-lines table"
  (2026-09-01) added exactly those two props plus that test, reasoning from
  *consistency* with the neighbouring numeric columns — and missing that
  those columns were hidden only *because they were re-surfaced* in the
  `hiddenFrom="sm"` sub-line. Pin got the hiding half of the pattern
  without the re-surfacing half.
- `PinToggle` (`frontend/components/PinToggle.tsx:40`) is a
  `Tooltip` + `ActionIcon` at default Mantine `md` size (34×34px), with
  `aria-label` from `:109` (`'Unpin (currently pinned)'` /
  `'Pin (currently not pinned)'`) and `variant`/`color` both varying with
  state (`:115`, `:118`).

### F5 — three separate leaks, three separate root causes

**(5a) Raw snake_case validation copy.** Origin:
`crates/api/src/data/train_tracking.rs:40-51`, `validate_pin`. Three
strings, all of them dev-facing: `:42` `"origin_crs must not be empty"`,
`:45` `"origin_crs must be a 3-letter CRS code"`, `:48`
`"scheduled_departure is too far in the past to track"`. `MAX_PIN_AGE = 6
hours` at `:23`.

Transport: **plain text, status 400.** `crates/api/src/routes/train.rs:367`
does `.map_err(|msg| (StatusCode::BAD_REQUEST, msg))?`, and axum's
`(StatusCode, String)` `IntoResponse` emits `text/plain`. There is no error
envelope anywhere in this API.

Rendering: `frontend/components/TrackTrainForm.tsx:122-125` reads the body
as text on a 400 and stores it unmodified; `:186-190` renders it as the
`Alert` body under the fixed title "Couldn't track this train".
`frontend/app/api/[...path]/route.ts:127-137` passes status and body
through verbatim. **`TrackTrainForm.test.tsx:122` and `:131` currently
assert the raw string is displayed** — Task 5 must update them.

Sibling with the same defect, one notch worse:
`validate_ticket_entry` (`crates/api/src/data/train_tracking.rs:99-113`),
whose `:100` does `format!("source must be one of {TICKET_SOURCES:?}")` —
a Rust `Debug` of a `[&str; 4]` array rendered into user copy, surfaced at
`frontend/components/TicketEntryForm.tsx:178` and `:240`.

Existing conventions that shape the fix: there is **no** error-code enum or
message-mapping layer between backend and frontend (grepped: no
`ErrorCode`/`error_code`/JSON `{"code": …}` in `crates/api/src`). The
`internal_error` helpers (`routes/train.rs:695-703` and eight siblings) do
establish the house rule *"generic string to the user, real error to the
log"* — but only for 5xx; 400s bypass it entirely. On the frontend the
good precedents are `AttachTicketAction.tsx:45-58` (status → house copy,
never reads the body) and `lib/impactType.ts:7-24` (value → label map,
documented as *"fail safe to render nothing rather than a raw snake_case
string"*).

**(5b) `app/error.tsx` renders `error.message`.** The whole file is 20
lines. `:15` is `<Text c="dimmed">{error.message}</Text>`. It does **no**
logging — no `useEffect`, no `console.error` — and `error.digest`, declared
at `:10`, is never used, so the one value that would correlate with server
logs is discarded while the useless one is shown. There is a `reset` button
at `:16-18` and **no navigation link at all**, unlike all five
`not-found.tsx` templates.

**This is the only error boundary in the app.** `find frontend -name
"error*"` returns exactly one file: no `global-error.tsx`, no per-route
`error.tsx`. `frontend/app/layout.tsx:59-62` and `:73-76` explicitly
acknowledge that gap and work around it with `.catch()` fallbacks. That is
also why its hardcoded title "Couldn't load status data" (`:13`) is wrong
on `/track`, `/chat` and everywhere else. **There is no
`frontend/app/error.test.tsx`.**

**(5c) 32-hex ID in the incident footer.** Rendered at
`frontend/components/DisruptionDetail.tsx:35-39` as
`Source: {disruption.source}` (type `string | null`,
`frontend/lib/types.ts:17`), reached through
`frontend/components/IssueList.tsx:390` inside the accordion panel — so it
appears on every line page and station page, not only incident pages.
Pinned by `DisruptionDetail.test.tsx:42`.

The value is constructed at `crates/aggregator/src/aggregation.rs:156`
(`format!("knowledgebase-incident-{}", incident.incident_id)`) and takes
three known shapes: `knowledgebase-incident-{id}`, `ldbws-sampling`
(`aggregation.rs:959`), `tfl-line-status-{lineId}`
(`crates/poller-tfl/src/schema.rs:143`). **The prefix already encodes a
human-mappable source enum; no frontend label map exists for it.**

The ID itself is already consumed for something useful *by the next line
down*: `frontend/lib/incidents.ts:11-14` strips the prefix and
`DisruptionDetail.tsx:40-44` renders a "View full incident details" link
from it. So the `Source:` line is pure debug text with zero affordance
sitting directly above a properly-labelled link built from the same value.

### F7 — blue links in sanitized incident HTML

- Exactly **two** `dangerouslySetInnerHTML` call sites in the repo, both
  through one sanitizer: `frontend/components/DisruptionDetail.tsx:20` and
  `frontend/app/incidents/[id]/page.tsx:61`.
- **Both are a bare `<div>` with no `className`, no `data-*`, no Mantine
  `Box`, no `TypographyStylesProvider`.** There is nothing to scope a CSS
  rule to today. The review's one-line "just add a CSS rule" is a step
  short — see "Where code investigation corrected the review", item 5.
- The sanitizer (`frontend/lib/sanitizeHtml.ts`) allows `a` (`:17`
  `ALLOWED_TAGS`) with `ALLOWED_ATTR = ['href']` (`:18`), and an
  `afterSanitizeAttributes` hook at `:10-15` re-adds `target="_blank"` /
  `rel="noopener"`. **It emits no `class` and no `data-*` on the anchors**,
  so they cannot be targeted except as descendants of a container. No
  backend sanitization exists (`crates/api/src/routes/incidents.rs:69`
  passes `description` through verbatim; no `ammonia` in any `Cargo.toml`).
- `frontend/app/globals.css:25-27` is the only custom-property block:
  ```css
  html:root[data-mantine-color-scheme='light'] {
    --mantine-color-anchor: var(--mantine-color-grape-7);
  }
  ```
  **Light only, deliberately.** The dark scheme falls back to Mantine's own
  `primaryColor`-4 (grape 4 `#da77f2`, 5.84:1 on the dark body), documented
  at `globals.css:16-18` and asserted at `globals.test.ts:53-55`. So
  `var(--mantine-color-anchor)` resolves correctly in **both** schemes and
  the F7 rule needs no dark companion — which matters, because
  `globals.test.ts:50` asserts the file contains no
  `data-mantine-color-scheme='dark'` selector at all.
- `globals.test.ts` is a pure-Node string-assertion suite:
  `const css = readFileSync('app/globals.css', 'utf8')` at `:5`, with a
  WCAG `luminance(hex: string): number` at `:18-22`, `contrast(a, b):
  number` at `:24-27`, `AA_BODY_TEXT = 4.5` at `:29`, and hex constants
  `GRAPE_4 / GRAPE_6 / GRAPE_7 / WHITE / DARK_7` at `:9-13`. Both contrast
  facts F7 would want are **already asserted**: grape-7 on white ≥ 4.5 at
  `:36-38`, grape-4 on `#242424` ≥ 4.5 at `:53-55`. Do not duplicate them
  — Decision 7.
- **CSS-module convention: there is none.** `find . -name '*.module.css'`
  returns nothing; `frontend/app/globals.css` is the only `.css` file in
  the repo. The established pattern is a `data-*` hook set in the component
  plus a flat rule in `globals.css` — `data-text-link`
  (`TextLink.tsx:40` ↔ `globals.css:38-49, 901-903`), `data-status-badge`
  (`globals.css:918-922`), `data-site-title` (`layout.tsx:161`),
  `.issueRow*` (`IssueList.tsx:343-352` ↔ `globals.css:933-991`). Mantine
  `styles={{…}}`/`classNames={{…}}` props are used **nowhere**.
- `frontend/lib/theme.ts` is 26 lines of comment and one line of code:
  `createTheme({ primaryColor: 'grape' })`. No `autoContrast`, no
  `luminanceThreshold`, no `variantColorResolver`.

### Environment and tooling

- **`frontend/node_modules` is not installed in this worktree.** Every
  claim about Mantine internals must be confirmed with `npm ci` first —
  the same posture the accessibility-fixes plan takes.
- CI (`.github/workflows/ci.yml`) runs `cargo clippy --workspace
  --all-features` (`:98`), `cargo fmt --all --check` (`:139`,
  `continue-on-error` today), `cargo test --workspace` (`:206`), then —
  after `sqlx migrate run` against a **freshly migrated, empty** database —
  `cargo test -p api -p aggregator -- --ignored --test-threads=1` (`:216`).
  Frontend: `npm test` (`:252`) then `npm run build` (`:255`), plus
  `tsc --noEmit` as the lint step.
- **DB-backed tests therefore run in CI and are real coverage.** The house
  pattern is `#[tokio::test]` + `#[ignore = "requires a live database; …"]`
  reading `DATABASE_URL`, connecting with `PgPoolOptions`, seeding its own
  fixtures and deleting them at the end — see
  `crates/api/src/data/custom_lines.rs:321-380` and
  `crates/api/src/data/users.rs:229-280`. The CI database has **no seeded
  reference data**, so a ranking test must insert its own `stations` rows.

---

## Where code investigation corrected or extended the review

The review's Method section warns that several screenshots were mislabeled
or byte-identical duplicates and tells later readers not to trust a
citation over the code. Six places where reading the code changed the
picture:

1. **F4 is worse than reported, and it is a regression, not an
   oversight.** The review says a mobile user's *"only route to a pinned
   line is via each station page's pin button"* — but that button is
   `<PinToggle kind="station" …>` (`frontend/app/stations/[crs]/page.tsx:77`).
   `PinToggle` appears in exactly two places app-wide, and
   `frontend/app/lines/[id]/page.tsx` has no import of it at all, while the
   home dashboard renders pinned lines as read-only `LineStatusCard`s.
   **Below 768px there is no way to pin or unpin a *line* anywhere in the
   application.** It also arrived one day before the review, in commit
   `bd4d739`, with a test locking it in.

2. **F1 is a navigation-correctness bug, not only a display-ranking one.**
   `frontend/app/stations/StationSearchForm.tsx:27` resolves the "Look up"
   button's target as
   `exactCode ?? exactName ?? suggestions[0]?.code ?? trimmed.toUpperCase()`
   — so the backend's `ORDER BY` decides *where the user lands* whenever
   they type a partial name. Under `ORDER BY name`, typing "Yor" and
   pressing Look up navigates to Bentley (South Yorkshire). (The review
   scoped a separate "stations-lookup wrong navigation" bug out of its own
   findings; Task 1 changes this code path's behaviour and whoever holds
   that bug should be told.)

3. **F1 is a prerequisite for F3, and fixes a latent data bug on the way.**
   `getStationName` (`frontend/lib/api.ts:115-121`) resolves a CRS by
   calling the *substring* search and filtering for an exact code match —
   but the backend caps at `SUGGESTION_LIMIT = 20` and orders
   alphabetically. For a code whose letters are a common name substring
   (`WAT` also matches Blackwater, Bridgwater, Waterbeach, Watford
   Junction, Waterloo…), the exact row can be truncated out of the window,
   and `getStationName` silently returns `null`. Exact-code-first ordering
   makes the correct row *always* row 1, so it can never be truncated.
   Task 1 Step 4 tests exactly this.

4. **F3 is not frontend-only, contrary to the review.** The review says
   *"Station names are already in the frontend's reach (the same reference
   data the autocomplete queries)."* For the tracked-train and ticket
   surfaces that is not true in any usable sense: no name field exists on
   any of the four read models, there is no bulk stations endpoint, and the
   only lookup is a per-CRS substring search that would mean ten
   round-trips per home-page render. **It *is* frontend-only for the
   `CustomLineForm` chips** — `nameByCode` already holds the answer there.
   This plan therefore splits F3 into a backend task (8) and a frontend
   task (9).

5. **F7 needs a component change, not just a CSS rule.** The review's
   recommendation is *"a scoped CSS rule on the sanitized-content
   container"* — but both containers are bare `<div>`s with no class, and
   the sanitizer's `ALLOWED_ATTR = ['href']` strips any `class`/`data-*`
   the upstream HTML might carry. There is nothing to scope to until an
   attribute is added. **F7's blast radius is also larger than the review
   states:** `DisruptionDetail` renders inside `IssueList`'s accordion
   (`IssueList.tsx:390`), so the blue links are on every line and station
   page, not just `/incidents/[id]`.

6. **F5 has a fourth instance the review didn't see, and 5b is worse than
   described.** `validate_ticket_entry`
   (`crates/api/src/data/train_tracking.rs:100`) formats a Rust `Debug` of
   a `[&str; 4]` into user copy — the same defect class, in the same file,
   one function down. And `app/error.tsx` is not merely rendering
   `error.message`: it is the app's *only* error boundary, it logs nothing
   at all, it discards `error.digest`, and it offers no way out of the page
   but a `reset` that will re-throw on a render error.

Two things the review got exactly right and this plan does not
second-guess: **F2's premise** (`allReports` really is fetched
unconditionally at `frontend/app/page.tsx:95-101`, and
`notGoodServiceSummary` really is already extracted at `:59-71`, so the fix
really is a call-site move), and **F7's premise** (the blue "Planned Work"
badge at `frontend/app/incidents/[id]/page.tsx:58` really does sit three
lines above the injected HTML at `:61`).

---

## Design

### Decision 1 — F1 is three tiers in one `ORDER BY`, with no new index, no `pg_trgm`, and the same `WHERE`

The review's recommendation is *"keep the substring match, add a three-tier
`ORDER BY` — exact CRS/code match first, name-prefix match second,
everything else after, alphabetical within tiers. This is a one-query
change, not a search engine."* This plan takes that literally.

```sql
SELECT crs AS code, name FROM stations
WHERE crs ILIKE $1 OR name ILIKE $1
ORDER BY
  CASE
    WHEN crs ILIKE $2 THEN 0   -- $2 = q            (exact code, no wildcards)
    WHEN name ILIKE $3 THEN 1  -- $3 = q || '%'     (name prefix)
    ELSE 2                     --                    (substring, already in WHERE)
  END,
  name
LIMIT $4
```

Three properties worth stating, because each is load-bearing:

- **The `WHERE` clause is unchanged.** Nothing that matched before stops
  matching; only the order changes. That keeps the autocomplete spec's
  "plain substring is enough" *mechanism* while overriding its conclusion
  about ordering, and it means the change cannot lose a result.
- **`crs ILIKE $2` with no wildcards is an exact, case-insensitive
  comparison.** It is used rather than `crs = UPPER($2)` so that both
  operands go through the same operator the `WHERE` already uses on this
  `CHAR(3)` column, and so a two-character query can never accidentally
  match a three-character code.
- **Alphabetical-within-tier already puts an exact name match at the top of
  the prefix tier for free.** A name that equals the query is the shortest
  string having the query as a prefix, and a shorter string that is a
  prefix of a longer one sorts first. So "Ash" beats "Ash Vale" and "Leeds"
  beats "Leeds Whatever" without a fourth tier. **Do not add an exact-name
  tier**; it would be dead code.

**No index.** The autocomplete spec's Non-goal ("No new database indexes…
`stations`/`tocs` are small (~2,500 / ~30 rows); a sequential `ILIKE` scan
is fast enough") is *not* overridden here — it is still right, and the
`CASE` is evaluated on rows the scan already produced. Only the
"plain substring is enough" Non-goal is overridden.

**`search_tocs` gets the identical treatment** (`atoc_code` in place of
`crs`). The review only names the station query, but `search_tocs` is a
byte-for-byte copy of the same defect over the same shape; fixing one and
not the other would leave the operator field ranking "SW" below whatever
sorts first alphabetically among ~30 names, and would make the two
functions diverge for no reason. Zero extra risk: same query shape, same
30-row table.

**Not fixed here:** the single-character query still returns 20 rows,
because 20 rows *do* match. Tiering makes them better rows (exact code
first), but a minimum query length is a separate product decision the
review did not ask for and this plan does not make.

### Decision 2 — F2 renders "Right now" whenever a logged-in user has zero *pinned lines*, and the module becomes a real component

The gate is **`pinnedLineReports.length === 0`**, exactly as the review
words it ("whenever they have zero pinned lines"), not "zero pins of any
kind". A user with pinned stations but no pinned lines still has a
line-shaped hole in their dashboard, and "Right now" is a lines module.

Mechanically:

- Move the `notGoodServiceSummary(allReports)` call from inside the
  anonymous branch (`frontend/app/page.tsx:104`) to just above the branch,
  so both branches read one value. It is a pure function of data already
  fetched at `:95-101` — **no new fetch, no new endpoint, no extra
  latency.**
- Extract the module's JSX (currently inline at `:122-145`) into a local
  component in the same file, `RightNowModule({ summary })`. Local, not a
  new file under `components/`: it is used twice, in one file, and both
  uses are server-rendered. A new shared component would be the right call
  only if a third page wanted it.
- Render it in the authenticated branch **after** the "Your Stations"
  section, so the review's instruction ("keeping the empty 'Your
  Lines/Stations' prompts above it as one-liners") is satisfied literally.

**Heading level: `h2`.** The authenticated branch's hierarchy becomes
h1 "Your Lines" → h2 "Your Stations" → **h2 "Right now"** → h2 "Your
Tracked Trains". No skip. This changes a table in the sibling
accessibility plan — see "Interaction with the accessibility-fixes plan".

**Not changed:** the anonymous branch renders exactly what it renders
today. This task adds a second call site; it must not restyle the module,
and a screenshot of the logged-out home before and after should be
identical.

### Decision 3 — F3 puts station names on the API rows (`LEFT JOIN stations`), rather than looking them up from the frontend

Three mechanisms were considered.

**(a) Fan out `getStationName` from the server components. Rejected.**
It is the existing helper and it is already used for pinned stations, so it
looks like the obvious answer — but: the home page would issue up to 10
lookups per render (5 rows × 2 CRS), `/track/mine` is capped at 100 trains
and 100 tickets rather than 5, the helper is a substring search with the
LIMIT-20 truncation hazard described above (so it depends on Task 1 landing
first just to be *correct*), and it produces values in server components
that then have to be threaded as extra props into three client components
(`AttachTicketAction`, `TicketSummary`, and `TrainJourney`'s callers). A
caller that forgets the prop silently renders a bare code again, with
nothing to catch it.

**(b) Add `/public/stations/all`, mirroring `/public/tocs/all`, and map
client-side. Rejected, but it is the recorded fallback.** It is a genuinely
small backend change (a copy of `get_all_tocs`/`list_all_tocs`) and
`frontend/app/lines/AllLinesTable.tsx:81` already does exactly this for
TOC codes. It fails on two counts: it ships ~2,500 rows to resolve two
codes on `/train/by-id`, and it still needs the same prop-threading as (a),
with the same "forgot the prop" failure mode. **If Task 8 proves
unexpectedly awkward, this is the escape hatch — record why before taking
it.**

**(c) `LEFT JOIN stations` in the four read models. Chosen.** The name
travels with the row, so every consumer — server component, client
component, `Select` option, list row — gets it by reading a field, and
TypeScript enforces it. The cost is one indexed lookup per CRS against a
2,500-row table with `crs` as its primary key, inside queries that already
join two tables. This is also the principle this file already states in its
own words at `crates/api/src/data/train_tracking.rs:806-810`, about
`list_tickets_for_user`'s existing joins: *"the joins to
`tracked_trains`/`train_current_state` exist purely to pull in enough train
context for a useful row … with no per-ticket follow-up query."* A station
name is exactly that kind of context.

**The join must go through `UPPER(...)`:**

```sql
LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs)
```

`pin_origin_crs` is `TEXT` and `validate_pin` never normalises its case,
while `stations.crs` is `CHAR(3)`. A user who typed `kgx` has `kgx` in the
table, and a naive `so.crs = tt.pin_origin_crs` would silently produce
`NULL` for them — i.e. exactly the bare-code fallback this task exists to
remove, for the subset of users most likely to hit it. **Do not "fix" this
by normalising on write:** that needs a migration over existing rows and is
a separate change; `UPPER()` on the read side is complete and reversible.

**Display format: `Name (CRS)`, via one shared helper.** Not name-only.
Two existing call sites already do this (`app/page.tsx:236`,
`app/stations/[crs]/page.tsx:55`), the review endorses it explicitly
("show 'London Kings Cross (KGX)' or name-only"), and the code is what a
user cross-references against a ticket or a departure board — dropping it
would trade one lookup exercise for another. A single helper in
`frontend/lib/stationLabel.ts` keeps the fallback ("no name → bare code",
never `undefined`, never "null") in one place.

### Decision 4 — F4 simply restores the Pin column at all widths; no action sheet, no size change

The review offers two options: *"keep the star at all widths (it needs
~44px; the folded layout has the room), or failing that, add pin to a
row-tap action sheet."* Take the first. At 390px the table then carries
three columns — Name (with its folded numeric sub-line), Status, Pin —
which is what it carried before `bd4d739`, plus the sub-line that commit's
siblings added. There is no width problem to solve; the commit that created
this was reasoning from column-count consistency, not from a measured
overflow.

**The `ActionIcon` size is left at Mantine's default `md` (34×34px).** The
review's "~44px" is WCAG 2.5.5 Target Size (Enhanced, AAA); the AA
requirement is 2.5.8 Target Size (Minimum) at 24×24 CSS px, which 34px
clears with room. Mantine's `ActionIcon` takes no responsive `size`, so a
44px mobile variant would mean rendering two `ActionIcon`s behind
`hiddenFrom`/`visibleFrom` — duplicating an interactive control, and its
`LoginPromptModal`, to buy an AAA criterion nothing else in this app
targets. Not worth it. **Record this if it is revisited.**

**Out of scope, but found while investigating and worth filing:**
`frontend/app/lines/[id]/page.tsx` has no `PinToggle` at all, so a line's
own detail page cannot pin it at any width. That is a separate gap from
F4's, the review did not raise it, and this plan does not fix it.

### Decision 5 — F5 is three tasks, because it is three unrelated defects

The review lists three bullets under one heading because they are the same
*class* of problem. They share no mechanism:

| | Where the wrong string comes from | Fix layer |
|---|---|---|
| 5a | Rust validation functions writing spec prose | backend copy + a frontend test update |
| 5b | A React error boundary printing its input | frontend component rewrite |
| 5c | A provenance ID with no label map | new frontend label map |

Three tasks, three commits, three revert surfaces. Merging them would mean
a backend copy change and a React error-boundary rewrite in one diff.

**5a fixes the copy at the source rather than mapping it on the
frontend.** With no error envelope in this API, the frontend cannot tell a
validation 400 from any other 400, so a frontend map would have to key on
the string it is trying to stop trusting. Rewriting the four strings as
human copy is complete for every path the app's own forms can reach, and
the 6-hour figure must be interpolated from `MAX_PIN_AGE` rather than typed
into prose, so the message can never drift from the constant.

**Explicitly not done in 5a:** introducing a JSON error envelope
(`{code, message}`) so the frontend can map codes to copy. It is the
better long-term shape and it would also contain axum's own
`Failed to deserialize the JSON body into the target type: …` extractor
rejection at `crates/api/src/routes/train.rs:365` — but that rejection is
unreachable from the app's own form, which always sends a well-typed body,
and an envelope means touching every `(StatusCode, String)` handler in the
crate. Out of scope; filed in Open Questions.

**5b keeps `error.digest` and drops `error.message`.** That is not a
contradiction of this finding: `digest` is Next.js's deliberately opaque
correlation hash, produced *for* showing to users so they can quote it, and
it is the review's own prescription for 5c's ID ("formatted as a short
reference"). `message` is the one that carries React internals and stack
detail. `digest` is only populated for server-side errors, hence the
conditional render.

**5c replaces the ID with a label rather than hiding it.** The review
offers "behind the ⓘ affordance or formatted as a short reference". A third
option is better: the prefix already *is* a source enum, so map it —
`knowledgebase-incident-*` → "National Rail Knowledgebase", `ldbws-sampling`
→ "Live departure boards", `tfl-line-status-*` → "Transport for London".
That turns debug text into the fact a reader actually wants, follows the
established `lib/impactType.ts` / `DATA_QUALITY_LABELS` convention
including its documented fail-safe ("render nothing rather than a raw
snake_case string"), and needs no new interactive control. The raw string
stays reachable as a `title=` attribute for debugging — the same tactic
`CustomLineForm.tsx:152` already uses. The `InfoIcon` + `Tooltip` pattern
(`components/InfoIcon.tsx`, used by `LineDefinitionTooltip` and
`DataFreshnessInfo`) is rejected here as a heavier control than a value no
user needs to read deserves.

### Decision 6 — F7 adds a `data-rich-text` hook, matching `data-text-link`/`data-status-badge`

The rule goes in `frontend/app/globals.css`, flat, keyed on a new
`data-rich-text` attribute set on both injecting `<div>`s:

```css
[data-rich-text] a {
  color: var(--mantine-color-anchor);
}
```

`var(--mantine-color-anchor)` is the right token specifically because it
resolves in both schemes — grape 7 in light via the override at
`globals.css:25-27`, grape 4 in dark via Mantine's own dark block — so no
dark-scheme companion rule is needed, and adding one would break the
`not.toContain("data-mantine-color-scheme='dark'")` assertion at
`globals.test.ts:50`. This also answers the review's Open Question 1 (that
these links are "very likely *worse* in dark mode") without a dark-mode
screenshot pass: the same variable fixes both.

A CSS module would be a novel convention here — there are no `*.module.css`
files in the repo — and Mantine `styles`/`classNames` props are used
nowhere. `data-*` + flat rule is the house pattern, and it is the pattern
this file's own tests can assert against by string match.

**The underline stays.** `a[data-text-link]` removes it
(`globals.css:38-42`) because those links sit in chrome and headings; these
sit mid-paragraph in prose, where colour alone is not a sufficient
distinguisher (WCAG 1.4.1). The new rule must set `color` and nothing else.

**Update the stale comment.** `globals.css:11-13` currently claims
*"Nothing but Mantine's `Anchor` and the `c="var(--mantine-color-anchor)"`
call sites read this variable, so the change is contained to links"*. After
this task a third reader exists; say so.

### Decision 7 — F7's contrast verification adds the *new* property, and does not re-assert the two already proven

The task brief asks for the sibling accessibility plan's
contrast-verification approach, "blue-on-white link contrast, not just
blue-vs-badge-blue confusion". Applying it honestly here means noticing
that the background-contrast half is **already done**:
`globals.test.ts:36-38` asserts grape 7 on white ≥ 4.5 (it is 4.85:1) and
`:53-55` asserts grape 4 on `#242424` ≥ 4.5 (5.84:1). Because the new rule
uses `var(--mantine-color-anchor)` and introduces no new colour value,
those two assertions already cover it. Re-stating them in a new `describe`
block would be duplication dressed as rigour.

What is genuinely new and currently unasserted is the **WCAG 1.4.1
"use of colour"** property: in-prose links must be distinguishable from the
body text around them by something other than colour. So the new block
asserts (a) the `[data-rich-text] a` rule exists and sets
`color: var(--mantine-color-anchor)`, and (b) it does **not** contain
`text-decoration: none` — i.e. the underline the browser gives these
anchors survives the theming change. Assertion (b) is the regression guard
that matters: the obvious future "tidy-up" is to make in-content links look
like `TextLink`, and that would be wrong.

One thing must be checked empirically rather than assumed, because
`node_modules` is not installed: that Mantine v9.5.2 does not itself reset
bare `<a>` colour. The reasoning that it does not is sound —
`TextLink.tsx:41` has to set `c="var(--mantine-color-anchor)"` explicitly,
which would be redundant if Mantine styled bare anchors — but Task 3 Step 1
confirms it against the installed source before the rule is written.

### Decision 8 — ordering, and what actually depends on what

Genuinely independent (any order, separate commits): **Tasks 1, 2, 3, 4,
5, 6.** They touch disjoint files.

Real dependencies:

- **Task 9 depends on Task 8.** Task 9 renders `pinOriginName` etc.; those
  fields do not exist until Task 8 adds them. `npm run build` would fail.
- **Task 9 must land after Task 7.** Both edit `frontend/app/page.tsx` —
  Task 7 in the branch structure (`:95-161`) and the returned JSX, Task 9
  in `TrackedTrainSummaryRow` (`:279-307`). Different regions, so this is a
  merge-conflict avoidance ordering rather than a logical dependency, but
  doing 9 first would mean rebasing a JSX diff under a control-flow diff.
- **Task 1 should land before Task 9** — not because Task 9 calls
  `getStationName` (it does not, by Decision 3), but because Task 1 fixes
  the truncation bug in the two places that *do* call it
  (`app/page.tsx:183`, `app/stations/[crs]/page.tsx:31`), and shipping
  station names on the dashboard while a sibling station-name lookup can
  still return `null` for common codes is an inconsistency a reviewer will
  rightly ask about.

Recommended sequence: **1, 2, 3, 4, 5, 6, 7, 8, 9, 10.**

---

## Interaction with the accessibility-fixes plan

`docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md` exists
on `main` and is **unimplemented** — verified during planning:
`frontend/lib/theme.ts` contains no `autoContrast`, no
`luminanceThreshold` and no `variantColorResolver`, and
`frontend/app/globals.css` contains no `--mantine-color-dimmed` or
`--mantine-color-grape-filled` override. Four real collisions, all
mechanical, none requiring either plan to change its design:

| File | That plan | This plan | Resolution |
|---|---|---|---|
| `frontend/app/error.tsx` | Task 3 changes `:14` `<Title order={2}>` → `<Title order={1} size="h2">` | Task 4 rewrites the whole file | Whichever lands second keeps both: the rewritten file must use `<Title order={1} size="h2">` |
| `frontend/app/globals.css` | Tasks 4–5 extend the `light` custom-property block at `:25` | Task 3 appends a new flat rule at the end | No overlap in the file; a textual conflict is unlikely and trivially resolved |
| `frontend/app/globals.test.ts` | Task 4 Step 5 adds a palette-wide `describe` | Task 3 adds a rich-text-link `describe` | Additive; both use the same `contrast`/`luminance` helpers at `:18-27` |
| `frontend/app/page.tsx` | Decision 7's per-route heading table lists the authenticated branch as h1 → h2 → h2 | Task 7 inserts an h2 "Right now" | **Update that table** — the branch becomes h1 → h2 → h2 → h2, still skip-free |

One non-collision worth knowing: that plan's `autoContrast` change will
flip `PinToggle`'s pinned star from white-on-yellow to black-on-yellow.
Task 2 makes that star visible on mobile for the first time, so the two
changes compound visually — but they do not conflict, and Task 2 needs no
adjustment for it.

---

## Global Constraints

- **No new dependencies**, backend or frontend. No `pg_trgm`, no
  `eslint-*`, no new crates.
- **No migrations.** Task 8 reads `stations` from queries that already
  exist; Task 1 adds no index. Nothing in `crates/api/migrations/` changes.
- **No change to the `WHERE` clause of either reference search.** Task 1
  is ordering only — if a fix appears to need a `WHERE` change, the
  mechanism has been chosen wrong; re-read Decision 1.
- **No hand-mixed colours.** Task 3's only colour value is
  `var(--mantine-color-anchor)`. `globals.test.ts:113`'s existing
  `not.toMatch(/#[0-9a-f]{3,8}/i)` habit applies to any new rule.
- **Light/dark parity comes from the variable, not from a second block.**
  `globals.test.ts:50` asserts `app/globals.css` contains no
  `data-mantine-color-scheme='dark'` selector. That must still pass.
- **No `.sqlx` cache, no `query_as!` macros.** Every query stays
  runtime-checked `sqlx::query_as`, per
  `crates/api/src/data/reference.rs:5-8`.
- **Testing:**
  - Backend: `cargo test -p api` for the unit tests;
    `DATABASE_URL=… cargo test -p api <name> -- --ignored` for DB-backed
    ones. `cargo clippy --workspace --all-features` and
    `cargo fmt --all --check` before committing, matching CI.
  - Frontend: `npm test && npm run build` from `frontend/`. `npm ci` first
    — `node_modules` is not installed in this worktree.
- **File scope.** Modified: `crates/api/src/data/reference.rs`,
  `crates/api/src/data/train_tracking.rs`,
  `frontend/app/lines/AllLinesTable.tsx`,
  `frontend/app/lines/AllLinesTable.test.tsx`,
  `frontend/app/globals.css`, `frontend/app/globals.test.ts`,
  `frontend/components/DisruptionDetail.tsx`,
  `frontend/components/DisruptionDetail.test.tsx`,
  `frontend/app/incidents/[id]/page.tsx`, `frontend/app/error.tsx`,
  `frontend/components/TrackTrainForm.test.tsx`,
  `frontend/components/TicketEntryForm.test.tsx`,
  `frontend/app/page.tsx`, `frontend/app/page.test.tsx`,
  `frontend/app/track/mine/page.tsx`,
  `frontend/components/AttachTicketAction.tsx`,
  `frontend/components/TicketSummary.tsx`,
  `frontend/components/TrainJourney.tsx`,
  `frontend/app/lines/CustomLineForm.tsx`, `frontend/lib/types.ts`,
  plus the colocated test files named per task. Created:
  `frontend/app/error.test.tsx`, `frontend/lib/incidentSource.ts`,
  `frontend/lib/incidentSource.test.ts`, `frontend/lib/stationLabel.ts`,
  `frontend/lib/stationLabel.test.ts`.

---

### Task 1: Rank station and operator search results (F1) — **backend**

**Files:**
- Modify: `crates/api/src/data/reference.rs`

Independent of every other task. Land first: two other call sites
(`StationSearchForm`'s "Look up" target and `getStationName`'s exact-match
filter) silently depend on this ordering for correctness.

- [ ] **Step 1: Rewrite `search_stations`'s query**

`crates/api/src/data/reference.rs:23-35`. Keep the `WHERE` byte-identical;
replace `ORDER BY name` with the three-tier `CASE`, and bind two new
parameters.

```rust
/// Matches `q` as a case-insensitive substring of either the CRS code or
/// the station name, ranked in three tiers: exact code match, then
/// name-prefix match, then any other substring match, alphabetical within
/// each tier.
///
/// The ranking exists because plain `ORDER BY name` demonstrably buries
/// the answer to the single most likely query on this dataset: "York"
/// is a substring of ~40 Yorkshire station names, so the unranked query
/// returned Bentley (South Yorkshire), Bramley (West Yorkshire),
/// Chapeltown (South Yorkshire) and Clapham (North Yorkshire) above the
/// 20-row cap while York itself was never visible
/// (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F1).
///
/// This overrides the autocomplete spec's Non-goal that "plain substring
/// `ILIKE` on code or name is enough for a list this size"
/// (docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md:23-24).
/// The substring matching *is* still enough -- the WHERE clause is
/// unchanged and nothing that matched before stops matching -- but the
/// ordering was not. Still no `pg_trgm`, still no index: the CASE is
/// evaluated on rows the existing sequential scan already produced.
///
/// Two callers depend on this ordering for correctness, not just display:
/// `StationSearchForm`'s "Look up" button navigates to `suggestions[0]`
/// when the typed text isn't an exact match (frontend/app/stations/
/// StationSearchForm.tsx:27), and `getStationName` filters this response
/// for an exact code match (frontend/lib/api.ts:115-121) -- which the
/// 20-row cap could truncate out of the window for a code whose letters
/// are a common name substring (WAT also matches Blackwater, Bridgwater,
/// Waterbeach, Watford Junction...). Exact-code-first makes that row
/// always row 1, so it can never be capped away.
///
/// `q` must already be trimmed and non-empty (callers go through
/// `routes::reference::sanitize_query` first).
pub async fn search_stations(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let contains = format!("%{q}%");
    let prefix = format!("{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT crs AS code, name FROM stations \
         WHERE crs ILIKE $1 OR name ILIKE $1 \
         ORDER BY \
           CASE \
             WHEN crs ILIKE $2 THEN 0 \
             WHEN name ILIKE $3 THEN 1 \
             ELSE 2 \
           END, \
           name \
         LIMIT $4",
    )
    .bind(&contains)
    .bind(q)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

Note `.bind(q)` for `$2` — the bare query, no wildcards, so `ILIKE` is an
exact case-insensitive comparison. Do **not** write `crs = UPPER($2)`:
`ILIKE` keeps both operands on the same operator the `WHERE` already uses
against this `CHAR(3)` column.

- [ ] **Step 2: Apply the identical treatment to `search_tocs`**

`crates/api/src/data/reference.rs:40-52`, substituting `atoc_code` for
`crs`. Add a short comment pointing at `search_stations`'s reasoning rather
than repeating it, and stating why the sibling is included: the two
functions are the same query shape over the same kind of table, and leaving
one unranked would make the operator field rank "SW" below whatever sorts
first alphabetically among ~30 names.

- [ ] **Step 3: Add the DB-backed ranking test**

Append a `db_tests` module to `crates/api/src/data/reference.rs`, following
`crates/api/src/data/custom_lines.rs:321-380`'s pattern exactly:
`#[tokio::test]` + `#[ignore = "requires a live database; …"]`, read
`DATABASE_URL`, connect with `PgPoolOptions`, seed, assert, delete.

The fixtures deliberately use a reserved `Z…` CRS namespace and invented
names rather than real Yorkshire data, so the test cannot be perturbed by
whatever reference data a developer's database happens to hold, and so
cleanup cannot delete real rows. Write that reason into the module comment,
along with the real-world case each fixture stands in for.

Seed these seven rows into `stations`:

| `crs` | `name` | Tier for the query `"zor"` |
|---|---|---|
| `ZOR` | `Somewhere Else` | 0 — exact code (name does not contain "zor") |
| `ZBU` | `Zorbury` | 1 — name prefix |
| `ZAA` | `Zork` | 1 — name prefix |
| `ZRK` | `Zorkton Parkway` | 1 — name prefix |
| `ZZR` | `Ashby-de-la-Zork` | 2 — substring only |
| `ZBY` | `Bentley (South Zorkshire)` | 2 — substring only |
| `ZBL` | `Bramley (West Zorkshire)` | 2 — substring only |

One lowercase query, `"zor"`, exercises all three tiers plus
case-insensitivity in a single call. Assert the returned `code` sequence is
**exactly** `["ZOR", "ZBU", "ZAA", "ZRK", "ZZR", "ZBY", "ZBL"]` — full
sequence equality, not "York is in the list", because the defect being
fixed is ordering. Note in a comment that `ZBU`/`ZAA`/`ZRK` land in that
order because alphabetical-within-tier puts the shortest prefix-match
first, which is why no separate exact-name tier is needed (Decision 1).

Delete the seven rows by `crs` at the end, unconditionally.

- [ ] **Step 4: Add the truncation regression test**

A second DB-backed test proving the fix that matters to `getStationName`
(see Decision 1 and correction 3). Seed the `ZOR` / `Somewhere Else` row
plus **22 filler rows** whose names all contain `zor` and all sort
alphabetically before `Somewhere Else` (e.g. `A-Zor Filler 01` …
`A-Zor Filler 22`, with codes `Y01`…`Y22`).

Call `search_stations(&pool, "zor", 20)` — the real
`SUGGESTION_LIMIT`. Assert `rows[0].code == "ZOR"`.

Add the comment explaining the failure this guards: under `ORDER BY name`
all 22 fillers sort ahead of `Somewhere Else`, so the exact-code row falls
outside `LIMIT 20` entirely and `getStationName` returns `null`, making the
UI fall back to a bare code — which is the very thing Tasks 8–9 exist to
remove. Under the tiered ordering the exact-code row is row 1 and cannot be
capped away.

Delete all 23 rows at the end.

- [ ] **Step 5: Add a `search_tocs` ranking test**

One DB-backed test in the same module, seeding three `tocs` rows with a
`Z`-namespaced `atoc_code` (the table's PK is `CHAR(2)`, and `legal_name`
is `NOT NULL`, so supply it) shaped to prove exact-code-first, and
asserting the full code sequence. Clean up.

- [ ] **Step 6: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api reference -- --ignored --test-threads=1
```

Expected: all PASS. The `--ignored` run is real CI coverage — CI runs
exactly this (`.github/workflows/ci.yml:216`) against a freshly migrated,
empty database, which is why these tests seed their own rows.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/reference.rs
git commit -m "Rank station/operator search: exact code, then name prefix, then substring"
```

---

### Task 2: Restore the Pin column at mobile width (F4) — **frontend only**

**Files:**
- Modify: `frontend/app/lines/AllLinesTable.tsx`,
  `frontend/app/lines/AllLinesTable.test.tsx`

Independent of every other task. Two prop deletions and one inverted test.

- [ ] **Step 1: Delete the two `visibleFrom="sm"` props**

`frontend/app/lines/AllLinesTable.tsx:210` — `<TableTh visibleFrom="sm">Pin</TableTh>`
→ `<TableTh>Pin</TableTh>`.

`frontend/app/lines/AllLinesTable.tsx:260` — `<TableTd visibleFrom="sm">`
(the one wrapping `<PinToggle>` at `:261`) → `<TableTd>`.

Leave the `visibleFrom="sm"` props on the Avg Delay and Cancelled cells
(`:198`, `:204`, `:230`, `:245`) exactly as they are — those columns *are*
re-surfaced in the mobile sub-line at `:225-227`, which is what makes
hiding them correct.

Add a comment above the Pin `<TableTh>` recording why it is deliberately
not treated like its neighbours:

```tsx
{/* No `visibleFrom="sm"`, unlike the two numeric columns beside it.
    Those are hidden on mobile only because they are re-surfaced in the
    `hiddenFrom="sm"` sub-line under the line name (:225) -- Pin got the
    hiding half of that pattern without the re-surfacing half in
    bd4d739, and unlike a number it is an interactive control with no
    other home: `PinToggle` exists in exactly two places in this app,
    here and on the station detail page (`kind="station"`), so below the
    sm breakpoint there was no way to pin or unpin a LINE anywhere in
    the application, and a pinned row was visually identical to an
    unpinned one. See
    docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F4. */}
```

- [ ] **Step 2: Invert the test that locks in the bug**

`frontend/app/lines/AllLinesTable.test.tsx:352-357` is
`it('hides the Pin column below the sm breakpoint too', …)`. Replace it
with the positive assertion, keeping the same
`.mantine-visible-from-sm`-class technique the neighbouring tests use
(jsdom has no layout, and `vitest.setup.ts:13-27` stubs `matchMedia` to
always return `matches: false`, so class presence is the only responsive
signal available):

```tsx
it('keeps the Pin column at every width, unlike the numeric columns', () => {
  const { container } = renderMobileTable();
  const hidden = Array.from(container.querySelectorAll('.mantine-visible-from-sm'));
  // The numeric columns above this test are legitimately hidden below sm
  // because they reappear in the sub-line; Pin has no such fallback, so it
  // must never carry the class. Asserted on the class rather than on
  // visibility because jsdom has no layout and vitest.setup.ts stubs
  // matchMedia to `matches: false`.
  expect(hidden.map((el) => el.textContent).some((t) => t?.includes('Pin'))).toBe(false);
});

it('renders a usable pin control in every row at mobile width', () => {
  renderMobileTable();
  expect(screen.getByRole('button', { name: 'Pin (currently not pinned)' })).toBeInTheDocument();
});
```

The second assertion's name string comes from `PinToggle.tsx:109` — it is
the real `aria-label`, so this test also fails if the star stops being a
button.

- [ ] **Step 3: Add a pinned-state test**

Render the same table with `pinnedLineIds={['northern']}` and assert
`getByRole('button', { name: 'Unpin (currently pinned)' })`. This covers
the second half of F4 that the review flagged separately — that at mobile
width an already-pinned row was indistinguishable from an unpinned one.

- [ ] **Step 4: Verify the three-column layout actually fits at 390px**

Run the app and view `/lines` at 390px. Expected: Name (with its numeric
sub-line), Status and Pin all on one row, no horizontal scroll, no
truncated status label. If it does not fit, **stop** — the fallback is the
review's own second option (pin behind a row-tap action sheet), which is a
different and much larger task, and needs its own plan.

- [ ] **Step 5: Test and build**

Run (from `frontend/`): `npm ci && npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "Keep the Pin column at mobile width on All Lines (reverts bd4d739's consistency argument)"
```

---

### Task 3: Theme the links inside sanitized incident HTML (F7) — **frontend only**

**Files:**
- Modify: `frontend/components/DisruptionDetail.tsx`,
  `frontend/components/DisruptionDetail.test.tsx`,
  `frontend/app/incidents/[id]/page.tsx`, `frontend/app/globals.css`,
  `frontend/app/globals.test.ts`

Independent of every other task.

- [ ] **Step 1: Confirm Mantine does not already style bare `<a>`**

`node_modules` is not installed in this worktree.

```bash
cd frontend && npm ci
grep -n "^a,\|^a{\|^a \|:where(a)" node_modules/@mantine/core/styles.css | head -20
```

Expected: no rule setting `color` on a bare `a` selector. The finding's
premise is that these anchors fall through to the browser default blue; if
Mantine *does* reset them, re-read the finding before writing the rule (the
fix is the same, but the "what it looks like today" wording changes).

- [ ] **Step 2: Add the `data-rich-text` hook to both injection sites**

`frontend/components/DisruptionDetail.tsx:20`:

```tsx
{/* `data-rich-text`: the CSS hook for `app/globals.css`'s
    `[data-rich-text] a` rule. Anchors inside knowledgebase incident
    copy arrive as external HTML, so they carry no Mantine class and
    (per `lib/sanitizeHtml.ts`'s `ALLOWED_ATTR = ['href']`) no class or
    data attribute of their own -- they were rendering browser-default
    blue next to blue "PLANNED WORK" badges, the exact collision the
    grape theme was created to eliminate
    (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F7).
    A descendant selector from this container is the only way to reach
    them, and this is the same data-attribute pattern `data-text-link`
    and `data-status-badge` already use. */}
<div data-rich-text dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }} />
```

`frontend/app/incidents/[id]/page.tsx:61` — the same attribute, with a
one-line comment pointing at `DisruptionDetail.tsx`'s block rather than
repeating it. (Note the blue `Badge` three lines above at `:58` is *also*
touched by the accessibility plan's Task 3/Decision 3 — do not change it
here.)

- [ ] **Step 3: Add the rule to `globals.css`**

Append a flat rule (the file's own style is flat selectors throughout;
`postcss-preset-mantine` would accept nesting, but nothing else in the file
nests). Put it next to the other link rules, after
`a[data-text-link='always']` at `:897-903`:

```css
/* Links inside sanitized incident HTML. These anchors come from external
   Knowledgebase copy, not from Mantine's `Anchor` or this app's
   `TextLink`, so nothing had ever set their colour: they rendered
   browser-default blue while every app-chrome link beside them rendered
   theme grape -- and blue is the colour this theme reserved for "planned
   closure" badges. `--mantine-color-anchor` is deliberately the token
   used rather than a grape shade: it is grape 7 in the light scheme (the
   override at the top of this file) and Mantine's own grape 4 in dark, so
   one rule is correct in both schemes and no dark-scheme selector is
   needed here (which `app/globals.test.ts` forbids anyway).

   Colour only -- no `text-decoration`. Unlike `a[data-text-link]` above,
   which drops the underline because those links sit in chrome and
   headings, these sit mid-paragraph in prose, where colour alone is not a
   sufficient distinguisher from surrounding body text (WCAG 1.4.1). */
[data-rich-text] a {
  color: var(--mantine-color-anchor);
}
```

- [ ] **Step 4: Update the now-stale comment at `globals.css:11-13`**

It currently says *"Nothing but Mantine's `Anchor` and the
`c="var(--mantine-color-anchor)"` call sites read this variable, so the
change is contained to links"*. Add the third reader: the
`[data-rich-text] a` rule below.

- [ ] **Step 5: Add the CSS assertions**

In `frontend/app/globals.test.ts`, matching that file's string-matching
style (`css.match(/…/)` then `expect(rule![0]).toContain(…)`):

```ts
describe('links inside sanitized incident HTML', () => {
  const rule = css.match(/\[data-rich-text\]\s+a\s*\{[^}]*\}/);

  it('themes in-content anchors with the shared anchor colour', () => {
    expect(rule).not.toBeNull();
    // The token, not a grape shade: it resolves to grape 7 in light (the
    // override at the top of globals.css) and Mantine's grape 4 in dark,
    // so both schemes are correct from one rule. The two contrast facts
    // this relies on are already asserted above -- grape 7 on white at
    // 4.85:1 and grape 4 on #242424 at 5.84:1 -- and are deliberately not
    // restated here.
    expect(rule![0]).toContain('color: var(--mantine-color-anchor)');
  });

  it('leaves the underline in place, unlike the chrome link treatment', () => {
    // WCAG 1.4.1: these anchors sit mid-paragraph in prose, so colour
    // alone cannot be what distinguishes them from the body text around
    // them. `a[data-text-link]` above deliberately does remove the
    // underline; copying that here would be the wrong tidy-up.
    expect(rule![0]).not.toContain('text-decoration');
  });
});
```

- [ ] **Step 6: Add the component assertion**

In `frontend/components/DisruptionDetail.test.tsx`, assert the injected
container carries the hook — the CSS rule above is inert without it, and
these two files can drift independently:

```tsx
it('marks the sanitized-HTML container so globals.css can theme its links', () => {
  const { container } = renderWithMantine(<DisruptionDetail disruption={/* existing fixture */} />);
  expect(container.querySelector('[data-rich-text]')).not.toBeNull();
});
```

Reuse whatever fixture that file already defines; do not invent a new one.

- [ ] **Step 7: Verify visually in both schemes**

Load `/incidents/[a real id]` with a link-dense incident, in light and dark.
Expected: body links render grape (light) / light grape (dark), the
"Planned Work" badge is the only blue on the page, and the links are still
underlined.

- [ ] **Step 8: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, including the pre-existing `globals.test.ts:50`
(no dark-scheme selector) and `:113` (no hardcoded hex).

- [ ] **Step 9: Commit**

```bash
git add frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx \
        "frontend/app/incidents/[id]/page.tsx" frontend/app/globals.css frontend/app/globals.test.ts
git commit -m "Theme links inside sanitized incident HTML with the app's anchor colour"
```

---

### Task 4: Stop the error boundary printing its input (F5b) — **frontend only**

**Files:**
- Modify: `frontend/app/error.tsx`
- Create: `frontend/app/error.test.tsx`

Independent of every other task. **Collides with the accessibility plan's
Task 3** on this same file — see the interaction table.

- [ ] **Step 1: Rewrite `frontend/app/error.tsx`**

Four changes, each with its own reason:

1. **Drop `error.message`.** It is what rendered "Minified React error
   #130; visit https://react.dev/errors/130?args[]=…" to a user.
2. **Log it instead.** A `useEffect` calling `console.error` with both
   `error` and `error.digest`. This app has no error-reporting service, so
   the browser console is the only channel; today the value is discarded
   entirely.
3. **Show `error.digest` as a short reference, when present.** This is not
   a contradiction of the finding: `digest` is Next's deliberately opaque
   correlation hash, produced for exactly this purpose, and it is the
   review's own prescription for 5c's ID ("formatted as a short
   reference"). It is only populated for server-side errors, hence the
   conditional.
4. **Generalise the title and add a way out.** "Couldn't load status data"
   is wrong on `/track`, `/chat` and everywhere else, because this is the
   app's *only* error boundary (no `global-error.tsx`, no per-route
   `error.tsx` — `app/layout.tsx:59-62` already notes the gap). And `reset`
   alone is a dead end for a render error, which will re-throw; the five
   `not-found.tsx` templates all offer a link, and this should match them.

```tsx
'use client';

import { useEffect } from 'react';
import { Button, Group, Stack, Text, Title } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

/** The app's ONLY error boundary -- there is no `global-error.tsx` and no
 * per-route `error.tsx` (see `app/layout.tsx:59-62`, which works around
 * that gap with `.catch()` fallbacks rather than adding one). So its copy
 * has to work on every route, which is why the heading is no longer
 * "Couldn't load status data".
 *
 * `error.message` is deliberately NOT rendered. It used to be, and on the
 * `/connect-claude` crash it printed "Minified React error #130; visit
 * https://react.dev/errors/130?args[]=..." as the page's body copy
 * (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5). The
 * message goes to the console instead; `digest` -- Next's opaque
 * server-error correlation hash, which exists precisely to be quoted by a
 * user -- is what gets shown, and only when there is one (client-side
 * render errors have none). */
export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    // The only reporting channel this app has. Previously nothing logged
    // the error at all, while the useless half of it was rendered.
    console.error('Unhandled error rendering a page', { digest: error.digest, error });
  }, [error]);

  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Something went wrong</Title>
      <Text c="dimmed">
        This page couldn&apos;t be loaded. It may be a temporary problem with the live data
        feeds — try again in a moment.
      </Text>
      <Group>
        <Button onClick={reset} w="fit-content">
          Try again
        </Button>
        <TextLink href="/">Back to your dashboard</TextLink>
      </Group>
      {error.digest && (
        <Text size="xs" c="dimmed">
          Reference: {error.digest}
        </Text>
      )}
    </Stack>
  );
}
```

**If the accessibility plan has already landed**, keep its
`<Title order={1} size="h2">` here instead of `order={2}` — do not revert
it.

- [ ] **Step 2: Create `frontend/app/error.test.tsx`**

None exists today, which is conspicuous given every sibling under `app/`
has one. Assert the four properties that matter, using
`renderWithMantine` and the `next/navigation` stub pattern from
`app/page.test.tsx:12-15` (`TextLink` is a client component):

```tsx
it('never renders the raw error message', () => {
  renderWithMantine(<Error error={Object.assign(new Error('Minified React error #130'), { digest: 'abc123' })} reset={() => {}} />);
  expect(screen.queryByText(/Minified React error/)).not.toBeInTheDocument();
});

it('logs the error instead of showing it', () => {
  const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
  const error = Object.assign(new Error('boom'), { digest: 'abc123' });
  renderWithMantine(<Error error={error} reset={() => {}} />);
  expect(spy).toHaveBeenCalledWith('Unhandled error rendering a page', { digest: 'abc123', error });
  spy.mockRestore();
});

it('shows the digest as a quotable reference when there is one', () => { /* 'Reference: abc123' */ });
it('omits the reference line entirely for a client-side error with no digest', () => { /* … */ });
it('offers a route out, not just a retry that will re-throw', () => { /* link to '/' */ });
```

- [ ] **Step 3: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/error.tsx frontend/app/error.test.tsx
git commit -m "Stop the error page rendering error.message; log it and show the digest instead"
```

---

### Task 5: Write the tracking validation messages for humans (F5a) — **backend + a frontend test**

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`,
  `frontend/components/TrackTrainForm.test.tsx`,
  `frontend/components/TicketEntryForm.test.tsx`

Independent of every other task.

- [ ] **Step 1: Rewrite `validate_pin`'s three messages**

`crates/api/src/data/train_tracking.rs:40-51`. These are rendered verbatim
to users as an `Alert` body (`TrackTrainForm.tsx:122-125`, `:186-190`)
because the endpoint returns `(StatusCode::BAD_REQUEST, String)` as plain
text (`routes/train.rs:367`) and there is no error envelope in this API.
Fix the copy at the source; do not introduce a mapping layer keyed on the
strings being replaced.

- `:42` → `"Enter the station you're departing from."`
- `:45` → `"That doesn't look like a station code — CRS codes are three letters, like WOK or EUS."`
  (deliberately echoing `app/stations/[crs]/not-found.tsx`'s copy, which
  the review credits in §C1 as teaching the format well.)
- `:48` → **interpolated from `MAX_PIN_AGE`, not typed as prose**:
  ```rust
  return Err(format!(
      "That departure time is more than {} hours ago — trains can only be tracked \
       within {} hours of departure.",
      MAX_PIN_AGE.num_hours(),
      MAX_PIN_AGE.num_hours(),
  ));
  ```
  so the message can never drift from the constant at `:23`.

Add a module-level comment above `validate_pin` recording that these
strings are user-facing copy, not developer diagnostics, and why: the 400
body is rendered verbatim by the form, so a snake_case field name here
becomes a snake_case field name on screen
(`docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md` §F5).

- [ ] **Step 2: Fix `validate_ticket_entry:100` — the `Debug`-formatted array**

`crates/api/src/data/train_tracking.rs:100` currently does
`format!("source must be one of {TICKET_SOURCES:?}")`, which renders
`source must be one of ["manual", "pkpass-semantics", "pkpass-heuristic",
"pdf-heuristic"]` — a Rust `Debug` of a `[&str; 4]` — into user copy at
`TicketEntryForm.tsx:178` and `:240`.

The review did not find this one; it is the same defect class, in the same
file, one function down (see correction 6). Replace with a plain sentence
that names no internals: this path is unreachable from the app's own form
(which supplies `source` itself), so listing the valid values buys a
direct-API caller nothing a 400 doesn't already tell them.

Review the rest of `validate_ticket_entry` (`:99-113`) for the same defect
and fix any sibling with a field name or a `{:?}` in it, using the same
posture.

- [ ] **Step 3: Update the frontend tests that pin the old strings**

`frontend/components/TrackTrainForm.test.tsx:122` and `:131` currently
assert the raw string is displayed. They are testing the *pass-through*,
which is still the correct behaviour — only the string changes. Update both
to the new copy, and add a one-line comment naming
`crates/api/src/data/train_tracking.rs:validate_pin` as the source of
truth, so a future reader knows where to change it.

Do the same for any assertion in `TicketEntryForm.test.tsx` carrying the
`{:?}` array string.

- [ ] **Step 4: Add or extend the Rust unit tests**

`validate_pin` is a pure function with no DB dependency, so this needs no
`db_test`. Assert each branch returns a message that (a) is non-empty and
(b) contains no `_` character — a cheap, durable guard against a future
field name creeping back into user copy:

```rust
#[test]
fn validation_messages_carry_no_internal_field_names() {
    // The 400 body is rendered verbatim as the form's error Alert
    // (frontend/components/TrackTrainForm.tsx:186-190), so a snake_case
    // field name here lands on screen. See the review's §F5.
    for message in [/* each error branch's message */] {
        assert!(!message.contains('_'), "user-facing copy leaked an identifier: {message}");
    }
}
```

Extend whatever `validate_pin` tests already exist rather than duplicating
their setup.

- [ ] **Step 5: Test, lint, build**

```bash
cargo fmt --all && cargo clippy --workspace --all-features && cargo test -p api
cd frontend && npm test && npm run build
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/train_tracking.rs \
        frontend/components/TrackTrainForm.test.tsx frontend/components/TicketEntryForm.test.tsx
git commit -m "Write the train-tracking and ticket validation errors as human copy"
```

---

### Task 6: Label the incident source instead of dumping its ID (F5c) — **frontend only**

**Files:**
- Create: `frontend/lib/incidentSource.ts`, `frontend/lib/incidentSource.test.ts`
- Modify: `frontend/components/DisruptionDetail.tsx`,
  `frontend/components/DisruptionDetail.test.tsx`

Independent of every other task.

- [ ] **Step 1: Add the label map**

`frontend/lib/incidentSource.ts`, modelled directly on
`frontend/lib/impactType.ts:7-24` — including its documented fail-safe
posture ("render nothing rather than a raw snake_case string"):

```ts
/** Maps an incident's `source` provenance string to a human label.
 *
 * `source` is not a free string: it is one of three prefixed shapes
 * produced by the pipeline -- `knowledgebase-incident-{id}`
 * (crates/aggregator/src/aggregation.rs:156), `ldbws-sampling`
 * (aggregation.rs:959) and `tfl-line-status-{lineId}`
 * (crates/poller-tfl/src/schema.rs:143). The prefix already IS a source
 * enum; nothing had ever mapped it, so `DisruptionDetail` rendered the
 * whole thing, producing "Source:
 * knowledgebase-incident-EC354602568440DB82B2835903B7A5FE" in body copy
 * (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5).
 *
 * Returns `null` for anything unrecognised, so a new pipeline source
 * renders nothing rather than a raw internal string -- the same fail-safe
 * `lib/impactType.ts` documents. */
export function incidentSourceLabel(source: string | null): string | null { … }
```

Three prefix branches plus a `null` default. Keep it a pure function with
no React import so it is trivially unit-testable.

- [ ] **Step 2: Render the label, keep the ID reachable**

`frontend/components/DisruptionDetail.tsx:35-39`:

```tsx
{incidentSourceLabel(disruption.source) && (
  // `title` keeps the raw provenance string one hover away for debugging
  // without putting a 32-hex ID in body copy -- the same tactic
  // CustomLineForm.tsx:152 uses for its code/name pills. Not an
  // InfoIcon+Tooltip (components/InfoIcon.tsx): that's a heavier control
  // than a value no user needs to read deserves. Note the incident id
  // this string carries is ALREADY surfaced usefully on the next line, as
  // the "View full incident details" link (:40-44, via
  // lib/incidents.ts:11-14) -- so nothing is lost by not printing it.
  <Text size="xs" c="dimmed" title={disruption.source ?? undefined}>
    Source: {incidentSourceLabel(disruption.source)}
  </Text>
)}
```

- [ ] **Step 3: Tests**

`frontend/lib/incidentSource.test.ts` — one case per known prefix
(including a real-shaped 32-hex knowledgebase id), plus `null` input, plus
an unrecognised string returning `null`.

`frontend/components/DisruptionDetail.test.tsx:42` currently asserts the
raw string renders. Replace it with two assertions: the label text is
present, and `queryByText(/knowledgebase-incident-/)` is **absent** — the
second is the actual regression guard for this finding.

- [ ] **Step 4: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/incidentSource.ts frontend/lib/incidentSource.test.ts \
        frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx
git commit -m "Label incident provenance instead of printing its internal source id"
```

---

### Task 7: Show "Right now" to logged-in users with no pinned lines (F2) — **frontend only**

**Files:**
- Modify: `frontend/app/page.tsx`, `frontend/app/page.test.tsx`

Independent of Tasks 1–6. **Must land before Task 9** (same file).

- [ ] **Step 1: Hoist the `notGoodServiceSummary` call above the branch**

`frontend/app/page.tsx:104` currently calls it as the first statement
inside the anonymous branch. Move the call to just after the `Promise.all`
at `:95-101`, so both branches read one value.

Add a comment recording that this costs nothing: it is a pure function
(`:59-71`) of `allReports`, which `:99` already fetches on every load
regardless of auth state.

- [ ] **Step 2: Extract the module's JSX into a local component**

The "Right now" markup is currently inline at `:122-145` inside the
anonymous branch's `return`. Lift it to a local
`function RightNowModule({ summary }: { summary: ReturnType<typeof notGoodServiceSummary> })`
in the same file, beside `TrackedTrainSummaryRow` (`:279-307`), and call it
from the anonymous branch.

**Local, not a new file under `components/`** — two call sites, one file,
both server-rendered (Decision 2). Move the markup verbatim; this step must
be a pure extraction, and a screenshot of the logged-out home before and
after must be identical.

- [ ] **Step 3: Render it for pinless logged-in users**

In the authenticated branch's returned JSX, after the "Your Stations"
section (which ends at `:251`) and before "Your Tracked Trains" (`:253`):

```tsx
{/* The anonymous home gives a visitor a genuinely useful live-status
    module; logging in used to REMOVE it, so a user's reward for the
    single action this app most wants them to take was a blank page with
    two "you haven't pinned anything" lines
    (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F2).
    `2026-08-31-anonymous-user-ux-design.md` called that case "arguably
    fine"; the rendered pages settled the argument the other way, and this
    deliberately overrides that spec decision.

    Gated on pinned LINES only, not on pins of any kind: a user with
    pinned stations but no pinned lines still has a line-shaped hole here,
    and this is a lines module. Costs nothing -- `allReports` is fetched
    unconditionally above and `notGoodServiceSummary` is pure. */}
{pinnedLineReports.length === 0 && <RightNowModule summary={rightNow} />}
```

Confirm `RightNowModule`'s heading renders at `order={2}`, so the
authenticated branch reads h1 "Your Lines" → h2 "Your Stations" → h2
"Right now" → h2 "Your Tracked Trains" with no skip.

- [ ] **Step 4: Tests**

`frontend/app/page.test.tsx` already has the mocking scaffolding
(`vi.mock('@/lib/api')` at `:8`, the `next/navigation` stub at `:12-15`,
and the logged-in `beforeEach` at `:125-129`). Add to the logged-in
describe block:

```tsx
it('shows the live "Right now" module to a logged-in user with no pinned lines', async () => { … });
it('still shows it when they have pinned stations but no pinned lines', async () => { … });
it('hides it once they pin a line', async () => { … });
it('renders the module at heading level 2 in the authenticated branch', async () => {
  // h1 "Your Lines" -> h2 "Your Stations" -> h2 "Right now" -> h2 "Your Tracked Trains".
  expect(screen.getByRole('heading', { name: /Right now/, level: 2 })).toBeInTheDocument();
});
```

**Check `page.test.tsx:98` still passes** — it asserts `/Right now/` is
absent for a logged-in user, but that test pins `['central']`, so under
this rule it should be unaffected. If it fails, the gate has been written
wrong. Add a comment there noting the assertion is now load-bearing for the
*pinned* case specifically.

Also add an anonymous-branch regression: the module still renders
identically for a logged-out visitor after the extraction.

- [ ] **Step 5: Verify visually**

Log in with an account that has no pins. Expected: the two empty-state
one-liners, then the live "Right now" list. Log out: the anonymous home is
pixel-identical to before this task.

- [ ] **Step 6: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/page.tsx frontend/app/page.test.tsx
git commit -m "Show the 'Right now' module to logged-in users with no pinned lines"
```

---

### Task 8: Put station names on the tracked-train and ticket read models (F3a) — **backend**

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`

Independent of Tasks 1–7. **Task 9 depends on this.** Per Decision 3 this
is a `LEFT JOIN` on each of four queries, not a frontend lookup.

- [ ] **Step 1: `TrackedTrainState`**

Add `pub pin_origin_name: Option<String>` and
`pub pin_destination_name: Option<String>` to the struct
(`crates/api/src/data/train_tracking.rs:307-320`), and join in
`TRACKED_TRAIN_STATE_SELECT` (`:322-331`):

```sql
SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs,
       so.name AS pin_origin_name, sd.name AS pin_destination_name,
       tt.resolution_status, tt.train_uid, tt.train_id,
       cs.status, cs.last_reported_location, cs.last_event_type,
       cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source
FROM tracked_trains tt
LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id
LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs)
LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs)
```

**`UPPER(...)` is mandatory, not defensive tidiness.** `pin_origin_crs` is
`TEXT` (`migrations/20260828120000_train_tracking.sql:61`) and
`validate_pin` never normalises its case, while `stations.crs` is
`CHAR(3)`. Without `UPPER`, a user who typed `kgx` gets `NULL` and falls
back to the bare code — the exact outcome this task removes. Record that in
a comment on the join.

`LEFT JOIN`, never `JOIN`: a CRS with no reference row (a code the
stations feed doesn't carry) must still return the train.

- [ ] **Step 2: `TrackedTrainListItem`**

Same two fields on the struct (`:344-357`) and the same two joins in
`list_tracked_trains_for_user`'s query (`:374-383`). This is the one that
feeds the home dashboard, `/track/mine`, and `AttachTicketAction`'s
`Select` — i.e. three of F3's six sites.

- [ ] **Step 3: `TrackedTrainTicket`**

Add `pub origin_name: Option<String>` / `pub destination_name:
Option<String>` (`:592-601`) and two joins to `TICKET_SELECT` (`:603-606`)
on the ticket's **own** `origin_crs`/`destination_crs`. Note
`TICKET_SELECT` is `format!`ed into two callers
(`list_tickets_for_tracked_train`, `get_ticket_owned`), so the joins belong
in the shared constant and both callers get them; check the `WHERE` clauses
appended by each still work with table aliases (they reference unqualified
column names today, so aliasing the base table means qualifying them).

- [ ] **Step 4: `TicketListItem` / `TicketListRow`**

Add the same two fields to both (`:695-713`, `:741-762`), carry them
through `build_ticket_list_item` (`:768-802`), and add two joins to
`list_tickets_for_user`'s query (`:826-838`) on `t.origin_crs` /
`t.destination_crs`.

Only the ticket's own origin/destination are needed here — the pin route
these rows also carry is rendered from `TrackedTrainListItem`, which Step 2
already covers, so do **not** add two more joins for the pin CRS. Say so in
a comment; the omission looks like an oversight otherwise.

Update `ticket_list_tests`'s `row()` fixture (`:848-870`) with the two new
fields.

- [ ] **Step 5: Add the DB-backed round-trip test**

One `#[ignore]`d DB test in the house pattern
(`crates/api/src/data/custom_lines.rs:321-380`), proving the join actually
resolves and — critically — that it resolves for a **lower-case** stored
CRS, which is the case that silently breaks without `UPPER`:

- seed a fixture user, a `stations` row (`ZQQ` / `Zedbury`), and two
  `tracked_trains` rows, one with `pin_origin_crs = 'ZQQ'` and one with
  `pin_origin_crs = 'zqq'`;
- call `list_tracked_trains_for_user` and assert **both** rows come back
  with `pin_origin_name == Some("Zedbury")`;
- seed a third with a CRS that has no `stations` row and assert
  `pin_origin_name == None` (the `LEFT JOIN`, not `JOIN`, guarantee);
- delete everything.

- [ ] **Step 6: Test, lint, build**

```bash
cargo fmt --all && cargo clippy --workspace --all-features && cargo test -p api
DATABASE_URL=<url> cargo test -p api train_tracking -- --ignored --test-threads=1
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Return station names alongside CRS codes on tracked-train and ticket rows"
```

---

### Task 9: Render station names instead of bare codes (F3b) — **frontend only**

**Files:**
- Create: `frontend/lib/stationLabel.ts`, `frontend/lib/stationLabel.test.ts`
- Modify: `frontend/lib/types.ts`, `frontend/app/page.tsx`,
  `frontend/app/page.test.tsx`, `frontend/app/track/mine/page.tsx`,
  `frontend/components/AttachTicketAction.tsx`,
  `frontend/components/TicketSummary.tsx`,
  `frontend/components/TrainJourney.tsx`,
  `frontend/app/lines/CustomLineForm.tsx`, plus the colocated test files

**Depends on Task 8** (the fields must exist) **and should follow Task 7**
(same file, `frontend/app/page.tsx`).

- [ ] **Step 1: Add the shared formatter**

`frontend/lib/stationLabel.ts` — one place for the format and, more
importantly, one place for the fallback:

```ts
/** `"London Kings Cross (KGX)"`, or the bare code when no name resolved.
 *
 * `Name (CRS)` rather than name-only: this is already what
 * `app/stations/[crs]/page.tsx:55` renders as its heading and what
 * `app/page.tsx:236` renders for pinned stations, and the code is what a
 * reader cross-references against a ticket or a departure board. See
 * docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F3.
 *
 * `name` is `null` whenever the backend's `LEFT JOIN stations` found no
 * reference row for the code, so every caller needs this fallback and
 * none of them should hand-roll it. */
export function stationLabel(crs: string, name: string | null | undefined): string {
  return name ? `${name} (${crs})` : crs;
}

/** `"A (AAA) → B (BBB)"`, or just the origin when there is no destination
 * (a pre-match pin genuinely has none -- see
 * `2026-09-01-tracked-trains-home-page-design.md` Decision 1). */
export function routeLabel(
  originCrs: string, originName: string | null | undefined,
  destinationCrs: string | null | undefined, destinationName: string | null | undefined,
): string { … }
```

Unit-test both, including every `null` combination.

- [ ] **Step 2: Widen the TypeScript types**

`frontend/lib/types.ts` — add the fields Task 8 now returns:
`pinOriginName` / `pinDestinationName` on `TrackedTrainState` (`:238-239`'s
neighbourhood) and `TrackedTrainListItem` (`:261-262`'s), and
`originName` / `destinationName` on `TrackedTrainTicket` (`:316-317`'s) and
`TicketListItem` (`:427-428`'s). All `string | null`.

- [ ] **Step 3: Replace the six render sites**

| File:line | Change |
|---|---|
| `frontend/app/page.tsx:290` | build `route` via `routeLabel(...)` |
| `frontend/app/track/mine/page.tsx:132` | same |
| `frontend/components/AttachTicketAction.tsx:66-68` | `Select` option label via `routeLabel(...)`, keeping the ` (${formatDate(...)})` suffix |
| `frontend/components/TicketSummary.tsx:41` | `routeLabel(...)`, preserving the existing `'?'` fallback for a ticket with neither CRS |
| `frontend/components/TrainJourney.tsx:20-21` | `routeLabel(...)` in `pinSummary` (rendered in all five state branches) |
| `frontend/app/lines/CustomLineForm.tsx:193-197` | `title={nameByCode[crs]}` on the station `Badge` |

`TicketSummary`'s prop is a `Pick<...>` of six fields (`:35-38`) — widen it
to eight rather than loosening it to the full type.

**The `CustomLineForm` chip is the one site that needed no backend work**:
`nameByCode` (`:58-67`) already holds the answer and is already used for
`title=` on the operator pill (`:152`) and the destination-CRS pill
(`:213`). The review asks for exactly this ("keep bare codes in the compact
chips on the line form if space demands, but add `title`/tooltip names
there"), so the chip keeps its bare code and gains the tooltip.

- [ ] **Step 4: Update the colocated tests**

Every test file asserting a bare code as a label needs its fixture widened
with the new name fields and its assertion updated. At minimum:
`app/page.test.tsx` (the tracked-trains describe block at `:124-195`),
`app/track/mine/page.test.tsx`, `components/AttachTicketAction.test.tsx`,
`components/TicketSummary.test.tsx`, `components/TrainJourney.test.tsx`,
`app/lines/CustomLineForm.test.tsx`.

In each, add one assertion that survives a future refactor: the **name** is
present, and a **`null` name still renders the bare code** rather than
"null" or an empty label. The second is the regression that matters —
`stationLabel`'s fallback is the whole reason it exists.

- [ ] **Step 5: Verify visually**

The home dashboard's "Your Tracked Trains" rows, `/track/mine`, and
`/train/by-id/[id]`. Expected: "London Kings Cross (KGX) · 2 Sept 2026 ·
16:53" in place of "KGX · 2 Sept 2026 · 16:53", and long route lines
wrapping rather than overflowing at 390px. If a row overflows, adjust the
`Text` wrapping — do **not** fall back to name-only, which would answer the
review's ask differently from the app's two existing call sites.

- [ ] **Step 6: Test and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. `npm run build` matters here specifically — the new
required fields fail type-checking at any fixture that was missed.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/stationLabel.ts frontend/lib/stationLabel.test.ts frontend/lib/types.ts \
        frontend/app/page.tsx frontend/app/page.test.tsx frontend/app/track/mine \
        frontend/components/AttachTicketAction.tsx frontend/components/TicketSummary.tsx \
        frontend/components/TrainJourney.tsx frontend/app/lines/CustomLineForm.tsx
git commit -m "Show station names, not bare CRS codes, on tracked trains and tickets"
```

---

### Task 10: Final verification

**Files:** none — verification only.

- [ ] **Step 1: Full suites**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -p aggregator -- --ignored --test-threads=1
cd frontend && npm test && npm run build
```

Expected: all PASS. (`cargo fmt --all --check` is `continue-on-error` in CI
today per `.github/workflows/ci.yml:118-124` because it is not clean on
`main` — confirm only that *this plan's* files are clean, not the whole
workspace.)

- [ ] **Step 2: Walk the six findings against the running app**

One pass, in review order:

1. **F1** — type "York" on `/stations`: York is the first suggestion. Type
   "Yor" and press "Look up": it navigates to York, not to Bentley (South
   Yorkshire). Type "KGX" on `/lines/new`: unchanged, still correct.
2. **F2** — log in with an account holding no pins: the "Right now" module
   is on the dashboard under the two empty-state lines. Pin a line: it
   disappears. Log out: the anonymous home is unchanged.
3. **F3** — the dashboard's tracked-train rows, `/track/mine` and
   `/train/by-id` read "Name (CRS)". Hover a station chip on `/lines/new`:
   the tooltip names the station.
4. **F4** — `/lines` at 390px: a star in every row, filled and yellow on
   the pinned one, and tapping it pins/unpins.
5. **F5** — submit the Track form with a departure 8 hours ago: the alert
   reads as English with no `scheduled_departure` in it. Expand an
   incident: "Source: National Rail Knowledgebase", no 32-hex ID. (The
   error page needs an induced error to see; the unit tests in Task 4 are
   the real coverage.)
6. **F7** — a link-dense incident in **both** colour schemes: body links
   grape, still underlined, and the "Planned Work" badge is the only blue.

- [ ] **Step 3: Update the two documents this plan makes stale**

- `docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md`
  — its Non-goals (`:23-24`) and its verbatim query (`:45-48`) no longer
  describe the code. Mark the ordering decision superseded, pointing at
  this plan and at the review's §F1. Its *other* Non-goals (no `pg_trgm`,
  no new index, no auth) all still hold — say so, so a later reader doesn't
  assume the whole section fell.
- `docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md` —
  Decision 7's per-route heading table lists `/` (authenticated) as
  h1 → h2 → h2. Task 7 adds a third h2. Update the row.

- [ ] **Step 4: Hand off what this plan deliberately did not fix**

Three things found during planning that are real but out of scope, and
should be filed rather than forgotten (see "Explicitly out of scope"): the
missing `PinToggle` on `/lines/[id]`, the absent JSON error envelope, and
the `validate_pin` case-normalisation gap that Task 8's `UPPER()` works
around on the read side.

---

## Explicitly out of scope

- **Review findings F6, F8, F9, F10, F11, F12, F13 and F14**, and the
  dissents recorded under D1 and D3. The task brief scoped this plan to the
  six ranked findings above. F9 (the date picker offering times the form
  rejects) is worth noting as F5a's natural partner — Task 5 makes the
  rejection message readable, F9 would stop it being reached — but it is a
  separate change to `TrackTrainForm`'s picker bounds and is not planned
  here.
- **The three functional bugs** the review itself scoped out
  (auto-navigation hijack, `/connect-claude` authenticated 500,
  stations-lookup wrong navigation). Task 1 does change the code path the
  third one runs through (`StationSearchForm.tsx:27`); whoever holds that
  bug should re-test after Task 1 lands, but this plan does not claim to
  fix it.
- **Everything in `2026-09-02-frontend-accessibility-fixes.md`** — the
  contrast, landmark and heading work. The four file collisions are listed
  above; neither plan needs to change its design for the other.
- **A JSON error envelope for 4xx responses.** The better long-term shape
  for F5a, and the only thing that would contain axum's own extractor
  rejection at `crates/api/src/routes/train.rs:365`. Out of scope because
  it touches every `(StatusCode, String)` handler in the crate — see
  Decision 5.
- **A minimum query length on the reference search.** The review notes a
  single character returns an apparently unfiltered list; Task 1 makes
  those 20 rows better-ranked but does not reduce them. A minimum length is
  a product decision the review did not ask for.
- **Normalising `pin_origin_crs` case on write.** Task 8 handles it with
  `UPPER()` on the read side, which is complete and needs no migration.
  Normalising the column would need a data migration over existing rows.
- **A `PinToggle` on `/lines/[id]`.** Found during F4's investigation
  (that page has no pin control at any width), not raised by the review,
  not fixed here.

## Open questions / risks

1. **Task 8 is the largest diff in this plan and the only one that changes
   an API response shape.** Four structs, four queries, eight join clauses.
   Nothing consumes these responses but this app's own frontend (grepped:
   these types are API-crate-local, never sent between Rust services), so
   there is no external contract to break — but Task 9 must land in the
   same series or the new fields are dead weight. The recorded fallback if
   it proves awkward is Decision 3's option (b), `/public/stations/all` +
   client-side mapping; **record the reason before taking it.**
2. **Task 1's `crs ILIKE $2` against a `CHAR(3)` column is asserted, not
   proven, to behave as an exact comparison.** `CHAR(n)` pads with spaces,
   and `LIKE` treats trailing spaces as significant — but every `crs` value
   is exactly three characters, so there is no padding. Task 1's DB tests
   exercise this directly (the `ZOR` fixture is a 3-char code matched by a
   3-char query). If they fail in a way that implicates padding, the fix is
   `crs::text ILIKE $2`.
3. **Task 2 restores a control that the accessibility plan will restyle.**
   That plan's `autoContrast` change flips the pinned star from
   white-on-yellow to black-on-yellow. Whichever lands second should look
   at the mobile table specifically — a 34px black-on-yellow star in a
   dense ~130-row list is the one place the two changes compound.
4. **Task 7 changes what a logged-in user sees on the app's front page**,
   and the design spec it overrides made the opposite call deliberately. If
   the "Right now" module turns out to compete with the pinning prompts
   rather than complement them, the cheap adjustment is ordering (module
   above the prompts, or a "worst five — see all 89" framing per the
   review's §F14), not reverting the decision.
5. **No dark-mode screenshot evidence exists for any of this.** The review
   captured 73 light-scheme screenshots and flagged the absence as its
   own highest-value follow-up. Task 3's fix is scheme-correct *by
   construction* (it uses `--mantine-color-anchor`, which both schemes
   define), but Task 3 Step 7 asks for a dark-mode look because
   construction arguments are not observations.
