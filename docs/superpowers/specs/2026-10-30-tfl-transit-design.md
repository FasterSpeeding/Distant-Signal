# Transit networks: TfL (Underground, DLR and London Trams)

Design, 2026-10-30. Status: **accepted for build** (DS-212). It extends
[the transit networks design](2026-10-09-transit-networks-design.md)
("the transit spec"), reuses the TransXChange adapter of
[the NET design](2026-10-24-nottingham-net-design.md) ("the NET design"),
and replaces the London parts of
[the transit trip planning design](2026-10-24-transit-trip-planning-design.md)
("the planning spec", §12 and §13). "§" alone means a section of this
document.

The user decided on 2026-10-30:

- **Source:** TfL's Journey Planner timetables (TransXChange).
- **Shape:** one transit network, `tfl`, at `/transit/tfl`, holding the
  Underground, the DLR and London Trams. Scheduled times join the existing
  TfL stop pages (`/tfl/stops/{id}`); the Overground and the Elizabeth line
  link to their National Rail pages; the "Trams" navigation and copy
  become mode-neutral.
- **Scope:** phases P0 to P4 (§13), the planner included, each behind an
  off-by-default switch.

## Contents

1. [Summary](#1-summary)
2. [Sources and measurements](#2-sources-and-measurements)
3. [The override rule](#3-the-override-rule)
4. [The network](#4-the-network)
5. [Adapter changes](#5-adapter-changes)
6. [Line modes](#6-line-modes)
7. [Status from TfL's line status](#7-status-from-tfls-line-status)
8. [Pages and API](#8-pages-and-api)
9. [Stream, budget, Redis, grants and chart](#9-stream-budget-redis-grants-and-chart)
10. [Storage and database load](#10-storage-and-database-load)
11. [The planner (P4)](#11-the-planner-p4)
12. [Attribution](#12-attribution)
13. [Phases, tests and effort](#13-phases-tests-and-effort)
14. [Deploy changes](#14-deploy-changes)
15. [Risks and open questions](#15-risks-and-open-questions)
16. [Errata to the planning spec](#16-errata-to-the-planning-spec)

## 1. Summary

- **The source** is TfL's weekly Journey Planner export,
  `https://tfl.gov.uk/tfl/syndication/feeds/journey-planner-timetables.zip`:
  anonymous, about 141 MB, a zip of zips. One inner zip,
  `LULDLRTRAMRIVERCABLE FULL <ddmmyyyy>.zip` (8.8 MB, 333 MB unzipped),
  holds the Underground, DLR, tram, river and cable car timetables in
  TransXChange 2.4, about 60 days ahead (§2.1).
- **TfL publishes overrides as whole files.** A line has one long base file
  and short files for engineering work and bank holidays, with no
  exclusion in the base file. A day's timetable is the **narrowest active
  file** of its line, not the union. Checked on three drops (2024, 2025,
  2026) and against TfL's own status for a tram part closure (§3).
  The BODS London GTFS takes the union, so its London counts are inflated
  (§16).
- **One network, `tfl`,** through the existing producer, stream and
  tables, with the TXC adapter extended: the inner zip, three ATCO
  prefixes, `DepartureDayShift`, the override rule and a two-pass read
  that never holds the whole feed as a tree (§5).
- **Lines carry a mode** (`tube`, `dlr`, `tram`), a nullable column on
  `transit_lines`, so `/transit/tfl` groups them (§6).
- **Status comes from TfL**, the `tfl-<line>` rows poller-tfl already
  writes, not from SIRI-SX: the network has no situations loop (§7).
- **Pages.** `/transit` becomes "Trams and metros" with a London card;
  `/transit/tfl` lists the lines by mode plus the Overground and Elizabeth
  line linking out; a Tube line shows a frequency summary, first and last
  trains and its branches; a stop's scheduled departures show on
  `/tfl/stops/{id}` beside the live arrivals, and `/transit/tfl/stops/…`
  redirects there (§8).
- **Size.** About 7,600 patterns, 210,000 pattern stops and 64,000 trips
  per feed version; a weekday has 8,810 Underground trips with 206,000
  calls (§2.3, §10). The stream needs about 150 parts a day and the
  ingest budget a rise (§9).
- **Planner (P4).** About 230,000 connections a weekday, half the rail
  day: real Tube, DLR and tram times and line changes, replacing the
  `tfl_access` estimates where the timetable covers an end, behind
  `api.tripPlanTransitNetworks: [tfl]` with a measured memory and CPU
  budget (§11).

## 2. Sources and measurements

All figures were measured on 2026-10-30 in the session scratchpad.
Nothing from the feed is committed; tests use hand-trimmed fixtures.

### 2.1 TfL's Journey Planner timetables

| What | Value |
|---|---|
| URL | `https://tfl.gov.uk/tfl/syndication/feeds/journey-planner-timetables.zip` (anonymous; the `content.tfl.gov.uk` copy linked from the open data page is a stale 2025 file) |
| Updated | weekly (`Last-Modified: Mon, 26 Oct 2026`; the London Datastore says Thursdays) |
| Outer zip | 141 MB, six inner zips: four bus parts, replacement buses, and `LULDLRTRAMRIVERCABLE FULL 26102026.zip` |
| Rail inner zip | 8.8 MB, 33 XML documents, 333 MB unzipped, the largest 43 MB (`tfl_1-NTN-…`) |
| Format | TransXChange 2.4, one service per document; `OperatorCode` `LUL`, `DLR`, `TCL` (no `NationalOperatorCode`); `Mode` `underground`, `rail` (DLR and cable car), `tram`, `ferry` |
| Horizon | 2026-10-24 to 2026-12-23 |
| Stops | platform ATCO codes, `9400ZZLUBST1`, `9400ZZDLBNK2`, `9400ZZCRWMB1` |
| Times | every journey timetabled (no `Frequency`); journeys after midnight are `00:12:00` with `DepartureDayShift` 1; 8,056 `pass` activities |
| Licence | TfL's open data terms (OGL v2 based, "Powered by TfL Open Data") |

Documents by mode: 22 Underground (11 lines, some with override files),
3 DLR, 2 tram, 4 river, 2 cable car. The Overground and the Elizabeth
line are not in it.

### 2.2 The other sources (not used)

- **BODS London GTFS** (anonymous, OGL, 321 MB, `stop_times.txt` 1.58 GB)
  has `LULD`, `LDLR`, `TRAM` and the cable car, built from the same TfL
  data (its agency URLs are Traveline's), but with the override files
  merged in as extra trips (§16).
- **TNDS** (`L.zip`) very likely carries the same TfL documents (the BODS
  agencies point at Traveline); not confirmed, it needs the DS-171 login,
  and it would add FTP and a login for nothing the TfL file lacks.
- **The Unified API** (`/Line/{id}/Timetable/{stop}`) gives each stop's
  departures by day type (`knownJourneys`), frequency bands (`periods`)
  and stopping patterns (`stationIntervals`), but no dates, no trip ids,
  whole minutes, and only what departs the stop queried: about 1,000
  calls a day for full cover. The Piccadilly answer from Cockfosters had
  only "Mondays" and "Tuesdays" on 2026-10-30, apparently one override
  file. Kept for poller-tfl's running times (#82); not a timetable source.
- **The Overground and the Elizabeth line** are National Rail services in
  CIF and Darwin (operators `LO` and `XR`), already catalogue lines
  (`lines/overground-*.toml`, `lines/elizabeth-*.toml`) with timetables at
  `/lines/{id}/timetable`, live times and planner coverage.
  `common::TFL_TO_NR_LINE_ID` already folds TfL's status for them into
  those lines. They are not ingested again (§8.5).

### 2.3 Counts under the override rule

A week with no override (Monday 2026-11-16 to Sunday 11-22), journeys
expanded by their profiles, the narrowest active file per line and day:

| Day | Underground trips / calls | DLR trips / calls | Tram trips / calls |
|---|---|---|---|
| Mon to Thu | 8,806 to 8,810 / 206,450 | 1,584 / 20,574 | 707 / 13,881 |
| Fri | 8,977 / 211,736 | 1,584 / 20,574 | 707 / 13,881 |
| Sat | 8,134 / 203,447 | 1,434 / 18,703 | 659 / 12,703 |
| Sun | 6,977 / 173,059 | 1,200 / 15,631 | 515 / 9,375 |
| **Week** | **59,324 / 1,414,046** | 10,554 / 137,204 | 4,709 / 91,483 |

(DLR figures are the "rail" mode less the cable car's 254 weekday and
230 Sunday journeys of two calls.) A weekday's connections (calls less
trips): about 197,600 Underground, 19,000 DLR and 13,200 tram, **about
230,000** in all.

Per feed version, folded as the adapter folds (line, direction, calls and
offsets): **7,617 patterns with 210,021 pattern stops** (Underground
7,495 / 208,515; DLR 101 / 1,158; tram 21 / 348) and about **64,000
trips** (every journey of every file, the cable car and ferries left out;
the override rule drops none, it only narrows their dates).

## 3. The override rule

### 3.1 The evidence

Measured with throwaway scripts in the session scratchpad: each line's files were compared with
its longest file (the base) by period, day types, the stations called at
and the departure times in common.

- **2026-10-26 drop.** Central has a base (10-24 to 12-23) and files for
  10-30 to 11-01, 11-02 to 11-09 and 11-10, each the same timetable
  (1,045 of 1,046 departures in common). Piccadilly 10-27 to 11-02 has 351
  journeys on Wednesday to Friday against the base's 844, only 46 in
  common: a reduced service, not extra trains. District, Circle,
  Hammersmith & City, Northern and DLR the same way.
- **The tram file** `tfl_63-TR-_-y05-150` (10-24 to 11-05) calls at 20 of
  the 39 stops with 292 weekday journeys, against the base's 707. TfL's
  status on 2026-10-30: "Saturday 24 October to Thursday 5 November, no
  service between East Croydon and Beckenham Junction / Elmers End / New
  Addington due to planned engineering work." So the short file is the
  whole tram timetable for those days; the base must not run alongside it.
- **2025-04-29 drop** (an archived copy): bank holiday files (Victoria,
  Jubilee, Northern on 5 May, 26 May, 25 August), DLR part closures (17 or
  25 of 45 stations missing), Bakerloo and Metropolitan successors whose
  periods do not overlap the base.
- **2024-06-10 drop**: the same shapes, including a Northern file
  (06-10 to 12-15) nested inside its base (06-08 to 12-23) with the base's
  timetable.

In no case is an override additive: each is a complete timetable of its
line for its days, often identical to the base. TfL's base files have no
`DaysOfNonOperation` for override days.

### 3.2 The rule

For each **line** (its TXC `LineName`, mapped to a DS line) and each
**date** in the horizon:

1. A document is **active** on the date when the date is inside its
   service's `OperatingPeriod` and the service-level `OperatingProfile`
   (if any) runs on that weekday, ignoring bank holiday and special-day
   rules (a bank holiday file is narrower anyway).
2. The **winner** is the active document with the shortest
   `OperatingPeriod` (`end - start`); ties go to the later start, then
   the later `ModificationDateTime`, then the entry name.
3. Only the winner's journeys run on that date. Each journey's own
   profile (days, bank holidays, special days) still applies inside it.

A line with no active document that date has no service (Waterloo & City
on a Sunday). The rule is per line, not per network: a DLR closure file
never hides the Underground.

**Scope.** The rule is a network-file option, `override_rule =
"narrowest-period"`, off for NET (TNDS's own documents are one service per
code, and the existing "a later document of the same `ServiceCode` takes
over" stays). It runs after that existing rule.

**Guard.** A winner with under 25% of its base's journeys on a weekday is
logged with both counts (`transit_override_short_total{network, line}`),
not refused: part closures are real (the tram file above is 41%).

## 4. The network

`crates/poller-transit-schedule/networks/tfl.toml`:

```toml
id = "tfl"
name = "London (TfL)"
mode = "metro"            # display only
timezone = "Europe/London"
adapter = "txc"
nocs = ["LUL", "DLR", "TCL"]   # OperatorCode values
override_rule = "narrowest-period"
inner_zip_prefix = "LULDLRTRAMRIVERCABLE"
replacement_bus_route_names = []
naptan_area = "940"
atco_prefixes = ["9400ZZLU", "9400ZZDL", "9400ZZCR"]
public_stop_id = "stop-area"   # 9400ZZLUBST -> 940GZZLUBST on the wire
stop_name_suffixes = [" Underground Station", " DLR Station", " Tram Stop", " Station"]
situations = none              # status from TfL (§7)

[[lines]]  id = "bakerloo"          name = "Bakerloo"            mode = "tube" txc_line_names = ["Bakerloo"]
…          id = "central", "circle", "district", "hammersmith-city",
             "jubilee", "metropolitan", "northern", "piccadilly",
             "victoria", "waterloo-city"                          mode = "tube"
           id = "dlr"     name = "DLR"           mode = "dlr"  txc_line_names = ["DLR"]
           id = "tram"    name = "London Trams"  mode = "tram" txc_line_names = ["Tram"]
```

- **Line ids are TfL's own** (`hammersmith-city`, `waterloo-city`, `dlr`,
  `tram`), so `tfl-<id>` is the status row (§7) and `/lines/tfl-<id>` the
  status page. The cable car (`IFS Cloud Cable Car`, operator `IFS` in
  some drops, `CAB` service codes) and river services are left out by
  operator.
- **Colours** are DS's own display choices per line name, as for
  Metrolink, not TfL's brand palette; no roundels or TfL logos.
- **Stations** are the platform code less its trailing digits
  (`9400ZZLUBST1` to `9400ZZLUBST`, the existing `station_of`), per
  prefix. Bank's Underground (`9400ZZLUBNK`) and DLR (`9400ZZDLBNK`)
  stations stay two stations, as `tfl_stops` keeps `940GZZLUBNK` and
  `940GZZDLBNK`.
- **Rail links** (`reference-data/transit/tfl-rail-links.csv`): generated
  from `tfl_stop_crs` by a typed script (`scripts/gen-tfl-rail-links.py`,
  read-only against a database), checked in, and checked against
  `crs-tiploc.csv` by the existing unit test. A DB test (`#[ignore]`)
  compares it with `tfl_stop_crs` so drift shows in CI.
- `common::transit::NETWORKS` gains `tfl`.

## 5. Adapter changes

All in `adapter/txc.rs` and `network.rs`, each with a fixture test.

1. **The inner zip.** With `inner_zip_prefix` set, the adapter finds the
   one entry whose name starts with it and ends `.zip`, copies it through
   a `CappedReader` (cap 64 MiB) to the work dir, and reads that. None or
   two matches refuse the feed (`malformed`). Without the option, nested
   zips are skipped as today.
2. **Several prefixes.** `atco_prefix` becomes `atco_prefixes` (a single
   string still accepted); `station_of`, `stop_rows` and the NaPTAN filter
   take the list.
3. **`DepartureDayShift`.** A journey's departure gains `shift × 86,400`
   seconds; the trip's `start_s` may reach 47:59 (the column allows
   172,800). Today the adapter ignores the element and would put a 00:12
   train at the start of the service day.
4. **The override rule** (§3.2) as a per-(line, date) winner table built in
   the first pass; a journey's dates are filtered to those where its
   document wins.
5. **Two passes, bounded memory.** Today every kept document's tree is
   held at once; TfL's 333 MB of XML would be well over a gigabyte as
   trees. Pass 1 reads each document's `Services` head only (code, line
   names, period, profile, `ModificationDateTime`) and drops it; pass 2
   parses one document at a time into journeys and drops its tree. Peak
   memory is one document (43 MB of XML) plus the journeys.
6. **Stop names.** `stop_name_suffixes` already exists. NaPTAN's
   `ShortCommonName` still wins where present.
7. **Caps.** `txcMaxBytes` default 400 MiB covers the 141 MB outer zip;
   the per-entry cap (256 MiB) covers the 43 MB documents.

Parse time is to be measured in P1; a stdlib Python pass over the 333 MB
took about 70 seconds.

## 6. Line modes

`/transit/tfl` groups 13 lines into Underground, DLR and Trams, and the
planner and stop pages label rides by mode. `transit_lines` has no mode.

- **Wire:** `common::transit::TransitLine` gains `mode: Option<String>`
  (`#[serde(default, skip_serializing_if = "Option::is_none")]`), so the
  `transit-timetable-part/1` schema stays readable both ways.
- **Migration:** `ALTER TABLE transit_lines ADD COLUMN mode TEXT` (nullable,
  no default: metadata only) and a `CHECK (mode ~ '^[a-z-]{1,16}$') NOT
  VALID`, validated in the next file.
- **Read:** a line's mode is its own, else the network's.
- The network file's line `mode` is optional; Metrolink and NET send none.

## 7. Status from TfL's line status

The transit spec's status (§10 there) reads `transit_situations` and
`line_status` rows `source = 'transit'`. For `tfl` that would duplicate
TfL's status, which DS already shows on `/lines` and `/status`.

- **A status source per network,** compiled into the api next to
  `PLANNER_NETWORKS`: `Situations` (Metrolink, NET) or
  `TflLineStatus` (`tfl`).
- With `TflLineStatus`, a line's `status` is the `line_status` row
  `tfl-<line>` (`source = 'tfl'`), and the network's state is fresh while
  `ingest_freshness('tfl')` is under 30 minutes old ("unavailable" after,
  as today). `situations` lists are empty; the line page links to
  `/lines/tfl-<line>` for TfL's own text ("See the status for this line").
- No transit `line_status` rows are written for `tfl` (the producer
  refuses `situationsEnabled` for it, as for NET), so nothing is counted
  twice in rollups, pins or notifications. Pins stay on `tfl-<line>`.

## 8. Pages and API

### 8.1 Navigation and `/transit`

- The navigation entry "Trams" becomes **"Trams and metros"**
  (`TRANSIT_DESTINATION`), and `/transit`'s title and description follow.
  The "We don't have live tram times" line moves into each network's page
  (Metrolink and NET keep it; London has live arrivals).
- `/transit` lists networks as today; `London (TfL)` reads "Underground,
  DLR and London Trams".

### 8.2 `/transit/tfl`

- Lines grouped by mode (Underground, DLR, Trams), each with its status.
- **"Also run by TfL":** cards for the Elizabeth line and the six
  Overground lines, linking to `/lines/elizabeth-line` and
  `/lines/overground-<name>` (their timetables and live trains live
  there). Their ids come from `common::TFL_TO_NR_LINE_ID`, served by
  `/public/transit/tfl` as `nationalRailLines`.

### 8.3 A Tube line: frequency, first and last, branches

`/transit/tfl/lines/{line}`:

- **Frequency summary** (new `frequency` block on the line, for a `date`,
  default today): per direction, for each band (early to 07:00, 07:00 to
  10:00, 10:00 to 16:00, 16:00 to 19:00, 19:00 to 22:00, late), the
  shortest and longest gap between departures at the direction's busiest
  station, and the count: "Every 2 to 4 minutes". Computed in SQL from
  the trips of the date; cached 5 minutes.
- **First and last trains** per direction from each terminus.
- **Branches** (`branches`): the line's distinct station sequences that
  carry at least 5% of its trips in a direction, merged when one is a
  prefix of another, named "First – Last" (Northern: "Edgware – Morden via
  Bank", "High Barnet – Battersea Power Station via Charing Cross"…). The
  page shows each branch's stations instead of one merged list; Metrolink
  lines keep one list (they have one branch).
- **The timetable grid** takes `branch` and defaults to one hour for a
  line with more than 20 trips an hour; the station list follows the
  branch. The "full list" stays one click away.

### 8.4 Stops: one identity

- The public stop identity stays `940G…` and `/tfl/stops/{id}`. The api
  serves `tfl` stops with `public_stop_id = stop-area`:
  `/public/transit/tfl/stops/{940G…}` and its `/departures` accept the
  stop-area spelling and map it to `9400…`.
- `/transit/tfl/stops/{id}` (either spelling) redirects (308) to
  `/tfl/stops/{940G…}`.
- `/tfl/stops/{id}` gains **"Timetable"** under "Live arrivals": the next
  scheduled departures by line and direction (the transit board, 20
  rows, `realtime: false`), and first and last trains today. Shown only
  while `tfl` is in `api.transitNetworks` and the stop is in the feed.
- Live arrivals stay first: they are what a traveller wants; the
  timetable answers "is there a train at 23:40?".

### 8.5 The Overground and the Elizabeth line

No ingest. Besides §8.2's cards, `/tfl/stops/{id}` already links a stop's
National Rail station. Nothing else changes: their timetables, live
times and planner legs come from CIF and Darwin.

### 8.6 API summary (all under `/public`, dark until listed)

| Route | Change |
|---|---|
| `/transit/networks` | `tfl` appears when listed |
| `/transit/tfl` | lines carry `mode`; `nationalRailLines` |
| `/transit/{network}/lines/{line}` | `mode`, `branches`, `frequency` (any network) |
| `/transit/{network}/lines/{line}/timetable` | `branch` filter |
| `/transit/tfl/stops/{id}` and `/departures` | accept `940G…` |

`docs/api-changelog.md` gets an entry per phase.

## 9. Stream, budget, Redis, grants and chart

As the runbook's "Adding a network" with `<n> = tfl`:

- **Stream** `ds:ingest:transit-tfl`, timetable only. Parts are capped at
  64 KiB after gzip (`TRANSIT_PART_BYTES`); the content above is about 30
  MB of JSON before gzip (pattern stops about 90 bytes each, trips about
  170), about 3 MB after, so **about 100 to 150 parts** an item. Sized for
  two items a day (a refresh): `entries_per_day: 300`, covering a day.
  That is `(300 + 100) × (64 KiB + 512) × 2 ≈ 53 MB` worst case with its
  dead-letter stream.
- **The ingest budget** is at about 396 MB of 512 MiB today, its alert at
  384 MiB. The new stream does not fit under the alert. P1 measures the
  real part count, sets the MAXLEN from it, and **raises `BUDGET_BYTES` to
  576 MiB** (the Redis `maxmemory` is 1.5 to 2 GB; decision D5 reserved 512
  MB of it), with the alert at 432 MiB. If the measured part count is
  under 60, the stream fits without the raise and it is not made.
- **Redis user** `poller-transit-tfl` (client `pollerTransitTfl`),
  optional, in `redis-users.acl.tpl`, `render-redis-acl.py`, the ACL test
  and docs/redis-acl.md.
- **Values:** `pollerTransitSchedule.networks.tfl` (`enabled`,
  `situationsEnabled` refused, `txcUrl` defaulting to the TfL URL, no
  `txcCredentials`), `ingestWriter.streams.transit-tfl`, the backlog and
  stall maps, `ingestSourceStale` for `transit-tfl-timetable` at 8 days
  (the source is weekly; the daily re-send moves freshness, so 36 hours
  would also hold, but a missed TfL week must not page).
- **Grants:** `transit-tfl-timetable` joins the writer's
  `ingest_freshness` row policy; regenerate `postgres-grants.sql`.
- **NetworkPolicy:** HTTPS to `tfl.gov.uk` only; no FTP.
- **Producer resources:** the parse holds one document plus about 64,000
  journeys and their calls (about 1.6 million call tuples before
  folding, every file's journeys counted once); P1 measures RSS and sets the producer's `tfl` memory request
  and limit from it (the default may need raising to 1 GiB).

## 10. Storage and database load

Per feed version (§2.3): 13 lines, about 400 stations and 800 platforms,
7,617 patterns, 210,021 pattern stops, about 64,000 trips and a few
hundred service-day rows (one per distinct date set).

| Table | Rows | Size with indexes (estimate) |
|---|---|---|
| `transit_pattern_stops` | 210,000 | 40 to 55 MB (about 200 to 250 bytes a row with the primary key and the stop index) |
| `transit_trips` | 64,000 | 15 to 20 MB |
| `transit_patterns` | 7,600 | 1 to 2 MB |
| the rest | small | under 1 MB |

So about **60 to 80 MB per version**; at most three versions coexist
(staging, active, previous), **under 250 MB** at peak, freed when the
previous version is deleted. Metrolink is 1,684 pattern stops for scale.

- **Write load:** a new version only when the content changes, normally
  weekly. The daily re-send of an unchanged feed only moves freshness.
  The horizon slides daily (yesterday to today plus 60), which changes
  the expanded service days, so P1 checks whether that makes every day a
  new version; if it does, the horizon's start is rounded to the source's
  `OperatingPeriod` start, so content changes only with the feed.
- **Read load:** the busy queries are a stop's board (pattern stops by
  stop, then trips by pattern) and a line's timetable window. King's
  Cross is on about 1,500 patterns; P2 measures `stop_departures` and
  the frequency query with `EXPLAIN ANALYZE` on a seeded copy and adds an
  index only if the plan needs it (`transit_trips (network, feed_version,
  line_id, start_s)` is the likely one, `CONCURRENTLY`).

## 11. The planner (P4)

### 11.1 What changes

With `api.tripPlanTransitNetworks` listing `tfl`, the network's trips of
the date become connections like Metrolink's (planning spec §6): real
Tube, DLR and tram times, line changes anywhere, and rail-Tube-rail
journeys through the rail links. `PLANNER_NETWORKS` gains:

```rust
PlannerNetwork { id: "tfl", change_minutes: 3, public_stop_id: PublicStopId::StopArea }
```

- **Nodes** are the network's stations (`9400ZZLUBST`), shown as
  `940GZZLUBST` (the existing `StopArea` spelling).
- **Changes inside a station** use `change_minutes`. **Changes between
  stations of one TfL hub** (Bank Underground and Bank DLR; `tfl_stops.
  hub_code`) get a walk link of 5 minutes, read from `tfl_stops` in the
  graph build.
- **Rail links** from §4's CSV.
- **Ends.** A `940G…` end resolves to its timetabled node when `tfl` is
  in the switch and the stop is in the feed for the date; otherwise it
  falls back to `tfl_access` exactly as today (the planning spec §12.2's
  branch in `routes/trips.rs`). Stops the feed lacks (none expected) and
  dates outside the horizon keep the estimates.
- **`tfl_access` stays** for the fallback and is not deleted.

### 11.2 Budget

About 230,000 connections a weekday, half the rail day's 457,000.

- **Memory.** At about 192 bytes a connection (planning spec §10.1),
  about 44 MB per cached date, 88 MB for the two cached dates, plus the
  transit interchange copy. That is far over the planning spec's 25 MB
  transit budget. P4 measures it with the existing benchmark
  (`DS_RUN_BUDGET_BENCH=1 cargo test --release -p api --test
  transit_planning_budget`), extended with a TfL-sized network
  (230,000 connections, 420 nodes, hub walks).
  - The planned fix is the planning spec §10.3's: intern the codes in
    `Connection` (shared `Arc<str>` or `u32` ids for stops and trips),
    which shrinks the rail day too. P4 does it if the measured cost is
    over **+60 MB RSS for two dates** (the new budget proposed for TfL).
- **Latency.** Each sweep grows by about 50%: `fastest` from 8–47 ms to
  roughly 12–70 ms, `options` from about 550 ms to about 800 ms. Budget:
  `fastest` +60% and `options` +60% at most, measured by the same
  benchmark; `api.tripPlanMaxOptionsSearchSize` is rechecked.
- **Off-switch identity.** With `tfl` not listed, the graph, its cache key
  and every response are byte-identical (the existing golden test).

### 11.3 What stays an estimate

Access walks, change times inside stations, and anything outside the
horizon. Legs say "Scheduled" and carry `realtime: false`; the TfL stop
page's live arrivals are linked from a Tube leg as "Live arrivals".

## 12. Attribution

TfL's credit, "Powered by TfL Open Data", is already in the footer and
on `/attribution` (`lib/tflAttribution.ts`). The network file's
`[attribution]` names TfL's Journey Planner timetables under TfL's open
data terms; `OpenDataAttribution.tsx` gains a "Timetables: Transport for
London" line in the TfL section, shown while `tfl` is served. NaPTAN's
OGL line is unchanged.

## 13. Phases, tests and effort

Each phase is one PR, off by default, in order.

| Phase | Content | Tests | Effort |
|---|---|---|---|
| **P0** | This spec, the errata (§16). | none | done |
| **P1** | Adapter (§5), line modes (§6), the `tfl` network file and wiring (§4, §9), status source (§7), `/transit` and `/transit/tfl` pages with modes and the National Rail cards (§8.1, §8.2), mode-neutral copy, the frequency summary and first and last trains (§8.3, without branches). | TXC fixtures: an inner zip, an override case (base plus a narrower file, a part closure), `DepartureDayShift`, three prefixes; DB tests for line modes and the status source; api tests; vitest; e2e and visual on a seeded `tfl` network. | 7–10 days |
| **P2** | Branches, the branch-aware grid, stop departures on `/tfl/stops/{id}`, the redirect (§8.3, §8.4); query measurements (§10). | api and DB tests for branches and stop-area ids; vitest; e2e on the TfL stop page. | 4–6 days |
| **P3** | DLR and trams: in the same feed from P1, so this phase is their checks and copy (DLR's single line with branches, the tram line), the attribution line, and the runbook. | fixtures for a DLR and a tram document. | 1–2 days |
| **P4** | The planner (§11), behind `api.tripPlanTransitNetworks: [tfl]`, with the benchmark and, if needed, interned connections. | planner unit and DB tests, the budget bench, the golden off-switch test. | 8–15 days |

P3 is small because one feed and one network carry all three modes; P1
already ingests them. If P1 must stay smaller, its network file can list
the Underground only and P3 adds the DLR and tram lines (an unmapped line
refuses the feed, so the unlisted `DLR` and `Tram` names would need the
P1 file to drop their operators instead).

## 14. Deploy changes

For Ranma, in order, after P1 is released (each its own change):

1. **Redis user:** add `poller-transit-tfl-password` (letters and digits)
   to the Redis users SealedSecret, and `redis.acl.clients.pollerTransitTfl:
   true`.
2. **Writer and producer:** `ingestWriter.streams.transit-tfl: apply` and
   `pollerTransitSchedule.networks.tfl.enabled: true`. No credentials.
3. **Check** as the runbook's step 3 with `transit-tfl` (counts: 13 lines,
   about 7,600 patterns and 64,000 trips).
4. **Public:** `api.transitNetworks: [metrolink, tfl]` after a day.
5. **Planner (P4, later):** `api.tripPlanTransitNetworks: [metrolink, tfl]`
   after the measurement.

If P1 raises `BUDGET_BYTES`, Redis's `maxmemory` already covers it
(no value change), but the `DistantSignalIngestStreamMemoryHigh` alert
threshold moves with the code.

## 15. Risks and open questions

| Risk | Likelihood | Effect | Mitigation |
|---|---|---|---|
| An override that is additive after all | low (none in three drops) | missing trains on a line for some days | §3.2's guard logs short winners; the runbook says to compare with TfL's status |
| TfL changes the file names or nesting | low | refused feed, old version keeps serving | `inner_zip_prefix` in the network file; the refusal alert |
| The weekly file stops updating | low | a stale timetable | freshness alert at 8 days |
| Memory of the producer | medium | OOM-killed parse | two-pass read; P1 measures |
| Ingest budget | certain | alert threshold | §9's raise |
| Planner memory | high | api RSS | §11.2's measurement and interned connections |
| Branch naming wrong for loops (Circle) | medium | odd labels | Circle and the Hammersmith & City shown as one branch each |

Open:

- **Night Tube.** Friday and Saturday files carry it (Victoria to 26:57).
  Shown in timetables; the planner uses it like any trip.
- **Cable car and river buses** are left out; adding them later is a
  network-file change.

## 16. Errata to the planning spec

The planning spec's §3.3 measured the BODS London GTFS, which merges
TfL's override files with their base files. Measured again on 2026-10-30
(feed of 2026-10-22):

- **London Trams, Wednesday 2026-11-04: 998 trips**, the base file's 707
  weekday journeys **plus** the 24 October to 5 November part closure's
  292. The real figure is 292 that day and **707 on a normal weekday**
  (13,881 calls, about 13,200 connections, not 17,585).
- **The Underground, Wednesday 2026-11-04: 9,814 trips** includes Central
  twice (2,062 against TfL's 1,046). Without it, about 8,800 trips and
  about 197,600 connections on a normal weekday, not 226,430.
- **DLR:** 1,584 weekday trips is right (no override that day); its
  connections about 19,000.
- §12's London Trams network and §13.2's DLR, if built from BODS, would
  carry the duplicates. They are superseded by this design: London Trams
  and the DLR are lines of the `tfl` network, from TfL's file.

## 17. Implementation notes

### 17.1 P1 (DS-212, PR #91)

- **No rail-links file** for `tfl` (§4): TfL's stops already link their
  National Rail stations (`tfl_stop_crs`), so `/transit/rail-links` lists
  none for `tfl` and station pages keep their TfL links. P4 reads
  `tfl_stop_crs` for the planner's walks instead.
- **Station codes keep three letters** after the prefix (`9400ZZLUHR41`
  is `9400ZZLUHR4`, not `9400ZZLUHR`), and a catch-all `9400ZZ` prefix
  takes the Northern line's `9400ZZBPSUST` and `9400ZZNEUGST` as stations.
  `stop_name_suffixes` has no plain " Station" (it turned "Battersea Power
  Station" into "Battersea Power").
- **The freshness alert** for `transit-tfl-timetable` is 36 h like the
  others (§9 said 8 days): the daily re-send moves it even when TfL's
  file has not changed, so it pages for a stuck producer, not a stale
  source.
- **The short-override guard** of §3.2 (`transit_override_short_total`)
  is not built: the real part closures (the tram file at 41% of its base)
  made it noise.
- **Measured end to end** on TfL's 2026-10-26 export (the producer, Redis,
  the writer and the api on a local database): a 9.5 s parse, 7,849
  patterns, 216,766 calls, 58,383 trips, 417 service-day rows; 61 stream
  entries using 1.1 MB of Redis (the budget allows for 30 MB); 65 MB of
  transit tables per version (47 MB of pattern stops, 16 MB of trips),
  inside §10's estimate. The first activation spent 49 s in one reference
  check, a `NOT EXISTS` the planner ran per trip on unanalysed rows; the
  checks are `EXCEPT`s now. The api: a line with its frequency in
  50 to 105 ms, a stop's board 70 ms, the stop list 1 s (cached an hour).
- **Copy**: the navigation says "Trams and metros"; the National Rail
  lines' section is "Also run by TfL" (the copy check refuses a heading
  with a capital after its first word).

### 17.2 P2

- **Branches** (§8.3) are computed on read from the line's patterns, each
  pattern's stations as a run; a run inside a longer one counts for it,
  under 5% is dropped. On TfL's export: the Northern line 7 and 6
  branches (its Bank and Charing Cross runs told apart by `via`), the
  District 5, the Metropolitan 4, the Circle one each way.
- **A line page** lists a branched direction as closed panels, one per
  branch, each with its stations and a link to its timetable; the
  direction's heading names the branches' ends ("Towards Edgware or High
  Barnet").
- **The timetable** shows an hour at a time on the Underground and the
  DLR (by line mode), three hours on a tram line, and offers the
  direction's branches.
- **A TfL stop's timetable** is a section of `/tfl/stops/{id}` under
  the live arrivals: first and last departures today per line and way,
  then the next ten scheduled departures for `?date=` (default today),
  paged by `?cursor=`. `/transit/tfl/stops/{id}` redirects there (308),
  and every TfL station link points there directly.

### 17.3 P3

- **The DLR and the trams** were in the feed from P1 (one network file,
  their operators `DLR` and `TCL`); P3 adds an adapter test with a DLR
  document, a tram base and part-closure pair, and the cable car and a
  river bus left out by operator; an e2e test of the DLR line page; and
  the runbook's DLR and tram checks. The DLR's branches (Bank to Lewisham,
  Stratford to Canary Wharf and the rest) come from P2 unchanged.

### 17.4 P4

- **`tfl` is a planner network** (`PLANNER_NETWORKS`, `tfl_stops: true`):
  stop-area spelling, rail links from `tfl_stop_crs`, walks between the
  stations of one `tfl_stops.hub_code` (timed as interchange walks from
  their coordinates), and its legs' `serviceMode` from the line's mode
  (`tube`, `dlr`, else `tram`). Notices are every non-good `tfl-<line>`
  status.
- **A `TfL` stop end** resolves to its node only with `transit=true` and
  a timetable for the date (`network_covers`); otherwise `tfl_access`
  plans it, unchanged. So a client that doesn't opt in (DS-MCP) gets
  exactly today's answers.
- **Memory.** Measured with `transit_planning_budget` on TfL-scale
  hand-made data (222,158 connections): with `String` codes, +48 MB a
  date, 96 MB for the two cached dates, over the 60 MB budget. §11.2's
  fix was made, more simply than interning: `schedule_query::Connection`
  holds its three codes as `compact_str::CompactString` (inline up to 24
  bytes), 96 bytes a connection with no allocation, against 192. Now: the
  rail day 64 MB (from 104), TfL +28 MB a date, 56 MB for two (within
  budget). The struct's padding still fits the planner operator filtering
  spec's 2-byte `OperatorId`; `TransitGraph::trip_mode` gives a transit
  trip's line mode for its operator.
- **Latency** (same bench): searches with Metrolink and `TfL` against
  Metrolink alone, `fastest` +8%, `options` -1%.

