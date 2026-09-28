# Distant Signal: Design Document

A personal UK rail companion: line-status aggregation, individual train
tracking, accounts, and ticket/Delay-Repay support.

This document captures the design, the decisions behind it, and the open
questions, in enough detail that an implementer (human or LLM) can extend
the system without re-deriving the reasoning.

---

## 1. Goal

Build an open-source service that, given UK National Rail's data feeds,
produces TfL-Unified-API-shaped line status responses:

```
GET /Line/Mode/national-rail/Status
GET /Line/{ids}/Status?detail=true
GET /StopPoint/{crs}/Disruption
```

The output should let any client already built against TfL's API extend to
National Rail with minimal changes.

The aggregation layer — turning raw incidents and live train data into
"Severe Delays on the West Coast Main Line, lines blocked between Watford
Junction and Milton Keynes Central" — does not exist as open source today.
This project fills that gap.

---

## 2. Scope

**In scope.**
- Reading Knowledgebase incident messages (NRE-curated disruption text).
- Sampling LDBWS departure boards for live delay/cancellation rates.
- Defining a curated catalogue of "lines" (passenger-facing routes).
- Classifying each incident's scope (exclusive segment, shared trunk,
  operator-wide, etc.) and producing per-line statuses accordingly.
- Emitting TfL-shaped JSON.
- Train-level live tracking, implemented via TRUST movement events that
  `crates/movement-relay` relays from RDM's Kafka feed into a Redis Stream
  for `crates/trust-consumer` to read, not deferred TD/TRUST territory.
- Authentication — OIDC SSO is implemented (`crates/api/src/auth/oidc.rs`).

**Out of scope for v1.**
- Predicting future disruption (we report current state).
- Engineering-works calendars beyond what Knowledgebase already exposes.
- Multi-tenant isolation (a deployment-time concern). Per-client-IP rate
  limiting of the expensive public routes did land in the api
  (`crates/api/src/rate_limit.rs`).

---

## 3. Data sources

| Source | What it gives us | How we use it |
|---|---|---|
| **Darwin Knowledgebase Incidents** | Human-curated disruption messages with operator and station tags | Primary signal for `reason` text and severity. Highest data quality. |
| **OpenLDBWS** (or the new REST equivalent) | Live departure boards per station, including delay minutes, cancellations, and reason text | Sampling-based inference when no incident covers a line. Secondary signal. |
| **CIF SCHEDULE feed** | Static + short-term timetable, pushed to us over SFTP | Implemented (`crates/schedule-ingest` + `crates/schedule-reference`): schedule matching for tracked trains, station timetables, the full-coverage population, and trip planning. Line attribution still comes from the hand-curated catalogue, not CIF service groups (see §9). |
| **TRUST movement events** | Per-train movement and cancellation events, published by RDM on Kafka | Implemented and load-bearing: `crates/movement-relay` is the only Kafka client and relays every event into the `movement-events` Redis Stream, read by `crates/trust-consumer` (individual train tracking), `crates/full-coverage-consumer` (whole-line delay stats) and `crates/trust-backlog-consumer` (a short backlog for late-tracked trains). |
| **RDM Stations and TOC reference feeds** | Station and operator reference data | Station/operator catalogues (`poller-stations`, `poller-tocs`). |
| **TfL Unified API** | TfL's own line status | Shown as-is for TfL lines (`poller-tfl`, `dataQuality: tfl`). |
| **Island of Ireland feeds** | Iarnród Éireann GTFS and realtime XML; OpenDataNI's NIR station lists | Irish station/line catalogue and departure samples (`poller-irish-rail-gtfs`, `poller-irish-rail-live`, `poller-nir-stations`). |

The wider Network Rail/Darwin ecosystem (TD signal positions, RTPPM
performance, VSTP short-term schedule changes) is not used. They're
available if needed but aren't on the path to v1.

Knowledgebase and LDBWS, the two original sources, are accessible via the
**Rail Data Marketplace** (raildata.org.uk) — single sign-up, free tier
sufficient for development and small production loads — as are the
reference feeds and the TRUST Train Movements feed.

