//! Identity primitives for the shared `trains` table
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md).
//! `find_or_create_train` is the one idempotent upsert every dual-write
//! path in this plan funnels through -- Step B's own one-off backfill uses
//! the exact same `ON CONFLICT ... DO UPDATE ... RETURNING id` shape.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

// Moved to ds_store::trains (ingest architecture plan 1A.4)
pub use ds_store::trains::{
    bind_subscription_unless_other_train, destination_crs_for_train,
    destination_crs_for_trains_batch, find_or_create_train,
    find_or_create_train_with_schedule_match, find_or_create_trains_batch, mark_train_resolved,
    mark_trains_resolved_batch,
};

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
    /// TRUST's 10-character movement-feed train id (`trains.train_id`, e.g.
    /// `"721S00MF25"`) -- set on TRUST activation. NOT the headcode, even
    /// though it embeds one; see `headcode` below for that.
    pub train_id: Option<String>,
    /// The CIF `BS` Train Identity -- the 4-character signalling headcode
    /// (e.g. `"1S00"`) -- read from the published schedule
    /// (`schedule_destination_departures.headcode`), not from TRUST. `None`
    /// when no row carries one or the rows disagree.
    pub headcode: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    /// The delay at the passenger's own stop, against the PUBLIC timetable
    /// (`data::stop_delay`, design doc §9 decision 2): a public train page has no stop of the passenger's own, so this is the delay at the latest call the train reported at, else (nothing reported at a call yet) TRUST's running delay with `delay_basis: working`. `None` until
    /// known.
    pub delay_minutes: Option<i32>,
    /// What `delay_minutes` was measured against (`public`, `publicSchedule`
    /// or `working`, see `common::public_delay::DelayBasis`); `None` exactly
    /// when `delay_minutes` is.
    #[sqlx(skip)]
    pub delay_basis: Option<crate::data::stop_delay::DelayBasis>,
    /// `true` while `delay_minutes` is a forecast (the train has not reported
    /// at that stop yet).
    #[sqlx(skip)]
    pub delay_provisional: bool,
    /// TRUST's running delay against the working timetable
    /// (`train_current_state.delay_minutes`), internal: the input to the
    /// per-stop estimates and to the forecast above. Never serialized.
    #[serde(skip_serializing)]
    pub working_delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
    /// The shared row's own captured Darwin skip snapshot
    /// (`trains.skipped_stations`) -- internal plumbing for
    /// `routes::train::attach_journey_stops_public`'s
    /// `journey::build_journey_stops` call, never sent to the frontend
    /// (which already gets this signal per-stop, on each `JourneyStop`'s
    /// own `skipSource`, rather than as a second copy of the raw CRS list
    /// here). `#[serde(skip_serializing)]`, same posture as
    /// `TrackedTrainState::trains_id`'s own doc comment for why an
    /// internal-only field on an otherwise-public struct stays off the
    /// wire.
    #[serde(skip_serializing)]
    pub skipped_stations: Vec<String>,
    /// The shared row's own captured origin-platform snapshot
    /// (`trains.platform`/`planned_platform`) -- same internal-plumbing
    /// posture as `skipped_stations` immediately above.
    #[serde(skip_serializing)]
    pub platform: Option<String>,
    #[serde(skip_serializing)]
    pub planned_platform: Option<String>,
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
    /// See `train_tracking::TrackedTrainState::may_have_arrived`'s own doc
    /// comment -- same contract, populated the same "read row, then
    /// overlay" way by `routes::train::attach_journey_stops_public`.
    #[sqlx(skip)]
    pub may_have_arrived: bool,
    /// The operating company's ATOC code (for example `"SW"`), from the CIF
    /// schedule. Serialized as `operatorCode`. Filled after the read by
    /// `data::train_operator`, hence `#[sqlx(skip)]`. `None` when no single
    /// code is known; see that module's doc for when that happens.
    #[sqlx(skip)]
    pub operator_code: Option<String>,
    /// The `tocs` display name for `operator_code` (for example
    /// `"South Western Railway"`). Serialized as `operatorName`. `None` when
    /// `operator_code` is `None` or the code has no `tocs` row.
    #[sqlx(skip)]
    pub operator_name: Option<String>,
    /// Whether the train is cancelled: `status == "cancelled"`. Filled
    /// after the read by `data::train_reasons`, like every field below.
    #[sqlx(skip)]
    pub cancelled: bool,
    /// The TRUST cancellation reason code (e.g. `"TG"`), only while
    /// `cancelled` is true.
    #[sqlx(skip)]
    pub cancel_reason_code: Option<String>,
    /// `cancel_reason_code`'s delay attribution glossary text (e.g.
    /// `"Driver"`). `None` for a code the glossary lacks or a system code
    /// (`PD`, `ZW`). There is no delay-reason equivalent: TRUST carries none.
    #[sqlx(skip)]
    pub cancel_reason: Option<String>,
    /// The TRUST change-of-origin reason code, when the train's origin was
    /// changed.
    #[sqlx(skip)]
    pub change_of_origin_reason_code: Option<String>,
    /// Its glossary text, under the same rules as `cancel_reason`.
    #[sqlx(skip)]
    pub change_of_origin_reason: Option<String>,
    /// `serviceMode` (`train`/`replacementBus`/`bus`/`ferry`) and
    /// `liveTracking` (`false` for a bus or ferry, which TRUST never
    /// reports), from `schedule_services`; filled after the read by
    /// `data::schedule_services::attach`. A train when unknown.
    #[sqlx(skip)]
    #[serde(flatten)]
    pub service: crate::data::schedule_services::ServiceModeFields,
}

