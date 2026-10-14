//! The delegated-access tables (migration `20261014200000`): grants, the
//! consent page's confirmation tokens, the record of delegated deletes and
//! the hourly sweep. See `crate::auth::delegated` and
//! docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md §5.3.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// How long the consent page's confirmation token lives (§5.3).
pub const CONFIRMATION_TTL_MINUTES: i64 = 10;
/// How long a `delegated_deletions` row is kept (§5.5a).
pub const DELETION_RETENTION_DAYS: i64 = 30;
/// How long a revoked grant row is kept, as the record of when and why the
/// access ended (§5.11), before the sweep deletes it.
pub const REVOKED_GRANT_RETENTION_DAYS: i64 = 90;
/// How long an `Idempotency-Key` is remembered (§5.8).
pub const IDEMPOTENCY_RETENTION_HOURS: i64 = 24;
/// How long the "an app was connected" banner shows (§5.3).
pub const NEW_GRANT_NOTICE_DAYS: i64 = 7;

/// Why a grant ended (`delegated_grants.revoked_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    /// Disconnect on the Connected apps page.
    User,
    /// `POST /public/admin/users/revoke-sessions`.
    Admin,
    /// Authentik back-channel logout on the `distant-signal` provider (D-7
    /// path 1).
    Backchannel,
    /// `POST /delegated-grants/revoke` from the client's service identity
    /// (D-7 path 2).
    ClientLogout,
    /// 90 days unused (D-6).
    Inactive,
    /// A new consent replaced it.
    Replaced,
}

impl RevokeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RevokeReason::User => "user",
            RevokeReason::Admin => "admin",
            RevokeReason::Backchannel => "backchannel",
            RevokeReason::ClientLogout => "client_logout",
            RevokeReason::Inactive => "inactive",
            RevokeReason::Replaced => "replaced",
        }
    }
}

/// What the extractor needs to decide a delegated request.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActiveGrant {
    pub id: i64,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub not_before: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub sessions_invalidated_at: Option<DateTime<Utc>>,
}

