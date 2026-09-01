# Enricher `MAX_PERIODS` Soft-Cap Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the enricher's retry-forever failure mode for incidents whose primary LLM extraction produces more `ExtractionPeriod`s than `MAX_PERIODS` (currently 8). Today `LlmClient::extract_primary` hard-fails (`anyhow::bail!`) on a cap breach, before `queries::write_extraction` ever runs — `source_text_hash`/`extraction_model_version` never advance, so the hourly sweep and the reclaim loop re-select and re-fail the same incident indefinitely, and all NLP-derived severity signal for it is lost. This plan implements two of the design's three axes now (Axis 1: tighten `PRIMARY_PROMPT` so fewer incidents hit the cap in the first place; Axis 3: change the failure mode itself, from hard-fail to truncate-to-the-`MAX_PERIODS`-most-severe/soonest-periods-and-succeed) and schedules the third (Axis 2: the *process* for eventually raising `MAX_PERIODS`, once Axis 1 has real eval data to size it against) as an explicit, separate, not-yet-executable follow-on task.

**Architecture:**

```
crates/enricher/src/llm.rs
  PRIMARY_PROMPT (const &str, lines 224-274)
    + new guidance paragraph (Task 1, inserted after the existing
      "still one period" sentence)
    + new second worked example (Task 1, appended before the closing
      quote)
  live-eval battery (lines 896-1081)
    + BARRHEAD_DUMFRIES_*/NORWOOD_JUNCTION_* fixtures + 1 soft-assertion
      test each (Task 2)
        │ (Task 1, Task 2: Axis 1 -- narrows how often the cap is hit)
        ▼
  PrimaryExtraction { +dropped_period_count: usize }
  extract_primary's over-cap branch (lines 440-445):
    bail! -> sort-and-truncate, Ok(..) always (Task 3)
  new fn select_periods_within_cap / effective_from_date (Task 3)
        │ (Task 3: Axis 3 -- what happens when the cap is hit anyway)
        ▼
crates/enricher/src/combine.rs
  severity_hint_rank: private -> pub(crate) (Task 3, reused, not duplicated)
        │
        ▼
crates/enricher/src/main.rs
  process_incident, right after the primary_result match (lines 285-291):
    + dropped_period_count > 0 branch: tracing::warn! +
      enricher_period_truncations_total counter (Task 4)
    + new #[cfg(test)] mod tests, live-DB crux test proving
      source_text_hash/extraction_model_version now advance on a
      truncated-but-successful write (Task 4)
        │
        ▼ (Task 5: depends only on Task 1+2's fixtures existing)
  Axis 2 process task -- run the extended live-eval battery, record the
  period-count distribution, THEN choose MAX_PERIODS's replacement value
  and update its doc comment. Not executable in this environment (no
  live LLM_BASE_URL); tracked as an explicit task, not skipped.
        │
        ▼
Task 6: cargo test -p enricher (+ cargo build --workspace sanity check --
  enricher is a leaf binary crate, nothing else depends on it)
```

**Tech Stack:** Rust, `crates/enricher` only (a standalone binary crate — grep-confirmed no other workspace crate depends on it). `wiremock` (already a dev-dependency) for HTTP-level LLM mocking; `sqlx::postgres::PgPoolOptions` + `DATABASE_URL` (already used elsewhere in this crate/workspace) for the one live-DB test, following `crates/api`'s established `#[ignore = "requires a live database..."]` convention — no new dependency added anywhere in this plan.

**Spec:** `docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md` — read in full before starting; this plan does not restate its research or its rejected alternatives (schema change to `schedule_window`, flatten-to-single-period, a synthetic marker period, a distinct `extraction_model_version` suffix — all explicitly rejected there, do not reintroduce any of them). Cross-references below to "Decision N" refer to that document. **Note for implementers:** as of this plan's writing, this spec lives on `main` but not yet on this plan's own branch (`worktree-agent-a4cfc47cdb0482b87`, base commit `1a883cd`) — merge or cherry-pick `docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md` and `docs/superpowers/specs/2026-09-01-enricher-period-cap-failures-research.md` (its prerequisite) in before starting Task 1, or fetch their content from `main`.

**Status note — every citation below re-confirmed directly against `main`'s current source (commit `3e31e60`), not trusted from the spec, since this plan's own branch predates the spec:** `crates/enricher/src/llm.rs` matches the spec's own citations line-for-line: `MAX_PERIODS: usize = 8` at line 179 (doc comment 168-178), `primary_schema()` 185-222, `PRIMARY_PROMPT` 224-274 (the "do NOT split... that is still one period" sentence spans 230-231, the worked example ends at line 274), `extract_primary` 420-447, the over-cap `bail!` at 440-445, the live-eval battery section 896-1081 (`WANDSWORTH_TOWN_*` 921-931, `FLAT_ETA_*` 933-935, `TRAP_*` 939-943, `live_eval_battery` 981-996, `live_eval_wandsworth_town_segments_into_two_periods` 1000-1044). Two existing tests shifted by one line versus the spec's citation (`extract_primary_rejects_periods_beyond_the_soft_cap` is at 641-669, not 640-669; `extract_primary_accepts_periods_exactly_at_the_soft_cap` is at 672-700, not 671-700) — immaterial, noted for precision. `crates/enricher/src/combine.rs`'s `severity_hint_rank` is at lines 36-42 (spec's own citation of `combine.rs:144` for `combine_periods`'s clone-through of `schedule_window` also confirmed, at line 144 exactly). `crates/enricher/src/main.rs`'s `process_incident` is at lines 252-347, the `primary_result` match at 285-291, `record_llm_call_metrics` at 221-233, `MismatchTracker` at 150-180. `crates/enricher/src/sweep.rs`'s `incidents_needing_extraction` is at lines 27-35 (fn signature at 27, not 28 as the spec states — one line off, immaterial). `crates/enricher/src/queries.rs`'s `write_extraction` (57-85) confirmed to persist only `category`/`periods` (the post-`combine::combine_periods` list) — `PrimaryExtraction.dropped_period_count` is never written to the database by this plan, matching Decision 3's "no schema change" (nothing in `crates/aggregator` or `crates/api` needs touching).

