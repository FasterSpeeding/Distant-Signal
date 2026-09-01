//! `/public/freshness`: how fresh the five data sources feeding the status
//! API are (stations reference data, TOC reference data, the raw incidents
//! feed, the TfL line-status feed, and the CIF SCHEDULE feed pushed by
//! `schedule-ingest`). Unauthenticated, read-only — same `public_router()`
//! pattern as `reference.rs`. Reuses the same `last_*_fetch` queries the
//! private poller-startup endpoints already call
//! (`crates/api/src/routes/ingest.rs`) — this is a public read of the same
//! underlying data, just aimed at the frontend instead of poller backoff.
//! Station-samples is deliberately omitted: it's per-station polling data,
//! not one of the five sources this endpoint reports on.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/freshness", axum::routing::get(get_freshness))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DataFreshness {
    pub stations: Option<DateTime<Utc>>,
    pub tocs: Option<DateTime<Utc>>,
    pub incidents: Option<DateTime<Utc>>,
    /// When TfL line status last landed. Unlike its three siblings this is
    /// not a poller-fed raw table but the `computed_at` of the TfL-owned
    /// `line_status` rows themselves — for this source, ingest and
    /// computation are the same event.
    pub tfl: Option<DateTime<Utc>>,
    /// When a CIF SCHEDULE feed delivery was last recorded by
    /// `schedule-ingest`'s push to `/private/schedule-feed-ingests`. Its own
    /// new source, not previously reported by this endpoint.
    pub schedule_feed: Option<DateTime<Utc>>,
}

async fn get_freshness(State(app): State<App>) -> Result<Json<DataFreshness>, (StatusCode, String)> {
    let (stations, tocs, incidents, tfl, schedule_feed) = tokio::try_join!(
        queries::last_stations_fetch(&app.database),
        queries::last_tocs_fetch(&app.database),
        queries::last_incidents_fetch(&app.database),
        queries::last_tfl_line_status_fetch(&app.database),
        queries::last_schedule_feed_fetch(&app.database),
    )
    .map_err(internal_error)?;
    Ok(Json(DataFreshness { stations, tocs, incidents, tfl, schedule_feed }))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "data freshness query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn serializes_missing_data_as_null() {
        let freshness =
            DataFreshness { stations: None, tocs: None, incidents: None, tfl: None, schedule_feed: None };
        let json = serde_json::to_value(&freshness).unwrap();
        assert!(json["stations"].is_null());
        assert!(json["tocs"].is_null());
        assert!(json["incidents"].is_null());
        assert!(json["tfl"].is_null());
        assert!(json["schedule_feed"].is_null());
    }

    #[test]
    fn round_trips_a_present_timestamp() {
        let ts = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        let freshness = DataFreshness {
            stations: Some(ts),
            tocs: None,
            incidents: None,
            tfl: None,
            schedule_feed: Some(ts),
        };
        let json = serde_json::to_value(&freshness).unwrap();
        let roundtripped: DateTime<Utc> = json["stations"].as_str().unwrap().parse().unwrap();
        assert_eq!(roundtripped, ts);
        let schedule_feed_roundtripped: DateTime<Utc> = json["schedule_feed"].as_str().unwrap().parse().unwrap();
        assert_eq!(schedule_feed_roundtripped, ts);
    }
}
