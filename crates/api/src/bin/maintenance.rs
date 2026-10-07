//! `maintenance`: ONE pass of the api's user-data sweeps, then exit (spec
//! R3 and §12.3, plan 1B.8). The chart's hourly `api-maintenance` `CronJob`
//! runs it from the api image, as the api role:
//!
//! ```text
//!   DATABASE_URL=postgres://... maintenance
//! ```
//!
//! The pass is what `main.rs`'s `session_cleanup_sweep_loop` does each
//! hour, calling the same functions with the same settings:
//!
//! 1. `data::users::prune_expired_sessions`;
//! 2. `data::unlisted_links::prune_dead_links`;
//! 3. `data::retention::prune_personal_data`, with the retention limits
//!    from the api's own variables (`PAST_TRAVEL_RETENTION_DAYS`,
//!    `STALE_PUSH_SUBSCRIPTION_DAYS`, `INACTIVE_ACCOUNT_RETENTION_DAYS`,
//!    same defaults).
//!
//! Each step runs even if an earlier one failed, as in the loop. The exit
//! status is 0 only if all three succeeded, so a failing step fails the
//! Job. Every step is idempotent. The loop stays in the api, behind
//! `API_BACKGROUND_LOOPS`, until the `CronJob` is on.

use clap::Parser;
use common::secret::Secret;

use api::data::retention::{RetentionOutcome, RetentionPolicy};

/// `pg_stat_activity.application_name`.
const APPLICATION_NAME: &str = "distant-signal-api-maintenance";
/// The steps run one after another, so one connection is enough; the
/// second is headroom. Overridable with `DATABASE_MAX_CONNECTIONS`.
const DEFAULT_MAX_CONNECTIONS: u32 = 2;

/// The api's variables for these sweeps, same names and defaults as
/// `ServiceArguments` (checked by `the_settings_match_the_api_server`).
#[derive(Debug, Parser)]
#[command(name = "maintenance")]
struct Args {
    #[arg(long, env, hide_env_values = true)]
    database_url: Secret,
    #[arg(long, env, default_value_t = 548)]
    past_travel_retention_days: i64,
    #[arg(long, env, default_value_t = 365)]
    stale_push_subscription_days: i64,
    #[arg(long, env, default_value_t = 0)]
    inactive_account_retention_days: i64,
}

impl Args {
    fn retention_policy(&self) -> RetentionPolicy {
        RetentionPolicy {
            past_travel_days: self.past_travel_retention_days,
            stale_push_subscription_days: self.stale_push_subscription_days,
            inactive_account_days: self.inactive_account_retention_days,
        }
    }
}

/// What one pass did. A step that failed has `None` and is named in
/// `failed`.
#[derive(Debug, Default)]
struct PassReport {
    expired_sessions: Option<u64>,
    dead_links: Option<u64>,
    personal_data: Option<RetentionOutcome>,
    failed: Vec<&'static str>,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    common::logging::init_with_filter(
        "api-maintenance",
        common::logging::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| common::logging::EnvFilter::new("info")),
    );
    let pool = common::pg::PoolSettings::from_env(APPLICATION_NAME, DEFAULT_MAX_CONNECTIONS)?
        .connect(args.database_url.expose())
        .await?;
    let report = run_pass(&pool, args.retention_policy(), chrono::Utc::now()).await;
    pool.close().await;
    tracing::info!(
        expired_sessions = report.expired_sessions,
        dead_links = report.dead_links,
        personal_data = ?report.personal_data,
        failed = ?report.failed,
        "maintenance pass finished"
    );
    anyhow::ensure!(
        report.failed.is_empty(),
        "maintenance steps failed: {:?}",
        report.failed
    );
    Ok(())
}

