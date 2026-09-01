# Other UK Local Rail/Light-Rail Networks — Landscape Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
(the closest structural precedent — a research doc that evaluates
plausibility, cites what it could and couldn't confirm, and reaches a
recommendation without being an implementation plan). This is primarily a
landscape survey, not a settled design; if any candidate below is judged
worth pursuing, the next step is a dedicated design-spec pass for that
network specifically, at the depth
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` gave
TfL/Overground.

## Problem being researched

DESIGN.md scopes this app to two data ecosystems: National Rail's
Knowledgebase/LDBWS feeds (`poller-incidents`, `poller-ldbws`) and TfL's
own Unified API (`poller-tfl`, covering tube, DLR, Overground, Elizabeth
line, and tram). The UK has several other local rail-and-similar transit
networks outside London that are neither — light rail, metro, and tram
systems run by their own regional authorities, each with its own (if any)
open-data story. This document surveys those networks to answer: does any
of them have a real, publicly documented **real-time** open-data API (not
just a static GTFS schedule), and if so, is integrating it worth a
follow-up design-spec pass?

Per the task convention this app already follows (see the trust-schedule
doc's "no invented API details" discipline), every claim below is either
cited to a fetched page or explicitly flagged as unconfirmed. Nothing here
should be read as a confirmed field-level schema for implementation.

## Method and a real limitation to disclose up front

Research was split across five parallel sub-agents (one per network pair,
plus a general catch-all sweep), each using web search and direct page
fetches. **The web-search tool's session-wide budget was exhausted early
by the first sub-agents to run**, before several of the others got to run
a single query — those sub-agents fell back to direct `WebFetch` of
known/guessed URLs (operator sites, data.gov.uk search pages, Wikipedia)
only. Several official sites also bot-blocked or otherwise refused
automated fetches outright: `nexus.org.uk` (HTTP 403),
`travelsouthyorkshire.com` (Radware bot-wall), `spt.co.uk/open-data/`
(404 on a guessed path — the real path, if any, is unconfirmed),
`developer.tfwm.org.uk` (TLS handshake failure), and `web.archive.org`
(fetches blocked in this environment entirely, which also foreclosed
checking historical documentation for APIs that have since gone offline).

This means several "no real-time API found" conclusions below are
**negative results under constrained tooling, not exhaustive audits**.
Each is flagged individually where this matters. A follow-up pass with a
working browser (a human, or an agent with working search) could still
turn up something this pass missed, particularly for Tyne and Wear Metro,
West Midlands Metro, Glasgow Subway, and Sheffield Supertram.

## The two existing integration patterns, and a third precedent worth citing

Per DESIGN.md and the `poller-tfl` module doc:

- **Pattern A — `poller-tfl` shape.** The upstream API already computes
  and publishes aggregate line status (TfL's `statusSeverity` per line).
  The poller just polls, maps severity codes, and forwards — "nothing
  downstream has to infer it from incidents or departure boards, and the
  aggregator is not involved" (`crates/poller-tfl/src/main.rs:1-9`).
  Cheapest integration shape by far.
- **Pattern B — LDBWS-sampling shape.** The upstream source only gives raw
  per-service data (departure boards, individual predictions) with no
  aggregate status concept. This app has to sample, threshold, and
  classify itself (`infer_from_samples`, DESIGN.md §6.2). Meaningfully
  more expensive: it means designing and tuning a severity-inference
  pipeline, not just a schema mapping.
- **A third, cautionary precedent from inside this app's own TfL
  integration**: DLR. Even though DLR is served by TfL's own Unified API
  (Pattern A in principle), its live feed is arrivals-predictions-only
  with no aggregate status — so `crates/poller-tfl/src/dlr/` had to build
  its own inference pilot, diffing live Arrivals predictions against a
  published Timetable
  (`crates/poller-tfl/src/dlr/mod.rs`: *"Unlike the rest of `poller-tfl`,
  which only relays status TfL has already computed, this module infers
  `common::SampleStats` itself... No other TfL line does this."*). The
  lesson for this research: "has an API from a single sensible-looking
  operator" does not by itself mean Pattern A cost. The API's actual
  return shape — aggregate status vs. raw per-service data — is what
  determines the pattern, and has to be checked per network, not assumed
  from the existence of an API alone.

## Findings by network

### Manchester Metrolink (TfGM)

**Had a real, working real-time API. Currently closed to new
integrators.** TfGM (Transport for Greater Manchester) operated an Open
Data Portal issuing API keys for real-time Metrolink data
(`developer.tfgm.com` redirects to
[tfgm.com/data-analytics-and-insight/open-data-portal](https://tfgm.com/data-analytics-and-insight/open-data-portal),
fetched directly and independently confirmed by both a research sub-agent
and this document's author). The page states, verbatim, as of this
research:

> "Our Open Data Portal providing real time data feeds is no longer in
> operation."
>
> "Current API keys for real time Metrolink data will continue to
> function, however the creation of new subscriptions or new keys is not
> possible."
>
> "We are exploring options for a new solution and when we have more
> details we will provide updates on this page and via email to
> registered API subscribers."

- **What it returned**: unconfirmed at field level. `web.archive.org` was
  unreachable in this environment, so the historical API docs (endpoint
  list, whether it exposed aggregate line status or only per-stop live
  departures) could not be recovered. This is a genuine open question.
- **Access/licensing**: was API-key/subscription-based via the portal; no
  pricing or rate-limit figures found. **New signups are not currently
  possible**, full stop — this alone rules Metrolink out for any near-term
  integration regardless of how the rest of the evaluation would have come
  out.
- **Network size**: 8 lines (general knowledge, not independently
  re-verified this pass).
- **Integration pattern**: cannot be characterized (Pattern A vs B)
  without the field-level schema, which is unrecoverable right now.

### Tyne and Wear Metro (Nexus)

**No real-time API found — but this is a weak negative result.**
`nexus.org.uk` returned HTTP 403 to every direct fetch (bot-blocked, not a
content signal). A possible rebrand domain, `travelnortheast.uk`, also
403'd. `data.gov.uk` search for Nexus/Metro datasets returned nothing
relevant (only unrelated water-body, heritage, and Translink-NI results).
Wikipedia's [Tyne and Wear Metro
article](https://en.wikipedia.org/wiki/Tyne_and_Wear_Metro) makes no
mention of an API, open-data feed, or developer program — but Wikipedia is
not authoritative for this kind of operational detail either way.

- **Open question, not a confirmed absence**: whether Nexus has any
  real-time feed (its own, or through Bus Open Data Service, or a
  Rail-Data-Marketplace-adjacent product — unlikely since Metro isn't a
  National Rail TOC, but unconfirmed) could not be resolved by this
  research pass; every path tried was bot-blocked or produced no results.
- **Network size**: 2 lines (Yellow/Green), 60 stations, 77.5 km — small.

### West Midlands Metro (TfWM / Midland Metro Limited)

**Ambiguous — a live API host exists, but its scope and public
availability could not be confirmed.** `api.tfwm.org.uk` resolves and
presents a valid TLS certificate issued for `*.itoworld.com` (confirming
it's a live, ITO-World-hosted service — ITO World holds TfWM's data
contract), but its root path returns HTTP 403 with no discoverable public
documentation page. `developer.tfwm.org.uk` failed with a TLS handshake
error on fetch (could not even establish a connection, let alone read
content). `disruptions.tfwm.org.uk` is a live, human-facing disruptions
portal ("Find live and planned disruptions for public transport in the
West Midlands") with no linked machine-readable feed found.
`data.gov.uk` search for West Midlands Metro returned no relevant
datasets. Wikipedia's [West Midlands Metro
article](https://en.wikipedia.org/wiki/West_Midlands_Metro) makes no
mention of an open API.

- **Open question, not a confirmed absence**: `api.tfwm.org.uk` may be a
  real, documented, registration-gated API that simply doesn't surface
  its docs to an anonymous automated fetch — or it may be bus/roadworks/
  car-park data only (ITO World's historical TfWM scope), with no Metro
  tram coverage at all. Genuinely unresolved by this pass.
- **Network size**: 2 lines — Line 1 (operational, Wolverhampton–
  Edgbaston/Millennium Point) and Line 2 (under construction, due late
  2026, Wednesbury–Dudley). One of the smallest networks surveyed, and
  will only be 2 lines even once fully built.

### Sheffield Supertram

**No real-time API found.** `supertram.com` redirects to
`travelsouthyorkshire.com`, which sits behind a Radware bot-wall
(`perfdrive.com` challenge) that blocked all automated access in this
pass. The historical `sypte.co.uk` domain does not resolve — South
Yorkshire's transport authority appears to have rebranded to "Transport
for South Yorkshire" (per `data.gov.uk`'s search UI), and a guessed
`transportforsouthyorkshire.gov.uk` domain also failed to resolve
(`getaddrinfo ENOTFOUND`), so the correct current domain for that
authority's open-data presence, if any, was not found. `data.gov.uk`
search for "supertram" returned zero results. Wikipedia's [Sheffield
Supertram article](https://en.wikipedia.org/wiki/Sheffield_Supertram)
makes no mention of real-time tracking or an open API.

- **This is a weaker negative than most on this list** — the operator's
  entire current web presence was inaccessible to automated fetch in this
  pass, not merely undocumented.
- **Network size**: 4 lines (Yellow, Blue, Purple, Tram-Train) — the
  largest of the light-rail-only networks surveyed, still tiny next to
  National Rail or TfL.

### Glasgow Subway (SPT — Strathclyde Partnership for Transport)

**No real-time API found.** `spt.co.uk`'s Subway page was reachable, but
no open-data or developer section was found in its navigation, and a
guessed `/open-data/` path 404'd. `data.gov.uk` search for "glasgow
subway" returned no relevant datasets. Wikipedia's [Glasgow Subway
article](https://en.wikipedia.org/wiki/Glasgow_Subway) mentions only a
consumer-facing app (iShoogle), not a public API.

- **Open question**: the real open-data path on `spt.co.uk`, if one
  exists, was not found by guessing — this pass did not get a working
  general web search to locate it properly.
- **Network size**: a single circular line — the smallest network
  surveyed, and one of the smallest metro systems in the world. Even a
  best-case API here buys coverage for one line.

### Edinburgh Trams

**No real-time API found.** `edinburghtrams.com` has a "Live Tram
Departures" feature, but it reads as an embedded website widget (JS-driven
consumer display), not a documented public API or developer feed — no
developer/open-data links were found in the site's navigation.
`data.gov.uk` search for "edinburgh trams" returned no relevant datasets.

- **Open question**: whether the live-departures widget is backed by any
  API that's separately documented for third-party use was not resolved.
- **Network size**: a single line (Edinburgh Airport–York Place). Smallest
  network surveyed alongside Glasgow Subway.

### Nottingham Express Transit (NET)

**No real-time API found.** `thetram.net` (NET's own site) references only
a "NETGO!" consumer app for service updates — no developer, API, or
open-data links found. `data.gov.uk` search returned only static
infrastructure/mapping datasets; the DfT's own generic "Tram/Light Rail
Networks" dataset entry notes explicitly that "this data is held by the
transport operators" — i.e., DfT does not centrally hold or republish it
either.

- **Network size**: 2 lines (approximate — not independently re-verified
  this pass).

### Translink / Northern Ireland Railways — the genuinely different case

As flagged at the outset of this research: NI Railways sits outside Great
Britain's National Rail structure entirely, which makes it a different
kind of candidate from the seven networks above (all of which at least
share GB's rail data ecosystem, even though none of the ones checked
turned out to plug into it for light rail).

**NIR's separateness from GB National Rail is circumstantially confirmed,
not directly verified.** Direct fetch of the Open Rail Data Wiki's TOC
Codes page (the page DESIGN.md itself cites,
`wiki.openraildata.com/index.php/TOC_Codes`) returned HTTP 403 in this
pass, so NIR's absence from that specific list could not be checked
directly. However, [OpenDataNI](https://www.opendatani.gov.uk) publishes
NIR as its own separate dataset family ("Northern Ireland Railways NIR
Railway Network," "Northern Ireland Railways Stations" — distinct
infrastructure datasets, not folded into any GB rail data source), which
is consistent with, but does not prove, NIR sitting entirely outside
ATOC/RDG's Darwin/Knowledgebase/Rail-Data-Marketplace ecosystem.

**Two relevant real-time APIs were found, both currently accessible**:

1. **"Translink Northern Ireland Railway Real Time Passenger
   Information"** — documented (per its `data.gov.uk` dataset
   description; the docs page itself, `apis.opendatani.gov.uk/translink/`,
   returned HTTP 503 on two fetch attempts and its content is therefore
   only known secondhand through that description, not independently
   read) to provide "near real time information about arrivals and
   departures for the Translink rail stations," in JSON (station codes)
   or XML (station responses), with a 2-minute server-side cache
   (`X-Cached` header mentioned). This is **arrivals/departures per
   station** — Pattern B shaped (LDBWS-like: raw per-service data,
   this app would need to build its own severity inference), not a
   ready-made line-status field.
2. **"Translink Transport Information API"** — documented at
   [translink.co.uk/api](https://www.translink.co.uk/api) (fetched
   directly, independently confirmed twice — once by a research sub-agent,
   once by this document's author). Covers four data types: journey
   plans, departure boards, bus stop data, and **incident information**,
   all JSON over a RESTful interface. Access requires emailing
   `servicedata@translink.co.uk` with name, company (if any), and contact
   email to receive a key used in a request header, subject to a "fair
   usage policy" with no published rate-limit numbers. The page does not
   state whether coverage spans NI Railways specifically, or is scoped
   only to bus/Glider — **unconfirmed, and this matters**: "incident
   information" is the closest thing found across this whole survey to a
   ready-made disruption feed (potentially Pattern-A-shaped), but its
   actual schema and rail-vs-bus scope are gated behind key registration
   and were not retrievable in this pass.
   Licensing across OpenDataNI's rail dataset is stated as Open
   Government Licence v3.0 (per `data.gov.uk`); the broader Transport
   API's licensing framework is referenced but not spelled out on the
   public page.

**Reference-data implication (the point that makes NIR structurally
different from the other candidates).** This app's entire domain model —
`lines/*.toml`, `common::Station`'s CRS/TIPLOC fields, ATOC operator
codes (DESIGN.md §5.1, §13) — is GB-National-Rail-shaped, and none of it
covers NIR. Unlike the seven GB light-rail networks above (which, even
where a status API doesn't exist, at least sit in a media/geography this
app's curatorial conventions already understand), integrating NIR would
mean **hand-curating an entire new line/station catalogue from scratch**
— comparable in curatorial cost to onboarding a brand-new NR TOC, minus
the benefit of NR's existing Knowledgebase/CIF tooling. OpenDataNI's own
"NI Railways Stations" dataset could plausibly seed station reference
data, which is more than any of the seven GB networks above offered, but
this is unverified as a concrete data source in this pass.

- **Integration pattern**: ambiguous. The RTPI API is confirmed
  Pattern-B-shaped (arrivals data, needs inference). The Transport API's
  "incident information" might be Pattern-A-shaped but its schema is
  unconfirmed. A follow-up pass would need registered API access to
  settle this.
- **Network size**: ~4 lines out of Belfast — Bangor, Larne, Portadown/
  Dublin (Enterprise), and Londonderry, per OpenDataNI's own network
  dataset description ("~220 route miles").

## Confirmed out of scope

- **Croydon Tramlink.** Confirmed via TfL's own
  [`api.tfl.gov.uk/Line/Meta/Modes`](https://api.tfl.gov.uk/Line/Meta/Modes)
  endpoint, which lists "tram" alongside tube/DLR/Overground/Elizabeth
  line as one of TfL's own modes. It is already reachable through this
  app's existing `poller-tfl` integration. Not a new-network candidate, as
  the task assumed but asked to be verified rather than assumed — verified.
- **Merseyrail.** Per [Wikipedia](https://en.wikipedia.org/wiki/Merseyrail):
  Merseyrail's Northern/Wirral lines operate as a concession (a Serco/
  Transport UK Group joint venture under Merseytravel) rather than a
  standard National Rail franchise, but it retains a TOC code and National
  Rail Enquiries listing ("Merseyrail Electrics"). This reads as already
  inside the Darwin/Knowledgebase/LDBWS ecosystem this app already
  ingests, despite its unusual concession structure — **reasonably
  confirmed via a secondary source, not independently verified against
  Darwin/Rail-Data-Marketplace documentation directly.** Not a new-network
  candidate under this research's own framing (it doesn't need a new
  `poller-<network>`-shaped integration; it's covered, or should already
  be coverable, by the existing NR pollers).

## Other paths checked and ruled out

- **Blackpool Tramway.** [Blackpool Transport's open-data
  page](https://www.blackpooltransport.com/open-data/) publishes only
  TransXChange and GTFS static schedule data ("stops, lines, journeys, and
  schedules"). No live position, arrivals, or status feed mentioned. Not a
  viable candidate under this project's real-time-API requirement — a
  clean example of the "static-GTFS-only, not what this app needs"
  category the task asked to watch for.
- **Bus Open Data Service (BODS).** Per
  [Wikipedia](https://en.wikipedia.org/wiki/Bus_Open_Data_Service): covers
  bus services only, England-only. Publishes real-time formats (SIRI-VM,
  GTFS-RT) but none of it extends to tram/light-rail operators. Confirmed
  not a route into any network on this list.
- **No unified UK-wide light-rail status aggregator found.**
  [TransportAPI.com](https://www.transportapi.com/) is the closest
  candidate (a commercial managed transport-data platform advertising
  "TAPI Rail Information," "TAPI Rail Performance," etc.), but its own
  site makes **no mention of light rail or tram coverage** in any product
  line, and its pricing is not disclosed on the public page (implying a
  paid/tiered developer-portal model — unconfirmed). **Open question**:
  no evidence was found of a single API that aggregates status across
  Tyne & Wear Metro, Metrolink, West Midlands Metro, etc. If this app
  pursues multiple light-rail networks, the evidence gathered here points
  to N separate integrations, not one aggregator.

## Cost/benefit framing

Every network surveyed here is geographically tiny and locally scoped
compared to this app's two existing sources: National Rail (currently
~20 curated lines, DESIGN.md §10 targets 50-100) and TfL (a whole city's
multi-modal network). The largest network found is Sheffield Supertram at
4 lines; the smallest, Glasgow Subway and Edinburgh Trams, are 1 line
each. Even a best-case, cheapest-possible integration (Pattern A, TfL-
shaped, no inference needed) buys a handful of lines serving one city —
materially less user value per unit of engineering effort than either
existing source.

The DLR precedent (see above) is the sharper warning: even where an API
exists from a sensible-looking operator, it may not be Pattern-A cheap —
it may require its own bespoke inference pipeline (Pattern B, or a novel
third shape like DLR's own arrivals-vs-timetable diffing), at which point
the cost approaches a scaled-down version of the LDBWS-sampling
investment this app already made for something twenty-plus times larger
in scope.

Combined with the fact that only two of the eight networks surveyed
(Manchester Metrolink, historically; Translink/NI Railways, currently)
turned up any confirmed real-time API at all — and both come with
material caveats (Metrolink's is currently closed to new integrators;
Translink's clean disruption-shaped data is schema-unconfirmed and its
network is reference-data-disconnected from the rest of this app) — the
cost/benefit case for any of these eight networks is weak relative to
continuing to invest in National Rail line-catalogue coverage or TfL
feature depth, both already-proven high-leverage areas per DESIGN.md's own
build-sequence (§8).

## Recommendation

**Nothing surveyed here clears the bar for a genuine follow-up
design-spec pass right now.** This is an honest "none of these are
compelling" conclusion, not a failure of the research — it's what the
evidence actually supports.

Ranked:

1. **Translink / NI Railways — possible, not recommended now.** The only
   network with a currently-accessible, real-time, rail-specific API (in
   fact two). Disqualified from "worth it now" by: (a) total reference-
   data disconnection from this app's GB-only curatorial conventions —
   integrating it costs as much as onboarding a whole new NR TOC's line
   catalogue, without NR's existing tooling to lean on; (b) unresolved
   ambiguity over whether either API is Pattern-A (ready status) or
   Pattern-B (needs inference) shaped, which is only resolvable behind
   registration this research didn't pursue; (c) a small network (~4
   lines) serving a region this app has never targeted. Worth revisiting
   specifically if/when this app decides Northern Ireland coverage is a
   goal in its own right — not as an incremental "add one more light-rail
   network" line item.
2. **Manchester Metrolink — watch, don't build.** Real, working
   infrastructure existed (a genuine TfL-adjacent-quality open-data
   portal) but new API access is explicitly closed as of this research,
   per TfGM's own page. Nothing to design against today. If TfGM's
   promised "new solution" ships with public registration, a fresh check
   (starting from the same URL above) would take a few minutes and might
   change this.
3. **Tyne and Wear Metro, West Midlands Metro — inconclusive, low
   priority for a research re-check, not for design work.** Both hit hard
   tooling blockers (bot-walls, TLS failures) rather than confirmed
   absence of an API. A human with an ordinary browser could likely
   resolve both in well under an hour. Worth doing only opportunistically
   — neither is large enough (2 lines each) to justify dedicating
   research time on its own merits.
4. **Sheffield Supertram, Glasgow Subway, Edinburgh Trams, Nottingham
   Express Transit — not recommended, in any respect.** No evidence of
   any real-time API surfaced in this pass (Supertram's negative result is
   weaker than the others', due to a bot-walled operator site, but even a
   confirmed API here would serve one of the smallest networks surveyed).
   Not worth further research time under current priorities.
5. **Croydon Tramlink, Merseyrail — no action needed.** Already covered by
   existing integrations (TfL and National-Rail-adjacent respectively);
   confirmed out of scope for "new network" work, not omissions.

If priorities change and this area becomes worth revisiting, the cheapest
next step is not a design spec — it's closing the specific open questions
flagged above (Nexus, TfWM, SPT's real open-data paths; Translink's actual
API schema behind a registered key) with working search/browser access,
since several "no API found" results here are artifacts of this session's
tooling constraints, not settled facts.

## Open questions (explicit, not resolved here)

1. **Nexus/Tyne and Wear Metro's real open-data presence, if any** — every
   path this research tried was bot-blocked (`nexus.org.uk` 403) or
   produced no results (`data.gov.uk` search).
2. **`api.tfwm.org.uk`'s actual scope and public-availability** — a live,
   properly-certificated host exists, but whether it documents (or has any
   plan to document publicly) West Midlands Metro tram data specifically,
   versus bus/roadworks/car-park data only, is unresolved.
3. **SPT's real open-data path for Glasgow Subway, if one exists** — a
   guessed URL 404'd; this doesn't rule out a correctly-linked page
   elsewhere on `spt.co.uk`.
4. **Transport for South Yorkshire's current domain and any open-data
   presence for Sheffield Supertram** — `sypte.co.uk` no longer resolves
   and the plausible replacement domain guessed in this pass also failed
   to resolve; the authority's actual current web presence was not found.
5. **Manchester Metrolink's historical API schema** — unrecoverable in
   this pass because `web.archive.org` fetches are blocked in this
   environment; someone with working Wayback Machine access could likely
   settle whether it was Pattern A or B shaped, which would matter
   immediately if TfGM reopens registration.
6. **Translink Transport Information API's actual field-level schema and
   NIR-vs-bus/Glider coverage scope** — gated behind emailing
   `servicedata@translink.co.uk` for a key; not obtained in this pass.
7. **Whether NI Railways is genuinely and completely absent from RDM/
   Darwin/Knowledgebase coverage** — inferred from OpenDataNI's separate
   dataset family, not directly confirmed against the Open Rail Data
   Wiki's TOC Codes list (that page 403'd on fetch in this pass).

## References

- TfL Unified API modes list (Croydon Tramlink confirmation):
  https://api.tfl.gov.uk/Line/Meta/Modes
- `crates/poller-tfl/src/main.rs` (Pattern A module doc) and
  `crates/poller-tfl/src/dlr/mod.rs` (DLR inference-pilot precedent)
- TfGM Open Data Portal (Metrolink, confirmed closed to new signups):
  https://tfgm.com/data-analytics-and-insight/open-data-portal
- Translink Transport Information API docs:
  https://www.translink.co.uk/api
- Translink/OpenDataNI real-time rail dataset (description only; docs page
  itself 503'd): found via `data.gov.uk` search
- Open Rail Data Wiki TOC Codes (DESIGN.md's own citation; 403'd on direct
  fetch this pass): https://wiki.openraildata.com/index.php/TOC_Codes
- Blackpool Transport open data (GTFS-static only):
  https://www.blackpooltransport.com/open-data/
- Bus Open Data Service overview:
  https://en.wikipedia.org/wiki/Bus_Open_Data_Service
- Merseyrail concession structure:
  https://en.wikipedia.org/wiki/Merseyrail
- TransportAPI.com (no confirmed light-rail coverage):
  https://www.transportapi.com/
- Wikipedia articles for network size/scale (Tyne and Wear Metro, West
  Midlands Metro, Sheffield Supertram, Glasgow Subway) — used only for
  uncontested scale facts (line counts, station counts), not for API
  claims, per this document's own citation discipline.
