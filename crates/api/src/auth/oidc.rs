//! OIDC relying-party client: lazy discovery, PKCE authorization-code
//! flow, and ID-token claim mapping. Wraps `openidconnect`/`oauth2`
//! directly -- see
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's
//! crate-landscape research for why no third-party axum-oidc wrapper is
//! used instead.

use anyhow::{Context, Result};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::url::Url;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope,
};

/// The claims this app actually reads out of a verified ID token and
/// persists -- see the design doc's `users` table section for why nothing
/// beyond these four is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct OidcIdentity {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
}

/// The four raw claim values pulled off a verified `openidconnect`
/// `CoreIdTokenClaims`, immediately after signature/issuer/audience/nonce
/// verification (which is `openidconnect`'s job -- see this plan's Global
/// Constraints on why that surface isn't re-tested here). This
/// indirection exists so `identity_from_claims` -- the one piece of this
/// app's *own* logic in the whole OIDC exchange -- is testable against a
/// plain, hand-constructed fixture, without needing a real or fake-signed
/// ID token to build one.
#[derive(Debug, Clone)]
pub struct RawClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}

/// Maps raw claims onto the subset this app persists. A missing/absent
/// `email_verified` claim defaults to `false` (never trust silence as
/// verification) -- see design doc Open Question 2.
pub fn identity_from_claims(claims: RawClaims) -> OidcIdentity {
    OidcIdentity {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
        name: claims.name,
    }
}

#[derive(Clone)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

/// Hand-rolled rather than `#[derive(Debug)]` -- a derived impl would print
/// `client_secret` in plaintext, which is exactly the kind of thing that
/// ends up in a log line the moment anything ever debug-formats an
/// `OidcConfig`/`OidcClient`/`AppState` value. Every other field here is
/// non-sensitive (or already a documented, deliberately-not-secret
/// deployment detail), so only `client_secret` needs redacting.
impl std::fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcConfig")
            .field("issuer_url", &self.issuer_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_url", &self.redirect_url)
            .finish()
    }
}

/// `CoreClient` after `from_provider_metadata` + `set_redirect_uri`, at its
/// concrete typestate: `from_provider_metadata` always sets the
/// authorization endpoint (`EndpointSet`, required by OIDC discovery) but
/// leaves the token/userinfo endpoints merely possibly-set
/// (`EndpointMaybeSet`, since `ProviderMetadata` models them as optional at
/// the type level even though a real provider always returns them), and
/// never sets device-auth/introspection/revocation endpoints at all
/// (`EndpointNotSet` -- this app never uses those flows). `openidconnect`
/// 4.0's `CoreClient` type alias defaults every one of these six params to
/// `EndpointNotSet`, which does NOT match this shape -- naming it bare here
/// (as the plan's draft implicitly did) does not compile, since
/// `authorize_url`/`exchange_code` are only defined for clients whose
/// typestate matches what discovery actually produces.
type DiscoveredClient =
    CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;

/// The OIDC relying-party client. Discovery is deliberately lazy -- see
/// this plan's Global Constraints -- so constructing this value can never
/// fail on a briefly-unreachable issuer; only `IssuerUrl`/`RedirectUrl`
/// syntax is validated eagerly, in `new`.
pub struct OidcClient {
    config: OidcConfig,
    http_client: reqwest::Client,
    inner: tokio::sync::OnceCell<DiscoveredClient>,
}

impl OidcClient {
    pub fn new(config: OidcConfig) -> Result<Self> {
        // Validate URL syntax now (fail fast on a typo'd env var) without
        // making a network call -- the real discovery fetch is deferred
        // to `client()`, below.
        IssuerUrl::new(config.issuer_url.clone()).context("invalid SSO_ISSUER_URL")?;
        RedirectUrl::new(config.redirect_url.clone()).context("invalid SSO_REDIRECT_URL")?;

        // `redirect(Policy::none())`: `openidconnect`/`oauth2`'s own docs
        // (see openidconnect 4.0's crate-level docs) require the caller
        // supply an HTTP client that does NOT auto-follow redirects --
        // "Following redirects opens the client up to SSRF
        // vulnerabilities." An HTTP client that transparently follows
        // redirects could be tricked by a malicious/compromised endpoint
        // into fetching an unintended internal URL during discovery or
        // token exchange.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build OIDC HTTP client")?;

        Ok(Self { config, http_client, inner: tokio::sync::OnceCell::new() })
    }

