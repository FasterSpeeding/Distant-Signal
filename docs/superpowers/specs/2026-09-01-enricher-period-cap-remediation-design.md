# Enricher `MAX_PERIODS` Soft-Cap Remediation — Design

**Status: design proposal, not approved.**

This spec turns
`docs/superpowers/specs/2026-09-01-enricher-period-cap-failures-research.md`
(root-cause research, merged to `main`, not re-litigated here) into a
concrete design across the three axes that research doc's own
recommendation ranked. It does not re-derive the root cause: a schema-forced
structural cause (`ExtractionPeriod.schedule_window` is one nested object
per period, not an array), a real prompt-coverage gap (no worked example or
eval fixture for day-of-week variation across multiple route legs within one
shared date range), and a confirmed retry-forever failure mode (a cap breach
fails before `queries::write_extraction` runs, so `source_text_hash`/
`extraction_model_version` never advance, and both the hourly sweep and the
no-delivery-limit reclaim loop re-select and re-fail the same incident
indefinitely). Written to the same rigor as
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` (a
recent multi-area design spec covering independently-planned sub-scopes) and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(Decisions-with-real-alternatives-weighed structure). No code was edited to
produce this document.

## Goal

For each of the research's three ranked recommendations, produce a design
concrete enough to plan from:

1. Resolve whether the day-of-week-within-shared-date-range-across-legs
   case is fixed by prompt guidance alone, or needs a schema change too —
   and if prompt-only, draft the actual new guidance text, a worked
   example, and the eval fixture(s) it needs.
2. Design the *process* for choosing a new `MAX_PERIODS` value once (1)
   lands — not a number picked now.
3. Design a graceful-degradation failure mode for whatever cap is chosen,
   replacing "hard fail, retry forever, lose all signal" — plus how this
   stops being silent to operators.

## Current relevant state (re-verified this session)

- `MAX_PERIODS: usize = 8` (`crates/enricher/src/llm.rs:179`), enforced at
  `llm.rs:440-445` inside `LlmClient::extract_primary`
  (`llm.rs:420-447`), immediately after JSON parsing and the
  empty-`periods` check, and strictly *before* `extract_adversarial`/
  `extract_severity_adversarial`/`combine::combine_periods` ever run.
- `ExtractionPeriod` (`llm.rs:54-80`) has exactly one
  `schedule_window: Option<ScheduleWindow>` field. The JSON schema
  (`primary_schema()`, `llm.rs:185-222`) types it `["object", "null"]`
  (`llm.rs:204-212`) — never an array. `ScheduleWindow` itself
  (`llm.rs:42-49`) is `days_of_week: Vec<u8>` + `start_time`/`end_time`
  strings — one weekly rule per period, full stop.
- **All consumers of `ExtractionPeriod.schedule_window`** (grep-verified,
  `grep -rn schedule_window crates/`):
  - `crates/enricher/src/llm.rs` — the field's own definition/schema/prompt
    text, and the adversarial passes' read-only skeleton
    (`build_period_user_content`, `llm.rs:481-499`, which includes
    `schedule_window` in what's shown to both adversarial passes but never
    lets them modify it).
  - `crates/enricher/src/combine.rs:144` — `combine_periods` clones
    `period.schedule_window` straight through unmodified into the
    combined `ExtractionPeriod`; it is not an adversarial-checked field
    (no verdict type carries it).
  - `crates/aggregator/src/aggregation.rs` — the sole *behavioral*
    consumer: a private mirror struct `ScheduleWindow` (`aggregation.rs:
    301-306`) and `ExtractionPeriod.schedule_window: Option<ScheduleWindow>`
    (`aggregation.rs:341`), read in exactly two places: (a)
    `has_recurring_schedule` (`aggregation.rs:258-264`) — `Active` periods
    with `schedule_window.is_some()` and high `resolution_status_confidence`
    exempt an incident from the rail-day age cutoff (recurring-disruption
    detection); (b) `apply_extraction`'s per-`Active`-period loop
    (`aggregation.rs:617-624`) — `now_within_window` (`aggregation.rs:
    456-494`) demotes to `Severity::MinorDelays` with a "reported active
    HH:MM-HH:MM only" annotation when `now` falls outside the window.
  - `crates/enricher/src/queries.rs:51` (a doc-comment reference only, to
    the *deprecated* flat `extracted_schedule_window` column
    `write_extraction` no longer writes — not a live consumer of the new
    shape).
  - No frontend/API code reads `schedule_window` at all (not present in
    `frontend/lib/types.ts` or any `crates/api` route — `extracted_periods`
    is stored as opaque JSON and never re-exposed on the public API; it is
    consumed only inside the aggregator's severity computation).
- `PRIMARY_PROMPT` (`llm.rs:224-274`, read in full) contains one worked
  example: a clean two-phase platform closure, each phase with its own
  single `schedule_window`-free date range and no route-leg variation. Its
  explicit anti-over-segmentation instruction: *"do NOT split for
  stylistic variation, repeated wording, or several stations/lines listed
  under one shared date range — that is still one period"* (`llm.rs:
  229-231`). It says nothing about what to do when a shared date range's
  sub-rule genuinely differs by leg.
- The live-eval battery (`llm.rs:896-1081`) has exactly three fixtures:
  `WANDSWORTH_TOWN_*` (two sequential date ranges, `llm.rs:921-931`),
  `FLAT_ETA_*` (single fact, `llm.rs:933-935`), `TRAP_*` (three stations,
  one shared date range, must stay one period, `llm.rs:939-943`). None
  combines more than one date range with more than one `schedule_window`
  variant inside a date range. Fixtures are plain `const &str` pairs fed
  through `run_battery_attempt` (`llm.rs:954-979`) inside
  `live_eval_battery` (`llm.rs:981-996`, `#[ignore]`d, run manually against
  `LLM_BASE_URL`), plus, for the Wandsworth Town fixture specifically, a
  dedicated test with a **soft** assertion (`live_eval_wandsworth_town_
  segments_into_two_periods`, `llm.rs:1000-1044`: logs a `NOTE:` line
  rather than failing the test when period count doesn't match, "since a
  single run should [not] assert pass/fail on" a segmentation-reliability
  risk, `llm.rs:1034-1043`).
