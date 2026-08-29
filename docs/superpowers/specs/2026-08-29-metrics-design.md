# Metrics Tracking in the Background — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md` and
`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`) so it
can be reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. It does **not**
contain a task-by-task implementation plan — that is a separate, later step
in this repo's process, done only after a design like this has been
reviewed.

## Problem

`docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s own Non-goals
section says, verbatim: "**No ServiceMonitor / metrics wiring.** The
services expose no metrics endpoint today." That line has stayed true since
— the chart has since grown an `enricher-deployment.yaml`, a
`redis-deployment.yaml`/`redis-service.yaml` and a `poller-deployments.yaml`
covering all five pollers, none of which changes the fact. Concretely,
today:

- `crates/api/src/main.rs` runs an `axum` server with `TraceLayer` for
  request *tracing* (line 34), but nothing that aggregates request
  counts/latencies into a queryable metric.
- `crates/aggregator/src/main.rs`'s `run_cycle` (lines 55–87) logs one
  `tracing::info!` line per cycle with `lines`, `incidents`,
  `removed_lines` and `pruned_history_rows` counts (lines 78–84) — useful
  for a human tailing logs, useless for graphing a trend or alerting on a
  stall, since nothing turns it into a time series.
- `crates/enricher/src/main.rs` runs three independent loops (stream
  consumer, hourly sweep, PEL reclaim) against an LLM endpoint whose
  request timeout was already raised once in production, from 60s to the
  current configurable default of 120s, "after a real remote self-hosted
  endpoint proved it too tight in practice"
  (`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`,
  lines 468–472). That is exactly the kind of regression a latency
  histogram would have surfaced before an operator had to notice extraction
  silently stalling.
- `crates/poller-tfl/src/main.rs` already has a documented,
  non-trivial retry policy (`MAX_ATTEMPTS = 3`, exponential backoff,
  `should_retry` distinguishing 429/5xx from a caller mistake — lines
  43–60) built around a rate limit the code's own comment admits is not
  reliably known: "TfL's registered free tier is documented at roughly 500
  requests per minute, but community reports say the enforcement is
  inconsistent" (lines 45–46). There is currently no way to see, in
  aggregate, how often this poller actually hits 429s.
- Every one of `charts/nr-status/templates/{aggregator,enricher,poller}-*.yaml`
  states, in its own comments, that the workload "exposes no HTTP surface at
  all" (`aggregator-deployment.yaml:40`), "like aggregator... exposes no
  HTTP surface" (`enricher-deployment.yaml:51`), and "none of the pollers
  expose an HTTP surface" (`poller-deployments.yaml:55`). The
  `networkpolicy.yaml` template repeats this three more times as the
  justification for a bare default-deny NetworkPolicy on each of those
  workloads (lines 88, 169, 185: *"Default-deny: \[X\] exposes no
  listener\[...\]"*). Seven of this app's eight Rust binaries are, by
  design, silent — reachable by nothing, and therefore invisible to
  anything that isn't reading their stdout logs.

This design proposes closing that gap: background instrumentation (request
counts/latencies, poll-cycle success/failure and timing, aggregator cycle
duration, LLM call latency/failure/retry, Redis Streams consumer lag, and
TfL rate-limit/retry behavior) across all eight services, plus the Helm
chart wiring the earlier doc explicitly deferred.

## Goals

1. Give every service a way to expose the operational signals this repo's
   own design docs already treat as important — poll-cycle health, LLM call
   latency/failure, stream consumer lag, external API retry/rate-limit
   behavior — as a scrapeable time series, not just log lines.
2. Pick a protocol/architecture that fits this app's actual topology: one
   binary with an HTTP server (`api`) and seven that today have none, all
   shipped as separate Docker images per `charts/nr-status`.
3. Close the specific gap `docs/superpowers/specs/2026-08-18-helm-chart-design.md`
   left open: no `/metrics` endpoint, no ServiceMonitor/scrape wiring in the
   chart.
4. Land a scoped, prioritized v1 — not blanket instrumentation of every
   function call — informed by what this codebase's own docs already flag
   as operationally sensitive (LLM timeout tuning, TfL's undocumented rate
   limit, Redis Streams' documented "disposable, no persistence" posture in
   `crates/enricher/src/main.rs` lines 94–98).
5. Fit existing conventions: `crates/common` as the place for cross-crate
   shared code (it already owns `ingest::post_batch`/
   `time_until_next_poll`, reused by all five pollers), env-only
   configuration via `clap`'s `env` feature, and the chart's existing
   posture of "no operator prerequisites, works on a bare cluster"
   (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s Goals: "no
   subchart dependencies... installable in an air-gapped cluster given the
   images").

## Non-goals (this pass)

- **Distributed tracing / spans.** `TraceLayer` (`crates/api/src/main.rs:34`)
  already gives per-request tracing inside `api`; this design is about
  *metrics* (aggregated counters/histograms/gauges), not adding span
  export or a tracing backend (Jaeger/Tempo/OTLP traces). A future design
  could layer tracing on top of whatever this doc lands on, but it's a
  separate concern with its own cardinality and backend questions.
- **Bundling a Prometheus/Grafana/Alertmanager stack in the chart.** Per
  the Helm chart design's own Goals ("no subchart dependencies... no
  operator prerequisites"), this design assumes the operator brings their
  own metrics backend — it wires scrape *targets*, not a metrics
  *platform*. See Open Questions for the reasoning this borrows from the
  chart's existing bundled-vs-assumed posture.
- **Dashboards or alerting rules.** No Grafana JSON, no `PrometheusRule`
  CRD. Once metrics exist and have stable names, dashboards are a
  follow-up, not part of defining what to instrument.
- **Per-line / per-station cardinality metrics.** The aggregator computes
  status for potentially 50-100+ lines (`DESIGN.md` §10: "production needs
  ~50-100 lines") and the ldbws poller samples one station per line's
  `sample_stations`. Labeling metrics by line id or station CRS is exactly
  the kind of unbounded-cardinality trap Prometheus users get bitten by;
  v1 aggregates at the service/cycle level only. See Open Questions.
- **DB query-level histograms.** `sqlx` query timing per call site is
  cheap to add later (a single macro or query-wrapper in `crates/common`),
  but no design doc currently treats query latency as an active pain point
  the way LLM timeout tuning and TfL rate limiting are — deferred to v2,
  not designed here.
- **HorizontalPodAutoscaler / custom-metrics-adapter wiring.** The Helm
  chart design already has an explicit non-goal here ("The aggregator and
  all four pollers are singleton loops that must not be scaled" —
  `docs/superpowers/specs/2026-08-18-helm-chart-design.md` line 39-41) that
  this design doesn't revisit. Exposing metrics doesn't imply autoscaling
  on them.
- **An implementation plan.** See the status note at the top.

## Research: protocol/architecture — Prometheus pull vs. OpenTelemetry push

The ask explicitly says to confirm this rather than assume it. Two
mainstream shapes fit "expose operational metrics from a set of Rust
services running in Kubernetes": a pull-based `/metrics` endpoint scraped
by Prometheus, or a push-based exporter (OTLP to a collector, or a
Prometheus push-gateway as a hybrid). Weighed against what this app
actually is:

- **This app is already a Kubernetes-native, Helm-deployed stack with a
  gap the earlier design explicitly named as "ServiceMonitor."** The
  vocabulary the prior design doc reaches for — "ServiceMonitor" — is
  Prometheus Operator's own CRD name, not a generic term. That's a strong
  signal about what shape of solution this repo's own prior design work
  already assumed, even though it didn't build it.
- **Pull fits "one process, restart-on-crash, no sidecar" better than push
  does for this app's failure modes.** Every non-`api` service here is a
  bare loop with `restartPolicy` as its only resilience mechanism (no
  probes, no sidecar, confirmed above). A push-based OTLP exporter needs a
  live collector endpoint configured and reachable *from inside* each of
  those loops, plus its own retry/buffering story for when the collector
  is briefly unreachable — one more moving part each of these already
  intentionally-minimal binaries would need to get right. A pull-based
  `/metrics` endpoint has no equivalent problem: the service just holds
  current values in memory; if nothing scrapes it for a while, nothing is
  lost except recent history, and there's no outbound call to fail or
  retry.
- **Pull matches this app's every other integration's directionality.**
  Every existing external integration in this codebase (Knowledgebase,
  LDBWS, Stations, TOCs, TfL) is *this app* initiating outbound calls on
  its own schedule (`crates/poller-*/src/main.rs`, all of them). A
  Prometheus-style pull endpoint is the same shape flipped once: the
  monitoring system, not this app, owns the schedule and the outbound
  call. An OTLP push exporter would be the first thing in this codebase
  where a background loop initiates traffic *to* an observability system
  rather than *to* a data source — a new directionality with no existing
  precedent to reuse.
- **The seven currently-silent binaries make the "does this need its own
  HTTP server" question look worse than it is.** The natural objection to
  pull is "aggregator/enricher/the five pollers have no HTTP surface,
  adding one just for metrics is new attack surface for infrastructure
  that deliberately has none today" — and the NetworkPolicy template's
  three "exposes no listener at all" comments (`networkpolicy.yaml` lines
  88, 169, 185) show that absence was a deliberate security posture, not
  an oversight. This is a real cost either way, but it turns out to be
  *smaller* under the pull design than it looks: `metrics-exporter-
  prometheus`'s `PrometheusBuilder::with_http_listener` spins up its own
  minimal `hyper`-based HTTP listener with no `axum` dependency at all
  (confirmed against its current docs.rs page, see crate table below) — so
  adding a scrape endpoint to the five pollers, the aggregator and the
  enricher does not mean pulling `axum` (currently only an `api` and
  `frontend` dependency) into six crates that have never needed a web
  framework. It's a few lines of `hyper`, already a transitive dependency
  of every one of these crates via `reqwest` (`crates/common/Cargo.toml:10`
  — `reqwest` with `default-features = false` still pulls `hyper` for its
  HTTP client). The new listening port is real and needs NetworkPolicy
  wiring (see Architecture below), but it is not a new *dependency
  category* the way OTLP's collector-and-protobuf stack would be.
- **OTLP push wins on vendor-neutrality, which this app doesn't currently
  need.** OpenTelemetry's actual differentiator — one wire protocol that
  fans out to Prometheus, Datadog, Honeycomb, Tempo, etc. via a collector
  — matters most for an app whose backend is unknown or expected to
  change. This app's chart already hard-codes its infra choices (bundled
  Postgres, bundled Redis, no pluggable-backend abstraction anywhere) —
  vendor-neutrality is not a value this codebase optimizes for elsewhere,
  so it's not a strong pull here either.
- **`metrics-exporter-prometheus` itself supports a push-gateway mode**
  (`with_push_gateway`, mutually exclusive with `with_http_listener`, gated
  behind a `push-gateway` feature — confirmed via its current docs.rs
  page). That's a real escape hatch if a future deployment genuinely can't
  permit inbound scraping (e.g. a very short-lived batch-style poller) —
  worth knowing it exists, without adopting it in v1 given nothing in this
  app's current architecture is that short-lived (the shortest poll
  interval is `poller-ldbws` at 60s, `docker-compose.yml` line 196).

**Conclusion: Prometheus-style pull `/metrics` endpoints, via the `metrics`
facade + `metrics-exporter-prometheus`'s embedded listener — not
OpenTelemetry.** This matches the prior design doc's own vocabulary, fits
every service's existing "outbound-initiator, no inbound surface by
default" shape better than an outbound push loop would, and does not force
`axum` into six crates that have never needed it. OpenTelemetry's
vendor-neutrality doesn't offset that, since nothing about this app's
current infra choices values backend-swappability. If a genuine multi-
backend requirement shows up later, `opentelemetry` + `opentelemetry-
prometheus` (both actively released, see below) is a viable migration path
that keeps the same underlying Prometheus wire format — this isn't a
one-way door.

## Research: Rust crate landscape (checked 2026-08-29, via the crates.io API
## and GitHub, not from training-data recall)

| Crate | Role | Latest stable | Released | Notes |
|---|---|---|---|---|
| [`metrics`](https://crates.io/crates/metrics) | Facade: `counter!`/`histogram!`/`gauge!` macros against a pluggable global recorder | 0.24.6 | 2026-05-13 | 102M+ all-time downloads — the de facto standard metrics facade in the Rust ecosystem, the `log`/`tracing` of metrics. Actively released. |
| [`metrics-exporter-prometheus`](https://crates.io/crates/metrics-exporter-prometheus) | Prometheus exporter/recorder for the `metrics` facade: `PrometheusBuilder` with `with_http_listener` (embedded `hyper` scrape server) or `with_push_gateway` | 0.18.3 | 2026-04-30 | 43M+ downloads. GitHub (`metrics-rs/metrics` monorepo) has commits as recent as 2026-08-04 (a `metrics-exporter-prometheus: use portable atomics` fix). Actively maintained, fast release cadence. |
| [`metrics-util`](https://crates.io/crates/metrics-util) | Helper types (`Handle`, quantile/summary helpers, upkeep) used across the `metrics` ecosystem | 0.20.4 | 2026-05-13 | 86M+ downloads, same release day as `metrics` core — released in lockstep. Transitive dependency, not one this app adds directly. |
| [`prometheus`](https://crates.io/crates/prometheus) (`tikv/rust-prometheus`) | Direct Prometheus client library: build/register `Counter`/`Histogram`/`Gauge` yourself, no facade layer | 0.14.0 | 2025-03-27 | 139M+ downloads (highest raw count, reflecting age/ecosystem entrenchment more than current activity), but the **most recent GitHub commit is 2025-10-17** — roughly 10 months stale as of this writing, and its last crates.io release predates that by another 7 months. No facade — every crate that wants to record a metric needs a direct `prometheus` dependency and hand-registered metric objects, which is more coupling across 8 crates than the facade pattern below. |
| [`opentelemetry`](https://crates.io/crates/opentelemetry) + [`opentelemetry-prometheus`](https://crates.io/crates/opentelemetry-prometheus) + [`opentelemetry-otlp`](https://crates.io/crates/opentelemetry-otlp) | OTel metrics API/SDK, with either a Prometheus-format exporter or an OTLP push exporter | 0.32.0 (all three) | 2026-05-08 (all three, same day) | 250M+ / 13.5M+ / 144M+ downloads respectively. Actively released, all three crates version-locked together. Real option; not chosen here because it's a materially heavier API surface (Meter providers, resource attributes, a separate SDK crate) for a pull-based-only v1 that doesn't need OTLP's multi-backend fan-out — see protocol reasoning above. |
| [`axum-prometheus`](https://crates.io/crates/axum-prometheus) | Tower/axum middleware that auto-records HTTP request count/latency/in-flight metrics via the `metrics` facade | 0.10.1 | 2026-07-31 | 5.2M+ downloads. GitHub's most recent commit is the 2026-07-31 v0.10.1 release itself — actively maintained, only ~4 weeks stale as of this writing. Builds directly on `metrics`, so it composes with the recommendation below rather than competing with it. |

**Recommendation: `metrics` (facade) + `metrics-exporter-prometheus`
(recorder/exporter), with `axum-prometheus` for `api`'s HTTP-layer metrics
specifically, and hand-instrumented `counter!`/`histogram!`/`gauge!` calls
elsewhere.** Reasoning:

- The facade/exporter split (`metrics` + `metrics-exporter-prometheus`) is
  the same shape this repo already uses for logging: `tracing` (facade,
  used everywhere via `tracing::info!`/`tracing::error!`) +
  `tracing-subscriber` (the backend that actually formats/emits it,
  configured once per `main.rs` — e.g. `crates/api/src/main.rs:37-39`). A
  metrics facade that every crate calls into, with the actual Prometheus
  wiring configured once per binary's `main.rs`, mirrors a pattern this
  codebase already has in every single one of its eight binaries, rather
  than introducing a new one.
- `prometheus` (direct) is passed over specifically for its GitHub
  staleness (10 months at the time of this check) relative to `metrics`/
  `metrics-exporter-prometheus`'s multi-times-a-quarter cadence, and
  because it has no facade — every one of the eight crates would need a
  direct dependency on it and its own hand-registered metric objects, a
  worse fit for `crates/common` owning shared metric definitions (see
  Architecture below) than a facade macro any crate can call without a
  registry handle.
- `opentelemetry`+`opentelemetry-prometheus` is passed over per the
  protocol-choice reasoning above: it's an actively-maintained, credible
  alternative (not rejected on maintenance grounds, unlike `prometheus`),
  but its API surface (Meter providers, `Resource` construction, SDK
  configuration) is heavier than this v1 needs for a pull-only, single-
  backend deployment, and nothing in this app's existing architecture
  currently benefits from OTel's vendor-neutrality.
- `axum-prometheus` is adopted for `api` specifically (not the other seven
  crates, which have no axum router to instrument) because it is the one
  place in this app where "HTTP request count/latency by route" is exactly
  what an off-the-shelf axum middleware already does well, and its release
  is the freshest of everything in this table (~4 weeks old at the time of
  this check). It builds on the same `metrics` facade recommended above,
  so it doesn't introduce a second, incompatible metrics system alongside
  it.
- Everywhere else (poller cycles, aggregator cycles, enricher LLM calls and
  stream lag) has no off-the-shelf middleware to reach for — those are
  hand-written `counter!`/`histogram!`/`gauge!` calls at the specific call
  sites identified in the v1 scope below, the same way this codebase
  already hand-writes its `tracing::info!`/`tracing::error!` calls rather
  than reaching for a framework that infers them.

## Architecture

### Per-service topology: who gets a `/metrics` endpoint

| Service | HTTP server today? | Change needed |
|---|---|---|
| `api` | Yes — `axum::serve` (`crates/api/src/main.rs:44`) | Add `axum-prometheus`'s middleware layer to the existing router (`crates/api/src/main.rs:29-35`) plus a `GET /metrics` route reading the exporter's render output. Cheapest case — reuses the listener and port `api` already has. |
| `aggregator` | No (`aggregator-deployment.yaml:40-41`: "No probes: this binary exposes no HTTP surface at all") | New: install `PrometheusBuilder::new().with_http_listener(...)`  at startup in `crates/aggregator/src/main.rs`, on a new port (proposed `9091`, see values below). No `axum` dependency added — the exporter runs its own `hyper` listener. |
| `enricher` | No (`enricher-deployment.yaml:51`: "No probes: like aggregator, this binary exposes no HTTP surface") | Same pattern as `aggregator`. |
| `poller-incidents`, `poller-stations`, `poller-tocs`, `poller-ldbws`, `poller-tfl` | No (`poller-deployments.yaml:55`: "No probes: none of the pollers expose an HTTP surface") | Same pattern, once per poller binary. Since `poller-deployments.yaml` already generalizes all five pollers through one templated `range` over `.Values.pollers` (`poller-deployments.yaml:9-111`), the *chart* change is one shared block; the *code* change is five near-identical `PrometheusBuilder` setup calls, one per poller's own `main.rs` (candidate for a `crates/common` helper — see below). |
| `frontend` | Yes (Next.js, `next start`) | Out of scope for this design. Next.js has its own metrics story (e.g. `prom-client` for Node) if ever wanted; this design is about the Rust services per the ask, and the frontend is explicitly excluded from every other cross-cutting design in this repo too (routing, sessions — see the SSO design's framing of the frontend as "no server-side identity of its own to protect"). |

### A shared `crates/common::metrics` helper

`crates/common` already owns the one piece of cross-cutting infrastructure
every poller (and `api`) shares: `ingest::post_batch` and
`ingest::time_until_next_poll` (`crates/common/src/ingest.rs`), with a
module doc explicitly framed as "Previously each poller ... independently
redefined these constants/logic; this module is the one place that
changes if either ... ever needs to" (`crates/common/src/ingest.rs:8-13`).
The same argument applies here: six binaries (`aggregator`, `enricher`,
and the five pollers) need the identical "start a `PrometheusBuilder` with
an HTTP listener on a configurable port, at startup, before entering the
main loop" boilerplate. Proposed: `crates/common::metrics::install(port:
u16)` (or similar), called once near the top of each `main()` — one place
that owns the exporter setup, matching `ingest`'s precedent instead of
five-plus-two copies of the same `PrometheusBuilder` call. `api` does not
call this helper (it composes the exporter into its existing `axum` router
via `axum-prometheus` instead, since it already has a listener to attach
to), but shared *metric naming* (a `nr_status_` prefix, consistent unit
suffixes) belongs in `crates/common` regardless of which binary emits it,
the same way `Severity` and the TfL severity mapping already live in
`crates/common/src/lib.rs` as the one shared domain vocabulary every crate
draws from.

### Helm chart changes

- **New `metrics.*` values block**, following the existing three-way
  secret-handling convention's spirit (explicit override vs. sane default)
  used throughout `values.yaml` (e.g. the `secrets:` block, lines 36-44):
  a `metrics.enabled` toggle (default `true` — cheap, in-process, no new
  runtime dependency once built), and a `metrics.port` (proposed `9091`
  for the six non-`api` workloads, distinct from `api.service.port`
  8080 and `frontend.service.port` 3000, avoiding collision).
- **Every Deployment template gains a second `containerPort`** for its
  metrics listener — `api-deployment.yaml`'s existing `ports:` block
  (lines 35-38) is the model; `aggregator-deployment.yaml`,
  `enricher-deployment.yaml` and `poller-deployments.yaml` currently
  render no `ports:` at all (nothing to name `containerPort: http` for,
  since "No probes" / "exposes no HTTP surface" — see table above), so
  this is new, not a modification of an existing block.
- **New Services for `aggregator`, `enricher` and each poller.** Today
  only `api` (`api-service.yaml`) and `frontend`
  (`frontend-service.yaml`) have `Service` objects — the other six
  workloads have never needed one, since nothing addresses them by name.
  A `ClusterIP` (or headless) `Service` exposing just the metrics port is
  the simplest way to give Prometheus Operator's `ServiceMonitor` CRD
  something to select; the alternative — a `PodMonitor` CRD, which
  selects pods directly by label with no `Service` required — avoids
  creating six new `Service` objects purely as scrape-target plumbing.
  **Recommendation: `PodMonitor`, not `ServiceMonitor`,** for uniformity
  across all eight workloads (including `api`, so there's one scrape
  mechanism, not two) and because it avoids inventing six `Service`
  objects whose only purpose would be satisfying `ServiceMonitor`'s
  selector requirement.
- **`metrics.podMonitor.enabled`, default `false`.** `PodMonitor` is a
  Prometheus Operator CRD, not a core Kubernetes resource — assuming it
  exists would violate the chart's own stated Goal of "no operator
  prerequisites" (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`,
  Goals). This mirrors `networkPolicy.enabled`'s existing default-off
  reasoning almost verbatim: *"many clusters run no CNI that enforces it,
  and a silently-unenforced policy is worse than an absent one"* — the
  identical argument applies to a CRD that may not be installed: a
  silently-unrendered `PodMonitor` (because the CRD doesn't exist and
  `helm install` would otherwise fail) is worse than an explicit opt-in.
