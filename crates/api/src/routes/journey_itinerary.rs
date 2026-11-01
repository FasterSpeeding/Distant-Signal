//! `POST /Journeys/itinerary`: a journey and all of its `knownTrain` legs in
//! one transaction, idempotent under an `Idempotency-Key` (spec
//! docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md §5.8).
//!
//! Two callers:
//!
//! - **The web planner** (`PlanTripFlow.tsx`, decision D-15), with the
//!   session cookie. It used to send one `POST /Journeys` and then one
//!   `POST /Journeys/{id}/legs` per further leg, so a failure part-way left
//!   a half-built journey. The key is optional; errors are plain-text sentences, like every other
//!   journey route.
//! - **A delegated client** (ds-mcp, `journeys:write`). `Idempotency-Key`
//!   required, at most
//!   `DELEGATED_DAILY_SAVE_LIMIT` (50) saves per user per rolling 24 hours,
//!   and errors in the delegated JSON contract (§5.15). The journey is
//!   stamped `created_via = <client>`, which is what lets that client
//!   delete it later (§5.5a).
//!
//! Either caller may send 1 to [`MAX_ITINERARY_LEGS`] legs, the journey's
//! own leg cap, at least one of them a train leg.
//!
//! Optional `"operatorFilter": {"mode": "only", "entries": ["VT", "LT"]}`
//! (2026-10-31): the `/plan` operator filter the route was planned with
//! (`only`, `avoid` or `prefer`, entries as `/Trips/plan`'s `operator`
//! takes them), stored on the journey so its replans honour it
//! (`data::journey_operator_filter`).
//!
//! Body (camelCase, unknown fields refused):
//!
//! ```json
//! {"customName": "Optional", "legs": [
//!   {"trainUid": "C12345", "serviceDate": "2026-10-20",
//!    "originCrs": "CRE", "destinationCrs": "PRE"},
//!   {"kind": "untracked", "transitMode": "tube", "lineName": "Victoria line",
//!    "originName": "Euston", "destinationName": "Victoria",
//!    "plannedDeparture": "2026-10-20T10:05:00+01:00",
//!    "plannedArrival": "2026-10-20T10:20:00+01:00"}]}
//! ```
//!
//! `originCrs`/`destinationCrs` are optional, as for `POST /Journeys`'s
//! `knownTrain` leg. A leg with no `kind` is a train leg, as before
//! 2026-10-20; `"kind": "untracked"` is a leg we have no data for (spec
//! 2026-10-20-untracked-legs-and-tracked-train-states-design.md §2.4),
//! allowed for a delegated client too. A repeat with the same key and the
//! same body returns the first response (200); the same key with a
//! different body is 409 `idempotency_conflict`. Keys are remembered per
//! user for 24 hours.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::App;
use crate::auth::delegated::{DelegatedOrSessionUser, WriteScope, error_response};
use crate::data::delegated::IdempotencyClaim;
use crate::data::{journeys, leg_validation, train_tracking};

/// Legs one itinerary may have, for every caller: the journey's own cap,
/// [`journeys::MAX_LEGS_PER_JOURNEY`] (a test keeps the two equal).
pub const MAX_ITINERARY_LEGS: usize = 20;
/// The longest `Idempotency-Key` accepted (the column's own limit).
pub const MAX_IDEMPOTENCY_KEY_LENGTH: usize = 200;

/// The request body. `Serialize` too: its canonical JSON is what the
/// idempotency hash covers, so key order and whitespace never matter.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItineraryRequest {
    #[serde(default)]
    custom_name: Option<String>,
    legs: Vec<ItineraryLegRequest>,
    /// The plan's operator filter (additive, 2026-10-31). Last, and skipped
    /// when absent, so a body without it hashes as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operator_filter: Option<crate::data::journey_operator_filter::OperatorFilterRequest>,
}

