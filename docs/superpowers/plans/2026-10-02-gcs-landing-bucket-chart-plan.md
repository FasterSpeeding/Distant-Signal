# GCS landing bucket: the chart work, task by task

Spec: [2026-10-02-schedule-feed-gcs-landing-design](../specs/2026-10-02-schedule-feed-gcs-landing-design.md)
(decisions D1–D13). Parent plan:
[2026-10-02-schedule-feed-gcs-landing-plan](2026-10-02-schedule-feed-gcs-landing-plan.md).
This plan expands that plan's **phase 4** (and the chart half of phase 5)
into tasks an engineer with no other context can execute, and fixes one
bug in phase 0's `charts/ds-ingest-bucket`. Section numbers (§) refer to
the spec.

It covers only the charts, their CI checks and their docs. The Rust
reader (`crates/schedule-ingest` phases 1–3 and 5 of the parent plan) is a
**separate follow-up**. This plan fixes the interface between the two: the
env vars and metrics in "The interface contract" below. The chart can land
first. Every new setting is off, and an image without the bucket source
ignores the extra env vars (clap only reads the vars it declares).

## Settled; don't reopen

- GCS, europe-west2, provisioned by Crossplane v2 with provider-upjet-gcp
  (D8, D12). The bucket is an **alternative** to SFTP, not a replacement.
  Both can run at once, deduplicated by SHA-256, and the bucket copy wins
  a disagreement (D5, D6).
- Generic framing only (D7). Service-account emails, project ids, bucket
  names, amounts and recipients live in Ranma-Config. Every example here
  is a placeholder (`example-project`, `example-ds-ingest`,
  `...@example-project.iam.gserviceaccount.com`).
- The publisher's (DTD's) service accounts get DTD's four roles at bucket
  level. The reader gets `objectViewer` plus the delete-only custom role
  (D10).
- Data is held only in transit (D11). The reader fetches only expected
  names, verifies size and CRC32C, archives locally, then deletes with
  `ifGenerationMatch`. Unexpected objects are flagged and then deleted
  unread. Versioning is off, soft delete is at its 7-day minimum, and a
  7-day lifecycle rule is the backstop.
- Kill switch (D13): Ranma owns it. The chart labels `BucketIAMMember`s
  with `ds-ingest-bucket/kill-switch-group` and never sets
  `crossplane.io/paused`.
- The reader guards against loops: it never re-downloads a confirmed
  generation, has per-poll, per-hour and per-day caps, and backs off. When
  access is revoked it raises a clear alert, backs off, doesn't crash-loop,
  and leaves SFTP unaffected.

**Update 2026-10-06 (spec D14): keyless reader.** Done after this plan:
`scheduleFeed.bucket.auth: key | workloadIdentity` (default `key`, which
renders byte for byte as before; `check-schedulefeed-chart.py --baseline`
covers both sources and bucket-only + NetworkPolicy in key mode) and
`scheduleFeed.serviceAccount.{create,name}`. In `workloadIdentity` mode the
chart mounts the `external_account` configuration from
`bucket.workloadIdentity.credentialConfigMap` (key `credentialConfigKey`)
at `/var/run/secrets/distant-signal/gcs/credential-config.json`, sets
`GOOGLE_APPLICATION_CREDENTIALS` to it, mounts a projected token
(`bucket.workloadIdentity.audience`, default `gcp-ds-ingest`, 3600 s) at
`/var/run/secrets/distant-signal/gcs-token/token`, requires the dedicated
ServiceAccount (`<release>-schedulefeed`), and opens STS and IAM
Credentials instead of `oauth2.googleapis.com`. The contract table below
has the new env var; the spec's §9 "Keyless reader credentials" has the
reader's side. Key mode stays for deployments without workload identity
federation.

## Ground rules

- Follow `/home/coder/ds-review/fix-brief-common.md`:
  - scratch files only in the session scratchpad;
  - the disk-budget cargo env (`CARGO_INCREMENTAL=0`,
    `CARGO_PROFILE_*_DEBUG=line-tables-only`, the shared
    `CARGO_TARGET_DIR`);
  - Python, not bash, for any new check (stdlib plus the pinned PyYAML,
    ruff, `mypy --strict`, run with `uv run`);
  - the strict config baseline for any config you touch.
- **One task, one commit.** Each commit carries its own tests and CI
  assertions, and the README rows for any values it adds
  (`chart-values-doc.py check` must pass at every commit).
- No credential anywhere in the repo, test fixtures included. The chart
  only ever *references* the reader key's Secret by name.
- The chart **never** renders `crossplane.io/paused`.

## Verification commands (run after every task)

Use the brief's environment. Render flags shared by every
`charts/distant-signal` render below:

```sh
export UV_PROJECT_ENVIRONMENT=$SCRATCH/venv RUFF_CACHE_DIR=$SCRATCH/ruff-cache MYPY_CACHE_DIR=$SCRATCH/mypy-cache
BASE=(--set trustConsumer.kafka.brokers=k:9094 --set trustConsumer.kafka.topic=t
      --set trustConsumer.kafka.saslMechanism=PLAIN --set enricher.llm.baseUrl=http://l/v1
      --set enricher.llm.model=m --set api.sso.issuerUrl=https://sso.example.com
      --set api.sso.clientId=c --set api.sso.clientSecret=s
      --set api.sso.redirectUrl=https://app.example.com/cb
      --set api.sso.postLoginRedirectUrl=https://app.example.com/)

helm lint --strict charts/distant-signal
helm lint --strict charts/distant-signal -f charts/distant-signal/values-example.yaml
helm lint --strict charts/ds-ingest-bucket
helm lint --strict charts/ds-ingest-bucket -f charts/ds-ingest-bucket/ci/example-values.yaml
uv run scripts/check-ingest-bucket-chart.py --download-crds "$SCRATCH/crossplane-crds"   # from task 1
uv run scripts/check-schedulefeed-chart.py                                             # from task 2
uv run scripts/check-alert-payloads.py --report --download-promtool "$SCRATCH/promtool"
uv run scripts/chart-values-doc.py check
uv run scripts/lint-scripts.py
uv run python -m unittest discover -s scripts/tests
actionlint .github/workflows/ci.yml   # also run by lint-scripts.py
```

Task 2 also touches Rust tests:
`cargo test -p schedule-ingest chart_env_wiring` and
`cargo test -p schedule-reference chart_env_wiring`, plus `cargo fmt --all
--check` and `cargo clippy -p schedule-ingest -p schedule-reference
--all-targets -- -D warnings`.

### Byte-identical guarantee (tasks 2 and 3)

The refactors in tasks 2 and 3 must not change any existing render. Check
this once per task, by hand, against the merge base:

```sh
git archive "$(git merge-base main HEAD)" charts/distant-signal | tar -x -C "$SCRATCH/base"
uv run scripts/check-schedulefeed-chart.py --baseline "$SCRATCH/base/charts/distant-signal"
```

`--baseline DIR` renders both charts with the same flags and compares them
after normalisation. Normalisation drops `data`/`stringData` of every
`Secret`, because `genPrivateKey` and `randAlphaNum` differ between runs.
It compares four value sets: the defaults; `scheduleFeed.enabled=true
scheduleFeed.sftp.authMethod=password`; the same plus
`networkPolicy.enabled=true networkPolicy.egress.enabled=true`; and
`-f values-example.yaml`. Any difference is a failure, printed as a
unified diff per document. CI doesn't run `--baseline`: on `main` it would
compare main with itself. The permanent CI check is the "explicit equals
implicit" check in task 3.

