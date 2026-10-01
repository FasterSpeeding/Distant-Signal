//! Shared fixture hygiene for this crate's database-gated (`#[ignore]`)
//! tests -- DB review 2026-09-27 B1/B3, decision DQ16.
//!
//! Those tests share ONE database (CI runs them against a single migrated
//! database with `--test-threads=1`; a developer runs them against a local
//! one that may also hold real reference data). The rules this module backs:
//!
//! * clean up BEFORE seeding, not only after, so a run that panicked
//!   half-way (and so never reached its trailing cleanup) cannot poison the
//!   next run;
//! * clean up again on `Drop`, so a failing assertion no longer leaks its
//!   fixtures into every later test;
//! * only ever delete synthetic data: every statement must carry a `WHERE`
//!   clause naming the test's own fixture keys (digit-bearing CRS codes,
//!   `TEST-`/fixture uid prefixes, far-future dates), never a whole table or
//!   a whole real day.
//!
//! New test modules should prefer `#[sqlx::test]` (a throwaway database per
//! test) instead; this is for the existing shared-database modules.

use sqlx::{Connection, PgConnection, PgPool};

/// URL for the few database-gated tests that need the schema owner's rights
/// (DDL: creating and dropping tables and indexes, running the migrator):
/// `MIGRATION_DATABASE_URL` when set and not blank, else `DATABASE_URL`.
///
/// With the role split (docs/postgres-app-role.md) the suite runs with
/// `DATABASE_URL` as the non-superuser app role, which only has DML, and
/// `MIGRATION_DATABASE_URL` as the owner role -- exactly as in production,
/// where only `api::migrate` uses the owner. Without it both are the same
/// (super)user, as before.
pub(crate) fn owner_database_url() -> String {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let migration_database_url = std::env::var(crate::migrate::MIGRATION_DATABASE_URL_ENV).ok();
    crate::migrate::migration_url(&database_url, migration_database_url.as_deref())
        .0
        .to_owned()
}

/// Fixture dates must be at least this far in the future, so a date-scoped
/// delete can never touch a real published day. See [`assert_synthetic_date`].
pub(crate) const FIRST_SYNTHETIC_YEAR: i32 = 2050;

/// Panics unless `date` is a far-future fixture date. Every test helper that
/// deletes "everything on `date`" calls this first: the tables involved
/// (`schedule_destination_departures`, `schedule_calling_points_full`, ...)
/// are replaced a whole `service_date` at a time by the code under test, so
/// such a test necessarily owns its whole day -- which is only safe when
/// that day is synthetic.
pub(crate) fn assert_synthetic_date(date: chrono::NaiveDate) {
    use chrono::Datelike;
    assert!(
        date.year() >= FIRST_SYNTHETIC_YEAR,
        "refusing a date-scoped test delete on {date}: fixture dates must be in \
         {FIRST_SYNTHETIC_YEAR} or later so they can never be real data"
    );
}

/// A set of scoped `DELETE`s run once when created (before the test seeds
/// anything) and again when dropped (after the test, pass or fail).
///
/// Hold it in a named binding for the whole test (`let _cleanup = ...`, not
/// `let _ = ...`, which drops it immediately).
pub(crate) struct FixtureCleanup {
    database_url: String,
    statements: Vec<String>,
}

impl FixtureCleanup {
    /// Runs `statements` against `pool` now and remembers them for `Drop`.
    ///
    /// Each statement must be a single scoped `DELETE ... WHERE ...` with
    /// its fixture keys inlined as literals (they are test constants).
    pub(crate) async fn new<S: Into<String>>(
        pool: &PgPool,
        statements: impl IntoIterator<Item = S>,
    ) -> Self {
        let statements: Vec<String> = statements.into_iter().map(Into::into).collect();
        for statement in &statements {
            let upper = statement.to_ascii_uppercase();
            assert!(
                upper.trim_start().starts_with("DELETE FROM ") && upper.contains(" WHERE "),
                "fixture cleanup must be a scoped DELETE ... WHERE, never a whole table: \
                 {statement}"
            );
        }
        run_all(pool, &statements).await;
        Self {
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set to run this test"),
            statements,
        }
    }
}

