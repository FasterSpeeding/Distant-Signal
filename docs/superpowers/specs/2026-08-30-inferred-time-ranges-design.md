# Design: Genuine Inferred Time Ranges for LDBWS-Derived Line Statuses

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(the closest structural precedent — a scoped, code-verified frontend/backend
design doc, not a research-only survey). No implementation plan is included;
that is a separate, later step in this repo's process.

**Scope correction up front, because the brief that prompted this research
assumed something broader than what's actually there.** "Integrate inferred
date and time ranges into the frontend" reads like a frontend task. It
isn't, or at least not mostly. The frontend (`frontend/lib/validity.ts`,
`frontend/components/IssueList.tsx`, `frontend/lib/dateFormat.ts`) already
renders `ValidityPeriod.fromDate`/`toDate`/`isNow` correctly and completely
for every status shape the backend currently produces — active/upcoming/
ended bucketing, "From 10 May 2026" / "10 May 2026 – ongoing" / date-range
summaries, all real, working code, not something this spec needs to add.
**What's missing is entirely on the backend**: for LDBWS-sample-derived
statuses (the majority of non-incident line statuses this app renders —
`DataQuality::LdbwsInferred`), `ValidityPeriod.from_date` is not a genuine
"observed since" time at all. It is `Utc::now()`, re-stamped fresh on
*every single aggregation cycle*, for as long as the same disruption
persists. The frontend faithfully renders this fiction as if it were real —
which is the actual bug this spec fixes, not a missing rendering feature.

## What was researched, and why this isn't the TRUST-line-level project

