//! Identity primitives for the shared `trains` table
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md).
//! `find_or_create_train` is the one idempotent upsert every dual-write
//! path in this plan funnels through -- Step B's own one-off backfill uses
//! the exact same `ON CONFLICT ... DO UPDATE ... RETURNING id` shape.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// Finds or creates the `trains` row for `(train_uid, service_date)`,
/// returning its surrogate `id`. `DO UPDATE` (never `DO NOTHING`) is what
/// makes `RETURNING id` reliable on a re-run against a row that already
/// exists -- safe to call more than once for the same identity.
pub async fn find_or_create_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO trains (train_uid, service_date) \
         VALUES ($1, $2) \
         ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid \
         RETURNING id",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Batch-shaped sibling of [`find_or_create_train`] -- one multi-row
/// `INSERT ... SELECT * FROM UNNEST(...) ... ON CONFLICT DO UPDATE
/// ... RETURNING` covering every DISTINCT `(train_uid, service_date)` pair
/// in `pairs`, instead of one single-row `INSERT` per pair. `DO UPDATE`
/// (never `DO NOTHING`) is what makes every input pair come back in the
/// `RETURNING` set even when it already existed, exactly like the
/// single-row version -- callers can rely on the returned map having
/// exactly one entry per element of `pairs` (never fewer).
///
/// `pairs` MUST already be deduplicated by the caller: passing the same
/// `(train_uid, service_date)` pair twice in one call is a caller bug,
/// not something this function guards against -- `ingest_shared_movement`'s
/// whole reason for calling this at all is to have already collapsed a
/// batch's repeated identities down to their distinct pairs before this
/// point.
pub async fn find_or_create_trains_batch(
    pool: &PgPool,
    pairs: &[(String, NaiveDate)],
) -> anyhow::Result<HashMap<(String, NaiveDate), i64>> {
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }
    let train_uids: Vec<&str> = pairs.iter().map(|(uid, _)| uid.as_str()).collect();
    let service_dates: Vec<NaiveDate> = pairs.iter().map(|(_, date)| *date).collect();

    let rows: Vec<(String, NaiveDate, i64)> = sqlx::query_as(
        "INSERT INTO trains (train_uid, service_date) \
         SELECT * FROM UNNEST($1::text[], $2::date[]) \
         ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid \
         RETURNING train_uid, service_date, id",
    )
    .bind(&train_uids)
    .bind(&service_dates)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(train_uid, service_date, id)| ((train_uid, service_date), id))
        .collect())
}

/// Same idempotent shape as [`find_or_create_train`], but also mirrors a
/// successful schedule match's own result onto the shared row (Step A's
/// "attempt_schedule_match... mirror their result onto the shared trains
/// row too"). `COALESCE`d against the existing value on every schedule
/// column so a second subscriber's independent match against the same
/// physical train never clobbers data an earlier one already wrote.
#[allow(clippy::too_many_arguments)]
pub async fn find_or_create_train_with_schedule_match(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
    origin_crs: &str,
    scheduled_departure: DateTime<Utc>,
    destination_crs: Option<&str>,
    matched_line_id: &str,
    calling_points: &serde_json::Value,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO trains \
            (train_uid, service_date, origin_crs, scheduled_departure, destination_crs, \
             matched_line_id, calling_points, schedule_matched_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) \
         ON CONFLICT (train_uid, service_date) DO UPDATE SET \
            train_uid            = EXCLUDED.train_uid, \
            origin_crs           = COALESCE(trains.origin_crs, EXCLUDED.origin_crs), \
            scheduled_departure  = COALESCE(trains.scheduled_departure, EXCLUDED.scheduled_departure), \
            destination_crs      = COALESCE(trains.destination_crs, EXCLUDED.destination_crs), \
            matched_line_id      = COALESCE(trains.matched_line_id, EXCLUDED.matched_line_id), \
            calling_points       = COALESCE(trains.calling_points, EXCLUDED.calling_points), \
            schedule_matched_at  = COALESCE(trains.schedule_matched_at, EXCLUDED.schedule_matched_at) \
         RETURNING id",
    )
    .bind(train_uid)
    .bind(service_date)
    .bind(origin_crs)
    .bind(scheduled_departure)
    .bind(destination_crs)
    .bind(matched_line_id)
    .bind(calling_points)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Mirrors a live-TRUST resolution onto the shared row's own
