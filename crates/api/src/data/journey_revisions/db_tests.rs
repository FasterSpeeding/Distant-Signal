//! Revision writes against a live database (`DATABASE_URL`), on a
//! synthetic date with their own trains, stations and user.

use super::*;
use crate::data::journeys;
use crate::test_support::{FixtureCleanup, assert_synthetic_date};
use sqlx::postgres::PgPoolOptions;

const USER: &str = "revision-test-user";

fn date() -> NaiveDate {
    let date = NaiveDate::from_ymd_opt(2099, 5, 4).expect("valid date");
    assert_synthetic_date(date);
    date
}

async fn connect() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("connect to postgres")
}

async fn cleanup(pool: &PgPool) -> FixtureCleanup {
    FixtureCleanup::new(
        pool,
        [
            format!("DELETE FROM journeys WHERE user_id = '{USER}'"),
            format!("DELETE FROM train_subscriptions WHERE user_id = '{USER}'"),
            format!("DELETE FROM users WHERE id = '{USER}'"),
            "DELETE FROM trains WHERE service_date = '2099-05-04' \
             AND train_uid IN ('RV0001', 'RV0002')"
                .to_string(),
            "DELETE FROM schedule_calling_points_full WHERE service_date = '2099-05-04' \
             AND uid IN ('RV0001', 'RV0002')"
                .to_string(),
            "DELETE FROM tiploc_crs WHERE tiploc IN ('RVTSTA', 'RVTSTB')".to_string(),
        ],
    )
    .await
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(sql).execute(pool).await.expect(sql);
}

/// Two trains from ZRA (RVTSTA) to ZRB (RVTSTB): RV0001 10:00-11:00 and
/// RV0002 10:30-11:30, public times.
async fn seed(pool: &PgPool) -> (i64, i64) {
    exec(pool, &format!("INSERT INTO users (id) VALUES ('{USER}')")).await;
    exec(
        pool,
        "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) VALUES \
         ('RVTSTA', 'ZRA', 'Revision A', 'RVA', 1), ('RVTSTB', 'ZRB', 'Revision B', 'RVB', 1)",
    )
    .await;
    for (uid, dep, arr) in [("RV0001", "10:00", "11:00"), ("RV0002", "10:30", "11:30")] {
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_calling_points_full \
                     (service_date, uid, seq, tiploc, kind, booked_departure, public_departure, \
                      booked_arrival, public_arrival, day_offset) VALUES \
                 ('2099-05-04', '{uid}', 1, 'RVTSTA', 'origin', '{dep}', '{dep}', NULL, NULL, 0), \
                 ('2099-05-04', '{uid}', 2, 'RVTSTB', 'terminate', NULL, NULL, '{arr}', '{arr}', 0)"
            ),
        )
        .await;
    }
    let one = ds_store::trains::find_or_create_train(pool, "RV0001", date())
        .await
        .expect("train 1");
    let two = ds_store::trains::find_or_create_train(pool, "RV0002", date())
        .await
        .expect("train 2");
    (one, two)
}

async fn create(pool: &PgPool, trains_id: i64) -> (i64, i64) {
    let mut tx = pool.begin().await.expect("begin");
    let (journey_id, legs) = journeys::create_journey_from_itinerary(
        &mut tx,
        USER,
        None,
        "web",
        &[journeys::ItineraryLeg::Train {
            trains_id,
            service_date: date(),
            origin_crs: Some("ZRA".to_string()),
            destination_crs: Some("ZRB".to_string()),
        }],
    )
    .await
    .expect("create journey");
    tx.commit().await.expect("commit");
    (journey_id, legs[0].0)
}

