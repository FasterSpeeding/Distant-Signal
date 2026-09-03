# Schedule-Feed Cadence Research: Which DTD Product(s) Should This App Subscribe To?

**Status: research only, not a design and not implementation-track work.**
Written to the same citation discipline as
`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md` ("the
ingress design doc" throughout) — every external claim attributed to a
source (RSPS5046 by section number, a licence PDF by clause, this repo's own
code by `file:line`), every unconfirmed inference flagged as such rather than
asserted. This document does not re-derive RSPS5046's contents or the
ingress design doc's SFTP-mechanism findings — it assumes them as settled
background (see `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md`
for the currently-live push mechanism this app actually runs) and asks one
narrower question: **given push-over-SFTP is settled, which combination of
cadences/products — full-daily (the status quo, "Timetable - Full Refresh -
Daily," RDM product `P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`), daily
update-only, full-weekly, and/or full-monthly — should this app actually
subscribe to, including holding more than one simultaneously?** This is
read-only research; no file in `crates/schedule-ingest`,
`charts/distant-signal/values.yaml`, or
`charts/distant-signal/templates/schedulefeed-deployment.yaml` was touched
while writing it (a separate concurrent fix was in flight against those
files).

**A framing correction, attributed to its source rather than independently
verified, per this app's research documents' established convention for
owner-supplied facts** (mirroring how the push design doc attributes the
pull-staff-only finding to "the repo owner, 2026-09-01" rather than
presenting it as something `WebFetch`/`WebSearch` independently confirmed):
**the repo owner states, 2026-09-03, that DTD/RDG does not require picking
exactly one of daily/weekly/monthly, or full vs. update-only — these are
independently subscribable, and a recipient can hold more than one
simultaneously** (e.g. a full-weekly baseline plus daily-update deltas, or
full-daily plus a monthly archival copy). **No primary source already cited
by this app's research (RSPS5046, the licence PDFs) explicitly confirms or
rules out combinability one way or the other** — every RSPS5046 passage
already quoted in the ingress design doc describes each cadence from a
single subscriber's-eye view ("Data Recipients that choose to receive
weekly timetable feeds...," §7.7.1) without stating whether that choice is
exclusive of also holding a daily subscription. This document treats the
repo owner's statement as given, per this repo's convention for such facts,
and evaluates the combination space accordingly throughout — not "pick
one."

## Summary of the status quo, confirmed against real code and real data

- `crates/schedule-ingest/src/main.rs:453-468`'s `is_manifest_filename`
  recognizes exactly one manifest-filename shape:
  `RJTTF<digits>DAT.txt` (prefix `RJTTF`, suffix `DAT.txt`, digits
  in between) — the Full Refresh product's manifest naming, confirmed
  against RSPS5046 §5.2.2's own worked example and the real
  `RJTTF942DAT.txt` sample (ingress design doc:107-124). There is no
  second recognized manifest shape anywhere in this crate.
- `crates/schedule-ingest/src/config.rs:35-37`: `retention_keep_sequences`
  defaults to `2` — "current + fallback."
  `crates/schedule-ingest/src/main.rs:507` (`prune_old_sequences`) enforces
  this by deleting all but the `keep` highest numeric sequence
  subdirectories of `storage_dir`.
- `charts/distant-signal/values.yaml:973`: `scheduleFeed.sftp.persistence.size`
  defaults to `5Gi`.
- The untracked reference file `timetable_full.zip` is **still present at
  the repo root**, confirmed this session: `76M`, dated 2 Sep — matching
  the ingress design doc's own byte-exact measurement, **76,446,640 bytes
  compressed / 711,352,325 bytes uncompressed across all 9 files**
  (ingress design doc:315-330), of which `RJTTF942MCA.txt` alone is
  707,743,886 bytes / 8,631,021 lines. This is a real, confirmed-genuine
  DTD Full Refresh delivery instance, not a synthetic stand-in (ingress
  design doc:322-330).
- `crates/schedule-reference/src/main.rs:65,94,106` and
  `crates/schedule-reference/src/sequence.rs:4,34`: the downstream
  STANOX/CRS-table builder treats every `storage_dir/<n>/` sequence
  directory as a **self-contained** unit — it looks for `RJTTF<n>MCA.txt`
  and `RJTTF<n>MSN.txt` together, streams `MCA` line-by-line without ever
  holding the 707MB file in memory whole (comment at `main.rs:65`), and
  extracts `TI`/`A` records fresh from *that one sequence* every time. It
  never reads or diffs against any prior sequence's output. This is the
  concrete downstream consumer referenced by
  `docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md`
  (that document's own working title before the crate was named).

## 1. Storage/bandwidth cost of the status quo

**Confirmed real numbers, not estimates.** At `retention_keep_sequences=2`
and a ~711MB-uncompressed-per-delivery anchor (ingress design doc:315-330,
re-confirmed above against the still-present `timetable_full.zip`), steady-
state disk usage is roughly 2 × 711MB ≈ **1.4GB**, comfortably inside the
chart's 5Gi PVC default (`values.yaml:973`) with headroom for a third,
in-flight delivery mid-download before the older one is pruned. This matches
the ingress design doc's own sizing reasoning exactly: "a few times the size
of one full extract, a few GB, comfortably" (ingress design doc:1193-1197).
**Nothing about this figure is hypothetical or extrapolated from a single
sample's provenance being uncertain** — the file is still sitting in this
repo's working tree at the size the design doc measured, and the chart's
shipped PVC size was set with that measurement already in hand.

At a genuinely daily cadence (RSPS5046 §7.6/§7.7, confirmed: this product
delivers "a full refresh every day," ingress design doc:270-271), ingest
bandwidth is ~711MB/day = ~5GB/week = ~22GB/month, uncompressed (the SFTP
transfer itself would move the compressed ~76MB/day if DTD compresses
in-flight the same way the sample zip does — **unconfirmed**, since RSPS5046
does not document the wire-transfer compression format for SFTP delivery,
only that the reference *sample* this app holds happens to be a zip; this
gap is not resolved by this pass).

**Would daily update-only deliveries reduce this meaningfully?** In
principle, yes — RSPS5046 §4.2 documents `RJTTCnnn.CFA` as the incremental
counterpart to `MCA`, and every prior document in this app's research chain
that touches the question describes an update file as expected to be "much
smaller than a full extract... an inference from the format's own design
(update files carry only changed schedules), not a measured number" (ingress
design doc:1166-1170, restated verbatim as still-unconfirmed by the push
design doc, which does not revisit it). **No real `CFA` sample has ever
been obtained anywhere in this app's research** — three separate documents
(ingress design doc, pull design doc, push design doc) all flag this as the
same standing gap, never closed. So: the *direction* of the storage/
bandwidth saving is well-supported by the file-format's own design intent,
but this app has zero real measurement of its magnitude. Given the full
extract already comfortably fits a 5Gi PVC with 2 retained sequences, the
practical storage saving from switching is not "otherwise-infeasible becomes
feasible" — it is "a comfortable footprint becomes an even more comfortable
one." At this app's scale (a single-tenant PVC, not a fleet of receivers),
that is a marginal win, not a structural one.

**Would downstream parsing need to change materially to apply an
update-only delta on top of a full baseline?** Yes, and this is the more
important cost, not the storage line. Confirmed by direct inspection of the
two real consumers:

- `crates/schedule-reference` (STANOX/CRS table) is **stateless per
  delivery** today: each new full-refresh sequence is parsed from scratch
  and its output *replaces* the prior table (per
  `2026-09-01-schedule-ingest-stanox-crs-table-design.md:157-169`, "every
  delivery's `TI`/`A` extraction is a complete, standalone, from-scratch
  snapshot — never a merge against a prior day's partial state"). Applying
  a `CFA`-style delta would require this crate (or a new one) to instead
  **maintain a durable baseline** (the last full refresh's parsed state) and
  apply an ordered sequence of incremental patches on top of it, with all
  the correctness hazards that implies: what happens if a delta is missed,
  corrupted, or arrives out of order; how staleness is detected if the
  baseline silently drifts from DTD's actual current state; whether `TI`
  amend/delete records (`TA`/`TD` — confirmed absent from every full-refresh
  sample this app has seen, ingress design doc:296-304, only expected in the
  update file) need new parsing logic this crate has never exercised against
  a real sample.
- The only other real consumer of `MCA` content beyond STANOX/CRS is
  **the not-yet-built full-timetable ingestion** described in
  `2026-09-01-schedule-ingest-stanox-crs-table-design.md:494-556` (Decision
  5): parsing `BS`/`BX`/`LO`/`LI`/`CR`/`LT` (8,610,939 of `MCA`'s 8,631,021
  lines), STP overlay resolution (`C`/`N`/`O`/`P` precedence by calendar
  date — "a genuinely stateful algorithm, not a lookup"), and a schema on
  the order of 400,000+ schedule rows / 6.8M calling-point rows nationwide.
  That document is explicit this is **unbuilt, scoped separately, and sized
  at "roughly a month of work"** by direct comparison to train-mcp's
  equivalent system (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:530-542`,
  citing `2026-09-01-train-mcp-integration-research.md:256-268`). **This
  matters directly for the update-delta question**: this app has, today,
  zero code anywhere that holds a durable, mutable representation of "the
  current timetable" that a delta could even be applied *to*. Every real
  consumer that exists is a stateless, from-scratch reader of one
  self-contained full extract. Adopting update-only deltas now would mean
  building the stateful-baseline machinery *before* there is a real reason
  to (no full-timetable consumer exists yet to benefit from it), on top of
  reference-table logic that was deliberately designed to avoid exactly this
  kind of merge complexity (the STANOX/CRS design doc's own Decision framing,
  cited above, treats "never a merge against a prior day's partial state" as
  a feature, not an oversight).

**Is that complexity worth the storage/bandwidth savings for an app of this
scale?** No, not now, on the evidence available. The status quo already
fits a 5Gi PVC with room to spare (§1 above), the saving from switching is
unmeasured but bounded by "less than 711MB/day of headroom this app doesn't
currently need," and the cost is a new class of stateful-merge bug surface
introduced into the one crate (`schedule-reference`) whose design explicitly
optimized against that exact complexity, for a benefit this app has not
identified a concrete need for.

**Combination-specific storage math** (evaluating combinations, not just a
straight switch, per the repo owner's non-exclusivity correction above): a
**monthly-full-refresh subscription added alongside the existing full-daily
one** costs almost nothing extra to store — one additional ~711MB delivery
roughly 12 times a year (≈8.5GB/year if every monthly delivery were kept
indefinitely; effectively free if only the latest is retained, mirroring
`retention_keep_sequences`'s existing "current + fallback" policy applied
to a second, independent sequence-number space). A **full-weekly-baseline
plus daily-update combination**, evaluated as a *replacement* for the
current full-daily subscription rather than an addition to it, is the one
combination that would meaningfully change the storage/bandwidth profile:
roughly one 711MB transfer/week plus six unmeasured-but-presumed-small delta
transfers, instead of seven 711MB transfers/week under the status quo — but
this is the same complexity trade already analyzed above (a delta model
requires `schedule-reference` to hold state it does not hold today), just
anchored to a weekly rather than daily baseline; switching the baseline
cadence does not remove that cost, only defers it to a slower cycle.

## 2. What each cadence buys, and why full-daily was chosen over delta

**This app never ran a comparative evaluation of full-daily vs.
update-only as a build decision** — this is worth stating plainly rather
than reverse-engineering a rationale that was never actually deliberated.
The chain of documents shows the product was **already the one this app
held a real, signed licence for** before any of the cadence-comparison
research happened:

- The ingress design doc's Addendum (2026-08-30) §2 read a real, signed RDM
  licence PDF and found the licensed product is named, verbatim, "**Timetable
  - Full Refresh - Daily**" (product ID `P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`,
  ingress design doc:175-178) — a pre-existing fact this research
  *discovered*, not a choice this research *made*. Every downstream document
  (the pull design doc, the push design doc, the STANOX/CRS table design)
  treats this as the given, licensed product and designs against it as-is.
- Grepping every prior spec in this repo (`docs/superpowers/specs/*.md`) for
  an explicit "we considered update-only and rejected it because..."
  argument turns up **no such passage anywhere**. The closest any document
  comes is the STANOX/CRS table design's Decision 5 (cited above), which
  argues for *not building full-timetable ingestion at all yet* — a scope
  decision about what to parse, not a cadence decision about what to
  subscribe to.

**What full-daily concretely buys, per RSPS5046 and this app's own
downstream code, that a delta model would not:**

- **Self-contained correctness.** RSPS5046 §7.6.1: "New Daily Recipients
  that begin the service will be provided with a full refresh of timetable
  data" (ingress design doc:616-618) — a full-refresh subscriber can never
  end up in a state where its data depends on successfully applying an
  unbroken chain of prior deltas. §7.4's resumption text (ingress design
  doc:604-614) confirms sequence numbers are not guaranteed contiguous even
  for full-refresh recipients ("the sequence number of this Full Refresh
  will not necessarily be contiguous from the last feed sequence") — for a
  full-refresh subscriber this is a non-issue (each delivery stands alone);
  for a delta subscriber, a gap in the delta chain would be a real data-
  integrity problem requiring detection and a resync-from-full-refresh
  fallback path this app has never designed.
- **Matches this app's actual failure-recovery story.** `schedule-ingest`'s
  own `retention_keep_sequences=2` design keeps a fallback full sequence
  specifically so a bad parse of the newest delivery can fall back to the
  prior one (ingress design doc:1190-1193, "keeping only whatever the
  in-flight ingest needs plus perhaps one prior full extract as a fallback
  if the most recent one fails to parse"). This fallback story is trivial
  under full-daily (the prior sequence is itself a complete, independently
  valid snapshot) and would need real redesign under a delta model (the
  "prior sequence" would only be valid in combination with everything
  already applied on top of a much-older full baseline).
- **RSPS5046 §7.7's weekly/monthly full-refresh cadences buy operational
  cost reduction, not data-quality improvement**, and are explicitly framed
  in the spec as an alternative to *this app's actual product*, not an
  addition to it: "Data Recipients that choose to receive weekly timetable
  feeds will receive a full refresh of timetable data each Wednesday of
  each week" / "...monthly... on the first Wednesday of each period"
  (ingress design doc:622-627, quoting §7.7.1/§7.7.2). A UK national
  timetable genuinely changes within a week (engineering-work overlays,
  short-notice STP amendments) — nothing in this app's research quantifies
  how stale a weekly or monthly snapshot would leave the STANOX/CRS table
  or a future full-timetable consumer, but the base spec's whole premise
  (TRUST-vs-schedule delay inference needing an accurate *current* planned
  timetable, `2026-08-29-trust-schedule-delay-inference-design.md`, cited
  throughout the design chain) argues for currency over the marginal
  bandwidth/storage saving weekly or monthly would offer over daily. No
  document in this app's research trail argues *for* weekly or monthly on
  the merits — they were noted as cited facts (RSPS5046 documents them),
  never proposed as a substitute.

**Does the original reasoning still hold up, or has anything changed now
that `schedule-ingest` is live and shipped rather than speculative?**
It holds up, and the fact that the app has since shipped strengthens rather
than weakens it: `crates/schedule-reference` was built, after
`schedule-ingest` went live, specifically around the "every sequence is a
complete, independent, from-scratch snapshot" property
(`2026-09-01-schedule-ingest-stanox-crs-table-design.md:157-169`). That is a
real, shipped architectural commitment to full-refresh semantics, made with
full knowledge that update-only existed as a documented alternative
(the same document cites `CFA` by name in its own text, line 163) and chose
not to build against it. Switching now would mean **retrofitting** a
stateful-merge model onto a crate deliberately built to avoid one, not
merely picking a different starting cadence for a system that hasn't
committed to anything yet.

### What subscribing to more than one cadence would actually buy, evaluated combination by combination

Since daily/weekly/monthly and full/update-only are independently
subscribable (repo owner, 2026-09-03, cited above), the real design space
is larger than "pick one." Working through each combination this app could
plausibly hold, given what RSPS5046 and this app's own code already
establish:

- **Full-daily + full-weekly.** No content value: RSPS5046 §7.7.1's weekly
  full refresh (every Wednesday) delivers a strict subset of what the
  existing daily subscription already delivers every day, Wednesdays
  included. Every Wednesday's weekly-full delivery would be redundant with
  that same day's daily-full delivery, arriving from the same underlying
  DTD service. Not recommended under any combination this pass can
  construct a rationale for.
- **Full-daily + daily-update.** Also redundant on content grounds: the
  update-only file (`CFA`) is, per RSPS5046 §4.2, "the CIF update file to
  be applied to Full Basic Timetable Detail" — its entire purpose is
  letting a recipient avoid re-downloading the full file. A recipient who
  already receives the full file daily gains nothing from also receiving
  the delta against it. Not recommended.
- **Full-weekly + daily-update, as a replacement for full-daily.** The one
  combination that changes this app's actual bandwidth/storage profile
  (§1's combo math above) — a smaller weekly-anchored baseline plus daily
  deltas instead of a daily full re-transfer. This is the option worth
  taking seriously, and it inherits every complexity cost §1 already
  identified: `crates/schedule-reference` would need a durable baseline and
  delta-application logic it has never had, for a storage/bandwidth saving
  this app does not currently need (§1). It does have one real point in its
  favor that a bare "switch straight to daily-update" would not: the
  *content* delivered over any given week is the same either way (a
  week-old full baseline plus that week's deltas reconstructs the same
  state daily-full snapshots would show), so this combination doesn't cost
  currency the way weekly-full-alone would. Still not recommended given no
  identified current need for the storage/bandwidth saving, but flagged as
  the most defensible alternative if that calculus ever changes.
- **Full-daily + full-monthly, as an addition (not a replacement).** A
  different kind of combination from the other three — its value isn't
  "different content" (a monthly full refresh's content is, again, a
  strict subset of what daily-full already provides), it's **an
  independent, DTD-provided long-horizon archival copy that does not
  depend on this app's own PVC/retention correctness**. If
  `schedule-ingest`'s retention pruning (`prune_old_sequences`,
  `crates/schedule-ingest/src/main.rs:507`) ever had a bug, or the PVC were
  lost, a separately-subscribed monthly delivery arriving on its own
  cadence would be a real, externally-sourced fallback snapshot,
  independent of anything this app already retained locally. This is a
  disaster-recovery/audit-trail argument, not a freshness or bandwidth
  argument, and it costs very little (§1's combo math: ~711MB roughly 12
  times a year). **No document in this app's research, including this one,
  identifies a concrete current requirement for this** — this app retains
  no schedule-feed history today beyond `retention_keep_sequences`'s
  "current + fallback" (`crates/schedule-ingest/src/config.rs:35-37`), and
  no consumer reads or would benefit from a months-old snapshot today. It
  is named here as the one combination with a real, if speculative,
  rationale and a low cost — worth revisiting if/when the full-timetable
  ingestion project (`2026-09-01-schedule-ingest-stanox-crs-table-
  design.md`'s Decision 5) is undertaken and historical-snapshot value
  becomes concrete, not something to add speculatively today.

## 3. Licensing/access cost — precisely what Task 1's "favorable" finding covers

`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
Task 1 (lines 56-102) is the document named in the task brief. Read in full
for this pass; its "favorable, not a blocker" verdict is **real but scoped
narrower than "the whole full-daily/full-weekly/full-monthly/daily-update
product family is one already-licensed thing."** Specifically:

- Task 1 confirms exactly **two** real, signed licences: **"Darwin Timetable
  Files"** (RDM product `P-9ca6bc7e-62e1-44d6-b93a-1616f7d2caf8`, OGL v3.0,
  free, "**Daily** update frequency," validation-findings:65-70) and
  **"NWR CORPUS"** (RDM product `P-9d26e657-26be-496b-b669-93b217d45859`,
  OGL v3.0, free, "**Monthly** update frequency," validation-
  findings:72-77). CORPUS is a location/reference-master-data product
  (STANOX/TIPLOC/CRS master lists), not a schedule-cadence variant of the
  Timetable product — its monthly cadence is not evidence about whether a
  monthly *Timetable* full-refresh is licensed.
- **Neither of these two product IDs is the product this app actually
  ingests today.** The ingress design doc's Addendum §2 (lines 200-236,
  cited in this document's §2 above) already flags this precisely: the real
  licence PDF it read in full names a **third, different** product ID
  (`P-1caaf2e8-3d5e-466d-8bf8-06d97e675bef`, "Timetable - Full Refresh -
  Daily") with **different permitted-purpose wording** ("research & analysis
  purposes only," narrower than Darwin's "internal business purposes only")
  and a **different territory** ("UK and Europe" vs. Darwin's "Global minus
  sanctioned countries"). That addendum states outright: "this research pass
  cannot reconcile which one (if either, or both) is the one actually
  governing production use of the real `timetable_full.zip` sample, since
  RDM product IDs, not display names, are the authoritative identifier and
  the two documents show two different IDs" (ingress design
  doc:220-226). **This document does not resolve that reconciliation
  either** — it remains a real, open, previously-flagged gap, not something
  this pass can close from documents alone.
- **Task 1's "Daily" cadence note for the Darwin licence does not itself
  distinguish full-refresh-daily from update-only-daily.** The validation-
  findings document records only the word "Daily" (validation-
  findings:69) — it was written before the ingress design doc's later
  RSPS5046 research established that "daily" is not one product but at
  least two distinct ones (full-refresh-daily vs. update-only-daily,
  RSPS5046 §4.2/§7.6, ingress design doc:268-275). Task 1 cannot be read,
  in hindsight, as having specifically confirmed licensing for an
  update-only daily product — it predates the finding that such a product
  even exists as a documented distinct SKU.
- **No document anywhere in this app's research trail has read a real
  licence PDF (or any other primary source) for a weekly or monthly
  Timetable full-refresh product, or for an update-only daily product.**
  The only primary-source-confirmed, real, signed licence for *any*
  Timetable-family product with full text read in this pass's research
  chain is the one PDF the ingress design doc's addendum read
  (`P-1caaf2e8-...`, "Timetable - Full Refresh - Daily" specifically) — and
  that is the exact product `schedule-ingest` already receives.

**Conclusion for this section**: Task 1's "favorable, not a blocker"
verdict is real evidence that *this general category of data* (RDG/RSP
Timetable and CORPUS products) tends to be free, OGL-adjacent or
OGL-equivalent-in-effect, and low-friction to license for this recipient —
a reasonable basis for optimism that a cadence change would *also* clear
licensing. **It is not itself proof that a weekly, monthly, or update-only
product is already licensed and available to switch to without any new
step.** Switching cadence/product would need its own RDM catalogue check
and (most likely) a new Data Sharing Agreement for a new, distinct product
ID — the same kind of step that produced the currently-held
`P-1caaf2e8-...` agreement in the first place, not a toggle on an
already-covers-everything subscription.

## 4. Recommendation

**Keep full-daily as the sole subscription today. Do not add daily
update-only, full-weekly, or full-monthly right now.** But — reflecting the
repo owner's non-exclusivity correction (this document's opening section
above) — this is not a "pick exactly one, forever" verdict: one specific
addition, **full-daily plus a cheap monthly archival copy**, is the one
combination worth revisiting later, not something this pass recommends
acting on today.

Working through the combination space evaluated in §2 above:

- **Full-daily + full-weekly**: not recommended — redundant, no content
  value (§2, weekly's content is a strict subset of daily's).
- **Full-daily + daily-update**: not recommended — redundant, no content
  value (§2, the update file's whole purpose is avoiding a full
  re-download this app already gets).
- **Full-weekly + daily-update, as a replacement for full-daily**: not
  recommended now — the one combination that would genuinely change this
  app's bandwidth/storage profile (§1's combo math), and unlike a plain
  switch to daily-update it wouldn't cost currency either (§2). But it
  still requires building stateful-baseline-plus-delta logic into
  `crates/schedule-reference` that does not exist today, for a saving this
  app does not currently need (§1). Worth reconsidering only if
  bandwidth/storage becomes a real constraint — e.g. once full-timetable
  ingestion, not just the narrow STANOX/CRS table, is built and its much
  larger content is actually retained rather than streamed-and-discarded.
- **Full-daily + full-monthly, as an addition**: not recommended
  *immediately*, but the most defensible thing to add later — cheap
  (~711MB roughly 12 times a year, §1), no new delta-merge complexity (it
  is still a self-contained full refresh, not a delta), and buys a real
  disaster-recovery/audit-trail property (an independent, externally-held
  long-horizon snapshot) this app's own retention policy does not provide
  today. No document, including this one, finds a *current* concrete need
  for it — flag as a candidate to revisit alongside the full-timetable
  ingestion project named in `2026-09-01-schedule-ingest-stanox-crs-
  table-design.md`'s Decision 5, not as an action item now.

Basis for keeping full-daily as the (sole, for now) subscription, restated
from §§1-3:

- **Storage/bandwidth is not a real constraint today.** Confirmed real
  numbers: ~711MB/delivery, 2 retained sequences ≈ 1.4GB steady state,
  inside a 5Gi PVC with room to spare (§1). There is no operational pain
  this app is currently experiencing that any addition or combination
  would relieve.
- **The complexity any delta-bearing combination would add lands
  specifically on `crates/schedule-reference`, a crate deliberately built
  around "every sequence is a complete, independent, from-scratch
  snapshot"** (`2026-09-01-schedule-ingest-stanox-crs-table-
  design.md:157-169`), and there is no consumer today (full MCA
  schedule-body ingestion is explicitly unbuilt and separately scoped, §1)
  that would benefit enough to justify that retrofit yet. The
  non-delta-bearing addition (full-monthly) avoids this cost entirely,
  which is exactly why it's treated differently above.
- **Licensing is confirmed only for the one product this app already
  holds** (§3) — none of weekly, monthly, or update-only has a real,
  primary-source-confirmed licence anywhere in this app's research trail,
  so any addition needs its own RDM/DTD confirmation step regardless of
  which combination is eventually chosen.

**If a change is ever made** (either the weekly+daily-update replacement or
the monthly archival addition), at a high level, not as a design:

- `schedule-ingest` would need to recognize a **second, genuinely distinct
  manifest-filename family** for a daily-**update-only** product
  specifically — RSPS5046's own naming convention implies an update-only
  product's manifest would carry a different prefix (the `CFA` file itself
  is documented as `RJTTCnnn.CFA`, i.e. `RJTTC`, not `RJTTF` — ingress
  design doc:261; this app's research only cites the `.CFA` data-file
  name, not a `.DAT`-manifest-shape sibling, so the exact manifest filename
  for an update delivery is **inferred by analogy to the full-refresh
  manifest's own naming pattern, not confirmed against any real sample** —
  a genuine open question, not asserted as fact here) alongside the
  existing `RJTTF<digits>DAT.txt` pattern `is_manifest_filename` checks
  today (`crates/schedule-ingest/src/main.rs:453-468`). This would be
  additive, not a replacement — RSPS5046 §7.6.1 still applies (new
  recipients get a full refresh first), so a weekly-baseline-plus-daily-
  update arrangement would still need to receive and recognize full-refresh
  manifests at least once a week.
- **A monthly (or weekly) full-refresh addition is a different, simpler
  case**: since it is still a *full refresh*, its manifest almost certainly
  follows the same `RJTTF<digits>DAT.txt` shape `is_manifest_filename`
  already recognizes — no new filename-shape recognition needed. The open
  question there is instead **sequence-number and directory collision**
  with the existing daily feed: whether DTD assigns monthly/weekly full
  refreshes sequence numbers from the same numbering space as daily ones
  (risking a collision if two different-cadence deliveries land in the
  same watch directory around the same sequence number) or a separate
  space, and whether a second subscription would need its own SFTP
  account/path (a second `scheduleFeed.*`-shaped subsystem, or a second
  subdirectory under the existing one) to keep the two cadences' files from
  overwriting each other — **unconfirmed, not addressed by any document in
  this app's research to date, including this one.**
- `charts/distant-signal`'s `scheduleFeed.*` values would need either a
  second retention knob (distinguishing "how many full baselines to keep"
  from "how many deltas since the last baseline to keep" for the
  weekly+update combination, since the two have different pruning-safety
  rules — a delta can only be safely deleted once every consumer has
  applied it, unlike a full sequence which is independently disposable) or,
  for the monthly-archival addition, a separate, much longer retention
  count/period for that one product's sequences than
  `retention_keep_sequences` currently applies to the daily feed.
- `crates/schedule-reference` (and any future full-timetable consumer)
  would need genuinely new, stateful logic only for the delta-bearing
  combination: a durable baseline store, ordered-delta application, and a
  resync-from-full-refresh recovery path distinct from today's stateless,
  from-scratch-per-sequence design. The monthly-archival addition needs no
  parsing changes at all — it would sit untouched on disk as a backup, not
  be fed to `schedule-reference` on any code path different from the daily
  feed's (or not consumed by it at all, if its sole purpose is disaster
  recovery).

This is a "not now" conclusion tied to this app's current scale and its
currently-unbuilt full-timetable consumer, not a "never" — the STANOX/CRS
table design's own Decision 5 (`2026-09-01-schedule-ingest-stanox-crs-
table-design.md:494-556`) already names full-timetable ingestion as real,
separately-scoped, future work; if and when that work is undertaken, both
the storage/bandwidth math in §1 (a "few GB, comfortably" PVC becoming
potentially much larger once 400,000+ schedule rows / 6.8M calling-point
rows nationwide are actually being retained per delivery, not merely
streamed-and-discarded as `schedule-reference` does today) and the
monthly-archival addition are worth revisiting alongside that project, not
before it.

## Open questions this pass could not resolve

- **Which of the two differently-numbered "Timetable" RDM licences
  (`P-9ca6bc7e-...` "Darwin Timetable Files" vs. `P-1caaf2e8-...` "Timetable
  - Full Refresh - Daily") actually governs production use of the data
  `schedule-ingest` receives today** — flagged first by the ingress design
  doc's Addendum §2, restated here because §3 above shows it also directly
  bears on how confidently this app can claim *any* cadence variant is
  licensed. Needs a direct question to RDG/RDM, not resolvable from
  documents already in hand.
- **Real magnitude of the storage/bandwidth saving an update-only delta
  would provide.** No real `CFA` sample has ever been obtained by this
  app's research (three prior documents all flag this same gap,
  unchanged by this pass). The direction (smaller) is well-supported by
  the format's documented design intent; the size is not measured.
- **The exact manifest-filename shape an update-only delivery would use**
  (relevant to the sketch in §4) — inferred by analogy to
  `RJTTF<digits>DAT.txt`, not confirmed against a real sample or against
  RSPS5046's own worked example (which, per the ingress design doc, only
  documents the full-refresh manifest's worked example, not an update
  delivery's).
- **Whether DTD compresses full-refresh deliveries in transit the way the
  reference `timetable_full.zip` sample is compressed** (76MB vs. 711MB) —
  this bears on real wire-bandwidth cost, not just steady-state disk
  usage, and is not documented in RSPS5046 per this app's existing
  research.
- **Whether a monthly or weekly full-refresh subscription, held alongside
  the existing daily one, would share the same sequence-number space and
  the same DTD-side delivery account/path as the daily feed, or a
  genuinely separate one** — directly affects whether a second
  `scheduleFeed.*`-shaped subsystem (its own SFTP path/PVC) would be
  needed, or whether the existing one could simply grow a second watch
  directory. Not addressed in RSPS5046 or any document in this app's
  research to date, including this one (§4 above).
- **Whether the repo owner's 2026-09-03 non-exclusivity statement (this
  document's opening section) means DTD literally permits combinations
  under this app's *existing* signed licence(s) without a new agreement,
  or only that DTD's product catalogue offers each cadence as an
  independent, separately-licensable SKU** — the distinction matters for
  §3's licensing conclusion (a new Data Sharing Agreement per additional
  product is still assumed necessary here) and was not resolved by
  anything available to this pass.
