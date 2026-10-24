//! `push_subscriptions`: one row per browser/device a user has granted
//! push permission on. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decision 5.
//!
//! 2026-09 security review finding (SECURITY, Medium): this module used to
//! accept ANY `endpoint` URL with no validation, and to silently reassign
//! ownership of an existing row on an `endpoint` conflict. Both are fixed
//! here -- see [`validate_push_endpoint`] and [`upsert_push_subscription`]'s
//! own doc comments for each half of the fix.
//!
//! **L9 follow-up (2026-09-26 review):** [`validate_push_endpoint`] only
//! ever ran once, at registration time. A DNS name that resolved to a
//! public IP that day can rebind to an internal/private address by the
//! time `crates/notifier` actually sends to it -- registration-time
//! validation alone doesn't catch that. The actual resolve-and-check logic
//! moved to `common::outbound_endpoint_guard::validate_outbound_url` (this
//! function is now a thin, same-signature wrapper over it) specifically so
//! `crates/notifier` -- which has no dependency on this crate -- can run
//! the identical check again immediately before its own send, closing that
//! window. See `common::outbound_endpoint_guard`'s own module doc and
//! `crates/notifier/src/send.rs`'s send-time call for the other half.

use anyhow::Result;
use sqlx::PgPool;

/// Rejects any `endpoint` a malicious (or merely careless) caller could use
/// to make [`crates/notifier/src/send.rs`]'s later VAPID-signed `POST`
/// land somewhere inside the cluster's own network instead of at a real
/// push service -- an SSRF vector, since that POST is issued server-side
/// on every notification, using whatever URL this function let through.
/// Thin wrapper over `common::outbound_endpoint_guard::validate_outbound_url`
/// -- see that function's own doc comment for the full scheme/DNS-rebinding
/// rationale, shared verbatim with `crates/notifier`'s own send-time
/// re-check (this module's doc comment above).
pub async fn validate_push_endpoint(endpoint: &str) -> Result<(), String> {
    common::outbound_endpoint_guard::validate_outbound_url(endpoint).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushSubscriptionUpsert {
    /// Row inserted, or updated in place because the caller already owned
    /// the existing row at this `endpoint`.
    Saved,
    /// The `endpoint` already belongs to a DIFFERENT user -- rejected, not
    /// silently reassigned. See this function's own doc comment.
    EndpointOwnedByAnotherUser,
}

/// `ON CONFLICT (endpoint)`, not `(user_id, endpoint)`: the Push API's
/// `endpoint` is already a globally unique per-device-registration URL, so
/// a conflict here can only mean "this caller already has a row for this
/// exact endpoint" (the ordinary re-subscribe/refresh case, which this
/// still updates in place) or "someone else's endpoint got `POSTed` by this
/// caller" (2026-09 security review finding: previously handled by
/// blindly reassigning `user_id` to the new caller, silently taking over
/// another user's subscription -- their notifications would go dark, and
/// the attacker's own notifications would start being pushed to the
/// VICTIM's device, since `endpoint` is inherently tied to one physical
/// browser registration no `INSERT` can actually redirect). The `WHERE
/// push_subscriptions.user_id = EXCLUDED.user_id` clause on the `DO
/// UPDATE` makes that second case a silent no-op at the SQL level --
/// `rows_affected() == 0` is how this function tells the two apart and
/// reports the conflict back to the caller instead of pretending it
/// succeeded.
pub async fn upsert_push_subscription(
    pool: &PgPool,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    device_label: Option<&str>,
) -> Result<PushSubscriptionUpsert> {
    let result = sqlx::query(
        "INSERT INTO push_subscriptions \
             (user_id, endpoint, p256dh, auth, created_at, last_seen_at, device_label) \
         VALUES ($1, $2, $3, $4, NOW(), NOW(), $5) \
         ON CONFLICT (endpoint) DO UPDATE SET \
           p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth, last_seen_at = NOW(), \
           device_label = COALESCE(EXCLUDED.device_label, push_subscriptions.device_label) \
         WHERE push_subscriptions.user_id = EXCLUDED.user_id",
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .bind(device_label)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(PushSubscriptionUpsert::EndpointOwnedByAnotherUser);
    }
    evict_push_subscriptions_over_cap(pool, user_id).await?;
    Ok(PushSubscriptionUpsert::Saved)
}

/// How many push subscriptions (browsers/devices) one user may hold
/// (2026-09-26 review, L10). Every row is a separate outbound web-push POST
/// on every notification the notifier sends that user, so an unbounded
/// count is unbounded outbound work (and, before L9, unbounded SSRF
/// attempts). 20 comfortably covers a real person's phones, tablets, work
/// and home browsers, and profile churn.
pub const MAX_PUSH_SUBSCRIPTIONS_PER_USER: i64 = 20;

/// Enforces [`MAX_PUSH_SUBSCRIPTIONS_PER_USER`] by EVICTING the
/// least-recently-seen rows beyond the cap, rather than rejecting the new
/// subscription. A browser that's been reinstalled or had its site data
/// cleared leaves its old row behind until the push service 404s/410s it
/// (`notifier::send`), so a hard reject would lock a real user out of
/// subscribing their CURRENT device because of dead ones; the row they just
/// saved is by definition the most recently seen, so it always survives.
async fn evict_push_subscriptions_over_cap(pool: &PgPool, user_id: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM push_subscriptions WHERE id IN ( \
            SELECT id FROM push_subscriptions WHERE user_id = $1 \
            ORDER BY last_seen_at DESC, id DESC OFFSET $2)",
    )
    .bind(user_id)
    .bind(MAX_PUSH_SUBSCRIPTIONS_PER_USER)
    .execute(pool)
    .await?;
    Ok(())
}

