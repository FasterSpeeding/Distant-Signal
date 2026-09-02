//! Verifies an internal-service OAuth2 client-credentials access token
//! (the `Authorization: Bearer` header on a `/private/*` request) against
//! Authentik's JWKS, fetched via standard OIDC discovery and cached
//! in-process. See
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
//! Decision 2.
//!
//! Deliberately reuses `openidconnect::core::CoreJsonWebKeySet`/
//! `CoreJsonWebKey`/`openidconnect::JsonWebKey::verify_signature` --
//! already an `api` dependency for the human-login path
//! (`crate::auth::oidc`) -- rather than adding a new JWT-verification
//! crate. `CoreJsonWebKey::verify_signature` is a generic
//! "verify this signature over this message with this key" primitive,
//! decoupled from `CoreIdTokenVerifier`'s ID-token-specific semantics
//! (nonce, `at_hash`, etc.), which this module has no use for -- claim
//! checks (`exp`/`iss`/`aud`) are done by hand below, which is why this
//! module still counts as "narrow, hand-rolled logic on top of a real
//! cryptography dependency" rather than "hand-rolled cryptography":
//! `verify_signature` does the actual signature math; everything here is
//! base64/JSON plumbing and string comparisons.

use std::collections::HashMap;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreJsonWebKey, CoreJsonWebKeySet, CoreProviderMetadata};
use openidconnect::{IssuerUrl, JsonWebKey as _, JsonWebKeySetUrl};
use serde::Deserialize;

/// The claims this design's route-scoping check reads off a verified
/// client-credentials access token (Decision 3). `groups` defaults to
/// empty when the claim is entirely absent from the token -- never
/// treated as "unscoped/allow everything" (see the route-scoping check in
/// `crate::auth::require_internal_oauth`, Task 5).
///
/// `aud` is modeled as a plain `String`, matching the spec's own stated
/// assumption (Open Question 2: "almost certainly the provider's own
/// `client_id`... not confirmed against a real emitted token"). If a real
/// Authentik-issued token turns out to encode `aud` as a JSON array
/// instead of a bare string, this struct's `aud` field needs to become
/// `Vec<String>` (or an enum accepting either shape) and the audience
/// check in `verify` below needs to check membership instead of equality
/// -- flagged here as the concrete, single place that assumption would
/// need revisiting.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceClaims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: openidconnect::core::CoreJwsSigningAlgorithm,
    kid: Option<String>,
}

/// Every failure mode collapses to one of these three -- `require_internal_oauth`
/// (Task 5) maps all three to a `401`, deliberately not distinguishing
/// which check failed to a caller that isn't yet a proven-valid identity
/// (see the design doc's Error handling section).
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Not three '.'-separated segments, invalid base64, or invalid JSON
    /// in the header/payload.
    Malformed,
    /// No `kid` in the header, or the `kid` still isn't present in the
    /// JWKS after one refetch attempt (including a refetch that itself
    /// failed, e.g. Authentik unreachable).
    UnknownKey,
    /// Signature, `exp`, `iss`, or `aud` failed verification.
    Invalid,
}

/// Fetches (and caches) Authentik's JWKS for the internal-service OAuth2
/// provider, and verifies bearer tokens against it. JWKS endpoint learned
/// via standard OIDC discovery against `issuer_url` -- the same mechanism
/// `crate::auth::oidc::OidcClient` already uses for the human-login flow
/// (Decision 6) -- rather than hardcoding Authentik's own
/// `/application/o/<slug>/jwks/` URL convention. Discovery is lazy (first
/// use, not construction), mirroring `OidcClient`'s own documented
/// posture, so a briefly-unreachable Authentik at `api` startup cannot
/// fail construction or crash-loop the pod.
pub struct ServiceTokenVerifier {
    issuer_url: String,
    expected_audience: String,
    http_client: reqwest::Client,
    jwks_uri: tokio::sync::OnceCell<JsonWebKeySetUrl>,
    keys: tokio::sync::RwLock<HashMap<String, CoreJsonWebKey>>,
}

impl ServiceTokenVerifier {
    pub fn new(issuer_url: String, expected_audience: String) -> Result<Self> {
        IssuerUrl::new(issuer_url.clone()).context("invalid internal_oauth_issuer_url")?;
        // Same redirect-policy rationale as `OidcClient::new` -- an HTTP
        // client that transparently follows redirects during discovery or
        // the JWKS fetch could be tricked into fetching an unintended
        // internal URL.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build internal-oauth JWKS HTTP client")?;
        Ok(Self {
            issuer_url,
            expected_audience,
            http_client,
            jwks_uri: tokio::sync::OnceCell::new(),
            keys: tokio::sync::RwLock::new(HashMap::new()),
        })
    }