/// Whether `(train_uid, service_date)` is a real, CIF-published scheduled
/// train, per `schedule_destination_departures` -- the same product the
/// `/trains` search page itself reads
/// (`queries::search_schedule_destination_departures`). The sole caller is
/// `routes::train::get_by_uid_and_date`'s schedule-only view: a GET must be
/// able to show a train a search result actually pointed at (see that
/// route's own doc comment), but never invent one for an arbitrary string
/// someone puts in the URL -- this is the gate that tells those two cases
/// apart. (Until 2026-10-06 that view was a `trains` row the GET created.)
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
                (SELECT CASE WHEN COUNT(DISTINCT sdd.headcode) = 1 THEN MIN(sdd.headcode) END \
                   FROM schedule_destination_departures sdd \
                  WHERE sdd.train_uid = tr.train_uid AND sdd.service_date = tr.service_date) AS headcode, \
                tr.skipped_stations, tr.platform, tr.planned_platform, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.delay_minutes AS working_delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs)::bpchar \
         WHERE tr.train_uid = $1 AND tr.service_date = $2",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    let mut rows: Vec<PublicTrainState> = row.into_iter().collect();
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    crate::data::schedule_services::attach(pool, &mut rows).await;
    Ok(rows.pop())
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
                (SELECT CASE WHEN COUNT(DISTINCT sdd.headcode) = 1 THEN MIN(sdd.headcode) END \
                   FROM schedule_destination_departures sdd \
                  WHERE sdd.train_uid = tr.train_uid AND sdd.service_date = tr.service_date) AS headcode, \
                tr.skipped_stations, tr.platform, tr.planned_platform, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.delay_minutes AS working_delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs)::bpchar \
         WHERE tr.train_uid = ANY($1) AND tr.service_date = $2",
    )
    .bind(train_uids)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    let mut rows = rows;
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    crate::data::schedule_services::attach(pool, &mut rows).await;
    Ok(rows)
}

impl crate::data::stop_delay::PublicDelayFields for PublicTrainState {
    /// No stop of the passenger's own: the latest call.
    fn delay_target(&self) -> Option<crate::data::stop_delay::StopDelayTarget> {
        crate::data::stop_delay::target(
            Some(self.trains_id),
            Some(&self.train_uid),
            self.service_date,
            None,
            self.working_delay_minutes,
        )
    }

