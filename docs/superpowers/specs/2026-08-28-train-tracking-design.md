# Individual Train Tracking — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md` and
`docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`) so
it can be reviewed and iterated on the same way, but it has not gone
through implementation planning and nothing here is committed. It does
**not** contain a task-by-task implementation plan — that is a separate,
later step in this repo's process, done only after a design like this has
been reviewed.

## Problem

Everything this app does today is **line-level aggregate** status.
DESIGN.md §1 states the goal as TfL-shaped `/Line/.../Status` responses,
and §2 explicitly lists "Train-level live tracking (that's TD/TRUST
territory; we stay at line granularity)" as **out of scope for v1**. The
data model reflects that scope choice everywhere:

- `common::LineStatusReport` / `LineStatus` (`crates/common/src/lib.rs`)
  are keyed by line id, not by train.
- `poller-ldbws` samples `GetDepBoardWithDetails` per station on a cron
  and writes into `station_samples`, which is **wholesale-replaced on
  every poll** — "No history table for `station_samples` ... matching the
  existing table's design" (`docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md`,
  Global Constraints). Nothing persists any individual `service_id`
  across polls or across the stations it calls at.
- `common::StationDeparture` (`crates/common/src/lib.rs:350-371`) is a
  point-in-time snapshot of one departure at one station: `service_id`,
  `scheduled`/`estimated` (both bare `HH:MM` strings, not tied to a
  calendar date beyond "today"), and an optional `headcode` that is in
  fact **always `None`** from the RDM `GetDepBoardWithDetails` call this
  app uses — confirmed absent from that endpoint's schema entirely
  (`crates/poller-ldbws/src/schema.rs:101`, `:125`). There is no
  persistent identity a caller could use to ask "show me everything that
  happened to this one train."

The user's ask — "how did *this specific 18:32 Waterloo–Woking service*
actually perform, minute by minute, along its whole route, and where is
it right now" — cannot be expressed by anything currently in the schema.
It requires an identity-preserving, whole-journey record per train, which
is a materially different read/write shape from the existing per-line
`line_status` table (DESIGN.md §4) and the existing per-station
`station_samples` table.

## Goals

1. Let a user (or the system) mark interest in one specific train service
   on one specific day ("tracking").
2. Persist that train's full journey as a historical record: its
   scheduled calling points, and the actual/estimated event (arrival,
   departure, pass, cancellation) recorded at each one.
3. Derive **current status and position in journey** for a tracked train
   from Network Rail's TRUST movement feed, per the user's explicit
   direction.
4. Derive **ETAs** for the tracked train's remaining calling points, to
   the extent that's honestly achievable — blending TRUST with Darwin's
   own estimates rather than re-deriving from scratch where Darwin
   already does it better.
5. Fit this into the existing crate/service architecture wherever
   possible, and call out explicitly where it can't (this is the single
   biggest architectural departure this codebase would take).
6. Get the licensing story right for a **different** Network Rail data
   publisher than the one already covered
   (`frontend/components/OpenDataAttribution.tsx`) — don't assume TRUST's
   terms mirror NRE's.

## Non-goals (this pass)

- **Train Describer (TD) / berth-level physical position.** Investigated
  and deliberately not recommended for v1 — see "Data source
  recommendation" below.
- **A trained/ML ETA model.** The ETA approach here is a simple,
  explainable propagation-plus-blend, matching this codebase's existing
  preference for simple, auditable heuristics over ML where a heuristic
  is honest about its own limits (DESIGN.md §6.1's severity classifier
  takes the same posture).
- **Auto-tracking every train nationwide.** Out of scope for the reasons
  covered under "Tracking semantics" — this is opt-in/scoped tracking,
  not a full national movement mirror.
- **Frontend UI design.** Only sketched at a high level; a UI-focused
  follow-up design doc (in the style of
  `docs/superpowers/specs/2026-07-07-frontend-design.md`) should cover
  the actual page/interaction design once this data-layer design is
  reviewed.
- **An implementation plan.** See the status note at the top.

## Research summary: TRUST, TD, and RDM

### TRUST movement feed

