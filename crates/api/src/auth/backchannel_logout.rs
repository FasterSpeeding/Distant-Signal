//! OpenID Connect Back-Channel Logout 1.0 (M14, 2026-09-27): validation of the
//! signed logout token the IdP (Authentik) POSTs to
//! `POST /public/auth/backchannel-logout` (`routes::auth::backchannel_logout`)
//! when a user logs out there, is deactivated, or has an Authentik session
//! deleted by an admin.
//!
//! The signature is checked against the human-login provider's JWKS
//! (discovered from `SSO_ISSUER_URL`, the same issuer `auth::oidc` logs users
//! in against) by a second [`ServiceTokenVerifier`], so it shares that type's
//! kid cache, negative cache and refetch cooldown. The claim checks are the
//! ones spec §2.6 lists for a logout token:
//!
//! - `typ` header, when present, is `logout+jwt` (Authentik 2026.8 always sets
//!   it: `providers/oauth2/utils.py::create_logout_token`). A plain `JWT` is
//!   refused so an ID token can't be replayed here;
//! - `iss` equals `SSO_ISSUER_URL` exactly; `aud` contains `SSO_CLIENT_ID`;
//! - `iat` is present, not in the future (60s leeway) and no older than
//!   [`MAX_LOGOUT_TOKEN_AGE`]; `exp`, when present, has not passed;
//! - `events` is an object with the
//!   `http://schemas.openid.net/event/backchannel-logout` member, itself an
//!   object;
//! - `sub` and/or `sid` is present;
//! - no `nonce` claim at all (an ID token always carries one in this app's
//!   flow; a logout token must not).
//!
//! `jti` is not required and not remembered: replaying a captured logout token
//! within its lifetime can only end the same user's sessions again.

use std::time::Duration;

use serde::Deserialize;

use super::internal_oauth::{Audience, CLOCK_SKEW_LEEWAY, ServiceTokenVerifier, VerifyError};

/// The one event member a logout token must carry (spec §2.4).
pub const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// Oldest `iat` accepted. Authentik sends the request from a background
/// worker task that retries on failure, so this is generous; a token older
/// than this is refused even if its `exp` (Authentik sets it to `iat +` the
/// provider's access-token validity) has not passed.
pub const MAX_LOGOUT_TOKEN_AGE: Duration = Duration::from_secs(60 * 60);

/// Why a logout token was refused. The route answers `400` for all of them
/// and logs the variant; the IdP learns nothing more.
#[derive(Debug, PartialEq, Eq)]
pub enum LogoutTokenError {
    /// Not a JWT, bad base64/JSON, no `kid`, or a claim of the wrong type.
    Malformed,
    /// The `kid` is not in the JWKS (after one refetch), or the JWKS could
    /// not be fetched.
    UnknownKey,
    /// The signature does not verify.
    BadSignature,
    /// Header `typ` is present and not `logout+jwt`.
    WrongType,
    WrongIssuer,
    WrongAudience,
    /// `iat` missing.
    MissingIssuedAt,
    /// `iat` in the future beyond the leeway.
    IssuedInFuture,
    /// `exp` passed, or `iat` older than [`MAX_LOGOUT_TOKEN_AGE`].
    Expired,
    /// `events` missing or without the back-channel logout member.
    MissingLogoutEvent,
    /// A `nonce` claim is present (forbidden by spec §2.4).
    HasNonce,
    /// Neither `sub` nor `sid`.
    MissingSubjectAndSession,
}

/// What a verified logout token names. At least one field is `Some`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoutSubject {
    pub sub: Option<String>,
    pub sid: Option<String>,
}

#[derive(Deserialize)]
struct LogoutTokenClaims {
    #[serde(default)]
    iss: Option<String>,
    #[serde(default)]
    aud: Option<Audience>,
    #[serde(default)]
    iat: Option<i64>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    events: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    sid: Option<String>,
}

/// Verifies logout tokens for one OIDC client. Built by `OidcClient::new`
/// from the same issuer and client id the login flow uses.
pub struct LogoutTokenVerifier {
    issuer_url: String,
    client_id: String,
    jwks: ServiceTokenVerifier,
}

impl LogoutTokenVerifier {
    pub fn new(issuer_url: String, client_id: String) -> anyhow::Result<Self> {
        // The audience argument is unused by `verify_signed_jwt`; the
        // audience check is done below against `client_id`.
        let jwks = ServiceTokenVerifier::new(issuer_url.clone(), client_id.clone())?;
        Ok(Self {
            issuer_url,
            client_id,
            jwks,
        })
    }

    pub async fn verify(&self, token: &str) -> Result<LogoutSubject, LogoutTokenError> {
        let signed = self
            .jwks
            .verify_signed_jwt(token)
            .await
            .map_err(|err| match err {
                VerifyError::Malformed => LogoutTokenError::Malformed,
                VerifyError::UnknownKey => LogoutTokenError::UnknownKey,
                VerifyError::Invalid => LogoutTokenError::BadSignature,
            })?;
        check_claims(
            signed.typ.as_deref(),
            &signed.payload,
            &self.issuer_url,
            &self.client_id,
            chrono::Utc::now().timestamp(),
        )
    }
}