- **`prometheus.io/scrape`, `prometheus.io/port`, `prometheus.io/path` pod
  annotations, rendered unconditionally whenever `metrics.enabled` is
  true**, as the CRD-free fallback — the old-style annotation-based
  discovery a self-managed Prometheus using `kubernetes_sd_configs` can
  use with zero CRDs installed. This keeps the "install and get value
  with no operator prerequisites" posture intact even for the metrics
  feature specifically, rather than making metrics collection depend on
  Prometheus Operator being present.
- **NetworkPolicy**: each of the currently-"default-deny, no listener"
  policies (`networkpolicy.yaml` lines 88, 169, 185, and the equivalent
  implicit default-deny on the poller policies) needs an explicit `allow`
  for the metrics port once one exists, sourced from a configurable
  `networkPolicy.monitoringNamespace` (mirroring the existing
  `networkPolicy.ingressControllerNamespace` pattern used for the
  ingress-controller allow at `networkpolicy.yaml` lines 119-126), gated
  the same way — only rendered when `networkPolicy.enabled` is true.
- **No change to `secret.yaml`.** Nothing proposed here requires
  authenticating the scrape itself in v1 (see Open Questions for why that
  might change later) — the endpoint's protection is "not reachable
  outside the cluster" (no Ingress route, ever) plus NetworkPolicy when
  enabled, not a bearer token.

