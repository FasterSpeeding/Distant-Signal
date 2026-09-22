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
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}",
            axum::routing::delete(delete_journey_leg),
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
mod leg_candidate_json_tests {
    use super::leg_candidate_json;
    use serde_json::json;

    /// One `search_journey_leg_candidates` row for a York->Newcastle leg
    /// riding a London Kings Cross -> Edinburgh service.
    fn row() -> serde_json::Value {
        json!({
            "uid": "P9E011",
            "destination_crs": "EDB",
            "true_origin_crs": "KGX",
            "scheduled": "19:00:00",
            "destination_arrival": "21:40:00",
            "destination_arrival_day_offset": 0,
            "leg_destination_arrival": "19:55:00",
            "leg_destination_arrival_day_offset": 0,
        })
    }

    #[test]
    fn renders_the_leg_s_own_endpoints_and_arrival_alongside_the_train_s_own_route() {
        let json = leg_candidate_json(&row(), "YRK", "NCL");
        // The leg: leaves York 19:00, reaches Newcastle 19:55.
        assert_eq!(json["stationCrs"], "YRK");
        assert_eq!(json["legOriginCrs"], "YRK");
        assert_eq!(json["legDestinationCrs"], "NCL");
        assert_eq!(json["scheduled"], "19:00");
        assert_eq!(json["legDestinationArrival"], "19:55");
        // The train, unchanged and still available as secondary detail --
        // this is what every candidate row USED to be labelled with, on
        // its own.
        assert_eq!(json["originCrs"], "KGX");
        assert_eq!(json["destinationCrs"], "EDB");
        assert_eq!(json["destinationArrival"], "21:40");
    }

    #[test]
    fn a_missing_leg_arrival_is_an_explicit_null_not_a_guess_from_the_terminus() {
        let mut row = row();
        row["leg_destination_arrival"] = serde_json::Value::Null;
        let json = leg_candidate_json(&row, "YRK", "NCL");
        assert!(json["legDestinationArrival"].is_null());
        // Specifically NOT silently backfilled from the terminus arrival,
        // which is a different station.
        assert_eq!(json["destinationArrival"], "21:40");
    }

    #[test]
    fn an_absent_day_offset_key_reads_as_same_day_rather_than_failing() {
        let mut row = row();
        row.as_object_mut()
            .unwrap()
            .remove("leg_destination_arrival_day_offset");
        let json = leg_candidate_json(&row, "YRK", "NCL");
        assert_eq!(json["legDestinationArrivalDayOffset"], 0);
    }