/// One leg, a train leg or (`kind: "untracked"`) an untracked one. Flat
/// rather than a tagged enum so a body from before untracked legs existed
/// (no `kind`) still parses, and errors stay sentences.
///
/// **Field order and `skip_serializing_if` are load-bearing.** The
/// idempotency hash covers this struct's own serialisation, and keys are
/// remembered across a deploy: a train leg must serialise byte for byte as
/// it did when `trainUid`/`serviceDate` were required and these four were
/// the only fields (`a_train_leg_hashes_as_it_did_before_untracked_legs`).
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ItineraryLegRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    train_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_date: Option<NaiveDate>,
    #[serde(default)]
    origin_crs: Option<String>,
    #[serde(default)]
    destination_crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transit_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    destination_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    planned_departure: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    planned_arrival: Option<chrono::DateTime<chrono::Utc>>,
    /// An untracked leg's `TfL` stops (additive, 2026-10-20). Skipped when
    /// absent, so a body without them hashes as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin_stop_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    destination_stop_id: Option<String>,
    /// A timetabled leg's network ride (DS-237, additive; skipped when
    /// absent, so older bodies hash as before):
    /// `journeys::attach_leg_timetable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transit_network: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transit_line_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    headsign: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItineraryLegResponse {
    leg_id: i64,
    /// `None` (`null`) for an untracked leg, which tracks no train.
    tracking_id: Option<i64>,
    /// `train` or `untracked`. Absent from a response stored before
    /// 2026-10-20 and replayed (always a train leg then).
    #[serde(default = "train_kind")]
    kind: String,
}

fn train_kind() -> String {
    journeys::LEG_KIND_TRAIN.to_string()
}

/// A superset of `POST /Journeys`' response, so the web planner's
/// `onCreated` handler takes either: `legId` is the first leg's,
/// `trackingId` the first TRAIN leg's (an itinerary always has one).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItineraryResponse {
    journey_id: i64,
    leg_id: i64,
    tracking_id: i64,
    resolution_status: Option<String>,
    legs: Vec<ItineraryLegResponse>,
    created_via: String,
}

/// An error, rendered as the delegated JSON contract (`{error, message}`)
/// for a delegated caller and as a plain-text sentence for the web app.
#[derive(Debug)]
pub(crate) enum RouteError {
    Coded {
        status: StatusCode,
        code: &'static str,
        message: String,
        retry_after_secs: Option<i64>,
    },
    /// An existing helper's `(status, body)`, kept as it is for a 503 (its
    /// body is already the `service_unavailable` JSON).
    Legacy(StatusCode, String),
}

