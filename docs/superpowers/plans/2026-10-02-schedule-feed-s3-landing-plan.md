# Schedule feed S3 source: implementation plan

Spec: [2026-10-02-schedule-feed-s3-landing-design](../specs/2026-10-02-schedule-feed-s3-landing-design.md).
Read the spec first. Section numbers (§) below refer to it.

The goal is an S3 bucket in AWS eu-west-2 as a second schedule-feed
source, peer to SFTP. Each source can be switched on independently, and
both can run at once. A delivery that arrives through both is ingested
once.

Phase 0 is done. The others are open.

## Decisions this plan builds on (user, 2026-10-02)

The spec's "Decisions" table has the detail.

- **D1:** AWS S3, eu-west-2.
- **D2:** a **new, dedicated AWS account**. The user holds root MFA. A
  $5/month budget alert.
- **D3:** **ACK via Helm** (`charts/ds-ingest-bucket`). Keys are created
  once by hand and sealed in Ranma-Config. **OpenTofu only for the
  one-off setup** (account baseline, controller users, boundary, budget).
- **D4:** **S3 server access logs only**, no CloudTrail data events. Phase
  5's tailer is therefore the only way object-level audit reaches Loki,
  so it is recommended, not merely optional.
- **D5:** **the bucket wins** when the sources disagree within the
  window; the disagreement alert still fires.
- **D6:** both sources are kept long-term; nothing is retired.
- **Default unless the user objects:** lifecycle-only cleanup.
  `reader.allowDelete` and `deleteAfterIngest` stay off, and task 2.5's
  delete path can wait.

Still open (spec §14):

- the budget alert email;
- the node egress IP pin;
- the steady state after verification;
- all of RDM's S3 destination details. These go to DTD/RDM; the
  ready-to-send list is in spec §14.

## Ground rules for whoever executes this

- Follow `/home/coder/ds-review/fix-brief-common.md`. It covers:
  - rustc 1.88, with no dependency whose MSRV is higher. `aws-sdk-*` is
    out (§6);
  - the disk-budget cargo environment and the shared `CARGO_TARGET_DIR`;
  - the assigned migration timestamp range only;
  - Python over bash for tooling (uv, ruff, mypy --strict).
- Every new setting ships **off**, apart from `scheduleFeed.sftp.enabled`,
  which defaults to `true` so that existing values render unchanged. List
  them in the report.
- Keep commits small: one task, one commit, with tests in the same
  commit.
- Never put an AWS key anywhere in this repo, including test fixtures.
  Tests use the example key pair published in AWS's SigV4 test-suite
  documentation (its access key id is `AKIDEXAMPLE`), never a real one.

## Phase 0: the AWS-side chart (done in this change)

- `charts/ds-ingest-bucket/` covers the delivery bucket, the log bucket,
  the IAM reader and writer, and the optional SQS queue and DLQ, all as
  ACK custom resources. It is off by default.
- `scripts/check-ingest-bucket-chart.py` checks the policies.
- CI: two `helm lint --strict` runs and a `helm template` in `helm-lint`.
  The policy check runs in `scripts-lint`, after Helm is installed.

## Phase 1: sources as peers, no behaviour change