fn london(hour: u32, minute: u32) -> DateTime<Utc> {
    use chrono::TimeZone;
    chrono_tz::Europe::London
        .from_local_datetime(&date().and_hms_opt(hour, minute, 0).expect("time"))
        .single()
        .expect("unambiguous")
        .with_timezone(&Utc)
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn creating_a_journey_writes_the_booked_plan_and_a_change_before_travel_refreshes_it() {
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    let (one, two) = seed(&pool).await;
    let (journey_id, leg_id) = create(&pool, one).await;

    let revisions = list_revisions(&pool, journey_id).await.expect("list");
    assert_eq!(revisions.len(), 1);
    let booked = &revisions[0];
    assert_eq!(
        (
            booked.revision_no,
            booked.reason.as_str(),
            booked.is_compensation_baseline
        ),
        (1, "booked", true)
    );
    assert_eq!(booked.legs.len(), 1);
    let leg = &booked.legs[0];
    assert_eq!(leg.train_uid.as_deref(), Some("RV0001"));
    assert_eq!(leg.source_leg_id, Some(leg_id));
    assert_eq!(
        (leg.booked_departure, leg.booked_arrival),
        (Some(london(10, 0)), Some(london(11, 0)))
    );

    // Change train before travel: still one revision, now the new train.
    journeys::create_subscription_and_bind_leg(&pool, journey_id, leg_id, USER, two)
        .await
        .expect("change train")
        .expect("bound");
    let revisions = list_revisions(&pool, journey_id).await.expect("list");
    assert_eq!(revisions.len(), 1);
    let leg = &revisions[0].legs[0];
    assert_eq!(leg.train_uid.as_deref(), Some("RV0002"));
    assert_eq!(leg.booked_departure, Some(london(10, 30)));
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_refresh_keeps_the_booked_times_of_an_unchanged_leg() {
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    let (one, _) = seed(&pool).await;
    let (journey_id, _) = create(&pool, one).await;
    // The timetable moves after booking; a planning edit refreshes.
    exec(
        &pool,
        "UPDATE schedule_calling_points_full SET public_departure = '10:05' \
         WHERE service_date = '2099-05-04' AND uid = 'RV0001' AND seq = 1",
    )
    .await;
    let mut conn = pool.acquire().await.expect("acquire");
    record_plan_change(&mut conn, journey_id, london(8, 0))
        .await
        .expect("record");
    drop(conn);
    let revisions = list_revisions(&pool, journey_id).await.expect("list");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].legs[0].booked_departure, Some(london(10, 0)));
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_change_after_the_journey_starts_appends_a_revision_and_keeps_the_booked_one() {
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    let (one, two) = seed(&pool).await;
    let (journey_id, leg_id) = create(&pool, one).await;

    // At 10:10 the first train has left: the change is a replacement.
    let mut tx = pool.begin().await.expect("begin");
    assert!(
        !still_planning(&mut tx, journey_id, london(10, 10))
            .await
            .expect("planning")
    );
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(&mut *tx, two, USER)
            .await
            .expect("subscribe");
    sqlx::query("UPDATE journey_legs SET train_subscription_id = $1 WHERE id = $2")
        .bind(tracking_id)
        .bind(leg_id)
        .execute(&mut *tx)
        .await
        .expect("rebind");
    record_plan_change(&mut tx, journey_id, london(10, 10))
        .await
        .expect("record");
    // Recording an unchanged plan again writes nothing.
    record_plan_change(&mut tx, journey_id, london(10, 11))
        .await
        .expect("record again");
    tx.commit().await.expect("commit");

    let revisions = list_revisions(&pool, journey_id).await.expect("list");
    assert_eq!(revisions.len(), 2, "{revisions:?}");
    let (latest, booked) = (&revisions[0], &revisions[1]);
    assert_eq!(
        (
            latest.revision_no,
            latest.reason.as_str(),
            latest.cause.as_deref()
        ),
        (2, "replace", Some("user_choice"))
    );
    assert_eq!(latest.legs[0].train_uid.as_deref(), Some("RV0002"));
    // The booked plan is kept; its leg lives on in revision 2 (same leg
    // id), so it stays planned. A chosen change moves the baseline.
    assert_eq!(booked.legs[0].train_uid.as_deref(), Some("RV0001"));
    assert_eq!(booked.legs[0].fate, "planned");
    assert!(latest.is_compensation_baseline);
    assert!(!booked.is_compensation_baseline);

    let summary = summary(&pool, journey_id).await.expect("summary");
    assert_eq!(summary.revision_count, 2);
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn the_backfill_writes_revision_one_once() {
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    let (one, _) = seed(&pool).await;
    let (journey_id, _) = create(&pool, one).await;
    exec(
        &pool,
        &format!("DELETE FROM journey_revisions WHERE journey_id = {journey_id}"),
    )
    .await;
    let backfill = include_str!(
        "../../../../ds-store/migrations/20261023090500_journey_revisions_backfill.sql"
    );
    for _ in 0..2 {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::raw_sql(backfill)
            .execute(&mut *tx)
            .await
            .expect("backfill");
        tx.commit().await.expect("commit");
    }
    let revisions = list_revisions(&pool, journey_id).await.expect("list");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].reason, "booked");
    assert_eq!(revisions[0].actor, "system");
    assert_eq!(revisions[0].legs[0].booked_arrival, Some(london(11, 0)));
}
