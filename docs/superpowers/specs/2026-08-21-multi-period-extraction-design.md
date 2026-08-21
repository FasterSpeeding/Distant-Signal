# Multi-Period Incident NLP Extraction — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing extraction design
(`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`,
"the original design" below) so it can be reviewed and iterated on the same
way, but it has not gone through implementation planning and nothing here
is committed.

## Problem

The original design's `PrimaryExtraction`
(`crates/enricher/src/llm.rs:19-31`) treats one incident's `summary` +
`description` as producing exactly one flat set of facts: one `category`,
one `resolution_status`, at most one `schedule_window`, at most one `eta`,
one `apparent_severity`. Real Knowledgebase incident text routinely
describes multiple sequential, distinctly-scoped periods instead. A real
Wandsworth Town incident:

```
Monday 11 May to Sunday 26 July:
Platform 2 at Wandsworth Town is closed. Trains will call at platform 1 during this period.
Monday – Thursday between 11:00 – 14:00:
No trains travelling from London Waterloo will call at Wandsworth Town. Passengers for Wandsworth Town should circulate via Putney.

Monday 27 July to Sunday 11 October:
Platform 3 at Wandsworth Town is closed. Trains will call at platform 4 during this period.
Monday – Thursday between 11:00 – 14:00:
No trains travelling towards London Waterloo will call at Wandsworth Town. Passengers for Wandsworth Town should circulate via Clapham Junction.
```

Two sequential date ranges, each with its own nested weekly time-of-day
restriction, different affected platform/direction, different circulation
advice — and in general (not necessarily this exact example) potentially
different `apparent_severity` and even different `resolution_status` per
period (an elapsed phase vs. an upcoming phase vs. the one actually running
now). `PrimaryExtraction`'s current shape has no way to represent this: the
JSON schema (`primary_schema()`, `crates/enricher/src/llm.rs:85-108`) types
`schedule_window` as `["object", "null"]`, never an array, and every other
field is similarly singular. Forcing this text through today's schema
either collapses the two periods into one (losing the platform/direction
distinction and picking an arbitrary one of the two time windows) or the
model silently drops one period's facts.

Everything downstream inherits the same flat assumption:
`crates/api/migrations/20260820120000_incident_extraction.sql` stores one
row's worth of flat columns per incident, overwritten (not appended to) on
each re-extraction; `LoadedIncident`
(`crates/aggregator/src/queries.rs:18-27`) mirrors that as flat `Option`
fields; `apply_extraction`
(`crates/aggregator/src/aggregation.rs:416-489`) and `now_within_window`
(`crates/aggregator/src/aggregation.rs:298-336`) both assume there is at
most one `ScheduleWindow` to check `now` against; and the rail-day-cutoff
exemption (`has_recurring_schedule`,
`crates/aggregator/src/aggregation.rs:240-246`) reads the single
`extracted_schedule_window` + `extraction_confidence` pair directly.

`IncidentMessage.validity: Vec<ValidityPeriod>`
(`crates/common/src/lib.rs:362-372`) is a genuinely multi-entry Vec, but it
comes from the RDM feed's structured XML via pollers, not LLM text
extraction, and is unrelated to `schedule_window`. It is *also* collapsed
to one displayed period downstream, in `validity_for_output`
(`crates/aggregator/src/aggregation.rs:158-168`) — a documented,
intentional simplification this design does not touch or attempt to fix.

## Goals

- Let `PrimaryExtraction` represent zero-or-more distinct periods per
  incident, each independently carrying a date range, an optional nested
  weekly schedule window, a short scope description, a resolution status,
  and an apparent severity — while the common single-fact case (the
  overwhelming majority of incidents, per the original design's own
  motivating examples) still round-trips through the new shape with no
  meaningful behavior change.
- Keep the three-pass adversarial-verification pattern (primary +
  resolution-adversarial + severity-adversarial) intact in *purpose* — it
  exists specifically to prevent false escalation/false resolution — while
  adapting it to operate over a list of periods, without letting the LLM
  call count scale with period count. This is the central design lever:
  cost/timeout pressure on this pipeline is already a live, recently-tuned
  concern (`8f3801b Expose LLM request timeout and reclaim tuning as Helm
  values`, `0619b8d Make the LLM request timeout configurable and raise its
  default`) that a naive "N periods × 3 calls" design would make
  materially worse.
- Generalize `apply_extraction`/`now_within_window`/
  `has_recurring_schedule` to loop over periods and combine
  simultaneously-relevant ones, reusing the exact "most severe floor wins,
  all annotations kept" combination rule the current code already applies
  across its independent demote-only rows
  (`crates/aggregator/src/aggregation.rs:416-489`), rather than inventing a
  new combination model.
- Preserve every existing safety invariant: demote/escalate only, never
  suppress; a missing or low-confidence signal is always a no-op; malformed
  extraction data must never manufacture a demotion or an age-cutoff
  exemption.
- Do not require a frontend or public-API contract change in this
  iteration — same posture the original design landed on for
  `extracted_schedule_window` (display/annotation text only).

## Non-goals (this iteration)

