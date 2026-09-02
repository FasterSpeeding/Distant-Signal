//! `/Train/...`: individual train tracking. Pin *creation* requires an
//! authenticated session (`AuthenticatedUser`, from
//! docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 6) --
//! every tracked train has a real owner from birth, per that plan's
//! coordination fix to this one. State *reads* (`get_by_tracking_id`,
//! `get_by_uid_and_date`) originally stayed unauthenticated/unscoped per
//! Task 5's note on why that wasn't a strict "everything private" posture
//! -- since retrofitted to require the caller own the pin (see the
//! 2026-08-31 private-custom-lines-and-tracked-trains plan's Task 8; same
//! 404-for-both-"missing"-and-"not-yours" convention as every other
//! ownership check in this app, never `403`). Mounted directly (not under
//! `/public`) to match the design doc's sketched URL shape for the
//! eventual frontend page.

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::{TicketEntryRequest, TrackPinRequest};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{eta_blend, ticket_extraction, train_tracking, delay_repay_rules};

pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/mine", axum::routing::get(get_my_tracked_trains))
        // Literal segments under `/Train/tickets/...`, coexisting with the
        // dynamic `/Train/{tracking_id}/tickets/...` family below by the
        // same literal-beats-same-position-dynamic precedent already
        // established for `/Train/mine` vs `/Train/{tracking_id}` (see
        // `literal_route_wins_over_same_position_dynamic_route`, this
        // file's own test module). `/Train/tickets` (no ownership check
        // needed, unlike `/Train/{tracking_id}/tickets` -- there's no
        // tracking id to own yet) creates a STANDALONE ticket, per this
        // plan's upload-first flow; `/Train/tickets/pkpass`/`/pdf` are the
        // same preview-only parse as their `{tracking_id}`-scoped
        // siblings, just reachable before a tracked train exists;
        // `/Train/tickets/{ticket_id}/attach` is how a standalone ticket
        // later gets a `tracked_train_id`.
        .route("/Train/tickets", axum::routing::post(post_standalone_ticket))
        .route("/Train/tickets/mine", axum::routing::get(get_my_tickets))
        .route("/Train/tickets/pkpass", axum::routing::post(post_pkpass_upload_standalone))
        .route("/Train/tickets/pdf", axum::routing::post(post_pdf_upload_standalone))
        .route("/Train/tickets/{ticket_id}/attach", axum::routing::post(post_attach_ticket))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id).delete(delete_tracked_train))
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

    let ticket_id = train_tracking::create_ticket(&app.database, Some(tracking_id), &entry, &user.id)
        .await
        .map_err(internal_error("create ticket"))?;

    Ok(Json(TicketCreatedResponse { ticket_id }))
}

