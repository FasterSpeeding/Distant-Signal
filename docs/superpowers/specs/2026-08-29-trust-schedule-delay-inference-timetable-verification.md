# TRUST-Schedule Delay Inference: Real-Timetable-Data Verification Pass

**Status: research addendum, not an approved design.** This document
follows up on
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
("the earlier spec"). Do not read this in place of it — it assumes the
earlier spec's problem statement, architecture options, and recommendation
as already-established context and only revisits the specific factual
claims listed below.

## Why this exists

The earlier spec was written entirely from documentation/web research —
its own words: "no invented API details... findings below are
search-engine-summarized citations," and several claims were explicitly
flagged as unverified against real data (open questions 1, 4, 5 in
particular). Its recommendation was **"proceed with caveats, not
yet"** — the segment-precision case for TRUST-vs-schedule correlation was
judged strong, the delay-*accuracy* case weak (Darwin already fuses
TRUST into something richer), and CIF/CORPUS access, cost and cadence were
unconfirmed.

Since that spec was written, a real file — `timetable_full.zip` (76MB
compressed, ~711MB uncompressed, 9 files, untracked local reference data,
not committed to this repo) — became available: a genuine National Rail
full-timetable extract in fixed-width CIF format, generated 28/08/2026.
This document checks the earlier spec's claims against that real sample.
It does **not** re-litigate the architecture options (A/B/C) or the
Darwin-fusion argument — neither depends on schedule-file contents, and
neither is touched by this pass.

All excerpts below are copy-pasted from the real file via
`unzip -p timetable_full.zip <name> | ...` (streaming reads; the archive
was never extracted to disk, per the task constraint). Byte offsets for
fixed-width fields were decoded against the published CIF User Spec field
layout (Open Rail Data Wiki / RSPS5046) and then checked against the
sampled bytes directly — quoted, not paraphrased.

## Claim 1 — CIF SCHEDULE record format (BS/BX/LO/LI/LT/CR/AA, STP overlay)

**Verdict: VERIFIED, with one STILL OPEN nuance on product identity.**

The main file, `RJTTF942MCA.txt` (707,743,886 bytes uncompressed), is a
genuine fixed-width CIF extract. A full pass over every line (`unzip -p
... | awk '{print substr($0,1,2)}' | sort | uniq -c`, 5.8s wall time)
gives the exact record-type population for the whole file:

```
6803900 LI
 488798 BS
 407636 LT
 407636 LO
 407636 BX
  97363 CR
  12085 TI
   5967 AA
      1 ZZ
      1 HD
```
(sums to 8,631,021 lines total.) Every record type the earlier spec named
— `HD`, `TI`, `AA`, `BS`/`BX`/`LO`/`LI`/`CR`/`LT`, terminated by `ZZ` — is
present, in the proportions you'd expect (one `LO`/`LT`/`BX` per non-cancelled
schedule, several `LI` per schedule, one `TI` per named location, `HD`/`ZZ`
exactly once each as file bookends).

**STP overlay behavior is real, not just documented.** Every `BS` line's
final character is the STP indicator; a full tally:

```
  81162 C
 122230 N
 149201 O
 136205 P
```
(sums to 488,798, the total `BS` count — all four indicators the earlier
spec named are populated with real records.) A directly-quoted `C`
(Cancellation) record:

