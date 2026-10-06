//! `schedule_services`: what kind of vehicle each CIF schedule is, per
//! `(service_date, uid)` -- see migration `20261006160000_schedule_services.sql`.
//!
//! Buses and ferries are in the CIF timetable (9% of weekday schedules, 25%
//! on Sundays) but TRUST never reports them, so they have no live position,
//! delay or arrival. This module is the one place `api` learns a schedule's
//! [`ServiceMode`]:
//!
//! * the write side, [`replace_for_date`], behind `POST /private/schedule-services`
//!   (`schedule-reference`'s `publish_schedule_services`);
//! * the read side, [`modes_for`]/[`mode_for`], which every public surface
//!   uses to add `serviceMode`/`liveTracking` (see [`ServiceModeFields`]).
//!
//! A schedule with no row is a [`ServiceMode::Train`]: the table may not be
//! published yet for a date, and degrading to "train" is the old behaviour.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize, Serializer};
use sqlx::PgPool;

/// What kind of vehicle a schedule is. The stored spelling is
/// [`Self::as_db_str`] (`replacement_bus`), the wire spelling camelCase
/// (`replacementBus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceMode {
    #[default]
    Train,
    ReplacementBus,
    Bus,
    Ferry,
}

impl ServiceMode {
    /// The `schedule_services.mode` spelling.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::ReplacementBus => "replacement_bus",
            Self::Bus => "bus",
            Self::Ferry => "ferry",
        }
    }

    /// Parses [`Self::as_db_str`]; anything else (there is a CHECK
    /// constraint, so nothing should be) reads as a train.
    pub fn from_db_str(value: &str) -> Self {
        match value {
            "replacement_bus" => Self::ReplacementBus,
            "bus" => Self::Bus,
            "ferry" => Self::Ferry,
            _ => Self::Train,
        }
    }

    /// Classifies from a CIF Train Status alone -- the fallback for a
    /// reader that has the status (the line population carries it) but no
    /// `schedule_services` row. Without the category a permanent `B` bus
    /// cannot be told from a `BBR` replacement one; it reads as a bus.
    pub fn from_train_status(train_status: Option<char>) -> Self {
        Self::from(schedule_query::service_mode(train_status, None))
    }

    /// Whether TRUST reports this service live. Only a train.
    pub const fn live_tracking(self) -> bool {
        matches!(self, Self::Train)
    }

    /// Whether this is a bus or ferry: timetable-only.
    pub const fn is_timetable_only(self) -> bool {
        !self.live_tracking()
    }
}

impl From<schedule_query::ServiceMode> for ServiceMode {
    fn from(mode: schedule_query::ServiceMode) -> Self {
        match mode {
            schedule_query::ServiceMode::Train => Self::Train,
            schedule_query::ServiceMode::ReplacementBus => Self::ReplacementBus,
            schedule_query::ServiceMode::Bus => Self::Bus,
            schedule_query::ServiceMode::Ferry => Self::Ferry,
        }
    }
}

/// The two public fields every surface adds for a schedule: `serviceMode`
/// (`train`/`replacementBus`/`bus`/`ferry`) and `liveTracking` (`false` for
/// anything but a train). One value, so the two can never disagree;
/// `#[serde(flatten)]` it into a response struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServiceModeFields(pub ServiceMode);

impl Serialize for ServiceModeFields {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("serviceMode", &self.0)?;
        map.serialize_entry("liveTracking", &self.0.live_tracking())?;
        map.end()
    }
}

/// Adds `serviceMode`/`liveTracking` to a hand-built JSON object (the
/// `render::*_json` rows). A non-object is left alone.
pub fn annotate_json(value: &mut serde_json::Value, mode: ServiceMode) {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "serviceMode".to_string(),
            serde_json::to_value(mode).unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "liveTracking".to_string(),
            serde_json::Value::Bool(mode.live_tracking()),
        );
    }
}

/// A read model that carries [`ServiceModeFields`] for one schedule.
pub trait HasServiceMode {
    /// The schedule's `(uid, service_date)`, or `None` when it has no uid
    /// yet (a pending pin), which then stays a train.
    fn schedule_key(&self) -> Option<(&str, NaiveDate)>;
    fn set_service_mode(&mut self, mode: ServiceMode);
}

