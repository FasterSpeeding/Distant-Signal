# Tracked trains as one-leg journeys (2026-10-21)

Status: design, awaiting sign-off on §9 before implementation. Work log
DS-124.

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
| `tracked_standalone` | Phase 1 stops writing it but still reads it in the keep rule, for rows old pods write during the rollout. Phase 2 stops reading it. Phase 3 drops the column. |
| `journeyIds` on `GET /Train/mine` | Same meaning. Once Phase 2 is out it is never empty. A new additive field, `singleLegJourneyIds`, lists the train's own journeys: the ones "Stop tracking" deletes. |
| Dedup | `create_subscription_for_train` stays idempotent per `(user, trains_id)`. `POST /Train/by-uid/.../track` on a train a leg already uses creates no new journey. It returns the existing subscription and the journey of its first leg (see §9, Q2). A pin that later resolves onto a train the user already tracks stays its own one-leg journey. Its rows still fold through `journeyIds`, and notifications are still one per user (see §9, Q3). |
| Change train | No change: the old subscription moves its tickets and shares to the new one and is deleted unless another leg uses it. Before Phase 2, `tracked_standalone` also keeps it. |
| Remove leg | No change. Removing the only leg deletes the journey and releases the train, so removing a train from its own journey stops tracking it. |
| Delete journey (web or delegated) | No change: legs cascade, and each leg's train is released. |
| Stop tracking (`DELETE /Train/{id}`) | **Changed.** Deletes the train's own journeys through the `delete_journey` path, which also writes template skip tombstones. The train is then released: deleted, with its tickets and shares, unless a leg of a multi-leg journey still uses it. Then it stays, and the modal names that journey. Status codes stay `204` / `404`. A legless row from the rollout window is deleted directly, as today. |
| Group shares | `group_trains` stays keyed on the subscription and keeps working. Sharing at track time (`TrackDestinationModal`) still writes `group_trains`. Moving one-leg shares to `group_journeys` is a separate decision (§9, Q5). |
| Tickets | No change (`tracked_train_tickets.tracked_train_id` is the subscription). Attach by `trackingId` is unchanged. |
| Delegated save, list and delete | No change to the routes. `GET /Journeys/mine` also lists the converted trains (one row in production). Converted journeys are `created_via = 'web'`, so a delegated delete refuses them (`404`), as for any web-made journey. Converted journeys don't count toward the daily save cap. |
| Retention | No change. A one-leg journey and its train share a date, so `prune_past_travel` removes both in one transaction. |
| Account export | `journeys` gains the converted journeys. `tracked_trains` keeps every subscription, but loses the `tracked_standalone` key in Phase 3 (noted in the API changelog). |
| Notifications | Train delay and cancellation: no change (one per user per train). Leg-level alerts start for converted trains: station skip, through `list_committed_legs_for_today`, per leg, when the leg has an origin and destination. Every web-tracked train already gets these. The "unmatched" alert covers only auto-matched legs, so converted legs (manual) never get it. Converting never puts a train in two legs, so it adds no duplicate skip alerts. Copy is unchanged: a one-leg journey already uses the plain tracked-train wording (`journey_leg_for_train_subscription`, `total_legs == 1`). |

## 4. API compatibility

Every route stays. All shape changes are additive.

| Route | Change |
|---|---|
| `POST /Train/track` | Thin wrapper over `create_journey_with_pin_leg`, with the same validation, pin cap and schedule/backlog match. It also checks the journey cap (`MAX_JOURNEYS_PER_USER`, 1000), the one new possible `400`. Response gains `journeyId`. |
| `POST /Train/by-uid/{uid}/{date}/track` | Thin wrapper over `create_journey_with_known_train_leg`. It keeps its own checks: horizon, past window, both caps and uid normalisation. If a leg already uses the user's subscription for that train, it creates nothing and returns that subscription (idempotent, as today). Response `{trackingId}` gains `journeyId`. |
| `GET /Train/mine` | Gains `singleLegJourneyIds: number[]`. `journeyIds` is unchanged. |
| `GET /Train/{id}` | Gains `journeyIds` and `singleLegJourneyIds`, so the by-id page can link to the journey. |
| `DELETE /Train/{id}` | New meaning (§3), same status codes. |
| `POST /Train/{id}/name` | Still renames the subscription. Whether it also renames the train's own journey is §9, Q4. |
| `/Train/{id}/tickets/*`, `/Train/tickets/*` | No change. |
| `/Journeys/*`, `/groups/*` | No change. |

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

