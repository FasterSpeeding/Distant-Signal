//! Read-only endpoint exposing which stations `poller-ldbws` should
//! sample, computed from the line catalogue already loaded into
//! `AppState` at startup (see `crates/api/src/data/config.rs`).

use axum::Json;
use axum::extract::State;

use crate::app::{App, Router};
use crate::data::samples::dedup_sample_stations;

pub fn router() -> Router {
    Router::new().route("/sample-stations", axum::routing::get(get_sample_stations))
}

async fn get_sample_stations(State(app): State<App>) -> Json<Vec<String>> {
    Json(dedup_sample_stations(&app.config.lines))
}