## What's instrumented in v1

Prioritized by what this codebase's own docs already flag as
operationally load-bearing, not an exhaustive per-function sweep:

1. **`api` HTTP request count/latency/in-flight, by route and status
   code**, via `axum-prometheus`'s middleware on the existing router
   (`crates/api/src/main.rs:29-35`). Cheapest to add (off-the-shelf
   middleware, existing listener) and the most directly comparable to
   "is the read API healthy" — the whole point of `DESIGN.md` §1's
   TfL-shaped endpoints.
2. **Poller cycle outcome and duration, per poller.** Every poller's
   `main.rs` already wraps its cycle in `if let Err(err) =
   poll_once(...).await { tracing::error!(...) }` (identical shape in
   `poller-incidents/src/main.rs:49-51`, `poller-ldbws/src/main.rs:56-58`,
   and `poller-tfl/src/main.rs:106-108`) — a `counter!` for
   success/failure and a `histogram!` for cycle duration wraps that exact
   call site, one per poller, with a `poller` label (`incidents`,
   `stations`, `tocs`, `ldbws`, `tfl` — a small, fixed set, not user data,
   so no cardinality risk).
3. **`poller-tfl` retry/rate-limit behavior specifically**, since it's the
   one poller whose own code already tracks retryable-vs-fatal status
   codes and admits the actual rate limit is unconfirmed
   (`crates/poller-tfl/src/main.rs:43-60`). A `counter!` in `fetch_json`
   (lines 241-265) labeled by outcome (`success`, `retried_429`,
   `retried_5xx`, `exhausted`) turns "community reports say the
   enforcement is inconsistent" from a shrug into an observable rate.