---

## 4. Architecture

The system is a Rust workspace of small services around a shared Postgres
database, plus a streaming pipeline (Kafka in, Redis Streams inside the
cluster) for train-level tracking. Only `api`, `aggregator`, `enricher` and
`notifier` talk to Postgres directly; every other service writes through
the api's internal-OAuth-protected `/private/*` ingest endpoints.

- The `poller-*` crates (`poller-incidents`, `poller-ldbws`,
  `poller-stations`, `poller-tocs`, `poller-tfl`, and the island-of-Ireland
  `poller-irish-rail-gtfs`, `poller-irish-rail-live`, `poller-nir-stations`)
  each pull one upstream source on a schedule and POST it to the api.
- `api` is a Rust/axum HTTP service — the ingest, read and auth layer. It
  serves the TfL-shaped line-status endpoints and the accounts, train
  tracking, journeys, groups and trip-planning endpoints, backed by
  Postgres. When an ingested incident's text changes it adds an entry to
  the `incident-text-changed` Redis Stream.
- The `aggregator` crate periodically loads incidents and samples from
  Postgres and the line catalogue from `lines/`, runs the matcher, applies
  scope/threshold rules, and writes `line_status`/`line_status_history`.
- `enricher` reads `incident-text-changed`, sends the incident text to an
  OpenAI-compatible LLM, and stores the extracted resolution status,
  category and per-period schedule facts, which the aggregator then applies
  to severity (`apply_extraction`).
- `movement-relay` is the only Kafka client: it reads RDM's TRUST Train
  Movements feed and relays every event into the `movement-events` Redis
  Stream. Three consumer groups read that stream: `trust-consumer`
  (movements for user-tracked trains), `full-coverage-consumer` (every
  scheduled train on full-coverage-enabled lines, for delay stats) and
  `trust-backlog-consumer` (a short backlog so a train tracked late can
  catch up).
- `schedule-ingest` watches the directory the CIF SCHEDULE feed is pushed
  into over SFTP and forwards each new delivery to the api;
  `schedule-reference`, in the same Pod, reads the extracted delivery and
  publishes derived tables to the api (STANOX/TIPLOC → CRS mappings, fixed
  links, per-line schedule populations, destination departures and calling
  points).
- `notifier` polls `line_status_history` and train movement events and
  sends Web Push notifications for users' pinned lines and tracked trains.
- `frontend/` is a Next.js web client. Browsers only ever talk to it; its
  `/api/*` route proxies to the `api` service. In production, public
  traffic arrives through a Cloudflare tunnel (Cloudflare tunnel →
  cloudflared → frontend → api); there is no ingress controller.
- The whole stack is deployable via the Helm chart at
  `charts/distant-signal/`.

```
  pollers (8)          schedule-ingest,        RDM Kafka (TRUST)
      │                schedule-reference             │
      │                      │                        ▼
      │                      │                 movement-relay
      │                      │                        │
      │                      │                        ▼
      │                      │          Redis Stream `movement-events`
      │                      │                        │
      │                      │                        ▼
      │                      │        trust-consumer, full-coverage-consumer,
      │                      │        trust-backlog-consumer
      │                      │                        │
      └──────────────────────┴───── POST /private/* ──┘
                                      │
                                      ▼
   browser ──▶ frontend ──▶ api (axum) ──▶ Postgres ◀──▶ aggregator, notifier,
              (Next.js)        │                           enricher
                               │                              ▲
                               └── Redis Stream ──────────────┘
                                   `incident-text-changed`
```