The target shape is the spec's §9. The watch directory becomes the first
`Source`, and the pipeline stops knowing where files come from.

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 1.1 | Add `source.rs`: `SourceId` (`Sftp`, `Bucket`, `as_str()` giving `"sftp"` and `"bucket"`), `Candidate`, `SourceIdentity`, `LocalDelivery { path, sha256, bytes, delivered_at }`, and the `enum Source` dispatch with `poll`, `fetch` and `settle` | `crates/schedule-ingest/src/source.rs`, `main.rs` (mod) | Unit: `as_str` strings are the contract values |
| 1.2 | `WatchDirSource`: move `scan_incoming`, `StabilityTracker`, stray detection and the restart recognition (`recognise_completed`) behind `poll`. `fetch` hashes in place (`audit::DeliveredFile::hash_file`). `settle` keeps CORPUS's `move_if_unchanged`/archive. Behaviour is identical, including log messages that existing tests match | `source.rs`, `scan.rs`, `main.rs`, `corpus.rs` | **Every existing `main.rs`, `corpus.rs`, `scan.rs` and `delivery.rs` test passes unmodified.** Write them against the new seams only if a signature must change, and say so in the commit |
| 1.3 | Pipeline: `run_scan_cycle` and `run_corpus_cycle` take `&[Candidate]` from all sources instead of scanning. Quarantine, pending POST and last-ingested state are keyed by `(source, identity)` rather than bare mtime | `main.rs`, `corpus.rs` | Existing tests; a new test where two candidates from one source keep today's newest-wins and superseded behaviour |
| 1.4 | Audit lines: add `source` to every `schedule_ingest::audit` line. Add `Outcome::Duplicate` (`"duplicate"`) and `Outcome::Superseded` (`"superseded"`) | `audit.rs` | Extend `outcome_strings_are_the_contract_values`; extend the JSON-line contract test with `source` |
| 1.5 | `.delivery-ingested` v2: append `source` and `source_ref` (tab-separated). The reader accepts 4 fields (as `sftp`, `sftp:<name>`) or 6. Add `.delivery-sources`, an append-only file of arrivals | `delivery.rs` | Round-trip v1 and v2; a v1 file on disk is recognised after the upgrade (the restart test with a pre-written v1 record) |
| 1.6 | Content ledger: `fn find_by_sha256(storage_dir, sha) -> Option<(dir, outcome)>`, reading every delivery directory's `.delivery-ingested` and a new `quarantine.log` (sha, reason, source) under `storage_dir`. CORPUS: the same lookup over `storage_dir/corpus/` names, plus a sidecar `.sha256` written at archive time | `delivery.rs`, `corpus.rs` | Ledger hit or miss; a corrupt line is ignored, not fatal |
| 1.7 | Config: `--sftp-source-enabled` (env `SFTP_SOURCE_ENABLED`, default `true`), `--source-precedence` (`bucket,sftp`), `--disagreement-window-minutes` (120). Refuse to start with no source enabled | `config.rs` | clap parse tests; `chart_env_wiring_tests` gains the `SFTP_SOURCE_*` and `SOURCE_*` prefixes and the chart sets them (task 4.2) |

Check: the fmt, clippy, 1.88 check and workspace test commands from the
brief. Deploying phase 1 alone must be a no-op in production.

## Phase 2: the S3 source

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 2.1 | Add `object_store = { version = "0.14.2", default-features = false, features = ["aws"] }` to schedule-ingest, with the same spec as the aggregator, so nothing new enters `Cargo.lock`. Confirm that with `git diff Cargo.lock` | `crates/schedule-ingest/Cargo.toml` | `cargo +1.88.0 check -p schedule-ingest`; `cargo deny check` if the repo runs it |
| 2.2 | Bucket config: `--bucket-source-enabled` (default false), `BUCKET_NAME`, `BUCKET_PREFIX` (`rdm/`), `BUCKET_REGION` (`eu-west-2`), `BUCKET_ENDPOINT` (empty), `BUCKET_ALLOW_HTTP` (false; tests only), `BUCKET_POLL_INTERVAL_SECS` (300), `BUCKET_MAX_OBJECT_BYTES` (512 MiB), `BUCKET_MAX_DOWNLOAD_BYTES_PER_DAY` (1 GiB), `BUCKET_DELETE_AFTER_INGEST` (false). Credentials come only from `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`, which `AmazonS3Builder::from_env` reads; there are no CLI flags for them | `config.rs` | Parse tests; a prefix without a trailing `/` is refused |
| 2.3 | `ObjectStoreSource::poll`: `list(Some(prefix))`. Take only direct children of the prefix (no `/` after it), route by file name through `Routing` (the CIF and CORPUS globs, strays logged once), and drop identities already in `sources/bucket/.seen`. Poll on its own interval, separate from the 120 s watch-dir tick: a `next_poll_at` per source | `source_bucket.rs` (new) | `object_store::memory::InMemory` behind a small `trait ListGet` seam: a new object appears once; a seen one doesn't; strays are logged once; nested keys are ignored |
| 2.4 | `fetch`: `get_opts` with `if_match` = the listed ETag. Stream into `storage_dir/sources/bucket/.partial-<name>` through `audit::HashingWriter`. Check the byte count equals the listed size and is at most `max_object_bytes`; refuse **before** downloading if the listed size is over the cap. Then fsync, rename to `sources/bucket/<name>`, and `File::set_modified(LastModified)`. Take the version id from the result's `meta.version`. Count bytes in `schedule_feed_source_downloaded_bytes_total{source}`. Refuse (with an error metric) once today's downloads pass `max_download_bytes_per_day` | `source_bucket.rs` | InMemory: hash and size are correct; a size mismatch is an error; the over-cap refusal never calls get; the daily budget stops a loop. Wiremock S3 fake (ListObjectsV2 XML, GET with `ETag`, `x-amz-version-id`, `Last-Modified`; 412 on an `If-Match` miss; 403) so the real `AmazonS3` client parses versions and errors: a 403 maps to `kind="auth"` |
| 2.5 | `settle`: append to `.seen`, keeping the last 50 lines and writing atomically. With `delete_after_ingest`, `delete` with the **version id** (object_store has no versioned delete, so use a signed request through `object_store`'s `aws` client if it is exposed; otherwise defer this to phase 6's SigV4 module and leave the setting refusing to start until then) | `source_bucket.rs` | `.seen` bounds and atomicity; a restart doesn't re-download (`downloaded_bytes` unchanged) |
| 2.6 | Metrics: `schedule_feed_source_last_new_object_seconds{source}`, `schedule_feed_source_errors_total{source,kind}` (`auth`, `list`, `get`, `size`, `budget`), `schedule_feed_source_downloaded_bytes_total{source}`, `schedule_feed_source_poll_duration_seconds{source}`. Register them at 0 so the alerts' `increase()` sees the first event | `source.rs`, `source_bucket.rs` | Metric names in a contract test, the same pattern as the existing `*_METRIC` consts |
| 2.7 | Integration (`#[ignore]`): MinIO with versioning on, or LocalStack. Put a real-shaped fixture zip (built by `delivery::build_test_zip` from `tests/fixtures/cif_delivery_excerpt`), run two cycles, and assert one extraction, one POST (wiremock api) and one `accepted` line with `source="bucket"`. Overwrite the object: a new version, a new delivery. CI service container in a new `schedule-ingest-s3` job, or under the existing DB-gated job | `crates/schedule-ingest/tests/bucket_source.rs` | — |

