# Schedule feed: an S3 delivery source alongside SFTP

Design, 2026-10-02. Status: **proposed**, with the user's decisions D1–D6 recorded the same day (below). Nothing here is deployed.

This change adds `charts/ds-ingest-bucket`, off by default and installed
nowhere. Everything else in this document is a proposal, and
[the plan](../plans/2026-10-02-schedule-feed-s3-landing-plan.md) is how to
build it.

## Decisions (user, 2026-10-02)

| # | Decision | Effect on this design |
| --- | --- | --- |
| D1 | **Cloud: AWS S3, eu-west-2.** | §3 |
| D2 | **A new, dedicated AWS account** owns the bucket and everything in it. The user holds its root MFA. An AWS Budget alert at **$5 a month**. | §7 bootstrap creates the account baseline. The 100 GB free egress allowance belongs to this workload alone, so the typical cost in §13 holds |
| D3 | **Provisioning: ACK via Helm** (`charts/ds-ingest-bucket`). Access keys are created **once by hand** and sealed in Ranma-Config. **OpenTofu only for the one-off setup**: account baseline, controller users, permissions boundary, budget. | §4, §7. The "OpenTofu alone" alternative is no longer under consideration |
| D4 | **Audit: S3 server access logs only.** No CloudTrail data events. | §8. CloudTrail keeps only its free default 90-day management-event history (IAM and bucket changes) |
| D5 | **Precedence: the bucket wins** when SFTP and the bucket deliver different content within the disagreement window. The disagreement alert still fires. | §9. `sourcePrecedence: [bucket, sftp]` becomes the fixed default |
| D6 | **Both sources are kept long-term.** Nothing is retired. | §10, §12 |

**Default unless the user objects:** clean up by lifecycle expiry only, so
the reader never deletes (§5, "Deleting processed objects").

Still open: §14.

## Summary

The Rail Data Marketplace (RDM, "DTD" in older docs) can push file feeds to
three places: SFTP, or a customer bucket in AWS, Google Cloud or Azure. It
cannot push to our own S3-compatible store (Thoth). Today Distant Signal
takes the CIF timetable and the CORPUS extract over SFTP:

- an SFTPGo container on NodePort 30450, the only listener on the node's
  public IP;
- schedule-ingest, which reads the shared PVC.

This design adds an **S3 bucket in AWS eu-west-2 (London)** as a second,
equal delivery source. Each source is switched on separately: SFTP only,
bucket only, or both at once.

- **Cloud and region.** AWS S3, eu-west-2. The user chose this, based on
  the cost and security research in the "Cloud Bucket Ingest Costs"
  artifact. GCS europe-west2 is the fallback (§3).
- **Bucket.** Private, TLS-only, SSE-S3, versioned, and with
  lifecycle-expiry. S3 server access logs go to a second private bucket.
  - The publisher gets `PutObject` on one prefix and nothing else.
  - schedule-ingest gets list and get on that prefix, and by default no
    delete.
- **Provisioning.** A new Helm chart, `charts/ds-ingest-bucket`, renders
  ACK (AWS Controllers for Kubernetes) resources. The ACK s3, iam and sqs
  controllers are installed by Ranma-Config. ACK was chosen over
  Crossplane for footprint and least privilege (§4). OpenTofu is the
  non-Helm alternative, and it does the one-off bootstrap.
- **How DS learns of new objects.** It polls `ListObjectsV2` on the prefix
  every 5 minutes. S3 → SQS notifications were evaluated in full (§6). They
  are templated in both charts but off: at one CIF a day, with
  schedule-reference publishing every 30 minutes, they buy nothing that
  justifies a queue, a DLQ, a third controller and a hand-rolled SigV4
  client.
- **schedule-ingest.**
  - Sources become peers behind one abstraction: the SFTP watch directory
    and the S3 bucket.
  - One content-addressed pipeline extracts, validates, audits and posts
    each delivery **once**, whichever source or sources it came through.
    It dedups by SHA-256 and records the source.
  - A fixed precedence decides when the two sources disagree.
- **Adoption.** Stand up the bucket and turn the bucket source on
  alongside SFTP. Ask RDM to add the bucket as a second destination, then
  verify both. Running bucket-only, which closes the public port, is an
  operator choice. Nothing is retired.
- **Cost.** About $0.06 a month typical, and under $1.50 in the worst
  realistic case. A $5 budget alert guards against a runaway download
  loop.

## 1. What RDM supports, and what is still unknown

From the "Cloud Bucket Ingest Costs" artifact (prices checked 2026-10-02),
the Open Rail Data wiki and the 2026-10-01 SFTP evidence in
[schedule-feed-sftp.md](../../schedule-feed-sftp.md).

### Confirmed

| Fact | Value | Source |
| --- | --- | --- |
| Push destinations | AWS S3, Azure Blob, Google Cloud Storage, or SFTP. No pull option. Our own S3-compatible store (Thoth) is not a target | User; Open Rail Data wiki ("Rail Data Marketplace") |
| Publisher location | Google Cloud, europe-west2; the SFTP pushes come from one GCP address | SFTP `login` lines |
| CIF file | `timetable_full.zip`, 77.2–77.9 MB, which extracts to a ~724 MB MCA file. Deflate, about 12 entries | SFTP `Upload` lines; ingest |
| CIF cadence | Daily, about 20:00 UTC (19:59:59 on 2026-09-30) | SFTP log; PVC mtimes |
| CORPUS | `CORPUSExtract.json.gz`, ~788 KB, about monthly. Also `CORPUSExtract.csv.gz` (SMART, ~296 KB, ignored) | Artifact; `config.rs` |
| Upload behaviour (SFTP) | One session per delivery. It writes straight to the final name and overwrites in place, with no temp name and rename | schedule-feed-sftp.md |
| Monthly volume | ~2.36 GB and ~33 PUTs | Artifact |

### Not confirmed

No public RDM documentation describes the S3 destination form. The
artifact marks every item below as unconfirmed, and so does this design.
The chart handles each of them, but the user needs to check them on RDM's
portal or with RDM support (§12).

| Unknown | Why it matters | How the design copes |
| --- | --- | --- |
| **Credential model.** Does RDM take an IAM access key pair, or write as its own AWS principal (cross-account)? Does it need a role ARN or an external ID? | Decides whether we hand over a key or grant a principal | `writer.mode: iamUser` (default) or `crossAccount` with `writer.principalArns` |
| **Permissions the client needs.** HEAD, LIST, delete, or temp-name-and-rename (copy plus delete)? | Rename or delete weakens write-only | `writer.allowList` (off); delete is never granted. If RDM needs rename, that is a decision for the user (§12) |
| **Object keys.** The same key overwritten daily, as on SFTP, or dated keys? Is there a prefix we can set? | Change detection and the lifecycle rule | Both work: the source tracks (key, ETag, LastModified, size) and the version id. Filename globs are unchanged (`timetable_full.zip`, `CORPUSExtract.json.gz`) |
| **Multipart upload** for the 78 MB zip? | A multipart ETag is not an MD5 | No ETag-as-MD5 assumption anywhere. Dedup is by our own SHA-256 |
| **ACL header and SSE header** on the PUT | `BucketOwnerEnforced` rejects any ACL other than `bucket-owner-full-control`. An `aws:kms` header would need a key grant | Ask RDM. SSE-S3 is the default, so no header is needed |
| **Delivery timing and retries** to a bucket | Alert windows | Same 30h "no new object" window as SFTP |
| **Two destinations at once.** Can one subscription push to SFTP and S3 together, or does it need two subscriptions? | Running both sources at once depends on it | Ask before the parallel phase. Fallback: two subscriptions, or alternate destinations (§12) |
| **Notifications** from RDM | None known | Not relied on |