**New finding this plan's own verification pass surfaced, not called out by the spec:** the design's Decision 3 selection rule says "sort by `severity_rank(apparent_severity)`", but `common::severity_rank` (`crates/common/src/lib.rs:108`) takes a `common::Severity` enum value, not the four `apparent_severity` strings (`normal`/`moderate_disruption`/`severe_disruption`/`blocked_or_suspended`) that live on `ExtractionPeriod` — those two types are unrelated, and `common::Severity`/`severity_rank` are not even used anywhere in this crate today (grep-confirmed: `enricher`'s `Cargo.toml` does depend on `common`, but only for `common::metrics`). `crates/enricher/src/combine.rs` already solves exactly this problem for a different purpose: `severity_hint_rank(hint: &str) -> u8` (`combine.rs:36-42`) maps the same four strings to a 0/1/2 ordinal (`blocked_or_suspended`/`severe_disruption` tied at 2, `moderate_disruption` at 1, everything else at 0) for `combine_severity`'s escalation-detection logic. Task 3 below promotes this existing function from private to `pub(crate)` and reuses it for period selection, rather than duplicating an equivalent ranking function or pulling `common::Severity`/`escalation_ceiling`-style logic into this crate for the first time — the same relative ordering (severe/blocked > moderate > normal) is all Decision 3's selection rule needs, and this crate already has it.

## Global Constraints

- **No schema change to `schedule_window`, anywhere.** Decision 1 evaluates and rejects it. `ExtractionPeriod`, `primary_schema()`, `ADVERSARIAL_PROMPT`/`SEVERITY_ADVERSARIAL_PROMPT`, `combine::combine_periods`, and `crates/aggregator` are all out of scope for every task in this plan.
- **No new dependency, dev or otherwise.** Every task uses only what's already in `crates/enricher/Cargo.toml` (`wiremock`, `sqlx`, `chrono`, `serde`, `metrics`, `tracing`) — no `metrics-util`/`tracing-test`/similar added for observability testing (see Task 4's own note on why the counter/log aren't independently unit-tested).
- **Axis 2 (choosing a new `MAX_PERIODS` value) is a process, not a number, and is not implemented in this plan.** Task 5 documents the process steps as an explicit, trackable task — do not pick a specific replacement value in any other task, and do not fold Task 5's steps into Task 1-4's work.
- **Truncation must never produce an `Err` from `extract_primary`.** This is the entire point of Decision 3 — a cap breach after this plan succeeds and writes normally, advancing `source_text_hash`/`extraction_model_version` so the sweep stops re-selecting the incident. If any task's test asserts `.is_err()` for an over-cap response, that test is wrong per this plan, not a valid regression check.
- **No synthetic "N periods omitted" marker period, and no per-row `extraction_model_version` suffix (e.g. `+truncated`).** Both rejected explicitly in Decision 3 — the former because `apply_extraction`'s `"ongoing"` branch has no annotation-producing arm (so it would be silently invisible), the latter because it would permanently desync from `sweep::incidents_needing_extraction`'s single comparison string and reintroduce the exact retry-forever bug this plan closes. Do not add either in any task.
- **Truncation selection key:** `(severity_hint_rank(apparent_severity) descending, effective_from_date ascending with `None` sorting first)`, where `effective_from_date` collapses both "no `date_range` at all" and "`date_range.from_date: None`" to `None` (both already mean "treat as already active" per `DateRange`'s own doc comment, `llm.rs:18-19`). Use `combine::severity_hint_rank` (promoted to `pub(crate)` in Task 3) for the severity ordinal — do not introduce a second, differently-tuned ranking function.
- **Testing convention:** `#[cfg(test)] mod tests` colocated per file, `#[tokio::test]` + `wiremock::MockServer` for anything hitting `LlmClient` (matching every existing test in `llm.rs`), `cargo test -p enricher` to run the whole crate. The one test needing a live database (Task 4) follows `crates/api`'s established `db_tests`/`#[ignore = "requires a live database; run with ..."]` pattern (e.g. `crates/api/src/data/queries.rs:1120-1145`'s `test_pool()`/seed/assert/cleanup shape) — this is a new pattern for `crates/enricher` specifically (it has none today), not an existing one being extended.
- **Live-eval fixtures (Task 2) are `#[ignore]`d and not part of ordinary CI**, exactly like the three existing ones (`WANDSWORTH_TOWN_*`, `FLAT_ETA_*`, `TRAP_*`) — soft `NOTE:`-log assertions, never hard-pinned expected period counts, per the design's own disclosed "not run against a live model yet" caveat.
- **Sequencing:** Task 1 → Task 2 (Axis 1, both edit `PRIMARY_PROMPT`/the eval-fixture section of `llm.rs`, strictly sequential — Task 2's fixtures exist to measure Task 1's prompt change). Task 3 → Task 4 (Axis 3, `main.rs`'s Task 4 needs `dropped_period_count` from Task 3's field addition). Axis 1 (Tasks 1-2) and Axis 3 (Tasks 3-4) are logically independent of each other per the spec (different code paths, unrelated Decisions) — this plan sequences Axis 1 first only because both tracks touch `llm.rs` in different regions and a single-branch sequential implementation avoids an unnecessary merge; do the reverse order and nothing breaks. Task 5 (Axis 2) depends only on Task 1 + Task 2 landing (it measures against their fixtures) — it does **not** depend on Task 3/4, but must not start before Task 2. Task 6 (verification) depends on all prior tasks.

---

### Task 1: `PRIMARY_PROMPT` — new day-of-week-across-legs guidance + second worked example

**Files:**
- Modify: `crates/enricher/src/llm.rs` (the `PRIMARY_PROMPT` const, lines 224-274)

**Interfaces:**
- Produces: no new function/type — `PRIMARY_PROMPT`'s text grows, its `&str` type and every consumer signature (`chat_completion`, `extract_primary`) are unchanged.
- Consumed by: Task 2's new eval fixtures (they exist to measure this task's prompt change against real segmentation behavior).
- **Depends on:** nothing — this is the foundational Axis 1 task.

This is Decision 1's prompt-only fix for the day-of-week-within-a-shared-date-range-across-legs gap (Example 1, "Barrhead-Dumfries": a Mon-Sat bus-substitute leg and a Sunday no-service leg inside the same date range, which the current prompt's "several stations/lines listed under one shared date range -- that is still one period" rule would wrongly merge).

- [ ] **Step 1: Insert the new guidance paragraph**

In `crates/enricher/src/llm.rs`, `PRIMARY_PROMPT` currently reads (lines 229-232, exact text):

```
    stations/lines listed under one shared date range -- that is still one period. `periods` must always \
    contain at least one element. \
```

Insert the new sentence immediately after "...that is still one period." and before "`periods` must always contain at least one element.", so the const becomes:

