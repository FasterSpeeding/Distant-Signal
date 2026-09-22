# Design: `/trains` Search State Lost on Back-Navigation

**Status: design proposal, research + recommendation only. No implementation
code was written for this document; see the parent task's explicit
instruction.**

Product ask, verbatim intent: on `/trains` ("Find a Train"), a visitor fills
in the search form, gets results, opens one result (`/train/[uid]/[date]`),
then presses Back (browser back button or an in-app link). Today the search
page comes back empty — every filter and the results list are gone, forcing
the visitor to re-enter everything. This document traces the actual code
path (not a guess), names the precise root cause, and recommends a fix
modeled on this codebase's own existing conventions rather than inventing a
new state-management approach.

Files read in full before writing this document: `frontend/app/trains/page.tsx`;
`frontend/components/TrainSearchForm.tsx`; `frontend/app/train/[uid]/[date]/page.tsx`
(the "View live status" destination and its own back-link to `/trains`);
`frontend/app/stations/page.tsx` and `frontend/app/stations/StationSearchForm.tsx`;
`frontend/app/lines/page.tsx` and `frontend/app/lines/AllLinesTable.tsx`;
`frontend/components/IncidentSearchForm.tsx` and `frontend/app/incidents/page.tsx`;
`frontend/app/track/page.tsx`; `docs/superpowers/plans/2026-09-22-operator-overview-phase1-aggregated-dashboard-plan.md`
(Tasks 4-5, the `?statusGroup=` design); `docs/superpowers/specs/2026-09-22-operator-overview-design.md`.

---

## 1. What actually happens today, verified by reading the code

### 1.1 `/trains` is a Server Component that reads `searchParams` — but only ever ONE-DIRECTIONALLY

`frontend/app/trains/page.tsx:53-94` is an `async function TrainsPage({ searchParams })` that reads
`station`/`origin`/`stops_at`/`date`/`ticketId` off the URL once, on the server, and passes them down
as `initialStation`/`initialOrigin`/`initialStopsAt`/`initialDate`/`attachTicketId` props
(`frontend/app/trains/page.tsx:85-91`) to `TrainSearchForm`, a Client Component.

This is real and it works, as far as it goes: a shared link like `/trains?station=RDG&origin=PAD`
correctly pre-fills those two fields. That is the entire scope of what the URL does today — it is a
one-shot seed for the form's *initial render*, read exactly once (there is no `useSearchParams()`
anywhere in `TrainSearchForm.tsx`, confirmed by grep). It is not a live, two-way binding.

### 1.2 `TrainSearchForm` holds every filter, AND the results, in plain `useState` — seeded once, never written back

`frontend/components/TrainSearchForm.tsx:219-254`:

```
const [stationCrs, setStationCrs] = useState(initialStation);
const [originCrs, setOriginCrs] = useState(initialOrigin);
const [stopsAt, setStopsAt] = useState(initialStopsAt);
const [dateValue, setDateValue] = useState<string | null>(initialDate || null);
const [fromTime, setFromTime] = useState('');
const [toTime, setToTime] = useState('');
const [arrivalFrom, setArrivalFrom] = useState('');
const [arrivalTo, setArrivalTo] = useState('');
...
const [results, setResults] = useState<Results>(null);
```

Two things worth noting precisely:

- Only four of the eight filter fields (`stationCrs`, `originCrs`, `stopsAt`, `dateValue`) are seeded
  from the URL at all. The four time-range filters (`fromTime`/`toTime`/`arrivalFrom`/`arrivalTo` —
  the wire's `from`/`to`/`arrival_from`/`arrival_to`, per the `TrainSearchRow` doc comment at
  `TrainSearchForm.tsx:60-68`) always start blank, regardless of what's in the URL, because
  `TrainsPage` never reads or forwards them and `TrainSearchForm` accepts no such props. They are
  not part of even the one-directional convention.
- `results` (the actual list of trains on screen) is never derived from the URL at all. It starts
  `null` unconditionally, and the only two things that ever populate it are `handleSubmit`
  (`TrainSearchForm.tsx:336-369`, fired by the user pressing Search) and `handleLoadMore`
  (`TrainSearchForm.tsx:371-424`, "Load more"). There is no mount-time effect that runs a search
  automatically — confirmed by grep: `TrainSearchForm.tsx` imports no `useEffect`, no `useRouter`,
  nothing from `next/navigation`. Even a visitor who arrives via a fully pre-filled deep link sees
  the static "Press Search to find trains that call at this station" placeholder
  (`TrainSearchForm.tsx:441-447`) until they click the button themselves.

