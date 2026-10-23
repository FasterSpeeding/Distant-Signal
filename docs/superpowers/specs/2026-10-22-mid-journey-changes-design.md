# Mid-journey train changes and auto-suggest (2026-10-22)

Status: design, awaiting sign-off on §14 before implementation. Work log
DS-128.

The request, from the user, in short:

- Choosing an alternative for a missed leg must be easy. The leg may be
  missed through the user's own lateness, or because an earlier leg was
  late or cancelled. It must be a lighter "choose an alternative" flow, not
  full leg editing.
- Replacing a leg must never destroy what was booked or planned. The
  older plan is kept for Delay Repay and for history, especially when a
  train is cancelled.
- When an earlier leg changes, later legs are updated, or updates are
  suggested.
- New suggestions appear by themselves whenever a journey stops working:
  a connection missed on live times and minimum connection times (MSN), a
  cancellation, or the user marking a train as missed.
- **The headline, early-phase feature is proactive auto-suggest
  notifications.** When a journey breaks, a push (or in-app notice)
  deep-links straight to suggestions already computed for that journey.
  It must:
  - send one notice per break, re-notify only if things get worse, go
    quiet once the user acts, and stay inside the one-push-per-user-per-
    train rule;
  - re-plan on open when the suggestions are stale;
  - offer a one-tap accept that keeps the original plan for Delay Repay;
  - have an opt-out;
  - be measured: sent, opened and accepted.

Builds on:

- `2026-09-22-journey-tracking-design.md` (journeys and legs);
- `2026-09-28-trips-plan-live-overlay-design.md` (the live overlay this
  design reuses as its feasibility engine);
- `2026-10-20-untracked-legs-and-tracked-train-states-design.md` (Tube,
  bus and walk legs);
- DS-93, "Change train drops the old train" (`release_leg_subscription`,
  migration `20261021120000`);
- `2026-10-21-tracked-trains-as-journeys-design.md` (DS-124, in flight:
  every tracked train becomes a one-leg journey);
- `2026-10-21-pre-departure-prediction-design.md` (DS-123, a later input
  for "at risk");
- `2026-10-14-ds-mcp-delegated-access-design.md` (delegated clients).

Contents: §1 what exists today, §2 problem and user stories, §3 decisions,
§4 data model, §5 feasibility engine, §6 auto-suggest notifications, §7
API, §8 UX, §9 Delay Repay, §10 migrations and backfill, §11 edge cases,
§12 phasing, §13 risks, §14 open questions, §15 tests, §16 metrics,
§17 ranking alternatives.

## 1. What exists today

All paths are from the repository root, at `origin/main` 097fd582.

### 1.1 Journeys, legs and trains

| Thing | Where | Today |
|---|---|---|
| `journeys` | `crates/ds-store/migrations/20260922090000_journeys.sql` | `id`, `user_id`, `custom_name`, timestamps, later `source_template_id` and `created_via` (`20261015010000_delegated_access.sql`). |
| `journey_legs` | same file, plus `20260925090000` (`window_searched`), `20261020150000_journey_legs_untracked.sql` (`kind`, `transit_mode`, places, `planned_departure`/`planned_arrival`) | `UNIQUE (journey_id, leg_order)`. `train_subscription_id` FK `ON DELETE SET NULL`. `match_mode` is `unmatched`, `manual` or `auto`. No status, no "missed", no history. |
| `train_subscriptions` | `20260828120000_train_tracking.sql` (as `tracked_trains`), `20260906100000_trains.sql` | Per user, per train (`trains_id`). Idempotent per `(user_id, trains_id)` in code, under `common::pg::lock_user_train_subscription`, not by a unique index. `notifications_enabled` is internal only. `tracked_standalone` (DS-93) goes away under DS-124. |
| `tracked_train_tickets` | `20260829090000_journey_ticket_tracking.sql` | Tickets hang off the **subscription** (`tracked_train_id`), not the leg. |
| `group_journeys` | `20260922110000` | Journey shares. Train shares are `group_trains`. |

**Change train overwrites the leg in place.** `POST
/Journeys/{j}/legs/{l}/train` (`post_leg_train`,
`crates/api/src/routes/journeys.rs`) calls
`create_subscription_and_bind_leg_with_search`
(`crates/api/src/data/journeys.rs`). In one transaction it finds or
creates the new subscription, then `set_leg_train_subscription_locked`
runs `UPDATE journey_legs SET train_subscription_id = $1, match_mode =
'manual'` (and, with `search`, rewrites the leg's stations, date and
window). Then `release_leg_subscription(old, move_to = Some(new))` moves
the old row's tickets and `group_trains` to the new row and **deletes**
the old row unless another leg (or, until DS-124 Phase 2,
`tracked_standalone`) keeps it. Nothing records which train the leg had
before. So today a Change train after a cancellation loses the booked
train, and the Delay Repay estimate then describes the replacement
train, not the booked journey.

**"Change train" is hidden on most legs.** `JourneyLegCard.tsx` gates the
button on `hasWindow = leg.windowSearched`. `window_searched` is `TRUE`
only for window legs (`create_journey_with_window_leg`,
`add_window_leg_to_journey_at`). Pin legs, "I know the train" legs
(`create_journey_with_known_train_leg`, `add_known_train_leg_to_journey_at`)
and planner-saved legs (`create_journey_from_itinerary`) are all inserted
with `FALSE`. So a journey saved from the planner, the main way to make a
multi-leg journey, has no way to pick another train except "Remove leg"
then "Add a leg".

**Leg edits.** `PUT /Journeys/{j}/legs/{l}` (`put_journey_leg`) edits
untracked legs only. `POST /Journeys/{j}/legs?before={legId}` inserts. A
train leg's train changes only through `.../train`.

**Release paths** (all through `release_leg_subscription`): Change
train, `delete_leg`, `delete_journey_row_releasing_trains` (web and
delegated delete). Retention (`crates/api/src/data/retention.rs`,
`prune_past_travel`) deletes past subscriptions and relies on a leg and
its train sharing a date.

### 1.2 Status and connections

- **No leg-level missed state** anywhere: no column, no API field, no
  button. "Travelled" is computed (`crates/api/src/data/travel_order.rs`,
  `TRAVEL_ARRIVAL_GRACE_MINUTES = 30`).
- **Connections are judged only in the browser.**
  `frontend/lib/journeyStatus.ts` `worstConnection` uses
  `connectionGapMinutes` (`frontend/lib/journeyLegChaining.ts`): this leg's
  *live* expected arrival against the next leg's *timetabled* departure,
  with **no minimum connection time** ("No minimum interchange time is
  assumed"). A gap below 0 is `missed`; a late leg with a gap under
  `CONNECTION_AT_RISK_MINUTES = 5` is `atRisk`. A next train that is
  itself late is not credited, so a connection that still works on live
  times can show as missed.
- `summariseJourney` turns that into `connectionMissed` /
  `connectionAtRisk`, but only `journeyDetailSummary` feeds it (journey
  page, shared page). `GET /Journeys/mine` rows send `connection: null`,
  and the home card's `homeJourneyStatusGroup`
  (`frontend/lib/homeJourneys.ts`) uses `worstLegStatus`, so **the home
  Current journey card never shows a missed connection**.

### 1.3 The live overlay already knows when a plan breaks

`crates/api/src/data/trip_plan_live.rs` (spec 2026-09-28) is a pure,
layered engine used by `GET /Trips/plan`:

1. `fetch_train_lives` / `train_live`: each train's `TrainLive` from TRUST,
   Darwin board stops (`journey::build_journey_stops_batch`),
   `train_current_state` and `train_reasons`. It models whole and part
   cancellations (0002 `EN ROUTE` / `OUT OF PLAN` cut a train short),
   change of origin (0006) and skipped calls.
2. `evaluate_chain`: live times along the train, delays carried forward.
3. `build_overlay`: a `trip_planner::ConnectionOverlay` the planners
   search over.
4. `annotate`: per leg a `LegLive` with `interchange_feasible = live_dep >=
   previous_live_arrival (+ walk) + change_minutes(from_tiploc)`, and per
   itinerary `liveFeasible`.
5. `plan_invalidated` / `seed_became_catchable` decide whether to
   re-plan; `routes::trips::plan_live` runs up to `max_replans_for` rounds.

`LiveConfig` (same file): horizon 180 min ahead, lookback 120 min, TRUST
facts older than 30 min ignored, 60 trains per request. Costs, from the
same file: CSA 80-150 ms with live data; RAPTOR 0.5-1.3 s. Graphs are
cached per service date in `GRAPH_CACHE` (about 100 MB each, 2 dates,
600 s max age), searches are gated by `PLAN_SLOTS` (4) in
`crates/api/src/routes/trips.rs`.

**MSN and fixed links.** `crates/schedule-query/src/interchange.rs`:
`minimum_change_time` returns `ChangeTime::Finite` (default 5 min) or
`NoInterchange` (98/99). Fixed links come from `fixed_links` plus
`reference-data/supplementary-fixed-links.csv`, and since DS-81
(`index_fixed_links_both_ways`) are indexed in both directions.
`crates/api/src/data/trip_planning.rs::fetch_interchange_data` builds it.
The planners charge the MSN at every fresh boarding
(`csa.rs` `Scan::ready_source_at`, mirrored in `raptor.rs` and
`staged.rs`).

**Nothing applies this to a saved journey.** The overlay only ever sees
planner results.

### 1.4 Notifier

`crates/notifier` polls Postgres only (no streams). Web Push with VAPID
(`send.rs`), payload `{title, body, url, tag}`, through a bounded queue
with at-most-once delivery (`push_queue.rs`).

- **Train delay or cancel** (`decision.rs` `decide_train_notification`,
  `train_severity_rank`): escalation only, rank 0 normal, 1 delay of 15
  min or more (`train_delay_threshold_minutes`), 2 cancelled. State in
  `train_notification_state (user_id, tracked_train_id)`. A growing delay
  inside rank 1 does not re-push.
- **One push per user per train**: `candidates_for_trains_id`
  (`crates/notifier/src/queries.rs`) is `SELECT DISTINCT ON
  (ts.user_id) ... ORDER BY ts.user_id, <leg-backed first>, ts.id`,
  tested by `candidates_for_trains_id_notifies_a_user_once_per_train`.
  `journey_leg_for_train_subscription` takes the first leg when one
  subscription backs two.
- **Leg alerts**: skipped stop and template "unmatched", state in
  `journey_leg_notification_state`.
- **Deep links**: `/journeys/{journey_id}` for multi-leg trains, skip and
  unmatched (`main.rs`), never to a leg. One-leg and lone trains link to
  `/track/{tracked_train_id}` (`main.rs`, near line 556), **a route that
  does not exist**: `frontend/app/track` has only `page.tsx` and `mine/`,
  and `next.config.mjs` redirects only `/track/mine` and `/track/tickets`.
  So those pushes open a 404 today (§12, Phase 0).
- **Service worker** (`frontend/public/sw.js`): shows the payload, skips
  it when a focused tab is already on the URL, and `notificationclick`
  opens `sameOriginNotificationUrl(url)`. No action buttons, no open
  tracking.
- **Opt-outs**: none. No preference table, no quiet hours. A push
  subscription plus a tracked train or pinned line is the opt-in.
- **Metrics**: `notifier_push_*` counters (`push_queue.rs`), none by
  notification type. **No product analytics anywhere** (no events table,
  no click tracking).

### 1.5 Delay Repay

Pure and read-only, never submits a claim.

- `crates/api/src/data/delay_repay_rules.rs`: `Scheme` (DR15, DR30,
  Heathrow Express, own scheme), `OPERATORS`, `scheme_for`,
  `estimate_delay_repay(scheme, delay_minutes, provisional, ticket)`,
  `BORDERLINE_MINUTES = 3`, `classify_outcome(OutcomeFacts) ->
  Outcome::{Arrived, DepartedOnly, NotReached}` with `CancelPosition`,
  and `assess` returning `DelayRepayFields`.
- `crates/api/src/data/delay_repay_outcome.rs`: one batched query per
  `OutcomeTarget` (trains_id, uid, date, destination CRS) for TRUST
  arrival/departure/pass, `reported_beyond`, Darwin skipped calls, and
  the 0002 cancellation position.
- **Per ticket on one train**: `GET
  /Train/{id}/tickets/{tid}/delay-repay` (`routes/train.rs`), delay on
  public times at the ticket's destination (`delay_repay_destination`).
  No whole-journey view.
- `Outcome::NotReached` shows no percentage because "eligibility depends
  on the replacement journey" (`delay_repay_rules.rs`,
  `frontend/components/DelayRepayEstimate.tsx`). DS has no replacement
  journey to look at. This design supplies one.
