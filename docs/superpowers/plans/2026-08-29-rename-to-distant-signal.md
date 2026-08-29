# Rename to "Distant Signal" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the project from "nr-status" to **Distant Signal** across
every live, currently-read piece of code, config, and top-level
documentation — while fixing README.md/DESIGN.md's independently-stale
architecture description as part of the same pass — without touching any
dated historical spec/plan doc, any append-only migration comment, or the
superseded pre-Rust Python prototype, and without performing the one-off
human actions (GitHub repo rename, remote URL update) that sit outside a
codebase change.

**Architecture: what changes and what deliberately doesn't.**

*Changes:*
- Two top-level docs (`README.md`, `DESIGN.md`) — name **and** the
  independently-flagged architecture staleness (both currently describe a
  single-package Python demo with no HTTP layer, no persistence, and
  train-tracking explicitly out of scope — none of which is true today).
- One API response field: the `$type` wire-format discriminator
  (`crates/api/src/render.rs`), which embeds `NRStatus` as a PascalCase,
  no-hyphen echo of the project's short name inside a TfL-style pseudo
  .NET type name — plus its six frontend test-fixture echoes.
- Three cosmetic Postgres env-var defaults (`POSTGRES_USER`,
  `POSTGRES_DB`) in the two local-dev env-example files.
- The Helm chart's directory, `Chart.yaml` name, every internal
  `nr-status.*` template-helper name and call site, its own values files'
  image-repository/secret-name examples, and its own `README.md`.
