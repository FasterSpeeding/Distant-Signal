# Design: Distinguishing "No Sample Data" From "Genuinely Quiet Right Now"

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` (the closest
structural precedent — a research-heavy audit of an ambiguous UI state,
ending in a tiered policy and concrete copy, not a schema-first design) and,
for how it treats a materially different but code-adjacent piece of
already-drafted work, `docs/superpowers/specs/2026-08-31-line-history-graphics-design.md`.
No implementation plan is included; that is a separate, later step in this
repo's process.

## Goal

Today, every rendering surface that shows sample-derived delay/cancellation
stats collapses two genuinely different situations into one string, `"No
sample data"` (`frontend/lib/sampleStats.ts`'s `formatSampleSummary`):

1. **This line's data source structurally never produces `SampleStats`
   via this app's own sampling** — a TfL-quality line (tube, Overground,
   Elizabeth line, tram; DLR is a real, currently-inactive exception, see
   below).
2. **This line's data source does produce `SampleStats`, and did this very
   cycle** — but the live count came back below the threshold this app
   requires before it will report a rate at all, so the field is `None`
   anyway. This can mean any of: genuinely few/no trains running right now
   (late night, early morning, an engineering closure); this line's chosen
   `sample_stations` are structurally too thin to ever reliably clear the
   threshold, quiet or not; or a real gap — a fetch failure or a station
   that has simply never been polled — that today is **silently
   indistinguishable from "quiet."**

A user has no way to tell these apart anywhere in the app. This document
enumerates the real, code-grounded reasons `sample_stats` ends up absent or
near-zero, decides how much of that distinction the existing data already
supports on the frontend alone (a lot, for case 1; none, for the three
sub-cases of case 2), and proposes what should change — including where a
small, real backend addition is justified and where it is not.

## Corrections to the brief's assumptions (recorded for posterity)

Following this repo's established "Corrections" precedent (first used in
`2026-08-29-journey-ticket-tracking-frontend-design.md`, reused since):
direct inspection of the code turned up several things the brief's framing
didn't establish precisely, materially affecting the taxonomy below.

1. **`compute_sample_stats` is not gated by `data_quality` — it runs for
   every national-rail line, every cycle, unconditionally.**
   `aggregate()`'s Layer 2 (`crates/aggregator/src/aggregation.rs:89-107`)
   calls either `infer_from_samples` (line has no incident-derived status
   yet) or `compute_sample_stats` (line already has one or more
   incident-derived statuses) for **every** line in the merged catalogue,
   every cycle, and attaches the result to every status the line currently
   carries. There is no branch anywhere that skips this because a line's
   incidents happen to read as `Knowledgebase`/`Planned` — the
   `2026-08-31-line-history-graphics-design.md` spec's own Correction 2
   already established this for the historical-rollup problem; it applies
   identically here, and this document does not re-derive it, only cites
   it. The practical consequence for this document's taxonomy: **for a
   national-rail line, `sample_stats` being absent has exactly one gate**
   — `relevant.len() < thresholds.min_sample_size` inside
   `compute_sample_stats` (`aggregation.rs:730-743`) — not "the line's
   incident text sounded like Knowledgebase wording."
