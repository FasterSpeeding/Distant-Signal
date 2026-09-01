# Schedule-Feed Ingress: How Would This App Actually Receive RDM's Pushed Files? — Research Addendum

> **Read `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md`
> first if you want this app's current settled direction.** The
> "Addendum (2026-08-30)" section immediately below concluded SFTP *pull*
> was viable and became the new leading option; a 2026-09-01 finding (see
> this document's own "Addendum (2026-09-01)" section, appended after
> Addendum (2026-08-30)'s §7, before the `---` that starts the original
> pre-addendum body) ruled pull out entirely for this app. **Push is once
> again the settled mechanism** — meaning this document's *original*,
> pre-addendum sections (everything from "## What already exists in this
> codebase" onward) are the currently-correct premise, not the
> Addendum (2026-08-30) sections that follow this notice. This document is
> kept in full, in its original written order, for its research value
> (RSPS5046 citations, the real licence PDF findings, the real sample-data
> findings) — not as a table of contents for the currently-recommended
> path. The new document is the one place that reconciles all of this.

**Status: research/infrastructure-options only, not an approved design, and
not application code.** This document follows up specifically on Open
Question #2 of
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
("the base spec") — do not read this in place of it; it assumes the base
spec's problem statement, its "proceed with caveats, not yet" recommendation,
and its confirmed finding that CIF SCHEDULE/CORPUS are **push-only** RDM
file feeds (SFTP or a cloud storage bucket RDM pushes into — no
pull/on-request retrieval exists) as already-settled context. **This
document does not re-litigate whether the feature is worth building.** It
answers one narrower question the base spec explicitly declined to design:
*if* CIF SCHEDULE ingestion proceeds, how would this app practically stand
up the receiving side of that push, in a way that fits its two real
deployment shapes (`docker-compose.yml`, and the Helm chart at
`charts/distant-signal/`)?

Written to the same citation discipline as the base spec and
`docs/superpowers/specs/2026-08-18-helm-chart-design.md` (the closest
precedent for how this repo documents infrastructure decisions): every
external claim is attributed to a source, and anything this pass could not
verify is flagged as an open question rather than asserted.

**A tooling constraint that shaped this pass, stated plainly:** this
session's web-search budget was already exhausted before this research began
(a fixed per-session quota, unrelated to this task), so unlike the base
spec — which used search-engine-summarized citations of the Open Rail Data
Wiki — this pass could only use direct `WebFetch` of specific URLs already
known or guessed, plus this repository's own prior research documents. The
Open Rail Data Wiki's Rail Data Marketplace page returned HTTP 403 to a
direct fetch in this pass too (the base spec hit the same wall), and an
archive.org fallback was unavailable in this environment. Where this
matters for a specific claim, it is flagged inline rather than glossed
over.

## Addendum (2026-08-30): three new primary sources close most of this document's open questions

**This addendum was written after three new sources became available that
this document's original pass (above and below) did not have: (1) a real,
signed RDM licence PDF for the exact "Timetable" product
(`P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`), (2) a real ~711MB sample CIF
timetable extract (`timetable_full.zip`, 9 files, sampled directly — not
just the single `RJTTF942MCA.txt` byte-count already cited below), and (3)
RDG/RSP's own publicly-documented technical interface specification for
this exact feed, found via the RSP Accreditation ("ASSIST") site's public
documentation index.** Where a finding below contradicts something written
in the original sections (below this addendum), the original text is left
in place, not silently rewritten, per this doc's own citation discipline —
this addendum states plainly which specific claims are superseded and why.

### 1. The single biggest finding: this feed supports SFTP *pull*, not push-only

The original research below (citing the Open Rail Data Wiki, search-engine-
summarized) concluded CIF SCHEDULE/timetable file feeds are **push-only**:
"there's no supported mechanism to retrieve files from the RDM on
request." That conclusion is the load-bearing premise for this entire
document — it's why Sections 1-4 spend so much effort designing an
internet-facing SFTP receiver or a cloud bucket for RDM to push into.

**That premise does not hold for the specific product this app has a real
licence for.** RDG/RSP's own **RSPS5046 "Timetable Information Data Feed
Interface Specification"** (Subject Ref RSPS5046, Version P-04-02,
03-Jun-2025 — a public document, no accreditation or login needed to read
it, fetched directly from
`https://www.rspaccreditation.org/publicDocumentation.php#RSPS5046` and
its linked PDF in this pass) states, in two places:

> "The output feed files are distributed to those registered Data Feed
> recipients with appropriate entitlements via **SFTP Pull or Push**."
> (§3.1)

> "The DTD provides the following delivery methods for Registered Data
> Feed Users to receive their feeds: **SFTP Pull over the Internet from a
> publicly addressable and accessible SFTP server with the domain
> `dtd.atocrsp.org`**[, or] **SFTP Push over the Internet from the DTD's
> SFTP Client to the Data Recipient's SFTP Server**." (§7.1.2)

"DTD" is the **Data Transformation and Distribution Service**, "a service
owned by RDG" (§7.1.1) — the same Rail Delivery Group that is the Data
Publisher on this app's real licence PDF (see below). Recipients manage
their SFTP configuration — including choosing pull vs. push — via a
**separate portal**, `https://dtdportal.atocrsp.org/` (§7.5.1), distinct
from the Rail Data Marketplace (`raildata.org.uk`) portal used for
licensing. Concretely, this means: **RDM is the commercial/licensing
front end; DTD is the actual technical delivery mechanism underneath it
for this specific product family**, and DTD's own spec says outbound pull
is a real, supported, first-class option, not an exception.

