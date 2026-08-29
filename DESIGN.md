# Distant Signal: Design Document

A personal UK rail companion: line-status aggregation, individual train
tracking, accounts, and (soon) ticket/Delay-Repay support.

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
- Train-level live tracking, implemented via TRUST movement events consumed
  from Kafka (`crates/trust-consumer`), not deferred TD/TRUST territory.
- Authentication — OIDC SSO is implemented (`crates/api/src/auth/oidc.rs`).

**Out of scope for v1.**
- Predicting future disruption (we report current state).
- Engineering-works calendars beyond what Knowledgebase already exposes.
- Rate limiting, multi-tenant isolation (deployment-time concerns).

---

## 3. Data sources

| Source | What it gives us | How we use it |
|---|---|---|
| **Darwin Knowledgebase Incidents** | Human-curated disruption messages with operator and station tags | Primary signal for `reason` text and severity. Highest data quality. |
| **OpenLDBWS** (or the new REST equivalent) | Live departure boards per station, including delay minutes, cancellations, and reason text | Sampling-based inference when no incident covers a line. Secondary signal. |
| **CIF SCHEDULE feed** (optional, post-v1) | Static + short-term timetable | Resolving service groups for line attribution. Not required for v1. |
| **TRUST movement events** | Per-train movement and cancellation events, consumed live from Kafka | Implemented and load-bearing: the primary source for individual train tracking (`crates/trust-consumer` + `enricher`). |

The wider Network Rail/Darwin ecosystem (TD signal positions, RTPPM
performance, VSTP short-term schedule changes) is not used. They're
available if needed but aren't on the path to v1.

Both required sources are accessible via the **Rail Data Marketplace**
(raildata.org.uk) — single sign-up, free tier sufficient for development
and small production loads.

---

## 4. Architecture

The system is a Rust workspace of small services around a shared Postgres
database, plus a Kafka-based streaming pipeline for train-level tracking.

- A set of `poller-*` crates (`poller-incidents`, `poller-ldbws`,
  `poller-stations`, `poller-tfl`, `poller-tocs`) each pull one upstream
  source on a schedule and write into Postgres.
- The `aggregator` crate periodically loads incidents, samples, and line
  definitions, runs the matcher, applies scope/threshold rules, and writes
  `line_status`. It signals the `enricher` via a Redis queue when there's
  new work for it to pick up.
- `trust-consumer` is a long-running Kafka consumer for Network Rail's
  TRUST movement-event feed. It's the first persistent stream-consumer
  service in the stack (alongside `enricher`), and is what makes
  individual train tracking possible.
- `enricher` derives higher-level state (train positions, per-line status
  detail) from what the pollers and `trust-consumer` produce, triggered via
  the Redis queue the aggregator writes to.
- `api` is a Rust/axum HTTP service — the read (and auth) layer, serving
  both the TfL-shaped line-status endpoints and the newer accounts/
  train-tracking endpoints, backed by Postgres.
- `frontend/` is a Next.js web client against the `api` service.
- The whole stack is deployable via the Helm chart at
  `charts/distant-signal/`.

```
 poller-incidents  poller-ldbws  poller-stations  poller-tfl  poller-tocs
        │               │              │              │           │
        └───────────────┴──────────────┴──────────────┴───────────┘
                                    │
                                    ▼
                              Postgres  ◀──────────────┐
                                    │                  │
                                    ▼                  │
                              aggregator ──(Redis queue)──▶ enricher
                                                               ▲
  TRUST/Kafka feed ──▶ trust-consumer ─────────────────────────┘
                                    │
                                    ▼
                            api (axum) ──▶ frontend (Next.js)
```

