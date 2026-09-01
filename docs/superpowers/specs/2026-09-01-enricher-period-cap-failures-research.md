# Enricher `MAX_PERIODS` Soft-Cap Failures — Root-Cause Research

**Status: research/root-cause investigation only, not an approved design.**
Written to the same rigor as
`docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(this session's structural template: cite what was fetched/read, flag what
couldn't be confirmed, reach a real recommendation rather than a hedge). No
code was edited to produce this document.

## Problem

A real production log line, verbatim:

```
enricher-1  | 2026-08-31T22:27:47.341614Z ERROR enricher: primary extraction failed error=primary extraction returned 13 periods, exceeding the soft cap of 8 incident_id="8B79E940727A4170AFA846A0561D77B8"
```

`crates/enricher`'s primary LLM extraction pass segmented one real incident
into 13 periods, which exceeds a hard-coded soft cap of 8
(`MAX_PERIODS`), and the whole extraction attempt was discarded. The
question this research answers: **is 13 periods evidence of the model
duplicating/over-segmenting text it shouldn't have split, or evidence that
this specific real-world incident genuinely has that much structure, such
that 8 was simply too low a cap?** Both are real possibilities in
principle; this document reasons through the actual code, the actual
prompt, and the actual test-coverage gaps to reach an evidenced answer
rather than restating the question.

## Method

- Read `crates/enricher/src/llm.rs` in full, including `PRIMARY_PROMPT`
  verbatim, the JSON schema, `MAX_PERIODS`'s doc comment, and the full
  `#[cfg(test)]` module (fixtures, live-eval battery, and what it does and
  does not exercise).
- Traced the actual control flow from a primary-extraction failure through
  to its downstream consequences: `crates/enricher/src/main.rs`
  (`process_incident`, `sweep_loop`, `reclaim_loop`), `crates/enricher/src/sweep.rs`
  (`incidents_needing_extraction`), `crates/enricher/src/queries.rs`
  (`write_extraction`, `fetch_incident_state`), `crates/enricher/src/stream.rs`
  (`claim_stale`, `ack`), `crates/enricher/src/config.rs` (the real interval
  defaults), and `crates/aggregator/src/aggregation.rs` (`apply_extraction`,
  `parse_periods`).
