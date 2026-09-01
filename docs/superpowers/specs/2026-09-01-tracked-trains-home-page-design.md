# Design: Tracked Trains on the Home Page

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` (the
closest and most directly relevant precedent — same feature area, same
data, same team, same session's conventions) and
`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` (whose
Policy and home-page proposal this spec directly builds on and, in one
place, revisits — see Corrections below). No implementation plan is
included; that is a separate, later step in this repo's process.

## Goal

Individual train tracking and the "my tracked trains" list page
(`/track/mine`, `GET /Train/mine`) are both complete, shipped features — a
logged-in user can pin a train, watch its live progress, and come back
later to see everything they're tracking. But the only way to reach that
list is a dedicated nav-bar link (`TrackedTrainsNavItem` in
`app/layout.tsx`, itself only visible when logged in); nothing about it is
visible from the home page (`/`), which today is built entirely around
line and station status. A logged-in user with, say, a delayed train
pinned for later today has no reason to think to click "My Tracked
Trains" unless they already remember they pinned something — the home
page, their actual landing page, gives no hint. This spec designs a
condensed tracked-trains section for the home page, scoped to logged-in
users, and works out what it should show, where, how it's fetched, and
what happens when there's nothing to show.

## Corrections / relationship to prior specs

Following this repo's established "Corrections" precedent
(`2026-08-29-train-tracking-frontend-design.md`,
`2026-08-31-tracked-trains-list-design.md`): one prior decision is being
knowingly revisited here, not overlooked.

**`2026-08-31-tracked-trains-list-design.md`'s Decision 3 already
considered and rejected a home-page section**, with this reasoning: *"the
home page renders identically in shape whether or not you're logged
in... bolting a third section onto an otherwise anonymous-friendly page
would either need to silently hide the section for anonymous visitors
(fine) or show a login nudge inline (breaks that page's existing
'renders the same shape for everyone' character)."* That rejection was
written **before `/Train/mine` or `/track/mine` existed** — at the time,
the entire tracked-trains-list feature was still being designed in the
same document, and the "silently hide it" branch was dismissed mostly in
passing, in favor of keeping the home page's contract untouched while
the rest of that spec's scope (the list page and its nav link) shipped.

Two things have changed since:

1. **`TrackedTrainsNavItem` (Decision 4 of that same spec, now shipped
   in `app/layout.tsx`) already broke the exact "same shape for
   everyone" contract this rejection leaned on** — it renders `null`
   entirely for a logged-out visitor and a real link for a logged-in
   one. The nav bar is not visually identical across sessions today, and
   nothing in this codebase has treated that as a problem since it
   shipped. "Silently hide it for anonymous visitors," the branch that
   spec dismissed as merely "fine," is in fact the exact pattern this
   repo already uses one component over.
2. **This task is explicitly scoped to logged-in users**, not to
   preserving an anonymous-identical page. The anonymous-user-ux spec's
   own home-page proposal (§Home page redesign) is itself already a
   session-branching page (`getSession()`, different content for
   anonymous vs. logged-in) — as of this writing that redesign has not
   been implemented (`frontend/app/page.tsx` still has no `getSession()`
   call anywhere; confirmed by reading it in full and by `git log
   --oneline -- frontend/app/page.tsx`, which shows no commit past
   `d268d75` implementing it), but this spec does not depend on it
   landing first or conflict with it if it does — see Decision 5.

So: this spec does not contradict the tracked-trains-list spec's actual
concern (don't make the page look broken/incomplete for an anonymous
visitor) — it uses the "hide it entirely" branch that spec already named
as acceptable, now backed by a real, already-shipped precedent for doing
exactly that.

## Current relevant state (verified 2026-09-01)

- **`frontend/app/page.tsx`** (read in full): an async Server Component,
  `export const revalidate = 0`. Fetches `getPreferences()` (401-tolerant,
  returns `{ pinnedLines: [], pinnedStations: [] }` for an anonymous
  visitor), then `getLineStatusForMode(DISPLAYED_MODES_PARAM)` (all lines,
  every displayed mode), then builds `pinnedStationEntries` via
  `Promise.all(...)`. Renders two `Stack` sections in order: "Your Lines"
  (`Title order={1}`, a `SimpleGrid` of `LineStatusCard`s, or dimmed
  empty-state text with a link to `/lines`) then "Your Stations" (`Title
  order={2}`, a `Stack` of `Card`s linking to `/stations/{crs}`, or
  dimmed empty-state text with a link to `/stations`). No `getSession()`
  call anywhere on this page today, and no third section of any kind.
- **`getMyTrackedTrains()`** (`frontend/lib/api.ts`, confirmed by reading
  it): `GET /Train/mine`, cookie-forwarded, `cache: 'no-store'`, returns
  `null` on `401` (not logged in — the complete signal, no separate
  `getSession()` call needed, per `/track/mine`'s own precedent) or
  `TrackedTrainListItem[]` on `200`. Already used, unmodified, by
  `frontend/app/track/mine/page.tsx`.
- **The backend query** (`crates/api/src/data/train_tracking.rs`'s
  `list_tracked_trains_for_user`, confirmed by reading it): `SELECT ...
  FROM tracked_trains tt LEFT JOIN train_current_state cs ... WHERE
  tt.user_id = $1 ORDER BY tt.tracked_at DESC LIMIT $2`, bound to
  `MINE_LIST_LIMIT = 100`, indexed on `user_id`. **No query parameter or
  other mechanism exists to ask for fewer rows, or for any
  status-based filter** — confirmed by reading `get_my_tracked_trains` in
  `crates/api/src/routes/train.rs`, which takes only `State<App>` and
  `AuthenticatedUser`, no `Query<...>` extractor. The route always
  returns up to 100 rows.
- **No reliable "still active" signal exists**, restated from that
  spec's own Finding 1 because it's load-bearing here too:
  `train_current_state.status` can reach `'cancelled'`, but
  `crates/trust-consumer/src/journey.rs`'s `apply_movement` never emits
  `'completed'` — every journey that finished normally sits at
  `'en_route'` (or whatever its last real status was) indefinitely. A
  filter for "currently active/en route" would therefore not reliably
  exclude trains that plainly finished hours or days ago; it's the same
  problem that spec's Decision 2 already ruled out an "active only"
  filter over, and it applies equally to any narrower home-page query.
- **`AutoRefresh`** (`frontend/components/AutoRefresh.tsx`): mounted
  once in `app/layout.tsx`, calls `router.refresh()` every 30s for every
  page, no per-route opt-out. This already re-runs `page.tsx`'s
  `no-store` fetches on every open home-page tab.
- **`/track/mine`'s own empty state** (`frontend/app/track/mine/page.tsx`,
  read in full): `trains.length === 0` → `<Text c="dimmed">You haven't
  tracked any trains yet. <Link href="/track">Track a train</Link> to get
  started.</Text>`. This is the existing, real "nudge to `/track`" copy
  already live for this exact "zero tracked trains" case — relevant to
  Decision 3 below (don't duplicate it).
- **Reusable rendering logic**: `TrackedTrainListRow` (route text,
  `formatDate`/`formatTime`, `RowStatusBadge` with its `STATUS_LABELS`
  map) lives locally inside `frontend/app/track/mine/page.tsx`, not
  exported — a home-page component reusing this look either duplicates a
  trimmed version of it or the file is refactored to export a shared
  piece; noted as an implementation-time choice, not decided here (see
  Explicitly out of scope).

## Decisions

### 1. What shows: a condensed "recently tracked" list, capped at 5 — not the full list, and not an "active only" filter

Two axes were considered separately: how many rows, and which rows.

**How many**: the home page's primary job is a line-status overview
(`Title order={1}` "Your Lines" is literally the first thing on the
page); a tracked-trains section competing for that same attention with a
full, up-to-100-row list would invert the page's priority. **Chosen: cap
at 5 rows**, with a "View all" link to `/track/mine` for anything beyond
that — matching the existing pattern this page already uses for "Your
Stations" (a compact list of cards, not a data table) and for "Your
Lines" (`Browse all lines` link out to the fuller `/lines` view rather
than trying to be that fuller view itself). 5 is a starting, reasonable
number, not a researched one — flagged in Open questions, same posture
the parent spec's own `MINE_LIST_LIMIT` took for its figure.

**Which rows**: two real options were considered.

- **Filter to "active"/"en route" only.** **Rejected**, for the same
  reason `tracked-trains-list-design.md`'s Decision 2 already rejected it
  for the full list: `'completed'` is never actually emitted (see Current
  relevant state), so a status filter here would not reliably mean
  "currently happening" — it would just as easily surface a train that
  finished normally two days ago as one running right now, while reading
  as if it were curated. Building a *second* filter that has the same
  honesty problem the sibling spec already flagged and avoided once would
  be a regression, not a refinement.
- **The 5 most-recently-tracked (`trackedAt DESC`), unfiltered.**
  **Chosen.** This is exactly `list_tracked_trains_for_user`'s existing
  order — no new sort semantics are introduced, so the home-page widget
  and `/track/mine` never disagree about "what's first" for the same
  data; a user who opens `/track/mine` from the "View all" link sees the
  same 5 rows they just saw on the home page, at the top, unsurprised.
  This is deliberately **not** "next N departures" (sorting by
  `pinScheduledDeparture`): that was considered and rejected for the same
  reason the parent spec rejected it for the full list — a train pinned a
  month in advance would rank ahead of one departing in twenty minutes
  that was pinned five minutes ago, which is very likely the one thing a
  user glancing at their home page actually wants to see first.

Row content mirrors `TrackedTrainListRow`'s existing fields, trimmed for
a smaller card: route (`origin → destination` or bare origin),
`serviceDate` + `pinScheduledDeparture` (via `formatDate`/`formatTime`,
already imported by the sibling page), and the same status/delay badge
treatment (`RowStatusBadge`'s existing `resolutionStatus` vs. `status` +
`delayMinutes` branching). No new visual language — this reuses, not
reinvents, the sibling page's already-designed row.

### 2. Placement: a third section, after "Your Stations"

`frontend/app/page.tsx`'s current layout is two `Stack` sections in a
fixed order: "Your Lines" (`order={1}`), "Your Stations" (`order={2}`).
**Chosen: append a third section, "Your Tracked Trains" (`order={2}`,
same heading level as "Your Stations"), directly below "Your Stations."**
Not a sidebar — this page has no sidebar layout anywhere today (a single
`Stack` down the page, per `Container size="lg"` in `app/layout.tsx`),
and introducing one for a single section would be a bigger structural
change than this feature needs. Not above "Your Lines" — line status is
this app's core purpose and its own `Title order={1}`; a supplementary,
narrower feature (tracked trains, opt-in and only relevant to a subset of
logged-in users) sitting above it would misstate the page's priorities.
Bottom placement also matches this spec's Decision 4 (silently hiding the
section when empty): a section that may or may not render at all reads
more naturally as the last, optional thing on the page than as a
sometimes-present gap in the middle of it.

### 3. Data fetching: reuse `getMyTrackedTrains()` unmodified, fetched in parallel, sliced to 5 in the page component

**Chosen: call the existing, unmodified `getMyTrackedTrains()`** —
already `cache: 'no-store'`, already cookie-forwarded, already
`null`-on-401 (the complete "not logged in" signal, no separate
`getSession()` call needed on this page either, mirroring `/track/mine`'s
own established reasoning for why one isn't needed here). Slice to the
first 5 elements after the fetch — no backend change, since the query is
already ordered `trackedAt DESC`, so "first 5 of the response" already
*is* "5 most recently tracked" (Decision 1); no client-side re-sorting
needed either.

Fetched via `Promise.all` alongside `getPreferences()` and
`getLineStatusForMode(...)`, not awaited sequentially after them — this
page already has one internal precedent for concurrent independent
fetches (`pinnedStationEntries`'s own `Promise.all(...)` over per-station
reads), and there is no data dependency between tracked trains and either
existing fetch, so serializing them would only add latency for no
reason.

**Considered and rejected: adding an optional `?limit=` query parameter
to `GET /Train/mine`**, so the home page could ask the backend for only 5
rows instead of up to 100. Rejected as premature for this pass: the
existing query is already bounded (`LIMIT 100`, indexed on `user_id`,
one `LEFT JOIN`) — not an unbounded scan — and this codebase has no
real-usage data yet on how many trains a typical user tracks or how
expensive this query actually is under real load (the same "no
real-world data yet" posture `MINE_LIST_LIMIT`'s and `MAX_PIN_AGE`'s own
doc comments already take for their figures). Adding a second call shape
to an already-shipped, tested route on a hypothesis rather than a
measured cost would be exactly the kind of speculative complexity this
codebase's existing comments consistently avoid elsewhere. If the home
page's added query load turns out to be a real, measured problem once
this ships, a `?limit=` parameter (default `MINE_LIST_LIMIT`, still
capped server-side to prevent a caller requesting more than 100) is the
natural, incremental follow-up — not designed further here. See Open
questions/risks.

### 4. Empty state: hide the section entirely — no CTA, no placeholder

Three real options were considered for "a logged-in user has zero
tracked trains":

- **Show a dimmed empty-state sentence with a link to `/track`**,
  mirroring "Your Lines"/"Your Stations"' own empty-state pattern
  exactly. **Rejected.** Those two sections are permanent, foundational
  navigation surfaces of this page — they always render, pinned or not,
  because "pin a line" / "pin a station" is core, first-class
  functionality this page exists to surface. Tracked trains is a
  narrower, opt-in, supplementary feature layered on top; giving a
  brand-new logged-in user *three* separate "you haven't done X yet, go
  do X" prompts on first login (two of which are core, one of which
  isn't) reads as nagging rather than helpful, especially since
  `/track/mine` already has its own, more specific empty-state copy for
  exactly this case, reachable any time via the nav bar.
- **Show a shorter, plainer hint** (e.g. just a muted line, no full
  sentence/CTA). **Rejected** as splitting the difference badly — still a
  third "empty" block on a page whose primary job is line status, for a
  feature most visitors (including plenty of logged-in ones who simply
  don't track trains) will never use, while adding less real guidance
  than `/track/mine`'s existing copy already gives.
- **Render nothing at all — no heading, no text, no card — when the
  fetched list (post-slice) is empty.** **Chosen.** A logged-in user who
  has never tracked a train, or who is between pins, sees a home page
  that looks exactly like it does today (two sections, not three) — no
  new noise. A user who *does* want to start tracking already has two
  ways in regardless of this section's presence: the "Track a Train" nav
  link (always visible) and, once they've tracked at least one train
  ever, `/track/mine`'s own nav entry. This section's only job is to
  surface something once it exists, not to prompt someone into creating
  it in the first place — that prompting job already belongs to `/track`
  and `/track/mine`, and duplicating it here would be redundant with
  both.

This is the same "hide entirely, not present-but-empty" branch Decision
2/4 of `tracked-trains-list-design.md` already used for
`TrackedTrainsNavItem` (`null` for a logged-out session) — here applied
one level further, to "logged in but nothing to show" as well as
"logged out."

### 5. Loading/revalidation: no new mechanism — inherits the page's existing posture

`frontend/app/page.tsx` already sets `export const revalidate = 0` (so
Next.js doesn't try to statically prerender it — the `api` service only
exists on the compose network at runtime, per that line's own existing
comment) and is already refreshed every 30s by the global `AutoRefresh`
mounted in `app/layout.tsx`, the same as every other dynamic page.
`getMyTrackedTrains()` is already `cache: 'no-store'`, matching every
other fetch already on this page (`getLineStatusForMode`,
`getStopPointDisruption`, `getPreferences`, all confirmed `no-store` in
`frontend/lib/api.ts`). **No new revalidation strategy, polling interval,
or client-side refresh logic is introduced** — this section updates on
exactly the same cadence the rest of the home page already does, for the
same reason (the existing global mechanism, not a per-feature one).

If the anonymous-user-ux spec's own home-page redesign (a `getSession()`
call branching anonymous vs. logged-in content) lands before or after
this spec's implementation, the two don't conflict: this section's own
gating already comes for free from `getMyTrackedTrains()`'s `null`-on-401
return, so it needs no coordination with a separate, page-level
`getSession()` call even if one gets added for unrelated reasons (the
anonymous "Right now" widget). The two features are independent both in
data and in control flow.

## Architecture

```
┌───────────────────────────────────────────────────────────────────────┐
│ frontend/app/page.tsx                                                   │
│                                                                             │
│  Promise.all([                                                          │
│    getPreferences(),              existing                              │
│    getLineStatusForMode(...),     existing                              │
│    getMyTrackedTrains(),          NEW -- reused unmodified from lib/api  │
│  ])                                                                      │
│                                                                             │
│  trackedTrains = (result ?? []).slice(0, 5)   // null on 401 -> []      │
│                                                                             │
│  <Stack>                                                                 │
│    Your Lines        (unchanged)                                        │
│    Your Stations     (unchanged)                                        │
│    Your Tracked Trains   NEW -- rendered only if trackedTrains.length>0 │
│      5 rows, trackedAt DESC, reusing TrackedTrainListRow's row shape     │
│      "View all" -> /track/mine                                          │
│  </Stack>                                                                 │
└──────────────────────────┬──────────────────────────────────────────────┘
     no-store, cookie-fwd  │  (existing route, existing function,
                            ▼   no backend change)
                 GET /Train/mine  (crates/api, unchanged)
