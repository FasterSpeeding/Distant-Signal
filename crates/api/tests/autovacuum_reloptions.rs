//! Checks that `20260926181000_autovacuum_tuning_big_batch_delete_tables.sql`
//! left its per-table autovacuum storage parameters in `pg_class.reloptions`.
//! See that migration's header for why each table gets the values it does.

use sqlx::postgres::PgPoolOptions;

const EXPECTED: &[(&str, &[&str])] = &[
    (
        "train_movement_events",
        &[
            "autovacuum_vacuum_scale_factor=0.02",
            "autovacuum_analyze_scale_factor=0.02",
        ],
    ),
    (
        "schedule_destination_departures",
        &[
            "autovacuum_vacuum_scale_factor=0.05",
            "autovacuum_vacuum_insert_scale_factor=0.05",
            "autovacuum_analyze_scale_factor=0.05",
        ],
    ),
    (
        "schedule_calling_points_full",
        &[
            "autovacuum_vacuum_scale_factor=0.05",
            "autovacuum_vacuum_insert_scale_factor=0.05",
            "autovacuum_analyze_scale_factor=0.05",
        ],
    ),
    (
        "trust_event_backlog",
        &[
            "autovacuum_vacuum_scale_factor=0.05",
            "autovacuum_analyze_scale_factor=0.05",
        ],
    ),
];

#[tokio::test]
#[ignore = "requires a live, migrated database; run with `DATABASE_URL=... cargo test -p api \
            --test autovacuum_reloptions -- --ignored --test-threads=1`"]
async fn big_batch_delete_tables_carry_their_autovacuum_reloptions() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("connect to postgres");

    for (table, expected) in EXPECTED {
        let reloptions: Option<Vec<String>> = sqlx::query_scalar(
            "SELECT reloptions FROM pg_class \
             WHERE oid = to_regclass($1) AND relkind = 'r'",
        )
        .bind(*table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|err| panic!("read reloptions for {table}: {err}"));
        let reloptions = reloptions.unwrap_or_default();

        for option in *expected {
            assert!(
                reloptions.iter().any(|actual| actual == option),
                "{table} is missing reloption {option}; has {reloptions:?}"
            );
        }
    }
}

/// `20260927140000_schedule_line_population_lz4_autovacuum.sql` (F4 / DQ8):
/// the population column compresses new values with lz4, and both the heap
/// and its TOAST table carry the 0.05 autovacuum scale factor.
#[tokio::test]
#[ignore = "requires a live, migrated database; run with `DATABASE_URL=... cargo test -p api \
            --test autovacuum_reloptions -- --ignored --test-threads=1`"]
async fn schedule_line_population_uses_lz4_and_tuned_toast_autovacuum() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("connect to postgres");

    let (heap, toast, compression): (Option<Vec<String>>, Option<Vec<String>>, String) =
        sqlx::query_as(
            "SELECT c.reloptions, t.reloptions, a.attcompression::text \
             FROM pg_class c \
             JOIN pg_class t ON t.oid = c.reltoastrelid \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'population' \
             WHERE c.oid = 'schedule_line_population'::regclass",
        )
        .fetch_one(&pool)
        .await
        .expect("read schedule_line_population storage settings");

    assert_eq!(compression, "l", "population should compress with lz4");
    let heap = heap.unwrap_or_default();
    for option in [
        "autovacuum_vacuum_scale_factor=0.05",
        "autovacuum_analyze_scale_factor=0.05",
    ] {
        assert!(
            heap.iter().any(|actual| actual == option),
            "heap missing {option}: {heap:?}"
        );
    }
    let toast = toast.unwrap_or_default();
    assert!(
        toast
            .iter()
            .any(|actual| actual == "autovacuum_vacuum_scale_factor=0.05"),
        "TOAST table missing its scale factor: {toast:?}"
    );
}
