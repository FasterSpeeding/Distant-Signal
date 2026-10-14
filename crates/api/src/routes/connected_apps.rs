//! `/public/account/connected-apps`: the user's consent to a delegated
//! client (spec docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md
//! §5.3; operator doc docs/connected-apps.md).
//!
//! Every route here takes `AuthenticatedUser`, the session cookie, and
//! nothing else: a delegated bearer can never reach them (`gate` refuses a
//! delegated token on any route outside `DELEGATED_ROUTES`), so a client
//! cannot widen its own grant.
//!
//! * `GET /account/connected-apps`: the user's active grants, the clients
//!   they could connect, and what a client deleted in the last 30 days
//!   ("Recently deleted by the assistant", §5.5a).
//! * `GET /account/connected-apps-notice`: the newest grant made in the
//!   last 7 days, for the banner every page shows (§5.3 control 2).
//! * `GET /account/connected-apps/{client}/consent`: what the consent page
//!   shows, plus a single-use confirmation token bound to this session and
//!   client, valid 10 minutes, stored hashed (§5.3 control 1). Each call
//!   replaces the previous token.
//! * `POST /account/connected-apps/{client}` `{scopes, confirmationToken}`:
//!   creates the grant, replacing an active one. The scopes come from this
//!   body only and must be a subset of the client's `maxScopes`. Strict
//!   same-origin check; 403 `confirmation_required` without a matching,
//!   unexpired, unused token. The notifier then sends the `grant_created`
//!   web push.
//! * `DELETE /account/connected-apps/{client}`: Disconnect. Idempotent
//!   204, strict same-origin.
//!
//! Errors are JSON `{error, message}`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::delegated::{ClientConfig, Scope, error_response, log_hash, valid_client_name};
use crate::auth::{self, AuthenticatedUser, SESSION_COOKIE_NAME, hash_session_token, parse_cookie};
use crate::data::delegated::{self as data, CreateGrantOutcome, RevokeReason};

pub fn router() -> Router {
    Router::new()
        .route(
            "/account/connected-apps",
            axum::routing::get(list_connected_apps),
        )
        .route(
            "/account/connected-apps-notice",
            axum::routing::get(connected_app_notice),
        )
        .route(
            "/account/connected-apps/{client}/consent",
            axum::routing::get(consent),
        )
        .route(
            "/account/connected-apps/{client}",
            axum::routing::post(create_grant).delete(revoke_grant),
        )
}

/// A grant as the page shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantView {
    client: String,
    display_name: String,
    scopes: Vec<String>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    /// `false` when Distant Signal has turned this client off (the kill
    /// switch) or no longer lists it: the grant then does nothing.
    client_enabled: bool,
}

/// A client the user could connect.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientView {
    name: String,
    display_name: String,
    scopes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectedAppsResponse {
    grants: Vec<GrantView>,
    available: Vec<ClientView>,
    recent_deletions: Vec<data::DeletionRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoticeResponse {
    notice: Option<GrantView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsentResponse {
    client: ClientView,
    /// Who is signed in, so the page can say whose account this is.
    username: Option<String>,
    name: Option<String>,
    confirmation_token: String,
    expires_at: DateTime<Utc>,
    existing_grant: Option<GrantView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateGrantRequest {
    scopes: Vec<String>,
    confirmation_token: String,
}

fn database_error(operation: &'static str) -> impl Fn(anyhow::Error) -> Response {
    move |err| {
        if let Some(unavailable) = crate::unavailable::response_for(&err) {
            return unavailable.into_response();
        }
        tracing::error!(error = ?err, operation, "connected-apps request failed");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("failed to {operation}"),
        )
    }
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "no such app")
}

fn client_view(client: &ClientConfig) -> ClientView {
    ClientView {
        name: client.name.clone(),
        display_name: client.display_name.clone(),
        scopes: client
            .max_scopes
            .iter()
            .map(|scope| scope.as_str())
            .collect(),
    }
}

fn grant_view(app: &App, grant: data::GrantRow) -> GrantView {
    let client_enabled = app.delegated.enabled_client(&grant.client).is_some();
    GrantView {
        client: grant.client,
        display_name: grant.client_display_name,
        scopes: grant.scopes,
        created_at: grant.created_at,
        last_used_at: grant.last_used_at,
        client_enabled,
    }
}

/// The strict check `logout` and account deletion use: `Origin` or
/// `Referer` must name this app's own origin, and both absent is refused.
fn same_origin(app: &App, headers: &HeaderMap) -> bool {
    match auth::expected_browser_origin(&app.config.sso_redirect_url) {
        None => true,
        Some(expected) => auth::is_same_origin(
            headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()),
            headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
            &expected,
        ),
    }
}