- The two real example texts named in this task's brief are not
  reproduced in the research doc itself, but real quoted fragments of both
  survive in a sibling same-day research doc,
  `docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md`
  (read in full for this section): **Example 1** ("Barrhead–Dumfries"),
  quoted fragments — "Buses replace trains between Barrhead and
  Kilmarnock / Dumfries", "Buses operate between Kilmarnock and Troon,
  where passengers can connect with trains to / from Ayr" (Saturday leg),
  "No scheduled services operate between Kilmarnock and Ayr / Stranraer on
  Sundays" — with the sibling doc's own illustrative segmentation table
  (lines 325-330): a **29 Aug–11 Sep** date range containing three
  co-existing legs (Glasgow Central–Kilmarnock/Dumfries/Carlisle, all
  days; Kilmarnock–Ayr Mon–Sat, buses; Kilmarnock–Ayr/Stranraer Sunday, no
  service at all — "not a weaker version of 'buses replace trains'... a
  categorically different fact", lines 332-334), followed by a **12–13
  Sep** date range repeating a similar three-way structure with Carlisle
  swapped for Dumfries. **Example 2** ("Norwood Junction"), quoted
  fragments — "buses will replace trains... via Norwood Junction,"
  Monday–Thursday overnight, plus a second, undated, vaguely-scoped clause
  "Some trains will be diverted via alternative route" (lines 344-352).
  Neither text's actual full prose is available in this repo; the fragments
  above are the only parts confirmed quoted anywhere in this session's
  reachable files. This spec's worked example and eval fixture (below) are
  built from these confirmed fragments plus the sibling doc's own
  segmentation table, not from invented prose filling the gaps.
- `process_incident` (`crates/enricher/src/main.rs:252-347`): on
  `primary_result` being `Err` (a cap breach today), logs
  `tracing::error!` and returns `false` at `main.rs:287-290`, before
  `write_extraction` (`main.rs:340`) or any adversarial call. This crate
  already has one precedent for a *deliberately similar but distinct*
  operational-visibility mechanism: `MismatchTracker`
  (`main.rs:150-180`), a `Mutex<HashMap<incident_id, u32>>` counting
  consecutive `CombineError` failures per incident, exposed as the gauge
  `enricher_mismatch_incidents` (`main.rs:376`, set every `reclaim_loop`
  tick) and escalated to `tracing::error!` with a distinguishing message
  once `consecutive > 1` (`main.rs:320-336`). `record_llm_call_metrics`
  (`main.rs:221-233`) is the other precedent: a `histogram!` (call
  duration) plus a `counter!` (`enricher_llm_call_total`, labeled `call`
  and `outcome` — `"success"`/`"error"` only, deliberately not
  cardinality-risky per-incident data, per its own doc comment,
  `main.rs:216-220`).
- The sweep's mismatch check (`sweep::incidents_needing_extraction`,
  `sweep.rs:28-37`) re-selects a row whenever its stored
  `extraction_model_version` doesn't equal the *literal* `current_model_
  version` string the caller passes in — which is always the single
  `model_version` value computed once in `main` (`main.rs:75`,
  `format!("{}@periods-v1", config.llm_model)`) and threaded unchanged into
  every `process_incident` call site (stream loop, sweep, reclaim). This is
  load-bearing for Decision 3 below: any per-row *variant* of that string
  (e.g. a `+truncated` suffix) would permanently mismatch the sweep's
  single comparison value and reintroduce exactly the retry-forever
  pattern this axis exists to close.
- `severity_rank` (`crates/common/src/lib.rs:108-131`) and `Severity`
  (`lib.rs:22-61`): the true (non-discriminant) severity ordering already
  used throughout `aggregation.rs` for "which of these is worse."
  `escalation_ceiling` (`aggregation.rs:500-507`) maps `apparent_severity`
  strings to a `Severity` ceiling: `blocked_or_suspended` →
  `PartSuspended` (rank 4), `severe_disruption` → `SevereDelays` (rank 4),
  `moderate_disruption` → `ReducedService` (rank 3), anything else → no
  escalation.
- `apply_extraction`'s `Active`-period branch (`aggregation.rs:602-625`)
  only emits a `reason`-text annotation for `resolution_status ==
  "resolved"`, `"residual"`, or a schedule-window exclusion — the `_ =>
  {}` arm at `aggregation.rs:615` means an `"ongoing"` period contributes
  **no** annotation at all. This is load-bearing for Decision 3's choice
  not to rely on a synthetic "ongoing" period for end-user-visible
  truncation signal (see Decision 3).
- `apply_extraction`'s annotations are `.join("; ")`-joined
  (`aggregation.rs:639, 658`) into one `reason` string per incident.

## Decisions

### Decision 1 (Axis 1) — Prompt-only fix; no schema change

**Chosen: fix the day-of-week-within-shared-date-range-across-legs gap
entirely at the prompt level** (new guidance text + a new worked example +
a new eval fixture). **Rejected: changing `schedule_window` to an array of
windows per period.**

**Why the schema change doesn't actually solve the motivating failure.**
The array-of-windows idea (`schedule_window: Vec<ScheduleWindow>` per
period) only helps when the *only* thing varying by day-of-week is the
time membership of one otherwise-identical fact — same
`apparent_severity`, same `resolution_status`, same `scope_description` —
so two time windows could legitimately describe one semantic "scope."
Example 1, the actual motivating case, is not that shape: within its
shared **29 Aug–11 Sep** date range, the Kilmarnock–Ayr leg's Mon–Sat
sub-rule ("buses operate... connect with trains") and its Sunday sub-rule
("no scheduled services... at all") are not two time-windows over one
fact — they are two *categorically different facts* (per the sibling
research doc's own framing, quoted above: "not a weaker version of 'buses
replace trains'"). Representing that pair still needs two distinct
`resolution_status`/`apparent_severity` payloads regardless of whether
`schedule_window` is singular or an array, because severity and
resolution status themselves differ by day-of-week here, not just the
clock/day membership. An array-of-windows change would add real
invasiveness (touches `ExtractionPeriod`, `primary_schema()`,
`ADVERSARIAL_PROMPT`/`SEVERITY_ADVERSARIAL_PROMPT`'s per-period skeleton,
`combine::combine_periods`, and both `aggregation.rs` consumers listed
above) for a case it would not actually collapse. It would only reduce
period count for a narrower case this incident doesn't demonstrate
(identical severity/resolution/scope, differing only in *when* within the
week) — real, but not the failure this remediation is aimed at, and not
evidenced as common enough on its own to justify the change now. If a
future incident demonstrates that narrower case driving its own cap
breaches, it can be evaluated then, on its own evidence — not spent here
on a case it wouldn't fix.

**What the prompt-only fix actually needs to do**, per the research's own
framing (`failures-research.md`, Recommendation 1): teach the model to
*group what can genuinely be grouped* (legs sharing an identical
day-of-week rule, severity, and resolution status within one date range
stay one period with a broader `scope_description`) while still splitting
per (leg × distinct day-of-week/severity/resolution combination) where the
text genuinely states different facts for different legs — i.e., extend
the existing "several stations/lines listed under one shared date range —
that is still one period" rule (`llm.rs:229-231`) to name its actual
boundary: identical treatment merges, a stated *different* treatment
(different substitute service, different severity, different resolution
wording) does not, even under a shared date range.

**New guidance text** (drafted at the same register/precision as the
existing `PRIMARY_PROMPT`; to be inserted immediately after the existing
"do NOT split for stylistic variation... that is still one period"
sentence at `llm.rs:229-231`, not replacing it):

> "When a shared date range covers multiple named route legs, keep them in
> one period only if every leg is treated identically — same substitute
> service or lack of one, same `apparent_severity`, same
> `resolution_status` — and describe every affected leg together in
> `scope_description`. Split into a separate period per leg (or per
> leg-and-day-of-week combination) whenever the text states a genuinely
> different treatment for one leg or for specific days within the range —
> e.g. one leg gets a rail replacement bus while another has no scheduled
> service at all, or a leg's rule only applies on certain days of the
> week and a different rule applies on the rest. A no-scheduled-service
> statement is never the same fact as a rail-replacement-bus statement,
> even when both fall inside the same date range and even when the text
> presents them as neighboring clauses — do not merge them, and do not let
> the shared date range alone suggest they are one period."

**New worked example** (second worked example, appended after the
existing one at `llm.rs:274`; grounded only in the confirmed quoted
fragments and segmentation table cited above, reference date
2026-08-01T00:00:00Z so "29 Aug" et al. resolve unambiguously into the
same year):

> "Second worked example, reference date 2026-08-01T00:00:00Z: input
> 'From Saturday 29 August to Friday 11 September, buses replace trains
> between Barrhead and Kilmarnock / Dumfries. Monday to Saturday during
> this period, buses operate between Kilmarnock and Troon, where
> passengers can connect with trains to / from Ayr. No scheduled services
> operate between Kilmarnock and Ayr / Stranraer on Sundays.' segments
> into exactly three periods, all sharing the same overall date range but
> none merged into one, because each names a different leg and/or a
> different treatment: period 1 — `scope_description` 'buses replace
> trains, Barrhead to Kilmarnock / Dumfries', `date_range`
> `{"from_date": "2026-08-29T00:00:00Z", "to_date": "2026-09-12T00:00:00Z"}`,
> `schedule_window: null` (applies every day of the range), `apparent_
> severity: "severe_disruption"`; period 2 — `scope_description` 'buses
> operate Kilmarnock to Troon, connecting to Ayr trains', same
> `date_range`, `schedule_window` `{"days_of_week": [1,2,3,4,5,6],
> "start_time": "00:00", "end_time": "23:59"}` (Monday–Saturday only),
> `apparent_severity: "severe_disruption"`; period 3 — `scope_description`
> 'no scheduled service, Kilmarnock to Ayr / Stranraer', same
> `date_range`, `schedule_window` `{"days_of_week": [7], "start_time":
> "00:00", "end_time": "23:59"}` (Sunday only), `apparent_severity:
> "blocked_or_suspended"` (a full withdrawal is more severe than a bus
> substitute, not the same fact restated). Note periods 2 and 3 are NOT
> merged despite sharing both the date range and the same underlying
> Kilmarnock–Ayr/Stranraer leg — the text states two different treatments
> for different days, which is exactly the case that must still split even
> though 'several things under one shared date range' would otherwise
> argue for merging."

This worked example is deliberately silent on Example 1's second date
range (12–13 Sep) and on Example 2 (Norwood Junction) — the prompt already
has two worked examples after this addition, and a third risks diluting
rather than clarifying; both remaining texts are used for the eval fixture
below instead, where a soft, non-blocking signal is the right rigor level
for material this session cannot live-run against a model.

**New eval fixture(s)**, added to the existing battery
(`llm.rs:896-1081`) following its established conventions — plain
`const &str` summary/description pairs, run through `run_battery_attempt`
inside `live_eval_battery`, plus one dedicated soft-assertion test
mirroring `live_eval_wandsworth_town_segments_into_two_periods`
(`llm.rs:1000-1044`)'s exact shape (`NOTE:` log line on mismatch, not a
hard failure — this is a segmentation-reliability risk, not a wire-format
contract):

- `BARRHEAD_DUMFRIES_SUMMARY`/`_DESCRIPTION`: the same text as the worked
  example above, extended with the second, 12–13 Sep date range in the
  same "similar structure, Carlisle instead of Dumfries, Saturday-only bus
  leg, Sunday no-service" shape the sibling research doc's table describes
  (lines 325-330) — this is the fixture that actually exercises **two**
  date ranges each containing the day-of-week-across-legs shape, which the
  single-date-range worked example above does not by itself prove the
  model generalizes to. Labeled `"dow_legs"` in the battery loop.
  Soft-assertion test `live_eval_barrhead_dumfries_segments_into_six_
  periods`, expecting 6 periods (3 per date range, per the reasoning
  above) but logging rather than failing on a different count, exactly
  matching the existing Wandsworth Town test's "soft signal, not a hard
  assertion" posture (`llm.rs:1034-1043`) — the exact right count is a
  judgment call this design pass cannot verify against a live model, and
  should not be hard-pinned before one runs.
- `NORWOOD_JUNCTION_SUMMARY`/`_DESCRIPTION`: built from the confirmed
  fragments — the Monday–Thursday overnight bus-replacement clause plus
  the separate, undated "some trains will be diverted via alternative
  route" clause. Labeled `"undated_aside"` in the battery loop, run
  without a dedicated hard-count expectation (the sibling research doc
  itself flags this as "genuinely ambiguous whether the existing
  segmentation rules would give it its own period... or fold it into the
  same period's `scope_description`" — an open question that doc
  explicitly left unresolved, not something this design pass should
  silently resolve by inventing a specific expected count). This fixture's
  purpose is observational: log what the improved prompt actually does
  with an undated aside, as input to a future pass, not to assert a
  specific right answer here.

### Decision 2 (Axis 2) — Process for choosing the new `MAX_PERIODS`, not a number

Sequenced strictly after Decision 1 lands and its eval fixtures have been
run against a live model at least once, per the research's own ordering
(`failures-research.md`, Recommendation 2: "raise `MAX_PERIODS`, but only
after (1)... sized against real eval data"). The process:

1. **Run the extended `live_eval_battery`** (existing three fixtures +
   the two new ones from Decision 1) against the deployed model, at the
   existing `LIVE_EVAL_REPEATS` (default 3, `llm.rs:985`) repeat count,
   and record each attempt's `period_count` from the battery's own
   greppable log line (`llm.rs:959-963`).
2. **If historically-failing incident text is retrievable by then**
   (unavailable to this and the prior research pass — the local dev DB's
   `incidents` table had zero rows, per the research doc's own Method
   section — but may become available via production log/DB access this
   design pass also does not have), run the same battery against that
   real sample instead of/in addition to synthetic fixtures. Real
   incidents are strictly better evidence than modeled worst cases; this
   step is conditional only because retrievability wasn't established in
   either pass so far, not because synthetic fixtures are preferred.
3. **Take the resulting period-count distribution** (across repeats,
   across whichever real/synthetic fixtures exercise the day-of-week-
   across-legs shape specifically — the `dow_legs` fixture above, plus any
   real incidents of similar shape from step 2) and set `MAX_PERIODS` at a
   measured high percentile with explicit headroom, not the observed
   maximum: the same "generous headroom over every motivating example"
   principle the original `295e478` commit already used when it set 8
   against a 2-period and a "3+"-period fixture (`llm.rs:172-178`), just
   re-run against harder fixtures than were available at that time. A
   concrete target: headroom of at least +50% over the highest observed
   count for the hardest fixture, rounded up, mirroring how 8 already sat
   4x the 2-period Wandsworth Town fixture. (Not committing to a specific
   number now — that is exactly the "blind bump" the research warned
   against, `failures-research.md` Recommendation 2.)
4. **Re-run the full battery once more at the chosen value** to confirm
   nothing in the *existing* three fixtures regresses (still 2/1/1 periods
   respectively) — Decision 1's prompt changes touch shared guidance text
   that could in principle affect segmentation on unrelated fixtures, so
   this is a regression check, not just a sizing exercise.
5. **Record the chosen value's justification in `MAX_PERIODS`'s own doc
   comment** (`llm.rs:168-178`), the same place its current "3+"-fixture
   justification already lives — replacing that stale justification with
   the new one, not leaving both.

This whole axis is explicitly a follow-on implementation step, not
designed further here — see Explicitly out of scope.

### Decision 3 (Axis 3) — Truncate to the N most-severe/soonest periods; visibility via a new counter + a warn-level log, not a new tracker

**Chosen: when a primary extraction still exceeds `MAX_PERIODS` after
Decisions 1/2 land, keep the `MAX_PERIODS` periods ranked highest by
`(severity_rank(apparent_severity) descending, date_range.from_date
ascending with `None` sorting first)`, drop the rest, and let extraction
succeed with the truncated set** — replacing today's `anyhow::bail!` at
`llm.rs:444`. **Rejected: flatten-to-single-period.**

**Selection rule, and why severity is the primary key.** `apparent_severity`
is what `apply_extraction`'s escalation path (`aggregation.rs:568-588`)
and demotion floors (`aggregation.rs:606-625`) are built to surface as the
single most consequential thing a period carries — this pipeline's whole
adversarial-pass machinery exists specifically to avoid *under*-reporting
severity (`SEVERITY_ADVERSARIAL_PROMPT`, `llm.rs:345-354`, exists to argue
the mildest *defensible* reading, i.e. the primary pass's own severity
claim is the one being protected from over-trust, not under-trust). Given
a forced choice about which periods to keep, keeping the ones the pipeline
itself already treats as most consequential is the direction consistent
with that posture — the same "when in doubt, choose the safer answer"
shape as the prompt's own resolution-status rule (`llm.rs:261-263`: "When
in doubt about `resolution_status`, choose `ongoing`"), applied to period
selection instead of status inference: **dropping the periods least
likely to change the incident's displayed severity is the safe-direction
choice; dropping severe or soon-relevant ones to keep milder or
later-dated ones would not be.** Date is the tiebreak, not the primary
key, because two periods of equal severity differ in urgency by how soon
they're relevant, and `None` (no stated start) sorts first because
`DateRange.from_date: None` already means "treat it as already active"
(`llm.rs:18-19`) — the most urgent reading, matching that field's existing
convention rather than inventing a new one for this selection rule.

**Where the cut happens**: inside `extract_primary`, at the exact point
the current `if extraction.periods.len() > MAX_PERIODS` check sits
(`llm.rs:440-445`) — sort-and-truncate `extraction.periods` there instead
of `bail!`ing, then continue returning `Ok(extraction)` as normal. This
keeps every downstream step (`extract_adversarial`,
`extract_severity_adversarial`, `combine::combine_periods`,
`write_extraction`) completely unaware anything unusual happened — they
already operate on "whatever's in `primary.periods`" today, so a
pre-truncated, in-bounds list needs no changes to any of them. No schema
change (consistent with Decision 1).

**Why not flatten to a single period instead.** A single-flattened-period
fallback discards *all* period-level granularity — every `scope_description`,
every leg-specific `schedule_window`, every per-period `resolution_status`
— even for the majority of an incident's periods that were never the
problem (a 13-period incident against even today's cap of 8 is 5 periods
over, not an unbounded blowout the research itself notes, `failures-
research.md`: "reaching exactly 13... is more consistent with... real
compound structure" than runaway hallucination). Truncating preserves
`apply_extraction`'s full per-period active/elapsed/schedule-window logic
for whichever periods survive selection, which is strictly more signal
than one flattened summary line for the same incident, at no extra
implementation cost over flattening (both are "pick the source data,
proceed" changes at the same call site).

**Why no synthetic "N periods omitted" marker period.** Considered:
appending one synthetic trailing `ExtractionPeriod` (e.g.
`scope_description: "N further reported periods not shown (cap reached)"`,
`resolution_status: "ongoing"`, `date_range: None`) so the omission is
visible in the stored `extracted_periods` JSON and, ideally, in the
API-facing `reason` text. **Rejected for the `reason`-text goal
specifically**: `apply_extraction`'s `Active`-period match arm has no
annotation-producing case for `"ongoing"` (`aggregation.rs:606-616`, the
`_ => {}` arm at line 615) — a synthetic `"ongoing"` period would silently
produce *zero* visible effect in `reason` text, the opposite of the goal,
while adding a period that consumes one of the `MAX_PERIODS` slots for
nothing. Surfacing truncation in end-user-facing `reason` text would need
a real `apply_extraction` special case (recognizing a reserved marker and
emitting an annotation regardless of `resolution_status`) — a genuine,
larger change than this axis needs to solve the concrete problem in scope
(retry-forever + total signal loss), and is explicitly deferred (see
Explicitly out of scope). Operator-facing visibility (below) is designed
in full instead, since that is what actually closes the "silently retries
forever with no visible degradation signal to anyone" problem the
research identified (`failures-research.md`, "The downstream data-quality
consequence").

**Why not a distinct `extraction_model_version` marker.** The research's
own recommendation text floats this as one option ("a distinct
`extraction_model_version` marker (or an annotation flagging
truncation)"). **Rejected outright**: `sweep::incidents_needing_
extraction` (`sweep.rs:28-37`) compares a row's stored
`extraction_model_version` against one literal `current_model_version`
string, the same `model_version` value computed once in `main`
(`main.rs:75`) and threaded unchanged into every `process_incident` call.
Writing any per-row *variant* of that string for a truncated incident
(e.g. `"{model}@periods-v1+truncated"`) would make that row permanently
mismatch the sweep's single comparison value on every future sweep tick —
literally reintroducing the exact retry-forever pattern this axis exists
to close, just for a different reason. This is a hazard specific to how
the sweep's comparison is implemented today, not a hypothetical; it is why
this decision does not use that option.

**Operator-facing visibility — new counter + a log, modeled directly on
existing patterns, no new tracker/tooling.** `process_incident`
(`main.rs:252-347`) already owns all logging/metrics for this function
(`record_llm_call_metrics`, `main.rs:221-233`, called after every one of
the three LLM calls). Mirror that:

- `PrimaryExtraction` gains a `#[serde(default)] pub dropped_period_count:
  usize` field — populated by `extract_primary` after truncation, never
  sent by the model, following the exact precedent already established for
  `ExtractionPeriod.resolution_status_confidence`/`severity_confidence`
  (`llm.rs:76-79`: "primary pass's response never sends these two fields,
  so deserializing it straight into `ExtractionPeriod` would otherwise
  hard-fail... `#[serde(default)]` is load-bearing").
