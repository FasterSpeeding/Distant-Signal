# Design: Status/Observability, Respec'd Around a Real Grafana Deployment

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass.**

## This document supersedes part of an existing spec — read this first

`docs/superpowers/specs/2026-09-05-status-observability-page-design.md`
("Design: A Status/Observability Page — Dependency Graph, Historical
Uptime, Trust-Event Throughput") already exists, merged to `main`. Its
central call, **Decision 1**, was to reject deploying Prometheus and
instead roll a custom Postgres-backed metrics-storage pipeline inside
`distant-signal` itself (`service_metric_samples`,
`POST /private/service-metrics`, `service_heartbeats`,
`service_status_history`) — reasoned through carefully, but resting on one
load-bearing premise stated in that document's own words: *"This cluster
has no Prometheus, Grafana, or any monitoring/collection stack."*

**That premise is no longer true.** Since that document was written, the
repo owner deployed a real `kube-prometheus-stack` (Prometheus +
Alertmanager + Grafana) to the live `mine-bringer` cluster via
`Ranma-Config` (`clusters/base/apps/monitoring.yaml` +
`clusters/mine-bringer/apps/monitoring.yaml`). This is not a proposal —
it is real, already-merged GitOps config for infrastructure that now
exists. Everything in the prior document that was reasoned from "there is
no monitoring stack" needs re-deriving from "there is one, but it isn't
wired up to see this app yet, and it's deployed with constraints (tailnet-
only) that don't automatically satisfy every part of the original ask."

**What this document supersedes, precisely:** the prior document's
Decision 1 (custom Postgres storage for metrics/uptime) is superseded in
full — rejected in favor of using the real Prometheus that now exists.
Decision 2 (dependency graph/uptime) and Decision 4 (throughput graphs)
are superseded in their *mechanism* (no `service_heartbeats`,
`service_status_history`, or `service_metric_samples` tables; no
`POST /private/service-metrics`) but several of their *underlying
findings* — the real service topology, the real edges, the real health-
signal inventory, the real metric names — remain accurate and are reused
here by reference rather than re-derived. Decision 3 (`/public/freshness`)
and Decision 6 (public-vs-private posture reasoning) are **not**
superseded — they're restated and reused, because they were never
premised on "no monitoring stack" in the first place. A future reader
should treat the 2026-09-05 (page-design) document as historical record of
a decision made under since-changed facts, and this document as the
current answer.

## What was asked for, verbatim (unchanged from the prior document)

> a status page which shows metrics such as when data was last received,
> graphs for how many trust events are being processed and ingressed (both
> globally and by individual services), and to give the statuses of the
> microservices

Follow-up clarifications: microservice status should render as a
**dependency graph**, with live status overlaid per node; the page should
also carry **historical uptime** per service, not just current status.

## Ground truth: what's actually deployed in `Ranma-Config`, read fresh

All three files below were read in full from a fresh clone of
`git@github.com:FasterSpeeding/Ranma-Config.git`.

- **`clusters/base/apps/monitoring.yaml`**: a `HelmRepository` +
  `HelmRelease` for `kube-prometheus-stack`, chart version `89.2.2`,
  release **name `kube-prometheus-stack`**, namespace `monitoring`. Values:
  `kubeControllerManager`/`kubeScheduler`/`kubeEtcd` disabled (k3s doesn't
  expose these — a documented kube-prometheus-stack-on-k3s gotcha, per that
  file's own comment); `kubeApiServer`/`kubeProxy` left at chart defaults
  (enabled, since k3s does expose both). Grafana enabled with Authentik
  generic-OAuth wired (`role_attribute_path` maps `grafana-admins`→Admin,
  `grafana-editors`→Editor, else Viewer; `allow_sign_up: true` because
  group membership, not account existence, is the real gate). Prometheus
  and Alertmanager are explicitly commented as staying **ClusterIP-only,
  no ingress of their own** — "this stack's one intended human-facing
  surface is Grafana."
- **`clusters/mine-bringer/apps/monitoring.yaml`**: the cluster overlay.
  Grafana's ingress is `ingressClassName: tailscale`, hostname
  `ranma-grafana`, tag `intranet-private` — the file's own comment states
  this is "Private, tailnet-only... not exposed via the public
  cursed.solutions/Cloudflare Tunnel path Distant-Signal/authentik use,"
  explicitly modeled on `headlamp.yaml`'s identical pattern (confirmed by
  reading `clusters/base/apps/headlamp.yaml`: same `ingressClassName:
  tailscale`, same `tag:k8s,tag:intranet-private` annotation, same
  no-`secretName`-TLS shape). Real OAuth wiring points at
  `sso.cursed.solutions` — the same real Authentik instance
  `distant-signal` itself uses — with a sealed `client-secret`. **Treated
  here as a hard constraint, not something to reconsider**: this instance
  is genuinely not reachable from outside the tailnet, full stop.
