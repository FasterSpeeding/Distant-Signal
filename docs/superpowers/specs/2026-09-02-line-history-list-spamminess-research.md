# Line-History Incident List Spamminess — Root-Cause Research

**Status: research/root-cause investigation only, not an approved design.**
Written to the same rigor as
`docs/superpowers/specs/2026-09-01-enricher-period-cap-failures-research.md`
(this session's structural template: cite what was fetched/read, flag what
couldn't be confirmed, reach a real recommendation rather than a hedge). No
code was edited to produce this document.

## Goal

`https://konata.fox-prometheus.ts.net/lines/thameslink-cambridge/history`'s
Timeline tab (the incident list, not the Trends charts) reads as "spammy" —
too many, too repetitive, too noisy entries. This document investigates
*why*, grounded in the real page plus the actual data-model/write/read code,
and sketches prioritized directions — not a fix.

## Method

- **Real-URL fetch: succeeded, contrary to expectation.** The task brief
  expected this Tailscale (`*.ts.net`) hostname to be unreachable from this
  sandbox and told me to fall back to code review immediately on failure.
  Instead, both `WebFetch` and a direct `curl` succeeded: `curl -v` shows a
  full TLS handshake against `100.87.228.55:443` with a valid Let's
  Encrypt certificate for `konata.fox-prometheus.ts.net`, and a plain
  `curl -sS -o ... -w "HTTP_STATUS:%{http_code}"` against the exact history
  URL returned `HTTP_STATUS:200`, ~205KB of HTML. **This sandbox has real
  network reachability into the target Tailscale node** — the task's
  stated assumption did not hold this time.
- The saved HTML is a Next.js App Router shell; the incident list itself is
  not in it. Decoding the embedded RSC flight payload
  (`self.__next_f.push(...)` chunks) shows only Trends-tab rollup fields
  (`skipRate`, `delayRate`, `cancellationRate`, `avgDelayMinutes`,
  `bucketKey`, `sampleCycles`) — confirming the Trends tab is prefetched/
  streamed differently from the Timeline tab, whose `HistoryResults` server
  component (`frontend/app/lines/[id]/history/page.tsx:148-200`) sits behind
  its own `<Suspense>` boundary and calls `getLineStatusHistory`
  (`frontend/lib/api.ts:123-133`), which hits `GET /Line/{id}/Status/{from}/to/{to}`
  on an **internal-only** `API_BASE_URL` (`frontend/lib/api.ts:43-46`,
  throws if unset) — not a route exposed on the public Tailscale hostname
  directly. So the raw JSON `LineStatusHistoryEntry[]` for this specific
  request could not be captured verbatim by `curl`.
- Used `WebFetch` against the real URL to get a description of the rendered
  page instead. Its top-line figure — "200 status recomputes across 87
  incidents" over a ~7-day window — **matches the exact copy template**
  hardcoded in `HistoryResults` (`page.tsx:161-164`: `"{entries.length}
  status {recompute(s)} across {spanCount} {incident(s)}, newest first."`),
  which is strong corroboration this reflects real rendered content rather
  than a fabricated answer. Its more granular observations (recurring "X of
  Y sampled services delayed"-style phrasing, "delayed by congestion"
  recurring, "speed restrictions" dominating some mornings, 5–15-minute
  update clustering, severity swinging between Minor/Severe for similar
  causes) are a summarizing model's paraphrase of the real page, not a
  verbatim quote — treated as directionally reliable, not word-for-word
  evidence (see Open questions/risks).
- Read the full incident-list render path: `frontend/app/lines/[id]/history/page.tsx`,
  `frontend/lib/history.ts` (`groupHistoryByDay`, `collapseDay`,
  `coreReason`), `frontend/lib/api.ts`, `frontend/lib/types.ts`.
- Read the full write path: `crates/aggregator/src/aggregation.rs`
  (`aggregate`, `status_from_incident`, `infer_from_samples`, `classify`,
  `escalate_from_sample_stats`, `is_active`, `next_rail_day_boundary`,
  `has_recurring_schedule`) and `crates/aggregator/src/queries.rs`
  (`write_line_status`, `normalize_for_diff`, `normalize_entry_for_diff`,
  `strip_live_sample_annotation`, `prune_history`).
- Read the full matcher: `crates/aggregator/src/matcher.rs`
  (`lines_affected_by`, `match_one`, `MatchScope`) to check the task
  brief's hypothesis (d) — whether the `EXCLUSIVE_SEGMENT`/`SHARED_SEGMENT`/
  `STATION_HIT`/`KEYWORD_ONLY`/`OPERATOR_ONLY` taxonomy could produce more
  than one entry per line for one real incident.
- Read `docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md`
  (found via `git log --all`/`git show 343e4a2`, per the brief — it is not
  present under `docs/superpowers/plans/` but its design doc **is** still
  present under `docs/superpowers/specs/`, and its rail-day-boundary
  incident-expiry mechanism it specifies is implemented as described in the
  current `aggregation.rs`) for prior context on incident lifecycle/
  staleness.
- Grepped `docs/superpowers/specs/` for existing "dedup"/"duplicate"/
  "spam"/"noisy" discussion specific to the history list. None found: the
  only pre-existing "dedup" code in this codebase is
  `crates/aggregator/src/dedup.rs`'s `dedup_new_sample_stats`, which
  deduplicates *TRUST/LDBWS sample departures* to avoid double-counting a
  train polled twice — a different problem from incident-identity dedup,
  not investigated further here.
- Read `lines/thameslink-cambridge.toml` to understand the real line's
  shape: it shares `gn-peterborough-branch` as a genuine `SharedSegment`
  with `great-northern-kings-lynn.toml` (its Peterborough branch), and has
  station-overlap-only (not shared-segment) relationships elsewhere. This
  line legitimately sees incidents that originate on Great Northern's own
  network wherever they touch the shared Peterborough trunk — relevant
  context for why this specific line's incident *count* might be
  genuinely higher than a simpler line's, separate from the spamminess bug
  itself.
- **Local repro: not attempted.** The task brief suggested this as a
  fallback for when the real URL is unreachable, and separately assumed
  `docs/superpowers/specs/2026-08-31-sample-data-availability-design.md`
  documents seed/sample-data availability for a docker-compose repro —
  it does not: that document is about distinguishing "no sample data" from
  "genuinely quiet right now" in `SampleAvailability`/`SampleStats`
  wire shape, unrelated to seeding a local Postgres for a repro. Since the
  real production URL was reachable and gave directly-observed evidence
  stronger than a synthetic local seed would, local repro was not pursued
  given this pass's effort budget. The raw JSON gap this leaves is noted
  in Open questions/risks.

## Findings

### Root cause (primary, well-evidenced): LDBWS-sample-derived `reason` text is not normalized for identity, on either the write or read side

Two independent grouping/dedup mechanisms exist in this codebase, and
**both** exempt the live-sample-derived reason text that most directly
drives its own churn:

**Write side — `write_line_status` inserts a new `line_status_history` row
only if `changed`.** (`crates/aggregator/src/queries.rs:273-324`)
`changed` compares `normalize_for_diff(existing)` against
`normalize_for_diff(fresh)` (`queries.rs:163-170`), and
`normalize_entry_for_diff` (`queries.rs:177-192`) strips exactly three
things: `validity.from_date`, the whole `sample_stats`/`sample_availability`
objects, and a trailing `" (live samples show: ...)"` suffix
(`strip_live_sample_annotation`, `queries.rs:262-268`) — the annotation
`escalate_from_sample_stats` appends when live samples escalate an
*incident-derived* status's severity. It does **not** touch anything else
inside `reason` itself.

**Read side — `collapseDay` groups a day's recomputes into one row per
distinct `reason` identity.** (`frontend/lib/history.ts:84-125`) The
identity key is `coreReason(status.reason)` (`history.ts:16-18`), which
strips only the identical `" (live samples show: ...)"` pattern via the
same regex shape (`LIVE_SAMPLE_ANNOTATION`, `history.ts:7`). Anything else
different between two `reason` strings makes them two different spans/rows,
not two "flips" collapsed into one span.

**The gap: `infer_from_samples`'s own reason text is neither of the things
either side strips.** (`crates/aggregator/src/aggregation.rs:884-966`) For a
line with no matching Knowledgebase incident, its LDBWS-sample-derived
`reason` is built by `classify()` (`aggregation.rs:1010-1069`) with the
live counts baked directly into the string, e.g. `format!("{delayed} of
{total} sampled services delayed.")` (`aggregation.rs:1062`) or `format!(
"{cancelled} of {total} sampled services cancelled.")`
(`aggregation.rs:1023`/`1029`) — then `infer_from_samples` appends a
**second**, separately-noisy suffix: `" (most cited: {most_common})"`
(`aggregation.rs:928-935`), built from a fresh per-cycle scan of live
departures' free-text `delay_reason`/`cancel_reason` fields via
`most_common`. Neither the embedded counts nor the `"(most cited: ...)"`
suffix match the `" (live samples show: ...)"` pattern either
`strip_live_sample_annotation` or `coreReason` strip — they are a
different annotation than the one the escalation path uses, and were
evidently never plumbed into either normalization function.

**Consequence:** during any stretch when a line's displayed status is
LDBWS-sample-derived (`DataQuality::LdbwsInferred` — i.e. no matching
Knowledgebase incident is currently active for it) rather than
incident-derived, its `reason` string is very likely to differ on almost
every poll cycle (the aggregator recomputes every 5–15 minutes, per
`frontend/lib/history.ts:127`'s own doc comment), because live delayed/
cancelled counts and the most-cited free-text delay reason both fluctuate
that often. This is broken on **both** ends independently:
1. The backend's "insert only if changed" guard is defeated, so
   `line_status_history` accumulates a near-continuous stream of rows for
   what is really one ongoing situation.
2. Even where two cycles' text happened to coincide (or where the backend
   guard worked as intended for some other reason), the frontend's
   `collapseDay` still can't fold genuinely-the-same-situation rows
   together across cycles where the text *did* drift, because its identity
   key is the same unstripped `reason` string.

This matches the real page's observed symptoms closely: recurring
near-identical-but-not-identical "X of Y sampled services delayed"-style
entries, "delayed by congestion"/"speed restrictions" recurring as
seemingly separate items, updates clustering every 5–15 minutes, and
severity apparently flapping between Minor/Severe for "the same" cause —
each is exactly what unstripped, per-cycle-varying sample-derived text
grouped by exact-string identity would produce.

**Scoping note — Knowledgebase-incident-derived spans are NOT broken this
way.** `status_from_incident` (`aggregation.rs:123-173`) builds `reason`
from `incident.summary` plus at most two annotations: a fixed scope tag
(`" (shared trunk — also affects other lines)"` / `" (operator-wide
report)"`, stable per match) and `extraction_annotation` from
`apply_extraction`, whose `Elapsed`-period case (`elapsed_annotation`,
`aggregation.rs:468-480`) is a fixed `"expected to end by HH:MM"` string
pinned to the extracted period's own `to_date` — not something that
changes cycle to cycle. The one place live-sample data reaches an
incident-derived reason (`escalate_from_sample_stats`'s `" (live samples
show: ...)"` suffix) **is** correctly stripped on both the write and read
sides. So a genuine, ongoing Knowledgebase incident collapses into one
clean span as designed; it's specifically the sample-inferred fallback
path that leaks unnormalized live text into what both sides treat as
identity.

### Contributing factor (secondary, evidenced): rail-day-boundary incident expiry feeds directly into the broken path above

`is_active` (`aggregation.rs:240-247`) — implemented per
`docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md` —
filters out any non-planned, non-recurring-schedule incident once `now`
passes `next_rail_day_boundary(first_seen_at)` (~24h after first seen,
02:00 Europe/London), **regardless of whether the incident is still
present and uncleared in the Knowledgebase feed**. This is deliberate
(designed to catch incidents whose free text says "resolved" while RDM's
own structured fields never get updated), but it means: a genuinely
still-ongoing, multi-day real-world disruption that isn't flagged
`is_planned` and has no matching recurring `schedule_window` will, ~24h
after it was first seen, stop producing an incident-derived `LineStatus`
at all — the line falls back to `infer_from_samples`, landing squarely in
the noisy, unnormalized path described above. A single real event can
therefore legitimately produce: (a) one clean incident-derived span while
it's fresh, then (b) a fragmented run of many sample-derived spans once it
ages out, for as long as the residual delays it caused keep showing up in
live departure data. This plausibly explains the WebFetch summary's
"trespassers on the railway generate numerous cascading entries" pattern —
one real event, later generations of entries are the aged-out fallback
path's noise, not the original incident repeating itself.

### Structural factor (secondary, minor): day-boundary span splitting

`groupHistoryByDay` (`frontend/lib/history.ts:135-150`) buckets recomputes
into London calendar days *before* calling `collapseDay`, by design (so an
evening doesn't split across a UTC date boundary, and so a long-flapping
incident's grouping stays bounded). One side effect: a genuinely continuous
incident spanning a London midnight is unavoidably rendered as two separate
rows under two separate day headings. This is real but minor next to the
primary cause — it doesn't multiply entries the way the reason-text churn
does, and only affects incidents that happen to straddle midnight.

### Ruled out: matcher-scope duplication (hypothesis (d) from the task brief)

`lines_affected_by` (`crates/aggregator/src/matcher.rs:39-65`) calls
`match_one` (`matcher.rs:67-156`) exactly once per `(incident, line)` pair.
`match_one` itself is a strict single-`Option` tiered fallthrough — it
checks station hits (classifying by segment into `ExclusiveSegment`/
`SharedSegment`/`StationHit`), then keyword hits, then operator overlap,
`return`ing at the **first** tier that matches — so one incident can never
produce two different-scoped `Match`es (and therefore two different
`LineStatus` entries) for the *same* line. Multiple concurrent
`LineStatus` entries for one line do happen (`aggregation.rs:75-80`: `for
loaded in incidents ... for m in lines_affected_by(...)`, one push per
match), but only when multiple genuinely **different** active incidents
each independently match that line — which is correct, desired behavior
(two real disruptions really are two entries), not the bug under
investigation. This specific hypothesis is not supported by the code.

## Possible directions (prioritized, not designed)

1. **Highest priority — normalize LDBWS-sample-derived `reason` text for
   identity purposes, on both sides.** Extend
   `normalize_entry_for_diff`/`strip_live_sample_annotation`
   (`queries.rs`) and `coreReason`
   (`history.ts`) to also neutralize the embedded live counts and the
   `"(most cited: ...)"` suffix `infer_from_samples` produces — e.g. by
   bucketing on `(severity, coarse cause)` rather than the literal
   sentence, mirroring how the escalation annotation is already
   special-cased. This is the most directly evidenced, largest-effect fix:
   it addresses both the write-side row-proliferation problem and the
   read-side span-proliferation problem with the same underlying
   normalization concept already established for the other annotation.
2. **Second priority — reconsider whether per-cycle live counts and
   "most cited" free text belong in the identity-bearing `reason` field at
   all**, versus living only in `disruption.description`/detail-only
   fields — mirroring how `sample_stats`/`sample_availability` are already
   deliberately excluded from `normalize_entry_for_diff`. This is a
   narrower, more targeted version of (1) worth weighing against it: fixing
   normalization keeps the detail available in `reason` but ignores it for
   identity; moving the detail out changes what `reason` means everywhere
   it's rendered, not just in the history list.
3. **Third priority — add a materiality threshold before a sample-derived
   change counts as "new."** E.g., require a severity-rank change or a
   cause-bucket change (not just a count/percentage wobble) before
   `write_line_status` treats it as `changed`. Overlaps significantly with
   (1)/(2); worth considering together rather than as a fourth independent
   knob.
4. **Lower priority — revisit whether rail-day-boundary expiry's fallback
   transition should read as continuation rather than a fresh sequence of
   entries** for a still-genuinely-ongoing (if stale-per-KB-fields)
   disruption. This is a downstream amplifier of (1), not an independent
   cause — fixing (1) alone would already collapse most of what this
   produces into far fewer entries. Revisiting the expiry policy itself
   risks reopening tradeoffs the stale-incident-handling design doc already
   settled deliberately (planned-work exemption, recurring-schedule
   exemption, rail-day convention), so this should stay low priority unless
   (1) turns out insufficient in practice.
5. **Lowest priority — UI-side collapsing/pagination as a backstop.** For
   lines that legitimately see many *distinct* real incidents (e.g. this
   Thameslink line's genuine `SharedSegment` exposure to Great Northern's
   Peterborough-branch incidents, per `lines/thameslink-cambridge.toml`),
   even a fully-normalized identity scheme will still show a real, possibly
   long, list. A "show more"/collapse-low-severity-or-short-duration-spans
   affordance is a reasonable complement regardless of whether (1)–(4) are
   pursued, but treats the symptom, not the cause identified above.
6. **Cosmetic — let a span visually continue across a London-midnight
   boundary** rather than resetting per day heading. Minor; not central to
   the "spammy" complaint, listed for completeness since the task brief's
   hypothesis (a) touches it.

## Explicitly out of scope

- Implementing any of the above (research only).
- The Trends tab/charts (`TrendsResults`, `line_status_daily_stats`,
  `line_status_hourly` rollups, `HourlyTrendsResults`) — confirmed to be a
  separate data source (daily/hourly rollup stats, not per-incident spans)
  from what the Timeline tab's `HistoryResults` renders; not investigated
  further per the task brief's own instruction to ignore it.
- Redesigning the matcher's `MatchScope` taxonomy — investigated
  specifically as a duplication hypothesis and ruled out; no other matcher
  changes considered.
- `dedup::dedup_new_sample_stats` (`crates/aggregator/src/dedup.rs`) — a
  different, unrelated dedup mechanism (departure/sample-count
  double-counting), noted only to distinguish it from the incident-identity
  problem this document investigates.
- A full normalization-scheme design for sample-derived `reason` identity —
  direction (1)/(2)/(3) above are sketched, not specified (thresholds,
  exact cause-bucketing scheme, and migration/backfill implications for
  existing `line_status_history` rows are all open).
- Revisiting the rail-day-boundary cutoff's own design tradeoffs (planned-
  work exemption, recurring-schedule exemption, the 02:00 convention
  itself) — direction (4) names it as a downstream amplifier but does not
  propose changing it.
- Setting up and driving a local docker-compose repro with Playwright — not
  attempted this pass; the real production page was reachable and gave
  stronger, directly-observed evidence than a synthetic local seed would
  have (see Method).

## Open questions/risks

- **`WebFetch`'s output is a summarized paraphrase, not verbatim page
  text.** The one figure I could cross-check exactly against the real
  frontend copy template ("200 status recomputes across 87 incidents")
  matches precisely, which is strong evidence the fetch reflects the real
  page rather than a fabricated answer. The more granular observations
  (specific phrasing like "3 of 5 sampled services delayed", the exact mix
  of causes named) are that summarizing model's characterization of the
  real page, not something this pass independently confirmed against raw
  JSON or raw HTML — the actual `LineStatusHistoryEntry[]` for this
  request lives behind an internal-only API route this environment
  couldn't reach directly (see Method). Confidence in "the mechanism is
  real and matches the code" is high; confidence in the *exact* wording
  quoted above is necessarily lower.
- **No raw `line_status_history` rows were inspected**, locally or in
  production, so the actual frequency of `reason`-text churn for this
  specific line (how often counts/most-cited-reason actually differ
  cycle-to-cycle, versus how often they happen to coincide) is inferred
  from the code's structure, not directly measured. The root-cause
  mechanism is solidly evidenced from the code; its precise magnitude for
  this line specifically is not.
- **No baseline comparison against other lines.** Whether 87 incidents/7
  days is unusually high for this specific line (a busy, `SharedSegment`-
  exposed Thameslink branch) versus typical across the catalogue was not
  checked — the `SharedSegment` exposure to Great Northern's Peterborough
  branch (see `lines/thameslink-cambridge.toml`) means this line
  legitimately sees more distinct real incidents than a simpler,
  non-shared-trunk line would, independent of the reason-text-churn bug.
  Disentangling "genuinely more incidents" from "the same incidents
  fragmented more" was not attempted quantitatively.
- **Whether `most_common`'s per-cycle scan of live departures'
  `delay_reason`/`cancel_reason` text is inherently this noisy in general**
  (as opposed to specifically noisy for this line's mix of operators/
  stations right now) was not independently assessed — it depends on how
  varied TOC-provided free-text delay reasons are in practice, which this
  pass did not measure.
- **This sandbox's Tailscale reachability into the target tailnet is
  itself an environment fact this task's brief explicitly did not expect**
  (it anticipated failure and instructed an immediate fallback). Future
  research passes in this repo should not assume the opposite either way —
  confirm reachability directly (as done here via `curl -v`) rather than
  relying on the general assumption in either direction.

## References

- `frontend/app/lines/[id]/history/page.tsx` — `HistoryResults`
  (lines 148–200, especially the copy template at 161–164).
- `frontend/lib/history.ts` — `coreReason`/`LIVE_SAMPLE_ANNOTATION`
  (lines 7–18), `collapseDay` (lines 84–125), `groupHistoryByDay`
  (lines 135–150).
- `frontend/lib/api.ts` — `getLineStatusHistory` (lines 123–133),
  `baseUrl` (lines 43–46).
- `crates/aggregator/src/aggregation.rs` — `aggregate` (lines 47–116,
  the incident/sample two-layer loop at 75–113), `status_from_incident`
  (lines 123–173), `elapsed_annotation` (lines 468–480), `infer_from_samples`
  (lines 884–966, the "most cited" append at 928–935), `classify`
  (lines 1010–1069), `is_active` (lines 240–247), `has_recurring_schedule`
  (lines 279–286+), `next_rail_day_boundary` (lines 208+).
- `crates/aggregator/src/queries.rs` — `write_line_status` (lines 273–324),
  `normalize_for_diff`/`normalize_entry_for_diff` (lines 163–192),
  `strip_live_sample_annotation` (lines 262–268), `prune_history`
  (lines 326–335).
- `crates/aggregator/src/matcher.rs` — `lines_affected_by` (lines 39–65),
  `match_one` (lines 67–156), `MatchScope` (lines 10–17).
- `crates/aggregator/src/dedup.rs` — `dedup_new_sample_stats`, an
  unrelated sample-count dedup mechanism, cited only to rule it out as the
  source of this problem.
- `docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md`
  (recovered via `git show 343e4a2`; not present under
  `docs/superpowers/plans/`, still present under `docs/superpowers/specs/`)
  — origin of the rail-day-boundary expiry mechanism discussed above.
- `docs/superpowers/specs/2026-08-31-sample-data-availability-design.md` —
  checked per the task brief's assumption that it documents local
  seed-data availability; it does not (it's about `SampleAvailability`/
  `SampleStats` wire-shape distinctions), so it did not inform a local
  repro attempt.
- `lines/thameslink-cambridge.toml` — the real line's segment/overlap
  shape, in particular its genuine `SharedSegment` relationship with
  `great-northern-kings-lynn.toml`'s `gn-peterborough-branch`.
- Real-page evidence: `curl -v` TLS/HTTP trace and `curl -sS -o ... -w
  "HTTP_STATUS:%{http_code}"` against
  `https://konata.fox-prometheus.ts.net/lines/thameslink-cambridge/history`
  (200, ~205KB HTML, valid Let's Encrypt cert); `WebFetch` against the same
  URL for a description of the rendered Timeline tab.