- Read the originating design doc,
  `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`
  (§2 prompt redesign, §4 period-phase/combination logic, §7 risks, the
  testing plan), and the commit that introduced the cap
  (`git show 295e478`, "Cap the primary extraction pass at 8 periods per
  incident").
- Checked whether the local dev environment has any incident data that
  could stand in as a sanity check. It does not: a local Postgres instance
  is reachable (`railstatus` database, role `lucy`), but its `incidents`
  table has zero rows (confirmed via `SELECT count(*) FROM incidents;` →
  `0`), so incident `8B79E940727A4170AFA846A0561D77B8`'s actual text is
  not recoverable in this environment. See Open questions/risks.
- No web search or production log/DB access was available or used for this
  incident specifically; the two real complex-incident text examples cited
  below (Barrhead/Kilmarnock/Dumfries engineering-works notice) were
  supplied directly as part of this task's brief, not independently
  fetched by this research pass, and are **not confirmed to be the same
  incident** as `8B79E940727A4170AFA846A0561D77B8` — they are used only as
  a grounded example of the *shape* of text this feed is known to produce.

## Current relevant state (verified control-flow trace)

### The cap itself

`crates/enricher/src/llm.rs:179`: `const MAX_PERIODS: usize = 8;` — a
compile-time constant, not configurable via `crates/enricher/src/config.rs`
(confirmed: `config.rs` has no period-count-related field; `MAX_PERIODS` is
referenced nowhere outside `llm.rs`). It is enforced in Rust, after JSON
parsing, at `llm.rs:440-445`:

```rust
if extraction.periods.len() > MAX_PERIODS {
    anyhow::bail!("primary extraction returned {} periods, exceeding the soft cap of {MAX_PERIODS}", extraction.periods.len());
}
```

Its own doc comment (`llm.rs:168-178`) records the original justification
directly: not enforced via JSON-schema `maxItems` because "a `maxItems`
schema constraint may not be reliably enforced by every backend" (design
§7 item 6), and the value 8 was chosen as "generous headroom over every
motivating example in the design doc (the Wandsworth Town fixture has 2;
the design's own soft-cap sanity-check fixture is described as '3+')."
**A real production incident needing 13 periods is direct empirical
evidence that this headroom assumption did not hold for at least this one
incident.** The commit that introduced the cap (`295e478`, 2026-08-28,
three days before this failure was logged) reused the same "3+" framing
verbatim in its own commit message — the cap was never validated against
anything close to the complexity this failure represents.

### What happens when the cap is hit (traced, not assumed)

`process_incident` (`crates/enricher/src/main.rs:252-347`) calls
`llm.extract_primary(...)` at `main.rs:283`. On `Err` (which is exactly
what a soft-cap breach produces, via the `anyhow::bail!` above), it logs
and returns `false` immediately at `main.rs:288-289`, **before**
`queries::write_extraction` is ever reached (that call is at `main.rs:340`,
downstream of two more LLM calls and `combine::combine_periods` that never
run). `write_extraction` (`crates/enricher/src/queries.rs:57-85`) is the
only place that updates `source_text_hash`, `extracted_category`,
`extracted_periods`, and `extraction_model_version` on the `incidents` row
— so on this failure path, none of those columns change. If this was the
incident's first extraction attempt, they stay whatever they were before
(NULL, per `queries.rs`'s `IncidentState` shape and the `Option<String>`
typing of `source_text_hash`/`extraction_model_version`).

Both of this app's two independent re-trigger mechanisms then re-select
this exact incident indefinitely, because both key off that same
never-updated stored state:

1. **The hourly sweep** (`crates/enricher/src/sweep.rs`,
   `sweep_loop` at `main.rs:188-203`). `incidents_needing_extraction`
   (`sweep.rs:28-37`) re-selects any incident whose current text hash
   doesn't match the *stored* `source_text_hash`, or whose stored
   `extraction_model_version` doesn't match the running version. A failed
   extraction never updates either column, so this incident matches the
   "needs extraction" filter on every single sweep run — by default every
   `sweep_interval_secs` = 3600s (`config.rs:38-44`) — for as long as the
   incident remains un-cleared and its source text is unchanged.
2. **The stream reclaim loop** (`reclaim_loop`, `main.rs:355-399`, using
   `stream::claim_stale`, `crates/enricher/src/stream.rs:83-104`, an
   `XAUTOCLAIM` scan). `process_incident` returning `false` means the
   stream entry is never acked (`stream::ack` is only called on the `true`
   branch, `main.rs:384-388`), so it stays in the consumer group's
   pending-entries list. `claim_stale` re-claims *any* entry idle longer
   than `reclaim_min_idle_secs` (default 1000s ≈ 16.7 min,
   `config.rs:53-61`), checked every `reclaim_interval_secs` (default 60s,
   `config.rs:46-51`), with **no retry-count limit or dead-letter
   mechanism** — `claim_stale`'s `XAUTOCLAIM` scan (`stream.rs:83-104`)
   claims purely on idle time, never inspecting Redis's own per-entry
   delivery count. Confirmed by reading the full function: nothing bounds
   how many times one entry can be reclaimed.

**Conclusion, verified rather than assumed**: assuming the LLM's
segmentation behavior for this exact text is reasonably repeatable at
`temperature: 0.0` (set at `llm.rs:396`, the same for every call this
client makes), this incident is retried by *both* mechanisms indefinitely
— roughly every ~1000s+ via reclaim, and again every sweep interval via
the sweep's separate direct call to `process_incident` — and fails
identically every time, for as long as the incident's summary/description
text is unchanged. This is a real, ongoing operational cost: repeated
wasted LLM calls (three per attempt: primary is the only one that runs
here, since it fails before the two adversarial calls even start) forever,
for one incident, until either its text changes upstream or `MAX_PERIODS`
changes.

### The downstream data-quality consequence

`crates/aggregator/src/aggregation.rs`'s `apply_extraction`
(`aggregation.rs:553-`) is the sole consumer of `extracted_periods`. It
runs `parse_periods` (`aggregation.rs:402-414`) first, which is explicitly
fail-safe: a missing `extracted_periods` column, or any deserialization
failure, degrades identically to "no periods at all" (`unwrap_or_default()`
at `aggregation.rs:412-413`). The test
`apply_extraction_is_a_no_op_with_no_extraction`
(`aggregation.rs:1830-1841`) pins this directly: with
`extracted_periods: None`, `apply_extraction` returns the base severity
unchanged and `annotation: None`. So the concrete cost of this incident's
permanently-failed extraction is that it **permanently forfeits every
NLP-derived signal `apply_extraction` would otherwise apply**: no
resolved/residual severity-floor demotion, no schedule-window-excludes-now
demotion, no elapsed-period demotion, and no severity-hint escalation
(`escalation_ceiling`, `aggregation.rs:496-503`) — the incident's displayed
severity and `reason` text rest entirely on whatever `severity_from_incident`
(the pre-NLP classifier) produces from raw feed fields, with none of the
text-derived refinement this whole pipeline exists to add. This is a
silent, indefinite degradation, not a visible error to end users — nothing
in the aggregator or API surfaces "this incident's extraction has been
failing since it was first seen."

## Root-cause analysis: over-segmentation vs. genuine complexity

### The prompt's actual anti-over-segmentation guidance

`PRIMARY_PROMPT` (`llm.rs:224-274`, read in full) says, in relevant part:

> "Only split into more than one period where the text itself demarcates a
> distinct date range and/or a distinct scope/impact... Err toward fewer
> periods when in doubt: do NOT split for stylistic variation, repeated
> wording, or several stations/lines listed under one shared date range --
> that is still one period."

This guidance, and the design doc's own live-eval battery
(`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`,
§"Risks and open questions" item 1, lines ~697-716), were validated
specifically against: (a) the Wandsworth Town fixture — **two** distinct
date ranges, each with its own scope/direction and its own nested
`schedule_window` (same Mon-Thu 11:00-14:00 shape in both, different
platforms/direction) — correctly segmenting into 2 periods across 3 live
runs; (b) a flat single-fact ETA fixture staying at 1 period; and (c) an
explicit "over-segmentation trap" — three stations sharing one date range,
which must stay one period — correctly resisted across 2 live runs, on
`gemma3:12b` specifically (7/7 correct). **Neither the prompt's own
worked example (a clean two-phase, one-`schedule_window`-each case) nor
the live-eval battery's three fixtures ever construct a case with more
than one `schedule_window`-scoped sub-rule sharing a single date range
across multiple named route legs.**

### The schema-forced multiplication the guidance doesn't cover

`ExtractionPeriod` (`llm.rs:54-80`) has exactly one `schedule_window: Option<ScheduleWindow>` field — a single nested weekly time-of-day
restriction per period, not an array (confirmed by the schema at
`llm.rs:204-212`: `schedule_window` is `["object", "null"]`, never an
array). This is a **structural** constraint, not a prompt-wording choice:
if one date range genuinely needs two *different* day-of-week rules — say,
because the disruption affects two different route legs on different
days — the model cannot represent that inside a single period no matter
how conservatively it reads the "err toward fewer periods" instruction. It
is schema-forced to emit one period per (date range × distinct
day-of-week-rule) combination, because that's the only way to attach a
distinct `schedule_window` to each rule.

The example text given in this task's brief (two real, though not
confirmed-identical, disruption notices covering the Barrhead/
Kilmarnock/Dumfries area) is exactly this shape: multiple distinct date
ranges, multiple named route legs per date range, and day-of-week-scoped
sub-rules that differ *within* a shared date range across those legs. The
prompt's existing guidance explicitly covers "several stations/lines
listed under one shared date range" (→ stays one period) but says nothing
about "a shared date range with a day-of-week rule that differs by leg" (→
schema-forced split). These are genuinely different cases: the first is a
list of things affected identically; the second is a list of things
affected *differently*, which the schema's single-`schedule_window`-per-
period shape cannot flatten into one period even under a maximally
conservative reading of the current prompt. A realistic incident combining,
say, 2-3 date ranges × 2-3 route legs × a couple of day-of-week variants
per leg plausibly produces well over 8 periods through entirely correct,
schema-compliant segmentation — no hallucination or duplication required.