/// Live-TRUST-derived columns (`train_id`, `resolved_at`) -- never
/// clobbers `train_uid`/schedule columns, which this function doesn't
/// touch at all. Safe to call more than once for the same `trains_id`
/// (a later Movement re-supplying the same `train_id` is a harmless
/// no-op overwrite of identical values).
pub async fn mark_train_resolved(
    pool: &PgPool,
    trains_id: i64,
    train_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE trains SET train_id = $2, resolved_at = NOW() WHERE id = $1")
        .bind(trains_id)
        .bind(train_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Batch-shaped sibling of [`mark_train_resolved`] -- one `UPDATE ...
/// FROM UNNEST(...)` covering every `(trains_id, train_id)` pair in
/// `pairs`, instead of one single-row `UPDATE` per pair.
///
/// `pairs` MUST carry at most one entry per `trains_id` (the caller's
/// responsibility, same as [`find_or_create_trains_batch`]'s dedup
/// contract) -- if a batch has more than one event resolving to the same
/// `trains_id`, the caller must already have collapsed those down to the
/// LAST one in event order, matching what a sequential loop of
/// [`mark_train_resolved`] calls would leave behind (each call plainly
/// overwrites `train_id`, so only the final call's value survives).
pub async fn mark_trains_resolved_batch(
    pool: &PgPool,
    pairs: &[(i64, String)],
) -> anyhow::Result<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let trains_ids: Vec<i64> = pairs.iter().map(|(id, _)| *id).collect();
    let train_ids: Vec<&str> = pairs
        .iter()
        .map(|(_, train_id)| train_id.as_str())
        .collect();

    sqlx::query(
        "UPDATE trains AS t SET train_id = u.train_id, resolved_at = NOW() \
         FROM UNNEST($1::bigint[], $2::text[]) AS u(trains_id, train_id) \
         WHERE t.id = u.trains_id",
    )
    .bind(&trains_ids)
    .bind(&train_ids)
    .execute(pool)
    .await?;
    Ok(())
}

// Step B's one-off backfill job (`backfill_trains_id_for_resolved_rows`,
// which used to live here) already ran, exactly once, against every real
// environment -- Task 22's own Step 1 dry-run count of 0 confirms nothing
// was left for it to do. Its own `SELECT id, train_uid, service_date FROM
// tracked_trains WHERE train_uid IS NOT NULL ...` read a column
// (`tracked_trains.train_uid`) Task 22's migration drops entirely, so it
// can never run again; removed here, alongside its own one-off
// `run_step_b_backfill_of_existing_resolved_rows` test below, rather than
// left as permanently-broken dead code.

/// The shared `trains` row's own known final/terminus CRS
/// (`trains.destination_crs`) for one `trains_id`, or `None` if the row
/// doesn't exist yet or no schedule has ever matched it. Feeds
/// `trust_event_backlog_match::replay_backlog_history`'s confirmed-terminus-
/// ARRIVAL detection (`trust_schema::journey::apply_movement`'s
/// `destination_crs` param) -- a read-only precheck, same posture as
/// `shared_train_enrichment_state` just below: it never creates a row, so a
/// train with no `trains` row at all simply reports "unknown" rather than
/// conjuring one into existence.
pub async fn destination_crs_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<String>> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT destination_crs FROM trains WHERE train_uid = $1 AND service_date = $2",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(destination_crs,)| destination_crs))
}

/// Batch-shaped sibling of [`destination_crs_for_train`] -- one SELECT
/// covering every DISTINCT `trains_id` in `trains_ids`, rather than one
/// SELECT per id, mirroring `trust_event_backlog::fetch_previous_derived_states_batch`'s
/// own shape. A `trains_id` with no known `destination_crs` (no schedule
/// match yet, or the row doesn't exist) is simply absent from the returned
/// map -- callers must treat a missing key the same as `None`.
pub async fn destination_crs_for_trains_batch(
    pool: &PgPool,
    trains_ids: &[i64],
) -> anyhow::Result<HashMap<i64, String>> {
    if trains_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT id, destination_crs FROM trains WHERE id = ANY($1)")
            .bind(trains_ids)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, destination_crs)| destination_crs.map(|crs| (id, crs)))
        .collect())
}

