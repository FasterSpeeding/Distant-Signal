# Design: Ingestion-Frequency Metrics — "Is Data Actually Arriving," Not Just "Is the Pod Up"

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass.**

## What was asked for, paraphrased

The repo owner wants Grafana to show, for every data source `distant-signal`
ingests, how regularly new data is actually arriving — historically, so a
question like "did the incidents feed go quiet for 3 hours last Tuesday" or
"is the schedule feed still landing daily" can be answered after the fact.
`up{}` (already dashboarded per
`docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md`'s
merged dashboard `ConfigMap`) proves the pod's process is alive; it says
nothing about whether that process is successfully ingesting anything.

This document audits what already exists before proposing anything new, per
this repo's own established practice (the Grafana work itself reused the
existing `up{}` metric for uptime rather than inventing one — see that
plan's Panels 5/6, `distant-signal-status.json` ids 5/6). The finding,
stated up front: **of the app's ~11 distinct ingestion paths, three need
zero new work (dashboard panel only), one needs a small new query exposed
as a gauge, one needs a genuinely new signal distinct from what exists
today, and the rest already have a working answer via existing counters.**
No source gets a new mechanism invented without first showing why the
cheaper existing-data option does or doesn't already cover it.

## Relationship to prior work — why this isn't the same question the last document already answered

`docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md`'s
"When data was last received" section (line 425) already considered, and
explicitly rejected, putting freshness data in Grafana: *"a Grafana panel
could technically show `time() - <a timestamp gauge>` for the same data, but
doing so would mean (a) inventing new gauge metrics for data that already
has a perfectly good source-of-truth column in Postgres, (b) putting a
genuinely public-interest fact... behind a tailnet-only Grafana instance
real users can never reach, and (c) duplicating... the prior document's own
already-designed extension."* That call stands, unmodified, for the
question it was answering: **"what should the public frontend show a site
visitor about current data freshness right now."** `/public/freshness`
remains exactly right for that — nothing here changes it, extends it, or
asks the frontend to call Prometheus.

The question this document answers is different in kind, not degree:
**"as an operator investigating an incident after the fact, can I see how
this source's freshness *changed over time*."** `/public/freshness` is a
single current-value snapshot with no history — it cannot answer "did this
go quiet for three hours last Tuesday" no matter how often it's polled,
because nothing retains the samples. That is precisely what Prometheus is
for (this is the same reasoning the Grafana respec used to reject a new
Postgres-backed metrics store in its own Decision 1 — reuse a time-series
system that already exists rather than build one). Concern (b) doesn't
apply here — this data is for the tailnet-only operator dashboard, not the
public page. Concern (c) doesn't apply — nothing here touches
`frontend/components/DataFreshnessInfo.tsx` or `getDataFreshness`. Concern
(a) is the one real tension, addressed head-on in Decision 1 below: yes,
this does add gauge metrics that mirror a Postgres column — that
duplication is the entire point, because Prometheus's value-add over the
column is retention and `rate()`/graphing, not the value itself.

## Audit: every ingestion path, and what already answers "how regularly is this arriving"

### Already fully covered — no new work, dashboard panel only

**Trust/movement events** (`crates/movement-relay`, `crates/trust-consumer`,
`crates/full-coverage-consumer`). Confirmed still true on a fresh read:
`crates/movement-relay/src/main.rs:96-97` increments
`distant_signal_movement_relay_events_published_total`;
`crates/trust-consumer/src/process.rs:322-328` increments
`distant_signal_trust_consumer_events_received_total`;
`crates/full-coverage-consumer/src/main.rs:382-383` increments
`distant_signal_full_coverage_consumer_events_matched_total`, each tagged
per relevant label (`msg_type`, `line_id`). `rate()` over any of these
directly answers "how regularly is this arriving," and a gap or flatline in
the rate directly answers "did this go quiet." **These three already have
panels** — dashboard ids 1-4 in the merged `distant-signal-status.json`
(`docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md`
lines ~975-1012). Nothing new needed for cadence; Decision 6 below only
proposes a threshold/alerting refinement, not a new metric.

