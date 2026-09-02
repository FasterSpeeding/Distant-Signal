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
        .route("/notifications/vapid-public-key", axum::routing::get(get_vapid_public_key))
        .route("/notifications/subscribe", axum::routing::post(post_subscribe))
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
    notifications::upsert_push_subscription(&app.database, &user.id, &body.endpoint, &body.keys.p256dh, &body.keys.auth)
        .await
        .map_err(|err| {
            tracing::error!(error = ?err, "failed to upsert push subscription");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to save subscription".to_string())
        })?;
    Ok(StatusCode::NO_CONTENT)
}
