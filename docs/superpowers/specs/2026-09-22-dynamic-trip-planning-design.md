# Dynamic Trip Planning — Design Investigation

**Status: research/design investigation, not an approved implementation
plan.** This document answers a single question honestly: given a start
station, a finish station, and optionally some waypoints, can this app work
out an actual multi-hop *route* across the network — which line(s)/train(s)
to take and where to change — instead of requiring the user to already know
every interchange? No code was written to produce this document. Citations
are file:line against `main` as checked out at investigation time; anything
not directly verified is flagged **Speculative**.

**Correction (2026-09-22, after this investigation completed): `/journeys/new`
now exists.** It merged into `main` in a near-simultaneous, independent piece
of work — a dedicated whole-journey creation page (`frontend/app/journeys/new/page.tsx`,
`frontend/components/JourneyCreationFlow.tsx`), now the app's primary,
nav-linked entry point for tracking (`TRACK_JOURNEY_DESTINATION` in
`frontend/lib/navLinks.ts`, placed first in `PRIMARY_NAV_DESTINATIONS`).
Every claim below that `/journeys/new` "doesn't exist yet" and would need to
be built as a prerequisite for a "Plan a trip" entry point (§0.8, §2, §5.3,
§8) is now **stale** — read those passages as: **a "Plan a trip" mode should
integrate INTO the existing `/journeys/new`/`JourneyCreationFlow` as a third
creation mode, alongside its current "pick a known train" and "search a
time window" modes, not as a reason to build that page from scratch.** The
underlying architectural reasoning in those sections (why a trip-planning
result should still create a journey via the existing `POST /Journeys` +
`POST /Journeys/{id}/legs` machinery, one leg at a time) is unaffected and
still correct — only the "this page doesn't exist, building it is now in
scope here" framing needs discarding.

---

## Addendum (2026-09-22): findings from a sibling reference implementation

A sister project in this same organisation —
`ssh://git@git-bringer-ssh.fox-prometheus.ts.net/lucy/Distant-Signal-MCP.git`,
an MCP server over the same National Rail CIF data this app ingests — has
**already built and shipped** exactly this feature (`plan_journey`, per its
own `docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md`
and `src/tools/plan-journey.ts` + `src/timetable/plan/{csa,raptor,connections,interchange,constraints}.ts`).
This is real, measured, working prior art on the identical problem against
the identical data source, not a hypothetical comparison — and it changes
three of this document's own conclusions materially enough to record here
rather than silently keep the superseded reasoning above as if untouched.