This session inherited a substantial, already-committed body of research —
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`,
its `-timetable-verification.md` follow-up, and
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
(the executed validation pass, blocked on a broken SSO redirect, verdict
**"not yet"**) — investigating whether `crates/trust-consumer`'s real-time
TRUST feed could be correlated against CIF schedule data to produce
segment-level, full-population delay inference for line status, as a
richer alternative to today's 3-5-sample-station LDBWS polling
(`infer_from_samples`). Read in full before writing this doc, since the
brief specifically warned this ground might already be covered.

**That research is not superseded or duplicated here — it answers a
different question.** It's about *severity/segment accuracy*: could TRUST
give a truer picture of *which stations* are affected and *how bad* the
delay is, versus a coarse line-wide sample. Its own verdict was measured
and honest: the coverage/segment-precision case is real, the delay-minute-
accuracy case is weak (Darwin, which already backs `ldbws-inferred`,
already fuses TRUST into something richer than a homegrown diff would
produce — see `crates/api/src/data/eta_blend.rs`'s "prefer Darwin, TRUST is
the honest floor" precedent, cited directly in that research), and the
concrete recommendation was **not to build it yet**, pending a real
empirical validation run that never completed (SSO blocker,
`2026-08-29-trust-schedule-delay-validation-findings.md`'s Task 4). None of
that changes here. `DataQuality::TrustInferred` remains unused
(`crates/common/src/lib.rs:275`) and this doc does not propose wiring it up.

**This doc is about a narrower, orthogonal, much cheaper problem: *when did
the currently-reported status begin*, for the statuses this app already
computes today** — not whether a richer data source could compute a better
status in the first place. It requires no new data feed, no CIF, no wider
TRUST scope, and no `trust-consumer` change at all. It's a bug in how an
already-existing field is populated, discovered by reading
`crates/aggregator/src/queries.rs`'s own `normalize_for_diff` doc comment,
which already names the problem precisely (quoted below) without anyone
having closed the loop on it.

## The bug, verified against real code

`common::ValidityPeriod` (`crates/common/src/lib.rs:286-290`) has
`from_date: DateTime<Utc>`. Three call sites in
`crates/aggregator/src/aggregation.rs` construct one:

1. **`validity_for_output` (line 158-168), empty-periods fallback (line
   160)**: `ValidityPeriod { from_date: Utc::now(), to_date: None, is_now:
   true }` — used only when an incident has no `validity` data at all
   (rare; Knowledgebase incidents normally carry real NRE-published
   start/end times, which flow through untouched via the non-empty branch).
2. **`infer_from_samples` (line 774)**: same literal `Utc::now()` stamp,
   for every `DataQuality::LdbwsInferred` status this function produces —
   the **primary, high-volume case**, hit every aggregation cycle for
   every line with no active incident (which per the real 8-day validation
   window in `2026-08-29-trust-schedule-delay-validation-findings.md`'s
   Task 2, was ~15% of WCML's recomputes and ~0% of SWR-Alton's in that
   particular window — real but a minority there, not negligible
   elsewhere, and the *only* signal at all for lines with no
   Knowledgebase coverage).
3. **`good_service()` (line 886)**: same stamp, for the "line is fine"
   fallback. Lower stakes — `frontend/components/IssueList.tsx` never
   surfaces Good-Service statuses as issues (`isGoodSeverity` gates them
   out into the "all clear" empty state), so this from_date is presently
   inert for end users, but it's the same bug and worth fixing for
   consistency/any future consumer.

**The aggregator re-runs on every poll cycle** (DESIGN.md's stated 30-60s
LDBWS cadence). Each cycle that recomputes an `LdbwsInferred` status for a
line calls `infer_from_samples` fresh, which calls `Utc::now()` fresh. A
disruption that has genuinely been ongoing for six hours is re-stamped as
"started right now" on every single one of those cycles — not a one-time
initialization artifact, a *permanent, continuous* misrepresentation for as
long as the disruption lasts.

**The codebase already knows this is wrong, in its own words.**
`crates/aggregator/src/queries.rs:138-161`, `normalize_for_diff`'s doc
comment:

> `validity.from_date`: the no-incident/no-inference fallback paths
> (`good_service()`, the LDBWS-inferred branch of `infer_from_samples`,
> and `validity_for_output`'s empty-periods case) stamp this with a fresh
> `Utc::now()` on every call. Incident-driven statuses are unaffected:
> their `from_date` comes from the incident's own stored
> `validity_periods` and stays stable across cycles as long as the
> incident data doesn't change.

`normalize_for_diff` exists *specifically* to strip `from_date` (along
with `sample_stats` and the live-sample-annotation reason suffix) before
comparing old vs. new `statuses` JSON, so that this churn doesn't spuriously
trigger a `line_status_history` row on every cycle
(`write_line_status`, same file, lines 195-238). **The fix for the
*symptom* (spurious history rows) has already been built. The fix for the
*field itself* (making `from_date` mean something) has not.**

## User-visible consequence, confirmed against real rendering code

`frontend/components/IssueList.tsx:53-58`:

```ts
function formatFullValidity(status: LineStatus, now: number): string {
  const period = governingPeriod(status, now);
  if (!period) return '';
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}
```

rendered at line 381 as `Valid: {formatFullValidity(status, now)}` inside
every expanded issue's accordion panel. For an `LdbwsInferred` status, this
literally prints something like **"Valid: 30 Aug 2026, 14:32 – ongoing"**
where the timestamp is whichever poll cycle most recently ran before the
page was rendered — visibly different (to the minute) on every page
refresh, for a disruption that may have started hours earlier. This is
already live, user-facing, and wrong today; it isn't a missing feature, it's
an existing minor deception. (The collapsed-row summary,
`formatValiditySummary`, is *not* similarly broken — it checks
`periodIsActive` first and prints the flat string `"Now"` for any active
period rather than a timestamp, so the worst of this is currently hidden
from the collapsed view and only surfaces on expand.)

**A second, independent correctness bug shares the same root cause.**
`frontend/lib/stationIssues.ts:21-26`, `statusKey` — the identity function
`dedupeStationIssues` uses to merge the same disruption's appearance across
multiple lines/statuses at a station page:

```ts
export function statusKey(status: LineStatus): string {
  return [
    status.statusSeverity,
    status.dataQuality,
    status.reason,
    status.validityPeriods.map((p) => `${p.fromDate}/${p.toDate ?? ''}/${p.isNow}`).join(';'),
  ].join(' ');
}
```

`fromDate` is part of the dedup key. `infer_from_samples` is called
separately, once per affected line, inside `aggregate()`'s per-line loop
(`crates/aggregator/src/aggregation.rs:89-95`), and each call makes its own
independent `Utc::now()` call — not a single shared timestamp reused across
lines in one aggregation pass. Two lines genuinely sharing one operator-wide
LDBWS-detected delay pattern (a real, plausible case — an operator running
several lines through a shared trunk) get **two different microsecond-
precision `fromDate` values**, which never dedupe under this key. A station
served by both lines would show the same underlying disruption as two
separate cards instead of one merged one. This is a real, currently-live
correctness bug, independently discovered while researching this feature,
not a hypothetical this doc invents to justify itself — and it is fixed by
the same backend change proposed below, for free.

## The fix: carry forward `from_date` across cycles when the underlying status hasn't materially changed

**The mechanism this needs already exists, one call away from being reused
for this.** `write_line_status` (`crates/aggregator/src/queries.rs:198-238`)
already fetches the previous cycle's stored `statuses` JSON via
`existing_statuses` (line 130-136), purely to decide *whether* to insert a
`line_status_history` row. The comparison it performs —
`normalize_for_diff(&existing) != normalize_for_diff(&statuses_json)` —
already defines, precisely, "is this the same status as last cycle, modulo
the fields that churn every cycle regardless." That's exactly the
condition under which `from_date` should be **carried forward from the
existing row instead of overwritten**, not just used to suppress a history
insert.

**Proposed change, scoped entirely to `crates/aggregator/src/queries.rs`
(plus a small, optional tweak inside `aggregation.rs` — see Open question
1):**

1. In `write_line_status`, before comparing, walk the new `statuses_json`
   array. For each entry whose `dataQuality` is `"ldbws-inferred"` (the
   only data quality actually affected — see below), look up the
   positionally-corresponding entry in the *existing* stored array (see
   Open question 2 on why positional matching is safe today but not
   forever).
2. If that existing entry also has `dataQuality == "ldbws-inferred"` **and**
   the two entries are equal under the same `normalize_for_diff`-style
   per-entry stripping already defined (severity, reason minus the live-
   sample-annotation suffix, disruption fields — i.e., "this is the same
   underlying disruption, just a fresh poll of it") — overwrite the *new*
   entry's `validity.from_date` with the *existing* entry's `validity.
   from_date` before serializing.
3. Otherwise (no existing entry, existing entry has a different data
   quality, or the content differs after stripping) — leave the fresh
   `Utc::now()` stamp as-is. This is the correct behavior for a status that
   is genuinely new or has genuinely changed (severity moved, a different
   "most cited" reason took over, an incident replaced/was replaced by
   LDBWS inference).
4. Everything downstream — the `changed` diff, the `line_status_history`
   insert, the actual `line_status` upsert — proceeds exactly as today,
   just now writing a `from_date` that means "first observed, this
   specific status" rather than "most recent poll."

This is deliberately **not** "track a separate `first_seen_at` column the
way `incidents` already does" (`crates/api/src/data/queries.rs`'s
`first_seen_at`, used for stale-incident rail-day-boundary detection,
`crates/aggregator/src/aggregation.rs:170-223`) — that precedent is real
and worth citing as prior art for "this codebase already persists
first-observed timestamps across cycles for a different purpose," but
`line_status` has no natural place to persist a *value that changes* across
statuses without a schema change, whereas the carry-forward approach reuses
data already sitting in the same JSON column, no migration required.

**Why "same content" must mean "content minus the same fields
`normalize_for_diff` already strips," not "byte-identical status objects":**
`sample_stats` and the `(live samples show: ...)` annotation churn every
cycle even when nothing meaningful changed (that's the whole reason
`normalize_for_diff` exists). If the carry-forward comparison didn't use
the same stripped view, `from_date` would reset every cycle anyway for a
different reason — sample counts ticking over — defeating the fix in a way
that would be easy to miss in review (it would still "work" in a quick
manual test where sample stats happen to be stable between two consecutive
polls, and only show up as broken under sustained observation).

## Data quality scope: `ldbws-inferred` only, and why that's correct, not incomplete

- **`Knowledgebase`/`Planned`** statuses already have genuine `from_date`
  values, sourced from the incident's own NRE-published `validity_periods`
  via `validity_for_output`'s non-empty branch — untouched by this bug,
  untouched by this fix.
- **`Tfl`** statuses are written by a separate ingest path
  (`crates/api/src/data/queries.rs`'s `upsert_tfl_line_status`), not
  `crates/aggregator` at all — out of scope for this change, and not
  investigated further here since the brief's leads didn't point at it and
  nothing in this pass suggested it shares the bug.
- **`TrustInferred`** is not constructed anywhere in this codebase today
  (confirmed by the repo-wide grep this session ran, matching the brief's
  own lead 3) — nothing to fix.
- **`LdbwsInferred`** is the entire affected surface: `infer_from_samples`'s
  main branch and `good_service()`'s fallback. Both get the fix, since both
  go through the same `write_line_status` code path and the same
  `normalize_for_diff`-shaped comparison applies to both (a "Good Service"
  entry's stripped content is just its severity/reason, which never
  actually changes for that fixed string — so in practice `good_service()`
  entries will always carry-forward once written once, which is harmless
  and arguably more correct than today's constant restamping, even though
  no current UI surfaces it).

## Frontend changes: none required, one small polish item

Per the scope correction above, `frontend/lib/validity.ts` and
`IssueList.tsx`'s rendering already do the right thing once `from_date` is
honest — `periodIsActive`, `bucketFor`, `governingPeriod`,
`formatValiditySummary`, `formatFullValidity` all already read `fromDate`/
`toDate`/`isNow` exactly as this fix would populate them, with no schema
change to `ValidityPeriod`/`common::LineStatus`. **No new frontend type,
route, or component is needed for the backend fix to reach users.**

One optional, small polish item, not required to ship the fix:
**`formatFullValidity` currently phrases every governing period identically
regardless of `dataQuality`** — "10 May 2026, 08:00 – ongoing" reads with
the same confidence whether it came from NRE's own published incident times
(`Knowledgebase`) or from this app's own sample-based inference
(`LdbwsInferred`). Once `LdbwsInferred`'s `from_date` becomes a genuine
"first observed by this app's own sampling" time rather than an outright
fiction, it is still an *approximation* — samples run on a poll cycle, so
the true start could be up to one polling interval earlier than what gets
recorded, and a severity that briefly dipped below threshold for exactly
one cycle (sampling noise, not a real recovery) would incorrectly reset the
carry-forward and understate how long the disruption has really run.
Consider phrasing the `LdbwsInferred` case as "since approximately
{time}" or attaching a tooltip, distinguishing it from the Knowledgebase
case's precise, NRE-sourced "Valid: {time} – {time}". `DATA_QUALITY_LABELS`
(`IssueList.tsx:37-43`) already renders a "LDBWS-inferred" provenance badge
next to every such status, so the information to make this distinction is
already on screen — this is a wording/precision-hedging refinement, not new
plumbing.

## What this does *not* fix, stated plainly

- **Severity flapping still resets `from_date`.** If `infer_from_samples`'s
  sample-derived severity oscillates cycle-to-cycle near a threshold
  (noisy sampling, not a real change), each flap is treated as "a new
  status" by the stripped-content comparison, and `from_date` resets. This
  is the same tradeoff `normalize_for_diff` already accepts for
  `line_status_history` row creation today — a flapping status already
  writes a fresh history row on every flap, so this fix doesn't make
  flapping-driven noise any worse than it already is, it just extends the
  same acceptance to `from_date`. Genuinely damping flapping (e.g.
  hysteresis before demoting) is a separate, unrelated piece of work, not
  proposed here.
- **This does not improve segment precision or delay-minute accuracy** —
  that's the TRUST-line-level project's territory (see above), explicitly
  not what this fix touches. A carried-forward `from_date` on an
  `LdbwsInferred` status is still describing a line-wide aggregate with no
  segment attribution; this fix makes its *timing* honest, not its
  *content* richer.
- **The one-poll-cycle-of-slop in exactly when a disruption started is
  inherent to a polling-based inference method** and isn't closeable
  without a push-based signal (which is what the TRUST research explored
  and deferred). This fix moves `from_date` from "always wrong, resets
  every cycle" to "accurate to within one polling interval of the first
  cycle that detected it" — a large, concrete improvement, not a perfect
  one.

## Testing

Following this repo's existing convention (`crates/aggregator/src/
queries.rs` and `aggregation.rs` both already have substantial `#[tokio::
test]`/unit coverage in-file):

- `write_line_status` (or a new pure helper it calls, e.g.
  `carry_forward_ldbws_from_date(existing: &Value, fresh: &Value) -> Value`,
  kept pure and unit-testable separately from the DB call): 
  - two consecutive `LdbwsInferred` statuses with identical stripped
    content → new `from_date` equals the *old* one, not the fresh stamp.
  - two consecutive `LdbwsInferred` statuses with a different `severity`
    or `reason` (post-strip) → new `from_date` is the fresh stamp,
    unchanged from today's behavior.
  - existing entry is `Knowledgebase`/`Planned`/absent (first time this
    line has ever produced an `LdbwsInferred` status) → fresh stamp, no
    carry-forward.
  - `good_service()` entries carry forward the same way (covers the
    lower-stakes third call site).
  - confirm `sample_stats` and the live-sample-annotation suffix churning
    between cycles does **not** by itself defeat the carry-forward (the
    exact case the existing `normalize_for_diff` doc comment already
    warns about).
- Integration-level: a real two-cycle `aggregate()` → `write_line_status()`
  sequence (matching this file's existing `aggregate_with_defaults`-style
  tests) confirming a stable `LdbwsInferred` status across two cycles
  produces one `line_status_history` row (unchanged from today) *and* a
  stable `from_date` in the second cycle's stored `line_status` row (new
  assertion).
- Frontend: `frontend/lib/stationIssues.test.ts` (if it exists — verify at
  implementation time) gets a regression case for the dedup bug found
  above: two `LineStatusReport`s with `LdbwsInferred` statuses carrying the
  *same* `fromDate` string (as the fix would now produce for a genuinely
  shared disruption) merge under `statusKey`; two with *different*
  `fromDate` strings (a genuinely different disruption) do not. No frontend
  production code changes, but this locks in that the backend fix is what
  actually closes the dedup gap, rather than asserting it without a test.

## Open questions / risks

1. **Whether the carry-forward logic belongs inside `queries.rs` (post-hoc,
   operating on the serialized JSON, as sketched above) or earlier, inside
   `aggregation.rs`'s `aggregate()` itself (operating on typed
   `LineStatus` values, requiring `aggregate()` to be handed the previous
   cycle's reports as an extra input) is a real implementation-time
   decision, not settled here.** The JSON-level approach in `queries.rs`
   keeps `aggregate()`'s existing pure-function signature and test suite
   completely untouched (a real, valuable property — `aggregation.rs`'s
   tests are extensive and construction-order-sensitive) at the cost of
   working with `serde_json::Value` munging instead of typed structs. The
   `aggregation.rs` approach is more type-safe but means threading
   "previous cycle's reports" through `aggregate()`'s signature and
   `main.rs`'s call site, a wider-blast-radius change to a function this
   codebase's own module doc already treats as a clean, pure port from a
   Python prototype. Recommend the `queries.rs`/JSON approach for exactly
   that reason, but flag this as worth a second opinion at implementation
   time.
2. **Positional matching (comparing `statuses[0]` old vs. new) is correct
   today because `infer_from_samples`/`good_service()` only ever produce a
   single-entry `statuses` array per line** (confirmed:
   `aggregate()`'s Layer 2 loop only pushes when `report.statuses.is_empty()`
   — an `LdbwsInferred` entry never coexists with an incident-derived one
   in the same array). **This assumption would break silently if a future
   change (e.g. the deferred TRUST-line-level work, or a hypothetical
   segment-level LDBWS split) ever produced multiple `LdbwsInferred`
   entries for one line.** If that ever happens, positional matching needs
   to become content-keyed (e.g. match on `(severity, reason-prefix)` before
   stripping the live-count suffix) — noted here so a future implementer
   doesn't have to rediscover this constraint, not solved preemptively for
   a case that doesn't exist yet.
3. **The "since approximately X" frontend wording polish (above) is a
   genuine judgment call about how much epistemic hedging is worth
   showing a user**, and this doc doesn't mandate it — the backend fix
   alone is a strict improvement over today's status quo even with zero
   frontend changes, since "accurate to one poll cycle" beats "always
   wrong" regardless of whether the UI hedges about it.
4. **This fix makes `line_status.statuses`'s `from_date` durable/meaningful
   for the first time for `LdbwsInferred` entries — worth confirming this
   doesn't interact badly with `prune_removed_lines`
   (`crates/aggregator/src/queries.rs:106-126`) or any other code path that
   currently assumes `line_status` rows are cheaply, harmlessly
   fully-overwritten every cycle.** A quick grep at implementation time for
   any other reader of `line_status.statuses` beyond the API's own render
   path is worth doing before shipping, though nothing found in this
   research pass suggests a conflict.