## Tasks

### Task 1. `charts/ds-ingest-bucket`: fix AlertPolicy, check against the real CRDs, check the kill-switch labels

**Found while verifying this plan.** `templates/alerts.yaml` renders
`spec.forProvider.documentation` as a **list**. In provider-upjet-gcp
v3.0.0 `monitoring.gcp.m.upbound.io/v1beta1` `AlertPolicy`,
`documentation` is an **object** (`content`, `links`, `mimeType`,
`subject`). With `usageAlerts.enabled` the API server would reject all six
policies. `helm lint` can't see it without the CRDs.

Files:
- `charts/ds-ingest-bucket/templates/alerts.yaml`
- `charts/ds-ingest-bucket/Chart.yaml`: `version: 0.2.1`, with a comment
  line for 0.2.1
- `scripts/check-ingest-bucket-chart.py`
- `.github/workflows/ci.yml`: the `ds-ingest-bucket chart policies` step

1. `alerts.yaml`:

   ```yaml
       documentation:
         content: >-
           The schedule-feed landing bucket {{ $.Values.bucket.name }} passed its
           ...
         mimeType: text/markdown
   ```

2. `check-ingest-bucket-chart.py` gains a schema check:
   - **Flags.** `--crds DIR` (CRD YAMLs already on disk) and
     `--download-crds DIR`. The second downloads the nine files below from
     `https://raw.githubusercontent.com/crossplane-contrib/provider-upjet-gcp/v3.0.0/package/crds/<file>`
     with `urllib.request` and verifies each one's SHA-256 against a
     pinned table, as `check-alert-payloads.py` does for promtool. Pin
     `PROVIDER_UPJET_GCP_VERSION = "3.0.0"` and the table, and comment
     "bump both together by hand".
   - **Without either flag** the schema check is skipped, with one line
     on stderr saying so. CI always passes `--download-crds`.
   - **What it checks.** For every resource rendered in every mode the
     script already renders, look up the CRD by `apiVersion` and `kind`
     and walk `spec` against that version's `openAPIV3Schema`. Fail on:
     - an unknown field (any `properties` map without
       `additionalProperties`);
     - a type mismatch (object, array, string, integer/number, boolean);
     - a string outside an `enum`;
     - a missing `required` field;
     - a resource whose `apiVersion` is not served;
     - any `spec.forProvider.<x>` named by the CRD's
       `x-kubernetes-validations` messages of the form
       `spec.forProvider.<x> is a required parameter` that is absent
       from both `forProvider` and `initProvider`.
   - Keep it under 100 lines; stdlib plus PyYAML.

   | File | SHA-256 (fetched 2026-10-02) |
   | --- | --- |
   | `storage.gcp.m.upbound.io_buckets.yaml` | `3f4fcc974532b86e49af744c432aa1feecba9911fc81dbe4093b4bac86a737f0` |
   | `storage.gcp.m.upbound.io_bucketiammembers.yaml` | `448704a135030012b04535827a55cbba6480a44351727371103969b09ac571a2` |
   | `storage.gcp.m.upbound.io_notifications.yaml` | `14a770e96674348f37993e6292e8d8577586eea92a4885c47115ff5ea411daca` |
   | `cloudplatform.gcp.m.upbound.io_projectiamcustomroles.yaml` | `a1fc3f7da5bea5dbf4b2dd42e54f0c9c6429e4dc240c7627d0a28232a44a91a7` |
   | `monitoring.gcp.m.upbound.io_alertpolicies.yaml` | `76d162546dc90779db96e5ce733b9bc323f253650417f945478079cf8a41e72a` |
   | `pubsub.gcp.m.upbound.io_topics.yaml` | `908ff37dc3f20553438fef86a8e62447132b3675497655c20691c1eba4899eb0` |
   | `pubsub.gcp.m.upbound.io_subscriptions.yaml` | `3c50f42fefdcc64386b48eb6976a49f98aed3a71c562e43ec9665606ebda8aff` |
   | `pubsub.gcp.m.upbound.io_topiciammembers.yaml` | `dad422c633a72a786d861018be0727d2f89df4a56ef53b5ea41f70e5859ecb7b` |
   | `pubsub.gcp.m.upbound.io_subscriptioniammembers.yaml` | `f2acddd4c20ad837fbdae2de54303d2babb6119e92f6adc5d4bcae5e4c956494` |

   Re-download and re-hash before pinning. If a hash differs, the tag was
   moved: stop and report it rather than updating silently.

3. `check-ingest-bucket-chart.py` gains the D13 checks, in every mode:
   - every `BucketIAMMember` whose member is a publisher has label
     `ds-ingest-bucket/kill-switch-group: publisher`;
   - every reader binding (both buckets) has `...: reader`;
   - the audit-sink binding has **no** kill-switch label;
   - **no** rendered resource has a `crossplane.io/paused` annotation.

4. Update the module docstring's bullet list for the new checks.

5. CI step:

   ```yaml
   - name: ds-ingest-bucket chart policies
     run: uv run --no-sync scripts/check-ingest-bucket-chart.py --download-crds "${RUNNER_TEMP}/crossplane-crds"
   ```

Tests and assertions:
- Against the unfixed `alerts.yaml`, the new check must report
  `AlertPolicy/...documentation: expected object` for all six policies.
  With the fix it passes. Record both runs in the commit message body.
- Temporarily add `crossplane.io/paused: "true"` to one binding: the
  paused check fails. Revert.
- The existing `helm lint`/`helm template` CI step is unchanged.

### Task 2. `ingest` env through `mergeEnv`; the new check script

The `ingest` container's env is inline, so `extraEnv` can't override it
and the bucket env (task 4) would have no override path. Move it into a
define rendered by `distant-signal.mergeEnv`, as `sftp` already is. No
render change.

Files:
- `charts/distant-signal/templates/schedulefeed-deployment.yaml`
- `charts/distant-signal/values.yaml`: `scheduleFeed.ingest.extraEnv: []`
- `charts/distant-signal/README.md`: its row
- `crates/schedule-ingest/src/config.rs` (`chart_env_wiring_tests`)
- `crates/schedule-reference/src/config.rs` (`chart_env_wiring_tests`)
- `scripts/check-schedulefeed-chart.py` (new)
- `.github/workflows/ci.yml` (`scripts-lint` job)

1. In the deployment, replace the `ingest` container's `env:` block with:

   ```yaml
             env:
               {{- include "distant-signal.mergeEnv" (dict "env" (include "distant-signal.scheduleFeedIngestEnv" .) "extra" .Values.scheduleFeed.ingest.extraEnv) | trim | nindent 12 }}
   ```

   Move the existing entries, comments included and unchanged, into
   `{{- define "distant-signal.scheduleFeedIngestEnv" }}` at the **end** of
   the file, after `distant-signal.scheduleFeedSftpEnv`. Keep the same
   12-space item indent the sftp define uses. Keep the header comment that
   lists the clap env names, above the include. Add the same one-line
   comment over the define that the sftp define has.