**1. The resident-index-vs-bounded-subgraph dilemma (§3) may be a false
choice — there's a third option, already proven at scale.** The sibling
project builds the **whole day's** connection set fresh per query and
discards it, no prefilter, no residency: "That is small enough to build per
query and discard; the store's seven million calling points are never all
in memory" (design doc, "The connection set"). Measured on a real weekday
(2026-10-15): 26,848 schedules, 316,362 public calling points, ~289,514
connections — built once per `plan_journey` call, shared by both search
algorithms, then thrown away. **This directly undercuts §3's Option 2
downside** (a catalogue-guided prefilter can silently miss an obscure real
route) **without taking on §3's Option 1 cost** (a resident whole-network
index, this codebase's first-ever such commitment) — if ~290K connection
objects for one day is genuinely cheap to build-and-discard per request (it
evidently is, in a sibling project against the same feed), a full,
unbounded connection set built fresh per query may be entirely viable for
this app's MVP too, sidestepping the tradeoff §3 spent most of its length
on. **This needs its own measurement against this app's actual schema**
(the sibling project's own `TimetableStore` is a from-scratch SQLite-backed
CIF store, not `schedule-query`'s `ScheduleIndex` — the shapes are
comparable, not identical, and this app's per-CRS Postgres row fetch
pattern may have different per-query overhead than a purpose-built
timetable store's own query path) before treating this as settled, but it
is a real, working existence proof that the false dichotomy this document
posed in §3 is not the only shape the problem can take.

**2. Walking/differently-named-station transfers are NOT "fully new" (§2's
table) — they're an unparsed CIF file member this app's own ingestion
already touches the delivery for.** Confirmed directly against this app's
own code (`crates/schedule-reference/src/{parser.rs,discovery.rs,main.rs}`):
this pipeline already discovers and downloads the `RJTTF*MSN.txt` file per
delivery, but only parses its `TI` (station name/TIPLOC/CRS) records — not
column 65, which the sibling project verified (against real extract 904:
3,295 stations, 2,512 of them at 5 minutes, range 0–9, with two documented
sentinel sub-cases at 98/99 meaning "not a real rail interchange, a
bus/coach stand" — `src/timetable/plan/interchange.ts`'s own carefully-reasoned
handling) carries each station's own minimum same-station change time. This
is a **small parser addition to an already-fetched file**, not new data
acquisition. Separately, the `ALF` member (fixed links between *different*
CRS codes — the sibling project's own count: 4,222 links, 1,772 metro,
1,600 tube, 557 transfer, 237 walk, 50 bus, 4 tram, 2 ferry, each with its
own validity window since e.g. Euston↔King's Cross is 5–10 min by tube but
15 by transfer depending on time of day) is **not currently fetched by this
app's CIF ingestion at all** (grepped, confirmed absent) — genuinely new
ingestion work, but of an already-standardised CIF file this codebase's
existing delivery-discovery mechanism would need only a small extension to
also pick up, not a from-scratch transfer-graph invention as §2's table
currently states.

**3. Two concrete API/architecture ideas worth adopting regardless of which
compute option (§3) is chosen:**
- **Let the caller pick the algorithm by desired outcome, not expose the
  algorithm name**: a `results: 'fastest' | 'options'` parameter, `'fastest'`
  running Connection Scan (single earliest-arrival answer, cheaper), `'options'`
  running RAPTOR (a Pareto set trading arrival time against number of
  changes — "fewest changes" falls out of RAPTOR's own round structure
  rather than needing separate logic). This app's MVP (§4) proposed
  "earliest arrival only, plus 1-2 cheap alternatives as a byproduct" —
  the sibling project's two-algorithm split is a cleaner way to reach that
  same place, deferring the RAPTOR/"options" half to a later phase rather
  than building it into an MVP that only needs CSA.
- **Differential testing as the primary correctness mechanism**: "For any
  query, RAPTOR's earliest arrival must equal Connection Scan's... on a
  graph of roughly 290,000 edges per day no fixture can be hand-verified."
  Running two independently-implemented algorithms against the same
  connection set and asserting agreement is a materially cheaper way to
  gain confidence at this scale than hand-verifying itineraries — directly
  relevant once/if this app builds a second algorithm, and worth keeping in
  mind even for a CSA-only MVP as the reason a second algorithm might be
  worth building sooner than "later phase" if correctness confidence turns
  out to matter more than this document originally weighted it.

**Not re-litigated here**: the sibling project's `via`/`avoid`/`viaStop`/`avoidStop`
route constraints, its rich per-leg output (headcode/"wider working" linkage,
explicit interchange description), and its failure-attribution UX ("name the
constraint that made it impossible") are all real, well-reasoned ideas worth
reading directly from that repo's design doc when this feature reaches an
implementation-planning stage — not summarised exhaustively here since they
don't change this document's own MVP-scope recommendation (§4), only enrich
a later phase's design once one exists.

---

**A naming collision, flagged up front, same posture as the two parent
specs' own front-matter warnings.** Two existing design docs already use the
phrase "trip search"/"whole-network trip search"
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
history were trying to prevent.

Required reading consumed in full before this document was written:
`lines/SCHEMA.md`; `lines/gwr-main-line.toml` (and spot-checks of several
siblings); `crates/schedule-query/src/resolve.rs`, `records.rs`, `parse.rs`,
`lib.rs`; `crates/schedule-reference/src/main.rs`;
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
and its sibling design doc; `docs/superpowers/specs/2026-09-03-trip-search-design.md`.
A repo-wide grep for `interchange|walking connection|walk_time|footpath|transfer_time`
and for `template|preset|recurring|recurrence|repeat|cron|rrule` (the latter
already run by the reusable-journeys doc, reused here) found nothing
product-facing relevant to routing in either case.

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
  (`docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md:56-64`,
  reconfirmed by this document's own reading of `resolve.rs`/`records.rs`
  in full, not re-run here).
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
  output of the *same* poll cycle: `common::StanoxCrsRecord`
  (`{stanox, crs, tiploc, station_name, source_sequence}`), read live via
  `stanox_crs`, a real **~3,100-row table**, fully rewritten every cycle
  (`2026-09-01-schedule-ingest-stanox-crs-table-design.md:521`,
  `queries::upsert_stanox_crs`). Combined with `ScheduleIndex`, this gives
  — for the whole GB National Rail network, for one calendar day, rebuilt
  every 30 minutes — every train's ordered `(CRS, scheduled arrival,
  scheduled departure)` sequence. **This is not a future cost to build; it
  is a byproduct this app already produces and currently only projects
  down into narrower per-station/per-line views.**
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
  (§0.6) — same shape, no `true_origin_crs`/`stops_at` params, built to
  answer "candidates for leg N's own origin→destination, within its own
  time window" for exactly **one** leg at a time.

**Neither function, nor anything wrapping them, is ever called more than
once per user action today.** There is no code anywhere in
`crates/api`/`crates/schedule-reference`/`crates/schedule-query` that
chains one such query's result into a second query's input — no "having
found a train to the interchange, now search onward from there." **The
existing multi-leg journey feature (§0.6) achieves "multiple legs" entirely
by asking the *user* to run this single-hop search once per leg, by hand,
after the user has already decided the interchange themselves.** This is
the precise gap the task brief describes: the building block for "one hop,
time-windowed" exists and is solid; the building block for "chain hops
together to find a route the user didn't already know" does not exist
anywhere in this codebase, in any form, today.

### 0.4 "Stations near each other" — geographic only, on demand, not a transfer graph

`GET /public/stations/nearby?lat=&lon=` (`crates/api/src/routes/reference.rs:100-118`,
`reference::nearest_stations`) answers "nearest stations to an arbitrary
lat/lon" via a Haversine-distance query, for the frontend's "near me"
feature — unauthenticated, read-only, capped at `NEARBY_MAX_LIMIT = 50`
(`reference.rs:35`). It is **seeded by the user's own GPS location today**,
but nothing about the query itself requires that — the same query, seeded
with one *station's own* lat/lon instead of a user's, would answer "which
other stations are near this one," which is exactly the primitive a
walking-transfer feature (e.g., "Reading is a 4-minute walk from Reading
West," or the classic London-terminus cluster) would need.

**What does not exist**: any persisted table of *known* walking
interchanges (no `station_transfers`, no curated "these two differently-named
CRSs are the same real-world interchange" list), any walk-time estimate
derived from distance, and — most importantly — **zero integration of this
route anywhere near a search/candidate/journey code path**. It is used
exclusively by a "stations near me" feature
(per the git log's own recent commit, "feat(frontend): add 'Use my
location' near-me station lookup"), unrelated to routing. A real trip
planner that wants to say "change at Reading, it's a short walk to the bus
station" (or, more importantly for rail-only routing, "King's Cross and
St Pancras are effectively one interchange") has **no existing data model
to draw on** beyond this raw distance-query building block — it would need
either a new curated table (mirroring how `lines/*.toml`'s `segment` field
is itself a curated, hand-maintained hint) or a live radius-search-derived
heuristic, neither of which exists today.

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
30-minute republish cycle exists for it.

### 0.6 What "the network" actually spans today, and the hard boundary that follows from it

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

**The concrete, load-bearing consequence for this whole document**: a
route-finding engine built on top of §0.2's already-existing whole-network
CIF resolve can, realistically, cover **Great Britain National Rail (and
whatever TfL-branded services are folded into the same CIF feed) —
and nothing else.** It cannot route across the London Underground network
at all (no timetable data exists for it in this codebase, of any kind —
only line status), and it cannot route within or across Northern Ireland
or the Republic of Ireland without an entirely separate data-modeling
effort building a *second*, GTFS-based route-finding engine over
`poller-irish-rail-gtfs`'s already-fetched (but currently discarded after
one representative-trip extraction) GTFS data — itself a real, but
separately-scoped and separately-sized, piece of future work. **The task
brief's framing of "~125+ lines... National Rail, TfL, Northern Ireland,
Republic of Ireland" should be read, honestly, as roughly two independent
route-finding problems of very different maturity — one (GB National Rail)
has almost all its hard data-plumbing already built and sunk-cost-paid;
the other (Underground + island-of-Ireland) has essentially none of it.**
This document scopes its MVP (§4) to the former only, and names the
latter as explicitly out of scope, not silently assumed away.

### 0.7 Precedent search — genuinely no prior design for this

A full read of `docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md`
and its design doc, and `docs/superpowers/specs/2026-09-03-trip-search-design.md`
(§0.3 above already summarizes why these are adjacent-but-different), plus
a scan of every other spec/plan title in `docs/superpowers/specs/` and
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

### 0.8 The existing journey-tracking feature — the integration target, confirmed live on `main`

Full detail in `docs/superpowers/specs/2026-09-22-journey-tracking-design.md`;
the load-bearing pieces for this document, reconfirmed directly against
`crates/api/src/routes/journeys.rs`/`crates/api/src/data/journeys.rs` as
they exist on `main` today (not a WIP branch):

```
journeys       (id, user_id, custom_name, created_at, updated_at)
journey_legs   (id, journey_id, leg_order, origin_crs, destination_crs,
                service_date, depart_after/before, arrive_after/before,
                train_subscription_id, match_mode CHECK IN
                ('unmatched','manual','auto'))
```

Routes (`journeys.rs:24-45`): `POST /Journeys` (create, with an inline
first leg — three wire-discriminated modes, `mode: "pin" | "knownTrain" |
"window"`, §0.8's own doc comment at `journeys.rs:47-60` explains the tag
design), `GET /Journeys/mine`, `GET /Journeys/{id}`, `GET
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
.../candidates` themselves. §5 below designs exactly this integration.

Frontend note: **`frontend/app/journeys/new/` does not exist yet**
(confirmed: `find frontend/app/journeys -maxdepth 2` returns only
`frontend/app/journeys/[id]/page.tsx` — the detail/view page — and the
existing creation entry points remain `frontend/app/track/page.tsx`
(`TrackTrainForm`) and `TrackThisTrainButton.tsx`, per the parent spec's
own §7.2, which recommends but had not yet, as of this reading, shipped
`/track` → `/journeys/new` becoming the primary creation surface). **A
"Plan a trip" flow has no existing page to slot into as an "alternative
mode" on day one** — it would be creating that missing `/journeys/new`
surface itself, or a sibling `/journeys/plan` page, not modifying an
established one. See §5.4.

### 0.9 Reusable/repeating journeys (in-flight, per the task brief) — orthogonal, not blocking

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

## 1. The real algorithmic problem, researched honestly

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
  cycle.
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
  "best" answer. **This is the more scalable, more feature-complete
  choice long-term**, but it needs one thing CSA doesn't: grouping trips
  into "routes" by identical stop pattern, which this app's data doesn't
  do today (§0.2's `ScheduleIndex` is UID-keyed, not route-pattern-keyed) —
  a real, if modest, additional preprocessing step.
- Both CSA and RAPTOR are **static-timetable, single-service-day**
  algorithms by nature — neither natively reasons about *live* delay data
  changing which connections are actually reachable mid-journey (that's a
  separate, harder "realtime-aware routing" extension neither this
  document nor a first build should attempt — see §4's MVP scope).

**Superseded by product decisions below (recorded 2026-09-22) — kept for its
reasoning, not its conclusion.** The product owner has since confirmed
multi-criteria ranking IS a day-one requirement, not deferred to Phase 2
(§7 Q4, resolved). This section's original recommendation ("CSA only, defer
RAPTOR") no longer describes v1. **Revised recommendation: build BOTH
algorithms from day one, adopting the sibling `Distant-Signal-MCP` project's
own proven shape (see the Addendum above) — Connection Scan for a single
`results: 'fastest'` earliest-arrival answer, RAPTOR for a `results:
'options'` Pareto set trading arrival time against interchange count — and
use their required agreement on earliest-arrival as the primary correctness
mechanism (Addendum, point 3), since that mechanism is now available from
day one rather than "once a second algorithm eventually gets built."** The
reasoning in points 1-3 below for why CSA fits this codebase's existing
"transient, rebuilt-per-cycle, never resident" posture still holds and still
applies to CSA's own role — RAPTOR is additive, not a replacement, and its
own connections-array input is the *same* structure CSA consumes (per the
Addendum, both algorithms share one connections build in the sibling
project). This does mean v1 now needs the sibling project's `raptor.ts`-shaped
round-based search too, not just `csa.ts`-shaped — a materially larger v1
than this section originally scoped, reflected in §4's revised MVP below.

**Original CSA-only reasoning, preserved for context:**

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
3. For an MVP scoped to single-criterion earliest-arrival ranking (§4), CSA
   and RAPTOR perform comparably; RAPTOR's real advantage (multi-criteria
   Pareto sets via McRAPTOR) only pays off once multi-criteria ranking is
   an actual requirement (§4's Phase 2+), at which point revisiting the
   engine choice — or layering a simple Pareto filter over repeated CSA
   runs — is a real, but deferrable, follow-on decision (§7).

**This is genuinely hard, well-precedented engineering, not a quick SQL
query — but this codebase's specific starting position is unusually good**:
the single most expensive part of any real implementation (a correct,
whole-network, STP-overlay-resolved, day-offset-correct national timetable)
is not a future cost here. It is a sunk, already-running cost (§0.2). What
remains is: (a) a mechanical connections-array transform of that existing
data, (b) the scan algorithm itself, (c) a hosting/refresh model for where
that transform runs (§3), and (d) the product-integration layer (§5) — all
real work, none of it hypothetical-scale-of-a-month CIF parsing this
codebase has already separately, correctly concluded is not worth
re-doing (§0.2's own citations).

---

## 2. What's genuinely new vs. reusable

| Piece | New or reused |
|---|---|
| Whole-network, STP-resolved, day-offset-correct calling-point data for one service day | **Reused entirely** — `schedule_query::ScheduleIndex`, already running every 30 min (§0.2) |
| TIPLOC→CRS resolution | **Reused entirely** — `stanox_crs`, same cycle (§0.2) |
| One-hop, time-windowed candidate search | **Reused entirely** — `search_schedule_calling_point_departures`/`search_journey_leg_candidates` (§0.3), though the *multi-hop planner itself will not call these Postgres queries* — it needs an in-memory connections array, not per-hop round-trip SQL (§3) |
| A connections array (`(dep_stop, dep_time, arr_stop, arr_time, trip_id)`, sorted by departure time) | **New** — a mechanical transform of `ScheduleIndex`'s calling points, structurally adjacent to `departures_by_crs`'s own per-schedule walk (§0.2), but nothing today produces this exact shape |
| The CSA scan itself (multi-hop path search) | **New** — no code anywhere in this repo composes more than one hop (§0.3, §0.7) |
| Same-CRS interchange ("change trains at a station both trips call at") | **New logic, but needs no new data** — a CRS a departing connection and an arriving connection share is already expressible from the connections array alone |
| Walking transfers between *different* CRSs (e.g. King's Cross ↔ St Pancras) | **Fully new** — no persisted transfer graph exists (§0.4); `nearby_stations`'s Haversine query is a usable primitive but is not wired to anything transfer-related today |
| Ranking / itinerary selection (earliest arrival; later, multi-criteria) | **New** — CSA's raw output is "earliest arrival per stop," not a ranked, human-presentable itinerary list; assembling the actual leg-by-leg path (not just the arrival-time frontier) needs a standard CSA back-pointer/predecessor trace, itself new code |
| Turning a computed itinerary into a real tracked journey | **Reused entirely, and cleanly** — `POST /Journeys` + `POST /Journeys/{id}/legs`, `knownTrain` mode, exact fit (§0.8, §5) |
| A "Plan a trip" frontend entry point | **New page** — no `/journeys/new` exists yet to extend (§0.8); this is net-new UI, not a modification of an established flow |
| Underground/DLR/tram or Northern Ireland/ROI routing | **Not buildable from any data this codebase has today** (§0.6) — explicitly out of scope, not a smaller version of the same problem |

---

## 3. Compute cost and hosting model: where does this actually run?

This is a real, first-order architecture decision, not a detail — and it's
where this document's recommendation cuts most directly against this
codebase's own established, twice-restated precedent.

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
   established for a smaller case) and serves CSA queries directly against
   that in-memory structure for the rest of the cycle. **This is the
   option that actually works at national scale within this app's own
   existing performance envelope**, but it is a genuine, honestly-flagged
   departure from this codebase's own repeatedly-stated "no resident
   whole-network index" posture — the first time this app would keep a
   large, whole-network derived structure alive between requests, in a
   service that now needs to be more than a batch poller (an HTTP-serving,
   longer-uptime-dependent process, closer in operational character to
   `crates/aggregator` than to `schedule-reference`'s current shape — the
   task brief's own suggested comparison). This is a real, new class of
   operational commitment (memory provisioning, staleness-during-a-crash
   risk, restart cost now mattering to a live query path) that should be
   named to the product owner explicitly, not smuggled in as "just a bigger
   version of what already exists."
2. **Bound the search scope per query, so only a small subgraph is ever
   built.** Rather than a whole-network resident index, restrict a given
   trip-planning query to lines/stations the existing `lines/*.toml`
   catalogue (§0.1) already identifies as plausibly relevant near the
   origin/destination (a coarse geographic/line-membership prefilter),
   plus a hard cap on the number of interchanges considered (§4's MVP
   scope recommends ≤2). This keeps each query's working set small enough
   to build on demand from the already-published, small, per-CRS Postgres
   rows (`schedule_destination_departures` and siblings, §0.3), fetched
   iteratively as the search explores outward — closer in spirit to CSA
   run over a lazily-materialized subgraph than a full national build.
   **Real risk this option accepts**: correctness is now bounded by how
   good the prefilter is — a genuinely obscure but real route (an
   unusual, low-frequency cross-country connection the catalogue's
   candidate-narrowing didn't think to include) could be silently missed,
   which a full whole-network CSA scan would never miss. This is an
   honest quality/cost tradeoff, not free correctness.

**Recommendation: start with option 2 for the MVP** (§4), explicitly
because it does not require this codebase to take on its first-ever
resident whole-network index, and because an MVP scoped to same-day,
≤2-change itineraries (§4) is exactly the shape where a bounded,
catalogue-guided subgraph search is both cheap and, in practice, likely to
find the real, sensible route a human would take (real UK rail journeys
overwhelmingly resolve via well-known interchanges the line catalogue
already names). **Revisit option 1 explicitly, as a deliberate,
separately-reviewed architecture decision, once real usage data shows the
prefilter missing routes often enough to matter** — this mirrors this
app's own repeated "ship the honest partial thing, revisit with real data"
posture (the per-station-stats 53/286 precedent, the LDBWS-fallback-only
CIF picker precedent) rather than committing to the larger architectural
change up front on projected need alone.

---

## 4. Proposed MVP scope, and what's deliberately deferred

**Revised 2026-09-22 per three product decisions** (§7 Q2, Q4, Q5 all
resolved) — v1 is materially larger than this section's original draft.
Two of the three original scope cuts below are now reversed; only the
network-scope cut (GB National Rail only) was confirmed as originally
recommended.

Real journey planners (this document takes Citymapper/Google Maps transit
mode as the honest reference point for "full generality," not a strawman)
support same-day and future-day search, walking transfers between any two
stations within a radius, multi-criteria ranking, accessibility
constraints, live disruption-aware re-routing, and fare information. The
product owner has now confirmed two of those ARE day-one requirements here
(walking transfers, multi-criteria ranking) — the remaining cuts below are
still real and still narrow the problem meaningfully, just not as far as
originally proposed:

- **Single service day only.** The user picks one date; the planner
  searches only within that calendar day (matching `journey_legs.service_date`'s
  own existing single-`DATE` shape, §0.8 — no schema mismatch to reconcile).
  A journey that would need to continue past midnight into the next
  calendar day (a genuinely late overnight service) is out of scope for
  v1 — flagged, not silently handled by an incorrect date rollover.
- **RESOLVED — multi-criteria ranking IS a v1 requirement (§7 Q4).**
  Reversing this document's original recommendation: v1 must return both a
  `results: 'fastest'` single earliest-arrival answer (Connection Scan) AND
  a `results: 'options'` Pareto set trading arrival time against
  interchange count (RAPTOR), per the Addendum's `Distant-Signal-MCP`
  precedent. §1's engine recommendation is updated accordingly — build
  both algorithms from day one, use their required agreement on
  earliest-arrival as the correctness mechanism. This is the single
  largest scope increase from the original draft: v1 now needs a
  round-based RAPTOR search, not just a CSA sweep.
- **A hard cap of at most 2 interchanges** (i.e., at most 3 legs) per
  computed itinerary, still recommended unchanged — bounds both the search
  space (§3) and presentation complexity for the `'fastest'` mode; RAPTOR's
  own round count for `'options'` mode should use the same cap (per the
  sibling project's `PLAN_MAX_CHANGES` config precedent, Addendum) rather
  than a separate, undiscussed limit.
- **RESOLVED — walking transfers between differently-named stations ARE a
  v1 requirement (§7 Q2).** Reversing this document's original
  recommendation: v1 must be able to route via a cross-London (or
  equivalent) Underground/walk/bus/tram/ferry hop between two different
  CRS codes, not same-CRS interchange only. Per the Addendum, this is
  buildable from the sibling project's proven `ALF` fixed-links approach —
  **new CIF ingestion work now pulled into v1** (this app's
  `schedule-reference` delivery-discovery mechanism needs extending to also
  fetch the `ALF` file member, and a new parser + `fixed_links`-shaped
  table, mirroring `crates/schedule-reference/src/cif/alf.ts`'s reference
  shape — mode, from-CRS, to-CRS, minutes, validity window, day mask). This
  is real, non-trivial new scope, not a small addition — it needs its own
  task(s) in a Phase 1 plan, not a "quick data model tweak" framing.
  Same-CRS interchange (MSN column 65, also not yet parsed by this app's
  ingestion per the Addendum) is a smaller, still-necessary companion
  piece — both interchange data sources are needed together for v1's
  interchange validity checking (§0's baseline "Interchange" section, once
  written into this doc's own §0, mirrors the Addendum's findings).
- **Optional waypoints are ordered, not a "visit these in any order"
  problem.** The user-supplied waypoint list is treated as fixed
  sub-journey boundaries — start→waypoint₁, waypoint₁→waypoint₂, ...,
  waypointₙ→finish — each independently solved by the same CSA engine,
  exactly mirroring how the existing multi-leg journey feature already
  treats leg N's destination as leg N+1's suggested (not enforced) origin
  (parent spec §3). **Not** a traveling-salesman-style "find the best
  order to visit these" problem — the task brief's own phrasing
  ("optionally some waypoints") supports this reading, and solving
  unordered waypoint visitation is a materially harder, separate problem
  this document does not recommend attempting.
- **GB National Rail (+ whatever's in the same CIF feed) only** — no
  Underground/DLR/tram, no Northern Ireland, no Republic of Ireland (§0.6).
  This is not a smaller version of "all four networks" — it is the only
  one of the four with the underlying data this feature needs at all.
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

**What this MVP *does* deliver, honestly**: for the large majority of real
GB National Rail journeys — same-day travel, a route resolvable via a
same-station interchange, at most two changes — a user who does not
already know the route gets one back, computed, with a "track this whole
thing" action (§5), which is real, new capability this app has never had
before. It is not Citymapper-grade, and should not be marketed as such.

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
   train's full origin/terminus** — reusing the parent spec's own §1.1
   design intent verbatim ("the leg's own intent, kept even once matched...
   what makes station-skip detection well-defined"). This matters
   concretely for a route-planner-sourced leg: the traveller boards
   partway through a long-distance service and alights before its true
   terminus at the interchange — the leg's own origin/destination must
   reflect *that*, not the train's full advertised route, for the existing
   skip-detection and notification logic (parent spec §5.2) to keep
   meaning what it already means.
4. **Nothing else is new.** Once created this way, every leg is an
   ordinary `journey_legs` row with `match_mode = 'manual'` (it has a
   concretely bound train, exactly like a human's own known-train pick —
   there is no reason to invent a fourth `match_mode` value for
   "planner-picked"; §0.9 already reserves `'auto'` for a different,
   template-driven meaning and this is not that). Delay/cancellation
   notification, the notifier's per-`trains_id` fan-out, group sharing,
   ticket attachment — **all reused unchanged**, exactly the same
   "composition of existing primitives" pattern the parent spec's own
   Phase 1 used for its non-planner legs.

### 5.2 What the planner itself needs to expose, distinct from journey-tracking's own surface

A new, separate read-only endpoint, e.g. `GET /Trips/plan?origin=&destination=&waypoints=&date=&departAfter=`
(exact naming a product/API-design decision, not resolved here — see §7),
returning candidate itineraries (§4's cap: one primary, up to two
alternatives), each `{legs: [{originCrs, destinationCrs, trainUid,
scheduledDeparture, scheduledArrival, ...}], totalDuration, changeCount}`
— enough for the frontend to render a route summary and then, on
selection, drive §5.1's sequence of existing journey-creation calls. This
is a **new read path**, computed from §3's CSA engine, that produces
input to the *existing* write path — not a modification of `POST
/Journeys` itself, which needs no change at all to accept
planner-sourced legs (it already accepts arbitrary `knownTrain` legs from
any caller, § confirmed by reading `journeys.rs`'s own request handling —
it has no notion of "this leg came from a human search vs. a planner,"
which is exactly the point: no new schema, no new `journey_legs` column).

### 5.3 Where this slots into the frontend

Since `frontend/app/journeys/new/` does not exist yet (§0.8), a "Plan a
trip" flow is not competing with an established manual-entry page — it can
be designed as one of two **modes on the same, still-to-be-built**
journey-creation surface: "I know my route" (today's per-leg manual/window
entry, per the parent spec's own recommended `/track` → `/journeys/new`
migration) vs. "Plan a route for me" (this document's new flow: origin,
destination, optional waypoints, date → candidate itineraries → pick one →
§5.1's creation sequence). Recommend they ship as two tabs/entry points of
one page rather than two separate pages, since both terminate in the exact
same `journeys`/`journey_legs` data and both should feel like one coherent
"track a journey" feature to the user, not two unrelated products. **This
is a real sequencing dependency worth naming plainly**: if `/journeys/new`
per the parent spec's Phase 1/§7.2 has not shipped by the time this
feature is built, this document's frontend work either waits on it or
builds `/journeys/new` itself as a prerequisite — not something this
document should assume away.

---

## 6. Per-requirement analysis (task brief's own asks)

- **"Given a start station, a finish station, and optionally some
  waypoints"** — supported as scoped in §4 (ordered waypoints, single
  day, GB National Rail only).
- **"Work out an actual route across the network"** — supported via the
  new CSA engine (§1, §3), built from already-existing data (§0.2); this
  is the genuinely new algorithmic core this document designs the shape
  of but does not implement.
- **"Which line(s)/train(s) to take and where to change"** — a computed
  itinerary is, by construction, a sequence of `(train_uid, service_date,
  origin_crs, destination_crs)` hops — the "which train, where to change"
  answer *is* the itinerary's own shape, with no separate "explain the
  route in words" step needed (though a human-readable summary, e.g. "Take
  the 08:14 to Reading, change there for the 08:52 to Bristol," is a
  frontend presentation detail, not a new backend concept).
- **Real infrastructure vs. from-scratch**: **substantially real
  infrastructure already exists** (§0.2 is the headline finding — the
  hardest, most expensive part of any such system, whole-network
  STP-resolved timetable data, is already produced in production every 30
  minutes) — but the *route-finding/pathfinding logic itself*, the
  *hosting model* for querying it at low enough latency for a user-facing
  request (§3), and the *walking-transfer data model* (§0.4) are all
  genuinely greenfield. This is not "mostly there, just wire it up," but
  it is also nowhere near "parse CIF from scratch" — the honest middle
  ground this document's own §0.2/§1 finding establishes.

---

## 7. Open questions for the product owner

1. **Resident-index architecture commitment (§3)** — is the product owner
   willing to accept this app's first-ever resident, whole-network,
   large-memory derived structure (option 1) if the bounded-subgraph
   approach (option 2, this document's MVP recommendation) turns out to
   miss real routes too often in practice? Not resolved here — flagged as
   the single biggest architecture decision this feature raises, deferred
   until real usage data exists per §3's own recommended sequencing.
2. ~~**Walking transfers between differently-named stations**~~ —
   **RESOLVED 2026-09-22 (product owner): required in v1, not deferred.**
   Same-CRS-only is NOT acceptable as v1. Per the Addendum, this is
   buildable via the sibling project's proven `ALF` fixed-links approach —
   real, new CIF ingestion work, now a Phase 1 task, not a later-phase
   design pass (§4).
3. **Naming** — "Plan a trip" / "Route planner," or different
   product-facing language, and does the API surface live under a new
   `/Trips/*` prefix (§5.2) or somewhere under the existing `/Journeys/*`
   namespace? No existing precedent settles this; flagged the same way the
   two parent specs flagged their own "Journey" naming collisions.
4. ~~**Multi-criteria ranking priority**~~ — **RESOLVED 2026-09-22 (product
   owner): required in v1, not deferred.** "Earliest arrival only" is NOT
   acceptable as v1 — the product owner considers multi-criteria ranking a
   day-one requirement. §1's engine recommendation has been revised: build
   both CSA (`'fastest'`) and RAPTOR (`'options'`) from day one (§4), per
   the Addendum's sibling-project precedent.
5. ~~**Underground/NI/ROI expectations**~~ — **RESOLVED 2026-09-22 (product
   owner): GB National Rail only for v1 is accepted.** No Underground/DLR/tram
   routing, no Northern Ireland/Republic of Ireland routing, in v1 — matches
   this document's original recommendation, unchanged (§4, §0.6).
6. **Live-disruption honesty in the UI** — confirm the recommended framing
   ("this is a scheduled plan, not live-confirmed," mirroring the existing
   CIF-fallback picker's own disclosed-staleness copy) is acceptable,
   rather than users expecting the planner to already account for today's
   actual delays/cancellations.
7. **Composition with reusable/recurring journeys (§0.9)** — should a
   planner-computed itinerary be save-able as a `journey_template` (so a
   regular commute found once via the planner doesn't need re-planning
   every day)? Real future value, explicitly not designed in this
   document — flagged for a later, separate pass once both features
   individually exist.

---

## 8. Proposed phased delivery plan

**Note (2026-09-22): the phasing below predates the three product decisions
resolved in §4/§7 above and needs revisiting by whoever writes the actual
implementation plan.** In particular, walking transfers (ALF ingestion) and
RAPTOR (multi-criteria ranking) — both originally sketched as later phases
below — are now confirmed v1/Phase-1 requirements, not deferred. Read this
section for its sequencing logic (what depends on what) rather than trusting
its phase boundaries as still-current.

**Phase 0 — Connections-array extraction + CSA engine, no product surface
yet.** A new pure function (structurally sibling to `departures_by_crs`/
`departures_by_destination_crs`, likely living in `crates/schedule-query`
itself, reusing `ScheduleIndex`/`resolve_for_date` unchanged) that walks
one service day's resolved calling points into a sorted connections array,
plus the CSA scan itself (earliest-arrival, ≤2-interchange cap, same-CRS
transfer only, per §4) as a library with real unit tests against
constructed fixtures (mirroring `resolve.rs`'s own test-fixture
conventions). **No new route, no new table, no frontend** — this phase
proves the algorithm correct and its performance characteristics real
(timing/memory-profiling a bounded-subgraph build, per §3's option 2,
against real `timetable_full.zip` data — the same kind of real-data
validation this codebase's other CIF work always insists on, per §0.2's
own citations). **Complexity: medium-high** — this is the phase with
genuinely new, unprecedented-in-this-codebase algorithmic logic; everything
after it is comparatively closer to routine plumbing.

**Phase 1 — Read-only planning API + a minimal "here's your route"
frontend view, no tracking tie-in yet.** `GET /Trips/plan` (§5.2, naming
per Open Question 3), served from Phase 0's engine using §3's bounded-subgraph
hosting model; a new frontend page (or the "Plan a route for me" tab of a
new `/journeys/new`, per §5.3, sequencing-dependent on that page's own
delivery timeline) that takes origin/destination/waypoints/date and shows
computed itinerary options, with no "track this" action yet — purely
informational, letting real usage validate the engine's route quality
(does the ≤2-change, same-CRS-only cap actually find the routes real users
want?) before committing to the tracking integration's own scope.
**Complexity: medium** — mostly a new read route and a new display
component over Phase 0's already-proven engine.

**Phase 2 — "Track this itinerary" integration (§5.1).** Wires a picked
itinerary into `POST /Journeys` + `POST /Journeys/{id}/legs`, exactly as
designed in §5.1 — no backend schema change, no change to the existing
journey-creation routes, purely a new orchestration on the frontend (a
sequence of already-existing API calls) plus whatever minimal
itinerary-selection state the frontend needs to hold between Phase 1's
display and this phase's commit action. **Complexity: low-medium** — this
is composition of existing, already-reviewed primitives, the same
"mechanically close to typing" character the parent specs' own later
phases repeatedly have.

**Phase 3 (stretch, not committed) — Multi-criteria ranking
(McRAPTOR-style Pareto set, or a simpler layered-CSA approximation) and/or
a walking-transfer data model.** Either or both, independently addable
once Phase 1/2 usage data answers Open Questions 2 and 4 — **not** blocking
anything in Phases 0-2 shipping first.

**Not phased, explicitly out of scope for all of the above**: Underground/
DLR/tram routing, Northern Ireland/Republic of Ireland routing (§0.6),
live-disruption-aware re-planning, accessibility constraints, fare/cost
information, multi-day/overnight itineraries, and composition with
reusable/recurring journey templates (§0.9, Open Question 7) — each is
independently addable later, none blocks Phases 0-2.
