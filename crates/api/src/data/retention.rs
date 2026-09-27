//! Storage-limitation pruning of personal data (UK legal audit LEG-5,
//! 2026-09-27; UK GDPR Art. 5(1)(e)).
//!
//! Run hourly from `main.rs`'s `session_cleanup_sweep_loop`, beside the
//! existing expired-session and dead-link prunes. Every limit is a config
//! value (`ServiceArguments::past_travel_retention_days`,
//! `stale_push_subscription_days`, `inactive_account_retention_days`) and 0
//! disables that part. See `docs/personal-data-retention.md` for the full
//! schedule, including the parts pruned elsewhere (sessions, OIDC login
//! state, dead share/invite links).
//!
//! Deliberately NOT pruned by age: anything a user expects to keep while
//! they use the account -- journey templates (they recur), groups and
//! memberships, pins/preferences, custom lines, and upcoming or recent
//! travel.

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::account;

/// The configured limits, in days. 0 disables that limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Tracked trains, standalone tickets, journeys and template skip
    /// markers whose travel date is more than this many days ago.
    pub past_travel_days: i64,
    /// Push subscriptions of users who have not logged in for this many
    /// days (and whose subscription was not renewed in that time either).
    pub stale_push_subscription_days: i64,
    /// Whole accounts with no login for this many days and no live session.
    pub inactive_account_days: i64,
}

/// Rows deleted by one [`prune_personal_data`] run, for logging and tests.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct RetentionOutcome {
    pub journeys: u64,
    pub tracked_trains: u64,
    pub standalone_tickets: u64,
    pub template_skipped_dates: u64,
    pub push_subscriptions: u64,
    pub accounts: u64,
}

impl RetentionOutcome {
    pub fn total(&self) -> u64 {
        self.journeys
            + self.tracked_trains
            + self.standalone_tickets
            + self.template_skipped_dates
            + self.push_subscriptions
            + self.accounts
    }
}

/// How many inactive accounts one run deletes at most, so a first run
/// against a large backlog does not hold one long sweep. The rest go on the
/// next run.
const MAX_ACCOUNTS_PER_RUN: i64 = 100;

/// The London service date `days` before `now`'s own London date.
fn cutoff_date(now: DateTime<Utc>, days: i64) -> NaiveDate {
    now.with_timezone(&chrono_tz::Europe::London).date_naive() - Duration::days(days)
}

/// Applies `policy` as of `now`. Each part runs as its own statement(s);
/// one failing aborts the rest of this run, and the next hourly run
/// retries everything (every part is idempotent).
pub async fn prune_personal_data(
    pool: &PgPool,
    policy: RetentionPolicy,
    now: DateTime<Utc>,
) -> Result<RetentionOutcome> {
    let mut outcome = RetentionOutcome::default();

    if policy.past_travel_days > 0 {
        let date = cutoff_date(now, policy.past_travel_days);
        let instant = now - Duration::days(policy.past_travel_days);
        prune_past_travel(pool, date, instant, &mut outcome).await?;
    }

    if policy.stale_push_subscription_days > 0 {
        let cutoff = now - Duration::days(policy.stale_push_subscription_days);
        outcome.push_subscriptions = sqlx::query(
            "DELETE FROM push_subscriptions p USING users u \
             WHERE p.user_id = u.id AND u.last_login_at < $1 AND p.last_seen_at < $1",
        )
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected();
    }

    if policy.inactive_account_days > 0 {
        let cutoff = now - Duration::days(policy.inactive_account_days);
        let inactive: Vec<String> = sqlx::query_scalar(
            "SELECT u.id FROM users u \
             WHERE u.last_login_at < $1 \
               AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.expires_at > $2) \
             ORDER BY u.last_login_at \
             LIMIT $3",
        )
        .bind(cutoff)
        .bind(now)
        .bind(MAX_ACCOUNTS_PER_RUN)
        .fetch_all(pool)
        .await?;
        for user_id in inactive {
            // Same path as self-service deletion, so group ownership is
            // handed over rather than the group vanishing.
            if account::delete_account(pool, &user_id).await?.is_some() {
                outcome.accounts += 1;
            }
        }
    }

    Ok(outcome)
}

