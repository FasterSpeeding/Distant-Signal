# Design/Scoping: A TfL Disruption "Incident Page"

**Status: scoping/research document, not approved, not implementation-ready.
No migration, no Rust code, no frontend code in this pass — matching this
repo's own convention for a document at this stage**
(`docs/superpowers/specs/2026-09-06-schedule-line-population-future-dates-design.md`,
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`, whose own
opening line reads "Status: design proposal, not approved").

This document is a direct extension of
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md` (the
"incident-detail-page design"), which built `/incidents/[id]` for
Knowledgebase-sourced National Rail incidents and explicitly, by name,
scoped TfL out of it (its "Explicitly out of scope" section's first bullet:
*"A detail page (or anything resembling one) for LDBWS-inferred or TfL
statuses… There is no comparable row to look up for either"*). This document asks what it would
actually take to close that gap for TfL specifically, and — per the brief
— treats both open questions below as real, unresolved product/architecture
decisions rather than settled implementation details.

Required reading consumed in full before this document was written:
`crates/poller-tfl/src/{main,schema}.rs`; `crates/api/migrations/20260510023522_initial.sql`;
`crates/api/migrations/20260822120000_line_status_source.sql`;
`crates/api/src/routes/{ingest,incidents,line_status}.rs`;
`crates/api/src/data/queries.rs` (the TfL-upsert and incident-detail
sections); `crates/common/src/lib.rs` (`Severity`, `Disruption`,
`LineStatus`, `LineStatusReport`, `severity_from_tfl_code`); `frontend/lib/incidents.ts`;
`frontend/app/incidents/[id]/page.tsx`; `frontend/app/lines/[id]/page.tsx`;
`frontend/lib/severity.ts`; `crates/enricher/src/{main,llm}.rs`;
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md` (in
full — this document's Correction 1, Decisions 1–6, and the first bullet
of its "Explicitly out of scope" section are the direct precedent for
everything below); `docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`.

## Why this document exists

A user wants a TfL-disruption page comparable to `/incidents/[id]`. On
inspection this is not a small frontend addition: the National Rail
incident page works because Knowledgebase incidents already carry a real,
persisted, RDM-assigned `incident_id` that survives across polls, and a
whole `incidents`/`incident_history` schema exists to store it. TfL's API
has no equivalent concept anywhere in its response shape. Section 2 below
scopes what it would take to invent one; Section 3 covers a second,
independent gap this research surfaced along the way — TfL's status text
routinely describes several differently-affected segments of one line in
a single free-text string, and the current pipeline has no way to keep
that distinction once TfL's response is mapped into this app's schema.

## 1. Current architecture, recapped with exact citations

### 1a. The TfL path today

**Poll and map.** `poller-tfl` polls `GET /Line/Mode/{modes}/Status`
(`crates/poller-tfl/src/main.rs:297-304`, `fetch_status_json`) every
`config.poll_interval_secs` (300s by default). The response is parsed by
`schema::parse_line_status` (`crates/poller-tfl/src/schema.rs:92-95`) into
`Vec<common::LineStatusReport>`. The module's own doc comment states the
scope line explicitly (`schema.rs:21-24`):

> Everything stop-level (`affectedStops`, `affectedRoutes`) is dropped: TfL
> identifies stops by Naptan id (`940GZZLUABC`) and this app's station
> columns are `CHAR(3)` CRS codes. That is v1's scope line, not an
> oversight.

Each TfL line (`TflLine`, `schema.rs:36-48`) carries a `Vec<TflLineStatus>`
(`schema.rs:50-63`) — one entry per simultaneous status TfL is reporting on
that line (a planned closure and a live disruption can coexist, and the
codebase has a named regression test for exactly that, `"keeps_every_simultaneous_status_on_a_line"`,
`schema.rs:325-363`). **Each `TflLineStatus` carries exactly one
`status_severity: u8` and one `reason: Option<String>`** (`schema.rs:52-58`)
— there is no field anywhere in this struct, or in TfL's own wire shape as
modelled here, for "severity of this reason text's second sentence" or
"severity between these two stations." `map_status`
(`schema.rs:115-159`) turns each `TflLineStatus` into exactly one
`common::LineStatus` (`crates/common/src/lib.rs:374-396`), and where a
`disruption` object is present, one `common::Disruption`
(`lib.rs:346-370`) with:

```rust
source: Some(format!("tfl-line-status-{line_id}")),   // schema.rs:143
affected_stops: vec![],                                // schema.rs:141, always empty
affected_routes: vec![],                                // schema.rs:142, always empty
impact_type: None,  // TfL never runs the enricher's extraction pipeline — schema.rs:144
```

**Ingest.** `poll_once` (`main.rs:145-192`) POSTs the batch via
`common::ingest::post_batch` to `config.api_ingest_url`, landing on
`/private/tfl-line-status` — registered in `crates/api/src/routes/ingest.rs:56-58`
and handled by `post_tfl_line_status` (`ingest.rs:225-233`), whose own doc
comment states the load-bearing architectural fact: *"Unlike the other four
ingest routes, this one writes the aggregator's output table directly…
TfL publishes finished line status, so there is nothing for the aggregator
to infer"* (`ingest.rs:218-224`). The handler calls
`queries::upsert_tfl_line_status` (`crates/api/src/data/queries.rs:387-457`),
which, per line, in one transaction:

1. Diffs the incoming `statuses` JSON against what's stored for that
   `line_id` (`tfl_statuses_changed`, `queries.rs:342-350`, ignoring
   `sample_stats`/`sample_availability`/the full-coverage pair via
   `normalize_for_diff`, `queries.rs:360-373`, so a live delay count
   rolling over doesn't spam history).
2. Upserts into `line_status` with `source = 'tfl'`
   (`queries.rs:405-424`) — the `source` **column** added by
   `crates/api/migrations/20260822120000_line_status_source.sql`, which is
   a completely different thing from `Disruption.source` (a JSONB field
   nested inside `statuses`; see that migration's own doc comment,
   `line_status_source.sql:1-27`, for exactly this same distinction the
   incident-detail-page design's Correction 2 had to call out).
3. If changed, appends one row to `line_status_history`
   (`queries.rs:427-434`).
4. Prunes any `line_status` row with `source = 'tfl'` that's missing from
   this batch (`queries.rs:444-450`) — TfL owns pruning its own rows, the
   aggregator owns pruning its own (`ingest.rs:439-443`'s comment).

**At no point does any TfL data touch `incidents` or `incident_history`.**
Grepped exhaustively: no file under `crates/poller-tfl` or the
`post_tfl_line_status` code path references either table. TfL disruptions
exist, today, exclusively as JSONB inside `line_status.statuses` (current
snapshot) and `line_status_history.statuses` (append-only audit log of
past snapshots) — both keyed by `line_id`, never by anything resembling an
incident id.

**No historical endpoint on TfL's own side**, per `poller-tfl/src/main.rs:11-13`'s
own module doc: *"Everything this app can ever show for 'the Victoria line
last Tuesday' is what this poller wrote into `line_status_history` at the
time."* Nothing this document proposes can retroactively recover
better-than-`line_status_history` fidelity for anything already polled.

### 1b. The National Rail (Knowledgebase) path, for comparison

1. `poller-incidents` polls RDM's Knowledgebase Incidents feed and POSTs
   to `/private/incidents` (`crates/poller-incidents/src/main.rs:3` doc
   comment, `:64-68` `post_batch` call).
2. `post_incidents` (`crates/api/src/routes/ingest.rs:159-167`) calls
   `queries::upsert_incidents`, which writes into `incidents` — a table
   keyed on RDM's own `incident_id TEXT PRIMARY KEY`
   (`crates/api/migrations/20260510023522_initial.sql:16-27`) — and
   appends to `incident_history`
   (`initial.sql:107-121`) whenever `summary`/`description`/`validity_periods`
   differ from the stored row (per the incident-detail-page design's
   Current-relevant-state section, `incident_changed`).
3. `aggregator`'s `status_from_incident`
   (`crates/aggregator/src/aggregation.rs:123`) matches each `incidents`
   row against lines/stations and builds a `common::Disruption` with
   `source: Some(format!("knowledgebase-incident-{}", incident.incident_id))`
   (`aggregation.rs:156`) — the **only** `Disruption.source` format that
   names a real, persisted row anywhere in this codebase (the
   incident-detail-page design's Correction 1, still true today; the
   `infer_from_samples` LDBWS path builds a `Disruption` too, but with the
   literal constant `source: Some("ldbws-sampling".to_string())`,
   `aggregation.rs:920`, not an id).
4. `GET /public/incidents/{incidentId}` (`crates/api/src/routes/incidents.rs:17-19`,
   `get_incident` at `:31`) reads `incidents`/`incident_history` back via
   `queries::incident_by_id` (`queries.rs:1405`),
   `incident_history_for_id` (`queries.rs:1435`), and
   `lines_currently_reporting_incident` (`queries.rs:1473`, a
   `jsonb_array_elements` unnest over `line_status.statuses` matching the
   reconstructed `knowledgebase-incident-{id}` source string).
5. `frontend/app/incidents/[id]/page.tsx` (confirmed fully built, not just
   designed — this spec's precedent shipped) renders summary, sanitized
   HTML description, affected-stations badges, every validity period, a
   "currently affects" line list, and a newest-first change-history list
   with a per-entry field-diff summary
   (`describeChanges`, `page.tsx:29-39`). `frontend/lib/incidents.ts`'s
   `incidentIdFromSource` is the single gate that gets a user there —
   `null` for anything not prefixed `knowledgebase-incident-`, which is
   why no TfL `Disruption` renders a link today (`IssueList.tsx` embeds
   `DisruptionDetail`, which calls this function before rendering the
   "View full incident details" link).

The structural asymmetry the rest of this document has to resolve:
**National Rail incidents have a stable identity that arrives already
attached, from RDM itself, before this app ever sees the data. TfL
disruptions have no such thing at any layer — not in TfL's wire format,
not in `common::Disruption`, not in `line_status`.**

## 2. The core design problem: TfL has no stable incident identity

TfL's `/Line/Mode/{modes}/Status` response — even before `poller-tfl`
drops anything — carries no per-disruption id. What repeats poll-to-poll
is: a line id, a `statusSeverity` code, a free-text `reason`, and
`validityPeriods`. There is no `disruptionId`, no assertion from TfL that
"this is the same disruption as it reported 300 seconds ago" — every poll
is a fresh, complete re-statement of current line status, re-derived from
whatever TfL's own operational systems currently believe. A signal failure
at Oxford Circus reported at 09:00 and again at 09:05 is, from this app's
point of view, two independent, textually-similar `LineStatus` entries
with no linkage TfL itself asserts.

Giving TfL an `/incidents/[id]`-style page requires inventing that
linkage. Two genuinely different approaches were considered.

### Option A — synthesize identity from `(line_id, reason_text_hash)`

Treat "the same `line_id` reporting the same (or near-same) `reason` text
on two consecutive polls" as "this is the same real-world disruption
continuing," and persist a row the first time a given `(line_id,
reason_hash)` pair is seen, updating/append-historying it on every poll
where the pair recurs, closing it out (marking it no-longer-current) the
first poll where it doesn't.

**Failure modes, concrete, not hypothetical:**

- **Text reword without a new event.** TfL's operational feed rewords its
  own prose between polls for what is, on the ground, the same ongoing
  disruption — "Minor delays due to an earlier signal failure" becoming
  "Minor delays while we recover services following a signal failure
  earlier" is a realistic, unremarkable edit a TfL controller might make.
  A naive exact-text hash would read that as "old incident closed, new
  incident opened" — silently truncating the very continuity history the
  National-Rail page's whole reason for existing is to show. A fuzzy
  match (edit distance, token-set similarity) narrows this but trades it
  for the opposite risk: two textually similar but genuinely distinct
  disruptions on the same line in quick succession (a signal failure at
  09:00 recovering by 09:30, then an unrelated points failure at 09:45 with
  similarly-generic wording, "Minor delays due to a points failure") could
  be folded into one continuing "incident" that never actually happened as
  one event.
- **The compound multi-segment string (Section 3) makes this fuzzier
  still.** A real, production-shaped TfL status string looks like:

  > "Severe delays between Heathrow Terminals 2&3 and Heathrow Terminal 5
  > due to [cause]… MINOR DELAYS between Hayes & Harlington and Reading…
  > MINOR DELAYS between Shenfield and Whitechapel… GOOD SERVICE on the
  > rest of the line."

  This is one `reason` string covering four segments at three different
  severities, all landing in one `TflLineStatus`/`common::LineStatus`
  entry with a single `status_severity` (the worst of the four, by TfL's
  own convention, though nothing in the modelled schema records that it
  *is* an aggregate — see Section 3). A hash over that whole string
  changes the moment **any one segment's clause changes** — e.g. the
  Heathrow segment recovering to Minor while the other two clauses stay
  word-for-word identical. Under Option A that reads as "the whole
  incident closed and a new one opened," even though three of the four
  underlying facts didn't change at all. There is no sub-string
  granularity to key off without first solving Section 3's segment-parsing
  problem — the two problems are coupled, not independent, for any design
  that wants Option A's identity to track real-world continuity at
  finer-than-whole-status granularity.
- **No operator confirmation, ever.** Unlike Knowledgebase incidents,
  which RDM assigns and republishes with the same `incident_id` for the
  incident's whole lifecycle (giving `upsert_incidents`' diff check
  something authoritative to key off), an Option-A identity is this app's
  own guess, silently wrong in both directions (splitting one real event
  into several "incidents," or merging several real events into one) with
  no way to self-correct beyond adjusting the fuzz threshold and hoping.

### Option B — a live-snapshot view, no persistent incident-with-history record

Don't invent an identity at all. Instead of a `/incidents/[id]`-style page
keyed on a synthetic id, build a page that answers a narrower, honest
question: **"what is TfL reporting right now for this line?"** — a live
snapshot, not a history timeline.

**This can reuse `line_status`/`line_status_history` directly, with no new
table.** `line_status` already is, structurally, exactly this: "one row
per line, fully replaced each cycle" (`initial.sql:59-67`'s own header
comment), already keyed by `line_id` (which — via `TFL_LINE_ID_PREFIX`,
`crates/common/src/lib.rs:254` — already round-trips cleanly to and from a
TfL line id, e.g. `tfl-victoria` ↔ `victoria`), and already has a real
history sibling in `line_status_history` (`initial.sql:82-96`), keyed
`(line_id, computed_at DESC)` — exactly the access pattern needed to show
"here's how the Victoria line's status has changed over the last N hours,"
with **zero schema change**. A "TfL line disruption page" under Option B
is really "a `/lines/[id]` detail view, scoped down to one line and framed
around its current disruption(s)" rather than a genuinely new incident
concept — the frontend equivalent of `IssueList`'s existing TfL-status
rendering on `/lines/[id]` (`frontend/app/lines/[id]/page.tsx:171`,
`<IssueList items={report.tflStatus.map(...)} now={now} />`) promoted to
its own URL, rather than a new data model.

**UI/UX tradeoff, stated plainly:** this is a genuinely less rich page
than the National Rail one. There is no "first seen," no per-field change
diff, no durable identity a user could bookmark and expect to still
resolve to "the same disruption" a day later — a bookmark to a TfL
line-status page would just show whatever TfL is reporting for that line
*at the time it's opened*, disruption or not, which may be an entirely
different, later disruption than what the user originally bookmarked. It
trades the National Rail page's core value proposition — "how has this
one specific thing evolved" — for a narrower, honestly-scoped one — "what
is this line's status right now, and roughly how has it trended over the
history window." `line_status_history` can still show a real trend
(computed_at-ordered snapshots), it just cannot promise "these snapshots
are all the same incident," because nothing asserts that.

### Recommendation

**Option B**, with the reasoning stated as the real, unresolved design
call it is, not a foregone conclusion:

Option A can be built — the `(line_id, reason_hash)` mechanics are not
technically hard — but it manufactures a false promise. A page titled
"TfL Incident #whatever" with a first-seen date and a history timeline
implicitly asserts the same thing the National Rail page correctly
asserts (this is one tracked real-world event), and TfL's feed gives no
factual basis for that assertion. Getting it wrong is not a rare edge case
— reworded text and multi-segment compound strings (Section 3) are TfL's
*normal* operating mode for anything beyond "Good Service," not unusual
input. Option B is honest about what TfL's API actually is: a live
line-status feed with no incident concept, and building on top of
`line_status`/`line_status_history` — tables that already encode exactly
that shape, with a pruning/retention story already solved
(`HISTORY_RETENTION_DAYS`, named in the incident-detail-page design's own
"Explicitly out of scope" section as the retention mechanism
`line_status_history` already has and `incidents`/`incident_history` still
lack) — costs no new migration and no new failure mode to reason about.

This is explicitly flagged as the open product decision it is: if a
future stakeholder decides a fuzzy-match "best effort" continuity view is
worth the false-positive/false-negative risk (e.g. framed to the user as
"probably the same disruption," not asserted as fact), Option A is not
ruled out by anything technical here — it is ruled out by this document's
judgment that the honesty cost outweighs the UX gain, which is a call this
document is making, not one the underlying data forces.

## 3. Second design problem: compound multi-segment status text collapses to one severity

**Confirmed by reading the code, not assumed:** the current pipeline has
no way to represent partial-line/segment-level severity. Every TfL status
update necessarily collapses to exactly one `Severity` value for the
whole entry, for two independent, stacking reasons:

1. **TfL's own wire schema, as modelled here, is per-status-entry, not
   per-segment.** `TflLineStatus` (`crates/poller-tfl/src/schema.rs:50-63`)
   has one `status_severity: u8` and one `reason: Option<String>` field —
   full stop. Even where TfL's real API does carry route-level detail
   (`affectedRoutes` on the `disruption` object — acknowledged as existing
   on the wire by `schema.rs`'s own doc comment, `:21-24`, "Everything
   stop-level… is dropped," which only makes sense if there is something
   there to drop), that detail is never a *per-segment severity* — TfL's
   `statusSeverity` is scoped to the whole `LineStatus` entry it belongs
   to, and `common::Severity`/`common::LineStatus`
   (`crates/common/src/lib.rs:28-67`, `374-396`) mirror that: one
   `severity` field, no nested per-segment breakdown type exists anywhere
   in this crate.
2. **`poller-tfl` additionally drops what segment-adjacent data TfL's wire
   format does carry.** `map_status` (`schema.rs:115-159`) hardcodes
   `affected_stops: vec![]` and `affected_routes: vec![]` unconditionally
   (`schema.rs:141-142`) — even if a future change stopped collapsing
   severity to one value, the Naptan-id/route data needed to say *which*
   segment a given sub-severity applies to is thrown away before it ever
   reaches `common::Disruption`.

The real production example the brief supplies —

> "Severe delays between Heathrow Terminals 2&3 and Heathrow Terminal 5…
> MINOR DELAYS between Hayes & Harlington and Reading… MINOR DELAYS
> between Shenfield and Whitechapel… GOOD SERVICE on the rest of the
> line."

— is exactly the shape this schema cannot express: one `reason` string,
one `status_severity` (by TfL's own convention, the worst of the four
clauses — "Severe delays" — though nothing downstream records that this
is an aggregate rather than a literal whole-line severity), and a
frontend badge (`frontend/lib/severity.ts`'s `SEVERITY_TABLE`/`GROUP_COLOR`)
that will render the entire Elizabeth line as uniformly "Severe Delays"
red, even on the three-quarters of the line the text itself says is fine.
**A TfL incident page built on top of only whole-line severity would show
a strictly less accurate picture than the raw TfL `reason` text already
contains** — the prose already distinguishes the segments; this app's own
schema is what discards that distinction on the way in.

### Is segment-level parsing a v1 prerequisite, or a deferrable follow-up?

**Assessed here as a deferrable follow-up, not a hard prerequisite for
shipping any TfL page at all** — but only for the specific page shape
Section 2 recommends (Option B). The reasoning:

- **Under Option B, the raw `reason` string is already the whole
  content.** A live-snapshot page's job is to show "what TfL is reporting
  right now" — and the honest, zero-parsing version of that is the exact
  compound string TfL sent, rendered as plain text (or the same sanitized
  HTML treatment `DisruptionDetail`/`sanitizeDescription` already give the
  Knowledgebase `description` field). A v1 TfL page that shows *"Severe
  delays between Heathrow Terminals 2&3 and Heathrow Terminal 5… [full
  text]…"* verbatim, under one severity-colored heading badge, is honestly
  worse than a segment-aware page but is **not wrong** — it doesn't assert
  anything the text doesn't say, it just doesn't visually separate what
  the prose already separates. This is a materially different risk
  profile from Option A's identity problem (Section 2), which risks
  asserting a false *fact* (continuity); showing the compound string
  as-is risks only a coarser *badge*, with the disambiguating detail still
  present, just unstyled, directly below it.
- **Under Option A, this gap would matter more** — a synthetic identity
  keyed on whole-reason-text hashing degrades further, not better, when
  the text is compound (Section 2's third bullet already covers this) —
  which is one more mark against Option A, not a reason to build segment
  parsing first regardless of which option is chosen.
- **Reuse of the `enricher` LLM-extraction pattern is architecturally
  plausible but not a drop-in.** `enricher` (`crates/enricher/src/main.rs:1-5`
  doc comment) extracts, per Knowledgebase incident, a `category` plus a
  list of `ExtractionPeriod`s (`crates/enricher/src/llm.rs:54-89`) — but
  every field on `ExtractionPeriod` (`date_range`, `schedule_window`,
  `resolution_status`, `apparent_severity`) is a **temporal** dimension:
  "this fact holds between these dates/times." Nothing in that schema
  splits by **geography** — a TfL "segment" (Heathrow T2&3–T5 vs. Hayes &
  Harlington–Reading) is a spatial concept the existing `ExtractionPeriod`
  shape has no field for. Adapting the pattern for TfL would mean
  designing a parallel, geography-keyed extraction schema (a `segment`
  concept: `scope_description`-like free text but resolved, ideally, to
  concrete stop/route identifiers, plus a severity per segment) — a
  genuinely new prompt/schema/combine-logic design, not a parameter change
  to the existing one. It would also need to solve a harder resolution
  problem than the temporal extractor does: turning "Heathrow Terminals
  2&3" and "Hayes & Harlington" into TfL Naptan ids or this app's own CRS
  codes is a geocoding/reference-matching problem the temporal extractor
  never had to face (a date is self-describing; a station name embedded in
  free prose is not, and TfL line topology for the Elizabeth line/Overground
  spans dozens of stations this app may or may not have complete Naptan
  coverage for — an open question of its own, not scoped here).
- **Recommendation: defer.** Ship a v1 TfL page (Option B: live snapshot,
  raw compound text rendered as-is) without segment parsing. Treat
  segment-level extraction as a genuinely separate, later feature — one
  that, if pursued, would need its own scoping document with its own
  reference-data-coverage research, not a rider on this one. This mirrors
  the precedent the National Rail path itself already set: `enricher`
  (2026-08-20) shipped only after the base incident page's core plumbing
  (2026-08-31, later) — extraction is documented throughout as *"an
  additional signal,"* never a precondition for the underlying feature to
  exist at all, and this document's Option-B recommendation preserves the
  same layering: ship the honest raw view first, consider enrichment as a
  strict addition afterward if it proves worth the geocoding investment.

## 4. Data model

**Under the recommended approach (Option B), no new table is needed.**
`line_status` and `line_status_history` already carry everything a
live-snapshot TfL page needs, in the shape they already have
(`initial.sql:69-96`, `line_status_source.sql`'s `source = 'tfl'` column).
No migration is proposed by this document.

For contrast, had Option A been chosen, the shape it would need — sketched
here only to make the two options' costs comparable, not proposed for
implementation:

```sql
-- NOT proposed for implementation -- Option A only, shown for cost comparison.
CREATE TABLE tfl_disruptions (
    disruption_id     TEXT PRIMARY KEY,   -- synthetic: hash(line_id, reason_text) or similar
    line_id           TEXT NOT NULL,
    severity          SMALLINT NOT NULL,
    reason            TEXT NOT NULL,
    category          TEXT,               -- TfL's RealTime/PlannedWork/Information
    first_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_current        BOOLEAN NOT NULL DEFAULT TRUE  -- false once a poll no longer reports this pair
);
CREATE TABLE tfl_disruption_history (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    disruption_id  TEXT NOT NULL,
    reason         TEXT NOT NULL,
    severity       SMALLINT NOT NULL,
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

This would require new upsert logic in `queries::upsert_tfl_line_status`
(or a parallel path) to compute the `(line_id, reason_hash)` key per
status entry, diff against `is_current` rows, and manage the
open/close lifecycle Section 2 already flagged as unreliable — real
implementation cost on top of the identity problem itself, which is the
concrete "cost of Option A" this section exists to make visible.

## 5. API + frontend surface (Option B)

**New route: `GET /public/tfl-lines/{lineId}`** (naming mirrors the
existing `Path(String)` / `{incidentId}` convention in
`crates/api/src/routes/incidents.rs:18` and the `{id}`-in-URL convention
that `docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`'s
Decision 1 established) — or, more minimally, this could be served by
*extending* the existing line-status route family
(`crates/api/src/routes/line_status.rs`) with a history parameter, since
`/lines/[id]` already fetches and renders a TfL line's current statuses
today (`frontend/app/lines/[id]/page.tsx:36`, `:171`) and a dedicated
`/tfl-lines/[id]` page would otherwise duplicate that fetch. Two
sub-questions genuinely open, flagged rather than resolved:

- **Is a dedicated page needed at all, or does `/lines/[id]` already
  satisfy the ask once its existing TfL `IssueList` section is expanded to
  show `line_status_history` trend data?** `/lines/[id]` already shows a
  TfL line's current disruption(s) in the exact rendering the brief is
  asking for (Section 1a); the incremental piece under Option B is really
  "add a history view," which the sibling `/lines/[id]/history` route
  already exists as a precedent for (referenced by the incident-detail-page
  design's Decision 6 as "the closer precedent" for exactly this kind of
  "changes over time" list). A `/lines/[id]/tfl-history`-shaped addition,
  reusing the existing `groupHistoryByDay`-style rendering, may be a
  smaller, more consistent change than a wholly separate `/tfl-lines/[id]`
  page family. This document does not resolve which; either shape is
  compatible with the Option B recommendation and the reuse of
  `line_status`/`line_status_history` as-is.
- **Linking in from `DisruptionDetail.tsx`.** `incidentIdFromSource`
  (`frontend/lib/incidents.ts`) is the single gate the incident-detail-page
  design built for exactly this kind of link decision. Under Option B, a
  TfL `Disruption.source` (`tfl-line-status-{lineId}`) *does* carry
  everything needed to link somewhere real — the line id — unlike the
  LDBWS `ldbws-sampling` constant, which names nothing. A second helper
  (e.g. `tflLineIdFromSource`) mirroring `incidentIdFromSource`'s shape
  could extract `{lineId}` from that prefix and link to whichever page
  shape is chosen above. This is a small, additive change to the same
  file/pattern the incident-detail-page design already established, not a
  new mechanism.

Whichever shape is chosen, the page content itself is comparatively
simple relative to `/incidents/[id]`:

- Line name/mode heading, current severity badge(s) — one per
  simultaneous `LineStatus` entry, reusing `IssueList`'s existing
  rendering rather than inventing new markup.
- Raw `reason` text per status entry, rendered as plain text (TfL sends no
  HTML in `reason`, unlike Knowledgebase's `description` — no
  `sanitizeDescription` call needed here, a genuine simplification vs. the
  National Rail page).
- Validity period(s), reusing `formatValidityPeriod`'s existing pattern
  from `frontend/app/incidents/[id]/page.tsx:18-21`.
- A history section sourced from `line_status_history`, framed honestly as
  "status snapshots over time for this line" — **not** a per-incident
  timeline, and **not** using `describeChanges`'s incident-field-diff
  language, since there is no single incident identity for a diff to be
  "of." A plain reverse-chronological list of past `(severity, reason,
  computed_at)` snapshots is the honest equivalent.
- **No "currently affects [these lines]" section** — meaningless under
  Option B, since the page is already scoped to one line; that section
  exists on `/incidents/[id]` specifically because one Knowledgebase
  incident can span several lines, a concept Option B does not carry over.
- **No "first seen" field** — Option B has nothing corresponding to it;
  the closest honest substitute is "earliest snapshot in the retained
  history window," which is a retention-window artifact, not a real fact
  about when the disruption started (TfL doesn't say).

## 6. Explicitly out of scope

- **Option A's synthetic-identity mechanism, in any form.** Recommended
  against in Section 2; not designed further here beyond the cost-sketch
  in Section 4. If a future decision overturns this recommendation, it
  needs its own design pass — this document does not pre-design it.
- **Segment-level parsing/extraction of TfL's compound status text**, LLM-
  or otherwise-assisted. Named in Section 3 as a real gap and assessed as
  deferrable; not designed here. Any future pursuit of it needs its own
  scoping pass covering, at minimum: a new geography-keyed extraction
  schema (distinct from `enricher`'s existing temporal `ExtractionPeriod`
  shape), a station/Naptan-id resolution strategy for free-text place
  names, and a rendering design for a severity-per-segment UI — none of
  which this document attempts.
- **Naptan-id/CRS reconciliation.** `poller-tfl` drops `affectedStops`/
  `affectedRoutes` because TfL's Naptan ids don't map onto this app's
  `CHAR(3)` CRS station columns (`schema.rs:21-24`). Neither Option A nor
  Option B as designed here requires solving that mapping — Option B
  never needs stop-level data at all (it shows the raw line-level text
  verbatim), and Option A's synthetic identity is line-scoped, not
  stop-scoped. Solving Naptan/CRS reconciliation would only become
  necessary if a future iteration pursued Section 3's deferred
  segment-parsing work and wanted to render segments against this app's
  own station catalogue rather than TfL's own place names as free text.
- **A "browse all TfL disruptions" index page.** Mirroring the incident-
  detail-page design's own equivalent non-goal — only a per-line detail
  view is scoped here.
- **Retroactive backfill of pre-existing `line_status_history` rows into
  whatever new shape Option A would have needed**, had it been chosen.
  Moot under the Option B recommendation, since no new table exists to
  backfill.
- **Changing how severity/status is computed or displayed anywhere else
  in this app** (e.g. `/lines/[id]`'s existing `IssueList` rendering,
  `frontend/lib/severity.ts`'s badge table, the dashboard's worst-severity
  summary). This document proposes an additional, narrowly-scoped detail
  view; it does not revisit any existing rendering.
- **DLR pilot sample-stats integration on this new page.** The DLR arrival-
  diffing pilot (`crates/poller-tfl/src/dlr`) already attaches
  `sample_stats`/`sample_availability` to `tfl-dlr`'s `LineStatus` entries
  today; a TfL detail page under Option B would render whatever
  `IssueList`-equivalent markup already shows for those fields on
  `/lines/[id]`, but this document does not design anything TfL-detail-page-specific
  for that data — it is inherited "for free" via the same status entries,
  nothing more.

## 7. Rough task breakdown

At the granularity used in `docs/superpowers/plans/` for prior features
(see e.g. `docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md`'s
Task 1–13 breakdown) — enough to show the scope is understood and
estimable, not a full implementation plan. Assumes the Section 2
recommendation (Option B, no new table) and defers Section 3's
segment-parsing work entirely (per Section 6).

1. **Resolve the two open sub-questions Section 5 leaves open**: dedicated
   `/tfl-lines/[id]` page vs. an extension of `/lines/[id]/history`; and
   whether the history section is worth building in v1 at all, or whether
   v1 ships as "current status only" with history deferred to a fast
   follow. This is a product decision, not engineering work, but blocks
   everything below being scoped precisely.
2. **Backend route** — whichever shape Task 1 resolves on: either a new
   `crates/api/src/routes/tfl_lines.rs` (mirroring `incidents.rs`'s
   shape: `Path(String)` extraction, 404 on unknown line id) reading
   `line_status`/`line_status_history` filtered to `source = 'tfl'`, or an
   extension of the existing line-status/history query functions to
   expose what's already fetched for `/lines/[id]` in the new page's
   needed shape. No migration either way (Section 4).
3. **`incidentIdFromSource`-equivalent frontend helper** — a small
   addition to `frontend/lib/incidents.ts` (or a sibling file) extracting
   `{lineId}` from a `tfl-line-status-{lineId}` source string, and wiring
   the resulting link into `DisruptionDetail.tsx` alongside the existing
   Knowledgebase link, gated so it never fires for `ldbws-sampling`.
4. **Frontend page** — the new route/page component itself: severity
   badge(s), raw `reason` text (no HTML sanitization needed), validity
   period(s), and (if Task 1 keeps it in v1) the history list sourced from
   `line_status_history`, explicitly not styled or worded as a
   per-incident diff (Section 5).
5. **Tests** — Rust: any new/extended query functions get unit/integration
   coverage mirroring `incident_by_id`/`incident_history_for_id`'s
   existing style; frontend: page-level tests mirroring
   `frontend/app/incidents/[id]/page.test.tsx`'s found/404/empty-history
   cases, plus a case for the compound-multi-segment `reason` string
   rendering as plain, unstyled text (the concrete regression test for
   Section 3's "raw text as-is" v1 decision).
6. **Explicitly not a task in this breakdown**: anything from Section 6
   (segment parsing, Naptan/CRS reconciliation, Option A's synthetic
   identity, a browse-all index). Each would need its own scoping pass
   before becoming a task list.

## Open questions / risks

1. **Whether a TfL line-status page is worth shipping at all, given how
   much less rich it necessarily is than the National Rail one.** Section 2
   names this directly: Option B's page cannot promise "this is the same
   disruption evolving," only "here is what's live now, and here's a
   snapshot trend." Worth validating user expectations before building —
   if what was actually wanted is the National Rail page's continuity
   guarantee, no version of Option B delivers that, and Option A's
   dishonesty risk (Section 2) may be judged worth it after all by a
   future decision-maker with more context on how the request arose than
   this document has.
2. **Whether `/lines/[id]`'s existing TfL rendering already satisfies most
   of the real ask**, making a dedicated page a smaller win than it first
   appears (Section 5's first bullet). Not resolved here — needs a product
   call, not more code-reading.
3. **Segment-level parsing's real cost is unknown**, not just deferred.
   Section 3 argues it's architecturally plausible via an `enricher`-
   adjacent pattern, but the Naptan/CRS place-name resolution problem it
   would introduce (turning "Hayes & Harlington" into a concrete stop
   identifier) has not been sized at all — it could be a modest reference-
   table lookup or a much harder open-ended geocoding problem depending on
   how TfL's free text names places in practice across all five TfL modes
   this app polls (tube, DLR, Overground, Elizabeth line, tram — per
   `poller-tfl/src/main.rs:1-2`'s own module doc). Any future pursuit needs
   its own research pass before estimation is possible, not just a scoping
   document.
4. **TfL's own `statusSeverity` convention for a compound string** (is the
   reported severity always literally the worst clause, as Section 3
   assumes by inspection of the one real example given?) is asserted here
   from a single example, not verified against a corpus of TfL responses.
   Worth confirming against more live captures before this assumption is
   relied on by any future segment-parsing design — a wrong assumption
   here would misdirect that design's most basic validation check ("does
   my extracted worst-segment severity match the wire `statusSeverity`?").
