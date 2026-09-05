# Ireland vs. Northern Ireland — Friction Research

**Status: lightweight research note, not a design spec.** Written at the
repo owner's request ("a lil research") to answer one question: *how much
friction would adding Republic-of-Ireland (Iarnród Éireann) coverage add
on top of Northern Ireland coverage, versus just doing NI alone?* This
builds on `docs/superpowers/specs/2026-09-05-northern-ireland-rail-support-design.md`
("the NI spec") but does not re-litigate it — NI's own findings (no
GTFS/GTFS-RT, a currently-unreachable bespoke JSON/XML API, real static
CSV/GeoJSON reference data, ~50 stations/5-6 lines) are taken as given.
Everything below is this session's own verification — direct fetches,
downloads, and one crates.io API query — not a restatement of prior
web-search summaries. Per this repo's citation convention, every claim is
tied to something actually fetched this session, with unconfirmed points
flagged as such.

---

## 1. What's actually involved in ingesting Iarnród Éireann's data

Two independent, unrelated data sources exist, and it matters which one
this app would use for what:

**A. Static GTFS via Transport for Ireland — real, no key, no
registration.** `transportforireland.ie/transitData/PT_Data.html` (fetched
this session) links directly to per-operator GTFS zips, including
`Data/GTFS_Irish_Rail.zip`. This session downloaded it directly — no API
key, no sign-up, a plain anonymous HTTPS GET of a 9.6 MB zip. Unzipped and
inspected directly:

- `feed_info.txt`: publisher is the National Transport Authority (not
  Iarnród Éireann itself), `feed_start_date=20260904`,
  `feed_end_date=20270905` — i.e. a live, currently-valid, rolling
  one-year feed, refreshed regularly (today's date is baked into the
  window).
- `agency.txt`: single agency, `IR` / "Iarnród Éireann / Irish Rail".
- `routes.txt`: 19 route rows / 18 distinct named corridors (one corridor,
  `DUB-BFT`, is split into explicit `-I`/`-O` inbound/outbound rows; the
  rest are single rows each).
- `stops.txt`: **152 stations** (data row count, header excluded).
- Full standard GTFS file set: `stop_times.txt`, `trips.txt`,
  `shapes.txt`, `calendar.txt`, `calendar_dates.txt`, `translations.txt` —
  nothing missing or stubbed.

This is the single cleanest fact in this whole research pass: **a
complete, schema-correct, immediately-parseable rail schedule feed was in
hand within one `curl` and one `unzip`, no gate at all.** Contrast NI
spec §1.2/§1.5: NIR has no static bulk-schedule feed of any kind; its
station/line reference data is CSV/GeoJSON, not GTFS, and its live API
500s/503s on every fetch attempt across two research sessions.

**B. Live/real-time data — NOT via GTFS-Realtime; via a separate, older,
plain XML API.** This required checking, because it would have been the
obvious next assumption ("static is GTFS, so realtime is GTFS-RT") and
it's wrong. Searched and confirmed via data.gov.ie's own blog
announcement: the NTA's GTFS-Realtime product (`gtfsr.transportforireland.ie/v1`,
registration required at `developer.nationaltransport.ie`) covers **Dublin
Bus, Bus Éireann, and Go-Ahead Ireland only**. No rail. No DART. A
community project (`ha-gtfs-rt-irl`) independently corroborates this: as
of its writing, GTFS-R v1 shipped trip-updates only, with a vehicle-position
API "not... provided thus far," and no rail agency listed anywhere in the
GTFS-R rollout.

Instead, Iarnród Éireann's own real-time data lives at a much older,
separate, unauthenticated endpoint: `api.irishrail.ie/realtime/realtime.asmx`
— a legacy ASMX/SOAP web service, callable as plain REST-style GET too.
This session fetched it live, successfully, on the first attempt, no key:

```
GET http://api.irishrail.ie/realtime/realtime.asmx/getStationDataByCodeXML?StationCode=BFSTC
```
returned a well-formed, fully populated `<objStationData>` record per
train serving that station — `Origin`, `Destination`, `Scharrival`/
`Schdepart`, `Exparrival`/`Expdepart`, `Late` (minutes), `Status`,
`Duein` — i.e. **a live per-service departure-board record with an
explicit delay-minutes field already computed**, structurally almost
identical to what `poller-ldbws` already ingests for GB stations. A
companion `getAllStationsXML` call returned a 171-station master list
with codes/coordinates.