### 1.3 The URL is never updated when a search actually runs

`searchParams()` at `TrainSearchForm.tsx:316-334` builds a `URLSearchParams` object — but it is used
for exactly one purpose, building the `fetch()` URL at `TrainSearchForm.tsx:347` and
`TrainSearchForm.tsx:400`. It is never handed to `router.replace`/`router.push`/`window.history`.
Grepping the whole file for `useRouter`/`next/navigation` confirms this: there is no import of either.
So whatever the visitor actually types into the form — a station, an origin, a date, any of the four
time filters — the browser's address bar stays exactly as it was when `/trains` first loaded. If the
visitor arrived via a bare `/trains` link (the overwhelmingly common case — this is the primary nav
destination, not a link most visitors click with query params already attached), the address bar
stays `/trains` no matter what they search for.

### 1.4 Following a result navigates away; back-navigation fully remounts the form

A result row's "View live status" link is `TextLink href={`/train/${encodeURIComponent(row.uid)}/${displayDate}`}` (`TrainSearchForm.tsx:564`). `TextLink` (`frontend/components/TextLink.tsx`) wraps
Next's own `<Link>`, so this is an ordinary client-side App Router navigation to a **different route
segment** (`/train/[uid]/[date]`, not a shared layout under `/trains`). `/train/[uid]/[date]/page.tsx`
even links back to `/trains` itself (`frontend/app/train/[uid]/[date]/page.tsx:356,364`,
`<TextLink href="/trains" ...>Find a train</TextLink>`) — but as a bare, param-less href, and it is a
"start a new search" link (a purposeful reset), not a back-navigation affordance, so it is not itself
the bug, just confirmation that nothing in this app threads `/trains` state forward into the detail
page and back.

Because `/trains` and `/train/[uid]/[date]` are unrelated route trees (nothing shared below the root
layout), navigating from one to the other unmounts `TrainSearchForm` entirely. Pressing Back re-enters
the `/trains` history entry and Next.js's App Router mounts a **fresh** `TrainSearchForm` instance
against whatever `searchParams` that history entry's URL carries. This is not a bfcache/scroll-
restoration quirk and it is not something Next.js's client-side Router Cache can paper over — a
brand-new component instance means every `useState` call re-runs its initializer, which is exactly
`initialStation` etc. read from that URL (§1.1-§1.2). Since that URL was, per §1.3, never updated to
reflect what the visitor actually searched, the "state to restore" is simply not there to restore.

### 1.5 Net effect, precisely

- Visitor arrives at bare `/trains`, fills in Station (+ maybe Origin/Stops at/Date/times), presses
  Search, opens a result, presses Back → lands back on bare `/trains`, every field empty, "Press
  Search…" placeholder. **This is the reported bug**, and it reproduces on every ordinary visit,
  because the vast majority of visits start from the nav link, not a pre-filled deep link.
- Visitor arrives via a full deep link (`/trains?station=RDG&origin=PAD&date=2026-09-25`), presses
  Back *without ever touching the form* → Station/Origin/Date fields correctly reappear (the
  one-directional seed still applies, since the URL for that history entry is unchanged), but the
  results list does **not** reappear — still "Press Search to find trains…" — because §1.2's
  mount-effect gap applies regardless of how the fields got filled in. This is a narrower but real
  sub-case of the same underlying bug.
- Any of the four time-range filters, or any edit the visitor makes to Station/Origin/Stops
  at/Date after landing, are lost on Back in every case — there is no path, deep-linked or not, by
  which they survive, since they are never written to the URL at all.

This is **not** a case of "the Server Component + `searchParams` pattern already handles this and the
gap is something subtler like scroll restoration" — the gap is exactly what it looks like: state lives
only in a client component instance that Back always throws away, and the one channel that could
survive that (the URL) is populated in one direction only, and even then covers 4 of 8 filters and
never the results themselves.

---

## 2. Root cause

