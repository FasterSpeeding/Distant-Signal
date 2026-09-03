# Client-Local Timezone Display — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1 → 2 → 3 are strictly sequential** (each builds on the previous
> file). Task 4 is verification and runs last. This is a small, four-task
> plan; it is not worth parallelising.
>
> **This plan deliberately changes exactly one user-visible string in the
> whole app.** If implementation starts touching a second call site, stop —
> that is a scope error, not a discovery. See "Explicitly NOT in scope".

**Goal:** implement the recommendation of
`docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md`
— a *partial* switch to viewer-local time display. Every **network-time**
value (train schedules/ETAs, incident validity periods, line-status
history, trend-chart buckets, day-key grouping) stays pinned to
`Europe/London`. Exactly one **viewer-relative** value —
`components/TicketSummary.tsx`'s "Added {time}", the record of when *the
viewer themselves* added a ticket to the app — switches to the browser's
own local timezone, using the `useMounted()`-gated SSR-then-client-adjust
pattern this codebase already uses three times.

**Architecture:** frontend-only. No new npm packages, no backend changes,
no migrations, no new API routes, no prop-drilling refactor. One new
formatter function, one new tiny Client Component leaf, one one-line swap
in an existing Server Component.

| | File | Change |
|---|---|---|
| MOD | `frontend/lib/dateFormat.ts` | add `formatLocalDateTime()` — the *only* formatter in the module that is not pinned to `Europe/London` |
| NEW | `frontend/components/LocalDateTime.tsx` | 'use client' leaf: `useMounted() ? formatLocalDateTime : formatDateTime` |
| MOD | `frontend/components/TicketSummary.tsx` | `{formatDateTime(ticket.createdAt)}` → `<LocalDateTime value={ticket.createdAt} />` |
| MOD | `frontend/lib/dateFormat.test.ts` | new `formatLocalDateTime` describe block |
| NEW | `frontend/components/LocalDateTime.test.tsx` | SSR-determinism + post-mount-local + hydration-mismatch regression tests |
| MOD | `frontend/components/TicketSummary.test.tsx` | update the "added-on date" test to assert the local-time path |

**Tech stack:** Next.js 16 App Router + TypeScript (strict) + Mantine
9.5.2 (pinned exact), Vitest 2 + `@testing-library/react` via
`frontend/test/render.tsx`'s `renderWithMantine`, Playwright for e2e.

**Specs:**
- `docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md`
  — the research this implements. Its **Recommendation** section is
  authoritative for *what* to build. Its Findings 1 and 3 are the reasoning
  this plan does not re-derive.

---

## Verified facts (ground truth for this plan — do not re-derive)

Everything below was read out of the working tree while planning, at
commit `6903e5d`. File and line numbers are as of that HEAD.

### The formatter module

- `frontend/lib/dateFormat.ts` is 67 lines. Four module-level
  `Intl.DateTimeFormat` constants — `DATE`, `DATE_TIME`, `TIME`,
  `DAY_KEY` (`:17-43`) — each with an explicit
  `timeZone: 'Europe/London'`, exported as `formatDate`,
  `formatDateTime`, `formatTime`, `londonDayKey` (`:50-67`). A private
  `asDate()` helper (`:45-47`) normalises `string | Date`.
- The module header (`:1-16`) states the two reasons for pinning: a real
  past hydration bug, and the UK-rail product stance. **This header must
  be amended, not contradicted**, by Task 1 — see that task.
- `frontend/lib/dateFormat.test.ts` (37 lines) covers all four exports.
  Its `formatDate` "is independent of the runtime locale and timezone"
  test asserts `Intl.DateTimeFormat().resolvedOptions().locale` is not
  `en-GB`, i.e. it already relies on the test process's ambient
  locale/zone being non-UK.

### `TicketSummary` is a **Server** Component

- `frontend/components/TicketSummary.tsx` has **no `'use client'`
  directive** (`:1` is `import { Badge, Group, Stack, Text } from
  '@mantine/core';`). It is a Server Component.