/// Creates a STANDALONE ticket -- no `tracked_train_id` at all, the
/// upload-first flow this plan adds. No ownership check needed (unlike
/// `post_ticket` above): there's no existing tracking id to own yet, the
/// caller just needs to be logged in (`AuthenticatedUser`, same as every
/// other write in this file). Attach it to a tracked train later via
/// `post_attach_ticket`, once the caller has found or created the one this
/// ticket is actually for.
async fn post_standalone_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(entry): Json<TicketEntryRequest>,
) -> Result<Json<TicketCreatedResponse>, (StatusCode, String)> {
    train_tracking::validate_ticket_entry(&entry).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let ticket_id = train_tracking::create_ticket(&app.database, None, &entry, &user.id)
        .await
        .map_err(internal_error("create ticket"))?;

    Ok(Json(TicketCreatedResponse { ticket_id }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachTicketRequest {
    tracking_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachTicketResponse {
    ticket_id: i64,
    tracked_train_id: i64,
}

/// Attaches an existing standalone ticket (`post_standalone_ticket`,
/// above) to a tracked train the caller owns -- the step that closes the
/// upload-first loop: upload/enter a ticket, then find or create the
/// tracked train it's actually for, then attach. Ownership-scoped on BOTH
/// sides (the ticket and the tracked train must belong to the same caller)
/// -- never `403`, matching this file's universal "exists but not yours ->
/// 404" convention. `409 Conflict` is the one new status this route
/// introduces: a ticket that's already attached to a (possibly different)
/// tracked train is a real, distinguishable state, not an ownership
/// failure, so it gets its own status rather than folding into the `404`
/// family.
async fn post_attach_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(ticket_id): Path<i64>,
    Json(body): Json<AttachTicketRequest>,
) -> Result<Json<AttachTicketResponse>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, body.tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let ticket = train_tracking::get_ticket_owned(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("read ticket"))?
        .ok_or((StatusCode::NOT_FOUND, "no ticket with that id".to_string()))?;

    if ticket.tracked_train_id.is_some() {
        return Err((StatusCode::CONFLICT, "ticket is already attached to a tracked train".to_string()));
    }

    let attached = train_tracking::attach_ticket_to_tracked_train(&app.database, ticket_id, body.tracking_id, &user.id)
        .await
        .map_err(internal_error("attach ticket"))?;
    if !attached {
        // Lost a race against a concurrent attach/re-check between the
        // reads above and this write -- treat it the same as the
        // already-attached case above rather than a 500, since that's
        // exactly what it now is.
        return Err((StatusCode::CONFLICT, "ticket is already attached to a tracked train".to_string()));
    }

    Ok(Json(AttachTicketResponse { ticket_id, tracked_train_id: body.tracking_id }))
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
        .filter(|t| t.tracked_train_id == Some(tracking_id))
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
        disclaimer: delay_repay_rules::ROUTE_DISCLAIMER,
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

/// Always `200` with a (possibly empty) array for any authenticated
/// caller -- never `404`, unlike the ticket routes' "exists but not
/// yours -> 404" convention (Decision 1 of the design spec). There's no
/// id in the URL to be wrong about: the only two real outcomes are
/// "logged in, here's your list" and "not logged in, bare 401" (handled
/// by the `AuthenticatedUser` extractor itself, before this function
/// runs) -- matching `post_track`'s own two-outcome shape more closely
/// than the ticket routes' three-outcome one.
async fn get_my_tracked_trains(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<train_tracking::TrackedTrainListItem>>, (StatusCode, String)> {
    let trains = train_tracking::list_tracked_trains_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list tracked trains"))?;
    Ok(Json(trains))
}

/// Always `200` with a (possibly empty) array for any authenticated
/// caller -- never `404`, matching `GET /Train/mine`'s own two-outcome
/// shape more closely than the per-ticket routes' three-outcome ("exists
/// but not yours" -> 404) shape. There's no id in this route's path to be
/// wrong about: the only two real outcomes are "logged in, here's your
/// list" and "not logged in, bare 401" (handled by the `AuthenticatedUser`
/// extractor itself, before this function runs).
async fn get_my_tickets(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<train_tracking::TicketListItem>>, (StatusCode, String)> {
    let tickets = train_tracking::list_tickets_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list tickets"))?;
    Ok(Json(tickets))
}

async fn get_by_tracking_id(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?;
    match state {
        Some(state) => Ok(Json(blend_darwin_eta(&app, state).await)),
        None => Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }
}

/// `DELETE /Train/{trackingId}` -- mirrors `lines.rs`'s `delete_line`
/// (same `AuthenticatedUser` + 404-for-unknown-or-not-yours shape, same
/// `204 No Content` on success), on the same path shape `GET
/// /Train/{trackingId}` (`get_by_tracking_id`, above) already uses. The
/// ownership check is folded into `train_tracking::delete_tracked_train`'s
/// own `WHERE id = $1 AND user_id = $2`, rather than a separate
/// `tracked_train_owner` lookup followed by an unscoped delete -- see that
/// function's doc comment for why nothing else needs deleting here
/// (`train_movement_events`/`train_current_state`/`tracked_train_tickets`
/// all cascade).
async fn delete_tracked_train(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = train_tracking::delete_tracked_train(&app.database, tracking_id, &user.id)
        .await
        .map_err(internal_error("delete tracked train"))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_by_uid_and_date(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_uid_and_date(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("read tracked train state"))?;
    let Some(state) = state else {
        return Err((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string()));
    };

    match train_tracking::tracked_train_owner(&app.database, state.id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string())),
    }

    Ok(Json(blend_darwin_eta(&app, state).await))
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
    multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    handle_pkpass_upload(multipart).await
}

/// The standalone counterpart of `post_pkpass_upload` above, reachable
/// before a tracked train exists at all (`POST /Train/tickets/pkpass`) --
/// same handler logic (`handle_pkpass_upload`), just with no `tracking_id`
/// path segment to ignore, since `post_pkpass_upload`'s own `_tracking_id`
/// was already unused for routing-symmetry reasons only (see that
/// function's doc comment) -- this route simply doesn't have one.
async fn post_pkpass_upload_standalone(
    _user: AuthenticatedUser,
    multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    handle_pkpass_upload(multipart).await
}

/// REVIEW-BEFORE-SAVE, structurally: this function contains no
/// `sqlx::query` call and touches no database handle -- there is nothing
/// in this file that could accidentally persist an unreviewed upload. See
/// this plan's Global Constraints.
async fn handle_pkpass_upload(mut multipart: Multipart) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
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
    multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    handle_pdf_upload(multipart).await
}

/// The standalone counterpart of `post_pdf_upload` above, reachable before
/// a tracked train exists at all (`POST /Train/tickets/pdf`) -- same
/// reasoning as `post_pkpass_upload_standalone`.
async fn post_pdf_upload_standalone(
    _user: AuthenticatedUser,
    multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    handle_pdf_upload(multipart).await
}

async fn handle_pdf_upload(mut multipart: Multipart) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
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
            tracked_train_id: Some(1),
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

    /// `router_builds_without_panicking` above only proves matchit sees no
    /// *conflict* between `/Train/mine` (literal) and `/Train/{tracking_id}`
    /// (dynamic, same segment position) at construction time -- it says
    /// nothing about which handler an actual `GET /Train/mine` request gets
    /// dispatched to. This test closes that gap: a minimal, state-free
    /// two-route reproduction of the same shape (literal vs. dynamic GET at
    /// the same position), proving matchit's real request-time precedence
    /// sends the literal route to the literal handler rather than letting
    /// `Path<i64>` on the dynamic sibling capture it. Deliberately does not
    /// build a real `AppState`/`App` or call this file's own `router()` --
    /// the crate has no existing `oneshot()`-style router-test
    /// infrastructure, and standing that up just to exercise this one
    /// mechanism would be disproportionate; a throwaway `Router::new()`
    /// with matching route shapes isolates the exact risk without it.
    #[tokio::test]
    async fn literal_route_wins_over_same_position_dynamic_route() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route("/Train/mine", axum::routing::get(|| async { "literal" }))
            .route("/Train/{id}", axum::routing::get(|| async { "dynamic" }));

        let response = app
            .oneshot(Request::builder().uri("/Train/mine").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"literal");
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

/// HTTP-layer tests for this task's two now-ownership-gated routes
/// (`get_by_tracking_id`, `get_by_uid_and_date`) -- the first HTTP-layer
/// tests for any route in this file, including the ticket routes (Task 3),
/// which already use this exact `tracked_train_owner` pattern but were
/// never exercised at this layer. Follows the `db_tests` convention Task 4
/// established in `crate::routes::lines::db_tests` and Task 6/7 repeated in
/// `crate::routes::line_status::db_tests` (`test_app`/`test_router`/
/// `seed_session` built by hand against a real `App`, exercised through the
/// real `axum::Router` via `tower::ServiceExt::oneshot`) -- kept as this
/// file's own colocated copy rather than importing another file's private
/// helpers, per those modules' own doc comments ("promote only once a
/// third file needs them" -- this is that third file; still not promoted
/// here, since that promotion is its own, separate decision for the plan's
/// controller to make, not something to do unprompted mid-task).
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
    /// this file's routes don't read `config.lines` at all, unlike
    /// `lines.rs`/`line_status.rs`'s copies of this helper, so there's no
    /// caller-supplied variance to thread through. Copied from
    /// `crate::routes::lines::db_tests::test_app` -- see this module's own
    /// doc comment for why it's not shared cross-file.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_token: "test-internal-token".to_string(),
            internal_token_poller_incidents: "test-internal-token-poller-incidents".to_string(),
            internal_token_poller_stations: "test-internal-token-poller-stations".to_string(),
            internal_token_poller_tocs: "test-internal-token-poller-tocs".to_string(),
            internal_token_poller_ldbws: "test-internal-token-poller-ldbws".to_string(),
            internal_token_poller_tfl: "test-internal-token-poller-tfl".to_string(),
            internal_token_trust_consumer: "test-internal-token-trust-consumer".to_string(),
            internal_token_schedule_ingest: "test-internal-token-schedule-ingest".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
        };

        // None of this file's routes are private_router() endpoints, so an
        // empty registry is fine -- see AppState::init for the real
        // from_tokens construction this mirrors.
        let internal_services = crate::auth::InternalServiceRegistry::default();

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. Neither of this file's
            // gated routes touch Redis at all.
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_services,
        })
    }

    /// The real `train::router()`, mounted unprefixed exactly as `main.rs`
    /// does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new().merge(super::router()).with_state(app)
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user if
    /// it doesn't already exist) and returns the *raw* token -- send it as
    /// `Cookie: distant_signal_session=<raw>`, never the hash `sessions`
    /// actually stores.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query("INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING")
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

    /// Deletes a fixture user and its fixtures. Unlike `custom_lines.user_id`
    /// (`ON DELETE CASCADE`, per `crates/api/migrations/20260828100000_add_ownership.sql`),
    /// `tracked_trains.user_id` has no `ON DELETE CASCADE` at all (see
    /// `crates/api/migrations/20260828120000_train_tracking.sql`) -- a plain
    /// `DELETE FROM users` here would fail with a foreign-key violation
    /// while any owned `tracked_trains` row still exists. So this deletes
    /// owned `tracked_trains` rows first (which *does* cascade on to
    /// `train_movement_events`/`train_current_state`/`tracked_train_tickets`,
    /// all `ON DELETE CASCADE` from `tracked_trains`), then `sessions`
    /// (`ON DELETE CASCADE` from `users`, so implicit via the final delete),
    /// then the user itself.
    /// Deletes `tracked_train_tickets` rows directly first (Part A: a
    /// STANDALONE ticket, per `20260901140000_standalone_tickets.sql`, has
    /// `tracked_train_id IS NULL` and so is NOT reachable via the
    /// `tracked_trains` cascade below at all -- `tracked_train_tickets.user_id
    /// REFERENCES users(id)` has no `ON DELETE CASCADE` of its own, so
    /// leaving an orphaned standalone-ticket fixture behind would make the
    /// final `DELETE FROM users` fail on a foreign-key violation).
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM tracked_train_tickets WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_train_tickets rows");
        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
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
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    /// Inserts one `tracked_trains` fixture row owned by `user_id`, plus a
    /// matching `train_current_state` row so a 200 response has non-null
    /// state to assert on. `train_uid: Some(..)` also marks the row
    /// `resolved` (required for `get_by_uid_and_date` to find it at all --
    /// see that route's `WHERE tt.train_uid = $1 AND tt.service_date = $2`).
    /// Returns the new row's `id`.
    async fn seed_tracked_train(pool: &PgPool, user_id: &str, train_uid: Option<&str>, service_date: chrono::NaiveDate) -> i64 {
        let resolution_status = if train_uid.is_some() { "resolved" } else { "pending" };
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, \
                 train_uid, train_id, resolution_status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("KGX")
        .bind(service_date.and_hms_opt(12, 0, 0).unwrap().and_utc())
        .bind("EDB")
        .bind(train_uid)
        .bind(train_uid.map(|_| "1A23"))
        .bind(resolution_status)
        .fetch_one(pool)
        .await
        .expect("insert fixture tracked_trains row");

        sqlx::query(
            "INSERT INTO train_current_state \
                (tracked_train_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'York', 'DEPARTURE', 12, 'Newcastle', NOW())",
        )
        .bind(id)
        .execute(pool)
        .await
        .expect("insert fixture train_current_state row");

        id
    }

    /// Seeds one `station_samples` row with a single non-cancelled
    /// departure heading to `destination_crs`, at `estimated` (a bare
    /// `"HH:MM"`, London local time -- see `eta_blend::find_darwin_eta`).
    /// Lets a 200-case test prove `blend_darwin_eta`'s overlay is still
    /// applied post-ownership-gate, not just that the route returns 200.
    async fn seed_station_sample(pool: &PgPool, crs: &str, destination_crs: &str, estimated: &str) {
        let departures = serde_json::json!([{
            "service_id": "test-service",
            "operator": "GR",
            "destination_crs": destination_crs,
            "scheduled": "11:55",
            "estimated": estimated,
            "is_cancelled": false,
            "delay_minutes": 5,
        }]);
        sqlx::query("INSERT INTO station_samples (crs, polled_at, departures) VALUES ($1, NOW(), $2::jsonb) \
                     ON CONFLICT (crs) DO UPDATE SET polled_at = EXCLUDED.polled_at, departures = EXCLUDED.departures")
            .bind(crs)
            .bind(departures)
            .execute(pool)
            .await
            .expect("seed fixture station_samples row");
    }

    async fn cleanup_station_sample(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM station_samples WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture station_samples row");
    }

    /// Issues a GET against `router`, optionally with a session cookie, and
    /// returns `(status, parsed JSON body)`. Both gated routes return either
    /// a JSON object body or a plain-text `(StatusCode, String)` error
    /// body, so wrapping the latter as a JSON string lets every case share
    /// one return shape.
    async fn request(router: axum::Router, uri: String, raw_token: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
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
    async fn post_json(router: axum::Router, uri: String, raw_token: Option<&str>, body: serde_json::Value) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri).method("POST").header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::from(serde_json::to_vec(&body).expect("serialize request body"))).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    // --- post_standalone_ticket / post_attach_ticket (Part A: upload-first tickets) ----

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                standalone_ticket -- --ignored --test-threads=1`"]
    async fn post_standalone_ticket_no_session_is_401() {
        let pool = connect().await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) =
            post_json(router, "/Train/tickets".to_string(), None, serde_json::json!({ "source": "manual" })).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                standalone_ticket -- --ignored --test-threads=1`"]
    async fn post_standalone_ticket_creates_an_unattached_ticket_visible_on_the_mine_list() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-STANDALONE-TICKET").await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "operator": "LNER", "source": "pkpass-semantics" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ticket_id = body.get("ticketId").and_then(Value::as_i64).expect("ticketId present");

        // No tracked_train_id at all yet -- must still show up on the
        // cross-train "mine" list (Decision: list_tickets_for_user's LEFT
        // JOIN), not be silently dropped.
        let (status, tickets) = request(router, "/Train/tickets/mine".to_string(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        let row = rows.iter().find(|r| r.get("id").and_then(Value::as_i64) == Some(ticket_id)).expect("ticket present in mine list");
        assert_eq!(row.get("trackedTrainId"), Some(&Value::Null));
        assert_eq!(row.get("operator").and_then(Value::as_str), Some("LNER"));
        assert!(row.get("claimUrl").and_then(Value::as_str).is_some_and(|u| !u.is_empty()), "claimUrl must still be populated: {row:?}");
        assert!(row.get("disclaimer").and_then(Value::as_str).is_some_and(|d| !d.is_empty()), "disclaimer must still be populated: {row:?}");

        cleanup_user(&pool, "TEST-STANDALONE-TICKET").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attach_ticket -- --ignored --test-threads=1`"]
    async fn post_attach_ticket_the_owner_can_attach_their_own_standalone_ticket_to_their_own_train() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ATTACH-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(&pool, "TEST-ATTACH-OWNER", None, "2026-09-01".parse().unwrap()).await;

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "operator": "LNER", "source": "manual" }),
        )
        .await;
        let ticket_id = created.get("ticketId").and_then(Value::as_i64).expect("ticketId present");

        let (status, body) = post_json(
            router.clone(),
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&token),
            serde_json::json!({ "trackingId": tracking_id }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "attach response: {body:?}");
        assert_eq!(body.get("ticketId").and_then(Value::as_i64), Some(ticket_id));
        assert_eq!(body.get("trackedTrainId").and_then(Value::as_i64), Some(tracking_id));

        // Now visible under the tracked train's own scoped ticket list too.
        let (status, tickets) = request(router, format!("/Train/{tracking_id}/tickets"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        assert!(rows.iter().any(|r| r.get("id").and_then(Value::as_i64) == Some(ticket_id)), "attached ticket should now be listed under its tracked train: {rows:?}");

        cleanup_user(&pool, "TEST-ATTACH-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attach_ticket -- --ignored --test-threads=1`"]
    async fn post_attach_ticket_a_tracked_train_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let ticket_owner_token = seed_session(&pool, "TEST-ATTACH-TICKET-OWNER").await;
        seed_session(&pool, "TEST-ATTACH-TRAIN-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let other_users_train = seed_tracked_train(&pool, "TEST-ATTACH-TRAIN-OWNER", None, "2026-09-01".parse().unwrap()).await;

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&ticket_owner_token),
            serde_json::json!({ "source": "manual" }),
        )
        .await;
        let ticket_id = created.get("ticketId").and_then(Value::as_i64).expect("ticketId present");

        let (status, body) = post_json(
            router,
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&ticket_owner_token),
            serde_json::json!({ "trackingId": other_users_train }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        cleanup_user(&pool, "TEST-ATTACH-TICKET-OWNER").await;
        cleanup_user(&pool, "TEST-ATTACH-TRAIN-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attach_ticket -- --ignored --test-threads=1`"]
    async fn post_attach_ticket_an_already_attached_ticket_is_409() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ATTACH-CONFLICT").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(&pool, "TEST-ATTACH-CONFLICT", None, "2026-09-01".parse().unwrap()).await;

        let (_, created) =
            post_json(router.clone(), "/Train/tickets".to_string(), Some(&token), serde_json::json!({ "source": "manual" })).await;
        let ticket_id = created.get("ticketId").and_then(Value::as_i64).expect("ticketId present");

        let (first_status, _) = post_json(
            router.clone(),
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&token),
            serde_json::json!({ "trackingId": tracking_id }),
        )
        .await;
        assert_eq!(first_status, StatusCode::OK);

        let (second_status, body) = post_json(
            router,
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&token),
            serde_json::json!({ "trackingId": tracking_id }),
        )
        .await;
        assert_eq!(second_status, StatusCode::CONFLICT);
        assert_eq!(body, Value::String("ticket is already attached to a tracked train".to_string()));

        cleanup_user(&pool, "TEST-ATTACH-CONFLICT").await;
    }

    // --- get_by_tracking_id -------------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TRACKID-401-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-TRACKID-401-OWNER", Some("A11111"), "2026-08-29".parse().unwrap()).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/{tracking_id}"), None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));

        cleanup_user(&pool, "TEST-TRACKID-401-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_a_non_owner_session_gets_the_same_404_as_unknown() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TRACKID-OWNER").await;
        let other_token = seed_session(&pool, "TEST-TRACKID-OTHER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-TRACKID-OWNER", Some("A22222"), "2026-08-29".parse().unwrap()).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/{tracking_id}"), Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        cleanup_user(&pool, "TEST-TRACKID-OWNER").await;
        cleanup_user(&pool, "TEST-TRACKID-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_a_nonexistent_id_is_404_with_the_unchanged_message() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TRACKID-NOTFOUND").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, "/Train/99999999".to_string(), Some(&token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        cleanup_user(&pool, "TEST-TRACKID-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_the_owner_gets_full_state_with_the_darwin_overlay_applied() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-TRACKID-REAL-OWNER").await;
        let service_date: chrono::NaiveDate = "2026-08-29".parse().unwrap();
        let tracking_id = seed_tracked_train(&pool, "TEST-TRACKID-REAL-OWNER", Some("A33333"), service_date).await;
        seed_station_sample(&pool, "KGX", "EDB", "13:45").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/{tracking_id}"), Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("id").and_then(Value::as_i64), Some(tracking_id));
        assert_eq!(body.get("trainUid").and_then(Value::as_str), Some("A33333"));
        assert_eq!(body.get("lastReportedLocation").and_then(Value::as_str), Some("York"));
        // blend_darwin_eta overlay: eta_source flips to darwin-estimated and
        // eta_next becomes a concrete timestamp derived from the seeded
        // station sample, not whatever train_current_state itself held
        // (nothing -- this fixture never seeded eta_next/eta_source there).
        assert_eq!(body.get("etaSource").and_then(Value::as_str), Some("darwin-estimated"));
        assert!(body.get("etaNext").and_then(Value::as_str).is_some(), "etaNext should be populated by the overlay: {body:?}");

        cleanup_station_sample(&pool, "KGX").await;
        cleanup_user(&pool, "TEST-TRACKID-REAL-OWNER").await;
    }

    // --- delete_tracked_train -------------------------------------------------

    /// Issues `DELETE /Train/{trackingId}`, optionally with a session
    /// cookie. Mirrors `request` above -- a success here is `204 No
    /// Content` with an empty body, so an empty body is treated as
    /// `Value::Null` rather than fed to `serde_json::from_slice` (which
    /// would otherwise fail to parse it and mask a real body on any other
    /// status).
    async fn delete_request(router: axum::Router, tracking_id: i64, raw_token: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder().method("DELETE").uri(format!("/Train/{tracking_id}"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
            })
        };
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-DELETE-401-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-DELETE-401-OWNER", Some("D11111"), "2026-08-29".parse().unwrap()).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_request(router, tracking_id, None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));

        cleanup_user(&pool, "TEST-DELETE-401-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_a_non_owner_session_gets_the_same_404_as_unknown_and_the_row_survives() {
        let pool = connect().await;
        seed_session(&pool, "TEST-DELETE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-DELETE-OTHER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-DELETE-OWNER", Some("D22222"), "2026-08-29".parse().unwrap()).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_request(router, tracking_id, Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        // Confirms the delete was a genuine no-op for a non-owner, not just
        // a 404 with the row quietly gone anyway.
        let still_there = crate::data::train_tracking::get_by_tracking_id(&pool, tracking_id).await.expect("read tracked train state");
        assert!(still_there.is_some(), "row should survive a non-owner's delete attempt");

        cleanup_user(&pool, "TEST-DELETE-OWNER").await;
        cleanup_user(&pool, "TEST-DELETE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_a_nonexistent_id_is_404_with_the_unchanged_message() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-DELETE-NOTFOUND").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_request(router, 99999999, Some(&token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        cleanup_user(&pool, "TEST-DELETE-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_the_owner_can_delete_it() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-DELETE-REAL-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-DELETE-REAL-OWNER", Some("D33333"), "2026-08-29".parse().unwrap()).await;

        let router = test_router(test_app(pool.clone()));
        let (status, _body) = delete_request(router, tracking_id, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::NO_CONTENT);

        let gone = crate::data::train_tracking::get_by_tracking_id(&pool, tracking_id).await.expect("read tracked train state");
        assert!(gone.is_none(), "row should be gone after the owner deletes it");

        cleanup_user(&pool, "TEST-DELETE-REAL-OWNER").await;
    }

    // --- get_by_uid_and_date -------------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_uid_and_date -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-UIDDATE-401-OWNER").await;
        let service_date: chrono::NaiveDate = "2026-08-29".parse().unwrap();
        seed_tracked_train(&pool, "TEST-UIDDATE-401-OWNER", Some("B11111"), service_date).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/by-uid/B11111/{service_date}"), None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));

        cleanup_user(&pool, "TEST-UIDDATE-401-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_uid_and_date -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_a_non_owner_session_gets_the_same_404_as_unresolved() {
        let pool = connect().await;
        seed_session(&pool, "TEST-UIDDATE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-UIDDATE-OTHER").await;
        let service_date: chrono::NaiveDate = "2026-08-29".parse().unwrap();
        seed_tracked_train(&pool, "TEST-UIDDATE-OWNER", Some("B22222"), service_date).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/by-uid/B22222/{service_date}"), Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no resolved tracked train for that uid/date".to_string()));

        cleanup_user(&pool, "TEST-UIDDATE-OWNER").await;
        cleanup_user(&pool, "TEST-UIDDATE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_uid_and_date -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_an_unresolved_pair_is_404_with_the_unchanged_message() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-UIDDATE-NOTFOUND").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, "/Train/by-uid/NOSUCHUID/2026-08-29".to_string(), Some(&token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no resolved tracked train for that uid/date".to_string()));

        cleanup_user(&pool, "TEST-UIDDATE-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_uid_and_date -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_the_owner_gets_full_state_with_the_darwin_overlay_applied() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-UIDDATE-REAL-OWNER").await;
        let service_date: chrono::NaiveDate = "2026-08-29".parse().unwrap();
        let tracking_id = seed_tracked_train(&pool, "TEST-UIDDATE-REAL-OWNER", Some("B33333"), service_date).await;
        seed_station_sample(&pool, "KGX", "EDB", "13:45").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/by-uid/B33333/{service_date}"), Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("id").and_then(Value::as_i64), Some(tracking_id));
        assert_eq!(body.get("trainUid").and_then(Value::as_str), Some("B33333"));
        assert_eq!(body.get("lastReportedLocation").and_then(Value::as_str), Some("York"));
        assert_eq!(body.get("etaSource").and_then(Value::as_str), Some("darwin-estimated"));
        assert!(body.get("etaNext").and_then(Value::as_str).is_some(), "etaNext should be populated by the overlay: {body:?}");

        cleanup_station_sample(&pool, "KGX").await;
        cleanup_user(&pool, "TEST-UIDDATE-REAL-OWNER").await;
    }
}