impl HasServiceMode for crate::data::trains::PublicTrainState {
    fn schedule_key(&self) -> Option<(&str, NaiveDate)> {
        Some((self.train_uid.as_str(), self.service_date))
    }

    fn set_service_mode(&mut self, mode: ServiceMode) {
        self.service = ServiceModeFields(mode);
    }
}

impl HasServiceMode for crate::data::train_tracking::TrackedTrainState {
    fn schedule_key(&self) -> Option<(&str, NaiveDate)> {
        self.train_uid
            .as_deref()
            .map(|uid| (uid, self.service_date))
    }

    fn set_service_mode(&mut self, mode: ServiceMode) {
        self.service = ServiceModeFields(mode);
    }
}

impl HasServiceMode for crate::data::train_tracking::TrackedTrainListItem {
    fn schedule_key(&self) -> Option<(&str, NaiveDate)> {
        self.train_uid
            .as_deref()
            .map(|uid| (uid, self.service_date))
    }

    fn set_service_mode(&mut self, mode: ServiceMode) {
        self.service = ServiceModeFields(mode);
    }
}

/// Fills every state's [`ServiceModeFields`] with one query. A database
/// error is logged and leaves them all trains: the label is an
/// enhancement, never a reason to fail the read.
pub async fn attach<T: HasServiceMode>(pool: &PgPool, states: &mut [T]) {
    let pairs: Vec<(String, NaiveDate)> = states
        .iter()
        .filter_map(|state| state.schedule_key())
        .map(|(uid, date)| (uid.to_string(), date))
        .collect();
    let modes = match modes_for_pairs(pool, &pairs).await {
        Ok(modes) => modes,
        Err(err) => {
            tracing::warn!(error = ?err, "could not read service modes");
            return;
        }
    };
    for state in states.iter_mut() {
        let mode = state
            .schedule_key()
            .and_then(|(uid, date)| modes.get(&(uid.to_string(), date)).copied())
            .unwrap_or_default();
        state.set_service_mode(mode);
    }
}

/// [`attach`] for one state, by value.
pub async fn attach_one<T: HasServiceMode>(pool: &PgPool, state: T) -> T {
    let mut states = [state];
    attach(pool, &mut states).await;
    let [state] = states;
    state
}

/// Adds `serviceMode`/`liveTracking` to every rendered departure row in
/// `rows` (each carries its schedule's `uid`), with one lookup for the
/// whole page. A database error is logged and leaves every row a train.
pub async fn annotate_uid_rows(
    pool: &PgPool,
    service_date: NaiveDate,
    rows: &mut [serde_json::Value],
) {
    let mut uids: Vec<String> = rows
        .iter()
        .filter_map(|row| row.get("uid").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();
    uids.sort_unstable();
    uids.dedup();
    let modes = modes_for_or_trains(pool, service_date, &uids).await;
    for row in rows.iter_mut() {
        let mode = row
            .get("uid")
            .and_then(serde_json::Value::as_str)
            .map_or(ServiceMode::Train, |uid| mode_in(&modes, uid));
        annotate_json(row, mode);
    }
}

/// One published `schedule_services` row -- the wire shape of
/// `POST /private/schedule-services` (`snake_case`, like every other
/// `schedule-reference` product).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ScheduleServiceRow {
    pub service_date: NaiveDate,
    pub uid: String,
    /// `train`/`replacement_bus`/`bus`/`ferry`.
    pub mode: String,
    pub train_status: Option<String>,
    pub train_category: Option<String>,
    pub headcode: Option<String>,
    pub rsid: Option<String>,
    pub operator_atoc: Option<String>,
    /// `P`/`O`/`N`.
    pub stp: String,
}

/// Why a publish was refused before touching the table: the caller's data
/// is wrong (a 400), not the database.
#[derive(Debug)]
pub struct InvalidPublish(pub String);

impl std::fmt::Display for InvalidPublish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidPublish {}