/// `(has schedule data, has a live/backlog resolution)` for one shared
/// `trains` row, or `None` if no such row exists.
///
/// Only ever a precheck: `routes::train::enrich_shared_train` uses it to
/// skip an enrichment pass that provably has nothing to add (the common
/// case once a train has any subscribers at all), and every writer it
/// guards is independently idempotent, so a stale read here costs at worst
/// one redundant no-op pass.
pub async fn shared_train_enrichment_state(
    pool: &PgPool,
    trains_id: i64,
) -> anyhow::Result<Option<(bool, bool)>> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT schedule_matched_at IS NOT NULL, resolved_at IS NOT NULL \
         FROM trains WHERE id = $1",
    )
    .bind(trains_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The public, unscoped read-model for `GET /Train/by-uid/{uid}/{date}`
/// (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §4).
/// Unlike `train_tracking::TrackedTrainState` (which this route used to
/// return), this carries no `custom_name`/pin fields at all -- those are
/// per-subscriber private data with no place on a shared, public row.
/// Darwin ETA blending (`crate::data::eta_blend`, wired into the legacy
/// `TrackedTrainState` read paths) is deliberately NOT applied here --
/// that helper operates on `TrackedTrainState`'s own pin-shaped input;
/// wiring it into this new public shape is left as a fast-follow, not
/// part of this task's own scope (only the ownership-check removal).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicTrainState {
    /// The SHARED `trains` row's own surrogate key, serialized as
    /// `trainsId`. Deliberately NOT named `id`, which is what this struct
    /// used to call it: every `/Train/{trackingId}` route in this app
    /// interprets its path id as a `train_subscriptions.id`, a completely
    /// different `BIGSERIAL` space that also starts at 1 -- and the
    /// frontend page for this route was feeding this field straight into
    /// `RenameTrainButton`/`DeleteTrainButton`/`TicketPanel` as a
    /// `trackingId`, so a logged-in visitor could rename or delete an
    /// unrelated subscription of their OWN that happened to share the
    /// number. Ownership scoping made cross-user damage impossible, but not
    /// same-user damage. The name is the fix on this side; the frontend no
    /// longer renders those controls on this page at all (see
    /// `frontend/app/train/[uid]/[date]/page.tsx`).
    pub trains_id: i64,
    pub train_uid: String,
    pub service_date: NaiveDate,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub calling_points: Option<serde_json::Value>,
    pub train_id: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
    /// See `train_tracking::TrackedTrainState::journey_stops`'s doc
    /// comment -- same contract, populated the same "read row, then
    /// overlay" way by `routes::train::get_by_uid_and_date`. This struct
    /// already carries `trains_id` on the wire (unlike `TrackedTrainState`,
    /// where it's an internal-only addition), so no extra field is needed
    /// to know which `trains_id` to key the overlay query on. `#[sqlx(skip)]`,
    /// not `#[sqlx(default)]` -- see
    /// `train_tracking::TrackedTrainState::journey_stops`'s doc comment for
    /// why: `JourneyStop` doesn't implement `sqlx::Type`/`Decode`, and
    /// `#[sqlx(default)]`'s generated code still needs that bound even
    /// though this column is never selected.
    #[sqlx(skip)]
    pub journey_stops: Option<Vec<crate::data::journey::JourneyStop>>,
}

/// Whether `(train_uid, service_date)` is a real, CIF-published scheduled
/// train, per `schedule_destination_departures` -- the same product the
/// `/trains` search page itself reads
/// (`queries::search_schedule_destination_departures`). The sole caller is
/// `routes::train::get_by_uid_and_date`'s read-triggered `find_or_create_train`
/// upsert: a GET must be able to conjure a shared `trains` row into
/// existence for an identity a search result actually pointed at (see that
/// route's own doc comment for the bug this closes), but never for an
/// arbitrary string someone puts in the URL -- this is the gate that tells
/// those two cases apart.
///
/// Deliberately a bare existence probe scoped to `train_uid` +
/// `service_date` only, ignoring `destination_crs`/`origin_crs`/`scheduled`
/// entirely -- unlike `queries::search_schedule_destination_departures`,
/// which is a real paginated search, this only ever needs a yes/no answer.
/// `schedule_destination_departures`' primary key leads with
/// `service_date`, so this still rides an index range scan on that column
/// before filtering `train_uid` -- bounded by one rail day's worth of rows
/// (retention is 2 days; see that table's own migration), not a full-table
/// scan.
pub async fn is_known_scheduled_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<bool> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM schedule_destination_departures \
         WHERE train_uid = $1 AND service_date = $2 LIMIT 1",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Public, unscoped read for `(train_uid, service_date)` -- no