The contrast worth stating plainly: **this legacy Irish Rail API is
non-standard, undocumented on any current NTA developer portal, and (per
a third-party developer's public comment surfaced in search) apparently
unchanged since around 2012 — yet it answered every query this session
sent it, immediately, with a clean and legible schema.** NIR's own,
much more modern-looking OpenDataNI-hosted API has done the opposite
across two independent research sessions: well-documented, actively
maintained-looking, and completely unreachable. Maturity of *documentation*
and reliability of *access* are not the same axis, and for Ireland they
point in opposite directions from what a naive "GTFS is old and rail-only
this-legacy-thing is a red flag" read would suggest.

**Registration**: the NTA developer portal (`developer.nationaltransport.ie`,
partially fetched — its content is behind an Azure API Management portal
shell that doesn't render statically, so most of its detail comes from an
initial summarized fetch, not full verification) requires sign-up for API
keys generally, but **the static GTFS zips and the legacy `api.irishrail.ie`
endpoint needed no key at all** in this session's direct tests. Whatever
key-gated products exist on the NTA portal (GTFS-RT bus feeds, journey
planner APIs) are simply not the path this app would use for Iarnród
Éireann rail data, based on what was actually reachable.

---

## 2. Is GTFS parsing more or less work than NIR's bespoke API?

Genuinely less, on the evidence gathered this session — not a foregone
conclusion, but not close either:

- **The spec itself is mature, versioned, and free.** `gtfs.org/documentation/schedule/reference/`
  (fetched this session, maintained by MobilityData): a single reference
  document covering all ~31 possible GTFS files (6 required, 5
  conditionally required, the rest optional), formal RFC-2119 requirement
  language, an explicit change history, described as the standard for
  "current and upcoming service" data across the industry. There is
  nothing to reverse-engineer — every field this app would need
  (`stops.txt`'s lat/lon, `routes.txt`'s long/short names, `trips.txt`/
  `stop_times.txt` for topology) is already named and typed in a public
  spec, unlike NIR's still-unread field-level schema.
- **The Rust ecosystem already has this solved, and it's actively
  maintained, not abandoned.** Queried `crates.io`'s own API directly
  (`GET /api/v1/crates?q=gtfs`) rather than trusting a prior summary.
  Real, current results: `gtfs-structures` (195,057 downloads, latest
  `0.50.0` published **2026-09-01** — four days before this research) is
  a complete in-memory GTFS parser with by far the largest install base
  found. A cluster of validator crates from one project —
  `gtfs-analyzer`/`gtfs-core`/`gtfs-config`/`gtfs-pipeline`/`gtfs-rules`
  (all `0.12.0`, all published **2026-09-01/09-02**) — shows active,
  current-week development, not an abandoned ecosystem. `gtfs-rt`
  (26,665 downloads) and `gtfs-realtime` (24,902 downloads) cover the
  GTFS-Realtime protobuf side, and several agency-specific crates
  (`amtrak-gtfs-rt`, `via-rail-gtfsrt`, `chicago-gtfs-rt`,
  `rtc-quebec-gtfs-rt`, `nictd-gtfs-rt`, `zotgtfs`) show this "wrap a
  legacy real-time feed and expose/consume it GTFS-RT-shaped" pattern is
  a well-trodden path elsewhere, not something this app would be
  pioneering. Net: this app would not be writing a GTFS parser; it would
  be picking one off the shelf (`gtfs-structures` is the obvious default)
  and mapping its already-typed structs onto `common::` types.
- **What GTFS parsing does *not* remove**: the real-time side still needs
  bespoke work either way. Per §1, GTFS-RT doesn't cover Irish rail at
  all, so a `poller-irish-rail`-equivalent would still poll
  `api.irishrail.ie`'s legacy per-station XML and do LDBWS-style
  delay-threshold inference — the same Pattern-B shape and cost this
  app already pays for GB LDBWS and (per the NI spec) would likely pay
  for NIR's RTPI feed too. **GTFS only cheapens the static/topology half
  of the problem** (station catalogue, route definitions, geometry) —
  exactly the half the NI spec found itself hand-curating from CSV/GeoJSON
  because no GTFS existed for NIR. It does not cheapen live-status
  inference, which is comparable cost for both networks.
- **Honest bottom line for this question**: for the static/topology tier
  (NI spec's "Tier A"), Iarnród Éireann is **less** work than NIR, not
  more — a mature parser reads a live, complete, standard feed, versus
  hand-curating structs from a CSV whose exact column schema this
  research still hasn't read (NI spec §1.4's own caveat: the NI stations
  dataset page 403'd on direct fetch). For the live/real-time tier, the
  two are roughly comparable in kind (both need bespoke per-service
  inference) but Iarnród Éireann's endpoint is demonstrably *reachable
  right now*, which NIR's is not, across two research sessions.

---

## 3. Scale comparison

| | Northern Ireland (NIR) | Republic of Ireland (Iarnród Éireann) |
|---|---|---|
| Stations | ~50 (NI spec §1.4, secondary-source cross-check, not a verified dataset schema) | **152** (this session's own count of `stops.txt` in the live-downloaded GTFS feed — a primary count, not a secondary estimate) |
| Lines/routes | 5-6 (NI spec §1.4, same caveat) | **18** distinct named corridors (this session's own count of `routes.txt`, base IDs with `-I`/`-O` suffixes collapsed) |
| Route-network size | ~220 route miles (NI spec §1.4, OpenDataNI dataset description) | 2,200 km / 146 stations per Wikipedia's Iarnród Éireann article (Feb 2025 company figures) — broadly consistent with, and a bit lower than, this session's own 152-stop GTFS count (plausibly because the GTFS feed catalogues a few Enterprise-side NI stops Iarnród Éireann doesn't count as "its" network — see §4) |

Ireland's rail network is roughly **3x** Northern Ireland's by both
station and line count, on numbers this session pulled directly rather
than took from a secondary source for the Ireland side. This matters for
framing: "adding Ireland too" is not a marginal addition on top of NI —
it's the larger of the two catalogues, by a clear margin, even though NI
was the network this app already spec'd first.

---

## 4. The cross-border Enterprise service — complication, not a clean split

Checked directly rather than assumed, and the answer is unambiguous:
**Iarnród Éireann's own data — both the static GTFS feed and the live
XML API — extends well past the border into Northern Ireland. It is not
cleanly split at the border.**

Evidence, all from this session's own downloads/fetches:

- `stops.txt` in the downloaded `GTFS_Irish_Rail.zip` includes **Belfast**,
  **Lisburn**, **Portadown**, **Lurgan**, and **Newry** as first-class
  stops (grepped directly out of the file; coordinates confirm they're
  the real NI towns, e.g. Belfast at 54.59°N/-5.94°W).
- `routes.txt` includes an explicit `DUB-BFT-I`/`DUB-BFT-O` pair
  ("Belfast - Dublin" / "Dublin - Belfast"), with real trip and
  stop-time rows in `trips.txt`/`stop_times.txt` for both directions.
- The live `api.irishrail.ie/realtime/realtime.asmx/getAllStationsXML`
  call (fetched directly this session) lists not just the passenger
  stations above but also **CITY JUNCTION, CENTRAL JUNCTION, DUNMURRAY,
  MOIRA** — signalling/junction points on the NI side of the route,
  each explicitly aliased `"Dublin Belfast"` — showing Iarnród Éireann's
  own real-time system models the entire physical route into Belfast,
  not just its own-country segment.
- A live `getStationDataByCodeXML?StationCode=BFSTC` call (BFSTC = Irish
  Rail's own code for Belfast) returned a real, currently-scheduled
  Enterprise service (`Traincode A101`, Belfast → Dublin Connolly,
  06:00 departure) with full delay/status fields populated — i.e. this
  is live, queryable data for a train sitting entirely within Northern
  Ireland at query time, under Iarnród Éireann's own station-code scheme
  (`BFSTC`, `LBURN`, `PDOWN`, `LURGN`, `NEWRY` — all distinct from
  whatever code scheme NIR's own RTPI feed would use for the same
  physical places).

**Consequence for "adding both": real double-counting/correlation risk,
not a clean handoff.** If both NIR's own feed and Iarnród Éireann's
feed/API were ingested, the same physical Enterprise train and the same
physical stations (Belfast, Lisburn, Portadown, Lurgan, at minimum) would
appear in both sources, under two unrelated identifier schemes, with two
independently-computed delay/status readings that could disagree. This
isn't hypothetical — it's what these two live systems are already doing
independently today, per the NI spec's own account of NIR's Belfast
Grand Central/Enterprise coverage and this session's direct confirmation
of Iarnród Éireann's mirroring coverage of the same stations. A combined
integration would need to pick one authoritative source for the shared
Belfast–border segment (most naturally: NIR's own data for the Belfast-side
stations, Iarnród Éireann's for everything south of the border, with the
Enterprise service itself either de-duplicated or deliberately shown from
both operators' perspectives) — a genuine design question the "just add
both" framing doesn't resolve for free. This is real added complexity
specific to combining the two, that neither network alone would face.

---

## 5. Data-model implications: still two parallel catalogues, or a shared abstraction?

The NI spec's answer (§2) for NIR alone is a parallel
`common::NirStation`/`common::NirLineDefinition` catalogue, deliberately
not reusing `common::Station`/`LineDefinition` (whose `crs: String` field
is required and has no NIR-shaped value), mirroring how TfL's five modes
already bypass `LineDefinition` entirely via `TflLineSummaryRow`.

**Adding Iarnród Éireann does not fit that same NIR-specific shape
as-is, and doesn't obviously need CRS/ATOC either — a real, if modest,
generalization opportunity appears once there are two non-GB networks
instead of one.** Concretely:

- Both NIR and Iarnród Éireann share the same two structural gaps from
  `common::Station`: no CRS code, no ATOC operator code. A
  `common::NirStation` typed specifically for NIR wouldn't be reusable
  for Iarnród Éireann as-is (wrong name, and it would invite confusing a
  ROI station for an NI one), but the *shape* — `id: String`,
  `name: String`, `latitude/longitude: Option<f64>`, no CRS/ATOC/segment
  — is identical for both. A generically-named type (something like
  `common::NonGbStation`/`common::NonGbLineDefinition`, or one keyed by
  an explicit `network: NonGbNetwork` enum with `NorthernIreland`/
  `Ireland` variants) would serve both without duplicating the struct
  definition twice.
- This generalization is **only worth doing if both are actually in
  scope.** If NI alone is ever the only network added, the NI spec's own
  NIR-specific naming is simpler and clearer — there's no reason to
  build an abstraction for a category of one. The generalization case
  only becomes real the moment a second non-GB network is actually on
  the table, which is exactly the scenario this doc is about.
- One real point of difference the shared type would still need to
  represent: Iarnród Éireann's data is GTFS-shaped (route/trip/stop
  topology already exists in a standard form, per §1/§2), while NIR's is
  not (hand-curated from CSV/GeoJSON, per the NI spec). A shared
  `common::` type can still hold the same fields either way — the
  *ingestion* pipelines feeding it would differ (a GTFS-parser-backed
  loader for Ireland vs. a hand-curated/CSV-backed loader for NI), but
  that's a poller/ingestion-layer difference, not a reason to fork the
  domain type itself. This is analogous to how this app already has
  multiple distinct pollers (`poller-ldbws`, `poller-tfl`,
  `schedule-ingest`) feeding a shared `common::` model today.
- The one place a shared abstraction gets awkward: §4's border overlap.
  A clean `network` enum per station stops being clean exactly at
  Belfast/Lisburn/Portadown/Lurgan, which — depending on the chosen
  source of truth — might need to be modeled as NIR stations, Iarnród
  Éireann stations, or (least appealing but most honest) both, with a
  cross-reference. This is a real wrinkle a two-network shared model has
  to answer that a single-network (NI-only) model never would.

---

## 6. Bottom-line friction estimate

**Adding both is not meaningfully more expensive than NI alone for the
static/reference-data tier — it may be cheaper per unit of coverage on
the Ireland side specifically — but it is genuinely more expensive
overall than NI alone once live status and the cross-border overlap are
counted, because of two costs unique to "both" that "NI alone" would
never incur:**

1. **The Iarnród Éireann static/topology tier is close to free relative
   to NIR's equivalent.** A live, complete, standard GTFS feed,
   downloadable with no key, parseable with an existing, actively
   maintained crate (`gtfs-structures`), covering 3x NIR's station/line
   count — versus NIR's own Tier A, which requires hand-curating structs
   from a CSV whose schema this research still hasn't directly read.
   Per-station or per-line, Ireland's reference-data tier is **cheaper**
   to stand up than NIR's, not more expensive, despite being the larger
   catalogue.
2. **The live-status tier is comparable in kind, and currently more
   accessible in practice, for Ireland.** Both networks need bespoke
   LDBWS-style delay inference (neither has a Pattern-A pre-computed
   status feed confirmed reachable — Iarnród Éireann's GTFS-RT is
   bus-only, and NI spec §1.3's Translink incident-API lead is still
   unregistered/unconfirmed). But this session could actually read
   Iarnród Éireann's live per-station data on the first try; NIR's
   equivalent has failed on every attempt across two independent
   sessions. If NIR's API remains unreachable, "add Iarnród Éireann's
   live tier too" is not an incremental cost on top of a working NI
   live tier — it may end up being the *only one of the two that's
   actually buildable* in the near term.
3. **The real added cost specific to "both" is the border overlap
   (§4) and the data-model fork-or-generalize decision (§5).** Neither
   of these exists at all for "NI alone." Reconciling two independent,
   differently-coded views of the same physical Enterprise stations and
   trains is genuine new design work with no NIR-only analogue — this is
   the one place "more countries = more friction" clearly holds.
4. **Scale (§3) is not, by itself, the source of friction.** Iarnród
   Éireann being 3x NIR's size doesn't 3x the engineering cost, because
   GTFS parsing cost is close to flat with respect to feed size (the
   same `gtfs-structures` call reads 15 stations or 1,500) — the
   friction that does scale with "adding a second network" is entirely
   in the border-overlap and shared-abstraction work of §4/§5, which is
   roughly fixed-cost, not proportional to either network's size.

**So: the honest comparative read is "roughly the same total engineering
lift as NI alone, redistributed" — a genuinely cheaper static/reference
tier for the larger Ireland side, offset by one real fixed cost (the
Belfast-area border overlap and the two-network data-model decision)
that NI alone would never face — rather than a straightforward "two
networks cost twice as much as one."** If anything, doing both together
may be a more *efficient* first target than NI alone, precisely because
Iarnród Éireann's reference-data tier is real, standard, and immediately
usable in a way NIR's own isn't yet — the honest caveat being that this
flips the NI spec's own "Tier A first, cheaply, now; Tier B/C later,
conditionally" recommendation into "Ireland's Tier A is the cheap win
available today; NIR's Tier A still needs its CSV schema actually read
first."

---

## References

- `docs/superpowers/specs/2026-09-05-northern-ireland-rail-support-design.md`
  — NI-side findings taken as given throughout (§§1-2 especially).
- `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
  — tone/format precedent for this doc.
- Transport for Ireland open-data index (fetched this session; source of
  the direct GTFS zip links used in §1):
  https://www.transportforireland.ie/transitData/PT_Data.html
- `GTFS_Irish_Rail.zip` (downloaded and unzipped directly this session):
  https://www.transportforireland.ie/transitData/Data/GTFS_Irish_Rail.zip
  — `feed_info.txt`, `agency.txt`, `routes.txt`, `stops.txt` all read
  directly; figures in §1/§3/§4 are this session's own counts from this
  file, not a secondary source.
- NTA developer portal (partially fetched — mostly an Azure API
  Management shell that doesn't render statically; used only for the
  GTFS-RT/registration claims in §1, corroborated by the data.gov.ie
  post below): https://developer.nationaltransport.ie/
- data.gov.ie blog post confirming GTFS-Realtime is bus-only (Dublin
  Bus/Bus Éireann/Go-Ahead), no rail:
  https://data.gov.ie/ga/blog/gtfs-r-real-time-for-dublin-bus-bus-eireann-and-go-ahead-services
- `ha-gtfs-rt-irl` (community project; corroborates GTFS-R v1/v2 scope
  and the "no vehicle-position API yet" note in §1):
  https://github.com/Jerry-F-1/ha-gtfs-rt-irl
- `api.irishrail.ie` legacy realtime API — fetched live and directly
  this session (§1, §4): `getCurrentTrainsXML`, `getAllStationsXML`,
  `getStationDataByCodeXML?StationCode=BFSTC`, all at
  `http://api.irishrail.ie/realtime/realtime.asmx/`.
- GTFS Schedule reference specification (fetched this session, confirms
  maintainer/maturity claims in §2): https://gtfs.org/documentation/schedule/reference/
- crates.io API, queried directly this session (§2):
  `https://crates.io/api/v1/crates?q=gtfs&per_page=50` — `gtfs-structures`,
  `gtfs-rt`, `gtfs-realtime`, `gtfs-analyzer`/`gtfs-core`/`gtfs-config`/
  `gtfs-pipeline`/`gtfs-rules`, and agency-specific GTFS-RT crates cited
  directly from this response.
- Wikipedia, Iarnród Éireann article (fetched this session; source of
  the 2,200 km / 146-station Feb 2025 company figures in §3):
  https://en.wikipedia.org/wiki/Iarnr%C3%B3d_%C3%89ireann
