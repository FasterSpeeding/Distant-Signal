# Dynamic Trip Planning — Design Investigation

**Status: design investigation with a finalized v1 scope, not yet an
approved implementation plan.** This document answers a single question
honestly: given a start station, a finish station, and optionally some
waypoints, can this app work out an actual multi-hop *route* across the
network — which line(s)/train(s) to take and where to change — instead of
requiring the user to already know every interchange? No code was written
to produce this document. Citations are file:line against `main`;
anything not directly verified against this app's own code is flagged
**Speculative**.

**v1 scope, confirmed by the product owner 2026-09-22 (not re-litigated
below):**

- **GB National Rail only.** No London Underground/DLR/tram, no Northern
  Ireland, no Republic of Ireland.
- **Walking transfers between differently-named stations ARE required.**
  Same-CRS interchange alone is not acceptable; the planner must be able to
  route via e.g. a cross-London walk between two different CRS codes.
- **Multi-criteria ranking IS required**, not deferred: a single
  `results: 'fastest'` earliest-arrival answer AND a `results: 'options'`
  Pareto set trading arrival time against interchange count.

These three reverse two of this document's own original recommendations
(walking transfers and multi-criteria ranking were both originally proposed
as later-phase cuts) and confirm the third (network scope) as originally
recommended. The reasoning for why the original, narrower recommendations
made sense — and why it changed — is preserved inline below (§1, §4) where
a future reader would otherwise wonder why v1 builds two search algorithms
and two interchange data sources instead of the simpler thing this
document first proposed. Everything else in the original narrower scope
(single service day, ≤2 interchanges, no live-disruption awareness, no
accessibility/fare data) is unchanged and still current.

A sister project in this organisation —
`Distant-Signal-MCP`, an MCP server over the same National Rail CIF data
this app ingests — has **already built and shipped** exactly this feature
(`plan_journey`), and its design and code are cited throughout this
document as real, working prior art on the identical problem against the
identical data source. **Provenance note on those citations**: this
document's original investigation cloned that repository and read
`src/tools/plan-journey.ts` and `src/timetable/plan/{csa,raptor,connections,
interchange,constraints}.ts` and `src/timetable/cif/alf.ts` directly. This
rewrite attempted to re-read those files to describe the algorithm shapes
in more depth, but the clone no longer exists on disk at the path it was
checked out to — it was not re-fetched for this pass. Every sibling-project
claim below is therefore carried forward from the original investigation's
own file:line citations and direct quotes (which are specific enough to be
real evidence, not vague gesturing at "a sibling project has this"), not
independently re-verified against the source a second time. This app's
*own* code was re-verified directly this pass (see §0.2, §0.4, §0.8 below).

---

## 0. What already exists (baseline)

### 0.1 The line catalogue (`lines/*.toml`) — a curated display list, not a routing graph

**243 files today** (`ls lines/*.toml | wc -l`), collectively naming **284
distinct TIPLOCs** (`grep -h "tiploc = " lines/*.toml | sort -u | wc -l`) —
a small, curated, *documentation-oriented* subset of the roughly 2,500-row
`stations` reference table
(`docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md:20`).
Per `lines/SCHEMA.md`, each file is: `id`, `name`, `mode` (**always
`"national-rail"`**, confirmed by `grep -h "^mode = " lines/*.toml | sort -u`
returning exactly one value across all 243 files — see §0.6 below for why
this matters), `category`, `operators` (ATOC codes), an **ordered list of
`stations`** (CRS + optional `tiploc`/`role`/`segment`), plus matching
metadata (`match_keywords`, `sample_stations`, `destination_crs_filter`,
etc.) that exists to drive **incident-to-line text matching**
(`crates/common/src/matcher.rs`), not journey planning.

Concretely, per `lines/SCHEMA.md`'s own field table and `gwr-main-line.toml`
as a worked example: a station object is `{crs, tiploc?, role?, segment?}`
— **no scheduled times, no calling-point ordering beyond the file's own
station-array order (which is geographic, not a per-train stopping
pattern), and no cross-line linkage beyond a station's CRS appearing in
more than one file's list.** `SCHEMA.md`'s own note is explicit that
`tiploc` here is "purely documentation/display metadata... not required for
correctness" (`SCHEMA.md:35-44`) — schedule matching resolves real TIPLOCs
from `stanox_crs` at runtime instead (§0.2), independent of this file.

