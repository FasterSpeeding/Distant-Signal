# Ingest architecture, phase 5: removing `/private` and locking down

This runbook prepares phase 5 of
[the ingest architecture plan](superpowers/plans/2026-10-06-ingest-architecture-plan.md)
(spec: [design](superpowers/specs/2026-10-06-ingest-architecture-design.md),
§13, §15.1, §6.4 and §16). It was written on 2026-10-08 against `main` at
`dc2b4405`. File and line references are to that commit.

**Phase 5 code starts only once every producer runs off `/private` in
production** (user decision). Nothing in this document is built yet. It
lists, for each step:

- the evidence needed to start;
- what to delete, file by file;
- what Ranma changes in Ranma-Config;
- the checks and the rollback.

On 2026-10-08 every phase 1–4 switch is built and still **off** in
production, except phase 1B (migrate Job, writer loops, api-maintenance),
which is live. Every producer still calls `/private` today (§1.2).

Agents only read production. Every flip and every Ranma-Config change below
is made by the user or by Ranma.

## Contents

1. [Entry criteria and the `/private` inventory](#1-entry-criteria-and-the-private-inventory)
2. [Steps 5.1–5.7](#2-steps-5157)
3. [The api's Redis client (5.4)](#3-the-apis-redis-client-54)
4. [Final grants (5.5) and NetworkPolicies (5.6)](#4-final-grants-55-and-networkpolicies-56)
5. [Risks and open questions](#5-risks-and-open-questions)

## 1. Entry criteria and the `/private` inventory

### 1.1 The routes

`crates/api/src/routes/mod.rs::private_router` merges `ingest::router()`
(28 paths, `crates/api/src/routes/ingest.rs`) and `samples::router()`
(`/sample-stations`, `crates/api/src/routes/samples.rs`). That makes
**29 paths and 44 path-and-method pairs**. The spec's 27/44 predates
`/tiploc-locations` and `/schedule-services`.

`crates/api/src/main.rs` nests them under `/private`, behind
`require_internal_oauth` (`crates/api/src/auth.rs`). That middleware checks
each pair against `internal_oauth_route_table` (`crates/api/src/app.rs`).
`api::route_metrics::register` pre-registers every pair at 0, so each pair
has a series even with no traffic.

### 1.2 Who calls each route today

The callers below come from each crate's `config.rs` defaults and the
chart's `*_URL` env. The 7-day counts come from production Prometheus,
read-only, for the 7 days to 2026-10-08 (query in §1.3).

| Route | Method | Caller (crate, env) | 7 days to 2026-10-08 | Moves off with (Ranma value, plan task) |
|---|---|---|---|---|
| `/schedule-line-population` | GET | full-coverage-consumer (`SCHEDULE_LINE_POPULATION_URL`) | 1,369,412 | `fullCoverageConsumer.internalReads.source: db` (4.3) |
| `/trust-event-backlog` | POST | trust-backlog-consumer (`API_INGEST_URL`) | 475,157 | `trustBacklogConsumer.ingest.sink: db` (3b.1) |
| `/tracked-trains` | GET | trust-consumer (`API_TRACKED_TRAINS_URL`) | 29,111 | `trustConsumer.internalReads.source: db` (4.4) |
| `/train-reasons` | POST | trust-backlog-consumer (derived from `API_INGEST_URL`) | 28,716 | `trustBacklogConsumer.ingest.sink: db` (3b.1) |
| `/sample-stations` | GET | poller-ldbws (`API_SAMPLE_STATIONS_URL`) | 9,721 | `pollers.ldbws.internalReads.source: db` (4.5) |
| `/station-samples` | POST | poller-ldbws (`API_INGEST_URL`) | 9,675 | `pollers.ldbws.ingest.sink: stream` with writer `streams.station-samples: apply` (3a.7) |
| `/station-samples` | GET | poller-ldbws (startup cursor) | 910 | same; the cursor becomes `XREVRANGE` |
| `/full-coverage-stats` | POST | full-coverage-consumer (`FULL_COVERAGE_STATS_URL`) | 9,589 | `fullCoverageConsumer.ingest.sink: stream` with writer `streams.full-coverage: apply` (3a.8) |
| `/full-coverage-window-stats` | POST | full-coverage-consumer (`FULL_COVERAGE_WINDOW_STATS_URL`) | 9,589 | same |
| `/station-full-coverage-samples` | POST | full-coverage-consumer (`STATION_FULL_COVERAGE_STATS_URL`) | 9,206 | same |
| `/schedule-line-population` | POST | schedule-reference (`SCHEDULE_LINE_POPULATION_URL`) | 3,332 | `scheduleFeed.reference.ingest.sink: db` (2a) |
| `/tfl-line-status` | POST | poller-tfl (`API_INGEST_URL`) | 2,101 | `pollers.tfl.ingest.sink: stream` with writer `streams.tfl: apply` (3c.2), after the notifier skip (3c.4) |
| `/tfl-line-status` | GET | poller-tfl (startup cursor) | 834 | same |
| `/incidents` | POST | poller-incidents (`API_INGEST_URL`) | 2,094 | `pollers.incidents.ingest.sink: db` (2c) |
| `/incidents` | GET | poller-incidents (startup cursor) | 840 | same; the cursor is read from `ingest_freshness` |
| `/stanox-crs` | GET | trust-consumer, full-coverage-consumer, trust-backlog-consumer (`STANOX_CRS_URL`) | 558 | each reader's `internalReads.source: db` (4.3, 4.4); the backlog consumer's `ingest.sink: db` (3b.1) |
| `/stanox-crs` | POST | schedule-reference (`API_INGEST_URL`) | 1 | `scheduleFeed.reference.ingest.sink: db` (2a) |
| `/schedule-calling-points-full` | POST | schedule-reference | 475 | same |
| `/schedule-destination-departures` | POST | schedule-reference | 280 | same |
| `/schedule-services` | POST | schedule-reference | 8 | same |
| `/schedule-network-departures` | POST | schedule-reference | 1 | same |
| `/fixed-links` | POST | schedule-reference | 1 | same |
| `/tiploc-crs` | POST | schedule-reference | 1 | same |
| `/tiploc-locations` | POST | schedule-reference | 1 | same |
| `/schedule-reference-publishes` | GET | schedule-reference (its restart marker) | 7 | same |
| `/schedule-reference-publishes` | POST | schedule-reference | 1 | same |
| `/train-events` | POST | trust-consumer (`API_INGEST_URL`) | 36 | `trustConsumer.ingest.sink: db` (3b.3, D1); needs `ingestWriter.loops.enabled` |
| `/train-forward-signals` | POST | trust-consumer (`FORWARD_SIGNALS_URL`) | 36 | same |
| `/schedule-feed-ingests` | POST | schedule-ingest (`API_INGEST_URL`) | 4 | `scheduleFeed.ingest.sink: db` (2d) |
| `/stations` | POST, GET | poller-stations | 2, 2 | `pollers.stations.ingest.sink: db` (2b) |
| `/tocs` | POST, GET | poller-tocs | 2, 1 | `pollers.tocs.ingest.sink: stream` with writer `streams.reference: apply` (3c.2) |
| `/corpus-locations` | POST | schedule-ingest's CORPUS mode (`CORPUS_API_URL`) | 0 (monthly) | `scheduleFeed.ingest.sink: db` (2d) |
| `/full-coverage-stats`, `/full-coverage-window-stats`, `/station-full-coverage-samples` | GET | **no caller in the code** | 0 | nothing to move |
| `/schedule-feed-ingests` | GET | **no caller in the code** (schedule-reference's grant was removed 2026-09-25) | 0 | nothing to move |
| `/island-of-ireland-{stations,lines,station-samples}` | GET, POST | **no caller**: since 3c.2 (D8) the three pollers are stream-only and disabled | 0 | nothing to move |

`/{unmatched}` saw 19 `GET` 404s in the same 7 days.

**Not ingest, and must keep working.** The Distant-Signal-MCP (Ranma
namespace `ds-mcp`) never calls `/private`. It calls the public routes at
`http://distant-signal-api.distant-signal.svc.cluster.local:8080` with a
client-credentials token for `srv-ds-mcp`, issued by the same Authentik
provider as the producers' tokens (`distant-signal-internal`).

The api verifies that token with `AppState::internal_oauth_verifier` (JWKS
discovered from `api.internalOauth.issuerUrl`). It uses it only to move the
MCP onto its own rate-limit budget: `api::rate_limit::ServiceCallerAuth`,
group `api.internalOauth.groups.mcp`, default `srv-ds-mcp`.

Phase 5 must therefore keep all of the following:

- in DS:
  - `crates/api/src/auth/internal_oauth.rs`;
  - `rate_limit::ServiceTokenCheck`;
  - the `INTERNAL_OAUTH_ISSUER_URL`, `INTERNAL_OAUTH_CLIENT_ID` and
    `INTERNAL_OAUTH_GROUP_MCP` env on the api;
  - the api's egress to the issuer;
- in Ranma-Config:
  - the Authentik `distant-signal-internal` provider and application;
  - the `srv-ds-mcp` account and group, and its
    `authentik-svc-accounts-secret` key `INTERNAL_OAUTH_PASSWORD_DS_MCP`;
  - `allow-ingress-api-from-ds-mcp`.

The enricher's Authentik use (`ds-openai-enricher`, keyless OpenAI) is a
separate provider and is unaffected. No frontend, script or Ranma workload
calls `/private`. `docker-compose.yml` and the two `*.env.example` files do:
they wire every local producer to `/private` (see 5.3).

### 1.3 The evidence, per route and per producer

**A. Zero `/private` traffic, per pair.** Run this read-only, through the
kube API service proxy or Grafana:

```promql
sum by (exported_endpoint, method) (
  increase(distant_signal_http_requests_total{
    namespace="distant-signal", exported_endpoint=~"/private/.*"}[7d]))
```

Every one of the 44 rows must be 0. A missing row means that api pod
generation never registered the pair, which is not the same as 0, so check
that all 44 rows are present. The label is `exported_endpoint` behind the
PodMonitor (`metrics.prometheusRule.api5xx.routeLabel`).

**Prometheus keeps about 8 days** (`time() -
min(prometheus_tsdb_lowest_timestamp_seconds)` was 8 days on 2026-10-08).
A 7-day window is therefore right at the edge, and the spec's "every switch
on its new value for 14 days" cannot come from Prometheus. Take the switch
dates from Ranma-Config's `git log` on `clusters/mine-bringer/apps/distant-signal.yaml`.

**B. The rare routes need more than a zero count.** Several routes are
called so rarely that 7 quiet days prove nothing:

- `/corpus-locations` is monthly;
- schedule-reference posts once per published delivery (1 in the last 7
  days);
- `/stations` and `/tocs` run daily (2 each in 7 days).

For these, the evidence is that the producer has done the same work on its
new path at least once since the flip:

| Producer | Evidence on the new path |
|---|---|
| schedule-reference | `distant_signal_db_writes_total{outcome="ok"}` from the schedulefeed pod rising since the flip; a new `schedule_reference_publishes` row since the flip (`SELECT delivery, completed_at FROM schedule_reference_publishes ORDER BY completed_at DESC LIMIT 3`); `pg_stat_activity.usename = 'distant_signal_schedule_reference'` while it publishes |
| schedule-ingest (CIF markers and CORPUS) | a `schedule_feed_ingests` row since the flip; **a `corpus_deliveries` row since the flip** (one monthly CORPUS load on `db`), or an explicit decision to accept the config alone (Q7) |
| poller-stations | `ingest_freshness` row `source = 'stations'` advancing daily; `db_writes_total` from the poller pod |
| poller-incidents | `ingest_freshness` `source = 'incidents'` advancing every 5 min; `XLEN incident-text-changed` still growing (now from the poller) |
| poller-tocs, poller-tfl, poller-ldbws, full-coverage-consumer | `distant_signal_ingest_stream_produce_total{stream=…, outcome="ok"}` rising; the writer's `distant_signal_ingest_stream_consumed_total{stream=…, outcome="applied"}` rising; no `DistantSignalIngestStreamStalled`, `…Backlog` or `…DeadLetters` alert for 7 days |
| trust-backlog-consumer, trust-consumer | `db_writes_total` from their pods; `trust_event_backlog` rows per hour near the ~30k/h baseline; `train_event_outbox` drained (`DistantSignalTrainEventOutboxStuck` silent) |
| readers (full-coverage-consumer, trust-consumer, poller-ldbws) | the deployed env says `*_SOURCE=db`; `pg_stat_activity.usename` shows `distant_signal_full_coverage_ro`, `distant_signal_trust_consumer`, `distant_signal_ldbws_ro` |

**C. The deployed config.** For each Deployment, read the env (names only,
no Secret values):

```sh
kubectl -n distant-signal get deploy -o json | jq -r '
  .items[] | .metadata.name as $d | .spec.template.spec.containers[] |
  .env[]? | select(.name | test("INGEST_SINK|_SOURCE$|API_.*URL|_URL$")) |
  "\($d) \(.name)=\(.value // "<from secret>")"'
```

Every `INGEST_SINK` must be `db` or `stream` (never `http` or
`http+shadow`), and every `*_SOURCE` must be `db`. The writer's
`INGEST_WRITER_STREAMS` must be `apply` for `station-samples`,
`full-coverage`, `tfl` and `reference`. `island-of-ireland` stays `off`
while those pollers are disabled (D8).

**D. Off the api's Redis.** The api's only Redis use is the `XADD
incident-text-changed` inside `POST /private/incidents` (§3). The evidence:

- `/private/incidents` POST is at 0 (A);
- poller-incidents is on `db`, which publishes the stream itself;
- with `redis.acl` on, `redis-cli --user ds-admin CLIENT LIST` shows no
  `user=api` connection, and `ACL LOG` shows nothing for `api`;
- with `redis.acl` off (production on 2026-10-08), the api pod's IP is
  absent from `CLIENT LIST`. The api connects per publish, so check this
  over a day, or rely on A.

**E. The prerequisites phase 5 assumes but its entry list does not name.**

- **0b is live**: `postgresql.roles.perService.enabled` and `connect: true`
  for api, aggregator, enricher, notifier and writer. Also, the 7-day
  `observe-role-usage.py` report exists. 5.5 narrows from it. In production
  on 2026-10-08, `perService` is off.
- Every direct writer and reader also connects as its own role.
  `distant_signal_app` cannot be dropped while anything still connects as
  it.
- 1B is live, so the api runs no loops and no migrations. Production
  already has `api.backgroundLoops: false` and `migrateOnStartup: false`.
- 0c is optional for phase 5. With `redis.acl` on, 5.4 also deletes the
  `api` ACL user; with it off, 5.4 only takes the shared password away
  from the api.

**Phase 5 may start when:**

- A, B and C hold;
- D holds;
- the switch dates in Ranma's git history are 14 days old;
- E's first two bullets hold.

## 2. Steps 5.1–5.7

Order: **5.1 → 7-day soak → 5.2 + 5.3 → 5.4 → 5.5 → 5.6**. 5.7 can run at
any point after 5.2. The constraints:

- **5.3 must not ship before the 5.1 soak ends.** Deleting the producers'
  HTTP sinks removes the per-producer rollback (`sink: http`). After that,
  `API_PRIVATE_ROUTES=true` alone restores nothing.
- **5.2 before 5.5.** Narrowing the api role while the ingest handlers
  still exist would make a 5.1 rollback fail on permissions instead of
  working.
- **5.4 after 5.2.** The only Redis caller is `post_incidents`.
- **5.6's api-ingress narrowing after 5.3 is deployed**, so no pod still
  holds an HTTP path. Its api-to-Redis cut comes after 5.4.
- **Authentik decommissioning (Ranma) after 5.3 has run for a day.**
- **Dropping `distant_signal_app` is the last DB change.** Do it in its own
  release, 7 days after the narrowing (Q5).

### 5.1 `API_PRIVATE_ROUTES` and the 7-day soak

**Code (one commit):**

- `crates/api/src/data/config.rs`: `private_routes: bool`, env
  `API_PRIVATE_ROUTES`, default `true`.
- `crates/api/src/main.rs`: when it is false, do not `.nest("/private",
  …)`. **Correction to the plan:** do not just skip the nest. A skipped
  nest turns each request into a `/{unmatched}` 404, and the soak can then
  no longer see which route a straggler called. Instead, nest a fallback
  router under `/private` that answers 404 and increments
  `distant_signal_api_private_disabled_total{route}`. Use the matched
  pair's label from the existing table, so cardinality stays bounded (44
  values plus `other`), and log the caller's `sub`/`azp` claim if a
  bearer is present. Also skip `route_metrics::register`'s private pairs.
- `charts/distant-signal/values.yaml`: `api.privateRoutes.enabled: true`.
  `templates/api-deployment.yaml`: `API_PRIVATE_ROUTES`. README via
  `chart-values-doc.py`.
- Tests: `/private/*` is 404 when off (and counted); unchanged when on; the
  public routes are unchanged either way.

**Ranma:** `api.privateRoutes.enabled: false`, one release, then 7 days.

**Checks during the soak:**

- `sum(increase(distant_signal_api_private_disabled_total[1d])) == 0`
  every day;
- `DistantSignalApiPublic5xx` is silent;
- the MCP's budget still applies: the api logs "MCP service caller
  recognised" at startup, and the MCP's 429 rate is unchanged;
- every alert from §1.3 B stays silent.

**Rollback:** `api.privateRoutes.enabled: true`, then put the affected
producer back on `sink: http` / `source: http`.

### 5.2 Delete the api's `/private` code

**Code. Delete:**

- `crates/api/src/routes/ingest.rs` (whole file; 3,668 lines including
  its tests) and `crates/api/src/routes/samples.rs`. Check first that
  `data::samples` keeps no other user; the public `/public/stanox-crs` in
  `routes/stanox_crs.rs` stays.
- `crates/api/src/routes/mod.rs`:
  - `private_router`;
  - the `pub mod ingest` and `pub mod samples` lines;
  - the 100 MB `DefaultBodyLimit`;
  - the `require_internal_oauth` import.
- `crates/api/src/auth.rs`: `require_internal_oauth` and its tests.
  **Keep** `auth/internal_oauth.rs`, which holds the verifier the MCP
  budget uses.
- `crates/api/src/app.rs`:
  - `AppState::internal_oauth_routes`;
  - `build_internal_oauth_routes`;
  - `internal_oauth_route_table`;
  - `ensure_mcp_group_grants_no_private_route` and its call and tests.
    With no private group left it can never fail, so delete it. The
    plan's "adapted MCP group test" becomes a test that a verified
    `srv-ds-mcp` bearer still gets the MCP budget;
  - the `/private` text in `AppState`'s docs and `Debug` impl.
- `crates/api/src/data/config.rs`:
  - the 13 `internal_oauth_group_*` fields except `internal_oauth_group_mcp`;
  - `chart_env_wiring_tests`' `>= 13` sanity bound. It becomes "exactly
    `INTERNAL_OAUTH_GROUP_MCP`";
  - `private_routes` from 5.1.
- `crates/api/src/test_support.rs`: the deleted groups.
- `crates/api/src/edge.rs`:
  - `private_request_timeout_secs`;
  - `private_timeout_layer`;
  - `API_PRIVATE_REQUEST_TIMEOUT_SECS`.
- `crates/api/src/rate_limit.rs`: the `/private` exemption (line 160) and
  the tests `private_routes_are_never_limited` and the
  `classify(…/private/…)` asserts.
- `crates/api/src/route_metrics.rs`: `register`'s `private_routes`
  parameter and its private half.
- `crates/api/src/main.rs`:
  - the `/private` nest;
  - the `register_schedule_publish_metrics`,
    `incident_removal::register_metrics` and
    `trust_event_backlog::register_uid_inference_metrics` calls. Those
    metrics now come from the producers' pods; see 5.4 for their names;
  - the `/private` text in the CORS comment.
- The api's `data::*` shims that only `ingest.rs` used. Find them with
  the dead-code warnings of `cargo +1.88.0 check` and clippy after the
  delete. Likely candidates:
  - `queries::upsert_incident_snapshot` and `upsert_incidents`;
  - the publish and upsert re-exports in `queries.rs`, `corpus.rs`,
    `island_of_ireland.rs`, `full_coverage_window.rs`, `train_reasons.rs`
    and `trust_event_backlog.rs`;
  - `data::incident_removal`.

  They are deleted, not moved: the producers already call `ds_store`
  directly.

**Chart:**

- `templates/api-deployment.yaml`: the 13 `INTERNAL_OAUTH_GROUP_*` env
  except `_MCP`, `API_PRIVATE_REQUEST_TIMEOUT_SECS` and `API_PRIVATE_ROUTES`.
- `values.yaml`: `api.privateRoutes`, `api.timeouts.privateRequestTimeoutSecs`,
  and `api.internalOauth.groups.*` except `mcp`. Rewrite the
  `api.internalOauth` comment: it now exists only for the MCP.
- `templates/prometheusrule.yaml`:
  - the api 5xx split's `ingest` scope (the recording-rule list and the
    `DistantSignalApiIngest5xx` alert), the public scope's `/private/.*`
    exclusion and `metrics.prometheusRule.api5xx.ingest`;
  - `scripts/alert-rules-tests/api-5xx.yaml`'s `/private` series;
  - the `docs/alerts.md` entry for `distantsignalapiingest5xx` (CI checks
    every alert has its anchor).
- `templates/NOTES.txt`: the "/private/* is reachable" warning.
- `values-example.yaml`: line 254's comment.

**Tests:** the api's suite; the chart env-wiring tests; `helm template` with
the CI flags; `chart-values-doc.py check`; the alert-rules tests.

**Ranma:** delete `api.internalOauth.groups.{incidents,stations,tocs,ldbws,tfl,trustConsumer}`
from `clusters/mine-bringer/apps/distant-signal.yaml`. Keep `issuerUrl`,
`clientId` and the MCP group. Delete `api.privateRoutes` once the chart
no longer has it.

**Checks:**

- `/private/anything` is `/{unmatched}` 404;
- the MCP still gets its own budget: the startup log "MCP service caller
  recognised", and the MCP's 429 rate is unchanged;
- `DistantSignalApiPublic5xx` is silent.

**Rollback:** revert the commit and redeploy. This only helps while the
producers still have their HTTP paths, which is why 5.3 ships with or after
this step, never before.

### 5.3 Delete the producers' HTTP paths

Each producer collapses to its new path. `INGEST_SINK` and `*_SOURCE`
either go entirely, or stay with a single accepted value for one release,
so an old Ranma value fails loudly instead of being ignored (Q10).

| Crate | Delete |
|---|---|
| poller-incidents | `sink.rs` `HttpSink` and the `IngestSink` enum's `http` (`config.rs`), `api_ingest_url` (default `…/private/incidents`), the `internal_oauth` flatten, `main.rs`'s `run_poll_loop` branch |
| poller-stations | `sink.rs` `HttpSink`, `config.rs` `IngestSink::Http` and the URL, the OAuth flatten |
| poller-tfl, poller-tocs | `config.rs` `API_INGEST_URL` and OAuth; `main.rs`'s `run_poll_loop` (HTTP) branch; `SinkMode::{Http,HttpShadow}` use |
| poller-ldbws | `sink.rs`'s HTTP delivery and `last_fetched` HTTP arm; `pending.rs` (the HTTP retry buffer; check that `stream` does not use it); `main.rs` `fetch_sample_stations`' HTTP arm and `sample_stations_url`; `sample_source.rs`'s `ReadSource::Http`; `config.rs` `api_ingest_url`, `api_sample_stations_url`, OAuth |
| full-coverage-consumer | `sink.rs`'s `http` and `http+shadow` arms; `queries.rs` `post_*`, `fetch_stanox_crs`, the population GET; `population_reload.rs` `HttpSource`/`HttpSourceRef`; `reads.rs` `StanoxCrsSource::Http`; `config.rs` the five `…/private/…` URLs and OAuth |
| trust-consumer | `sink.rs` `HttpSink` and `ActiveSink`'s HTTP arm; `queries.rs` (the GETs); `reads.rs` HTTP arm; `config.rs` the four URLs, `IngestSink::Http`, OAuth; `API_CALL_OPERATIONS` loses `reload_tracked_trains`, `post_train_events` and `reload_stanox_crs` if they become unreachable |
| trust-backlog-consumer | `sink.rs` `HttpSink` (`/private/trust-event-backlog`, `/private/train-reasons`); `queries.rs` `get_json`; `config.rs` URLs, `IngestSink::Http`, OAuth; `API_CALL_OPERATIONS` loses `post_batch`, `post_train_reasons` and `reload_stanox_crs` |
| schedule-ingest | `sink.rs` `HttpSink` and `Sink::Http`; `corpus.rs`'s HTTP path and its wiremock tests; `config.rs` `API_INGEST_URL`, `CORPUS_API_URL`, OAuth |
| schedule-reference | `sink.rs` `HttpSink`/`HttpUrls`; `config.rs` the nine `…/private/…` URLs and `IngestSink::Http`; `main.rs`'s HTTP-path tests (`posts_to`, `fail_n_times`, the `{base}/private/…` fixture) |
| common | `oauth_client.rs` (whole module; no non-producer user, and `notifier/src/send.rs` only cites a constant in a comment); `ingest.rs`'s HTTP helpers (`get_json`, `post_*`, `ApiWait`, `CursorSource::Http`, `invalidate_on_auth_rejection`, `HttpStatusError` if unused); `poller_loop.rs`'s HTTP-cursor `run_poll_loop` |
| ingest-stream | `snapshot::SinkMode::{Http, HttpShadow}` and the shadow tests |
| prometheusrule | `DistantSignalConsumerApiCallsFailing`'s operation lists (lines ~236–243) shrink to `db_write`, `db_write_reasons` and `startup_reference_load` (if kept); the alert may be renamed. `DistantSignalIngestShadowMismatch` goes with `http+shadow` |

**Chart:**

- `templates/poller-deployments.yaml`, `trust-consumer-deployment.yaml`,
  `trust-backlog-consumer-deployment.yaml`,
  `full-coverage-consumer-deployment.yaml` and
  `schedulefeed-deployment.yaml`: every `printf "%s/private/…"` env, the
  `INTERNAL_OAUTH_*` env and the `ingest.sink`/`internalReads.source`
  conditionals, which become unconditional.
- `templates/secret.yaml`: the per-producer OAuth password keys.
- `values.yaml`:
  - the top-level `internalOauth` (tokenUrl, clientId, scope);
  - every `internalOauthUsername`/`internalOauthPassword` (lines 1008,
    1091, 1172, 1239, 1337, 3077, 3206, 3333, 4020, 4208);
  - `pollers.*.ingestPath`;
  - `pollers.ldbws.sampleStationsPath`;
  - the `ingest.sink` and `internalReads.source` values.
- `_helpers.tpl`: `pollerSinkDb`, `pollerSinkStream`, `trustSinkDb`,
  `internalReadsDb`, `scheduleFeedPostgres` and `trustConsumerPool` lose
  their HTTP cases.
- `distant-signal.apiBaseUrl` keeps only the frontend and the helm test as
  users.

**Local dev (missed by the plan):**

- `docker-compose.yml` wires 18 producer env vars to `http://api:8080/private/…`
  with Authentik credentials, and has no ingest-writer and no
  per-producer DB or Redis users.
- `local.env.example` and `dev.env.example` document the internal OAuth
  service accounts.

Either add the writer and give each producer `DATABASE_URL`/`REDIS_URL`
(superuser locally), or drop the producers from the default profile (Q10).

**Ranma:**

1. Delete every `internalOauthUsername`/`internalOauthPassword` from
   the sealed `distant-signal-secret` values overlay (reseal), and the
   top-level `internalOauth` block from `distant-signal.yaml`.
2. After 24 h with no `client_credentials` token issued to the ingest
   accounts (Authentik → Events, filter by user), decommission in
   `clusters/mine-bringer/apps/authentik.yaml`:
   - the users `srv-ds-incidents-poller`, `srv-ds-stations-poller`,
     `srv-ds-tocs-poller`, `srv-ds-ldbws-poller`, `srv-ds-tfl-poller`,
     `srv-ds-trust-poller`, `srv-ds-schedule-ingest`,
     `srv-ds-reference-ingest`, `srv-ds-full-coverage-consumer` and
     `srv-ds-trust-backlog-consumer`, with their app-password tokens;
   - the groups `ds-incidents-write`, `ds-stations-write`,
     `ds-tocs-write`, `ds-ldbws-write`, `ds-tfl-write`, `ds-trust-write`,
     `svc-schedule-ingest`, `svc-corpus-ingest`, `svc-schedule-reference`,
     `svc-full-coverage-consumer` and `svc-trust-backlog-consumer`;
   - the matching `AUTHENTIK_SVC_*_PASSWORD` worker env and the
     `authentik-svc-accounts-secret` keys
     `INTERNAL_OAUTH_PASSWORD_{INCIDENTS,STATIONS,TOCS,LDBWS,TFL,TRUST_CONSUMER,SCHEDULE_INGEST,REFERENCE,FULL_COVERAGE,TRUST_BACKLOG}`.

   Removing an entry from a blueprint does not delete the object, so mark
   each one `state: absent` for one apply, then remove the entries. **Keep**
   `distant-signal-internal` (provider and application), `srv-ds-mcp`,
   its group, `INTERNAL_OAUTH_PASSWORD_DS_MCP` and
   `authentik-distant-signal-internal-secret`.

**Checks:**

- every producer's `/livez` is green;
- the §1.3 B metrics keep rising;
- no pod has an `INTERNAL_OAUTH_*` env except the api's three;
- Authentik shows no failed `client_credentials` logins.

**Rollback:** revert the code and chart and redeploy. Restoring the
Authentik accounts needs their sealed passwords, so keep the old sealed
values in Ranma's git history.

### 5.4 Delete the api's Redis client

The full use list is in §3.

**Code:**

- `AppState.redis` (`app.rs`) and its construction (`app.rs`, around line
  631);
- `ServiceArguments::redis_url` and `redis_password` (`data/config.rs`);
- the `redis` dependency and `common`'s `redis` feature in
  `crates/api/Cargo.toml`;
- the `RedisError` branch in `unavailable.rs` (line 117) and its test;
- the `redis_url`, `redis_password` and `redis` fixture lines in about
  20 route test modules, `auth.rs` and `test_support.rs`;
- the incident tests in `data/queries.rs` and `incident_removal.rs` that
  build a `redis::Client`. They go with the 5.2 shims;
- `crates/common/tests/redis_acl.rs`'s `api` client sequence (lines ~336,
  ~504);
- the `api` line in `charts/distant-signal/files/redis-users.acl.tpl` (line
  62); `scripts/render-redis-acl.py` and `check-ingest-phase0-chart.py`
  if they list it;
- `redis.acl.clients.api`; `templates/api-deployment.yaml`'s `REDIS_URL`
  and `redisClientAuthEnv` for `api` (lines 471–473);
- `docs/redis-acl.md`'s `api` row.

Then the `api_*` metric names. The plan says "the `or` clauses", but there
is only one: `DistantSignalSchedulePublishStagedMismatch` matches
`(api|store)_…`. `ds-store` still emits **`api_`-prefixed** names from the
producers' pods:

- `api_corpus_last_delivered_at_seconds`;
- `api_incident_removal_inference_total`;
- `api_incidents_marked_removed_total`;
- `api_incidents_without_resolved_place`;
- `api_trust_event_backlog_*`;
- `api_train_reasons_rejected_rows_total`;
- `api_corpus_comparison_*`.

`DistantSignalCorpusStale` and `DistantSignalIncidentRemovalStalled` read
those names. Either rename them all to `store_*` with a one-release
`(api|store)` regex in each alert, or keep the names (Q1).

**Ranma:**

- with `redis.acl` on: drop `api-password` from the
  `distant-signal-redis-users` SealedSecret after the release. The ACL
  file no longer lists `api`;
- with it off: nothing. The api simply stops receiving
  `distant-signal-redis-auth`.

**Checks:**

- `CLIENT LIST` shows no `user=api`;
- the api pod has no `REDIS_*` env;
- `XINFO STREAM incident-text-changed` still shows recent entries (from
  poller-incidents);
- the enricher's lag stays 0.

**Rollback:** revert the commit.

### 5.5 Final grants

The diff against `db-grants.yaml` is in §4.1. Code and chart:

- `charts/distant-signal/files/db-grants.yaml`, then `uv run
  scripts/gen-db-grants.py render`; `postgres-grants.sql`;
- **a `gen-db-grants.py` change**: one role per table takes a single
  privilege string, and `columns` apply to every letter of it. "enricher:
  SELECT on `incidents`, UPDATE on five columns" cannot be written today.
  Allow a list of grants per role (for example `enricher: [S, {privileges:
  U, columns: […]}]`), plus a unit test in `scripts/tests/test_gen_db_grants.py`;
- `scripts/db-grants.sql.tpl` and `files/postgres-roles.sql`: stop
  creating `distant_signal_app` and its membership grants; revoke
  membership from every role; then `REASSIGN OWNED`/`DROP OWNED BY
  distant_signal_app` and `DROP ROLE` (idempotent, `IF EXISTS`);
- `postgresql.roles.app`, the default `databaseEnv` fallback to `app`, and
  `perService.*.connect` collapse to always-on (the chart refuses a
  service with no role);
- `docs/postgres-app-role.md` is rewritten for per-service roles.

**Ranma, two releases:**

1. **Narrow.** Release the chart with the narrowed `db-grants.yaml`. The
   setup Job (post-upgrade) applies it. Nothing restarts. Watch for 7
   days:
   - `kubectl logs` of every DB service for SQLSTATE `42501`;
   - `observe-role-usage.py` (read-only), which must show no verb outside
     the grants;
   - the per-role connection counts against their limits
     (`DistantSignalDbRoleNearConnectionLimit`).
2. **Drop `app`.** In the next release, delete `postgresql.roles.app` and
   the `distant-signal-postgres-app` SealedSecret
   (`distant-signal-postgres-roles-sealed.yaml`).

**Negative checks (Ranma):** `psql` as each role:

- `CREATE TABLE x (i int)`, `TRUNCATE stations`, `DELETE FROM users` and
  `ALTER TABLE incidents ADD COLUMN y int` must all fail;
- the api role: `UPDATE incidents SET summary = summary WHERE false` must
  fail;
- the enricher: `UPDATE incidents SET summary = …` must fail, while
  `UPDATE incidents SET extracted_at = extracted_at WHERE false` succeeds.

**Rollback:**

- re-release with the previous `db-grants.yaml`. The setup Job re-grants
  idempotently, and the services do not restart;
- for the `app` drop: restore the SealedSecret and `postgresql.roles.app`,
  and the setup Job recreates the role;
- every `perService.*.connect` must be back on `app` before anything
  connects as it again.

**Before narrowing the api:** these api binaries write tables the narrow
api role cannot write. Run them first, or give them a home (Q2):

- `backfill_incident_lines`: `UPDATE incidents`;
- `backfill_line_train_summaries`: `line_train_summaries`;
- `replay_uidless_movements`: backlog and movements;
- `backfill_trains`: `trains`, but only before the legacy contract
  migration, which every environment has passed.

### 5.6 NetworkPolicies

The diff against `templates/networkpolicy.yaml` is in §4.2. Chart only,
plus Ranma's comments.

**Ranma:**

- update the flow comment in
  `clusters/mine-bringer/apps/netpol-distant-signal.yaml` (lines ~24–40):
  api's in-namespace clients become the frontend and the helm test; Redis
  loses api and gains the stream producers; Postgres gains the producers;
- optionally narrow `allow-egress-same-namespace`, today every pod to
  every pod (Q6).

**Checks:**

- render tests for each component (CI's chart checks);
- after the release, a day with no new failure in any producer or reader;
- from a producer pod (Ranma), `curl -m 5
  http://distant-signal-api:8080/public/health` times out, while the
  frontend still renders.

**Rollback:** the previous chart release.

### 5.7 Docs

- the README architecture section; `DESIGN.md`'s `/private` section;
- `charts/distant-signal/README.md`, regenerated with
  `uv run scripts/chart-values-doc.py`;
- `docs/alerts.md`, `docs/trust-reason-codes.md`,
  `docs/movement-events-deadletter.md` and `docs/api-changelog.md`, all of
  which name `/private` routes;
- `frontend/lib/types.ts`'s two comments;
- `reference-data/line-catalogue-validation.md`'s suggested
  `/private/corpus-export`;
- mark the spec `implemented` in `docs/superpowers/README.md`.

Check with `chart-values-doc.py check`, plus `command grep -rn
'/private' --exclude-dir=superpowers` coming back empty apart from history.

## 3. The api's Redis client (5.4)

`crates/api` uses Redis in exactly one place.

| Use | Where | Replaced by | Decision needed |
|---|---|---|---|
| `XADD incident-text-changed` per changed incident, best effort, after the snapshot commits | `routes/ingest.rs::post_incidents` → `data::queries::upsert_incident_snapshot` → `common::incident_text_changed::publish(&app.redis, …)` | poller-incidents' DB sink, which already calls the same `publish` as Redis user `poller-incidents` (`%W~incident-text-changed +xadd`, plan 2c.2). The enricher's hourly sweep is the backstop for a missed entry, as today | no |
| `redis::RedisError` classified as "unavailable" (503 + `Retry-After`) | `unavailable.rs:117` | nothing: with no Redis client, the branch is dead | no |
| Rate limiting | `rate_limit.rs`: in-memory, per pod | unchanged; never used Redis | no. With `RollingUpdate` (D3) a surge pod has its own buckets for seconds, which the spec accepts. A shared limiter for more replicas would need Redis back: out of scope |
| Caches | the JWKS cache (`auth/internal_oauth.rs`), the trip-planner graph cache, `schedule_crs_line_index`, `line_matcher` | unchanged; all in-process | no |
| Sessions, pub/sub, locks | Postgres sessions; advisory locks in Postgres; no pub/sub | – | no |
| Readiness | `/public/ready` checks Postgres only (`readiness.rs`) | – | no |

The only open choice in 5.4 is the `api_*` metric names (Q1). Two things
change in the api's environment: it loses `REDIS_URL`, and
`distant-signal-redis-auth` is no longer mounted.

## 4. Final grants (5.5) and NetworkPolicies (5.6)

### 4.1 `charts/distant-signal/files/db-grants.yaml`

Against `dc2b4405`. The YAML already holds the design-target grants for
the five observed roles. They have no effect today because the roles
inherit everything from `distant_signal_app`. The diff is mostly
`status`:

```diff
 roles:
   api:
     name: distant_signal_api
-    status: observed
-    phase: 0b
+    status: narrow
+    phase: "5"
     groups: [read_shared, schema_gate]
   aggregator:
-    status: observed
+    status: narrow
   enricher:
-    status: observed
+    status: narrow
   notifier:
-    status: observed
+    status: narrow
   writer:
-    # Created (a member of app) from 1B.9 on, ...
-    status: observed
+    # Narrow from phase 5 (plan 5.5): its grants below, nothing from app.
+    status: narrow
```

**Correction to the plan:** 5.5 names api, aggregator, enricher and
notifier, but **the writer is `observed` too** (a member of `app` since
1B.9), so it must be narrowed in the same step. Its rows already carry its
target grants: `trains` SIU, `train_subscriptions` SU, `train_event_outbox`
SUD, the ingest snapshots SIU, `line_status` SIUD under the RLS policy,
`tocs` SIU, `ingest_freshness` SIU, the crosswalk SIUD and `ingest_dedup`
SID.

```diff
-  # enricher: UPDATE only on its extraction columns once narrowed (phase 5;
-  # the column list comes from the 0b report).
-  incidents: {class: ingest, grants: {api: S, incidents: SIU, enricher: SU, aggregator: S}}
+  # enricher: UPDATE only on its extraction columns (enricher/src/queries.rs).
+  incidents: {class: ingest, grants: {api: S, incidents: SIU, aggregator: S,
+    enricher: [S, {privileges: U, columns: [source_text_hash, extracted_category,
+      extracted_periods, extraction_model_version, extracted_at]}]}}
```

This needs the generator change in 5.5. The column list comes from
`crates/enricher/src/queries.rs`, lines ~158 and ~233. Confirm it against
the 0b report.

The api's rows stay as they are unless the 0b report shows otherwise:

- `trains` SIUD (spec: SIU, plus D for the journey cleanup);
- `train_subscriptions` SIUD;
- `train_movement_events` and `train_current_state` S (spec: plus I/D
  only if the report shows a public path writing them);
- the notifier-state tables S (account deletion relies on FK cascades,
  which need no grant);
- nothing on `*_publish_keys` or `ingest_dedup`.

The aggregator and notifier rows also stay unless the report shows
otherwise.

These comments become history:

- the `other_roles` comment "The app role is retired in phase 5" (it is
  retired);
- the `*_publish_keys` comment "The api publishes until …";
- the `line_train_summaries` and `schedule_services` "through the api's
  ingest endpoint until phase 2a" notes;
- the `trust_consumer` "Its reads … stay on the api until phase 4" note.

Also bring `connection_limit` for `api` from 34 down to what the api pool
really needs once ingest bodies are gone. This is optional; the render-time
budget check sums the limits either way.

### 4.2 `charts/distant-signal/templates/networkpolicy.yaml`

**postgres** ingress (lines 41–98): the producer and reader entries stop
depending on their switches. The template lines below are a sketch.

```diff
                   - api
                   - aggregator
                   - enricher
                   - notifier
                   {{- if .Values.ingestWriter.enabled }}
                   - ingest-writer
                   {{- end }}
-                  {{- if include "distant-signal.scheduleFeedPostgres" . }}
+                  {{- if .Values.scheduleFeed.enabled }}
                   - schedulefeed
                   {{- end }}
-                  {{- range … pollerSinkDb … }}
-                  - poller-<name>
-                  {{- end }}
+                  # stations, incidents (direct writers), ldbws (reads)
+                  {{- range $name := list "stations" "incidents" "ldbws" }}…{{ if enabled }}- poller-{{ $name }}{{ end }}
-                  {{- if … trustSinkDb trust_backlog }}
                   - trust-backlog-consumer
-                  {{- end }}
-                  {{- if … trustConsumerPool }}
                   - trust-consumer
-                  {{- end }}
-                  {{- range … internalReadsDb … }}
                   - full-coverage-consumer
-                  {{- end }}
```

Delete the comment at lines 35–40 ("The pollers/consumers that POST to
api's /private/* ingest endpoints do NOT belong here …").

**redis** ingress (lines 148–188):

```diff
-                  - api
                   - enricher
                   - trust-consumer
                   - trust-backlog-consumer
                   - full-coverage-consumer
                   - movement-relay
-                  {{- if include "distant-signal.ingestWriterStreamsOn" . }}
+                  {{- if .Values.ingestWriter.enabled }}
                   - ingest-writer
                   {{- end }}
-                  {{- if include "distant-signal.pollerIncidentsDbSink" . }}
-                  - poller-incidents
-                  {{- end }}
-                  {{- range … pollerSinkStream … }}
+                  # incidents (incident-text-changed), ldbws, tfl, tocs (ds:ingest:*)
+                  {{- range $name := list "incidents" "ldbws" "tfl" "tocs" }}…{{ if enabled }}
                   - poller-{{ $name }}
```

The IoI pollers are unchanged. Rewrite the comment block at lines 132–147.

**api** ingress (lines 244–331):

```diff
                   - frontend
-                  {{- range $name, $poller := .Values.pollers }}
-                  - poller-{{ $name }}
-                  {{- end }}
-                  - trust-consumer
-                  - trust-backlog-consumer
-                  - full-coverage-consumer
-                  - schedulefeed
-                  # Not the three island-of-Ireland pollers: …
                   {{- if .Values.tests.enabled }}
                   - test
```

Keep `apiExtraIngressNamespaces` (ds-mcp), the tunnel rule, the ingress
controller rule (drop its "SECURITY: this also exposes /private/*"
sentence) and the metrics rule. Rewrite the header comment at lines
221–243.

**api** egress (line 361): drop `redis`. Keep `postgres`, `idp` and both
issuer URLs: `api.internalOauth.issuerUrl` is the MCP verifier's JWKS.

```diff
-"deps" (dict "postgres" true "redis" true "idp" true) … "urls" (list .Values.api.sso.issuerUrl .Values.api.internalOauth.issuerUrl)
+"deps" (dict "postgres" true "idp" true) … "urls" (list .Values.api.sso.issuerUrl .Values.api.internalOauth.issuerUrl)
```

**Workers** (lines 427–509):

- `$callsApi` and `$queueWorker` go;
- `$token` (`internalOauth.tokenUrl`) is dropped from every worker's
  `urls`.

Final egress per worker:

| Component | Egress deps | `urls` |
|---|---|---|
| trust-consumer | redis, postgres | Kafka brokers |
| trust-backlog-consumer | redis, postgres | – |
| full-coverage-consumer | redis, postgres | Kafka brokers |
| poller-stations | postgres | its upstream |
| poller-incidents | postgres, redis | its upstream |
| poller-ldbws | redis, postgres | its upstream |
| poller-tfl, poller-tocs | redis | their upstream |
| schedulefeed (`$sfDeps`, line 612) | postgres | the bucket URLs only (drop `internalOauth.tokenUrl` from `$sfUrls`, line 595) |

On 2026-10-08, full-coverage-consumer keeps `api` egress even with
`ingest.sink: stream`, because `$fullCoverageEgress` only looks at
`internalReads`. That is harmless until 5.6, which drops it anyway.

**`_helpers.tpl`:** `egressSection`'s `deps.api` is still used by the
frontend. Its `devAuthentik` clause (`or $deps.api $deps.idp`) then only
matters for the api and frontend.

## 5. Risks and open questions

### Risks

| Risk | Mitigation |
|---|---|
| A rare route has a caller that 7 days cannot see (monthly CORPUS, per-delivery schedule products) | §1.3 B: each producer must have done its work on the new path at least once since the flip |
| Prometheus keeps about 8 days, so a 7-day `increase` is at its edge and the 14-day rule cannot come from it | the switch dates come from Ranma's git history; run the 7-day query on the last possible day |
| A 5.1 straggler is invisible as `/{unmatched}` | the counted 404 fallback in 5.1 |
| 5.3 deletes the last rollback path | ship 5.3 only after the 5.1 soak; keep the producers' old sealed OAuth values in Ranma's git history |
| Narrowing breaks a rare api path the 0b window did not see (a backfill binary, an account deletion edge) | the 0b report; the per-role suites in CI; §5.5's binary list; one-release rollback via the setup Job |
| Dropping `distant_signal_app` while a forgotten client still uses it | its own release, after `pg_stat_activity` shows no `usename = 'distant_signal_app'` for 7 days |
| Decommissioning an Authentik object the MCP needs | §1.2's keep list; the MCP's own 401 rate after the change |
| Local dev breaks after 5.3 | 5.3's local-dev item (Q10) |

### Open questions for the user

| # | Question | Suggested default |
|---|---|---|
| Q1 | `ds-store` still emits `api_*` metric names (corpus, incident removal, TRUST backlog, train reasons) from the producers' pods. Rename them to `store_*` in 5.4 (with a one-release `(api\|store)` regex in each alert and in any Grafana panel), or keep the names? | rename, with the regex |
| Q2 | The api's one-off binaries that write (`backfill_incident_lines`, `backfill_line_train_summaries`, `replay_uidless_movements`, `backfill_trains`) cannot run as the narrowed api role. Run them before 5.5 and then delete them, move them to the ingest-writer image (writer role), or keep a break-glass role? | delete those already run; move `replay_uidless_movements` to the writer image |
| Q3 | Extend `gen-db-grants.py` so a role can hold a table-level privilege and a column-level one on the same table (needed for the enricher)? | yes |
| Q4 | Under `API_PRIVATE_ROUTES=false`, answer 404 with a counted fallback (as above), or 410 Gone? | 404, counted |
| Q5 | Drop `distant_signal_app` in the narrowing release, or 7 days later? | 7 days later |
| Q6 | In phase 5, should Ranma also narrow `allow-egress-same-namespace` (every pod to every pod) to the chart's egress lists? | yes, as a follow-up release after 5.6 |
| Q7 | For `/corpus-locations` (monthly), wait for one CORPUS load on the `db` sink before starting, or accept the deployed config as enough? | wait for one load |
| Q8 | After 5.3, should `INGEST_SINK` and `*_SOURCE` disappear, or stay for one release with only the new value accepted (so a stale Ranma value fails at startup)? | stay one release, then go |
| Q9 | The four never-called GET routes (`/full-coverage-stats`, `/full-coverage-window-stats`, `/station-full-coverage-samples`, `/schedule-feed-ingests`) and the IoI routes have no caller now. Delete them early, as a cleanup outside phase 5, so the inventory shrinks? | yes |
| Q10 | Local dev after 5.3: add the ingest-writer and DB/Redis URLs to `docker-compose.yml`, or drop the producers from the default compose profile? | add the writer; producers use the superuser locally |
