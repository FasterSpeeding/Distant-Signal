# Line Catalogue Coverage — Batched Curation Plan

> **For agentic/human workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This is not a "build feature X" plan, the same way
> `docs/superpowers/plans/2026-08-29-trust-schedule-delay-validation.md` is
> not one.** There is no new crate, no migration, no route, no poller, no
> aggregator logic. Every task's deliverable is either (a) one new
> hand-curated `lines/<id>.toml` file plus the regression tests that prove
> the matcher treats it correctly, or (b) a one-line correctness fix to an
> existing file. The engineering pattern (`LineDefinition::from_dir`,
> `SegmentRegistry`, the incident matcher) already exists and is not
> touched by this plan — only its input data grows. Treat every "Step 1:
> research" below as load-bearing, not boilerplate: this repo's whole
> convention is "no invented API details," and for this catalogue that
> means no invented station lists or segment boundaries either. An
> implementer who cannot independently verify a fact should say so in the
> file's comments and flag it for follow-up rather than guess.

**Goal:** Close the line-catalogue coverage gap identified in
`docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md` (~70–95
missing hand-curated line-definition files across 13+ TOCs) plus the
London Overground gap identified separately in
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` Area
2 (not covered by the gap analysis at all — see "The London Overground
gap" below) — by adding new `lines/*.toml` files in batches small enough
to review and execute independently, each tested to the same standard the
existing 20 files already meet.

**Architecture:** No architecture changes. Every task uses the existing
`lines/*.toml` schema (`lines/SCHEMA.md`) and the existing
`LineDefinition::from_dir`/`SegmentRegistry`/matcher pipeline
(`crates/common/src/lib.rs`, `crates/aggregator/src/segments.rs`,
`crates/aggregator/src/matcher.rs`) exactly as they already work for the
20 files that exist today. This plan is data curation using an existing
model, batched by TOC/network cluster, in the order
`2026-08-29-line-coverage-gap-analysis.md`'s own "Prioritized list, across
everything" section already recommends.

**Tech Stack:** TOML data files under `lines/`; Rust 2024 edition tests
(`cargo test -p aggregator`, `-p common`, `-p api`) as the verification
mechanism — no new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md`
(primary — every batch below is a direct slice of its "Missing TOC/network
sections" and "Prioritized list" sections; cite it, don't re-derive its
research) and `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
Area 2 (source of the London Overground gap folded into Batch 3, since the
gap-analysis document itself never mentions Overground — confirmed by
direct search of that file, not an oversight this plan is guessing at).

## Global Constraints

- **One line per file, matching `lines/SCHEMA.md`'s explicit curation
  rule.** Never combine two distinct lines into one file to save a task,
  even when a batch feels large.
- **Never invent a station list, segment boundary, or ATOC code.** Every
  fact in a new `.toml` file must trace to either (a) a fact already
  stated in the gap-analysis document (cite the section), or (b) fresh
  verification the implementer actually did against a live source
  (Wikipedia cross-checked against a second source — railwaycodes.org.uk,
  Network Rail's own route pages, National Rail Enquiries, or the TOC's
  own site — following the exact method the gap analysis itself used, its
  own "Method" section, item 3). If a fact can't be confirmed to that
  standard, say so in a `.toml` comment and leave it out rather than
  guess — this matches how `northern-furness.toml`'s own comment already
  flags the Cumbrian Coast gap instead of inventing content for it.
- **Segment names are the one place two files in different batches can
  silently collide or silently fail to share when they should.** Before
  naming a new segment, grep `lines/*.toml` for any existing segment name
  at the shared station and read that file's own comment about why it did
  or didn't share — follow the precedent
  `xc-south-coast.toml`/`xc-manchester.toml` already set ("station overlap
  is fine, segment-sharing is a deliberate choice") rather than reinventing
  the judgment call from scratch each time.
- **Testing convention (binding on every task):** every new line gets at
  least one new `#[test]` in whichever of `crates/aggregator/src/matcher.rs`
  or `crates/aggregator/src/segments.rs` already carries the closest
  existing precedent, asserting the same two things the existing suite
  already asserts for SWR/CrossCountry/Elizabeth line:
  1. An incident at a station on this line's **exclusive** segment matches
     only this line (`MatchScope::ExclusiveSegment`) — mirrors
     `swr_exclusive_segment_incident_does_not_propagate` and
     `elizabeth_branch_incident_stays_on_its_branch`.
  2. If this line shares a segment with a sibling line (in this batch or
     an already-curated one), an incident on that shared segment matches
     **every** line sharing it, all with `MatchScope::SharedSegment` —
     mirrors `swr_shared_trunk_incident_propagates` and
     `xc_hub_incident_propagates_to_every_cross_country_arm`. Skip this
     assertion only for a genuinely standalone line with no shared
     segment (e.g. c2c, Merseyrail) and say so in the task's acceptance
     criteria.
  3. **Station overlap without segment sharing** turned out to be the
     dominant pattern across the catalogue, not an edge case: two files'
     station lists genuinely overlap at a real station, but the physical
     track/segment isn't shared, so each line independently resolves
     `MatchScope::ExclusiveSegment` at that station rather than one
     propagated `MatchScope::SharedSegment` match. Whenever a task's
     station-list research turns up this situation (per the
     "station overlap is fine, segment-sharing is a deliberate choice"
     precedent above), add a regression test asserting exactly that —
     both (or all) lines match, and every match stays
     `MatchScope::ExclusiveSegment` — mirroring the many existing
     `*_station_overlap_*` tests in `crates/aggregator/src/matcher.rs`
     (e.g. `llj_station_overlap_matches_both_lines_as_exclusive`,
     `chester_station_overlap_matches_both_lines_as_exclusive`,
     `gwr_south_wales_station_overlap_with_xc_cardiff_stays_exclusive_each_line`).
  Every task also re-runs `cargo test -p aggregator -p common -p api`
  (the three crates that call `LineDefinition::from_dir` over `lines/`) to
  confirm the whole directory still parses — see the Recipe below.
- **Commit after each task**, one commit per new `.toml` file plus its
  tests (matches this repo's existing "small, reviewable diff" convention
  and the Elizabeth-line-merge/TfL-integration plans' own per-task commit
  discipline).
- **Out of scope for this entire plan, every batch:**
  - No data-source integration changes (no new poller, no new feed, no
    Knowledgebase/LDBWS/TRUST wiring).
  - No new aggregator/matcher logic. `SegmentRegistry`, `lines_affected_by`,
    and severity classification are complete and correct for arbitrary
    line counts today — this plan only grows their input.
  - No frontend changes. Confirmed by inspection: nothing under `frontend/`
    hardcodes the set of NR line ids except `frontend/lib/modes.ts`'s
    TfL-mode list (which lists `'elizabeth-line'` for the *existing*
    TfL/NR merge, unrelated to adding new NR-only lines) — every other
    frontend reference to a line id is a test fixture, not catalogue-driven
    logic. `/public/lines` and the line-detail page already render
    whatever the catalogue contains.
  - No London-Overground/TfL merge work. Batch 3 produces the six
    `lines/overground-*.toml` files only — the mapping-table extension and
    suppress-and-overlay mechanism described in the tfl-service-metrics-v2
    spec's Area 1/Area 2 is separate, later, already-scoped-elsewhere work
    that depends on this batch but isn't part of it.
  - No re-litigating the gap analysis's own severity/priority reasoning.
    Batch order below follows its "Prioritized list" section directly;
    disagreement with that ordering is out of scope for this plan (raise
    it as a revision to the gap-analysis doc instead).

---

## Per-Line-File Task Recipe

Every task below (except the Task 0 bug fix and the Batch-3 research task)
follows this same five-step shape. Individual tasks give the
TOC/line-specific *content* of each step; this section is the mechanics,
defined once rather than repeated ~85 times.

- [ ] **Step 1: Research and confirm the facts this file needs.** Pull
  together, for this specific line: current operator/ATOC code, ordered
  station list (CRS + TIPLOC where knowable), where it starts/ends,
  which stations are junctions with a sibling line already in the
  catalogue (or a sibling being added in the same batch), and any
  severity-relevant operating characteristic (rural single-track branch
  vs. dense commuter corridor — see `lines/SCHEMA.md`'s "Severity tuning"
  section). Start from the task's cited gap-analysis section; verify
  anything not already stated there against a live source, cross-checked
  against a second source per this plan's Global Constraints. Do not
  proceed to Step 2 with an unconfirmed fact — comment it out or omit it
  instead.

- [ ] **Step 2: Draft `lines/<id>.toml`.** Follow `lines/SCHEMA.md`
  exactly: `id`/`name`/`mode = "national-rail"`/`category`/`operators`
  required; order `stations` geographically end-to-end per the curation
  rule about parsing "Lines blocked between A and B" messages; tag every
  station's `segment`, applying the shared-trunk rule of thumb (junction
  station belongs to the shared segment, the exclusive segment starts at
  the *next* station); add `sample_stations`,
  `match_keywords`/`excluded_keywords`, and `severity_overrides` only
  where the task's acceptance criteria calls for them or where Step 1's
  research clearly warrants them (don't pad `match_keywords` past two or
  three high-precision phrases, per the curation rules).

- [ ] **Step 3: Add the regression test(s)** required by the Global
  Constraints' testing convention, in the file the task names (usually
  `crates/aggregator/src/matcher.rs`, sometimes
  `crates/aggregator/src/segments.rs` for a pure segment-sharing
  assertion), following the exact style of the existing tests in that
  file (`load_all_lines()`/`load_line(id)` helpers already exist — reuse
  them, don't duplicate).

- [ ] **Step 4: Run the tests.**

```bash
cargo test -p aggregator -p common -p api
```

  Expected: all pass, including the new test(s) from Step 3 and every
  pre-existing test (confirms `lines/` still parses as a whole and no
  existing shared-trunk/exclusive-segment assertion broke).

- [ ] **Step 5: Commit.**

```bash
git add lines/<id>.toml crates/aggregator/src/<file>.rs
git commit -m "feat(lines): add <Line Name> (<TOC>)"
```

---

## Batching rationale

70–95 missing files (84 concretely enumerated below, once London
Overground's 6 are folded in — see next section) is too much for one
task list. This plan follows the gap-analysis document's own "Prioritized
list, across everything" section (12 numbered items, each already
reasoned about size, dependency, and sequencing) and turns each item — or
an explicitly-justified merge of two adjacent items the gap analysis
itself says to pair — into one batch. That produces **12 batches**,
listed in the gap analysis's own priority order, each independently
executable as its own subagent-driven-development run:

| # | Batch | New files | Gap-analysis source |
|---|---|---|---|
| 1 | WCML integrity + Avanti branches + WMR/LNWR | 8 (+1 bug fix) | Priority items 1, 6, 9 (explicitly paired: "same file, same research" / "shared-operator-code entanglement") |
| 2 | Greater Anglia | 6 | Priority item 2 |
| 3 | London Overground (folded-in gap) | 6 | Not in the gap analysis — see below; tfl-service-metrics-v2 spec Area 2 |
| 4 | Great Western Railway | 6 | Priority item 3 |
| 5 | Southeastern + GTR (Southern/Gatwick Express/Great Northern/Thameslink branches) | 14 | Priority item 4 (explicitly paired: shared London Bridge/Lewisham dependency) |
| 6 | LNER + open-access ECML operators + Heathrow Express | 8 | Priority item 5 |
| 7 | East Midlands Railway | 4 | Priority item 7 |
| 8 | Northern's real completeness gaps | 6 | Priority item 8 |
| 9 | TransPennine Express | 4 | Priority item 10 |
| 10 | ScotRail | 10 | Priority item 11 (first half) |
| 11 | Transport for Wales | 7 | Priority item 11 (second half) |
| 12 | Chiltern, c2c, Merseyrail | 5 | Priority item 12 |

Batch 5 is deliberately the largest (14 files) rather than split further,
because the gap analysis explicitly calls out Southeastern's metro
services and Thameslink's Sevenoaks branch as needing the *same*
coordinated pass to avoid an accidental segment-naming mismatch at London
Bridge/Lewisham — splitting it into two batches would recreate exactly
the uncoordinated-branches risk the gap analysis warned against. ScotRail
and TfW (batches 10–11) are kept as two batches rather than one, unlike
Southeastern/GTR, because the gap analysis explicitly says they have "no
overlap with any currently-defined line" and "essentially zero collision
risk" against each other — there is no coordination dependency forcing
them together, and combined they'd be 17 files, too large for one
reviewable pass.

Batch 7 (EMR) is sequenced right after Batch 5 rather than merged into it,
even though the gap analysis says to sequence EMR "near" the GTR
Thameslink-branch work (shared Bedford–St Pancras infrastructure) —
merging would make Batch 5 22 files, too large. Instead, Batch 7's EMR
core-file task explicitly cites whatever segment name Batch 5's Thameslink
Bedford-branch task chose for the Bedford–St Pancras stretch, so the
dependency is honored by citation and sequencing rather than by size.

## The London Overground gap

**Confirmed as a real gap in the gap-analysis document itself, not just a
different framing of something it already covers**: a direct text search
of `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md` for
"Overground" (and TOC code `LO`) returns zero matches. Its own "TOCs found
with National Rail passenger services that this audit confirms are
genuinely missing" list and its 12-item priority list both omit Overground
entirely.

The source of this gap is `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`,
Area 2 ("Overground — groundwork only, not yet a merge"), which found:
"Unlike Elizabeth line, **Overground is not ingested on the NR side at
all**: no `lines/overground-*.toml` files exist, and no `"LO"` operator
code... appears anywhere in `lines/` or `crates/`." That spec explicitly
scopes six lines by their current (Nov 2024 rebrand) names — Liberty,
Lioness, Mildmay, Suffragette, Weaver, Windrush — each needing
`operators = ["LO"]`, `mode = "national-rail"`, and correct `segment`
tagging, since "the six lines share trunk sections in multiple places
(e.g. the East London and North London corridors)." That same spec is
explicit that "Verified station lists and segment boundaries for all six
Overground lines are not established by this spec — that curation is
comparable effort to the existing three-file Elizabeth line definitions,
times six, and needs its own research pass before implementation planning
starts."

This plan folds that gap in as **Batch 3**, sequenced immediately after
Greater Anglia (Batch 2) rather than at the end: both batches directly
extend the same Elizabeth-line-adjacent TfL/NR precedent (Greater Anglia's
Shenfield-corridor segment-sharing decision; Overground's own eventual
TfL/NR merge, out of scope here but the six files are its prerequisite),
so doing them while that context is fresh mirrors the gap analysis's own
stated reason for sequencing Greater Anglia early. Batch 3 opens with an
explicit research task (Task 3.0) rather than jumping straight into the
six per-line-file tasks, honoring the tfl-service-metrics-v2 spec's own
statement that this needs its own research pass — this plan does not
invent Overground's six station lists.

---

## Batch 1: WCML integrity + Avanti branch split + WMR/LNWR

**Depends on:** nothing. **Produces:** a fixed `west-coast-main-line.toml`,
4 new WCML branch files, 4 new WMR/LNWR files.

Gap-analysis source: P0 section ("`lines/west-coast-main-line.toml` labels
operator code `"AW"` as Avanti West Coast. That's incorrect"), and
Priority items 1, 6, 9.

### Task 1.0: Fix `west-coast-main-line.toml`'s AW→VT operator-code bug

**Files:** Modify `lines/west-coast-main-line.toml`; Modify/create a test
in `crates/common/src/lib.rs` or `crates/aggregator/src/matcher.rs`.

**Source material:** the gap analysis's P0 section, verbatim: Avanti West
Coast's real ATOC code is `VT` (confirmed via Wikidata, Wikipedia's Avanti
infobox, and a RailUK Forums thread on the Virgin→Avanti rebrand — all
independently agree the code carried over unchanged from Virgin Trains).
Transport for Wales' real code is `AW` (confirmed via Network Rail's own
"References and Symbols" document). The current file has these swapped:
it lists `"AW"` under a comment claiming Avanti West Coast.

Also **verify, and fix if confirmed**, the secondary flag in the same
section: the file lists both `"LM"` and `"LN"` as separate operators
(commented "London Northwestern Railway" and "West Midlands Trains
(London Northwestern services)" respectively); one source
(railwaycodes.org.uk) says WMR and LNWR both use `LM`, with `LN` "not yet
used in GBTT/eNRT" — the gap analysis flags this as single-source and
unconfirmed to the same standard as the AW/VT finding, so re-verify
against a second source before removing `LN` outright.

**Acceptance criteria:**
- [ ] `lines/west-coast-main-line.toml`'s `operators` list contains `"VT"`
  (commented Avanti West Coast) and does **not** contain `"AW"`.
- [ ] If the `LM`/`LN` duplication is independently confirmed by a second
  source, the file lists only the real code(s); if not confirmed, leave
  both in with an updated comment noting the open question (per the
  Global Constraint on not inventing facts — "unconfirmed" is a valid
  outcome, don't force a resolution).
- [ ] Add one new test (in `crates/common/src/lib.rs`'s existing test
  module or `crates/aggregator/src/matcher.rs`) asserting
  `LineDefinition::from_dir`'s loaded `"wcml"` entry's `operators` contains
  `"VT"` and not `"AW"` — a regression guard against this exact bug
  recurring.
- [ ] `cargo test -p aggregator -p common -p api` passes.
- [ ] Commit separately from the branch-split tasks below (this is a fix
  to an existing file, not a new one).

### Tasks 1.1–1.4: WCML branch files

Gap-analysis source: "WCML — does the single generic entry need
splitting?" section. `west-coast-main-line.toml`'s own comment already
says: "Branches (e.g. Birmingham via Trent Valley) are not included here
— they belong in their own line definitions." Avanti's real service group
beyond the modelled Euston–Carlisle spine: Birmingham (via Rugby/Coventry,
not the spine), Manchester (via Stoke or Crewe/Wilmslow), Liverpool (via
Crewe/Runcorn), North Wales/Holyhead (via Crewe/Chester).

- [ ] **Task 1.1 — `lines/wcml-birmingham.toml`** (Euston–Birmingham via
  Rugby/Coventry). Operators: `VT` (confirmed by Task 1.0). Segment
  decision: shares Euston-area trunk with `west-coast-main-line.toml`'s
  `wcml-london` segment up to the Rugby-area junction where it diverges
  onto the Birmingham route (Rugby itself is on the WCML spine already —
  research the actual diverging point, likely near Rugby or further
  north depending on the real route via Coventry). Follow the Recipe.

- [ ] **Task 1.2 — `lines/wcml-manchester.toml`** (Euston–Manchester via
  Stoke or Crewe/Wilmslow). Operators: `VT`. Segment decision: shares
  the WCML spine up to Crewe (already a `wcml-northwest` junction station
  in the existing file), diverges beyond it. Research which of the two
  real routes (via Stoke vs. via Crewe/Wilmslow) is Avanti's actual
  current pattern before choosing one or modelling both as one file's
  branch-in-comments, matching how `swr-south-west-main.toml` handles
  Bournemouth+Weymouth as one file.

- [ ] **Task 1.3 — `lines/wcml-liverpool.toml`** (Euston–Liverpool via
  Crewe/Runcorn). Operators: `VT`. Segment decision: shares the spine to
  Crewe, exclusive segment beyond it via Runcorn to Liverpool Lime Street.
  Note in the file's comment (per the gap analysis's Merseyrail section)
  that Liverpool Lime Street is a different station/corridor from
  Merseyrail's own underground loop — no segment collision expected, but
  worth a comment for whoever curates Merseyrail in Batch 12.

- [ ] **Task 1.4 — `lines/wcml-north-wales.toml`** (Euston–North
  Wales/Holyhead via Crewe/Chester). Operators: `VT`. Segment decision:
  shares the spine to Crewe, exclusive segment via Chester to Holyhead.
  This is the one branch the gap analysis flags as a **confirmed future
  overlap**: Transport for Wales' North Wales Coast Line (Batch 11) covers
  the same Chester–Holyhead corridor. Comment the segment name choice
  clearly so Batch 11's TfW task can either share it or deliberately not,
  the same judgment call already documented at Reading/Cardiff for other
  lines — don't force the decision now since TfW's file doesn't exist yet;
  just don't accidentally pick a name that collides by coincidence.

### Tasks 1.5–1.8: WMR/LNWR files

Gap-analysis source: "West Midlands Railway / London Northwestern
Railway" section. WMR: Snow Hill lines (Stratford-upon-Avon/Dorridge–
Worcester/Stourbridge) and Cross-City Line (Lichfield–Birmingham New
Street–Redditch). LNWR: Euston–Milton Keynes–Northampton/Birmingham/Crewe
semi-fast commuter services on WCML metals.

- [ ] **Task 1.5 — `lines/wmr-snow-hill.toml`** (WMR Snow Hill lines).
  Operators: `LM` (per Task 1.0's confirmed/re-confirmed code). No
  meaningful overlap with the existing WCML files per the gap analysis
  (approaches Birmingham via Snow Hill/Moor Street, not New Street).

- [ ] **Task 1.6 — `lines/wmr-cross-city.toml`** (WMR Cross-City Line,
  Lichfield–Birmingham New Street–Redditch). Operators: `LM`. Segment
  decision: passes through Birmingham New Street, which XC's existing
  files also touch at the station level (not segment level per XC's own
  precedent) — follow the same "station overlap is fine" pattern.

- [ ] **Task 1.7 — `lines/lnwr-euston-commuter.toml`** (LNWR Euston–Milton
  Keynes–Northampton semi-fast). Operators: `LM`/`LN` per Task 1.0's
  resolution. Segment decision: this is the **direct, already-flagged
  overlap** — `west-coast-main-line.toml` already lists these operator
  codes today precisely because WMT services run on WCML metals between
  Euston and points north, but models no LNWR-specific stations/segments.
  Follow `xc-manchester.toml`'s precedent: station-level overlap with
  WCML's `wcml-london`/`wcml-midlands` segments is expected; don't force
  a shared segment name given WCML's coarser granularity.

- [ ] **Task 1.8 — `lines/lnwr-birmingham-crewe.toml`** (LNWR
  Birmingham/Crewe local services). Operators: `LM`/`LN`. Same
  station-overlap-not-segment-sharing treatment as Task 1.7.

### Task 1.9: Batch verification

- [ ] Run `cargo test -p aggregator -p common -p api` with all of Batch
  1's changes present; confirm every new test from Tasks 1.0–1.8 passes
  alongside the full pre-existing suite.
- [ ] Grep `lines/*.toml` for any segment name used by more than one file
  in this batch that wasn't a deliberate shared-trunk choice documented in
  that task; fix any accidental collision before moving to Batch 2.
- [ ] Confirm `git status` is clean except for the 9 new/modified files
  from this batch, each already committed individually per the Recipe.

---

## Batch 2: Greater Anglia

**Depends on:** nothing (but see the London Overground note above on why
it's sequenced immediately before Batch 3). **Produces:** 6 new files.

Gap-analysis source: "Greater Anglia" section, and Priority item 2
("specifically because of the confirmed, concrete Elizabeth-line-Shenfield
segment-overlap question. Doing this early forces that decision to be made
deliberately... rather than as an afterthought").

- [ ] **Task 2.1 — `lines/greater-anglia-main-line.toml`** (Great Eastern
  Main Line, Liverpool Street–Norwich via Ipswich/Colchester). Operators:
  `LE`. **This is the task that must resolve the flagged decision.** Per
  the gap analysis: `elizabeth-shenfield.toml` runs Liverpool Street →
  Stratford → Shenfield and terminates there; Greater Anglia's mainline
  services physically continue past Shenfield on the same corridor (GA on
  "main" tracks, Elizabeth line on dedicated "electric"/metro tracks from
  around Bethnal Green outward, converging again toward Shenfield, with
  both calling at some of the same intermediate stations — e.g. Ilford,
  Romford). Per `lines/SCHEMA.md`'s junction rule, Shenfield is exactly a
  junction: Elizabeth line terminates there, Greater Anglia continues
  beyond. **Make the call explicitly and document it in the file's
  comments**: either share a segment name with `elizabeth-shenfield.toml`
  for the Liverpool Street–Shenfield stretch (an incident there then
  propagates to both lines), or treat it as station-level-only overlap
  per the `xc-south-coast.toml` precedent (don't force a shared segment
  when exclusive territory diverges far beyond the shared bit). Whichever
  is chosen, add the shared-trunk-or-not regression test the Global
  Constraints require, naming it clearly (e.g.
  `greater_anglia_shenfield_corridor_shares_segment_with_elizabeth_line` or
  `greater_anglia_shenfield_corridor_is_station_overlap_only`, matching
  whichever outcome).

- [ ] **Task 2.2 — `lines/greater-anglia-west-anglia.toml`** (West Anglia
  Main Line, Liverpool Street–Cambridge/King's Lynn). Operators: `LE`.

- [ ] **Task 2.3 — `lines/greater-anglia-stansted-express.toml`**
  (Liverpool Street–Stansted Airport). Operators: `LE`. Segment decision:
  likely shares a trunk with Task 2.2's West Anglia line for at least
  part of the route — research the actual diverging point before
  choosing segment names.

- [ ] **Task 2.4 — `lines/greater-anglia-essex-branches.toml`** (Essex
  branches: Southminster, Clacton/Walton/Braintree via Colchester).
  Operators: `LE`.

- [ ] **Task 2.5 — `lines/greater-anglia-suffolk-branches.toml`** (Suffolk
  branches: Sudbury, Felixstowe, Harwich). Operators: `LE`.

- [ ] **Task 2.6 — `lines/greater-anglia-norfolk-branches.toml`** (Norfolk
  branches: Bittern Line Norwich–Sheringham, Wherry Lines Norwich–
  Yarmouth/Lowestoft, Breckland Line Norwich–Cambridge). Operators: `LE`.

- [ ] **Task 2.7: Batch verification.** Same shape as Task 1.9: full
  workspace test run, segment-collision grep, clean `git status` except
  this batch's 6 commits.

---

## Batch 3: London Overground

**Depends on:** nothing structurally, but sequenced here (see "The London
Overground gap" above). **Produces:** 6 new files.

Source: `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
Area 2. Not covered by the gap-analysis document.

- [ ] **Task 3.0: Research pass — confirm the six lines' real current
  station lists and shared-trunk points.** The tfl-service-metrics-v2
  spec explicitly states this "needs its own research pass" and does not
  itself supply station lists — this task is that pass, done before any
  `.toml` file is drafted, per this plan's ban on inventing topology.
  Deliverable: a short written note (can live as a comment block at the
  top of Task 3.1's file, or a scratch note the batch's tasks share) per
  line — Liberty, Lioness, Mildmay, Suffragette, Weaver, Windrush —
  covering: full ordered station list, which stations are genuine
  junctions with a sibling Overground line (the spec specifically flags
  "the East London and North London corridors" as having multiple shared
  trunk sections), and confirmation that `operators = ["LO"]` is the
  correct single ATOC code for all six (per the spec: "no `\"LO\"`
  operator code... appears anywhere in `lines/` or `crates/`" today,
  meaning this is unverified inside the codebase and must come from an
  external source — TfL's own Overground pages or National Rail
  Enquiries). Verify each fact against a live source, cross-checked
  against a second, exactly as this plan's Global Constraints require.

- [ ] **Task 3.1 — `lines/overground-liberty.toml`**. Operators: `LO`.
  Category: likely `commuter` per the existing `thameslink-core.toml`
  precedent for a dense London metro-style service — confirm against
  Task 3.0's research rather than assuming.

- [ ] **Task 3.2 — `lines/overground-lioness.toml`**. Operators: `LO`.

- [ ] **Task 3.3 — `lines/overground-mildmay.toml`**. Operators: `LO`.

- [ ] **Task 3.4 — `lines/overground-suffragette.toml`**. Operators: `LO`.

- [ ] **Task 3.5 — `lines/overground-weaver.toml`**. Operators: `LO`.

- [ ] **Task 3.6 — `lines/overground-windrush.toml`**. Operators: `LO`.

  For Tasks 3.1–3.6: each must correctly tag any segment shared with
  another of the six per Task 3.0's findings (the East London/North
  London shared-trunk sections the spec calls out), with a shared-trunk
  regression test per pair that actually shares track, following the
  Recipe and the Global Constraints' testing convention exactly as any
  other multi-file operator (CrossCountry, Elizabeth line) already does
  in this catalogue.

- [ ] **Task 3.7: Batch verification.** Full workspace test run, plus an
  explicit check that all six `overground-*` lines' shared segments (per
  Task 3.0) show up correctly in `SegmentRegistry` (i.e. a
  `segments.rs`-style test asserting `is_shared(...)` for each real shared
  segment identified in Task 3.0 — not just the matcher-level propagation
  tests already required per-file). Clean `git status` except this
  batch's 7 commits (6 files + Task 3.0's research note, if committed
  separately as a comment-only diff or folded into Task 3.1's first
  commit).

**Explicitly out of scope for this batch:** the TfL/NR merge itself (the
mapping-table extension, suppress-and-overlay mechanism) described in the
tfl-service-metrics-v2 spec's Area 1/Area 2. This batch only makes that
future work possible by giving the aggregator something to attribute
Overground incidents/samples to; it does not touch `crates/api`,
`crates/poller-tfl`, or `frontend/lib/modes.ts`.

---

## Batch 4: Great Western Railway

**Depends on:** nothing. **Produces:** 6 new files.

Gap-analysis source: "Great Western Railway (GWR)" section, and Priority
item 3 ("the largest single total-absence gap by passenger volume").

- [ ] **Task 4.1 — `lines/gwr-main-line.toml`** (GWML core,
  Paddington–Bristol Temple Meads). Operators: `GW`.

- [ ] **Task 4.2 — `lines/gwr-cotswold.toml`** (Cotswold Line,
  Didcot/Oxford–Worcester). Operators: `GW`.

- [ ] **Task 4.3 — `lines/gwr-south-wales.toml`** (South Wales Main Line,
  branching off the GWML at Wootton Bassett via Bristol Parkway and the
  Severn Tunnel to Newport/Cardiff/Swansea). Operators: `GW`. Segment
  decision: real, flagged overlap at Cardiff/Newport with
  `xc-cardiff.toml`, which already terminates at CDF/NWP — station-level
  overlap expected, follow the existing XC precedent on not forcing a
  shared segment unless the two lines' exclusive territory genuinely
  coincides.

- [ ] **Task 4.4 — `lines/gwr-west-of-england.toml`** (West of England
  Line/Reading–Taunton line through to the Cornish Main Line: Reading →
  Westbury/Castle Cary → Exeter → Plymouth → Penzance, plus the Night
  Riviera sleeper). Operators: `GW`. The gap analysis notes this could
  arguably split into two files (West of England vs. Cornish are
  "operationally distinct halves") — this task takes the single-file
  reading, matching `swr-south-west-main.toml`'s Bournemouth+Weymouth
  precedent; if research during Step 1 makes the two halves feel too
  large/distinct for one file, split into `gwr-west-of-england.toml` +
  `gwr-cornish-main-line.toml` instead and note the deviation from this
  plan's 6-file count in the commit message.

- [ ] **Task 4.5 — `lines/gwr-thames-valley.toml`** (Thames Valley
  suburban, Paddington–Reading–Newbury/Oxford local stopping). Operators:
  `GW`. Segment decision: real, flagged overlap at Reading with
  `xc-south-coast.toml` (already documents "sharing no segment, by
  design" through RDG) and `elizabeth-line.toml`'s western arm (terminates
  at RDG). Same station-overlap judgment call as Task 4.3.

- [ ] **Task 4.6 — `lines/gwr-bristol-suburban.toml`** (Bristol-area
  suburban group: Severn Beach line, Bristol–Weymouth via Bath/Castle
  Cary). Operators: `GW`.

- [ ] **Task 4.7: Batch verification.** Same shape as prior batches.

---

## Batch 5: Southeastern + GTR (Southern/Gatwick Express/Great Northern/Thameslink branches)

**Depends on:** nothing structurally, but internally sequenced (Task 5.6's
Thameslink Sevenoaks-adjacent branch should land before or alongside Task
5.4's Southeastern metro tasks, since they're the one pair the gap
analysis calls "the single clearest segment-coordination dependency found
in this whole audit"). **Produces:** 14 new files.

Gap-analysis source: "Southeastern" and "Southern / Gatwick Express /
Great Northern / Thameslink branches (GTR)" sections, and Priority item 4.

### Southeastern (5 files)

- [ ] **Task 5.1 — `lines/southeastern-main-line.toml`** (South Eastern
  Main Line, Charing Cross/Cannon Street via Tonbridge to
  Ashford/Dover/Hastings). Operators: `SE`.

- [ ] **Task 5.2 — `lines/southeastern-chatham.toml`** (Chatham Main
  Line, Victoria via Chatham to Ramsgate/Dover). Operators: `SE`.

- [ ] **Task 5.3 — `lines/southeastern-highspeed.toml`** (domestic
  "Javelin" HS1 service via Ebbsfleet/Ashford International). Operators:
  `SE`.

- [ ] **Task 5.4 — `lines/southeastern-metro-north-kent.toml`**
  (Dartford Loop, Bexleyheath). Operators: `SE`. **This is the flagged
  coordination task.** Per the gap analysis: Southeastern's metro services
  converge on London Bridge and share tracks south toward Lewisham with
  Thameslink's Sevenoaks branch (Task 5.13 below). Coordinate the segment
  name at London Bridge/Lewisham directly with whichever of Task 5.4 or
  5.13 lands second — read the other's committed file before naming this
  one's segment, and share the name deliberately if the corridor really
  is the same shared trunk (which the gap analysis says it is), per the
  matcher's shared-trunk convention.

- [ ] **Task 5.5 — `lines/southeastern-metro-south-london.toml`** (Sidcup,
  Hayes lines). Operators: `SE`.

### GTR: Southern / Gatwick Express (3 files)

- [ ] **Task 5.6 — `lines/southern-brighton-main-line.toml`** (Brighton
  Main Line, Victoria/London Bridge–Gatwick–Brighton). Operators: `SN`.
  The gap analysis suggests folding Gatwick Express (`GX`) in here "as a
  keyword/threshold variant rather than a full separate file, mirroring
  how this catalogue hasn't split every branded service into its own file
  where journeys are a strict subset." If taken, add `GX` to `operators`
  and document the fold-in decision in a comment; if research shows
  Gatwick Express's non-stop pattern is distinct enough to warrant its
  own file, split it out as `southern-gatwick-express.toml` instead and
  note the deviation.

- [ ] **Task 5.7 — `lines/southern-coastway-east.toml`** (Brighton–
  Eastbourne–Hastings). Operators: `SN`.

- [ ] **Task 5.8 — `lines/southern-coastway-west.toml`** (Brighton–
  Portsmouth). Operators: `SN`.

- [ ] **Task 5.9 — `lines/southern-oxted-uckfield.toml`** (Oxted/Uckfield
  branches). Operators: `SN`.

### GTR: Great Northern (2 files)

- [ ] **Task 5.10 — `lines/great-northern-kings-lynn.toml`** (King's
  Cross–Peterborough/Cambridge/King's Lynn). Operators: `GN`. Segment
  decision: shares ECML "slow line" tracks out of King's Cross as far as
  Alexandra Palace/Potters Bar/Welwyn with LNER (Batch 6) — LNER's task
  doesn't exist yet when this one runs, so document the intended segment
  name in a comment for Batch 6 to pick up, same as Task 1.4 did for
  TfW's North Wales Coast Line.

- [ ] **Task 5.11 — `lines/great-northern-suburban.toml`** (Great Northern
  suburban/Moorgate services). Operators: `GN`.

### GTR: Thameslink branches (3 files)

`thameslink-core.toml`'s own comment already promises these: "Northern
and southern branches... are separate lines."

- [ ] **Task 5.12 — `lines/thameslink-bedford.toml`** (Thameslink's
  northern branch, Bedford–St Pancras–[core]). Operators: `TL`. Segment
  decision: shares Bedford–St Pancras infrastructure with EMR's Midland
  Main Line (Batch 7, not yet written when this runs) — document the
  chosen segment name clearly in a comment; Batch 7's Task 7.1 is required
  to cite it (see Batch 7 below).

- [ ] **Task 5.13 — `lines/thameslink-cambridge.toml`** (Thameslink's
  Cambridge/Peterborough branch). Operators: `TL`.

- [ ] **Task 5.14 — `lines/thameslink-southern.toml`** (Thameslink's
  southern branches: Sutton loop, Sevenoaks, Brighton-via-Thameslink).
  Operators: `TL`. **Paired with Task 5.4** — see that task's note; this
  is the other half of the flagged London Bridge/Lewisham coordination.

- [ ] **Task 5.15: Batch verification.** Full workspace test run (14 new
  files is enough that this is worth running mid-batch too, e.g. after
  Task 5.5 and again after Task 5.11, not only at the very end — catch a
  segment-naming mistake before it's compounded by 9 more files). Explicit
  check that Task 5.4 and Task 5.14's London Bridge/Lewisham segment
  decision is consistent between the two files (same segment name if
  shared was chosen, or both independently and deliberately not sharing
  if that was the call — not one file assuming sharing while the other
  doesn't reference it at all).

---

## Batch 6: LNER + open-access ECML operators + Heathrow Express

**Depends on:** nothing structurally; internally, Task 6.1 (LNER core)
should land before Tasks 6.5–6.7 (Grand Central/Hull Trains/Lumo) so their
King's Cross-area station overlap has something to compare against, though
per the existing XC/WCML precedent this doesn't require a shared segment.
**Produces:** 8 new files.

Gap-analysis source: "LNER (East Coast Main Line)" and "Open access
operators" sections, Priority item 5.

- [ ] **Task 6.1 — `lines/lner-ecml.toml`** (ECML core, King's Cross–
  Edinburgh, extending to Aberdeen/Inverness via ScotRail metals).
  Operators: `GR`. If Batch 5's Task 5.10 left a comment about the
  intended Great Northern slow-line segment name near King's Cross, honor
  it here if a shared segment is genuinely warranted.

- [ ] **Task 6.2 — `lines/lner-leeds.toml`** (Leeds branch via Wakefield
  Westgate, folding Harrogate/Skipton services per the gap analysis's
  suggestion). Operators: `GR`. Segment decision: the gap analysis flags
  a real judgment call here — Northern's `northern-yorkshire` segment
  (LDS↔YRK, already shared between `northern.toml` and
  `northern-yorkshire-coast.toml`) is also LNER territory between Leeds
  and York. Decide, document the reasoning in a comment, and add the
  corresponding shared-trunk-or-station-overlap-only regression test.

- [ ] **Task 6.3 — `lines/lner-hull.toml`** (Hull branch via
  Selby/Brough). Operators: `GR`.

- [ ] **Task 6.4 — `lines/lner-lincoln.toml`** (Lincoln branch via
  Newark/Grantham). Operators: `GR`.

- [ ] **Task 6.5 — `lines/grand-central.toml`** (King's Cross–Sunderland
  and King's Cross–Bradford). Operators: `GC`. Single simple route, closer
  in scope to `swr-alton.toml` than a multi-branch entry, per the gap
  analysis.

- [ ] **Task 6.6 — `lines/hull-trains.toml`** (King's Cross–Hull).
  Operators: `HT`.

- [ ] **Task 6.7 — `lines/lumo.toml`** (King's Cross–Edinburgh, extended
  to Glasgow Queen Street). Operators: `LD`.

- [ ] **Task 6.8 — `lines/heathrow-express.toml`** (Paddington–Heathrow
  non-stop). Operators: `HX`. **Has a live in-repo precedent to follow,
  not just a risk to flag**: `elizabeth-heathrow.toml` already lists
  `"Heathrow Express"` in its `excluded_keywords` specifically because
  "Heathrow Express and the Piccadilly line serve the same terminals and
  turn up constantly in Heathrow messages; neither is this line." As part
  of this task, re-check whether that exclusion still needs to exist once
  a real, correctly-scoped `heathrow-express.toml` exists — the gap
  analysis explicitly calls this out as a required follow-up check, not
  optional cleanup.

- [ ] **Task 6.9: Batch verification.** Full workspace test run. Also
  explicitly confirm Task 6.8's `elizabeth-heathrow.toml` exclusion check
  was actually done (grep the diff for a change to that file's
  `excluded_keywords`, or a comment explaining why none was needed).

---

## Batch 7: East Midlands Railway

**Depends on:** Batch 5 (Task 5.12's Thameslink-Bedford segment-name
choice for the shared Bedford–St Pancras stretch — read that file before
naming this batch's own segment there). **Produces:** 4 new files.

Gap-analysis source: "East Midlands Railway (EMR)" section, Priority item
7 ("sequenced near GTR's Thameslink-branch work... given the Bedford–St
Pancras MML/Thameslink infrastructure overlap").

- [ ] **Task 7.1 — `lines/emr-midland-main-line.toml`** (Midland Main
  Line, St Pancras–Sheffield via Leicester/Derby/Chesterfield, folding in
  the Nottingham spur per the gap analysis's SWML/Weymouth-style
  precedent). Operators: `EM`. **Must cite and, if the two corridors
  genuinely coincide, reuse the segment name `lines/thameslink-bedford.toml`
  (Task 5.12) chose for Bedford–St Pancras** — this is the one dependency
  in this plan that crosses batches by requirement, not just by
  courtesy, per the gap analysis's explicit flag.

- [ ] **Task 7.2 — `lines/emr-regional.toml`** (EMR Regional intercity,
  Liverpool–Norwich via Nottingham/Sheffield). Operators: `EM`. Gap
  analysis notes this crosses Northern/TPE territory around
  Sheffield/Manchester "without literal track-sharing in most places" —
  lower coordination risk than Task 7.1, confirm during research rather
  than assume.

- [ ] **Task 7.3 — `lines/emr-connect.toml`** (EMR Connect, St Pancras–
  Luton Airport local). Operators: `EM`.

- [ ] **Task 7.4 — `lines/emr-rural-branches.toml`** (Robin Hood Line
  Nottingham–Worksop, the Poacher/Skegness line, Derby–Matlock).
  Operators: `EM`.

- [ ] **Task 7.5: Batch verification.** Full workspace test run; confirm
  Task 7.1's Bedford–St Pancras segment decision is consistent with (not
  silently divergent from) `lines/thameslink-bedford.toml`'s.

---

## Batch 8: Northern's real completeness gaps

**Depends on:** nothing. **Produces:** 6 new files.

Gap-analysis source: "Northern — is the existing 6-branch coverage
actually complete?" section, Priority item 8 ("this operator already has
the most-developed segment/branch conventions in the catalogue, so
extending it is comparatively low-risk, high-familiarity work").

- [ ] **Task 8.1 — `lines/northern-cumbrian-coast.toml`** (Barrow-in-
  Furness → Whitehaven → Carlisle). Operators: `NT`. This is the one gap
  already flagged in-repo — `northern-furness.toml`'s own comment says
  "The Cumbrian Coast route continues beyond Barrow to Whitehaven and
  Carlisle and belongs in its own definition." Segment decision: shares a
  trunk with `northern-furness.toml` up to Barrow.

- [ ] **Task 8.2 — `lines/northern-calder-valley.toml`** (Leeds/Bradford
  ↔ Manchester via Halifax and Rochdale). Operators: `NT`. A genuinely
  distinct corridor from `northern.toml`'s existing `northern-transpennine`
  segment (which runs via Huddersfield) — confirm no accidental segment
  overlap given both are "Leeds–Manchester."

- [ ] **Task 8.3 — `lines/northern-airedale.toml`** (Leeds/Bradford
  Forster Square → Skipton, some services on to Carlisle/Lancaster).
  Operators: `NT`. Segment decision: shares track with Task 8.4
  (Wharfedale) as far as Shipley — the gap analysis flags this as "itself
  a shared-trunk consideration for whichever gets written first." Also
  research whether LNER touches this corridor per the gap analysis's note
  ("both LNER and Northern services use parts of Airedale").

- [ ] **Task 8.4 — `lines/northern-wharfedale.toml`** (Leeds/Bradford →
  Ilkley). Operators: `NT`. Shares the Shipley trunk with Task 8.3 — add
  the shared-trunk test regardless of task-execution order.

- [ ] **Task 8.5 — `lines/northern-esk-valley.toml`** (Middlesbrough →
  Whitby, Community Rail line). Operators: `NT`. Gap analysis notes this
  is "entirely separate from anything currently modelled" — no
  shared-segment test needed, document that explicitly per the Global
  Constraints' exception for standalone lines.

- [ ] **Task 8.6 — `lines/northern-clitheroe.toml`** (Manchester/
  Blackburn → Clitheroe, the Ribble Valley Line). Operators: `NT`.

- [ ] **Task 8.7: Batch verification.** Full workspace test run;
  explicit check of the Task 8.3/8.4 Shipley shared-trunk assertion.

**Explicitly deferred, not in this batch:** the gap analysis's own
"Assorted Manchester/West Yorkshire suburban branches not yet checked in
detail (e.g. Manchester–Southport, Wigan–Kirkby)" — it says these are
"flagged for the follow-up curation pass rather than fully scoped here."
This plan does not scope them either, for the same reason: they aren't
confirmed enough yet to turn into a task without inventing detail. A
future revision of the gap-analysis document should confirm them first.

**Follow-up discovered during Batch 8's final review, not fixed in this
batch:** `lines/northern-furness.toml` (one of the pre-existing 20 files,
predating this whole plan) tags **all four** of its stations (LAN, CNF,
ULV, BIF) with the segment name `northern-furness`, not just the junction
station (BIF). Batch 8's Task 8.1 (`lines/northern-cumbrian-coast.toml`)
correctly puts only BIF on that same shared segment name, per SCHEMA.md's
shared-trunk rule of thumb — but because `northern-furness.toml` itself
was never scoped for correction (Global Constraints: a batch never edits
a `lines/*.toml` file it didn't create), the shared segment name is now
used by a second file, so `SegmentRegistry::is_shared("northern-furness")`
returns true for ALL of Furness's stations, not just Barrow. An incident
at Lancaster, Carnforth, or Ulverston — nowhere near the Cumbrian Coast
Line — now resolves to `MatchScope::SharedSegment` instead of
`MatchScope::ExclusiveSegment`, so the Furness Line can no longer report a
purely exclusive incident anywhere on its own exclusive territory. This is
a live behavior change introduced by Batch 8, not a pre-existing bug (it
only manifests once a second file claims the segment name), and no
regression test currently catches it. **Recommended fix, for a future
task (in this batch's own follow-up pass, or bundled into whichever batch
next touches Northern's existing files):** split
`lines/northern-furness.toml` so only BIF keeps the shared
`northern-furness` segment, and move LAN/CNF/ULV onto a new exclusive
segment (e.g. `northern-furness-branch`), matching the same shared-trunk
rule of thumb every other file in this batch already follows. Add a
`segments.rs` or `matcher.rs` regression test asserting a Lancaster/
Carnforth/Ulverston incident matches only `northern-furness` as
`ExclusiveSegment`, mirroring `swr_exclusive_segment_incident_does_not_
propagate`.

---

## Batch 9: TransPennine Express

**Depends on:** nothing, but sequenced after Batch 8 per the gap
analysis's own reasoning ("sequence after Northern's own gaps are filled
so the segment-naming precedent is fresh"). **Produces:** 4 new files.

Gap-analysis source: "TransPennine Express (TPE)" section, Priority item
10. TPE's own current timetable structure organizes into exactly four
named routes — "an unusually clean fit for this catalogue's per-route
file convention."

- [ ] **Task 9.1 — `lines/tpe-anglo-scottish.toml`** (Liverpool/
  Manchester–Glasgow/Edinburgh via Preston/Carlisle). Operators: `TP`.

- [ ] **Task 9.2 — `lines/tpe-south.toml`** (Cleethorpes–
  Manchester/Liverpool via Sheffield/Leeds). Operators: `TP`.

- [ ] **Task 9.3 — `lines/tpe-borders.toml`** (Newcastle–Edinburgh).
  Operators: `TP`.

- [ ] **Task 9.4 — `lines/tpe-north.toml`** (Newcastle–Manchester/
  Liverpool via York/Leeds). Operators: `TP`.

  For all four: heavy station overlap with Northern (Leeds, York,
  Huddersfield, Manchester), WCML (Preston, Carlisle), and LNER
  (Edinburgh, Newcastle) is expected and, per the precedent already set in
  `xc-manchester.toml` ("no segment is shared with wcml because that
  line's segments are far coarser... an incident on the shared stretch
  matches both lines by station anyway"), none of it needs literal
  segment-sharing. Each task's acceptance criteria should explicitly note
  which existing lines it overlaps at the station level and confirm no
  segment name was force-shared without a real reason.

- [ ] **Task 9.5: Batch verification.** Full workspace test run.

---

## Batch 10: ScotRail

**Depends on:** nothing (gap analysis: "no overlap with any
currently-defined line... low coordination risk, purely additive").
**Produces:** 10 new files.

Gap-analysis source: "ScotRail" section, Priority item 11 (first half).
By far the largest single network in the audit.

- [ ] **Task 10.1 — `lines/scotrail-central-belt.toml`** (Edinburgh–
  Glasgow core via Falkirk High, plus alternates via Shotts and via
  Bathgate/North Clyde, grouped per the gap analysis's suggestion).
  Operators: `SR`.

- [ ] **Task 10.2 — `lines/scotrail-glasgow-suburban.toml`** (Glasgow
  suburban electric network: North Clyde, Argyle Line). Operators: `SR`.

- [ ] **Task 10.3 — `lines/scotrail-ayrshire.toml`** (Ayrshire
  Coast/Glasgow South Western Line to Ayr/Stranraer/Girvan). Operators:
  `SR`.

- [ ] **Task 10.4 — `lines/scotrail-fife-borders.toml`** (Fife Circle +
  the Borders Railway, Edinburgh–Tweedbank, grouped per the gap
  analysis's suggestion). Operators: `SR`.

- [ ] **Task 10.5 — `lines/scotrail-highland-main-line.toml`** (Perth–
  Inverness). Operators: `SR`.

- [ ] **Task 10.6 — `lines/scotrail-far-north.toml`** (Inverness–
  Wick/Thurso). Operators: `SR`.

- [ ] **Task 10.7 — `lines/scotrail-kyle.toml`** (Kyle of Lochalsh Line,
  Dingwall–Kyle via Inverness). Operators: `SR`. Segment decision: shares
  a trunk with the Far North Line (Task 10.6) and Highland Main Line (Task
  10.5) around Inverness — research the actual shared stretch.

- [ ] **Task 10.8 — `lines/scotrail-west-highland-fort-william.toml`**
  (West Highland Line, Glasgow–Fort William/Mallaig). Operators: `SR`.
  Segment decision: shares a Glasgow trunk with Task 10.9 (Oban arm) —
  "materially two different routes off a shared Glasgow trunk" per the
  gap analysis.

- [ ] **Task 10.9 — `lines/scotrail-west-highland-oban.toml`** (West
  Highland Line, Glasgow–Oban). Operators: `SR`. Shares the Glasgow trunk
  with Task 10.8 — add the shared-trunk regression test regardless of
  which task lands first.

- [ ] **Task 10.10 — `lines/scotrail-aberdeen-inverness.toml`**
  (Aberdeen–Inverness Line). Operators: `SR`.

- [ ] **Task 10.11: Batch verification.** Full workspace test run
  (worth an intermediate run after Task 10.5 given 10 files); explicit
  check of the Task 10.8/10.9 Glasgow-trunk assertion and the Task
  10.5/10.6/10.7 Inverness-area assertions.

---

## Batch 11: Transport for Wales

**Depends on:** Batch 1 (Task 1.4's `wcml-north-wales.toml` segment-name
choice for Chester–Holyhead, since TfW's North Wales Coast Line shares
that corridor with Avanti). **Produces:** 7 new files.

Gap-analysis source: "Transport for Wales (TfW)" section, Priority item
11 (second half). Operator code `AW` — confirmed correct in the P0
section (Batch 1, Task 1.0), and worth double-checking against Task 1.0's
own fix before drafting these files, since the gap analysis specifically
calls out that this exact code was already found confused with Avanti's
elsewhere in the catalogue.

- [ ] **Task 11.1 — `lines/tfw-cambrian.toml`** (Cambrian Line,
  Shrewsbury–Aberystwyth, splitting to Pwllheli as the Cambrian Coast
  Line). Operators: `AW`.

- [ ] **Task 11.2 — `lines/tfw-heart-of-wales.toml`** (Heart of Wales
  Line, Shrewsbury/Craven Arms–Swansea). Operators: `AW`.

- [ ] **Task 11.3 — `lines/tfw-conwy-valley.toml`** (Conwy Valley Line,
  Llandudno Junction–Blaenau Ffestiniog). Operators: `AW`.

- [ ] **Task 11.4 — `lines/tfw-north-wales-coast.toml`** (North Wales
  Coast Line, Chester–Holyhead, shared with Avanti West Coast).
  Operators: `AW` (and confirm whether `VT` should also appear, given the
  shared corridor, following how `west-coast-main-line.toml` already lists
  multiple operators for a shared line). **Read
  `lines/wcml-north-wales.toml` (Task 1.4) before naming this file's
  segment(s)** — that task deliberately left its segment-name choice
  documented for this task to pick up; honor whatever decision it recorded
  rather than picking a name independently.

- [ ] **Task 11.5 — `lines/tfw-marches.toml`** (Marches Line, Newport–
  Shrewsbury–Chester). Operators: `AW`.

- [ ] **Task 11.6 — `lines/tfw-valley-lines-north.toml`** (Cardiff Valley
  Lines, northern group: Rhymney, Merthyr, Aberdare, Treherbert).
  Operators: `AW`. Gap analysis notes this dense commuter network is
  "structurally more like Merseyrail's two-line metro than a long-distance
  route" — this task and Task 11.7 are this plan's reading of how to split
  it; if research shows a different natural split, deviate and note why.

- [ ] **Task 11.7 — `lines/tfw-valley-lines-south.toml`** (Cardiff Valley
  Lines, southern group: City Line, Coryton branch, and any other
  Cardiff-area circle services not covered by Task 11.6). Operators: `AW`.
  Segment decision: also check the gap analysis's flagged South Wales
  Main Line overlap (Newport/Cardiff/Swansea stations shared with
  `xc-cardiff.toml` and Batch 4's `gwr-south-wales.toml`) at the station
  level, same pattern as elsewhere.

- [ ] **Task 11.8: Batch verification.** Full workspace test run;
  explicit check that Task 11.4's segment choice matches what Task 1.4
  documented.

---

## Batch 12: Chiltern, c2c, Merseyrail

**Depends on:** nothing. **Produces:** 5 new files.

Gap-analysis source: "Chiltern Railways," "c2c," and "Merseyrail"
sections, Priority item 12 ("the three cleanest, lowest-risk,
smallest-scope gaps in the whole audit... don't unblock or get blocked by
anything else").

- [ ] **Task 12.1 — `lines/chiltern-main-line.toml`** (Marylebone–
  Birmingham Snow Hill main line via High Wycombe, Bicester, Warwick,
  with a Stratford-upon-Avon extension). Operators: `CH`. No significant
  segment overlap with anything currently defined per the gap analysis —
  Chiltern's Birmingham approach via Solihull/Dorridge is distinct from
  both XC's and any WMR Snow Hill entry's segments (station-level overlap
  only, at Birmingham Snow Hill/Moor Street, which XC doesn't touch — XC
  uses New Street).

- [ ] **Task 12.2 — `lines/chiltern-aylesbury.toml`** (Marylebone–
  Aylesbury stopping service via Amersham, folding the Bicester–Oxford
  branch per the gap analysis's `swr-alton.toml`-style precedent; the
  branch diverges via a purpose-built chord near Bicester Village, not
  through Bicester North — see the correction in
  `lines/chiltern-aylesbury.toml`'s own comments). Operators: `CH`.

- [ ] **Task 12.3 — `lines/c2c.toml`** (Fenchurch Street–Shoeburyness via
  Basildon, the London, Tilbury & Southend line, including the minor
  Ockendon loop branch as part of the same file). Operators: `CC`. No
  overlap found with anything else in the catalogue per the gap analysis
  — standalone line, no shared-trunk regression test needed (document
  this exception explicitly per the Global Constraints).

- [ ] **Task 12.4 — `lines/merseyrail-northern.toml`** (Merseyrail
  Northern Line, Southport–Hunts Cross with Kirkby and Ormskirk branches,
  underground through central Liverpool). Operators: `ME`. Per the gap
  analysis, worth a note-to-self confirming no literal station-code
  collision with `northern.toml`'s `northern-merseyside` segment (LIV–
  HUY–NLW) — different Liverpool terminus (Lime Street vs. the
  underground loop via Liverpool Central) and different electrification,
  low risk but confirm during research rather than assume.

- [ ] **Task 12.5 — `lines/merseyrail-wirral.toml`** (Merseyrail Wirral
  Line, a loop via Liverpool Central/Moorfields out to New Brighton, West
  Kirby, Chester, Ellesmere Port). Operators: `ME`.

- [ ] **Task 12.6: Batch verification.** Full workspace test run; this is
  also the plan's final batch, so also run a full `cargo test --workspace`
  once (not just the three `lines/`-consuming crates) to catch any
  unrelated regression, and confirm `git status` is clean with nothing
  outside `lines/*.toml` and the specific `crates/aggregator/src/*.rs`
  test files touched across all 12 batches.

---

## What this plan does not do (explicit, repeated from Global Constraints)

- Does not integrate any new data source, poller, or feed.
- Does not add or change aggregator/matcher production logic — only its
  test coverage and its `lines/` input grow.
- Does not touch the frontend.
- Does not perform the London Overground TfL/NR merge itself (mapping
  table, suppress-and-overlay) — Batch 3 only produces the prerequisite NR
  line definitions the tfl-service-metrics-v2 spec's Area 1/Area 2 already
  scopes that future work around.
- Does not scope Northern's "not yet checked in detail" suburban branches
  (Manchester–Southport, Wigan–Kirkby) — the gap analysis itself declined
  to fully scope these, and this plan follows suit rather than inventing
  detail to fill the gap.
- Does not revisit or re-prioritize the gap analysis's own ordering —
  batch order here is a direct, cited application of its "Prioritized
  list" section, not a new judgment call.
