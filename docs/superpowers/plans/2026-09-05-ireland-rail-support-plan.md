# Ireland Rail Support (Iarnród Éireann) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This plan has two task groups, deliberately separable, not two parts of
> one inseparable change** (unlike the Grafana plan's Part 1/in-repo vs.
> Part 2/GitOps split — everything here lives in this repo). **Group A
> (Tasks A1–A6) is Iarnród Éireann Tier A: the static GTFS-backed
> station/line catalogue. It is the must-ship deliverable and can be
> reviewed, merged, and deployed entirely on its own.** **Group B (Tasks
> B1–B6) is Iarnród Éireann Tier B: the live-departures poller. It is a
> fast-follow, explicitly allowed to merge later or separately** — it adds
> new tables/routes/a new crate and touches nothing Group A shipped, so nothing
> about Group A's mergeability depends on Group B landing at all. If you are
> executing this plan and only have appetite for one group, do Group A in
> full and stop — it is a complete, independently valuable, independently
> testable unit of work.

**Goal:** implement
`docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md`'s
confirmed "go" scope — Iarnród Éireann Tier A (static GTFS-backed
station/line reference data) as the must-ship deliverable, and Iarnród
Éireann Tier B (live per-station departures via `api.irishrail.ie`) as a
fast-follow in the same document. Backend only: a new shared `common::`
data model, a new GTFS-parsing ingestion crate, a new live-departures
polling crate, the minimal `api` storage/routes needed to make both
verifiable, and the Helm/Docker/CI wiring every existing ingestion crate in
this repo already gets. No Northern Ireland work of any kind (still blocked
on an unread CSV schema, design spec §8 open question #1). No
frontend/UI work (design spec's own Non-goals; `frontend/app/stations/[crs]/page.tsx`
and `frontend/app/stations/StationSearchForm.tsx:11` are CRS-keyed and
Iarnród Éireann stations have no CRS).

**Architecture:** one new `common::island_of_ireland` module holds the
network-tagged station/line/departure/sample types the design spec's §3
decided on. Tier A is a new standalone crate, `poller-irish-rail-gtfs`,
modeled directly on `crates/poller-stations` (fetch a feed on an interval,
parse it, `POST` the result to a new `api` private ingest route) but backed
by `gtfs-structures` instead of hand-rolled JSON/XML parsing. Tier B is a
second new standalone crate, `poller-irish-rail-live`, modeled on
`crates/poller-ldbws`'s per-station polling loop and `crates/poller-incidents`'s
`quick-xml` usage, but self-contained: it discovers its own station list
from `api.irishrail.ie`'s own `getAllStationsXML` call rather than depending
on Tier A's catalogue (see "Judgment calls," #1 — the two tiers' station id
schemes are not confirmed to match). Both crates get their own `api`
Postgres tables (upsert-on-id, no history — the same posture `stations`/`tocs`
already use), their own private ingest routes gated by their own internal-OAuth
group, a small public read-only route each (the "minimal API surface" the
brief allows, framed as backend plumbing, not a product feature), their own
Dockerfile, CI matrix entry, and Helm chart wiring (bespoke `Deployment`
templates, matching `movement-relay-deployment.yaml`'s single-purpose
precedent, not the generic `pollers` map — see "Judgment calls," #2).

**Tech Stack:** Rust 2024 edition (this workspace's existing floor), `axum`/`sqlx`/`tokio`
(existing `api` stack), `gtfs-structures` 0.50 (new, Tier A only),
`quick-xml` 0.41 (already a workspace dependency via `poller-incidents`/`poller-tocs`,
reused for Tier B), Postgres (existing), Helm (existing chart).

**Spec:** `docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md`
— authoritative for every architectural decision below; this plan
implements it, it does not re-derive it. Also load-bearing:
`docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md`
("the friction doc" below) — the design spec's own primary source for every
Iarnród Éireann factual claim (GTFS feed contents, `api.irishrail.ie`'s
confirmed schema), cited directly here rather than re-verified a third time,
per the brief's own instruction.

## Global Constraints

- **No Northern Ireland work of any kind.** Not a stub type, not a TODO
  comment naming NIR fields, not a `network` enum variant left unused —
  `IslandOfIrelandNetwork::NorthernIreland` is defined (the design spec's
  own §3 type, verbatim) because the enum is inherently two-sided, but no
  task in this plan writes, reads, or ingests a single `NorthernIreland`-tagged
  row. Blocked on design spec §8 open question #1 (NIR's OpenDataNI CSV
  schema, still unread) — out of this plan's power to resolve.
- **No frontend/UI work.** No new Next.js route, no new component, no
  change under `frontend/`. The new `api` public routes this plan adds
  (Tasks A3, B3) are consumed by `curl`/integration tests in this plan's own
  verification steps, not by any UI.
- **No aggregator wiring, no `LineStatus`/severity inference for Tier B.**
  The design spec's own §6 leaves "is a shared, generic delay-inference
  pipeline realistic across networks" an open, unresolved question,
  deferred until "at least one of the two Tier-B pipelines is actually
  built" — this plan is that first build, and deliberately stops at "raw
  samples, stored and readable," matching the git history precedent set by
  `crates/api/src/routes/departures.rs::get_station_departures` (a raw
  `station_samples` pass-through, shipped and merged to `main` with zero
  aggregator involvement, per this repo's own recent commit
  `a17513c Add GET /public/stations/{crs}/departures, a raw pass-through of
  the live station_samples board`). See "Judgment calls," #3.
- **No dedup/correlation between Iarnród Éireann's and NIR's border-area
  data.** Design spec §4 already resolved this: Iarnród Éireann is the sole
  source for Belfast/Lisburn/Portadown/Lurgan/Newry/the Enterprise line, no
  ingestion-time filtering needed (GTFS's own `stops.txt`/`routes.txt`
  already contains no NIR-side junction points — those only appear in the
  live API's `getAllStationsXML`, per friction doc §4 — so Tier A ingests
  every GTFS row unfiltered, tagged `RepublicOfIreland`).
- **`gtfs-structures` pinned to `0.50`**, the version confirmed live on
  crates.io by the friction doc (§2: "latest `0.50.0` published
  2026-09-01") and re-confirmed against docs.rs directly in this planning
  pass (see Task A4's own citations for the exact struct/method shapes
  used). Do not bump without re-reading its changelog — this plan's field
  names (`Stop.latitude`/`longitude`, `Route.long_name`/`short_name`,
  `Trip.stop_times`, `StopTime.stop_sequence`) are cited against this exact
  version.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` after every task that touches
  Rust — matching `.github/workflows/ci.yml`'s existing jobs (`cargo fmt
  --check` at `:139`, `cargo clippy --workspace --all-features` at `:98`,
  `cargo build --workspace`/`cargo test --workspace` at `:216-220`, plus
  `cargo test -p api -p aggregator -- --ignored --test-threads=1` at `:230`
  for DB-backed tests — this plan's new `api` DB tests must run under that
  same `-p api ... --ignored` invocation, not a new CI step). Helm: `helm
  lint`/`helm template` in both the new components' `enabled: true` and
  `enabled: false` states, matching this chart's existing per-toggle
  verification convention (see the Grafana plan's Task 2 for the precedent
  this plan's own Helm tasks follow).
- **File scope.**
  - New crates: `crates/poller-irish-rail-gtfs/`, `crates/poller-irish-rail-live/`.
  - Modified: `Cargo.toml` (workspace members), `crates/common/src/lib.rs`
    (one new `pub mod` line), `crates/common/src/island_of_ireland.rs`
    (new file), `crates/api/migrations/` (two new files), `crates/api/src/data/config.rs`,
    `crates/api/src/app.rs`, `crates/api/src/data/queries.rs` (or a new
    `crates/api/src/data/island_of_ireland.rs` — see Task A2's own
    reasoning), `crates/api/src/routes/ingest.rs`, `crates/api/src/routes/mod.rs`,
    a new `crates/api/src/routes/island_of_ireland.rs`, `docker/poller-irish-rail-gtfs.Dockerfile`,
    `docker/poller-irish-rail-live.Dockerfile`, `.github/workflows/containers.yml`,
    `charts/distant-signal/values.yaml`, `charts/distant-signal/values-example.yaml`,
    `charts/distant-signal/templates/podmonitor.yaml`, `charts/distant-signal/templates/secret.yaml`,
    two new Helm Deployment templates (`charts/distant-signal/templates/poller-irish-rail-gtfs-deployment.yaml`,
    `charts/distant-signal/templates/poller-irish-rail-live-deployment.yaml`),
    `charts/distant-signal/templates/api-deployment.yaml`. No other file
    changes.

---

## Judgment calls this plan makes (read before Task A1)

1. **Tier B does not depend on Tier A's catalogue for its own station
   list — it calls `api.irishrail.ie`'s own `getAllStationsXML` directly.**
   The obvious-looking alternative (Tier B reads Tier A's
   `island_of_ireland_stations` table, mirroring how `poller-ldbws` reads
   `api`'s `/private/sample-stations`) was considered and rejected: **this
   plan has not verified that GTFS `stops.txt`'s `stop_id` values (Tier A's
   station `id`) are the same identifier scheme as the live API's
   `StationCode` values (e.g. `BFSTC`, used by `getStationDataByCodeXML`).**
   The friction doc confirms both exist (§1: GTFS `stops.txt`'s 152 rows;
   a separate `getAllStationsXML` "171-station master list with codes/
   coordinates") but never states whether the two id spaces are the same —
   it treats them as two independent facts, never joins them. Given that,
   coupling Tier B's operation to Tier A's `id` column would be building on
   an unverified assumption, and would additionally make Tier B
   un-testable/non-functional if Tier A were ever deployed with stale or
   partial data. Calling `getAllStationsXML` directly makes Tier B fully
   self-contained (matches Tier A's own "zero dependency on any other new
   component" independence) at the cost of a real, stated consequence: **a
   Tier A `island_of_ireland_stations.id` value and a Tier B
   `island_of_ireland_station_samples.station_id` value for the same
   physical station are not guaranteed to match.** This plan does not
   reconcile them — flagged here and in Non-goals below as this plan's
   single biggest open risk. A future consumer wanting "show live
   departures under this catalogue station's page" needs that mapping built
   first; this plan does not need it, since it builds no such page.
2. **Both new pollers get bespoke Helm `Deployment` templates, not a new
   entry in the existing `.Values.pollers` map
   (`charts/distant-signal/templates/poller-deployments.yaml`).** That
   template's own header comment (`:1-10`) states its whole reason for
   existing: "Everything that differs between the pollers lives in the
   `.Values.pollers` map... this file never branches on a poller's name" —
   but every value in that map assumes an RDM-style upstream needing an API
   key (the template unconditionally renders an `apiKeyEnvVar` `Secret`
   reference at `:87-92`, defaulting to `RDM_API_KEY`). Neither new source
   needs a key at all: the GTFS zip is a plain anonymous HTTPS GET (friction
   doc §1: "no API key, no sign-up") and `api.irishrail.ie` is unauthenticated
   (friction doc §1: "fetched it live, successfully, on the first attempt,
   no key"). Forcing a mandatory-but-unused `apiKey`/`existingSecretApiKeyKey`
   pair onto both new crates' `Config` just to fit the generic template
   would mean either a fake required CLI arg with no real value ever set
   (this repo's own stated ethos, e.g. `poller-ldbws/src/config.rs`'s
   `ldbws_base_url` doc comment: "deliberately has no default... must be
   supplied out of band once confirmed, not guessed" — the opposite
   direction: never invent a config knob nothing real uses) or extending
   the shared template's conditional logic for a category of exactly two.
   `movement-relay-deployment.yaml` is this chart's own precedent for
   "a new ingestion-shaped component with its own Config shape gets its own
   template" (it isn't in the four-RDM-poller map either, for the same
   underlying reason: its `Config` doesn't fit that map's shape). This
   plan's two new templates follow that precedent, not the generic map's.
3. **Tier B ships as "poll, store, read back raw" — no `LineStatus`, no
   aggregator involvement, no severity classification.** Considered and
   rejected: wiring Tier B's samples through an `infer_from_samples`-shaped
   pipeline into a new `island-of-ireland-*` `LineStatus`/`modeName`, per
   design spec §3's closing paragraph ("whichever network's poller emits
   `LineStatus` rows would still route through the existing TfL-shaped JSON
   path"). Rejected for this pass specifically because design spec §6 says
   plainly that whether a shared inference pipeline is even realistic
   "depends on actually building at least one of the two Tier-B pipelines
   first and seeing how much of `infer_from_samples` genuinely generalizes"
   — i.e. the design spec itself treats severity inference as a *following*
   design question, not a decided part of "build Tier B." Scoping Tier B to
   raw ingestion + raw read-back (this plan's Tasks B1–B6) delivers
   everything design spec §5 actually promises ("A `poller-ldbws`-shaped
   service per station, feeding the same delay-threshold inference this app
   already runs" is the *target*, not a same-pass requirement) while
   leaving the inference-generalization question genuinely open for whoever
   answers it, backed by this plan's real data. This also keeps Tier B
   completely decoupled from `crates/aggregator`'s GB-specific,
   `lines/*.toml`-loaded `LineDefinition` machinery — no risk of
   destabilizing the existing GB severity pipeline while landing Ireland
   support.
4. **The new `common::` types live in one new module,
   `crates/common/src/island_of_ireland.rs`, not scattered into `lib.rs`
   alongside `Station`/`StationDeparture`/etc.** `lib.rs` is already 1,593
   lines and holds every GB-shaped type inline; `common::` does have a
   precedent for pulling a cohesive concern into its own file when it's
   substantial enough (`ingest.rs`, `metrics.rs`, `oauth_client.rs`,
   `rail_day.rs`, all declared via `pub mod` at `lib.rs:11-14`). The whole
   Iarnród Éireann/NIR domain (network enum, station, line, and — once
   Group B lands — departure and sample types) is exactly that kind of
   cohesive, separable concern, and keeping it in one file makes the "this
   whole module is GB-agnostic scaffolding" boundary visible at a glance
   rather than interleaved through `lib.rs`.
5. **Public read routes are added (`GET /public/island-of-ireland/stations`,
   `/lines`, and — Group B — `/stations/{id}/departures`), scoped
   deliberately narrow.** The brief allows "a small [API surface]... in the
   spirit of backend plumbing, not new frontend-facing product surface" if
   needed for verifiability. Without any public route, the only way to
   confirm Tier A actually populated real data end-to-end would be a raw
   `psql` query or an `--ignored` DB test — this plan adds both anyway
   (Tasks A2/B2), but a real HTTP round-trip (ingest → store → serve) is
   the more complete verification a reviewer would actually want, and it
   costs one small, unauthenticated, list-only route per tier — no search,
   no pagination (the whole catalogue is ~150-300 rows, comfortably small
   enough to return in one response, unlike `/public/stations`'s
   type-ahead search over thousands of GB stations). No frontend consumes
   either route in this plan.

---

## Non-goals

- **Northern Ireland (NIR) work of any kind.** See Global Constraints.
- **Frontend/UI work of any kind.** See Global Constraints.
- **Reconciling Tier A's GTFS-derived station `id` scheme with Tier B's
  live-API `StationCode` scheme.** Real, flagged (Judgment call #1), not
  resolved here. A future plan wanting to join the two needs to actually
  fetch and diff a real `getAllStationsXML` response against real
  `stops.txt` `stop_id` values first — this plan does not do that
  verification.
- **`LineStatus`/severity inference over Tier B's samples, or any
  `aggregator`/`crates/aggregator` change.** See Judgment call #3.
- **A dedup/correlation layer for the Belfast–Dublin Enterprise corridor.**
  Design spec §4 already decided against building one for now; this plan
  doesn't reopen that.
- **Any `lines/*.toml` change.** Iarnród Éireann's line catalogue lives
  entirely in the new `island_of_ireland_lines` table, not the GB
  TOML-loaded `LineDefinition` catalogue `crates/aggregator`/`crates/api`
  already load via `LineDefinition::from_dir` (`crates/common/src/lib.rs:507-510`).
  The two catalogues never merge in this plan.
- **NIR-shaped or GB-shaped route-mounting for Iarnród Éireann stations**
  (e.g. anything resembling `/stations/{crs}`). Iarnród Éireann stations
  have no CRS; the new routes are entirely separate paths
  (`/public/island-of-ireland/...`), never layered onto the existing
  CRS-keyed station routes.
- **Confirming `api.irishrail.ie`'s rate limits, uptime guarantees, or
  long-term availability.** Same open question the design spec's §8 (open
  question #4) already flagged and left open; this plan's Tier B config
  defaults conservatively (Task B4) precisely because this is unconfirmed,
  but does not resolve it.
- **Re-verifying the friction doc's own findings** (the GTFS feed's
  contents, `api.irishrail.ie`'s field list) by re-fetching either source.
  Per the brief, these are cited from the friction doc directly. This plan
  does independently re-verify `gtfs-structures`' current API shape against
  live `docs.rs` pages (cited per-task below), since the friction doc only
  confirmed the crate exists and is maintained, not its exact method/field
  names.
- **An implementation plan for Tier C (line-status/incident parity) for
  either network**, or for NIR Tier A/B. Design spec §7: no-go on all of
  these for now.

---

# Group A — Iarnród Éireann Tier A (must-ship)

## Task A1: `common::island_of_ireland` module — station/line types

**Files:** create `crates/common/src/island_of_ireland.rs`; modify
`crates/common/src/lib.rs`.

Independent first task — pure data types, no I/O, no DB, no other crate
depends on it yet.

- [ ] **Step 1: Declare the module.** In `crates/common/src/lib.rs`,
  alongside the existing module declarations at `:11-14`:

```rust
pub mod ingest;
pub mod island_of_ireland;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
```

- [ ] **Step 2: Write the types**, verbatim per design spec §3
  (`docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md:178-197`),
  with `Serialize`/`Deserialize` derives matching this crate's existing
  convention for a wire type consumed by both a producer crate (serializes)
  and `api` (deserializes) — the same posture `StationReference`
  (`crates/common/src/lib.rs:722-731`) and `StanoxCrsRecord` (`:740-750`)
  already use, snake_case on the wire (no `#[serde(rename_all =
  "camelCase")]`): these are private-ingest wire types between this app's
  own crates, not a frontend-facing response shape (see `ScheduleNetworkDeparturesRow`'s
  identical posture, `crates/api/src/data/queries.rs`).

```rust
//! Shared data model for both Irish-jurisdiction rail networks -- Iarnród
//! Éireann (Republic of Ireland) and, once
//! docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md's open
//! question #1 (NIR's OpenDataNI stations-CSV schema) is resolved, Northern
//! Ireland Railways/Translink. See that design doc's §3 for the full
//! reasoning behind one generic, network-tagged type rather than two
//! parallel network-specific ones.
//!
//! **Only `RepublicOfIreland`-tagged rows are ever constructed anywhere in
//! this codebase today.** `IslandOfIrelandNetwork::NorthernIreland` exists
//! because the enum is inherently two-sided (a station's authoritative
//! network has to be nameable even when only one value is real yet), not
//! because any NIR ingestion exists -- see
//! docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's Global
//! Constraints.
//!
//! `id` is the *sourcing* network's own station code/slug -- for Iarnród
//! Éireann, GTFS's own `stops.txt` `stop_id` (Tier A) is a DIFFERENT
//! identifier scheme from `api.irishrail.ie`'s own `StationCode` (Tier B) --
//! see the plan's Judgment Call #1. Do not assume the two match.

use serde::{Deserialize, Serialize};

/// Which network's own feed is authoritative for this row -- "which feed do
/// we source this from," not "which jurisdiction is this station
/// physically in" (design spec §3: the Belfast-area border stations are
/// tagged `RepublicOfIreland` despite being physically in Northern
/// Ireland, per design spec §4's single-authoritative-source policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IslandOfIrelandNetwork {
    NorthernIreland,
    RepublicOfIreland,
}

/// A station on either Irish-jurisdiction network. Deliberately NOT
/// `common::Station` (`crates/common/src/lib.rs:443-451`): that type's
/// `crs: String` field is required and has no Irish-network-shaped value
/// (design spec §3, carried from the superseded NI spec's own §2 finding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandOfIrelandStation {
    pub id: String,
    pub name: String,
    pub network: IslandOfIrelandNetwork,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// A named line/route on either Irish-jurisdiction network. Deliberately
/// NOT `common::LineDefinition` (`crates/common/src/lib.rs:461-500`): that
/// type's `stations: Vec<Station>` embeds CRS-keyed rows, `operators` is
/// ATOC-coded, and its `severity_overrides`/`sample_stations`/`exclusive_segments`
/// fields all exist to support this app's own GB severity-inference
/// pipeline, which this type does not participate in (see this plan's
/// Judgment Call #3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandOfIrelandLineDefinition {
    pub id: String,
    pub name: String,
    pub network: IslandOfIrelandNetwork,
    /// Ordered `IslandOfIrelandStation.id` values, same-network only. For a
    /// GTFS-sourced row (Tier A), this is the stop sequence of that
    /// route's longest trip -- see Task A4's own reasoning for why
    /// "longest trip" is this plan's chosen representative stopping
    /// pattern.
    pub stations: Vec<String>,
}
```

- [ ] **Step 2: Round-trip tests**, mirroring
  `crates/common/src/lib.rs`'s existing `sample_availability_tests`/
  `full_coverage_availability_tests` style (wire-tag assertions, not just
  "it compiles"):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_value(IslandOfIrelandNetwork::RepublicOfIreland).unwrap(),
            serde_json::json!("republic-of-ireland")
        );
        assert_eq!(
            serde_json::to_value(IslandOfIrelandNetwork::NorthernIreland).unwrap(),
            serde_json::json!("northern-ireland")
        );
    }

    #[test]
    fn station_round_trips_through_json() {
        let station = IslandOfIrelandStation {
            id: "8350IR0001".to_string(),
            name: "Dublin Connolly".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            latitude: Some(53.3556),
            longitude: Some(-6.2497),
        };
        let json = serde_json::to_value(&station).unwrap();
        let back: IslandOfIrelandStation = serde_json::from_value(json).unwrap();
        assert_eq!(back.id, station.id);
        assert_eq!(back.network, station.network);
    }

    #[test]
    fn line_definition_round_trips_through_json() {
        let line = IslandOfIrelandLineDefinition {
            id: "DUB-BFT-I".to_string(),
            name: "Belfast - Dublin".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            stations: vec!["8350IR0001".to_string(), "BFSTC".to_string()],
        };
        let json = serde_json::to_value(&line).unwrap();
        let back: IslandOfIrelandLineDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(back.stations, line.stations);
    }
}
```

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p common --all-features
cargo test -p common
```

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/lib.rs crates/common/src/island_of_ireland.rs
git commit -m "common: add island_of_ireland module with network-tagged Station/LineDefinition types"
```

---

## Task A2: Postgres schema + `api` data layer

**Files:** create `crates/api/migrations/20260905120000_island_of_ireland_reference.sql`;
create `crates/api/src/data/island_of_ireland.rs`; modify `crates/api/src/data/mod.rs`.

Depends on Task A1 (uses its types). Independent of Task A3/A4 otherwise —
this is pure storage, no routes yet.

- [ ] **Step 1: Migration.** Same upsert-on-id, no-history posture as
  `stations`/`tocs` (`crates/api/migrations/20260706004003_reference_data.sql:9-32`,
  whose own comment states the reasoning: "reference data is a snapshot of
  'current facts', not an event stream worth auditing").

```sql
-- Iarnród Éireann (and, once built, NIR) station/line reference data --
-- Tier A of docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md.
-- Same upsert-on-id, no-history posture as `stations`/`tocs`
-- (20260706004003_reference_data.sql): reference data is a snapshot of
-- current facts, not an event stream worth auditing.
--
-- `id` is TEXT, not CHAR(3) like `stations.crs` -- GTFS stop/route ids are
-- not fixed-width (friction doc's own sample ids run longer than 3
-- characters). `network` is a TEXT enum tag
-- (common::island_of_ireland::IslandOfIrelandNetwork's kebab-case wire
-- values: 'republic-of-ireland' | 'northern-ireland'), not a Postgres
-- native enum -- matching this schema's existing preference for plain TEXT
-- columns over native enum types (no CREATE TYPE anywhere in this
-- migration set).

CREATE TABLE island_of_ireland_stations (
    id          TEXT        PRIMARY KEY,
    name        TEXT        NOT NULL,
    network     TEXT        NOT NULL,
    latitude    DOUBLE PRECISION,
    longitude   DOUBLE PRECISION,
    fetched_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX island_of_ireland_stations_network_idx ON island_of_ireland_stations (network);

-- `stations` is an ordered JSONB array of `island_of_ireland_stations.id`
-- values (IslandOfIrelandLineDefinition.stations), not a join table --
-- same posture `schedule_line_population.population` and
-- `station_samples.departures` already take for a "always written and
-- read as a whole ordered unit" value (20260510023522_initial.sql's own
-- comment on `line_status.statuses`).
CREATE TABLE island_of_ireland_lines (
    id          TEXT        PRIMARY KEY,
    name        TEXT        NOT NULL,
    network     TEXT        NOT NULL,
    stations    JSONB       NOT NULL DEFAULT '[]',
    fetched_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX island_of_ireland_lines_network_idx ON island_of_ireland_lines (network);
```

- [ ] **Step 2: Data layer.** New file, mirroring
  `crates/api/src/data/reference.rs`'s module-per-concern structure and
  `crates/api/src/data/queries.rs::upsert_stations`/`upsert_tocs`/`last_stations_fetch`
  (`:225-249, 511-535, 552-558`) for the upsert/last-fetch shape:

```rust
//! Storage for `island_of_ireland_stations`/`island_of_ireland_lines` --
//! Tier A of docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md.
//! Same upsert-on-id, no-history shape as `crates/api/src/data/queries.rs`'s
//! `upsert_stations`/`upsert_tocs`.

use anyhow::Result;
use common::island_of_ireland::{IslandOfIrelandLineDefinition, IslandOfIrelandNetwork, IslandOfIrelandStation};
use sqlx::PgPool;

fn network_wire(network: IslandOfIrelandNetwork) -> &'static str {
    match network {
        IslandOfIrelandNetwork::NorthernIreland => "northern-ireland",
        IslandOfIrelandNetwork::RepublicOfIreland => "republic-of-ireland",
    }
}

fn network_from_wire(wire: &str) -> Result<IslandOfIrelandNetwork> {
    match wire {
        "northern-ireland" => Ok(IslandOfIrelandNetwork::NorthernIreland),
        "republic-of-ireland" => Ok(IslandOfIrelandNetwork::RepublicOfIreland),
        other => anyhow::bail!("unrecognized island_of_ireland network: {other}"),
    }
}

pub async fn upsert_stations(pool: &PgPool, stations: &[IslandOfIrelandStation]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for station in stations {
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_stations (id, name, network, latitude, longitude, fetched_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            ON CONFLICT (id) DO UPDATE SET
                name       = EXCLUDED.name,
                network    = EXCLUDED.network,
                latitude   = EXCLUDED.latitude,
                longitude  = EXCLUDED.longitude,
                fetched_at = NOW()
            "#,
        )
        .bind(&station.id)
        .bind(&station.name)
        .bind(network_wire(station.network))
        .bind(station.latitude)
        .bind(station.longitude)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

pub async fn upsert_lines(pool: &PgPool, lines: &[IslandOfIrelandLineDefinition]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for line in lines {
        let stations_json = serde_json::to_value(&line.stations)?;
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_lines (id, name, network, stations, fetched_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (id) DO UPDATE SET
                name       = EXCLUDED.name,
                network    = EXCLUDED.network,
                stations   = EXCLUDED.stations,
                fetched_at = NOW()
            "#,
        )
        .bind(&line.id)
        .bind(&line.name)
        .bind(network_wire(line.network))
        .bind(&stations_json)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

pub async fn last_stations_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM island_of_ireland_stations")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

pub async fn last_lines_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM island_of_ireland_lines")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

/// Backs `GET /public/island-of-ireland/stations` (Task A3) -- the whole
/// catalogue is small (~150-300 rows across both networks even once NIR
/// exists), so this is a plain unpaginated list, optionally filtered by
/// network, ordered by name -- same ordering choice as
/// `reference::get_all_tocs`.
pub async fn list_stations(
    pool: &PgPool,
    network: Option<IslandOfIrelandNetwork>,
) -> Result<Vec<IslandOfIrelandStation>> {
    let rows: Vec<(String, String, String, Option<f64>, Option<f64>)> = match network {
        Some(network) => {
            sqlx::query_as(
                "SELECT id, name, network, latitude, longitude FROM island_of_ireland_stations \
                 WHERE network = $1 ORDER BY name",
            )
            .bind(network_wire(network))
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, name, network, latitude, longitude FROM island_of_ireland_stations \
                 ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        }
    };

    rows.into_iter()
        .map(|(id, name, network, latitude, longitude)| {
            Ok(IslandOfIrelandStation {
                id,
                name,
                network: network_from_wire(&network)?,
                latitude,
                longitude,
            })
        })
        .collect()
}

pub async fn list_lines(
    pool: &PgPool,
    network: Option<IslandOfIrelandNetwork>,
) -> Result<Vec<IslandOfIrelandLineDefinition>> {
    let rows: Vec<(String, String, String, serde_json::Value)> = match network {
        Some(network) => {
            sqlx::query_as(
                "SELECT id, name, network, stations FROM island_of_ireland_lines \
                 WHERE network = $1 ORDER BY name",
            )
            .bind(network_wire(network))
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, name, network, stations FROM island_of_ireland_lines ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        }
    };

    rows.into_iter()
        .map(|(id, name, network, stations)| {
            Ok(IslandOfIrelandLineDefinition {
                id,
                name,
                network: network_from_wire(&network)?,
                stations: serde_json::from_value(stations)?,
            })
        })
        .collect()
}
```

- [ ] **Step 3: Wire the module.** In `crates/api/src/data/mod.rs`, add
  `pub mod island_of_ireland;` alongside the existing `pub mod reference;`
  etc. declarations.

- [ ] **Step 4: DB-backed tests**, following the reserved `Z…`-fixture-namespace
  convention `crates/api/src/data/reference.rs::db_tests` and
  `crates/api/src/routes/ingest.rs::db_tests` already use (seed under an
  invented id, delete after):

```rust
#[cfg(test)]
mod db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixture_station(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_stations WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture station");
    }

    async fn delete_fixture_line(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_lines WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture line");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_stations_then_list_round_trips_and_filters_by_network() {
        let pool = connect().await;
        delete_fixture_station(&pool, "ZIOI1").await;
        delete_fixture_station(&pool, "ZIOI2").await;

        let stations = vec![
            IslandOfIrelandStation {
                id: "ZIOI1".to_string(),
                name: "Zesttown".to_string(),
                network: IslandOfIrelandNetwork::RepublicOfIreland,
                latitude: Some(53.0),
                longitude: Some(-6.0),
            },
            IslandOfIrelandStation {
                id: "ZIOI2".to_string(),
                name: "Zorough".to_string(),
                network: IslandOfIrelandNetwork::NorthernIreland,
                latitude: None,
                longitude: None,
            },
        ];
        let upserted = upsert_stations(&pool, &stations).await.expect("upsert");
        assert_eq!(upserted, 2);

        let roi_only = list_stations(&pool, Some(IslandOfIrelandNetwork::RepublicOfIreland))
            .await
            .expect("list roi");
        assert!(roi_only.iter().any(|s| s.id == "ZIOI1"));
        assert!(!roi_only.iter().any(|s| s.id == "ZIOI2"));

        let all = list_stations(&pool, None).await.expect("list all");
        assert!(all.iter().any(|s| s.id == "ZIOI1"));
        assert!(all.iter().any(|s| s.id == "ZIOI2"));

        delete_fixture_station(&pool, "ZIOI1").await;
        delete_fixture_station(&pool, "ZIOI2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_lines_stores_ordered_stations_and_repeat_upsert_replaces_not_duplicates() {
        let pool = connect().await;
        delete_fixture_line(&pool, "ZLINE1").await;

        let first = IslandOfIrelandLineDefinition {
            id: "ZLINE1".to_string(),
            name: "Zest Line".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            stations: vec!["ZIOI1".to_string(), "ZIOI2".to_string()],
        };
        upsert_lines(&pool, &[first]).await.expect("first upsert");

        let second = IslandOfIrelandLineDefinition {
            id: "ZLINE1".to_string(),
            name: "Zest Line (renamed)".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            stations: vec!["ZIOI2".to_string(), "ZIOI1".to_string()],
        };
        upsert_lines(&pool, &[second]).await.expect("second upsert");

        let lines = list_lines(&pool, None).await.expect("list");
        let line = lines.iter().find(|l| l.id == "ZLINE1").expect("row present");
        assert_eq!(line.name, "Zest Line (renamed)");
        assert_eq!(line.stations, vec!["ZIOI2".to_string(), "ZIOI1".to_string()]);

        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM island_of_ireland_lines WHERE id = 'ZLINE1'")
                .fetch_all(&pool)
                .await
                .expect("select");
        assert_eq!(rows.len(), 1, "upsert must replace, not duplicate");

        delete_fixture_line(&pool, "ZLINE1").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn last_fetch_against_an_empty_table_is_null() {
        let pool = connect().await;
        // Reads the real table as-is -- relies on CI's freshly-migrated,
        // otherwise-empty database, same posture
        // `station_full_coverage_samples_get_last_fetched_on_an_empty_table_is_null`
        // already documents for its own table.
        let fetched = last_stations_fetch(&pool).await;
        assert!(fetched.is_ok());
    }
}
```

- [ ] **Step 5: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
# Requires a local Postgres with migrations applied -- see this repo's
# existing DATABASE_URL / sqlx-cli setup instructions (.env.example /
# README.md), unchanged by this task.
cargo test -p api island_of_ireland -- --ignored --test-threads=1
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/migrations/20260905120000_island_of_ireland_reference.sql \
        crates/api/src/data/island_of_ireland.rs crates/api/src/data/mod.rs
git commit -m "api: add island_of_ireland_stations/lines tables and data layer"
```

---

## Task A3: `api` routes — private ingest, public read, auth wiring

**Files:** modify `crates/api/src/routes/ingest.rs`, `crates/api/src/routes/mod.rs`,
`crates/api/src/data/config.rs`, `crates/api/src/app.rs`; create
`crates/api/src/routes/island_of_ireland.rs`.

Depends on Task A2.

- [ ] **Step 1: Private ingest routes.** In `crates/api/src/routes/ingest.rs`,
  add two route pairs to `router()` (alongside the existing `/stations`/`/tocs`
  pairs at `:38-45`):

```rust
        .route(
            "/island-of-ireland-stations",
            axum::routing::get(get_island_of_ireland_stations_last_fetched)
                .post(post_island_of_ireland_stations),
        )
        .route(
            "/island-of-ireland-lines",
            axum::routing::get(get_island_of_ireland_lines_last_fetched)
                .post(post_island_of_ireland_lines),
        )
```

  and the handlers, mirroring `post_stations`/`get_stations_last_fetched`
  (`:100-107, 146-154`):

```rust
async fn get_island_of_ireland_stations_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_stations_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_stations(
    State(app): State<App>,
    Json(stations): Json<Vec<common::island_of_ireland::IslandOfIrelandStation>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_stations(&app.database, &stations)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_island_of_ireland_lines_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_lines_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_lines(
    State(app): State<App>,
    Json(lines): Json<Vec<common::island_of_ireland::IslandOfIrelandLineDefinition>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_lines(&app.database, &lines)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 2: Public read route.** New file, mirroring
  `crates/api/src/routes/reference.rs`'s `public_router()`-style shape and
  `crates/api/src/routes/departures.rs`'s 404-vs-`200 []` honesty split is
  NOT needed here (a network filter with zero matches is a legitimate,
  non-error "no rows yet" state for a reference listing, unlike a specific
  station's departure board):

```rust
//! `GET /public/island-of-ireland/stations`, `/lines`: read-only listing of
//! the Iarnród Éireann (and, once built, NIR) station/line catalogue.
//! Unauthenticated, read-only, no pagination -- the whole catalogue is a
//! few hundred rows at most. See
//! docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
//! Judgment Call #5 for why this route exists at all (verifiability, not a
//! frontend feature -- nothing in `frontend/` consumes this).

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use common::island_of_ireland::{IslandOfIrelandLineDefinition, IslandOfIrelandNetwork, IslandOfIrelandStation};
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::island_of_ireland;

pub fn router() -> Router {
    Router::new()
        .route("/island-of-ireland/stations", axum::routing::get(list_stations))
        .route("/island-of-ireland/lines", axum::routing::get(list_lines))
}

#[derive(Debug, Deserialize)]
struct NetworkFilter {
    #[serde(default)]
    network: Option<String>,
}

fn parse_network(raw: &Option<String>) -> Result<Option<IslandOfIrelandNetwork>, (StatusCode, String)> {
    match raw.as_deref() {
        None => Ok(None),
        Some("republic-of-ireland") => Ok(Some(IslandOfIrelandNetwork::RepublicOfIreland)),
        Some("northern-ireland") => Ok(Some(IslandOfIrelandNetwork::NorthernIreland)),
        Some(other) => Err((
            StatusCode::BAD_REQUEST,
            format!("unrecognized network filter: {other}"),
        )),
    }
}

async fn list_stations(
    State(app): State<App>,
    Query(filter): Query<NetworkFilter>,
) -> Result<Json<Vec<IslandOfIrelandStation>>, (StatusCode, String)> {
    let network = parse_network(&filter.network)?;
    let stations = island_of_ireland::list_stations(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok(Json(stations))
}

async fn list_lines(
    State(app): State<App>,
    Query(filter): Query<NetworkFilter>,
) -> Result<Json<Vec<IslandOfIrelandLineDefinition>>, (StatusCode, String)> {
    let network = parse_network(&filter.network)?;
    let lines = island_of_ireland::list_lines(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok(Json(lines))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "island-of-ireland catalogue query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_network_accepts_both_known_values_and_none() {
        assert_eq!(parse_network(&None).unwrap(), None);
        assert_eq!(
            parse_network(&Some("republic-of-ireland".to_string())).unwrap(),
            Some(IslandOfIrelandNetwork::RepublicOfIreland)
        );
        assert_eq!(
            parse_network(&Some("northern-ireland".to_string())).unwrap(),
            Some(IslandOfIrelandNetwork::NorthernIreland)
        );
    }

    #[test]
    fn parse_network_rejects_unknown_values() {
        assert!(parse_network(&Some("mars".to_string())).is_err());
    }
}
```

  Register it in `crates/api/src/routes/mod.rs`'s `public_router()` merge
  chain (find the existing `.merge(reference::router())`-shaped line and
  add `.merge(island_of_ireland::router())` alongside it, plus `pub mod
  island_of_ireland;` in that file's module declarations).

- [ ] **Step 3: New internal-OAuth group.** In `crates/api/src/data/config.rs`,
  alongside the existing group fields (`:77-102`):

```rust
    /// Gates `POST`/`GET /private/island-of-ireland-stations` and
    /// `/island-of-ireland-lines` -- the new `poller-irish-rail-gtfs`
    /// crate's own credential. See
    /// docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md Task A3.
    #[arg(long, env, default_value = "svc-poller-irish-rail-gtfs")]
    pub internal_oauth_group_irish_rail_gtfs: String,
```

- [ ] **Step 4: Wire it into `build_internal_oauth_routes`.** In
  `crates/api/src/app.rs`, alongside the existing `/stations`/`/tocs`
  entries (`:71-90`):

```rust
        (
            "/island-of-ireland-stations",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-stations",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-lines",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-lines",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
```

  Also add the field to the `Debug`-adjacent field-name-listing arrays
  around `:304-334` (search that block for `"internal_oauth_group_stations"`
  to find the exact repeated-field-list pattern; add a matching
  `("internal_oauth_group_irish_rail_gtfs", &config.internal_oauth_group_irish_rail_gtfs),`
  entry) and to every test-fixture `ServiceArguments { .. }` literal this
  plan's own new DB tests construct (Task A2 doesn't construct one; Task A4
  doesn't either — no other test file needs updating for this field, since
  `#[derive(Debug, clap::Parser)]`-constructed structs in existing tests
  like `routes::ingest::db_tests::test_app` (`:433-461`) DO need this new
  field added to their literal, since `ServiceArguments` gains a new
  required-to-construct field with a default only at the CLI-parsing layer,
  not the struct-literal layer — add
  `internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),`
  to every existing `test_app`/fixture `ServiceArguments` literal across
  `crates/api/src/routes/*.rs` and `crates/api/src/data/*.rs` (confirm the
  full list via `grep -rln "internal_oauth_group_full_coverage:" crates/api/src`
  before starting this step — every file that list returns needs the same
  one-line addition).

- [ ] **Step 5: Verify**

```bash
grep -rln "internal_oauth_group_full_coverage:" crates/api/src   # confirm every fixture site found
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/ingest.rs crates/api/src/routes/mod.rs \
        crates/api/src/routes/island_of_ireland.rs crates/api/src/data/config.rs \
        crates/api/src/app.rs
git commit -m "api: add private ingest + public read routes for the island-of-ireland catalogue"
```

---

## Task A4: `poller-irish-rail-gtfs` crate

**Files:** create `crates/poller-irish-rail-gtfs/Cargo.toml`,
`crates/poller-irish-rail-gtfs/src/{main.rs,config.rs,mapping.rs}`; modify
`Cargo.toml` (workspace members).

Depends on Tasks A1/A3 (the private ingest routes must exist for this
crate's `main.rs` to target, though the crate compiles and its own unit
tests pass without a live `api`).

- [ ] **Step 1: Cargo.toml**, modeled directly on
  `crates/poller-stations/Cargo.toml` (fetch-JSON-parse-post shape) plus
  `gtfs-structures`:

```toml
[package]
name = "poller-irish-rail-gtfs"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
gtfs-structures = "0.50"
metrics = "0.24"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

  No `gtfs-structures` feature flags enabled: its `reqwest`-backed
  `from_url`/`from_url_async` constructors are behind an opt-in feature this
  crate does not enable (confirmed against `docs.rs/gtfs-structures/latest`
  this planning session: "the `reqwest` library... is listed as an
  *optional* dependency, meaning you must enable the `reqwest` feature flag
  to use URL-based loading" — left disabled so this crate fetches bytes
  itself via its own already-configured `reqwest::Client`, matching every
  sibling poller's own fetch/parse separation, e.g.
  `poller-stations/src/main.rs`'s `fetch_stations_json` + `schema::parse_stations`
  split). No `serde_json` dependency: this crate parses GTFS CSV via
  `gtfs-structures`, never JSON directly.

- [ ] **Step 2: `config.rs`**, modeled on
  `crates/poller-stations/src/config.rs`, minus the RDM API key (none
  needed — Judgment Call #2):

```rust
use clap::Parser;

/// CLI/env configuration for the `poller-irish-rail-gtfs` service.
///
/// `gtfs_url` DOES have a working default, unlike every RDM poller's own
/// `baseUrl` (which is account-specific and unpublished): the friction doc
/// (docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md
/// §1) confirms this is a real, public, key-free, anonymous-GET URL,
/// downloaded and verified directly in that research session -- matching
/// `poller-tfl`'s own precedent (`TFL_BASE_URL` defaults to the real TfL
/// API root) for "a genuinely public endpoint gets a working default,
/// unlike an account-gated one."
#[derive(Debug, Parser)]
pub struct Config {
    /// Transport for Ireland's public GTFS zip for Iarnród Éireann.
    #[arg(
        long,
        env,
        default_value = "https://www.transportforireland.ie/transitData/Data/GTFS_Irish_Rail.zip"
    )]
    pub gtfs_url: String,

    /// The `api` crate's ingestion endpoint for the station catalogue.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/island-of-ireland-stations"
    )]
    pub api_stations_ingest_url: String,

    /// The `api` crate's ingestion endpoint for the line catalogue.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/island-of-ireland-lines"
    )]
    pub api_lines_ingest_url: String,

    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    /// The friction doc confirms `feed_start_date`/`feed_end_date` show "a
    /// live, rolling one-year window" but never states how often the feed
    /// itself is regenerated -- unlike RDM's `stations`/`tocs` feeds, whose
    /// spec explicitly recommends a 24-hour poll. Defaulted to the same
    /// 24-hour cadence as `poller-stations`/`poller-tocs`
    /// (`crates/poller-stations/src/config.rs`'s own `poll_interval_secs`
    /// default) as the conservative, already-established convention for
    /// "static reference data with an unconfirmed real refresh cadence" --
    /// not a confirmed fact about this specific feed's own update
    /// frequency.
    #[arg(long, env, default_value_t = 86400)]
    pub poll_interval_secs: u64,

    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

- [ ] **Step 3: `mapping.rs`** — GTFS → `common::island_of_ireland` mapping,
  the crate's real logic, fully unit-testable with no network/DB. Field
  names below are cited against `docs.rs/gtfs-structures/latest`'s
  `Gtfs`/`Stop`/`Route`/`Trip`/`StopTime` pages, fetched directly in this
  planning session (version 0.50.0, matching the friction doc's own
  crates.io confirmation):

```rust
//! Maps a parsed `gtfs_structures::Gtfs` feed onto
//! `common::island_of_ireland::{IslandOfIrelandStation, IslandOfIrelandLineDefinition}`.
//!
//! Field provenance, confirmed against docs.rs/gtfs-structures/latest
//! (v0.50.0) directly in this crate's planning pass:
//! - `Gtfs.stops: HashMap<String, Arc<Stop>>`, `Gtfs.routes: HashMap<String, Route>`,
//!   `Gtfs.trips: HashMap<String, Trip>`.
//! - `Stop.id: String`, `Stop.name: Option<String>`, `Stop.latitude`/`longitude: Option<f64>`.
//! - `Route.id: String`, `Route.long_name`/`short_name: Option<String>` (both optional).
//! - `Trip.id: String`, `Trip.route_id: String`, `Trip.stop_times: Vec<StopTime>`.
//! - `StopTime.stop: Arc<Stop>`, `StopTime.stop_sequence: u32`.
//!
//! Every Iarnród Éireann row is tagged `RepublicOfIreland` unconditionally
//! -- no border-station filtering. Design spec §4 already decided this
//! (Iarnród Éireann is the sole source for the Belfast-area stations/the
//! Enterprise line), and the friction doc (§4) confirms GTFS's own
//! `stops.txt` already contains no NIR-side signalling junctions (those
//! only appear in the live API's `getAllStationsXML`) -- so there is
//! nothing to filter out even if this crate wanted to.

use std::collections::HashMap;

use common::island_of_ireland::{IslandOfIrelandLineDefinition, IslandOfIrelandNetwork, IslandOfIrelandStation};
use gtfs_structures::Gtfs;

pub fn map_stations(gtfs: &Gtfs) -> Vec<IslandOfIrelandStation> {
    gtfs.stops
        .values()
        .map(|stop| IslandOfIrelandStation {
            id: stop.id.clone(),
            // `Stop.name` is `Option<String>` in gtfs-structures; GTFS's
            // own spec requires it for a `location_type` of "stop"
            // (ordinary passenger stations), so an empty fallback here is
            // defensive, not an expected real-world case for this feed.
            name: stop.name.clone().unwrap_or_default(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            latitude: stop.latitude,
            longitude: stop.longitude,
        })
        .collect()
}

/// For each route, picks that route's LONGEST trip (most `stop_times`) as
/// the representative stopping pattern. A route can have multiple trips
/// with different stopping patterns (e.g. a peak express skipping stops an
/// off-peak service calls at); GTFS carries no single "canonical" stop
/// sequence per route, only per-trip sequences. "Longest trip wins" is a
/// concrete, defensible v1 choice (it captures the fullest possible
/// picture of a route's own stations, at the cost of not distinguishing
/// express/stopping variants) -- deliberately not a general timetable
/// model. A future pass wanting real trip-variant awareness needs a
/// different `IslandOfIrelandLineDefinition.stations` shape entirely, not a
/// tweak to this function.
pub fn map_lines(gtfs: &Gtfs) -> Vec<IslandOfIrelandLineDefinition> {
    let mut trips_by_route: HashMap<&str, Vec<&gtfs_structures::Trip>> = HashMap::new();
    for trip in gtfs.trips.values() {
        trips_by_route.entry(trip.route_id.as_str()).or_default().push(trip);
    }

    gtfs.routes
        .values()
        .map(|route| {
            let name = route
                .long_name
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| route.short_name.clone())
                .unwrap_or_else(|| route.id.clone());

            let stations = trips_by_route
                .get(route.id.as_str())
                .and_then(|trips| trips.iter().max_by_key(|t| t.stop_times.len()))
                .map(|trip| {
                    let mut stop_times: Vec<&gtfs_structures::StopTime> = trip.stop_times.iter().collect();
                    stop_times.sort_by_key(|st| st.stop_sequence);
                    stop_times.into_iter().map(|st| st.stop.id.clone()).collect()
                })
                .unwrap_or_default();

            IslandOfIrelandLineDefinition {
                id: route.id.clone(),
                name,
                network: IslandOfIrelandNetwork::RepublicOfIreland,
                stations,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal, valid GTFS feed in-memory (three files are the
    /// GTFS-required floor for `Gtfs::from_reader` to succeed at all:
    /// `agency.txt`, `stops.txt`, `routes.txt`, plus `trips.txt`/
    /// `stop_times.txt` for this test's own assertions) and round-trips it
    /// through a real zip so this test exercises the same
    /// `Gtfs::from_reader` code path `main.rs` uses, not a hand-built
    /// `Gtfs` struct literal (whose exact field set could drift from what
    /// the crate actually requires).
    fn build_test_feed_zip() -> Vec<u8> {
        use std::io::Write;

        let files: &[(&str, &str)] = &[
            ("agency.txt", "agency_id,agency_name,agency_url,agency_timezone\nIR,Iarnrod Eireann,https://example.invalid,Europe/Dublin\n"),
            ("stops.txt", "stop_id,stop_name,stop_lat,stop_lon\nSTOP_A,Zesttown,53.0,-6.0\nSTOP_B,Zorough,53.1,-6.1\n"),
            ("routes.txt", "route_id,agency_id,route_short_name,route_long_name,route_type\nROUTE_1,IR,,Zesttown - Zorough,2\n"),
            ("trips.txt", "route_id,service_id,trip_id\nROUTE_1,WEEKDAY,TRIP_1\n"),
            ("stop_times.txt", "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nTRIP_1,08:00:00,08:00:00,STOP_A,1\nTRIP_1,08:10:00,08:10:00,STOP_B,2\n"),
            ("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nWEEKDAY,1,1,1,1,1,0,0,20260101,20271231\n"),
        ];

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            for (name, contents) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(contents.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    fn test_feed() -> Gtfs {
        let bytes = build_test_feed_zip();
        Gtfs::from_reader(std::io::Cursor::new(bytes)).expect("parse test feed")
    }

    #[test]
    fn map_stations_maps_id_name_coordinates_and_tags_republic_of_ireland() {
        let gtfs = test_feed();
        let stations = map_stations(&gtfs);
        assert_eq!(stations.len(), 2);
        let a = stations.iter().find(|s| s.id == "STOP_A").expect("STOP_A present");
        assert_eq!(a.name, "Zesttown");
        assert_eq!(a.network, IslandOfIrelandNetwork::RepublicOfIreland);
        assert_eq!(a.latitude, Some(53.0));
        assert_eq!(a.longitude, Some(-6.0));
    }

    #[test]
    fn map_lines_uses_long_name_and_orders_stations_by_stop_sequence() {
        let gtfs = test_feed();
        let lines = map_lines(&gtfs);
        assert_eq!(lines.len(), 1);
        let route = &lines[0];
        assert_eq!(route.id, "ROUTE_1");
        assert_eq!(route.name, "Zesttown - Zorough");
        assert_eq!(route.network, IslandOfIrelandNetwork::RepublicOfIreland);
        assert_eq!(route.stations, vec!["STOP_A".to_string(), "STOP_B".to_string()]);
    }
}
```

  This test needs a dev-dependency on the `zip` crate to construct its
  fixture: add `zip = "8"` under `[dev-dependencies]` in this crate's
  `Cargo.toml` (matching the version floor `docs.rs` reported
  `gtfs-structures` 0.50 itself depends on — confirmed this planning
  session: "The crate lists `zip ^8.6` as a normal dependency" — pin to the
  same major version so the fixture zip's format is guaranteed compatible
  with what `gtfs-structures` reads).

- [ ] **Step 4: `main.rs`**, modeled on `crates/poller-stations/src/main.rs`'s
  poll-loop shape:

```rust
//! `poller-irish-rail-gtfs`: downloads Transport for Ireland's public GTFS
//! zip for Iarnród Éireann on an interval, parses it via `gtfs-structures`,
//! and forwards the derived station/line catalogue to `api`'s
//! `/private/island-of-ireland-{stations,lines}` ingestion endpoints. Tier
//! A of docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md;
//! see docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md Task A4.

mod config;
mod mapping;

use std::time::Duration;

use clap::Parser;
use common::ingest::{self};
use config::Config;
use gtfs_structures::Gtfs;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    // Freshness is checked against the stations endpoint only -- both
    // ingest together every cycle (see poll_once), so one check suffices,
    // matching poller-ldbws's own single freshness check even though it
    // also posts to a second api endpoint conceptually (sample-stations is
    // a GET, not a parallel POST target, but the precedent for "one
    // freshness check per poller, not one per ingest target" holds).
    let delay = ingest::time_until_next_poll(
        &client,
        &config.api_stations_ingest_url,
        &internal_oauth,
        poll_interval,
    )
    .await;
    if !delay.is_zero() {
        tracing::info!(delay_secs = delay.as_secs(), "data still fresh from a prior run; delaying first poll");
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);

    loop {
        interval.tick().await;

        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &internal_oauth).await;
        metrics::histogram!(
            common::metrics::metric_name("poller_cycle_duration_seconds"),
            "poller" => "irish-rail-gtfs"
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            common::metrics::metric_name("poller_cycle_total"),
            "poller" => "irish-rail-gtfs",
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<()> {
    let bytes = client
        .get(&config.gtfs_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let gtfs = Gtfs::from_reader(std::io::Cursor::new(bytes.to_vec()))
        .map_err(|err| anyhow::anyhow!("failed to parse GTFS feed: {err}"))?;

    let stations = mapping::map_stations(&gtfs);
    let lines = mapping::map_lines(&gtfs);
    tracing::info!(
        stations = stations.len(),
        lines = lines.len(),
        "parsed Iarnrod Eireann GTFS feed"
    );

    ingest::post_batch(client, &config.api_stations_ingest_url, internal_oauth, &stations, "island-of-ireland stations").await?;
    ingest::post_batch(client, &config.api_lines_ingest_url, internal_oauth, &lines, "island-of-ireland lines").await?;
    Ok(())
}
```

- [ ] **Step 5: Add to workspace.** In the root `Cargo.toml`'s `members`
  list, add `"crates/poller-irish-rail-gtfs",` alongside the existing
  poller entries.

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p poller-irish-rail-gtfs --all-features
cargo test -p poller-irish-rail-gtfs
cargo build --workspace
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/poller-irish-rail-gtfs
git commit -m "add poller-irish-rail-gtfs: GTFS-backed Iarnrod Eireann station/line catalogue ingestion"
```

---

## Task A5: Docker + CI + Helm chart wiring

**Files:** create `docker/poller-irish-rail-gtfs.Dockerfile`, create
`charts/distant-signal/templates/poller-irish-rail-gtfs-deployment.yaml`;
modify `.github/workflows/containers.yml`, `charts/distant-signal/values.yaml`,
`charts/distant-signal/values-example.yaml`, `charts/distant-signal/templates/podmonitor.yaml`,
`charts/distant-signal/templates/secret.yaml`, `charts/distant-signal/templates/api-deployment.yaml`.

Depends on Task A4 (the crate/binary must exist for the Dockerfile to
build). Independent of Task A6.

- [ ] **Step 1: Dockerfile**, copied from `docker/poller-stations.Dockerfile`
  with the binary name substituted throughout (`poller-stations` →
  `poller-irish-rail-gtfs`), same rustc 1.88 pin, same cache-mount/copy-out
  pattern, same non-root numeric-UID runtime user:

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for the `poller-irish-rail-gtfs` service.
#
# Same rustc 1.88 floor as every other crate in this workspace -- see
# docker/poller-stations.Dockerfile's own comment for the confirmed
# icu_provider transitive-dependency reasoning.
#
# Build from the repo root:
#   docker build -f docker/poller-irish-rail-gtfs.Dockerfile .
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin poller-irish-rail-gtfs; \
    else \
      cargo build --bin poller-irish-rail-gtfs; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/poller-irish-rail-gtfs /usr/local/bin/poller-irish-rail-gtfs

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1000 poller \
    && useradd --system --no-create-home --shell /usr/sbin/nologin --uid 1000 --gid 1000 poller

COPY --from=builder /usr/local/bin/poller-irish-rail-gtfs /usr/local/bin/poller-irish-rail-gtfs

USER 1000:1000

ENTRYPOINT ["/usr/local/bin/poller-irish-rail-gtfs"]
```

- [ ] **Step 2: CI matrix.** In `.github/workflows/containers.yml`, add
  alongside the existing `poller-*` entries (`:124-135`):

```yaml
          - service: poller-irish-rail-gtfs
            dockerfile: docker/poller-irish-rail-gtfs.Dockerfile
            target: ""
```

- [ ] **Step 3: Helm values.** In `charts/distant-signal/values.yaml`, add a
  new top-level block, sibling to `movementRelay:` (`:962`), not nested
  inside `pollers:` (Judgment Call #2):

```yaml
# ---------------------------------------------------------------------------
# pollerIrishRailGtfs (crates/poller-irish-rail-gtfs/src/config.rs: Config)
# ---------------------------------------------------------------------------
pollerIrishRailGtfs:
  enabled: false
  image:
    repository: distant-signal/poller-irish-rail-gtfs
    tag: ""
    pullPolicy: IfNotPresent
  # -- Transport for Ireland's public GTFS zip for Iarnrod Eireann -- real,
  # key-free, confirmed reachable directly
  # (docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md
  # section 1), unlike every RDM poller's own baseUrl. Overridable in case
  # the feed ever moves.
  gtfsUrl: "https://www.transportforireland.ie/transitData/Data/GTFS_Irish_Rail.zip"
  apiStationsIngestPath: /private/island-of-ireland-stations
  apiLinesIngestPath: /private/island-of-ireland-lines
  # -- No confirmed real refresh cadence for this feed; defaults to the
  # same 24h convention pollers.stations/pollers.tocs already use for
  # reference data with an unconfirmed cadence.
  pollIntervalSecs: 86400
  # -- No API key needed at all (unlike every RDM poller) -- see this
  # chart's own Judgment Call #2 in
  # docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md.
  internalOauthUsername: ""
  internalOauthPassword: ""
  existingSecret: ""
  existingSecretInternalOauthUsernameKey: internal-oauth-username-poller-irish-rail-gtfs
  existingSecretInternalOauthPasswordKey: internal-oauth-password-poller-irish-rail-gtfs
  logLevel: info
  metricsPort: 9091
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

  Also add the two new group names to `api.internalOauth.groups`
  (`:432-441`):

```yaml
      irishRailGtfs: svc-poller-irish-rail-gtfs
```

  (Group B's Task B5 adds a second `irishRailLive` line to this same
  block.)

- [ ] **Step 4: New Deployment template**, modeled on
  `charts/distant-signal/templates/movement-relay-deployment.yaml`'s
  standalone shape but simpler (no health-check port — this crate exposes
  no HTTP surface at all beyond an optional `/metrics` listener, same as
  every RDM poller):

```yaml
{{- if .Values.pollerIrishRailGtfs.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-poller-irish-rail-gtfs" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "poller-irish-rail-gtfs") | nindent 4 }}
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "poller-irish-rail-gtfs") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "poller-irish-rail-gtfs") | nindent 8 }}
      {{- if or .Values.metrics.enabled .Values.pollerIrishRailGtfs.podAnnotations }}
      annotations:
        {{- if .Values.metrics.enabled }}
        prometheus.io/scrape: "true"
        prometheus.io/port: {{ .Values.pollerIrishRailGtfs.metricsPort | quote }}
        prometheus.io/path: "/metrics"
        {{- end }}
        {{- with .Values.pollerIrishRailGtfs.podAnnotations }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "distant-signal.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "distant-signal.podSecurityContext" (dict "override" .Values.pollerIrishRailGtfs.podSecurityContext) | nindent 8 }}
      containers:
        - name: poller-irish-rail-gtfs
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.pollerIrishRailGtfs.image) | quote }}
          imagePullPolicy: {{ .Values.pollerIrishRailGtfs.image.pullPolicy }}
          securityContext:
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          {{- if .Values.metrics.enabled }}
          ports:
            - name: metrics
              containerPort: {{ .Values.pollerIrishRailGtfs.metricsPort }}
              protocol: TCP
          {{- end }}
          env:
            - name: GTFS_URL
              value: {{ .Values.pollerIrishRailGtfs.gtfsUrl | quote }}
            - name: API_STATIONS_INGEST_URL
              value: {{ printf "%s%s" (include "distant-signal.apiBaseUrl" .) .Values.pollerIrishRailGtfs.apiStationsIngestPath | quote }}
            - name: API_LINES_INGEST_URL
              value: {{ printf "%s%s" (include "distant-signal.apiBaseUrl" .) .Values.pollerIrishRailGtfs.apiLinesIngestPath | quote }}
            - name: INTERNAL_OAUTH_TOKEN_URL
              value: {{ .Values.internalOauth.tokenUrl | quote }}
            - name: INTERNAL_OAUTH_CLIENT_ID
              value: {{ .Values.internalOauth.clientId | quote }}
            - name: INTERNAL_OAUTH_SCOPE
              value: {{ .Values.internalOauth.scope | quote }}
            - name: INTERNAL_OAUTH_USERNAME
              valueFrom:
                secretKeyRef:
                  name: {{ .Values.pollerIrishRailGtfs.existingSecret | default (include "distant-signal.fullname" .) }}
                  key: {{ .Values.pollerIrishRailGtfs.existingSecretInternalOauthUsernameKey }}
            - name: INTERNAL_OAUTH_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ .Values.pollerIrishRailGtfs.existingSecret | default (include "distant-signal.fullname" .) }}
                  key: {{ .Values.pollerIrishRailGtfs.existingSecretInternalOauthPasswordKey }}
            - name: POLL_INTERVAL_SECS
              value: {{ .Values.pollerIrishRailGtfs.pollIntervalSecs | quote }}
            - name: METRICS_ENABLED
              value: {{ .Values.metrics.enabled | quote }}
            {{- if .Values.metrics.enabled }}
            - name: METRICS_PORT
              value: {{ .Values.pollerIrishRailGtfs.metricsPort | quote }}
            {{- end }}
            - name: RUST_LOG
              value: {{ .Values.pollerIrishRailGtfs.logLevel | quote }}
            {{- with .Values.pollerIrishRailGtfs.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.pollerIrishRailGtfs.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.pollerIrishRailGtfs.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.pollerIrishRailGtfs.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.pollerIrishRailGtfs.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

  (Confirm the exact secret-name-fallback expression used above — `{{
  .Values.pollerIrishRailGtfs.existingSecret | default (include
  "distant-signal.fullname" .) }}` — against how `poller-deployments.yaml`
  actually resolves `distant-signal.pollerSecretName` (its own named
  template, `_helpers.tpl`) before implementing; use that exact helper
  instead of hand-rolling the fallback if one already exists generically
  enough to accept this poller's own values shape, to avoid duplicating
  logic `_helpers.tpl` already owns.)

- [ ] **Step 5: `podmonitor.yaml` selector.** Add to the
  `matchExpressions` values list (`:52-70`), guarded like `movement-relay`:

```yaml
          {{- if .Values.pollerIrishRailGtfs.enabled }}
          - poller-irish-rail-gtfs
          {{- end }}
```

- [ ] **Step 6: `secret.yaml`.** Add a new username/password pair, mirroring
  the per-poller pattern at `:43-44`:

```yaml
{{- $_ := set $data "internal-oauth-username-poller-irish-rail-gtfs" (.Values.pollerIrishRailGtfs.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-poller-irish-rail-gtfs" (.Values.pollerIrishRailGtfs.internalOauthPassword | default "" | b64enc) -}}
```

- [ ] **Step 7: `api-deployment.yaml`.** Add the new group env var,
  alongside the existing block (`:108-125`):

```yaml
            - name: INTERNAL_OAUTH_GROUP_IRISH_RAIL_GTFS
              value: {{ .Values.api.internalOauth.groups.irishRailGtfs | quote }}
```

- [ ] **Step 8: `values-example.yaml`.** Add a filled-in
  `pollerIrishRailGtfs: enabled: true` block plus internal-oauth
  username/password, mirroring how that file already fills in every other
  optional toggle (see the Grafana plan's Task 2 for the identical
  "exercise the `enabled: true` render path in CI" reasoning):

```yaml
pollerIrishRailGtfs:
  enabled: true
  internalOauthUsername: poller-irish-rail-gtfs
  internalOauthPassword: dummy-password
```

- [ ] **Step 9: Verify**

```bash
docker build -f docker/poller-irish-rail-gtfs.Dockerfile . -t poller-irish-rail-gtfs:test

helm lint charts/distant-signal
helm lint charts/distant-signal -f charts/distant-signal/values-example.yaml

# enabled: false (default) -- confirm the new Deployment/PodMonitor entry
# do not render at all:
helm template distant-signal charts/distant-signal \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 --set trustConsumer.kafka.topic=t \
  --set trustConsumer.kafka.saslMechanism=PLAIN --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=m --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=c --set api.sso.clientSecret=s \
  --set api.sso.redirectUrl=https://app.example.com/callback --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  | grep -c "poller-irish-rail-gtfs"   # expect 0

# enabled: true, via values-example.yaml:
helm template distant-signal charts/distant-signal -f charts/distant-signal/values-example.yaml \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 --set trustConsumer.kafka.topic=t \
  --set trustConsumer.kafka.saslMechanism=PLAIN --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=m --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=c --set api.sso.clientSecret=s \
  --set api.sso.redirectUrl=https://app.example.com/callback --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  | grep -c "poller-irish-rail-gtfs"   # expect > 0, includes the Deployment and the PodMonitor selector entry
```

- [ ] **Step 10: Commit**

```bash
git add docker/poller-irish-rail-gtfs.Dockerfile .github/workflows/containers.yml \
        charts/distant-signal/values.yaml charts/distant-signal/values-example.yaml \
        charts/distant-signal/templates/podmonitor.yaml charts/distant-signal/templates/secret.yaml \
        charts/distant-signal/templates/api-deployment.yaml \
        charts/distant-signal/templates/poller-irish-rail-gtfs-deployment.yaml
git commit -m "chart+ci: wire up poller-irish-rail-gtfs (Docker, CI matrix, Helm Deployment/PodMonitor/secret)"
```

---

## Task A6: End-to-end verification (Group A)

**Files:** none (verification only).

- [ ] **Step 1: Full workspace check**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features
cargo build --workspace
cargo test --workspace
cargo test -p api island_of_ireland -- --ignored --test-threads=1
```

- [ ] **Step 2: Local end-to-end smoke test.** With a local Postgres
  migrated and `api` running (this repo's existing `docker-compose.dev.yml`
  workflow, unchanged by this plan):

```bash
DATABASE_URL=... RDM_STATIONS_BASE_URL=... \
  cargo run -p poller-irish-rail-gtfs -- \
  --internal-oauth-token-url ... --internal-oauth-client-id ... \
  --internal-oauth-username ... --internal-oauth-password ... \
  --poll-interval-secs 999999   # single-shot-ish: interval fires once immediately (fresh DB => zero delay), then this process can be Ctrl-C'd

curl -s http://localhost:8080/public/island-of-ireland/stations | jq 'length'
# expect ~152, matching the friction doc's own stops.txt row count
curl -s "http://localhost:8080/public/island-of-ireland/stations?network=republic-of-ireland" | jq '.[0]'
curl -s http://localhost:8080/public/island-of-ireland/lines | jq 'length'
# expect ~19 (18 named corridors, one split into -I/-O rows -- friction doc §1)
```

- [ ] **Step 3: Confirm the specific border-area stations landed**, per
  design spec §4's own naming:

```bash
curl -s http://localhost:8080/public/island-of-ireland/stations | jq '[.[] | select(.name | test("Belfast|Lisburn|Portadown|Lurgan|Newry"; "i"))]'
# expect non-empty -- these rows exist and are tagged republic-of-ireland,
# per design spec section 4's single-authoritative-source policy
```

No commit for this task (verification only) — if any step fails, fix the
relevant earlier task and re-verify before considering Group A complete.

---

# Group B — Iarnród Éireann Tier B (fast-follow)

## Task B1: `common::island_of_ireland` additions — departure/sample types

**Files:** modify `crates/common/src/island_of_ireland.rs`.

Depends on Task A1 (same file). Independent of every other Group A task —
Group B can start once `island_of_ireland.rs` exists, without waiting for
Tier A's crate/routes to be built or merged.

- [ ] **Step 1: Add the types**, appended to the module Task A1 created:

```rust
/// One service from an Iarnród Éireann live departure board
/// (`api.irishrail.ie/realtime/realtime.asmx/getStationDataByCodeXML`).
/// Field names/types below are the confirmed live schema, per the friction
/// doc (docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md
/// section 1): "Origin, Destination, Scharrival/Schdepart, Exparrival/Expdepart,
/// Late (minutes), Status, Duein -- a live per-service departure-board
/// record with an explicit delay-minutes field already computed."
/// Deliberately NOT `common::StationDeparture` -- that type's
/// `destination_crs: String` has no Irish-network-shaped value, mirroring
/// why `IslandOfIrelandStation` isn't `common::Station`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandOfIrelandDeparture {
    /// Iarnrod Eireann's own train identifier, e.g. "A101" -- the friction
    /// doc's own confirmed example (section 4, the Enterprise service).
    pub train_code: String,
    pub origin: String,
    pub destination: String,
    /// HH:MM scheduled times, carried as the upstream API's own string
    /// representation -- same posture `common::StationDeparture.scheduled`
    /// already takes for GB LDBWS times, not parsed into a `NaiveTime`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_arrival: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_departure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_arrival: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_departure: Option<String>,
    /// Minutes late -- the upstream API's own `Late` field, already
    /// computed server-side (unlike GB LDBWS, no client-side delay
    /// derivation is needed here).
    pub late_minutes: i32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_in_minutes: Option<i32>,
}

/// One live poll of one station's departure board.
/// `station_id` is `api.irishrail.ie`'s own `StationCode` (e.g. `"BFSTC"`)
/// -- NOT necessarily the same identifier as
/// `IslandOfIrelandStation.id` for the same physical station when that
/// station's `id` came from GTFS (Tier A). See
/// docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
/// Judgment Call #1 -- this is a real, unreconciled gap, not an oversight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandOfIrelandStationSample {
    pub station_id: String,
    pub network: IslandOfIrelandNetwork,
    pub polled_at: chrono::DateTime<chrono::Utc>,
    pub departures: Vec<IslandOfIrelandDeparture>,
}
```

  Add `use chrono` if not already imported in this file (Task A1's version
  doesn't need it; Task B1 does) — add `use chrono::{DateTime, Utc};` at the
  top and use bare `DateTime<Utc>` in the struct rather than the fully
  qualified path shown above, matching this crate's existing style
  elsewhere in `lib.rs`.

- [ ] **Step 2: Tests**

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;

    #[test]
    fn station_sample_round_trips_through_json() {
        let sample = IslandOfIrelandStationSample {
            station_id: "BFSTC".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            polled_at: "2026-09-05T06:00:00Z".parse().unwrap(),
            departures: vec![IslandOfIrelandDeparture {
                train_code: "A101".to_string(),
                origin: "Belfast".to_string(),
                destination: "Dublin Connolly".to_string(),
                scheduled_arrival: None,
                scheduled_departure: Some("06:00".to_string()),
                expected_arrival: None,
                expected_departure: Some("06:00".to_string()),
                late_minutes: 0,
                status: "On Time".to_string(),
                due_in_minutes: Some(5),
            }],
        };
        let json = serde_json::to_value(&sample).unwrap();
        let back: IslandOfIrelandStationSample = serde_json::from_value(json).unwrap();
        assert_eq!(back.departures[0].train_code, "A101");
    }
}
```

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p common --all-features
cargo test -p common
```

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/island_of_ireland.rs
git commit -m "common: add IslandOfIrelandDeparture/StationSample types for the live-departures tier"
```

---

## Task B2: Postgres schema + `api` data layer (station samples)

**Files:** create `crates/api/migrations/20260905130000_island_of_ireland_station_samples.sql`;
modify `crates/api/src/data/island_of_ireland.rs`.

Depends on Task B1. Independent of Group A's Tasks A2-A6 otherwise (this is
a separate table, no foreign key to `island_of_ireland_stations` — see
Judgment Call #1: the two tiers' station ids are not confirmed to match, so
no FK is asserted here that would fail or silently mismatch).

- [ ] **Step 1: Migration**, mirroring `station_samples`'s exact shape
  (`crates/api/migrations/20260510023522_initial.sql:52-56`) — upsert on
  the station's own id, no history:

```sql
-- Iarnrod Eireann live departure-board samples -- Tier B of
-- docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md. Same
-- shape as `station_samples` (20260510023522_initial.sql): one row per
-- station, replaced wholesale each poll cycle, no history table.
--
-- Deliberately NO foreign key to `island_of_ireland_stations.id` --
-- `station_id` here is api.irishrail.ie's own StationCode, which this
-- plan has not confirmed matches the GTFS-derived id Tier A stores. See
-- docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
-- Judgment Call #1.

CREATE TABLE island_of_ireland_station_samples (
    station_id  TEXT        PRIMARY KEY,
    network     TEXT        NOT NULL,
    polled_at   TIMESTAMPTZ NOT NULL,
    departures  JSONB       NOT NULL DEFAULT '[]'
);
```

- [ ] **Step 2: Data layer.** Append to `crates/api/src/data/island_of_ireland.rs`:

```rust
use common::island_of_ireland::{IslandOfIrelandDeparture, IslandOfIrelandStationSample};

pub async fn upsert_station_samples(
    pool: &PgPool,
    samples: &[IslandOfIrelandStationSample],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let departures_json = serde_json::to_value(&sample.departures)?;
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_station_samples (station_id, network, polled_at, departures)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (station_id) DO UPDATE SET
                network    = EXCLUDED.network,
                polled_at  = EXCLUDED.polled_at,
                departures = EXCLUDED.departures
            "#,
        )
        .bind(&sample.station_id)
        .bind(network_wire(sample.network))
        .bind(sample.polled_at)
        .bind(&departures_json)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

pub async fn last_station_samples_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (polled_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(polled_at) FROM island_of_ireland_station_samples")
            .fetch_one(pool)
            .await?;
    Ok(polled_at)
}

/// Backs `GET /public/island-of-ireland/stations/{id}/departures` (Task
/// B3) -- raw pass-through, mirrors `queries::latest_station_sample`
/// (`crates/api/src/data/queries.rs:918-934`) exactly, one level down in a
/// different table.
pub async fn latest_station_sample(
    pool: &PgPool,
    station_id: &str,
) -> Result<Option<IslandOfIrelandStationSample>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT station_id, network, polled_at, departures FROM island_of_ireland_station_samples \
         WHERE station_id = $1",
    )
    .bind(station_id)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        let network: String = row.try_get("network")?;
        let departures_json: serde_json::Value = row.try_get("departures")?;
        Ok(IslandOfIrelandStationSample {
            station_id: row.try_get("station_id")?,
            network: network_from_wire(&network)?,
            polled_at: row.try_get("polled_at")?,
            departures: serde_json::from_value::<Vec<IslandOfIrelandDeparture>>(departures_json)?,
        })
    })
    .transpose()
}
```

- [ ] **Step 3: DB-backed tests**, mirroring
  `crates/api/src/routes/departures.rs::db_tests`' fixture/seed/assert/delete
  shape:

```rust
#[cfg(test)]
mod sample_db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect")
    }

    async fn delete_fixture(pool: &PgPool, station_id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_station_samples WHERE station_id = $1")
            .bind(station_id)
            .execute(pool)
            .await
            .expect("cleanup fixture sample");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland_samples -- --ignored --test-threads=1`"]
    async fn upsert_then_latest_round_trips_and_repeat_upsert_replaces() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMP1").await;

        let first = IslandOfIrelandStationSample {
            station_id: "ZSAMP1".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            polled_at: chrono::Utc::now(),
            departures: vec![IslandOfIrelandDeparture {
                train_code: "Z1".to_string(),
                origin: "Zesttown".to_string(),
                destination: "Zorough".to_string(),
                scheduled_arrival: None,
                scheduled_departure: Some("10:00".to_string()),
                expected_arrival: None,
                expected_departure: Some("10:00".to_string()),
                late_minutes: 0,
                status: "On Time".to_string(),
                due_in_minutes: Some(3),
            }],
        };
        upsert_station_samples(&pool, &[first]).await.expect("first upsert");

        let fetched = latest_station_sample(&pool, "ZSAMP1")
            .await
            .expect("fetch")
            .expect("row present");
        assert_eq!(fetched.departures[0].train_code, "Z1");

        let second = IslandOfIrelandStationSample {
            station_id: "ZSAMP1".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            polled_at: chrono::Utc::now(),
            departures: vec![],
        };
        upsert_station_samples(&pool, &[second]).await.expect("second upsert");
        let fetched = latest_station_sample(&pool, "ZSAMP1")
            .await
            .expect("fetch")
            .expect("row still present");
        assert!(fetched.departures.is_empty());

        delete_fixture(&pool, "ZSAMP1").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland_samples -- --ignored --test-threads=1`"]
    async fn latest_for_an_unseen_station_is_none_not_an_error() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMPNONE").await;
        let fetched = latest_station_sample(&pool, "ZSAMPNONE").await.expect("query");
        assert_eq!(fetched, None);
    }
}
```

  (`IslandOfIrelandStationSample`/`IslandOfIrelandDeparture` need
  `PartialEq` derives added in Task B1 for the second test's
  `assert_eq!(fetched, None)` to compile against `Option<T>` — add
  `PartialEq` to both structs' and `IslandOfIrelandNetwork`'s derive lists
  in Task B1 if not already covered — `IslandOfIrelandNetwork` already
  derives it there.)

- [ ] **Step 4: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api island_of_ireland_samples -- --ignored --test-threads=1
```

- [ ] **Step 5: Commit**

```bash
git add crates/api/migrations/20260905130000_island_of_ireland_station_samples.sql \
        crates/api/src/data/island_of_ireland.rs
git commit -m "api: add island_of_ireland_station_samples table and data layer"
```

---

## Task B3: `api` routes — private ingest, public raw pass-through, auth wiring

**Files:** modify `crates/api/src/routes/ingest.rs`, `crates/api/src/routes/island_of_ireland.rs`,
`crates/api/src/data/config.rs`, `crates/api/src/app.rs`; possibly modify
`crates/api/src/render.rs`.

Depends on Task B2. Depends on Task A3 only in the sense that it edits the
same two files — sequence after A3 to avoid a merge conflict, not because
of a functional dependency.

- [ ] **Step 1: Private ingest route.** In `crates/api/src/routes/ingest.rs`:

```rust
        .route(
            "/island-of-ireland-station-samples",
            axum::routing::get(get_island_of_ireland_station_samples_last_fetched)
                .post(post_island_of_ireland_station_samples),
        )
```

```rust
async fn get_island_of_ireland_station_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_station_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_station_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<common::island_of_ireland::IslandOfIrelandStationSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_station_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 2: Public raw pass-through route.** In
  `crates/api/src/routes/island_of_ireland.rs`, add the departures route,
  mirroring `routes::departures::get_station_departures`'s exact
  404-vs-`200 []` honesty split (`:35-59`):

```rust
        .route(
            "/island-of-ireland/stations/{id}/departures",
            axum::routing::get(get_station_departures),
        )
```

```rust
async fn get_station_departures(
    State(app): State<App>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let Some(sample) = island_of_ireland::latest_station_sample(&app.database, &id)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for island-of-ireland station: {id}"),
        ));
    };

    Ok(Json(sample.departures.iter().map(departure_json).collect()))
}

/// Hand-built camelCase JSON, matching `render::station_departure_json`'s
/// established convention for a public departures endpoint even though
/// this route's own producer (Task B4) writes snake_case internally.
fn departure_json(d: &common::island_of_ireland::IslandOfIrelandDeparture) -> serde_json::Value {
    serde_json::json!({
        "trainCode": d.train_code,
        "origin": d.origin,
        "destination": d.destination,
        "scheduledArrival": d.scheduled_arrival,
        "scheduledDeparture": d.scheduled_departure,
        "expectedArrival": d.expected_arrival,
        "expectedDeparture": d.expected_departure,
        "lateMinutes": d.late_minutes,
        "status": d.status,
        "dueInMinutes": d.due_in_minutes,
    })
}
```

- [ ] **Step 3: New internal-OAuth group.** In `crates/api/src/data/config.rs`:

```rust
    /// Gates `POST`/`GET /private/island-of-ireland-station-samples` -- the
    /// new `poller-irish-rail-live` crate's own credential.
    #[arg(long, env, default_value = "svc-poller-irish-rail-live")]
    pub internal_oauth_group_irish_rail_live: String,
```

- [ ] **Step 4: Wire it into `build_internal_oauth_routes`** in
  `crates/api/src/app.rs`:

```rust
        (
            "/island-of-ireland-station-samples",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_live.clone()],
        ),
        (
            "/island-of-ireland-station-samples",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_live.clone()],
        ),
```

  Same fixture-literal update as Task A3 Step 4, applied a second time for
  this new field.

- [ ] **Step 5: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/ingest.rs crates/api/src/routes/island_of_ireland.rs \
        crates/api/src/data/config.rs crates/api/src/app.rs
git commit -m "api: add private ingest + public raw departures route for island-of-ireland station samples"
```

---

## Task B4: `poller-irish-rail-live` crate

**Files:** create `crates/poller-irish-rail-live/Cargo.toml`,
`crates/poller-irish-rail-live/src/{main.rs,config.rs,schema.rs}`; modify
`Cargo.toml` (workspace members).

Depends on Tasks B1/B3.

- [ ] **Step 1: Cargo.toml**, modeled on `crates/poller-incidents/Cargo.toml`'s
  exact dependency set (the workspace's own established XML-parsing
  precedent — `quick-xml` with the `serialize` feature) plus `chrono` for
  `polled_at`:

```toml
[package]
name = "poller-irish-rail-live"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
metrics = "0.24"
quick-xml = { version = "0.41.0", features = ["serialize"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

- [ ] **Step 2: `config.rs`**:

```rust
use clap::Parser;

/// CLI/env configuration for the `poller-irish-rail-live` service.
///
/// Unlike `poller-irish-rail-gtfs`, this crate needs no `api`-hosted
/// station list at all -- it discovers its own station codes from
/// `api.irishrail.ie`'s own `getAllStationsXML` call each cycle. See
/// docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
/// Judgment Call #1 for why this is deliberate, not a missed
/// simplification.
#[derive(Debug, Parser)]
pub struct Config {
    /// `api.irishrail.ie`'s legacy realtime ASMX service root -- real,
    /// key-free, confirmed reachable directly (friction doc section 1).
    #[arg(long, env, default_value = "http://api.irishrail.ie/realtime/realtime.asmx")]
    pub irish_rail_base_url: String,

    /// The `api` crate's ingestion endpoint for station samples.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/island-of-ireland-station-samples"
    )]
    pub api_ingest_url: String,

    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    /// Conservative default (5 minutes), NOT `poller-ldbws`'s 60s, despite
    /// the structural similarity -- `api.irishrail.ie`'s real rate limits
    /// and operational durability are unconfirmed (design spec section 8,
    /// open question 4: "apparently unmaintained since ~2012... whether it
    /// has any informal rate limits... are all unconfirmed"), and this
    /// crate polls EVERY station returned by `getAllStationsXML` each
    /// cycle (up to ~171 per the friction doc, not a curated subset like
    /// GB LDBWS's `sample_stations`, since Tier B has no line-catalogue
    /// coupling to curate against -- see this plan's Judgment Call #3).
    /// 300s bounds the total request volume against an unconfirmed legacy
    /// API more conservatively than GB LDBWS's own 60s does against a
    /// modern, documented, actively-supported one.
    #[arg(long, env, default_value_t = 300)]
    pub poll_interval_secs: u64,

    /// Optional comma-separated allowlist of station codes to poll,
    /// bypassing `getAllStationsXML`'s own full list -- an operator escape
    /// hatch if polling all ~171 stations every cycle turns out to be too
    /// aggressive against this unconfirmed-capacity upstream. Empty (the
    /// default) means "poll everything `getAllStationsXML` returns."
    #[arg(long, env, value_delimiter = ',', default_value = "")]
    pub station_codes_override: Vec<String>,

    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

- [ ] **Step 3: `schema.rs`** — XML shapes for `getAllStationsXML` and
  `getStationDataByCodeXML`, field names transcribed from the friction
  doc's own confirmed live-fetch results (§1, §4), same
  `#[serde(rename_all = "PascalCase")]` convention `poller-incidents/src/schema.rs`
  already established for this workspace's other ASP.NET-style XML feed:

```rust
//! `api.irishrail.ie` legacy realtime XML schema and its mapping to
//! `common::island_of_ireland::{IslandOfIrelandDeparture, IslandOfIrelandStationSample}`.
//!
//! Field names transcribed from the friction doc's own live-fetch results
//! (docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md
//! sections 1 and 4), not invented: `Origin`, `Destination`, `Scharrival`,
//! `Schdepart`, `Exparrival`, `Expdepart`, `Late`, `Status`, `Duein`,
//! `Traincode` for a station's departure board; `StationCode`,
//! `StationDesc`, `StationLatitude`, `StationLongitude` for the master
//! station list (`getAllStationsXML`). PascalCase, matching this
//! workspace's other ASP.NET-style XML feed
//! (`crates/poller-incidents/src/schema.rs`'s own `PtIncident`
//! convention) -- both are legacy .NET web services.
//!
//! GAP, same posture `poller-stations/src/schema.rs` documents for its own
//! unconfirmed casing: the exact XML element names above are this
//! session's best transcription from the friction doc's prose description
//! of a live response, not a byte-for-byte XML sample. If a live poll ever
//! fails to deserialize, `RUST_LOG=poller_irish_rail_live=debug` (Task B4's
//! `main.rs` logs the raw body before parsing, mirroring
//! `poller-stations/src/main.rs`'s own `fetch_stations_json`) is the
//! mechanism for correcting a field name here.

use anyhow::Result;
use common::island_of_ireland::{IslandOfIrelandDeparture, IslandOfIrelandNetwork, IslandOfIrelandStationSample};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArrayOfObjStationData {
    #[serde(default, rename = "objStationData")]
    pub station_data: Vec<ObjStationData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ObjStationData {
    pub traincode: String,
    pub origin: String,
    pub destination: String,
    #[serde(default)]
    pub scharrival: Option<String>,
    #[serde(default)]
    pub schdepart: Option<String>,
    #[serde(default)]
    pub exparrival: Option<String>,
    #[serde(default)]
    pub expdepart: Option<String>,
    pub late: i32,
    pub status: String,
    #[serde(default)]
    pub duein: Option<String>,
}

impl From<&ObjStationData> for IslandOfIrelandDeparture {
    fn from(d: &ObjStationData) -> Self {
        IslandOfIrelandDeparture {
            train_code: d.traincode.clone(),
            origin: d.origin.clone(),
            destination: d.destination.clone(),
            scheduled_arrival: d.scharrival.clone(),
            scheduled_departure: d.schdepart.clone(),
            expected_arrival: d.exparrival.clone(),
            expected_departure: d.expdepart.clone(),
            late_minutes: d.late,
            status: d.status.clone(),
            due_in_minutes: d.duein.as_deref().and_then(|s| s.parse().ok()),
        }
    }
}

pub fn parse_station_departures(xml: &str) -> Result<Vec<IslandOfIrelandDeparture>> {
    let response: ArrayOfObjStationData = quick_xml::de::from_str(xml)?;
    Ok(response.station_data.iter().map(IslandOfIrelandDeparture::from).collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArrayOfObjStation {
    #[serde(default, rename = "objStation")]
    pub station: Vec<ObjStation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ObjStation {
    pub station_code: String,
    #[serde(default)]
    pub station_desc: Option<String>,
}

pub fn parse_all_stations(xml: &str) -> Result<Vec<String>> {
    let response: ArrayOfObjStation = quick_xml::de::from_str(xml)?;
    Ok(response.station.into_iter().map(|s| s.station_code).collect())
}

pub fn to_sample(station_id: &str, departures: Vec<IslandOfIrelandDeparture>) -> IslandOfIrelandStationSample {
    IslandOfIrelandStationSample {
        station_id: station_id.to_string(),
        network: IslandOfIrelandNetwork::RepublicOfIreland,
        polled_at: chrono::Utc::now(),
        departures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-written sample following the friction doc's own confirmed field
    /// list (section 4's Enterprise/BFSTC example) -- exact XML tag casing
    /// is this test's own best-effort transcription (see module docs' GAP
    /// note above), not a byte-for-byte upstream capture.
    const SAMPLE_STATION_DATA_XML: &str = r#"
        <ArrayOfObjStationData>
            <objStationData>
                <Traincode>A101</Traincode>
                <Origin>Belfast</Origin>
                <Destination>Dublin Connolly</Destination>
                <Scharrival></Scharrival>
                <Schdepart>06:00</Schdepart>
                <Exparrival></Exparrival>
                <Expdepart>06:00</Expdepart>
                <Late>0</Late>
                <Status>On Time</Status>
                <Duein>5</Duein>
            </objStationData>
        </ArrayOfObjStationData>
    "#;

    #[test]
    fn parses_sample_station_data_and_maps_every_field() {
        let departures = parse_station_departures(SAMPLE_STATION_DATA_XML).expect("sample XML should parse");
        assert_eq!(departures.len(), 1);
        assert_eq!(departures[0].train_code, "A101");
        assert_eq!(departures[0].origin, "Belfast");
        assert_eq!(departures[0].destination, "Dublin Connolly");
        assert_eq!(departures[0].scheduled_departure, Some("06:00".to_string()));
        assert_eq!(departures[0].late_minutes, 0);
        assert_eq!(departures[0].due_in_minutes, Some(5));
    }

    const SAMPLE_ALL_STATIONS_XML: &str = r#"
        <ArrayOfObjStation>
            <objStation>
                <StationCode>BFSTC</StationCode>
                <StationDesc>Belfast</StationDesc>
            </objStation>
            <objStation>
                <StationCode>DCNLL</StationCode>
                <StationDesc>Dublin Connolly</StationDesc>
            </objStation>
        </ArrayOfObjStation>
    "#;

    #[test]
    fn parses_all_stations_into_a_code_list() {
        let codes = parse_all_stations(SAMPLE_ALL_STATIONS_XML).expect("sample XML should parse");
        assert_eq!(codes, vec!["BFSTC".to_string(), "DCNLL".to_string()]);
    }
}
```

- [ ] **Step 4: `main.rs`**, modeled on `crates/poller-ldbws/src/main.rs`'s
  per-station-loop shape, without its `numRows` retry logic (not applicable
  — `api.irishrail.ie` has no analogous parameter):

```rust
//! `poller-irish-rail-live`: polls `api.irishrail.ie`'s legacy realtime XML
//! service for every station it lists, and forwards raw per-station
//! departure-board samples to `api`'s
//! `/private/island-of-ireland-station-samples` endpoint. Tier B of
//! docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md; see
//! docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md Task B4.
//! Deliberately raw ingestion only -- no severity inference, no
//! aggregator involvement (Judgment Call #3 there).

mod config;
mod schema;

use std::time::Duration;

use clap::Parser;
use common::ingest;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let delay = ingest::time_until_next_poll(&client, &config.api_ingest_url, &internal_oauth, poll_interval).await;
    if !delay.is_zero() {
        tracing::info!(delay_secs = delay.as_secs(), "data still fresh from a prior run; delaying first poll");
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);

    loop {
        interval.tick().await;

        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &internal_oauth).await;
        metrics::histogram!(
            common::metrics::metric_name("poller_cycle_duration_seconds"),
            "poller" => "irish-rail-live"
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            common::metrics::metric_name("poller_cycle_total"),
            "poller" => "irish-rail-live",
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<()> {
    let station_codes = if config.station_codes_override.is_empty() {
        fetch_all_station_codes(client, config).await?
    } else {
        config.station_codes_override.clone()
    };
    tracing::info!(count = station_codes.len(), "fetched station code list to sample");

    let mut samples = Vec::with_capacity(station_codes.len());
    for code in &station_codes {
        match fetch_station_departures(client, config, code).await {
            Ok(departures) => samples.push(schema::to_sample(code, departures)),
            Err(err) => {
                tracing::error!(station_code = %code, error = ?err, "failed to sample station; skipping");
            }
        }
    }

    if samples.is_empty() {
        tracing::warn!("no station samples collected this cycle; nothing to post");
        return Ok(());
    }

    ingest::post_batch(client, &config.api_ingest_url, internal_oauth, &samples, "island-of-ireland station samples").await
}

async fn fetch_all_station_codes(client: &Client, config: &Config) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/getAllStationsXML", config.irish_rail_base_url);
    let body = client.get(&url).send().await?.error_for_status()?.text().await?;
    tracing::debug!(body = %body, "raw getAllStationsXML response body");
    schema::parse_all_stations(&body)
}

async fn fetch_station_departures(
    client: &Client,
    config: &Config,
    station_code: &str,
) -> anyhow::Result<Vec<common::island_of_ireland::IslandOfIrelandDeparture>> {
    let url = format!(
        "{}/getStationDataByCodeXML?StationCode={station_code}",
        config.irish_rail_base_url
    );
    let body = client.get(&url).send().await?.error_for_status()?.text().await?;
    tracing::debug!(station_code = %station_code, body = %body, "raw getStationDataByCodeXML response body");
    schema::parse_station_departures(&body)
}
```

- [ ] **Step 5: Add to workspace.** Add
  `"crates/poller-irish-rail-live",` to the root `Cargo.toml`'s `members`
  list.

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p poller-irish-rail-live --all-features
cargo test -p poller-irish-rail-live
cargo build --workspace
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/poller-irish-rail-live
git commit -m "add poller-irish-rail-live: raw live-departures ingestion from api.irishrail.ie"
```

---

## Task B5: Docker + CI + Helm chart wiring (Tier B)

**Files:** create `docker/poller-irish-rail-live.Dockerfile`, create
`charts/distant-signal/templates/poller-irish-rail-live-deployment.yaml`;
modify `.github/workflows/containers.yml`, `charts/distant-signal/values.yaml`,
`charts/distant-signal/values-example.yaml`, `charts/distant-signal/templates/podmonitor.yaml`,
`charts/distant-signal/templates/secret.yaml`, `charts/distant-signal/templates/api-deployment.yaml`.

Exact mechanical mirror of Task A5, once per file, for
`poller-irish-rail-live` instead of `poller-irish-rail-gtfs`. Depends on
Task B4.

- [ ] **Step 1: Dockerfile** — copy Task A5 Step 1's Dockerfile verbatim,
  substituting `poller-irish-rail-live` for `poller-irish-rail-gtfs`
  throughout.

- [ ] **Step 2: CI matrix** — add to `.github/workflows/containers.yml`:

```yaml
          - service: poller-irish-rail-live
            dockerfile: docker/poller-irish-rail-live.Dockerfile
            target: ""
```

- [ ] **Step 3: Helm values** — new top-level block:

```yaml
# ---------------------------------------------------------------------------
# pollerIrishRailLive (crates/poller-irish-rail-live/src/config.rs: Config)
# ---------------------------------------------------------------------------
pollerIrishRailLive:
  enabled: false
  image:
    repository: distant-signal/poller-irish-rail-live
    tag: ""
    pullPolicy: IfNotPresent
  irishRailBaseUrl: "http://api.irishrail.ie/realtime/realtime.asmx"
  apiIngestPath: /private/island-of-ireland-station-samples
  # -- Conservative default: api.irishrail.ie's real rate limits/uptime are
  # unconfirmed (design spec section 8, open question 4), and this poller
  # samples every station getAllStationsXML returns (no line-catalogue
  # curation like GB LDBWS's sample_stations -- see the plan's Judgment
  # Call #3), unlike pollers.ldbws's 60s default against a modern,
  # documented API.
  pollIntervalSecs: 300
  # -- Comma-separated station-code allowlist; empty means "poll
  # everything getAllStationsXML returns."
  stationCodesOverride: ""
  internalOauthUsername: ""
  internalOauthPassword: ""
  existingSecret: ""
  existingSecretInternalOauthUsernameKey: internal-oauth-username-poller-irish-rail-live
  existingSecretInternalOauthPasswordKey: internal-oauth-password-poller-irish-rail-live
  logLevel: info
  metricsPort: 9091
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

  Add the second group name to `api.internalOauth.groups`:

```yaml
      irishRailLive: svc-poller-irish-rail-live
```

- [ ] **Step 4: New Deployment template** — same shape as Task A5 Step 4,
  substituting the component name/env vars (`IRISH_RAIL_BASE_URL`,
  `API_INGEST_URL`, `STATION_CODES_OVERRIDE` — only rendered `{{- if
  .Values.pollerIrishRailLive.stationCodesOverride }}` — and no
  lines/`apiStationsIngestPath`/`apiLinesIngestPath` split, since this
  crate has exactly one ingest target).

- [ ] **Step 5: `podmonitor.yaml` selector** — add, guarded:

```yaml
          {{- if .Values.pollerIrishRailLive.enabled }}
          - poller-irish-rail-live
          {{- end }}
```

- [ ] **Step 6: `secret.yaml`** — add the username/password pair:

```yaml
{{- $_ := set $data "internal-oauth-username-poller-irish-rail-live" (.Values.pollerIrishRailLive.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-poller-irish-rail-live" (.Values.pollerIrishRailLive.internalOauthPassword | default "" | b64enc) -}}
```

- [ ] **Step 7: `api-deployment.yaml`** — add the group env var:

```yaml
            - name: INTERNAL_OAUTH_GROUP_IRISH_RAIL_LIVE
              value: {{ .Values.api.internalOauth.groups.irishRailLive | quote }}
```

- [ ] **Step 8: `values-example.yaml`**:

```yaml
pollerIrishRailLive:
  enabled: true
  internalOauthUsername: poller-irish-rail-live
  internalOauthPassword: dummy-password
```

- [ ] **Step 9: Verify** — same `helm lint`/`helm template` pair as Task A5
  Step 9, `grep`-ing for `poller-irish-rail-live` instead.

- [ ] **Step 10: Commit**

```bash
git add docker/poller-irish-rail-live.Dockerfile .github/workflows/containers.yml \
        charts/distant-signal/values.yaml charts/distant-signal/values-example.yaml \
        charts/distant-signal/templates/podmonitor.yaml charts/distant-signal/templates/secret.yaml \
        charts/distant-signal/templates/api-deployment.yaml \
        charts/distant-signal/templates/poller-irish-rail-live-deployment.yaml
git commit -m "chart+ci: wire up poller-irish-rail-live (Docker, CI matrix, Helm Deployment/PodMonitor/secret)"
```

---

## Task B6: End-to-end verification (Group B)

**Files:** none (verification only).

- [ ] **Step 1: Full workspace check**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features
cargo build --workspace
cargo test --workspace
cargo test -p api island_of_ireland -- --ignored --test-threads=1
cargo test -p api island_of_ireland_samples -- --ignored --test-threads=1
```

- [ ] **Step 2: Local end-to-end smoke test**, against the real
  `api.irishrail.ie` (unauthenticated, per the friction doc — no fixture
  needed):

```bash
cargo run -p poller-irish-rail-live -- \
  --internal-oauth-token-url ... --internal-oauth-client-id ... \
  --internal-oauth-username ... --internal-oauth-password ... \
  --station-codes-override BFSTC,DCNLL

curl -s http://localhost:8080/public/island-of-ireland/stations/BFSTC/departures | jq '.'
# expect a non-404 response; an empty array is a legitimate "no scheduled
# service right now" result, not a failure
curl -s http://localhost:8080/public/island-of-ireland/stations/ZNOPE/departures -w '%{http_code}\n'
# expect 404 -- honesty split matches routes::departures::get_station_departures
```

No commit for this task (verification only).

---

## Self-review

- **Spec coverage.** Design spec §5's Tier A recommendation ("GTFS-parse
  ... into `IslandOfIrelandStation`/`IslandOfIrelandLineDefinition` rows
  tagged `RepublicOfIreland`, including the five border-area stations")
  is Tasks A1–A6 in full. §5's Tier B recommendation ("A
  `poller-ldbws`-shaped service per station, feeding... delay-threshold
  inference," qualified by §6's own hedge on whether that inference
  actually generalizes yet) is Tasks B1–B6, deliberately stopping at raw
  ingestion per Judgment Call #3 — the inference half is explicitly not
  promised by this plan, matching the design spec's own hedge rather than
  overclaiming it. §3's data-model decision is Task A1/B1 verbatim. §4's
  border-overlap policy needs no implementation task of its own — it's a
  non-filtering decision, verified in Task A6 Step 3. §7's go/no-go (go on
  ROI Tier A/B, nothing on NIR) is this plan's entire scope. No NIR/Tier C
  work appears anywhere, matching §7's "no-go" for both.
- **Placeholder scan.** No "TBD"/"handle appropriately"/"similar to Task
  N" language anywhere above; every code block is real, complete Rust/SQL/
  YAML, not a sketch.
- **Type consistency.** `IslandOfIrelandStation`/`LineDefinition` (Task A1)
  are the exact types Task A2's data layer, Task A3's routes, and Task A4's
  `mapping.rs` all import and use identically. `IslandOfIrelandStationSample`/
  `Departure` (Task B1) flow the same way through Tasks B2/B3/B4. Every
  `network_wire`/`network_from_wire` pair (Task A2) is defined once and
  reused by Task B2's own additions to the same file, not redefined.

---

## Summary of new surface area

- **New crates:** `poller-irish-rail-gtfs` (Tier A), `poller-irish-rail-live`
  (Tier B).
- **New `common::` module:** `common::island_of_ireland` —
  `IslandOfIrelandNetwork`, `IslandOfIrelandStation`,
  `IslandOfIrelandLineDefinition` (Task A1); `IslandOfIrelandDeparture`,
  `IslandOfIrelandStationSample` (Task B1).
- **New Postgres tables:** `island_of_ireland_stations`,
  `island_of_ireland_lines` (Task A2); `island_of_ireland_station_samples`
  (Task B2).
- **New private routes:** `POST`/`GET /private/island-of-ireland-stations`,
  `/island-of-ireland-lines` (Task A3); `/island-of-ireland-station-samples`
  (Task B3).
- **New public routes:** `GET /public/island-of-ireland/stations`, `/lines`
  (Task A3); `/island-of-ireland/stations/{id}/departures` (Task B3) — all
  read-only, no frontend consumer, justified in Judgment Call #5 as
  verification plumbing.
