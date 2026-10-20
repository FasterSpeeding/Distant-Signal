//! Delegated access end to end, against a live database: the consent flow,
//! the grant and scope checks, revocation on the next request, the
//! delegated delete rules, the atomic itinerary save and the sweep (spec
//! docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md
//! §7 Phase 1b item 9).
//!
//! Tokens are real RS256 JWTs signed with the test key and verified through
//! a mock Authentik's JWKS (`internal_oauth::test_support`), so the real
//! verifier, gate, extractor and handlers run. Requests go through the
//! delegated listener's router unless a test says otherwise.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use wiremock::MockServer;

use crate::app::App;
use crate::auth::delegated::{
    ClientConfig, DelegatedAuth, DelegatedSettings, Scope, delegated_listener_router, gate,
};
use crate::auth::hash_session_token;
use crate::auth::internal_oauth::test_support::{mock_authentik, sign_token, valid_claims};
use crate::data::delegated::RevokeReason;

const AUDIENCE: &str = "distant-signal-api";
const ORIGIN: &str = "https://example.invalid";
/// Fixture UIDs: six characters, like a CIF UID, but never real ones.
const UIDS: [&str; 3] = ["ZDLG01", "ZDLG02", "ZDLG03"];

async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to postgres")
}

struct Harness {
    /// The delegated listener.
    router: axum::Router,
    issuer: String,
    internal_issuer: String,
    _servers: (MockServer, MockServer),
}

async fn harness(pool: PgPool) -> Harness {
    let (delegated_server, _) = mock_authentik().await;
    let (internal_server, internal_verifier) = mock_authentik().await;
    let mut state = Arc::try_unwrap(crate::test_support::inert_app(pool)).expect("a fresh app");
    state.internal_oauth_verifier = internal_verifier;
    let settings = DelegatedSettings {
        clients: String::new(),
        listen_addr: "127.0.0.1:0".to_string(),
        daily_save_limit: 3,
        daily_delete_limit: 2,
        revoked_grant_retention_days: 365,
    };
    let client = ClientConfig {
        name: "ds-mcp".to_string(),
        display_name: "Distant Signal assistant".to_string(),
        issuer_url: delegated_server.uri(),
        audience: AUDIENCE.to_string(),
        max_scopes: Scope::ALL.to_vec(),
        enabled: true,
        service_group: "srv-ds-mcp".to_string(),
    };
    state.delegated = Arc::new(DelegatedAuth::from_settings(&settings, vec![client]).unwrap());
    let app: App = Arc::new(state);
    let routes = crate::app::Router::new()
        .merge(super::journeys::router())
        .merge(super::connected_apps::service_router())
        .nest("/public", super::public_router())
        .with_state(app.clone())
        .layer(axum::middleware::from_fn_with_state(
            app.delegated.clone(),
            gate,
        ));
    Harness {
        router: delegated_listener_router(routes),
        issuer: delegated_server.uri(),
        internal_issuer: internal_server.uri(),
        _servers: (delegated_server, internal_server),
    }
}

fn service_date() -> String {
    (crate::routes::london_today() + chrono::Duration::days(2)).to_string()
}

async fn cleanup(pool: &PgPool, prefix: &str) {
    let pattern = format!("{prefix}%");
    sqlx::query("DELETE FROM groups WHERE id LIKE $1")
        .bind(&pattern)
        .execute(pool)
        .await
        .ok();
    sqlx::query(
        "DELETE FROM unlisted_links WHERE resource_type = 'journey' \
           AND resource_id IN (SELECT id::text FROM journeys WHERE user_id LIKE $1)",
    )
    .bind(&pattern)
    .execute(pool)
    .await
    .ok();
    sqlx::query("DELETE FROM users WHERE id LIKE $1")
        .bind(&pattern)
        .execute(pool)
        .await
        .expect("delete fixture users");
    sqlx::query("DELETE FROM trains WHERE train_uid = ANY($1)")
        .bind(UIDS.to_vec())
        .execute(pool)
        .await
        .ok();
}

/// Seeds a user and a session; returns the raw cookie value.
async fn seed_user(pool: &PgPool, user: &str, session: &str) -> String {
    sqlx::query(
        "INSERT INTO users (id, email, name, username) VALUES ($1, $1, $1, $1) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user)
    .execute(pool)
    .await
    .expect("seed user");
    let raw = format!("raw-{session}");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, now() + INTERVAL '1 day')",
    )
    .bind(hash_session_token(&raw))
    .bind(user)
    .execute(pool)
    .await
    .expect("seed session");
    raw
}

