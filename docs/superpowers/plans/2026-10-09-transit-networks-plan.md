# Transit networks (Manchester Metrolink first): implementation plan

Spec: [2026-10-09-transit-networks-design](../specs/2026-10-09-transit-networks-design.md).
Read the spec first, §0 especially: it reconciles the design with DS as
of 2026-10-14 (release B, Redis ACLs on, the `Recreate` rollouts, the
style guide), and this plan was updated to match on the same day. Section
numbers (§) below refer to the spec, and "ingest D5" means the ingest
architecture spec's decision D5.

**The goal.** Manchester Metrolink in DS, from the BODS feeds:

- network, line and stop pages;
- scheduled stop boards and a line timetable (phase 1);
- disruptions as line status, with follow and push notifications, and
  links between rail stations and tram stops (phase 2).

All of it uses one producer, `poller-transit-schedule`. Its own stream
`ds:ingest:transit-metrolink` feeds the ingest-writer, which writes the
generic `transit_*` tables. Decisions D1–D7 (spec §14) apply, and so do
the ingest architecture's rules (D7).

## Ground rules for whoever executes this

- **Follow `/home/coder/ds-review/fix-brief-common.md`:**
  - rustc 1.88, with no dependency above it;
  - the Cargo.lock pins (`zip` 8.x, `csv` 1.4 and `quick-xml` 0.41 are
    already in the lockfile, so add no new parser crate);
  - the disk-budget cargo environment and a per-agent `CARGO_TARGET_DIR`
    (the shared sccache);
  - **migration timestamps after the newest existing one** (batch 1:
    `20261014…`), each new file's line added to
    `crates/api/tests/migration_checksums.lock`;
  - index DDL on an **existing** table in its own `-- no-transaction`
    file (an index on a table the same file creates may stay in it);
  - `SET LOCAL lock_timeout = '5s';` in transactional migrations;
  - never edit an existing migration.
- **New tooling is typed stdlib Python**, run with `uv run`, ruff and
  mypy `--strict` clean.
- **Everything ships off by default.** The defaults are:
  - `pollerTransitSchedule.networks.metrolink.enabled: false`;
  - `ingestWriter.streams.transit-metrolink: off`;
  - `redis.acl.clients.pollerTransitMetrolink: false` (and so the
    `optional` ACL user is not rendered, spec §0);
  - `api.transitNetworks: []` (`API_TRANSIT_NETWORKS` empty, so every
    `/public/transit` route 404s and the frontend hides its nav entry).

  List every off-by-default setting in the report.
- **One task, one commit, tests in the same commit.** Commit messages end
  with the `Co-Authored-By` line the session gives.