- No SQL-level querying/filtering of individual periods (e.g. "all
  incidents with a currently-active period"). Periods are opaque JSONB,
  loaded whole and processed in Rust — the same posture `validity_periods`
  already has today, and `apply_extraction`'s existing shape already
  assumes a full in-memory `Vec<LoadedIncident>` pass (DESIGN.md §4: the
  aggregator is pure CPU over a snapshot).
- No structured `periods` array exposed via the public API or frontend.
  Multi-period facts are folded into `LineStatus.reason` as concatenated
  annotation text, exactly mirroring how `validity_for_output` already
  collapses a multi-entry `validity` down to one displayed period today.
  A real structured multi-period UI (e.g. a timeline view) is a plausible
  follow-up, explicitly deferred.
- No unbounded period counts. A soft cap (see §7) bounds both hallucinated
  over-segmentation and worst-case payload/cost growth.
- No per-period `category`. Category remains one flat, advisory-only field
  per incident, unchanged from the original design's non-goal on this
  point.
- No change to `IncidentMessage.validity`/`ValidityPeriod`/
  `validity_for_output`. That is RDM-structured data on an entirely
  separate pipeline from LLM text extraction; its own documented
  single-period-display simplification is out of scope here.
- No automatic reinterpretation of existing single-window extraction rows
  into the new periods shape (see §5) — re-extraction only.
- No cross-period contradiction detection (e.g. flagging two periods with
  overlapping date ranges and materially different severities as
  suspicious) beyond the existing "most severe wins" combination.
- No change to the suppression non-goal already established: this design
  still only ever demotes, escalates, or annotates — never hides a status.

## Design

### 1. New extraction shape

Rust structs (`crates/enricher/src/llm.rs`), replacing the current
`ScheduleWindow`/`PrimaryExtraction` pair:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DateRange {
    /// Null = text doesn't state an explicit start for this period
    /// (treat as "already active as of first_seen_at").
    pub from_date: Option<DateTime<Utc>>,
    /// Null = open-ended / no stated end.
    pub to_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleWindow {
    // unchanged from today — nested *inside* a period now, not top-level.
    pub days_of_week: Vec<u8>,
    pub start_time: String,
    pub end_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractionPeriod {
    /// Short text distinguishing this period's scope from the incident's
    /// other periods (e.g. "platform 2 closed, calls at platform 1").
    /// Display/annotation only — never matched against.
    pub scope_description: Option<String>,
    /// Null = this "period" is really the whole-incident flat fact with
    /// no distinct date range (today's common case, and the shape a
    /// single-fact incident always collapses to).
    pub date_range: Option<DateRange>,
    /// Nested weekly time-of-day restriction *within* date_range, if any
    /// — same semantics as today's top-level field, just scoped to one
    /// period instead of the whole incident.
    pub schedule_window: Option<ScheduleWindow>,
    pub resolution_status: String,       // ongoing | residual | resolved
    pub apparent_severity: String,       // normal | moderate_disruption | severe_disruption | blocked_or_suspended
    /// NOT part of the primary pass's LLM-facing schema below — the model
    /// never asserts its own confidence, exactly as today. Populated by
    /// §2's per-index `combine`/`combine_severity` step once the
    /// adversarial passes return, mirroring the existing top-level
    /// `extraction_confidence`/`extracted_severity_confidence` columns
    /// (original design §6) one-for-one, just scoped per period instead
    /// of per incident. `#[serde(default)]` is load-bearing, not
    /// decorative: the primary pass's JSON schema below never sends these
    /// two fields, so deserializing its response straight into
    /// `ExtractionPeriod` would otherwise hard-fail with a serde "missing
    /// field" error on every single response. With the attribute, a
    /// freshly-parsed `PrimaryExtraction::periods` gets `String::new()` in
    /// both fields, which §2's combination step then overwrites before
    /// anything is stored.
    #[serde(default)]
    pub resolution_status_confidence: String, // high | low
    #[serde(default)]
    pub severity_confidence: String,          // high | low
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PrimaryExtraction {
    pub category: String,          // unchanged, stays incident-level
    pub periods: Vec<ExtractionPeriod>,  // always >= 1 entry
}
```

**"Always >= 1 entry" is a stated invariant, not an enforced one — it must
be enforced in Rust.** Nothing in the JSON schema below constrains
`periods`' length (no `minItems`; §7 item 2 already flags that array-shape
constraints aren't reliably enforceable across backends anyway), so an
empty `periods: []` parses without error. Left unchecked, that would be
recorded as a *successful* extraction — `write_extraction`'s equivalent
would stamp `source_text_hash`/`extracted_at`/`extraction_model_version` as
normal — which then permanently short-circuits `process_incident`'s
unchanged-text guard (`crates/enricher/src/main.rs:164-169`) for that
incident: it looks identical to "already successfully extracted, nothing to
do" on every subsequent sweep/reclaim pass until the incident's text next
changes, silently and irreversibly de-enriching it. So: `periods.is_empty()`
after parsing must be treated the same as a length mismatch (§2) — a hard
failure of the whole extraction attempt, discarded, existing columns left
untouched, sweep retries later.

`eta` is deliberately *not* kept as a separate top-level field: it folds
into whichever period's `date_range.to_date` is currently relevant. A
flat "normal service expected to resume from 18:00" incident becomes a
single period with `date_range: { from_date: null, to_date: 18:00 }` and no
`schedule_window` — functionally identical to today's `eta` field, just
expressed inside the new shape rather than alongside it. This removes a
field rather than adding one, at the cost of the model needing to
understand "an ETA is a one-period date range with no stated start."

**`DateRange` parsing conventions.** Real incident text states dates the
way the Wandsworth Town example does — "Monday 11 May to Sunday 26 July,"
no year, no time-of-day — which leaves three things unspecified unless
pinned down here:

- **Year inference.** The text alone is frequently ambiguous (an incident
  first seen in April describing "11 May to 26 July" almost certainly means
  this year; one first seen in December describing "11 May to 26 July"
  could mean either this year or next). The primary pass's user content
  must therefore include a reference date — the incident's `first_seen_at`
  if the caller has it, otherwise the current date at extraction time — as
  plain context (e.g. "This incident was first reported around {date}.")
  prepended to `summary`/`description`, and the prompt instructs the model
  to resolve any year-less date as whichever occurrence of it falls closest
  to that reference date. This is a real, if small, change to
  `extract_primary`'s signature and user-content construction (today: just
  `summary` + `description`, `crates/enricher/src/llm.rs:226-234`) — worth
  calling out explicitly here rather than leaving it implicit in "the model
  infers the year somehow."
- **Inclusivity.** "to Sunday 26 July" reads in plain English as *through*
  that Sunday, not up to its start. So a stated end day's `to_date` is the
  *following* day's 00:00 in the incident's local time, converted to UTC —
  not that day's own 00:00 — matching the plain-English reading and
  avoiding an off-by-one-day gap where `period_phase` (§4) would treat the
  stated last day of a closure as already elapsed.
- **Timezone.** `DateRange.from_date`/`to_date` are `DateTime<Utc>` on the
  wire, same type as today's `eta`. But a bare date with no stated
  time-of-day — the overwhelmingly common case, since `ScheduleWindow` is
  what carries times, not `DateRange` — is interpreted as a Europe/London
  calendar-day boundary before conversion to UTC, exactly matching
  `ScheduleWindow`'s existing local-time convention
  (`crates/enricher/src/llm.rs:11-13`). Without this, `DateRange` and its
  own nested `ScheduleWindow` would silently disagree about whose clock
  convention governs a period's boundaries.

JSON schema for the primary pass (deliberately omits `resolution_status_confidence`/`severity_confidence` — those don't exist until §2's combination step runs against the adversarial passes' output, so there is nothing for the model to populate them with; requiring them here would just invite a self-reported confidence value the design already doesn't trust, the same reason today's flat `PrimaryExtraction` has no confidence field of its own):

```json
{
  "type": "object",
  "properties": {
    "category": { "type": "string" },
    "periods": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "scope_description": { "type": ["string", "null"] },
          "date_range": {
            "type": ["object", "null"],
            "properties": {
              "from_date": { "type": ["string", "null"] },
              "to_date": { "type": ["string", "null"] }
            },
            "required": ["from_date", "to_date"]
          },
          "schedule_window": {
            "type": ["object", "null"],
            "properties": {
              "days_of_week": { "type": "array", "items": { "type": "integer", "minimum": 1, "maximum": 7 } },
              "start_time": { "type": "string" },
              "end_time": { "type": "string" }
            },
            "required": ["days_of_week", "start_time", "end_time"]
          },
          "resolution_status": { "type": "string", "enum": ["ongoing", "residual", "resolved"] },
          "apparent_severity": { "type": "string", "enum": ["normal", "moderate_disruption", "severe_disruption", "blocked_or_suspended"] }
        },
        "required": ["scope_description", "date_range", "schedule_window", "resolution_status", "apparent_severity"]
      }
    }
  },
  "required": ["category", "periods"]
}
```

**Does `resolution_status` meaningfully vary per period, and is it
separable from the RDM feed's own incident-level state?** Yes to both, and
they answer different questions. RDM's `is_cleared`/`is_planned` are
structured, incident-level, and untouched by any of this. The *extracted*
`resolution_status` has always been "what does the prose itself assert
about the root cause" — per period, that can genuinely differ (a two-phase
engineering project where phase 1 finished exactly as planned and phase 2
is still running is a plausible real case, distinct from the Wandsworth
Town example but the same shape of problem). However: for a period whose
`date_range.to_date` has already elapsed, the *computed* temporal fact
should win over the model's textual guess — deterministic date arithmetic
is strictly more reliable than asking the model to notice a period is over,
consistent with this codebase's existing preference for computing what can
be computed rather than asking a model to reconstruct it (see
`now_within_window`'s and `is_active`'s pure-function design). So: keep
`resolution_status` as a per-period, model-asserted field, but §4's
period-phase logic discards an elapsed period's own asserted
`resolution_status` regardless of what it says, rather than trusting the
model's word for something Rust can verify directly — it does NOT treat
the period as inert, though: an elapsed period still contributes its own
synthetic demotion floor (§4), which is what preserves today's
`extracted_eta`-passed behavior for the common single-period case.

### 2. Prompt redesign

`PRIMARY_PROMPT` (`crates/enricher/src/llm.rs:110-120`) is rewritten to:
instruct the model to segment the text into one entry per period only where
the text itself demarcates a distinct date range and/or a distinct
scope/impact (explicitly: "if the entire text describes one continuous
fact with no clearly distinct sub-periods, return a single-element
`periods` array with `date_range: null`; err toward fewer periods when in
doubt — do not split for stylistic variation within one continuous block");
restate today's per-field rules (`resolution_status`,
`apparent_severity`) as applying *per period*; and describe
`scope_description` as short, display-only text distinguishing what's
different about that period. This over-segmentation risk is the single
biggest reliability question in this design — see §7.

**The three-pass pattern, generalized without multiplying call count.**
The core move: the two adversarial passes do *not* re-derive periods. They
are handed the primary pass's already-segmented period list (`date_range`,
`schedule_window`, `scope_description` — stripped of `resolution_status`/
`apparent_severity`) as part of their user content, and are asked to return
an array of exactly that length, index-aligned, containing only their
verdict for each given period:

- `ADVERSARIAL_PROMPT` (today: `crates/enricher/src/llm.rs:134-138`)
  becomes: "for each of the following N periods, argue the most cautious
  reading — assume `ongoing` unless clear, explicit evidence says
  otherwise — and return one `resolution_status` per period, in the same
  order and against the same `scope_description` given to you."
  Schema: `{ "periods": [{ "scope_description": string|null,
  "resolution_status": "ongoing"|"residual"|"resolved" }, ...] }` (array,
  length not schema-enforceable across periods — see §7 item 3; each
  element echoes back the `scope_description` it was given, not just a
  bare enum value — see the ordinal-alignment paragraph below).
- `SEVERITY_ADVERSARIAL_PROMPT`
  (`crates/enricher/src/llm.rs:160-164`) becomes the mirror-image
  per-period version, same shape, returning `apparent_severity` per period.

**Ordinal alignment is a distinct risk from the length mismatch above, and
harder to catch.** A length-preserving but *reordered* adversarial
response — a known failure mode for "return N things in the same order"
instructions, not a hypothetical — would pass the length check in §7 item
3 and then silently misattribute period A's verdict to period B in the
elementwise combination below, corrupting data with no error raised at
all. This is worse than the already-documented length-mismatch failure,
which at least fails loudly. Mitigation: each adversarial array element
echoes back its `scope_description` (or, for periods where that's `None`,
an explicit `period_index`) alongside its verdict, so the Rust-side
combination step can assert positional alignment — echoed value equals
what was sent at that index — before trusting the array at all, rather
than trusting bare positional order. A mismatch here is treated the same
as a length mismatch: hard failure of the whole attempt, discard, let the
sweep retry.

This keeps the call count fixed at **exactly three per incident** —
primary, adversarial-resolution, adversarial-severity — regardless of how
many periods the primary pass found. Only the *payload size* scales with
period count (primary's output JSON grows; both adversarial passes' input
now includes the serialized period skeleton instead of just
`summary`+`description`). For the common single-period case this is
roughly today's cost plus small serialization overhead; for a genuinely
multi-period incident (rare, per the motivating problem) cost grows mildly
while the call count — and therefore the timeout/reclaim math
(`RECLAIM_MIN_IDLE_SECS` ≈ `3 * LLM_REQUEST_TIMEOUT_SECS`, per
`charts/nr-status/values.yaml:370-388`) — does not need to change at all.

Combination logic (`crates/enricher/src/combine.rs`'s `combine`/
`combine_severity`, `:8-58`) generalizes to operate **elementwise** over
the primary/adversarial arrays, applying the exact same three-row table as
today (agreement → high confidence; primary already at the "no demotion/
escalation possible" extreme (`ongoing`/`normal`) → high confidence,
answer unaffected either way; primary claims a stronger reading than the
adversarial pass agrees with → low confidence, store primary's verdict for
audit only) once per period index, producing
`Vec<ExtractionPeriod>` with `resolution_status`/`resolution_status_confidence`
and `apparent_severity`/`severity_confidence` now living *inside* each
period rather than as incident-level scalars. **A length mismatch between
an adversarial array and the primary period count is a hard failure of the
whole extraction attempt** (discard entirely, leave existing columns
untouched, let the sweep retry) — the same "no partial-credit storage"
philosophy the original design already applies to any schema-validation
failure (§5 of the original design), extended to this new failure mode.

### 3. Storage

**Recommendation: one JSONB array column on `incidents`, not a child
table.** Concretely, replace six existing flat columns
(`extracted_resolution_status`, `extracted_schedule_window`,
`extracted_eta`, `extraction_confidence`, `extracted_severity`,
`extracted_severity_confidence`) with a single new nullable
`extracted_periods JSONB` column holding
`Vec<ExtractionPeriod>` (each period now self-contained, including its own
confidence fields per §2). `extracted_category`, `source_text_hash`,
`extraction_model_version`, and `extracted_at` are unaffected — they stay
incident-level.

Illustrative migration shape (actual file gets its own timestamp at
implementation time, after `20260821090000_incident_severity_escalation.sql`):

```sql
ALTER TABLE incidents
    ADD COLUMN extracted_periods JSONB;
-- extracted_resolution_status / extracted_schedule_window / extracted_eta /
-- extraction_confidence / extracted_severity / extracted_severity_confidence
-- deprecated in code immediately, dropped in a later follow-up migration
-- once the sweep has re-populated extracted_periods for the whole table
-- (see §5).
```

**Tradeoffs considered:**

| | JSONB array (recommended) | Child table (`incident_extraction_periods`) |
|---|---|---|
| Overwrite-on-re-extraction | Free — one `UPDATE ... SET extracted_periods = $n` per row, same atomic-swap semantics `write_extraction` already relies on (`crates/enricher/src/queries.rs:45-89`) | Needs delete-then-reinsert in a transaction; loses the "one UPDATE" simplicity, more surface for a partial-write bug |
| Query shape | Matches the existing pattern exactly — `validity_periods` (`crates/common/src/lib.rs:369`) is *already* a JSONB array of a `Vec<ValidityPeriod>`, loaded whole and processed in Rust by `aggregator` with zero JOINs, direct precedent sitting right next to this feature | Requires a JOIN + `GROUP BY`/`array_agg` (or N+1) in `load_incidents` (`crates/aggregator/src/queries.rs:29-40`) to reconstitute one incident's periods, working against the "one row per incident, pure in-memory pipeline over `Vec<LoadedIncident>`" shape DESIGN.md §4 already establishes as this codebase's principle |
| SQL-level per-period querying | Not supported without JSON operators | Supported, but nothing in the current pipeline needs it (§ Non-goals) |
| Migration risk | Additive nullable column — same proven, zero-downtime pattern as the last two migrations (`20260820120000`, `20260821090000`) | New table + FK + index + a data-migration script to move existing `extracted_schedule_window` rows into it; more moving parts on a live schema with no down-migration tooling evident in this repo's migration history |

Given no part of the current pipeline needs SQL-level per-period access,
and the codebase already has a working precedent for "JSONB array, loaded
whole, processed in Rust" one column away, a child table buys query
flexibility this feature doesn't need at the cost of migration risk and
pipeline-shape friction it does need to avoid.

### 4. Downstream consumption

`LoadedIncident` (`crates/aggregator/src/queries.rs:18-27`) drops its six
flat `Option` fields for one: `extracted_periods: Option<serde_json::Value>`.
`load_incidents`'s `SELECT` list updates to match
(`crates/aggregator/src/queries.rs:29-40`).

New private deserialize types in `aggregation.rs` mirror §1's `DateRange`/
`ScheduleWindow`/`ExtractionPeriod` (parallel to today's private
`ScheduleWindow` at `crates/aggregator/src/aggregation.rs:283-288`).

**Period phase**, a new function analogous in spirit to
`period_covers_now`/`validity_for_output`
(`crates/aggregator/src/aggregation.rs:158-168, 207-209`), but — unlike
that pair — deliberately distinguishing *why* a period is out of scope,
not just whether it is:

```
enum PeriodPhase { Active, Elapsed, NotStarted }

fn period_phase(period: &ExtractionPeriod, now: DateTime<Utc>) -> PeriodPhase {
    // date_range: None => Active (flat fact, always in scope — today's
    // common case).
    // Some(range) that covers now => Active.
    // Some(range) whose to_date has passed => Elapsed.
    // Some(range) whose from_date is still in the future => NotStarted.
}
```

**`NotStarted` periods contribute nothing** — an upcoming phase 2 that
hasn't begun yet is exactly as irrelevant to `now`'s severity as an
unstarted `ValidityPeriod` is today; this half of the original single
`in_scope_now` boolean was correct as originally sketched and is
unchanged in substance, just renamed to make room for the distinction
below.

**`Elapsed` periods are NOT simply excluded — that was the flaw in an
earlier draft of this section.** Folding `eta` into `date_range.to_date`
(§1) means the common single-period case of "normal service expected to
resume from 18:00" becomes a period whose `date_range` elapses at 18:00.
Treating an elapsed period as fully inert — contributing zero floor,
zero annotation — would silently drop today's `extracted_eta`-passed rule
(original design §7: "extracted_eta present, already passed → demote to
`MinorDelays`, annotate 'expected to end by HH:MM'"), which is supposed to
fire on *every* aggregation cycle for as long as the ETA stays in the
past — exactly the mechanism that catches an incident whose stated ETA
came and went but nobody cleared it. Excluding elapsed periods entirely
would reproduce, for the ETA case specifically, the same "shows a stale
severe status indefinitely" failure mode the original 2026-08-20 design
was built to close, defeating this doc's own goal that "the common
single-fact case ... still round-trips through the new shape with no
meaningful behavior change" (Goals).

So: an `Elapsed` period contributes exactly one synthetic floor —
`Severity::MinorDelays`, with an annotation mirroring the existing
`resolved`/eta-passed text (e.g. "expected to end by 18:00" when
`scope_description` is absent, or "platform 2 (11 May–26 Jul): expected to
end 26 Jul" when present) — **regardless of what that period's own
`resolution_status` field asserts.** That last part is the one piece of
"inert" that *is* correct: a model's `ongoing` claim about a period whose
own stated date range has already ended shouldn't be trusted over the
deterministic date arithmetic (§1's "computed beats model text" rule) —
but "don't trust its resolution_status" and "contribute nothing at all"
are different claims, and only the first one holds.

`apply_extraction` (`crates/aggregator/src/aggregation.rs:416-489`)
generalizes from "check the one schedule window" to: for every `Active`
period, run the *same* per-row checks the function already runs today
(resolved/residual floor, schedule-window-excludes-now floor, severity
escalation ceiling), scoped with that period's `scope_description` in the
annotation text when present (e.g. "platform 2 (11 May–26 Jul): reported
active 11:00–14:00 only"); for every `Elapsed` period, contribute the
synthetic floor above instead. All fired floors/escalations across all
`Active` and `Elapsed` periods are combined with the **exact existing
rule**: most severe (highest `severity_rank` — the scale runs ascending
from `GoodService = 0` to the severe tier at `4`, `crates/common/src/lib.rs:84-101`)
floor wins, every firing
annotation is kept and joined (`crates/aggregator/src/aggregation.rs:456-471`'s
`floors.iter().max_by_key(...)` pattern, generalized from "one row per
rule" to "one row per (period, rule) pair"). No new combination model is
introduced — this is a direct extension of logic that already handles
multiple simultaneously-firing signals, and it composes correctly for a
genuinely two-phase incident where phase 1 has elapsed and phase 2 is
`Active`: phase 1 contributes its `MinorDelays` synthetic floor and
annotation, phase 2 contributes whatever its own checks produce, and the
existing most-severe-wins rule picks between them exactly as it already
does for any two simultaneously-firing rows today.

`now_within_window` (`crates/aggregator/src/aggregation.rs:298-336`) is
unchanged in its own logic (still takes one `ScheduleWindow` and `now`) —
it's simply called once per `Active` period's `schedule_window` instead of
once per incident. (An `Elapsed` period's `schedule_window`, if any, is
irrelevant — it only ever contributes the synthetic floor above, never a
schedule-window check of its own.)

`has_recurring_schedule` (`crates/aggregator/src/aggregation.rs:240-246`)
generalizes to: *any* period whose phase is `Active` carries a
high-confidence, successfully-parsed `schedule_window`. **Filtering to
`Active` periods only is load-bearing, not incidental** — checking for a
recurring schedule anywhere in the raw array (without that filter) would
let an incident whose only recurring-schedule period has already elapsed
keep exempting itself from the rail-day cutoff forever, which is exactly
the "SWR forgot about it" failure mode that cutoff exists to catch (see
the function's existing doc comment,
`crates/aggregator/src/aggregation.rs:225-239`). This is the sharpest new
correctness trap this redesign introduces into that function; flagging it
explicitly here so implementation planning doesn't rediscover it the hard
way. Note this is deliberately narrower than `apply_extraction`'s own
period handling above — an `Elapsed` period is allowed to contribute its
synthetic demotion floor, but must NOT be allowed to contribute a
recurring-schedule *exemption*, since granting an age-cutoff exemption
from a schedule that's no longer running is the unsafe direction (§1's
fail-safe asymmetry between demotions and exemptions, applied here too).

**Simultaneously-active periods**, if the text ever genuinely describes two
independently-scoped, temporally-overlapping periods (as opposed to a
single period with a nested narrower `schedule_window`, which is what the
Wandsworth Town example actually is — one period per date range, each with
its own nested window): both are `Active`, both contribute their
floors/escalations/annotations, and the existing most-severe-wins,
all-annotations-kept combination handles it with no special-casing needed.

**Display**: `LineStatus.reason`
(`crates/aggregator/src/aggregation.rs:126-134`) keeps its existing
single-string shape — annotations from multiple `Active`/`Elapsed` periods
are semicolon-joined into it, the same mechanism already used to combine an
escalation annotation with a demotion annotation today
(`crates/aggregator/src/aggregation.rs:424, 470`). No frontend type change
(`frontend/lib/types.ts`) is required. This is a conscious continuation of
the original design's approach to `extracted_schedule_window`, and mirrors
`validity_for_output`'s existing multi-to-one collapse for the unrelated
RDM `validity` data — consistent with, not a new departure from, how this
codebase already displays "the system knows more than it shows in one
line" facts.

### 5. Backward compatibility / migration path

**Re-extract, do not reinterpret.** Existing rows' flat
`extracted_schedule_window` cannot be mechanically wrapped into a
one-element `extracted_periods` array without silently fabricating a
`date_range: null` that may be wrong — the entire point of this redesign
is capturing a date-range dimension the old schema never asked for, so a
row extracted under the old prompt structurally cannot know whether its
text actually had a distinct date range the model just wasn't asked about.
The original design's `extraction_model_version` column already exists to
force re-extraction on a version bump, and the sweep already treats a
mismatch there the same as a stale text hash — but that mechanism is not
"already-built" for this purpose the way an earlier draft of this section
claimed. Today, `crates/enricher/src/main.rs:48` sets
`model_version = config.llm_model.clone()` verbatim, and that same string
is what `LlmClient` sends as the literal `model` field of every
chat-completion request (`crates/enricher/src/llm.rs`'s
`ChatCompletionRequest.model`). Appending a suffix to it — the "bump the
version string, e.g. append `@periods-v1`" idea — would not just force
re-extraction; it would make every subsequent LLM call request a model name
the configured endpoint doesn't have, breaking inference outright. (The
original design's own Open items list this exact question — "should
`extraction_model_version` encode a prompt-template version alongside the
model name" — as still unresolved, not settled by any later commit; there
is no commit in this repository's history that resolves it.)

So this design requires a small but real change to `enricher`, not just a
value bump: `main.rs`'s `model_version` variable must diverge from what's
passed to `LlmClient::new`/sent as `model`. Concretely, `enricher` computes
two separate strings — `config.llm_model.clone()` continues to be the only
thing ever sent to the API as `model`, unsuffixed; a new, separate
`stored_version = format!("{}@periods-v1", config.llm_model)` (or similar)
is what's written to and compared against the `extraction_model_version`
column and used by the sweep's mismatch check. The suffix only ever touches
the stored/compared value, never the wire request, so bumping it (here, and
on any future prompt-only change) forces re-extraction via the existing
sweep mechanism without asking the endpoint for a model it doesn't serve.

Two-step column migration, to bound risk on a live schema:

1. **This design's migration**: add `extracted_periods JSONB` nullable.
   `enricher` starts writing only the new column; `aggregator` starts
   reading only the new column (`extracted_periods: None` behaves
   identically to today's "no extraction yet," same fail-safe default).
   The six deprecated flat columns are left in place, untouched, as a
   rollback window — if a problem surfaces, reverting `enricher`/
   `aggregator` to the previous release still has valid data to read.
2. **Follow-up housekeeping migration**, once satisfied the sweep has
   caught the whole table up (observable via `extraction_model_version`
   coverage, same signal the original design's sweep test already
   exercises): drop the six deprecated columns.

Operational note: bumping the version forces a full-table re-extraction —
3 LLM calls per uncleared incident, all at once, paced only by however fast
the hourly sweep's consumer loop naturally processes its query result
(`crates/enricher/src/main.rs:108-123`). Recommend relying on that existing
pacing rather than writing a special one-shot backfill script — deliberately
listed as a non-goal (§ Non-goals) to avoid a second, less-tested code path
for what the sweep is already designed to do.

## Testing plan (sketch)

- **Golden corpus additions**: the literal Wandsworth Town example as a
  fixture (2 periods, differing scope/direction/circulation advice, same
  nested weekly window shape each time); a genuinely single-period incident
  that superficially looks list-like (e.g. bullet points describing
  multiple *stations* under one shared date range) to catch
  over-segmentation; a 3+ period incident to sanity-check the soft cap
  (§7); an elapsed-period-plus-active-period incident to verify
  `period_phase` classification and combination; a single flat period
  whose `date_range.to_date` is the folded-in `eta` (§1), passed, to
  verify the elapsed-ETA demotion (below) reproduces today's
  `extracted_eta`-passed behavior.
- **`period_phase`**: null date_range (always `Active`), covers now
  (`Active`), elapsed (`Elapsed`), not-yet-started (`NotStarted`),
  malformed/unparseable dates resolve to `Active` — not `Elapsed` (which
  would manufacture a synthetic demotion out of bad data) and not
  `NotStarted` (which would silently drop a period that might genuinely be
  live right now). `Active` falls through to that period's own ordinary,
  independently-confidence-gated checks instead of forcing an outcome,
  mirroring `now_within_window`'s existing "malformed → assume inside the
  window → no forced demotion" fail-safe shape
  (`crates/aggregator/src/aggregation.rs:298-310`).
- **Elapsed-period demotion (the case an earlier draft of this design
  dropped)**: a single-period incident whose `date_range.to_date` (i.e.
  its folded-in `eta`) has passed demotes to `Severity::MinorDelays` with
  an "expected to end by HH:MM"-style annotation, matching today's
  `extracted_eta`-passed rule exactly — asserted regardless of what that
  period's own `resolution_status` claims (an `ongoing`-claiming elapsed
  period must demote identically to a `resolved`-claiming one, since the
  claim is ignored either way).
- **`apply_extraction` with multiple periods**: two `Active` periods both
  firing floors (most severe wins, both annotations kept); one `Active`
  and one `Elapsed` period (the `Elapsed` period contributes only its
  synthetic `MinorDelays` floor and annotation — never its own
  `resolution_status`/`apparent_severity` claims — while the `Active`
  period's own checks run normally, and the more severe of the two floors
  wins); a `NotStarted` period contributing nothing at all; a
  recurring-schedule period that has elapsed no longer exempts the
  incident from the rail-day cutoff (the trap flagged in §4).
- **Elementwise combination** (`combine`/`combine_severity` generalized):
  synthetic primary/adversarial period-array pairs through the existing
  three-row table, per index; a length mismatch between primary and
  adversarial arrays is a hard failure, asserted explicitly.
- **Migration/sweep**: a row with the old flat columns populated and
  `extracted_periods` null is treated as "never extracted under the new
  scheme" and re-queued by the version-bump mechanism, not silently
  wrapped.
- **`DateRange` parsing conventions (§1)**: a fixture with no stated year
  (verify the reference-date-proximity rule against a `first_seen_at` near
  the stated month/day, both for a date shortly before and shortly after
  the reference date); a fixture verifying the inclusive end-of-day
  boundary (a period stated as ending "Sunday 26 July" is still `Active`
  for all of that Sunday and only becomes `Elapsed` starting the following
  day).
- **Empty `periods` array**: a primary-pass response with `periods: []`
  is treated as a hard failure of the whole extraction attempt, identically
  to a length mismatch — not silently accepted as a zero-period success.

## Risks and open questions

1. **Segmentation reliability is the central open risk — EMPIRICALLY
   RESOLVED for `gemma3:12b` (2026-08-21).** The model must reliably
   distinguish "text describing genuinely distinct periods" from "text
   with list-like formatting describing one continuous fact" (e.g. several
   stations affected under one shared date range should *not* become
   several periods). Over-segmentation quietly reintroduces a worse
   version of today's problem (fragmented, possibly-contradictory floors);
   under-segmentation reproduces exactly the original bug this design
   exists to fix. A live-eval battery (Wandsworth Town two-period fixture
   ×3, a flat single-period fixture ×2, and an explicit over-segmentation
   trap ×2 — three stations sharing one date range, which must stay one
   period) against a real self-hosted endpoint found `gemma3:12b` correct
   on all 7 runs, including never falling for the over-segmentation trap.
   `qwen3.5:4b` (this deployment's prior default) was unreliable across
   identical repeats; several other candidates had their own gaps. Full
   comparison table and recommendation:
   `docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`'s
   "Live-eval results (2026-08-21)" section. Still worth re-running this
   battery on any future model/prompt/backend change — this closes the
   risk for the specific combination tested, not permanently.
2. **`strict: true` JSON schema support for variable-length arrays of
   objects across target backends — CONFIRMED WORKING for this
   deployment's Ollama-based endpoint (2026-08-21).** (llama.cpp server,
   vLLM, and any other hosted OpenAI-compatible provider remain
   unverified — the original design's schemas were all flat objects with
   no arrays of objects, so this was new ground.) The live-eval battery
   above exercised the exact `periods` array-of-objects schema against
   several real models on this backend; every completing model (i.e.
   excluding `qwen3.5:9b`, which failed at the connection level rather
   than a schema-rejection level) returned schema-valid JSON for the
   array shape, `gemma3:12b` doing so consistently across every repeat.
   Whether every OTHER backend the project might point `LLM_BASE_URL` at
   also enforces `strict` correctly for this shape still needs its own
   empirical check before switching backends.
3. **No JSON Schema mechanism can enforce "this array's length equals the
   primary pass's array length"** across two separate LLM calls — that
   invariant can only be checked after the fact in Rust, and is treated as
   a hard failure when violated (§2). This is a structural limitation of
   the two-call design, not a bug to fix, and keeping the call count fixed
   at three remains the higher-priority constraint given the team's current
   cost/timeout pressure. But it is not merely "a small amount of
   wasted-call risk" per incident: `chat_completion` sends `temperature:
   0.0` (`crates/enricher/src/llm.rs:206`, unchanged by this design), so a
   mismatch on one incident's *current* text is deterministic — every retry
   against the same input reproduces the identical mismatch. Nothing in the
   retry paths advances past it: a failed attempt never updates
   `source_text_hash`, so the reclaim loop (every `reclaimIntervalSecs` once
   past `reclaimMinIdleSecs`) and the hourly sweep will both keep re-queuing
   that incident indefinitely, at 3 LLM calls per attempt, until its text
   next actually changes. Decision: keep the hard-failure-and-retry behavior
   as-is rather than adding bounded-retry/degradation logic (degrading would
   mean giving up the adversarial safety net for that incident, a worse
   trade) — but make it operationally visible: log consecutive mismatches
   per `incident_id` distinctly from a one-off transient failure (e.g. a
   counter, or a log line explicitly naming it "persistent length mismatch,
   likely needs prompt tuning" rather than folding it into the generic
   malformed-response path), so an operator can tell "this one incident has
   been silently failing for days" apart from ordinary noise.
4. **Ordinal misalignment between primary and adversarial arrays is a
   silent-failure risk, not just the length-mismatch one above.** Even at
   the correct length, an adversarial response that reorders its elements
   relative to the primary pass's periods would misattribute verdicts with
   no detectable error — see the mitigation in §2 (each element echoes
   back its `scope_description`/`period_index` so alignment, not just
   length, is checked before the response is trusted). Whether real
   backends reliably preserve list order under a "same order" instruction
   even without an explicit echo is itself worth empirically checking
   during implementation — the echo is defense-in-depth regardless of the
   answer.
5. **Cost/timeout, even with call count held fixed.** Recent history
   (`8f3801b`, `0619b8d`) shows real self-hosted endpoints already needed
   timeout tuning at *today's* flat, single-period payload sizes. A
   genuinely multi-period incident's larger structured JSON response (both
   in the primary pass's output and the adversarial passes' now-larger
   input) could push local small-model generation time up meaningfully.
   Recommend **not** pre-emptively raising `LLM_REQUEST_TIMEOUT_SECS`'s
   default for this — most incidents will still resolve to one period —
   but explicitly re-running the timeout/reclaim tuning exercise post-
   rollout against the golden corpus's multi-period fixtures specifically.
6. **A soft period-count cap is needed but its exact enforcement point is
   unresolved.** Given open question #2, a hard `maxItems` schema
   constraint may or may not be reliably enforced by a given backend. Plan
   for enforcement in Rust after parse (e.g. if `periods.len()` exceeds
   some N, treat as a schema-adjacent validation failure and discard the
   attempt, same as any other malformed response) rather than depending on
   the schema alone — exact N is a prompt-engineering/eval question, not
   an architectural one.
7. **Annotation-text readability, and a one-time history churn on rollout.**
   Multiple `Active`/`Elapsed` periods' annotations concatenated into one
   `LineStatus.reason` string could produce a long, hard-to-parse line on
   the frontend even though no frontend code change is structurally
   required. Worth a manual UX check against real multi-period fixtures
   before considering this done, even without scoping in a structured
   periods UI. Separately: today's schedule-window annotation is exactly
   `"reported active {start}-{end} only"` (`crates/aggregator/src/aggregation.rs:442`);
   §4 introduces a `scope_description`-qualified variant (e.g. `"platform 2
   (11 May–26 Jul): reported active 11:00–14:00 only"`) whenever a period's
   `scope_description` is present — a user-visible string change for the
   multi-period case only (the flat single-null-scope case keeps today's
   exact text unchanged). Because `write_line_status`'s `normalize_for_diff`
   (`crates/aggregator/src/queries.rs:154-167`) does not strip `reason`
   before comparing, the first time any incident with a non-null
   `scope_description` gets aggregated post-rollout, its changed `reason`
   string will trigger a `line_status_history` row, same as any other
   genuine reason-text change. This is expected, one-time churn per
   affected incident on first rollout, not a bug — worth knowing it'll show
   up in `line_status_history` diffs post-deploy so it isn't mistaken for
   something wrong.
8. **Full-table re-extraction load.** The version-bump migration path
   (§5) triggers a full sweep-driven re-extraction of every uncleared
   incident at 3 calls each — worth sizing against whatever `LLM_BASE_URL`
   endpoint is actually deployed before triggering it, though no special
   tooling is proposed to manage this beyond the sweep's existing pacing
   (§ Non-goals).
9. **This design permanently forecloses parallelizing the three LLM calls.**
   Today (2026-08-20 design), all three calls per incident — primary,
   resolution-adversarial, severity-adversarial — take only
   `summary`+`description` and are mutually independent;
   `crates/enricher/src/main.rs:171-193` calls them serially by choice, not
   necessity, so a future latency optimization could parallelize them. §2's
   change — handing both adversarial passes the primary pass's already-
   segmented period skeleton as input — makes those two calls data-dependent
   on the primary call completing first, so the chain becomes serial by
   necessity instead. This is a known, accepted tradeoff: it's what keeps
   the call count fixed at three regardless of period count (the higher-
   priority constraint per item 5's cost/timeout framing), at the cost of
   giving up that future parallelization option.