/// Is `typ` acceptable? Absent is allowed (the spec only RECOMMENDS explicit
/// typing); otherwise it must be `logout+jwt`, optionally with the
/// `application/` prefix RFC 7515 §4.1.9 allows omitting, case-insensitive.
fn typ_is_acceptable(typ: Option<&str>) -> bool {
    match typ {
        None => true,
        Some(typ) => {
            let typ = typ.to_ascii_lowercase();
            typ == "logout+jwt" || typ == "application/logout+jwt"
        }
    }
}

/// The claim half of [`LogoutTokenVerifier::verify`], split out so it's
/// testable without a JWKS. `now` is Unix seconds.
fn check_claims(
    typ: Option<&str>,
    payload: &[u8],
    issuer_url: &str,
    client_id: &str,
    now: i64,
) -> Result<LogoutSubject, LogoutTokenError> {
    if !typ_is_acceptable(typ) {
        return Err(LogoutTokenError::WrongType);
    }
    // Parsed as a map first: a `nonce` key must be refused even when its
    // value is `null`, which a typed `Option` field can't tell from absent.
    let raw: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(payload).map_err(|_| LogoutTokenError::Malformed)?;
    if raw.contains_key("nonce") {
        return Err(LogoutTokenError::HasNonce);
    }
    let claims: LogoutTokenClaims = serde_json::from_value(serde_json::Value::Object(raw))
        .map_err(|_| LogoutTokenError::Malformed)?;

    if claims.iss.as_deref() != Some(issuer_url) {
        return Err(LogoutTokenError::WrongIssuer);
    }
    if !claims.aud.is_some_and(|aud| aud.contains(client_id)) {
        return Err(LogoutTokenError::WrongAudience);
    }

    let leeway = CLOCK_SKEW_LEEWAY.as_secs() as i64;
    let iat = claims.iat.ok_or(LogoutTokenError::MissingIssuedAt)?;
    if iat.saturating_sub(leeway) > now {
        return Err(LogoutTokenError::IssuedInFuture);
    }
    if iat.saturating_add(MAX_LOGOUT_TOKEN_AGE.as_secs() as i64 + leeway) < now {
        return Err(LogoutTokenError::Expired);
    }
    if let Some(exp) = claims.exp
        && exp.saturating_add(leeway) <= now
    {
        return Err(LogoutTokenError::Expired);
    }

    let has_event = claims
        .events
        .as_ref()
        .and_then(|events| events.get(BACKCHANNEL_LOGOUT_EVENT))
        .is_some_and(serde_json::Value::is_object);
    if !has_event {
        return Err(LogoutTokenError::MissingLogoutEvent);
    }

    let sub = claims.sub.filter(|s| !s.is_empty());
    let sid = claims.sid.filter(|s| !s.is_empty());
    if sub.is_none() && sid.is_none() {
        return Err(LogoutTokenError::MissingSubjectAndSession);
    }
    Ok(LogoutSubject { sub, sid })
}

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::json;

    use super::BACKCHANNEL_LOGOUT_EVENT;

    pub(crate) const CLIENT_ID: &str = "test-client";

    /// A valid logout-token claim set for `sub`, overridable per test.
    pub(crate) fn logout_claims(
        issuer: &str,
        sub: &str,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> serde_json::Value {
        let now = chrono::Utc::now();
        let mut claims = json!({
            "iss": issuer,
            "aud": CLIENT_ID,
            "iat": now.timestamp(),
            "exp": (now + chrono::Duration::hours(1)).timestamp(),
            "jti": "3f1d7a0e-test",
            "events": { BACKCHANNEL_LOGOUT_EVENT: {} },
            "sub": sub,
            "sid": "hashed-authentik-session-key",
        });
        mutate(&mut claims);
        claims
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::test_support::{CLIENT_ID, logout_claims};
    use super::*;
    use crate::auth::internal_oauth::test_support::{
        mock_authentik, sign_token, sign_token_with_typ,
    };

    const ISSUER: &str = "https://sso.example.invalid/application/o/distant-signal/";

    fn check(claims: &serde_json::Value) -> Result<LogoutSubject, LogoutTokenError> {
        check_claims(
            Some("logout+jwt"),
            &serde_json::to_vec(claims).unwrap(),
            ISSUER,
            CLIENT_ID,
            chrono::Utc::now().timestamp(),
        )
    }

    #[test]
    fn a_valid_claim_set_yields_sub_and_sid() {
        assert_eq!(
            check(&logout_claims(ISSUER, "user-1", |_| {})),
            Ok(LogoutSubject {
                sub: Some("user-1".to_string()),
                sid: Some("hashed-authentik-session-key".to_string()),
            })
        );
    }

    #[test]
    fn an_audience_array_containing_the_client_id_is_accepted() {
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c["aud"] = json!(["other", CLIENT_ID]);
        });
        assert!(check(&claims).is_ok());
    }

    #[test]
    fn a_nonce_is_rejected_even_when_null() {
        for nonce in [json!("n-0S6_WzA2Mj"), serde_json::Value::Null] {
            let claims = logout_claims(ISSUER, "user-1", |c| c["nonce"] = nonce);
            assert_eq!(check(&claims), Err(LogoutTokenError::HasNonce));
        }
    }

    #[test]
    fn wrong_issuer_and_audience_are_rejected() {
        let claims = logout_claims("https://evil.invalid/", "user-1", |_| {});
        assert_eq!(check(&claims), Err(LogoutTokenError::WrongIssuer));
        let claims = logout_claims(ISSUER, "user-1", |c| c["aud"] = json!("other-client"));
        assert_eq!(check(&claims), Err(LogoutTokenError::WrongAudience));
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c.as_object_mut().unwrap().remove("aud");
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::WrongAudience));
    }

    #[test]
    fn time_checks() {
        let now = chrono::Utc::now().timestamp();
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c.as_object_mut().unwrap().remove("iat");
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::MissingIssuedAt));

        let claims = logout_claims(ISSUER, "user-1", |c| c["iat"] = json!(now + 600));
        assert_eq!(check(&claims), Err(LogoutTokenError::IssuedInFuture));

        let claims = logout_claims(ISSUER, "user-1", |c| {
            c["iat"] = json!(now - 7200);
            c["exp"] = json!(now - 3600);
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::Expired));

        // `exp` far in the future does not rescue a stale `iat`.
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c["iat"] = json!(now - 3 * 3600);
            c["exp"] = json!(now + 14 * 86400);
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::Expired));

        // `exp` is optional.
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c.as_object_mut().unwrap().remove("exp");
        });
        assert!(check(&claims).is_ok());
    }

    #[test]
    fn the_logout_event_is_required() {
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c.as_object_mut().unwrap().remove("events");
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::MissingLogoutEvent));
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c["events"] = json!({"http://schemas.openid.net/event/other": {}});
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::MissingLogoutEvent));
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c["events"] = json!({ BACKCHANNEL_LOGOUT_EVENT: "yes" });
        });
        assert_eq!(check(&claims), Err(LogoutTokenError::MissingLogoutEvent));
    }

    #[test]
    fn sub_or_sid_is_required() {
        let claims = logout_claims(ISSUER, "user-1", |c| {
            let obj = c.as_object_mut().unwrap();
            obj.remove("sub");
            obj.remove("sid");
        });
        assert_eq!(
            check(&claims),
            Err(LogoutTokenError::MissingSubjectAndSession)
        );
        let claims = logout_claims(ISSUER, "user-1", |c| {
            c.as_object_mut().unwrap().remove("sub");
        });
        assert_eq!(
            check(&claims),
            Ok(LogoutSubject {
                sub: None,
                sid: Some("hashed-authentik-session-key".to_string()),
            })
        );
    }

    #[test]
    fn typ_must_be_logout_jwt_when_present() {
        let payload = serde_json::to_vec(&logout_claims(ISSUER, "user-1", |_| {})).unwrap();
        let now = chrono::Utc::now().timestamp();
        for ok in [None, Some("logout+jwt"), Some("application/logout+JWT")] {
            assert!(
                check_claims(ok, &payload, ISSUER, CLIENT_ID, now).is_ok(),
                "{ok:?}"
            );
        }
        for bad in [Some("JWT"), Some("at+jwt")] {
            assert_eq!(
                check_claims(bad, &payload, ISSUER, CLIENT_ID, now),
                Err(LogoutTokenError::WrongType),
                "{bad:?}"
            );
        }
    }

    /// End to end through a mocked Authentik JWKS: a real signature
    /// verifies; a corrupted one does not.
    #[tokio::test]
    async fn signature_is_verified_against_the_jwks() {
        let (server, _) = mock_authentik().await;
        let issuer = server.uri();
        let verifier = LogoutTokenVerifier::new(issuer.clone(), CLIENT_ID.to_string()).unwrap();

        let token = sign_token_with_typ(&logout_claims(&issuer, "user-1", |_| {}), "logout+jwt");
        assert_eq!(
            verifier.verify(&token).await.map(|s| s.sub),
            Ok(Some("user-1".to_string()))
        );

        let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
        parts[2] = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            b"not-a-real-signature",
        );
        assert_eq!(
            verifier.verify(&parts.join(".")).await,
            Err(LogoutTokenError::BadSignature)
        );

        // An ID-token-shaped JWT (typ JWT) is refused even if well signed.
        let token = sign_token(&logout_claims(&issuer, "user-1", |_| {}));
        assert_eq!(
            verifier.verify(&token).await,
            Err(LogoutTokenError::WrongType)
        );
    }
}