TRUST (Train RUnning System TOPS) is Network Rail's system for comparing
actual train movement events against the planned schedule, built on the
TOPS mainframe lineage, and used to record delay attribution for the
industry's performance/incentive regime
([Wikipedia: TRUST](https://en.wikipedia.org/wiki/TRUST)). The **Train
Movements** feed exposes TRUST's events as JSON messages, historically
delivered over STOMP from `feeds.networkrail.co.uk`/NROD on topic
`TRAIN_MVT_ALL_TOC`
([Open Rail Data Wiki: Train Movements](https://wiki.openraildata.com/index.php/Train_Movements)),
and **it is now also offered on the Rail Data Marketplace (RDM)** as a
streaming product — RDM's streaming feeds are delivered over **Apache
Kafka only** ("STOMP, AMQP and OpenWire are unavailable" on RDM). A
real user in the `openraildata-talk` community confirms live production
use: "subscribed to about 20 out of curiosity but only using 1 actively
(train movements, ... so far 150,000 activations recorded and counting)."
Network Rail's own TD feed is confirmed reachable the same way — one
community member describes "signing up and subscribing to NWR Train
Describer (TD) data via the RDM."

**Message shape** (confirmed field-by-field where cited): the feed is a
JSON list of `{header, body}` messages, sent every 5 seconds or on
batches of 32, all timestamps in epoch milliseconds
(Open Rail Data Wiki: Train Movements). Eight message types exist. Five
are independently confirmed by name/fields in this research pass:

| `msg_type` | Name | Key fields |
|---|---|---|
| `0001` | Train Activation | `train_id`, `train_uid`, `toc_id`, `train_service_code`, `schedule_wtt_id`, `schedule_start_date`, `schedule_end_date` — **the only message type that links a TRUST signalling identity (`train_id`) to a CIF schedule (`train_uid`)** |
| `0002` | Train Cancellation | `canx_timestamp`, `canx_reason_code`, `canx_type` (`"EN ROUTE"` / `"AT ORIGIN"`) |
| `0003` | Train Movement | `event_type` (arrival/departure/pass), `gbtt_timestamp`, `planned_timestamp`, `actual_timestamp`, `current_train_id`, `reporting_stanox`, `loc_stanox`, `toc_id`, `variation_status` |
| `0006` | Change of Origin | sent when a train starts from a location other than its schedule's first location |
| `0007` | Change of Identity | freight/engineering trains only, when the train's reporting class changes mid-journey |

`0005` and `0008` were **not** independently confirmed by name/fields in
this pass (community summaries commonly list "Unidentified Train" and
"Change of Location" as the remaining two, but that is not verified
against a primary source here). Per this codebase's existing convention
of not inventing API details
(`docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md`'s "No
invented API details" constraint), treat these two as unconfirmed and
resolve them against RDM's actual schema/spec once a subscription exists,
not by guessing.

**Identifiers, and which is authoritative for what:**

- **`train_uid`** (e.g. `"C21373"`) — the CIF/schedule UID. Identifies a
  *service pattern* across its whole timetable validity period (weeks to
  months), not a single day by itself; a specific day's instance is
  `(train_uid, date)`. This is the closest thing to a stable, authoritative
  train identity in the whole ecosystem, and the natural primary key for
  "one journey."
- **`train_id`** — TRUST's own 10-character daily identifier: the first
  two digits are from the origin STANOX, the next four are the headcode,
  and the remainder encodes day-of-month. Community guidance is that this
  is unique "in any one month," so a durable key needs month/year appended
  by the consumer. `train_id` is what movement/cancellation/etc. messages
  actually carry after activation — **it is the join key across TRUST
  messages for one day's journey**, but only Activation (`0001`) tells you
  which `train_uid` it corresponds to. Miss the Activation message for a
  given `train_id` and the rest of that train's movement events can't be
  attributed to a schedule.
- **headcode** (4 chars, e.g. `"1U24"`) — embedded inside `train_id`, and
  the field this codebase already stores on `StationDeparture.headcode`
  (`crates/common/src/lib.rs:364-366`) and matches lines against via
  `LineDefinition.headcode_prefixes`. **Not globally unique at a point in
  time** — unique only per signalling area, so two unrelated trains in
  different parts of the country can carry the same headcode on the same
  day. Freight services generally don't carry a Darwin-visible headcode at
  all; it has to be extracted from `train_id` instead.
- **Darwin `RID`** — a third, independent identifier minted by Darwin's own
  engine, not derivable from `train_uid` or `train_id` by any documented
  transform found in this research. This app's LDBWS integration doesn't
  currently capture it at all: `schema.rs`'s `service_id` maps to RDM's
  opaque `serviceID`, which is "an opaque token for chaining into
  `GetServiceDetails`, not a Darwin headcode/trainid"
  (`docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md`, Global
  Constraints), and `headcode` on that same struct is hardcoded `None`
  because RDM's `GetDepBoardWithDetails` doesn't expose it
  (`crates/poller-ldbws/src/schema.rs:101`).

  **Correlating a TRUST journey to a Darwin/LDBWS observation of the same
  real-world train is therefore a heuristic, not a guaranteed join** —
  practically, `(date, headcode, origin CRS, scheduled origin departure
  time)` should disambiguate in the vast majority of cases (headcode
  collisions require *both* same day *and* overlapping signalling areas,
  which the small, user-pinned tracked-train set makes rarer still — see
  Tracking semantics below), but this is a real risk to call out, not a
  solved problem — see Open Questions.

### TD (Train Describer) — investigated, not recommended for v1

TD gives berth-to-berth stepping data: "low-level detail about the
position of trains ... through a network of berths," each usually (not
always) corresponding to a signal, with C-Class messages for berth steps
and S-Class messages for signalling-equipment state
([Open Rail Data Wiki: TD](https://wiki.openraildata.com/index.php?title=TD)).
This is materially finer-grained than TRUST's schedule-location events —
it can show a train's physical position between measuring points, not
just "arrived/passed/departed location X."

The cost is real complexity TRUST doesn't have. TD has **no built-in
train-identity binding**: a berth step reports a *headcode* stepping
between berths within one signalling area's topic, and translating that
into "this is train_uid C21373's current position" requires (a) the
SMART database to translate berth → physical location, (b) hand-following
a headcode's steps across berths within an area, and (c) re-identifying
the same physical train across signalling-area boundaries by timing/
headcode-continuity heuristics, since a headcode can be reused by an
unrelated train once the first one clears an area. TRUST's Activation
message, by contrast, does the identity-binding work for you as a single
authoritative event.

Given the user's stated requirement is "current status and position in
journey" — i.e., "where is this train relative to its scheduled calling
points" — **TRUST alone already answers that** at the granularity a
passenger-facing feature needs (which station/calling-point was it last
seen at, how late, what's next), without TD's berth-map/stepping-table
domain knowledge and cross-boundary re-identification problem. TD would
only add sub-station physical position (e.g. "between Woking and
Basingstoke, roughly here"), which isn't asked for and is a meaningfully
harder, differently-shaped system (closer to the TD/berth complexity this
app's own DESIGN.md §2 already named as out of scope: "Train-level live
tracking (that's TD/TRUST territory...)"). **Recommendation: TRUST only
for v1; treat TD as an explicit future option if sub-station physical
position ever becomes a real requirement**, not as part of this design.

### Darwin `GetServiceDetails` — a complement, not a spine

`GetServiceDetails` (keyed by the same board-relative `serviceID` this
app already ingests) returns the fuller calling-point list plus Darwin's
own predicted times for one service — but it is only available "while the
particular service is showing on the station departure board, normally
for up to two minutes after the service is expected to depart." That
short, board-relative availability window makes it unusable as the
backbone of a durable, poll-ahead-of-time or replay-after-the-fact
historical store: you can't look up `GetServiceDetails` for a train that
departed an hour ago, or that hasn't reached a station's board window yet.
It's a good **live-lookup enrichment** source (particularly for ETAs — see
below) while a tracked service is actively on a board somewhere, not a
substitute for TRUST as the system of record for journey history.

### RDM as a front door — real leverage, with one important caveat

RDM genuinely is "one front door" in the sense this app already relies
on: one account, one catalogue, credentials issued per-product. That part
of the existing pattern *does* carry over. Two things do **not** carry
over automatically, and both matter for planning:

1. **Access protocol differs by product type.** This app's four existing
   RDM integrations (Knowledgebase, LDBWS, Stations, TOCs) are all
   simple REST pulls with an API key header (`x-apikey`, per
   `crates/poller-ldbws/src/config.rs`'s `rdm_api_key` doc comment) —
   which is exactly why the "poller crate on a cron" pattern works for
   them. **Train Movements (and TD) are Kafka-only on RDM.** That's not
   an HTTP GET a cron job can make; it's a persistent broker connection
   with SASL-style consumer credentials (`kafka-client-rdm-darwin`, the
   closest official reference client, uses a "Consumer key"/"Consumer
   secret" pair copied into the client, not an `x-apikey` header), a
   consumer group, offset tracking, and reconnect/backoff handling. This
   is the single most architecturally significant finding in this
   research: reusing the *RDM account* is real leverage; reusing the
   *poller-crate-per-cron-poll code shape* is not.

2. **Licence terms are per-Data-Publisher, not uniform across RDM.**
   RDM's own contract terms state that "the Data Publisher is inviting
   the Data Consumer to enter into a licence for that Content, the terms
   of which depend on the Data Publisher's approach to licensing" (Open
   Access / RDM's Licence-Builder / a bespoke publisher licence subject
   to RDG approval), and RDG itself "is not a party to the Data Sharing
   Agreement." Knowledgebase/LDBWS/Stations/TOCs are all published by
   **National Rail Enquiries** and already covered by the attribution
   work in `frontend/components/OpenDataAttribution.tsx` (NRE Terms &
   Conditions v3.0, Developer Guidelines v06.01 §4). **TRUST and TD are
   Network Rail's own operational systems, published under Network Rail
   Infrastructure Limited's own terms** — confirmed distinct and, per a
   prior research pass already reflected in this app's attribution
   component's doc comment, carrying the *opposite* posture from NRE's:
   Network Rail's open-data-feeds terms page states plainly "You may not
   use our brand or logo or those of any of our partners including
   National Rail and the train companies," and that applications must
   not call themselves "official," "as this would mislead the public"
   ([Network Rail: Open data feeds](https://www.networkrail.co.uk/who-we-are/transparency-and-ethics/transparency/open-data-feeds/),
   fetched 2026-08-28, no publication/last-updated date visible on the
   page itself — re-verify wording at integration time). The same page
   states registration is "first come, first served" with "a current
   limit of 1,000 users" and no mention of any fee — treat the numeric
   cap as a real, currently-stated figure from Network Rail's own summary,
   but flag it as possibly superseded by RDM's now-1,200+-organisation
   scale reported elsewhere; don't rely on it as current without
   re-checking at integration time.

   **Practical consequence:** subscribing to Train Movements on RDM is a
   **separate licence acceptance** from the NRE agreement already in
   place for this app's four existing feeds, even though it's the same
   RDM account. And **the existing "Powered by National Rail Enquiries"
   attribution pattern must not be copied verbatim for this feature** —
   a Network Rail-sourced attribution should be unbranded (no logo,
   no NR/NRE/TOC marks) and must not describe the feature as "official."
   A plain factual line ("Live train movement data from Network Rail's
   open data feeds") is the licence-consistent approach; the exact
   wording needs its own sign-off pass against Network Rail's current
   terms before shipping, the same way the NRE wording was independently
   verified before `OpenDataAttribution.tsx` was written.

**Approval lag, cost tier:** neither was confirmed in this research pass.
The Open Rail Data Wiki notes some RDM products are manual-approval;
no SLA or turnaround figure was found. No current price was found for
Train Movements specifically (RDM does have chargeable products
elsewhere, e.g. Fares APIs' £50/1,000-call tier, but Train Movements has
historically been part of Network Rail's free open-data family and
community evidence suggests it's still freely subscribable). **Both are
open risks, not assumptions** — see Open Questions.

## Data source recommendation

**TRUST alone, accessed via RDM's Kafka Train Movements product**,
supplemented at read/display time by this app's existing Darwin/LDBWS
data for ETAs where available. Reasoning, restated concisely:

- TRUST's Activation + Movement + Cancellation + Change-of-Origin/Identity
  messages together already give everything the stated requirements ask
  for: journey history (goal 2), current status/position relative to
  scheduled calling points (goal 3), and a base for ETA propagation
  (goal 4).
- TD adds real value only for sub-station physical position, which isn't
  asked for, at a real cost (berth-map domain knowledge, cross-boundary
  re-identification) this design doesn't need to take on.
- RDM is confirmed as a viable, currently-used path for Train Movements
  (and TD, if ever needed later) — so this is not "STOMP client vs. RDM,"
  it's "RDM Kafka client" either way. The user's instinct to prefer RDM
  for architectural consistency with this app's existing feeds is
  correct in that it's the same *account and catalogue*; it is incorrect
  only insofar as it doesn't mean the same *cron-poll code shape* — a new
  persistent-connection service pattern is unavoidable regardless of
  which door TRUST is reached through, RDM or legacy NROD.
- Darwin `GetServiceDetails`/LDBWS remains valuable as a **secondary,
  best-effort ETA source** blended in, not as the identity spine (see
  ETA approach below) — it is unsuitable as a spine given its ~2-minute
  board-relative availability window.

## Train identity

`(train_uid, service_date)` is the **authoritative** identity for one
journey — it's what a user is conceptually pinning ("the 18:32 Waterloo–
Woking"), and it's stable across the whole day regardless of how TRUST's
`train_id` or Darwin's `RID` label it internally.

`train_id` (TRUST's daily 10-char identifier, `train_uid`-scoped via the
Activation message) is the **operational** join key for correlating
TRUST messages to each other within a day — every Movement/Cancellation/
Change-of-* message after Activation is filtered and appended by
matching `train_id`, not `train_uid` directly (TRUST doesn't repeat
`train_uid` on every message type).

headcode is **not** an identity by itself — only a matching/display hint,
consistent with how this app already treats it purely as a
`headcode_prefixes` matching aid on `LineDefinition`
(`crates/common/src/lib.rs:431`, `:556`), never as a primary key.

Darwin `RID`/`serviceID` correlation to a TRUST-identified train is
**best-effort**, keyed on `(date, headcode, origin CRS, scheduled origin
departure time)` — sufficient in practice for this app's scoped tracking
population, but explicitly not a guaranteed exact join (see Open
Questions).

## Tracking semantics

**What triggers tracking:** user-initiated pin only, for v1. A user
viewing a departure they already see today (this app already surfaces
`StationDeparture.service_id` per station) marks it as tracked. This
naturally reuses an identifier this app already threads through the UI,
though note it's RDM's `serviceID` (board-relative, LDBWS-side), not a
TRUST `train_id`/`train_uid` — tracking creation needs to resolve the
pinned service to `(train_uid, date)` at pin time (e.g. via CIF schedule
lookup by headcode/origin/time, or a same-day cross-reference once TRUST
Activation for it arrives) rather than store the ephemeral `serviceID`
as if it were durable.

A system-level "auto-track configured routes" mode is a plausible later
extension (e.g. always track the first/last train of the day on a
curated line) but is **deliberately not proposed for v1** — it multiplies
the volume problem covered below for no requirement currently stated,
and should be revisited only once real usage patterns from user-pinned
tracking are known.

**What gets stored:** for each tracked `(train_uid, date)`:
- The planned journey (calling points, scheduled times), seeded
  immediately at tracking time from CIF schedule data — independent of
  whether TRUST has activated the train yet, so a user can track a train
  before it even starts running.
- Every subsequent TRUST message matched to that train's `train_id`
  (movement/cancellation/change events), appended as an immutable event
  log — this is the "historical store."
- A denormalized "current state" row (last known location, delay,
  cancelled/not, next scheduled calling point, best-available ETAs) kept
  up to date as events arrive, so reads don't have to replay the whole
  event log every time (mirrors how `line_status` is a materialized table
  the aggregator writes rather than something recomputed per request,
  DESIGN.md §4).

**Retention:** no existing retention policy elsewhere in this repo to
point to (flagged as an open question, not invented here) — propose a
bounded window (e.g. 90 days) with a periodic prune job on the event-log
table, keeping the denormalized "current state" summary rows longer/
indefinitely since they're cheap and are the more useful long-term
record ("how did this train perform that day").

**Volume sanity check:** a third-party pipeline case study ingesting the
full national TRUST feed over 9 days recorded **5.7M messages total**
(~630k/day), dominated by Movement (`0003`) messages at 5.5M over 9 days
(~611k/day), with **~240,300 Activations over 9 days (~26,700/day)** — a
reasonable proxy for total scheduled trains/day nationwide — and
cancellations/change-of-origin/etc. in the low thousands or hundreds per
9 days. That's the **unfiltered national volume**; this design does not
propose ingesting all of it, or any coarser proxy for it (an earlier
draft of this section proposed also matching this app's curated line/
station catalogue as a secondary allowlist — the same
`dedup_sample_stations` set `poller-ldbws` already computes,
`crates/api/src/data/samples.rs` — but that's explicitly rejected: it
would silently track every train touching a curated line, not just
user-targeted ones, contradicting this design's own Non-goal above). The
filter is exactly one thing: **does this message's `train_id` resolve
to a currently-tracked `(train_uid, date)`.** Nothing else qualifies a
train for storage.

Given that, only *tracked* trains are persisted past the correlation
step, so stored volume should be orders of magnitude smaller than the
national figures above: tens to low hundreds of tracked trains at once
is plausible for a v1 user base, each producing on the order of 10-30
movement events across its journey (one per scheduled calling point plus
activation/cancellation), i.e. low thousands of stored rows per day, not
hundreds of thousands. The **live ingest-side** cost is the real number
to watch regardless of how narrow the stored set is: even a `train_uid`-
only filter happens in-process (Kafka has no server-side content
filter), so the consumer still receives and inspects the full ~600k
messages/day to find the ones worth keeping — narrowing the *stored*
scope to user-pinned trains only does not reduce the *ingest-side* read
volume, only the write volume and the storage footprint.

## Position-in-journey derivation

Because Activation binds `train_id` to the day's CIF schedule (its
ordered list of scheduled locations with scheduled arrival/departure/
pass times), "position in journey" is a straightforward sequence-walk
over that schedule, advanced by each subsequent Movement message:

1. Seed the ordered calling-point list from CIF at tracking time (or at
   Activation, if tracking started before Activation).
2. On each Movement (`0003`) message, mark that scheduled location as
   "reported" with its `event_type` (arrival/pass/departure),
   `actual_timestamp`, and `variation_status` (on time / early / late,
   with minutes).
3. "Current position" = the last calling point marked reported; "next"
   = the immediate next unreported scheduled calling point.
4. Cancellation (`0002`) marks all remaining unreported calling points as
   cancelled rather than pending. Change of Origin (`0006`) adjusts which
   locations are even in play from that point forward.

No TD/berth data is needed for this — it's exactly the granularity TRUST
already reports at, and matches what a passenger-facing "where is my
train relative to its stops" feature actually needs.

## ETA approach

TRUST messages are **retrospective, not predictive** — every message
reports an event that has already happened, with a real timestamp; TRUST
does not push forward-looking estimates for locations not yet reached.
So any TRUST-only ETA has to be derived, and the honest approach is
simple propagation: take the delay (`variation_status`) observed at the
train's last-reported location and apply it uniformly forward across the
remaining scheduled times. This is cheap and always available, but coarse
— it ignores timetabled recovery/pathing time and can't anticipate a
train catching up or losing more time later in the journey.

Darwin already solves this better for services it's actively predicting:
this app already ingests Darwin's `estimated`/`etd` field and computes
delay from it (`crates/poller-ldbws/src/schema.rs`'s
`compute_delay_minutes`), and `GetServiceDetails` (while the service is
within its ~2-minute board-relative availability window) returns Darwin's
own live predicted times for the *rest* of that service's calling points.
**Recommendation: prefer Darwin's estimate wherever it's available for
the same real-world train (via the best-effort correlation above),
falling back to the naive TRUST-delay-propagation for any tracked
train/remaining-stop combination Darwin doesn't currently have a live
estimate for** (outside the `GetServiceDetails` window, or a
non-passenger/freight service Darwin doesn't carry at all). Do not
re-derive from scratch what Darwin already computes better.

**Confidence:** be explicit in the data model and to users that these are
two different quality tiers, not one number — consistent with this app's
existing `dataQuality` philosophy of surfacing signal provenance rather
than collapsing it (DESIGN.md §5.5, which already distinguishes
`knowledgebase` / `planned` / `ldbws-inferred` / reserved
`trust-inferred`). A `trust-propagated` vs. `darwin-estimated` distinction
on any ETA field this feature emits would extend that same pattern
naturally rather than inventing a new one.

## Architecture sketch

```
┌───────────────────────────────────────────────────────────────┐
│  RDM Kafka: Train Movements topic (persistent connection)      │
└───────────────────────────────┬─────────────────────────────────┘
                                 │ consumer group, at-least-once delivery
                                 ▼
                  ┌──────────────────────────────┐
                  │  poller-trust (new crate)     │
                  │  - long-running Kafka consumer│
                  │    (NOT a cron poll — see     │
                  │    naming note below)         │
                  │  - filters to EXACTLY: a       │
                  │    currently-tracked            │
                  │    (train_uid, date). No       │
                  │    broader allowlist -- see    │
                  │    Volume sanity check above.  │
                  │  - Activation → resolves/binds │
                  │    train_id -> train_uid       │
                  │  - Movement/Cancellation/etc.  │
                  │    -> appends event, updates   │
                  │    position/ETA                │
                  └───────────────┬────────────────┘
                                  │ POST, X-Internal-Token
                                  ▼
                  ┌──────────────────────────────┐
                  │  crates/api                    │
                  │  - new private_router() routes:│
                  │    POST /private/train-events  │
                  │    (write-through to Postgres) │
                  │  - new public routes:          │
                  │    GET /Train/{uid}/{date}     │
                  │    POST /Train/{uid}/{date}/track│
                  │  - new tables (new migration,  │
                  │    next after                  │
                  │    20260822120000_...):        │
                  │    tracked_trains,              │
                  │    train_movement_events        │
                  └──────────────────┬──────────────┘
                                     │
                                     ▼
                        (frontend: new page/route,
                         separate design doc)
```

**What extends existing patterns:**
- Ingestion auth: the same `X-Internal-Token` gate every other poller
  uses (`crates/api/src/auth.rs`'s `require_internal_token`, mounted in
  `crates/api/src/routes/mod.rs`'s `private_router()`).
- Migration convention: timestamp-prefixed SQL under
  `crates/api/migrations/`, sorting after the latest existing one
  (`20260822120000_line_status_source.sql`).
- Materialize-don't-recompute: a denormalized "current state" row per
  tracked train, mirroring `line_status` being a table the aggregator
  writes rather than something computed per API request (DESIGN.md §4).
- Config shape: `clap` + `#[arg(env)]`, matching every existing poller
  (`crates/poller-ldbws/src/config.rs`), for whatever *is* still
  environment-configurable (Kafka broker address, consumer key/secret,
  topic name, internal token, api base URL).
- `dataQuality`-style provenance tagging on ETAs, extending DESIGN.md
  §5.5's existing philosophy rather than inventing a new one.

**What does NOT extend existing patterns, and is the real architectural
addition here:**
- **A persistent-connection service.** Every current poller
  (`poller-incidents`, `poller-ldbws`, `poller-stations`, `poller-tocs`,
  `poller-tfl`) is a cron-style loop making periodic HTTP calls
  (DESIGN.md §4: "Why no streaming for v1... The Network Rail STOMP feeds
  are powerful but operationally heavy (24/7 consumers, backpressure
  handling). Defer until there's a concrete need."). This feature *is*
  that concrete need. A Kafka consumer needs: startup connection with
  retry/backoff, consumer-group offset management, "at-least-once"
  delivery semantics (meaning the write path into Postgres must be
  idempotent — dedupe on some stable event key, not assume each message
  arrives exactly once), and a different health-check shape (connected/
  lagging/disconnected, not "last poll succeeded at T"). Nothing in this
  codebase does this today. The closest existing precedent for "a new
  long-running consumer crate with its own deploy unit" is
  `crates/enricher`
  (`docs/superpowers/plans/2026-08-20-incident-nlp-extraction.md`), which
  consumes a Redis Stream rather than an external Kafka topic — same
  *shape* (long-running process, consumer-group semantics, writes via
  `crates/api`'s ingestion pattern), different *source* (internal queue
  this codebase controls vs. an external broker it doesn't). Redis
  already exists in this stack's `docker-compose.yml` for `enricher`; it
  could optionally be reused as an internal dedup/buffer layer between
  the raw Kafka consumer and the Postgres writer (mirroring `enricher`'s
  `incident-text-changed` stream), but that's an implementation-level
  choice, not required by this design.
- **`crates/aggregator` is deliberately NOT extended.** Its job is
  line-level aggregation over incidents + station samples (DESIGN.md §6);
  per-train journeys are a different read/write shape entirely (one row
  per tracked train, not one row per line) and don't belong in its
  `aggregate()` loop. Position/ETA derivation should live as its own pure
  function (structured the way `crates/aggregator/src/matcher.rs` is pure
  and independently testable), called either from `poller-trust` at
  ingest time (denormalize on write, preferred per the
  materialize-don't-recompute point above) or from `crates/api` at read
  time — an implementation-level choice, not a design-level one.
- **Deploy/ops.** A Kafka-consuming service needs its own entry in
  whatever the Helm chart precedent covers
  (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`) with
  different liveness semantics than the existing pollers' cron-shaped
  containers — not designed here, flagged for that doc's owner to extend.
- **Naming.** Calling this crate `poller-trust` for consistency with the
  `poller-*` family is a real naming tension, since it doesn't poll
  anything — it holds a persistent connection. Worth resolving
  explicitly (`stream-trust`? `trust-consumer`?) rather than defaulting
  to the existing prefix out of habit. Left as an open question.

**Frontend:** a new page (sketched only: something like
`/Train/{uid}/{date}`) showing the journey timeline, current position,
delay, and ETAs for a tracked train, reachable from a "track this train"
action on the existing per-station departure UI. `OpenDataAttribution.tsx`
needs a new, unbranded, non-logo attribution line for Network Rail's own
feeds once this ships (see Licensing above) — full UI/interaction design
deferred to its own follow-up spec, the way
`docs/superpowers/specs/2026-07-07-frontend-design.md` did for the
original line-status UI.

## Open questions / risks

1. **RDM approval lag for the Train Movements (and, if ever needed, TD)
   product is unconfirmed.** No SLA or typical turnaround was found in
   this research pass. Start the subscription/approval process early —
   it gates every downstream task — and don't assume it's as fast as this
   app's existing REST-product subscriptions.
2. **Cost tier for Train Movements is unconfirmed.** Historically part of
   Network Rail's free open-data family; RDM does have chargeable
   products elsewhere. Confirm directly in the RDM catalogue (requires a
   logged-in account) before committing to always-on ingestion.
3. **Licence sign-off is a genuinely separate task from the existing NRE
   review**, since Train Movements/TD are published under Network Rail
   Infrastructure Limited's own terms, not NRE's. Don't let the existing
   NRE Ts&Cs review stand in for it. Re-verify the exact current wording
   of the no-logo/no-"official" clause at integration time — the page
   cited here had no visible last-updated date, and the "1,000 users"
   cap it states may be stale.
4. **Two of TRUST's eight message types (`0005`, `0008`) are unconfirmed**
   by name/fields in this research pass. Resolve against the real
   schema/spec once RDM access exists, per this codebase's "no invented
   API details" convention.
5. **Darwin↔TRUST correlation is heuristic, not a guaranteed join.** The
   `(date, headcode, origin CRS, scheduled origin time)` approach is
   expected to work in the large majority of cases for this app's scoped
   tracking population, but headcode reuse across signalling areas and
   Darwin's own RID being independently minted mean it can silently
   mismatch. This directly affects ETA-blending quality — treat any
   Darwin-sourced ETA on a TRUST-tracked train as provisional/best-effort,
   not as a confirmed join, and consider surfacing that in the
   `dataQuality`-style provenance tag.
6. **A persistent Kafka-consumer service is operationally new for this
   codebase** — reconnect/backoff, consumer-group offsets,
   at-least-once-delivery idempotency, and different health-check
   semantics than every existing poller. This is real new operational
   surface, not a copy-paste of an existing poller crate.
7. **Data volume/retention costs beyond the rough order-of-magnitude
   estimate above are unknown** until real tracking usage exists. No
   existing retention policy elsewhere in this repo to anchor a number
   against — the 90-day figure above is a starting proposal, not a
   researched one.
8. **TD is explicitly deferred, not ruled out forever.** If sub-station
   physical position ever becomes a real requirement, it needs its own
   design pass — the berth/SMART/cross-boundary-re-identification
   complexity here was sized just enough to justify not taking it on now,
   not analyzed to implementation depth.
9. **TRUST/TD sit on a legacy technology lineage.** At least one industry
   commentary describes TRUST/TD as an "aged" system family, with LINX
   (a Traffic-Management integration platform consuming TD data) as part
   of a longer-term modernization direction
   ([LinkedIn: "Transport vs. Data — TRUST the aged"](https://www.linkedin.com/pulse/transport-vs-data-trust-aged-ian-gordon)).
   Not an immediate risk, but worth knowing Network Rail is not
   necessarily investing in TRUST/TD's current form indefinitely.
