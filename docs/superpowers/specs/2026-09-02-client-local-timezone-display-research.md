# Client-Local Timezone Display — Research

**Status: research/survey only, not an approved design.** No code was
changed to produce this. Written to answer one question: should this app's
displayed timestamps/dates/times switch from the current hardcoded
`Europe/London` rendering to the *viewer's own* local timezone?

## Goal

`frontend/lib/dateFormat.ts` is this app's single date/time formatting
module, and it currently pins every `Intl.DateTimeFormat` instance to
`timeZone: 'Europe/London'` regardless of who's viewing the page or where
they are. That was a deliberate choice, documented in the module's own
header comment. This research re-examines that choice on its merits: is
London-time-for-everyone still correct, or should the app show times in
each viewer's own local timezone instead — fully, partially, or not at
all? The task explicitly asked for a genuine investigation, not a
foregone conclusion either way.

## Current relevant state

**The module and its stated reasoning.** `frontend/lib/dateFormat.ts:1-16`
gives two reasons for the current design, in its own header comment:

1. Hydration-safety: `new Date(x).toLocaleDateString()` with no explicit
   timezone follows the *process's* ambient locale/timezone — en-GB/
   Europe-London in a British browser, en-US/UTC in the Node process
   rendering the page server-side — which "was simultaneously showing
   Americans' dates to UK users ('5/10/2026' for 10 May) and emitting
   different server and client markup for the same timestamp." This was a
   real, previously-hit bug, not a hypothetical.
2. A product stance: "this is a UK rail product, so dates are en-GB and
   times are London wall-clock" — i.e. train times should read as
   London/station time no matter where the viewer is, the same convention
   airline departure boards and National Rail's own departure boards use
   (always the station's local time, never the passenger's).

Four formatters exist — `DATE`, `DATE_TIME`, `TIME`, `DAY_KEY`
(`dateFormat.ts:17-43`) — each with `timeZone: 'Europe/London'` set
explicitly, exported as `formatDate`, `formatDateTime`, `formatTime`,
`londonDayKey` (`dateFormat.ts:50-67`).

**BST-transition verification (not assumed — tested).** Because
`Intl.DateTimeFormat` is pointed at a real IANA zone identifier
(`Europe/London`), not a fixed UTC offset, it should resolve GMT/BST
automatically, including exactly at the spring-forward boundary. Verified
with a standalone script (`/tmp/.../bst_verify.mjs`, not part of the repo)
that instantiates the *exact* formatter options `dateFormat.ts` uses and
feeds it instants bracketing the UK's 2027 spring transition (last Sunday
in March, clocks go 01:00 GMT → 02:00 BST at 01:00 UTC):

```
2027-03-28T00:59:59.000Z  formatTime -> 00:59   (still GMT)
2027-03-28T01:00:00.000Z  formatTime -> 02:00   (exact transition instant, correctly BST)
2027-03-28T01:30:00.000Z  formatTime -> 02:30   (BST)
2027-03-28T09:00:00.000Z  formatTime -> 10:00   (BST, mid-morning)
```

The formatter jumped cleanly from 00:59 to 02:00 across the transition
instant with no off-by-one and no stale offset — correct. A second check
confirmed the result is **invariant to the Node process's own ambient
timezone**: with `process.env.TZ` forced to `UTC`, `America/New_York`,
`Europe/London`, and `Asia/Tokyo` in turn, the explicit-`Europe/London`
formatter produced the identical `02:00` in every case. This directly
confirms the module's own premise — pinning a real IANA zone, rather than
leaving the timezone implicit, is exactly what makes the server (UTC-ish
process) and client (British-browser process) agree, and exactly what
makes BST transitions resolve correctly without any bespoke
BST-awareness in this codebase. **The current approach is not fragile
here; it is doing the one thing IANA tz data is for, correctly.**

**Deployment timezone (server side).** `frontend/Dockerfile` (all three
stages: `builder-base`, `runtime-prod`, `runtime-dev`) sets no `TZ`
environment variable anywhere, and `docker-compose.yml`'s `frontend:`
service block (lines 662–687) sets only `API_BASE_URL`, `RAILMCP_*`
env vars — no `TZ`. The base image is `node:22-bookworm-slim`
(`Dockerfile:17,64,95`), which ships with no `/etc/localtime` override,
so the container's ambient timezone is UTC — consistent with
`dateFormat.ts`'s own comment ("en-US/UTC in the Node process rendering
the page"). (Docker itself was not available in this sandbox to spin up
the image directly; this is the same fact the module's own header comment
already asserts, and lines up with `node:*-bookworm-slim`'s well-known
default of UTC with no localtime symlink.)

