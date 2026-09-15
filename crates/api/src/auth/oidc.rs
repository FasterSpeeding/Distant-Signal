//! OIDC relying-party client: lazy discovery, PKCE authorization-code
//! flow, and ID-token claim mapping. Wraps `openidconnect`/`oauth2`
//! directly -- see
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's
//! crate-landscape research for why no third-party axum-oidc wrapper is
//! used instead.

use anyhow::{Context, Result};
use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreErrorResponseType,
    CoreGenderClaim, CoreJsonWebKey, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, CoreRevocableToken, CoreRevocationErrorResponse,
    CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::url::Url;
use openidconnect::{
    AdditionalClaims, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken,
    EmptyExtraTokenFields, EndpointMaybeSet, EndpointNotSet, EndpointSet, IdTokenClaims,
    IdTokenFields, IssuerUrl, LocalizedClaim, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, StandardErrorResponse, StandardTokenResponse,
};

/// The claims this app actually reads out of a verified ID token and
/// persists -- see the design doc's `users` table section for why nothing
/// beyond these is stored.
///
/// `name` and `preferred_username` are not raw single-claim copies: each
/// is the best of SEVERAL standard `profile`-scope claims, resolved by
/// `identity_from_claims`. See `RawClaims` for which claims feed which,
/// and why.
#[derive(Debug, Clone, PartialEq)]
pub struct OidcIdentity {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    /// The user's real name, if the IdP asserted one in any of the
    /// standard name-shaped `profile` claims (`name`, else
    /// `given_name`/`family_name`).
    pub name: Option<String>,
    /// The user's handle: the `preferred_username` claim, else `nickname`.
    /// The ONLY non-email identifier besides `name` that reaches us, and
    /// the reason a user whose IdP has no name on file for them still has
    /// something to be shown as in a shared group instead of their email
    /// address (which must never be shown to other members -- see
    /// `data::users::display_label`).
    pub preferred_username: Option<String>,
    pub groups: Vec<String>,
}

/// The raw claim values pulled off a verified `openidconnect`
/// `IdTokenClaims`, immediately after signature/issuer/audience/nonce
/// verification (which is `openidconnect`'s job -- see this plan's Global
/// Constraints on why that surface isn't re-tested here). This
/// indirection exists so `identity_from_claims` -- the one piece of this
/// app's *own* logic in the whole OIDC exchange -- is testable against a
/// plain, hand-constructed fixture, without needing a real or fake-signed
/// ID token to build one. `raw_claims_from_id_token` is the (also tested)
/// step that fills one in from a real parsed ID token.
///
/// # Why five name-ish claims and not one
///
/// Verified against Authentik's own shipped `profile` scope mapping
/// (goauthentik/authentik `blueprints/system/providers-oauth2.yaml`,
/// version-2026.8 -- the mapping this deployment attaches, see
/// `authentik-blueprints/oauth2-client.yaml`), which evaluates to:
///
/// ```text
/// name:               request.user.name
/// given_name:         user.attributes["given_name"]   else request.user.name
/// family_name:        user.attributes["family_name"]  else omitted
/// preferred_username: request.user.username
/// nickname:           request.user.username
/// ```
///
/// Two independent things can therefore carry a person's real name --
/// `User.name`, and the `given_name`/`family_name` pair in `User.attributes`
/// -- and an account provisioned into Authentik by an admin, an import, or
/// a directory sync routinely has the latter filled in while `User.name` is
/// still the empty string it defaults to. Reading only `name` showed those
/// users as their bare login handle (or, before `users.username` existed,
/// as nothing at all) even though their real name was sitting right there
/// in the same token.
#[derive(Debug, Clone)]
pub struct RawClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub preferred_username: Option<String>,
    pub nickname: Option<String>,
    pub groups: Option<Vec<String>>,
}

/// Blank-or-absent are the same thing for every claim this app reads: an
/// IdP with nothing on file for a field often sends `""` rather than
/// omitting the claim (Authentik's `profile` mapping returns `User.name`
/// verbatim, and that column defaults to the empty string -- an unset
/// `User.attributes` key is the other way round, dropped from the token
/// entirely by `delete_none_values`, which is why both shapes have to be
/// handled). Trimmed, because `"  Ada  "` is a name with padding, not a
/// name.
///
/// `data::users` keeps its own copy of this, applied when writing a row and
/// again when reading one back; see its doc comment for why the duplication
/// is deliberate here but not for `looks_like_email_address`.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// THE definition of "this claim value looks like an email address", used
/// both here (to rank an email-shaped claim below a non-email alternative
/// when picking a name/username) and by `data::users::shareable` (to refuse
/// to render one to other members at all). Deliberately one function and
/// not two copies of `contains('@')`: the two sites have to agree, or the
/// boundary could promote a value the display layer then silently drops.
///
/// `@` is a deliberately blunt test -- see `data::users::shareable`'s own
/// doc comment for the privacy reasoning and for what it costs on IdPs
/// whose `preferred_username` is a UPN.
pub fn looks_like_email_address(value: &str) -> bool {
    value.contains('@')
}

