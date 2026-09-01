# Design: Per-Period Disruption Impact Type (`impact_type`)

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(structural template: Goal, verified Current relevant state with
`file:line` citations, Decisions weighing real alternatives, an ASCII
Architecture diagram, Error handling, Testing, Explicitly out of scope,
Open questions/risks). This spec turns
`docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md`
("the research doc," merged to `main` earlier the same day) into a
concrete, implementable design — it does not re-litigate that document's
findings or its ranked recommendation, both of which it adopts, but it
independently re-verifies every load-bearing claim against the source
files itself (see Current relevant state) and adds design work the
research doc explicitly left open: the exact prompt/schema text, the
multi-active-period collapsing rule for `Disruption.impact_type`, the
frontend placement call for `IssueList.tsx`, and the concrete breaking-test
cost of the schema change. No code, migration, or prompt file was touched
producing this document.

## Goal

Give the `enricher` pipeline's existing primary LLM pass a new, nullable,
per-period `impact_type` fact — `rail_replacement_bus`,
`no_scheduled_service`, or `diversion` — and wire it all the way through
`aggregator` → `common::Disruption` → the API's wire shape → a frontend
badge, so a passenger can tell "buses replace trains" apart from "no
service at all" apart from a plain delay, for lines/incidents where the
Knowledgebase text states one of these facts. This is deliberately scoped
as *reuse the existing call, add one field* — see Decisions — and is
written specifically so it cannot repeat the fate of the pre-existing
`extracted_category` field (extracted every cycle, persisted, never read
by anything downstream — see Current relevant state, item 1).

## Current relevant state (re-verified this session)

### 1. `extracted_category` is confirmed dead code, independently re-checked

`PrimaryExtraction` (`crates/enricher/src/llm.rs:88-92`) has `pub category:
String`, required in `primary_schema()` (`crates/enricher/src/llm.rs:189,
220`). Read `PRIMARY_PROMPT` in full
(`crates/enricher/src/llm.rs:224-274`, all ~50 lines) this session: it
never once mentions `category`. `write_extraction`
(`crates/enricher/src/queries.rs:57-85`) binds it into
`incidents.extracted_category` (line 70,78). The aggregator's
`load_incidents` SELECT (`crates/aggregator/src/queries.rs:31-40`,
re-read in full this session) does not select `extracted_category`, and
`LoadedIncident` (same file, lines 18-29) has no field for it — only
`extracted_periods`. This document does not revive or redefine
`extracted_category`; the new field below has a different name, a
different axis (effect, not cause — see Decisions), and a different scope
(per-period, not whole-incident).

### 2. The primary/adversarial pipeline shape (verified by reading `llm.rs` and `combine.rs` in full)

- `ExtractionPeriod` (`crates/enricher/src/llm.rs:54-80`) currently has:
  `scope_description: Option<String>`, `date_range: Option<DateRange>`,
  `schedule_window: Option<ScheduleWindow>`, `resolution_status: String`,
  `apparent_severity: String`, then two `#[serde(default)]` confidence
  fields (`resolution_status_confidence`, `severity_confidence`) populated
  only after `combine::combine_periods` runs.
- `primary_schema()`'s period `items` object
  (`crates/enricher/src/llm.rs:192-217`) lists `scope_description`,
  `date_range`, `schedule_window`, `resolution_status`,
  `apparent_severity` all in its `"required"` array (line 216) — "required"
  here means *the key must be present*, not non-null: `scope_description`
  is typed `["string", "null"]` (line 195) and is still required. None of
  the three non-adversarial-checked fields (`scope_description`,
  `date_range`, `schedule_window`) has `#[serde(default)]` on the Rust
  struct — every existing test-fixture mock response
  (`crates/enricher/src/llm.rs:519-534, 564-602, 624-632, 643-663,
  674-694, 708-723`) explicitly includes all three keys, `null` or
  otherwise, in every period object. This is the exact precedent
  `impact_type` should follow (see Decisions/Testing) — and the exact
  reason adding it as *required-but-nullable* is a breaking change to
  those fixtures, not a backward-compatible addition.
- `combine::combine_periods` (`crates/enricher/src/combine.rs:112-152`)
  only combines `resolution_status` and `apparent_severity` against the two
  adversarial passes' verdicts; `scope_description`, `date_range`, and
  `schedule_window` are copied straight through unchanged in the
  `Ok(ExtractionPeriod { ... })` literal at lines 141-149. The
  length/ordinal-alignment checks (`CombineError::LengthMismatch`/
  `AlignmentMismatch`, lines 71-82, enforced at 117-136) only ever compare
  the *adversarial* arrays against the primary array — they have no
  awareness of `scope_description`/`date_range`/`schedule_window`/a future
  `impact_type` at all, so adding a field neither adversarial pass touches
  cannot interact with those invariants in any way. Confirmed by reading
  `combine_periods` and every one of its 7 unit tests in full.
- `write_extraction` (`crates/enricher/src/queries.rs:57-85`) persists
  `periods` via `serde_json::to_value(periods)` (line 65) into
  `incidents.extracted_periods JSONB` — confirmed schemaless by reading
  the migration that added it,
  `crates/api/migrations/20260822090000_incident_extraction_periods.sql`
  (single `ALTER TABLE incidents ADD COLUMN extracted_periods JSONB;`, no
  companion schema/constraint). **No migration is needed for this
  feature** — a new Rust struct field just serializes into the existing
  JSONB column automatically.

### 3. Aggregator's consumption path (verified by reading `aggregation.rs` and `queries.rs` in full)

- `LoadedIncident` (`crates/aggregator/src/queries.rs:18-29`) carries
  `extracted_periods: Option<serde_json::Value>`, raw, deserialized lazily.