```
BSNG007042605172608300000001            1                                      C
```
— immediately followed in the file by the next `BS` line, with **no**
`BX`/`LO`/`LI`/`LT` body at all. This isn't a one-off: `488,798 (BS) -
407,636 (LO/BX/LT) = 81,162`, which is exactly the `C`-indicator count.
Real data confirms a fact the earlier spec could only assert from the
wiki's prose: a Cancellation-STP schedule carries nothing but the `BS`
header — no calling points, because it doesn't describe a service, only
withdraws one. An `O` (Overlay) record with a full body, for contrast:

```
BSNW684682605172610180000001 POO2E88    113560015 EMU    075D     S            O
BX         SRYSR408800
LOBALLOCH 2308 2308          TB
```

The `HD` header record decodes cleanly against the published field layout:

```
HDTPS.UCFCATE.PD1107191907112108DFTTISG       FA190711300912
```
→ File Mainframe Identity `TPS.UCFCATE.PD110719`, Date of Extract
`190711`, Time `2108`, Update Indicator `F` (Full), User Start/End Date
`190711`/`300912`. **This is the one real surprise this pass turned up
that documentation research could never have surfaced**: the embedded
extract date is `19/07/2011` (or possibly `11/07/19` — either reading is
years stale), while the file's own banner headers (`RJTTF942DAT.txt`,
`RJTTF942MSN.txt`) both say `Generated: 28/08/2026` — matching the current
date. The `HD` record's identity string (`TPS.UCFCATE.PD...`) is
apparently a fixed, long-lived dataset identity carried forward across
regenerations, not a live freshness timestamp — **don't use the `HD`
record's embedded date as a freshness signal if this is ever ingested;
use the plain-text `DAT`/banner "Generated:" line instead.**

**Product-identity nuance, left STILL OPEN**: nothing in this file
confirms it is literally RDM's "SCHEDULE" product listing rather than the
ATOC/RSP "Full Timetable" product the earlier spec's own wiki citation
distinguished from RDM SCHEDULE (same underlying CIF format, historically
different distribution channel). The companion-file bundle here — `MCA`
(schedule) + `MSN` (station names) + `ZTR` (a second, separate ~2.9MB
CIF-format file, own `HD`/`BS` records, apparently `Z`-prefixed UIDs e.g.
`BSNZ01401...` — possibly freight/possession/short-notice schedules; not
identified further in this pass) + `ALF`/`FLF` (interchange/walk-link
files) + `TSI` (TOC interchange minimum-connection-time file, e.g. `AFK,SE,
SN,6,(Ashford International)`) — matches the standard ATOC/RSP CIF-extract
bundle the Open Rail Data Wiki describes, which is suggestive but not
conclusive either way. This file's own provenance (how it reached this
sandbox — RDM SFTP/bucket push, a direct ATOC/RSP subscription, or
something else) is unknown to this pass and matters directly for open
question #2 in the earlier spec (the "new file-push ingestion shape"
finding assumed RDM's push-only delivery specifically) — **that open
question is unaffected by this file's existence**, since having a sample
in hand says nothing about how a live feed of it would actually arrive.

## Claim 2 — TIPLOC join from `lines/*.toml` to CIF schedule locations

**Verdict: VERIFIED, with a real-world complication (TIPLOC proliferation)
documentation research couldn't have surfaced.**

Every TIPLOC value already present in this app's `lines/*.toml` that was
checked appears as a real location identifier, in the field position CIF
defines, in the real file:

- `lines/west-coast-main-line.toml`: `crs = "EUS"` / `tiploc = "EUSTON"`,
  `crs = "WFJ"` / `tiploc = "WATFDJ"`, `crs = "CAR"` / `tiploc = "CARLILE"`.
- `lines/swr-alton.toml`: `crs = "WAT"` / `tiploc = "WATRLMN"`, `crs = "AON"`
  / `tiploc = "ALTON"`.

Confirmed against real `TI` (TIPLOC-master) records in `RJTTF942MCA.txt`:

```
TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON
TIWATFDJ 16140200UWATFORD JUNCTION          710402991WFJWATFORD JUNCTION
TICARLILE00211800PCARLISLE                  091612827CARCARLISLE
TIALTON  00554400HALTON                     87021   0AONALTON
```

And, more importantly for a real join, as the actual location field in
schedule-body `LO`/`LI`/`LT` lines (the records TRUST-vs-schedule
correlation would actually diff against), e.g.:

```
LOEUSTON  0822 08227  C      TB
LTEUSTON  0804 08079     TF
LICARLILE 1202 1213      120212131        T
LOWATRLMN 0754 075315 MFL    TB
```

The join is real: the app's curated TIPLOC values are exactly what
appears, 7-characters-wide-padded, at the front of every relevant CIF
schedule line. One small, concrete implementation detail this confirms
that documentation alone wouldn't: the schedule-body field is a
**fixed 7-character, space-padded** TIPLOC (`"EUSTON "`, `"CARLILE"` — no
padding needed since it's exactly 7 — `"WATFDJ "`), while `lines/*.toml`
stores the bare, unpadded string (`"EUSTON"`, 6 chars) — any real join
needs to pad/trim consistently; a naive substring compare would silently
fail for every TIPLOC shorter than 7 characters (`EUSTON`, `WATFDJ`,
`ALTON`, `WATRLMN`... `CARLILE` happens to be exactly 7 and would mask the
bug in casual testing).

**The complication**: Waterloo is not one TIPLOC, it's (at least) three,
all sharing CRS `WAT`:

```
$ grep -c '^L[OIT]WATRLOO' MCA   → 0
$ grep -c '^L[OIT]WATRLMN' MCA   → 25382
```

`WATRLMN` is the TIPLOC that real train schedules actually use (25,382
occurrences across `LO`/`LI`/`LT` lines); the alternate `WATRLOO` TIPLOC
never appears in a single schedule body line in this whole file, despite
also existing as a named location. This app's `lines/swr-alton.toml`
already has this right (`tiploc = "WATRLMN"`) — a real validation of the
existing curation, not a bug — but it demonstrates concretely that
"TIPLOC per CRS" is not 1:1 at complex stations, and picking the wrong one
of several candidates would silently produce zero matches rather than an
error. Worth a note for whoever eventually builds the ingestion: don't
assume a CRS→TIPLOC lookup is unambiguous; validate against real schedule
body occurrence counts, the way this check just did.

## Claim 3 — CORPUS/STANOX gap

**Verdict: CORRECTED — this is the most consequential finding of this
pass.**

First, the code claim: `crates/trust-consumer/src/process.rs` still
hardcodes `let loc_crs = None; // STANOX->CRS translation: see this
module's docs.` (line 288), and the module doc comment at the top of the
file is unchanged from what the earlier spec quoted — this part is still
accurate.

The earlier spec's proposed fix was CORPUS, a **separate** RDM reference
feed. Real data shows this is not necessary, or at least not necessary as
a *third* feed: **`RJTTF942MCA.txt`'s own `TI` records already carry
STANOX, TIPLOC, and CRS together, in one record**, decoded field-by-field
against the published `TI` layout:

```
TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON
```
→ TIPLOC `EUSTON `, STANOX `72410`, CRS `EUS`. Same for Watford Junction
(`WATFDJ ` → STANOX `71040` → CRS `WFJ`) and Carlisle (`CARLILE` → STANOX
`09161` → CRS `CAR`). This is a genuine, structural correction: **if this
app ever ingests the CIF schedule feed at all (which the earlier spec
already established is required for the schedule side of this feature,
independent of the CORPUS question), the STANOX↔CRS mapping trust-consumer
needs comes along for free as a byproduct of the same file** — not a
separate feed dependency.

**With one real caveat, also only visible from real data**: `TI` records'
CRS field is blank for the large majority of TIPLOCs —

```
$ awk '.../^TI/{crs=substr($0,54,3); ...}' MCA
blank CRS: 8510   non-blank CRS: 3575   (of 12,085 total TI records)
```

— which is *mostly* expected (most TIPLOCs are signals/junctions/sidings
with no CRS at all, e.g. `TIABHL81100937802LEDINBURGH SIGNAL 811`), but
it also affects at least one real passenger-relevant TIPLOC: `WATRLMN`'s
own `TI` record has a **blank** CRS field —

```
TIWATRLMN16559801RLONDON WATERLOO           87212   0
```
— despite being the TIPLOC real Waterloo schedules actually use (see
Claim 2), and despite operationally corresponding to CRS `WAT`. A naive
"read CRS straight off the `TI` record" join would silently fail for this
station.

The companion `RJTTF942MSN.txt` (Master Station Names — same file drop,
same product, no extra feed) closes exactly this gap: its `A` (station)
records are **100% populated** with CRS — all 3,302 of them:

```
A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815
A    LONDON WATERLOO               9WATRLOOWAT   WAT15312 6179815
A    LONDON WATERLOO               9WATRINTWAT   WAT15312 6179815
```
`WATRLMN` → CRS `WAT`, confirmed, exactly where the `TI` record left it
blank. But MSN does **not** carry STANOX at all — its `A` record layout
(`Name(30) / CATE(1) / TIPLOC(7) / SubsidiaryCRS(3) / CRS(3) / Easting(5)
/ EstFlag(1) / Northing(5) / ChangeTime(2)`) has no STANOX column,
confirmed by decoding the full 81-byte record and finding only OS
grid-reference fields where a STANOX might otherwise be expected.

**Net correction**: the earlier spec framed CORPUS as *the* closer of
trust-consumer's STANOX gap, and treated it as one of "three new feeds"
this feature would cost. Real data shows the schedule extract this app
would need to ingest *regardless* (for schedule/calling-point knowledge)
already bundles both halves of the mapping — `TI` records in `MCA` for
STANOX, `A` records in `MSN` for complete CRS — in the same drop, at no
extra feed cost. **This is one fewer new data-feed dependency than the
earlier spec assumed** — CORPUS may still be worth having for extra
robustness/currency (nightly-updated, purpose-built, possibly cleaner for
edge cases like renamed/decommissioned TIPLOCs), but it is no longer
obviously *necessary*, which is a real, positive correction to the
feasibility picture the earlier spec painted.

## Claim 4 — Scale/volume and update cadence

**Verdict: scale ASSESSED (concrete numbers now exist); cadence STILL
OPEN.**

Concrete numbers for "how many schedule records exist," direct from the
full-file record-type tally above:

- **488,798** `BS` (schedule) records total: 136,205 `P` (permanent base),
  149,201 `O` (overlay/variation), 122,230 `N` (new), 81,162 `C`
  (cancellation-only, no body).
- **407,636** schedules carry a full body (`LO`+`BX`+`LT`, one each) —
  the `P`/`O`/`N` total, exactly.
- **6,803,900** `LI` (intermediate calling point) records — an average of
  ~16.7 intermediate stops per full-bodied schedule.
- **12,085** distinct `TI` (TIPLOC) locations nationwide; of those,
  **3,302** are CRS-bearing passenger stations per `MSN`.
- **8,631,021** total lines in the one 707.7MB `MCA` file.

This is a real, load-bearing number for the earlier spec's still-open
"is the coverage gain worth the ingestion cost" question: a full national
extract is on the order of half a million individual schedule records,
before any per-day filtering down to a specific line's TIPLOC set. Any
ingestion design (Option B in the earlier spec) needs to hold or index
against a working set of this size, not a handful of records — a much
more concrete number than the earlier spec had to reason with.

**Coverage window, not just one day**: sampled `BS` date ranges span
multi-month windows in successive, slightly-overlapping chunks under the
same or related UIDs — e.g. one `ZTR` UID (`Z01401`) recurs across
`260407–260502`, `260505–260523`, `260526–260829`, `260901–261102` (7
April 2026 through into November 2026), and the `WCML` `EUSTON` sample
above runs `2605172612060000001` → `2605172612060000001` decodes to
17/05/2026–06/12/2026 for that particular base schedule. **This is not
"today's trains" — it is the whole current+upcoming timetable period**,
encoded as many date-ranged base records plus overlays, matching (and
now confirming with real numbers) the earlier spec's inference that a
"full" extract means the whole live timetable book, not a single day's
slice.

**Cadence — still genuinely unresolved, and this pass cannot resolve it**:
this is one file, generated once (28/08/2026, per the `DAT`/`MSN` banner
lines — not the stale `HD` record date, see Claim 1). A single sample
cannot establish periodicity: it says nothing about whether a live feed
would mean re-pulling a ~700MB full file daily (per the earlier spec's
`CIF_ALL_FULL_DAILY` citation), applying much smaller `CIF_ALL_UPDATE_DAILY`-style
deltas most days, or some other cadence. The earlier spec's open question
#1 (RDM listing/cadence/cost) is untouched by having this sample — it
answers "what does the data look like," not "how often does new data
arrive, from where, at what cost."

## Anything else real data revealed

- **`REJ` (rejected trains) file is empty** in this sample — `Start of
  rejected trains file` / `End of rejected trains file` with nothing
  between them — meaning this particular export validated cleanly.
  Minor, but confirms the rejection-reporting mechanism the format
  defines is real and currently reporting zero problems, not merely
  present-but-silent.
- **`ZTR` is a second, independent CIF-format schedule file** (its own
  `HD`/`BS`/`BX`/`LO`/`LI`/`LT` records, ~2.9MB, `Z`-prefixed train UIDs)
  bundled alongside the main `MCA` file. Neither the earlier spec nor any
  banner/header in this drop explains what subset of services this
  represents (a guess: freight, possessions, or other non-timetabled
  `Z`-headcode services, based on the UID prefix convention, but this is
  a guess, not confirmed — flagging per this app's own "don't invent API
  details" convention rather than asserting it). If line-level delay
  inference is ever built, whether `ZTR` schedules should be included or
  excluded is a real design question this sample surfaces but doesn't
  answer.
- **`FLF`/`ALF` are duplicate-looking but differently-formatted
  interchange/walk-link files** (`ADDITIONAL LINK: WALK BETWEEN X AND Y
  IN N MINUTES` prose vs. `M=WALK,O=X,D=Y,T=N,...` CSV-like), and `TSI`
  is a TOC-pair minimum-connection-time file keyed by CRS
  (`AFK,SE,SN,6,(Ashford International)`). None of the three carry
  schedule or STANOX/CRS/TIPLOC-mapping data relevant to this feature;
  noted only because their presence in the bundle is one more data point
  toward (not proof of) this being the standard ATOC/RSP companion-file
  set discussed in Claim 1.

## Does this change the recommendation?

**No — "proceed with caveats, not yet" still stands, unchanged in its
bottom line, though the caveats are now narrower and better-evidenced.**

What real data *did* move: the format, STP-overlay, and TIPLOC-join
claims all move from "confirmed via documentation" to "confirmed against
real bytes" — no surprises there, which is itself useful (nothing in the
earlier spec's structural assumptions was wrong). The CORPUS finding is a
genuine, positive correction: **one of the earlier spec's "three new data
feeds" (TRUST at wider scope, CIF SCHEDULE, CORPUS) is very likely not
needed as a separate feed at all** — `TI`+`MSN`, both already inside the
schedule extract this feature needs regardless, appear sufficient for the
STANOX↔CRS join that was the whole reason CORPUS was proposed. That's a
real reduction in the earlier spec's stated cost side.

What real data did **not** move, and could not have: the two things that
actually decided "not yet" in the earlier spec were (a) CIF SCHEDULE's
RDM approval lag, licensing, and cost being unconfirmed, and (b) whether
Darwin's already-fused TRUST+TD+human-input prediction leaves enough of a
real accuracy gap to justify a homegrown TRUST-vs-schedule diff at all.
Having one static reference file in hand — however genuine — answers
neither. It doesn't tell us how this file would arrive on an ongoing
basis, from whom, under what licence or approval process (this file's own
route into this sandbox is unknown to this pass), and it says nothing
about live TRUST correlation accuracy, which requires live TRUST data
against live disruption days, not a schedule snapshot. Those remain
exactly as open as the earlier spec left them, and the earlier spec's own
concrete next step — (a) confirm RDM/licensing terms directly, (b) run a
cheap empirical LDBWS-vs-TRUST validation pass once live TRUST access
exists in production — is still the right next step, now with one fewer
open question to chase (CORPUS) and firmer confidence in the format/join
assumptions than before.
