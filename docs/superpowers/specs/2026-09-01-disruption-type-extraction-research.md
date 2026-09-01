# Disruption-Type Extraction (Rail Replacement Bus & Siblings) — Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(structural template: Problem, Method, findings with `file:line` citations,
a ranked Recommendation, explicit out-of-scope and open-questions
sections). Nothing here has gone through implementation planning; no code,
schema, or prompt in this repo was touched while writing it.

## Problem

This app's `enricher` crate already runs a working three-pass LLM
extraction pipeline over every Knowledgebase incident's `summary` +
`description` text, producing a structured `resolution_status` and
`apparent_severity` per segmented time period. It does not currently
extract *what kind* of disruption a period represents beyond that severity
read — in particular, whether it is a **rail replacement bus service**, as
opposed to a generic delay, a plain "no scheduled service," or a diversion.
That distinction is operationally significant to a passenger (a bus
replacing a train is a very different journey than "expect delays") and,
per this research, is not surfaced as a distinct fact anywhere in this
app today.

The user supplied two real Knowledgebase incident texts as concrete
grounding (reproduced in full in the task; not re-pasted here to keep this
document shorter, referenced below as **Example 1**, the Barrhead–Dumfries
multi-period notice, and **Example 2**, the Norwood Junction overnight
notice). Both texts mix more than one impact type across periods/legs/days
within a single incident — a design constraint this document treats as
central, not an edge case.

## Method

Direct inspection of the relevant source this session, in the order the
task specified: `crates/enricher/src/llm.rs` (in full), `queries.rs` in
both `enricher` and `aggregator`, the migration history under
`crates/api/migrations/`, `crates/aggregator/src/aggregation.rs`,
`crates/api/src/render.rs`, `crates/common/src/lib.rs`, the two design docs
the enricher's own comments cite
(`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`,
`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`), the
detail-page design doc's scoping decisions
(`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`), and
the frontend surfaces (`frontend/lib/types.ts`,
`frontend/components/DisruptionDetail.tsx`,
`frontend/components/IssueList.tsx`, `frontend/lib/validity.ts`). Every
factual claim below is grounded in one of these reads and cited
`file:line`; no field name, column, or prompt behavior is invented.

## Current relevant state (verified)

### 1. The primary extraction pass, and a real, confirmed gap in it

`PrimaryExtraction` (`crates/enricher/src/llm.rs:88-92`) has a `category:
String` field, `required` in `primary_schema()`
(`crates/enricher/src/llm.rs:189, 220`: `"category": { "type": "string" }`,
`"required": ["category", "periods"]`). `PRIMARY_PROMPT`
(`crates/enricher/src/llm.rs:224-274`) is a single ~50-line constant that
spells out, in real prose detail, how to segment `periods`, and the exact
rules for `resolution_status`, `apparent_severity`, `date_range`,
`schedule_window`, including a full worked example with resolved
timestamps. **It never once mentions `category`** — no enum, no worked
value, no instruction on what the model should put there. Confirmed by
reading the entire constant, not by sampling. The only place `category`
values appear anywhere in this crate are the test fixtures
(`crates/enricher/src/llm.rs:524, 568`: `"signal_failure"`,
`"engineering_works"`) — ad-hoc strings a test author picked, not values
the model is ever told to choose from. This is a real, pre-existing gap:
the schema demands a field the prompt gives no guidance on.

### 2. `category` is persisted, but genuinely never read back by anything downstream

`write_extraction` (`crates/enricher/src/queries.rs:57-85`) binds
`category` straight into `incidents.extracted_category`
(`crates/enricher/src/queries.rs:70`). That column exists —
`crates/api/migrations/20260820120000_incident_extraction.sql:11`:
`ADD COLUMN extracted_category TEXT`.

The aggregator's read side, `load_incidents`
(`crates/aggregator/src/queries.rs:31-40`), issues:

```sql
SELECT incident_id, summary, description, operators, affected_stations,
       priority, validity_periods, is_planned, is_cleared, first_seen_at,
       extracted_periods
FROM incidents WHERE NOT is_cleared
```

`extracted_category` is not in that column list. `LoadedIncident`
(`crates/aggregator/src/queries.rs:18-29`) has no field for it at all —
only `extracted_periods`. A repo-wide grep for `extracted_category`
(outside the enricher write path, the migration, and design-doc prose)
returns nothing in `aggregator`, `api`, or the frontend. **This is
definitive, not inferential: the column is written every extraction cycle
and never selected by any query anywhere else in the codebase.**