**This is confirmed, not merely plausible, as the mechanism behind the
real sample data in hand**: RSPS5046 §5.2.2's own worked example of the
Contents/manifest file (`RJTTFnnn.DAT`) lists exactly
`RJTTF956.ZTR`/`.REJ`/`.SET`/`.FLF`/`.MCA`/`.MSN`/`.ALF`/`.TSI` — the same
eight-file pattern (differing only in sequence number, 956 vs. this app's
sample's 942) as the real `RJTTF942DAT.txt` manifest sampled directly from
`timetable_full.zip` in this pass:

```
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

And RSPS5046 §5.9's description of the `SET` file — "the file's data is
effectively redundant and permanently fixed with `UCFCATE`" — matches the
real `RJTTF942SET.txt` byte-for-byte: its entire payload is the single
token `UCFCATE`. This is about as strong a confirmation as this research
discipline gets: the real sample file's structure, the officially
published interface spec, and the RDM-brokered licence's product family
all line up as the same pipeline, not three merely-adjacent things.

**Why this changes the recommendation:** SFTP *pull* means this app opens
an **outbound** connection to a well-known, stable hostname
(`dtd.atocrsp.org`) on a schedule — structurally identical to the
`poller-*` pattern this app already runs everywhere else, just swapping an
HTTP client for an SFTP client. None of Section 1's LoadBalancer/NodePort
Service, SSH host-key generation/rotation, or "first inbound-facing
backend service" security posture from Section 4 is required for the pull
path — there is no listening daemon on this app's side at all. The
`atmoz/sftp`-vs-SFTPGo image comparison, the Helm `Service` type
discussion, and most of Section 4's "categorically different posture"
security analysis were solving a real problem *for the push variant*, but
pull was always available as an alternative RDG documents just as
plainly, and it is a dramatically smaller change for this app's
architecture. **The "SFTP vs. cloud bucket" framing in Sections 1-2 was
an incomplete menu**: SFTP pull, SFTP push, and a cloud bucket push are
three real options, not two, and pull is the one that requires this app
to build the least new infrastructure by a wide margin.

What's still genuinely unconfirmed about pull specifically: the exact
authentication handshake (password vs. public key vs. something else) for
a pull connection isn't stated in RSPS5046 — §7.5.1 just says "Data
Recipients can manage their SFTP Server configuration details using the
DTD Web Portal," implying credential/key setup happens there, not
documented at the protocol level in this spec. Getting a real DTD portal
account (a separate registration step from the RDM licence itself,
apparently) is the concrete next step to close this, not something this
document-research pass can resolve further.

### 2. What the licence PDF actually says — and a nuance the earlier validation-findings doc didn't have

`P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef.pdf`, read in full in this pass,
is a **Rail Data Marketplace "Data Sharing Agreement"** between "the Data
Publisher" (Rail Delivery Group, acting via Rail Settlement Plan Limited)
and "the Data Consumer" (this app's operator). Concrete terms, cited by
clause:

- **Fee: free, confirmed by Schedule 2** — "Licence Fee Type: **Open** —
  The Data Consumer can access the Licensed Data without charge," with
  Fair Usage Policy, Non-Chargeable Limit, and Charges-Following-Limit all
  marked "Not applicable," and Prepaid "No." No paid tier, no metered
  cost, no hidden cap.
- **Licensed product, by exact name and ID (Schedule 1 §3)**: "**Timetable
  - Full Refresh - Daily** (P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef)" —
  this is the literal RDM catalogue listing name, not an inference.
- **Permitted purpose (Schedule 1 §5)**: "The raw data may be used,
  copied, cleansed, adapted and/or aggregated with data from other sources
  for **research & analysis purposes only**."
- **Territory (Schedule 1 §7)**: "**UK and Europe**," no prohibited
  countries listed.
- **Term (Schedule 1 §6)**: 1 year, auto-renewing on each anniversary on
  the same terms, 1-month notice period for the Data Consumer to
  terminate for convenience.
- **Retention (Schedule 1 §9)**: "May retain any data which has been
  received" — no forced-deletion-on-termination obligation for this
  product.
- **Attribution (Schedule 1 §8)**: left blank for this product — no named
  organisation is currently listed as requiring credit if this data is
  "subsequently published," though Clause 3.3.1 of the main terms still
  imposes a general "give appropriate credit to the Data Publisher"
  obligation regardless of whether Schedule 1 names anyone specifically.
- **Liability (Clause 6.1)**: capped at "100% of the Licence Fees paid" —
  i.e., effectively zero, since the fee is zero. Data is provided "as is"
  (Clause 7.2), no accuracy warranty beyond "reasonable skill and care."
- **Governing law**: English law, exclusive jurisdiction of English
  courts (Clause 8.9).

**One important correction to the sibling validation-findings document**
(`2026-08-29-trust-schedule-delay-validation-findings.md`, Task 1): that
document characterizes its "Licence 1" as "**OGL v3.0** — free," for a
product named "**Darwin Timetable Files**" with RDM product ID
`P-9ca6bc7e-62e1-44d6-b93a-1616f7d2caf8` and permitted purpose "internal
business purposes only," territory "Global (minus sanctioned countries)."
**The PDF actually available to this pass is a different product ID**
(`P-1caaf2e8-...`, named "Timetable - Full Refresh - Daily") **with
different permitted-purpose wording and a different, narrower territory**
("UK and Europe" here vs. "Global minus sanctioned countries" there).
Neither this document's own text, nor the Data Sharing Agreement's boiler-
plate terms, actually use the phrase "Open Government Licence" or "OGL"
anywhere as a description of *this* agreement — the closest match is a
*definition*, in Clause 1, for a different concept: "Open Access Content"
(data that happens to arrive already under "Creative Commons, Open
Government 3.0 or similar" from some *other* rights holder folded into
the feed) — not a statement that this Agreement itself is an OGL licence.
This agreement is RDG's own bespoke "Data Sharing Agreement" contract
template, which happens to also land on "free, no cap" via its own
Schedule 2, not literally OGL v3.0.

**This means there are apparently two distinct real, signed RDM licences
in this app's paper trail for two distinct-but-similarly-named "Timetable"
products** — this research pass cannot reconcile which one (if either, or
both) is the one actually governing production use of the real
`timetable_full.zip` sample, since RDM product IDs, not display names, are
the authoritative identifier and the two documents show two different
IDs. **A concrete, previously-unflagged risk worth surfacing plainly**:
this pass's PDF's permitted-purpose wording — "research & analysis
purposes only" — is textually narrower than "internal business purposes,"
and arguably narrower than what a live, public-facing status-aggregation
feature (as opposed to an internal research exercise) would want to rely
on without a documented broader reading. This is not resolved by this
pass — it needs a direct question to RDG/RDM (or careful re-reading of
whichever licence is confirmed to be the operative one) before treating
"research & analysis" as covering production use, not treated here as
either a blocker or a non-issue.

### 3. What the real 711MB sample data revealed: nine files, not one — and what each one actually is

The base spec and the original sections below both discuss "the SCHEDULE
file" in the singular. **The real sample data is a 9-file bundle inside
one zip**, and this exactly matches RSPS5046 §4.2's own documented file
list for "the DTD CIF Timetable Data Feed" (quoting the spec's table
directly, matched line-for-line against what this pass sampled from
`timetable_full.zip` with `unzip -p ... | head -c N`, never extracting the
711MB to disk):

| File (real name in sample) | RSPS5046's stated contents | Confirmed against real bytes |
|---|---|---|
| `RJTTF942DAT.txt` (618B) | **Contents** manifest — lists every other file in this delivery, "except for the Contents file itself" | Yes — literal filename list, 8 entries, matching §5.2.2's own worked example format exactly |
| `RJTTF942MCA.txt` (707,743,886B) | **Full Basic Timetable Detail** — "Full CIF refresh file containing all timetable details in TTIS CIF format" | Yes — real `HD`/`TI`/`BS`/`BX`/`LO`/`LI`/`CR`/`LT`/`AA`/`ZZ` records, counted in full below |
| `RJTTF942REJ.txt` (246B) | **TTIS Reject File** — "records that have been rejected in the DTD processing before the MCA...files are created" | Yes — real content is literally an empty "Start/End of rejected trains file" pair; zero rejects in this real extract |
| `RJTTF942ZTR.txt` (2.9MB) | **Z-trains** — "Quasi-CIF format file containing details of bus and ferry transportation," always a full-refresh-only file, kept separate specifically "to avoid mixing daily updates with full files... and to avoid mixing CIF format and Quasi-CIF format files" | Yes — real content is CIF-shaped (`BS`/`BX`/`LO`/`LI`/`LT` records) but for bus-substitution-style services, not train schedules |
| `RJTTF942SET.txt` (499B) | **CIF Set Details** — "effectively redundant and permanently fixed with `UCFCATE`" | Yes — real payload is exactly the token `UCFCATE` |
| `RJTTF942FLF.txt` (101KB) | **Fixed Links** — "links between stations involving transfer by other than train," always a full-refresh-only file | Yes — real content is human-readable "ADDITIONAL LINK: WALK BETWEEN X AND Y IN N MINUTES" lines |
| `RJTTF942ALF.txt` (233KB) | **Additional Fixed Links** — same link data as FLF but machine-parseable, "including day/date/time variations" | Yes — real content is `M=WALK,O=...,D=...,T=...,S=...,E=...,P=...,R=...` CSV-shaped rows with time bands and a day-of-week run bitmask, richer than FLF's plain-English rows |
| `RJTTF942TSI.txt` (714B) | **TOC Specific Interchange Times** — "minimum interchange times at stations at which different minimum interchange times apply, depending on the TOC(s)" | Yes — real content is `CRS,TOC,TOC,minutes,(station name)` rows, e.g. `VIC,SE,SN,10,(London Victoria)` |
| `RJTTF942MSN.txt` (340KB) | **Master Station Names** — "Station details including Name, Location Codes, Interchange Suitability, Minimum Interchange Time, Map reference, Alias name etc.," always a full-refresh-only file | Yes — real content is fixed-width station rows keyed by name, TIPLOC, 3-alpha, NLC, etc. |

Two files RSPS5046 documents that are **not** present in this sample —
`RJTTCnnn.CFA` (Daily Updates to Timetable Detail, the incremental
counterpart to `MCA`) and the feed-level `.ZIP` wrapper itself — are
absent for a documented reason, not an anomaly: §4.2 states `CFA` is the
"CIF update file to be applied to Full Basic Timetable Detail," i.e. it
only exists in a *daily update* delivery, not a *full refresh* delivery
like the one sampled here (confirmed independently: this app's own
licensed product is literally named "Timetable - **Full Refresh** -
Daily," not an update-only product). **This directly corrects the base
spec's assumption of "a full weekly extract plus daily update extracts"
as the only two shapes** — per RSPS5046 §7.6/§7.7, this specific product
delivers a **full refresh every day** (not weekly), and a separate,
narrower update-only product (`CFA`-bearing) exists for consumers who
opted into daily-update instead of daily-full-refresh; §7.7 additionally
documents week/month-only refresh cadences as consumer-selectable options
distinct from both.

**Real per-file record-type breakdown for `RJTTF942MCA.txt`** (counted in
full over all 8,631,023 lines via `cut -c1-2 | sort | uniq -c`, not
sampled):

```
6,803,900  LI  (Intermediate calling point)
  488,798  BS  (Basic Schedule header)
  407,636  LT  (Schedule Terminate)
  407,636  LO  (Schedule Origin)
  407,636  BX  (Schedule Extra details)
   97,363  CR  (Change en route)
   12,085  TI  (TIPLOC Insert)
    5,967  AA  (Association)
        1  ZZ  (end-of-file terminator)
        1  HD  (file header)
```

This both confirms the record-type set the base spec cited from the wiki
(`HD`, `TI`, `AA`, `BS`/`BX`/`LO`/`LI`/`CR`/`LT`, `ZZ`) is complete and
correct for a real file, and adds one real, previously-undocumented
observation: **`TA`/`TD` (TIPLOC amend/delete) records never appear in
this full-refresh file** — only `TI` (insert) does. This is consistent
with RSPS5046 §5.3.5 discussing TIPLOC records only in terms of presence,
not amendment, and is the expected shape for a full refresh (a from-
scratch snapshot has no prior state to amend or delete against) — `TA`/
`TD` would be expected only in the `CFA` update-only file this sample
doesn't include, unconfirmed since no real `CFA` sample exists anywhere
in this app's research.

**A real, sourced explanation for why `BS` (488,798) doesn't equal `LO`/
`BX`/`LT` (407,636 each, identical to one another as expected — one of
each per schedule body)**: the gap, 81,162 records, is plausibly explained
by STP=`C` (Cancellation) schedules, which per the base spec's own already-
cited CIF Basic Schedule structure consist of a `BS` header only with no
`LO`/`LI`/`CR`/`LT` body — this is an inference from the documented STP-
overlay mechanism, not something RSPS5046 states explicitly about record
counts, so it's flagged as plausible, not confirmed.

**File-size anchor, now correcting the exact figure**: this pass measured
the real zip at **76,446,640 bytes compressed** (≈72.9 MiB / 76.4 MB
depending on unit convention — the earlier-cited "76MB" and "73MB" figures
elsewhere in this app's research are both right, just using different
units) and confirmed the **711,352,325-byte uncompressed total across all
9 files** (`unzip -l`'s own reported total), matching the previously-cited
"~711MB uncompressed" almost exactly and now sourced to the whole
manifest, not just the single `MCA` file's byte count. **This is no
longer merely "one sample of unconfirmed provenance"**: RSPS5046's own
naming convention (`RJTTFnnn.<ext>`, sequence-numbered, generated by
exporter `RjEhrTTT`) matches the real sample's filenames exactly, so this
is confirmed to be a genuine DTD-distributed Full Refresh feed instance,
not a differently-sourced or synthetic stand-in. Whether this exact size
recurs on every daily delivery (timetables do grow/shrink incrementally
over time) is still not something a single sample can establish — that
part of the original open question stands.

### 4. RSP Accreditation (RSPS5046's own host site) — read in full, and it is not a gate this app needs to pass

The task brief asked specifically whether RSPS5046 (or whatever's under
that anchor) implies this app needs its own accreditation through the RSP
Accreditation ("ASSIST") body to receive this feed. Two documents were
read in full from `rspaccreditation.org` to answer this directly, not by
assumption:

- **RSPS5046 itself** (the Timetable Information Data Feed Interface
  Specification, discussed above) is published on
  `rspaccreditation.org/publicDocumentation.php` — note "**public**
  Documentation" — and required no login, account, or accreditation of
  any kind to fetch and read in full. It is a **technical interface spec
  for consuming a data feed**, addressed to "Registered Data Feed
  recipients"/"Registered Data Feed Users" — a *registration* (via the DTD
  portal, to set up SFTP delivery) is implied, but nothing in this
  document describes an accreditation *process* (testing, sign-off,
  compliance review) for merely receiving the feed.
- **RSPA2000 "Ticket Issuing System Accreditation Guide"** (also hosted on
  the same site, downloaded and read in full in this pass, 42 pages) is
  the document that actually defines what "RDG Accreditation" means on
  this site, and it is unambiguous about its own scope: "**TOCs and
  third-party retailers are bound by the [Ticketing & Settlement
  Agreement], either directly or via their retail licence, to use an
  'approved' and Accredited TIS that complies with the criteria specified
  by RDG to retail industry rail products**" (§2.1.1). Its stated purpose
  is to "ensure passengers are able to purchase valid interoperable...
  tickets," "ensure accurate apportionment of revenue between the TOCs,"
  and "protect RDG systems such as the Reservation Service, LSM and PMS"
  (§1.1.2) — i.e., **this accreditation scheme governs organisations that
  sell rail tickets or otherwise participate in revenue settlement**, via
  a Ticket Issuing System, not organisations that merely ingest a
  published data feed for their own read-only use. Its intended audience
  is explicitly "TIS Suppliers," "Retailers seeking to procure a TIS," and
  "existing retailers... who already operate TIS" (§1.3) — none of which
  describes this app.

**Concrete answer to the task brief's question**: RSPS5046 is directly
and highly relevant to this app (it's the actual delivery-mechanism
documentation for the exact feed this app is researching — see finding
#1 above), but the *accreditation* half of "RSP Accreditation" is a
**separate, inapplicable scheme** for ticket retailing/revenue allocation
(RSPA2000/TIS), not a gate this app's read-only ingestion of a Timetable
Data Feed needs to pass. The site hosting both is shared infrastructure
(RDG's "ASSIST" documentation portal serves TIS accreditation material
*and* plain data-feed interface specs like RSPS5046/RSPS5047/RSPS5051
Darwin side-by-side), which is presumably why the task brief's framing
treated this as worth checking rather than assuming — it was a reasonable
thing to verify, and the verification comes back clean: **not relevant**.

### 5. Revised architecture recommendation: SFTP pull is the new leading option

Given finding #1, the rough architecture sketch later in this document
(the "Chart additions... SFTP path" section) should be read as describing
the **push** variant specifically, which remains a real, documented
RDG-supported option (RSPS5046 §7.1.2) but is no longer the *only* way to
receive this feed over SFTP. A **pull-based** variant is materially
simpler and fits this app's existing architecture better:

- **No new Kubernetes Service, no LoadBalancer/NodePort, no SSH host-key
  Secret material.** Nothing needs to accept an inbound connection. This
  removes essentially all of Section 4's "categorically different
  posture" concerns and Section 1's "Service type this chart has never
  rendered for production use" discussion — they simply don't apply to a
  pull-based watcher.
- **The watcher crate (Section 3) becomes an outbound SFTP client on a
  schedule** — closer in shape to `poller-*`'s HTTP client than anything
  else considered in this document: connect to `dtd.atocrsp.org`
  (confirmed literal hostname, RSPS5046 §7.1.2), authenticate (mechanism
  unconfirmed — see finding #1's caveat), list/download the current
  manifest (`RJTTFnnn.DAT`) and the files it names, matching the
  9-file structure from finding #3 (the watcher must fetch and correctly
  associate *all* files a manifest lists — not just one — before treating
  a delivery as complete; RSPS5046 §7.2.2 itself puts this exact
  responsibility on the recipient: "the Data Recipient should ensure that
  all files in the manifest file are present... it is the Data
  Recipients' responsibility to process the files according to their
  requirements").
- **Cadence/timing is now a documented fact, not a guess**: RSPS5046
  §7.3.1 states normal daily distribution happens "at around 10.30pm to
  1am," with a hard latest-distribution time of 4pm (§7.3.2) — if DTD
  itself is running late or has an upstream failure, it ships an "Empty"
  feed (the previous full refresh's files, re-sent, plus empty update
  files) by 4pm rather than nothing at all. This directly informs the
  "same-day processing guarantees" open question the original Section 5
  flagged as unanswerable — it is now answered, for this specific
  product: **expect delivery between ~22:30 and 01:00, with a documented
  worst-case fallback by 16:00**, which should size the watcher's poll
  window/schedule concretely rather than an arbitrary "every 15-60
  minutes" guess.
- **A resilience feature this document didn't know existed**: RSPS5046
  §7.5.2 states "Data Recipients that require a resilient service can set
  up two SFTP servers and the DTD will distribute... to both" — relevant
  if this app ever wants push-side redundancy, though pull sidesteps the
  need for this entirely (there's nothing to fail over on this app's
  side; DTD's own §7.5.3-documented server-side resilience is DTD's
  problem, not this app's).
- **The credentials/firewall gap narrows, but doesn't close**: RSPS5046
  §7.5.4 states IP addresses (for firewalling either direction) are
  available "using the web portal" — meaning a real, named mechanism
  exists to get this information, unlike the original research's dead
  end on this exact question, but actually obtaining the number still
  requires a DTD portal account this research pass doesn't have.

**This does not overturn the "proceed with caveats, not yet" verdict** the
base spec reached, nor invalidate the general "which of SFTP-push/cloud-
bucket/self-hosted" analysis in Sections 1-2 for consumers of a genuinely
push-only RDM feed — but it does mean that **for the specific, real,
licensed product this app has in hand, the practical recommendation
changes**: prototype the pull path first. It is less new infrastructure,
lower security exposure, and a closer architectural fit than either
option this document originally treated as exhaustive.

### 6. Open questions this addendum resolves, narrows, or leaves standing

**Resolved:**

- Licence/cost tier (base spec's Open Question #1, this document's own
  framing throughout): **free, confirmed directly from the signed PDF**,
  no fair-usage cap, no paid tier — for the specific product ID
  `P-1caaf2e8-...` ("Timetable - Full Refresh - Daily").
- Whether SCHEDULE/timetable file feeds are push-only: **no** — SFTP pull
  from `dtd.atocrsp.org` is a real, RDG-documented, first-class delivery
  option for this product family, per RSPS5046 §3.1/§7.1.2.
- Whether RSP Accreditation is a prerequisite for this app: **no** — that
  scheme (RSPA2000/TIS) governs ticket retailing and revenue settlement,
  not read-only data-feed consumption; RSPS5046 itself required no
  accreditation to access.
- The ~711MB file-size anchor's provenance: **confirmed as a genuine
  DTD-distributed Full Refresh delivery**, matching RSPS5046's own
  documented file-naming and manifest format exactly, not merely "a
  sample of unconfirmed origin."
- The "single SCHEDULE file" assumption: **corrected** — it is a 9-file
  bundle (`DAT`/`MCA`/`REJ`/`ZTR`/`SET`/`FLF`/`ALF`/`TSI`/`MSN`), each with
  a distinct, now-documented purpose; a full-refresh delivery additionally
  never includes the update-only `CFA` file.
- Same-day delivery timing: **documented** — normally 22:30-01:00, worst-
  case fallback by 16:00 via an "Empty" feed.

**Narrowed, not fully resolved:**

- **Pull-connection authentication mechanism** — confirmed that
  credentials are managed via the DTD Web Portal (`dtdportal.atocrsp.org`,
  separate from the RDM licensing portal), but the specific mechanism
  (password/public-key/other) is not stated in RSPS5046 and needs a real
  portal account to confirm. **Still true after §7's second-source check
  below**: a full-text search of the complete 39-page document turned up
  no mention of "password" or "key" in this context at all.
- **Fixed IP ranges for firewalling** — confirmed a mechanism exists to
  obtain them (the DTD Web Portal, per §7.5.4), narrowing this from "no
  known source" to "known source, not yet accessed." **Still true after
  §7's second-source check**: no IP address or range appears anywhere in
  the document's 39 pages; §7.5.3 does add that DTD states it preserves
  "the same domain and IP address" through its own server-side failover,
  which is new context but not the number itself.
- **Which of the two differently-worded, differently-numbered RDM
  licences (this pass's `P-1caaf2e8-...` "Timetable - Full Refresh -
  Daily," vs. the validation-findings document's `P-9ca6bc7e-...` "Darwin
  Timetable Files") actually governs the real sample data, and whether
  "research & analysis purposes only" (this pass's PDF) is broad enough
  to cover a live, public-facing product feature** — this is new, and it
  is a real risk to flag plainly, not merely a licensing-tier question:
  confirm directly with RDG/RDM before relying on the "research & analysis
  purposes only" wording to justify production use.

**Genuinely still unresolved, honestly:**

- No RDM/DTD account access exists in this research pass, so nothing
  requiring an actual portal login (exact IP ranges, real credential
  provisioning flow, whether pull is actually offered/enabled for *this*
  specific licensed product rather than being a generic DTD-wide
  capability) could be directly confirmed end-to-end. RSPS5046 documents
  DTD's delivery mechanism as a service-wide capability, but this pass has
  no way to confirm this app's specific RDM subscription has pull enabled
  or configured.
- Whether a **daily-update-only** (`CFA`-bearing) delivery has a
  meaningfully smaller size than the ~711MB full-refresh anchor is still
  unconfirmed — no real sample of that file type exists anywhere in this
  app's research, this pass included.
- The exact SFTP authentication handshake and firewall/IP details
  (immediately above) remain open pending real portal access. **A second
  document, checked after this list was first written and reported by the
  app owner as "the documentation for the dtd portal," turned out to be
  the identical RSPS5046 document already mined for this whole section —
  see §7 immediately below. It does not close this gap; it confirms the
  gap is real by ruling the answer out of the only document available,
  rather than leaving open the possibility the first fetch simply missed
  it.**

### 7. A second source check (same day): the app owner's "DTD portal documentation" URL turned out to be the same RSPS5046 document already mined above — confirmed by direct comparison, not assumed

After the sections above were written, the app owner supplied a further URL,
described as "the documentation for the dtd portal" and "the only
information I have access to right now":
`https://www.rspaccreditation.org/downloadPublic.php?did=c5VkXAQOgMj8q024cALYymTpxTFaroiwLL7mvDA0A3UB5FJKuO`.

**What the URL actually serves**: a raw PDF binary (`application/pdf`,
~708KB). A direct `WebFetch` of the URL could not read it — `WebFetch`
converts HTML to markdown and returned only the raw, undecoded PDF byte
stream (`%PDF-1.7` structure markers, `FlateDecode` object streams) rather
than text. The PDF itself was recovered from `WebFetch`'s own saved copy
and its text extracted directly in this pass with `pypdf` (Python), reading
all 39 pages in full — not sampled, not OCR'd, not summarized by an
intermediate model.

**The document this URL serves is, byte-for-byte in every substantive
respect, the same RSPS5046 "Timetable Information Data Feed Interface
Specification" already read in full and extensively cited throughout
sections 1-6 of this addendum** (via a different route,
`rspaccreditation.org/publicDocumentation.php#RSPS5046` and its linked
PDF). Confirmed by direct comparison of the extracted text against the
sections already quoted above: identical title, identical "Subject Ref:
RSPS5046", identical "Version: P-04-02", identical date "03-Jun-2025",
identical page count (39 pages), identical author metadata ("Draft A"
via Microsoft Word), and — checked line-by-line — identical wording for
every clause already quoted in sections 1-6 above: §3.1's "SFTP Pull or
Push," §7.1.2's `dtd.atocrsp.org` delivery-method text, §7.2.2's manifest-
completeness responsibility, §7.3.1/§7.3.2's distribution-window and 4pm-
fallback text, §7.5.1's and §7.5.4's DTD Web Portal
(`https://dtdportal.atocrsp.org/`) references, and §7.5.2's resilient-
delivery text. **This is not a new primary source — it is the same
document, reached by a second URL the app owner had independent access
to.** This is worth stating plainly rather than silently treating it as
confirmatory padding: it means this pass could not add a fourth
independent source to the three already listed at the top of this
addendum, only re-verify the third one via its full original text rather
than the fetch this addendum's earlier sections had to work from.

**Because it is the same document, it also does not close the open
questions those earlier sections already identified as document-level
gaps** — this pass searched the complete 39-page extracted text
specifically for these three points and confirms they are absent:

- **No authentication mechanism is named anywhere in the document.** The
  words "password" and "key" do not appear in connection with SFTP login
  at all; the only relevant text remains §7.5.1's "Data Recipients can
  manage their SFTP Server configuration details using the DTD Web Portal
  at https://dtdportal.atocrsp.org/" — configuration happens in a portal
  this document doesn't describe the contents of, not in the interface
  spec itself.
- **No IP address or IP range appears anywhere in the document.** §7.5.4
  states, verbatim: "Data Recipients should use the web portal at
  https://dtdportal.atocrsp.org/ for the IP address of the DTD SFTP Server
  or Client if firewall configuration is required" — again pointing to the
  portal, not publishing a number itself.
- **No DTD portal account registration process, approval lag, cost, or
  account-tier information appears anywhere in the document.** This is a
  pure technical interface specification (file formats, record layouts,
  distribution scheduling); it has no "how to sign up" section, consistent
  with account provisioning being a separate step handled through
  `dtdportal.atocrsp.org` itself (or whatever process leads to being
  registered there) rather than documented in RSPS5046.

**What this second pass over the full text does add — three genuinely new,
previously-unquoted operational details, all supporting the "SFTP pull
fits the `poller-*` shape" recommendation already reached in section 5**:

- **§7.5.3 (not previously quoted): DTD's own server-side resilience keeps
  a stable address across failover.** Verbatim: "The DTD SFTP service is a
  resilient service. If the infrastructure on which the service fails, the
  DTD will automatically start up another SFTP server instance on an
  alternative server **at the same domain and IP address**." This means
  `dtd.atocrsp.org` isn't merely a stable hostname in the DNS sense — DTD
  states it preserves the same IP address through its own failover, which
  is a relevant (if incomplete — the number itself is still unpublished)
  data point for anyone eventually configuring outbound firewall rules
  around a single pinned address rather than a range.
- **§7.4 (not previously quoted): a documented resumption procedure after
  an "Empty" feed.** Verbatim: "In circumstances where one or more 'Empty'
  feeds have been distributed, DTD may need to provide more than one feed
  in a 24-hour period. This will not be done without contacting Data
  Recipients to arrange the scheduling of feeds in accordance with their
  systems requirements. Data Recipients that are unable to process more
  than one feed in a 24-hour period would resume with a Full Refresh Feed
  and the sequence number of this Full Refresh will not necessarily be
  contiguous from the last feed sequence." Two concrete implications for
  the watcher design in section 3: (1) the manifest sequence number
  (`nnn` in `RJTTFnnn.*`) is **not** safe to treat as strictly contiguous —
  a gap does not by itself indicate a missed delivery; and (2) DTD's stated
  practice is to *contact* recipients directly before sending more than one
  feed a day, meaning a human/support relationship with DTD exists
  alongside the automated pull, not just an unattended machine-to-machine
  channel.
- **§7.6.1 and §7.7 (previously described only generically in section 3's
  finding, now quoted exactly): the specific bootstrap and non-daily
  cadence rules.** "New Daily Recipients that begin the service will be
  provided with a full refresh of timetable data" (§7.6.1) — confirms the
  watcher's first-ever pull should expect (and must handle) a full refresh
  regardless of which day it starts on. For consumers who choose a
  non-daily cadence instead of this app's actual daily-full-refresh
  product: "Data Recipients that choose to receive weekly timetable feeds
  will receive a full refresh of timetable data **each Wednesday** of each
  week" (§7.7.1) and "...monthly timetable feeds will receive a full
  refresh...on the **first Wednesday** of each period" (§7.7.2) — the
  specific weekday wasn't captured when section 3's finding described this
  generically as "week/month-only refresh cadences."

**Bottom line for this second-pass check**: the previously-open questions —
exact SFTP pull authentication mechanism, and fixed IP ranges for
firewalling — remain genuinely open after this document as well, for the
same reason stated in section 1's original caveat: RSPS5046 is explicit,
in the interface-spec's own words, that both details live inside the DTD
Web Portal (`https://dtdportal.atocrsp.org/`), not in this document. A
real DTD portal account is still the only way to close them; no document
reachable by this app's owner today substitutes for that. This does not
change the "proceed with caveats, not yet" verdict, and does not add any
new caveat beyond confirming, more thoroughly than the first pass could,
that the gap is real rather than an artifact of an incomplete fetch.

## Addendum (2026-09-01): pull ruled out by the repo owner — push is the settled mechanism again

**Source: the repo owner, 2026-09-01.** Stated directly, not independently
discovered via `WebFetch`/`WebSearch` in this pass — per this document's own
citation discipline, this is attributed to its source rather than presented
as something this research verified itself: **SFTP Pull access via the DTD
portal (`dtdportal.atocrsp.org`) is staff-only** — gated behind an RDG/RSP
staff account or equivalent internal access this app's operator does not
have and cannot get, not something a normal registered Data Recipient can
self-serve. This is a *permanent* structural blocker, not the "application
pending" state Addendum (2026-08-30) §6 and the sibling pull-design
document's "Open questions" section both left open.

**What this resolves, from Addendum (2026-08-30) §6's own three buckets:**

- **"Genuinely still unresolved" item 1** ("whether pull is actually
  offered/enabled for *this* specific licensed product... this pass has no
  way to confirm") is now resolved, negatively: it doesn't matter whether
  DTD-the-service offers pull in the abstract (RSPS5046 §7.1.2 still
  documents it as a real, general DTD capability) — this app's operator has
  no path to the portal access pull requires, regardless.
- **Addendum (2026-08-30) §5's "Revised architecture recommendation: SFTP
  pull is the new leading option" no longer holds.** Every advantage that
  section credited to pull (no new Kubernetes `Service`, no SSH host-key
  Secret material, no inbound-facing daemon) was contingent on pull being
  reachable at all. It isn't.
- **The pre-addendum sections of this document (Sections 1-5 below,
  originally written under a push-only assumption, before either addendum
  existed) are the correct premise again.** They were never wrong about
  push's mechanics — only incomplete about pull existing as an alternative,
  and that alternative has now been closed off.

**One correction this pass adds, from re-reading RSPS5046's full text
directly** (the same local PDF Addendum (2026-08-30) fetched, re-read in
full again in this pass, specifically keyword-searched for
`bucket`/`S3`/`Amazon`/`Azure`/`AWS`/`cloud`/`Google`, all with zero
matches): **RSPS5046 — the interface spec for the exact "Timetable - Full
Refresh - Daily" product this app has a real, signed licence for — documents
exactly two delivery methods, SFTP Pull and SFTP Push (§7.1.2), and does not
mention a cloud-storage-bucket option anywhere in its 39 pages.** Section 2
below's "cloud storage bucket push — the alternative RDM explicitly
supports" framing is sourced from the Open Rail Data Wiki's *generic*
description of RDM file feeds broadly ("File feeds can be transferred via
'push' options to major cloud providers... or via SFTP," quoted in the base
spec) — not from this specific product's own authoritative interface spec.
**This narrows, for this specific licensed product, "SFTP vs. cloud bucket"
down to "SFTP push, confirmed by a primary source; cloud bucket,
unconfirmed for this product specifically, plausible only by inference from
a more general wiki description of RDM file feeds as a category."** Treat
Section 2 below as still-useful general reasoning (the operational-burden
comparison, the self-hostability argument), not as evidence this app's
actual DTD-delivered feed supports it.

**This document's SFTP-push sections (1, 4, and the SFTP-path halves of 3
and the architecture sketch) are the ones that now matter.** See
`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` for the
concrete design that reconciles this finding against everything both
addenda and the (now-superseded-for-its-core-premise)
`2026-08-30-schedule-feed-sftp-pull-design.md` learned in the meantime —
the real manifest format, file-size anchors, retention math, gap-detection
logic, and database-bookkeeping approach that document worked out for pull
carry over to push almost unchanged, since all of that is mechanism-
agnostic (it's about what arrives, not how it arrives).

---

## What already exists in this codebase — confirmed, not assumed

Grepped directly for this pass:

```
grep -rniE "aws-sdk|rusoto|azure_storage|azure-storage|google-cloud|gcs|s3|blob" --include="Cargo.toml" .
grep -rniE "aws_sdk|rusoto|azure_storage|google_cloud|s3::|GCS|s3_bucket" --include="*.rs" crates/
grep -rniE "sftp|ssh2|russh|libssh" --include="*.toml" --include="*.rs" .
```

All three return **zero matches**. This codebase today has no cloud object-
storage SDK dependency of any kind, and no SFTP/SSH client or server
library. Every existing feed integration is either an outbound HTTP GET
(`poller-*`, via `reqwest`) or an outbound Kafka consumer connection
(`trust-consumer`, via `rdkafka`) — both are connections *this app*
initiates. Nothing in the current architecture accepts an unsolicited
inbound connection from the internet. This confirms the base spec's own
framing of file-push ingestion as "a third, genuinely new ingestion shape,"
empirically, not just by inference from the wiki's description.

Also confirmed by reading `charts/distant-signal/templates/*.yaml`: the
chart's only Kubernetes `Service` objects today are `ClusterIP` (postgres,
redis, api, frontend) plus exactly one `NodePort` — `devauthentik-
service.yaml`, gated behind `devAuthentik.enabled` and explicitly a
local/dev-only component per its own values.yaml comments (no bootstrap
admin credentials, blueprint-provisioned — see
`docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md`). The chart's
single `ingress.yaml` renders a plain `networking.k8s.io/v1` `Ingress`
carrying only HTTP(S) host-based rules to the frontend and api Services —
**it cannot carry raw TCP traffic on an arbitrary port like SFTP's 22**.
Nothing in this chart today provisions a `type: LoadBalancer` Service or any
other production-facing non-HTTP ingress path. Whatever exposes an SFTP
listener to RDM's infrastructure would be the **first** component of this
kind in the chart.

`charts/distant-signal/templates/networkpolicy.yaml` (read in full) governs
only in-cluster, pod-to-pod traffic: every rule there is `podSelector`/
`namespaceSelector`-scoped, egress is left explicitly unrestricted (its own
comment: constraining egress "would require operators to enumerate" every
external RDM host), and there is no mechanism anywhere in this resource for
restricting *external internet* source IPs reaching a `LoadBalancer` or
`NodePort` Service — that control point, if it exists at all, sits at the
cloud provider's security-group/firewall layer, entirely outside anything
this chart renders today.

## 1. SFTP receiving-endpoint options

### Candidate images

**`atmoz/sftp`** (Docker Hub, fetched directly for this pass) — a thin
wrapper around OpenSSH's own `sftp-server`, configured via a `SFTP_USERS`
env var, command arguments, or a mounted `/etc/sftp/users.conf`, supporting
per-user password or public-key auth, custom uid/gid, and automatic home-
directory creation. Users are chrooted to their home directory (cannot
create files at the home root itself — a subdirectory must be mounted
inside it). SSH host keys are freshly generated on first container start
unless operator-supplied keys are mounted at `/etc/ssh/ssh_host_*`. Two
tags: `debian` (~68.1MB, per the fetched page) and a smaller `alpine`
variant. The fetched page states it has been pulled over 1 billion times
but was **last updated approximately 2 years ago** — worth flagging as a
maintenance-currency concern for a component that would sit on the public
internet, not confirmed further in this pass (e.g. whether it still
receives OpenSSH CVE-driven rebuilds).
[atmoz/sftp on Docker Hub](https://hub.docker.com/r/atmoz/sftp)

**SFTPGo** (`drakkan/sftpgo` on GitHub, fetched directly for this pass) — a
more full-featured, actively maintained ("event-driven file transfer
solution," 12.5k GitHub stars, AGPLv3 community edition plus a commercial
enterprise edition, per the fetched page) server supporting SFTP, FTP/S,
WebDAV and HTTP/S on the same virtual-user model, with pluggable storage
backends: local filesystem, S3-compatible object storage, Google Cloud
Storage, Azure Blob Storage, or even a remote SFTP target. Ships standard,
Alpine and distroless Dockerfiles. **This is notable for this specific
research question**: SFTPGo could receive an SFTP push from RDM and write
directly into a cloud bucket backend, meaning the "SFTP vs. cloud bucket"
choice in section 2 below is not strictly either/or — SFTPGo is a way to
present an SFTP front end to RDM while getting the storage-backend
properties of the bucket path underneath. The fetched page did not surface
Helm-chart or Kubernetes-specific deployment guidance directly, so whether
an official chart exists is an open question below; it is at minimum
containerized, so it fits this chart's existing pattern of adding a
bespoke Deployment + Service, same as `devAuthentik.*` does for a chart
that likewise has no upstream Helm chart of its own.
[drakkan/sftpgo on GitHub](https://github.com/drakkan/sftpgo)

### How it would fit this chart's existing pattern

The closest, most literal precedent in this repo is
`devauthentik-postgres-statefulset.yaml` / `devauthentik-server-
deployment.yaml`: a values block (`devAuthentik.*`) gating a whole optional
subsystem, its own Secret entries (`devauthentik-secret.yaml`), its own
Service, and a PVC for the stateful half. A schedule-feed receiver would
follow the same shape (a new `scheduleFeed.*` — or similar — values block),
with three differences from that precedent worth calling out:

- **Deployment, not StatefulSet.** The devAuthentik Postgres StatefulSet
  uses `volumeClaimTemplates` because StatefulSets give stable per-replica
  identity, which matters for a *clustered* database. An SFTP receiver here
  is a singleton (RDM pushes to one endpoint; there is no clustering story),
  which matches this chart's existing `aggregator-deployment.yaml` pattern
  more closely: `replicas: 1` (fixed, not a value) and
  `strategy: Recreate` — the same rationale the base helm-chart design doc
  gives for the aggregator ("a singleton write loop... a rolling update
  would briefly run two copies") applies just as directly to an SFTP daemon
  writing into one shared directory: two replicas racing to accept
  connections and write files to the same PVC is a correctness risk with no
  offsetting benefit, since there is nothing to horizontally scale. A plain
  pre-declared `PersistentVolumeClaim` resource (not a `volumeClaimTemplate`)
  mounted into that single Deployment is simpler than a StatefulSet for this
  shape.
- **New Secret material this chart has never held: SSH host keys.** Every
  existing secret in `secret.yaml`/`devauthentik-secret.yaml` is a
  password or bearer token — flat strings, generated with `randAlphaNum 32`
  and preserved across upgrades via the chart's documented lookup-preserve
  pattern (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s
  Secrets section). An SSH host keypair is structurally different (an
  RSA/Ed25519 keypair, not a random string) and Helm's `genPrivateKey`
  Sprig function is the plausible mechanism, but this was **not tested
  against Helm 4 in this pass** — the base helm-chart design doc already
  found one Helm-4-specific gotcha in the existing lookup-preserve pattern
  (`$existingData` normalisation), so a new secret-generation shape should
  be verified against real `helm template`/`helm upgrade` behaviour before
  being trusted, not assumed to work by analogy. Getting this wrong means
  RDM (or a human validating the connection) sees the SSH host key change
  on every pod reschedule, which for a real SFTP client is at minimum a
  scary warning and at worst a hard connection failure depending on how
  strict host-key checking is configured on RDM's end (unconfirmed how RDM
  configures this from its side — open question below).
- **A Service type this chart has never rendered for production use.**
  `devauthentik-service.yaml`'s `NodePort` is the only non-`ClusterIP`
  Service in the chart today, and it exists purely for local-dev
  reachability of a component the values.yaml header comments describe as
  dev-only. Exposing SFTP to RDM's real infrastructure needs either a
  `type: LoadBalancer` Service (the standard way a cloud-managed Kubernetes
  cluster gets a stable externally-routable IP for a non-HTTP TCP port) or
  a `NodePort` fronted by an operator-managed external load balancer/DNS
  record pointing at node IPs. Either way this is architecturally a
  **sibling** to `ingress.yaml`, not an extension of it — `Ingress`
  resources are HTTP(S)-only by the `networking.k8s.io/v1` spec itself, not
  a limitation specific to this chart.

### Credentials, host keys, static IP/hostname

- **Credentials.** Both candidate images support either password or
  public-key SFTP authentication scoped to one virtual/chrooted user.
  Public-key-only auth for RDM's account (no password fallback) is the
  safer default if RDM's push configuration supports supplying/registering
  a public key on its end — **unconfirmed**, since that configuration lives
  inside RDM's own portal, which requires an account this research pass
  does not have (same limitation the base spec hit for RDM's Kafka product
  listing).
- **Host key provisioning/rotation.** Generate once, persist in a Secret,
  mount into the container at `/etc/ssh/ssh_host_*` (per `atmoz/sftp`'s own
  documented mechanism) so the fingerprint is stable across pod restarts
  and Helm upgrades. Rotation (deliberately changing the key) is an
  operator action with real coordination cost — RDM's side would need to
  accept the new fingerprint — so this should be a rare, deliberate
  operation, not something the chart auto-rotates.
- **Static IP/hostname for RDM to push to.** Whether RDM's push
  configuration wants a static IP, a stable hostname (which can sit behind
  a changing IP via DNS), or something else entirely (e.g. RDM might
  require the destination to be *pre-registered* and validated before it
  will push, the way many enterprise SFTP-push integrations work) is
  **unconfirmed in this pass** — this is exactly the kind of RDM-portal-
  gated detail the base spec already flagged it could not access. What is
  confirmed, from Kubernetes' own model: a `LoadBalancer` Service's
  external IP is not guaranteed stable across Service recreation on every
  cloud provider (behaviour varies; some providers preserve it, some
  don't), so if RDM does require a static destination, the chart (or the
  operator) would need a stable hostname (via external-dns or manual DNS
  pointed at the LB) rather than relying on the raw IP, or a cloud
  provider's "static/reserved IP" feature bound to the LB — neither
  currently modeled anywhere in this chart.
- **Firewall/allowlisting.** Whether RDM publishes fixed outbound source-IP
  ranges for its push infrastructure (which would let an operator restrict
  the LoadBalancer/security-group to just those ranges, meaningfully
  shrinking the internet-facing attack surface) was **not found in any
  source accessible to this pass** — the relevant wiki page 403'd on direct
  fetch and no web-search budget remained this session to search further.
  This is a genuinely important open question, not a minor one: without a
  published RDM IP range, the SFTP port is exposed to the entire internet,
  relying on SSH auth alone as the security boundary.

### Persistent storage

The PVC pattern is a direct, low-risk reuse of what this chart already
does: `values.yaml`'s existing `postgresql.persistence.{enabled,size,
storageClass}` shape (and the identically-shaped `devAuthentik.postgresql.
persistence` block) is the template — `accessModes: [ReadWriteOnce]` is
sufficient here too, since (per the Deployment-not-StatefulSet reasoning
above) only one pod ever mounts it. Sizing depends on the retention policy
in section 5, not on any Postgres-specific consideration (no `PGDATA`
subdirectory gotcha applies — this isn't a database data directory).

## 2. Cloud storage bucket push — the alternative RDM explicitly supports

Confirmed (see "What already exists" above): this app has zero existing
cloud SDK dependency of any kind, in any crate. Adopting the bucket path
would be a genuinely new category of dependency for this codebase, not an
extension of anything already there — same magnitude of a first as
`rdkafka` was when `trust-consumer` was first built, per the base spec's
own framing.

### Operational-burden comparison

**Running SFTP ourselves** means owning, indefinitely:

- A network daemon exposed to the internet, needing its own patch cadence
  independent of this app's own release cycle (an OpenSSH/SFTP CVE is this
  app's operational problem the moment the port is open, regardless of
  whether any application code changed) — a materially different
  maintenance burden than a `poller-*` crate's `Cargo.toml` dependency
  bumps, because it's server software accepting untrusted-until-
  authenticated inbound connections, not a library this app calls out to.
- The new Secret/host-key management burden above.
- A cloud-provider (or bare-metal) LoadBalancer/NodePort + firewall/
  security-group configuration this chart has never needed before.
- The PVC and its own storage-class/backup considerations.

**Provisioning a cloud bucket** shifts the burden differently:

- No daemon this app operates is exposed to the internet — RDM pushes
  directly into a cloud provider's own object-storage API (S3, Azure Blob,
  or GCS), which is a professionally operated, continuously patched surface
  none of this app's own infrastructure has to defend.
- The new moving part on *this app's* side becomes **credentials to read
  from (not write to) that bucket** — an IAM user/role/service-principal
  scoped, ideally, to read-only (plus delete, if this app's own watcher
  handles cleanup) access on exactly that one bucket/prefix. This is much
  closer in shape to an outbound, pull-based `poller-*`-style credential
  than to a network daemon — the watcher component (section 3) becomes
  "poll a bucket's object listing on an interval," structurally similar to
  how `poller-incidents` etc. already poll an HTTP endpoint on an interval,
  just swapping the client library.
- Ongoing burden shifts toward the cloud bill (object storage + request
  costs for the data volumes in section 5 are almost certainly small, but
  this pass makes **no invented pricing claim** — check current AWS S3 /
  Azure Blob / GCS pricing directly before budgeting) and toward periodic
  IAM credential rotation, not toward patching a bespoke SSH daemon.
- **A real, unconfirmed cost variable**: if this app's own compute doesn't
  run in the same cloud/region as the bucket, cross-cloud/cross-region
  egress when the watcher reads the file back out could be a non-trivial,
  recurring cost — this pass has no information on where this app would
  actually be deployed relative to any specific cloud provider's regions,
  so this is flagged as an open question, not sized.

### Which is genuinely simpler for a small team — a judgment call, stated as one

For a team **already using, or willing to adopt, one of AWS/Azure/GCS**,
the bucket path is very likely the lower ongoing-maintenance option: it
turns this into "poll a bucket" (a shape this app already knows how to
build and operate, per `poller-*`) rather than "run and secure a bespoke
internet-facing SSH daemon" (a shape this app has never built, and whose
failure modes — CVEs, host-key mismanagement, credential/chroot
misconfiguration — are unfamiliar territory here).

Against that: this repository's whole existing posture is deliberately
**cloud-provider-agnostic and self-hostable** — Postgres, Redis, and the
Kafka broker `trust-consumer` talks to are all either self-run by the
chart or pointed at *any* provider via a plain connection string/broker
address, and nothing today assumes AWS, Azure, or GCS specifically. Taking
the bucket path is not merely an operational choice, it is also a
**philosophical one**: it would be this app's first hard (or at minimum
first strongly-encouraged) dependency on a specific major cloud provider's
proprietary API, a departure from the "installable in an air-gapped
cluster given the images" goal `docs/superpowers/specs/2026-08-18-helm-
chart-design.md` states for the whole chart. Running SFTP ourselves,
despite the larger ops burden, keeps that self-hostable posture intact —
it is "just" another container in the same chart, with no external account
or proprietary API dependency, matching how every other backing service in
this chart works today (including, notably, SFTPGo's own bucket-backend
option from section 1, which could deliver *both* properties at once: an
SFTP front end that satisfies "stay self-hostable, no proprietary API
dependency for the receiving edge," writing to local PVC storage — the
cloud-bucket *backend* mode of SFTPGo is a distinct, separable decision
from whether RDM talks SFTP or a cloud API directly).

**No single "correct" answer without more input.** If the answer to "does
this team already operate in one of AWS/Azure/GCS for other reasons" is
yes, the bucket path is probably the pragmatic choice. If the answer is
no (this repo's other infra choices suggest it might be), self-run SFTP —
ideally via SFTPGo rather than the more stagnant-looking `atmoz/sftp`,
given its more active maintenance per section 1 — is the option that
doesn't introduce a new external dependency class.

## 3. The watcher component

Three shapes considered, per the task brief:

**(a) Polling a mounted volume/bucket on an interval.** Structurally
identical to every existing `poller-*` crate: a `clap`-derived `Config`
(broker/endpoint, credentials, poll interval, ingest target), a `tokio`
interval loop, and — for the bucket variant — a cloud SDK `list_objects`-
style call instead of an HTTP GET; for the SFTP/PVC variant, a directory
scan (`std::fs::read_dir`, comparing filenames/mtimes/checksums against
what's already been ingested).

**(b) inotify/filesystem-event-driven (only applicable to the PVC/SFTP
path).** Checked the `notify` crate specifically (Rust's standard
cross-platform filesystem-event library), fetched directly from
`docs.rs` for this pass. Its own documentation states two findings that
argue directly against this shape for this app's Kubernetes deployment:
"network mounted filesystems like NFS may not emit any events for notify
to listen to" (many Kubernetes storage classes — anything backed by NFS,
EFS, Azure Files, and similar network-backed CSI drivers — are exactly
this), and separately that the Linux inotify backend can hit "max-files
watched limits," producing "Bad File Descriptor / No space left on device"
errors in production, with the crate's own documented recommendation being
to fall back to its `PollWatcher` (i.e., polling) for exactly these
unreliable cases.
[notify crate docs on docs.rs](https://docs.rs/notify/latest/notify/)
This is a real, sourced reason to prefer polling over event-driven
watching for this specific shape — not merely "polling is what we already
do," but "the event-driven alternative is documented as unreliable on the
kind of storage a PVC often is."

**(c) Cloud-provider event notifications (bucket path only).** Confirmed
directly against AWS's own documentation for this pass: S3 Event
Notifications can publish `s3:ObjectCreated` (and other) events to SQS,
SNS, Lambda, or EventBridge, "delivered at least once... typically... in
seconds but can sometimes take a minute or longer."
[AWS: Amazon S3 Event Notifications](https://docs.aws.amazon.com/AmazonS3/latest/userguide/EventNotifications.html)
Azure Event Grid and GCS Pub/Sub offer the equivalent for their respective
providers but were **not independently researched in this pass** — doing
so meaningfully requires first picking a specific provider, which this
document does not do. This is the cleanest "no polling loop" option, but
it adds a **new consumer dependency** (e.g. an `aws-sdk-sqs`-equivalent
client, on top of whatever reads the object itself) rather than removing
complexity, and for a cadence this app's own base spec already describes
as weekly-full-plus-daily-update (not sub-minute), the latency advantage
of event notification over a modest poll interval is unlikely to be worth
that added integration surface. This is a judgment call, not a sourced
fact.

### Recommendation

**Polling fits this app's existing architecture and Rust toolchain best,
and — for the SFTP/PVC path specifically — is also the empirically
better-supported option**, per the `notify` crate's own documented NFS/
inotify caveats above. It reuses the exact `clap` Config + `tokio`
interval-loop shape every `poller-*` crate already uses, so there is no
new pattern for this codebase to learn, only a new instantiation of an
existing one. Cadence: nothing in this research suggests sub-minute
latency matters for a once-daily push — the base spec's own precedent
(DESIGN.md: "Polling is good enough for the time granularity this product
reports at," cited for LDBWS's 30-60s cadence) argues even more strongly
for a modest interval here (e.g. every 15-60 minutes) being more than
sufficient for a feed that updates once a day.

### New crate, or something simpler?

DESIGN.md §12's "one crate per concern... don't merge them" convention,
already cited in the base spec's own architecture-options discussion,
argues for a new crate rather than bolting file-watching onto an existing
one (`trust-consumer` and `aggregator` are both already-loaded processes
with their own correctness requirements — see the base spec's Option A/B/C
discussion for why blast radius matters here). Where this document departs
slightly from a naive "one crate per pipeline stage" reading: watch → parse
→ load is a single linear pipeline with no independent scaling or
failure-isolation need the way, e.g., `trust-consumer`'s Kafka *read* and
`aggregator`'s status *decision* logic genuinely do (per the base spec's
Option C rejection reasoning). Recommend one new crate covering watch +
parse + load together (e.g. `schedule-ingest`), rather than three separate
crates for what is really one pipeline — unless a concrete reason to split
them emerges once real implementation is attempted.

## 4. Security considerations specific to accepting inbound pushes

**This is a categorically different posture from everything this app runs
today.** Every existing feed connection — `poller-*` via `reqwest`,
`trust-consumer` via `rdkafka` — is outbound: this app is always the
calling party, no port is opened on this app's side for any of them, and a
credential compromise (a leaked RDM API key or Kafka SASL password) is bad
but does not, by itself, expose a listening service to the internet.

An SFTP receiver inverts that: this app must run and secure a listening
authentication daemon that accepts unsolicited inbound connections from
the internet (or, at best, from RDM's IP space specifically, if
allowlisting turns out to be possible — unconfirmed, section 1). This
would be this app's **first inbound-facing backend service other than the
deliberately-public frontend/api behind `ingress.yaml`** — and even those
are HTTP behind a well-understood ingress-controller/TLS-termination
model, not a raw SSH daemon this app operates directly. Concretely, this
means:

- **The daemon itself is now an attack surface this app's own code doesn't
  control.** Unlike a `poller-*` crate's dependency bumps (this app's own
  release cycle), an OpenSSH/SFTP CVE in whichever image is chosen is an
  operational emergency the moment it's disclosed, independent of whether
  any of this app's own code changed. `atmoz/sftp`'s ~2-year-stale last
  update (section 1) is a real, cited data point against it specifically
  for this reason; SFTPGo's actively-maintained-and-commercially-backed
  posture is the stronger comparison on this axis.
- **Credential scoping should be as narrow as the software allows.** Both
  candidate images support chrooting a virtual user to exactly one
  directory with no shell access — RDM's account should get nothing more
  than write access to one directory (or, if the loader wants to move
  processed files rather than delete them, write+list, not broader). This
  is a straightforward extension of the least-privilege posture the base
  helm-chart design already applies everywhere else in this chart
  (`runAsNonRoot`, dropped capabilities, `readOnlyRootFilesystem` by
  default).
- **`atmoz/sftp`-family images typically need to start as root** to chroot/
  chown the mounted directory before dropping privileges — the same shape
  of exception the existing chart already documents and accepts for
  `postgres:16` (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s
  PostgreSQL section: "a bare `runAsNonRoot: true` would fail admission
  before the entrypoint ever runs"). This was **not independently verified
  against either candidate image's actual entrypoint behaviour in this
  pass** — flagged as an implementation-time check, not assumed.
- **Source-IP allowlisting is the single most consequential unconfirmed
  fact in this whole document.** If RDM publishes fixed outbound ranges,
  the exposure shrinks from "the whole internet can attempt an SSH
  handshake against this port" to "only RDM's known infrastructure can" —
  a large difference. This pass could not confirm one way or the other
  (section 1). Kubernetes' own `NetworkPolicy` resource — confirmed by
  reading this chart's `networkpolicy.yaml` in full — governs only
  in-cluster traffic and has no concept of external-internet source-IP
  filtering; that control, if used at all, would need to live at the cloud
  load balancer / security-group layer, entirely outside anything this
  chart renders today. This is worth stating as a concrete future gap:
  even if this feature proceeds, `charts/distant-signal` would not, by
  itself, be able to express "only allow RDM's IPs" — that would be a
  manual, cloud-specific operator step, documented but not automated by
  the chart, unless a future change teaches the chart to render
  cloud-specific firewall/security-group resources (a bigger scope change
  than anything else in this document).
- **The cloud-bucket alternative narrows this specific risk class but adds
  a different one.** No daemon this app operates is exposed — RDM talks
  directly to the cloud provider's own object-storage API, a surface this
  app's own security posture never has to defend. In its place: cloud IAM
  credentials with access to that bucket become the sensitive material,
  and (unconfirmed in this pass, since it requires RDM account access) how
  RDM authenticates *to* push into a bucket this app owns — a cross-account
  IAM role assumption, a bucket policy naming a specific RDM principal, or
  something else — is itself a place an overly broad grant could quietly
  become a real vulnerability, and this pass has no visibility into RDM's
  actual mechanism for it.

## 5. Cadence and file-handling practicalities

The base spec already confirmed (Open Rail Data Wiki citations): a full
weekly extract (Fridays) plus daily update extracts, both available in CIF
text and JSON (JSON recommended for non-advanced consumers).

**A real, load-bearing file-size data point exists in this repo's own
prior research**, not invented for this pass:
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-
timetable-verification.md` documents a genuine sample full-timetable CIF
extract obtained during that verification pass — `timetable_full.zip`,
**76MB compressed, ~711MB uncompressed** (the main schedule file,
`RJTTF942MCA.txt`, alone measured 707,743,886 bytes, 8,631,021 lines, per a
full pass over the real file quoted in that document). That document is
explicit that this single sample cannot establish periodicity or confirm
this size recurs on every "full" pull, and its own provenance (whether it
actually arrived via RDM's SFTP/bucket push specifically, versus some
other channel) is unconfirmed — but it is nonetheless the best real-world
size anchor available anywhere in this app's research, and it directly
sizes both the PVC (SFTP path) and the expected object size (bucket path).
No equivalent real sample of a **daily UPDATE-only** extract exists in this
app's research; a delta extract would be expected to be much smaller than
a full extract, but that is an inference from the format's own design
(update files carry only changed schedules), not a measured number — flag
as unconfirmed.

**Same-day processing guarantees.** No RDM SLA or delivery-window
documentation was accessible in this pass (the relevant wiki page 403'd,
no search budget remained to find an alternative source). Whether "daily"
means "always lands by a specific time" or "sometime during the day, no
guarantee" is unknown. If this proceeds, a practical mitigation that fits
this app's existing patterns: this app already documents a `/public/
freshness` concept (per the base helm-chart design doc's route table) —
extending that existing freshness-signalling idea to also report "when did
the last successful schedule-file ingest happen" would let operators (and
potentially the frontend) surface staleness rather than silently serving
increasingly-outdated schedule data, without inventing a new mechanism.

**Retention/cleanup.** Given the ~700MB size anchor above and a daily push
cadence, unbounded retention on a PVC would grow without bound —
`crates/aggregator`'s own existing `HISTORY_RETENTION_DAYS` (default 7,
per the base helm-chart design doc's aggregator section) is a directly
reusable precedent shape: a small integer "keep N days" env var, pruned by
the ingest process itself. A sane default policy, following that
precedent: delete a received file immediately after it has been
successfully parsed and loaded (the raw file has no lasting value once
ingested, unlike Postgres's own retained history rows), keeping only
whatever the in-flight ingest needs plus perhaps one prior full extract as
a fallback if the most recent one fails to parse. This shrinks the
realistic PVC size requirement from "accumulate every daily push forever"
(which at ~700MB/day would be tens of GB/month) down to "a few times the
size of one full extract," a few GB, comfortably.

## Rough architecture sketch — "if we proceed," not an implementation plan

This section sketches the shape only; it does not write any Helm template,
Dockerfile, or Rust code, per this task's explicit constraint.

**Chart additions (`charts/distant-signal/`), SFTP path:**

- A new values block (e.g. `scheduleFeed.*`), gating the whole subsystem,
  mirroring `devAuthentik.enabled`'s pattern of an optional, wholly
  toggleable component.
- A new Deployment (`replicas: 1` fixed, `strategy: Recreate`, mirroring
  `aggregator-deployment.yaml`'s singleton rationale) running the chosen
  SFTP image (SFTPGo, per section 4's maintenance comparison, unless a
  concrete reason favours `atmoz/sftp`), with a plain `PersistentVolumeClaim`
  (not a `volumeClaimTemplate`) mounted for received files, sized per the
  retention policy in section 5.
- New Secret keys (extending `secret.yaml`'s existing existingSecret-
  override pattern): SFTP account credentials, plus SSH host keys —
  the latter genuinely new secret *material*, not just a new key name; see
  section 1's caveat about verifying Helm's key-generation mechanism
  against this chart's Helm-4-specific lookup-preserve quirks before
  relying on it.
- A new Service, `type: LoadBalancer` (or `NodePort` + an operator-managed
  external LB/DNS, mirroring `devauthentik-service.yaml`'s NodePort
  pattern but intended for real external use, not just dev reachability) —
  a sibling to `ingress.yaml`, not an extension of it, since `Ingress`
  cannot carry raw TCP/SFTP traffic.
- A new watch+parse+load crate (e.g. `schedule-ingest`, per section 3),
  deployed as its own `Deployment` (`replicas: 1` fixed, same singleton
  rationale), polling the mounted PVC directory on an interval, feeding
  whatever downstream store the base spec's Option B recommendation
  ultimately lands on (this document does not re-derive that).
- `networkpolicy.yaml` would need a new in-cluster allow rule if the
  watcher crate is a separate pod from the SFTP daemon (watcher ← SFTP
  daemon's PVC is a shared-volume relationship, not network traffic, so no
  new rule is needed for that specific link; a rule would be needed only if
  the watcher also talks to the SFTP daemon's container directly, which
  this sketch does not require). Critically, as section 4 states, nothing
  in this chart's `NetworkPolicy` model can restrict the *external* traffic
  hitting the new LoadBalancer Service — that is a cloud-specific,
  operator-configured control this chart does not currently express.

**Chart additions, cloud-bucket path (as an alternative, not additive to
the above):** no in-cluster receiving component at all — RDM pushes
directly to the cloud provider's bucket API, bypassing the cluster
entirely, which is itself a meaningful architectural simplification worth
naming plainly. The only new chart surface is the `schedule-ingest` crate's
Deployment (same singleton shape as above) plus a new Secret entry for
cloud SDK credentials (scoped read-only, ideally, to the one bucket/
prefix) — no new PVC, no new Service, no new SSH host-key material, no new
LoadBalancer.

**`docker-compose.yml` additions (SFTP path):** a new service block
following the shape of the existing `trust-consumer:`/`devauthentik-
server:` blocks — the SFTP daemon container on a mapped host port, a named
volume for received files (mirroring the existing `postgres`/`devauthentik-
postgres` named-volume pattern), and the new `schedule-ingest` crate's
binary as another compose service pointed at that same volume. Matching
this repo's existing `.env.example` convention of deliberately non-
functional `*.example.invalid` placeholders for feeds with no confirmed
endpoint: local dev has no real RDM to push into the compose SFTP
container, so exercising the pipeline locally would need either a manual
SFTP/SCP of a sample file (e.g. the kind of sample described in section 5)
or a small seed script — neither designed by this document. For the
bucket path, local dev would instead need either a real (test/sandbox)
cloud bucket, or a local S3-compatible emulator (e.g. MinIO) as a compose
service — genuinely new territory for this repo's compose file either way,
not evaluated further here.

## Open questions and risks — honest, not resolved here

**Read the "Addendum (2026-08-30)" section near the top of this document
first** — it resolves or substantially narrows several of the items below
(the push-only premise behind item 1, the licence/cost-tier question, the
single-file assumption behind item 6) using three new primary sources
(a real signed licence PDF, a real sample timetable extract, and RDG's own
public RSPS5046 interface spec). Items below are left as originally
written, with inline pointers added where the addendum changes the
picture, rather than silently rewritten — see the addendum for the current
state of each.

1. **RDM's exact push-destination configuration mechanics are entirely
   unconfirmed**: whether it wants a static IP vs. a stable hostname, how
   (or whether) a destination is pre-registered/validated before RDM will
   push, whether it supports SFTP public-key auth on the receiving
   account, and — most consequentially — **whether RDM publishes fixed
   outbound source-IP ranges** that could be allowlisted. All of these live
   inside RDM's own portal/support channel, which this research pass has
   no access to (same limitation the base spec hit for TRUST's Kafka
   product details). Confirm directly with RDM before any implementation.
   **Addendum update**: this item assumed push was the only path. RDG's
   own RSPS5046 spec confirms SFTP *pull* from a named, stable hostname
   (`dtd.atocrsp.org`) is also a first-class option, which sidesteps most
   of this question for the pull variant specifically (no destination to
   register on RDM's end at all) — see Addendum §1/§5. The push-specific
   sub-questions here (IP ranges, pre-registration) are narrowed, not
   closed: a mechanism to obtain IP addresses exists (the DTD Web Portal,
   per RSPS5046 §7.5.4), but this pass could not access it directly.
2. **How RDM authenticates to push into a customer-owned cloud bucket is
   unconfirmed** — cross-account IAM role assumption, a bucket policy
   naming a specific RDM principal, or another mechanism entirely. This
   has direct security implications (an overly broad grant is an easy
   misconfiguration) and could not be researched without RDM account
   access.
3. **This session's web-search budget was exhausted before this research
   began**, so this pass relied on direct `WebFetch` of specific URLs
   (some of which also failed — the Open Rail Data Wiki's RDM page 403'd
   again, matching the base spec's own experience) plus this repo's own
   prior research documents. A follow-up pass with search available could
   plausibly surface RDM's IP-range publication (if any exists), current
   SFTPGo/atmoz Kubernetes deployment guidance, and current cloud storage
   pricing — none of which this pass could independently confirm or deny.
4. **Neither candidate SFTP image's exact container entrypoint/security-
   context requirements (root-start-then-drop-privileges behaviour,
   specifically) were tested against a real cluster in this pass** — the
   `postgres:16`-style `securityContext` carve-out this document assumes
   is analogical, not verified.
5. **Whether an SSH-host-key-generation mechanism (e.g. Sprig's
   `genPrivateKey`) actually works cleanly under this chart's existing
   Helm-4 lookup-preserve pattern was not tested** — the base helm-chart
   design doc already found one non-obvious Helm-4 gotcha in the simpler
   flat-string case; a keypair-shaped secret has not been exercised at all.
6. **The ~711MB full-extract size anchor (section 5) comes from one
   sample, of unconfirmed provenance, in this app's own prior research** —
   it is the best available real number, not a guaranteed constant across
   every future pull, and no real daily-UPDATE-extract sample exists
   anywhere in this app's research to size that smaller case.
   **Addendum update**: provenance is no longer unconfirmed — the sample's
   9-file structure, filenames, and per-file contents match RDG's own
   published RSPS5046 spec exactly, confirming this is a genuine DTD Full
   Refresh delivery, not a differently-sourced stand-in. The 711MB figure
   itself is unchanged (still one day's sample, still no daily-UPDATE-only
   sample exists) — see Addendum §3.
7. **No cloud provider, region, or account has been chosen for this app**,
   so the cross-region egress cost question (section 2) and the
   Azure-Event-Grid/GCS-Pub-Sub research gap (section 3) cannot be closed
   without that decision being made first, which is out of scope for this
   document.

## Summary (for the person who asked)

**Note (2026-08-30 addendum)**: the summary below predates three new
primary sources (a real licence PDF, a real sample timetable extract, and
RDG's own RSPS5046 interface spec) that add a third option this summary
doesn't mention: **SFTP pull** from a documented, stable RDG hostname
(`dtd.atocrsp.org`), which needs neither a LoadBalancer/NodePort, SSH
host-key material, nor any inbound-facing daemon at all — see the
"Addendum (2026-08-30)" section near the top of this document,
particularly §1 and §5, before treating the SFTP-push-vs-cloud-bucket
framing below as the complete menu of options. The "proceed with caveats,
not yet" posture inherited from the base spec is unchanged; the licensing
question this summary treats as unresolved is now resolved (free, per a
real signed PDF), with one new caveat the addendum surfaces (the
licensed product's "research & analysis purposes only" wording).

Both paths are real, buildable options, and neither is a small addition —
this is genuinely new infrastructure for this app either way, not a
variant of anything it runs today. **Self-run SFTP** (SFTPGo is the
better-maintained of the two candidate images checked) keeps this app's
existing self-hostable, no-cloud-lock-in posture intact, but means owning
an internet-facing SSH daemon indefinitely: its own CVE exposure, SSH
host-key provisioning (a new class of secret material this chart's Helm-4
lookup-preserve pattern has never handled and would need verifying), and a
brand-new Kubernetes `Service` type (`LoadBalancer`/raw TCP) this chart has
never rendered for real external use — `ingress.yaml`'s existing HTTP-only
`Ingress` genuinely cannot carry this traffic. **A cloud bucket** (AWS/
Azure/GCS) removes the "we operate a daemon on the internet" risk entirely
— RDM talks to the cloud provider's own API instead — and turns the
watcher into something structurally close to this app's existing
`poller-*` pattern, but it is a first-ever hard dependency on a specific
cloud provider for a codebase whose whole design (confirmed by grep: zero
existing cloud SDK usage) has been provider-agnostic and self-hostable
until now, and its cost/region implications can't be sized without knowing
where this app would actually run.

Whichever receiving mechanism is chosen, **polling fits this app's
architecture best for the watcher** — it reuses the exact `poller-*` shape
already proven out, and for the SFTP/PVC option specifically, Rust's own
`notify` crate documents that event-driven filesystem watching is
unreliable on network-backed volumes (a real risk for Kubernetes PVCs)
and recommends falling back to polling for exactly this case. One new
crate covering watch+parse+load together is the right scope, not three.

The most consequential open question by far is **whether RDM publishes a
fixed source-IP range for its push infrastructure**. If it does, the SFTP
option's internet exposure shrinks to "known RDM infrastructure only," a
much safer posture; if it doesn't, an SFTP daemon on the open internet
relying on SSH auth alone is a meaningfully bigger risk than anything this
app currently runs, and that alone might tip the recommendation toward the
cloud-bucket path even for a team that would otherwise prefer to avoid a
cloud-provider dependency. This pass could not resolve that question —
confirming it directly with RDM (which requires the account access this
research pass doesn't have) should be the first concrete step before
committing to either path.