/// The active grant `user_id` holds for `client`, with the user's
/// `sessions_invalidated_at`. `None` for no grant, or no such user.
pub async fn active_grant(
    pool: &PgPool,
    user_id: &str,
    client: &str,
) -> Result<Option<ActiveGrant>> {
    Ok(sqlx::query_as::<_, ActiveGrant>(
        "SELECT g.id, g.scopes, g.created_at, g.not_before, g.last_used_at, \
                u.sessions_invalidated_at \
         FROM delegated_grants g JOIN users u ON u.id = g.user_id \
         WHERE g.user_id = $1 AND g.client = $2 AND g.revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(client)
    .fetch_optional(pool)
    .await?)
}

/// Records a use, at most once a minute per grant (§5.7 step 5).
pub async fn touch_grant(pool: &PgPool, grant_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE delegated_grants SET last_used_at = now() \
         WHERE id = $1 AND revoked_at IS NULL \
           AND (last_used_at IS NULL OR last_used_at < now() - INTERVAL '1 minute')",
    )
    .bind(grant_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Revokes one grant, if still active.
pub async fn revoke_grant(pool: &PgPool, grant_id: i64, reason: RevokeReason) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE delegated_grants SET revoked_at = now(), revoked_reason = $2 \
         WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(grant_id)
    .bind(reason.as_str())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Revokes `user_id`'s active grants: for one client, or for every client
/// when `client` is `None`. Returns how many it revoked.
pub async fn revoke_grants_for_user<'c, E>(
    executor: E,
    user_id: &str,
    client: Option<&str>,
    reason: RevokeReason,
) -> Result<u64>
where
    E: sqlx::PgExecutor<'c>,
{
    let result = sqlx::query(
        "UPDATE delegated_grants SET revoked_at = now(), revoked_reason = $3 \
         WHERE user_id = $1 AND ($2::text IS NULL OR client = $2) AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(client)
    .bind(reason.as_str())
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

/// A grant as the Connected apps page and the account export show it.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRow {
    pub id: i64,
    pub client: String,
    pub client_display_name: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// `user_id`'s active grants, newest first.
pub async fn list_active_grants(pool: &PgPool, user_id: &str) -> Result<Vec<GrantRow>> {
    Ok(sqlx::query_as::<_, GrantRow>(
        "SELECT id, client, client_display_name, scopes, created_at, last_used_at \
         FROM delegated_grants WHERE user_id = $1 AND revoked_at IS NULL \
         ORDER BY created_at DESC, id DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// The newest active grant made in the last [`NEW_GRANT_NOTICE_DAYS`], for
/// the banner.
pub async fn recent_grant(pool: &PgPool, user_id: &str) -> Result<Option<GrantRow>> {
    Ok(sqlx::query_as::<_, GrantRow>(
        "SELECT id, client, client_display_name, scopes, created_at, last_used_at \
         FROM delegated_grants \
         WHERE user_id = $1 AND revoked_at IS NULL \
           AND created_at > now() - make_interval(days => $2) \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(i32::try_from(NEW_GRANT_NOTICE_DAYS)?)
    .fetch_optional(pool)
    .await?)
}

/// Issues the consent page's single-use confirmation token for this
/// session and client, replacing any earlier one. Returns the raw token
/// (sent to the page, stored only hashed) and its expiry.
pub async fn issue_confirmation(
    pool: &PgPool,
    session_id: &str,
    user_id: &str,
    client: &str,
) -> Result<(String, DateTime<Utc>)> {
    let token = crate::auth::generate_session_token();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM delegated_grant_confirmations WHERE session_id = $1 AND client = $2")
        .bind(session_id)
        .bind(client)
        .execute(&mut *tx)
        .await?;
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO delegated_grant_confirmations \
             (token_hash, session_id, user_id, client, expires_at) \
         VALUES ($1, $2, $3, $4, now() + make_interval(mins => $5)) RETURNING expires_at",
    )
    .bind(crate::auth::hash_session_token(&token))
    .bind(session_id)
    .bind(user_id)
    .bind(client)
    .bind(i32::try_from(CONFIRMATION_TTL_MINUTES)?)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((token, expires_at))
}

/// What [`create_grant`] did.
#[derive(Debug)]
pub enum CreateGrantOutcome {
    Created(GrantRow),
    /// No matching, unexpired, unused confirmation token for this session
    /// and client: 403 `confirmation_required`.
    ConfirmationRequired,
}

/// Consumes the confirmation token, revokes any active grant for the
/// client (`replaced`) and records the new one, in one transaction. The
/// new row's `not_before` is now, so tokens minted before this consent are
/// refused.
pub async fn create_grant(
    pool: &PgPool,
    session_id: &str,
    user_id: &str,
    client: &str,
    client_display_name: &str,
    scopes: &[String],
    confirmation_token: &str,
) -> Result<CreateGrantOutcome> {
    let mut tx = pool.begin().await?;
    let consumed: Option<i32> = sqlx::query_scalar(
        "DELETE FROM delegated_grant_confirmations \
         WHERE token_hash = $1 AND session_id = $2 AND user_id = $3 AND client = $4 \
           AND expires_at > now() \
         RETURNING 1",
    )
    .bind(crate::auth::hash_session_token(confirmation_token))
    .bind(session_id)
    .bind(user_id)
    .bind(client)
    .fetch_optional(&mut *tx)
    .await?;
    if consumed.is_none() {
        tx.rollback().await?;
        return Ok(CreateGrantOutcome::ConfirmationRequired);
    }
    revoke_grants_for_user(&mut *tx, user_id, Some(client), RevokeReason::Replaced).await?;
    let grant = sqlx::query_as::<_, GrantRow>(
        "INSERT INTO delegated_grants (user_id, client, client_display_name, scopes) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, client, client_display_name, scopes, created_at, last_used_at",
    )
    .bind(user_id)
    .bind(client)
    .bind(client_display_name)
    .bind(scopes)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(CreateGrantOutcome::Created(grant))
}

/// What [`claim_idempotency_key`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum IdempotencyClaim {
    /// The key is new (or older than 24 hours): go ahead, and
    /// [`record_idempotent_response`] on the same transaction.
    Fresh,
    /// The same key and the same request: answer with this stored response.
    Replay(serde_json::Value),
    /// The same key with a different request: 409 `idempotency_conflict`.
    Conflict,
}

/// Claims `(user_id, key)` on the caller's transaction (spec 2026-10-14
/// §5.8). The row is inserted now and its response filled in by
/// [`record_idempotent_response`] before the same transaction commits, so a
/// concurrent request with the same key waits on the insert and then sees
/// the finished response; a rolled-back attempt leaves no key behind.
pub async fn claim_idempotency_key(
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    key: &str,
    request_hash: &str,
) -> Result<IdempotencyClaim> {
    sqlx::query(
        "DELETE FROM journey_idempotency_keys \
         WHERE user_id = $1 AND idempotency_key = $2 \
           AND created_at < now() - make_interval(hours => $3)",
    )
    .bind(user_id)
    .bind(key)
    .bind(i32::try_from(IDEMPOTENCY_RETENTION_HOURS)?)
    .execute(&mut *tx)
    .await?;
    let inserted: Option<i32> = sqlx::query_scalar(
        "INSERT INTO journey_idempotency_keys (user_id, idempotency_key, request_hash) \
         VALUES ($1, $2, $3) ON CONFLICT (user_id, idempotency_key) DO NOTHING RETURNING 1",
    )
    .bind(user_id)
    .bind(key)
    .bind(request_hash)
    .fetch_optional(&mut *tx)
    .await?;
    if inserted.is_some() {
        return Ok(IdempotencyClaim::Fresh);
    }
    let existing: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT request_hash, response FROM journey_idempotency_keys \
         WHERE user_id = $1 AND idempotency_key = $2",
    )
    .bind(user_id)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(match existing {
        Some((hash, Some(response))) if hash == request_hash => IdempotencyClaim::Replay(response),
        // A different body, or (unreachable while claim and response commit
        // together) a row with no response yet.
        _ => IdempotencyClaim::Conflict,
    })
}

/// Stores the response for a key [`claim_idempotency_key`] claimed.
pub async fn record_idempotent_response(
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    key: &str,
    response: &serde_json::Value,
) -> Result<()> {
    sqlx::query(
        "UPDATE journey_idempotency_keys SET response = $3 \
         WHERE user_id = $1 AND idempotency_key = $2",
    )
    .bind(user_id)
    .bind(key)
    .bind(response)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// What [`delete_journey_delegated`] did (spec 2026-10-14 §5.5a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegatedDeleteOutcome {
    /// Deleted, and recorded in `delegated_deletions`.
    Deleted,
    /// Not the caller's, or not created by this client: 404 `not_found`.
    NotFound,
    /// Shared into a group or through an active link: 409 `journey_shared`.
    Shared,
    /// The user's delegated deletes in the last 24 hours reached the cap:
    /// 429 `daily_limit`, retry after this many seconds.
    DailyLimit { retry_after_secs: i64 },
}

/// A delegated delete under rules 1 to 4 of §5.5a, in one transaction: the
/// journey row is locked, ownership, `created_via` and "not shared" are
/// checked under that lock, the user's deletes in the last 24 hours are
/// counted (under a per-user advisory lock, so two concurrent deletes
/// cannot both pass the cap), and the delete runs exactly as
/// `journeys::delete_journey` does (template tombstone included), with a
/// `delegated_deletions` row recording what went.
pub async fn delete_journey_delegated(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    client: &str,
    grant_id: i64,
    daily_limit: i64,
) -> Result<DelegatedDeleteOutcome> {
    let mut tx = pool.begin().await?;
    let row: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT custom_name, created_via FROM journeys WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((custom_name, created_via)) = row else {
        return Ok(DelegatedDeleteOutcome::NotFound);
    };
    if created_via != client {
        return Ok(DelegatedDeleteOutcome::NotFound);
    }
    let shared: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM group_journeys WHERE journey_id = $1) \
             OR EXISTS (SELECT 1 FROM unlisted_links \
                        WHERE resource_type = 'journey' AND resource_id = $1::text \
                          AND revoked_at IS NULL \
                          AND (expires_at IS NULL OR expires_at > now()))",
    )
    .bind(journey_id)
    .fetch_one(&mut *tx)
    .await?;
    if shared {
        return Ok(DelegatedDeleteOutcome::Shared);
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('delegated_delete:' || $1, 0))")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let (recent, oldest): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT COUNT(*), MIN(deleted_at) FROM delegated_deletions \
         WHERE user_id = $1 AND deleted_at > now() - INTERVAL '24 hours'",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if recent >= daily_limit {
        let frees_at = oldest.map_or_else(Utc::now, |oldest| oldest + chrono::Duration::hours(24));
        let retry_after_secs = (frees_at - Utc::now()).num_seconds().max(1);
        return Ok(DelegatedDeleteOutcome::DailyLimit { retry_after_secs });
    }
    let legs: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object( \
             'serviceDate', jl.service_date, 'originCrs', jl.origin_crs, \
             'destinationCrs', jl.destination_crs, 'trainUid', tr.train_uid) \
             ORDER BY jl.leg_order), '[]'::jsonb) \
         FROM journey_legs jl \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         WHERE jl.journey_id = $1",
    )
    .bind(journey_id)
    .fetch_one(&mut *tx)
    .await?;
    let summary = serde_json::json!({ "customName": custom_name, "legs": legs });
    crate::data::journeys::record_template_occurrence_skips(&mut tx, journey_id).await?;
    sqlx::query("DELETE FROM journeys WHERE id = $1 AND user_id = $2")
        .bind(journey_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO delegated_deletions (user_id, client, grant_id, journey_id, summary) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(client)
    .bind(grant_id)
    .bind(journey_id)
    .bind(&summary)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(DelegatedDeleteOutcome::Deleted)
}