- **No fault model.** `train_reasons` has TRUST reason codes for 0002 and
  0006 only; plain delays carry no reason.

### 1.6 Delegated clients

`crates/api/src/auth/delegated.rs` `DELEGATED_ROUTES` allows four
routes: `GET /Journeys/mine`, `GET /Journeys/{id}`, `POST
/Journeys/itinerary`, `DELETE /Journeys/{id}`. Every leg route,
Change train included, is cookie-only.

## 2. Problem and user stories

Today, when a journey breaks, the user has to notice (the home card does
not say), work out what to do (no MSN, no alternatives), and then edit
the journey by removing and re-adding legs, which on most legs is the
only way, and which destroys the booked plan.

| # | As a traveller | So that |
|---|---|---|
| U1 | when my first train is cancelled, I get one push "Your 08:00 to York is cancelled. 3 other ways to Edinburgh", and one tap shows them | I don't have to plan on the platform |
| U2 | when my inbound train is 12 minutes late and my 6-minute change at York (MSN 5) no longer works, I'm told before I arrive | I can head for the right platform |
| U3 | I tap "I missed this train" on the leg I just missed | the next options appear at once |
| U4 | I tap "Take this one" and my journey shows the new trains, with later legs fixed up | I don't edit legs one by one |
| U5 | after a cancellation, my journey still shows what I booked and how late I really got there | I can claim Delay Repay against the booked arrival |
| U6 | when I missed the train myself, DS doesn't suggest I'm owed Delay Repay for that | the estimate stays honest |
| U7 | I get no more pushes once I've chosen, and only one more if it gets worse | notifications stay useful |
| U8 | I can turn these suggestions off | I'm in control |
| U9 (owner) | I can see sent, opened and accepted counts | we know whether it helps |

## 3. Decisions

| # | Decision |
|---|---|
| D-1 | `journey_legs` stays the **current plan**, unchanged in meaning. History lives in new, append-only snapshot tables (§4). Every existing reader (notifier, retention, travel order, lists, DS-MCP) keeps working untouched. |
| D-2 | A plan change made **after the journey starts or after a break** is a *replacement*: it writes a new revision and never edits the booked revision. Before that point, edits are planning and update the booked snapshot (§4.3). |
| D-3 | The feasibility engine is the live overlay's pure layers (`train_live`, `evaluate_chain`, MSN via `minimum_change_time`, fixed links both ways), lifted into a module usable for saved journeys. One rule, server-side, for the planner, the journey page, the home card and the notifier. |
| D-4 | Break detection and suggestion planning run **in the api** (where the graph cache lives), as a `ds_store::loops` sweep under an advisory lock. The **notifier stays the only push sender**: it reads `journey_breaks` rows. |
| D-5 | A break notice for a train **replaces** that user's plain delay or cancel push for the same train, so the one-push-per-user-per-train rule holds (§6.3). |
| D-6 | Trains are never swapped without the user. Untracked legs (Tube, bus, walk) are shifted automatically after an upstream change, recorded as a revision (§5.5). |
| D-7 | Delay Repay gains a journey-level view: booked arrival (from the compensation baseline revision) against actual arrival at the final destination, with user-fault breaks moving the baseline (§9). |
| D-8 | Every new route is additive and cookie-only at first. `GET /Journeys/{id}` gains additive fields that delegated readers see; delegated writes are out of scope (§14 Q7). |

## 4. Data model: revisions and breaks

### 4.1 Tables

