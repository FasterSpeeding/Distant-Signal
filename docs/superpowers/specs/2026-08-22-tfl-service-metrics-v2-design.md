# TfL Service Metrics v2 Design

## Problem

The TfL line-status integration (v1, see
`docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`) ingests
only TfL's `/Line/Mode/{modes}/Status` endpoint — line-level severity and
reason text, with `sample_stats` always `None` on every TfL-sourced
`LineStatus` (`common::LineStatus.sample_stats`, set to `None` in
`crates/poller-tfl/src/schema.rs`). National Rail lines, by contrast, get
real `sample_stats` (`total`, `delayed`, `cancelled`, `skipped`,
`avg_delay_minutes`) from the LDBWS/Darwin-based aggregator
(`crates/aggregator`).

A follow-up investigation (spike, 2026-08-22) into closing this gap found
four materially different situations depending on the TfL mode, rather than
one uniform problem:

- **Elizabeth line** already has real `sample_stats` — via National Rail,
  under a second, entirely separate `line_status` row the TfL integration
  doesn't know about. The fix is a merge, not new data.
- **London Overground** has no National Rail counterpart at all today — the
  fix is new line-definition curation, not a merge (yet).
- **DLR** has no NR-equivalent, but TfL's own data (Arrivals predictions +
  timetable) could plausibly support a bespoke inference pipeline, at real
  engineering cost.
- **Tube (network-wide) and Tram** have no viable path to real
  delay/cancellation metrics at all, for different structural reasons.

This spec documents all four, at the level of design detail each one's
readiness supports, so they can be independently planned and implemented
rather than treated as one v2 feature.

## Goals

- Eliminate the Elizabeth line duplicate-entry problem: one line, one
  status, backed by the richer (NR) data source where both exist.
- Establish what it would take to give London Overground real
  `sample_stats`, as groundwork for a future Elizabeth-line-style merge.
- Scope a concretely-defined, technically defensible pilot for DLR
  delay inference — approved to move into implementation planning
  alongside area 1.
- Record, with reasoning, why Tube and Tram are out of scope — so this
  doesn't get re-investigated from scratch later.

## Non-goals

- No station/stop-level detail beyond what's described in areas 1–3 below
  (no per-service or per-stop breakdowns anywhere in the UI).
- No new `common::DataQuality` variant beyond the existing `Tfl` and
  `LdbwsInferred` — none of the four areas need one.
- No unification of TfL's and National Rail's severity models beyond what
  the Elizabeth line merge (area 1) and Overground groundwork (area 2)
  already require.
- No Tube or Tram work of any kind (area 4 explains why, so this is a
  decision record, not an oversight).

## Area 1: Elizabeth line duplicate-entry merge

### Current state

Two independent `line_status` rows exist for the same railway, distinguished
only cosmetically:

- **NR/aggregator row**: `line_id = "elizabeth-line"`, `source =
  'aggregator'`, `operators = ["XR"]`. Defined across three catalogue files —
  `lines/elizabeth-line.toml` (Reading–Abbey Wood trunk + west branch),
  `lines/elizabeth-heathrow.toml`, `lines/elizabeth-shenfield.toml` (the
  other two branches, sharing the `elizabeth-central`/`elizabeth-west`
  segments per `lines/SCHEMA.md`'s shared-trunk rules). Carries real
  `sample_stats` from LDBWS sampling.
- **TfL row**: `line_id = "tfl-elizabeth"` (via `common::TFL_LINE_ID_PREFIX`
  applied in `crates/poller-tfl`), `source = 'tfl'`, `operators =
  [common::TFL_OPERATOR]` (`"TfL"`). `sample_stats` always `None`.

Both are written to the same `line_status` table (see
`crates/api/src/data/queries.rs::upsert_tfl_line_status`, which scopes all
its writes/deletes to `source = 'tfl'`, and the aggregator's own write path,
which never touches `source = 'tfl'` rows — `prune_removed_lines` is
correctly scoped away from them). `crates/api/src/routes/lines.rs::list_lines`
(lines 89–135) queries catalogue lines and `queries::tfl_line_summaries`
(`source = 'tfl'` only) independently and concatenates both into
`/public/lines`, with no deduplication step. The only differentiation
applied today is cosmetic: `tfl_display_name` (line 144) appends `" (TfL)"`
to the TfL row's name, purely so the All Lines table (which has no
Category/Operators column) doesn't show two identically-named rows with no
way to tell them apart.

### Design

1. **Mapping table.** Add a small explicit map from TfL line id to NR
   catalogue line id, starting with one entry:
   `"tfl-elizabeth" → "elizabeth-line"`. Keep it as a literal `const` or
   small static map near `TFL_LINE_ID_PREFIX` in `crates/common/src/lib.rs`
   — this is metadata about the railway, not configuration, and there's
   exactly one entry until area 2 (Overground) produces more.
2. **Suppress the TfL row from `/public/lines`.** In
   `crates/api/src/routes/lines.rs::list_lines`, when building the `tfl`
   vec (lines 126–132), skip any TfL row whose `id` (pre-prefix TfL id) is a
   key in the mapping table — it must not appear as a second entry in the
   list response.