/// A delegated delete, as the Connected apps page lists it.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionRow {
    pub id: i64,
    pub client: String,
    pub journey_id: i64,
    pub deleted_at: DateTime<Utc>,
    pub summary: serde_json::Value,
}

/// `user_id`'s delegated deletes of the last [`DELETION_RETENTION_DAYS`],
/// newest first.
pub async fn recent_deletions(pool: &PgPool, user_id: &str) -> Result<Vec<DeletionRow>> {
    Ok(sqlx::query_as::<_, DeletionRow>(
        "SELECT id, client, journey_id, deleted_at, summary FROM delegated_deletions \
         WHERE user_id = $1 AND deleted_at > now() - make_interval(days => $2) \
         ORDER BY deleted_at DESC, id DESC LIMIT 200",
    )
    .bind(user_id)
    .bind(i32::try_from(DELETION_RETENTION_DAYS)?)
    .fetch_all(pool)
    .await?)
}

/// What one sweep did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepOutcome {
    pub inactive_grants: u64,
    pub revoked_grants_purged: u64,
    pub deletions_purged: u64,
    pub idempotency_keys_purged: u64,
    pub confirmations_purged: u64,
}

impl SweepOutcome {
    pub fn total(self) -> u64 {
        self.inactive_grants
            + self.revoked_grants_purged
            + self.deletions_purged
            + self.idempotency_keys_purged
            + self.confirmations_purged
    }
}

