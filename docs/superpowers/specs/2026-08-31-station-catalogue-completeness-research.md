# Station Catalogue Completeness — Scoping Research

**Status: research/scoping only, not an approved plan.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(landscape survey plus a recommendation) and grounded the same way
`docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md` is grounded
in `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md` — this
document is the gap-analysis half; a follow-up batched plan (comparable in
shape to that one) would be the execution half, not written here. Unlike
the other-UK-networks research, this isn't a "should we bother" survey —
the repo owner has already confirmed the gap is real and asked for a
scoping/sizing pass toward closing it, so this document ends with a
concrete batch/task-count recommendation rather than a pure go/no-go
verdict.

## Problem statement

`crates/api/src/routes/line_status.rs`'s `get_stop_point_disruption`
handler — which backs the `/stations/[crs]` station-lookup page — finds
disruptions for a station by filtering every catalogue line through
`line.has_station(&crs)`:

```rust
let matching_line_ids: Vec<String> = app
    .config
    .lines
    .iter()
    .filter(|line| line.has_station(&crs))
    .map(|line| line.id.clone())
    .collect();

if matching_line_ids.is_empty() {
    return Ok(Json(vec![]));
}
```

(`crates/api/src/routes/line_status.rs:186-196`). `has_station` (see
"Runtime behavior" below) is a linear scan of `line.stations`, the exact
list `lines/*.toml`'s `[[stations]]` entries populate. If a real,
currently-open station isn't listed in *any* line's `stations` array,
`matching_line_ids` comes back empty and the handler returns `[]` at line
195 — **identical to the response for a station with zero active
incidents.** A user looking up a station this catalogue never listed sees
exactly what a user looking up a station with genuinely good service sees:
an empty list, no error, no "not tracked" signal. This is a real,
silent correctness gap, not a cosmetic one.

`lines/SCHEMA.md`'s own schema table lists `stations` as **required**,
described as "Ordered list of CRS codes from one end to the other" — full,
ordered, end-to-end coverage was always the intended policy. The
omissions documented below are acknowledged, in-repo-visible departures
from that stated policy, made under time pressure during this session's
`docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md` batch
build-out — not a deliberate, sanctioned lighter-weight alternative to
full coverage.

## Scope boundary: `sample_stations` is untouched by this work

Stated once, plainly, up front, because it is the single easiest thing for
a future implementer to get wrong: **this document, and any work that
follows from it, never adds to, removes from, or otherwise touches any
line's `sample_stations` field.**

`lines/SCHEMA.md` defines two station-shaped fields with completely
different jobs:

- **`stations`** (required): the full catalogue/segment-membership list.
  This is what `has_station` and `segment_for` read, what
  `SegmentRegistry` indexes, and what the incident matcher uses to decide
  which lines an incident affects. This is the field with the gap.
- **`sample_stations`** (optional): "CRS codes to poll for LDBWS
  sampling" — a deliberately narrow, hand-curated subset used for
  rate-limited/costly live-departure-board polling (`crates/aggregator`'s
  `infer_from_samples`-style pipeline, per DESIGN.md §6.2). It stays
  exactly as curated today, for every line, for the entire duration of any
  work that comes out of this document.

Adding a minor station to `stations` — even a station that sits in the
literal middle of an already-sampled corridor — is purely a catalogue/
matching-correctness change. It does not imply, suggest, or quietly
justify adding that station to `sample_stations`, which remains a
separate, deliberately-narrow, LDBWS-cost-driven decision outside this
work's scope. Any future task list derived from this document that touches
`sample_stations` has gone out of scope and should be rejected in review.

## Method