**Why separate pollers from the aggregator.** Pollers are I/O-bound and
need retries/backoff; the aggregator is pure CPU over a snapshot.
Decoupling lets you restart either without losing data, and makes testing
the aggregator trivial (it's a function from inputs to outputs).

**Why Postgres.** No special needs — boring relational storage with JSON
columns for the variable bits (incident metadata, sample departures).

**Why Redis.** Used for streams, not as a database: the
`incident-text-changed` stream gives push-driven enrichment (the api
signals `enricher` rather than the enricher polling Postgres; an hourly
sweep is only the backstop for a missed event), and the
`movement-events` stream lets one Kafka connection feed three independent
TRUST consumer groups.

**Why Kafka for TRUST.** Kafka is how RDM publishes it. Unlike
Knowledgebase incidents and LDBWS departure boards, TRUST movement events
are a genuine high-volume stream that benefits from an always-on consumer
rather than periodic polling. `movement-relay` and the three consumers are
long-running services by design — the operational cost of a 24/7 stream
consumer was worth it once individual train tracking became a real
feature, not just a hypothetical.

---

## 5. Domain model

### 5.1 Lines

A "line" is the unit of status reporting. Defined in TOML, one file per
line, under `lines/`. Each line has:

- A stable `id` (used in URLs — never change).
- A `name` (display text — change freely).
- A `mode` and `category` (grouping for display).
- An ordered list of `stations` from one end to the other.
- A list of `operators` (ATOC codes) that run services on it.
- Optional `match_keywords` and `excluded_keywords` for incident matching.
- Optional `severity_overrides` for per-line threshold tuning.
- Optional `sample_stations` (the stations LDBWS samples for this line).
- Optional `destination_crs_filter` / `headcode_prefixes` for LDBWS
  service-pattern filtering.
- Optional `full_coverage_enabled`, opting the line into TRUST-vs-schedule
  full-coverage stats (see `lines/SCHEMA.md`).

### 5.2 Segments — the central modelling decision

A naive design treats each line as an independent collection of stations.
That fails for any operator with parallel routes that share trunk track —
SWR (Waterloo trunk feeds 4+ routes), Southeastern (London Bridge / Charing
Cross), Northern, ScotRail. An incident at a junction would either be
attributed to one line (wrong — it affects all of them) or to all lines
sharing the operator (also wrong — most aren't actually involved).

The system models this by giving each station a `segment` field. Stations
on the same segment form a contiguous section of track. **The same segment
name appearing across multiple line definitions marks that section as a
shared trunk.** A `SegmentRegistry` computed at startup tells us, for any
segment, which lines use it and whether it's shared or exclusive.

**Authoring rule (the one rule everyone gets wrong).** Junction stations
belong to the **shared trunk**, not the exclusive segment. The exclusive
segment starts at the *next* station after the junction.

```
SWR example:

  WAT - CLJ - WIM - SUR - WOK | BSK - WIN - SOU - BMH - WEY
  [-------- swr-trunk-waterloo --------|--- swr-swml-south ----]
                                  junction
```

Woking (WOK) is on the shared trunk in all SWR line definitions that pass
through it. The South West Main Line's exclusive segment starts at
Basingstoke. An incident at Woking propagates to all SWR lines using the
trunk; an incident at Basingstoke or further south is local to SWML.

If you don't follow this rule, the segment registry will see the junction
station as belonging to one line's segment but not the others, and the
matcher will mis-classify shared-trunk incidents as exclusive.

### 5.3 Match scopes

When the matcher considers an incident against a line, it produces one of:

| Scope | Means | Severity treatment |
|---|---|---|
| `EXCLUSIVE_SEGMENT` | Incident's stations all sit on segments unique to this line | No demotion. Highest confidence. |
| `SHARED_SEGMENT` | At least one touched segment is shared | No demotion. Reason text annotated "shared trunk — also affects other lines." |
| `STATION_HIT` | Stations on the line, but no segment metadata to classify | No demotion. Fallback for under-specified line definitions. |
| `KEYWORD_ONLY` | Line named in incident text, no station hits | Capped at Severe Delays (severity 6). |
| `OPERATOR_ONLY` | Only operator overlap | Capped at Minor Delays (severity 9). |

The caps are measured with `common::severity_rank`, not the raw severity
number (see 5.4).

The matcher applies one further rule: **drop an `OPERATOR_ONLY` match
when another line sharing one of its operator codes got a more precise
match for the same incident**. This is what stops a single-station
incident on the Alton branch from also flagging South West Main and
Portsmouth Direct just because they share the SW operator code. The rule
is scoped per operator: a precise hit for one operator must not remove a
different operator's operator-only matches. It was added in response to a
test failure during development; do not remove it.

### 5.4 Severity scale

We use TfL's `statusSeverity` scale verbatim, with two extensions:

```
0  Special Service        7  Reduced Service
1  Closed                 8  Rail Replacement (BUS_SERVICE)
2  Suspended              9  Minor Delays
3  Part Suspended        10  Good Service
4  Planned Closure       11  Part Closed
5  Part Closure          12  Exit Only
6  Severe Delays         13  No Step Free Access
                         14  Change of Frequency

# NR-specific extensions, outside TfL's range to avoid clashes
20  Recovering   (post-incident catch-up)
21  Diverted     (services running but on alternative route)

# TfL codes 16-20, renumbered because 20/21 were already taken
22  Service Closed (TfL 20)   25  No Issues   (TfL 18)
23  Not Running    (TfL 16)   26  Information (TfL 19)
24  Issues Reported (TfL 17)
```

**The numbers are not ordered by severity.** TfL's codes are not
monotonic (Good Service = 10 sits between Minor Delays = 9 and
Part Closed = 11; Diverted = 21 is severe), so never compare two
severities by their number. `common::severity_rank` maps each one to a
rank (higher is worse) that mirrors `frontend/lib/severity.ts`;
`demote_for_scope`, `apply_extraction` and `LineStatusReport::worst_severity`
all go through it.

### 5.5 Data quality

Every emitted status carries a `dataQuality` field:

- `knowledgebase` — derived from a curated NRE incident message
- `planned` — derived from a Knowledgebase planned-work entry
- `ldbws-inferred` — derived from sampling departure boards
- `trust-inferred` — derived from full-coverage TRUST-vs-schedule stats,
  for lines with full coverage enabled
- `tfl` — TfL's own published line status

Clients should be able to filter or weight by quality. Surfacing this is
a deliberate departure from TfL's model, which doesn't expose it.

---

## 6. Aggregation logic

```
def aggregate(lines, incidents, samples, registry):
    reports = {line.id: empty_report(line) for line in lines}

    # Layer 1: incidents (highest confidence)
    for incident in incidents:
        for match in lines_affected_by(incident, lines, registry):
            status = status_from_incident(match, incident)
            reports[match.line.id].statuses.append(status)

    # Layer 2: samples. Inference for lines with no incidents; for lines
    # with incidents, live samples may escalate (never demote) severity
    for line in lines:
        if reports[line.id].statuses:
            escalate_from_sample_stats(reports[line.id], line, samples)
            continue
        inferred = infer_from_samples(line, samples)
        reports[line.id].statuses.append(inferred or good_service())

    return reports
```

(A sketch of `aggregation::aggregate`. Incidents first pass `is_active`,
and `main.rs` then runs `merge_full_coverage` over the result for
full-coverage-enabled lines.)

### 6.1 Incident → severity

The severity classifier is a sequence of keyword/hint checks against the
incident's combined summary + description text, in priority order:

```
is_planned                              → PLANNED_CLOSURE (4) (checked first)
"suspended" / "no service"              → SUSPENDED (2)
"rail replacement" / "replacement bus"  → BUS_SERVICE (8)
"lines blocked"                         → PART_SUSPENDED (3)
"cancel" (any inflection)               → PART_SUSPENDED (3)
"severe delays" / "major disruption"    → SEVERE_DELAYS (6)
"diverted"                              → DIVERTED (21)
otherwise                               → MINOR_DELAYS (9)
```

(`severity_from_incident` in `crates/aggregator/src/aggregation.rs`. The
RDM feed has no `severity_hint`, so the prototype's hint branches are
gone.)

After classification, `apply_extraction` adjusts the result using the
enricher's LLM extraction for the incident (demoting resolved or
out-of-window periods, and escalating on a high-confidence apparent
severity), and then `demote_for_scope` may cap it for weaker match scopes
(see 5.3).

The keyword ladder is intentionally simple; the LLM extraction is the
more sophisticated layer on top, and a missing or low-confidence
extraction leaves the keyword result unchanged.

### 6.2 Inference from LDBWS samples

For each line with no incident-derived status, the aggregator:

1. Collects departures from sampled stations (`line.sample_stations`).
2. Filters to departures matching the line by operator, plus optionally
   `destination_crs_filter` and `headcode_prefixes`.
3. Requires at least `min_sample_size` (default 3) services to make any
   non-Good determination — small samples are noisy.
4. Computes cancellation rate, delay rate (above
   `delay_threshold_minutes`) and skipped-stop rate.
5. Classifies against thresholds:

```
cancel_rate ≥ part_suspended_pct (60%)  → PART_SUSPENDED (3)
cancel_rate ≥ reduced_service_pct (25%) → REDUCED_SERVICE (7)
delay_rate  ≥ severe_delays_pct (50%)   → SEVERE_DELAYS (6)
delay_rate  ≥ minor_delays_pct (25%)    → MINOR_DELAYS (9)
otherwise                                → GOOD_SERVICE (10)
```

The skipped-stop rate is checked the same way against
`severe_delays_skip_pct` (50%) and `minor_delays_skip_pct` (25%), and the
more severe of the delay and skip results wins.

Thresholds are per-line overridable. Commuter lines should use tighter
thresholds than long-distance routes; a 5-minute delay on an 8-minute-
frequency service is materially worse than the same delay on an hourly
one.

### 6.3 What infer_from_samples deliberately doesn't do

- It doesn't try to identify *which* segment of a line is affected.
  Inference produces a line-wide status only. Segment-precision requires
  incident data.
- It doesn't replace an incident-derived status. If an incident is
  active, its status stays; live samples can only escalate its severity
  (`escalate_from_sample_stats`), never demote it.
- It doesn't compute trends (improving/worsening). Add a separate
  `Recovering` heuristic in v2 if useful.

---

## 7. Project layout

```
distant-signal/
├── README.md
├── DESIGN.md                  (this document)
├── lines/                     curatorial asset; well-reviewed, hand-edited
│   ├── SCHEMA.md
│   └── *.toml                 one file per line
├── crates/                    Rust workspace (25 crates; see root Cargo.toml)
│   ├── common/                 shared types, matcher, segments, config helpers
│   ├── api/                    axum HTTP service (read API, auth, ingest)
│   ├── aggregator/             the core status decision logic
│   ├── enricher/               LLM extraction from incident text
│   ├── notifier/               Web Push notifications
│   ├── poller-*/               eight pollers (RDM, TfL, island of Ireland)
│   ├── movement-relay/         RDM TRUST Kafka → `movement-events` stream
│   ├── trust-consumer/         movements for user-tracked trains
│   ├── full-coverage-consumer/ TRUST-vs-schedule stats for whole lines
│   ├── trust-backlog-consumer/ short TRUST backlog for late tracking
│   ├── schedule-ingest/        CIF SCHEDULE delivery watcher
│   ├── schedule-reference/     reference tables derived from the CIF delivery
│   ├── schedule-query/         CIF parsing / STP resolution library
│   ├── trip-planner/           CSA and RAPTOR journey search
│   ├── trust-schema/           TRUST message parsing library
│   ├── movement-feed/          shared movement-feed trait + Redis Stream client
│   ├── health-http/            shared /healthz endpoint
│   └── line-catalogue-validator/ checks lines/*.toml against reference data
├── frontend/                  Next.js web client
├── charts/distant-signal/     Helm chart for deploying the full stack
├── reference-data/            checked-in reference tables (e.g. stanox-crs.csv)
└── docker-compose.yml         local dev environment
```

The old single-package Python implementation this document originally
described has been superseded by the Rust workspace above; the domain
model and aggregation logic in §5 and §6 carried over largely unchanged.

---

## 8. Build sequence

For an implementer picking this up, the recommended order:

**Stage 1 — make the existing code production-ready.** Done: the
Knowledgebase and LDBWS pollers, scheduling, Postgres persistence, and the
HTTP read API are all implemented (`crates/poller-incidents`,
`crates/poller-ldbws`, `crates/aggregator`, `crates/api` — see §4). See
§10 for what's actually still open.

**Stage 2 — broaden the line catalogue.** Largely done: `lines/` now
holds 243 line definitions. The steps still apply to each new line.
1. Add the busiest 15-20 lines first. Major main lines (ECML, GWML,
   Midland Main Line) and busy commuter routes (Brighton Main Line,
   Chiltern, Northern City Line).
2. For each multi-route operator (SWR is the model), define one line
   per route with shared trunk segments correctly named.
3. Add tests for each new line that exercise a shared-trunk and an
   exclusive-segment incident.

**Stage 3 — improve quality.**
1. Better severity classifier (move from regex to a small trained model
   or LLM-based classifier, with the regex as fallback). Done: the
   `enricher`'s LLM extraction, with the keyword ladder as fallback (§6.1).
2. TRUST-feed integration for higher-fidelity inference. Done:
   `full-coverage-consumer` and the `trust-inferred` data quality (§5.5).
3. Trend detection (`Recovering` severity). Partly done: an incident the
   enricher reads as "residual delays only" is floored at `Recovering`;
   sample-based trend detection is still open.
4. History endpoints (`/Line/{id}/Status/{from}/to/{to}`). Done, along
   with the `/Line/{id}/Stats/...` rollups.

---

## 9. Decisions and their rationale

These are the choices someone might want to revisit. For each, the
reasoning that led to the current decision is recorded so revisiting can
be informed.

**TfL response shape.** Chosen so existing TfL clients can be reused with
minimal changes. The cost is some impedance mismatch (TfL has no concept
of "operator", we have to invent dataQuality, etc.) but the alternative —
bespoke schema — gets no leverage from the existing TfL ecosystem.

**Hand-curated line catalogue rather than auto-derivation from CIF.** CIF
service groups don't map cleanly to passenger-facing lines. "West Coast
Main Line" means different things to different services. Hand-curation is
the only way to get something a passenger would recognise. The cost is
ongoing maintenance, partially mitigated by treating the catalogue as a
contributor-friendly asset (one file per line, simple schema).

**Segments rather than ELRs.** Network Rail's ELRs (Engineer's Line
References) are physical track segments and the closest thing to a
canonical line definition in the industry data. We don't use them because:
(a) they're too granular — a passenger line crosses many ELRs; (b) ELR
boundaries don't align with service patterns; (c) mapping ELRs to user-
facing lines would be its own project. Our `segment` field is a
deliberately simpler abstraction: any string, defined by the line author,
with the only rule being "same string = same shared section."