- `aggregation.rs`'s **private** `ExtractionPeriod` mirror
  (`crates/aggregator/src/aggregation.rs:337-348`) is the real
  consumption point:
  ```rust
  struct ExtractionPeriod {
      scope_description: Option<String>,
      date_range: Option<DateRange>,
      schedule_window: Option<ScheduleWindow>,
      resolution_status: String,
      apparent_severity: String,
      #[serde(default)]
      resolution_status_confidence: String,
      #[serde(default)]
      severity_confidence: String,
  }
  ```
  Its doc comment (lines 328-336) explains `#[serde(default)]` is
  load-bearing there specifically so a row written *before* this field
  existed degrades to an empty string (a closed confidence gate) instead
  of failing the whole `extracted_periods` parse. `parse_periods`
  (lines 412-418) already treats any parse failure as "no periods at all,"
  fail-safe.
- `period_phase` (`crates/aggregator/src/aggregation.rs:374-404`) classifies
  each period `Active` / `Elapsed` / `NotStarted` against `now`, using only
  `date_range` — `schedule_window` (day-of-week/time-of-day) is a *separate*
  concern, checked later, only inside `apply_extraction`'s demote branch
  via `now_within_window` (lines 456-494). This means two periods sharing
  one `date_range` (e.g. a Saturday-bus period and a Sunday-no-service
  period both spanning the same multi-week window, exactly Example 1's
  shape) are **both** `Active` simultaneously regardless of which
  day of the week `now` actually is — `period_phase` alone cannot tell
  them apart. This is load-bearing for Decision 4 below.
- `apply_extraction` (`crates/aggregator/src/aggregation.rs:557-659`) has
  no single "the governing period" concept: it computes independent
  escalation candidates and demotion floors across **every** `Active`/
  `Elapsed` period, takes the most-severe-wins result via
  `common::severity_rank`, and joins every firing annotation into one
  semicolon-separated string (confirmed by the
  `apply_extraction_two_active_periods_both_fire_most_severe_wins_both_annotations_kept`
  test, lines 2445-2482). There is no existing precedent function that
  picks "the one period whose facts should represent this incident" —
  this design has to add one (Decision 4).
- `status_from_incident` (`crates/aggregator/src/aggregation.rs:120-156`)
  constructs exactly one `Disruption` per incident/line match
  (lines 139-145):
  ```rust
  let disruption = Disruption {
      category: if incident.is_planned { "PlannedWork" } else { "RealTime" }.to_string(),
      description: ...,
      affected_stops: affected_stations,
      affected_routes,
      source: Some(format!("knowledgebase-incident-{}", incident.incident_id)),
  };
  ```
  — confirming the research doc's claim that the several-`ExtractionPeriod`s-
  to-one-`Disruption` collapse already happens today, the same shape
  `validity_for_output` (lines 158-172) already does for `ValidityPeriod`.
- `severity_from_incident` (`crates/aggregator/src/aggregation.rs:266-288`)
  independently confirms the research doc's Finding 4: `is_planned` returns
  `Severity::PlannedClosure` at line 268, before the `"rail replacement"`/
  `"replacement bus"` keyword check at line 275 is ever reached — that
  keyword path is structurally unreachable for planned engineering-works
  text. The same function's `"diverted"` keyword check (line 284,
  `Severity::Diverted`, described `"Diverted"` at
  `crates/common/src/lib.rs`) is a **third**, pre-existing, whole-incident,
  keyword-driven signal, adjacent in name only to this design's new
  per-period `diversion` `impact_type` value — worth naming explicitly so
  the two are never conflated during implementation.

### 4. API and frontend surfaces (verified by reading each file in full)

- `common::Disruption` (`crates/common/src/lib.rs:309-321`):
  ```rust
  pub struct Disruption {
      pub category: String,       // "RealTime" | "PlannedWork" | "Information"
      pub description: String,
      #[serde(default)] pub affected_stops: Vec<String>,
      #[serde(default)] pub affected_routes: Vec<AffectedRoute>,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub source: Option<String>,
  }
  ```
  No `#[serde(rename_all)]` anywhere on this struct — the public
  camelCase wire shape is produced entirely by hand in `render.rs`, not by
  serde derive.
- `render.rs::status_to_json` (`crates/api/src/render.rs:47-96`) only
  attaches the `disruption` JSON block `if detail` (line 80) — confirmed
  by reading the whole function — and builds it by hand at lines 83-92:
  `category`, `description`, `affectedStops`, `affectedRoutes`, `source`.
  `detail` is a caller-supplied `bool` (`crates/api/src/routes/line_status.rs:48`);
  the two call sites feeding the incident-detail/line-detail pages that
  already render `DisruptionDetail`/`IssueList` pass `true`
  (`crates/api/src/routes/line_status.rs:261, 302`), so a `Disruption`
  reaching either existing frontend surface already always carries a
  `disruption` block today.
- `frontend/lib/types.ts:12-18`:
  ```ts
  export interface Disruption {
    category: string;
    description: string;
    affectedStops: string[];
    affectedRoutes: AffectedRoute[];
    source: string | null;
  }
  ```
  `source` is the precedent for how an `Option<String>` field that's
  *always present but sometimes null* is typed on the wire — not
  `field?: string`. This design follows `source`'s exact shape for
  `impactType`, not the research doc's looser `impactType?: string`
  suggestion (see Decisions).
- `DisruptionDetail.tsx` (`frontend/components/DisruptionDetail.tsx`, read
  in full, 40 lines): renders sanitized `description`, then
  `affectedStops` as `variant="outline" color="gray"` badges, then
  `affectedRoutes`, then a `source`/incident-link line. It does **not**
  render `disruption.category` anywhere.