## 2. Today's path, and what stays the same

The schedulefeed Deployment (`charts/distant-signal/templates/schedulefeed-*.yaml`)
runs three containers that share a PVC:

- **`sftp`**: SFTPGo v2.7.5. `dtd-push` has upload, overwrite and list on
  one directory. It has the defender, rate limits, JSON audit log,
  telemetry and the `distant-signal.schedule-sftp` alerts. The Loki rules
  in Ranma add new-source and off-hours login alerts
  (Ranma-Config `docs/specs/sftp-audit-observability.md`).
- **`ingest`**: `crates/schedule-ingest`. Every `POLL_INTERVAL_SECS` (120)
  it scans `WATCH_DIR`, waits for `STABILITY_CYCLES` (5) unchanged polls,
  then for the CIF zip:
  1. SHA-256s it.
  2. Extracts it atomically, within entry and byte caps.
  3. Runs the CIF content checks (`cif_check.rs`).
  4. Writes `.delivery-complete`.
  5. POSTs `schedule_feed_ingests`.
  6. Writes `.delivery-ingested` (name, size, mtime ns, SHA-256).
  7. Prunes to `retentionKeepDeliveries` (3).

  Quarantine is by mtime. CORPUS (`corpus.rs`) is checked, loaded and
  archived to `storage_dir/corpus/`. One `schedule_ingest::audit` JSON line
  is written per decision.
- **`reference`**: `crates/schedule-reference`. It publishes from the
  newest complete delivery directory every 30 minutes.

All of this stays. The S3 source changes only how a candidate file reaches
the pipeline, and what "delivered at" and "already seen" mean for it.

## 3. Options and recommendation

### Cloud

| | AWS S3 eu-west-2 | GCS europe-west2 | Azure Blob UK South |
| --- | --- | --- | --- |
| RDM can push to it | Yes | Yes | Yes |
| Typical / worst monthly (artifact) | $0.06 / $1.30 | $0.37 / $1.10 | $0.07 / $0.29 |
| Egress to our node | Within the account-wide 100 GB/month free allowance | $0.12/GiB, no free tier in London | 100 GB/month free |
| Writer credential | IAM user key (PutObject on a prefix), or a cross-account principal | Service-account key or HMAC, or a grant to RDM's own Google service account | SAS token (create + write) |
| Reader credential | IAM user key, pinnable to the node's IP by bucket policy (`aws:SourceIp`) | Service-account key or HMAC | SAS or account key |
| Audit | S3 server access logs (free apart from storage); CloudTrail data events (pennies) | Data Access logs (free tier) | Diagnostic logs |
| Kubernetes-native provisioning | ACK (AWS-maintained), Crossplane | Config Connector (heavy), Crossplane | ASO, Crossplane |
| Already in DS | `object_store` `aws` feature (aggregator archive) | No | No |

**Decision (user, 2026-10-02): AWS S3 in eu-west-2.** It is the cheapest
typical case. The reader key can be pinned to an IP. Workspace
dependencies already cover it. ACK makes it Helm-deployable with three
small controllers.

**Fallback: GCS europe-west2**, and only if RDM's S3 form turns out to
need something we won't give, such as an account-wide key.

- GCS can grant RDM's own Google service account `roles/storage.objectCreator`
  on the bucket, so no long-lived writer key exists at all.
- It costs about $0.30 a month more.
- `object_store`'s `gcp` feature covers the reader.
- The ingest design in §5 is provider-neutral apart from the version
  token: GCS has `generation`, S3 has `versionId`.

Azure is ruled out. SAS tokens are the weakest writer credential, and
RDM may ask for an account key.

### SFTP alongside

Both sources are first-class and switched on separately
(`scheduleFeed.sftp.enabled`, `scheduleFeed.bucket.enabled`), long-term. §12
is an adoption plan, not a migration.

## 4. Provisioning: Helm via ACK (recommended) vs Crossplane vs OpenTofu

Helm can't call AWS. A Kubernetes controller that turns custom resources
into AWS API calls can, and Helm renders those resources. Two serious
options were considered.

| | **ACK** (aws-controllers-k8s) | **Crossplane v2** + provider-upjet-aws |
| --- | --- | --- |
| Pieces | One controller per service: `s3-controller` v1.12.2, `iam-controller` v1.9.1, `sqs-controller` v1.7.1 (all released 2026-09-18) | Crossplane core v2.4.2, plus the provider family and `provider-aws-s3`, `-iam`, `-sqs` (provider-upjet-aws v2.8.1) |
| Footprint on the single-node k3s | Three small Go controllers with a handful of CRDs: `Bucket`; `User`, `Role`, `Policy` and others (7 in iam); `Queue`; plus ACK's `AdoptedResource` and `FieldExport` | Core, RBAC manager and one pod per provider. Upjet providers wrap Terraform providers and typically need several hundred MiB each. They install large CRD sets unless trimmed with v2 activation policies. On a node whose disk and memory are already shared with every PVC and build cache, this is the deciding point |
| Maturity | AWS-maintained. These three controllers are GA. API group `v1alpha1` despite GA | CNCF graduated, widely used, more general (compositions) |
| Access keys | **No `AccessKey` kind** (iam CRDs checked 2026-10-02: groups, instanceprofiles, openidconnectproviders, policies, roles, servicelinkedroles, users). A person mints keys and seals them (§7) | `AccessKey` writes the key into a connection Secret automatically |
| Controller credentials on k3s | No IRSA or Pod Identity off EKS. The Helm values take a static shared-credentials Secret (`aws.credentials.secretName`, `profile`), which Ranma seals | Same: a `ProviderConfig` pointing at a static-credentials Secret |
| Least privilege for the controller | Easy. The iam controller never needs `iam:CreateAccessKey`. Scope by IAM path, permissions boundary, exact bucket ARNs and a queue-name prefix | Same scoping is possible, but auto-minted keys need `iam:CreateAccessKey` on the users |
| Drift | Periodic resync (`reconcile.defaultResyncPeriod`; set 1h). Console changes are reverted at the next resync | Continuous poll, about 10 minutes by default. Stronger |
| Delete safety | `services.k8s.aws/deletion-policy: retain`, plus Helm `resource-policy: keep` | `deletionPolicy: Orphan` |

**Recommendation: ACK.** The footprint is decisive on this node. ACK's
missing `AccessKey` is a security plus here: the controller can't mint
credentials, and keys never sit in a custom resource or an unsealed
Secret that Git doesn't track. That fits Ranma's sealed-secret practice.

Crossplane would make sense if Ranma later manages many more cloud
resources and wants compositions.

**OpenTofu** (in `mise`) is the non-Helm alternative. It has a plan and
apply review step, real drift detection on `tofu plan`, and no in-cluster
controller holding AWS write credentials. It is the better tool for
**bootstrap**: the account, the controller users, the permissions
boundary and the budget alarm, which must exist before any controller can
run. This design uses OpenTofu, or the equivalent CLI, only for that
bootstrap. The Helm chart is the deliverable for the bucket and its
access.

**Decided (D3, 2026-10-02):**