**Existing SSR-then-client-adjust precedent, already used three times in
this codebase for the exact same hydration-mismatch problem class:**

- `components/ThemeToggle.tsx:28-33,37,39-40` — Mantine's `colorScheme`
  reads `localStorage` synchronously even pre-hydration, so it can already
  disagree with the server-rendered default. The fix: render the SSR
  default until `useMounted()` (`@mantine/hooks`) flips true, then switch
  to the real value. Comment: "Rendering the layout's default until after
  mount keeps that first client render identical to the server output;
  the real, possibly-stored preference then takes over post-hydration."
- `components/ColorSchemeMeta.tsx:14-25` — same `useMounted()` gate,
  imperative DOM mutation only inside a `useEffect`, explicitly citing
  "the same `useMounted()`-gated imperative-DOM-mutation shape
  `PrideToggle.tsx` already uses... and the same
  `useComputedColorScheme('light')` hook/fallback `ThemeToggle.tsx`
  already uses — no new hook, no new gating pattern, no new fallback
  constant," and explicitly naming "the hydration-mismatch bug class
  already fixed in ThemeToggle/PrideToggle/LastUpdated."
- `components/LastUpdated.tsx:16-24,34-40` — directly analogous to a
  client-local-time switch: `mounted ? relativeTime(date, new Date()) :
  exact`. Before mount it shows a fixed, `Europe/London`-formatted
  absolute time (`formatDateTime`, timezone/locale-deterministic
  regardless of which process renders it); after `useMounted()` flips
  true, it switches to a live relative string ("Xm ago") computed from
  `Date.now()`. The component's own comment states the rationale
  verbatim: "A relative 'time ago' string depends on `Date.now()` at
  render time, so it can't be computed identically during SSR and the
  client's pre-hydration render — the same class of bug fixed in
  `ThemeToggle`."

So this exact problem — a value that's only knowable/correct on the
client, colliding with SSR — has already been solved three times in this
codebase with the same `useMounted()`-gated pattern, and one of those
three (`LastUpdated`) is itself a date/time formatter. There is a proven,
idiomatic way to introduce a client-only value (like the viewer's own
timezone) without reintroducing a hydration mismatch, if that's the
direction chosen.

**Note: `relativeTime.ts` (`frontend/lib/relativeTime.ts:1-16`) is
timezone-agnostic by construction** — it's a pure `Date.now()` millisecond
diff bucketed into "just now"/"Nm ago"/"Nh ago"/"Nd ago," with no
`Intl.DateTimeFormat` or timezone involved at all. `LastUpdated`'s
dominant, most-visible state (the "Xm ago" text) is therefore *already*
correct and identical for a viewer in London or Tokyo — the London-vs-
client-local question only touches its absolute-time tooltip and its
pre-mount fallback text, not its primary display.

## Findings

### 1. Call-site categorization