    #[test]
    fn a_leg_that_ends_at_the_schedule_s_own_terminus_still_names_the_leg_s_end() {
        let json = leg_candidate_json(&row(), "KGX", "EDB");
        assert_eq!(json["legOriginCrs"], "KGX");
        assert_eq!(json["legDestinationCrs"], "EDB");
        assert_eq!(json["stationCrs"], "KGX");
    }
}

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
    /// Whether the CALLER owns this journey, as opposed to reading it via a
    /// group it's been shared into (`journey_readable_by`). The frontend
    /// gates every owner-only action (share-to-group button, unmatched-leg
    /// candidate picker, matched-leg "Change train") on this flag -- the
    /// backend still refuses all three regardless for a non-owner, but
    /// showing them at all to a fellow group member who can only ever get a
    /// 404 is its own bug. See this plan's final-review findings (I1).
    is_owner: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyLegDetailResponse {
    id: i64,
    origin_crs: Option<String>,
    /// `None` whenever `origin_crs` is `None`, or there is no `stations`
    /// reference row for the code (a real, if rare, gap) -- see
    /// `data::journeys::JourneyLegWithNamesRow`'s own doc comment.
    origin_name: Option<String>,
    destination_crs: Option<String>,
    /// See `origin_name`'s doc comment -- same mechanism, joined on
    /// `destination_crs`.
    destination_name: Option<String>,
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
    /// `null` when the leg has no matched train yet, or no known
    /// origin/destination to check against (nothing to report -- not
    /// "checked and clean"). See station_skip.rs and this plan's §5.2.
    leg_skip: Option<LegSkipResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegSkipResponse {
    origin_skipped: bool,
    destination_skipped: bool,
}

/// `GET /Journeys/{journeyId}` -- design doc §4. No new backend read-model
/// query beyond joining straight into the existing
/// `TRACKED_TRAIN_STATE_SELECT` per matched leg, exactly as the design doc
/// itself specifies: "the wire payload for a matched leg is exactly
/// today's `TrackedTrainState` shape, unchanged."
///
/// `GET /Journeys/{journeyId}` -- the ONE read route in this file gated on
/// `journeys::journey_readable_by` (owner OR group-shared-with) rather
/// than the ownership-only check folded directly into every write route's
/// own query (e.g. `journeys::get_owned_leg`/
/// `journeys::set_leg_train_subscription`'s `WHERE ... AND user_id = $N`).
/// See
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §6's final
/// paragraph and
/// docs/superpowers/plans/2026-09-22-journey-tracking-phase4-group-sharing-plan.md's
/// Task 4: this is the one place in the whole /Journeys/* surface where a
/// caller who does not own the resource can still read it, and it must
/// stay that way -- deliberately -- while every other handler in this
/// file keeps the ownership-only gate.
async fn get_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
) -> Result<Json<JourneyDetailResponse>, (StatusCode, String)> {
    let readable = journeys::journey_readable_by(&app.database, journey_id, &user.id)
        .await
        .map_err(internal_error("check journey readability"))?;
    if !readable {
        return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string()));
    }

    let summary = journeys::get_journey_summary(&app.database, journey_id)
        .await
        .map_err(internal_error("read journey"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey with that id".to_string()))?;
    let is_owner = summary.user_id == user.id;

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

        let leg_skip = match (
            leg.origin_crs.as_deref(),
            leg.destination_crs.as_deref(),
            &tracked_train_state,
        ) {
            (Some(origin_crs), Some(destination_crs), Some(state)) => {
                let match_target = state
                    .pin_destination_crs
                    .as_deref()
                    .or(state.next_calling_point.as_deref());
                let status = match state.trains_id {
                    Some(trains_id) => {
                        crate::data::station_skip::leg_skip_status(
                            &app.database,
                            trains_id,
                            origin_crs,
                            destination_crs,
                            match_target,
                        )
                        .await
                    }
                    None => crate::data::station_skip::LegSkipStatus::default(),
                };
                Some(LegSkipResponse {
                    origin_skipped: status.origin_skipped,
                    destination_skipped: status.destination_skipped,
                })
            }
            _ => None,
        };

        legs.push(JourneyLegDetailResponse {
            id: leg.id,
            origin_crs: leg.origin_crs,
            origin_name: leg.origin_name,
            destination_crs: leg.destination_crs,
            destination_name: leg.destination_name,
            service_date: leg.service_date,
            depart_after: leg.depart_after,
            depart_before: leg.depart_before,
            arrive_after: leg.arrive_after,
            arrive_before: leg.arrive_before,
            match_mode: leg.match_mode,
            tracked_train_state,
            leg_skip,
        });
    }

    Ok(Json(JourneyDetailResponse {
        id: summary.id,
        custom_name: summary.custom_name,
        created_at: summary.created_at,
        legs,
        is_owner,
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
            .map(|row| leg_candidate_json(row, origin_crs, destination_crs))
            .collect::<Vec<serde_json::Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(crate::routes::trains::encode_cursor),
    })))
}

