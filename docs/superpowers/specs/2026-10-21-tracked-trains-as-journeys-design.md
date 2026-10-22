# Tracked trains as one-leg journeys (2026-10-21)

Status: signed off 2026-10-22 (decisions in §9). Phase 1 (expand) is
built; phases 2 and 3 are not. Work log DS-124.

The sign-off changed one recommendation: group shares move to journeys
now (§3, §5, §7), not in a later project.

The decision, from the user: "Track this train" makes a real one-leg
journey in the database, so trains and journeys are one model everywhere:
the data, the API and DS-MCP.

Builds on `2026-09-22-journey-tracking-design.md` (journeys and legs; its
§7 already planned this), `2026-10-14-ds-mcp-delegated-access-design.md`
(DS-MCP's delegated routes) and DS-93 (`tracked_standalone`, migration
`20261021120000`).

## 1. What exists today

**The web app already makes journeys.** `TrackThisTrainButton` sends
`POST /Journeys` with a `knownTrain` leg, and `TrackTrainForm` sends a
`pin` leg (both since journey tracking Phase 1, September). Each makes a
`journeys` row, one `journey_legs` row and the leg's
`train_subscriptions` row in one transaction
(`journeys::create_journey_with_known_train_leg` / `_with_pin_leg`).

**Standalone tracks come only from the two old routes**, which no code in
this repo or DS-MCP calls any more:

- `POST /Train/track` (a CRS + time pin): `train_tracking::create_standalone_pin`.
- `POST /Train/by-uid/{uid}/{date}/track`: `create_standalone_subscription_for_train`.

Both create or reuse a `train_subscriptions` row with no leg and set
`tracked_standalone = TRUE`. DS-93's migration also set the flag on every
row that had no leg.

**One subscription per user per train.** `create_subscription_for_train`
is idempotent per `(user_id, trains_id)`. So one row can be a leg's
train, a standalone train, or both. It can also back legs in several
journeys. A pin has no `trains_id` until it resolves, so a pin can end
up as a second row on a train the user already tracks.

**DS-93's keep rule.** When a leg stops using a subscription ("Change
train", "Remove leg", deleting a journey, or a delegated delete),
`release_leg_subscription` deletes it unless `tracked_standalone` is set
or another leg still uses it. `DELETE /Train/{id}` ("Stop tracking")
deletes the row if no leg uses it. If a leg does use it, the route only
clears the flag.

A side effect of this: on a train the user tracked with the web button
(a one-leg journey), "Stop tracking" does nothing. The row's leg keeps
it, and the modal says "it stays tracked in the journey".

**Lists.** `GET /Train/mine` returns every subscription. `journeyIds`
lists the journeys any of whose legs use the subscription, or another of
the user's subscriptions on the same `trains_id`. The web app's
`splitJourneyTrains` folds each train under its journey. Rows with no
listed journey are "lone trains", shown as one-leg journeys built on the
client (`homeJourneys.loneTrainLeg`, `loneTrainPhase`, `splitTrainRows`).

**Production, read-only check on 2026-10-21** (aggregates only, latest
migration `20261021120000`):

| | |
|---|---|
| subscriptions / users | 23 / 3 |
| `tracked_standalone` | 1 (no leg, resolved, past, not shared, no ticket) |
| standalone and also in a leg | 0 |
| subscriptions with no leg | 1 (the same row) |
| subscriptions in more than one leg | 0 |
| two rows on one `(user, trains_id)` | 0 |
| unresolved pins (`trains_id` NULL) | 10, all in legs |
| journeys | 29 (27 with one leg, 2 with two) |
| legs / bound legs | 31 / 22 |
| `group_trains` / `group_journeys` / tickets | 2 (both on leg trains) / 3 / 0 |
| custom names (subscriptions / journeys) | 0 / 0 |

So the migration converts one row in production.

## 2. Target model

