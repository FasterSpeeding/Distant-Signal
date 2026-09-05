# Design: A Status/Observability Page — Dependency Graph, Historical Uptime, Trust-Event Throughput

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass**, matching the rigor and format of
`docs/superpowers/specs/2026-08-29-metrics-design.md` and
`docs/superpowers/specs/2026-09-04-movement-relay-design.md`. Every table,
schema and route sketch below is a sketch, not final code.

## What was asked for, verbatim

> a status page which shows metrics such as when data was last received,
> graphs for how many trust events are being processed and ingressed (both
> globally and by individual services), and to give the statuses of the
> microservices

Follow-up clarifications: microservice status should render as a
**dependency graph** (not a grid/traffic-light list), with live status
overlaid per node; the page should also carry **historical uptime** per
service, not just current status.

## The central problem, stated precisely

This cluster has **no Prometheus, Grafana, or any monitoring/collection
stack**. Confirmed independently for this document (not taken on faith from
an earlier session's memory — see Correction 1 below for where that earlier
account had drifted): a full grep of the `Ranma-Config` GitOps repo (cloned
locally at `/tmp/ranma-config`) for `prometheus|monitoring|grafana|scrape`
across every `*.yaml` file turns up nothing but (a) unrelated Tailscale
hostnames that happen to contain the substring `fox-prometheus`, (b) three
`prometheus.io/scrape`/`prometheus.io/port` annotation pairs baked into
Flux's own vendored `gotk-components.yaml` (Flux's controllers self-annotate
for a Prometheus that would have to already exist to read them), and (c) one
`- grafana` line, also inside vendored Flux manifests, unrelated to this
app. `clusters/{base,production}/infrastructure/` — the place cluster-wide
infra like this would live — contains exactly `core-dns.yaml`,
`cert-approver.yaml`, and `metrics-server.yaml` (the Kubernetes Metrics API
server, which feeds `kubectl top`/HPAs — a completely different, in-memory,
no-history system, not Prometheus). There is no `HelmRelease` for
`kube-prometheus-stack`, `prometheus`, or `grafana` anywhere in either
cluster overlay. **Conclusion independently reconfirmed: nothing scrapes or
stores this app's metrics over time.**