2. **A `Tfl`-quality status is not unconditionally sample-stats-free —
   DLR is a real, code-present exception, currently inactive by config.**
   `crates/poller-tfl/src/main.rs`'s `poll_dlr_sample_stats`/
   `merge_dlr_sample_stats` attach a real `common::SampleStats` (from a
   separate Arrivals-vs-Timetable diffing pilot, one fixed station) onto
   the DLR line's `Tfl`-quality status. Every other TfL mode (tube,
   Overground, Elizabeth line, tram) hardcodes `sample_stats: None`
   (`crates/poller-tfl/src/schema.rs:148`). This pilot is gated behind
   `dlr_pilot_enabled`, which **defaults to `false`**
   (`crates/poller-tfl/src/config.rs:49-60`, its own comment: "the pilot
   was built without a real deployment to run it against... it stays off
   until the plan's Task 8 manual verification checklist has been run
   clean"). So today, in production, `data_quality == 'tfl'` does reliably
   imply `sample_stats` is absent — but that is a **currently-true fact
   about a feature flag's default, not a structural guarantee**, and any
   frontend logic that hardcodes "TfL quality → never show stats" (rather
   than deriving from whether `sampleStats` is actually present) would
   silently go stale the day this flag flips. See Decision 1.
3. **No staleness check exists anywhere on `station_samples`, at any
   layer.** `crates/aggregator/src/queries.rs`'s `load_station_samples`
   (`SELECT crs, polled_at, departures FROM station_samples`, no `WHERE`
   clause at all) loads every row in the table regardless of how old
   `polled_at` is, and `relevant_departures`
   (`aggregation.rs:682-692`) does `line.sample_stations.iter()
   .filter_map(|crs| samples.get(crs))` — a station whose row doesn't
   exist yet (never polled) is silently skipped, identically to a station
   whose row exists but is hours stale (`poller-ldbws` down or erroring).
   Grepping every crate for `polled_at` confirms it is written
   (`crates/api/src/data/queries.rs`'s upsert, `poller-ldbws/src/main.rs`)
   and read back raw in exactly two places — `load_station_samples`
   (ignored) and `crates/api/src/routes/freshness.rs`'s
   `last_stations_fetch`-style queries, which is a **different** table
   (`stations`, the reference-data catalogue, not `station_samples`).
   `/public/freshness`'s own module doc says this explicitly: *"Station-
   samples is deliberately omitted: it's per-station polling data, not one
   of the four sources this endpoint reports on."* **There is no signal
   anywhere in this app today — backend or frontend, global or per-line —
   that distinguishes "this line's live departure data is fresh and
   genuinely shows nothing running" from "this line's live departure data
   is stale, missing, or has never been fetched."** This is the sharpest,
   most concrete finding of this research pass — see Taxonomy case 2c.
4. **`report.computedAt` looks like a freshness signal and isn't one, for
   this purpose.** `write_line_status` (`crates/aggregator/src/queries.rs:258-296`)
   stamps `computed_at = NOW()` on `line_status`'s `ON CONFLICT` branch
   **unconditionally, every cycle, for every line** — independent of
   whether that cycle's `statuses` actually changed (the `changed` check
   only gates the separate `line_status_history` insert). `LastUpdated`
   (rendered on every `LineStatusCard`/detail page) therefore always shows
   a timestamp seconds-to-a-minute old whenever the aggregator process
   itself is alive, **regardless of whether that specific line's own
   `sample_stations` have any fresh (or any) LDBWS data behind them.** A
   viewer reading "Updated 1 minute ago" next to "No sample data" has no
   reason to suspect a data gap — the timestamp actively suggests
   everything is working normally.
5. **Every catalogue and custom line configures `sample_stations` — the
   "line has zero sample stations" branch is a real code path but not a
   currently-occurring one.** Audited directly: `grep -L
   "sample_stations" lines/*.toml` over all 107 catalogue TOML files
   returns zero matches (every file sets the key), and `grep -l
   "sample_stations *= *\[\]"` also returns zero (none set it to an empty
   array). Custom lines can't have an empty one either —
   `CustomLine::from`'s conversion (`crates/common/src/lib.rs:703-730`)
   sets `sample_stations: c.stations` verbatim, and `create_line`
   (`crates/api/src/routes/lines.rs`) rejects fewer than 2 stations at
   creation. So **"a line whose data source structurally can't sample
   because it has no `sample_stations` configured" does not occur in this
   app's actual data today**, for either catalogue or custom lines — it
   remains a real, worth-guarding-against shape in `LineDefinition`
   (`#[serde(default)]` on the field means a malformed/future TOML file
   *could* omit it), but the taxonomy below treats it as a defensive edge
   case, not a live problem to design a prominent UI state around.

## Current relevant state (verified 2026-09-01)

**`common::SampleStats`** (`crates/common/src/lib.rs:674-681`) — unchanged
from the line-history-graphics spec's own citation: `{ total, delayed,
cancelled, skipped, avg_delay_minutes }`, no provenance/reason field of any
kind.

**`common::LineStatus`** (`lib.rs:324-335`): `sample_stats:
Option<SampleStats>` and `data_quality: DataQuality` are independent
fields, both present on the same struct, with **no field anywhere linking
"why `sample_stats` is `None`" to a machine-readable reason** — that
absence is exactly what this document evaluates whether to fix.

**`DataQuality`** (`lib.rs:280-294`): `Knowledgebase` (default),
`LdbwsInferred`, `TrustInferred` (constructed nowhere, confirmed by the
`2026-08-30-inferred-time-ranges-design.md` spec's own repo-wide grep, not
re-run here since nothing in this pass suggests that's changed), `Planned`,
`Tfl`.

**Frontend surfaces rendering this ambiguity today** (every call site of
`firstSampleStats`/`formatSampleSummary`/`cancelledPercent`, confirmed by
grep across `frontend/`):

| Surface | File | What renders when `stats` is `undefined` |
|---|---|---|
| All Lines table, mobile subtitle | `app/lines/AllLinesTable.tsx:224` | `formatSampleSummary(stats)` → **"No sample data"** |
| All Lines table, desktop Avg Delay / Cancelled columns | `AllLinesTable.tsx:228-245` | `"—"` (already honestly non-committal — no change needed here) |
| Pinned-line dashboard card | `components/LineStatusCard.tsx:47-51` | Whole `<Text>` block omitted (`{stats && (...)}`) — no message at all, not even "No sample data" |
| Pinned-station dashboard row | `app/page.tsx:108-112` | Same conditional-omission pattern as `LineStatusCard` |
| Station detail page, per-line subtitle | `app/stations/[crs]/page.tsx:98` | `formatSampleSummary(stats)` → **"No sample data"** (unconditional — this call site does *not* guard on `stats` truthiness first, so it always renders the string, unlike the two rows above) |
| Line detail page, `RepresentativeInfo` card | `components/RepresentativeInfo.tsx:9-10` | Not rendered at all (`if (!withStats?.sampleStats) return null;`) — this component only ever shows the "stats present" case, and is out of scope for the "absent" messaging this doc addresses |

So three different treatments of the same underlying `undefined` already
coexist by accident (a real string, a silently omitted line, and — on the
station page — an unconditional real string with no guard at all) before
this document proposes anything new. Any fix should also converge these,
not just improve the wording.

**`DataQuality` is already surfaced as its own per-status provenance
badge** — `IssueList.tsx`'s `DATA_QUALITY_LABELS` (`knowledgebase` →
"Knowledgebase", `ldbws-inferred` → "LDBWS-inferred", `tfl` → "TfL", etc.)
renders next to every issue and is filterable via a `Chip` control. This
establishes the app's own existing precedent for "tell the user honestly
where a number came from" — the fix proposed below extends that precedent
to the sample-stats-absent case rather than inventing a new pattern.

**No catalogue field is exposed to the frontend that would let it know a
line's `sample_stations` configuration** (audited: `LineSummary` in
`crates/api/src/routes/lines.rs` carries `id`/`name`/`category`/
`operators`/`source` only; `LineDefinitionSummary` — the `/lines/{id}/definition`
endpoint — carries `stations`/`operators` only, deliberately built for the
custom-line edit form's pre-fill, not this purpose). Not that it would help
much given Correction 5 (structurally-empty `sample_stations` doesn't occur
today), but worth recording: even the coarsest structural check ("does this
line have any sample stations at all") isn't currently answerable from the
frontend without a new field.

## Root-cause taxonomy

Grounded entirely in `infer_from_samples`/`compute_sample_stats`/
`relevant_departures` (`aggregation.rs:667-809`) and the poller-tfl code
cited in Correction 2 — not a guessed list.

### Case 1 — Data source doesn't produce `SampleStats` via this app's own sampling

Applies to: every TfL-quality status except (currently-inactive) DLR.
`data_quality == 'tfl'` and `sample_stats` is unconditionally `None`
(`poller-tfl/src/schema.rs:145-148`). This is not a threshold miss or a
gap — this app simply has no LDBWS-equivalent live-departure sampling
pipeline for tube/Overground/Elizabeth line/tram; TfL's own published line
status is the only signal that exists, and it's already the most
authoritative one available for those modes.

**Sub-case 1a — DLR, pilot disabled (current default, per Correction 2):**
identical in observable behavior to every other TfL mode today.
**Sub-case 1b — DLR, pilot enabled:** `sample_stats` is genuinely present,
computed from a real (if narrow — one station, Poplar, outbound only)
independent measurement. A correct frontend rule must not special-case
"DLR" by name to decide which sub-case applies — see Decision 1.

### Case 2 — Data source is active, this cycle's count is below `min_sample_size`

Applies to: every national-rail (aggregator-path) line, whenever
`relevant_departures(line, samples).len() < thresholds.min_sample_size`
(default `3`, `Defaults::min_sample_size`, `crates/common/src/lib.rs:770-772`,
overridable per-line via `severity_overrides`). Per Correction 1, this is
the **only** gate — `data_quality` plays no role. This single `None`
outcome is actually three different underlying situations, indistinguishable
from each other in the data as it stands:

- **2a — Genuinely quiet.** The real, meaningful "too little happening
  right now" signal the brief names: late night, very early morning, an
  engineering closure, a rail-replacement-bus period with no LDBWS-visible
  train departures at the configured stations. The stations *were*
  successfully polled this cycle (`samples.get(crs)` returned `Some`) and
  genuinely have zero or near-zero departures in Darwin's rolling window.
  This is honest, current, correct data saying "not much is running" — not
  missing data.
- **2b — Structurally under-sampled.** The line's `sample_stations` choice
  is too narrow (too few stations, or low-traffic ones) to reliably clear
  `min_sample_size` even during genuinely busy periods — a configuration
  problem, not a live-quietness problem. **Not audited in this pass**
  (per Correction 5, no line currently has *zero* `sample_stations`, but
  whether some lines' non-empty station lists are nonetheless too thin to
  clear the threshold at peak times was not checked station-by-station
  against real traffic volumes — flagged as a real follow-up, see Open
  questions).
- **2c — A genuine gap.** Per Correction 3: a station in `sample_stations`
  has no row in `station_samples` at all (never successfully polled — a
  brand-new custom line, or `poller-ldbws` has never once succeeded for
  that CRS), or has a stale row (`poller-ldbws` down or erroring for an
  extended period, but the last-known departures — now hours old — either
  get silently reused as if fresh, or, if the departures themselves have
  since all departed/cleared Darwin's rolling window, produce the exact
  same `relevant.len() < min_sample_size` outcome as case 2a). **This case
  is real and currently invisible anywhere in the app** — not degraded
  gracefully with a "stale" flag, not surfaced in `/public/freshness`
  (which explicitly excludes `station_samples`), not distinguishable from
  2a by `report.computedAt` (which updates every cycle regardless, per
  Correction 4).

**2a vs. 2b vs. 2c cannot be told apart from any single aggregation
cycle's output.** 2a and 2b are both "successfully polled, genuinely
near-zero" from one cycle's point of view — the only thing that
distinguishes them is a *pattern over time* (does this line dip to `None`
only overnight, or does it sit at `None` most of the day, every day?).
2c is distinguishable from both **only** if `polled_at`/fetch-success
information is actually surfaced somewhere it isn't today.

## Where should this distinction be represented?

**Not one answer for all three sub-cases — the existing data already fully
supports Case 1, but genuinely cannot support 2a/2b/2c without new backend
work.**

### Decision 1: Case 1 (TfL-quality, no app-sampled stats) needs no new backend field — derive it from existing fields, value-driven, not by hardcoding a data-quality name

Everything needed is already on the wire: `status.dataQuality` and
`status.sampleStats`. The correct frontend rule is:

```ts
// Sketch, not final. Value-driven -- correct whether or not the DLR pilot
// (Correction 2) is ever turned on, with zero special-casing by line id or
// mode name.
function sampleAvailability(status: LineStatus): 'present' | 'not-sampled-by-app' | 'sampled-but-quiet' {
  if (status.sampleStats) return 'present';
  return status.dataQuality === 'tfl' ? 'not-sampled-by-app' : 'sampled-but-quiet';
}
```

This is deliberately **not** `status.dataQuality === 'tfl' &&
line.mode !== 'dlr'` or any other identity check on which line this is —
per Correction 2, the moment `dlr_pilot_enabled` flips to `true`, DLR's
status starts carrying a real `sampleStats`, which the `if (status.sampleStats)`
branch already correctly routes to `'present'` with no further change
needed anywhere. Hardcoding "DLR is special" would be both unnecessary and
a second, parallel place that fact would need to be kept in sync if the
pilot's scope ever changes (e.g. more stations). **No backend change is
proposed for Case 1.**

### Decision 2: Case 2c (a genuine gap) is worth a small, real backend addition — LDBWS freshness, not a full reason-enum

Per Correction 3, this is the one sub-case that is not just "hard to
distinguish from the frontend" but **actually invisible anywhere in the
running system today** — a `poller-ldbws` outage produces exactly the same
observable `None` as a quiet 3am. That is a materially worse failure mode
than 2a/2b (which are both, at least, honestly reflecting *something* real
about current traffic) because it silently degrades data quality with zero
signal, the same category of problem
`2026-08-30-inferred-time-ranges-design.md` fixed for `validity.from_date`
("the codebase already knows this is wrong, in its own words" —
`normalize_for_diff`'s own doc comment named the `from_date` staleness
before anyone closed the loop on it; `/public/freshness`'s own doc comment
does the same thing here, naming exactly what it excludes and why, without
anything filling the gap it names).

**Proposed, minimal addition — not a full "reason" enum on `SampleStats`:**
extend `/public/freshness`'s `DataFreshness` (`crates/api/src/routes/freshness.rs`)
with a fifth field, `stationSamples: Option<DateTime<Utc>>`, sourced from a
new `MAX(polled_at) FROM station_samples` query (mirroring the existing
`last_stations_fetch`-style helpers in `crates/api/src/data/queries.rs`
exactly). This is a **global** staleness signal (one timestamp for the
whole LDBWS pipeline, matching the granularity every other field on
`DataFreshness` already uses — none of `stations`/`tocs`/`incidents`/`tfl`
are per-line either), surfaced the same way the existing four fields are,
via `DataFreshnessInfo`'s nav-bar tooltip. It answers "is the LDBWS
pipeline as a whole currently working at all" — enough to let a viewer
who sees "No sample data" everywhere, all at once, understand that as a
systemic outage rather than every line coincidentally going quiet
simultaneously. It does **not** answer "is *this specific line's* data
stale" (a per-line/per-station signal), which is a larger, real follow-up
this document deliberately does not design (see Open questions) —
proposed here only because it is cheap, directly plugs a documented gap
this app's own code already names but doesn't fill, and requires no schema
change (`station_samples.polled_at` already exists and is already written
correctly by `poller-ldbws`; only a read query and a response field are
new).

**Explicitly rejected for now: a new `SampleStats`/`LineStatus` field
carrying a structured "why is this absent" reason** (e.g. `enum
SampleAvailability { NotSampled, BelowThreshold, Stale }` on `LineStatus`).
Considered and rejected for this document's scope on the grounds that:

- It would need to encode a per-line, per-cycle judgment about *why* the
  threshold wasn't cleared — which, per the taxonomy above, this app's own
  current data model has no way to compute correctly for 2a vs. 2b (both
  look identical from inside one aggregation cycle; only a pattern over
  many cycles could tell them apart, which is a different kind of data
  than a single cycle's `LineStatus` can carry).
  Building it now would mean either always reporting `BelowThreshold`
  (no better than what a value-driven frontend check on `sampleStats`
  already gives, since it can't actually say "quiet" vs. "under-sampled")
  or building the cycles-over-time machinery this doc's Decision 3 argues
  belongs to the line-history-graphics work instead, not duplicated here.
- The one sub-case (2c) that *is* cheaply and honestly answerable today
  (Decision 2's freshness field) doesn't need a `LineStatus`-level field at
  all — a global pipeline-health signal is enough to catch the case that
  actually matters (a real outage), without pretending to give a per-line
  verdict the underlying data can't support yet.

### Decision 3: 2a vs. 2b (quiet vs. structurally under-sampled) is not a live-status-page problem — defer to `line_status_daily_stats`'s `sample_cycles`, don't build a parallel mechanism

Per the taxonomy above, telling these two apart needs a *pattern over
time* — exactly what `2026-08-31-line-history-graphics-design.md`'s
proposed `line_status_daily_stats.sample_cycles` (that spec's Decision 1)
is built to answer: "how many of today's poll cycles actually had enough
live data to report." A line that's near-`min_sample_size` only between
02:00-05:00 every day (2a, genuinely quiet) will show a `sample_cycles`
dip confined to those hours across many days' rows; a line that sits near
`min_sample_size` at 09:00 on a Tuesday too (2b, structurally
under-sampled) will show it doesn't matter what time it is. **This is not
something a single live `LineStatus` object, however it's shaped, can ever
answer on its own — it requires the rollup that spec already designs.**
Building a second, separate "coverage over time" mechanism for the live
view, ahead of or parallel to that work, would directly duplicate it. See
Relationship to line-history-graphics below for the concrete sequencing
recommendation this implies.

## UI treatment

Concrete copy for each distinguishable state, replacing the single
`"No sample data"` string and converging the three inconsistent rendering
treatments found in Current relevant state.

**A single new shared helper should own this**, replacing
`formatSampleSummary`'s `if (!stats) return 'No sample data';` branch — every
call site listed in the table above should route through it rather than
each hand-rolling its own conditional-omission or unconditional-string
choice:

```ts
// frontend/lib/sampleStats.ts -- sketch, not final.
export function sampleAvailabilityMessage(status: LineStatus): string {
  if (status.sampleStats) return formatSampleSummary(status.sampleStats); // unchanged, real-data path
  if (status.dataQuality === 'tfl') {
    return 'Not measured by this app — status is TfL’s own.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}
```

Rendered copy, per case:

- **Case 1 (TfL, no app-sampled stats):** *"Not measured by this app —
  status is TfL's own."* Deliberately does not say "no data" (TfL's own
  status IS the data for this line, and is already shown as the badge/
  reason above this line) or "not available" (which would read as a
  fault). Names *whose* data this is, matching the existing
  `DATA_QUALITY_LABELS` precedent of naming provenance rather than judging
  it.
- **Case 2 (any of 2a/2b/2c, collapsed — because the app genuinely cannot
  tell them apart yet, per Decision 2/3):** *"Too few live departures
  sampled to report a rate right now."* Chosen over the brief's own
  candidate wording ("this line just doesn't have this kind of data") and
  over a more confident-sounding "No trains running right now" specifically
  **because** it must not overclaim in the 2c case (a real gap dressed up
  as calm quiet would be worse than today's honestly-vague "No sample
  data" — see Correction 3). "Sampled ... right now" is accurate in all
  three sub-cases: the app did attempt to sample this cycle (true in 2a,
  2b, and — silently — even in 2c, since the failure is invisible to the
  aggregator itself, not something it can honestly claim as "we tried and
  failed"), and the count came back too low to report, which is also true
  in all three. Once Decision 2's freshness field ships, this string can
  be conditionally suffixed system-wide (not per-line) when
  `stationSamples` is stale, e.g. appending *" (live departure data may be
  delayed — see the freshness indicator)"* — a small, additive change to
  this same string once that data exists, not a prerequisite to shipping
  the string itself.
- **The desktop numeric columns in `AllLinesTable`** (`"—"` for both Avg
  Delay and Cancelled when `stats` is absent) need no wording change — a
  dash in a numeric column already correctly reads as "no number here" in
  either case; only the two `formatSampleSummary`-driven text surfaces
  need the richer copy above. Optionally, wrap the dash in a `Tooltip`
  reusing the same `sampleAvailabilityMessage(status)` string for parity
  with the text rows, but this is polish, not required.
- **`LineStatusCard` and the pinned-station dashboard row** (currently
  silently omit the whole line rather than rendering anything): change
  from `{stats && (<Text>...)}` to always rendering
  `sampleAvailabilityMessage(worst)` (or the representative status), for
  consistency with the other two surfaces — a card that says nothing at
  all about sample data reads, on a dashboard specifically built to answer
  "is my line OK", as an accidental omission rather than a deliberate
  "we checked, there's nothing to report" statement. This is the one
  behavior change beyond wording this document proposes: **stop omitting
  the line entirely on these two surfaces.**

## Relationship to line-history-graphics

**Explicit recommendation: this document's Decision 1 (Case 1, TfL) and
Decision 2 (Case 2c, LDBWS freshness) are independent, small, and can ship
before, after, or fully decoupled from line-history-graphics — neither
depends on the daily rollup existing.** Decision 3 (Case 2a vs. 2b) is
**not** independent — it is explicitly deferred to that spec's
`sample_cycles` rollup rather than solved here, per the reasoning above,
so *that* part of this problem should not be considered "done" until (or
unless) the line-history-graphics Trends tab ships and a "genuinely quiet
vs. structurally under-sampled" affordance is added on top of it (e.g. a
line detail page that, on seeing a persistent live `None`, links to
"View trends for this line" rather than trying to answer the question
itself).

**Vocabulary alignment, concretely:** the line-history-graphics spec's
Decision 2/7 commit to phrasing every rolled-up rate as "share of sampled
poll cycles," specifically to avoid overclaiming "share of trains" given
the same-service-recounted-every-poll issue that spec's Correction 5
documents. This document's live-view copy ("Too few live departures
sampled to report a rate right now") is written to be consistent with that
framing — both describe the underlying LDBWS signal as *sampled poll
activity*, not *trains* — without literally reusing the word
"sample_cycles" (a rollup-specific term that wouldn't mean anything to a
user on the live status page, which has no cycles-over-time concept to
attach it to). If a future affordance links directly from a live line
card to that line's Trends tab (per the recommendation above), the two
surfaces' wording should be re-checked together at that point for drift,
since they'll be visible side-by-side for the first time.

**Not duplicated:** this document does not propose any new "coverage over
N cycles" counter, table, or endpoint of its own — Decision 3 explicitly
routes that need at the one already-designed for it. The only new backend
surface this document proposes (Decision 2's `stationSamples` field on
`/public/freshness`) is a global pipeline-health timestamp, a different
shape and a different question ("is LDBWS ingestion working at all right
now") from `sample_cycles` ("how much has this specific line been
measured over a day") — the two are complementary, not overlapping.

## Explicitly out of scope

- **A structured "why is sample_stats absent" enum on `LineStatus`/`SampleStats`.**
  Considered and rejected in Decision 2 for the reasons given there — the
  data to compute it correctly for 2a/2b doesn't exist in a single
  aggregation cycle, and the one sub-case that is cheaply answerable
  (2c) is better served by a global freshness field.
- **Per-line/per-station LDBWS freshness** (as opposed to Decision 2's
  single global timestamp). A real, larger follow-up — would need either
  a new per-line aggregate query or exposing raw `station_samples.polled_at`
  values per CRS, and a decision about how to roll many stations' freshness
  up into one line-level signal (oldest? newest? a threshold count?) that
  this document does not attempt to settle.
- **Auditing which of the 107 catalogue lines' `sample_stations` choices
  are structurally under-sampled (Case 2b)** at real traffic volumes — not
  performed in this pass (Correction 5 only confirms none are *empty*, not
  that all are *sufficient*); flagged as real follow-up work, likely best
  done once `sample_cycles` data exists to check against directly rather
  than guessed at from station lists alone.
- **Any change to `min_sample_size`'s default or per-line
  `severity_overrides`** — this document is about honestly labeling the
  `None` outcome, not about tuning when it fires.
- **The DLR arrivals-diffing pilot's own correctness/rollout** — treated
  here only as a fact this document's Decision 1 must design around
  (per Correction 2), not itself in scope to evaluate or advance.

## Open questions / risks

1. **Whether Decision 2's global `stationSamples` freshness field is worth
   shipping on its own, ahead of any per-line signal, is a real product
   call this document doesn't settle** — it closes a real, currently
   completely-invisible gap (Correction 3) cheaply, but a global timestamp
   is a blunt instrument: a partial `poller-ldbws` outage (some stations
   fine, others down) would still show as "fresh" globally as long as any
   station anywhere succeeded recently. Flagged as a real, known
   limitation of the minimal fix proposed, not silently glossed over.
2. **Case 2b's audit (structurally under-sampled lines) has no owner or
   timeline proposed here.** Whether it's worth doing manually now, or
   waiting for real `sample_cycles` data to make it a data-driven exercise
   instead of a station-list-reading one, is left open — leaning toward
   the latter given Decision 3's reasoning, but not decided.
3. **The exact wording proposed under UI treatment is this document's own
   best attempt, not user-tested copy** — same posture the anonymous-user-
   ux-design spec takes with its own proposed strings; a real product/copy
   review before shipping is expected, same as that document's own
   precedent.
4. **This document does not resolve whether `LineStatusCard`'s and the
   pinned-station row's behavior change (always rendering a sample-
   availability line instead of omitting it) is a net UX improvement or
   just adds a permanently-present, mostly-unhelpful line to every card
   for the (currently common) case of a national-rail line that's simply
   quiet.** Recommended on consistency grounds (three different treatments
   of the same absence today is itself a bug), but flagged as a genuine,
   not fully closed, product judgment call.