### Weighing the two explanations

Both mechanisms are real, and there is no access to incident
`8B79E940727A4170AFA846A0561D77B8`'s actual text to determine which one
(or what mix) actually fired for this specific 13-period case — that is a
genuine, disclosed limitation (see Open questions/risks). But reasoning
from what's verifiable in the code:

- **Pure hallucinatory over-segmentation** (the model inventing
  distinctions the text doesn't support, or splitting on "stylistic
  variation") is exactly what the existing prompt guidance and live-eval
  battery were built to catch, and did catch, on the fixtures tested. It
  remains *possible* this incident is a case the battery's narrower
  fixture set simply didn't anticipate, but there's no positive evidence
  for it beyond the possibility itself.
- **Schema-forced multiplication from day-of-week-scoped sub-rules across
  multiple route legs within a shared date range** is a *structural* gap:
  demonstrable directly from the schema (`schedule_window` is
  singular, not an array) and from what the test suite does and doesn't
  cover (no fixture combines >1 date range with >1 `schedule_window`
  variant per date range). This doesn't require assuming the model
  misbehaved at all — a perfectly correct, conservative segmentation of a
  legitimately compound incident can still produce many periods under the
  current schema.

**Conclusion: most likely both, but the second is the better-evidenced
driver of this specific failure mode, and is the one worth fixing first.**
A single incident reaching exactly 13 (not 20, not 50) is more consistent
with "real compound structure hit a schema-forced multiplier" than with
unbounded hallucination, which would have no natural ceiling anywhere near
13. Real incidents affecting multiple route legs across the timeframe
described in the brief's example plausibly *do* need double-digit periods
under the current one-`schedule_window`-per-period schema, which makes
"genuine complexity" a real, evidenced contributor — but the *prompt*
also has a concrete, nameable gap (no guidance or worked example for the
day-of-week-within-shared-date-range-across-legs case) that current
tooling has never exercised, which makes "the guidance doesn't fully cover
this shape yet" an equally real, evidenced contributor. Treating this as
one-or-the-other would be less accurate than treating it as two
compounding causes.

## Recommendation

Three genuinely separate axes, ranked:

1. **Highest priority — improve prompt guidance and worked-example
   coverage for the day-of-week-within-shared-date-range-across-multiple-
   legs case.** This is the most concretely evidenced gap: the current
   `PRIMARY_PROMPT` worked example and the live-eval battery's fixtures
   never exercise it, and the schema genuinely cannot flatten it into
   fewer periods than one per (date range × distinct schedule_window) pair
   without new guidance about how to *group* what can be grouped (e.g.
   whether multiple route legs sharing the identical day-of-week rule
   within one date range should stay one period with a broader
   `scope_description`, rather than splitting per leg needlessly). This
   doesn't require a schema change — it's a prompt-engineering exercise:
   add a worked example modeled on the real complexity this production
   incident represents, and extend the live-eval battery
   (`llm.rs`'s `live_eval_battery`/fixtures) with a fixture of this shape.
   This addresses root cause if — as reasoned above — some of the 13
   periods really were avoidable duplication the model produced without
   being told how to avoid it; it does not, by itself, help if some
   incidents genuinely need double-digit periods even after maximally
   tight grouping.
2. **Second priority — raise `MAX_PERIODS`, but only after (1), and
   informed by what (1)'s battery run against a fixture of this shape
   actually produces.** Given the structural (schema-forced) argument
   above, some real incidents plausibly need more than 8 periods even
   under ideal segmentation. A blind raise now (to, say, 15-20) without
   first tightening the prompt risks papering over genuine
   over-segmentation the model could have avoided, and directly costs
   output readability: `apply_extraction`'s per-period annotations are
   semicolon-joined into one `reason` string
   (`aggregation.rs:635,654`, `.join("; ")`), so a 13-plus-period incident
   already means a `reason` string built from that many joined fragments
   — raising the cap without tightening guidance risks normalizing that
   as routine rather than exceptional. Do this second, sized against real
   eval data from (1)'s new fixture, not as a standalone bump.
3. **Independent axis — change the failure mode so a cap breach doesn't
   retry forever and lose all NLP signal.** Regardless of where the cap
   ends up, the current behavior (hard `bail!` → permanent retry loop,
   traced above) is a real, separate problem: every incident that hits
   the cap, for any reason, gets zero extraction benefit indefinitely and
   burns LLM calls on every sweep and reclaim cycle forever. A graceful
   degradation — e.g. truncate to the N most severe/soonest periods with
   a logged warning and a distinct `extraction_model_version` marker (or
   an annotation flagging truncation), or fall back to a single flattened
   period — would at minimum stop the "fails identically forever" cost and
   give the incident *some* NLP-derived signal instead of none, even when
   the underlying segmentation isn't perfect. This is complementary to,
   not a substitute for, fixing (1)/(2): it bounds the blast radius of
   whatever cap value is chosen, for whichever incidents still exceed it
   (there will likely always be some, at any finite cap).

None of these three should be treated as sufficient alone: the prompt fix
addresses the part of this failure that's plausibly the model's fault, the
cap-value question addresses the part that's plausibly genuine incident
complexity, and the failure-mode change addresses the operational cost
that exists independent of which of the first two is "more true."

## Explicitly out of scope

- Implementing any of the three recommendations above (this document is
  research only, per its own constraints).
- Determining the actual root cause for incident
  `8B79E940727A4170AFA846A0561D77B8` specifically — its text was not
  accessible in this environment (see below).
- Redesigning `ScheduleWindow` into an array-per-period shape (a schema
  change) — mentioned above only to explain *why* the schema forces
  multiplication in the day-of-week-across-legs case, not evaluated here
  as a remediation option; adding a worked example/prompt guidance
  (Recommendation 1) can address the segmentation-choice problem without
  touching the schema.
- Evaluating alternative LLM models/backends — this document did not
  re-run the design doc's live-eval battery (no network/LLM access in this
  environment) and takes the existing `gemma3:12b` battery results as
  given, not as something to re-litigate.
- Any change to `combine::combine_periods`, the adversarial passes, or the
  mismatch-tracker logic — none of those ever ran for this specific
  failure (it fails before they're reached) and are out of scope for this
  investigation.

## Open questions/risks

- **The actual content of incident `8B79E940727A4170AFA846A0561D77B8` was
  not available.** There is no live production DB or API access in this
  environment, and the local dev Postgres database (`railstatus`,
  confirmed reachable) has zero rows in its `incidents` table — this
  specific historical incident could not be recovered locally either. The
  root-cause conclusion above is reasoned from the prompt text, the
  schema, and a *plausibly-related but not confirmed-identical* real-world
  example given in this task's brief, not from the actual failing text.
  Confidence in "which of the two mechanisms actually produced these
  particular 13 periods" is therefore necessarily lower than confidence in
  "both mechanisms are real and both are evidenced from the code" — the
  former is inference from analogous examples, the latter is directly
  verified.
- **Whether the LLM backend's output is actually deterministic enough for
  the "retries forever, fails identically" conclusion to hold exactly.**
  `temperature: 0.0` (`llm.rs:396`) is set for every call, which makes
  exact repetition likely but not architecturally guaranteed (backend-
  dependent sampling implementations, model updates, or non-determinism in
  some inference servers even at temperature 0 could mean the incident
  eventually produces ≤8 periods on a later retry and succeeds). The
  operational-cost claim should be read as "very likely to repeat many
  times," not as a mathematical certainty. It also relies on the upstream
  incident's summary/description text staying byte-identical between
  retries — a real feed update (even a minor wording change from National
  Rail) would change `text_hash` and could change the outcome either way.
- **No visibility into how common this failure is beyond this one log
  line.** This document investigates one incident's cap breach; whether
  this is a rare edge case or a recurring pattern across many incidents of
  similar shape (multi-leg, multi-date-range engineering works, which are
  a known National Rail incident category) is not something this pass can
  quantify without either production log access or the live-eval battery
  extension recommended above.
- **The "two real example texts" cited in the root-cause analysis came
  from this task's brief, not from this research pass's own fetch/search.**
  They are used only to ground the *shape* of complexity this feed is
  known to produce (multi-date-range, multi-leg, day-of-week-scoped), per
  this document's own no-invented-details discipline — they are explicitly
  not claimed to be incident `8B79E940727A4170AFA846A0561D77B8` itself.

## References

- `crates/enricher/src/llm.rs` — `MAX_PERIODS` (line 179), the cap
  enforcement (lines 440-445), `PRIMARY_PROMPT` (lines 224-274),
  `ExtractionPeriod`/`ScheduleWindow` (lines 42-80), primary JSON schema
  (lines 185-222), live-eval fixtures and battery (lines 896-1081).
- `crates/enricher/src/main.rs` — `process_incident` (lines 252-347,
  especially 282-291 and 340), `sweep_loop` (lines 188-203),
  `reclaim_loop` (lines 355-399).
- `crates/enricher/src/sweep.rs` — `incidents_needing_extraction`
  (lines 28-37).
- `crates/enricher/src/queries.rs` — `write_extraction` (lines 57-85),
  `fetch_incident_state`/`IncidentState` (lines 12-46).
- `crates/enricher/src/stream.rs` — `claim_stale` (lines 83-104), `ack`
  (lines 65-68).
- `crates/enricher/src/config.rs` — `sweep_interval_secs`,
  `reclaim_interval_secs`, `reclaim_min_idle_secs` defaults (lines 38-61).
- `crates/aggregator/src/aggregation.rs` — `apply_extraction`
  (lines 553-), `parse_periods` (lines 402-414), the
  `apply_extraction_is_a_no_op_with_no_extraction` test
  (lines 1830-1841), annotation joining (lines 635, 654).
- `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md` —
  §2 prompt redesign (lines ~323-335), §"Risks and open questions" item 1
  and the live-eval battery description (lines ~697-716), item 6 (the
  soft-cap decision, lines ~781-790).
- `git show 295e478` — "Cap the primary extraction pass at 8 periods per
  incident" (2026-08-28), the commit that introduced `MAX_PERIODS`.
- Local dev database check: `psql -h localhost -U lucy -d railstatus -c
  "select count(*) from incidents;"` → `0` rows (confirms no local
  incident data was available as a sanity check).