This isn't an accidental oversight this research is the first to notice —
the original design doc says so explicitly, at the time `extracted_category`
was added: "the LLM's own category guess (`extracted_category`) is stored
for cross-validation but remains advisory only... `extracted_category`
itself has no use in severity classification"
(`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md:105-110`),
and its own data-model table: "Stored for future cross-validation against
the regex table; not currently consumed by severity classification"
(same doc, line 285). A later, more recent design doc goes further and
explicitly scopes exposing it *out*: "**Exposing NLP-extraction fields**
(`extracted_category`, `extracted_severity`, `extracted_periods`, etc.) on
the detail page. Not requested by the brief's content list, and these
columns are explicitly documented as an internal `enricher`→`aggregator`
channel, not end-user-facing data"
(`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md:706-710`).
That framing is slightly generous to the current state, though — per the
`load_incidents` query above, `extracted_category` isn't even part of the
"internal `enricher`→`aggregator` channel" today; it's an
`enricher`→database dead end. `extracted_periods`, by contrast, genuinely
is that channel (read at `crates/aggregator/src/queries.rs:35, 59` and
consumed in `crates/aggregator/src/aggregation.rs`, see §5 below).

**Headline finding: this app already runs a live LLM call that produces a
`category` guess for every incident, and nothing downstream — not
severity, not the API, not the frontend — has ever read it.** This
reframes the ask from "add new NLP extraction" to, at least in part,
"decide what the existing extraction output should populate, and wire it
through" — exactly as the task brief anticipated.

### 3. A second, unrelated field also named `category` — do not conflate

`common::Disruption` (`crates/common/src/lib.rs:309-321`) has its own
`category: String` field, documented inline as `"RealTime" | "PlannedWork"
| "Information"`. It is set in `status_from_incident`
(`crates/aggregator/src/aggregation.rs:140`):

```rust
category: if incident.is_planned { "PlannedWork" } else { "RealTime" }.to_string(),
```

— a plain binary flag off `IncidentMessage.is_planned` (itself mapped
straight from the Knowledgebase feed's own `planned` flag,
`crates/poller-incidents/src/schema.rs:109`: `is_planned:
incident.planned`). It has nothing to do with the LLM pipeline, is
rendered at `crates/api/src/render.rs:76`
(`"category": disruption.category`), and reaches the frontend as
`Disruption.category: string` (`frontend/lib/types.ts:13`). This document
uses **`extracted_category`** and **`Disruption.category`** as distinct
names throughout specifically to avoid the collision the task flagged;
neither is what a new disruption-type tag should reuse (see §Taxonomy
below for why).

### 4. There is already a THIRD, independent "rail replacement" signal — and it is verifiably broken for both example texts

This was not mentioned in the task brief but is directly relevant and
worth surfacing as its own finding. `severity_from_incident`
(`crates/aggregator/src/aggregation.rs:262-282`) is a plain keyword
classifier that computes the *base* severity for an incident before any
LLM extraction is applied:

```rust
fn severity_from_incident(incident: &IncidentMessage) -> Severity {
    if incident.is_planned {
        return Severity::PlannedClosure;
    }
    let text = format!("{} {}", incident.summary, incident.description).to_lowercase();
    if text.contains("suspended") || text.contains("no service") {
        return Severity::Suspended;
    }
    if text.contains("rail replacement") || text.contains("replacement bus") {
        return Severity::BusService;
    }
    ...
```

`Severity::BusService` is a real, already-shipped, user-facing severity
value — discriminant 8, description `"Rail Replacement"`
(`crates/common/src/lib.rs:32, 74`), rendered through the same
`statusSeverityDescription` path as every other severity and given its own
color in the frontend (`frontend/lib/severity.test.ts:34`,
`severityColor(8)` → `'red'`). So in one narrow sense the app *does*
already have a "rail replacement" concept — just not one that reaches
either of the task's two motivating examples, for two independent
reasons, both verified by reading the function above against the literal
example text (not executed, but the checks are plain `.contains()`
substring tests, directly traceable by inspection):

1. **The `is_planned` short-circuit fires first.** Both example texts open
   with `[Engineering work]... is taking place` — classic planned
   engineering-works notices, which the Knowledgebase feed marks
   `planned: true` (§3 above). `severity_from_incident` returns
   `Severity::PlannedClosure` on line 264, before the `"rail replacement"`
   check on line 271 is ever reached. **The keyword-based bus-replacement
   detection is structurally unreachable for exactly the incident type —
   planned engineering works — that most commonly involves a rail
   replacement bus in practice**, including both texts given as this
   research's own grounding.