    async fn jwks_uri(&self) -> Result<&JsonWebKeySetUrl> {
        self.jwks_uri
            .get_or_try_init(|| async {
                let issuer = IssuerUrl::new(self.issuer_url.clone())?;
                let metadata = CoreProviderMetadata::discover_async(issuer, &self.http_client)
                    .await
                    .context("internal-service OIDC discovery failed")?;
                Ok::<_, anyhow::Error>(metadata.jwks_uri().clone())
            })
            .await
    }

    async fn refresh_keys(&self) -> Result<()> {
        let uri = self.jwks_uri().await?.clone();
        let jwks = CoreJsonWebKeySet::fetch_async(&uri, &self.http_client)
            .await
            .context("failed to fetch internal-oauth JWKS")?;
        let mut map = HashMap::new();
        for key in jwks.keys() {
            if let Some(kid) = key.key_id() {
                map.insert(kid.as_str().to_string(), key.clone());
            }
        }
        *self.keys.write().await = map;
        Ok(())
    }

    async fn key_for_kid(&self, kid: &str) -> Result<CoreJsonWebKey, VerifyError> {
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(key.clone());
        }
        // kid not cached -- refetch exactly once. Guards against an
        // infinite refetch loop on a persistently-unknown kid (e.g. a
        // forged token), and matches Decision 2's stated caching design.
        if self.refresh_keys().await.is_err() {
            return Err(VerifyError::UnknownKey);
        }
        self.keys
            .read()
            .await
            .get(kid)
            .cloned()
            .ok_or(VerifyError::UnknownKey)
    }

    /// Verifies `token`'s signature against the cached (or freshly
    /// fetched) JWKS, then its `exp`/`iss`/`aud`, returning the parsed
    /// claims only if every check passes.
    pub async fn verify(&self, token: &str) -> Result<ServiceClaims, VerifyError> {
        let mut parts = token.split('.');
        let (Some(header_b64), Some(payload_b64), Some(sig_b64), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(VerifyError::Malformed);
        };

        let header_bytes = URL_SAFE_NO_PAD
            .decode(header_b64)
            .map_err(|_| VerifyError::Malformed)?;
        let header: JwtHeader =
            serde_json::from_slice(&header_bytes).map_err(|_| VerifyError::Malformed)?;
        let kid = header.kid.ok_or(VerifyError::Malformed)?;
        let signature = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| VerifyError::Malformed)?;

        let key = self.key_for_kid(&kid).await?;
        let signing_input = format!("{header_b64}.{payload_b64}");
        key.verify_signature(&header.alg, signing_input.as_bytes(), &signature)
            .map_err(|_| VerifyError::Invalid)?;

        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| VerifyError::Malformed)?;
        let claims: ServiceClaims =
            serde_json::from_slice(&payload_bytes).map_err(|_| VerifyError::Malformed)?;

        if claims.iss != self.issuer_url {
            return Err(VerifyError::Invalid);
        }
        if claims.aud != self.expected_audience {
            return Err(VerifyError::Invalid);
        }
        if claims.exp <= chrono::Utc::now().timestamp() {
            return Err(VerifyError::Invalid);
        }
        Ok(claims)
    }
}