- Its only two consumers are also Server Components:
  `frontend/components/TicketPanel.tsx:95` and
  `frontend/app/track/mine/page.tsx:167,203` (the latter twice — a
  tracked train's attached tickets, and the standalone-tickets section).
- The target line is `TicketSummary.tsx:73`:
  `Added {formatDateTime(ticket.createdAt)}`, inside
  `<Text size="xs" c="dimmed">` (`:72-74`), inside a `<Group gap="xs">`
  next to the provenance `<Badge>`.
- `createdAt` is documented at `TicketSummary.tsx:28-32` as never
  `null`/`undefined` on either wire shape — **no fallback branch is
  needed** in the new component.

### The established `useMounted()` precedent

Three existing working examples of "a value only the client can know,
gated so the first client render still matches the server's markup":

- `frontend/components/ThemeToggle.tsx:36-40` — `const mounted =
  useMounted(); const displayedScheme = mounted ? colorScheme : 'auto';`
- `frontend/components/ColorSchemeMeta.tsx:14-25` — same gate, imperative
  DOM mutation in a `useEffect`.
- `frontend/components/LastUpdated.tsx:34-40` — the closest precedent,
  and itself a date formatter: `const exact = formatDateTime(date); const
  mounted = useMounted(); const displayed = mounted ? relativeTime(date,
  new Date()) : exact;`

`useMounted` comes from `@mantine/hooks` (already a pinned dependency).

### Test conventions

- `frontend/test/render.tsx` exports `renderWithMantine(ui, options)` —
  the single sanctioned way a test wraps a subject in `MantineProvider`
  with the real production `theme`. Its own comment notes `ui` is typed
  `ReactNode` specifically so Server Components under test can be passed.
- Vitest config (`frontend/vitest.config.ts`): `environment: 'jsdom'`,
  `globals: true`, `setupFiles: ['./vitest.setup.ts']`,
  `include: ['**/*.test.{ts,tsx,js,jsx}']` (deliberately excludes
  `frontend/e2e/`, which is Playwright's), `@` aliased to `frontend/`.
- **The SSR-determinism regression-test shape already exists** in
  `frontend/components/LastUpdated.test.tsx:11-22` and
  `frontend/components/ThemeToggle.test.tsx`: `renderToString(<MantineProvider
  theme={theme}>…</MantineProvider>)` from `react-dom/server`, asserting on
  the raw HTML string, with the comment "renderToString never runs
  effects, so this is exactly what the server sends down." Task 3 reuses
  this shape verbatim rather than inventing one.
- `frontend/components/TicketSummary.test.tsx` (129 lines) has seven
  `it()` blocks, all `renderWithMantine` + `screen.getByText`. The last
  one is `'renders the added-on date via formatDateTime'` — Task 2 updates
  it.

### Lint / typecheck / test commands

`.github/workflows/ci.yml:220-256` documents this explicitly:
`frontend/package.json` **has no `lint` script and there is no
`eslint.config.*`** — "lint" in this repo *is* `npx tsc --noEmit` against
the strict tsconfig. There is no prettier config either. So the full
frontend gate is, from `frontend/`:

```
npx tsc --noEmit
npm test
npm run build
```

### Playwright / live verification

- `frontend/playwright.config.ts` has a `webServer` block that runs
  `npm run dev` on `http://localhost:3000` unless `E2E_BASE_URL` is set.
  `frontend/e2e/` already contains five specs.
- **Docker is not available in this sandbox.** `/track/mine` (the only
  page that renders `TicketSummary` outside `TicketPanel`) is
  session-gated and fetches from the `api` service, which only exists on
  the compose network. A live end-to-end browser check of a *real* ticket
  row is therefore **not** achievable here. Task 4 substitutes a genuine
  hydration test at the Vitest level (which exercises the exact risk —
  server markup vs. client markup for the same instant, in two different
  timezones) and reports honestly on what could and could not be
  observed. **Do not fabricate a browser observation.**

---

## Decisions this plan closes (do not reopen during implementation)

### D1. `Intl.DateTimeFormat` with no `timeZone` is sufficient. No offset detection.

Per ECMA-402, omitting `timeZone` resolves to the host environment's own
IANA zone (the same value `Intl.DateTimeFormat().resolvedOptions().timeZone`
returns) — a real zone identifier, not a frozen offset. That is *strictly
better* than any "detect the offset and apply it" approach, which by
construction cannot handle the viewer's own DST transitions (a viewer in
`America/New_York` would render EDT instants with an EST offset for half
the year). The research already verified the identical mechanism resolves
GMT/BST correctly at the exact transition instant for `Europe/London`; the
same tz-database lookup applies to whatever zone the browser reports.

**No `timeZone` key at all** — not `timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone`,
which is a redundant round-trip to the same value. The omission is made
deliberate-and-greppable by a doc comment rather than by ceremony.

### D2. The local formatter is constructed **per call**, not as a module-level constant.

`dateFormat.ts:14-16` explains why the other four are module-level
constants (construction is comparatively expensive; they run once per
rendered row). `formatLocalDateTime` deviates, on purpose:

- A module-level constant captures the host zone **at module-load time**.
  `dateFormat.ts` is imported by Server Components, so on the server that
  is the container's UTC — the wrong answer, cached process-wide.
- It also makes the function untestable: a test that flips
  `process.env.TZ` (Node re-reads it for subsequently-constructed
  `Intl` objects) could never observe the change through a formatter
  built at import time.
- The cost is negligible at this call site's actual scale: it runs
  post-mount only, in the browser, once per ticket row on a page that
  shows a handful of a single user's tickets.

### D3. Scope is `TicketSummary` only. `LastUpdated` is **deferred**, not forgotten.

The research (Recommendation, third bullet) explicitly ranks
`LastUpdated`'s switch as "optional/low-priority relative to
`TicketSummary`'s", because its dominant always-visible text is
`relativeTime`'s duration string, which is *already* viewer-clock-
independent by construction (`lib/relativeTime.ts` is a pure `Date.now()`
millisecond diff — no `Intl`, no timezone). The London-formatted absolute
time only surfaces in a tooltip and as a brief pre-mount fallback.

Deferring it is the spec-aligned call, and it also avoids the exact
inconsistency the research warned about in the same bullet: `LastUpdated`
appears in the app's nav/data-freshness furniture alongside many
network-time values, so flipping its tooltip alone buys one tooltip and
costs a subtle cross-page inconsistency. **Not in scope. Revisit only if
a user actually reports the tooltip as confusing.**

### D4. No "your local time" label, badge, or tooltip on the "Added" row.

The research's Open Question 1 asked whether `TicketSummary`'s local
"Added" time reading next to `AttachTicketAction.tsx`'s London
`formatDate(train.serviceDate)` on `/track/mine` would parse as sensible
or as an unexplained inconsistency. **Resolved: no extra UI treatment.**

- The two values already read as different *kinds* of fact from their own
  labels: "Added <when>" is about the viewer's own action; a train's
  service date is a property of the train. Nothing about the current copy
  invites the reader to compare them as the same clock.
- For the overwhelmingly dominant case this product is designed for — a
  UK-based viewer of a UK rail app (`README.md:3`: "A personal UK rail
  companion"; the research found *zero* evidence of a designed out-of-UK
  use case) — local time **is** London time, so a "your local time" label
  would be visible noise on a dimmed `size="xs"` metadata line ~always,
  to disambiguate a difference that ~never exists.
- Adding a `Tooltip` would put interactive chrome on a passive provenance
  line, which is precisely the kind of thing `TicketSummary.tsx:65-68`'s
  existing badge comment is already careful about.

If this ever needs revisiting, the cheap escalation is a `<time
dateTime={iso}>` wrapper (semantic, zero visual change) — but the
codebase uses no `<time>` element anywhere today, and introducing the
convention for one line is not justified now.

### D5. `TicketSummary` stays a Server Component; a Client leaf does the conversion.

The research's Finding 3 correctly notes a Server Component has no
`useEffect`/`useMounted()` escape hatch, so the conversion must happen
client-side. Two ways to get there: promote `TicketSummary` to
`'use client'` wholesale, or extract the one timestamp into a Client leaf.

**The leaf wins.** `TicketSummary` renders provenance labels, route
strings and Mantine layout that have no reason to ship to the browser or
to re-render; promoting it would push all of that (plus its `stationLabel`
dependency) into the client bundle to serve one string. The leaf takes a
single `string` prop (trivially serializable across the RSC boundary) and
returns bare text, so the surrounding `<Text size="xs" c="dimmed">`
styling stays exactly where it is in the Server Component.

### D6. Mantine flat named exports, never dot-notation.

`TicketSummary.tsx:1` already imports `{ Badge, Group, Stack, Text }`
flat. The new Client leaf imports **no** Mantine components at all (it
returns a bare fragment), so there is nothing to get wrong there — but if
implementation finds itself reaching for `<Text>` inside the leaf, that
is a signal it is duplicating styling that already exists at the call
site. Don't.

---

## Explicitly NOT in scope

- **Every network-time call site stays `Europe/London`.** That is thirteen
  sites, listed in the research's Finding 1 table. Two of them
  (`app/lines/[id]/history/TrendsResults.tsx:38` and `lib/history.ts:138`,
  both via `londonDayKey`) are a **hard backend-alignment correctness
  dependency**, not a preference: `lib/types.ts:124` documents
  `LineDailyStats.day` as "'YYYY-MM-DD', **Europe/London calendar day**",
  so the backend's own aggregation buckets are London days. A viewer-local
  day key would silently shift or clip which days' stats get requested and
  how history entries group. **Do not touch `londonDayKey` or either of
  its call sites.**
- `components/LastUpdated.tsx` — see D3.
- `components/TrackTrainForm.tsx`'s `DateTimePicker`/`dayjs` *input* flow.
  Not a `dateFormat.ts` call site; the research flagged it as adjacent and
  explicitly out of scope.
- **Locale.** This is a timezone change only. `formatLocalDateTime` keeps
  the explicit `'en-GB'` locale, exactly like every other formatter in the
  module — the viewer's *locale* is a separate question the research
  explicitly declined to scope in, and switching it would reintroduce the
  "5/10/2026 for 10 May" bug class the module header exists to prevent.
- Push-notification scheduling (backend, `chrono-tz`-aware). Untouched.

---

## Tasks

### Task 1 — `formatLocalDateTime` in `frontend/lib/dateFormat.ts`

- [ ] Add an exported `formatLocalDateTime(value: string | Date): string`
      that formats via `new Intl.DateTimeFormat('en-GB', { dateStyle:
      'medium', timeStyle: 'short' })` — **the same options as
      `DATE_TIME`, minus the `timeZone` key** — applied to `asDate(value)`.
      Constructed inside the function body, per D2.
- [ ] Give it a doc comment that carries the full weight of the exception:
      that the missing `timeZone` is deliberate and load-bearing (D1);
      that it resolves the *host's* zone, so it is **only correct in a
      browser, post-mount** and must never be called from a Server
      Component or from a client component's pre-hydration render; that
      `components/LocalDateTime.tsx` is the sanctioned way to use it; and
      why it is not a module-level constant (D2). Point at
      `docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md`.
- [ ] Amend the **module header comment** (`:1-16`) so it no longer reads
      as an absolute rule that the module now violates. It should still
      state the London-for-network-time default as the rule, and name
      `formatLocalDateTime` as the single, narrow, documented exception
      for viewer-relative timestamps. Keep the existing hydration-bug
      history — that reasoning is still exactly why the *default* is what
      it is.
- [ ] Add a `describe('formatLocalDateTime')` block to
      `frontend/lib/dateFormat.test.ts` covering:
      - it matches `formatDateTime` when the host zone **is** London
        (set `process.env.TZ = 'Europe/London'`);
      - it **differs** from `formatDateTime` when the host zone is not
        London (`process.env.TZ = 'Asia/Tokyo'` — 19 Aug 2026 18:56 UTC is
        `20 Aug 2026, 03:56` in Tokyo vs. `19 Aug 2026, 19:56` in London,
        so this asserts both a different time *and* a different day);
      - the output shape still matches the `en-GB` medium-date /
        short-time form (no seconds, no `M/D/YYYY`).
      Save and restore the original `process.env.TZ` in `beforeEach`/
      `afterEach` (or `afterAll`) so the surrounding tests in the file,
      which depend on the ambient zone, are unaffected. **If flipping
      `process.env.TZ` mid-process turns out not to affect newly
      constructed `Intl` objects on this Node version, say so and switch
      to comparing against a locally constructed
      `new Intl.DateTimeFormat('en-GB', {…, timeZone:
      Intl.DateTimeFormat().resolvedOptions().timeZone})` — do not silently
      drop the test.**
- [ ] Verify: `cd frontend && npx tsc --noEmit && npm test`.
- [ ] Commit.

### Task 2 — `frontend/components/LocalDateTime.tsx` + the `TicketSummary` swap

- [ ] Create `frontend/components/LocalDateTime.tsx`: `'use client'`,
      importing `useMounted` from `@mantine/hooks` and both
      `formatDateTime`/`formatLocalDateTime` from `@/lib/dateFormat`.
      Props: `{ value: string }`. Body: `const mounted = useMounted();
      return <>{mounted ? formatLocalDateTime(value) : formatDateTime(value)}</>;`
      — a bare fragment, no Mantine wrapper (D5/D6).
- [ ] Doc-comment it in this codebase's voice, following
      `LastUpdated.tsx:11-24`'s shape: what it renders, why the
      `useMounted()` gate exists (the server has no idea what zone the
      viewer is in, so an ungated local format would emit different server
      and client markup for the same instant — the exact bug class
      `dateFormat.ts`'s header describes), that the pre-mount value is the
      London-formatted one (deterministic on both sides), that this
      component is for **viewer-relative** timestamps only and network-time
      values must keep calling `formatDateTime` directly, and that a brief
      pre-hydration flash from London to local is the accepted trade-off
      (same one `ThemeToggle`/`LastUpdated` already take).
- [ ] In `frontend/components/TicketSummary.tsx`: replace
      `{formatDateTime(ticket.createdAt)}` (`:73`) with
      `<LocalDateTime value={ticket.createdAt} />`, add the
      `import { LocalDateTime } from './LocalDateTime';` and **remove the
      now-unused `formatDateTime` import** (`:3`) — `tsc --noEmit` under
      `noUnusedLocals` will catch it if missed, but don't rely on that.
      Leave the surrounding `<Text size="xs" c="dimmed">Added …</Text>`
      untouched.
- [ ] Add a short comment at the swap site saying why *this one*
      timestamp is viewer-local while the service dates rendered a few
      lines away in sibling components stay London — pointing at the
      research doc. This is the artefact that stops a future reader
      "fixing" the inconsistency (D4).
- [ ] Update `TicketSummary.test.tsx`'s last `it()` (currently `'renders
      the added-on date via formatDateTime'`): rename it to describe the
      local-time behaviour and keep it asserting the rendered "Added …"
      text is present. Do **not** hard-code a London-formatted expected
      string there — the component now renders in the *test process's*
      zone post-mount, and pinning a literal would make the test
      machine-dependent.
- [ ] Verify: `cd frontend && npx tsc --noEmit && npm test`.
- [ ] Commit.

### Task 3 — `LocalDateTime` tests, including a real hydration check

- [ ] Create `frontend/components/LocalDateTime.test.tsx`. Mirror
      `LastUpdated.test.tsx`'s imports and structure (`renderToString`
      from `react-dom/server`, `MantineProvider` + `theme` for the SSR
      case, `renderWithMantine` from `@/test/render` for the mounted
      case).
- [ ] Test: **server-rendered output is the London-formatted time.** With
      `process.env.TZ` forced to something non-London (`Asia/Tokyo` — i.e.
      simulating a server whose ambient zone is *not* the viewer's),
      `renderToString` must contain the London string and must not contain
      the Tokyo one. This is the regression guard for "someone deletes the
      `useMounted()` gate."
- [ ] Test: **post-mount output is the host-local time.** With
      `process.env.TZ` forced to `Asia/Tokyo`, `renderWithMantine` must
      show the Tokyo-formatted string, not the London one.
- [ ] Test: **no hydration mismatch.** This is the one that actually
      exercises the risk end to end. Produce server HTML while the process
      zone is `Europe/London`, then set the process zone to `Asia/Tokyo`
      (the "browser"), `hydrateRoot` that markup, and assert (a) React
      logged **no** hydration error/warning (spy on `console.error` — and
      on `console.warn`; React 19 routes recoverable hydration errors
      through `onRecoverableError`, so **also** pass an
      `onRecoverableError` callback to `hydrateRoot` and assert it was not
      called with a hydration mismatch), and (b) after `act()` flushes,
      the text has become the Tokyo one. If React 19's exact channel for
      this differs from the above, **find out empirically and assert on
      whatever it actually is** — do not weaken the test to "renders
      something".
- [ ] Verify: `cd frontend && npx tsc --noEmit && npm test`.
- [ ] Commit.

### Task 4 — Full verification pass

- [ ] From `frontend/`, run and record real output for all three gates:
      `npx tsc --noEmit`, `npm test`, `npm run build`.
- [ ] Attempt a live check. `npm run dev` can start without Docker, but
      every page that renders a `TicketSummary` needs the `api` service.
      Try it; if the page cannot render a ticket row, **say exactly that**
      and fall back to the Task 3 hydration test as the substantive
      evidence. A Playwright `timezoneId` context option would be the
      right tool if the app *were* servable — note it as the follow-up if
      it isn't.
- [ ] **Report honestly what was and was not observed.** No invented
      screenshots, no invented console output, no "verified in the
      browser" unless a browser actually rendered it.
- [ ] Confirm no network-time call site changed:
      `git diff main --stat` should show only the six files in the
      Architecture table plus this plan. `grep -rn "londonDayKey"` should
      show no diff at any of its call sites.

---

## Risks

- **The pre-hydration flash.** For a non-UK viewer, "Added 19 Aug 2026,
  19:56" briefly paints before flipping to their local rendering. The
  research's Open Question 2 flagged this; it is the same trade-off
  `ThemeToggle` and `LastUpdated` already ship, on a lower-stakes value
  (a dimmed `size="xs"` provenance line, not a control). Accepted.
  For UK viewers — the designed audience — there is no visible flash at
  all, because the two strings are identical.
- **Someone later "tidies" `formatLocalDateTime` into a module-level
  constant**, silently freezing it to the server's UTC. Mitigated by D2
  being written into the function's own doc comment, not just this plan.
- **Someone later reaches for `formatLocalDateTime` at a network-time call
  site**, because it is now an available export. Mitigated by the module
  header amendment (Task 1) and the swap-site comment (Task 2) both
  naming the boundary explicitly.