/// Picks the best of an ordered list of candidate claim values: the first
/// that is non-blank AND not email-shaped, else -- if every candidate looks
/// like an email address -- the first merely-non-blank one.
///
/// The second arm matters. Dropping an email-shaped value outright here
/// would leave `users.name`/`users.username` NULL where they used to hold
/// something, and `data::users::display_label` is already the single
/// enforcement point for "never show one member's email to the rest of the
/// group". So this function only ever REORDERS, never empties: an
/// email-shaped `name` stops shadowing a perfectly good
/// `given_name`/`family_name` (so what gets stored may now be a different,
/// better value), but a field that had something to store before still has
/// something to store -- this never turns a `Some` into a `None`.
fn best_candidate(candidates: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let usable: Vec<String> = candidates
        .into_iter()
        .filter_map(|candidate| non_blank(candidate.as_deref()).map(str::to_string))
        .collect();
    usable
        .iter()
        .find(|candidate| !looks_like_email_address(candidate))
        .or_else(|| usable.first())
        .cloned()
}

/// `given_name` + `family_name` as one name, tolerating either half being
/// blank or absent. Both halves really do go missing independently under
/// Authentik's mapping: `family_name` is omitted entirely unless
/// `User.attributes["family_name"]` is set, while `given_name` is always
/// present, holding `User.attributes["given_name"]` if that is set and
/// otherwise falling back to `User.name` (so it is blank when BOTH are --
/// an attribute-provisioned account with an empty `User.name` is exactly
/// the case where `given_name` is populated and `name` is not, which is
/// the whole reason this function exists).
fn joined_name(given_name: Option<&str>, family_name: Option<&str>) -> Option<String> {
    match (non_blank(given_name), non_blank(family_name)) {
        (Some(given), Some(family)) => Some(format!("{given} {family}")),
        (Some(only), None) | (None, Some(only)) => Some(only.to_string()),
        (None, None) => None,
    }
}

/// Maps raw claims onto the subset this app persists. A missing/absent
/// `email_verified` claim defaults to `false` (never trust silence as
/// verification) -- see design doc Open Question 2. A missing/absent
/// `groups` claim defaults to an empty vec -- see
/// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
/// Decision 2.
///
/// `name` and `preferred_username` are each resolved from several claims
/// (see `RawClaims`), so that `data::users::display_label`'s
/// name-else-username-else-placeholder chain is fed the best thing the
/// token actually carried rather than whichever single claim was checked
/// first:
///
/// - `name`: the `name` claim, else `given_name`/`family_name` joined.
/// - `preferred_username`: the `preferred_username` claim, else `nickname`.
///
/// `nickname` ranks as a USERNAME, not a name, on purpose. OIDC Core calls
/// it a "casual name", but Authentik -- this app's IdP -- emits
/// `request.user.username` for it verbatim, identical to
/// `preferred_username`. Treating it as a name would write a login handle
/// into `users.name`, where the rest of the app reads "a real name"; as a
/// username fallback it is free insurance for an IdP (or a custom Authentik
/// scope mapping) that emits one and not the other, and a no-op otherwise.
pub fn identity_from_claims(claims: RawClaims) -> OidcIdentity {
    let joined = joined_name(claims.given_name.as_deref(), claims.family_name.as_deref());
    OidcIdentity {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
        name: best_candidate([claims.name, joined]),
        preferred_username: best_candidate([claims.preferred_username, claims.nickname]),
        groups: claims.groups.unwrap_or_default(),
    }
}