4. **Aggregator cycle duration and per-cycle counts.** `run_cycle`
   already computes everything a metric needs — `reports.len()`,
   `incidents.len()`, `removed`, `pruned` — right before logging them
   (`crates/aggregator/src/main.rs:78-84`). Emitting the same values as a
   `histogram!` (cycle duration, wrapping the whole function body) and
   `gauge!`/`counter!`s (lines processed, incidents loaded, rows pruned)
   is close to free — the values already exist at that call site, this is
   "also record it as a metric," not new computation.
5. **`enricher` LLM call latency and outcome, by call site.**
   `process_incident` makes three sequential LLM calls per incident
   (`extract_primary`, `extract_adversarial`,
   `extract_severity_adversarial` — `crates/enricher/src/main.rs:228-250`)
   against a request timeout that was already tuned once in production
   (120s, up from an initial 60s —
   `docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`
   lines 468-472). A `histogram!` around each call (labeled by call site)
   with bucket boundaries extending past 120s (so a call that's *about* to
   time out is visible as "slow," not just invisible until it becomes a
   binary failure) directly serves the exact tuning problem this repo has
   already had to solve once by trial and error.
6. **`enricher` Redis Streams consumer-group lag.** `crates/enricher/src/
   main.rs`'s own comment block explains why this queue is a soft
   dependency: "Redis is deployed WITHOUT persistence on purpose — it is a
   disposable trigger queue, not a system of record" (lines 94-98), with
   the hourly sweep as the correctness backstop. That's precisely the kind
   of soft dependency worth watching from the outside: a `gauge!` sourced
   from `XINFO GROUPS incident-text-changed` (Redis 7's `lag` field —
   available given `docker-compose.yml:69` already pins `redis:7`; not
   currently queried anywhere in `crates/enricher/src/stream.rs`, so this
   is a new call, not reusing an existing one), sampled on the same
   interval as the reclaim loop (`config.reclaim_interval_secs`,
   `crates/enricher/src/main.rs:71-79`), turns "consumer group fell behind
   or a pod restart nuked the group" (already handled defensively in code,
   `main.rs:93-107`) into something an operator can see happening rather
   than infer from log volume.
