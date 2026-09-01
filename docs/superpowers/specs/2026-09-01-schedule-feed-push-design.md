# Schedule-Feed Ingestion via SFTP Push — Design

**Status: design/proposal, not an approved implementation plan.** Written to
the same rigor and citation discipline as
`docs/superpowers/specs/2026-08-30-schedule-feed-sftp-pull-design.md` ("the
pull design doc" — this document's immediate predecessor, whose core "pull"
recommendation this document replaces, not merely amends — see "Why this
document exists" immediately below) and
`docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` (this repo's
general precedent for a real, later-implemented infrastructure design for a
new backing service). Concrete `docker-compose.yml`/Helm sketches marked
"sketch — not final," every external claim attributed to a real source, and
nothing here is committed code — no Rust code, Dockerfile, or Helm template,
sketches only. It also does not touch `crates/trust-consumer` in any way,
for the same reason the pull design doc gave: that crate's STANOX↔CRS
matching gap is separate, currently-in-flight work this document neither
depends on nor blocks.

## Why this document exists

`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md` ("the
ingress research doc") was originally written entirely under a **push-only**
assumption, sourced from the Open Rail Data Wiki's generic description of
RDM file feeds ("there's no supported mechanism to retrieve files from the
RDM on request"). Its own 2026-08-30 addendum then reversed that premise,
after finding RDG/RSP's own **RSPS5046 "Timetable Information Data Feed
Interface Specification"** documents SFTP *pull* from `dtd.atocrsp.org` as a
first-class, RDG-supported option too — which the pull design doc then took
as its settled starting point and designed against in full.

**That premise has now been reversed a second time.** Per the pull design
doc's own new superseding notice (added 2026-09-01, at its top): **the repo
owner confirmed that SFTP Pull access via the DTD portal
(`dtdportal.atocrsp.org`) is staff-only** — gated behind an RDG/RSP staff
account or equivalent internal access this app's operator does not have and
cannot get, not something a normal registered Data Recipient can self-serve.

**Source: the repo owner, 2026-09-01.** This is stated as given information,
not independently discovered or re-verified via `WebFetch`/`WebSearch` in
this pass — flagged explicitly, per this app's research documents'
established convention of attributing external/task-given claims to their
source rather than asserting them as independently confirmed. Nothing in
RSPS5046 itself documents portal-access eligibility rules one way or the
other (confirmed again in this pass — see "A primary-source re-check," below
— the document describes the *feed*, not who may register for a portal
account), so this finding could only ever come from the repo owner directly,
not from a document search.

**This is a permanent structural blocker, not a temporary one.** The pull
design doc's own "Open questions — blocked on DTD portal access" section
opened with "access is pending/being applied for" and its item 7 asked
"whether SFTP pull is actually enabled on this app's specific RDM/DTD
subscription... as opposed to being a DTD-wide capability RSPS5046 documents
in the abstract" — both phrased as open, resolvable-with-more-access
questions. They are now resolved, negatively and durably: pull is not merely
unconfirmed for this app's subscription, it is unreachable in principle,
because the portal step every pull path runs through is staff-gated.

**Consequently: this document reinstates SFTP push as the design this app
should actually build**, matching the ingress research doc's *original*,
pre-addendum premise (its Sections 1, 3's SFTP half, 4, and the SFTP-path
half of its architecture sketch) — but cross-checked against everything
learned since, including a re-read of RSPS5046's push-specific text
(below) and the pull design doc's own reusable research (the real manifest
format, file-size anchors, gap-detection policy, and database-bookkeeping
approach, all mechanism-agnostic). Section-by-section reuse notes are called
out throughout, rather than silently re-deriving what's already been
established.

## Problem

Unchanged from both predecessor documents, restated briefly rather than
re-derived: `docs/superpowers/specs/2026-08-29-trust-schedule-delay-
inference-design.md` ("the base spec") names CIF SCHEDULE ingestion as the
prerequisite for TRUST-vs-schedule delay inference — the segment-level
status precision and full-population coverage `infer_from_samples` cannot
deliver at any sample size (base spec, "What 'higher fidelity' actually
buys"). That document's own **Open Question #2** frames the ingestion side
plainly: "File-feed push delivery is a new operational commitment this app
has never taken on: this app would need to stand up and maintain either an
SFTP endpoint or a cloud storage bucket that RDM pushes into, plus a
component that watches that destination for new full/update extracts and
ingests them... a genuinely new category of moving part, not a variant of
something already running." This document answers exactly that question,
narrowed by everything both `dtd.atocrsp.org`-focused documents already
found: the "cloud storage bucket" half of that framing is not confirmed
for this app's specific licensed product (see below); the SFTP half is,
and push is now the only reachable SFTP variant.

`trust-consumer` receives real TRUST movement events (STANOX-keyed) but has
nothing to compare them against today — no planned timetable, no
calling-point list. This document is scoped identically to the pull design
doc's own scope statement: **get the daily full-refresh files onto disk
(and their arrival recorded in a database) reliably** — not the
delay-inference logic that consumes them, and not `trust-consumer`.

## Goals

- Reliably **receive** DTD's **"Timetable - Full Refresh - Daily"** feed
  (RDM product `P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`, per the ingress
  research doc's Addendum §2) via an SFTP server this app's operator stands
  up and controls, once per day, arriving in the window RSPS5046 documents.
- Land the delivery's files on persistent storage, verifying the manifest
  (`RJTTFnnn.DAT`) lists every file it names as actually present and fully
  written before treating a delivery as usable — **reused directly from the
  pull design doc's Pull procedure section**, since manifest verification is
  about what arrived, not how.
- Record each successful ingest (sequence number, timestamp, file
  count/sizes) in a database, reusing this app's existing poller-freshness
  conventions — **reused directly from the pull design doc's Database
  bookkeeping section**, unchanged.
- A sane retention/cleanup policy sized against the real ~711MB-uncompressed
  anchor — **reused directly from the pull design doc's Storage and
  retention section**, unchanged.
- Detect and loudly surface a sequence-number gap or an unexpected delivery
  shape, per RSPS5046 §7.4 — **reused directly from the pull design doc's
  gap-handling policy**, unchanged (§7.4's resumption text describes DTD's
  own behaviour, not which party initiated the connection).
- Stand up and secure the **receiving** side of the push: an SFTP server
  bound to a reachable address, its own credential/host-key material, and
  (new, relative to the pull design) the architectural question of how a
  generic off-the-shelf SFTP server image and this app's own ingest logic
  share the files that land on it — addressed concretely below.
- Concrete `docker-compose.yml` and Helm chart sketches, following this
  chart's own established conventions, matching the depth (not the content)
  of the pull design doc's own sketches.

## Non-goals

Reused verbatim from the pull design doc's Non-goals, with one item
inverted:

- **Implementing the delay-inference logic that consumes this data.**
  Unchanged.
- **Touching `crates/trust-consumer`.** Unchanged.
- **Re-litigating whether to build this at all.** Unchanged — the base
  spec's "proceed with caveats, not yet" verdict stands.
- ~~**Designing the SFTP-push or cloud-bucket variants.**~~ **Inverted**:
  this document *is* the SFTP-push design; it does not design the
  cloud-bucket variant (see "Cloud bucket, reconsidered," below, for why
  that option is set aside rather than designed).
- **Designing SFTP pull.** The pull design doc already did this in full; it
  is not re-litigated here, only superseded as a recommendation for the
  reason stated above.
- **How a future downstream consumer reads these landed files back out**
  beyond the specific reader/writer architecture this document does commit
  to below (a shared-Pod, shared-volume design — see "The reader/writer
  problem push introduces"). What a future Option-B delay-inference service
  does with the files once they're on disk is still out of scope.
- **Adopting a cron-expression-parsing crate.** Unchanged — a hand-rolled
  check-times list remains sufficient.
- **Any Rust code, Dockerfile, or Helm template.** Everything below marked
  "sketch" is illustrative only.
- **Resolving every open question below.** Several remain genuinely open —
  see "Open questions," which are honest gaps, not resolved by this pass.

## A primary-source re-check: RSPS5046's push-specific text, read directly

The pull design doc and the ingress research doc's addendum both already
quote RSPS5046 §7.1.2's push bullet ("SFTP Push over the Internet from the
DTD's SFTP Client to the Data Recipient's SFTP Server"). Rather than taking
that single quoted line on faith, this pass re-read the same primary source
directly — the local PDF at
`licnses/RSPS5046 P-04-02 Timetable Information Data Feed Interface
Specification-1.pdf` (the exact file the ingress research doc's addendum
fetched and extracted with `pypdf`; here read the same way, from the copy
already present in this repo's working tree rather than re-fetched over the
network) — specifically to check for any push-specific detail neither
existing document had reason to look for yet, and to re-verify the
cloud-bucket question below.

**Result: §7 ("Data Feed Distribution Service") contains no push-specific
detail beyond what's already quoted in the two existing documents.**
Confirmed line-by-line against the full extracted text of pages 37-38: §7.1.2
(the two delivery methods), §7.2.2 (manifest-completeness is the recipient's
job, direction-agnostic), §7.3.1/§7.3.2 (the 22:30-01:00 window and 16:00
fallback, direction-agnostic — DTD *produces* the feed on this schedule
regardless of which delivery method carries it out), §7.4 (resumption/gap
semantics, direction-agnostic), §7.5.1 ("Data Recipients can manage their
SFTP Server configuration details using the DTD Web Portal" — this
sentence, on its face, does not distinguish "configuring a pull account" from
"configuring where DTD should push to and with what credentials"; see "Does
the staff-only finding also block push configuration?" below for why this
matters a great deal here), §7.5.2 (dual-server resilience, "the DTD will
distribute Fares Data to both servers" — verbatim, "Fares Data" not
"Timetable Data," an apparent copy-paste artifact in RDG's own document
worth flagging rather than silently correcting), §7.5.3 (DTD's own SFTP
service preserves "the same domain and IP address" through its own
failover — describes the pull hostname's stability, not directly relevant
to push), §7.5.4 ("Data Recipients should use the web portal... for the IP
address of the DTD SFTP Server **or Client** if firewall configuration is
required" — the "or Client" phrase is the one new, previously-unquoted
detail this re-check surfaces: it confirms the portal is where an operator
would learn DTD's outbound push-client IP, for inbound firewall
allowlisting on this app's own SFTP server, if that turns out to be worth
doing), §7.6.1 (new recipients get a full refresh first), §7.7 (weekly/
monthly Wednesday cadences, not relevant to this app's actual daily-full-
refresh product). No authentication mechanism, port number, or
push-destination-registration process is named anywhere in the document —
the same absence the addendum already found for pull, confirmed here to
apply identically to push (a full-text keyword search of the extracted text
for "password," "key," "authenticat," "certificate," and "PGP" returns zero
matches in connection with SFTP login, for either direction).

### Cloud bucket, reconsidered

**A genuinely new finding from this re-check**: a full-text keyword search
of RSPS5046's complete extracted text for `bucket`, `S3`, `Amazon`, `Azure`,
`AWS`, `cloud`, and `Google` returns **zero matches**, anywhere in the
document's 39 pages. **RSPS5046 — the interface spec for the exact
"Timetable - Full Refresh - Daily" product this app has a real, signed
licence for — documents exactly two delivery methods: SFTP Pull and SFTP
Push. It does not document a cloud-storage-bucket delivery option at all.**

This directly narrows the ingress research doc's original Section 2, "Cloud
storage bucket push — the alternative RDM explicitly supports." That
section's premise ("the alternative RDM explicitly supports") traces back to
the base spec's citation of the Open Rail Data Wiki's *generic* description
of RDM file feeds as a whole: "File feeds can be transferred via 'push'
options to major cloud providers (AWS, Azure, Google Cloud) or via SFTP."
That is a true statement about RDM file feeds as a product category — but it
is not confirmed, by any primary source this app's research has read, to
apply to *this specific product*. RSPS5046 is the one document in this
app's whole research trail that is unambiguously specific to this exact
licensed feed (matched line-for-line against the real sample data, per the
ingress research doc's Addendum §1/§3), and it names only SFTP.

**Practical consequence**: this document designs the SFTP-push receiver only.
The cloud-bucket path is not ruled out with certainty — RSPS5046 could
simply be silent on a delivery option DTD offers but chooses not to
document in this particular interface spec, and the wiki's broader claim
about RDM file feeds generally could still be accurate for this product in
practice — but building against it would mean building against a claim with
no primary-source confirmation specific to this feed, where a claim *with*
specific confirmation (SFTP push) is available instead. Flagged as an open
question below, not asserted as closed either way.

### Does the staff-only finding also block push configuration?

**A new open question this re-check surfaces, not resolved by anything
available to this pass.** RSPS5046 §7.5.1 states, without distinguishing
push from pull, that "Data Recipients can manage their SFTP Server
configuration details using the DTD Web Portal." The repo owner's
2026-09-01 finding, as given to this document, is specifically about **pull
access** being staff-only. Whether that finding describes:

1. **A pull-specific toggle or feature** within an otherwise generally
   accessible DTD portal (in which case push-side configuration — telling
   DTD where to push, registering this app's SFTP server details, whatever
   credential exchange push requires — might still be reachable through the
   same portal by a normal Data Recipient), or
2. **The DTD portal itself, as a whole**, being staff-gated (in which case
   *no* self-service configuration of *either* delivery method exists for
   this app's operator, and push would additionally require some other,
   currently-unknown channel — a support ticket, an account manager, direct
   correspondence with RDG/DTD — to get configured at all),

is genuinely ambiguous from the phrasing available to this document, and
this pass has no way to resolve it further (RSPS5046 itself, re-read
specifically for this question, says nothing about portal-access
eligibility rules for either direction — see above). **This is arguably the
single most consequential open question in this entire document**: if
reading 2 is correct, this design's entire premise — that push is reachable
where pull wasn't — could be wrong in the same way pull turned out to be,
and the concrete next step for anyone picking this design up is confirming
with RDG/DTD support directly (not via the portal, since access to it is
exactly what's in question) whether push-side configuration for a normal
Data Recipient is actually obtainable before investing further engineering
effort. See "Open questions," below, for how this interacts with the rest
of the design.

## Research recap (see the ingress research doc and the pull design doc for the full trail)

Facts this design leans on, all sourced in the two predecessor documents and
independently re-confirmed against the local RSPS5046 PDF in this pass
(mechanism-agnostic facts carry over from the pull design doc's own "Research
recap" unchanged; mechanism-specific ones are marked):

- **Delivery window**: "around 10.30pm to 1am" normally (§7.3.1), worst-case
  fallback "Empty" feed (previous full refresh resent, empty update files)
  by 4pm (§7.3.2) — **direction-agnostic**: this describes when DTD
  *produces* the feed, not which party's connection carries it.
- **Manifest format**: a `.DAT` "Contents" file lists every other file in
  the delivery except itself; the real sample (`RJTTF942DAT.txt`) matches
  RSPS5046 §5.2.2's worked example line-for-line — **reused unchanged**.
- **9-file structure and sizes**: `DAT` (618B), `MCA` (707.7MB, the actual
  CIF Basic Schedule data), `REJ` (246B), `ZTR` (2.9MB), `SET` (499B,
  literal `UCFCATE`), `FLF` (101KB), `ALF` (233KB), `TSI` (714B), `MSN`
  (340KB). Total **76,446,640 bytes compressed / 711,352,325 bytes
  uncompressed** across all 9 files, as bundled for transport to this app's
  research — **reused unchanged**, with the same caveat the pull design doc
  already flagged: a live delivery (pull *or* push) most likely presents 9
  loose files matching the manifest, not a single archive, so the
  uncompressed ~711MB figure is the one to size storage against, not the
  ~76MB compressed figure.
- **Resumption/gap semantics**: RSPS5046 §7.4 — a sequence-number gap after
  an "Empty" feed is documented, expected behaviour, not proof of a missed
  delivery, and DTD's own practice is to contact recipients directly before
  sending more than one feed a day — **reused unchanged**, since §7.4
  describes DTD's own resumption behaviour irrespective of delivery
  direction.
- **Bootstrap**: "New Daily Recipients that begin the service will be
  provided with a full refresh of timetable data" (§7.6.1) — **reused
  unchanged**.
- **Manifest-completeness is explicitly the recipient's job**: RSPS5046
  §7.2.2 — **reused unchanged**, and if anything more directly load-bearing
  for push than pull: a pull connection can re-list a remote directory on
  demand to re-check completeness, but a push receiver only ever sees files
  arrive at its own pace on its own server, making the manifest-driven
  "wait for every named file, verify byte counts, then treat as complete"
  procedure (below) the *only* signal available, not one of several.
- **What does NOT carry over from the pull design doc**: `dtd.atocrsp.org`
  as a connection target (this app is no longer the party dialing out to a
  known hostname — DTD is now the party dialing in, to an address *this
  app's operator* must provide, not a value RSPS5046 publishes), the "verify
  DTD's host key" host-key-verification design (inverted — see Credentials,
  below), the `russh-sftp`/`ssh2` client-library research (this app is not
  running an SFTP *client* against DTD any more; the SFTP crate research the
  ingress research doc did for the push-*receiver* case, Section 1's
  `atmoz/sftp`-vs-SFTPGo comparison, is what actually applies), and the
  check-times-against-a-known-hostname scheduling shape (replaced by
  directory-polling, structurally the same *idea* — check on a schedule
  matching RSPS5046's documented window — but watching a local mount, not
  dialing a remote server).

## Design

### Why push changes the shape back, restated concretely

The pull design doc's whole premise was "this app is always the calling
party... no port is opened on this app's side." That is no longer available.
**Push means this app must run and secure a listening SFTP daemon that
accepts an unsolicited inbound connection from DTD's infrastructure** — the
ingress research doc's Section 4 already named this precisely: "This would
be this app's first inbound-facing backend service other than the
deliberately-public frontend/api behind `ingress.yaml`." Every concern that
section raised — the daemon itself is an attack surface this app's own code
doesn't control, credential/host-key management is new secret material this
chart has never held, a new Kubernetes `Service` type this chart has never
rendered for real external use, source-IP allowlisting is the single most
consequential open item — applies again, in full, exactly as originally
written. This document does not re-derive that reasoning; it reuses it
directly and builds the concrete design on top of it.

### The receiving component: reused directly from the ingress research doc's Section 1

**Candidate images**: `atmoz/sftp` (thin OpenSSH wrapper, ~2 years stale per
the ingress research doc's own fetch, a real maintenance-currency concern
for a component on the public internet) vs. **SFTPGo** (`drakkan/sftpgo`,
actively maintained, event-driven, supports SFTP/FTP/S/WebDAV with pluggable
storage backends including local filesystem). **Recommendation unchanged
from the ingress research doc: SFTPGo**, for the maintenance-currency
argument specifically — an internet-facing SSH daemon's CVE exposure is an
operational emergency independent of whether this app's own code changed,
and `atmoz/sftp`'s multi-year-stale last-update is a real, cited data point
against it for exactly that reason.

**How it fits this chart's existing pattern**: `devauthentik-postgres-
statefulset.yaml`/`devauthentik-server-deployment.yaml` remain the closest
literal precedent — a values block gating a whole optional subsystem, its
own Secret entries, its own Service, and persistent storage for the stateful
half — with the same three differences from that precedent the ingress
research doc already named:

- **Deployment, not StatefulSet.** A singleton receiver (RDG pushes to one
  endpoint; there is no clustering story) matches `aggregator-
  deployment.yaml`'s pattern: `replicas: 1` fixed, `strategy: Recreate`. Two
  replicas racing to accept connections and write to the same volume is a
  correctness risk with no offsetting benefit.
- **New Secret material this chart has never held: SSH host keys.** Every
  existing secret in this chart is a flat password/token string; an SSH
  host keypair is structurally different. Helm's `genPrivateKey` Sprig
  function is the plausible mechanism, **not tested against this chart's
  Helm-4-specific lookup-preserve pattern in either predecessor document or
  this one** — flagged again here as an implementation-time verification
  task, not assumed to work by analogy.
- **A Service type this chart has never rendered for production use.**
  Either `type: LoadBalancer` or `NodePort` fronted by an operator-managed
  external LB/DNS record — a sibling to `ingress.yaml`, not an extension of
  it, since `Ingress` resources are HTTP(S)-only by the
  `networking.k8s.io/v1` spec itself and cannot carry raw TCP/SFTP traffic
  on port 22 (or whatever port DTD's push client ultimately targets — see
  Open questions).

### The reader/writer problem push introduces — a genuine new architectural question the pull design never faced

**This is the one structurally new problem push has that pull did not**,
and the pull design doc's own Non-goals explicitly deferred it as a future
gap: "How a future downstream consumer reads these landed files back out...
is explicitly deferred... flagged again here as a real design gap for
whoever picks that up next." For pull, that deferral was safe, because the
same `schedule-ingest` crate both fetched *and* verified the files — a
single process, a single writer, on a PVC only it ever mounted. **Push
breaks that assumption**: the component that receives the files (an
off-the-shelf SFTP server image, SFTPGo or `atmoz/sftp`) is necessarily a
*different* process from the component that needs to verify manifest
completeness, record the ingest in `api`, and prune old sequences — a
generic SFTP daemon has no CIF-manifest-awareness or `api`-POSTing logic of
its own, and bolting that logic into SFTPGo (e.g. via its documented
event-hook mechanism, not independently researched in either predecessor
document or verified in this pass) would mean either forking/heavily
configuring third-party server software or accepting an unfamiliar
extension surface for logic this app would rather own directly in Rust,
matching every other ingestion component's own-crate pattern.

**This document's concrete resolution**: run the SFTP daemon and a
`schedule-ingest`-equivalent watcher as **two containers in one Pod**,
sharing one mounted `PersistentVolumeClaim`. This is deliberately not two
separate Deployments connected by a `ReadWriteMany`-capable PVC or network
call, for a specific, sourced reason: Kubernetes' `ReadWriteOnce` access
mode restricts a volume to being mounted by pods scheduled on a single
*node*, not to a single *container* — multiple containers within the same
Pod can mount the same `ReadWriteOnce` volume simultaneously without issue,
which is exactly this shape (one shared Pod, one shared `emptyDir`-free real
PVC, two containers). This sidesteps two real problems at once:

- **No `ReadWriteMany` storage-class dependency.** This chart's entire
  existing persistence story (`postgresql.persistence.*`,
  `devAuthentik.postgresql.persistence`) already assumes `ReadWriteOnce` —
  introducing a `ReadWriteMany`-capable storage class (typically NFS/EFS/
  Azure-Files-backed) would be a first for this chart, and would reintroduce
  exactly the reliability caveat the ingress research doc's Section 3(b)
  already found against event-driven filesystem watching on network-backed
  volumes (the `notify` crate's own documentation: "network mounted
  filesystems like NFS may not emit any events... [use] `PollWatcher`"),
  which argues for polling regardless, but is a reason to prefer avoiding
  NFS-class storage in the first place if a simpler option exists — and it
  does.
- **Both containers stay singleton-shaped**, matching this chart's existing
  `replicas: 1`/`strategy: Recreate` rationale for `aggregator` and every
  `poller-*` Deployment: there is exactly one Pod, ever, for this subsystem,
  so "which node is it scheduled on" is a non-question a `ReadWriteOnce`
  volume answers trivially.

The trade-off, stated plainly rather than glossed over: this ties the two
containers' lifecycle together in one Pod spec (a restart of one container
via `kubectl` targets the Pod, not either container individually in the
values/chart surface, though Kubernetes does restart failed containers
independently within a Pod by default) and means a future move to
horizontally scale either half independently is not available without
revisiting this decision — acceptable here because neither half has any
scaling story to begin with (one push source, one watcher). This is a
concrete design decision this document makes, not left open — worth stating
since it is the one genuinely new problem this document had to solve that
neither predecessor document already had an answer for.

**Directory layout inside the shared PVC**, reused directly from the pull
design doc's Storage and retention section (mechanism-agnostic — describes
what's on disk, not how it got there):

```
/data/schedule-feed/
  incoming/                 # SFTP daemon's chroot target -- files land here
    RJTTF942DAT.txt
    RJTTF942MCA.txt
    ... (9 files, in progress or complete -- SFTP daemon has no concept
    ... of "complete", it just writes whatever bytes arrive)
  942/                      # a fully-verified, complete sequence, moved
                             # here by schedule-ingest once the manifest
                             # confirms every file is present and complete
    RJTTF942DAT.txt
    ...
  943/
    ...
```

`schedule-ingest` (the watcher container) polls `incoming/` on the same
check-times cadence the pull design doc already worked out (a
`chrono-tz`-aware `Europe/London` clock, times matching RSPS5046's
documented 22:30-01:00 window plus its 16:00 fallback — reused directly,
since the window describes when DTD *produces* the feed regardless of
delivery direction), looking for a new `RJTTFnnn.DAT` manifest it hasn't
already ingested. **This reuses the ingress research doc's own Section 3(a)
"polling a mounted volume" shape and Section 3(b)'s finding against
event-driven watching** (the `notify` crate's documented NFS/inotify
caveats) more directly than the pull design doc did, since this is now
genuinely a local-filesystem polling problem, not a remote-SFTP-listing one.

### Manifest verification, gap detection — reused directly from the pull design doc, adapted for a local directory

The pull design doc's Pull procedure steps 3-7 (locate the manifest, compare
sequence numbers against the last recorded ingest via `api` — not a local
marker file — treat `nnn == last+1` as expected and any other relationship
as a loud-logged-but-still-ingested gap per RSPS5046 §7.4, verify every
manifest-listed file's byte count before considering a delivery complete,
atomically move a verified-complete delivery out of the in-progress
directory, prune old sequences per retention) **all apply here unchanged**,
with one substitution: step 1-2 ("connect and authenticate," "list the
remote directory") is replaced by "scan `incoming/` via `std::fs::read_dir`"
— the ingress research doc's own Section 3(a) shape — since there is no
remote connection for this component to make at all; DTD's push already
did the connecting.

**One new completeness wrinkle push introduces that pull's design didn't
have to consider**: a pull connection lists a remote directory *DTD already
finished writing to* — by the time this app's watcher can see a file over
SFTP, DTD's own write to *its* side is done. A push receiver instead sees
files land via *DTD's* outbound connection to *this app's* server in
real time, meaning `schedule-ingest`'s directory scan could observe a
manifest file that has landed but sibling files still mid-transfer. The
byte-count-matching completeness check (comparing each file's size on disk
against the size the manifest itself declares, once the CIF manifest format
is confirmed to declare per-file sizes — **unconfirmed**, RSPS5046 §5.2.2's
worked example format was not independently re-checked for a size field in
this pass, flagged as a concrete follow-up rather than assumed) is the same
mitigation the pull design doc already named for its own, milder version of
this problem ("a file mid-write on DTD's side that happens to already match
its eventual final size... would not be caught by this check") — but it is
more directly load-bearing here, since push gives this app no "ask again,
the remote side is authoritative" fallback the way a pull re-list does.
**Recommendation**: `schedule-ingest` should require a manifest file's own
modification time (`mtime`) to be stable — unchanged across two consecutive
polling cycles — before treating any of its listed files as candidates for
completeness checking at all, as a cheap additional guard against reading a
manifest DTD's SFTP client is still in the process of writing. This is a
design decision this document makes, not verified against a real push
session (none exists yet), flagged as such.

### Storage and retention — reused directly from the pull design doc, unchanged

The ~711MB-uncompressed-per-generation anchor, `retention_keep_sequences`
default of 2 (current + one fallback), and the resulting "a 3-4GB PVC is
comfortably sufficient, 5Gi sketched as the default with headroom" sizing
all carry over from the pull design doc's Storage and retention section
without modification — this is a mechanism-agnostic conclusion about what
arrives and how much of it to keep, not about how it got here.

**One difference**: the pull design doc's PVC was mounted by exactly one
process (its own `schedule-ingest` crate, both downloader and verifier in
one). Here the PVC is mounted by **two** containers in the same Pod (the
SFTP daemon writing into `incoming/`, `schedule-ingest` reading from
`incoming/` and writing into the per-sequence directories) — still a single
`ReadWriteOnce` PVC, per "The reader/writer problem," above, just with two
writers/readers instead of one, both within the same Pod.

### Database bookkeeping — reused directly from the pull design doc, unchanged

The same `/private/schedule-feed-ingests` `api` route (`POST` on successful
ingest, `GET` returning the last recorded ingest in the existing
`LastFetchedResponse` shape every poller's freshness logic already reads),
the same `/public/freshness` `DataFreshness` struct gaining a
`schedule_feed` field, and the same reasoning for recording the sequence
number alongside the timestamp — **all reused directly, unchanged**, since
none of this depends on how the files arrived, only on the fact that
`schedule-ingest` (not the SFTP daemon) is the component responsible for
recording their arrival.

### Credentials and host keys — inverted from the pull design, reused from the ingress research doc

This is the one area where push and pull are near-mirror-images of each
other, and where the ingress research doc's original (pre-addendum)
reasoning — written for exactly this push scenario — is the correct source
to reuse, not the pull design doc's (which solved the opposite problem):

- **This app now generates and owns SSH host keys**, not DTD's. The pull
  design doc had to verify *DTD's* host key on connect (trust-on-first-
  connect vs. pinned fingerprint); here, this app's own SFTP server
  presents *its own* host key to DTD's connecting client, and it is DTD's
  side that must trust it — a step this app's operator cannot control or
  verify from this side at all. Generate once (per the Helm-`genPrivateKey`
  caveat above, unverified against this chart's Helm-4 lookup-preserve
  pattern), persist in a Secret, mount at `/etc/ssh/ssh_host_*`
  (`atmoz/sftp`'s documented mechanism; SFTPGo's equivalent was not
  independently re-checked in this pass). Rotation is a deliberate,
  rare operator action with real coordination cost on DTD's side — DTD (or
  whoever configures its push client, per the "does the staff-only finding
  also block push configuration" open question above) would need to accept
  a new fingerprint, a step this design cannot automate or verify.
- **Account credentials for DTD's push client to authenticate against this
  app's server**: both candidate images support password or public-key
  auth scoped to one chrooted virtual user. Public-key-only (no password
  fallback) is the safer default *if* DTD's push configuration supports
  registering/supplying a public key on its end — **unconfirmed**, same
  open item RSPS5046 leaves unstated for both directions (see "A
  primary-source re-check," above). Unlike host keys, this credential is
  never auto-generated by the chart — a random password or keypair would
  be meaningless without DTD's side registering it, identical reasoning to
  why `pollers.*.apiKey` is "rendered (possibly empty) but never generated"
  in `secret.yaml` today, and identical to the pull design doc's own
  stated reasoning for its (inverse) credential, just applied to the
  receiving side instead of the connecting side.
- **Static IP/hostname for DTD to push to**: unconfirmed whether DTD's push
  configuration wants a static IP, a stable hostname behind a changing IP,
  or a pre-registered/validated destination before it will push — the
  ingress research doc already flagged this exact gap for the generic push
  case; it is unresolved here too, and possibly gated behind the same
  staff-only-portal question above.

### `docker-compose.yml` sketch

```yaml
# sketch — not final
services:
  schedule-sftp:
    image: drakkan/sftpgo:latest   # tag pinning is an implementation-time
                                    # decision, not sketched further here
    restart: unless-stopped
    ports:
      - "${SCHEDULE_SFTP_PORT:-2222}:2022"   # host port -- SFTPGo's own
                                              # default container port,
                                              # confirmed against its
                                              # documented image, not
                                              # re-verified in this pass
    volumes:
      - schedule_feed_data:/data/schedule-feed
      - ${SCHEDULE_SFTP_HOST_KEY_HOST_PATH:-/dev/null}:/srv/sftpgo/host_keys:ro
    environment:
      SFTPGO_SFTPD__BINDINGS__0__PORT: "2022"
      # Real credentials for DTD's push client -- placeholders here,
      # matching this repo's *.env.example convention for feeds with no
      # confirmed endpoint yet. Never auto-generated -- see Credentials.
      SCHEDULE_SFTP_USERNAME: ${SCHEDULE_SFTP_USERNAME:?SCHEDULE_SFTP_USERNAME must be set once DTD's push account details are known}

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
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      RUST_LOG: ${RUST_LOG:-info}
      API_INGEST_URL: http://api:8080/private/schedule-feed-ingests
    volumes:
      - schedule_feed_data:/data/schedule-feed

volumes:
  schedule_feed_data:
```

Two separate compose *services* sharing one named volume (compose has no
native "two containers, one Pod" primitive the way Kubernetes does — the
closest equivalent here is simply two services both mounting the same named
volume, which achieves the same shared-storage effect for local dev even
though it isn't a single Kubernetes Pod the way the Helm sketch below is).
Matching this repo's `*.env.example` convention of deliberately
non-functional placeholders for feeds with no confirmed endpoint: local dev
has no real DTD to push into the compose SFTP container, so exercising the
pipeline locally needs either a manual SFTP/SCP of a sample file (the kind
described in "Research recap," above) or a small seed script — neither
designed by this document, matching the pull design doc's own equivalent
deferral for its SFTP path.

### Helm chart sketch

**New `values.yaml` block**, following `devAuthentik.*`'s enabled-flag-plus-
block convention:

```yaml
# sketch — not final
scheduleFeed:
  enabled: false          # opt-in, matches every RDM poller's own default
  sftp:
    image:
      repository: drakkan/sftpgo
      tag: ""
      pullPolicy: IfNotPresent
    port: 2022             # SFTPGo's own default -- not independently
                            # re-verified against a real deployed instance
                            # in this pass
    username: ""            # required when enabled; DTD's push account,
                            # no default possible
    authMethod: ""           # "password" or "public-key" -- UNCONFIRMED
                              # which DTD's push client supports; see Open
                              # questions
    password: ""
    publicKey: ""             # DTD's public key, if key-based auth is
                                # confirmed and DTD can supply one
    hostKeyExistingSecret: ""  # this app's OWN host key, generated once,
                                 # NOT DTD's -- inverted from the pull
                                 # design's dtd_sftp_host_key_fingerprint
    existingSecret: ""
  ingest:
    image:
      repository: distant-signal/schedule-ingest
      tag: ""
      pullPolicy: IfNotPresent
    checkTimes: "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
    retentionKeepSequences: 2
  service:
    type: LoadBalancer      # or NodePort + an operator-managed external
                              # LB/DNS -- see below
    annotations: {}
  persistence:
    enabled: true
    size: 5Gi
    storageClass: ""
    existingClaim: ""
  logLevel: info
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

**New templates**, following this chart's existing per-component naming:

- **`schedulefeed-secret.yaml`** — same non-generating posture as
  `pollers.*.apiKey`: renders `schedule-sftp-password` and/or
  `schedule-sftp-dtd-public-key` (whichever `authMethod` needs) plus the
  **generated** SSH host key material for this app's own server (the one
  genuinely new case in this chart where auto-generation *is* correct,
  mirroring `internalToken`'s existing lookup-preserve pattern rather than
  the "never generate a real external credential" rule that applies to the
  DTD-facing account credential itself).
- **`schedulefeed-pvc.yaml`** — a standalone `PersistentVolumeClaim`
  (`ReadWriteOnce`), mounted by both containers of the single Pod below —
  per "The reader/writer problem," above, this is sufficient; no
  `ReadWriteMany` storage class is needed.
- **`schedulefeed-deployment.yaml`** — **one Deployment, `replicas: 1`
  fixed, `strategy: Recreate`, with two containers**: `sftp` (SFTPGo,
  mounting the PVC at `/data/schedule-feed` chrooted to `incoming/`, mounting
  the host-key Secret at `/etc/ssh/ssh_host_*` or SFTPGo's equivalent path)
  and `ingest` (the `schedule-ingest` crate, mounting the same PVC at
  `/data/schedule-feed`). This is the concrete rendering of the shared-Pod
  design above — a genuine departure from every existing Deployment in this
  chart, which are all single-container, worth flagging plainly as a first
  for this chart's own conventions, not merely a variation on an existing
  pattern.
- **`schedulefeed-service.yaml`** — `type: LoadBalancer` (or `NodePort` +
  operator-managed external LB/DNS), targeting the `sftp` container's port
  only — a sibling to `ingress.yaml`, not an extension of it, for the same
  reason the ingress research doc already gave (`Ingress` is HTTP(S)-only
  by spec).
- **`networkpolicy.yaml` addition** — the **inbound** SFTP traffic reaching
  the new `LoadBalancer`/`NodePort` Service is, per the ingress research
  doc's already-confirmed reading of this chart's `networkpolicy.yaml`,
  entirely outside anything `NetworkPolicy` can express (it governs only
  in-cluster pod-to-pod traffic; external-source-IP filtering lives at the
  cloud load-balancer/security-group layer, a manual operator step this
  chart does not automate). The only new **in-cluster** rule needed is the
  same scoped `/metrics`-from-monitoring-namespace ingress-allow every
  poller/aggregator block already has, for the `ingest` container's metrics
  port.

**No SFTP-client-side sketch this time** — the `dtd_sftp_host`/
`dtd_sftp_port`/etc. fields from the pull design doc's `Config` sketch are
gone entirely, replaced by the receiver-side fields above.

### `NOTES.txt` / documentation touch

Same instinct as the pull design doc's equivalent section, inverted: when
`scheduleFeed.enabled` is true, the rendered notes should surface **this
app's own generated host-key fingerprint** (not DTD's, since this app no
longer needs to learn DTD's) — "schedule-feed's SFTP server generated a new
host key on first install; its fingerprint is `<value>` — this must be
communicated to DTD (via whatever channel push-configuration turns out to
require, see the design doc's Open questions) so DTD's push client can
trust it," mirroring `devAuthentik`'s own precedent of surfacing a manual
follow-up step at install time rather than leaving it to be discovered
later in logs.

## Open questions — honest, not resolved here

**The most consequential item is listed first, matching the pull design
doc's own convention of leading with what matters most rather than burying
it.**

1. **Whether push-side configuration is reachable at all for a normal Data
   Recipient, given the DTD portal is confirmed staff-only for pull** — see
   "Does the staff-only finding also block push configuration?" above. If
   the portal is staff-gated as a whole, this design's entire premise could
   fail the same way pull's did, and the concrete next step is a direct
   question to RDG/DTD support (not the portal) before investing further
   engineering effort here.
2. **RDG's exact push-destination configuration mechanics remain entirely
   unconfirmed** — carried forward from the ingress research doc, never
   resolved by either addendum or the pull design doc, and not resolved
   here: whether DTD wants a static IP vs. a stable hostname, how (or
   whether) a destination is pre-registered/validated before DTD will push,
   and whether DTD's push client supports SFTP public-key auth against this
   app's server (RSPS5046 states neither mechanism, for either direction,
   confirmed again by this pass's own full-text search).
3. **Whether the cloud-storage-bucket alternative genuinely applies to this
   specific licensed product** — see "Cloud bucket, reconsidered," above.
   RSPS5046 documents only SFTP (pull and push); the wiki's broader claim
   about RDM file feeds as a category is unconfirmed for this product
   specifically. Worth a direct question to RDG/RDM if the SFTP-push path
   above turns out to be blocked by item 1, since it may be a genuinely
   viable fallback RSPS5046 simply doesn't happen to mention.
4. **Whether DTD publishes fixed outbound source-IP ranges for its push
   client** — RSPS5046 §7.5.4's "or Client" phrasing (found in this pass's
   re-check, above) confirms a mechanism exists to learn this via the
   portal, narrowing but not closing the ingress research doc's original
   "single most consequential open question" framing. Still gated behind
   the same portal-access question as item 1.
5. **Neither candidate SFTP image's exact container entrypoint/security-
   context requirements were tested against a real cluster** — carried
   forward from the ingress research doc, unchanged, still open.
6. **Whether an SSH-host-key-generation mechanism (`genPrivateKey`) works
   cleanly under this chart's Helm-4 lookup-preserve pattern** — carried
   forward from the ingress research doc, unchanged, still open.
7. **Whether SFTPGo's event-hook mechanism could eventually replace the
   two-container shared-Pod design** with a single-container design (SFTPGo
   itself invoking `schedule-ingest`'s verification logic on file-close
   events, rather than a separate polling container) — not researched in
   this pass at all; flagged as a plausible future simplification, not
   designed here, since the shared-Pod/polling design above is sufficient
   and doesn't require trusting an unfamiliar third-party extension
   surface for logic this app would rather own directly.
8. **Whether a manifest file's declared per-file sizes (if any) can be used
   for completeness-checking, versus only comparing against DTD's
   eventual final on-disk size** — flagged in "Manifest verification,"
   above, not resolved: RSPS5046 §5.2.2's manifest format was not
   independently re-checked in this pass for a size field.
9. **The "research & analysis purposes only" licensing-wording question**
   the ingress research doc's Addendum §2 raised — unresolved, not
   re-litigated here, and unaffected by the pull-vs-push question: it needs
   confirming with RDG regardless of which delivery mechanism this app
   ultimately uses.
10. **Account/portal provisioning lag, if push-side access does turn out to
    be reachable** — unknown, same reasoning as the pull design doc's
    equivalent item, just for whatever channel push configuration actually
    requires (portal or otherwise, per item 1).

## Summary (for the person who asked)

**Why this document exists**: the pull design that
`docs/superpowers/specs/2026-08-30-schedule-feed-sftp-pull-design.md`
worked out in full detail turned out not to be buildable — the repo owner
confirmed (2026-09-01) that SFTP Pull access via the DTD portal is
staff-only, not something this app's operator can self-serve, permanently
closing the path that document's entire architecture depended on. This
document reinstates the ingress research doc's *original*, pre-addendum
premise — SFTP push, this app's operator standing up and controlling the
receiving server — as the correct design, cross-checked against RSPS5046's
push-specific text directly (re-read from the local PDF copy in this
repo's `licnses/` directory) and against everything both addenda and the
pull design doc learned about the feed's shape in the meantime.

**What's reused, largely unchanged, because it's mechanism-agnostic**: the
real 9-file manifest format and ~711MB-uncompressed size anchor, the
sequence-gap-detection policy (log loudly, alert via metric, still ingest,
per RSPS5046 §7.4), the retention policy (2 generations, ~5GB PVC), and the
database-bookkeeping approach (reuse `api`'s existing freshness-contract
pattern via a new `/private/schedule-feed-ingests` route) — all carried
forward directly from the pull design doc.

**What's genuinely new, because push is a structurally different problem
than pull**: this app must stand up and secure an inbound-facing SFTP
server (SFTPGo, per the ingress research doc's maintenance-currency
argument over `atmoz/sftp`) — the exact new Kubernetes `Service` type,
SSH-host-key Secret material, and "first inbound-facing backend service"
security posture the pull design avoided entirely and the ingress research
doc's original Section 4 already analyzed in full. And push introduces a
problem pull never had: the component that *receives* files (a generic SFTP
server) is necessarily different from the component that *verifies and
records* them (`schedule-ingest`) — this document resolves that concretely
with a single Kubernetes Pod, two containers, one shared `ReadWriteOnce`
PVC, rather than either merging unfamiliar logic into a third-party SFTP
image or reaching for a `ReadWriteMany` storage class this chart has never
needed before.

**What's most urgently unresolved**: whether push-side configuration is
even reachable for a normal Data Recipient, given pull's own portal access
turned out to be staff-gated and RSPS5046's text doesn't distinguish
pull-configuration from push-configuration within the same portal. That is
the concrete first question to put to RDG/DTD directly — not through the
portal, since portal access is exactly what's in question — before treating
any of this document's Helm/compose sketches as worth implementing.
Confirming that (open question 1) is the natural next step; per this
project's convention, that confirmation and any resulting implementation
plan are separate, later work, not part of this design document.