**Operator-only matches capped at Minor Delays.** A vague TOC-wide
disruption message ("SWR services are subject to delays") shouldn't
trigger Suspended status across the entire SWR network. Capping is the
crude but effective fix. If a real network-wide event happens, the
Knowledgebase will produce a precise message that doesn't go through this
cap.

**The "drop operator-only matches when any precise match exists" rule.**
Caught by `aggregator_isolates_exclusive_incident`
(`crates/aggregator/src/aggregation.rs`) during development. Without it, an Alton-only incident also lights up SWML and
Portsmouth Direct as operator-only matches. This is structural: when any
line has precise evidence, the operator-only matches are noise from the
same incident, not separate evidence.

**Lower severity numbers = worse.** Inherited from TfL. Easy to
misimplement; documented prominently because of this.

**No streaming feeds for v1.** Polling is good enough for the time
granularity this product reports at (30-60s). Streaming infrastructure is
a meaningful operational burden. (Line status is still polled. Train
tracking later took on the TRUST stream; see §4.)

**Per-line threshold overrides instead of a global config.** A 5-minute
delay isn't equally significant on every line. Tuning is curatorial work
that lives next to the line definition. Defaults exist for the common case.

---

## 10. Known gaps and follow-ups

- **CRS extraction from incident prose.** Knowledgebase incidents
  reference stations by name in free text. We need a station-name → CRS
  lookup with fuzzy matching ("Watford Junction", "Wat Junction", "WFJ"
  all → WFJ). Use the `network-rail-gis` or equivalent reference data.
  Still open. Note what this does *not* block any more: the incident
  archive's Line filter used to depend on it (it matched a line's CRS list
  against `incidents.affected_stations`, a column nothing populates, and so
  returned zero rows for every line) and now does not — it matches
  `incidents.affected_lines`, written at ingest by `common::matcher`. What
  CRS extraction would still buy is the matcher's station/segment tiers,
  which no production incident reaches today.