7. **`enricher`'s `MismatchTracker` consecutive-failure counts as a
   gauge**, not just a log line. The tracker already exists specifically
   "to satisfy design §7 item 3's operational-visibility requirement"
   (`crates/enricher/src/main.rs:121-135`) and already distinguishes a
   persistent per-incident failure from transient noise in its log output
   (lines 258-271) — exposing `counts.len()` (incidents currently stuck)
   and a counter for the "persistent length mismatch" log path turns
   visibility that today only exists in `grep`-the-logs form into
   something gauge-able.

**Explicitly deferred, not v1:**

- Per-line / per-station labeled metrics anywhere (aggregator match
  outcomes by line, ldbws sample results by station) — cardinality risk,
  see Non-goals and Open Questions.
- DB query-level timing (`sqlx` per-query histograms) — no existing doc
  treats this as an active pain point; cheap to add later behind the same
  `crates/common` helper once it exists.
- `poller-incidents`/`poller-stations`/`poller-tocs` retry-specific
  metrics beyond the generic cycle success/failure counter in item 2 —
  unlike `poller-tfl`, none of these three currently implement in-cycle
  retry logic to instrument (confirmed against each `main.rs`: they call
  `.error_for_status()?` once and bail, no retry loop), so there is
  nothing poller-tfl-shaped to add for them yet.