3. **Overlay TfL's status onto the NR row.** Extend the API's line-detail
   response (wherever `render.rs::to_tfl_shape`/the line-status route reads
   the NR row for `"elizabeth-line"`) to also fetch the suppressed TfL row's
   current `statuses` and expose it as a secondary field — e.g.
   `LineStatusReport` gains an optional side-channel
   (`tfl_status: Option<Vec<LineStatus>>` or similar, exact shape TBD in
   implementation) rather than merging into the primary `statuses` list.
   The NR row's `statuses`/`sample_stats` stay primary and unmodified,
   since only that side has real sample data.
4. **Frontend.** `frontend/lib/types.ts`'s line-status type gains the
   matching optional `tflStatus` field. The line detail page renders it as
   a clearly-labeled secondary status line (e.g. "TfL also reports: ...")
   only when present and only for lines that have a mapping entry — no
   change needed for every other line.
5. **History stays untouched.** `line_status_history` keeps recording both
   sources exactly as it does today (one row per `source` per change) — the
   merge is a read-time presentation concern only, not a storage change.
   Do not write a synthesized/merged row into `line_status_history` under
   either `line_id`.

### Hard constraint

Do **not** reuse the aggregator's existing volatile reason-annotation
pattern (`crates/aggregator/src/aggregation.rs:879`,
`(sample_severity, Some(format!("live samples show: {reason}")))`, and the
similar `" (most cited: {most_common})"` pattern at line 759) for any
merged or synthesized text produced by this overlay. That exact
pattern — sample-derived text appended into `reason`, changing on every
aggregation cycle even when the underlying disruption hasn't — is the
confirmed root cause of a separate, currently-under-investigation
line-history duplication bug (near-duplicate history rows a few minutes
apart, same disruption). Any text this overlay produces must be stable
across polling cycles for unchanged input, and the overlay logic overall
must be read-time/display-only, as stated above.

## Area 2: Overground — groundwork only, not yet a merge

### Current state

