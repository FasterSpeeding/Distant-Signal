# Journey Tracking — Specification

Extends the existing single-train tracking feature into a "journey" — one or
more legs, each a real train working (or a not-yet-committed time-window
search), tracked and notified-on as one shareable unit. Single-train
tracking is a subset (a one-leg journey), not a separate system going
forward.

This is a design spec, not an implementation. Citations are file:line;
anything I couldn't verify directly is flagged **Speculative**.

**A naming collision, flagged up front.** `crates/api/src/data/journey.rs`,
`JourneyStop`, `build_journey_stops`, `frontend/components/JourneyTimeline.tsx`,
`JourneyProgress.tsx`, and `TrainJourneyState` **already exist** and mean
something narrower and older than what this doc calls a "journey": they are
the calling-point list (schedule + live overlay) of **one train working**.
This spec's new "Journey" entity is one level up — a named collection of
one-or-more train workings (legs). The two concepts nest (a Journey has
legs; a leg, once bound to a real train, gets its calling points rendered by
the *existing* `journey.rs`/`JourneyTimeline` machinery unchanged) but they
are not the same noun. I recommend leaving the existing module/component
names alone (renaming them is unrelated churn across a dozen files) and
giving the new entity distinctly-pluralized symbols throughout new code:
module `crates/api/src/data/journeys.rs` (plural), route file
`crates/api/src/routes/journeys.rs`, frontend `frontend/app/journeys/`,
Rust types `Journey`/`JourneyLeg` (capitalized, never bare `journey` as a
new type name). Called out again in Open Questions — the alternative is to
rename the product-facing new feature "Trip" instead of "Journey" to kill
the ambiguity outright.

---

## 0. What already exists (baseline)

### 0.1 Single-train tracking — the thing being extended

The tracked-train table is `train_subscriptions` (renamed from
`tracked_trains` by `crates/api/migrations/20260907100000_rename_tracked_trains.sql`).
Current effective schema, reconstructed across 13 migrations
(`20260828120000_train_tracking.sql` through `20260911090000_shared_groups.sql`):

```
train_subscriptions
  id                      BIGSERIAL PRIMARY KEY
  user_id                 TEXT NOT NULL REFERENCES users(id)
  service_date            DATE NOT NULL
  pin_origin_crs          TEXT               -- nullable (NR-primary rows have none yet)
  pin_scheduled_departure TIMESTAMPTZ        -- nullable, same reason
  pin_destination_crs     TEXT
  pin_operator            TEXT
  resolution_status       TEXT NOT NULL DEFAULT 'pending'
                           CHECK IN ('pending','schedule_matched','resolved','unresolved')
  tracked_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
  custom_name             TEXT
  trains_id               BIGINT REFERENCES trains(id) ON DELETE SET NULL
  notifications_enabled   BOOLEAN NOT NULL DEFAULT TRUE
Indexes: (user_id), (resolution_status), (trains_id)
```

Related tables, unchanged by this spec: `tracked_train_tickets`
(`tracked_train_id` nullable FK, `ON DELETE CASCADE`), `group_trains`
(`group_id, train_subscription_id, added_by, added_at`), `trains` (the
shared, deduplicated real-world-train row: `train_uid`, `service_date`,
`origin_crs`, `scheduled_departure`, `destination_crs`, `calling_points
JSONB`, `UNIQUE(train_uid, service_date)`), `train_current_state`
(`trains_id`-keyed live status/delay), `train_movement_events` (immutable
TRUST event log).

**Two creation paths exist today** (`crates/api/src/data/train_tracking.rs`,
`crates/api/src/routes/train.rs`):

1. **Legacy CRS+time pin** (`create_pin`, `train_tracking.rs:84-105`; route
   `POST /Train/track`, `train.rs:442-515`): user supplies `origin_crs` +
   `scheduled_departure` (a departure-board guess, not a confirmed identity).
   `validate_pin` (`train_tracking.rs:55-77`) rejects anything more than
   `MAX_PIN_AGE` (6h) in the past. The row starts `resolution_status =
   'pending'` and is resolved two ways: synchronously at creation
   (`schedule_matching::attempt_schedule_match` + `attempt_backlog_match`,
   both best-effort, `train.rs:459-509`), and by two **periodic sweeps** —
   `list_pending_pins_for_schedule_match` (`train_tracking.rs:914-925`) and
   `list_pending_pins_for_backlog_match` (`train_tracking.rs:989-1001`) —
   both selecting `WHERE trains_id IS NULL AND resolution_status = 'pending'
   AND pin_origin_crs IS NOT NULL AND pin_scheduled_departure IS NOT NULL`.
   **This "I don't have an exact train yet, match me one later" pending-row
   + periodic-sweep pattern is the direct architectural precedent for this
   spec's leg-matching mechanism (§2)** — but note its key is a single
   *point* (CRS, time), not a *window*; §2 explains why that key shape
   doesn't transfer as-is.
2. **NR-primary, identity-already-known** (`create_subscription_for_train`,
   `train_tracking.rs:198-230`; route `POST /Train/by-uid/{uid}/{date}/track`,
   `train.rs:739-755`): the caller already has a real `train_uid` (from
   `/trains` search or a train detail page). No `pending` waypoint at all —
   `find_or_create_train` resolves the shared `trains` row immediately and
   the subscription links to it in the same request. **Idempotent by
   `(user_id, trains_id)`** — a second click returns the existing
   subscription id rather than inserting a duplicate (`train_tracking.rs:167-197`'s
   doc comment).

Read model: `TrackedTrainState` (`train_tracking.rs:1011-1112`, full single-train
detail, ownership-gated at `GET /Train/{trackingId}`) and
`TrackedTrainListItem` (`train_tracking.rs:1181-1212`, the lighter `/Train/mine`
list row). Both carry `shared_group_count` via a correlated `COUNT(*) FROM
group_trains` subquery — the exact "attach a count to the row that owns it"
shape this spec's journey-group-sharing count should mirror (§6).