- Any push-based (OTLP or push-gateway) path — noted as available if a
  future requirement demands it, not built in v1.
- Dashboards, alerting rules, a bundled Prometheus/Grafana stack — see
  Non-goals.
- Authenticating the `/metrics` endpoint — deferred, see Open Questions.

## Proposed metric names (illustrative, not final)

Following Prometheus naming convention (unit-suffixed, `_total` for
counters), all prefixed `nr_status_` to avoid collision with anything
`metrics-exporter-prometheus`'s own process-level defaults might emit:

| Metric | Type | Labels |
|---|---|---|
| `nr_status_http_requests_total` / `nr_status_http_request_duration_seconds` | counter / histogram | via `axum-prometheus` defaults (`method`, `path`, `status`) |
| `nr_status_poller_cycle_total` | counter | `poller` (`incidents`\|`stations`\|`tocs`\|`ldbws`\|`tfl`), `result` (`success`\|`failure`) |
| `nr_status_poller_cycle_duration_seconds` | histogram | `poller` |
| `nr_status_tfl_fetch_total` | counter | `what` (`line-status`\|`dlr-arrivals`\|`dlr-timetable`), `outcome` (`success`\|`retried_429`\|`retried_5xx`\|`exhausted`) |
| `nr_status_aggregator_cycle_duration_seconds` | histogram | none |
| `nr_status_aggregator_lines_total` / `nr_status_aggregator_incidents_loaded` / `nr_status_aggregator_history_rows_pruned_total` | gauge / gauge / counter | none |
| `nr_status_enricher_llm_call_duration_seconds` | histogram | `call` (`primary`\|`resolution_adversarial`\|`severity_adversarial`) |
| `nr_status_enricher_llm_call_total` | counter | `call`, `outcome` (`success`\|`error`\|`timeout`) |
| `nr_status_enricher_stream_lag` | gauge | none (single stream/group, per §5.1's non-goal on cardinality) |
| `nr_status_enricher_mismatch_incidents` | gauge | none |

## Open questions / risks

1. **Cardinality risk from per-line or per-station labels.** Explicitly
   excluded from v1 (Non-goals). If a future iteration wants "which line
   is the matcher spending time on," it needs a bounded-cardinality
   approach (e.g. a histogram of match duration with no line label, plus
   a separate low-frequency export like a periodic log line or a `/debug`
   endpoint for per-line detail) rather than a `line_id`-labeled
   Prometheus series against a 50-100+ line catalogue that's expected to
   grow (`DESIGN.md` §10).
2. **Whether a Prometheus/Grafana stack is assumed to already exist in the
   target cluster.** This design's answer is *yes, assumed, not bundled* —
   directly following the Helm chart design's own posture, which bundles
   Postgres and Redis (things this app cannot function without) but treats
   both as swappable for a managed equivalent (`postgresql.enabled: false`
   + `externalDatabase`, and the newer `redis.enabled` toggle implied by
   `redis-deployment.yaml`/`redis-service.yaml`'s existence), and bundles
   *nothing* for things the app can run without (no ServiceMonitor/
   PodMonitor CRD install, no bundled Prometheus). Metrics collection is
   squarely in the "app can run without it" category — `metrics.enabled:
   false` should leave every service working exactly as it does today.
   Worth confirming this assumption explicitly with whoever reviews this,
   since it's the single biggest scope decision in the whole design (bundle
   vs. assume, same axis the chart design already drew a line on for its
   own dependencies).
3. **Should `/metrics` require the internal token?** This design's default
   is no — the endpoint is cluster-internal only (no Ingress route, ever;
   NetworkPolicy-gated when enabled), which is a materially smaller
   exposure than `/private/*`'s own posture (also cluster-internal by
   convention, but additionally token-gated because it accepts writes).
   `/metrics` is read-only and, per the v1 scope above, carries no
   per-line/per-station/per-user data — but if a reviewer judges even
   aggregate cycle counts and LLM-call-volume information sensitive
   enough to gate, the existing `require_internal_token` pattern
   (`crates/api/src/auth.rs`) is directly reusable for `api`'s `/metrics`
   route; the six non-axum services would need a much smaller bespoke
   check (`metrics-exporter-prometheus`'s `with_http_listener` doesn't
   support pluggable auth out of the box, so gating there means either a
   different exporter path or wrapping the listener — not researched
   further here since v1 assumes no gate is needed).