- In `process_incident`, immediately after the existing `primary_result`
  match (`main.rs:285-291`), check `primary.dropped_period_count > 0` and,
  if so: `tracing::warn!(incident_id, original_count = primary.periods.len()
  + primary.dropped_period_count, kept_count = primary.periods.len(),
  "primary extraction exceeded the period cap; truncated to the N most
  severe/soonest periods")`, and increment a new counter,
  `enricher_period_truncations_total` (no per-incident label, same
  cardinality discipline `record_llm_call_metrics`'s own doc comment
  already states for `call`/`outcome`, `main.rs:216-220`) — a `counter!`,
  not a `gauge!` like `enricher_mismatch_incidents`, because unlike a
  `CombineError` mismatch (which can recur indefinitely for the same
  incident until its text changes or the prompt is fixed,
  `main.rs:138-149`), a truncated extraction *succeeds* and writes
  normally (`write_extraction` still runs, `source_text_hash`/
  `extraction_model_version` both advance) — there is no "currently
  outstanding" set of truncated incidents to gauge, only a rate of
  one-shot events to count. `tracing::warn!`, not `tracing::error!`,
  deliberately distinguishing this from `MismatchTracker`'s
  `consecutive > 1` `tracing::error!` case (`main.rs:320-336`): a
  truncation is bounded, partial data loss on an otherwise-successful
  pipeline run, not the indefinite total failure a persistent mismatch
  represents — using the same log level for both would blur a real
  severity difference this codebase's own two existing tracked-failure
  patterns (mismatch vs. plain LLM-call error) already keep apart.