- **Every tracked train is part of a journey.** A standalone track is a
  journey with one train leg (`kind = 'train'`, `match_mode = 'manual'`,
  no window) bound to the subscription. "The train's own journey" means a
  journey with exactly one leg, bound to that subscription. Nothing marks
  it as special, so a one-leg journey from the web button, from a
  conversion or from the old routes is the same thing.
- **A subscription exists only while a leg uses it.** The keep rule
  becomes "another leg still uses it". `tracked_standalone` goes away
  (§7, Phase 3).
- **`train_subscriptions` stays** as the per-user, per-train record.
  Tickets, `group_trains`, notification state, `/Train/{id}` and the
  notifier all keep using its id. Subscription ids do not change, so every
  `/Train/{id}` and `/train/by-id/{id}` link keeps working.
- **One push per user per train** stays as it is. The notifier's
  `candidates_for_trains_id` is already `DISTINCT ON (user_id)`.

## 3. Behaviour by area

| Area | After |
|---|---|
| `tracked_standalone` | Phase 1 stops reading and writing it. The keep rule is "another leg uses it" from phase 1 (a change from the first draft, which kept reading the flag until phase 2): every flagged row with no leg is converted, so the flag only mattered for a row that was also a leg's (0 in production), and decision 2 says such a train goes when its leg does. The migration leaves the flag as it was, for a code rollback. Phase 3 drops the column. |
| `journeyIds` on `GET /Train/mine` | Same meaning. Once Phase 2 is out it is never empty. A new additive field, `singleLegJourneyIds`, lists the train's own journeys: the ones "Stop tracking" deletes. |
| Dedup | `create_subscription_for_train` stays idempotent per `(user, trains_id)`. `POST /Train/by-uid/.../track` on a train a leg already uses creates no new journey. It returns the existing subscription and the journey of its first leg (§9, decision 2). A pin that later resolves onto a train the user already tracks stays its own one-leg journey. Its rows still fold through `journeyIds`, and notifications are still one per user (§9, decision 3). |
| Change train | No change: the old subscription moves its tickets and shares to the new one and is deleted unless another leg uses it. Before Phase 2, `tracked_standalone` also keeps it. |
| Remove leg | No change. Removing the only leg deletes the journey and releases the train, so removing a train from its own journey stops tracking it. |
| Delete journey (web or delegated) | No change: legs cascade, and each leg's train is released. |
| Stop tracking (`DELETE /Train/{id}`) | **Changed.** Deletes the train's own journeys through the `delete_journey` path, which also writes template skip tombstones. The train is then released: deleted, with its tickets and shares, unless a leg of a multi-leg journey still uses it. Then it stays, and the modal names that journey. Status codes stay `204` / `404`. A legless row from the rollout window is deleted directly, as today. |
| Group shares | **Move to journeys (decision 5).** `group_journeys` (it exists since 2026-09-22, with `journey_readable_by` letting members open a shared journey) is the one way to share a tracked train. The migration moves every `group_trains` row whose train has a one-leg journey of its own onto that journey (`group_journeys`, keeping `added_by`/`added_at`) and deletes the moved row. `POST /groups/{id}/trains` becomes a wrapper that shares the train's own journey, and `DELETE /groups/{id}/trains/{trackingId}` removes those journey shares as well as any `group_trains` row. A train with no journey of its own (one only a multi-leg journey uses, or a rollout-window row) is still shared as a `group_trains` row: group pages keep listing those until phase 2 decides them (§9). Sharing at track time now shares the new journey (`POST /groups/{id}/journeys`). `sharedGroupCount` counts groups through both tables, each group once. |
| Tickets | No change (`tracked_train_tickets.tracked_train_id` is the subscription). Attach by `trackingId` is unchanged. |
| Delegated save, list and delete | No change to the routes. `GET /Journeys/mine` also lists the converted trains (one row in production). Converted journeys are `created_via = 'web'`, so a delegated delete refuses them (`404`), as for any web-made journey. Converted journeys don't count toward the daily save cap. A delegated delete already refuses a journey shared into a group (`409 journey_shared`, reading `group_journeys`), so a moved share protects its journey the same way. |
| Retention | No change. A one-leg journey and its train share a date, so `prune_past_travel` removes both in one transaction. |
| Account export | `journeys` gains the converted journeys, and `group_shared_journeys` the moved shares (`group_shared_trains` loses them). `tracked_trains` keeps every subscription, but loses the `tracked_standalone` key in phase 3 (noted in the API changelog). The migration's bookkeeping tables are not exported: they hold nothing the export does not already have, and cascade with the user's journeys on account deletion. |
| Notifications | Train delay and cancellation: no change (one per user per train). Leg-level alerts start for converted trains: station skip, through `list_committed_legs_for_today`, per leg, when the leg has an origin and destination. Every web-tracked train already gets these. The "unmatched" alert covers only auto-matched legs, so converted legs (manual) never get it. Converting never puts a train in two legs, so it adds no duplicate skip alerts. Copy is unchanged: a one-leg journey already uses the plain tracked-train wording (`journey_leg_for_train_subscription`, `total_legs == 1`). **Group members:** the notifier sends nothing to group members today, for a shared train or a shared journey; moving shares changes no push. Pushing to members would be a new feature (§9). |

