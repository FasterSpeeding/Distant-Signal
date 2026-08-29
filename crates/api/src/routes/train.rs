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
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::{TicketEntryRequest, TrackPinRequest};
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{eta_blend, ticket_extraction, train_tracking, delay_repay_rules};

pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
        .route("/Train/{tracking_id}/tickets", axum::routing::post(post_ticket).get(get_tickets))
        .route("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay", axum::routing::get(get_delay_repay_estimate))
        .route("/Train/{tracking_id}/tickets/pkpass", axum::routing::post(post_pkpass_upload))
        .route("/Train/{tracking_id}/tickets/pdf", axum::routing::post(post_pdf_upload))
        // 8 MiB: generous for a real boarding pass or e-ticket PDF (both
        // are typically tens of KB to low single-digit MB), bounded
        // against abuse. Applies to every route on this router, including
        // the small-JSON ones above -- harmless headroom for those, load-
        // bearing for the two upload routes (this one and Task 9's PDF
        // route). See this plan's Global Constraints on file upload
        // hygiene.
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackPinResponse {
    tracking_id: i64,
    resolution_status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TicketCreatedResponse {
    ticket_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DelayRepayEstimateResponse {
    delay_minutes: Option<i32>,
    estimate: Option<delay_repay_rules::DelayRepayEstimate>,
    // Always populated, independent of whether `estimate` is `Some` --
    // this route must never leave a caller with a bare percentage and no
    // caveat, or with nowhere real to go. See this plan's Global
    // Constraints.
    claim_url: String,
    disclaimer: &'static str,
}

const DELAY_REPAY_ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";

async fn post_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
    Json(entry): Json<TicketEntryRequest>,
) -> Result<Json<TicketCreatedResponse>, (StatusCode, String)> {
    train_tracking::validate_ticket_entry(&entry).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let ticket_id = train_tracking::create_ticket(&app.database, tracking_id, &entry, &user.id)
        .await
        .map_err(internal_error("create ticket"))?;

    Ok(Json(TicketCreatedResponse { ticket_id }))
}

async fn get_tickets(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<Json<Vec<train_tracking::TrackedTrainTicket>>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let tickets = train_tracking::list_tickets_for_tracked_train(&app.database, tracking_id, &user.id)
        .await
        .map_err(internal_error("list tickets"))?;
    Ok(Json(tickets))
}

async fn get_delay_repay_estimate(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((tracking_id, ticket_id)): Path<(i64, i64)>,
) -> Result<Json<DelayRepayEstimateResponse>, (StatusCode, String)> {
    let ticket = train_tracking::get_ticket_owned(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("read ticket"))?
        .filter(|t| t.tracked_train_id == tracking_id)
        .ok_or((StatusCode::NOT_FOUND, "no ticket with that id for that tracked train".to_string()))?;

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?
        .ok_or((StatusCode::NOT_FOUND, "no tracked train with that id".to_string()))?;

    Ok(Json(build_delay_repay_response(&ticket, &state)))
}

/// Pure response assembly for `get_delay_repay_estimate`, extracted out of
/// the handler so it's unit-testable without a `PgPool`/`App` at all --
/// deliberately given no I/O capability of any kind, consistent with this
/// whole feature's "the estimator's own call sites stay provably
/// read-only/pure" posture (see `delay_repay_rules`'s module doc).
fn build_delay_repay_response(
    ticket: &train_tracking::TrackedTrainTicket,
    state: &train_tracking::TrackedTrainState,
) -> DelayRepayEstimateResponse {
    let estimate = match (ticket.operator.as_deref(), state.delay_minutes) {
        (Some(operator), Some(delay_minutes)) => delay_repay_rules::estimate_delay_repay(operator, delay_minutes),
        _ => None,
    };
    let claim_url = ticket.operator.as_deref().map(delay_repay_rules::claim_url_for).unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

    DelayRepayEstimateResponse {
        delay_minutes: state.delay_minutes,
        estimate,
        claim_url: claim_url.to_string(),
        disclaimer: DELAY_REPAY_ROUTE_DISCLAIMER,
    }
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

/// `tracking_id` is accepted in the path for URL-shape consistency with
/// every other `/Train/{trackingId}/tickets/...` route, and `_user`
/// requires the caller be logged in (no anonymous file-parsing endpoint --
/// see this plan's Global Constraints), but neither is otherwise used:
/// this handler reads and writes no `tracked_train_id`-scoped row at all.
/// It parses an uploaded file and returns a preview; the tracking id only
/// matters to the client's later, separate confirm request
/// (`POST /Train/{trackingId}/tickets`, Task 3).
///
/// REVIEW-BEFORE-SAVE, structurally: this function contains no
/// `sqlx::query` call and touches no database handle -- there is nothing
/// in this file that could accidentally persist an unreviewed upload. See
/// this plan's Global Constraints.
async fn post_pkpass_upload(
    _user: AuthenticatedUser,
    Path(_tracking_id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;
    ticket_extraction::parse_pkpass(&bytes)
        .map(Json)
        .map_err(|err| (StatusCode::UNPROCESSABLE_ENTITY, format!("could not read this as a train .pkpass: {err}")))
}

/// Same contract as `post_pkpass_upload` (Task 7) -- see that handler's
/// doc comment for why `_user`/`_tracking_id` are otherwise unused, and
/// the same REVIEW-BEFORE-SAVE note: no `sqlx::query` call, no database
/// handle, anywhere in this function.
///
/// Unlike `.pkpass` parsing (a bounded zip-entry read), `ticket_extraction::parse_pdf`
/// runs the third-party `pdf_extract` crate over untrusted, potentially
/// pathological PDF bytes with no time bound of its own -- CPU-bound,
/// synchronous work that would otherwise stall a tokio worker thread for
/// the whole API, not just this route, if called directly from this async
/// handler. It's pushed onto a blocking-pool thread via `spawn_blocking`
/// and given a hard wall-clock budget via `timeout` so a pathological
/// upload degrades to one failed request, not a stuck worker thread.
async fn post_pdf_upload(
    _user: AuthenticatedUser,
    Path(_tracking_id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;

    let parsed = tokio::time::timeout(PDF_PARSE_TIMEOUT, tokio::task::spawn_blocking(move || ticket_extraction::parse_pdf(&bytes))).await;

    match parsed {
        Ok(Ok(Ok(ticket))) => Ok(Json(ticket)),
        Ok(Ok(Err(err))) => Err((StatusCode::UNPROCESSABLE_ENTITY, format!("could not read this as a PDF e-ticket: {err}"))),
        Ok(Err(join_err)) => {
            tracing::error!(error = ?join_err, "PDF parse task panicked or was cancelled");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "failed to parse PDF e-ticket".to_string()))
        }
        Err(_elapsed) => {
            Err((StatusCode::GATEWAY_TIMEOUT, "PDF e-ticket parsing took too long; try a smaller or simpler file".to_string()))
        }
    }
}

/// Wall-clock budget for a single PDF's text extraction (Finding 2 of the
/// final review of this plan) -- generous for any legitimate ticket PDF
/// (typically well under a second), bounded against a pathological upload
/// tying up a blocking-pool thread indefinitely.
const PDF_PARSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Shared by this route and Task 9's PDF upload route: reads the single
/// multipart field named `field_name` (expected to be `"file"` for both)
/// into memory and returns its raw bytes.
async fn read_single_file_field(multipart: &mut Multipart, field_name: &str) -> Result<Vec<u8>, (StatusCode, String)> {
    while let Some(field) = multipart.next_field().await.map_err(|err| (StatusCode::BAD_REQUEST, format!("malformed upload: {err}")))? {
        if field.name() == Some(field_name) {
            let bytes = field.bytes().await.map_err(|err| (StatusCode::BAD_REQUEST, format!("failed to read upload: {err}")))?;
            return Ok(bytes.to_vec());
        }
    }
    Err((StatusCode::BAD_REQUEST, format!("no '{field_name}' field in upload")))
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;

    // Fixed instants for the `created_at`/`eta_next` fields below -- their
    // exact values are irrelevant to every test in this module, only their
    // presence/absence (`Option`-ness) is.
    fn fixed_instant() -> DateTime<Utc> {
        "2026-08-29T12:00:00Z".parse().unwrap()
    }

    fn ticket(operator: Option<&str>) -> train_tracking::TrackedTrainTicket {
        train_tracking::TrackedTrainTicket {
            id: 1,
            tracked_train_id: 1,
            operator: operator.map(str::to_string),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
            created_at: fixed_instant(),
        }
    }

    fn state(delay_minutes: Option<i32>) -> train_tracking::TrackedTrainState {
        train_tracking::TrackedTrainState {
            id: 1,
            service_date: "2026-08-29".parse().unwrap(),
            pin_origin_crs: "KGX".to_string(),
            pin_destination_crs: Some("EDB".to_string()),
            resolution_status: "resolved".to_string(),
            train_uid: Some("A12345".to_string()),
            train_id: Some("1A23".to_string()),
            status: Some("late".to_string()),
            last_reported_location: Some("York".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes,
            next_calling_point: Some("Newcastle".to_string()),
            eta_next: Some(fixed_instant()),
            eta_source: Some("darwin-estimated".to_string()),
        }
    }

    /// axum's `Router::route` panics synchronously, at construction time,
    /// on a route-table conflict it can't disambiguate -- this test exists
    /// purely to catch that class of bug at `cargo test` time instead of at
    /// production startup.
    #[test]
    fn router_builds_without_panicking() {
        let _ = router();
    }

    #[test]
    fn dr30_operator_with_a_qualifying_delay_gets_a_specific_estimate_and_claim_url() {
        let response = build_delay_repay_response(&ticket(Some("LNER")), &state(Some(45)));

        let estimate = response.estimate.expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, 50);
        assert_eq!(response.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(response.delay_minutes, Some(45));
    }

    #[test]
    fn no_operator_on_the_ticket_yields_no_estimate_but_still_a_real_claim_link_and_disclaimer() {
        let response = build_delay_repay_response(&ticket(None), &state(Some(45)));

        assert_eq!(response.estimate, None);
        assert_eq!(response.claim_url, delay_repay_rules::GENERIC_CLAIM_URL);
        assert!(!response.disclaimer.is_empty());
    }

    #[test]
    fn an_unresolved_delay_yields_no_estimate_but_claim_url_and_disclaimer_are_still_populated() {
        // Safety property #3: a caller must never see a bare/absent
        // caveat, even when the train hasn't resolved/reported a delay yet.
        let response = build_delay_repay_response(&ticket(Some("LNER")), &state(None));

        assert_eq!(response.estimate, None);
        assert_eq!(response.delay_minutes, None);
        assert_eq!(response.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert!(!response.disclaimer.is_empty());
    }
}