- The counter is what an alert rule would fire on (e.g. a sustained
  nonzero rate over some window) — exactly how `enricher_mismatch_
  incidents` is already described as feeding "operational visibility"
  (`main.rs:141` doc comment) without this codebase needing any new
  dashboard/tooling; the log line is human debugging context alongside it,
  the same split `MismatchTracker` already uses (gauge for the alertable
  signal, `tracing::error!` for the human-readable why).

### Architecture — failure-mode flow

```
LlmClient::extract_primary (llm.rs)
  |
  v
JSON parsed -> extraction.periods
  |
  |-- periods.is_empty() ---------------------------> Err (unchanged: hard parse failure)
  |
  |-- periods.len() <= MAX_PERIODS ------------------> Ok(PrimaryExtraction { dropped_period_count: 0, .. })
  |                                                     (unchanged path)
  '-- periods.len() > MAX_PERIODS  [CHANGED]
        |
        v
      sort by (severity_rank(apparent_severity) desc,
               date_range.from_date asc, None-first)
        |
        v
      keep first MAX_PERIODS, drop the rest
        |
        v
      Ok(PrimaryExtraction {
        periods: <= MAX_PERIODS,
        dropped_period_count: original_len - MAX_PERIODS,
      })

process_incident (main.rs), after primary_result match:
        |
        v
      dropped_period_count > 0 ?
        |                    \
        no                    yes
        |                      |
        v                      v
   (unchanged)          tracing::warn!(incident_id, original_count, kept_count, ...)
                         enricher_period_truncations_total.increment(1)
                              |
                              v
                    pipeline continues UNCHANGED:
                    extract_adversarial -> extract_severity_adversarial
                    -> combine::combine_periods -> queries::write_extraction
                              |
                              v
                    source_text_hash / extraction_model_version BOTH advance
                    -> sweep::incidents_needing_extraction no longer re-selects
                       this incident (unless its text later changes)
                    -> stream entry IS acked (main.rs:101-104) -- never
                       reaches reclaim_loop's stale-entry path at all
                    -> retry-forever loop (research doc, "What happens when
                       the cap is hit") is closed for this incident
```