`TrainSearchForm` follows this codebase's established "seed a client component's initial `useState`
from a Server Component's `searchParams`, via `initialX` props" convention — the same shape
`IncidentSearchForm` uses (`initialOperator`/`initialLine`/`initialFrom`/`initialTo`,
`frontend/components/IncidentSearchForm.tsx:108-118`) and the same shape the (currently unimplemented,
only planned) operator-overview Phase 1 work describes adding to `AllLinesTable` via
`initialStatusGroup` (`docs/superpowers/plans/2026-09-22-operator-overview-phase1-aggregated-dashboard-plan.md:117-121`,
explicitly documented there as **"one-directional only"**, line 118). That convention was designed to
answer "can I deep-link a shareable, pre-filtered view *into* this page" — and it answers that
question correctly. It was never designed to answer "will *my own* filters still be here if I leave
this page and come back," because it only ever reads the URL, never writes to it.

Back-navigation is exactly the case that exposes the gap: it re-delivers a *stored* URL rather than a
freshly-typed one, and if that stored URL was never kept in sync with what the visitor actually did,
there is nothing for the one-directional seed to recover. The App Router's full remount on a
cross-segment navigation (§1.4) is what turns "the URL is stale" into "the state is gone" — with no
live component instance for React to have quietly preserved anything in.

---

## 3. Recommended fix

Extend the same convention this codebase already uses, in the one direction it currently lacks, rather
than introducing a new state-management mechanism (no SWR/React Query, no sessionStorage, no global
client cache — none of those exist anywhere else in this frontend, and adding one just for this page
would be a new architectural pattern, not reuse of an existing one).

### 3.1 Write the search back to the URL when it runs

In `TrainSearchForm`, import `useRouter`/`usePathname` from `next/navigation` (the same import this
codebase already uses in `StationSearchForm.tsx:4`, just for a different purpose there) and, inside
`handleSubmit` (`TrainSearchForm.tsx:336-369`) right after building `searchParams()`, call
`router.replace(`${pathname}?${searchParams().toString()}`, { scroll: false })` before/alongside the
`fetch()`. Use `replace`, not `push`: the goal is "the current `/trains` history entry always reflects
the last search that actually ran," not "every search press adds a new entry to Back-button history" —
`replace` keeps `/trains` a single entry whose URL is kept current, matching how the page already
behaves today (one entry, just a stale one).

This should cover all eight filters `searchParams()` already knows how to serialize (station, date,
origin, stops_at, arrival_from, arrival_to, from, to) — §1.2 already found that `TrainSearchForm`'s own
`searchParams()` builds these correctly for the fetch call; the only change is *also* pushing that same
string to the address bar.

### 3.2 Carry the four time filters through `initialX` props too

`frontend/app/trains/page.tsx` needs to read `from`/`to`/`arrival_from`/`arrival_to` off
`searchParams` the same way it already reads `station`/`origin`/`stops_at`/`date`
(`frontend/app/trains/page.tsx:56-74`), and `TrainSearchForm` needs matching
`initialFrom`/`initialTo`/`initialArrivalFrom`/`initialArrivalTo` props to seed `fromTime`/`toTime`/
`arrivalFrom`/`arrivalTo` (`TrainSearchForm.tsx:223-226`), mirroring exactly what already exists for
the other four fields. Without this, 3.1 would put `from`/`to`/`arrival_from`/`arrival_to` in the URL
on search, but a remounted form would still never read them back out — an asymmetric fix that silently
drops two of the six filters (arrival-time pair only shows once `stopsAt` is set, so this is really a
"drop up to 4 of 8" gap) on every restore.

### 3.3 Auto-run the search once on mount, when the restored URL carries a station

Add a mount-only `useEffect` to `TrainSearchForm`, structurally identical to
`IncidentSearchForm.tsx:258-264`'s existing "auto-run-on-mount" effect: if `initialStation` is a valid
CRS code, call the same search logic `handleSubmit` uses (factor the body of `handleSubmit` after its
`canSearch` guard into a shared `runSearch()` the way `IncidentSearchForm.tsx:223-238`'s `runSearch`
is already shared between its Search button and its own mount effect) exactly once, against whatever
`initial*` values were just seeded. Gate it on `initialStation` specifically (not "any initial value"),
matching how `stationValid` is already this form's one *required* precondition
(`TrainSearchForm.tsx:273,427-433`).

This is the step that actually makes Back show the previous results, not just the previous form
values — 3.1+3.2 alone would restore the address bar and the form fields correctly but still leave the
visitor looking at "Press Search to find trains that call at this station" and needing one redundant
click, which is a real, user-visible partial fix, not the full one asked for.

### 3.4 Known, accepted limitation: pagination state (`nextCursor`) does not survive