impl RouteError {
    pub(crate) fn coded(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        RouteError::Coded {
            status,
            code,
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub(crate) fn render(self, delegated: bool, bad_request_code: &'static str) -> Response {
        let (status, code, message, retry_after_secs) = match self {
            RouteError::Coded {
                status,
                code,
                message,
                retry_after_secs,
            } => (status, code, message, retry_after_secs),
            RouteError::Legacy(status, body) => {
                if status == StatusCode::SERVICE_UNAVAILABLE || !delegated {
                    return (status, body).into_response();
                }
                let code = match status {
                    StatusCode::BAD_REQUEST => bad_request_code,
                    StatusCode::NOT_FOUND => "not_found",
                    StatusCode::INTERNAL_SERVER_ERROR => "internal_error",
                    _ => "error",
                };
                (status, code, body, None)
            }
        };
        let mut response = if delegated {
            error_response(status, code, &message)
        } else {
            (status, message).into_response()
        };
        if let Some(secs) = retry_after_secs {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from(u64::try_from(secs.max(1)).unwrap_or(1)),
            );
        }
        response
    }
}

impl From<(StatusCode, String)> for RouteError {
    fn from((status, body): (StatusCode, String)) -> Self {
        RouteError::Legacy(status, body)
    }
}

fn internal(operation: &'static str) -> impl Fn(anyhow::Error) -> RouteError {
    move |err| {
        if let Some((status, body)) = crate::unavailable::response_for(&err) {
            return RouteError::Legacy(status, body);
        }
        tracing::error!(error = ?err, operation, "itinerary request failed");
        RouteError::coded(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("failed to {operation}"),
        )
    }
}

/// `POST /Journeys/itinerary`. See the module doc.
pub(crate) async fn post_itinerary(
    State(app): State<App>,
    user: DelegatedOrSessionUser<WriteScope>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let delegated = user.is_delegated();
    match save_itinerary(&app, &user, &headers, &body).await {
        Ok(response) => {
            // Usage analytics, phase 1 (DS-190): a planned trip saved.
            if response.status().is_success() {
                crate::usage_events::FeatureEvent::TripPlanSavedJourney.record();
            }
            response
        }
        Err(err) => err.render(delegated, "invalid_itinerary"),
    }
}

/// A validated leg, before a train leg's `trains` row is resolved.
#[derive(Debug)]
pub(crate) enum ValidLeg {
    Train {
        train_uid: String,
        service_date: NaiveDate,
        origin_crs: Option<String>,
        destination_crs: Option<String>,
    },
    /// Its stop ids are checked against `tfl_stops`, and its network ride
    /// against the timetable, when it is saved.
    Untracked {
        leg: journeys::UntrackedLeg,
        origin_stop_id: Option<String>,
        destination_stop_id: Option<String>,
        ride: Box<RideRequest>,
    },
}

/// An untracked leg's network ride as the body names it (DS-237),
/// checked by `journeys::attach_leg_timetable` when it is saved. Boxed in
/// [`ValidLeg`]: most legs have none.
#[derive(Debug)]
pub(crate) struct RideRequest {
    transit_network: Option<String>,
    transit_line_id: Option<String>,
    headsign: Option<String>,
}

fn invalid(message: impl Into<String>) -> RouteError {
    RouteError::coded(StatusCode::BAD_REQUEST, "invalid_itinerary", message)
}

/// The body's `operatorFilter`, parsed and checked: a `400` for an
/// unknown mode, a malformed or unknown entry (as `/Trips/plan` judges
/// them, against the train legs' service dates), or no entry at all.
async fn check_operator_filter(
    app: &App,
    request: Option<&crate::data::journey_operator_filter::OperatorFilterRequest>,
    legs: &[ValidLeg],
) -> Result<Option<crate::data::journey_operator_filter::JourneyOperatorFilter>, RouteError> {
    let Some(request) = request else {
        return Ok(None);
    };
    let filter = crate::data::journey_operator_filter::parse(request).map_err(invalid)?;
    let codes = filter.named_atoc_codes();
    if !codes.is_empty() {
        let mut dates: Vec<NaiveDate> = legs
            .iter()
            .filter_map(|leg| match leg {
                ValidLeg::Train { service_date, .. } => Some(*service_date),
                ValidLeg::Untracked { .. } => None,
            })
            .collect();
        dates.sort_unstable();
        dates.dedup();
        let known = crate::data::reference::known_operator_codes(&app.database, &codes, &dates)
            .await
            .map_err(internal("check operator codes"))?;
        let unknown: Vec<&str> = codes
            .iter()
            .filter(|code| !known.contains(*code))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(invalid(format!(
                "operatorFilter: {} {} not a known train operator code",
                unknown.join(", "),
                if unknown.len() == 1 { "is" } else { "are" }
            )));
        }
    }
    Ok(Some(filter))
}

/// Parses and validates the body: the leg count, each train leg's UID,
/// overrides and date horizon, each untracked leg's fields
/// (`journeys::validate_untracked_leg`), at least one train leg, and the
/// custom name.
fn validate(
    body: &[u8],
    max_legs: usize,
    london_today: NaiveDate,
) -> Result<(ItineraryRequest, Option<String>, Vec<ValidLeg>), RouteError> {
    let request: ItineraryRequest = serde_json::from_slice(body).map_err(|err| {
        invalid(format!(
            "The itinerary is not valid JSON of the expected shape: {err}"
        ))
    })?;
    if request.legs.is_empty() || request.legs.len() > max_legs {
        return Err(invalid(format!(
            "An itinerary needs between 1 and {max_legs} legs."
        )));
    }
    let custom_name =
        train_tracking::validate_custom_name(request.custom_name.as_deref()).map_err(invalid)?;
    let mut legs = Vec::with_capacity(request.legs.len());
    for leg in &request.legs {
        legs.push(validate_leg(leg, london_today)?);
    }
    if !legs.iter().any(|leg| matches!(leg, ValidLeg::Train { .. })) {
        return Err(invalid("An itinerary needs at least one train leg."));
    }
    Ok((request, custom_name, legs))
}

