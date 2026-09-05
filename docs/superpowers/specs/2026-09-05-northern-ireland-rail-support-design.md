# Northern Ireland Rail Support — Design Spec

**Status: design spec, not an approved plan.** Written at the request of
the repo owner ("research and spec out adding northern ireland support"),
building directly on
`docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
("the survey doc") — specifically its "Translink / Northern Ireland
Railways" section and its own recommendation ("possible, not recommended
now... worth revisiting specifically if/when this app decides Northern
Ireland coverage is a goal in its own right"). That condition is what
triggered this doc: the owner is now asking for the goal-in-its-own-right
version, not another "should we bother" survey. This document does the
next step the survey doc deferred: closes its one flagged open question
(NIR's exclusion from GB's TOC-code system — survey doc's open question
7), confirms what Translink/NIR actually publishes, and designs concretely
how a real integration would fit this app's architecture.

Per this repo's own citation discipline (survey doc's "no invented API
details," carried from
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`):
every claim below is cited to either this repo's code (`file:line`) or a
source actually fetched/searched this session, with an explicit note
where something remains unconfirmed. Nothing here should be read as a
committed schema — nothing was implemented, and no field-level API
response was directly read (see §1's access gate).

---

## 1. What Translink/NIR actually publishes

### 1.1 Closing the survey doc's open question: NIR is confirmed outside GB's TOC-code system

The survey doc flagged this as "circumstantially confirmed, not directly
verified" because a direct fetch of the Open Rail Data Wiki's TOC Codes
page (`wiki.openraildata.com/index.php/TOC_Codes`) 403'd. **It still
403's** (re-attempted this session, same result). However, Wikipedia's
["List of companies operating trains in the United
Kingdom"](https://en.wikipedia.org/wiki/List_of_companies_operating_trains_in_the_United_Kingdom)
article — fetched directly this session — states explicitly that TOC
codes apply "except in Northern Ireland," and lists Northern Ireland
Railways in a separate "Passenger operators in Northern Ireland" section,
outside its main GB operator/TOC-code table and outside its franchised-
operator structure table entirely. This is a direct textual statement
about the TOC-code system's scope from a source that could be fetched,
not an inference from a separate dataset family (which is all the survey
doc had). **Treat this as resolved**: NIR is confirmed to sit outside
ATOC/TOC codes, and by extension outside the Darwin/Knowledgebase/RDM
ecosystem this app's five existing pollers ingest, which are all built on
that code system (`reference-data/stanox-crs.csv`'s 3,125 rows are, per
the background this task was given, generated purely from GB CIF SCHEDULE
data — zero NI entries).

### 1.2 Real-time data: a genuine, currently-live API exists — but it's raw arrivals data, and its full schema is still access-gated

**"Translink Northern Ireland Railway Real Time Passenger Information"**
(`opendatani.gov.uk/dataset/real-time-rail-stations-arrivals-and-departures`,
fetched directly this session): a live, OGL-licensed API providing
near-real-time arrivals/departures per station, in **JSON, XML, or HTML**
formats, hosted at `apis.opendatani.gov.uk/translink/`, with a stated
2-minute server-side response cache (`X-Cached: HIT`/`MISS`/`EXPIRED`
header) and a documented fallback path if OpenDataNI's mirror is down
(`https://tiger.worldline.global/toc/NIR`, run by Worldline — the same
vendor that operates this feed for OpenDataNI). The dataset page lists
four resources: a station-and-formats index (HTML), a **station codes
reference (JSON)**, an example XML response, and an example HTML
rendering — i.e., this feed publishes its own internal station-code list
as a first-class resource, not something to reverse-engineer.

**What could not be confirmed this session**: the actual field-level
schema of an arrivals response. Both direct endpoints were unreachable on
fetch — `apis.opendatani.gov.uk/translink/index.html` returned HTTP 503,
then HTTP 500 on a second attempt this session (matching the survey doc's
own 503 from its pass); `tiger.worldline.global/toc/NIR` returned only a
client-rendered "Loading..." shell with no data in the fetched HTML. This
is the same class of failure the survey doc hit, not a new problem, but it
means **the exact JSON/XML field names for an arrivals/departures record
are still unread**, even though the dataset's existence, licensing, and
format list are now independently confirmed by two fetches. What is
firmly known: it is described as **arrivals/departures data**, i.e.
per-service predictions at a station — not an aggregate line-status
field. This matches the survey doc's Pattern-B characterization
(DESIGN.md's own vocabulary via the survey doc: "raw per-service data,
this app would need to build its own severity inference," the same shape
as OpenLDBWS).

### 1.3 A second, broader API exists — access-gated, scope still unconfirmed for rail

**"Translink Transport Information API"**
(`translink.co.uk/api`, fetched directly this session, matching the
survey doc's own finding): covers four data types — journey plans,
departure boards, bus stops, and **incident information** — RESTful,
JSON, with data described as updating on a weekly cycle for "core
information." Access requires emailing `servicedata@translink.co.uk` with
name/company/contact to receive a key, accepting a "fair usage policy"
whose terms aren't published on the page, with further technical
documentation stated to arrive only after registration ("more information
about the API can be found in the guidance document for developers which
will be provided on sign up"). **This session did not register for a
key** (out of scope for a research pass — see §5's recommendation on this
point) — so, as in the survey doc, whether "incident information" covers
NI Railways specifically, or is scoped only to bus/Glider, remains
unconfirmed. This is the single most consequential unresolved fact in
this whole area: if it covers rail and returns anything shaped like a
pre-computed disruption/status feed, it would be the one real path to a
TfL-shaped (Pattern A) integration for NIR. Nothing found this session
moves this beyond "worth checking by actually registering," which the
survey doc also concluded.

### 1.4 Static reference data: real, and directly useful, independent of the two APIs above

Two more OpenDataNI datasets, found this session and not covered by the
survey doc's earlier pass:

- **"Northern Ireland Railways Stations"**
  (`admin.opendatani.gov.uk/dataset/northern-ireland-railways-stations`,
  found via search; the dataset page itself 403'd on direct fetch, so its
  exact column schema is unconfirmed, but its resource list is not — CSV
  and GeoJSON formats, one file each, described as covering "all Railway
  Stations along the Translink Rail Network," geo-referenced in Irish
  Grid). This is a genuine, standalone station catalogue with
  coordinates — the single most directly reusable artifact for a static
  reference-data tier (§4 Tier A), independent of either live API's
  uptime or access gate.
- **"Northern Ireland Railways NIR Railway Network"** (companion
  dataset, SHP + GeoJSON): route-line geometry, described as ~220 route
  miles across named lines (Dublin/Enterprise, Bangor, Larne, and
  others).

**A general-knowledge cross-check, not independently verified against a
primary Translink source**: web search this session surfaced a station-
name list (Adelaide, Antrim, Ballycarry, Ballymena, Bangor, Belfast
Central, Coleraine, Derry~Londonderry, Great Victoria Street, Larne
Harbour, Lisburn, Newry, Portadown, Portrush, University, Yorkgate, and
roughly 40 more) and a route structure of five to six named
lines/branches (Bangor Line; Larne Line to Larne Harbour/Whitehead;
Derry~Londonderry Line with a Portrush branch off Coleraine; the
Belfast–Dublin Enterprise line via Portadown/Newry, jointly run with
Iarnród Éireann; plus the new Belfast Grand Central hub opened in 2024,
replacing Lanyon Place as the Enterprise terminus). **This is
corroborating detail from secondary sources (a fan wiki, Wikipedia
station/line articles), not a verified station count** — treat the ~50
station / 5-6 line figures as approximate scale, not a citable dataset
schema. The survey doc's "~4 lines" estimate looks like an undercount by
this cross-check; the real figure is closer to 5-6 lines depending on how
branches are split, which matters for §4's scope estimate.

### 1.5 No GTFS or GTFS-RT feed found

Checked explicitly per this task's instruction, since GTFS is common
elsewhere. Web search this session found no evidence of a native
Translink NIR GTFS or GTFS-RT feed. Translink's own open-data formats are
described (per search-result summaries of Translink's open-data
commentary) as historically TransXChange/CIF-shaped for schedule data,
not GTFS, with the OpenDataNI rail-specific products (§1.2-1.4) being
JSON/XML/CSV/GeoJSON, not GTFS. (Note: "Translink" GTFS/GTFS-RT feeds
that do appear on aggregators like Transitland belong to unrelated
same-named agencies — TransLink of Vancouver and TransLink
Brisbane/Queensland — not Translink NI. This was checked and ruled out
explicitly, not assumed.) **Bottom line: no static-GTFS-republish
pattern (the `schedule-ingest` SFTP-delivery shape) applies here** — there
is no periodic bulk schedule file to ingest. Whatever timetable/schedule
data NIR needs would come from the CSV/GeoJSON reference datasets (§1.4,
static, low-cadence, not a "feed" in the polling sense) or from
hand-curation, not from a GTFS pipeline.

### Summary of what's actually confirmed vs. open

| Question | Status |
|---|---|
| Is NIR outside GB's TOC-code/Darwin/RDM ecosystem? | **Confirmed** this session (Wikipedia's explicit "except in Northern Ireland" statement), closing the survey doc's open question 7. |
| Does a real-time NIR arrivals/departures API exist and is it currently live? | **Confirmed to exist and be documented** (dataset page, formats, licensing, station-code resource). **Field-level schema unconfirmed** — every attempt to fetch actual response content 500'd/503'd/rendered client-side-only, both this session and the survey doc's. |
| Is there a pre-computed disruption/incident feed that could be Pattern-A cheap? | **Unresolved.** The Transport API's "incident information" type is the only candidate found, and its NIR-rail-vs-bus scope requires registering for a key (`servicedata@translink.co.uk`) that this research pass did not pursue. |
| Is there usable static station/line reference data? | **Confirmed** — CSV/GeoJSON station dataset, SHP/GeoJSON network-line dataset, both on OpenDataNI, independent of either API's uptime. |
| Is there a GTFS/GTFS-RT feed? | **Checked, not found.** Ruled out, not merely unsearched. |

---

## 2. Data model gap analysis

Grounded directly in `crates/common/src/lib.rs`:

- **`common::Station`** (`crates/common/src/lib.rs:443-451`):
  ```rust
  pub struct Station {
      pub crs: String,               // required, not Option
      pub tiploc: Option<String>,
      pub role: String,               // default "minor"
      pub segment: Option<String>,
  }
  ```
  `crs` is a **required, non-optional `String`** — every station in this
  app's domain model is assumed to have a CRS code. NIR stations have no
  CRS codes at all (they're not in the National Rail station-code
  registry; `reference-data/stanox-crs.csv`, generated purely from GB CIF
  data, has zero NI rows, per this task's background). There is no
  natural value to put in this field for an NIR station — not "make it
  optional and often absent," but "the concept the field encodes doesn't
  exist for this network."

- **`common::LineDefinition`** (`crates/common/src/lib.rs:461-500`): `mode`
  is a plain `String` with exactly one value used anywhere in this
  codebase's `lines/*.toml` catalogue today: `"national-rail"`
  (`crates/common/src/lib.rs:1070`, `:1418`, and confirmed by grepping
  every `lines/*.toml` file — only `national-rail` appears). TfL's five
  modes (tube, dlr, overground, elizabeth-line, tram —
  `crates/common/src/lib.rs:1442-1443`) are **not** `LineDefinition.mode`
  values at all; they never pass through `lines/*.toml` or
  `LineDefinition`. `operators` is a `Vec<String>` of ATOC codes
  (DESIGN.md §5.1: "A list of `operators` (ATOC codes)") — NIR has no
  ATOC code (§1.1).

- **The TfL precedent is the load-bearing example for how this app has
  already solved almost exactly this problem once.** TfL line status
  does **not** flow through `LineDefinition`/`lines/*.toml` at all. Per
  the doc comment directly on `crates/api/src/data/queries.rs:466-476`:

  > "TfL lines, derived from the rows `crates/poller-tfl` wrote rather
  > than from a hand-curated `lines/*.toml` entry. A TOML entry would be
  > wrong three ways: the aggregator loads that directory and would
  > overwrite each ingested TfL status with a Good-Service fallback on
  > its next cycle; a `LineDefinition` is mostly route topology (ordered
  > CRS stations, segments, sample stations, keywords, thresholds) that a
  > finished-status feed has no use for; and it would drift out of date."

  Instead, TfL lines are their own row shape (`TflLineSummaryRow { id,
  name, mode_name }`, `crates/api/src/data/queries.rs:460-464`), written
  directly into `line_status` with `source = 'tfl'`
  (`crates/api/src/data/queries.rs:480`), and merged into the public API
  response shape by `to_tfl_shape`/`to_tfl_shape_with_overlay`
  (`crates/api/src/render.rs:14-44`) — a shape keyed on `id`/`name`/
  `modeName`/`operators`/`lineStatuses`, with **no CRS/TIPLOC/segment
  fields anywhere in it**. The frontend already renders this shape for
  five non-national-rail modes today.

**Conclusion: this is a structurally separate catalogue/pipeline, not a
bolt-on to `common::Station`/`LineDefinition`.** Two reasons, both
concrete:

1. `Station.crs` being a required field, not `Option<String>`, means
   every existing consumer of `LineDefinition.stations` (`aggregator`'s
   segment matcher, LDBWS-inference destination/station filtering, the
   `/stations/[crs]` frontend route — `frontend/app/stations/[crs]/page.tsx`,
   confirmed keyed on CRS by its test file path and by
   `frontend/app/stations/StationSearchForm.tsx:11`'s `const [crs, ...]
   = useState('')`) is written assuming a CRS code exists and is the
   primary key. Making it optional doesn't remove this — every call site
   would still need new branching logic for "this station has no CRS,"
   which is the same cost as a separate type with none of the type-safety
   benefit.
2. NIR's likely real data shape (§1.2: raw per-station arrivals, Pattern
   B) doesn't carry route topology, ATOC operators, or segments either —
   the same "a finished-status feed / raw-feed has no use for
   `LineDefinition`'s fields" argument the TfL doc comment already makes
   applies here for a different reason (TfL: the feed is *already an
   aggregate*, so topology fields are redundant; NIR: the *feed's own
   station-code scheme* doesn't map onto CRS at all, so topology fields
   are actively wrong wherever they'd need a CRS).

**What a parallel NIR catalogue would look like, concretely**: an
`common::NirStation { id: String /* Translink's own station code or a
stable slug */, name: String, latitude/longitude: Option<f64> }` and an
`common::NirLineDefinition` with an `id`/`name`/an ordered station list
using that same `id`, no `operators` (or a single hardcoded "NIR"
value), no `segment`/ATOC/CRS fields at all — deliberately not reusing
`Station`/`LineDefinition`, mirroring how `TflLineSummaryRow` deliberately
doesn't reuse `LineDefinition` either. `LineDefinition.mode` would gain a
value like `"ni-railways"` for the record it does surface through, the
same way `"tfl-tube"`/`"tfl-dlr"`/etc. act as the `modeName` values in the
TfL-shaped JSON without ever appearing as a `lines/*.toml` `mode` field.

---

## 3. Reusable infrastructure vs. genuinely new work

**Frontend line-status rendering: largely reusable, if the backend
produces the same JSON shape.** The frontend already renders `national-
rail` and five TfL `modeName` values through one shape
(`to_tfl_shape`'s `$type: "DistantSignal.LineStatusReport"` JSON —
`crates/api/src/render.rs:14-24` — consumed generically per
`frontend/lib/types.ts`'s `modeName`-keyed handling, referenced across
`frontend/app/lines/[id]/page.tsx` and `LineStatusCard`). If a
`poller-nir`-equivalent produces `LineStatus`/`LineStatusReport` values
(even coarse-grained: "delayed"/"disrupted"/"normal" mapped onto the
existing severity scale) keyed by an NIR line id, the **existing
`/public/lines` and `/lines/[id]` list/detail views need no new
rendering path** — this is the strongest "reuse" finding in this spec.

**Frontend station pages: need new rendering, not just new data.** The
station detail route is `/stations/[crs]`
(`frontend/app/stations/[crs]/page.tsx`, confirmed by its test file and
the search-form's CRS-typed state) and the station search flow
(`StationSearchForm.tsx`) is built around typing/matching CRS codes. NIR
stations have no CRS. Showing an NIR station page means either a parallel
route (e.g. `/ni-stations/[id]`) or reworking the existing route to be
keyed on a generic station identifier — a real, non-trivial frontend
change, not a data-only addition. The task's framing question (§3 in the
brief) — "does the UI need new rendering paths, or just compatible
shapes" — has **two different answers for the two page types**: line
status, no; station pages, yes.

**Backend poller shape: matches `poller-tfl`'s shape for real-time data,
not `poller-ldbws`'s or `schedule-ingest`'s.** Per §1.5, there's no
GTFS/static-bulk-file delivery to justify a `schedule-ingest`-shaped
(SFTP-watch-and-parse) service. Per §1.2, the RTPI API is a REST
endpoint polled per-station on an interval — structurally the same shape
as `poller-ldbws` (`crates/poller-ldbws/src/main.rs:1-16`: "samples live
departure-board data for every station... forwards parsed
`StationSample`s") far more than `poller-tfl` (which relays an
already-computed status). **A `poller-nir` would most resemble
`poller-ldbws`, not `poller-tfl`** — because, per §1.2, what's confirmed
about the RTPI feed is raw arrivals data, Pattern B, needing this app's
own inference (thresholding delay minutes into a severity), exactly the
job `aggregator`'s `infer_from_samples` already does for GB LDBWS
sampling (DESIGN.md §6.2). This is a meaningfully bigger lift than a
`poller-tfl`-shaped integration: it needs a parallel `infer_from_samples`-
equivalent tuned to NIR data (or, optimistically, a shared one — but
that would first need to work generically over an id scheme that isn't
CRS, which today's `infer_from_samples` almost certainly isn't, given
`destination_crs_filter`'s literal name at `crates/common/src/lib.rs:483`).

**What genuinely doesn't exist yet and has to be built, regardless of
tier**: an NIR station/line catalogue (curated, since neither confirmed
data source hands over an authoritative *line-status-ready* catalogue —
only station/network *geometry*, §1.4), a parallel ingestion pipeline
(new poller + new severity-inference logic, if going past static
reference data), and — if journeys to `/stations/[nir-id]`-shaped pages
are wanted — new frontend routes.

**If §1.3's Transport API incident feed turns out to cover rail and
returns pre-computed status** (still unconfirmed — see §1.3), the
`poller-tfl` shape becomes available instead, which is meaningfully
cheaper: no bespoke inference logic, just a schema mapping onto the
existing `LineStatus` shape, the same "nothing downstream has to infer
it" property `poller-tfl`'s own module doc claims for itself
(`crates/poller-tfl/src/main.rs:5-9`). This is the single fact that would
most change this spec's cost estimate if resolved.

---

## 4. Scope options, tiered

**Tier A — static station/line reference data only, no live status.**
Hand-curate an `common::NirStation`/`NirLineDefinition`-shaped catalogue
(§2) seeded from OpenDataNI's "Northern Ireland Railways Stations" CSV/
GeoJSON and "NIR Railway Network" SHP/GeoJSON (§1.4) — both real,
accessible, licensed under OGL. Surfaces `/stations` (or a parallel NIR
listing) with real NI stations instead of zero results, and a static line
list, with no live departures, no status inference, no incident feed. No
new poller. Lowest cost tier; entirely deliverable from data already
confirmed available in §1.4, independent of either API's schema being
resolved. Frontend needs at minimum a parallel station-listing surface
(§3) since the existing CRS-keyed route can't host these without rework.

**Tier B — real-time departures if the RTPI feed's schema turns out
usable.** Add a `poller-nir`-shaped service (§3, `poller-ldbws` shape)
polling `apis.opendatani.gov.uk/translink/` per station, on whatever
cadence its 2-minute cache implies (matching `poller-ldbws`'s per-station-
per-cycle REST-call pattern, not a bulk pull). Requires actually reading
the field-level response schema first (§1.2's central unconfirmed
fact) — this session could not, due to repeated 500/503/client-render
failures on every direct fetch attempt. This tier delivers live
departure boards for NI stations (a genuine feature, comparable to what
LDBWS gives GB stations today) but **not** aggregate line status —
that's Tier C.

**Tier C — line-status/incident tracking parity with GB coverage.**
Requires one of two currently-unconfirmed things to actually be true:
(a) the Transport API's incident-information type (§1.3) covers rail and
returns something Pattern-A-shaped, letting this app relay status the
way `poller-tfl` does — cheap, if true; or (b), absent that, building a
bespoke severity-inference pipeline over Tier B's raw arrivals data,
mirroring `aggregator::infer_from_samples` but re-derived for an id
scheme that isn't CRS/TIPLOC and a network with no ATOC/segment
concepts to reuse from `LineDefinition`. Option (b) is realistically a
scaled-down repeat of this app's own LDBWS-sampling investment (DESIGN.md
§6.2), for a network roughly 5-6 lines / ~50 stations against GB's
~20-lines-today/50-100-lines-target catalogue (DESIGN.md §10) — a
meaningfully worse effort-to-coverage ratio than continuing GB line-
catalogue growth, echoing the survey doc's own cost/benefit framing for
every other network it surveyed.

**Recommended minimum viable first tier: Tier A.** It's the only tier
this spec can ground entirely in confirmed, currently-accessible data
(§1.4) with no dependency on an API schema this session couldn't read or
an access-gated key this session didn't request. It's also honestly
scoped to what the confirmed data supports — a real, if modest, win
(`/stations` stops returning zero NI results) without overclaiming live
status capability the actual data situation doesn't yet support.

---

## 5. Go/no-go recommendation

**No-go on Tier B/C right now; conditional go on Tier A only if NI
coverage is wanted as a stated goal, not as incremental network
coverage.**

The data situation is more substantial than the survey doc's own
caveated finding suggested in one respect (§1.4's static station/network
datasets are real, accessible, and unambiguous — better than "unverified
as a concrete data source," the survey doc's own hedge) but **no more
resolved in the respect that mattered most**: whether either live API is
Pattern-A (cheap, TfL-shaped) or Pattern-B (expensive, needs bespoke
inference) remains unconfirmed after two independent research passes
now, both blocked by the same access/availability failures
(`apis.opendatani.gov.uk` 500/503, `tiger.worldline.global` client-render-
only, `translink.co.uk/api`'s real schema gated behind an unrequested
email registration). What is confirmed points toward the more expensive
shape by default (§1.2: "arrivals/departures," i.e. raw per-service
data, not a status field) — the same conclusion the survey doc reached,
now with the GB-TOC-exclusion question actually closed instead of
circumstantial (§1.1).

Combined with §2's finding that NIR needs a **structurally separate**
catalogue and pipeline — not an extension of `common::Station`/
`LineDefinition`, given `Station.crs`'s required-field status and the
CRS-keyed frontend route — and §4's sizing (5-6 lines / ~50 stations,
smaller than this app's *current* GB catalogue, let alone its 50-100-line
target), a full Tier C build is not recommended: it would cost roughly a
scaled-down repeat of the LDBWS-sampling investment (DESIGN.md's largest
single engineering investment to date, per §6.2) for a network smaller
than what's already covered, mirroring almost exactly the survey doc's
cost/benefit conclusion for every other network it looked at.

**If Northern Ireland coverage becomes a goal in its own right** (the
condition the survey doc set, and the frame this task's owner appears to
be operating in), the concrete, low-risk next steps, in order:

1. **Ship Tier A.** Cheap, fully groundable in already-confirmed data
   (§1.4), delivers real user-visible value (`/stations` stops lying
   about NI's existence), and requires no new poller or inference logic.
2. **Actually register for the Transport API key**
   (`servicedata@translink.co.uk`, §1.3) and separately retry
   `apis.opendatani.gov.uk/translink/` fetches with a real browser/
   session (both endpoints failed only with automated single-shot
   fetches this session and the survey doc's — a human with a browser, or
   a registered key, would likely resolve this in well under an hour,
   the same caveat the survey doc gave for its own tooling-blocked
   networks). This single step is what would convert §4's Tier B/C from
   "no-go, unconfirmed" to an actual scoped estimate either way.
3. **Only after that**, revisit whether Tier B or C is worth a dedicated
   follow-up design-spec pass, at the depth
   `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
   gave TfL/Overground — this document is not that spec; it's the
   evidence-gathering and scoping step the survey doc explicitly asked
   for before one gets written.

---

## References

- `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
  — this document's starting point; its Translink/NI section, open
  question 7, and overall recommendation are the basis for §1's structure.
- `crates/common/src/lib.rs:443-451` (`Station`), `:461-500`
  (`LineDefinition`), `:1070`/`:1418` (only `"national-rail"` mode value
  in this codebase's line catalogue), `:1442-1443` (TfL's five modes,
  confirmed separate from `LineDefinition.mode`).
- `crates/api/src/data/queries.rs:460-494` (`TflLineSummaryRow`,
  `tfl_line_summaries` — the TfL-bypasses-`LineDefinition` doc comment
  quoted in §2).
- `crates/api/src/render.rs:14-44` (`to_tfl_shape`/
  `to_tfl_shape_with_overlay` — the shared JSON shape §3 argues NIR line
  status could reuse).
- `crates/poller-tfl/src/main.rs:1-13` (Pattern A module doc, quoted in
  §3); `crates/poller-ldbws/src/main.rs:1-16` (Pattern B / per-station
  polling shape, the closer analogue for a hypothetical `poller-nir`).
- `frontend/app/stations/[crs]/page.tsx` and
  `frontend/app/stations/StationSearchForm.tsx:11` (CRS-keyed station
  route/search, cited for §2/§3's frontend-rework argument).
- DESIGN.md §5.1 (Lines: ATOC operators), §6.2 (LDBWS inference — the
  cost precedent cited in §4/§5), §10 (line-catalogue size targets, used
  for §4/§5's sizing comparison).
- Open Rail Data Wiki TOC Codes page (still 403's on direct fetch this
  session, as it did in the survey doc's pass):
  https://wiki.openraildata.com/index.php/TOC_Codes
- Wikipedia, "List of companies operating trains in the United Kingdom"
  (fetched this session; source for §1.1's "except in Northern Ireland"
  confirmation):
  https://en.wikipedia.org/wiki/List_of_companies_operating_trains_in_the_United_Kingdom
- OpenDataNI, "Translink Northern Ireland Railway Real Time Passenger
  Information" dataset (fetched this session):
  https://www.opendatani.gov.uk/dataset/real-time-rail-stations-arrivals-and-departures
- Translink Transport Information API page (fetched this session, same
  page the survey doc fetched): https://www.translink.co.uk/api
- `apis.opendatani.gov.uk/translink/index.html` (503, then 500, on two
  fetch attempts this session — matches the survey doc's own 503) and
  `https://tiger.worldline.global/toc/NIR` (client-render-only shell, no
  data recovered) — both cited in §1.2 as the still-unresolved schema
  gap.
- OpenDataNI, "Northern Ireland Railways Stations" dataset (found via
  search this session; dataset page itself 403'd on direct fetch, so its
  existence/format list is confirmed via search-result summary and the
  CKAN portal's own indexing, not a directly-read page):
  https://admin.opendatani.gov.uk/dataset/northern-ireland-railways-stations
- Station-name and line-structure cross-check (secondary sources, not a
  verified dataset schema — see §1.4's caveat): general web search this
  session, corroborated by Wikipedia's Belfast–Derry line and Belfast
  Grand Central station articles.
