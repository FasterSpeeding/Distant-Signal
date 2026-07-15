//! `/public/freshness`: how fresh the three data sources feeding the
//! aggregator are (stations reference data, TOC reference data, the raw
//! incidents feed). Unauthenticated, read-only — same `public_router()`
//! pattern as `reference.rs`. Reuses the same `last_*_fetch` queries the
//! private poller-startup endpoints already call
//! (`crates/api/src/routes/ingest.rs`) — this is a public read of the same
//! underlying data, just aimed at the frontend instead of poller backoff.
//! Station-samples is deliberately omitted: it's per-station polling data,
//! not one of the three sources this endpoint reports on.

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
}

async fn get_freshness(State(app): State<App>) -> Result<Json<DataFreshness>, (StatusCode, String)> {
    let (stations, tocs, incidents) = tokio::try_join!(
        queries::last_stations_fetch(&app.database),
        queries::last_tocs_fetch(&app.database),
        queries::last_incidents_fetch(&app.database),
    )
    .map_err(internal_error)?;
    Ok(Json(DataFreshness { stations, tocs, incidents }))
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
        let freshness = DataFreshness { stations: None, tocs: None, incidents: None };
        let json = serde_json::to_value(&freshness).unwrap();
        assert!(json["stations"].is_null());
        assert!(json["tocs"].is_null());
        assert!(json["incidents"].is_null());
    }

    #[test]
    fn round_trips_a_present_timestamp() {
        let ts = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        let freshness = DataFreshness { stations: Some(ts), tocs: None, incidents: None };
        let json = serde_json::to_value(&freshness).unwrap();
        let roundtripped: DateTime<Utc> = json["stations"].as_str().unwrap().parse().unwrap();
        assert_eq!(roundtripped, ts);
    }
}