fn cross_site() -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        "cross_site",
        "cross-site request rejected",
    )
}

/// The `sessions.id` (a hash) of the cookie `AuthenticatedUser` already
/// resolved.
fn session_id(headers: &HeaderMap) -> Option<String> {
    parse_cookie(headers, SESSION_COOKIE_NAME).map(|token| hash_session_token(&token))
}

async fn list_connected_apps(State(app): State<App>, user: AuthenticatedUser) -> Response {
    let grants = match data::list_active_grants(&app.database, &user.id).await {
        Ok(grants) => grants,
        Err(err) => return database_error("list connected apps")(err),
    };
    let recent_deletions = match data::recent_deletions(&app.database, &user.id).await {
        Ok(rows) => rows,
        Err(err) => return database_error("list recent deletions")(err),
    };
    let available = app
        .delegated
        .clients()
        .filter(|client| client.enabled)
        .map(|client| client_view(client))
        .collect();
    let mut response = Json(ConnectedAppsResponse {
        grants: grants
            .into_iter()
            .map(|grant| grant_view(&app, grant))
            .collect(),
        available,
        recent_deletions,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn connected_app_notice(State(app): State<App>, user: AuthenticatedUser) -> Response {
    match data::recent_grant(&app.database, &user.id).await {
        Ok(grant) => Json(NoticeResponse {
            notice: grant.map(|grant| grant_view(&app, grant)),
        })
        .into_response(),
        Err(err) => database_error("read the connected-app notice")(err),
    }
}

async fn consent(
    State(app): State<App>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path(client): Path<String>,
) -> Response {
    let Some(config) = app.delegated.enabled_client(&client).cloned() else {
        return not_found();
    };
    let Some(session_id) = session_id(&headers) else {
        return error_response(StatusCode::UNAUTHORIZED, "no_session", "no session");
    };
    let (token, expires_at) =
        match data::issue_confirmation(&app.database, &session_id, &user.id, &config.name).await {
            Ok(issued) => issued,
            Err(err) => return database_error("issue a confirmation token")(err),
        };
    let existing_grant = match data::list_active_grants(&app.database, &user.id).await {
        Ok(grants) => grants
            .into_iter()
            .find(|grant| grant.client == config.name)
            .map(|grant| grant_view(&app, grant)),
        Err(err) => return database_error("list connected apps")(err),
    };
    let mut response = Json(ConsentResponse {
        client: client_view(&config),
        username: user.username,
        name: user.name,
        confirmation_token: token,
        expires_at,
        existing_grant,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn create_grant(
    State(app): State<App>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path(client): Path<String>,
    body: Result<Json<CreateGrantRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !same_origin(&app, &headers) {
        tracing::warn!(
            "connected-app grant rejected: Origin/Referer did not match this app's own origin"
        );
        return cross_site();
    }
    let Some(config) = app.delegated.enabled_client(&client).cloned() else {
        return not_found();
    };
    let Ok(Json(request)) = body else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "send {\"scopes\": [...], \"confirmationToken\": \"...\"}",
        );
    };
    let mut scopes: Vec<Scope> = Vec::new();
    for scope in &request.scopes {
        match Scope::parse(scope) {
            Some(parsed) if config.max_scopes.contains(&parsed) => {
                if !scopes.contains(&parsed) {
                    scopes.push(parsed);
                }
            }
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_scope",
                    &format!("{scope:?} is not a scope this app can be given"),
                );
            }
        }
    }
    if scopes.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "choose at least one thing the app may do",
        );
    }
    scopes.sort();
    let scopes: Vec<String> = scopes
        .iter()
        .map(|scope| scope.as_str().to_string())
        .collect();
    let Some(session_id) = session_id(&headers) else {
        return error_response(StatusCode::UNAUTHORIZED, "no_session", "no session");
    };
    let outcome = data::create_grant(
        &app.database,
        &session_id,
        &user.id,
        &config.name,
        &config.display_name,
        &scopes,
        request.confirmation_token.trim(),
    )
    .await;
    match outcome {
        Ok(CreateGrantOutcome::Created(grant)) => {
            tracing::info!(
                audit = true,
                event = "delegated_grant_created",
                client = %config.name,
                user_id = %user.id,
                grant_id = grant.id,
                scopes = %scopes.join(" "),
                session_hash = %log_hash(&session_id),
                "a user connected an app"
            );
            (StatusCode::CREATED, Json(grant_view(&app, grant))).into_response()
        }
        Ok(CreateGrantOutcome::ConfirmationRequired) => {
            tracing::warn!(
                audit = true,
                event = "delegated_grant_refused",
                client = %config.name,
                user_id = %user.id,
                reason = "confirmation_required",
                session_hash = %log_hash(&session_id),
                "a grant was attempted without a valid confirmation token"
            );
            error_response(
                StatusCode::FORBIDDEN,
                "confirmation_required",
                "open the Connected apps page and confirm there",
            )
        }
        Err(err) => database_error("create the grant")(err),
    }
}