pub(crate) fn validate_leg(
    leg: &ItineraryLegRequest,
    london_today: NaiveDate,
) -> Result<ValidLeg, RouteError> {
    match leg.kind.as_deref() {
        None | Some(journeys::LEG_KIND_TRAIN) => {
            if leg.transit_mode.is_some()
                || leg.origin_name.is_some()
                || leg.destination_name.is_some()
                || leg.line_name.is_some()
                || leg.planned_departure.is_some()
                || leg.planned_arrival.is_some()
                || leg.origin_stop_id.is_some()
                || leg.destination_stop_id.is_some()
                || leg.transit_network.is_some()
                || leg.transit_line_id.is_some()
                || leg.headsign.is_some()
            {
                return Err(invalid(
                    "A train leg takes no transitMode, originName, destinationName, lineName, \
                     plannedDeparture, plannedArrival, originStopId, destinationStopId, \
                     transitNetwork, transitLineId or headsign.",
                ));
            }
            let (Some(train_uid), Some(service_date)) = (&leg.train_uid, leg.service_date) else {
                return Err(invalid("A train leg needs a trainUid and a serviceDate."));
            };
            journeys::validate_train_uid(train_uid).map_err(invalid)?;
            journeys::validate_known_train_overrides(
                leg.origin_crs.as_deref(),
                leg.destination_crs.as_deref(),
            )
            .map_err(invalid)?;
            crate::routes::train::enforce_tracking_horizon(service_date)
                .map_err(|(_, message)| invalid(message))?;
            Ok(ValidLeg::Train {
                train_uid: train_uid.trim().to_ascii_uppercase(),
                service_date,
                origin_crs: leg
                    .origin_crs
                    .as_deref()
                    .map(|crs| crs.trim().to_ascii_uppercase()),
                destination_crs: leg
                    .destination_crs
                    .as_deref()
                    .map(|crs| crs.trim().to_ascii_uppercase()),
            })
        }
        Some(journeys::LEG_KIND_UNTRACKED) => {
            if leg.train_uid.is_some()
                || leg.service_date.is_some()
                || leg.origin_crs.is_some()
                || leg.destination_crs.is_some()
            {
                return Err(invalid(
                    "An untracked leg takes no trainUid, serviceDate, originCrs or destinationCrs.",
                ));
            }
            let untracked = journeys::validate_untracked_leg(
                journeys::UntrackedLegInput {
                    transit_mode: leg.transit_mode.as_deref().unwrap_or_default(),
                    origin_name: leg.origin_name.as_deref(),
                    destination_name: leg.destination_name.as_deref(),
                    line_name: leg.line_name.as_deref(),
                    planned_departure: leg.planned_departure,
                    planned_arrival: leg.planned_arrival,
                },
                london_today,
            )
            .map_err(invalid)?;
            Ok(ValidLeg::Untracked {
                leg: untracked,
                origin_stop_id: leg.origin_stop_id.clone(),
                destination_stop_id: leg.destination_stop_id.clone(),
                ride: Box::new(RideRequest {
                    transit_network: leg.transit_network.clone(),
                    transit_line_id: leg.transit_line_id.clone(),
                    headsign: leg.headsign.clone(),
                }),
            })
        }
        Some(_) => Err(invalid("A leg's kind must be train or untracked.")),
    }
}

/// `422` for the first train leg whose train doesn't call at its
/// `originCrs` then its `destinationCrs` on its service date, or doesn't
/// let passengers on or off there ([`leg_validation::validate_train_leg`];
/// a leg without overrides, or a train with no published schedule, passes).
/// Untracked legs have no timetable and are skipped. The code is
/// [`leg_validation::LegCallError::code`]; with more than one leg the
/// message starts "Leg N: ", N counting every leg of the itinerary
/// (1-based). Runs after the idempotency claim, like the caps, so a replay
/// still gets its first answer.
pub(crate) async fn check_legs_call(app: &App, legs: &[ValidLeg]) -> Result<(), RouteError> {
    for (index, leg) in legs.iter().enumerate() {
        let ValidLeg::Train {
            train_uid,
            service_date,
            origin_crs,
            destination_crs,
        } = leg
        else {
            continue;
        };
        let rejection = leg_validation::validate_train_leg(
            &app.database,
            train_uid,
            *service_date,
            origin_crs.as_deref(),
            destination_crs.as_deref(),
        )
        .await
        .map_err(internal("check the train's calling points"))?;
        if let Some(rejection) = rejection {
            let code = rejection.code();
            let message = if legs.len() > 1 {
                format!("Leg {}: {}", index + 1, rejection.message)
            } else {
                rejection.message
            };
            return Err(RouteError::coded(
                StatusCode::UNPROCESSABLE_ENTITY,
                code,
                message,
            ));
        }
    }
    Ok(())
}