/// Checks every row belongs to `service_date` and carries values the
/// table's CHECK constraints accept, so a bad publish is a clear 400 rather
/// than a constraint-violation 500.
fn validate(service_date: NaiveDate, rows: &[ScheduleServiceRow]) -> Result<(), InvalidPublish> {
    for row in rows {
        if row.service_date != service_date {
            return Err(InvalidPublish(format!(
                "row for {} {} is outside the published service_date {service_date}",
                row.uid, row.service_date
            )));
        }
        if !matches!(
            row.mode.as_str(),
            "train" | "replacement_bus" | "bus" | "ferry"
        ) {
            return Err(InvalidPublish(format!(
                "row {} has unknown mode {:?}",
                row.uid, row.mode
            )));
        }
        if !matches!(row.stp.as_str(), "P" | "O" | "N") {
            return Err(InvalidPublish(format!(
                "row {} has unknown stp {:?}",
                row.uid, row.stp
            )));
        }
        if row.uid.is_empty() || row.uid.len() > 16 {
            return Err(InvalidPublish(format!("row has a bad uid {:?}", row.uid)));
        }
        if row
            .train_status
            .as_deref()
            .is_some_and(|s| s.chars().count() != 1)
            || row
                .train_category
                .as_deref()
                .is_some_and(|c| c.chars().count() != 2)
        {
            return Err(InvalidPublish(format!(
                "row {} has a malformed train status or category",
                row.uid
            )));
        }
    }
    Ok(())
}

/// `statement_timeout` for the publish transaction. A day is ~25-30k rows;
/// the upsert and anti-join delete take well under a second.
const PUBLISH_STATEMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Replaces `service_date`'s rows with `rows`, in one transaction: upserts
/// every row (an unchanged row is not rewritten), then deletes the date's
/// rows whose uid the publish did not carry. A reader sees the old day or
/// the new one, never a mix. An empty `rows` clears the date -- the
/// publisher only sends one when the rest of its window has schedules (the
/// same PL-14 guard as the other per-date products). Returns rows inserted
/// or changed.
///
/// Small enough (one row per schedule, not per calling point) that one
/// request carries a whole day, so this needs none of the multi-chunk diff
/// protocol `schedule_calling_points_full` uses.
pub async fn replace_for_date(
    pool: &PgPool,
    service_date: NaiveDate,
    rows: &[ScheduleServiceRow],
) -> Result<u64> {
    validate(service_date, rows)?;

    let uids: Vec<&str> = rows.iter().map(|r| r.uid.as_str()).collect();
    let modes: Vec<&str> = rows.iter().map(|r| r.mode.as_str()).collect();
    let statuses: Vec<Option<&str>> = rows.iter().map(|r| r.train_status.as_deref()).collect();
    let categories: Vec<Option<&str>> = rows.iter().map(|r| r.train_category.as_deref()).collect();
    let headcodes: Vec<Option<&str>> = rows.iter().map(|r| r.headcode.as_deref()).collect();
    let rsids: Vec<Option<&str>> = rows.iter().map(|r| r.rsid.as_deref()).collect();
    let operators: Vec<Option<&str>> = rows.iter().map(|r| r.operator_atoc.as_deref()).collect();
    let stps: Vec<&str> = rows.iter().map(|r| r.stp.as_str()).collect();

    let mut tx = pool.begin().await?;
    common::pg::set_local_statement_timeout(&mut tx, PUBLISH_STATEMENT_TIMEOUT).await?;

    // DISTINCT ON keeps the LAST row per uid (`ORDER BY ... ord DESC`) so a
    // duplicated uid cannot make ON CONFLICT touch one row twice.
    let upserted = sqlx::query(
        "INSERT INTO schedule_services AS s \
            (service_date, uid, mode, train_status, train_category, headcode, rsid, \
             operator_atoc, stp) \
         SELECT DISTINCT ON (uid) $1, uid, mode, train_status, train_category, headcode, rsid, \
                operator_atoc, stp \
         FROM UNNEST($2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[], \
                     $8::text[], $9::text[]) \
              WITH ORDINALITY AS t(uid, mode, train_status, train_category, headcode, rsid, \
                                   operator_atoc, stp, ord) \
         ORDER BY uid, ord DESC \
         ON CONFLICT (service_date, uid) DO UPDATE SET \
            mode = EXCLUDED.mode, \
            train_status = EXCLUDED.train_status, \
            train_category = EXCLUDED.train_category, \
            headcode = EXCLUDED.headcode, \
            rsid = EXCLUDED.rsid, \
            operator_atoc = EXCLUDED.operator_atoc, \
            stp = EXCLUDED.stp \
         WHERE (s.mode, s.train_status, s.train_category, s.headcode, s.rsid, \
                s.operator_atoc, s.stp) \
               IS DISTINCT FROM \
               (EXCLUDED.mode, EXCLUDED.train_status, EXCLUDED.train_category, \
                EXCLUDED.headcode, EXCLUDED.rsid, EXCLUDED.operator_atoc, EXCLUDED.stp)",
    )
    .bind(service_date)
    .bind(&uids)
    .bind(&modes)
    .bind(&statuses)
    .bind(&categories)
    .bind(&headcodes)
    .bind(&rsids)
    .bind(&operators)
    .bind(&stps)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let deleted = sqlx::query(
        "DELETE FROM schedule_services \
         WHERE service_date = $1 AND NOT (uid = ANY($2::text[]))",
    )
    .bind(service_date)
    .bind(&uids)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;
    tracing::debug!(%service_date, upserted, deleted, "published schedule_services");
    Ok(upserted)
}

