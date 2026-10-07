//! The live and origin fields the CIF schedule lists carry on top of their
//! timetable fields, so a client can show them the way the line view
//! (`GET /public/lines/{id}/trains?view=summary`) does (2026-10-07).
//!
//! Used by `GET /public/trains/search` (`routes::trains`) and
//! `GET /public/stations/{crs}/schedule-departures` (`routes::departures`).
//! Every helper here works on one page of already-rendered rows and reads
//! the database in batches (one query per kind of lookup, never one per
//! row), so a page costs the same few round trips however many rows it
//! has. The fields are additive: no existing key on a row is touched.

use std::collections::HashMap;

use serde_json::Value;
use sqlx::PgPool;

use crate::routes::line_trains_summary::live_summaries;

/// The distinct `uid`s of `rows`, sorted.
fn row_uids(rows: &[Value]) -> Vec<String> {
    let mut uids: Vec<String> = rows
        .iter()
        .filter_map(|row| row.get("uid").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    uids.sort_unstable();
    uids.dedup();
    uids
}

/// Sets `live` on every row: the line summary's compact live status
/// (`status`, `delayMinutes`, `delayProvisional`, `cancelled`,
/// `lastReportedLocation`, the same `LiveSummary` object) for the row's
/// `(uid, service_date)`, or `null` when that train has no live state.
///
/// No live state is the common case: a train TRUST has not activated yet,
/// or one nothing has looked up. A failed read is logged and also renders
/// `null`, so live status can never break the timetable list around it,
/// the same posture as `schedule_services::annotate_uid_rows`.
pub(crate) async fn attach_live(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    rows: &mut [Value],
) {
    let uids = row_uids(rows);
    let live = if uids.is_empty() {
        HashMap::new()
    } else {
        match live_summaries(pool, &uids, service_date).await {
            Ok(live) => live,
            Err(err) => {
                tracing::warn!(error = ?err, %service_date, "could not read live state for schedule rows");
                HashMap::new()
            }
        }
    };
    for row in rows.iter_mut() {
        let summary = row
            .get("uid")
            .and_then(Value::as_str)
            .and_then(|uid| live.get(uid))
            .and_then(|summary| serde_json::to_value(summary).ok())
            .unwrap_or(Value::Null);
        if let Some(object) = row.as_object_mut() {
            object.insert("live".to_string(), summary);
        }
    }
}

/// Sets `originName` on every row from its `originCrs` through `names` (the
/// same batched `crs -> name` map the route already reads for
/// `destinationName`); `null` when the row has no `originCrs` or the code
/// has no station name.
pub(crate) fn attach_origin_names(rows: &mut [Value], names: &HashMap<String, String>) {
    for row in rows.iter_mut() {
        let name = row
            .get("originCrs")
            .and_then(Value::as_str)
            .and_then(|crs| names.get(&crs.to_uppercase()))
            .map_or(Value::Null, |name| Value::String(name.clone()));
        if let Some(object) = row.as_object_mut() {
            object.insert("originName".to_string(), name);
        }
    }
}

/// Each `uid`'s true origin CRS (the schedule's first calling point) on
/// `service_date`, read from `schedule_destination_departures` in one query
/// over the `(train_uid, service_date, scheduled)` index. A uid whose
/// origin never resolved to a CRS, or that has no rows that day, is absent.
pub(crate) async fn true_origins(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    uids: &[String],
) -> anyhow::Result<HashMap<String, String>> {
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT ON (train_uid) train_uid, true_origin_crs \
         FROM schedule_destination_departures \
         WHERE train_uid = ANY($1) AND service_date = $2 \
           AND true_origin_crs IS NOT NULL \
         ORDER BY train_uid",
    )
    .bind(uids)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// [`true_origins`] for the uids of `rows`, logging and returning an empty
/// map on a failed read (an origin is display-only; the list still loads).
pub(crate) async fn true_origins_for_rows(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    rows: &[Value],
) -> HashMap<String, String> {
    match true_origins(pool, service_date, &row_uids(rows)).await {
        Ok(origins) => origins,
        Err(err) => {
            tracing::warn!(error = ?err, %service_date, "could not read schedule origins");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn origin_name_is_looked_up_case_insensitively_and_null_when_unknown() {
        let mut rows = vec![
            json!({"uid": "A1", "originCrs": "pad"}),
            json!({"uid": "A2", "originCrs": "ZZZ"}),
            json!({"uid": "A3", "originCrs": null}),
        ];
        let names = HashMap::from([("PAD".to_string(), "London Paddington".to_string())]);
        attach_origin_names(&mut rows, &names);
        assert_eq!(rows[0]["originName"], "London Paddington");
        assert!(rows[1]["originName"].is_null());
        assert!(rows[2]["originName"].is_null());
        assert!(rows[2].as_object().unwrap().contains_key("originName"));
    }

    #[test]
    fn row_uids_are_distinct_and_skip_rows_without_one() {
        let rows = vec![
            json!({"uid": "B"}),
            json!({"uid": "A"}),
            json!({"uid": "B"}),
            json!({"uid": null}),
        ];
        assert_eq!(row_uids(&rows), vec!["A".to_string(), "B".to_string()]);
    }
}
