# Schedule feed: a cloud-bucket delivery source alongside SFTP

Design, 2026-10-02. Status: **proposed**, with the user's decisions D1–D13
recorded the same day (below). Nothing here is deployed.

The bucket is **Google Cloud Storage** (D8). The first draft targeted AWS S3.

This change adds `charts/ds-ingest-bucket`, off by default and installed
nowhere. Everything else in this document is a proposal, and
[the plan](../plans/2026-10-02-schedule-feed-gcs-landing-plan.md) is how to
build it.

## Decisions (user, 2026-10-02)

| # | Decision | Effect on this design |
| --- | --- | --- |
| D1 | ~~AWS S3, eu-west-2.~~ **Superseded by D8** the same day | — |
| D2 | **A new, dedicated cloud project** owns the bucket and everything in it, with a **$5 a month** budget alert. Under D8 this is a dedicated GCP project | §7, §12 |
| D3 | **Provisioning as Helm.** Credentials are created **once by hand** and sealed in Ranma-Config. **OpenTofu only for the one-off setup** (project baseline, budget, controller credential, kill switch). Under D8 the in-cluster controller is Crossplane v2 with the GCP providers (the recommendation in §4) | §4, §7 |
| D4 | **Audit: the storage service's own object-level logs only**, no extra paid trail. Under D8: Cloud Audit Logs Data Access for Cloud Storage | §8 |
| D5 | **Precedence: the bucket wins** when SFTP and the bucket deliver different content within the disagreement window. The disagreement alert still fires | §9. `sourcePrecedence: [bucket, sftp]` is the fixed default |
| D6 | **Both sources are kept long-term.** Nothing is retired | §10, §12 |
| D7 | **Repo scope.** This repo holds only generic framing and templates: the chart, its values contract and the schedule-ingest side. Everything project- or deploy-specific (project id and number, org, billing, budget amounts and recipients, every service-account email, bucket names, the node's address, controller installs, sealed keys, the kill switch) is Ranma-Config's. Examples use obvious placeholders. (Recorded as "D6" in the first hand-off note; renumbered because D6 was taken) | Whole document |
| D8 | **Google Cloud Storage, europe-west2**, replacing AWS S3. RDM's GCS destination needs **no credential from us**: we grant DTD's own service accounts access to the bucket, and revoking is removing a binding | §3 has the comparison |
| D9 | **The bucket is dedicated** to timetable/CORPUS ingest and similar data. Nothing else ever goes in it. **Usage limits** cap the blast radius | §5, "Usage limits" |
| D10 | **Publisher access is exactly what DTD specifies**: a list of DTD's service accounts, each with DTD's four roles, bound on the bucket only | §5, "Publisher access" |
| D11 | **The bucket holds data only in transit.** schedule-ingest fetches only expected names, verifies each download, archives it locally, then **deletes it from the bucket** (conditional on generation). Unexpected objects are flagged and deleted unread. Versioning off, soft delete at the 7-day minimum, one lifecycle backstop | §5, "An object's life"; §9 |
| D12 | **Crossplane v2** (provider-upjet-gcp) provisions the bucket from Helm, not Config Connector | §4 |
| D13 | **A kill-switch trip pauses reconciliation of the affected bindings.** The Ranma-side kill switch sets `crossplane.io/paused: "true"` on the labelled `BucketIAMMember` resources before removing the bindings; Ranma's reapply unpauses them | §5, "The kill switch and Crossplane" |

Also recorded: Ranma-Config's **kill switch** (user-approved): budget and
Cloud Monitoring alerts → Pub/Sub → a function that removes bucket
bindings, never billing (§5, "Usage limits").

Still open: §14.

## Summary

The Rail Data Marketplace (RDM, "DTD" in older docs) can push file feeds to
SFTP, or to a customer bucket in AWS, Google Cloud or Azure. It cannot push
to our own S3-compatible store (Thoth). Today Distant Signal takes the CIF
timetable and the CORPUS extract over SFTP:

- an SFTPGo container on NodePort 30450, the only listener on the node's
  public IP;
- schedule-ingest, which reads the shared PVC.

This design adds a **Google Cloud Storage bucket in europe-west2
(London)** as a second, equal delivery source. Each source is switched on
separately: SFTP only, bucket only, or both at once.

- **Why GCS** (D8). DTD's GCS destination works by us granting *their*
  service accounts roles on our bucket. No secret is created or handed
  over, and revoking access is removing one binding. AWS would need an
  access key we mint and give to DTD; Azure an account key or SAS (§3).
- **Bucket.** Dedicated (D9), uniform bucket-level access, public access
  prevention enforced, Google-managed encryption, unversioned, 7-day soft
  delete, a 7-day lifecycle backstop. Cloud Audit Logs (Data Access) go
  to a second private bucket.
  - The publisher: DTD's service accounts with DTD's four roles, on the
    bucket only (D10). Effectively create, overwrite, delete, read and
    list objects; no IAM or bucket-setting changes.
  - schedule-ingest: our own service account with get, list and delete
    on the bucket only. It can't create or overwrite.
- **An object's life** (D11). DTD writes at the bucket root. Within
  minutes schedule-ingest lists the root, downloads only expected names,
  verifies size and CRC32C, archives locally, and deletes the object
  `ifGenerationMatch`. Unexpected objects are alerted on and deleted
  unread.
- **Provisioning.** `charts/ds-ingest-bucket` renders Crossplane v2
  managed resources (provider-upjet-gcp). Crossplane and its providers
  are installed by Ranma-Config. OpenTofu does the one-off project
  bootstrap (§4, §7).
- **How DS learns of new objects.** Polling the bucket every 5 minutes.
  Pub/Sub notifications to a pull subscription were evaluated (§6); they
  are templated but off.
- **schedule-ingest.** Sources become peers behind one abstraction. One
  content-addressed pipeline ingests each delivery once, whichever
  source it came through, deduplicated by SHA-256.
- **Usage limits.** No native GCS quota exists. The chart has optional
  Cloud Monitoring alerts; Ranma's kill switch removes bindings
  automatically; schedule-ingest has its own size, download and
  loop guards.
- **Adoption.** Stand up the bucket, turn the bucket source on alongside
  SFTP, ask RDM to add the bucket as a second destination, verify both.
  Running bucket-only is an operator choice. Nothing is retired.
- **Cost.** About $0.35 a month typical. The realistic expensive failure
  is a looping reader (hundreds of dollars a month), which the reader's
  guards, the alerts and the kill switch each stop.

## 1. What RDM supports, and what is still unknown

From the "Cloud Bucket Ingest Costs" artifact, DTD's destination
instructions (received 2026-10-02), the Open Rail Data wiki and the
2026-10-01 SFTP evidence in
[schedule-feed-sftp.md](../../schedule-feed-sftp.md).

### Confirmed

| Fact | Value | Source |
| --- | --- | --- |
| Push destinations | AWS S3, Azure Blob, Google Cloud Storage, or SFTP. No pull option. Thoth is not a target | User; Open Rail Data wiki |
| Publisher location | Google Cloud, europe-west2; the SFTP pushes come from one GCP address | SFTP `login` lines |
| GCS credential model | **None from us.** "Grant the following service accounts access to your Cloud Storage bucket": two of DTD's service accounts, a malware scanner's in their production project and their project's Storage Transfer Service agent | DTD's instructions |
| GCS permissions | Each service account gets `roles/storage.objectViewer`, `roles/storage.legacyBucketReader`, `roles/storage.bucketViewer` and `roles/storage.legacyBucketWriter` on the bucket | DTD's instructions |
| Transfer mechanism | Google's Storage Transfer Service, run from DTD's project, writes into our bucket; the scanner reads objects | DTD's instructions (the STS agent among the principals) |
| CIF file | `timetable_full.zip`, 77.2–77.9 MB, extracting to a ~724 MB MCA file. Deflate, about 12 entries | SFTP `Upload` lines; ingest |
| CIF cadence | Daily, about 20:00 UTC | SFTP log; PVC mtimes |
| CORPUS | `CORPUSExtract.json.gz`, ~788 KB, about monthly. Also `CORPUSExtract.csv.gz` (SMART, ~296 KB, ignored) | Artifact; `config.rs` |
| Upload behaviour (SFTP) | One session per delivery, straight to the final name, overwriting in place | schedule-feed-sftp.md |
| Monthly volume | ~2.36 GB and ~33 writes | Artifact |

### Not confirmed

| Unknown | Why it matters | How the design copes |
| --- | --- | --- |
| **Object naming.** Root or a prefix? The same names overwritten (as on SFTP), or dated names? | The reader's allowlist; whether DTD overwrites an object we haven't fetched yet | The reader lists the root and matches names against `expectedKeys` (the same globs as SFTP). Anything else is unexpected (§5) |
| **Scanner behaviour.** Does the malware scanner only read, or does it delete or quarantine in place? | It holds delete; a quarantine-by-delete looks like an unexplained delete | The reader tolerates a vanished object (404). The `DeleteObject` usage alert and the audit rule name the principal |
| **Read-back after write.** How long after the write do STS and the scanner read the object? | We delete after fetching | The reader waits `deleteMinAgeSecs` (default 1 h) after an object's creation before deleting it |
| **Probe objects.** Does the destination check write a test object when it is set up? | It would be "unexpected" | Flagged and deleted after `deleteMinAgeSecs`, which lets their check finish |
| **Dual delivery.** Can one subscription push to SFTP and GCS together, or are two needed? | Running both sources at once | Ask before the parallel phase (§12) |
| **A test delivery** once the destination is set | Verification | Ask |

## 2. Today's path, and what stays the same

The schedulefeed Deployment (`charts/distant-signal/templates/schedulefeed-*.yaml`)
runs three containers that share a PVC:

- **`sftp`**: SFTPGo v2.7.5. `dtd-push` has upload, overwrite and list on
  one directory. It has the defender, rate limits, JSON audit log,
  telemetry and the `distant-signal.schedule-sftp` alerts. Ranma's Loki
  rules add new-source and off-hours login alerts.
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

All of this stays. The bucket source changes only how a candidate file
reaches the pipeline, and what "delivered at" and "already seen" mean for
it.

## 3. Options and recommendation

### Cloud

DTD's generic connector has the same shape everywhere: write, then read
back and list to verify. What differs is the credential.

| | **GCS europe-west2** | AWS S3 eu-west-2 | Azure Blob UK South |
| --- | --- | --- | --- |
| What DTD needs from us | **Nothing secret**: bindings for their service accounts | An **access key** for an IAM user we create, entered in RDM's form | A storage-account **access key** and/or a **SAS** |
| Revoking | Remove a binding; effective within minutes (IAM propagation), needs no one else | Deactivate the key; deliveries stop until DTD enters a new one | Rotate the account key (breaks every SAS not tied to a stored access policy) |
| Grant breadth | Create, overwrite, delete, read, list on one bucket | List, get, put on one bucket, plus `ListAllMyBuckets` | Account key: everything in the account |
| Publisher already in that cloud | Yes (GCP europe-west2): writes stay in-region | No | No |
| Typical / worst monthly (artifact) | $0.37 / $1.10 | $0.06 / $1.30 | $0.07 / $0.29 |
| Reader credential | Our service account's key, sealed (§7) | IAM user key, pinnable to an IP by bucket policy | SAS or account key |
| Reader IP pin | Not per principal (§5) | Yes (`aws:SourceIp`) | SAS IP range |
| Already in DS | `object_store` `gcp` feature (same dependency tree as `aws`) | `object_store` `aws` | No |

**Decision (D8): GCS in europe-west2.** It is the only option where we
never create or hand over a secret. The AWS draft of this design ended up
needing exactly that: DTD's S3 form asks for an access key ID and secret,
so we would mint a key for an IAM user in our account, send it through
RDM's form, keep no copy, and rotate it with DTD's cooperation. A leak in
RDM's systems would be outside our control. GCS costs about $0.30 a month
more, mostly egress to our node, and loses the reader's IP pin.

**Azure** would be worse still. A storage-account key is effectively root
over the whole account, with no scoping. A SAS can be scoped (container,
permissions, expiry, HTTPS-only, an IP range), but an ad-hoc SAS can be
revoked only by rotating the account key unless it is tied to a stored
access policy, and a user-delegation SAS lasts at most 7 days, impractical
for a long-lived feed. If Azure were ever needed: a dedicated storage
account and a container-scoped, read/write/list, HTTPS-only SAS bound to
a stored access policy, never the account key.

### SFTP alongside

Both sources are first-class and switched on separately
(`scheduleFeed.sftp.enabled`, `scheduleFeed.bucket.enabled`), long-term
(D6). §12 is an adoption plan, not a migration.

## 4. Provisioning: Helm via Crossplane (recommended) vs Config Connector vs OpenTofu

Helm can't call GCP. A controller that turns custom resources into GCP API
calls can, and Helm renders those resources.

| | **Crossplane v2** + provider-upjet-gcp | **Config Connector** (KCC) |
| --- | --- | --- |
| Versions (2026-10-02) | Crossplane v2.4.2; provider-upjet-gcp v3.0.0 (`provider-gcp-storage`, `-cloudplatform`, `-monitoring`, `-pubsub`) | v1.157.0 |
| Non-GKE support | Any cluster | Supported through the manual "other Kubernetes distributions" install with a service-account key Secret; Google's docs warn that importing a key into a cluster is "generally considered insecure" and the install is otherwise GKE-focused |
| Footprint on the single-node k3s | Core, RBAC manager, and one pod per provider family member actually installed. v2's managed-resource activation policies activate only the CRDs used (here about eight kinds) | One controller manager, but it installs CRDs for every supported GCP service (hundreds), each held by the API server on a node whose memory and disk are shared with every PVC |
| Kinds used | `storage.gcp.m.upbound.io` `Bucket`, `BucketIAMMember`, `Notification`; `cloudplatform.gcp.m.upbound.io` `ProjectIAMCustomRole`; `monitoring.gcp.m.upbound.io` `AlertPolicy`; `pubsub.gcp.m.upbound.io` `Topic`, `Subscription`, `TopicIAMMember`, `SubscriptionIAMMember` (all namespaced, v1beta1; field names checked against the v3.0.0 CRDs) | `StorageBucket`, `IAMPolicyMember`, `IAMCustomRole`, `MonitoringAlertPolicy`, `PubSubTopic`, … |
| Controller credential | A `ClusterProviderConfig` pointing at a sealed service-account key | A sealed key Secret in `cnrm-system` |
| Drift | Provider poll, about 10 minutes by default | Reconcile about every 10 minutes by default |
| Delete safety | Management policies without `Delete` (orphan) | `cnrm.cloud.google.com/deletion-policy: abandon` |
| Maturity | CNCF graduated; the GCP providers are crossplane-contrib (formerly Upbound's official ones) | Google product |

**Recommendation: Crossplane v2.** Footprint is decisive on this node, and
Config Connector off GKE is a second-class path whose own docs discourage
the credential model it requires. Both need one sealed controller key, so
neither wins on bootstrap.

**Keys are never minted by the controller.** provider-gcp-cloudplatform has
a `ServiceAccountKey` kind, but it writes the private key into a
connection Secret, unsealed, in the cluster. The reader's key is created
by hand and sealed (§7).

**OpenTofu** does the one-off bootstrap in Ranma-Config: the project, its
budget, the controller's service account and key, the reader's service
account, the Data Access audit config and sink, and the kill switch. Every
resource in §5 would also map one-to-one onto `google_storage_*` and
`google_*_iam_member` if Helm were ever dropped.

### Chart placement

The chart is separate (`charts/ds-ingest-bucket`), not part of
`charts/distant-signal`, because:

- it needs the Crossplane CRDs, and `charts/distant-signal` must keep
  installing without them;
- its resources are retained on uninstall and rarely change, while the app
  chart rolls with every image;
- it installs into its own namespace, which the providers serve;
- only Ranma's Flux applies it.

## 5. The bucket and its access (`charts/ds-ingest-bucket`)

Committed with this spec, off by default. Its README lists the templates
and its values file documents every key. The templates are the source of
truth; this section explains them.

### Delivery bucket (`templates/bucket.yaml`)

| Setting | Value | Why |
| --- | --- | --- |
| Name | `bucket.name` (required; no dots) | Placeholder in examples; the real name is Ranma's |
| Location | `EUROPE-WEST2` | The publisher runs there; closest to the node |
| Uniform bucket-level access | On | IAM only, no object ACLs. DTD's legacy roles are IAM roles and work with it (below) |
| Public access prevention | `enforced` | `allUsers`/`allAuthenticatedUsers` can never be granted, whatever IAM says |
| Encryption | Google-managed (recommended); CMEK available (`bucket.encryption.defaultKmsKeyName`) | CMEK adds a KMS key, a grant to the Cloud Storage service agent and a monthly key cost, for public timetable data. Writers need no KMS permission either way |
| Versioning | **Off** | The bucket holds data in transit only (D11). Noncurrent generations would only retain bytes we deleted on purpose |
| Soft delete | **7 days** (`bucket.softDeleteRetentionDays`; 0 or 7–90) | Every deleted or overwritten object stays restorable for a week, and no object-level role, the publisher's included, can purge it early. It costs about a cent a month here. It keeps an *unexpected* object, which we delete unread, available for examination, and undoes an overwrite or delete by the publisher before we fetched |
| Lifecycle | Delete any object older than **7 days** (backstop); abort unfinished XML multipart uploads after **1 day** | The reader deletes within an hour or so; the backstop matters only during a reader outage, and 7 days is well past the reader-outage alerts (§8). Unfinished JSON-API resumable uploads expire on their own after a week |
| Object retention / retention policy | Off / none | So the publisher's `storage.objects.setRetention` (in `legacyBucketWriter`) can't lock objects against deletion |
| Usage logs | None | Cloud Storage usage logs have no caller identity; Data Access audit logs do (§8) |
| Labels | `managed-by: ds-ingest-bucket`, `purpose: schedule-feed-ingest`, plus `labels` | Billing and audit filters |
| Deletion | Management policies without `Delete`, plus `helm.sh/resource-policy: keep` | An uninstall never destroys the bucket |

### Publisher access (D10)

`publisher.members` (required; the render fails closed when it is empty)
lists DTD's principals, each `serviceAccount:<email>`. The concrete emails
are Ranma's; examples use `serviceAccount:publisher@example-publisher.iam.gserviceaccount.com`.
Each member gets every role in `publisher.roles`, which defaults to DTD's
four, as a non-authoritative `BucketIAMMember` **on the bucket only**. The
render refuses any other member type and any role that can change IAM or
bucket settings (`storage.admin`, `objectAdmin`, `legacyBucketOwner`,
`legacyObjectOwner`).

What the four roles contain (Google's role reference, read 2026-10-02):

| Role | Permissions |
| --- | --- |
| `roles/storage.objectViewer` | `storage.objects.get`, `storage.objects.list`, `storage.folders.get/list`, `storage.managedFolders.get/list`, `resourcemanager.projects.get/list` |
| `roles/storage.legacyBucketReader` | `storage.buckets.get`, `storage.objects.list`, `storage.folders.get/list`, `storage.managedFolders.get/list`, `storage.multipartUploads.list` |
| `roles/storage.bucketViewer` (beta) | `storage.buckets.get`, `storage.buckets.list` |
| `roles/storage.legacyBucketWriter` | `storage.buckets.get`, `storage.objects.create`, `storage.objects.createContext`, `storage.objects.delete`, `storage.objects.list`, `storage.objects.restore`, `storage.objects.setRetention`, `storage.multipartUploads.*`, `storage.folders.*`, `storage.managedFolders.create/delete/get/list/update` |

So, granted on this bucket, DTD's accounts can **create, overwrite, delete,
read and list objects**, read bucket metadata, and restore soft-deleted
objects. They can't change IAM (`setIamPolicy` isn't in any of them),
bucket settings (`storage.buckets.update` isn't either), soft delete or
lifecycle. Notes:

- Overwriting an existing object needs `storage.objects.delete` as well
  as `create` ("In order to replace existing objects, both
  `storage.objects.create` and `storage.objects.delete` permissions are
  required"), which is why `legacyBucketWriter` is needed at all if DTD
  overwrites daily.
- Project-level permissions in these roles (`resourcemanager.projects.*`,
  `storage.buckets.list`) do nothing when the role is granted on a bucket.
  `bucketViewer`'s bucket listing is harmless anyway in a dedicated
  project.
- Legacy bucket roles are ordinary IAM roles grantable only on individual
  buckets, and work with uniform bucket-level access on. UBLA turns off
  object ACLs, not these roles.
- `setRetention` does nothing because object retention is off.
- Folder and managed-folder permissions are inert: no hierarchical
  namespace, and creating a managed folder grants nothing without
  `setIamPolicy`.

This is broad, and acceptable because the bucket is dedicated (D9), holds
data only in transit (D11), is soft-deleted for a week, and every write
lands in the audit log and the usage alerts. A smaller custom role
(create, get, list, delete, `buckets.get`) would work for most clients, but
STS and the scanner are DTD's code; we grant what DTD specifies and say
so (§14 keeps the scanner's behaviour as a question).

The publisher can't be pinned by IP: DTD gives no addresses, and STS runs
on Google's network.

### Reader access

`reader.member` is our own service account in our project (created by
Ranma, required here). On the delivery bucket it gets:

- `roles/storage.objectViewer`: get and list;
- the custom role `reader.deleteRole.roleId` (default
  `dsIngestObjectDeleter`), a `ProjectIAMCustomRole` holding **only**
  `storage.objects.delete`, also bound on this bucket only.

On the audit-log bucket it gets `objectViewer`. It can never create or
overwrite an object.

Why not `roles/storage.objectUser`, which also covers get, list and
delete? It includes `storage.objects.create`, `update`, `move` and
`restore`. A leaked reader key could then **plant** a delivery, which is
the one thing the publisher's key and our ingest checks exist to guard
against. The custom role costs one more resource.

**No IP pin for the reader.** IAM Conditions offer no source-IP attribute
for Cloud Storage. VPC Service Controls can restrict by IP through access
levels, but need an organization and a perimeter that DTD's cross-org
service accounts would have to be let into. Bucket IP filtering exists
but is **bucket-wide**: a filter allowing only our node would also block
DTD's scanner (an ordinary service account calling from addresses we
don't know). Google service agents keep access under a filter, which may
or may not cover STS; not relied on. So the reader key is protected by
sealing, rotation and the audit rule "reader used from an address other
than the node" (§8), which detects but doesn't prevent.

**Workload identity federation** from k3s was evaluated. GCP can trust the
cluster's service-account token issuer if its OIDC discovery document and
JWKS are publicly reachable, and the pod would exchange its projected
token through STS for a short-lived token, so no long-lived key. Against
it: `object_store` 0.14.2's GCP credentials support service-account keys,
`authorized_user` and the metadata server, not `external_account`, so we
would write the token exchange behind `with_credentials` (about 150
lines). And the issuer's discovery endpoint would have to be published
through the tunnel or a public bucket. Deferred; the default is a
hand-made key (§7).

### An object's life (D11)

1. DTD (STS) writes `timetable_full.zip` or `CORPUSExtract.json.gz` at
   the bucket root. The scanner reads it.
2. schedule-ingest lists the root (`delimiter=/`, so nested names are
   never seen) every `pollIntervalSecs`.
3. For each listed object, by name:
   - **Expected** (matches `expectedKeys`, the same globs as SFTP) and
     **not yet confirmed** (name + generation not in the seen-ledger):
     refuse before download if the listed size is over `maxObjectBytes`
     (treated as unexpected); otherwise download **that generation**,
     hashing SHA-256 and CRC32C while writing to local storage; check size
     and CRC32C (and MD5 when GCS has one) against the object's metadata;
     fsync; rename into the local archive; record name + generation +
     SHA-256 in the ledger; hand it to the pipeline (§9).
   - **Unexpected**: never downloaded. One log line with the name
     (escaped and truncated; it is attacker-controlled), size and
     generation, no content; `schedule_feed_source_unexpected_objects_total`
     and the `...UnexpectedObject` alert.
4. Once an object is at least `deleteMinAgeSecs` old (default 1 h, so
   DTD's read-back and scan finish) and either confirmed or unexpected,
   schedule-ingest **deletes it with `ifGenerationMatch`** set to the
   generation it saw. A newer upload under the same name in between makes
   the delete fail with 412, so it is never lost; the next poll picks it
   up. A 404 means it is already gone, which is fine.
5. Archival is local: the raw object stays under
   `storage_dir/sources/bucket/archive/` for `archiveKeep` deliveries, as
   well as the extracted delivery directory.

The ingest outcome doesn't gate the delete: a quarantined CIF is kept
locally, as today. A failed verification (size or CRC mismatch) keeps the
object in the bucket and retries on later polls, within the loop guards.

Residual risk: DTD overwrites or deletes an object **before** we fetch it.
Soft delete keeps the prior copy for a week, the SFTP path delivers the
same content, and dedup by SHA-256 ingests it once.

### Audit-log bucket (`templates/audit-bucket.yaml`)

The destination of Ranma's Logging sink for this bucket's Cloud Audit
Logs. Same privacy settings, no versioning, 7-day soft delete, objects
expire after 400 days (the SFTP audit retention). Only the sink's writer
identity (`auditLogs.sinkWriterIdentity`, from Ranma) gets
`objectCreator`; the reader gets `objectViewer`.

### Usage limits (D9)

GCS has no native quota on a bucket's size, request rate or egress, and
IAM can't cap the size of an upload (objects may be up to 5 TiB). The
controls are alerts, an automatic kill switch, short retention and
schedule-ingest's own guards. Who owns what:

| Control | Owner | What |
| --- | --- | --- |
| Usage alert policies | This chart, `usageAlerts` (off; needs `provider-gcp-monitoring` and Ranma's notification channels) | Hourly: total bytes, object count, write requests, delete requests, received bytes, sent bytes, all on the free `storage.googleapis.com` metrics for this bucket |
| Kill switch | Ranma-Config (OpenTofu), user-approved | A dedicated project budget plus Cloud Monitoring alerts on received bytes, write requests and **sent** bytes → Pub/Sub → a function holding only `getIamPolicy`/`setIamPolicy` on this one bucket. It removes bucket bindings and never disables billing: an ingress or write spike removes the publisher members; an egress spike removes our reader; a budget trip removes all of them. Re-enabling is a deliberate reapply of the deploy values. Amounts and identities are Ranma's |
| Project budget | Ranma-Config | Alert, and a kill-switch trigger. A budget alone never caps spending |
| Retention | This chart | 7-day lifecycle backstop, 7-day soft delete, 1-day multipart abort |
| Reader guards | schedule-ingest (§9) | Expected-name allowlist; size cap before download; never re-download a confirmed generation; per-poll, per-hour and per-day download caps; backoff on errors |

**The kill switch and Crossplane.** The bindings are Helm-owned Crossplane
resources. If the function removes a binding, Crossplane re-creates it at
the provider's next poll (about 10 minutes by default), which would turn
a kill into a throttle. So the trip must also stop reconciliation of the
affected bindings (D13): a small Ranma-side watcher pulls the
kill-switch topic (outbound only) and sets `crossplane.io/paused: "true"`
on the bucket's `BucketIAMMember` resources selected by the chart's label
`ds-ingest-bucket/kill-switch-group: publisher` or `reader`. The chart
never sets `crossplane.io/paused`, so a Helm upgrade leaves a pause in
place; recovery (Ranma's reapply) removes the annotation. For the reader
alone, disabling our reader service account (Ranma-owned, not reconciled
by this chart) is an extra lever Ranma may add; how the watcher is built
is Ranma's call.

### Optional Pub/Sub notifications (`templates/pubsub.yaml`, `notifications.pubsub.enabled: false`)

It renders a topic that only the
project's Cloud Storage service agent may publish to, a pull subscription
(never expires) that only the reader may consume, and an `OBJECT_FINALIZE`
notification on the bucket. No dead-letter topic: messages are hints for a
LIST reconcile (§6).

### Rendering and tests

```sh
helm lint --strict charts/ds-ingest-bucket                     # renders nothing (enabled: false)
helm lint --strict charts/ds-ingest-bucket -f charts/ds-ingest-bucket/ci/example-values.yaml
helm template t charts/ds-ingest-bucket -f charts/ds-ingest-bucket/ci/example-values.yaml
uv run scripts/check-ingest-bucket-chart.py                    # CI scripts-lint job
```

`check-ingest-bucket-chart.py` checks UBLA, enforced PAP, versioning off,
7-day soft delete, the two lifecycle rules, no retention, orphan-on-delete
and `keep` on both buckets; that every grant is a bucket-level (or
topic/subscription) member, fully managed, a service account, and never
public; the exact publisher (members × four roles) and reader bindings;
the delete-only custom role; alerts and Pub/Sub only when enabled; and
that bad values (no members, `allUsers`, `domain:`, admin roles, soft
delete out of range, …) refuse to render.

## 6. How DS learns of a new object

The cluster has no inbound path except the Cloudflare tunnel, so only
**pull** consumers are viable.

| Option | Latency | Reliability | Cost | Extra GCP and IAM | Rust at MSRV 1.88 |
| --- | --- | --- | --- | --- | --- |
| **A. Poll** the bucket root every 5 min | ≤5 min | Self-healing: a missed cycle is caught by the next | ~8,600 Class A LISTs a month, about $0.04 | None | `object_store` 0.14.2 (`gcp` feature, MSRV 1.85; its dependencies are the ones `aws` already brings): `list_with_delimiter`, `get_opts` with `version` = generation |
| **B. Pub/Sub** `OBJECT_FINALIZE` → topic → **pull** subscription | Seconds | At-least-once, unordered; needs ack-deadline extension during extraction | Inside Pub/Sub's free tier | Topic with the Cloud Storage service agent as publisher; reader as subscriber | REST `subscriptions.pull`, `acknowledge`, `modifyAckDeadline` over the existing `reqwest`, with the bearer token from `GoogleCloudStorage::credentials()` (public API, `cloud-platform` scope). No new crates, no hand-written signing |
| **C. Hybrid**: B for latency plus A hourly as the safety net | Seconds, or ≤1 h | Messages are hints; the LIST is the truth | As B | As B | As B |
| **D. Push** subscription to an endpoint behind the tunnel | Seconds | Needs the endpoint up | Free tier | Topic, push subscription, OIDC-token verification | Re-opens an **inbound** path, which the bucket source exists partly to remove. Rejected |

**Recommendation: A, polling.** schedule-reference publishes every 30
minutes and the CIF lands once a day, so seconds buy nothing; polling has
no failure mode that loses a delivery and needs no extra resources. If
latency ever matters, switch to C with both charts' Pub/Sub switches.

## 7. Credentials and their handling

| Credential | Holder | Created by | Sealed as | Rotation |
| --- | --- | --- | --- | --- |
| Crossplane provider's service-account key | `crossplane-system` | Ranma's OpenTofu bootstrap, once | A SealedSecret referenced by the `ClusterProviderConfig` | 180 days |
| Reader service-account key (JSON) | schedule-ingest | A person, once: `gcloud iam service-accounts keys create` piped into `kubeseal`, never written to disk | `distant-signal-schedulefeed-bucket` (key `service-account.json`) | 90 days, two-key overlap: create, reseal, roll, confirm a poll, delete the old key |
| DTD's | DTD | — | Nothing: **we hold no publisher credential** | Revoking = removing a member |

An HMAC key for the reader (S3-compatible XML API through `object_store`'s
`aws` client) was considered and rejected: it is still a long-lived
secret, and the JSON API calls the reader needs (generation-conditional
delete, CRC32C metadata) use OAuth tokens anyway.

The controller's service account needs, on the dedicated project, a
custom role with `storage.buckets.create/get/update/delete/getIamPolicy/setIamPolicy`,
`iam.roles.create/get/update/delete/undelete` (for the delete-only role)
and, when enabled, `monitoring.alertPolicies.*` and the Pub/Sub topic and
subscription permissions. Bucket `setIamPolicy` on the project lets a
stolen controller key grant anything on any bucket in it; the project is
dedicated, so that is the whole blast radius.

### Org policies

A dedicated project with **no organization** has no org policies, and
nothing below applies. If the project sits in an organization:

- **Domain-restricted sharing** (`iam.allowedPolicyMemberDomains`) blocks
  granting DTD's service accounts, which belong to DTD's organization.
  Allow DTD's customer ID for this project, or exempt it.
- **`iam.disableServiceAccountKeyCreation`** blocks the reader's key.
  Override it for this project only.
- `storage.uniformBucketLevelAccess` and `storage.publicAccessPrevention`
  match this design and can stay enforced.

Organizations created since May 2024 enforce several of these by default.
Check the effective policies on the project before step 3 of §12.

## 8. Audit and alerting for the bucket source

The SFTP audit stays exactly as it is while SFTP is enabled.

### Audit trail (D4)

Cloud Audit Logs for Cloud Storage: **Admin Activity** (IAM and bucket
changes) is always on and free. **Data Access** (`DATA_READ`,
`DATA_WRITE`) must be enabled explicitly, for `storage.googleapis.com` in
the project's audit config. At a few dozen operations a day it stays far
inside Cloud Logging's free ingestion allowance. Each entry has the
principal (`authenticationInfo.principalEmail`), `requestMetadata.callerIp`
and user agent, `methodName` (`storage.objects.create`, `.get`, `.delete`,
…), `resourceName` and status. This is the equivalent of S3 server access
logs, with the caller's identity included.

### Shipping to Loki: the simplest pull path

Ranma's Logging sink (filter: `resource.type="gcs_bucket"` and this
bucket's name, `cloudaudit.googleapis.com` logs) writes hourly JSON files
into the audit-log bucket. schedule-ingest's optional audit tailer
(`scheduleFeed.bucket.auditLogs.ship`) lists that bucket after a persisted
cursor, reads new files with the same reader key, and writes one JSON line
per entry (`target=schedule_ingest::bucket_access`: `principal`,
`caller_ip`, `method`, `object`, `status`, `time`). Alloy tags them
`audit="bucket-delivery"`, kept 400 days.

Rejected: a sink to Pub/Sub (another subscription to pull) and polling
the Logging API (another client and quota). The bucket path reuses the
reader's client and key.

### Loki rules (Ranma `loki-rules`, group `distant-signal-bucket-audit`)

| Alert | Fires when |
| --- | --- |
| `DistantSignalBucketUnexpectedPrincipal` | Any `storage.objects.*` call by a principal that is neither a publisher member nor the reader. Keyed on principal, which is the strongest signal: STS runs on Google's network, so its `callerIp` says little |
| `DistantSignalBucketPublisherDelete` | A `storage.objects.delete` by a publisher principal (the scanner question in §1 decides whether this is noise) |
| `DistantSignalBucketReaderNewSource` | A reader call from an address other than the node's (the detection half of the missing IP pin) |
| `DistantSignalBucketPublisherOffHours` | A publisher write outside the observed delivery window |
| `DistantSignalBucketAccessDenied` | Five or more `PERMISSION_DENIED` entries in 15 minutes |
| `DistantSignalBucketIamChanged` | Any Admin Activity `SetIamPolicy` on the bucket: a deploy, or the kill switch |

### Prometheus (new schedule-ingest metrics, labelled `source`)

Group `distant-signal.schedule-bucket`, rendered only with
`scheduleFeed.bucket.enabled`.

| Alert | Expression (sketch) | Severity |
| --- | --- | --- |
| `DistantSignalScheduleBucketAccessRevoked` | `schedule_feed_source_access_revoked{source="bucket"} == 1` for 10m | critical. "Bucket access revoked": the kill switch tripped, or the key was rotated or disabled. SFTP carries on |
| `DistantSignalScheduleBucketNoNewObject` | `time() - schedule_feed_source_last_new_object_seconds{source="bucket"} > 30h` (only once one has been seen) | warning |
| `DistantSignalScheduleBucketReadErrors` | `increase(schedule_feed_source_errors_total{source="bucket",kind!="auth"}[1h]) > 0` for 15m (`list`, `get`, `verify`, `delete`) | warning |
| `DistantSignalScheduleBucketUnexpectedObject` | `increase(schedule_feed_source_unexpected_objects_total[1h]) > 0` | warning |
| `DistantSignalScheduleBucketDownloadBudget` | `schedule_feed_source_download_capped{source="bucket"} == 1` (a per-hour or per-day cap hit) | critical: a loop or an attack, stopped by the reader itself |
| `DistantSignalScheduleFeedSourcesDisagree` | `increase(schedule_feed_source_disagreement_total[1d]) > 0`, only with both sources on | warning |

`DistantSignalScheduleReferencePublishStale` remains the authoritative "no
timetable" signal.

### A kill-switch trip, seen from DS (runbook material)

| Trip | What DS sees | What to do |
| --- | --- | --- |
| Egress spike → reader removed | `...AccessRevoked` within minutes; the bucket source backs off and logs once per state change; SFTP ingest continues | Find the cause in the audit log (a reader loop shows as repeated `objects.get` of one generation). Fix it, then ask Ranma to reapply. Nothing in DS restarts it |
| Ingress or write spike → publishers removed | Nothing new in the bucket; `...NoNewObject` after 30 h if SFTP is off; DTD's transfers fail on their side | Read the audit log for what was written and by whom; examine soft-deleted or unexpected objects; then a deliberate reapply |
| Budget trip → everything removed | Both of the above | As above, plus the billing report |

Recovery is always a deliberate Ranma reapply of the deploy values.
These go into `docs/schedule-feed-bucket.md` and `docs/alerts.md` with the
alerts (plan tasks 4.3 and 4.4).

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
    delivered_at: DateTime<Utc>,  // SFTP: mtime at upload close; GCS: object creation time
    bytes: u64,
    identity: SourceIdentity,     // what "seen before" means for this source
}
enum SourceIdentity {
    File { mtime: SystemTime },                              // + bytes
    Object { name: String, generation: i64, crc32c: u32 },   // + bytes
}

/// A peer source. Enum dispatch rather than `dyn`.
enum Source { WatchDir(WatchDirSource), Bucket(GcsSource) }

impl Source {
    /// Complete, routable candidates now. WatchDir applies today's
    /// stability gate. Bucket lists the root (finalized objects only, so no
    /// gate), flags unexpected names, and skips (name, generation) pairs
    /// already in its ledger.
    async fn poll(&mut self) -> anyhow::Result<Vec<Candidate>>;
    /// A local, fully-read, verified copy plus its SHA-256. Bucket: GET of
    /// exactly that generation into `sources/bucket/.partial`, hashing
    /// SHA-256 and CRC32C, size and CRC32C (and MD5 when present) checked
    /// against the object's metadata, fsync, rename into the archive.
    async fn fetch(&mut self, c: &Candidate) -> anyhow::Result<LocalDelivery>;
    /// After a decision: WatchDir moves CORPUS out as today. Bucket records
    /// the generation as confirmed and, once deleteMinAgeSecs has passed,
    /// deletes it with ifGenerationMatch.
    async fn settle(&mut self, c: &Candidate, d: &Decision) -> anyhow::Result<()>;
}
```

`GcsSource` wraps `object_store::gcp::GoogleCloudStorage`, built from the
bucket name and `GOOGLE_SERVICE_ACCOUNT_PATH` (the mounted key), with an
optional base URL for a fake-GCS test server. `object_store` has no
preconditioned delete and doesn't expose `x-goog-hash`, so two small JSON
API calls go over the existing `reqwest` with the bearer token from
`GoogleCloudStorage::credentials()`:

- `GET /storage/v1/b/{b}/o/{o}?generation={g}&fields=size,generation,crc32c,md5Hash,timeCreated`;
- `DELETE /storage/v1/b/{b}/o/{o}?ifGenerationMatch={g}`.

CRC32C uses the already-locked `crc` crate (`CRC_32_ISCSI`); MD5 the
already-locked `md-5`. Nothing new enters `Cargo.lock`.

### Loop and revocation guards

The looping reader is the realistic expensive failure, so the bucket source
guards against it on its own, independent of the alerts and the kill
switch:

- **Never re-download a confirmed generation.** The ledger
  (`sources/bucket/.seen`, last 200 entries, atomic writes) holds name,
  generation, CRC32C and SHA-256. A failed delete doesn't cause a
  re-download; the object is simply deleted again later, or expires.
- **Caps.** At most `maxDownloadsPerPoll` (2) downloads per poll,
  `maxDownloadBytesPerHour` (256 MiB) and `maxDownloadBytesPerDay`
  (1 GiB). Hitting a cap stops downloads until the window passes, sets
  `schedule_feed_source_download_capped` and fires the alert.
- **Backoff.** Errors back the poll interval off exponentially, to
  `maxBackoffSecs` (3600), and reset on success. A verification failure
  counts against the caps.
- **Revocation.** A 401 or 403 from any call sets
  `schedule_feed_source_access_revoked{source="bucket"} = 1`, logs **one**
  line per state change (not per poll), and backs off to
  `maxBackoffSecs`. It never crashes the process or blocks the SFTP
  source. The first successful call clears it.

### One pipeline, deduplicated by content

For each kind (CIF, CORPUS):

1. Order the candidates from every source by `delivered_at`. Within the
   same second, order by `sourcePrecedence`.
2. For the newest candidate not yet settled:
   1. `fetch` it and get the SHA-256.
   2. **Dedup.** If the SHA-256 is already in the *content ledger* (every
      delivery directory's `.delivery-ingested` and the quarantine
      records), do no extraction, check or POST. Log `outcome: duplicate`
      with `source`, `duplicate_of` and `first_source`; append the arrival
      to `.delivery-sources`; `settle`.
   3. **Otherwise** run today's path: extract with caps, run the CIF
      checks, mark complete, POST, record.
3. Older unsettled candidates are superseded, as `corpus.rs` already
   does.

### Records

- **`.delivery-ingested` v2** adds `source` and `source_ref`
  (`sftp:<file>` or `gs://<bucket>/<name>#<generation>`). A v1 (4-field)
  record reads as `source=sftp`.
- **`.delivery-sources`** (new, append-only) lists every arrival of this
  content.
- The **delivery directory name** is still `delivery_dir_name(delivered_at)`;
  a different-content collision in the same second gets a `-<source>`
  suffix.
- **api.** `schedule_feed_ingests.delivery_source` and
  `corpus_deliveries.delivery_source` (nullable text) record the source
  of the accepted copy.
- The **audit line** gains `source`, plus `duplicate_of` and
  `first_source` on `duplicate`.

### When the two sources disagree

Same content means dedup. Different content within
`disagreementWindowMinutes` (default 120) is unexpected:

- `schedule_feed_source_disagreement_total{kind}` is incremented and both
  SHA-256s, sizes and sources are logged.
- **The bucket wins** (D5). The other copy gets `outcome: superseded`.
  The bucket comes first because its publisher authenticates as DTD's
  own Google identity, while SFTP's password faces the public internet.
- **Every check still applies to the winner.** A quarantined winner does
  **not** promote the loser automatically.
- Outside the window, the later delivery is simply the newer one, and the
  existing `Generated`-date and record-count checks apply.

## 10. Chart: `charts/distant-signal` `scheduleFeed`

Separate switches, not a `sources:` list.

```yaml
scheduleFeed:
  enabled: false
  sftp:
    enabled: true            # NEW. Default true: existing values keep today's behaviour
    # ... every existing sftp.* key, unchanged
  bucket:
    enabled: false           # NEW, off by default
    provider: gcs            # only gcs is implemented
    name: ""                 # bucket name; required when enabled (Ranma's value)
    baseUrl: ""              # empty = storage.googleapis.com; set for a fake-GCS test server
    existingSecret: ""       # required: sealed reader key
    serviceAccountKey: service-account.json
    expectedKeys: [timetable_full.zip, CORPUSExtract.json.gz]   # same globs as SFTP routing
    pollIntervalSecs: 300
    deleteMinAgeSecs: 3600        # let the publisher's read-back and scan finish
    archiveKeep: 5                # raw objects kept locally
    maxObjectBytes: 536870912     # 512 MiB, as sftp.maxUploadFileSize
    maxDownloadsPerPoll: 2
    maxDownloadBytesPerHour: 268435456
    maxDownloadBytesPerDay: 1073741824
    maxBackoffSecs: 3600
    notifications:
      pubsub:
        enabled: false
        subscription: ""      # projects/<p>/subscriptions/<s>
        reconcileIntervalSecs: 3600
    auditLogs:
      ship: false
      bucket: ""             # the audit-log bucket
      pollIntervalSecs: 600
  sourcePrecedence: [bucket, sftp]
  disagreementWindowMinutes: 120
```

### Rendering rules

- `scheduleFeed.enabled` with neither source enabled → `fail`.
- **SFTP off.** No `sftp` container, Service, NodePort, entrypoint
  ConfigMap, host-key Secret or volume, 2022 ingress rule, `sftp-metrics`
  PodMonitor endpoint or `schedule-sftp` alert group. `ingest` gets
  `SFTP_SOURCE_ENABLED=false`. The PVC stays.
- **Bucket off.** No `BUCKET_*` env, no key mount, no bucket alerts.
- **Bucket on.** `BUCKET_SOURCE_ENABLED=true`, the `BUCKET_*` env, and the
  key from `existingSecret` mounted read-only with
  `GOOGLE_SERVICE_ACCOUNT_PATH` pointing at it (`fail` if
  `existingSecret` is empty).
- **NetworkPolicy.** The existing public-internet rule on 443 covers
  `storage.googleapis.com` and `oauth2.googleapis.com`; the chart adds
  both to the `egressSection` `urls` list so a custom `baseUrl` port is
  allowed too.
- `scheduleFeed.sftp.enabled` defaults to `true`, so every existing values
  file renders the same. The CI render test asserts this byte for byte.

Settings that ship **off by default**: `scheduleFeed.bucket.enabled`,
`bucket.notifications.pubsub.enabled`, `bucket.auditLogs.ship`, and in
`ds-ingest-bucket`: `enabled`, `usageAlerts.enabled`,
`notifications.pubsub.enabled`.

## 11. Security review: threat model compared with SFTP

| Concern | SFTP source | Bucket source |
| --- | --- | --- |
| Inbound exposure | NodePort 30450 on the public IP. Scanned and brute-forced; mitigated by the defender, rate limits, a 190-bit password and login-anomaly rules | **None.** Outbound HTTPS from the ingest pod only |
| Publisher credential | A shared password we created and gave DTD | **None of ours.** DTD's own Google service accounts; nothing to leak from our side, nothing to rotate with DTD |
| Publisher authority | Upload, overwrite, list in one directory | Create, overwrite, delete, read, list objects in one dedicated bucket; no IAM or bucket settings |
| Our authority | Filesystem access to the PVC | Get, list and delete on one bucket; never create |
| A compromised DTD identity | Replaces the timetable with a crafted file | The same: a poisoned or junk file. Mitigated by ingest content checks, quarantine, the SHA-256 audit, the expected-name allowlist, the audit-log rules, usage alerts and the kill switch. An overwrite or delete before we fetch is restorable from soft delete for a week, and SFTP plus dedup delivers the genuine copy |
| A leaked reader key | — | Reads public timetable data (egress cost, capped by alerts and the kill switch's egress trigger) and can delete undelivered objects (soft delete restores them; SFTP still delivers). Can't plant a file. No IP pin; detected by the "reader new source" rule |
| Cost abuse | None | A looping reader or a flood of uploads. Reader guards, usage alerts, the kill switch and the project budget (§5) |
| Public exposure of the bucket | — | Impossible: public access prevention enforced, UBLA on, and the render refuses public members |
| Detection | SFTPGo logs → Loki rules | Data Access audit logs → Loki rules; Prometheus for revocation, no-new-object, unexpected objects, download caps |
| New dependency | None | A GCP project to secure, Crossplane in the cluster holding a scoped project key |

**Why no private access path.** Private Service Connect or VPC endpoints
would keep traffic off the internet only for clients inside a VPC. The
publisher is STS in DTD's project and the reader is on our netcup node,
outside any VPC, so both must use the public endpoint. TLS and IAM
protect it.

**Running bucket-only removes the node's only public listener.** That is
the operator's choice (§12, step 7). If chosen, Ranma updates
`public-exposure-check.yml` and the NodePort monitoring exception.

## 12. Adoption plan

Nothing here retires SFTPGo. Each step can be undone by flipping a value.

1. **Project and bootstrap** (Ranma, OpenTofu, D2/D3/D7).
   - The dedicated project, its billing link and the budget (amount in Ranma-Config).
   - Check the effective org policies (§7).
   - The Crossplane provider's service account, its custom role and its
     key, sealed.
   - The reader's service account; its key made by hand and sealed as
     `distant-signal/distant-signal-schedulefeed-bucket`.
   - The Data Access audit config for `storage.googleapis.com` and the
     sink to the audit-log bucket.
   - The kill switch: alert policies, topic, function and its narrow IAM.
   - *Rollback:* destroy the stack.
2. **Crossplane** (Ranma): Crossplane v2 and provider-upjet-gcp
   (`provider-gcp-storage`, `-cloudplatform`; `-monitoring` and `-pubsub`
   only if used), activation limited to the kinds used, the
   `ClusterProviderConfig`, PSA labels, an egress policy for 443, and the
   kill-switch watcher (§5).
3. **The bucket** (Ranma, a HelmRelease of `charts/ds-ingest-bucket`):
   `enabled: true`, project, bucket name, `publisher.members` (DTD's
   service accounts), `reader.member`, `auditLogs.sinkWriterIdentity`.
   Wait for every resource to be `READY`. *Rollback:* `enabled: false`;
   the buckets stay.
4. **DS code and chart** (the plan's phases 1–4): turn on
   `scheduleFeed.bucket.enabled: true` **with SFTP still on**.
5. **Ask RDM to add the bucket** as a destination for the timetable and
   CORPUS subscriptions, **in addition to SFTP**, and send §14's
   questions.
6. **Verify both** for at least 7 daily deliveries and one CORPUS: each
   CIF has one `accepted` and one `duplicate` line with the same
   `sha256`; zero disagreement; the bucket's `delivered_at` within
   minutes of SFTP's; the audit log shows only DTD's principals writing
   and the reader reading and deleting; `...PublishStale` quiet.
7. **Steady state: the operator's choice**, reversible: both (default),
   bucket-only (closes the public port), or SFTP-only.

## 13. Costs (rough, monthly, USD)

From the artifact's europe-west2 figures, adjusted for D11.

| Item | Typical | Worst realistic |
| --- | --- | --- |
| Storage (objects in transit for under an hour; 7 days of soft-deleted CIFs, ~0.55 GB) | $0.01 | $0.02 |
| Requests (LIST every 5 min; a GET and a DELETE per delivery) | $0.04 | $0.22 (LIST every minute) |
| Egress to the node (~2.4 GB) | $0.29 | $0.29 |
| Audit logs (Data Access, sink, audit-log bucket) | $0 (inside the free allowance) | <$0.01 |
| Usage alert policies (if enabled) | cents | cents |
| Pub/Sub (only if enabled) | $0 (free tier) | $0 |
| **Total** | **≈ $0.35** | **≈ $0.55** |
| Runaway reader (re-downloading the zip every few minutes) | — | Hundreds of dollars a month. Prevented by the ledger and caps (§9), and stopped by the egress kill switch and the budget |

## 14. Open questions

Answered 2026-10-02 and recorded under "Decisions": the cloud (D8), the
project (D2), provisioning (D3, with Crossplane recommended), the audit
trail (D4), precedence (D5), repo scope (D7), the dedicated bucket (D9),
the publisher's access (D10), the in-transit object life (D11), Crossplane (D12) and pausing
bindings on a kill-switch trip (D13).
DTD's credential model and permissions are answered by their
instructions (§1).

### For the user

1. **Steady state** after verification (§12, step 7).

### Questions for DTD/RDM (ready to send)

Send these through the RDM support channel, not to the user. Don't send
any credential: none is needed.

> We'd like to add a Google Cloud Storage bucket (europe-west2) as a
> delivery destination for our timetable and CORPUS file subscriptions,
> alongside our existing SFTP destination. We'll grant your service
> accounts the roles in your instructions on that bucket only.
>
> 1. **Object names.** Will files land at the bucket root, and keep the
>    names `timetable_full.zip` and `CORPUSExtract.json.gz`, overwritten
>    on each delivery? Or are names dated, or under a prefix we can set?
> 2. **Scanner.** Does the malware scanner only read objects, or can it
>    delete or quarantine them in place? Does it, or the transfer, write
>    any other object (a probe or marker) into the bucket?
> 3. **Read-back.** After writing, how long do the transfer and the scanner
>    keep reading the object? We remove objects once we've downloaded
>    them, and want to wait long enough.
> 4. **Dual delivery.** Can one subscription deliver to both SFTP and GCS
>    at the same time, or would we need a second subscription?
> 5. **Testing.** Can you trigger a test delivery once the destination is
>    configured?

## Previously open, now decided (kept for the record)

- Which account owns this → D2: a new dedicated project.
- OpenTofu alone vs a controller → D3: Helm with a controller, OpenTofu
  for the one-off setup.
- Object-level audit → D4: the storage service's own logs.
- Source precedence → D5: the bucket wins.
- AWS's credential model (an access key we create and hand to DTD), and
  Azure's (account key or SAS) → D8: GCS instead.
- Reader IP pin → not available per principal on GCS (§5); detection
  instead.
- Cleanup → D11: the reader deletes after a verified download.

## Sources

- "Cloud Bucket Ingest Costs" artifact (user's, read 2026-10-02).
- DTD's GCS destination instructions (via the user, 2026-10-02).
- Google Cloud docs (2026-10-02): IAM roles for Cloud Storage
  (`iam-roles`), IAM permissions (`iam-permissions`: overwrite needs
  create and delete), soft delete (retains deleted and overwritten
  objects; 7–90 days), bucket IP filtering overview, Config Connector
  "Installing on other Kubernetes distributions".
- crossplane/crossplane v2.4.2 and crossplane-contrib/provider-upjet-gcp
  v3.0.0 release tags and CRDs (`package/crds`, namespaced `*.m.upbound.io`
  kinds); GoogleCloudPlatform/k8s-config-connector v1.157.0.
- `object_store` 0.14.2 source: `gcp` feature dependencies,
  `GoogleCloudStorageBuilder` credential options,
  `ApplicationDefaultCredentials` (no `external_account`),
  `GoogleCloudStorage::credentials()`, `get_opts` generation support.
- This repo: `crates/schedule-ingest`, `charts/distant-signal/templates/schedulefeed-*.yaml`,
  `networkpolicy.yaml`, `prometheusrule.yaml`, `docs/schedule-feed-sftp.md`.
