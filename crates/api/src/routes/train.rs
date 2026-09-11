//! `/Train/...`: individual train tracking. Pin *creation* requires an
//! authenticated session (`AuthenticatedUser`, from
//! docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 6) --
//! every tracked train has a real owner from birth, per that plan's
//! coordination fix to this one. State *reads*: `get_by_tracking_id` stays
//! ownership-gated (see the 2026-08-31 private-custom-lines-and-tracked-trains
//! plan's Task 8; same 404-for-both-"missing"-and-"not-yours" convention as
//! every other ownership check in this app, never `403`).
//! `get_by_uid_and_date`, by contrast, is now PUBLIC and UNSCOPED -- a real,
//! reviewed API-contract change
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §4,
//! implemented by this repo's shared-train-identity plan's Task 19): a
//! train is a shared, real-world entity, and anyone can look up its
//! schedule/live status by `(train_uid, date)`. It reads only the shared
//! `trains`/`train_current_state` tables (`crate::data::trains::get_public_train_state`),
//! never `tracked_trains`, so no caller can ever see another user's private
//! per-subscription data (`custom_name`, tickets, notification state)
//! through it. Mounted directly (not under `/public`) to match the design
//! doc's sketched URL shape for the eventual frontend page. It also
//! read-triggers the same idempotent `find_or_create_train` upsert
//! `post_track_by_uid` uses, gated by
//! `crate::data::trains::is_known_scheduled_train` -- see its own doc
//! comment for the "View live status" 404 bug this closes.
//! `post_track_by_uid` (`POST /Train/by-uid/{train_uid}/{date}/track`,
//! design spec §4, Task 20) is the NR-primary counterpart to `post_track`:
//! back behind normal `AuthenticatedUser` session auth (this is a user
//! creating their own subscription, not a service push), but unlike
//! `post_track`'s legacy CRS+time flow it never passes through `pending`/
//! `schedule_matched` -- identity is already known upfront, so it
//! find-or-creates the shared `trains` row and links a subscription to it
//! in the same request.

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use common::{TicketEntryRequest, TrackPinRequest};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{
    delay_repay_rules, eta_blend, schedule_matching, ticket_extraction, train_tracking,
};

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
        // later gets a `tracked_train_id`. `/Train/tickets/{ticket_id}`
        // (`DELETE` only) removes a ticket outright, regardless of
        // attachment status -- see `delete_ticket`'s own doc comment.
        .route(
            "/Train/tickets",
            axum::routing::post(post_standalone_ticket),
        )
        .route("/Train/tickets/mine", axum::routing::get(get_my_tickets))
        .route(
            "/Train/tickets/pkpass",
            axum::routing::post(post_pkpass_upload_standalone),
        )
        .route(
            "/Train/tickets/pdf",
            axum::routing::post(post_pdf_upload_standalone),
        )
        .route(
            "/Train/tickets/{ticket_id}/attach",
            axum::routing::post(post_attach_ticket),
        )
        .route(
            "/Train/tickets/{ticket_id}/name",
            axum::routing::post(post_ticket_name),
        )
        .route(
            "/Train/tickets/{ticket_id}",
            axum::routing::delete(delete_ticket),
        )
        .route(
            "/Train/{tracking_id}",
            axum::routing::get(get_by_tracking_id).delete(delete_tracked_train),
        )
        .route(
            "/Train/{tracking_id}/name",
            axum::routing::post(post_tracked_train_name),
        )
        .route(
            "/Train/by-uid/{train_uid}/{date}",
            axum::routing::get(get_by_uid_and_date),
        )
        .route(
            "/Train/by-uid/{train_uid}/{date}/track",
            axum::routing::post(post_track_by_uid),
        )
        .route(
            "/Train/{tracking_id}/tickets",
            axum::routing::post(post_ticket).get(get_tickets),
        )
        .route(
            "/Train/{tracking_id}/tickets/{ticket_id}/delay-repay",
            axum::routing::get(get_delay_repay_estimate),
        )
        .route(
            "/Train/{tracking_id}/tickets/pkpass",
            axum::routing::post(post_pkpass_upload),
        )
        .route(
            "/Train/{tracking_id}/tickets/pdf",
            axum::routing::post(post_pdf_upload),
        )
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameRequest {
    /// Absent, JSON `null`, or an empty/whitespace-only string all mean
    /// "clear the custom name" -- see `train_tracking::validate_custom_name`.
    #[serde(default)]
    custom_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameResponse {
    /// The normalized value actually stored -- `None` if the name was
    /// cleared, `Some(trimmed)` otherwise. Never echoes back an
    /// un-normalized value the caller sent.
    custom_name: Option<String>,
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

    match train_tracking::tracked_train_owner(&app.database, tracking_id)
        .await
        .map_err(internal_error("check tracked train ownership"))?
    {
        Some(owner) if owner == user.id => {}
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                "no tracked train with that id".to_string(),
            ));
        }
    }

    let ticket_id =
        train_tracking::create_ticket(&app.database, Some(tracking_id), &entry, &user.id)
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
    match train_tracking::tracked_train_owner(&app.database, body.tracking_id)
        .await
        .map_err(internal_error("check tracked train ownership"))?
    {
        Some(owner) if owner == user.id => {}
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                "no tracked train with that id".to_string(),
            ));
        }
    }

    let ticket = train_tracking::get_ticket_owned(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("read ticket"))?
        .ok_or((StatusCode::NOT_FOUND, "no ticket with that id".to_string()))?;

    if ticket.tracked_train_id.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "ticket is already attached to a tracked train".to_string(),
        ));
    }

    let attached = train_tracking::attach_ticket_to_tracked_train(
        &app.database,
        ticket_id,
        body.tracking_id,
        &user.id,
    )
    .await
    .map_err(internal_error("attach ticket"))?;
    if !attached {
        // Lost a race against a concurrent attach/re-check between the
        // reads above and this write -- treat it the same as the
        // already-attached case above rather than a 500, since that's
        // exactly what it now is.
        return Err((
            StatusCode::CONFLICT,
            "ticket is already attached to a tracked train".to_string(),
        ));
    }

    Ok(Json(AttachTicketResponse {
        ticket_id,
        tracked_train_id: body.tracking_id,
    }))
}