/// `AuthenticatedUser`/ownership check anywhere in this call path. Reads
/// `trains` directly (joined with the re-pointed `train_current_state` via
/// `trains_id`, Task 9/11/14), never touches `tracked_trains` at all, so
/// there is no code path here that could reach a subscriber's own
/// `custom_name`/ticket/notification data.
pub async fn get_public_train_state(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<PublicTrainState>> {
    let row = sqlx::query_as::<_, PublicTrainState>(
        "SELECT tr.id AS trains_id, tr.train_uid, tr.service_date, tr.origin_crs, so.name AS origin_name, \
                tr.destination_crs, sd.name AS destination_name, tr.scheduled_departure, \
                tr.calling_points, tr.train_id, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs) \
         WHERE tr.train_uid = $1 AND tr.service_date = $2",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Batched sibling of [`get_public_train_state`] -- one query covering
/// every `train_uid` in `train_uids` for the same `service_date`, instead
/// of one query per train. Backs `GET /public/lines/{id}/trains?date=`
/// (docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
/// §5.3) -- the whole reason that route exists is to collapse what would
/// otherwise be one `GET /Train/by-uid` call per scheduled service on a
/// line into a single round trip.
///
/// Returns only rows that actually exist. Does NOT preserve `train_uids`'
/// own order, and does NOT synthesize a placeholder for a UID with no
/// `trains` row -- a scheduled service TRUST hasn't activated yet
/// legitimately has none. Callers key the result by `train_uid` /
/// `PublicTrainState::train_uid` themselves.
///
/// Never writes: unlike `routes::train::get_by_uid_and_date`'s
/// read-triggered `find_or_create_train` upsert, this function performs
/// no insert for a UID with no existing row -- see the spec's "no write
/// side effect" decision (§5.3).
pub async fn get_public_train_states_for_line(
    pool: &PgPool,
    train_uids: &[String],
    service_date: NaiveDate,
) -> anyhow::Result<Vec<PublicTrainState>> {
    if train_uids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, PublicTrainState>(
        "SELECT tr.id AS trains_id, tr.train_uid, tr.service_date, tr.origin_crs, so.name AS origin_name, \
                tr.destination_crs, sd.name AS destination_name, tr.scheduled_departure, \
                tr.calling_points, tr.train_id, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs) \
         WHERE tr.train_uid = ANY($1) AND tr.service_date = $2",
    )
    .bind(train_uids)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_or_create_train_returns_the_same_id_on_a_repeat_call -- --ignored"]
    async fn find_or_create_train_returns_the_same_id_on_a_repeat_call() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let first = find_or_create_train(&pool, "TEST-TRAINS-UID-1", service_date)
            .await
            .expect("first find_or_create_train");
        let second = find_or_create_train(&pool, "TEST-TRAINS-UID-1", service_date)
            .await
            .expect("second find_or_create_train");
        assert_eq!(
            first, second,
            "the same (train_uid, service_date) must resolve to one row"
        );

        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-UID-1'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_or_create_trains_batch_dedups_and_resolves_every_distinct_pair \
                -- --ignored"]
    async fn find_or_create_trains_batch_dedups_and_resolves_every_distinct_pair() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        // Seed one of the two identities up front, so this also proves the
        // batched call resolves a PRE-EXISTING row via its ON CONFLICT
        // branch, not just fresh inserts.
        let pre_existing = find_or_create_train(&pool, "TEST-TRAINS-BATCH-UID-1", service_date)
            .await
            .expect("seed one identity");

        let pairs = vec![
            ("TEST-TRAINS-BATCH-UID-1".to_string(), service_date),
            ("TEST-TRAINS-BATCH-UID-2".to_string(), service_date),
        ];
        let map = find_or_create_trains_batch(&pool, &pairs)
            .await
            .expect("find_or_create_trains_batch");

        assert_eq!(map.len(), 2, "one entry per distinct pair");
        assert_eq!(
            map[&("TEST-TRAINS-BATCH-UID-1".to_string(), service_date)],
            pre_existing,
            "a pre-existing identity must resolve to its EXISTING id, not a new row"
        );
        let second_id = map[&("TEST-TRAINS-BATCH-UID-2".to_string(), service_date)];
        assert_ne!(second_id, pre_existing);

        sqlx::query("DELETE FROM trains WHERE train_uid IN ($1, $2)")
            .bind("TEST-TRAINS-BATCH-UID-1")
            .bind("TEST-TRAINS-BATCH-UID-2")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                mark_trains_resolved_batch_sets_train_id_for_every_pair -- --ignored"]
    async fn mark_trains_resolved_batch_sets_train_id_for_every_pair() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let id_1 = find_or_create_train(&pool, "TEST-TRAINS-BATCH-RESOLVE-1", service_date)
            .await
            .expect("find_or_create_train");
        let id_2 = find_or_create_train(&pool, "TEST-TRAINS-BATCH-RESOLVE-2", service_date)
            .await
            .expect("find_or_create_train");

        mark_trains_resolved_batch(
            &pool,
            &[
                (id_1, "221800001".to_string()),
                (id_2, "221800002".to_string()),
            ],
        )
        .await
        .expect("mark_trains_resolved_batch");

        let (train_id_1,): (Option<String>,) =
            sqlx::query_as("SELECT train_id FROM trains WHERE id = $1")
                .bind(id_1)
                .fetch_one(&pool)
                .await
                .expect("read back trains row 1");
        assert_eq!(train_id_1, Some("221800001".to_string()));

        let (train_id_2,): (Option<String>,) =
            sqlx::query_as("SELECT train_id FROM trains WHERE id = $1")
                .bind(id_2)
                .fetch_one(&pool)
                .await
                .expect("read back trains row 2");
        assert_eq!(train_id_2, Some("221800002".to_string()));

        sqlx::query("DELETE FROM trains WHERE id IN ($1, $2)")
            .bind(id_1)
            .bind(id_2)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                mark_train_resolved_sets_train_id_and_resolved_at -- --ignored"]
    async fn mark_train_resolved_sets_train_id_and_resolved_at() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = find_or_create_train(&pool, "TEST-TRAINS-UID-2", service_date)
            .await
            .expect("find_or_create_train");

        mark_train_resolved(&pool, trains_id, "221832406")
            .await
            .expect("mark_train_resolved");

        let (train_id, resolved_at): (Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as("SELECT train_id, resolved_at FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains row");
        assert_eq!(train_id, Some("221832406".to_string()));
        assert!(resolved_at.is_some());

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_or_create_train_with_schedule_match_never_clobbers_an_earlier_match -- --ignored"]
    async fn find_or_create_train_with_schedule_match_never_clobbers_an_earlier_match() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-06T12:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["PAD", "RDG"]);

        let first_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-TRAINS-UID-3",
            service_date,
            "PAD",
            scheduled_departure,
            None,
            "line-a",
            &calling_points,
        )
        .await
        .expect("first find_or_create_train_with_schedule_match");

        let second_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-TRAINS-UID-3",
            service_date,
            "ZZZ",
            scheduled_departure,
            None,
            "line-b",
            &calling_points,
        )
        .await
        .expect("second find_or_create_train_with_schedule_match");

        assert_eq!(
            first_id, second_id,
            "the same (train_uid, service_date) must resolve to one row"
        );

        let (origin_crs, matched_line_id): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT origin_crs, matched_line_id FROM trains WHERE id = $1")
                .bind(first_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains row");
        assert_eq!(
            origin_crs,
            Some("PAD".to_string()),
            "a later independent match must not clobber the first match's origin_crs"
        );
        assert_eq!(
            matched_line_id,
            Some("line-a".to_string()),
            "a later independent match must not clobber the first match's matched_line_id"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(first_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_state_returns_none_for_no_matching_row -- --ignored"]
    async fn get_public_train_state_returns_none_for_no_matching_row() {
        let pool = connect().await;
        let result = get_public_train_state(&pool, "NOSUCHUID", "2026-09-06".parse().unwrap())
            .await
            .expect("get_public_train_state");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_state_reads_the_shared_row_and_its_current_state -- --ignored"]
    async fn get_public_train_state_reads_the_shared_row_and_its_current_state() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-06T12:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "MKC"]);

        let trains_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-PUBLIC-STATE-UID",
            service_date,
            "EUS",
            scheduled_departure,
            Some("MKC"),
            "line-a",
            &calling_points,
        )
        .await
        .expect("seed a trains row via schedule match");
        mark_train_resolved(&pool, trains_id, "1A23")
            .await
            .expect("mark_train_resolved");

        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'Watford Junction', 'DEPARTURE', 4, 'MKC', NOW())",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        let state = get_public_train_state(&pool, "TEST-PUBLIC-STATE-UID", service_date)
            .await
            .expect("get_public_train_state")
            .expect("row should be found");

        assert_eq!(state.trains_id, trains_id);
        assert_eq!(state.train_uid, "TEST-PUBLIC-STATE-UID");
        assert_eq!(state.origin_crs, Some("EUS".to_string()));
        assert_eq!(state.destination_crs, Some("MKC".to_string()));
        assert_eq!(state.train_id, Some("1A23".to_string()));
        assert_eq!(state.status, Some("en_route".to_string()));
        assert_eq!(
            state.last_reported_location,
            Some("Watford Junction".to_string())
        );
        assert_eq!(state.delay_minutes, Some(4));
        assert_eq!(state.next_calling_point, Some("MKC".to_string()));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids \
                -- --ignored"]
    async fn get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-09".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-09T08:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "BHM"]);

        // One resolved train (has both schedule match and live state)...
        let resolved_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-LINE-TRAINS-RESOLVED",
            service_date,
            "EUS",
            scheduled_departure,
            Some("BHM"),
            "line-a",
            &calling_points,
        )
        .await
        .expect("seed resolved trains row");
        mark_train_resolved(&pool, resolved_id, "1A11")
            .await
            .expect("mark_train_resolved");
        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'Watford Junction', 'DEPARTURE', 2, 'BHM', NOW())",
        )
        .bind(resolved_id)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        // ...and one UID that was requested but has NO trains row at all
        // (a scheduled service TRUST hasn't activated yet) -- must simply
        // be absent from the result, not an error and not a null-filled
        // placeholder row.
        let requested = vec![
            "TEST-LINE-TRAINS-RESOLVED".to_string(),
            "TEST-LINE-TRAINS-UNSEEN".to_string(),
        ];

        let states = get_public_train_states_for_line(&pool, &requested, service_date)
            .await
            .expect("get_public_train_states_for_line");

        assert_eq!(
            states.len(),
            1,
            "only the one UID with a real trains row should come back: {states:?}"
        );
        let state = &states[0];
        assert_eq!(state.train_uid, "TEST-LINE-TRAINS-RESOLVED");
        assert_eq!(state.trains_id, resolved_id);
        assert_eq!(state.train_id, Some("1A11".to_string()));
        assert_eq!(state.status, Some("en_route".to_string()));
        assert_eq!(state.delay_minutes, Some(2));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(resolved_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_states_for_line_returns_empty_for_an_empty_uid_list -- --ignored"]
    async fn get_public_train_states_for_line_returns_empty_for_an_empty_uid_list() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-09".parse().unwrap();

        let states = get_public_train_states_for_line(&pool, &[], service_date)
            .await
            .expect("get_public_train_states_for_line with no uids");

        assert!(
            states.is_empty(),
            "an empty uid list must short-circuit to no rows, not a malformed empty-array SQL query"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                is_known_scheduled_train_is_false_for_an_unpublished_uid -- --ignored"]
    async fn is_known_scheduled_train_is_false_for_an_unpublished_uid() {
        let pool = connect().await;
        let known = is_known_scheduled_train(&pool, "NOSUCHUID", "2026-09-06".parse().unwrap())
            .await
            .expect("is_known_scheduled_train");
        assert!(!known, "a uid never published by CIF must not read as known");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                is_known_scheduled_train_is_true_for_a_published_row -- --ignored"]
    async fn is_known_scheduled_train_is_true_for_a_published_row() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs) \
             VALUES ($1, 'EDB', '12:00:00', 'TEST-SCHED-KNOWN-UID', 'KGX')",
        )
        .bind(service_date)
        .execute(&pool)
        .await
        .expect("seed fixture schedule_destination_departures row");

        let known =
            is_known_scheduled_train(&pool, "TEST-SCHED-KNOWN-UID", service_date)
                .await
                .expect("is_known_scheduled_train");
        assert!(
            known,
            "a uid CIF actually published for this service_date must read as known"
        );

        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-SCHED-KNOWN-UID'",
        )
        .execute(&pool)
        .await
        .ok();
    }
}