```

## Error handling

- `getMyTrackedTrains()`'s `401` branch is not an error path on this page
  either, same as `/track/mine`'s own treatment — it's the expected,
  common "not logged in" outcome, collapsed to an empty section (Decision
  4), not a thrown error.
- Any other non-ok status (5xx, network failure) throws via the shared
  `errorForResponse`, same as every other fetch already on this page —
  this page has no bespoke `error.tsx` today and none is proposed here;
  a failure here falls through to the existing root `app/error.tsx`,
  taking down the whole home page exactly as a failure in
  `getLineStatusForMode` already would. This is an accepted, pre-existing
  posture on this page, not a new risk introduced by this feature — see
  Open questions/risks for whether it deserves its own `.catch()`
  eventually.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest):

- `app/page.tsx`: render tests for the three new real outcomes —
  `getMyTrackedTrains()` returning `null` (section absent, page otherwise
  unchanged from today's two-section shape), `[]` (section absent), and a
  populated list (section present, capped display at 5 even when more
  than 5 are returned, "View all" link present and pointing at
  `/track/mine`, rows ordered as returned — i.e. no client-side
  re-sorting is introduced).
- If `TrackedTrainListRow`/`RowStatusBadge` are extracted to a shared
  location to avoid duplicating `/track/mine`'s local versions
  (implementation-time choice, not decided here): a test confirming both
  call sites render identically for the same input, so the two pages
  can't silently drift apart in status-badge wording.

## Explicitly out of scope for this spec

- **Any backend change to `GET /Train/mine`** (a `?limit=` parameter or
  otherwise). Decision 3 explicitly rejects this for now; the route and
  query are reused exactly as they exist today.
- **A ranking smarter than "5 most recently tracked"** — e.g. surfacing
  cancelled or delayed trains first, or genuinely "next departing." Both
  were considered (Decision 1) and set aside as either dishonest given
  what the data can support (Finding 1's carryover) or a bigger design
  question than this pass needs to answer; flagged in Open
  questions/risks as worth revisiting with real usage.
- **Extracting `TrackedTrainListRow`/`RowStatusBadge` into a shared
  component.** Noted in Testing/Architecture as a reasonable
  implementation-time choice to avoid duplication, but not mandated or
  designed here — the home page's row can equally well be a smaller,
  independently-written variant if that turns out cleaner in practice.
- **The anonymous-user-ux spec's own home-page redesign** (the
  session-branching "Right now" widget for anonymous visitors). Not
  implemented as of this writing (Corrections, above) and not designed
  or touched by this spec — the two are independent, as Decision 5 notes.
- **Pagination, filtering, or any control on the new section itself.**
  It is a fixed, capped, unfiltered 5-row summary with one link out to
  the full list; no in-place "load more" is proposed.

## Open questions / risks

1. **5 rows is a starting, unresearched figure**, same posture as
   `MINE_LIST_LIMIT`'s own `100` — this codebase has no real usage data
   yet on how many trains a logged-in visitor typically has tracked at
   once, or how a home-page reader actually wants that traded off against
   screen space. Worth revisiting once real usage exists.
2. **Stale-but-not-cancelled trains linger indefinitely at the top of
   this widget**, same root cause as the parent spec's own Finding 1: a
   user who tracked a train once and hasn't tracked anything since will
   keep seeing that same, quite possibly long-finished, train as their
   top "recently tracked" result on every home-page visit, since nothing
   in this codebase can currently distinguish "finished normally" from
   "still en route" (`'completed'` is never emitted). Not fixed here —
   the same underlying gap the parent spec already deferred to
   `trust-consumer`, not a frontend concern.
3. **Added query cost on every home-page load, for every logged-in
   visitor, every 30s via `AutoRefresh`.** The query itself is bounded
   and indexed (Current relevant state), but this is a genuinely new,
   periodic, per-user database read added to what is likely this app's
   single most-visited page — not benchmarked here. If this turns out to
   be a measurable real cost, Decision 3's rejected `?limit=` parameter
   (or, more aggressively, a short server-side cache with a coarser TTL
   than `AutoRefresh`'s 30s) is the natural follow-up.
4. **The home page's "same shape for every visitor" character is now
   fully gone**, not just weakened — a visitor can see two, or three,
   sections depending on both auth state and whether they have tracked
   trains. Corrections (above) argues this is an acceptable, already-
   precedented direction (`TrackedTrainsNavItem`), but it's worth naming
   plainly as a real shift from this page's original, simpler design
   intent, not an incidental side effect.
5. **Silently hiding the section on zero tracked trains (Decision 4)
   means a logged-in visitor who has never tracked a train gets no signal
   from the home page that the feature exists at all** — discovery relies
   entirely on the always-visible "Track a Train" nav link. This was a
   deliberate trade-off against nagging (Decision 4), but if user
   research later shows tracked-train discovery is a real problem, a
   softer, one-time (not permanent) nudge might be worth revisiting —
   not designed here.