/// `DELETE /Train/tickets/{ticketId}` -- mirrors `delete_tracked_train`
/// (below) exactly: same `AuthenticatedUser` + 404-for-unknown-or-not-yours
/// shape, same `204 No Content` on success. Ownership is folded directly
/// into `train_tracking::delete_ticket`'s own `WHERE id = $1 AND user_id =
/// $2` (no join, no separate ownership lookup first -- see that function's
/// doc comment). Deliberately flat (`/Train/tickets/{ticket_id}`, not
/// nested under a `{tracking_id}`), matching `post_attach_ticket`'s own
/// reasoning just above: a ticket may have no owning tracked train at all
/// (a STANDALONE ticket), so a route shape that requires a `tracking_id`
/// in its path cannot express deleting one. Applies uniformly regardless
/// of attachment status -- `tracked_train_tickets` has no child rows to
/// clean up either way (unlike `delete_tracked_train`, this is a leaf in
/// the FK graph).
async fn delete_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(ticket_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = train_tracking::delete_ticket(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("delete ticket"))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "no ticket with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /Train/tickets/{ticketId}/name` -- sets or clears a ticket's
/// display name. Same shape as `post_tracked_train_name` below, against
/// `train_tracking::rename_ticket` instead. Deliberately flat
/// (`/Train/tickets/{ticket_id}/name`, not nested under a
/// `{tracking_id}`), matching `delete_ticket`'s own reasoning immediately
/// above it: a ticket may have no owning tracked train at all (a
/// STANDALONE ticket), so a route shape requiring a `tracking_id` in its
/// path cannot express renaming one.
async fn post_ticket_name(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(ticket_id): Path<i64>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, (StatusCode, String)> {
    let normalized = train_tracking::validate_custom_name(body.custom_name.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let renamed =
        train_tracking::rename_ticket(&app.database, ticket_id, &user.id, normalized.as_deref())
            .await
            .map_err(internal_error("rename ticket"))?;
    if !renamed {
        return Err((StatusCode::NOT_FOUND, "no ticket with that id".to_string()));
    }

    Ok(Json(RenameResponse {
        custom_name: normalized,
    }))
}

async fn get_tickets(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<Json<Vec<train_tracking::TrackedTrainTicket>>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, tracking_id)
        .await
        .map_err(internal_error("check tracked train ownership"))?
    {
        Some(owner) if owner == user.id => {}
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                "no tracked train with that id".to_string(),
            ));
        }
    }

    let tickets =
        train_tracking::list_tickets_for_tracked_train(&app.database, tracking_id, &user.id)
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
        .ok_or((
            StatusCode::NOT_FOUND,
            "no ticket with that id for that tracked train".to_string(),
        ))?;

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?
        .ok_or((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        ))?;

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
        (Some(operator), Some(delay_minutes)) => {
            delay_repay_rules::estimate_delay_repay(operator, delay_minutes)
        }
        _ => None,
    };
    let claim_url = ticket
        .operator
        .as_deref()
        .map(delay_repay_rules::claim_url_for)
        .unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

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

    // Best-effort schedule-first match, attempted synchronously in the
    // same request (Decision 3 of
    // docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
    // A failure here must never fail pin creation itself -- the periodic
    // sweep (Task 8) retries any pin this call didn't resolve, including
    // one that failed with a real error.
    let resolution_status = match schedule_matching::attempt_schedule_match(
        &app.database,
        tracking_id,
        &pin.origin_crs,
        pin.scheduled_departure,
        pin.service_date,
        &app.schedule_crs_line_index,
    )
    .await
    {
        Ok(true) => "schedule_matched",
        Ok(false) => "pending",
        Err(err) => {
            tracing::warn!(
                error = ?err,
                tracking_id,
                "schedule match attempt failed at pin creation; pin stays pending"
            );
            "pending"
        }
    };

    // Best-effort backlog match for a pin whose train's live TRUST window
    // has already closed (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md
    // Task 5). Independent of, and safe to run in any order relative to,
    // the schedule-first match above -- see that plan's own "Dependency
    // on the schedule-first plan" section: attempt_schedule_match's own
    // UPDATE is guarded by `WHERE train_uid IS NULL AND resolution_status
    // = 'pending'`, so it no-ops against a row this call already
    // resolved, and this call's own upsert_train_event write has no
    // dependency on resolution_status's prior value at all. A failure
    // here must never fail pin creation itself, same posture as the
    // schedule match above; unlike that one, there is no periodic sweep
    // to retry a backlog match later -- see attempt_backlog_match's own
    // module doc comment for why that's a deliberate simplification, not
    // an oversight. Note this doesn't update `resolution_status` above:
    // the response returned to the caller reflects the schedule-match
    // outcome only, same as the plan's own Task 5 Step 3 sketch -- a
    // successful backlog match still lands correctly in the database, a
    // subsequent GET just sees it a moment sooner than this response does.
    if let Err(err) = crate::data::trust_event_backlog_match::attempt_backlog_match(
        &app.database,
        tracking_id,
        &pin.origin_crs,
        pin.scheduled_departure,
        pin.service_date,
    )
    .await
    {
        tracing::warn!(error = ?err, tracking_id, "backlog match attempt failed; pin remains pending");
    }

    Ok(Json(TrackPinResponse {
        tracking_id,
        resolution_status,
    }))
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
    match train_tracking::tracked_train_owner(&app.database, tracking_id)
        .await
        .map_err(internal_error("check tracked train ownership"))?
    {
        Some(owner) if owner == user.id => {}
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                "no tracked train with that id".to_string(),
            ));
        }
    }

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?;
    match state {
        Some(state) => Ok(Json(
            attach_journey_stops(&app, blend_darwin_eta(&app, state).await).await,
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        )),
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
        return Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /Train/{trackingId}/name` -- sets or clears a tracked train's
/// display name. `POST` to a narrow `/name` sub-path, not `PATCH` or a
/// bare `PUT /Train/{trackingId}`: this router has zero existing `PATCH`
/// routes, and every other narrow single-field mutation here (e.g.
/// `POST /Train/tickets/{ticket_id}/attach`) already follows this exact
/// shape -- see this plan's Judgment Call 2 for the full reasoning.
/// Ownership is folded directly into `train_tracking::rename_tracked_train`'s
/// own `WHERE id = $1 AND user_id = $2` -- same 404-never-403 convention as
/// `delete_tracked_train` immediately above.
async fn post_tracked_train_name(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, (StatusCode, String)> {
    let normalized = train_tracking::validate_custom_name(body.custom_name.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let renamed = train_tracking::rename_tracked_train(
        &app.database,
        tracking_id,
        &user.id,
        normalized.as_deref(),
    )
    .await
    .map_err(internal_error("rename tracked train"))?;
    if !renamed {
        return Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        ));
    }

    Ok(Json(RenameResponse {
        custom_name: normalized,
    }))
}

/// `GET /Train/by-uid/{uid}/{date}` -- PUBLIC and UNSCOPED. This is a real,
/// reviewed API-contract change
/// (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §4):
/// this route used to require `AuthenticatedUser` and 404 unless the
/// caller's own `tracked_trains` row matched: "your own tracked trains
/// only, 404 for anyone else's." As of this task it's "anyone can look up
/// any known train" -- no `AuthenticatedUser` extractor, no ownership
/// check, no `tracked_trains` table anywhere in this call path at all. It
/// reads only the shared `trains`/`train_current_state` rows via
/// `crate::data::trains::get_public_train_state`, whose own `PublicTrainState`
/// return type structurally carries no `custom_name`/ticket/notification
/// field -- there is nothing in this response shape that COULD leak another
/// user's private per-subscription data, regardless of caller.
///
/// READ-TRIGGERED UPSERT (bug fix, post-Task-19): the `/trains` search page
/// links every result straight here using a `(train_uid, service_date)` it
/// read off `schedule_destination_departures` -- a table populated for
/// every scheduled train regardless of tracking status. A `trains` row,
/// however, has only ever been created by `find_or_create_train`, reachable
/// (before this fix) solely from `post_track_by_uid` or live TRUST/backlog
/// resolution -- so a real, just-searched train that nobody had tracked yet
/// and that hadn't activated in TRUST yet had NO `trains` row, and this
/// route 404'd on a completely valid search result. Fixed the same way
/// `post_track_by_uid` (below) already creates its own `trains` row: on a
/// miss, call the exact same idempotent `find_or_create_train` upsert --
/// just without `post_track_by_uid`'s subsequent
/// `create_subscription_for_train`, since this is a read, not a track
/// request, and needs no `AuthenticatedUser` at all.
///
/// That upsert is gated by `crate::data::trains::is_known_scheduled_train`
/// first, so this can only conjure a `trains` row into existence for an
/// identity CIF actually published -- never for an arbitrary string in the
/// URL. This deliberately differs from `post_track_by_uid`, which calls
/// `find_or_create_train` unconditionally with no such gate: that route
/// requires a real session (a garbage uid there is, at worst, one
/// authenticated user creating one throwaway row they can already see and
/// delete), where this one is reachable by anyone, unauthenticated, so an
/// ungated version would let any caller mint arbitrary `trains` rows for
/// made-up identities by hitting this URL in a loop.
async fn get_by_uid_and_date(
    State(app): State<App>,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<crate::data::trains::PublicTrainState>, (StatusCode, String)> {
    let mut state = crate::data::trains::get_public_train_state(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("read public train state"))?;

    if state.is_none() {
        let known =
            crate::data::trains::is_known_scheduled_train(&app.database, &train_uid, date)
                .await
                .map_err(internal_error("check schedule for train"))?;
        if known {
            crate::data::trains::find_or_create_train(&app.database, &train_uid, date)
                .await
                .map_err(internal_error("find or create train"))?;
            state = crate::data::trains::get_public_train_state(&app.database, &train_uid, date)
                .await
                .map_err(internal_error("read public train state"))?;
        }
    }

    match state {
        Some(state) => Ok(Json(attach_journey_stops_public(&app, state).await)),
        None => Err((
            StatusCode::NOT_FOUND,
            "no known train for that uid/date".to_string(),
        )),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackByUidResponse {
    tracking_id: i64,
}

/// `POST /Train/by-uid/{train_uid}/{date}/track` -- the NR-primary tracking
/// entry point (Task 20,
/// docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §4).
/// AUTHENTICATED via the normal user-session `AuthenticatedUser` extractor
/// -- same auth model as `post_track` below, NOT the internal-OAuth
/// service-token pattern the poller/consumer routes use (this is a user
/// creating their own subscription, not a backend service pushing data).
/// Unlike `post_track`'s legacy CRS+time flow, this never goes through
/// `pending`/`schedule_matched`: identity is already known upfront (a real
/// `train_uid`, not a departure-board guess), so `find_or_create_train`
/// resolves the shared row immediately and
/// `train_tracking::create_subscription_for_train` links the new
/// subscription to it in the same request, no waypoint in between.
async fn post_track_by_uid(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<TrackByUidResponse>, (StatusCode, String)> {
    let trains_id = crate::data::trains::find_or_create_train(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("find or create train"))?;
    let tracking_id =
        train_tracking::create_subscription_for_train(&app.database, trains_id, &user.id)
            .await
            .map_err(internal_error("create subscription"))?;

    enrich_shared_train(&app, tracking_id, trains_id, &train_uid, date).await;

    Ok(Json(TrackByUidResponse { tracking_id }))
}

/// Best-effort enrichment of the shared `trains` row a brand-new NR-primary
/// subscription just linked to. Review finding I1: without this, that
/// subscription NEVER acquired schedule data (origin, destination, calling
/// points) or a `train_id` from any path at all.
///
/// Why nothing else covered it:
/// * `attempt_schedule_match` (the legacy pin's path) is gated on
///   `apply_schedule_match`'s `WHERE trains_id IS NULL AND
///   resolution_status = 'pending'`, which is false the instant
///   `create_subscription_for_train` returns.
/// * `list_pending_pins_for_schedule_match` (the periodic sweep)
///   deliberately excludes `trains_id`-bearing rows for the same reason,
///   and could not use them anyway -- it selects `pin_origin_crs` as a
///   non-`Option`, which is exactly `NULL` for this shape.
/// * `attempt_backlog_match` needs a CRS+time pin to discover an identity
///   from; this path already HAS the identity and has no pin.
///
/// So it goes the other way round, via `find_train_id_by_uid` (Task 15,
/// which had no production caller until now): identity -> TRUST `train_id`
/// -> replayed history -> origin departure -> schedule match. Both steps
/// are best-effort and logged-not-propagated: a train nobody has any
/// retained history for is a completely normal outcome (it may not have
/// run yet -- see the live-data path below), and none of it may fail the
/// subscription the caller actually asked for.
///
/// LIVE data does NOT come through here and never did: a train that has
/// not yet run is resolved by `trust-consumer`, which sees this
/// subscription through `list_active_tracked_trains`' `LEFT JOIN trains`
/// (so its `train_uid` reaches `Reference::by_train_uid`) and matches it
/// directly off the Activation. That path is verified end to end by
/// `nr_primary_subscriptions_resolve_from_a_live_activation_and_movement`
/// in `crates/trust-consumer/src/process.rs`, and by
/// `an_nr_primary_subscription_receives_live_movement_events` in
/// `train_tracking`'s own `db_tests`.
async fn enrich_shared_train(
    app: &App,
    tracking_id: i64,
    trains_id: i64,
    train_uid: &str,
    date: NaiveDate,
) {
    // Cheap precheck: a shared row that already has both halves has nothing
    // left for this to add, and a second subscriber to a popular train is
    // the common case this endpoint exists for.
    match crate::data::trains::shared_train_enrichment_state(&app.database, trains_id).await {
        Ok(Some((true, true))) => return,
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(error = ?err, trains_id, "could not read shared train enrichment state");
            return;
        }
    }

    let outcome = match crate::data::trust_event_backlog_match::attempt_backlog_match_by_uid(
        &app.database,
        tracking_id,
        train_uid,
        date,
    )
    .await
    {
        Ok(Some(outcome)) => outcome,
        Ok(None) => {
            tracing::debug!(
                train_uid,
                "no retained TRUST history for this train; leaving it to live trust-consumer \
                 resolution"
            );
            return;
        }
        Err(err) => {
            tracing::warn!(error = ?err, train_uid, "backlog replay failed for a new NR-primary subscription");
            return;
        }
    };

    tracing::info!(
        train_uid,
        train_id = outcome.train_id,
        replayed_rows = outcome.replayed_rows,
        "replayed retained TRUST history onto a new NR-primary subscription"
    );

    let Some((origin_crs, scheduled_departure)) = outcome.origin_departure else {
        tracing::debug!(
            train_uid,
            "replayed history carries no located origin departure, so there is no (CRS, time) \
             key to look a schedule up by"
        );
        return;
    };

    match schedule_matching::attempt_schedule_match_for_shared_train(
        &app.database,
        train_uid,
        &origin_crs,
        scheduled_departure,
        date,
        &app.schedule_crs_line_index,
    )
    .await
    {
        Ok(true) => tracing::info!(train_uid, origin_crs, "schedule-matched a shared train row"),
        Ok(false) => tracing::debug!(
            train_uid,
            origin_crs,
            "no schedule match for this train's replayed origin departure"
        ),
        Err(err) => {
            tracing::warn!(error = ?err, train_uid, "schedule match failed for a shared train row")
        }
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
async fn blend_darwin_eta(
    app: &App,
    mut state: train_tracking::TrackedTrainState,
) -> train_tracking::TrackedTrainState {
    let Some(destination) = state
        .pin_destination_crs
        .as_deref()
        .or(state.next_calling_point.as_deref())
    else {
        return state;
    };
    // `pin_origin_crs` is `Option` as of Fix 2 (it is `NULL` for an
    // NR-primary subscription whose `trains` row has no schedule data yet).
    // There is no origin station to fetch a Darwin departure board for in
    // that case, so this overlay simply doesn't apply -- the same
    // return-`state`-unchanged posture as the two failure branches around
    // it, not an error.
    let Some(pin_origin_crs) = state.pin_origin_crs.as_deref() else {
        return state;
    };
    let Ok(samples) =
        crate::data::queries::latest_station_sample(&app.database, pin_origin_crs).await
    else {
        return state;
    };
    if let Some(sample) = samples
        && let Some(eta) = eta_blend::find_darwin_eta(
            &sample.departures,
            Some(destination),
            None,
            state.service_date,
        )
    {
        state.eta_next = Some(eta);
        state.eta_source = Some("darwin-estimated".to_string());
    }
    state
}

/// Attaches `journey_stops` to an already-fetched `TrackedTrainState`,
/// mirroring `blend_darwin_eta`'s own "read row, then overlay a computed
/// field" shape on the same struct. Only attempted once `train_uid`/
/// `trains_id` are both known (§1 of the design doc) -- a `pending`/
/// `unresolved` state has neither, and this returns `state` unchanged for
/// it, same as `blend_darwin_eta`'s own early-return branches. A DB error
/// building the overlay degrades to `journey_stops: None` rather than
/// failing the whole request -- the same best-effort posture
/// `blend_darwin_eta` already has for its own overlay.
async fn attach_journey_stops(
    app: &App,
    mut state: train_tracking::TrackedTrainState,
) -> train_tracking::TrackedTrainState {
    let (Some(trains_id), Some(train_uid)) = (state.trains_id, state.train_uid.clone()) else {
        return state;
    };
    match crate::data::journey::build_journey_stops(
        &app.database,
        trains_id,
        &train_uid,
        state.service_date,
        state.schedule_calling_points.as_ref(),
        state.delay_minutes,
    )
    .await
    {
        Ok(stops) => {
            // `may_have_arrived` is computed from the SAME `stops` this
            // just built (it reads the final stop's `estimated_arrival`,
            // which `build_journey_stops` only just populated), not
            // recomputed later against whatever `state.journey_stops` ends
            // up holding -- so it's derived before the move below.
            state.may_have_arrived = stops
                .as_deref()
                .is_some_and(|stops| crate::data::journey::may_have_arrived(stops, Utc::now()));
            state.journey_stops = stops;
        }
        Err(err) => {
            tracing::warn!(error = ?err, trains_id, "could not build journey stops");
        }
    }
    state
}

/// Public-route sibling of `attach_journey_stops`, for `PublicTrainState`.
/// `PublicTrainState.train_uid` is a bare `String` (always present once any
/// `trains` row exists at all, per `get_public_train_state`'s `SELECT
/// tr.train_uid`), so it cannot itself signal "train_uid unknown" the way
/// `TrackedTrainState.train_uid: Option<String>` can.
///
/// Unlike `attach_journey_stops` above, this has NO early-return gate on
/// `train_id`/`origin_crs`. A gate mirroring the frontend's
/// `toJourneyState` "pending" check (`train.trainId ? 'resolved' :
/// train.originCrs ? 'schedule_matched' : 'pending'`) was tried here and
/// removed (final whole-branch review, Finding 3): `GET
/// /public/trains/search` results link straight to `/train/{uid}/{date}`,
/// which `get_by_uid_and_date` above serves for a not-yet-seen train by
/// calling the BARE `find_or_create_train` (not the schedule-match
/// version) after `is_known_scheduled_train` has already confirmed the
/// train really is a CIF-published schedule for that day -- so the
/// resulting row has `origin_crs: None`/`train_id: None` even though the
/// identity is provably real. Excluding a genuinely-unknown/`pending`
/// train (design doc §1's stated reasoning: no reliable way to know which
/// CIF schedule a real-world service corresponds to without a known
/// identity) doesn't apply on this route -- the identity here is the
/// URL's own `(train_uid, date)`, already validated by
/// `is_known_scheduled_train`/`get_public_train_state` finding a row at
/// all. `build_journey_stops` already returns `Ok(None)` safely when
/// neither source has anything, so no replacement gate is needed.
async fn attach_journey_stops_public(
    app: &App,
    mut state: crate::data::trains::PublicTrainState,
) -> crate::data::trains::PublicTrainState {
    match crate::data::journey::build_journey_stops(
        &app.database,
        state.trains_id,
        &state.train_uid,
        state.service_date,
        state.calling_points.as_ref(),
        state.delay_minutes,
    )
    .await
    {
        Ok(stops) => {
            // See `attach_journey_stops`'s own comment just above -- same
            // "derive from the freshly-built list before it moves" reasoning.
            state.may_have_arrived = stops
                .as_deref()
                .is_some_and(|stops| crate::data::journey::may_have_arrived(stops, Utc::now()));
            state.journey_stops = stops;
        }
        Err(err) => {
            tracing::warn!(error = ?err, trains_id = state.trains_id, "could not build journey stops");
        }
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
async fn handle_pkpass_upload(
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;
    ticket_extraction::parse_pkpass(&bytes)
        .map(Json)
        .map_err(|err| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("could not read this as a train .pkpass: {err}"),
            )
        })
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

async fn handle_pdf_upload(
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;

    let parsed = tokio::time::timeout(
        PDF_PARSE_TIMEOUT,
        tokio::task::spawn_blocking(move || ticket_extraction::parse_pdf(&bytes)),
    )
    .await;

    match parsed {
        Ok(Ok(Ok(ticket))) => Ok(Json(ticket)),
        Ok(Ok(Err(err))) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("could not read this as a PDF e-ticket: {err}"),
        )),
        Ok(Err(join_err)) => {
            tracing::error!(error = ?join_err, "PDF parse task panicked or was cancelled");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to parse PDF e-ticket".to_string(),
            ))
        }
        Err(_elapsed) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            "PDF e-ticket parsing took too long; try a smaller or simpler file".to_string(),
        )),
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
async fn read_single_file_field(
    multipart: &mut Multipart,
    field_name: &str,
) -> Result<Vec<u8>, (StatusCode, String)> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("malformed upload: {err}")))?
    {
        if field.name() == Some(field_name) {
            let bytes = field.bytes().await.map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("failed to read upload: {err}"),
                )
            })?;
            return Ok(bytes.to_vec());
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        format!("no '{field_name}' field in upload"),
    ))
}