    /// Performs OIDC discovery on first use only, then caches the result
    /// for the process lifetime. Deliberately NOT done in `new`/at
    /// `AppState::init` time -- see this plan's Global Constraints.
    async fn client(&self) -> Result<&DiscoveredClient> {
        self.inner
            .get_or_try_init(|| async {
                let issuer = IssuerUrl::new(self.config.issuer_url.clone())?;
                let metadata = CoreProviderMetadata::discover_async(issuer, &self.http_client)
                    .await
                    .context("OIDC discovery failed")?;
                let client = CoreClient::from_provider_metadata(
                    metadata,
                    ClientId::new(self.config.client_id.clone()),
                    Some(ClientSecret::new(self.config.client_secret.clone())),
                )
                .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);
                Ok::<_, anyhow::Error>(client)
            })
            .await
    }

    /// Builds the browser-redirect URL for `GET /auth/login`, plus the
    /// three values that must be round-tripped to the callback (stored
    /// server-side -- see `data::users::insert_login_state`, Task 5): the
    /// PKCE verifier, the CSRF state token, and the nonce.
    pub async fn authorize_url(&self) -> Result<(Url, PkceCodeVerifier, CsrfToken, Nonce)> {
        let client = self.client().await?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, csrf_state, nonce) = client
            .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        Ok((url, pkce_verifier, csrf_state, nonce))
    }

    /// Exchanges the authorization code for tokens, verifies the ID
    /// token's signature/issuer/audience/nonce/expiry (`openidconnect`'s
    /// job, not re-implemented here), extracts the four claims this app
    /// cares about into `RawClaims`, and maps them through
    /// `identity_from_claims`. Also returns the refresh token, if the
    /// provider issued one (not guaranteed).
    pub async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: PkceCodeVerifier,
        expected_nonce: &Nonce,
    ) -> Result<(OidcIdentity, Option<String>)> {
        let client = self.client().await?;

        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .context("failed to build code exchange request")?
            .set_pkce_verifier(pkce_verifier)
            .request_async(&self.http_client)
            .await
            .context("token exchange failed")?;

        let id_token = token_response
            .extra_fields()
            .id_token()
            .context("token response had no id_token")?;
        let claims = id_token
            .claims(&client.id_token_verifier(), expected_nonce)
            .context("id token verification failed")?;

        let raw = RawClaims {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().to_string()),
            email_verified: claims.email_verified(),
            name: claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string()),
        };
        let refresh_token = token_response.refresh_token().map(|t| t.secret().clone());

        Ok((identity_from_claims(raw), refresh_token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(email_verified: Option<bool>) -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
        }
    }

    #[test]
    fn sub_and_name_pass_through_unconditionally() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.sub, "user-123");
        assert_eq!(identity.name, Some("Ada Rider".to_string()));
    }

    #[test]
    fn verified_email_is_kept() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.email, Some("rider@example.com".to_string()));
        assert!(identity.email_verified);
    }

    #[test]
    fn unverified_email_claim_still_flows_through_here_unfiltered() {
        // identity_from_claims itself doesn't drop the email on
        // email_verified: false -- that gating happens one layer up, in
        // data::users::upsert_user (Task 5), which is the actual
        // enforcement point per design doc Open Question 2. This function
        // only maps and defaults; asserting that split explicitly here
        // documents where the real decision lives.
        let identity = identity_from_claims(claims(Some(false)));
        assert_eq!(identity.email, Some("rider@example.com".to_string()));
        assert!(!identity.email_verified);
    }

    #[test]
    fn missing_email_verified_claim_defaults_to_unverified() {
        let identity = identity_from_claims(claims(None));
        assert!(!identity.email_verified);
    }
}