2. **Even for an unplanned incident, the phrasing wouldn't match.** Both
   examples use "buses replace trains" / "buses will replace trains" —
   neither contains the literal substring `"rail replacement"` nor
   `"replacement bus"` (the checked strings require "replacement"
   immediately before "bus", not "buses ... replace"). Real Knowledgebase
   prose apparently doesn't reliably use the phrasing this classifier
   keys on.

This matters for scoping the new work: it is not correct to say the app
has zero rail-replacement awareness today, but the one mechanism it does
have (a) can never fire for planned engineering works at all, and (b)
would miss the literal wording of both real examples even if it could.
Any new extraction target should be judged against this baseline, not
against "nothing exists" — and should not be satisfied by patching the
keyword list, since problem (1) is structural, not a wording gap.

### 5. The multi-period pipeline's design philosophy (what any new field must match)

`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`
generalized the original flat extraction into per-period facts, on a
specific philosophy this research treats as binding on any new field:

- **Err toward fewer periods.** Only split when the text itself
  demarcates a distinct date range and/or scope/impact; several
  stations/lines under one shared date range stay one period
  (`PRIMARY_PROMPT`, `crates/enricher/src/llm.rs:228-232`, and the
  design's own "over-segmentation trap" test fixture,
  `crates/enricher/src/llm.rs:939-943`, empirically confirmed not to
  over-split for the target model,
  `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md:701-712`).
- **Never guess resolved.** `resolution_status` defaults to `ongoing`
  unless the text explicitly says otherwise
  (`crates/enricher/src/llm.rs:261-263`); the same discipline for
  `apparent_severity` defaulting toward the milder reading absent
  explicit escalatory language.
- **Exactly three LLM calls per incident, always** — one primary, two
  adversarial (resolution-adversarial arguing the most cautious reading,
  severity-adversarial arguing the least severe reading), regardless of
  period count
  (`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md:378-386`).
  Adding a field to the *existing* primary call's schema costs one more
  JSON key per response; adding a fourth call would double the pipeline's
  per-incident cost/latency and complicate the `RECLAIM_MIN_IDLE_SECS ≈ 3
  * LLM_REQUEST_TIMEOUT_SECS` retry math the design explicitly built
  around a fixed call count of three.
- **Only `resolution_status` and `apparent_severity` get an adversarial
  check.** `scope_description`, `date_range`, and `schedule_window` are
  primary-pass-only, unverified, and flow straight through
  `combine_periods` untouched:

  ```rust
  Ok(ExtractionPeriod {
      scope_description: period.scope_description.clone(),
      date_range: period.date_range.clone(),
      schedule_window: period.schedule_window.clone(),
      resolution_status,               // combined w/ adversarial pass
      apparent_severity,                // combined w/ adversarial pass
      resolution_status_confidence,
      severity_confidence,
  })
  ```
  (`crates/enricher/src/combine.rs:141-149`; `ADVERSARIAL_PROMPT` and
  `SEVERITY_ADVERSARIAL_PROMPT`, `crates/enricher/src/llm.rs:302-312,
  345-354`, only ever ask for `resolution_status`/`apparent_severity`
  verdicts). This is a load-bearing precedent for §Verification below.
- **Hard-fail-and-discard-and-retry, never partial-credit storage.** Any
  schema-validation, length-mismatch, or ordinal-misalignment failure
  discards the whole attempt and leaves existing columns untouched, retried
  later by the sweep (`crates/enricher/src/combine.rs:63-99`,
  `crates/enricher/src/llm.rs:430-446`).
- **`extracted_periods` is a schemaless JSONB column**
  (`crates/api/migrations/20260822090000_incident_extraction_periods.sql:22`:
  `ADD COLUMN extracted_periods JSONB`). Unlike `extracted_category`'s
  fixed `TEXT` column, adding a new field to `ExtractionPeriod` requires
  **no new migration** — `write_extraction`'s
  `serde_json::to_value(periods)` (`crates/enricher/src/queries.rs:65`)
  serializes whatever fields the Rust struct has. The aggregator's private
  mirror struct (`crates/aggregator/src/aggregation.rs:333-343`) already
  establishes the pattern for adding a field safely against old rows:
  `#[serde(default)]` on `resolution_status_confidence`/
  `severity_confidence` so a row written before the combination step (or
  before this field existed) degrades to an empty string rather than
  failing the whole parse (comment at `crates/aggregator/src/aggregation.rs:328-332`
  spells out exactly why).

### 6. No frontend surface shows disruption type today, beyond severity/planned-vs-real-time/data-quality

