# Station Catalogue Completeness — Batched Fill-In Plan

> **For agentic/human workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This is a data-curation plan, not a "build feature X" plan** — same
> framing as `docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md`,
> which this plan is the direct sequel to. There is no new crate, no
> migration, no route, no poller, no aggregator/matcher code change,
> **and no new `lines/*.toml` file**. Every task edits an existing,
> already-reviewed line file in place, adding `[[stations]]` entries at
> their correct geographic position. Treat every "Step 1/2: research" below
> as load-bearing, not boilerplate — this catalogue's whole convention is
> "no invented station lists," and that applies exactly as much to a minor
> infill station as it did to an entire new file.

## Status: COMPLETE (audited 2026-09-01, same day as this plan's own date)

**All 69 file-level tasks across all 10 batches, plus every batch's own
verification task, were already executed on this branch before this audit
began** — in a prior session (or sessions) that ran this exact plan (or an
identical earlier draft of it) task-by-task, but never checked off this
document's own boxes. This audit found and confirmed that work rather than
redoing it; the checkboxes throughout this document were flipped to `[x]`
to reflect reality, not to record new work done during the audit itself.

Evidence (all independently re-verified 2026-09-01, not taken on faith
from commit messages alone):

- **42 `git log --oneline -- lines/` commits** matching this plan's own
  `lines(<id>): add missing stations (...)` / `lines(<id>): confirm no
  missing stations (...)` commit-message convention exist on this branch,
  one (or more, where a review-fix landed a follow-up correction) per
  `[FILL-IN]`/`[BRANCH-RESEARCH]` task in every one of the 10 batches —
  including both `[BRANCH-RESEARCH]` pieces (`scotrail-glasgow-suburban.toml`'s
  Lanarkshire branch group — Whifflet, Coatbridge Central, the Hamilton
  Circle, Larkhall and Lanark branches, all present with their own segment
  names; `c2c.toml`'s Rainham branch — Dagenham Dock, Rainham, Purfleet;
  `southeastern-chatham.toml`'s Minster–Martin Mill coastal loop, landed as
  a new `chatham-deal` segment cross-referenced, not duplicated, from
  `southeastern-highspeed.toml`'s own comments).
- Every `[VERIFY]` task's target file (all 30+ of them: ScotRail's 11,
  Greater Anglia's 6, EMR's 3, TPE's 1, LNER/Chiltern/Merseyrail's 4,
  TfW's 4, GWR's 1, Thameslink/GN's 2, plus `northern-esk-valley.toml`)
  has **no corresponding `lines(` commit and no working-tree changes**,
  and each still carries its own unchanged "TIPLOC-only" / "no intermediate
  stations are omitted" comment this plan predicted it would — exactly the
  outcome the plan's own convention calls for ("A `[VERIFY]` task that
  genuinely makes no file change needs no commit at all").
- Spot-checked several `[FILL-IN]` files' actual diffs against the plan's
  own named starting lists and found the real work went *further* than
  copying the plan's suggestions verbatim — e.g. `gwr-west-of-england.toml`
  only added 4 of the 17 named candidates (Kintbury, Hungerford, Bedwyn,
  Pewsey) after live research found the other 13 don't belong there
  (6 sit on a different Reading-based service, 6 are historically closed,
  Frome isn't on this line's calling pattern) — and one file
  (`southeastern-highspeed.toml`) has an explicit "review fix" commit that
  *walked back* an earlier addition (Canterbury West, Thanet Parkway) once
  a re-check found its sourcing didn't clear the two-independent-source
  bar. Both are exactly the disciplined, non-invented-data behaviour this
  plan's Global Constraints require, not shortcut-taking.
- `crates/aggregator/src/matcher.rs` carries the Step 4 regression test(s)
  this plan's Testing convention requires for essentially every
  `[FILL-IN]`/`[BRANCH-RESEARCH]` file (e.g.
  `xc_manchester_recognises_newly_added_stations`,
  `wmr_snow_hill_recognises_newly_added_stations`,
  `gwr_cornish_main_line_plymouth_area_infill_stations_present`,
  `c2c_rainham_branch_stations_are_in_the_catalogue`,
  `scotrail_glasgow_suburban_new_whifflet_branch_incident_does_not_propagate`,
  `chiltern_main_line_has_previously_omitted_birmingham_approach_stations`,
  `southern_brighton_main_line_fillin_stations_are_now_modelled`).
- `cargo test -p aggregator -p common -p api` passes in full on this
  branch as of this audit (370 aggregator tests, 27 common tests, 163 api
  tests, 0 failures) and `git status` is clean — no uncommitted `lines/`
  or `matcher.rs` changes are sitting outstanding.
- Cross-batch segment-naming coordination the plan calls out explicitly
  was done correctly and without collision: `northern-wharfedale.toml`'s
  Frizinghall addition cites `northern-airedale.toml`'s Task 2.1
  "RESOLUTION" by name; `scotrail-glasgow-suburban.toml`/`scotrail-shotts.toml`
  got a dedicated follow-up commit fixing an initially-undisclosed UDD/BLH
  overlap; `chatham-deal` (Batch 5) appears as a real segment only in
  `southeastern-chatham.toml`, referenced (not duplicated) in
  `southeastern-highspeed.toml`'s comments.

**Net effect: this audit found nothing left to implement.** No new
`lines/*.toml` edits, no new `matcher.rs` tests, and no new commits to the
`lines/` catalogue were needed or made as part of this task — only this
document's own checkboxes were stale. See the session's final report for
the full accounting.

## Blockers

**None.** Every prerequisite this plan depends on is already in place,
confirmed by direct inspection while writing this plan (2026-09-01):

- The sourcing method (Wikipedia infobox/article cross-checked against a
  second independent source — railwaycodes.org.uk, National Rail
  Enquiries' `nationalrail.co.uk/stations/<crs>/details.html` pages, or a
  TOC's own site) needs no new tool, credential, or API access; it's the
  same method every existing `lines/*.toml` file, including all 69
  affected by this plan, was already built with.
- The runtime code path this plan's output feeds (`has_station`,
  `segment_for`, `SegmentRegistry`) needs zero changes — confirmed directly
  in the spec (`docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`,
  "Runtime behavior" section) and re-confirmed here by reading
  `crates/common/src/lib.rs` and `crates/aggregator/src/segments.rs`.
- The 15 files the spec flagged as hitting a disclosed tooling failure
  (railwaycodes.org.uk per-letter page truncation, a hit `WebFetch`
  session-usage limit) are a property of the *curation session that wrote
  them*, not of the stations themselves or of any tool available today —
  nothing about this repo or its tooling prevents a fresh fetch from
  working normally in a new session.
- The disclosed tooling failures for three of those 15 files
  (`tfw-heart-of-wales.toml`, `tfw-marches.toml`,
  `tfw-valley-lines-south.toml`) turn out, on direct re-inspection for
  this plan, to have affected only TIPLOC/second-source-CRS
  cross-verification for stations already listed — not missing stations —
  so there's nothing blocking those specific tasks either; see the Status
  note below and each task's own note.

If a future implementer hits a genuine live blocker mid-task (e.g. a
source site is down for an extended period), that blocks only that one
task, not this plan — leave the station out with a comment per the
sourcing bar below and move to the next task.

**Goal:** Close the station-catalogue coverage gap identified in
`docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
(roughly 300–450 real, currently-open, currently-served stations missing
from `lines/*.toml`'s `[[stations]]` lists across 69 of 107 files) so that
`GET` requests to the `/stations/[crs]` lookup page
(`crates/api/src/routes/line_status.rs`'s `get_stop_point_disruption`)
stop silently returning an empty disruption list for a real station this
catalogue simply never listed — today indistinguishable from a station
with genuinely good service. Every task adds real, verified stations to
an existing, already-reviewed file; none of this plan creates a new file,
changes a segment-matching algorithm, or widens LDBWS sampling scope (see
Global Constraints).

**Architecture:** No architecture changes, identical in shape to
`line-catalogue-coverage.md`. Every task uses the existing `lines/*.toml`
schema (`lines/SCHEMA.md`) and the existing
`LineDefinition::from_dir`/`SegmentRegistry`/matcher pipeline exactly as
it already works. Per the spec's "Runtime behavior" section (re-confirmed
directly against `crates/common/src/lib.rs:456-486` and
`crates/aggregator/src/segments.rs` while writing this plan): a minor
station inserted strictly between two already-modelled stations on the
same physical stretch needs no new segment-boundary judgment call — it
simply inherits the `segment` name its immediate neighbours already
carry. The one place correctness *does* depend on the implementer: station
order. `stations_between()` (`crates/common/src/lib.rs:475-485`) slices
the station list by index position to resolve "lines blocked between A
and B" incident messages, so every new station must be inserted at its
true geographic position, never appended at the end of the list.

**Tech Stack:** TOML data files under `lines/`; Rust 2024 edition tests
(`cargo test -p aggregator -p common -p api`) as the verification
mechanism — no new dependencies, matching `line-catalogue-coverage.md`.

**Spec:** `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
— read in full before starting; this plan does not re-derive its
quantification (69/107 files, ~300–450 stations) or its bucket analysis
(a: genuinely couldn't source in time; b: not prioritized because not
needed for segment boundaries — the dominant pattern; c: whole unconfirmed
branch/loop continuations, out of scope for routine fill-in), only carries
those findings into concrete, file-level tasks. Cross-references below to
"bucket (a)/(b)/(c)" refer to that document's "Why the omissions happened"
section.

**Status note:** confirmed by direct inspection while writing this plan
(2026-09-01), refining (not contradicting) the spec's own numbers:

- `grep -lI omitted lines/*.toml | wc -l` returns **69**, exactly matching
  the spec's own headline count — the file-level scope below is the same
  69 files the spec quantified, enumerated here explicitly for the first
  time as a concrete task list.
- Read every one of those 69 files' own "omitted"/scope-boundary comments
  directly (not just the spec's representative sample) to classify each
  file's *actual* task shape before writing its task below. Three
  classifications are used throughout this plan, assigned per-file from
  that direct read:
  - **`[FILL-IN]`** — the file's own comments name specific missing,
    currently-open stations (bucket a/b), or use the unenumerated
    "minor intermediate calls are omitted" boilerplate needing a fresh
    route-diagram pass to even produce a candidate list. This is the real
    work this plan exists to do.
  - **`[VERIFY]`** — direct inspection found the file's only "omitted"
    hits are TIPLOC-only, `match_keywords`-only, `destination_crs_filter`-
    only, or an explicit "no intermediate stations are omitted" statement
    already in the file — i.e., *not* a `stations`-list gap at all. These
    tasks exist to confirm that classification still holds (comments can
    go stale) and, optionally, to close the smaller TIPLOC/second-source
    gap if one exists — never to invent a station-list change where none
    is needed.
  - **`[BRANCH-RESEARCH]`** — bucket (c): a whole unconfirmed branch or
    loop continuation, not a "one more minor station" fill-in. These carry
    their own segment-boundary research, sized like an original
    `line-catalogue-coverage.md` new-file task, not routine infill.
- This reclassification surfaces two corrections to the spec's own
  "Batching rationale" section, worth stating plainly since they affect
  where real work lands (neither changes the spec's headline 69-file/
  300–450-station figures, which already include these files):
  1. The spec's closing paragraph says "Chiltern/c2c/Merseyrail don't need
     a repeat pass (their files have no omission language at all)." Direct
     inspection shows this holds for `chiltern-aylesbury.toml`,
     `merseyrail-northern.toml`, and `merseyrail-wirral.toml` (all
     `[VERIFY]`, TIPLOC-only or explicit no-gap), but **not** for
     `chiltern-main-line.toml` (`[FILL-IN]`: names Sudbury & Harrow Road,
     Denham, Beaconsfield, Princes Risborough, plus a whole
     Hatton–Bordesley Birmingham-approach stretch) or `c2c.toml`
     (`[BRANCH-RESEARCH]`: the unmodelled Rainham branch). Both are
     included below, in Batch 10.
  2. `thameslink-bedford.toml`'s disclosed "omitted" language, read in
     full, turns out to be about `destination_crs_filter` precision for
     an unconfirmed peak turn-back/short-working pattern — **not** about
     its `stations` list, which its own sourcing comment confirms is
     independently verified complete, station-by-station, Bedford through
     St Pancras. This is a different kind of gap than this plan's scope
     (`destination_crs_filter` accuracy, not station-catalogue
     completeness) — Batch 5's task for this file is a `[VERIFY]`
     confirming no `stations` edit is needed, explicitly not a
     `[BRANCH-RESEARCH]` task, correcting the spec's bucket-(c) framing of
     it.
  - Also notable, not a correction but worth flagging for batch-sizing:
    all 6 Greater Anglia files' (Batch 6) "omitted" hits read, on direct
    inspection, as correctly-reasoned exclusions (a station not on that
    line's own calling pattern, a closed or not-yet-open station, a
    non-passenger junction) rather than named real gaps — unlike GWR/
    Northern/WMR's files. Batch 6 is sized accordingly: six `[VERIFY]`
    tasks, not six large fill-in tasks. Similarly, 11 of ScotRail's 12
    files (Batch 3) are `[VERIFY]` — only `scotrail-glasgow-suburban.toml`
    carries a real named gap.
- Re-confirmed the spec's three "don't fix" closed-station false
  positives directly: `gwr-bristol-suburban.toml`, `emr-rural-branches.toml`,
  and `tfw-valley-lines-north.toml` each correctly exclude genuinely
  closed/historical stations, verified via each excluded station's own
  Wikipedia "status: disused" field — these are correct exclusions, not
  gaps, and no task below touches them.
- Total 1,731 `[[stations]]` entries today
  (`grep -c '^\[\[stations\]\]' lines/*.toml | awk -F: '{s+=$2} END {print s}'`),
  matching the spec's own baseline figure.

## Global Constraints

- **NEVER add, remove, reorder, or otherwise touch any line's
  `sample_stations` field. This applies to every task in this plan,
  without exception, even when a newly-added station sits in the literal
  middle of an already-sampled corridor or immediately next to an
  existing `sample_stations` entry.** `sample_stations` is a separate,
  deliberately narrow, hand-curated, LDBWS-polling-cost-driven field
  (`lines/SCHEMA.md`: "CRS codes to poll for LDBWS sampling") — entirely
  unrelated to `stations`, the field this plan grows. Adding a station to
  `stations` is a pure catalogue/matching-correctness change; it never
  implies, suggests, or justifies also adding that station to
  `sample_stations`. If any task's diff touches `sample_stations` for any
  reason, that diff is out of scope for this plan and must be reverted
  before commit — this is the single easiest thing to get wrong on this
  plan (see the spec's own "Scope boundary" section) and review should
  reject it on sight.
- **Never lower the two-source verification bar.** Every station, CRS
  code, and segment fact added by this plan must be independently
  verified against two live sources (Wikipedia infobox/article
  cross-checked against a second independent source — typically
  railwaycodes.org.uk, National Rail Enquiries' station detail pages, or
  a TOC's own site) — the identical bar `line-catalogue-coverage.md`
  already codified. A station that can't be confirmed to this standard
  stays out, with a comment explaining what wasn't confirmed. Guessing a
  CRS code, station name, or segment membership to "finish" a task is
  never acceptable, no matter how confident the guess feels.
- **No new `lines/*.toml` files.** Every task in this plan modifies an
  existing file. If a task's research reveals the "gap" is actually an
  entire unmodelled line (not a branch/loop off an existing line), that's
  new-file work belonging to a different plan (comparable to
  `line-catalogue-coverage.md`), not this one — flag it in the task's
  notes and stop, don't invent scope.
- **Insert every new station at its true geographic position, never
  appended at the end of the file's `stations` list.** Per the spec's
  Runtime Behavior finding, `stations_between()` slices by index position;
  a misordered insertion doesn't break compilation but silently corrupts
  "between A and B" incident-range resolution for any incident that
  happens to span the new station. This is real and easily avoided — take
  it as seriously as the original plan's segment-boundary judgment calls.
- **No new segment name for a station strictly between two already-
  modelled stations on the same physical stretch.** It inherits the
  `segment` its immediate neighbours already carry — this is the one
  piece of research the spec confirms this batch of work does *not* need
  to redo. A genuinely new segment-boundary decision is needed only for
  `[BRANCH-RESEARCH]` tasks (bucket c) and is called out explicitly where
  it applies.
- **Do not "fix" the three closed-station false positives.**
  `gwr-bristol-suburban.toml`, `emr-rural-branches.toml`, and
  `tfw-valley-lines-north.toml` each correctly exclude real, verified
  closed/historical stations (see the Status note). No task adds any of
  those specific stations back; re-litigating them wastes a task slot on
  work already done correctly.
- **A file matching this plan's `[FILL-IN]`/`[VERIFY]`/`[BRANCH-RESEARCH]`
  tag is this plan's own classification as of 2026-09-01, not a
  guarantee.** Every task's Step 1 (see the Recipe) requires re-reading
  the file's own comments before editing — if a fresh read shows the tag
  was wrong (e.g. a `[VERIFY]` file turns out to have a real gap this
  plan's author missed, or a `[FILL-IN]` file's named stations turn out
  to already be listed), follow what the file and live sources actually
  say, not this plan's label.
- **No aggregator/matcher/API code changes anywhere in this plan.** Per
  the spec's "Runtime behavior" section, `has_station`, `segment_for`,
  and `SegmentRegistry` are already correct for arbitrary station counts
  — this plan only grows their input data. The only non-`lines/*.toml`
  file any task touches is `crates/aggregator/src/matcher.rs`, and only to
  add a regression test per the Testing convention below — never
  production logic.
- **Testing convention (binding on every `[FILL-IN]`/`[BRANCH-RESEARCH]`
  task):** add at least one new `#[test]` to `crates/aggregator/src/matcher.rs`
  (reuse the file's existing `load_line`/`load_all_lines` helpers) that:
  1. Asserts `has_station` now returns `true` for at least one newly-added
     station in this file (via the line loaded through
     `LineDefinition::from_dir`).
  2. If the newly-added station's segment is shared with a sibling line
     already in the catalogue, also asserts the resulting `MatchScope`
     (mirrors the existing shared-segment/exclusive-segment test pairs
     already in that file, e.g. `swr_shared_trunk_incident_propagates`).
     Skip this second assertion, and say so in the task, when the new
     station's segment has no sibling.
  `[VERIFY]` tasks that end up making no `stations` edit need no new test
  (nothing new to regress-guard) — note this explicitly in the task
  instead of skipping it silently.
- **Commit per task**, one commit per file (`git commit -m "lines(<id>):
  add missing stations (<short description>)"`, or `"lines(<id>): confirm
  no missing stations (<what was verified>)"` for a `[VERIFY]` task that
  changes nothing). A `[VERIFY]` task that genuinely makes no file change
  needs no commit at all.
- **Out of scope for this entire plan, every batch:**
  - No `sample_stations` changes (restated above because it's the easiest
    mistake, not because one mention is enough).
  - No new pollers, feeds, or Knowledgebase/LDBWS/TRUST wiring changes.
  - No frontend changes — confirmed by the spec, `/public/lines` and the
    line-detail/station-lookup pages already render whatever the
    catalogue contains; nothing hardcodes a station list.
  - No `destination_crs_filter`/`headcode_prefixes`/`match_keywords`
    tuning as a goal in itself. A task may need to *read* these fields to
    understand a line's real calling pattern, but editing them is out of
    scope unless a task explicitly says otherwise (none currently does —
    see the `thameslink-bedford.toml` correction above for the case that
    would have looked like this kind of task and isn't one).
  - No re-litigating this plan's own file classification as a plan-level
    activity — if a task's Step 1 finds its tag was wrong, fix that one
    task's scope and move on; don't stop to re-audit the whole plan.

---

## Per-File Task Recipe

Every task below follows this same shape. Individual tasks give the
file-specific *content* (which stations, what's already known about them,
any segment-naming note); this section is the mechanics, defined once
rather than repeated 69 times.

- [x] **Step 1: Re-read the file's own comments in full**, specifically
  its scope-boundary/"omitted" section, before touching anything. Confirm
  this task's `[FILL-IN]`/`[VERIFY]`/`[BRANCH-RESEARCH]` tag still holds
  (see Global Constraints — this plan's tags are current as of
  2026-09-01, not guaranteed). Note the file's existing segment names
  near where new stations will land.

- [x] **Step 2 (FILL-IN/BRANCH-RESEARCH only): Research and confirm each
  candidate station to the two-source bar.** Start from the task's own
  starting list (named stations where the spec/this plan already found
  them; otherwise a fresh route-diagram read, e.g. Wikipedia's line
  article and its `Template:...` route-diagram box) — this starting list
  is never exhaustive; confirm it's complete against the line's real
  current stopping pattern before moving on. For each station: confirm
  CRS code (cross-checked against a second source — codes have been wrong
  in this catalogue before, e.g. `scotrail-glasgow-suburban.toml`'s own
  sourcing notes catching several wrong guesses), and TIPLOC where
  confirmable (optional, don't block on it). Leave out, with a comment
  explaining what wasn't confirmed, anything that doesn't clear the bar
  — never guess.

- [x] **Step 3 (FILL-IN/BRANCH-RESEARCH only): Insert each confirmed
  station** as a new `[[stations]]` entry at its correct geographic
  position (never appended at the end). Tag `segment` with the name
  already covering that stretch, inherited from immediate neighbours —
  for `[BRANCH-RESEARCH]` tasks, apply `SCHEMA.md`'s shared-trunk rule of
  thumb fresh, the same as an original new-file task would. Update or
  remove the file's own comment describing the now-filled gap so
  comments stay accurate — don't leave a stale "X is omitted" note next
  to a station this task just added.

- [x] **Step 4 (FILL-IN/BRANCH-RESEARCH only): Add the regression
  test(s)** required by the Testing convention above, in
  `crates/aggregator/src/matcher.rs`.

- [x] **Step 5: Run the tests.**

```bash
cargo test -p aggregator -p common -p api
```

  Expected: all pass, including any new test(s) from Step 4 and the full
  pre-existing suite (confirms `lines/` still parses as a whole and no
  existing shared-trunk/exclusive-segment assertion broke).

- [x] **Step 6: Commit** (skip entirely if a `[VERIFY]` task made no file
  change).

```bash
git add lines/<id>.toml crates/aggregator/src/matcher.rs
git commit -m "lines(<id>): add missing stations (<short description>)"
```

---

## Batching rationale

69 files is too much for one task list, same reasoning as
`line-catalogue-coverage.md`. This plan batches by TOC/region, mirroring
that plan's own cluster boundaries (same files, since this plan edits
what that one created) rather than the spec's alternative
named/unenumerated/tooling-failure split — a TOC/region batch keeps
segment-naming coordination (the one place two files in the same corridor
can interact) inside a single batch, the same reason the original plan
used this grouping. This produces **10 batches**, one task per file
(69 file-level tasks total) plus a short verification task closing out
each batch:

| # | Batch | Files | Shape |
|---|---|---|---|
| 1 | GWR | 6 | 5 `[FILL-IN]`, 1 `[VERIFY]` (don't-fix) |
| 2 | Northern | 11 | 8 `[FILL-IN]`, 3 `[VERIFY]` |
| 3 | ScotRail | 12 | 1 `[FILL-IN]`+`[BRANCH-RESEARCH]`, 11 `[VERIFY]` |
| 4 | Transport for Wales | 7 | 3 `[FILL-IN]`, 4 `[VERIFY]` |
| 5 | Southeastern + GTR | 6 | 2 `[FILL-IN]`, 2 `[BRANCH-RESEARCH]`, 2 `[VERIFY]` |
| 6 | Greater Anglia | 6 | 6 `[VERIFY]` |
| 7 | East Midlands Railway | 4 | 2 `[FILL-IN]`, 2 `[VERIFY]` |
| 8 | TransPennine Express | 4 | 2 `[FILL-IN]`, 2 `[VERIFY]` |
| 9 | WMR/LNWR + XC + WCML | 7 | 3 `[FILL-IN]`, 4 `[VERIFY]` |
| 10 | LNER + Chiltern + c2c + Merseyrail | 6 | 1 `[FILL-IN]`, 1 `[BRANCH-RESEARCH]`, 4 `[VERIFY]` |

Batches are independently executable — no batch's tasks depend on another
batch's output, since every task edits its own file and the "inherit the
neighbour's segment" rule needs no cross-file lookup for `[FILL-IN]` work.
The two exceptions are noted explicitly in their own tasks: Batch 3's
`scotrail-glasgow-suburban.toml` Lanarkshire-branch research
(`[BRANCH-RESEARCH]`) and Batch 5/10's `[BRANCH-RESEARCH]` tasks, each of
which may newly touch a segment name a sibling file in a *different*
batch could later want to share — flagged per-task, same as the original
plan's own cross-batch segment-naming notes.

---

## Batch 1: Great Western Railway

**Depends on:** nothing. **Touches:** 6 files.

### Task 1.1 — `lines/gwr-cornish-main-line.toml` `[FILL-IN]`

Named starting list (research spec + this plan's direct read): St
Germans, Devonport, Keyham, St Budeaux Ferry Road, Saltash, Menheniot —
6 suburban Plymouth-area halts currently excluded as "non-principal"
per the file's own comment (line ~120), matching `gwr-cotswold.toml`'s
precedent. Confirm each to the two-source bar, insert in true
Plymouth-area geographic order, inherit the existing segment covering
that stretch. Follow the Recipe.

### Task 1.2 — `lines/gwr-cotswold.toml` `[FILL-IN]`

Named starting list: Combe, Finstock, Ascott-under-Wychwood, Shipton —
4 halts between Oxford and Kingham. The file's own comment (line ~92)
already confirms these are real, currently-open stations, positively
investigated and excluded only for running "a minimal local service (2–3
trains/day)" rather than the broadly-hourly London pattern — that's not
a reason to leave them out of `stations` under this plan's full-coverage
mandate (it only means severity thresholds for that stretch may already
be tuned for the busier pattern; don't change `severity_overrides` as
part of this task). Blockley, Chipping Campden, and Littleton & Badsey
(closed 1966) are correctly excluded already — do not add them. Follow
the Recipe.

### Task 1.3 — `lines/gwr-south-wales.toml` `[FILL-IN]`

Named starting list: Patchway, Pilning, Severn Tunnel Junction — 3
stations between Bristol Parkway and Newport, excluded per the file's
own comment (line ~71) as non-principal on the London–Swansea pattern.
Confirm each to the two-source bar, insert at true position, inherit
segment. Follow the Recipe.

### Task 1.4 — `lines/gwr-thames-valley.toml` `[FILL-IN]`

Named starting list: Ealing Broadway, Southall, Hayes & Harlington, West
Drayton, Iver, Langley, Burnham, Taplow — ~8 inner-suburban stations
excluded per the file's own comment (line ~115) as non-principal.
**Scope boundary, already correctly drawn by this file**: it explicitly
does *not* cover Reading West/Theale/Aldermaston/Midgham/Thatcham/Newbury
Racecourse — those belong to `gwr-west-of-england.toml` (Task 1.6), not
here; don't duplicate them into this file. Follow the Recipe.

### Task 1.5 — `lines/gwr-west-of-england.toml` `[FILL-IN]`

Named starting list (largest single GWR task): Reading West, Theale,
Aldermaston, Midgham, Thatcham, Newbury Racecourse, Kintbury, Hungerford,
Bedwyn, Savernake Low Level, Pewsey, Woodborough, Patney and Chirton,
Lavington, Edington, Bratton, Frome — 17 stations excluded per the
file's own comments (lines ~95, ~104) as non-principal on the Reading–
Taunton express/semi-fast pattern. Frome specifically is reached via a
branch diverging at Clink Road Junction off the direct Castle
Cary route per the file's own comment — confirm the correct segment
boundary for that branch point before tagging Frome's segment (it may
need its own short branch segment rather than inheriting the trunk's,
since it sits off the direct route — this is the one station in this
task closer to a mini `[BRANCH-RESEARCH]` decision than a pure inherit).
Follow the Recipe.

### Task 1.6 — `lines/gwr-bristol-suburban.toml` `[VERIFY]`

Per the Status note and Global Constraints, this file's excluded
stations (7 pre-1970 Heart of Wessex closures) are correctly excluded,
already re-confirmed via each excluded station's own Wikipedia "status:
disused" field. **Do not add them back.** Re-read the file to confirm no
other, currently-open station is missing (the file's own comment already
states every currently-open station on this branch is listed) — if that
still holds, no `stations` edit, no commit. Follow the Recipe (Steps 1,
5–6 only).

### Task 1.7: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  1's changes present.
- [x] Grep `lines/*.toml` for any segment name Task 1.5's Frome decision
  introduced or reused, confirm no accidental collision with an unrelated
  file.
- [x] Confirm `git status` is clean except for Batch 1's committed files.

---

## Batch 2: Northern

**Depends on:** nothing. **Touches:** 11 files.

### Task 2.1 — `lines/northern-airedale.toml` `[FILL-IN]`

Named starting list: Manningham, Frizinghall, Saltaire, Bingley,
Crossflatts, Steeton & Silsden, Kildwick & Crosshills, Cononley — 8
stations on the Shipley–Keighley and Keighley–Skipton stretches,
excluded per the file's own comment (line ~83) as non-principal.
**Coordinate with Task 2.10 (`northern-wharfedale.toml`)**: Manningham
and Frizinghall sit on the shared Bradford approach both files mention —
confirm which file's segment each belongs to before tagging (station
overlap without segment sharing is the expected outcome per the
`xc-south-coast.toml`/`xc-manchester.toml` precedent, not necessarily a
shared segment — verify rather than assume either way). Follow the
Recipe.

### Task 2.2 — `lines/northern-blackpool.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~16) — no named starting list in
the file or the spec. Do a fresh Wikipedia route-diagram pass for the
full Blackpool-area corridor(s) this file covers before Step 2 of the
Recipe; expect a handful (spec's estimate: roughly 3–10) of real
intermediate stations. Follow the Recipe.

### Task 2.3 — `lines/northern-calder-valley.toml` `[FILL-IN]`

Named starting list: Bramley, New Pudsey, Laisterdyke, Low Moor,
Brighouse, Mytholmroyd, Hebden Bridge, Littleborough, Castleton — 9
stations excluded per the file's own comment (line ~57) as
non-principal. Follow the Recipe.

### Task 2.4 — `lines/northern-cumbrian-coast.toml` `[FILL-IN]`

Named starting list (the largest, per the file's own comment, line
~28): Askam, Kirkby-in-Furness, Foxfield, Green Road, Silecroft, Bootle,
Ravenglass, Drigg, Seascale, Sellafield, Braystones, Nethertown, St
Bees, Corkickle, Parton, Harrington, Flimby, Aspatria, Wigton, Dalston —
20 named stations, and the comment itself ends "etc.", implying the list
isn't exhaustive — confirm the full current stopping pattern via a fresh
route-diagram read, don't stop at these 20. Follow the Recipe.

### Task 2.5 — `lines/northern-esk-valley.toml` `[VERIFY]`

File's own comment (line ~69) states explicitly: "no intermediate calls
are omitted here: the whole route is only 18 stations and every one of
them is a principal call." Confirmed false positive on direct
re-inspection. Re-confirm this still holds; if so, no edit, no commit.
Follow the Recipe (Steps 1, 5–6 only).

### Task 2.6 — `lines/northern-furness.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~22). Fresh route-diagram pass
needed. Follow the Recipe.

### Task 2.7 — `lines/northern-hope-valley.toml` `[FILL-IN]`

Named starting list: Chinley, Hathersage, Grindleford — 3 stations
excluded per the file's own comment (line ~22) as "pending CRS
verification." **Important distinction from `emr-regional.toml` (Task
7.3)**: EMR's own Hope Valley Line service genuinely runs fast between
Manchester Piccadilly/Stockport and Sheffield and does *not* call at
these stations (`emr-regional.toml`'s comment confirms this is a
correct, deliberate exclusion for that operator, not a gap) — but
Northern's own service on the same physical line is an all-stations
service, so these are real gaps *here*, not there. Also confirm whether
Edale, Bamford, and Dore & Totley (named in the spec's combined
Hope-Valley tally, possibly already listed in this file or missing too)
belong in this task. Follow the Recipe.

### Task 2.8 — `lines/northern-lakes.toml` `[FILL-IN]`

Named starting list: Burneside, Staveley — 2 stations excluded per the
file's own comment (line ~23) as "pending CRS verification." Smallest
task in this batch. Follow the Recipe.

### Task 2.9 — `lines/northern-tyne-valley.toml` `[FILL-IN]`

Named starting list: Haltwhistle, Prudhoe — plus the file's own comment
(line ~22) references unnamed "Northumberland stops" needing a fresh
route-diagram pass to enumerate fully. Follow the Recipe.

### Task 2.10 — `lines/northern-wharfedale.toml` `[FILL-IN]`

The file's own comment (line ~107) covers the Manningham/Frizinghall
Bradford-approach overlap with `northern-airedale.toml` — see Task 2.1's
coordination note; resolve both files' tagging in whichever task runs
first, and reference that resolution from the other. Beyond that overlap,
do a fresh route-diagram pass for the rest of the Wharfedale corridor.
Follow the Recipe.

### Task 2.11 — `lines/northern-yorkshire-coast.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~19). Fresh route-diagram pass
needed. Follow the Recipe.

### Task 2.12: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  2's changes present.
- [x] Confirm Tasks 2.1 and 2.10's Manningham/Frizinghall coordination
  produced a single, consistent tagging decision across both files (no
  contradictory segment names for the same station).
- [x] Confirm `git status` is clean except for Batch 2's committed files.

---

## Batch 3: ScotRail

**Depends on:** nothing. **Touches:** 12 files. Per the Status note,
only one file in this batch carries a real named gap — the rest are
`[VERIFY]`, confirmed via direct inspection of all 12 files' own
comments.

### Task 3.1 — `lines/scotrail-glasgow-suburban.toml` `[FILL-IN]` + `[BRANCH-RESEARCH]`

The largest single task in this plan; two distinct pieces of work, per
the file's own "Scope boundaries" comment (line ~134):

- **`[FILL-IN]` piece** — 19 named minor stations: Jordanhill,
  Anniesland, Westerton, Bearsden, Hillfoot, Scotstounhill, Garscadden,
  Yoker, Clydebank, Singer, Drumry, Kilpatrick, Bowling, Renton,
  Cardross, Alexandria, High Street, Uddingston, Bellshill. Confirm each
  to the two-source bar (this file's own sourcing notes already flag
  several past CRS-code guessing mistakes on this exact line — e.g.
  Balloch is BHC not BAC, Dalmarnock is DAK not DAL — treat that as a
  live warning to verify every code independently, not carry over any
  guess). Insert each at true geographic position on whichever of the
  file's existing segments (`west-approach`, `west-trunk`,
  `balloch-branch`, `helensburgh-branch`, `core`, `springburn-spur`,
  `airdrie-branch`, `argyle-core`, `argyle-east`) already covers its
  stretch.
- **`[BRANCH-RESEARCH]` piece** — the unmodelled Lanarkshire branch group
  (Whifflet → Coatbridge Central → Hamilton Central, then Larkhall and
  Lanark branches), explicitly bucket (a) in both the spec and this
  file's own comment ("this task could not independently verify their
  CRS codes/exact station order to the two-source standard within its
  research budget"). This needs a fresh, full research pass (station
  list, CRS codes, segment boundaries relative to the existing
  `argyle-east` segment's Rutherglen/Motherwell extent) — treat it with
  the same rigor as an original `line-catalogue-coverage.md` new-file
  task, not as routine infill.

Follow the Recipe for both pieces; two separate regression tests are
appropriate (one per piece) if that keeps the diff reviewable.

### Tasks 3.2–3.12 — remaining ScotRail files, all `[VERIFY]`

Each of the following files' only "omitted" hits, confirmed by direct
inspection, are TIPLOC-only, or (for three of them) an explicit "no
intermediate stations are omitted" statement already in the file. None
names or implies a real `stations`-list gap. Each task: re-read the
file's comments (Recipe Step 1), confirm the classification still holds,
and — since these are TIPLOC-only or already-complete files — optionally
close the TIPLOC gap if research time allows (source against
railwaycodes.org.uk's CRS/TIPLOC tables; a fresh session should not hit
the truncation issue some of these files' comments mention). No
`stations` edit is expected for any of these 11 tasks.

- [x] **Task 3.2 — `scotrail-aberdeen-inverness.toml`**: explicit
  "no intermediate stations are omitted" (line ~95); TIPLOC-only
  otherwise (line ~321).
- [x] **Task 3.3 — `scotrail-ayrshire.toml`**: TIPLOC-only (line ~105).
- [x] **Task 3.4 — `scotrail-bathgate.toml`**: TIPLOC-only (line ~323).
- [x] **Task 3.5 — `scotrail-central-belt.toml`**: TIPLOC-only (line ~73).
- [x] **Task 3.6 — `scotrail-far-north.toml`**: explicit "no
  intermediate stations are omitted" (line ~65); TIPLOC-only otherwise
  (line ~407).
- [x] **Task 3.7 — `scotrail-fife-borders.toml`**: TIPLOC-only (line ~387).
- [x] **Task 3.8 — `scotrail-highland-main-line.toml`**: explicit "no
  intermediate stations are omitted" (line ~77); TIPLOC-only otherwise
  (line ~284).
- [x] **Task 3.9 — `scotrail-kyle.toml`**: TIPLOC-only (line ~324), plus
  one correctly-excluded closed halt (Glencarron Platform, closed 1964 —
  do not add it back).
- [x] **Task 3.10 — `scotrail-shotts.toml`**: TIPLOC-only (line ~320).
- [x] **Task 3.11 — `scotrail-west-highland-fort-william.toml`**:
  TIPLOC-only (line ~548).
- [x] **Task 3.12 — `scotrail-west-highland-oban.toml`**: TIPLOC-only
  (line ~419).

### Task 3.13: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  3's changes present.
- [x] Confirm Task 3.1's new Lanarkshire-branch segment name(s) don't
  collide with any existing segment (grep `lines/*.toml`).
- [x] Confirm `git status` is clean except for Batch 3's committed files
  (likely just `scotrail-glasgow-suburban.toml` plus any TIPLOC-only
  files a Task 3.2–3.12 implementer chose to also update).

---

## Batch 4: Transport for Wales

**Depends on:** nothing. **Touches:** 7 files.

### Task 4.1 — `lines/tfw-cambrian.toml` `[FILL-IN]`

Named starting list (partial, per the file's own comment, line ~68,
which itself trails off with "..." implying more): Penhelig, Tonfanau,
Llangelynin, Llwyngwril, Llanaber, Talybont, Dyffryn (Ardudwy) — coast
branch request-stop halts, excluded as non-principal. Also check the
file's mention of "several closed intermediate stations between
Shrewsbury and Machynlleth" on the trunk — confirm those really are
closed (don't add them) as distinct from any currently-open trunk
station this comment doesn't separately name. Fresh route-diagram
cross-check recommended given the trailing "...". Follow the Recipe.

### Task 4.2 — `lines/tfw-conwy-valley.toml` `[VERIFY]`

File's own comment (line ~35): "No intermediate calls are omitted here
... this line's complete current station list is short enough that
every stop (including request stops) is enumerated below." Confirmed
false positive. Re-confirm; if it holds, no edit, no commit. Follow the
Recipe (Steps 1, 5–6 only).

### Task 4.3 — `lines/tfw-heart-of-wales.toml` `[VERIFY]`

File's own comment (line ~97): "no further request-stop halts are known
to be omitted on this line beyond what's already listed" — the station
list itself reads as already complete. The disclosed tooling failure
(railwaycodes.org.uk truncation, plus a hit `WebFetch` session limit)
affected TIPLOC confirmation for several stations (HPT/KNI/LLO/LLV/LLL/
LLE were confirmed; others weren't), not the station list. This task is
a TIPLOC-refresh pass (a fresh fetch in a new session should not hit the
same truncation), not a station fill-in — confirm no station-count gap
exists before closing out, per Recipe Step 1. Follow the Recipe (mostly
Steps 1, 5–6; Steps 2–4 apply only if TIPLOC work is undertaken and
warrants its own note, though TIPLOC is optional and doesn't need a new
regression test).

### Task 4.4 — `lines/tfw-marches.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hits are TIPLOC omissions
for CWM/PPL/LUD/CRV/CTT/CRK/RUA/WRX, disclosed as the same
railwaycodes.org.uk per-letter-page truncation `tfw-heart-of-wales.toml`
hit. No named station-count gap found. Same shape as Task 4.3: confirm
via Recipe Step 1 that no station gap exists, then treat as an optional
TIPLOC-refresh pass in a fresh session.

### Task 4.5 — `lines/tfw-north-wales-coast.toml` `[FILL-IN]`

Named starting list, confirmed via the file's own comment (line ~66,
already cross-checked station-by-station against each one's own
Wikipedia infobox by the original author): Shotton, Conwy, Penmaenmawr,
Llanfairfechan, and (on Anglesey, after Bangor) Llanfairpwll, Bodorgan,
Tŷ Croes, Rhosneigr, Valley — 9 stations excluded as minor intermediate
calls. Shotton is also the Borderlands Line junction; no line in this
catalogue reaches the Borderlands Line yet, so no shared-segment
question arises there — note this in the new entry's comment, don't
invent a Borderlands segment. Queensferry, Connah's Quay, Bagillt,
Mostyn, Talacre, Llandulas, Llysfaen, Old Colwyn, Mochdre & Pabo, Menai
Bridge, Britannia Bridge, and Gaerwen are correctly excluded as closed —
do not add them. Llandudno itself is correctly out of scope (different
line's branch territory) — do not add it either. Follow the Recipe.

### Task 4.6 — `lines/tfw-valley-lines-north.toml` `[VERIFY]`

Per the Status note, this file's excluded stations (two 1950s/60s
Aberdare-branch halts) are correctly excluded closed stations,
re-confirmed via each excluded station's own Wikipedia infobox. **Do not
add them back.** Re-confirm no other currently-open station is missing;
if so, no edit, no commit. Follow the Recipe (Steps 1, 5–6 only).

### Task 4.7 — `lines/tfw-valley-lines-south.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hits are TIPLOC omissions
for three already-listed stations (Ty Glas, Whitchurch, Coryton — CRS
codes are Wikipedia-only pending a clean second-source re-fetch,
disclosed as the same railwaycodes.org.uk truncation
`tfw-marches.toml` hit). Tongwynlais (closed 1931) and Butetown (not yet
open, "Proposed station") are correctly excluded — do not add them.
Same shape as Task 4.3/4.4: confirm no station-count gap via Recipe
Step 1, then optionally close the TIPLOC/second-source CRS gap for
TGS/WHT/COY in a fresh session.

### Task 4.8: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  4's changes present.
- [x] Confirm `git status` is clean except for Batch 4's committed files.

---

## Batch 5: Southeastern + GTR

**Depends on:** nothing. **Touches:** 6 files.

### Task 5.1 — `lines/southeastern-chatham.toml` `[FILL-IN]` + `[BRANCH-RESEARCH]`

Two distinct pieces, per the file's own comments (lines ~61, ~80, ~245):

- **`[FILL-IN]` piece** — 4 named stations: West Dulwich, Sydenham Hill,
  Penge East, Kent House, excluded (line ~80) for "unconfirmed
  fast/slow calling pattern" reasons shared with
  `southeastern-main-line.toml`. Confirm each to the two-source bar,
  insert at true position.
- **`[BRANCH-RESEARCH]` piece** — the unconfirmed Minster–Martin Mill
  coastal-loop stretch (Deal/Sandwich area, beyond Ramsgate), explicitly
  bucket (c): the file's own comment treats Ramsgate as "the confirmed
  limit of the coastal branch rather than guess at the remainder of the
  loop." Needs its own segment-boundary research pass, comparable to an
  original new-file task.

Follow the Recipe for both pieces.

### Task 5.2 — `lines/southeastern-highspeed.toml` `[BRANCH-RESEARCH]`

The unconfirmed Ashford–Canterbury West–Thanet Parkway–Ramsgate/
Broadstairs "one train" pattern, explicitly bucket (c) per the file's own
comment (line ~70): CRS codes for Canterbury West and Thanet Parkway
weren't cross-checked to a second source in the original session, and
this file deliberately doesn't fill the same stretch
`southeastern-chatham.toml` (Task 5.1) also leaves unconfirmed, for
consistency. **Coordinate with Task 5.1**: if either task's research
resolves this stretch, the other file's comment should be updated to
reference the resolution (matching the cross-file "RESOLUTION" comment
style already used elsewhere in this catalogue, e.g.
`scotrail-glasgow-suburban.toml`'s Bathgate/Bellgrove notes) rather than
independently re-deriving it. This needs its own segment-boundary
research pass. Follow the Recipe.

### Task 5.3 — `lines/southeastern-main-line.toml` `[FILL-IN]`

Named starting list: New Cross, Hither Green, Grove Park, Chislehurst,
Petts Wood — excluded (line ~60) "pending CRS verification of the exact
fast-line calling order," and because they were judged to sit more
naturally with Southeastern's own metro files (if/when those exist —
confirm via `ls lines/southeastern-metro-*.toml` before starting; if
they don't exist yet, this file is the right home for these stations
today). The comment's trailing "etc." suggests more than these 5 —
confirm the full set via a fresh route-diagram pass. Follow the Recipe.

### Task 5.4 — `lines/southern-brighton-main-line.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~155). Fresh route-diagram pass
needed. Follow the Recipe.

### Task 5.5 — `lines/thameslink-bedford.toml` `[VERIFY]`

Per the Status note's correction to the spec: this file's `stations`
list (Bedford through St Pancras) is independently confirmed complete,
station-by-station, via each station's own Wikipedia infobox route
diagram, cross-checked against a second source per station. The file's
disclosed "omitted" language is entirely about `destination_crs_filter`
precision for an unconfirmed peak turn-back/short-working pattern (e.g.
services terminating at St Albans City or Luton rather than Bedford) —
a matching-scope question, not a station-catalogue-completeness gap, and
explicitly **out of scope for this plan** (see Global Constraints' note
on `destination_crs_filter`). Confirm via Recipe Step 1 that no station
is actually missing; if so, no edit, no commit — do not attempt to
resolve the turn-back pattern as part of this task.

### Task 5.6 — `lines/great-northern-kings-lynn.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hit (line ~159) is
`match_keywords` — "No confirmed live-Knowledgebase phrasing was
available to check candidate keywords... against" — not a `stations`
gap, and `match_keywords` tuning is out of scope for this plan (Global
Constraints). Confirm via Recipe Step 1 that no station-count gap exists
(the spec's tooling-failure list includes this file, so double-check
this specific point rather than assuming); if none, no edit, no commit.

### Task 5.7: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  5's changes present.
- [x] Confirm Tasks 5.1 and 5.2's coastal-loop/Ashford-pattern research
  didn't produce two independently-named, colliding segments for the
  same physical stretch if both were resolved.
- [x] Confirm `git status` is clean except for Batch 5's committed files.

---

## Batch 6: Greater Anglia

**Depends on:** nothing. **Touches:** 6 files. Per the Status note, every
file in this batch reads, on direct inspection, as correctly-reasoned
exclusions rather than named real gaps — all 6 tasks are `[VERIFY]`,
sized as confirmation passes, not large fill-ins. **Each task's Step 1
must still do a full, careful re-read** — this plan's spot-check of each
file's "omitted" hits is not a substitute for the implementer's own full
read, and a real gap elsewhere in a large file (some of these run 300+
lines) is possible even where the specific hits checked for this plan
were all correct exclusions.

- [x] **Task 6.1 — `greater-anglia-essex-branches.toml`**: spot-checked
  hit (line ~113) is the Colchester Town branch spur, correctly excluded
  as a self-contained shuttle outside this file's three-branch scope, not
  a gap. Confirm via a full read; if no gap, no edit, no commit.
- [x] **Task 6.2 — `greater-anglia-main-line.toml`**: spot-checked hits
  (lines ~57, ~136, ~188) are all correct exclusions (a station not on
  GA's own calling pattern; Needham Market explicitly "not served by
  main line trains" per its Wikipedia services table). Confirm via a
  full read; if no gap, no edit, no commit.
- [x] **Task 6.3 — `greater-anglia-norfolk-branches.toml`**:
  spot-checked hit (line ~209, ~499) is Waterbeach, correctly excluded as
  a non-stopping pass-through, not a gap. Confirm via a full read
  (this file is large — 500+ lines); if no gap, no edit, no commit.
- [x] **Task 6.4 — `greater-anglia-stansted-express.toml`**:
  spot-checked hit (line ~67) is West Anglia Main Line calling points not
  served by the Stansted Express specifically, correctly excluded.
  Confirm via a full read; if no gap, no edit, no commit.
- [x] **Task 6.5 — `greater-anglia-suffolk-branches.toml`**:
  spot-checked hit (line ~128) is the Port of Felixstowe's non-passenger
  terminals, correctly excluded. Confirm via a full read; if no gap, no
  edit, no commit.
- [x] **Task 6.6 — `greater-anglia-west-anglia.toml`**: spot-checked hit
  (line ~273) is a station referenced only in an unrelated Wikipedia
  section (Trumpington area, not a station article), correctly excluded
  as not a current calling point. Confirm via a full read; if no gap, no
  edit, no commit.

Each task follows the Recipe (Steps 1, 5–6; Steps 2–4 apply only if a
full read overturns the `[VERIFY]` classification for that specific
file, in which case treat the rest of that task as `[FILL-IN]`).

### Task 6.7: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  6's changes present (a no-op run if all 6 tasks found nothing to add).
- [x] Confirm `git status` — likely clean, no committed files, if every
  task's full read confirms the `[VERIFY]` classification.

---

## Batch 7: East Midlands Railway

**Depends on:** nothing. **Touches:** 4 files.

### Task 7.1 — `lines/emr-connect.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hit (line ~83) is a single
TIPLOC omission for COR ("not listed in Wikipedia's infobox... omitted
rather than guessed"). Not a station-count gap. Confirm via Recipe Step
1; if no gap, optionally source COR's TIPLOC in a fresh session, no
`stations` edit expected.

### Task 7.2 — `lines/emr-midland-main-line.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hit (line ~177) is a
single TIPLOC omission on the shared-trunk segment boundary station
("the value this file originally carried here was not independently
sourced/cited"). Not a station-count gap. Confirm via Recipe Step 1; no
`stations` edit expected.

### Task 7.3 — `lines/emr-regional.toml` `[VERIFY]`

**Important — read alongside Task 2.7.** This file's own comment (line
~61) states its Hope Valley Line stations (Edale, Hope, Bamford,
Hathersage, Grindleford, Dore & Totley) are "correctly omitted, not just
'pending verification'" — EMR's genuine service pattern on this stretch
runs fast between Manchester Piccadilly/Stockport and Sheffield and does
not call at these stations, confirmed via both the EMR services table
and the Hope Valley Line article's own calling-pattern list. **This is a
correct exclusion for this file and this operator — do not add these
stations to `emr-regional.toml`.** The real gap for the *same physical
stations* belongs to `northern-hope-valley.toml` (Task 2.7), whose own
Northern service genuinely is all-stations there. Confirm via Recipe
Step 1 that no other, separate gap exists in this file; if none, no
edit, no commit.

### Task 7.4 — `lines/emr-rural-branches.toml` `[FILL-IN]`

Named starting list: Netherfield & Colwick, Radcliffe-on-Trent,
Aslockton & Whatton, Elton & Orston, Bottesford — 5 currently-open gaps
named directly in the spec. **Critical: do not touch this file's
already-correct closed-station exclusions** (several pre-Beeching Robin
Hood Line stations, already re-confirmed per the Status note and Global
Constraints) — this task adds only the 5 named currently-open stations
above, confirmed to the two-source bar, at their true geographic
position. Also check the file's own BIN-area comment (referencing
`northern-hope-valley.toml`'s "pending CRS verification" pattern) for
any additional named-but-unconfirmed station in the same bucket. Follow
the Recipe.

### Task 7.5: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  7's changes present.
- [x] Confirm Task 7.3 made no edit to `emr-regional.toml`'s Hope Valley
  stretch (this is the one task in this batch where "doing nothing" is
  the correct, verified outcome, not an incomplete task).
- [x] Confirm `git status` is clean except for Batch 7's committed files.

---

## Batch 8: TransPennine Express

**Depends on:** nothing. **Touches:** 4 files.

### Task 8.1 — `lines/tpe-anglo-scottish.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~45), following
`xc-manchester.toml`'s convention. Fresh route-diagram pass needed.
Follow the Recipe.

### Task 8.2 — `lines/tpe-borders.toml` `[FILL-IN]`

Named starting list: Cramlington, East Linton — 2 "very-low-frequency"
calls excluded (lines ~89, ~93) per the "principal stations only"
convention. Both are real, currently-served stations per the file's own
sourcing (one train per day for Cramlington, confirmed via
nationalrail.co.uk) — a low frequency doesn't disqualify a station from
`stations`, per this plan's full-coverage mandate. Follow the Recipe.

### Task 8.3 — `lines/tpe-north.toml` `[VERIFY]`

Direct inspection: this file's one "omitted" hit (line ~39) is Manchester,
Manchester International Airport, and Stalybridge — correctly excluded
because TPE's "North Route" grouping genuinely doesn't reach them (per
the file's own SCOPE comment, "So MAN, MIA and Stalybridge are all
deliberately omitted below rather than transcribed from the brief's
unverified starting list"). Confirm via a full read whether a separate,
un-flagged "principal stations only" gap also exists elsewhere in this
file (the spot-check for this plan found only the correct exclusion
above, but this file wasn't read in full); if a real gap turns up,
treat the rest of this task as `[FILL-IN]` and follow the Recipe from
Step 2. Otherwise no edit, no commit.

### Task 8.4 — `lines/tpe-south.toml` `[FILL-IN]`

Named starting list: Liverpool South Parkway, Warrington West,
Warrington Central, Birchwood, Irlam, Urmston, Manchester Oxford Road,
Meadowhall, Barnetby, Habrough — 9 (some sources give 10 counting a
near-duplicate) excluded (line ~76) per `xc-manchester.toml`'s
"principal stations only" convention. Follow the Recipe.

### Task 8.5: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  8's changes present.
- [x] Confirm `git status` is clean except for Batch 8's committed files.

---

## Batch 9: WMR/LNWR + XC + WCML

**Depends on:** nothing. **Touches:** 7 files.

### Task 9.1 — `lines/wmr-snow-hill.toml` `[FILL-IN]`

The single largest fill-in task in this plan. Named starting list (~25
stations per the spec's own tally, file's own comment at line ~99):
Jewellery Quarter, The Hawthorns, Langley Green, Rowley Regis, Old Hill,
Cradley Heath, Lye, Hagley, Blakedown, Fernhill Heath, Bordesley, Small
Heath, Spring Road, Hall Green, Yardley Wood, Wythall, Earlswood, Wood
End, Danzey, Wootton Wawen, Wilmcote, Stratford-upon-Avon Parkway,
Acocks Green, Olton, Widney Manor — excluded per the file's own comment
as "only principal/interchange stations are listed," matching
`wcml-birmingham.toml`/`xc-manchester.toml`/`northern-furness.toml`'s
convention. Given the size, consider splitting Step 2's research across
this line's distinct branches (Stourbridge/Worcester leg vs.
Stratford-upon-Avon/Dorridge leg) for reviewability, but keep it one
task/one file/one set of commits per the Recipe (multiple commits across
one task are fine if that keeps diffs small — the Recipe's "one commit
per file" is a floor, not a ceiling). Follow the Recipe.

### Task 9.2 — `lines/lnwr-birmingham-crewe.toml` `[FILL-IN]`

The file's own comment (line ~83) correctly excludes Polesworth
(one daily northbound service only, not a genuine regular calling
point) — do not add it. Beyond that, do a fresh route-diagram pass for
any other minor stations on this line's real current stopping pattern
(the spec's own research read this file in full and found "several more
1–9-name pockets," without listing them individually in its summary
table) — confirm the full list via live sources rather than relying on
this plan's own partial read. Follow the Recipe.

### Task 9.3 — `lines/wcml-birmingham.toml` `[FILL-IN]`

Named starting list: Canley, Tile Hill, Berkswell, Hampton-in-Arden,
Marston Green, Lea Hall, Stechford, Adderley Park — 8 West Midlands
Trains local stops excluded (line ~46) as not called at by Avanti
service, matching `xc-manchester.toml`'s convention. Follow the Recipe.

### Task 9.4 — `lines/xc-cardiff.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~20) — the convention several
other tasks in this plan cite as precedent. Fresh route-diagram pass
needed for this file's own corridor. Follow the Recipe.

### Task 9.5 — `lines/xc-manchester.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~24) — this is the file whose own
convention several other tasks in this plan cite as precedent for what
"minor" meant in the original build-out; filling its own gap doesn't
change that precedent's validity for already-completed files, only adds
this file's own missing stations. Fresh route-diagram pass needed.
Follow the Recipe.

### Task 9.6 — `lines/xc-south-coast.toml` `[FILL-IN]`

Unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate (line ~26). Fresh route-diagram pass
needed. Follow the Recipe.

### Task 9.7 — `lines/xc-stansted.toml` `[FILL-IN]`

Named starting list: Melton Mowbray, Oakham, Stamford, March, Audley
End — 5 stations excluded (line ~20) "pending CRS verification." Follow
the Recipe.

### Task 9.8: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  9's changes present.
- [x] Confirm `git status` is clean except for Batch 9's committed files.

---

## Batch 10: LNER + Chiltern + c2c + Merseyrail

**Depends on:** nothing. **Touches:** 6 files. This batch includes the
two files the Status note flags as a correction to the spec's own
"don't need a repeat pass" framing (`chiltern-main-line.toml`,
`c2c.toml`).

### Task 10.1 — `lines/lner-leeds.toml` `[VERIFY]`

Direct inspection: this file's one "omitted" hit (line ~40) is a group
of Northern-only stations (Outwood, Sandal & Agbrigg, Fitzwilliam, South
Elmsall, Adwick, Bentley) that LNER's own service on this shared trunk
genuinely doesn't call at — correctly excluded, matching
`xc-cardiff.toml`/`xc-south-coast.toml`'s convention, and the file's own
comment confirms the shared segment names still apply for
incident-matching purposes even where LNER's own trains skip these
stops. **Do not add these to `lner-leeds.toml`** — if they're missing
from *every* line's catalogue (check whichever Northern file covers this
same trunk), that would be a gap in that Northern file, not this one, and
is out of this task's scope (raise it separately if found, don't fix it
here). Confirm via Recipe Step 1 that no other gap exists in this file;
if none, no edit, no commit.

### Task 10.2 — `lines/chiltern-main-line.toml` `[FILL-IN]`

**Correction to the spec's "Chiltern/c2c/Merseyrail don't need a repeat
pass" framing — see the Status note.** Named starting list, per the
file's own comment (lines ~23, ~30): Sudbury & Harrow Road, Denham,
Beaconsfield, Princes Risborough (and the comment's own "etc." implying
more) on the Marylebone approach; plus, on the Birmingham approach via
Hatton, an entire named intermediate sequence — Dorridge, Lapworth,
Widney Manor, Solihull, Olton, Acocks Green, Tyseley, Small Heath,
Bordesley — all currently minor-call-omitted per the file's own comment
(line ~32). **Coordinate with Task 9.1 (`wmr-snow-hill.toml`)**: Olton,
Acocks Green, and Widney Manor appear in both files' "omitted" starting
lists (this task's Birmingham-approach stretch and Task 9.1's
Snow Hill list) — confirm via live sources whether these are genuinely
the same physical stations on a shared trunk (in which case both files
list them with a shared segment, following this catalogue's
station-overlap-vs-segment-sharing precedent) or distinct stations on
parallel routes, before tagging segments in either file. Follow the
Recipe.

### Task 10.3 — `lines/chiltern-aylesbury.toml` `[VERIFY]`

File's own comment (line ~36): "this is itself an all-stations local
service... so no intermediate calls are omitted here." Confirmed false
positive. Re-confirm; if it holds, no edit, no commit. Follow the Recipe
(Steps 1, 5–6 only).

### Task 10.4 — `lines/c2c.toml` `[BRANCH-RESEARCH]`

**Correction to the spec's "Chiltern/c2c/Merseyrail don't need a repeat
pass" framing — see the Status note.** The file's own comment (line
~144) explicitly scopes out the Rainham branch (Barking–Dagenham
Dock–Rainham–Purfleet–Grays) as "a third, distinct route to Grays...
out of scope for this file — omitted rather than guessed at." This is a
whole unmodelled branch, bucket (c): needs its own station list, CRS
codes, and a fresh segment-boundary decision relative to this file's
existing Barking-area segment (the branch diverges from the file's
already-modelled trunk at or near Barking — confirm the exact junction
station via live sources, applying `SCHEMA.md`'s shared-trunk rule of
thumb). Treat with the same rigor as an original new-file task. Follow
the Recipe.

### Task 10.5 — `lines/merseyrail-northern.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hit (line ~63) is
TIPLOC-only ("the only source found for them... produced
misaligned/wrong rows for two stations"). Not a station-count gap.
Confirm via Recipe Step 1; if no gap, optionally source TIPLOCs in a
fresh session (cross-check carefully against National Rail Enquiries
given the file's own note about misaligned rows from
railwaycodes.org.uk), no `stations` edit expected.

### Task 10.6 — `lines/merseyrail-wirral.toml` `[VERIFY]`

Direct inspection: this file's only "omitted" hit (line ~61) is
TIPLOC-only, same reason as Task 10.5's sibling file. Not a
station-count gap. Confirm via Recipe Step 1; no `stations` edit
expected.

### Task 10.7: Batch verification

- [x] Run `cargo test -p aggregator -p common -p api` with all of Batch
  10's changes present.
- [x] Confirm Task 10.2 and Task 9.1's Olton/Acocks Green/Widney Manor
  coordination produced a single, consistent segment-sharing decision
  across both files (not a silent duplicate-but-uncoordinated segment
  name).
- [x] Confirm `git status` is clean except for Batch 10's committed
  files.
- [x] **Whole-plan closeout**: run `cargo test -p aggregator -p common -p api`
  once more with every batch's changes present (if executed
  sequentially) to confirm the full, now-larger `lines/` directory still
  parses as a whole and no cross-batch segment name accidentally
  collided (grep `lines/*.toml` for every segment name touched across
  all 10 batches' notes above and confirm each collision, if any, was a
  deliberate, documented shared-trunk decision).

---

## References

- `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
  — the gap-analysis this plan executes; read in full before starting any
  task.
- `docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md` — the
  original batched build-out plan this plan is the direct sequel to; same
  batch-by-TOC/region shape, same per-task testing convention, same
  Recipe-once-defined-many-times-applied structure.
- `lines/SCHEMA.md` — schema, `stations` vs. `sample_stations` field
  definitions (the field this plan must never touch), shared-trunk rule
  of thumb (needed only for `[BRANCH-RESEARCH]` tasks).
- `crates/api/src/routes/line_status.rs:182-220` —
  `get_stop_point_disruption`, the handler whose silent-empty-list
  behavior this whole plan exists to fix.
- `crates/common/src/lib.rs:396-486` — `Station`, `LineDefinition`,
  `has_station`, `segment_for`, `stations_between` (the ordering-sensitive
  function every task's Global-Constraint on insertion position protects).
- `crates/aggregator/src/segments.rs` — `SegmentRegistry`, confirmed
  purely per-station-`segment`-field-driven, needs zero code changes.
- `crates/aggregator/src/matcher.rs` — where every `[FILL-IN]`/
  `[BRANCH-RESEARCH]` task's regression test(s) are added; reuse its
  existing `load_line`/`load_all_lines` helpers.