Unlike Elizabeth line, **Overground is not ingested on the NR side at
all**: no `lines/overground-*.toml` files exist, and no `"LO"` operator
code (Overground's National Rail TOC code) appears anywhere in `lines/` or
`crates/`. The NR/aggregator pipeline has no `LineDefinition` to attribute
any Overground-related Knowledgebase incident or LDBWS sample to, so it
produces zero `line_status` rows for Overground today.

On the TfL side, `crates/poller-tfl` already reports Overground correctly
under its current (Nov 2024 rebrand) structure — six separate lines
(Liberty, Lioness, Mildmay, Suffragette, Weaver, Windrush) — because it
just parses whatever `id`/`name` values TfL's own API returns, with no
hardcoded line list. All six already exist as `source = 'tfl'` rows with
`sample_stats: None`, same as every other TfL line.

### Design (groundwork)

This section is explicitly **not** ready for an implementation plan the way
area 1 is — it describes what has to exist before an Elizabeth-line-style
merge becomes possible:

1. Author six new `lines/overground-*.toml` files, one per current line
   name, each with `operators = ["LO"]`, a verified station list, `mode =
   "national-rail"`, and `segment` tags per `lines/SCHEMA.md`'s shared-trunk
   rules — the six lines share trunk sections in multiple places (e.g. the
   East London and North London corridors), so getting `segment` boundaries
   right matters for correct incident-scope inference the same way it does
   for Elizabeth line's three-file split today.
2. Once these definitions exist and the aggregator starts producing real
   `sample_stats`-bearing `line_status` rows for the six Overground line
   ids, area 1's mapping-table approach extends directly: six more entries
   (`tfl-<id> → overground-<name>`), same suppress-and-overlay mechanism,
   no new architecture needed.

### Open item

Verified station lists and segment boundaries for all six Overground lines
are not established by this spec — that curation is comparable effort to
the existing three-file Elizabeth line definitions, times six, and needs
its own research pass before implementation planning starts.

## Area 3: DLR arrivals-diffing pilot

### Current state

TfL's Unified API has no Darwin-equivalent endpoint for any mode — no
endpoint returns aggregate delay/cancellation/skipped-station counts
directly. The only path toward DLR-specific metrics is inference:

- `GET /Line/dlr/Arrivals` returns real-time predictions for every stop on
  the DLR network in a single bulk call (not per-stop-point polling —
  confirmed via TfL API docs). Each `Prediction` carries a stable
  `vehicleId`, so an individual train is trackable across successive polls.
- `GET /Line/dlr/Timetable/{stopPointId}` returns DLR's scheduled service,
  which would need to be matched against live predictions per `vehicleId`
  to compute an actual delay (predicted-vs-scheduled), and to infer a
  cancellation when an expected scheduled trip's vehicle never appears in
  predictions.

Request volume is not the constraint: DLR-wide coverage is one bulk call
per poll cycle, and even a tight 30s interval (~2 req/min for DLR alone)
sits far under TfL's 500 req/min app-key rate-limit tier. The real cost is
that **the timetable-matching logic doesn't exist anywhere in this
codebase** — mapping a live prediction to the specific scheduled trip it
corresponds to is a new capability, not an extension of the existing
severity-mapping or LDBWS-sampling patterns.

DLR specifically (rather than Tube) is the defensible scope for this: it's
a single line, ~45 stations, fixed-consist, driverless, and genuinely
schedule-adherent — closer in character to a National Rail branch line than
to the Tube's frequency-based operation.

### Design sketch (not a full plan)

- New poller (or new mode within `poller-tfl`) polling `/Line/dlr/Arrivals`
  on a short interval (candidate: 30–60s, well within rate limit).
- A timetable-matching component: fetch/cache DLR's scheduled timetable,
  match each live `Prediction` to its scheduled trip by `vehicleId` +
  approximate time window, compute delay as predicted-minus-scheduled, and
  flag a scheduled trip as a cancellation candidate if no matching
  prediction appears within some grace window past its scheduled time.
- Feed the result into a `SampleStats`-shaped value on the DLR `line_status`
  row, following the existing field semantics
  (`total`/`delayed`/`cancelled`/`avg_delay_minutes`); `skipped` has no
  clear DLR analogue (no calling-point skip concept for a metro service)
  and would likely stay `0`/unused.

### Status

Approved to move into implementation planning alongside area 1. It
remains meaningfully higher-risk and higher-effort than areas 1–2 (new
poller, new inference subsystem, no existing pattern to extend) — whoever
plans it should carry that risk profile forward rather than treat it as a
routine extension of the existing TfL/NR patterns.

## Area 4: Explicitly out of scope, permanently

- **Tube, network-wide.** Same technique as area 3 in principle, but at
  6×+ the scale (11 lines, ~272 stations) and against a fundamentally
  frequency-based operating model where TfL itself doesn't track per-train
  cancellations — "cancellation" is not a well-defined concept for the
  Tube the way it is for a timetabled service. The matching-logic cost of
  area 3 gets substantially worse with a payoff that gets substantially
  fuzzier. Not reconsidered unless TfL's own API starts exposing
  schedule-adherence data directly.
- **Tram.** Has a nominal published timetable
  (`tfl.gov.uk/tram/timetable/tram/`), but is managed and reported on as a
  frequency service in practice (TfL's own guidance: trams "can easily be
  late or early"), with no PPM/cancellation-style industry metric — Tram
  isn't part of the national rail PPM framework at all. Unlike Overground
  and Elizabeth line, Tramlink is TfL/concession-owned infrastructure, not
  Network Rail track, so there is no NR/Darwin feed to surface as a
  shortcut. Any real data would need the same Arrivals-diffing approach
  already judged not worthwhile for Tube.
- **General TfL Arrivals-diffing (beyond DLR's bounded case).** A TfL tech
  forum thread
  (`techforum.tfl.gov.uk/t/inferring-delays-from-arrival-predictions`)
  describing ETA-drift-based delay inference was investigated as prior art.
  It concerns **buses**, not rail, is written by third-party developers
  (not TfL staff), describes delay inference only (no cancellation
  detection), and its own final reply argues the technique is unreliable
  even for its original bus use case. This is corroborating evidence, not
  the sole reason, that Arrivals-diffing outside DLR's narrower/more
  bounded case is not a well-trodden path.

## Testing plan

Testing plans for areas 2 and 3 are deferred to their own implementation
plans: area 2 because Overground's line definitions don't exist yet, and
area 3 because this spec sketches DLR's approach at a design level, not
enough implementation detail to write meaningful test cases yet — approval
to proceed doesn't change that; test cases land with area 3's own
implementation plan. For area 1, once planned:

- `crates/api/src/routes/lines.rs`: `list_lines` test asserting the mapped
  TfL row (`tfl-elizabeth`) is absent from `/public/lines` output while the
  NR row (`elizabeth-line`) is present — extends the existing
  `tfl_display_name` unit tests (lines 328–329) rather than replacing them,
  since the suffix logic still applies to every *unmapped* TfL line.
- Overlay-fetch test: NR row's line-detail response includes the TfL row's
  current status under the new secondary field when both exist; absent
  when only one source has data.
- Regression test confirming no code path produces `reason` text that
  changes between identical polling cycles for the merged Elizabeth line
  presentation, guarding directly against the hard constraint above.
- `line_status_history` test confirming history rows are unaffected by the
  overlay — both sources keep writing independently, exactly as before.

## Open items

- Overground station-list and segment curation (area 2) needs its own
  research pass before an implementation plan can be written — this spec
  intentionally does not invent station lists.
- The DLR pilot (area 3) is approved to proceed to implementation
  planning — it should be scoped as its own implementation plan, separate
  from area 1's Elizabeth line merge, rather than bundled together (the two
  have different risk profiles and different readiness levels).
- The Elizabeth line merge (area 1) should be re-checked once the separate
  line-history-duplication bug fix lands, in case that fix changes how
  `reason` text is produced or annotated in ways that affect what "stable
  merged text" means for the overlay.