If the visitor had pressed "Load more" one or more times before leaving, only *page 1* of that result
set is reconstructable from the URL — `nextCursor` is server-issued opaque pagination state
(`TrainSearchForm.tsx:87-94`) with no representation in `searchParams()`. Back-navigation after 3.1-3.3
will correctly re-run the *first* page of the same search, not resume exactly where "Load more" had
gotten to. This is the same class of accepted, explicitly-documented imprecision this codebase already
carries elsewhere (e.g. `dateWindow()`'s browser-vs-`Europe/London` clock-skew note,
`TrainSearchForm.tsx:44-52`) — worth a one-line doc comment at the fix site, not a blocker, since
re-running page 1 is a correct, if smaller, restoration rather than a wrong one.

### 3.5 Why `router.replace` + refetch, not a client-side cache

Since §1.4 established there is no live component instance for Back-navigation to return to (the App
Router remounts across route segments regardless of any router-level RSC caching), the URL is the only
piece of state that can actually survive the round trip without inventing new machinery. Refetching
page 1 on mount is cheap (this is exactly what the initial "Search" button click already costs today)
and guarantees the restored view reflects current data rather than a snapshot that might be stale by
the time the visitor returns — consistent with this route's own "may be up to 30 minutes out of date"
disclaimer already shown in the results header (`TrainSearchForm.tsx:477-480`).

---

## 4. Other pages found with the same class of bug (found while investigating precedent, not designed in detail here)

- **`frontend/components/IncidentSearchForm.tsx` / `frontend/app/incidents/page.tsx`.** Same
  underlying gap: `searchParamsFor()` (`IncidentSearchForm.tsx:194-210`) is used only to build the
  fetch and the mount-effect's initial query; there is no `router.replace`/`useRouter` anywhere in the
  file, so filter changes never reach the URL. It is *partially* masked in practice by its
  auto-run-on-mount effect (`IncidentSearchForm.tsx:258-264`), which always has a non-empty query
  thanks to the default 30-day `fromDate` floor (`IncidentSearchForm.tsx:134-138`) — so Back at least
  shows *some* results, just the default last-30-days view, not whatever operator/line/priority/
  planned/cleared filters the visitor had actually applied (none of which have any URL representation
  at all, not even one-directionally — `plannedFilter`/`clearedFilter`/`priorityMin`/`priorityMax` take
  no `initialX` props today).
- **`frontend/app/lines/page.tsx` / `frontend/app/lines/AllLinesTable.tsx`.** Today this page has *no*
  URL-driven filter state at all — `selectedOperators`/`selectedCountries`/`nameQuery`/`sort` are all
  plain local `useState` with no `initialX` seeding and no `searchParams` read on the page
  (confirmed: `AllLinesPage` in `frontend/app/lines/page.tsx` takes no `searchParams` argument today).
  The in-flight operator-overview Phase 1 plan
  (`docs/superpowers/plans/2026-09-22-operator-overview-phase1-aggregated-dashboard-plan.md`) will add
  exactly one field's worth of one-directional seeding (`?statusGroup=` → `initialStatusGroup`,
  explicitly scoped as "one-directional only" in that plan) — once that ships, `/lines` will have
  precisely the same latent "Back loses my filters" gap this document describes for `/trains`, for the
  same reason. Worth flagging to whoever implements that plan, so the two-way sync recommended here
  (§3.1) can be considered for `AllLinesTable` at the same time rather than needing its own follow-up
  investigation later.
- **`frontend/components/TrackTrainForm.tsx` / `frontend/app/track/page.tsx`.** Not audited in depth
  in this pass, but `frontend/app/track/page.tsx:86` seeds `TrackTrainForm` with an `initialOrigin`
  prop from `searchParams` in the same one-directional shape as `/trains`, and `TrainSearchForm.tsx`'s
  own doc comment (`TrainSearchForm.tsx:47-48`) states its query-param convention "mirrors `/track`'s
  own convention" — strongly suggesting the same write-back gap exists there too. Flagged as a
  candidate for the same fix, not yet designed.

Not affected by this class of bug: `frontend/app/stations/StationSearchForm.tsx` — it has no
intermediate results list to lose in the first place. "Look up" and "Use my location" both navigate
straight to `/stations/[crs]` (`StationSearchForm.tsx:53-64`, `goToStation`), so there is no
search-results state that a remount could strip; the only thing Back could plausibly lose is whatever
partial text the visitor had typed into the Autocomplete before pressing the button, which is a much
smaller, arguably-expected loss (most text inputs don't restore un-submitted drafts on Back either) and
out of scope for this investigation.