`Disruption` on the wire (`frontend/lib/types.ts:12-18`) has `category`
(the `PlannedWork`/`RealTime`/`Information` flag, §3), `description`,
`affectedStops`, `affectedRoutes`, `source` — no type/impact field.
`DisruptionDetail.tsx` renders the sanitized description, a row of
gray-outline `Badge`s for `affectedStops`
(`frontend/components/DisruptionDetail.tsx:14-20`), the affected routes,
and a source/link line — it does not render `disruption.category` at all
today, interestingly (severity is shown one level up via `StatusBadge`).
`IssueList.tsx` filters/labels by `dataQuality`
(`DATA_QUALITY_LABELS`, `frontend/components/IssueList.tsx:36-41`:
Knowledgebase/LDBWS-inferred/Trust-inferred/Planned/TfL) and by validity
bucket (active/upcoming/ended, via `bucketFor`/`governingPeriod` in
`frontend/lib/validity.ts:40-` — note this `governingPeriod` operates over
`LineStatus.validityPeriods`, the RDM-derived `ValidityPeriod` list, a
different multi-entry concept from the enricher's `ExtractionPeriod`
list). There is no badge or label anywhere for "this is a bus
replacement" today. This confirms the task's framing: whatever gets
extracted needs new frontend surface, not just new backend plumbing, or it
repeats exactly the `extracted_category` dead-end (§2).

## Taxonomy/schema evaluation, against both real example texts

### Cause vs. effect — a real axis mismatch, not just a naming collision

The existing `extracted_category` fixture values —
`"signal_failure"`, `"engineering_works"` — are **causes**. A rail
replacement bus is an **effect/impact** of a disruption, not a cause: the
same cause (`engineering_works`) can produce a bus replacement, a plain
platform closure with no substitute service, or nothing passenger-facing
at all, and in principle a different cause (a landslip, flooding,
emergency infrastructure work) could also produce a bus replacement. These
are two orthogonal axes. Even setting aside that `extracted_category` is
unconsumed (§2) and whole-incident-scoped (next point), giving it real
values for "rail replacement bus" would conflate a cause enum with an
effect enum in one free-text field — a bad idea independent of the unused
question. **Recommendation: this is a new field, not a redefinition of
`extracted_category`.**

### Whole-incident vs. per-period — both examples force the per-period answer

Segmenting Example 1 by the existing `PRIMARY_PROMPT` rules (split on a
distinct date range and/or distinct scope/impact) plausibly yields at
least four periods, not one:

| Period (illustrative segmentation) | Impact |
|---|---|
| 29 Aug–11 Sep, Glasgow Central–Kilmarnock/Dumfries/Carlisle | buses replace trains |
| 29 Aug–11 Sep, Kilmarnock–Ayr (Mon–Sat) | buses operate Kilmarnock–Troon, connect to trains |
| 29 Aug–11 Sep, Kilmarnock–Ayr/Stranraer (Sunday) | **no scheduled service at all** — explicitly not a bus |
| 12–13 Sep, similar structure but Carlisle instead of Dumfries, Saturday-only bus leg, Sunday no-service | mixed, same two impact types repeated |

The Sunday "no scheduled services operate... on Sundays" line is not a
weaker version of "buses replace trains" — it is a categorically
different fact (no substitute at all) that the existing
`resolution_status`/`apparent_severity`-per-period machinery already has
the *scaffolding* to hold distinctly (each row is already a
`scope_description` + optional nested `schedule_window` for exactly this
kind of day-of-week variation), just not a field for *which* of these two
things it is. A single flat "this incident is BusService" tag, at the
whole-incident level, would either overwrite the Sunday no-service fact
entirely or force an arbitrary pick between two genuinely different truths
— the task brief's concern, confirmed by working through the real text.

Example 2 is simpler but shows the same shape in miniature: the dominant
fact ("buses will replace trains... via Norwood Junction," Monday–Thursday
overnight) is one clear `rail_replacement_bus` period; "some trains will
be diverted via alternative route" is a second, distinctly-worded fact
with no stated date/time boundary of its own — genuinely ambiguous whether
the existing segmentation rules would give it its own period (a
non-dated, vaguely-scoped aside) or fold it into the same period's
`scope_description`. This is flagged as an open question below rather
than resolved here.

**Recommendation: the impact-type tag belongs on `ExtractionPeriod`, not
`PrimaryExtraction`.** This mirrors exactly how `resolution_status` and
`apparent_severity` already moved from incident-level to period-level in
the 2026-08-21 redesign, for the identical reason (one incident, several
independently-true facts).