## Phase 3: two sources at once

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 3.1 | Dedup: after `fetch`, look the SHA-256 up in the ledger (1.6). On a hit, write a `duplicate` audit line (`duplicate_of`, `first_source`), append to `.delivery-sources`, `settle`, and skip extraction and the POST | `main.rs`, `corpus.rs` | The same zip through SFTP (watch dir) and the bucket (InMemory) in one cycle, and in successive cycles in either order: one extraction, one POST, one `accepted` and one `duplicate` line, with the same sha256 and both sources |
| 3.2 | Disagreement: different SHA-256s for the same kind within `disagreement_window` → increment `schedule_feed_source_disagreement_total{kind}`; ingest the higher-precedence one (default `[bucket, sftp]`: the bucket wins, D5); mark the other `superseded`. Write the rules from §9 exactly, including "a quarantined winner does not promote the loser" | `main.rs`, `corpus.rs` | A table-driven test: arrival orders (sftp→bucket, bucket→sftp) × precedence orders × winner passes or fails its checks; outside the window → treated as a newer delivery |
| 3.3 | api: a migration in the **assigned** range. `ALTER TABLE schedule_feed_ingests ADD COLUMN delivery_source text` and the same on `corpus_deliveries`, nullable, with a CHECK of `IN ('sftp','bucket')`. Request structs get an optional `delivery_source`. schedule-ingest sends it | `crates/api/migrations/<assigned>_schedule_feed_delivery_source.sql`, `crates/api/src/routes/ingest.rs`, `data/queries.rs`, `data/corpus.rs`; ingest's request structs | `migration_index_locking`, `migration_checksums`, `check-migration-order.py`; the api's DB-gated ingest tests with and without the field; `docs/api-changelog.md` row if this route is listed there |
| 3.4 | Delivery directory collision: a different SHA-256 in the same second gets a `-<source>` suffix. Teach `is_delivery_dir_name` and schedule-reference's discovery to accept it and to order it after the bare name | `delivery.rs`, `crates/schedule-reference/src/discovery.rs` | Ordering test in both crates |