- **Branching lines.** Current model handles linear lines well and
  shared-trunk-then-branch decently. True multi-branch lines (e.g. a
  service that splits at Haslemere with portions to different
  destinations) aren't modelled directly — define each branch as a
  separate line with a shared trunk segment.
- **Line catalogue is still growing.** 243 line definitions today,
  covering the major TOCs and regional networks; new routes still need
  hand-authoring.
- **Severity for engineering works.** Currently mapped to PLANNED_CLOSURE
  regardless of actual impact. A planned partial closure should map to
  PART_CLOSURE; needs a richer mapping.
- **No de-duplication of incidents.** If the same disruption appears in
  multiple Knowledgebase entries, the same line gets multiple statuses.
  Add deduplication keyed on incident IDs and overlapping station/time.

---

## 11. Testing strategy

Three test layers, in order of importance:

**Matcher tests (highest leverage).** For each line, exercise:
- An incident on an exclusive segment (must match only this line).
- An incident on a shared trunk segment (must match all lines using it).
- An incident matching by keyword only.
- An incident matching by operator only with no other lines matching
  precisely (should match, capped at Minor Delays).
- An incident matching by operator only when another line matches
  precisely (should NOT match — the suppression rule).
- An incident with an excluded keyword (must not match).

**Aggregator tests.** Verify the matcher's outputs become correct
statuses with correct severity, including demotion for weak scopes and
the inference fallback to Good Service.