## Error handling

- The empty-`periods` hard failure (`llm.rs:431-439`) is untouched by this
  design — it remains a genuine parse failure with nothing to
  truncate/select from, not a truncation case.
- A truncation event never returns an `Err` from `extract_primary` — this
  is the entire point of the change (Decision 3). `process_incident`'s
  existing `Err` branch (`main.rs:287-290`, `return false`) is therefore
  never reached for a cap breach after this change; it remains reachable
  for every other kind of primary-extraction failure (malformed JSON,
  network/timeout error, empty periods).
- `combine::combine_periods`'s existing length/ordinal-alignment checks
  (`combine.rs:112-152`) are unaffected: they operate on whatever
  `primary.periods` contains post-truncation, at whatever length that is
  (always `<= MAX_PERIODS`), the same as they operate on any other
  in-bounds period list today. No new failure mode is introduced there.
- If `dropped_period_count` is nonzero but `write_extraction`
  (`main.rs:340`) itself then fails (a genuine DB error, unrelated to
  truncation), `process_incident` returns `false` exactly as it does for
  any other write failure today (`main.rs:341-343`) — the truncation
  warning/counter has already fired by that point (it fires right after
  `primary_result`, not after the write succeeds), so an operator sees
  "this incident was truncated" even on a run that ultimately still fails
  to write, which is correct: the truncation happened and is worth
  knowing about regardless of what happens next in that same attempt.

