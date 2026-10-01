//! Applies the `full_coverage_line_stats` re-key migrations
//! (`20260927050000`-`20260927050200`) to a table that ALREADY HAS ROWS --
//! the production case -- rather than to the empty table a fresh
//! `sqlx migrate run` sees.
//!
//! Runs in a throwaway schema (the table's original DDL, then these three
//! files, exactly as sqlx would: the transactional ones inside a
//! transaction, the `-- no-transaction` one outside), so it never touches
//! the shared dev database's real table.

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, Row};

/// Embedded at compile time instead of read from `env!("CARGO_MANIFEST_DIR")`
/// at run time: with `CARGO_TARGET_DIR` shared between worktrees, cargo can
/// reuse a binary built in another worktree, which would then read (or fail
/// to find) THAT worktree's files (Train Register verification 2026-10-01,
/// N6). `include_str!` also makes cargo rebuild this test whenever one of
/// these files changes.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "20260904100000_full_coverage_line_stats.sql",
        include_str!("../migrations/20260904100000_full_coverage_line_stats.sql"),
    ),
    (
        "20260927050000_full_coverage_line_stats_partial.sql",
        include_str!("../migrations/20260927050000_full_coverage_line_stats_partial.sql"),
    ),
    (
        "20260927050100_full_coverage_line_stats_line_date_key.sql",
        include_str!("../migrations/20260927050100_full_coverage_line_stats_line_date_key.sql"),
    ),
    (
        "20260927050200_full_coverage_line_stats_line_date_pkey.sql",
        include_str!("../migrations/20260927050200_full_coverage_line_stats_line_date_pkey.sql"),
    ),
];

fn migration(name: &str) -> &'static str {
    MIGRATIONS
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, sql)| *sql)
        .unwrap_or_else(|| panic!("{name} is not embedded in MIGRATIONS"))
}

async fn apply(conn: &mut sqlx::PgConnection, name: &str) {
    let sql = migration(name);
    if sql.starts_with("-- no-transaction") {
        sqlx::raw_sql(sql)
            .execute(&mut *conn)
            .await
            .unwrap_or_else(|err| panic!("apply {name}: {err}"));
    } else {
        let mut tx = conn.begin().await.unwrap();
        sqlx::raw_sql(sql)
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|err| panic!("apply {name}: {err}"));
        tx.commit().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api --test \
            full_coverage_line_stats_history_migration -- --ignored --test-threads=1`"]
async fn the_rekey_migrations_apply_to_a_table_that_already_has_rows() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let schema = format!(
        "fcls_migration_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("connect to postgres");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();

    let options: PgConnectOptions = database_url.parse().unwrap();
    let mut conn = sqlx::PgConnection::connect_with(&options.options([("search_path", &schema)]))
        .await
        .unwrap();

    // The table as production has it before these migrations, with rows.
    apply(&mut conn, "20260904100000_full_coverage_line_stats.sql").await;
    sqlx::raw_sql(
        "INSERT INTO full_coverage_line_stats
             (line_id, service_date, availability, total, delayed, cancelled, skipped, avg_delay_minutes)
         VALUES ('waterloo-reading', '2026-09-27', 'pending', 40, 3, 30, 0, 1.5),
                ('old-line', '2026-09-22', 'available', 12, 1, 2, 0, 0.5)",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    for name in [
        "20260927050000_full_coverage_line_stats_partial.sql",
        "20260927050100_full_coverage_line_stats_line_date_key.sql",
        "20260927050200_full_coverage_line_stats_line_date_pkey.sql",
    ] {
        apply(&mut conn, name).await;
    }

    // Existing rows survive, keep their dates, and are marked partial.
    let rows = sqlx::query(
        "SELECT line_id, service_date, availability, cancelled, partial
         FROM full_coverage_line_stats ORDER BY line_id",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(
            row.get::<bool, _>("partial"),
            "pre-existing rows cannot be trusted complete"
        );
    }
    assert_eq!(rows[1].get::<String, _>("line_id"), "waterloo-reading");
    assert_eq!(
        rows[1].get::<chrono::NaiveDate, _>("service_date"),
        "2026-09-27".parse::<chrono::NaiveDate>().unwrap()
    );
    assert_eq!(rows[1].get::<i32, _>("cancelled"), 30);

    // The primary key is now (line_id, service_date), under the old name,
    // and the step-2 index was adopted rather than left as a duplicate.
    let key_columns: Vec<String> = sqlx::query_scalar(
        "SELECT a.attname::text
         FROM pg_index i
         JOIN pg_class c ON c.oid = i.indexrelid
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
         WHERE i.indrelid = 'full_coverage_line_stats'::regclass AND i.indisprimary
           AND c.relname = 'full_coverage_line_stats_pkey'
         ORDER BY array_position(i.indkey::int2[], a.attnum)",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(key_columns, vec!["line_id", "service_date"]);
    let index_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_index WHERE indrelid = 'full_coverage_line_stats'::regclass",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(index_count, 1, "no leftover duplicate unique index");

    // A second day for the same line is now a second row; new rows default
    // to not partial; the same (line, day) is still unique.
    sqlx::raw_sql(
        "INSERT INTO full_coverage_line_stats (line_id, service_date, availability)
         VALUES ('waterloo-reading', '2026-09-28', 'pending')",
    )
    .execute(&mut conn)
    .await
    .expect("a new day for an existing line");
    let partial: bool = sqlx::query_scalar(
        "SELECT partial FROM full_coverage_line_stats
         WHERE line_id = 'waterloo-reading' AND service_date = '2026-09-28'",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert!(!partial);
    let duplicate = sqlx::raw_sql(
        "INSERT INTO full_coverage_line_stats (line_id, service_date, availability)
         VALUES ('waterloo-reading', '2026-09-28', 'pending')",
    )
    .execute(&mut conn)
    .await;
    assert!(duplicate.is_err(), "(line_id, service_date) stays unique");

    drop(conn);
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