Grepped every file under `lines/*.toml` (107 files total as of this
research) for the omission-documenting language this session's line-
catalogue build-out used, in its several real phrasings — `omitted`,
`not individually verified`, `not individually cross-checked`,
`two-source` (used both to confirm sourcing *and*, negatively, to flag
what didn't clear the bar), `session limit`, `truncat[ed]`. Then read the
full comment context (not just the matching line) for a representative
sample spanning GWR, Northern, ScotRail, TfW, Southeastern, Greater
Anglia, EMR, CrossCountry, Thameslink, TransPennine, WMR/LNWR, LNER, and
Merseyrail — roughly 30 of the 69 affected files — to characterize both
the real named-station gap size and the reasons behind it, per the task's
instruction not to stop at "how many files have some gap."

## Quantified scope

**69 of 107 `lines/*.toml` files (~64%) contain "omitted" language**
(`grep -lI omitted lines/*.toml | wc -l`). A broader search adding the
"not verified"/"unverified" variants brings the file count to 78; the
narrower, more precise "omitted" count is used as the headline figure
because it's the phrase actually used for both station-list omissions and
field-level (TIPLOC) omissions, and reading the context confirms it's the
dominant real signal. This is close to, and somewhat higher than, the
roughly-71-file figure already found by a prior grep pass — the exact
number is sensitive to which phrasing variant is searched, which is
expected given the wording differs by which batch/task wrote each file;
69–78 is the honest range, not a single precise count.

**Only the 28 files with no omission language at all are genuinely
complete against `SCHEMA.md`'s "full ordered list" policy**: the original
~20 pre-existing files (`cross-country.toml`, `elizabeth-*.toml`,
`swr-*.toml`, `west-coast-main-line.toml`, `thameslink-core.toml`,
`northern.toml`, etc.) plus a handful of this session's own new files that
happened to need no omissions (`grand-central*.toml`, `hull-trains.toml`,
`lner-ecml.toml`, `lner-hull.toml`, `lumo.toml`, `heathrow-express.toml`,
all six `overground-*.toml` files, `northern-clitheroe.toml`,
`southeastern-hayes-line.toml`, the three WCML branch-to-branch files).

**Not every "omitted" file has a real station-count gap.** Reading the
comments directly surfaces an important nuance a pure file-count misses:
a handful of files that match the grep (`scotrail-far-north.toml`,
`scotrail-aberdeen-inverness.toml`, `scotrail-highland-main-line.toml`,
`tfw-conwy-valley.toml`, `northern-esk-valley.toml`) explicitly state "no
intermediate stations are omitted" — their only "omitted" hits are for
the unrelated TIPLOC field (see below). Around 20 of the 69 files' only
"omitted" language is TIPLOC-related ("TIPLOCs are omitted throughout:
none were confirmed against a live source"), which is a real but
much smaller, non-station-count issue — `tiploc` is an optional field
per `SCHEMA.md`, used only to help TRUST/SCHEDULE correlation, and its
absence doesn't affect `has_station`/station-lookup correctness at all.

**Real, named missing-station count**: reading the actual comment text in
~25 files that name specific omitted stations (not just "minor
intermediate calls are omitted" boilerplate) surfaces roughly **190
individually-named missing stations** in that sample alone. A
non-exhaustive tally from files read in full:

| File | Named omitted stations (rough count) |
|---|---|
| `scotrail-glasgow-suburban.toml` | 19 named (Jordanhill, Anniesland, Westerton, Bearsden, Hillfoot, Scotstounhill, Garscadden, Yoker, Clydebank, Singer, Drumry, Kilpatrick, Bowling, Renton, Cardross, Alexandria, High Street, Uddingston, Bellshill) + an entire unmodelled Lanarkshire branch group (Whifflet–Coatbridge Central–Hamilton Central–Larkhall–Lanark, several more) |
| `wmr-snow-hill.toml` | ~25 named (Jewellery Quarter, The Hawthorns, Langley Green, Rowley Regis, Old Hill, Cradley Heath, Lye, Hagley, Blakedown, Fernhill Heath, Bordesley, Small Heath, Spring Road, Hall Green, Yardley Wood, Wythall, Earlswood, Wood End, Danzey, Wootton Wawen, Wilmcote, Stratford-upon-Avon Parkway, Acocks Green, Olton, Widney Manor) |
| `gwr-west-of-england.toml` | 17 named (Reading West, Theale, Aldermaston, Midgham, Thatcham, Newbury Racecourse, Kintbury, Hungerford, Bedwyn, Savernake Low Level, Pewsey, Woodborough, Patney and Chirton, Lavington, Edington, Bratton, Frome) |
| `northern-cumbrian-coast.toml` | 20 named (Askam, Kirkby-in-Furness, Foxfield, Green Road, Silecroft, Bootle, Ravenglass, Drigg, Seascale, Sellafield, Braystones, Nethertown, St Bees, Corkickle, Parton, Harrington, Flimby, Aspatria, Wigton, Dalston — comment itself ends "etc.", implying more) |
| `tpe-south.toml` | 9 named (Liverpool South Parkway, Warrington West, Warrington Central, Birchwood, Irlam, Urmston, Manchester Oxford Road, Meadowhall, Barnetby, Habrough) |
| `northern-calder-valley.toml` | 9 named (Bramley, New Pudsey, Laisterdyke, Low Moor, Brighouse, Mytholmroyd, Hebden Bridge, Littleborough, Castleton) |
| `northern-airedale.toml` | 8 named (Manningham, Frizinghall, Saltaire, Bingley, Crossflatts, Steeton & Silsden, Kildwick & Crosshills, Cononley) |
| `wcml-birmingham.toml` | 8 named (Canley, Tile Hill, Berkswell, Hampton-in-Arden, Marston Green, Lea Hall, Stechford, Adderley Park) |
| `gwr-cornish-main-line.toml` | 6 named (St Germans, Devonport, Keyham, St Budeaux Ferry Road, Saltash, Menheniot) |
| `gwr-thames-valley.toml` | ~8 named (Ealing Broadway, Southall, Hayes & Harlington, West Drayton, Iver, Langley, Burnham, Taplow) |
| `xc-stansted.toml` | 5 named (Melton Mowbray, Oakham, Stamford, March, Audley End) |
| `emr-rural-branches.toml` | 5 named currently-open gaps (Netherfield & Colwick, Radcliffe-on-Trent, Aslockton & Whatton, Elton & Orston, Bottesford), plus several correctly-excluded long-closed stations that are *not* a real gap |
| `southeastern-chatham.toml` | ~9 named (West Dulwich, Sydenham Hill, Penge East, Kent House, plus the unconfirmed Minster/Sandwich/Deal/Walmer/Martin Mill coastal-loop stretch) |
| `gwr-south-wales.toml` | 3 named (Patchway, Pilning, Severn Tunnel Junction) |
| `northern-hope-valley.toml` / `emr-regional.toml` | ~7 named (Chinley, Edale, Hope, Bamford, Hathersage, Grindleford, Dore & Totley) |
| `northern-lakes.toml` | 2 named (Burneside, Staveley) |
| `tpe-borders.toml` | 2 named (Cramlington, East Linton) |
| everything else read in full | several more 1–9-name pockets each (`southeastern-main-line.toml`, `northern-wharfedale.toml`, `northern-tyne-valley.toml`, `lnwr-birmingham-crewe.toml`, `gwr-cotswold.toml`, `tfw-marches.toml`, `tpe-north.toml`) |

On top of this named sample, roughly **20 more files use only the
unenumerated "minor intermediate calls are omitted; only principal
stations are listed" boilerplate** (verbatim or near-verbatim, e.g.
`xc-cardiff.toml`, `xc-manchester.toml`, `xc-south-coast.toml`,
`tpe-anglo-scottish.toml`, `northern-furness.toml`,
`northern-yorkshire-coast.toml`, `northern-blackpool.toml`,
`southern-brighton-main-line.toml`) — these files don't name the specific
missing stations in-comment, but by comparison with the named examples on
similarly-shaped routes, each likely represents another handful (roughly
3–10) of real, unenumerated missing stations.

**Putting it together**: against the catalogue's current 1,731 total
`[[stations]]` entries (`grep -c '^\[\[stations\]\]' lines/*.toml | awk`),
a reasonable order-of-magnitude estimate is **roughly 300–450 real,
currently-open, currently-served stations missing catalogue-wide** — a
proportional gap of very roughly 15–25% on top of what's already listed.
This is deliberately a range, not a false-precision point figure: it
mixes directly-counted named stations with an estimate for the
unenumerated-boilerplate files, and a full count is only obtainable by
actually doing the fill-in research (see Recommendation).

A meaningfully sized minority of what the "omitted" grep surfaces is
**not** a real gap at all: closed/non-existent historical stations
correctly left out (e.g. `gwr-bristol-suburban.toml`'s 7 pre-1970 closures
on the Heart of Wessex line; several pre-Beeching Robin Hood Line stations
in `emr-rural-branches.toml`; `tfw-valley-lines-north.toml`'s two 1950s/
60s Aberdare-branch halts). These were investigated and correctly
excluded — closing this gap must not "rediscover" and re-add them.

## Why the omissions happened

Reading the comments directly, the gap sorts cleanly into three buckets,
confirming the task's expected split:

**(a) Genuinely couldn't source to the two-source bar in the time
available — needs real per-station research work.** The smaller bucket,
but real and named explicitly in several files:
- `scotrail-glasgow-suburban.toml`'s own words: "this task could not
  independently verify their CRS codes/exact station order to the
  two-source standard within its research budget, so they are left out
  rather than guessed" (for the unmodelled Lanarkshire branch group).
- `tfw-heart-of-wales.toml` and `tfw-marches.toml` both disclose a live
  tooling failure, not just time pressure: railwaycodes.org.uk's
  per-letter pages "repeatedly truncated" before reaching the relevant
  station row, and `tfw-heart-of-wales.toml` additionally hit a hard
  `WebFetch` session-usage limit mid-research ("this task's WebFetch tool
  hit a hard session-usage limit... partway through"). 15 files in total
  disclose this exact truncation/session-limit failure mode
  (`chiltern-main-line.toml`, `great-northern-kings-lynn.toml`,
  `great-northern-suburban.toml`, `southeastern-metro-north-kent.toml`,
  `southern-coastway-east.toml`, `southern-brighton-main-line.toml`,
  `southern-coastway-west.toml`, `emr-regional.toml`, `tfw-marches.toml`,
  `tfw-valley-lines-south.toml`, `thameslink-southern.toml`,
  `tfw-conwy-valley.toml`, `tfw-heart-of-wales.toml`,
  `southeastern-chatham.toml`, `tfw-cambrian.toml`) — a tooling
  limitation of that curation session, not a property of the stations
  themselves, so it should resolve cleanly with a fresh research pass.
- Most TIPLOC-only omissions ("TIPLOCs are omitted throughout: none were
  confirmed against a live source") are this bucket too, but are a
  smaller, separate, lower-priority problem (optional field, doesn't
  affect station-lookup correctness).

**(b) Not needed to establish segment/junction boundaries, so simply
wasn't prioritized in the time available.** The dominant pattern by far.
The recurring, near-verbatim phrase across dozens of files —
"only principal stations are listed; minor intermediate calls are
omitted, matching the `xc-manchester.toml`/`xc-south-coast.toml`
convention" — describes a deliberate scoping choice made once early in
this session's batch build-out and then propagated file-to-file as
precedent, not a station-by-station research failure. `gwr-cotswold.toml`
is explicit that this was a *conclusion*, not a gap: intermediate halts
were "independently confirmed... to carry only a minimal local service...
rather than the broadly-hourly London pattern," so they were positively
investigated and then scoped out as non-principal, not left un-researched.
This bucket is good news for the fix: the segment structure is already
correct and verified end-to-end for these lines; filling the gap is
"only" adding intermediate stations to an already-correctly-modelled
line, tagging each with whatever segment name already covers that
stretch (see "Runtime behavior" below — no new segment judgment calls are
needed for a station that sits strictly between two already-modelled
stations on the same physical stretch).

**(c) Something else — deliberately out-of-scope branches/loops, not a
"minor station" omission at all.** A smaller third category: entire
unconfirmed branch continuations rather than individual missing stations
— `southeastern-chatham.toml`'s unconfirmed Minster–Martin Mill coastal
loop stretch, `southeastern-highspeed.toml`'s unconfirmed
Ashford–Canterbury West–Thanet Parkway–Ramsgate pattern,
`thameslink-bedford.toml`'s unconfirmed peak turn-back pattern short of
Bedford. These read more like "a whole sub-route needs its own research
task" than "one more station to add to an existing list," and should be
scoped and sized separately from the plain minor-station fill-in work
this document otherwise describes.

## Runtime behavior: what happens when a station is added mid-segment

Checked `crates/common/src/lib.rs:456-465` (`has_station`, `segment_for`)
and `crates/aggregator/src/segments.rs` (`SegmentRegistry`) directly.

**No special handling is needed for a minor station added strictly
between two already-modelled stations on the same segment.** The data
model is purely per-station:

```rust
pub fn has_station(&self, crs: &str) -> bool {
    self.stations.iter().any(|s| s.crs == crs)
}

pub fn segment_for(&self, crs: &str) -> Option<&str> {
    self.stations.iter().find(|s| s.crs == crs).and_then(|s| s.segment.as_deref())
}
```

`SegmentRegistry::new` (`crates/aggregator/src/segments.rs:18-35`) builds
its `segment_lines` (segment → line IDs) and `station_segments`
((line, CRS) → segment) maps by iterating every station in every loaded
line and reading that station's own `segment` field — there is no
adjacency/ordering logic that would need updating when a new row is
inserted into the middle of `stations`. A newly-added minor station simply
inherits whichever `segment` name already covers its stretch of track
(the same segment its immediate neighbours already carry), same as any
other station in that file, and `is_shared`/`is_exclusive_to`/
`segments_touched_by` all work correctly on it with zero code changes —
this is exactly the same mechanism the existing ~1,731 stations already
rely on.

**One place ordering *does* matter, and is worth flagging for
implementers**: `SCHEMA.md`'s curation rule that `stations` be ordered
geographically end-to-end, because `stations_between()`
(`crates/common/src/lib.rs:475-485`) slices the station list by index
position between two CRS codes to resolve "lines blocked between A and B"
incident messages. A minor station inserted at the *wrong* position in
the list (not geographically between its true neighbours) wouldn't break
compilation or matching correctness for direct-hit incidents, but would
silently corrupt `stations_between()`'s inclusive-range result for any
incident phrased as a between-two-stations range that happens to span the
new station. This is a real but easily-avoided risk — insert each new
station in true geographic order, per the existing curation rule, and it
doesn't apply.

**Performance/complexity**: not a real concern, checked directly.
`get_stop_point_disruption` calls `has_station` (an O(stations-in-that-
line) linear scan) once per line in `app.config.lines` per HTTP request
(`crates/api/src/routes/line_status.rs:186-192`) — today, roughly 107
lines × up to a few dozen stations each, a few thousand simple string
comparisons per request. Even a worst-case +450 stations catalogue-wide
(the top of this document's estimated range) roughly doubles that to a
still-negligible few thousand more comparisons per request — microseconds
of work, not a scaling concern worth engineering around for this body of
work. `SegmentRegistry::new` is built once (`HashMap` inserts over every
station in every line at load time), also unaffected by station count at
this scale. No code change to either file is needed as part of closing
this gap.

## Sourcing bar (restated, not lowered)

Every existing file in this catalogue — including the ~69 with
omissions — was built and reviewed against the same convention:
**every station, CRS code, and segment fact must be independently
verified against two live sources** (Wikipedia's station infobox/article,
cross-checked against a second independent source — typically
railwaycodes.org.uk's CRS/TIPLOC tables, National Rail Enquiries'
`nationalrail.co.uk/stations/<crs>/details.html` pages, or occasionally a
TOC's own site or a franchise service-level-commitment document). A fact
that can't be confirmed to this standard gets a comment explaining what
wasn't confirmed and is left out, never guessed — the exact discipline
`docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md`'s Global
Constraints already codify ("Never invent a station list, segment
boundary, or ATOC code... If a fact can't be confirmed to that standard,
say so in a `.toml` comment and leave it out rather than guess").

**This is the bar any fill-in work must clear, unchanged.** The
overwhelming majority of today's gap (bucket (b) above — "not
prioritized," not "couldn't be sourced") is not evidence the bar is too
high for minor stations; it's evidence the *first* pass had a time budget
that ran out before reaching every minor station on every line, while
correctly reaching every station needed to establish each line's segment
structure. Loosening the bar for "minor" stations specifically would be
the wrong fix and is explicitly not recommended here: the entire reason
several stations are in bucket (a) — genuinely unconfirmed — is that they
*didn't* clear this exact bar in the time available, and the point of a
follow-up pass is to actually do that research, not to declare the gap
closed by accepting single-source or invented data for the stations that
were hardest to confirm the first time.

## Recommended completion approach

**Shape: closer to a single coordinated multi-batch plan than to the
12-batch, from-scratch `line-catalogue-coverage.md` structure — batched
by operator/region, but each batch is "read one existing file, extend
it," not "research and draft a new file from a gap-analysis paragraph."**
The work differs from the original build-out in a way that changes its
shape, not just its size:

- **No new segment-boundary judgment calls, in the common case.** Per
  "Runtime behavior" above, a minor station strictly between two already-
  modelled stations just inherits the existing segment name. This removes
  the single most research-expensive part of the original plan (deciding
  whether two lines share a segment at a junction) for the bulk of the
  work. The exceptions are bucket (c) items — unconfirmed branch/loop
  continuations — which do carry real segment-boundary research and
  should be scoped as their own tasks, sized like an original-plan task,
  not folded into routine fill-in work.
- **No new files.** Every task edits an existing, already-reviewed
  `lines/*.toml` file in place, adding `[[stations]]` entries at the
  correct geographic position and re-verifying/adjusting any comment that
  described the now-filled gap. This is lower-ceremony than drafting a
  new file (no new `id`/`category`/`operators` decisions, no new
  regression-test scaffolding beyond what already exists for that line —
  though a new mid-segment station **does** deserve a short assertion
  that `has_station` now returns true for it, and, where the line has a
  shared-trunk sibling, that the new station resolves the correct
  `MatchScope`).
- **Batch by file, not by named-station-list, to keep each task
  reviewable.** Given the ~69 affected files split roughly:
  - ~25 files with **named** omitted stations in-comment (the table
    above and its unlisted siblings) — the research cost here is lower
    per station, since the target station names, and often even which
    two sources partially confirmed them already, are already written
    down; the task is closing out verification, not starting from
    nothing.
  - ~20 files with only **unenumerated** "minor intermediate calls
    omitted" boilerplate — these need a fresh from-Wikipedia-route-diagram
    pass per file to even produce the candidate station list before
    verification starts, closer in cost to the original plan's per-file
    research step.
  - ~15 files whose gap is attributable specifically to a **disclosed
    tooling failure** (railwaycodes.org.uk page truncation, a hit
    `WebFetch` session limit) rather than a scoping choice — likely the
    cheapest bucket to close, since a fresh fetch of the same page in a
    normal session should just work.
  - A handful of **bucket (c)** unconfirmed-branch tasks (Southeastern's
    Chatham coastal loop and Ashford–Ramsgate pattern, Thameslink
    Bedford's peak turn-back pattern) that need scoping as their own,
    larger, single-purpose tasks.

**Rough task-count estimate**: treating each of the ~69 affected files as
one task (consistent with the original plan's "one file, one task"
granularity, adapted here to "one file, one fill-in pass" since no new
files are created) gives **roughly 65–75 tasks**, organized into batches
by TOC/region mirroring the original plan's own batch boundaries (GWR,
Northern, ScotRail, TfW, Southeastern+GTR, Greater Anglia, EMR, TPE,
WMR/LNWR, XC, Merseyrail — the same clusters `line-catalogue-coverage.md`
already used, since they're the same files). This is comparable in total
task count to that original 12-batch plan (which ran ~85 file-level
tasks across its 12 batches) but meaningfully smaller in per-task
research cost for the ~25 named-station and ~15 tooling-failure files
(more than half the total), since the segment-boundary question — the
single most expensive part of the original build-out — is already solved
for the large majority of this work. A reasonable batching mirrors the
original plan's groupings directly (one batch per TOC/region cluster,
same 10–14 file-task ceiling per batch to stay reviewable), which would
produce roughly **8–10 batches**, somewhat fewer than the original 12
since London Overground, WCML-branch-split, and Chiltern/c2c/Merseyrail
don't need a repeat pass (their files have no omission language at all).

Two structural differences from the original plan worth calling out
explicitly for whoever writes the execution plan:

1. **Verification-budget/tooling-failure tracking deserves its own
   explicit checklist item per task**, given how often (15 of 69 files)
   the *reason* for a gap was a tool limitation rather than a scoping
   decision — a future task list should flag which of its files fall in
   this bucket, since those are the ones most likely to close quickly.
2. **The three closed-station false positives found while reading this
   session's comments should not be "fixed."** A future plan's Global
   Constraints should explicitly note that `gwr-bristol-suburban.toml`,
   `emr-rural-branches.toml`, and `tfw-valley-lines-north.toml`'s
   currently-listed closed/historical stations are correct exclusions,
   not gaps — re-litigating them would waste a task slot on work already
   done correctly.

## Explicit non-goals (repeated for a future implementer)

- **Never expand `sample_stations`.** Restated from the dedicated section
  above — this is worth repeating at the point where a future task list
  gets written, since "we're adding more stations to this line" is
  exactly the framing under which someone might reach for the wrong
  field by habit.
- **Never lower the two-source verification bar** for stations judged
  "minor." A station that can't be confirmed to the existing standard
  gets a comment and stays out, exactly like today — the whole point of
  this work is closing the bucket-(b)/bucket-(a) gap by doing the
  research, not by relaxing what counts as done.
- **No aggregator/matcher code changes are implied or required** by this
  work, per "Runtime behavior" above — this is a pure data-completeness
  effort using the existing `LineDefinition`/`SegmentRegistry`/matcher
  pipeline exactly as it stands today, the same framing
  `line-catalogue-coverage.md` used for the original build-out.
- **No new `lines/*.toml` files.** Every task in a future execution plan
  edits an existing file.

## References

- `lines/SCHEMA.md` — schema, `stations` vs `sample_stations` field
  definitions, shared-trunk rule of thumb.
- `crates/api/src/routes/line_status.rs:182-220` —
  `get_stop_point_disruption`, the handler with the silent-empty-list
  behavior this document is scoped around.
- `crates/common/src/lib.rs:396-486` — `Station`, `LineDefinition`,
  `has_station`, `segment_for`, `stations_between`.
- `crates/aggregator/src/segments.rs` — `SegmentRegistry`, confirmed
  purely per-station-`segment`-field-driven, no adjacency logic to update.
- `docs/superpowers/plans/2026-08-29-line-catalogue-coverage.md` — the
  original batched build-out plan this document's recommended approach is
  modelled on (same batch-by-TOC shape, same per-task testing
  convention), including its own follow-up note about the
  `northern-furness.toml` segment-sharing bug found during Batch 8 review
  (unrelated to this document's gap, but the same "read the existing file
  before assuming it's finished" discipline applies).
- `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
  — structural analog for this document's landscape-survey-plus-
  recommendation shape.
- Representative files read in full for the named-station tally:
  `lines/scotrail-glasgow-suburban.toml`, `lines/wmr-snow-hill.toml`,
  `lines/gwr-west-of-england.toml`, `lines/gwr-thames-valley.toml`,
  `lines/gwr-cotswold.toml`, `lines/gwr-cornish-main-line.toml`,
  `lines/gwr-south-wales.toml`, `lines/gwr-bristol-suburban.toml`,
  `lines/northern-cumbrian-coast.toml`, `lines/northern-calder-valley.toml`,
  `lines/northern-airedale.toml`, `lines/northern-hope-valley.toml`,
  `lines/northern-lakes.toml`, `lines/northern-tyne-valley.toml`,
  `lines/northern-wharfedale.toml`, `lines/wcml-birmingham.toml`,
  `lines/lnwr-birmingham-crewe.toml`, `lines/emr-midland-main-line.toml`,
  `lines/emr-rural-branches.toml`, `lines/emr-regional.toml`,
  `lines/xc-cardiff.toml`, `lines/xc-stansted.toml`,
  `lines/tfw-heart-of-wales.toml`, `lines/tfw-marches.toml`,
  `lines/tfw-valley-lines-south.toml`, `lines/tfw-valley-lines-north.toml`,
  `lines/southeastern-main-line.toml`, `lines/southeastern-chatham.toml`,
  `lines/southeastern-highspeed.toml`, `lines/greater-anglia-main-line.toml`,
  `lines/greater-anglia-norfolk-branches.toml`,
  `lines/greater-anglia-stansted-express.toml`, `lines/thameslink-bedford.toml`,
  `lines/thameslink-cambridge.toml`, `lines/thameslink-southern.toml`,
  `lines/tpe-north.toml`, `lines/tpe-south.toml`, `lines/tpe-borders.toml`,
  `lines/scotrail-far-north.toml`, `lines/scotrail-aberdeen-inverness.toml`,
  `lines/scotrail-highland-main-line.toml`, `lines/scotrail-west-highland-oban.toml`.