## Phase 4: the `charts/distant-signal` `scheduleFeed` changes

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 4.1 | Gate everything SFTP on `scheduleFeed.sftp.enabled` (default `true`): the `sftp` container, the Service (`schedulefeed-service.yaml`), the ConfigMap and `checksum/sftp-entrypoint`, the host-key Secret and volume, `sftp-bootstrap`, the `authMethod` guard, the 2022 ingress rule, the `sftp-metrics` PodMonitor endpoint and the `schedule-sftp` alert group (`$sftpOn`). Fail when `scheduleFeed.enabled` is on with neither source enabled | `schedulefeed-*.yaml`, `networkpolicy.yaml`, `podmonitor.yaml`, `prometheusrule.yaml`, `values.yaml` | CI render step "schedulefeed sources". The **default render is byte-identical** to `origin/main`'s (diff the two `helm template` outputs). With `sftp.enabled=false, bucket.enabled=true`: no `drakkan/sftpgo`, no schedulefeed Service, no `nodePort`, no host-key volume; the 2022 rule is gone. Neither enabled → render fails |
| 4.2 | `scheduleFeed.bucket.*` per §10. `ingest` env: `SFTP_SOURCE_ENABLED`, `BUCKET_SOURCE_ENABLED`, and `BUCKET_*` only when enabled. `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` come from `existingSecret` (fail if empty). Also `SOURCE_PRECEDENCE` and `DISAGREEMENT_WINDOW_MINUTES`. Add the bucket endpoint to the schedulefeed `egressSection` `urls` | `schedulefeed-deployment.yaml`, `networkpolicy.yaml`, `values.yaml` | Render: bucket off → no `BUCKET_` and no `AWS_` env; on → both `secretKeyRef`s name `existingSecret`. `config.rs` `chart_env_wiring_tests` extended to `BUCKET_*`, `SFTP_SOURCE_*` and `SOURCE_*` |
| 4.3 | Alert group `distant-signal.schedule-bucket` (`metrics.prometheusRule.scheduleBucket`, on, renders only with `bucket.enabled`): `DistantSignalScheduleBucketNoNewObject` (30h), `...ReadErrors` (`auth` → critical), `...DownloadBudget` (500 MB/day), `...DeadLetters` (SQS mode only), `DistantSignalScheduleFeedSourcesDisagree` (both sources on). Add a `source` label on the existing rejected counters, keeping the alert expressions source-agnostic | `prometheusrule.yaml`, `values.yaml`, `docs/alerts.md` (a runbook section per alert) | `scripts/check-alert-payloads.py` (sizes, runbook anchors, `promtool check rules`) with the new group added to its `RENDER_FLAGS`; promtool unit tests in `scripts/alert-rules-tests/` for NoNewObject (quiet before the first object; fires at 30h) and ReadErrors |
| 4.4 | Docs: README values rows (`chart-values-doc.py check`); a new `docs/schedule-feed-bucket.md` (the operator page: the switches, adoption steps, key rotation, the audit LogQL); `docs/schedule-feed-sftp.md` gets a short "Alongside the S3 source" section (`sftp.enabled`, dedup, precedence) | as listed | `uv run scripts/chart-values-doc.py check`; `helm lint --strict` on both example files |
| 4.5 | Bump `charts/distant-signal` `version`. Add `charts/ds-ingest-bucket` to `push-helm-chart` (package and push next to `distant-signal`, so Ranma can source it from `oci://ghcr.io/fasterspeeding/charts`) | `Chart.yaml`, `.github/workflows/containers.yml` | actionlint (scripts-lint) |

## Phase 5 (recommended, per D4): the access-log tailer

| # | Task | Files | Tests |
| --- | --- | --- | --- |
| 5.1 | `BUCKET_ACCESS_LOGS_SHIP` (off), `..._BUCKET`, `..._PREFIX`, `..._POLL_INTERVAL_SECS` (600). List after a cursor persisted in `sources/bucket/.access-log-cursor`, GET, parse the S3 server-access-log format (quoted fields, `-` for empty), and write one JSON line per record (`target=schedule_ingest::bucket_access`). Never log the `Authorization` header or query string. The format has neither, but strip `X-Amz-*` signature parameters from `Request-URI` | `access_log.rs` (new), `config.rs`, chart env | Parser against AWS's documented example records; the cursor survives a restart; a malformed line is counted, not fatal |
| 5.2 | Ranma side: Alloy stage `audit="bucket-delivery"` with 400-day retention, and the Loki group `distant-signal-bucket-audit` (§8) | Ranma-Config `logging.yaml` | Ranma's own rule checks |

## Phase 6 (deferred; only if latency matters): the SQS hybrid

| # | Task | Notes |
| --- | --- | --- |
| 6.1 | `common::aws_sigv4`: about 150 lines on `hmac` 0.12.1 and `sha2` (both already in `Cargo.lock`) | Test against AWS's published SigV4 test suite vectors |
| 6.2 | SQS JSON-protocol client: `ReceiveMessage` (`WaitTimeSeconds=20`, `MaxNumberOfMessages=10`), `DeleteMessage`, `ChangeMessageVisibility`, `GetQueueAttributes`. `reqwest` with `X-Amz-Target` | wiremock fake of the endpoint |
| 6.3 | Wake-up loop: a message triggers the bucket source's LIST reconcile early. Delete the message once the cycle has handled every object the message names (match key, plus version or ETag). Extend visibility during extraction. Ignore `s3:TestEvent`. Run the periodic LIST every `reconcileIntervalSecs` (3600). Add the DLQ depth gauge | Table tests: duplicates, out-of-order events, a lost event caught by the reconcile, a poison message reaching the DLQ |
| 6.4 | Chart: `scheduleFeed.bucket.notifications.sqs.*`; `ds-ingest-bucket` `notifications.sqs.enabled: true`; the Ranma `sqs-chart` HelmRelease | Render tests |

