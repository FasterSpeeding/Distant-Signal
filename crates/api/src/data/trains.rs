//! Identity primitives for the shared `trains` table
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md).
//! `find_or_create_train` is the one idempotent upsert every dual-write
//! path in this plan funnels through -- Step B's own one-off backfill uses
//! the exact same `ON CONFLICT ... DO UPDATE ... RETURNING id` shape.

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

// Step B's one-off backfill job (`backfill_trains_id_for_resolved_rows`,
// which used to live here) already ran, exactly once, against every real
// environment -- Task 22's own Step 1 dry-run count of 0 confirms nothing
// was left for it to do. Its own `SELECT id, train_uid, service_date FROM
// tracked_trains WHERE train_uid IS NOT NULL ...` read a column
// (`tracked_trains.train_uid`) Task 22's migration drops entirely, so it
// can never run again; removed here, alongside its own one-off
// `run_step_b_backfill_of_existing_resolved_rows` test below, rather than
// left as permanently-broken dead code.

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
}