## 4. API compatibility

Every route stays. All shape changes are additive.

| Route | Change |
|---|---|
| `POST /Train/track` | Thin wrapper over `create_journey_with_pin_leg`, with the same validation, pin cap and schedule/backlog match. It also checks the journey cap (`MAX_JOURNEYS_PER_USER`, 1000), the one new possible `400`. Response gains `journeyId`. |
| `POST /Train/by-uid/{uid}/{date}/track` | Thin wrapper over `create_journey_with_known_train_leg`. It keeps its own checks: horizon, past window, both caps and uid normalisation. If a leg already uses the user's subscription for that train, it creates nothing and returns that subscription (idempotent, as today). Response `{trackingId}` gains `journeyId`. |
| `GET /Train/mine` | Gains `singleLegJourneyIds: number[]`. `journeyIds` is unchanged. |
| `GET /Train/{id}` | Gains `journeyIds` and `singleLegJourneyIds`, so the by-id page can link to the journey. |
| `DELETE /Train/{id}` | New meaning (§3), same status codes. |
| `POST /Train/{id}/name` | Still renames the subscription, and now also renames the train's own one-leg journeys in the same transaction (§9, decision 4). |
| `/Train/{id}/tickets/*`, `/Train/tickets/*` | No change. |
| `POST /groups/{id}/trains` | Wrapper: shares the train's own journey (`group_journeys`), else, for a train with none, writes `group_trains` as before. Same body, same `204` / `404`. |
| `DELETE /groups/{id}/trains/{trackingId}` | Also removes the group's shares of the train's own journeys, by the same sharer-or-manager rule. `404` only when neither table had a row. |
| `GET /groups/{id}/trains`, `GET /groups/shared-trains` | Unchanged shape; list only the remaining `group_trains` rows. A moved share is listed by `GET /groups/{id}/journeys` and `GET /groups/shared-journeys`, which the Journeys page now reads too. |
| `/Journeys/*`, other `/groups/*` | No change. |

**What DS-MCP calls** (from `src/ds/client.ts`, `src/ds/delegatedClient.ts`
and `src/tools/*`), and the effect of each:

| Call | Effect |
|---|---|
| `GET /Train/by-uid/{uid}/{date}` (public train state) | None. |
| `GET /Journeys/shared/{token}` | None. |
| `GET /Journeys/mine` (delegated, `list_my_journeys`) | Same shape. It may list a few more one-leg journeys: one in production. |
| `GET /Journeys/{id}` (delegated) | None. |
| `POST /Journeys/itinerary` (delegated save) | None. A train leg still reuses the user's subscription for that train. |
| `DELETE /Journeys/{id}` (delegated) | None. Converted journeys are web-made, so they return `404 not_found`, as before. |
| Plus `/public/*`, `/Line/*`, `/Trips/plan`, `/delegated-grants/revoke` | Not touched. |