4. **New listening ports on seven previously-silent binaries is a real,
   if small, security-posture change.** The NetworkPolicy template's
   "exposes no listener at all" comments (lines 88, 169, 185) were a
   deliberate choice, not an oversight — this design deliberately reverses
   part of that for `aggregator`, `enricher` and all five pollers. The
   mitigations proposed (NetworkPolicy allow scoped to the monitoring
   namespace only, when NetworkPolicy is enabled at all; no Ingress route
   ever) reduce but don't eliminate this — worth a reviewer's explicit
   sign-off given it's a repo-wide pattern being broken, not a
   single-service decision.
5. **Error taxonomy for poll-cycle `result` labels.** `poll_once` returns
   `anyhow::Result<()>`, which carries a free-form error message, not a
   typed error. Using the raw error string as a label value would be a
   cardinality footgun (every distinct error message becomes a new time
   series). v1's proposed `result: success|failure` label sidesteps this
   entirely by not trying to distinguish failure *reasons* in the metric
   (reasons stay in the paired `tracing::error!` log line, which already
   captures the full error via `error = ?err`) — worth flagging as a
   deliberate simplification rather than an oversight, in case a reviewer
   wants a small bounded reason enum (e.g. `network`\|`parse`\|`ingest`)
   instead.
6. **`PodMonitor` vs. `ServiceMonitor` is a real, opinionated choice made
   here, not researched exhaustively.** Prometheus Operator supports both;
   this design picked `PodMonitor` for uniformity and to avoid six new
   `Service` objects, but a reviewer already running `ServiceMonitor`-based
   scrape config elsewhere in their cluster might prefer consistency with
   their existing setup over this reasoning. Both are cheap to support
   (mutually non-exclusive — nothing stops rendering both, gated by
   separate toggles) if that turns out to matter.
7. **`metrics.port` default (proposed `9091`) is unchecked against any
   convention beyond "distinct from 8080/3000/5432/6379."** No existing
   value in this chart's `values.yaml` establishes a metrics-port
   convention to match, since none of these six workloads have ever had a
   second port. Fine as a placeholder; worth a deliberate bikeshed rather
   than silent acceptance.
8. **This is the second design in this repo to propose adding a
   dependency + listener footprint to `crates/common`/every crate in short
   succession** (the SSO design proposes `openidconnect`+`oauth2` for
   `api` only; this one proposes `metrics`+`metrics-exporter-prometheus`
   for all eight crates). Worth sequencing awareness if both land close
   together — no functional conflict identified, but two cross-cutting
   infra changes touching every `main.rs` around the same time is worth a
   reviewer noting explicitly rather than discovering via merge conflicts.