Separately, and importantly: **this is not a fresh gap this document is
discovering — it is a gap a prior design in this same repo already
identified, reasoned about, and deliberately chose not to close.**
`docs/superpowers/specs/2026-08-29-metrics-design.md` ("Metrics Tracking in
the Background") already shipped real Prometheus-format `/metrics`
instrumentation into every non-frontend binary. Its own Non-goals section
says, verbatim: *"Bundling a Prometheus/Grafana/Alertmanager stack in the
chart... this design assumes the operator brings their own metrics
backend."* Its Open Question 2 frames this explicitly as a deliberate
bundle-vs-assume choice, mirrored on the same axis the Helm chart design
already drew for Postgres/Redis (bundled, because the app cannot run
without them) versus monitoring (assumed, because it can). **Nobody has
since brought that assumed backend.** This document's job is to resolve
that — for the one purpose actually being asked for (a status page inside
this app), not to retroactively fulfil the general assumption every
possible future monitoring need might have relied on.

## Corrections to the brief's assumptions

Following this repo's own convention of recording where direct inspection
overturned an inherited framing (e.g. `2026-09-04-movement-relay-design.md`'s
"Ground truth this document corrects"):

1. **The specific quoted comment ("this cluster has no Prometheus/monitoring
   stack... to scrape it") does not exist verbatim anywhere in
   `Ranma-Config`.** `clusters/base/apps/distant-signal.yaml` and
   `clusters/mine-bringer/apps/distant-signal.yaml` were read in full (159
   and 185 lines respectively) and grepped case-insensitively for
   `prometheus|monitoring|grafana|scrape|no Prometheus`; no such comment is
   present in either. This looks like drift from an earlier session's
   memory of a conversation, not this repo's actual committed content. The
   underlying **fact** the quote asserted — no monitoring stack exists — is
   independently true regardless, confirmed by the whole-repo grep above,
   so nothing in this document's conclusions changes; only the specific
   citation is corrected here for honesty.
2. **"Last data received" is not merely "mostly already solved" — it is
   partially already shipped, including a working frontend consumer.**
   `crates/api/src/routes/freshness.rs` already implements
   `GET /public/freshness`, unauthenticated, aggregating five of the eight
   `last_*_fetch` queries (`stations`, `tocs`, `incidents`, `tfl`,
   `schedule_feed`) into one `DataFreshness` JSON response — deliberately
   omitting `station_samples`, `station_full_coverage_samples` and
   `full_coverage_stats` per its own module doc ("Station-samples is
   deliberately omitted: it's per-station polling data, not one of the five
   sources this endpoint reports on"). This is already wired end-to-end
   into the frontend: `frontend/lib/api.ts:359`'s `getDataFreshness` calls
   it, `frontend/app/layout.tsx:56-72` fetches it once per page load, and
   `frontend/components/DataFreshnessInfo.tsx` renders it as a nav-bar info
   icon tooltip today. This document's "last data received" section
   (Decision 3) is therefore a genuine **extension** of shipped code, not a
   greenfield aggregation job — see Decision 3 for exactly what changes.
3. **Real metric names carry a `distant_signal_` prefix the brief's
   examples omitted.** `crates/common/src/metrics.rs`'s `metric_name()`
   prepends `distant_signal_` to every hand-emitted metric name
   specifically "so it can never collide with... a future metric from an
   unrelated process sharing the same Prometheus instance." The real,
   grep-confirmed names this document designs against are
   `distant_signal_movement_relay_events_published_total{msg_type}`
   (`crates/movement-relay/src/main.rs:96-100`),
   `distant_signal_movement_relay_stream_lag{group}` (`:135-139`),
   `distant_signal_trust_consumer_events_received_total{msg_type}`
   (`crates/trust-consumer/src/process.rs:322-326`),
   `distant_signal_trust_consumer_events_matched_total` (no label,
   `:328-331`), `distant_signal_trust_consumer_stream_gap_detected_total`
   (`crates/trust-consumer/src/main.rs:203-206`),
   `distant_signal_full_coverage_consumer_events_matched_total{line_id}`
   (`crates/full-coverage-consumer/src/main.rs:382-386`), plus that same
   crate's `stream_gap_detected_total`, `cycle_duration_seconds`,
   `station_buckets_dropped_total`, `lines_available_total`/
   `lines_pending_total`/`stations_available_total` gauges (`:222-484`).
   12 crates set `metrics_enabled`/expose a `/metrics` listener, confirmed
   by grepping `crates/*/src/config.rs` — matching the brief's count.
4. **The frontend's actual charting convention is `@mantine/charts`'
   `LineChart`, with raw `recharts` imported only for one low-level
   primitive (`<ReferenceArea>`) `@mantine/charts` doesn't expose.**
   `frontend/app/lines/[id]/history/TrendsCharts.tsx:5` is the only direct
   `recharts` import in the app (`import { ReferenceArea } from 'recharts'`);
   every actual chart is `@mantine/charts`' `LineChart` (`package.json`:
   both `@mantine/charts@9.5.2` and `recharts@^3.2.1` are present —
   `@mantine/charts` is itself built on `recharts` under the hood, which is
   why both appear as direct dependencies even though almost no app code
   touches `recharts` directly). This document's chart pieces (Decision 4)
   follow that same convention — `@mantine/charts` `LineChart` as the
   primary API, occasional `recharts` primitives only where
   `@mantine/charts` has a documented gap — not a new charting approach.
5. **No colored "status-over-time" strip/timeline-bar component exists
   anywhere in this app to reuse for historical uptime.** Searched
   `frontend/app/lines` and `frontend/components` for
   `timeline|StatusBar|StatusStrip|segment` — no such visual idiom exists
   today (the "Timeline tab" referenced elsewhere in this repo's specs is a
   textual list of status changes, not a rendered bar). Decision 5 proposes
   reusing the same generalized `TrendsCharts` leaf (Correction 4 /
   `2026-09-02-trend-chart-granularity-design.md` Decision 9) rather than
   inventing a new widget, since no existing one is being displaced.

## Ground truth traced fresh for this document

**Real service topology**, traced from every Deployment template under
`charts/distant-signal/templates/*.yaml` (env vars referencing other
services/Kafka/Redis/Postgres), not assumed from memory:

- **`movement-relay`**: Kafka (`KAFKA_BROKERS`/`KAFKA_TOPIC`/
  `KAFKA_CONSUMER_GROUP`, `movement-relay-deployment.yaml:93-106`) → Redis
  (`REDIS_URL`, `:111`). Has **no** dependency on `api` or Postgres at all —
  confirmed by reading its full env block. This is the one node with zero
  today-existing connection into this app's own write surface, which
  Decision 2 has to account for explicitly.
- **`trust-consumer`** / **`full-coverage-consumer`**: each gated by its own
  `.Values.{trustConsumer,fullCoverageConsumer}.movementFeed` value
  (`trust-consumer-deployment.yaml:14`, `full-coverage-consumer-deployment.yaml:10`)
  — `"kafka"` renders the direct `KAFKA_*` env block (a transitional/
  fallback mode, per `2026-09-04-movement-relay-design.md`'s own cutover
  sequencing), `"redis"` (the target end-state) renders `REDIS_URL` +
  `REDIS_AUTOCLAIM_MIN_IDLE_SECS`/`REDIS_GAP_CHECK_SECS` instead
  (`trust-consumer-deployment.yaml:149-155`,
  `full-coverage-consumer-deployment.yaml:165-171`). Both also call `api`'s
  `/private/*` routes regardless of feed mode: `trust-consumer` →
  `API_INGEST_URL`/`API_TRACKED_TRAINS_URL`/`STANOX_CRS_URL`
  (`:119-143`); `full-coverage-consumer` →
  `SCHEDULE_LINE_POPULATION_URL`/`FULL_COVERAGE_STATS_URL`/
  `STATION_FULL_COVERAGE_STATS_URL`/`STANOX_CRS_URL` (`:121-129`).
- **5 pollers** (`incidents`/`stations`/`tocs`/`ldbws`/`tfl`, one templated
  block in `poller-deployments.yaml`): external RDM/TfL feed → `api` via
  `API_INGEST_URL`/`API_SAMPLE_STATIONS_URL` (`:113-118`).
- **`schedule-ingest`** (SFTP receiver + ingest pod) and
  **`schedule-reference`** (`schedulefeed-deployment.yaml`): external DTD
  SFTP push → `api` via `API_INGEST_URL`/`SCHEDULE_LINE_POPULATION_URL`/
  `SCHEDULE_NETWORK_DEPARTURES_URL` (`:217-292`).
- **`aggregator`**, **`notifier`**, **`enricher`**: each renders
  `{{- include "distant-signal.databaseEnv" . -}}` directly
  (`aggregator-deployment.yaml:56`, and confirmed present in
  `notifier-deployment.yaml`/`enricher-deployment.yaml` by the same grep) —
  **direct Postgres clients, not `api` HTTP callers**, unlike every service
  in the two bullets above. `enricher` additionally holds `REDIS_URL`
  (`enricher-deployment.yaml:83`, the `incident-text-changed` stream) and
  an external LLM endpoint (`LLM_BASE_URL`, `:85`).
- **`api`**: the only direct `DATABASE_URL` consumer outside the three
  above that isn't reached through the shared helper (`api-deployment.yaml`
  wires it by hand, with a `PGPASSWORD`-ordering comment at line 102), plus
  `REDIS_URL` (`:135`) and the SSO/internal-OAuth issuer (Authentik,
  external to this chart, `:105,156,165,167`).
- **`frontend`**: `API_BASE_URL` (`:44`) + `RAILMCP_BASE_URL` (`:54`).
- **`railmcp`**: `DS_API_BASE_URL` (`:72`), `OAUTH_REDIS_URL` (`:99`), and
  direct `LDBWS_*_URL`s to RDM (`:108-128`) — a second, independent client
  of both `api` and Redis.
- **Postgres and Redis are the two genuinely shared infrastructure nodes.**
  Postgres: `api`, `aggregator`, `notifier`, `enricher` (direct); every
  other service reaches it only transitively through `api`. Redis:
  `api` (incident-text-changed producer), `enricher` (consumer),
  `movement-relay` (producer), `trust-consumer`/`full-coverage-consumer`
  (consumers, feed-mode-dependent), `railmcp` (its own OAuth token cache).

**Health-signal inventory**, confirmed by `find crates -iname health.rs`:
exactly `movement-relay`, `trust-consumer`, `full-coverage-consumer` (each
its own `/healthz`, bound to `HEALTH_BIND_URL` — an address `kubelet`
probes directly against the pod, per the Deployment templates' liveness/
readiness blocks) plus `api`'s own `/public/health` (its own liveness, not
a report on anything else). `trust-consumer`'s and
`full-coverage-consumer`'s `ConnectionState` flips true "once the consumer
has successfully polled at least one batch (or confirmed group
membership)"; `movement-relay`'s is a stricter Kafka-rebalance-callback
signal (`ready` on confirmed partition assignment, independent of message
arrival — `crates/movement-relay/src/health.rs`, deliberately not matching
the other two, per that file's own doc comment). **None of these three
ports has a Kubernetes `Service` in front of it** — the chart's only
`Service` objects are `api`, `frontend`, `postgres`, `redis`, `railmcp`,
`schedulefeed`, and the dev-Authentik pair (`ls
charts/distant-signal/templates/*service*.yaml`) — so nothing in-cluster
other than `kubelet` can reach any of these three `/healthz` endpoints
today. Every other backend service (`aggregator`, `notifier`, `enricher`,
the 5 pollers, `schedule-ingest`, `schedule-reference`) has zero explicit
health signal beyond "the pod is Running."

**Public/private posture**: `crates/api/src/routes/mod.rs`'s
`public_router()`/`private_router()` split is the load-bearing precedent.
`private_router()` (`ingest.rs`, `samples.rs`) is wrapped in
`middleware::from_fn_with_state(app, require_internal_oauth)`
(`mod.rs:70`) — every route in it needs a verified client-credentials
bearer token, checked against a `(prefix, method) → required groups` table
built in `crate::app::build_internal_oauth_routes`
(`crates/api/src/app.rs:53-200`+), per-service group names sourced from
`ServiceArguments` (e.g. `internal_oauth_group_incidents`,
`internal_oauth_group_trust_consumer`). `public_router()` is
unauthenticated by design — health, freshness, line status, reference data,
notifications, chat, station stats, departures. The eight `last_*_fetched`
handlers the brief names all live in `ingest.rs` and are therefore
**private today**, reachable only with a service's own token — confirmed
by reading `ingest.rs` in full (router at lines 32-84, each `get_*_last_fetched`
handler at 91-134 and 166-395). `/public/freshness` is a **separate**,
already-public read of the same underlying `last_*_fetch` queries, built
specifically so the frontend doesn't need a private credential to show
this — exactly the shape Decision 6 below extends, not reinvents.

## Decision 1: metrics/history storage — the central call

Three options, weighed honestly, one chosen.

### Option A: deploy a real (lightweight) Prometheus, scraping the 12 existing `/metrics` endpoints

**What it would take.** The instrumentation already exists end-to-end
(Correction 3). Prometheus's own `kubernetes_sd_configs: pod` role
discovers scrape targets by pod IP + annotated port directly — it does
**not** need a `Service` object per workload, so the missing `Service`
objects noted above (Ground truth) are not actually a blocker; the
`prometheus.io/scrape`/`prometheus.io/port` annotations
`2026-08-29-metrics-design.md` already wired onto every workload
(`aggregator-deployment.yaml:20-24` and equivalents) are exactly the
CRD-free fallback this style of discovery reads. So the scrape-config half
is nearly free. What is **not** free: a real Prometheus server — its own
container, its own TSDB storage (a `PersistentVolumeClaim` if history must
survive a restart, which "historical uptime" explicitly requires), and its
own resource footprint (baseline memory/CPU for even a small single-node
Prometheus is not nothing, typically well above what any single binary in
this chart runs today).

**Where it would live is itself unresolved, and the lack of a clean answer
is evidence against this option, not just a detail to sort out later.**
`docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s own Goals
("no subchart dependencies... installable in an air-gapped cluster given
the images") and `2026-08-29-metrics-design.md`'s own Non-goal
("Bundling a Prometheus/Grafana/Alertmanager stack in the chart") both
already, explicitly, rule this out of `charts/distant-signal` itself.
Reversing that would need to be a deliberate act, not a side effect of this
document. The alternative — placing it in `Ranma-Config`'s
`clusters/{base,production}/infrastructure/`, alongside `core-dns.yaml`/
`cert-approver.yaml`/`metrics-server.yaml` — doesn't fit either: every
existing file in that directory is genuinely cluster-wide, app-agnostic
infrastructure; a Prometheus instance scraping `distant_signal_*`-prefixed
business metrics for one specific app's own status page is not that shape
of thing, and grafting it into the generic-infra folder would be the first
app-specific file to live there. **There is no repository this cleanly
belongs in for the actual, narrow thing being asked for** — a real,
general-purpose Prometheus deployment is a bigger piece of infrastructure
than "show trust-event throughput and per-service uptime on one page,"
and forcing it into either repo means accepting an architectural
mismatch either way.

**The real, freshly-relevant cost**: this cluster has already had one real
incident this session from an under-provisioned single Redis instance — a
new stateful, single-instance component (Prometheus, with no HA story
proposed or needed at this scale) is the same class of risk again, at the
exact moment there is fresh, direct evidence of what getting that sizing
wrong costs. Nothing about this option is hypothetical risk-aversion; it's
the same mistake shape recurring on a deliberately short timeline.

**Rejected** — not because it wouldn't work technically (it would, cleanly,
and would additionally hand every metric this repo already emits real
PromQL query power for free), but because it reopens a scope and placement
decision this repo's own prior design docs already closed deliberately, at
a cost (new stateful singleton, ambiguous ownership between two repos) that
is disproportionate to what's actually being asked for: one page's worth of
trust-event graphs and per-service uptime, not general-purpose ad-hoc
metrics querying.

### Option B: the app rolls its own storage — a Postgres table, written on an interval

**Chosen.** Concretely: services already computing the counters this page
needs periodically report a small, typed snapshot of just those counters
to a new private `api` endpoint; `api` writes each snapshot as a row in a
new table, read back later for graphing. This is a genuinely narrower slice
of what Prometheus does — no PromQL, no arbitrary ad-hoc querying, no
general-purpose alerting substrate — in exchange for **zero new
infrastructure dependency** and a mechanism that is, block for block, the
same "poll on an interval, write to Postgres" shape this app has used five
times already (the five pollers) plus the aggregator's own accumulate-
upsert precedent (`line_status_daily_stats`/`line_status_hourly_stats`).
Reasoning through the real design questions this raises, rather than
waving at "just add a table":

- **Which services push, and how, given they don't all have the same
  starting shape.** Ground truth above already splits the 15 backend
  workloads into three groups by what they do today:
  1. Services that already call `api`'s `/private/*` routes every cycle
     (5 pollers, `schedule-ingest`, `schedule-reference`, `trust-consumer`,
     `full-coverage-consumer`) — these need no *new outbound call*, only a
     small addition to a request they already make (see Decision 2's
     "derive from the request itself" mechanism, which this option shares).
  2. Services that are direct Postgres clients with no `api` HTTP
     dependency at all (`aggregator`, `notifier`, `enricher`) — these
     write directly, one more query inside their already-open per-cycle
     transaction, no new network edge.
  3. `movement-relay` — the one node with **no** connection to `api` or
     Postgres today (Ground truth above). Giving it counters worth
     graphing (`events_published_total`, `stream_lag`) unavoidably requires
     introducing its first-ever dependency on `api`, whichever mechanism is
     chosen. This is a real, new coupling this document is introducing —
     stated plainly, not hidden inside "just add reporting." It's a small,
     one-way, best-effort HTTP POST on a cycle boundary — structurally the
     same shape every poller already has toward `api` — not a new
     bidirectional dependency or a change to `movement-relay`'s actual
     Kafka/Redis relay logic.
- **New route: `POST /private/service-metrics`** (sketch, name not final),
  private, gated the same way every other `/private/*` route is — a new
  `internal_oauth_group_*` entry per calling service, following
  `build_internal_oauth_routes`'s existing per-(prefix, method) convention
  exactly (`crates/api/src/app.rs`). Body: `{ service: "movement-relay",
  counters: [{ name: "movement_relay_events_published_total", labels:
  {"msg_type": "0003"}, value: 611234.0 }, ...] }` — a small, explicit,
  typed list, **not** a raw Prometheus-text-format blob. Deliberately not
  parsing Prometheus exposition format server-side: it would need a text
  parser dependency in `api` for a format whose only real purpose (letting
  an *external* scraper self-discover arbitrary metric names) doesn't apply
  here — the emitting service already knows exactly which of its own
  named values matter to this page (a small, curated list per service),
  so it computes and serializes exactly that, the same way every existing
  `/private/*` POST body is already a small typed struct
  (`IncidentMessage`, `StationSample`, etc.), not a generic blob.
- **Storage table**: `service_metric_samples(service TEXT, metric_name
  TEXT, label_value TEXT NULL, captured_at TIMESTAMPTZ, value DOUBLE
  PRECISION, PRIMARY KEY (service, metric_name, label_value, captured_at))`.
  A normalized, narrow shape rather than fixed named columns (contrast
  `line_status_daily_stats`'s fixed columns) because the counter set is
  genuinely heterogeneous across services and carries different label
  dimensions (`msg_type` for `movement-relay`/`trust-consumer`, `line_id`
  for `full-coverage-consumer`, none at all for
  `trust_consumer_events_matched_total`) — a fixed-column schema would need
  a new column (and a migration) for every future counter; this shape
  doesn't. `label_value` is deliberately singular, not a generic key-value
  map: every metric this document actually needs to store has **at most
  one** label dimension (confirmed against the full grep in Correction 3 —
  none of the real counters carry two labels at once), so a second column
  is enough without inventing a JSONB label bag this page's real data never
  needs.
- **`full_coverage_consumer_events_matched_total{line_id}`'s cardinality
  must be collapsed before it reaches this table, not stored per-line.**
  `line_id` ranges over this catalogue's ~100+ lines
  (`DESIGN.md` §10) — exactly the unbounded-cardinality trap
  `2026-08-29-metrics-design.md`'s own Non-goals section already flagged
  and avoided in the *Prometheus* metric itself by leaving `line_id`
  unbounded only because Prometheus's own local cardinality is a Prometheus
  operator's problem, not this app's. For this page's own storage, the
  emitting service **sums across `line_id` before reporting** — the status
  page cares about "how many trust events has this service processed in
  total," not a 100-row-per-service-per-interval breakdown (per-line
  breakdown already exists elsewhere, per-station-stats/per-line pages).
- **Raw cumulative values are stored, not pre-computed deltas — deltas are
  computed at read time, mirroring how Prometheus itself works, not
  reinvented.** A Prometheus counter is monotonic *within* a process
  lifetime and resets to zero on every restart (a known, standard property
  — not new to this design). Storing the raw value at each interval and
  computing `max(0, value[i] - value[i-1])` at query time, treating a
  negative diff as a restart-caused reset (skip/zero that one bucket rather
  than reporting a nonsensical negative "events processed" figure), is
  exactly `rate()`/`increase()`'s own documented reset-handling behavior —
  cited as established prior art this document borrows, not invented from
  scratch. This also means no aggregator-style dedup ledger is needed here:
  unlike `line_status_daily_stats`'s "don't double-count a train seen every
  poll cycle" problem, a Prometheus counter is already cumulative by
  construction — each sample is a total-so-far, and subtracting consecutive
  samples is the entire "how many happened in this interval" computation.
- **Retention: short, mirroring `line_status_hourly_stats`'s posture, not
  `daily_stats_retention_days`'s.** A new `metric_samples_retention_hours`
  knob (aggregator-crate-style CLI/env flag), proposed default **7 days**
  (168h) — long enough for the "recent trend" graphing this page actually
  asks for, an order of magnitude below `daily_stats_retention_days`'s
  300-day LDBWS-licence-driven ceiling (which doesn't apply here — this
  data isn't LDBWS-derived), and short enough that even at the write volume
  below, storage stays trivial. **Volume estimate**: ~7 emitting services ×
  ~5 counters average × one sample per ~60s cycle × 7 days ≈ 7 × 5 × 1440 ×
  7 ≈ 353,000 rows at steady state — comparable in order of magnitude to
  `line_status_daily_stats`'s own "~38k rows/year" figure scaled up by a
  shorter retention window and more emitters, not remotely close to the
  volumes Redis Streams' `movement-events` (~630k/**day**) has to manage —
  a plain indexed Postgres table handles this without special treatment.
  Pruned by a `prune_metric_samples` mirroring `prune_daily_stats`'s exact
  shape (`crates/aggregator/src/queries.rs:402-408`), called from
  wherever the periodic sweep in Decision 2 already runs.

### Option C: hybrid — live gauges for "up now," a narrower heartbeat/uptime mechanism separately

**Folded into the chosen design, not a separate rejected option.** Option B
above already only stores time-series for the small set of *counters* the
throughput graphs need (Decision 4) — it does not try to make "is this
service up right now" a query over that same table. Decision 2 below
designs a **separate**, purpose-built `service_heartbeats`/
`service_status_history` mechanism for liveness/uptime specifically,
because "is it up" and "how many events did it process" are different
questions with different natural storage shapes (a current-value upsert
plus a transition log, versus a numeric time series) — this is exactly the
brief's own Option 3 framing, adopted as part of Option B's overall
Postgres-only answer, not a fourth, competing choice.

**Decision, stated plainly**: Option B, in full — a new Postgres table (or
two: `service_metric_samples` for throughput, `service_heartbeats`/
`service_status_history` for liveness/uptime, Decision 2) written on an
interval by services that already run on an interval, read back for
graphing by `api`. No new stateful component, no new infrastructure
dependency, a real (honestly stated, not hidden) but bounded cost of
reimplementing a narrow slice of what a real Prometheus would give for
free, and — atop everything above — consistency with this repo's own,
already-made "assume, don't bundle" monitoring-stack decision
(`2026-08-29-metrics-design.md`), which this document does not need to
reopen because the actual ask (one status page, not general observability)
never needed the thing that decision assumed away.

## Decision 2: dependency graph — topology, live status, historical uptime

### 2a. The graph's nodes and edges

Fixed, known at design time (traced in Ground truth above, not
runtime-discovered) — 18 nodes:

```
External:      RDM Kafka (TRAIN_MVT_ALL_TOC), RDM REST feeds (Knowledgebase/
               LDBWS/incidents/TOCs), TfL API, DTD SFTP push, Authentik SSO,
               external LLM (Ollama)

App services:  movement-relay, trust-consumer, full-coverage-consumer,
               poller-incidents, poller-stations, poller-tocs, poller-ldbws,
               poller-tfl, schedule-ingest, schedule-reference, aggregator,
               notifier, enricher, api, frontend, railmcp

Infra:         Postgres, Redis
```

Edges are exactly the ones traced in Ground truth: `movement-relay` →
Kafka, Redis; `trust-consumer`/`full-coverage-consumer` → Kafka **or**
Redis (feed-mode-dependent — the graph should render the edge that matches
each service's live `movementFeed` value, not both at once) plus → `api`;
5 pollers/`schedule-ingest`/`schedule-reference` → external feed, → `api`;
`aggregator`/`notifier`/`enricher` → Postgres directly; `api` → Postgres,
Redis, Authentik; `frontend`/`railmcp` → `api`; `railmcp` → Redis, LDBWS
directly. **18 nodes is small enough that a hand-authored, fixed-position
diagram is the right call — not a graph-layout library.** `react-flow`/
`dagre`/`cytoscape`-shaped libraries earn their keep when a graph's shape
is user-editable or changes at runtime and needs automatic re-layout;
this graph's shape is fully known and stable at author time (only the
per-node *status* overlay changes live, never the topology), which is
exactly the situation a few dozen lines of fixed CSS grid/absolute
positioning plus SVG `<line>`/`<path>` edges handles well, with no new
frontend dependency — consistent with this repo's existing restraint
(Correction 4: even the *existing* charts reach for `recharts` directly
only for the one primitive `@mantine/charts` lacks, not wholesale).

### 2b. Deriving each node's live status — the real gap the brief flags, closed concretely

Three different mechanisms for three different node shapes, not one
universal check (because, per Ground truth, nothing today observes every
node the same way):

- **Services that already call `api`'s `/private/*` routes every cycle**
  (5 pollers, `schedule-ingest`, `schedule-reference`, `trust-consumer`,
  `full-coverage-consumer`): derive liveness **from the request they
  already make, for free** — no new call needed. `require_internal_oauth`
  (`crates/api/src/auth.rs`) already verifies every private request's
  `ServiceClaims` (`sub`, `groups`) before it reaches a handler
  (`crates/api/src/auth/internal_oauth.rs:36-48`). Add one small step at
  the end of that middleware: map the matched group to a canonical service
  name (a static table, the same shape `build_internal_oauth_routes`
  already uses per-service) and upsert
  `service_heartbeats(service, last_seen) VALUES ($1, NOW()) ON CONFLICT
  (service) DO UPDATE SET last_seen = NOW()`. This costs one cheap upsert
  per already-happening authenticated request; it adds no new network
  edge and no new call any service has to remember to make.
- **`movement-relay`, `trust-consumer`, `full-coverage-consumer`'s
  `/healthz` booleans**: fold into the same periodic snapshot push
  Decision 1 already needs for counters (`POST /private/service-metrics`).
  Add one more field to that same payload — `healthy: bool`, read straight
  off each service's existing `ConnectionState`/`ReadyState` — so one
  mechanism serves both counters and liveness, rather than two. This also
  answers the "internal-only ports, unreachable in-cluster" problem
  (Ground truth) without adding a `Service` object for any of the three:
  the direction of the call is outbound, from each service to `api`,
  identical in shape to every poller's existing relationship.
- **`aggregator`/`notifier`/`enricher`** (direct Postgres clients, no
  `api` HTTP dependency): upsert `service_heartbeats` **directly via SQL**,
  inside their existing per-cycle transaction — one more query against a
  connection that's already open, not a new call to anywhere. This is the
  concrete answer to the brief's own "derive a proxy status from something
  already observable, e.g. last successful DB write" suggestion for
  services with no `health.rs` — made literal, not just gestured at.
- **`api` and `frontend`**: self-evidently "up" if either is able to serve
  the status page request at all — no heartbeat mechanism needed for
  either, same posture this repo already gives the frontend elsewhere
  ("no server-side identity of its own to protect," per the SSO design's
  own framing, cited in `2026-08-29-metrics-design.md`'s topology table).
- **Postgres, Redis**: no direct ping is proposed as a first-class
  mechanism here — their health is inferred **transitively**: if Postgres
  is down, every other node's heartbeat write fails and every node goes
  stale within one cycle; if Redis is down, the three stream-touching
  services' own existing error handling already surfaces it (and, once
  Decision 1's push reaches them, their own `healthy` field would reflect
  it). A genuine direct check (`api` doing a cheap `SELECT 1`/`PING` at
  render time) is a cheap, honest follow-up worth adding, but this
  document doesn't treat it as load-bearing — it's flagged as an easy
  addition, not a designed-away gap (see Open Questions).

**Status classification**: for every heartbeat-bearing node, compare
`now() - last_seen` against a staleness threshold sized to roughly 2–3x
that service's own known cycle interval — the identical reasoning this
app already applies in `common::ingest::time_until_next_poll`
(`crates/common/src/ingest.rs:88-101`, comparing a fetch timestamp against
a poll interval) and the Timeline tab's `retentionShortfallDays`
(`frontend/lib/history.ts:257`). Three buckets: **healthy** (within
threshold), **stale** (overdue, not yet declared down), **down** (past a
longer multiple, or an explicit `healthy: false` in the push payload).

### 2c. Historical uptime — a transition log, mirroring `line_status_history` directly

`service_heartbeats` (an upsert, current value only) cannot answer "how did
this service's uptime look over the last week" by itself. **Chosen: a
second table, `service_status_history(service TEXT, status TEXT, changed_at
TIMESTAMPTZ)`, written only on a genuine status transition** — the exact
same "write on change, not every tick" shape `line_status_history` already
uses (`crates/aggregator/src/queries.rs:434-451`'s `write_line_status`:
diff against the last-persisted state, write only if it actually changed).
Uptime percentage over any window is then reconstructed from consecutive
transition rows the same way a line's own "how long was it Good today"
would be computed from its transition history — cheap to store (a handful
of rows per service per incident, not one row per heartbeat), and gives
exact interval boundaries rather than a resolution limited by however
often a bucketed sampler happened to check.

**The one real mechanical wrinkle, addressed explicitly, not glossed
over**: a service that crashes outright never gets to write its own "now
down" transition — by definition, nothing is left running to report it.
This has to be detected from the *outside*. Concretely: `api` runs a small
periodic sweep (piggybacked on whatever interval already prunes
`service_metric_samples`, Decision 1) that scans `service_heartbeats` for
any service whose `last_seen` has crossed the "down" threshold **without**
a matching "down" row already at the head of its `service_status_history` —
and writes one. The reverse direction (`down` → `up`) needs no separate
sweep: it's naturally caught the next time that service's own heartbeat
path (whichever of the three 2b mechanisms applies to it) fires again,
which can check "was my last recorded transition a `down`? if so, write an
`up` transition now" inline, symmetric with `write_line_status`'s own
"compare against last persisted state" shape.

## Decision 3: "last data received" — extend `/public/freshness`, don't replace it

**Chosen: add the three currently-omitted fields
(`station_samples`, `station_full_coverage_samples`, `full_coverage_stats`)
to the existing `DataFreshness` struct and `get_freshness` handler**
(`crates/api/src/routes/freshness.rs`), rather than building a second,
separate aggregation endpoint. The status page's "last data received"
section becomes a fuller, dedicated rendering of the same
already-public, already-fetched data
`DataFreshnessInfo.tsx`'s nav-bar tooltip uses today — reusing
`getDataFreshness`/`DataFreshness` (`frontend/lib/api.ts:359`,
`frontend/lib/types.ts:266`) unchanged on the frontend, and reusing the
existing `LastUpdated` component (`frontend/components/LastUpdated.tsx`)
per row instead of the tooltip's compact `freshnessRow` helper. The
"deliberately omitted" comment in `freshness.rs`'s module doc reasoned that
station-samples data is "per-station polling data, not one of the five
[aggregator-feeding] sources" — a fair distinction for a *nav-bar* summary
whose whole point is "is the data driving line status fresh," but not one
that should extend to a dedicated status page whose whole point is
comprehensiveness across every data producer this app has, matching the
brief's explicit ask for all 8. This is a small, additive backend change
(three more fields, three more `tokio::try_join!` arms, all three queries —
`last_station_samples_fetch`, `last_station_full_coverage_samples_fetch`,
`last_full_coverage_stats_fetch` — already exist and are already called
from `ingest.rs`'s own GET handlers) with **zero** change to the nav-bar
tooltip's own rendering (it can keep reading only the five fields it
already destructures, ignoring the three new ones, since `DataFreshness`
gains fields additively).

## Decision 4: trust-event throughput graphs — global vs. per-service, concretely

**"Ingressed, globally"** maps onto `distant_signal_movement_relay_events_published_total`,
summed across every `msg_type` label at each sampled interval —
`movement-relay` is the single real Kafka ingress point once the
`2026-09-04-movement-relay-design.md` cutover (Deploy B) is complete, so
its publish count is the one true "how much of the real feed came in"
figure. **Caveat, stated plainly**: while any service still runs with
`movementFeed: "kafka"` (the transitional/fallback mode traced in Ground
truth), that service is reading Kafka directly and `movement-relay`'s
counter does not reflect its traffic — this graph's "global ingress"
framing is only fully accurate once every consumer is off the `"kafka"`
feed mode. This document doesn't resolve that migration (out of scope,
already designed elsewhere); it's flagged here as a real precondition for
this graph reading correctly, not silently assumed away.

**"Processed, by individual services"** maps onto each downstream
consumer's own counters: `distant_signal_trust_consumer_events_received_total`
(summed across `msg_type`) and `distant_signal_trust_consumer_events_matched_total`
(unlabeled) for `trust-consumer`; `distant_signal_full_coverage_consumer_events_matched_total`
(summed across `line_id` — Decision 1's cardinality-collapse) for
`full-coverage-consumer`. **The single most operationally useful framing is
overlaying all three on one chart** (`movement-relay` published vs.
`trust-consumer` received vs. `full-coverage-consumer` matched, as three
series on one `@mantine/charts` `LineChart`, same multi-series/legend/gap-band
shape `TrendsCharts.tsx` already established) — the most recent commit in
this repo's own history (`4ca522e`, "trust-consumer: add per-msg_type
received/matched counters, comparable directly against movement-relay's
own publish counter") already frames these two counters as intentionally
comparable; this graph is that comparison made visible, rather than only
inferable from a log line or an alert threshold. A growing, sustained gap
between the published and received series is a visual, human-readable form
of exactly the gap-detection problem `2026-09-04-movement-relay-design.md`
Decision 2 already designed a metric+log-based alert for — this graph
doesn't replace that alert, it gives a human looking at the status page the
same signal without needing to already know to look for it.

**Read path** (sketch): `GET /status/trust-events?from=...&to=...`
(placement/auth per Decision 6), computing per-bucket deltas from
`service_metric_samples` at query time per Decision 1's reset-aware
`max(0, value[i] - value[i-1])` rule, bucketed to whatever interval the
frontend range picker requests (mirroring `HistoryRangePicker`'s existing
day/hour-range precedent, `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`).

## Decision 5: frontend — a new `/status` route

New top-level route `frontend/app/status/page.tsx`, sibling to the existing
flat top-level routes (`/track`, `/train`, `/incidents`), not nested under
any existing section. Sketch of sections, each a Server Component fetching
its own data (matching this app's existing per-section-fetch convention,
e.g. `TrendsResults`/`HourlyTrendsResults`):

1. **Dependency graph** (Decision 2a/2b) — the fixed hand-laid-out diagram,
   live status color per node (healthy/stale/down), a hover/click target
   per node surfacing its own current uptime percentage over a selectable
   window plus last-seen timestamp.
2. **Last data received** (Decision 3) — a full table (not a tooltip) over
   all 8 sources, each row using the existing `LastUpdated` component.
3. **Trust-event throughput** (Decision 4) — one multi-series
   `@mantine/charts` `LineChart`, generalizing the same `TrendsCharts`/
   `ChartPoint`/`gapSpans` leaf `2026-09-02-trend-chart-granularity-design.md`
   Decision 9 already generalized for bucket-key-agnostic reuse, rather
   than forking a third copy of that rendering logic.
4. **Per-service historical uptime** (Decision 2c) — a numeric
   percentage per service per window as the primary presentation (exact,
   cheap to compute from `service_status_history`'s transition rows), with
   an optional simple step-line visualization (0/1 "up" value) using the
   same generalized chart leaf as item 3, rather than a bespoke
   colored-segment widget (Correction 5 — no such widget exists today to
   extend, and inventing one is more surface than this page needs).

Exact visual density/layout (how much of this fits above the fold, whether
the graph and the throughput chart share a row) is left to an
implementation-time screenshot pass, consistent with this repo's own
established posture of not pinning exact UI dimensions in a design doc
(`2026-09-02-line-history-chart-fixes-design.md`'s precedent, cited
directly in the trend-chart-granularity spec).

## Decision 6: public vs. private access — public, unauthenticated

**Chosen: `/status` and its backing reads are public**, following this
app's own established comparable-sensitivity precedent rather than
inventing a new posture. `/public/health`, `/public/freshness`, `/Line/...`
status endpoints, and `/public/stations/{crs}/departures` (the most recent
addition to this exact family, per this session's own git log) are all
unauthenticated today, and none of the data this page adds is more
sensitive than what's already public: aggregate counter values
(`distant_signal_*_total`, summed, never per-user or per-line-labelled at
the storage layer per Decision 1's cardinality collapse), service up/down
status, and last-fetched timestamps. None of it is PII, none of it is a
credential, and none of it reveals more about this app's internals than
the already-public `/Line/{id}/Stats/...` history endpoints or the
already-shipped `distant_signal_*` metric *names* themselves (which, per
`2026-08-29-metrics-design.md`'s own Open Question 3, were deliberately
left ungated specifically because they're read-only and carry no
per-line/per-station/per-user data). A public status page is also a
deliberate, common pattern for real products (the brief's own framing) —
this app choosing to expose one is consistent with, not a departure from,
its existing default-public posture for read-only operational/status data.
**What stays private, unchanged**: the new `POST /private/service-metrics`
write path (Decision 1/2b) — that's a write, gated exactly like every
other `/private/*` write, with its own per-service `internal_oauth_group_*`
entries; only the **read** side of this feature is public.

## Non-goals

- **A general-purpose alerting/paging system.** This design produces
  visibility (a page a human looks at), not `PrometheusRule`-style
  threshold alerting, on-call paging, or notification-on-breach. The
  gap-detection *metric* `2026-09-04-movement-relay-design.md` already
  proposed is unaffected by this document either way.
- **A full Grafana replacement or ad-hoc query capability.** No PromQL-
  equivalent query language, no arbitrary dashboard-building, no querying
  metrics this document didn't explicitly decide to store. If a future
  need for genuinely general-purpose metrics querying emerges, that's the
  moment to revisit Decision 1's Option A, not something this document
  should pre-build capacity for speculatively.
- **Reopening `2026-08-29-metrics-design.md`'s own scope.** The 12
  existing `/metrics` endpoints, their instrumentation, and the
  Prometheus-exposition-format machinery underneath them are unchanged by
  this document — this design consumes a small, curated slice of the
  *values* those endpoints already compute (via each service's own
  in-process state, not by scraping the endpoint itself), it does not touch
  how or whether those endpoints exist.
- **Resolving the `movementFeed: "kafka"` transitional-mode caveat
  (Decision 4).** This document flags it as a precondition for the
  "globally ingressed" figure reading correctly; completing that cutover is
  `2026-09-04-movement-relay-design.md`'s job, not this one's.
- **Direct Postgres/Redis liveness checks.** Flagged as a cheap, honest
  follow-up (Decision 2b), not designed or built here — this document's
  inference-from-dependents approach is judged sufficient for a first pass.
- **Any change to `railmcp`'s own health/observability.** It appears as a
  node in the dependency graph (Ground truth traces its real edges) but
  gets no new heartbeat/metrics-push mechanism of its own in this pass —
  it would need the same treatment as `movement-relay` (a new, first-time
  dependency on this reporting mechanism), left as a follow-up rather than
  bundled into this document's already-substantial scope.
- **Historical uptime/throughput data surviving a full database restore
  or migration.** Same posture this app already takes with
  `line_status_history`/`line_status_daily_stats` — operational history,
  not a system of record; no backfill mechanism is designed for either new
  table, consistent with every prior table of this shape in this repo.
- **An implementation plan.** Separate, later step, per this repo's
  process.

## Open questions / risks

1. **Exact `internal_oauth_group_*` naming/scoping for the new
   `POST /private/service-metrics` route** — an implementation-time
   convention choice (one shared group all reporting services share, or
   one group per service mirroring every other route's per-service
   granularity) — not fixed here.
2. **`movement-relay`'s new dependency on `api` (Decision 1) is a genuine,
   new architectural edge** that didn't exist before this document — worth
   a reviewer's explicit sign-off, the same way `2026-09-04-movement-relay-design.md`
   flagged Redis becoming a new dependency for `trust-consumer`/
   `full-coverage-consumer` as worth calling out plainly rather than
   burying in a subordinate clause.
3. **Staleness-threshold multiplier (2–3x cycle interval, Decision 2b) is
   an unvalidated placeholder**, same posture as every other first-guess
   cadence constant already flagged elsewhere in this repo's own docs
   (e.g. `trust-consumer/src/config.rs:126-128`'s
   `stanox_crs_reload_secs` comment) — not empirically tuned against real
   restart/redeploy timing.
4. **`metric_samples_retention_hours`'s proposed 7-day default (Decision
   1) is a reasoned starting point, not measured against real usage** of
   how far back anyone actually wants to look on this page.
5. **Whether `service_heartbeats`/`service_status_history` should also
   feed `notifier`** (i.e., should a service going down trigger a push
   notification the way a line-status change does today) is a real
   product question this document doesn't answer — flagged, not decided,
   since the brief only asked for a status *page*, not an alerting
   integration (see Non-goals).
6. **Whether Postgres/Redis should get a direct liveness probe from `api`**
   (Decision 2b's flagged-but-not-built follow-up) is worth a deliberate
   yes/no from whoever reviews this, rather than staying implicit.
7. **Exact schema/route naming throughout this document is illustrative,
   not final** — consistent with this repo's own stated convention for a
   design-stage document (`2026-09-04-movement-relay-design.md`'s own
   framing: "schema, route shapes... marked as a sketch, not final code").