    fn set_public_delay(&mut self, delay: Option<crate::data::stop_delay::StopDelay>) {
        (self.delay_minutes, self.delay_basis, self.delay_provisional) =
            crate::data::stop_delay::split(delay);
    }
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
                get_public_train_state_returns_none_for_no_matching_row -- --ignored --test-threads=1`"]
    async fn get_public_train_state_returns_none_for_no_matching_row() {
        let pool = connect().await;
        let result = get_public_train_state(&pool, "NOSUCHUID", "2026-09-06".parse().unwrap())
            .await
            .expect("get_public_train_state");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_state_reads_the_shared_row_and_its_current_state -- --ignored --test-threads=1`"]
    async fn get_public_train_state_reads_the_shared_row_and_its_current_state() {
        let pool = connect().await;
        let service_date: NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: DateTime<Utc> = "2026-09-06T12:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "MKC"]);

        let trains_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-PUBLIC-STATE-UID",
            service_date,
            Some("EUS"),
            Some(scheduled_departure),
            Some("MKC"),
            "line-a",
            &calling_points,
            &[],
            None,
            None,
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
        // No schedule_destination_departures rows for this uid.
        assert_eq!(state.headcode, None);
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
                get_public_train_state_reads_the_cif_headcode_not_the_trust_train_id -- --ignored --test-threads=1`"]
    async fn get_public_train_state_reads_the_cif_headcode_not_the_trust_train_id() {
        let pool = connect().await;
        let service_date: NaiveDate = "2026-09-07".parse().unwrap();
        let uid = "TEST-PUBLIC-HEADCODE-UID";
        let trains_id = find_or_create_train(&pool, uid, service_date)
            .await
            .expect("seed a trains row");
        // A real-shaped TRUST 10-character train id -- NOT the headcode.
        mark_train_resolved(&pool, trains_id, "721S00MF07")
            .await
            .expect("mark_train_resolved");
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, headcode) \
             VALUES ($1, 'EDB', '12:00:00', $2, 'KGX', '1S00'), \
                    ($1, 'EDB', '13:00:00', $2, 'YRK', '1S00')",
        )
        .bind(service_date)
        .bind(uid)
        .execute(&pool)
        .await
        .expect("seed schedule_destination_departures rows");

        let state = get_public_train_state(&pool, uid, service_date)
            .await
            .expect("get_public_train_state")
            .expect("row should be found");
        assert_eq!(state.train_id.as_deref(), Some("721S00MF07"));
        assert_eq!(state.headcode.as_deref(), Some("1S00"));
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["headcode"], "1S00");
        assert_eq!(json["trainId"], "721S00MF07");

        // Rows that disagree are "not known", never a guess.
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, headcode) \
             VALUES ($1, 'EDB', '14:00:00', $2, 'NCL', '1S01')",
        )
        .bind(service_date)
        .bind(uid)
        .execute(&pool)
        .await
        .expect("seed a conflicting row");
        let state = get_public_train_state(&pool, uid, service_date)
            .await
            .expect("get_public_train_state")
            .expect("row should be found");
        assert_eq!(state.headcode, None);
        let json = serde_json::to_value(&state).unwrap();
        assert!(json.as_object().unwrap().contains_key("headcode") && json["headcode"].is_null());

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = $1")
            .bind(uid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids \
                -- --ignored --test-threads=1`"]
    async fn get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids() {
        let pool = connect().await;
        let service_date: NaiveDate = "2026-09-09".parse().unwrap();
        let scheduled_departure: DateTime<Utc> = "2026-09-09T08:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "BHM"]);

        // One resolved train (has both schedule match and live state)...
        let resolved_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-LINE-TRAINS-RESOLVED",
            service_date,
            Some("EUS"),
            Some(scheduled_departure),
            Some("BHM"),
            "line-a",
            &calling_points,
            &[],
            None,
            None,
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
                get_public_train_states_for_line_returns_empty_for_an_empty_uid_list -- --ignored --test-threads=1`"]
    async fn get_public_train_states_for_line_returns_empty_for_an_empty_uid_list() {
        let pool = connect().await;
        let service_date: NaiveDate = "2026-09-09".parse().unwrap();

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
                is_known_scheduled_train_is_false_for_an_unpublished_uid -- --ignored --test-threads=1`"]
    async fn is_known_scheduled_train_is_false_for_an_unpublished_uid() {
        let pool = connect().await;
        let known = is_known_scheduled_train(&pool, "NOSUCHUID", "2026-09-06".parse().unwrap())
            .await
            .expect("is_known_scheduled_train");
        assert!(
            !known,
            "a uid never published by CIF must not read as known"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                is_known_scheduled_train_is_true_for_a_published_row -- --ignored --test-threads=1`"]
    async fn is_known_scheduled_train_is_true_for_a_published_row() {
        let pool = connect().await;
        let service_date: NaiveDate = "2026-09-06".parse().unwrap();

        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs) \
             VALUES ($1, 'EDB', '12:00:00', 'TEST-SCHED-KNOWN-UID', 'KGX')",
        )
        .bind(service_date)
        .execute(&pool)
        .await
        .expect("seed fixture schedule_destination_departures row");

        let known = is_known_scheduled_train(&pool, "TEST-SCHED-KNOWN-UID", service_date)
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
