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
//! own leg cap.
//!
//! Body (camelCase, unknown fields refused):
//!
//! ```json
//! {"customName": "Optional", "legs": [
//!   {"trainUid": "C12345", "serviceDate": "2026-10-20",
//!    "originCrs": "CRE", "destinationCrs": "PRE"}]}
//! ```
//!
//! `originCrs`/`destinationCrs` are optional, as for `POST /Journeys`'s
//! `knownTrain` leg. A repeat with the same key and the same body returns
//! the first response (200); the same key with a different body is 409
//! `idempotency_conflict`. Keys are remembered per user for 24 hours.

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
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItineraryLegRequest {
    train_uid: String,
    service_date: NaiveDate,
    #[serde(default)]
    origin_crs: Option<String>,
    #[serde(default)]
    destination_crs: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItineraryLegResponse {
    leg_id: i64,
    tracking_id: i64,
}

/// A superset of `POST /Journeys`' response, so the web planner's
/// `onCreated` handler takes either: `legId`/`trackingId` are the first
/// leg's.
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
        Ok(response) => response,
        Err(err) => err.render(delegated, "invalid_itinerary"),
    }
}

/// A validated leg, before its `trains` row is resolved.
struct ValidLeg {
    train_uid: String,
    service_date: NaiveDate,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
}

fn invalid(message: impl Into<String>) -> RouteError {
    RouteError::coded(StatusCode::BAD_REQUEST, "invalid_itinerary", message)
}

/// Parses and validates the body: the leg count for this caller, each
/// leg's train UID, overrides and date horizon, and the custom name.
fn validate(
    body: &[u8],
    max_legs: usize,
) -> Result<(ItineraryRequest, Option<String>, Vec<ValidLeg>), RouteError> {
    let request: ItineraryRequest = serde_json::from_slice(body).map_err(|err| {
        invalid(format!(
            "The itinerary is not valid JSON of the expected shape: {err}"
        ))
    })?;
    if request.legs.is_empty() || request.legs.len() > max_legs {
        return Err(invalid(format!(
            "An itinerary needs between 1 and {max_legs} train legs."
        )));
    }
    let custom_name =
        train_tracking::validate_custom_name(request.custom_name.as_deref()).map_err(invalid)?;
    let mut legs = Vec::with_capacity(request.legs.len());
    for leg in &request.legs {
        journeys::validate_train_uid(&leg.train_uid).map_err(invalid)?;
        journeys::validate_known_train_overrides(
            leg.origin_crs.as_deref(),
            leg.destination_crs.as_deref(),
        )
        .map_err(invalid)?;
        crate::routes::train::enforce_tracking_horizon(leg.service_date)
            .map_err(|(_, message)| invalid(message))?;
        legs.push(ValidLeg {
            train_uid: leg.train_uid.trim().to_ascii_uppercase(),
            service_date: leg.service_date,
            origin_crs: leg
                .origin_crs
                .as_deref()
                .map(|crs| crs.trim().to_ascii_uppercase()),
            destination_crs: leg
                .destination_crs
                .as_deref()
                .map(|crs| crs.trim().to_ascii_uppercase()),
        });
    }
    Ok((request, custom_name, legs))
}

/// `422` for the first leg whose train doesn't call at its `originCrs` then
/// its `destinationCrs` on its service date, or doesn't let passengers on
/// or off there ([`leg_validation::validate_train_leg`]; a leg without
/// overrides, or a train with no published schedule, passes). The code is
/// [`leg_validation::LegCallError::code`]; with more than one leg the
/// message starts "Leg N: " (1-based). Runs after the idempotency claim, like
/// the caps, so a replay still gets its first answer.
async fn check_legs_call(app: &App, legs: &[ValidLeg]) -> Result<(), RouteError> {
    for (index, leg) in legs.iter().enumerate() {
        let rejection = leg_validation::validate_train_leg(
            &app.database,
            &leg.train_uid,
            leg.service_date,
            leg.origin_crs.as_deref(),
            leg.destination_crs.as_deref(),
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
    let (request, custom_name, legs) = validate(body, MAX_ITINERARY_LEGS)?;
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
        let pair = (leg.train_uid.as_str(), leg.service_date);
        if new_trains.contains(&pair) {
            continue;
        }
        let tracked = train_tracking::user_tracks_train_uid(
            &app.database,
            &user.user_id,
            &leg.train_uid,
            leg.service_date,
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

    let mut resolved = Vec::with_capacity(legs.len());
    for leg in &legs {
        let trains_id = crate::data::trains::find_or_create_train(
            &app.database,
            &leg.train_uid,
            leg.service_date,
        )
        .await
        .map_err(internal("find or create train"))?;
        resolved.push(journeys::ItineraryLeg {
            trains_id,
            service_date: leg.service_date,
            origin_crs: leg.origin_crs.clone(),
            destination_crs: leg.destination_crs.clone(),
        });
    }
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
    let Some(&(first_leg_id, first_tracking_id)) = created.first() else {
        return Err(internal("create journey from itinerary")(anyhow::anyhow!(
            "an itinerary with legs created none"
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
        crate::routes::train::enrich_shared_train(
            app,
            tracking_id,
            resolved.trains_id,
            &leg.train_uid,
            leg.service_date,
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
    }

    #[test]
    fn leg_counts_are_bounded() {
        assert_eq!(
            i64::try_from(MAX_ITINERARY_LEGS).unwrap(),
            journeys::MAX_LEGS_PER_JOURNEY
        );
        let leg = r#"{"trainUid":"C12345","serviceDate":"2099-01-01"}"#;
        let body = |n: usize| format!(r#"{{"legs":[{}]}}"#, vec![leg; n].join(","));
        assert!(matches!(
            validate(body(0).as_bytes(), MAX_ITINERARY_LEGS),
            Err(RouteError::Coded {
                code: "invalid_itinerary",
                ..
            })
        ));
        assert!(matches!(
            validate(body(21).as_bytes(), MAX_ITINERARY_LEGS),
            Err(RouteError::Coded {
                code: "invalid_itinerary",
                ..
            })
        ));
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
