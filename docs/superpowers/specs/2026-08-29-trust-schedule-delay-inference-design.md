# TRUST-Based Schedule-Adherence Inference for Line Status — Research Spec

**Status: research/proposal only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-28-train-tracking-design.md`
(the closest precedent — a data-layer design doc researching a new
TRUST-adjacent capability). This document evaluates plausibility and lays
out architecture options; it reaches a recommendation but is **not** an
implementation plan. If the recommendation below is "proceed," the next
step is a separate planning pass, done only after this doc has been
reviewed.

## Problem being researched

DESIGN.md's roadmap (§8, Stage 3, item 2) names, without designing it,
"TRUST-feed integration for higher-fidelity inference." Today, line status
is computed two ways:

1. **Knowledgebase incidents** (`crates/aggregator/src/aggregation.rs`) —
   human-curated NRE disruption text. Highest confidence, always wins when
   present.
2. **LDBWS sampling** (`infer_from_samples`, same file; DESIGN.md §6.2-6.3)
   — polls Darwin departure boards at a handful of `sample_stations` per
   line (3-5 stations for a line like WCML spanning Euston to Carlisle),
   and infers a line-wide severity from the sampled delay/cancellation
   rate. This is the fallback when no incident covers a line, and can also
   *escalate* (never demote) an incident-derived status
   (`escalate_from_sample_stats`).

The idea under research: instead of (or alongside) station-sampling,
compare every scheduled service's real TRUST movement events against its
planned schedule, for every train on a line rather than a handful of
sampled stations, and derive delay/cancellation rates from that full
population. `DataQuality::TrustInferred` already exists as a defined enum
variant reserved for this (`crates/common/src/lib.rs:275`) — currently
unused by any code path; only `Knowledgebase`, `LdbwsInferred`, `Planned`,
and `Tfl` are actually produced.

## What already exists (verified against code, not re-derived)

**`trust-consumer` (`crates/trust-consumer/`) is real, running code**, not
a stub — it holds a persistent `rdkafka` consumer group connection to
Network Rail's Train Movements Kafka topic
(`crates/trust-consumer/src/feed/kafka.rs`), parses TRUST's `{header,
body}` envelopes (`schema.rs` — recently fixed to accept a single bare
envelope object as well as an array, after a live-data surprise), and
matches incoming messages against a small **in-memory set of currently-
`pending` tracked pins** (`matching.rs::resolve_origin_departure`,
`process.rs`). Confirmed by reading `feed/kafka.rs`: `consumer.subscribe(&
[&config.kafka_topic])` subscribes to the *whole* configured topic — Kafka
has no server-side content filter — so **the ingest-side read volume is
already the full national feed**, regardless of how narrow the pin set is.
Non-matching messages are read, parsed, and discarded, not skipped upstream.

This is per-user opt-in individual train tracking
(`docs/superpowers/plans/2026-08-28-train-tracking.md`,
`docs/superpowers/specs/2026-08-28-train-tracking-design.md`), not
line-level aggregate inference. Three things about its current shape
matter directly for this research:

1. **STANOX→CRS translation does not exist.** `process.rs`'s module doc
   states plainly: "`loc_crs` is hardcoded `None` throughout
   `process_message`... TRUST messages carry STANOX codes, not CRS; the
   `crates/api`-owned `stations` reference table doesn't currently store a
   STANOX column (`common::StationReference` has no such field —
   confirmed, not assumed)." Pin resolution today works around this by
   comparing a pin's `origin_crs` directly against the raw STANOX string
   from the feed (`matching.rs`'s doc comment even flags this as
   presently a near-miss: it only matches "when [the pin's] `pin_origin_crs`
   happens to compare equal to the feed's STANOX string"). This is a real,
   already-acknowledged gap, not something new this research introduces —
   but it is a **hard blocker** for any line-level feature, because
   `lines/*.toml` keys every station by CRS (and separately, optionally,
   by TIPLOC — see below), never by STANOX.
2. **`lines/*.toml` already carries TIPLOC per station.**
   `common::Station` (`crates/common/src/lib.rs:389-397`) has an optional
   `tiploc` field, populated for every station in the two line files
   inspected (`lines/swr-alton.toml`, `lines/west-coast-main-line.toml` —
   e.g. `WAT`/`WATRLMN`, `EUS`/`EUSTON`, `CAR`/`CARLILE`). This matters
   because CIF SCHEDULE data (see below) is TIPLOC-keyed, not
   STANOX-keyed or CRS-keyed — so there is a plausible direct join between
   this app's curated line catalogue and CIF schedule locations that does
   **not** require solving STANOX↔CRS at all, if CIF is used as the
   spine. It does not help with TRUST's own messages, though, which carry
   STANOX (via `loc_stanox`/`reporting_stanox`), so a STANOX bridge is
   still needed to place a raw *TRUST* movement event on a line, only CIF
   schedule locations bypass it.
3. **No CIF SCHEDULE ingestion exists anywhere in this codebase**
   (confirmed: no file or module matching "cif" or "schedule" under
   `crates/`, other than the reserved `DataQuality::TrustInferred`
   variant and DESIGN.md's own two mentions). The train-tracking plan
   explicitly declined to add it ("No CIF SCHEDULE ingestion in this
   pass," `docs/superpowers/plans/2026-08-28-train-tracking.md` Global
   Constraints), narrowing that feature's own "seed the planned journey
   from CIF" goal down to "build the calling-point list incrementally
   from what TRUST itself reports" instead.

**The existing sampling approach's real, documented limitations**
(DESIGN.md §6.3): it produces a *line-wide* status only (no segment
precision — that's reserved for incident data), it never overrides an
incident-derived status (only escalates it, since 2026-08-21's
`escalate_from_sample_stats`), and it has no trend detection. Its
`min_sample_size` default is 3 — meaning a line with `sample_stations`
covering only 3-5 of a route's dozens of stations produces no
determination at all below that floor, and even above it is inferring the
whole line's health from a small, geographically clustered subset (WCML's
five: Euston, Milton Keynes Central, Crewe, Preston, Carlisle — leaving,
e.g., the whole Trent Valley corridor and Scottish extension
unrepresented by any sample point).

**Precedent for blending a secondary live signal onto a primary read**:
`crates/api/src/data/eta_blend.rs` (implemented, not just planned — 1059
lines of the train-tracking plan's Task 6 are realized as real code with
tests) does exactly this shape for individual-train ETAs: TRUST's own
propagated delay is the always-available baseline, and Darwin's live
`GetServiceDetails`/departure-board estimate is substituted in at *read
time* when a heuristic correlation finds one, without ever writing the
substitution back to the materialized `train_current_state` row. This is
a genuine, working precedent for "TRUST is the honest floor, Darwin's own
prediction is preferred when correlatable" — worth citing directly as
either a template or a cautionary comparison for this research (see
"What higher fidelity actually buys," below).

## Data source research

### CIF SCHEDULE feed

Confirmed via the Open Rail Data Wiki and Rail Data Marketplace (RDM)
search results (not independently fetched — the wiki's pages returned
HTTP 403 to a direct fetch in this research pass; findings below are
search-engine-summarized citations of those pages, flagged accordingly,
matching this app's "no invented API details" convention by attributing
rather than asserting from memory):

- **What it is.** An extract of train schedules from Network Rail's ITPS
  (Integrated Train Planning System), available in **both JSON and CIF
  text format**. The Open Rail Data Wiki's own guidance: "If you are just
  starting out with the service you should use the JSON files, as the CIF
  data is more suited to advanced users... and requires additional
  parsing." [Open Rail Data Wiki: SCHEDULE](https://wiki.openraildata.com/index.php/SCHEDULE)
- **Format (CIF text variant).** Fixed-length 80-character-per-row
  records, first two characters a record-type code: `HD` (header), `TI`/
  `TA`/`TD` (TIPLOC insert/amend/delete), `AA` (Association), and the
  Basic Schedule family `BS`/`BX`/`LO`/`LI`/`CR`/`LT`, terminated by `ZZ`.
  [Open Rail Data Wiki: CIF File Format](https://wiki.openraildata.com/index.php/CIF_File_Format)
- **STP overlay structure — confirmed, not assumed.** A schedule is
  uniquely identified by `(UID, start date, STP indicator)`. STP
  indicators are `P` (Permanent/base), `O` (Overlay/Variation), `C`
  (Cancellation), `N` (New) — for a given day, the lowest alphabetically
  wins (`C`/`O` beat `P`), which is how a Mon-Fri base schedule gets a
  one-off Christmas Day cancellation without touching the base record.
  [Open Rail Data Wiki: CIF Basic Schedule](https://wiki.openraildata.com/index.php/CIF_Basic_Schedule)
  This confirms the task brief's expectation of "a base schedule plus
  daily overlay/variation records" — it is real, not assumed.
- **Cadence.** A full weekly extract (Fridays) plus daily update extracts,
  per the wiki's description of the legacy `CIF_ALL_FULL_DAILY` /
  `CIF_ALL_UPDATE_DAILY` naming (community discussion on
  `openraildata-talk` confirms current JSON usage follows the same
  daily-full / daily-update split, not a weekly-only cadence in practice
  for JSON consumers). Very-short-term same-day changes are a **separate**
  feed, VSTP, not part of SCHEDULE itself.
- **Associations.** `AA` records link joining/splitting/diverging service
  pairs — relevant if this feature ever needs to reason about a service
  that changes identity mid-route, not required for a first pass.
- **Delivery mechanism — the single most important finding of this
  section.** RDM offers exactly three feed *types*: files, streaming
  (Kafka), and APIs. SCHEDULE (and CORPUS/SMART reference data, below)
  are **file-type feeds**, and per the Open Rail Data Wiki's own
  description of RDM file feeds: **"File feeds can be transferred via
  'push' options to major cloud providers (AWS, Azure, Google Cloud) or
  via SFTP, but there's no supported mechanism to retrieve files from the
  RDM on request."**
  [Open Rail Data Wiki: Rail Data Marketplace](https://wiki.openraildata.com/index.php/Rail_Data_Marketplace)
  This means CIF SCHEDULE is **neither** a `poller-*`-shaped periodic
  HTTP GET (like Knowledgebase/LDBWS/Stations/TOCs today) **nor** a Kafka
  consumer (like `trust-consumer`) — it is push-only delivery into
  infrastructure *this app would have to stand up and own*: either an
  SFTP server RDM pushes into, or a cloud storage bucket (S3/Azure/GCS)
  RDM pushes into. A third, genuinely new ingestion shape, not a
  variation on either existing one.
- **Licensing/access.** Historically free under the Open Government
  Licence via the legacy `datafeeds.networkrail.co.uk` portal; the
  RDG plans to retire that legacy National Rail Data Portal in early 2026
  (i.e., imminently/already, relative to this document's writing date),
  moving remaining feeds onto RDM. No evidence of a paid tier specifically
  for SCHEDULE was found in this pass — but per this codebase's own
  established caution (the train-tracking design doc's TRUST licensing
  section), **licence terms are per-Data-Publisher on RDM, not uniform**,
  and SCHEDULE/CORPUS are Network Rail Infrastructure Limited feeds (the
  same publisher as TRUST) — so if TRUST integration ever needs its own
  Network Rail Infrastructure Limited licence sign-off (already flagged
  as a real, separate task in the train-tracking plan), CIF SCHEDULE
  likely rides on the *same* sign-off, not a second one, since it's the
  same publisher's terms. This is a plausible inference from the existing
  research, not independently re-confirmed for SCHEDULE specifically in
  this pass — flag as an open question below.

### CORPUS/SMART reference data — closes the STANOX↔CRS/TIPLOC gap

Found while researching CIF, and directly relevant to `trust-consumer`'s
already-documented STANOX gap: **CORPUS** (Codes for Operations, Retail &
Planning – a Unified Solution) is Network Rail's own cross-reference
database linking STANOX, TIPLOC, CRS (3-alpha), NLC, and UIC codes for
every location. It's available via the same "All Reference Data" topic on
both the legacy Network Rail Open Data platform and RDM, updated nightly,
delivered as a plain-text file containing a JSON (or, per some sources,
tarred-bzip2 XML) representation.
[Open Rail Data Wiki: Reference Data](https://wiki.openraildata.com/index.php/Reference_Data)
This is also a **file-type feed** (same push-only delivery caveat as
SCHEDULE above) and, per community sources, has historically been free.

**This is good news for the feasibility of this whole line of work,
independent of whether CIF SCHEDULE itself gets adopted**: CORPUS is the
exact reference dataset `trust-consumer`'s own module docs already flagged
as the needed-but-missing piece ("source it from whatever RDM reference
product publishes the STANOX↔CRS mapping (unconfirmed — another GAP)").
This research confirms that product exists, is named, and is (per current
evidence) free — closing an "unconfirmed" flagged in this codebase's own
comments, though the exact RDM product listing wasn't independently
browsed in this pass (RDM's catalogue requires a logged-in account, same
limitation the train-tracking design doc already hit for TRUST's cost
tier).

### Does this app already have enough schedule data without CIF?

Checked `crates/poller-ldbws` and `crates/poller-stations` specifically,
per the task brief's instruction not to assume CIF is required without
checking. Findings:

- **`poller-ldbws`** samples `GetDepBoardWithDetails` per station on a
  cron and writes into `station_samples`, wholesale-replaced every poll —
  no calling-point list, no persisted per-service identity across polls,
  and `StationDeparture.headcode` is hardcoded `None` (confirmed absent
  from that RDM endpoint's schema, `crates/poller-ldbws/src/schema.rs`,
  per the train-tracking design doc's own research). This gives **zero**
  advance schedule/calling-point knowledge — only what's on a board right
  now, for a station, for up to roughly the current day's near-term
  window.
- **`poller-stations`** (not read in full for this pass, but referenced
  throughout the train-tracking docs as owning `common::StationReference`)
  provides station reference data — CRS, name, coordinates — not train
  schedules. It's the wrong layer entirely; it doesn't carry service
  patterns.
- **Conclusion: no existing feed in this app carries a full-day advance
  timetable.** Darwin/LDBWS is fundamentally a live/near-live board, not a
  schedule store. If line-level TRUST-vs-schedule diffing needs to know
  "what services are *supposed* to run on this line today, and when," CIF
  SCHEDULE (or an equivalent schedule source) really is the only
  candidate already surfaced in this app's research — there is no lighter
  internal substitute to fall back to. This matches, and reinforces, the
  train-tracking design doc's own conclusion for the individual-tracking
  feature.

## Feasibility: correlating full-feed TRUST events to line-level status

Three sub-problems, assessed independently:

**1. Matching a TRUST message to a scheduled service.** TRUST's
Activation (`0001`) message is the only one binding a `train_id` (the
day's operational identity, the join key across subsequent Movement/
Cancellation messages) to `train_uid` (the CIF schedule identity) — this
is unchanged from the train-tracking design doc's own research and
applies identically here. Unlike individual tracking (which only needs to
resolve *pinned* trains), line-level inference needs to resolve **every**
train whose schedule touches a line's stations — i.e., needs the CIF
schedule loaded first (to know which `train_uid`s are relevant to line
X today) rather than resolving opportunistically as pins arrive. This is
a materially different consumption shape from `trust-consumer`'s current
"wait for Activation, bind reactively" pattern.

**2. Placing a TRUST message's location on a line.** TRUST messages carry
STANOX (`loc_stanox`); CIF schedule locations are TIPLOC-keyed; this
app's line catalogue keys stations by CRS with an optional TIPLOC field
already populated. The natural pipeline: CORPUS resolves STANOX↔TIPLOC
(and TIPLOC↔CRS) once as a reference table; from there, both TRUST events
*and* CIF schedule rows can be placed against `lines/*.toml`'s TIPLOC
values directly. This is a real, closeable gap, not a fundamental
blocker — but it is **three feeds this feature would depend on being
correctly ingested and kept fresh** (TRUST, CIF SCHEDULE, CORPUS), each
with its own cadence and its own file-drop/Kafka-consumer plumbing, where
today's aggregator depends on exactly two (Knowledgebase, LDBWS), both
simple REST pulls.

**3. Throughput at national-feed volume.** The train-tracking design
doc's own volume research (a third-party 9-day case study) puts
unfiltered TRUST volume at ~630k messages/day, ~611k of them Movement
(`0003`) messages, with ~26,700 Activations/day (a reasonable proxy for
total scheduled trains/day nationwide). `trust-consumer` already reads
the *entire* topic today (no server-side filter) to serve a tiny,
user-pinned subset. Line-level inference would need to read the same
full volume but, unlike individual tracking, **retain and act on a much
larger fraction of it** — every Movement event whose location resolves to
any curated line's TIPLOC set, across ~20 lines today, growing toward the
50-100 DESIGN.md §10 anticipates. This is not obviously worse than
today's *read* cost (same topic, same full-feed subscription) but is a
real step up in *processing/write* volume compared to trust-consumer's
current near-total discard rate for non-pinned trains.

**Whether this can reuse `trust-consumer`'s existing plumbing**: partially.
The Kafka connection, envelope parsing (`schema.rs`), and dedup logic
(`dedup.rs`) are source-format concerns, reusable regardless of consumer
purpose. The *matching* logic (`matching.rs`, built entirely around
"does this message's `train_id` resolve to a currently-tracked pending
pin") is not reusable as-is — line-level matching needs "does this
message's location fall on any curated line's segment," a per-message
geography lookup against the `SegmentRegistry`
(`crates/aggregator/src/segments.rs`), not a lookup against a small
pinned-train set. These are genuinely different filter shapes over the
same raw stream.

## What "higher fidelity" actually buys — assessed honestly, not assumed

This is the crux of whether this is worth doing, and the place the task
brief specifically asked for honesty about a possible negative finding.

**Coverage**, in the literal sense, is real and significant: every
scheduled service touching a line's TIPLOCs, versus 3-5 sample stations.
For a long line like WCML this is a genuine, defensible improvement —
the current sample can produce no result at all for any train south of
Euston or between Preston and Carlisle whose problem never surfaces at
one of the five sampled stations, whereas a full-feed correlation would
see everything.

**Precision of delay minutes**, however, is *not* obviously better, and
this is the finding worth surfacing plainly. TRUST is explicitly a
retrospective, non-predictive record — it reports what already happened,
nothing more (confirmed in the train-tracking design doc's own research,
and re-confirmed independently in this pass: "TRUST has few prediction
capabilities — it merely reports what has just happened," Open Rail Data
Wiki: TRUST vs Darwin). Meanwhile, **Darwin — the system already behind
this app's `ldbws-inferred` path — does not treat TRUST as a poor
relation to improve upon; it consumes TRUST as one of its own primary
inputs already**, alongside Train Describer (TD, Darwin's *primary*
movement source, more granular than TRUST) and TOC customer-information
systems. This app independently confirmed via research in this pass
(not previously stated in this codebase's docs, and worth stating
plainly): "Train describer feeds are the primary source of train movement
information, with TRUST filling in the gaps... TRUST's primary purpose is
to act as a historical record... Darwin uses its own internal algorithms
to forecast arrival and departure times... Other inputs to Darwin include
a daily timetable revision, [and TOC] workstations."
[Open Rail Data Wiki: TRUST vs Darwin](https://wiki.openraildata.com/index.php/TRUST_vs_Darwin)

**The practical consequence**: this app's existing `estimated`/
`delay_minutes` fields (`StationDeparture`, sourced from Darwin via LDBWS)
are already, transitively, TRUST-informed — filtered through Darwin's own
richer TD+TRUST+human-input blend, not raw TRUST alone. A homegrown
TRUST-vs-schedule diff computed independently by this app would, for the
large majority of passenger services Darwin already actively predicts, be
re-deriving a *coarser* version of a number Darwin already computes
better — the same conclusion the train-tracking design doc's ETA section
already reached for individual trains ("Darwin already solves this
better... do not re-derive from scratch what Darwin already computes
better"), and the same shape `eta_blend.rs` encodes in real, working code:
prefer Darwin's live estimate, fall back to a naive TRUST-only
computation only where Darwin doesn't have one.

Where raw TRUST *does* plausibly add something Darwin-via-LDBWS-sampling
doesn't already give this app:

- **Coverage of stations/segments Darwin sampling never reaches** (the
  central case above) — this is real and is the strongest argument for
  proceeding at all.
- **Cancellations recorded at the moment TRUST reports them**, rather than
  waiting for the next LDBWS poll cycle to reflect it — a latency win, but
  a modest one given DESIGN.md already polls LDBWS at 30-60s granularity,
  which this product's own scope statement treats as sufficient time
  granularity for v1 ("Polling is good enough for the time granularity
  this product reports at").
- **A verifiable, timestamped factual record** (actual arrival/departure
  times) rather than a live *prediction* — genuinely different in kind
  from Darwin's forward-looking ETA, useful for anything wanting to look
  backward ("how did this line actually perform today"), which is not
  what line-status reporting currently does (DESIGN.md §2: "Out of scope
  for v1... Predicting future disruption").

**Segment precision** — the task brief's item 3 asks specifically whether
this could improve on `infer_from_samples`'s admitted lack of segment
precision (DESIGN.md §6.3: "It doesn't try to identify *which* segment of
a line is affected"). This is where full-coverage TRUST correlation is
most clearly better in kind, not just degree: knowing *which* stations
along a line are seeing delayed/cancelled TRUST events, rather than only
a line-wide aggregate rate, is a capability sampling structurally cannot
offer no matter how many sample stations are added, because
`infer_from_samples` was designed to produce one number per line, not a
per-segment breakdown. If segment-level *inferred* status (as opposed to
segment-level status only from incidents, today) is a real product goal,
TRUST-vs-schedule correlation is the only avenue in this app's current
research that could deliver it.

## Architecture options

Three genuinely different shapes, evaluated on operational complexity,
blast radius on `trust-consumer`'s already-working individual-tracking
feature, and Kafka consumer-group implications.

### Option A — Extend `trust-consumer` itself to also emit line-level signals

Add a second matching path inside the existing consumer: alongside
`matching::resolve_origin_departure` (pin-based), add a
`SegmentRegistry`-based lookup that, for every Movement/Cancellation
event whose resolved location falls on a curated line's TIPLOC set,
accumulates per-line delay/cancellation counters and periodically flushes
an aggregate to `crates/api`.

- **Pros**: one Kafka subscription, one consumer-group, no duplicate
  broker connection or duplicate full-feed read cost. Reuses the envelope
  parsing/dedup machinery directly.
- **Cons**: this is the option with the largest blast radius on a feature
  that is already real, working code with its own correctness
  requirements (idempotent writes, at-least-once delivery semantics,
  `dedup_key`-based dedup). Adding a second, unrelated responsibility to
  the same process means a bug or a resource-hungry code path in the
  line-aggregation logic can degrade or crash the individual-tracking
  path it now shares a process and a consumer-group offset sequence with.
  `trust-consumer`'s health-check semantics (`health.rs`) and its Helm
  deployment story would also need to represent two different kinds of
  "healthy," which the current design (`config.rs`, single-replica-only
  per its own doc comment on `kafka_consumer_group`) doesn't anticipate.

### Option B — A new dedicated consumer, wholly separate from `trust-consumer`

A new crate (e.g. `trust-line-aggregator` or similar) with its own Kafka
consumer group against the same Train Movements topic, running
independently of `trust-consumer`, writing its own materialized
per-line schedule-adherence data that `aggregator` reads.

- **Pros**: zero blast radius on individual tracking — a bug or outage in
  this new service cannot affect `trust-consumer`'s correctness or
  uptime. Independent scaling/restart lifecycle. Matches this codebase's
  existing preference for one-crate-per-concern (DESIGN.md §12: "one
  crate per concern... `matcher.rs` matches. `aggregation.rs` aggregates.
  Don't merge them" — the same instinct extended to service boundaries,
  not just modules).
- **Cons**: a **second** persistent full-topic Kafka consumer-group
  reading the entire national feed independently — doubling the
  ingest-side read cost this codebase would carry (Kafka consumer groups
  are independent; two groups on one topic each receive the full stream,
  they don't share reads). Also a second service with its own
  reconnect/backoff/offset-management operational surface
  (train-tracking design doc's Open Question #6 called this "real new
  operational surface" once — this option pays that cost twice).

### Option C — Hybrid: widen `trust-consumer`'s matching, keep aggregation logic in `aggregator`

`trust-consumer` gains the line/segment matching path (as in Option A),
but instead of computing line status itself, it writes a new, narrow
"schedule-adherence" table (e.g. per-line or per-segment rolling counts
of on-time/delayed/cancelled events over a trailing window) that
`aggregator` polls on its existing cycle, alongside incidents and LDBWS
samples — a third input to `aggregate()`, parallel to `infer_from_samples`,
rather than a replacement for it.

- **Pros**: keeps the *decision logic* (thresholds, severity mapping,
  escalate-not-demote posture) in `aggregator`, where DESIGN.md already
  says it belongs and where it's already tested against the real `lines/`
  catalogue (§11) — a genuinely different, better-precedented shape than
  Option A's "matching AND status-computation both inside the Kafka
  consumer." `trust-consumer` only gains a matching/write responsibility,
  not a decision one — closer in spirit to how `trust-consumer` already
  hands *individual*-train position/ETA derivation to pure functions
  (`journey.rs`, `eta.rs`) called from its own process loop rather than
  baking decisions into the Kafka-handling code directly.
- **Cons**: still carries Option A's blast-radius concern (one process,
  one consumer group, two responsibilities) unless the matching path is
  split into its own service — in which case this collapses into Option
  B with an extra hop (new service → new table → `aggregator` polls it,
  instead of new service → `aggregator`-shaped output directly).

### Recommendation among the three

**Option B, if this proceeds at all.** The blast-radius argument is
decisive given `trust-consumer` is real, currently-deploying code
serving a live feature, not a hypothetical to be casually extended — this
matches the train-tracking design doc's own explicit design principle for
`crates/aggregator` ("per-train journeys are a different read/write shape
entirely... don't belong in its `aggregate()` loop"), applied here in the
opposite direction: per-line schedule-adherence aggregation is a
different-enough concern from per-train tracking that it deserves its own
service, not a second job bolted onto trust-consumer. The doubled
full-feed Kafka read is a real cost, but it is a **known, bounded** cost
(the existing volume research already puts a number on it), whereas
blast radius on a shipping feature is an open-ended risk. If the doubled
read cost proves material in practice, that's a reason to revisit the
Kafka consumer-group design (e.g., a single shared consumer with two
downstream sinks) as a *follow-up* optimization, not a reason to accept
Option A's coupling up front.

Whichever service does the matching, it should hand `aggregator` a
per-line materialized signal to consume as a **third input alongside
incidents and LDBWS samples**, not a replacement for either — the
existing `escalate_from_sample_stats` precedent (escalate-only, never
demote, LDBWS-vs-incident) is the right template to extend rather than
reinvent: TRUST-derived stats would slot in as a comparably-weighted or
stronger signal than LDBWS samples (more coverage, but still never
overriding an active Knowledgebase incident, per DESIGN.md §6.3's
existing, deliberate rule).

## Open questions and risks — honest, not resolved here

1. **CIF SCHEDULE and CORPUS's exact RDM product listings, approval lag,
   and cost tier were not independently browsed** (RDM's catalogue
   requires a logged-in account — the same limitation the train-tracking
   design doc hit for TRUST). Historical evidence points to both being
   free under OGL via the legacy portal, and likely riding on the same
   Network Rail Infrastructure Limited licence sign-off TRUST integration
   already needs — but this is an inference from adjacent research, not a
   confirmed fact for these two products specifically. Confirm directly
   before committing engineering time.
2. **File-feed push delivery is a new operational commitment this app has
   never taken on**: this app would need to stand up and maintain either
   an SFTP endpoint or a cloud storage bucket that RDM pushes into, plus
   a component that watches that destination for new full/update
   extracts and ingests them — materially different infrastructure from
   both the existing `poller-*` HTTP-GET pattern and `trust-consumer`'s
   Kafka-consumer pattern. This is a genuinely new category of moving
   part, not a variant of something already running.
3. **Whether Darwin's TRUST-derived predictions are "good enough" is an
   empirical question this research cannot settle from documentation
   alone.** The finding that Darwin already fuses TD+TRUST+human input is
   solid, but whether the *specific gap* this feature would fill
   (coverage on unsampled stations/segments) is big enough in practice to
   justify three new data-feed dependencies can only be tested against
   real running data, not reasoned out in the abstract. If this proceeds,
   validating that gap concretely (e.g., comparing LDBWS-sampled status
   against a manually-checked TRUST-vs-schedule diff for a handful of
   real disruption days) before committing to full ingestion would be a
   cheap way to de-risk the "is this actually worth it" question before
   building three new ingestion pipelines.
4. **STANOX↔TIPLOC↔CRS correlation via CORPUS is a plausible closer of
   `trust-consumer`'s existing documented gap, but untested against real
   data in this pass** — CORPUS's existence and shape are confirmed by
   documentation, but this research did not (and could not, without RDM
   access) verify completeness or currency of that mapping against this
   app's specific `lines/*.toml` TIPLOC values.
5. **National-feed sustained-volume experience is still zero** for this
   app in practice — the design doc's 9-day third-party case study is the
   only volume evidence available anywhere in this codebase's research,
   and `trust-consumer` itself has, per this session's own history,
   only recently begun connecting to a live feed at all. Any volume
   estimate for a widened, line-level consumer (Option B) inherits that
   same "never actually sustained" uncertainty, doubled.
6. **The "no invented API details" convention applies to CIF's own
   uncommon-path records** (e.g. Association records, freight-service
   category stripping in the OGL-open extract, or `0005`/`0008`'s
   still-unconfirmed TRUST message shapes) exactly as it did for TRUST —
   nothing in this document should be read as a confirmed field-level
   schema for implementation; that level of detail needs the real feed
   subscriptions in hand.

## Recommendation

**Proceed with caveats — but not yet, and not as a straightforward
"replace/augment sampling" project.**

The honest picture this research surfaced: the strongest, clearest case
for this feature is **segment-level inferred status**, a capability
`infer_from_samples` cannot deliver at any sample size because it's
architected to produce one number per line. That is a genuine capability
gap, not a marginal precision improvement. The weaker, more assumption-
laden case — "TRUST gives materially better delay-minute precision than
LDBWS sampling already does" — does not hold up well under scrutiny: this
app's own `ldbws-inferred` signal is already a downstream consumer of
Darwin, which already fuses TD (Darwin's *primary* movement source, finer
than TRUST) and TRUST together, and this codebase's own `eta_blend.rs`
precedent already encodes "prefer Darwin's live estimate over TRUST-only
propagation" as the right call for individual trains. A homegrown line-
level TRUST diff would, for the majority of well-sampled passenger
services, likely re-derive a coarser number Darwin/LDBWS sampling already
gives this app today.

Given that, the case for proceeding rests almost entirely on **coverage**
(every scheduled service vs. a handful of sample stations) and
**segment precision** (a real, structural gap sampling cannot close), not
on delay-minute accuracy. Both are real enough to be worth pursuing
eventually, but the cost side is also real and larger than DESIGN.md's
one-line "post-v1, optional" framing implies: **three** new data-feed
dependencies (TRUST at wider scope, CIF SCHEDULE, CORPUS), a **new
file-push ingestion shape** this app has never built, and (per the
architecture recommendation above) a **second full-national-feed Kafka
consumer group** running independently of `trust-consumer`.

**Concrete recommendation**: don't start full implementation yet. First,
(a) independently confirm CIF SCHEDULE's and CORPUS's actual RDM listing,
approval lag, and licensing terms (open question #1) — this alone could
change the calculus if either turns out to require separate paid
licensing or a slow manual-approval process, the same way TRUST's own
approval lag is still unconfirmed; and (b) once real TRUST access exists
in production (which the train-tracking feature is already driving),
spend a small, cheap validation pass comparing real LDBWS-sampled line
status against a manual/spot-check TRUST-vs-schedule read for a handful
of actual disruption days, specifically targeting the segment-precision
question — if that validation shows segment-level TRUST inference would
have caught real disruptions the current sampling missed or
under-attributed, that's the concrete justification to greenlight Option
B as a scoped follow-up project. Building three new ingestion pipelines
on the strength of documentation research alone, without that empirical
check, risks repeating DESIGN.md's own already-stated lesson about CIF
service groups not mapping cleanly onto passenger lines — this time for
delay inference rather than line definition, but from the same
underlying evidence gap: real coverage/accuracy value that has never been
measured against this app's real curated catalogue.