### 5.1 Conversion (Phase 1, `2026102xxxxxxx_standalone_tracks_to_journeys.sql`)

```sql
SET LOCAL lock_timeout = '5s';

-- Which journeys this migration made, for the rollback in 5.3.
-- Dropped in Phase 3.
CREATE TABLE IF NOT EXISTS standalone_track_conversions (
    journey_id            BIGINT PRIMARY KEY REFERENCES journeys(id) ON DELETE CASCADE,
    train_subscription_id BIGINT NOT NULL UNIQUE
                          REFERENCES train_subscriptions(id) ON DELETE CASCADE,
    converted_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DO $$
DECLARE converted INT;
BEGIN
  LOOP
    WITH batch AS (
      SELECT nextval(pg_get_serial_sequence('journeys', 'id')) AS journey_id,
             ts.id, ts.user_id, ts.custom_name, ts.tracked_at, ts.service_date,
             COALESCE(ts.pin_origin_crs, tr.origin_crs) AS origin_crs,
             COALESCE(ts.pin_destination_crs, tr.destination_crs) AS destination_crs
      FROM train_subscriptions ts
      LEFT JOIN trains tr ON tr.id = ts.trains_id
      WHERE NOT EXISTS (SELECT 1 FROM journey_legs jl WHERE jl.train_subscription_id = ts.id)
      ORDER BY ts.id
      LIMIT 500
      FOR UPDATE OF ts
    ), j AS (
      INSERT INTO journeys (id, user_id, custom_name, created_at, updated_at)
      SELECT journey_id, user_id, custom_name, tracked_at, tracked_at FROM batch
    ), l AS (
      INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs,
                                service_date, train_subscription_id, match_mode)
      SELECT journey_id, 1, origin_crs, destination_crs, service_date, id, 'manual' FROM batch
    )
    INSERT INTO standalone_track_conversions (journey_id, train_subscription_id)
    SELECT journey_id, id FROM batch;
    GET DIAGNOSTICS converted = ROW_COUNT;
    EXIT WHEN converted = 0;
  END LOOP;
END $$;
```

- **What it converts:** every subscription no leg uses. Since DS-93,
  that set is exactly the trains tracked on their own. A row that is
  standalone and also in a leg (0 in production) gets no second journey
  (§9, Q2).
- **Ids:** the journey id is taken from the sequence before the insert,
  so the leg and the mapping row match it exactly. That avoids the
  `RETURNING` problem the 2026-09-22 migration solved with a scratch
  column. Subscription ids, ticket and share rows and notification state
  are not touched.
- **Idempotent:** each batch picks only subscriptions with no leg, and a
  converted row has one. A rerun, or the Phase 2 catch-up, converts only
  what is still legless.
- **Batched:** 500 rows per statement inside the migration's one
  transaction. That bounds each statement's work and memory under
  `MIGRATION_STATEMENT_TIMEOUT_SECS`. For production (1 row) it is one
  batch. A database too large for one transaction would move this loop
  into a `ds-migrate` subcommand, as `backfill-trains` did. That is not
  needed here.
- **Defaults:** `created_via` defaults to `'web'` and `kind` to
  `'train'`. `journeys.created_at` is the track time, so lists keep their
  order. The journey's `custom_name` is copied from the subscription.
- **Grants:** the mapping table goes in `db-grants.yaml` with no service
  role. Only the migrator touches it, and `gen-db-grants.py check` must
  pass.

### 5.2 Catch-up (Phase 2) and contract (Phase 3)

During the Phase 1 rollout, old pods can still write legless standalone
rows. Phase 2 ships a data-only migration that runs the same loop again.
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
- **Data** (before Phase 3 only): a reviewed script,
  `scripts/rollback-standalone-track-conversion.sql`, deletes the journeys
  listed in `standalone_track_conversions` that still have exactly that
  one leg, bound to the same subscription, together with their
  `unlisted_links` rows. Group journey shares cascade. The subscriptions
  stay (the FK is on the leg), and their `tracked_standalone` is still
  set. The script is tested against a fixture like the existing
  `scripts/test-remediate-*.sh`.
- **After Phase 3** the conversion cannot be undone. That is why the
  contract step waits a release.

### 5.4 Production

One row converts into one journey for one user. That train is in the
past and not shared, so the retention sweep removes both together. Users
see one more row in `/journeys` (Past), where the train was already
listed. DS-MCP's list for that user gains one past journey. Nothing else
changes.

