# Schedule feed bucket source (GCS): implementation plan

Spec: [2026-10-02-schedule-feed-s3-landing-design](../specs/2026-10-02-schedule-feed-s3-landing-design.md).
Read the spec first. Section numbers (§) below refer to it. Both files keep
their `s3-landing` names from the first, AWS draft; the bucket is now
Google Cloud Storage (D8).

The goal is a GCS bucket in europe-west2 as a second schedule-feed source,
peer to SFTP. Each source can be switched on independently, and both can
run at once. A delivery that arrives through both is ingested once.

Phase 0 is done. The others are open.

## Decisions this plan builds on (user, 2026-10-02)

The spec's "Decisions" table has the detail.

- **D8 (replaces D1):** Google Cloud Storage, europe-west2. DTD's own
  service accounts are granted on the bucket; we create and hand over no
  secret.
- **D2:** a new, dedicated GCP project with a $5/month budget.
- **D3:** Helm; Crossplane v2 + provider-upjet-gcp recommended (spec §4).
  Keys created once by hand and sealed in Ranma-Config. OpenTofu only for
  the one-off setup.
- **D4:** object-level audit from Cloud Audit Logs Data Access only.
  Phase 5's tailer is how it reaches Loki, so it is recommended.
- **D5:** the bucket wins when the sources disagree within the window.
- **D6:** both sources are kept long-term.
- **D7 (repo scope):** only generic templates and docs here. Project ids
  and numbers, bucket names, service-account emails, amounts and
  addresses are Ranma's; examples use placeholders.
- **D9:** the bucket is dedicated to timetable/CORPUS ingest; usage limits
  cap the blast radius.
- **D10:** publisher access is DTD's four roles for each of DTD's service
  accounts, on the bucket only.
- **D11:** objects are in transit only. The reader fetches expected names,
  verifies, archives locally, and deletes `ifGenerationMatch`; unexpected
  objects are flagged and deleted unread.
- **Kill switch** (Ranma, user-approved): bindings removed automatically
  on usage or budget trips; recovery is a deliberate reapply.

Still open (spec §14): the controller choice, how the kill switch stops
Crossplane re-creating bindings, the steady state, and DTD's answers on
object names, the scanner, read-back timing, dual delivery and a test
delivery.

## Ground rules for whoever executes this

- Follow `/home/coder/ds-review/fix-brief-common.md`: rustc 1.88 with no
  dependency above it, the disk-budget cargo environment, the assigned
  migration range only, Python over bash for tooling (uv, ruff, mypy
  --strict).
- Every new setting ships **off**, apart from `scheduleFeed.sftp.enabled`,
  which defaults to `true` so that existing values render unchanged. List
  them in the report.
- One task, one commit, tests in the same commit.
- Never put a credential anywhere in this repo, including test fixtures.
  Tests use a throwaway RSA key generated at test time, or a fake token
  source; never a real key. No real project, bucket or service-account
  identifier either (D7).

## Phase 0: the GCP-side chart (done in this change)