/// The hourly pass (§5.3, §5.5a): grants unused for `idle_days` are
/// revoked (`inactive`); grants revoked more than 90 days ago,
/// `delegated_deletions` rows past 30 days, expired idempotency keys and
/// confirmation tokens are deleted. Idempotent.
pub async fn sweep(pool: &PgPool, now: DateTime<Utc>, idle_days: i64) -> Result<SweepOutcome> {
    let inactive_grants = sqlx::query(
        "UPDATE delegated_grants SET revoked_at = $1, revoked_reason = 'inactive' \
         WHERE revoked_at IS NULL \
           AND COALESCE(last_used_at, created_at) < $1 - make_interval(days => $2)",
    )
    .bind(now)
    .bind(i32::try_from(idle_days)?)
    .execute(pool)
    .await?
    .rows_affected();
    let revoked_grants_purged = sqlx::query(
        "DELETE FROM delegated_grants \
         WHERE revoked_at IS NOT NULL AND revoked_at < $1 - make_interval(days => $2)",
    )
    .bind(now)
    .bind(i32::try_from(REVOKED_GRANT_RETENTION_DAYS)?)
    .execute(pool)
    .await?
    .rows_affected();
    let deletions_purged = sqlx::query(
        "DELETE FROM delegated_deletions WHERE deleted_at < $1 - make_interval(days => $2)",
    )
    .bind(now)
    .bind(i32::try_from(DELETION_RETENTION_DAYS)?)
    .execute(pool)
    .await?
    .rows_affected();
    let idempotency_keys_purged = sqlx::query(
        "DELETE FROM journey_idempotency_keys WHERE created_at < $1 - make_interval(hours => $2)",
    )
    .bind(now)
    .bind(i32::try_from(IDEMPOTENCY_RETENTION_HOURS)?)
    .execute(pool)
    .await?
    .rows_affected();
    let confirmations_purged =
        sqlx::query("DELETE FROM delegated_grant_confirmations WHERE expires_at < $1")
            .bind(now)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(SweepOutcome {
        inactive_grants,
        revoked_grants_purged,
        deletions_purged,
        idempotency_keys_purged,
        confirmations_purged,
    })
}