2. Values (`scheduleFeed.ingest`, after `stabilityCycles`):

   ```yaml
       # -- Extra env vars for the ingest container. Same shape as a
       # container `env` list. An entry named like one the chart sets
       # replaces it (one entry per name); other entries follow the chart's.
       extraEnv: []
   ```

3. `schedule-ingest`'s `ingest_container_block()` must also see the
   define. Append the text from
   `{{- define "distant-signal.scheduleFeedIngestEnv" }}` to the next
   `{{- define ` (or EOF):

   ```rust
   const DEFINE: &str = "{{- define \"distant-signal.scheduleFeedIngestEnv\" }}";
   let at = rendered.find(DEFINE).expect("the ingest env define must exist");
   let rest = &rendered[at + DEFINE.len()..];
   let end_def = rest.find("{{- define ").map_or(rendered.len(), |o| at + DEFINE.len() + o);
   block.push_str(&rendered[at..end_def]);
   ```

4. `schedule-reference`'s `reference_container_block()` reads to EOF, so
   it would now include the ingest define. Then an env var set only on
   `ingest` would satisfy the reference check. End the slice at the first
   `\n{{- define ` after `start`.

5. `scripts/check-schedulefeed-chart.py`, new, modelled on
   `check-ingest-bucket-chart.py`:
   - a `Checker` class with `render(*args)` and `docs(*args)`;
   - `container(docs, "ingest")` returns the container mapping;
   - `env(container)` returns `{name: entry}`;
   - `--helm`, and `--baseline DIR` (above).

   Checks in this task:
   - **extraEnv override.** `--set scheduleFeed.ingest.extraEnv[0].name=RUST_LOG`
     with `--set-string ...value=trace` renders `RUST_LOG` once on
     `ingest`, with `trace`, in the chart's position (the entry before
     it is still `PROGRESS_STALL_SECS`). An extra-only name follows the
     chart's entries.
   - **No duplicates.** No container repeats an env name.
   - **Docstring.** Lists every check, as the sibling script does.

6. CI, `scripts-lint` job, after the ds-ingest-bucket step:

   ```yaml
   # charts/distant-signal's schedulefeed in each source mode (SFTP only,
   # bucket only, both, neither): what renders, the ingest env contract,
   # the reader key mount, alerts, and bad values refusing to render.
   - name: schedulefeed chart sources
     run: uv run --no-sync scripts/check-schedulefeed-chart.py
   ```

Tests: the byte-identical baseline run (four value sets, zero diffs); the
two Rust tests; the new script; `chart-values-doc.py check`.

### Task 3. `scheduleFeed.sftp.enabled` (default `true`)

Files:
- `charts/distant-signal/values.yaml` and `README.md`
- `templates/schedulefeed-deployment.yaml`
- `templates/schedulefeed-service.yaml`
- `templates/schedulefeed-configmap.yaml`
- `templates/schedulefeed-secret.yaml`
- `templates/networkpolicy.yaml`
- `templates/podmonitor.yaml`
- `templates/prometheusrule.yaml`
- `templates/NOTES.txt`
- `scripts/check-schedulefeed-chart.py`

Values (first key under `scheduleFeed.sftp`):

```yaml
    # -- Run the SFTP receiver (the `sftp` container, its Service/NodePort,
    # host keys and entrypoint). On by default, so existing values render
    # unchanged. Off for bucket-only delivery (scheduleFeed.bucket); the
    # PVC and the ingest/reference containers stay. scheduleFeed.enabled
    # with neither source enabled fails the render.
    enabled: true
```

Gating. `$sftp := .Values.scheduleFeed.sftp.enabled` throughout.

| Template | What `$sftp` gates |
| --- | --- |
| deployment | The `checksum/sftp-entrypoint` annotation; the `host-key`, `sftp-entrypoint` and `sftp-bootstrap` volumes; the whole `- name: sftp` container; the `authMethod` `fail` (now inside `if $sftp`) |
| service | The whole Service (`if and .Values.scheduleFeed.enabled $sftp`), including the `externalTrafficPolicy` checks |
| configmap, secret | The whole template. The Secret holds only the host keys and the push credential, so with SFTP off nothing needs it |
| networkpolicy | The SFTP-port ingress rule (with `allowedCidrs`) and the telemetry port in the monitoring rule |
| podmonitor | The `sftp-metrics` endpoint (`and $sftp telemetry.enabled`) |
| prometheusrule | `$sftpOn` gains `.Values.scheduleFeed.sftp.enabled` |
| NOTES.txt | The SFTP connection block and the password/host-key read-back blocks |

The "neither source" guard goes at the top of the deployment, beside the
existing one:

```
{{- if not (or .Values.scheduleFeed.sftp.enabled (.Values.scheduleFeed.bucket).enabled) }}
{{- fail "scheduleFeed.enabled needs at least one source: scheduleFeed.sftp.enabled and/or scheduleFeed.bucket.enabled" }}
{{- end }}
```

Until task 4 adds `bucket`, `(.Values.scheduleFeed.bucket).enabled` is
nil, which is correct.