## 6. Frontend

- **"Track this train" and the track form:** no change. They already
  send `POST /Journeys` and go to `/journeys/{id}`.
- **Train pages** (`/train/[uid]/[date]`, `/train/by-id/[trackingId]`):
  - "Stop tracking" copy comes from `singleLegJourneyIds` and
    `journeyIds`. With only the train's own journey it reads "Stop
    tracking this train? Its journey is deleted too", plus the group
    warning. With a multi-leg journey it says the train stays in that
    journey. This fixes today's "Stop tracking" doing nothing.
  - An "Open journey" link to the train's own journey.
  - The by-id page stays. Redirecting it to the journey is §9, Q6.
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
  mirroring is §9, Q4.

## 7. Phasing (expand, then contract)

1. **Phase 1, expand plus behaviour.**
   - Migration 5.1.
   - The wrappers for both track routes.
   - The new "Stop tracking" semantics.
   - Additive `journeyId`, `singleLegJourneyIds` and `journeyIds` fields.
   - Stop writing `tracked_standalone`. The keep rule still reads it.
   - Train-page copy and the journey link.
   - API changelog entry.
2. **Phase 2, settle.**
   - Catch-up migration.
   - Remove every read of `tracked_standalone`, so the keep rule is
     "another leg uses it".
   - Remove the frontend lone-train path.
   - The daily retention run logs a warning, and a metric counts it,
     for any legless subscription. It should never find one.
3. **Phase 3, contract.** Drop the column and the mapping table, with the
   changelog note for the export.

Phases 1 to 3 are three merges, each deployed before the next. Step B
builds all three on one branch as separate commits, so each can be
merged and deployed in turn.

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

## 9. Open questions for the user

1. **Stop tracking deletes the train's own journey.** That includes a
   one-leg journey the user named or got from a template, and its shares
   (with a clear modal). Recommended: yes. That is what "one model" means,
   and it fixes today's no-op button.
2. **Tracking a train already in a journey,** through the old by-uid
   route. Recommended: return the existing subscription and create no
   second journey (idempotent, as today). The train then stops being
   tracked if it is removed from that journey. Production has 0 such
   rows. The alternative is a separate one-leg journey, which shows the
   train twice.
3. **A pin that resolves onto a train already tracked** stays a separate
   one-leg journey. Recommended: accept (rare, 0 in production, one push
   per user either way). The alternative, merging at resolution time, is
   larger and can surprise the user.
4. **Names.** Recommended: renaming the train (`POST /Train/{id}/name`)
   also renames its own journeys. Renaming a journey does not touch the
   train. The conversion copies the train's name to the journey (0 named
   rows in production).
5. **Group shares of a one-leg journey.** Recommended: keep `group_trains`
   in this project. Moving one-leg shares (and track-time sharing) to
   `group_journeys` would be a separate change.
6. **`/train/by-id/{id}`.** Recommended: keep it as the live train view
   and link to the journey. The alternative is to redirect it to the
   train's own journey.
7. **Phasing.** Recommended: three deploys (§7). The alternative is to
   fold Phase 2 into Phase 1 and accept that rows written by old pods
   during the rollout show as lone trains until Phase 3's catch-up.

## 10. Tests (step B)

- **Migration (DB tests):**
  - Converts legless rows with their fields, ids and mapping.
  - Leaves rows that have a leg alone.
  - A rerun is a no-op.
  - Batches past 500 rows.
  - Checked under `scripts/test-postgres-roles.py`, as superuser and per
    service.
- **Rollback script:** tested against a fixture.
- **Wrapper routes:**
  - Each creates a one-leg journey and returns `journeyId`.
  - by-uid is idempotent when a leg already uses the train.
  - Caps.
- **Stop tracking:**
  - Own journey only.
  - Own journey plus a multi-leg journey.
  - Template occurrence (tombstone).
  - Legless rollout row.
  - Shares and tickets cascade.
- **Keep rule** without the flag (Phase 2). The DS-93 tests updated.
- **`GET /Train/mine` and `GET /Train/{id}`:** the new fields.
- **Frontend:** vitest for the stop-tracking copy, the journey link, and
  the removed lone-train paths (home and `/journeys`).
- **Checks:**
  - `cargo fmt`, `clippy -D warnings`.
  - `check-migration-order.py`, `gen-db-grants.py check`.
  - Frontend lint, `tsc`, vitest and build.
  - The API changelog entry.