/// Reads the claims this app cares about off an already-verified ID token.
///
/// Split out of `exchange_code` so the claim-NAME-to-field wiring is
/// testable against a realistic provider payload (see this module's tests,
/// which deserialize real Authentik-shaped ID-token JSON through it).
/// Getting a claim name wrong here is invisible to `identity_from_claims`'s
/// own tests -- they start from a hand-built `RawClaims` and so assume this
/// step was right.
fn raw_claims_from_id_token(
    claims: &IdTokenClaims<AccessGroupClaims, CoreGenderClaim>,
) -> RawClaims {
    RawClaims {
        sub: claims.subject().as_str().to_string(),
        email: claims.email().map(|e| e.as_str().to_string()),
        email_verified: claims.email_verified(),
        name: localized(claims.name()),
        given_name: localized(claims.given_name()),
        family_name: localized(claims.family_name()),
        // Not a `LocalizedClaim`, unlike the name-shaped claims above --
        // `preferred_username` is a bare string in OIDC Core.
        preferred_username: claims.preferred_username().map(|u| u.as_str().to_string()),
        nickname: localized(claims.nickname()),
        groups: claims.additional_claims().groups.clone(),
    }
}

/// The untagged (no language tag) entry of a localizable claim. This app
/// has no locale to negotiate against, and every provider it targets sends
/// the plain, untagged form; a token that carried ONLY `name#de` would read
/// here as no name at all, which is the correct conservative outcome.
///
/// Bounded on `Deref<Target = String>`, which is what `openidconnect`'s
/// `new_type!` macro actually generates for these claim newtypes
/// (`EndUserName`, `EndUserGivenName`, ...) -- they implement neither
/// `AsRef<str>` nor `Deref<Target = str>`.
fn localized<T: std::ops::Deref<Target = String>>(
    claim: Option<&LocalizedClaim<T>>,
) -> Option<String> {
    claim
        .and_then(|localized| localized.get(None))
        .map(|value| (**value).clone())
}

/// The `groups` claim this app additionally requests and reads off the ID
/// token, beyond what `openidconnect::core`'s fixed `CoreClient` alias can
/// see. Confirmed directly against the pinned `openidconnect` 4.0.1
/// source this session: `core::CoreClient` hardcodes its
/// `AdditionalClaims` type parameter to `EmptyAdditionalClaims`, which
/// silently discards any claim beyond the standard set `openidconnect::
/// core` models -- reading `groups` requires this real `AdditionalClaims`
/// impl and a `Client` built on it, not just a new field read off an
/// already-parsed struct. See
/// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
/// Decision 2.
///
/// `#[serde(default)]` on `groups`: a missing claim deserializes to
/// `None`, never a deserialization error -- the same "never trust silence
/// as something stronger than it is" posture `email_verified`'s own
/// handling already takes below.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AccessGroupClaims {
    #[serde(default)]
    pub groups: Option<Vec<String>>,
}

impl AdditionalClaims for AccessGroupClaims {}

/// Mirrors `openidconnect::core`'s own `CoreIdTokenFields`/
/// `CoreTokenResponse` type aliases (`core/mod.rs`), but with
/// `AccessGroupClaims` in place of `EmptyAdditionalClaims` as the
/// `AdditionalClaims` type parameter. Both must be redefined together --
/// see this file's own module-level note on why swapping only
/// `DiscoveredClient`'s `AC` parameter and leaving `TR` at
/// `CoreTokenResponse` would silently keep discarding the claim.
type GroupsIdTokenFields = IdTokenFields<
    AccessGroupClaims,
    EmptyExtraTokenFields,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;
type GroupsTokenResponse = StandardTokenResponse<GroupsIdTokenFields, CoreTokenType>;

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