Deletion (`delete_tracked_train`, `train_tracking.rs:1310-1317`) is a single
`DELETE ... WHERE id=$1 AND user_id=$2`; every dependent row
(`train_movement_events`, `train_current_state`, `tracked_train_tickets`)
cascades. **No retention/pruning job exists for `train_subscriptions` at
all** (`train_tracking.rs:27-32`'s own comment) — it grows unbounded; only
`MINE_LIST_LIMIT = 100` (`train_tracking.rs:46`) bounds one response's size.
This matters for the migration (§7): a per-user table with no cap could be
large, but a one-time `INSERT ... SELECT` is still a bounded, single-pass
operation.

### 0.2 Frontend flow (existing)

`/track` (`frontend/app/track/page.tsx`) renders `TrackTrainForm.tsx`
(client component: station/time picker, live departure-board lookup,
submits `POST /api/Train/track`, best-effort ticket-attach and
group-share on success, redirects to `/train/by-id/{trackingId}`).
`TrackThisTrainButton.tsx` is the second entry point (identity already
known — e.g. from `/trains` search — posts straight to
`POST /Train/by-uid/{uid}/{date}/track`). `/track/mine`
(`frontend/app/track/mine/page.tsx`) merges three calls
(`getMyTrackedTrains`, `getMyTickets`, `getSharedGroupTrains`) into one list:
owned trains (`TrackedTrainListRow`), group-shared trains (`SharedTrainListRow`,
read-only), and unattached tickets. Detail pages
(`app/train/by-id/[trackingId]/page.tsx` — owner-scoped;
`app/train/[uid]/[date]/page.tsx` — public, separately checks `/Train/mine`
for an ownership match) both render **`TrainJourney.tsx`**
(`frontend/components/TrainJourney.tsx:39-`): `StatusMessage` +
`JourneyDetails` (ETA badge, last-reported location, delay, next calling
point) + `JourneyTimeline` (the calling-point list, "you are here" via
`JourneyProgress.tsx`) + `TicketPanel`. `TrackedTrainOwnerControls.tsx`
bundles rename/delete/add-to-group for the owner. **`TrainJourney` is
exactly the reusable per-leg display component this spec's new journey view
(§4) should embed once per leg, unmodified** — it already does everything
requirement #4 asks for at the single-train level (timings, delay,
cancellation via `StatusMessage`'s existing branches), minus platform and
station-skip, which don't exist on `JourneyStop` at all yet (see §0.5, §4).

### 0.3 Notifications (`crates/notifier`) — what #5 extends

Design doc: `docs/superpowers/specs/2026-09-02-line-status-notifications-design.md`.
Crate layout: `main.rs`, `decision.rs`, `send.rs`, `queries.rs`, `config.rs` — nothing else.

**Loop structure** (`main.rs:53-81`): one `tokio::select!` with two independent
`tokio::time::interval`s sharing state — a main cycle (`poll_interval_secs`,
default 60s) and a faster forward-queue cycle (`forward_queue_poll_interval_secs`,
default 15s, fed by `notifier_forward_queue`) that both funnel into the same
decision/send path (`main.rs:163-168`'s comment: "ONE decision path fed by
two inputs"). A third branch for journey-specific checks would slot into
this same `select!` (§5).

**Decision logic** (`decision.rs`), train side: `train_severity_rank(status,
delay_minutes, threshold)` (`decision.rs:67-79`) — cancelled=2 outranks
delayed(≥`train_delay_threshold_minutes`, default 15)=1 outranks 0.
`decide_train_notification(prev, new)` (`decision.rs:83-89`) is
**escalation-only** — fires the instant rank increases, no cooldown, no
de-escalation notify, no cold-start guard (a newly-tracked already-delayed
train notifies once immediately).

**Data path — per-shared-train, fanned out to subscribers, not per-row
polling.** `candidates_for_trains_id` (`queries.rs:145-204`) reads
`train_current_state` **once per `trains_id`**, then joins
`train_subscriptions WHERE trains_id = $1` to fan that one read out to every
subscriber's own row, each judged independently against its own
`train_notification_state` entry. `poll_train_candidates`
(`queries.rs:221-256`) pre-filters to `trains_id`s with ≥1 subscriber. **This
per-physical-train-fanned-to-subscribers shape is exactly what a
per-leg-fanned-to-journey-owners shape needs to mirror** — a leg's bound
`trains_id` is unchanged by this spec, so the *existing* fan-out already
reaches every journey whose leg points at that `trains_id`, with zero query
changes; only the *message copy* (which leg of which journey) and *audience*
(§5, §6) need journey-awareness.

**Dedup**: `train_notification_state(user_id, tracked_train_id,
last_notified_status, last_notified_delay_minutes, last_notified_at)`, PK
`(user_id, tracked_train_id)`, upserted only after a send succeeds
(`queries.rs:333-358`, `main.rs:209-217`) — never before, so an unresolved
failure retries at the next real transition rather than queuing.

**Delivery**: Web Push only. `push_subscriptions(id, user_id, endpoint
UNIQUE, p256dh, auth, created_at, last_seen_at)`
(`20260902100000_notifications.sql`); subscribe via
`POST /notifications/subscribe`, VAPID key via
`GET /notifications/vapid-public-key` (`crates/api/src/routes/notifications.rs:15-25`).
`send.rs` sends VAPID-signed payloads (`NotificationPayload{title, body, url,
tag}`), 3 bounded retries, 404/410 self-deletes the subscription
(`queries.rs:384-390`). `frontend/public/sw.js:138-163` handles
`push`/`notificationclick`. **This entire delivery layer is reused
unchanged** — a journey notification is just a differently-worded
`NotificationPayload` sent through the same pipe.

**Station-skip detection: does not exist for tracked trains today, but a
real building block exists elsewhere.** `JourneyStop`
(`crates/api/src/data/journey.rs:121-142`) has no skip/cancellation field at
all — `crs, name, tiploc, kind, scheduled_*, actual_*, estimated_*,
last_event_type, variation_status, delay_minutes`, nothing else.
Whole-train cancellation is TRUST's `Cancellation` message
(`crates/trust-schema/src/schema.rs:74-81`, `canx_type: "EN ROUTE" |
"AT ORIGIN"`) — **train-level only, no location field, no per-stop
concept**; it becomes `train_current_state.status = 'cancelled'`, already
fully wired for both requirement #5's "cancellation" signal and existing
notifications.

What genuinely already detects a per-stop skip is a **separate, unrelated
pipeline**: `common::StationDeparture.skipped_stations: Vec<String>`
(`crates/common/src/lib.rs:443-448`, design doc
`docs/superpowers/specs/2026-07-13-skipped-station-detection-design.md`) —
Darwin/LDBWS's per-calling-point `isCancelled` flag, parsed by
`poller-ldbws`, one field on **each live departure-board sample**
(`GET /public/stations/{crs}/departures` → `latest_station_sample`, already
read by `eta_blend.rs:22` for the existing Darwin-ETA overlay on tracked
trains). Today this only feeds **line-level aggregate severity**
(`crates/aggregator/src/aggregation.rs`'s skip-rate classification) — it has
**never been wired to a specific tracked train**. The precedent for wiring
it there already exists in the codebase, though: `eta_blend::find_darwin_eta`
(`crates/api/src/data/eta_blend.rs:22-`) already does a best-effort lookup of
"the live sample at this train's origin station matching this train's
destination" to overlay `eta_next`. **The same lookup, checking
`skipped_stations` instead of `etd`, is the concrete mechanism for #5's
"no longer stopping here" signal** — see §5.

### 0.4 Groups sharing (`crates/api/src/routes/groups.rs`, `data/groups.rs`) — the precedent for #6a

Schema (`20260911090000_shared_groups.sql`, `20260915100000_custom_line_group_grants.sql`):
`groups(id TEXT PK, name, created_by, created_at)` — `id` a random,
non-guessable token; `group_members(group_id, user_id, role CHECK IN
('owner','admin','member'), joined_at)`; `group_trains(group_id,
train_subscription_id, added_by, added_at)`, PK `(group_id,
train_subscription_id)`, both FKs `ON DELETE CASCADE`; and
`custom_line_group_grants(group_id, line_id, granted_by, granted_at)` — a
second, independently-evolved sharing table for a *different* personal
resource, useful as a contrast case.

**The exact permission model to copy** (both are live, tested, not just
design docs — `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`
is the fullest write-up, despite its stale "not yet approved" header):

1. **Share**: any current member may share, but only a resource they
   **own** — enforced inside the data function, not by group role.
   `add_train_to_group` (`data/groups.rs:692`) checks `SELECT id FROM
   train_subscriptions WHERE id=$1 AND user_id=$2` first; a mismatch is
   `404`, never relaxed for a group owner/admin ("a group's management
   structure has no standing over a member's private resource").
2. **Unshare**: sharer-or-manager. `remove_train_from_group`
   (`data/groups.rs:728`) lets a group `owner`/`admin` remove **any**
   shared row, or the original sharer remove **their own** — same shape for
   `remove_custom_line_grant`.
3. **View**: any current member (`require_member`, 404-if-not-a-member,
   never `403` — hides even whether the group exists) sees the full shared
   list, **strictly read-only** — `SharedTrainRow`/`SharedCustomLineRow`
   (`frontend/app/groups/[id]/page.tsx:348-467`) gate the remove button on
   `canManage || currentUserId === addedBy`, and **no group role can edit or
   delete someone else's shared resource**, only unshare it.
4. **Departed-member cleanup differs between the two precedents**:
   `group_trains` rows are deleted in the same transaction as a departing
   member's `group_members` row; `custom_line_group_grants` deliberately is
   **not** — the grant survives so the (now possibly ex-member) owner can
   still revoke it later. §6 recommends journeys follow the `group_trains`
   precedent (cascade on departure), since a journey is an active,
   live-tracked personal thing, not a static definition.

Routes: `GET/POST /groups/{id}/trains`, `DELETE
/groups/{id}/trains/{train_subscription_id}`
(`crates/api/src/routes/groups.rs:31-118`) — **this exact route shape,
copied verbatim for `/groups/{id}/journeys`, is §6's proposal.**

### 0.5 Train search / candidate-matching (`crates/api/src/data/queries.rs`) — the building block for #2

`search_schedule_calling_point_departures` (`crates/api/src/data/queries.rs`,
~1414-1562) — current signature, just extended today for directional
`stops_at` ordering:

```rust
pub async fn search_schedule_calling_point_departures(
    pool: &PgPool,
    station_crs: &str,
    service_date: NaiveDate,
    scheduled_from: NaiveTime,
    true_origin_crs: Option<&str>,
    stops_at: Option<&str>,
    to_time: Option<NaiveTime>,
    stop_arrival_from: Option<NaiveTime>,
    stop_arrival_to: Option<NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>>
```

Queries `schedule_destination_departures`, anchored on ONE fixed station
(`origin_crs = station_crs`, always equality), with `scheduled_from`/`to_time`
bounding that station's own departure time and `stop_arrival_from`/`to`
bounding the **arrival** at `stops_at` (an `EXISTS` self-join on `train_uid`,
scoped to whichever calling point `stops_at` matched). **This is already,
almost exactly, "depart X after/before T1, arrive Y after/before T2"** —
`station=WAT&stops_at=RDG&from=08:00&to=09:00&arrival_from=08:30&arrival_to=09:30`
is a real, working query today. Exposed at `GET /public/trains/search`
(`crates/api/src/routes/trains.rs`, params struct `TrainSearchParams`,
~162-239; handler `get_trains_search`, ~357-489), consumed by
`frontend/components/TrainSearchForm.tsx` (773 lines) — a required Station
field, optional Date/Origin/"Stops at", and **two paired
`TimeFilterInput.tsx` instances** for each time bound ("Earliest
departure"/"Latest departure", and — once "Stops at" is filled —
"Earliest arrival"/"Latest arrival"). `TimeFilterInput` itself
(`frontend/components/TimeFilterInput.tsx`) is a single generic
optional-time control (`{label, name, description, value, onChange,
onIncompleteChange, error}`, value `"HH:MM"|""`) — **the before/after
convention is compositional**: two instances of the same component, one
labeled as a lower bound, one an upper bound, no single toggle widget. This
is the UI pattern §2/§3 should reuse verbatim for leg-window entry, not
invent a new one.

**The one real gap**, worth stating precisely since it's load-bearing for
§2: for two *different* stations, `stops_at` verifies membership ("Y is
called at somewhere on this working") but **not order** — nothing today
confirms Y is reached *after* departing X for a non-loop pair (route doc
comment, item 2; `queries.rs` ~1341-1349). A journey leg's "arrive Y" is
meaningless if Y precedes X on the same working, so this spec's
candidate-search extension needs an explicit ordering predicate the
existing function doesn't have (§2).

No dedicated before/after Rust type exists anywhere (`crates/common/src/lib.rs`
checked) — `TrackPinRequest` (`common/src/lib.rs:638-646`) carries one
`scheduled_departure`, not a bound pair. Every existing before/after pair is
two sibling `Option` fields, duplicated at each layer — a real,
worth-fixing gap this spec's new leg-window type should close by
introducing one reusable struct (§2) rather than repeating the pattern a
third time.

### 0.6 Per-stop platform/delay display (#4's building block) — in-flight external dependency

**Found, in progress, not yet merged**: worktree
`/home/coder/Distant-Signal/.claude/worktrees/agent-ad6ebb365cf9fcac0`,
branch `worktree-agent-ad6ebb365cf9fcac0`, uncommitted. So far it adds
`platform: Option<String>` and `planned_platform: Option<String>` to
`common::StationDeparture` and to `TrackPinRequest`, and parses RDM's
`"platform"` field in `crates/poller-ldbws/src/schema.rs`. It references
(but has not yet created) a `poller_ldbws::platform_history::PlatformHistory`
module for reconstructing "the earliest platform seen for this service"
as a stand-in for a true planned-vs-actual distinction (Darwin only ever
exposes one "current" platform on the wire). **Not yet done, as of this
snapshot**: `render.rs`'s JSON serialization, and — critically for this
spec — **no `TrainScheduleRow` component and no delay-colour-coding scheme
exist anywhere yet**, on any branch. On `main` today, `JourneyStop` has no
platform field at all, and `poller-ldbws` already ingests the feed but
currently discards the `"platform"` key entirely.

**Treat this exactly as the operator-overview spec treated its own Phase 3→4
dependency**: this journey feature's candidate-list and per-leg display
(§4) should consume platform/delay data through whatever shape that work
lands with (`StationDeparture.platform`/`.plannedPlatform`, and a reusable
row component once one exists) rather than re-deriving platform data itself
— but since neither the frontend component nor the `render.rs` wiring exist
yet, **Phase 1 of this spec should not block on it** (see §8); platform
display in the journey view should be added once that work merges, and in
the meantime the journey view can ship with delay/cancellation/skip only
(all three have real data today) and a platform column added later without
a redesign.

---

## 1. Data model

### 1.1 New entities

Two new tables, additive only — **`train_subscriptions` itself is
unchanged** (no new columns, no renamed columns), so every existing route,
the notifier, tickets, and `group_trains` sharing keep working byte-for-byte
regardless of this feature.

```sql
CREATE TABLE journeys (
    id          BIGSERIAL PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id),
    custom_name TEXT,                                   -- mirrors train_subscriptions.custom_name
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX journeys_user_id ON journeys (user_id);

CREATE TABLE journey_legs (
    id                  BIGSERIAL PRIMARY KEY,
    journey_id          BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    leg_order           INT NOT NULL,                    -- 1-based sequence within the journey
    origin_crs          TEXT NOT NULL,                   -- the leg's OWN intent, kept even once matched
    destination_crs     TEXT NOT NULL,
    service_date         DATE NOT NULL,
    -- Time-window search intent. NULL only for a leg created by picking a
    -- specific known train directly (no window was ever searched); once a
    -- window IS set, it is kept FOREVER, even after train_subscription_id
    -- is populated -- see Decision (2026-09-22) in §2.3: a matched leg's
    -- window stays live so "Change train" can re-open the exact same
    -- candidate search without the user re-entering criteria.
    depart_after        TIME,
    depart_before        TIME,
    arrive_after         TIME,
    arrive_before         TIME,
    -- Binding to a real train working, reusing 100% of existing
    -- train_subscriptions/trains/notifier/journey.rs machinery.
    train_subscription_id BIGINT REFERENCES train_subscriptions(id) ON DELETE SET NULL,
    match_mode           TEXT NOT NULL DEFAULT 'unmatched'
                          CHECK (match_mode IN ('unmatched', 'manual', 'auto')),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (journey_id, leg_order)
);
CREATE INDEX journey_legs_journey_id ON journey_legs (journey_id);
CREATE INDEX journey_legs_train_subscription_id ON journey_legs (train_subscription_id);
```

Why this shape, and not folding leg fields directly onto
`train_subscriptions`:

- **A leg can exist with no train at all yet** (an open time-window
  search). `train_subscriptions` has no row-without-a-pin concept — every
  existing row already represents a specific pin attempt (`pin_origin_crs`
  + `pin_scheduled_departure`, however unresolved). Overloading it with a
  window (`depart_after`/`depart_before` instead of a point) would force
  every existing reader of `pin_scheduled_departure` (the pending-sweep
  queries, `TrackedTrainState`/`TrackedTrainListItem`'s selects, the
  notifier's `candidates_for_trains_id`) to handle a shape they were never
  written for. Keeping the window on a new table means **zero changes** to
  `train_tracking.rs`'s existing queries.
- **Once a leg picks a candidate**, `train_subscription_id` is set and from
  that point on the leg is, for every purpose except journey-level
  grouping, an ordinary `train_subscriptions` row — schedule matching,
  live TRUST resolution, `journey::build_journey_stops`, ticket
  attachment, `group_trains` sharing, and the notifier's existing
  escalation logic all apply **unchanged**, because they operate on
  `trains_id`/`train_subscriptions.id`, which a leg is just a foreign key
  to.
- **`origin_crs`/`destination_crs` are kept on the leg even after
  matching**, not derived solely from the bound train's own pin fields.
  This is what makes station-skip detection (§5) well-defined: "the station
  this leg's traveller cares about" is the leg's own origin/destination
  (their actual journey intent), which may differ from the matched train's
  full-route origin/destination (e.g. boarding partway, or a train that
  continues past the traveller's stop).

### 1.2 What's genuinely new vs. reusable

| Piece | New or reused |
|---|---|
| "One pin resolves to one real train" | **Reused entirely** — `train_subscriptions`, `trains`, schedule matching, TRUST resolution |
| "I don't have an exact match yet, retry me later" sweep pattern | **Pattern reused, mechanism new** — `PendingSchedulePin`/`list_pending_pins_for_schedule_match` is the precedent, but its key is a point (CRS+time); a window needs a new sweep function (§2) |
| Per-train calling points, live overlay, "you are here" | **Reused entirely** — `journey.rs`, `JourneyTimeline`, `JourneyProgress`, `TrainJourney.tsx` |
| Delay/cancellation detection + notification | **Reused entirely** — `train_current_state.status`/`.delay_minutes`, `decision.rs`'s escalation logic, `train_notification_state` |
| Station-skip detection *for a specific tracked train* | **New** — `skipped_stations` exists (§0.3) but has never been read for anything but line-level aggregates |
| Grouping multiple legs as one unit | **New** — `journeys`/`journey_legs` |
| Group sharing of a whole journey | **New table, copied pattern** — `group_journeys` mirroring `group_trains` exactly (§6) |
| Platform display | **Blocked on external in-flight work** (§0.6) |

---

## 2. Time-window candidate matching (#2)

### 2.1 The query extension

Extend `search_schedule_calling_point_departures` (or add a sibling
function reusing its query fragments — a judgment call for the implementer,
not this spec) with:

1. **An explicit downstream-order predicate** when `station_crs !=
   stops_at`: the existing `EXISTS` clause matching `stops_at` needs an
   added condition that the matched calling point's own scheduled time (or
   sequence number within the schedule, more robust than time for
   same-minute edge cases) is strictly after `station_crs`'s scheduled time
   on the same `train_uid`. This closes the ordering gap named in §0.5 —
   without it, a leg search for "WAT → RDG" could surface a train that
   calls at RDG *then* WAT later on the same diagram (rare, but real for
   looping/reversing services), which would silently produce a nonsensical
   candidate.
2. No other schema change — `depart_after`/`depart_before` map onto the
   existing `scheduled_from`/`to_time` params, `arrive_after`/`arrive_before`
   onto `stop_arrival_from`/`stop_arrival_to`.

Introduce one new shared type, closing the "no reusable before/after type"
gap (§0.5):

```rust
// crates/common/src/lib.rs
pub struct TimeWindow {
    pub after: Option<NaiveTime>,
    pub before: Option<NaiveTime>,
}
```

used for both `depart` and `arrive` bounds on the new leg-creation request
type, rather than four more standalone `Option<NaiveTime>` fields.

### 2.2 Route

`GET /Journeys/{journeyId}/legs/{legId}/candidates` — runs the extended
query with the leg's own `origin_crs`/`destination_crs`/`depart_*`/`arrive_*`,
returns the same `CallingPointDeparturePage` shape `/public/trains/search`
already returns (reuse the response type verbatim — no new DTO needed).

### 2.3 Commit vs. leave-open — resolved

**Decision (2026-09-22, product owner):** `'manual'`-only for Phase 1,
`'auto'` deferred, exactly as recommended below — **with one addition**:
committing a leg to a candidate does not retire that leg's search. The
window (`depart_after`/`depart_before`/`arrive_after`/`arrive_before`,
§1.1) is kept forever, not cleared on commit, specifically so a matched leg
can offer a **"Change train"** action that re-opens `GET .../candidates`
against the exact same persisted window — no re-entering origin/destination/
time criteria to pick a different service. This is the schema's existing
"never overwritten" behavior (§1.1) put to explicit product use, not a new
table/column; the only new surface is the "Change train" affordance itself
(§4) and re-running the *same* commit route (§2.2) against an
already-matched leg (an `UPDATE ... SET train_subscription_id = $new`,
not a fresh leg row — the old `train_subscription_id`'s underlying
`train_subscriptions` row is simply orphaned from this leg, left exactly as
today's `delete_tracked_train`/re-pin flows already leave an
unreferenced row, no new cleanup logic required).

Two real modes:

- **`match_mode = 'manual'`**: the user browses `GET .../candidates` (same
  list UI as `/trains` search results, reusing `TrainSearchForm`'s result
  rendering) and picks one. The route calls `find_or_create_train` +
  `create_subscription_for_train` (both **already exist**,
  `train_tracking.rs:198-230`) and sets `journey_legs.train_subscription_id`
  — on first pick, or on any later "Change train" re-pick. This is the
  safe, unambiguous mode and the only one shipping in Phase 1 (§9) — it
  never has to decide "which of several matching trains did the user mean,"
  and the persisted window means the user is never worse off than a
  from-scratch search when they want to switch.
- **`match_mode = 'auto'`**: the leg stays unbound, and a periodic sweep
  (structurally the direct descendant of `list_pending_pins_for_schedule_match`,
  §0.1) re-runs the candidate query each cycle. **Deferred past Phase 1, per
  the same 2026-09-22 decision** — a window like "depart after 08:00,
  before 10:00" on an hourly service can match several distinct trains on
  one service day, and auto-mode still needs a follow-up product decision on
  which one wins whenever it's picked up (candidates for that future
  decision: (a) always the *earliest* candidate matching the window,
  committed the moment scheduling data exists for it — closest analogue to
  today's point-in-time pin behavior; (b) the *nearest-to-now* candidate,
  re-decided daily for a recurring commute-style journey). Manual-pick with
  a persisted, re-openable window fully satisfies requirement #2's literal
  wording for Phase 1 without needing that follow-up decision made now.

### 2.4 What "candidate matching" reuses for a leg once committed

Nothing further — a committed leg's `train_subscription_id` row goes
through exactly the same schedule-matching/backlog-matching/live-TRUST
pipeline every other `train_subscriptions` row does today, because it *is*
one.

---

## 3. Chaining UX (#3)

Adding a second leg: `POST /Journeys/{id}/legs` with either `{trainUid,
serviceDate}` (a direct, already-known-identity leg — reuses
`create_subscription_for_train` immediately, `match_mode` unused/`'manual'`
implicitly) or `{originCrs, destinationCrs, serviceDate, departWindow,
arriveWindow}` (an open leg, per §2). `leg_order` is assigned as
`max(leg_order) + 1` for that journey; no hard validation that leg N's
`destination_crs` equals leg N+1's `origin_crs` — interchange stations
aren't always textually identical (a walk between two nearby stations, a
tram/bus-linked transfer), so the frontend should **default-suggest** the
prior leg's destination as the new leg's origin without enforcing it.

**What "the journey" means with legs in mixed states.** No single derived
enum is authoritative — the journey list/detail view rolls up **per-leg**
status into one summary the same way `LineStatusCard`'s "worst status"
pattern already works elsewhere in this app (`frontend/lib/severity.ts`'s
`worstStatus`, cited by the operator-overview spec, §0's "Frontend
conventions" — same reduce-to-worst idiom, just over legs instead of
lines): an unmatched leg contributes a "needs a train picked" state that
outranks a merely-delayed leg for the journey summary badge, and a
cancelled/skipped leg outranks both. This needs no new severity-ranking
code — the shape is a straight port of an idiom this codebase already
leans on twice (`severity.ts`'s `GROUP_RANK`, `decision.rs`'s
`train_severity_rank`).

**Connection buffer, explicitly flagged as valuable but out of scope for
Phase 1**: once two adjacent legs are both matched, the gap between leg N's
arrival and leg N+1's departure is a natural "will you make your
connection" signal worth surfacing and worth eroding visibly as delay
accrues. This is real product value but a materially separate feature
(comparing two legs' live ETAs against each other, not just reading one
leg's own state) — recommend Phase 3+ (§8), not blocking the core
multi-leg-tracking ship.

---

## 4. The journey view (#4)

New route `frontend/app/journeys/[id]/page.tsx`. Structure:

- Header: journey `custom_name` (editable, same rename pattern as
  `RenameTrainButton`), a share-to-group button (§6), an "Add a leg" button
  (§3).
- One card per leg, `leg_order` ascending:
  - **Matched leg** (`train_subscription_id IS NOT NULL`): embed
    `<TrainJourney state={...} />` **unmodified** — this already renders
    timings, delay, cancellation via its existing `StatusMessage`/
    `JourneyDetails`/`JourneyTimeline` branches (§0.2). Additions this spec
    needs on top of it: a **station-skip badge** for the leg's own
    `origin_crs`/`destination_crs` (new — see §5's detection mechanism;
    render as a small badge next to the relevant `JourneyTimeline` row,
    same visual family as the existing cancelled-stop styling that
    component presumably already has for a `PASS`-kind stop); and, **only
    when the leg has a persisted window** (`depart_after` etc. non-null,
    §1.1/§2.3) — i.e. it was created via time-window search, not a direct
    known-train pick — a **"Change train" button** that re-opens the same
    open-leg candidate card described below, pre-scoped to the leg's own
    persisted origin/destination/window, letting the user swap to a
    different candidate in one action instead of re-searching from
    scratch (§2.3's 2026-09-22 decision). A leg created by picking a
    specific train directly has no window to re-open and so gets no
    "Change train" action in Phase 1 — swapping that kind of leg means
    delete-and-recreate, same as today's single-train tracking. Once
    **§0.6's in-flight platform work merges**, a platform column is also
    added — explicitly deferred, not blocking.
  - **Open leg** (`train_subscription_id IS NULL`): a lighter card showing
    the search parameters (origin, destination, windows) and a live-fetched
    candidate list (§2.2's route), each row rendered with the same shape
    `/trains` search results already use, plus a "Track this train" action
    per candidate that commits the leg (§2.3).
- Between two matched, adjacent legs: a thin "connection" divider showing
  scheduled arrival → scheduled departure, no live buffer computation in
  Phase 1 (§3).

No new backend read-model is strictly required beyond a `GET
/Journeys/{id}` that returns `{journey, legs: [{..., trackedTrainState?:
TrackedTrainState}]}` — for each matched leg, join straight into the
**existing** `TRACKED_TRAIN_STATE_SELECT` query (`train_tracking.rs:1151-1168`)
by `journey_legs.train_subscription_id`, so the wire payload for a matched
leg is exactly today's `TrackedTrainState` shape, unchanged, with the skip
badge (§5) as the one new field layered on.

---

## 5. Notifications (#5)

### 5.1 Delay and cancellation — fully reused, no new decision logic

A committed leg's `train_subscription_id` is an ordinary
`train_subscriptions` row; the notifier's existing per-`trains_id` fan-out
(`candidates_for_trains_id`, §0.3) already reaches it with zero query
changes, and `decide_train_notification`'s escalation-only rank logic
already fires on delay-threshold-crossing and cancellation exactly as it
does for any other tracked train today.

**What does need a small, additive change**: message copy. Today's
`NotificationPayload` (built in `send.rs`) presumably reads something like
"Your tracked train to Edinburgh is delayed" — a journey-aware version
should say "Leg 2 of 'Weekend in Edinburgh' (Waverley → Kings Cross) is
delayed 18 minutes." This needs one new query
(`journey_leg_for_train_subscription(train_subscription_id) -> Option<(journey_name,
leg_order, total_legs)>`) consulted by the payload builder, and a fallback
to today's exact copy when a `train_subscription_id` has no `journey_legs`
row (true for every legacy tracked train until/unless it's migrated — §7 —
and for any train tracked outside the journeys flow, if that path is kept
open at all).

### 5.2 Station skip — new detection, reused delivery

**No existing mechanism ties `skipped_stations` (§0.3) to a specific
tracked train.** The concrete new work:

1. A new best-effort lookup, structurally identical to
   `eta_blend::find_darwin_eta` (`eta_blend.rs:22-`): given a leg's matched
   train and its `origin_crs`, fetch `latest_station_sample(origin_crs)`
   (already exists, `queries::latest_station_sample`, already called by
   `blend_darwin_eta` for the ETA overlay), find the sample entry matching
   this service (by headcode/destination, the same matching `find_darwin_eta`
   already does), and check whether the leg's `destination_crs` appears in
   that entry's `skipped_stations`. This answers "is *this* leg's
   destination among the stops *this* service is no longer calling at
   today" — the exact signal requirement #5 asks for. Symmetrically check
   `origin_crs` against a sample taken further back down the line, if one
   exists, for the (rarer) case of a dropped *origin* stop on a
   through-service the traveller was joining partway.
2. Wire this into the same read path §4 uses for the skip badge (best-effort,
   never fails the request — same posture `blend_darwin_eta` already takes).
3. For the *notification* side (not just display), add a parallel state
   table `journey_leg_notification_state(user_id, journey_leg_id,
   last_notified_skipped BOOLEAN, last_notified_at)`, and a new decision
   function `decide_skip_notification(was_skipped, is_skipped)` — same
   escalation-only shape as `decide_train_notification` (fire on
   `false→true`, never on `true→false`, same "written only after a
   successful send" dedup discipline as every other `*_notification_state`
   table).
4. A third branch in the notifier's existing `main.rs` `select!` loop
   (§0.3), polling committed legs' bound trains' latest station samples on
   its own interval — reusing `send.rs`/`push_subscriptions` unchanged.

**Clarifying "the station the user cared about" (requirement #5's own
open point)**: this resolves cleanly to the leg's own `origin_crs`/
`destination_crs` (§1.1's design already keeps these on the leg row
independent of the matched train's full route), *not* every intermediate
stop the matched train happens to skip elsewhere on its diagram — a skip
50 miles from either end of this traveller's leg is not this traveller's
problem.

### 5.3 Audience — open question, not resolved here

Should a group a journey is shared into (§6) also receive push
notifications for it, or only the owner? The `group_trains`/
`custom_line_group_grants` precedent (§0.4) is silent on this because
neither resource has any notification concept today — this is genuinely
new ground. Recommend **owner-only** for Phase 1 (matches every other
sharing precedent's "view-only, no side effects for other members" posture)
and flag squarely as Open Question §9.3.

---

## 6. Groups sharing (#6a)

Direct copy of the `group_trains` pattern (§0.4), one new table:

```sql
CREATE TABLE group_journeys (
    group_id   TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    journey_id BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    added_by   TEXT NOT NULL REFERENCES users(id),
    added_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, journey_id)
);
```

Routes `GET/POST /groups/{id}/journeys`, `DELETE
/groups/{id}/journeys/{journey_id}`, copied line-for-line from
`crates/api/src/routes/groups.rs`'s train endpoints (§0.4's routes).
Permission model identical: **share** requires the caller to own the
journey (`WHERE id=$1 AND user_id=$2` on `journeys`, same as
`add_train_to_group`'s ownership check); **unshare** is sharer-or-manager;
**view** is any member, read-only — no group role may edit or delete
someone else's shared journey, only unshare it, matching
`SharedTrainRow`/`SharedCustomLineRow`'s existing "deliberately view-only"
posture (§0.4).

**Departed-sharer cleanup**: follow the `group_trains` precedent (cascade
`group_journeys` row deletion in the same transaction as the departing
member's `group_members` row), not `custom_line_group_grants`' persist-
after-departure exception — a journey is a live, actively-tracked personal
thing (closer in kind to a tracked train) rather than a static definition a
group might want to keep referencing after its author leaves.

**A real, non-trivial extension this needs that `group_trains` didn't**:
viewing a shared *journey* means reading every one of its legs' bound
`train_subscriptions` rows on behalf of a caller who is **not** that row's
owner. `TrackedTrainState`'s existing ownership gate
(`get_by_tracking_id`, `train.rs:552-582`) checks `tracked_train_owner ==
user.id` only — it has **no path today** for "this row's owner shared it
into a group I'm in." The journey-detail read needs its own authorization
check: `caller is a member of any group this journey is shared into` OR
`caller owns the journey`, checked once at the journey level, then reading
every leg's `train_subscriptions` row **without** re-checking that row's
own ownership (deliberately bypassing the per-train ownership gate for
this one read path, since journey-level sharing is the authority here, not
per-leg `group_trains` membership). This is new code, not a reuse of
`TrackedTrainState`'s existing gate — call this out explicitly during
implementation review since it's the one place this feature's sharing
model isn't a pure copy-paste.

---

## 7. Migration (#6b)

### 7.1 The migration itself

One data-migrating `.sql` migration, consistent with this repo's existing
migration style (DML inside a versioned migration file is already normal
here — e.g. `20260906140000_drop_legacy_columns.sql`-adjacent migrations
in this table's own history did structural changes; this one does a data
backfill instead, same mechanism):

```sql
-- One journey per existing train_subscriptions row (1:1, never merged —
-- requirement #6b is explicit: EACH pre-existing tracked train becomes
-- its OWN single-train journey, not grouped by user).
INSERT INTO journeys (user_id, custom_name, created_at, updated_at)
SELECT user_id, custom_name, tracked_at, tracked_at
FROM train_subscriptions;

-- journeys.id is a fresh serial sequence -- correlate back to the source
-- row via a positional join on (user_id, tracked_at) is NOT safe (not
-- unique). Use a session-local mapping instead: this needs to run as a
-- single statement using a CTE that RETURNS the new id alongside the
-- source row's own id, e.g.:
WITH inserted AS (
    INSERT INTO journeys (user_id, custom_name, created_at, updated_at)
    SELECT id, user_id, custom_name, tracked_at, tracked_at
    FROM train_subscriptions
    RETURNING id AS journey_id, ... -- see note below
)
INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs,
                           service_date, train_subscription_id, match_mode)
SELECT inserted.journey_id, 1,
       COALESCE(ts.pin_origin_crs, ''), COALESCE(ts.pin_destination_crs, ''),
       ts.service_date, ts.id, 'manual'
FROM inserted JOIN train_subscriptions ts ON ...;
```

(The exact CTE plumbing to carry the source `train_subscriptions.id`
through the `journeys` INSERT — Postgres's `RETURNING` on a multi-row
`INSERT ... SELECT` doesn't let you echo an arbitrary source column
directly without also selecting it — is an implementation detail for
whoever writes the real migration; sketched here to make the **row-count
and 1:1 correlation intent** unambiguous, not as literal ship-ready SQL.)

**`origin_crs`/`destination_crs` on the new `journey_legs` row can be
empty-string** for an NR-primary subscription whose `pin_origin_crs`/
`pin_destination_crs` were never populated (§0.1's accepted gap — the same
one `TrackedTrainState`'s own fields already model as `Option`). Recommend
`journey_legs.origin_crs`/`.destination_crs` be nullable after all,
loosening §1.1's `NOT NULL` — noted as a correction to §1.1's schema sketch
prompted by this migration's own data reality, same kind of discovery
`20260906130000_nullable_pin_columns.sql` made about the exact same
underlying columns.

Every `journey_legs` row created by this migration has `match_mode =
'manual'` (it's already bound to a real train — there's no window to
re-open) and `depart_after`/`depart_before`/`arrive_after`/`arrive_before`
all `NULL`.

### 7.2 Backward compatibility

**Nothing about `train_subscriptions` changes structurally.** Every
existing route continues to work exactly as before the migration:
`GET/POST /Train/*`, ticket attach/list/delete, `group_trains` sharing,
the notifier's existing per-`trains_id` fan-out, `journey.rs`'s
calling-point overlay. A user who never touches the new `/Journeys/*`
surface at all sees **zero behavior change**.

**What does change, per requirement #6's "supersede" framing**: going
forward, the *user-facing entry points* should stop creating bare
`train_subscriptions` rows directly. Concretely:

- `TrackTrainForm.tsx`'s submit handler and `TrackThisTrainButton.tsx`
  should be rewritten to call a new `POST /Journeys` (with an inline first
  leg) instead of `POST /Train/track` / `POST /Train/by-uid/.../track`
  directly — the new journey-creation route internally calls the exact
  same `create_pin`/`create_subscription_for_train` functions (§0.1,
  unchanged), then wraps the result in a one-row `journeys` +
  `journey_legs` pair, matching what the migration (§7.1) produces for
  historical rows. This makes "create a single-train journey" and "track a
  single train" the literal same code path from this point on, closing
  the "two parallel systems" concern at the product-surface level even
  though the underlying primitive (`train_subscriptions`) is deliberately
  kept as internal plumbing, not deleted.
- `/track` and `/track/mine` become redirects/aliases to `/journeys/new`
  and `/journeys/mine` respectively (a single-leg journey's detail page can
  render identically to today's `TrainJourney`-only view — no visual
  regression for the common case). Recommend keeping the **backend**
  `/Train/*` routes exactly as they are (tickets, groups, and the notifier
  all still key off `train_subscriptions.id` directly, and rewriting them
  to key off `journeys`/`journey_legs` instead is materially larger,
  cross-cutting surgery this spec does not recommend attempting in the
  same phase — see §8).
- `GET /Train/mine` stays as-is (still useful as "every train working I
  have, regardless of journey" — e.g. for `AttachTicketAction`'s picker,
  §0.1, which has no reason to become journey-aware) but the *primary*
  user-facing list becomes `GET /Journeys/mine`.

---

## 8. Open questions for the product owner

1. ~~**Auto-match commit rule for an open leg (§2.3)**~~ — **RESOLVED
   2026-09-22 (product owner): manual-pick only for Phase 1, with the
   leg's search window persisted (never cleared on commit) so a matched
   leg can offer a "Change train" re-pick against the same window without
   re-searching (§2.3, §4). `'auto'` mode (does it ever commit without a
   human pick, and to which candidate) is deferred past Phase 1 — see §9's
   revised phasing, which now ships window-search + manual-pick + "Change
   train" in Phase 1 itself, not Phase 2.**
2. **Naming collision (front matter)** — keep "Journey" as the product term
   despite the existing `journey.rs`/`JourneyTimeline`/`JourneyProgress`
   per-train machinery meaning something narrower, or rename the new
   feature "Trip" end-to-end to avoid any ambiguity in code, routes, and
   docs going forward? This spec assumes "Journey" (matches the brief's own
   wording) with careful new-vs-existing symbol separation, but it's a real
   naming cost worth the PO's explicit sign-off.
3. **Notification audience for a shared journey (§5.3)** — owner-only, or
   do group members who a journey is shared with also get push
   notifications for its legs? No existing precedent answers this (neither
   `group_trains` nor `custom_line_group_grants` has any notification
   concept). Recommend owner-only for Phase 1.
4. **Does `/Train/*` stay a semi-public, directly-usable API surface**, or
   should it become fully internal (auth-gated to only the frontend's own
   server, not documented/stable for other callers) now that `/Journeys/*`
   is the intended entry point? Affects whether `post_track`/
   `post_track_by_uid` need deprecation warnings, versioning, or can simply
   keep existing unchanged (this spec's default assumption).
5. **Connection-buffer feature (§3)** — real value, explicitly out of
   Phase 1/2 scope in this spec; confirm that's an acceptable sequencing
   rather than an expected day-one capability.
6. **Should sharing a journey into a group also grant `group_trains`-level
   access to its individual legs**, so a member could see a leg's detail
   even outside the journey view (e.g. from a future "all trains shared
   with me" list) — or is journey-scoped access (§6's new authorization
   path) sufficient and legs never independently visible? This spec
   assumes the latter (simpler, one new authorization check, no duplicate
   `group_trains` rows to keep in sync).

---

## 9. Phased delivery plan (proposal)

**Phase 1 — Single-leg journeys + migration + time-window search
(manual-pick, with persisted-window reselect) — the "supersede"
groundwork.** Revised scope, per the 2026-09-22 decision resolving Open
Question #1: this phase now absorbs the manual-pick half of what was
previously Phase 2, because "manual pick + persisted window + Change
train" is the actual Phase 1 requirement, not a stretch goal layered on
top of a bare migration. Backend: `journeys`/`journey_legs` migration
(§1.1) + the historical-data migration (§7.1); `POST /Journeys` (wrapping
existing `create_pin`/`create_subscription_for_train`, §7.2, for a
direct-known-train leg) **and** `POST /Journeys` with an open
origin/destination/window body (§2.1's query extension +
`GET /Journeys/{id}/legs/{legId}/candidates`, §2.2, for a window-search
leg); the commit route (§2.3, `'manual'` mode only) reused unchanged for
both the first pick and any later "Change train" re-pick (an `UPDATE`, not
a new leg); `GET /Journeys/mine`, `GET /Journeys/{id}` (single leg only,
reusing `TRACKED_TRAIN_STATE_SELECT` verbatim, §4). Frontend:
`TrackTrainForm`/`TrackThisTrainButton` rewired to the new creation route
(supporting both a direct pick and a window search, reusing
`TrainSearchForm`'s existing before/after time-window UI, §0.5); the
open-leg candidate-list card and matched-leg "Change train" action (§4).
**Still explicitly out of Phase 1: multi-leg chaining (adding a second leg
to an existing journey, §3), `'auto'` mode, new notification logic, group
sharing.** This phase satisfies requirement #1 (single-train tracking
preserved), the manual-pick half of requirement #2, and lands the bulk of
requirement #6b (migration + superseding entry point) — the query
extension is the one genuinely new piece of logic; everything else is
composition of existing primitives (`create_subscription_for_train`, the
existing pending-pin sweep pattern, `TrainSearchForm`'s window UI).

**Phase 2 — Multi-leg chaining.** Backend: `POST /Journeys/{id}/legs`
(both creation shapes, §3, reusing Phase 1's commit route and candidate
query per-leg rather than per-journey). Frontend: "Add a leg" flow, the
per-leg-status-rollup journey summary badge (§3). **Complexity: low-medium**
now that Phase 1 already ships window-search and manual-pick — this phase
is materially smaller than originally scoped, since it's "let a journey
have more than one leg card," not "invent window search."

**Phase 3 — Journey-aware notifications + station skip.** Backend: journey/
leg-aware `NotificationPayload` copy (§5.1); the Darwin-sample skip lookup
(§5.2, modeled on `eta_blend::find_darwin_eta`) wired into both the journey
view (§4's skip badge) and a new notifier decision path + state table +
`main.rs` interval branch. **Complexity: medium-high** — this is the one
phase with genuinely new detection logic (nothing today ties
`skipped_stations` to a tracked train), not just composition of existing
pieces.

**Phase 4 — Group sharing.** Backend: `group_journeys` table + routes
(§6, a near-verbatim copy of `group_trains`'s pattern) + the one real new
piece, the journey-scoped cross-owner read authorization (§6's final
paragraph). Frontend: journey share button, `SharedJourneyRow` in the
group detail page (mirroring `SharedTrainRow`). **Complexity: low-medium**
— mechanically close to Phase 3 of the operator-overview spec's own E
("once the pattern exists elsewhere, copying it is mostly typing"), except
for the cross-owner read path, which needs real design review.

**Phase 5 (stretch, not committed) — connection buffers, auto-commit
matching, platform display once the in-flight sibling work lands.** Each
is independently addable once its own dependency (§0.6 for platform, Open
Question #1's resolution for auto-commit) is settled; none blocks shipping
Phases 1-4.

**Not phased separately: reusing `TrainJourney`/`JourneyTimeline`/
`JourneyProgress` for per-leg display** — already fully built (§0.2),
needs no work beyond embedding.