async fn prune_past_travel(
    pool: &PgPool,
    date: NaiveDate,
    instant: DateTime<Utc>,
    outcome: &mut RetentionOutcome,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    // A journey is past when it has no leg on or after the cutoff date. A
    // leg-less journey (only ever transient, mid-creation) is judged by
    // its creation time instead. Its share links go with it: they are
    // polymorphic rows with no FK to journeys.
    let doomed: Vec<i64> = sqlx::query_scalar(
        "SELECT j.id FROM journeys j \
         WHERE j.created_at < $2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM journey_legs l WHERE l.journey_id = j.id AND l.service_date >= $1)",
    )
    .bind(date)
    .bind(instant)
    .fetch_all(&mut *tx)
    .await?;
    if !doomed.is_empty() {
        sqlx::query(
            "DELETE FROM unlisted_links \
             WHERE resource_type = 'journey' AND resource_id = ANY($1::bigint[]::text[])",
        )
        .bind(&doomed)
        .execute(&mut *tx)
        .await?;
        outcome.journeys = sqlx::query("DELETE FROM journeys WHERE id = ANY($1)")
            .bind(&doomed)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }

    // Any surviving journey leg still pointing at a train about to go is
    // marked unmatched, as `train_tracking::delete_tracked_train` does; the
    // FK then sets its `train_subscription_id` to NULL. Tickets attached to
    // the train, its notification state and its group shares cascade.
    sqlx::query(
        "UPDATE journey_legs SET match_mode = 'unmatched' \
         WHERE train_subscription_id IN (SELECT id FROM train_subscriptions WHERE service_date < $1)",
    )
    .bind(date)
    .execute(&mut *tx)
    .await?;
    outcome.tracked_trains = sqlx::query("DELETE FROM train_subscriptions WHERE service_date < $1")
        .bind(date)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Tickets not attached to any tracked train: judged by their departure
    // date when known, else by when they were added.
    outcome.standalone_tickets = sqlx::query(
        "DELETE FROM tracked_train_tickets \
         WHERE tracked_train_id IS NULL AND COALESCE(current_departure_date, created_at) < $1",
    )
    .bind(instant)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // "Not this date" markers on templates only ever matter for today (the
    // notifier's recurrence sweep mints only today's occurrence).
    outcome.template_skipped_dates =
        sqlx::query("DELETE FROM journey_template_skipped_dates WHERE service_date < $1")
            .bind(date)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_date_uses_the_london_date() {
        // 23:30 UTC on 2026-06-30 is already 1 July in London (BST).
        let now = "2026-06-30T23:30:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            cutoff_date(now, 1),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()
        );
    }

    #[test]
    fn outcome_total_sums_every_part() {
        let outcome = RetentionOutcome {
            journeys: 1,
            tracked_trains: 2,
            standalone_tickets: 3,
            template_skipped_dates: 4,
            push_subscriptions: 5,
            accounts: 6,
        };
        assert_eq!(outcome.total(), 21);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::data::account::db_tests::{cleanup, connect, seed_every_feature, seed_user};

    async fn count(pool: &PgPool, sql: &str, user: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(user)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|err| panic!("{sql}: {err}"))
    }

    const OFF: RetentionPolicy = RetentionPolicy {
        past_travel_days: 0,
        stale_push_subscription_days: 0,
        inactive_account_days: 0,
    };

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored`"]
    async fn past_travel_is_pruned_and_recent_travel_and_templates_are_kept() {
        let pool = connect().await;
        let user = "ret-travel-user";
        cleanup(&pool, "ret-travel-").await;
        seed_user(&pool, user).await;

        // Old: a tracked train 600 days ago with an attached ticket, a
        // standalone ticket for then, a journey whose only leg was then
        // (with a share link), and a template skip marker for then.
        // Recent: the same set dated today.
        for (days_ago, label) in [(600, "old"), (0, "new")] {
            sqlx::query(
                "INSERT INTO train_subscriptions (user_id, service_date, resolution_status, custom_name) \
                 VALUES ($1, CURRENT_DATE - $2::int, 'pending', $3)",
            )
            .bind(user)
            .bind(days_ago)
            .bind(label)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO tracked_train_tickets (tracked_train_id, user_id, source) \
                 SELECT id, $1, 'manual' FROM train_subscriptions WHERE user_id = $1 AND custom_name = $2",
            )
            .bind(user)
            .bind(label)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO tracked_train_tickets (user_id, source, custom_name, created_at, current_departure_date) \
                 VALUES ($1, 'manual', $2, NOW() - make_interval(days => $3), NOW() - make_interval(days => $3))",
            )
            .bind(user)
            .bind(label)
            .bind(days_ago)
            .execute(&pool)
            .await
            .unwrap();
            let journey: i64 = sqlx::query_scalar(
                "INSERT INTO journeys (user_id, custom_name, created_at) \
                 VALUES ($1, $2, NOW() - make_interval(days => $3)) RETURNING id",
            )
            .bind(user)
            .bind(label)
            .bind(days_ago)
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO journey_legs (journey_id, leg_order, service_date) \
                 VALUES ($1, 0, CURRENT_DATE - $2::int)",
            )
            .bind(journey)
            .bind(days_ago)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO unlisted_links (token_hash, resource_type, resource_id, created_by) \
                 VALUES ($1, 'journey', $2, $3)",
            )
            .bind(format!("ret-travel-{label}"))
            .bind(journey.to_string())
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        }
        let template: i64 = sqlx::query_scalar(
            "INSERT INTO journey_templates (user_id, custom_name, created_at) \
             VALUES ($1, 'Old commute', NOW() - INTERVAL '900 days') RETURNING id",
        )
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO journey_template_skipped_dates (template_id, service_date) \
             VALUES ($1, CURRENT_DATE - 600), ($1, CURRENT_DATE)",
        )
        .bind(template)
        .execute(&pool)
        .await
        .unwrap();

        // Disabled: nothing happens.
        let outcome = prune_personal_data(&pool, OFF, Utc::now()).await.unwrap();
        assert_eq!(outcome.total(), 0);

        let policy = RetentionPolicy {
            past_travel_days: 548,
            ..OFF
        };
        let outcome = prune_personal_data(&pool, policy, Utc::now())
            .await
            .unwrap();
        assert!(outcome.journeys >= 1 && outcome.tracked_trains >= 1);
        assert!(outcome.standalone_tickets >= 1 && outcome.template_skipped_dates >= 1);

        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM train_subscriptions WHERE user_id = $1",
                user
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM tracked_train_tickets WHERE user_id = $1",
                user
            )
            .await,
            2,
            "the new attached ticket and the new standalone ticket survive"
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM journeys WHERE user_id = $1 AND custom_name = 'new'",
                user
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM journeys WHERE user_id = $1",
                user
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM unlisted_links WHERE created_by = $1",
                user
            )
            .await,
            1,
            "the old journey's share link went with it"
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM journey_templates WHERE user_id = $1",
                user
            )
            .await,
            1,
            "templates are kept for the life of the account"
        );
        assert_eq!(
            count(
                &pool,
                "SELECT COUNT(*) FROM journey_template_skipped_dates s JOIN journey_templates t ON t.id = s.template_id WHERE t.user_id = $1",
                user
            )
            .await,
            1
        );

        // Idempotent.
        let again = prune_personal_data(&pool, policy, Utc::now())
            .await
            .unwrap();
        assert_eq!(
            again.journeys + again.tracked_trains + again.standalone_tickets,
            0
        );
        cleanup(&pool, "ret-travel-").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored`"]
    async fn stale_push_subscriptions_are_pruned_only_for_long_absent_users() {
        let pool = connect().await;
        cleanup(&pool, "ret-push-").await;
        for (user, last_login_days_ago) in [("ret-push-away", 400), ("ret-push-here", 1)] {
            seed_user(&pool, user).await;
            sqlx::query(
                "UPDATE users SET last_login_at = NOW() - make_interval(days => $2) WHERE id = $1",
            )
            .bind(user)
            .bind(last_login_days_ago)
            .execute(&pool)
            .await
            .unwrap();
            // Both subscriptions are old: only the absent user's goes.
            sqlx::query(
                "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at, last_seen_at) \
                 VALUES ($1, 'https://push.example/' || $1, 'k', 'a', NOW() - INTERVAL '500 days', NOW() - INTERVAL '500 days')",
            )
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        }
        let policy = RetentionPolicy {
            stale_push_subscription_days: 365,
            ..OFF
        };
        let outcome = prune_personal_data(&pool, policy, Utc::now())
            .await
            .unwrap();
        assert!(outcome.push_subscriptions >= 1);
        let sql = "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1";
        assert_eq!(count(&pool, sql, "ret-push-away").await, 0);
        assert_eq!(count(&pool, sql, "ret-push-here").await, 1);
        cleanup(&pool, "ret-push-").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored`"]
    async fn inactive_accounts_are_deleted_only_when_enabled_and_without_a_live_session() {
        let pool = connect().await;
        cleanup(&pool, "ret-inact-").await;
        let (gone, other) = ("ret-inact-gone", "ret-inact-other");
        seed_every_feature(&pool, gone, other).await;
        seed_user(&pool, "ret-inact-session").await;
        sqlx::query(
            "UPDATE users SET last_login_at = NOW() - INTERVAL '800 days' WHERE id IN ($1, 'ret-inact-session')",
        )
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();
        // `gone`'s seeded session is expired; `ret-inact-session` has a live one.
        sqlx::query("UPDATE sessions SET expires_at = NOW() - INTERVAL '1 day' WHERE user_id = $1")
            .bind(gone)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, expires_at) VALUES ('ret-inact-live', 'ret-inact-session', NOW() + INTERVAL '1 day')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Off by default: nothing is deleted.
        prune_personal_data(&pool, OFF, Utc::now()).await.unwrap();
        let users = "SELECT COUNT(*) FROM users WHERE id = $1";
        assert_eq!(count(&pool, users, gone).await, 1);

        let policy = RetentionPolicy {
            inactive_account_days: 730,
            ..OFF
        };
        let outcome = prune_personal_data(&pool, policy, Utc::now())
            .await
            .unwrap();
        assert!(outcome.accounts >= 1);
        assert_eq!(count(&pool, users, gone).await, 0);
        assert_eq!(
            count(&pool, users, other).await,
            1,
            "recently active user kept"
        );
        assert_eq!(
            count(&pool, users, "ret-inact-session").await,
            1,
            "live session kept"
        );
        // Group ownership handed over exactly as for self-service deletion.
        let role: String = sqlx::query_scalar(
            "SELECT role FROM group_members WHERE group_id = 'ret-inact-gone-g-shared' AND user_id = $1",
        )
        .bind(other)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(role, "owner");
        cleanup(&pool, "ret-inact-").await;
    }
}