/// The three sweeps, in the loop's order, each logged and attempted
/// whatever the others did.
async fn run_pass(
    pool: &sqlx::PgPool,
    policy: RetentionPolicy,
    now: chrono::DateTime<chrono::Utc>,
) -> PassReport {
    let mut report = PassReport::default();
    match api::data::users::prune_expired_sessions(pool).await {
        Ok(deleted) => report.expired_sessions = Some(deleted),
        Err(err) => {
            tracing::error!(error = ?err, "expired-session prune failed");
            report.failed.push("expired_sessions");
        }
    }
    match api::data::unlisted_links::prune_dead_links(pool).await {
        Ok(deleted) => report.dead_links = Some(deleted),
        Err(err) => {
            tracing::error!(error = ?err, "dead-link prune failed");
            report.failed.push("dead_links");
        }
    }
    match api::data::retention::prune_personal_data(pool, policy, now).await {
        Ok(outcome) => report.personal_data = Some(outcome),
        Err(err) => {
            tracing::error!(error = ?err, "personal-data retention sweep failed");
            report.failed.push("personal_data");
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    /// The `CronJob` gets the api's values (plan 1B.8), so each setting must
    /// read the same variable with the same default as the server's.
    #[test]
    fn the_settings_match_the_api_server() {
        let ours = Args::command();
        let server = api::data::config::ServiceArguments::command();
        for id in [
            "database_url",
            "past_travel_retention_days",
            "stale_push_subscription_days",
            "inactive_account_retention_days",
        ] {
            let find = |command: &clap::Command| {
                let arg = command
                    .get_arguments()
                    .find(|arg| arg.get_id() == id)
                    .unwrap_or_else(|| panic!("no {id}"));
                (
                    arg.get_env().map(std::ffi::OsStr::to_os_string),
                    arg.get_default_values().to_vec(),
                )
            };
            assert_eq!(find(&ours), find(&server), "{id}");
        }
    }

    async fn count(pool: &sqlx::PgPool, sql: &str, key: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(key)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn cleanup(pool: &sqlx::PgPool, user: &str) {
        for sql in [
            "DELETE FROM sessions WHERE user_id = $1",
            "DELETE FROM unlisted_links WHERE created_by = $1",
            "DELETE FROM train_subscriptions WHERE user_id = $1",
            "DELETE FROM users WHERE id = $1",
        ] {
            sqlx::query(sql).bind(user).execute(pool).await.unwrap();
        }
    }

    /// One pass prunes what the hourly loop prunes: an expired session, a
    /// link dead for over 30 days and past travel beyond the retention,
    /// keeping their live counterparts.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api --bin maintenance \
                -- --ignored --test-threads=1`"]
    async fn one_pass_prunes_what_the_loop_prunes() {
        const USER: &str = "TEST-API-MAINTENANCE";
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::PgPool::connect(&database_url).await.unwrap();
        cleanup(&pool, USER).await;
        sqlx::query("INSERT INTO users (id, email, name) VALUES ($1, $2, $1)")
            .bind(USER)
            .bind(format!("{USER}@example.com"))
            .execute(&pool)
            .await
            .unwrap();
        for (id, expires) in [
            ("test-maintenance-expired", "NOW() - INTERVAL '1 day'"),
            ("test-maintenance-live", "NOW() + INTERVAL '1 day'"),
        ] {
            sqlx::query(&format!(
                "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
                 VALUES ($1, $2, NULL, NOW() - INTERVAL '2 days', {expires})"
            ))
            .bind(id)
            .bind(USER)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (token, revoked_days_ago) in [
            ("test-maintenance-dead", Some(31)),
            ("test-maintenance-link", None),
        ] {
            sqlx::query(
                "INSERT INTO unlisted_links \
                    (token_hash, resource_type, resource_id, created_by, revoked_at) \
                 VALUES ($1, 'widget-maintenance', $1, $2, NOW() - make_interval(days => $3))",
            )
            .bind(token)
            .bind(USER)
            .bind(revoked_days_ago)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (days_ago, label) in [(600, "old"), (0, "new")] {
            sqlx::query(
                "INSERT INTO train_subscriptions \
                    (user_id, service_date, resolution_status, custom_name) \
                 VALUES ($1, CURRENT_DATE - $2::int, 'pending', $3)",
            )
            .bind(USER)
            .bind(days_ago)
            .bind(label)
            .execute(&pool)
            .await
            .unwrap();
        }

        // The defaults minus the push-subscription limit, so the pass
        // touches nothing but this test's rows and other tests' leftovers.
        let policy = RetentionPolicy {
            past_travel_days: 548,
            stale_push_subscription_days: 0,
            inactive_account_days: 0,
        };
        let report = run_pass(&pool, policy, chrono::Utc::now()).await;
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(report.expired_sessions >= Some(1), "{report:?}");
        assert!(report.dead_links >= Some(1), "{report:?}");
        assert!(
            report
                .personal_data
                .as_ref()
                .is_some_and(|outcome| outcome.tracked_trains >= 1),
            "{report:?}"
        );

        let sessions = "SELECT COUNT(*) FROM sessions WHERE user_id = $1";
        let links = "SELECT COUNT(*) FROM unlisted_links WHERE created_by = $1";
        let trains = "SELECT COUNT(*) FROM train_subscriptions WHERE user_id = $1";
        assert_eq!(count(&pool, sessions, USER).await, 1, "the live session");
        assert_eq!(count(&pool, links, USER).await, 1, "the live link");
        assert_eq!(count(&pool, trains, USER).await, 1, "today's train");
        let kept: String =
            sqlx::query_scalar("SELECT custom_name FROM train_subscriptions WHERE user_id = $1")
                .bind(USER)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kept, "new");

        // Idempotent: a second pass finds nothing more of ours.
        let again = run_pass(&pool, policy, chrono::Utc::now()).await;
        assert!(again.failed.is_empty(), "{again:?}");
        assert_eq!(count(&pool, sessions, USER).await, 1);
        assert_eq!(count(&pool, links, USER).await, 1);
        assert_eq!(count(&pool, trains, USER).await, 1);

        cleanup(&pool, USER).await;
    }
}