/// `render::calling_point_departure_json`'s output plus the three fields
/// that make a row about the TRAVELLER'S LEG rather than about the train's
/// own route -- 2026-09-22 UX review, C4.
///
/// The shared renderer already gives `scheduled` (the departure at the
/// leg's ORIGIN, since `search_journey_leg_candidates` keys `main` on
/// `main.origin_crs = <leg origin>`) and `stationCrs` (that origin). What
/// it cannot give is where the leg ENDS: its `destinationCrs`/
/// `destinationArrival` are the schedule's own terminus and the arrival
/// there, which for a York->Newcastle leg on a London->Edinburgh service
/// describe Edinburgh. Rendered on their own, three candidates all read
/// "19:00 · KGX → EDB" and the one decision the window search exists to
/// support -- which of these gets me from York to Newcastle, and when --
/// cannot be made from what is on screen.
///
/// Additive, never a rename: `destinationCrs`/`destinationArrival` keep
/// their existing meaning so `/public/trains/search` and the shared
/// renderer are untouched, and the frontend renders the train's own route
/// as dimmed secondary text beside the leg-scoped primary line.
///
/// `legDestinationArrival` is `null` whenever the schedule records neither
/// an arrival nor a booked departure at the leg's destination -- the row
/// then shows no arrival rather than a fabricated one.
fn leg_candidate_json(
    row: &serde_json::Value,
    leg_origin_crs: &str,
    leg_destination_crs: &str,
) -> serde_json::Value {
    let mut json = crate::render::calling_point_departure_json(row, leg_origin_crs);
    let arrival = row
        .get("leg_destination_arrival")
        .and_then(serde_json::Value::as_str)
        // Trimmed "HH:MM:SS" -> "HH:MM" exactly as the shared renderer
        // trims `scheduled` and `destinationArrival`, so the two times on
        // one row are always the same shape.
        .map(|s| s.chars().take(5).collect::<String>());
    let day_offset = row
        .get("leg_destination_arrival_day_offset")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if let Some(object) = json.as_object_mut() {
        object.insert(
            "legOriginCrs".to_string(),
            serde_json::Value::String(leg_origin_crs.to_string()),
        );
        object.insert(
            "legDestinationCrs".to_string(),
            serde_json::Value::String(leg_destination_crs.to_string()),
        );
        object.insert(
            "legDestinationArrival".to_string(),
            arrival.map_or(serde_json::Value::Null, serde_json::Value::String),
        );
        object.insert(
            "legDestinationArrivalDayOffset".to_string(),
            serde_json::Value::from(day_offset),
        );
    }
    json
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
        // between the read above and this write -- vanishingly unlikely,
        // but handled rather than silently ignored, matching
        // `post_attach_ticket`'s own analogous race-handling posture in
        // `routes/train.rs`. (`delete_journey_leg`, below, is that
        // deletion route -- added by the 2026-09-22 UX review's I14/2.4
        // fix, after this comment's original "no delete route exists"
        // reasoning was written.)
        return Err((
            StatusCode::NOT_FOUND,
            "no journey leg with that id".to_string(),
        ));
    }

    Ok(Json(MatchLegResponse { tracking_id }))
}

