# Design: Shared Train Identity — the Backend Foundation for a Public Train-Status Page

**Status: approved design, spec stage only. No migration, no Rust code, no
config change in this pass.**

Triggered by a new product surface: a public "train status" page that lets
any user look up any train — via direct search by headcode/UID or by
origin+destination+time, or via a refactored departure-board picker reached
from a station/line page — and see its current position, scheduled calling
points, and delay, with Track/Pin-ticket buttons at the top. Those lookup
and picker UIs are separate frontend sub-projects, out of scope here. This
document covers only the backend data-model and ingestion foundation that
page (and the existing track-a-train flow) will run on.

The core requirement driving this design: **trains must be de-duplicated
across users.** Today, `tracked_trains` is a per-user row that owns its own
copy of every piece of schedule and live-movement data
(`crates/api/migrations/20260828120000_train_tracking.sql`) — if two users
happen to track the same physical service, they get two independent rows,
two independent `train_movement_events` histories, and two independent
`train_current_state` rows, each fed by its own copy of the same TRUST
messages. A train has to become a shared, public entity, keyed by its own
real-world identity, not by who happened to pin it first. A user's
relationship to a train — a custom display name, notification preferences,
attached tickets — is a separate, private, per-user layer on top of that
shared entity. This split is what makes a train's URL meaningfully
shareable regardless of whether the recipient already tracks it, and it
means users never learn that other users track the same train.

