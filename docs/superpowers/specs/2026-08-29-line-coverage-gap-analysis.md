# Line Catalogue Coverage Gap Analysis

**Status: research pass only. Nothing in this document has been actioned.**
No `.toml` line definitions were written as part of this work — that's
real curation work (station-by-station, segment-by-segment, matching the
rigor of the 20 files that already exist under `lines/`) and belongs to a
later, separate effort once this analysis has been reviewed. This document
answers one question: **what's missing, how big is each gap, and in what
order should it be tackled** — not "here is the content of the missing
files."

Written to the same rigor/tone as this repo's other spec docs (see e.g.
`docs/superpowers/specs/2026-08-28-train-tracking-design.md`) but it is
explicitly an audit, not a feature design, so it's structured as one
section per missing TOC/network rather than goals/architecture/tasks.

## Method

1. Read `lines/SCHEMA.md` and `DESIGN.md` §5.1–5.2 (the "line"/segment
   model) in full, then every one of the 20 existing `lines/*.toml` files,
   to calibrate what "one line entry" means in this catalogue and how
   multi-route operators are already split (CrossCountry: spine + 4 named
   arms; Northern: trunk + 6 named regional branches; SWR: trunk + 2 named
   branches; Elizabeth line: 3 branches sharing a central-section segment).
2. Read `docs/superpowers/plans/2026-08-22-elizabeth-line-merge.md` as a
   template for how a prior line-catalogue-adjacent effort was scoped —
   it's a read-side merge, not a new-line addition, but it confirmed the
   convention that TOC franchise identity and ATOC operator codes are
   treated as separate, both-must-be-current facts (its own mapping table
   comment: "TfW... AW code has persisted through ownership transitions").