## Testing

Following this crate's existing conventions (`#[tokio::test]` +
`wiremock` for `llm.rs`'s own tests, `#[cfg(test)] mod tests` colocated
per file):

- `llm.rs`: a new test mirroring `extract_primary_rejects_periods_beyond_
  the_soft_cap` (`llm.rs:640-669`) but asserting the new behavior —
  `extract_primary_truncates_periods_beyond_the_soft_cap`: mock response
  with `MAX_PERIODS + 5` periods of varying `apparent_severity` and
  `date_range`, assert the result is `Ok`, `periods.len() == MAX_PERIODS`,
  `dropped_period_count == 5`, and that the kept periods are exactly the
  ones the selection rule predicts (most severe first, soonest
  `from_date` as tiebreak) — not just a length check, since the ordering
  guarantee is the part actually worth pinning.
- `llm.rs`: a test confirming the selection rule's tiebreak specifically —
  two periods at equal `apparent_severity`, different `from_date`
  (including a `None`-vs-`Some` pair, confirming `None` sorts first per
  `DateRange`'s own "already active" convention, `llm.rs:18-19`).
- `llm.rs`: `extract_primary_accepts_periods_exactly_at_the_soft_cap`
  (`llm.rs:671-700`, already existing) continues to pin `dropped_period_
  count == 0` at exactly `MAX_PERIODS` — extend its existing assertions
  rather than adding a new test, since this is the boundary case the new
  field must not fire on.
- `main.rs` has no existing `#[cfg(test)] mod tests` for `process_incident`
  itself (it's exercised only via the crate's integration surface, per
  this session's read of the file) — if one is added as part of
  implementing this, it should assert `tracing::warn!`/the new counter
  fire exactly when `dropped_period_count > 0`, and that `write_extraction`
  is still reached (unlike today's `Err` short-circuit) — a
  behavior-level test, not just a log-line grep.
- The Decision 1 eval fixtures (`BARRHEAD_DUMFRIES_*`, `NORWOOD_JUNCTION_*`)
  are live-eval-only (`#[ignore]`d, per the existing convention), not part
  of ordinary CI — consistent with every other fixture in this battery.

## Explicitly out of scope

- **Implementing any of these three decisions** — this is a design spec,
  no `llm.rs`/`main.rs`/`combine.rs`/`aggregation.rs` edits were made to
  produce it.
- **Choosing the actual new `MAX_PERIODS` value.** Decision 2 designs the
  process only, per the research's own sequencing requirement (fix the
  prompt first, size the cap against what that produces).
- **A schema change to `schedule_window`.** Decision 1 evaluates and
  rejects it for the motivating case; not designed further here. If a
  future incident demonstrates the narrower case it *would* help
  (identical severity/resolution/scope, only day-of-week membership
  differing) driving real cap breaches on its own, that would be new
  evidence worth a fresh look — not assumed here.
- **Surfacing truncation in end-user-facing `reason` text or anywhere in
  the frontend/API.** Decision 3 designs operator-facing visibility only
  (log + counter); a real end-user-visible signal would need an
  `apply_extraction` special case this design deliberately does not spec
  out (see Decision 3's "no synthetic marker period" reasoning). Flagged
  again in Open questions/risks.
- **Retrieving incident `8B79E940727A4170AFA846A0561D77B8`'s actual text**,
  or any other production incident text, to validate any of this
  empirically. Neither this pass nor the prior research pass had
  production DB/log access; the local dev DB's `incidents` table remains
  empty (unchanged since the research pass's own check).
- **Running the live-eval battery against a real model.** No network/LLM
  access in this environment, same constraint the research doc already
  disclosed. Decision 2's process depends on this being run as its first
  step by whoever implements it.
- **Changing `combine::combine_periods`'s length/ordinal-alignment
  behavior, or the adversarial prompts' text**, beyond what Decision 1's
  worked example/guidance addition requires in `PRIMARY_PROMPT` itself —
  the adversarial prompts are unaffected by this remediation.
- **A dead-letter mechanism or delivery-count limit for the Redis Stream
  reclaim loop** (`stream::claim_stale`, `stream.rs:83-111`, still has no
  such limit). The research doc noted its absence; Decision 3 closes the
  retry-forever *outcome* for a cap breach specifically (by making
  extraction succeed instead of failing), which makes this gap moot for
  this particular failure mode, but does not add a general limit that
  would also protect against other kinds of persistent per-incident
  failure (e.g. a genuine, non-cap LLM error that recurs forever for
  unrelated reasons). That remains a separate, broader hardening question,
  not scoped to this remediation.

## Open questions/risks

- **The Decision 1 worked example and eval fixtures have not been run
  against a live model.** Their expected period counts (3 for the single-
  date-range worked example, 6 for the two-date-range eval fixture) are
  this design pass's own reasoned prediction from the source text and the
  new guidance's intent, not an observed result — exactly why the eval
  fixture's own test is designed as a **soft** assertion (mirroring
  `live_eval_wandsworth_town_segments_into_two_periods`'s existing
  posture), not a hard pin. The actual counts, once run, may differ and
  should not be treated as validated until they are.
- **Whether prompt guidance alone is sufficient**, or whether some real
  incidents genuinely need more than whatever `MAX_PERIODS` value Decision
  2's process eventually lands on even after tight grouping, is exactly
  the open question the research doc itself could not resolve without the
  incident's real text (`failures-research.md`, "Weighing the two
  explanations"). Decision 3's truncation path is the safety net for
  whichever incidents this turns out to be true for, at any finite cap —
  this spec does not claim Decision 1 alone eliminates the need for it.
- **The severity-first truncation selection rule assumes `apparent_
  severity` is itself reasonably trustworthy pre-adversarial-verification**
  (truncation happens inside `extract_primary`, before either adversarial
  pass runs, so the ranking uses the primary pass's raw, unverified
  severity claim, not the post-`combine` confidence-gated value). This is
  a deliberate, disclosed tradeoff: verifying first would mean running the
  adversarial passes over periods that might then get discarded anyway
  (wasted calls), and the primary pass's severity claim, while unverified,
  is still the model's own most-informed read of the text — but it means
  a rare case (the primary pass over-claims severity on a period that
  should have been ranked lower) could keep a less-truly-severe period
  over a more-severe one. Not resolved here; flagged as an accepted
  tradeoff given the "avoid unbounded wasted LLM calls" precedent
  `LlmClient`'s own docs already care about (`llm.rs:361-372`,
  `request_timeout` docs on why serial-processing cost matters).
- **No visibility into how often truncation will actually fire** once
  Decision 2's cap is chosen — by design, since `MAX_PERIODS` is sized
  with headroom specifically to make this rare, but "rare" is not
  "never," and this spec has no data (same disclosed gap as the research
  doc) on the real-world rate. The new counter (Decision 3) is what
  would answer this, once deployed — not knowable in advance from this
  design pass alone.
- **The end-user-facing visibility gap** (Decision 3's rejected synthetic-
  marker option) means a truncated incident's displayed `reason` text
  looks identical to an untruncated one even though some of its reported
  periods were silently dropped from storage — an accepted, disclosed
  limitation of this pass's scope, not an oversight; flagged for a
  possible future `apply_extraction` follow-up if this turns out to
  matter in practice (e.g. if the dropped periods were ones a passenger
  would have wanted to see).
