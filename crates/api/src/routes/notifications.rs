//! `/public/notifications`: the two endpoints the browser-side push
//! subscribe flow needs. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decision 6 for the frontend flow this serves.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::notifications;

pub fn router() -> Router {
    Router::new()
        .route(
            "/notifications/vapid-public-key",
            axum::routing::get(get_vapid_public_key),
        )
        .route(
            "/notifications/subscribe",
            axum::routing::post(post_subscribe),
        )
}

/// Unauthenticated on purpose -- the browser needs this key BEFORE it has
/// established any session-gated call, to construct the
/// `PushManager.subscribe({ applicationServerKey })` call itself (Decision
/// 6). It is public key material; there is nothing to protect here.
async fn get_vapid_public_key(State(app): State<App>) -> String {
    app.config.vapid_public_key.clone()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscribeRequest {
    endpoint: String,
    keys: SubscribeKeys,
}

#[derive(Debug, Deserialize)]
struct SubscribeKeys {
    p256dh: String,
    auth: String,
}

/// Authenticated (Decision 6: a 401 here is what the frontend's
/// `useNeedsLogin()` reacts to). Body shape matches the Push API's own
/// `PushSubscription.toJSON()` output directly, so the frontend can pass
/// it through with no reshaping.
async fn post_subscribe(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<SubscribeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // SECURITY (2026-09 review): rejects a non-`https` endpoint or one
    // whose host resolves to a private/loopback/link-local/multicast IP,
    // BEFORE it ever reaches the database -- see
    // `notifications::validate_push_endpoint`'s own doc comment. Without
    // this, any logged-in user could register an arbitrary internal URL
    // as their push endpoint, and `crates/notifier/src/send.rs` would
    // later issue a VAPID-signed POST to it from inside the cluster on
    // every notification: an SSRF vector.
    notifications::validate_push_endpoint(&body.endpoint)
        .await
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    match notifications::upsert_push_subscription(
        &app.database,
        &user.id,
        &body.endpoint,
        &body.keys.p256dh,
        &body.keys.auth,
    )
    .await
    {
        Ok(notifications::PushSubscriptionUpsert::Saved) => Ok(StatusCode::NO_CONTENT),
        // SECURITY (2026-09 review): rejected, not silently reassigned --
        // see `upsert_push_subscription`'s own doc comment.
        Ok(notifications::PushSubscriptionUpsert::EndpointOwnedByAnotherUser) => Err((
            StatusCode::CONFLICT,
            "that push endpoint is already registered to a different account".to_string(),
        )),
        Err(err) => {
            tracing::error!(error = ?err, "failed to upsert push subscription");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to save subscription".to_string(),
            ))
        }
    }
}
