# Transit networks: Manchester Metrolink first

Design, 2026-10-09. Status: **accepted**, reconciled with DS on
2026-10-14 ([§0](#0-reconciliation-with-ds-2026-10-14)). The user's
decisions of 2026-10-09 are recorded as D1–D7 in
[§14](#14-decisions-2026-10-09), and their answers to the open questions
in [§16](#16-open-questions-for-the-user).
[The plan](../plans/2026-10-09-transit-networks-plan.md) is how to build
it, batch by batch.

This design adds networks that are neither National Rail nor TfL to
Distant Signal (DS). Manchester Metrolink is the first. It builds on the
ingest architecture
([spec](2026-10-06-ingest-architecture-design.md),
[plan](../plans/2026-10-06-ingest-architecture-plan.md)); "ingest D5" below
means that spec's decision D5, and "§" alone means a section of this
document.

## Contents

0. [Reconciliation with DS (2026-10-14)](#0-reconciliation-with-ds-2026-10-14)
1. [Summary](#1-summary)
2. [Scope](#2-scope)
3. [Sources and measurements](#3-sources-and-measurements)
4. [Choosing the timetable source](#4-choosing-the-timetable-source)
5. [Architecture](#5-architecture)
6. [The producer, `poller-transit-schedule`](#6-the-producer-poller-transit-schedule)
7. [Stream, schemas and budget](#7-stream-schemas-and-budget)
8. [Tables](#8-tables)
9. [The ingest-writer handlers](#9-the-ingest-writer-handlers)
10. [Disruptions and line status](#10-disruptions-and-line-status)
11. [API](#11-api)
12. [Frontend, notifications and attribution](#12-frontend-notifications-and-attribution)
13. [Chart, security and observability](#13-chart-security-and-observability)
14. [Decisions (2026-10-09)](#14-decisions-2026-10-09)
15. [Phases, later work and risks](#15-phases-later-work-and-risks)
16. [Open questions for the user](#16-open-questions-for-the-user)

## 0. Reconciliation with DS (2026-10-14)

The design was written on 2026-10-09 against main at 87340fa5 and parked.
DS changed a lot before it restarted. This section lists what changed and
where the spec now differs from its first draft. Where a later section
still disagrees with this one, this one wins.

**Ingest release B (phase 5) is complete** (docs/ingest-phase5-runbook.md).

- There are no `/private` routes. Every producer writes Postgres as its
  own role or XADDs to its stream, and the HTTP sinks are deleted. The
  producer here was stream-only from the start, so nothing changes in its
  shape (§5, §6).
- **The writer's `shadow` mode is gone.** `ingestWriter.streams.<x>`
  takes `off` or `apply`, and `shadow` fails at startup and at render.
  So the rollout (§15.1) has no shadow step. Its dark-launch guard is
  `api.transitNetworks: []`: the writer applies the stream, the tables
  fill, and nothing is public until the api switch. The writer's tests
  cover `apply` only.
- **Chart render checks.** An enabled producer whose stream is not
  `apply` fails the render (`distant-signal.requireStreamApplied`, as the IoI
  pollers). `ci/ingest-prereqs.yaml` is the CI render baseline, and
  `zz-ingest-sink-preflight.yaml` lists release prerequisites. Transit adds
  nothing to the preflight: it is off by default and needs no release
  prerequisite.
- **Rollouts.** Single-replica Postgres workers use `Recreate`, after the
  2026-10-10 rollout deadlock (runbook §0.5). The producer has no Postgres
  role, but it is a single-instance producer, so its Deployment uses
  `Recreate` from creation too. Two overlapping pods would both XADD a
  timetable. Creating it with `Recreate` avoids the Flux server-side-apply
  problem of switching an existing Deployment (runbook §1, "Switching a
  live Deployment to Recreate").
- **The writer needs its own role before it applies `line_status`.** The
  chart refuses `ingestWriter.streams.tfl: apply` unless the writer
  connects as `distant_signal_writer` (security review M4), because the
  row policy binds that role only. A transit stream at `apply` gets the
  same check, from batch 1, because the timetable handler already writes
  `ingest_freshness` under the writer's row policy.

**Redis ACLs are fully on in production**: per-client users,
`stage: narrow`, `defaultUser: "on-unshared"` (docs/redis-acl.md).

- **Every user line in `files/redis-users.acl.tpl` needs a key in the
  users Secret.** A missing `<user>-password` key stops the Redis pod
  from starting, and any change to the users file restarts Redis (the
  ConfigMap checksum). A Secret change restarts it too (Reloader). The
  first draft added `poller-transit-metrolink` as a `final` line. Merged
  that way, batch 1 would restart Redis on deploy, and Redis would not
  come back until Ranma-Config added the key. That breaks "batch 1 is
  safe to merge".
- **Fix: a new kind, `optional`.** It has `final` rights, but the chart
  renders the user (its ACL line and its password env in the Redis pod)
  only while `redis.acl.clients.<client>` is on. `<client>` is the user
  name in camelCase (`poller-transit-metrolink` →
  `pollerTransitMetrolink`). So merging changes nothing that renders.
  Turning the client on needs the Secret key first and restarts Redis
  once, about 4 s of AOF load with the consumers retrying, as any Redis
  restart. Ranma adds the key and flips the client in one Ranma-Config
  change. Reloader and the ConfigMap checksum may then roll Redis twice in
  quick succession. `scripts/render-redis-acl.py` and
  `crates/common/tests/redis_acl.rs` learn the kind (§13.2).
- **The default user.** With `defaultUser` other than `"on"`, the chart
  refuses any client still on `default` whose workload renders.
  `pollerTransitMetrolink` counts as deployed only while
  `pollerTransitSchedule.networks.metrolink.enabled` is set
  (`distant-signal.redisAclClientDeployed`).

**Postgres grants and RLS.**

- **`ingest_freshness` has a writer row policy** listing every source
  the writer may write (security review L4; migration
  `20261009170000_ingest_freshness_rls.sql`). The timetable handler
  (batch 1) records `transit-metrolink-timetable`, so batch 1, not 4.1,
  adds both transit sources to `row_policies.ingest_freshness.writer`.
  The situations one is unused until batch 4.
- **`line_status`'s writer policy is still `source = 'tfl'`**
  (`row_policies.line_status.writer`, migration
  `20261009131300_line_status_rls.sql`). Batch 4.1 widens it to
  `source IN ('tfl', 'transit')` as planned. No other `line_status`
  source has appeared since.
- **`read_shared`.** Class `ingest` tables are in the `read_shared`
  group, so the api, aggregator, notifier and writer all get SELECT on
  `transit_*`. The first draft said the aggregator gets no grants: it gets
  SELECT through the group, which is harmless (public data, and the
  aggregator has no code reading it). Only the writer gets DML.
- **DB tests run per role.** In CI's per-service step, ds-store's suite
  runs as the api role and ingest-writer's as the writer. So the transit
  write tests (plan 1.5) live in `crates/ingest-writer/tests/`, where
  they run as the writer and prove its grants. ds-store's suite keeps
  read-only tests (batch 3).
- **Migrations.** The newest existing migration is
  `20261010120000_enricher_llm_batches.sql`, so the transit migrations
  use `20261014…`. The lock is `crates/api/tests/migration_checksums.lock`.
  Index DDL needs its own `-- no-transaction` file only on an existing
  table: `migration_index_locking` allows an index on a table the same
  file creates. So the transit indexes go in the table migration (§8.2).

**Line status (since 2026-10-09).**

- **`line_status.statuses` is a `Vec<common::LineStatus>`** (`severity`
  as a `common::Severity` number, `reason`, `validity`, `data_quality`),
  not TfL's camelCase API shape. Transit rows use the same type (§10.3).
- **"No reported disruption" needs a new `Severity` variant.** The
  closest existing ones are `NoIssues` (25, TfL's "No Issues") and
  `GoodService` (10). Neither says what Q1 decided. Batch 4.2 adds
  `Severity::NoReportedDisruption = 27`, grouped "good" in
  `frontend/lib/severity.ts`. It also adds `DataQuality::Transit`
  (`"transit"`), so a reader can tell a published-situations status from
  a sampled one.
- **Recently cleared incidents** (`/public/incidents/recently-cleared`,
  2026-10-12) cover Knowledgebase incidents on catalogue lines. Transit
  does not use them. A transit line page lists its own current and
  upcoming situations (§11).
- **The aggregator's reason text** (2026-10-09, plain sentences) touches
  only `source = 'aggregator'` rows.

**Frontend (the polish rounds; docs/style-guide.md).**

- **Copy rules.** New transit pages follow the style guide: sentence
  case; no em dashes, ellipses or arrows; `describeFailure` for every
  failure; no inline `style` props (ESLint bans them); the shared
  components (`CardLink`, `TextLink`, `ConfirmActionButton`, `ServiceRow`
  and friends); tokens. Their copy passes `scripts/check-copy.py` and the
  ESLint copy rules.
- **Copy changes.** The board caption "Scheduled times — not live" breaks
  the em-dash rule, so it becomes "Scheduled times, not live". A
  tram is a "tram", not a "train" (the terminology table's "train" is
  National Rail), and stops are "stops".
- **Visual baselines.** Each new page gets one (`frontend/e2e/visual.spec.ts`,
  `scripts/visual-tests.py --update`, reviewed images committed).

**Plan mechanics.**

- **The schema names** live in `common::transit` (constants), and the
  stream → schemas map is `ingest_writer::stream::STREAMS`, not a list
  in `ingest_stream`. ingest-stream has no schema registry. Its golden
  tests cover the envelope only, so the payload fixtures go with the
  types, under `crates/common/tests/fixtures/transit/`.
- **The parsers are in the lockfile:** `zip` 8.6, `csv` 1.4 and
  `quick-xml` 0.41, so batch 2 adds no parser crate.

## 1. Summary

DS covers National Rail (Darwin, Knowledgebase, CIF, TRUST) and TfL (line
status). It has a disabled island-of-Ireland integration. This design adds
**generic transit networks**, with Manchester Metrolink first:

- **Sources.** The DfT Bus Open Data Service (BODS), under the Open
  Government Licence (OGL). The North West regional GTFS file is filtered
  to NOC `METL` for timetables, and the SIRI-SX bulk archive is filtered to
  TfGM's tram situations for disruptions. NaPTAN (OGL) supplies clean stop
  names and platform indicators. Neither BODS bulk download needs an API
  key (§3.1). TfGM's live tram times wait for a key that TfGM does not
  issue today; this design reserves a slot for them (§15.2) and does not
  build it.
- **One producer for every network.** `poller-transit-schedule` is
  configured per network and has source adapters: GTFS now, TransXChange
  later (for Nottingham NET). The chart deploys it once per enabled
  network, from one image, off by default.
- **Stream only.** Each network has its own stream,
  `ds:ingest:transit-<network>`, and its own Redis ACL user. The
  ingest-writer is the only database writer.
- **Generic tables.** `transit_*` tables are keyed by network and id, and
  each timetable is a versioned feed that is swapped in atomically. The
  island-of-Ireland data could move into these tables later.
- **Network-qualified identity.** Routes are `/public/transit/{network}/…`
  and line ids look like `metrolink:blue`. `SUPPORTED_MODES` does not
  change: its `tram` stays TfL Trams.
- **Phase 1 is scheduled data.** Network, line and stop pages, scheduled
  stop boards and a line timetable. Boards are labelled as scheduled, and
  DS infers no punctuality or cancellations.
- **Phase 2 adds disruptions.** Disruptions become `line_status` rows with
  `source = 'transit'`, under an extended row-level security (RLS) policy.
  Notifications for followed lines, and links between rail stations and
  tram stops, come in the same phase.

The work is five batches of about 17–22 engineer-days (§15.1). Batches 1–3
ship off by default.

## 2. Scope

**In scope (phases 1–2):**

- network, line and stop pages;
- scheduled stop boards and a line timetable;
- disruptions mapped to line status, with stop notices;
- push notifications for disruptions on followed (pinned) transit lines;
- links between rail interchanges (`transit_rail_links`);
- attribution in the footer, on `/attribution` and in the API docs.

**Out of scope:**

- **The trip planner.** Multimodal planning across rail and tram is a later
  phase of size L/XL (§15.2).
- **Inferred performance.** DS infers no punctuality, cancellations or
  "trains running" figures for a frequency tram service. The deleted DLR
  pilot and `2026-08-22-tfl-service-metrics-v2-design.md` set this rule: a
  timetable with no live data says nothing about what ran.
- **Full-coverage stats and the line catalogue** (`lines/`,
  `line-catalogue-validator`) stay National Rail only.
- **`SUPPORTED_MODES`** and the TfL-shaped `/Line/Mode/{modes}/…`
  endpoints do not change (D4).
- **Nottingham NET** is parked until Metrolink is done (D6). It appears
  here only as the future user of the TransXChange adapter.
- **TfGM's live tram times** (closed API; §15.2).

## 3. Sources and measurements

All figures were measured on 2026-10-09, with downloads into the session
scratchpad.

### 3.1 BODS bulk downloads

| What | URL | Auth | Size | Notes |
|---|---|---|---|---|
| GTFS, North West | `https://data.bus-data.dft.gov.uk/timetable/download/gtfs-file/north_west/` | **none** (HTTP 200, anonymous) | 123 MB zip, 756 MB unzipped (`stop_times.txt` 430 MB, `shapes.txt` 309 MB) | `itm_north_west_gtfs.zip`. Rebuilt nightly: `feed_info` reads `feed_version=20261009_025808`, with entries dated 03:09. No `ETag` or `Last-Modified` |
| GTFS, national | `…/timetable/download/gtfs-file/all/` | none | 1.69 GB zip | not used |
| SIRI-SX bulk | `https://data.bus-data.dft.gov.uk/disruptions/download/bulk_archive` | **none**: a 302 to a signed, expiring `https://download.bus-data.dft.gov.uk/disruptions/sirisx_<ts>.zip` | 290 KB zip, 5.6 MB XML | one `sirisx.xml`. Regenerated about every minute (`Last-Modified` was 13:21:57 when fetched at 13:22). It has an `ETag` |
| SIRI-SX API | `https://data.bus-data.dft.gov.uk/api/v1/siri-sx/` | **API key** (HTTP 401 without one) | – | not used |
| Download index pages | `…/timetable/download/`, `…/disruptions/download/` | login (302 to `/account/login/`) | – | for humans only |

The research doc of 2026-08-31 said that BODS covers bus only. That is
wrong: it carries Metrolink timetables and TfGM tram disruptions. The doc
now has a dated correction note.

### 3.2 Metrolink in the North West GTFS

| Item | Value |
|---|---|
| Agency | `agency_id=OP288`, `agency_name=Metrolink`, `agency_noc=METL` |
| Routes | 83, all `route_type=0`. `route_short_name` is one of seven colours (`Blue Line`, `Green Line`, `Navy Line`, `Pink Line`, `Purple Line`, `Red Line`, `Yellow Line`) or `Replacement Bus` (2 routes). There are no colours and no long names |
| Trips | 15,849, on 12 `service_id`s |
| `stop_times` rows | 303,369 (26 MB of CSV) |
| Stops | 199 platforms. `stop_id` is the NaPTAN ATCO code (`9400ZZMAPIC2`) and `parent_station` is the NaPTAN stop area (`940GZZMAPIC`) on every one |
| Calendar | from 2026-10-09 to 2027-07-17, with 40 `calendar_dates` exceptions |
| Shapes | on 14,988 trips (not used, §6.2) |
| Distinct stop patterns (stop sequence plus run times) | **109** |
| Compact payload (patterns plus trips, JSON) | 1.4 MB raw, 244 KB gzip (with trip ids shortened to 12 characters; full 42-character ids add about 300 KB gzip) |
| Flat `stop_times` payload, for comparison | 25.5 MB raw, 3.4 MB gzip |

### 3.3 TfGM tram situations in SIRI-SX

The bulk archive held 493 situations, 157 of them with
`ParticipantRef=TfGM`. 17 had `VehicleMode=tram`: the Metrolink ones, plus
some from South Yorkshire (Supertram, `9400ZZSY…`). They have these
quirks:

- **Mostly stop-level equipment.** 12 of 17 are `EquipmentReason=liftFailure`,
  with `AllLines` plus one `AffectedStopPoint` (`9400ZZMAHEA`, Heaton Park).
  These are stop notices, not line disruptions (§10.2).
- **`LineRef` values are inconsistent:** `BLUE`, `PURP`, `Green_Line`,
  `Navy_Line`. The network config maps them through an alias table (§6.4).
- **`OperatorRef=METL`** is on only some situations. Others name no
  operator (`AllOperators`).
- **Stop references are the stop-area ATCO code** (`9400ZZMAHEA`), not the
  platform code.

### 3.4 Other sources

- **NaPTAN** `https://naptan.api.dft.gov.uk/v1/access-nodes?atcoAreaCodes=940&dataFormat=csv`
  needs no key and is under the OGL. It is 618 KB and holds 309
  `9400ZZMA…` rows: `StopType` `MET` for the station (`9400ZZMAPIC`) and
  `PLT` for each platform (`9400ZZMAPIC1`). It adds `ShortCommonName`
  (`Piccadilly`), `Indicator` (`To Ashton`) and `Status`.
- **TfGM GTFS** (`https://odata.tfgm.com/opendata/downloads/TfGMgtfsnew.zip`)
  is 41.7 MB, under the ODbL and rebuilt nightly. Its quirks:
  - replacement buses are tagged `route_type 0`;
  - there is no `parent_station`;
  - there are no colours;
  - the horizon is about one month;
  - `stop_code` is the ATCO code.

  This is the fallback (§4).
- **TfGM's live tram data.** The Open Data Portal API is closed to new
  keys. The undocumented `apiary.tfgm.com` GraphQL exists but is
  **off-limits**, and so is scraping thetram.net or any other website HTML
  (D5).
- **National Rail interchanges.** CRS and TIPLOC from
  `reference-data/crs-tiploc.csv`, checked on 2026-10-09:

  | CRS | TIPLOC | Station |
  |---|---|---|
  | MAN | MNCRPIC | Manchester Piccadilly |
  | MCV | MNCRVIC | Manchester Victoria |
  | DGT | MNCRDGT | Deansgate |
  | ALT | ALTRNHM | Altrincham |
  | NVR | NAVGTNR | Navigation Road |
  | RCD | RCHDALE | Rochdale |
  | EDY | EDIDBRY | East Didsbury |
  | AHN | ASHONUL | Ashton-under-Lyne |
  | MIA | MNCRIAP | Manchester Airport |
  | ECC | ECCLES | Eccles |

  The CORPUS pseudo-CRS rows for Metrolink stops (`AMM`/`MNCRAMM` "Abraham
  Moss Mtlk", `AMO`, `ANH`, `ASM`, `AUL`, …) are excluded as `NotAStation`
  (`common::corpus_inference::Exclusion`). They **must not** be used as
  keys or as link targets.

## 4. Choosing the timetable source

**Recommendation: the BODS North West GTFS, filtered to NOC `METL`. The
TfGM GTFS is the fallback** (one config value, `adapter.gtfsUrl`, plus its
attribution, §6.4).

| | BODS North West | TfGM GTFS | BODS national |
|---|---|---|---|
| Licence | OGL v3 (preferred, D5) | ODbL (acceptable with attribution, D5) | OGL v3 |
| Download | 123 MB | 41.7 MB | 1.69 GB |
| Unzipped | 756 MB | smaller (Greater Manchester only) | many GB |
| Memory | the zip goes to an `emptyDir`; the entries are **streamed** and filtered by trip id; the peak heap is the Metrolink subset (about 60 MB, §6.2) | could be held whole, but is also streamed | the same streaming, but 14× the download for no gain |
| Filtering | NOC `METL` → `agency_id` → routes → trips → `stop_times` | agency and route filter, plus replacement buses tagged `route_type 0` | as North West |
| Stations | `parent_station` = NaPTAN stop area | none: NaPTAN needed for the grouping | as North West |
| Horizon | about 9 months | about 1 month | as North West |
| Same family as the disruptions | yes (BODS SIRI-SX, the same ATCO codes) | no | yes |

The North West file costs about 123 MB of download a day and a streaming
parse. It wins on licence, station grouping, horizon and consistency with
SIRI-SX. The national file has no advantage for one network. Its only use
would be a future network whose region is unclear, and the adapter takes
any URL.

## 5. Architecture

```
 BODS GTFS (NW)  ──┐                                   ┌── transit_* tables
 BODS SIRI-SX    ──┼─ poller-transit-schedule ──XADD──►│   (versioned feeds,
 NaPTAN 940      ──┘   (one Deployment per network,    │    situations,
 rail-links CSV        network=metrolink)              │    rail links)
                       Redis user                      │
                       poller-transit-metrolink        ├── line_status / _history
                                                       │   (source='transit')
          ds:ingest:transit-metrolink ──► ingest-writer┤
                                       (stream task,   └── ingest_freshness
                                        off/apply)         (transit-metrolink-*)

 api /public/transit/{network}/… ── SELECT ── transit_*, line_status
 notifier ── line_status_history (pinned transit lines) ── web push
```

These ingest rules apply unchanged (D7):

- **Stream only.** The producer has no `http` sink and no database
  credentials, like the island-of-Ireland pollers (ingest D8). There is no
  `/private` route to add or delete.
- **One stream per network**, `ds:ingest:transit-<network>`. The writer
  pins each stream to its network (§9.1), so a compromised producer cannot
  write another network's rows.
- **One Redis ACL user per network**, `poller-transit-<network>`, with
  `~ds:ingest:transit-<network> +xadd +xrevrange` and the handshake
  commands (ingest D6).
- **The ingest-writer is the only database writer.** The handlers follow
  `handlers/island_of_ireland.rs`: decode, pin the network, write under
  dedup, record freshness with `produced_at` (ingest D13).
- **`db-grants.yaml` classifies every new table** (§8.6). The CI check
  fails on an unclassified table.
- **The stream budget and constants apply** (`ingest_stream::budget`,
  ingest D5; §7.3).

**Adding a network is a code change plus a values change.** The stream's
budget declaration and the ACL line are compiled or checked-in data, so
they cannot appear at deploy time. The network's config file (§6.4) is also
checked in. The chart then turns the network on.

## 6. The producer, `poller-transit-schedule`

### 6.1 Shape

A new crate, `crates/poller-transit-schedule`, builds one binary and one
image. Its process serves exactly one network (`TRANSIT_NETWORK=metrolink`)
and runs two loops:

| Loop | Input | Cadence | Output schema |
|---|---|---|---|
| timetable | the GTFS adapter (§6.2), NaPTAN (§6.3) and the rail-links CSV | every 24 h, and at startup if the stream's newest timetable commit is older than 24 h (cursor through `XREVRANGE`) | `transit-timetable/1` parts plus one `transit-timetable-commit/1` |
| situations | the SIRI-SX adapter (§10.1) | every 5 min (`If-None-Match` with the archive's `ETag`) | `transit-situations/1` |

The two loops use **two separate producers** on the same stream. Under
`ProducePolicy::LatestSnapshot` a newer item replaces an unsent older one
(`ingest_stream::snapshot`: "Two items would supersede each other"). One
producer would let a situations snapshot drop an unsent timetable while
Redis is down. Each `SnapshotProducer` keeps its own latest item, so
neither loop can supersede the other. The timetable's parts and its commit
are **one item**, XADDed in order.

The process has the standard worker parts:

- `health-http` `/livez` and `/readyz`. Readiness reports
  `stream_unavailable` from `is_available()`.
- A metrics port.
- `readOnlyRootFilesystem`, with an `emptyDir` at `/work` for the download.
- `REDIS_USERNAME` and `REDIS_PASSWORD` for its own user.

### 6.2 The GTFS adapter

The steps:

1. **Download** to `/work/feed.zip`, with a byte cap
   (`TRANSIT_GTFS_MAX_BYTES`, default 400 MB) and a timeout. A zip needs a
   seekable file, because its central directory is at the end. The file is
   deleted after the parse.
2. **`agency.txt`**: the `agency_id`s whose `agency_noc` is in the
   network's NOC list (`METL`).
3. **`routes.txt`**: those agencies' routes. `route_short_name` maps to a
   DS line through the network config (`Blue Line` → `blue`). A route whose
   name is in `replacement_bus_route_names` marks its trips as replacement
   buses. Any other unmapped name fails the feed, as
   `transit_feed_refused_total{reason="unmapped_route"}`: a new line needs a
   config change, not a guess.
4. **`trips.txt`** (14 MB) is streamed. The adapter keeps the trips of
   those routes: about 16k, a few MB.
5. **`stop_times.txt`** (430 MB) is **streamed line by line**. A row is
   kept only if its `trip_id` is in the set. This is the only large read,
   and its memory is bounded by the Metrolink subset (300k rows, about
   60 MB as parsed structs, then folded into patterns).
6. **`stops.txt`**: only the stops used, plus their `parent_station`.
7. **`calendar.txt` and `calendar_dates.txt`** are expanded into explicit
   service dates from London-yesterday to today plus
   `TRANSIT_HORIZON_DAYS` (default 60). The window starts at yesterday
   because GTFS times run past 24:00.
8. **`shapes.txt` is never opened** (309 MB). Shapes are not needed for
   phases 1–2.
9. **Folding into patterns.** A pattern is a line, a direction and the
   ordered `(platform, arrival offset, departure offset, pickup, drop-off)`
   tuples relative to the first departure. Each trip becomes `(trip_id,
   line, pattern, service_id, start seconds, headsign, replacement_bus)`.
   That gives 109 patterns and 15,849 trips (§3.2).
10. **Per-line station order.** For each line and direction, the station
    order of its longest pattern, merged with the stations of its other
    patterns. The line page uses it.
11. **The feed guard** (as `poller-irish-rail-gtfs`'s `feed_guard.rs`).
    The adapter refuses a feed if any of these holds:
    - it is empty;
    - the trip, stop or line count dropped by more than
      `feedMaxDropFraction` (default 0.3) since the last committed feed;
    - today has no service.

    The baseline survives restarts: the producer reads the last commit's
    counts from the stream (`XREVRANGE`, allowed by its ACL), not only from
    memory. When it refuses a feed, the previous version stays active.

The parser uses the `zip` (8.x) and `csv` crates already in the lockfile
(`poller-irish-rail-gtfs` uses `zip`). It adds no new dependency.

**The TransXChange adapter** (later, for NET from TNDS) produces the same
in-memory model: lines, stations, platforms, patterns, trips and service
days. Everything after the adapter is shared. The adapter is chosen by
`TRANSIT_ADAPTER=gtfs|txc`, and only `gtfs` is built now.

### 6.3 Stops, NaPTAN and rail links

- **Stations and platforms.** A GTFS `parent_station` `940GZZMAxxx`
  becomes the station id **`9400ZZMAxxx`**, the NaPTAN `MET` access-node
  ATCO code that SIRI-SX also uses (§3.3). A platform keeps its `stop_id`
  (`9400ZZMAxxx<n>`). The mapping is a documented prefix rule (`940G` →
  `9400`), and a test checks it against the NaPTAN rows.
- **Names.** From NaPTAN: `ShortCommonName` for the station (`Piccadilly`)
  and `Indicator` for the platform (`To Ashton`). If NaPTAN is unavailable
  or has no row, the adapter falls back to the GTFS `stop_name`, with the
  ` (Manchester Metrolink)` suffix removed. A failed NaPTAN fetch never
  blocks a timetable; it is counted
  (`transit_enrichment_failed_total{source="naptan"}`).
- **Rail links** come from a checked-in
  `reference-data/transit/metrolink-rail-links.csv` with the columns
  `stop_id,crs,tiploc,kind,note`. `kind` is `interchange` (the same
  complex) or `nearby` (a short walk). The file is baked into the image
  like `lines/` and sent with every timetable. A unit test checks that:
  - every `crs`/`tiploc` pair is in `reference-data/crs-tiploc.csv`;
  - no row uses a CRS excluded as `PseudoCode` or `NotAStation` (the
    Metrolink pseudo-CRS rows of §3.4);
  - every `stop_id` is a `MET` row in a NaPTAN fixture.

  The initial rows are the ten stations of §3.4. Their station ATCO codes
  are filled in from NaPTAN when the file is written; this spec does not
  invent them.

### 6.4 Network config

The per-network file is `crates/poller-transit-schedule/networks/<network>.toml`.
It is compiled in with `include_str!`, so the network list is known at
build time and tests can check it. Its contents:

```toml
id = "metrolink"                     # [a-z0-9-]+, the URL and id prefix
name = "Manchester Metrolink"
mode = "tram"                        # display only; NOT a SUPPORTED_MODES value
timezone = "Europe/London"
nocs = ["METL"]
replacement_bus_route_names = ["Replacement Bus"]
naptan_area = "940"
atco_prefix = "9400ZZMA"

[attribution]
licence = "OGL-UK-3.0"
text = "Contains public sector information licensed under the Open Government Licence v3.0. Timetables and disruptions: Bus Open Data Service (Department for Transport). Stops: NaPTAN (Department for Transport)."

[situations]
participant_refs = ["TfGM"]
vehicle_modes = ["tram"]
operator_refs = ["METL"]

[[lines]]
id = "blue"                          # public id metrolink:blue
name = "Blue Line"
gtfs_route_names = ["Blue Line"]
siri_line_refs = ["BLUE", "Blue_Line"]
colour = "#…"                        # display colour, set from the network map when the file is written
# …green, navy, pink, purple, red, yellow
```

The hex colours, and any branding beyond the network's name, are chosen
when the file is written. DS uses no TfGM logos.

## 7. Stream, schemas and budget

### 7.1 The stream

| Stream | Schemas | Producer | Cadence | Entry size (gzip) |
|---|---|---|---|---|
| `ds:ingest:transit-metrolink` | `transit-timetable/1`, `transit-timetable-commit/1`, `transit-situations/1` | poller-transit-schedule (network `metrolink`) | timetable daily, about 12–16 parts plus 1 commit; situations every 5 min, 1 part | timetable parts at most **64 KiB** each; situations about 5–10 KB; the commit under 2 KB |

`ingest_stream::streams::TRANSIT_METROLINK = "ds:ingest:transit-metrolink"`.
The writer's stream key is `transit-metrolink`
(`INGEST_WRITER_STREAMS=transit-metrolink:apply`).

### 7.2 Schemas

All the types live in a new `common::transit` module, used by the producer,
the writer and the api. Each schema has golden fixtures in
`crates/ingest-stream/tests/fixtures/`, as the existing ones do.

- **`transit-timetable/1`.** One part of a feed version. Each part carries:
  - `network`, `feed_version` and `part_kind`;
  - the rows of one kind: `network` (one row), `lines`, `stops`,
    `patterns`, `trips`, `service_days` or `rail_links`.

  `feed_version` is `<source feed_version>-<first 12 hex of the SHA-256 of
  the canonical model>`, for example `20261009_025808-3fa1c2…`. Parts are
  split by row count. Trips go in chunks of 1,500 at first, and a chunk is
  halved until its gzip body is at most 64 KiB. That is a byte cap on top
  of `split_snapshot`'s 512 KiB, added to `ingest_stream::snapshot` as
  `Snapshot::add_capped`.
- **`transit-timetable-commit/1`.** `{network, feed_version, content_sha256,
  parts: {kind: count}, counts: {lines, stops, patterns, trips,
  service_days}, horizon: {from, to}, source: {url_kind, feed_version,
  published_at}}`. It is the last entry of the item.
- **`transit-situations/1`.** The whole current set of the network's
  situations (§10.1). The bulk archive is complete, so the set is too: a
  situation missing from it has ended.

**Why patterns, not flat `stop_times`.** The flat form is 3.4 MB gzip a
day: about 55 entries at 64 KiB, which crowds the stream's cap. The
pattern form is about 0.6 MB with full trip ids, and the database stores
the same shape (§8.2).

### 7.3 Budget

The new `StreamDecl`:

```rust
// poller-transit-schedule (metrolink): situations every 5 min (288/day)
// plus a daily timetable of up to 32 entries; parts capped at 64 KiB.
// Covers a day, so one whole timetable item survives a day-long writer
// outage (a trimmed part would leave its commit incomplete, §9.2).
StreamDecl {
    stream: crate::streams::TRANSIT_METROLINK,
    entries_per_day: 288 + 32,
    entry_bytes: 64 * KIB,
    bound: Bound::Covers(Duration::from_secs(24 * 3600)),
}
```

This gives a `MAXLEN` of 320. The worst case is (320 + 100) × (64 KiB +
512 B) ≈ 27.7 MB for the stream and the same for its dead-letter stream,
so **about 55 MB**. The existing declarations total about 292 MB worst
case, so the new total is about 347 MB of the 512 MB budget (ingest D5),
under the 384 MB alert at 75%. `check_budget`'s unit test and
`crates/ingest-stream/tests/chart_values.rs` cover the new stream. In
practice the stream holds well under 3 MB: most entries are situations of
under 10 KB.

## 8. Tables

All tables are created in `crates/ds-store/migrations` as expand-only
migrations. They are generic (D3): every key starts with `network`.

### 8.1 Networks and feed versions

```sql
CREATE TABLE transit_networks (
    network              TEXT PRIMARY KEY CHECK (network ~ '^[a-z0-9-]+$'),
    name                 TEXT NOT NULL,
    mode                 TEXT NOT NULL,
    timezone             TEXT NOT NULL,
    attribution          JSONB NOT NULL,
    active_feed_version  TEXT,             -- NULL until the first commit
    activated_at         TIMESTAMPTZ,
    updated_at           TIMESTAMPTZ NOT NULL
);

CREATE TABLE transit_feed_versions (
    network          TEXT NOT NULL,
    feed_version     TEXT NOT NULL,
    content_sha256   TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('staging','active','previous','refused')),
    parts_seen       JSONB NOT NULL DEFAULT '{}',  -- {kind: count}
    counts           JSONB,
    horizon_from     DATE, horizon_to DATE,
    produced_at      TIMESTAMPTZ NOT NULL,         -- envelope produced_at (ingest D13)
    activated_at     TIMESTAMPTZ,
    PRIMARY KEY (network, feed_version)
);
```

### 8.2 The versioned timetable

Every row carries `(network, feed_version)`, so activation only changes
`transit_networks.active_feed_version`. Readers join on the active version.

| Table | Key | Columns | Metrolink rows |
|---|---|---|---|
| `transit_lines` | network, feed_version, line_id | name, colour, sort_order, `stations` JSONB per direction (§6.2 step 10) | 7 |
| `transit_stops` | network, feed_version, stop_id | kind (`station`/`platform`), parent_stop_id, name, indicator, lat, lon, wheelchair | about 100 stations and 199 platforms |
| `transit_patterns` | network, feed_version, pattern_id | line_id, direction, headsign | 109 |
| `transit_pattern_stops` | network, feed_version, pattern_id, seq | stop_id (platform), arr_offset_s, dep_offset_s, pickup, drop_off | about 3k |
| `transit_trips` | network, feed_version, trip_id | line_id (NULL for a replacement bus), pattern_id, service_id, start_s (may exceed 86400), headsign, replacement_bus | 15,849 |
| `transit_service_days` | network, feed_version, service_id, service_date | – | about 12 × 61 |

The indexes are `transit_pattern_stops (network, feed_version, stop_id)`
for boards and `transit_trips (network, feed_version, pattern_id)`. They
are created in the tables' own migration: the `-- no-transaction` rule
is for an index on an existing table, and `migration_index_locking`
allows an index on a table the same file creates (§0).

At most two versions are kept, `active` and `previous`, so about 40k
rows per network. Older versions are deleted at activation.

### 8.3 Situations

```sql
CREATE TABLE transit_situations (
    network         TEXT NOT NULL,
    situation_id    TEXT NOT NULL,        -- SIRI SituationNumber
    version         INT,
    planned         BOOLEAN NOT NULL,
    reason          TEXT,                 -- e.g. equipment:liftFailure
    summary         TEXT NOT NULL,
    description     TEXT,
    info_url        TEXT,
    validity        JSONB NOT NULL,       -- [{start,end}]
    publication     JSONB NOT NULL,
    consequences    JSONB NOT NULL,       -- normalised, §10.1
    line_ids        TEXT[] NOT NULL,      -- mapped lines (§10.2)
    stop_ids        TEXT[] NOT NULL,      -- station ATCO codes
    scope           TEXT NOT NULL CHECK (scope IN ('line','stop','network')),
    observed_at     TIMESTAMPTZ NOT NULL, -- envelope produced_at
    first_seen_at   TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (network, situation_id)
);
```

### 8.4 Rail links

```sql
CREATE TABLE transit_rail_links (
    network   TEXT NOT NULL,
    stop_id   TEXT NOT NULL,              -- station ATCO
    crs       TEXT NOT NULL,
    tiploc    TEXT NOT NULL,
    kind      TEXT NOT NULL CHECK (kind IN ('interchange','nearby')),
    note      TEXT,
    observed_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (network, stop_id, crs)
);
```

This table is not versioned. It is replaced whole (delete absent, upsert
present) at each timetable commit, in the activation transaction.

### 8.5 Existing tables

- **`line_status` and `line_status_history`** get transit rows (§10.3):
  `source = 'transit'`, `line_id = '<network>:<line>'`,
  `mode_name = '<network>'`. A CHECK constraint, added `NOT VALID` and
  then validated:
  `CHECK (source <> 'transit' OR line_id ~ '^[a-z0-9-]+:[a-z0-9-]+$')`.
- **`ingest_freshness`** gets the sources `transit-<network>-timetable` and
  `transit-<network>-situations`. Both join the writer's freshness row
  policy in batch 1 (§0, §8.6).

### 8.6 `db-grants.yaml`

| Table | class | Grants |
|---|---|---|
| `transit_networks`, `transit_feed_versions` | ingest | api: S; writer: SIU (+D on `transit_feed_versions`, for pruning) |
| `transit_lines`, `transit_stops`, `transit_patterns`, `transit_pattern_stops`, `transit_trips`, `transit_service_days` | ingest | api: S; writer: SID (rows are inserted per version and deleted with it; never updated) |
| `transit_situations` | ingest | api: S; writer: SIUD |
| `transit_rail_links` | ingest | api: S; writer: SIUD |

**Row policies:**

- `line_status`: the writer's policy becomes
  `source IN ('tfl', 'transit')` (§10.4).
- `ingest_freshness`: the writer's write list gains the two
  `transit-metrolink-*` names, in batch 1 with the tables (the timetable
  handler records the first one; §0).

`postgres-grants.sql` is regenerated (`uv run scripts/gen-db-grants.py`).
The notifier needs nothing new: it already reads `line_status_history`.
The aggregator gets no grants of its own on `transit_*`. As class
`ingest` tables they are in `read_shared`, so it, the notifier and the
writer can SELECT them through the group (§0).

**Moving the island-of-Ireland data later.** `island_of_ireland_stations`
maps to `transit_stops` (network `irish-rail` or `nir`, kind `station`),
and `island_of_ireland_lines` maps to `transit_lines`. The live samples
would map to the reserved live table (§15.2). That move is not planned
here.

## 9. The ingest-writer handlers

`crates/ingest-writer/src/handlers/transit.rs` registers three schemas.
It uses the shared handler helpers: `decode`, `count_apply`,
`classify_anyhow` and `Observed`. (`count_shadow` went with the shadow
mode in release B.)

### 9.1 Network pinning

`stream_network(stream)` returns `Some("metrolink")` only for
`ds:ingest:transit-metrolink`. It reads a table generated from the same
network list the producer compiles in. These are poison
(`network_not_for_stream`), checked before
anything is written:

- a body whose `network` differs from the stream's;
- a line or stop id with another network's prefix;
- a `line_status` id outside `<network>:`.

This mirrors `island_of_ireland::pin_network`.

### 9.2 Timetable

The timetable has two handlers:

- **`transit-timetable/1`** inserts its rows under `(network,
  feed_version)` and records the part in `parts_seen` of a `staging` row in
  `transit_feed_versions`. It does nothing if that version is already
  `active` or `previous`. Re-delivery is idempotent through `ingest_dedup`
  and `ON CONFLICT DO NOTHING`.
- **`transit-timetable-commit/1`** does this in one transaction:
  1. If `content_sha256` equals the active version's, it records freshness
     and stops. The common case, an unchanged daily feed, writes nothing
     else.
  2. If the staged `parts_seen` differs from the commit's `parts`, the
     version is incomplete: a part was dead-lettered or trimmed. That is
     **poison** (`incomplete_feed_version`). The active version stays, and
     the staging rows are deleted by the next commit's prune. The next
     day's feed retries.
  3. Otherwise it checks consistency: every trip's pattern exists, every
     pattern stop exists, and today has service. A failure is poison.
  4. It flips the versions: `active` → `previous`, staging → `active`,
     `transit_networks.active_feed_version` set, and anything older than
     `previous` deleted.
  5. It replaces `transit_rail_links`.
  6. It calls `record_ingest(conn, "transit-metrolink-timetable",
     Some(produced_at))`.

  Its observed-time guard: a commit whose `produced_at` is older than the
  active version's is skipped, as an out-of-order redelivery (ingest D13).

### 9.3 Situations

**`transit-situations/1`**, in one transaction:

1. Upsert every situation. The guard is
   `EXCLUDED.observed_at >= observed_at OR observed_at > now() + 2 min`.
2. Delete the network's situations missing from this snapshot, but only if
   this snapshot is not older than the newest applied one.
3. Recompute the network's line statuses (§10.2) and write them (§10.3).
4. Record freshness for `transit-metrolink-situations`.

## 10. Disruptions and line status

### 10.1 Filtering and normalising SIRI-SX

The producer's SIRI-SX adapter does the following:

1. It downloads the bulk archive (302 → signed URL; `If-None-Match`; a
   size cap of 50 MB) and stream-parses `sirisx.xml` with `quick-xml`
   (already in the lockfile).
2. It keeps a `PtSituationElement` only if its `ParticipantRef` is in
   `participant_refs` (`TfGM`), **and** at least one of these holds:
   - an `AffectedNetwork` or `AffectedStopPoint` has a `VehicleMode` in
     `vehicle_modes` (`tram`);
   - a `StopPointRef` starts with `atco_prefix` (`9400ZZMA`);
   - an `OperatorRef` is in `operator_refs` (`METL`).

   This excludes South Yorkshire tram situations (`9400ZZSY…`, another
   participant).
3. It normalises each situation:
   - `LineRef` → line ids, through the config's `siri_line_refs`. An
     unmapped ref is counted
     (`transit_situation_unmapped_line_refs_total{ref}`) and kept raw.
   - `StopPointRef` → station ATCO codes. A platform code is folded to its
     station.
   - Reason, condition, severity, validity, publication window, summary,
     description, advice and the first `InfoLink`.
4. It emits the set, sorted by `SituationNumber`.

### 10.2 From situations to line status

A situation is **current** when all of these hold:

- `Progress=open`;
- now is inside a `PublicationWindow`;
- now is inside a `ValidityPeriod`.

A situation whose next validity period starts within 7 days is
**upcoming**. Rules for each consequence:

| Consequence | Scope | Effect on line status |
|---|---|---|
| `EquipmentReason` (lift, escalator, …) with stop points | `stop` | **none**. It is a stop notice on the stop page and on the pages of lines serving that stop |
| Mapped line refs, or `AllLines` with no stop points, and a disruptive condition | `line` | a status on each affected line (table below) |
| Mapped line refs or `AllLines` **with** stop points, and a disruptive condition | `line` | **Part suspended** or **Minor delays** on the lines serving those stops |
| Mapped line refs, non-disruptive condition (`unknown`, `normal`) | `line` | **Information** (shown on the line, never pushed) |
| No mappable line or stop | `network` | none. It is shown on the network page |

| SIRI `Condition` | SIRI severity | Status (`common::Severity`) |
|---|---|---|
| `noService`, `suspended`, `cancelled` (no stop subset) | any | Suspended |
| the same, with a stop subset | any | Part suspended |
| `diverted`, `altered`, `stopMoved`, `partService` | any | Reduced service |
| `delayed`, `disrupted`, `interrupted` | `severe`, `verySevere` | Severe delays |
| `delayed`, `disrupted`, `interrupted` | other | Minor delays |
| other or `unknown` | any | Information |

- The worst status per line wins, and every contributing situation is
  listed.
- **A line with no current line-scoped situation reads "No reported
  disruption"**, not "Good service". DS cannot observe service, so it only
  reports that nothing was published (open question Q1).
- Upcoming situations go to `line_status.upcoming`, as for TfL.
- **The rules never use the timetable.** A tram that is scheduled but has
  no situation is not "running", and a gap in the timetable is not a
  cancellation (§2).
- **Staleness.** If `ingest_freshness('transit-metrolink-situations')` is
  older than 30 min, the api serves the line status as **"Status
  unavailable"** with the as-of time.

### 10.3 Writing `line_status`

The statuses use the same JSON shape as TfL rows: `statuses` JSONB is a
`Vec<common::LineStatus>` (`severity` as a `common::Severity` number,
`reason`, `validity`, `data_quality`). So the existing line-status UI and
the notifier's comparison work unchanged. The status names in §10.2 map
to `Severity` variants: Suspended, PartSuspended, ReducedService,
SevereDelays, MinorDelays, Information, and the new
`NoReportedDisruption` (27, §0). `data_quality` is the new
`DataQuality::Transit`. The row is:

```
line_id     = 'metrolink:blue'
name        = 'Blue Line'
mode_name   = 'metrolink'
operators   = '{METL}'
source      = 'transit'
computed_at = source_updated_at = produced_at        (ingest D13)
```

- A history row is appended only when `statuses` changes, as in the TfL
  handler (`upsert_tfl_line_status_observed`).
- The prune is scoped:
  `DELETE … WHERE source = 'transit' AND line_id LIKE 'metrolink:%' AND NOT (line_id = ANY($1))`.
- The aggregator's prune (`source = 'aggregator'`) and the TfL prune
  (`source = 'tfl'`) never touch these rows.

### 10.4 The source value and the row policy

**Chosen: one generic `source = 'transit'`**, with the writer's row policy
widened to `source IN ('tfl', 'transit')` and network pinning in the
handler (§9.1). A per-network `source = 'metrolink'` was rejected.

- **RLS cannot tell networks apart.** It works per database role, and
  every network's stream is applied by the one writer role. A per-network
  `source` would add a policy change and a migration per network, and no
  database-enforced isolation.
- **Isolation between networks is enforced where it can be:** in the
  handler (stream → network pin) and in the line id grammar (the CHECK
  constraint of §8.5).
- **Readers** filter on `source = 'transit'` plus the `line_id` prefix.
  `mode_name` carries the network.

As D10's policy did, the policy is a RESTRICTIVE `ds_grants_writer`
rendered from `row_policies` in `db-grants.yaml`. The test
`line_status_rls.rs` gains these cases:

- a transit row is allowed for the writer;
- an `aggregator` row is still refused;
- the CHECK refuses `source = 'transit'` with an unqualified id.

### 10.5 Readers of `line_status` that must leave transit rows out

Some readers assume every row is National Rail or TfL. Batch 4 audits each
reader and excludes `source = 'transit'` where National Rail or TfL
semantics apply:

- the status dashboard and line lists;
- operator overview and its history;
- the `line_status_*_stats` and coverage rollups, which stay National Rail
  and TfL only (open question Q6);
- the MCP-facing `/public/lines/{id}` paths;
- `tfl_line_summaries` (already `source = 'tfl'`).

The `/Line/Mode/{modes}` endpoints already filter by `SUPPORTED_MODES`, and
`metrolink` is not one of them.

## 11. API

All routes are under `/public/transit`, served from the active feed
version. They answer 404 for a network not in `API_TRANSIT_NETWORKS` (a
comma list, **empty by default**), so routes can merge before a network is
on.

| Route | Returns | Cache-Control |
|---|---|---|
| `GET /public/transit/networks` | the enabled networks: id, name, mode, attribution, freshness (`timetableAsOf`, `situationsAsOf`) | 300 s |
| `GET /public/transit/{network}` | the network, its lines with current status (phase 2), the active feed's horizon, attribution | 60 s |
| `GET /public/transit/{network}/lines/{line}` | the line (`id: "metrolink:blue"`), its stations in order per direction, status and situations (phase 2) | 300 s (60 s with status) |
| `GET /public/transit/{network}/lines/{line}/timetable?date=&direction=&from=&to=` | a stations × trips grid for one direction, in a time window (default 3 h) | 300 s |
| `GET /public/transit/{network}/stops` | stations, with lines and rail links | 3600 s |
| `GET /public/transit/{network}/stops/{stop}` | a station, its platforms, lines, rail links and stop notices (phase 2) | 300 s |
| `GET /public/transit/{network}/stops/{stop}/departures?date=&after=&limit=&cursor=` | the **scheduled** board (below) | 60 s |
| `GET /public/transit/{network}/situations` | current and upcoming situations (phase 2) | 60 s |

- **Path segments.** `{line}` is the local id (`blue`). Bodies always
  carry the qualified id (`metrolink:blue`), and pins and notifications use
  the qualified id. `{stop}` is the station ATCO code (`9400ZZMAPIC`).
- **Departures.** The board is the union of a station's platforms. A row is
  dropped if its pickup is not allowed or it is the trip's last stop. A
  service date of D includes trips of D−1 whose `start_s + offset` passes
  midnight. Each row carries:
  - `scheduledDeparture` (London ISO time);
  - `line` (id, name, colour);
  - `destination` (the headsign, or the last stop's name);
  - `platform` (indicator);
  - `replacementBus`.

  Every board response carries `"realtime": false` and `"kind":
  "scheduled"`. There is no `expected`, `delay`, `cancelled` or `status`
  field, so no client can read a schedule as live (§2).
- **Rate limits.** The routes join the public route group's existing
  per-IP budget. `rate_limit.rs` and the route-metrics table gain the
  routes.
- **Docs.** `docs/api-changelog.md` gets an entry per batch that adds
  routes, with the attribution sentence and the `realtime: false`
  contract.

**Reusing `schedule_rows`.** National Rail boards come from
`schedule_rows` and `TrainSearchResult`, which are built around CRS,
TIPLOC, UID and operator codes. The transit board does not reuse that
query or type. It does reuse the frontend's row primitives (§12).

## 12. Frontend, notifications and attribution

### 12.1 Pages

| Page | Content |
|---|---|
| `/transit/[network]` | the lines as chips with their colour and (phase 2) status, current disruptions, a stop search, attribution |
| `/transit/[network]/lines/[line]` | stations in order (tabs per direction), status and situations (phase 2), a follow (pin) star (phase 2), a link to the timetable |
| `/transit/[network]/lines/[line]/timetable` | the grid, with date, direction and time window controls |
| `/transit/[network]/stops/[stop]` | the scheduled board, platforms, lines, rail links ("Change for National Rail at Manchester Piccadilly" → `/stations/MAN`), stop notices (phase 2) |

- **Components.** The board uses `ServiceRow`, `ServiceRowList`,
  `ServiceListNotice` and `LoadMoreControl`, whose shape is a time, a
  title, a subtitle and details. `StationTimetable` is not reused: it is
  bound to `TrainSearchResult` and CRS. A new `TransitStopBoard` maps the
  transit row onto `ServiceRow`. Every board shows the caption "Scheduled
  times, not live" (the style guide bans the em dash, §0).
- **Navigation.** The frontend lists `/public/transit/networks`. An empty
  list hides the nav entry, so the API switch is the only switch.
- **National Rail station pages** (phase 2) show "Metrolink stop:
  Piccadilly" for a CRS with a rail link. They read the link from
  `GET /public/transit/rail-links?crs=MAN`, a small extra route.
- **`frontend/lib/modes.ts` does not change** (D4).

### 12.2 Notifications

Followed transit lines use the existing pinned-lines path:

- **Pinning.** `preferences.rs` `filter_known_pinned_lines` accepts a
  pinned id if it is a `source = 'transit'` row in `line_status`, as it
  already does for TfL ids (`tfl_line_summaries`). Without this, a pin is
  silently dropped on read.
- **The notifier** reads `line_status_history` as today. Transit rows are
  stamped with `produced_at`, so its stale-history skip
  (`LINE_HISTORY_MAX_AGE_SECS`, ingest plan 3c.4) applies. The batch
  checks two things: that the push title and deep link resolve a transit
  line to `/transit/<network>/lines/<line>`, and that "Information" changes
  are not pushed.
- **No new notifier grants.**

### 12.3 Attribution

- **Footer** (`OpenDataAttribution`): one short line, "Contains public
  sector information licensed under the Open Government Licence v3.0",
  shown when any transit network is enabled. The full detail goes on the
  attribution page.
- **`/attribution`**: a Metrolink block with the network config's
  `attribution.text`, links to the OGL v3.0, BODS and NaPTAN, and the line
  "Metrolink is a trademark of Transport for Greater Manchester. Distant
  Signal is not affiliated with TfGM." The comment block in
  `OpenDataAttribution.tsx` records the licence basis, as it does for each
  existing source.
- **API.** `/public/transit/networks` and each network response carry
  `attribution`, and the api-changelog entry repeats it.
- **If the TfGM GTFS fallback is ever used,** the ODbL notice TfGM
  requires replaces the BODS timetable line. Its wording is taken from
  TfGM's licence page at that time.

## 13. Chart, security and observability

### 13.1 Values

`poller-transit-schedule` gets its own Deployment template, as the IoI
pollers do: it has no RDM key and no api client. It renders one Deployment
per enabled network:

```yaml
pollerTransitSchedule:
  image: {repository: ghcr.io/fasterspeeding/distant-signal/poller-transit-schedule, tag: "", digest: "", pullPolicy: IfNotPresent}
  progressStallSecs: 3600        # one 123 MB download + parse + produce
  logLevel: info
  metricsPort: 9091
  networks:
    metrolink:
      # -- Off by default (D2). Turn on with ingestWriter.streams.transit-metrolink,
      # redis.acl.clients.pollerTransitMetrolink and api.transitNetworks.
      enabled: false
      adapter: gtfs
      gtfsUrl: "https://data.bus-data.dft.gov.uk/timetable/download/gtfs-file/north_west/"
      gtfsMaxBytes: 419430400
      situationsUrl: "https://data.bus-data.dft.gov.uk/disruptions/download/bulk_archive"
      naptanUrl: "https://naptan.api.dft.gov.uk/v1/access-nodes?atcoAreaCodes=940&dataFormat=csv"
      timetableIntervalSecs: 86400
      situationsIntervalSecs: 300
      horizonDays: 60
      feedMaxDropFraction: 0.3
      workDirSizeLimit: 512Mi
      resources:
        requests: {cpu: 100m, memory: 192Mi}
        limits: {memory: 768Mi}
      extraEnv: []
api:
  transitNetworks: []            # API_TRANSIT_NETWORKS; empty: every /public/transit route 404s
ingestWriter:
  streams:
    transit-metrolink: "off"     # off | apply (release B removed shadow)
redis:
  acl:
    clients:
      pollerTransitMetrolink: false
```

- **Render checks** (they fail the render):
  - an enabled network with no `redis.acl.clients.pollerTransit<Network>`
    while `redis.acl.enabled`;
  - an enabled network whose name has no ACL line;
  - an enabled network without `ingestWriter.streams.transit-<network>`
    set to something other than `off` (a producer with no consumer only
    fills the stream to its cap).
- **The deployment.** Name `poller-transit-schedule-<network>`, component
  label `poller-transit-schedule`, label `distant-signal/transit-network`.
  It has an `emptyDir` at `/work` with `sizeLimit` and
  `readOnlyRootFilesystem`. `strategy: Recreate` from creation, so two
  pods never XADD at once (§0).
- **No secret.** There is no BODS key (§3.1). The Redis password comes
  from `redis.acl.existingSecret`, key `poller-transit-metrolink-password`.
  Ranma-Config adds that key to the SealedSecret.
- `uv run scripts/chart-values-doc.py check` covers the new values.

### 13.2 Redis ACL

Add to `charts/distant-signal/files/redis-users.acl.tpl`:

```
poller-transit-metrolink optional ~ds:ingest:transit-metrolink +xadd +xrevrange +ping +hello +auth +client|setname +client|setinfo +client|id
```

- **The user is `optional`** (§0): `final` rights in every stage, but
  rendered only while `redis.acl.clients.pollerTransitMetrolink` is on.
  The first draft made it `final`, which would have restarted Redis on
  merge and needed the Secret key before any deploy. Now merging renders
  nothing new. Turning the client on needs the
  `poller-transit-metrolink-password` key first and restarts Redis once
  (twice in quick succession if Reloader rolls it for the Secret too).
  That is the operational cost of each new producer.
- The `ingest-writer` user's `~ds:ingest:*` and `~ds:dlq:*` already cover
  the new stream.
- `crates/common/tests/redis_acl.rs` gains the user's allowed sequence
  (XADD with MAXLEN, XREVRANGE COUNT 1) and refused commands (XTRIM, XDEL,
  XREADGROUP, XADD to `ds:ingest:tfl`).
- `crates/ingest-stream/tests/redis_stream.rs` runs the producer as this
  user.
- `scripts/render-redis-acl.py` must agree with the chart (CI).
- `docs/redis-acl.md` lists the user.

### 13.3 NetworkPolicy

The worker policy is ingress from monitoring only (the metrics and health
ports). Its egress:

- `redis: true`; no postgres, no api, no token URL;
- `urls`: `gtfsUrl`, `situationsUrl`, `naptanUrl`, and
  `https://download.bus-data.dft.gov.uk/`, the SIRI-SX redirect target.

NetworkPolicy is port-based, so in practice this is TCP 443 to the
internet, as for the IoI pollers. A host allow-list would need an egress
proxy, which no DS worker has today. The redis policy's ingress list gains
the component.

### 13.4 Metrics and alerts

| Metric | Labels |
|---|---|
| `transit_feed_fetch_total` | `network, source (gtfs, sirisx, naptan), outcome` |
| `transit_feed_bytes` | `network, source` |
| `transit_feed_refused_total` | `network, reason (empty, sharp_drop, unmapped_route, no_service_today, oversize)` |
| `transit_feed_rows` (gauge, last parse) | `network, kind` |
| `transit_situations_current` (gauge) | `network, scope` |
| `transit_situation_unmapped_line_refs_total` | `network, ref` |
| `transit_enrichment_failed_total` | `network, source` |
| the `ingest_stream_*` family | `stream="ds:ingest:transit-metrolink"` |

| Alert | Expression (sketch) | Severity |
|---|---|---|
| the existing `DistantSignalIngestSourceStale` (2026-10-14: replaces the first draft's `DistantSignalTransitTimetableStale` and `DistantSignalTransitSituationsStale`) | `ingestSourceStale.maxAgeSeconds` gains `transit-<n>-timetable` (36 h) and `transit-<n>-situations` (30 min), each in `whileStreamApplies` under `transit-<n>`, so it is checked only while the writer applies the stream | as today |
| `DistantSignalTransitFeedRefused` | `increase(transit_feed_refused_total[26h]) > 0` | warning |
| `DistantSignalTransitUnmappedLineRef` | `increase(transit_situation_unmapped_line_refs_total[1h]) > 0` | info or warning (the config needs an alias) |
| the existing `DistantSignalIngestStreamBacklog` | per-stream map: `MAXLEN` 320, so warning at 160 and critical at 256 | as today |
| the existing `DistantSignalIngestStreamStalled` | cadence 300 s, so it fires after 900 s with no apply while produces succeed | as today |
| the existing `DistantSignalIngestDeadLetters` | covers `incomplete_feed_version` and `network_not_for_stream` | as today |

The new alerts render only when the network is enabled. Each alert
needs its `docs/alerts.md` runbook section and must pass
`scripts/check-alert-payloads.py` (CI).
`crates/ingest-stream/tests/chart_values.rs` keeps the per-stream maps
equal to `budget.rs`.

### 13.5 Security posture

- **The producer** holds a single Redis user that can only XADD to its own
  stream. It has no database or api credentials and no secrets beyond the
  Redis password.
- **Upstream data is untrusted.** The producer enforces caps on download
  size, zip entry size and XML size (zip-bomb guards). The writer re-checks
  the envelope limits, ingest §7.2. All text is rendered as text in the
  frontend.
- **Per-network isolation** is enforced in the writer (§9.1). It cannot be
  enforced in RLS, which works per role (§10.4).

## 14. Decisions (2026-10-09)

The user decided D1–D7 on 2026-10-09. They override anything in this
document that disagrees.

| # | Decision | Where it lands |
|---|---|---|
| D1 | **Metrolink starts now on DfT BODS feeds (OGL):** GTFS timetables filtered to NOC `METL`, and SIRI-SX disruptions (BODS carries TfGM tram situations today). Live PID times from TfGM's closed API wait until TfGM grants a key: there is a designed slot, and it is not built | §3, §4, §6, §10.1, §15.2 |
| D2 | **One shared producer, `poller-transit-schedule`,** configured per network. It has source adapters: GTFS now, TransXChange later for NET from TNDS. It is deployed once per configured network from one image, **off by default** (as ingest D8) | §6, §13.1 |
| D3 | **Generic, network-agnostic tables** (`transit_*`, keyed by network and id), which the island-of-Ireland data could later move into. No per-network table families | §8 |
| D4 | **Network-qualified API routes** `/transit/{network}/…` (served under `/public`) and line ids such as `metrolink:blue`. **Not** added to `SUPPORTED_MODES`, where `tram` means TfL Trams | §11, §12 |
| D5 | **Licences:** ODbL is acceptable with attribution, but the OGL BODS feeds are preferred. TfGM's undocumented `apiary.tfgm.com` GraphQL is noted as existing but **off-limits**. Scraping thetram.net or any website HTML is **off-limits** | §3.4, §4, §12.3 |
| D6 | **NET is parked** until after Metrolink. It is mentioned only as the TXC adapter's future user | §2, §6.2, §15.2 |
| D7 | **The ingest architecture rules apply:** a stream-only producer with its own `ds:ingest:transit-<network>` stream; its own Redis ACL user (XADD and XREVRANGE on that stream only); the ingest-writer as the only database writer, with handlers like `handlers/island_of_ireland.rs`; `db-grants.yaml` classifying every new table; the ingest-stream budget and constants (ingest D5–D8, D13) | §5, §7, §8.6, §9, §13.2 |

**Design choices made here, open to review (not user decisions):**

| # | Choice | Why |
|---|---|---|
| C1 | The BODS **North West** GTFS is the primary timetable source; the TfGM GTFS is the config-only fallback | OGL, NaPTAN parent stations, a 9-month horizon, the same family as SIRI-SX; streaming bounds memory (§4) |
| C2 | A **pattern-compressed, versioned** timetable payload, committed atomically | 0.6 MB a day instead of 3.4 MB; readers never see half a feed (§7.2, §9.2) |
| C3 | Timetable and situations share the network's stream but use **two producers** | `LatestSnapshot` would otherwise let one schema supersede the other (§6.1) |
| C4 | A generic `source = 'transit'`, the writer's row policy widened to `IN ('tfl','transit')`, network pinning in the handler and a line-id CHECK | RLS is per role and cannot isolate networks; one policy covers all networks (§10.4) |
| C5 | Equipment situations (lifts, escalators) are stop notices and never change a line's status; absence of situations is "No reported disruption" | 12 of 17 TfGM tram situations are lift failures; DS cannot observe service (§10.2) |
| C6 | Scheduled boards say `realtime: false` and carry no delay or cancellation fields | the DLR pilot and tfl-service-metrics-v2 rule (§2) |
| C7 | The 64 KiB part cap and a 24-hour stream cover for `transit-metrolink` | a day-long writer outage keeps a whole timetable item; about 55 MB worst case within ingest D5 (§7.3) |
| C8 | `API_TRANSIT_NETWORKS` (empty by default) is the only user-facing switch; the frontend follows `/public/transit/networks` | one switch, and routes can merge dark (§11) |

## 15. Phases, later work and risks

### 15.1 Phases

| Phase | Batches (plan) | What ships | Effort (engineer-days) |
|---|---|---|---|
| 1: scheduled | 1 foundations; 2 producer and chart; 3 API and frontend | network, line and stop pages, scheduled boards, the line timetable, attribution, all **off** until the values are flipped | 12–15 |
| 2: disruptions | 4 situations, line status, notifications; 5 rail links | line status, stop notices, follow and push, National Rail interchange links | 5–7 |
| | | **Total** | **17–22**, plus soak |

Rollout (by the user or Ranma, in Ranma-Config), in this order. The
writer has no shadow mode since release B (§0), so the api switch is the
dark-launch guard:

1. Add `poller-transit-metrolink-password` to the Redis users Secret and
   set `redis.acl.clients.pollerTransitMetrolink: true` in the same
   change. Redis restarts (once, or twice in quick succession if Reloader
   also rolls it for the Secret). Check `ACL USERS` lists the user.
2. In one change, `ingestWriter.streams.transit-metrolink: apply` and
   `pollerTransitSchedule.networks.metrolink.enabled: true`. The chart
   refuses the producer without the `apply`. The writer restarts (its
   env); the producer starts.
3. Check, with nothing public yet: the first timetable item with
   `XRANGE`, the writer's applied and dead-letter counts, the
   `transit_*` row counts against §3.2, and one situations snapshot.
4. After a day with the timetable applied, `api.transitNetworks:
   [metrolink]`.

Rollback: set `api.transitNetworks` back to `[]` (the pages disappear),
then turn the producer off. Transit rows in `line_status` stop updating
when the producer stops. The prune removes them at the next snapshot of an
empty line set, or one SQL delete in the runbook removes them.

### 15.2 Later work (not planned in detail)

- **TfGM live tram times (the reserved slot).**
  - A third adapter, `tfgm-pids`, behind a key in a SealedSecret
    (`tfgm-api-key`).
  - Schema `transit-stop-predictions/1` on the same stream, with its own
    producer (C3).
  - A table `transit_stop_predictions (network, stop_id, platform,
    line_id, destination, wait_minutes, status, observed_at)`, a live
    snapshot like `station_samples`.
  - The board would gain a `live` section with `realtime: true` beside the
    scheduled rows.
  - The budget declaration is resized: about 100 stations every 30–60 s
    is several entries a minute.
  - **Not built** until TfGM issues a key and its terms allow this use.
    TfGM's undocumented endpoints stay off-limits (D5).
- **The TransXChange adapter and Nottingham NET** (D6). TNDS TXC is parsed
  into the same model. NET then needs a network file, a stream and budget
  declaration, an ACL line and values.
- **The trip planner** (L/XL). Tram patterns would go into the RAPTOR
  connections, with footpaths from `transit_rail_links` and NaPTAN. It
  needs a common stop identity across rail and tram, and a policy on
  frequency-based trips. It is not started.
- **Island of Ireland into `transit_*`** (§8.6).
- **DS-MCP tools** for transit boards and status, after phase 2.

### 15.3 Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| BODS gates bulk downloads behind a key or login | low–medium | timetables stop | `DistantSignalTransitTimetableStale`; register a BODS account and add an optional key secret then (Q5); the TfGM GTFS fallback |
| The North West zip grows and the pod runs out of memory or disk | low | no refresh | streamed parse; `gtfsMaxBytes`; the `emptyDir` `sizeLimit`; the memory limit; the feed guard keeps the old version |
| A BODS route rename (a new line, renamed colours) | medium | refused feed | `unmapped_route` refusal plus an alert; a one-line config change |
| The SIRI-SX `LineRef`s drift | high | situations shown at network level only | the alias table, the unmapped counter and its alert |
| Line status read as service quality | medium | misleading UI | "No reported disruption" wording, "Scheduled times, not live" captions, `realtime: false` |
| Transit rows leak into National Rail or TfL lists and stats | medium | wrong dashboards | the §10.5 audit in batch 4, plus tests that each list excludes `source = 'transit'` |
| One timetable part is lost (DLQ or trim) | low | that day's feed is not applied | `incomplete_feed_version` poison plus the dead-letter alert; the previous version keeps serving; the next day retries |

## 16. Open questions for the user

The user answered all but Q7 on 2026-10-09. The answers are binding, as
D1–D7 are.

| # | Question | Answer (2026-10-09) |
|---|---|---|
| Q1 | With no current line-scoped disruption, should a Metrolink line read "No reported disruption" or "Good service"? | **"No reported disruption"** when nothing is published (a new `Severity` variant, §0) |
| Q2 | Should stop-level equipment situations (lift and escalator outages) also show on the line status, or only on the stop and line pages as notices? | **Stop notices**, not line status |
| Q3 | Should replacement-bus trips from the timetable appear on stop boards (labelled "Replacement bus")? | **Yes, on boards, labelled as buses**; not in line timetables |
| Q4 | Should transit lines appear on the home and status dashboard next to National Rail and TfL, or only on `/transit` pages and in the user's pinned lines? | **Own `/transit` pages plus pinned lines only**; not on the home page or the status dashboard. (The brief that relayed the answers numbered this Q9; its wording is this question's.) |
| Q5 | No BODS key is needed today. Register a BODS account anyway, as a named contact and for any later key (Ranma-Config would hold it as a SealedSecret)? | **Register an account**, though no key is needed; add no secret until one is |
| Q6 | Should transit status history feed the half-hourly and daily status rollups (as TfL's does), or stay out of all stats? | **Not in the rollups** (or any stats) |
| Q7 | Who applies to TfGM for live-data access, and should DS apply now so the §15.2 slot can be built when a key arrives? | **Still open.** Nothing is built until a key exists |
| Q8 | The timetable horizon stored: 60 days, or the whole BODS horizon (about 9 months, about 4× the service-day rows but the same trip rows)? | **60 days** |
| Q9 | Navigation: a top-level "Trams" entry, or under the existing Network page? | Taken as answered by Q4's "own `/transit` pages". The nav entry's label and place are settled in batch 3 against the style guide; until then the default stands (under Network, as "Metrolink") |
| Q10 | Should rail-link `nearby` rows (for example Deansgate and Deansgate-Castlefield, or Eccles) show walking minutes, which would be hand-entered? | **No walking minutes**; `interchange` versus `nearby` only |
