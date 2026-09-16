# Design: TfL Services in the Incident Archive

**Status: design proposal, not approved. Recommendation: not yet — see
Section 1 and Section 6 for the full reasoning. No implementation, no code
or migration changes.** Written to the same rigor as
`docs/superpowers/specs/2026-09-12-incident-archive-design.md` (the feature
this one proposes extending, now shipped), and following
`docs/superpowers/specs/2026-09-12-group-lines-design.md`'s shape for a
design whose honest conclusion is "not yet" — the alternatives are still
costed in Section 5, the two disruption-shaped ones in real design detail,
so that a future decision to overturn the recommendation does not start
from zero. The recommendation itself is not a formality.

**Two findings up front, for anyone reading only this far:** the briefed
assumption that Elizabeth line and London Overground incidents are already
in the archive is **confirmed**, against live production data, not just
against the code (§1b). And the archive's Line filter returns **zero rows
for every line**, National Rail included, because the column it filters on
is never populated (§1c) — a live defect that has nothing to do with TfL
and that §6 puts ahead of anything in this document's nominal subject.

Required reading consumed in full before this document was written:
`docs/superpowers/specs/2026-09-12-incident-archive-design.md`;
`docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md`;
`docs/superpowers/plans/2026-09-07-tfl-incident-page-v1-implementation-plan.md`;
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`;
`docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(its "Confirmed out of scope" section);
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`;
`docs/superpowers/specs/2026-09-02-line-history-list-spamminess-research.md`;
`crates/poller-incidents/src/{main,schema}.rs`;
`crates/poller-tfl/src/{main,config,schema}.rs` and `crates/poller-tfl/src/dlr/`;
`crates/api/src/routes/{incidents,line_status,lines,ingest,reference}.rs`;
`crates/api/src/data/queries.rs` (the incident-upsert, incident-read,
`search_incidents` and TfL-upsert sections); `crates/api/src/render.rs`;
`crates/common/src/lib.rs` (`IncidentMessage`, `Disruption`, `LineStatus`,
`Severity`, `TFL_OPERATOR`, `TFL_LINE_ID_PREFIX`, `TFL_TO_NR_LINE_ID`);
`crates/enricher/src/{main,queries}.rs`;
`crates/aggregator/src/{config,queries,main}.rs` (the pruning path);
`frontend/app/incidents/page.tsx`;
`frontend/components/IncidentSearchForm.tsx`;
`frontend/lib/{incidents,incidentSource,modes,types}.ts`; every migration
under `crates/api/migrations/` touching `incidents`, `incident_history`,
`line_status` or `line_status_history`; and every `lines/*.toml`.

## 0. The idea, as briefed

The incident archive (`/incidents`, `GET /public/incidents`, backed by
`queries::search_incidents` and the `incidents_first_seen_at_id` index)
shipped on 2026-09-12 and is live: a keyset-paginated, filterable browse of
every National Rail Knowledgebase incident this app has ingested. The
question put to this document is whether TfL services can be brought into
it, and what that would cost.

The brief came with one stated assumption to check rather than accept:
that services running on National Rail infrastructure but branded as TfL —
the **Elizabeth line** and **London Overground** — are already implicitly
covered, because the archive is sourced from a feed that covers all
National Rail Knowledgebase incidents regardless of operator branding.
Section 1 tests that assumption. Section 2 onward deals with the real gap:
TfL services that do not touch National Rail infrastructure at all.

## 1. Verifying the Elizabeth line / Overground assumption

**Verdict: the assumption holds, confirmed end to end. The code contains
no TfL-branding exclusion anywhere (§1a), and live production data shows
real Elizabeth line and Overground incidents in the archive right now
(§1b). It nonetheless sits next to a separate, network-wide defect that
will make a user testing it conclude the opposite (§1c).** Everything
below is verified against the code or against the running deployment, not
inferred.

### 1a. The assumption holds: nothing in the pipeline excludes an operator

`incidents` has exactly one production writer of its feed columns:
`queries::upsert_incidents` (`crates/api/src/data/queries.rs:83`), reached
only via `POST /private/incidents`
(`crates/api/src/routes/ingest.rs:36-39`, handler at `:163-171`), whose
only client is `poller-incidents`
(`crates/poller-incidents/src/main.rs:54-71`,
`crates/poller-incidents/src/config.rs`'s `api_ingest_url` default). The
enricher is the only other production writer and it touches NLP columns
only (`crates/enricher/src/queries.rs:81-88`, called once from
`crates/enricher/src/main.rs:412`).

That single ingest path applies **no operator, region, mode or branding
filter at any layer**:

- `crates/poller-incidents/src/schema.rs::parse_incidents` (`:116`) maps
  every `PtIncident` in the RDM XML document to an `IncidentMessage`,
  carrying `operators` through verbatim as raw `OperatorRef` ATOC codes
  (`schema.rs:77-88`). No allowlist, no skip.
- `upsert_incidents` binds them verbatim into `operators TEXT[]`
  (`queries.rs:143`, `:165`).
- `queries::search_incidents` (`queries.rs:2046-2092`) applies
  `($1::text[] IS NULL OR operators && $1)` — an unfiltered request has no
  operator predicate at all.
- `routes::incidents::search_incidents`
  (`crates/api/src/routes/incidents.rs:189-276`) adds nothing else. Its
  entire filter set is `operator`, `line`, `from`, `to`, `planned`,
  `cleared`, `priority_min`, `priority_max` (`:54-91`).

Elizabeth line and London Overground are ordinary National Rail TOCs with
ordinary ATOC codes, and this repo already treats them as such: nine
catalogue files exist for them — `lines/elizabeth-line.toml`,
`lines/elizabeth-heathrow.toml`, `lines/elizabeth-shenfield.toml` (all
`operators = ["XR"]`) and `lines/overground-{liberty,lioness,mildmay,suffragette,weaver,windrush}.toml`
(all `operators = ["LO"]`), every one of them `mode = "national-rail"`.
The 2026-08-22 TfL-service-metrics spec's Area 2 ("Overground is not
ingested on the NR side at all — no `lines/overground-*.toml` files
exist") is **out of date**: those files now exist, and the TfL↔NR merge
shipped for all seven railways
(`crates/common/src/lib.rs:269-277`'s `TFL_TO_NR_LINE_ID`, suppression at
`crates/api/src/routes/lines.rs:387-407`, overlay at
`crates/api/src/routes/line_status.rs:100-127` and
`crates/api/src/render.rs:34-46`).

Consequences for the archive as it stands today, all of which follow
directly from the above:

- An `XR` or `LO` Knowledgebase incident lands in `incidents` like any
  other and appears in an unfiltered `GET /public/incidents` page.
- `GET /public/incidents?operator=XR` and `?operator=LO` work, with GIN
  overlap support already in place (`incidents_operators_gin`).
- Both codes reach the `/incidents` UI's Operator control, which is
  populated from `getAllTocs()` → `GET /public/tocs/all`
  (`crates/api/src/routes/reference.rs:33`), fed by `poller-tocs` from
  RDM's own Train Operating Company List — so any TOC RDM publishes is
  selectable, with no curation step this app could accidentally omit them
  from.
- Both railways also reach the UI's Line control: it renders
  `lines.filter((line) => line.source === 'catalogue')`
  (`frontend/components/IncidentSearchForm.tsx:90`), and all nine
  Elizabeth/Overground catalogue lines are `source: 'catalogue'`.

### 1b. The premise the brief did not state — checked against live data, and it holds

"The archive covers all National Rail Knowledgebase incidents regardless of
operator branding" is true **of this app's code**. Whether RDM's
Knowledgebase Incidents feed itself actually publishes incidents attributed
to `XR` and `LO` is a separate, upstream data question the code cannot
answer: RDM could omit those operators from the feed, or publish them under
an `OperatorRef` this app's TOC reference data does not carry, and nothing
in §1a would detect either.

**So it was checked empirically, against the live deployment
(https://ds.cursed.solutions, 2026-09-16), rather than left as a caveat.
It holds.** Both operator codes return real, current rows:

```
GET /api/incidents?operator=XR  -> {"operators":["XR"], "priority":2, "isCleared":true,
  "summary":"Residual disruption to Elizabeth line services between Shenfield and Romford", ...}
GET /api/incidents?operator=LO  -> {"operators":["LO"], "priority":2, "isCleared":true,
  "summary":"Disruption to London Overground services to / from West Croydon", ...}
```

(Queried through `/api/incidents`, the frontend's same-origin proxy —
`/public/incidents` is not internet-exposed directly.) **The briefed
assumption is confirmed end to end, not merely un-contradicted by the
code.** Elizabeth line and London Overground incidents are in the archive
today and are reachable through its Operator filter.

### 1c. The gap the assumption does not anticipate: the Line filter matches nothing, for any line

`crates/poller-incidents/src/schema.rs:106` hardcodes
`affected_stations: vec![]` on every `IncidentMessage` it builds, and
`crates/common/src/lib.rs:586` documents why in the type itself:

```rust
pub affected_stations: Vec<String>, // left empty by pollers — no CRS field exists in the Incidents schema, only free-text RoutesAffected
```

RDM's `Affects.RoutesAffected` is free text and is deliberately left
unparsed (`crates/poller-incidents/src/schema.rs:1-14`'s module doc). No
other production code path writes that column — the only other `INSERT
INTO incidents` statements naming it are inside `#[cfg(test)]` modules
(`crates/enricher/src/main.rs:544`, `crates/api/src/routes/incidents.rs:601`,
`crates/api/src/data/queries.rs:2154`,
`crates/api/src/data/queries.rs:3184`,
`crates/aggregator/src/queries.rs:1056`). So **`incidents.affected_stations`
is `'{}'` on every production row.**

The archive's `line` filter resolves a catalogue line id to its CRS list
(`crates/api/src/routes/incidents.rs:203-215`) and applies
`affected_stations && $2` (`crates/api/src/data/queries.rs:2067`) — against
always-empty arrays. **It therefore returns zero rows for every line, in
production, today — confirmed live, not only reasoned from the code:**

```
GET /api/incidents?line=elizabeth-line     -> {"nextCursor":null,"results":[]}
GET /api/incidents?line=overground-mildmay -> {"nextCursor":null,"results":[]}
```

while the unfiltered and `operator=`-filtered queries above return plenty
of rows, every one of them carrying `"affectedStations":[]`. The
incident-archive spec's Decision 2 anticipated the
filter would *under-count* relative to the real matcher
(`KeywordOnly`/`OperatorOnly` matches invisible to it) and worded the UI
copy accordingly; it did not anticipate that the column it filters on is
never populated at all. `incidents_affected_stations_gin` indexes an
always-empty array for the same reason.

Why this matters specifically to the assumption under test: a user checking
"is the Elizabeth line in the archive?" will reach for the Line dropdown,
pick "Elizabeth line", get an empty result set, and conclude the archive
does not cover it. That conclusion would be wrong — `?operator=XR` works —
but the UI gives them no way to discover that. **The assumption holds and
the user-visible behavior still contradicts it.**

This is a pre-existing, network-wide defect of the shipped archive, not a
TfL problem, and this document does not design its fix (Section 7). It is
raised here because it is load-bearing for the recommendation: until the
archive's line-scoped browse works for the railways it already covers,
extending its coverage to railways it does not cover is optimising the
wrong end of the feature.

### 1d. What is *not* covered for these seven railways

The Elizabeth line's and Overground's **TfL-sourced** status — the
`source = 'tfl'` `line_status` rows for `tfl-elizabeth`, `tfl-liberty`,
`tfl-lioness`, `tfl-mildmay`, `tfl-suffragette`, `tfl-weaver`,
`tfl-windrush` — reaches the archive not at all, the same as every other
TfL line (Section 2). For these seven the practical loss is small: the NR
side is the richer source (it is why the merge suppresses the TfL row from
`/public/lines` and demotes its statuses to an additive `tflStatus`
overlay, `crates/api/src/render.rs:34-46`), and the NR side *is* archived.
For tube, DLR and tram there is no NR side at all, and that is the real
gap.

## 2. Current relevant state: what TfL data this app actually has (verified 2026-09-16)

### 2a. Modes polled

`crates/poller-tfl/src/config.rs:30` — `tfl_modes` defaults to
`"tube,dlr,overground,elizabeth-line,tram"`, passed straight through to
TfL's `/Line/Mode/{modes}/Status` path segment. The same closed set, plus
`national-rail`, is the API's own `SUPPORTED_MODES`
(`crates/api/src/routes/line_status.rs:187-194`), mirrored in
`frontend/lib/modes.ts:12-19`.

`bus`, `river-bus` and `cable-car` are deliberately absent
(`config.rs:25-26`: "v1's scope is rail-like TfL modes"), and
`national-rail` is absent because this app's own four NR pollers produce
better data than TfL's summary view (`config.rs:26-30`).

On the brief's "TfL Rail-adjacent modes, if in scope at all": **there is no
such extra mode to find.** "TfL Rail" was the Elizabeth line's name before
its 2022 rebrand and is not a mode TfL's API returns; it is the same
railway already covered under `elizabeth-line`. Croydon Tramlink is `tram`,
already polled — verified in
`docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`'s
"Confirmed out of scope" section against TfL's own `/Line/Meta/Modes`
endpoint. So the TfL-native, non-National-Rail modes in scope for this
question are exactly three: **tube, DLR, tram.**

### 2b. What is fetched, and what is kept

Three TfL URLs are constructed anywhere in this repo, all in
`crates/poller-tfl/src/main.rs`:

- `:299` — `{base}/Line/Mode/{modes}/Status`, every `poll_interval_secs`
  (300 by default).
- `:248` — `{base}/Line/dlr/Arrivals` (DLR pilot only).
- `:280` — `{base}/Line/dlr/Timetable/{stopPointId}?direction=outbound`
  (DLR pilot only).

**TfL's `Disruption`, `StopPointDisruption` and `getStopPointDisruption`
endpoints are not called anywhere.** The `/StopPoint/{crs}/Disruption` and
`/Line/...` routes in `crates/api/src/routes/line_status.rs:39-80` are this
app's *own inbound* routes mimicking TfL's URL shape over National Rail
data — `get_stop_point_disruption` (`:311-361`) resolves the CRS against
`app.config.lines` (the TOML catalogue) and never consults TfL or sees a
TfL line id. This is worth stating plainly because the brief reasonably
read those route names as evidence of a richer TfL disruption ingest than
exists.

The DLR pilot (`crates/poller-tfl/src/dlr/`) infers `SampleStats` for one
pilot station (Poplar, outbound) by diffing Arrivals against the published
Timetable, and writes nothing of its own — it mutates the in-memory
`LineStatusReport` for `tfl-dlr` before the normal batch POST
(`merge_dlr_sample_stats` at `main.rs:207-215`,
`mark_dlr_pending` at `:226-236`). It is `dlr_pilot_enabled = false` by default and is
not enabled in `docker-compose.yml`. It produces no disruption records and
is not a candidate archive source.

### 2c. The TfL wire shape, and the absence of an identity

`crates/poller-tfl/src/schema.rs:50-63`:

```rust
pub struct TflLineStatus {
    pub status_severity: u8,
    #[serde(default)]
    pub status_severity_description: String,
    /// Absent on a healthy line — TfL sends no prose for Good Service.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub validity_periods: Vec<TflValidityPeriod>,
    #[serde(default)]
    pub disruption: Option<TflDisruption>,
}
```

and `:76-84`:

```rust
pub struct TflDisruption {
    /// `"RealTime"` | `"PlannedWork"` | `"Information"` in every observed
    /// response; `Option` only so a missing one cannot fail the whole poll.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}
```

**There is no id field on either, and none in TfL's wire shape that would
survive a poll.** TfL's `lineStatuses[].id` is a per-response ordinal
(`"id": 0` in the captured fixture at `schema.rs:234`) and is
deliberately not modelled. The only identifier attached to a TfL disruption
anywhere in this app is `Disruption.source = "tfl-line-status-{line_id}"`
(`schema.rs:143`) — keyed off the **line**, so three simultaneous statuses
on one line produce three `Disruption`s carrying the identical string. The
frontend encodes this fact as a hard gate:
`frontend/lib/incidents.ts:11-14`'s `incidentIdFromSource` returns `null`
for a `tfl-line-status-` source because "neither names a real `incidents`
row". The only TfL-source awareness that shipped is a display label
(`frontend/lib/incidentSource.ts:20` → `'Transport for London'`), not a
link.

`schema.rs:136-145` also hardcodes `affected_stops: vec![]` and
`affected_routes: vec![]` — TfL's Naptan ids have nowhere to go in a
CRS-shaped schema (`schema.rs:21-24`) — and `impact_type: None`, because
TfL never runs the enricher's extraction pipeline.

### 2d. Where TfL disruptions are persisted, and for how long

`upsert_tfl_line_status` (`crates/api/src/data/queries.rs:387-457`) writes
one `line_status` row per line with `source = 'tfl'`, appends one
`line_status_history` row when `tfl_statuses_changed`, and prunes TfL rows
missing from the batch (`queries.rs:446`). So TfL disruption text exists in
exactly two places: `line_status.statuses` (current snapshot, one row per
line, fully replaced) and `line_status_history.statuses` (append-only
snapshots).

Two facts about that history table decide most of this design:

1. **`line_status_history` has no `source` column.** The 2026-08-22
   migration added `source` to `line_status` only
   (`crates/api/migrations/20260822120000_line_status_source.sql:29`). A
   history row carries `(id, line_id, statuses, computed_at)`
   (`20260510023522_initial.sql:89-94`); TfL rows are distinguishable only
   by the `tfl-` id prefix.
2. **It is pruned at 7 days by default, and the pruner is blind to
   source.** `crates/aggregator/src/queries.rs:488-496`:

   ```rust
   pub async fn prune_history(pool: &PgPool, retention_days: i64) -> Result<u64> {
       let result = sqlx::query(
           "DELETE FROM line_status_history WHERE computed_at < NOW() - ($1 || ' days')::interval",
       ).bind(retention_days.to_string()).execute(pool).await?;
       Ok(result.rows_affected())
   }
   ```

   called unconditionally every cycle from
   `crates/aggregator/src/main.rs:246` with
   `config.history_retention_days`, whose default is `7`
   (`crates/aggregator/src/config.rs:22-24`). The aggregator does not write
   TfL history rows but it does delete them.

TfL's own side offers nothing to backfill from:
`crates/poller-tfl/src/main.rs:11-13` — *"There is no historical endpoint
on TfL's side. Everything this app can ever show for 'the Victoria line
last Tuesday' is what this poller wrote into `line_status_history` at the
time."*

**Net: the deepest TfL disruption archive that could be built from existing
data is seven days old.** That is the ceiling on any feature built over
this data, and it is a policy number rather than a law of nature — but
raising it needs a decision (§6), and no amount of engineering recovers
depth that was already deleted.

### 2d-bis. A seven-day TfL disruption history is already browsable, per line

This was nearly missed and is the single most important piece of current
state in this document, so it is called out separately rather than left
implicit.

`GET /Line/{id}/Status/{from}/to/{to}`
(`crates/api/src/routes/line_status.rs:361-405`, `get_line_status_history`)
special-cases only `custom-` ids for an ownership check and otherwise
passes any id straight to `queries::line_status_history_for_range`. It has
no `source` awareness and needs none — `tfl-victoria` is just a `line_id`.
`frontend/app/lines/[id]/history/` is likewise id-agnostic.

**Verified against the live deployment (https://ds.cursed.solutions,
2026-09-16) in a real browser, not only by reading the code or fetching
HTML:** `/lines/tfl-victoria` renders the line's current TfL disruption
("Minor Delays"), and `/lines/tfl-victoria/history` renders a populated
seven-day Timeline, headed *"229 status recomputes across 21 incidents,
newest first"*, with entries such as:

> **16 Sept 2026** — Part Suspended · "Victoria Line: No service Seven
> Sisters to Walthamstow Central while emergency services deal with a
> casualty on the track. SEVERE delays on the rest of the line…"
> 11:08–12:08 *(severity changed 24 times)*

`/tfl-lines/victoria` returns `404`, confirming §2e.

Three things in that output matter beyond "the page works":

- The rendering is **already disruption-shaped to the eye** — day-grouped,
  severity-badged, with the full TfL prose and a time range per entry — even
  though the underlying rows are snapshots. The existing UI even calls them
  "incidents" in its own header copy.
- It already **collapses churn**: "(severity changed 24 times)" is one
  rendered entry standing in for 24 underlying `line_status_history` rows.
  That is the read-side mitigation
  `docs/superpowers/specs/2026-09-02-line-history-list-spamminess-research.md`
  discussed, already built and already applied to TfL rows.
- The seven-day window is **visibly full**, not sparse: 229 recomputes for
  one tube line in a week.

So the honest statement of the gap is narrower than "TfL has no disruption

So the honest statement of the gap is narrower than "TfL has no disruption
history in this app". TfL disruption history exists, is persisted, is
rendered, and is reachable — **per line, for seven days**. What is missing
is a *cross-line* browse: the thing `/incidents` is to `/lines/[id]`'s own
per-line incident rendering. That reframing is load-bearing for Section 5.

### 2e. The TfL incident page from 2026-09-07 was never implemented

`docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md` recommended
Option B (a live-snapshot page, no synthetic incident identity), and
`docs/superpowers/plans/2026-09-07-tfl-incident-page-v1-implementation-plan.md`
resolved its open questions into a concrete task list. Neither shipped, 359 commits later:
`crates/api/src/routes/tfl_lines.rs` does not exist and is not in
`crates/api/src/routes/mod.rs`'s module list; `frontend/app/tfl-lines/`
does not exist; `queries::tfl_line_status_by_id`, `getTflLine` and
`tflLineIdFromSource` do not exist. The only two commits for that feature
are docs-only (`d54708c`, `d96dee9`).

**This is a smaller problem than it looks, and an earlier draft of this
document got it wrong.** Every row in the shipped archive links to
`/incidents/[id]` (`frontend/components/IncidentSearchForm.tsx:331`), and
it is tempting to conclude a TfL row would have nowhere to go. It has
somewhere: `/lines/{tfl-line-id}` already exists and already renders that line's
current TfL disruption text (§2d-bis, verified live). The 2026-09-07 plan
chose a dedicated `/tfl-lines/[id]` over it on *fit* grounds — its own
Decision 1 says `/lines/[id]` "already works end-to-end … with no
special-casing needed" but carries edit/delete management chrome, trend
charts and a filter/accordion `IssueList` that do not suit a single-purpose
link target — **not** because no destination existed. So `/tfl-lines/[id]`
is a preference, not a prerequisite for anything in Section 5.

What genuinely is missing from that plan is smaller and worth keeping on
the list: `tflLineIdFromSource`, which would close the dead end at
`frontend/lib/incidents.ts:11-14` where a TfL `Disruption.source` resolves
to no link at all, even though it carries a line id that would resolve
fine.

### 2f. The `incidents` table, and what a TfL disruption would have to fit into

The current 22-column shape, assembled from
`20260510023522_initial.sql:16-27`, `20260706004003_reference_data.sql:55-65`,
`20260716180000_incident_first_seen.sql:10-11`,
`20260820120000_incident_extraction.sql:9-19`,
`20260821090000_incident_severity_escalation.sql:13-17`,
`20260822090000_incident_extraction_periods.sql:21-22`, and
`20260912090000_incidents_first_seen_at_id.sql` (index only), reduced to
the eleven columns the archive reads plus the enricher block (which it does
not read, listed because a TfL row would have to carry them as `NULL`):

| column | today | a TfL disruption |
| --- | --- | --- |
| `incident_id TEXT PK` | RDM's own `IncidentNumber`, stable for the incident's whole lifecycle | **nothing corresponds** (Section 2c) |
| `summary TEXT NOT NULL` | short RDM prose | `statusSeverityDescription`, or the first clause of `reason` — neither is a summary TfL wrote as one |
| `description TEXT NOT NULL` | RDM HTML, sanitized on render | `reason`/`disruption.description`, plain text; often a compound multi-segment string (2026-09-07 spec §3) |
| `operators TEXT[]` | ATOC codes | the literal `"TfL"` for every line, tube to tram (`crates/common/src/lib.rs:230-235`) — one value, zero selectivity |
| `affected_stations TEXT[]` | CRS codes — empty on every production row (§1c) | Naptan ids, dropped at the poller (`schema.rs:136-145`) |
| `priority INTEGER NOT NULL` | raw RDM `IncidentPriority`, no documented enum | `statusSeverity` 0–20, a *different* scale — identical to this app's `Severity` for codes 0–14 and divergent above (TfL 15 is Diverted where ours is 21; TfL 20 is Service Closed where ours is the NR extension Recovering), which is why `severity_from_tfl_code` exists to translate rather than pass through (`crates/common/src/lib.rs:191-231`) |
| `validity_periods JSONB` | repeated RDM periods | TfL `validityPeriods[]`, already collapsed to one by `select_validity` (`schema.rs:186-209`) — the only column with a clean analogue |
| `is_planned BOOLEAN` | RDM `Planned` | derivable from `disruption.category == "PlannedWork"`, but `Information` maps to neither value |
| `is_cleared BOOLEAN` | RDM `ClearedIncident`; the feed retains cleared incidents for a time | **nothing corresponds** — a TfL disruption simply stops appearing in the next poll |
| `first_seen_at` / `fetched_at` | this app's own clock | derivable |
| `extracted_*` (enricher) | populated for Knowledgebase incidents | permanently `NULL` — the enricher reads the Knowledgebase text pipeline only |

There is **no country, region, mode or network column on `incidents`, and
no proxy for one.** (Country filtering elsewhere in this app is
frontend-only and currently inert:
`frontend/lib/modes.ts:67`'s `MODE_TO_COUNTRY` is deliberately an empty
record, so `countryForMode` returns `'Gb'` for everything — per
`docs/superpowers/specs/2026-09-05-country-filtering-design.md`'s own
Decisions 2–4. The only network-ish column in the schema is `network` on
the Ireland reference tables, which never joins to `incidents`.)

Reading that table downward: of the eleven columns the archive surfaces,
**two have no TfL analogue at all (`incident_id`, `is_cleared`), three
would be actively misleading if populated (`operators`, `priority`,
`affected_stations`), and one (`summary`) would have to be invented.**

## 3. The four structural mismatches, stated once

Everything in Sections 4–6 follows from these. Each is verified above, not
assumed.

1. **No stable identity.** TfL asserts no continuity between polls
   (§2c). The archive's primary key, its detail-page URL, its
   `incident_history` diff, and its keyset cursor's tiebreak all require
   one. This is the 2026-09-07 spec's Section 2 finding, unchanged and
   re-verified; that spec recommended against synthesizing an identity
   (Option A) on honesty grounds, and nothing since has changed the input
   data.
2. **No lifecycle.** `is_cleared` is a fact RDM publishes. TfL publishes a
   complete re-statement of current status every 300s; "cleared" is
   inferred from absence, which is indistinguishable from a rename or the
   line being dropped from the polled mode set. **Narrower than it first
   looks, and deliberately so**: the whole-feed-outage case is already
   guarded, on both sides — `upsert_tfl_line_status` returns `Ok(0)` on an
   empty batch rather than mass-deleting
   (`crates/api/src/data/queries.rs:388-390`, whose doc comment reads
   *"'TfL returned nothing' is a fault"*), and
   `crates/poller-tfl/src/main.rs:158-163` refuses to post one. The
   residual ambiguity is per-line-within-a-non-empty-batch, and the same
   guard pattern would carry into anything built here.
3. **No archive depth, today.** Seven days (§2d), with no upstream history
   endpoint to backfill from. Two distinguishable problems live under this
   heading and should not be conflated: a **retention policy** (7 days,
   changeable by decision, though it needs a `source` column on
   `line_status_history` first, since `prune_history` is source-blind) and
   a **cold start** (a new table would begin empty and could never recover
   the past, exactly as `incidents` could not at its own first ingest).
   Only the second is structural.
4. **No shared vocabulary for search.** The archive's filters are
   ATOC operator codes, CRS-resolved catalogue lines, `is_planned`, and a
   raw RDM priority integer. TfL has a single pseudo-operator, Naptan
   stops that are dropped before storage, a three-valued category, and a
   different severity scale. A combined search surface would have filters
   that are inert for one half of the rows and filters that are inert for
   the other.

Only mismatch 1 is both structural and unavoidable: it is a property of
TfL's API, and every design that wants disruption-shaped rows has to paper
over it. Mismatch 2 is real but narrow. Mismatch 3 is half policy, half
cold start. Mismatch 4 is a design problem with real solutions (Section 5).
**The important consequence is that mismatch 1 only binds a design that
tries to build disruption-shaped rows** — Option D in Section 5 sidesteps
it entirely by staying snapshot-shaped, which is why it is the cheapest
option and why an earlier draft of this document, which omitted it,
reached its recommendation for partly wrong reasons.

## 4. Scope

Scope has two axes — which modes, and which *capability* — and §1 and
§2d-bis narrowed both.

**Modes in scope:** TfL modes with no National Rail counterpart — **tube,
DLR, tram**. Secondarily, the TfL-sourced side of the seven merged railways
(Elizabeth line, six Overground lines), whose NR side is already archived
(§1d).

**Capability in scope: a *cross-line* browse, and only that.** Per
§2d-bis, a TfL line's current disruption and its seven-day history are both
already persisted, rendered and reachable, per line. So this document is
not asking "can TfL disruptions be shown at all" — they are — but the
narrower "can they be browsed across lines the way `/incidents` browses
National Rail incidents across the network." Every option in Section 5 is
costed against that narrower question, and §9 item 4 records that nobody
has actually confirmed it is the ask.

**Not in scope, and why:**

- **`bus`, `river-bus`, `cable-car`, coach.** Never polled
  (`crates/poller-tfl/src/config.rs:25-26`); bringing them in is a
  `poller-tfl` scope change, not an archive question.
- **"TfL Rail" as a separate mode.** Does not exist (§2a) — it is the
  Elizabeth line, already covered on the NR side.
- **Non-TfL UK light rail** (Metrolink, Tyne & Wear Metro, West Midlands
  Metro, Supertram, Glasgow Subway, Edinburgh Trams, NET). Covered by
  `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`;
  none is ingested today.

## 5. The four options, costed (A and B in design detail; C and D in outline)

Four options were considered — A and B build disruption-shaped rows, C and
D do not. Each is costed honestly; Section 6 recommends against building an
archive from any of them now, while pulling two cheap pieces out of C and
naming D as the shape to build if the decision is reversed.

### Option A — put TfL disruptions in the `incidents` table

Synthesize an `incident_id` (e.g. `tfl-{line_id}-{hash(reason)}`), write
TfL disruptions into `incidents` from a new branch of
`upsert_tfl_line_status`, and let the existing route, index, cursor and
frontend serve them unchanged.

**Rejected.** It inherits the 2026-09-07 spec's Option A identity problem
in full (reworded prose reads as a new incident; compound multi-segment
strings re-hash whenever any one clause changes) and adds three costs that
spec did not have to weigh:

- Six of the eleven columns the archive surfaces become null, constant, or
  a lie for half the rows (§2f). `operators = ['TfL']` in particular
  destroys the operator filter's selectivity: one value would cover every
  tube, DLR and tram row, and it would not appear in the Operator
  dropdown at all, because that list comes from RDM's TOC feed
  (`GET /public/tocs/all`), which has no reason to carry a TfL pseudo-code.
- `incidents` is a table every column of which is RDM-shaped, read by the
  aggregator's matcher, the enricher's sweep
  (`crates/enricher/src/sweep.rs:42`, `SELECT ... WHERE NOT is_cleared`),
  the detail route, and the freshness route. Rows that satisfy none of
  those readers' assumptions would have to be excluded from each, one
  `WHERE source <> 'tfl'` at a time — including from the enricher, which
  would otherwise spend LLM calls extracting temporal periods from
  "Good Service".
- It would make the `incidents` retention gap — still open, still
  unaddressed, re-verified for this document: no `prune_incidents` exists
  anywhere — worse by adding a source that writes a new synthetic row
  every time TfL rewords a sentence.

### Option B — a parallel `tfl_disruptions` archive, surfaced beside `/incidents`

A new table and ingest path, with its own search route and its own filter
vocabulary, presented on `/incidents` as a second tab or a mode toggle
rather than blended into one result list.

This is the only shape that could work, and it is worth writing down
concretely.

**Schema** (not proposed for implementation):

```sql
-- NOT proposed. Sketched to make Option B's cost comparable to Option A's.
CREATE TABLE tfl_disruptions (
    disruption_id   TEXT PRIMARY KEY,   -- synthetic: hash(line_id, normalized reason)
    line_id         TEXT NOT NULL,      -- 'tfl-'-prefixed, as in line_status
    mode_name       TEXT NOT NULL,      -- tube | dlr | tram | overground | elizabeth-line
    severity        SMALLINT NOT NULL,  -- common::Severity, NOT raw statusSeverity
    category        TEXT,               -- RealTime | PlannedWork | Information
    reason          TEXT NOT NULL,
    valid_from      TIMESTAMPTZ,
    valid_to        TIMESTAMPTZ,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_current      BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE INDEX tfl_disruptions_first_seen_at_id
    ON tfl_disruptions (first_seen_at DESC, disruption_id DESC);
CREATE INDEX tfl_disruptions_mode ON tfl_disruptions (mode_name);
```

`first_seen_at`/`last_seen_at`/`is_current` replace `is_cleared`: they are
honest about what the data supports ("this text was last seen at T"), where
`is_cleared` would assert something TfL never said. The keyset index
mirrors `incidents_first_seen_at_id` exactly so the cursor mechanics are
the same code shape.

**Ingest.** A new branch inside `upsert_tfl_line_status`
(`crates/api/src/data/queries.rs:387-457`), in the same transaction that
already diffs and writes `line_status`: for each status entry with a
`disruption`, compute the synthetic key, `ON CONFLICT DO UPDATE
last_seen_at = NOW()`, and mark `is_current = FALSE` for any row of a line
in this batch whose key did not recur. This reuses the existing
`normalize_for_diff` discipline and adds no new poller.

**Search route.** `GET /public/tfl-disruptions`, a sibling of
`GET /public/incidents` in a new `crates/api/src/routes/tfl_disruptions.rs`,
copying `routes::incidents`'s conventions verbatim — `#[serde(deny_unknown_fields)]`,
base64url-no-pad keyset cursor, `nextCursor` explicit `null`, clamped
`limit`, 400 on a malformed cursor. Its filters differ because its data
does: `mode` (comma-separated, over the closed `SUPPORTED_MODES` set),
`line` (a `tfl-` line id, exact match, not a station approximation),
`category` (`RealTime`/`PlannedWork`/`Information`), `severity_min`/
`severity_max` (over `common::Severity`, which unlike `priority` has a
documented ordering), `from`/`to`, `current`, `after`, `limit`.

**Frontend.** A `SegmentedControl` at the top of `/incidents` switching
between "National Rail" and "TfL", each rendering its own filter set
against its own route — **not** one blended list. Blending is the thing to
avoid: a single result list whose rows carry mutually inapplicable badges
(an ATOC operator badge on a tube row, a "TfL" badge on every tube row) and
whose filter panel greys half out per row would be worse than two honest
lists. Rows would link to `/lines/{tfl-line-id}`, which already exists and
already renders that line's disruption (§2d-bis), or to the 2026-09-07
plan's narrower `/tfl-lines/[id]` if that is built first — either works;
neither blocks the other.

**Why Option B is not recommended *now*** — four reasons, in order of
weight:

1. **The identity problem is unsolved, not relocated.** Moving the
   synthetic key out of `incidents` and into `tfl_disruptions` removes the
   schema-pollution objection but not the 2026-09-07 spec's actual
   argument, which was about truthfulness: a row with a `first_seen_at` and
   a stable URL asserts "this is one tracked real-world event," and TfL's
   feed gives no basis for that assertion. `is_current`/`last_seen_at`
   soften the claim; they do not establish it.
2. **Option D gets most of the value for a fraction of the cost.** A
   cross-line browse over `line_status_history` (below) needs one index
   and no new identity concept. Option B's entire additional contribution
   over Option D is disruption-shaped rows — which is precisely the part
   mismatch 1 says cannot be made truthful. Paying for a new table, a new
   ingest branch and a new lifecycle to buy the one part that is unsound
   is the wrong trade.
3. **It would start empty and could never be backfilled.** No TfL history
   predating the feature exists in a form the new table could ingest, and
   TfL publishes no history endpoint. An archive that answers "what
   happened on the Victoria line last month" with silence for its first
   month is a different product from the one `/incidents` is. (Note this
   is the cold-start half of mismatch 3, not the retention half —
   `prune_history` would not be the constraint, since the new table would
   have its own knob.)
4. **The equivalent feature for National Rail is half-broken.** Its Line
   filter matches nothing (§1c, confirmed live). Fixing a live defect in
   the shipped feature outranks doubling its surface area.

### Option C — do nothing to the archive; close the remaining TfL dead end

Ship the 2026-09-07 plan (the `/tfl-lines/[id]` page), and leave
`/incidents` National-Rail-only with honest copy saying so.

**Weaker than an earlier draft of this document claimed, and worth saying
so plainly.** That draft sold Option C as (a) giving TfL disruptions a
destination and (b) producing the demand signal Option B lacks. Both
collapse on inspection:

- The destination already exists — `/lines/{tfl-line-id}` (§2d-bis).
- The v1 plan's own Decision 2 puts the history section **out of scope for
  v1** ("ship 'current status only'"), so the page it describes could not
  generate evidence about demand for *history* even in principle. And the
  surface that does show TfL history already ships, so any signal it
  produces is already available.

What Option C genuinely adds is narrower: a link-target-shaped page without
`/lines/[id]`'s management chrome, and `tflLineIdFromSource` closing the
`frontend/lib/incidents.ts:11-14` dead end. Both worth doing; neither is an
archive, and neither unblocks one.

### Option D — a cross-line search over `line_status_history`, as it already exists

**The cheapest option, and the one an earlier draft of this document
omitted — its absence was a real hole in the analysis, not a judgment
call.** §2d-bis establishes that TfL disruption history is already
persisted, already rendered, and already browsable *per line*, for seven
days. The only thing `/incidents` does that `/lines/{id}/history` does not
is drop the line filter. So: a route that reads `line_status_history`
across lines, newest-first, keyset-paginated, filtered by mode, line set
and time range.

**No new table. No new ingest path. No synthetic per-disruption identity —
so mismatch 1, the one structural blocker that sinks Options A and B, never
arises.** Rows are snapshots, which is what the data actually is, and each
links to `/lines/{id}` (which exists) or `/lines/{id}/history` (which
exists).

Its real costs, which are not zero:

- **A new index.** `line_status_history`'s only index is
  `(line_id, computed_at DESC)` (`20260510023522_initial.sql:96`). A
  cross-line, time-ordered keyset scan has nothing supporting it; it would
  need `(computed_at DESC, id DESC)` or similar. One migration, same shape
  as `incidents_first_seen_at_id`.
- **No `source` column** (§2d), so "TfL only" is a `line_id LIKE 'tfl-%'`
  prefix predicate — workable but ugly, and the reason to add the column
  properly rather than pattern-match an id convention.
- **Seven days deep** — and extending that is coupled to the previous
  bullet. As things stand `prune_history` is source-blind, so raising the
  depth means raising `history_retention_days` for every line, National
  Rail included, on a table written every aggregation cycle for 109+ lines
  — a storage decision with blast radius well beyond TfL. **Adding the
  `source` column decouples it**: `prune_history` could then keep TfL rows
  longer than aggregator rows, making TfL depth a cheap, contained
  decision. These two bullets are one piece of work, not two, and doing
  them together is what makes Option D's depth adjustable at all.
- **A new route, query, cursor and frontend surface** — the same
  components Option B needs, minus the table and the ingest branch. Not
  free, and not counted as free here just because the storage is free:
  a `search_line_status_history`-shaped query, keyset encode/decode
  mirroring `routes::incidents`, and a results surface on `/incidents` or
  its own page. Roughly Option B's cost minus its riskiest third.
- **Volume — but not, on inspection, the spamminess research's actual
  failure mode.** One tube line produced 229 status recomputes in seven
  days (§2d-bis), so a tube+DLR+tram browse is plausibly low thousands of
  rows a week. It is tempting to cite
  `docs/superpowers/specs/2026-09-02-line-history-list-spamminess-research.md`
  as showing that list is known to read badly, and an earlier draft of this
  document did. **That citation does not transfer, and saying so is the
  difference between a blocker and a cost.** That research's primary root
  cause is specifically LDBWS-sample-derived `reason` text —
  `infer_from_samples`' per-cycle counts and its `"(most cited: …)"` suffix
  — churning on every aggregation cycle for what is one ongoing situation,
  and it carries its own scoping note that *incident*-derived spans are not
  broken that way. TfL rows are neither: they are written by
  `upsert_tfl_line_status` behind `tfl_statuses_changed`/`normalize_for_diff`
  (`crates/api/src/data/queries.rs:342-373`), over TfL's own prose, which
  does not carry per-cycle numbers. The observed 229 writes against ~2,000
  polls in the same week is that diff guard working, not defeating itself.

  What remains is ordinary volume plus genuine severity oscillation during
  real incidents (§2d-bis's "severity changed 24 times" spans one hour of
  an actual line suspension — real events, not text churn). The existing
  Timeline already collapses that, so Option D inherits a working
  mitigation. **The one thing it would not inherit is any guarantee the
  collapse still reads well once 15+ lines interleave**, since the existing
  grouping is per-line-per-day by construction. That is the specific,
  narrow question to answer — not "is this list inherently spam".

Option D is snapshot-shaped, not disruption-shaped: it can answer "what
were TfL's lines reporting on Tuesday afternoon" but not "show me that
signal failure as one thing." Whether that is the product anyone wants is
the open question — but it is a question about *value*, answerable by
asking, rather than a question about feasibility.

## 6. Recommendation

**Not yet — do not build a TfL incident archive.** But the gap this
document was asked to close turns out to be smaller than the brief
supposed, and two of the four things worth doing about it are cheap. In
order:

**1. The briefed assumption is confirmed; treat the remaining gap as
tube/DLR/tram only.** §1b verified against the live deployment that `XR`
and `LO` Knowledgebase incidents are in the archive today and are reachable
through its Operator filter. The archive already covers the two TfL-branded
railways most people mean when they say "TfL". No work is required here —
this item exists to record that the question is closed, not open.

**2. Fix the archive's Line filter, or make its emptiness honest — this is
the most urgent item in this document, and it is not a TfL problem.**
`affected_stations` is empty on every production row, and the Line filter
returns zero rows for every line (§1c, confirmed live). Two defensible
fixes, neither designed here: populate the column (parse
`Affects.RoutesAffected`, or write the aggregator matcher's
`evidence.stations` back), or replace the filter with one backed by real
data (the catalogue line's `operators`, which would at least return the
`OperatorOnly` tier honestly). Either needs its own spec. Until one lands,
the Line control should say what it does — an empty result there is
currently indistinguishable from "this railway is not archived," which is
exactly the wrong lesson for someone testing Elizabeth line coverage, and
is how this whole question is most likely to be asked again.

**3. Close the TfL link dead end, cheaply.** Add `tflLineIdFromSource`
(`frontend/lib/incidents.ts`) so a TfL `Disruption.source` resolves to
`/lines/{tfl-line-id}` — a page that already exists and already renders
that line's disruption (§2d-bis). This is a few lines, not the 2026-09-07
plan's whole `/tfl-lines/[id]` page, which remains optional polish rather
than a prerequisite for anything (§2e).

**4. If a cross-network TfL browse is genuinely wanted, build Option D,
not Option B — but answer one question first.** Option D reads
`line_status_history` as it already exists: one index, no new table, no
synthetic identity, rows linking to pages that already work. Its blocker is
not feasibility but value, and specifically volume: 229 status recomputes
for one tube line in one week (§2d-bis). **The question to answer before
building it is whether the existing Timeline's churn-collapsing still reads
as useful once 15+ lines interleave**, since that collapsing is per-line
per-day by construction. Looking at `/lines/{id}/history` for a few TfL
lines side by side answers it more cheaply than a prototype.

**Do not build Option A (TfL rows inside `incidents`) at all**, and treat
Option B (a parallel `tfl_disruptions` table with synthetic identity) as
gated on TfL publishing a stable disruption id, or on an explicit product
decision to accept fuzzy identity framed to the user as "probably the same
disruption" rather than asserted as fact. The 2026-09-07 spec made that
call on honesty grounds and flagged it as reversible by someone with more
context on how the request arose; that remains true, and nothing in the
input data has changed since. Section 5 sketches Option B concretely enough
to plan from should it be reversed.

**One thing that must be decided before either D or B, and is not decided
here:** retention. TfL disruption history lives for seven days (§2d).
Option D inherits that number and cannot change it without changing it for
every National Rail line too, on a table written every aggregation cycle.
Option B would need its own knob. Note that `incidents`/`incident_history`
still have *no* pruning at all — re-verified: no `prune_incidents` exists
anywhere — so "match what `incidents` does" is not an available answer.

## 7. Non-goals

- **Designing the fix for `affected_stations`.** Named in §1c as a real,
  live defect of the shipped archive and recommended in §6 item 2; parsing
  `Affects.RoutesAffected`, or persisting the aggregator matcher's
  `evidence.stations`, is a separate feature with its own accuracy and
  backfill questions.
- **Designing retention for `incidents`/`incident_history`.** Still absent
  (re-verified: no `prune_incidents` exists in `crates/aggregator`), still
  scoped out, exactly as the 2026-09-12 spec scoped it out.
- **Designing retention or a `source` column for
  `line_status_history`.** Named in §2d and §5 Option D as coupled and as a
  precondition for adjustable TfL depth, and in §6's closing note; not
  designed here.
- **Re-litigating the 2026-09-07 spec's Option A vs. Option B call.** This
  document re-verified its inputs and found them unchanged; it does not
  re-open the decision.
- **Segment-level parsing of TfL's compound status text.** Deferred by the
  2026-09-07 spec's Section 3 and still deferred. It bites Option B hardest
  (a hash-keyed identity degrades further on compound text) and Option D
  not at all (a snapshot list renders the string verbatim, as
  `/lines/[id]`'s `IssueList` already does) — one more reason to prefer D.
- **Designing Option D's route, index or UI in detail.** §5 establishes it
  is the cheapest shape and names its two real costs (a new index, the
  spamminess risk at network scale); it is not designed to
  implementation-readiness here, because §6 item 4 puts a value question
  ahead of it that this document cannot answer alone.
- **Naptan↔CRS reconciliation.** Would only become necessary if a TfL
  archive wanted station-level filtering; Option B's `mode`/`line` filters
  deliberately avoid needing it.
- **Any change to `poller-tfl`'s polled mode set.** Bus, river bus and
  cable car stay out (§4).
- **Full-text search over TfL `reason` text.** No `tsvector`/`pg_trgm`
  index exists on any disruption text column today, the same as for
  `incidents.summary`/`description`; adding one is a separate indexing
  decision for both halves at once, not a TfL-specific one.
- **Merging the two archives into one blended result list.** Rejected on
  its merits in §5 Option B, not merely deferred.

## 8. Testing approach (if Option D or B is ever built)

Recorded so a future plan does not have to re-derive it. Nothing here is a
task list for now. Split by option, since §6 recommends D over B and the
two need substantially different coverage.

**Option D (the recommended shape, if anything is built):**

- `crates/api/src/data/queries.rs`: a cross-line
  `search_line_status_history`-shaped query — no-filter ordering
  (`computed_at DESC`, tie-broken on the surrogate `id`), the mode/line
  filters, `from`/`to` inclusivity, and the keyset
  pages-without-gaps-or-repeats case across a `computed_at` tie, modelled
  on
  `search_incidents_keyset_pagination_pages_without_gaps_or_repeats_and_breaks_ties_on_incident_id_desc`.
- A regression test that the TfL/aggregator split is done on a real
  predicate and not a `line_id` prefix — i.e. that it still works for a
  hypothetical non-`tfl-`-prefixed TfL line id. This is the test that
  forces the `source` column rather than letting `LIKE 'tfl-%'` calcify.
- Pruning: `prune_history` with a source-aware retention keeps TfL rows
  past the aggregator cutoff and still deletes them at their own. The
  guard against a regression here is that TfL rows are currently deleted
  by a job that never wrote them (§2d).
- Frontend: the day-collapsing that already works per line still collapses
  correctly when rows from several lines interleave — the one behavior
  §5 Option D names as genuinely untested at this shape.

**Option B (only if the identity decision is reversed):**

- `crates/api/src/data/queries.rs`: `search_tfl_disruptions` mirroring
  `search_incidents`'s existing suite — no-filter ordering, each filter in
  isolation, AND semantics across filter kinds, and the keyset
  pages-without-gaps-or-repeats case over a `first_seen_at` tie, modelled
  on
  `search_incidents_keyset_pagination_pages_without_gaps_or_repeats_and_breaks_ties_on_incident_id_desc`.
- The identity mechanism needs its own tests, and they are the ones that
  would actually decide whether Option B is shippable: a reworded `reason`
  for an ongoing disruption must not produce a second row; two genuinely
  distinct disruptions with similar prose must not collapse into one; a
  compound multi-segment string whose first clause recovers must not
  re-key the whole row. These are the 2026-09-07 spec's Section 2 failure
  modes turned into assertions, and if they cannot be made to pass against
  captured real TfL responses, that is the answer to the whole question.
- `upsert_tfl_line_status`: a disruption absent from the next batch flips
  `is_current` to `FALSE` and leaves `first_seen_at` untouched; an
  unchanged batch writes no new row (reusing `normalize_for_diff`'s
  existing discipline).
- Frontend: the mode toggle renders each tab's own filter set and never
  shows an ATOC operator control on the TfL tab; empty and error states
  per tab, matching `IncidentSearchForm`'s existing branches.

## 9. Open questions / risks

1. ~~**Does RDM's Knowledgebase feed actually publish `XR` and `LO`
   incidents?**~~ **Closed.** Checked against the live deployment during
   this document's review pass and confirmed: both codes return real rows
   (§1b). Left in the list, struck through rather than deleted, because it
   was this document's highest-priority open question and the fact that it
   is now answered changes §6's framing.
2. **Was the archive's `line` filter ever exercised against production
   data?** Its tests seed `affected_stations` directly
   (`crates/api/src/data/queries.rs:2154`), so they pass while the
   production behavior is empty — a real gap between test fixtures and the
   ingest path, and worth checking whether other features depend on that
   column the same way.
3. **How much TfL disruption history exists right now?** Bounded above by
   7 days (§2d) but not measured — the live checks in §1b/§1c/§2d-bis went
   through public HTTP routes, not the database, so row counts and the
   real configured retention were not observed. If `history_retention_days` is overridden in the real
   deployment, the ceiling differs — the value is duplicated between
   `crates/aggregator/src/config.rs:22-24` and
   `crates/api/src/data/config.rs:183-197`; `docker-compose.yml` and the
   Helm chart both source the two env vars from one value, but nothing in
   the code enforces it, as that second doc comment says itself.
4. **Is "TfL in the archive" actually the ask?** Given §2d-bis — a TfL
   line's current disruption and its seven-day history are both already
   browsable — the residual ask is specifically a *cross-line* view. Nobody
   has said that is what they want; it is this document's inference from
   the brief. Worth confirming before §6 item 4 is acted on, because if the
   real ask was "I want to see TfL disruptions at all", it is already met.
5. **Would a synthetic TfL identity survive contact with real data?** The
   2026-09-07 spec's failure modes were reasoned from one captured example
   and TfL's documented behavior, not measured against a corpus. Before
   anyone builds Option B, capture a week of `/Line/Mode/{modes}/Status`
   responses and measure how often `reason` text changes without a
   real-world event changing. That measurement, not an argument, should
   decide it.

## Self-review notes

- Placeholder scan: no `TODO`/`TBD`/bracketed placeholder text remains.
- Every file, function, line number, constant and migration named above
  was read directly in this repository on 2026-09-16. Where a prior spec's
  claim is repeated, it was re-verified rather than cited on trust — this
  turned up two things prior specs got right at the time and are now stale
  about: the 2026-08-22 spec's "no `lines/overground-*.toml` files exist"
  (they do now, and the merge shipped for all seven), and the 2026-09-12
  spec's implicit assumption that `incidents.affected_stations` carries
  data (it does not).
- The brief's assumption is answered explicitly in §1 with a verdict
  sentence rather than left implicit: it holds at the code level, it holds
  against live production data (§1b), and it is nonetheless contradicted by
  user-visible behavior for an unrelated reason (§1c).
- **This document was materially wrong in an earlier draft and the
  corrections are marked in place rather than quietly absorbed.** An
  independent review caught three: that TfL disruptions already have a
  destination page and a seven-day per-line history browse
  (`/lines/{tfl-id}` and `/lines/{tfl-id}/history`, both verified returning
  200 with live content) where the draft claimed there was "nowhere to
  link"; that the 2026-09-07 plan's page is therefore a preference, not a
  prerequisite; and that the options analysis had omitted its own cheapest
  option, Option D. A second review round then caught that Option D's own
  headline objection was miscited — the spamminess research's diagnosed
  root cause is LDBWS-sample-derived text, which does not reach TfL rows —
  and that its retention and `source`-column costs are one piece of work,
  not two. §2d-bis, §2e, §5 Option C/D, §6 and §8 all carry the
  corrections explicitly, because a design document that silently repairs
  its own reasoning is harder to trust than one that shows the repair. Net
  effect across both rounds: Option D looks cheaper and less risky than the
  first draft implied, which is an argument for acting on §6 item 4 sooner,
  not later.
- The recommendation is "not yet" for an archive, with four ordered,
  actionable items rather than a vague deferral, and the alternatives it
  declines are still costed (§5 — Option B in design detail, Option D in
  outline) so overturning it does not mean starting over — the same structure
  `docs/superpowers/specs/2026-09-12-group-lines-design.md` used. Two of
  the four items (2 and 3) are things to do *now*, so this is not a "no"
  wearing a schedule.
- Scope check against the brief: TfL modes actually in scope are named and
  narrowed to three with evidence (§2a, §4), including the explicit finding
  that "TfL Rail" is not a separate mode; whether TfL data can support a
  persistent searchable archive is answered directly (§3, §5); the schema
  and ingest changes that would be needed are given concretely (§5 Option
  B); the search/filter UX adaptation is designed (§5 Option B, frontend);
  and the recommendation is stated as "not yet" with the conditions that
  would change it, rather than forced into a "yes".
- Claims about live behavior are marked as such and were actually checked
  against https://ds.cursed.solutions on 2026-09-16 (§1b, §1c, §2d-bis,
  §2e) — the API assertions by querying `/api/incidents` directly, and
  §2d-bis by rendering `/lines/tfl-victoria/history` in a real browser
  rather than trusting the served HTML. Claims that remain unmeasured —
  total row counts, the deployment's real `history_retention_days` — are
  named in §9 rather than quietly assumed.