/// `CoreClient` after `from_provider_metadata` + `set_redirect_uri`, at
/// its concrete typestate -- see the original comment on this type (now
/// below) for why the six endpoint-typestate parameters are what they
/// are; unchanged by this task. Built on the fully generic
/// `openidconnect::Client<...>` rather than the `core` module's
/// `CoreClient` alias, since `CoreClient` fixes its `AdditionalClaims`
/// parameter to `EmptyAdditionalClaims` (see `AccessGroupClaims`'s own
/// doc comment, above). Every parameter here besides `AC`/`TR` is copied
/// verbatim from `core::CoreClient`'s own definition
/// (`openidconnect::core::mod`, confirmed against the pinned 4.0.1
/// source this session).
///
/// `from_provider_metadata` always sets the authorization endpoint
/// (`EndpointSet`, required by OIDC discovery) but leaves the
/// token/userinfo endpoints merely possibly-set (`EndpointMaybeSet`,
/// since `ProviderMetadata` models them as optional at the type level
/// even though a real provider always returns them), and never sets
/// device-auth/introspection/revocation endpoints at all
/// (`EndpointNotSet` -- this app never uses those flows).
type DiscoveredClient = Client<
    AccessGroupClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    GroupsTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

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

        Ok(Self {
            config,
            http_client,
            inner: tokio::sync::OnceCell::new(),
        })
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
                let client = Client::from_provider_metadata(
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
    ///
    /// # What this app can and cannot fix about missing display names
    ///
    /// `profile` below is the only thing that makes `name`/`given_name`/
    /// `family_name`/`preferred_username`/`nickname` available at all, and
    /// `raw_claims_from_id_token` now reads every one of them. Beyond that,
    /// three things are the IDENTITY PROVIDER's configuration, not this
    /// app's, and no amount of widening here reaches them:
    ///
    /// 1. **Claims must be in the ID token.** This app reads the ID token
    ///    and never calls the userinfo endpoint. Authentik puts scope
    ///    claims in the ID token by default (`OAuth2Provider.
    ///    include_claims_in_id_token`, `default=True`), and that flag gates
    ///    the SCOPE claims specifically -- turned off, the token still has
    ///    its `iss`/`aud`/`exp`/`nonce`/`sub` machinery but none of the
    ///    name-shaped claims below, so every user shows as the generic
    ///    placeholder.
    /// 2. **The provider must actually have the stock `profile` mapping
    ///    attached** (`authentik-blueprints/oauth2-client.yaml` does). A
    ///    deployment that swapped in a hand-written scope mapping decides
    ///    for itself which of these claims exist.
    /// 3. **Authentik has to have something on file.** For an account
    ///    enrolled through a social source, Authentik's source type sets
    ///    the user's `name`/`username` at ENROLLMENT only -- e.g.
    ///    `authentik/sources/oauth/types/discord.py`'s
    ///    `get_base_user_properties` writes the Discord handle to both. An
    ///    account that instead got LINKED to a pre-existing Authentik user
    ///    never ran that step, so `User.name` stays the empty string it
    ///    defaults to and the token genuinely carries no name; the same
    ///    goes for a local account an admin created without filling the
    ///    Name field in. The `username` fallback
    ///    (`data::users::display_label`) is the whole of what this app can
    ///    do there -- showing a real name requires filling `User.name` (or
    ///    `User.attributes.given_name`/`family_name`) in Authentik.
    ///
    /// One more rollout note, not a configuration issue: `users.username`
    /// was added nullable with no backfill possible (see
    /// `migrations/20260915093000_users_username.sql`), so an existing user
    /// has no username stored until their NEXT sign-in.
    pub async fn authorize_url(&self) -> Result<(Url, PkceCodeVerifier, CsrfToken, Nonce)> {
        let client = self.client().await?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, csrf_state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            // A dedicated scope, not relying on the built-in `profile`
            // mapping's own group-membership behaviour alone -- see
            // Decision 2 of
            // docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
            // on why (real-world reports of the built-in mapping not
            // reliably populating groups). Task 6 adds the matching
            // custom ScopeMapping to this repo's own dev Authentik
            // blueprint; a real deployment's operator provisions the
            // equivalent on their own instance.
            .add_scope(Scope::new("groups".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        Ok((url, pkce_verifier, csrf_state, nonce))
    }

    /// Exchanges the authorization code for tokens, verifies the ID
    /// token's signature/issuer/audience/nonce/expiry (`openidconnect`'s
    /// job, not re-implemented here), extracts the claims this app cares
    /// about into `RawClaims` (`raw_claims_from_id_token`), and maps them
    /// through `identity_from_claims`. Also returns the refresh token, if the
    /// provider issued one (not guaranteed) -- though no caller consumes
    /// it today: `routes::auth::callback` deliberately drops it rather
    /// than persisting it, since nothing implements silent renewal yet
    /// (see `data::users::insert_session`). It is surfaced here so that
    /// work has nothing to re-plumb through the exchange.
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

        let raw = raw_claims_from_id_token(claims);
        let refresh_token = token_response.refresh_token().map(|t| t.secret().clone());

        Ok((identity_from_claims(raw), refresh_token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(email_verified: Option<bool>) -> RawClaims {
        claims_with_groups(email_verified, None)
    }

    fn claims_with_groups(email_verified: Option<bool>, groups: Option<Vec<String>>) -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
            given_name: None,
            family_name: None,
            preferred_username: Some("ada".to_string()),
            nickname: None,
            groups,
        }
    }

    /// Every name-ish claim absent, so a test can set exactly the ones the
    /// scenario it names would really carry.
    fn nameless_claims() -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: None,
            email_verified: None,
            name: None,
            given_name: None,
            family_name: None,
            preferred_username: None,
            nickname: None,
            groups: None,
        }
    }

    /// `sub` really is unconditional (it is the primary key, not a label).
    /// `name` is not, any more -- it is trimmed, blank-filtered and can be
    /// displaced by the `given_name`/`family_name` join; this only pins
    /// that an ordinary `name` claim still wins when there is one.
    #[test]
    fn sub_is_unconditional_and_an_ordinary_name_claim_still_wins() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.sub, "user-123");
        assert_eq!(identity.name, Some("Ada Rider".to_string()));
    }

    /// It is still `data::users::display_label` that decides where a
    /// username RANKS against a name; this only decides which claim the
    /// username is read out of.
    #[test]
    fn preferred_username_wins_the_username_slot_when_present() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.preferred_username, Some("ada".to_string()));

        let mut raw = claims(Some(true));
        raw.preferred_username = None;
        assert_eq!(identity_from_claims(raw).preferred_username, None);

        // ...and with a DIFFERENT non-blank nickname competing for the
        // same slot, so the precedence is actually exercised rather than
        // being true by the other candidate's absence.
        let mut both = claims(Some(true));
        both.nickname = Some("ada-nick".to_string());
        assert_eq!(
            identity_from_claims(both).preferred_username,
            Some("ada".to_string())
        );
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

    #[test]
    fn groups_claim_is_kept_when_present() {
        let identity = identity_from_claims(claims_with_groups(
            Some(true),
            Some(vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]),
        ));
        assert_eq!(
            identity.groups,
            vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]
        );
    }

    #[test]
    fn missing_groups_claim_defaults_to_empty_vec_not_an_error() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.groups, Vec::<String>::new());
    }

    // ---------------------------------------------------------------
    // Widened name/username resolution.
    // ---------------------------------------------------------------

    #[test]
    fn a_blank_name_falls_through_to_given_and_family_name() {
        let mut raw = nameless_claims();
        raw.name = Some(String::new());
        raw.given_name = Some("Ada".to_string());
        raw.family_name = Some("Rider".to_string());
        assert_eq!(
            identity_from_claims(raw).name,
            Some("Ada Rider".to_string())
        );
    }

    /// Authentik omits `family_name` entirely unless
    /// `User.attributes["family_name"]` is set, so "given name only" is the
    /// common half-populated shape, not an edge case. A lone family name is
    /// handled the same way rather than being special-cased away.
    #[test]
    fn either_half_of_the_name_pair_alone_is_still_a_name() {
        let mut given_only = nameless_claims();
        given_only.given_name = Some("Ada".to_string());
        assert_eq!(
            identity_from_claims(given_only).name,
            Some("Ada".to_string())
        );

        let mut family_only = nameless_claims();
        family_only.family_name = Some("Rider".to_string());
        assert_eq!(
            identity_from_claims(family_only).name,
            Some("Rider".to_string())
        );
    }

    #[test]
    fn a_present_name_still_beats_the_given_family_pair() {
        let mut raw = nameless_claims();
        raw.name = Some("Ada Lovelace Rider".to_string());
        raw.given_name = Some("Ada".to_string());
        raw.family_name = Some("Rider".to_string());
        assert_eq!(
            identity_from_claims(raw).name,
            Some("Ada Lovelace Rider".to_string())
        );
    }

    #[test]
    fn every_name_claim_blank_is_no_name_at_all_not_an_empty_string() {
        let mut raw = nameless_claims();
        raw.name = Some(String::new());
        raw.given_name = Some("   ".to_string());
        raw.family_name = Some("\t".to_string());
        assert_eq!(identity_from_claims(raw).name, None);
    }

    #[test]
    fn name_candidates_are_trimmed_and_the_pair_is_joined_with_one_space() {
        let mut raw = nameless_claims();
        raw.given_name = Some("  Ada  ".to_string());
        raw.family_name = Some("  Rider  ".to_string());
        assert_eq!(
            identity_from_claims(raw).name,
            Some("Ada Rider".to_string())
        );
    }

    #[test]
    fn nickname_backs_up_an_absent_or_blank_preferred_username() {
        let mut absent = nameless_claims();
        absent.nickname = Some("ada".to_string());
        assert_eq!(
            identity_from_claims(absent).preferred_username,
            Some("ada".to_string())
        );

        let mut blank = nameless_claims();
        blank.preferred_username = Some("  ".to_string());
        blank.nickname = Some("ada".to_string());
        assert_eq!(
            identity_from_claims(blank).preferred_username,
            Some("ada".to_string())
        );
    }

    /// `nickname` is a USERNAME candidate, never a name one -- Authentik
    /// emits `request.user.username` for it verbatim, and writing a login
    /// handle into `users.name` would mislabel it for everything that reads
    /// that column as "a real name".
    #[test]
    fn nickname_is_never_promoted_into_the_name_slot() {
        let mut raw = nameless_claims();
        raw.nickname = Some("ada".to_string());
        let identity = identity_from_claims(raw);
        assert_eq!(identity.name, None);
        assert_eq!(identity.preferred_username, Some("ada".to_string()));
    }

    // ---------------------------------------------------------------
    // The email guard, at the boundary. `data::users::shareable` is still
    // the enforcement point; these assert the boundary never HELPS an
    // email-shaped value win, and never hides a good value behind one.
    // ---------------------------------------------------------------

    #[test]
    fn an_email_shaped_name_no_longer_shadows_a_real_given_family_name() {
        let mut raw = nameless_claims();
        raw.name = Some("rider@example.com".to_string());
        raw.given_name = Some("Ada".to_string());
        raw.family_name = Some("Rider".to_string());
        assert_eq!(
            identity_from_claims(raw).name,
            Some("Ada Rider".to_string())
        );
    }

    #[test]
    fn an_email_shaped_preferred_username_no_longer_shadows_a_real_nickname() {
        let mut raw = nameless_claims();
        raw.preferred_username = Some("rider@example.com".to_string());
        raw.nickname = Some("ada".to_string());
        assert_eq!(
            identity_from_claims(raw).preferred_username,
            Some("ada".to_string())
        );
    }

    /// A joined pair is guarded as ONE value: an email address in EITHER
    /// half makes the whole join email-shaped, so it must not outrank a
    /// non-email `name`. Both halves are checked because `joined_name`
    /// concatenates them, and a guard that only looked at the first would
    /// pass "Ada rider@example.com" straight through.
    #[test]
    fn an_email_inside_either_half_disqualifies_the_whole_join() {
        let mut given_half = nameless_claims();
        given_half.name = Some("Ada Rider".to_string());
        given_half.given_name = Some("rider@example.com".to_string());
        given_half.family_name = Some("Rider".to_string());
        assert_eq!(
            identity_from_claims(given_half).name,
            Some("Ada Rider".to_string())
        );

        let mut family_half = nameless_claims();
        family_half.name = Some("Ada Rider".to_string());
        family_half.given_name = Some("Ada".to_string());
        family_half.family_name = Some("rider@example.com".to_string());
        assert_eq!(
            identity_from_claims(family_half).name,
            Some("Ada Rider".to_string())
        );
    }

    /// The privacy-critical new value shape, end to end: a genuinely
    /// JOINED string (both halves present, one of them an email address)
    /// with no non-email alternative to fall back to. It is still stored --
    /// the boundary only reorders -- and `display_label` is what declines
    /// to render it, exactly as it does for a plain email-shaped claim.
    #[test]
    fn a_joined_pair_containing_an_email_is_stored_but_never_rendered() {
        let mut raw = nameless_claims();
        raw.given_name = Some("rider@example.com".to_string());
        raw.family_name = Some("Rider".to_string());
        let stored = identity_from_claims(raw).name;
        assert_eq!(stored, Some("rider@example.com Rider".to_string()));
        assert_eq!(crate::data::users::display_label(stored, None), None);

        let mut trailing = nameless_claims();
        trailing.given_name = Some("Ada".to_string());
        trailing.family_name = Some("rider@example.com".to_string());
        let stored = identity_from_claims(trailing).name;
        assert_eq!(stored, Some("Ada rider@example.com".to_string()));
        assert_eq!(crate::data::users::display_label(stored, None), None);
    }

    /// A lone email-shaped half never becomes a join at all, but must be
    /// declined just the same.
    #[test]
    fn a_lone_email_shaped_half_is_stored_but_never_rendered() {
        let mut only_email = nameless_claims();
        only_email.given_name = Some("rider@example.com".to_string());
        let stored = identity_from_claims(only_email).name;
        assert_eq!(stored, Some("rider@example.com".to_string()));
        assert_eq!(crate::data::users::display_label(stored, None), None);
    }

    /// The shared predicate itself, asserted directly rather than only
    /// through its two callers -- it is the single thing standing between a
    /// claim and a leaked address, so a change to it should break a test
    /// that names it.
    #[test]
    fn looks_like_email_address_is_the_blunt_at_sign_test_it_claims_to_be() {
        assert!(looks_like_email_address("rider@example.com"));
        assert!(looks_like_email_address("ADA@EXAMPLE.COM"));
        assert!(looks_like_email_address("Ada Rider <ada@example.com>"));
        assert!(looks_like_email_address("@"));
        assert!(!looks_like_email_address("Ada Rider"));
        assert!(!looks_like_email_address("ada"));
        assert!(!looks_like_email_address(""));
    }

    /// The boundary REORDERS, it does not drop: when every candidate is
    /// email-shaped the first non-blank one is still stored, exactly as it
    /// was before this widening, and `display_label` is what refuses to
    /// render it.
    #[test]
    fn all_candidates_email_shaped_still_stores_one_and_display_label_declines_it() {
        let mut raw = nameless_claims();
        raw.name = Some("first@example.com".to_string());
        raw.given_name = Some("second@example.com".to_string());
        raw.preferred_username = Some("third@example.com".to_string());
        raw.nickname = Some("fourth@example.com".to_string());
        let identity = identity_from_claims(raw);
        assert_eq!(identity.name, Some("first@example.com".to_string()));
        assert_eq!(
            identity.preferred_username,
            Some("third@example.com".to_string())
        );
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            None
        );
    }

    // ---------------------------------------------------------------
    // Real provider payloads, parsed the way `exchange_code` parses them.
    //
    // Claim values below are what Authentik's own shipped `profile` scope
    // mapping evaluates to (goauthentik/authentik
    // `blueprints/system/providers-oauth2.yaml`, version-2026.8), for the
    // two account shapes this app actually sees. `email_verified: false`
    // is not a typo either: Authentik's stock `email` scope mapping
    // hardcodes it.
    // ---------------------------------------------------------------

    /// Everything an ID token needs to parse, minus the claims a given
    /// scenario is about. Not signed or verified -- `raw_claims_from_id_token`
    /// runs on claims `openidconnect` has ALREADY verified, so the only
    /// thing under test here is the claim-name-to-field wiring.
    fn id_token_claims(
        extra: serde_json::Value,
    ) -> IdTokenClaims<AccessGroupClaims, CoreGenderClaim> {
        let mut payload = serde_json::json!({
            "iss": "https://sso.example.com/application/o/distant-signal/",
            "sub": "b4f1c0de-0000-4000-8000-000000000001",
            "aud": "distant-signal",
            "exp": 4_102_444_800i64,
            "iat": 1_700_000_000i64,
        });
        let object = payload.as_object_mut().expect("object");
        for (key, value) in extra.as_object().expect("extra must be an object") {
            object.insert(key.clone(), value.clone());
        }
        serde_json::from_value(payload).expect("ID token claims should deserialize")
    }

    /// A user created directly in Authentik, never federated, whose `name`
    /// attribute was left unset at provisioning time -- so `name` and
    /// `given_name` both arrive as the empty string Authentik defaults
    /// `User.name` to, and the only real identifier in the token is the
    /// username (which Authentik requires, and emits twice).
    #[test]
    fn local_authentik_user_with_no_name_is_labelled_by_their_username() {
        let claims = id_token_claims(serde_json::json!({
            "email": "rider@example.com",
            "email_verified": false,
            "name": "",
            "given_name": "",
            "preferred_username": "ada",
            "nickname": "ada",
            "groups": ["mcp-users"],
        }));
        let identity = identity_from_claims(raw_claims_from_id_token(&claims));
        assert_eq!(identity.name, None);
        assert_eq!(identity.preferred_username, Some("ada".to_string()));
        assert_eq!(identity.groups, vec!["mcp-users".to_string()]);
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            Some("ada".to_string())
        );
    }

    /// The same local account, but provisioned (by an admin, an import, or
    /// a directory sync) with `given_name`/`family_name` in
    /// `User.attributes` and `User.name` still empty. Authentik's mapping
    /// reads those attributes for `given_name`/`family_name` while `name`
    /// stays blank -- the case that used to show this person as their bare
    /// login handle despite their real name being in the same token.
    #[test]
    fn local_authentik_user_with_only_attribute_names_is_labelled_by_their_real_name() {
        let claims = id_token_claims(serde_json::json!({
            "email": "rider@example.com",
            "email_verified": false,
            "name": "",
            "given_name": "Ada",
            "family_name": "Rider",
            "preferred_username": "ada",
            "nickname": "ada",
        }));
        let identity = identity_from_claims(raw_claims_from_id_token(&claims));
        assert_eq!(identity.name, Some("Ada Rider".to_string()));
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            Some("Ada Rider".to_string())
        );
    }

    /// A user who signed up through Authentik's Discord OAuth source.
    /// `authentik/sources/oauth/types/discord.py`'s
    /// `get_base_user_properties` sets BOTH `username` and `name` to the
    /// Discord handle at enrollment, so both claims arrive populated --
    /// there is no missing claim to go hunting for on this path.
    #[test]
    fn discord_sourced_user_is_labelled_by_their_discord_handle() {
        let claims = id_token_claims(serde_json::json!({
            "email": "rider@example.com",
            "email_verified": false,
            "name": "coolrider",
            "given_name": "coolrider",
            "preferred_username": "coolrider",
            "nickname": "coolrider",
            "picture": "https://cdn.discordapp.com/avatars/1/2.png",
            "groups": [],
        }));
        let identity = identity_from_claims(raw_claims_from_id_token(&claims));
        assert_eq!(identity.name, Some("coolrider".to_string()));
        assert_eq!(identity.preferred_username, Some("coolrider".to_string()));
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            Some("coolrider".to_string())
        );
    }

    /// The Discord shape that DOES lose its name: the source was linked to
    /// an Authentik account that already existed (so enrollment never ran
    /// and never wrote `User.name`), leaving `name` blank while the
    /// username stays populated. Nothing in the token carries the Discord
    /// handle in that case -- the username fallback is the whole answer,
    /// and it is an Authentik-side matter whether `User.name` gets filled
    /// in.
    #[test]
    fn discord_sourced_user_linked_to_a_pre_existing_account_falls_back_to_the_username() {
        let claims = id_token_claims(serde_json::json!({
            "email": "rider@example.com",
            "email_verified": false,
            "name": "",
            "given_name": "",
            "preferred_username": "ada",
            "nickname": "ada",
        }));
        let identity = identity_from_claims(raw_claims_from_id_token(&claims));
        assert_eq!(identity.name, None);
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            Some("ada".to_string())
        );
    }

    /// A token carrying nothing name-shaped at all: `display_label` says
    /// `None`, and the frontend renders its generic placeholder. Never the
    /// email, and never the opaque subject.
    #[test]
    fn a_token_with_no_usable_identifier_yields_no_label_not_an_email() {
        let claims = id_token_claims(serde_json::json!({
            "email": "rider@example.com",
            "email_verified": true,
        }));
        let identity = identity_from_claims(raw_claims_from_id_token(&claims));
        assert_eq!(identity.name, None);
        assert_eq!(identity.preferred_username, None);
        assert_eq!(identity.email, Some("rider@example.com".to_string()));
        assert_eq!(
            crate::data::users::display_label(identity.name, identity.preferred_username),
            None
        );
    }

    /// `localized`'s documented contract, which is otherwise only an
    /// assertion in a comment: the untagged form is what this app reads,
    /// it wins over any language-tagged sibling, and a token carrying ONLY
    /// a tagged form reads as no name at all rather than as some
    /// arbitrarily-chosen locale's.
    #[test]
    fn only_the_untagged_form_of_a_localizable_claim_is_read() {
        let tagged_only = id_token_claims(serde_json::json!({
            "name#de": "Ada Fahrerin",
            "preferred_username": "ada",
        }));
        let raw = raw_claims_from_id_token(&tagged_only);
        assert_eq!(raw.name, None);
        assert_eq!(
            identity_from_claims(raw).name,
            None,
            "a language-tagged-only name must not be promoted into the name slot"
        );

        let both = id_token_claims(serde_json::json!({
            "name": "Ada Rider",
            "name#de": "Ada Fahrerin",
        }));
        assert_eq!(
            raw_claims_from_id_token(&both).name.as_deref(),
            Some("Ada Rider")
        );
    }

    /// Guards the claim-NAME wiring itself: a typo in
    /// `raw_claims_from_id_token` would read `None` for a claim the token
    /// plainly carries, and no `identity_from_claims` test could see it.
    #[test]
    fn every_name_shaped_claim_is_read_off_the_token_under_its_own_name() {
        let claims = id_token_claims(serde_json::json!({
            "name": "n",
            "given_name": "g",
            "family_name": "f",
            "preferred_username": "p",
            "nickname": "k",
        }));
        let raw = raw_claims_from_id_token(&claims);
        assert_eq!(raw.name.as_deref(), Some("n"));
        assert_eq!(raw.given_name.as_deref(), Some("g"));
        assert_eq!(raw.family_name.as_deref(), Some("f"));
        assert_eq!(raw.preferred_username.as_deref(), Some("p"));
        assert_eq!(raw.nickname.as_deref(), Some("k"));
        assert_eq!(raw.sub, "b4f1c0de-0000-4000-8000-000000000001");
    }
}