## Ranma-Config tasks (the cluster owner; read-only from this repo)

The order matches the spec's §12.

1. **Bootstrap**, run once with OpenTofu (D3), stack
   `aws/ds-ingest-bootstrap/`, in the **new dedicated account** (D2;
   the user creates the account and holds root MFA):
   - the boundary policy and the controller users with the policies in
     §7;
   - the $5/month budget alert (recipient email still open).

   No CloudTrail data events (D4).

   Seal the controller credentials files.
2. **ACK controllers.** In `clusters/mine-bringer/controllers/6.ack/`:
   - a namespace with PSA labels;
   - an OCI `HelmRepository` at `oci://public.ecr.aws/aws-controllers-k8s`;
   - HelmReleases for `s3-chart` 1.12.2 and `iam-chart` 1.9.1 (and
     `sqs-chart` 1.7.1 only with phase 6), each with:
     - `aws.region: eu-west-2`;
     - `aws.credentials.secretName` and `aws.credentials.profile`;
     - `installScope: namespace` with `watchNamespace: ds-ingest-aws`;
     - `reconcile.defaultResyncPeriod: 3600`;
     - resources;
   - the SealedSecrets;
   - an egress NetworkPolicy for 443.

   Then a `c06-ack` Kustomization (`dependsOn: c00-sealed-secrets`).
   Before pinning, check each chart's values keys with `helm show values`.
3. **The bucket.** In `clusters/mine-bringer/apps/`:
   - a `ds-ingest-aws` namespace;
   - a `ds-ingest-bucket` HelmRelease (`enabled: true`, account, bucket
     name, boundary ARN, `writer.mode` per DTD/RDM's answers to spec §14);
   - after sync, the reader key sealed as
     `distant-signal-schedulefeed-bucket` in `distant-signal`.
4. **DS release values:**
   - `scheduleFeed.bucket.enabled: true`;
   - `bucket.bucket`;
   - `bucket.existingSecret: distant-signal-schedulefeed-bucket`;
   - SFTP left on.
5. **Logging and alerts:** phase 5.2, plus routing the new
   `distant-signal.schedule-bucket` alerts through the existing
   ntfy/Grafana path.
6. **Optional, the operator's choice (bucket-only):**
   - `scheduleFeed.sftp.enabled: false`;
   - remove 30450 from `public-exposure-check.yml`'s `ALLOWED_TCP` and the
     NodePort monitoring exception;
   - update `docs/specs/public-ip-exposure.md`.

   This step can be reversed by setting it back.

## Test strategy summary

| Layer | How |
| --- | --- |
| AWS policies | `scripts/check-ingest-bucket-chart.py` (CI), rendering every mode |
| Chart wiring | CI render steps: a byte-identical default render; SFTP-off, bucket-on and neither-on renders; `chart_env_wiring_tests`; `check-alert-payloads.py` and promtool unit tests |
| Source logic | Unit tests with `object_store::memory::InMemory` behind a seam |
| S3 protocol | A wiremock S3 fake: ListObjectsV2 XML, version and ETag headers, 412, 403 |
| End to end | `#[ignore]` MinIO or LocalStack integration in a CI service container. The sandbox has no Docker |
| Multi-source | Table tests for arrival order × precedence × check outcome |
| Regression | Every existing schedule-ingest test unchanged after phase 1 |
| In production | The spec's §12 step 6: a week of `accepted` + `duplicate` pairs with equal sha256 and zero disagreement |

## Off-by-default settings this work adds

- `charts/ds-ingest-bucket`: `enabled`, `reader.allowDelete`,
  `writer.allowList` and `notifications.sqs.enabled`.
- `charts/distant-signal`: `scheduleFeed.bucket.enabled`,
  `bucket.deleteAfterIngest`, `bucket.notifications.sqs.enabled` and
  `bucket.accessLogs.ship`.

`scheduleFeed.sftp.enabled` is new but defaults to **on**, which is
today's behaviour.