**Why separate pollers from the aggregator.** Pollers are I/O-bound and
need retries/backoff; the aggregator is pure CPU over a snapshot.
Decoupling lets you restart either without losing data, and makes testing
the aggregator trivial (it's a function from inputs to outputs).

**Why Postgres.** No special needs — boring relational storage with JSON
columns for the variable bits (incident metadata, sample departures).

**Why Redis.** Used as a lightweight trigger queue between the aggregator
and `enricher`, not as a database — pub/sub-style signalling for
push-driven enrichment rather than the enricher polling Postgres.

**Why Kafka for TRUST.** Unlike Knowledgebase incidents and LDBWS
departure boards, TRUST movement events are a genuine high-volume stream
that benefits from an always-on consumer rather than periodic polling.
`trust-consumer` is a long-running service by design — the operational
cost of a 24/7 stream consumer was worth it once individual train
tracking became a real feature, not just a hypothetical.

---

## 5. Domain model

### 5.1 Lines

A "line" is the unit of status reporting. Defined in TOML, one file per
line, under `lines/`. Each line has:

- A stable `id` (used in URLs — never change).
- A `name` (display text — change freely).
- An ordered list of `stations` from one end to the other.
- A list of `operators` (ATOC codes) that run services on it.
- Optional `match_keywords` and `excluded_keywords` for incident matching.
- Optional `severity_overrides` for per-line threshold tuning.
- Optional `destination_crs_filter` / `headcode_prefixes` for LDBWS
  service-pattern filtering.

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

The matcher applies one further rule: **if any precise match exists for an
incident, drop all `OPERATOR_ONLY` matches**. This is what stops a
single-station incident on the Alton branch from also flagging South West
Main and Portsmouth Direct just because they share the SW operator code.
This rule was added in response to a test failure during development; do
not remove it.

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
```

**Lower numbers are more disruptive.** This trips people up — `min` is
the worst, `max` is the mildest. The `_demote_for_scope` function and the
`worst_severity` property both depend on this convention; if you change it
you must change both.

### 5.5 Data quality

Every emitted status carries a `dataQuality` field:

- `knowledgebase` — derived from a curated NRE incident message
- `planned` — derived from a Knowledgebase planned-work entry
- `ldbws-inferred` — derived from sampling departure boards
- `trust-inferred` — reserved for post-v1 TRUST-feed inference

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

    # Layer 2: inference for lines with no incidents
    for line in lines:
        if reports[line.id].statuses:
            continue
        inferred = infer_from_samples(line, samples)
        reports[line.id].statuses.append(inferred or good_service())

    return reports
```

### 6.1 Incident → severity

The severity classifier is a sequence of keyword/hint checks against the
incident's combined summary + description text, in priority order:

```
"suspended" / "no service"      → SUSPENDED (2)
"rail replacement" / "bus"      → BUS_SERVICE (8)
"lines blocked"                 → PART_SUSPENDED (3)
"severe delays" / "major"       → SEVERE_DELAYS (6)
severity_hint == "major"        → SEVERE_DELAYS (6)
"diverted"                      → DIVERTED (21)
"minor delays" / hint == "minor"→ MINOR_DELAYS (9)
otherwise                       → MINOR_DELAYS (9)
is_planned                      → PLANNED_CLOSURE (4) (overrides above)
```

After classification, `_demote_for_scope` may cap the result for weaker
match scopes (see 5.3).

This is intentionally simple. A more sophisticated approach (NLP, learned
classifiers) could improve precision but is out of scope for v1.

### 6.2 Inference from LDBWS samples

For each line with no incident-derived status, the aggregator:

1. Collects departures from sampled stations (`line.sample_stations`).
2. Filters to departures matching the line by operator, plus optionally
   `destination_crs_filter` and `headcode_prefixes`.
3. Requires at least `min_sample_size` (default 3) services to make any
   non-Good determination — small samples are noisy.
4. Computes cancellation rate and delay rate (above
   `delay_threshold_minutes`).
5. Classifies against thresholds:

```
cancel_rate ≥ part_suspended_pct (60%)  → PART_SUSPENDED (3)
cancel_rate ≥ reduced_service_pct (25%) → REDUCED_SERVICE (7)
delay_rate  ≥ severe_delays_pct (50%)   → SEVERE_DELAYS (6)
delay_rate  ≥ minor_delays_pct (25%)    → MINOR_DELAYS (9)
otherwise                                → GOOD_SERVICE (10)
```

Thresholds are per-line overridable. Commuter lines should use tighter
thresholds than long-distance routes; a 5-minute delay on an 8-minute-
frequency service is materially worse than the same delay on an hourly
one.

### 6.3 What infer_from_samples deliberately doesn't do

- It doesn't try to identify *which* segment of a line is affected.
  Inference produces a line-wide status only. Segment-precision requires
  incident data.
- It doesn't override an incident-derived status. If an incident is
  active, its status wins regardless of what samples show.
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
├── crates/                    Rust workspace
│   ├── common/                 shared types, config, metrics helpers
│   ├── api/                    axum HTTP service (read API, auth, ingest)
│   ├── aggregator/             the core status decision logic
│   ├── enricher/               derives train/line detail from raw events
│   ├── trust-consumer/         long-running Kafka consumer for TRUST
│   ├── poller-incidents/       Knowledgebase incidents poller
│   ├── poller-ldbws/           LDBWS departure-board sampler
│   ├── poller-stations/        station reference-data poller
│   ├── poller-tfl/             TfL reference/status poller
│   └── poller-tocs/            TOC (operator) reference-data poller
├── frontend/                  Next.js web client
├── charts/distant-signal/     Helm chart for deploying the full stack
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

**Stage 2 — broaden the line catalogue.**
1. Add the busiest 15-20 lines first. Major main lines (ECML, GWML,
   Midland Main Line) and busy commuter routes (Brighton Main Line,
   Chiltern, Northern City Line).
2. For each multi-route operator (SWR is the model), define one line
   per route with shared trunk segments correctly named.
3. Add tests for each new line that exercise a shared-trunk and an
   exclusive-segment incident.

**Stage 3 — improve quality.**
1. Better severity classifier (move from regex to a small trained model
   or LLM-based classifier, with the regex as fallback).
2. TRUST-feed integration for higher-fidelity inference.
3. Trend detection (`Recovering` severity).
4. History endpoints (`/Line/{id}/Status/{from}/to/{to}`).

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
Caught by `test_aggregator_isolates_exclusive_incident` during
development. Without it, an Alton-only incident also lights up SWML and
Portsmouth Direct as operator-only matches. This is structural: when any
line has precise evidence, the operator-only matches are noise from the
same incident, not separate evidence.

**Lower severity numbers = worse.** Inherited from TfL. Easy to
misimplement; documented prominently because of this.

**No streaming feeds for v1.** Polling is good enough for the time
granularity this product reports at (30-60s). Streaming infrastructure is
a meaningful operational burden.

**Per-line threshold overrides instead of a global config.** A 5-minute
delay isn't equally significant on every line. Tuning is curatorial work
that lives next to the line definition. Defaults exist for the common case.

---

## 10. Known gaps and follow-ups

- **CRS extraction from incident prose.** Knowledgebase incidents
  reference stations by name in free text. We need a station-name → CRS
  lookup with fuzzy matching ("Watford Junction", "Wat Junction", "WFJ"
  all → WFJ). Use the `network-rail-gis` or equivalent reference data.
- **Branching lines.** Current model handles linear lines well and
  shared-trunk-then-branch decently. True multi-branch lines (e.g. a
  service that splits at Haslemere with portions to different
  destinations) aren't modelled directly — define each branch as a
  separate line with a shared trunk segment.
- **Line catalogue is still growing.** ~20 lines defined today (WCML,
  Thameslink, SWR, Northern, Elizabeth line, Cross Country, and their
  branches); production coverage of all major TOCs/regional networks
  needs ~50-100 lines.
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

`crates/aggregator`'s own `mod tests` (in `src/matcher.rs` and
`src/aggregation.rs`) cover the matcher and aggregator layers, loading the
real `lines/` catalogue via `LineDefinition::from_dir` rather than
synthetic fixtures. Each new line should add at least one shared-trunk and
one exclusive-segment test case.

---

## 12. Conventions

- Rust workspace, one crate per concern (`common`, `aggregator`, `api`,
  `enricher`, `trust-consumer`, one `poller-*` crate per feed — see §7).
- Domain types are plain structs deriving `serde::{Serialize, Deserialize}`
  in `crates/common` (e.g. `LineDefinition`), not separate wire-format
  wrapper types.
- One concept per module within `aggregator`. `matcher.rs` matches.
  `aggregation.rs` aggregates. Don't merge them.
- `lines/*.toml` is the source of truth for the line catalogue, loaded via
  `LineDefinition::from_dir`. Don't hardcode line data in Rust.
- Tests live inline as `mod tests` next to the code they cover (see §11),
  not in a separate top-level test tree.
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