**What this catalogue *can* give a trip planner, honestly stated**: a
coarse, curated "which named lines run through station X" index — useful
as a human-facing display grouping and possibly as a *candidate-narrowing
hint* (e.g., "these are the lines this app already knows to be relevant
near Reading") — but it is **not** itself a queryable "which lines call at
which stations, in what order, at what time" graph. It has no times, its
station-order isn't a real train's individual stopping pattern (an express
service skips minor stations the file still lists), and it covers only 284
of ~2,500 stations. **Building a trip planner on top of this catalogue
alone would be building on curated display metadata, not real timetable
data.** The real timetable data lives one layer down, in §0.2.

### 0.2 `crates/schedule-query` + `crates/schedule-reference` — the actual, whole-network, time-resolved calling-point data, already running in production

This is the single most important finding of this investigation, and it
changes the shape of the whole problem: **this codebase already has, in
production, an in-memory data structure that is functionally a GTFS-style
"trips + ordered stop_times" table for the entire real GB National Rail
network, resolved for a specific service day** — exactly the raw input a
real journey-planning algorithm (§1) needs. It is not persisted in that
shape and it is not currently used for anything beyond single-station
departure lookups, but the hard, expensive part of producing it — CIF
parsing plus STP-overlay resolution at national scale — is a **sunk cost,
already paid, already running correctly every 30 minutes**
(`crates/schedule-reference/src/main.rs`'s `poll_once`, default
`poll_interval_secs` 1800s).

Concretely:

- `schedule_query::ScheduleIndex::from_text` (`schedule-query/src/resolve.rs:445-447`)
  parses the whole national CIF `SCHEDULE` feed into
  `by_uid: HashMap<UID, Vec<RawSchedule>>` — every train UID's every STP
  variant (Permanent/Overlay/New/Cancellation), independently confirmed
  against real production data at **463,947 real `BS` records, 234,941
  distinct UIDs, zero parse errors**
  (`docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md:56-64`).
- `ScheduleIndex::schedule_for_uid(uid, date)` → `resolve_for_date`
  (`resolve.rs:97-128`) resolves **one train's one calendar day** to a
  single winning STP variant (`min_by_key` over `StpIndicator`'s
  precedence-ordered `Ord` — `C` beats `N` beats `O` beats `P`), and, for a
  non-cancelled result, returns `calling_points: Vec<CallingPoint>` — an
  **ordered** sequence, each with `tiploc`, `kind`
  (`Origin`/`Intermediate`/`Terminate`), `booked_arrival`/`booked_departure`,
  and (critically for any multi-hop search that must reason about
  midnight-crossing services) a computed `day_offset`
  (`assign_day_offsets`, `resolve.rs:61-78`, tested against a real
  live-confirmed overnight Liverpool Street→Shenfield working crossing
  midnight, `resolve.rs:696-726`). **This is, field-for-field, a GTFS
  `trip` + its ordered `stop_times`, minus a persisted schema** — the raw
  shape a Connection Scan Algorithm or RAPTOR needs (§1) is already sitting
  in memory, once per 30-minute cycle, for the whole network.
- Two already-built, already-tested whole-network grouping passes prove
  this is cheap enough to compute transiently, not hypothetically: `for uid
  in index.uids() { schedule_for_uid(...) }` is a single O(all UIDs) pass —
  `schedules_touching` (`resolve.rs:135-154`, the existing per-line
  population publish), `departures_by_crs` (`resolve.rs:243-286`, backs
  `GET /public/stations/{crs}/schedule-departures`) and
  `departures_by_destination_crs` (`resolve.rs:333-420`, backs the
  whole-network train-search-by-destination feature) all do this exact
  pass, each producing a different bucketing of the *same* underlying
  resolved network. **Nothing about this data structure is currently kept
  resident** — `schedule-reference` builds it, uses it, and drops it every
  cycle (confirmed directly: the crate opens no HTTP server, only a
  metrics port and outbound `reqwest` calls — no persistent whole-network
  index anywhere in this codebase today, by deliberate, twice-documented
  design choice, see §3).
- The TIPLOC→CRS bridge is a **second, independent, also-whole-network**
  output of the *same* poll cycle, and its own ingestion pipeline is worth
  describing precisely because it's the exact mechanism a walking-transfers
  feature needs to extend (§0.4, §8 Phase 1). Re-verified directly against
  `crates/schedule-reference/src/{parser.rs,discovery.rs,main.rs}` this
  pass:
  - `discovery::latest_complete_delivery` (`discovery.rs:38-72`) finds the
    most recent delivery directory that has **both** a `RJTTF*MCA.txt` and
    a `RJTTF*MSN.txt` file directly inside it (matched by filename
    prefix/suffix, not a reconstructed sequence number) — a delivery
    missing either file is skipped entirely, falling back to the
    next-most-recent complete one.
  - `main.rs`'s `poll_once` (`main.rs:101-131`) then reads **`TI`-prefixed
    lines from the `MCA` file** and **`A`-prefixed lines from the `MSN`
    file** (`read_prefixed_lines(&delivery.mca_path, "TI")` /
    `read_prefixed_lines(&delivery.msn_path, "A")`, `main.rs:119-120`) —
    **correcting an imprecision in this document's own prior draft**,
    which described the TI/TIPLOC/CRS records it discusses as coming from
    the MSN file; they in fact come from the MCA file (`parser.rs:17`,
    "One parsed `TI`... record from a CIF `MCA` file"). The MSN file's own
    contribution is its `A` records, and today only two of their fields are
    parsed at all: `parse_msn_a_lines` (`parser.rs:76-93`) reads only bytes
    `36..43` (TIPLOC) and `49..52` (CRS) off each `A` line, purely to
    backfill a `TI` record's blank CRS field (the WATRLMN case) — every
    byte of the `A` line past offset 52 is read into memory (`a_text` is
    the *whole* matching line, not a truncated one) but never parsed into
    any structured field today.
  - **This matters directly for interchange-time data (§0.4, §4).** The
    sibling `Distant-Signal-MCP` project's own verified extraction of a
    real MSN `A` record found a minimum-interchange-time value at column
    65 (range 0–9 minutes, with 98/99 sentinel values meaning "not a real
    rail interchange, a bus/coach stand"). Since this app's own `a_text`
    already holds that byte range in memory once per 30-minute cycle —
    it's simply never read past byte 52 — **surfacing it needs no new file
    discovery and no new I/O of any kind**, only a new field on
    `parse_msn_a_lines`' output (or a sibling parse function reading the
    same already-fetched `a_text`). This is a smaller change than even the
    original addendum framed it as: not "parse a new part of an
    already-fetched file" but "parse a few more bytes of a line this
    process already holds as a `String` in memory."
  - The resolved STANOX→CRS table (`common::StanoxCrsRecord`
    `{stanox, crs, tiploc, station_name, source_sequence}`) is a real
    **~3,100-row table**, fully rewritten every cycle
    (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:521`,
    `queries::upsert_stanox_crs`, posted from `schedule-reference` to
    `api`'s ingest route — `crates/api/src/routes/ingest.rs:365-370`).
    Combined with `ScheduleIndex`, this gives — for the whole GB National
    Rail network, for one calendar day, rebuilt every 30 minutes — every
    train's ordered `(CRS, scheduled arrival, scheduled departure)`
    sequence. **This is not a future cost to build; it is a byproduct this
    app already produces and currently only projects down into narrower
    per-station/per-line views.**
  - **The `ALF` file (CIF's fixed-links member, needed for walking
    transfers between *different* CRS codes) is not part of this pipeline
    at all today.** `discovery.rs`'s `CompleteDelivery` struct has exactly
    two path fields, `mca_path` and `msn_path` — no third. A repo-wide
    grep for `ALF` against `crates/` (this pass) finds zero references to
    the CIF file member anywhere. This is genuinely new ingestion work:
    extending `CompleteDelivery` to also require and locate a
    `RJTTF*ALF.txt` file, and a new parser module (mirroring `parser.rs`'s
    shape) for its fixed-link records. Per the sibling project's own
    verified count: 4,222 links, 1,772 metro, 1,600 tube, 557 transfer,
    237 walk, 50 bus, 4 tram, 2 ferry, each with its own validity window
    (e.g. Euston↔King's Cross is 5–10 min by tube but 15 by transfer
    depending on time of day) — a genuinely richer record shape than the
    STANOX/CRS table this pipeline already produces, not a trivial
    addition, but of an already-standardised CIF file this codebase's
    existing delivery-discovery mechanism needs only a structural
    extension, not a reinvention, to pick up.
- **What it genuinely lacks**, confirmed by reading `records.rs`/`parse.rs`
  in full (also independently reconfirmed by
  `2026-09-04-whole-network-trip-search-research.md:87-121`): no operator,
  no headcode, no CRS on `CallingPoint` itself (only TIPLOC — the
  `stanox_crs` join supplies CRS), no `CR`/`AA`/freight-specific record
  decoding. None of these block route-finding itself (a route only needs
  stop, time, and a trip identity to group by), though "no operator" means
  a computed itinerary can't show which TOC operates each leg without a
  second join the existing pin-creation flow already knows how to do
  (`create_subscription_for_train`, §0.6).

### 0.3 `search_schedule_calling_point_departures`/`search_journey_leg_candidates` — real, reusable, but fundamentally single-hop

Both live in `crates/api/src/data/queries.rs`, querying a Postgres-persisted
subset of §0.2's data (`schedule_destination_departures`, populated by
`schedule-reference`'s publish loop) rather than an in-memory
`ScheduleIndex` directly:

- `search_schedule_calling_point_departures` (`queries.rs`, ~1446-1562):
  anchored on **one fixed `station_crs`** (`origin_crs = station_crs`,
  always equality), `scheduled_from`/`to_time` bounding that station's own
  departure time, and an optional `stops_at` (verified via a correlated
  `EXISTS`) that **also enforces real downstream ordering** — the
  function's own extensive doc comment (`queries.rs:1297-1434`) documents,
  in detail, a same-schedule case where `stops_at` names the *searched*
  station itself (a loop/reversing service), and the exact `ORDER BY
  day_offset, scheduled` (not a bare `ORDER BY scheduled`) needed to keep
  overnight schedules correctly ordered (`queries.rs:1223,1241`). This is
  careful, well-tested single-station-outbound search — not a
  multi-station path query.
- `search_journey_leg_candidates` (`queries.rs`, ~1654-1763) is the
  sibling used by the shipped journey-tracking feature's window-search leg
  (§0.8) — same shape, no `true_origin_crs`/`stops_at` params, built to
  answer "candidates for leg N's own origin→destination, within its own
  time window" for exactly **one** leg at a time.

**Neither function, nor anything wrapping them, is ever called more than
once per user action today.** There is no code anywhere in
`crates/api`/`crates/schedule-reference`/`crates/schedule-query` that
chains one such query's result into a second query's input — no "having
found a train to the interchange, now search onward from there." **The
existing multi-leg journey feature (§0.8) achieves "multiple legs" entirely
by asking the *user* to run this single-hop search once per leg, by hand,
after the user has already decided the interchange themselves.** This is
the precise gap this document addresses: the building block for "one hop,
time-windowed" exists and is solid; the building block for "chain hops
together to find a route the user didn't already know" does not exist
anywhere in this codebase, in any form, today.

### 0.4 "Stations near each other" and walking transfers — geographic primitive exists, no transfer graph exists yet

`GET /public/stations/nearby?lat=&lon=` (`crates/api/src/routes/reference.rs:100-118`,
`reference::nearest_stations`) answers "nearest stations to an arbitrary
lat/lon" via a Haversine-distance query, for the frontend's "near me"
feature — unauthenticated, read-only, capped at `NEARBY_MAX_LIMIT = 50`
(`reference.rs:35`). It is **seeded by the user's own GPS location today**
(per the git log's own recent commit, "feat(frontend): add 'Use my
location' near-me station lookup"), and is not wired to anything
transfer-related — but nothing about the query itself requires GPS input;
the same query, seeded with one *station's own* lat/lon, would answer
"which other stations are near this one."

**What genuinely does not exist today**: any persisted table of *known*
walking interchanges (no `station_transfers`, no curated "these two
differently-named CRSs are the same real-world interchange" list), and any
walk-time estimate derived from distance. §0.2 above covers the two real
data sources v1 needs to build this properly, confirmed directly against
this app's own ingestion this pass:

- **Same-station changes** (e.g. changing platforms at a station both
  trips call at): needs no new *data source*, only the MSN `A` record's
  column-65 minimum-interchange-time field this app's ingestion already
  holds in memory but doesn't parse (§0.2) — a small parser addition.
- **Cross-station walking transfers** (e.g. King's Cross ↔ St Pancras,
  differently-named CRSs): needs the `ALF` CIF file member, not fetched by
  this app's ingestion at all today (§0.2) — genuinely new acquisition
  work, of an already-standardised file.

The Haversine "nearby stations" query remains a useful independent
fallback/sanity-check primitive (e.g. flagging an `ALF` link whose two
stations are implausibly far apart), but is not itself a substitute for
either of the two CIF-sourced interchange data sources above — it has no
notion of which nearby stations are *actually* a sanctioned interchange
(two stations can be geographically close without any real walking route
or fare-through arrangement between them).

### 0.5 GTFS (`crates/poller-irish-rail-gtfs`) — a real trip/stop_times shape exists, but only as a one-shot catalogue-population tool, scoped to the Republic of Ireland only

`poller-irish-rail-gtfs` depends on the `gtfs_structures` crate, which
does give a real, standard GTFS in-memory model —
`Trip.stop_times: Vec<StopTime>`, `StopTime.stop_sequence: u32`
(`crates/poller-irish-rail-gtfs/src/mapping.rs:10-11`) — structurally the
same "ordered stop times per trip" shape §0.2 derives from CIF. **But this
poller does not use that shape for live querying at all.** Its only
consumer, `mapping.rs:45-82`, picks **one representative trip per
route — the longest one, by stop-time count** — purely to backfill
`IslandOfIrelandLineDefinition.stations` (`crates/common/src/island_of_ireland.rs:58-68`),
the *separate*, ROI/NI-specific line-catalogue analogue to `lines/*.toml`
(confirmed: this is a distinct Rust struct, a distinct catalogue concept,
not a variant of the GB `lines/*.toml` schema in §0.1). It ingests
`stop_times.txt` once, at line-definition-build time, and never persists
or re-queries the full GTFS trip set afterward — no `transfers.txt` is
read at all (grepped directly, zero hits for `transfers` anywhere in this
crate). **This is a one-off ETL helper for the ROI line catalogue, not a
running schedule-query service** — nothing like `schedule-reference`'s
30-minute republish cycle exists for it. This is also, independently, the
data source for the network this v1 excludes entirely (§0.6), so it is not
on this feature's critical path at all.

### 0.6 What "the network" actually spans today, and why v1 is GB National Rail only

Confirmed directly, not assumed: `lines/*.toml`'s `mode` field is **always**
`"national-rail"` (§0.1) — this includes London Overground, the Elizabeth
line, and other TfL-branded-but-Network-Rail-infrastructure services,
which are catalogued the same way as any GWR/LNER line and (presumably,
**Speculative** — not independently re-verified against a real CIF extract
this session) are covered by the same CIF feed since they run on NR metals
and use NR-style schedules.

**London Underground (tube), DLR, and tram are a different matter
entirely.** `crates/poller-tfl` ingests the TfL Unified API for exactly
five `mode_name`s: `tube, dlr, overground, elizabeth-line, tram`
(`crates/poller-tfl/src/config.rs:30`), but its schema
(`TflLine`/`TflLineStatus`, `poller-tfl/src/schema.rs:36-103`) is **purely
line-level operational status** ("Good Service"/"Minor Delays"/etc.) —
there is no `TflLine` field anywhere carrying a stop sequence, a calling
pattern, or a scheduled time. **The Underground/DLR/tram network has no
calling-point-level data anywhere in this codebase** — the only network
with that level of detail is the CIF-derived data in §0.2, which is a
National Rail feed, not a TfL one.

**Northern Ireland and the Republic of Ireland are structurally separate
systems**, confirmed by §0.5: a distinct catalogue type
(`IslandOfIrelandLineDefinition`), distinct pollers
(`poller-nir-stations`, `poller-irish-rail-live`, `poller-irish-rail-gtfs`),
and — critically — **none of it flows through CIF or `schedule-query` at
all**. NI Railways and Iarnród Éireann are simply not in the Network Rail
CIF SCHEDULE feed; they are entirely different national rail
administrations with entirely different upstream data sources.

**The concrete, load-bearing consequence**: a route-finding engine built on
top of §0.2's already-existing whole-network CIF resolve can, realistically,
cover **Great Britain National Rail (and whatever TfL-branded services are
folded into the same CIF feed) — and nothing else.** It cannot route across
the London Underground network at all (no timetable data exists for it in
this codebase, of any kind — only line status), and it cannot route within
or across Northern Ireland or the Republic of Ireland without an entirely
separate data-modeling effort building a *second*, GTFS-based route-finding
engine over `poller-irish-rail-gtfs`'s already-fetched (but currently
discarded after one representative-trip extraction) GTFS data — itself a
real, but separately-scoped and separately-sized, piece of future work.
This is roughly two independent route-finding problems of very different
maturity — one (GB National Rail) has almost all its hard data-plumbing
already built and sunk-cost-paid; the other (Underground + island-of-Ireland)
has essentially none of it. The product owner has confirmed v1 scopes to
the former only (§4); the latter is named as explicitly out of scope, not
silently assumed away.

### 0.7 Precedent search, and a naming collision flagged up front

A full read of `docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md`
and its design doc, and `docs/superpowers/specs/2026-09-03-trip-search-design.md`,
plus a scan of every other spec/plan title in `docs/superpowers/specs/` and
`docs/superpowers/plans/` for "journey planning," "route finding,"
"interchange," "connections," or "multi-leg search," turned up **nothing
that designs or even seriously scopes multi-hop, cross-line route
computation**. The closest adjacent work is the journey-tracking feature
itself (§0.8) and its reusable/repeating follow-on (§0.9) — both explicitly
treat "which train to take" as something the *user* already knows and
manually specifies per leg. `crates/aggregator/src/aggregation.rs` (the
per-line status aggregation engine) and `crates/common/src/matcher.rs` (the
~12,000-line incident-text-to-line matcher) were both checked directly for
anything reasoning about a train's cross-line route — neither does; both
operate at the "one line's own aggregate health" and "does this incident
text match this line's stations/keywords" level respectively, never
composing routes across lines. **This is genuinely greenfield algorithmic
territory for this codebase**, not a rediscovery of existing, unused
capability.

**A naming collision, worth flagging with the same posture as the two
adjacent specs' own front-matter warnings.** Two existing design docs
already use the phrase "trip search"/"whole-network trip search"
(`docs/superpowers/specs/2026-09-03-trip-search-design.md`,
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md` and
its research doc). **Neither is what this document is about.** Both are
about auto-filling a *single* `TrackTrainForm`/`journey_legs` window-search
leg's own fields from a live or CIF-derived departure board at one already-known
station — a nicer picker for a leg the user still originates and terminates
themselves. Both explicitly say so: *"Neither shape needs a
journey-planning algorithm (RAPTOR/Connection Scan). That was the original
research doc's inclusion, not a requirement of `TrackTrainForm` itself...
[it] was never a multi-leg journey planner"*
(`2026-09-04-whole-network-trip-search-research.md:316-322`). This
document is the first one in this codebase to actually take on that
deferred, harder problem: computing the hops and interchanges themselves,
not just searching within one already-chosen hop. Recommend calling the new
product surface **"Plan a trip"** or **"Route planner"** in all
user-facing copy and code, never "trip search," to avoid exactly the kind
of collision the two prior naming warnings in this codebase's own spec
history were trying to prevent. (Still an open question, §7 — no existing
precedent settles the exact name or API prefix.)

**Required reading consumed in full before this document was originally
written**: `lines/SCHEMA.md`; `lines/gwr-main-line.toml` (and spot-checks
of several siblings); `crates/schedule-query/src/resolve.rs`, `records.rs`,
`parse.rs`, `lib.rs`; `crates/schedule-reference/src/main.rs`;
`crates/api/src/data/queries.rs` (`search_schedule_calling_point_departures`,
`search_journey_leg_candidates`, and their surrounding doc comments in
full); `crates/api/src/routes/reference.rs` (`get_nearby_stations` and its
tests); `crates/poller-irish-rail-gtfs/src/mapping.rs`;
`crates/common/src/island_of_ireland.rs`; `crates/poller-tfl/src/main.rs`,
`schema.rs`, `config.rs`; `crates/api/src/routes/journeys.rs`,
`crates/api/src/data/journeys.rs` (every public function signature);
`docs/superpowers/specs/2026-09-22-journey-tracking-design.md`;
`docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`;
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md`
and its sibling design doc; `docs/superpowers/specs/2026-09-03-trip-search-design.md`;
and, for the sibling reference implementation, its own
`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md`,
`src/tools/plan-journey.ts`, and
`src/timetable/plan/{csa,raptor,connections,interchange,constraints}.ts`
and `src/timetable/cif/alf.ts` (see provenance note at the top of this
document for this pass's re-verification status). A repo-wide grep for
`interchange|walking connection|walk_time|footpath|transfer_time` and for
`template|preset|recurring|recurrence|repeat|cron|rrule` found nothing
product-facing relevant to routing in either case, this pass's own `ALF`
grep (§0.2) included.

### 0.8 The existing journey-tracking feature and `/journeys/new` — the integration target, confirmed live on `main`

Full detail in `docs/superpowers/specs/2026-09-22-journey-tracking-design.md`;
the load-bearing pieces for this document, reconfirmed directly against
`crates/api/src/routes/journeys.rs`/`crates/api/src/data/journeys.rs` as
they exist on `main` today:

```
journeys       (id, user_id, custom_name, created_at, updated_at)
journey_legs   (id, journey_id, leg_order, origin_crs, destination_crs,
                service_date, depart_after/before, arrive_after/before,
                train_subscription_id, match_mode CHECK IN
                ('unmatched','manual','auto'))
```

Routes (`journeys.rs:24-45`): `POST /Journeys` (create, with an inline
first leg — three wire-discriminated modes, `mode: "pin" | "knownTrain" |
"window"`, `journeys.rs:47-60` explains the tag design), `GET
/Journeys/mine`, `GET /Journeys/{id}`, `GET
/Journeys/{id}/legs/{leg_id}/candidates` (the single-hop search, §0.3),
`POST /Journeys/{id}/legs/{leg_id}/train` (commit a chosen candidate —
`set_leg_train_subscription`, `journeys.rs`(data) `:514`), `POST
/Journeys/{id}/legs` (add another leg to an existing journey — either
`knownTrain` or `window` shape, `add_known_train_leg_to_journey`/
`add_window_leg_to_journey`, `:325,:466`), `DELETE
/Journeys/{id}/legs/{leg_id}`.

**This is exactly the machinery a computed itinerary needs to become a
real tracked journey**, and it already supports the precise shape a route
planner would produce per hop: a `knownTrain`-mode leg
(`create_journey_with_known_train_leg`, `journeys.rs`(data)`:276`,
`add_known_train_leg_to_journey`, `:325`) takes an **already-identified**
`train_uid`/`service_date` directly, with **no window search needed at
all** — precisely what a route-planning engine has already determined for
each hop, unlike a human user who still has to browse `GET
.../candidates` themselves.

**`/journeys/new` now exists** — re-verified directly this pass by reading
`frontend/app/journeys/new/page.tsx` and
`frontend/components/JourneyCreationFlow.tsx`. It is the app's primary,
nav-linked entry point for tracking (`TRACK_JOURNEY_DESTINATION` in
`frontend/lib/navLinks.ts:43`, placed first in `PRIMARY_NAV_DESTINATIONS`).
Concretely: `JourneyCreationFlow` renders `TrackTrainForm` (unmodified
except for one new `onCreated` prop) until a first leg exists as a real
`POST /Journeys` row, then switches to a per-leg summary view with an
inline "Add a leg" (`AddJourneyLegButton`, gated by the existing
`journeyCanAddLeg` rule) and an always-available "Done" link to
`/journeys/{id}`. It creates leg 1 immediately and chains further legs on
one `POST /Journeys/{id}/legs` call at a time, rather than composing a
whole draft client-side and submitting it atomically — deliberately, per
the page's own doc comment, to avoid inventing a new atomic
multi-leg-create endpoint and its own partial-failure semantics. **A "Plan
a trip" flow's natural home is a third mode on this same page** (§5.3),
not a new page built from scratch.

### 0.9 Reusable/repeating journeys (in-flight) — orthogonal, not blocking

`docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`
adds `journey_templates`/`journey_template_legs` (a saved, date-less
shape a `journeys` row can be stamped from, on demand or on a
day-of-week recurrence) and finally gives `journey_legs.match_mode =
'auto'` a real meaning (a template-driven sweep silently commits a
candidate, per an `auto_commit_rule`). **This is orthogonal to trip
planning, not a dependency in either direction**: a template's legs are
still individually origin/destination/window-specified by a human when the
template is created — nothing in that design computes a route either.
The two features *could* compose later (a computed multi-hop itinerary
saved as a reusable/recurring template, so a regular multi-leg commute
doesn't need re-planning every day), but this document treats that as a
clearly-later, independently-addable combination (§7), not something to
design for now.

---

## 1. The real algorithmic problem, and why v1 builds two search algorithms

Real timetable-based journey planning at national-network scale is a
well-studied, decades-old research area — this is not a "write some SQL"
problem, and this document does not pretend otherwise. The dominant
families, and why each does or doesn't fit this app's actual shape:

- **Time-dependent Dijkstra over a time-expanded or time-dependent
  graph.** The classical approach: model every scheduled departure/arrival
  event as a graph node (or use edge-weight functions that depend on
  arrival time at the tail), run a shortest-path search. Correct, well
  understood, but for a national timetable-scale network the naive
  time-expanded graph is enormous (one node per stop-event, i.e. on the
  order of the ~7.6M daily calling points this app's own prior research
  already measured, `2026-09-04-whole-network-trip-search-research.md:216-219`),
  and a priority-queue-based Dijkstra pass over that is slower in practice
  than approaches purpose-built for the "many parallel scheduled trips"
  structure below. **Not recommended** as the primary engine — it's the
  textbook baseline every other approach below is measured against, not
  the production choice for a system this size.
- **Connection Scan Algorithm (CSA)** (Dibbelt et al.). Flattens the
  timetable into one array of "connections" — `(dep_stop, dep_time,
  arr_stop, arr_time, trip_id)`, one per adjacent pair of stops on every
  trip — sorted once by departure time, then a single linear scan forward
  from the query's earliest-departure time answers "earliest arrival at
  every reachable stop." Simple to implement and reason about (no
  route-grouping trick needed, unlike RAPTOR), scales well for a
  single-query, single-day search, and — this is the fit that matters most
  for this codebase — **its required input is almost exactly what §0.2
  already computes**: walking each `ResolvedSchedule.calling_points` in
  order and emitting one connection per adjacent pair is a small,
  mechanical transform of data this app's own poller already builds every
  cycle. CSA's raw output is an earliest-arrival *frontier*, not a
  human-presentable itinerary — recovering the actual leg-by-leg path
  needs a standard back-pointer/predecessor trace over that scan, itself
  new code (§2).
- **RAPTOR (Round-based Public Transit Optimized Router)** (Delling et
  al.), and its multi-criteria sibling **McRAPTOR**. Processes the query in
  "rounds" (round *k* = "reachable using at most *k* trips"), each round
  scanning trips grouped by *route* (a route = every trip sharing the same
  ordered stop sequence) rather than by individual connection — this is
  what makes RAPTOR fast in practice on real transit networks and what
  every major production journey planner descended from (Google's transit
  routing heritage traces to this family). McRAPTOR extends it to return a
  Pareto-optimal set of itineraries balancing arrival time against number
  of transfers (and other criteria) simultaneously, instead of a single
  "best" answer — this is what gives v1's `results: 'options'` mode its
  Pareto set, with "fewest changes" falling out of RAPTOR's own round
  structure rather than needing separate logic. It needs one thing CSA
  doesn't: grouping trips into "routes" by identical stop pattern, which
  this app's data doesn't do today (§0.2's `ScheduleIndex` is UID-keyed,
  not route-pattern-keyed) — a real, if modest, additional preprocessing
  step.
- Both CSA and RAPTOR are **static-timetable, single-service-day**
  algorithms by nature — neither natively reasons about *live* delay data
  changing which connections are actually reachable mid-journey (that's a
  separate, harder "realtime-aware routing" extension neither this
  document nor v1 should attempt — see §4's scope).

### Why v1 builds both, not CSA alone

This document's own original recommendation, before the product owner's
2026-09-22 decisions, was **CSA only**, deferring RAPTOR to a later phase.
That reasoning is worth preserving, because it explains a real, considered
tradeoff rather than an oversight later corrected on a whim:

1. CSA's input requirement is the smallest possible delta on top of data
   this app already produces every 30 minutes — no route-pattern grouping
   step, no new preprocessing concept.
2. This app's own architecture has a strong, twice-independently-arrived-at
   precedent against building anything RAPTOR-shaped that needs a
   persistent "routes" abstraction resident in a service — every prior
   CIF-adjacent design in this codebase (§0.2's own citations) deliberately
   keeps the whole-network resolve **transient, rebuilt per cycle, never
   resident** specifically to avoid a new class of operational risk (stale
   state, unbounded memory growth in a service that's otherwise a
   lightweight restart-safe poller). CSA's single flat sorted array is a
   more natural fit for "build fresh, use once, discard" than RAPTOR's
   round/route bookkeeping.
3. For a single-criterion earliest-arrival MVP, CSA and RAPTOR perform
   comparably; RAPTOR's real advantage (multi-criteria Pareto sets via
   McRAPTOR) only pays off once multi-criteria ranking is an actual
   requirement.

**What changed**: the product owner has confirmed multi-criteria ranking is
a v1 requirement, not a later phase — so point 3's premise no longer holds.
Rather than build RAPTOR from a standing start, v1 adopts the sibling
`Distant-Signal-MCP` project's own proven shape for exactly this situation:
let the caller pick the algorithm by desired outcome, not by name — a
`results: 'fastest' | 'options'` parameter, `'fastest'` running Connection
Scan (single earliest-arrival answer, cheaper), `'options'` running RAPTOR
(the Pareto set described above). Both algorithms consume the *same*
connections-array build (point 1's reasoning about a minimal, mechanical
transform still holds — it's shared, not duplicated), so the "transient,
rebuilt-per-cycle-or-per-query, never resident" posture in point 2 still
applies to both, not just CSA; RAPTOR is additive to that posture, not a
departure from it.

This pairing also solves a real, separate problem v1 would otherwise face
on its own: **correctness confidence at this scale.** On a graph of roughly
290,000 connections per day (§3), no fixture set can be hand-verified
exhaustively. The sibling project's own design reasoning — "for any query,
RAPTOR's earliest arrival must equal Connection Scan's" — makes running two
independently-implemented algorithms against the same connections array
and asserting agreement on earliest-arrival the primary correctness
mechanism, materially cheaper to gain confidence from than hand-verifying
itineraries one at a time. This is available from day one precisely
because v1 builds both algorithms together, rather than "once a second
algorithm eventually gets built" — see §8 Phase 4's differential testing
task.

**This is genuinely hard, well-precedented engineering, not a quick SQL
query — but this codebase's specific starting position is unusually good**:
the single most expensive part of any real implementation (a correct,
whole-network, STP-overlay-resolved, day-offset-correct national timetable)
is not a future cost here. It is a sunk, already-running cost (§0.2). What
remains is: (a) a mechanical connections-array transform of that existing
data, (b) the two scan algorithms themselves, (c) the interchange data
(§0.4) both need, (d) a hosting/refresh model for where the transform runs
(§3), and (e) the product-integration layer (§5) — all real work, none of
it hypothetical-scale-of-a-month CIF parsing this codebase has already
separately, correctly concluded is not worth re-doing (§0.2's own
citations).

**Not re-litigated here**: the sibling project's `via`/`avoid`/`viaStop`/
`avoidStop` route constraints, its rich per-leg output (headcode/"wider
working" linkage, explicit interchange description), and its
failure-attribution UX ("name the constraint that made it impossible") are
all real, well-reasoned ideas worth reading directly from that repo's
design doc when this feature reaches an implementation-planning stage — not
summarised exhaustively here since they don't change this document's own
v1-scope recommendation (§4), only enrich a later phase's design once one
exists.

---

## 2. What's genuinely new vs. reusable

| Piece | New or reused |
|---|---|
| Whole-network, STP-resolved, day-offset-correct calling-point data for one service day | **Reused entirely** — `schedule_query::ScheduleIndex`, already running every 30 min (§0.2) |
| TIPLOC→CRS resolution | **Reused entirely** — `stanox_crs`, same cycle (§0.2) |
| One-hop, time-windowed candidate search | **Reused entirely** — `search_schedule_calling_point_departures`/`search_journey_leg_candidates` (§0.3), though the *multi-hop planner itself will not call these Postgres queries* — it needs an in-memory connections array, not per-hop round-trip SQL (§3) |
| A connections array (`(dep_stop, dep_time, arr_stop, arr_time, trip_id)`, sorted by departure time) | **New** — a mechanical transform of `ScheduleIndex`'s calling points, structurally adjacent to `departures_by_crs`'s own per-schedule walk (§0.2), but nothing today produces this exact shape; shared input to both CSA and RAPTOR (§1) |
| Same-CRS interchange, with a real minimum-change-time ("change trains at a station both trips call at, allowing at least N minutes") | **Small parser addition, no new I/O** — the MSN `A` record's column-65 field is already held in memory by this app's own ingestion, just not parsed past byte 52 today (§0.2, re-verified this pass) |
| Walking transfers between *different* CRSs (e.g. King's Cross ↔ St Pancras) | **New CIF ingestion work** — the `ALF` file member is not fetched by this app's delivery-discovery mechanism at all today (re-confirmed this pass, §0.2), though it is an already-standardised file that mechanism can be extended, not reinvented, to also pick up |
| The CSA scan (single earliest-arrival path, `results: 'fastest'`) | **New** — no code anywhere in this repo composes more than one hop (§0.3, §0.7) |
| The RAPTOR scan (Pareto set, `results: 'options'`) | **New**, and needs one preprocessing step CSA doesn't: grouping trips into routes by identical stop pattern (§1) — `ScheduleIndex` is UID-keyed today, not route-pattern-keyed |
| Ranking / itinerary selection | **New** — CSA's raw output is "earliest arrival per stop," not a ranked, human-presentable itinerary list; RAPTOR's raw output is a per-round label set, not a sorted-by-preference options list; both need a presentation-layer trace/sort on top |
| Turning a computed itinerary into a real tracked journey | **Reused entirely, and cleanly** — `POST /Journeys` + `POST /Journeys/{id}/legs`, `knownTrain` mode, exact fit (§0.8, §5) |
| A "Plan a trip" frontend entry point | **New mode on an existing page** — `/journeys/new` already exists (§0.8, corrected from this document's original draft); this is a third mode on `JourneyCreationFlow`, not a new page built from scratch |
| Underground/DLR/tram or Northern Ireland/ROI routing | **Not buildable from any data this codebase has today** (§0.6) — explicitly out of scope, not a smaller version of the same problem |

---

## 3. Compute cost and hosting model: where does this actually run?

This is a real, first-order architecture decision, not a detail — and it's
where this document's recommendation cuts most directly against this
codebase's own established, twice-restated precedent, and it remains
**unresolved** (§7, Open Question 1) independent of the three 2026-09-22
scope decisions.

**The tension, stated plainly**: every prior CIF-adjacent design in this
app (§0.2's citations, and independently the whole-network-trip-search
research doc's own Part 3a) has explicitly reasoned about, and rejected,
keeping a whole-network resolved index **resident** anywhere, specifically
because `schedule-reference` is meant to stay "a lightweight,
restart-safe poller" with no HTTP server and no long-lived in-memory
state carrying operational risk across cycles. A CSA connections array for
the *whole* GB National Rail network for one day is large — this app's own
prior, honest, unmeasured-but-reasoned estimate for the full calling-point
set alone is on the order of 700MB-1GB resident
(`2026-09-04-whole-network-trip-search-research.md:210-228`); a
connections array (one entry per *adjacent pair*, not per calling point) is
smaller than that but still a comparable order of magnitude, not a
small structure. **Rebuilding that from scratch, synchronously, inside an
`api` request handler, on every trip-planning query, is not viable** — it
would mean re-parsing/re-walking a multi-hundred-MB structure on the
critical path of a user-facing request, a materially different cost class
than every existing per-hop query this app runs today (a bounded SQL
query against an already-published, small, per-CRS row).

**Two real options, weighed honestly:**

1. **A new periodic, resident-per-cycle build, structurally like
   `schedule-reference` itself but exposing an HTTP query surface** — a new
   service (or a new capability added to `schedule-reference`, which would
   then need to grow an HTTP server it doesn't have today) that rebuilds
   the day's connections array once per cycle (reusing the *same* already-built
   `ScheduleIndex` that cycle's existing publishes already construct — no
   second parse, same "one pass over the feed, multiple outputs" precedent
   `2026-09-04-whole-network-trip-search-design.md`'s Decision 1 already
   established for a smaller case) and serves CSA/RAPTOR queries directly
   against that in-memory structure for the rest of the cycle. **This is
   the option that most directly works at national scale within this
   app's own existing performance envelope**, but it is a genuine, honestly-flagged
   departure from this codebase's own repeatedly-stated "no resident
   whole-network index" posture — the first time this app would keep a
   large, whole-network derived structure alive between requests, in a
   service that now needs to be more than a batch poller (an HTTP-serving,
   longer-uptime-dependent process, closer in operational character to
   `crates/aggregator` than to `schedule-reference`'s current shape). This
   is a real, new class of operational commitment (memory provisioning,
   staleness-during-a-crash risk, restart cost now mattering to a live
   query path) that should be named to the product owner explicitly, not
   smuggled in as "just a bigger version of what already exists."
2. **Bound the search scope per query, so only a small subgraph is ever
   built.** Rather than a whole-network resident index, restrict a given
   trip-planning query to lines/stations the existing `lines/*.toml`
   catalogue (§0.1) already identifies as plausibly relevant near the
   origin/destination (a coarse geographic/line-membership prefilter),
   plus a hard cap on the number of interchanges considered (§4: ≤2). This
   keeps each query's working set small enough to build on demand from the
   already-published, small, per-CRS Postgres rows (`schedule_destination_departures`
   and siblings, §0.3), fetched iteratively as the search explores outward
   — closer in spirit to CSA/RAPTOR run over a lazily-materialized subgraph
   than a full national build. **Real risk this option accepts**:
   correctness is now bounded by how good the prefilter is — a genuinely
   obscure but real route (an unusual, low-frequency cross-country
   connection the catalogue's candidate-narrowing didn't think to include)
   could be silently missed, which a full whole-network scan would never
   miss. This is an honest quality/cost tradeoff, not free correctness.

**A third possibility, raised by the sibling `Distant-Signal-MCP` project's
own measured, shipped design, worth naming explicitly because it may
dissolve this tradeoff rather than choose a side of it**: that project
builds the **whole day's** connection set fresh per query and discards it
immediately after — no prefilter, no residency. Its own design doc reports
this as cheap enough in practice on a real weekday (2026-10-15): 26,848
schedules, 316,362 public calling points, ~289,514 connections, built once
per `plan_journey` call and shared by both its algorithms, then thrown
away. If a comparably-sized build-and-discard is genuinely cheap enough
against *this app's own* data path too, it would sidestep option 2's
prefilter-completeness risk without taking on option 1's resident-index
operational commitment. **This needs its own measurement against this
app's actual schema before being treated as settled** — the sibling
project's own `TimetableStore` is a from-scratch SQLite-backed CIF store,
not `schedule-query`'s `ScheduleIndex`, and this app's per-CRS Postgres row
fetch pattern may have materially different per-query overhead than a
purpose-built timetable store's own query path — but it is a real, working
existence proof that this isn't necessarily a binary choice between a
resident whole-network index and a lossy geographic prefilter.

**Recommendation: start with option 2 for v1**, explicitly because it does
not require this codebase to take on its first-ever resident whole-network
index, and because v1's own scope (same-day, ≤2-change itineraries, §4) is
exactly the shape where a bounded, catalogue-guided subgraph search is both
cheap and, in practice, likely to find the real, sensible route a human
would take (real UK rail journeys overwhelmingly resolve via well-known
interchanges the line catalogue already names). **Measure the
build-and-discard option above against real usage before ruling it out**,
and **revisit option 1 explicitly, as a deliberate, separately-reviewed
architecture decision, only once real usage data shows the prefilter
missing routes often enough to matter** — this mirrors this app's own
repeated "ship the honest partial thing, revisit with real data" posture
(the per-station-stats 53/286 precedent, the LDBWS-fallback-only CIF picker
precedent) rather than committing to the larger architectural change up
front on projected need alone.

---

## 4. v1 scope, and what's deliberately still out of scope

Real journey planners (this document takes Citymapper/Google Maps transit
mode as the honest reference point for "full generality," not a strawman)
support same-day and future-day search, walking transfers between any two
stations within a radius, multi-criteria ranking, accessibility
constraints, live disruption-aware re-routing, and fare information. v1, as
finalized by the product owner 2026-09-22, is narrower than that reference
point in some ways and broader than this document's own original proposal
in others:

- **Single service day only.** The user picks one date; the planner
  searches only within that calendar day (matching `journey_legs.service_date`'s
  own existing single-`DATE` shape, §0.8 — no schema mismatch to reconcile).
  A journey that would need to continue past midnight into the next
  calendar day (a genuinely late overnight service) is out of scope for
  v1 — flagged, not silently handled by an incorrect date rollover.
- **Multi-criteria ranking is required, not a later phase.** v1 returns
  both a `results: 'fastest'` single earliest-arrival answer (Connection
  Scan) and a `results: 'options'` Pareto set trading arrival time against
  interchange count (RAPTOR), per §1's revised engine recommendation and
  the sibling `Distant-Signal-MCP` project's own precedent for that exact
  split. (This document's original recommendation was "earliest arrival
  only, plus 1-2 cheap alternatives as a byproduct," deferring RAPTOR — see
  §1 for why that changed.)
- **A hard cap of at most 2 interchanges** (i.e., at most 3 legs) per
  computed itinerary — bounds both the search space (§3) and presentation
  complexity for `'fastest'` mode; RAPTOR's own round count for `'options'`
  mode uses the same cap (per the sibling project's `PLAN_MAX_CHANGES`
  config precedent) rather than a separate, undiscussed limit.
- **Walking transfers between differently-named stations are required, not
  a later phase.** v1 must be able to route via a cross-London (or
  equivalent) walk/tube/bus/tram/ferry hop between two different CRS
  codes, not same-CRS interchange only. Per §0.2/§0.4, this is buildable
  from the sibling project's proven `ALF` fixed-links approach — genuinely
  new CIF ingestion work (this app's `schedule-reference` delivery-discovery
  mechanism needs extending to also fetch the `ALF` file member, plus a new
  parser and a new `fixed_links`-shaped table: mode, from-CRS, to-CRS,
  minutes, validity window, day mask). Same-CRS interchange (the MSN
  column-65 field, also not yet parsed, but needing no new fetch — §0.2) is
  a smaller, still-necessary companion piece — both interchange data
  sources are needed together for v1's interchange validity checking.
  (This document's original recommendation was same-CRS interchange only,
  deferring cross-station walking transfers — see §0.4 for what's actually
  needed to support the reversal.)
- **Optional waypoints are ordered, not a "visit these in any order"
  problem.** The user-supplied waypoint list is treated as fixed
  sub-journey boundaries — start→waypoint₁, waypoint₁→waypoint₂, ...,
  waypointₙ→finish — each independently solved by the same engines,
  exactly mirroring how the existing multi-leg journey feature already
  treats leg N's destination as leg N+1's suggested (not enforced) origin.
  **Not** a traveling-salesman-style "find the best order to visit these"
  problem — solving unordered waypoint visitation is a materially harder,
  separate problem this document does not recommend attempting.
- **GB National Rail (+ whatever's in the same CIF feed) only** — no
  Underground/DLR/tram, no Northern Ireland, no Republic of Ireland (§0.6).
  This was this document's original recommendation, and the product owner
  has confirmed it, unchanged. This is not a smaller version of "all four
  networks" — it is the only one of the four with the underlying data this
  feature needs at all.
- **No live-disruption awareness.** The planner reasons over the
  *scheduled* timetable only (exactly what §0.2's `ScheduleIndex` already
  gives it) — it does not check whether a candidate leg's real train is
  currently cancelled or running late. This mirrors the existing CIF
  fallback-picker's own honesty posture (`2026-09-04-whole-network-trip-search-design.md`
  Decision 5: "this source is not live running information") and is a
  correctness-relevant caveat the UI must state plainly, not a silent
  simplification — a computed itinerary is a *plan*, not a live-confirmed
  path, until each leg is actually committed and tracked (§5).
- **No accessibility constraints, no fare/cost information.** Both are
  real, named-but-deferred scope, consistent with this being a first
  slice, not a general-purpose planner.

**What v1 delivers, honestly**: for the large majority of real GB National
Rail journeys — same-day travel, at most two changes, including changes
that require a short walk between differently-named stations — a user who
does not already know the route gets one back, computed, ranked (a single
fastest answer, or a small set of options trading speed against number of
changes), with a "track this whole thing" action (§5). This is real, new
capability this app has never had before. It is not Citymapper-grade
(no live disruption, no accessibility, no fares, no multi-day), and should
not be marketed as such.

---

## 5. Integration with journey-tracking (the key product tie-in)

**Recommendation: a computed itinerary becomes a real journey through the
existing `POST /Journeys` + `POST /Journeys/{id}/legs` machinery, reusing
it exactly as designed for a human-picked candidate — never a separate,
parallel "just show me a route" feature with no tracking tie-in.** This is
not a close call: the whole reason §0.8's `knownTrain` leg mode exists is
"the caller already has a real `train_uid`" — a route planner's own output,
per hop, is precisely that. Building a separate, unconnected route-display
feature would duplicate the entire multi-leg data model this app already
has, for strictly less value (no notifications, no delay tracking, no
group sharing, none of §0.8's/§0.9's existing machinery).

### 5.1 What "commit this itinerary" does, concretely

For an *n*-leg computed itinerary the user picks:

1. `POST /Journeys` with the **first** leg in `knownTrain` mode
   (`{mode: "knownTrain", trainUid, serviceDate}`, exactly the shape
   `create_journey_with_known_train_leg` already accepts,
   `journeys.rs`(data)`:276`) — the planner already knows this hop's exact
   `train_uid` (it came from a real, resolved `ScheduleIndex` entry, not a
   window search), so there is **no candidate-browsing step for the user
   at all** for a planner-sourced leg, unlike today's manual `window`-mode
   flow. This is a strictly better creation path than what a human
   building the same multi-leg journey by hand goes through today.
2. For each subsequent leg, `POST /Journeys/{id}/legs` with the same
   `knownTrain` shape (`add_known_train_leg_to_journey`, `journeys.rs`(data)`:325`),
   in order, `leg_order` following naturally from repeated calls.
3. **`origin_crs`/`destination_crs` on each created leg should be the
   planner's own computed hop boundary, not necessarily the matched
   train's full origin/terminus** — reusing the journey-tracking design's
   own intent verbatim ("the leg's own intent, kept even once matched...
   what makes station-skip detection well-defined"). This matters
   concretely for a route-planner-sourced leg: the traveller boards
   partway through a long-distance service and alights before its true
   terminus at the interchange — the leg's own origin/destination must
   reflect *that*, not the train's full advertised route, for the existing
   skip-detection and notification logic to keep meaning what it already
   means.
4. **Nothing else is new.** Once created this way, every leg is an
   ordinary `journey_legs` row with `match_mode = 'manual'` (it has a
   concretely bound train, exactly like a human's own known-train pick —
   there is no reason to invent a fourth `match_mode` value for
   "planner-picked"; §0.9 already reserves `'auto'` for a different,
   template-driven meaning and this is not that). Delay/cancellation
   notification, the notifier's per-`trains_id` fan-out, group sharing,
   ticket attachment — **all reused unchanged**, the same "composition of
   existing primitives" pattern journey-tracking's own phases repeatedly
   use.
5. **A cross-station walking-transfer hop (§4) needs its own, honest
   representation.** Unlike a same-CRS interchange, a leg that ends by
   walking from one station to a different, differently-named one has no
   `train_uid` for that walking segment at all — it is not itself a
   `journey_legs` row in the existing schema's sense (that table's shape
   is fundamentally "board a specific train"). The two most likely shapes
   — a walking hop folded into the presentation layer only (the itinerary
   shows "walk from X to Y, 8 min" between two ordinary train legs, with no
   corresponding `journey_legs` row created for it at all), versus a new,
   minimal leg concept for a non-train hop — are a real design decision
   for the implementation-planning stage, not resolved here; this document
   only establishes that the *train* legs on either side of a walk fit the
   existing model exactly as described above.

### 5.2 What the planner itself needs to expose, distinct from journey-tracking's own surface

A new, separate read-only endpoint, e.g. `GET /Trips/plan?origin=&destination=&waypoints=&date=&departAfter=&results=fastest|options`
(exact naming a product/API-design decision, not resolved here — see §7),
returning candidate itineraries per §4's cap (a single itinerary for
`results: 'fastest'`, a Pareto set for `results: 'options'`), each
`{legs: [{originCrs, destinationCrs, trainUid, scheduledDeparture,
scheduledArrival, ...}], totalDuration, changeCount}` — enough for the
frontend to render a route summary and then, on selection, drive §5.1's
sequence of existing journey-creation calls. This is a **new read path**,
computed from §1/§3's engines, that produces input to the *existing* write
path — not a modification of `POST /Journeys` itself, which needs no
change at all to accept planner-sourced legs (it already accepts arbitrary
`knownTrain` legs from any caller — confirmed by reading `journeys.rs`'s
own request handling, it has no notion of "this leg came from a human
search vs. a planner," which is exactly the point: no new schema, no new
`journey_legs` column, for the train-leg case at least — §5.1 point 5's
open question about walking hops aside).

### 5.3 Where this slots into the frontend

Since `/journeys/new`/`JourneyCreationFlow` already exists and is the
app's primary tracking entry point (§0.8, corrected from this document's
original draft), a "Plan a trip" flow's natural home is a **third mode**
on that same component, alongside its current two: "pin a specific train"
and "search a time window" (both currently surfaced through
`TrackTrainForm`). Concretely: a mode selector offering "I know my route"
(today's flow, unchanged) vs. "Plan a route for me" (this document's new
flow — origin, destination, optional waypoints, date, and a fastest/options
toggle → candidate itineraries from §5.2's endpoint → pick one → §5.1's
creation sequence, reusing `JourneyCreationFlow`'s existing
`handleLegOneCreated`/`handleLegAdded` state machine once the first
planner-sourced leg is created). This keeps "plan a trip" and "track a
journey" feeling like one coherent feature terminating in the same
`journeys`/`journey_legs` data, rather than two separate products — and,
unlike this document's original draft (written before `/journeys/new`
shipped), there is no sequencing dependency to name here: the page this
feature integrates into is real, live on `main`, today.

---

## 6. Per-requirement analysis

- **"Given a start station, a finish station, and optionally some
  waypoints"** — supported as scoped in §4 (ordered waypoints, single
  day, GB National Rail only).
- **"Work out an actual route across the network"** — supported via the
  new CSA/RAPTOR engines (§1, §3), built from already-existing data (§0.2);
  this is the genuinely new algorithmic core this document designs the
  shape of but does not implement.
- **"Which line(s)/train(s) to take and where to change"** — a computed
  itinerary is, by construction, a sequence of `(train_uid, service_date,
  origin_crs, destination_crs)` hops, plus (per §4/§5.1) any walking
  transfer between them — the "which train, where to change" answer *is*
  the itinerary's own shape, with no separate "explain the route in words"
  step needed (though a human-readable summary, e.g. "Take the 08:14 to
  Reading, change there for the 08:52 to Bristol," is a frontend
  presentation detail, not a new backend concept).
- **Real infrastructure vs. from-scratch**: **substantially real
  infrastructure already exists** (§0.2 is the headline finding — the
  hardest, most expensive part of any such system, whole-network
  STP-resolved timetable data, is already produced in production every 30
  minutes) — but the *route-finding/pathfinding logic itself* (now two
  algorithms, §1), the *hosting model* for querying it at low enough
  latency for a user-facing request (§3), and the *walking-transfer
  ingestion* (§0.2, §0.4) are all genuinely greenfield. This is not
  "mostly there, just wire it up," but it is also nowhere near "parse CIF
  from scratch" — the honest middle ground this document's own §0.2/§1
  finding establishes.

---

## 7. Open questions for the product owner

The three scope questions this document originally raised about network
coverage, walking transfers, and multi-criteria ranking were all resolved
by the product owner on 2026-09-22 and are reflected as final scope
throughout this document (§4) — not re-listed here. What remains open:

1. **Resident-index architecture commitment (§3)** — is the product owner
   willing to accept this app's first-ever resident, whole-network,
   large-memory derived structure (option 1) if the bounded-subgraph
   approach (option 2, this document's v1 recommendation) turns out to
   miss real routes too often in practice, or if a measured
   build-and-discard approach (§3's "third possibility") turns out not to
   be cheap enough against this app's own data path? Not resolved here —
   flagged as the single biggest architecture decision this feature
   raises, deferred until real usage data exists per §3's own recommended
   sequencing.
2. **Naming** — "Plan a trip" / "Route planner," or different
   product-facing language, and does the API surface live under a new
   `/Trips/*` prefix (§5.2) or somewhere under the existing `/Journeys/*`
   namespace? No existing precedent settles this; flagged the same way
   adjacent specs have flagged their own "Journey" naming collisions
   (§0.7).
3. **Live-disruption honesty in the UI** — confirm the recommended framing
   ("this is a scheduled plan, not live-confirmed," mirroring the existing
   CIF-fallback picker's own disclosed-staleness copy) is acceptable,
   rather than users expecting the planner to already account for today's
   actual delays/cancellations.
4. **Composition with reusable/recurring journeys (§0.9)** — should a
   planner-computed itinerary be save-able as a `journey_template` (so a
   regular commute found once via the planner doesn't need re-planning
   every day)? Real future value, explicitly not designed in this
   document — flagged for a later, separate pass once both features
   individually exist.
5. **Walking-transfer leg representation (§5.1 point 5)** — does a
   cross-station walk get its own minimal `journey_legs`-adjacent record,
   or stay presentation-only with no corresponding database row? Not
   resolved here; a real design decision for the implementation-planning
   stage.

---

## 8. Proposed phased delivery plan

v1's scope (§4) needs, in dependency order: interchange data before
connections-and-interchange logic can be validated end-to-end; a
connections array before either search algorithm; CSA before RAPTOR (so
RAPTOR's differential test has something to check itself against); an API
surface once at least one engine is queryable; and frontend integration
last, since it depends on the API surface existing. The phases below
reflect that dependency order, not a fixed commitment to exactly six
phases — later phases can be resequenced relative to each other (e.g. the
API surface could expose `'fastest'` before RAPTOR lands, if shipping a
CSA-only read path early has independent value) without disturbing the
phases before them.

**Phase 1 — CIF ingestion extension: ALF fixed links + MSN interchange
minutes.** Extend `crates/schedule-reference`'s `discovery.rs` to also
require and locate an `RJTTF*ALF.txt` file per delivery (mirroring
`mca_path`/`msn_path`'s existing shape); add a new parser module for `ALF`
fixed-link records (mode, from-CRS, to-CRS, minutes, validity window, day
mask — §0.2/§4) and a new field extraction on the MSN `A`-record parse
(the column-65 minimum-interchange-time value, §0.2) reading bytes this
app's `poll_once` already holds in memory; add a new table (or extend the
existing stanox-crs publish path) to carry both outward from
`schedule-reference` to `api`, mirroring the existing `upsert_stanox_crs`
pattern (`crates/api/src/routes/ingest.rs:365-370`). **No search
algorithm, no new route, no frontend** — this phase's own deliverable is
real, queryable interchange data (same-CRS minimum change time; ALF
cross-CRS fixed links with validity windows), independently useful and
independently testable against real `timetable_full.zip` data before
anything downstream depends on it.

**Phase 2 — Connections-array extraction + interchange-aware connection
building.** A new pure function (structurally sibling to `departures_by_crs`/
`departures_by_destination_crs`, likely living in `crates/schedule-query`
itself, reusing `ScheduleIndex`/`resolve_for_date` unchanged) that walks
one service day's resolved calling points into a sorted connections array,
plus the logic that turns Phase 1's interchange data into synthetic
"transfer connections" a search algorithm can traverse the same way as a
real train connection (a same-CRS change respecting its minimum-interchange
minutes; a cross-CRS `ALF` walk respecting its validity window). **No
search algorithm yet** — this phase proves the connections array and its
interchange augmentation are correct in isolation, with real unit tests
against constructed fixtures (mirroring `resolve.rs`'s own test-fixture
conventions), before either scan algorithm is built on top.

**Phase 3 — Connection Scan Algorithm (`results: 'fastest'`).** The CSA
scan itself (earliest-arrival, ≤2-interchange cap, both same-CRS and
cross-CRS `ALF` transfers per Phase 1/2, per §4) plus the back-pointer
trace needed to recover an actual leg-by-leg itinerary from CSA's raw
arrival-time frontier (§1, §2). Timing/memory-profiled against real
`timetable_full.zip` data under §3's bounded-subgraph hosting model (the
same kind of real-data validation this codebase's other CIF work always
insists on, per §0.2's own citations). **Complexity: medium-high** — the
first genuinely new pathfinding logic in this codebase.

**Phase 4 — RAPTOR (`results: 'options'`) + differential testing against
CSA.** The route-pattern-grouping preprocessing step CSA doesn't need
(§1), the round-based RAPTOR/McRAPTOR scan producing a Pareto set trading
arrival time against interchange count, and — as the primary correctness
mechanism for both algorithms, per §1 — a differential test suite
asserting RAPTOR's earliest-arrival result agrees with CSA's on the same
query, across a real, varied set of origin/destination/date combinations.
**Complexity: medium-high** — RAPTOR's own logic plus the differential
harness that makes trusting either algorithm at national scale tractable.

**Phase 5 — Read-only planning API.** `GET /Trips/plan` (§5.2, naming per
Open Question 2), served from Phase 3/4's engines using §3's
bounded-subgraph hosting model (or whichever option §3's own open question
resolves to by this point), supporting both `results` modes. **No
frontend yet** — this phase's own deliverable is a real, queryable
endpoint, testable directly (e.g. via existing API integration-test
conventions) before frontend work depends on its exact response shape.
**Complexity: medium** — mostly a new read route over Phase 3/4's already-proven engines.

**Phase 6 — Frontend integration: a third `/journeys/new` mode, and
"track this itinerary."** Adds "Plan a route for me" as a third mode on
`JourneyCreationFlow` (§5.3) — origin/destination/waypoints/date input,
Phase 5's endpoint for results, a route-summary display for both
`'fastest'` and `'options'` responses — and wires a picked itinerary into
`POST /Journeys` + `POST /Journeys/{id}/legs` exactly as designed in §5.1.
No backend schema change beyond whatever Open Question 5 (walking-transfer
leg representation) resolves to; otherwise purely new frontend
orchestration (a sequence of already-existing API calls, reusing
`JourneyCreationFlow`'s existing post-creation state machine) plus new
itinerary-selection state. **Complexity: medium** — composition of
existing, already-reviewed primitives for the tracking tie-in, plus a
genuinely new selection/comparison UI for the `'options'` Pareto set.

**Not phased, explicitly out of scope for all of the above**: Underground/
DLR/tram routing, Northern Ireland/Republic of Ireland routing (§0.6),
live-disruption-aware re-planning, accessibility constraints, fare/cost
information, multi-day/overnight itineraries, and composition with
reusable/recurring journey templates (§0.9, Open Question 4) — each is
independently addable later, none blocks Phases 1-6.