/// One of a user's devices with push on, for the Notifications page.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushDevice {
    pub id: i64,
    /// "Chrome on Android"; `None` when it could not be worked out.
    pub label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The later of the last push delivered to it and the last time it
    /// subscribed.
    pub last_used_at: chrono::DateTime<chrono::Utc>,
    /// The SHA-256 of the push endpoint, hex: the page hashes its own
    /// endpoint to mark "this device" without the endpoint being sent back.
    pub endpoint_sha256: String,
}

/// `user_id`'s push subscriptions, most recently used first.
pub async fn list_push_devices(pool: &PgPool, user_id: &str) -> Result<Vec<PushDevice>> {
    Ok(sqlx::query_as(
        "SELECT id, device_label AS label, created_at, \
                GREATEST(last_seen_at, COALESCE(last_pushed_at, last_seen_at)) AS last_used_at, \
                encode(sha256(convert_to(endpoint, 'UTF8')), 'hex') AS endpoint_sha256 \
         FROM push_subscriptions WHERE user_id = $1 \
         ORDER BY last_used_at DESC, id DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Removes one of `user_id`'s devices from the list: no more pushes to it.
/// `false` when it is not theirs.
pub async fn delete_push_device(pool: &PgPool, user_id: &str, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM push_subscriptions WHERE user_id = $1 AND id = $2")
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// A short name for the browser and system a `User-Agent` names, such as
/// "Chrome on Android" or "Safari on iPhone", or `None` when it names
/// neither. Only this label is stored, never the header.
pub fn device_label(user_agent: &str) -> Option<String> {
    let ua = user_agent;
    let browser = if ua.contains("SamsungBrowser/") {
        Some("Samsung Internet")
    } else if ua.contains("Edg/") || ua.contains("EdgA/") || ua.contains("EdgiOS/") {
        Some("Edge")
    } else if ua.contains("OPR/") || ua.contains("Opera") {
        Some("Opera")
    } else if ua.contains("Firefox/") || ua.contains("FxiOS/") {
        Some("Firefox")
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") || ua.contains("Chromium/") {
        Some("Chrome")
    } else if ua.contains("Safari/") || ua.contains("AppleWebKit/") {
        Some("Safari")
    } else {
        None
    };
    let system = if ua.contains("iPhone") {
        Some("iPhone")
    } else if ua.contains("iPad") {
        Some("iPad")
    } else if ua.contains("Android") {
        Some("Android")
    } else if ua.contains("CrOS") {
        Some("ChromeOS")
    } else if ua.contains("Windows") {
        Some("Windows")
    } else if ua.contains("Mac OS X") || ua.contains("Macintosh") {
        Some("Mac")
    } else if ua.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    match (browser, system) {
        (Some(browser), Some(system)) => Some(format!("{browser} on {system}")),
        (Some(name), None) | (None, Some(name)) => Some(name.to_string()),
        (None, None) => None,
    }
}

/// "Turn off on this device": deletes `user_id`'s subscription for
/// `endpoint`. `false` when they have none for it (never another user's:
/// the endpoint alone does not identify the owner to the caller).
pub async fn delete_push_subscription_by_endpoint(
    pool: &PgPool,
    user_id: &str,
    endpoint: &str,
) -> Result<bool> {
    let result = sqlx::query("DELETE FROM push_subscriptions WHERE user_id = $1 AND endpoint = $2")
        .bind(user_id)
        .bind(endpoint)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod device_label_tests {
    use super::device_label;

    #[test]
    fn names_the_common_browsers_and_systems() {
        for (ua, label) in [
            (
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/129.0.0.0 Mobile Safari/537.36",
                "Chrome on Android",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
                "Safari on iPhone",
            ),
            (
                "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0",
                "Firefox on Android",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
                "Edge on Windows",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
                "Safari on Mac",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) \
                 SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
                "Samsung Internet on Android",
            ),
        ] {
            assert_eq!(device_label(ua).as_deref(), Some(label), "{ua}");
        }
        assert_eq!(device_label("curl/8.5.0"), None);
        assert_eq!(device_label(""), None);
    }
}

#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "test code: casts of small known test values"
)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed fixture user");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM push_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture subscriptions");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_same_user_resubscribe_updates_in_place -- --ignored --test-threads=1`"]
    async fn upsert_same_user_resubscribe_updates_in_place() {
        let pool = connect().await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-SAME").await;

        let first = upsert_push_subscription(
            &pool,
            "TEST-NOTIF-SUB-USER-SAME",
            "https://push.example/ep-same",
            "p256dh-a",
            "auth-a",
            None,
        )
        .await
        .expect("first insert");
        assert_eq!(first, PushSubscriptionUpsert::Saved);

        // Same user, same endpoint, refreshed keys -- the ordinary
        // browser-resubscribe case. Must update in place, not error.
        let second = upsert_push_subscription(
            &pool,
            "TEST-NOTIF-SUB-USER-SAME",
            "https://push.example/ep-same",
            "p256dh-b",
            "auth-b",
            None,
        )
        .await
        .expect("second insert (conflict path, same owner)");
        assert_eq!(second, PushSubscriptionUpsert::Saved);

        let (owner, p256dh): (String, String) =
            sqlx::query_as("SELECT user_id, p256dh FROM push_subscriptions WHERE endpoint = $1")
                .bind("https://push.example/ep-same")
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(owner, "TEST-NOTIF-SUB-USER-SAME");
        assert_eq!(
            p256dh, "p256dh-b",
            "keys should refresh on a same-user re-subscribe"
        );

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = $1")
                .bind("https://push.example/ep-same")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(
            count, 1,
            "ON CONFLICT must update in place, not insert a second row"
        );

        cleanup_user(&pool, "TEST-NOTIF-SUB-USER-SAME").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_rejects_reassigning_another_users_endpoint -- --ignored --test-threads=1`"]
    async fn upsert_rejects_reassigning_another_users_endpoint() {
        // 2026-09 security review finding: user B learning user A's real
        // endpoint URL must NOT let B silently take over A's push
        // subscription (previously: `ON CONFLICT ... DO UPDATE SET
        // user_id = EXCLUDED.user_id` reassigned it unconditionally).
        let pool = connect().await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-A2").await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-B2").await;

        let first = upsert_push_subscription(
            &pool,
            "TEST-NOTIF-SUB-USER-A2",
            "https://push.example/ep2",
            "p256dh-a",
            "auth-a",
            None,
        )
        .await
        .expect("first insert");
        assert_eq!(first, PushSubscriptionUpsert::Saved);

        // Same endpoint, a DIFFERENT user -- must be rejected, not
        // reassigned and not silently ignored either.
        let second = upsert_push_subscription(
            &pool,
            "TEST-NOTIF-SUB-USER-B2",
            "https://push.example/ep2",
            "p256dh-b",
            "auth-b",
            None,
        )
        .await
        .expect("second insert (conflict path, different owner)");
        assert_eq!(second, PushSubscriptionUpsert::EndpointOwnedByAnotherUser);

        let (owner, p256dh): (String, String) =
            sqlx::query_as("SELECT user_id, p256dh FROM push_subscriptions WHERE endpoint = $1")
                .bind("https://push.example/ep2")
                .fetch_one(&pool)
                .await
                .expect("read back after rejected conflict");
        assert_eq!(
            owner, "TEST-NOTIF-SUB-USER-A2",
            "ownership must NOT change on a rejected conflict"
        );
        assert_eq!(
            p256dh, "p256dh-a",
            "keys must NOT change on a rejected conflict either"
        );

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = $1")
                .bind("https://push.example/ep2")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(
            count, 1,
            "a rejected conflict must not insert a second row either"
        );

        cleanup_user(&pool, "TEST-NOTIF-SUB-USER-A2").await;
        cleanup_user(&pool, "TEST-NOTIF-SUB-USER-B2").await;
    }

    /// L10 (2026-09-26 review): saving a subscription past
    /// `MAX_PUSH_SUBSCRIPTIONS_PER_USER` evicts the least-recently-seen
    /// rows instead of refusing, so the device just saved always survives;
    /// a re-subscribe of an old endpoint refreshes its `last_seen_at` and so
    /// protects it from eviction.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_evicts_the_least_recently_seen_subscriptions_over_the_cap \
                -- --ignored --test-threads=1`"]
    async fn upsert_evicts_the_least_recently_seen_subscriptions_over_the_cap() {
        const USER: &str = "TEST-NOTIF-SUB-CAP";
        let pool = connect().await;
        cleanup_user(&pool, USER).await;
        seed_user(&pool, USER).await;

        // Exactly at the cap, endpoint i last seen i minutes ago (so ep-1
        // is the most recent and ep-20 the stalest).
        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, last_seen_at) \
             SELECT $1, 'https://push.example/cap-' || i, 'p', 'a', \
                    NOW() - make_interval(mins => i) \
             FROM generate_series(1, $2::int) i",
        )
        .bind(USER)
        .bind(MAX_PUSH_SUBSCRIPTIONS_PER_USER as i32)
        .execute(&pool)
        .await
        .expect("seed subscriptions up to the cap");

        let endpoints = || async {
            let rows: Vec<String> = sqlx::query_scalar(
                "SELECT endpoint FROM push_subscriptions WHERE user_id = $1 ORDER BY endpoint",
            )
            .bind(USER)
            .fetch_all(&pool)
            .await
            .expect("list endpoints");
            rows
        };

        // Re-subscribing the stalest endpoint is an update, not a new row:
        // nothing is evicted and it becomes the most recently seen.
        let refreshed = format!("https://push.example/cap-{MAX_PUSH_SUBSCRIPTIONS_PER_USER}");
        assert_eq!(
            upsert_push_subscription(&pool, USER, &refreshed, "p2", "a2", None)
                .await
                .expect("refresh"),
            PushSubscriptionUpsert::Saved
        );
        assert_eq!(
            endpoints().await.len() as i64,
            MAX_PUSH_SUBSCRIPTIONS_PER_USER
        );

        // A brand-new device: saved, and the now-stalest (cap-19) evicted.
        assert_eq!(
            upsert_push_subscription(&pool, USER, "https://push.example/cap-new", "p", "a", None)
                .await
                .expect("new device"),
            PushSubscriptionUpsert::Saved
        );
        let after = endpoints().await;
        assert_eq!(after.len() as i64, MAX_PUSH_SUBSCRIPTIONS_PER_USER);
        assert!(after.contains(&"https://push.example/cap-new".to_string()));
        assert!(
            after.contains(&refreshed),
            "a refreshed endpoint must survive"
        );
        let evicted = format!(
            "https://push.example/cap-{}",
            MAX_PUSH_SUBSCRIPTIONS_PER_USER - 1
        );
        assert!(
            !after.contains(&evicted),
            "the least-recently-seen row is evicted"
        );

        cleanup_user(&pool, USER).await;
    }
}

#[cfg(test)]
mod validate_push_endpoint_tests {
    use std::net::IpAddr;

    use common::outbound_endpoint_guard::is_disallowed_ip;

    use super::*;

    #[tokio::test]
    async fn rejects_a_plain_http_endpoint() {
        let err = validate_push_endpoint("http://93.184.216.34/ep")
            .await
            .unwrap_err();
        assert!(err.contains("https"), "unexpected message: {err}");
    }

    #[tokio::test]
    async fn rejects_a_malformed_url() {
        assert!(validate_push_endpoint("not-a-url").await.is_err());
    }

    /// The SSRF regression case named directly in the 2026-09 security
    /// review finding: a private RFC1918 endpoint (an internal service
    /// like Elasticsearch) must be rejected. IP-literal, not a hostname,
    /// so this needs no real network access to resolve -- `lookup_host`
    /// parses a numeric address locally.
    #[tokio::test]
    async fn rejects_a_private_range_endpoint() {
        let err = validate_push_endpoint("https://10.0.0.7:9200/_cluster/settings")
            .await
            .unwrap_err();
        assert!(err.contains("disallowed"), "unexpected message: {err}");
    }

    /// The other SSRF regression case the review named explicitly: a
    /// loopback endpoint must be rejected too (targeting a service bound
    /// only to the notifier's own host).
    #[tokio::test]
    async fn rejects_a_loopback_endpoint() {
        let err = validate_push_endpoint("https://127.0.0.1/ep")
            .await
            .unwrap_err();
        assert!(err.contains("disallowed"), "unexpected message: {err}");
    }

    #[tokio::test]
    async fn rejects_an_ipv6_loopback_endpoint() {
        let err = validate_push_endpoint("https://[::1]/ep")
            .await
            .unwrap_err();
        assert!(err.contains("disallowed"), "unexpected message: {err}");
    }

    /// DNS-rebinding-shaped bypass attempt: an IPv6 address that is really
    /// just an IPv4-mapped loopback address in disguise. Must be caught by
    /// the same check, not slip through the IPv6 branch unexamined.
    #[tokio::test]
    async fn rejects_an_ipv4_mapped_private_endpoint() {
        let err = validate_push_endpoint("https://[::ffff:10.0.0.7]/ep")
            .await
            .unwrap_err();
        assert!(err.contains("disallowed"), "unexpected message: {err}");
    }

    #[tokio::test]
    async fn accepts_a_well_formed_public_ip_literal_endpoint() {
        // A real, public, non-routable-to-us IP literal -- no actual
        // network reachability is asserted here (nor implied by
        // acceptance), only that the scheme+range checks don't reject it.
        validate_push_endpoint("https://93.184.216.34/ep")
            .await
            .expect("a public IPv4 literal must be accepted");
    }

    #[test]
    fn is_disallowed_ip_covers_every_named_ipv4_range() {
        for addr in [
            "10.1.2.3",
            "172.16.0.5",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "0.0.0.0",
            "192.0.2.1",
            "100.64.0.1",
        ] {
            let ip: IpAddr = addr.parse().unwrap();
            assert!(is_disallowed_ip(ip), "{addr} should be disallowed");
        }
        for addr in ["93.184.216.34", "1.1.1.1"] {
            let ip: IpAddr = addr.parse().unwrap();
            assert!(!is_disallowed_ip(ip), "{addr} should be allowed");
        }
    }

    #[test]
    fn is_disallowed_ip_covers_ipv6_ranges() {
        for addr in ["::1", "::", "ff02::1", "fc00::1", "fe80::1"] {
            let ip: IpAddr = addr.parse().unwrap();
            assert!(is_disallowed_ip(ip), "{addr} should be disallowed");
        }
        let public: IpAddr = "2606:4700:4700::1111".parse().unwrap();
        assert!(
            !is_disallowed_ip(public),
            "a public IPv6 address should be allowed"
        );
    }
}