- `IssueList.tsx` (read in full, 397 lines): the collapsed accordion row
  (`.issueRow`, lines 342-371) shows `StatusBadge`, the `reason` text, a
  validity summary, an optional "N lines" gray-outline badge (only when
  `> 1`, line 355-359), and a `DATA_QUALITY_LABELS`
  (lines 37-43) gray-outline badge (always shown). The expanded panel
  (lines 373-391) shows the "Valid: ..." line, then `DisruptionDetail`
  when `status.disruption` is present, else "No further detail
  available." There is no per-severity-value badge-color map beyond
  `StatusBadge`'s `variant="filled"` — the row's *other* badges
  (lines, data-quality) are deliberately neutral gray-outline, documented
  by the row's own test
  (`renders the data-quality badge as neutral gray, not the brand colour`,
  `frontend/components/IssueList.test.tsx:360`).
- `frontend/lib/severity.ts` (`SEVERITY_TABLE`/`GROUP_COLOR`, lines 5-40):
  the five severity-group colors in use are `green`/`gray`/`blue`/`yellow`/
  `red`. `orange` is unused anywhere in this palette — relevant to the
  badge-color decision below (avoiding a color that already carries
  severity meaning elsewhere on the same page).
- Existing severity 8, `Severity::BusService` ("Rail Replacement",
  `crates/common/src/lib.rs:32,74`), is colored `red` (severe group,
  `frontend/lib/severity.ts:14`) via `StatusBadge` — a structurally
  separate signal from this design's `impact_type` (see research doc
  Finding 4; this design's badge is additive, not a replacement for that
  severity value, and the two can legitimately co-occur or disagree).

## Decisions

### 1. Per-period, not per-incident — reconfirmed against the real segmentation rule

`PRIMARY_PROMPT` instructs the model to segment into `periods` only where
text "demarcates a distinct date range and/or a distinct scope/impact"
(`crates/enricher/src/llm.rs:227-229`). Both example texts described in
the research doc mix impact types across periods within one incident
(Example 1's Saturday-bus vs. Sunday-no-service legs; Example 2's
bus-replacement paragraph vs. its separate diversion clause). A
whole-incident field cannot hold two simultaneously-true, mutually
exclusive facts — it would have to overwrite one or arbitrarily pick.
Placing `impact_type` on `ExtractionPeriod` mirrors exactly how
`resolution_status`/`apparent_severity` already moved from incident-level
to period-level in the prior multi-period redesign, for the identical
reason. **Chosen.**

Alternative considered: extend `extracted_category` (whole-incident,
already-shipped column) instead of adding a new field. **Rejected** — it's
a cause axis (`engineering_works`/`signal_failure`-shaped per its own test
fixtures, `crates/enricher/src/llm.rs:524,568`), not an effect axis;
reusing it would still need every downstream wiring step below, with the
added confusion of a field whose established name doesn't match its new
meaning.

### 2. No dedicated adversarial pass — reconfirmed against `combine.rs`'s actual invariants

Re-reading `combine.rs` in full (Current relevant state, item 2) confirms
the research doc's claim structurally, not just by citation:
`scope_description`/`date_range`/`schedule_window` already flow through
`combine_periods` completely unverified, with zero interaction with the
length/ordinal-alignment checks that exist specifically to protect the two
fields that *are* adversarially checked. `impact_type` is purely
display/annotation — nothing in `apply_extraction`'s escalation-candidate
or demote-floor logic (lines 557-659) reads it, matching
`scope_description`'s existing role exactly. Adding a fourth LLM call
would need its own timeout/reclaim accounting (the design this pipeline
already runs on is built around exactly three calls per incident,
`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`'s
`RECLAIM_MIN_IDLE_SECS ≈ 3 * LLM_REQUEST_TIMEOUT_SECS` math) for a field
that carries none of the two risks (false-resolved, false-escalation) the
two existing adversarial passes exist to catch. **Chosen: primary-pass
only, no adversarial check**, reusing the existing three-call shape as-is.

The counter-consideration (also in the research doc, taken seriously
here too): unlike `scope_description` (internal-only) or `date_range`
(bad values fail safe to `Active`), `impact_type` as designed here *is*
shown directly to end users (Decisions 6/7). A wrong badge is more visible
than a wrong internal annotation. The mitigating factors — `temperature:
0.0` (`crates/enricher/src/llm.rs:396`, confirmed deterministic per input
text), the same close-reading prompt discipline already relied on for
`resolution_status`/`apparent_severity`, and a wrong-but-plausible badge
being a milder failure than a wrong severity — are accepted here as
sufficient to not add a fourth call, same conclusion as the research doc,
independently re-weighed rather than taken on faith.

### 3. Taxonomy: exactly three values plus null, unchanged from the research doc