- One Prometheus metric-name prefix, applied from **two** call sites, not
  one (`crates/common/src/metrics.rs`'s `metric_name` helper *and*
  `crates/api/src/main.rs`'s separate, explicit
  `PrometheusMetricLayerBuilder::with_prefix("nr_status")` call — the
  research doc's survey only names the first of these).
- One Kafka consumer-group ID default, duplicated across four files
  (`crates/trust-consumer/src/config.rs`, `docker-compose.yml`,
  `dev.env.example`, `local.env.example`) plus the Helm chart's own copy
  of the same default.

*Deliberately doesn't change:*
- Every crate name under `crates/` (none is `nr-status`-branded today) and
  every `lines/*.toml` internal reference (none exists) — confirmed again
  in this plan's own verification pass, not just inherited from the
  research doc.
- Every dated file under `docs/superpowers/plans/` and
  `docs/superpowers/specs/` — this repo's established convention treats
  these as an append-only historical record of what was true on the date
  each was written; a 2026-07-06 plan saying "nr-status" is accurately
  describing 2026-07-06, not something to correct.
- `crates/api/migrations/20260510023522_initial.sql`'s
  `-- nr-status-v2 database schema` comment — migrations are the same kind
  of append-only history, by this repo's own established convention (see
  that same reasoning already applied in
  `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 2).
- The superseded pre-Rust Python prototype (`src/*.py`, `demo.py`,
  `tests/test_matcher.py`, `plans/01-poller-microservices.md`) — dead code
  from before the Rust rewrite, not part of the live system this rename
  targets. Renaming or removing it is a separate janitorial concern; see
  Global Constraints.
- The GitHub repository name and this project's git remote — a one-off
  human action, not a codebase change; see Task 7.

**Spec:** `docs/superpowers/specs/2026-08-29-project-naming-research.md` —
read in full before starting. That research recommended fixing
README.md/DESIGN.md first and deferring the rename; the project owner has
since decided to rename now, folding the docs fix into the same pass. This
plan does not re-litigate that call — it only re-verifies the research
doc's footprint survey against the current repo (one day newer) and plans
the mechanics.

## This plan's own verification of the research doc's footprint (2026-08-29, base commit `4bee75a`)

The research doc's "~1,203 occurrences of `nr-status`, ~122 of `nr_status`"
was a same-day estimate, not a precise, deduplicated count, and predates
one more day of commits. A fresh, case-insensitive repo-wide grep for
`nr-status`/`nr_status`/`nrstatus` (no `NR_STATUS` hits exist) at this
plan's base commit finds **1,172 matching lines across 65 files** — close
to the research doc's combined estimate, with the small drift expected
from a day of unrelated commits. Splitting those 1,172 lines three ways:

| Bucket | Lines | Treatment |
|---|---|---|
| `docs/superpowers/plans/`, `docs/superpowers/specs/` (15 files) | 870 | Historical record — **not touched** (see Global Constraints). |
| Superseded Python prototype (`plans/01-poller-microservices.md`, `src/render.py`) | 2 | Dead code, not the live system — **not touched** (see Global Constraints). |
| Everything else — the live footprint | **302**, across 25 files | **This plan's actual scope.** |

Those 302 live-footprint lines break down as: `README.md`/`DESIGN.md` (2
title lines, one stale project-layout mention — the real work in Task 1 is
prose accuracy, which isn't line-countable this way), `charts/nr-status/`'s
full 24 files (the bulk of the 302 — directory path repeated per file,
`_helpers.tpl`'s ~20 template-helper definitions, and 193
`{{ include "nr-status.*" }}` call sites across 20 of those files), six
Rust source sites (`crates/api/src/main.rs`, `crates/api/src/render.rs`,
`crates/common/src/metrics.rs`, `crates/enricher/src/main.rs`,
`crates/poller-tfl/src/main.rs`, `crates/trust-consumer/src/config.rs`),
three env/compose files (`dev.env.example`, `local.env.example`,
`docker-compose.yml`), and six frontend Vitest fixture files.

**Two findings beyond the research doc's own survey**, both incorporated
into this plan's tasks below:

1. **`crates/api/src/main.rs:53`** sets
   `PrometheusMetricLayerBuilder::new().with_prefix("nr_status")`
   explicitly for `api`'s own HTTP-layer metrics — a second, independent
   metric-prefix call site the research doc's survey (and, per its own
   text, the `2026-08-29-metrics.md` plan's Global Constraints) describes
   as "keeps whatever prefix `axum-prometheus` applies by default." The
   code does not match that description: `api` explicitly opts into the
   same `nr_status` prefix as every hand-written metric. Both call sites
   must move together (Task 5) or `api`'s HTTP metrics and every other
   service's metrics would end up under two different prefixes after this
   rename, which is strictly worse than today.
2. **`crates/api/src/render.rs:16`** — `to_tfl_shape` emits
   `"$type": "NRStatus.LineStatusReport"` in every line-status API
   response, mirroring TfL's own PascalCase-dotted pseudo-.NET type-name
   convention (e.g. `Tfl.Api.Presentation.Entities...`) with this
   project's own short name standing in for TfL's. This is a live API
   response value, not a doc or comment — worth its own small task
   (Task 2) rather than folding into the README/DESIGN pass.

## Global Constraints

- **The full name is "Distant Signal."** Three machine-readable short
  forms are used, chosen deliberately rather than left to whoever
  implements a given task to improvise:
  - **`distant-signal` (kebab-case)** for anything that is today a
    directory name, a Helm chart/resource-name convention, or a
    hyphen-safe identifier: the Helm chart directory and `Chart.yaml`
    `name:`, every `charts/*/templates/_helpers.tpl` template-helper name
    (`nr-status.fullname` → `distant-signal.fullname`, etc.) and every
    template call site, the `app.kubernetes.io/part-of` label value, the
    Kafka consumer-group ID, and the chart's own example secret
    names/SSO client ID/image-repository paths. This matches the existing
    `nr-status` convention exactly (it was already kebab-case for all of
    these), so no new convention is introduced.
  - **`distant_signal` (snake_case)** for anything that is today a bare
    Postgres identifier or a Prometheus metric-name prefix: `POSTGRES_USER`/
    `POSTGRES_DB` defaults (Postgres identifiers can't contain a hyphen
    without quoting, which is presumably why `nr_status`, not `nr-status`,
    was chosen for these originally) and `common::metrics::metric_name`'s
    prefix plus `api`'s `axum-prometheus` prefix. Kept as full, unabbreviated
    words (`distant_signal_poller_cycle_total`, not `ds_poller_cycle_total`)
    — matching this codebase's own stated reasoning for the *current*
    prefix (`crates/common/src/metrics.rs`'s doc comment: the prefix exists
    so a hand-written metric "can never collide... with a future metric
    from an unrelated process sharing the same Prometheus instance," a
    property a two-letter abbreviation like `ds_` would weaken, not
    strengthen).
  - **`DistantSignal` (PascalCase, no separator)** for exactly one site:
    the `$type` API response discriminator (Task 2), matching the
    PascalCase-no-punctuation convention of the TfL type names it already
    imitates (`Tfl.Api.Presentation.Entities...`), which neither
    kebab-case nor snake_case would resemble.
- **Historical docs are never touched, full stop.** Nothing under
  `docs/superpowers/plans/` or `docs/superpowers/specs/` is in scope for
  any task in this plan, including this plan's own two source documents
  (the naming research doc and this file, once written, describe this
  rename as a point-in-time decision and are not retroactively kept
  "current"). If a task's grep turns up a match under either directory,
  skip it — do not edit it, do not ask whether to.
- **Migration files are the same kind of append-only history.** No task
  touches `crates/api/migrations/20260510023522_initial.sql`'s
  `-- nr-status-v2 database schema` comment, matching this repo's existing
  precedent (`docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s
  Task 2 leaves an equally-dated schema comment alone for the same
  reason).