/// The `Idempotency-Key` header: `Ok(None)` when absent, refused when
/// present but empty, too long or not visible ASCII.
fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, RouteError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let key = value.to_str().unwrap_or_default().trim();
    if key.is_empty()
        || key.len() > MAX_IDEMPOTENCY_KEY_LENGTH
        || !key.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(RouteError::coded(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            format!(
                "Idempotency-Key must be 1 to {MAX_IDEMPOTENCY_KEY_LENGTH} visible ASCII characters."
            ),
        ));
    }
    Ok(Some(key.to_string()))
}

/// Each validated leg ready to save: a train leg's shared `trains` row
/// found or created, an untracked leg's `TfL` stops checked. Shared with
/// `POST /Journeys/{id}/legs/{legId}/replace`.
pub(crate) async fn resolve_legs(
    app: &App,
    legs: &[ValidLeg],
) -> Result<Vec<journeys::ItineraryLeg>, RouteError> {
    let mut resolved = Vec::with_capacity(legs.len());
    for leg in legs {
        match leg {
            ValidLeg::Untracked {
                leg,
                origin_stop_id,
                destination_stop_id,
                ride,
            } => {
                let leg = journeys::attach_untracked_stops(
                    &app.database,
                    leg.clone(),
                    origin_stop_id.as_deref(),
                    destination_stop_id.as_deref(),
                )
                .await
                .map_err(internal("check untracked leg stops"))?
                .map_err(invalid)?;
                let leg = journeys::attach_leg_timetable(
                    &app.database,
                    leg,
                    ride.transit_network.as_deref(),
                    ride.transit_line_id.as_deref(),
                    ride.headsign.as_deref(),
                )
                .await
                .map_err(internal("check the leg's timetable"))?
                .map_err(invalid)?;
                resolved.push(journeys::ItineraryLeg::Untracked(leg));
            }
            ValidLeg::Train {
                train_uid,
                service_date,
                origin_crs,
                destination_crs,
            } => {
                let trains_id = crate::data::trains::find_or_create_train(
                    &app.database,
                    train_uid,
                    *service_date,
                )
                .await
                .map_err(internal("find or create train"))?;
                resolved.push(journeys::ItineraryLeg::Train {
                    trains_id,
                    service_date: *service_date,
                    origin_crs: origin_crs.clone(),
                    destination_crs: destination_crs.clone(),
                });
            }
        }
    }
    Ok(resolved)
}