```sql
-- One immutable snapshot of a whole journey plan.
CREATE TABLE journey_revisions (
    id              BIGSERIAL PRIMARY KEY,
    journey_id      BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    revision_no     INT NOT NULL,             -- 1 = booked
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reason          TEXT NOT NULL,            -- booked | replace | auto_shift | revert
    cause           TEXT,                     -- see 4.2; NULL for booked
    fault           TEXT,                     -- operator | user | none | unknown
    break_id        BIGINT,                   -- FK added with journey_breaks
    actor           TEXT NOT NULL,            -- owner | system
    is_compensation_baseline BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (journey_id, revision_no)
);

-- Its legs, denormalised so they outlive subscriptions and trains rows.
CREATE TABLE journey_revision_legs (
    revision_id       BIGINT NOT NULL REFERENCES journey_revisions(id) ON DELETE CASCADE,
    position          INT NOT NULL,
    source_leg_id     BIGINT,                 -- journey_legs.id at the time; no FK
    kind              TEXT NOT NULL,          -- train | untracked
    train_uid         TEXT,
    service_date      DATE NOT NULL,
    trains_id         BIGINT,                 -- no FK: trains has its own retention
    atoc_code         TEXT,
    origin_crs        TEXT,
    destination_crs   TEXT,
    booked_departure  TIMESTAMPTZ,            -- public (GBTT) time at origin_crs
    booked_arrival    TIMESTAMPTZ,            -- public time at destination_crs
    transit_mode      TEXT, origin_place TEXT, destination_place TEXT, line_name TEXT,
    planned_departure TIMESTAMPTZ, planned_arrival TIMESTAMPTZ,  -- untracked legs
    ticket_ids        BIGINT[] NOT NULL DEFAULT '{}',
    fate              TEXT NOT NULL DEFAULT 'planned',
        -- planned | travelled | missed | cancelled | part_cancelled | not_used
    PRIMARY KEY (revision_id, position)
);

-- One row per break of a journey; at most one open per journey.
CREATE TABLE journey_breaks (
    id               BIGSERIAL PRIMARY KEY,
    journey_id       BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    detected_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    cause            TEXT NOT NULL,           -- 4.2
    fault            TEXT NOT NULL,           -- operator | user | unknown
    severity         SMALLINT NOT NULL,       -- 6.2
    trigger_leg_id   BIGINT REFERENCES journey_legs(id) ON DELETE SET NULL,
    trigger_trains_id BIGINT,
    break_position   INT NOT NULL,            -- first leg that no longer works
    fingerprint      TEXT NOT NULL,           -- inputs the suggestions came from
    status           TEXT NOT NULL DEFAULT 'open',
        -- open | accepted | dismissed | recovered | expired
    suggestions      JSONB,                   -- PlannedItinerary[] plus ranking
    suggestions_at   TIMESTAMPTZ,
    notified_at      TIMESTAMPTZ,
    notified_severity SMALLINT,
    opened_at        TIMESTAMPTZ,
    opened_via       TEXT,                    -- push | in_app
    resolved_at      TIMESTAMPTZ,
    accepted_revision_id BIGINT REFERENCES journey_revisions(id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX journey_breaks_one_open ON journey_breaks (journey_id)
    WHERE status = 'open';
CREATE INDEX journey_breaks_to_notify ON journey_breaks (detected_at)
    WHERE status = 'open';

CREATE TABLE notification_preferences (
    user_id              TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    journey_suggestions  BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Plus one catalog-only column on `journeys`:
`suggestions_muted BOOLEAN NOT NULL DEFAULT FALSE` (per-journey mute,
§6.5). CHECK constraints for the text enums follow the `NOT VALID`, then
`VALIDATE` two-file pattern (`20261020150000` / `...150100`).

Why snapshots of whole plans rather than per-leg versions: a replacement
often changes the shape of the journey (one cancelled leg becomes two
trains and a walk, or a re-route drops a change). Per-leg version rows
cannot express "these three legs replaced those two". A revision is the
whole plan, so any shape change is just the next snapshot, and
"booked versus actual" is a compare of two snapshots.

Why denormalised: `release_leg_subscription` deletes a replaced train's
subscription (DS-93), retention deletes `trains` and subscriptions, and
CIF short-term changes can move a train's booked times later. The
snapshot keeps the booked public times as they were when booked.
`train_uid` and `service_date` keep the public train page
(`/train/{uid}/{date}`) reachable for any old leg.

### 4.2 Causes and fault

| `cause` | Set when | `fault` |
|---|---|---|
| `cancelled` | a train leg's train is cancelled at or before the user's alighting stop (`LegLive.cancelled`) | operator |
| `part_cancelled` | the train is cut short or changes origin so it misses the user's boarding or alighting stop | operator |
| `connection_missed` | the next train leg's live departure is before the previous leg's live arrival plus MSN (or the fixed-link time) | operator |
| `user_missed` | the user taps "I missed this train", and the previous leg (if any) arrived in time for it on live times | user |
| `user_missed` with a late inbound | as above, but the inbound leg was late enough to break the connection | operator (data wins, §14 Q5) |
| `user_choice` | the user replaces a leg with no break open | none |
| `skipped_stop` | the train no longer calls at the boarding or alighting stop (Darwin) | operator |

### 4.3 When a revision is written

- **Booked (revision 1).** Written in the same transaction as every
  journey creation path: `create_journey_with_*`,
  `create_journey_from_itinerary`, template materialise and the
  notifier's template sweep (`auto_commit_leg_to_train` only binds a
  leg, so it refreshes revision 1's leg row, see next point). Revision 1
  is the compensation baseline until a user-fault break moves it.
- **Planning edits refresh revision 1** while the journey is *not
  started* and has *no break*: Change train, add leg, insert, remove,
  untracked edit and a template auto-match rewrite revision 1's legs in
  the same transaction. This matches how people book: they adjust a plan
  until they travel.
- **After the first leg starts, or once any break exists,** every change
  to the plan is a replacement: a new revision (`reason = replace`,
  `auto_shift` or `revert`), never an edit of an older one. Change train
  used then counts as a replacement with `cause = user_choice`, unless a
  break is open, when it takes the break's cause.
- **Revision fates.** When a revision is superseded, its legs' `fate` is
  set once from the facts at that time (`travelled` for legs before the
  break, `missed` / `cancelled` / `part_cancelled` for the trigger leg,
  `not_used` for the rest). This is the only update ever made to a
  snapshot row, and only from `planned`.

### 4.4 How a replacement changes `journey_legs`

One transaction, `FOR UPDATE` on the journey (as `delete_leg` does):

1. Insert the new revision and its legs.
2. Keep legs before `break_position` unchanged (same ids, so
   `journey_leg_notification_state`, links and the current-leg logic stay
   put).
3. Delete the replaced legs and insert the new ones, renumbering
   `leg_order` with the two-step renumber `post_journey_leg ?before=`
   already uses (`owned_leg_order_at`).
4. For each replaced train leg: `release_leg_subscription(old,
   move_to)`, where `move_to` is the subscription of the first new train
   leg run by an operator the ticket covers, else the first new train
   leg. Tickets move as DS-93 does; their ids are already recorded in the
   snapshot. With no new train leg, `move_to = None` and the tickets go
   standalone (nullable `tracked_train_id`) rather than being deleted:
   **a replacement must never cascade a ticket away**. This is a change
   to the DS-93 path, gated on a `keep_tickets` flag so Remove leg and
   Delete journey keep today's behaviour.
5. Resolve the open break (`status = accepted`, `accepted_revision_id`).

The new legs are created through the same helpers as `POST
/Journeys/itinerary` (`create_journey_from_itinerary`'s leg loop), so
validation, caps, horizon checks and untracked transfers
(`transferAsUntracked`) are shared.

### 4.5 Relation to DS-93 and DS-124

- **DS-93**: unchanged for Change train, Remove leg and Delete journey,
  except the snapshot written first and the `keep_tickets` flag in
  §4.4. The released train stops pushing, which is what we want: the
  user is not on it any more.
- **DS-124 (one-leg journeys)**: every tracked train becomes a one-leg
  journey, so every tracked train gets breaks and alternatives for free.
  A one-leg journey's replacement may have several legs; it is then a
  multi-leg journey, and "Stop tracking" on the old train (now released)
  is moot. DS-124's conversion migration must also write revision 1 for
  each converted journey (or its rows are caught by §10.2's backfill,
  which runs after it).
- **Group sharing**: shares are moving from `group_trains` to
  `group_journeys` (DS-124 §9 Q5, and the user's direction). This design
  keys everything on the journey, so a shared journey's members see the
  current plan, the open break (read-only) and the history through
  `group_journeys`, never through `group_trains`. A replacement keeps
  `group_journeys` rows (the journey id never changes) and moves
  `group_trains` rows as DS-93 does, for as long as they exist.

## 5. Feasibility engine

### 5.1 One module

New `crates/api/src/data/journey_feasibility.rs`, pure, built on
`trip_plan_live`'s public layers:

- Input: the journey's legs (train legs as `(uid, service_date, from
  CRS, to CRS)`, untracked legs as planned times and mode), a
  `HashMap<uid, TrainLive>` from `fetch_train_lives`, the train chains
  from the cached `PlanningGraph`, and `InterchangeData`.
- For each train leg: `evaluate_chain` gives live departure at the
  boarding stop and live arrival at the alighting stop, or `served =
  false` (cancelled, cut short, skipped).
- For each change between consecutive legs: `ready = live_arrival +
  change`, where `change` is:
  - same station: `minimum_change_time` at the boarding TIPLOC
    (`ChangeTime::NoInterchange` means the change is impossible, unlike
    `trip_plan_live::change_minutes`, which maps it to 0 because the
    planner never proposed one);
  - different stations: the fixed link from `fixed_links_from`, which now
    works both ways (DS-81) and honours its time window and day mask,
    plus the boarding station's MSN as the planners charge it;
  - an untracked leg in between: its planned duration
    (`planned_arrival - planned_departure`) moved to start at the
    previous ready time, then the next station's MSN.
- Output per connection: `Works`, `AtRisk { slack_minutes }` (slack
  under `AT_RISK_SLACK_MINUTES`, default 3, after MSN) or `Broken {
  cause }`, plus per leg `LegLive` and the first broken position.

`trip_plan_live::annotate`'s interchange rule then calls the same helper,
so the planner and saved journeys can never disagree. Live data uses the
same `LiveConfig` horizon and freshness rules. Outside the horizon a leg
is judged on the timetable only (it cannot break on live times yet,
except by a timetable cancellation).

### 5.2 Where it runs

| Caller | When | What |
|---|---|---|
| `GET /Journeys/{id}` | every read | adds `connections[]` and `break` (§7). Replaces the frontend's `connectionGapMinutes` rule, which ignored MSN and the next train's own delay. |
| `GET /Journeys/mine` | list | `connection` stops being `null`: computed only for journeys in progress today (bounded by `LiveConfig.max_trains`). |
| Break sweep (api loop) | every 60 s | today's and yesterday's journeys with a leg inside the live window and not travelled (§5.3). |
| `POST .../missed` | on tap | the user's own break, evaluated at once. |
| Notifier | never | reads `journey_breaks` only (D-4). |

The notifier does not get its own copy. It holds no planning graph (about
100 MB per date), and duplicating the overlay there would let the two
drift apart. The api already runs background loops
(`api_loop_runner`, `ds_store::loops::LoopRunner`, behind
`API_BACKGROUND_LOOPS`) and takes `pg_try_advisory_lock` per loop
(`ds_store::loops::runner`), so with several api replicas only one runs
the sweep.

### 5.3 The break sweep

Each tick:

1. Pick candidate journeys with one query: a leg with `service_date` in
   (today, yesterday), not travelled (`travel_order`'s rule), and a
   train leg departing within `horizon_minutes` or already en route.
   Production has 29 journeys in total, so this is tiny; the query is
   bounded by `journeys_user_id_created_at` and the leg date.
2. Batch-read every train's `TrainLive` once (`fetch_train_lives` takes
   a list of UIDs).
3. Evaluate each journey (§5.1). For a newly broken one, or one whose
   fingerprint changed (§5.4), upsert the open break and plan
   suggestions (§5.6). For an open break that now works again (the train
   recovered), set `status = recovered`.
4. Expire open breaks whose journey is travelled or past midnight plus
   the overnight allowance (§11).

The sweep is cheap: evaluation is pure; only breaks plan, and planning
reuses the graph cache. Its own planning goes through a dedicated
`SWEEP_PLAN_SLOTS` semaphore of 1, so user requests in `PLAN_SLOTS` are
never starved. Tunables, chart `api.journeyBreaks.*`:
`JOURNEY_BREAKS_ENABLED` (kill switch), `JOURNEY_BREAKS_INTERVAL_SECS`
(60), `JOURNEY_BREAKS_MAX_JOURNEYS_PER_TICK` (200).

### 5.4 Fingerprint and staleness

A break's `fingerprint` is a hash of: the break position and cause, the
live departure and arrival minutes of every train leg from the break
point on, and the newest `observed_at` among them, rounded to the minute.
Suggestions are **stale** when:

- the fingerprint now differs, or
- `suggestions_at` is older than `JOURNEY_BREAKS_SUGGESTIONS_MAX_AGE_SECS`
  (120), or
- the first suggestion's first train is due to leave within 2 minutes of
  now.

On open, `GET /Journeys/{id}/breaks/current` returns the stored
suggestions with `stale: true` when stale, and the page immediately
calls `POST .../replan/preview` (§7). Stored suggestions render at once,
dimmed with "Updating", so the deep link is never a blank page.

### 5.5 Later legs

After a replacement (or on every sweep for an open break) the engine
re-evaluates every leg after the replaced part:

- **Untracked legs** (Tube, bus, walk) are shifted automatically: start
  at the new ready time, keep their duration. Written as an `auto_shift`
  revision, in the same transaction as the accept. No push.
- **Later train legs that still work** stay.
- **Later train legs that no longer work** are part of the same break.
  The suggestions already cover them, because a suggestion goes to the
  final destination (§5.6), and accepting one replaces everything from
  the break point to where it rejoins the plan.
- **A suggestion that rejoins** a later leg (arrives in time for it,
  with MSN) keeps that leg and its train. The ranking prefers these, so
  tickets and bookings for later legs survive when they can.

### 5.6 What is planned

From the break point:

- **Origin**: the station where the user will be. For a missed or
  cancelled first leg, its origin. For a missed connection, the
  previous leg's alighting station. For a part-cancelled train the user
  is on, the last station it serves before the cut (`TrainLive`'s
  `last_served_min`), when the user is aboard.
- **Depart after**: `max(now, live arrival there + MSN)`.
- **Two searches, both with `live=true`**, through the same
  `SegmentSearch` code `/Trips/plan` uses:
  1. *Rejoin*: to each later leg's boarding station, arrive by its live
     departure minus MSN, CSA `fastest`. Kept when found.
  2. *To the destination*: to the journey's final rail destination (the
     last train leg's destination, or the station an untracked last leg
     starts from), `results=options`, `maxChanges` = the original's
     changes plus 1, capped at `MAX_CHANGES_LIMIT`.
- **Ranking**: context-aware, by ticket, break cause and the user's
  preference, shown as a Recommended pick plus badged "best for X"
  picks (§17). At most 3 cards shown, up to 9 stored.
- **Overnight**: when nothing arrives before the last train, plan with
  `dayOffset=1` / `acrossMidnight` (`trip_plan_overnight.rs`) and say so.

## 6. Auto-suggest notifications

### 6.1 Flow

```mermaid
sequenceDiagram
    participant S as api break sweep
    participant DB as Postgres
    participant N as notifier
    participant U as phone
    participant W as web app
    S->>DB: upsert journey_breaks (open, severity, suggestions)
    N->>DB: poll open breaks to notify
    N->>U: push "Connection at York missed. 3 other ways to Edinburgh"
    N->>DB: notified_at, notified_severity, train_notification_state
    U->>W: tap: /journeys/42?break=7&src=push
    W->>DB: GET breaks/current (stamps opened_at)
    W->>W: stale? POST replan/preview
    U->>W: "Take this one"
    W->>DB: POST replan/accept (new revision, break accepted)