async fn revoke_grant(
    State(app): State<App>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path(client): Path<String>,
) -> Response {
    if !same_origin(&app, &headers) {
        tracing::warn!(
            "connected-app revoke rejected: Origin/Referer did not match this app's own origin"
        );
        return cross_site();
    }
    if !valid_client_name(&client) {
        return StatusCode::NO_CONTENT.into_response();
    }
    match data::revoke_grants_for_user(&app.database, &user.id, Some(&client), RevokeReason::User)
        .await
    {
        Ok(revoked) => {
            if revoked > 0 {
                tracing::info!(
                    audit = true,
                    event = "delegated_grant_revoked",
                    reason = RevokeReason::User.as_str(),
                    client = %client,
                    user_id = %user.id,
                    session_hash = session_id(&headers).as_deref().map(log_hash),
                    "a user disconnected an app"
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => database_error("revoke the grant")(err),
    }
}

/// `POST /delegated-grants/revoke`, mounted at the root (not under
/// `/public`, so the frontend proxy never reaches it). See
/// [`revoke_for_client_logout`].
pub fn service_router() -> Router {
    Router::new().route(
        "/delegated-grants/revoke",
        axum::routing::post(revoke_for_client_logout),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientLogoutRequest {
    sub: String,
}

/// `POST /delegated-grants/revoke {sub}`: D-7's path 2 (spec §5.9). A
/// client's own service identity (an internal-OAuth token whose `groups`
/// holds that client's `serviceGroup`, `srv-ds-mcp` for ds-mcp) revokes
/// `sub`'s grant **for that client only**, reason `client_logout`, when the
/// user's session at the client ended without DS hearing of it.
///
/// It can only revoke, and its answer never says whether a grant existed:
/// 204 for every well-formed request from a recognised service. A bad or
/// missing token is 401, a verified token in no client's `serviceGroup`
/// 403, a malformed body 400. Accepted on either listener: it is
/// authenticated by the internal token, as the MCP's public reads are, and
/// a leaked `srv-ds-mcp` credential can at worst disconnect ds-mcp users.
async fn revoke_for_client_logout(
    State(app): State<App>,
    headers: HeaderMap,
    body: Result<Json<ClientLogoutRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    use crate::rate_limit::ServiceTokenCheck;
    let unauthorized = || {
        error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "a service token is required",
        )
    };
    let Some(token) = auth::bearer_token(&headers) else {
        return unauthorized();
    };
    let Ok(claims) = app.verify_service_token(&token).await else {
        return unauthorized();
    };
    let clients: Vec<String> = app
        .delegated
        .clients()
        .filter(|client| {
            !client.service_group.is_empty() && claims.groups.contains(&client.service_group)
        })
        .map(|client| client.name.clone())
        .collect();
    if clients.is_empty() {
        tracing::warn!(
            audit = true,
            event = "delegated_revoke_denied",
            service = %claims.sub,
            "a service token outside every client's serviceGroup tried to revoke grants"
        );
        return error_response(
            StatusCode::FORBIDDEN,
            "not_a_client_service",
            "this service may not revoke grants",
        );
    }
    let Ok(Json(request)) = body else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "send {\"sub\": \"...\"}",
        );
    };
    let sub = request.sub.trim();
    if sub.is_empty() || sub.len() > 255 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "sub must be 1 to 255 characters",
        );
    }
    for client in &clients {
        match data::revoke_grants_for_user(
            &app.database,
            sub,
            Some(client),
            RevokeReason::ClientLogout,
        )
        .await
        {
            Ok(revoked) => {
                if revoked > 0 {
                    tracing::warn!(
                        audit = true,
                        event = "delegated_grant_revoked",
                        reason = RevokeReason::ClientLogout.as_str(),
                        client = %client,
                        user_id = %sub,
                        "a client's logout revoked the user's grant"
                    );
                }
            }
            Err(err) => return database_error("revoke the grant")(err),
        }
    }
    StatusCode::NO_CONTENT.into_response()
}