- **Verification (the user's rule):**
  - **per task:** `cargo fmt --all --check` plus the tests of the crates
    the task touched (`cargo test -p <crate>`; DB-gated and Redis-gated
    tests of those crates when the task touches SQL or Redis). For a
    frontend task, `npm run lint` plus the vitest files touched. For a
    chart task, `helm template` with the task's render flags plus
    `uv run scripts/chart-values-doc.py check`.
  - **once per batch:** clippy `-D warnings` (stable with
    `--all-features`, stable with `--no-default-features`, and +1.88.0),
    the 1.88 check, `cargo test --workspace`, the DB-gated suites on a
    fresh database (as the app role and per service role,
    `scripts/test-postgres-roles.py`), the Redis-gated suites,
    `uv run scripts/lint-scripts.py`, `helm lint` plus the CI render
    flags and `ci/ingest-prereqs.yaml`, the chart check scripts
    (`check-ingest-phase0-chart.py` and friends), the frontend `npm test`
    and `npm run build`, and for a frontend batch `scripts/check-copy.py`
    and the visual baselines (`scripts/visual-tests.py`).
- **Never put a credential in the repo**, including fixtures. There is no
  BODS key (spec §3.1).
- **Never fetch from TfGM's `apiary.tfgm.com` and never scrape a website**
  (D5). Test fixtures are trimmed extracts of the BODS and NaPTAN files,
  under OGL, with a `SOURCE.md` naming the URL and date.
- **Production is read-only for agents.** Flips are Ranma-Config values
  changes made by the user or Ranma.

## Batch overview

| Batch | What | Tasks | Effort (engineer-days) | Ships |
|---|---|---|---|---|
| 1 | Foundations: types, stream, ACL, tables, grants, writer timetable handlers | 1.1–1.6 | 4–5 | off (no producer yet) |
| 2 | The producer and its chart | 2.1–2.7 | 5–6 | off (`enabled: false`) |
| 3 | Read API and frontend (scheduled) | 3.1–3.6 | 3–4 | off (`api.transitNetworks: []`) |
| 4 | Disruptions, line status, notifications | 4.1–4.8 | 4–5 | off (needs `transitNetworks`) |
| 5 | Rail interchange links in the UI, the runbook, the rollout check | 5.1–5.3 | 1–2 | off |
| | **Total** | | **17–22** | |

**The smallest useful first step is batches 1–3**, merged with every
switch off. Flipping the four values of spec §15.1 then gives users
Metrolink pages and scheduled boards. Batch 1 alone is also safe to merge:
it adds tables, a stream declaration and an `optional` ACL line that does
not render until its client is turned on (spec §0).

Batches 1 and 2 can run in parallel after 1.1 (2.x needs only the types).
Batch 3 needs 1.4. Batch 4 needs 1.5 and 2.3. Batch 5 needs 3 and 4.

## Batch 1: foundations

| # | Task | Files | Migrations | Tests | Verify |
|---|---|---|---|---|---|
| 1.1 | **`common::transit` types and ids.** The payload rows for the three schemas (spec §7.2): `TransitNetworkRow`, `TransitLine`, `TransitStop`, `TransitPattern` (with stops), `TransitTrip`, `TransitServiceDay`, `TransitRailLink`, `TimetablePart { network, feed_version, kind, rows }`, `TimetableCommit`, `TransitSituation` (normalised, §10.1). `NetworkId` and `QualifiedLineId` newtypes with the grammar `^[a-z0-9-]+$` and `<network>:<line>`, `parse` and `Display`. The schema names `transit-timetable`, `transit-timetable-commit` and `transit-situations` as constants in `common::transit` (ingest-stream has no schema list; the writer's `STREAMS` maps stream to schemas, task 1.6). `NETWORKS` (the compiled-in network list, `metrolink`) | `crates/common/src/transit.rs`, `crates/common/src/lib.rs` | – | unit: id grammar (accept `metrolink:blue`; refuse `Metrolink:blue`, `metrolink`, `a:b:c`); serde round trips; golden JSON fixtures per schema under `crates/common/tests/fixtures/transit/` | fmt; `cargo test -p common` |
| 1.2 | **The stream, budget and capped parts.** `streams::TRANSIT_METROLINK`; the `StreamDecl` of spec §7.3 (320 a day, 64 KiB, `Covers(24 h)`); the budget test's table gains it, and the total stays under the 512 MB budget. `Snapshot::add_capped(…, max_part_bytes)`: halves a part's rows until its encoded body is at most `max_part_bytes` (64 KiB here), on top of the 512 KiB rule | `crates/ingest-stream/src/{streams.rs,budget.rs,snapshot.rs}` | – | unit: `check_budget` passes with the new stream, and its cover is at least 24 h; `add_capped` splits a 300 KiB fixture into parts of 64 KiB or less, with keys `<schema>:<produced_at>:<i>/<n>` stable across a re-encode; one row larger than the cap is a `TooLarge` error, not a loop | fmt; `cargo test -p ingest-stream` |
| 1.3 | **Redis ACL user.** A new user kind `optional` (spec §0, §13.2): `final` rights, rendered (ACL line and Redis password env) only while `redis.acl.clients.<camelCase user>` is on, in the chart helpers, `scripts/render-redis-acl.py` (`--client <key>`) and the redis_acl test. Add the `poller-transit-metrolink optional …` line; `redis.acl.clients.pollerTransitMetrolink: false` in values (and the README table); `distant-signal.redisAclClientDeployed` maps it to `pollerTransitSchedule.networks.metrolink.enabled`; `check-ingest-phase0-chart.py` checks the default render is unchanged and the client renders the user; `docs/redis-acl.md` | `charts/distant-signal/files/redis-users.acl.tpl`, `charts/distant-signal/templates/{_helpers.tpl,redis-deployment.yaml}`, `charts/distant-signal/values.yaml`, `charts/distant-signal/README.md`, `docs/redis-acl.md`, `scripts/render-redis-acl.py`, `scripts/check-ingest-phase0-chart.py` | – | `crates/common/tests/redis_acl.rs` (ignored, real Redis): the allowed sequence (XADD MAXLEN ~ on its stream, XREVRANGE COUNT 1) and the refused ones (XTRIM, XDEL, XREADGROUP, XADD `ds:ingest:tfl`, GET); `crates/ingest-stream/tests/redis_stream.rs`: a producer run as `poller-transit-metrolink` | fmt; `cargo test -p common --test redis_acl -- --ignored` against local valkey; `helm template` with `redis.acl.enabled=true`; the render-script CI check |
| 1.4 | **Tables.** The migration for spec §8.1–8.4: `transit_networks`, `transit_feed_versions`, `transit_lines`, `transit_stops`, `transit_patterns`, `transit_pattern_stops`, `transit_trips`, `transit_service_days`, `transit_situations` and `transit_rail_links`, with their indexes in the same file (new tables). `db-grants.yaml` classifies every one (spec §8.6) and adds `transit-metrolink-timetable` and `transit-metrolink-situations` to the writer's `ingest_freshness` row policy (spec §0); `postgres-grants.sql` is regenerated | `crates/ds-store/migrations/20261014100000_transit_tables.sql`, `crates/api/tests/migration_checksums.lock`, `charts/distant-signal/files/db-grants.yaml`, `charts/distant-signal/files/postgres-grants.sql` | the expand-only `transit_*` tables (no change to existing tables) | `migration_checksums` locks the new files; `check-migration-order.py`; `gen-db-grants.py check` on a fresh database; `migration_index_locking` | fmt; `cargo test -p ds-store` (DB-gated); `uv run scripts/gen-db-grants.py check --database-url …` |
| 1.5 | **ds-store `transit` module, write side.** `stage_part(conn, part, produced_at)` (inserts with `ON CONFLICT DO NOTHING`, updates `parts_seen`); `commit(conn, commit, produced_at) -> CommitOutcome { Activated, Unchanged, Incomplete(missing), Inconsistent(why), Older }` (spec §9.2: hash short-circuit, part check, consistency checks, version flip, prune of everything older than `previous`, rail-link replace); `record_ingest` with `transit-<network>-timetable` | `crates/ds-store/src/transit/{mod.rs,write.rs}`, `crates/ingest-writer/tests/transit_store.rs` (CI runs ds-store's DB suite as the api role, so the write tests live with the writer's, spec §0) | – | DB, run as the writer role: a two-part feed staged then committed is active; the same commit again is `Unchanged` and writes only freshness; a commit with a missing part is `Incomplete`, and the old version stays active; a third feed prunes the first; an older `produced_at` is `Older` and changes nothing; a trip naming a missing pattern is `Inconsistent`; the writer role's grants allow exactly this | fmt; `cargo test -p ingest-writer --test transit_store` (DB-gated); `scripts/test-postgres-roles.py --mode per-service` with `DATABASE_URL_WRITER` |
| 1.6 | **Writer handlers: timetable.** `handlers/transit.rs` with `stream_network` (from `common::transit::NETWORKS`), `pin_network` (poison `network_not_for_stream`), and the handlers `transit-timetable/1` and `transit-timetable-commit/1`, mapping `CommitOutcome` to `Applied` or `Poison` (`incomplete_feed_version`, `inconsistent_feed_version`); an unchanged or older commit is `Applied` and writes nothing else (a redelivered entry is the runtime's `ingest_dedup` duplicate). `streams::TRANSIT_METROLINK` in the writer's `STREAMS` with the timetable schemas, so `INGEST_WRITER_STREAMS` accepts `transit-metrolink` (`off` or `apply`; release B removed `shadow`). Chart: `ingestWriter.streams.transit-metrolink: "off"`, and `apply` needs `postgresql.roles.perService.writer.connect` as `tfl` does (M4, spec §0) | `crates/ingest-writer/src/handlers/{mod.rs,transit.rs}`, `crates/ingest-writer/src/stream.rs`, `charts/distant-signal/values.yaml`, `charts/distant-signal/templates/_helpers.tpl`, `charts/distant-signal/README.md` | – | `crates/ingest-writer/tests/transit_handlers.rs` (DB plus envelope): a body for another network on `transit-metrolink` is poison before any write; apply activates a fixture feed; an incomplete commit is dead-lettered and the old version stays; a redelivered part is a duplicate | fmt; `cargo test -p ingest-writer` (DB-gated) |

End of batch 1: the batch verification list.

## Batch 2: the producer and its chart

| # | Task | Files | Migrations | Tests | Verify |
|---|---|---|---|---|---|
| 2.1 | **Crate skeleton.** `crates/poller-transit-schedule`: `Config` from env (`TRANSIT_NETWORK`, `TRANSIT_ADAPTER=gtfs`, `TRANSIT_GTFS_URL`, `TRANSIT_GTFS_MAX_BYTES`, `TRANSIT_SITUATIONS_URL`, `TRANSIT_NAPTAN_URL`, intervals, `TRANSIT_HORIZON_DAYS`, `FEED_MAX_DROP_FRACTION`, `TRANSIT_WORK_DIR`, the Redis args); `health-http` probes; the metrics port; the network files `networks/metrolink.toml`, loaded with `include_str!` and validated at startup (spec §6.4: ids, every line has a GTFS route name, colours set); two `SnapshotProducer`s on the network's stream (C3); graceful shutdown. Workspace member, Dockerfile (generated: `scripts/gen-rust-dockerfiles.py`), CI image matrix | `crates/poller-transit-schedule/{Cargo.toml,src/{main.rs,config.rs,network.rs}}`, `networks/metrolink.toml`, `Cargo.toml`, `docker/…`, `scripts/gen-rust-dockerfiles.py`, `.github/workflows/…` | – | unit: config parsing and defaults; an unknown network refuses to start; `metrolink.toml` validates; a network file whose id differs from its file name fails | fmt; `cargo test -p poller-transit-schedule`; `cargo deny check` |
| 2.2 | **GTFS adapter.** Spec §6.2 steps 1–10: a capped download to the work dir; the `agency_noc` filter; the route-name → line map, with replacement-bus flagging and the `unmapped_route` refusal; streamed `trips.txt` and `stop_times.txt`; `shapes.txt` never opened; service-day expansion in London time from yesterday to today plus the horizon; pattern folding; per-line station order; the `940G` → `9400` station-id rule. The output is an in-memory `TransitModel` | `src/adapter/{mod.rs,gtfs.rs}`, `src/model.rs`, `tests/fixtures/bods-nw-metl-mini.zip` (a trimmed extract: the METL agency, 3 routes including 1 replacement bus, about 40 trips, the stops, the calendar, and one other operator's rows to prove the filter) plus `SOURCE.md` | – | unit, on the fixture: other operators are excluded; replacement-bus trips are flagged and have no line; patterns dedupe; times over 24:00 survive; a `calendar_dates` removal drops a day; an unmapped route refuses; a download over the cap aborts; a zip entry over its cap aborts; peak memory on a synthetic 50 MB `stop_times` stays bounded (a streaming assertion: rows are processed without collecting the file) | fmt; `cargo test -p poller-transit-schedule` |
| 2.3 | **Timetable loop and feed guard.** NaPTAN enrichment (spec §6.3; fails soft and is counted); rail links from `reference-data/transit/metrolink-rail-links.csv` (the ten interchanges of spec §3.4, with station ATCO codes from NaPTAN); the feed guard with a baseline from the stream's last commit (`XREVRANGE` for the newest `transit-timetable-commit/1`); `feed_version` = source version plus the content hash; parts through `add_capped` (64 KiB) then the commit, as one item; skip the publish when the hash equals the last commit's and it is under 24 h old; the `transit_feed_*` metrics | `src/{timetable.rs,naptan.rs,rail_links.rs,feed_guard.rs}`, `reference-data/transit/metrolink-rail-links.csv`, `tests/fixtures/naptan-940-mini.csv` | – | unit: NaPTAN names and indicators applied, and the suffix-strip fallback; the rail-links CSV check (spec §6.3: every pair in `crs-tiploc.csv`, no `PseudoCode` or `NotAStation` CRS, each `stop_id` a `MET` row); guard refusals (empty, drop over 30%, no service today) keep the previous version; the encoded item ends with the commit and every part is 64 KiB or less; Redis-gated: the baseline is read back from the stream after a restart | fmt; `cargo test -p poller-transit-schedule` (plus `--ignored` Redis) |
| 2.4 | **SIRI-SX adapter (producer side only; the writer handler is 4.2).** Download the bulk archive, following the 302 to the signed URL, with `If-None-Match`, a 50 MB cap and a zip-entry cap; stream-parse with `quick-xml`; filter and normalise (spec §10.1); publish `transit-situations/1` every 5 min through its own producer. Writer stream mode stays `off`, so nothing consumes it until batch 4 | `src/adapter/sirisx.rs`, `src/situations.rs`, `tests/fixtures/sirisx-mini.xml` (TfGM lift, METL line-scoped, AllLines plus stops, Supertram `9400ZZSY`, a bus situation) plus `SOURCE.md` | – | unit: only the TfGM tram situations survive (Supertram and bus excluded); `BLUE`, `PURP`, `Green_Line` and `Navy_Line` map through the aliases; an unmapped ref is counted and kept raw; a platform code folds to its station; the output is sorted and stable; a 304 publishes nothing new | fmt; `cargo test -p poller-transit-schedule` |
| 2.5 | **Chart: values and Deployment.** `pollerTransitSchedule` (spec §13.1); `templates/poller-transit-schedule-deployment.yaml` ranging over enabled networks: env, `emptyDir` `/work` with `sizeLimit`, `readOnlyRootFilesystem`, probes (`progressStallSecs` 3600), resources, `redisClientAuthEnv` for client `pollerTransit<Network>`; `strategy: Recreate` from creation (spec §0); the render checks through `distant-signal.requireStreamApplied` (its own ACL user, and the writer applying `transit-<network>`); the component in the known-components list and `check-pod-labels-chart.py`; `chart-values-doc` entries | `charts/distant-signal/values.yaml`, `charts/distant-signal/templates/poller-transit-schedule-deployment.yaml`, `charts/distant-signal/templates/_helpers.tpl`, `charts/distant-signal/templates/tests/…` | – | `helm template` render tests: nothing renders by default; `enabled=true` renders one Deployment named `…-poller-transit-schedule-metrolink` with the right env and volume; each render check fails as intended; the values doc check | fmt (n/a); `helm template` with the task flags; `uv run scripts/chart-values-doc.py check` |
| 2.6 | **Chart: NetworkPolicy, alerts, budget maps.** The worker policy (monitoring ingress; egress `redis` plus the four URLs, spec §13.3); redis ingress admits the component; the `DistantSignalTransitFeedRefused` and `DistantSignalTransitUnmappedLineRef` alerts, rendered only for enabled networks, with their `docs/alerts.md` sections; staleness through the existing `ingestSourceStale` maps (`maxAgeSeconds` and `whileStreamApplies`, spec §13.4); the per-stream maps of the ingest alerts gain `ds:ingest:transit-metrolink` (`MAXLEN` 320, cadence 300 s) | `charts/distant-signal/templates/{networkpolicy.yaml,prometheusrule.yaml}`, `values.yaml`, `docs/alerts.md` | – | `crates/ingest-stream/tests/chart_values.rs`: the maps equal `budget.rs`; netpol render test: with the network on, egress has redis and 443 and no postgres or api; alert render test; `helm lint` | `cargo test -p ingest-stream --test chart_values`; `helm template` with the CI flags; `uv run scripts/check-alert-payloads.py` |
| 2.7 | **CI wiring.** The image build and publish, the crate in the 1.88 check, `check-crate-deps.py` (the producer must not depend on `sqlx` or `ds-store`) | `.github/workflows/*.yml`, `scripts/check-crate-deps.py` config | – | `uv run scripts/check-crate-deps.py` passes, and fails if `sqlx` is added | the CI scripts locally |

End of batch 2: the batch verification list, plus a **local end-to-end
run**. Use docker compose or a kind cluster with valkey and Postgres.
Point `TRANSIT_GTFS_URL` at a local copy of the real North West zip
(downloaded once into the scratchpad, never committed), with the writer on
`apply` (there is no shadow mode). Record in the batch report:

- the parse time and peak RSS;
- the part count and sizes;
- the row counts against spec §3.2.

## Batch 3: read API and frontend (scheduled)

| # | Task | Files | Migrations | Tests | Verify |
|---|---|---|---|---|---|
| 3.1 | **ds-store read queries.** `list_networks`, `get_network`, `list_lines`, `get_line` (stations per direction), `list_stops`, `get_stop` (platforms, lines serving, rail links), `stop_departures(network, station, date, after, limit, cursor)` (spec §11: the union of platforms; no pickup-forbidden or last-stop rows; the D−1 overflow past midnight; London time), and `line_timetable(network, line, date, direction, from, to)`. All read the active version | `crates/ds-store/src/transit/read.rs` | – | DB, as the api role: the board for a fixture feed in order across midnight; a replacement-bus row is flagged; an `after` cursor pages without gaps or duplicates; a date outside the horizon gives empty plus `outsideHorizon`; activation of a new version switches results atomically (read in a transaction during the flip) | fmt; `cargo test -p ds-store` (DB-gated) |
| 3.2 | **API routes.** `routes/transit.rs` with the routes of spec §11 under `/public/transit`; `API_TRANSIT_NETWORKS` (chart `api.transitNetworks`, default `[]`) gating with 404; `Cache-Control` per route; `realtime: false` and `kind: "scheduled"` on boards; the public rate-limit group and route-metrics table entries | `crates/api/src/routes/{mod.rs,transit.rs}`, `crates/api/src/config.rs`, `crates/api/src/rate_limit.rs`, `crates/api/src/route_metrics.rs`, the chart's api env and values | – | api tests: every route 404s while the network is not configured; the shapes are snapshot-tested against a fixture feed; a board row has no `expected`, `delay` or `cancelled` field; `{line}` accepts `blue` and refuses `metrolink:blue` in the path; the route table test (no `/private`, MCP group checks) | fmt; `cargo test -p api` (DB-gated where needed) |
| 3.3 | **API changelog and docs.** A `docs/api-changelog.md` entry (newest first): the routes, `realtime: false`, the attribution field and sentence, the 404-until-enabled behaviour | `docs/api-changelog.md` | – | – (docs) | read through |
| 3.4 | **Frontend: network and line pages.** `app/transit/[network]/page.tsx` and `app/transit/[network]/lines/[line]/page.tsx`; the nav entry shown only when `/public/transit/networks` is non-empty; types in `lib/types.ts`; line colour chips | `frontend/app/transit/**`, `frontend/lib/{types.ts,api.ts}`, `frontend/components/AppNavBar.tsx` | – | vitest: the pages render the fixture responses; the nav is hidden for an empty list; a 404 network renders not-found; axe checks as the other page tests do | `npm run lint`; vitest of the touched files |
| 3.5 | **Frontend: stop board and line timetable.** `TransitStopBoard` (maps rows onto `ServiceRow`, `ServiceRowList`, `ServiceListNotice` and `LoadMoreControl`; the caption "Scheduled times, not live" (no em dash, spec §0); a replacement-bus label); `app/transit/[network]/stops/[stop]/page.tsx`; `app/transit/[network]/lines/[line]/timetable/page.tsx` (a grid with date, direction and window controls, scrolling horizontally in its own container) | `frontend/components/{TransitStopBoard.tsx,TransitLineTimetable.tsx}`, `frontend/app/transit/**` | – | vitest: paging, the empty and outside-horizon states, the caption always present, a row with no status or delay text; an e2e smoke test in `frontend/e2e` against the mocked API | `npm run lint`; vitest of the touched files |
| 3.6 | **Attribution.** The footer line when a network is enabled; a `/attribution` block (spec §12.3); the licence-basis comment in `OpenDataAttribution.tsx` | `frontend/components/OpenDataAttribution.tsx`, `frontend/app/attribution/page.tsx` | – | vitest: the OGL line shows only with a network enabled; the attribution page lists BODS, NaPTAN, the OGL link and the non-affiliation line; `legalPages.test.tsx` still passes | `npm run lint`; vitest of the touched files |

Every batch 3 and 4 frontend task follows docs/style-guide.md (spec §0):
sentence case, no em dashes, ellipses or arrows, `describeFailure` for
failures, no inline styles, the shared components and tokens. Its copy
passes `scripts/check-copy.py` and the ESLint copy rules, and each new
page gets a visual-regression baseline in `frontend/e2e/visual.spec.ts`
(`uv run scripts/visual-tests.py --update`, images reviewed and
committed).

End of batch 3: the batch verification list, plus `npm run build` and the
e2e smoke test. Phase 1 is now complete and still off.

## Batch 4: disruptions, line status, notifications

| # | Task | Files | Migrations | Tests | Verify |
|---|---|---|---|---|---|
| 4.1 | **`line_status` for transit: constraint and row policy.** The CHECK of spec §8.5 (`NOT VALID`, then `VALIDATE` in a second file); `db-grants.yaml` `row_policies.line_status.writer` becomes `source IN ('tfl', 'transit')` (the `ingest_freshness` names went in with 1.4, spec §0); `postgres-grants.sql` regenerated | `crates/ds-store/migrations/<assigned>_line_status_transit_check.sql`, `<assigned>_line_status_transit_check_validate.sql`, `db-grants.yaml`, `postgres-grants.sql` | the CHECK constraint (expand-only) | `crates/ingest-writer/tests/line_status_rls.rs` gains: as the writer, a transit row is allowed and an `aggregator` row is refused; the CHECK refuses `source='transit'` with an unqualified id; the freshness RLS test covers the new names; `migration_checksums` | fmt; `cargo test -p ingest-writer` (DB-gated, per-service role mode) |
| 4.2 | **Status derivation (pure).** `common::transit::status::derive(situations, lines, line_stops, now) -> Vec<LineStatusRow>` implementing spec §10.2 (current or upcoming; the scope rules; the condition and severity table; worst wins; "No reported disruption"; Information never degrades) and producing `statuses` as `Vec<common::LineStatus>`. Adds `Severity::NoReportedDisruption = 27` and `DataQuality::Transit` (spec §0), with `frontend/lib/severity.ts` and the display labels | `crates/common/src/transit/status.rs`, `crates/common/src/lib.rs`, `frontend/lib/severity.ts` | – | a unit table test per row of both spec tables; the lift-failure fixture leaves every line at "No reported disruption" with a stop notice; AllLines plus stops with `suspended` gives Part suspended on the lines serving those stops; a future validity goes to `upcoming`; deterministic output order | fmt; `cargo test -p common` |
| 4.3 | **ds-store and writer: situations.** `ds_store::transit::apply_situations(conn, network, situations, produced_at)` (spec §9.3: guarded upsert, the prune of absent situations, `derive`, the `line_status` upsert with `source='transit'`, history only on change, the scoped prune, `record_ingest`); the `transit-situations/1` handler with network pinning (a `line_status` id outside the network prefix is poison) | `crates/ds-store/src/transit/situations.rs`, `crates/ingest-writer/src/handlers/transit.rs` | – | DB, as the writer: a snapshot writes situations and line rows; the same snapshot again writes no history; an older snapshot after a newer one changes nothing; a removed situation is deleted and the line returns to "No reported disruption" with one history row; aggregator and TfL rows are untouched by the prune; `computed_at` equals `produced_at` | fmt; `cargo test -p ds-store -p ingest-writer` (DB-gated) |
| 4.4 | **Audit of `line_status` readers.** Every query reading `line_status` or `line_status_history` without a `source` or mode filter, in `api` (status dashboard, `lines.rs`, operator overview and history, the MCP-facing paths), the aggregator (rollups and coverage stats) and `ds-store`: exclude `source = 'transit'` where National Rail or TfL semantics apply (spec §10.5); record each reader and its verdict in the commit message | the files found by `grep -rn 'line_status' crates/{api,aggregator,ds-store}/src` | – | per changed reader, a DB test with one transit row present that the list or stat is unchanged; `/public/lines/metrolink:blue` (the National Rail line route) is 404 | fmt; `cargo test -p api -p aggregator -p ds-store` (DB-gated) |
| 4.5 | **API: status and situations.** Network and line responses gain `status`, with "Status unavailable" plus `asOf` when the situations freshness is over 30 min; `GET /public/transit/{network}/situations`; stop responses gain `notices`; changelog entry | `crates/api/src/routes/transit.rs`, `crates/ds-store/src/transit/read.rs`, `docs/api-changelog.md` | – | api tests: fresh, stale and missing freshness; notices on the stop; situations listing ordered by validity | fmt; `cargo test -p api` |
| 4.6 | **Follow and pin.** `preferences.rs`: `filter_known_pinned_lines` accepts `source = 'transit'` ids from `line_status` (as for TfL); the frontend line page gets the follow star | `crates/api/src/routes/preferences.rs`, `crates/api/src/data/queries.rs` (a `transit_line_ids` query), `frontend/app/transit/[network]/lines/[line]/page.tsx` | – | api test: a pinned `metrolink:blue` survives the read, and an unknown `metrolink:nope` is dropped; vitest for the star | fmt; `cargo test -p api`; vitest |
| 4.7 | **Notifier.** Transit line changes are pushed for pinned lines; the deep link resolves `<network>:<line>` to `/transit/<network>/lines/<line>`; "Information" is never pushed; the stale-history skip applies (an existing test, extended) | `crates/notifier/src/{queries.rs,line_notifications.rs or equivalent}` | – | DB: a history change Minor delays → No reported disruption on a pinned transit line queues one push with the transit URL; Information to Information is not pushed; a stale row is skipped and counted | fmt; `cargo test -p notifier` (DB-gated) |
| 4.8 | **Frontend: status and disruptions.** Line chips with status on the network page; a disruption list; the line page's status and situations; stop notices on stop pages; the "Status unavailable" state | `frontend/app/transit/**`, `frontend/components/TransitDisruptions.tsx` | – | vitest: each status state; the description renders as text (no HTML); the `upcoming` section | `npm run lint`; vitest |

End of batch 4: the batch verification list. Phase 2's disruption half is
complete and still off.

## Batch 5: rail links in the UI, runbook, rollout check

| # | Task | Files | Migrations | Tests | Verify |
|---|---|---|---|---|---|
| 5.1 | **Rail interchange links.** `GET /public/transit/rail-links?crs=` (all enabled networks; empty when none); the National Rail station page shows "Metrolink stop: <name>" linking to the stop page; the transit stop page links to `/stations/<crs>`; changelog entry | `crates/api/src/routes/transit.rs`, `crates/ds-store/src/transit/read.rs`, `frontend/app/stations/[crs]/page.tsx`, `frontend/app/transit/[network]/stops/[stop]/page.tsx`, `docs/api-changelog.md` | – | api: MAN returns Piccadilly; a CRS with no link returns empty; nothing is returned while the network is disabled; vitest for both pages | fmt; `cargo test -p api`; vitest |
| 5.2 | **Runbook.** `docs/transit-networks.md`: the rollout order and rollback (spec §15.1); inspecting the stream (`XRANGE`/`XREVRANGE`) and the dead letters (`incomplete_feed_version`, `network_not_for_stream`, with re-injection per `docs/ingest-streams-deadletter.md`); forcing a timetable refresh (restart the pod); adding a SIRI line alias; switching to the TfGM GTFS fallback and its attribution; **adding a network** (network file, stream constant and budget, ACL line, values, alerts, freshness names in `row_policies`, attribution); the cleanup SQL for turning a network off | `docs/transit-networks.md`, links from `docs/redis-acl.md` and `docs/alerts.md` | – | – (docs) | read through; the links resolve |
| 5.3 | **Rollout dry run (local).** On the batch 2 local stack, run the spec §15.1 order end to end: the ACL client, apply plus the producer, the checks, enable the api, then the rollback. Record the observed metrics and any gap in the batch report; fix gaps as follow-up tasks, not here | – | – | the manual checklist in the runbook, executed | the report |

End of batch 5: the batch verification list.

## Off-by-default settings this plan adds

- `pollerTransitSchedule.networks.metrolink.enabled` (`false`);
- `ingestWriter.streams.transit-metrolink` (`off`);
- `redis.acl.clients.pollerTransitMetrolink` (`false`);
- `api.transitNetworks` / `API_TRANSIT_NETWORKS` (`[]`, so every
  `/public/transit` route 404s and the frontend hides the nav entry).

Settings that take effect at once but change nothing for existing data:

- **The ACL user `poller-transit-metrolink`** (an `optional` line: not
  rendered until `redis.acl.clients.pollerTransitMetrolink` is on, so
  merging restarts nothing; spec §0).
- **The `transit_*` tables**, which stay empty.
- **The `line_status` CHECK**, which no existing row violates.
- **The writer's row policy widened to `IN ('tfl','transit')`**, which
  allows nothing new until a transit stream is applied.
- **The writer's `ingest_freshness` row policy** gains the two
  `transit-metrolink-*` sources (batch 1).
- **`INGEST_WRITER_STREAMS` gains `transit-metrolink:off`**, so the
  ingest-writer restarts once on the deploy that adds it (an env
  change).

## Effort summary

| Batch | Days |
|---|---|
| 1 Foundations | 4–5 |
| 2 Producer and chart | 5–6 |
| 3 API and frontend (scheduled) | 3–4 |
| 4 Disruptions and notifications | 4–5 |
| 5 Rail links, runbook, dry run | 1–2 |
| **Total** | **17–22** (spec §15.1), plus soak between the flips |