/// The [`ServiceMode`] of every uid in `uids` that is NOT a train on
/// `service_date`. A uid absent from the map is a train (no row, or a
/// `train` row) -- look it up with [`mode_in`].
pub async fn modes_for(
    pool: &PgPool,
    service_date: NaiveDate,
    uids: &[String],
) -> Result<HashMap<String, ServiceMode>> {
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT uid, mode FROM schedule_services \
         WHERE service_date = $1 AND uid = ANY($2) AND mode <> 'train'",
    )
    .bind(service_date)
    .bind(uids)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(uid, mode)| (uid, ServiceMode::from_db_str(&mode)))
        .collect())
}

/// Every published row's [`ServiceMode`] on `service_date`, `train` rows
/// included -- unlike [`modes_for`], a uid absent from the map has NO row
/// (not published yet), so a caller can tell "a train" from "unknown" and
/// fall back to its own heuristic. Used once per trip-planner graph build
/// (a day's schedules, ~25-30k rows).
pub async fn all_modes_for_date(
    pool: &PgPool,
    service_date: NaiveDate,
) -> Result<HashMap<String, ServiceMode>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT uid, mode FROM schedule_services WHERE service_date = $1")
            .bind(service_date)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(uid, mode)| (uid, ServiceMode::from_db_str(&mode)))
        .collect())
}

/// [`modes_for`] across several service dates at once (a journey's legs).
pub async fn modes_for_pairs(
    pool: &PgPool,
    pairs: &[(String, NaiveDate)],
) -> Result<HashMap<(String, NaiveDate), ServiceMode>> {
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }
    let uids: Vec<&str> = pairs.iter().map(|(uid, _)| uid.as_str()).collect();
    let dates: Vec<NaiveDate> = pairs.iter().map(|(_, date)| *date).collect();
    let rows: Vec<(String, NaiveDate, String)> = sqlx::query_as(
        "SELECT s.uid, s.service_date, s.mode \
         FROM UNNEST($1::text[], $2::date[]) AS p(uid, service_date) \
         JOIN schedule_services s ON s.uid = p.uid AND s.service_date = p.service_date \
         WHERE s.mode <> 'train'",
    )
    .bind(&uids)
    .bind(&dates)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(uid, date, mode)| ((uid, date), ServiceMode::from_db_str(&mode)))
        .collect())
}

/// `uid`'s mode in a [`modes_for`] result.
pub fn mode_in<S: std::hash::BuildHasher>(
    modes: &HashMap<String, ServiceMode, S>,
    uid: &str,
) -> ServiceMode {
    modes.get(uid).copied().unwrap_or_default()
}

/// One schedule's [`ServiceMode`] on `service_date`; a train when there is
/// no row.
pub async fn mode_for(pool: &PgPool, uid: &str, service_date: NaiveDate) -> Result<ServiceMode> {
    let mode: Option<String> = sqlx::query_scalar(
        "SELECT mode FROM schedule_services WHERE service_date = $1 AND uid = $2",
    )
    .bind(service_date)
    .bind(uid)
    .fetch_optional(pool)
    .await?;
    Ok(mode
        .as_deref()
        .map_or(ServiceMode::Train, ServiceMode::from_db_str))
}