```

### 6.2 Severity

| Severity | Meaning |
|---|---|
| 1 | Connection missed, and a suggestion arrives within 30 minutes of the plan |
| 2 | Connection missed with a longer delay, or a leg part-cancelled |
| 3 | A leg cancelled |
| 4 | No way to the destination today |

"At risk" (slack below `AT_RISK_SLACK_MINUTES` but still feasible) is
shown in-app only and never pushed (§14 Q2).

### 6.3 Decision rule (notifier)

A new notifier cycle, `journey_breaks`, every 15 s, reads open breaks
with suggestions ready and `notified_severity IS NULL OR severity >
notified_severity`. New pure function in `decision.rs`:

```
decide_break_notice(break, prefs, journey_muted, train_state_for_trigger)
  -> Send | SuppressOptOut | SuppressMuted | SuppressNotWorse | SuppressActed
```

- **One per break.** Sends when nothing was sent for this break.
- **Only if worse.** Re-sends only when `severity > notified_severity`.
  A new break on the same journey (after the user accepted, the new plan
  breaks too) is a new row and gets its own notice.
- **Quiet once acted.** Any `status` other than `open` stops it; so does
  `opened_at` set within the last 10 minutes at the same severity.
- **One push per user per train.** The trigger train's plain delay or
  cancel push and the break notice are one push:
  - The break notice writes the user's `train_notification_state` row
    for the trigger subscription at the equivalent rank (cancelled 2,
    late 1), so `decide_train_notification` sees no escalation and stays
    silent.
  - The train path, before sending, checks for an open break on a
    journey whose leg uses that subscription; if one exists and is due
    within the next cycle, it defers to it (the break notice says more).
  - In `push_queue.rs` a new `Target::JourneyBreak(journey_id)`
    supersedes a queued `Target::Train(id)` for the same user and
    trigger train.
  - The notification `tag` is `train-{tracked_train_id}` for a
    train-triggered break, so the phone collapses a plain push that
    raced it; `journey-break-{journey_id}` otherwise.
- **Opt-out.** `notification_preferences.journey_suggestions = FALSE` or
  `journeys.suggestions_muted` suppresses the push only. The in-app
  banner still shows, and the plain train push then goes out as today.
- **Owner only.** Group members get no break notice (§14 Q8).

### 6.4 Payload and deep link

```json
{
  "title": "Connection at York missed",
  "body": "3 other ways to Edinburgh. First arrives 11:12, 38 min later.",
  "url": "/journeys/42?break=7&src=push#break",
  "tag": "train-118"
}
```

Copy follows `docs/style-guide.md` (no arrows, ellipses or em dashes,
sentence case, departure-board terse). Titles by cause: "08:00 to York
cancelled", "Connection at York missed", "You missed the 08:00 to York"
(never pushed, the user is already in the app), "08:00 to York no longer
stops at Doncaster". With no suggestion: "No more trains to Edinburgh
tonight".

`sw.js` needs no change for the payload, and `notificationclick`
already opens the URL. One small change: the "skip when a focused tab is
on that URL" rule compares `new URL(client.url).pathname === url`, so a
`url` with a query or hash never matches and the push always shows. It
should compare against the payload URL's pathname, so a user already
looking at that journey sees the in-app banner instead of a push (the
break is then stamped `opened_via = in_app` by the page). Action buttons ("Take the first one") are left out: one-tap accept
from the lock screen would accept without seeing the change (§14 Q9).

### 6.5 Opt-out surfaces

- `/account/preferences`: a switch "Suggest other trains when a journey
  breaks" under the existing `NotificationsToggle`, through new `GET`/`PUT
  /preferences/notifications` routes in `routes/preferences.rs`.
- In the suggestions sheet: "Stop suggestions for this journey" (sets
  `journeys.suggestions_muted`).

### 6.6 Measured

See §16. Each break row carries `notified_at`, `opened_at` +
`opened_via`, `resolved_at` + `status`. `src=push` on the deep link is
how an open is attributed to the push.

## 7. API (all additive)

All routes are cookie-only (`AuthenticatedUser`), owner-only (as
`get_owned_leg`), and logged in `docs/api-changelog.md`.

| Route | Purpose |
|---|---|
| `GET /Journeys/{id}` | Gains `connections: [{fromLegId, toLegId, state: works\|atRisk\|broken, slackMinutes, changeMinutes, cause}]`, `openBreak: {id, cause, fault, severity, suggestionsReady, stale} \| null`, `revisionCount`, and per leg `replacesLegIds: number[]` (from the latest revision). Delegated readers see these too. |
| `GET /Journeys/mine` | `connection` filled for today's journeys; gains `openBreakId`. |
| `GET /Journeys/{id}/legs/{legId}/alternatives?cause=` | The light picker for one leg: rejoin options and to-destination options, live, from that leg. Works on **every** train leg, whatever `window_searched` is. `cause` is `missed`, `cancelled`, `connection` or `choice`. |
| ~~`POST /Journeys/{id}/legs/{legId}/missed`~~ | Removed (2026-10-23). "I missed this train" only looks, with `alternatives?cause=missed` (fault from §4.2), and taking one (`replace`, `cause: missed`) records the miss as its own revision. A break the old route opened just by loading the sheet stayed open after it closed and showed "You missed the ..." for a train that left on time with the traveller on it. |
| `GET /Journeys/{id}/breaks/current?src=push` | The open break with stored suggestions and `stale`. Stamps `opened_at` once. 404 with none. |
| `POST /Journeys/{id}/replan/preview` | `{breakId?, fromLegId, departAfter?}`. Recomputes suggestions live and stores them on the break. Rate-limited like `/Trips/plan`. |
| `POST /Journeys/{id}/replan/accept` | `{breakId, suggestionId, fingerprint}` with `Idempotency-Key` (reusing `journey_idempotency_keys`). Writes the revision (§4.4). `409 suggestion_stale` with a fresh preview when the chosen option no longer works on live times. |
| `POST /Journeys/{id}/legs/{legId}/replace` | The general form: `{cause, legs: [itinerary legs, as POST /Journeys/itinerary], throughLegId?}`. What the alternatives picker and Change train after travel start both call. |
| `POST /Journeys/{id}/breaks/{breakId}/dismiss` | `{reason: willMakeIt \| notTravelling \| other}`. |
| `GET /Journeys/{id}/revisions` | Every revision with legs and fates, newest first. |
| `POST /Journeys/{id}/revisions/{no}/revert` | Back to an earlier plan while its remaining trains still work. Written as a new revision. |
| `GET /Journeys/{id}/delay-repay` | Journey-level estimate (§9). |
| `GET`/`PUT /preferences/notifications` | `{journeySuggestions: boolean}`. |
| `PUT /Journeys/{id}/suggestions-muted` | `{muted: boolean}`. |

`POST .../train` (Change train) keeps its shape. After the journey
starts, or with a break open, it writes a revision first (D-2).

## 8. UX

Mobile-first, following `docs/style-guide.md`: grape for "you need to
act", blue only for changed arrangements, red for cancelled, 44 px tap
targets, a bottom `Drawer` below `sm` and a `Modal` above it, Mantine
`sm` buttons, no hand-rolled confirms.

### 8.1 Home, Current journey card

`CurrentJourneyCard` (`frontend/app/(home)/HomeJourneys.tsx`) gains a
grape light `Alert` inside the card when `openBreakId` is set:

> **Connection at York missed**
> 3 other ways to Edinburgh. First arrives 11:12.
> [See options]

"See options" opens the suggestions sheet in place. The card's badge
comes from the whole-journey summary including the server's connection
state, fixing `homeJourneyStatusGroup` (§1.2).

### 8.2 Journey page

- A banner above the legs, same copy, when a break is open.
- `LegConnector` shows the server's connection state with the MSN:
  "Change at York, 4 min to change (needs 5)." in orange or red, with a
  "Find another way" text button.
- Leg cards (`JourneyLegCard.tsx`), owner only:
  - **"I missed this train"** on the current or next train leg, before
    it is travelled. One tap, no confirm (nothing is written), opens
    the sheet, titled "Other ways to {destination}" (never "You missed
    the ..." while the traveller only looks). Only taking a train there
    records the leg as missed;
    closing the sheet leaves no break and no banner. DS has no other
    evidence of a missed first leg: its departure time passing is not one.
  - **"Choose another train"** on every train leg, not just
    `windowSearched` ones. It opens the same sheet with `cause=choice`.
    Before travel this is today's Change train, without needing a
    window.
  - Cancelled legs show "Find another way" in the leg's red status row.
- **Earlier plans**: a collapsed section at the end, "Earlier plans (2)".
  Each revision lists its legs with fate badges: "Booked" (blue),
  "Cancelled" (red), "Missed" (orange), "Not used" (gray), and "Go back to
  this plan" while it still works.

### 8.3 Suggestions sheet

```
Connection at York missed
Your 09:52 to Edinburgh left before you arrived.

[ 10:04 York to Edinburgh   arrives 12:31  +12 min ]
  Direct. Same operator.            [Take this one]
[ 10:15 York to Newcastle, change ... arrives 12:58 ]
                                      [Take this one]
More options

