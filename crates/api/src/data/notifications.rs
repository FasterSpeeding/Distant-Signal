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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::Result;
use sqlx::PgPool;

/// Rejects any `endpoint` a malicious (or merely careless) caller could use
/// to make [`crates/notifier/src/send.rs`]'s later VAPID-signed `POST`
/// land somewhere inside the cluster's own network instead of at a real
/// push service -- an SSRF vector, since that POST is issued server-side
/// on every notification, using whatever URL this function let through.
///
/// Two checks, both required:
///   1. Scheme must be `https` -- the Push API never issues a plain-`http`
///      endpoint, and requiring TLS also means the host below is at least
///      nominally reachable as a real internet service.
///   2. The host must resolve -- via a REAL DNS lookup, not a string
///      pattern match on the hostname -- to only public IP addresses.
///      Resolving is deliberate, not merely parsing an IP literal out of
///      the URL: a hostname like `attacker.example` can resolve to
///      `10.0.0.7` just as easily as an IP-literal URL can name it
///      directly, and a string check on the HOSTNAME alone (e.g. "does it
///      look like `10.x.x.x`") would miss that entirely -- exactly the
///      DNS-rebinding gap this function is written to close. Every IPv4
///      and IPv6 private/loopback/link-local/multicast (and a few other
///      non-public) range is rejected; see [`is_disallowed_ip`].
///
/// Deliberately NOT also restricted to a small allowlist of known
/// push-service hosts (Mozilla autopush's `updates.push.services.
/// mozilla.com`, FCM's `fcm.googleapis.com`, Apple's `web.push.apple.com`,
/// etc.) -- that list is neither small nor stable in practice. Browsers
/// that route push through microG/UnifiedPush distributors (common on
/// de-Googled Android) or self-hosted relays (ntfy, Gotify, a
/// self-hosted UnifiedPush distributor) legitimately use arbitrary
/// operator-chosen hosts, so a hard allowlist would silently break real
/// subscriptions for exactly the privacy-conscious users this app should
/// not be turning away. The scheme + real-DNS-resolved-private-range check
/// above is judged sufficient on its own.
pub async fn validate_push_endpoint(endpoint: &str) -> Result<(), String> {
    let url = url::Url::parse(endpoint).map_err(|_| "endpoint must be a valid URL".to_string())?;

    if url.scheme() != "https" {
        return Err("endpoint must use https".to_string());
    }

    let host = url
        .host()
        .ok_or_else(|| "endpoint must have a host".to_string())?
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(443);

    // `url::Host` is matched on directly (an already-typed IPv4/IPv6
    // literal needs no resolution at all -- and skipping `lookup_host` for
    // those avoids any ambiguity around whether the OS resolver treats an
    // IPv6 literal's `[...]` bracket syntax as part of the hostname). A
    // domain name gets a REAL async DNS resolution, not a hostname string
    // match -- see this function's own doc comment on why that distinction
    // matters (DNS rebinding).
    let addrs: Vec<IpAddr> = match host {
        url::Host::Ipv4(v4) => vec![IpAddr::V4(v4)],
        url::Host::Ipv6(v6) => vec![IpAddr::V6(v6)],
        url::Host::Domain(domain) => tokio::net::lookup_host((domain.as_str(), port))
            .await
            .map_err(|_| "endpoint host does not resolve".to_string())?
            .map(|socket_addr| socket_addr.ip())
            .collect(),
    };
    if addrs.is_empty() {
        return Err("endpoint host does not resolve".to_string());
    }

    if addrs.iter().any(|addr| is_disallowed_ip(*addr)) {
        return Err("endpoint host resolves to a disallowed address".to_string());
    }
    Ok(())
}

/// Every IPv4 and IPv6 non-public range worth rejecting an SSRF-candidate
/// URL over. An IPv6 address that is really an IPv4-mapped address
/// (`::ffff:a.b.c.d`) is unwrapped and re-checked against the IPv4 rules
/// below rather than sailing through the IPv6 branch unexamined -- the
/// same "attacker picks the representation that evades the filter" concern
/// [`validate_push_endpoint`]'s own doc comment raises about DNS
/// rebinding, applied to address FORM instead of resolution timing.
fn is_disallowed_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_disallowed_ipv4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(mapped) => is_disallowed_ipv4(mapped),
            None => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || is_unique_local_v6(v6)
                    || is_link_local_v6(v6)
            }
        },
    }
}

fn is_disallowed_ipv4(v4: Ipv4Addr) -> bool {
    v4.is_private() // RFC1918: 10/8, 172.16/12, 192.168/16
        || v4.is_loopback() // 127/8
        || v4.is_link_local() // 169.254/16
        || v4.is_multicast()
        || v4.is_broadcast()
        || v4.is_unspecified() // 0.0.0.0
        || v4.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || is_carrier_grade_nat_v4(v4) // 100.64/10 (RFC6598)
}

/// `std::net::Ipv4Addr` has no stable `is_shared` yet -- this is that
/// range's own check, kept as a tiny standalone function rather than an
/// inline expression so its RFC citation has somewhere to live.
fn is_carrier_grade_nat_v4(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    a == 100 && (64..=127).contains(&b) // 100.64.0.0/10
}

/// fc00::/7 -- IPv6 Unique Local Addresses (RFC4193), the IPv6 rough
/// equivalent of RFC1918.
fn is_unique_local_v6(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// fe80::/10 -- IPv6 link-local (RFC4291).
fn is_link_local_v6(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
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
/// still updates in place) or "someone else's endpoint got POSTed by this
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
) -> Result<PushSubscriptionUpsert> {
    let result = sqlx::query(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, NOW(), NOW()) \
         ON CONFLICT (endpoint) DO UPDATE SET \
           p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth, last_seen_at = NOW() \
         WHERE push_subscriptions.user_id = EXCLUDED.user_id",
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        Ok(PushSubscriptionUpsert::EndpointOwnedByAnotherUser)
    } else {
        Ok(PushSubscriptionUpsert::Saved)
    }
}

#[cfg(test)]
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
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
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
                upsert_same_user_resubscribe_updates_in_place -- --ignored`"]
    async fn upsert_same_user_resubscribe_updates_in_place() {
        let pool = connect().await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-SAME").await;

        let first = upsert_push_subscription(
            &pool,
            "TEST-NOTIF-SUB-USER-SAME",
            "https://push.example/ep-same",
            "p256dh-a",
            "auth-a",
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
                upsert_rejects_reassigning_another_users_endpoint -- --ignored`"]
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
}

#[cfg(test)]
mod validate_push_endpoint_tests {
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