Assertions in `check-schedulefeed-chart.py`:
- **Explicit equals implicit (CI's permanent byte-identity check).**
  Rendering with `scheduleFeed.sftp.enabled=true` is identical to
  rendering without it (normalised), for the four value sets above.
- **SFTP off, bucket not yet available.** `scheduleFeed.enabled=true
  scheduleFeed.sftp.enabled=false` fails, and the error names both
  switches.

Run the baseline comparison.

### Task 4. `scheduleFeed.bucket.*`: values, validation, env, key mount, egress, NOTES

Files:
- `charts/distant-signal/values.yaml` and `README.md`
- `templates/schedulefeed-deployment.yaml`
- `templates/networkpolicy.yaml`
- `templates/NOTES.txt`
- `templates/_helpers.tpl`: a `distant-signal.scheduleFeedValidateBucket`
  define
- `scripts/check-schedulefeed-chart.py`

**Values.** The block goes after `scheduleFeed.corpus`. The two
top-level keys go after `scheduleFeed.reference`.

```yaml
  # -- The Google Cloud Storage delivery source (docs/schedule-feed-bucket.md):
  # schedule-ingest polls a dedicated bucket that the feed provider pushes
  # into, downloads only expected names, verifies size and CRC32C,
  # archives each object on the PVC, then deletes it from the bucket. Runs
  # alongside SFTP or instead of it. The bucket and its IAM are
  # charts/ds-ingest-bucket's. Off by default. Needs a schedule-ingest
  # image with the bucket source.
  bucket:
    enabled: false
    # -- Only `gcs` is implemented.
    provider: gcs
    # -- Bucket name. Required when enabled; set in deploy values.
    name: ""
    # -- Empty: https://storage.googleapis.com. Set only for a fake-GCS
    # test server.
    baseUrl: ""
    # -- Pre-existing Secret holding the reader's service-account key
    # (sealed in deploy config, never in values). Required when enabled.
    # Mounted read-only into `ingest` only, as an optional volume, so a
    # missing Secret leaves SFTP running and raises
    # DistantSignalScheduleBucketAccessRevoked instead of blocking the pod.
    existingSecret: ""
    # -- Key in existingSecret holding the JSON key.
    serviceAccountKey: service-account.json
    # -- Object names (case-insensitive `*` globs, bucket root only) the
    # reader downloads. Anything else is flagged and deleted unread after
    # deleteMinAgeSecs.
    expectedKeys:
      - timetable_full.zip
      - CORPUSExtract.json.gz
    # -- Seconds between bucket listings. At least 60.
    pollIntervalSecs: 300
    # -- An object is deleted only once it is this old, so the
    # publisher's read-back and malware scan finish first. Under 6 days
    # (the bucket's lifecycle backstop is 7).
    deleteMinAgeSecs: 3600
    # -- Raw objects kept under /data/schedule-feed/sources/bucket/archive.
    archiveKeep: 5
    # -- Objects listed larger than this are never downloaded (flagged as
    # unexpected). At most maxDownloadBytesPerHour.
    maxObjectBytes: 268435456
    # -- Loop guards: downloads per poll, and bytes per rolling hour and
    # day. A capped hour or day raises DistantSignalScheduleBucketDownloadBudget.
    maxDownloadsPerPoll: 2
    maxDownloadBytesPerHour: 268435456
    maxDownloadBytesPerDay: 1073741824
    # -- Ceiling of the exponential backoff after errors, and the retry
    # interval while access is revoked. At least pollIntervalSecs.
    maxBackoffSecs: 3600
    # -- Ship the bucket's Cloud Audit Logs (from the audit-log bucket) to
    # stdout as `schedule_ingest::bucket_access` lines for Loki. Off.
    auditLogs:
      ship: false
      # -- Audit-log bucket name. Required with ship.
      bucket: ""
      pollIntervalSecs: 600
  # -- Which source wins when SFTP and the bucket deliver different
  # content within disagreementWindowMinutes (decision D5: the bucket).
  # Must be a permutation of [bucket, sftp].
  sourcePrecedence:
    - bucket
    - sftp
  # -- Different content from the two sources within this many minutes
  # counts as a disagreement (DistantSignalScheduleFeedSourcesDisagree).
  disagreementWindowMinutes: 120
```

`maxObjectBytes` defaults to 256 MiB, not the spec's 512 MiB. An object
over the hourly cap could never be downloaded, and the reader would trip
`DownloadBudget` on every attempt. The real CIF is about 78 MB. See open
question 1.

**Validation** (`fail`, from the deployment, only when `bucket.enabled`):

| Rule | Message names |
| --- | --- |
| `provider` is `gcs` | `scheduleFeed.bucket.provider` |
| `name` matches `^[a-z0-9][a-z0-9_-]{1,61}[a-z0-9]$` (no dots, as `ds-ingest-bucket`) | `scheduleFeed.bucket.name` |
| `existingSecret` matches a DNS-1123 subdomain | `...existingSecret` and "never put the key in values" |
| `serviceAccountKey` matches `^[-._a-zA-Z0-9]+$` | `...serviceAccountKey` |
| `expectedKeys` is non-empty; no entry is empty or contains `/` or `,`; each is ≤ 1024 bytes | `...expectedKeys` |
| `baseUrl` is empty or matches `^https?://[^/]+` | `...baseUrl` |
| `pollIntervalSecs` ≥ 60; `maxBackoffSecs` ≥ `pollIntervalSecs` | both keys |
| 0 ≤ `deleteMinAgeSecs` < 518400 | `...deleteMinAgeSecs` and the 7-day backstop |
| `archiveKeep` ≥ 1; `maxDownloadsPerPoll` ≥ 1 | key |
| 1 ≤ `maxObjectBytes` ≤ `maxDownloadBytesPerHour` ≤ `maxDownloadBytesPerDay` (compare with `int64`) | all three |
| `sourcePrecedence` is exactly `{bucket, sftp}` as a 2-element list | key |
| `disagreementWindowMinutes` ≥ 0 | key |
| `auditLogs.ship` needs `auditLogs.bucket` (same regex, ≠ `name`) and `auditLogs.pollIntervalSecs` ≥ 60 | keys |

**Env.** Inside `distant-signal.scheduleFeedIngestEnv`, after `RUST_LOG`.
Integers via `int64 | quote`.

```yaml
            {{- $sf := .Values.scheduleFeed }}
            {{- if or $sf.bucket.enabled (not $sf.sftp.enabled) }}
            # Source switches (docs/schedule-feed-bucket.md). Rendered only
            # when they differ from schedule-ingest's defaults (SFTP on,
            # bucket off), so an SFTP-only release renders as before.
            - name: SFTP_SOURCE_ENABLED
              value: {{ $sf.sftp.enabled | quote }}
            - name: BUCKET_SOURCE_ENABLED
              value: {{ $sf.bucket.enabled | quote }}
            {{- end }}
            {{- if $sf.bucket.enabled }}
            {{- $b := $sf.bucket }}
            - name: SOURCE_PRECEDENCE
              value: {{ join "," $sf.sourcePrecedence | quote }}
            - name: DISAGREEMENT_WINDOW_MINUTES
              value: {{ $sf.disagreementWindowMinutes | int | quote }}
            - name: BUCKET_NAME
              value: {{ $b.name | quote }}
            {{- with $b.baseUrl }}
            - name: BUCKET_BASE_URL
              value: {{ . | quote }}
            {{- end }}
            # The reader key, mounted from scheduleFeed.bucket.existingSecret.
            - name: GOOGLE_SERVICE_ACCOUNT_PATH
              value: /var/run/secrets/distant-signal/gcs/service-account.json
            - name: BUCKET_EXPECTED_KEYS
              value: {{ join "," $b.expectedKeys | quote }}
            - name: BUCKET_POLL_INTERVAL_SECS
              value: {{ $b.pollIntervalSecs | int64 | quote }}
            # ... BUCKET_DELETE_MIN_AGE_SECS, BUCKET_ARCHIVE_KEEP,
            # BUCKET_MAX_OBJECT_BYTES, BUCKET_MAX_DOWNLOADS_PER_POLL,
            # BUCKET_MAX_DOWNLOAD_BYTES_PER_HOUR, BUCKET_MAX_DOWNLOAD_BYTES_PER_DAY,
            # BUCKET_MAX_BACKOFF_SECS, same pattern
            {{- if $b.auditLogs.ship }}
            - name: BUCKET_AUDIT_LOGS_SHIP
              value: "true"
            - name: BUCKET_AUDIT_LOGS_BUCKET
              value: {{ $b.auditLogs.bucket | quote }}
            - name: BUCKET_AUDIT_LOGS_POLL_INTERVAL_SECS
              value: {{ $b.auditLogs.pollIntervalSecs | int64 | quote }}
            {{- end }}
            {{- end }}
```

**Key mount.** In `volumes`, after `sftp-bootstrap` (outside the `$sftp`
gate):

```yaml
        {{- if .Values.scheduleFeed.bucket.enabled }}
        # The bucket reader's service-account key (a sealed Secret from
        # deploy config; the chart never renders it). Optional: a missing
        # Secret or key must not stop the pod, and so SFTP; schedule-ingest
        # reports it as revoked access instead. No subPath, so a rotated
        # key reaches the running pod.
        - name: bucket-credentials
          secret:
            secretName: {{ .Values.scheduleFeed.bucket.existingSecret }}
            optional: true
            defaultMode: 0440
            items:
              - key: {{ .Values.scheduleFeed.bucket.serviceAccountKey }}
                path: service-account.json
        {{- end }}
```

Add `volumeMounts` on `ingest` **only**:
`{name: bucket-credentials, mountPath: /var/run/secrets/distant-signal/gcs, readOnly: true}`.

**PVC.** No new volume. The archive lives on the existing PVC under
`STORAGE_DIR`. Add one sentence to `scheduleFeed.persistence.size`'s
comment: with the bucket on, budget `(archiveKeep + 1) × maxObjectBytes`
on top of today's use. The defaults fit in 5Gi: about 0.5 GB for real
78 MB objects, against about 2.3 GB for today's three extracted
deliveries.

**NetworkPolicy.** The schedulefeed `egressSection` call:

```
{{- $sfUrls := list .Values.internalOauth.tokenUrl }}
{{- if .Values.scheduleFeed.bucket.enabled }}
{{- /* Ports only: the internet rule is by CIDR, so these add 443 (or a
custom baseUrl's port) when networkPolicy.egress.internetPorts is set. */}}
{{- $sfUrls = concat $sfUrls (list "https://storage.googleapis.com" "https://oauth2.googleapis.com") }}
{{- with .Values.scheduleFeed.bucket.baseUrl }}{{ $sfUrls = append $sfUrls . }}{{ end }}
{{- end }}
{{- with include "distant-signal.egressSection" (dict "root" . "component" "schedulefeed" "deps" (dict "api" true) "internet" true "urls" $sfUrls) }}
```

schedulefeed already has `"internet" true`, so egress to Google on 443
already works. This keeps it working when an operator narrows
`internetPorts`. `object_store` 0.14.2 signs its own JWT from a
service-account key and never calls a token endpoint
(`gcp/credential.rs`, `SelfSignedJwt`). `oauth2.googleapis.com` is listed
only in case the reader ever does. (2026-10-06: in `auth:
workloadIdentity` mode the list is `storage`, `sts` and `iamcredentials`
instead; all three are 443.)

**NOTES.txt.** When `bucket.enabled`, print:
- the bucket name (`gs://<bucket>`);
- the Secret name and key it expects, with a reminder that it is sealed
  in deploy config and never set in values;
- the expected keys;
- one line saying that charts/ds-ingest-bucket must grant the reader.

When SFTP is off, print "SFTP receiver disabled (bucket-only)" instead of
the connection block.

**Assertions** in `check-schedulefeed-chart.py`. Every mode renders with
`BASE`, `scheduleFeed.enabled=true` and `scheduleFeed.sftp.authMethod=password`.
`BUCKET` means `--set scheduleFeed.bucket.enabled=true
--set scheduleFeed.bucket.name=example-ds-ingest
--set scheduleFeed.bucket.existingSecret=distant-signal-schedulefeed-bucket`.

| Mode | Must hold |
| --- | --- |
| SFTP only (default) | No env name on any container starting `BUCKET_` or `GOOGLE_`, nor `SFTP_SOURCE_ENABLED`, `BUCKET_SOURCE_ENABLED`, `SOURCE_PRECEDENCE`, `DISAGREEMENT_WINDOW_MINUTES`. No `bucket-credentials` volume. The schedulefeed Deployment has containers `[sftp, ingest, reference]` |
| Both (`BUCKET`) | Containers `[sftp, ingest, reference]`. `ingest` has `SFTP_SOURCE_ENABLED="true"`, `BUCKET_SOURCE_ENABLED="true"`, `SOURCE_PRECEDENCE="bucket,sftp"`, `DISAGREEMENT_WINDOW_MINUTES="120"`, `BUCKET_NAME="example-ds-ingest"`, `GOOGLE_SERVICE_ACCOUNT_PATH="/var/run/secrets/distant-signal/gcs/service-account.json"`, `BUCKET_EXPECTED_KEYS="timetable_full.zip,CORPUSExtract.json.gz"` and every `BUCKET_*` default from the contract table, as plain integers (no `e+`). No `BUCKET_BASE_URL` and no `BUCKET_AUDIT_LOGS_*`. Volume `bucket-credentials`: `secretName` is the existingSecret, `optional: true`, `defaultMode` 0o440 (288), one item `service-account.json` with key `service-account.json`. Mounted `readOnly` on `ingest`, not on `sftp` or `reference`. No `sftp` or `reference` env name starts `BUCKET_`/`GOOGLE_` |
| Bucket only (`BUCKET` + `sftp.enabled=false`) | Containers `[ingest, reference]`. `SFTP_SOURCE_ENABLED="false"`. No schedulefeed Service, no `-sftp-entrypoint` ConfigMap, no chart-rendered schedulefeed Secret (the `ingest`/`reference` OAuth Secrets stay). No `host-key`/`sftp-entrypoint`/`sftp-bootstrap` volumes, no `checksum/sftp-entrypoint` annotation. PVC present. Renders without `sftp.authMethod` too |
| Bucket only + `networkPolicy.enabled=true networkPolicy.egress.enabled=true networkPolicy.egress.internetPorts={443}` | The schedulefeed NetworkPolicy has no ingress rule on 2022 and no 9097. Its internet rule's ports include 443. With `bucket.baseUrl=http://fake-gcs:4443` they include 4443 |
| Both + `bucket.auditLogs.ship=true bucket.auditLogs.bucket=example-ds-ingest-audit` | The three `BUCKET_AUDIT_LOGS_*` vars, as in the contract |
| Both + `scheduleFeed.ingest.extraEnv[0]={name: BUCKET_POLL_INTERVAL_SECS, value: "120"}` | Rendered once, value `"120"` |
| Every mode | No rendered document contains `private_key` or `BEGIN PRIVATE KEY` outside the chart's own generated SFTP host-key Secret. No rendered Secret is named the `existingSecret` |
| Contract | Every env name in the contract table appears on `ingest` in "Both + auditLogs + baseUrl" |

Failures (render must exit non-zero, and the message must name the key):
- neither source;
- bucket on with an empty `existingSecret`;
- bucket on with an empty `name`;
- `name=Example.Bucket`;
- `expectedKeys=null`;
- `expectedKeys[0]=a/b`;
- `maxObjectBytes=300000000` (over the hour cap);
- `maxDownloadBytesPerHour=2000000000` (over the day cap);
- `sourcePrecedence={sftp}`;
- `sourcePrecedence={bucket,bucket}`;
- `provider=s3`;
- `auditLogs.ship=true` without a bucket;
- `baseUrl=ftp://x`;
- `deleteMinAgeSecs=604800`;
- `pollIntervalSecs=10`.

Also extend the existing bash step `helm template (extraEnv overrides chart
env by name)` in `ci.yml`. Its all-containers duplicate check adds
`--set scheduleFeed.enabled=true --set scheduleFeed.sftp.authMethod=password`
and the `BUCKET` flags, so it covers the new env.

### Task 5. Bucket alerts

Files:
- `charts/distant-signal/values.yaml` and `README.md`: the
  `metrics.prometheusRule.<alert>` row gains `scheduleBucket`; one row per
  alert in the alerts table
- `templates/prometheusrule.yaml`
- `docs/alerts.md`
- `scripts/check-alert-payloads.py` (`RENDER_FLAGS`)
- `scripts/alert-rules-tests/schedule-bucket.yaml` (new)
- `scripts/check-schedulefeed-chart.py`

Values, after `scheduleSftp`:

```yaml
    scheduleBucket:
      enabled: true
      # DistantSignalScheduleBucketAccessRevoked: every bucket call answered
      # 401/403 (the kill switch removed the reader, or the key was rotated,
      # disabled or is missing).
      accessRevokedFor: 10m
      # DistantSignalScheduleBucketNoNewObject: no new expected object for
      # this many hours, once one has been seen. Daily CIF at ~20:00 UTC.
      noNewObjectHours: 30
      noNewObjectFor: 30m
      # DistantSignalScheduleBucketReadErrors: list/get/verify/delete errors.
      readErrorsWindow: 1h
      readErrorsFor: 15m
      # DistantSignalScheduleBucketUnexpectedObject.
      unexpectedWindow: 1h
      # DistantSignalScheduleFeedSourcesDisagree (both sources on).
      disagreeWindow: 1d
      severity: warning
      criticalSeverity: critical
```

Template. Add `$bucketOn := and ($rule.scheduleBucket).enabled
.Values.scheduleFeed.enabled (.Values.scheduleFeed.bucket).enabled` and
OR it into `$anyOn`. A new group `distant-signal.schedule-bucket` goes
after `schedule-sftp`. All metric names carry the `distant_signal_`
prefix that `common::metrics::metric_name` adds. The spec's sketches omit
it. `NoNewObject`'s expr is close to `check-alert-payloads.py`'s 300-character
limit. If it goes over, record
`max by (namespace) (distant_signal_schedule_feed_source_last_new_object_seconds{source="bucket"})`
as `distant_signal:schedule_bucket_last_new_object_seconds:max`, in the
group's own recording rules, as `distant_signal:consumer_api_errors:increase`
does, and alert on that.

| Alert | `expr` | `for` | Severity |
| --- | --- | --- | --- |
| `DistantSignalScheduleBucketAccessRevoked` | `max by (namespace) (distant_signal_schedule_feed_source_access_revoked{namespace="NS", source="bucket"}) == 1` | `accessRevokedFor` | critical |
| `DistantSignalScheduleBucketNoNewObject` | `time() - max by (namespace) (distant_signal_schedule_feed_source_last_new_object_seconds{namespace="NS", source="bucket"}) > H*3600 and on (namespace) max by (namespace) (distant_signal_schedule_feed_source_last_new_object_seconds{namespace="NS", source="bucket"}) > 0` | `noNewObjectFor` | warning |
| `DistantSignalScheduleBucketReadErrors` | `sum by (namespace) (increase(distant_signal_schedule_feed_source_errors_total{namespace="NS", source="bucket", kind!="auth"}[W])) > 0` | `readErrorsFor` | warning |
| `DistantSignalScheduleBucketUnexpectedObject` | `sum by (namespace) (increase(distant_signal_schedule_feed_source_unexpected_objects_total{namespace="NS", source="bucket"}[W])) > 0` | `0m` | warning |
| `DistantSignalScheduleBucketDownloadBudget` | `max by (namespace) (distant_signal_schedule_feed_source_download_capped{namespace="NS", source="bucket"}) == 1` | `0m` | critical |
| `DistantSignalScheduleFeedSourcesDisagree` (only if `scheduleFeed.sftp.enabled` too) | `sum by (namespace, kind) (increase(distant_signal_schedule_feed_source_disagreement_total{namespace="NS"}[disagreeWindow])) > 0` | `0m` | warning |

Annotations stay within `check-alert-payloads.py`'s limits: a one-line
summary of 80 characters or less, a description of 150 or less, and
`runbook_url` = `docs/alerts.md#<lowercased alert name>`. Summaries:
- "Schedule-feed bucket access revoked";
- "No new object in the schedule-feed bucket in {{ H }}h";
- "Schedule-feed bucket read errors";
- "Unexpected object in the schedule-feed bucket";
- "Schedule-feed bucket download cap hit";
- "SFTP and bucket delivered different {{ $labels.kind }}".

The AccessRevoked description says the SFTP source carries on and
recovery is a Ranma reapply.

`docs/alerts.md`: a new `## schedule bucket` section after
`## schedule SFTP`, with one `### <AlertName>` per alert, so
`check-alert-payloads.py`'s anchor check passes. Each covers:

- **AccessRevoked**: the three kill-switch trips (spec §8 table). DS
  backs off and logs once per state change; nothing in DS restarts it.
  Check the audit log for a reader loop (repeated `objects.get` of one
  generation). Then ask Ranma for a deliberate reapply, which unpauses
  the `kill-switch-group: reader` bindings. Also covers a missing or
  rotated key: check `kubectl get secret` exists, without reading it.
- **NoNewObject**: SFTP may still be delivering
  (`...PublishStale` is authoritative). Publisher-side causes include a
  publisher kill-switch trip.
- **ReadErrors**: by `kind`.
- **UnexpectedObject**: the object is deleted unread and recoverable for
  7 days from soft delete; how to inspect it (link
  `docs/schedule-feed-bucket.md`).
- **DownloadBudget**: a loop or an attack, stopped by the reader. Don't
  raise the caps before finding the cause.
- **SourcesDisagree**: the bucket copy won (D5); compare the two
  SHA-256s in the audit lines.

`check-alert-payloads.py` `RENDER_FLAGS` add
`scheduleFeed.bucket.enabled=true`,
`scheduleFeed.bucket.name=example-ds-ingest` and
`scheduleFeed.bucket.existingSecret=distant-signal-schedulefeed-bucket`.

`scripts/alert-rules-tests/schedule-bucket.yaml`, in `health.yaml`'s
shape:
- **AccessRevoked.** The gauge goes 0 then 1 at 5m. Not firing at 14m;
  firing at 16m with the exact labels and annotations.
- **NoNewObject.** The series is 0 throughout: never fires. The series
  is a timestamp 31h old: fires after 30m.
- **DownloadBudget.** Fires on the first evaluation at 1.
- **UnexpectedObject.** The counter goes 0→1: fires; it stays flat for
  more than 1h: resolves.

Assertions in `check-schedulefeed-chart.py`:
- bucket off: no `distant-signal.schedule-bucket` group;
- both on: six alerts;
- bucket only: five (no `SourcesDisagree`) and no `schedule-sftp` group;
- `scheduleBucket.enabled=false`: no group.

### Task 6. Docs, chart versions, publishing

Files:
- `docs/schedule-feed-bucket.md` (new)
- `docs/schedule-feed-sftp.md`
- `charts/distant-signal/Chart.yaml`
- `charts/distant-signal/README.md`
- `charts/ds-ingest-bucket/README.md`
- `.github/workflows/containers.yml`
- `docs/superpowers/README.md`

1. **`docs/schedule-feed-bucket.md`.** The ds-ingest-bucket AlertPolicy
   documentation already links it, and it doesn't exist yet. It covers:
   - the two switches and the four modes;
   - adoption (spec §12), steps 3–7, from DS's side;
   - an object's life (D11);
   - the reader key: where it is sealed, the 90-day two-key rotation
     (no pod restart needed, since the mount has no subPath), and never
     reading the Secret;
   - restoring a soft-deleted object (`gcloud storage restore
     gs://<bucket>/<name>#<generation>`, placeholders only);
   - the kill-switch runbook (§8 table; recovery is a deliberate Ranma
     reapply that removes `crossplane.io/paused`);
   - audit LogQL for `schedule_ingest::bucket_access`;
   - the PVC sizing note from task 4.

   Generic only: no project ids, emails or bucket names.
2. **`docs/schedule-feed-sftp.md`.** A short "Alongside the bucket
   source" section: `scheduleFeed.sftp.enabled`, what bucket-only
   removes (the NodePort, the only public listener), and that Ranma then
   updates `public-exposure-check.yml`.
3. **Chart README.** The `### scheduleFeed` intro gains two sentences on
   sources. Check that every row added in tasks 2–5 is present
   (`chart-values-doc.py check`).
4. **Versions.** `charts/distant-signal/Chart.yaml` `version` gets a
   minor bump (0.3.0 → 0.4.0, new opt-in features), with a one-line
   comment as the file's convention requires. `ds-ingest-bucket` was
   already bumped in task 1.
5. **Publishing.** `containers.yml` `push-helm-chart` also packages and
   pushes `charts/ds-ingest-bucket`. It has no images to pin, so it
   needs `helm package charts/ds-ingest-bucket` and `helm push` to the
   same `oci://ghcr.io/fasterspeeding/charts` with the Chart.yaml
   version as is. A same-version re-push is how distant-signal is
   handled today: read that job's comments on version minting and follow
   the same scheme (`<version>-<run>` or equivalent). See open
   question 5.
6. **`docs/superpowers/README.md`.** Update the GCS spec row's note:
   which chart pieces landed and what's still missing (the Rust source).

Tests: `chart-values-doc.py check`, `actionlint`, `helm lint --strict`
for both charts, and every script above.

## The interface contract (chart → schedule-ingest)

The chart sets these env vars. The Rust follow-up (parent plan, phases
1–3 and 5) declares them with clap `#[arg(long, env)]`, using these exact
names, defaults and meanings. Extend `chart_env_wiring_tests` to cover
the `SFTP_SOURCE_`, `BUCKET_`, `SOURCE_`, `DISAGREEMENT_` and `GOOGLE_`
prefixes. Task 2 already makes the test read the define.

| Env var | Rendered when | Default in code | Semantics |
| --- | --- | --- | --- |
| `SFTP_SOURCE_ENABLED` | bucket on, or SFTP off | `true` | Scan `WATCH_DIR` as today. `false`: never read `WATCH_DIR` |
| `BUCKET_SOURCE_ENABLED` | bucket on, or SFTP off | `false` | Run the GCS source. Both false: exit non-zero at start with a clear message (the chart already refuses this) |
| `SOURCE_PRECEDENCE` | bucket on | `bucket,sftp` | Comma list; a permutation of `bucket`,`sftp`. Disagreement winner (D5) and same-second ordering |
| `DISAGREEMENT_WINDOW_MINUTES` | bucket on | `120` | Different SHA-256s for the same kind within this window are a disagreement |
| `BUCKET_NAME` | bucket on | none (required when enabled) | Bucket to list at the root (`delimiter=/`) |
| `BUCKET_BASE_URL` | bucket on and set | empty = `https://storage.googleapis.com` | Base URL for both the `object_store` client and the hand-written JSON-API calls (metadata GET, conditional DELETE). Tests only |
| `GOOGLE_SERVICE_ACCOUNT_PATH` | bucket on, `auth: key` | none | Path to the JSON key. Pass it to `GoogleCloudStorageBuilder::with_service_account_path` explicitly; don't rely on `from_env`. **Missing, unreadable or invalid:** don't exit. Treat it as revoked access (gauge 1, one log line per state change, retry at `BUCKET_MAX_BACKOFF_SECS`), and re-read the file on each retry so a fixed or rotated Secret recovers without a restart. Never log the file's contents |
| `GOOGLE_APPLICATION_CREDENTIALS` | bucket on, `auth: workloadIdentity` (2026-10-06) | none | Path to an `external_account` credential configuration (`/var/run/secrets/distant-signal/gcs/credential-config.json`); its `credential_source.file` is the projected token at `/var/run/secrets/distant-signal/gcs-token/token`. Load it with `common::gcp_external_account::ExternalAccountConfig::from_file` plus `check_google_endpoints()`, wrap an `ExternalAccountTokenSource` in an `object_store::CredentialProvider` and pass it to `GoogleCloudStorageBuilder::with_credentials` (spec §9, "Keyless reader credentials"); never `from_env`. Exactly one of this and `GOOGLE_SERVICE_ACCOUNT_PATH` is set; both or neither: exit at start. **Missing, unreadable or invalid, or a token exchange refused** (`CredentialError::is_access_revoked`): revoked access, exactly as for the key, re-reading the file on each retry. Never log the file's subject token or any access token |
| `BUCKET_EXPECTED_KEYS` | bucket on | none (required, non-empty) | Comma list of case-insensitive `*` globs (same matcher as `CIF_FILE_PATTERN`), matched against root object names. A match is downloaded only if it is also routable: CIF by `CIF_FILE_PATTERN`/`CIF_EXCLUDE_PATTERN`, or CORPUS by `CORPUS_FILE_PATTERN` with `CORPUS_INGEST_ENABLED=true`. A matching name that isn't routable is handled as unexpected, reason `unroutable` |
| `BUCKET_POLL_INTERVAL_SECS` | bucket on | `300` | The bucket source's own schedule, independent of `POLL_INTERVAL_SECS` |
| `BUCKET_DELETE_MIN_AGE_SECS` | bucket on | `3600` | Delete a confirmed or unexpected object only once `now - timeCreated ≥` this. Delete with `ifGenerationMatch=<seen generation>`: 412 means a newer upload, keep it; 404 means already gone, fine |
| `BUCKET_ARCHIVE_KEEP` | bucket on | `5` | Raw objects kept under `$STORAGE_DIR/sources/bucket/archive/`. Partials go in `$STORAGE_DIR/sources/bucket/.partial-*`, the ledger in `.../.seen`. Nothing is written outside `STORAGE_DIR` (the root filesystem is read-only) |
| `BUCKET_MAX_OBJECT_BYTES` | bucket on | `268435456` | A listed size above this is never downloaded; it is unexpected, reason `size` |
| `BUCKET_MAX_DOWNLOADS_PER_POLL` | bucket on | `2` | Hitting it defers to the next poll. It does **not** set `download_capped` |
| `BUCKET_MAX_DOWNLOAD_BYTES_PER_HOUR` | bucket on | `268435456` | Rolling hour. Before each GET: if bytes in the window plus the listed size exceed the cap, don't GET; set `download_capped=1` until the window allows it; log once per state change. Verification failures count their bytes too |
| `BUCKET_MAX_DOWNLOAD_BYTES_PER_DAY` | bucket on | `1073741824` | Rolling 24 h, same rules |
| `BUCKET_MAX_BACKOFF_SECS` | bucket on | `3600` | Errors back off exponentially from the poll interval up to this; reset on success. While revoked, retry at this interval |
| `BUCKET_AUDIT_LOGS_SHIP` | `auditLogs.ship` | `false` | Parent plan, phase 5.1 |
| `BUCKET_AUDIT_LOGS_BUCKET` | `auditLogs.ship` | none (required with ship) | Audit-log bucket, read with the same credential (key or keyless) |
| `BUCKET_AUDIT_LOGS_POLL_INTERVAL_SECS` | `auditLogs.ship` | `600` | |

Behaviour the chart relies on:
- **SFTP is never affected.** Bucket-source errors, backoff, revocation
  and caps never block the SFTP scan cycle, never exit the process, and
  never stall the `/livez` progress heartbeat (`PROGRESS_STALL_SECS`).
  A long download heartbeats while it streams. Downloads stream to disk
  and are never buffered whole (ingest's memory limit is 256Mi).
- **Never re-download** a `(name, generation)` already in `.seen`, across
  restarts.
- **Every outbound call** has a connect timeout and an overall timeout.

Metrics, all `{source="bucket"}` unless noted, registered at 0 at start
when the bucket source is on. Names before `common::metrics::metric_name`
adds `distant_signal_`; the alerts in task 5 depend on them:

| Metric | Type | Notes |
| --- | --- | --- |
| `schedule_feed_source_access_revoked` | gauge 0/1 | 401/403 from any call, or no usable key |
| `schedule_feed_source_last_new_object_seconds` | gauge | Unix time of the last new expected object seen; 0 until the first |
| `schedule_feed_source_errors_total{kind}` | counter | `kind` ∈ `auth`, `list`, `get`, `verify`, `delete`, `size` |
| `schedule_feed_source_unexpected_objects_total{reason}` | counter | `reason` ∈ `name`, `size`, `unroutable` |
| `schedule_feed_source_download_capped` | gauge 0/1 | The hour or day cap only |
| `schedule_feed_source_downloaded_bytes_total` | counter | |
| `schedule_feed_source_poll_duration_seconds` | histogram | |
| `gcp_token_exchange_total{stage, outcome}` | counter, no `source` label | Keyless mode only (registered by `ExternalAccountTokenSource`). `stage` ∈ `sts`, `impersonation`; `outcome` ∈ `success`, `token_file_error`, `invalid_grant`, `invalid_target`, `invalid_request`, `unauthenticated`, `permission_denied`, `http_error`, `timeout`, `error` |
| `gcp_token_remaining_seconds` | gauge, no `source` label | Keyless mode only: the cached access token's remaining lifetime |
| `schedule_feed_source_disagreement_total{kind}` | counter, no `source` label | `kind` ∈ `cif`, `corpus` |

## Off-by-default guarantees

- `scheduleFeed.bucket.enabled: false` and `bucket.auditLogs.ship: false`.
  With these off, nothing bucket-related renders: no env, no volume, no
  egress URL, no alert group. Task 4's "SFTP only" assertions enforce it.
- `scheduleFeed.sftp.enabled` is new and defaults to **true**. Any values
  file that doesn't set it renders exactly as before. Tasks 2–3 check
  this once against the merge base (`--baseline`), and CI checks
  explicit-equals-implicit for good.
- `metrics.prometheusRule.scheduleBucket.enabled: true` renders nothing
  unless `scheduleFeed.bucket.enabled`.
- `ds-ingest-bucket`: `enabled`, `usageAlerts.enabled` and
  `notifications.pubsub.enabled` stay `false`. Task 1 changes no default.
- `scheduleFeed.ingest.extraEnv: []` renders nothing.

## How the Crossplane fields were verified

- **The CRDs.** Downloaded on 2026-10-02 from
  `crossplane-contrib/provider-upjet-gcp` tag `v3.0.0`, `package/crds/`.
  The nine kinds `ds-ingest-bucket` renders are listed in task 1 with
  their SHA-256. Every one is `Namespaced`, and `v1beta1` is served and is
  the storage version (`Bucket` also serves `v1beta2`).
- **Rendered and walked.** `ds-ingest-bucket` was rendered with every
  feature on (usage alerts, Pub/Sub, CMEK), and each resource's `spec` was
  walked against its CRD schema, checking unknown fields, types, enums,
  `required`, and the `... is a required parameter` CEL rules.
  - Result: 24 resources; the only error was `AlertPolicy.forProvider.documentation`
    (list, schema says object), for all six policies (task 1).
  - With that changed to an object, all 26 resources validate,
    including the Pub/Sub IAM members.
  - Fields confirmed present:
    - `Bucket`: `softDeletePolicy.retentionDurationSeconds`,
      `lifecycleRule[].condition.age`, `lifecycleRule[].action.type`,
      `versioning`, `publicAccessPrevention`,
      `uniformBucketLevelAccess`, `enableObjectRetention`;
    - `BucketIAMMember`: `forProvider.{bucket, role, member, condition}`;
    - `ProjectIAMCustomRole`: `forProvider.{project, title, description,
      stage, permissions}`;
    - `Subscription`: `expirationPolicy.ttl` (`""` = never expires).
- **The pause annotation.** `crossplane.io/paused` is
  `AnnotationKeyReconciliationPaused` in crossplane-runtime v2.0.0
  `pkg/meta/meta.go`. `IsPaused` is true only for the exact value
  `"true"`. The chart only labels; it never sets this.
- **No token endpoint.** That a service-account key needs no OAuth
  token endpoint comes from `object_store` 0.14.2's `src/gcp/credential.rs`
  (`ServiceAccountCredentials::token_provider` → `SelfSignedJwt`), in
  the local cargo registry.

Task 1 makes this check permanent in CI.

## Out of scope; follow-ups

- **The Rust reader**: the parent plan's phases 1–3 and 5, against the
  contract above. Until it ships, keep `scheduleFeed.bucket.enabled`
  false. An older image ignores the env vars, so nothing would happen.
- **Pub/Sub on the DS side** (parent plan 6.3): no
  `scheduleFeed.bucket.notifications.*` values until the code exists.
- **Ranma-Config**: the HelmRelease values, the sealed reader key
  `distant-signal-schedulefeed-bucket` (key `service-account.json`) in
  key mode, or in keyless mode the `external_account` ConfigMap, the pool
  provider (uploaded JWKS, subject
  `system:serviceaccount:<ns>:<release>-schedulefeed`, allowed audience
  `gcp-ds-ingest`) and the impersonation binding; and the kill-switch
  watcher. Also check that helm-controller drift detection
  doesn't strip a `crossplane.io/paused` it doesn't own.

## Open questions (decided 2026-10-02)

The user confirmed the defaults below: 256 MiB, an optional volume,
the rename to `name` (3), no DS-side Pub/Sub values yet, and publishing
`ds-ingest-bucket` to the OCI registry.

1. **`maxObjectBytes` default.** 256 MiB, not the spec's 512 MiB, so it
   fits under the hourly cap. The alternative is raising
   `maxDownloadBytesPerHour` to 512 MiB, which allows twice the egress
   per hour in a loop.
2. **Missing key Secret.** The volume is `optional: true`, so the pod
   (and SFTP) starts and the bucket source reports `AccessRevoked`. The
   alternative, fail-fast (the pod stuck in `ContainerCreating`), would
   also stop SFTP.
3. **The value name.** Settled: `scheduleFeed.bucket.name` (renamed from
   spec §10's `scheduleFeed.bucket.bucket` before any deploy values used
   it; the spec and parent plan now say `name`). `auditLogs.bucket` keeps
   its name: it names a second bucket.
4. **Pub/Sub values** are deferred, as above.
5. **Publishing `ds-ingest-bucket`.** It is pushed to the same OCI
   registry as distant-signal. If Ranma's Flux reads charts from a
   GitRepository instead, drop task 6 step 5.
