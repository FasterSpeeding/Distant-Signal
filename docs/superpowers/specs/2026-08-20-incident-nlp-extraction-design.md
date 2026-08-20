# Incident NLP Extraction & Severity Classification Design

## Problem

Knowledgebase incidents carry almost all their real content as unstructured
English prose (`summary`, `description`). The system currently reads that
prose with a small ordered set of substring checks
(`severity_from_incident`, DESIGN.md §6.1) and otherwise trusts RDM's
structured fields (`is_planned`, `is_cleared`, `validity`, `priority`).
Four things the prose actually encodes, that the structured fields don't
reliably capture, are left on the table:

1. **A schedule window narrower than `validity`.** "Rail replacement buses
   will operate between Woking and Basingstoke each night this week, 22:00
   to 06:00" narrows a multi-day `validity` span to a few hours a night.
   Today this either shows as disruptive around the clock, or not at all
   if `validity` is absent.
2. **A real incident category** (signal failure, points failure, rail
   replacement, trespass, weather, strike, engineering overrun, etc.),
   used today only indirectly via a few severity-adjacent keywords ("rail
   replacement", "suspended").
3. **Resolution status as actually asserted by the author**, independent
   of `is_cleared`/`validity`. This is the gap the stale-incident-handling
   design
   (`docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md`)
   explicitly could not close: operators (SWR especially) routinely edit
   an incident's free text to say the root cause is fixed while leaving
   `is_cleared = false` and `validity` untouched, sometimes for weeks. The
   rail-day cutoff that design added bounds the damage for genuinely
   abandoned incidents, but still shows a stale "Severe Delays" for up to
   a full extra rail day, and does nothing at all for planned-work
   incidents, which are exempt from that cutoff by design.
4. **An estimated end time**, when the author states one in prose ("normal
   service expected to resume from 18:00"). Useful directly for display,
   and as an independent signal for retiring an incident sooner than the
   blunt rail-day cutoff would.

This document specifies extraction of all four, an LLM-backed extraction
service to produce them, and the severity-classifier changes that consume
the result.

## Goals

- Extract, per incident, a best-effort structured view of: incident
  category, resolution status (`ongoing` / `residual` / `resolved`), a
  narrowed applicability window (when the text states one more specific
  than `validity`), and an estimated end time (when stated).
- Cross-check resolution-status extraction with a second, adversarial pass
  that specifically argues the incident is still ongoing, and derive a
  confidence from agreement/disagreement between the two — a single
  self-reported confidence field is not trusted on its own for this field.
- Feed the result into severity classification as an additional signal,
  composed with what the pipeline already computes (match scope, LDBWS
  sample stats), without touching either of those.
- Degrade to exactly today's regex-only behavior whenever extraction is
  absent, low-confidence, or wrong-shaped. This project has no SLA and no
  funded on-call; a broken enrichment step must never be able to take
  displayed status down with it.
- Keep the enrichment step decoupled from the aggregator's per-cycle
  30-60s pure-function loop (DESIGN.md §4's stated design principle: pollers
  are I/O-bound, the aggregator is pure CPU over a snapshot).
- Build the LLM client against the generic OpenAI-compatible Chat
  Completions REST API (the de facto standard implemented by llama.cpp
  server, vLLM, Ollama, LM Studio, text-generation-webui, and every major
  hosted provider) with a configurable base URL/key/model, rather than
  against any one vendor's SDK. The intended first target is a local
  OpenAI-compatible server running a small-to-medium model; the design
  must not assume that server's identity or capabilities beyond the
  standard.

## Non-goals (this iteration)

- No fully-automated *suppression* of a `LineStatus` from a "resolved"
  extraction. A false "resolved" — silently hiding a genuinely active
  disruption — is strictly worse than today's staleness problem, where a
  status merely lingers too long but is never wrong about *ongoing*
  disruption. Extraction may only demote severity or annotate reason text
  in this iteration. Full suppression is a decision to revisit only after
  a season of production data shows the false-resolved rate is
  acceptably low.
- No weaving of `extracted_schedule_window` into segment/route matching
  (`matcher.rs`). It's surfaced as display/annotation metadata and used
  as a time-relevance gate on severity, not integrated into the matcher's
  evidence model.
- No local/on-device model, no always-on inference service beyond the one
  `enricher` crate this design adds. Given this project's dependency-light,
  single-maintainer profile, standing up and operating a model server is
  disproportionate — the point of building against the OpenAI-compatible
  standard is that the *caller* never needs to know whether what's behind
  the URL is local or hosted.
- No structured CRS/station-list extraction from prose in this pass — a
  related but distinct, already-flagged gap (DESIGN.md §10). Conflating it
  with this work risks shipping neither.
- No change to the regex category classifier's authority. The LLM's own
  category guess is stored for cross-validation but is advisory only; the
  keyword table remains the source of truth for `extracted_category`'s
  practical use in severity classification (there isn't any — see Design
  §5).

## Design

### 1. New crate: `crates/enricher`

A new binary crate, structurally parallel to `aggregator` and `api` (own
Docker image, own Helm deployment), added to the workspace `members` list.
It owns three concerns:

- A Redis Streams consumer-group loop that reacts to incident text changes.
- The two-pass OpenAI-compatible extraction client.
- Writing extraction results to `incidents`.

It connects to Postgres directly via its own `PgPool` — the same pattern
`aggregator` already uses (`crates/aggregator/src/main.rs`), not the
poller pattern of going through `api`'s ingest endpoints. `enricher` is not
a poller: it never touches an RDM feed, and nothing else writes the
columns it owns.

Configuration (env vars, following the existing `RDM_*_BASE_URL` pattern
in `local.env.example`/`dev.env.example`):

- `LLM_BASE_URL` — OpenAI-compatible base URL (e.g.
  `http://localhost:8080/v1` for a local server).
- `LLM_API_KEY` — optional; many local servers don't require one.
- `LLM_MODEL` — model name/identifier as the endpoint expects it.
- `DATABASE_URL`, `REDIS_URL` — same shape as existing DB/infra config.

### 2. Trigger: text-change detection at the existing diff point

`crates/api/src/data/queries.rs::upsert_incidents` already fetches the
existing row (`ExistingIncident { summary, description, validity_periods
}`) and computes `incident_changed` — true if the incident is new, or its
summary, description, or validity periods differ from what's stored — to
decide whether to append an `incident_history` snapshot. This is reused,
not duplicated:

- Add a narrower `text_changed` check against the same already-fetched
  `existing` row: true if summary or description differ (validity-only
  changes don't need re-extraction — the prose didn't move, so any prior
  extraction for that text is still valid).
- After the transaction **commits successfully**, `upsert_incidents`
  publishes one entry per text-changed incident to a Redis Stream
  (`incident-text-changed`), payload `{ incident_id }`. A publish failure
  is logged and does not fail the ingest request — `enricher`'s periodic
  sweep (§4) is the backstop for any gap here, not a reason to couple
  ingestion's success to Redis's availability.
- `api` gains a `redis` client dependency for this publish only; it does
  not consume from the stream.

### 3. Consumption: `enricher`'s Redis Streams consumer group

`enricher` runs a single consumer group against `incident-text-changed`.
On each entry: load the incident's current `summary`/`description` from
Postgres by `incident_id` (not the payload — always re-read current state,
since the entry may be processed well after publish), run the two-pass
extraction (§5), write results, `XACK` the entry. Consumer-group semantics
give at-least-once delivery — a crash between processing and `XACK`
reprocesses the entry, which is safe since extraction is idempotent per
text version (keyed by `source_text_hash`, see §6).

### 4. Backstop: periodic reconciliation sweep

Independent of the stream, `enricher` runs an hourly sweep: select
incidents where `source_text_hash IS DISTINCT FROM
encode(digest(summary || description, 'sha256'), 'hex')` (or the
equivalent computed in Rust after a plain fetch — exact mechanism is an
implementation-planning detail) and enqueues each directly, the same way a
stream entry would. This is the same "precise mechanism, blunt backstop
underneath" shape as the rail-day cutoff sitting under
`is_cleared`/`validity` in the stale-incident design — it exists
specifically to cover: Redis down at commit time, a publish that's lost
before the consumer group ever sees it, or `enricher` being down long
enough to fall behind in a way worth double-checking.

### 5. Extraction: two-pass, OpenAI-compatible

Two calls per changed incident, both against `{LLM_BASE_URL}/chat/completions`
with forced JSON-schema structured output
(`response_format: { type: "json_schema", json_schema: {...}, strict: true
}` — the broadly-supported structured-output variant across current
llama.cpp server/vLLM/Ollama builds and hosted providers; not tool-calling,
since local-server tool-calling support is less consistent):

- **Primary pass**: extracts `{ category, resolution_status,
  schedule_window, eta }` from `summary` + `description`.
  `resolution_status` is one of `ongoing` / `residual` / `resolved`.
  `schedule_window` and `eta` are nullable; `null` means "not stated in
  the text," not "unknown."
- **Adversarial pass**: given the same text, instructed specifically to
  argue the incident is still ongoing — i.e. to steelman the case against
  `resolved`/`residual`. Produces its own `resolution_status` verdict only
  (not the other fields — no benefit to re-extracting category/schedule/eta
  adversarially).

Combining the two:

| Primary | Adversarial | Result |
|---|---|---|
| `resolved` or `residual` | agrees (not `ongoing`) | `extraction_confidence = high`; store primary's `resolution_status` |
| `resolved` or `residual` | `ongoing` (disagrees) | `extraction_confidence = low`; store primary's `resolution_status` for audit, but the severity classifier treats this identically to no signal (§6 — disagreement is genuine ambiguity in the text, not evidence to average) |
| `ongoing` | anything | `extraction_confidence = high`; store `ongoing` (no demotion possible either way, so the adversarial pass's answer doesn't change the outcome) |

Either call failing schema validation, timing out, or erroring discards
that pass entirely — the whole extraction attempt is treated as failed,
not partially stored. A failed extraction leaves the incident's existing
columns untouched (they stay at their prior value, or null if this is the
first attempt) and does not update `extracted_at`, so the periodic sweep
naturally retries it on the next pass.

Category and schedule-window/ETA are single-pass (primary only) — the
adversarial cross-check is specific to resolution-status, the one field
with the asymmetric failure cost (§ Non-goals; DESIGN.md's demonstrated
project-wide preference for spending complexity only where the risk
justifies it).

### 6. Data model

New nullable columns on `incidents` (migration in `crates/api/migrations`,
mirroring how `first_seen_at` was added by the stale-incident-handling
migration — additive, no existing column touched):

| Column | Type | Purpose |
|---|---|---|
| `source_text_hash` | `TEXT` | Hash of `summary \|\| description` as of the last extraction attempt (successful or not — see §5). Change-detection key for the sweep. |
| `extracted_category` | `TEXT`, nullable | Primary pass's category guess. Stored for future cross-validation against the regex table; not currently consumed by severity classification. |
| `extracted_resolution_status` | `TEXT` (`ongoing` \| `residual` \| `resolved`), nullable | The core new signal. Null = no attempted or no successful extraction; behaves identically to today's classifier. |
| `extracted_schedule_window` | `JSONB`, nullable | Structured narrower applicability (days-of-week + time-of-day range) when stated. Display/annotation only (§ Non-goals). |
| `extracted_eta` | `TIMESTAMPTZ`, nullable | Author-stated expected resolution time, when present. |
| `extraction_confidence` | `TEXT` (`high` \| `low`), nullable | Gates whether `extracted_resolution_status` is allowed to influence severity (§5, §7). Null (no extraction yet) behaves as low. |
| `extraction_model_version` | `TEXT` | Identifies which `LLM_MODEL` (and, if versioned, prompt revision) produced this row — lets an operator force re-extraction of the whole table after a model/prompt change by bumping this and having the sweep treat a version mismatch as a hash mismatch, and correlates a regression with a specific rollout. |
| `extracted_at` | `TIMESTAMPTZ`, nullable | When extraction last *succeeded*. Observability, and lets the sweep distinguish "never attempted" from "attempted and failed" if needed later. |

All read-only outside `enricher`. `IncidentMessage`'s poller-facing wire
shape (`crates/common/src/lib.rs`) is untouched — the same separation
`first_seen_at` already established between what a poller emits and what
the database/aggregator-facing layer adds.

### 7. Severity classifier

`apply_extraction` is a new pure function in
`crates/aggregator/src/aggregation.rs`, run between the existing
`severity_from_incident` and `demote_for_scope` — both unchanged:

```
severity = severity_from_incident(incident)      // unchanged
severity = apply_extraction(severity, incident)  // new
severity = demote_for_scope(severity, scope)      // unchanged, still last
```

Rule table:

| Condition | Effect |
|---|---|
| No extraction row, or `extraction_confidence != high` | No change — identity function. Covers null, low-confidence (including the primary/adversarial disagreement case), and any failed extraction. |
| `extracted_resolution_status == "resolved"`, confidence high | Demote to `Severity::MinorDelays` (9) at most — never dropped entirely. Annotate reason text: "reported resolved — showing residual impact." Rail-day cutoff (`is_active`) still applies as an independent backstop. |
| `extracted_resolution_status == "residual"`, confidence high | Demote to `Severity::Recovering` (20) — the existing-but-currently-unused NR extension is exactly this state (DESIGN.md §5.4/§6.3). Annotated. |
| `extracted_resolution_status == "ongoing"` | No change — corroborates existing severity, no demotion. |
| `extracted_schedule_window` present and "now" falls outside it | Demote to `Severity::MinorDelays` (9) at most — same cap as the `resolved` case, never suppressed — and annotate reason text with the stated window (e.g. "reported active 22:00-06:00 only"). |
| `extracted_eta` present, already passed, no fresher extraction since | Same treatment as `resolved` — demote + annotate ("expected to end by HH:MM"), not suppressed, since a missed ETA is informative but not proof of resolution. |

This composes without touching the rest of the pipeline:

- **Match-scope demotion** (`demote_for_scope`) stays strictly last —
  scope is about matching confidence, an orthogonal question to what
  happened to the incident since it started.
- **LDBWS sample-derived severity** (`classify()` in the inference path)
  is untouched — it only runs when a line has no incident-derived status
  at all. An incident demoted by extraction still counts as "this line
  has an incident-derived status" (DESIGN.md §6.3's existing invariant),
  so sample-derived inference still won't override it.
- **Frontend `severityRank`/`GROUP_RANK`** (`frontend/lib/severity.ts`)
  needs no change — every severity value `apply_extraction` can produce
  (`MinorDelays`, `Recovering`, `GoodService`-adjacent) is already a
  defined `SEVERITY_TABLE` entry. The only frontend-visible change is
  which severity number a given incident maps to, plus new annotation
  text in `reason`.

This is not a proposal for a parallel severity scale — the existing
TfL-shaped scale already has the right extension point (`Recovering`,
previously unused) for the "residual effects" state this design's
research kept surfacing as the missing piece.

### 8. Deployment

- `docker-compose.yml` gains a `redis` service (standard `redis:` image,
  no special config needed for Streams) and an `enricher` service, matching
  the existing per-crate service pattern.
- `docker/enricher.Dockerfile` follows the existing per-crate Dockerfile
  pattern (`docker/aggregator.Dockerfile` as the closest analogue — same
  Rust toolchain version pinning, same BuildKit cache-mount approach).
- `local.env.example` and `dev.env.example` gain `LLM_BASE_URL`,
  `LLM_API_KEY`, `LLM_MODEL`, `REDIS_URL` — `local.env.example`'s
  `LLM_BASE_URL` follows the existing non-functional-placeholder
  convention used for `RDM_*_BASE_URL` (`*.example.invalid`);
  `dev.env.example` may point at a real local server address, since dev
  mode is meant to be actually run.
- `charts/nr-status` gains a `redis` dependency (subchart or a minimal
  in-chart Deployment/Service, matching whatever level of ceremony the
  existing chart uses for stateful services — resolve during
  implementation planning) and an `enricher` Deployment/config, following
  the pattern the recently-added Helm chart
  (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`) already
  establishes for the other services.

## Testing plan

- **Golden corpus, run as a live eval, not just fixtures.** A set of
  hand-labeled incident texts covering: clearly ongoing, clearly resolved,
  residual-only, an explicit ETA, a schedule window narrower than
  `validity`, and adversarial negation cases (`"has now ended"` vs. `"not
  expected to end soon"`; `"expect disruption until 18:00"` vs.
  `"disruption which began at 18:00"`). Run against whatever endpoint
  `LLM_BASE_URL` points at. This matters more here than a typical fixture
  suite: unlike a design built around one fixed hosted model, extraction
  accuracy is now a deployment-time variable, so this corpus is the thing
  that answers "is the configured model good enough" for a given
  deployment, not just "did the plumbing not regress."
- **`text_changed`** (query-layer, alongside the existing
  `incident_changed` tests if any exist, or newly added following the same
  style): new incident (true), summary changed (true), description
  changed (true), only `validity_periods` changed (false — the point of
  splitting this out from `incident_changed`), nothing changed (false).
- **`apply_extraction`**: every row of the §7 rule table as its own case,
  plus the primary/adversarial-disagreement path explicitly (confidence
  low, no demotion despite a `resolved` primary verdict), plus confirming
  it runs correctly *before* `demote_for_scope` (a `KeywordOnly`-scoped
  incident that's also extraction-demoted ends up capped at whichever is
  milder, not double-demoted past what either rule alone would produce).
- **Two-pass extraction combination logic**: unit-testable independent of
  any real LLM call — feed synthetic primary/adversarial verdict pairs
  through the combination table in §5 and assert the resulting
  `(resolution_status, confidence)`.
- **Sweep query**: an incident with a stale `source_text_hash` is
  selected; one with a current hash is not; one with a mismatched
  `extraction_model_version` behaves the same as a hash mismatch (covers
  the "force re-extraction after a model change" path from §6).
- **Redis consumer-group behavior**: at-least-once delivery and safe
  reprocessing after a crash-before-ack. Exact test mechanism
  (testcontainers, a docker-compose-based integration harness, or
  something else) is left to implementation planning — this project
  doesn't yet have an established pattern for infra-backed integration
  tests beyond the DB, so the right approach should be chosen alongside
  whatever precedent (if any) the `first_seen_at`/stale-incident work set
  for DB-backed query tests.
- **Deployment sanity**: `enricher` starts and reaches a healthy state
  against a `docker-compose` stack with `LLM_BASE_URL` pointed at a real
  OpenAI-compatible server (manual verification, matching how the project
  currently verifies new services — no existing end-to-end automated
  deployment test to extend).

## Open items carried into the implementation plan

- Exact schema/JSON-schema definitions for the primary and adversarial
  extraction calls (field names, enum values, nullability rules as sent to
  the model) — a prompt-engineering and eval concern best iterated on
  against the golden corpus during implementation, not fixed in this
  design.
- Redis subchart vs. minimal in-chart resource for
  `charts/nr-status` — depends on conventions the Helm chart work already
  settled that weren't re-examined in detail here.
- Whether `extraction_model_version` should encode a prompt-template
  version alongside the model name (e.g. `"gpt-oss-20b@prompt-v2"`) so a
  prompt-only change also forces re-extraction without a model swap —
  worth deciding before the first migration ships, since it's a format
  choice that's awkward to change once rows exist.
- Sweep interval (this design assumes hourly) and stream consumer-group
  name/count — tunable operational parameters, not architectural
  decisions.
- Whether `enricher` needs its own retry/backoff policy for a
  slow-but-not-erroring local LLM server (e.g. a small self-hosted model
  under load), distinct from simple failure — currently "discard and let
  the sweep retry later" (§5) is the whole policy; whether that's
  responsive enough in practice is worth revisiting once there's a real
  local server to test against.