- `charts/ds-ingest-bucket/` renders the delivery bucket, the audit-log
  bucket, bucket-level IAM members (publisher members × roles, the
  reader's objectViewer and delete-only custom role, the audit sink),
  optional usage alert policies and optional Pub/Sub notifications, as
  Crossplane v2 managed resources. It is off by default.
- `scripts/check-ingest-bucket-chart.py` checks the bucket settings and
  every grant.
- CI: two `helm lint --strict` runs and a `helm template` in `helm-lint`;
  the policy check in `scripts-lint`, after Helm is installed.

## Phase 1: sources as peers, no behaviour change

The target shape is the spec's §9. The watch directory becomes the first
`Source`, and the pipeline stops knowing where files come from.

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 1.1 | Add `source.rs`: `SourceId` (`Sftp`, `Bucket`, `as_str()` giving `"sftp"` and `"bucket"`), `Candidate`, `SourceIdentity`, `LocalDelivery { path, sha256, bytes, delivered_at }`, and the `enum Source` dispatch with `poll`, `fetch` and `settle` | `crates/schedule-ingest/src/source.rs`, `main.rs` (mod) | Unit: `as_str` strings are the contract values |
| 1.2 | `WatchDirSource`: move `scan_incoming`, `StabilityTracker`, stray detection and `recognise_completed` behind `poll`. `fetch` hashes in place. `settle` keeps CORPUS's `move_if_unchanged`/archive. Behaviour identical, including log messages that tests match | `source.rs`, `scan.rs`, `main.rs`, `corpus.rs` | **Every existing `main.rs`, `corpus.rs`, `scan.rs` and `delivery.rs` test passes unmodified** |
| 1.3 | Pipeline: `run_scan_cycle` and `run_corpus_cycle` take `&[Candidate]` from all sources. Quarantine, pending POST and last-ingested state are keyed by `(source, identity)` | `main.rs`, `corpus.rs` | Existing tests; two candidates from one source keep newest-wins and superseded |
| 1.4 | Audit lines: add `source`; add `Outcome::Duplicate` and `Outcome::Superseded` | `audit.rs` | Extend `outcome_strings_are_the_contract_values` and the JSON-line contract test |
| 1.5 | `.delivery-ingested` v2 (`source`, `source_ref`; 4-field v1 reads as `sftp`) and the append-only `.delivery-sources` | `delivery.rs` | Round-trip v1 and v2; a v1 file is recognised after the upgrade |
| 1.6 | Content ledger: `find_by_sha256(storage_dir, sha)` over every `.delivery-ingested` and a new `quarantine.log`; CORPUS the same over `storage_dir/corpus/` plus a sidecar `.sha256` | `delivery.rs`, `corpus.rs` | Hit, miss; a corrupt line is ignored |
| 1.7 | Config: `--sftp-source-enabled` (`SFTP_SOURCE_ENABLED`, default `true`), `--source-precedence` (`bucket,sftp`), `--disagreement-window-minutes` (120). Refuse to start with no source | `config.rs` | clap parse tests; `chart_env_wiring_tests` gains `SFTP_SOURCE_*` and `SOURCE_*` |

Check: fmt, clippy, the 1.88 check and the workspace tests from the
brief. Deploying phase 1 alone must be a no-op in production.

## Phase 2: the GCS source

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 2.1 | `object_store = { version = "0.14.2", default-features = false, features = ["gcp"] }` in schedule-ingest. Its dependencies are the ones the aggregator's `aws` feature already brings (`cloud-base`, `reqwest`/rustls, `aws-lc-rs`); confirm nothing new enters `Cargo.lock` with `git diff Cargo.lock`. CRC32C from the locked `crc` 3.3.0 (`CRC_32_ISCSI`), MD5 from the locked `md-5` | `crates/schedule-ingest/Cargo.toml` | `cargo +1.88.0 check -p schedule-ingest`; `cargo deny check` if the repo runs it |
| 2.2 | Bucket config: `--bucket-source-enabled` (false), `BUCKET_NAME`, `BUCKET_BASE_URL` (empty; tests only), `BUCKET_EXPECTED_KEYS` (the SFTP routing globs), `BUCKET_POLL_INTERVAL_SECS` (300), `BUCKET_DELETE_MIN_AGE_SECS` (3600), `BUCKET_ARCHIVE_KEEP` (5), `BUCKET_MAX_OBJECT_BYTES` (512 MiB), `BUCKET_MAX_DOWNLOADS_PER_POLL` (2), `BUCKET_MAX_DOWNLOAD_BYTES_PER_HOUR` (256 MiB), `BUCKET_MAX_DOWNLOAD_BYTES_PER_DAY` (1 GiB), `BUCKET_MAX_BACKOFF_SECS` (3600). The credential comes only from `GOOGLE_SERVICE_ACCOUNT_PATH` (the mounted key), never a flag. There is no prefix setting: the reader lists the root | `config.rs` | Parse tests; an empty `expectedKeys` is refused |
| 2.3 | `GcsSource::poll`: `list_with_delimiter(None)` (the root only; nested names are never seen). Route each name: expected and under `maxObjectBytes` → candidate unless its (name, generation) is in `sources/bucket/.seen`; otherwise **unexpected**: one log line (name escaped and truncated to 256 bytes, size, generation; no content), `schedule_feed_source_unexpected_objects_total`, and queued for deletion. Its own `next_poll_at`, separate from the 120 s watch-dir tick | `source_gcs.rs` (new) | `object_store::memory::InMemory` behind a small `trait ListGet` seam: a new object appears once; a confirmed one never again; nested names ignored; an unexpected name is logged once, never fetched, queued for delete; an oversized expected name is never fetched |
| 2.4 | `fetch`: object metadata (`size`, `generation`, `crc32c`, `md5Hash`, `timeCreated`) by JSON API GET with the bearer from `GoogleCloudStorage::credentials()`; `get_opts` with `version` = that generation; stream into `.partial-<name>` through a writer hashing SHA-256 and CRC32C; check size, CRC32C and (if present) MD5; fsync; rename into `sources/bucket/archive/`; prune to `archiveKeep`. Count bytes in `schedule_feed_source_downloaded_bytes_total`. Enforce the per-poll, per-hour and per-day caps **before** each GET; on a cap, set `schedule_feed_source_download_capped` and stop until the window passes | `source_gcs.rs` | Hash and size correct; a CRC mismatch is an error and leaves the object; the caps stop a loop (a fake that always reports a new generation downloads at most the cap); a confirmed generation is never fetched again after a restart. Wiremock fake of the JSON and XML endpoints (list, metadata, GET by generation, DELETE with `ifGenerationMatch`; 403, 404, 412) so the real client's error mapping is tested |
| 2.5 | `settle` and deletion: append to `.seen` (last 200, atomic). Delete confirmed and unexpected objects once `timeCreated + deleteMinAgeSecs` has passed, with JSON API `DELETE ...?ifGenerationMatch=<g>`. 412 → a newer upload exists: log, leave it. 404 → gone: fine. A failed delete never triggers a re-download | `source_gcs.rs` | 412 keeps the newer object and the next poll ingests it; 404 is not an error; the min-age wait; deleting an unexpected object never GETs it |
| 2.6 | Revocation and backoff: 401/403 from any call sets `schedule_feed_source_access_revoked{source}` to 1, logs once per state change, and backs off to `maxBackoffSecs`; the first success clears it. Other errors back off exponentially from the poll interval. The bucket source's errors never stop the SFTP source or crash the process | `source.rs`, `source_gcs.rs`, `main.rs` | A 403 storm: one log line, the gauge set, no tight loop (count requests against a fake clock), SFTP candidates still processed in the same cycles; recovery clears the gauge |
| 2.7 | Metrics: `schedule_feed_source_last_new_object_seconds`, `..._errors_total{kind}` (`auth`, `list`, `get`, `verify`, `delete`, `size`), `..._downloaded_bytes_total`, `..._unexpected_objects_total`, `..._download_capped`, `..._access_revoked`, `..._poll_duration_seconds`, all `{source}`; registered at 0 | `source.rs`, `source_gcs.rs` | Metric names in a contract test, like the existing `*_METRIC` consts |
| 2.8 | Integration (`#[ignore]`): a fake-GCS server (e.g. fake-gcs-server as a CI service container) via `BUCKET_BASE_URL`. Put a real-shaped fixture zip (`delivery::build_test_zip`), run cycles, assert one extraction, one POST (wiremock api), one `accepted` line with `source="bucket"`, the object deleted, the local archive present. Re-upload: a new generation, a new delivery | `crates/schedule-ingest/tests/bucket_source.rs` | — |

## Phase 3: two sources at once

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 3.1 | Dedup: after `fetch`, look the SHA-256 up in the ledger (1.6). On a hit, write a `duplicate` audit line, append to `.delivery-sources`, `settle`, skip extraction and POST | `main.rs`, `corpus.rs` | The same zip through SFTP and the bucket, in one cycle and in either order: one extraction, one POST, one `accepted` and one `duplicate` line |
| 3.2 | Disagreement: different SHA-256s for the same kind within the window → increment `schedule_feed_source_disagreement_total{kind}`; ingest by precedence (`[bucket, sftp]`, D5); the other is `superseded`; a quarantined winner doesn't promote the loser | `main.rs`, `corpus.rs` | Table test: arrival order × precedence × winner passes/fails; outside the window → newer delivery |
| 3.3 | api: a migration in the **assigned** range adding nullable `delivery_source text` with `CHECK (... IN ('sftp','bucket'))` to `schedule_feed_ingests` and `corpus_deliveries`; optional request field; schedule-ingest sends it | `crates/api/migrations/<assigned>_schedule_feed_delivery_source.sql`, `routes/ingest.rs`, `data/queries.rs`, `data/corpus.rs` | `migration_index_locking`, `migration_checksums`, `check-migration-order.py`; DB-gated ingest tests with and without the field; `docs/api-changelog.md` if listed |
| 3.4 | Same-second, different-content collision: `-<source>` suffix accepted by `is_delivery_dir_name` and schedule-reference discovery, ordered after the bare name | `delivery.rs`, `crates/schedule-reference/src/discovery.rs` | Ordering test in both crates |

## Phase 4: the `charts/distant-signal` `scheduleFeed` changes

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 4.1 | Gate everything SFTP on `scheduleFeed.sftp.enabled` (default `true`): container, Service, ConfigMap and checksum, host-key Secret and volume, `sftp-bootstrap`, the `authMethod` guard, the 2022 ingress rule, the `sftp-metrics` endpoint and the `schedule-sftp` alert group. Fail with neither source enabled | `schedulefeed-*.yaml`, `networkpolicy.yaml`, `podmonitor.yaml`, `prometheusrule.yaml`, `values.yaml` | CI render step: **the default render is byte-identical** to `origin/main`'s; SFTP-off/bucket-on has no sftpgo, Service, NodePort or host key; neither → fails |
| 4.2 | `scheduleFeed.bucket.*` per spec §10. `ingest` env: `SFTP_SOURCE_ENABLED`, `BUCKET_SOURCE_ENABLED`, `BUCKET_*` only when enabled; the key from `existingSecret` mounted read-only, `GOOGLE_SERVICE_ACCOUNT_PATH` (fail if empty); `SOURCE_PRECEDENCE`, `DISAGREEMENT_WINDOW_MINUTES`. Add `storage.googleapis.com` and `oauth2.googleapis.com` to the schedulefeed `egressSection` `urls` | `schedulefeed-deployment.yaml`, `networkpolicy.yaml`, `values.yaml` | Render: bucket off → no `BUCKET_`/`GOOGLE_` env and no key mount; on → mount and env present. `chart_env_wiring_tests` extended |
| 4.3 | Alert group `distant-signal.schedule-bucket` (renders only with `bucket.enabled`): `...AccessRevoked` (critical, "bucket access revoked"), `...NoNewObject` (30 h), `...ReadErrors`, `...UnexpectedObject`, `...DownloadBudget` (critical), `...SourcesDisagree` (both sources on). Runbook sections in `docs/alerts.md`, including **what a kill-switch trip looks like from DS and that recovery is a deliberate Ranma reapply** (spec §8) | `prometheusrule.yaml`, `values.yaml`, `docs/alerts.md` | `scripts/check-alert-payloads.py` with the group in its `RENDER_FLAGS`; promtool unit tests for NoNewObject, AccessRevoked and DownloadBudget |
| 4.4 | Docs: README values rows (`chart-values-doc.py check`); new `docs/schedule-feed-bucket.md` (switches, adoption, the object's life, key rotation, restoring a soft-deleted object, the kill-switch runbook, audit LogQL); `docs/schedule-feed-sftp.md` gets "Alongside the bucket source" | as listed | `uv run scripts/chart-values-doc.py check`; `helm lint --strict` on both example files |
| 4.5 | Bump `charts/distant-signal` `version`. Add `charts/ds-ingest-bucket` to `push-helm-chart` | `Chart.yaml`, `.github/workflows/containers.yml` | actionlint |

## Phase 5 (recommended, per D4): the audit-log tailer

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 5.1 | `BUCKET_AUDIT_LOGS_SHIP` (off), `..._BUCKET`, `..._POLL_INTERVAL_SECS` (600). List the audit-log bucket after a cursor in `sources/bucket/.audit-cursor`, GET new sink files, parse the Cloud Logging JSON entries, write one line each (`target=schedule_ingest::bucket_access`: principal, caller IP, method, object, status, time). Never log request bodies or tokens | `audit_log.rs` (new), `config.rs`, chart env | Parser against documented sample entries; the cursor survives a restart; a malformed entry is counted, not fatal |
| 5.2 | Ranma side: Alloy stage `audit="bucket-delivery"` (400 days) and the Loki group `distant-signal-bucket-audit` (spec §8) | Ranma-Config `logging.yaml` | Ranma's rule checks |

## Phase 6 (deferred; only if latency matters): Pub/Sub hybrid

| # | Task | Notes |
| --- | --- | --- |
| 6.1 | Pub/Sub REST client: `subscriptions.pull`, `acknowledge`, `modifyAckDeadline` over `reqwest` with the bearer from `GoogleCloudStorage::credentials()` | wiremock fake of the endpoints; no new crates |
| 6.2 | Wake-up loop: a message triggers the bucket source's LIST reconcile early; ack once the cycle has handled the named generation; extend the deadline during extraction; periodic LIST every `reconcileIntervalSecs` (3600) | Table tests: duplicates, out-of-order, a lost message caught by the reconcile |
| 6.3 | Chart: `scheduleFeed.bucket.notifications.pubsub.*`; `ds-ingest-bucket` `notifications.pubsub.enabled: true` (needs `gcp.projectNumber`) | Render tests |

## Ranma-Config tasks (the cluster owner; read-only from this repo)

Per D7, every concrete value below lives in Ranma-Config. The order
matches spec §12.

1. **Project bootstrap** (OpenTofu, once):
   - create the dedicated GCP project, link billing, and set the $5/month
     budget and its recipient;
   - check the effective org policies if the project is in an
     organization (spec §7: domain-restricted sharing must admit DTD's
     service accounts; key creation must be allowed for the reader);
   - the Crossplane provider's service account, its custom project role
     (spec §7) and its key, sealed for `crossplane-system`;
   - **the reader's service account**; its key created once by hand,
     piped into `kubeseal` as `distant-signal/distant-signal-schedulefeed-bucket`
     (key `service-account.json`), with a 90-day rotation reminder;
   - **the Data Access audit config** (`DATA_READ`, `DATA_WRITE` for
     `storage.googleapis.com`) and **the Logging sink** to the audit-log
     bucket; its writer identity goes into the chart values;
   - **the kill switch**: alert policies on received bytes, write requests
     and sent bytes, the budget's Pub/Sub topic, and the function holding
     only `getIamPolicy`/`setIamPolicy` on the bucket (removes publisher
     members on ingress/write, the reader on egress, all on budget).
2. **Crossplane**: Crossplane v2 and the GCP providers
   (`provider-gcp-storage`, `-cloudplatform`; `-monitoring`, `-pubsub` only
   if used), activation limited to the kinds used, the
   `ClusterProviderConfig`, PSA labels, egress 443, and **the kill-switch
   watcher** that pauses the bindings labelled
   `ds-ingest-bucket/kill-switch-group` (or the reader-disable lever;
   spec §5).
3. **The bucket**: a `ds-ingest-bucket` HelmRelease with `enabled: true`,
   `gcp.projectId`, `bucket.name`, **`publisher.members` set to DTD's
   service accounts** (from DTD's instructions; roles stay at the
   default four), `reader.member`, and `auditLogs.sinkWriterIdentity`.
   Optionally `usageAlerts` with the notification channels.
4. **DS release values:** `scheduleFeed.bucket.enabled: true`,
   `bucket.bucket`, `bucket.existingSecret: distant-signal-schedulefeed-bucket`,
   `bucket.auditLogs.*` if shipping; SFTP left on.
5. **Logging and alerts:** phase 5.2, and route
   `distant-signal.schedule-bucket` through the existing ntfy/Grafana
   path.
6. **Optional (bucket-only):** `scheduleFeed.sftp.enabled: false`; remove
   30450 from `public-exposure-check.yml` and the NodePort exception;
   update `docs/specs/public-ip-exposure.md`.

## Test strategy summary

| Layer | How |
| --- | --- |
| GCP resources | `scripts/check-ingest-bucket-chart.py` (CI) |
| Chart wiring | CI render steps: byte-identical default; SFTP-off, bucket-on and neither-on; `chart_env_wiring_tests`; `check-alert-payloads.py` and promtool tests |
| Source logic | `object_store::memory::InMemory` behind a seam |
| GCS protocol | A wiremock fake: list, metadata, GET by generation, DELETE with `ifGenerationMatch`; 403, 404, 412 |
| Loop and revocation | Fake clock and fake store: caps, backoff, one log line per state change |
| End to end | `#[ignore]` fake-GCS integration in a CI service container |
| Multi-source | Table tests for arrival order × precedence × check outcome |
| Regression | Every existing schedule-ingest test unchanged after phase 1 |
| In production | Spec §12 step 6: a week of `accepted` + `duplicate` pairs and zero disagreement |

## Off-by-default settings this work adds

- `charts/ds-ingest-bucket`: `enabled`, `usageAlerts.enabled`,
  `notifications.pubsub.enabled`.
- `charts/distant-signal`: `scheduleFeed.bucket.enabled`,
  `bucket.notifications.pubsub.enabled` and `bucket.auditLogs.ship`.

`scheduleFeed.sftp.enabled` is new but defaults to **on**.
