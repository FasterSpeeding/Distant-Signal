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
        .route(
            "/Journeys/{journey_id}/legs",
            axum::routing::post(post_journey_leg),
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
///
/// `rename_all_fields = "camelCase"`, ADDITIONALLY to the container-level
/// `rename_all` above, is load-bearing and not redundant with it: serde's
/// enum-level `rename_all` only renames the *variant* identifiers used for
/// the `mode` tag ("Pin" -> "pin" etc.) -- it does NOT cascade into the
/// fields of a struct-shaped variant the way it would for a plain struct.
/// Without `rename_all_fields` too, every field below (`originCrs`,
/// `scheduledDeparture`, `trainUid`, `departWindow`, the `skippedStations`
/// field this finding adds, ...) would only deserialize off a snake_case
/// wire key, which nothing that calls `POST /Journeys` (`TrackTrainForm.tsx`,
/// `TrackThisTrainButton.tsx`, this file's own `db_tests`) ever sends --
/// confirmed by writing `wire_format_tests` below against the *actual*
/// enum (not just believing the doc comments): every field failed to
/// deserialize with "missing field" until this attribute was added.
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
        /// Field-for-field the same as `common::TrackPinRequest::skipped_stations`
        /// (same name, same default-empty-list semantics via
        /// `#[serde(default)]`) -- `TrackTrainForm.tsx`'s departure-board
        /// picker (`pickDeparture`) is the only producer of this signal
        /// anywhere in the codebase, and it's wired straight through to the
        /// `TrackPinRequest` built below, the same way it always reached
        /// `train_tracking::create_pin` via the legacy `POST /Train/track`
        /// route.
        #[serde(default)]
        skipped_stations: Vec<String>,
        /// Darwin's CURRENT platform for the picked departure-board row, and
        /// the earliest platform this poller had seen for it, both captured
        /// by `TrackTrainForm.tsx`'s `pickDeparture` exactly as
        /// `skipped_stations` above is. Field-for-field the same as
        /// `common::TrackPinRequest::platform`/`planned_platform`, which is
        /// where they are forwarded below.
        ///
        /// Integration note (2026-09-22): these arrived with the legacy
        /// `POST /Train/track` body, which this route replaced. Without them
        /// here the pin-mode leg would deserialize fine but silently drop
        /// the platform snapshot, so the journey page would show no platform
        /// for the origin calling point of any journey pinned this way.
        /// `#[serde(default)]` for the same reason as `skipped_stations`:
        /// the CIF-picker and manual-entry paths have no platform signal at
        /// all, and an older frontend build omits the fields entirely.
        #[serde(default)]
        platform: Option<String>,
        #[serde(default)]
        planned_platform: Option<String>,
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

/// Two of `CreateJourneyLegRequest`'s three shapes (no `Pin` -- spec §3's
/// "add a leg" only offers a direct known-train pick or an open
/// time-window search), for `POST /Journeys/{journeyId}/legs` (Phase 2).
/// Field names and the `mode` tag are IDENTICAL to the matching
/// `CreateJourneyLegRequest` variants on purpose -- same wire shape, same
/// serde gotcha applies: `rename_all_fields = "camelCase"` is REQUIRED in
/// addition to the container-level `rename_all`, or every field below
/// fails to deserialize off camelCase JSON (see `CreateJourneyLegRequest`'s
/// own doc comment for the full explanation; this plan's `wire_format_tests`
/// step below reproduces that same regression test against this enum).
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum AddJourneyLegRequest {
    KnownTrain {
        train_uid: String,
        service_date: NaiveDate,
    },
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddLegResponse {
    leg_id: i64,
    /// `Some` only for a `knownTrain`-mode leg -- mirrors
    /// `CreateJourneyResponse::tracking_id`'s own `None`-for-window
    /// convention exactly.
    tracking_id: Option<i64>,
}

/// No live database needed -- pure wire-format deserialization, unlike
/// `db_tests` below. Covers Finding I2's regression risk directly: that
/// `CreateJourneyLegRequest::Pin`'s `skippedStations` field actually
/// deserializes off the wire (and defaults to empty when the caller omits
/// it, for an older frontend build or the CIF-picker/manual-entry path),
/// since `post_journey` silently dropped this value entirely before this
/// field existed at all.
///
/// Also covers the `rename_all_fields` gap this same investigation turned
/// up (see the enum's own doc comment above): before that attribute was
/// added, NONE of these three variants' camelCase wire fields deserialized
/// at all, not just `skippedStations` -- `pin_mode_leg_...` and
/// `known_train_and_window_mode_legs_deserialize_their_camel_case_fields`
/// below both fail with a real live "missing field" error against the enum
/// as it stood before that fix.
#[cfg(test)]
mod wire_format_tests {
    use super::{AddJourneyLegRequest, CreateJourneyLegRequest};

    #[test]
    fn pin_mode_leg_deserializes_a_present_skipped_stations_array() {
        let leg: CreateJourneyLegRequest = serde_json::from_str(
            r#"{
                "mode": "pin",
                "originCrs": "WAT",
                "scheduledDeparture": "2026-09-22T18:32:00Z",
                "serviceDate": "2026-09-22",
                "skippedStations": ["CLJ", "WOK"]
            }"#,
        )
        .expect("valid pin-mode leg JSON should deserialize");

        let CreateJourneyLegRequest::Pin { skipped_stations, .. } = leg else {
            panic!("expected a Pin-mode leg, got {leg:?}");
        };
        assert_eq!(skipped_stations, vec!["CLJ".to_string(), "WOK".to_string()]);
    }

    #[test]
    fn pin_mode_leg_defaults_skipped_stations_to_empty_when_omitted() {
        let leg: CreateJourneyLegRequest = serde_json::from_str(
            r#"{
                "mode": "pin",
                "originCrs": "WAT",
                "scheduledDeparture": "2026-09-22T18:32:00Z",
                "serviceDate": "2026-09-22"
            }"#,
        )
        .expect("a pin-mode leg omitting skippedStations should still deserialize");

        let CreateJourneyLegRequest::Pin { skipped_stations, .. } = leg else {
            panic!("expected a Pin-mode leg, got {leg:?}");
        };
        assert!(skipped_stations.is_empty());
    }

    #[test]
    fn known_train_and_window_mode_legs_deserialize_their_camel_case_fields() {
        let known_train: CreateJourneyLegRequest = serde_json::from_str(
            r#"{"mode": "knownTrain", "trainUid": "A11111", "serviceDate": "2026-09-22"}"#,
        )
        .expect("valid knownTrain-mode leg JSON should deserialize");
        assert!(matches!(known_train, CreateJourneyLegRequest::KnownTrain { .. }));

        let window: CreateJourneyLegRequest = serde_json::from_str(
            r#"{
                "mode": "window",
                "originCrs": "WAT",
                "destinationCrs": "RDG",
                "serviceDate": "2026-09-22",
                "departWindow": {"after": "08:00:00"}
            }"#,
        )
        .expect("valid window-mode leg JSON should deserialize");
        assert!(matches!(window, CreateJourneyLegRequest::Window { .. }));
    }

    #[test]
    fn add_journey_leg_known_train_and_window_mode_legs_deserialize_their_camel_case_fields() {
        let known_train: AddJourneyLegRequest = serde_json::from_str(
            r#"{"mode": "knownTrain", "trainUid": "A11111", "serviceDate": "2026-09-22"}"#,
        )
        .expect("valid knownTrain-mode leg JSON should deserialize");
        assert!(matches!(known_train, AddJourneyLegRequest::KnownTrain { .. }));

        let window: AddJourneyLegRequest = serde_json::from_str(
            r#"{
                "mode": "window",
                "originCrs": "WAT",
                "destinationCrs": "RDG",
                "serviceDate": "2026-09-22",
                "departWindow": {"after": "08:00:00"}
            }"#,
        )
        .expect("valid window-mode leg JSON should deserialize");
        assert!(matches!(window, AddJourneyLegRequest::Window { .. }));
    }
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
            skipped_stations,
            platform,
            planned_platform,
        } => {
            let pin = common::TrackPinRequest {
                service_date,
                origin_crs,
                scheduled_departure,
                destination_crs,
                operator,
                // Wired straight through from the wire request -- an older
                // frontend build, or the CIF-picker/manual-entry path
                // (neither of which has this signal at all), still
                // deserializes as an empty list, "no known skip", via
                // `#[serde(default)]` on `CreateJourneyLegRequest::Pin`'s
                // own `skipped_stations` field above.
                skipped_stations,
                // Same straight-through wiring, for the platform snapshot --
                // see `CreateJourneyLegRequest::Pin`'s own fields above.
                platform,
                planned_platform,
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
                // Kept in lockstep with `routes::train::post_track`'s own
                // call, per the design doc's "the literal same code path
                // from this point on" -- the platform snapshot has to reach
                // the match attempt here exactly as it does there, or a
                // pin-mode journey would resolve without the origin
                // platform a legacy bare pin would have carried.
                pin.platform.as_deref(),
                pin.planned_platform.as_deref(),
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

/// `POST /Journeys/{journeyId}/legs` -- adds a new leg to an existing
/// journey the caller owns (spec §3). `leg_order` is assigned by
/// `data::journeys::add_known_train_leg_to_journey`/`add_window_leg_to_journey`
/// as `max(leg_order) + 1` for this journey, never supplied by the caller.
/// Same 404-never-403 ownership convention as every other route in this
/// app: "journey doesn't exist" and "journey exists but isn't yours" are
/// indistinguishable to the caller.
async fn post_journey_leg(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    Json(request): Json<AddJourneyLegRequest>,
) -> Result<Json<AddLegResponse>, (StatusCode, String)> {
    match request {
        AddJourneyLegRequest::KnownTrain {
            train_uid,
            service_date,
        } => {
            let trains_id =
                crate::data::trains::find_or_create_train(&app.database, &train_uid, service_date)
                    .await
                    .map_err(internal_error("find or create train"))?;
            let added = journeys::add_known_train_leg_to_journey(
                &app.database,
                journey_id,
                &user.id,
                trains_id,
                service_date,
            )
            .await
            .map_err(internal_error("add leg to journey (known-train)"))?;
            let Some((leg_id, tracking_id)) = added else {
                return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string()));
            };

            // Same best-effort enrichment `post_journey`'s own `KnownTrain`
            // arm and `post_leg_train` already make for the exact same
            // `create_subscription_for_train` call.
            crate::routes::train::enrich_shared_train(
                &app,
                tracking_id,
                trains_id,
                &train_uid,
                service_date,
            )
            .await;

            Ok(Json(AddLegResponse {
                leg_id,
                tracking_id: Some(tracking_id),
            }))
        }
        AddJourneyLegRequest::Window {
            origin_crs,
            destination_crs,
            service_date,
            depart_window,
            arrive_window,
        } => {
            journeys::validate_window_leg(&origin_crs, &destination_crs, &depart_window, &arrive_window)
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

            let added = journeys::add_window_leg_to_journey(
                &app.database,
                journey_id,
                &user.id,
                &origin_crs.trim().to_ascii_uppercase(),
                &destination_crs.trim().to_ascii_uppercase(),
                service_date,
                depart_window,
                arrive_window,
            )
            .await
            .map_err(internal_error("add leg to journey (window)"))?;
            let Some(leg_id) = added else {
                return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string()));
            };

            Ok(Json(AddLegResponse {
                leg_id,
                tracking_id: None,
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

async fn get_my_journeys(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<journeys::JourneyListItem>>, (StatusCode, String)> {
    let rows = journeys::list_journeys_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list journeys"))?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyDetailResponse {
    id: i64,
    custom_name: Option<String>,
    created_at: DateTime<Utc>,
    legs: Vec<JourneyLegDetailResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyLegDetailResponse {
    id: i64,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
    service_date: NaiveDate,
    depart_after: Option<NaiveTime>,
    depart_before: Option<NaiveTime>,
    arrive_after: Option<NaiveTime>,
    arrive_before: Option<NaiveTime>,
    match_mode: String,
    /// `Some` once a train is bound -- the EXACT same shape
    /// `GET /Train/{trackingId}` returns
    /// (`train_tracking::TRACKED_TRAIN_STATE_SELECT`, `attach_journey_stops`,
    /// `blend_darwin_eta`, all reused unchanged; design doc §4). `None`
    /// for an unmatched (`train_subscription_id IS NULL`) leg.
    tracked_train_state: Option<train_tracking::TrackedTrainState>,
}

/// `GET /Journeys/{journeyId}` -- design doc §4. No new backend read-model
/// query beyond joining straight into the existing
/// `TRACKED_TRAIN_STATE_SELECT` per matched leg, exactly as the design doc
/// itself specifies: "the wire payload for a matched leg is exactly
/// today's `TrackedTrainState` shape, unchanged."
async fn get_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
) -> Result<Json<JourneyDetailResponse>, (StatusCode, String)> {
    let summary = journeys::get_owned_journey_summary(&app.database, journey_id, &user.id)
        .await
        .map_err(internal_error("read journey"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey with that id".to_string()))?;

    let leg_rows = journeys::list_legs_for_journey(&app.database, journey_id)
        .await
        .map_err(internal_error("list journey legs"))?;

    let mut legs = Vec::with_capacity(leg_rows.len());
    for leg in leg_rows {
        let tracked_train_state = match leg.train_subscription_id {
            Some(tracking_id) => {
                match train_tracking::get_by_tracking_id(&app.database, tracking_id)
                    .await
                    .map_err(internal_error("read tracked train state"))?
                {
                    Some(state) => Some(
                        crate::routes::train::attach_journey_stops(
                            &app,
                            crate::routes::train::blend_darwin_eta(&app, state).await,
                        )
                        .await,
                    ),
                    None => None,
                }
            }
            None => None,
        };
        legs.push(JourneyLegDetailResponse {
            id: leg.id,
            origin_crs: leg.origin_crs,
            destination_crs: leg.destination_crs,
            service_date: leg.service_date,
            depart_after: leg.depart_after,
            depart_before: leg.depart_before,
            arrive_after: leg.arrive_after,
            arrive_before: leg.arrive_before,
            match_mode: leg.match_mode,
            tracked_train_state,
        });
    }

    Ok(Json(JourneyDetailResponse {
        id: summary.id,
        custom_name: summary.custom_name,
        created_at: summary.created_at,
        legs,
    }))
}

#[derive(Debug, Deserialize)]
struct CandidatesParams {
    limit: Option<String>,
    after: Option<String>,
}

/// `GET /Journeys/{journeyId}/legs/{legId}/candidates` -- design doc §2.2.
/// Runs [`crate::data::queries::search_journey_leg_candidates`] against the
/// leg's own persisted `origin_crs`/`destination_crs`/`depart_*`/
/// `arrive_*`, returns the exact same envelope shape
/// `GET /public/trains/search` already returns (`{results, nextCursor}`,
/// reusing `render::calling_point_departure_json` verbatim) -- no new DTO,
/// per the design doc's own explicit direction.
async fn get_leg_candidates(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    Query(params): Query<CandidatesParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let leg = journeys::get_owned_leg(&app.database, journey_id, leg_id, &user.id)
        .await
        .map_err(internal_error("read journey leg"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey leg with that id".to_string()))?;

    let (Some(origin_crs), Some(destination_crs)) =
        (leg.origin_crs.as_deref(), leg.destination_crs.as_deref())
    else {
        // Only reachable for a `pin`/`knownTrain`-mode leg whose underlying
        // train has no schedule data yet -- see this plan's own Judgment
        // Call 1. Such a leg was never window-searched and has nothing to
        // browse candidates for.
        return Err((
            StatusCode::BAD_REQUEST,
            "this leg has no origin/destination to search candidates for".to_string(),
        ));
    };

    let limit = crate::routes::trains::normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(crate::routes::trains::decode_cursor)
        .transpose()?;

    let Some(page) = crate::data::queries::search_journey_leg_candidates(
        &app.database,
        origin_crs,
        destination_crs,
        leg.service_date,
        leg.depart_after,
        leg.depart_before,
        leg.arrive_after,
        leg.arrive_before,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error("search journey leg candidates"))?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for this leg's service date"
                .to_string(),
        ));
    };

    Ok(Json(serde_json::json!({
        "results": page
            .departures
            .iter()
            .map(|row| crate::render::calling_point_departure_json(row, origin_crs))
            .collect::<Vec<serde_json::Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(crate::routes::trains::encode_cursor),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchLegRequest {
    train_uid: String,
    service_date: NaiveDate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchLegResponse {
    tracking_id: i64,
}

/// `POST /Journeys/{journeyId}/legs/{legId}/train` -- binds (or re-binds)
/// a leg to a real train, `'manual'` mode only (design doc §2.3). The SAME
/// route handles a leg's first pick (from `GET .../candidates`, Task 10)
/// and any later "Change train" re-pick -- it is always an `UPDATE`, never
/// a new leg (see `journeys::set_leg_train_subscription`'s own doc
/// comment).
async fn post_leg_train(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    Json(body): Json<MatchLegRequest>,
) -> Result<Json<MatchLegResponse>, (StatusCode, String)> {
    // Ownership-checked read first -- same 404-never-403 posture as every
    // other route in this crate. Also confirms the leg exists before this
    // conjures a `trains`/`train_subscriptions` row for it.
    journeys::get_owned_leg(&app.database, journey_id, leg_id, &user.id)
        .await
        .map_err(internal_error("read journey leg"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey leg with that id".to_string()))?;

    let trains_id =
        crate::data::trains::find_or_create_train(&app.database, &body.train_uid, body.service_date)
            .await
            .map_err(internal_error("find or create train"))?;
    let tracking_id =
        train_tracking::create_subscription_for_train(&app.database, trains_id, &user.id)
            .await
            .map_err(internal_error("create subscription"))?;

    crate::routes::train::enrich_shared_train(
        &app,
        tracking_id,
        trains_id,
        &body.train_uid,
        body.service_date,
    )
    .await;

    let updated =
        journeys::set_leg_train_subscription(&app.database, journey_id, leg_id, &user.id, tracking_id)
            .await
            .map_err(internal_error("set journey leg train"))?;
    if !updated {
        // Lost a race against a concurrent deletion of the underlying leg
        // between the read above and this write -- vanishingly unlikely
        // (no delete-journey route exists in Phase 1 at all, per this
        // plan's own Non-goals), but handled rather than silently ignored,
        // matching `post_attach_ticket`'s own analogous race-handling
        // posture in `routes/train.rs`.
        return Err((
            StatusCode::NOT_FOUND,
            "no journey leg with that id".to_string(),
        ));
    }

    Ok(Json(MatchLegResponse { tracking_id }))
}

/// Scaffolding copied verbatim from `routes::train::db_tests` (same
/// cross-file duplication convention Task 6 already follows for the data
/// layer's own `db_tests` module in `data::journeys`) -- `test_app`/
/// `test_router`/`seed_session`/`connect`/`request`/`post_json` are
/// byte-for-byte identical to that module's own copies. `cleanup_user` is
/// the one adaptation: `routes::train::db_tests::cleanup_user` only knows
/// about `train_subscriptions`/`trains`/`users`, nothing about
/// `journeys`/`journey_legs` -- a plain copy of it here would leave a
/// fixture's `journeys`/`journey_legs` rows behind (`journeys.user_id
/// REFERENCES users(id)` has no `ON DELETE CASCADE`, so `DELETE FROM users`
/// would then fail on a foreign-key violation). This instead follows
/// `data::journeys::db_tests::cleanup_user`'s own lead (same schema, same
/// problem, already solved there): delete `journey_legs`, then `journeys`,
/// then `train_subscriptions`, then the user.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::users::insert_session;

    /// Every `ServiceArguments` field filled with an inert placeholder --
    /// this file's routes don't read `config.lines` at all. Copied from
    /// `crate::routes::train::db_tests::test_app_with_schedule_index`'s
    /// empty-index default (this module has no test needing a non-empty
    /// `schedule_crs_line_index`, so there is no separate
    /// `test_app_with_schedule_index` variant here).
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. None of this file's
            // routes touch Redis at all.
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    /// The real `journeys::router()`, mounted unprefixed exactly as
    /// `main.rs` does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user if
    /// it doesn't already exist) and returns the *raw* token -- send it as
    /// `Cookie: distant_signal_session=<raw>`, never the hash `sessions`
    /// actually stores.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");

        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed fixture session");
        raw_token
    }

    /// Deletes a fixture user and its fixtures -- see this module's own doc
    /// comment above for why this is NOT a byte-for-byte copy of
    /// `routes::train::db_tests::cleanup_user`.
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "DELETE FROM journey_legs WHERE journey_id IN \
                (SELECT id FROM journeys WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_legs rows");
        sqlx::query("DELETE FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journeys rows");
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains rows");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Issues a GET against `router`, optionally with a session cookie, and
    /// returns `(status, parsed JSON body)`. Every route in this file
    /// returns either a JSON object body or a plain-text `(StatusCode,
    /// String)` error body, so wrapping the latter as a JSON string lets
    /// every case share one return shape.
    async fn request(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    /// Issues a `POST` with a JSON body against `router`, optionally with a
    /// session cookie -- the write-path counterpart to `request` above
    /// (which only ever issues an empty-body `GET`). Same shared
    /// `(status, parsed body)` return shape, same plain-text-becomes-`Value::String`
    /// handling for a `(StatusCode, String)` error response.
    async fn post_json(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .uri(uri)
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder
            .body(Body::from(
                serde_json::to_vec(&body).expect("serialize request body"),
            ))
            .expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[test]
    fn router_builds_without_panicking() {
        let _ = super::router();
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_leg_candidates -- --ignored --test-threads=1`"]
    async fn get_leg_candidates_a_non_owner_gets_404() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-CANDIDATES-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-CANDIDATES-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, _) = request(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/candidates"),
            Some(&bystander_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, "TEST-ROUTE-CANDIDATES-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-CANDIDATES-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_leg_train -- --ignored --test-threads=1`"]
    async fn post_leg_train_commits_a_first_pick_then_a_change_train_repick() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, body) = post_json(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "A11111", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "first pick: {body:?}");
        let first_tracking_id = body["trackingId"].as_i64().expect("trackingId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "A22222", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "change train: {body:?}");
        let second_tracking_id = body["trackingId"].as_i64().expect("trackingId present");
        assert_ne!(first_tracking_id, second_tracking_id);

        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_leg_train -- --ignored --test-threads=1`"]
    async fn post_leg_train_a_leg_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&bystander_token),
            serde_json::json!({ "trainUid": "A33333", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, serde_json::Value::String("no journey leg with that id".to_string()));

        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey -- --ignored --test-threads=1`"]
    async fn get_journey_returns_the_matched_legs_tracked_train_state() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A44444", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = request(router, format!("/Journeys/{journey_id}"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK, "get journey: {body:?}");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["matchMode"], "manual");
        assert!(legs[0]["trackedTrainState"].is_object());
        assert_eq!(legs[0]["trackedTrainState"]["trainUid"], "A44444");

        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey -- --ignored --test-threads=1`"]
    async fn get_journey_a_journey_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A55555", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, _) = request(router, format!("/Journeys/{journey_id}"), Some(&bystander_token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_my_journeys -- --ignored --test-threads=1`"]
    async fn get_my_journeys_lists_every_owned_journey_most_recent_first() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-MY-JOURNEYS").await;
        let router = test_router(test_app(pool.clone()));

        let (_, first_created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({ "leg": { "mode": "knownTrain", "trainUid": "A66666", "serviceDate": "2026-09-22" } }),
        )
        .await;
        let first_tracking_id = first_created["trackingId"].as_i64().expect("trackingId present");

        let (_, second_created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({ "leg": { "mode": "knownTrain", "trainUid": "A77777", "serviceDate": "2026-09-22" } }),
        )
        .await;
        let second_tracking_id = second_created["trackingId"].as_i64().expect("trackingId present");

        let (status, body) = request(router, "/Journeys/mine".to_string(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("array response");
        assert_eq!(rows.len(), 2);
        // `list_journeys_for_user` orders `ORDER BY j.created_at DESC` --
        // the more-recently-created journey (the second POST, A77777) must
        // come first. Correlating via `trainSubscriptionId`/`trackingId`
        // (rather than array position alone) is what actually pins this
        // test to catch a reversed/dropped `ORDER BY`, unlike a bare
        // `rows.len() == 2` check.
        assert_eq!(
            rows[0]["trainSubscriptionId"].as_i64(),
            Some(second_tracking_id),
            "most-recently-created journey should be first: {rows:?}"
        );
        assert_eq!(
            rows[1]["trainSubscriptionId"].as_i64(),
            Some(first_tracking_id),
            "first-created journey should be last: {rows:?}"
        );

        cleanup_user(&pool, "TEST-ROUTE-MY-JOURNEYS").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_adds_a_window_leg_and_assigns_the_next_leg_order() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-ADD-LEG-WINDOW").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&token),
            serde_json::json!({
                "mode": "window",
                "originCrs": "RDG",
                "destinationCrs": "PAD",
                "serviceDate": "2026-09-22",
                "departWindow": { "after": "10:00:00" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "add window leg: {body:?}");
        let leg_id = body["legId"].as_i64().expect("legId present");
        assert!(body["trackingId"].is_null());

        let leg_order: i32 =
            sqlx::query_scalar("SELECT leg_order FROM journey_legs WHERE id = $1")
                .bind(leg_id)
                .fetch_one(&pool)
                .await
                .expect("read leg_order");
        assert_eq!(leg_order, 2);

        cleanup_user(&pool, "TEST-ROUTE-ADD-LEG-WINDOW").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_a_journey_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-ADD-LEG-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-ADD-LEG-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&bystander_token),
            serde_json::json!({
                "mode": "knownTrain",
                "trainUid": "A88888",
                "serviceDate": "2026-09-22"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, serde_json::Value::String("no journey with that id".to_string()));

        cleanup_user(&pool, "TEST-ROUTE-ADD-LEG-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-ADD-LEG-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_a_known_train_leg_is_immediately_matched() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-ADD-LEG-KNOWN-TRAIN").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&token),
            serde_json::json!({
                "mode": "knownTrain",
                "trainUid": "A99999",
                "serviceDate": "2026-09-22"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "add known-train leg: {body:?}");
        let leg_id = body["legId"].as_i64().expect("legId present");
        let tracking_id = body["trackingId"].as_i64().expect("trackingId present");

        let (match_mode, train_subscription_id): (String, Option<i64>) = sqlx::query_as(
            "SELECT match_mode, train_subscription_id FROM journey_legs WHERE id = $1",
        )
        .bind(leg_id)
        .fetch_one(&pool)
        .await
        .expect("read leg match_mode/train_subscription_id");
        assert_eq!(match_mode, "manual");
        assert_eq!(train_subscription_id, Some(tracking_id));

        cleanup_user(&pool, "TEST-ROUTE-ADD-LEG-KNOWN-TRAIN").await;
    }
}