Your booked plan stays saved for Delay Repay.
Stop suggestions for this journey
```

- Rows reuse `ItineraryOption`'s layout and `DelayBadge`, with the delay
  measured against the booked arrival ("+12 min" orange, red at 60).
- "Take this one" is filled grape and is the whole accept: no confirm.
  After it, a green `Alert` "Journey updated. Booked plan saved." with
  "Undo" (revert, §7) for 30 seconds.
- Stale suggestions show at once, dimmed, with "Updating" and a spinner,
  then refresh in place.
- Errors: "Couldn't load other trains." / "Couldn't update the
  journey."; a `409 suggestion_stale` swaps in the fresh list with "That
  train no longer works. Here are the latest options."
- Under the list, the ticket advice (the set's `hint`, from the same
  ticket constraint and entitlement as the ranking and fit badges,
  §17.3). Station staff have the last word on a ticket, and must help
  finish a journey the railway disrupted (`NRCoT` 9.4, 28.3), so unless
  the ticket is known to be flexible it starts with them:
  - railway-caused (a late inbound, cancellation, part-cancellation):
    "Ask station staff. Your ticket should be accepted on the next
    suitable train because your journey was disrupted. Check before you
    board.";
  - traveller-caused, Advance: "Ask station staff if there are any. Your
    Advance ticket is only valid on the train you missed, so you'll
    likely need a new one." (a free choice: "... only valid on your
    booked train, ...");
  - traveller-caused, unknown ticket: "Ask station staff if there are
    any. If you have an Advance ticket, you'll likely need a new one.
    Check before you board.";
  - traveller-caused, operator-only: "Ask station staff if there are
    any. Your ticket may not be valid on a different train. Check before
    you board.";
  - flexible (any permitted, Off-Peak, Anytime, a season), any cause:
    "Your ticket is usually valid on other trains on the same route."

  Then "Your booked plan stays saved for Delay Repay."

## 9. Delay Repay

### 9.1 What is compared

For a journey with at least one revision after the booked one:

- **Booked arrival**: the compensation baseline revision's last train
  leg's `booked_arrival` (public time) at its `destination_crs`.
- **Actual arrival**: the current plan's last train leg's arrival at the
  same CRS, from the existing outcome query (`delay_repay_outcome.rs`,
  one `OutcomeTarget` for that train and CRS) and `stop_delay` on public
  times, so it is provisional until TRUST reports it, as today.
- **Delay** = actual minus booked, fed to `estimate_delay_repay` with
  the scheme from the baseline leg's ticket (`scheme_for(ticket.operator,
  atoc_code)`) and `ticket_kind`.
- A different final station (a re-route to another London terminal, for
  example) gets no percentage, with copy saying why (§14 Q6).

### 9.2 Fault moves the baseline

Revisions are walked in order:

- An operator-fault revision (`cancelled`, `part_cancelled`,
  `connection_missed`, `skipped_stop`) leaves the baseline where it is.
  The user is owed against the booked arrival.
- A user-fault revision (`user_missed` with the inbound in time) or a
  `user_choice` revision moves `is_compensation_baseline` to itself: from
  then on the user is measured against the plan they chose. Delays to
  that new plan still count.
- Fault for `user_missed` is decided from the data (§4.2): if the
  inbound leg arrived after the missed train's live departure minus MSN,
  it is operator fault whatever the user tapped.

### 9.3 Surfaces

- `GET /Journeys/{id}/delay-repay`: `{bookedArrival, actualArrival,
  delayMinutes, provisional, baselineRevision, faultNote, estimate:
  DelayRepayFields}`.
- `DelayRepayEstimate` on a ticket whose train's outcome is
  `notReached` and whose journey has a replacement now says "Measured on
  your whole journey" and shows the journey estimate, which is the case
  `delay_repay_rules.rs` leaves open today.
- The journey page shows it under Earlier plans; "Your reliability"
  (`computeDelayRepayRollup`) counts it once per journey, not per train.

Disclaimers stay as they are: estimates only, never a claim.

## 10. Migrations and backfill

Forward-only, `YYYYMMDDHHMMSS_name.sql` in `crates/ds-store/migrations/`,
each starting `SET LOCAL lock_timeout = '5s';`, merged files immutable
(`crates/api/tests/migration_checksums.rs` and its lock file), versions
newer than every existing one.

### 10.1 Files (Phase 1)

| File | Content | Locks |
|---|---|---|
| `20261023090000_journey_revisions.sql` | `journey_revisions`, `journey_revision_legs`, their indexes. | New tables; the FK takes a brief `SHARE ROW EXCLUSIVE` on `journeys`. |
| `20261023090100_journey_breaks.sql` | `journey_breaks`, its partial indexes, `journey_revisions.break_id` FK. | New table; brief locks on `journeys`, `users`, `journey_legs`. |
| `20261023090200_notification_preferences.sql` | the table; `journeys.suggestions_muted` (constant default, catalog only). | brief `ACCESS EXCLUSIVE` on `journeys` for the column, under the 5 s `lock_timeout`. |
| `20261023090300_journey_revisions_checks.sql` | enum CHECKs `NOT VALID`. | none |
| `20261023090400_journey_revisions_checks_validate.sql` | `VALIDATE CONSTRAINT`. | `SHARE UPDATE EXCLUSIVE` only |
| `20261023090500_journey_revisions_backfill.sql` | §10.2 | row locks on new tables only |

No index on an existing table is needed. If one becomes necessary (for
example on `journey_legs (service_date)` for the sweep), it goes alone in
a `-- no-transaction` file with `CREATE INDEX CONCURRENTLY IF NOT
EXISTS`, as `crates/api/tests/migration_index_locking.rs` enforces.
Grants for the new tables go in `charts/distant-signal/files/db-grants.yaml` (api read/write,
notifier read on `journey_breaks` and `notification_preferences` plus
update of the notify columns), checked by `gen-db-grants.py check`.

### 10.2 Backfill

Revision 1 for every journey that has no revision and has a leg with
`service_date >= CURRENT_DATE - 1` (today, upcoming and overnight).
Past journeys get none: their history is gone already, and retention
removes them. Idempotent (`WHERE NOT EXISTS`), batched 500 at a time in
a loop, the same shape as DS-124 §5.1's conversion, taking ids from the
sequence first. Booked times come from `schedule_calling_points_full`
public times for the leg's train at its CRS; legs with no bound train
get `NULL` times and are refreshed on first bind (§4.3). Production has
29 journeys, so this is one batch. A database too large for one
transaction would move it to a `ds-migrate` subcommand, as
`backfill-trains` did.

Journeys created by old pods during the rollout have no revision 1.
Code treats "no revision" as "revision 1 is the current plan" and writes
it lazily before any replacement, so the window is safe. Phase 2 ships
the same backfill again as a data-only catch-up.

### 10.3 Rollback

Code rollback: the old image ignores the new tables. Revisions written
meanwhile stay and are used again on roll-forward. The only behaviour an
old image loses is the `keep_tickets` flag. There is no contract step:
nothing existing is dropped.

## 11. Edge cases

| Case | Behaviour |
|---|---|
| **Unbound leg** (no train yet) | Never breaks on live data; judged on its window only. The picker offers its candidates as today. |
| **Untracked legs** (Tube, bus, walk) | Never the trigger. Their planned duration counts in the change (§5.1); they shift automatically (§5.5). A break *before* a Tube leg plans to the Tube leg's start station and keeps it. TfL disruption is out of scope. |
| **Travelled legs** | Frozen: never replaced, never offered "I missed this train". The break position is always after the last travelled leg. |
| **User aboard a part-cancelled train** | Planned from its last served stop; `fate = part_cancelled`. |
| **Overnight** | Service dates today and yesterday are swept, as the overlay already reads. Suggestions may cross midnight (§5.6). An open break expires at 04:00 London time after the journey's last service date. |
| **Tickets** | Recorded in each snapshot (`ticket_ids`), moved to the new train (§4.4), never cascaded away. Advance tickets are train-specific: on an operator-fault break the copy says the ticket is usually valid on the next reasonable train; on a user-fault break it warns (§8.3). Ticket fit per alternative is §17.3. DS never says a ticket *is* valid. |
| **Groups** | Members see the break banner and history read-only on the shared journey; no accept, no push. The owner's accept is what they see next. |
| **Shared links** (`unlisted_links`) | Show the current plan; history hidden by default (§14 Q10). |
| **Templates** | A replacement applies to that day's journey only; the template is untouched. Template materialise writes revision 1. |
| **Two legs on one subscription** | Replacing one leg releases the subscription only if no other leg uses it (DS-93 rule, unchanged). |
| **Pin legs not yet resolved** | No live data, so no break; "I missed this train" still works and plans from the pin's origin and time. |
| **Delegated / DS-MCP** | `GET /Journeys/{id}` shows the additive fields, so `get_my_journey` can report "connection at York missed". DS-MCP cannot accept or replace (writes beyond `POST /Journeys/itinerary` are never delegated). A journey created by DS-MCP gets breaks and pushes like any other. |
| **Account export and retention** | Revisions and breaks are exported with the journey and cascade with it; retention needs no change (journey FK cascades). |
| **Clock skew / stale data** | TRUST facts older than `trust_max_age_minutes` are ignored, as in the overlay; a break needs two consecutive sweeps (about a minute) before it is notified, unless the cause is a cancellation. |

## 12. Phasing

| Phase | Content | Why first |
|---|---|---|
| **0. Fixes** (small, independent) | Fix one-leg push links (`/track/{id}` 404, link to `/journeys/{id}` or `/train/by-id/{id}`). Offer Change train on every train leg (derive a window from the leg's booked times when `window_searched` is false). Server-side `connections[]` with MSN on `GET /Journeys/{id}`; the frontend uses it instead of `connectionGapMinutes`; the home card shows connection state. | Each is useful alone and de-risks the engine. |
| **1. Keep the plan, choose an alternative** | Migrations §10.1, backfill §10.2. Revision 1 on every create path. `journey_feasibility` module. Alternatives, missed, replace, accept, revisions, revert routes. Suggestions sheet, "I missed this train", "Choose another train", Earlier plans. In-app banner on open breaks (computed on read, no sweep yet). | Accept must keep the booked plan before any push can offer it. |
| **2. Auto-suggest notifications** (headline) | Break sweep in the api, `journey_breaks` lifecycle, stored suggestions and staleness, notifier `journey_breaks` cycle with `decide_break_notice`, one-push dedupe with the train path, deep link with `src=push`, opt-out switch and per-journey mute, metrics §16. Catch-up backfill. Behind `JOURNEY_BREAKS_ENABLED`, on for the owner first, then everyone. | The headline feature, as soon as the data model is safe. |
| **3. Later legs** | Automatic untracked-leg shifts, rejoin-first ranking, re-evaluation after accept. | Builds on 1 and 2. |
| **4. Journey Delay Repay** | §9 route and surfaces, fault-moved baseline, `notReached` follow-through, reliability rollup per journey. | Needs a few weeks of real revisions to check against. |
| **5. Later** | "At risk" pushes using DS-123's `predictedStart`; DS-MCP read of alternatives (new read-only scope); group-member notices if wanted. | Each needs its own decision (§14). |

Phases 1 and 2 are meant to ship within the same week; Phase 2 is the
first one users notice.

## 13. Risks

- **False breaks** from stale or missing live data would push wrongly.
  Two-sweep confirmation, the overlay's freshness rules, and severity
  thresholds limit it; `dismiss` reasons are counted to spot it.
- **Push fatigue**: one per break, worse-only, and the dedupe with train
  pushes means a broken journey never gets more pushes than today's
  train pushes, at most one more on a new break.
- **Planner load**: the sweep plans only on breaks, through one slot.
  Production has 3 users and 29 journeys; at 1,000 active journeys and a
  5 % break rate, that is about 50 RAPTOR runs spread over the sweep
  interval.
- **Wrong fault** would mislead Delay Repay. Fault comes from data, with
  the reason shown; the estimate keeps its disclaimers.
- **Ticket moves** could attach a ticket to the wrong new train when a
  replacement has several operators; the snapshot keeps the original
  link and the ticket can be re-attached as today.
- **DS-124 overlap**: its conversion writes journeys; our backfill must
  run after it or its rows rely on the lazy revision 1. Both are
  idempotent.

## 14. Open questions for the user

1. **Never auto-swap trains?** Recommended: yes, never. Trains change
   only on the user's tap; untracked legs shift automatically.
2. **Push on "at risk"?** Recommended: no in Phase 2 (in-app only).
   Revisit once DS-123's prediction gives a confidence.
3. **Break notice replaces the train's plain delay or cancel push** for
   the same train. Recommended: yes (D-5).
4. **Opt-out granularity.** Recommended: a global switch plus a
   per-journey mute; no quiet hours yet.
5. **"I missed this train" when the data says the inbound was late.**
   Recommended: the data decides fault (operator), and the sheet says so.
6. **Delay Repay when the replacement ends at a different station.**
   Recommended: no percentage, explain why. Treating a station group
   (London terminals) as one is a later step.
7. **DS-MCP.** Recommended: additive read fields now; a read-only
   alternatives tool behind a new scope later; never delegated accept.
8. **Group members' notices.** Recommended: owner only.
9. **Lock-screen action button** ("Take the first one"). Recommended:
   no; the user should see the change. Revisit with data from §16.
10. **History on shared links.** Recommended: hidden; group members see
    it.
11. **Change train before travel**: refreshes the booked snapshot
    (§4.3). Recommended: yes, until the journey starts or breaks.
12. **Revert window.** Recommended: allowed while the earlier plan's
    remaining trains still work; Undo shown for 30 s after accept.
13. **Sweep in the api, not the notifier.** Recommended: yes (D-4).

### Decisions (2026-10-22)

The user accepted every recommendation above: trains are never swapped
without a tap (1); "at risk" stays in-app only (2); a break notice
replaces that train's own push (3); a global opt-out plus a per-journey
mute, no quiet hours yet (4); the data decides fault when the user marks
a train missed (5); and 6 to 13 as recommended.

Also decided: phase 0 starts now and phases 1 and later after the
one-leg-journeys change (DS-124) lands; suggestions warn "check your
ticket" when an alternative may not be valid on it (a hint only, no fares
data); the push states what happened and the next option ("You'll miss
the 10:16 to Charing Cross. Next: 10:31, arrive 10:52. Tap to switch.").
How alternatives are ranked is still open: the user wants it to depend
on context (ticket validity, speed, changes), pending research.

## 15. Tests

- **Pure engine** (`journey_feasibility`): MSN at a station, `NoInterchange`,
  fixed link in its listed and reverse direction (DS-81), its time
  window, an untracked leg between trains, the next train itself late
  (still works), cancellation, part cancellation (`EN ROUTE`), change of
  origin, overnight across midnight, outside the horizon. A parity test
  with `trip_plan_live::annotate` on the same inputs.
- **Decision** (`decide_break_notice`): the dedupe matrix: first notice,
  same severity, worse, acted, opened recently, opt-out, mute, train
  push already sent, train push racing.
- **Notifier DB tests**: `candidates_for_trains_id_notifies_a_user_once_per_train`
  extended with a break on the same train; queue supersede.
- **DB tests**: each migration, rerun of the backfill, the CHECKs; replace
  transaction (legs before the break keep ids, renumbering, ticket
  move, no ticket cascade, revision immutability, idempotency key); the
  booked snapshot refreshed before travel and frozen after; DS-93 tests
  still pass; under `scripts/test-postgres-roles.py`.
- **Routes**: ownership, delegated refusal, 409 stale, 409 travelled,
  `src=push` stamping once.
- **Delay Repay**: baseline walk over mixed operator and user faults;
  different-station refusal; `notReached` follow-through.
- **Frontend**: vitest for the sheet states and copy (`scripts/check-copy.py`),
  `homeJourneys` and `journeyStatus` with server connections; Playwright
  from `frontend/e2e/journey-edit-insert.spec.ts`'s API-created journeys
  and `frontend/test/journeyFixtures.ts`: miss, accept, history, undo,
  deep link; visual baselines for the banner and sheet at phone width.
- **Checks**: `cargo fmt`, `clippy -D warnings`, `check-migration-order.py`,
  `gen-db-grants.py check`, migration lock, frontend lint, `tsc`, build,
  the API changelog entry.

## 16. Metrics

Prometheus, through `common::metrics::metric_name`:

| Metric | Labels |
|---|---|
| `api_journey_breaks_detected_total` | `cause`, `fault` |
| `api_journey_breaks_resolved_total` | `status` (accepted, dismissed, recovered, expired) |
| `api_journey_break_suggestions_seconds` | histogram, `trigger` (sweep, missed, preview) |
| `api_journey_break_suggestions_total` | `result` (found, none, error) |
| `api_journey_break_opened_total` | `via` (push, in_app) |
| `api_journey_replan_accepted_total` | `via`, `rank` (1, 2, 3, more) |
| `api_journey_replan_stale_total` | |
| `api_journey_breaks_sweep_seconds` | histogram |
| `notifier_journey_break_notice_total` | `result` (sent, opt_out, muted, not_worse, acted, failed) |

The funnel (sent, opened, accepted, time to accept, dismiss reasons) is
read from `journey_breaks` timestamps with one documented SQL query in
the runbook, aggregates only, no per-user data leaves the database.
Success for Phase 2: at least half of notified breaks opened, and at
least a third of opened breaks accepted, over the first month.

## 17. Ranking alternatives

Status: decided 2026-10-22 (see the end of §17.10).
Work log DS-129. Replaces the single rule in §5.6 ("rejoins first, then
arrival, then changes, then same operator"), which the user rejected:
which alternative is best depends on the ticket, on why the journey
broke and on what the traveller cares about. This section sets out the
rules that decide it (§17.1), what DS knows today (§17.2), a ranking
model (§17.3 to §17.6), the data it needs (§17.7), phasing (§17.8) and
the decisions needed (§17.10). Sources are in §17.11; condition numbers
are from the National Rail Conditions of Travel (NRCoT) in force from 1
April 2026.

### 17.1 What decides it in GB rail

A ticket restricts travel along five independent axes. An alternative is
"on your ticket" only if it passes all five, and disruption rules relax
some of them in defined cases.

| Axis | Where it comes from | Examples |
|---|---|---|
| **Train** | Train-specific fares (NRCoT 12.1; the Advance terms: "valid ONLY on the date and train service(s) shown") | Advance |
| **Operator** | The route printed on the ticket naming a train company (NRCoT 12.1, 13.1); in the data, a route whose route-data records list TOCs that must (`T`) or must not (`X`) be used (RSPS5047 §6.12.3) | "LNER ONLY", "NOT AVANTI" |
| **Route** | The route printed on the ticket (NRCoT 13.1). With no route shown: any direct train, or a route the National Routeing Guide permits (13.1.1, 13.1.2). Route data: stations that must all (`A`) or any (`I`) be passed, or must not be (`E`), and required or barred modes (`L`, `N`) | "ANY PERMITTED", "VIA BIRMINGHAM", "NOT VIA LONDON" |
| **Time** | Off-Peak and Super Off-Peak time restrictions (restriction codes in the fares data) | Wrong-time travel is charged the difference to the cheapest valid fare (9.5.1), but 9.5.1 does not cover Advance |
| **Date and break** | Validity dates (11.1; a ticket expiring during a delayed journey stays valid, 11.2); break of journey (16; Advance does not allow it) | |

What the rules allow when things go wrong:

| Situation | Train-specific (Advance) | Operator-restricted ("X only") | Any permitted (Off-Peak, Anytime) | Season |
|---|---|---|---|---|
| **A connection missed because an earlier train was late or cancelled** | The next train of the operator(s) the ticket was valid on, without penalty (9.4). The Advance terms say "the next available train(s) to complete your journey". The reservation does not carry over. | Next train permitted by the ticket, so that operator's (the "Changing trains" guidance: "board the next train to your destination, as permitted by your ticket") | Next train by any permitted route and operator | As any permitted, within the season's stations and route |
| **The booked train cancelled** | Up to two trains before or after on the same operator (28.3). Under the industry ticket acceptance policy, also any participating operator from 2 hours before to 2 hours after the cancelled train, for cancellations only, with a ticket bought before it was due to leave. | As Advance's 28.3 and acceptance rules, plus any later train of that operator | Any permitted train | Any permitted train |
| **Disruption stops the journey** | Any train company will, "where it reasonably can", provide alternative means of travel (28.2). Declared ticket acceptance (other operators, the Underground, buses, sometimes lifting Advance restrictions) is published per incident on the National Rail disruption pages. | Same | Same | Same |
| **Delay of more than 60 minutes expected** | May travel on a later date with comparable restrictions (28.4) | Same | Same | Same |
| **The passenger misses the train themselves** | "If you miss the first train on which you are booked for any reason, a new ticket must be purchased" (Advance terms); off the booked itinerary "you will be treated as if no Ticket were held unless the circumstances of clause 9.4 apply" (9.5.1) | Next permitted train of that operator, inside time restrictions | Next permitted train, inside time restrictions; wrong-time travel is charged the difference (9.5.1) | Any permitted train |

**Fault: railway or passenger, not operator or Network Rail.** None of
these rights depends on which part of the railway caused the delay.
9.4 applies to any delayed or cancelled connecting train; Delay Repay
is owed for a 60-minute delay "regardless of fault" (32.1). The
distinction that changes what a ticket allows is whether the
*passenger* caused the miss. That is exactly the `fault` this design
already records (`operator` meaning "the railway", or `user`, §4.2). So
the TRUST delay attribution codes are **not needed for ranking**, and
DS should not ingest them for this purpose. "Any reasonable route" is
wording from declared ticket acceptance notices, not a general right
that a fault code unlocks.

**Railcards** do not change which trains a ticket is valid on; they
change the price. They are not a ranking input.

### 17.2 What DS knows today

At `origin/main` e98f1c8a.

| Input | Today | Gap |
|---|---|---|
| **Ticket type** | `tracked_train_tickets.ticket_type`, free text (`crates/ds-store/migrations/20260829090000_journey_ticket_tracking.sql`). Extraction (`crates/api/src/data/ticket_extraction.rs`) is regex only: `.pkpass` `auxiliaryFields` "ticketType", PDF `TICKET_TYPE_KEYWORDS` ("Advance Single", "Off-Peak Day Return", "Anytime Day Single", "Season", ...). `delay_repay_rules::ticket_kind` reduces it to `Single`/`Return`/`Unknown`. | No structured fare class. Advance versus flexible is distinguished nowhere. |
| **Ticket operator** | `operator`, free text (`.pkpass` `organizationName`, or a retailer marker in PDFs), mapped by `scheme_for` / `rule_for_text`. | It is the retailer or issuing operator, not the route's operator restriction. |
| **Route, restriction code, reservation, class, validity dates** | Not captured. The migration's audit header lists the only allowed columns; it forbids price, barcode or ITSO data, passenger name and the file. | All missing. None of them is in a forbidden category. |
| **Booked train for an Advance** | Implied: a ticket hangs off the subscription it was attached to (`tracked_train_id`), and §4 records `ticket_ids` per revision leg. | Enough for "your booked train". |
| **Each leg's operator** | CIF `operator_atoc` on `schedule_destination_departures`, `schedule_services`; `data::train_operator` derives it per train; `PlannedLeg::Train.operator` is filled after a search by `trip_leg_details::attach_leg_details`. | `schedule_query::Connection` has no operator, so the planners cannot filter or prefer by operator. |
| **Planner options** | `GET /Trips/plan` (`TripPlanParams`, `crates/api/src/routes/trips.rs`): `via`, `avoid`, `avoidStop`, `avoidChange`, `waypoints`, `maxChanges` (default 2, limit 6), `results=fastest` (CSA) or `options` (RAPTOR Pareto set over arrival and changes, `build_pareto_set`). `trip_planner::Restrictions` covers stations only. | No operator filter, no transfer-mode filter. |
| **Transfers** | `PlannedLeg::Transfer { mode, minutes, line }`, modes `WALK`, `TUBE`, `METRO`, `BUS` from fixed links and `reference-data/supplementary-fixed-links.csv`. | Enough to count Tube and walk transfers and minutes. |
| **User settings** | Pins only (`pinned_lines`, `pinned_stations`, `pinned_operators`); appearance and push in the browser or `push_subscriptions`. | No travel preference of any kind. |
| **Fault and disruption** | §4.2's `cause` and `fault`. `train_reasons` holds TRUST 0002/0006 reason codes; `reference-data/delay-attribution-reasons.tsv` has a category column that no code reads. LDBWS `delayReason`/`cancelReason` text per stop. | Enough (§17.1). No ticket acceptance data. |
| **Crowding** | None. Darwin loading and formation come only through Push Port, not ingested (DS-126 is the subscription request). | Not available. |
| **Fares and routeing data** | None ingested. | §17.7. |

### 17.3 The model

Three pure steps, in a new `crates/api/src/data/alternative_ranking.rs`
(no I/O, unit-tested with tables):

**1. Ticket constraint.** From the tickets on the replaced legs (the
revision's `ticket_ids`):

```rust
enum TicketConstraint {
    TrainSpecific { operators: Vec<Atoc>, booked: Vec<TrainRef> }, // Advance
    OperatorOnly { operators: Vec<Atoc> },          // "X ONLY", T entries
    Route { via_all: Vec<Crs>, via_any: Vec<Crs>, not_via: Vec<Crs>,
            not_operators: Vec<Atoc> },             // A, I, E, X entries
    AnyPermitted { time_restricted: bool },         // Off-Peak, Anytime, season
    Unknown,                                        // no ticket, or unparsed
}
```

`operators` for an Advance comes from its route ("LNER ONLY") when known,
else the booked train's `operator_atoc`. Several tickets on one journey
(split tickets) give one constraint per covered segment; an alternative
is judged per segment.

**2. Entitlement.** From the break (§4.2):

| Entitlement | From |
|---|---|
| `MissedConnection` | `connection_missed`, `part_cancelled`, `skipped_stop`, and `user_missed` with a late inbound |
| `Cancelled { booked_departure }` | `cancelled` |
| `PassengerFault` | `user_missed` with the inbound in time |
| `Choice` | `user_choice` (no break) |
| `DeclaredAcceptance { operators, modes }` | later, §17.7 |

**3. Ticket fit per alternative.** `ticket_fit(constraint, entitlement,
itinerary) -> Fit`, applied to every train leg of the alternative:

| Fit | Meaning | Badge |
|---|---|---|
| `Covered` | The rules in §17.1 cover it (for example, 9.4's next train of the Advance's operator) | "Usually covered by your ticket" (gray) |
| `CoveredIfCancelled` | Only under the ticket acceptance policy's 2-hour window or 28.3's two trains | "Accepted when your train is cancelled" (gray) |
| `Check` | DS cannot tell (unknown ticket, or a route DS cannot check) | "Check your ticket" (yellow) |
| `LikelyNewTicket` | Outside every rule DS knows (another operator on an Advance with no cancellation; any train after a passenger-fault miss on an Advance; a station the route excludes) | "May need a new ticket" (yellow) |

DS never says a ticket *is* valid (§11), and never hides an option for
fit alone, unless the user chose "Only trains on my ticket" (§17.5).
Every fit badge explains itself on tap, citing the rule in plain words
("Your ticket lets you take the next LNER train when a connection is
missed.").

### 17.4 Signals and the Recommended pick

Per alternative, computed from the itinerary and its live overlay:

| Signal | Source |
|---|---|
| `fit` | §17.3 |
| `arrival` | live arrival at the final destination (`LegLive`) |
| `changes` | `changeCount` |
| `transfer_burden` | number of `TUBE`/`BUS`/`METRO` transfers and total `WALK` minutes |
| `rejoins` | keeps a later booked leg (§5.5) |
| `slack` | the tightest change's slack after MSN (§5.1); a 1-minute change is worth less than a 6-minute one |
| `same_operator` | every train leg run by the replaced leg's operator |
| `crowding` | reserved, not used until loading data exists |

**Recommended** is the lowest generalised cost, in minutes:

```
cost = arrival_minutes
     + change_weight   * changes            (default 10)
     + transfer_weight * tube_or_bus_transfers (default 10) + walk_minutes
     + slack_penalty   (10 when slack < AT_RISK_SLACK_MINUTES)
     + fit_penalty     (Covered 0, CoveredIfCancelled 0, Check 15,
                        LikelyNewTicket 90)
     - rejoin_bonus    (15)
