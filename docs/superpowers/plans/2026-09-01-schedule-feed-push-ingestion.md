# Schedule-Feed Ingestion via SFTP Push — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the receiving side of Network Rail/RDG's SFTP-push
delivery of the "Timetable - Full Refresh - Daily" CIF SCHEDULE feed —
a new `schedule-ingest` crate plus an off-the-shelf SFTP server, sharing one
Pod and one `ReadWriteOnce` PVC, reliably landing each day's 9-file delivery
on disk, verifying it against its own manifest, and recording each
successful ingest in `api`'s database — for both `docker-compose.yml` and
`charts/distant-signal/`, per
`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` ("the push
design doc"). This is the same scope the now-superseded
`docs/superpowers/specs/2026-08-30-schedule-feed-sftp-pull-design.md` ("the
pull design doc") targeted — files on disk, their arrival recorded in a
database — not the delay-inference logic that will eventually consume them,
and not `crates/trust-consumer`.

**Why this plan exists now, and why it starts with a blocker, not code:**
the push design doc's own closing section ("Summary (for the person who
asked)") states plainly that its single most urgent unresolved item — whether
push-side configuration is reachable *at all* for a normal Data Recipient,
given pull's own DTD-portal access turned out to be staff-only — is "the
concrete first question to put to RDG/DTD directly... before treating any of
this document's Helm/compose sketches as worth implementing," and that
"confirming that... and any resulting implementation plan are separate,
later work, not part of this design document." This plan is that later work.
It does not treat that question as settled, and it does not block every task
on it either — see Task 1 for exactly which tasks are gated and which are
not, and why.

**Architecture (net summary, per the push design doc's Design section):**

```
One Kubernetes Pod / two docker-compose services, one shared PVC/volume
  schedule-sftp   SFTPGo (drakkan/sftpgo) -- receives DTD's inbound push,
                  writes raw files into incoming/, no CIF/manifest awareness
                  of its own.
  schedule-ingest NEW crate. Polls incoming/ on a check-times cadence
                  (Europe/London, matching RSPS5046's documented window),
                  parses RJTTFnnn.DAT, verifies every listed file has landed
                  and stopped changing, moves a complete sequence to <nnn>/,
                  POSTs the ingest record to api, prunes old sequences.

crates/api
  routes/ingest.rs      + POST/GET /private/schedule-feed-ingests
  data/queries.rs        + last_schedule_feed_fetch, upsert_schedule_feed_ingest
  routes/freshness.rs     DataFreshness + schedule_feed field
  migrations/             + schedule_feed_ingests table

charts/distant-signal/
  values.yaml             + scheduleFeed block (opt-in, enabled: false)
  templates/
    schedulefeed-secret.yaml       NEW -- this app's own generated SSH host
                                    key + (never-generated) DTD account creds
    schedulefeed-pvc.yaml          NEW -- ReadWriteOnce, 5Gi default
    schedulefeed-deployment.yaml   NEW -- ONE Deployment, TWO containers
    schedulefeed-service.yaml      NEW -- LoadBalancer/NodePort, port 22-ish
    _helpers.tpl            + naming/secret helpers
    networkpolicy.yaml       + metrics-only ingress-allow for `ingest`
    podmonitor.yaml           + schedulefeed-ingest to the selector
    NOTES.txt                 + surfaces the generated host-key fingerprint

docker-compose.yml        + schedule-sftp, schedule-ingest services,
                             schedule_feed_data named volume
docker/schedule-ingest.Dockerfile   NEW
local.env.example / dev.env.example  + placeholders, following the
                                        existing RDM_*_BASE_URL convention
```

**Tech Stack:** Rust (no new async runtime, no new HTTP framework — reuses
`common::ingest`/`common::metrics`, `clap`, `tokio`, exactly the existing
`poller-*` toolkit); `chrono-tz` (new dependency for `schedule-ingest`, for
an `Europe/London`-aware check-times clock, matching the pull design doc's
own reasoning); **no SFTP/SSH client library** — unlike the now-superseded
pull design, `schedule-ingest` never dials out, it only scans a local mount
(`std::fs::read_dir`), so the pull design's `russh-sftp`/`ssh2` research
does not apply here at all. `drakkan/sftpgo` (the receiving daemon — an
off-the-shelf image, not application code this plan writes). Postgres
(`sqlx`) for the new ingest-record table. Docker Compose and Helm 4, no
subchart.

**Spec:** `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md`
— read in full before starting; this plan does not restate its research,
only carries its decisions into concrete tasks. Also load-bearing:
`docs/superpowers/specs/2026-08-30-schedule-feed-sftp-pull-design.md` (its
Pull procedure, Storage/retention, Database bookkeeping, and Configuration
shape sections are cited directly below — the push design doc says these
carry over "reused directly," and this plan reuses them the same way,
adapted for a local directory scan instead of a remote SFTP session) and
`docs/superpowers/specs/2026-08-18-helm-chart-design.md` (this chart's own
conventions, which every Helm task below must match). The
`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md`
("the ingress research doc") is cited only where the push design doc itself
cites it — its pre-addendum Sections 1, 3, and 4 are the ones still live per
the push design doc's own "Why push changes the shape back" and "The
receiving component" sections.

**A factual finding made while writing this plan, not asserted by any
predecessor document:** the repo's working tree currently contains an
untracked `timetable_full.zip` (76,446,640 bytes — matching the push design
doc's cited compressed-size anchor exactly) containing the real 9-file
sample delivery, including `RJTTF942DAT.txt`, the manifest. Reading that
file directly in this session resolves the push design doc's **Open
question 8** ("whether a manifest file's declared per-file sizes... can be
used for completeness-checking") **concretely, in the negative**: the real
manifest is an 8-line header/footer-wrapped list of bare filenames —
sequence number, generation date, and a bare `RJTTFnnn<SUFFIX>.txt` per
line, no byte-count or checksum field anywhere in it. So Task 3 below
implements the push design doc's own "mtime/size-stability across polling
cycles" fallback as the **only** completeness signal, not one of two
options. **`timetable_full.zip` itself must never be committed to this
repository** — see Global Constraints — it is real, licensed Network Rail
data (the push design doc's own Open question 9, the "research & analysis
purposes only" licensing wording, is still unresolved) and is, at 76MB,
wildly disproportionate to a git-tracked test fixture regardless. Task 3
uses small, hand-written fixtures that reproduce the manifest's *format*
(confirmed against the real file this session), not its content.

## What this plan is not

This is infrastructure and a modest amount of new backend code, not a
frontend feature — there is no user-facing surface here at all. Every
task's verification step is one or more of: `cargo test -p schedule-ingest`
/ `cargo test -p api` (real, runnable now — the manifest-parsing and
gap-detection logic is pure and unit-testable without any live SFTP
session), `cargo check --workspace`, `docker compose ... config` (validates
YAML/interpolation without starting anything), `helm lint`/`helm template`
(renders without a cluster), or — where genuinely meaningful and stated
explicitly as such — actually bringing up a real compose stack and
confirming the pipeline moves a sample delivery from `incoming/` through to
a recorded `api` ingest. **There is no way to test an actual DTD push
connection in this or any future session** — nobody controls DTD's side,
and per Task 1, whether DTD can even be configured to push here at all is
still an open question. Every task below says plainly which of its
verification steps are static/local versus which need real infrastructure
this environment may or may not have, and none of this plan's tasks pretend
a live DTD delivery has been exercised.

This plan also does not pretend the push design doc's other genuinely open
risks are solved. Carried forward explicitly into the tasks that touch them:

- **Whether `genPrivateKey` works cleanly under this chart's Helm-4
  lookup-preserve pattern (push design doc Open question 6) is untested by
  any predecessor document.** Task 6 is where this gets an actual
  `helm template`/`helm lint` check, and this plan's own text reports the
  outcome as "verified here" or "not verified here" rather than assuming
  success, exactly like the dev-oidc-server plan's precedent for its own
  analogous `AUTHENTIK_SECRET_KEY` generation.
- **Neither candidate SFTP image's exact entrypoint/security-context
  requirements have been tested against a real cluster (Open question 5).**
  Task 8 states this plainly rather than asserting a `securityContext` block
  is correct by construction.
- **Whether DTD's push client supports public-key auth, and what static
  destination it needs (Open questions 1, 2, 4)** are not resolved by this
  plan — Task 1 is a decision checkpoint, not a resolution, and every later
  task that touches DTD-facing credentials or Service reachability says so
  explicitly rather than guessing.

## Global Constraints

- **Task 1 is a required, non-code checkpoint. Do not skip it or treat its
  outcome as a foregone "yes."** Per the push design doc's own framing, this
  is the single most consequential open item in the whole design. See Task 1
  for exactly which later tasks are gated on its outcome (credential
  wiring, the real Service's external reachability) and which are not
  (everything that only concerns this app's own infrastructure and is
  testable against sample data regardless of whether DTD can ever reach it).
- **`timetable_full.zip` (the real 76MB sample delivery, currently
  untracked in the working tree) must never be `git add`ed, by any task in
  this plan.** It is real Network Rail data of unresolved licensing status
  (push design doc Open question 9) and is not an appropriate size or
  content for a committed test fixture. Every task that needs sample data
  for a test uses small, hand-written fixtures whose *format* (not content)
  is confirmed against it in this plan's own research above.
- **`schedule-ingest` is a new crate, not an extension of `aggregator` or
  `trust-consumer`.** Per `DESIGN.md`'s "one crate per concern" convention,
  already the deciding factor in the (now-superseded) pull design doc's
  identical reasoning — reused here unchanged, since the "new crate" call
  was about separation of concerns, not about which mechanism moves bytes.
  **Name stays `schedule-ingest`, not `poller-schedule`** — same reasoning
  the pull design doc gave (it does not share the `poller-*` family's tight
  interval-loop shape, and the name should not visually invite someone to
  "simplify" it back toward one).
- **No SFTP/SSH client library dependency in `schedule-ingest`.** This
  crate never dials out — it only scans a local mounted directory
  (`std::fs::read_dir`/`tokio::fs`). The pull design doc's `russh-sftp`
  vs `ssh2` research is dead code for this plan; do not add either crate.
- **SFTPGo (`drakkan/sftpgo`), not `atmoz/sftp`, is the receiving image —
  fixed by the push design doc's own maintenance-currency argument, not
  revisited here.**
- **Two containers, one Pod, one `ReadWriteOnce` PVC — fixed by the push
  design doc's "The reader/writer problem" section, not revisited here.**
  Do not split `schedule-sftp` and `schedule-ingest` into two Deployments
  connected by a `ReadWriteMany` storage class or a network call; do not
  attempt SFTPGo's event-hook mechanism as a single-container alternative
  (Open question 7 — explicitly deferred future work, not this plan's job).
- **`replicas: 1` fixed, `strategy: Recreate`, matching `aggregator` and
  every `poller-*` Deployment's own existing rationale.** There is exactly
  one Pod for this subsystem, ever.
- **This app generates and owns its own SSH host keys. It never touches
  DTD's host key.** Inverted from the pull design's "verify DTD's host key"
  problem, per the push design doc's Credentials section. Do not write any
  "verify the remote host key" logic anywhere in this plan — there is no
  remote connection for `schedule-ingest` to make.
- **DTD account credentials (username, password/public-key) are NEVER
  auto-generated by this chart, on either path** — identical rule to
  `pollers.*.apiKey` ("rendered, possibly empty, but never generated") and
  the pull design doc's own stated reasoning for the inverse credential.
  A random password or keypair is meaningless without DTD's side
  registering it — which Task 1 establishes is not even confirmed to be
  self-service. `authMethod` has no default (per push design doc Open
  question 2, RSPS5046 states neither mechanism for either direction).
- **`scheduleFeed.enabled` defaults to `false`, matching every RDM poller's
  own opt-in-by-default posture.** A default install is completely
  unaffected by anything in this plan.
- **Retention: keep `retentionKeepSequences` (default 2) complete
  generations; default PVC size `5Gi`** — reused directly from the pull
  design doc's Storage and retention section (mechanism-agnostic — the
  push design doc states this carries over unchanged), sized against the
  real ~711MB-uncompressed-per-generation anchor.
- **No cloud-storage-bucket variant anywhere in this plan.** Explicitly out
  of scope per the push design doc's own Non-goals ("this document *is* the
  SFTP-push design; it does not design the cloud-bucket variant").
- **`/private/schedule-feed-ingests` follows this codebase's existing
  ingest-route contract exactly** (`common::ingest::{LastFetchedResponse,
  post_batch}`-shaped, gated by `require_internal_token`, same
  `GET`-returns-freshness / `POST`-records-arrival pattern every other
  `routes/ingest.rs` handler already uses) — no new wire contract invented.
- **`schedule-ingest` never touches Postgres directly** — same rule
  `networkpolicy.yaml`'s own comment already states for every poller ("the
  pollers never do -- they reach the database only indirectly, by POSTing
  to the api's `/private/*` ingest endpoints"). It reaches `api` over HTTP
  only, exactly like `poller-ldbws`/`poller-incidents`/etc.
- **The manifest-completeness check uses mtime/size stability across
  consecutive polling cycles, not a declared per-file size.** Per this
  plan's own research finding above (Open question 8, resolved negatively
  against the real sample): RSPS5046's manifest format has no size field.
  Do not implement or assume a size-comparison-against-the-manifest check
  anywhere in Task 3 — it cannot exist against the real wire format.
- **`Europe/London`-aware check times (`chrono-tz`), not naive UTC** —
  reused directly from the pull design doc's Scheduling section, which the
  push design doc's "Directory layout inside the shared PVC" section
  explicitly carries forward unchanged (the window describes when DTD
  *produces* the feed, independent of delivery direction).
- **New top-level files/directories this plan introduces:**
  `crates/schedule-ingest/` (new crate — `Cargo.toml`, `src/main.rs`,
  `src/config.rs`, `src/scan.rs`, `src/manifest.rs`),
  `docker/schedule-ingest.Dockerfile`,
  `charts/distant-signal/templates/schedulefeed-secret.yaml`,
  `charts/distant-signal/templates/schedulefeed-pvc.yaml`,
  `charts/distant-signal/templates/schedulefeed-deployment.yaml`,
  `charts/distant-signal/templates/schedulefeed-service.yaml`,
  `crates/api/migrations/<timestamp>_schedule_feed_ingests.sql`.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same
  file for pure logic (`schedule-ingest`'s manifest parsing, gap
  classification, retention selection — none of it needs a live filesystem
  or database, matching this repo's existing convention of unit-testing
  pure logic directly, e.g. `line_status.rs`'s `parse_modes` tests);
  `#[ignore]`d live-database tests colocated in `mod db_tests` for the new
  `api` route/queries, following `custom_lines.rs`'s existing fixture/
  cleanup shape. `schedule-ingest`'s directory-scanning I/O (real
  `std::fs` calls against a `tempdir`) is tested with plain `#[test]`s
  using `tempfile` (already a transitive dev-dependency pattern this
  workspace can add cleanly — check for an existing use before adding a new
  one) — no `#[ignore]` needed for these, since they need no live service,
  only a scratch directory. Every backend task's verification step runs
  `cargo test -p schedule-ingest` and/or `cargo test -p api` (plus, where
  noted, the relevant `#[ignore]`d test run by hand against a real
  database) and requires it to pass with no new failures.

## Prerequisites this plan cannot verify without real infrastructure

1. **A real DTD push connection.** Cannot be created or simulated by any
   task in this plan, ever, from a sandboxed session — DTD's side is
   entirely outside this repo's control. Every task below that would
   otherwise "verify against a live delivery" instead verifies against the
   sample-derived fixtures from Task 3, and says so.
2. **Docker + network access to pull `drakkan/sftpgo` and build the new
   `schedule-ingest` image** — needed for Task 10's live compose
   verification. If unavailable, Task 10's live steps must be reported as
   not run, not assumed to pass — same posture the dev-oidc-server plan's
   Task 4 already established for this repo.
3. **A local Kubernetes cluster (kind/minikube/k3d) with `kubeconform` or a
   `kubectl` with API-schema access** — needed for any live rendered-
   manifest schema validation beyond `helm template`'s own static YAML-shape
   check in Tasks 7-8. If neither is available, those steps stay static
   YAML-shape checks only, stated as such.
4. **`DATABASE_URL`/a live Postgres instance** — needed for Task 4's
   `#[ignore]`d `db_tests` to actually run (not merely compile).

---

### Task 1: RDG/DTD push-reachability decision checkpoint — NOT a code task

**This task produces a recorded decision/outreach outcome, not a diff.**
Per the push design doc's own "Summary (for the person who asked)" section,
this is "the concrete next step... before treating any of this document's
Helm/compose sketches as worth implementing" — but, per that same document's
Non-goals ("Resolving every open question below" is explicitly out of
scope for the design doc itself), this plan does not require the answer
before *starting* engineering work, only before **going live with a real
DTD-facing credential or a real internet-reachable Service**. Mirrors the
"decision checkpoint, not a code task" pattern established by
`docs/superpowers/plans/2026-08-31-private-custom-lines-and-tracked-trains.md`'s
Task 1.

**What to actually do:** put a direct question to RDG/DTD support (not the
portal — per the push design doc, portal access is exactly what's in
question), covering, in one outreach if possible:

- Push design doc **Open question 1** (most consequential): is push-side
  SFTP-server configuration reachable for a normal Data Recipient, or is
  the entire DTD portal staff-gated the same way pull access turned out to
  be?
- **Open question 2**: what does DTD's push-destination registration
  actually require — a static IP, a stable hostname, some other
  pre-validation step — before it will push to this app's server at all?
- **Open question 4**: does DTD publish fixed outbound source-IP ranges for
  its push client, for inbound firewall allowlisting?
- **Open question 9**: the "research & analysis purposes only" licensing
  wording question (carried from the ingress research doc's Addendum §2,
  unresolved by either later document) — worth asking in the same outreach
  since it needs confirming regardless of delivery mechanism.
- **Open question 10**: expected account/portal provisioning lag, if
  push-side access is reachable at all.
- Whether DTD's push client supports SFTP public-key auth against this
  app's server, or only password auth (affects `scheduleFeed.sftp.authMethod`
  below, Task 7).

**Which later tasks are gated on the outcome, and which are not** — record
this distinction explicitly, do not let it become implicit:

- **Not gated — proceed regardless of the outcome:** Tasks 2-6, 8-9. These
  build `schedule-ingest`'s own logic, the `api` route, the Helm/compose
  *shape* (secrets, PVC, Deployment, Service, values.yaml), and the
  Dockerfile. All of this is useful infrastructure to have ready
  independent of whether DTD can ever reach it, exactly the way this
  chart's other opt-in, `enabled: false`-by-default subsystems (every RDM
  poller, `devAuthentik`) already ship with unconfirmed or intentionally
  placeholder external endpoints. This mirrors the push design doc's own
  framing of its sketches as "sketch — not final," not "do not build."
- **Genuinely gated — do not attempt for real until Task 1 has an answer:**
  filling in a real `scheduleFeed.sftp.username`/`authMethod`/`password`/
  `publicKey` value, requesting DTD register this app's actual host-key
  fingerprint or destination address, and exposing the real Service to the
  actual internet in a way DTD is expected to reach (as opposed to
  rendering/lint-checking the Service resource, which Task 8 still does).
  Task 10's live verification explicitly does not attempt a real DTD
  connection either way, so it is unaffected by this gate.

**2026-09-01 status: unresolved, blocked-pending-repo-owner-action — not
skipped, not defaulted to "yes."** This is a real-world outreach to RDG/DTD
support, which no agent running in this sandbox has any channel to perform
(no email/support-portal access, no prior correspondence to continue, and
per the design doc itself, the DTD portal — the one channel that does
exist — is exactly the thing in question for pull access and unconfirmed
for push). Per this task's own framing and the identical precedent set by
`docs/superpowers/plans/2026-08-31-private-custom-lines-and-tracked-trains.md`'s
Task 1, this is recorded here as genuinely open rather than fabricated,
guessed, or silently treated as resolved. Tasks 2–9 were implemented
regardless, per this task's own "not gated" list below — none of them
required this outcome. **Genuinely gated and still not started as of this
status note**: filling in a real `scheduleFeed.sftp.username`/`authMethod`/
`password`/`publicKey` value, requesting DTD register this app's actual
host-key fingerprint or destination address, and exposing the real Service
to the actual internet. Whoever next has an actual channel to RDG/DTD
support should perform Step 1 below and update this status note with the
real outcome before any of those gated actions are taken.

- [ ] **Step 1: Send the outreach**

Via whatever channel is available to whoever executes this plan (repo
owner's own contact with RDG/DTD support — this step cannot be automated by
an agent with no such channel; if no such channel exists in this session,
say so plainly and treat this step as blocked-pending-repo-owner-action,
not skipped).

- [ ] **Step 2: Record the outcome**

Write the answer (or "still pending," or "no channel available in this
session") directly into this plan file, replacing this Step 2's text, before
any gated task (per the list above) proceeds past its render-only
verification into anything DTD-facing.

---

### Task 2: `schedule-ingest` crate scaffold

**Files:**
- Create: `crates/schedule-ingest/Cargo.toml`
- Create: `crates/schedule-ingest/src/main.rs`
- Create: `crates/schedule-ingest/src/config.rs`
- Modify: workspace `Cargo.toml` (add the new member)
- Create: `docker/schedule-ingest.Dockerfile`

**Interfaces:**
- Produces: the `schedule-ingest` binary crate, its `Config` struct.
  Consumed by Task 3 (manifest/scan logic lives in this crate), Task 9
  (docker-compose wiring), Task 8 (Helm image reference).

Not gated by Task 1 (see Task 1's "not gated" list).

- [x] **Step 1: Add the crate to the workspace**

Add `crates/schedule-ingest` to the root `Cargo.toml`'s `[workspace]
members`, following the existing list's ordering/style.

- [x] **Step 2: `Config`, following `crates/poller-ldbws/src/config.rs`'s
  exact convention**, adapted for a directory-scanning watcher instead of a
  remote SFTP client per the push design doc's own compose/Helm sketches
  (`WATCH_DIR`/`STORAGE_DIR`/`CHECK_TIMES`/`RETENTION_KEEP_SEQUENCES`):

```rust
// sketch — adapt field names/docs to match crates/poller-ldbws/src/config.rs's
// exact formatting conventions when implementing.
use std::path::PathBuf;
use clap::Parser;

/// CLI/env configuration for the `schedule-ingest` service.
///
/// Unlike the now-superseded pull design's equivalent Config, this crate
/// makes no outbound SFTP connection at all -- it only scans a local
/// mounted directory that the sibling `schedule-sftp` (SFTPGo) container
/// writes into. See docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md.
#[derive(Debug, Parser)]
pub struct Config {
    /// Where the SFTP daemon writes incoming files. Scanned each check
    /// time via std::fs::read_dir -- see src/scan.rs.
    #[arg(long, env, default_value = "/data/schedule-feed/incoming")]
    pub watch_dir: PathBuf,

    /// Root of the shared PVC. Verified-complete sequences move to
    /// `storage_dir/<nnn>/`; retention pruning operates on this directory's
    /// immediate numeric subdirectories.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// Comma-separated HH:MM times, Europe/London -- reused directly from
    /// the (now-superseded) pull design's Scheduling section: the window
    /// describes when DTD *produces* the feed, not which party connects.
    #[arg(long, env, default_value = "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00")]
    pub check_times: String,

    /// How many complete sequences to retain on disk (current + fallback).
    #[arg(long, env, default_value_t = 2)]
    pub retention_keep_sequences: u32,

    /// How many consecutive polling cycles a manifest-listed file's mtime
    /// and size must be unchanged before it's treated as a completeness
    /// candidate -- see Task 3. RSPS5046's manifest carries no per-file
    /// size field (confirmed directly against the real sample in this
    /// plan's own research), so this stability check is the only
    /// completeness signal available, not a fallback.
    #[arg(long, env, default_value_t = 2)]
    pub stability_cycles: u32,

    #[arg(long, env, default_value = "http://api:8080/private/schedule-feed-ingests")]
    pub api_ingest_url: String,

    #[arg(long, env)]
    pub internal_token: String,

    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

- [x] **Step 3: `main.rs` skeleton** — `tracing-subscriber` init,
  `common::metrics::install` (matching every other poller's `main.rs`
  opener), `Config::parse()`, and a `tokio::main` entry point that Task 5
  fills in with the real scheduling loop. Leave the loop body as a `todo!()`
  or a single immediate scan-and-log call for now — this step's job is only
  to get the crate compiling and matching this repo's existing `main.rs`
  shape, not to implement the loop (Task 5's job).

- [x] **Step 4: `docker/schedule-ingest.Dockerfile`**, copying
  `docker/poller-ldbws.Dockerfile` byte-for-byte except for the crate name
  and binary path (same `rust:1.88-bookworm` pin, same BuildKit cache-mount
  shape, same multi-stage builder/runtime split — no C-toolchain
  requirement exists for this crate, unlike `trust-consumer`, so the
  runtime stage needs nothing `poller-ldbws`'s doesn't already have).

- [x] **Step 5: Compile-check**

Run: `cargo check -p schedule-ingest` (and `cargo check --workspace` to
confirm the new workspace member doesn't break anything else).
Expected: PASS (a `main.rs` with a `todo!()` loop body still compiles).

- [x] **Step 6: Commit**

```bash
git add Cargo.toml crates/schedule-ingest docker/schedule-ingest.Dockerfile
git commit -m "Scaffold the schedule-ingest crate"
```

---

### Task 3: Manifest parsing, gap detection, and completeness logic (pure, unit-testable)

**Files:**
- Create: `crates/schedule-ingest/src/manifest.rs`
- Create: `crates/schedule-ingest/src/scan.rs`

**Interfaces:**
- Produces: `manifest::{Manifest, parse}` (parses an `RJTTFnnn.DAT`'s
  content into a sequence number + ordered file-name list), a
  `manifest::classify_sequence(last: Option<u32>, current: u32) ->
  SequenceRelation` (expected/gap, mirroring the pull design doc's Pull
  procedure step 4 classification, reused directly per the push design
  doc), and `scan::{scan_incoming, StabilityTracker}` (directory listing +
  the mtime/size-stability completeness check).
- Consumed by: Task 5 (the async orchestration loop wraps these pure
  functions with real I/O and the `api` POST).

Not gated by Task 1 — this logic is entirely testable against
hand-written fixtures matching the real manifest's confirmed format,
independent of whether DTD can ever push here.

- [x] **Step 1: `manifest::parse`**

The real `RJTTF942DAT.txt` (read directly from the untracked
`timetable_full.zip` while writing this plan — **not committed to this
repo**, see Global Constraints) has this exact shape:

```
/!! Start of file                                                               \r\n
/!! Content type:  DAT                                                          \r\n
/!! Sequence:      942                                                          \r\n
/!! Generated:     28/08/2026                                                   \r\n
/!! Exporter:      RjEhrTTT                                                     \r\n
RJTTF942ZTR.txt
RJTTF942REJ.txt
RJTTF942SET.txt
RJTTF942FLF.txt
RJTTF942MCA.txt
RJTTF942MSN.txt
RJTTF942ALF.txt
RJTTF942TSI.txt
/!! End of file (8 records) (28/08/2026)                                        
```

No byte-size or checksum field anywhere — confirming Open question 8
negatively, as noted above. Write `parse(content: &str) -> Result<Manifest>`
extracting the `Sequence:` line's number and the 8 bare filenames between
the header and footer `/!!` lines (do not depend on the exact fixed-width
padding of the `/!!` lines beyond recognizing the `/!!` prefix — the real
file pads with trailing spaces to a fixed column width, which is brittle to
match exactly and not meaningful to this parser's job). `Manifest` holds
`{ sequence: u32, files: Vec<String> }` — deliberately *not* the manifest's
own filename (`RJTTFnnnDAT.txt`), which the manifest correctly excludes
from its own listing (per RSPS5046 §5.2.2, already confirmed by prior
research — a `.DAT` file lists every other file in the delivery except
itself).

- [x] **Step 2: `classify_sequence`**, reused directly from the pull design
  doc's Pull procedure step 4 (the push design doc states this "all applies
  here unchanged"):

```rust
pub enum SequenceRelation {
    AlreadyIngested,
    Expected,       // nnn == last + 1, or first-ever ingest (last is None)
    Gap,            // anything else -- non-contiguous, per RSPS5046 §7.4
}

pub fn classify_sequence(last: Option<u32>, current: u32) -> SequenceRelation {
    match last {
        None => SequenceRelation::Expected,
        Some(last) if current == last => SequenceRelation::AlreadyIngested,
        Some(last) if current == last + 1 => SequenceRelation::Expected,
        Some(_) => SequenceRelation::Gap,
    }
}
```

A `Gap` is logged at `ERROR` with both sequence numbers and increments a
`distant_signal_schedule_feed_sequence_gap_total` counter (via
`common::metrics`) but **still proceeds to ingest** — per RSPS5046 §7.4, a
non-contiguous sequence number is documented, expected behaviour after an
"Empty" feed, not proof of a missed delivery (reused directly from the pull
design doc, which the push design doc states applies "irrespective of
delivery direction").

- [x] **Step 3: `scan::scan_incoming` and `StabilityTracker`**

```rust
// sketch -- adapt to this crate's actual error-handling conventions
// (anyhow, matching every other crate in this workspace) when implementing.

/// One directory listing of `watch_dir`, keyed by filename, each with its
/// current (mtime, len). Cheap to build every polling cycle
/// (std::fs::read_dir + metadata()).
pub struct DirSnapshot(pub std::collections::HashMap<String, (std::time::SystemTime, u64)>);

pub fn scan_incoming(watch_dir: &std::path::Path) -> anyhow::Result<DirSnapshot> { /* ... */ }

/// Tracks how many consecutive snapshots each filename's (mtime, len) pair
/// has been unchanged. A file only becomes a completeness candidate once
/// it has been stable for `stability_cycles` consecutive polls -- the push
/// design doc's own recommended mitigation for a push receiver seeing
/// files land via DTD's own outbound connection in real time (unlike a
/// pull's remote directory listing, which only ever sees a file DTD has
/// already finished writing).
pub struct StabilityTracker {
    stable_since: std::collections::HashMap<String, ((std::time::SystemTime, u64), u32)>,
}

impl StabilityTracker {
    pub fn observe(&mut self, snapshot: &DirSnapshot, required_cycles: u32) -> Vec<String> {
        // Returns filenames that have just reached `required_cycles`
        // consecutive identical (mtime, len) observations. Filenames
        // absent from `snapshot` (deleted/never seen) are dropped from
        // internal tracking, not just skipped.
        todo!()
    }
}
```

**Applies specifically to the manifest file itself first**, per the push
design doc's own recommendation ("require a manifest file's own
modification time to be stable... before treating any of its listed files
as candidates for completeness checking at all") — the orchestration loop
(Task 5) checks the `RJTTFnnn.DAT` file's own stability before even
attempting to parse it, then checks stability of every file the parsed
manifest names before considering the whole delivery complete.

- [x] **Step 4: Unit tests**

Using small, hand-written fixture strings reproducing the real manifest's
confirmed format (not the real 76MB delivery — see Global Constraints):
- `parse` extracts the correct sequence number and exactly 8 filenames from
  a fixture matching the real shape above; rejects/errors on a fixture
  missing the `Sequence:` line or with zero listed files.
- `classify_sequence`: `None` → `Expected`; `last=942, current=942` →
  `AlreadyIngested`; `last=942, current=943` → `Expected`; `last=942,
  current=944` → `Gap`; `last=942, current=941` → `Gap`.
- `StabilityTracker`: a file whose (mtime, len) is identical across
  `stability_cycles` consecutive `observe()` calls is returned exactly
  once, on the cycle it reaches the threshold, not on every subsequent
  cycle; a file whose size changes between observations resets its
  counter to 0, not to 1.
- `scan_incoming` (real `std::fs` I/O against a `tempfile::tempdir()`):
  returns the correct set of (name, mtime, len) triples for a directory
  seeded with a few files of known sizes; an empty directory returns an
  empty snapshot, not an error.

- [x] **Step 5: Run the crate's test suite**

Run: `cargo test -p schedule-ingest`
Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add crates/schedule-ingest/src/manifest.rs crates/schedule-ingest/src/scan.rs crates/schedule-ingest/Cargo.toml
git commit -m "Add manifest parsing, sequence-gap classification, and directory-stability tracking to schedule-ingest"
```

---

### Task 4: `api` — `/private/schedule-feed-ingests` route, migration, freshness field

**Files:**
- Create: `crates/api/migrations/<timestamp>_schedule_feed_ingests.sql`
  (timestamp after `20260901120000`, this directory's existing convention)
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/ingest.rs`
- Modify: `crates/api/src/routes/freshness.rs`

**Interfaces:**
- Produces: `POST`/`GET /private/schedule-feed-ingests`
  (`LastFetchedResponse`-shaped `GET`, matching every sibling ingest route),
  `DataFreshness.schedule_feed: Option<DateTime<Utc>>`.
- Consumed by: Task 5 (`schedule-ingest`'s POST call), the frontend's
  existing `/public/freshness` consumers (unaffected until a future task
  chooses to surface it — out of scope here, matching this plan's Non-goals
  inherited from the design doc).

Not gated by Task 1 — this is entirely this app's own database/API surface.

- [x] **Step 1: Migration**

```sql
-- -------------------------------------------------------------------------
-- Records each successfully-verified schedule-feed delivery, per
-- docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md's Database
-- bookkeeping section (reused directly from the pull design doc, unchanged
-- -- this table's shape doesn't depend on how the files arrived).
-- `files` is a JSONB array of {name, bytes} -- the per-file sizes
-- schedule-ingest itself observed on disk once stable, NOT a manifest-
-- declared size (the real manifest has no such field -- see this plan's own
-- Task 3 research note).
-- -------------------------------------------------------------------------
CREATE TABLE schedule_feed_ingests (
    sequence INTEGER PRIMARY KEY,
    ingested_at TIMESTAMPTZ NOT NULL,
    files JSONB NOT NULL
);
```

- [x] **Step 2: `queries.rs` additions**

```rust
pub async fn last_schedule_feed_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (ingested_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(ingested_at) FROM schedule_feed_ingests")
            .fetch_one(pool)
            .await?;
    Ok(ingested_at)
}

pub async fn insert_schedule_feed_ingest(
    pool: &PgPool,
    sequence: i32,
    ingested_at: chrono::DateTime<chrono::Utc>,
    files: &serde_json::Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO schedule_feed_ingests (sequence, ingested_at, files) VALUES ($1, $2, $3) \
         ON CONFLICT (sequence) DO NOTHING",
    )
    .bind(sequence)
    .bind(ingested_at)
    .bind(files)
    .execute(pool)
    .await?;
    Ok(())
}
```

`ON CONFLICT DO NOTHING` on `sequence`, not an upsert — a re-POST of an
already-recorded sequence (e.g. after `schedule-ingest` restarts mid-cycle
and re-observes a delivery it already recorded) is a harmless no-op, not an
error, matching this route's own idempotency needs — `schedule-ingest`
itself doesn't track "have I already POSTed this" locally (state lives in
`api`, per the pull design doc's own reasoning, reused unchanged).

- [x] **Step 3: `routes/ingest.rs` route**

```rust
#[derive(Debug, Deserialize)]
struct ScheduleFeedIngestRequest {
    sequence: i32,
    ingested_at: chrono::DateTime<chrono::Utc>,
    files: Vec<ScheduleFeedFile>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ScheduleFeedFile {
    name: String,
    bytes: u64,
}

async fn get_schedule_feed_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_schedule_feed_fetch(&app.database).await.map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_schedule_feed_ingest(
    State(app): State<App>,
    Json(req): Json<ScheduleFeedIngestRequest>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let files = serde_json::to_value(&req.files).map_err(|e| internal_error(e.into()))?;
    queries::insert_schedule_feed_ingest(&app.database, req.sequence, req.ingested_at, &files)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: 1 }))
}
```

Add the route to `router()`:
```rust
.route(
    "/schedule-feed-ingests",
    axum::routing::get(get_schedule_feed_last_fetched).post(post_schedule_feed_ingest),
)
```

This route is reached only via `private_router()`, already gated by
`require_internal_token` for the whole router — no new auth wiring needed,
matching every sibling route in this file.

- [x] **Step 4: `freshness.rs` field**

Add `schedule_feed: Option<DateTime<Utc>>` to `DataFreshness`, populate it
in `get_freshness` via a fifth `tokio::try_join!` arm calling
`queries::last_schedule_feed_fetch`. Update the module doc comment's list
of "four data sources" to five, and its "Station-samples is deliberately
omitted" note stays as-is (unrelated).

- [x] **Step 5: Tests**

Route-level `#[ignore]`d `db_tests` (real `PgPool`, matching this file's
existing convention): a `POST` followed by a `GET` returns the just-posted
`ingested_at`; re-`POST`ing the same `sequence` with a different
`ingested_at` does not change the recorded row (confirms `DO NOTHING`);
`GET` against an empty table returns `{"fetched_at": null}`. Extend
`freshness.rs`'s existing pure `#[cfg(test)]` tests
(`serializes_missing_data_as_null`, `round_trips_a_present_timestamp`) to
cover the new `schedule_feed` field the same way its four siblings are
already covered.

- [x] **Step 6: Run the crate's test suite**

Run: `cargo test -p api`
Expected: PASS (the new `db_tests` will show as `ignored` without a live
database, matching this repo's existing baseline — e.g. the private-custom-
lines plan's own "157 passed, 0 failed, 41 ignored" result).

- [x] **Step 7: Commit**

```bash
git add crates/api/migrations crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs crates/api/src/routes/freshness.rs
git commit -m "Add POST/GET /private/schedule-feed-ingests and a schedule_feed freshness field"
```

---

### Task 5: `schedule-ingest` — the async orchestration loop

**Files:**
- Modify: `crates/schedule-ingest/src/main.rs`

**Interfaces:**
- Consumes: Task 3's `manifest`/`scan` modules, Task 4's
  `/private/schedule-feed-ingests` route (via `common::ingest::post_batch`
  or a bespoke single-object POST, since this is one record per ingest, not
  a batch — check whether `post_batch`'s `&[T]` shape is worth reusing with
  a one-element slice for consistency, or whether a direct
  `reqwest::Client::post` call is clearer here; either is acceptable,
  document the choice in the code).
- Produces: the actual running behavior of the `schedule-ingest` binary.

Not gated by Task 1 — this wires together logic that runs identically
whether or not DTD can ever reach the receiver; the loop itself has no
DTD-facing code at all (that's entirely `schedule-sftp`'s job).

- [x] **Step 1: Check-times scheduling**

Reused directly from the pull design doc's Scheduling section (the push
design doc states this "reuses the ingress research doc's own Section 3(a)
'polling a mounted volume' shape... more directly... since this is now
genuinely a local-filesystem polling problem"): parse `check_times` into a
`Vec<NaiveTime>`, compute the next one after `now()` in `Europe/London`
(`chrono-tz`), `tokio::time::sleep_until` it, then run one scan cycle, loop.
**First run bypasses the check-time gate** and scans immediately — matching
the pull design doc's `time_until_next_poll` "no prior fetch → poll now"
precedent, itself matching RSPS5046 §7.6.1's "new recipients get a full
refresh regardless of when they start."

- [x] **Step 2: One scan cycle**

1. `scan::scan_incoming(watch_dir)`.
2. If a `RJTTFnnn.DAT` is present and stable for `stability_cycles`
   (`StabilityTracker`), `manifest::parse` it.
3. `GET /private/schedule-feed-ingests` for the last recorded sequence;
   `manifest::classify_sequence`.
   - `AlreadyIngested`: heartbeat log, done for this cycle.
   - `Gap`: `ERROR` log with both sequence numbers, increment the gap
     counter, proceed as `Expected` would.
   - `Expected`: proceed.
4. For every filename the manifest lists, require it present in the
   snapshot and stable for `stability_cycles` before treating the whole
   delivery as complete. If any listed file is missing or still unstable,
   leave everything in place and retry next cycle — **not** treated as a
   failure until the schedule's final check time (`16:00`, the documented
   fallback) passes with still no complete manifest, at which point log
   `ERROR` as a likely real delivery problem (reused directly from the pull
   design doc's Pull procedure step 6, adapted: "left in `tmp/`" becomes
   "left in `incoming/`," since there is no `tmp/` staging directory on the
   push side per the push design doc's directory layout).
5. Once complete: atomically move (`std::fs::rename`, same filesystem —
   the shared PVC guarantees this) every manifest-listed file plus the
   manifest itself from `watch_dir` into `storage_dir/<nnn>/`.
6. `POST /private/schedule-feed-ingests` with `{sequence, ingested_at:
   now(), files: [{name, bytes} for each moved file, using the size
   observed by the stability check]}`.
7. Prune: list `storage_dir`'s immediate numeric subdirectories, keep the
   `retention_keep_sequences` highest, delete the rest (`std::fs::remove_dir_all`).

- [x] **Step 3: Metrics**

`distant_signal_schedule_feed_sequence_gap_total` (Step 2's `Gap` branch),
`distant_signal_schedule_feed_last_ingest_sequence` (gauge, set on every
successful ingest), `distant_signal_schedule_feed_scan_duration_seconds`
(histogram, optional) — via `common::metrics`, matching every other
crate's existing metric-naming convention (`distant_signal_<crate>_*`, check
an existing poller's `main.rs` for the exact prefix macro/helper before
inventing a new one).

- [x] **Step 4: Integration-shaped tests**

Since the full loop needs both a real filesystem and a real (or mocked)
`api`, keep true end-to-end coverage to Task 10's live verification. Here,
add `#[test]`s (no `#[ignore]` needed — pure `tempdir` I/O, no network) for
the orchestration helper functions that don't need a live `api` call: "a
`tempdir` seeded with a complete, stable 9-file delivery matching the real
manifest's filename shape is correctly identified as ready to move," "a
`tempdir` missing one manifest-listed file is correctly identified as NOT
ready," "retention pruning keeps exactly the N highest-numbered
subdirectories of `storage_dir` and removes the rest." Mock the `api` HTTP
call behind a trait or a `Option<reqwest::Client>` seam if that keeps these
tests network-free; if the codebase has no existing precedent for mocking
an outbound HTTP call in a poller crate (check first), it is acceptable to
leave the full request/response round-trip to Task 10's live check only —
state which approach was actually taken.

- [x] **Step 5: Run the crate's test suite**

Run: `cargo test -p schedule-ingest`
Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add crates/schedule-ingest/src/main.rs
git commit -m "Implement schedule-ingest's check-times scan loop, api POST, and retention pruning"
```

---

### Task 6: SSH host-key generation and Secret delivery (Helm)

**Files:**
- Modify: `charts/distant-signal/templates/_helpers.tpl`
- Create: `charts/distant-signal/templates/schedulefeed-secret.yaml`

**Interfaces:**
- Produces: `distant-signal.scheduleFeedFullname`,
  `distant-signal.scheduleFeedSecretName`,
  `distant-signal.scheduleFeedHostKeySecretKey`,
  `distant-signal.scheduleFeedDtdPasswordSecretKey`,
  `distant-signal.scheduleFeedDtdPublicKeySecretKey`; the `Secret` object.
- Consumed by: Task 8 (`schedulefeed-deployment.yaml` mounts the host-key
  material; env vars reference the DTD-credential keys).

Not gated by Task 1 for the host-key half (this app generates its own keys
regardless). The DTD-credential half of this Secret renders an **empty**
value when `scheduleFeed.sftp.password`/`publicKey` are unset — same
"rendered but never generated" posture as `pollers.*.apiKey` — so this task
is not gated either, but a *real* credential value is (per Task 1).

- [x] **Step 1: Naming helpers**, following `devAuthentikFullname`'s exact
  pattern in `_helpers.tpl`:

```
{{- define "distant-signal.scheduleFeedFullname" -}}
{{- printf "%s-schedulefeed" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "distant-signal.scheduleFeedSecretName" -}}
{{- printf "%s-schedulefeed" (include "distant-signal.secretName" .) }}
{{- end }}
```

- [x] **Step 2: `schedulefeed-secret.yaml`**

```yaml
{{- if .Values.scheduleFeed.enabled }}
{{- $secretName := include "distant-signal.scheduleFeedSecretName" . -}}
{{/*
Host key: THIS APP generates and owns it -- inverted from the (now-
superseded) pull design's "verify DTD's host key" problem. Uses the SAME
lookup-preserve pattern secret.yaml/devauthentik-secret.yaml already use,
so a `helm upgrade` never silently rotates the key out from under an
already-DTD-trusted fingerprint.

UNVERIFIED against this chart's Helm-4 lookup-preserve pattern in either
predecessor document -- genPrivateKey is the plausible Sprig mechanism;
Task 6's own Step 3 below is where this actually gets checked, not assumed.

DTD account credentials (password / public key) are NEVER generated here --
same rule as pollers.*.apiKey. Left empty until Task 1 resolves what DTD's
push client actually needs and an operator supplies a real value.
*/}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace $secretName -}}
{{- $existingData := ternary $existing.data (dict) (not (empty $existing)) -}}
{{- $hostKey := ternary (get $existingData (include "distant-signal.scheduleFeedHostKeySecretKey" .) | b64dec) (genPrivateKey "ed25519") (hasKey $existingData (include "distant-signal.scheduleFeedHostKeySecretKey" .)) -}}
apiVersion: v1
kind: Secret
metadata:
  name: {{ $secretName }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
type: Opaque
stringData:
  ssh_host_ed25519_key: {{ $hostKey | quote }}
  {{- if .Values.scheduleFeed.sftp.password }}
  schedule-sftp-password: {{ .Values.scheduleFeed.sftp.password | quote }}
  {{- end }}
  {{- if .Values.scheduleFeed.sftp.publicKey }}
  schedule-sftp-dtd-public-key: {{ .Values.scheduleFeed.sftp.publicKey | quote }}
  {{- end }}
{{- end }}
```

**This exact `lookup`/`genPrivateKey`/`ternary` combination is a sketch,
not verified Helm syntax** — Step 3 below is where it actually gets
run through `helm template`/`helm lint` for real; if it errors, fix the
Sprig call shape (the goal — generate once, preserve across upgrades — is
fixed, the exact function chain is not) and record what changed.

- [x] **Step 3: Verify the lookup-preserve mechanism actually renders**

```bash
helm lint charts/distant-signal --set scheduleFeed.enabled=true
helm template charts/distant-signal --set scheduleFeed.enabled=true --show-only templates/schedulefeed-secret.yaml
```

Expected: a `Secret` with a non-empty `ssh_host_ed25519_key`. **Report the
actual outcome plainly** (per this plan's "What this plan is not" section)
— if `genPrivateKey` or the `lookup`/`ternary` chain errors, this is exactly
the push design doc's Open question 6 being hit for real; fix it and note
the working form here, do not silently paper over a failure by hand-waving
the template.

- [x] **Step 4: Commit**

```bash
git add charts/distant-signal/templates/_helpers.tpl charts/distant-signal/templates/schedulefeed-secret.yaml
git commit -m "Add schedule-feed SSH host-key generation and DTD-credential Secret delivery"
```

---

### Task 7: `values.yaml` — `scheduleFeed` block

**Files:**
- Modify: `charts/distant-signal/values.yaml`

**Interfaces:**
- Produces:
  `scheduleFeed.{enabled,sftp,ingest,service,persistence,logLevel,resources,nodeSelector,tolerations,affinity,podAnnotations,podSecurityContext}`.
  Consumed by every remaining Helm task.

Not gated by Task 1 for the block's *shape*; `sftp.username`/`authMethod`/
`password`/`publicKey` stay at their empty/no-default placeholders until
Task 1 resolves what a real value should be.

- [x] **Step 1: Add the block**, following the push design doc's own sketch,
  reconciled with this chart's actual conventions confirmed above
  (`postgresql.persistence`'s exact field names for the PVC block,
  `pollers.*.existingSecret`/`existingSecretApiKeyKey` shape for the
  credential escape hatch):

```yaml
# ---------------------------------------------------------------------------
# scheduleFeed -- OPT-IN CIF SCHEDULE feed SFTP-push receiver
# (docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md)
# ---------------------------------------------------------------------------
# Renders ONE Deployment with TWO containers (an SFTP server and this app's
# own schedule-ingest verifier) sharing one ReadWriteOnce PVC -- see the
# design doc's "The reader/writer problem push introduces" for why this is
# one Pod, not two. Off by default: enabling this without a real DTD push
# account configured (see docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md
# Task 1) stands up infrastructure nothing can reach yet, which is a
# legitimate thing to do ahead of time but not silently assumed to be "done."
scheduleFeed:
  enabled: false
  sftp:
    image:
      repository: drakkan/sftpgo
      tag: ""
      pullPolicy: IfNotPresent
    # -- SFTPGo's own default container port -- not independently
    # re-verified against a real deployed instance by this plan.
    port: 2022
    # -- DTD's push account username on THIS app's server. Required when
    # enabled; no default possible. See Task 1 -- unconfirmed whether DTD's
    # push client even needs a value here at all until push-side
    # configuration mechanics are known.
    username: ""
    # -- "password" or "public-key". NO DEFAULT: RSPS5046 states neither
    # mechanism for either delivery direction (design doc's own primary-
    # source re-check). Enabling without setting this aborts rendering.
    authMethod: ""
    # -- Used when authMethod=password. NEVER auto-generated -- same rule as
    # pollers.*.apiKey. Rendered into the chart Secret when existingSecret
    # is empty.
    password: ""
    # -- DTD's public key, used when authMethod=public-key AND DTD's push
    # configuration turns out to support supplying one (unconfirmed).
    publicKey: ""
    existingSecret: ""
    existingSecretPasswordKey: schedule-sftp-password
    existingSecretPublicKeyKey: schedule-sftp-dtd-public-key
    # -- This app's OWN generated SSH host key, NOT DTD's -- inverted from
    # the (superseded) pull design's dtd_sftp_host_key_fingerprint. Leave
    # empty to have the chart generate and preserve one (Task 6).
    existingSecretHostKey: ""
  ingest:
    image:
      repository: distant-signal/schedule-ingest
      tag: ""
      pullPolicy: IfNotPresent
    checkTimes: "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
    retentionKeepSequences: 2
    stabilityCycles: 2
  service:
    # -- LoadBalancer or NodePort + an operator-managed external LB/DNS --
    # NOT an Ingress (HTTP(S)-only by spec, cannot carry SFTP). Actually
    # exposing this to DTD is gated behind Task 1's outcome, per this
    # plan's own Task 1 -- rendering/lint-checking it is not.
    type: LoadBalancer
    annotations: {}
  persistence:
    enabled: true
    size: 5Gi
    storageClass: ""
    accessModes:
      - ReadWriteOnce
    existingClaim: ""
  logLevel: info
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

- [x] **Step 2: Confirm the chart still renders unaffected with the new
  block off**

```bash
helm lint charts/distant-signal
helm template charts/distant-signal >/dev/null
```
Expected: both succeed — `scheduleFeed.enabled` defaults `false` and
nothing reads it yet (Task 8 wires the actual templates).

- [x] **Step 3: Commit**

```bash
git add charts/distant-signal/values.yaml
git commit -m "Add the scheduleFeed values.yaml block"
```

---

### Task 8: Helm templates — PVC, two-container Deployment, Service, NOTES.txt, NetworkPolicy/PodMonitor wiring

**Files:**
- Create: `charts/distant-signal/templates/schedulefeed-pvc.yaml`
- Create: `charts/distant-signal/templates/schedulefeed-deployment.yaml`
- Create: `charts/distant-signal/templates/schedulefeed-service.yaml`
- Modify: `charts/distant-signal/templates/networkpolicy.yaml`
- Modify: `charts/distant-signal/templates/podmonitor.yaml`
- Modify: `charts/distant-signal/templates/NOTES.txt`

**Interfaces:**
- Depends on Tasks 6-7 (secret/values helpers).
- Produces: the actual rendered Kubernetes objects for this subsystem.

The Deployment/Service shape is gated by Task 1 only for **real external
reachability** (an actual internet-facing `LoadBalancer` DTD is expected to
reach) — rendering and lint-checking the resources is not gated; every
other opt-in subsystem in this chart (every RDM poller, `devAuthentik`)
ships fully render-able while its own external endpoint is still
unconfirmed, and this follows the same posture.

- [x] **Step 1: `schedulefeed-pvc.yaml`** — standalone PVC (not a
  `volumeClaimTemplate`, matching the pull design doc's own reasoning,
  reused: "this crate is a Deployment-shaped singleton... there is no
  per-replica identity to preserve"), `ReadWriteOnce`, mounted by both
  containers of the Deployment below:

```yaml
{{- if and .Values.scheduleFeed.enabled .Values.scheduleFeed.persistence.enabled (not .Values.scheduleFeed.persistence.existingClaim) }}
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ include "distant-signal.scheduleFeedFullname" . }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
spec:
  accessModes:
    {{- toYaml .Values.scheduleFeed.persistence.accessModes | nindent 4 }}
  {{- with .Values.scheduleFeed.persistence.storageClass }}
  storageClassName: {{ . }}
  {{- end }}
  resources:
    requests:
      storage: {{ .Values.scheduleFeed.persistence.size }}
{{- end }}
```

- [x] **Step 2: `schedulefeed-deployment.yaml`** — **one Deployment,
  `replicas: 1` fixed, `strategy: Recreate`, two containers.** This is the
  first multi-container Deployment in this chart — say so in a comment,
  matching the push design doc's own framing ("a genuine departure... worth
  flagging plainly as a first for this chart's own conventions").

```yaml
{{- if .Values.scheduleFeed.enabled }}
{{- if not .Values.scheduleFeed.sftp.authMethod }}
{{- fail "scheduleFeed.sftp.authMethod must be set (\"password\" or \"public-key\") when scheduleFeed.enabled -- see docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md, Credentials" }}
{{- end }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "distant-signal.scheduleFeedFullname" . }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "schedulefeed") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 8 }}
    spec:
      serviceAccountName: {{ include "distant-signal.serviceAccountName" . }}
      automountServiceAccountToken: false
      securityContext:
        {{- include "distant-signal.podSecurityContext" (dict "root" . "override" .Values.scheduleFeed.podSecurityContext) | nindent 8 }}
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: {{ default (include "distant-signal.scheduleFeedFullname" .) .Values.scheduleFeed.persistence.existingClaim }}
        - name: host-key
          secret:
            secretName: {{ default (include "distant-signal.scheduleFeedSecretName" .) .Values.scheduleFeed.sftp.existingSecretHostKey }}
            items:
              - key: ssh_host_ed25519_key
                path: ssh_host_ed25519_key
                mode: 0600
      containers:
        - name: sftp
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.scheduleFeed.sftp.image) | quote }}
          imagePullPolicy: {{ .Values.scheduleFeed.sftp.image.pullPolicy }}
          ports:
            - name: sftp
              containerPort: {{ .Values.scheduleFeed.sftp.port }}
          # SFTPGo's exact env-var wiring for a single chrooted virtual user
          # plus a mounted host key is SKETCHED, NOT independently verified
          # against a real deployed image in this pass (Open question 5) --
          # confirm SFTPGo's documented config format at implementation time
          # and adjust rather than assuming this env block is exact.
          env:
            - name: SFTPGO_SFTPD__BINDINGS__0__PORT
              value: {{ .Values.scheduleFeed.sftp.port | quote }}
          volumeMounts:
            - name: data
              mountPath: /data/schedule-feed
            - name: host-key
              mountPath: /srv/sftpgo/host_keys
              readOnly: true
        - name: ingest
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.scheduleFeed.ingest.image) | quote }}
          imagePullPolicy: {{ .Values.scheduleFeed.ingest.image.pullPolicy }}
          ports:
            - name: metrics
              containerPort: 9091
          env:
            - name: WATCH_DIR
              value: /data/schedule-feed/incoming
            - name: STORAGE_DIR
              value: /data/schedule-feed
            - name: CHECK_TIMES
              value: {{ .Values.scheduleFeed.ingest.checkTimes | quote }}
            - name: RETENTION_KEEP_SEQUENCES
              value: {{ .Values.scheduleFeed.ingest.retentionKeepSequences | quote }}
            - name: STABILITY_CYCLES
              value: {{ .Values.scheduleFeed.ingest.stabilityCycles | quote }}
            - name: API_INGEST_URL
              value: {{ printf "%s/private/schedule-feed-ingests" (include "distant-signal.apiBaseUrl" .) }}
            - name: INTERNAL_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.internalTokenSecretName" . }}
                  key: {{ include "distant-signal.internalTokenSecretKey" . }}
            - name: RUST_LOG
              value: {{ .Values.scheduleFeed.logLevel | quote }}
          volumeMounts:
            - name: data
              mountPath: /data/schedule-feed
{{- end }}
```

- [x] **Step 3: `schedulefeed-service.yaml`** — sibling to `ingress.yaml`,
  not an extension of it:

```yaml
{{- if .Values.scheduleFeed.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "distant-signal.scheduleFeedFullname" . }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
  {{- with .Values.scheduleFeed.service.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
spec:
  type: {{ .Values.scheduleFeed.service.type }}
  selector:
    {{- include "distant-signal.selectorLabels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
  ports:
    - name: sftp
      port: {{ .Values.scheduleFeed.sftp.port }}
      targetPort: sftp
{{- end }}
```

- [x] **Step 4: `networkpolicy.yaml` addition** — only the **in-cluster**
  metrics-scrape allow, matching every poller's existing block; per the
  design doc, inbound SFTP-from-the-internet is entirely outside what
  `NetworkPolicy` can express (cloud LB/security-group layer, a manual
  operator step, not this chart's job):

```yaml
{{- if .Values.scheduleFeed.enabled }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "distant-signal.scheduleFeedFullname" . }}-metrics
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "schedulefeed") | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "schedulefeed") | nindent 6 }}
  policyTypes:
    - Ingress
  ingress:
    - from:
        - namespaceSelector: {}
      ports:
        - protocol: TCP
          port: 9091
{{- end }}
```

(Match whatever the existing poller blocks' `from:` selector actually says
— `namespaceSelector: {}` above is illustrative; copy the real
monitoring-namespace-scoped selector those blocks use once read directly,
rather than inventing a broader one.)

- [x] **Step 5: `podmonitor.yaml` addition** — add `schedulefeed` to the
  `matchExpressions` `values:` list alongside `api`/`aggregator`/
  `enricher`/the per-poller entries, gated the same way (`if
  .Values.scheduleFeed.enabled`, following the existing `range
  .Values.pollers` conditional pattern for the per-poller entries).

- [x] **Step 6: `NOTES.txt` addition** — surfaces this app's own generated
  host-key fingerprint at install time, inverted from the pull design's
  equivalent (which would have surfaced DTD's), per the push design doc's
  own "`NOTES.txt` / documentation touch" section:

```
{{- if .Values.scheduleFeed.enabled }}
schedule-feed's SFTP server generated a new host key on first install.
Its fingerprint must be communicated to DTD (via whatever channel push
configuration turns out to require -- see
docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md Task 1)
so DTD's push client can trust it:
  kubectl get secret {{ include "distant-signal.scheduleFeedSecretName" . }} -n {{ .Release.Namespace }} -o jsonpath='{.data.ssh_host_ed25519_key}' | base64 -d | ssh-keygen -lf /dev/stdin
{{- end }}
```

- [x] **Step 7: Verify rendering**

```bash
helm lint charts/distant-signal --set scheduleFeed.enabled=true --set scheduleFeed.sftp.authMethod=password --set scheduleFeed.sftp.password=x --set scheduleFeed.sftp.username=x
helm template charts/distant-signal --set scheduleFeed.enabled=true --set scheduleFeed.sftp.authMethod=password --set scheduleFeed.sftp.password=x --set scheduleFeed.sftp.username=x >/dev/null
# Confirm the fail-guard actually fires when authMethod is left unset:
helm template charts/distant-signal --set scheduleFeed.enabled=true 2>&1 | grep -q "authMethod must be set" && echo "fail-guard OK"
```
Expected: both `helm lint`/`helm template` succeed with `authMethod` set,
and the fail-guard check prints "fail-guard OK" when it's left unset. If
`kubeconform`/a schema-aware `kubectl` is available in this environment,
also run the rendered manifests through it (Prerequisite 3) — otherwise
state plainly that only the static YAML-shape check ran.

- [x] **Step 8: Commit**

```bash
git add charts/distant-signal/templates/schedulefeed-pvc.yaml charts/distant-signal/templates/schedulefeed-deployment.yaml charts/distant-signal/templates/schedulefeed-service.yaml charts/distant-signal/templates/networkpolicy.yaml charts/distant-signal/templates/podmonitor.yaml charts/distant-signal/templates/NOTES.txt
git commit -m "Render the schedule-feed receiver: PVC, two-container Deployment, Service, NetworkPolicy/PodMonitor/NOTES wiring"
```

---

### Task 9: `docker-compose.yml` wiring and env-file placeholders

**Files:**
- Modify: `docker-compose.yml`
- Modify: `local.env.example`
- Modify: `dev.env.example`

**Interfaces:**
- Produces: `schedule-sftp`, `schedule-ingest` services, the
  `schedule_feed_data` named volume, following this file's existing
  per-service comment/environment conventions (`:?`-guarded required vars,
  `:-default` optional ones).

Not gated by Task 1 — a developer can bring up this pipeline locally and
manually drop a sample delivery into the SFTP container regardless of
whether a real DTD connection will ever exist, exactly like every
`RDM_*_BASE_URL=*.example.invalid` placeholder already lets the four RDM
pollers run against nothing real.

- [x] **Step 1: Add the two services**, adapted from the push design doc's
  own sketch (marked there "sketch — not final") reconciled with this
  file's real conventions (`restart: unless-stopped`, `depends_on` with
  `condition: service_healthy` where a healthcheck exists, the `:?`-guard
  style `SSO_ISSUER_URL`/`TFL_APP_KEY` already use for values with no safe
  default):

```yaml
  schedule-sftp:
    image: drakkan/sftpgo:latest   # tag pinning is an implementation-time
                                    # decision, not sketched further here
    restart: unless-stopped
    ports:
      - "${SCHEDULE_SFTP_PORT:-2222}:2022"
    volumes:
      - schedule_feed_data:/data/schedule-feed
      - ./schedule-sftp-host-keys:/srv/sftpgo/host_keys:ro
    environment:
      SFTPGO_SFTPD__BINDINGS__0__PORT: "2022"
      # Real credentials for DTD's push client -- placeholder here, matching
      # this repo's *.env.example convention for feeds with no confirmed
      # endpoint yet (see Task 1). Never auto-generated -- see Credentials
      # in the push design doc.
      SCHEDULE_SFTP_USERNAME: ${SCHEDULE_SFTP_USERNAME:?SCHEDULE_SFTP_USERNAME must be set once DTD's push account details are known -- see docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md Task 1}

  schedule-ingest:
    build:
      context: .
      dockerfile: docker/schedule-ingest.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
      schedule-sftp:
        condition: service_started
    environment:
      WATCH_DIR: /data/schedule-feed/incoming
      STORAGE_DIR: /data/schedule-feed
      CHECK_TIMES: ${SCHEDULE_FEED_CHECK_TIMES:-22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00}
      RETENTION_KEEP_SEQUENCES: ${RETENTION_KEEP_SEQUENCES:-2}
      STABILITY_CYCLES: ${SCHEDULE_FEED_STABILITY_CYCLES:-2}
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      RUST_LOG: ${RUST_LOG:-info}
      API_INGEST_URL: http://api:8080/private/schedule-feed-ingests
    volumes:
      - schedule_feed_data:/data/schedule-feed
```

Add `schedule_feed_data:` to the top-level `volumes:` block. Note the
push design doc's own caveat, reused here: **compose has no native
"two containers, one Pod" primitive** — two services sharing one named
volume is the closest local-dev equivalent, not a literal reproduction of
the Helm shape's single Pod.

- [x] **Step 2: Local dev host-key generation**

Since `schedule-sftp` bind-mounts a host-key directory from the host
filesystem (unlike the Helm path's chart-generated Secret), document a
one-time local setup step in `local.env.example`'s/`dev.env.example`'s
header comment area, e.g.:
```
mkdir -p schedule-sftp-host-keys
ssh-keygen -t ed25519 -f schedule-sftp-host-keys/ssh_host_ed25519_key -N ""
```
and confirm `schedule-sftp-host-keys/` is covered by `.gitignore` (add an
entry if it is not already covered by an existing broad pattern) — a
developer's local dev host key must never be committed.

- [x] **Step 3: `*.env.example` placeholders**

Add a commented section to both `local.env.example` and `dev.env.example`,
following the existing `RDM_*_BASE_URL`/`TFL_APP_KEY` placeholder-with-
explanation convention:
```
# CIF SCHEDULE feed (SFTP push) -- see
# docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md and
# docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md.
# GAP: whether push-side configuration is even reachable for a normal Data
# Recipient is still an open question (see the plan's Task 1) -- this
# placeholder has no real DTD account behind it yet.
SCHEDULE_SFTP_USERNAME=changeme-once-dtd-account-details-are-known
```

- [x] **Step 4: Static config-resolution check**

```bash
docker compose --env-file local.env config --quiet
docker compose --env-file dev.env config --quiet
```
Expected: both exit `0` once the new `:?`-guarded var is present in both
example files' resulting env (copy each `*.env.example` to a scratch
`*.env` for this check, matching this repo's existing verification
convention — never commit the scratch copy).

- [x] **Step 5: Commit**

```bash
git add docker-compose.yml local.env.example dev.env.example .gitignore
git commit -m "Wire schedule-sftp and schedule-ingest into docker-compose.yml"
```

---

### Task 10: Local end-to-end verification against a synthetic sample delivery

**Files:** none (verification only), plus a throwaway local fixture
directory this task creates and deletes, never committed.

**Requires real infrastructure: Docker plus network access to pull
`drakkan/sftpgo` and build the new `schedule-ingest` image.** If
unavailable in this environment, report every live step below as **not
run**, not assumed to pass — matching the dev-oidc-server plan's Task 4
precedent for this exact situation.

**This does not and cannot exercise a real DTD connection** — it exercises
the receiving/verifying pipeline this app fully controls, using a small
synthetic delivery (not `timetable_full.zip` — see Global Constraints).

**2026-09-01 status: Steps 2-5 (live compose) NOT RUN — no Docker in this
environment.** Confirmed via `which docker podman docker-compose` (all
absent) and `docker version` (`command not found`) — this sandbox has
`helm` but no container runtime at all, not even a daemon-less one, so
Task 9's compose services can't actually be built/started here. Per this
task's own instruction and the dev-oidc-server plan's Task 4 precedent,
this is reported as **not run**, not assumed to pass.

**Step 1 (build a synthetic sample delivery) was performed**, in a scratch
directory outside the repo (`/tmp/...`, deleted immediately after, never
committed): 9 files named `RJTTF999{DAT,ZTR,REJ,SET,FLF,MCA,MSN,ALF,TSI}.txt`,
the `.DAT` file containing a hand-written manifest reproducing the real
format exactly (header/footer `/!!` lines, `Sequence: 999`, the 8 sibling
filenames), the other 8 with trivial placeholder content. This is
substantively the same shape Task 5's own unit tests already build inside a
`tempfile::tempdir()` (real `std::fs` I/O against a real directory on disk,
not a mock) — `complete_stable_nine_file_delivery_is_ready_to_move` and
`delivery_missing_one_listed_file_is_not_ready` in
`crates/schedule-ingest/src/main.rs` already exercise exactly this
scenario end-to-end through `scan_incoming`/`StabilityTracker`/
`missing_listed_files`, so the scratch-directory build here added no
additional logic coverage beyond confirming the manifest content template
is byte-correct — it was deleted rather than kept as a redundant fixture.

**Steps 2-5, concretely not run**: bringing up `docker compose ... schedule-sftp
schedule-ingest api postgres`, copying the synthetic delivery into the SFTP
container's `incoming/`, observing a logged ingest and a `GET
/private/schedule-feed-ingests` response reflecting sequence 999, repeating
with sequences 1000/1001 to confirm retention pruning leaves only the two
newest directories, and tearing the stack down. **None of this has been
exercised in any session** — the pipeline's real HTTP POST/GET round-trip
against a live `api`, and the SFTPGo container's actual behavior, remain
unverified beyond the static `helm template`/`docker compose config`-style
checks Tasks 8-9 already ran. Whoever next has Docker access should run
these steps for real before treating this pipeline as proven end-to-end.

- [x] **Step 1: Build a synthetic sample delivery**

Create 9 small files reproducing the real filenames
(`RJTTF999DAT.txt`...`RJTTF999MSN.txt`, an arbitrary unused sequence
number) with trivial placeholder content, plus a `RJTTF999DAT.txt` manifest
matching the real format confirmed in Task 3 (header/footer `/!!` lines,
`Sequence: 999`, the 8 sibling filenames). This is what a developer would
manually `scp`/copy into the SFTP container's chroot — the push design doc
explicitly defers designing a real seed script; this step is that manual
substitute, done once for this verification.

- [ ] **Step 2: Bring up the stack**

```bash
docker compose --env-file dev.env up -d --build schedule-sftp schedule-ingest api postgres
```
Expected: all report running/healthy.

- [ ] **Step 3: Deliver the synthetic sample and observe ingestion**

Copy the 9 files from Step 1 into `schedule-sftp`'s mounted `incoming/`
directory (via `docker compose cp` or directly onto the named volume).
Wait through `stability_cycles` polling cycles (shorten `CHECK_TIMES`/add a
short-poll override for this test if the crate's real loop only checks at
fixed daily times — note whether `schedule-ingest`'s design as built
supports an accelerated test cadence, and if not, record that as a real
testability gap worth a follow-up, not something this task silently works
around by editing production defaults).

```bash
docker compose --env-file dev.env logs schedule-ingest | grep -i "ingest\|sequence"
curl -s http://localhost:8080/private/schedule-feed-ingests -H "X-Internal-Token: $INTERNAL_TOKEN"
```
Expected: a log line confirming sequence 999 was ingested, and the `GET`
response reflecting it.

- [ ] **Step 4: Confirm retention pruning**

Repeat Steps 1/3 with sequence `1000`, then `1001` (`retention_keep_sequences`
default 2): confirm `storage_dir` on the shared volume ends up with only
`1000/` and `1001/`, not `999/`.

- [ ] **Step 5: Tear down**

```bash
docker compose --env-file dev.env down -v
rm -rf <the local synthetic-delivery scratch directory from Step 1>
```

- [ ] **Step 6: No commit for this task**

Verification-only; nothing new to commit. If any live step could not run
(no Docker/network access), state that plainly rather than proceeding as if
the pipeline were proven end to end when it wasn't.

---

## Summary of what remains open after this plan

Even fully executed, this plan does **not** produce a working DTD
connection — it produces a fully built, fully render-able, locally-testable
receiving pipeline waiting for one. The two items that gate going live for
real, restated plainly: **Task 1's outcome** (is push-side configuration
reachable at all, and by what channel), and, once that's known, an operator
actually communicating this app's generated host-key fingerprint (surfaced
by Task 8's `NOTES.txt`) and a real destination address/credential to DTD —
neither of which any task in this plan can perform on its own.