/// Shared 500 mapper for every route in this file. Takes the operation that
/// failed rather than hardcoding one: this helper serves the write route and
/// both read routes, and it previously logged and answered "failed to create
/// train tracking pin" for all three -- so a database error on a GET pointed
/// whoever read the log at a pin-creation bug that hadn't happened.
fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "train tracking request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {operation}"),
        )
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
            origin_name: Some("London Kings Cross".to_string()),
            destination_name: Some("Edinburgh Waverley".to_string()),
            source: "manual".to_string(),
            created_at: fixed_instant(),
            custom_name: None,
        }
    }

    fn state(delay_minutes: Option<i32>) -> train_tracking::TrackedTrainState {
        train_tracking::TrackedTrainState {
            id: 1,
            service_date: "2026-08-29".parse().unwrap(),
            pin_origin_crs: Some("KGX".to_string()),
            pin_destination_crs: Some("EDB".to_string()),
            pin_origin_name: Some("London Kings Cross".to_string()),
            pin_destination_name: Some("Edinburgh Waverley".to_string()),
            resolution_status: "resolved".to_string(),
            train_uid: Some("A12345".to_string()),
            train_id: Some("1A23".to_string()),
            schedule_destination_crs: None,
            schedule_destination_name: None,
            schedule_calling_points: None,
            status: Some("late".to_string()),
            last_reported_location: Some("York".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes,
            next_calling_point: Some("Newcastle".to_string()),
            eta_next: Some(fixed_instant()),
            eta_source: Some("darwin-estimated".to_string()),
            custom_name: None,
            shared_group_count: 0,
            trains_id: Some(1),
            journey_stops: None,
            may_have_arrived: false,
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
            .oneshot(
                Request::builder()
                    .uri("/Train/mine")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"literal");
    }

    #[test]
    fn dr30_operator_with_a_qualifying_delay_gets_a_specific_estimate_and_claim_url() {
        let response = build_delay_repay_response(&ticket(Some("LNER")), &state(Some(45)));

        let estimate = response
            .estimate
            .expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, 50);
        assert_eq!(
            response.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
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
        assert_eq!(
            response.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
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
        test_app_with_schedule_index(pool, std::collections::HashMap::new())
    }

    /// Same fixture as `test_app`, but with a caller-supplied
    /// `schedule_crs_line_index` -- used by the one route test
    /// (`post_track_schedule_matches_a_pin_whose_train_a_live_movement_would_have_missed`)
    /// that needs a real candidate line for its origin CRS; every other
    /// test in this module gets the empty-index default via `test_app`.
    fn test_app_with_schedule_index(
        pool: PgPool,
        schedule_crs_line_index: std::collections::HashMap<String, Vec<String>>,
    ) -> App {
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
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index,
        })
    }

    /// The real `train::router()`, mounted unprefixed exactly as `main.rs`
    /// does, turned into a `tower::Service` a test can drive with
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
        // `seed_tracked_train` (Step C: `find_or_create_train`) can leave a
        // shared `trains` row behind that nothing else references once the
        // owning `tracked_trains` row below is deleted -- clean those up
        // first, while the FK linking them is still readable, so repeat
        // runs of the SAME fixture identity (e.g. "A33333") don't collide
        // with a leftover row's `ON CONFLICT (train_uid, service_date)`
        // ...though that upsert is idempotent anyway, leaving it behind
        // would just be silent, unbounded fixture-data accumulation.
        sqlx::query(
            "DELETE FROM trains WHERE id IN \
                (SELECT trains_id FROM train_subscriptions WHERE user_id = $1 AND trains_id IS NOT NULL)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture trains rows");
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

    /// Inserts one `tracked_trains` fixture row owned by `user_id`, plus a
    /// matching `train_current_state` row so a 200 response has non-null
    /// state to assert on. `train_uid: Some(..)` also marks the row
    /// `resolved` (required for `get_by_uid_and_date` to find it at all --
    /// see that route's `WHERE tt.train_uid = $1 AND tt.service_date = $2`,
    /// now `tr.train_uid = $1` post-Step-C) AND links `trains_id` via
    /// `find_or_create_train` -- Step C's read cutover
    /// (`TRACKED_TRAIN_STATE_SELECT`'s `LEFT JOIN trains tr`) reads
    /// `train_uid`/`train_id`/schedule fields through that join, so a
    /// fixture that set `tracked_trains.train_uid` directly without also
    /// linking `trains_id` (as every real dual-write path always does)
    /// would read back `None` -- a fixture gap Task 8's own end-to-end
    /// verification caught, not a production behavior change.
    ///
    /// The `train_current_state` row itself is ALSO keyed on `trains_id`
    /// (in addition to `tracked_train_id`) whenever one is available -- Step
    /// D's re-point (Task 11) moved `TRACKED_TRAIN_STATE_SELECT`'s `cs` join
    /// from `cs.tracked_train_id = tt.id` to `cs.trains_id = tt.trains_id`,
    /// matching real `upsert_train_movement` writes, which never populate
    /// `tracked_train_id` at all. A fixture that only set `tracked_train_id`
    /// here (as every real write path used to, pre-Task-11) would silently
    /// stop being visible through that join -- caught by this task's own
    /// regression pass, same class of fixture gap as the `trains_id`
    /// linking above. For a `train_uid: None` (still-`pending`) fixture,
    /// `trains_id` stays `NULL` here too -- correctly unreachable via the
    /// join, since a real pending pin can never have a `trains_id`-keyed
    /// current-state row either (nothing has resolved its identity yet).
    /// Returns the new row's `id`.
    async fn seed_tracked_train(
        pool: &PgPool,
        user_id: &str,
        train_uid: Option<&str>,
        service_date: chrono::NaiveDate,
    ) -> i64 {
        let resolution_status = if train_uid.is_some() {
            "resolved"
        } else {
            "pending"
        };
        let trains_id = match train_uid {
            Some(uid) => Some(
                crate::data::trains::find_or_create_train(pool, uid, service_date)
                    .await
                    .expect("find_or_create_train for fixture"),
            ),
            None => None,
        };
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, \
                 resolution_status, trains_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("KGX")
        .bind(service_date.and_hms_opt(12, 0, 0).unwrap().and_utc())
        .bind("EDB")
        .bind(resolution_status)
        .bind(trains_id)
        .fetch_one(pool)
        .await
        .expect("insert fixture tracked_trains row");
        if let Some(trains_id) = trains_id {
            crate::data::trains::mark_train_resolved(pool, trains_id, "1A23")
                .await
                .expect("mark_train_resolved for fixture");
        }

        // ONLY when there is a `trains_id` to key it on. This used to bind
        // `trains_id` unconditionally, which for a `train_uid: None`
        // (still-`pending`) fixture inserted a `train_current_state` row
        // with `trains_id IS NULL`. Since Task 22 dropped this table's
        // `tracked_train_id` column, such a row is joined to nothing, read
        // by nothing (every read path joins `cs ON cs.trains_id =
        // tt.trains_id`), and cascaded by nothing -- so `cleanup_user`
        // could never delete it and every run of this suite leaked one per
        // pending fixture. Skipping the insert is not a coverage loss: the
        // row it produced was already invisible to every assertion.
        if let Some(trains_id) = trains_id {
            sqlx::query(
                "INSERT INTO train_current_state \
                    (trains_id, status, last_reported_location, last_event_type, \
                     delay_minutes, next_calling_point, updated_at) \
                 VALUES ($1, 'en_route', 'York', 'DEPARTURE', 12, 'Newcastle', NOW())",
            )
            .bind(trains_id)
            .execute(pool)
            .await
            .expect("insert fixture train_current_state row");
        }

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

    // --- post_standalone_ticket / post_attach_ticket (Part A: upload-first tickets) ----

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                standalone_ticket -- --ignored --test-threads=1`"]
    async fn post_standalone_ticket_no_session_is_401() {
        let pool = connect().await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) = post_json(
            router,
            "/Train/tickets".to_string(),
            None,
            serde_json::json!({ "source": "manual" }),
        )
        .await;

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
        let ticket_id = body
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

        // No tracked_train_id at all yet -- must still show up on the
        // cross-train "mine" list (Decision: list_tickets_for_user's LEFT
        // JOIN), not be silently dropped.
        let (status, tickets) =
            request(router, "/Train/tickets/mine".to_string(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        let row = rows
            .iter()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(ticket_id))
            .expect("ticket present in mine list");
        assert_eq!(row.get("trackedTrainId"), Some(&Value::Null));
        assert_eq!(row.get("operator").and_then(Value::as_str), Some("LNER"));
        assert!(
            row.get("claimUrl")
                .and_then(Value::as_str)
                .is_some_and(|u| !u.is_empty()),
            "claimUrl must still be populated: {row:?}"
        );
        assert!(
            row.get("disclaimer")
                .and_then(Value::as_str)
                .is_some_and(|d| !d.is_empty()),
            "disclaimer must still be populated: {row:?}"
        );

        cleanup_user(&pool, "TEST-STANDALONE-TICKET").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attach_ticket -- --ignored --test-threads=1`"]
    async fn post_attach_ticket_the_owner_can_attach_their_own_standalone_ticket_to_their_own_train()
     {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ATTACH-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ATTACH-OWNER",
            None,
            "2026-09-01".parse().unwrap(),
        )
        .await;

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "operator": "LNER", "source": "manual" }),
        )
        .await;
        let ticket_id = created
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

        let (status, body) = post_json(
            router.clone(),
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&token),
            serde_json::json!({ "trackingId": tracking_id }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "attach response: {body:?}");
        assert_eq!(
            body.get("ticketId").and_then(Value::as_i64),
            Some(ticket_id)
        );
        assert_eq!(
            body.get("trackedTrainId").and_then(Value::as_i64),
            Some(tracking_id)
        );

        // Now visible under the tracked train's own scoped ticket list too.
        let (status, tickets) = request(
            router,
            format!("/Train/{tracking_id}/tickets"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        assert!(
            rows.iter()
                .any(|r| r.get("id").and_then(Value::as_i64) == Some(ticket_id)),
            "attached ticket should now be listed under its tracked train: {rows:?}"
        );

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
        let other_users_train = seed_tracked_train(
            &pool,
            "TEST-ATTACH-TRAIN-OWNER",
            None,
            "2026-09-01".parse().unwrap(),
        )
        .await;

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&ticket_owner_token),
            serde_json::json!({ "source": "manual" }),
        )
        .await;
        let ticket_id = created
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

        let (status, body) = post_json(
            router,
            format!("/Train/tickets/{ticket_id}/attach"),
            Some(&ticket_owner_token),
            serde_json::json!({ "trackingId": other_users_train }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

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
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ATTACH-CONFLICT",
            None,
            "2026-09-01".parse().unwrap(),
        )
        .await;

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "source": "manual" }),
        )
        .await;
        let ticket_id = created
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

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
        assert_eq!(
            body,
            Value::String("ticket is already attached to a tracked train".to_string())
        );

        cleanup_user(&pool, "TEST-ATTACH-CONFLICT").await;
    }

    // --- post_tracked_train_name / post_ticket_name -------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_the_owner_can_rename_and_clear() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-OWNER",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router.clone(),
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "  My commute  " }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "rename response: {body:?}");
        assert_eq!(
            body.get("customName").and_then(Value::as_str),
            Some("My commute")
        );

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": null }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "clear response: {body:?}");
        assert_eq!(body.get("customName"), Some(&Value::Null));

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_a_tracked_train_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-BYSTANDER").await;
        seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&owner_token),
            serde_json::json!({ "customName": "Hijacked" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-BYSTANDER").await;
        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_a_too_long_name_is_400() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-TOOLONG").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-TOOLONG",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "a".repeat(101) }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.as_str().is_some_and(|s| !s.contains('_')),
            "400 body leaked an identifier: {body:?}"
        );

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-TOOLONG").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_ticket_name -- --ignored --test-threads=1`"]
    async fn post_ticket_name_the_owner_can_rename_a_standalone_ticket() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TICKET-OWNER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "source": "manual" }),
        )
        .await;
        let ticket_id = created
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

        let (status, body) = post_json(
            router,
            format!("/Train/tickets/{ticket_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "Mum's ticket to Leeds" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "rename response: {body:?}");
        assert_eq!(
            body.get("customName").and_then(Value::as_str),
            Some("Mum's ticket to Leeds")
        );

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TICKET-OWNER").await;
    }

    // --- post_track (schedule-first matching, Task 7) -----------------------

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_track_schedule_matches -- --ignored --test-threads=1`"]
    async fn post_track_schedule_matches_a_pin_whose_train_a_live_movement_would_have_missed() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-SCHEDULE-MATCH").await;

        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-ROUTE-EUS-STANOX', 'EUS', 'EUSTON', 'LONDON EUSTON', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        // `validate_pin` rejects any `scheduled_departure` more than
        // `MAX_PIN_AGE` (6h) in the past relative to the real wall clock at
        // test time, so this can't be a fixed calendar date -- it's derived
        // from `Utc::now()` instead, then converted to its own Europe/London
        // local wall time/date so the seeded `schedule_line_population`
        // entry's `booked_departure` and `service_date` land exactly where
        // `attempt_schedule_match`'s `london_to_utc` conversion will look for
        // them, whatever day this test actually runs on.
        let scheduled_departure = chrono::Utc::now();
        let london_now = scheduled_departure.with_timezone(&chrono_tz::Europe::London);
        let service_date = london_now.date_naive();
        let booked_departure = london_now.format("%H:%M").to_string();

        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ('west-coast-main-line', $1, $2) \
             ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
        )
        .bind(service_date)
        .bind(serde_json::json!([{
            "uid": "C88888",
            "calling_points": [{
                "tiploc": "EUSTON ",
                "kind": "Origin",
                "booked_arrival": null,
                "booked_departure": booked_departure,
                "is_half_minute_arrival": false,
                "is_half_minute_departure": false
            }]
        }]))
        .execute(&pool)
        .await
        .expect("seed schedule_line_population");

        // This is the one route test that needs a real candidate line --
        // the shared `test_app` fixture's `schedule_crs_line_index` stays
        // empty, matching every other test's fixture default.
        let app = test_app_with_schedule_index(
            pool.clone(),
            std::collections::HashMap::from([(
                "EUS".to_string(),
                vec!["west-coast-main-line".to_string()],
            )]),
        );
        let router = test_router(app);
        let (status, body) = post_json(
            router,
            "/Train/track".to_string(),
            Some(&token),
            serde_json::json!({
                "service_date": service_date.to_string(),
                "origin_crs": "EUS",
                "scheduled_departure": scheduled_departure.to_rfc3339(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        assert_eq!(
            body.get("resolutionStatus").and_then(Value::as_str),
            Some("schedule_matched")
        );

        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = 'west-coast-main-line' AND service_date = $1")
            .bind(service_date)
            .execute(&pool)
            .await
            .expect("cleanup population");
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-ROUTE-EUS-STANOX'")
            .execute(&pool)
            .await
            .expect("cleanup stanox_crs");
        cleanup_user(&pool, "TEST-ROUTE-SCHEDULE-MATCH").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_track_schedule_matches -- --ignored --test-threads=1`"]
    async fn post_track_with_no_candidate_line_stays_pending() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-SCHEDULE-NO-MATCH").await;
        let router = test_router(test_app(pool.clone()));

        let now = chrono::Utc::now();
        let (status, body) = post_json(
            router,
            "/Train/track".to_string(),
            Some(&token),
            serde_json::json!({
                "service_date": now.date_naive().to_string(),
                "origin_crs": "ZZZ",
                "scheduled_departure": now.to_rfc3339(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        assert_eq!(
            body.get("resolutionStatus").and_then(Value::as_str),
            Some("pending")
        );

        cleanup_user(&pool, "TEST-ROUTE-SCHEDULE-NO-MATCH").await;
    }

    // --- get_by_tracking_id -------------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TRACKID-401-OWNER").await;
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-TRACKID-401-OWNER",
            Some("A11111"),
            "2026-08-29".parse().unwrap(),
        )
        .await;

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
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-TRACKID-OWNER",
            Some("A22222"),
            "2026-08-29".parse().unwrap(),
        )
        .await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) =
            request(router, format!("/Train/{tracking_id}"), Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

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
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

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
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-TRACKID-REAL-OWNER",
            Some("A33333"),
            service_date,
        )
        .await;
        seed_station_sample(&pool, "KGX", "EDB", "13:45").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) =
            request(router, format!("/Train/{tracking_id}"), Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("id").and_then(Value::as_i64), Some(tracking_id));
        assert_eq!(body.get("trainUid").and_then(Value::as_str), Some("A33333"));
        assert_eq!(
            body.get("lastReportedLocation").and_then(Value::as_str),
            Some("York")
        );
        // blend_darwin_eta overlay: eta_source flips to darwin-estimated and
        // eta_next becomes a concrete timestamp derived from the seeded
        // station sample, not whatever train_current_state itself held
        // (nothing -- this fixture never seeded eta_next/eta_source there).
        assert_eq!(
            body.get("etaSource").and_then(Value::as_str),
            Some("darwin-estimated")
        );
        assert!(
            body.get("etaNext").and_then(Value::as_str).is_some(),
            "etaNext should be populated by the overlay: {body:?}"
        );

        cleanup_station_sample(&pool, "KGX").await;
        cleanup_user(&pool, "TEST-TRACKID-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_by_tracking_id_includes_journey_stops -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_includes_journey_stops_once_a_schedule_match_exists() {
        let pool = connect().await;
        let user_id = "TEST-JS-ROUTE-OWNER";
        let token = seed_session(&pool, user_id).await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();

        // `find_or_create_train_with_schedule_match` seeds a `trains` row with
        // real `calling_points`, exactly like a successful schedule match would.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JSR-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);
        let trains_id = crate::data::trains::find_or_create_train_with_schedule_match(
            &pool,
            "TEST-JSR-UID",
            service_date,
            "KGX",
            service_date.and_hms_opt(9, 0, 0).unwrap().and_utc(),
            None,
            "line-a",
            &calling_points,
        )
        .await
        .expect("seed a schedule-matched trains row");

        let (tracking_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, resolution_status, trains_id) \
             VALUES ($1, $2, 'KGX', $3, 'schedule_matched', $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(service_date.and_hms_opt(9, 0, 0).unwrap().and_utc())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed fixture train_subscriptions row");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(router, format!("/Train/{tracking_id}"), Some(&token)).await;

        assert_eq!(status, StatusCode::OK);
        let stops = body
            .get("journeyStops")
            .expect("journeyStops present")
            .as_array()
            .expect("array");
        assert_eq!(stops.len(), 1);
        assert_eq!(
            stops[0]["crs"],
            Value::Null,
            "TEST-JSR-ORIGIN has no stanox_crs row in this fixture"
        );
        assert_eq!(stops[0]["kind"], Value::String("Origin".to_string()));

        cleanup_user(&pool, user_id).await;
    }

    // --- delete_tracked_train -------------------------------------------------

    /// Issues `DELETE /Train/{trackingId}`, optionally with a session
    /// cookie. Mirrors `request` above -- a success here is `204 No
    /// Content` with an empty body, so an empty body is treated as
    /// `Value::Null` rather than fed to `serde_json::from_slice` (which
    /// would otherwise fail to parse it and mask a real body on any other
    /// status).
    async fn delete_request(
        router: axum::Router,
        tracking_id: i64,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("DELETE")
            .uri(format!("/Train/{tracking_id}"));
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-DELETE-401-OWNER").await;
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-DELETE-401-OWNER",
            Some("D11111"),
            "2026-08-29".parse().unwrap(),
        )
        .await;

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
    async fn delete_tracked_train_a_non_owner_session_gets_the_same_404_as_unknown_and_the_row_survives()
     {
        let pool = connect().await;
        seed_session(&pool, "TEST-DELETE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-DELETE-OTHER").await;
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-DELETE-OWNER",
            Some("D22222"),
            "2026-08-29".parse().unwrap(),
        )
        .await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_request(router, tracking_id, Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

        // Confirms the delete was a genuine no-op for a non-owner, not just
        // a 404 with the row quietly gone anyway.
        let still_there = crate::data::train_tracking::get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train state");
        assert!(
            still_there.is_some(),
            "row should survive a non-owner's delete attempt"
        );

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
        assert_eq!(
            body,
            Value::String("no tracked train with that id".to_string())
        );

        cleanup_user(&pool, "TEST-DELETE-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_tracked_train -- --ignored --test-threads=1`"]
    async fn delete_tracked_train_the_owner_can_delete_it() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-DELETE-REAL-OWNER").await;
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-DELETE-REAL-OWNER",
            Some("D33333"),
            "2026-08-29".parse().unwrap(),
        )
        .await;

        let router = test_router(test_app(pool.clone()));
        let (status, _body) = delete_request(router, tracking_id, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::NO_CONTENT);

        let gone = crate::data::train_tracking::get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train state");
        assert!(
            gone.is_none(),
            "row should be gone after the owner deletes it"
        );

        // The tracked_trains row (and its trains_id FK) is already gone by
        // this point -- cleanup_user's own `trains` cleanup joins through
        // tracked_trains.trains_id, so it can't reach this row's shared
        // `trains` identity anymore. Clean it up directly by its known
        // fixture train_uid instead.
        sqlx::query("DELETE FROM trains WHERE train_uid = 'D33333'")
            .execute(&pool)
            .await
            .expect("cleanup fixture trains row orphaned by the delete route itself");
        cleanup_user(&pool, "TEST-DELETE-REAL-OWNER").await;
    }

    // --- delete_ticket (Decision 2 of
    // docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md) ---

    /// Seeds one `tracked_train_tickets` fixture row directly via
    /// `train_tracking::create_ticket` -- `tracking_id: None` creates a
    /// STANDALONE ticket, matching `create_ticket`'s own documented
    /// convention. Returns the new row's `id`.
    async fn seed_ticket(pool: &PgPool, user_id: &str, tracking_id: Option<i64>) -> i64 {
        let entry = common::TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
        };
        crate::data::train_tracking::create_ticket(pool, tracking_id, &entry, user_id)
            .await
            .expect("insert fixture ticket")
    }

    /// Issues `DELETE /Train/tickets/{ticketId}`, optionally with a session
    /// cookie -- mirrors `delete_request` above, for the ticket-scoped
    /// sibling route.
    async fn delete_ticket_request(
        router: axum::Router,
        ticket_id: i64,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("DELETE")
            .uri(format!("/Train/tickets/{ticket_id}"));
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TICKET-DELETE-401-OWNER").await;
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-401-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, ticket_id, None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));

        cleanup_user(&pool, "TEST-TICKET-DELETE-401-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_non_owner_session_gets_the_same_404_as_unknown_and_the_row_survives() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-TICKET-DELETE-OTHER").await;
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, ticket_id, Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no ticket with that id".to_string()));

        let still_there = crate::data::train_tracking::get_ticket_owned(
            &pool,
            ticket_id,
            "TEST-TICKET-DELETE-OWNER",
        )
        .await
        .expect("read ticket");
        assert!(
            still_there.is_some(),
            "row should survive a non-owner's delete attempt"
        );

        cleanup_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
        cleanup_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_nonexistent_id_is_404_with_the_unchanged_message() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-NOTFOUND").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, 99999999, Some(&token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no ticket with that id".to_string()));

        cleanup_user(&pool, "TEST-TICKET-DELETE-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_the_owner_can_delete_a_standalone_ticket() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        // tracking_id: None -- confirms this route applies uniformly to a
        // STANDALONE ticket, not just an attached one.
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-REAL-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, _body) = delete_ticket_request(router, ticket_id, Some(&token)).await;

        assert_eq!(status, StatusCode::NO_CONTENT);

        let gone = crate::data::train_tracking::get_ticket_owned(
            &pool,
            ticket_id,
            "TEST-TICKET-DELETE-REAL-OWNER",
        )
        .await
        .expect("read ticket");
        assert!(
            gone.is_none(),
            "ticket row should be gone after the owner deletes it"
        );

        cleanup_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_deleted_ticket_disappears_from_every_other_ticket_reading_route() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-CASCADE-READS").await;
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-TICKET-DELETE-CASCADE-READS",
            Some("D44444"),
            "2026-08-29".parse().unwrap(),
        )
        .await;
        let ticket_id =
            seed_ticket(&pool, "TEST-TICKET-DELETE-CASCADE-READS", Some(tracking_id)).await;

        let router = test_router(test_app(pool.clone()));
        let (status, _) = delete_ticket_request(router.clone(), ticket_id, Some(&token)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // The list route no longer includes it...
        let (status, tickets) = request(
            router.clone(),
            "/Train/tickets/mine".to_string(),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        assert!(
            rows.iter()
                .all(|r| r.get("id").and_then(Value::as_i64) != Some(ticket_id)),
            "deleted ticket should not appear in the mine list: {rows:?}"
        );

        // ...and the per-ticket delay-repay route 404s, same as any other
        // ticket that never existed -- proves Decision 3's "no orphaned
        // estimate" claim (both reads recompute fresh from the row on
        // every request; once the row is gone, there is nothing to find).
        let (status, body) = request(
            router,
            format!("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no ticket with that id for that tracked train".to_string())
        );

        cleanup_user(&pool, "TEST-TICKET-DELETE-CASCADE-READS").await;
    }

    // --- get_by_uid_and_date -- PUBLIC and UNSCOPED (Task 19) -----------------
    //
    // This route used to require `AuthenticatedUser` and 404 for anyone but
    // the pin's own owner (see git history for the three tests this section
    // replaces: `get_by_uid_and_date_no_session_is_401`,
    // `get_by_uid_and_date_a_non_owner_session_gets_the_same_404_as_unresolved`,
    // `get_by_uid_and_date_an_unresolved_pair_is_404_with_the_unchanged_message`
    // -- their own 401-for-anonymous / 404-for-non-owner assertions describe
    // exactly the ownership contract this task deliberately removes). It is
    // now public: reads `trains` directly via
    // `crate::data::trains::get_public_train_state`, with no
    // `tracked_trains` ownership check anywhere in the call path.

    /// Seeds a `trains` row directly (no owning `tracked_trains` row at
    /// all) with a linked `train_current_state` row, proving this route
    /// works for a train nobody has ever subscribed to -- the core claim of
    /// "trains are shared entities" this task exists to enable. Returns the
    /// new `trains.id`.
    async fn seed_public_train(
        pool: &PgPool,
        train_uid: &str,
        service_date: chrono::NaiveDate,
    ) -> i64 {
        let trains_id = crate::data::trains::find_or_create_train(pool, train_uid, service_date)
            .await
            .expect("find_or_create_train for fixture");
        crate::data::trains::mark_train_resolved(pool, trains_id, "1A23")
            .await
            .expect("mark_train_resolved for fixture");
        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'York', 'DEPARTURE', 12, 'Newcastle', NOW())",
        )
        .bind(trains_id)
        .execute(pool)
        .await
        .expect("insert fixture train_current_state row");
        trains_id
    }

    async fn cleanup_public_train(pool: &PgPool, train_uid: &str) {
        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_uid_and_date_is_public_and_unscoped -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_is_public_and_unscoped() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = seed_public_train(&pool, "TEST-PUBLIC-BY-UID", service_date).await;

        let router = test_router(test_app(pool.clone()));

        // No Authorization header/session cookie at all -- this must
        // succeed, not 401. No `tracked_trains` row of any kind exists for
        // this train either -- this must succeed, not 404.
        let (status, body) = request(
            router,
            format!("/Train/by-uid/TEST-PUBLIC-BY-UID/{service_date}"),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        // `trainsId`, NOT `id` (review finding C3): every `/Train/{trackingId}`
        // route in this app reads its path id as a `train_subscriptions.id`,
        // and both surrogate-key spaces are `BIGSERIAL` starting at 1. The
        // old `id` name let this shared-row key be mistaken for a tracking
        // id -- which is exactly what the frontend page for this route was
        // doing. The absence of a bare `id` key is asserted explicitly
        // below, so a future rename back to `id` fails here loudly.
        assert_eq!(
            body.get("trainsId").and_then(Value::as_i64),
            Some(trains_id)
        );
        assert!(
            body.get("id").is_none(),
            "the public response must not carry a bare `id` that could be read as a \
             tracking id: {body:?}"
        );
        assert_eq!(
            body.get("trainUid").and_then(Value::as_str),
            Some("TEST-PUBLIC-BY-UID")
        );
        assert_eq!(
            body.get("lastReportedLocation").and_then(Value::as_str),
            Some("York")
        );
        assert_eq!(body.get("delayMinutes").and_then(Value::as_i64), Some(12));

        cleanup_public_train(&pool, "TEST-PUBLIC-BY-UID").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_uid_and_date_a_session_cookie_does_not_gate_a_train_owned_by_someone_else \
                -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_a_session_cookie_does_not_gate_a_train_owned_by_someone_else() {
        // Inverts what this route used to assert: a caller who is logged
        // in, but is NOT the subscriber who tracked this train, used to get
        // the same 404 as "unknown". Now there's no ownership concept in
        // this call path at all -- any authenticated (or anonymous) caller
        // sees the same public row.
        let pool = connect().await;
        seed_session(&pool, "TEST-UIDDATE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-UIDDATE-OTHER").await;
        let service_date: chrono::NaiveDate = "2026-08-29".parse().unwrap();
        seed_tracked_train(&pool, "TEST-UIDDATE-OWNER", Some("B22222"), service_date).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(
            router,
            format!("/Train/by-uid/B22222/{service_date}"),
            Some(&other_token),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        assert_eq!(body.get("trainUid").and_then(Value::as_str), Some("B22222"));

        cleanup_user(&pool, "TEST-UIDDATE-OWNER").await;
        cleanup_user(&pool, "TEST-UIDDATE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_uid_and_date_an_unknown_pair_is_404 -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_an_unknown_pair_is_404() {
        let pool = connect().await;

        let router = test_router(test_app(pool.clone()));
        // No session cookie at all -- must still 404, not 401: an unknown
        // identity is a real "not found", independent of who's asking.
        let (status, body) = request(
            router,
            "/Train/by-uid/NOSUCHUID/2026-08-29".to_string(),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no known train for that uid/date".to_string())
        );
    }

    /// Reproduces the "View live status 404s on a real search result" bug
    /// this task fixes: a `(train_uid, service_date)` that IS
    /// search-visible (present in `schedule_destination_departures`, the
    /// same table `/trains` search reads) but has never been tracked and
    /// has no `trains` row at all yet -- the exact state a train sits in
    /// between being searched and either being tracked or activating in
    /// TRUST. Before the fix this 404'd identically to a wholly unknown
    /// uid (`get_by_uid_and_date_an_unknown_pair_is_404` above); after it,
    /// the route read-triggers `find_or_create_train` and returns the new,
    /// bare shared row.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_uid_and_date_creates_the_shared_row_for_a_search_visible_train \
                -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_creates_the_shared_row_for_a_search_visible_train() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let train_uid = "TEST-SEARCH-VISIBLE-UID";

        // Seed ONLY `schedule_destination_departures` (what the search page
        // reads) -- deliberately no `trains` row, no `tracked_trains` row,
        // nothing else. This is the fixture the bug report describes: "a
        // real, valid search result" with no prior tracking/TRUST activity.
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs) \
             VALUES ($1, 'EDB', '12:00:00', $2, 'KGX')",
        )
        .bind(service_date)
        .bind(train_uid)
        .execute(&pool)
        .await
        .expect("seed fixture schedule_destination_departures row");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = request(
            router,
            format!("/Train/by-uid/{train_uid}/{service_date}"),
            None,
        )
        .await;

        assert_eq!(
            status,
            StatusCode::OK,
            "a search-visible train must no longer 404: {body:?}"
        );
        assert_eq!(body.get("trainUid").and_then(Value::as_str), Some(train_uid));
        assert!(
            body.get("trainsId").and_then(Value::as_i64).is_some(),
            "the read must have created and returned a real shared trains row: {body:?}"
        );
        // Regression coverage for the final whole-branch review's Finding
        // 3: this row was created BARE (`origin_crs`/`train_id` both
        // `None`) by the read-triggered `find_or_create_train` above, but
        // the identity IS provably CIF-scheduled (the same
        // `schedule_destination_departures` row seeded above), so
        // `attach_journey_stops_public` must not gate `journeyStops` to
        // `null` on `origin_crs`/`train_id` being unset -- it must still
        // build a fallback stop list from `schedule_destination_departures`.
        let stops = body
            .get("journeyStops")
            .expect("journeyStops present")
            .as_array()
            .expect("journeyStops must be a non-null array for a provably CIF-scheduled train, \
                     even when found via the bare find_or_create_train path");
        assert_eq!(
            stops.len(),
            2,
            "the seeded origin row (KGX) plus the synthetic EDB terminus"
        );
        assert_eq!(stops[0]["crs"], Value::String("KGX".to_string()));
        assert_eq!(stops[1]["crs"], Value::String("EDB".to_string()));
        assert_eq!(stops[1]["kind"], Value::String("Terminate".to_string()));

        // A second read must be idempotent -- no duplicate-row error, same
        // `trainsId` both times.
        let router = test_router(test_app(pool.clone()));
        let (status, second_body) = request(
            router,
            format!("/Train/by-uid/{train_uid}/{service_date}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {second_body:?}");
        assert_eq!(
            second_body.get("trainsId"),
            body.get("trainsId"),
            "a repeat read must resolve to the SAME trains row, not create a second one"
        );

        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
    }

    /// The security-critical test for this task: seeds a `tracked_trains`
    /// row with a real subscriber's own private `custom_name`, linked (via
    /// `trains_id`) to the exact same shared `trains` row the public route
    /// reads. Proves that an anonymous caller -- and separately, a
    /// different, authenticated user -- reading that train's public state
    /// never sees that subscriber's `customName` (or any other
    /// per-subscription field) anywhere in the response, no matter what
    /// `tracked_trains` holds for it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_uid_and_date_never_exposes_another_users_custom_name \
                -- --ignored --test-threads=1`"]
    async fn get_by_uid_and_date_never_exposes_another_users_custom_name() {
        let pool = connect().await;
        seed_session(&pool, "TEST-UIDDATE-PRIVACY-OWNER").await;
        let other_token = seed_session(&pool, "TEST-UIDDATE-PRIVACY-OTHER").await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-UIDDATE-PRIVACY-OWNER",
            Some("B44444"),
            service_date,
        )
        .await;

        // Give the owner's own subscription a private custom name -- this
        // must never appear in the public route's response, to anyone.
        let renamed = crate::data::train_tracking::rename_tracked_train(
            &pool,
            tracking_id,
            "TEST-UIDDATE-PRIVACY-OWNER",
            Some("My secret commute nickname"),
        )
        .await
        .expect("set fixture custom_name");
        assert!(renamed, "rename must succeed for the real owner");

        let router = test_router(test_app(pool.clone()));

        // Case 1: fully anonymous caller.
        let (status, anon_body) = request(
            router.clone(),
            format!("/Train/by-uid/B44444/{service_date}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {anon_body:?}");
        assert!(
            anon_body.get("customName").is_none(),
            "public response must never carry a customName field at all: {anon_body:?}"
        );
        let anon_raw = serde_json::to_string(&anon_body).unwrap();
        assert!(
            !anon_raw.contains("secret commute nickname"),
            "the owner's private custom_name text leaked into the public response: {anon_raw}"
        );

        // Case 2: a different, authenticated user (not the owner).
        let (status, other_body) = request(
            router,
            format!("/Train/by-uid/B44444/{service_date}"),
            Some(&other_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {other_body:?}");
        assert!(
            other_body.get("customName").is_none(),
            "public response must never carry a customName field at all: {other_body:?}"
        );
        let other_raw = serde_json::to_string(&other_body).unwrap();
        assert!(
            !other_raw.contains("secret commute nickname"),
            "the owner's private custom_name text leaked into another user's response: {other_raw}"
        );

        cleanup_user(&pool, "TEST-UIDDATE-PRIVACY-OWNER").await;
        cleanup_user(&pool, "TEST-UIDDATE-PRIVACY-OTHER").await;
    }

    // --- post_track_by_uid (Task 20: the NR-primary tracking entry point) ---

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_no_session_is_401 -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_no_session_is_401() {
        let pool = connect().await;
        let router = test_router(test_app(pool));

        // No session cookie at all -- unlike GET /Train/by-uid/{uid}/{date}
        // (Task 19, deliberately public), this WRITE route must stay behind
        // normal user authentication.
        let (status, body) = post_json(
            router,
            "/Train/by-uid/TEST-TRACK-BY-UID-NOAUTH/2026-09-07/track".to_string(),
            None,
            serde_json::json!({}),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "response: {body:?}");
        assert_eq!(body, Value::String("no session".to_string()));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_creates_a_subscription_that_inherits_known_schedule_data \
                -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_creates_a_subscription_that_inherits_known_schedule_data() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TRACK-BY-UID-KNOWN").await;
        let router = test_router(test_app(pool.clone()));
        let service_date: chrono::NaiveDate = "2026-09-07".parse().unwrap();
        let scheduled_departure = service_date.and_hms_opt(19, 15, 0).unwrap().and_utc();

        sqlx::query(
            "INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure) \
             VALUES ('TEST-TRACK-BY-UID-KNOWN-UID', $1, 'EUS', $2)",
        )
        .bind(service_date)
        .bind(scheduled_departure)
        .execute(&pool)
        .await
        .expect("seed a trains row with known schedule data");

        let (status, body) = post_json(
            router,
            format!("/Train/by-uid/TEST-TRACK-BY-UID-KNOWN-UID/{service_date}/track"),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        let tracking_id = body
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present");

        // No pending/schedule_matched waypoint at all -- trains_id and the
        // schedule-derived pin_* columns must be set immediately, in this
        // same request.
        let (trains_id, resolution_status, pin_origin_crs, pin_scheduled_departure): (
            Option<i64>,
            String,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
        ) = sqlx::query_as(
            "SELECT trains_id, resolution_status, pin_origin_crs, pin_scheduled_departure \
             FROM train_subscriptions WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back the new subscription");
        assert!(trains_id.is_some(), "trains_id must be set immediately");
        assert_eq!(
            pin_origin_crs,
            Some("EUS".to_string()),
            "known schedule data must be inherited immediately, not left NULL"
        );
        assert_eq!(pin_scheduled_departure, Some(scheduled_departure));
        // `resolution_status` DOES stay at this table's own `DEFAULT
        // 'pending'` here -- deliberately, not an oversight:
        // `create_subscription_for_train` never touches this column, and
        // leaving it 'pending' is what routes this row into
        // `trust-consumer::process::apply_reference_reload`'s
        // `by_train_uid` fast-path branch once Task 21's read cutover picks
        // up this row's `trains_id` (see that function's own doc comment).
        // "No pending/schedule_matched waypoint" (the design spec's own
        // phrasing) refers to IDENTITY never being in question here -- it
        // does not mean this literal DB value differs from the legacy
        // path's own 'pending' value.
        assert_eq!(resolution_status, "pending");

        cleanup_user(&pool, "TEST-TRACK-BY-UID-KNOWN").await;
        cleanup_public_train(&pool, "TEST-TRACK-BY-UID-KNOWN-UID").await;
    }

    /// The accepted §1 gap: a bare `train_uid`/`date` with no schedule match
    /// yet. Proves the whole request succeeds end-to-end (route ->
    /// find_or_create_train -> create_subscription_for_train) and persists
    /// `NULL` pin columns rather than erroring -- only possible because this
    /// task's own migration dropped their `NOT NULL` constraint.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_allows_a_bare_uid_with_no_schedule_data \
                -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_allows_a_bare_uid_with_no_schedule_data() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TRACK-BY-UID-BARE").await;
        let router = test_router(test_app(pool.clone()));
        let service_date: chrono::NaiveDate = "2026-09-07".parse().unwrap();

        // No pre-existing `trains` row at all -- find_or_create_train must
        // create one fresh, with no schedule data to inherit.
        let (status, body) = post_json(
            router,
            format!("/Train/by-uid/TEST-TRACK-BY-UID-BARE-UID/{service_date}/track"),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        let tracking_id = body
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present");

        let (trains_id, pin_origin_crs, pin_scheduled_departure): (
            Option<i64>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
        ) = sqlx::query_as(
            "SELECT trains_id, pin_origin_crs, pin_scheduled_departure FROM train_subscriptions \
             WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back the new subscription");
        assert!(
            trains_id.is_some(),
            "trains_id must still be set immediately"
        );
        assert_eq!(pin_origin_crs, None);
        assert_eq!(pin_scheduled_departure, None);

        cleanup_user(&pool, "TEST-TRACK-BY-UID-BARE").await;
        cleanup_public_train(&pool, "TEST-TRACK-BY-UID-BARE-UID").await;
    }

    /// Explicit idempotency check at the HTTP layer, mirroring the
    /// data-layer test of the same name in `train_tracking::db_tests`: the
    /// SAME authenticated caller hits this route twice for the SAME
    /// `(train_uid, date)`, with no reset in between. Result: the SAME
    /// `trackingId` both times, i.e. one subscription -- this route calls
    /// `create_subscription_for_train`, which was made idempotent per
    /// `(user_id, trains_id)` so that a "Track this train" button a user
    /// can click twice no longer produces duplicate subscriptions,
    /// duplicate `/track/mine` entries and duplicate notification streams.
    /// This test previously asserted the opposite (two distinct ids); it
    /// was inverted alongside the fix, not reverted.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_called_twice_returns_the_same_subscription \
                -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_called_twice_returns_the_same_subscription() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TRACK-BY-UID-TWICE").await;
        let router = test_router(test_app(pool.clone()));
        let service_date: chrono::NaiveDate = "2026-09-07".parse().unwrap();
        let uri = format!("/Train/by-uid/TEST-TRACK-BY-UID-TWICE-UID/{service_date}/track");

        let (status1, body1) = post_json(
            router.clone(),
            uri.clone(),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status1, StatusCode::OK, "first call response: {body1:?}");
        let first_tracking_id = body1
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present on first call");

        let (status2, body2) = post_json(router, uri, Some(&token), serde_json::json!({})).await;
        assert_eq!(status2, StatusCode::OK, "second call response: {body2:?}");
        let second_tracking_id = body2
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present on second call");

        assert_eq!(
            first_tracking_id, second_tracking_id,
            "this route must be idempotent -- a second call for the same (uid, date) by the \
             same user returns the EXISTING subscription rather than creating a second one"
        );

        let (row_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM train_subscriptions WHERE id = $1")
                .bind(first_tracking_id)
                .fetch_one(&pool)
                .await
                .expect("count the subscription row");
        assert_eq!(row_count, 1, "exactly one row must exist after two calls");

        cleanup_user(&pool, "TEST-TRACK-BY-UID-TWICE").await;
        cleanup_public_train(&pool, "TEST-TRACK-BY-UID-TWICE-UID").await;
    }

    // --- legacy POST /Train/track: validation unaffected by Task 20's own
    // nullable-column migration --------------------------------------------

    /// Task 20's migration dropped `tracked_trains.pin_origin_crs`/
    /// `pin_scheduled_departure`'s `NOT NULL` constraint -- required for the
    /// NEW NR-primary path above, which legitimately has no schedule data at
    /// pin time. This proves that relaxation did NOT weaken the LEGACY
    /// `POST /Train/track` path: `validate_pin`'s own application-level gate
    /// still runs and still rejects a malformed pin, and no
    /// `tracked_trains` row is ever created for a rejected request, even
    /// though the database itself no longer has a `NOT NULL` constraint to
    /// fall back on if that application check were ever accidentally
    /// removed.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_still_rejects_a_pin_missing_required_fields_after_nullable_pin_columns_migration \
                -- --ignored --test-threads=1`"]
    async fn post_track_still_rejects_a_pin_missing_required_fields_after_nullable_pin_columns_migration()
     {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-LEGACY-VALIDATION-STILL-ENFORCED").await;
        let router = test_router(test_app(pool.clone()));

        // Empty origin_crs: deserializes fine (still a String, just empty),
        // so only `validate_pin`'s own check -- not serde/axum -- can catch
        // this. This is the scenario Task 20's migration could have
        // silently broken, since the DB column itself would now happily
        // accept this value if it were ever coerced to NULL upstream.
        let (status, body) = post_json(
            router.clone(),
            "/Train/track".to_string(),
            Some(&token),
            serde_json::json!({
                "service_date": "2026-09-07",
                "origin_crs": "",
                "scheduled_departure": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "response: {body:?}");
        assert!(
            body.as_str()
                .is_some_and(|s| s.to_lowercase().contains("station")),
            "expected validate_pin's own empty-origin message: {body:?}"
        );

        let (rejected_row_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM train_subscriptions WHERE user_id = $1")
                .bind("TEST-LEGACY-VALIDATION-STILL-ENFORCED")
                .fetch_one(&pool)
                .await
                .expect("count tracked_trains rows for this user");
        assert_eq!(
            rejected_row_count, 0,
            "a pin rejected by validate_pin must never reach the database"
        );

        // A request missing scheduled_departure entirely (not just empty) is
        // rejected too -- axum's Json extractor itself refuses to
        // deserialize TrackPinRequest without it (a required, non-Option
        // field), independent of anything this migration touched.
        let (status2, body2) = post_json(
            router,
            "/Train/track".to_string(),
            Some(&token),
            serde_json::json!({
                "service_date": "2026-09-07",
                "origin_crs": "EUS",
            }),
        )
        .await;
        assert_ne!(
            status2,
            StatusCode::OK,
            "a pin missing scheduled_departure must not be accepted: {body2:?}"
        );

        cleanup_user(&pool, "TEST-LEGACY-VALIDATION-STILL-ENFORCED").await;
    }

    // --- Fix 2 (review finding C2): NULL pin columns on the two read paths ---

    /// End-to-end at the HTTP layer: track a train the NR-primary way (the
    /// route's own default outcome is a `trains` row with no schedule data,
    /// hence `NULL` pins), then call BOTH read routes that used to decode
    /// those columns into non-`Option` fields.
    ///
    /// Before this fix, `GET /Train/mine` and `GET /Train/{trackingId}` both
    /// returned `500` here -- sqlx cannot decode a `NULL` into `String`/
    /// `DateTime<Utc>`. Driven through the real router (not the data layer
    /// alone) so the assertion covers the whole stack the frontend actually
    /// hits, `blend_darwin_eta`'s own `pin_origin_crs` read included.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                null_pin_subscription_is_readable_on_both_read_routes \
                -- --ignored --test-threads=1`"]
    async fn null_pin_subscription_is_readable_on_both_read_routes() {
        let pool = connect().await;
        let user_id = "TEST-NULL-PIN-READ-ROUTES";
        cleanup_user(&pool, user_id).await;
        let token = seed_session(&pool, user_id).await;
        let service_date: chrono::NaiveDate = "2026-09-07".parse().unwrap();
        let train_uid = "TEST-NULL-PIN-ROUTES-UID";

        let (status, body) = post_json(
            test_router(test_app(pool.clone())),
            format!("/Train/by-uid/{train_uid}/{service_date}/track"),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        let tracking_id = body
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present");

        // Read path 1: GET /Train/mine (list_tracked_trains_for_user).
        let (status, body) = request(
            test_router(test_app(pool.clone())),
            "/Train/mine".to_string(),
            Some(&token),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET /Train/mine must not 500 on a NULL-pin subscription: {body:?}"
        );
        let items = body.as_array().expect("an array of tracked trains");
        assert_eq!(items.len(), 1, "response: {body:?}");
        assert_eq!(
            items[0].get("id").and_then(Value::as_i64),
            Some(tracking_id)
        );
        assert_eq!(
            items[0].get("pinOriginCrs"),
            Some(&Value::Null),
            "the NULL pin must serialize as JSON null, not be omitted or defaulted"
        );
        assert_eq!(items[0].get("pinScheduledDeparture"), Some(&Value::Null));

        // Read path 2: GET /Train/{trackingId} (TRACKED_TRAIN_STATE_SELECT
        // + blend_darwin_eta).
        let (status, body) = request(
            test_router(test_app(pool.clone())),
            format!("/Train/{tracking_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET /Train/{{trackingId}} must not 500 on a NULL-pin subscription: {body:?}"
        );
        assert_eq!(body.get("id").and_then(Value::as_i64), Some(tracking_id));
        assert_eq!(body.get("pinOriginCrs"), Some(&Value::Null));
        assert_eq!(
            body.get("trainUid").and_then(Value::as_str),
            Some(train_uid),
            "the shared trains row's identity still comes through"
        );

        cleanup_user(&pool, user_id).await;
        cleanup_public_train(&pool, train_uid).await;
    }

    // --- Fix 4 (review finding I1): the NR-primary path acquires data ----

    /// The whole of finding I1, end to end through the real router.
    ///
    /// Seeds a train that has ALREADY RUN -- its retained
    /// `trust_event_backlog` history holds an Activation (the only row type
    /// that ever carries a `train_uid`) plus a located origin DEPARTURE and
    /// a later ARRIVAL -- then tracks it via
    /// `POST /Train/by-uid/{uid}/{date}/track` and asserts the shared
    /// `trains` row comes out with BOTH halves it used to be permanently
    /// missing:
    ///
    /// * live-TRUST identity: `train_id`/`resolved_at` from the replay, and
    ///   real `train_movement_events`/`train_current_state` rows;
    /// * schedule data: `origin_crs`/`destination_crs`/`calling_points`/
    ///   `schedule_matched_at`, reached via the replayed origin departure's
    ///   own (CRS, planned time) -- the key a bare `train_uid` otherwise
    ///   has no way to produce.
    ///
    /// Before this fix, `post_track_by_uid` called bare `find_or_create_train`
    /// and stopped: every one of those columns stayed `NULL` forever, since
    /// `attempt_schedule_match` is gated on `trains_id IS NULL` and
    /// `list_pending_pins_for_schedule_match` excludes `trains_id`-bearing
    /// rows outright.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_backfills_schedule_and_movement_data_from_the_backlog \
                -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_backfills_schedule_and_movement_data_from_the_backlog() {
        let pool = connect().await;
        let user_id = "TEST-NR-ENRICH-USER";
        cleanup_user(&pool, user_id).await;
        let token = seed_session(&pool, user_id).await;
        let train_uid = "TEST-NR-ENRICH-UID";
        let train_id = "TEST-NR-ENRICH-TRAINID";

        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-NR-ENRICH-STANOX', 'EUS', 'EUSTON', 'LONDON EUSTON', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-NR-ENRICH-STANOX-2', 'MKC', 'MKNSCEN', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed destination stanox_crs");

        // Same wall-clock derivation as `post_track_schedule_matches_...`:
        // `london_to_utc` resolves the seeded `booked_departure` against
        // the service date's own Europe/London day, so a fixed calendar
        // date would drift in and out of tolerance depending on when this
        // test runs.
        let departure = chrono::Utc::now();
        let london_now = departure.with_timezone(&chrono_tz::Europe::London);
        let service_date = london_now.date_naive();
        let booked_departure = london_now.format("%H:%M").to_string();

        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ('west-coast-main-line', $1, $2) \
             ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
        )
        .bind(service_date)
        .bind(serde_json::json!([{
            "uid": train_uid,
            "calling_points": [
                {
                    "tiploc": "EUSTON ",
                    "kind": "Origin",
                    "booked_arrival": null,
                    "booked_departure": booked_departure,
                    "is_half_minute_arrival": false,
                    "is_half_minute_departure": false
                },
                {
                    "tiploc": "MKNSCEN",
                    "kind": "Terminate",
                    "booked_arrival": booked_departure,
                    "booked_departure": null,
                    "is_half_minute_arrival": false,
                    "is_half_minute_departure": false
                }
            ]
        }]))
        .execute(&pool)
        .await
        .expect("seed schedule_line_population");

        // The retained TRUST history. The Activation is what
        // `find_train_id_by_uid` looks for; the DEPARTURE is what supplies
        // the (CRS, planned time) the schedule lookup needs.
        for (msg_type, event_type, crs, dedup) in [
            ("0001", None, None, "test-nr-enrich-act"),
            ("0003", Some("DEPARTURE"), Some("EUS"), "test-nr-enrich-dep"),
            ("0003", Some("ARRIVAL"), Some("MKC"), "test-nr-enrich-arr"),
        ] {
            sqlx::query(
                "INSERT INTO trust_event_backlog \
                    (crs, train_uid, train_id, service_date, msg_type, event_type, \
                     planned_timestamp, actual_timestamp, variation_status, dedup_key) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $7, 'ON TIME', $8) \
                 ON CONFLICT (dedup_key) DO NOTHING",
            )
            .bind(crs)
            // Only the Activation ever carries a train_uid, exactly as the
            // real trust-backlog-consumer writes it.
            .bind(if msg_type == "0001" {
                Some(train_uid)
            } else {
                None
            })
            .bind(train_id)
            .bind(service_date)
            .bind(msg_type)
            .bind(event_type)
            .bind(departure)
            .bind(dedup)
            .execute(&pool)
            .await
            .expect("seed trust_event_backlog row");
        }

        let app = test_app_with_schedule_index(
            pool.clone(),
            std::collections::HashMap::from([(
                "EUS".to_string(),
                vec!["west-coast-main-line".to_string()],
            )]),
        );
        let (status, body) = post_json(
            test_router(app),
            format!("/Train/by-uid/{train_uid}/{service_date}/track"),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");
        let tracking_id = body
            .get("trackingId")
            .and_then(Value::as_i64)
            .expect("trackingId present");

        #[allow(clippy::type_complexity)]
        let (
            trains_id,
            row_train_id,
            resolved_at,
            origin_crs,
            destination_crs,
            calling_points,
            schedule_matched_at,
        ): (
            i64,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
            Option<chrono::DateTime<chrono::Utc>>,
        ) = sqlx::query_as(
            "SELECT id, train_id, resolved_at, origin_crs, destination_crs, calling_points, \
                    schedule_matched_at \
             FROM trains WHERE train_uid = $1 AND service_date = $2",
        )
        .bind(train_uid)
        .bind(service_date)
        .fetch_one(&pool)
        .await
        .expect("the shared trains row must exist");

        // Half 1: the backlog replay's live-TRUST identity.
        assert_eq!(
            row_train_id,
            Some(train_id.to_string()),
            "the replay must mark the shared row resolved with TRUST's own train_id"
        );
        assert!(resolved_at.is_some());

        // Half 2: schedule data, reached via the replayed origin departure.
        assert_eq!(
            origin_crs,
            Some("EUS".to_string()),
            "the shared row must have acquired schedule data -- this is the whole of finding I1"
        );
        assert_eq!(destination_crs, Some("MKC".to_string()));
        assert!(schedule_matched_at.is_some());
        let calling_points = calling_points.expect("calling points must be populated");
        assert_eq!(
            calling_points.as_array().map(Vec::len),
            Some(2),
            "calling points: {calling_points:?}"
        );

        // And the replayed movement history itself landed on the shared
        // tables, keyed on trains_id.
        let (event_count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count replayed movement events");
        assert_eq!(
            event_count, 2,
            "both Movement rows must be replayed (the Activation itself posts no event)"
        );
        let (last_location,): (Option<String>,) = sqlx::query_as(
            "SELECT last_reported_location FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("the shared current-state row must exist");
        assert_eq!(last_location, Some("MKC".to_string()));

        // The subscription itself flipped to resolved off the same replay.
        let (resolution_status,): (String,) =
            sqlx::query_as("SELECT resolution_status FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read back the subscription");
        assert_eq!(resolution_status, "resolved");

        sqlx::query("DELETE FROM trust_event_backlog WHERE train_id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = 'west-coast-main-line' AND service_date = $1")
            .bind(service_date)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-NR-ENRICH-STANOX%'")
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
        cleanup_public_train(&pool, train_uid).await;
    }

    /// The honest no-op half of the same fix: a train with NO retained
    /// backlog history at all (it has not run yet -- the ordinary case for
    /// someone tracking tomorrow's commute) must still create the
    /// subscription successfully and simply leave the shared row bare, for
    /// live `trust-consumer` resolution to fill in later. Proves the
    /// enrichment is genuinely best-effort and cannot fail the request.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_track_by_uid_with_no_backlog_history_still_succeeds \
                -- --ignored --test-threads=1`"]
    async fn post_track_by_uid_with_no_backlog_history_still_succeeds() {
        let pool = connect().await;
        let user_id = "TEST-NR-ENRICH-NOHISTORY";
        cleanup_user(&pool, user_id).await;
        let token = seed_session(&pool, user_id).await;
        let train_uid = "TEST-NR-ENRICH-NOHISTORY-UID";
        let service_date: chrono::NaiveDate = "2026-09-07".parse().unwrap();

        let (status, body) = post_json(
            test_router(test_app(pool.clone())),
            format!("/Train/by-uid/{train_uid}/{service_date}/track"),
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "response: {body:?}");

        let (train_id, origin_crs): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT train_id, origin_crs FROM trains WHERE train_uid = $1")
                .bind(train_uid)
                .fetch_one(&pool)
                .await
                .expect("the shared trains row must still have been created");
        assert_eq!(train_id, None, "nothing to replay -> nothing invented");
        assert_eq!(origin_crs, None);

        cleanup_user(&pool, user_id).await;
        cleanup_public_train(&pool, train_uid).await;
    }
}