```

A generalised cost rather than a lexicographic sort, because a strict
order is what produces "1 minute faster with two more changes" or "the
same operator two hours later". The weights are the preference profile
(§17.5); defaults are a starting point, tuned from which pick users
accept (§17.9).

### 17.5 Defaults per context

| Ticket | Break | Recommended | Other picks shown | Flagged |
|---|---|---|---|---|
| **Advance** | Missed connection (railway) | The next train of the ticket's operator to the destination, by the ticket's route (9.4); `fit_penalty` makes other operators lose unless the user prefers otherwise | "Fastest" when another operator arrives at least 15 minutes earlier | Other operators: "May need a new ticket" |
| **Advance** | Booked train cancelled | Earliest arrival among the same operator's trains up to two before or after (28.3) and participating operators within 2 hours either side (acceptance policy) | "Same operator" if Recommended is another operator | Outside the window: "May need a new ticket" |
| **Advance** | Passenger fault | Fastest, with the advice "Ask station staff if there are any. Your Advance ticket is only valid on the train you missed, so you'll likely need a new one." (§8.3) | "Fewest changes" | Every option `LikelyNewTicket`, so none is singled out |
| **Operator only** ("X ONLY") | Any railway break | Fastest on that operator | "Fastest" on any operator if 15 minutes or more earlier | Other operators: "May need a new ticket", unless declared acceptance |
| **Route restricted** ("VIA X", "NOT VIA Y") | Any railway break | Fastest whose calling points satisfy the route (checked on the itinerary's stations; the planner already takes `via` and `avoid`) | "Fastest" if it breaks the route | Route breakers: "May need a new ticket" |
| **Any permitted** (Off-Peak, Anytime) | Any railway break | Lowest cost; operator is irrelevant | "Fewest changes", "No Tube" when they differ | Nothing (DS cannot check the routeing guide, §17.7) |
| **Season** | Any | As any permitted, between the season's stations | As any permitted | A leg outside the season's stations: "Check your ticket" |
| **Unknown** (no ticket) | Railway break | Lowest cost with `same_operator` as a 5-minute bonus, so a same-operator train wins ties | Always a "Same operator" pick when it differs, so an Advance holder can find it | Nothing; the railway-caused advice, staff first (§8.3) |
| **Unknown** | Passenger fault | Lowest cost | "Same operator" | The advice "Ask station staff if there are any. If you have an Advance ticket, you'll likely need a new one. Check before you board." (§8.3) |
| **Any** | `user_choice` | The user's preference profile only | "Fastest", "Fewest changes" | As a passenger-initiated change: Advance shows "May need a new ticket" |

### 17.6 Presentation: picks with badges, not one list

The sheet in §8.3 shows **up to three cards**: Recommended first, then
the other picks for this context, each card carrying every pick it wins
(one itinerary can be "Fastest" and "Same operator" at once, then it is
one card with two badges). "More options" lists the rest by Recommended
cost. Pick badges are gray (informational); fit badges as in §17.3.

```
Connection at York missed
Your Advance ticket lets you take the next LNER train.