### Proposed minimal taxonomy — grounded only in what these two texts need

Per the task's explicit instruction not to invent categories neither
example needs, the taxonomy below is deliberately small:

| Value | Evidence |
|---|---|
| `rail_replacement_bus` | Example 1 ("Buses replace trains between Barrhead and Kilmarnock / Dumfries", "Buses operate between Kilmarnock and Troon"); Example 2 ("buses will replace trains between London Bridge and East / West Croydon") |
| `no_scheduled_service` | Example 1 ("No scheduled services operate between Kilmarnock and Ayr / Stranraer on Sundays") — distinct from a bus replacement, not a milder version of it |
| `diversion` | Example 2 ("Some trains will be diverted via alternative route routes") — weakest-evidenced of the three; a single clause with no distinct date/scope of its own, genuinely arguable whether it earns a period or just a `scope_description` note (see open question above) |
| (default / unset) | the overwhelming majority of periods today — a plain delay/cancellation notice with no specific substitute-service fact stated. Needs a real default value the model returns for this common case (e.g. `"normal_service_impact"` or, more simply, making the field nullable so `null` means "no specific impact type stated" — see below), not a forced pick from the three above. |

Deliberately **not** proposed, despite being named in the task's own
brainstorm list: `reduced_frequency`/`part_cancellation` and
`platform_alteration`. Neither example text describes either — Example
1's "no scheduled service" is a full withdrawal, not a frequency
reduction, and neither example mentions a platform change. Adding them now
would repeat the exact mistake this section is trying to avoid. If a
future worked example demonstrates one of these, add it then, following
the same close-reading discipline used here.

**Field shape recommendation**: `impact_type: Option<String>` (nullable,
not a forced enum member) on `ExtractionPeriod`, using a JSON Schema
`"enum"` constraint the same way `resolution_status`/`apparent_severity`
already do (`crates/enricher/src/llm.rs:213-214`), plus a `null`/absent
option for "no specific impact type stated" — the field should not force
the model to invent a bus-replacement or no-service claim for a plain
delay, mirroring how `date_range`/`schedule_window` are already `["...",
"null"]`-typed for exactly this "this fact doesn't apply here" reason
(`crates/enricher/src/llm.rs:195-206`).

**Naming**: `impact_type`, not `category` or `disruption_category` —
avoids colliding, in either name or connotation, with the two existing
`category` fields (§3/§4 above), and reads correctly as "what kind of
service impact," the effect axis, distinct from `extracted_category`'s
(currently unused, currently undefined) cause axis.

## Prompt design (sketch)

A new paragraph appended to `PRIMARY_PROMPT`, in the same declarative,
rule-by-rule style as the existing per-field instructions, roughly:

