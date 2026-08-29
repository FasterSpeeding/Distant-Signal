//! `/Train/...`: individual train tracking. Pin *creation* requires an
//! authenticated session (`AuthenticatedUser`, from
//! docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 6) --
//! every tracked train has a real owner from birth, per that plan's
//! coordination fix to this one. State *reads* (Task 5) stay
//! unauthenticated/unscoped -- see that task's note on why this isn't a
//! strict "everything private" posture. Mounted directly (not under
//! `/public`) to match the design doc's sketched URL shape for the
//! eventual frontend page.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::TrackPinRequest;
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{eta_blend, train_tracking};

pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackPinResponse {
    tracking_id: i64,
    resolution_status: &'static str,
}

async fn post_track(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(pin): Json<TrackPinRequest>,
) -> Result<Json<TrackPinResponse>, (StatusCode, String)> {
    train_tracking::validate_pin(&pin, Utc::now()).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let tracking_id = train_tracking::create_pin(&app.database, &pin, &user.id)
        .await
        .map_err(internal_error("create tracking pin"))?;

    Ok(Json(TrackPinResponse { tracking_id, resolution_status: "pending" }))
}

async fn get_by_tracking_id(
    State(app): State<App>,
    Path(tracking_id): Path<i64>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?;
    match state {
        Some(state) => Ok(Json(blend_darwin_eta(&app, state).await)),
        None => Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }
}

async fn get_by_uid_and_date(
    State(app): State<App>,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_uid_and_date(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("read tracked train state"))?;
    match state {
        Some(state) => Ok(Json(blend_darwin_eta(&app, state).await)),
        None => Err((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string())),
    }
}

/// Best-effort overlay: if a live Darwin/LDBWS departure board sample for
/// this train's origin station has a concrete estimated time for a
/// departure heading to the train's pinned destination (or, failing that,
/// its currently-known next calling point), that overlays `eta_next`/
/// `eta_source` in the response -- never written back to
/// `train_current_state` (see `crates/api/src/data/eta_blend.rs`'s module
/// doc for why this stays read-time-only). Any failure to fetch a sample
/// (no row yet, a transient DB error) just leaves `state` as TRUST's own
/// propagation already had it -- this is a nice-to-have enhancement, not
/// something either read route should fail over.
async fn blend_darwin_eta(app: &App, mut state: train_tracking::TrackedTrainState) -> train_tracking::TrackedTrainState {
    let Some(destination) = state.pin_destination_crs.as_deref().or(state.next_calling_point.as_deref()) else {
        return state;
    };
    let Ok(samples) = crate::data::queries::latest_station_sample(&app.database, &state.pin_origin_crs).await else {
        return state;
    };
    if let Some(sample) = samples
        && let Some(eta) = eta_blend::find_darwin_eta(&sample.departures, Some(destination), None, state.service_date)
    {
        state.eta_next = Some(eta);
        state.eta_source = Some("darwin-estimated".to_string());
    }
    state
}

/// Shared 500 mapper for every route in this file. Takes the operation that
/// failed rather than hardcoding one: this helper serves the write route and
/// both read routes, and it previously logged and answered "failed to create
/// train tracking pin" for all three -- so a database error on a GET pointed
/// whoever read the log at a pin-creation bug that hadn't happened.
fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "train tracking request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to {operation}"))
    }
}