`rail_replacement_bus`, `no_scheduled_service`, `diversion`, or `null`.
Each of the first three is grounded in a literal quoted clause from one of
the two example texts (research doc's evidence table); `diversion` is
explicitly the weakest-evidenced of the three (a single clause with no
stated date/scope boundary of its own in Example 2) and is kept anyway,
flagged, rather than dropped, because dropping it would silently discard
real evidence rather than making a considered call. **Not adding**
`reduced_frequency`/`platform_alteration` — neither example text supports
either, and inventing a category with a `null` field's own fallback
already available would violate the "grounded only in real evidence, not
invented" instruction that produced this taxonomy in the first place. If a
future incident text demonstrates a fourth value, add it then, on the same
close-reading discipline used here — not preemptively.

### 4. Collapsing multiple `Active` periods' `impact_type` into one `Disruption.impact_type`

This is genuinely new design work — the research doc's Open Question 3
flagged this as unresolved ("whether 'currently governing period' is the
right collapsing rule... not stress-tested against a case where two
periods are simultaneously Active with different `impact_type`s"). Having
re-read `period_phase`/`now_within_window`/`apply_extraction` in full
(Current relevant state, item 3), the exact shape of the ambiguity is now
concrete: **`period_phase` alone cannot distinguish Example 1's
simultaneously-`Active` Saturday-bus and Sunday-no-service periods**,
since both share one overarching `date_range` and `period_phase` never
looks at `schedule_window`.

**Chosen design**: a new function, `governing_impact_type(loaded:
&LoadedIncident, now: DateTime<Utc>) -> Option<String>`, added next to
`apply_extraction` in `crates/aggregator/src/aggregation.rs`:

1. Parse periods via the existing `parse_periods` (unchanged).
2. Filter to `period_phase(period, now) == PeriodPhase::Active` (the
   existing function, unchanged) — matching `apply_extraction`'s own
   definition of "currently relevant," so this never disagrees with the
   severity computation about which periods count as live.
3. Among those, filter to periods whose `impact_type` is `Some`.
4. **Refine further using the existing `now_within_window` helper**:
   prefer a period whose `schedule_window` is either absent, or present
   and currently matching `now` — the exact same real-time refinement
   `apply_extraction`'s own schedule-window demotion check already applies
   (`crates/aggregator/src/aggregation.rs:617-625`), reused here as a
   filter rather than a demotion trigger. This directly resolves the
   Saturday/Sunday case: on a Saturday, only the bus period's window
   matches; on a Sunday, only the no-service period's does; on a weekday
   with neither window matching, neither period counts and the result is
   `None` — correctly reflecting that neither stated fact currently
   applies.
5. Take the **first** (array/text order) remaining candidate's
   `impact_type`.

Called from `status_from_incident`
(`crates/aggregator/src/aggregation.rs:120-156`), alongside the existing
`apply_extraction(base_severity, loaded, now)` call at line 123, and
threaded into the `Disruption { ... }` literal at lines 139-145 as a new
`impact_type` field.

**Alternative considered and rejected**: invent a severity-like ranking
across the three `impact_type` values (e.g. "no service beats bus
replacement beats diversion") and take the most-severe, mirroring
`apply_extraction`'s own "most severe wins" pattern for genuinely
overlapping active periods. **Rejected** — the research doc's own taxonomy
work deliberately avoided treating these three values as ordered (they are
different *kinds* of fact, not different degrees of one fact — a bus
replacement and a no-service outage are not "the same thing, worse"), and
inventing an ordering here would contradict that reasoning for a
tie-break that step 4 above already resolves for the one concrete case
(Example 1) this design is grounded in.

**Named, not resolved, remaining gap**: if two `Active` periods pass step
4 simultaneously — e.g. two periods with different `impact_type` values,
neither carrying a `schedule_window` at all — step 5's "first in array
order" is a real but arbitrary tie-break, with no evidence from either
example text that this case occurs in practice. Flagged in Open
questions/risks rather than resolved with an invented rule, consistent
with how `diversion` itself was flagged rather than dropped in Decision 3.

### 5. Field shape: required-but-nullable in the schema, no `#[serde(default)]` on the enricher struct — a real, load-bearing choice, not incidental

Two shapes were weighed for `ExtractionPeriod::impact_type` in
`crates/enricher/src/llm.rs`:

- **Match `scope_description`/`date_range`/`schedule_window`'s exact
  precedent**: `pub impact_type: Option<String>`, no `#[serde(default)]`,
  added to `primary_schema()`'s period-item `"required"` array
  (`type: ["string", "null"]`, `enum` constrained to the three values plus
  `null`, matching `resolution_status`/`apparent_severity`'s existing
  `enum` usage at `crates/enricher/src/llm.rs:213-214`). This forces the
  model to always answer the question (present, possibly `null`) rather
  than silently omitting the field — the same contract every other
  primary-pass field already has. **Chosen** — consistency with every
  sibling field in this exact struct outweighs the cost below.
- **`#[serde(default)]`, field optional/absent-tolerant.** Would avoid
  breaking any existing test fixture (below), but would create the only
  field in this struct that behaves differently from its four siblings for
  no reason tied to `impact_type`'s own semantics — `#[serde(default)]`
  exists elsewhere in this codebase specifically for *backward
  compatibility with data written under an older schema* (the aggregator's
  confidence fields, and this design's own aggregator-side mirror, item
  below), not as a stylistic default for "this field happens to be
  optional." Using it here for a field the *current* schema always emits
  would blur that distinction. **Rejected.**

**Real, load-bearing cost of the chosen shape**: every existing primary-pass
mock JSON fixture in `crates/enricher/src/llm.rs`'s test module (the
period objects inside `extract_primary_parses_a_single_flat_period`,
`extract_primary_parses_multiple_periods_with_nested_schedule_windows`,
`extract_primary_rejects_periods_beyond_the_soft_cap`,
`extract_primary_accepts_periods_exactly_at_the_soft_cap`,
`extract_primary_threads_the_reference_date_into_user_content` — 5 test
functions, several period objects each) constructs its mock response
JSON **without** an `impact_type` key. Once the Rust struct field has no
`#[serde(default)]` and the real (non-test) schema marks it required, none
of these mock bodies need to change for `serde_json::from_str::<PrimaryExtraction>`
to keep working (deserialization only cares about the Rust struct's
`#[serde(default)]`, not a JSON Schema `"required"` list the mock server
never validates against) — **but this needs restating precisely**: the
mock fixtures bypass `primary_schema()` entirely (they hand-write the
`ChatCompletionResponse` body directly), so schema `"required"` has no
effect on them at all. The actual breaking-compilation cost is narrower
and purely Rust-level: every `ExtractionPeriod { ... }` **struct literal**
in this codebase — the test helpers in `crates/enricher/src/combine.rs`'s
`period()` function (line 215-225) and `combine_periods`'s own
`Ok(ExtractionPeriod { ... })` construction (lines 141-149) — must add an
`impact_type: ...` field or the crate fails to compile, since Rust struct
literals require every field. This is self-enforcing (the compiler catches
every site), unlike a JSON-fixture omission, which is why it is listed here
as a cost, not a risk — see Testing.

### 6. Aggregator's private mirror: `#[serde(default)]` IS load-bearing here, for a different reason than item 5

`crates/aggregator/src/aggregation.rs:337-348`'s private `ExtractionPeriod`
mirror must add `#[serde(default)] impact_type: Option<String>` **with**
`#[serde(default)]` — the opposite choice from item 5, for a different,
non-conflicting reason: this struct deserializes *stored* JSONB rows,
including rows written by an `enricher` process running *before* this
change ships, which have no `impact_type` key at all. Without
`#[serde(default)]` here, every pre-existing `extracted_periods` row would
fail to parse the instant this aggregator build deploys, and
`parse_periods`'s fail-safe (`crates/aggregator/src/aggregation.rs:412-418`,
treating a parse failure as "no periods") would silently drop **every**
extracted fact — not just the new one — for every incident until the
sweep re-extracts it. This mirrors exactly the existing doc comment at
lines 328-336 for `resolution_status_confidence`/`severity_confidence`,
now applying to a third field for the same reason.

### 7. Frontend badge treatment: `DisruptionDetail.tsx`, and — a real call — `IssueList.tsx`'s collapsed row too

**`DisruptionDetail.tsx`** (expanded panel): add a small badge above the
existing sanitized-description block, shown only when
`disruption.impactType` is both present and a recognized value. Mirrors
the existing `affectedStops` badge idiom in shape (a `Badge` inside the
same `Stack`) but deliberately **not** its `variant="outline" color="gray"`
styling — that styling already means "neutral provenance/list metadata" on
this page (also used by `IssueList`'s lines/data-quality badges); a
rail-replacement fact is a different kind of information (an operationally
significant substitute-service fact, per the research doc's own framing),
so it should not visually blend into the same "gray metadata" bucket.
**Chosen: `variant="light" color="orange"`** — `orange` is unused anywhere
in `frontend/lib/severity.ts`'s existing palette (`green`/`gray`/`blue`/
`yellow`/`red`), so this new badge cannot be mistaken for a severity
reading (severity 8/`BusService` itself already renders `red` via
`StatusBadge` when it fires) or for provenance metadata (already `gray`).
Label text: `"Rail Replacement Bus"` / `"No Scheduled Service"` /
`"Diversion"`, from a small shared lookup (below) — not the raw enum
string.

**`IssueList.tsx`'s collapsed row** — the real call the task asked for,
not deferred: **include it there too**, as a small badge in
`.issueRow__meta`, shown only when `status.disruption?.impactType` is a
recognized value, placed as the first badge in that group (before the
optional "N lines" badge, before the always-present data-quality badge).
Reasoning:

- The research doc's own framing is that this fact is "operationally
  significant to a passenger" — someone scanning a line's issue list
  without expanding any row is exactly the reader this matters most for;
  requiring an expand-click to learn "this is a bus replacement, not a
  train delay" undersells the stated motivation for doing this work at
  all.
- It is **directly precedented by this exact row's own existing
  conditional badge**: the "N lines" badge (`IssueList.tsx:355-359`) is
  already only rendered when there is something worth saying (`length >
  1`), keeping the row uncluttered in the common case. `impactType` is
  `null` for "the overwhelming majority of periods today" per the research
  doc's own evidence table — so, like the lines badge, it renders on a
  small minority of rows, not as permanent added chrome on every row.
- Since `status.disruption` is already present in the data `IssueList`
  receives whenever a `disruption` block exists at all (confirmed: the
  expanded panel already conditionally reads `status.disruption` today,
  `IssueList.tsx:383-384`, and Current relevant state item 4 confirms the
  API calls this page's data comes from already pass `detail: true`), no
  new fetch or backend change is needed to make this data available at the
  collapsed-row level — it is a pure rendering addition.

Uses the same `orange`/`light` treatment and shared label lookup as
`DisruptionDetail.tsx`, at `size="sm"` to match the row's other badges.

**Shared label lookup, not duplicated per file**: both components need the
same three-entry label map. Following this codebase's existing pattern of
small, focused `frontend/lib/*.ts` modules imported by `IssueList.tsx`
already (`dateFormat.ts`, `severity.ts`, `validity.ts`, all imported at
`IssueList.tsx:17-23`), add a new small `frontend/lib/impactType.ts`
exporting `IMPACT_TYPE_LABELS: Record<string, string>` and a lookup
helper, imported by both `DisruptionDetail.tsx` and `IssueList.tsx`.
**Not** duplicated inline in each file (as `DATA_QUALITY_LABELS` currently
is, locally, inside `IssueList.tsx` alone) — that pattern works there
because only one component uses it; here two components render the same
fact and must not silently drift on label wording between the collapsed
and expanded view of the same badge.

**Unrecognized value fails safe to "render nothing," not the raw string**:
if `impactType` is present but not one of the three known keys (a future
taxonomy addition the frontend hasn't shipped yet, or schema drift), both
badges render nothing rather than the raw snake_case value — unlike
`severityColor`'s fallback-to-`'gray'` (which must always render
*something*, since every status has a severity), `impact_type` is already
optional/supplementary everywhere in this design, so silently omitting an
unrecognized value is safe and consistent with `null` already meaning "no
specific fact stated."

## Architecture

```
 Knowledgebase incident text (summary + description)
        │
        ▼
 enricher: LlmClient::extract_primary()          crates/enricher/src/llm.rs
   PRIMARY_PROMPT (extended, new paragraph)         PRIMARY_SCHEMA (extended,
   ── one call, same as today ──                     new "impact_type" field
        │                                             on each period item)
        ▼
 PrimaryExtraction { periods: Vec<ExtractionPeriod> }
   each period now carries impact_type: Option<String>
        │
        ▼
 combine::combine_periods()                       crates/enricher/src/combine.rs
   copied through UNCHANGED (no adversarial pass    (one added line in the
    touches it, same as scope_description/           Ok(ExtractionPeriod{...})
    date_range/schedule_window)                       literal)
        │
        ▼
 write_extraction() → incidents.extracted_periods JSONB   (NO migration —
        │                                                   schemaless column)
        ▼
 aggregator: load_incidents() → LoadedIncident.extracted_periods (raw JSON)
        │
        ▼
 aggregation.rs private ExtractionPeriod mirror   #[serde(default)] impact_type
   (backward-compat for pre-change rows)             (load-bearing, Decision 6)
        │
        ▼
 status_from_incident()
   apply_extraction()        → (Severity, Option<String> annotation)   [unchanged]
   governing_impact_type()   → Option<String>                          [NEW, Decision 4]
        │
        ▼
 common::Disruption { ..., impact_type: Option<String> }   [NEW field]
        │
        ▼
 render.rs::status_to_json()  →  "disruption": { ..., "impactType": ... }
   (only when detail=true, same gate every other disruption field already has)
        │
        ▼
 frontend/lib/types.ts  Disruption.impactType: string | null   [NEW field]
        │
        ├──▶ DisruptionDetail.tsx   — badge above the description (Decision 7)
        └──▶ IssueList.tsx          — badge on the collapsed row  (Decision 7)
             (both via a shared frontend/lib/impactType.ts label lookup)
```

## Prompt/schema addition

### `primary_schema()` (`crates/enricher/src/llm.rs:185-222`)

Add to each period item's `"properties"`, and to that item's
`"required"` array:

```json
"impact_type": {
    "type": ["string", "null"],
    "enum": ["rail_replacement_bus", "no_scheduled_service", "diversion", null]
}
```

```json
"required": ["scope_description", "date_range", "schedule_window", "resolution_status", "apparent_severity", "impact_type"]
```

### `ExtractionPeriod` (`crates/enricher/src/llm.rs:54-80`)

Add, after `apparent_severity` and before the two confidence fields
(grouped with the other primary-pass-only, non-adversarially-checked
fields):

```rust
/// `rail_replacement_bus` | `no_scheduled_service` | `diversion` | `null`.
/// Primary-pass-only, like `scope_description` -- no adversarial check
/// exists for this field (see design doc Decision 2), so it is copied
/// through `combine::combine_periods` unchanged.
pub impact_type: Option<String>,
```

### `PRIMARY_PROMPT` addition (`crates/enricher/src/llm.rs:224-274`)

A new paragraph, in the same declarative, rule-by-rule register as the
existing `resolution_status`/`apparent_severity`/`date_range` paragraphs,
appended before the existing worked example (or as a second worked
example immediately following it — an implementation-time call, not
decided here):

> `impact_type` is `rail_replacement_bus` if that period states that
> buses (or another road vehicle) replace, substitute for, or operate in
> place of trains for some or all of the affected journey — regardless of
> the exact phrasing used ("buses replace trains," "a replacement bus
> service," "buses will operate between X and Y"). It is
> `no_scheduled_service` if that period states plainly that no trains
> (and no replacement service) run at all — do not use
> `rail_replacement_bus` for this; a withdrawn service and a substitute
> service are different facts even when both are severe. It is
> `diversion` if that period states trains are running via a different
> route than usual, without a bus substitute. Use `null` for any period
> that does not state one of these three specific facts — an ordinary
> delay or cancellation notice with no stated substitute-service
> arrangement is `null`, not a forced guess.

Worked example, built from the real quoted fragments the research doc
already extracted from Example 1 (the Barrhead–Dumfries notice), matching
`PRIMARY_PROMPT`'s existing worked-example convention of giving a full
input/output pair:

> Worked example, reference date 2026-08-01T00:00:00Z: input "Buses
> operate between Kilmarnock and Troon, where passengers can connect with
> trains to / from Ayr, Saturdays 29 August to 12 September. No scheduled
> services operate between Kilmarnock and Ayr / Stranraer on Sundays 30
> August to 13 September." segments into two periods (each with its own
> `schedule_window` restricting it to the stated day) — period 1:
> `scope_description` "Saturday bus, Kilmarnock–Troon", `schedule_window`
> `{"days_of_week": [6], ...}`, `impact_type: "rail_replacement_bus"`;
> period 2: `scope_description` "Sunday no service, Kilmarnock–Ayr/
> Stranraer", `schedule_window` `{"days_of_week": [7], ...}`,
> `impact_type: "no_scheduled_service"`. Note these are two periods with
> two different `impact_type` values, not one merged period and not the
> same tag applied to both — a substitute bus service and a full
> withdrawal are different facts even on immediately adjacent days of the
> same date range.

This is a concrete draft, not a final validated string — actually pinning
exact wording against a live model (the way the original multi-period
design's own over-segmentation-trap battery did,
`crates/enricher/src/llm.rs:936-996`) is implementation/eval work,
correctly out of scope for a design document (see Explicitly out of
scope).

### Rollout: `extraction_model_version` bump required

`crates/enricher/src/main.rs:75` bakes a literal suffix into the version
string: `format!("{}@periods-v1", config.llm_model)`. `sweep.rs`'s
`incidents_needing_extraction` (`crates/enricher/src/sweep.rs:28-33`)
re-extracts any row whose stored `extraction_model_version` doesn't match
the current one — the existing, already-used mechanism for forcing a
table-wide re-extraction after a prompt/schema change
(`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`'s
documented convention for this column). This change must bump that
literal (e.g. `"@periods-v2"`) so the sweep picks up every existing
incident and backfills `impact_type` — otherwise only newly-created or
newly-changed incidents would ever get a value, and every currently-active
incident would show `impact_type: null` until it happened to be
re-extracted for an unrelated reason. Operational cost (row count, real
latency) not evaluated here, matching the research doc's own posture on
this same open item.

## Error handling

- **Old `extracted_periods` rows with no `impact_type` key**: handled by
  Decision 6's `#[serde(default)]` on the aggregator's private mirror —
  degrades to `None`/`null`, not a parse failure, not a dropped incident.
- **A malformed/unrecognized `impact_type` value from a schema-drifted or
  future model response** (not one of the three enum values, despite the
  schema's `enum` constraint — `strict: true` on the JSON schema
  (`crates/enricher/src/llm.rs:394`) is the primary guard, but is a
  per-backend guarantee, not a Rust-level one): `Option<String>` accepts
  any string; nothing downstream panics on an unrecognized value —
  `governing_impact_type` treats it as an opaque string and passes it
  through, and the frontend's label lookup (Decision 7) fails safe to
  "render no badge" for anything not in `IMPACT_TYPE_LABELS`. No new
  validation logic is added in `aggregator` for this — consistent with how
  `escalation_ceiling` (`crates/aggregator/src/aggregation.rs:500-506`)
  already treats an unrecognized `apparent_severity` value as "never
  escalate" rather than erroring.
- **No period is `Active`, or no `Active` period has an `impact_type`**:
  `governing_impact_type` returns `None` — the correct, common-case
  outcome (research doc: "the overwhelming majority of periods today"),
  not an error.
- **Two simultaneously-`Active`, schedule-window-matching-or-absent
  periods with different `impact_type`s**: not treated as an error —
  Decision 4's "first in array order" tie-break always produces a value,
  deterministically, per `temperature: 0.0`'s guarantee that array order
  is stable for unchanged input text. Named as an accepted, unresolved
  imprecision in Open questions/risks, not a crash or a discarded
  extraction.
- **Schema-validation failure on the primary pass overall** (e.g. the
  model omits the now-required `impact_type` key, or emits a value outside
  the `enum`, and the backend's `strict: true` mode rejects it): behaves
  identically to any other primary-pass schema failure today —
  `extract_primary` bails with an error (`crates/enricher/src/llm.rs:429-430`
  covers malformed JSON generally), the whole attempt is discarded, no
  partial write happens, and the sweep/reclaim loop retries later. No new
  failure-handling code path is introduced by this field — it rides the
  existing hard-fail-and-discard-and-retry mechanism.

## Testing

Following this repo's existing patterns file-by-file, per the task's
explicit "extend existing patterns, don't invent a new testing approach"
instruction:

- **`crates/enricher/src/llm.rs`**: extend the existing primary-pass mock
  JSON fixtures (the tests listed in Decision 5) to include an
  `"impact_type"` key (`null` in most, a real value in at least one) in
  each period object, then assert `result.periods[i].impact_type` matches.
  Add one new fixture/test specifically exercising a non-null value (e.g.
  `"rail_replacement_bus"`) round-tripping through `extract_primary`,
  mirroring the existing `apparent_severity`/`resolution_status` coverage
  shape. The live-eval battery (`live_eval_battery`, `#[ignore]`d,
  `crates/enricher/src/llm.rs:981-996`) is the natural place to add the
  Example-1-shaped Saturday/Sunday fixture as a new named case (following
  `"multi"`/`"flat"`/`"trap"`'s existing pattern) once the prompt wording
  is finalized — not fixture-testable without a real model, consistent
  with how segmentation reliability is already tested only there.
- **`crates/enricher/src/combine.rs`**: update the `period()` test helper
  (line 215-225) to accept/pass through an `impact_type` (or hardcode
  `None` if the helper's signature shouldn't grow — implementation-time
  call), and add one test asserting `combine_periods` copies a non-null
  `impact_type` through unchanged, mirroring the absence of any existing
  `scope_description`-preservation-specific test today (there isn't one —
  `combine_periods_combines_each_index_independently` implicitly covers it
  via equality on the whole returned struct — the same implicit coverage
  is sufficient here too, so a dedicated new test is optional, not
  required, for parity with existing rigor).
- **`crates/aggregator/src/aggregation.rs`**: this is where the real new
  behavior lives (`governing_impact_type`), and needs dedicated coverage,
  mirroring the existing `period_phase_*`/`apply_extraction_*` test style
  (JSON period fixtures built inline, `LoadedIncident` constructed
  directly, as in `apply_extraction_two_active_periods_both_fire_most_severe_wins_both_annotations_kept`,
  lines 2445-2482):
  - a single `Active` period with a non-null `impact_type` → that value.
  - no periods, or no `Active` period → `None`.
  - an `Active` period with `impact_type: null` → `None` (not an empty
    string, not an error).
  - the Saturday/Sunday two-period case from Decision 4: two periods
    sharing one `date_range` (both `Active`), different
    `schedule_window`s, different `impact_type`s — assert the result
    matches whichever period's `schedule_window` matches the test's `now`,
    for both a Saturday `now` and a Sunday `now`.
  - both periods' `schedule_window`-matching fails (a weekday `now`) →
    `None`, not an arbitrary pick.
  - an `Elapsed` or `NotStarted` period's `impact_type` never contributes,
    mirroring the existing `apply_extraction_not_started_period_contributes_nothing`/
    `apply_extraction_elapsed_period_never_uses_its_own_resolution_status_or_severity_claims`
    style (lines 2532-2580).
  - a pre-change row (`extracted_periods` JSON with no `impact_type` key
    at all in a period object) still parses and yields `None` for that
    period, proving Decision 6's `#[serde(default)]` — the same failure
    mode already covered for the confidence fields, now for this field
    too.
- **`crates/api/src/render.rs`**: no dedicated existing test module was
  found for `status_to_json` in this pass (none of the reads above
  surfaced one) — if implementation adds `impactType` to the JSON block
  without one existing, a minimal test confirming a `Some`/`None`
  `Disruption.impact_type` renders as the literal string/`null`
  respectively (matching `source`'s existing `Option<String>` → wire-null
  precedent) is reasonable, but not mandated beyond what implementation
  planning decides — this spec does not assert a test module exists where
  this session's reads didn't find one.
- **`frontend/lib/types.ts`**: no runtime test needed (a type-only
  change); confirm via the existing `tsc`/type-check step already in this
  repo's CI, not a new test.
- **`frontend/components/DisruptionDetail.test.tsx`**: extend the existing
  `sample: Disruption` fixture (currently missing the new field entirely —
  add `impactType: null` as its default, matching how the fixture already
  carries every other field literally) and add tests mirroring the
  existing `affectedStops`/`source` coverage shape: renders the badge with
  the correct label for each of the three known values; renders no badge
  when `null`; renders no badge for an unrecognized string (Error handling
  above).
- **`frontend/components/IssueList.test.tsx`**: extend the existing
  `disruption:` object literals already used in this file's tests (lines
  250-263, 267-279) with `impactType`, and add a test mirroring
  `renders the data-quality badge as neutral gray, not the brand colour`
  (line 360) in spirit: a collapsed row with a non-null `impactType`
  renders the new badge with the correct label; a row with `impactType:
  null` (the common case) renders none of the existing rows' badge counts
  differently than today — i.e. this addition must not perturb any
  existing assertion in this 600+-line test file for the overwhelming
  majority of fixtures that don't set `impactType` at all.

## Explicitly out of scope

- **Finalizing exact prompt wording or running a live-eval battery against
  a real model.** The prompt paragraph and worked example above are a
  concrete draft in the right register, not a validated string — per the
  research doc's own scoping and this task's "design only" constraint,
  pinning wording against real model output is implementation/eval work.
- **Resolving whether Example 2's "diversion" clause earns its own
  `ExtractionPeriod` or folds into the bus-replacement period's
  `scope_description`.** Unresolved by the research doc, still unresolved
  here — this is a segmentation-rule question, not an `impact_type` schema
  question, and needs the same live-eval treatment as any other
  segmentation-reliability question in this pipeline.
- **Fixing `extracted_category`'s own missing-prompt-guidance gap.** A
  real, separate, pre-existing gap (Current relevant state, item 1) this
  design does not touch — reconfirmed dead code, not repaired.
- **Any change to `severity_from_incident`'s keyword classifier**
  (`Severity::BusService`, `Severity::Diverted`). Both remain exactly as
  they are; this design adds a structurally independent, per-period signal
  alongside them, not a replacement.
- **A UI filter/facet chip for `impact_type`.** `IssueList.tsx` already
  has two chip-filter rows (Severity, Source/`dataQuality` —
  `IssueList.tsx:248-289`) with an established `chipRowLabel`/`ChipGroup`
  pattern that a third "Impact type" row could technically reuse
  mechanically. **Deliberately not designed here**, for a reason specific
  to this field rather than generic caution: both existing filter axes
  (severity, data-quality) are populated on **every** status, so filtering
  by them always partitions the visible list meaningfully; `impact_type` is
  `null` on the overwhelming majority of periods (research doc's own
  framing), so a filter chip for it would spend permanent chrome on a
  facet that's usually not applicable to anything in the list at all —
  a materially different cost/value trade-off than reusing the existing
  pattern would suggest, not a trivial extension. A genuine follow-on, not
  designed further here.
- **Exposing `impact_type` per-period on the incident detail page**,
  rather than one collapsed "currently governing" value on the summary
  `Disruption`. The 2026-08-31 incident-detail-page design doc already
  scoped extraction-field exposure out of that page entirely; unchanged by
  this document.
- **Migration/schema PRs, prompt file edits, or any code.** Confirmed no
  migration is needed (Current relevant state, item 2); this document
  makes that claim concretely, not by assertion — the exact JSONB column
  and its schemaless nature were re-read this session.

## Open questions/risks

1. **The Decision 4 tie-break for two simultaneously-eligible `Active`
   periods with different `impact_type`s and no `schedule_window` to
   disambiguate them is a real, unresolved gap** — "first in array order"
   is deterministic and grounded in the one concrete case this design
   tested against (Example 1's Saturday/Sunday split, which *does* have
   disambiguating `schedule_window`s), but has no evidence either way for
   the schedule-window-less case. Worth revisiting once real production
   data shows whether this case occurs.
2. **Model reliability on this three-way-plus-null classification is
   unverified**, same open item the research doc already named — this
   design proposes the schema/prompt shape but, per Explicitly out of
   scope, does not test it against a real model.
3. **The `extraction_model_version` bump (rollout section above) triggers
   a whole-table re-extraction** — real cost (row count × per-call
   latency) not evaluated here, an operational question for
   implementation/rollout planning, not a design one.
4. **Whether the worked-example paragraph should be appended after the
   existing worked example or interleaved as a second full example** is
   left as an implementation-time call — both are plausible within
   `PRIMARY_PROMPT`'s existing style, and the difference doesn't change
   this design's schema/data-flow claims either way.
5. **`IssueList.tsx`'s collapsed-row badge (Decision 7) adds a fourth
   possible badge to an already 3-badge-capable row** (`StatusBadge`,
   optional lines badge, data-quality badge). Not benchmarked or
   user-tested for visual crowding on a narrow viewport — this row already
   has documented narrow-viewport stacking behavior
   (`marks up the collapsed row so it can stack on narrow viewports`,
   `IssueList.test.tsx:374`), which this addition relies on continuing to
   work rather than re-verifies from scratch.