/// JWT-signing/mock-Authentik fixtures shared by this module's own
/// verification tests AND `crate::auth`'s route-scoping test suite
/// (`crate::auth::route_scoping_tests`) -- both need "mint a
/// signature-valid internal-service token with these claims against a
/// real (mocked) Authentik," so this is factored out here (the module
/// that owns `ServiceTokenVerifier`) rather than duplicated. `pub(crate)`
/// rather than `pub`: test-only surface, never meant to be reachable
/// outside this crate.
#[cfg(test)]
pub(crate) mod test_support {
    use openidconnect::JsonWebKeyId;
    use openidconnect::PrivateSigningKey;
    use openidconnect::core::{
        CoreJsonWebKeySet, CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey,
    };
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::ServiceTokenVerifier;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    /// Test-only 2048-bit RSA keypair, generated once for this plan
    /// (`openssl genrsa -out priv.pem 2048 && openssl rsa -in priv.pem
    /// -traditional -out priv-pkcs1.pem`). PKCS#1 format -- required by
    /// `CoreRsaPrivateSigningKey::from_pem`, which calls
    /// `rsa::RsaPrivateKey::from_pkcs1_pem` internally, NOT the PKCS#8
    /// format `openssl genrsa`'s default output produces on some OpenSSL
    /// versions -- confirmed directly against the vendored
    /// `openidconnect-4.0.1` source this plan was written against. Never
    /// Authentik's real key.
    pub(crate) const TEST_RSA_PRIVATE_KEY_PEM: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAkXH3+3kYxgn6JADaRM0Wv8i3bo0vdLkuwI0LR00aDYiN6XGC
iSPnr1AfSPlNSQxNQ2q+xyRiLnAwkJ9bcnuHNR818Ok984u3k2nHI+fg5MVrAkcH
Ytk+OIrnVsAxFZjG+vBN9gVbrkiQoBVAZKJ7D6DGAXZnexri7FGR5ttnASvwtzBt
ZfzYHca87Uqk2jLd13a2GgTlrXA3HAwQ/ZDHu8JOPBx4zP3OK64LnG7ajK3NC+s6
PlhOFpJrJI5t1wyFz6gjtHNnijtbi4XDxKUL+Vl5zFWj/QvZkgC2kcBvTv3uYVhu
xyKsx/7MifbbDpHWgEHtfzsIi33gT/WwvjPnuQIDAQABAoIBAA8MD5VB2n5xPXfo
agHF3ALRulR4ISmTbP2juep8VLkuCvcp9KYeg6jUrQqTgYY7UmpVSqZ3TPxemU+l
BO92TdnWQEeLwd/b8Q08W3YLVm4klHpjAdBysZK6Ss5j9NALWG6mr3zH4iEeRcQi
CWGVQ7kCr5Qq0prfAGIQMFGGGlp5sDAkUTuDNmaZuvmLOWy9jpzgwwYNeYbvk+jH
+PbxNuPedRWAii43FY9c3/dT4qx1esCVYRQ4FvhNljE1+a/7rZXDpoPVDcizhNp2
oStI2b/SgSYlyrBmoCPnyLeYIWPxvgxaftU3ArRVtC6RYBlaI7ena3+z6fjl0EXN
7D84xBcCgYEAw388MOeEYTsfOBH9BDNz03RWPsplWAvE8l35Rxbq6n9a46+dWOaj
eiI9Hh/cAgbCmUp0PFucoXP/rPq+VcN1c9bZQc9YShW8UILK09RjpapXUIi4jOcc
GrVAFdKgMZD9CIG3s//CNlgiPmohUyh0Umurd1ScNB8Y/PodWNec/csCgYEAvnU/
EC4H7RQmKpM+D2g2+buCL3QaYXJHoHqX2sZ9q/WtjUyuqQlwKq0GzhPcE2ewN6x9
9UX4s6MC9rGH8aQq3j2OGXLxyRIaKP6+fNubm0ge6hP4/Bg4tbLgVUF0+AiH+xiD
gTqa5RpR31YJ5mpYM1gv0bd7skd2i7CudmC/AAsCgYB6w/TFdS2hbWIecNVlhPYQ
bLcYOTtI/iMQXEkFBnRBC/bEkmyJ/lPch5G/0Bv1vc8IOkQh/xmuHc0KEG/kJZkl
RF8sP4vfAiU+ndPHEFH/H6gzL5hNC3iPoRB8Y8crOTRc2jDFPS/1toTSkw0YTog1
ld2YUy7AYGLtwhcZylSQ3wKBgGH3EP8TjkQmLxOLNUrbghumlWovQDqLe8hSBrYj
jxTag/DAVr7f+fAZm/x4PqVEmmGougllem18FdQqsRBcLyitZOA2PaP9SbN4hSbY
FwwiZrRknZeeJd1gKv/vcWj7imZfz5SzPmVFyoMkUGdSoBeY7s/inx+uno1vze1a
CiTNAoGAD+qOUaY30EqKQryTRCTCABq0tclEm+UAD7aTFqzTqGjK8V7IxFAJ8rnw
okZbiUUfaTzSWjonh81igWBCbs9l7+FaaiMCy3Hy5rA7g2eTdJoU7gxlabEnzdUj
9O7hQg5LztVsx4CpVlyjw8gYB14pwoxrbJc4mDUwT7MPH29EgDE=
-----END RSA PRIVATE KEY-----";

    pub(crate) const KID: &str = "test-kid";
    pub(crate) const AUDIENCE: &str = "distant-signal-internal";