/// [`mode_for`] that logs and falls back to a train on a database error,
/// for read paths where the label is an enhancement, never a reason to
/// fail the request.
pub async fn mode_for_or_train(pool: &PgPool, uid: &str, service_date: NaiveDate) -> ServiceMode {
    match mode_for(pool, uid, service_date).await {
        Ok(mode) => mode,
        Err(err) => {
            tracing::warn!(error = ?err, uid, %service_date, "could not read service mode");
            ServiceMode::Train
        }
    }
}

/// [`modes_for`] with the same log-and-fall-back-to-train posture as
/// [`mode_for_or_train`].
pub async fn modes_for_or_trains(
    pool: &PgPool,
    service_date: NaiveDate,
    uids: &[String],
) -> HashMap<String, ServiceMode> {
    match modes_for(pool, service_date, uids).await {
        Ok(modes) => modes,
        Err(err) => {
            tracing::warn!(error = ?err, %service_date, "could not read service modes");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_serialize_mode_and_live_tracking_together() {
        for (mode, wire, live) in [
            (ServiceMode::Train, "train", true),
            (ServiceMode::ReplacementBus, "replacementBus", false),
            (ServiceMode::Bus, "bus", false),
            (ServiceMode::Ferry, "ferry", false),
        ] {
            let json = serde_json::to_value(ServiceModeFields(mode)).unwrap();
            assert_eq!(
                json,
                serde_json::json!({ "serviceMode": wire, "liveTracking": live })
            );
        }
    }

    #[test]
    fn fields_flatten_into_a_struct() {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Row {
            train_uid: &'static str,
            #[serde(flatten)]
            service: ServiceModeFields,
        }
        let json = serde_json::to_value(Row {
            train_uid: "C30818",
            service: ServiceModeFields(ServiceMode::Bus),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "trainUid": "C30818", "serviceMode": "bus", "liveTracking": false })
        );
    }

    #[test]
    fn db_spelling_round_trips() {
        for mode in [
            ServiceMode::Train,
            ServiceMode::ReplacementBus,
            ServiceMode::Bus,
            ServiceMode::Ferry,
        ] {
            assert_eq!(ServiceMode::from_db_str(mode.as_db_str()), mode);
            assert_eq!(mode.as_db_str(), mode_to_query(mode).as_str());
            assert_eq!(ServiceMode::from(mode_to_query(mode)), mode);
        }
        assert_eq!(ServiceMode::from_db_str("tram"), ServiceMode::Train);
    }

    fn mode_to_query(mode: ServiceMode) -> schedule_query::ServiceMode {
        match mode {
            ServiceMode::Train => schedule_query::ServiceMode::Train,
            ServiceMode::ReplacementBus => schedule_query::ServiceMode::ReplacementBus,
            ServiceMode::Bus => schedule_query::ServiceMode::Bus,
            ServiceMode::Ferry => schedule_query::ServiceMode::Ferry,
        }
    }

    #[test]
    fn status_only_fallback() {
        assert_eq!(
            ServiceMode::from_train_status(Some('P')),
            ServiceMode::Train
        );
        assert_eq!(ServiceMode::from_train_status(None), ServiceMode::Train);
        assert_eq!(ServiceMode::from_train_status(Some('B')), ServiceMode::Bus);
        assert_eq!(
            ServiceMode::from_train_status(Some('5')),
            ServiceMode::ReplacementBus
        );
        assert_eq!(
            ServiceMode::from_train_status(Some('S')),
            ServiceMode::Ferry
        );
    }

    #[test]
    fn annotate_adds_both_fields_to_an_object() {
        let mut value = serde_json::json!({ "uid": "C30818" });
        annotate_json(&mut value, ServiceMode::Ferry);
        assert_eq!(value["serviceMode"], "ferry");
        assert_eq!(value["liveTracking"], false);
    }

    fn row(uid: &str, date: NaiveDate) -> ScheduleServiceRow {
        ScheduleServiceRow {
            service_date: date,
            uid: uid.to_string(),
            mode: "bus".to_string(),
            train_status: Some("B".to_string()),
            train_category: Some("BS".to_string()),
            headcode: Some("0B00".to_string()),
            rsid: None,
            operator_atoc: Some("NT".to_string()),
            stp: "P".to_string(),
        }
    }

    #[test]
    fn validate_rejects_a_row_for_another_date_and_bad_values() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert!(validate(date, &[row("C30818", date)]).is_ok());
        let other = date.succ_opt().unwrap();
        assert!(validate(date, &[row("C30818", other)]).is_err());
        let mut bad_mode = row("C30818", date);
        bad_mode.mode = "tram".to_string();
        assert!(validate(date, &[bad_mode]).is_err());
        let mut bad_stp = row("C30818", date);
        bad_stp.stp = "C".to_string();
        assert!(validate(date, &[bad_stp]).is_err());
        let mut bad_category = row("C30818", date);
        bad_category.train_category = Some("B".to_string());
        assert!(validate(date, &[bad_category]).is_err());
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    fn row(uid: &str, date: NaiveDate, mode: &str, status: &str) -> ScheduleServiceRow {
        ScheduleServiceRow {
            service_date: date,
            uid: uid.to_string(),
            mode: mode.to_string(),
            train_status: Some(status.to_string()),
            train_category: None,
            headcode: None,
            rsid: None,
            operator_atoc: None,
            stp: "P".to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_services -- --ignored --test-threads=1`"]
    async fn replace_for_date_upserts_deletes_missing_and_leaves_other_dates() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2031, 3, 3).unwrap();
        let other = NaiveDate::from_ymd_opt(2031, 3, 4).unwrap();
        sqlx::query("DELETE FROM schedule_services WHERE service_date IN ($1, $2)")
            .bind(date)
            .bind(other)
            .execute(&pool)
            .await
            .unwrap();

        replace_for_date(&pool, other, &[row("TSS0001", other, "bus", "B")])
            .await
            .unwrap();
        let first = replace_for_date(
            &pool,
            date,
            &[
                row("TSS0001", date, "train", "P"),
                row("TSS0002", date, "bus", "B"),
                row("TSS0003", date, "ferry", "S"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(first, 3);

        // Re-publish: one unchanged, one changed, one dropped.
        let second = replace_for_date(
            &pool,
            date,
            &[
                row("TSS0001", date, "train", "P"),
                row("TSS0002", date, "replacement_bus", "5"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(second, 1, "only the changed row is rewritten");

        let uids = vec![
            "TSS0001".to_string(),
            "TSS0002".to_string(),
            "TSS0003".to_string(),
        ];
        let modes = modes_for(&pool, date, &uids).await.unwrap();
        assert_eq!(mode_in(&modes, "TSS0001"), ServiceMode::Train);
        assert_eq!(mode_in(&modes, "TSS0002"), ServiceMode::ReplacementBus);
        assert_eq!(
            mode_in(&modes, "TSS0003"),
            ServiceMode::Train,
            "a dropped row reads as a train"
        );
        assert_eq!(
            mode_for(&pool, "TSS0001", other).await.unwrap(),
            ServiceMode::Bus,
            "another date's rows are untouched"
        );
        let all = all_modes_for_date(&pool, date).await.unwrap();
        assert_eq!(
            all,
            HashMap::from([
                ("TSS0001".to_string(), ServiceMode::Train),
                ("TSS0002".to_string(), ServiceMode::ReplacementBus),
            ]),
            "train rows are included; a dropped row is absent"
        );
        let pairs = vec![
            ("TSS0002".to_string(), date),
            ("TSS0001".to_string(), other),
            ("TSS0001".to_string(), date),
        ];
        let by_pair = modes_for_pairs(&pool, &pairs).await.unwrap();
        assert_eq!(by_pair.len(), 2);
        assert_eq!(
            by_pair[&("TSS0002".to_string(), date)],
            ServiceMode::ReplacementBus
        );
        assert_eq!(by_pair[&("TSS0001".to_string(), other)], ServiceMode::Bus);

        // An empty publish clears the date.
        replace_for_date(&pool, date, &[]).await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schedule_services WHERE service_date = $1")
                .bind(date)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);

        sqlx::query("DELETE FROM schedule_services WHERE service_date IN ($1, $2)")
            .bind(date)
            .bind(other)
            .execute(&pool)
            .await
            .unwrap();
    }
}
