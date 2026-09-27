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

/// Real push endpoints are well under 1 KB (FCM, Mozilla autopush and
/// Apple's web push all issue a few hundred characters).
const MAX_PUSH_ENDPOINT_CHARS: usize = 2048;
/// `p256dh` is a 65-byte P-256 point and `auth` a 16-byte secret, both
/// base64url: 87 and 22 characters. The bounds leave room for padding.
const MAX_P256DH_CHARS: usize = 128;
const MAX_AUTH_CHARS: usize = 64;

fn is_base64url(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
}

/// Size and shape checks on the subscription body (API-7). Without them
/// the keys went into TEXT columns unbounded.
fn validate_subscribe_request(body: &SubscribeRequest) -> Result<(), String> {
    super::validate_short_text("The push endpoint", &body.endpoint, MAX_PUSH_ENDPOINT_CHARS)?;
    super::validate_short_text("The push key", &body.keys.p256dh, MAX_P256DH_CHARS)?;
    super::validate_short_text("The push secret", &body.keys.auth, MAX_AUTH_CHARS)?;
    if !is_base64url(&body.keys.p256dh) || !is_base64url(&body.keys.auth) {
        return Err("The push subscription keys aren't in the expected format.".to_string());
    }
    Ok(())
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
    validate_subscribe_request(&body).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(endpoint: &str, p256dh: &str, auth: &str) -> SubscribeRequest {
        SubscribeRequest {
            endpoint: endpoint.to_string(),
            keys: SubscribeKeys {
                p256dh: p256dh.to_string(),
                auth: auth.to_string(),
            },
        }
    }

    const REAL_P256DH: &str =
        "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM";
    const REAL_AUTH: &str = "tBHItJI5svbpez7KI4CCXg";

    #[test]
    fn a_real_browser_subscription_is_accepted() {
        let body = request(
            "https://fcm.googleapis.com/fcm/send/abc",
            REAL_P256DH,
            REAL_AUTH,
        );
        assert!(validate_subscribe_request(&body).is_ok());
    }

    #[test]
    fn oversized_keys_and_endpoints_are_rejected() {
        let huge = "A".repeat(1_000_000);
        for body in [
            request("https://push.example/x", &huge, REAL_AUTH),
            request("https://push.example/x", REAL_P256DH, &huge),
            request(
                &format!("https://push.example/{huge}"),
                REAL_P256DH,
                REAL_AUTH,
            ),
        ] {
            let err = validate_subscribe_request(&body).unwrap_err();
            assert!(err.contains("too long"), "{err}");
        }
    }

    #[test]
    fn keys_that_are_not_base64url_are_rejected() {
        for (p256dh, auth) in [
            (REAL_P256DH, "not base64!"),
            ("<script>", REAL_AUTH),
            ("", REAL_AUTH),
        ] {
            assert!(
                validate_subscribe_request(&request("https://push.example/x", p256dh, auth))
                    .is_err(),
                "{p256dh:?}/{auth:?} should be rejected"
            );
        }
    }
}