Required reading consumed in full before this document was written:
`crates/api/migrations/{20260828120000_train_tracking,20260829090000_journey_ticket_tracking,20260901140000_standalone_tickets,20260902100000_notifications,20260905130000_custom_tracking_names,20260905150000_schedule_matched_resolution,20260905160000_trust_event_backlog}.sql`;
`crates/api/src/data/{train_tracking,schedule_matching,trust_event_backlog_match,queries}.rs`;
`crates/api/src/routes/train.rs`; `crates/api/src/app.rs`
(`schedule_crs_line_index`); `crates/trust-consumer/src/{matching,process}.rs`;
`crates/trust-backlog-consumer/src/process.rs`; `crates/notifier/src/queries.rs`;
`crates/aggregator/src/{main,queries}.rs`;
`crates/full-coverage-consumer/src/{correlate,population}.rs`;
`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
(the schedule-first design this work builds on top of and partially
supersedes); `docs/superpowers/specs/2026-09-06-schedule-line-population-future-dates-design.md`
and `2026-09-06-schedule-line-population-past-dates-design.md` (the two
sibling documents in this same family, written in parallel); and
`docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md` (the
design behind `trust_event_backlog`, kept and reused here rather than
retired).

## 1. Data model

### New table: `trains`

Shared, public, keyed by real-world identity rather than by who pinned it:

```sql
CREATE TABLE trains (
    id                  BIGSERIAL PRIMARY KEY,
    train_uid           TEXT NOT NULL,
    service_date        DATE NOT NULL,

    -- Schedule-derived (mirrors tracked_trains' own schedule-match columns,
    -- 20260905150000_schedule_matched_resolution.sql).
    origin_crs          TEXT,
    scheduled_departure TIMESTAMPTZ,
    destination_crs     TEXT,
    calling_points      JSONB,   -- was tracked_trains.schedule_calling_points
    matched_line_id     TEXT,    -- audit only, same posture as today
    schedule_matched_at TIMESTAMPTZ,

    -- Live-TRUST-derived.
    train_id            TEXT,    -- TRUST's own daily identifier -- NOT the
                                  -- same concept as this table's own
                                  -- surrogate `id` column. See the naming
                                  -- note below.
    resolved_at         TIMESTAMPTZ,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (train_uid, service_date)
);
```

**Deliberately no `resolution_status` enum on this table.** Today's single
CHECK-constrained column (`pending` / `schedule_matched` / `resolved` /
`unresolved`, `crates/api/migrations/20260905150000_schedule_matched_resolution.sql`)
encodes a linear progression that assumed schedule data and live data
always arrive to the same row, roughly in lockstep, for the same user's
pin. Once a `trains` row can be created by broad ingestion with no
subscriber at all, or by a schedule match with no live data yet, or by a
live Activation with no schedule match yet, "one row, one status" stops
describing reality — schedule data and live data are now two independent
arrival events, not two steps of one pipeline. This design deliberately
drops the enum and derives status from column presence instead:
`calling_points IS NOT NULL` means "has schedule data," `train_id IS NOT
NULL` means "has live data." The two are independent booleans, not a
single ordered state.

**Accepted gap, not solved here**: some `trains` rows will never gain
schedule data at all — a train discovered purely through broadened
ingestion with no subscriber, or one looked up by a bare `train_uid` whose
origin CRS was never seen so the existing `crs_to_line_ids` reverse index
(`crates/api/src/data/schedule_matching.rs::crs_to_line_ids`, built from
the static `lines/*.toml` catalogue and cached on `App` as
`schedule_crs_line_index`, `crates/api/src/app.rs:55,439-440`) has nothing
to key off. This design does **not** build a UID-keyed reverse schedule
index to close that gap — considered and rejected as YAGNI, deferred to
whenever the search sub-project is actually scoped, at which point real
usage data will say whether it's worth building.

### `tracked_trains` renamed to `train_subscriptions`

The rename itself touches every raw SQL string across `crates/api` that
names `tracked_trains` — this crate uses runtime-checked `sqlx::query`/
`sqlx::query_as`, not the `query!`/`query_as!` compile-time macros
(confirmed directly: no `.sqlx` query-cache directory or `query!` macro use
was found anywhere under `crates/api/src`), so a stale table name is a
runtime failure, not a compile error. **Sequence this rename as a late,
purely cosmetic step, after every functional change below has landed and
been verified — never as an early or combined step.**

Shrunk to a per-user join table over the shared `trains` row:

```sql
ALTER TABLE tracked_trains
    ADD COLUMN trains_id BIGINT REFERENCES trains(id) ON DELETE SET NULL;
```

`trains_id` is nullable, and `ON DELETE SET NULL` (not `CASCADE`) is
load-bearing: when a `trains` row is pruned (§5), the subscription — and
its custom name, tickets, notification history — must **survive** as a
no-live-data historical record, not cascade away with the train it once
pointed at.

New column, not a carry-forward: `notifications_enabled BOOLEAN NOT NULL
DEFAULT TRUE`. Flagged explicitly as new product surface needing
product-owner confirmation before it ships: no per-subscription mute/opt-in
flag exists anywhere in this schema today — `train_notification_state`
(`crates/api/migrations/20260902100000_notifications.sql:29-36`) is
escalation *state* (what was last sent, and when), not a stored user
*preference* for whether to send anything at all.

Retained as-is: `service_date`, `pin_origin_crs`, `pin_scheduled_departure`,
`pin_destination_crs`, `pin_operator`, `resolution_status`, `tracked_at`,
`custom_name`. `resolution_status`'s CHECK constraint narrows from today's
four values (`pending`, `schedule_matched`, `resolved`, `unresolved`) to
just `('pending', 'unresolved')` — once a subscription is matched to a
`trains` row, status lives on that joined row via column presence (above);
the per-user row's own status only matters pre-merge, on the legacy
CRS+time fallback path (§4) that still has to find its `trains` row before
one exists.

Dropped, once the migration cutover (§2) is verified complete:
`train_uid`, `train_id`, `matched_line_id`, `schedule_calling_points`,
`schedule_destination_crs`, `schedule_matched_at`, `resolved_at` — every
column that duplicated what now lives on `trains`.

### `train_movement_events` / `train_current_state` re-pointed

Both tables gain:

```sql
trains_id BIGINT REFERENCES trains(id) ON DELETE CASCADE
```

`CASCADE` here is correct and intentional, in deliberate contrast to
`train_subscriptions.trains_id`'s `SET NULL` above: neither of these two
tables has any reason to survive past its own train's retention window —
there is no private, user-owned data on either row worth preserving once
the train itself is pruned.

- `train_movement_events`'s dedup constraint moves from `UNIQUE
  (tracked_train_id, dedup_key)` to `UNIQUE (trains_id, dedup_key)`.
- `train_current_state`'s primary key moves from `tracked_train_id BIGINT
  PRIMARY KEY` (one row per subscription) to `trains_id BIGINT PRIMARY
  KEY` (one row per physical train, globally). This is the single
  concrete schema fact that makes an unsubscribed train's live status
  visible at all — today, `train_current_state` literally cannot answer
  "where is this train" for a train nobody has pinned, because no row
  exists to ask.

**Naming note, called out deliberately**: the new column is named
`trains_id`, not `train_id`. `train_id` already names something else in
this exact schema — TRUST's own daily identifier string
(`tracked_trains.train_id`, `trust_event_backlog.train_id`,
`crates/api/migrations/20260828120000_train_tracking.sql:68-69`,
`crates/api/migrations/20260905160000_trust_event_backlog.sql:48-50`).
Reusing that name for a different, `BIGINT`, surrogate-key concept (the new
`trains.id`) would collide two genuinely different identifiers — one is a
string TRUST assigns per real-world day, the other is this schema's own
internal row pointer — under one column name across several tables. Every
new FK introduced by this design is named `trains_id` for this reason,
consistently.

### `tracked_train_tickets` / `train_notification_state`: no schema change beyond the rename

Both already correctly point at the per-user table
(`tracked_train_tickets.tracked_train_id`,
`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:28`;
`train_notification_state.tracked_train_id`,
`crates/api/migrations/20260902100000_notifications.sql:31`), both already
`ON DELETE CASCADE` off it, and this is exactly right to keep — a ticket or
a notification-escalation record is inherently private to the user who
attached it, never shared with other subscribers of the same train. This
identity split never touched them; the only change either needs is
following `tracked_trains`' rename to `train_subscriptions` if the FK's
target-table reference needs updating at that late, cosmetic step.

## 2. Migration sequencing — expand/contract, not big-bang

Four tables currently FK into `tracked_trains.id`: `tracked_train_tickets`,
`train_current_state`, `train_movement_events`, `train_notification_state`
(confirmed directly against every migration file above). Re-pointing all
four at once, in one migration, would mean a single all-or-nothing cutover
across live-serving tables — this design stages it instead.

**Step A — expand, additive, independently deployable.** `CREATE TABLE
trains`. Add nullable `tracked_trains.trains_id`. App code starts
dual-writing: every **new** subscription find-or-creates a `trains` row and
sets `trains_id` on the subscription, alongside the existing, untouched
columns it already writes. `attempt_schedule_match`
(`crates/api/src/data/schedule_matching.rs`) and the backlog-replay path
(`crates/api/src/data/trust_event_backlog_match.rs`) both mirror their
result onto the shared `trains` row too. Zero observable behavior change
for any existing caller — this step only ever adds writes, never redirects
a read.

**Step B — one-off idempotent backfill.** For every existing
`tracked_trains` row with `train_uid IS NOT NULL`, find-or-create the
matching `trains` row and set `trains_id`:

```sql
INSERT INTO trains (train_uid, service_date, ...)
VALUES (...)
ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid
RETURNING id;
```

The `DO UPDATE` (rather than `DO NOTHING`) is what makes `RETURNING id`
reliable on a re-run against a row that already exists — safe to execute
more than once.

**Named edge case**: a row can have `train_id` set while `train_uid` is
still `NULL` — the resolved-via-live-Movement-alone path, where a schedule
match never happened. This row has no natural key (`train_uid`,
`service_date`) to backfill by. **Recommendation: leave these permanently
un-repointed** (`trains_id` stays `NULL`) rather than synthesizing a
placeholder identity for them — but before committing to that as final,
someone should run `SELECT count(*) FROM tracked_trains WHERE train_id IS
NOT NULL AND train_uid IS NULL` against real production data, so the size
of the gap being accepted is known rather than assumed. No existing
one-off-backfill-job convention was found anywhere in this repo to follow
(`crates/aggregator`'s prune jobs are recurring, not one-off; no
`bin/`-style one-shot job target exists in any crate today) — deciding
where this job lives (a `bin` target, a standalone script run once against
production, a temporary route) is flagged as a pre-implementation-planning
decision, not resolved here.

**Step C — cut reads over.** Once `SELECT count(*) FROM tracked_trains
WHERE train_uid IS NOT NULL AND trains_id IS NULL` returns `0`, flip the
read queries — `get_by_uid_and_date`, `get_by_tracking_id`, the `/Train/mine`
list query, all in `crates/api/src/data/train_tracking.rs` — to join
through `trains_id` into `trains`/`train_current_state` instead of reading
`tracked_trains`' own schedule/status columns directly. A pure read-path
change, trivially revertible by reverting the query text alone.

**Step D — re-point the movement tables.** Add nullable `trains_id` to
`train_movement_events` and `train_current_state`. Backfill via a batched
`UPDATE` joining through `tracked_trains.trains_id` — recommend batching by
id range (no existing large-backfill precedent in this repo to cite for
either table's row-count profile, so this is a defensive recommendation,
not one grounded in a measured need). Rows whose owning subscription's
`trains_id` is itself `NULL` (Step B's named edge case) simply can't be
re-pointed and stay `NULL` permanently — keep the column nullable rather
than forcing `NOT NULL` on either table.

Two required code changes fall out of this step, not just schema:

- **Split the combined upsert function.** `upsert_train_event`
  (`crates/api/src/data/train_tracking.rs:394-462`) today does two things
  in one call: write a movement/current-state row, and flip the owning
  subscription's `resolution_status`. Split it into two: one that writes
  movement/current-state for **any** `trains_id`, unconditional on whether
  any subscription references it at all (needed because most trains a
  broadened ingestion sees will have zero subscribers), and a much smaller
  one, scoped only to the legacy per-user resolution-flipping path, called
  in addition when a subscription actually exists.
- **`notifier`'s fan-out query must become one-to-many.** Today's
  watermark query, `SELECT DISTINCT tracked_train_id FROM
  train_movement_events WHERE id > $1` (`crates/notifier/src/queries.rs:143`),
  assumes one event maps to one user's row. Once one physical train can
  have many subscribers, this has to become a `trains_id`-keyed fan-out —
  find every subscription pointing at that `trains_id`, notify each,
  subject to each one's own cooldown/escalation state. This is a real,
  non-trivial follow-on change; it is **named here as a dependency for the
  implementation plan, not designed or solved in this document.**

Only after all of the above is verified via a dry-run row-count comparison
between the old and new columns: drop `tracked_train_id` from both tables,
and drop the now-redundant schedule/status columns from `tracked_trains`
(the `train_subscriptions` rename, §1, can happen any time after this, as
its own late, cosmetic step).

**Rollback posture.** Steps A, B, and C are each independently revertible —
A and B only ever add data, and C's read-path flip can be reverted by
reverting the query text alone, with no data loss on either side. Step D is
the hardest to reverse, since it changes what these two tables' *primary
identity* means (per-subscription to per-train) rather than only adding a
column — this is exactly why its final, irreversible act (dropping
`tracked_train_id`) is gated on a verified dry-run row-count comparison,
not performed as part of the same migration that adds `trains_id`.

## 3. Ingestion pipeline changes

**`trust-backlog-consumer` becomes the primary movement-event writer** for
the new shared `trains`/`train_movement_events`/`train_current_state`
tables. Its retention can start directly at ~30 days from day one, reusing
the past-dates sibling document's own figure (`2026-09-06-schedule-line-population-past-dates-design.md`) —
the RDM Data Marketplace licence question that gates `trust_event_backlog`'s
own retention increase has **already been confirmed by the repo owner as
fine to proceed on**, so it is not a blocker here. This is a deliberate
contrast with `trust_event_backlog_retention_days`'s own cautious
default-1-day-until-confirmed posture
(`crates/api/migrations/20260905160000_trust_event_backlog.sql:6-15`,
`crates/aggregator/src/main.rs:250-261`) — that caution doesn't need
reproducing for the new tables, since the underlying licensing question is
already answered; it only ever applied to `trust_event_backlog`'s own
separate retention knob.

Two concrete, required code changes, not just configuration:

**1. Close the live-ingest-time `train_uid` correlation gap.**
`trust-backlog-consumer`'s `ProcessorState` (`crates/trust-backlog-consumer/src/process.rs:40-42`)
tracks `pending_service_dates: HashMap<train_id, NaiveDate>` from Activation
messages, but has no equivalent map for `train_uid` — every outgoing
Movement/Cancellation event is written with `train_uid: None`
unconditionally today (`process.rs:122,152`), with the comment "this
consumer doesn't correlate Activation->Movement in-process; api's own
backlog-match joins them at read time instead." That was an accepted
deferral when this table was a rare, narrow backlog lookup for a small
number of late-tracking pins. It is **not** acceptable once this consumer
is the primary writer for a shared store meant to serve arbitrary,
previously-untracked trains: without a real `train_uid` on every event,
nothing can key a Movement into the right `trains` row at all.

Required change: add `pending_train_uids: HashMap<train_id, String>` to
`ProcessorState`, populated identically to `pending_service_dates` from the
same Activation message (`process.rs:57-59`'s exact pattern), so every
subsequent Movement/Cancellation for that `train_id` carries the real
`train_uid` in-process, before the event ever reaches `api`.

Residual, accepted gap: a Movement/Cancellation for a `train_id` whose
Activation was never seen by this process (a consumer restart mid-journey,
for instance) still has no `train_uid` to attach and is dropped for
shared-store purposes. This is the same accepted-gap posture this codebase
already takes for the structurally identical situation in `trust-consumer`
(`crates/trust-consumer/src/process.rs`'s own `pending_activations` map has
an identical "an Activation this process never saw" limitation) — not a new
category of gap this design introduces.

**2. Add a `train_uid` read path to `trust_event_backlog` itself.** This
table is kept, not retired (below), specifically for the legacy fallback
path's CRS+time lookup — but no index on `train_uid` exists on it today.
Confirmed directly against its migration
(`crates/api/migrations/20260905160000_trust_event_backlog.sql`): the only
indexes are a unique index on `dedup_key` (line 104-105), a partial index
on `(crs, planned_timestamp) WHERE crs IS NOT NULL` (line 110-112), and one
on `(train_id, service_date)` (line 127-128) — none on `train_uid`. Add:

```sql
CREATE INDEX ... ON trust_event_backlog (train_uid, service_date)
    WHERE train_uid IS NOT NULL;
```

plus a small new query function that resolves `train_id` from a bare
`train_uid` + `service_date` via the Activation row (the only row type in
this table that ever carries a `train_uid` at all,
`trust_event_backlog.sql:42-45`), then reuses whatever existing backfill
query already fetches full history by `train_id` (the plan behind this
table names it `fetch_backlog_history`) unchanged.

**`trust_event_backlog` is kept, not retired.** Approved decision: it
remains useful as a narrower, still-relevant table for the legacy
fallback path's CRS+time lookup even once the new `trains`/movement tables
exist and cover the primary case. Its own scope-narrowing tradeoffs
(Activation/Cancellation/Movement only, no `raw_body`, per its own header
comment) are unaffected — this design does not fold its use case into the
new tables, or the new tables' use case into it. Both systems are kept,
each serving its own scope.

**`trust-consumer` is repurposed, not deleted.** Today it independently
derives *and persists* its own copy of history via a fragile CRS+time+
departure-only heuristic, `resolve_origin_departure`
(`crates/trust-consumer/src/matching.rs:28-46`), used because a pin's
`train_uid` was never known upfront. Under the new model, persistence of
movement events moves entirely to `trust-backlog-consumer`. For the
NR-primary case (§4), a genuine simplification falls directly out of the
earlier decisions in this document: since an NR-primary subscription's
`train_uid` is known *before* any TRUST message ever arrives,
`trust-consumer` no longer needs to discover the live `train_id` via
heuristic CRS+time matching for that case at all — it can wait for the
Activation carrying that exact `train_uid` and bind by direct equality,
strictly more reliable than a ±20-minute tolerance window
(`common::MATCH_TOLERANCE`).

Concretely: add a second lookup map, `by_train_uid: HashMap<String,
Vec<subscriber_id>>`, seeded from every active subscription whose identity
is already known, alongside the existing CRS+time-keyed structure
`resolve_origin_departure` already checks. On any Activation, check
`by_train_uid` first; fall back to the existing CRS+time+departure
heuristic only for subscriptions genuinely still lacking identity — which,
per the legacy-fallback framing (§4), narrows to just that rare path going
forward.

`trust-consumer`'s new job, once a live message matches a currently-active
subscription: write a lightweight forwarding row into a new, small,
queue-style table that `notifier` polls on a tighter/faster timer than its
existing, slower poll of `train_movement_events`
(`crates/notifier/src/queries.rs:143`). This is the approved mechanism —
it preserves `notifier`'s existing tuned cooldown/escalation decision logic
as the sole gatekeeper for actually sending a push, rather than
`trust-consumer` writing directly into `train_notification_state` and
bypassing that logic entirely, and rather than a new synchronous RPC that
would couple the two services together at request time. The new table's
shape is deliberately minimal — a forwarding signal, not a data store — on
the order of `(trains_id, event_summary, created_at)`; `notifier`
incorporates polling it as a second, faster-cadence query alongside its
existing one, still deciding whether to actually notify via its own
unchanged cooldown/escalation logic.

**`full-coverage-consumer` is unaffected.** Confirmed directly: neither of
its output tables carries a `train_uid` column (it is aggregate-only, per
`crates/full-coverage-consumer/src/{correlate,population}.rs`) — it has no
relationship to individual train identity and nothing in this redesign
touches it.

## 4. API surface changes

**`GET /Train/by-uid/{train_uid}/{date}` — access model inverted.** This
route already exists today
(`crates/api/src/routes/train.rs:80-83,625-654`) but is backwards for the
new goal: it requires `AuthenticatedUser` and returns `404` unless the
caller's own subscription owns a matching row
(`get_by_uid_and_date`/`tracked_train_owner`, `routes/train.rs:625-654`,
confirmed directly — the handler fetches state unscoped, then explicitly
checks `owner == user.id` and 404s otherwise). **Approved change: drop the
auth/ownership requirement entirely.** Read the shared `trains` row (joined
with the re-pointed `train_current_state`) unconditionally — this becomes
the new public train-status page's backend.

Call this out explicitly as what it is: a real, visible API-contract
change to an existing route. Today: "your own tracked trains only, 404 for
anyone else's." Tomorrow: "anyone can look up any known train." This is
not a drop-in rename and should get explicit sign-off in review, separate
from the rest of this migration's otherwise-additive posture. Optionally,
if the caller *is* authenticated, the response can include their own
subscription id/custom name as a "you're already tracking this" hint —
nice-to-have, not required for this change to ship.

**New `POST /Train/by-uid/{train_uid}/{date}/track`.** For the NR-primary
path: authenticated, find-or-creates the `trains` row for that identity
(an idempotent upsert, same shape as Step B's backfill) and creates a
`train_subscriptions` row with `trains_id` set immediately — no
`pending`/`schedule_matched` waypoint at all, since identity is already
known at request time. A dedicated endpoint is recommended over overloading
the existing `POST /Train/track` request shape with a conditionally
required `train_uid` alongside legacy-only fields (`pin_origin_crs`,
`pin_scheduled_departure`, etc.) that an NR-primary caller shouldn't need
to supply at all.

**`POST /Train/track` stays, URL and request shape unchanged** — it becomes
exclusively the legacy fallback path's entry point: the rare,
non-NR-integration case where identity isn't known upfront (today's
CRS+time pin-creation flow, `routes/train.rs:420-484`). Internally updated
to find-or-create a `trains` row and link the subscription once a match
succeeds (via `attempt_schedule_match`/`attempt_backlog_match`, unchanged
in their own matching logic), instead of writing straight onto
`tracked_trains`' own now-legacy columns.

**`GET /Train/{trackingId}` and `GET /Train/mine` stay private and
ownership-gated, exactly as today.** Their underlying queries
(`get_by_tracking_id`, and the `/Train/mine` list query,
`crates/api/src/data/train_tracking.rs:663-677`) change from directly
selecting `tracked_trains`' own schedule/status columns to joining through
`train_subscriptions.trains_id -> trains -> train_current_state`. For a
still-unresolved legacy subscription (`trains_id IS NULL`), every
trains-derived column comes back `NULL` via the `LEFT JOIN`, identical in
observable behavior to today's "pending pin has `NULL train_uid`" shape —
no new null-handling logic needed at these two call sites.

**Ticket routes, rename routes, delete routes: unchanged.** They operate
entirely on the per-user table (`tracked_train_tickets`,
`train_subscriptions.custom_name`), which shrinks under this design but
never disappears, and their own ownership-scoped queries
(`WHERE id = $1 AND user_id = $2`, `crates/api/src/data/train_tracking.rs:729-736`)
need no change at all.

## 5. Retention / pruning

New `prune_trains` function in `crates/aggregator`, mirroring
`prune_trust_event_backlog`'s exact shape
(`crates/aggregator/src/queries.rs:503-511`):

```sql
DELETE FROM trains WHERE service_date < CURRENT_DATE - ($1 || ' days')::interval
```

A single, backward-only predicate on the parent row — movement events only
ever concern past/current dates, so there is no "forward" retention
component to reason about here, unlike `schedule_line_population`'s own
two-sided (past-dates/future-dates) retention question in the sibling
documents. `ON DELETE CASCADE` on `train_movement_events`/`train_current_state`
does the rest inside the same statement. `train_subscriptions.trains_id`'s
`ON DELETE SET NULL` (§1) is what makes the per-user subscription row
survive this delete.

Retention value: **30 days**, reusing the past-dates sibling document's own
figure rather than inventing a second, independent number for a
structurally similar concern. Unlike `trust_event_backlog_retention_days`'s
cautious default-1-until-licence-confirmed posture
(`crates/aggregator/src/main.rs:250-261`), this new config field can
default straight to 30 from day one, since the underlying licensing
question this caution exists to enforce is already closed for this data.

Recommend placing this alongside `crates/aggregator`'s existing five
structurally identical prune jobs — `prune_history`, `prune_trust_event_backlog`,
`prune_daily_stats`, `prune_half_hourly_stats`, `prune_daily_coverage_stats`,
`prune_half_hourly_coverage_stats` (`crates/aggregator/src/queries.rs`,
all called from the same per-cycle loop in `crates/aggregator/src/main.rs:62-`) —
for one single place to look for every prune job in this codebase. This is
an open placement question the past-dates sibling document also left
unresolved for its own future prune job (§Dimension 6 there); noted here as
the same parallel, not resolved differently.

## 6. Non-goals

- **The frontend train-status page itself, the departure-board picker
  refactor, and direct search.** Separate frontend sub-projects; this
  document is backend-foundation only.
- **`full-coverage-consumer` changes.** Confirmed unaffected (§3).
- **A UID-first schedule reverse-index.** Explicitly deferred as an
  accepted gap (§1) — not solved here; YAGNI until the search sub-project
  is scoped and real usage data exists.
- **Redesigning `notifier`'s escalation/cooldown decision logic.** This
  spec only adds a second, faster-polled input feeding into that existing
  logic (§3) — the decision logic itself is untouched.
- **Retiring `trust_event_backlog`.** Explicitly kept, not folded into the
  new tables (§3).
- **The exact backfill-job implementation and location for Step B (§2).**
  Flagged as a pre-implementation-planning decision, not resolved here.
- **`MAX_PIN_AGE` / forward pin-creation window numeric bounds.** This
  document references the past-dates/future-dates sibling documents'
  figures as given, existing bounds — it does not re-derive or change
  either one.
- **UI/copy for the new "historical record, no live data" per-user
  subscription state that pruning creates** (a `train_subscriptions` row
  whose `trains_id` has gone `NULL` after its `trains` row was pruned).
  This design's schema enables that state to exist and survive cleanly;
  designing copy for it is not part of this spec.
- **MCP server improvements tying into this new shared schedule data.**
  Explicitly out of scope for this spec — noted as a planned follow-up
  sub-project to be brainstormed fresh once this work lands, not scoped
  here even at a high level.

## 7. Open questions / risks

1. **Backfill edge-case size (§2, Step B).** Needs a real production-data
   count — `SELECT count(*) FROM tracked_trains WHERE train_id IS NOT NULL
   AND train_uid IS NULL` — before the "accept the gap, leave these rows
   permanently un-repointed" posture can be treated as final rather than a
   working assumption.
2. **`train_subscriptions.trains_id`'s nullability.** Kept nullable given
   the Step B backfill edge case and Step D's re-pointing gap; worth
   revisiting once real data from Open Question 1 is known — a large
   enough gap might argue for a different backfill strategy instead of
   permanently nullable.
3. **The new notifier-forwarding queue table's exact shape, and
   `notifier`'s polling-cadence change.** Sketched at a high level in §3
   (a minimal `(trains_id, event_summary, created_at)`-style row, polled
   on a second, faster-cadence query) — needs concrete design during
   implementation planning, including the exact cadence chosen and how
   consumed rows are cleared or pruned.
