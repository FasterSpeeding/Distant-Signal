# Schedule-Feed Ingestion via SFTP Pull — Design

**Status: design/proposal, not an approved implementation plan.** Written to
the same rigor and structure as
`docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` (this repo's
closest precedent for "a real, later-implemented infrastructure design for a
new backing service"): concrete `docker-compose.yml`/Helm sketches marked
"sketch — not final," every external claim attributed to a real source, and
nothing here is committed code. **This document does not contain any Rust
code, Dockerfile, or Helm template — sketches only.** It also does not touch
`crates/trust-consumer` in any way: that crate's STANOX↔CRS matching gap is
being fixed in a separate, currently in-flight worktree, and nothing in this
design depends on, blocks, or edits that work.

The decision this document takes as given, not re-derives: **SFTP pull**,
against DTD's documented `dtd.atocrsp.org` endpoint, is how this app will
receive the CIF Timetable feed — not SFTP push (this app running its own
internet-facing SFTP daemon) and not a cloud-storage-bucket push. That
decision, and the research establishing it, live in
`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md` ("the
ingress research doc") — specifically its "Addendum (2026-08-30)" section,
which found that RDG/RSP's own **RSPS5046 "Timetable Information Data Feed
Interface Specification"** (Subject Ref RSPS5046, Version P-04-02,
03-Jun-2025, fetched and read in full twice via two independent URLs in that
research pass) documents SFTP pull from `dtd.atocrsp.org` as a first-class,
RDG-supported delivery mechanism for this exact product family, requiring no
inbound-facing daemon on this app's side at all. Read that document's
Addendum §1, §3, §5, and §7 before this one if you haven't already — this
document assumes those findings as settled and does not repeat their
citation trail in full.

This document also assumes, without repeating, the context in
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
(why locally-available CIF schedule data matters — segment-level status
precision, TRUST-vs-schedule delay inference) and
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
(the real, live validation status: SSO now works end-to-end on the live
deployment; `trust-consumer`'s STANOX→CRS gap is a separate, currently-blocking
issue for the *consumption* side of this work, being fixed elsewhere, not
this document's concern).

## Problem

Today this codebase has no CIF schedule data anywhere. `trust-consumer`
receives real TRUST movement events (STANOX-keyed), but has nothing to
compare them against — no planned timetable, no calling-point list, no way
to know "was this train supposed to be here, and when." The delay-inference
design doc's whole case for proceeding rests on two things CIF schedule data
would unlock that today's LDBWS-sampling approach structurally cannot
deliver: **coverage** (every scheduled service on a line, not 3-5 sampled
stations) and **segment-level status precision** (which part of a line is
affected, not just a line-wide aggregate). Neither of those is buildable
without the schedule data landing somewhere this app can read it, reliably,
every day.

This document is scoped narrowly, per the brief: **get the daily full-refresh
files onto disk (and their arrival recorded in a database) reliably** — not
implementing the delay-inference logic that would consume them, and not
touching the crate (`trust-consumer`) whose own separate STANOX/CRS gap is
being fixed in parallel. Those are both explicitly later, separate work.

## Goals

- Reliably pull DTD's **"Timetable - Full Refresh - Daily"** feed
  (RDM product `P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`, per the ingress
  research doc's Addendum §2) via SFTP, once per day, on a schedule that
  fits RSPS5046's documented delivery window.
- Land the delivery's files on persistent storage, verifying the manifest
  (`RJTTFnnn.DAT`) lists every file it names as actually present and fully
  transferred before treating a delivery as usable.
- Record each successful ingest (sequence number, timestamp, file
  count/sizes) in a database, reusing this app's existing
  poller-freshness conventions, so `/public/freshness` (and any future
  consumer) can answer "how current is the local schedule data" without
  reading the filesystem directly.
- A sane retention/cleanup policy: given the ~711MB uncompressed size anchor
  (the ingress research doc's §5/Addendum §3) and a daily cadence, the
  storage volume must not grow without bound.
- Detect and loudly surface a sequence-number gap or an unexpected
  delivery shape, per RSPS5046 §7.4's documented resumption behaviour —
  "don't silently proceed as if nothing happened."
- A new crate, following this repo's `poller-*` conventions (config
  loading via `clap`, structured logging, Prometheus metrics, the same
  `common::ingest` freshness contract), adapted where a daily pull
  genuinely needs different scheduling semantics than a 30-300s poll loop.
- Concrete `docker-compose.yml` and Helm chart sketches, following this
  chart's own established conventions (`devAuthentik.*`'s opt-in-subsystem
  pattern, `pollers.*`'s per-component values shape, the existing
  Secret/PVC precedents) rather than inventing new ones.

## Non-goals

- **Implementing the delay-inference logic that consumes this data** — the
  TRUST-vs-schedule diffing, segment-level status computation, and the
  `trust-line-aggregator`-shaped Option B service the delay-inference design
  doc discusses. This document's contract ends at "the files for a given
  sequence are on disk, verified complete, and their arrival is recorded" —
  what happens after that is separate, later work, gated on its own design
  pass.
- **Touching `crates/trust-consumer`** in any way. Its STANOX→CRS matching
  gap is being fixed in a separate, currently in-flight worktree. Nothing
  here edits, depends on the internals of, or creates a merge surface with
  that crate.
- **Re-litigating whether to build this at all.** The delay-inference design
  doc's "proceed with caveats, not yet" verdict and the validation
  findings' favourable licensing conclusion both already stand; this
  document assumes "if/when this proceeds" and designs the concrete
  ingestion mechanism for that case.
- **Designing the SFTP-push or cloud-bucket variants.** Those are the
  ingress research doc's job, already done; this document is pull-only.
- **How a future downstream consumer reads these landed files back out** —
  whether that's a sidecar in the same Pod, a `ReadWriteMany`-capable PVC,
  or this crate eventually parsing CIF content directly into Postgres
  itself instead of leaving raw files for another pod to read. This is a
  real architectural question a future Option-B design will need to
  answer, flagged here only so it isn't forgotten — not resolved by this
  document, whose PVC is designed around a single writer (this crate) with
  no reader contract assumed yet.
- **Adopting a cron-expression-parsing crate** (e.g. `cron`,
  `tokio-cron-scheduler`) as a firm decision. A hand-rolled "list of check
  times" is sketched below as sufficient for this feed's cadence; whether
  to reach for a dedicated scheduling crate instead is an implementation-time
  call, not decided here.
- **Any Rust code, Dockerfile, or Helm template.** Everything below marked
  "sketch" is illustrative only.
- **Resolving the open questions in the dedicated section below.** They are
  real, named gaps this document could not close without DTD portal access
  the repo owner does not currently have (application pending) — see that
  section for exactly what's blocked, why, and what it means for this
  design if it resolves one way vs. another.

## Research recap (see the ingress research doc for the full trail)

Facts this design leans on directly, each already sourced in the ingress
research doc's Addendum:

- **Hostname**: `dtd.atocrsp.org`, RSPS5046 §7.1.2 — a real, stable,
  already-published hostname. Unlike every `RDM_*_BASE_URL` placeholder
  elsewhere in this repo's `.env.example` files, this one is not a
  guess-until-confirmed value.
- **Delivery window**: "around 10.30pm to 1am" normally (§7.3.1), with a
  worst-case fallback "Empty" feed (previous full refresh resent, empty
  update files) delivered by 4pm if DTD is running late (§7.3.2).
- **Manifest format**: a `.DAT` "Contents" file lists every other file in
  the delivery except itself; a real sample (`RJTTF942DAT.txt`) matches
  RSPS5046 §5.2.2's worked example line-for-line, listing 8 sibling files
  (`ZTR`/`REJ`/`SET`/`FLF`/`MCA`/`MSN`/`ALF`/`TSI`, sequence number `942`
  embedded in every filename) plus a trailer line, `/!! End of file (8
  records) (28/08/2026)`.
- **9-file structure and sizes**: `DAT` (618B), `MCA` (707.7MB — the actual
  CIF Basic Schedule data, `HD`/`TI`/`AA`/`BS`/`BX`/`LO`/`LI`/`CR`/`LT`/`ZZ`
  records), `REJ` (246B, empty in the sampled delivery), `ZTR` (2.9MB, bus/
  ferry Quasi-CIF), `SET` (499B, fixed literal `UCFCATE`), `FLF` (101KB,
  human-readable fixed-link text), `ALF` (233KB, machine-parseable fixed
  links), `TSI` (714B, TOC interchange times), `MSN` (340KB, station master
  names). Total: **76,446,640 bytes compressed / 711,352,325 bytes
  uncompressed** across all 9 files in the one real sample obtained.
  `CFA` (the daily-update-only counterpart to `MCA`) is documented but
  **never present** in a Full-Refresh delivery, which is what this app's
  actual licensed product delivers every day (RSPS5046 §7.6/§7.7).
- **Resumption/gap semantics**: RSPS5046 §7.4, quoted directly — "In
  circumstances where one or more 'Empty' feeds have been distributed, DTD
  may need to provide more than one feed in a 24-hour period. This will not
  be done without contacting Data Recipients... Data Recipients that are
  unable to process more than one feed in a 24-hour period would resume
  with a Full Refresh Feed and the sequence number of this Full Refresh
  will not necessarily be contiguous from the last feed sequence." Two
  concrete implications carried into the design below: a sequence-number
  gap is not by itself proof of a missed delivery, and DTD's own practice
  is to contact recipients directly before this happens — a human channel
  exists alongside the automated pull.
- **Bootstrap**: "New Daily Recipients that begin the service will be
  provided with a full refresh of timetable data" (§7.6.1) — the watcher's
  very first pull, whenever it happens to run, should expect (and must
  handle) a full refresh regardless of which day it is.
- **Manifest-completeness is explicitly the recipient's job**: RSPS5046
  §7.2.2 — "the Data Recipient should ensure that all files in the
  manifest file are present... it is the Data Recipients' responsibility to
  process the files according to their requirements." This is not this
  design's own invention; it's the documented contract.
- **Server-side resilience**: §7.5.3 — DTD's own failover preserves "the
  same domain and IP address," relevant to firewalling if this app's
  network posture ever needs to allowlist a destination (moot for outbound
  pull on the app's own side, but potentially relevant to whatever egress
  path a cluster's own NAT/firewall applies — see Open Questions).

## Design

### Why pull changes the shape entirely, restated concretely

The ingress research doc's SFTP-push sections (1 and 4) spent most of their
length on problems that **do not exist for pull at all**: no new Kubernetes
`Service`, no `LoadBalancer`/`NodePort`, no SSH host-key Secret material to
generate/rotate, no "first inbound-facing backend service" security review.
This app is always the calling party — exactly the same posture every
`poller-*` crate and `trust-consumer` already have today, just over SFTP
instead of HTTP or Kafka. The entire component this design adds is an
**outbound** SFTP client, on a schedule, writing to a `PersistentVolumeClaim`
only this app's own workload ever mounts.

### A new crate, not an extension of an existing one

Per `DESIGN.md` §12's "one crate per concern... don't merge them" convention
(already the deciding factor in the delay-inference design doc's own
Option A/B/C architecture analysis), this gets its own crate rather than
being bolted onto `aggregator` or `trust-consumer` — both already-loaded
processes with their own correctness requirements a slow/failing SFTP pull
shouldn't be able to degrade.

**Name: `schedule-ingest`, not `poller-schedule`.** This deliberately breaks
the naming pattern the four RDM pollers plus `poller-tfl`/`poller-ldbws`
share, for a reason worth stating rather than leaving implicit: those five
crates all share one scheduling shape (a tight `tokio::time::interval` loop,
30s-24h) and one wire shape (`common::ingest::post_batch`/
`time_until_next_poll` against a plain REST endpoint). This component shares
the second (see below) but not the first — grouping it under the `poller-`
prefix would visually imply "just another interval poll," which invites
someone to eventually "simplify" its scheduling back toward a tight loop
without understanding why it isn't one. Keeping the name distinct is a small
guardrail against that.

### Configuration shape

Following `crates/poller-ldbws/src/config.rs`'s exact convention — a
`clap::Parser`-derived `Config` struct, every field either a real default or
`#[arg(long, env)]` with no default when guessing would be worse than
failing loudly:

```rust
// sketch — not final
#[derive(Debug, Parser)]
pub struct Config {
    /// DTD's real, published SFTP pull hostname (RSPS5046 S7.1.2). Unlike
    /// every RDM_*_BASE_URL in this repo, this has a real, confirmed
    /// default -- it is not a placeholder.
    #[arg(long, env, default_value = "dtd.atocrsp.org")]
    pub dtd_sftp_host: String,

    /// Standard SFTP port. RSPS5046 does not state DTD uses a non-standard
    /// port; this is an ASSUMPTION (22 is the SFTP/SSH default), not a
    /// confirmed fact -- see Open Questions.
    #[arg(long, env, default_value_t = 22)]
    pub dtd_sftp_port: u16,

    /// Account username, assigned via the DTD Web Portal
    /// (dtdportal.atocrsp.org). No default -- cannot be guessed.
    #[arg(long, env)]
    pub dtd_sftp_username: String,

    /// "password" or "private-key". No default: RSPS5046 documents
    /// NEITHER mechanism (confirmed by full-text search of the whole
    /// 39-page spec, twice, by two independent fetches -- see the ingress
    /// research doc's Addendum SS1/7). Guessing wrong here fails loudly
    /// (missing required arg) rather than silently trying the wrong
    /// mechanism against a real account.
    #[arg(long, env)]
    pub dtd_sftp_auth_method: AuthMethod,

    /// Used when dtd_sftp_auth_method=password.
    #[arg(long, env)]
    pub dtd_sftp_password: Option<String>,

    /// Path to a mounted private key file, used when
    /// dtd_sftp_auth_method=private-key. A FILE PATH, not the key content
    /// itself -- see Credentials below for why.
    #[arg(long, env)]
    pub dtd_sftp_private_key_path: Option<PathBuf>,

    #[arg(long, env)]
    pub dtd_sftp_private_key_passphrase: Option<String>,

    /// Pinned host-key fingerprint, once known. Empty means trust-on-first-
    /// connect with a loud WARN log of the observed fingerprint -- see
    /// "Host-key verification" below. Confirming and pinning this is a
    /// day-one operator task once a real connection is possible.
    #[arg(long, env, default_value = "")]
    pub dtd_sftp_host_key_fingerprint: String,

    /// Where landed files live. Mounted PVC path in both compose and Helm.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// Comma-separated HH:MM times (Europe/London -- see Design's
    /// Scheduling section for why not UTC), checked once each. Defaults
    /// match RSPS5046 SS7.3.1/7.3.2's documented window plus its 4pm
    /// worst-case fallback.
    #[arg(long, env, default_value = "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00")]
    pub check_times: String,

    /// How many complete sequences to retain on disk (current + fallback).
    #[arg(long, env, default_value_t = 2)]
    pub retention_keep_sequences: u32,

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

This deliberately reuses every shared piece already available:
`common::ingest::{INTERNAL_TOKEN_HEADER, post_batch}` for talking to `api`,
`common::metrics::install` for the Prometheus listener, `tracing-subscriber`
for logs. Nothing here is a new cross-cutting pattern; it's the existing
`poller-*` toolkit pointed at a different transport.

### Scheduling: check-times, not a tight interval loop

**Why not copy the 60s-interval loop verbatim**: every existing poller polls
a live, cheap-to-query endpoint on a short cadence because the underlying
data can plausibly change every cycle. This feed does the opposite: DTD
publishes **at most once a day**, in a **documented window** (RSPS5046
§7.3.1/§7.3.2), so polling every 60 seconds around the clock would be almost
entirely wasted connections and (worse) 700MB-class downloads if a naive
implementation re-pulled an unchanged sequence repeatedly.

**Design**: a fixed list of check times (`check_times` above), computed
against a `chrono-tz`-aware `Europe/London` clock rather than naive UTC —
flagged as an assumption, not confirmed: RSPS5046's own "10:30pm to 1am"
phrasing does not state a timezone in the passages already fetched, but
Network Rail/RDG's operations are UK-based, so London local time is the far
more likely reading than UTC. **This is resolvable by re-reading the
already-public RSPS5046 document more closely for an explicit timezone
statement — it needs no DTD portal access, unlike the items in the section
below, so it's flagged here as a follow-up documentation task, not a
portal-blocked open question.** Getting this wrong only matters twice a year
(the London/UTC offset changes at BST transitions); a `chrono-tz`-computed
local time sidesteps the whole class of bug regardless.

At each configured check time, the watcher: connects, lists the remote
directory for the current `RJTTFnnn.DAT` manifest, and either finds a new
sequence (proceeds — see Pull procedure) or finds the same sequence already
ingested (no-op, just records a `checked_at` heartbeat). Between check
times, the process sleeps — no polling at all. The **very first run** on a
freshly-deployed instance bypasses the check-time gate entirely and attempts
an immediate pull, per RSPS5046 §7.6.1's "new recipients get a full refresh
regardless of when they start" — matching this repo's own
`time_until_next_poll` precedent of "no prior fetch recorded → poll now,"
just applied to a first-ever pull instead of a post-restart one.

This is intentionally simple enough to hand-write (`Vec<NaiveTime>`, find
the next one after `now`, `tokio::time::sleep_until`) rather than pulling in
a cron-expression crate — nothing here needs cron's generality (day-of-week
rules, month rules), just "check at these times of day, every day." If
implementation reveals a real need for that generality (e.g. modelling
RSPS5046 §7.7's Wednesday-only weekly/monthly cadences for a *different*
DTD product this app doesn't currently subscribe to), that's a concrete
reason to revisit — not assumed here.

### SFTP client library

No SFTP or SSH client library exists anywhere in this codebase today
(confirmed by the ingress research doc's own grep pass: zero matches for
`sftp|ssh2|russh|libssh` across the whole workspace). Two real candidates,
checked directly against their published crate pages for this design pass:

- **`ssh2`** — Rust bindings to libssh2 (a C library), confirmed via its
  `lib.rs` page: "Rust bindings to libssh2, an ssh client library," current
  version 0.9.6. Its `Sftp` struct (confirmed via `docs.rs`) provides a full
  SFTP client surface: `opendir`/`readdir` (directory listing),
  `open`/`create` (file transfer), `stat`/`lstat` (size/completeness
  checks), `rename`. Mature and widely used, but pulls in a C dependency —
  this workspace already tolerates one C-wrapping dependency
  (`rdkafka`/`librdkafka` in `trust-consumer`), so this would not be a
  first, but it is a real build-toolchain consideration (linking libssh2 in
  every Docker build stage) worth naming.
  [ssh2 on lib.rs](https://lib.rs/crates/ssh2),
  [`Sftp` on docs.rs](https://docs.rs/ssh2/latest/ssh2/struct.Sftp.html)
- **`russh-sftp`** — a pure-Rust SFTP implementation for the `russh` SSH
  stack, confirmed via its `lib.rs` page: "SFTP subsystem supported server
  and client for Russh and more!", explicitly documenting both "Client
  side" support and a client usage example, current version 2.4.0 (released
  2026-08-03 — actively maintained, not stale). Being pure Rust and
  `tokio`-native fits this crate's async runtime (every `poller-*` crate
  already runs on `tokio`) without adding a C toolchain dependency.
  [russh-sftp on lib.rs](https://lib.rs/crates/russh-sftp)

**Recommendation: `russh-sftp`**, for the async-native fit and to avoid
introducing a second C-linked dependency alongside `rdkafka` for a component
that otherwise has no C-toolchain need. This is a leaning, not a hard
verified conclusion — neither crate was exercised against a real SFTP server
in this pass (no real DTD credentials exist to test against yet — see Open
Questions), so an implementer should treat this as a starting point, re-
confirm it handles this workspace's actual TLS/crypto feature-flag choices
cleanly, and switch to `ssh2` without much sunk cost if `russh-sftp`'s
client-side maturity turns out weaker in practice than its docs suggest.

### Host-key verification

SFTP pull means *this app* is the one that must verify *DTD's* host key on
first connect — the mirror image of the SFTP-push design's "generate and
manage our own host keys" problem, and one RSPS5046 doesn't document a
fingerprint for anywhere in its 39 pages (confirmed by the same full-text
search that found no auth-mechanism text — see the ingress research doc's
Addendum §7). Two options, and the design below picks the second as a
pragmatic default while keeping the first available:

1. **Strict, pinned verification** — an operator obtains the real
   fingerprint from DTD (portal, support ticket, or an independent
   `ssh-keyscan`-style check the operator trusts) and sets
   `dtd_sftp_host_key_fingerprint`. The watcher refuses to connect if the
   presented key doesn't match. This is the secure default *once the value
   is known* — but the value isn't known yet (Open Questions).
2. **Trust-on-first-connect (default when the fingerprint is unset)** — on
   first successful connection, log the observed host-key fingerprint at
   `WARN` (not `INFO`) so it's impossible to miss in normal log output, and
   proceed. An operator who wants strict verification going forward copies
   that logged value into `dtd_sftp_host_key_fingerprint` and redeploys.
   This is a documented, honest trade-off (accepting on-path-attacker risk
   for the very first connection) rather than either silently skipping
   verification forever or blocking deployment entirely on a fact this repo
   owner cannot currently obtain.

### Pull procedure and manifest verification

1. Connect and authenticate per `dtd_sftp_auth_method`.
2. List the remote directory (path within the account's own root —
   unconfirmed, see Open Questions; assumed to be the account's home/chroot
   root, matching how the manifest's own worked example names files with no
   path prefix).
3. Locate the current `RJTTFnnn.DAT` manifest, parse its sequence number
   (`nnn`) and the file list it names (matching RSPS5046 §5.2.2's format,
   confirmed against the one real sample already obtained by prior
   research).
4. Compare `nnn` against the last successfully-ingested sequence, recorded
   via `api`'s ingest-metadata endpoint (see Storage/database bookkeeping
   below) — **not** a local marker file, so this state survives a Pod
   reschedule onto a fresh PVC-less node the same way every other poller's
   freshness state already does (via `api`/Postgres, not local disk).
   - `nnn == last`: nothing new. Record a `checked_at` heartbeat, done.
   - `nnn == last + 1`: the expected case. Proceed to step 5.
   - `nnn <= last - 1` or any other non-contiguous value (including
     `nnn > last + 1`, a genuine forward gap): **log at `ERROR` with both
     sequence numbers, increment a
     `distant_signal_schedule_feed_sequence_gap_total` counter, and still
     proceed to ingest the new sequence.** Per RSPS5046 §7.4, a
     non-contiguous sequence number after an "Empty" feed is documented,
     expected behaviour, not proof of a missed delivery — refusing to
     ingest a valid new Full Refresh over a hypothetical gap would leave
     this app silently serving stale schedule data for no protective
     benefit. "Log and alert, don't silently proceed as if nothing
     happened" is satisfied by the loud log + metric, not by refusing the
     new data.
5. Download every file the manifest names into a temporary path
   (`storage_dir/tmp/<nnn>/`), comparing each downloaded file's byte count
   against the SFTP-reported remote size (`stat`) before considering it
   complete — RSPS5046 doesn't document an application-level checksum
   (unconfirmed either way; not found in the passages already fetched), so
   byte-count matching is the completeness signal used here, not a hash.
6. Once **all** manifest-listed files are confirmed complete (RSPS5046
   §7.2.2's own stated recipient responsibility), atomically move
   `tmp/<nnn>/` to `storage_dir/<nnn>/`, and POST the ingest record to
   `api` (sequence, `ingested_at`, per-file byte counts). A delivery that
   is *incomplete* at a given check time is left in `tmp/` and retried at
   the next check time — not treated as a failure until the schedule's
   final check time (16:00, per the fallback default above) passes with
   still no complete manifest, at which point it's logged at `ERROR` as a
   likely real delivery problem worth a human looking at.
7. Prune anything on disk older than `retention_keep_sequences` generations
   (default 2 — current plus one fallback), per the retention policy below.

### Storage and retention

**Layout** (mirroring the ingress research doc's own §5 conclusion,
reused directly rather than re-derived):

```
/data/schedule-feed/
  tmp/<nnn>/            # in-progress downloads, not yet verified complete
  942/                  # a fully-verified, complete sequence
    RJTTF942DAT.txt
    RJTTF942MCA.txt
    ... (9 files total)
  943/
    ...
```

No zip/compression step: RSPS5046's own manifest format (§5.2.2) lists 9
**loose** filenames, not a single archive name, so the assumption here is
that a live SFTP pull presents 9 loose files per delivery, matching the
manifest — not the ~76MB *compressed* figure some of this app's prior
research quotes (that figure describes how the one real sample happened to
be bundled for transport to this research pass, not necessarily DTD's own
SFTP wire format). **This means the realistic per-sequence disk cost is the
uncompressed ~711MB anchor, not the ~76MB compressed one** — worth stating
plainly since it's an easy number to under-size a PVC against if only the
smaller figure is remembered.

**Retention policy**: keep `retention_keep_sequences` (default 2) complete
generations — the current one and one fallback, in case the most recent
sequence turns out to be unparseable by a downstream consumer after the
fact and an operator needs to fall back. Anything older is deleted
immediately after a new sequence is confirmed complete and successfully
recorded via `api`. At ~711MB/generation × 2, plus `tmp/` headroom for one
in-flight download that hasn't yet been confirmed complete, **a 3-4GB PVC
is comfortably sufficient** — the default sketched below (`5Gi`) leaves
real headroom without meaningfully over-provisioning, following the same
"small integer, generous default" instinct as `aggregator`'s
`HISTORY_RETENTION_DAYS`.

**PVC shape**: a plain, pre-declared `PersistentVolumeClaim`
(`ReadWriteOnce`), **not** a `volumeClaimTemplate` on a `StatefulSet` — this
crate is a Deployment-shaped singleton (see below), and the ingress research
doc's own reasoning for the (now-superseded) SFTP-*receiver* design applies
identically here: "a plain pre-declared `PersistentVolumeClaim`... mounted
into that single Deployment is simpler than a StatefulSet for this shape,"
because there is no per-replica identity to preserve — there is exactly one
replica, ever.

### Database bookkeeping — reusing the existing freshness contract

Rather than inventing a new state-tracking mechanism, this design extends
the same contract every RDM poller already uses
(`common::ingest::{LastFetchedResponse, time_until_next_poll, post_batch}`):
a new `api` route, `/private/schedule-feed-ingests`, accepting `POST`s of
`{sequence, ingested_at, files: [{name, bytes}]}` and a `GET` returning the
last recorded `fetched_at` (here: `ingested_at`) in the same
`LastFetchedResponse` shape every other poller's own first-run delay
computation already reads. This is a deliberate reuse, not a new pattern:

- The watcher never touches Postgres directly, matching the existing
  architectural rule (`networkpolicy.yaml`'s own comment: "the pollers
  never do -- they reach the database only indirectly, by POSTing to the
  api's `/private/*` ingest endpoints").
- `/public/freshness`'s `DataFreshness` struct
  (`crates/api/src/routes/freshness.rs`) gains a fifth field,
  `schedule_feed: Option<DateTime<Utc>>`, populated the same way its four
  existing siblings are — a `last_schedule_feed_fetch` query alongside
  `last_stations_fetch`/`last_tocs_fetch`/etc. This is a **sketch of an
  `api`-crate change**, not `trust-consumer` — allowed under this
  document's own non-goals, and small enough to fold into this feature's
  own implementation rather than needing a separate design pass.
- The sequence number itself (not just the timestamp) is also worth
  recording in that same table, since "how current" for this feed is
  better expressed as "sequence 943, ingested at 23:14" than a bare
  timestamp — a `DataFreshness` field could reasonably become a small
  struct (`{ sequence: u32, ingested_at: DateTime<Utc> }`) rather than a
  bare `Option<DateTime<Utc>>`, an implementation-time refinement not
  fully specified here.

### Credentials

Following this chart's existing three-way secrets rule (`values.yaml`'s own
header comment: `existingSecret` set → render nothing; explicit value set →
render it; neither → generate, but *only* for `internalToken` and the
postgres password — never for a real external credential, exactly like
every `pollers.*.apiKey` and `trustConsumer.kafka.sasl*` today):

- **Password auth**: a `dtd-sftp-password` key, rendered into a Secret and
  consumed via a normal `secretKeyRef` env var — identical shape to every
  existing `rdm-*-api-key`.
- **Private-key auth**: deliberately **not** an env var. A multi-line PEM
  blob in an environment variable is fragile (newline handling varies
  across shells/YAML-in-YAML layers, and several of this chart's existing
  env-var-based secrets are flat tokens specifically because they're
  single-line). Instead: a `dtd-sftp-private-key` Secret key, mounted as a
  **file** into the Pod (a `secret` volume, read-only), with
  `dtd_sftp_private_key_path` pointing at the mounted path. This is a
  genuine, deliberate deviation from this chart's usual "everything is an
  env var" convention, justified by the shape of the credential itself, not
  a stylistic choice.
- **Neither is ever auto-generated.** A random SFTP password or keypair
  would be meaningless without DTD's side registering it — identical
  reasoning to why `pollers.*.apiKey` is "rendered (possibly empty) but
  never generated" in `secret.yaml` today.

### `docker-compose.yml` sketch

```yaml
# sketch — not final
services:
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
    environment:
      # crates/schedule-ingest/src/config.rs: Config. Unlike every other
      # RDM_*_BASE_URL placeholder in this file, DTD_SFTP_HOST has a REAL
      # default (dtd.atocrsp.org, RSPS5046 S7.1.2) -- what's still a
      # placeholder is the username/credential, not the host.
      DTD_SFTP_HOST: ${DTD_SFTP_HOST:-dtd.atocrsp.org}
      DTD_SFTP_PORT: ${DTD_SFTP_PORT:-22}
      DTD_SFTP_USERNAME: ${DTD_SFTP_USERNAME:?DTD_SFTP_USERNAME must be set -- see the DTD portal section of local.env.example}
      # GAP, blocked on DTD portal access: which of these two mechanisms
      # applies is UNCONFIRMED -- see the design doc's Open Questions.
      DTD_SFTP_AUTH_METHOD: ${DTD_SFTP_AUTH_METHOD:?DTD_SFTP_AUTH_METHOD must be "password" or "private-key" -- unconfirmed which applies until DTD portal access exists}
      DTD_SFTP_PASSWORD: ${DTD_SFTP_PASSWORD:-}
      DTD_SFTP_PRIVATE_KEY_PATH: ${DTD_SFTP_PRIVATE_KEY_PATH:-}
      STORAGE_DIR: /data/schedule-feed
      RETENTION_KEEP_SEQUENCES: ${RETENTION_KEEP_SEQUENCES:-2}
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      RUST_LOG: ${RUST_LOG:-info}
      API_INGEST_URL: http://api:8080/private/schedule-feed-ingests
    volumes:
      - schedule_feed_data:/data/schedule-feed
      # Only relevant if DTD_SFTP_AUTH_METHOD=private-key -- an operator's
      # own key file, git-ignored, never committed.
      - ${DTD_SFTP_PRIVATE_KEY_HOST_PATH:-/dev/null}:/etc/schedule-ingest/dtd_key:ro

volumes:
  schedule_feed_data:
```

Same fail-fast posture as `trust-consumer`'s `KAFKA_SASL_MECHANISM` today
(no silent default for the auth-mechanism choice, because a wrong guess run
against a real account is worse than a loud startup failure) and the same
"real hostname, placeholder credential" shape `poller-tfl`'s
`TFL_BASE_URL`/`TFL_APP_KEY` pair already has, just inverted (there, the URL
is real and public and the key is the only secret; here, the host is real
and public and *both* the username and the auth mechanism are unconfirmed
placeholders).

### Helm chart sketch

**New `values.yaml` block**, following `devAuthentik.*`'s
enabled-flag-plus-block convention and `pollers.*`'s per-poller shape:

```yaml
# sketch — not final
scheduleFeed:
  enabled: false          # opt-in, matches every RDM poller's own default
  image:
    repository: distant-signal/schedule-ingest
    tag: ""
    pullPolicy: IfNotPresent
  sftp:
    host: dtd.atocrsp.org  # a REAL default, unlike pollers.*.baseUrl
    port: 22                # ASSUMED standard SFTP port -- unconfirmed
    username: ""             # required when enabled; no default possible
    # "password" or "private-key" -- required when enabled. UNCONFIRMED
    # which applies until DTD portal access exists; see README/Open
    # Questions. Enabling this subsystem without setting it aborts
    # rendering, same posture as pollers.*.baseUrl today.
    authMethod: ""
    password: ""
    privateKey: ""           # PEM content; chart mounts it as a file, not
                              # an env var -- see Credentials above.
    privateKeyPassphrase: ""
    # Empty = trust-on-first-connect with a loud WARN log of the observed
    # fingerprint. Set once known/confirmed, to pin strict verification.
    hostKeyFingerprint: ""
    existingSecret: ""
    existingSecretPasswordKey: dtd-sftp-password
    existingSecretPrivateKeyKey: dtd-sftp-private-key
  checkTimes: "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
  retentionKeepSequences: 2
  persistence:
    enabled: true
    size: 5Gi
    storageClass: ""
    existingClaim: ""
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

**New templates**, following this chart's existing per-component naming and
the `pollers.*.baseUrl`/`fail` precedent for required-when-enabled fields:

- **`schedulefeed-secret.yaml`** — same shape as `devauthentik-secret.yaml`
  structurally (its own dedicated Secret object, gated behind
  `scheduleFeed.enabled`), but **without** the lookup-preserve
  auto-generation pattern — like `pollers.*.apiKey`, these are real external
  credentials that must never be randomly generated. Renders
  `dtd-sftp-password` and/or `dtd-sftp-private-key`
  (+`dtd-sftp-private-key-passphrase`) depending on `sftp.authMethod`,
  mirroring `secret.yaml`'s existing per-item conditional-render style.
- **`schedulefeed-pvc.yaml`** — a standalone `PersistentVolumeClaim`
  (`ReadWriteOnce`), not a `volumeClaimTemplate`, per the Storage section's
  reasoning above. Follows `postgresql.persistence.*`'s existing
  `enabled`/`size`/`storageClass`/`existingClaim` shape exactly.
- **`schedulefeed-deployment.yaml`** — `replicas: 1` fixed,
  `strategy: Recreate` (a singleton writer to a shared `ReadWriteOnce` PVC —
  identical rationale to `aggregator-deployment.yaml` and every
  `poller-*` Deployment already in this chart: two replicas racing to pull
  and write to the same volume is a correctness risk with no scaling
  benefit). Mounts the PVC at `/data/schedule-feed`, mounts the private-key
  Secret as a read-only volume when `sftp.authMethod == "private-key"`,
  renders `fail`-guarded checks for `sftp.username`/`sftp.authMethod` being
  set when `enabled: true`, identical posture to
  `poller-deployments.yaml`'s existing `baseUrl` guard. Optional `/metrics`
  listener, same `prometheus.io/scrape` annotation pattern every poller
  already uses when `metrics.enabled`.
- **`networkpolicy.yaml` addition** — no inbound `Service` exists for this
  component at all (it's a pure outbound puller), so the **only** new rule
  needed is the same scoped ingress-allow for `/metrics` from
  `networkPolicy.monitoringNamespace` every other poller/aggregator block
  already has, added as its own dedicated block (like `aggregator`'s,
  since `scheduleFeed` isn't part of the `.Values.pollers` map). No egress
  rule is needed either: this chart's `networkpolicy.yaml` already states
  egress is "deliberately unrestricted" specifically because "the pollers
  must reach arbitrary [external hosts]" — this component fits that exact
  existing rationale without needing a new carve-out.

**No new `Service`, no `Ingress`, no `LoadBalancer`, no SSH host-key
material anywhere in this design.** Worth stating plainly since it's the
single biggest structural difference from the (superseded) push-based sketch
in the ingress research doc's own "Chart additions... SFTP path" section.

### `NOTES.txt` / documentation touch

Following `devAuthentik`'s precedent of documenting a manual step in the
chart's own `NOTES.txt` output when relevant: when `scheduleFeed.enabled` is
true and `sftp.hostKeyFingerprint` is still empty, the rendered notes should
say so explicitly — "schedule-feed is running in trust-on-first-connect
mode; check the pod's logs after its first successful connection for the
observed host-key fingerprint, and set `scheduleFeed.sftp.hostKeyFingerprint`
to pin it" — the same instinct as `devAuthentik`'s own documented recovery-
key fallback, surfaced at install time rather than discovered later in logs.

## Open questions — blocked on DTD portal access

**The repo owner does not currently have a DTD portal
(`dtdportal.atocrsp.org`) account — access is pending/being applied for.**
Every item below is a real fact this design would normally nail down, that
genuinely cannot be resolved without that access, because RSPS5046 (the only
document available to this app's research so far, fetched and read in full
twice) explicitly does not state it. Each item names what's unknown, why,
the concrete design impact both ways, and this document's best-effort
assumption where one was made — clearly labeled as an assumption, not a
finding.

1. **Authentication mechanism: password vs. SSH public/private key vs.
   something else entirely.** RSPS5046 says only that "Data Recipients can
   manage their SFTP Server configuration details using the DTD Web Portal"
   (§7.5.1) — the mechanism itself lives inside that portal's UI, not in the
   published spec. Confirmed absent from the full 39-page text by two
   independent fetches (the words "password" and "key" do not appear in
   connection with SFTP login anywhere in the document).
   - **If password-based**: the credentials design above (a single
     `dtd-sftp-password` Secret key, consumed via env var) is exactly
     right, and the private-key-specific pieces (the file-mount volume, the
     passphrase field, `AuthMethod::PrivateKey` in the config enum) are
     unused dead weight worth removing at implementation time.
   - **If SSH-key-based**: this app must additionally *generate* a keypair,
     register the **public** half with DTD via the portal (a manual,
     one-time bootstrap step no chart or compose file can automate — akin
     to, but not identical to, the SSH-host-key generation the now-
     superseded push design would have needed on the *receiving* side), and
     hold the **private** half as the file-mounted Secret already sketched
     above.
   - **This document makes no assumption about which is more likely** — no
     source available to this research states or implies either mechanism
     for pull specifically. Both code paths are sketched above precisely
     because neither could be ruled out.
   - **First implementation task, before anything else in this document is
     executed literally**: obtain DTD portal access, create/confirm the
     account, and read off the real mechanism from the portal UI.
2. **Exact port.** Assumed `22` (the SFTP/SSH standard) in this design's
   defaults — RSPS5046 states no port number anywhere. Low-impact if wrong
   (a single config value change), but worth confirming during the same
   portal visit as item 1.
3. **Whether DTD requires source-IP allowlisting/pre-registration even for
   pull connections.** RSPS5046 §7.5.4 discusses IP addresses being
   available "using the web portal... if firewall configuration is
   required" in a generic, direction-agnostic way — it does not say whether
   *DTD's own* SFTP service restricts inbound (from DTD's perspective)
   connections to pre-registered source IPs, the way many enterprise SFTP
   services do even for pull.
   - **If DTD requires it**: this app's Kubernetes egress path needs a
     stable, known source IP — a real new requirement, since this chart's
     `networkPolicy.yaml` egress is "deliberately unrestricted" today
     specifically because no external host this app talks to has ever
     needed IP pinning. A cloud NAT gateway with a static/reserved egress
     IP (or equivalent) would become a genuine new piece of infrastructure
     this chart doesn't currently model or document.
   - **If it doesn't**: no change needed at all; the existing unrestricted-
     egress posture already covers this connection exactly like every
     other outbound feed.
   - Confirming this requires either the portal itself or a direct question
     to DTD support — not resolvable from RSPS5046's public text.
4. **Account provisioning lag.** How long from applying for portal access to
   receiving usable SFTP credentials is unknown — this affects whether
   "confirm the facts above and implement" fits in one work session or
   needs a multi-day/week gap built into any implementation plan's
   scheduling. Not discoverable except by going through the process.
5. **Whether a live pull session's directory/file layout matches the
   documented manifest format exactly** — same filenames, same lack of a
   path prefix, whether older sequences remain listable in the directory
   after being pulled or whether DTD cleans up server-side. RSPS5046's own
   worked example (§5.2.2) and the one real (differently-sourced) sample
   this app's research already has both point the same way, but neither is
   a live pull session — this needs a real connection to confirm.
6. **Host-key fingerprint for `dtd.atocrsp.org`.** Needed to move from this
   design's trust-on-first-connect default to strict pinned verification.
   Whether DTD publishes this via the portal or some other channel (support
   ticket, a published security page) is unconfirmed.
7. **Whether SFTP pull is actually enabled on this app's specific RDM/DTD
   subscription**, as opposed to being a DTD-wide capability RSPS5046
   documents in the abstract. The ingress research doc already flagged this
   exact gap; it remains open. A subscription that defaults to push-only
   until pull is explicitly requested/enabled via the portal is plausible
   and would need that request made before any of this design's pull logic
   has anything to connect to.

**Concrete first task for an eventual implementation plan**: before any of
the crate-scaffolding, chart-template, or docker-compose tasks in this
document are executed literally as specified, the first task must be
*"obtain DTD portal access and confirm items 1-7 above,"* with an explicit
checkpoint to revise this design's credentials section (and, if item 3
resolves unfavourably, its network/egress assumptions) before implementing
the rest as currently written. Treating this design as directly executable
without that step risks building the wrong credential-handling code path
entirely (e.g., a password-only implementation against an account that
turns out to require key-based auth).

## Other open questions and risks (not portal-blocked)

- **`russh-sftp` vs. `ssh2` was not exercised against a real SFTP server in
  this pass** — no real DTD credentials exist yet to test against (see
  above). The recommendation for `russh-sftp` is a reasoned leaning from
  published crate metadata, not a verified integration.
- **The Europe/London vs. UTC assumption for `check_times`** is inferred,
  not confirmed from RSPS5046's own text in the passages already fetched —
  unlike the portal-blocked items above, this is resolvable by re-reading
  the same already-public document more carefully, with no new access
  needed.
- **Whether DTD's manifest ever omits the `.DAT` file itself from its own
  listing in a way that could confuse "is this delivery complete" logic**
  was not stress-tested — the one real sample's manifest lists exactly its
  8 siblings, matching RSPS5046's own worked example, but a manifest for a
  differently-shaped delivery (e.g. after an "Empty" feed, per §7.3.2) has
  never been observed by this app's research. The gap-handling logic above
  (log + metric + still ingest) is designed to degrade safely if a future
  real delivery's manifest looks different than expected, but this hasn't
  been tested against a real "Empty" feed sample.
- **Whether a live pull needs to worry about partial/concurrent writes on
  DTD's own side** (i.e., could the manifest be readable before every file
  it names has finished being written to DTD's own SFTP directory) is
  unconfirmed. The byte-count-matching completeness check in the Pull
  procedure section is a mitigation, not a guarantee — a file mid-write on
  DTD's side that happens to already match its *eventual* final size at the
  moment this app reads its `stat` would not be caught by this check. No
  evidence either way exists on whether DTD publishes files only after
  they're fully written server-side.
- **How a future Option-B delay-inference consumer would read these files
  back out** (sidecar-in-Pod, `ReadWriteMany` PVC, or this crate eventually
  parsing into Postgres directly) is explicitly deferred, per Non-goals —
  flagged again here as a real design gap for whoever picks that up next,
  not resolved by a `ReadWriteOnce` PVC design that assumes a single writer
  and no reader.
- **The "research & analysis purposes only" licensing-wording question**
  the ingress research doc's Addendum §2 raised (whether the specific RDM
  licence's permitted-purpose wording is broad enough to cover a live,
  public-facing product feature, not just internal research) is unresolved
  and not re-litigated here — it is a legal/licensing question, separate
  from this document's technical scope, that should be confirmed directly
  with RDG before this feature ships to production regardless of how the
  DTD-portal-blocked items above resolve.

## Summary (for the person who asked)

**Concrete decisions this document makes, given SFTP pull is the settled
choice:** a new `schedule-ingest` crate (not named `poller-*`, deliberately,
because its check-times scheduling is genuinely different from every
existing poller's tight interval loop) that connects outbound to
`dtd.atocrsp.org` at a handful of check times a day (matching RSPS5046's
documented 22:30-01:00 window plus its 16:00 worst-case fallback), verifies
a delivery's 9-file manifest is completely present before treating it as
usable, writes to a plain `ReadWriteOnce` PVC sized around the real
~711MB-uncompressed anchor with 2-generation retention, records each
ingest through `api`'s existing freshness-contract pattern rather than a new
mechanism, and treats a sequence-number gap as "log loudly, alert via a
metric, but still ingest" rather than either silent acceptance or a refusal
to proceed. No inbound Kubernetes `Service`, no SSH host keys to manage, no
internet-facing daemon — the entire structural simplification pull promised
over the (superseded) push/bucket designs is fully realized here.

**What genuinely can't be nailed down yet, and why**: seven concrete facts
— the auth mechanism above all — live inside `dtdportal.atocrsp.org`, an
account the repo owner doesn't yet have. This document sketches both
plausible credential-handling paths (password and private-key) rather than
guessing which one to build, and names getting portal access as the literal
first task of any implementation plan that follows this design. Everything
else here — the crate's shape, the scheduling model, the storage/retention
policy, the chart/compose wiring, the gap-handling posture — is real,
concrete, and buildable today without that access; only the credentials
plumbing's exact final shape depends on it.