- **Neither file overrides `prometheus.prometheusSpec.{podMonitorSelector,
  podMonitorNamespaceSelector,serviceMonitorSelector,
  serviceMonitorNamespaceSelector,*SelectorNilUsesHelmValues}`** — grepped
  both files for `Selector`; zero matches. Whatever the chart's own
  defaults do, unmodified, is what's actually running. This matters
  enormously for the scrape-gap analysis below and is not a detail either
  Ranma-Config file addresses today.
- **Neither file overrides `grafana.sidecar.dashboards.*`** either — same
  grep, zero matches. Also load-bearing below (dashboard provisioning).

## The scrape gap — resolved concretely, not assumed

### Finding 1: `charts/distant-signal`'s `PodMonitor` selector is missing three real components, not four

`charts/distant-signal/templates/podmonitor.yaml` (already merged, gated
behind `.Values.metrics.enabled && .Values.metrics.podMonitor.enabled`,
currently `podMonitor.enabled: false` by default per `values.yaml:1455-
1461` — "off by default... rendering a CRD that doesn't exist would fail
`helm install` outright"). Its `matchExpressions` on
`app.kubernetes.io/component` lists: `api`, `aggregator`, `enricher`,
every enabled `poller-<name>`, `schedulefeed`. Missing:
**`movement-relay`, `trust-consumer`, `full-coverage-consumer`** —
confirmed by reading each deployment template: all three carry a
container port literally named `metrics`
(`movement-relay-deployment.yaml:71-72`,
`trust-consumer-deployment.yaml:81-82`,
`full-coverage-consumer-deployment.yaml:75-76`) and a
`prometheus.io/scrape`/`prometheus.io/port` annotation pair pointing at
that same port (`:43-44`, `:53-54`, `:47-48` respectively) — exactly the
CRD-free fallback the podmonitor.yaml template's own comment describes,
which is real but which `kube-prometheus-stack`'s Prometheus does **not**
read (it discovers targets via `PodMonitor`/`ServiceMonitor` CRDs, not
legacy annotations, by design of the Prometheus Operator model these
CRDs implement).

**Correction to this task's own brief**: the brief named `notifier` as a
fourth service with existing scrape annotations, missing from the
selector. This is not accurate — verified by reading
`crates/notifier/src/main.rs`, every `crates/notifier/src/*.rs` file, and
`notifier-deployment.yaml` in full: `notifier` has **zero** metrics
instrumentation today. No `prometheus.io/scrape` annotation on its
Deployment, no `metrics::counter!`/`gauge!`/`histogram!` call anywhere in
its crate, and `crates/notifier/src/config.rs` has no
`metrics_enabled`/`metrics_bind`/`metrics_port` field at all (contrast:
grepping `metrics_enabled|metrics_bind|metrics_port` across every other
crate's `config.rs` returns exactly 13 hits: `aggregator`, `enricher`,
`full-coverage-consumer`, `movement-relay`, `trust-consumer`, all 5
pollers, `schedule-ingest`, `schedule-reference`, and `api`'s own
`data/config.rs` — `notifier` is not among them). Adding `notifier` to the
`PodMonitor` selector today would be a harmless no-op (a pod with no port
named `http`/`metrics` simply isn't matched by either
`podMetricsEndpoints` entry, per the template's own comment), but it
would misleadingly imply instrumentation that doesn't exist. **Fix: add
`movement-relay`, `trust-consumer`, `full-coverage-consumer` only.**
Instrumenting `notifier` is out of scope here — flagged as a real, small,
pre-existing gap in `2026-08-29-metrics-design.md`'s own coverage, not
something this document invents work to close.

(Also as a fresh-verification footnote: the prior document's Correction 3
said "12 crates set `metrics_enabled`" — the real count today is **13**
by the same grep; new metrics work landed since that count was taken.
Doesn't change any conclusion, noted for accuracy.)

### Finding 2: `PodMonitor`, not `ServiceMonitor`, was the right call — and remains it

Confirmed independently: `ls charts/distant-signal/templates/*service*.yaml`
returns exactly `api`, `frontend`, `postgres`, `redis`, `railmcp`,
`schedulefeed` (plus the dev-Authentik pair) — `movement-relay`,
`trust-consumer`, `full-coverage-consumer`, `aggregator`, `enricher`, and
every poller have **no** Kubernetes `Service` object. A `ServiceMonitor`
selects **Services** (and scrapes the Pods behind them); a `PodMonitor`
selects **Pods directly by label**, no Service required. Since most of
the missing/needed-scrape workloads have no Service and inventing one for
each purely to satisfy `ServiceMonitor` would be pure new-object churn for
no other purpose, `PodMonitor` remains correct. `podmonitor.yaml` itself
doesn't state this reasoning in its own comments (checked — it only
explains the CRD-gating and the two-endpoint port-name split), so this is
this document's own reasoning, not a re-quote.

### Finding 3: namespace discovery is *not* a gap — verified against real Prometheus Operator source and the real rendered chart output, not assumed

This was the task's own open question, and it resolves cleanly in
`distant-signal`'s favor:

- Rendered the actual `kube-prometheus-stack-89.2.2` chart
  (`helm template ... -s templates/prometheus/prometheus.yaml`, release
  name `test`) to see what the *Prometheus CR itself* ends up with when
  no overriding values are given (exactly `Ranma-Config`'s situation — it
  overrides neither selector):
  ```
  podMonitorSelector:
    matchLabels:
      release: "test"
  podMonitorNamespaceSelector: {}
  ```
  (mirrored identically for `serviceMonitorSelector`). This is because the
  chart's own `values.yaml` defaults `podMonitorSelectorNilUsesHelmValues:
  true` (and `podMonitorSelector: {}`, which Go templates treat as falsy)
  — so the template's `{{ else if
  .Values.prometheus.prometheusSpec.podMonitorSelectorNilUsesHelmValues
  }}` branch fires, forcing a `release: <helm release name>` label match.
  `podMonitorNamespaceSelector` independently defaults to the literal
  value `{}` (not omitted, not nil — an explicitly-set empty object).
- Cloned `prometheus-operator/prometheus-operator` directly and read
  `pkg/operator/operator.go`'s `SelectNamespacesFromCache`: `if sel == nil
  { return []string{obj.GetNamespace()} }` — **only a nil selector**
  restricts discovery to the Prometheus resource's own namespace. A
  non-nil-but-empty `*metav1.LabelSelector` (exactly what `{}` unmarshals
  to, and exactly what's rendered here) falls through to
  `metav1.LabelSelectorAsSelector`, which turns an empty selector into
  `labels.Everything()` — matching every namespace in the cluster,
  `distant-signal` included.
- **Conclusion, directly verified rather than inferred from chart
  documentation prose**: Prometheus already searches every namespace,
  including `distant-signal`, for `PodMonitor` objects. Namespace scoping
  requires zero change in either repo.

### Finding 4: the real, load-bearing gap — a missing `release: kube-prometheus-stack` label

Because `podMonitorSelectorNilUsesHelmValues` is left at its chart
default (`true`), the actual rendered `Prometheus` CR's `podMonitorSelector`
requires the label `release: kube-prometheus-stack` (the literal
`HelmRelease.metadata.name` from `monitoring.yaml:25`) on any `PodMonitor`
object it will select — this is `kube-prometheus-stack`'s own convention
for auto-discovering the `ServiceMonitor`/`PodMonitor` objects its own
bundled subcomponents (kube-state-metrics, node-exporter, etc.) ship with,
and it applies to *any* `PodMonitor` in the cluster, not just its own.
`charts/distant-signal/templates/podmonitor.yaml:26-28` labels the object
via `distant-signal.labels` (component `metrics`) — the chart's own
standard `app.kubernetes.io/*` labels, **no `release:` label at all**.
Flipping `metrics.podMonitor.enabled: true` alone, with the selector fix
from Finding 1, is **not sufficient** — the object would render, but
Prometheus's own `podMonitorSelector` would not match it, and it would sit
invisible with zero scrape targets, silently.

**Two ways to close this, weighed:**

- **(a) Hardcode/parameterize the `release: kube-prometheus-stack` label
  onto `charts/distant-signal`'s `PodMonitor` object.** Rejected as the
  primary fix: it makes `charts/distant-signal` — a chart whose own prior
  design docs (`2026-08-18-helm-chart-design.md`'s Goals, quoted in the
  prior status-page spec: "no subchart dependencies... no operator
  prerequisites") deliberately keep monitoring-stack-agnostic — silently
  depend on the *exact Helm release name* a completely separate chart, in
  a separate repo, in a separate namespace, happens to be installed under
  today. If `Ranma-Config` ever renames that `HelmRelease` (or a future
  cluster installs `kube-prometheus-stack` under a different release
  name), this breaks silently with no error, only a Prometheus target
  count of zero. It would also need a new `values.yaml` knob
  (`metrics.podMonitor.additionalLabels` or similar) purely to carry a
  fact that belongs to the *cluster's* monitoring topology, not the app's.
- **(b) In `Ranma-Config`'s `monitoring-config` ConfigMap, set
  `prometheus.prometheusSpec.podMonitorSelectorNilUsesHelmValues: false`
  (and the same for `serviceMonitorSelectorNilUsesHelmValues`, for
  consistency, though nothing needs it yet).** With `podMonitorSelector`
  left at its own default `{}`, this makes Prometheus select **all**
  `PodMonitor` objects cluster-wide regardless of label — this is
  `kube-prometheus-stack`'s own documented, standard answer to exactly
  this situation (confirmed from the chart's own `README.md`, "Discovery
  of PodMonitors/ServiceMonitors outside of Helm release" section: *"An
  easy way of doing this, without compromising the default
  PodMonitors/ServiceMonitors discovery, is allowing Prometheus to
  discover all PodMonitors/ServiceMonitors within its namespace [sic —
  the setting itself, per Finding 3, actually removes the namespace
  restriction too], without applying label filtering. To do so, you can
  set `prometheus.prometheusSpec.podMonitorSelectorNilUsesHelmValues` and
  `serviceMonitorSelectorNilUsesHelmValues` to `false`"*). **Chosen.** One
  cluster-level decision, made once, in the repo that already owns every
  other cluster-topology fact (the Grafana OAuth wiring, the ingress
  hostname, the tailnet posture) — not a new per-app-chart convention
  every future chart in this cluster would otherwise have to independently
  learn and hardcode. `charts/distant-signal` stays exactly as
  monitoring-stack-agnostic as its own prior design intended.

### Finding 5: a NetworkPolicy gap the task's own brief didn't flag — real, and specific to `schedulefeed`

`clusters/mine-bringer/apps/distant-signal.yaml`'s standalone
`distant-signal-schedulefeed-sftp-ingress` `NetworkPolicy` (lines 289-309)
selects `component: schedulefeed` pods and allows **only** `TCP 2022`
ingress, from any source. Its own comment (lines 274-281) currently reads:
*"this cluster has no Prometheus/monitoring stack... so leaving that port
out of this policy changes nothing observable today"* — **that comment is
now stale**, given the point of this whole document. Kubernetes
`NetworkPolicy` semantics: once *any* `Ingress`-type policy selects a pod,
that pod's ingress becomes default-deny except what's explicitly
permitted by the union of policies selecting it — this is the *only*
`NetworkPolicy` selecting `schedulefeed`, and it permits only port 2022.
Confirmed this cluster's CNI (kube-router) actually enforces
`NetworkPolicy` per that same comment's own prior verification. **Net
effect: even after Findings 1 and 4 are both fixed, Prometheus (running in
the `monitoring` namespace) will be unable to reach `schedulefeed`'s
`:9091` metrics port** — every other newly-or-already-selected component
(`api`, `aggregator`, `enricher`, the 5 pollers, `movement-relay`,
`trust-consumer`, `full-coverage-consumer`) has **no** `NetworkPolicy`
selecting it at all (`networkPolicy.enabled` is the chart's own default,
`false`, never overridden anywhere in `Ranma-Config` — confirmed by grep),
so Kubernetes' own default-allow-when-unselected rule means Prometheus can
already reach all of those with no network-policy change. **Fix, scoped
to `Ranma-Config` only** (this `NetworkPolicy` already lives there, as a
standalone resource, explicitly *not* driven by the chart's own
`templates/networkpolicy.yaml`): add a second `ingress` rule to
`distant-signal-schedulefeed-sftp-ingress` allowing TCP `9091` from a
`namespaceSelector` matching the `monitoring` namespace specifically
(not "any source," unlike the SFTP rule — there's no reason to open the
metrics port to the whole cluster when only Prometheus needs it), and
update the now-inaccurate comment.

### Answering the task's framing directly

Flipping `podMonitor.enabled: true` is **necessary but not sufficient**.
The full list, precisely: (1) fix the `PodMonitor` selector in
`charts/distant-signal` (add 3 components, not 4); (2) flip
`metrics.podMonitor.enabled: true` from `Ranma-Config`'s
`distant-signal-config` values overlay; (3) set
`podMonitorSelectorNilUsesHelmValues: false` in `Ranma-Config`'s
`monitoring-config` ConfigMap; (4) add a `schedulefeed`-scoped
`NetworkPolicy` allow-rule in `Ranma-Config`. Namespace discovery
(the task's explicitly flagged open question) needs **no** change —
verified, not assumed.

## Re-deriving each original ask against Grafana

### "Graphs for trust events processed/ingressed, globally and per-service"

Squarely a Grafana dashboard panel now — no new storage, no new endpoint.
Real counters (verified fresh against every crate's actual
`metrics::counter!`/`gauge!`/`histogram!` call site, not the prior
document's already-14-day-stale list):

- `distant_signal_movement_relay_events_published_total{msg_type}`
  (`crates/movement-relay/src/main.rs:96-100`) — the one true Kafka
  ingress point once the `movementFeed: "kafka"` transitional mode
  (`2026-09-04-movement-relay-design.md`) is fully retired.
- `distant_signal_trust_consumer_events_received_total{msg_type}` and
  `distant_signal_trust_consumer_events_matched_total` (no label)
  (`crates/trust-consumer/src/process.rs:322-331`).
- `distant_signal_full_coverage_consumer_events_matched_total{line_id}`
  plus its sibling `stream_gap_detected_total`,
  `cycle_duration_seconds` histogram, and `lines_available_total`/
  `lines_pending_total`/`stations_available_total` gauges
  (`crates/full-coverage-consumer/src/main.rs:222-481`).

**Real PromQL shapes a dashboard needs** (naming the queries, not
building panel JSON, per this task's own scope):

- **Global ingress, rate**: `sum(rate(distant_signal_movement_relay_events_published_total[5m]))`
- **Global ingress, by message type**: `sum by (msg_type) (rate(distant_signal_movement_relay_events_published_total[5m]))`
- **Per-service processed (trust-consumer)**: `sum(rate(distant_signal_trust_consumer_events_received_total[5m]))` and, matched, `sum(rate(distant_signal_trust_consumer_events_matched_total[5m]))`
- **The three-series overlay the prior document already identified as the
  single most useful framing** (published vs. received vs. matched, one
  panel, `msg_type` collapsed via `sum by` on each): this is a completely
  ordinary multi-query Grafana time-series panel — no special panel type,
  just three PromQL queries as three series on one graph, legended by
  query. A sustained, growing gap between the published and received
  series is the same signal `2026-09-04-movement-relay-design.md`'s own
  gap-detection metric already alerts on, made visually inspectable.
- **Full-coverage-consumer, per line** (only where a per-line breakdown is
  actually wanted — this is the one real per-label cardinality dimension,
  ~100+ lines per `DESIGN.md` §10): `sum by (line_id) (rate(distant_signal_full_coverage_consumer_events_matched_total[5m]))`.
  Unlike the prior document's Postgres design, **no cardinality collapse
  is needed here at all** — this is exactly the kind of ad-hoc,
  arbitrary-dimension query Prometheus's own local TSDB is built to
  absorb cheaply, which was precisely the capability the prior document's
  Decision 1 gave up in exchange for "no new infrastructure." That
  trade-off no longer needs to be made.

No PromQL is invented from memory here without also being checked against
the counters' real names/labels above; every query only uses metric names
and label sets already confirmed to exist in the crates' own source.

### "Statuses of the microservices... a dependency graph... historical uptime"

**Node Graph panel — investigated, not recommended for this.** Confirmed
(Grafana's own documentation, `panels-visualizations/visualizations/node-
graph/`): it's a core built-in panel (no plugin install needed), but it
requires a specific **nodes-and-edges field-shaped dataset** — a nodes
frame with a required `id` field (plus optional `title`/`mainstat`/
`color`/etc.) and a separate edges frame with required `id`/`source`/
`target` fields. Plain PromQL returns time series, not this shape — there
is no built-in Grafana transformation that reshapes an arbitrary Prometheus
query into node/edge frames; the panel's practical native users are data
sources that implement the graph shape themselves (tracing backends like
Tempo, or the X-Ray plugin, called out in Grafana's own docs as "the first
data source supporting this visualization"). Building this from Prometheus
alone would mean hand-rolling a synthetic table (one query per fixed edge,
reshaped via `Organize fields`/static field renames) for a topology that,
per the prior document's own Decision 2a reasoning, is **fixed and known
at design time** (18 nodes, edges never change shape, only per-node status
does) — exactly the situation that reasoning already concluded doesn't
benefit from a general graph-layout mechanism. That reasoning holds
whether the renderer is `react-flow` or Grafana's Node Graph: **a
hand-authored, fixed-position diagram remains the better fit**, not
Grafana's Node Graph panel. Where that diagram should live is addressed
below (Point 3) — the short version is it can't live in Grafana at all if
it needs to be public.

**State timeline panel — the right native fit, confirmed against
Grafana's own docs.** Core built-in, no plugin. Its documented "Supported
data formats" section says table-shaped state data is the *ideal* input,
but explicitly also documents feeding it **time-series data directly**:
*"You can also create a state timeline visualization using time series
data. To do this, add thresholds, which turn the time series into
discrete colored state regions."* This is exactly `up{job="..."}`'s
native shape — a 0/1 gauge Prometheus emits automatically for **every**
scrape target with zero new instrumentation, one label set per target.
Query: `up{job=~"distant-signal-.*"}` (job-naming convention to be fixed
at PodMonitor-authorship time), thresholds at `0`→red/"down",
`1`→green/"up". This directly answers "historical uptime" per the
Prometheus TSDB's own retention (a `kube-prometheus-stack`-default
Prometheus persists this out of the box — no new storage design needed at
all, unlike the prior document's from-scratch `service_heartbeats`/
`service_status_history` schema).

**Does `up{}` alone suffice, given the app's own stricter health
semantics?** The task's own framing is right to flag this: `up{}` only
proves a target's `/metrics` HTTP listener answered a scrape — it says
nothing about `movement-relay`'s Kafka-rebalance-confirmed `ReadyState`
or `trust-consumer`/`full-coverage-consumer`'s "at least one batch
processed" `ConnectionState` (`crates/movement-relay/src/health.rs:18-22`,
`crates/trust-consumer/src/health.rs:18-20`,
`crates/full-coverage-consumer/src/health.rs:23-26`). A process that's
up but still mid-rebalance, or has never yet successfully polled a batch,
reads as `up{}==1` while its own `/healthz` would still read unready.
**Call: worth adding, small, not blocking a v1.** Each of these three
crates already computes this boolean in-process (an `AtomicBool` read on
every `/healthz` request) — exposing it as one more
`metrics::gauge!(metric_name("<service>_ready"), 0.0 or 1.0)` call,
updated wherever the boolean already flips, is a few lines per crate, no
new endpoint, no new port, reusing the metrics-port/PodMonitor wiring this
document already fixes. This is real, still-needed **application-code**
instrumentation work — not infrastructure, not Grafana config — flagged
explicitly as the answer to "does anything still need building in the
app" (see Point 3 below). A first `/status` iteration can reasonably ship
on `up{}` alone and add these three gauges shortly after; it is not a
hard blocker.

### "When data was last received"

**Call: `/public/freshness` remains the better answer for this specific
piece — not a Grafana panel.** Re-verified fresh: `crates/api/src/routes/
freshness.rs` still exists exactly as the prior document described
(Correction 2 there): unauthenticated `GET /public/freshness`, aggregating
`stations`/`tocs`/`incidents`/`tfl`/`schedule_feed` via `tokio::try_join!`
over the existing `last_*_fetch` queries, already wired end-to-end into
the frontend (`frontend/lib/api.ts`'s `getDataFreshness`,
`frontend/app/layout.tsx`, `frontend/components/DataFreshnessInfo.tsx`).
Nothing about the new Grafana deployment changes this reasoning — a
Grafana panel could technically show `time() - <a timestamp gauge>` for
the same data, but doing so would mean (a) inventing new gauge metrics for
data that already has a perfectly good source-of-truth column in Postgres,
(b) putting a genuinely public-interest fact ("is the live-departures data
this app shows me right now fresh") behind a tailnet-only Grafana instance
real users can never reach, and (c) duplicating, not reusing, the prior
document's own already-designed, still-unimplemented extension (adding the
three currently-omitted fields — `station_samples`,
`station_full_coverage_samples`, `full_coverage_stats` — to
`DataFreshness`). That extension (prior document's Decision 3) is
**unchanged and still recommended** by this document; it has nothing to
do with the monitoring-stack question and doesn't need re-deriving.

## Point 3, answered directly: does anything still need building in the app?

**Mostly no — but not entirely, and the one real exception is a direct
consequence of Grafana being tailnet-only, which is a hard constraint
this document is told to accept, not question.**

- **The operator-facing half of the original ask — trust-event throughput
  graphs, a dependency-status overview, historical uptime — is now fully
  served by Grafana**, once the scrape gap above is closed. Zero new
  `distant-signal` application code. The only `charts/distant-signal`
  change is the `PodMonitor` selector fix (Finding 1) — a Helm template
  edit, not new instrumentation, and one this document classifies as
  belonging to `charts/distant-signal` for the same reason the chart
  already owns `podmonitor.yaml` itself (Point 5, below). Everything else
  needed to make Grafana show it lives in `Ranma-Config`.
- **The one genuine gap the app itself should still close: the
  three `_ready` gauges** (previous section) — small, optional for a v1,
  but real application-code work, because `up{}` alone doesn't carry this
  app's own stricter readiness semantics and nothing else will ever
  compute it if these three crates don't.
- **The one thing that structurally cannot live in Grafana at all: a
  public-facing view.** `ranma-grafana` is tailnet-only by explicit,
  stated, non-negotiable design — "note the instance is local only and
  not accessible over the internet." The prior document's Decision 6
  treated "public, unauthenticated" as a deliberate, load-bearing product
  choice for this whole feature (matching every other comparable-
  sensitivity endpoint this app already exposes publicly:
  `/public/health`, `/public/freshness`, line-status endpoints,
  `/public/stations/{crs}/departures`), not an incidental detail. **This
  document does not resolve whether that product intent still holds** —
  it's a real fork this respec surfaces rather than silently picks a side
  on:
  - **If an operator-only (tailnet) view is acceptable**: this feature is
    done once the scrape gap (above) and a dashboard (below) exist.
    Nothing further to design or build.
  - **If a genuinely public status page is still wanted**: the app needs
    *something* of its own again, but it does **not** need the prior
    document's heavy Option B (new Postgres tables, a new
    `POST /private/service-metrics` write path, a new `movement-relay`→
    `api` dependency). Prometheus and Alertmanager are ClusterIP-only —
    not internet-reachable — but they *are* reachable from any other pod
    in the same cluster, `distant-signal`'s own `api` pod included, over
    the in-cluster Service DNS name (by Helm's own standard fullname
    convention, since the release name `kube-prometheus-stack` already
    contains the chart name, this resolves to
    `kube-prometheus-stack-prometheus.monitoring.svc.cluster.local:9090`
    — confirmed by rendering the chart's own `templates/prometheus/
    service.yaml`; not yet confirmed against the live cluster, flagged as
    an open question below). A much lighter design than Decision 1/2
    becomes available: `api` makes a small number of server-side PromQL
    calls against that in-cluster URL — `up{job=...}` for current
    per-service status, `avg_over_time(up{...}[$window])` for the exact
    same historical-uptime percentage Grafana's own State Timeline would
    show, `sum(increase(distant_signal_..._total[$window]))` for the
    public-facing throughput numbers — and serves the results through a
    new, small, public read endpoint. Prometheus becomes the read-store;
    `distant-signal` builds no storage of its own. This is real, new
    scope this document does **not** design in full (route shape, exact
    query set, caching posture, what a `/status` page renders from it) —
    it's flagged here as the *shape* the remaining work would take if the
    public-page intent survives, deliberately not designed to completion,
    because whether it's needed at all is the open product question, not
    an engineering one.

**Be decisive, as asked**: if forced to pick without a human answering the
public-vs-operator-only question, the honest recommendation is Grafana-
only for v1 (ship the scrape-gap fix + one dashboard, get real signal
fast, cheap, using infrastructure that already exists) and treat the
public-page extension above as a fast-follow only if the product intent
genuinely requires public reachability — not something to build
speculatively now against an unconfirmed requirement.

## Point 4: dashboard provisioning-as-code

**The sidecar mechanism is already enabled — confirmed from the chart's
own defaults, not assumed.** `kube-prometheus-stack`'s top-level
`values.yaml` sets, for its bundled `grafana` subchart:
`sidecar.dashboards.enabled: true`, `label: grafana_dashboard`,
`labelValue: "1"`, `searchNamespace: ALL` (all four confirmed by reading
the chart's real `values.yaml` at the pinned `89.2.2` version — the
`grafana` subchart's *own* standalone default for this is actually
`false`; `kube-prometheus-stack` overrides it on its behalf). Neither
`Ranma-Config` file touches `grafana.sidecar.dashboards.*` at all, so this
default stands, unmodified, on the live cluster. Concretely: any
`ConfigMap`, in **any namespace** (per `searchNamespace: ALL`), carrying
the label `grafana_dashboard: "1"` and a data key holding dashboard JSON,
is picked up automatically — no further Helm value needs setting for the
discovery mechanism itself.

**Where the `ConfigMap` should live**: `Ranma-Config`, not
`charts/distant-signal` — applying the same boundary reasoning as Finding
4's label-coupling decision, and consistent with `charts/distant-signal`
having zero awareness that Grafana (or any specific dashboard-labeling
convention) exists at all today. A dashboard JSON blob is a much tighter,
more specific coupling to `kube-prometheus-stack`'s own sidecar convention
than the generic, vendor-agnostic `PodMonitor` CRD the chart already
carries — putting it in `Ranma-Config` (where `monitoring.yaml` itself
already lives) keeps that coupling in the one place that's already allowed
to know Grafana exists, rather than teaching the portable app chart a new,
Grafana-specific label convention it has no other reason to know.

## Point 5: repo-boundary summary

| Change | Repo | Why |
|---|---|---|
| `PodMonitor` selector: add `movement-relay`/`trust-consumer`/`full-coverage-consumer` | `charts/distant-signal` | Same file/mechanism that already exists there; a generic, vendor-agnostic CRD selector fix, no cluster-specific fact involved. |
| Optional: `distant_signal_<service>_ready` gauges (movement-relay/trust-consumer/full-coverage-consumer) | `charts/distant-signal`'s crates | Application instrumentation, same shape as every existing `metrics::gauge!` call. |
| `metrics.podMonitor.enabled: true` | `Ranma-Config` (`distant-signal-config` values overlay) | A deployment-time value flip for one specific cluster; the chart's own default (`false`) stays conservative for anyone installing this chart without Prometheus Operator's CRDs. |
| `podMonitorSelectorNilUsesHelmValues: false` (+ `serviceMonitorSelectorNilUsesHelmValues: false`) | `Ranma-Config` (`monitoring-config` ConfigMap) | A `kube-prometheus-stack`-specific, cluster-wide discovery policy — belongs wherever that HelmRelease's own values already live. |
| `schedulefeed` `NetworkPolicy`: add a `monitoring`-namespace-scoped allow on `9091` | `Ranma-Config` (`clusters/mine-bringer/apps/distant-signal.yaml`, the existing standalone resource) | Already a standalone, cluster-specific resource outside the chart's own `networkPolicy.enabled` gate, per its own comment. |
| Dashboard `ConfigMap`(s) | `Ranma-Config` | Couples to `kube-prometheus-stack`'s sidecar label convention specifically — a fact only the cluster-config repo should need to know. |
| Any future public-page endpoint (contingent, Point 3) | `charts/distant-signal` / `crates/api` | Only if the public-page product question resolves "yes" — application-layer read logic, same as every other `/public/*` route. |

This mirrors the prior document's own Decision 1 boundary reasoning
exactly: things that are true of the app regardless of which cluster it's
deployed to belong in the chart; things that are true of *this* cluster's
specific topology belong in `Ranma-Config`.

## Non-goals

- **Building the actual dashboard JSON.** Named the real PromQL queries a
  dashboard needs; did not author panel JSON or a `ConfigMap` manifest —
  implementation-time work, per this task's own scope.
- **Resolving the public-vs-operator-only product question (Point 3).**
  Surfaced as the one real open fork; not decided here, because it's a
  product-intent question this document's factual grounding can't answer
  on its own — flagged for explicit human sign-off.
- **Alerting / `PrometheusRule`s.** `kube-prometheus-stack` bundles
  Alertmanager, which makes this cheaper than it would have been under
  the prior document's design, but this document's scope is the status
  *page*, matching the prior document's own identical Non-goal.
- **Instrumenting `notifier`.** Real, pre-existing gap (Finding 1's
  correction), out of scope for this pass.
- **Anything about `railmcp`'s observability.** Same Non-goal the prior
  document already stated, unaffected by this respec.
- **Confirming the in-cluster Prometheus Service DNS name against the
  live cluster.** Derived from the chart's own rendered template and
  Helm's standard fullname convention (Point 3's contingent design);
  flagged as needing a `kubectl get svc -n monitoring` confirmation before
  any implementation depending on it.
- **An implementation plan.** Separate, later step, per this repo's
  process — same posture the prior document already took.

## Open questions / risks

1. **The public-vs-operator-only product question (Point 3)** is the
   single biggest open item — it determines whether this feature is
   "done" after the scrape-gap fix and one dashboard, or needs a genuinely
   new (if now much smaller) piece of `api`-side read logic.
2. **The in-cluster Prometheus Service hostname**
   (`kube-prometheus-stack-prometheus.monitoring.svc.cluster.local:9090`)
   is inferred from Helm's fullname convention and the chart's own
   rendered template, not confirmed against the live `mine-bringer`
   cluster — a five-second `kubectl` check before anything depends on it.
3. **Whether `podMonitorSelectorNilUsesHelmValues: false` should also be
   set for `probeSelector`/`ruleSelector`/`scrapeConfigSelector`** for
   consistency, even though nothing in this document's scope needs them
   yet — a cheap, get-it-right-once decision worth making at
   implementation time rather than revisiting piecemeal later.
4. **Whether the three `_ready` gauges (movement-relay/trust-consumer/
   full-coverage-consumer) ship in the same change as the `PodMonitor`
   selector fix, or as a fast-follow** — this document recommends
   fast-follow is fine, but a reviewer may reasonably want them bundled
   given how small the addition is once the port/PodMonitor plumbing
   exists anyway.
5. **Job-label naming convention for `up{job="..."}` queries** (what
   `job` label value each `PodMonitor` endpoint actually gets — Prometheus
   Operator auto-derives this from the `PodMonitor`'s own name/namespace
   by default unless overridden) is left to implementation time; this
   document names the query *shape*, not the exact label value, consistent
   with its own stated scope.
6. **Whether `serviceMonitorSelectorNilUsesHelmValues: false` (changed
   alongside the `podMonitor` one, Finding 4) has any unintended effect
   on `kube-prometheus-stack`'s own bundled `ServiceMonitor`s** — those
   already carry the `release:` label kube-prometheus-stack itself
   applies, so setting the selector to "match everything" is strictly
   additive, not exclusionary; flagged for a reviewer's sanity check
   rather than treated as a live risk.