- ACK via Helm.
- Keys created once by hand and sealed in Ranma-Config.
- OpenTofu only for the one-off setup.

For the record only: every resource in §5 would map one-to-one onto
OpenTofu `aws_s3_bucket*`, `aws_iam_user*` and `aws_sqs_queue*`, with
schedule-ingest unchanged.

### Chart placement

The chart is separate (`charts/ds-ingest-bucket`), not a subchart or part
of `charts/distant-signal`, for four reasons:

- It needs the ACK CRDs. `charts/distant-signal` must keep installing on
  clusters without them (docker-compose parity, the dev cluster).
- Its lifecycle differs. AWS resources are retained on uninstall and
  rarely change. The app chart rolls with every image.
- It installs into its own namespace (`ds-ingest-aws`), which the ACK
  controllers watch. App namespaces never get ACK custom resources.
- Its blast radius and RBAC are separate. Only Ranma's Flux applies it.

## 5. The bucket and its access (`charts/ds-ingest-bucket`)

Committed with this spec, off by default. Its README lists the templates.
Every resource carries `services.k8s.aws/region: eu-west-2`.

### Delivery bucket (`templates/bucket.yaml`, ACK `Bucket`)

| Setting | Value | Why |
| --- | --- | --- |
| Name | `bucket.name` (required; no dots) | Virtual-hosted TLS |
| Region | eu-west-2 (`createBucketConfiguration.locationConstraint`) | Closest to the publisher (GCP London) and to the node; free egress allowance |
| Block Public Access | all four on | |
| Object ownership | `BucketOwnerEnforced` | ACLs off; we own cross-account writes |
| Encryption | SSE-S3 (`AES256`), Bucket Key on | Free, and needs no KMS grant for a cross-account writer. SSE-KMS with a customer managed key costs $1/month and adds a key policy for RDM. It is available (`bucket.encryption.sseAlgorithm: aws:kms`) but not recommended: the data is public timetable data, and this is defence in depth only |
| Versioning | `Enabled` | Pins each ingest to a version id. Keeps a replaced delivery for 3 days, which is forensics after a leaked writer key. Makes "delete exactly what I ingested" safe if delete is ever turned on |
| Lifecycle | Prefix `rdm/`: current versions expire after 14 days, noncurrent after 3 (matching `retentionKeepDeliveries: 3`), unfinished multipart uploads abort after 1 day. A second rule removes expired delete markers. S3 rejects `expiredObjectDeleteMarker` together with `days` in one rule | The reader never needs to delete. Storage stays at about 3–4 CIF zips |
| Policy | Deny any request that is not TLS (`aws:SecureTransport`) or is below TLS 1.2 (`s3:TlsVersion`). Optionally deny the reader from any IP outside `reader.allowedSourceCidrs` (the node's egress /32). In `crossAccount` mode, allow `writer.principalArns` `s3:PutObject` (and `AbortMultipartUpload`) on `rdm/*` | |
| Access logs | To `<bucket>-logs/s3-access/` | §8 |
| Deletion | `services.k8s.aws/deletion-policy: retain`, `helm.sh/resource-policy: keep` | An uninstall never destroys deliveries |
| Object Lock | Off | It would block lifecycle expiry. The data is re-pushed daily |

The publisher's source address (34.147.132.114 on SFTP) is **not** put
into the bucket policy. If Google re-homes RDM's egress, deliveries would
silently fail. A new writer source IP is an alert (§8), the same posture
as SFTP (`DistantSignalSftpDtdPushNewSource`).

### Access-log bucket (`templates/log-bucket.yaml`)

- Same privacy settings as the delivery bucket.
- SSE-S3, which server access logging requires on the target.
- The policy allows only `logging.s3.amazonaws.com` to `PutObject` under
  `s3-access/`, with `aws:SourceArn` set to the delivery bucket and
  `aws:SourceAccount` set to our account.
- Objects expire after 400 days, the SFTP delivery-audit retention.

### IAM (`templates/iam-users.yaml`, ACK `User` with inline policies)

Both users live under path `/ds-ingest/` and carry the permissions
boundary `iam.permissionsBoundaryArn`.

| Identity | Grants | Not granted |
| --- | --- | --- |
| `ds-ingest-rdm-writer` (`writer.mode: iamUser`) | `s3:PutObject` and `s3:AbortMultipartUpload` on `arn:aws:s3:::<bucket>/rdm/*`. Optionally `s3:ListBucket` with an `s3:prefix` condition | Get, delete, list (by default), and anything outside the prefix. With `crossAccount`, no user exists |
| `ds-ingest-reader` (schedule-ingest) | `s3:ListBucket` and `ListBucketVersions` with `s3:prefix` `rdm/` and `rdm/*`; `s3:GetObject` and `GetObjectVersion` on `rdm/*`; read on `<bucket>-logs/s3-access/*` for the access-log tailer; with SQS, receive, delete and change-visibility on the main queue, and only `GetQueueAttributes` on the DLQ | Any write. Delete only with `reader.allowDelete` |

A leaked writer key can write junk under `rdm/`. That is the same blast
radius as a leaked SFTP password, and schedule-ingest validates every file,
as it does today. Lifecycle expiry caps the storage cost of junk. A leaked
reader key can read public timetable data. With `allowedSourceCidrs` set,
it can do so only from our node.

### Deleting processed objects

The design chooses **not to delete**. Lifecycle expiry does the cleanup,
for four reasons:

- The reader stays read-only.
- A crashed or rolled-back ingest can re-fetch.
- A bad deploy can't destroy an unprocessed delivery.
- It needs no conditional delete. Without versioning, "GET, then DELETE"
  races an overwrite and can delete an unprocessed upload.

If the user wants the bucket emptied on ingest, use
`reader.allowDelete: true` and `scheduleFeed.bucket.deleteAfterIngest: true`.
schedule-ingest then deletes **the exact version id it ingested**
(`DeleteObject` with `versionId`), which can't touch a newer upload.

### Optional SQS (`templates/sqs.yaml`, `notifications.sqs.enabled: false`)

- A standard queue `ds-ingest-events` with SSE-SQS. FIFO queues can't be
  S3 targets. An AWS-managed KMS key would refuse S3.
- Long-poll 20s, visibility 900s, retention 4 days.
- A redrive policy to `ds-ingest-events-dlq` after 5 receives. The DLQ
  keeps messages 14 days and its redrive-allow policy is limited to the
  main queue.
- The queue policy allows only `s3.amazonaws.com` `sqs:SendMessage`, with
  `aws:SourceArn` set to the bucket and `aws:SourceAccount` set to our
  account, and denies non-TLS.
- The bucket's `notification.queueConfigurations` sends
  `s3:ObjectCreated:*` filtered to prefix `rdm/`.
- S3 validates the destination when the configuration is written, so the
  `Bucket` stays unsynced until the queue and its policy exist. ACK
  retries, so this is eventually consistent.

### Rendering and tests

```sh
helm lint --strict charts/ds-ingest-bucket                     # renders nothing (enabled: false)
helm lint --strict charts/ds-ingest-bucket -f charts/ds-ingest-bucket/ci/example-values.yaml
helm template t charts/ds-ingest-bucket -f charts/ds-ingest-bucket/ci/example-values.yaml \
  --set notifications.sqs.enabled=true
uv run scripts/check-ingest-bucket-chart.py                    # policy assertions, CI scripts-lint job
```

CI runs all four: the `helm-lint` job, and the `scripts-lint` job after it
installs Helm.

`check-ingest-bucket-chart.py` parses every rendered policy and checks:

- the writer has only `PutObject` and `AbortMultipartUpload` on `rdm/*`,
  in both writer modes;
- the reader has no write, and no delete unless allowed;
- every `ListBucket` has an `s3:prefix` condition;
- both buckets block public access, are owner-enforced and TLS-only, and
  have `resource-policy: keep`;
- the log bucket accepts only the logging service;
- the queue takes only this bucket's S3 events and redrives to the DLQ;
- bad values refuse to render.

Install needs the ACK CRDs. `helm template` and `helm lint` don't.

The full templates are in `charts/ds-ingest-bucket/templates/`. They are
the source of truth and are not repeated here.

## 6. How DS learns of a new object

The cluster has no inbound path except the Cloudflare tunnel, so only
**pull** consumers were considered seriously. Inbound push options are
assessed for completeness.

| Option | Latency | Reliability | Cost at a few objects a day | Extra AWS and IAM | Extra in charts and controllers | Rust at MSRV 1.88 |
| --- | --- | --- | --- | --- | --- | --- |
| **A. Poll `ListObjectsV2`** on `rdm/` every 5 min, then HEAD and GET new objects | ≤5 min (average 2.5) | Self-healing. A missed cycle is caught by the next. Nothing to lose | ~8,800 LISTs a month, about $0.05 | None beyond `ListBucket` | None | `object_store` 0.14.2 (`aws`, already in the workspace, MSRV 1.85): `list`, `head`, `get_opts` (`if_match`, `version`); `ObjectMeta` has `e_tag` and `version` |
| **B. S3 → SQS**, schedule-ingest long-polls (`ReceiveMessage`, `WaitTimeSeconds=20`) | Seconds; AWS says "typically seconds, sometimes a minute or longer" | At-least-once: duplicates happen, there is no ordering (`sequencer` orders events per key), and notifications for concurrent writes to one key can be coalesced. Needs a DLQ and an alert. Visibility of 900s must exceed the 1–2 min CIF extraction | ~131k long-polls a month, inside SQS's 1M free requests (otherwise about $0.05) | Queue policy for `s3.amazonaws.com` with `aws:SourceArn`/`aws:SourceAccount`; reader gets `sqs:ReceiveMessage`/`DeleteMessage`/`ChangeMessageVisibility`/`GetQueueAttributes` | ACK sqs-controller; `Queue` + DLQ; bucket `notification` (all templated, off) | **`aws-sdk-sqs` is out:** v1.114.0 needs rustc 1.94.1 (checked 2026-10-02), and the smithy tree would bring cargo-deny duplicates. The viable route is the SQS JSON protocol (`application/x-amz-json-1.0`, `X-Amz-Target: AmazonSQS.ReceiveMessage`) over the existing `reqwest`, with a ~150-line SigV4 signer on `hmac` 0.12.1 and `sha2` (both already in `Cargo.lock`), tested against AWS's published SigV4 vectors. `object_store`'s signer is not public API |
| **C. S3 → EventBridge → SQS** | As B, plus about 1 s | As B. EventBridge adds an archive and replay, content filtering (key, size) and fan-out to several targets | S3 events to the default bus are free; the rule target is SQS as in B | Bucket `EventBridgeConfiguration`, a rule, a target role or queue policy for `events.amazonaws.com` | ACK's `Bucket.notification` has no EventBridge switch, so a second mechanism would be needed (eventbridge-controller, or OpenTofu) | As B |
| **D. S3 → SNS → SQS** | As B | As B, plus SNS retries | Free tier | Topic and its policy, a subscription, a queue policy for SNS | ACK sns-controller as well | As B |
| **D′. S3 → SNS → HTTPS push** to an endpoint behind the tunnel | Seconds | SNS HTTP retries are bounded (the default delivery policy gives up after a few attempts over about 20s) and need the endpoint up. Subscription confirmation and signature verification would have to be built | Free tier | Topic, subscription, an internet-reachable endpoint | A new **inbound** route through the Cloudflare tunnel to schedule-ingest, which has no HTTP surface today | New HTTP handler and SNS signature verification |
| **E. S3 → Lambda** | Seconds | Good | Pennies | Function, role | Code outside the cluster | Out of scope: processing happens in the cluster. A Lambda could only relay to B or D′ |
| **F. Hybrid**: B for latency, plus A every hour as a safety net and for bootstrap and backfill | Seconds, or ≤1h if events are lost | Best of both. Messages are hints; the LIST is the truth | As B, plus ~730 LISTs (a cent) | As B | As B | As B |

**Recommendation: A, polling.**

- The pipeline after ingest is not latency-sensitive. schedule-reference
  publishes from the newest complete delivery every 30 minutes
  (`scheduleFeed.reference.pollIntervalSecs: 1800`), and the CIF lands once
  a day at about 20:00 UTC. The ≤5 minutes polling costs is lost in that
  interval.
- Polling has no failure mode that loses a delivery. B and C have three:
  coalesced events, DLQ stalls, and a missed `s3:TestEvent` edge.
- Polling needs no third controller, no queue, no extra IAM and no SigV4
  code.
- The cost difference is pennies either way.

D′ is the worst fit. It is the only option that **re-opens an inbound
path**, which the S3 source exists partly to remove.

If latency ever matters, for example if schedule-reference becomes
event-driven, switch to **F** with both charts' SQS switches.

- `ds-ingest-bucket`'s `notifications.sqs.*` is already templated and
  tested.
- `scheduleFeed.bucket.notifications.sqs.*` is designed in §10.

In F, schedule-ingest treats each message only as a **wake-up hint**:

1. It runs the normal LIST reconcile, which is idempotent by version id.
2. It deletes the message once that cycle has handled the object.
3. It extends visibility with `ChangeMessageVisibility` while a long
   extraction runs.
4. It ignores `s3:TestEvent`.
5. It exports the DLQ depth (`GetQueueAttributes ApproximateNumberOfMessages`)
   for the `DistantSignalScheduleBucketDeadLetters` alert.

Duplicates and misordering then don't matter. The S3 event's
`object.key`, `versionId`, `eTag`, `size` and `sequencer` are logged with
the cycle but are not trusted as the source of truth.

**Testing B or F:** a wiremock fake of the SQS JSON endpoint (wiremock is
already a dev-dependency) for receive, delete, visibility and the DLQ
gauge, plus the SigV4 vectors. LocalStack (S3 + SQS + notifications) is
for `--ignored` integration tests in CI as a service container. The
sandbox has no Docker.

## 7. Credentials and their handling

Workload identity (IRSA, Pod Identity) is not available on k3s, so every
AWS principal used from the cluster is an IAM user with a static key. Each
key is minted by a person and sealed with `kubeseal` into Ranma-Config. No
key is ever written to Git unencrypted, to a custom resource, or to this
repo.

| Key | Holder | Sealed as | Rotation |
| --- | --- | --- | --- |
| `ack-s3-controller`, `ack-iam-controller`, `ack-sqs-controller` (only if notifications are on) | ACK controllers in `ack-system` | One SealedSecret each, a shared-credentials file referenced by the chart's `aws.credentials.secretName` | 180 days |
| `ds-ingest-reader` | schedule-ingest | `distant-signal-schedulefeed-bucket` (`access-key-id`, `secret-access-key`), the same shape as `archive.s3.existingSecret` | 90 days, two-key overlap: create the new key, reseal, roll, then delete the old one |
| `ds-ingest-rdm-writer` (`iamUser` mode) | RDM | Not stored by us. It is entered in RDM's destination form and never kept | 90 days, or as RDM allows; two-key overlap, then confirm the next delivery |

### Controller credentials: bootstrap and least privilege

Bootstrap runs once, with an admin profile, as OpenTofu in a new
`aws/ds-ingest-bootstrap/` stack in Ranma-Config, or the equivalent
`aws iam` CLI. It creates three things.

**1. The permissions boundary `ds-ingest-boundary`** (path `/ds-ingest/`).
It is the ceiling for every user the iam controller creates:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "DeliveryObjects", "Effect": "Allow",
     "Action": ["s3:PutObject", "s3:AbortMultipartUpload", "s3:GetObject", "s3:GetObjectVersion",
                "s3:DeleteObject", "s3:DeleteObjectVersion"],
     "Resource": ["arn:aws:s3:::<bucket>/rdm/*", "arn:aws:s3:::<bucket>-logs/s3-access/*"]},
    {"Sid": "ListPrefixes", "Effect": "Allow",
     "Action": ["s3:ListBucket", "s3:ListBucketVersions"],
     "Resource": ["arn:aws:s3:::<bucket>", "arn:aws:s3:::<bucket>-logs"]},
    {"Sid": "Events", "Effect": "Allow",
     "Action": ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility",
                "sqs:GetQueueAttributes", "sqs:GetQueueUrl"],
     "Resource": "arn:aws:sqs:eu-west-2:<account>:ds-ingest-*"}
  ]
}
```

**2. Controller users with these inline policies.** Treat them as a
starting point, and compare each with the controller's own
`config/iam/recommended-inline-policy` at install. Trim what that file
lists beyond what the chart uses.

- **s3 controller:**
  - `s3:CreateBucket`, `DeleteBucket`, `Get*`/`Put*` for
    `BucketPolicy`, `BucketVersioning`, `EncryptionConfiguration`,
    `LifecycleConfiguration`, `BucketLogging`, `BucketNotification`,
    `BucketTagging`, `BucketOwnershipControls`,
    `BucketPublicAccessBlock` and `BucketLocation`, plus
    `DeleteBucketPolicy`, all on exactly the two bucket ARNs;
  - `s3:ListAllMyBuckets` on `*`, which the controller uses for
    existence checks.
- **iam controller:**
  - `iam:CreateUser`, `PutUserPolicy`, `DeleteUserPolicy`,
    `AttachUserPolicy`, `DetachUserPolicy` and
    `PutUserPermissionsBoundary`, with a
    `Condition: StringEquals iam:PermissionsBoundary = <boundary ARN>`;
  - `iam:GetUser`, `UpdateUser`, `DeleteUser`, `TagUser`, `UntagUser`,
    `ListUserTags`, `GetUserPolicy`, `ListUserPolicies`,
    `ListAttachedUserPolicies`, `ListAccessKeys` and
    `ListGroupsForUser`;
  - everything on `arn:aws:iam::<account>:user/ds-ingest/*` only;
  - an explicit **Deny** on `iam:CreateAccessKey`,
    `iam:DeleteUserPermissionsBoundary`, `iam:CreatePolicyVersion`,
    `iam:SetDefaultPolicyVersion` and `iam:DeletePolicy` for the
    boundary. The controller can't mint keys, can't lift the ceiling,
    and can't touch anyone outside `/ds-ingest/`.
- **sqs controller:** `sqs:CreateQueue`, `DeleteQueue`,
  `GetQueueAttributes`, `SetQueueAttributes`, `GetQueueUrl`, `TagQueue`,
  `UntagQueue` and `ListQueueTags` on
  `arn:aws:sqs:eu-west-2:<account>:ds-ingest-*`.

**3. Account hygiene** (D2: a new, dedicated account).

- The user holds root MFA. There are no console users beyond one admin.
- An AWS Budget at $5 a month, alerting by email (the artifact's runaway
  re-download case is $270–380 a month). The recipient address is still
  open (§14).
- CloudTrail's default 90-day management event history, which records
  every IAM and bucket change the controllers make.

A compromised controller key can, at worst, re-shape these two buckets
and their users, and only within the boundary.

## 8. Audit and alerting for the bucket source

The SFTP audit, including the Loki rules, defender and telemetry, stays
exactly as it is while SFTP is enabled. For the bucket source:

### Audit trail

S3 server access logs land in `<bucket>-logs/s3-access/`. Each record
gives the requester ARN, remote IP, time, operation (`REST.PUT.OBJECT`),
key, HTTP status, error code, bytes, user agent and version id.

Delivery is best-effort, typically within an hour. That is good enough for
anomaly alerts, not for real-time ones.

**Decided (D4, 2026-10-02): server access logs are the only object-level
audit.** CloudTrail S3 data events, the guaranteed alternative, are not
used. ACK's `Trail` couldn't configure them anyway. Because access logs
can be delayed or, rarely, lost, the Prometheus alerts below, driven by
schedule-ingest's own observations, are the timely signal. The Loki
rules are forensic and anomaly checks.

### Shipping to Loki

An optional access-log tailer in schedule-ingest
(`scheduleFeed.bucket.accessLogs.ship`):

1. Every 10 minutes it lists `s3-access/` after a persisted cursor (the
   keys sort by time).
2. It GETs each new log object.
3. It parses the space-delimited format.
4. It logs one JSON line per record with
   `target=schedule_ingest::bucket_access` and the fields `requester`,
   `remote_ip`, `operation`, `key`, `status`, `error_code`, `bytes`,
   `version_id` and `time`.

Alloy, on the Ranma side, tags these `audit="bucket-delivery"` with the
same retention as the SFTP delivery audit (400 days).

### Loki rules (Ranma `loki-rules`, group `distant-signal-bucket-audit`)

The bucket equivalents of the SFTP rules:

| Alert | Fires when |
| --- | --- |
| `DistantSignalBucketWriterNewSource` | A `REST.PUT.OBJECT` by the writer principal from an IP not seen in the previous 7 days. Same shape as `DistantSignalSftpDtdPushNewSource` |
| `DistantSignalBucketWriterOffHours` | A writer PUT outside the observed window |
| `DistantSignalBucketAccessDenied` | Five or more 403s on the bucket in 15 minutes, which means someone is probing with a bad or stale key |
| `DistantSignalBucketUnexpectedPrincipal` | Any write or delete by a principal other than the writer, or (with `allowDelete`) the reader |

Login failures and brute force have no equivalent: there is no listener
to attack. AWS authentication failures surface only as 403s on our
bucket.

### Prometheus

These are new schedule-ingest metrics, all labelled `source`. The alerts
are in a new chart group, `distant-signal.schedule-bucket`, which renders
only with `scheduleFeed.bucket.enabled`.

| Alert | Expression (sketch) | Default |
| --- | --- | --- |
| `DistantSignalScheduleBucketNoNewObject` | `time() - schedule_feed_source_last_new_object_seconds{source="bucket"} > 30h` (only once one has been seen) | 30h, warning. This is the bucket twin of `DistantSignalSftpNoUpload` |
| `DistantSignalScheduleBucketReadErrors` | `increase(schedule_feed_source_errors_total{source="bucket"}[1h]) > 0` for 15m. `kind` is `auth` (403, expired or rotated key: **critical**), `list`, `get` or `size` | warning; critical for `auth` |
| `DistantSignalScheduleBucketDownloadBudget` | `increase(schedule_feed_source_downloaded_bytes_total{source="bucket"}[1d]) > 500 MB` | The runaway-loop guard (normal is ~80 MB a day) |
| `DistantSignalScheduleBucketDeadLetters` | DLQ depth > 0, only in SQS mode | |
| `DistantSignalScheduleFeedSourcesDisagree` | `increase(schedule_feed_source_disagreement_total[1d]) > 0`, only with both sources on | §9, "When the two sources disagree" |

The existing `DistantSignalScheduleFeedZipRejected`,
`DistantSignalCorpusRejected` and
`DistantSignalScheduleReferencePublishStale` stay source-agnostic.
`...PublishStale` remains the authoritative "no timetable" signal. The
quarantine counters gain a `source` label.

## 9. schedule-ingest: several concurrent sources

### Shape

```rust
/// Where a delivery came from. Stable strings: they appear in audit lines,
/// `.delivery-ingested`, metrics labels and api's `delivery_source`.
enum SourceId { Sftp, Bucket }

/// One file a source has *completely* received.
struct Candidate {
    source: SourceId,
    name: String,                 // file name the routing globs see ("timetable_full.zip")
    delivered_at: DateTime<Utc>,  // SFTP: mtime at upload close; S3: LastModified (PUT completion)
    bytes: u64,
    identity: SourceIdentity,     // what "seen before" means for this source
}
enum SourceIdentity {
    File { mtime: SystemTime },                                           // + bytes
    Object { key: String, e_tag: String, version: Option<String> },       // + bytes, LastModified
}

/// A peer source. Enum dispatch rather than `dyn` (no async-trait
/// dependency; async fn in traits is stable but not object-safe).
enum Source { WatchDir(WatchDirSource), Bucket(ObjectStoreSource) }

impl Source {
    /// Complete, routable candidates now. WatchDir applies today's
    /// stability gate inside; Bucket lists `prefix` (uploads are atomic,
    /// so no gate) and skips identities already in its seen-ledger.
    async fn poll(&mut self) -> anyhow::Result<Vec<Candidate>>;
    /// A local, fully-read copy to work on, plus its SHA-256 and size,
    /// hashed while reading. WatchDir: the file in place (hash only).
    /// Bucket: streamed GET with `if_match` = listed ETag into
    /// `sources/bucket/.partial`, size checked against the listing and
    /// `maxObjectBytes`, fsync, rename to `sources/bucket/<name>`, mtime set
    /// to LastModified (File::set_modified), version id from the response.
    async fn fetch(&mut self, c: &Candidate) -> anyhow::Result<LocalDelivery>;
    /// After a decision: WatchDir moves CORPUS out of the watch dir as
    /// today (the CIF zip stays, as today). Bucket records the identity
    /// as seen and, only with deleteAfterIngest, deletes that version.
    async fn settle(&mut self, c: &Candidate, d: &Decision) -> anyhow::Result<()>;
}
```

`WatchDirSource` is today's `scan.rs` + `StabilityTracker`, unchanged
in behaviour. `ObjectStoreSource` wraps `object_store::aws::AmazonS3`.
It is built with the region, bucket, the static key from env, and an
optional `endpoint` and `allow_http` for MinIO or LocalStack tests. It
keeps a seen-ledger, `sources/bucket/.seen`: one tab-separated line per
handled `(key, version or ETag, LastModified ns, bytes, sha256)`,
bounded to the last 50. After a restart it doesn't re-download what it
already handled, so egress stays at one download per version.

The main loop builds `Vec<Source>` from the enabled sources. Each cycle
it polls every source, so one failing source never blocks the other. It
then hands all candidates to the source-agnostic pipeline.

### One pipeline, deduplicated by content

For each kind (CIF, CORPUS):

1. Order the candidates from every source by `delivered_at`. Within the
   same second, order by `sourcePrecedence`.
2. For the newest candidate not yet settled:
   1. `fetch` it and get the SHA-256.
   2. **Dedup.** Look up the SHA-256 in the *content ledger*: every
      delivery directory's `.delivery-ingested` and the quarantine
      records. If it is already accepted or quarantined, do no
      extraction, check or POST. Log an audit line, `outcome: duplicate`,
      with `source`, `duplicate_of` (the delivery directory) and
      `first_source`. Append the arrival to that directory's
      `.delivery-sources`. Then `settle`.
   3. **Otherwise** run today's path: extract with caps, run the CIF
      checks, mark complete, POST, record. One `accepted` or
      `quarantined` line with `source`.
3. Older unsettled candidates are superseded, as `corpus.rs` already
   does: logged and settled, never ingested over newer data.

Size, SHA-256 and the content checks all stay. Only the stability gate
is skipped for the bucket source, because an S3 object is visible only
after its PUT or CompleteMultipartUpload succeeds. A zero-byte or
truncated object fails the existing zip and CIF checks as it does today.

### Records

- **`.delivery-ingested` v2** adds `source` and `source_ref`. `source_ref`
  is `sftp:<file>` or `s3://<bucket>/<key>#<versionId>`. A v1 (4-field)
  record reads as `source=sftp`.
- **`.delivery-sources`** (new, append-only) lists every arrival of this
  content: `source`, `source_ref`, `delivered_at` and the outcome
  (`accepted` or `duplicate`).
- The **delivery directory name** is still `delivery_dir_name(delivered_at)`.
  A different-content collision in the same second (practically
  impossible) gets a `-<source>` suffix, which `is_delivery_dir_name`
  learns to accept and schedule-reference already sorts correctly.
- **api.** `schedule_feed_ingests.delivery_source` (nullable text) and
  `corpus_deliveries.delivery_source` record the source of the accepted
  copy. Duplicates are not posted, because api's unique
  `delivered_at` + `ON CONFLICT DO NOTHING` would drop them anyway. They
  live in the audit stream.
- The **audit line** gains `source`, plus `duplicate_of` and
  `first_source` on `duplicate`.

### When the two sources disagree

Same content means dedup, ingested once. Different content within
`disagreementWindowMinutes` (default 120) of each other means RDM pushed
two different files to two destinations, which is unexpected:

- `schedule_feed_source_disagreement_total{kind}` is incremented, which
  drives `DistantSignalScheduleFeedSourcesDisagree`. Both SHA-256s,
  sizes and sources are logged.
- **The bucket wins** (D5, decided 2026-10-02). `sourcePrecedence`
  defaults to `[bucket, sftp]` and stays configurable only for tests and
  emergencies. The
  higher-precedence copy is ingested, whichever arrived first. The other
  gets `outcome: superseded` with `reason: "source precedence: <winner>"`
  and is not ingested. The bucket comes first because its writer
  credential is a scoped IAM key, while SFTP's password faces the public
  internet.
- **Every check still applies to the winner.** If it fails a check, it is
  quarantined. The loser is then **not** promoted automatically: an
  operator decides, as with any quarantine today, by lowering a threshold
  or re-pushing. A forged delivery on one channel can't win by causing
  the genuine one to be quarantined.
- Outside the window, the later delivery is simply the newer delivery,
  as with an SFTP re-push today. The existing checks still apply: the
  `Generated` date must not be older than the last accepted delivery,
  and the record-count drop limit still holds.

CORPUS follows the same rules. `corpus_deliveries.sha256` already exists,
and loading is idempotent on the api side.

## 10. Chart: `charts/distant-signal` `scheduleFeed`

**Separate switches, not a `sources:` list.**

- Every component in this chart is switched with its own `enabled`.
- Each source has its own block of settings anyway.
- `--set scheduleFeed.sftp.enabled=false` is a single, obvious override.
- `chart-values-doc.py` documents the keys naturally.

`sourcePrecedence` is the only list, and it orders sources that already
exist.

```yaml
scheduleFeed:
  enabled: false
  sftp:
    enabled: true            # NEW. Default true: existing values keep today's behaviour
    # ... every existing sftp.* key, unchanged
  bucket:
    enabled: false           # NEW, off by default
    provider: s3             # only s3 is implemented; gcs is the documented fallback
    bucket: ""               # required when enabled
    prefix: rdm/
    region: eu-west-2
    endpoint: ""             # empty = AWS's regional endpoint; set for MinIO/LocalStack tests
    existingSecret: ""       # required: sealed reader key (access-key-id / secret-access-key)
    accessKeyIdKey: access-key-id
    secretAccessKeyKey: secret-access-key
    pollIntervalSecs: 300
    maxObjectBytes: 536870912     # same 512 MiB cap as sftp.maxUploadFileSize
    maxDownloadBytesPerDay: 1073741824   # hard stop on a runaway re-download loop
    deleteAfterIngest: false      # needs ds-ingest-bucket reader.allowDelete
    notifications:
      sqs:
        enabled: false
        queueUrl: ""
        reconcileIntervalSecs: 3600   # the hybrid's safety-net LIST
    accessLogs:
      ship: false
      bucket: ""             # e.g. <bucket>-logs
      prefix: s3-access/
      pollIntervalSecs: 600
  sourcePrecedence: [bucket, sftp]
  disagreementWindowMinutes: 120
```

### Rendering rules

- `scheduleFeed.enabled` with neither source enabled → `fail`.
- **SFTP off.** No `sftp` container. No schedulefeed `Service`, so no
  NodePort or LoadBalancer. No `sftp-entrypoint` ConfigMap or
  `checksum/sftp-entrypoint` annotation. No host-key Secret, volume or
  mount, and no `sftp-bootstrap` emptyDir. No 2022 ingress rule. No
  `sftp-metrics` PodMonitor endpoint. No `distant-signal.schedule-sftp`
  alert group. The `authMethod` guard is skipped. `ingest` gets
  `SFTP_SOURCE_ENABLED=false` and does not scan `WATCH_DIR`. The PVC
  stays, because extraction needs it.
- **Bucket off.** No `BUCKET_*` env and no AWS secret reference, so no
  credentials are needed. No `distant-signal.schedule-bucket` alerts.
- **Bucket on.** `BUCKET_SOURCE_ENABLED=true` and the `BUCKET_*` env,
  with `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` from `existingSecret`
  (`fail` if it is empty).
- **NetworkPolicy.** The schedulefeed egress policy already has the
  public-internet rule on 443, for the OAuth token URL, which excludes
  private CIDRs. The S3 regional endpoint is public, so no new rule is
  needed. The chart adds the S3 endpoint to the `urls` list it passes to
  `egressSection`, so a custom `endpoint` port is allowed too. Pinning to
  AWS's published eu-west-2 S3 ranges (`ip-ranges.json`) was rejected:
  they change without notice, and a stale list fails silently. The
  bucket-policy `aws:SourceIp` pin on the reader key protects the other
  direction.
- `scheduleFeed.sftp.enabled` defaults to `true`, so every existing values
  file renders the same until someone changes it. The CI render test
  asserts this byte for byte against `origin/main`'s defaults.

Settings that ship **off by default**: `scheduleFeed.bucket.enabled`,
`bucket.deleteAfterIngest`, `bucket.notifications.sqs.enabled`,
`bucket.accessLogs.ship`, `ds-ingest-bucket` `enabled`,
`reader.allowDelete`, `writer.allowList` and `notifications.sqs.enabled`.

## 11. Security: threat model compared with SFTP

| Concern | SFTP source | Bucket source |
| --- | --- | --- |
| Inbound exposure | NodePort 30450 on the public IP. Scanned and brute-forced. Mitigated by the defender, rate limits, a 190-bit password and login-anomaly rules | **None.** Outbound HTTPS from the ingest pod only |
| Publisher authentication | A shared password. JSch 0.1.54 can't pin our host key | An IAM key (or a cross-account principal) over AWS TLS; no host-key question |
| Publisher authority | upload, overwrite, list in one directory | `PutObject` (+ abort) on one prefix |
| Our authority | Filesystem access to the PVC | List and get on one prefix, optionally pinned to our IP; no delete by default |
| What a stolen publisher credential does | Replace the timetable with a crafted file, which ingest checks reject | Same, and a replaced version survives 3 days for forensics |
| Detection | SFTPGo login and upload log → Loki rules (new source, off hours, failed logins, bans) | Access logs → Loki rules (new writer source, off hours, 403 bursts, unexpected principal); metrics for no-new-object, read and auth errors, and download budget |
| New dependency | None | An AWS account to secure: root MFA, budget alert, key rotation, and ACK controllers holding scoped AWS keys in the cluster |

**Running bucket-only removes the node's only public listener.** That is
the operator's choice (§12, step 7), not a plan step. If chosen, Ranma
updates `public-exposure-check.yml`'s allowed list and the NodePort
monitoring exception. The SFTP Loki rules simply go quiet.

## 12. Adoption plan

Nothing here retires SFTPGo. Each step can be undone by flipping a value.

1. **AWS account and bootstrap** (user and Ranma, OpenTofu, D2/D3).
   - The user creates the **new dedicated account** and holds its root
     MFA. There are no console users beyond one admin.
   - OpenTofu, run once, creates:
     - the boundary policy;
     - the three controller users and their policies;
     - the **$5/month budget alert** (recipient and other account
       specifics live in Ranma-Config, not here; see D6).
   - Seal the controller keys into `ack-system`.
   - *Rollback:* destroy the stack; nothing depends on it yet.
2. **ACK controllers** (Ranma, `clusters/mine-bringer/controllers/6.ack/`).
   - Add a `HelmRepository` (`type: oci`,
     `url: oci://public.ecr.aws/aws-controllers-k8s`).
   - Add HelmReleases `s3-chart`, `iam-chart` and later `sqs-chart`,
     pinned to the versions above, with:
     - `aws.region: eu-west-2`;
     - `aws.credentials.secretName` (the sealed Secret) and `profile`;
     - `installScope: namespace` with `watchNamespace: ds-ingest-aws`;
     - `reconcile.defaultResyncPeriod: 3600`;
     - `deployment.resources` set.
   - Add PSA `restricted` namespace labels; check the charts'
     securityContext against `psa-enforcement.md` first.
   - Add an egress NetworkPolicy allowing 443 to the internet, which is
     the AWS APIs.
   - Add a Flux Kustomization `c06-ack` that `dependsOn` `c00-sealed-secrets`.
   - *Rollback:* remove the Kustomization. CRs with `retain` leave AWS
     untouched.
3. **The bucket** (Ranma, a HelmRelease of `charts/ds-ingest-bucket`).
   - Source: package the chart in DS's `push-helm-chart` job next to
     `distant-signal`.
   - Namespace `ds-ingest-aws`, with values `enabled: true`, the account,
     the bucket name and the boundary ARN. `writer.mode` depends on RDM's
     answer.
   - Wait for `ACK.ResourceSynced=True` on every resource.
   - Mint the reader key, then seal it as
     `distant-signal/distant-signal-schedulefeed-bucket`.
   - Mint the writer key (`iamUser`) for RDM's form.
   - *Rollback:* set `enabled: false`. With `retain` and `keep`, the
     bucket stays.
4. **DS code and chart** (the plan's phases 1–4).
   - Release, then turn on `scheduleFeed.bucket.enabled: true` **with
     SFTP still on**. Until RDM pushes, the bucket source just lists an
     empty prefix, and `...NoNewObject` stays quiet until the first
     object arrives.
   - *Rollback:* `bucket.enabled: false`.
5. **Ask RDM to add the bucket** as a destination for the timetable and
   CORPUS subscriptions, **in addition to SFTP**. Send the questions in
   §14, "Questions for DTD/RDM", ideally before step 3, because the answer
   decides `writer.mode`.

   No credentials go by email. The writer key goes only into their
   portal form.
6. **Verify both**, for at least 7 daily deliveries and one CORPUS:
   - each CIF has exactly one `accepted` and one `duplicate` audit line,
     with the same `sha256` and both `source`s;
   - `schedule_feed_source_disagreement_total` stays at 0;
   - the bucket's `delivered_at` is within minutes of the SFTP one;
   - the access-log tailer shows only the writer's PUTs and our GETs;
   - `...PublishStale` stays quiet.
7. **Steady state: the operator's choice**, reversible at any time:
   - **both** (default): redundancy, so either channel can fail;
   - **bucket-only**: `scheduleFeed.sftp.enabled: false`, and ask RDM to
     drop the SFTP destination. This closes the public port; Ranma
     updates the exposure check;
   - **SFTP-only**: `scheduleFeed.bucket.enabled: false`. AWS resources
     stay until removed by hand.

## 13. Costs (rough, monthly, USD before VAT)

From the artifact, for eu-west-2.

| Item | Typical | Worst realistic |
| --- | --- | --- |
| S3 storage (3–4 versions of a 78 MB zip, CORPUS, logs) | $0.01 | $0.02 |
| Requests (LIST every 5 min; GET per new version) | $0.05 | $0.24 (LIST every minute) |
| Egress to the node (~2.4 GB) | $0 (100 GB free; the dedicated account, D2, has nothing else using it) | $0.32 only if the allowance were used elsewhere |
| Access logs | <$0.01 | <$0.01 |
| SQS (only if enabled) | $0 (free tier) | $0.05 |
| KMS | $0 (SSE-S3) | $1 if `aws:kms` is chosen |
| **Total** | **≈ $0.06** | **≈ $1.30** |
| Runaway bug (re-downloading the zip every minute) | — | ≈ $297. Prevented by the seen-ledger and `maxDownloadBytesPerDay`, and caught by `...DownloadBudget` and the $5 budget |

The ACK controllers cost nothing in AWS. In the cluster they are three
small pods; measure them at install.

## 14. Open questions

**Repo scope (D6, user, 2026-10-02).** This repo defines only the bucket
deployment: the `ds-ingest-bucket` chart, its values contract and the
schedule-ingest side. Everything account- or deploy-specific (the AWS
account itself, the OpenTofu bootstrap stack, the boundary policy and
controller users, the budget and its alert recipient, controller
installs, sealed keys, the concrete values for the chart) belongs to
Ranma-Config and is handed off to the deploy session.

Answered 2026-10-02 and recorded under "Decisions": the account (D2),
ACK vs OpenTofu (D3), the audit trail (D4) and source precedence (D5).

### For the user

1. **Cleanup.** The default is lifecycle expiry only: `reader.allowDelete`
   and `deleteAfterIngest` stay off. This stands unless the user objects
   and wants the bucket emptied on ingest.
2. **Reader IP pin.** Is the node's public egress address stable enough
   to set `reader.allowedSourceCidrs`? Until it is confirmed, the list
   stays empty.
3. **Steady state** after verification (§12, step 7): both (the
   default), bucket-only or SFTP-only.

### Questions for DTD/RDM (ready to send)

Send these through the RDM support channel or the subscription's
destination settings, not to the user. They are about RDM's S3
destination for our timetable (`timetable_full.zip`) and CORPUS
(`CORPUSExtract.json.gz`) subscriptions. Don't send credentials by
email.

> We'd like to add an AWS S3 bucket (region eu-west-2, key prefix `rdm/`)
> as a delivery destination for our timetable and CORPUS file
> subscriptions, alongside our existing SFTP destination.
>
> 1. **Credentials.** Does your S3 destination take an IAM access key
>    pair, or do you write as your own AWS principal (cross-account)? If
>    it's the latter, what is the principal ARN, and do you use an
>    external ID?
> 2. **Permissions.** Besides `s3:PutObject`, does your client need
>    anything else on the bucket, such as `HeadObject`, `ListBucket`,
>    `GetBucketLocation`, `AbortMultipartUpload`, or a write-then-rename
>    (copy and delete)?
> 3. **Object keys.** Can we set the key prefix? Will the files keep the
>    names `timetable_full.zip` and `CORPUSExtract.json.gz`, overwritten
>    on each delivery, or are keys dated or otherwise varied?
> 4. **Headers.** Do you send an ACL header (our bucket only accepts none,
>    or `bucket-owner-full-control`) or a server-side-encryption header
>    (our default is SSE-S3; `AES256` is fine, `aws:kms` isn't)?
> 5. **Uploads.** Do you use multipart upload for the ~78 MB timetable
>    zip? What are your retry behaviour and expected delivery time (today
>    about 20:00 UTC)?
> 6. **Dual delivery.** Can one subscription deliver to both SFTP and S3
>    at the same time, or would we need a second subscription?
> 7. **Testing.** Is there a way to trigger a test delivery once the
>    destination is configured?

## Previously open, now decided (kept for the record)

- Which AWS account owns this → D2: a new dedicated account.
- OpenTofu alone vs ACK in the cluster → D3: ACK, with OpenTofu for the
  one-off setup.
- CloudTrail data events → D4: no, access logs only.
- Source precedence → D5: the bucket wins.

## Sources

- "Cloud Bucket Ingest Costs" artifact (user's, read 2026-10-02): prices,
  volumes, security comparison, open questions.
- Open Rail Data wiki, "Rail Data Marketplace": push to AWS, Azure, GCP
  or SFTP; no pull.
- ACK API reference, S3 `Bucket` and SQS `Queue`; the `iam-controller`
  CRD list and `users` and `policies` CRDs; release tags for the
  s3, iam, sqs and cloudtrail controllers (GitHub, 2026-10-02).
- crossplane/crossplane and crossplane-contrib/provider-upjet-aws release
  tags (2026-10-02).
- `cargo info aws-sdk-sqs` / `aws-config` (rust-version 1.94.1);
  `object_store` 0.14.2's `Cargo.toml` (rust-version 1.85; `aws`, `gcp`
  and `azure` features) and `ObjectMeta`/`GetOptions`.
- This repo: `crates/schedule-ingest`, `charts/distant-signal/templates/schedulefeed-*.yaml`,
  `networkpolicy.yaml`, `prometheusrule.yaml`, `docs/schedule-feed-sftp.md`;
  Ranma-Config `docs/specs/sftp-audit-observability.md` and the
  controllers layout (read-only).