**End-to-end tests.** A small set of scenarios run against the full
pipeline with synthetic inputs, verifying the rendered JSON matches
expectations.

`crates/common/src/matcher.rs`'s and `crates/aggregator/src/aggregation.rs`'s
own `mod tests` cover the matcher and aggregator layers, loading the
real `lines/` catalogue via `LineDefinition::from_dir` rather than
synthetic fixtures. Each new line should add at least one shared-trunk and
one exclusive-segment test case.

---

## 12. Conventions

- Rust workspace, one crate per concern (`common`, `aggregator`, `api`,
  `enricher`, the TRUST and schedule services, one `poller-*` crate per
  feed — see §7).
- Domain types are plain structs deriving `serde::{Serialize, Deserialize}`
  in `crates/common` (e.g. `LineDefinition`), not separate wire-format
  wrapper types.
- One concept per module. `common::matcher` matches incidents to lines,
  `common::segments` indexes shared/exclusive track, `aggregator`'s
  `aggregation.rs` aggregates. Don't merge them. The first two live in
  `common` rather than `aggregator` because `api` runs the same matcher at
  ingest to fill `incidents.affected_lines` — there must be exactly one
  answer to "which lines does this incident affect".
- `lines/*.toml` is the source of truth for the line catalogue, loaded via
  `LineDefinition::from_dir`. Don't hardcode line data in Rust.
- Tests live inline as `mod tests` next to the code they cover (see §11).
  The exceptions are crate-level integration tests under a crate's own
  `tests/` (e.g. `crates/api/tests/` for migration checks), not a
  separate top-level test tree.
- Comments explain *why*, not *what*. The "junction belongs to the
  shared trunk" rule and the "drop operator-only when precise match
  exists" rule are both commented in the code because they're
  non-obvious.

---

## 13. References

- TfL Unified API (the response shape we mimic):
  https://api.tfl.gov.uk
- Rail Data Marketplace (single sign-up for NR feeds):
  https://raildata.org.uk
- Open Rail Data Wiki (community docs for NR feeds):
  https://wiki.openraildata.com
- Open Rail Data GitHub org (reference clients):
  https://github.com/openraildata
- ATOC operator codes (TOC reference):
  https://wiki.openraildata.com/index.php/TOC_Codes
- CRS station codes:
  https://www.nationalrail.co.uk/stations_destinations/48541.aspx