- **The superseded Python prototype is out of scope, by deliberate
  decision, not oversight.** `src/*.py`, `demo.py`, `tests/test_matcher.py`,
  and `plans/01-poller-microservices.md` predate the Rust rewrite and are
  not part of the live system README.md/DESIGN.md will accurately describe
  after Task 1. Two of these files contain live `nr-status`/`NRStatus`
  text (`plans/01-poller-microservices.md`'s `nr-status-v2` package-name
  mention, `src/render.py`'s `$type` literal — the Python original Task 2's
  Rust port was "ported from," per `render.rs`'s own doc comment) that this
  plan deliberately leaves untouched: renaming dead code most people would
  reasonably just delete is scope creep beyond "rename the live project,"
  not a natural forcing function the way README/DESIGN.md's staleness is.
  If the project owner wants this prototype deleted or renamed, that's a
  separate, smaller follow-up, not a task here.
- **The GitHub repository name and git remote are out of scope for every
  task below** — see Task 7 for why, and what a human still needs to do.
- **No new dependencies, no functional/behavioral changes anywhere in this
  plan.** Every task is a literal-identifier rename (text, a Helm
  chart-internal template-helper namespace, or a directory move) with a
  build/test/render verification step, never new logic.
- **Task order is risk-ascending**, per the project owner's own framing:
  cosmetic text (Tasks 1-3) → the Helm chart, which has a real
  existing-release migration wrinkle but is not itself a breaking wire
  format (Task 4) → the Prometheus metric prefix, the one item the
  research doc flags as a genuine breaking change *if* a live deployment
  with dashboards/alerts already exists (Task 5) → the Kafka
  consumer-group ID, which resets committed offsets for a live TRUST
  consumer if one exists (Task 6) → human follow-ups (Task 7). Tasks 5 and
  6 each include this plan's own reasoning for why no dual-prefix/
  dual-write transition period is used, rather than silently assuming
  either "just change it" or "always add a transition window."

---

### Task 1: `README.md` and `DESIGN.md` — rename and content-accuracy pass

**Files:**
- Modify: `README.md`
- Modify: `DESIGN.md`

**Interfaces:** None — prose only, no code/config consumes these files.

- [ ] **Step 1: Rewrite `README.md`'s name and stale sections**

  Fix, specifically (not a line-by-line rewrite of the whole file — the
  segment/matcher/severity-scale sections below are still accurate and
  stay as they are, aside from any stray `nr-status` text):
  - Title (`# nr-status` → `# Distant Signal`).
  - The "What it does" paragraph: broaden past "a TfL-style line status
    aggregator" to the research doc's own one-line characterization (§1)
    — a personal UK rail companion covering line-status aggregation,
    individual train tracking, accounts, and (soon) ticket/Delay-Repay
    support — while keeping the existing shared-trunk/exclusive-segment
    sentence, which is still this project's real differentiator.
  - "Layout": replace the `src/*.py` file tree (describes code that no
    longer runs) with a short, accurate pointer to the real structure —
    `crates/` (nine-crate Rust workspace: `common`, `api`, `aggregator`,
    `enricher`, `trust-consumer`, and five `poller-*` crates), `frontend/`
    (Next.js), `charts/distant-signal/` (Helm chart, once Task 4 lands —
    sequence this edit after Task 4 or use the post-rename path directly
    since this task can be written last if convenient), `lines/` (unchanged,
    still the curatorial TOML asset). Link to `DESIGN.md` for detail rather
    than duplicating a tree here.
  - "Run the demo": remove or replace — `PYTHONPATH=. python demo.py` no
    longer reflects how to run this system. Point instead at
    `docker-compose.yml` (local dev) and `charts/distant-signal/README.md`
    (Helm) rather than duplicating either's instructions.
  - "What's not included": remove or rewrite — it currently lists an HTTP
    layer, a poller, and a scheduler as absent; all three exist today.
  - Leave "How segments work," "Adding a complex operator," "Severity
    scale," and "Design notes" as they are — still accurate per the
    research doc's own confirmation that the core aggregation logic is
    unchanged.

- [ ] **Step 2: Rewrite `DESIGN.md`'s name and stale sections**

  Fix, specifically (§5 Domain model and §6 Aggregation logic are still
  accurate per the research doc and stay as-is, aside from stray
  `nr-status` text):
  - Title (`# nr-status: Design Document` → `# Distant Signal: Design
    Document`).
  - §2 Scope: move "Train-level live tracking" from "Out of scope for v1"
    to in-scope, noting it's implemented via TRUST/Kafka
    (`crates/trust-consumer`), not "TD/TRUST territory" left for later.
    Move "Authentication" out of the deployment-time-concerns bullet —
    OIDC SSO is implemented (`crates/api/src/auth/oidc.rs`). Rate limiting
    and multi-tenant isolation can stay listed as out of scope if still
    accurate (verify against current `crates/api` routes before
    finalizing this edit; not this plan's job to re-derive).
  - §3 Data sources table: the "TRUST movement events" row currently reads
    "optional, post-v1... Not required for v1" — update to reflect it's
    implemented and load-bearing for train tracking.
  - §4 Architecture: the ASCII diagram and "Why no streaming for v1"
    prose describe a FastAPI read layer and explicitly defer streaming
    ("The Network Rail STOMP feeds are powerful but operationally heavy" —
    now implemented via `trust-consumer`'s Kafka consumer). Replace with
    an accurate high-level description: Rust/axum `api`, Postgres, Redis
    (aggregator→enricher trigger queue), the Kafka-based `trust-consumer`
    + `enricher` pipeline, a Next.js frontend, all deployable via the Helm
    chart. A prose paragraph plus a lighter diagram is fine — this section
    doesn't need to become exhaustive, just no longer wrong.
  - §7 Project layout: same fix as `README.md`'s Layout section — replace
    the `src/*.py` tree with the real one.
  - Any remaining stray `nr-status` text elsewhere in the file (confirm via
    grep in Step 3, since this file wasn't read section-by-section in this
    plan's own research pass beyond §2-§4 and §7).

- [ ] **Step 3: Verify no live `nr-status`/`nr_status`/`nrstatus` text remains in either file**

  Run: `grep -in "nr-status\|nr_status\|nrstatus" README.md DESIGN.md`
  Expected: no output.

- [ ] **Step 4: Commit**

  ```bash
  git add README.md DESIGN.md
  git commit -m "Rename project to Distant Signal and fix README/DESIGN.md's stale architecture description"
  ```

---

### Task 2: API response type discriminator (`$type`)

**Files:**
- Modify: `crates/api/src/render.rs`
- Modify: `frontend/components/LineStatusCard.test.tsx`
- Modify: `frontend/app/lines/AllLinesTable.test.tsx`
- Modify: `frontend/lib/api.test.ts`
- Modify: `frontend/lib/history.test.ts`
- Modify: `frontend/lib/severity.test.ts`
- Modify: `frontend/lib/stationIssues.test.ts`

**Interfaces:**
- Produces: `to_tfl_shape`'s JSON now emits `"$type":
  "DistantSignal.LineStatusReport"` instead of
  `"NRStatus.LineStatusReport"`. No known external consumer parses this
  literal value today — `frontend/lib/types.ts` types the field as a bare
  `string`, and the six frontend test files below only use it as fixture
  input for unrelated assertions (line status rendering, severity
  ordering, etc.), never as an assertion target itself. Safe to change
  directly, no compatibility shim needed.

- [ ] **Step 1: Update the literal and its own test**

  In `crates/api/src/render.rs`, change `to_tfl_shape`'s
  `"$type": "NRStatus.LineStatusReport"` to
  `"$type": "DistantSignal.LineStatusReport"`, and update the matching
  `assert_eq!(json["$type"], "NRStatus.LineStatusReport")` in that file's
  own test module to the new value.

- [ ] **Step 2: Update the six frontend fixture files**

  In each of the six files listed above, change the hardcoded
  `$type: 'NRStatus.LineStatusReport'` fixture value to
  `$type: 'DistantSignal.LineStatusReport'`. These are fixture inputs, not
  assertions — the change is only needed so fixtures stop asserting a
  value the real API no longer returns, not because any test currently
  fails without it.

- [ ] **Step 3: Run both test suites**

  Run: `cargo test -p api` and (from `frontend/`) `npm test`
  Expected: both PASS — no test asserts on the literal `$type` value
  itself, per Step 2's note, so this is a smoke check that nothing was
  missed, not an expected-failure-then-fix cycle.

- [ ] **Step 4: Commit**

  ```bash
  git add crates/api/src/render.rs frontend/components/LineStatusCard.test.tsx frontend/app/lines/AllLinesTable.test.tsx frontend/lib/api.test.ts frontend/lib/history.test.ts frontend/lib/severity.test.ts frontend/lib/stationIssues.test.ts
  git commit -m "Rename the API's \$type response discriminator to DistantSignal.LineStatusReport"
  ```

---

### Task 3: Cosmetic Postgres env-var defaults

**Files:**
- Modify: `dev.env.example`
- Modify: `local.env.example`

**Interfaces:**
- Produces: `POSTGRES_USER`/`POSTGRES_DB` example defaults become
  `distant_signal` instead of `nr_status`. Cosmetic — these are
  `docker compose --env-file` example files, not read by any test. Only
  matters for anyone with an existing local dev Postgres volume already
  provisioned under the `nr_status` role/database name (their existing
  volume keeps working unchanged; only a fresh `docker compose up` after
  copying the updated example would create a differently-named role/db).

- [ ] **Step 1: Update both files**

  In `dev.env.example` and `local.env.example`, change:
  ```
  POSTGRES_USER=nr_status
  POSTGRES_DB=nr_status
  ```
  to:
  ```
  POSTGRES_USER=distant_signal
  POSTGRES_DB=distant_signal
  ```
  Leave `POSTGRES_PASSWORD` and every other line in both files untouched
  — `KAFKA_CONSUMER_GROUP` in both files is handled by Task 6, not here,
  since it carries the offset-reset risk this task's Postgres defaults do
  not.

- [ ] **Step 2: Verify no other `nr_status`/`nr-status` text remains in either file outside `KAFKA_CONSUMER_GROUP`**

  Run: `grep -in "nr-status\|nr_status" dev.env.example local.env.example`
  Expected: only the two `KAFKA_CONSUMER_GROUP=nr-status-trust-consumer`
  lines remain (Task 6's job).

- [ ] **Step 3: Commit**

  ```bash
  git add dev.env.example local.env.example
  git commit -m "Rename Postgres env-var example defaults from nr_status to distant_signal"
  ```

---

### Task 4: Helm chart rename — `charts/nr-status/` → `charts/distant-signal/`

**Files:**
- Rename: `charts/nr-status/` → `charts/distant-signal/` (git mv, all 24
  files move together: `Chart.yaml`, `README.md`, `values.yaml`,
  `values-example.yaml`, `templates/_helpers.tpl`, `templates/NOTES.txt`,
  17 other `templates/*.yaml`, `templates/tests/test-api-health.yaml`).
- Modify (post-rename, at the new path): `charts/distant-signal/Chart.yaml`,
  `charts/distant-signal/templates/_helpers.tpl`, every one of the 20
  template files that calls an `nr-status.*` helper, `charts/distant-signal/
  values.yaml`, `charts/distant-signal/values-example.yaml`,
  `charts/distant-signal/README.md`.

**Interfaces:**
- Produces: chart name `distant-signal`; every Helm template-helper
  renamed `distant-signal.*` (was `nr-status.*` — `.name`, `.fullname`,
  `.chart`, `.selectorLabels`, `.labels`, `.serviceAccountName`, `.image`,
  `.postgresFullname`, `.apiFullname`, `.frontendFullname`,
  `.redisFullname`, `.redisUrl`, `.apiBaseUrl`, `.podSecurityContext`,
  `.containerSecurityContext`, `.secretName`, `.postgresSecretName`,
  `.postgresSecretPasswordKey`, `.internalTokenSecretName`,
  `.internalTokenSecretKey`, `.ssoClientSecretName`, `.ssoClientSecretKey`,
  `.pollerSecretName`, `.pollerSecretKey`, `.databaseEnv`); default image
  repository paths `distant-signal/api`, `distant-signal/aggregator`, etc.
  No other file in this repo references `charts/nr-status` by path
  (verified in this plan's own research pass), so this task is
  self-contained.
- Consumes: nothing new. Confirmed no CI workflow exists in this repo
  (`.github/` is empty/absent) and no Dockerfile references an
  `nr-status`-branded registry path, so this rename has no build-pipeline
  side effect to account for.

- [ ] **Step 1: Move the directory**

  ```bash
  git mv charts/nr-status charts/distant-signal
  ```

- [ ] **Step 2: Rename every template-helper definition and call site in `_helpers.tpl`**

  In `charts/distant-signal/templates/_helpers.tpl`, rename every
  `{{- define "nr-status.XXX" -}}` to `{{- define "distant-signal.XXX" -}}`
  (24 definitions), every internal `{{ include "nr-status.XXX" ... }}`
  call within that same file to `distant-signal.XXX`, and the static
  `app.kubernetes.io/part-of: nr-status` label value (in the `.labels`
  helper) to `distant-signal`.

- [ ] **Step 3: Rename every `{{ include "nr-status.*" }}` call site across the other 19 template files**

  Across `templates/aggregator-deployment.yaml`, `api-deployment.yaml`,
  `api-service.yaml`, `enricher-deployment.yaml`, `frontend-deployment.yaml`,
  `frontend-service.yaml`, `ingress.yaml`, `networkpolicy.yaml`,
  `podmonitor.yaml`, `poller-deployments.yaml`, `postgres-service.yaml`,
  `postgres-statefulset.yaml`, `redis-deployment.yaml`, `redis-service.yaml`,
  `secret.yaml`, `serviceaccount.yaml`, `tests/test-api-health.yaml`,
  `trust-consumer-deployment.yaml`, and `NOTES.txt`, replace every
  `{{ include "nr-status.XXX" ... }}` with
  `{{ include "distant-signal.XXX" ... }}`. This is mechanical
  find-and-replace of the literal `nr-status.` prefix immediately inside
  an `include` call — confirm with the grep in Step 6 that nothing was
  missed, rather than hand-verifying each of the ~193 call sites
  individually.

- [ ] **Step 4: Update `Chart.yaml`, `values.yaml`, `values-example.yaml`**

  In `charts/distant-signal/Chart.yaml`: `name: nr-status` →
  `name: distant-signal`. Leave `home`/`sources`
  (`https://github.com/FasterSpeeding/nr-status-v2`) as they are — that
  URL depends on the GitHub repository's own name, which is explicitly
  out of scope for this plan (Task 7); update it only alongside that
  separate, human, follow-up action, not here (GitHub also transparently
  redirects the old path if the repo is later renamed, so leaving it for
  now is not a broken link in the meantime).

  In `charts/distant-signal/values.yaml` and `values-example.yaml`:
  - Every `repository: nr-status/<component>` default → `repository:
    distant-signal/<component>` (api, aggregator, enricher, frontend,
    poller-incidents, poller-stations, poller-tocs, poller-ldbws,
    poller-tfl — 9 entries per file).
  - `postgresql.auth.username`/`.database` defaults: `nr_status` →
    `distant_signal` (snake_case, per Global Constraints — these are the
    same identifiers Task 3 changed in the two env-example files; the
    chart's own copy is handled here rather than in Task 3, since it's
    part of the same directory being wholesale renamed).
  - `values.yaml`'s `trust-consumer.consumerGroup: nr-status-trust-consumer`
    default is **left as-is in this task** — it's handled together with
    every other Kafka consumer-group-ID copy in Task 6, not here, for the
    same reason `KAFKA_CONSUMER_GROUP` was excluded from Task 3.
  - Example secret names (`nr-status-db`, `nr-status-shared`,
    `nr-status-rdm`, `nr-status-llm`, `nr-status-sso`, `nr-status-tls`) in
    comments/example blocks → `distant-signal-*`.
  - `api.sso.clientId: nr-status` example → `distant-signal`.
  - The `# (nr-status.pollerSecretKey)` comment cross-reference → `#
    (distant-signal.pollerSecretKey)`.

- [ ] **Step 5: Rewrite `charts/distant-signal/README.md`**

  Update: title, the image-repository table (`nr-status/api` →
  `distant-signal/api`, etc., 8 rows), the `REG=registry.example.com/
  nr-status` example variable, every `helm install nr-status ./charts/
  nr-status -n nr-status` / `helm upgrade nr-status ./charts/nr-status
  -n nr-status` example command (4 occurrences) → `distant-signal`
  throughout, the `-f charts/nr-status/values-example.yaml` path, the
  `kubectl get secret -n nr-status nr-status` examples, the
  `existingSecret: nr-status-*` examples (db/shared/rdm ×2/llm/sso), the
  `clientId: nr-status` example, `secretName: nr-status-tls`, and the
  `postgresql.auth.username`/`.database` documentation rows
  (`nr_status` → `distant_signal`, including the inline
  `postgres://nr_status:s3cret@...` example URL).

  Additionally, add a short **"Renaming an existing release"** section (this
  is the "real redeploy wrinkle" the naming research doc flagged, and the
  reason this task isn't ordered before the purely-cosmetic Tasks 1-3):
  Helm has no in-place chart-rename operation for a release — the object
  names this chart's own `_helpers.tpl` derives are a function of
  `.Release.Name`/`.Chart.Name`, so a straight `helm upgrade` against the
  renamed chart directory produces a *new* set of object names (a
  StatefulSet with a new derived name, a new empty `volumeClaimTemplates`
  PVC) rather than renaming the existing ones in place. Document the
  concrete, low-risk path this chart's own `postgresql.persistence.
  existingClaim` value already supports: `helm get values <old-release>
  -n <ns> -o yaml > values.yaml` to capture the current config, `kubectl
  get pvc -n <ns>` to note the existing Postgres PVC's actual name, `helm
  uninstall <old-release> -n <ns>` (StatefulSet-owned PVCs are not deleted
  by `helm uninstall`), then `helm install <new-release-name> ./charts/
  distant-signal -n <ns> -f values.yaml --set postgresql.persistence.
  existingClaim=<the PVC name just noted>` to bind the new StatefulSet to
  the pre-existing data instead of provisioning an empty one. Flag this as
  untested against a real cluster in this plan (no live install exists to
  verify against) and recommend a backup/snapshot first regardless.

- [ ] **Step 6: Verify no `nr-status`/`nr_status` text remains anywhere in the chart, and the chart still renders**

  ```bash
  grep -rin "nr-status\|nr_status" charts/distant-signal/
  ```
  Expected: no output.

  ```bash
  helm lint charts/distant-signal
  helm template charts/distant-signal
  helm template charts/distant-signal --set metrics.podMonitor.enabled=true
  helm template charts/distant-signal --set networkPolicy.enabled=true
  ```
  Expected: all four render without error, and the rendered output's
  object names/labels read `distant-signal-*`/`app.kubernetes.io/part-of:
  distant-signal` throughout — spot-check with
  `helm template charts/distant-signal | grep -i "nr-status\|nr_status"`
  (expect no output) and
  `helm template charts/distant-signal | grep "part-of"` (expect
  `distant-signal` on every line).

- [ ] **Step 7: Commit**

  ```bash
  git add charts/distant-signal charts/nr-status
  git commit -m "Rename Helm chart from nr-status to distant-signal"
  ```

  (`git mv` plus the in-place edits above show up as renames with
  modifications under `git status`/`git add -A`-equivalent staging; adding
  both the new and — now-absent — old path is the standard way to stage a
  git-mv'd, subsequently-edited tree explicitly rather than relying on an
  implicit `git add -A`.)

---

### Task 5: Prometheus metrics prefix

**Files:**
- Modify: `crates/common/src/metrics.rs`
- Modify: `crates/api/src/main.rs`
- Modify: `crates/enricher/src/main.rs` (doc comment only)
- Modify: `crates/poller-tfl/src/main.rs` (doc comment only)

**Interfaces:**
- Produces: `common::metrics::metric_name` prepends `distant_signal_`
  instead of `nr_status_`; `api`'s `axum-prometheus` layer is built with
  `.with_prefix("distant_signal")` instead of `.with_prefix("nr_status")`.
  Every metric this app emits — hand-written (`aggregator`, `enricher`,
  five pollers) and `api`'s HTTP-layer metrics alike — moves to the new
  prefix together, in one commit, so no window exists where some metrics
  are `nr_status_*` and others `distant_signal_*` (both call sites are in
  this one task specifically to prevent that split; see this plan's
  verification findings above for why the research doc's original survey
  would have missed `api`'s call site).

**On whether this needs a dual-prefix transition window:** No — change
directly, for reasons specific to this project's actual current state,
verified in this plan's own research rather than assumed:
- This plan's Task 5-adjacent research confirms **no CI workflow exists in
  this repository** (no `.github/` directory) and the Helm chart's own
  `metrics.podMonitor.enabled` defaults to `false` (per
  `docs/superpowers/plans/2026-08-29-metrics.md`'s Global Constraints) —
  there is no evidence anywhere in this repo of a live Prometheus/Grafana
  deployment already scraping under the `nr_status_` prefix to break.
  Compare this to `docs/superpowers/specs/2026-08-21-multi-period-
  extraction-design.md`'s two-step column migration, which exists
  specifically *because* that schema is read by a running `aggregator`
  against live production data on every cycle — there is no equivalent
  "something is already reading this in production" pressure here.
- A dual-prefix window (emit under both names for a transition period)
  would add real complexity — two `metric_name`-shaped helpers, or a
  runtime flag, plus a follow-up cleanup task to remove it later — to
  guard against a scenario (an existing external dashboard/alert built
  against `nr_status_*`) this research pass found no evidence of. If that
  assumption is wrong for a specific real deployment, Task 7's follow-up
  list is exactly where that operator's own migration note belongs — this
  plan does not build speculative infrastructure for a risk that, per this
  plan's own verification, doesn't currently exist.

- [ ] **Step 1: Update `common::metrics::metric_name` and its tests**

  In `crates/common/src/metrics.rs`:
  - Change `format!("nr_status_{suffix}")` to
    `format!("distant_signal_{suffix}")`.
  - Update the preceding doc comment's `` `nr_status_` `` reference.
  - Update both existing unit tests' expected-string literals
    (`metric_name_adds_the_shared_prefix`,
    `metric_name_does_not_detect_or_strip_an_already_prefixed_suffix`) to
    the new prefix.

- [ ] **Step 2: Update `api`'s `axum-prometheus` prefix**

  In `crates/api/src/main.rs`, change
  `.with_prefix("nr_status")` to `.with_prefix("distant_signal")`.

- [ ] **Step 3: Update the two stale doc-comment cross-references**

  `crates/enricher/src/main.rs`'s `MismatchTracker::len` doc comment
  (`` `nr_status_enricher_mismatch_incidents` ``) and
  `crates/poller-tfl/src/main.rs`'s `fetch_json` doc comment
  (`` `nr_status_tfl_fetch_total` ``) — both reference the resulting full
  metric name for documentation purposes only (the actual name is computed
  through `metric_name` at the call site, already fixed in Step 1); update
  both comments to the new prefix so they stay accurate.

- [ ] **Step 4: Build and test**

  Run: `cargo build --workspace && cargo test -p common -p api`
  Expected: PASS.

- [ ] **Step 5: Manual verification (matches this repo's existing metrics-testing convention — a scripted `curl`, not a unit test, since the exporter installs a process-global recorder; see `docs/superpowers/plans/2026-08-29-metrics.md`'s "Testing convention for metrics")**

  With `api` running locally: `curl localhost:8080/metrics | grep
  distant_signal` should show output (or, if `api`'s HTTP metrics
  genuinely carry no hand-written `distant_signal_`-prefixed metric of
  their own beyond what `axum-prometheus` derives from the prefix option,
  confirm instead that no `nr_status` string appears in the output at
  all). With any of `aggregator`/`enricher`/a poller running locally with
  `METRICS_PORT` set: `curl localhost:9091/metrics | grep
  distant_signal_` should show output.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/common/src/metrics.rs crates/api/src/main.rs crates/enricher/src/main.rs crates/poller-tfl/src/main.rs
  git commit -m "Rename the Prometheus metric-name prefix from nr_status to distant_signal"
  ```

---

### Task 6: Kafka consumer-group ID

**Files:**
- Modify: `crates/trust-consumer/src/config.rs`
- Modify: `docker-compose.yml`
- Modify: `dev.env.example`
- Modify: `local.env.example`
- Modify: `charts/distant-signal/values.yaml`

**Interfaces:**
- Produces: `trust-consumer`'s `kafka_consumer_group` default (and every
  copy of the same literal default across the four config-example/compose
  files above, plus the Helm chart's `trust-consumer.consumerGroup`
  default) becomes `distant-signal-trust-consumer` instead of
  `nr-status-trust-consumer`.

**On whether this needs a transition/rollback posture:** No — change
directly, for reasons specific to this feature's actual current state,
verified in this task's own research: `crates/trust-consumer/src/
config.rs`'s own doc comments mark both `kafka_brokers` and `kafka_topic`
as `GAP: unconfirmed` — the real RDM Train Movements Kafka broker hostname
and topic name were never confirmed against a live subscription as of
this plan's writing (`docs/superpowers/specs/2026-08-28-train-tracking-
design.md`'s own Open Questions #1-#3). `dev.env.example`'s Kafka block
carries a matching comment: "trust-consumer will crash-loop on `docker
compose up` until real values are supplied." **There is no possible live
TRUST consumer with committed offsets against the current consumer-group
ID anywhere — not in this repo's own dev environment, let alone in
production** — because the broker connection this consumer group ID would
apply to has never been configured to a real endpoint. The research doc's
"offset-reset risk if a TRUST consumer is already live" concern is real in
general (renaming a Kafka consumer-group ID does reset offset tracking —
Kafka treats a new group ID as having no committed history, so it starts
from `auto.offset.reset` behavior on next connect), but does not currently
apply to this project, which this task treats as a fact to verify rather
than assume. If a real RDM Kafka subscription is connected before this
task lands, that changes the calculus — Task 7's follow-up list is where
that operator-specific migration note belongs, not a speculative
mechanism built here.

- [ ] **Step 1: Update the default in `trust-consumer`'s own config**

  In `crates/trust-consumer/src/config.rs`, change
  `default_value = "nr-status-trust-consumer"` to
  `default_value = "distant-signal-trust-consumer"`.

- [ ] **Step 2: Update the three matching literal defaults**

  In `docker-compose.yml`, `dev.env.example`, and `local.env.example`,
  change each `KAFKA_CONSUMER_GROUP=nr-status-trust-consumer` (or, in
  `docker-compose.yml`, `${KAFKA_CONSUMER_GROUP:-nr-status-trust-consumer}`)
  to use `distant-signal-trust-consumer`.

- [ ] **Step 3: Update the Helm chart's own copy of the default**

  In `charts/distant-signal/values.yaml`, change
  `trust-consumer.consumerGroup: nr-status-trust-consumer` to
  `distant-signal-trust-consumer`.

- [ ] **Step 4: Build and verify no residual references**

  Run: `cargo build -p trust-consumer`
  Run: `grep -rin "nr-status-trust-consumer" crates/trust-consumer docker-compose.yml dev.env.example local.env.example charts/distant-signal/values.yaml`
  Expected: build PASSes; grep produces no output.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/trust-consumer/src/config.rs docker-compose.yml dev.env.example local.env.example charts/distant-signal/values.yaml
  git commit -m "Rename the Kafka consumer-group ID from nr-status-trust-consumer to distant-signal-trust-consumer"
  ```

---

### Task 7: Out-of-scope follow-ups (human, one-off — no code changes)

This task produces no commit. It exists so the follow-ups this plan
deliberately doesn't automate are written down in one place rather than
scattered across the tasks above.

- [ ] **GitHub repository rename and remote URL update.** Rename the
  GitHub repository (currently referenced as
  `github.com/FasterSpeeding/nr-status-v2` in `charts/distant-signal/
  Chart.yaml`'s `home`/`sources` fields — confirm the actual current
  repository path directly on GitHub before renaming, since this may
  already differ from that Chart.yaml value) and update this local clone's
  `git remote -v` URL. GitHub redirects the old path automatically, so
  this is lower-risk than it sounds and not urgent relative to the rest of
  this plan.
- [ ] **Update `charts/distant-signal/Chart.yaml`'s `home`/`sources`
  fields** to the new repository URL once the above is done (Task 4
  deliberately left these pointed at the old URL — see that task's Step
  4 — specifically so this one edit happens once, alongside the actual
  repo rename, not twice).
- [ ] **Any external Prometheus/Grafana dashboards or alert rules** built
  against the `nr_status_*` metric prefix (Task 5) need their own,
  separate update once Task 5 lands — this plan found no evidence such a
  dashboard currently exists (see Task 5's reasoning), but if the project
  owner knows of one outside this repository, it isn't covered by
  anything in this plan.
- [ ] **Any live Kafka consumer with committed offsets** against
  `nr-status-trust-consumer` (Task 6) needs a real migration plan (e.g.
  manually copying committed offsets to the new group ID via the broker's
  admin tooling before cutting over, or accepting a one-time replay from
  `auto.offset.reset`) — this plan found no evidence such a consumer
  currently exists (see Task 6's reasoning: the broker connection itself
  is unconfigured), but the same caveat applies if that's changed since.
- [ ] **Any existing Helm release installed as `nr-status`** needs the
  migration path documented in Task 4's Step 5 (`charts/distant-signal/
  README.md`'s new "Renaming an existing release" section) — untested
  against a real cluster in this plan, since none exists to verify
  against; treat that section as a starting point, not a verified
  runbook, and back up/snapshot first.
- [ ] **The superseded Python prototype** (`src/*.py`, `demo.py`,
  `tests/test_matcher.py`, `plans/01-poller-microservices.md`) was left
  untouched by this plan on purpose (see Global Constraints). If the
  project owner wants it renamed, updated, or deleted, that's a separate,
  much smaller follow-up — not implied or required by this rename.