DS-MCP does not call `/Train/mine`, `/Train/track` or `/Train/{id}`, so it
needs no change. The API changelog entry says so.

## 5. Migration

### 5.1 Conversion and share move (phase 1, `20261022090000_tracked_trains_as_journeys.sql`)

- **Bookkeeping:** `standalone_track_conversions (journey_id, train_subscription_id,
  converted_at)` and `group_train_share_moves (group_id, train_subscription_id,
  journey_id, added_by, added_at, created_share, moved_at)`, for the rollback
  script. Both cascade with their journey, subscription or group, have no
  service grants (`db-grants.yaml`) and go in phase 3.
- **Conversion:** a `DO` loop of 500-row batches. Each batch takes
  subscriptions no leg uses, draws each journey id from the sequence
  first, then inserts the journey (`created_at` the track time, the
  subscription's `custom_name`), its one leg (`leg_order` 1, the pin's ends
  else the train's, the service date, `match_mode` `manual`) and the
  bookkeeping row in one statement. Drawing the id first avoids the
  `RETURNING` problem the 2026-09-22 migration solved with a scratch
  column. `created_via` defaults to `'web'`, `kind` to `'train'`.
- **Share move:** each `group_trains` row whose train has a one-leg
  journey of its owner's (the oldest, if several) becomes a
  `group_journeys` row on it, with the same `added_by` and `added_at`,
  unless the journey is already shared into that group; the move is
  recorded (with whether it made the journey share) and the row deleted.
- **Ids:** subscription ids, tickets, notification state and every
  remaining `group_trains` row are untouched. Only journeys get new ids.
- **Idempotent and rerunnable:** `CREATE ... IF NOT EXISTS`, and each step
  only picks what is left (a legless subscription, a `group_trains` row).
  Phase 2 reruns the same statements as its catch-up; the DB test runs the
  file twice.
- **Locks:** `lock_timeout` 5 s; the statements take row locks on the rows
  they write and `group_trains` rows they delete. No table rewrite.

### 5.2 Catch-up (Phase 2) and contract (Phase 3)

The api Deployment is `Recreate`, but the migrate hook Job runs while the
old pods still serve, so an old pod can still write a legless standalone
row (or a `group_trains` share) after the migration. Phase 1 code handles
both: "Stop tracking" deletes a legless row outright, and the share
wrapper and lists still read `group_trains`. Phase 2 ships a data-only
migration that runs the same statements again.
Phase 3 has a `-- contract:` header: it drops
`train_subscriptions.tracked_standalone` and `standalone_track_conversions`.
It ships at least one release after the last binary that read them
(`check-migration-order.py`).

### 5.3 Rollback

- **Code** (Phase 1 or 2): redeploy the previous image. Old code works
  with converted data. Each converted train is a one-leg journey whose
  subscription still has `tracked_standalone = TRUE`, so the old UI
  folds the train under its journey and the old keep rule still applies.
  The one visible difference: old "Stop tracking" on a converted train
  only clears the flag (today's DS-93 behaviour for a train in a
  journey), so the user deletes the journey instead.
- **Data** (before phase 3 only): `scripts/rollback-tracked-trains-as-journeys.sql`,
  run as the owner with `psql --single-transaction`. It puts each moved
  share back in `group_trains` (and removes the journey share the move
  made), then deletes each converted journey that still has exactly its
  one leg on the same train, with its share links. The subscriptions stay,
  with `tracked_standalone` as it was. The DB test
  `tracked_trains_as_journeys_migration_converts_and_rolls_back` runs the
  migration, a rerun and this script on fixtures in a rolled-back
  transaction.
- **After Phase 3** the conversion cannot be undone. That is why the
  contract step waits a release.

### 5.4 Production

One row converts into one journey for one user. That train is in the
past and not shared, so the retention sweep removes both together. Users
see one more row in `/journeys` (Past), where the train was already
listed. DS-MCP's list for that user gains one past journey.

Both `group_trains` rows (checked read-only on 2026-10-22) are on trains
with a one-leg journey of their own, and neither journey is already shared
into the same group, so both move: the two shares show under the groups'
journeys instead of their trains, and members open them at
`/journeys/{id}`.

## 6. Frontend

- **"Track this train" and the track form:** they already send
  `POST /Journeys` and go to `/journeys/{id}`. Sharing into a group at
  track time now shares the new journey (`POST /groups/{id}/journeys`).
- **Train pages** (`/train/[uid]/[date]`, `/train/by-id/[trackingId]`):
  - "Stop tracking" copy comes from `singleLegJourneyIds` and
    `journeyIds`. With only the train's own journey it reads "Stop
    tracking this train?" with "Its journey is deleted too.", plus the
    group warning. With a multi-leg journey it says the train stays in that
    journey. This fixes today's "Stop tracking" doing nothing.
  - An "Open journey" link to the train's own journey.
  - The by-id page stays (§9, decision 6).
- **`/journeys`, shared** (phase 1): journeys other members shared into
  the caller's groups are listed beside the shared trains, one row per
  journey with a "From <group>" tag per group, linking to
  `/journeys/{id}`. Without this, shares moved onto journeys would have
  dropped off the page.
- **`/journeys` and home** (Phase 2, once no legless rows remain):
  - Remove the lone-train path: `splitJourneyTrains`'s `standalone`
    output, `splitTrainRows`, `homeJourneys`' `HomeLoneTrain`,
    `isLoneTrain`, `loneTrainLeg`, `loneTrainPhase`, and the lone reads in
    `homeDetailReads`.
  - Home no longer needs `GET /Train/mine` just to fold trains, so that
    fetch goes. `/journeys` keeps it only where it still feeds something,
    such as the ticket-attach picker.
  - A train whose journey falls outside the 100-row cap is no longer
    shown on its own. It behaves like any journey past the cap.
- **Naming:** one-leg journeys follow the journey naming rule
  (`journeyDisplayName`, "Origin to Destination, 17 Sep 2026", custom
  name first). That is what web-tracked trains already show. The old
  lone-train label ("HH:MM Origin to Destination") disappears with the
  lone-train path. The departure time is already on the row. Rename
  mirroring: §9, decision 4.

## 7. Phasing (expand, then contract)

1. **Phase 1, expand plus behaviour (built).**
   - Migration `20261022090000` (§5.1) and the rollback script.
   - The two track routes as journey wrappers, with `journeyId`.
   - "Stop tracking" deletes the train's own journeys.
   - The keep rule without `tracked_standalone`; nothing writes the flag.
   - Train renames mirrored onto the train's own journeys.
   - `singleLegJourneyIds` on `GET /Train/mine`, `journeyIds` and
     `singleLegJourneyIds` on the owner's `GET /Train/{id}`, and
     `sharedGroupCount` over both share tables.
   - Group shares: the move, the `/groups/{id}/trains` wrappers, track-time
     sharing of the new journey, and the Journeys page listing journeys
     shared with the caller.
   - Train pages: "Open journey", and the "Stop tracking" copy.
   - API changelog entry.
2. **Phase 2, settle.**
   - Catch-up migration: the same statements again.
   - Frontend: remove the lone-train path (`splitJourneyTrains`'
     `standalone` output, `splitTrainRows`, `homeJourneys`' lone-train
     helpers), and home's `GET /Train/mine` fetch if nothing else needs it.
   - Decide the `group_trains` rows left (trains only a multi-leg journey
     uses, §9).
   - The daily retention run logs, and counts in a metric, any subscription
     no leg uses. It should find none.
3. **Phase 3, contract.** One migration with a `-- contract:` header drops
   `tracked_standalone`, `standalone_track_conversions` and
   `group_train_share_moves` (and `group_trains`, if phase 2 empties it),
   at least one release after the last binary that read them. Changelog
   note for the export.

## 8. Risks

- **Rollout window:** old pods write legless rows after the migration.
  Phase 1 code still handles them (they list with empty `journeyIds`,
  and "Stop tracking" deletes them directly). Phase 2 converts them.
- **New caps on old routes:** the journey cap now also applies to
  standalone tracks. Production is far from it (29 journeys in total).
- **Stop tracking now deletes the train's own journeys**, including a
  named or template-made one-leg journey, and its group or link shares.
  The modal says so. A template occurrence gets a skip tombstone, as
  `DELETE /Journeys/{id}` writes.
- **Duplicates:** a pin that resolves onto a train already in a journey
  shows as two journeys. Notifications still dedupe per user. Production
  has none.
- **More skip alerts:** converted trains gain station-skip alerts.
  Production has one converted row, and it is in the past.
- **The `RETURNING`-free conversion** relies on sibling data-modifying
  CTEs satisfying the leg and mapping foreign keys at the end of the
  statement. A DB test covers this, and the test reruns the migration.

## 9. Decisions (signed off 2026-10-22) and open questions

Decisions:

1. "Stop tracking" deletes the train's own one-leg journeys, including
   named or template-made ones and their shares. A multi-leg journey that
   uses the train keeps it.
2. The by-uid track route returns the existing subscription and journey
   when a leg already uses the train; no second journey.
3. A pin that later resolves onto an already-tracked train stays a
   separate journey.
4. Renaming a train renames its own one-leg journeys, one way only.
5. Group shares move to journeys now, with the old train routes kept as
   wrappers and `group_trains` readable until the contract phase.
6. `/train/by-id/{id}` stays and links to the journey.
7. Three deploys: expand, settle, contract.

Open:

1. **Shares of a train only a multi-leg journey uses** (none in
   production). Sharing the train is not sharing its whole journey, which
   would show the other legs. Phase 1 keeps these in `group_trains`.
   Options for phase 2: keep `group_trains` for this case only; refuse
   such a share (`409`, "share the journey instead"); or make the train a
   one-leg journey of its own when shared.
2. **Pushes to group members.** No member gets a push for a shared train
   today, and none does for a shared journey. Moving shares keeps that. If
   members should be told about delays on a shared journey, that is a new
   feature with its own opt-in.
3. **The shared trains list routes** (`GET /groups/{id}/trains`,
   `GET /groups/shared-trains`) keep listing only `group_trains` rows. An
   older client that reads only them no longer sees moved shares. Only the
   web app reads them, and it reads the journey lists too.

## 10. Tests (phase 1, as built)

- **Migration and rollback** (`tracked_trains_as_journeys_migration_converts_and_rolls_back`,
  as the schema owner): a legless train becomes a one-leg journey with its
  name, ends and date; its share moves with its `added_at`; a train a
  multi-leg journey uses gets no journey and keeps its `group_trains`
  share; a rerun converts nothing; the rollback script restores the shares
  and removes the journeys, leaving the flags. Batching is not exercised
  past one batch.
- **Track routes:** by-uid twice returns one subscription and one one-leg
  journey, with `journeyId` (`post_track_by_uid_called_twice_returns_the_same_subscription`);
  the data function's own test checks the leg and the no-second-journey
  rule, and the train rename reaching the journey.
- **Stop tracking:** own journey with a journey share, a `group_trains`
  share and a ticket all go; another user's id is refused; a legless row
  is deleted; a two-leg journey keeps the train (data and route tests).
- **DS-93 tests** rewritten for the model: "tracked on its own" is a
  one-leg journey, listed under it (`singleLegJourneyIds`), never as a
  lone row.
- **Groups:** sharing a train with a journey of its own shares the journey,
  counts once, and unsharing removes it (`add_train_to_group_shares_the_trains_own_journey`).
- **Frontend (vitest):** the "Stop tracking" copy, "Open journey" on the
  by-id page, track-time sharing of the journey, and shared journeys on
  `/journeys` (merged per journey, the caller's own left out).
- Template tombstones on "Stop tracking" reuse `delete_journey`'s
  function and are not tested again.