#[expect(
    clippy::too_many_lines,
    reason = "long but linear: validate, claim the key, check the caps, write, record"
)]
async fn save_itinerary(
    app: &App,
    user: &DelegatedOrSessionUser<WriteScope>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, RouteError> {
    let client = user.client().cloned();
    let key = idempotency_key(headers)?;
    if client.is_some() && key.is_none() {
        return Err(RouteError::coded(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "A delegated save needs an Idempotency-Key header.",
        ));
    }
    let (request, custom_name, legs) =
        validate(body, MAX_ITINERARY_LEGS, crate::routes::london_today())?;
    let operator_filter =
        check_operator_filter(app, request.operator_filter.as_ref(), &legs).await?;
    let request_hash = {
        let canonical =
            serde_json::to_vec(&request).map_err(|err| internal("hash request")(err.into()))?;
        let digest = Sha256::digest(&canonical);
        format!("{digest:x}")
    };

    let mut tx = app
        .database
        .begin()
        .await
        .map_err(|err| internal("start transaction")(err.into()))?;
    if let Some(key) = &key {
        match crate::data::delegated::claim_idempotency_key(
            &mut tx,
            &user.user_id,
            key,
            &request_hash,
        )
        .await
        .map_err(internal("claim idempotency key"))?
        {
            IdempotencyClaim::Fresh => {}
            IdempotencyClaim::Replay(response) => {
                return Ok(axum::Json(response).into_response());
            }
            IdempotencyClaim::Conflict => {
                return Err(RouteError::coded(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "This Idempotency-Key was already used for a different itinerary.",
                ));
            }
        }
    }

    // The caps, after the key, so a replay always gets its first answer.
    let owned = journeys::count_manual_journeys_for_user(&app.database, &user.user_id)
        .await
        .map_err(internal("count journeys"))?;
    if owned >= journeys::MAX_JOURNEYS_PER_USER {
        return Err(invalid(format!(
            "You already have {} journeys, which is the maximum. Delete some old ones to make \
             room for a new one.",
            journeys::MAX_JOURNEYS_PER_USER
        )));
    }
    if let Some(client) = &client {
        let (saved, oldest) = journeys::count_journeys_created_via_last_day(
            &app.database,
            &user.user_id,
            &client.name,
        )
        .await
        .map_err(internal("count delegated saves"))?;
        if saved >= app.delegated.daily_save_limit {
            let frees_at = oldest.unwrap_or_else(chrono::Utc::now) + chrono::Duration::hours(24);
            return Err(RouteError::Coded {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "daily_limit",
                message: format!(
                    "{} journeys were saved this way in the last 24 hours, which is the daily limit.",
                    app.delegated.daily_save_limit
                ),
                retry_after_secs: Some((frees_at - chrono::Utc::now()).num_seconds()),
            });
        }
    }
    // The pin cap, counting each distinct train this itinerary would newly
    // track (an already-tracked train reuses its subscription).
    let mut new_trains: Vec<(&str, NaiveDate)> = Vec::new();
    for leg in &legs {
        let ValidLeg::Train {
            train_uid,
            service_date,
            ..
        } = leg
        else {
            continue;
        };
        let pair = (train_uid.as_str(), *service_date);
        if new_trains.contains(&pair) {
            continue;
        }
        let tracked = train_tracking::user_tracks_train_uid(
            &app.database,
            &user.user_id,
            train_uid,
            *service_date,
        )
        .await
        .map_err(internal("check existing subscription"))?;
        if !tracked {
            new_trains.push(pair);
        }
    }
    if !new_trains.is_empty() {
        let upcoming =
            train_tracking::count_future_subscriptions_for_user(&app.database, &user.user_id)
                .await
                .map_err(internal("count tracked trains"))?;
        let new = i64::try_from(new_trains.len()).unwrap_or(i64::MAX);
        if upcoming.saturating_add(new) > train_tracking::MAX_FUTURE_PINS_PER_USER {
            return Err(invalid(train_tracking::pin_cap_message()));
        }
    }

    check_legs_call(app, &legs).await?;
    let resolved = resolve_legs(app, &legs).await?;
    let created_via = client
        .as_ref()
        .map_or_else(|| "web".to_string(), |client| client.name.clone());
    let (journey_id, created) = journeys::create_journey_from_itinerary(
        &mut tx,
        &user.user_id,
        custom_name.as_deref(),
        &created_via,
        &resolved,
    )
    .await
    .map_err(internal("create journey from itinerary"))?;
    if let Some(filter) = &operator_filter {
        crate::data::journey_operator_filter::store(
            &mut *tx,
            journey_id,
            &user.user_id,
            Some(filter),
        )
        .await
        .map_err(internal("store the operator filter"))?;
    }
    let (Some(&(first_leg_id, _)), Some(first_tracking_id)) = (
        created.first(),
        created.iter().find_map(|&(_, tracking_id)| tracking_id),
    ) else {
        return Err(internal("create journey from itinerary")(anyhow::anyhow!(
            "an itinerary with a train leg created none"
        )));
    };
    let response = ItineraryResponse {
        journey_id,
        leg_id: first_leg_id,
        tracking_id: first_tracking_id,
        resolution_status: None,
        legs: created
            .iter()
            .map(|&(leg_id, tracking_id)| ItineraryLegResponse {
                leg_id,
                tracking_id,
                kind: if tracking_id.is_some() {
                    journeys::LEG_KIND_TRAIN
                } else {
                    journeys::LEG_KIND_UNTRACKED
                }
                .to_string(),
            })
            .collect(),
        created_via,
    };
    let response_json =
        serde_json::to_value(&response).map_err(|err| internal("encode response")(err.into()))?;
    if let Some(key) = &key {
        crate::data::delegated::record_idempotent_response(
            &mut tx,
            &user.user_id,
            key,
            &response_json,
        )
        .await
        .map_err(internal("record idempotency key"))?;
    }
    tx.commit()
        .await
        .map_err(|err| internal("commit itinerary")(err.into()))?;

    // Best effort, after the commit, as `POST /Journeys` does per leg.
    for ((leg, resolved), &(_, tracking_id)) in legs.iter().zip(&resolved).zip(&created) {
        let (
            ValidLeg::Train {
                train_uid,
                service_date,
                ..
            },
            journeys::ItineraryLeg::Train { trains_id, .. },
            Some(tracking_id),
        ) = (leg, resolved, tracking_id)
        else {
            continue;
        };
        crate::routes::train::enrich_shared_train(
            app,
            tracking_id,
            *trains_id,
            train_uid,
            *service_date,
        )
        .await;
    }
    if let Some(client) = &client {
        tracing::info!(
            audit = true,
            event = "delegated_journey_saved",
            client = %client.name,
            user_id = %user.user_id,
            journey_id,
            legs = created.len(),
            untracked_legs = created.iter().filter(|(_, tracking_id)| tracking_id.is_none()).count(),
            "a delegated client saved a journey"
        );
    }
    Ok(axum::Json(response_json).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_strict_camel_case() {
        let body = br#"{"customName":"Trip","legs":[{"trainUid":"c12345","serviceDate":"2026-10-20","originCrs":"cre"}]}"#;
        let request: ItineraryRequest = serde_json::from_slice(body).unwrap();
        assert_eq!(request.legs.len(), 1);
        assert_eq!(request.legs[0].origin_crs.as_deref(), Some("cre"));
        assert!(
            serde_json::from_slice::<ItineraryRequest>(
                br#"{"legs":[{"trainUid":"C12345","serviceDate":"2026-10-20","mode":"knownTrain"}]}"#
            )
            .is_err(),
            "unknown fields are refused"
        );
        let response: ItineraryLegResponse =
            serde_json::from_str(r#"{"legId":1,"trackingId":2}"#).unwrap();
        assert_eq!(
            response.kind, "train",
            "a response stored before untracked legs reads as a train leg"
        );
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    }

    #[test]
    fn leg_counts_are_bounded() {
        assert_eq!(
            i64::try_from(MAX_ITINERARY_LEGS).unwrap(),
            journeys::MAX_LEGS_PER_JOURNEY
        );
        let leg = r#"{"trainUid":"C12345","serviceDate":"2026-10-21"}"#;
        let body = |n: usize| format!(r#"{{"legs":[{}]}}"#, vec![leg; n].join(","));
        assert!(matches!(
            validate(body(0).as_bytes(), MAX_ITINERARY_LEGS, today()),
            Err(RouteError::Coded {
                code: "invalid_itinerary",
                ..
            })
        ));
        assert!(matches!(
            validate(body(21).as_bytes(), MAX_ITINERARY_LEGS, today()),
            Err(RouteError::Coded {
                code: "invalid_itinerary",
                ..
            })
        ));
    }

    /// The idempotency hash is over this serialisation, and a key claimed
    /// before untracked legs existed must replay after the deploy: a train
    /// leg serialises exactly as the old four-field struct did.
    #[test]
    fn a_body_without_an_operator_filter_hashes_as_before_and_one_with_it_differs() {
        let body = br#"{"legs":[{"trainUid":"c12345","serviceDate":"2026-10-20"}]}"#;
        let request: ItineraryRequest = serde_json::from_slice(body).unwrap();
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"customName":null,"legs":[{"trainUid":"c12345","serviceDate":"2026-10-20","originCrs":null,"destinationCrs":null}]}"#
        );
        let with: ItineraryRequest = serde_json::from_slice(
            br#"{"legs":[{"trainUid":"c12345","serviceDate":"2026-10-20"}],"operatorFilter":{"mode":"only","entries":["VT"]}}"#,
        )
        .unwrap();
        assert!(
            serde_json::to_string(&with)
                .unwrap()
                .ends_with(r#""operatorFilter":{"mode":"only","entries":["VT"]}}"#)
        );
        assert!(
            serde_json::from_slice::<ItineraryRequest>(
                br#"{"legs":[],"operatorFilter":{"mode":"only","entries":["VT"],"extra":1}}"#
            )
            .is_err(),
            "unknown fields are refused"
        );
    }

    #[test]
    fn a_train_leg_hashes_as_it_did_before_untracked_legs() {
        let body = br#"{"customName":"Trip","legs":[{"originCrs":"cre","serviceDate":"2026-10-20","trainUid":"c12345"}]}"#;
        let request: ItineraryRequest = serde_json::from_slice(body).unwrap();
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"customName":"Trip","legs":[{"trainUid":"c12345","serviceDate":"2026-10-20","originCrs":"cre","destinationCrs":null}]}"#
        );
    }

    fn untracked_leg(extra: &str) -> String {
        format!(
            r#"{{"kind":"untracked","transitMode":"tube","lineName":"Victoria line","originName":"Euston","destinationName":"Victoria","plannedDeparture":"2026-10-21T10:05:00+01:00","plannedArrival":"2026-10-21T10:20:00+01:00"{extra}}}"#
        )
    }

    #[test]
    fn an_untracked_leg_validates_beside_a_train_leg() {
        let body = format!(
            r#"{{"legs":[{},{{"trainUid":"c12345","serviceDate":"2026-10-21"}}]}}"#,
            untracked_leg("")
        );
        let (_, _, legs) = validate(body.as_bytes(), MAX_ITINERARY_LEGS, today()).unwrap();
        let ValidLeg::Untracked { leg: untracked, .. } = &legs[0] else {
            panic!("the first leg is untracked");
        };
        assert_eq!(untracked.transit_mode, "tube");
        assert_eq!(untracked.line_name.as_deref(), Some("Victoria line"));
        assert_eq!(
            untracked.planned_departure,
            "2026-10-21T09:05:00Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        );
        assert!(matches!(legs[1], ValidLeg::Train { .. }));
    }

    #[test]
    fn untracked_legs_are_refused_when_malformed_or_alone() {
        let message = |body: String| match validate(body.as_bytes(), MAX_ITINERARY_LEGS, today()) {
            Err(RouteError::Coded { message, .. }) => message,
            other => panic!("expected a 400, got {other:?}"),
        };
        assert_eq!(
            message(format!(r#"{{"legs":[{}]}}"#, untracked_leg(""))),
            "An itinerary needs at least one train leg."
        );
        assert!(
            message(format!(
                r#"{{"legs":[{},{{"trainUid":"C12345","serviceDate":"2026-10-21"}}]}}"#,
                untracked_leg(r#","trainUid":"C99999""#)
            ))
            .starts_with("An untracked leg takes no trainUid")
        );
        assert!(
            message(
                r#"{"legs":[{"trainUid":"C12345","serviceDate":"2026-10-21","transitMode":"bus"}]}"#
                    .to_string()
            )
            .starts_with("A train leg takes no transitMode")
        );
        assert_eq!(
            message(r#"{"legs":[{"originCrs":"WOK"}]}"#.to_string()),
            "A train leg needs a trainUid and a serviceDate."
        );
        assert_eq!(
            message(
                r#"{"legs":[{"kind":"boat","trainUid":"C12345","serviceDate":"2026-10-21"}]}"#
                    .to_string()
            ),
            "A leg's kind must be train or untracked."
        );
        assert!(
            message(format!(
                r#"{{"legs":[{},{{"trainUid":"C12345","serviceDate":"2026-10-21"}}]}}"#,
                untracked_leg("").replace("tube", "hovercraft")
            ))
            .starts_with("transitMode must be one of")
        );
    }

    #[test]
    fn idempotency_keys_are_checked() {
        let mut headers = HeaderMap::new();
        assert!(idempotency_key(&headers).unwrap().is_none());
        headers.insert("idempotency-key", HeaderValue::from_static("abc-123"));
        assert_eq!(
            idempotency_key(&headers).unwrap().as_deref(),
            Some("abc-123")
        );
        headers.insert("idempotency-key", HeaderValue::from_static("   "));
        assert!(idempotency_key(&headers).is_err());
        let long = "k".repeat(MAX_IDEMPOTENCY_KEY_LENGTH + 1);
        headers.insert("idempotency-key", HeaderValue::from_str(&long).unwrap());
        assert!(idempotency_key(&headers).is_err());
    }

    #[test]
    fn errors_render_per_caller() {
        let err = || RouteError::coded(StatusCode::CONFLICT, "idempotency_conflict", "Used.");
        let delegated = err().render(true, "invalid_itinerary");
        assert_eq!(delegated.status(), StatusCode::CONFLICT);
        assert_eq!(
            delegated.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let web = err().render(false, "invalid_itinerary");
        assert_eq!(web.status(), StatusCode::CONFLICT);
        assert!(
            web.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/plain")
        );
    }
}