/// `DELETE /Journeys/{journeyId}/legs/{legId}` -- 2026-09-22 UX review
/// finding I14/2.4: a `pin`/`knownTrain`-mode leg has no persisted window
/// (`hasWindow` in `JourneyLegCard.tsx`), so it never offers "Change train",
/// and until this route existed a wrong pick had no recovery path at all.
/// Mirrors `routes::train::delete_tracked_train`'s own shape (same
/// `AuthenticatedUser` + 404-for-unknown-or-not-yours + `204 No Content` on
/// success) on the ownership check `journeys::delete_leg` folds into its own
/// query -- see that function's doc comment for why deleting the journey's
/// LAST leg deletes the whole journey too, and why the leg's underlying
/// `train_subscriptions` row (if any) is left untouched either way.
async fn delete_journey_leg(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = journeys::delete_leg(&app.database, journey_id, leg_id, &user.id)
        .await
        .map_err(internal_error("delete journey leg"))?;
    if deleted.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey leg with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
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

    /// Issues a `DELETE` against `router`, optionally with a session
    /// cookie -- mirrors `routes::train::db_tests::delete_request` exactly
    /// (same empty-body-becomes-`Value::Null` handling: a success here is
    /// `204 No Content` with no body, so feeding that straight to
    /// `serde_json::from_slice` would fail to parse and mask a real body on
    /// any OTHER status).
    async fn delete_request(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method("DELETE").uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
            })
        };
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

    /// End-to-end coverage for "Change train" (item 6 of the 2026-09-22 UX
    /// review's fix-cycle follow-up): a leg that has BOTH a persisted
    /// search window AND a currently-matched train -- the exact state
    /// `JourneyLegCard.tsx` gates its "Change train" button on
    /// (`hasWindow && isOwner`, rendered only in the MATCHED branch). No
    /// existing test covered this combination: `post_leg_train_commits_a_first_pick_then_a_change_train_repick`
    /// above re-picks a train but never calls `GET .../candidates` at all
    /// (it re-posts a fabricated train UID directly, skipping the button's
    /// own first step), and `get_leg_candidates_a_non_owner_gets_404`
    /// calls `GET .../candidates` but on a leg that is never matched --
    /// a different code path (an OPEN leg's card has no "Change train"
    /// button at all, see that component's own doc comment). This test
    /// walks the real sequence a click on the button performs: create a
    /// windowed leg, commit a first pick (now matched AND windowed),
    /// re-fetch candidates -- proving `get_leg_candidates` is NOT gated on
    /// match state, only on the leg's own persisted origin/destination/
    /// window -- then commit a second pick from that list and confirm
    /// `train_subscription_id` actually moved in the database, not just in
    /// the response body.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                change_train_end_to_end -- --ignored --test-threads=1`"]
    async fn change_train_end_to_end_candidates_stay_window_scoped_on_an_already_matched_leg() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-CHANGE-TRAIN-E2E").await;
        let router = test_router(test_app(pool.clone()));

        // A deliberately far-future, deliberately unrealistic fixture date
        // -- mirrors `data::queries::db_tests::fixture_date_feb`'s own 2099
        // convention -- so this test's own `schedule_destination_departures`
        // rows can never collide with real preview/production data sharing
        // this database (this repository's own live preview instance and
        // other agents' test runs included).
        let service_date =
            chrono::NaiveDate::from_ymd_opt(2099, 4, 17).expect("valid fixture date");
        async fn delete_fixture_day(pool: &PgPool, service_date: chrono::NaiveDate) {
            sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
                .bind(service_date)
                .execute(pool)
                .await
                .expect("cleanup fixture schedule_destination_departures rows");
        }
        delete_fixture_day(&pool, service_date).await;

        // Two real WAT -> RDG candidates within the leg's own persisted
        // window (08:00-11:00): CT-CHANGE-1 (the first pick) and
        // CT-CHANGE-2 (what "Change train" re-picks). This is the table
        // `GET .../candidates` actually reads
        // (`queries::search_journey_leg_candidates`) -- unlike
        // `post_leg_train`, which never touches it, so the sibling test
        // above (fabricated "A11111"/"A22222" UIDs) never exercises the
        // candidates-list route at all. Each train needs two rows: its own
        // departure from WAT, and its own arrival/departure record at its
        // destination RDG -- same two-row-per-train shape
        // `search_journey_leg_candidates_enforces_ordering_for_different_stations`
        // (`data/queries.rs`) already establishes for this table.
        fn candidate_rows(
            train_uid: &str,
            service_date: chrono::NaiveDate,
            departs: chrono::NaiveTime,
            arrives: chrono::NaiveTime,
        ) -> Vec<crate::data::queries::ScheduleDestinationDeparturesRow> {
            vec![
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "RDG".to_string(),
                    scheduled: departs,
                    day_offset: 0,
                    train_uid: train_uid.to_string(),
                    origin_crs: "WAT".to_string(),
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: Some(arrives),
                    destination_arrival_day_offset: 0,
                },
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "RDG".to_string(),
                    scheduled: arrives,
                    day_offset: 0,
                    train_uid: train_uid.to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: Some(arrives),
                    destination_arrival_day_offset: 0,
                },
            ]
        }
        let mut rows = candidate_rows(
            "CT-CHANGE-1",
            service_date,
            chrono::NaiveTime::from_hms_opt(9, 0, 0).expect("valid time"),
            chrono::NaiveTime::from_hms_opt(9, 30, 0).expect("valid time"),
        );
        rows.extend(candidate_rows(
            "CT-CHANGE-2",
            service_date,
            chrono::NaiveTime::from_hms_opt(10, 0, 0).expect("valid time"),
            chrono::NaiveTime::from_hms_opt(10, 30, 0).expect("valid time"),
        ));
        crate::data::queries::upsert_schedule_destination_departures(&pool, &rows)
            .await
            .expect("seed fixture schedule_destination_departures rows");

        // Create the window-mode leg -- the same persisted window
        // (`departAfter`/`departBefore`) `JourneyLegCard.tsx`'s `hasWindow`
        // check reads.
        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": service_date.to_string(),
                    "departWindow": { "after": "08:00:00", "before": "11:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        // First pick -- the leg is now MATCHED (`trainSubscriptionId` set)
        // while STILL carrying its window: exactly the combination
        // `JourneyLegCard.tsx` renders a "Change train" button for, and the
        // one gap this whole test exists to close.
        let (status, body) = post_json(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "CT-CHANGE-1", "serviceDate": service_date.to_string() }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "first pick: {body:?}");
        let first_tracking_id = body["trackingId"].as_i64().expect("trackingId present");

        // The button's own first step: `GET .../candidates` on this
        // now-MATCHED, still-windowed leg. `get_leg_candidates` reads the
        // leg's persisted origin/destination/window fields, never its
        // match state -- this proves that in practice, not just by reading
        // the handler.
        let (status, body) = request(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}/candidates"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "candidates on a matched leg: {body:?}");
        let uids: Vec<&str> = body["results"]
            .as_array()
            .expect("results is an array")
            .iter()
            .map(|r| r["uid"].as_str().expect("uid present"))
            .collect();
        assert!(
            uids.contains(&"CT-CHANGE-1") && uids.contains(&"CT-CHANGE-2"),
            "candidates must stay scoped to the leg's own persisted window on a matched leg too: {uids:?}"
        );

        // Picking a new candidate from that list -- the re-pick step
        // "Change train" performs, through the exact same commit route as
        // the first pick (`post_leg_train`'s own doc comment: "the SAME
        // route handles a leg's first pick ... and any later 'Change
        // train' re-pick").
        let (status, body) = post_json(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "CT-CHANGE-2", "serviceDate": service_date.to_string() }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "change train: {body:?}");
        let second_tracking_id = body["trackingId"].as_i64().expect("trackingId present");
        assert_ne!(first_tracking_id, second_tracking_id);

        // The leg's own `train_subscription_id` actually moved to the new
        // pick in the database, not just in the response body.
        let (train_subscription_id,): (Option<i64>,) =
            sqlx::query_as("SELECT train_subscription_id FROM journey_legs WHERE id = $1")
                .bind(leg_id)
                .fetch_one(&pool)
                .await
                .expect("read leg train_subscription_id");
        assert_eq!(train_subscription_id, Some(second_tracking_id));

        delete_fixture_day(&pool, service_date).await;
        cleanup_user(&pool, "TEST-ROUTE-CHANGE-TRAIN-E2E").await;
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
        // I1 (final review): the owner reading their own journey must see
        // `isOwner: true` -- the frontend gates every owner-only control on
        // this flag.
        assert_eq!(body["isOwner"], true);

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


    /// The wire contract for `legSkip`'s two states (this plan's `LegSkipResponse`,
    /// above) -- `null` for a leg with no matched train yet, an OBJECT (never
    /// `null`) once one is, regardless of whether either end actually turns
    /// out to be skipped. The unmatched half reuses the same `window`-mode
    /// leg shape `get_leg_candidates_a_non_owner_gets_404` already creates.
    /// The matched half deliberately uses `pin` mode, not `knownTrain`: a
    /// `knownTrain` leg's `origin_crs`/`destination_crs` are read back off
    /// `train_subscriptions.pin_origin_crs`/`pin_destination_crs`, which
    /// `create_subscription_for_train` only populates from the shared
    /// `trains` row's OWN schedule data (see that function's doc comment) --
    /// a fresh, fake test UID like `A88888` has none, so those columns (and
    /// therefore `legSkip`, which requires both CRSes known) would stay
    /// `NULL`/`None` even though a train IS bound. A `pin`-mode leg's
    /// `origin_crs`/`destination_crs` come straight off the request instead
    /// (`create_journey_with_pin_leg`'s own doc comment), so it's the only
    /// mode that reliably exercises the "matched AND both ends known"
    /// branch without first seeding a real schedule fixture. No
    /// `station_samples` row is seeded here, so `leg_skip_status` falls back
    /// to its own `LegSkipStatus::default()` (both flags `false`) -- fine,
    /// since this test is only proving the field is object-shaped once
    /// matched, not exercising the skip-detection logic itself (that's
    /// `station_skip.rs`'s own `find_leg_skip` unit tests, and
    /// `crates/notifier/src/skip_check.rs`'s DB-gated tests, for the
    /// notifier-side twin).
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey -- --ignored --test-threads=1`"]
    async fn get_journey_reports_leg_skip_as_null_when_unmatched_and_an_object_when_matched() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-LEG-SKIP").await;
        let router = test_router(test_app(pool.clone()));

        let (_, unmatched_created) = post_json(
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
        let unmatched_journey_id =
            unmatched_created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = request(
            router.clone(),
            format!("/Journeys/{unmatched_journey_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "get unmatched journey: {body:?}");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert!(
            legs[0]["trackedTrainState"].is_null(),
            "an unmatched (window-mode) leg should have no tracked train state: {legs:?}"
        );
        assert_eq!(
            legs[0]["legSkip"],
            serde_json::Value::Null,
            "an unmatched leg must report legSkip: null, not an object: {legs:?}"
        );

        let scheduled_departure = chrono::Utc::now().to_rfc3339();
        let (_, matched_created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "pin",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "scheduledDeparture": scheduled_departure,
                    "serviceDate": "2026-09-22"
                }
            }),
        )
        .await;
        let matched_journey_id =
            matched_created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) =
            request(router, format!("/Journeys/{matched_journey_id}"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK, "get matched journey: {body:?}");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert!(
            legs[0]["trackedTrainState"].is_object(),
            "a pin-mode leg is bound to a train_subscriptions row from birth, so this should be Some: {legs:?}"
        );
        let leg_skip = &legs[0]["legSkip"];
        assert!(
            leg_skip.is_object(),
            "a matched leg must report legSkip as an object, never null, once a train is bound: {leg_skip:?}"
        );
        assert!(
            leg_skip["originSkipped"].is_boolean(),
            "legSkip: {leg_skip:?}"
        );
        assert!(
            leg_skip["destinationSkipped"].is_boolean(),
            "legSkip: {leg_skip:?}"
        );

        cleanup_user(&pool, "TEST-ROUTE-LEG-SKIP").await;
    }


    /// Deletes a group row and its cascading `group_members`/`group_trains`/
    /// `group_journeys` rows -- this module's own equivalent of
    /// `data::journeys::db_tests`'s inline `DELETE FROM groups WHERE id =
    /// $1` cleanup, factored out since both new tests below need it.
    async fn cleanup_group(pool: &PgPool, group_id: &str) {
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized \
                -- --ignored --test-threads=1`"]
    async fn get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-SHARE-OWNER").await;
        let member_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-SHARE-MEMBER").await;
        let router = test_router(test_app(pool.clone()));

        // Owner creates a journey with a matched leg -- same shape
        // `get_journey_returns_the_matched_legs_tracked_train_state` above
        // already exercises, so this test's own point (whether the SECOND
        // user can read it) isn't muddied by also being the first test of
        // matched-leg rendering.
        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A88888", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        // Share the journey into a group both the owner and the member are
        // in -- the member was never given any `train_subscriptions`-level
        // ownership of the leg's underlying row at all.
        let group_id = crate::data::groups::create_group(
            &pool,
            "Get Journey Share Test",
            "TEST-ROUTE-GET-JOURNEY-SHARE-OWNER",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-ROUTE-GET-JOURNEY-SHARE-MEMBER")
        .execute(&pool)
        .await
        .expect("seed member");
        crate::data::groups::add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-ROUTE-GET-JOURNEY-SHARE-OWNER",
        )
        .await
        .expect("share journey");

        // The fellow group member reads the journey via the real HTTP
        // handler -- 200 with the same leg detail the owner would see, not
        // 404. This is the concrete, end-to-end proof of Task 4's whole
        // point: the member was never checked against
        // `train_subscriptions.user_id` at all, and correctly doesn't need
        // to be.
        let (status, body) = request(
            router,
            format!("/Journeys/{journey_id}"),
            Some(&member_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "group member read: {body:?}");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["matchMode"], "manual");
        assert!(legs[0]["trackedTrainState"].is_object());
        assert_eq!(legs[0]["trackedTrainState"]["trainUid"], "A88888");
        // I1 (final review): a group member reading a journey shared into
        // their group (never authorized as the owner) must see
        // `isOwner: false` -- the frontend uses this to hide the
        // share-journey button and the two owner-only leg controls
        // (`JourneyLegCandidates`/"Change train") that would otherwise 404
        // for them.
        assert_eq!(body["isOwner"], false);

        cleanup_group(&pool, &group_id).await;
        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-SHARE-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-SHARE-MEMBER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_journey_a_stranger_in_no_shared_group_still_gets_404 -- --ignored \
                --test-threads=1`"]
    async fn get_journey_a_stranger_in_no_shared_group_still_gets_404() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-STRANGER-OWNER").await;
        let stranger_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-STRANGER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A99999", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        // The stranger has no ownership and no group-share relationship to
        // this journey at all -- the ordinary 404, identical to today's
        // (pre-Phase-4) behavior. No group is even created here: this is
        // the plain, no-sharing-involved negative case.
        let (status, _) = request(
            router,
            format!("/Journeys/{journey_id}"),
            Some(&stranger_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-STRANGER-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-STRANGER").await;
    }

    // --- delete_journey_leg (I14/2.4 recovery path) ---------------------------

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                delete_journey_leg -- --ignored --test-threads=1`"]
    async fn delete_journey_leg_a_leg_owned_by_someone_else_is_404_not_403_and_survives() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-DELETE-LEG-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-DELETE-LEG-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "D11111", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, body) = delete_request(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}"),
            Some(&bystander_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::Value::String("no journey leg with that id".to_string())
        );

        // The leg -- and its journey -- must genuinely survive a
        // non-owner's delete attempt, not just return 404 with the row
        // quietly gone anyway.
        let (status, _) = request(
            router,
            format!("/Journeys/{journey_id}"),
            Some(&owner_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "owner's journey should still exist");

        cleanup_user(&pool, "TEST-ROUTE-DELETE-LEG-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-DELETE-LEG-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                delete_journey_leg -- --ignored --test-threads=1`"]
    async fn delete_journey_leg_a_nonexistent_leg_is_404() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-DELETE-LEG-NOTFOUND").await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) = delete_request(
            router,
            "/Journeys/99999999/legs/99999999".to_string(),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::Value::String("no journey leg with that id".to_string())
        );

        cleanup_user(&pool, "TEST-ROUTE-DELETE-LEG-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                delete_journey_leg -- --ignored --test-threads=1`"]
    async fn delete_journey_leg_the_owner_can_remove_a_no_window_leg_and_the_journey_goes_with_it_when_it_was_the_last_one()
     {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-DELETE-LEG-LAST").await;
        let router = test_router(test_app(pool.clone()));

        // A `knownTrain`-mode leg: no persisted window, exactly I14/2.4's
        // "wrong pick, no Change-train affordance" scenario.
        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "D22222", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, _) = delete_request(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}"),
            Some(&owner_token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // This was the journey's only leg -- the whole journey must be gone
        // too (`journeys::delete_leg`'s own doc comment), not left behind
        // as an empty, zero-leg row nothing else expects.
        let (status, _) = request(
            router,
            format!("/Journeys/{journey_id}"),
            Some(&owner_token),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "the now-empty journey should be gone too"
        );

        cleanup_user(&pool, "TEST-ROUTE-DELETE-LEG-LAST").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                delete_journey_leg -- --ignored --test-threads=1`"]
    async fn delete_journey_leg_removing_one_of_two_legs_leaves_the_journey_and_the_other_leg_intact()
     {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-DELETE-LEG-MULTI").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "D33333", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let first_leg_id = created["legId"].as_i64().expect("legId present");

        let (_, added) = post_json(
            router.clone(),
            format!("/Journeys/{journey_id}/legs"),
            Some(&owner_token),
            serde_json::json!({
                "mode": "knownTrain",
                "trainUid": "D44444",
                "serviceDate": "2026-09-22"
            }),
        )
        .await;
        let second_leg_id = added["legId"].as_i64().expect("legId present");

        let (status, _) = delete_request(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{first_leg_id}"),
            Some(&owner_token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = request(
            router,
            format!("/Journeys/{journey_id}"),
            Some(&owner_token),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "journey with a remaining leg should survive: {body:?}"
        );
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1, "only the deleted leg should be gone");
        assert_eq!(legs[0]["id"].as_i64(), Some(second_leg_id));

        cleanup_user(&pool, "TEST-ROUTE-DELETE-LEG-MULTI").await;
    }
}