**Stations and TOCs reference data** (`poller-stations`, `poller-tocs`).
`crates/api/src/data/queries.rs:225-255` (`upsert_stations`) and `:511-541`
(`upsert_tocs`) both write `fetched_at = NOW()` unconditionally, on every
successful upsert, whether or not any column value changed. For these two
sources, that unconditional tick is *not a bug to fix* — it's the right
question. RDM Stations/TOC reference data is expected to rarely change
content-wise; the actual operational question is "did the daily poll job
still run and successfully write," not "did a station's name change,"
because the latter is a rare, uninteresting event and the former is exactly
what a silent poller failure would break. `crates/api/src/routes/
freshness.rs`'s existing `last_stations_fetch`/`last_tocs_fetch` queries
(`queries.rs:552-566`) already read exactly this. **No new instrumentation
needed** — see Decision 1 (export, don't re-derive).

### Needs a small new query, not new producer instrumentation

**Incidents** (`poller-incidents` → `api`'s `/private/incidents`). This is
the one source where "cycle succeeded" and "new data arrived" are
genuinely, provably different signals, read directly from the upsert code:

- `crates/poller-incidents/src/main.rs:66-79`: every scheduled cycle fetches
  the whole current feed, parses it, and POSTs it — `poller_cycle_total`
  increments on `result="success"` whether the feed content changed,
  stayed exactly the same, or came back empty. The poller does no
  diffing of its own.
- `crates/api/src/data/queries.rs:106-143` (`upsert_incidents`): `fetched_at
  = NOW()` is written unconditionally on every incident row, every cycle,
  regardless of whether `incident_changed` (line 40) says anything is
  different. So `last_incidents_fetch`
  (`queries.rs:568-574`, MAX(fetched_at)) — and therefore
  `/public/freshness`'s `incidents` field — reflects "the poller last
  successfully executed," identically to what `poller_cycle_total`'s rate
  already shows. It does **not** reflect "the Knowledgebase incidents feed
  is still actively producing content." A poller that keeps succeeding
  against a silently-stale upstream feed (same incidents, unchanged, every
  cycle) would show "fresh" forever on both signals — exactly the failure
  mode the repo owner's "did the incidents feed go quiet for 3 hours"
  example is worried about.
- The good news: the real diff signal **already exists in the database**,
  unused by any metric or public endpoint today.
  `upsert_incidents` (`queries.rs:106-168`) writes a row to
  `incident_history` (`recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`,
  `crates/api/migrations/20260510023522_initial.sql`'s `incident_history`
  table) **only when `incident_changed` is true** — and `incident_changed`
  (`queries.rs:40-51`) returns `true` for a brand-new incident too, not
  only a modified one. `MAX(recorded_at) FROM incident_history` is
  therefore exactly "when did the incidents feed last actually produce new
  or changed content" — the signal the motivating example needs — with
  zero schema change and zero new producer-side instrumentation. See
  Decision 2.

### Already has a genuinely per-delivery record — needs api-side gauge, not new instrumentation

**Schedule feed** (`schedule-ingest` → `api`'s `/private/schedule-feed-
ingests`). Two existing signals, with different failure modes:

1. `crates/schedule-ingest/src/main.rs:187-189` and `:307-310` already set
   `metrics::gauge!(metric_name("schedule_feed_last_ingest_delivered_at_seconds"))`
   to the delivery's own `delivered_at` (the zip's mtime, as an absolute
   Unix-epoch value) whenever this process instance actually posts a new or
   retried delivery. This is genuinely useful and Prometheus-native
   already — no work needed to add it.
2. But this crate's own module doc (`main.rs:17-27`, "The
   last-ingested-mtime gap") states plainly that `schedule-ingest` keeps
   *no persistent state of its own* and does **not** seed this in-memory
   value from `GET /private/schedule-feed-ingests` at startup. A pod
   restart (deploy, crash, `kubectl delete pod`) wipes the gauge from that
   process's registry; it will not reappear in `/metrics` until the next
   *new* delivery lands, however long that takes. For a feed with a
   multi-day cadence, a restart can leave this specific gauge silently
   absent for a long, indistinguishable-from-"actually stalled" stretch —
   the opposite of what a historical-tracking dashboard needs.
3. `crates/api/src/data/queries.rs:596-615` (`last_schedule_feed_fetch`,
   `MAX(delivered_at) FROM schedule_feed_ingests`) has none of that
   problem — it's a live DB read, immune to `schedule-ingest` restarting,
   already backing `/public/freshness`'s `schedule_feed` field and `GET
   /private/schedule-feed-ingests`. This is the more robust of the two
   signals for a dashboard meant to survive the exact kind of event
   (a redeploy) that resets the process-local gauge. See Decision 3.

### Needs the same api-side export mechanism as the above (Decision 1), nothing else

**TfL line status** (`poller-tfl` → `api`'s `/private/tfl-line-status`).
`crates/api/src/data/queries.rs:387-424` (`upsert_tfl_line_status`) sets
`computed_at = NOW()` unconditionally per line, every successful post — the
same "cycle succeeded, not necessarily changed" shape as incidents. TfL
*does* have an equivalent change-gated history write
(`tfl_statuses_changed`, line 426, gating an insert into
`line_status_history`) — but unlike `incident_history`, `line_status_history`
(`crates/api/migrations/20260510023522_initial.sql`) carries no `source`
column, only `line_id`/`statuses`/`computed_at`; isolating TfL's rows
needs a join back to `line_status.source = 'tfl'`
(`crates/api/migrations/20260822120000_line_status_source.sql:29`) to get
the relevant `line_id` set first. This is a real, buildable query, but
`freshness.rs`'s own doc comment already states this crate's team decided,
for this specific source, that "ingest and computation are the same
event" is an acceptable simplification (TfL's upstream API already reports
pre-computed status — there's no separate "raw feed" to diff against the
way there is for Knowledgebase incident text). Given that existing,
deliberate call, this document does **not** add a TfL-specific
change-gated gauge in v1 — it's flagged as Open Question 1, optional
future work, not a Decision here. TfL still gets the plain
`last_tfl_line_status_fetch`-based age gauge from Decision 1.

**Full-coverage line stats / station-samples / station-full-coverage
samples** (`poller-ldbws`, aggregator's full-coverage write path).
`queries.rs` already has `last_station_samples_fetch` (:576-584),
`last_station_full_coverage_samples_fetch` (:586-594), and
`last_full_coverage_line_stats_fetch` (:900-908) — each a plain
`MAX(timestamp column)` query, same shape as the five `freshness.rs`
sources, already backing their own `/private/*` GET routes
(`crates/api/src/routes/ingest.rs`) for poller-startup skip-if-fresh
checks. `freshness.rs`'s own doc comment explicitly excludes
station-samples from the public five: *"per-station polling data, not one
of the five sources this endpoint reports on."* That exclusion reasoning
holds for a **single aggregate** age gauge too, for a different reason:
`poller-ldbws` samples ~280 stations sequentially inside one cycle
(`crates/poller-ldbws/src/main.rs:14-16`, "makes one LDBWS call *per
station* each cycle") and posts one batch at the end. A single
`MAX(polled_at)` across all ~280 stations tells you only "did the whole
cycle run at all" — a strict subset of what `poller_cycle_total{poller="ldbws"}`'s
rate already shows, since a single busy, successfully-sampled station is
enough to make the aggregate look fresh even if several other stations
silently failed within the same cycle (the poller logs and continues past
per-station failures rather than failing the whole cycle — same shape as
every other poller's "log and keep the loop alive" resilience). A
per-station gauge would answer the real question but means one time series
per CRS (~280 series) for a signal this document has no requirement to
build today — see Non-goals and Open Question 2. **No change proposed
here.**

### Ambiguous "is a cycle a real event" cases already resolved correctly, needing no fix

**Aggregator's own output** (`crates/aggregator/src/main.rs`). Aggregator
recomputes and rewrites `line_status`/`computed_at` every cycle by design —
this is continuous internal computation over already-ingested inputs
(incidents, station samples, etc.), not itself an "ingestion of an external
source." Its `aggregator_lines_total`/`aggregator_incidents_loaded` gauges
(`main.rs:338-340`) and per-cycle counters (`:342-370`) already describe
its own cadence fully; nothing here proposes touching it. Out of scope by
the same reasoning `freshness.rs` uses to omit computed/derived tables.

**Enricher** (`crates/enricher/src/main.rs`). `enricher_stream_lag`
(`:456`) already measures how far behind the LLM-extraction consumer is
relative to the `incident-text-changed` stream it reads — i.e., it already
answers "is enrichment keeping up with incidents that changed," which
implicitly depends on Decision 2's new incidents-change signal for its
own upstream cadence. No new metric needed on the enricher side.

## Decisions

### 1. Export the five existing `freshness.rs` timestamps as Prometheus age gauges, computed in `api` at scrape time — no new producer instrumentation

Add `distant_signal_api_data_freshness_seconds{source="stations|tocs|
incidents|tfl|schedule_feed"}` — a gauge holding **age in seconds**
(`now - last_fetch`), not the raw epoch — set inside `api`'s existing
`/metrics` handler (`crates/api/src/main.rs:83-86`,
`axum::routing::get(move || async move { metrics_handle.render() })`).
Making that closure do the same `tokio::try_join!` over
`last_stations_fetch`/`last_tocs_fetch`/`last_incidents_fetch`/
`last_tfl_line_status_fetch`/`last_schedule_feed_fetch` that
`freshness.rs`'s `get_freshness` already does (`freshness.rs:44-53`),
then setting each gauge, then rendering, is a small, self-contained change:
no new background task, no new poll loop, no new state to manage between
scrapes. The cost is one extra small `MAX()` query per source per scrape
(default scrape interval per the merged dashboard's `refresh: "30s"` /
whatever `metrics.podMonitor.interval` is set to) against small,
already-indexed tables — negligible relative to `api`'s existing request
load.

This works technically because `api` composes the same `metrics` facade
crate axum-prometheus already installs a global recorder for:
`PrometheusMetricLayerBuilder::new().with_default_metrics().build_pair()`
(`crates/api/src/main.rs:53-56`) calls through to `metrics-exporter-
prometheus`'s `PrometheusBuilder`/`install_recorder` under the hood
(confirmed reading `axum-prometheus` 0.10.1's `describe_metrics` in
`builder.rs`, which itself calls `metrics::describe_counter!`/
`describe_gauge!` — the same facade macro family this change would use).
Any `metrics::gauge!(...)` call anywhere in `api`'s own code after that
layer is built renders through the same `/metrics` endpoint automatically.
**Prerequisite**: `api`'s `Cargo.toml` does not list `metrics` as a direct
dependency today (confirmed: `grep '^metrics' crates/api/Cargo.toml` finds
nothing) — it only reaches the facade transitively through
`axum-prometheus`. Adding `metrics = "<version matching axum-prometheus
0.10.1's own metrics dependency>"` as a direct dependency is a small, real
prerequisite, not a design risk, but is called out explicitly so it isn't
missed at implementation time.

Gate the gauge computation the same way the `/metrics` route itself is
already gated: `if app.config.metrics_enabled` (`main.rs:68`) — when
metrics are disabled, do not run the extra queries at all, consistent with
`metrics_enabled`'s existing stated purpose ("only decides whether
`/metrics` is registered and whether requests are counted at all").

This requires **zero `charts/distant-signal` changes** — `api` is already
in the `PodMonitor` selector, already scraped on its `http` port at
`/metrics` (`charts/distant-signal/templates/podmonitor.yaml:38-49`,
confirmed live).

### 2. Add a genuinely new query — `last_incident_change`, backed by `incident_history.recorded_at` — as a second, distinct gauge for incidents

`distant_signal_api_data_last_change_seconds{source="incidents"}`
(age since `MAX(recorded_at) FROM incident_history`), alongside (not
instead of) `distant_signal_api_data_freshness_seconds{source="incidents"}`
from Decision 1. Both are real, different, and both worth keeping:

- `..._freshness_seconds{source="incidents"}` answers "is the
  poller-incidents → api pipeline itself still executing end-to-end" (the
  same question `poller_cycle_total`'s rate already answers — this gauge
  is redundant with it in principle, but cheap, DB-backed, and consistent
  in shape with the other four sources on the same panel; keeping it
  avoids a source-specific carve-out in the dashboard).
- `..._last_change_seconds{source="incidents"}` answers the actually novel
  question this whole feature exists for: is the upstream Knowledgebase
  feed still producing distinguishable content, independent of whether the
  poller is technically still running successfully. This is the one metric
  in this entire audit that would have directly shown a 3-hour quiet
  window the way the repo owner's own motivating example describes,
  because it cannot be satisfied by "the poller ran and re-wrote identical
  data" the way the freshness gauge can.

New query, same shape as every existing `last_*_fetch` function in
`queries.rs` (single `MAX()` scalar):

```rust
pub async fn last_incident_change(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (recorded_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(recorded_at) FROM incident_history")
            .fetch_one(pool)
            .await?;
    Ok(recorded_at)
}
```

No migration, no producer change, no new poller instrumentation — the
`incident_changed`/history-write logic this reads already exists and is
already exercised on every real incidents POST
(`queries.rs:106-168`). This is exactly the "genuinely new instrumentation"
case the task asked to identify, and it turns out to live entirely inside
`api`, not inside `poller-incidents`.

Do **not** expose this new query through `/public/freshness` — that
endpoint's five fields are a deliberate, named, stable public contract
(`freshness.rs`'s own struct and the frontend's `DataFreshnessInfo.tsx`
consumer); this is an operator-only Prometheus signal with a different
shape (a change-timestamp, not a fetch-timestamp) and belongs only in
`api`'s `/metrics` output, gated the same way as Decision 1.

### 3. For the schedule feed, standardize the dashboard on the DB-backed `api`-side gauge, not `schedule-ingest`'s own in-process gauge

Both signals from the audit above continue to exist and are each useful for
their own purpose — `schedule-ingest`'s own
`distant_signal_schedule_feed_last_ingest_delivered_at_seconds` is a
correct, real-time, low-latency signal *for that process's own belief
about the world*, useful when debugging `schedule-ingest` itself (e.g. "did
this pod's own extraction/POST cycle actually run"). But it should **not**
be the metric the dashboard's headline "is the schedule feed still landing"
panel keys off, because it silently disappears across a `schedule-ingest`
restart until the next real delivery — a false "gap" indistinguishable
from a real one, for a source whose whole point is to be checked during
exactly the kind of multi-day quiet periods that could span a redeploy.

`distant_signal_api_data_freshness_seconds{source="schedule_feed"}` from
Decision 1 (backed by `last_schedule_feed_fetch`,
`MAX(delivered_at) FROM schedule_feed_ingests`) has no such gap — it's a
live read against a Postgres table each scrape, unaffected by
`schedule-ingest`'s own process lifecycle. Recommend the dashboard panel
use the `api`-side gauge as primary, and optionally overlay
`schedule-ingest`'s own gauge as a secondary series for cross-checking
during active debugging of that specific crate — not as the source of
truth for historical tracking.

### 4. Do not touch any poller's own `poller_cycle_total`/`poller_cycle_duration_seconds`

Confirmed by reading `poll_once` in both `poller-stations` and
`poller-incidents` (near-identical structure across all five pollers):
every scheduled cycle fetches, parses, and POSTs the full current feed
state regardless of whether anything changed; `result="success"` is set
whenever that pipeline completes without error, not whenever new data was
found. This is a correct, working "is this poller's own scheduled work
loop still executing" signal (closer in spirit to `up{}` than to true
ingestion cadence) and already has dashboard value on its own — but per
the audit above, the real "how regularly is new data arriving" question
for the one source where the distinction actually matters (incidents) is
better and more cheaply answered by Decision 2's DB-backed change signal
than by teaching five separate poller binaries to diff their own payloads
against what they sent last cycle (which would also need each poller to
remember its last-sent payload across restarts, or duplicate the
change-detection logic `api` already owns). **No poller-side code changes
in this design.**

## Grafana dashboard changes

All panel additions target the same, already-merged ConfigMap this repo's
prior Grafana plan created — which itself lives in `Ranma-Config`
(`clusters/mine-bringer/apps/distant-signal.yaml`, per that plan's "The
dashboard `ConfigMap` — exact structure" section), not in this repo. As
with that plan, what follows is a precise specification for a human (or an
orchestrator with real cluster access) to apply to that file — not a change
this document makes here.

### Panel type: timeseries over the age gauges, not a stat/gauge snapshot

For every `distant_signal_api_data_freshness_seconds{source}` and
`distant_signal_api_data_last_change_seconds{source="incidents"}` series,
use a **timeseries panel plotting the raw gauge value (age in seconds)
directly**, not `time() - gauge` and not a single-value stat/gauge panel.
Reasoning:

- These are already age-in-seconds gauges by construction (Decision 1/2
  compute `now - last_fetch` at scrape time), so no `time()` arithmetic is
  needed in the PromQL at all — simpler queries, and correct even across a
  Prometheus restart (a `time() - <absolute-epoch-gauge>` expression is
  more fragile to reason about here, and this repo already has one gauge
  of that absolute-epoch shape — `schedule-ingest`'s own — which Decision 3
  explicitly avoids keying the primary panel on for exactly this kind of
  reasoning overhead).
- A timeseries makes the requested pattern directly visible: the line
  **climbs steadily between updates and drops to ~0 on every new arrival**
  — a sawtooth. A long, unbroken climb without a drop is precisely "this
  feed went quiet for N hours," visible at a glance and inspectable at any
  point in the dashboard's time range (`time.from`/`time.to`), which is
  the whole "historical tracking" ask a single current-value stat panel
  cannot satisfy.
- A stat/gauge panel (like dashboard ids 6/7's `instant: true` current-
  service-status panels) only shows the value *right now* — functionally
  no different from what `/public/freshness` already provides, and
  therefore adds nothing beyond what the frontend already surfaces. The
  value of putting this in Grafana specifically is the history, which only
  a timeseries panel exposes.

### Concrete panel additions

**Panel A — "Data source freshness (age since last successful ingest)"**,
timeseries, one series per `source` label:

```json
{
  "id": 8,
  "title": "Data source freshness (age since last ingest)",
  "type": "timeseries",
  "datasource": { "type": "prometheus", "uid": "${datasource}" },
  "gridPos": { "h": 8, "w": 24, "x": 0, "y": 44 },
  "targets": [
    { "expr": "distant_signal_api_data_freshness_seconds", "legendFormat": "{{source}}" }
  ],
  "fieldConfig": { "defaults": { "unit": "s" } }
}
```

**Panel B — "Incidents feed: pipeline freshness vs. actual content change"**,
timeseries, two series, specifically to make the Decision 2 distinction
visible on its own:

```json
{
  "id": 9,
  "title": "Incidents: poller freshness vs. last real content change",
  "type": "timeseries",
  "datasource": { "type": "prometheus", "uid": "${datasource}" },
  "gridPos": { "h": 8, "w": 24, "x": 0, "y": 52 },
  "targets": [
    { "expr": "distant_signal_api_data_freshness_seconds{source=\"incidents\"}", "legendFormat": "poller cycle succeeded (age)" },
    { "expr": "distant_signal_api_data_last_change_seconds{source=\"incidents\"}", "legendFormat": "feed content actually changed (age)" }
  ],
  "fieldConfig": { "defaults": { "unit": "s" } }
}
```

If the two lines track closely, the feed is both alive and actively
changing. If the "poller cycle succeeded" line stays low/flat while "feed
content actually changed" climbs for hours, that is exactly the "feed went
quiet but the poller doesn't know it" scenario the repo owner asked to be
able to see.

**Panel C — trust/movement/full-coverage cadence (already-covered sources,
explicitly not new metrics)**: no new metric needed (per the audit above,
Panels 1-4 of the existing dashboard already cover this via `rate()`);
optionally add threshold coloring or an `absent()`-based alert rule against
the existing counters if the operator wants a visual/alerting distinction
between "rate is merely low" and "rate has been exactly zero for N
minutes" — a refinement of presentation, not a new signal. Not designed
further here; see Open Question 3.

No panel is proposed for schedule-feed's own in-process gauge (Decision 3)
beyond an optional secondary series on Panel A, and no panel is proposed
for TfL's change-gated signal (deferred, Open Question 1) or per-station
LDBWS freshness (deferred, Open Question 2).

## Non-goals

- **No new instrumentation in any of the five poller crates.** Every gap
  this document found that needed closing lives inside `api` (a new query
  plus exporting existing queries as gauges), not inside
  `poller-stations`/`poller-tocs`/`poller-incidents`/`poller-ldbws`/
  `poller-tfl`.
- **No change to `/public/freshness` or its frontend consumer.** That
  contract is unrelated to this document's operator-facing, historical-
  tracking goal; see "Relationship to prior work" above.
- **No per-station (LDBWS) freshness gauge.** Flagged as a real gap
  (Audit, "Full-coverage line stats / station-samples..." section) but out
  of scope for this pass — see Open Question 2.
- **No TfL change-gated (as opposed to fetch-gated) freshness gauge.**
  `freshness.rs`'s own existing "ingest and computation are the same
  event" call for TfL is treated as still valid; not revisited here. See
  Open Question 1.
- **No alerting rules (`PrometheusRule`/Alertmanager).** This document
  designs dashboard visibility only, matching the scope the task was given
  ("add metrics to Grafana showing..."); alerting on these new gauges
  (e.g. "page if `distant_signal_api_data_last_change_seconds{source=
  "incidents"} > 3h`) is a natural, cheap follow-on once the gauges exist,
  but is new scope this document doesn't design.
- **No change to `metrics-exporter-prometheus`'s histogram bucket
  configuration** (`common::metrics::DEFAULT_BUCKETS`) — every metric this
  document proposes is a gauge, not a histogram.

## Open questions / risks

1. **Should TfL get its own `last_change`-style gauge eventually?** The
   query is buildable (join `line_status.source = 'tfl'` to get the
   relevant `line_id`s, then `MAX(recorded_at) FROM line_status_history`
   filtered to that set) but adds a join `incident_history`'s equivalent
   doesn't need, and the team's existing documented position
   (`freshness.rs`'s own comment) is that TfL's ingest-equals-compute
   simplification is intentional. Left as a future refinement, not a
   Decision, pending the repo owner confirming whether TfL's upstream feed
   is actually expected to go quiet in a way worth distinguishing from "no
   real change" — unlike incidents, TfL line status updating rarely may
   just mean "the Underground is running fine," not "the feed died."
2. **Per-station (LDBWS) staleness is a real, unaddressed gap** — a single
   poller cycle can silently fail for individual stations within an
   otherwise-"successful" cycle, and neither `poller_cycle_total` nor a
   naive `MAX(polled_at)` aggregate would show it. This may already be
   partially in scope for the separate, existing per-station-stats design
   line (`docs/superpowers/specs/2026-09-03-per-station-stats-design.md`,
   `docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`)
   — worth checking whether a per-CRS freshness gauge belongs there instead
   of here before building one, given the cardinality cost (~280 series)
   either design would need to justify.
3. **Should the existing throughput counters (trust-consumer et al.) get
   an explicit "went to zero" visual treatment**, beyond what `rate()`
   already shows on a timeseries axis? A human still has to notice a line
   near zero on Panels 1-4; an `absent_over_time()`-driven stat panel or
   threshold coloring would make a true gap harder to miss, at the cost of
   picking a "how long is too long" threshold per source with no
   established baseline yet. Flagged, not designed — likely belongs
   together with Non-goals' alerting-rules item as one follow-on, not
   split into two.
4. **Exact `metrics` crate version to pin as a new direct dependency of
   `api`** (Decision 1's prerequisite) needs to match whatever
   `axum-prometheus = "0.10"` already resolves to transitively, to
   guarantee both share the exact same global recorder instance rather
   than each trying to install a competing one — a `cargo tree -p api -i
   metrics` check at implementation time, not a research gap this document
   leaves unresolved by choice, just unverified against a live `Cargo.lock`
   in this pass.