enum Auth<'a> {
    Cookie(&'a str),
    /// The cookie, from another site's page.
    CrossSiteCookie(&'a str),
    Bearer(&'a str),
    Service(&'a str),
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    text: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    fn error(&self) -> String {
        self.json()["error"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

async fn send(
    h: &Harness,
    method: &str,
    uri: &str,
    auth: &Auth<'_>,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut request = Request::builder().method(method).uri(uri);
    request = match auth {
        Auth::Cookie(raw) => request
            .header(header::COOKIE, format!("distant_signal_session={raw}"))
            .header(header::ORIGIN, ORIGIN),
        Auth::CrossSiteCookie(raw) => request
            .header(header::COOKIE, format!("distant_signal_session={raw}"))
            .header(header::ORIGIN, "https://evil.example"),
        Auth::Bearer(token) | Auth::Service(token) => {
            request.header(header::AUTHORIZATION, format!("Bearer {token}"))
        }
    };
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = h.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        text: String::from_utf8_lossy(&bytes).to_string(),
    }
}

/// A delegated token for `sub`, issued `iat_offset` seconds from now.
fn token(h: &Harness, sub: &str, scope: &str, iat_offset: i64) -> String {
    let iat = chrono::Utc::now().timestamp() + iat_offset;
    sign_token(&json!({
        "sub": sub,
        "iss": h.issuer,
        "aud": AUDIENCE,
        "azp": AUDIENCE,
        "iat": iat,
        "exp": iat + 300,
        "jti": format!("jti-{iat}"),
        "scope": scope,
    }))
}

/// Connects the app through the consent route and the confirmation token,
/// as the Connected apps page does.
async fn grant(h: &Harness, cookie: &str, scopes: &[&str]) {
    let consent = send(
        h,
        "GET",
        "/public/account/connected-apps/ds-mcp/consent",
        &Auth::Cookie(cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(consent.status, StatusCode::OK, "{}", consent.text);
    let confirmation = consent.json()["confirmationToken"]
        .as_str()
        .unwrap()
        .to_string();
    let created = send(
        h,
        "POST",
        "/public/account/connected-apps/ds-mcp",
        &Auth::Cookie(cookie),
        Some(json!({ "scopes": scopes, "confirmationToken": confirmation })),
        &[],
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
}

fn itinerary(uids: &[&str]) -> Value {
    json!({
        "customName": "Weekend away",
        "legs": uids
            .iter()
            .map(|uid| json!({ "trainUid": uid, "serviceDate": service_date() }))
            .collect::<Vec<_>>(),
    })
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn the_consent_flow_needs_a_fresh_single_use_confirmation_token() {
    let pool = connect().await;
    let prefix = "TEST-DLG-CONSENT-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    let other_cookie = seed_user(&pool, &user, &format!("{prefix}s2")).await;
    let post = |token: &str, scopes: Value| json!({ "scopes": scopes, "confirmationToken": token });
    let uri = "/public/account/connected-apps/ds-mcp";

    // No token at all.
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&cookie),
        Some(post("nope", json!(["journeys:read"]))),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert_eq!(reply.error(), "confirmation_required");

    let consent = send(
        &h,
        "GET",
        &format!("{uri}/consent"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(consent.status, StatusCode::OK, "{}", consent.text);
    let body = consent.json();
    assert_eq!(body["client"]["displayName"], "Distant Signal assistant");
    assert_eq!(body["username"], user.as_str());
    assert_eq!(consent.headers[header::CACHE_CONTROL], "no-store");
    let confirmation = body["confirmationToken"].as_str().unwrap().to_string();

    // Bound to the session that rendered the page.
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&other_cookie),
        Some(post(&confirmation, json!(["journeys:read"]))),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    // A scope the client may not have.
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&cookie),
        Some(post(&confirmation, json!(["journeys:read_shared"]))),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    // Cross-site.
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::CrossSiteCookie(&cookie),
        Some(post(&confirmation, json!(["journeys:read"]))),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);

    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&cookie),
        Some(post(
            &confirmation,
            json!(["journeys:write", "journeys:read"]),
        )),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    assert_eq!(
        reply.json()["scopes"],
        json!(["journeys:read", "journeys:write"])
    );
    // Creating the grant leaves it due for the notifier's push.
    let (display, notified): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT client_display_name, push_notified_at FROM delegated_grants \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(&user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(display, "Distant Signal assistant");
    assert!(notified.is_none(), "the grant_created push is pending");

    // Single use.
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&cookie),
        Some(post(&confirmation, json!(["journeys:read"]))),
        &[],
    )
    .await;
    assert_eq!(reply.error(), "confirmation_required");
    // Expired.
    let consent = send(
        &h,
        "GET",
        &format!("{uri}/consent"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    let confirmation = consent.json()["confirmationToken"]
        .as_str()
        .unwrap()
        .to_string();
    sqlx::query("UPDATE delegated_grant_confirmations SET expires_at = now() - INTERVAL '1 minute' WHERE user_id = $1")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Cookie(&cookie),
        Some(post(&confirmation, json!(["journeys:read"]))),
        &[],
    )
    .await;
    assert_eq!(reply.error(), "confirmation_required");

    let list = send(
        &h,
        "GET",
        "/public/account/connected-apps",
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.json()["grants"].as_array().unwrap().len(), 1);
    assert_eq!(list.json()["grants"][0]["clientEnabled"], true);
    assert_eq!(list.json()["available"][0]["name"], "ds-mcp");
    let notice = send(
        &h,
        "GET",
        "/public/account/connected-apps-notice",
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(notice.json()["notice"]["client"], "ds-mcp");

    let reply = send(&h, "DELETE", uri, &Auth::Cookie(&cookie), None, &[]).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let list = send(
        &h,
        "GET",
        "/public/account/connected-apps",
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert!(list.json()["grants"].as_array().unwrap().is_empty());
    // Idempotent.
    let reply = send(&h, "DELETE", uri, &Auth::Cookie(&cookie), None, &[]).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn delegated_reads_need_an_active_grant_and_the_scope() {
    let pool = connect().await;
    let prefix = "TEST-DLG-READ-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;

    // No grant, and no such user: the same answer.
    for sub in [user.as_str(), "TEST-DLG-READ-nobody"] {
        let reply = send(
            &h,
            "GET",
            "/Journeys/mine",
            &Auth::Bearer(&token(&h, sub, "journeys:read", 0)),
            None,
            &[],
        )
        .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
        assert_eq!(reply.error(), "delegation_not_granted");
    }

    grant(&h, &cookie, &["journeys:read"]).await;
    let read = token(&h, &user, "journeys:read", 0);
    let reply = send(&h, "GET", "/Journeys/mine", &Auth::Bearer(&read), None, &[]).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let last_used: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT last_used_at FROM delegated_grants WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(&user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(last_used.is_some(), "a use is recorded");

    // The token's scope: empty, or another one.
    for scope in ["", "journeys:write"] {
        let reply = send(
            &h,
            "GET",
            "/Journeys/mine",
            &Auth::Bearer(&token(&h, &user, scope, 0)),
            None,
            &[],
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{scope:?}: {}",
            reply.text
        );
        assert_eq!(reply.error(), "insufficient_scope");
        assert_eq!(
            reply.headers[header::WWW_AUTHENTICATE],
            "Bearer error=\"insufficient_scope\", scope=\"journeys:read\""
        );
    }
    // The grant's scope: the token asks for write, the grant has only read.
    let reply = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Bearer(&token(&h, &user, "journeys:read journeys:write", 0)),
        Some(itinerary(&UIDS[..1])),
        &[("idempotency-key", "k1")],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert_eq!(reply.error(), "insufficient_scope");

    // Own journeys only; never a share link.
    let saved = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Cookie(&cookie),
        Some(itinerary(&UIDS[..1])),
        &[],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    let journey_id = saved.json()["journeyId"].as_i64().unwrap();
    let link = send(
        &h,
        "POST",
        &format!("/Journeys/{journey_id}/share-link"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(link.status, StatusCode::OK, "{}", link.text);
    let reply = send(
        &h,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &Auth::Bearer(&read),
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.json()["shareLink"], Value::Null);
    assert_eq!(reply.json()["createdVia"], "web");
    let web = send(
        &h,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert!(
        web.json()["shareLink"].is_object(),
        "the cookie path still shows it"
    );

    // Another user's journey, even one shared with this user: 404.
    let other = format!("{prefix}other");
    let other_cookie = seed_user(&pool, &other, &format!("{prefix}s3")).await;
    let theirs = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Cookie(&other_cookie),
        Some(itinerary(&UIDS[1..2])),
        &[],
    )
    .await;
    let their_id = theirs.json()["journeyId"].as_i64().unwrap();
    let group = format!("{prefix}group");
    sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, $1, $2)")
        .bind(&group)
        .bind(&other)
        .execute(&pool)
        .await
        .unwrap();
    for (member, role) in [(&other, "owner"), (&user, "member")] {
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, $3)")
            .bind(&group)
            .bind(member)
            .bind(role)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, $2, $3)")
        .bind(&group)
        .bind(their_id)
        .bind(&other)
        .execute(&pool)
        .await
        .unwrap();
    let web = send(
        &h,
        "GET",
        &format!("/Journeys/{their_id}"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(
        web.status,
        StatusCode::OK,
        "group members read it with a cookie"
    );
    assert!(
        web.json().get("createdVia").is_none(),
        "only the owner sees createdVia"
    );
    let reply = send(
        &h,
        "GET",
        &format!("/Journeys/{their_id}"),
        &Auth::Bearer(&read),
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);
    assert_eq!(reply.error(), "not_found");
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn revocation_takes_effect_on_the_next_request() {
    let pool = connect().await;
    let prefix = "TEST-DLG-REVOKE-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    let get = |token: String| {
        let h = &h;
        async move { send(h, "GET", "/Journeys/mine", &Auth::Bearer(&token), None, &[]).await }
    };
    let reason = || async {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT revoked_reason FROM delegated_grants WHERE user_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(&user)
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // Disconnect.
    grant(&h, &cookie, &["journeys:read"]).await;
    assert_eq!(
        get(token(&h, &user, "journeys:read", 0)).await.status,
        StatusCode::OK
    );
    send(
        &h,
        "DELETE",
        "/public/account/connected-apps/ds-mcp",
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(
        get(token(&h, &user, "journeys:read", 0)).await.error(),
        "delegation_not_granted"
    );
    assert_eq!(reason().await.as_deref(), Some("user"));

    // A new consent refuses tokens minted before it.
    let old = token(&h, &user, "journeys:read", -30);
    grant(&h, &cookie, &["journeys:read"]).await;
    let reply = get(old).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(reply.error(), "invalid_token");
    assert_eq!(
        get(token(&h, &user, "journeys:read", 0)).await.status,
        StatusCode::OK
    );

    // "Log out other sessions" keeps the grant but kills older tokens.
    sqlx::query("UPDATE users SET sessions_invalidated_at = now() WHERE id = $1")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get(token(&h, &user, "journeys:read", -5)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(token(&h, &user, "journeys:read", 2)).await.status,
        StatusCode::OK
    );

    // Admin revoke and back-channel logout.
    for (cause, expected) in [
        (RevokeReason::Admin, "admin"),
        (RevokeReason::Backchannel, "backchannel"),
    ] {
        let cookie = seed_user(&pool, &user, &format!("{prefix}s-{expected}")).await;
        grant(&h, &cookie, &["journeys:read"]).await;
        let revoked = crate::data::users::revoke_all_sessions(&pool, &user, cause)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(revoked.grants_revoked, 1);
        assert_eq!(
            get(token(&h, &user, "journeys:read", 2)).await.error(),
            "delegation_not_granted"
        );
        assert_eq!(reason().await.as_deref(), Some(expected));
    }

    // 90 days unused, before the sweep has run.
    let cookie = seed_user(&pool, &user, &format!("{prefix}s-idle")).await;
    grant(&h, &cookie, &["journeys:read"]).await;
    sqlx::query(
        "UPDATE delegated_grants SET created_at = now() - INTERVAL '120 days', \
           not_before = now() - INTERVAL '120 days', last_used_at = now() - INTERVAL '91 days' \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(&user)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        get(token(&h, &user, "journeys:read", 2)).await.error(),
        "delegation_not_granted"
    );
    assert_eq!(reason().await.as_deref(), Some("inactive"));

    // The client's own logout (path 2): its service identity only.
    grant(&h, &cookie, &["journeys:read"]).await;
    let service = |groups: Value| {
        sign_token(&valid_claims(&h.internal_issuer, |claims| {
            claims["sub"] = json!("srv-ds-mcp");
            claims["groups"] = groups;
        }))
    };
    let uri = "/delegated-grants/revoke";
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Service(&service(json!(["srv-other"]))),
        Some(json!({ "sub": user })),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    let reply = send(
        &h,
        "POST",
        uri,
        &Auth::Service("not-a-token"),
        Some(json!({ "sub": user })),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        get(token(&h, &user, "journeys:read", 2)).await.status,
        StatusCode::OK
    );
    for sub in [user.as_str(), "TEST-DLG-REVOKE-nobody"] {
        let reply = send(
            &h,
            "POST",
            uri,
            &Auth::Service(&service(json!(["srv-ds-mcp"]))),
            Some(json!({ "sub": sub })),
            &[],
        )
        .await;
        assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    }
    assert_eq!(
        get(token(&h, &user, "journeys:read", 2)).await.error(),
        "delegation_not_granted"
    );
    assert_eq!(reason().await.as_deref(), Some("client_logout"));
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn delegated_delete_reaches_only_unshared_journeys_the_client_saved() {
    let pool = connect().await;
    let prefix = "TEST-DLG-DELETE-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    grant(
        &h,
        &cookie,
        &["journeys:read", "journeys:write", "journeys:delete"],
    )
    .await;
    let all = token(&h, &user, "journeys:read journeys:write journeys:delete", 0);
    let save = |key: &'static str, uids: &'static [&'static str]| {
        let (h, all) = (&h, all.clone());
        async move {
            let reply = send(
                h,
                "POST",
                "/Journeys/itinerary",
                &Auth::Bearer(&all),
                Some(itinerary(uids)),
                &[("idempotency-key", key)],
            )
            .await;
            assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
            reply.json()["journeyId"].as_i64().unwrap()
        }
    };
    let delete = |id: i64, token: String| {
        let h = &h;
        async move {
            send(
                h,
                "DELETE",
                &format!("/Journeys/{id}"),
                &Auth::Bearer(&token),
                None,
                &[],
            )
            .await
        }
    };

    // Saved in the web app: 404, as if it did not exist.
    let web = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Cookie(&cookie),
        Some(itinerary(&UIDS[..1])),
        &[],
    )
    .await;
    let web_id = web.json()["journeyId"].as_i64().unwrap();
    let reply = delete(web_id, all.clone()).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);

    // No delete scope.
    let saved = save("d1", &UIDS[..2]).await;
    let reply = delete(saved, token(&h, &user, "journeys:read journeys:write", 0)).await;
    assert_eq!(reply.error(), "insufficient_scope");

    // Shared through a link: 409 until the link is revoked.
    send(
        &h,
        "POST",
        &format!("/Journeys/{saved}/share-link"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    let reply = delete(saved, all.clone()).await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert_eq!(reply.error(), "journey_shared");
    send(
        &h,
        "DELETE",
        &format!("/Journeys/{saved}/share-link"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    let reply = delete(saved, all.clone()).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    let summary: Value = sqlx::query_scalar(
        "SELECT summary FROM delegated_deletions WHERE user_id = $1 AND journey_id = $2",
    )
    .bind(&user)
    .bind(saved)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(summary["customName"], "Weekend away");
    assert_eq!(summary["legs"].as_array().unwrap().len(), 2);
    assert_eq!(summary["legs"][0]["trainUid"], UIDS[0]);

    // Shared into a group: 409.
    let grouped = save("d2", &UIDS[..1]).await;
    let group = format!("{prefix}group");
    sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, $1, $2)")
        .bind(&group)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(&group)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, $2, $3)")
        .bind(&group)
        .bind(grouped)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(delete(grouped, all.clone()).await.error(), "journey_shared");

    // The daily cap (2 in this harness): one delete left, then 429.
    let second = save("d3", &UIDS[..1]).await;
    assert_eq!(
        delete(second, all.clone()).await.status,
        StatusCode::NO_CONTENT
    );
    let third = save("d4", &UIDS[2..]).await;
    let reply = delete(third, all.clone()).await;
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        reply.text
    );
    assert_eq!(reply.error(), "daily_limit");
    assert!(reply.headers.contains_key(header::RETRY_AFTER));

    // The Connected apps page lists both deletions.
    let list = send(
        &h,
        "GET",
        "/public/account/connected-apps",
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(list.json()["recentDeletions"].as_array().unwrap().len(), 2);
    // The cookie path is unchanged: it deletes the web journey.
    let reply = send(
        &h,
        "DELETE",
        &format!("/Journeys/{web_id}"),
        &Auth::Cookie(&cookie),
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn the_itinerary_save_is_atomic_and_idempotent() {
    let pool = connect().await;
    let prefix = "TEST-DLG-ITIN-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    grant(&h, &cookie, &["journeys:write"]).await;
    let write = token(&h, &user, "journeys:write", 0);
    let post = |body: Value, key: Option<&'static str>| {
        let (h, write) = (&h, write.clone());
        async move {
            let headers: Vec<(&str, &str)> =
                key.map(|k| ("idempotency-key", k)).into_iter().collect();
            send(
                h,
                "POST",
                "/Journeys/itinerary",
                &Auth::Bearer(&write),
                Some(body),
                &headers,
            )
            .await
        }
    };
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journeys WHERE user_id = $1")
            .bind(&user)
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    let reply = post(itinerary(&UIDS[..2]), None).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.error(), "idempotency_key_required");

    let first = post(itinerary(&UIDS[..2]), Some("save-1")).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);
    let body = first.json();
    assert_eq!(body["createdVia"], "ds-mcp");
    assert_eq!(body["legs"].as_array().unwrap().len(), 2);
    assert_eq!(body["legId"], body["legs"][0]["legId"]);
    let journey_id = body["journeyId"].as_i64().unwrap();
    let legs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
        .bind(journey_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(legs, 2);

    // A retry with the same key and body: the first answer, no new journey.
    let again = post(itinerary(&UIDS[..2]), Some("save-1")).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json(), body);
    assert_eq!(count().await, 1);
    // The same key with a different body.
    let reply = post(itinerary(&UIDS[..1]), Some("save-1")).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.error(), "idempotency_conflict");

    // Invalid itineraries, and the leg cap for a delegated caller.
    let reply = post(
        json!({ "legs": [{ "trainUid": "nope", "serviceDate": service_date() }] }),
        Some("bad-1"),
    )
    .await;
    assert_eq!(reply.error(), "invalid_itinerary");
    let reply = post(json!({ "legs": [] }), Some("bad-2")).await;
    assert_eq!(reply.error(), "invalid_itinerary");
    let too_many: Vec<&str> = std::iter::repeat_n(UIDS[0], 21).collect();
    let reply = post(itinerary(&too_many), Some("bad-3")).await;
    assert_eq!(reply.error(), "invalid_itinerary");
    // A failing leg leaves nothing behind (horizon on the last leg).
    let mut body = itinerary(&UIDS[..2]);
    body["legs"][1]["serviceDate"] = json!("2099-01-01");
    let reply = post(body, Some("bad-4")).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(count().await, 1);

    // The daily cap (3 in this harness).
    assert_eq!(
        post(itinerary(&UIDS[2..]), Some("save-2")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        post(itinerary(&UIDS[2..]), Some("save-3")).await.status,
        StatusCode::OK
    );
    let reply = post(itinerary(&UIDS[2..]), Some("save-4")).await;
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        reply.text
    );
    assert_eq!(reply.error(), "daily_limit");
    // A replay of an earlier save still answers, even over the cap.
    assert_eq!(
        post(itinerary(&UIDS[..2]), Some("save-1")).await.status,
        StatusCode::OK
    );

    // The web planner: no key needed, plain-text errors, created_via web,
    // and not counted against the client's cap.
    let web = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Cookie(&cookie),
        Some(itinerary(&UIDS[..2])),
        &[],
    )
    .await;
    assert_eq!(web.status, StatusCode::OK, "{}", web.text);
    assert_eq!(web.json()["createdVia"], "web");
    let bad = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Cookie(&cookie),
        Some(json!({ "legs": [] })),
        &[],
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert!(
        bad.headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    cleanup(&pool, prefix).await;
}

/// Spec 2026-10-20 §2.4: a delegated save may include untracked legs
/// (here a Tube hop before the train); they come back with `kind` and no
/// tracking id, read back through the delegated `GET /Journeys/{id}`, and a
/// delegated delete still follows the `created_via` rule and names them in
/// its summary.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn a_delegated_save_may_include_untracked_legs() {
    let pool = connect().await;
    let prefix = "TEST-DLG-UNTRACKED-";
    cleanup(&pool, prefix).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    grant(
        &h,
        &cookie,
        &["journeys:read", "journeys:write", "journeys:delete"],
    )
    .await;
    let all = token(&h, &user, "journeys:read journeys:write journeys:delete", 0);
    // A TfL stop for the Tube hop's origin (TfL stop lookup, 2026-10-20).
    let stop_fixture = "DELETE FROM tfl_stops WHERE stop_id = '940GZZTESTK'";
    sqlx::query(stop_fixture).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO tfl_stops (stop_id, name, search_name, modes, fetched_at) \
         VALUES ('940GZZTESTK', 'Brixton', 'brixton', '{tube}', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();
    let departure = chrono::Utc::now() + chrono::Duration::days(2);
    let body = json!({
        "customName": "Door to door",
        "legs": [
            {
                "kind": "untracked",
                "transitMode": "tube",
                "lineName": "Victoria line",
                "originName": "Brixton",
                "originStopId": "940GZZTESTK",
                "destinationName": "Euston",
                "plannedDeparture": departure.to_rfc3339(),
                "plannedArrival": (departure + chrono::Duration::minutes(25)).to_rfc3339(),
            },
            { "trainUid": UIDS[0], "serviceDate": service_date() },
        ],
    });
    let saved = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Bearer(&all),
        Some(body.clone()),
        &[("idempotency-key", "u1")],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    let saved = saved.json();
    assert_eq!(saved["legs"][0]["kind"], "untracked");
    assert!(saved["legs"][0]["trackingId"].is_null());
    assert_eq!(saved["legs"][1]["kind"], "train");
    assert_eq!(saved["trackingId"], saved["legs"][1]["trackingId"]);
    assert_eq!(saved["legId"], saved["legs"][0]["legId"]);
    let journey_id = saved["journeyId"].as_i64().unwrap();

    // Replayed byte for byte.
    let again = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Bearer(&all),
        Some(body),
        &[("idempotency-key", "u1")],
    )
    .await;
    assert_eq!(again.json(), saved);

    let read = send(
        &h,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &Auth::Bearer(&all),
        None,
        &[],
    )
    .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.text);
    let legs = read.json()["legs"].clone();
    assert_eq!(legs[0]["kind"], "untracked");
    assert_eq!(legs[0]["transitMode"], "tube");
    assert_eq!(legs[0]["originName"], "Brixton");
    assert_eq!(legs[0]["originStopId"], "940GZZTESTK");
    assert!(legs[0]["destinationStopId"].is_null());
    assert_eq!(legs[0]["untrackedStatus"], "planned");
    assert_eq!(legs[1]["kind"], "train");
    assert!(legs[1]["originStopId"].is_null());

    // An unknown stop id, or one on a train leg, is refused.
    for (key, legs) in [
        (
            "u3",
            json!([
                {
                    "kind": "untracked", "transitMode": "tube", "originStopId": "940GZZNOWHERE",
                    "plannedDeparture": departure.to_rfc3339(),
                    "plannedArrival": departure.to_rfc3339(),
                },
                { "trainUid": UIDS[0], "serviceDate": service_date() },
            ]),
        ),
        (
            "u4",
            json!([{ "trainUid": UIDS[0], "serviceDate": service_date(), "originStopId": "940GZZTESTK" }]),
        ),
    ] {
        let refused = send(
            &h,
            "POST",
            "/Journeys/itinerary",
            &Auth::Bearer(&all),
            Some(json!({ "legs": legs })),
            &[("idempotency-key", key)],
        )
        .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.text);
        assert_eq!(refused.error(), "invalid_itinerary");
    }

    // An untracked leg alone is not an itinerary.
    let alone = send(
        &h,
        "POST",
        "/Journeys/itinerary",
        &Auth::Bearer(&all),
        Some(json!({ "legs": [{
            "kind": "untracked", "transitMode": "walk",
            "plannedDeparture": departure.to_rfc3339(),
            "plannedArrival": departure.to_rfc3339(),
        }] })),
        &[("idempotency-key", "u2")],
    )
    .await;
    assert_eq!(alone.status, StatusCode::BAD_REQUEST, "{}", alone.text);
    assert_eq!(alone.error(), "invalid_itinerary");

    let deleted = send(
        &h,
        "DELETE",
        &format!("/Journeys/{journey_id}"),
        &Auth::Bearer(&all),
        None,
        &[],
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text);
    let summary: Value = sqlx::query_scalar(
        "SELECT summary FROM delegated_deletions WHERE user_id = $1 AND journey_id = $2",
    )
    .bind(&user)
    .bind(journey_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(summary["legs"][0]["kind"], "untracked");
    assert_eq!(summary["legs"][0]["transitMode"], "tube");
    assert_eq!(summary["legs"][0]["originName"], "Brixton");
    assert_eq!(summary["legs"][0]["originStopId"], "940GZZTESTK");
    assert_eq!(summary["legs"][1]["trainUid"], UIDS[0]);
    cleanup(&pool, prefix).await;
    sqlx::query(stop_fixture).execute(&pool).await.unwrap();
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn the_sweep_expires_idle_grants_and_purges_old_rows() {
    let pool = connect().await;
    let prefix = "TEST-DLG-SWEEP-";
    cleanup(&pool, prefix).await;
    let user = format!("{prefix}user");
    let _ = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    let insert_grant = |client: &'static str,
                        created: &'static str,
                        used: &'static str,
                        revoked: &'static str| {
        let (pool, user) = (pool.clone(), user.clone());
        async move {
            sqlx::query_scalar::<_, i64>(&format!(
                "INSERT INTO delegated_grants (user_id, client, client_display_name, scopes, created_at, \
                     last_used_at, revoked_at, revoked_reason) \
                 VALUES ($1, '{client}', 'App', '{{journeys:read}}', {created}, {used}, {revoked}, \
                         CASE WHEN {revoked} IS NULL THEN NULL ELSE 'user' END) RETURNING id"
            ))
            .bind(&user)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let idle = insert_grant(
        "sweep-idle",
        "now() - INTERVAL '100 days'",
        "now() - INTERVAL '95 days'",
        "NULL",
    )
    .await;
    let never_used =
        insert_grant("sweep-unused", "now() - INTERVAL '91 days'", "NULL", "NULL").await;
    let active = insert_grant(
        "sweep-active",
        "now() - INTERVAL '200 days'",
        "now() - INTERVAL '1 day'",
        "NULL",
    )
    .await;
    let old_revoked = insert_grant(
        "sweep-gone",
        "now() - INTERVAL '500 days'",
        "NULL",
        "now() - INTERVAL '366 days'",
    )
    .await;
    // Revoked under a year ago: kept, as the record of the access.
    let recent_revoked = insert_grant(
        "sweep-kept",
        "now() - INTERVAL '200 days'",
        "NULL",
        "now() - INTERVAL '91 days'",
    )
    .await;
    sqlx::query(
        "INSERT INTO delegated_deletions (user_id, client, journey_id, deleted_at, summary) \
         VALUES ($1, 'ds-mcp', 1, now() - INTERVAL '31 days', '{}'), ($1, 'ds-mcp', 2, now(), '{}')",
    )
    .bind(&user)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO journey_idempotency_keys (user_id, idempotency_key, request_hash, created_at) \
         VALUES ($1, 'old', repeat('a', 64), now() - INTERVAL '25 hours'), ($1, 'new', repeat('a', 64), now())",
    )
    .bind(&user)
    .execute(&pool)
    .await
    .unwrap();

    let outcome = crate::data::delegated::sweep(&pool, chrono::Utc::now(), 90, 365)
        .await
        .unwrap();
    assert!(
        outcome.inactive_grants >= 2 && outcome.revoked_grants_purged >= 1,
        "{outcome:?}"
    );
    let reason = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT revoked_reason FROM delegated_grants WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(reason(idle).await, Some(Some("inactive".to_string())));
    assert_eq!(reason(never_used).await, Some(Some("inactive".to_string())));
    assert_eq!(reason(active).await, Some(None));
    assert_eq!(
        reason(old_revoked).await,
        None,
        "deleted a year after revocation"
    );
    assert_eq!(
        reason(recent_revoked).await,
        Some(Some("user".to_string())),
        "kept until a year after revocation"
    );
    let deletions: Vec<i64> =
        sqlx::query_scalar("SELECT journey_id FROM delegated_deletions WHERE user_id = $1")
            .bind(&user)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(deletions, vec![2]);
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT idempotency_key FROM journey_idempotency_keys WHERE user_id = $1",
    )
    .bind(&user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(keys, vec!["new".to_string()]);
    // Idempotent.
    let again = crate::data::delegated::sweep(&pool, chrono::Utc::now(), 90, 365)
        .await
        .unwrap();
    assert_eq!(again.inactive_grants, 0, "{again:?}");
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
async fn a_delegated_itinerary_leg_must_call_at_its_overrides() {
    use super::leg_validation_db_tests::{ORDINARY, SLEEPER, UNMATCHED, seed_schedules};

    let pool = connect().await;
    let prefix = "TEST-DLG-LEGVAL-";
    cleanup(&pool, prefix).await;
    let _schedules = seed_schedules(&pool).await;
    let h = harness(pool.clone()).await;
    let user = format!("{prefix}user");
    let cookie = seed_user(&pool, &user, &format!("{prefix}s1")).await;
    grant(&h, &cookie, &["journeys:write"]).await;
    let write = token(&h, &user, "journeys:write", 0);
    let date = super::leg_validation_db_tests::service_date().to_string();
    let leg = |uid: &str, origin: &str, destination: &str| {
        json!({ "trainUid": uid, "serviceDate": date,
                "originCrs": origin, "destinationCrs": destination })
    };
    let post = |legs: Vec<Value>, key: &'static str| {
        let (h, write) = (&h, write.clone());
        async move {
            send(
                h,
                "POST",
                "/Journeys/itinerary",
                &Auth::Bearer(&write),
                Some(json!({ "legs": legs })),
                &[("idempotency-key", key)],
            )
            .await
        }
    };

    // Every code, as `422 {error, message}`; nothing is saved.
    let cases = [
        (
            leg(ORDINARY, "ZQX", "ZQG"),
            "train_does_not_call_at_origin",
            "k1",
        ),
        (
            leg(ORDINARY, "ZQA", "ZQC"),
            "train_does_not_call_at_destination",
            "k2",
        ),
        (
            leg(ORDINARY, "ZQE", "ZQB"),
            "destination_before_origin",
            "k3",
        ),
        (leg(ORDINARY, "ZQD", "ZQG"), "boarding_not_allowed", "k4"),
        (leg(ORDINARY, "ZQA", "ZQF"), "alighting_not_allowed", "k5"),
        // The sleeper's pick-up-only Preston as a destination.
        (leg(SLEEPER, "ZSE", "ZSP"), "alighting_not_allowed", "k6"),
    ];
    for (body, code, key) in cases {
        let reply = post(vec![body], key).await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            reply.text
        );
        assert_eq!(reply.error(), code, "{}", reply.text);
        let message = reply.json()["message"].as_str().unwrap().to_string();
        assert!(
            message.starts_with("Train ") || message.starts_with("You can't"),
            "{message}"
        );
    }
    // A bad second leg names its position and saves neither leg.
    let reply = post(
        vec![leg(ORDINARY, "ZQA", "ZQB"), leg(SLEEPER, "ZSS", "ZSI")],
        "k7",
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.text
    );
    assert_eq!(reply.error(), "boarding_not_allowed");
    assert!(
        reply.json()["message"]
            .as_str()
            .unwrap()
            .starts_with("Leg 2: "),
        "{}",
        reply.text
    );
    let saved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journeys WHERE user_id = $1")
        .bind(&user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, 0);

    // A mixed itinerary: the untracked leg is never checked, but it counts
    // towards the leg number of the bad train leg after it.
    let departure = chrono::Utc::now() + chrono::Duration::days(2);
    let untracked = json!({
        "kind": "untracked",
        "transitMode": "tube",
        "lineName": "Victoria line",
        "originName": "Brixton",
        "destinationName": "Euston",
        "plannedDeparture": departure.to_rfc3339(),
        "plannedArrival": (departure + chrono::Duration::minutes(25)).to_rfc3339(),
    });
    let reply = post(vec![untracked.clone(), leg(SLEEPER, "ZSE", "ZSP")], "m1").await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        reply.text
    );
    assert_eq!(reply.error(), "alighting_not_allowed");
    assert!(
        reply.json()["message"]
            .as_str()
            .unwrap()
            .starts_with("Leg 2: "),
        "{}",
        reply.text
    );
    let saved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journeys WHERE user_id = $1")
        .bind(&user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, 0);
    let reply = post(vec![untracked, leg(SLEEPER, "ZSE", "ZSS")], "m2").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.json()["legs"][0]["kind"], "untracked");

    // The happy path: a request stop at either end, the sleeper to
    // Stirling, and a train not matched yet (accepted, as before).
    let reply = post(
        vec![
            leg(ORDINARY, "ZQA", "ZQE"),
            leg(ORDINARY, "ZQE", "ZQG"),
            leg(SLEEPER, "ZSW", "ZSS"),
            leg(UNMATCHED, "ZQX", "ZQY"),
        ],
        "k8",
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.json()["legs"].as_array().unwrap().len(), 4);

    cleanup(&pool, prefix).await;
}
