//! `/Journeys/...`: journey tracking, Phase 1 (single-leg journeys +
//! manual-pick window search). See
//! docs/superpowers/specs/2026-09-22-journey-tracking-design.md and
//! docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md.
//! Every route here requires an authenticated session
//! (`AuthenticatedUser`) -- journeys have no anonymous/service-token path,
//! matching `routes::train`'s own posture for its write routes. Unlike
//! `routes::train::get_by_uid_and_date`, there is no public/unscoped
//! journey read in Phase 1 at all -- group sharing (design doc §6) is what
//! eventually opens a journey to anyone other than its own owner, and that
//! is Phase 4's job, not this file's.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{journeys, schedule_matching, train_tracking};

pub fn router() -> Router {
    Router::new()
        .route("/Journeys", axum::routing::post(post_journey))
        .route("/Journeys/mine", axum::routing::get(get_my_journeys))
        .route("/Journeys/{journey_id}", axum::routing::get(get_journey))
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/candidates",
            axum::routing::get(get_leg_candidates),
        )
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/train",
            axum::routing::post(post_leg_train),
        )
}

/// Three mutually-exclusive leg-creation shapes, discriminated by an
/// explicit `mode` field on the wire (`"pin" | "knownTrain" | "window"`,
/// `#[serde(tag = "mode", rename_all = "camelCase")]`) rather than an
/// untagged enum -- an untagged enum's default serde error
/// ("data did not match any variant of untagged enum...") is exactly the
/// kind of internal, non-actionable message this codebase's
/// `validate_pin`/`validate_custom_name` family of user-facing errors
/// deliberately avoids; a missing/invalid `mode` instead gets serde's own
/// "unknown variant" message naming the three real, meaningful wire
/// values. See this plan's own Judgment Call 3 for why THREE shapes, not
/// two.
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
enum CreateJourneyLegRequest {
    /// The legacy CRS+time GUESS pin, field-for-field identical to
    /// `common::TrackPinRequest` -- what `TrackTrainForm.tsx`'s existing
    /// "Pick a departure" / manual-entry flow submits (Task 15).
    Pin {
        origin_crs: String,
        scheduled_departure: DateTime<Utc>,
        service_date: NaiveDate,
        #[serde(default)]
        destination_crs: Option<String>,
        #[serde(default)]
        operator: Option<String>,
    },
    /// An already-known identity -- what `TrackThisTrainButton.tsx` submits
    /// (Task 14).
    KnownTrain {
        train_uid: String,
        service_date: NaiveDate,
    },
    /// An open time-window search -- no train chosen yet. Design doc §2.1;
    /// creates an `'unmatched'` leg the caller browses via
    /// `GET .../candidates` and commits via `POST .../train` (Task 16).
    Window {
        origin_crs: String,
        destination_crs: String,
        service_date: NaiveDate,
        #[serde(default)]
        depart_window: TimeWindow,
        #[serde(default)]
        arrive_window: TimeWindow,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyRequest {
    #[serde(default)]
    custom_name: Option<String>,
    leg: CreateJourneyLegRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyResponse {
    journey_id: i64,
    leg_id: i64,
    /// `None` only for a `window`-mode leg -- no train is bound yet.
    tracking_id: Option<i64>,
    /// Mirrors `train::TrackPinResponse::resolution_status` for a
    /// `pin`-mode leg; `None` for `knownTrain`/`window` modes, neither of
    /// which has an equivalent synchronous-match-attempt outcome to report
    /// (a `knownTrain` leg is resolved eagerly, immediately, below; a
    /// `window` leg has no train at all yet).
    resolution_status: Option<&'static str>,
}

async fn post_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<CreateJourneyRequest>,
) -> Result<Json<CreateJourneyResponse>, (StatusCode, String)> {
    match body.leg {
        CreateJourneyLegRequest::Pin {
            origin_crs,
            scheduled_departure,
            service_date,
            destination_crs,
            operator,
        } => {
            let pin = common::TrackPinRequest {
                service_date,
                origin_crs,
                scheduled_departure,
                destination_crs,
                operator,
                // `CreateJourneyLegRequest::Pin` carries no Darwin
                // per-calling-point skip snapshot of its own (unlike
                // `TrackPinRequest`'s own optional field, which
                // `TrackTrainForm.tsx`'s departure-board picker
                // populates) -- an empty list is exactly what
                // `skipped_stations`'s own doc comment already calls
                // "no known skip", the same value an older frontend
                // build or the CIF-picker/manual-entry path already
                // produces via `#[serde(default)]` on that field.
                skipped_stations: Vec::new(),
            };
            train_tracking::validate_pin(&pin, Utc::now())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

            let (journey_id, leg_id, tracking_id) = journeys::create_journey_with_pin_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                &pin,
            )
            .await
            .map_err(internal_error("create journey (pin leg)"))?;

            // Same best-effort synchronous match attempts
            // `routes::train::post_track` already makes for the exact same
            // `create_pin` call -- kept in lockstep so a pin-mode journey
            // resolves exactly as fast as a legacy bare pin would have
            // (design doc §7.2: "the literal same code path from this
            // point on").
            let resolution_status = match schedule_matching::attempt_schedule_match(
                &app.database,
                tracking_id,
                &pin.origin_crs,
                pin.scheduled_departure,
                pin.service_date,
                &app.schedule_crs_line_index,
                &pin.skipped_stations,
            )
            .await
            {
                Ok(true) => "schedule_matched",
                Ok(false) => "pending",
                Err(err) => {
                    tracing::warn!(
                        error = ?err,
                        tracking_id,
                        "schedule match attempt failed at journey creation; leg stays pending"
                    );
                    "pending"
                }
            };
            if let Err(err) = crate::data::trust_event_backlog_match::attempt_backlog_match(
                &app.database,
                tracking_id,
                &pin.origin_crs,
                pin.scheduled_departure,
                pin.service_date,
            )
            .await
            {
                tracing::warn!(error = ?err, tracking_id, "backlog match attempt failed; leg remains pending");
            }

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: Some(tracking_id),
                resolution_status: Some(resolution_status),
            }))
        }
        CreateJourneyLegRequest::KnownTrain {
            train_uid,
            service_date,
        } => {
            let trains_id =
                crate::data::trains::find_or_create_train(&app.database, &train_uid, service_date)
                    .await
                    .map_err(internal_error("find or create train"))?;
            let (journey_id, leg_id, tracking_id) = journeys::create_journey_with_known_train_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                trains_id,
                service_date,
            )
            .await
            .map_err(internal_error("create journey (known-train leg)"))?;

            // Same best-effort enrichment `routes::train::post_track_by_uid`
            // already makes for the exact same `create_subscription_for_train`
            // call.
            crate::routes::train::enrich_shared_train(
                &app,
                tracking_id,
                trains_id,
                &train_uid,
                service_date,
            )
            .await;

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: Some(tracking_id),
                resolution_status: None,
            }))
        }
        CreateJourneyLegRequest::Window {
            origin_crs,
            destination_crs,
            service_date,
            depart_window,
            arrive_window,
        } => {
            journeys::validate_window_leg(&origin_crs, &destination_crs, &depart_window, &arrive_window)
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

            let (journey_id, leg_id) = journeys::create_journey_with_window_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                &origin_crs.trim().to_ascii_uppercase(),
                &destination_crs.trim().to_ascii_uppercase(),
                service_date,
                depart_window,
                arrive_window,
            )
            .await
            .map_err(internal_error("create journey (window leg)"))?;

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: None,
                resolution_status: None,
            }))
        }
    }
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "journey request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to {operation}"))
    }
}