[ Recommended  Same operator  Usually covered by your ticket ]
  10:04 York to Edinburgh, direct   arrives 12:31  +12 min   [Take this one]
[ Fastest  May need a new ticket ]
  09:58 York to Edinburgh, direct   arrives 12:10            [Take this one]
[ Fewest changes ] ...
More options
```

The pick set:

| Pick | Rule | Shown when |
|---|---|---|
| Recommended | lowest cost (§17.4) | always |
| Fastest | earliest arrival | it is not Recommended |
| Fewest changes | fewest changes, then earliest arrival | fewer changes than Recommended |
| Same operator | earliest arrival with `same_operator` | it is not Recommended and the ticket is Advance, operator-only or unknown |
| No Tube | lowest cost with no `TUBE`/`BUS` transfer | Recommended has one |
| Keeps your later trains | the best rejoin (§5.5) | it is not Recommended |

At most three cards are shown, chosen in the order above. The push
(§6.4) names Recommended in context: "Next LNER train 10:04, arrives
12:31." The accept metric gains a `pick` label (§17.9).

### 17.7 Ticket data, preference and planner fit

**Ticket fields to capture** (new nullable columns on
`tracked_train_tickets`, each added to the audit header's allowed list
in the same migration):

| Field | Why | How |
|---|---|---|
| `fare_class`: `advance`, `off_peak`, `super_off_peak`, `anytime`, `season`, `other`, `unknown` | The constraint kind | Derived from `ticket_type` text on save, shown as a select the user confirms |
| `route_text` | The operator and route restriction, as printed ("ANY PERMITTED", "LNER ONLY", "NOT VIA LONDON") | Extraction where the pass or PDF has a route line; a free text field with common choices |
| `route_operators`, `route_not_operators`, `route_via`, `route_not_via` | Parsed constraint, so ranking needs no text parsing | Parsed from `route_text` by a small grammar (ONLY, NOT, VIA, NOT VIA, operator names via `rule_for_text`); later from the route data (below) |
| `restriction_code` | Off-Peak time restriction, shown as text in the fit explanation | Optional, as printed |

Not captured: railcard (not a ranking input), seat and coach, price,
booking reference.

**Preference setting.** One setting on `/account/preferences`, in a new
`travel_preferences (user_id, alternative_priority, ...)` row (or
alongside §4.1's `notification_preferences`, which is per user too):

> When suggesting other trains, put first: **Automatic, based on my
> ticket** (default) / Fastest / Fewest changes / Fewest Tube and bus
> transfers / Only trains on my ticket

Each choice is a weight profile for §17.4 ("Only trains on my ticket"
hides `LikelyNewTicket` options and keeps `Check` ones). The picks
already let the user override it per break without a setting.

**Planner fit and cost.** The `options` Pareto set (arrival, changes)
already contains Fastest and Fewest changes. It does **not** contain
"Same operator" or "No Tube" when those are slower without being
simpler, so they need constrained searches:

1. Add an operator id to `schedule_query::Connection` (a `u16` index into
   the TOC list, filled at graph build from `operator_atoc` as
   `attach_leg_details` already does per train), and an
   `allowed_operators` / `barred_operators` set to
   `trip_planner::Restrictions`, honoured by CSA. About 2 bytes per
   connection on a roughly 100 MB graph.
2. Add a `barred_transfer_modes` set for "No Tube", applied where fixed
   links are expanded.
3. Route-restricted tickets pass their `via` and `not_via` to the
   existing `via` and `avoid` parameters.

Per break: the existing RAPTOR options run (0.5 to 1.3 s) and rejoin
CSA, plus at most three constrained CSA runs (80 to 150 ms each, the
"two trains before or after" window being CSA with `departAfter` stepped
to the window start). About 0.5 s more per break, inside the sweep's own
`SWEEP_PLAN_SLOTS` (§5.3). Stored suggestions grow from 6 to up to 9
(the Pareto set plus the constrained picks), with the picks and fit in
the `suggestions` JSON.

**Feeds.**

| Data | Need | Effort |
|---|---|---|
| None new | Phases R0 to R2 (§17.8) use ticket text, `operator_atoc` and the planner | |
| Route data: the fares feed's route file (RSPS5045, `.RTE`: route codes and the descriptions printed on tickets) and the Routeing Guide feed's routes file (RSPS5047 §6.12: per route code, `A`/`I`/`E` stations, `T`/`X` operators, `L`/`N` modes) | Turn the printed route into a structured constraint without hand-parsing, for every route in the country | Small: two fixed-format files, a parser in `ds-migrate` or a reference-data job, one table, refreshed with each feed release. About 2 to 3 days. Available through the Rail Data Marketplace. |
| The full Routeing Guide (maps, routeing points, permitted routes, easements, station distances) | Only to warn that a re-route is not permitted on an Any Permitted ticket | Large: a permitted-route engine, weeks. During disruption 9.4 and 28.2 mostly cover re-routes anyway. **Not recommended.** |
| Flows, fares and restriction files (`.FFL`, `.RST`) | Prices and time restrictions | Not needed for ranking; prices are out of scope (audit header). |
| Declared ticket acceptance | `DeclaredAcceptance` entitlement | Acceptance is published as prose per incident. Show "Ticket acceptance is in place" with the incident's text where DS has the incident; ranking on it needs structured data that is not published. Later. |
| Darwin loading | `crowding` | Push Port (DS-126); later. |
| TRUST delay attribution | Not needed (§17.1) | None |

### 17.8 Phasing

| Phase | Content | Lands with |
|---|---|---|
| **R0** | `alternative_ranking` module; `Unknown` and `fare_class` guessed from `ticket_type` text (Advance, Off-Peak, Anytime, Season keywords); picks Fastest, Fewest changes, Same operator (from the Pareto set and `PlannedLeg::Train.operator` only), No Tube; fit badges; the cost model with default weights | Phase 1 (the sheet) |
| **R1** | Operator id on connections and the operator and transfer-mode restrictions; constrained searches for Same operator and No Tube; the Advance cancelled window (28.3, acceptance policy); push copy naming Recommended | Phase 2 (sweep) |
| **R2** | Ticket fields and extraction (§17.7), parsed route constraint, the preference setting, "Only trains on my ticket" | after Phase 2 |
| **R3** | Route data from the fares and Routeing Guide feeds | when R2 shows route text is commonly entered |
| **R4** | Declared acceptance text, crowding | with DS-126 and incident ingest |

### 17.9 Tests and metrics

- Table tests for `ticket_fit` covering every row of §17.1's second table
  and §17.5, including split tickets and an unknown ticket.
- Cost tests: "1 minute faster, two more changes" loses; "same operator,
  2 hours later" loses to a covered other-operator train in the
  cancellation window.
- Pick tests: one itinerary winning several picks becomes one card; at
  most three cards.
- Planner tests for the operator and transfer-mode restrictions in CSA,
  and a parity test that an empty restriction changes nothing.
- `api_journey_replan_accepted_total` gains `pick` (recommended, fastest,
  fewest_changes, same_operator, no_tube, rejoin, more) and `fit`; the
  weights are re-tuned from it after a month.

### 17.10 Open questions for the user

1. **One Recommended pick plus badged alternatives, rather than one
   ranked list or picks with no recommendation?** Recommended: yes, at
   most three cards.
2. **With no ticket on the journey,** recommend by cost with a small
   same-operator preference and always show a "Same operator" pick?
   Recommended: yes, plus the prompt to add a ticket.
3. **Flag, never hide, options DS thinks the ticket does not cover**,
   except under "Only trains on my ticket"? Recommended: yes; DS's
   ticket knowledge is partial and the user may buy a new ticket.
4. **A global preference setting** (Automatic, Fastest, Fewest changes,
   Fewest transfers, Only on my ticket) with per-break picks as the
   override? Recommended: yes, default Automatic.
5. **Capture `fare_class` and the printed route** (and optionally the
   restriction code) on tickets, extending the audit header's allowed
   list? Recommended: yes; none is price, barcode, ITSO, name or file
   data.
6. **Ingest the route data from the fares and Routeing Guide feeds** (R3,
   2 to 3 days), but not the full Routeing Guide? Recommended: yes to the
   route files once R2 is in use; no to the full guide.
7. **Drop delay attribution as a ranking input**, since passenger rights
   turn on railway versus passenger fault, which §4.2 already has?
   Recommended: yes.
8. **Starting weights** (a change worth 10 minutes, a Tube or bus
   transfer 10, "may need a new ticket" 90, "Fastest" shown when 15
   minutes earlier)? Recommended: start with these, tune from the `pick`
   metric.
9. **Push names the Recommended train** ("Next LNER train 10:04, arrives
   12:31") rather than a count of options? Recommended: yes when a
   Recommended exists with `Covered` fit; the count otherwise.


**Decided 2026-10-22.** The user accepted every recommendation above: one
Recommended option plus up to two badged picks, each with a ticket-fit
badge, flagging rather than hiding what the ticket may not cover; a
travel preference setting defaulting to Automatic; capturing fare class
and the printed route now and ingesting the fares feed's route files
after that; and the remaining defaults (no-ticket ranking, no delay-fault
input, the starting weights, Fastest at 15+ min earlier, and the push
naming the Recommended train only when it is usually covered).

### 17.11 Sources

- National Rail Conditions of Travel, from 1 April 2026: conditions 9.4,
  9.5, 11, 12, 13, 16, 28, 32.
  <https://assets.nationalrail.co.uk/e8xgegruud3g/217USO6GPDMxJmB4YAJLRi/f103a3edc5f39a0a27832dbeeb53863e/National_Rail_Conditions_of_Travel_2026.pdf>
- National Rail, Advance tickets terms and conditions.
  <https://www.nationalrail.co.uk/tickets-railcards-and-offers/ticket-types/advance-tickets/advance-tickets-terms-and-conditions/>
- National Rail, Changing trains.
  <https://www.nationalrail.co.uk/travel-information/changing-trains/>
- National Rail, Ticket acceptance between participating Train Operators.
  <https://www.nationalrail.co.uk/help-and-assistance/ticket-acceptance/>
- National Rail, Routeing guide.
  <https://www.nationalrail.co.uk/travel-information/routeing-guide/>
- Rail Delivery Group, RSPS5047 National Routeing Guide Data Feed
  Specification (§6.12 Routes file).
  <https://www.raildeliverygroup.com/files/Publications/services/rsp/RSPS504702-00NationalRoutingGuideDataFeedSpecification.pdf>
- Rail Delivery Group, RSPS5045 Fares and Associated Data Feed Interface
  Specification.
  <https://www.raildeliverygroup.com/files/Publications/services/rsp/RSPS504502-00FaresandAssociatedDataFeedInterfaceSpecification.pdf>
