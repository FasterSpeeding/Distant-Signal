# Plan: Status/Observability via Grafana — Scrape Gap, Dashboard, Operator-Only

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This plan has two parts, deliberately not the same kind of thing**,
> mirroring `docs/superpowers/plans/2026-09-04-movement-relay-plan.md`'s own
> Deploy-A/Deploy-B split, scaled down to this feature's much lower risk.
> **Part 1 (Tasks 1–5) is normal, buildable, testable, mergeable
> implementation work in *this* repo** — plan and execute it exactly like
> any other task list here, checkbox by checkbox, `helm template`/`cargo
> test` verification at the end of each. **Part 2 (the four `Ranma-Config`
> edits below) is NOT a task list for an implementation agent to "complete"
> by pushing to that repository.** `Ranma-Config` is a separate GitOps repo
> this plan does not have write access to test against a real cluster — its
> edits are specified here exactly (full YAML, exact file, exact insertion
> point) for a human or orchestrator to apply and reconcile via Flux,
> watched, one step at a time, in the order given. **If you are an agent
> executing this plan: implement and merge Tasks 1–5 (Part 1), then stop
> and hand Part 2 to a human with this document.** Unlike the movement-relay
> plan's Deploy B, none of these four edits touches an irreplaceable
> external credential or a one-shot resource (Prometheus's `PodMonitor`
> discovery is idempotent, additive, and freely retryable) — so Part 2 is a
> precise specification the human applies via their normal `Ranma-Config`
> workflow, not a hazard-laden runbook with rollback choreography. It still
> isn't an implementation agent's job to run, because this plan cannot
> verify it against the real `mine-bringer` cluster.

**Goal:** implement
`docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md`
("the respec") end to end, for the **operator-only** scope the repo owner
has since confirmed (see "What changed since the respec" below) — no public
status page, no `api`-side PromQL endpoint, no new public route. The
respec's own "Open questions / risks" items 3–5 are resolved concretely
below, not left dangling a second time; see "Judgment calls this plan
makes."

**What changed since the respec, resolving its one open product question**:
the respec's Point 3 framed "public vs. operator-only" as the single
biggest open item, and recommended "Grafana-only for v1" as the decisive
default absent a human answer. The repo owner has now given that answer
explicitly: **operator-only, for now.** This plan implements exactly that —
the respec's Point 3 "public-page fast-follow" sketch (a new `api`-side
PromQL read endpoint) is **out of scope for this plan in full**, not merely
deferred within it. If that product decision changes later, it is new
design work against this plan's finished state, not a task this plan leaves
half-started.

**Architecture:** one Helm template edit
(`charts/distant-signal/templates/podmonitor.yaml`, Task 1) closes the real
scrape gap (respec Finding 1) and adds a `podTargetLabels` entry that makes
per-component dashboard queries possible at all (this plan's own Point 2
resolution, below — the respec left this as an open question, this plan
verifies and closes it). One `values.yaml`/`values-example.yaml` pair (Task
2) gives this now-genuinely-useful toggle real CI coverage in both states
for the first time. Three small, independent Rust changes (Tasks 3–5, one
per crate, explicitly a fast-follow — see Judgment Call 1) add a
`_ready` Prometheus gauge to `movement-relay`/`trust-consumer`/
`full-coverage-consumer`, closing the "`up{}` alone doesn't prove
application-level readiness" gap the respec named but didn't build. Four
`Ranma-Config` edits (Part 2) then: (a) make Prometheus discover
`distant-signal`'s `PodMonitor` at all (respec Finding 4), consistently
across every comparable Prometheus Operator selector (this plan's Point 3
resolution); (b) flip the chart's own conservative-by-default
`podMonitor.enabled` toggle for the one real cluster that has Prometheus
Operator installed; (c) close a real `NetworkPolicy` gap blocking
`schedulefeed`'s metrics port specifically (respec Finding 5); (d) ship a
dashboard `ConfigMap` Grafana's sidecar auto-discovers (respec Point 4),
containing a concrete, complete panel-by-panel dashboard (this plan's own
design, below — the respec named example queries, did not build panels).

**Design docs:**
`docs/superpowers/specs/2026-09-05-status-observability-page-design.md`
(topology/health-signal research, still valid where not superseded) and
`docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md`
(the respec this plan implements; its Decisions/Findings are authoritative
for reasoning this plan does not repeat).

---

## Judgment calls this plan makes (read before Task 1)

The respec's own "Open questions / risks" section (items 3–5) and this
task's own brief named four things as genuine judgment calls for "the
implementation plan" or fresh verification, not decided/confirmed there.
Resolved here, plainly, each verified against real source (Prometheus
Operator's own code, a real `helm template` render of the pinned chart
version, and a fresh `Ranma-Config` clone) rather than re-guessed:

1. **Do the three `_ready` gauges ship in the same change as the
   `PodMonitor` selector fix, or as a fast-follow? Fast-follow — Tasks 3–5,
   clearly labeled, still part of this plan, not dropped.** Reasoning: the
   `PodMonitor` selector fix (Task 1) is a complete, independently valuable,
   low-risk Helm-only change that unblocks the entire `Ranma-Config`
   rollout (Part 2) on its own — nothing about it depends on any Rust code
   change. The three gauges are real application-code work (a new call site
   inside `ConsumerContext::post_rebalance` for `movement-relay`, and a new
   shared `set_connected` helper replacing three raw `.store()` call sites
   apiece for `trust-consumer`/`full-coverage-consumer` — see Tasks 3–5),
   non-trivial enough to deserve its own review pass rather than being
   folded into a Helm-template PR a reviewer would otherwise skim quickly.
   The respec's own reasoning already supports this: "`up{}` alone... is
   not blocking a v1" — a first dashboard iteration is fully useful with
   `up{}` alone (Panel 5/6 below), and Panel 7 (readiness) is designed to
   degrade gracefully to "No data" until Tasks 3–5 land, not to be blocked
   on them.
