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
            --test autovacuum_reloptions -- --ignored`"]
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