```rust
const PRIMARY_PROMPT: &str = "You extract structured facts from UK National Rail Knowledgebase incident \
    text. Read the summary and description exactly as given -- do not speculate beyond what the text \
    states. The text describes one incident that may cover one or MORE distinct periods; segment it into \
    the `periods` array. Only split into more than one period where the text itself demarcates a distinct \
    date range and/or a distinct scope/impact -- if the entire text describes one continuous fact with no \
    clearly distinct sub-periods, return a single-element `periods` array with `date_range: null`. Err \
    toward fewer periods when in doubt: do NOT split for stylistic variation, repeated wording, or several \
    stations/lines listed under one shared date range -- that is still one period. When a shared date \
    range covers multiple named route legs, keep them in one period only if every leg is treated \
    identically -- same substitute service or lack of one, same `apparent_severity`, same \
    `resolution_status` -- and describe every affected leg together in `scope_description`. Split into a \
    separate period per leg (or per leg-and-day-of-week combination) whenever the text states a genuinely \
    different treatment for one leg or for specific days within the range -- e.g. one leg gets a rail \
    replacement bus while another has no scheduled service at all, or a leg's rule only applies on certain \
    days of the week and a different rule applies on the rest. A no-scheduled-service statement is never \
    the same fact as a rail-replacement-bus statement, even when both fall inside the same date range and \
    even when the text presents them as neighboring clauses -- do not merge them, and do not let the shared \
    date range alone suggest they are one period. `periods` must always \
    contain at least one element. \
    For each period: `scope_description` is short, display-only text distinguishing what's different about \
    that period from the incident's other periods (e.g. \"platform 2 closed, calls at platform 1\"), or \
    null if there's only one period. `resolution_status` is `resolved` only if the text explicitly says the \
    disruption/root cause has ended; `residual` if it says the cause is fixed but knock-on effects continue; \
    `ongoing` otherwise, including whenever the text doesn't clearly say either way -- judge this \
    per-period, since different periods of the same incident can genuinely have different resolution \
    states. `apparent_severity` is your own read of how severe that period's disruption sounds, independent \
    of any specific keywords: `blocked_or_suspended` if any line, route, or station is described as blocked, \
    suspended, or closed to trains; `severe_disruption` if the text describes major/widespread delays, \
    cancellations, or long journey-time increases without an outright blockage; `moderate_disruption` for a \
    noticeable but contained impact; `normal` for routine minor delay language with no sign of broader \
    impact. \
    `date_range` MUST be populated whenever the text states an explicit date, even approximately -- never \
    leave it null and describe the dates only in `scope_description` instead; `scope_description` is for \
    what's DIFFERENT about the period (platform, direction, route), not a place to restate dates you should \
    have structured. `date_range` is null ONLY when the text truly states no date at all for that period. \
    When present, `from_date`/`to_date` must each be a fully-resolved ISO-8601 UTC timestamp string (or null \
    on either side, meaning no stated start/end respectively). An ETA (\"normal service expected to resume \
    from 18:00\") is expressed as a period whose `date_range.to_date` is that time, with `from_date: null` \
    -- do not add a separate field for it. Apply these conventions when resolving a stated date into that \
    timestamp: (1) Year inference -- the text is given together with a reference date this incident was \
    first reported around; if a date has no stated year, resolve it to whichever occurrence of that \
    month/day falls closest to the reference date -- do NOT invent an unrelated year. (2) Inclusivity -- a \
    stated end day (e.g. \"to Sunday 26 July\") means *through* that day, so its resolved `to_date` must be \
    the *following* day's 00:00 in Europe/London local time, converted to UTC -- not that day's own 00:00. \
    (3) Timezone -- a bare date with no stated time-of-day is a Europe/London calendar-day boundary; convert \
    it to UTC accounting for GMT/BST as appropriate for that date. `schedule_window` is null unless the text \
    states a weekly time-of-day restriction narrower than the period's own date range. \
    When in doubt about `resolution_status`, choose `ongoing` -- never guess `resolved` or `residual` from \
    tone, length, or the absence of further detail; only an explicit statement that the disruption or its \
    root cause has ended justifies anything other than `ongoing`. \
    Worked example, reference date 2026-03-01T00:00:00Z: input \"Monday 6 April to Friday 15 May: Platform 3 \
    at Clapham Junction is closed, trains call at platform 4. Saturday 16 May to Sunday 14 June: Platform 5 \
    is closed, trains call at platform 6.\" segments into exactly two periods -- period 1: \
    `scope_description` \"platform 3 closed, calls at platform 4\", `date_range` `{\"from_date\": \
    \"2026-04-06T00:00:00Z\", \"to_date\": \"2026-05-16T00:00:00Z\"}` (2026 because that's the closest \
    occurrence to the March 2026 reference date; `to_date` is the day AFTER the stated 15 May end), \
    `schedule_window: null`, `resolution_status: \"ongoing\"` (no statement that it has ended); period 2: \
    `scope_description` \"platform 5 closed, calls at platform 6\", `date_range` `{\"from_date\": \
    \"2026-05-16T00:00:00Z\", \"to_date\": \"2026-06-15T00:00:00Z\"}`, `resolution_status: \"ongoing\"`. Note \
    both periods got real `date_range` values -- never null when dates are stated -- and neither was marked \
    `resolved` just because the text is matter-of-fact.";
```

(Step 2 below appends the second worked example before the final `";` — shown together as the complete new const in Step 2 to avoid two overlapping diffs on the same literal.)

- [ ] **Step 2: Append the second worked example**

Replace the final sentence's terminator (`...matter-of-fact.";`) with the new worked example appended before the closing quote, so the const's tail becomes:

```rust
    both periods got real `date_range` values -- never null when dates are stated -- and neither was marked \
    `resolved` just because the text is matter-of-fact. \
    Second worked example, reference date 2026-08-01T00:00:00Z: input \"From Saturday 29 August to Friday \
    11 September, buses replace trains between Barrhead and Kilmarnock / Dumfries. Monday to Saturday \
    during this period, buses operate between Kilmarnock and Troon, where passengers can connect with \
    trains to / from Ayr. No scheduled services operate between Kilmarnock and Ayr / Stranraer on \
    Sundays.\" segments into exactly three periods, all sharing the same overall date range but none \
    merged into one, because each names a different leg and/or a different treatment: period 1 -- \
    `scope_description` \"buses replace trains, Barrhead to Kilmarnock / Dumfries\", `date_range` \
    `{\"from_date\": \"2026-08-29T00:00:00Z\", \"to_date\": \"2026-09-12T00:00:00Z\"}`, `schedule_window: \
    null` (applies every day of the range), `apparent_severity: \"severe_disruption\"`; period 2 -- \
    `scope_description` \"buses operate Kilmarnock to Troon, connecting to Ayr trains\", same `date_range`, \
    `schedule_window` `{\"days_of_week\": [1,2,3,4,5,6], \"start_time\": \"00:00\", \"end_time\": \"23:59\"}` \
    (Monday-Saturday only), `apparent_severity: \"severe_disruption\"`; period 3 -- `scope_description` \"no \
    scheduled service, Kilmarnock to Ayr / Stranraer\", same `date_range`, `schedule_window` \
    `{\"days_of_week\": [7], \"start_time\": \"00:00\", \"end_time\": \"23:59\"}` (Sunday only), \
    `apparent_severity: \"blocked_or_suspended\"` (a full withdrawal is more severe than a bus substitute, \
    not the same fact restated). Note periods 2 and 3 are NOT merged despite sharing both the date range \
    and the same underlying Kilmarnock-Ayr/Stranraer leg -- the text states two different treatments for \
    different days, which is exactly the case that must still split even though 'several things under one \
    shared date range' would otherwise argue for merging.";
```

- [ ] **Step 3: Run the existing test suite to confirm nothing regressed**

Run: `cargo test -p enricher --lib llm::`
Expected: PASS, identical to before this change — every existing test mocks the LLM's *response* directly via `wiremock` (`respond_with(ResponseTemplate::new(200).set_body_json(...))`); none of them assert on `PRIMARY_PROMPT`'s literal text content, so editing it cannot break any existing assertion. This step exists to catch a stray syntax error in the new `&str` literal (an unescaped `"` would be a compile error), not a behavior regression.

- [ ] **Step 4: Commit**

```bash
git add crates/enricher/src/llm.rs
git commit -m "Add day-of-week-across-legs guidance and a second worked example to PRIMARY_PROMPT"
```

---

### Task 2: New eval fixtures — `BARRHEAD_DUMFRIES_*` / `NORWOOD_JUNCTION_*`

**Files:**
- Modify: `crates/enricher/src/llm.rs` (the `#[cfg(test)] mod tests` live-eval section, lines 896-1081)

**Interfaces:**
- Produces: `BARRHEAD_DUMFRIES_SUMMARY`/`BARRHEAD_DUMFRIES_DESCRIPTION`, `NORWOOD_JUNCTION_SUMMARY`/`NORWOOD_JUNCTION_DESCRIPTION` (new `const &str` pairs), wired into `live_eval_battery`'s loop; new test `live_eval_barrhead_dumfries_segments_into_six_periods`.
- Consumed by: Task 5 (the Axis 2 process runs this extended battery).
- **Depends on:** Task 1 (these fixtures exist specifically to measure Task 1's prompt change — land after it, not before, even though nothing here fails to compile without Task 1).

Per Decision 1, these fixtures are built only from the confirmed quoted fragments and the sibling research doc's own segmentation table (`docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md`, lines 325-334, 344-352) — not invented prose.

- [ ] **Step 1: Add the `BARRHEAD_DUMFRIES_*` fixture**

Add alongside the existing `TRAP_SUMMARY`/`TRAP_DESCRIPTION` consts (after line 943):

```rust
    // Day-of-week-across-legs stress test (design doc Decision 1): two
    // date ranges, each containing three co-existing legs with genuinely
    // different treatments -- the exact shape the new PRIMARY_PROMPT
    // guidance (Task 1) targets. Built only from fragments confirmed
    // quoted in docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md
    // (lines 325-334), not invented prose.
    const BARRHEAD_DUMFRIES_SUMMARY: &str = "Buses replace trains between Barrhead and Dumfries";
    const BARRHEAD_DUMFRIES_DESCRIPTION: &str = "From Saturday 29 August to Friday 11 September, buses \
        replace trains between Barrhead and Kilmarnock / Dumfries. Monday to Saturday during this period, \
        buses operate between Kilmarnock and Troon, where passengers can connect with trains to / from Ayr. \
        No scheduled services operate between Kilmarnock and Ayr / Stranraer on Sundays. \
        From Saturday 12 September to Sunday 13 September, buses replace trains between Barrhead and \
        Kilmarnock / Carlisle. On Saturday during this period, buses operate between Kilmarnock and Troon, \
        where passengers can connect with trains to / from Ayr. No scheduled services operate between \
        Kilmarnock and Ayr / Stranraer on Sunday.";
```

- [ ] **Step 2: Add the `NORWOOD_JUNCTION_*` fixture**

Add immediately after:

```rust
    // Undated-aside observational fixture (design doc Decision 1): a
    // dated bus-replacement clause plus a separate, undated, vaguely-scoped
    // clause. Deliberately has NO dedicated hard-count expectation -- the
    // sibling research doc explicitly left "does this get its own period,
    // or fold into scope_description" unresolved; this fixture's job is to
    // observe what the improved prompt actually does, not assert a
    // pre-decided right answer.
    const NORWOOD_JUNCTION_SUMMARY: &str = "Buses replace trains via Norwood Junction";
    const NORWOOD_JUNCTION_DESCRIPTION: &str = "Monday to Thursday overnight, buses will replace trains \
        between the affected stations via Norwood Junction. Some trains will be diverted via an alternative \
        route.";
```

- [ ] **Step 3: Wire both fixtures into `live_eval_battery`**

Extend the function (currently lines 981-996):

```rust
    #[tokio::test]
    #[ignore = "requires network access to a real LLM_BASE_URL; run explicitly, see comment above"]
    async fn live_eval_battery() {
        let client = live_client_from_env();
        let repeats: u32 = std::env::var("LIVE_EVAL_REPEATS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);

        for attempt in 1..=repeats {
            run_battery_attempt(&client, "multi", attempt, WANDSWORTH_TOWN_SUMMARY, WANDSWORTH_TOWN_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "flat", attempt, FLAT_ETA_SUMMARY, FLAT_ETA_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "trap", attempt, TRAP_SUMMARY, TRAP_DESCRIPTION).await;
        }
        for attempt in 1..=repeats {
            run_battery_attempt(&client, "dow_legs", attempt, BARRHEAD_DUMFRIES_SUMMARY, BARRHEAD_DUMFRIES_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "undated_aside", attempt, NORWOOD_JUNCTION_SUMMARY, NORWOOD_JUNCTION_DESCRIPTION).await;
        }
    }
```

(`"dow_legs"` runs at the full `repeats` count, matching `"multi"` — this is the fixture Task 5's sizing decision leans on most; `"undated_aside"` runs at `repeats.min(2)`, matching the existing `"flat"`/`"trap"` convention for a fixture that isn't the primary segmentation-reliability signal.)

- [ ] **Step 4: Add the dedicated soft-assertion test for `BARRHEAD_DUMFRIES_*`**

Add alongside `live_eval_wandsworth_town_segments_into_two_periods` (after line 1044), mirroring its exact shape:

```rust
    #[tokio::test]
    #[ignore = "requires network access to a real LLM_BASE_URL; run explicitly, see comment above"]
    async fn live_eval_barrhead_dumfries_segments_into_six_periods() {
        let client = live_client_from_env();
        let reference_date = "2026-08-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        let start = std::time::Instant::now();
        let primary = client
            .extract_primary(BARRHEAD_DUMFRIES_SUMMARY, BARRHEAD_DUMFRIES_DESCRIPTION, reference_date)
            .await
            .expect("primary extraction should succeed against a real endpoint");
        eprintln!("primary call took {:?}, category={:?}, periods={}", start.elapsed(), primary.category, primary.periods.len());
        for (i, p) in primary.periods.iter().enumerate() {
            eprintln!(
                "  period[{i}]: scope={:?} date_range={:?} schedule_window={:?} resolution_status={:?} apparent_severity={:?}",
                p.scope_description, p.date_range, p.schedule_window, p.resolution_status, p.apparent_severity
            );
        }
        assert!(!primary.periods.is_empty(), "periods must never be empty on a successful parse");

        // Soft signal, not a hard assertion -- exactly like
        // live_eval_wandsworth_town_segments_into_two_periods above. The
        // expected count of 6 (three legs x two date ranges) is this
        // plan's own reasoned prediction, not an observed result; it has
        // not been run against a live model.
        if primary.periods.len() != 6 {
            eprintln!(
                "NOTE: expected 6 periods for the Barrhead-Dumfries fixture (three co-existing legs per \
                 date range, two date ranges), model produced {} -- see design doc Decision 1 and its Open \
                 Questions section (counts not yet validated against a live model)",
                primary.periods.len()
            );
        }
    }
```

Note: no dedicated test is added for `NORWOOD_JUNCTION_*` beyond its presence in `live_eval_battery`'s loop (Step 3) — per the design, this fixture is purely observational (its expected period count is an explicitly open question, not something this plan should assert on).

- [ ] **Step 5: Run the non-live-eval test suite to confirm nothing regressed**

Run: `cargo test -p enricher --lib llm::`
Expected: PASS. The four new/modified items are all `#[ignore]`d, so `cargo test` without `--ignored` does not execute any of them — this step only confirms the crate still compiles and every pre-existing test still passes.

- [ ] **Step 6: Commit**

```bash
git add crates/enricher/src/llm.rs
git commit -m "Add BARRHEAD_DUMFRIES_*/NORWOOD_JUNCTION_* live-eval fixtures for the day-of-week-across-legs case"
```

---

### Task 3: Truncate instead of failing — `extract_primary`'s over-cap branch

**Files:**
- Modify: `crates/enricher/src/llm.rs` (`PrimaryExtraction`, `extract_primary`, new helper functions, tests)
- Modify: `crates/enricher/src/combine.rs` (`severity_hint_rank`: private → `pub(crate)`)

**Interfaces:**
- Produces: `PrimaryExtraction.dropped_period_count: usize` (new field, `#[serde(default)]`, always 0 unless `extract_primary` truncated). `llm::select_periods_within_cap(periods: Vec<ExtractionPeriod>) -> Vec<ExtractionPeriod>` (new, private to `llm.rs`). `llm::effective_from_date(period: &ExtractionPeriod) -> Option<DateTime<Utc>>` (new, private to `llm.rs`). `combine::severity_hint_rank(hint: &str) -> u8` (visibility change only, same signature/behavior).
- Consumed by: Task 4 (`process_incident` reads `primary.dropped_period_count`).
- **Depends on:** nothing structurally (Axis 3 is independent of Axis 1) — sequenced after Task 1/2 in this plan only to avoid a same-file merge, per Global Constraints.

- [ ] **Step 1: Promote `severity_hint_rank` to `pub(crate)` in `combine.rs`**

Current (`combine.rs:36-42`):

```rust
fn severity_hint_rank(hint: &str) -> u8 {
    match hint {
        "severe_disruption" | "blocked_or_suspended" => 2,
        "moderate_disruption" => 1,
        _ => 0, // "normal", or any unrecognized value -- fail toward no escalation.
    }
}
```

Change the signature line only, and extend its doc comment (the existing comment at `combine.rs:31-35`) to note the new caller:

```rust
/// Ordering for `apparent_severity` values, most to least severe possible
/// escalation. `severe_disruption` and `blocked_or_suspended` rank equally
/// -- both map to `common::severity_rank`'s "severe" tier in
/// `aggregation::escalation_ceiling`, so neither is a milder read of the
/// other for the purpose of detecting disagreement here. `pub(crate)`
/// because `llm.rs`'s over-cap truncation (Decision 3,
/// docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md)
/// reuses this exact ordering to rank which periods survive the
/// `MAX_PERIODS` cap -- the same relative severity order applies to both
/// "does the adversarial pass disagree" and "which periods are most
/// consequential to keep," so this is shared, not duplicated.
pub(crate) fn severity_hint_rank(hint: &str) -> u8 {
    match hint {
        "severe_disruption" | "blocked_or_suspended" => 2,
        "moderate_disruption" => 1,
        _ => 0, // "normal", or any unrecognized value -- fail toward no escalation.
    }
}
```

- [ ] **Step 2: Add `dropped_period_count` to `PrimaryExtraction`**

Current (`llm.rs:88-92`):

```rust
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PrimaryExtraction {
    pub category: String,
    pub periods: Vec<ExtractionPeriod>,
}
```

New:

```rust
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PrimaryExtraction {
    pub category: String,
    pub periods: Vec<ExtractionPeriod>,
    /// How many periods `extract_primary` dropped to bring the response
    /// within `MAX_PERIODS`, if any. The model never sends this field --
    /// `#[serde(default)]` is load-bearing, the same precedent already
    /// established for `ExtractionPeriod.resolution_status_confidence`/
    /// `severity_confidence` (see that struct's own doc comment above).
    /// `extract_primary` always sets this explicitly after parsing
    /// (0 when under/at the cap); `process_incident` (`main.rs`) reads it
    /// to decide whether to log/count a truncation.
    #[serde(default)]
    pub dropped_period_count: usize,
}
```

- [ ] **Step 3: Add `effective_from_date` and `select_periods_within_cap`**

Add these two new private functions in `llm.rs`, near `build_period_user_content` (after line 500):

```rust
/// `None` (whether from a wholly absent `date_range`, or an explicit
/// `date_range.from_date: null`) sorts first in the truncation selection
/// below -- both already mean "treat as already active" per `DateRange`'s
/// own doc comment (this file, lines 18-19), the most urgent reading.
/// `Option<T>`'s derived `Ord` already puts `None` before `Some(_)`, so no
/// custom comparator is needed for that part.
fn effective_from_date(period: &ExtractionPeriod) -> Option<DateTime<Utc>> {
    period.date_range.as_ref().and_then(|range| range.from_date)
}

/// Keeps the `MAX_PERIODS` periods ranked highest by
/// `(severity_hint_rank(apparent_severity) descending, effective_from_date
/// ascending, None-first)` -- Decision 3 of
/// docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md.
/// `sort_by_key` is a stable sort, so periods tied on both keys keep the
/// model's own original relative order rather than being reordered
/// arbitrarily. Called only when `periods.len() > MAX_PERIODS`; a caller
/// passing an already-in-bounds list is a no-op that still runs the sort
/// (cheap for at most a handful of periods, and keeping the function
/// total rather than adding an unused early-return branch is simpler).
fn select_periods_within_cap(mut periods: Vec<ExtractionPeriod>) -> Vec<ExtractionPeriod> {
    periods.sort_by_key(|period| (std::cmp::Reverse(crate::combine::severity_hint_rank(&period.apparent_severity)), effective_from_date(period)));
    periods.truncate(MAX_PERIODS);
    periods
}
```

- [ ] **Step 4: Replace the `bail!` with truncate-and-succeed in `extract_primary`**

Current (`llm.rs:429-446`):

```rust
        let extraction: PrimaryExtraction = serde_json::from_str(&content)
            .map_err(|err| anyhow::anyhow!("primary extraction returned malformed JSON: {err}"))?;
        if extraction.periods.is_empty() {
            // Design §1: an empty `periods` array parses without a schema
            // error (no `minItems`), but recording it as a "successful"
            // extraction would permanently short-circuit `process_incident`'s
            // unchanged-text guard for this incident on every subsequent
            // sweep/reclaim pass. Treat it as a hard parse failure instead --
            // discarded, existing columns untouched, sweep retries later.
            anyhow::bail!("primary extraction returned an empty `periods` array");
        }
        if extraction.periods.len() > MAX_PERIODS {
            // Design §7 item 6: treat over-cap as a schema-adjacent
            // validation failure, discarded the same as any other malformed
            // response, rather than storing a hallucinated over-segmentation.
            anyhow::bail!("primary extraction returned {} periods, exceeding the soft cap of {MAX_PERIODS}", extraction.periods.len());
        }
        Ok(extraction)
```

New:

```rust
        let mut extraction: PrimaryExtraction = serde_json::from_str(&content)
            .map_err(|err| anyhow::anyhow!("primary extraction returned malformed JSON: {err}"))?;
        if extraction.periods.is_empty() {
            // Design §1: an empty `periods` array parses without a schema
            // error (no `minItems`), but recording it as a "successful"
            // extraction would permanently short-circuit `process_incident`'s
            // unchanged-text guard for this incident on every subsequent
            // sweep/reclaim pass. Treat it as a hard parse failure instead --
            // discarded, existing columns untouched, sweep retries later.
            anyhow::bail!("primary extraction returned an empty `periods` array");
        }
        // Decision 3 of docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md:
        // an over-cap response used to be a hard failure here (discarded,
        // sweep retries forever, all NLP-derived severity signal lost for
        // this incident). Instead, keep the MAX_PERIODS most-severe/soonest
        // periods and let extraction succeed -- `dropped_period_count`
        // records how many were cut, so `process_incident` (main.rs) can
        // log/count it without any downstream step (extract_adversarial,
        // extract_severity_adversarial, combine::combine_periods,
        // queries::write_extraction) needing to know anything unusual
        // happened; they only ever see an already-in-bounds `periods` list.
        let original_count = extraction.periods.len();
        if original_count > MAX_PERIODS {
            extraction.periods = select_periods_within_cap(extraction.periods);
        }
        extraction.dropped_period_count = original_count.saturating_sub(MAX_PERIODS);
        Ok(extraction)
```

- [ ] **Step 5: Rewrite `extract_primary_rejects_periods_beyond_the_soft_cap`**

Current (`llm.rs:641-669`) asserts `.is_err()`. Rename and rewrite to assert the new truncate-and-succeed behavior, with an ordering assertion (not just a length check — the ordering guarantee is the part actually worth pinning, per the design's Testing section):

```rust
    #[tokio::test]
    async fn extract_primary_truncates_periods_beyond_the_soft_cap() {
        let server = MockServer::start().await;
        // 13 periods against a cap of 8 -- the same "13 vs 8" shape the
        // design doc's own root-cause research called out as more
        // consistent with real compound incident structure than runaway
        // hallucination. 8 are rank-2 severity (blocked_or_suspended /
        // severe_disruption, tied), 5 are rank-0 (normal) -- distinct
        // severities per period, so this test isolates severity ordering
        // without also exercising the date tiebreak (that's the dedicated
        // test in Step 6 below).
        let severities = [
            "blocked_or_suspended", "severe_disruption", "blocked_or_suspended", "severe_disruption",
            "blocked_or_suspended", "severe_disruption", "blocked_or_suspended", "severe_disruption",
            "normal", "normal", "normal", "normal", "normal",
        ];
        let periods: Vec<serde_json::Value> = severities
            .iter()
            .enumerate()
            .map(|(i, severity)| {
                serde_json::json!({
                    "scope_description": format!("p{i}"),
                    "date_range": null,
                    "schedule_window": null,
                    "resolution_status": "ongoing",
                    "apparent_severity": severity,
                })
            })
            .collect();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({ "category": "signal_failure", "periods": periods }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string(), DEFAULT_REQUEST_TIMEOUT);
        let result = client.extract_primary("Signal failure", "Delays", reference_date()).await;

        let extraction = result.expect("exceeding the soft cap must now truncate and succeed, not fail");
        assert_eq!(extraction.periods.len(), 8);
        assert_eq!(extraction.dropped_period_count, 5);
        let kept: Vec<&str> = extraction.periods.iter().map(|p| p.scope_description.as_deref().unwrap()).collect();
        assert_eq!(
            kept,
            vec!["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"],
            "the 8 rank-2-severity periods must be kept in their original relative order (stable sort, no date tiebreak triggered here); the 5 rank-0 (normal) periods must be dropped"
        );
    }
```

- [ ] **Step 6: Add the dedicated date-tiebreak test**

Add alongside the test from Step 5:

```rust
    #[tokio::test]
    async fn extract_primary_truncation_tiebreaks_by_from_date_ascending_with_none_first() {
        let server = MockServer::start().await;
        // 7 filler periods at blocked_or_suspended (rank 2), spread across
        // distinct dates so none of them tie with each other -- guaranteed
        // to be kept regardless of the two candidates below.
        let mut periods: Vec<serde_json::Value> = (0..7)
            .map(|i| {
                serde_json::json!({
                    "scope_description": format!("filler{i}"),
                    "date_range": { "from_date": format!("2026-0{}-01T00:00:00Z", i + 1), "to_date": null },
                    "schedule_window": null,
                    "resolution_status": "ongoing",
                    "apparent_severity": "blocked_or_suspended",
                })
            })
            .collect();
        // 2 candidates at moderate_disruption (rank 1, strictly below the
        // fillers' rank 2) competing for the single remaining slot: one
        // with from_date: null, one with a stated future date. Per the
        // "None sorts first" rule, the null one must be kept.
        periods.push(serde_json::json!({
            "scope_description": "candidate_none_date",
            "date_range": { "from_date": null, "to_date": null },
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "moderate_disruption",
        }));
        periods.push(serde_json::json!({
            "scope_description": "candidate_some_date",
            "date_range": { "from_date": "2026-12-01T00:00:00Z", "to_date": null },
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "moderate_disruption",
        }));

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({ "category": "signal_failure", "periods": periods }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string(), DEFAULT_REQUEST_TIMEOUT);
        let extraction = client
            .extract_primary("Signal failure", "Delays", reference_date())
            .await
            .expect("9 periods against a cap of 8 must truncate and succeed");

        assert_eq!(extraction.periods.len(), 8);
        assert_eq!(extraction.dropped_period_count, 1);
        let kept: Vec<&str> = extraction.periods.iter().map(|p| p.scope_description.as_deref().unwrap()).collect();
        assert!(kept.contains(&"candidate_none_date"), "the null-from_date candidate must win the tiebreak: {kept:?}");
        assert!(!kept.contains(&"candidate_some_date"), "the dated candidate must lose the tiebreak: {kept:?}");
    }
```

- [ ] **Step 7: Extend `extract_primary_accepts_periods_exactly_at_the_soft_cap`**

Current (`llm.rs:672-700`) only asserts `.is_ok()`. Add, at the end of the existing test body:

```rust
        let extraction = result.expect("exactly MAX_PERIODS should still be accepted, only exceeding it truncates");
        assert_eq!(extraction.periods.len(), MAX_PERIODS);
        assert_eq!(extraction.dropped_period_count, 0, "the boundary case must not report any truncation");
```

(This replaces the existing bare `assert!(result.is_ok(), ...)` line — keep the test name unchanged, since this is the boundary regression check the design's Testing section calls out by name.)

- [ ] **Step 8: Run the tests**

Run: `cargo test -p enricher --lib llm:: combine::`
Expected: PASS. All existing tests plus the new/rewritten ones from Steps 5-7.

- [ ] **Step 9: Commit**

```bash
git add crates/enricher/src/llm.rs crates/enricher/src/combine.rs
git commit -m "Truncate to the MAX_PERIODS most-severe/soonest periods instead of failing on cap breach"
```

---

### Task 4: Operator visibility + the crux regression test (truncation actually closes the retry loop)

**Files:**
- Modify: `crates/enricher/src/main.rs` (`process_incident`, new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `enricher_period_truncations_total` counter (via `common::metrics::metric_name`, no labels). New `tracing::warn!` at the truncation call site. New `#[cfg(test)] mod tests` in `main.rs` (this crate has none today).
- Consumed by: nothing downstream in this codebase (an operator/alert rule, per the design) — this is the last code-change task.
- **Depends on:** Task 3 (`primary.dropped_period_count` must exist).

Per Decision 3: a truncation event is a one-shot, bounded, "succeeded but with partial data loss" event — a `counter!`, not a `gauge!` like `enricher_mismatch_incidents` (there is no "currently outstanding" set to gauge, since the incident's write succeeds and its `source_text_hash`/`extraction_model_version` advance normally). `tracing::warn!`, not `tracing::error!`, deliberately distinguishing this from `MismatchTracker`'s escalated `tracing::error!` case (a truncation is not the indefinite total failure a persistent combine mismatch represents).

- [ ] **Step 1: Add the truncation branch to `process_incident`**

Current (`main.rs:285-291`):

```rust
    let primary = match primary_result {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(error = ?err, incident_id, "primary extraction failed");
            return false;
        }
    };
```

New — insert the truncation check immediately after this block, before `resolution_adversarial_start` (currently line 293):

```rust
    let primary = match primary_result {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(error = ?err, incident_id, "primary extraction failed");
            return false;
        }
    };

    // Decision 3 of docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md:
    // a truncated primary extraction is NOT an error -- it already
    // succeeded, and the pipeline below continues completely unaware
    // anything unusual happened (extract_adversarial/
    // extract_severity_adversarial/combine::combine_periods/
    // write_extraction all just see an already-in-bounds `periods` list).
    // This is purely operator-facing visibility: a counter for an alert
    // rule to fire on, and a human-readable log line alongside it -- the
    // same split MismatchTracker already uses (gauge for the alertable
    // signal there, tracing::error! for the human-readable why), except a
    // counter (not a gauge, no "currently outstanding" set to track) and
    // tracing::warn! (not tracing::error!, since this run still succeeds
    // and writes normally, unlike a persistent combine mismatch).
    if primary.dropped_period_count > 0 {
        tracing::warn!(
            incident_id,
            original_count = primary.periods.len() + primary.dropped_period_count,
            kept_count = primary.periods.len(),
            "primary extraction exceeded the period cap; truncated to the N most severe/soonest periods"
        );
        metrics::counter!(common::metrics::metric_name("enricher_period_truncations_total")).increment(1);
    }
```

- [ ] **Step 2: Add the live-DB crux test**

This is the assertion the whole remediation exists to satisfy: a truncated incident's `source_text_hash`/`extraction_model_version` now advance, so `sweep::incidents_needing_extraction` stops re-selecting it — closing the retry-forever loop. `crates/enricher` has no existing `#[cfg(test)] mod tests` in `main.rs`; add one at the end of the file, following `crates/api`'s established `db_tests` convention (`crates/api/src/data/queries.rs:1120-1145`'s `test_pool()`/seed/assert/cleanup shape) since this crate has no precedent of its own to extend:

```rust
#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn test_pool() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    /// The full crux of this plan: a primary extraction that exceeds
    /// MAX_PERIODS must now (a) still write successfully, and (b) leave
    /// `source_text_hash`/`extraction_model_version` matching the current
    /// text/version -- proving `sweep::incidents_needing_extraction` will
    /// NOT re-select this incident on its next tick (it re-selects only on
    /// a hash or version mismatch, `sweep.rs:27-35`). Before Decision 3,
    /// this incident would fail at `extract_primary` and neither column
    /// would ever be written, reproducing the retry-forever bug this test
    /// exists to close. Mocks all three LLM calls against one wiremock
    /// server, distinguished by each request's `response_format.json_schema.name`
    /// (`"incident_extraction"` / `"adversarial_resolution_check"` /
    /// `"adversarial_severity_check"`, matching PRIMARY_SCHEMA_NAME/
    /// ADVERSARIAL_SCHEMA_NAME/SEVERITY_ADVERSARIAL_SCHEMA_NAME in llm.rs)
    /// so the primary call can return more than MAX_PERIODS periods while
    /// the two adversarial calls return exactly MAX_PERIODS verdicts each
    /// -- matching what extract_primary's own truncation guarantees
    /// process_incident will actually send them.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p enricher process_incident -- --ignored`"]
    async fn process_incident_writes_successfully_and_advances_hash_and_version_when_primary_extraction_is_truncated() {
        let pool = test_pool().await;
        let incident_id = "TEST-ENRICHER-TRUNCATION-1";
        let summary = "Test incident exceeding the period cap";
        let description = "Thirteen distinct facts reported across this incident's lifetime.";

        sqlx::query(
            "INSERT INTO incidents (incident_id, summary, description, operators, affected_stations, priority) \
             VALUES ($1, $2, $3, '{}', '{}', 3) \
             ON CONFLICT (incident_id) DO UPDATE SET summary = EXCLUDED.summary, description = EXCLUDED.description, \
                 source_text_hash = NULL, extraction_model_version = NULL, extracted_periods = NULL",
        )
        .bind(incident_id)
        .bind(summary)
        .bind(description)
        .execute(&pool)
        .await
        .expect("seed fixture incident row");

        let server = MockServer::start().await;
        let over_cap_periods: Vec<serde_json::Value> = (0..(MAX_PERIODS_FOR_TEST + 3))
            .map(|i| {
                serde_json::json!({
                    "scope_description": format!("p{i}"),
                    "date_range": null,
                    "schedule_window": null,
                    "resolution_status": "ongoing",
                    "apparent_severity": "moderate_disruption",
                })
            })
            .collect();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("incident_extraction"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": serde_json::json!({ "category": "signal_failure", "periods": over_cap_periods }).to_string() } }]
            })))
            .mount(&server)
            .await;
        let kept_verdicts: Vec<serde_json::Value> = (0..MAX_PERIODS_FOR_TEST)
            .map(|i| serde_json::json!({ "period_index": i, "scope_description": format!("p{i}"), "resolution_status": "ongoing" }))
            .collect();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("adversarial_resolution_check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": serde_json::json!({ "periods": kept_verdicts }).to_string() } }]
            })))
            .mount(&server)
            .await;
        let kept_severity_verdicts: Vec<serde_json::Value> = (0..MAX_PERIODS_FOR_TEST)
            .map(|i| serde_json::json!({ "period_index": i, "scope_description": format!("p{i}"), "apparent_severity": "moderate_disruption" }))
            .collect();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("adversarial_severity_check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": serde_json::json!({ "periods": kept_severity_verdicts }).to_string() } }]
            })))
            .mount(&server)
            .await;

        let llm = LlmClient::new(server.uri(), None, "test-model".to_string(), Duration::from_secs(30));
        let model_version = "test-model@periods-v1";
        let mismatch_tracker = MismatchTracker::default();

        let ok = process_incident(&pool, &llm, model_version, incident_id, &mismatch_tracker).await;
        assert!(ok, "a truncated-but-successful extraction must return true (ack the entry), not false");

        let row: (Option<String>, Option<String>, Option<serde_json::Value>) = sqlx::query_as(
            "SELECT source_text_hash, extraction_model_version, extracted_periods FROM incidents WHERE incident_id = $1",
        )
        .bind(incident_id)
        .fetch_one(&pool)
        .await
        .expect("fetch written row");
        let expected_hash = hash::text_hash(summary, description);
        assert_eq!(row.0.as_deref(), Some(expected_hash.as_str()), "source_text_hash must advance even though the extraction was truncated");
        assert_eq!(row.1.as_deref(), Some(model_version), "extraction_model_version must advance even though the extraction was truncated");
        let periods = row.2.expect("extracted_periods must be written");
        assert_eq!(periods.as_array().expect("periods is an array").len(), MAX_PERIODS_FOR_TEST, "the written periods must be the truncated (in-cap) set, not the original over-cap one");

        // The actual retry-forever-loop-is-closed assertion: re-running
        // sweep::incidents_needing_extraction's own comparison against
        // what was just written must NOT re-select this incident.
        let current_hash = hash::text_hash(summary, description);
        assert!(
            row.0.as_deref() == Some(current_hash.as_str()) && row.1.as_deref() == Some(model_version),
            "this incident must no longer match sweep::incidents_needing_extraction's re-select condition (sweep.rs:27-35)"
        );

        sqlx::query("DELETE FROM incidents WHERE incident_id = $1").bind(incident_id).execute(&pool).await.expect("cleanup");
    }

    // `MAX_PERIODS` itself is private to `llm.rs`; this local alias avoids
    // either making it pub(crate) just for a test fixture or hardcoding
    // the literal `8` twice in a way that would silently desync if
    // Task 5's Axis 2 process ever changes the real constant. Update this
    // alongside `llm::MAX_PERIODS` if that ever happens.
    const MAX_PERIODS_FOR_TEST: usize = 8;
}
```

**Why the counter/log aren't independently asserted:** this workspace has no `metrics-util`/`tracing-test`-equivalent dev-dependency anywhere (grep-confirmed), and `common::metrics`'s own doc comment states its macros are silent no-ops against no installed recorder (`crates/common/src/metrics.rs`) — there is no existing convention in this codebase for asserting a specific counter value or log line fired (`record_llm_call_metrics`, the closest precedent, has no dedicated test either). Adding one would be a new dependency, which Global Constraints rules out. The test above instead asserts the *behavioral* guarantee the counter/log exist to make visible — a successful, non-erroring, hash/version-advancing write — which is the part that actually matters; the `if primary.dropped_period_count > 0` branch containing the `tracing::warn!`/`counter!` calls is still exercised by this same test path (it compiles and runs), just not independently value-asserted.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p enricher --lib main::` (Note: the new test is `#[ignore]`d and requires `DATABASE_URL`; run it explicitly with `cargo test -p enricher process_incident -- --ignored`, requires a running Postgres with this workspace's migrations applied.)
Expected: PASS for the non-ignored suite; the ignored test PASS when run explicitly against a live database.

- [ ] **Step 4: Commit**

```bash
git add crates/enricher/src/main.rs
git commit -m "Add enricher_period_truncations_total counter/warn and the crux hash/version-advance regression test"
```

---

### Task 5: Axis 2 — process for choosing the new `MAX_PERIODS` value (follow-on, not executable in this pass)

**Files:**
- Modify: `crates/enricher/src/llm.rs` (`MAX_PERIODS`'s doc comment, lines 168-179) — **only once the process below has actually been run**; this task's checkboxes are process steps, not something to check off speculatively.

**Interfaces:**
- Produces: nothing new — this task ends with `MAX_PERIODS`'s existing value (still `8`) or a new one, plus an updated doc-comment justification. Per Global Constraints, this plan does not choose that value.
- Consumed by: nothing else in this plan.
- **Depends on:** Task 1 + Task 2 (needs the new `BARRHEAD_DUMFRIES_*`/`NORWOOD_JUNCTION_*` fixtures to exist and the improved `PRIMARY_PROMPT` to be what's being measured). Independent of Task 3/4.

**This task cannot be executed in this environment** — it requires a live `LLM_BASE_URL` (no network/LLM access here, same constraint the design and its research doc both already disclosed) and, for its optional step 2, access to real incident text this session/environment does not have (the local dev DB's `incidents` table has zero rows). It is included as an explicit task, per Decision 2, so this axis is scheduled and trackable rather than silently dropped — not to be skipped, and not to be collapsed into Task 1-4's scope by inventing a number now.

- [ ] **Step 1: Run the extended live-eval battery against the deployed model**

```bash
LLM_BASE_URL=... LLM_API_KEY=... LLM_MODEL=... LIVE_EVAL_REPEATS=3 \
  cargo test -p enricher --lib llm::tests::live_eval_battery -- --ignored --nocapture
```

Record every `BATTERY fixture=... attempt=... status=... period_count=...` log line (`llm.rs`'s `run_battery_attempt`, greppable by design) across all five fixtures (`multi`, `flat`, `trap`, `dow_legs`, `undated_aside`) and all `LIVE_EVAL_REPEATS` attempts.

- [ ] **Step 2 (conditional): If real historically-failing incident text has become retrievable by this point** (production log/DB access this design pass and this plan's own writing did not have — check again before running this step, don't assume it's still unavailable), run the same battery against that real text instead of, or in addition to, the synthetic fixtures. Real incidents are strictly better evidence than modeled worst cases.

- [ ] **Step 3: Take the resulting period-count distribution and propose a new `MAX_PERIODS` value**

Using the `dow_legs` fixture's results (plus any real incidents of similar shape from Step 2) as the hardest-case data point: propose `MAX_PERIODS` at a measured high percentile with explicit headroom over the highest observed count for that fixture — a concrete starting target is +50% over the highest observed count, rounded up, mirroring how the original `8` already sat 4x the 2-period Wandsworth Town fixture (`llm.rs:172-178`'s existing doc comment). This is a proposal to review, not an automatic decision — do not commit a new value without a human sign-off on the resulting number, since this is a production-behavior-changing constant.

- [ ] **Step 4: Re-run the full battery once more at the chosen value**

Confirm the *existing* three fixtures (`multi`/`flat`/`trap`) still land at their expected 2/1/1 periods respectively — Task 1's prompt changes touch shared guidance text that could in principle affect segmentation on unrelated fixtures, so this is a regression check, not just a sizing exercise.

- [ ] **Step 5: Record the chosen value's justification in `MAX_PERIODS`'s own doc comment**

Replace the current justification (`llm.rs:168-178`, "8 is chosen as generous headroom over every motivating example in the design doc... the Wandsworth Town fixture has 2; the design's own soft-cap sanity-check fixture is described as '3+'") with the new one, citing the actual measured `dow_legs`/real-incident data from Steps 1-2 and the headroom multiplier chosen in Step 3 — replacing the stale justification, not leaving both.

- [ ] **Step 6: Run the full non-live-eval suite one more time, then commit**

```bash
cargo test -p enricher
git add crates/enricher/src/llm.rs
git commit -m "Raise MAX_PERIODS to <value> based on the extended live-eval battery (see updated doc comment for data)"
```

(This commit message is illustrative — it cannot be written for real until Steps 1-5 above have actually run against a live model.)

---

### Task 6: Final verification

**Files:** none (verification only).

**Interfaces:** none.
**Depends on:** Tasks 1-4 (Task 5 is explicitly not executable in this pass, per its own note, and does not gate this task).

- [ ] **Step 1: Run the full `enricher` test suite**

```bash
cargo test -p enricher
```

Expected: PASS. This includes every existing test (untouched), Task 1/2's `#[ignore]`d prompt/fixture additions (compiled but not run), Task 3's rewritten/new `llm.rs` cap tests (run and passing), and Task 4's new `main.rs` `#[cfg(test)] mod tests` module (compiles; its one test is `#[ignore]`d pending a live database, per Global Constraints' testing convention).

- [ ] **Step 2: Confirm the existing eval battery's three original fixtures are unchanged**

`cargo test -p enricher` does not run `#[ignore]`d tests, so this is a read-only confirmation: re-open `crates/enricher/src/llm.rs` and diff Task 2's edits against `git show <Task-1-commit>:crates/enricher/src/llm.rs` (or equivalent) to confirm `WANDSWORTH_TOWN_*`/`FLAT_ETA_*`/`TRAP_*` and their existing dedicated tests (`live_eval_wandsworth_town_segments_into_two_periods`, `live_eval_flat_single_fact_incident_stays_one_period`) were only ever appended around, never edited — per Task 2's own scope (Step 1-4 only ever add new consts/match arms/tests, never touch the three existing fixture consts).

- [ ] **Step 3: Workspace sanity build**

`enricher` is a leaf binary crate — grep-confirmed (`grep -rln "enricher" crates/*/Cargo.toml`) that no other workspace crate depends on it, so no other crate's tests are affected by this plan. Run a light build-only sanity check rather than a full workspace test run (which would need every other crate's own live-DB/live-network fixtures, out of scope for this plan's verification):

```bash
cargo build --workspace
```

Expected: PASS, confirming this plan's changes didn't somehow break the workspace `Cargo.lock`/dependency graph.

- [ ] **Step 4: Report**

No commit for this task (verification-only) — if Steps 1-3 all pass, the plan is complete through Task 4, with Task 5 tracked as an explicit, separate follow-on pending live LLM access.