    pub(crate) fn signing_key() -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(
            TEST_RSA_PRIVATE_KEY_PEM,
            Some(JsonWebKeyId::new(KID.to_string())),
        )
        .expect("test RSA key should parse")
    }

    pub(crate) fn sign_token(claims: &serde_json::Value) -> String {
        let key = signing_key();
        let header = json!({"alg": "RS256", "kid": KID, "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let signing_input = format!("{header_b64}.{payload_b64}");
        let signature = key
            .sign(
                &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                signing_input.as_bytes(),
            )
            .expect("signing with the test key should succeed");
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature);
        format!("{signing_input}.{sig_b64}")
    }

    /// Mints a valid claims JSON with `exp` far in the future, overridable
    /// per test via the closure.
    pub(crate) fn valid_claims(
        issuer: &str,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> serde_json::Value {
        let mut claims = json!({
            "sub": "svc-poller-incidents",
            "iss": issuer,
            "aud": AUDIENCE,
            "exp": (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
            "groups": ["svc-poller-incidents"],
        });
        mutate(&mut claims);
        claims
    }

    /// Stands up a mock Authentik: `.well-known/openid-configuration` plus
    /// a JWKS endpoint serving the test key's public half. Returns the
    /// server (whose `uri()` is also the `issuer_url`) and a
    /// `ServiceTokenVerifier` pointed at it.
    pub(crate) async fn mock_authentik() -> (MockServer, ServiceTokenVerifier) {
        let server = MockServer::start().await;
        let issuer = server.uri();

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/authorize"),
                "jwks_uri": format!("{issuer}/jwks"),
                "response_types_supported": ["token"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
            })))
            .mount(&server)
            .await;

        let public_jwk = signing_key().as_verification_key();
        let jwks = CoreJsonWebKeySet::new(vec![public_jwk]);
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&server)
            .await;

        let verifier = ServiceTokenVerifier::new(issuer.clone(), AUDIENCE.to_string()).unwrap();
        (server, verifier)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::test_support::{mock_authentik, sign_token, valid_claims};
    use super::*;

    #[tokio::test]
    async fn a_valid_token_with_expected_claims_verifies() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));

        let claims = verifier
            .verify(&token)
            .await
            .expect("valid token should verify");
        assert_eq!(claims.sub, "svc-poller-incidents");
        assert_eq!(claims.groups, vec!["svc-poller-incidents".to_string()]);
    }

    #[tokio::test]
    async fn an_expired_token_is_rejected() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c["exp"] = json!((chrono::Utc::now() - chrono::Duration::hours(1)).timestamp());
        }));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_a_corrupted_signature_is_rejected() {
        // Exercises the same "signature does not verify against the
        // serving key" path a genuinely-wrong-key token would hit,
        // without embedding a second RSA keypair fixture: a token signed
        // with the real test key, whose signature segment is then
        // replaced with unrelated bytes, must fail `verify_signature`
        // exactly like a token signed by a key the JWKS never served.
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));
        let mut parts: Vec<&str> = token.split('.').collect();
        let corrupted_sig = URL_SAFE_NO_PAD.encode(b"not-a-real-signature-at-all-000000");
        parts[2] = &corrupted_sig;
        let corrupted = parts.join(".");

        assert_eq!(verifier.verify(&corrupted).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_the_wrong_issuer_is_rejected() {
        let (_server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(
            "https://not-the-configured-issuer.invalid",
            |_| {},
        ));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_the_wrong_audience_is_rejected() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c["aud"] = json!("some-other-client-id");
        }));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_missing_groups_claim_defaults_to_empty_not_unscoped() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c.as_object_mut().unwrap().remove("groups");
        }));

        let claims = verifier
            .verify(&token)
            .await
            .expect("otherwise-valid token should still verify");
        assert!(
            claims.groups.is_empty(),
            "an absent groups claim must default to empty, never 'allow everything'"
        );
    }

    #[tokio::test]
    async fn an_unknown_kid_after_one_refetch_is_rejected() {
        // Sign with the real key/kid, then swap the header's `kid` to
        // something the JWKS never advertises. `key_for_kid` refetches
        // once (the mocked JWKS still won't have it), then rejects with
        // `UnknownKey` -- short-circuiting before `verify_signature` is
        // ever called, so the now-mismatched signature bytes carried over
        // from the original token are never actually checked.
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));
        let header = json!({"alg": "RS256", "kid": "kid-nobody-has", "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let mut parts: Vec<&str> = token.split('.').collect();
        parts[0] = &header_b64;
        let retagged = parts.join(".");

        assert_eq!(
            verifier.verify(&retagged).await,
            Err(VerifyError::UnknownKey)
        );
    }

    #[tokio::test]
    async fn a_malformed_token_is_rejected() {
        let (_server, verifier) = mock_authentik().await;
        assert_eq!(
            verifier.verify("not-a-jwt-at-all").await,
            Err(VerifyError::Malformed)
        );
        assert_eq!(
            verifier.verify("only.two-parts").await,
            Err(VerifyError::Malformed)
        );
    }
}