/// [`sweep`] with [`crate::auth::delegated::GRANT_IDLE_DAYS`], logged, and
/// the `delegated_grants_active` gauge refreshed. Shared by the api's
/// hourly loop and the `maintenance` binary. `None` when the sweep failed
/// (already logged), for the binary's exit status.
pub async fn run_sweep(pool: &PgPool, now: DateTime<Utc>) -> Option<SweepOutcome> {
    let outcome = match sweep(pool, now, crate::auth::delegated::GRANT_IDLE_DAYS).await {
        Ok(outcome) => outcome,
        Err(err) => {
            tracing::error!(error = ?err, "delegated-access sweep failed; will retry next interval");
            return None;
        }
    };
    if outcome.inactive_grants > 0 {
        tracing::warn!(
            audit = true,
            event = "delegated_grant_revoked",
            reason = RevokeReason::Inactive.as_str(),
            count = outcome.inactive_grants,
            "grants unused for 90 days were revoked"
        );
    }
    if outcome.total() > 0 {
        tracing::info!(?outcome, "delegated-access sweep pruned expired rows");
    }
    match active_grants_by_client(pool).await {
        Ok(rows) => {
            for (client, count) in rows {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a grant count is far below 2^52"
                )]
                metrics::gauge!(
                    common::metrics::metric_name("api_delegated_grants_active"),
                    "client" => client
                )
                .set(count as f64);
            }
        }
        Err(err) => tracing::warn!(error = ?err, "could not count active grants"),
    }
    Some(outcome)
}

/// Active grants per client, for `distant_signal_api_delegated_grants_active`.
pub async fn active_grants_by_client(pool: &PgPool) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT client, COUNT(*) FROM delegated_grants WHERE revoked_at IS NULL GROUP BY client",
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_match_the_check_constraint() {
        let migration =
            include_str!("../../../ds-store/migrations/20261014200000_delegated_access.sql");
        for reason in [
            RevokeReason::User,
            RevokeReason::Admin,
            RevokeReason::Backchannel,
            RevokeReason::ClientLogout,
            RevokeReason::Inactive,
            RevokeReason::Replaced,
        ] {
            assert!(
                migration.contains(&format!("'{}'", reason.as_str())),
                "{} is not allowed by delegated_grants.revoked_reason's CHECK",
                reason.as_str()
            );
        }
    }
}
