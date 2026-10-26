# Transit networks: Nottingham Express Transit (NET)

Design addendum, 2026-10-24. Status: **accepted for build, shipped off**
(DS-160). It extends
[the transit networks design](2026-10-09-transit-networks-design.md)
("the transit spec" below; "§" alone means a section of this document).
The transit spec's D6 parked NET "until after Metrolink" as the
TransXChange adapter's future user, and its §15.2 listed what NET needs:
the adapter, a network file, a stream and budget, an ACL line and values.
Metrolink is built and dark, so NET is next.

Everything here ships off by default, as Metrolink did (transit spec D2).

## Contents

1. [Summary](#1-summary)
2. [Sources and measurements](#2-sources-and-measurements)
3. [Choosing the source](#3-choosing-the-source)
4. [The TransXChange adapter](#4-the-transxchange-adapter)
5. [The TNDS download](#5-the-tnds-download)
6. [The network file](#6-the-network-file)
7. [Stream, budget and Redis](#7-stream-budget-and-redis)
8. [Grants, freshness and alerts](#8-grants-freshness-and-alerts)
9. [Chart](#9-chart)
10. [Attribution](#10-attribution)
11. [Disruptions](#11-disruptions)
12. [Rollout and what the user must do](#12-rollout-and-what-the-user-must-do)
13. [Risks and deferred work](#13-risks-and-deferred-work)

## 1. Summary

- **NET is not in BODS.** Not in the East Midlands GTFS, not in the
  national GTFS, not in the national TransXChange (TXC) archive, measured
  on 2026-10-24 (§2). So the GTFS adapter cannot serve NET, and the
  transit spec's suggestion stands: NET's timetable comes from the
  **Traveline National Dataset (TNDS)**, East Midlands region, in TXC.
- **TNDS needs an account.** It is free and under the Open Government
  Licence (OGL), but the download is FTP with a login that Traveline
  issues on request. So NET needs one secret, the TNDS login, which the
  user registers for and Ranma seals (§5, §12).
- **A TXC adapter** (`TRANSIT_ADAPTER=txc`) reads the TNDS zip, keeps the
  files whose operator has NOC `NEXT`, and produces the same
  `TransitModel` as the GTFS adapter (transit spec §6.2). Everything after
  the adapter is shared.
- **No disruptions.** BODS SIRI-SX has no Nottingham participant (§2.3),
  so NET runs the timetable loop only. Its lines show no status (the api's
  `none` state), not "No reported disruption".
- **The rest follows the runbook's "Adding a network":** network id `net`,
  stream `ds:ingest:transit-net`, Redis user `poller-transit-net`
  (client `pollerTransitNet`), values `pollerTransitSchedule.networks.net`
  and `ingestWriter.streams.transit-net`, freshness source
  `transit-net-timetable`, rail links to Nottingham, Bulwell and
  Hucknall. **No migration**: the tables are generic, and the only grant
  change is a row-policy list.

## 2. Sources and measurements

All figures were measured on 2026-10-24, with every download in the
session scratchpad; nothing was committed.

### 2.1 BODS (keyless)

| What | URL | Size | NET? |
|---|---|---|---|
| GTFS, East Midlands | `https://data.bus-data.dft.gov.uk/timetable/download/gtfs-file/east_midlands/` | 253 MB zip, 1.68 GB unzipped (`shapes.txt` 1.27 GB, `stop_times.txt` 395 MB); `feed_version=20261022_034732` | **No.** 71 agencies, none with NOC `NEXT`; 1,145 routes, all `route_type` 3 (bus) or 200 (coach); no stop with a `9400ZZNO` code |
| GTFS, national | `…/timetable/download/gtfs-file/all/` | 2.1 GB zip (no range requests) | **No.** Its tram agencies are South Yorkshire Future Tram (`SYFT`), London Tramlink (`TRAM`), Edinburgh Trams (`EDTR`) and Metrolink (`METL`); no `9400ZZNO` stop; the Nottingham names in `stops.txt` (Hucknall Road, Toton Lane) are bus stops |
| TXC, national | `…/timetable/download/bulk_archive` (302 to a signed URL) | 2.25 GB zip, 907 entries | **No.** The Nottingham publishers are Nottingham City Transport, Nottinghamshire County Council Fleet Services, Notts and Derby, Nottingham Minibuses and Sharpes; NCT's 7 MB zip has no `9400ZZNO` stop |
| SIRI-SX bulk | `…/disruptions/download/bulk_archive` | 330 KB zip, 6.5 MB XML | **No** (§2.3) |

An older note elsewhere says BODS carries NET. It did once; it does not
now. The BODS mandate covers local bus services, and NET's operator does
not publish to it.

### 2.2 TNDS and the NOC database

| What | Where | Access | Notes |
|---|---|---|---|
| TNDS East Midlands | FTP, `ftp.tnds.co.uk`, one zip per region (`EM.zip`) | **a free login on request** (Traveline's TNDS page has the form); port 21 did not answer anonymously from the build host | TXC XML; "bus, light rail, tram and ferry"; updated Monday to Thursday overnight; OGL (data.gov.uk's TNDS entry) |
| NOC database | `https://www.travelinedata.org.uk/noc/api/1.0/nocrecords.xml` | none | 8.8 MB XML. **NET is NOC `NEXT`**, "Nottingham Express Transit (Tram)", region EM, mode Tram (a second, older record says Bus) |

TNDS was not downloaded: it needs a login, and this work downloads only
keyless data. So the NET counts below are from NaPTAN and the published
network, not from TNDS, and the adapter's tests use hand-made fixtures
(§4.6). The first real TNDS file is checked in rollout step 3 (§12).

### 2.3 Disruptions

The SIRI-SX archive held 540 situations from twelve participants:
BedfordshireLuton, CheshireEast, Cornwall, Devon, Hampshire, Merseytravel,
NorthLincolnshire, SYMCA, TelfordWrekin, TfGM, WYCA and WestofEngland.
**None is Nottingham or Nottinghamshire**, and no situation names a
`9400ZZNO` stop. There are no NET `LineRef`s to map.

### 2.4 NaPTAN (keyless, OGL)

`https://naptan.api.dft.gov.uk/v1/access-nodes?atcoAreaCodes=940&dataFormat=csv`
(the same file as Metrolink's; 618 KB) has 155 `9400ZZNO` rows:

| | Active | Inactive |
|---|---|---|
| `MET` (stations) | 50 | 1 (Station Street, replaced by Nottingham Station) |
| `PLT` (platforms) | 97 | 7 |

- **Platform to station is a suffix rule.** Every NET platform code is its
  station's code plus one digit (`9400ZZNOBCF1` → `9400ZZNOBCF`); all
  104 `PLT` rows check out. The TXC adapter uses this rule (§4.3).
- **Names need cleaning.** `ShortCommonName` is empty, and `CommonName`
  ends in ` Tram Stop` (`Beaconsfield Street Tram Stop`). Indicators read
  `NET NE-bound`. The network file strips both (§6).
- **Rail interchanges** (CRS and TIPLOC from
  `reference-data/crs-tiploc.csv`):

  | Stop | ATCO | CRS | TIPLOC |
  |---|---|---|---|
  | Nottingham Station | `9400ZZNONOT` | NOT | NTNG |
  | Bulwell | `9400ZZNOBWL` | BLW | BULWELL |
  | Hucknall | `9400ZZNOHKL` | HKN | HUCKNAL |

  All three are `interchange` (the tram stop is at the station). Beeston
  Centre is not beside Beeston station, so it gets no row.

### 2.5 The network

Two lines, from the operator's published network: **Line 1** Hucknall to
Toton Lane and **Line 2** Phoenix Park to Clifton South, sharing the city
centre. 50 stations, as NaPTAN says.

## 3. Choosing the source

**Chosen: TNDS East Midlands TXC, filtered to NOC `NEXT`, over FTP with
a Traveline login.**

| | TNDS EM (TXC) | BODS (any form) | Scraping or operator APIs |
|---|---|---|---|
| Has NET | yes (TNDS covers tram; NOC `NEXT` is region EM) | **no** (§2.1) | – |
| Licence | OGL v3 | OGL v3 | **off-limits** (transit spec D5) |
| Access | free login, FTP | keyless HTTPS | – |
| Format | TXC (an adapter to build) | GTFS (built) | – |
| Freshness | Monday to Thursday overnight | nightly | – |

Only TNDS has NET under an open licence. The cost is an adapter and a
secret. The adapter is generic: any future TXC network reuses it, and a
BODS TXC URL works with it unchanged (HTTPS, no login).

**If NET appears in BODS later** (as GTFS under NOC `NEXT`), the switch is
config only: `adapter: gtfs`, a `gtfsUrl`, the line names as
`gtfs_route_names`, and the TNDS secret dropped. The tables and the
stream do not change.

## 4. The TransXChange adapter

`crates/poller-transit-schedule/src/adapter/txc.rs`, chosen by
`TRANSIT_ADAPTER=txc`. It reads the downloaded zip like the GTFS
adapter: entry by entry through a `CappedReader`, never holding a whole
entry.

### 4.1 Which files

TNDS's regional zip holds one TXC document per service. The adapter
stream-parses every `.xml` entry with `quick-xml` (in the lockfile) into
a small per-document structure, and keeps a document only if one of its
`Operators/Operator` has a `NationalOperatorCode` (or `OperatorCode`) in
the network file's `nocs`. Mode tags are not trusted (TNDS has tagged NET
as a bus). Nested zips are skipped and counted in the log.

### 4.2 What it reads

The subset of TXC 2.1 and 2.4 a timetable needs:

- `StopPoints/AnnotatedStopPointRef` (`StopPointRef`, `CommonName`);
- `JourneyPatternSections/JourneyPatternSection/JourneyPatternTimingLink`:
  `From` and `To` (`StopPointRef`, `Activity`, `WaitTime`) and `RunTime`;
- `Services/Service`: `ServiceCode`, `Lines/Line` (`@id`, `LineName`),
  `OperatingPeriod`, `OperatingProfile`, and
  `StandardService/JourneyPattern` (`@id`, `Direction`,
  `DestinationDisplay`, `JourneyPatternSectionRefs`, `OperatingProfile`);
- `VehicleJourneys/VehicleJourney`: `VehicleJourneyCode`, `ServiceRef`,
  `LineRef`, `JourneyPatternRef`, `DepartureTime`, `DestinationDisplay`,
  `OperatingProfile`, and `VehicleJourneyTimingLink` overrides of a
  link's `RunTime` and wait times.

`RouteSections`, `Routes`, tracks and `ServicedOrganisations` are not
read.

### 4.3 Mapping to the model

- **Lines.** A journey's `LineRef` names a `Line`, whose `LineName` maps
  to a DS line through the network file's `txc_line_names`. An unmapped
  name refuses the feed (`unmapped_route`), as in GTFS: a new line is a
  config change, never a guess.
- **Stops.** A call is at its `StopPointRef`, a platform. Its station is
  the platform code minus its trailing digits when that has the network's
  `atco_prefix` (§2.4); NaPTAN then names both, as for Metrolink. A stop
  outside the prefix (a replacement bus) has no station.
- **Times.** The first stop departs at `DepartureTime`. Each link adds its
  `RunTime`; a stop's dwell is the previous link's `To/WaitTime` plus the
  next link's `From/WaitTime`. A journey's overrides win. Offsets are
  relative to the first departure, as GTFS patterns are, and a
  `DepartureTime` after midnight on a journey that started before it
  stays above 24:00 only through offsets (the trip's start is under 24 h).
- **Activity.** `pickUp` is pickup 0 and drop-off 1; `setDown` the
  reverse; `pass` is not a call; `pickUpAndSetDown` (the default) is 0 and
  0. The first call takes no drop-off and the last no pickup, as BODS's
  GTFS conversion does.
- **Direction.** `inbound` and `antiClockwise` are inbound; anything else
  is outbound.
- **Headsign.** The journey's `DestinationDisplay`, else the journey
  pattern's, else the last stop's name.
- **Trip ids** are `<ServiceCode>:<VehicleJourneyCode>`, unique across
  files.

### 4.4 Service days

Each journey's operating profile is the journey's own, else its journey
pattern's, else its service's. The adapter expands it to explicit dates
from yesterday to today plus the horizon, inside the service's
`OperatingPeriod`:

1. `RegularDayType/DaysOfWeek`: the named days (`Monday` … `Sunday` and
   TXC's groups `MondayToFriday`, `MondayToSaturday`, `MondayToSunday`,
   `Weekend`, `NotMonday` … `NotSunday`). `HolidaysOnly` runs on no
   regular day.
2. `BankHolidayOperation`: a holiday in `DaysOfNonOperation` is removed;
   one in `DaysOfOperation` is added. England and Wales holidays are
   computed (fixed dates with weekend substitutes, Easter by the Gregorian
   computus, the May and August Mondays), with TXC's names: `NewYearsDay`,
   `GoodFriday`, `EasterMonday`, `MayDay`, `SpringBank`,
   `LateSummerBankHolidayNotScotland`, `ChristmasDay`, `BoxingDay`, the
   `…Holiday` displacement days, `ChristmasEve`, `NewYearsEve`, and the
   groups `AllBankHolidays`, `Christmas`, `EarlyRunOff`,
   `HolidayMondays`, `AllHolidaysExceptChristmas`. One-off holidays
   (a coronation) are not known; such a day runs as its weekday.
3. `SpecialDaysOperation`: `DaysOfOperation` date ranges are added and
   `DaysOfNonOperation` ranges removed, last.

Journeys with the same date set share a service id (`s1`, `s2`, …,
numbered in date-set order so the ids are stable). A journey with no date
in the horizon is dropped.

### 4.5 Guards

- The feed guard (transit spec §6.2 step 11) applies unchanged.
- No document of the network: `malformed` ("no TXC document with NOC
  NEXT"), so a TNDS file without NET refuses instead of emptying NET.
- A journey pattern or link that a journey names but its document lacks,
  an unparseable time or duration: `malformed`.
- Caps: the download (`TRANSIT_TXC_MAX_BYTES`, 400 MiB), each entry's
  inflated size, and the parse runs on the blocking pool.

### 4.6 Tests

Hand-made fixtures only, in `crates/poller-transit-schedule/tests/fixtures/`:
a small TXC document in TNDS's shape for NET (two lines, wait times, an
operating profile with bank holidays and special days, a journey-level
run-time override) and one of another operator, zipped in the test. The
parse tests cover the operator filter, times and offsets, activities,
direction, service-day expansion and refusals. A `#[ignore]`d test reads a
real TNDS `EM.zip` from `TRANSIT_REAL_TXC`, as the GTFS one reads
`TRANSIT_REAL_GTFS`.

## 5. The TNDS download

`TRANSIT_TXC_URL` is either `https://…` (a BODS-style keyless zip) or
`ftp://ftp.tnds.co.uk/EM.zip`. For `ftp://`, the producer logs in with
`TRANSIT_TXC_USERNAME` and `TRANSIT_TXC_PASSWORD`, read from a Secret
(`pollerTransitSchedule.networks.net.txcCredentials.existingSecret`, keys
`username` and `password`), and fetches the file in binary passive mode
(`EPSV`, falling back to `PASV`), under the same byte cap and timeout as
an HTTPS download. The client is a small one in the crate (`ftp.rs`), so
no dependency is added; it is tested against an in-process server.

- **Plain FTP sends the login in clear.** TNDS offers FTP; the login is a
  free registration that guards nothing else. The runbook says so, and
  the Secret holds nothing reused.
- **NetworkPolicy.** Passive FTP opens a data connection to a port the
  server picks. `distant-signal.urlPort` learns `ftp` (21); when
  `networkPolicy.egress.internetPorts` limits ports, the producer
  component's internet rule must allow every port
  (`networkPolicy.components.poller-transit-schedule.internetPorts: []`),
  which the runbook says. With the chart's default (no port limit)
  nothing changes.
- The chart renders the two env vars only when `txcCredentials.existingSecret`
  is set, and refuses an enabled `ftp://` network without it.

## 6. The network file

`crates/poller-transit-schedule/networks/net.toml`:

```toml
id = "net"
name = "Nottingham Express Transit"
mode = "tram"
timezone = "Europe/London"
adapter = "txc"
nocs = ["NEXT"]
replacement_bus_route_names = []
naptan_area = "940"
atco_prefix = "9400ZZNO"
stop_name_suffixes = [" Tram Stop"]
indicator_prefixes = ["NET "]

[attribution]
licence = "OGL-UK-3.0"
text = "Contains public sector information licensed under the Open Government Licence v3.0. Timetables: Traveline National Dataset (Traveline Information Ltd). Stops: NaPTAN (Department for Transport)."

[situations]          # none published today (§2.3); the loop stays off
participant_refs = []
vehicle_modes = ["tram"]
operator_refs = ["NEXT"]

[[lines]]
id = "1"              # public id net:1
name = "Line 1"
txc_line_names = ["1", "L1", "Line 1"]
siri_line_refs = ["1", "L1", "NET1"]
colour = "#…"

[[lines]]
id = "2"
…
```

New fields, all with defaults that leave `metrolink.toml` unchanged:

- `adapter` (`gtfs` by default): the producer refuses a
  `TRANSIT_ADAPTER` that differs, so a values typo cannot feed NET's file
  to the GTFS parser.
- `txc_line_names` per line: required for a `txc` network, as
  `gtfs_route_names` is for a `gtfs` one.
- `stop_name_suffixes` and `indicator_prefixes`: stripped from NaPTAN and
  source names (the GTFS ` (Manchester Metrolink)` rule stays).
- **The situations loop refuses to start** for a network with no
  `participant_refs`: it would publish an empty set every 5 minutes, and
  NET's lines would read "No reported disruption" without any source.

**`txc_line_names` and `siri_line_refs` are not observed values.** TNDS
was not downloaded (§2.2) and no NET situation exists (§2.3). The aliases
are the likely spellings of the two line names. A wrong guess refuses the
first feed with `unmapped_route` naming the real `LineName` (the
`DistantSignalTransitFeedRefused` alert); the fix is one line in the file
and a release.

## 7. Stream, budget and Redis

- `ingest_stream::streams::TRANSIT_NET = "ds:ingest:transit-net"`; the
  writer's `stream::STREAMS` gains `transit-net` with the three transit
  schemas (so turning situations on later needs no writer change).
- **Budget.** NET has no situations, so its stream carries one timetable
  item a day. NET is small: two lines, about 50 stations, a few thousand
  journeys a week, so an item is well under ten 64 KiB parts. The
  declaration is 24 entries a day covering a day (`MAXLEN` 24), and
  worst case (24 + 100) × (64 KiB + 512 B) ≈ 8.2 MB, twice with its
  dead-letter stream: **about 16 MB**. Metrolink's 288 + 32 would cost
  55 MB and push the total past the 384 MiB alert line; NET does not need
  it. Turning NET's situations on later means resizing this declaration
  (and the alert maps) first.
- **Redis user** `poller-transit-net`, kind `optional`, with Metrolink's
  rights on its own stream:
  `~ds:ingest:transit-net +xadd +xrevrange` plus the handshake. Client
  `pollerTransitNet` (default off), Secret key
  `poller-transit-net-password`.

## 8. Grants, freshness and alerts

- `db-grants.yaml`: `row_policies.ingest_freshness.writer` gains
  `transit-net-timetable` and `transit-net-situations` (the second for
  when situations exist; listing it costs nothing).
  `postgres-grants.sql` is regenerated. **No migration**, no new table.
- `ingestSourceStale.maxAgeSeconds`: `transit-net-timetable` at 36 h only.
  TNDS updates Monday to Thursday, but the producer re-sends an unchanged
  feed daily, which moves the freshness, so 36 h holds. No situations
  entry: it would fire for ever.
- The per-stream backlog and stall maps gain `ds:ingest:transit-net`
  (backlog 24, so warning 12 and critical 19; stall at three timetable
  intervals, 259,200 s).

## 9. Chart

```yaml
pollerTransitSchedule:
  networks:
    net:
      enabled: false
      adapter: txc
      txcUrl: "ftp://ftp.tnds.co.uk/EM.zip"
      txcMaxBytes: 419430400
      txcCredentials:
        existingSecret: ""     # keys username, password
      situationsEnabled: false
      …                        # the rest as metrolink
ingestWriter:
  streams:
    transit-net: "off"
redis:
  acl:
    clients:
      pollerTransitNet: false
```

`gtfsUrl` and `gtfsMaxBytes` stay on Metrolink only; the template sets
`TRANSIT_TXC_*` from the network's values and leaves the defaults
otherwise. Render checks: CI's helm-lint renders NET on (with its
stream, client and Secret) and asserts the Deployment, its env and its
secret refs, and that the chart refuses NET on with its stream off, or
with an `ftp://` URL and no credentials Secret.

## 10. Attribution

`OpenDataAttribution.tsx`'s `TRANSIT_DATA_SOURCES` gains NET: the
Traveline National Dataset under the OGL v3.0 and NaPTAN, and the
non-affiliation line ("Distant Signal is not affiliated with Nottingham
Express Transit."). They show only while `api.transitNetworks` lists
`net`. `/public/transit/networks` carries the network file's attribution
text, as for Metrolink. The api changelog records the new network id.

## 11. Disruptions

None for now (§2.3). The network file keeps a `[situations]` table so a
future source needs no schema change, with `operator_refs = ["NEXT"]`
and aliases for the two lines. If a Nottingham participant appears in
BODS SIRI-SX: add it to `participant_refs`, resize the stream's budget
(§7), add the situations staleness entry, then turn `situationsEnabled`
on.

## 12. Rollout and what the user must do

Before anything:

1. **The user registers for TNDS** on Traveline's TNDS page (free; a
   named contact, as transit spec Q5 did for BODS). Traveline emails the
   FTP login after a few working days.
2. **Ranma seals a Secret** in the DS namespace, say
   `distant-signal-tnds`, with keys `username` and `password`, byte-exact
   (`--from-file`), and adds `poller-transit-net-password` to the Redis
   users Secret.

Then, in Ranma-Config, each step its own change (docs/transit-networks.md,
"Rollout", with NET's names):

1. `redis.acl.clients.pollerTransitNet: true` (Redis restarts once).
2. `pollerTransitSchedule.networks.net.txcCredentials.existingSecret:
   distant-signal-tnds`, `pollerTransitSchedule.networks.net.enabled:
   true` and `ingestWriter.streams.transit-net: apply` together.
3. Check the first commit: 2 lines, about 50 stations and 100 platforms,
   a horizon of 60 days, and no `unmapped_route` refusal. A refusal names
   the real `LineName`: add it to `txc_line_names` and release.
4. After a day: `api.transitNetworks: [metrolink, net]` (or `[net]`).

Rollback is Metrolink's, with NET's names.

## 13. Risks and deferred work

| Risk | Likelihood | Mitigation |
|---|---|---|
| TNDS's `LineName`s differ from the guessed aliases | medium | `unmapped_route` names the real one; a one-line fix |
| TNDS's NET file uses TXC features the adapter skips (serviced organisations, frequency-based journeys) | low–medium | journeys still parse; frequency journeys are counted and dropped; the real-file `#[ignore]` test on the first download |
| The TNDS FTP host refuses or changes | low | the timetable stale alert; the previous version keeps serving |
| TNDS ends (it is "being superseded by BODS") | medium | if NET moves to BODS, switch to `adapter: gtfs` (§3) |
| A one-off bank holiday | low | that day runs as its weekday |

Deferred: NET disruptions (no source), live tram times (no open source),
TNDS over FTPS (TNDS offers FTP), and frequency-based TXC journeys
(`FrequentService`), which NET is not known to use.