2. **Job-label naming convention for `up{job="..."}` queries — verified
   against Prometheus Operator's own source, not assumed.** Cloned
   `prometheus-operator/prometheus-operator` fresh and read
   `pkg/prometheus/promcfg.go`'s `generatePodMonitorConfig`
   (`:1649-1662`): absent an explicit `spec.jobLabel` on the `PodMonitor`
   (`charts/distant-signal/templates/podmonitor.yaml` sets none today), the
   `job` label is a **static relabel replacement**, not derived per-pod:
   `{{ target_label: job, replacement: fmt.Sprintf("%s/%s", namespace,
   podmonitor-name) }}`. Confirmed concretely: **every pod this one
   `PodMonitor` object matches — `api`, `aggregator`, `enricher`, every
   poller, `schedulefeed`, and (after Task 1) `movement-relay`/
   `trust-consumer`/`full-coverage-consumer` — gets the exact same `job`
   label value** (`distant-signal/distant-signal`, from `<namespace>/<name>`
   given this chart's fullname). **`job` alone cannot distinguish
   components. The task's own suspicion is correct, and this plan closes
   it concretely**: the same source (`promcfg.go:1636-1645`) shows
   `namespace`/`container`/`pod` ARE promoted onto every scraped series
   automatically (no chart change needed for those three), but
   `app.kubernetes.io/component` is **not** promoted unless the
   `PodMonitor`'s own `spec.podTargetLabels` lists it (confirmed: this
   relabeling loop only fires `for _, l := range append(m.Spec.PodTargetLabels,
   cpf.PodTargetLabels...)` — neither is set anywhere in this chart or in
   `Ranma-Config` today). **Task 1 therefore adds
   `podTargetLabels: [app.kubernetes.io/component]` to `podmonitor.yaml`**,
   promoting the exact same component taxonomy this chart already uses
   everywhere else (Helm labels, `values.yaml` keys, the selector itself)
   into a real Prometheus label, `app_kubernetes_io_component` (Prometheus's
   own `/` and `.` → `_` sanitization of label names, confirmed from the
   same `sanitizeLabelName` calls throughout `promcfg.go`). Every dashboard
   query below (Point 5's design) uses `app_kubernetes_io_component`, never
   bare `job`. (One real, incidentally-discovered wrinkle, **not** part of
   this plan's scope, flagged for the record: `schedulefeed`'s pod has
   *three* containers — `sftp`, `ingest`, `reference`
   [`schedulefeed-deployment.yaml:78,186,255`] — sharing one
   `app_kubernetes_io_component=schedulefeed` value; `ingest`'s metrics port
   is named `metrics` and IS matched by the `PodMonitor`'s existing
   `metrics` endpoint, but `reference`'s own metrics port is named
   `ref-metrics` and matches **neither** of the `PodMonitor`'s two
   `podMetricsEndpoints` entries [`http`, `metrics`] — so `schedule-reference`
   is not scraped at all today, silently absent from `up{}` rather than
   reporting `0`. Pre-existing, unrelated to the three components this plan
   adds, and this plan's own Global Constraints forbid touching existing
   component coverage — noted here so a future reader doesn't mistake
   `schedule-reference`'s absence from the dashboard for a bug in this
   plan.)
3. **`probeSelector`/`ruleSelector`/`scrapeConfigSelector`
   `NilUsesHelmValues` — yes, all five, for consistency, verified against
   the real pinned chart's own `values.yaml`.** Rendered
   `helm show values prometheus-community/kube-prometheus-stack --version
   89.2.2` fresh: `podMonitorSelectorNilUsesHelmValues`,
   `serviceMonitorSelectorNilUsesHelmValues`,
   `probeSelectorNilUsesHelmValues`, `ruleSelectorNilUsesHelmValues`, and
   `scrapeConfigSelectorNilUsesHelmValues` are five independent, identically-
   shaped keys under `prometheus.prometheusSpec`, every one defaulting
   `true` (confirmed at `values.yaml:4495,4520,4543,4565,4590` of the
   rendered chart values). **Decision: set all five to `false`** (Part 2,
   `Ranma-Config` Edit R1) — the same reasoning Finding 4/Open Question 6
   already established for `podMonitorSelector`/`serviceMonitorSelector`
   (strictly additive: every object `kube-prometheus-stack`'s own bundled
   subcomponents ship already carries the `release:` label, so widening
   discovery to "everything, unfiltered" cannot un-select anything already
   selected) applies identically to `probeSelector`/`ruleSelector`/
   `scrapeConfigSelector` — none of which this plan's scope uses today, but
   getting cluster-wide CRD discovery right once, uniformly, avoids a
   future app in this cluster hitting the exact same silent-zero-targets
   trap Finding 4 diagnosed for `PodMonitor`, one selector at a time.
4. **Where does R1 (the `NilUsesHelmValues` flip) actually belong in
   `Ranma-Config`, precisely — re-derived from a fresh clone, refining the
   respec's own "wherever that HelmRelease's own values already live."**
   The respec's Point 5 table said "monitoring-config ConfigMap." A fresh
   clone shows two real candidate files:
   `clusters/base/apps/monitoring.yaml` (the `HelmRelease`'s own inline
   `spec.values`, applies to any cluster including this base file) and
   `clusters/mine-bringer/apps/monitoring.yaml` (the `monitoring-config`
   `ConfigMap`, `mine-bringer`-specific, wired via `valuesFrom`). Read both
   in full: `clusters/base/apps/monitoring.yaml` already carries exactly
   this shape of decision inline — `kubeControllerManager.enabled: false`,
   `kubeScheduler.enabled: false`, `kubeEtcd.enabled: false`,
   `grafana.enabled: true` — structural, non-secret, cluster-topology-
   agnostic chart policy, the identical category `podMonitorSelectorNilUsesHelmValues`
   falls into (it says nothing about a hostname, credential, or any
   `mine-bringer`-specific fact). **Decision: R1 goes in
   `clusters/base/apps/monitoring.yaml`'s inline `spec.values`, not the
   `mine-bringer` overlay's `monitoring-config` ConfigMap** — consistent
   with that file's own existing convention, and it automatically covers
   `production` too if that cluster overlay ever includes this same base
   file, without a second copy-paste. (One confirmed load-bearing fact from
   the same read, relevant to ordering only, not to this decision: that
   file's own comment states Flux's real merge order is `valuesFrom` THEN
   inline `spec.values` **last** — inline always wins on a key collision.
   R1 introduces no colliding key, so this doesn't change anything about
   *this* edit, but is worth knowing before anyone edits `monitoring.yaml`
   near other keys later.)

**One more fresh-verification correction, not a judgment call but load-
bearing for Part 2's own verification steps**: the respec's Ground Truth
section states Grafana's tailnet hostname is `ranma-grafana`. A fresh clone
(today) shows this is now stale — `clusters/mine-bringer/apps/monitoring.yaml`'s
`monitoring-config` ConfigMap carries a comment dated **2026-09-05** (i.e.
apparently changed the same day as or after the respec was written):
*"confirmed live 2026-09-05 that the bare label doesn't work for TLS
here... Full tailnet FQDN, not the bare MagicDNS label headlamp.yaml uses
(ranma-headlamp)"* — the real, current hostname is
**`https://grafana-bringer.fox-prometheus.ts.net`**. Every verification
step below that names "open Grafana" uses this corrected hostname, not
`ranma-grafana`.

**Also confirmed fresh, changing the respec's own stated caveat on Decision
4's "global ingress" framing**: the respec's re-derivation of "graphs for
trust events... globally" repeats the prior document's caveat that this
figure "is only fully accurate once every consumer is off the
`movementFeed: kafka` transitional mode." **That migration is now
complete** — `clusters/mine-bringer/apps/distant-signal.yaml`'s
`distant-signal-config` `ConfigMap` shows `trustConsumer.movementFeed:
redis-stream` and `fullCoverageConsumer.movementFeed: redis-stream`, both
at `replicaCount: 1`, with a comment confirming "movement-relay... is
confirmed the sole real Kafka client" and "partition assignment confirmed
and real events flowing." Panel 3 (below) reads correctly as a true global
figure from day one, not caveated.

---

## Non-goals

- **Any public-facing status page, `api`-side PromQL read endpoint, or new
  public route.** Explicitly out of scope per "What changed since the
  respec," above — not the respec's Point 3 fast-follow sketch, not any
  smaller version of it.
- **A dependency graph / topology diagram panel.** The respec's own Point
  investigated Grafana's Node Graph panel and found it a poor fit for this
  app's fixed, known-at-design-time topology; this plan's own panel list
  (Point 5, below) — sourced directly from the task's own enumerated
  requirements — does not include one. If a topology diagram is wanted
  later, it needs a genuinely different design pass (the original,
  first spec's Decision 2a fixed-position-diagram idea, which structurally
  cannot live inside a Grafana panel type), not a task this plan defers
  within itself.
- **Alerting / `PrometheusRule`s.** Same Non-goal the respec and the prior
  document both already stated.
- **Instrumenting `notifier`.** Confirmed zero metrics instrumentation
  exists (respec Finding 1's own correction); out of scope here, same as
  the respec.
- **`railmcp`'s observability.** Unaffected, same as both prior documents.
- **Fixing `schedule-reference`'s un-scraped `ref-metrics` port.** Real,
  freshly-confirmed (Judgment Call 2's footnote), pre-existing, and outside
  this plan's fixed scope of "add exactly three components, touch nothing
  else already selected."
- **Confirming the in-cluster Prometheus Service DNS name against a live
  `kubectl` session** — done differently and more concretely than the
  respec's own flagged-open item: `helm template kube-prometheus-stack
  prometheus-community/kube-prometheus-stack --version 89.2.2 --namespace
  monitoring` was rendered fresh in this planning pass and confirms the
  real Service is named `kube-prometheus-stack-prometheus`, namespace
  `monitoring`, port `9090` (`http-web`) — i.e.
  `kube-prometheus-stack-prometheus.monitoring.svc.cluster.local:9090` is
  now a **verified**, not merely inferred, fact. (Recorded here since the
  respec listed this as Non-goal/Open-Question material; this plan closes
  it rather than re-deferring it, since it's needed for Part 2's own
  verification steps below and costs nothing to confirm client-side.)
- **`schedule-ingest`/`schedule-reference`'s split identity inside one
  `app_kubernetes_io_component=schedulefeed` value.** Distinguishable only
  via the `container` label (`ingest` vs. `reference`), not
  `app_kubernetes_io_component` — noted in the dashboard design (Point 5)
  where relevant, not solved by a chart change here.
- **An implementation plan for the deferred public-page contingency.**
  If the operator-only decision is ever revisited, that is new scoping
  work against the respec's Point 3 sketch, not a section this plan leaves
  half-drafted.

## Global Constraints

- **No `lines/*.toml` changes anywhere in this plan.**
- **The `PodMonitor` selector fix (Task 1) must not remove or alter any of
  the existing component entries** (`api`, `aggregator`, `enricher`, every
  enabled `poller-<name>`, `schedulefeed`) — only add `movement-relay`
  (behind its existing `.Values.movementRelay.enabled` guard, matching how
  it's guarded everywhere else in this chart), `trust-consumer`, and
  `full-coverage-consumer` (unconditionally, matching how `aggregator`/
  `enricher` are already listed unconditionally — neither has a chart-level
  `enabled` toggle, confirmed via `grep -n "^trustConsumer:\|^fullCoverageConsumer:"
  -A3 charts/distant-signal/values.yaml`, which shows neither carries an
  `enabled:` key).
- **`charts/distant-signal` stays monitoring-stack-agnostic.** No hardcoded
  `release: kube-prometheus-stack` label, no new Helm value that only makes
  sense given `kube-prometheus-stack`'s specific existence, anywhere in this
  repo — per the respec's own Finding 4 reasoning (Option (a), rejected) and
  the chart's own prior design goals
  (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`). The
  `podTargetLabels` addition (Task 1) does not violate this: it promotes a
  label this chart *already* defines and uses everywhere
  (`app.kubernetes.io/component`) into a Prometheus target label using a
  vendor-neutral `PodMonitorSpec` field every Prometheus Operator
  installation supports — it names no monitoring-stack-specific fact.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` after Tasks 3–5. Helm:
  `helm lint`/`helm template` in both `podMonitor.enabled` states (Task 2)
  — this repo's existing convention
  (`.github/workflows/ci.yml`'s `helm-lint` job already runs both
  `helm lint`/`helm template` against default values AND
  `values-example.yaml`; Task 2 makes `values-example.yaml` the vehicle for
  the `enabled: true` case, so this rides the existing CI job with no new
  workflow step).
- **No test coverage is possible for the `Ranma-Config`-side YAML or the
  dashboard JSON in this repo's own CI.** Said explicitly, not glossed
  over: none of Part 2's four edits live in this repository, so nothing in
  `charts/distant-signal` or its CI can validate them. Part 2's own
  "Verify" sub-steps are manual `kubectl`/browser checks a human runs
  against the real cluster after applying each edit — see the Rollout
  Order section.
- **File scope.** Modified (this repo): `charts/distant-signal/templates/podmonitor.yaml`,
  `charts/distant-signal/values.yaml`, `charts/distant-signal/values-example.yaml`,
  `crates/movement-relay/src/health.rs`,
  `crates/trust-consumer/src/{health.rs,feed/kafka.rs,main.rs}`,
  `crates/full-coverage-consumer/src/{health.rs,feed/kafka.rs,main.rs}`. No
  other file in this repo changes. `Ranma-Config` files touched (Part 2,
  human-applied, not part of this repo's own file scope):
  `clusters/base/apps/monitoring.yaml`,
  `clusters/mine-bringer/apps/distant-signal.yaml`.

---

# Part 1 — this-repo tasks (normal review bar)

## Task 1: `PodMonitor` selector — add the three missing components + `podTargetLabels`

**Files:** modify `charts/distant-signal/templates/podmonitor.yaml`.

Independent, first task. Closes respec Finding 1 (the real scrape gap) and
this plan's own Judgment Call 2 (the `job`-label problem) in one template
edit.

- [ ] **Step 1: Add the three components to the selector.** Current
  `matchExpressions` block:

```yaml
      - key: app.kubernetes.io/component
        operator: In
        values:
          - api
          - aggregator
          - enricher
          {{- range $name, $poller := .Values.pollers }}
          {{- if $poller.enabled }}
          - poller-{{ $name }}
          {{- end }}
          {{- end }}
          {{- if .Values.scheduleFeed.enabled }}
          - schedulefeed
          {{- end }}
```

  Change to:

```yaml
      - key: app.kubernetes.io/component
        operator: In
        values:
          - api
          - aggregator
          - enricher
          - trust-consumer
          - full-coverage-consumer
          {{- if .Values.movementRelay.enabled }}
          - movement-relay
          {{- end }}
          {{- range $name, $poller := .Values.pollers }}
          {{- if $poller.enabled }}
          - poller-{{ $name }}
          {{- end }}
          {{- end }}
          {{- if .Values.scheduleFeed.enabled }}
          - schedulefeed
          {{- end }}
```

  `trust-consumer`/`full-coverage-consumer` are listed unconditionally
  (same as `aggregator`/`enricher` immediately above them) because neither
  has a chart-level `enabled` toggle — confirmed via
  `grep -n "^trustConsumer:\|^fullCoverageConsumer:" -A3
  charts/distant-signal/values.yaml`. `movement-relay` is guarded by
  `.Values.movementRelay.enabled` (default `false`,
  `values.yaml:963`) — matching every other conditionally-rendered
  component's own guard in this exact selector, and this chart's
  Deployment for it (`movement-relay-deployment.yaml`, similarly gated).

- [ ] **Step 2: Add `podTargetLabels`**, sibling to `selector:` and
  `podMetricsEndpoints:` under `spec:`:

```yaml
spec:
  podTargetLabels:
    - app.kubernetes.io/component
  selector:
    ...
```

  This promotes the Kubernetes pod label `app.kubernetes.io/component`
  (already set on every pod this chart deploys, via
  `distant-signal.labels`/`distant-signal.selectorLabels`,
  `_helpers.tpl:38-41`) into a real Prometheus target label
  (`app_kubernetes_io_component`, after Prometheus's own `.`/`/` → `_`
  sanitization) on every series this `PodMonitor` scrapes — without this,
  every pod matched by this one `PodMonitor` object shares the exact same
  `job` label (`<namespace>/<podmonitor-name>`, a static value, not
  per-pod — see Judgment Call 2), making per-component dashboard queries
  (Point 5, below) impossible. This is a vendor-neutral `PodMonitorSpec`
  field (`monitoring.coreos.com/v1`, not `kube-prometheus-stack`-specific),
  so it doesn't violate this plan's Global Constraint on staying
  monitoring-stack-agnostic.

- [ ] **Step 3: Update the template's own header comment** to reflect the
  now-complete component list and the reason for `podTargetLabels` (the
  existing comment only explains CRD-gating and the two-endpoint
  port-name split — add a third paragraph covering both this task's
  additions, citing this plan/the respec by path, matching this file's
  existing citation style).

- [ ] **Step 4: Verify**

```bash
helm template distant-signal charts/distant-signal \
  --set movementRelay.enabled=true \
  --set metrics.podMonitor.enabled=true \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 \
  --set trustConsumer.kafka.topic=test-topic \
  --set trustConsumer.kafka.saslMechanism=PLAIN \
  --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=test-model \
  --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=test-client \
  --set api.sso.clientSecret=test-secret \
  --set api.sso.redirectUrl=https://app.example.com/callback \
  --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  -s templates/podmonitor.yaml
```

  Confirm the rendered `PodMonitor`'s `matchExpressions` values list
  contains exactly: `api`, `aggregator`, `enricher`, `trust-consumer`,
  `full-coverage-consumer`, `movement-relay` (present because
  `movementRelay.enabled=true` was passed), and (per `values-example.yaml`,
  not this bare render) whichever pollers/`schedulefeed` that file enables
  — and that `podTargetLabels: [app.kubernetes.io/component]` is present at
  the `spec` level. Re-run with `--set movementRelay.enabled=false` (or
  omitted, the default) and confirm `movement-relay` is **absent** from the
  rendered list — the conditional guard actually works.

- [ ] **Step 5: Commit**

```bash
git add charts/distant-signal/templates/podmonitor.yaml
git commit -m "distant-signal: PodMonitor selector covers movement-relay/trust-consumer/full-coverage-consumer, promotes app.kubernetes.io/component"
```

---

## Task 2: `values.yaml` comment + `values-example.yaml` CI coverage

**Files:** modify `charts/distant-signal/values.yaml`,
`charts/distant-signal/values-example.yaml`.

Depends on Task 1 only in the sense that it's testing Task 1's output;
otherwise independent. Closes the Tests section's own requirement: neither
`helm lint`/`helm template` CI step today ever renders the chart with
`metrics.podMonitor.enabled=true` — this task makes that happen via the
existing `values-example.yaml` CI vehicle, rather than adding a new
workflow step.

- [ ] **Step 1: `values.yaml` comment update.** Current comment at
  `values.yaml:1455-1460`:

```yaml
  podMonitor:
    # Prometheus Operator's PodMonitor CRD -- off by default, mirroring
    # networkPolicy.enabled's identical reasoning: many clusters don't have
    # Prometheus Operator installed, and rendering a CRD that doesn't exist
    # would fail `helm install` outright rather than degrade gracefully.
    enabled: false
```

  Add a sentence after the existing reasoning (don't remove the existing
  reasoning — it's still why the *default* stays `false`): note that as of
  this plan, at least one real cluster (`mine-bringer`, via `Ranma-Config`)
  does run Prometheus Operator and does flip this to `true` in its own
  values overlay — so this is no longer a purely theoretical toggle, and a
  future reader shouldn't assume nothing ever sets it. Cite
  `docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md`
  and this plan by path, matching this repo's existing citation
  convention.

- [ ] **Step 2: `values-example.yaml` addition.** This file has no
  top-level `metrics:` section today (confirmed:
  `grep -n "^[a-zA-Z]" charts/distant-signal/values-example.yaml` lists
  `api`, `aggregator`, `enricher`, `frontend`, `postgresql`,
  `internalOauth`, `pollers`, `ingress`, `networkPolicy` — no `metrics`).
  Add one, anywhere consistent with the file's existing ordering (e.g.
  after `pollers:`, before `ingress:`):

```yaml
metrics:
  podMonitor:
    # Exercises the PodMonitor template's enabled=true render path in CI
    # (helm-lint job's "values-example.yaml" step) -- see
    # docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md
    # Task 2. helm template doesn't validate against a live API server, so
    # this renders and type-checks the object without needing the
    # monitoring.coreos.com CRDs installed anywhere in CI.
    enabled: true
```

  This file's own header already documents it as "A filled-in 'real
  deployment' example" rendered via `helm template ... -f values-example.yaml`
  — adding this is consistent with that file's existing purpose (exercising
  every optional feature toggle), not a new convention.

- [ ] **Step 3: Verify — both toggle states, matching this repo's
  test-every-toggle-state convention.**

```bash
# enabled: true, via values-example.yaml (existing CI step, now also
# covers the PodMonitor's enabled=true render path):
helm lint charts/distant-signal -f charts/distant-signal/values-example.yaml
helm template distant-signal charts/distant-signal -f charts/distant-signal/values-example.yaml \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 \
  --set trustConsumer.kafka.topic=test-topic \
  --set trustConsumer.kafka.saslMechanism=PLAIN \
  --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=test-model \
  --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=test-client \
  --set api.sso.clientSecret=test-secret \
  --set api.sso.redirectUrl=https://app.example.com/callback \
  --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  > /dev/null

# enabled: false (the chart default, unchanged) -- existing "default
# values" CI step already covers this; confirm it still renders nothing
# PodMonitor-shaped:
helm template distant-signal charts/distant-signal \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 \
  --set trustConsumer.kafka.topic=test-topic \
  --set trustConsumer.kafka.saslMechanism=PLAIN \
  --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=test-model \
  --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=test-client \
  --set api.sso.clientSecret=test-secret \
  --set api.sso.redirectUrl=https://app.example.com/callback \
  --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  | grep -c "kind: PodMonitor"   # expect 0
```

  Both existing CI steps (`helm lint`/`helm template`, default values and
  `values-example.yaml`) already run unmodified in
  `.github/workflows/ci.yml`'s `helm-lint` job — no workflow file change
  needed; this task's own diff is exactly what makes that job's existing
  `values-example.yaml` pass exercise the new code path.

- [ ] **Step 4: Commit**

```bash
git add charts/distant-signal/values.yaml charts/distant-signal/values-example.yaml
git commit -m "distant-signal: exercise podMonitor.enabled=true in values-example.yaml's existing CI coverage"
```

---

## Task 3: `movement-relay` — `distant_signal_movement_relay_ready` gauge (fast-follow)

**Files:** modify `crates/movement-relay/src/health.rs`.

Independent of Tasks 1–2 (no chart dependency for this to compile/run —
only for the gauge to actually be *scraped*, which is Part 2's job).
Labeled fast-follow per Judgment Call 1 — safe to merge and ship any time
after Task 1, not gated on it.

- [ ] **Step 1: Add the gauge alongside the existing `AtomicBool` flips**
  in `ConsumerContext::post_rebalance`. Current:

```rust
impl ConsumerContext for RelayContext {
    fn post_rebalance(&self, _base_consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Assign(partitions) if !partitions.elements().is_empty() => {
                self.ready.store(true, Ordering::Relaxed);
                tracing::info!(...);
            }
            Rebalance::Revoke(_) => {
                self.ready.store(false, Ordering::Relaxed);
                tracing::warn!(...);
            }
            Rebalance::Error(err) => {
                self.ready.store(false, Ordering::Relaxed);
                tracing::error!(...);
            }
            _ => {}
        }
    }
}
```

  Add one `metrics::gauge!` call per branch, right next to each existing
  `.store()` call (not factored into a helper — unlike Tasks 4–5, this
  struct has exactly one call site per state, so a helper buys nothing
  here):

```rust
            Rebalance::Assign(partitions) if !partitions.elements().is_empty() => {
                self.ready.store(true, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(1.0);
                tracing::info!(...);
            }
            Rebalance::Revoke(_) => {
                self.ready.store(false, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(0.0);
                tracing::warn!(...);
            }
            Rebalance::Error(err) => {
                self.ready.store(false, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(0.0);
                tracing::error!(...);
            }
```

  Both `metrics` (`Cargo.toml:13`, `metrics = "0.24"`) and `common`
  (already used via `common::metrics::install` in `main.rs`) are already
  crate dependencies — no `Cargo.toml` change needed.

- [ ] **Step 2: Tests — none added, and say so explicitly, matching this
  repo's own convention rather than silently skipping.**
  `RelayContext::post_rebalance` has **zero** existing unit test coverage
  today (confirmed: `grep -rn "post_rebalance\|#\[cfg(test)\]"
  crates/movement-relay/src/health.rs` shows no test module) — constructing
  a real `rdkafka::consumer::Rebalance<'_>` value in a unit test needs a
  live `BaseConsumer`/native client handle this repo has never attempted
  to fake, for good reason (this is exactly the kind of thing this crate's
  own doc comments already treat as verified live, not unit-tested — see
  `RelayContext`'s own module doc on why its readiness semantics are
  deliberately different from the other two crates'). Adding the gauge
  call doesn't change that calculus, and doesn't introduce a new testing
  gap beyond the one that already existed. Verified instead the same way
  every other hand-emitted metric in this codebase without a test-friendly
  recorder is verified: manually, via `curl localhost:<metrics_port>/metrics
  | grep movement_relay_ready` against a real or locally-Dockerized Kafka
  connection, during this task's own manual verification step (below) —
  not invented as a fake automated test.

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-relay --all-features
cargo test -p movement-relay
```

  Manual spot-check (optional, if a local/dev Kafka broker is available —
  not blocking merge, since this repo's own convention already accepts
  "verified live" for this exact struct): run `movement-relay` locally with
  `METRICS_ENABLED=true`, trigger a rebalance (or simply let the consumer
  come up and confirm partition assignment), `curl localhost:9091/metrics
  | grep distant_signal_movement_relay_ready` → expect `1`.

```bash
git add crates/movement-relay/src/health.rs
git commit -m "movement-relay: expose distant_signal_movement_relay_ready gauge alongside existing readiness AtomicBool"
```

---

## Task 4: `trust-consumer` — `distant_signal_trust_consumer_ready` gauge (fast-follow)

**Files:** modify `crates/trust-consumer/src/health.rs`,
`crates/trust-consumer/src/feed/kafka.rs`,
`crates/trust-consumer/src/main.rs`.

Independent of Task 3; same fast-follow labeling. Unlike `movement-relay`,
`ConnectionState` is flipped from **three** call sites today (two in
`feed/kafka.rs`, one in `main.rs`'s `ActiveFeed::next_batch` `RedisStream`
branch) — this task centralizes all three through one helper, per this
repo's own "one place that changes" convention
(`crates/common/src/metrics.rs`'s own module doc cites this exact
reasoning for `metric_name`).

- [ ] **Step 1: Add a `set_connected` helper to `health.rs`**, replacing
  the bare `ConnectionState` type alias's implicit "callers store to it
  directly" contract:

```rust
/// Centralizes every `ConnectionState` transition with a matching
/// Prometheus gauge update, so the AtomicBool and
/// `distant_signal_trust_consumer_ready` never drift out of sync across
/// this crate's three flip sites (`feed/kafka.rs`'s own internal update,
/// and `main.rs`'s `ActiveFeed::next_batch` RedisStream branch) -- one
/// place that changes, not three, matching
/// `crates/common/src/metrics.rs::metric_name`'s own stated reasoning for
/// the identical shape of problem.
pub fn set_connected(state: &ConnectionState, connected: bool) {
    state.store(connected, Ordering::Relaxed);
    metrics::gauge!(common::metrics::metric_name("trust_consumer_ready"))
        .set(if connected { 1.0 } else { 0.0 });
}
```

  (`Ordering` is already imported in this file:
  `use std::sync::atomic::{AtomicBool, Ordering};`.)

- [ ] **Step 2: Replace the two `feed/kafka.rs` call sites.** Current
  (`feed/kafka.rs:76-78` and `:93-95`):

```rust
self.connection_state
    .store(true, std::sync::atomic::Ordering::Relaxed);
```

```rust
self.connection_state
    .store(false, std::sync::atomic::Ordering::Relaxed);
```

  Replace both with:

```rust
crate::health::set_connected(&self.connection_state, true);
```

```rust
crate::health::set_connected(&self.connection_state, false);
```

- [ ] **Step 3: Replace the `main.rs` call site.** Current
  (`main.rs:62-65`, inside `impl MovementFeed for ActiveFeed`):

```rust
ActiveFeed::RedisStream(feed, connection_state) => {
    let result = feed.next_batch().await;
    connection_state.store(result.is_ok(), std::sync::atomic::Ordering::Relaxed);
    result
}
```

  Replace with:

```rust
ActiveFeed::RedisStream(feed, connection_state) => {
    let result = feed.next_batch().await;
    health::set_connected(connection_state, result.is_ok());
    result
}
```

- [ ] **Step 4: Tests.**
  - New unit test in `health.rs`, testable without any metrics recorder
    (unlike Task 3, this one IS testable — it only asserts the `AtomicBool`
    side, the same posture `common::metrics.rs`'s own tests take toward
    `install`'s recorder-installing half):

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::set_connected;

    #[test]
    fn set_connected_updates_the_shared_atomic_state() {
        let state = Arc::new(AtomicBool::new(false));

        set_connected(&state, true);
        assert!(state.load(Ordering::Relaxed));

        set_connected(&state, false);
        assert!(!state.load(Ordering::Relaxed));
        // The distant_signal_trust_consumer_ready gauge update inside
        // set_connected is not independently asserted here -- no recorder
        // is installed in this unit test, matching how
        // crates/movement-relay/src/main.rs's own
        // a_clean_batch_commits_and_publishes test already treats its
        // metrics::counter! call.
    }
}
```

  - Every existing `feed/kafka.rs` and `main.rs` test continues to pass
    unmodified — the call-site substitution is behavior-preserving
    (confirm via `cargo test -p trust-consumer` after the edit, not by
    inspection alone).

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p trust-consumer --all-features
cargo test -p trust-consumer
```

```bash
git add crates/trust-consumer/src/health.rs crates/trust-consumer/src/feed/kafka.rs crates/trust-consumer/src/main.rs
git commit -m "trust-consumer: centralize ConnectionState transitions through set_connected, expose distant_signal_trust_consumer_ready gauge"
```

---

## Task 5: `full-coverage-consumer` — `distant_signal_full_coverage_consumer_ready` gauge (fast-follow)

**Files:** modify `crates/full-coverage-consumer/src/health.rs`,
`crates/full-coverage-consumer/src/feed/kafka.rs`,
`crates/full-coverage-consumer/src/main.rs`.

Identical shape to Task 4, independent of it — this crate's `health.rs`/
`feed/kafka.rs`/`main.rs` are separate files with the same structure
(confirmed: `feed/kafka.rs`'s two `.store()` sites at `:54-55`/`:68-69`,
`main.rs`'s `ActiveFeed::next_batch` `RedisStream` branch at `:80-83`).

- [ ] **Step 1: Add `set_connected` to `health.rs`**, identical to Task 4's
  Step 1 except the metric name:

```rust
pub fn set_connected(state: &ConnectionState, connected: bool) {
    state.store(connected, Ordering::Relaxed);
    metrics::gauge!(common::metrics::metric_name("full_coverage_consumer_ready"))
        .set(if connected { 1.0 } else { 0.0 });
}
```

- [ ] **Step 2: Replace the two `feed/kafka.rs` call sites** — same
  mechanical substitution as Task 4 Step 2, this crate's own file/line
  numbers (`:54-55`, `:68-69`).

- [ ] **Step 3: Replace the `main.rs` call site** — same as Task 4 Step 3,
  this crate's own line (`:80-83`).

- [ ] **Step 4: Tests** — same shape as Task 4 Step 4, `set_connected`
  unit test in this crate's own `health.rs`, existing tests unmodified.

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer
```

```bash
git add crates/full-coverage-consumer/src/health.rs crates/full-coverage-consumer/src/feed/kafka.rs crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: centralize ConnectionState transitions through set_connected, expose distant_signal_full_coverage_consumer_ready gauge"
```

---

## Point 5's own design work: the dashboard, panel by panel

This is this plan's answer to the task's own requirement to design "the
real panel list and PromQL queries," not merely name example queries.
Every query below only uses metric names/labels already confirmed to exist
in the crates' own source (respec's "Re-deriving each original ask against
Grafana" section, plus this plan's own fresh reads above) or to be added by
Tasks 1–5 above.

| # | Panel | Type | Query / queries |
|---|---|---|---|
| 1 | Global trust-event ingress rate | Time series | `sum(rate(distant_signal_movement_relay_events_published_total[5m]))` |
| 2 | Ingress by message type | Time series | `sum by (msg_type) (rate(distant_signal_movement_relay_events_published_total[5m]))` |
| 3 | Published vs. received vs. matched | Time series, 3 queries on one panel | `sum(rate(distant_signal_movement_relay_events_published_total[5m]))` legend `published (movement-relay)`; `sum(rate(distant_signal_trust_consumer_events_received_total[5m]))` legend `received (trust-consumer)`; `sum(rate(distant_signal_trust_consumer_events_matched_total[5m]))` legend `matched (trust-consumer)` |
| 4 | `full-coverage-consumer` matches by line | Time series | `topk(15, sum by (line_id) (rate(distant_signal_full_coverage_consumer_events_matched_total[5m])))` (top-15, not all ~100+ lines — see note below) |
| 5 | Per-service historical uptime | State timeline | `up{namespace="distant-signal"}`, legend `{{app_kubernetes_io_component}} ({{container}})` |
| 6 | Current service status | Stat (multi-series) | `up{namespace="distant-signal"}` (instant), same legend as Panel 5, value mapping `0→DOWN` (red) / `1→UP` (green) |
| 7 | Application readiness (movement-relay / trust-consumer / full-coverage-consumer) | Stat (multi-series) | `distant_signal_movement_relay_ready`, `distant_signal_trust_consumer_ready`, `distant_signal_full_coverage_consumer_ready` (instant, one query per legend), value mapping `0→NOT READY` / `1→READY` |

Design notes, resolving what the task asked to be resolved rather than left
implicit:

- **Panel 4 uses `topk(15, ...)`, not the bare `sum by (line_id)` the
  respec named as an example.** The respec's own text already flags
  `line_id` as "~100+ lines" cardinality — fine for Prometheus's own TSDB
  (unlike the original Postgres-storage design, which had to collapse this
  before storage), but a 100+-series time-series panel is unreadable
  without a top-N cutoff. `topk(15, ...)` is a dashboard-legibility choice,
  not a storage-cardinality one — Prometheus still evaluates and can graph
  every line if an operator edits the query ad hoc in Grafana's Explore
  view; this panel's own default just doesn't try to render all of them at
  once.
- **Panel 5 (State timeline) and Panel 6 (Stat) both query `up{}` and both
  work with zero code changes beyond Task 1** — this is the respec's own
  "does anything still need building in the app? Mostly no" finding, made
  concrete: an operator gets a real, working status view from Tasks 1–2
  alone, before Tasks 3–5 (Panel 7) ever land.
- **Panel 7 degrades gracefully to "No data" until Tasks 3–5 merge and
  deploy — shipped in the same dashboard `ConfigMap` from the start, not
  as a second dashboard revision.** Grafana's Stat panel renders "No data"
  for an absent series without erroring; there's no reason to hold the
  whole dashboard JSON back waiting for the fast-follow gauges, or to
  maintain two versions of it.
- **`schedule-reference`'s absence from Panels 5/6** (Judgment Call 2's
  footnote — its `ref-metrics` port isn't matched by either
  `podMetricsEndpoints` entry) is expected, not a bug in this dashboard —
  it simply never appears as a series, the same as any other unscraped
  target. Not fixed here (Non-goals).
- **Legend format `{{app_kubernetes_io_component}} ({{container}})`**
  (Panels 5/6) rather than `{{app_kubernetes_io_component}}` alone: for
  every component except `schedulefeed` these two values are identical
  (redundant but harmless in the legend), but for `schedulefeed` itself
  this is the only way to tell `ingest`'s row apart from a hypothetical
  future second scraped container under the same component value — cheap
  future-proofing, not solving a problem that exists today (only `ingest`
  is actually scraped under that component, per the footnote above).

### The dashboard `ConfigMap` — exact structure

Per respec Point 4 (the sidecar mechanism — `sidecar.dashboards.enabled:
true`, label `grafana_dashboard`, value `"1"`, `searchNamespace: ALL` — is
already live, confirmed from the pinned chart's own `values.yaml` and zero
overrides in either `Ranma-Config` file) and this plan's own Judgment Call
4 reasoning (namespace placement is a pure organizational choice here,
`searchNamespace: ALL` means it has zero effect on discovery): **the
`ConfigMap` lives in the `distant-signal` namespace, in
`clusters/mine-bringer/apps/distant-signal.yaml`** (the same file that
already owns `distant-signal-config` and the `schedulefeed` `NetworkPolicy`)
— not `clusters/mine-bringer/apps/monitoring.yaml`. Reasoning: this
dashboard's content is fundamentally *about* `distant-signal`, not a
`monitoring`-stack-wide concern; colocating it with the app it describes
scales better as a pattern (a future second app's own dashboard belongs in
*that* app's own file, not accumulating inside `monitoring.yaml` forever),
and — confirmed above — `searchNamespace: ALL` makes this purely a matter
of taste, not correctness.

```yaml
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: distant-signal-dashboard
  namespace: distant-signal
  labels:
    reconcile.fluxcd.io/watch: Enabled
    grafana_dashboard: "1"
data:
  distant-signal-status.json: |
    {
      "title": "Distant-Signal: Trust-Event Pipeline & Service Status",
      "uid": "distant-signal-status",
      "tags": ["distant-signal"],
      "timezone": "browser",
      "schemaVersion": 39,
      "version": 1,
      "refresh": "30s",
      "time": { "from": "now-6h", "to": "now" },
      "templating": {
        "list": [
          {
            "name": "datasource",
            "type": "datasource",
            "query": "prometheus",
            "hide": 0,
            "current": {}
          }
        ]
      },
      "panels": [
        {
          "id": 1,
          "title": "Global trust-event ingress rate",
          "type": "timeseries",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 8, "w": 12, "x": 0, "y": 0 },
          "targets": [
            { "expr": "sum(rate(distant_signal_movement_relay_events_published_total[5m]))", "legendFormat": "published/sec" }
          ],
          "fieldConfig": { "defaults": { "unit": "reqps" } }
        },
        {
          "id": 2,
          "title": "Ingress by message type",
          "type": "timeseries",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 8, "w": 12, "x": 12, "y": 0 },
          "targets": [
            { "expr": "sum by (msg_type) (rate(distant_signal_movement_relay_events_published_total[5m]))", "legendFormat": "{{msg_type}}" }
          ],
          "fieldConfig": { "defaults": { "unit": "reqps" } }
        },
        {
          "id": 3,
          "title": "Published vs. received vs. matched (trust-consumer)",
          "type": "timeseries",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 8, "w": 24, "x": 0, "y": 8 },
          "targets": [
            { "expr": "sum(rate(distant_signal_movement_relay_events_published_total[5m]))", "legendFormat": "published (movement-relay)" },
            { "expr": "sum(rate(distant_signal_trust_consumer_events_received_total[5m]))", "legendFormat": "received (trust-consumer)" },
            { "expr": "sum(rate(distant_signal_trust_consumer_events_matched_total[5m]))", "legendFormat": "matched (trust-consumer)" }
          ],
          "fieldConfig": { "defaults": { "unit": "reqps" } }
        },
        {
          "id": 4,
          "title": "full-coverage-consumer matches by line (top 15)",
          "type": "timeseries",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 8, "w": 24, "x": 0, "y": 16 },
          "targets": [
            { "expr": "topk(15, sum by (line_id) (rate(distant_signal_full_coverage_consumer_events_matched_total[5m])))", "legendFormat": "{{line_id}}" }
          ],
          "fieldConfig": { "defaults": { "unit": "reqps" } }
        },
        {
          "id": 5,
          "title": "Per-service historical uptime",
          "type": "state-timeline",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 8, "w": 24, "x": 0, "y": 24 },
          "targets": [
            { "expr": "up{namespace=\"distant-signal\"}", "legendFormat": "{{app_kubernetes_io_component}} ({{container}})" }
          ],
          "fieldConfig": {
            "defaults": {
              "mappings": [
                { "type": "value", "options": { "0": { "text": "down", "color": "red" }, "1": { "text": "up", "color": "green" } } }
              ]
            }
          }
        },
        {
          "id": 6,
          "title": "Current service status",
          "type": "stat",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 6, "w": 24, "x": 0, "y": 32 },
          "targets": [
            { "expr": "up{namespace=\"distant-signal\"}", "legendFormat": "{{app_kubernetes_io_component}} ({{container}})", "instant": true }
          ],
          "options": { "reduceOptions": { "calcs": ["lastNotNull"] }, "orientation": "horizontal", "textMode": "name_and_value" },
          "fieldConfig": {
            "defaults": {
              "mappings": [
                { "type": "value", "options": { "0": { "text": "DOWN" }, "1": { "text": "UP" } } }
              ],
              "thresholds": { "mode": "absolute", "steps": [ { "color": "red", "value": null }, { "color": "green", "value": 1 } ] }
            }
          }
        },
        {
          "id": 7,
          "title": "Application readiness (fast-follow gauges)",
          "type": "stat",
          "datasource": { "type": "prometheus", "uid": "${datasource}" },
          "gridPos": { "h": 6, "w": 24, "x": 0, "y": 38 },
          "targets": [
            { "expr": "distant_signal_movement_relay_ready", "legendFormat": "movement-relay", "instant": true },
            { "expr": "distant_signal_trust_consumer_ready", "legendFormat": "trust-consumer", "instant": true },
            { "expr": "distant_signal_full_coverage_consumer_ready", "legendFormat": "full-coverage-consumer", "instant": true }
          ],
          "options": { "reduceOptions": { "calcs": ["lastNotNull"] }, "orientation": "horizontal", "textMode": "name_and_value" },
          "fieldConfig": {
            "defaults": {
              "mappings": [
                { "type": "value", "options": { "0": { "text": "NOT READY" }, "1": { "text": "READY" } } }
              ],
              "thresholds": { "mode": "absolute", "steps": [ { "color": "red", "value": null }, { "color": "green", "value": 1 } ] }
            }
          }
        }
      ]
    }
```

This is a sketch, not final JSON — the same "sketch, not final code" posture
this repo's design docs already take (this plan's own reasoning above is
authoritative; `gridPos`/`id` numbering/exact `fieldConfig` shape may need
adjustment once actually opened in Grafana's dashboard JSON editor, which
will reformat and fill in fields Grafana's own UI always adds — e.g.
`schemaVersion`, panel `id` uniqueness — on first save). The `${datasource}`
templating variable (not a hardcoded datasource UID) keeps this dashboard
portable across a `kube-prometheus-stack`-provisioned Prometheus datasource
regardless of its auto-generated UID, which varies per install.

---

# Part 2 — `Ranma-Config` edits (human-applied, not an implementation-agent task)

**Do not push any of the following to `Ranma-Config` as part of "completing
this plan."** These are precise specifications for a human (or an
orchestrator with real, verified cluster access) to apply via that repo's
own normal Flux/GitOps workflow, one at a time, in the order given below,
verifying each before moving to the next.

## Edit R1 — cluster-wide Prometheus Operator selector discovery

**File:** `clusters/base/apps/monitoring.yaml` (the `kube-prometheus-stack`
`HelmRelease`'s own inline `spec.values` — see Judgment Call 4 for why this
file, not the `mine-bringer` overlay's `monitoring-config` `ConfigMap`).

Add a new top-level `prometheus:` key inside the existing `spec.values`
block (sibling to the existing `kubeControllerManager`/`grafana` keys):

```yaml
    prometheus:
      # Cluster-wide CRD discovery: without this, kube-prometheus-stack's
      # own chart default (podMonitorSelectorNilUsesHelmValues: true, etc.)
      # requires every PodMonitor/ServiceMonitor/Probe/PrometheusRule/
      # ScrapeConfig in the cluster to carry this HelmRelease's own
      # `release: kube-prometheus-stack` label to be discovered at all --
      # a label distant-signal's own chart deliberately never sets (it
      # must stay monitoring-stack-agnostic, see
      # docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md
      # Finding 4). Setting all five NilUsesHelmValues flags false makes
      # this Prometheus discover every such object cluster-wide,
      # unfiltered by that label -- this chart's own documented, standard
      # answer to running alongside apps it doesn't own (its README's own
      # "Discovery of PodMonitors/ServiceMonitors outside of Helm release"
      # section). Strictly additive for kube-prometheus-stack's own bundled
      # ServiceMonitors/PodMonitors (they already carry the release label,
      # so widening discovery to "everything" cannot un-select them) -- see
      # docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md
      # Judgment Call 3 for why all five (not only pod/service) are set
      # together, for consistency, even though only podMonitorSelector is
      # load-bearing for distant-signal specifically today.
      prometheusSpec:
        podMonitorSelectorNilUsesHelmValues: false
        serviceMonitorSelectorNilUsesHelmValues: false
        probeSelectorNilUsesHelmValues: false
        ruleSelectorNilUsesHelmValues: false
        scrapeConfigSelectorNilUsesHelmValues: false
```

**Verify** (after Flux reconciles the `monitoring` `HelmRelease` — either
wait for its `interval: 1h` or force it, e.g.
`flux reconcile helmrelease kube-prometheus-stack -n monitoring`):

```bash
kubectl port-forward -n monitoring svc/kube-prometheus-stack-prometheus 9090:9090
```

(Service name/port confirmed fresh in this planning pass — see Non-goals'
own note — via `helm template kube-prometheus-stack
prometheus-community/kube-prometheus-stack --version 89.2.2 --namespace
monitoring`, which renders a `Service` named `kube-prometheus-stack-prometheus`
on port `9090`.) Then open `http://localhost:9090/config` in a browser and
confirm the rendered scrape config no longer has a `release:
kube-prometheus-stack` label-match requirement in its `podMonitor`/
`serviceMonitor`/etc. relabeling blocks (or, more directly, once Edit R2
below has also landed, confirm `http://localhost:9090/targets` lists
`distant-signal`'s pods at all — this single check subsumes R1's own
verification, so it's fine to defer this exact check until after R2 if
doing both in one sitting).

## Edit R2 — flip `metrics.podMonitor.enabled` for `distant-signal`

**File:** `clusters/mine-bringer/apps/distant-signal.yaml`, the
`distant-signal-config` `ConfigMap`'s `data.values.yaml` block.

**Precondition**: this repo's own Task 1 (the `PodMonitor` selector fix)
must have merged, and the `distant-signal` chart's own pinned reference in
`clusters/base/apps/distant-signal.yaml`'s `HelmRelease` must have been
bumped to a commit/version that includes it — same "chart pin bump" flow
this repo already uses (confirmed real precedent: that file's own comment
records "chart pin bumped to bfa965e086ab193ce1e2f4a1736a092e9e20f1fe" for
an unrelated prior change). **Not a hard ordering requirement** — see
Rollout Order below for why applying R2 before the pin bump is merely
*incomplete*, not broken — but doing the pin bump first means this edit
delivers full coverage immediately rather than partial-then-complete.

Add `metrics.podMonitor.enabled: true` to the existing values block — e.g.
near the existing `fullCoverageConsumer`/`trustConsumer`/`movementRelay`
keys already there:

```yaml
    metrics:
      podMonitor:
        # Flips charts/distant-signal's own conservative-by-default toggle
        # (values.yaml's own comment: "off by default... many clusters
        # don't have Prometheus Operator installed") now that this cluster
        # genuinely does -- see
        # docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md
        # and docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md.
        enabled: true
```

**Verify**:

```bash
kubectl get podmonitor -n distant-signal
```

expect one object (the chart's `distant-signal.fullname`-named
`PodMonitor`). Then, via the same port-forward as R1's verification:

```bash
kubectl port-forward -n monitoring svc/kube-prometheus-stack-prometheus 9090:9090
```

open `http://localhost:9090/targets`, filter to the `distant-signal`
namespace (or search for `job="distant-signal/distant-signal"` per Judgment
Call 2), and confirm targets exist for `api`, `aggregator`, `enricher`,
every enabled poller, `movement-relay` (if `movementRelay.enabled: true`,
which it already is per the fresh clone), `trust-consumer`,
`full-coverage-consumer`, and `schedulefeed`'s `ingest` container — all
`UP` (green), except `schedulefeed`'s SFTP-blocked case until R3 below
lands (expect that one specific target `DOWN`/scrape-failing until then,
not a sign anything else is broken).

## Edit R3 — `schedulefeed` `NetworkPolicy`: allow Prometheus's scrape

**File:** `clusters/mine-bringer/apps/distant-signal.yaml`, the standalone
`distant-signal-schedulefeed-sftp-ingress` `NetworkPolicy` (currently lines
267–309 of that file).

The current comment (quoted exactly, confirmed from a fresh clone,
immediately above the resource) reads:

> ```
> # networkPolicy.enabled is never set anywhere in this repo (chart default:
> # false), so nothing else currently governs ingress to this pod -- this is
> # the only NetworkPolicy that will ever select it. Confirmed no other
> # in-cluster traffic needs an explicit allow here: metrics.enabled defaults
> # to true upstream (so the ingest container's :9091 listener exists), but
> # this cluster has no Prometheus/monitoring stack (no monitoring.yaml/
> # prometheus manifests anywhere in this repo) to scrape it, so leaving that
> # port out of this policy changes nothing observable today.
> ```

**This is now stale** — `clusters/base/apps/monitoring.yaml` and
`clusters/mine-bringer/apps/monitoring.yaml` are real, merged
`kube-prometheus-stack` manifests (respec's whole premise). Replace that
paragraph with something to the effect of: *"metrics.enabled defaults to
true upstream (so the ingest container's :9091 listener exists); this
cluster now runs a real Prometheus
(clusters/{base,mine-bringer}/apps/monitoring.yaml) that needs to reach it
— see the second ingress rule below, scoped to the monitoring namespace
only, not opened to the whole cluster."* Then add a second `ingress` rule
to the `NetworkPolicy` spec itself:

```yaml
  ingress:
    # No `from:` -- an omitted/empty source list means "any source",
    # matching the "any source IP" exposure this Service is meant to have
    # (external rail-data partners pushing over SFTP from unknown IPs).
    - ports:
        - protocol: TCP
          port: 2022
    # Prometheus (monitoring namespace) needs to reach the ingest
    # container's :9091 metrics port -- scoped to that one namespace, not
    # "any source" like the SFTP rule above, since nothing else in-cluster
    # needs this port. See
    # docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md
    # Finding 5.
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: monitoring
      ports:
        - protocol: TCP
          port: 9091
```

(`kubernetes.io/metadata.name` is the automatically-applied, immutable
namespace label every namespace carries since Kubernetes 1.21+ — confirmed
this cluster runs a version with it available, since every other
namespace-scoped `NetworkPolicy`/`namespaceSelector` convention in a
Kubernetes-1.21+-or-later cluster relies on it; if `monitoring.yaml`'s own
`Namespace` object carries a different explicit label instead, use that one
— check `clusters/base/apps/monitoring.yaml`'s `Namespace` manifest at
apply time, it currently declares no extra labels beyond the default ones
Kubernetes stamps automatically, confirmed by this plan's own fresh read.)

**Verify**: re-check `http://localhost:9090/targets` (same port-forward as
R2) — `schedulefeed`'s `ingest` target should now read `UP`.

## Edit R4 — dashboard `ConfigMap`

**File:** `clusters/mine-bringer/apps/distant-signal.yaml` — new resource,
appended after the existing `distant-signal-schedulefeed-sftp-ingress`
`NetworkPolicy`. Exact content: the `ConfigMap` YAML given in this plan's
own "The dashboard `ConfigMap` — exact structure" section, above (Part 1,
Point 5's design work) — not repeated a second time here.

**Verify**: open `https://grafana-bringer.fox-prometheus.ts.net`
(corrected hostname — see "One more fresh-verification correction," above;
**not** `ranma-grafana`), log in via Authentik SSO, navigate to
Dashboards, confirm "Distant-Signal: Trust-Event Pipeline & Service Status"
appears (the sidecar's auto-discovery, per respec Point 4, needs no manual
"import" step) and that Panels 5/6 show live data for every scraped
component.

---

## Rollout order

Given this spans two repos this plan cannot apply changes to directly, the
order that actually matters, reasoned through rather than assumed:

1. **Merge this repo's Tasks 1–2** (the `PodMonitor` selector fix + its new
   CI coverage). Tasks 3–5 (the `_ready` gauges) can merge before, after, or
   interleaved with this — they have no dependency relationship in either
   direction (Judgment Call 1).
2. **Bump the `distant-signal` chart's pinned reference** in
   `clusters/base/apps/distant-signal.yaml`'s `HelmRelease`, via this
   repo's existing chart-pin-bump convention, to a commit including Tasks
   1–2 (and, ideally, 3–5 if they've landed by then — not required, see
   below).
3. **Apply R1** (`Ranma-Config`, `clusters/base/apps/monitoring.yaml`) —
   no dependency on step 2 in either direction: R1 changes what Prometheus
   itself is willing to discover, independent of whether `distant-signal`'s
   own chart has rendered a matching `PodMonitor` yet. Safe to apply before,
   during, or after step 2.
4. **Apply R2** (`metrics.podMonitor.enabled: true`) — technically safe
   even before step 2 lands (Helm would render the *old* `podmonitor.yaml`,
   missing the three new components — incomplete, not broken; the very
   next reconcile after step 2's pin bump picks up the rest automatically,
   no re-application of R2 needed). **Recommended order: after step 2**,
   so this single edit delivers full coverage in one shot rather than
   partial-then-complete — but if operational reality (e.g. wanting R1/R2
   applied together in one sitting) makes doing it earlier more convenient,
   that's genuinely fine.
5. **Apply R3** (`schedulefeed` `NetworkPolicy` fix) — fully independent of
   every other step; affects only `schedulefeed`'s own scrape target. Any
   time.
6. **Apply R4** (dashboard `ConfigMap`) — technically independent, but
   **apply last**, after confirming targets are `UP` (steps 2–5's own
   verification). Not a correctness requirement (Grafana renders "No data"
   gracefully for a not-yet-scraped target, same as Panel 7 before Tasks
   3–5), but a better first impression for whoever opens the dashboard
   first — no reason to make them see an all-empty dashboard when a few
   minutes' sequencing avoids it.

**What to check after each step**, concretely, given Prometheus/
Alertmanager have no ingress of their own (respec Ground Truth: "ClusterIP-
only, no ingress"): every Prometheus-side check in this plan uses
`kubectl port-forward -n monitoring svc/kube-prometheus-stack-prometheus
9090:9090` (Service name/port verified fresh in this planning pass, not
assumed) then a browser against `localhost:9090/targets` or `/config`.
Grafana itself needs no port-forward — it already has a real tailnet
ingress (`https://grafana-bringer.fox-prometheus.ts.net`, corrected
hostname per above) reachable directly from any tailnet-joined device.

## Open items for a human before/during implementation

1. **Nothing product-level remains open** — the one open product question
   the respec flagged (public vs. operator-only) has been answered by the
   repo owner (operator-only), which is what makes this plan fully
   decisive rather than forked. Flagged here only so a future reader
   doesn't go looking for a fork that no longer exists in this document.
2. **R1's exact `Ranma-Config` file (Judgment Call 4)** is this plan's own
   considered choice (`clusters/base/apps/monitoring.yaml`'s inline
   values, not the `mine-bringer` overlay ConfigMap) based on a fresh
   read — worth a quick sanity check from whoever applies it, since it's a
   refinement of the respec's own less-specific "wherever that HelmRelease's
   own values already live," not a verbatim instruction from either design
   doc.
3. **The `NetworkPolicy` namespace-selector label** (R3) — this plan
   assumes the standard, Kubernetes-1.21+-automatic
   `kubernetes.io/metadata.name: monitoring` label exists on the real
   `monitoring` `Namespace` object; confirmed no override exists in this
   plan's fresh clone, but worth a `kubectl get ns monitoring
   --show-labels` sanity check at apply time, since a wrong label here
   fails silently (the rule simply matches nothing, `schedulefeed` stays
   unscraped) rather than erroring loudly.
4. **The dashboard JSON's exact `fieldConfig`/`gridPos` shape** (Point 5's
   design) may need hand-adjustment once opened in Grafana's own UI for
   the first time — this plan specifies structure and intent precisely,
   but has not (and, per this plan's own Constraints, cannot) round-tripped
   it through a real Grafana instance to confirm every field name is
   exactly what a specific Grafana version expects.