3. Verified current TOC franchise holders and route networks with live web
   search — not training-data memory, since the nationalisation programme
   has moved fast in 2025–2026 — cross-checking Wikipedia against a second
   source (railwaycodes.org.uk, Network Rail's own route pages, or each
   TOC's own site) wherever a fact was load-bearing for this document's
   conclusions.

## Confirmed franchise-ownership state (as of 2026-08-29)

The Passenger Railway Services (Public Ownership) Act 2024 is
progressively nationalising English TOCs at contract break points. As of
today:

- **Already public before this programme:** LNER, Northern, Southeastern,
  TransPennine Express (all DfT Operator Ltd), plus ScotRail (Scottish
  Government) and Transport for Wales Rail (Welsh Government).
- **Nationalised under the 2024 Act so far:** South Western Railway
  (25 May 2025), c2c (20 July 2025), Greater Anglia (12 Oct 2025), West
  Midlands Trains — i.e. West Midlands Railway + London Northwestern
  Railway (1 Feb 2026), Govia Thameslink Railway — Thameslink/Southern/
  Great Northern/Gatwick Express (31 May 2026).
- **Confirmed upcoming:** Chiltern Railways (20 Sep 2026), Great Western
  Railway (13 Dec 2026).
- **Still privately operated, dates not yet confirmed:** East Midlands
  Railway (expected late 2026), Avanti West Coast (expected spring 2027),
  CrossCountry (expected autumn 2027).
- **Open access (outside the nationalisation programme entirely — they
  pay track access charges and take full commercial risk):** Grand
  Central, Hull Trains, Lumo, Heathrow Express. Caledonian Sleeper is
  Scottish-Government-owned but structurally separate from ScotRail.
  Eurostar is out of scope (international, not National Rail).

**Why this matters less than it looks like it should for this catalogue:**
`operators` is keyed on ATOC code, and — confirmed independently for both
Transport for Wales (KeolisAmey Wales → Transport for Wales Rail, code
stayed `AW`) and the wider industry pattern cited in the Elizabeth-line
merge plan (GNER's code outliving the GNER brand) — **ATOC codes survive
operator-of-record changes.** A newly-nationalised Southeastern or SWR
keeps its existing code. The nationalisation date matters for getting the
*display name* and *"who currently runs this" prose* right in each line's
comments, not for the `operators` list itself.

Sources: [Timeout — nationalisation dates](https://www.timeout.com/uk/news/when-will-every-major-uk-rail-operator-be-nationalised-full-list-of-routes-and-dates-chiltern-great-western-051226), [Commons Library — franchise transfer tracker](https://commonslibrary.parliament.uk/when-will-my-local-train-operator-be-nationalised/), [gov.uk — SWR transfer statement](https://www.gov.uk/government/speeches/transfer-of-south-western-railways-services-into-public-ownership), [House of Commons Library — open access operators](https://commonslibrary.parliament.uk/research-briefings/cdp-2025-0029/).

---

## P0 — an active correctness bug in existing coverage, not a gap

Before auditing what's *missing*, one existing file is **wrong today** in
a way that's directly relevant to the WCML question this research was
asked to answer, so it's called out ahead of the gap sections.

**`lines/west-coast-main-line.toml` labels operator code `"AW"` as Avanti
West Coast. That's incorrect — `AW` is Transport for Wales' code (carried
over from Arriva Trains Wales); Avanti West Coast's actual code is `VT`
(carried over from Virgin Trains).** Confirmed independently for both
halves of the mixup:

- Avanti West Coast = `VT`: Wikidata's National Operator Code field,
  Wikipedia's Avanti West Coast infobox reporting mark, and a RailUK
  Forums thread on the Virgin→Avanti rebrand all agree, and note the code
  carried over unchanged from Virgin Trains West Coast.
- Transport for Wales = `AW`: Network Rail's own "References and Symbols"
  timetable document lists `AW · Transport for Wales`, and this matches
  the KeolisAmey Wales → Transport for Wales Rail transition (the code
  did not change across that operator switch either — the same pattern
  as GNER→LNER cited in the Elizabeth-line merge plan).

**Effect:** any Knowledgebase incident tagged with Avanti's real operator
code (`VT`) currently fails to match `west-coast-main-line.toml` at all
via the `operators` field — the line is silently blind to Avanti-specific
incidents today, correctness bugs the aggregator's `OPERATOR_ONLY` scope
exists to catch. This is a live-data bug in a currently-shipped file, not
a coverage gap, and arguably deserves fixing before any of the net-new
work below (see Priority list).

**Secondary, lower-confidence flag, same file:** `west-coast-main-line.toml`
also lists both `"LM"` (commented "London Northwestern Railway") and
`"LN"` (commented "West Midlands Trains (London Northwestern services)")
as separate operators. One source (railwaycodes.org.uk, fetched during
this research) states West Midlands Railway *and* London Northwestern
Railway both use code `LM`, with `LN` "not yet used in GBTT/eNRT" —
implying the current file's two-code split may itself be a second,
smaller bug (duplicate/aspirational code) rather than two genuinely
distinct codes. This is a single-source claim and wasn't independently
cross-checked to the same confidence as the AW/VT finding above — flagging
it for verification alongside the AW/VT fix, not asserting it outright.

Sources: [Wikidata — Avanti West Coast](https://www.wikidata.org/wiki/Q68441695), [Wikipedia — Avanti West Coast](https://en.wikipedia.org/wiki/Avanti_West_Coast), [RailUK Forums — Avanti rebrand thread](https://www.railforums.co.uk/threads/avanti-west-coast-rebrand.196404/page-12), [Network Rail — References and Symbols PDF](https://www.networkrail.co.uk/wp-content/uploads/2020/02/NRT-References-and-Symbols.pdf), [TravelWest — Transport for Wales (AW)](https://travelwest.info/tickets-travelcards/rail/transport-for-wales/), [railwaycodes.org.uk — timetable codes](https://www.railwaycodes.org.uk/operators/toccodes.shtm).

---

## Northern — is the existing 6-branch coverage actually complete?

**No.** The current catalogue (`northern.toml` trunk +
`northern-blackpool`/`northern-furness`/`northern-hope-valley`/
`northern-lakes`/`northern-tyne-valley`/`northern-yorkshire-coast`) covers
real, distinct regional routes well, but Northern's network is
substantially larger than these 7 files. Confirmed gaps, in descending
order of how self-evident they are:

- **Cumbrian Coast Line (Barrow-in-Furness → Whitehaven → Carlisle).**
  This isn't a research finding — it's already flagged in-repo:
  `northern-furness.toml`'s own comment says "The Cumbrian Coast route
  continues beyond Barrow to Whitehaven and Carlisle and belongs in its
  own definition." That definition doesn't exist yet.
- **Calder Valley Line** (Leeds/Bradford ↔ Manchester via Halifax and
  Rochdale) — a real, distinct route from the `northern-transpennine`
  segment already in `northern.toml` (which runs via Huddersfield). Two
  genuinely different Leeds–Manchester corridors, both Northern-operated,
  only one modelled.
- **Airedale Line** (Leeds/Bradford Forster Square → Skipton, some
  services on to Carlisle/Lancaster) and **Wharfedale Line** (Leeds/
  Bradford → Ilkley) — share track as far as Shipley; both LNER and
  Northern services use parts of Airedale, which is itself a shared-trunk
  consideration for whichever gets written first.
- **Esk Valley Line** (Middlesbrough → Whitby) — Community Rail line,
  entirely separate from anything currently modelled.
- **Clitheroe/Ribble Valley Line** (Manchester/Blackburn → Clitheroe).
- Assorted Manchester/West Yorkshire suburban branches not yet checked in
  detail (e.g. Manchester–Southport, Wigan–Kirkby) — flagged for the
  follow-up curation pass rather than fully scoped here.

**Estimate:** 5–7 more Northern regional-branch files, on top of the
existing 7, following the exact pattern already established (regional
category, tight severity overrides, `destination_crs_filter`, minor
intermediate calls omitted pending CRS verification where the existing
files already do that).

Sources: [Calder Valley line — Wikipedia](https://en.wikipedia.org/wiki/Calder_Valley_line), [Airedale line — Wikipedia](https://en.wikipedia.org/wiki/Airedale_line), [Wharfedale line — Wikipedia](https://en.wikipedia.org/wiki/Wharfedale_line), [Esk Valley line — Wikipedia](https://en.wikipedia.org/wiki/Esk_Valley_line), [Northern's new network map — Transport Designed](https://transportdesigned.com/introducing-northerns-new-network-map/).

---

## Missing TOC/network sections

Each section: (a) confirmed current operator + route summary, (b) rough
entry count at this catalogue's existing granularity, (c) shared-trunk/
overlap risk with what's already defined, (d) size read.

### Great Western Railway (GWR)

**(a)** FirstGroup-operated; nationalises 13 Dec 2026 (confirmed upcoming,
not yet transferred). Code `GW`. Network: Great Western Main Line
(Paddington–Bristol Temple Meads, Brunel's original route), Cotswold Line
(Didcot/Oxford–Worcester), South Wales Main Line (branches off the GWML
at Wootton Bassett via Bristol Parkway and the Severn Tunnel to Newport/
Cardiff/Swansea), West of England Line / Reading–Taunton line (Reading →
Westbury/Castle Cary → Exeter), continuing as the Cornish Main Line
(Exeter → Plymouth → Penzance, plus the Night Riviera sleeper), and a
large Thames Valley suburban service (Paddington–Reading–Newbury/Oxford
local stopping).

**(b)** This is the single largest gap in the catalogue by any measure.
Calibrated against CrossCountry (spine + 4 arms = 5 files) and Northern
(trunk + 7 branches ≈ 8 files), GWR's route diversity is comparable to
or larger than Northern's: GWML core, Cotswold Line, South Wales Main
Line, West of England Line, Cornish Main Line (arguably one file with West
of England, arguably two — West of England/Cornish is a single named
route with two operationally distinct halves, similar to how
`swr-south-west-main.toml` treats Bournemouth+Weymouth as one file
rather than two), Thames Valley suburban, and likely a Bristol-area
suburban group (Severn Beach line, Bristol–Weymouth via Bath/
Castle Cary). **Estimate: 6–8 files.**

**(c)** Real overlap risk at Reading, already touched by two existing
files: `xc-south-coast.toml` runs through RDG (commented as sharing no
segment, by design) and `elizabeth-line.toml`'s western arm terminates at
RDG. A new GWR Thames Valley entry through Reading needs the same
"station overlap is fine, segment-sharing is a deliberate choice" judgment
call `xc-south-coast.toml` already documents. Also at Cardiff/Newport —
`xc-cardiff.toml` already terminates at CDF/NWP; a GWR South Wales entry
would share those stations (not necessarily segments) with an existing
XC line.

**(d)** Large — highest priority by network size and near-total absence.

### LNER (East Coast Main Line)

**(a)** DfT Operator Ltd (already public). Code `GR`. Network: King's
Cross–Edinburgh core, extending on to Aberdeen/Inverness via ScotRail
metals; branches diverge to Leeds (via Wakefield Westgate), Hull (via
Selby/Brough), Lincoln (via Newark/Grantham), plus Skipton/Harrogate
services that also touch Northern's territory.

**(b)** Structurally the closest existing analogue to CrossCountry — a
spine with named branches sharing an operator code. **Estimate: 4–5
files** (ECML core + Leeds branch + Hull branch + Lincoln branch, possibly
folding Harrogate/Skipton into the Leeds branch rather than a standalone
file, matching how `xc-cardiff.toml` folds Gloucester-area detail into
one file rather than splitting further).

**(c)** Real, multi-way overlap risk — the ECML corridor is dense with
open-access operators plus Northern:
- Grand Central and Hull Trains both run King's Cross–Yorkshire/North
  East services on ECML metals; Lumo runs King's Cross–Edinburgh (now
  extended to Glasgow). All three would need the same "station overlap
  without forced segment-sharing" treatment `xc-south-coast.toml`
  documents for its SWML/Elizabeth-line overlaps.
- Northern's `northern-yorkshire` segment (LDS↔YRK, shared today between
  `northern.toml` and `northern-yorkshire-coast.toml`) is also LNER
  territory between Leeds and York — a genuine candidate for whether LNER
  should share that segment name or define its own, a judgment call for
  whoever writes the LNER files, not resolved here.
- Great Northern (GTR) suburban services share ECML "slow line" tracks
  out of King's Cross as far as Alexandra Palace/Potters Bar/Welwyn —
  relevant once both LNER and Great Northern exist.

**(d)** Large — high priority given LNER is a flagship intercity brand and
currently has zero coverage despite the multi-operator ECML corridor
already having implicit dependents (Northern's Yorkshire segment) that
would benefit from LNER's presence to sanity-check.

### Southeastern

**(a)** DfT Operator Ltd (already public). Code `SE`. Two distinct route
families sharing overlapping London termini and diverging territory: the
South Eastern Main Line (Charing Cross/Cannon Street via Tonbridge to
Ashford/Dover/Hastings) and the Chatham Main Line (Victoria via Chatham
to Ramsgate/Dover), plus the domestic "Javelin" high-speed service on HS1
via Ebbsfleet/Ashford International, plus a large south-east-London metro
suburban network (Dartford Loop, Bexleyheath, Sidcup, Hayes lines) funnelled
through London Bridge/Charing Cross/Cannon Street.

**(b)** Comparable in structural complexity to SWR (trunk + branches) but
with more London termini and an added high-speed layer. **Estimate: 5–6
files** (SE Main Line via Tonbridge, Chatham Main Line, HS1 domestic
Javelin service, one or two metro-suburban groupings).

**(c)** Direct, concrete overlap with the one existing Thameslink file:
`thameslink-core.toml` terminates its documented segment at LBG with the
comment "Trains diverge south of London Bridge — handled by branch line
definitions." Southeastern's metro services also converge on London
Bridge and share tracks south toward Lewisham with Thameslink's
(currently unwritten) Sevenoaks branch. Whoever writes Southeastern's
metro entry and Thameslink's Sevenoaks branch will need to coordinate a
shared segment name through that corridor — this is the single clearest
segment-coordination dependency found in this whole audit.

**(d)** Large — high priority given passenger volume and the concrete
Thameslink-core dependency above.

### Southern / Gatwick Express / Great Northern / Thameslink branches (GTR)

**(a)** Nationalised 31 May 2026 as part of GTR → public ownership; the
constituent brands (Southern `SN`, Gatwick Express `GX`, Great Northern
`GN`, Thameslink `TL`) continue as separate ATOC codes and are likely to
continue as separate line entries, matching this catalogue's existing
`thameslink-core.toml`-only coverage. Southern covers the Brighton Main
Line (Victoria/London Bridge–Gatwick–Brighton) plus Sussex/Surrey/Kent
coastway services (Eastbourne, Hastings, Portsmouth) and the Oxted/
Uckfield branches. Gatwick Express is a single non-stop Victoria–Gatwick
service. Great Northern covers King's Cross/Moorgate–Peterborough/
Cambridge/King's Lynn. Thameslink's *branches* (as opposed to the core,
already covered) fan out to Bedford/Peterborough/Cambridge in the north
and Brighton/Horsham/East Grinstead/Sutton loop/Sevenoaks in the south —
this is explicitly called out as future work in the existing file's own
comment: "Northern and southern branches... are separate lines."

**(b)** This is a second GWR-scale cluster once treated as one group.
**Estimate: 8–10 files** across all four brands (Brighton Main Line,
Southern Coastway east + west, Oxted/Uckfield, Gatwick Express — possibly
folded into Brighton Main Line as a keyword/threshold variant rather than
a full separate file, mirroring how this catalogue hasn't split every
branded service into its own file where journeys are a strict subset —
Great Northern King's Lynn line, Great Northern suburban/Moorgate,
Thameslink Bedford branch, Thameslink Cambridge/Peterborough branch,
Thameslink southern branches to Sutton/Sevenoaks/Brighton-via-Thameslink).

**(c)** Multiple real overlaps: Thameslink's southern branches literally
continue past `thameslink-core.toml`'s LBG terminus southward, sharing
track with Southern's Brighton Main Line entry as far as Windmill Bridge
Jn/East Croydon — almost certainly a shared-trunk segment once both
exist. Thameslink's Bedford branch shares Midland Main Line infrastructure
with EMR (see EMR section below) roughly Bedford–St Pancras. Great
Northern's suburban services share ECML slow lines with LNER (see LNER
section). This is the most overlap-dense missing cluster in the whole
audit — worth sequencing carefully rather than writing branches in
isolation.

**(d)** Large — high priority for passenger volume (Brighton Main Line and
the Thameslink core it already partially covers are among the busiest
commuter corridors in the country) and because leaving Thameslink's
branches unwritten leaves the existing `thameslink-core.toml` structurally
incomplete (it already promises branch files that don't exist).

### East Midlands Railway (EMR)

**(a)** Transport UK Group, not yet nationalised (expected late 2026).
Code `EM`. Midland Main Line (St Pancras–Sheffield via Leicester/Derby/
Chesterfield, with a Nottingham spur), plus EMR Regional intercity
(Liverpool–Norwich via Nottingham/Sheffield), EMR Connect (St Pancras–
Luton Airport local), and rural branches (Robin Hood Line Nottingham–
Worksop, the Poacher/Skegness line, Derby–Matlock).

**(b) Estimate: 4–5 files** (MML core, Nottingham spur — likely folded
into MML core the way SWML folds Weymouth in, Liverpool–Norwich cross-
country regional line, a rural-branches group).

**(c)** MML's southern (Bedford–St Pancras) section is the same
infrastructure Thameslink's northern branch would need — direct
dependency, same as the GTR section above. Liverpool–Norwich crosses
Northern and TPE territory around Sheffield/Manchester without literal
track-sharing in most places (different routes into those cities), lower
risk than the MML/Thameslink overlap.

**(d)** Medium — the MML/Thameslink dependency argues for sequencing EMR's
core file near GTR's Thameslink-branches work rather than treating them
independently.

### TransPennine Express (TPE)

**(a)** DfT Operator Ltd (already public). Code `TP`. Organised (per
TPE's own current timetable structure) into four named routes: an
Anglo-Scottish route (Liverpool/Manchester–Glasgow/Edinburgh via Preston/
Carlisle), a South route (Cleethorpes–Manchester/Liverpool via Sheffield/
Leeds), a Borders route (Newcastle–Edinburgh), and a North route
(Newcastle–Manchester/Liverpool via York/Leeds).

**(b) Estimate: 3–4 files**, mapping close to 1:1 onto TPE's own
four-route structure — an unusually clean fit for this catalogue's
per-route file convention.

**(c)** Heavy station overlap with Northern (Leeds, York, Huddersfield,
Manchester) and with the WCML entry (Preston, Carlisle) and LNER
(Edinburgh, Newcastle), but by the precedent already set in
`xc-manchester.toml` ("no segment is shared with wcml because that line's
segments are far coarser... an incident on the shared stretch matches
both lines by station anyway"), none of this necessarily needs literal
segment-sharing — station-level overlap is expected and already has a
documented resolution pattern in this catalogue.

**(d)** Medium — clean-fitting but station-overlap-heavy; sequence after
Northern's own gaps are filled so the segment-naming precedent is fresh.

### ScotRail

**(a)** Scottish Government-owned (already public). Code `SR`. By far the
largest single network in this audit: Central Belt (Edinburgh–Glasgow via
Falkirk High as the core, plus via Shotts and via Bathgate/North Clyde as
alternate routes), extensive Glasgow suburban electric network (North
Clyde, Argyle Line, Ayrshire Coast/Glasgow South Western Line to Ayr/
Stranraer/Girvan), Fife Circle, the Borders Railway (Edinburgh–
Tweedbank), Highland Main Line (Perth–Inverness), Aberdeen–Inverness
Line, Far North Line (Inverness–Wick/Thurso), Kyle of Lochalsh Line
(Dingwall–Kyle, via Inverness), and the West Highland Line (Glasgow–Fort
William/Mallaig and Glasgow–Oban, materially two different routes off a
shared Glasgow trunk).

**(b)** The largest entry-count estimate in this audit. Even calibrated
conservatively against Northern's 8-file treatment of a comparable
regional operator, ScotRail's network is bigger and more geographically
spread. **Estimate: 8–12 files** (Edinburgh–Glasgow core + alternates
possibly grouped, Glasgow suburban group, Ayrshire/South West Scotland,
Fife Circle + Borders Railway possibly grouped, Highland Main Line, Far
North Line, Kyle Line, West Highland Line Fort William/Mallaig arm,
West Highland Line Oban arm, Aberdeen–Inverness).

**(c)** No overlap with any currently-defined line (nothing else in the
catalogue reaches Scotland except `west-coast-main-line.toml`'s northern
end at Carlisle, continuing to Glasgow Central in a comment but not
modelled as a station). Low coordination risk, purely additive.

**(d)** Very large — lower per-line passenger density than London
commuter routes but the largest raw coverage gap (an entire nation's rail
network currently has zero representation) and zero collision risk, which
makes it a good candidate for high-volume, low-coordination-overhead
parallel work later.

### Transport for Wales (TfW)

**(a)** Welsh Government-owned (already public), code `AW` (confirmed —
see the P0 section above on the AW/VT mixup, which is a strong reason to
get TfW's real code right when this is eventually written, given the
existing catalogue already has this exact code confused with Avanti).
Network: Cambrian Line (Shrewsbury–Aberystwyth, splitting to Pwllheli as
the Cambrian Coast Line), Heart of Wales Line (Shrewsbury/Craven Arms–
Swansea), Conwy Valley Line (Llandudno Junction–Blaenau Ffestiniog),
North Wales Coast Line (Chester–Holyhead, shared with Avanti), Marches
Line (Newport–Shrewsbury–Chester), and the Cardiff Valley Lines — a
dense commuter network (Rhymney, Merthyr, Aberdare, Treherbert, City
Line, Coryton branches) that's structurally more like Merseyrail's
two-line metro than a long-distance route.

**(b) Estimate: 6–9 files** — comparable in count to ScotRail relative to
its smaller passenger volume, because the network is geographically wide
(Cambrian Coast to Cardiff Valleys) even though total ridership is lower.

**(c)** North Wales Coast Line is shared with Avanti West Coast (once
WCML is split — see Priority list) between Chester and Holyhead. South
Wales Main Line stations (Newport, Cardiff, Swansea) overlap with both
`xc-cardiff.toml` and a future GWR South Wales entry at the station
level, same pattern as the other main-line overlaps already discussed.

**(d)** Large — similar scale read to ScotRail, lower urgency given lower
absolute ridership than the England-wide commuter gaps above it.

### Chiltern Railways

**(a)** Arriva-operated until 20 Sep 2026 nationalisation (confirmed
upcoming). Code `CH`. Marylebone–Birmingham Snow Hill main line (via High
Wycombe, Bicester, Warwick, with a Stratford-upon-Avon extension),
Marylebone–Aylesbury stopping service (via Amersham), and a
Bicester North–Oxford branch.

**(b) Estimate: 2–3 files** (Snow Hill main line, Aylesbury line, possibly
folding the Oxford branch into the main line the way `swr-alton.toml`
treats its branch as a standalone but small file).

**(c)** No significant segment overlap with anything currently defined —
Chiltern's route into Birmingham approaches via Solihull/Dorridge, distinct
from both XC's Birmingham hub segments and any future WMR Snow Hill
lines entry (station-level overlap at Birmingham Snow Hill/Moor Street
only, which XC doesn't currently touch at all — XC uses New Street).

**(d)** Small — clean, low-risk, quick win.

### c2c

**(a)** Nationalised 20 Jul 2025 (already public). Code `CC`. A single,
largely self-contained route: Fenchurch Street–Shoeburyness via Basildon
(the London, Tilbury & Southend line), with a minor Ockendon loop branch.

**(b) Estimate: 1 file** — comparable in scope to `thameslink-core.toml`,
possibly 2 if the Ockendon loop is judged distinct enough to split out
the way `swr-alton.toml` splits from the SWML trunk.

**(c)** None found. c2c's route via Limehouse/Barking is physically
separate from anything else in the catalogue; the only shared station is
Barking, which no other current or near-term line touches at the
segment level.

**(d)** Small — clean, low-risk, quick win; smallest real gap in the
audit by scope.

### Greater Anglia

**(a)** Nationalised 12 Oct 2025 (already public). Code `LE`. Great
Eastern Main Line (Liverpool Street–Norwich via Ipswich/Colchester), West
Anglia Main Line (Liverpool Street–Cambridge/King's Lynn), Stansted
Express (Liverpool Street–Stansted Airport), plus Essex branches
(Southminster, Clacton/Walton/Braintree via Colchester), Suffolk branches
(Sudbury, Felixstowe, Harwich), and Norfolk branches (Bittern Line
Norwich–Sheringham, Wherry Lines Norwich–Yarmouth/Lowestoft, Breckland
Line Norwich–Cambridge).

**(b) Estimate: 6–8 files** — comparable in scale to LNER once branches
are counted (GEML core, West Anglia Main Line, Stansted Express, Essex
branches group, Suffolk branches group, Norfolk branches group).

**(c) The overlap this research brief specifically flagged is real and
worth stating precisely.** `elizabeth-shenfield.toml` runs Liverpool
Street → Stratford → Shenfield on the `elizabeth-central` and
`elizabeth-shenfield` segments, terminating at SNF. Greater Anglia's
mainline services to Chelmsford/Colchester/Ipswich/Norwich physically
continue past Shenfield on the same Great Eastern main-line corridor (GA
runs on the "main" tracks, Elizabeth line on dedicated "electric"/metro
tracks from around Bethnal Green outward, but both share the same
physical route corridor into Shenfield, and GA main-line services also
call at some of the same intermediate stations Elizabeth line's Shenfield
branch lists — e.g. Ilford, Romford). Per SCHEMA.md's junction rule
("a junction station belongs to the shared trunk, not the exclusive
segment"), Shenfield is exactly this kind of junction: Elizabeth line
terminates there, Greater Anglia continues beyond. **Whoever writes
Greater Anglia's GEML file needs to make a deliberate call** — either
share a segment name with `elizabeth-shenfield` for the Liverpool
Street–Shenfield stretch (making an incident there propagate to both
lines) or treat it as station-level-only overlap (the `xc-south-coast.toml`
precedent: don't force a shared segment when the lines' exclusive
territory diverges too far beyond the shared bit). This isn't resolved
here — it's flagged as the clearest concrete decision point in the whole
audit, consistent with what the brief asked to check for.

**(d)** Large — high priority given the direct, already-confirmed overlap
with existing Elizabeth line coverage; writing Greater Anglia's core file
is also the natural trigger for finally resolving that segment-sharing
question rather than leaving it implicit.

### West Midlands Railway / London Northwestern Railway

**(a)** West Midlands Trains, nationalised 1 Feb 2026 (already public),
trading as two brands sharing one legal operator and (per this research's
single-source finding above) possibly one ATOC code (`LM`). West Midlands
Railway: Snow Hill lines (Stratford-upon-Avon/Dorridge–Worcester/
Stourbridge) and the Cross-City Line (Lichfield–Birmingham New Street–
Redditch). London Northwestern Railway: Euston–Milton Keynes–
Northampton/Birmingham/Crewe semi-fast commuter services, running on
WCML metals.

**(b) Estimate: 4–5 files** (WMR Snow Hill lines, WMR Cross-City Line,
LNWR London commuter/Euston semi-fast, LNWR Northampton/Crewe local —
possibly fewer if some fold together the way this catalogue already folds
related services into single files elsewhere).

**(c)** Direct, already-partially-acknowledged overlap: `west-coast-main-
line.toml` already lists `LM`/`LN` in its `operators` array today (see
the P0 secondary flag above) precisely because WMT services run on WCML
metals between Euston and points north — but the file doesn't model any
LNWR-specific stations or segments, just claims the operator codes. Once
an LNWR line is actually written, the right precedent to follow is the
one already established by `xc-manchester.toml`: station-level overlap
with WCML's `wcml-london`/`wcml-midlands` segments is fine and expected;
don't force a shared segment name given the coarser granularity of the
existing WCML file.

**(d)** Medium — the operator-code entanglement with the existing WCML
file (and that file's P0 fix) makes this a natural pair to plan alongside
the Avanti/WCML-split work rather than in isolation.

### Merseyrail

**(a)** Merseytravel (already public/PTE-owned, distinct structurally from
the DfT nationalisation programme). Code `ME`. Two lines: the Northern
Line (Southport–Hunts Cross, with Kirkby and Ormskirk branches, running
underground through central Liverpool) and the Wirral Line (a loop via
Liverpool Central/Moorfields out to New Brighton, West Kirby, Chester,
Ellesmere Port).

**(b) Estimate: 2 files** — self-contained metro network, matches the
"two lines" structure directly.

**(c)** Low risk. `northern.toml`'s `northern-merseyside` segment
(LIV–HUY–NLW, i.e. Northern Trains' Liverpool Lime Street–Wigan/Manchester
corridor) is physically and operationally distinct from Merseyrail's own
Northern Line (different Liverpool terminus — Lime Street vs. the
underground loop via Liverpool Central — and third-rail electrified
metro infrastructure rather than Northern Trains' diesel/main-line
services). Worth a note-to-self for whoever writes Merseyrail's files to
confirm no literal station-code collision, but not a load-bearing risk.

**(d)** Small — clean, low-risk, quick win.

### Open access operators (Grand Central, Hull Trains, Lumo, Heathrow Express)

**(a)** All commercial, outside the nationalisation programme, all
currently FirstGroup- or Arriva-owned. Grand Central (`GC`): King's
Cross–Sunderland and King's Cross–Bradford. Hull Trains (`HT`): King's
Cross–Hull (recently expanded). Lumo (`LD`): King's Cross–Edinburgh, now
extended to Glasgow Queen Street. Heathrow Express (`HX`): Paddington–
Heathrow non-stop.

**(b) Estimate: 4 files**, one per operator — each is structurally a
single simple route, closer in scope to `swr-alton.toml` than to any
multi-branch entry.

**(c) Heathrow Express is the one with an already-live, in-repo
precedent to follow, not just a risk to flag:** `elizabeth-heathrow.toml`
already lists `"Heathrow Express"` in its `excluded_keywords` specifically
because Heathrow Express and the Piccadilly line "serve the same
terminals and turn up constantly in Heathrow messages; neither is this
line." Writing an actual `heathrow-express.toml` file resolves that
exclusion from "defensive keyword veto" into "there's a real, correctly-
scoped line for that operator now," and should double check the
existing exclusion doesn't need updating once its own entry exists.
Grand Central, Hull Trains and Lumo share King's Cross-out ECML station
overlap with the not-yet-written LNER entry, following the same
station-overlap-is-fine pattern used elsewhere.

**(d)** Small individually, but four quick, low-risk, low-effort wins with
one genuine follow-up (the Heathrow Express/Elizabeth-line exclusion
check) worth bundling with the LNER/ECML work for the King's Cross three.

### WCML — does the single generic entry need splitting?

**Yes**, on the same logic that already justifies CrossCountry/Northern/
SWR's splits. `west-coast-main-line.toml` currently models only the core
spine (Euston–Watford–Milton Keynes–Rugby–Stafford–Crewe–Preston–
Carlisle) and says so explicitly in its own comment: "Branches (e.g.
Birmingham via Trent Valley) are not included here — they belong in their
own line definitions." Avanti West Coast's actual service group is wider
than the spine: Euston–Birmingham (via Rugby/Coventry, not the modelled
spine), Euston–Liverpool (via Crewe/Runcorn), Euston–Manchester (via
Stoke or via Crewe/Wilmslow), Euston–North Wales/Holyhead (via Crewe/
Chester, overlapping TfW's North Wales Coast Line), plus Caledonian
Sleeper's own two routes (already an operator on this file, no dedicated
segments).

**Estimate: 3–4 more files** (Birmingham branch, Manchester branch,
Liverpool branch, North Wales/Holyhead branch — the last one directly
overlapping TfW's North Wales Coast Line at the station level once that
exists).

This is a good candidate to sequence together with the P0 AW/VT fix,
since both touch the same file and the same underlying "what does Avanti
actually run" research.

---

## Rough total size

Summing every estimate above (excluding the already-partially-covered
Northern regional gaps, counted separately, and excluding the P0 fix,
which touches an existing file rather than adding one):

| Cluster | Estimate |
|---|---|
| GWR | 6–8 |
| LNER | 4–5 |
| Southeastern | 5–6 |
| GTR (Southern/Gatwick Express/Great Northern/Thameslink branches) | 8–10 |
| EMR | 4–5 |
| TPE | 3–4 |
| ScotRail | 8–12 |
| TfW | 6–9 |
| Chiltern | 2–3 |
| c2c | 1–2 |
| Greater Anglia | 6–8 |
| WMR/LNWR | 4–5 |
| Merseyrail | 2 |
| Open access (GC/HT/Lumo/Heathrow Express) | 4 |
| WCML split (Avanti branches) | 3–4 |
| Northern regional gaps (Cumbrian Coast, Calder Valley, Airedale, Wharfedale, Esk Valley, Clitheroe, +) | 5–7 |
| **Total** | **~70–95 new files** |

That range brackets `DESIGN.md`'s own stated target ("production needs
~50–100 lines") almost exactly once added to the 20 that already exist —
a reasonable sanity check that this audit's granularity matches the
project's original intent rather than over- or under-splitting.

**TOCs found with National Rail passenger services that this audit
confirms are genuinely missing (all of them, i.e. none of the brief's
list turned out to already be covered or turned out not to exist):**
GWR, LNER, Southeastern, Southern, Gatwick Express, Great Northern,
Thameslink's branches, EMR, TPE, ScotRail, TfW, Chiltern, c2c, Greater
Anglia, WMR, LNWR, Merseyrail, plus four open-access operators
(Grand Central, Hull Trains, Lumo, Heathrow Express) not named in the
brief but confirmed current and in scope. **That's 17 missing TOC/brand
identities across roughly 13 independent curation efforts** (GTR's four
brands and WMR/LNWR are natural single efforts each), plus Northern's
own incompleteness and the WCML split — so **15 distinct pieces of
follow-up work** in total.

---

## Prioritized list, across everything

Reasoning: fix live bugs before adding coverage; then take network gaps
in order of (passenger volume × total absence) first, breaking ties
toward whichever gap has a concrete, already-confirmed shared-trunk
dependency on existing files (so the segment-naming decision gets made
deliberately, not accidentally, by whoever writes it) — completeness
gaps in an operator this catalogue already partly covers come after
the highest-volume total absences but before the lowest-risk, lowest-
volume quick wins; the geographically enormous but low-collision-risk
gaps (ScotRail, TfW) are scheduled for the sustained middle of the effort
rather than first or last, since they're a lot of work but don't block
or get blocked by anything else.

1. **Fix `west-coast-main-line.toml`'s `AW`→`VT` operator-code bug** (and
   verify the LM/LN secondary flag). This is a live correctness bug in
   shipped data, not a coverage gap — it means Avanti-tagged incidents
   are silently unmatched today. Costs nothing to fix relative to any
   new-coverage work and should happen regardless of what else gets
   prioritized.
2. **Greater Anglia**, specifically because of the confirmed, concrete
   Elizabeth-line-Shenfield segment-overlap question. Doing this early
   forces that decision to be made deliberately while the Elizabeth line
   files are still fresh context, rather than as an afterthought once
   Greater Anglia's own files are half-written.
3. **GWR** — the largest single total-absence gap by passenger volume,
   and the one with the most existing-file station touchpoints (Reading,
   Cardiff, Newport) to coordinate against deliberately.
4. **Southeastern**, paired with **GTR's Thameslink branches** — these
   two have the clearest mutual shared-trunk dependency in the whole
   audit (London Bridge/Lewisham corridor) and `thameslink-core.toml`
   already promises branch files that don't exist yet, so this closes
   out a structurally incomplete existing file, not just adds a new one.
5. **LNER**, paired with the four open-access ECML operators (Grand
   Central, Hull Trains, Lumo) and Heathrow Express (which has its own
   already-live `excluded_keywords` precedent waiting to be resolved) —
   bundling these resolves several ECML-corridor and Heathrow-corridor
   overlap questions in one coordinated pass rather than five separate,
   uncoordinated ones.
6. **WCML split into Avanti's real branch structure**, alongside the P0
   fix from item 1 — same file, same "what does Avanti actually run"
   research, natural to do together.
7. **EMR**, sequenced near GTR's Thameslink-branch work (item 4) given
   the Bedford–St Pancras MML/Thameslink infrastructure overlap.
8. **Northern's real completeness gaps** (Cumbrian Coast especially,
   since it's already flagged in-repo as a known missing definition) —
   this operator already has the most-developed segment/branch
   conventions in the catalogue, so extending it is comparatively
   low-risk, high-familiarity work.
9. **WMR/LNWR**, paired with item 6's WCML work given the shared-operator-
   code entanglement already present in the WCML file today.
10. **TPE** — clean 1:1 fit with the operator's own four-route structure,
    heavy but well-precedented station overlap with Northern/WCML/LNER.
11. **ScotRail** and **TfW** — large, sustained efforts with essentially
    zero collision risk against anything else in the catalogue (ScotRail)
    or manageable, well-precedented overlap (TfW's North Wales Coast
    Line vs. Avanti). Good candidates for dedicated, uninterrupted
    curation passes once the higher-dependency items above have settled
    the segment-naming conventions they'd otherwise have to invent fresh.
12. **Chiltern, c2c, Merseyrail** — the three cleanest, lowest-risk,
    smallest-scope gaps in the whole audit. Good filler/quick-win work
    at any point, including interleaved earlier if a curator wants an
    easy win between larger efforts, but they don't unblock or get
    blocked by anything else, so they're listed last on pure priority
    grounds rather than difficulty.

---

## Appendix: ATOC codes referenced in this document

Cross-checked against railwaycodes.org.uk (fetched 2026-08-29) and, for
the two codes this research found actively confused in existing coverage
(`AW`/`VT`), against a second independent source each (see P0 section).

| Operator | Code |
|---|---|
| Great Western Railway | `GW` |
| LNER | `GR` |
| Southeastern | `SE` |
| Southern | `SN` |
| Gatwick Express | `GX` |
| Great Northern | `GN` |
| Thameslink | `TL` (already in use) |
| East Midlands Railway | `EM` |
| TransPennine Express | `TP` |
| ScotRail | `SR` |
| Transport for Wales | `AW` |
| Chiltern Railways | `CH` |
| c2c | `CC` |
| Greater Anglia | `LE` |
| West Midlands Railway | `LM` |
| London Northwestern Railway | `LM` (per one source — see secondary flag in P0) |
| Merseyrail | `ME` |
| Avanti West Coast | `VT` |
| Caledonian Sleeper | `CS` (already in use) |
| Grand Central | `GC` |
| Hull Trains | `HT` |
| Lumo | `LD` |
| Heathrow Express | `HX` |
| Elizabeth line | `XR` (already in use) |
| Northern | `NT` (already in use) |
| CrossCountry | `XC` (already in use) |
| South Western Railway | `SW` (already in use) |