Re-grepped fresh (`grep -rn "formatDate\|formatDateTime\|formatTime\|londonDayKey"` across `frontend/`, excluding `dateFormat.ts` itself and `*.test.*`). The parent conversation's list was accurate with one correction: **`components/TrackTrainForm.tsx` does not call `dateFormat.ts` at all** — it uses `@mantine/dates`' `DateTimePicker` and `dayjs` directly for a form input, not a display formatter (see the separate note below, since it's still relevant to the abroad-usage question). `lib/types.ts:140` has one match but it's a comment referencing `formatTime`, not a call site.

**Confirmed, current call sites, categorized:**

| Site | What it shows | Category |
|---|---|---|
| `components/EtaBadge.tsx:25` | A train's live ETA | **Network-time** |
| `components/AttachTicketAction.tsx:67-68` | A tracked train's service date | **Network-time** |
| `components/TrainJourney.tsx:21` | A tracked train's service date | **Network-time** |
| `app/page.tsx:301` | Dashboard: tracked train's service date + scheduled departure | **Network-time** |
| `app/track/mine/page.tsx:149` | Same, on the "my trains" page | **Network-time** |
| `app/lines/[id]/history/page.tsx:168,190-191` | Day headings and status-change times in a line's incident history | **Network-time** |
| `components/IssueList.tsx:50-51,57-58` | Incident/disruption validity periods ("Now", "From 10 May", full from–to) | **Network-time** |
| `app/incidents/[id]/page.tsx:19-20,107,119,122` | Incident validity periods, per-entry `recordedAt`, `firstSeenAt`, `fetchedAt` | **Network-time** (see reasoning below) |
| `app/lines/[id]/history/TrendsCharts.tsx:80` | X-axis tick labels on a line's delay/cancellation trend chart | **Network-time** |
| `app/lines/[id]/history/TrendsResults.tsx:38` | `londonDayKey(from)`/`(to)` used to build the *query range* sent to the stats API | **Network-time, non-negotiable** — see below |
| `lib/history.ts:138` | `londonDayKey(entry.computedAt)`, groups status-history entries into day buckets | **Network-time, non-negotiable** — see below |
| `components/LastUpdated.tsx:35` | "data refreshed" tooltip/pre-mount absolute time (dominant "Xm ago" text is already timezone-agnostic) | **Weak viewer-relative candidate** |
| `components/TicketSummary.tsx:58` | "Added {time}" — when the *viewer* created this ticket record in the app | **Viewer-relative candidate** |

**Why most of these are network-time, not just "the current default":** every one above except the last two is displaying a fact about the UK rail network's own clock — a train's scheduled/actual departure, an incident's validity window, a line's aggregate performance bucketed by London day/half-hour. This is exactly the departure-board precedent the task description raises: a train timed "14:32" is 14:32 at the London-area station regardless of where the person checking it happens to be sitting, the same way a Heathrow departure board shows local UK time to a passenger who is themselves already in the terminal, and the same way flight trackers (FlightAware, Flightradar24) show a flight's scheduled/actual times in the departure and arrival airports' own local zones, not the viewer's. Converting these to viewer-local would silently change what number the app is even claiming to state (no longer "the London wall-clock time this train leaves" but "what your own clock would read at that instant") — a real semantic change, not just cosmetic reformatting.

**`TrendsResults.tsx:38` and `history.ts:138` are a stronger case than "should stay London" — they can't correctly become viewer-local at all without breaking correctness**, independent of any UX preference: `lib/types.ts:124` documents `LineDailyStats.day` as "'YYYY-MM-DD', **Europe/London calendar day**" — the backend's own aggregation is bucketed by the London calendar day, and `dateFormat.ts:35-37`'s own comment on `DAY_KEY` says why: "grouping history by the UTC day would split a British summer evening across two headings." If the frontend computed day keys in the viewer's own zone instead, a US-based viewer's query range would no longer line up with the backend's own London-day buckets, silently shifting or clipping which days' stats get requested/grouped. This one is a correctness constraint, not a design preference.

**`app/incidents/[id]/page.tsx`'s `recordedAt`/`firstSeenAt`/`fetchedAt`** are arguably closer to `LastUpdated`'s "data freshness" framing than to a train's own schedule — they're about when *this app's* aggregator recorded/refetched something, not a fact intrinsic to the rail network. But they're categorized here as network-time because they sit on the same page, in the same list, interleaved with the incident's own `validityPeriods` (unambiguously network-time) — showing `firstSeenAt` in the viewer's zone while `validityPeriods` two sections up stays London would read as an internally inconsistent page, not an improvement.

**The two genuine viewer-relative candidates are narrow:**

- `TicketSummary.tsx:58`'s "Added {time}" is a record of the *viewer's own action* (uploading/entering a ticket) — this is conceptually the same as an email or notification timestamp, which apps conventionally show in the reader's own local time, not the sender's. This is the strongest single case for client-local in the whole call-site list.
- `LastUpdated.tsx`'s absolute-time tooltip is a weak candidate: its dominant, always-visible text is `relativeTime`'s duration string, which is already viewer-clock-independent by construction (see above). The London-formatted absolute time only surfaces in a tooltip and as the brief pre-mount fallback — low stakes either way, and switching it alone (while every surrounding timestamp on the same page stays London) would be a small, easily-missed inconsistency for the marginal benefit of one tooltip.

### 2. Is there a real out-of-UK usage case?

Searched `README.md` and `docs/superpowers/specs/` for any stated design intent around non-UK viewers (`grep -li "abroad|outside the uk|overseas|different timezone|non-uk|international"`). Every hit was a false positive — station names containing "International" (Ashford International, Ebbsfleet International) or Eurostar being explicitly *out of scope* ("Eurostar is out of scope (international, not National Rail)," `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md:61`). The train-tracking design docs (`2026-08-28-train-tracking-design.md`, `2026-08-29-train-tracking-frontend-design.md`) and the notifications design docs were also checked for any "tracking on behalf of a friend/family member," remote-monitoring, or push-while-abroad framing — none found.

`README.md:3` describes the product itself as **"A personal UK rail companion"** — singular, personal-use framing, not a shared/monitoring-someone-else tool. There is no account-sharing, no "share this tracked train with someone," and no evidence anywhere in the specs of a designed use case where the person looking at the app is not themselves the one dealing with the UK train.

That said, absence of a *designed* feature for it doesn't mean it never happens in practice — a UK-based user's family abroad checking a shared link, or a Brit travelling abroad glancing at their own commute status before flying back, are both plausible incidental uses this app was never asked to prevent. Reasoning through the UX for that incidental case: for network-time values (a train's departure, an incident's validity), the departure-board precedent holds up — "the 14:32 to Waterloo" reads the same and means the same thing whether you're in London or Los Angeles; converting it to the viewer's own clock ("your train leaves at 09:32 your time") is *more* work for someone trying to coordinate with someone else who is also looking at UK time (a station announcement, another passenger, a text from the traveller), because now two people looking at "the same train" see two different numbers with no timezone label to reconcile them, unlike a flight board which is always unambiguous because it's always airport-local. For the two viewer-relative candidates (`TicketSummary`'s "Added" timestamp, `LastUpdated`'s freshness tooltip), a non-UK viewer's own local time is plausibly *more* intuitive precisely because those values are about their own relationship to the app ("when did I do this," "how stale is what I'm looking at right now"), not about the rail network's clock.

### 3. Hydration-mismatch risk, concretely

The risk is real but well-understood in this codebase and not a blocker if handled the established way. Next.js Server Components render first on the server (ambient UTC per the Dockerfile finding above) then hydrate on the client (the viewer's actual device timezone). For a UK-based viewer, London time and "local" time are numerically the same, so a naive client-local switch would often *look* fine in casual UK testing while still being wrong in the two ways that matter:

- **Literal hydration-mismatch warnings.** If a client-local formatter ran unconditionally in a Server Component's render output, the server would emit UTC-formatted markup and the client would re-render with the browser's real zone during hydration — text-content mismatch, exactly the bug class `dateFormat.ts`'s own header comment describes as having already happened once ("simultaneously showing Americans' dates to UK users... and emitting different server and client markup for the same timestamp").
- **Server Components specifically can't know the client's zone at all.** Unlike a Client Component, a Server Component has no `useEffect`/`useMounted()` escape hatch — it never re-renders on the client. Any call site that's a Server Component today (e.g. `app/incidents/[id]/page.tsx`, `app/lines/[id]/history/page.tsx`, both `async function ... Page` with no `'use client'`) cannot itself defer to post-mount client state; genuinely switching one of those to client-local time would require either converting the relevant piece to a Client Component or passing the raw instant down and letting a Client Component leaf format it — a real, if mechanical, refactor, not a one-line change in `dateFormat.ts`.

**But this codebase has already solved this exact problem shape three times** (`ThemeToggle`, `ColorSchemeMeta`, `LastUpdated` — see Current relevant state above): render a stable, deterministic SSR value, gate the client-only value behind `useMounted()`, and swap it in post-hydration with no mismatch, because React only compares against the server's own output on the *first* client render, and by definition the `useMounted()`-gated branch doesn't run until after that first render completes. `LastUpdated` is the closest precedent of all, since it already does exactly this for a time-derived value (`formatDateTime` pre-mount, `relativeTime` post-mount) — extending that same component to show a client-local absolute time (instead of, or alongside, the relative string) post-mount would be following an established, working pattern in this exact file, not inventing a new one.

## Recommendation

**Partial switch — narrow, not full, and not "no change."**

- **Keep `Europe/London` as the default for every network-time value** (train schedules/ETAs, incident validity periods, line-status history, trend-chart buckets, the `londonDayKey` grouping/query-range logic). The BST-transition test confirms the current mechanism is technically sound, not a workaround masking a bug. The product-stance reasoning holds up under real UX scrutiny in Finding 2, not just as an assertion: these are facts about the rail network's own clock, the departure-board convention for exactly this kind of value is well-established and serves the same real coordination purpose here (two people, possibly in different timezones, both need to be able to say "the 14:32" and mean the same train), and `TrendsResults.tsx`/`lib/history.ts`'s day-bucketing is a hard correctness dependency on the backend's own London-day semantics, not a stylistic choice. There is no evidence — despite a genuine search — of a designed or documented out-of-UK-viewer use case that would outweigh this.
- **Switch the two viewer-relative candidates** — `TicketSummary.tsx:58`'s "Added {time}" and, more marginally, `LastUpdated.tsx`'s absolute-time tooltip/pre-mount fallback — to the viewer's own local timezone, using the `useMounted()`-gated pattern this codebase already has three working examples of (most directly, extending `LastUpdated.tsx`'s own existing `mounted ? … : formatDateTime(date)` branch, which is already primed for exactly this kind of swap). These are the only two call sites where the value being shown is about the *viewer's own relationship to the app* rather than a fact about the UK rail network, and the departure-board reasoning that justifies keeping the other twelve-plus sites in London time doesn't apply to them.
- Treat `LastUpdated`'s switch as optional/low-priority relative to `TicketSummary`'s: its primary text is already timezone-agnostic (`relativeTime`), so the benefit is limited to a tooltip and a brief pre-mount flash, while `TicketSummary`'s "Added" timestamp is the one call site where a UK-network-time framing genuinely doesn't fit what's being communicated.

This is not "switch everything" (the evidence doesn't support it — it would break the day-bucketing dependency outright and would degrade the coordination value of network-time display for the majority of call sites, with no offsetting designed use case to justify it) and it is not "change nothing" (there are two real, if narrow, sites where the current London-only framing is arguably answering the wrong question).

## Explicitly out of scope

- `components/TrackTrainForm.tsx`'s `DateTimePicker`/`dayjs` input flow (`TrackTrainForm.tsx:70-83,146-172`) is not a `dateFormat.ts` call site and this research did not investigate it as a display question. It's flagged here only because it's adjacent and genuinely interesting: the picker already captures the viewer's *raw local wall-clock* string with no explicit timezone, and the form's own comment (`TrackTrainForm.tsx:70-81`) documents working around exactly the ambiguity this whole research is about, for the opposite direction (input, not display) — a UK-based viewer picking "14:32" gets a `Date` parsed in their browser's own zone, which is correct for them, but a viewer physically abroad picking the London departure time they mean to track would need to mentally convert to their own local clock first, since the picker has no explicit "this is London time" framing. Whether that's a real problem, and whether it should also be addressed, is a separate question from the display-only scope this research was asked to cover.
- No code changes were made or proposed as a concrete diff; this document identifies which call sites would change under the recommendation above, not how to implement it (component boundaries, prop-drilling the raw instant vs. formatting server-side, etc.).
- Locale (`en-GB` vs. the viewer's own locale/number formatting) was not investigated — the task and the module's own header comment frame this as a timezone question, and `dateFormat.ts`'s original hydration bug was as much about locale (`toLocaleDateString()`'s en-US-in-Node default) as timezone; a client-local *locale* switch is a related but distinct question this research did not scope in.
- Push-notification scheduling/timing (`docs/superpowers/specs/2026-09-02-line-status-notifications-design.md`, `2026-09-01-schedule-feed-push-design.md`) is backend-scheduled against a `chrono-tz`-aware `Europe/London` clock and is not a frontend display concern; out of scope here.

## Open questions/risks

- If `TicketSummary.tsx` switches to client-local time, it would then disagree with `AttachTicketAction.tsx`'s adjacent `formatDate(train.serviceDate)` (staying London) on the same merged `/track/mine` page — both render in the same list context. Worth confirming this reads as sensible ("added" being about the viewer, "service date" being about the train) rather than as an unexplained inconsistency, possibly with a UI treatment that makes the distinction legible (e.g., a label or tooltip noting "your local time" vs. no such note needed for London times, matching how `LastUpdated`'s tooltip already exists to carry the exact value).
- The `useMounted()` pattern means the first client paint still shows the London-formatted (or otherwise-SSR) value for a brief moment before flipping to client-local — same trade-off `ThemeToggle`/`LastUpdated` already accept, but worth re-confirming it's acceptable for a ticket-creation timestamp specifically (lower stakes than a theme icon, but a visible "flash" nonetheless).
- This research could not run the actual Docker image in this sandbox to directly confirm the container's runtime `Intl.DateTimeFormat().resolvedOptions().timeZone` is UTC (only reasoned from the Dockerfile/compose file's absence of `TZ` plus `node:22-bookworm-slim`'s documented default and `dateFormat.ts`'s own comment asserting the same). Worth a quick `docker compose exec frontend node -e "console.log(Intl.DateTimeFormat().resolvedOptions().timeZone)"` against a real deployment to close that gap with a first-party observation rather than reasoning-by-documentation.
- If a future feature *does* introduce shared/multi-viewer tracking (the "check on a friend/family member's train" scenario this research found no current evidence for), the network-time-for-everyone recommendation above should be revisited specifically for that feature, since a shared link is exactly the case where a consistent, unambiguous London time matters most for coordination between viewers — this research's "no evidence of the use case" finding is a snapshot of the current app, not a permanent constraint.