async fn run_all(pool: &PgPool, statements: &[String]) {
    for statement in statements {
        sqlx::query(statement)
            .execute(pool)
            .await
            .unwrap_or_else(|err| panic!("fixture cleanup failed: {statement}: {err}"));
    }
}

impl Drop for FixtureCleanup {
    fn drop(&mut self) {
        // `Drop` cannot await, and the test's own runtime may be the
        // current-thread flavour that is busy dropping us, so run the
        // deletes on a fresh thread with its own runtime and connection.
        let database_url = std::mem::take(&mut self.database_url);
        let statements = std::mem::take(&mut self.statements);
        let outcome = std::thread::spawn(move || -> Result<(), String> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| err.to_string())?;
            runtime.block_on(async move {
                let mut conn = PgConnection::connect(&database_url)
                    .await
                    .map_err(|err| err.to_string())?;
                for statement in &statements {
                    sqlx::query(statement)
                        .execute(&mut conn)
                        .await
                        .map_err(|err| format!("{statement}: {err}"))?;
                }
                conn.close().await.map_err(|err| err.to_string())
            })
        })
        .join();
        let problem = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(err)) => err,
            Err(_) => "cleanup thread panicked".to_string(),
        };
        // Never panic while the test itself is already unwinding (that
        // aborts the whole test binary); the next run's up-front cleanup
        // covers anything left behind. A passing test, though, should not
        // silently leave fixtures behind.
        if std::thread::panicking() {
            eprintln!("fixture cleanup on drop failed: {problem}");
        } else {
            panic!("fixture cleanup on drop failed: {problem}");
        }
    }
}

/// The fabricated train UIDs the journeys and train route/data tests create
/// `trains` rows for (through `find_or_create_train` and the known-train
/// leg paths), on the service date they use. Those tests cleaned up their
/// users, journeys and subscriptions but never the `trains` row itself, so
/// every run left ~30 of them behind (Train Register verification
/// 2026-10-01, "Newly found" 1).
///
/// 2026-09-22 is a real date, so this names each UID rather than clearing
/// the day. `TEST-TRAIN-...` and `SHARE1`-style UIDs cannot be real (a CIF
/// UID is a letter and five digits), and the repeated-digit ones
/// (`A11111`, ...) are fixtures only these tests use.
const FIXTURE_TRAIN_UIDS_2026_09_22: &[&str] = &[
    "A11111", "A22222", "A33333", "A44444", "A55555", "A66666", "A77777", "A88888", "A99999",
    "D11111", "D22222", "D33333", "D44444", "E11111", "E22222", "E33333", "E44444", "SGC001",
    "SHARE1", "SHARE2", "SHARE3", "SHARE4", "SHARE5", "SHARE6",
];

/// Deletes the `trains` rows of [`FIXTURE_TRAIN_UIDS_2026_09_22`] (plus any
/// `TEST-TRAIN-%` row on that day) and the far-future `CTCHG1`/`CTCHG2`
/// change-train fixtures, now and again on drop. Their subscriptions and
/// legs go with them (`ON DELETE CASCADE` / `SET NULL`).
pub(crate) async fn fixture_trains_cleanup(pool: &PgPool) -> FixtureCleanup {
    let uids = FIXTURE_TRAIN_UIDS_2026_09_22
        .iter()
        .map(|uid| format!("'{uid}'"))
        .collect::<Vec<_>>()
        .join(", ");
    FixtureCleanup::new(
        pool,
        [
            format!(
                "DELETE FROM trains WHERE service_date = '2026-09-22' \
                 AND (train_uid IN ({uids}) OR train_uid LIKE 'TEST-TRAIN-%')"
            ),
            "DELETE FROM trains WHERE service_date = '2099-04-17' \
             AND train_uid IN ('CTCHG1', 'CTCHG2')"
                .to_string(),
        ],
    )
    .await
}