> `impact_type` is `rail_replacement_bus` if that period states that
> buses (or another road vehicle) replace, substitute for, or operate in
> place of trains for some or all of the affected journey — regardless of
> the exact phrasing used ("buses replace trains," "a replacement bus
> service," "buses will operate between X and Y"). It is
> `no_scheduled_service` if that period states plainly that no trains (and
> no replacement service) run at all — do not use `rail_replacement_bus`
> for this; a withdrawn service and a substitute service are different
> facts even when both are severe. It is `diversion` if that period states
> trains are running via an different route than usual, without a bus
> substitute. Use `null` for any period that does not state one of these
> three specific facts — an ordinary delay or cancellation notice with no
> stated substitute-service arrangement is `null`, not a forced guess.

Worked example, using Example 1's actual Sunday clause as the
illustrative contrast pair (mirroring how the existing worked example at
`crates/enricher/src/llm.rs:264-274` already uses a real multi-period
platform-closure text): given "Buses operate between Kilmarnock and Troon,
where passengers can connect with trains to / from Ayr" for the Saturday
leg and "No scheduled services operate between Kilmarnock and Ayr /
Stranraer on Sundays" for the Sunday leg of the same date range, these are
two periods with `impact_type: "rail_replacement_bus"` and `impact_type:
"no_scheduled_service"` respectively, not one merged period and not the
same tag applied to both.

This is a sketch of the addition's shape and rigor level, not a
drop-in replacement string — finalizing exact wording, and validating it
against a live model the way the 2026-08-21 design's own "over-segmentation
trap" battery did (`crates/enricher/src/llm.rs:939-996`), is
implementation work, correctly out of scope here.

## Verification posture — a real call, not punted

**Recommendation: no dedicated adversarial pass for `impact_type`.** This
follows the pipeline's own established precedent directly, not a new
argument invented for this feature: `scope_description`, `date_range`, and
`schedule_window` are *already* primary-pass-only fields today, with zero
adversarial verification, and flow straight through `combine_periods`
untouched (`crates/enricher/src/combine.rs:141-149`, §5 above). The
design's stated reason two fields *do* get adversarial passes is that they
each carry a specific, named risk: `resolution_status` because a false
"resolved" wrongly suppresses a real disruption from being shown at all,
and `apparent_severity` because a false escalation causes needless alarm —
both directly gate `apply_extraction`'s severity output
(`crates/aggregator/src/aggregation.rs:553-` area). `impact_type`, as
proposed, is purely a display/annotation fact — it does not feed
`apply_extraction`'s severity floor/ceiling logic, exactly like
`scope_description` doesn't today. Giving it a fourth LLM call would be
inconsistent with the pipeline's own risk-tiering: it would spend the same
verification budget on a lower-stakes fact that two already-shipped,
higher-stakes fields don't get.

The counter-consideration, taken seriously rather than dismissed: unlike
`scope_description` (internal, never matched against, explicitly "never
matched against" per `crates/enricher/src/llm.rs:58`) or `date_range`
(used for time-gating, but a bad value there degrades to `Active`,
fail-safe, per `period_phase`'s doc comment), `impact_type` as recommended
below IS proposed to be shown directly to end users (a "Rail Replacement"
badge). A wrong badge is a more visible, more embarrassing failure mode
than a wrong internal `scope_description`. The call made here is that this
is still not worth a fourth pass: `temperature: 0.0`
(`crates/enricher/src/llm.rs:396`) makes the primary pass deterministic
per input text, the same close-reading prompt discipline that keeps
`resolution_status`/`apparent_severity` reliable applies directly to a
simpler three-way classification, and a wrong-but-plausible badge is a
strictly milder failure than a wrong severity (which can hide a real
disruption or manufacture a false alarm). If real-world mistagging turns
out to be common once this ships (observable the same way the existing
pipeline is observed — spot-checking `extracted_periods` rows), revisit
with a cheap, narrowly-scoped adversarial check specifically for
`impact_type` rather than folding it into the existing two passes' scope.

## Downstream consumption — required, not optional

Per the task's framing, and directly motivated by the `extracted_category`
dead-end this research found (§2): a tag nobody reads is not a smaller
version of this feature, it is a repeat of the exact problem. A real
recommendation has to specify the full path.

1. **`enricher`**: add `impact_type: Option<String>` to `ExtractionPeriod`
   (`crates/enricher/src/llm.rs:54-80`) and to `primary_schema()`'s period
   item properties (`crates/enricher/src/llm.rs:192-217`); extend
   `PRIMARY_PROMPT`; `combine_periods` needs one added line copying it
   through unchanged, the same as `scope_description`
   (`crates/enricher/src/combine.rs:141-149`).
2. **Storage**: no new migration — `extracted_periods` is schemaless
   JSONB (§5 above); `write_extraction`'s existing
   `serde_json::to_value(periods)` picks up the new field automatically.
3. **`aggregator`**: add `impact_type: Option<String>,
   #[serde(default)]` to the private `ExtractionPeriod` mirror
   (`crates/aggregator/src/aggregation.rs:333-343`) — `#[serde(default)]`
   is load-bearing here for the same reason it already is on the
   confidence fields: old rows written before this field existed must not
   fail the whole `extracted_periods` parse (the comment at
   `crates/aggregator/src/aggregation.rs:328-332` already documents
   exactly this failure mode for a different field). `apply_extraction`
   itself does not need to *act* on `impact_type` (it stays outside
   severity computation, per §Verification above) — it only needs to
   surface which period's `impact_type` should reach the API, addressed
   next.
4. **`common`/API shape**: `LineStatus.disruption` is a single
   `Disruption`, not a per-period list (`crates/common/src/lib.rs:309-321`)
   — `status_from_incident` already collapses the incident's several
   `ExtractionPeriod`s down to one severity/reason via `apply_extraction`
   before constructing one `Disruption`
   (`crates/aggregator/src/aggregation.rs:117-141`). This is not a new
   problem this feature introduces — the same collapsing already happens
   for `ValidityPeriod` (`validity_for_output`,
   `crates/aggregator/src/aggregation.rs:158-`, whose own doc comment
   says plainly: "the real schema allows repeated validity periods, the
   output type doesn't"). Recommendation: follow that exact precedent —
   add `impact_type: Option<String>` to `Disruption`
   (`crates/common/src/lib.rs:309-321`), populated from whichever
   period is currently governing (the `PeriodPhase::Active` period
   `apply_extraction`/`period_phase` already identify,
   `crates/aggregator/src/aggregation.rs:354-397`), `None` if no active
   period states one. Render it in `status_to_json`'s existing
   `disruption` block (`crates/api/src/render.rs:74-83`) alongside
   `category`.
5. **Frontend**: add `impactType?: string` to `Disruption`
   (`frontend/lib/types.ts:12-18`); render as a `Badge` in
   `DisruptionDetail.tsx`, following the exact pattern already used for
   `affectedStops` (`frontend/components/DisruptionDetail.tsx:14-20`,
   `variant="outline"`), e.g. a distinctly-colored badge reading "Rail
   Replacement Bus" / "No Service" / "Diverted" placed before the
   description. `IssueList.tsx` is a plausible second surface (a small
   icon/label next to `StatusBadge`) but is not required for a minimal
   ship — `DisruptionDetail` is the natural first home since it already
   has the badge idiom.

## Cost/risk

Adding `impact_type` to the **existing** primary-pass schema is a few
more output tokens per period on a call that already runs; it does not
change the pipeline's call count, and per §Verification above deliberately
does not add a fourth call. This is unambiguously preferable to a separate
pass: the multi-period design was built specifically to keep call count
fixed at three regardless of how much new per-period detail is extracted
(`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md:378-386`,
§5 above), and the `RECLAIM_MIN_IDLE_SECS ≈ 3 * LLM_REQUEST_TIMEOUT_SECS`
retry math (cited in that same design doc) is built around that exact
count — a fourth pass would need its own timeout/reclaim accounting, not
just its own API cost.

## Recommendation

Ranked:

1. **Add `impact_type: Option<String>` to `ExtractionPeriod`, extracted by
   the existing primary pass, with no dedicated adversarial check, wired
   all the way to a `DisruptionDetail` badge.** This is the only option
   evaluated that (a) matches the real shape of both worked examples —
   per-period, not per-incident, (b) reuses cheap existing infrastructure
   (schemaless `extracted_periods` storage, the existing `#[serde(default)]`
   backward-compat pattern, the existing single-primary-call cost model),
   and (c) doesn't repeat the `extracted_category` mistake, because
   downstream consumption (aggregator mirror → `Disruption.impact_type` →
   API → frontend badge) is specified as part of the same piece of work,
   not deferred.
2. **Do not reuse/redefine `extracted_category` for this.** It is a cause
   axis (`engineering_works`/`signal_failure`-shaped), not an effect axis;
   it is whole-incident-scoped, not per-period; and it is currently
   completely unconsumed, so "finally giving it real values" would still
   need every downstream wiring step this document specifies for a new
   field anyway, with the added confusion of a field whose name and axis
   don't match its new purpose. If `extracted_category` is separately
   worth fixing (giving the *cause* axis a real taxonomy/prompt too — the
   gap identified in §1), that's a legitimate, but genuinely separate,
   follow-up; don't conflate the two fixes in one change.
3. **Do not patch `severity_from_incident`'s keyword-based
   `Severity::BusService` detection instead.** §4 above shows it's
   structurally unreachable for planned engineering works (the
   `is_planned` short-circuit fires before the keyword check ever runs) —
   the majority realistic case for a rail replacement bus, and the exact
   shape of both worked examples. Widening its keyword list would not fix
   the structural problem, only the wording problem, and would still be
   whole-incident-scoped, unable to distinguish Example 1's Sunday
   no-service leg from its bus-replacement legs.

## Explicitly out of scope

- Finalizing exact prompt wording, or running a live-eval battery against
  a real model the way the 2026-08-21 design's own
  over-segmentation-trap tests did — that's implementation/eval work, not
  research.
- Resolving whether Example 2's "some trains will be diverted" clause
  earns its own `ExtractionPeriod` or folds into the bus-replacement
  period's `scope_description` — flagged as a genuine open question below,
  not resolved by this document.
- Fixing `extracted_category`'s own missing-prompt-guidance gap (§1) — a
  real, separate gap this research surfaced but was not asked to design a
  fix for.
- Any change to `severity_from_incident`'s keyword classifier.
- A design for showing `impact_type` per-period on the incident detail
  page (rather than one collapsed "currently governing" value on the
  summary `Disruption`) — plausible future work, not designed here; the
  2026-08-31 detail-page design doc explicitly deferred exposing any
  extraction field on that page at all (§2 above), so this would need its
  own scoping pass.
- Migration/implementation planning, schema PRs, or prompt PRs of any
  kind. No file under `crates/enricher` (or anywhere else) was modified
  while producing this document.

## Open questions / risks

1. **Does Example 2's "diversion" clause deserve its own period at all?**
   It has no stated date/time boundary distinct from the surrounding bus
   replacement period. The existing segmentation rule ("split... where the
   text itself demarcates a distinct date range and/or a distinct
   scope/impact") is genuinely ambiguous here — "distinct scope/impact"
   could argue for a split, "err toward fewer periods" could argue against
   it for a single vague aside. This needs the same kind of live-eval
   battery the original multi-period design ran for its own
   over-segmentation risk, not a desk decision.
2. **Model reliability on a three-way (plus null) classification is
   unverified.** This document proposes the taxonomy and prompt shape but
   does not test it against a real model, per the explicit "research, not
   implementation" instruction for this task. The existing pipeline's
   `apparent_severity`/`resolution_status` reliability was empirically
   validated (`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md:701-712`)
   before shipping; `impact_type` would need the same treatment before any
   confidence is warranted.
3. **Whether "currently governing period" is the right collapsing rule for
   `Disruption.impact_type`.** §Downstream consumption proposes reusing
   `PeriodPhase::Active` (the same period `apply_extraction` already
   treats as live) — reasonable by precedent, but not stress-tested
   against a case where two periods are simultaneously `Active` with
   different `impact_type`s (`apply_extraction`'s own test suite already
   covers "two active periods, most severe wins, both annotations kept,"
   `crates/aggregator/src/aggregation.rs:2334-2372` — an analogous
   "which impact_type wins" rule for that case is unresolved here).
4. **`extraction_model_version` bump / re-extraction cost.** Any prompt
   change to `PRIMARY_PROMPT` is, per the existing design's own
   convention, a reason to bump `extraction_model_version` and let the
   sweep re-extract the whole `incidents` table
   (`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`'s
   table entry for that column). Adding `impact_type` would trigger that
   same table-wide re-extraction cost; not evaluated here in absolute
   terms (row count, real per-call latency) since that's an operational
   question, not a design one.

## References

- `crates/enricher/src/llm.rs` — `PrimaryExtraction`/`ExtractionPeriod`
  structs (lines 54-92), `primary_schema()` (185-222), `PRIMARY_PROMPT`
  (224-274), `ADVERSARIAL_PROMPT`/`SEVERITY_ADVERSARIAL_PROMPT`
  (302-312, 345-354), test fixtures using ad-hoc `category` values
  (524, 568).
- `crates/enricher/src/queries.rs` — `write_extraction` (57-85),
  `extracted_category` bind (70).
- `crates/enricher/src/combine.rs` — pass-through of unverified fields
  (141-149).
- `crates/api/migrations/20260820120000_incident_extraction.sql` —
  `extracted_category TEXT` column (11).
- `crates/api/migrations/20260822090000_incident_extraction_periods.sql`
  — schemaless `extracted_periods JSONB` (22).
- `crates/aggregator/src/queries.rs` — `load_incidents` SELECT list
  omitting `extracted_category` (31-40).
- `crates/aggregator/src/aggregation.rs` — `status_from_incident`
  (117-141, `Disruption.category` at 140), `validity_for_output`
  (158-), `severity_from_incident` (262-282), private `ExtractionPeriod`
  mirror (333-343), `PeriodPhase`/`period_phase` (354-397).
- `crates/common/src/lib.rs` — `Severity::BusService` (32, 74),
  `Disruption` struct (309-321).
- `crates/poller-incidents/src/schema.rs` — `is_planned` sourced from the
  feed's own `planned` flag (109).
- `crates/api/src/render.rs` — `status_to_json`'s `disruption` block
  (74-83).
- `frontend/lib/types.ts` — `Disruption` interface (12-18).
- `frontend/components/DisruptionDetail.tsx` — existing `Badge` pattern
  (14-20).
- `frontend/components/IssueList.tsx` — `DATA_QUALITY_LABELS` (36-41).
- `frontend/lib/validity.ts` — `governingPeriod` (52-).
- `docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md` —
  `extracted_category` documented as advisory-only/unconsumed (105-110,
  285).
- `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md` —
  per-period redesign, three-call-count discipline (§2, lines 323-390),
  segmentation-reliability live-eval results (701-712).
- `docs/superpowers/specs/2026-08-31-incident-detail-page-design.md` —
  explicit non-goal of exposing extraction fields on the detail page
  (706-710).
