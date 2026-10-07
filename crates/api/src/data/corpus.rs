//! Network Rail CORPUS location reference data (`corpus_locations`,
//! `corpus_deliveries`), loaded by `schedule-ingest` through
//! `POST /private/corpus-locations`. See
//! `crates/api/migrations/20260928100000_corpus_locations.sql` and
//! docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md.
//!
//! Every load also rebuilds the CORPUS-derived crosswalk in the same
//! transaction ([`crate::data::corpus_crosswalk`]), which only the
//! off-by-default lookup fallback reads, and the route logs a comparison
//! against the timetable crosswalk ([`crate::data::corpus_comparison`]).

// Moved to `ds_store::corpus` (ingest architecture plan 1A.6).
pub use ds_store::corpus::{
    CorpusLocation, DeliveredFileProvenance, LAST_DELIVERY_METRIC, last_corpus_delivery,
    refresh_last_delivery_metric, replace_corpus_locations,
    replace_corpus_locations_with_provenance,
};

/// Database-gated: each test gets its own throwaway database
/// (`#[sqlx::test]`, migrated from `./migrations`), because a load replaces
/// the WHOLE table and must never run against a shared database that may
/// hold a real extract (see `crate::test_support`'s module doc). Needs a
/// `DATABASE_URL` whose role may create databases.
#[cfg(test)]
#[expect(
    clippy::items_after_statements,
    reason = "test code: fixtures sit next to their use"
)]
mod db_tests {
    use chrono::{DateTime, TimeZone, Utc};
    use sqlx::PgPool;

    use super::*;

    fn location(nlc: &str, tiploc: Option<&str>, crs: Option<&str>) -> CorpusLocation {
        CorpusLocation {
            nlc: nlc.to_string(),
            stanox: Some("87219".to_string()),
            tiploc: tiploc.map(str::to_string),
            crs: crs.map(str::to_string),
            uic: None,
            nlc_desc: Some("CLAPHAM JUNCTION LONDON".to_string()),
            nlc_desc16: None,
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn a_load_replaces_the_whole_set_and_records_the_delivery(pool: PgPool) {
        let first = Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap();
        let second = Utc.with_ymd_and_hms(2026, 10, 1, 3, 0, 0).unwrap();
        assert_eq!(last_corpus_delivery(&pool).await.unwrap(), None);

        let n = replace_corpus_locations(
            &pool,
            first,
            "CORPUSExtract.json.gz",
            &[
                location("559500", Some("CLPHMJN"), Some("CLJ")),
                location("559569", Some("CLPHMJC"), None),
                location("000800", None, None),
            ],
        )
        .await
        .unwrap();
        assert_eq!(n, 3);

        let n = replace_corpus_locations(
            &pool,
            second,
            "CORPUSExtract.json.gz",
            &[location("559500", Some("CLPHMJN"), Some("CLJ"))],
        )
        .await
        .unwrap();
        assert_eq!(n, 1);

        type Row = (String, Option<String>, Option<String>, DateTime<Utc>);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT nlc, tiploc, crs, delivered_at FROM corpus_locations ORDER BY nlc",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![(
                "559500".to_string(),
                Some("CLPHMJN".to_string()),
                Some("CLJ".to_string()),
                second
            )]
        );
        assert_eq!(last_corpus_delivery(&pool).await.unwrap(), Some(second));
        let (deliveries,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM corpus_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(deliveries, 2);
    }

    /// Loading the same delivery twice (a restart between the load and the
    /// file's archive move) is idempotent.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn reloading_the_same_delivery_is_idempotent(pool: PgPool) {
        let at = Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap();
        let rows = [location("559500", Some("CLPHMJN"), Some("CLJ"))];
        for _ in 0..2 {
            replace_corpus_locations(&pool, at, "CORPUSExtract.json.gz", &rows)
                .await
                .unwrap();
        }
        let (locations, deliveries): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM corpus_locations), (SELECT COUNT(*) FROM corpus_deliveries)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((locations, deliveries), (1, 1));
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn an_empty_load_is_refused_and_keeps_the_current_set(pool: PgPool) {
        let at = Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap();
        replace_corpus_locations(
            &pool,
            at,
            "CORPUSExtract.json.gz",
            &[location("559500", Some("CLPHMJN"), Some("CLJ"))],
        )
        .await
        .unwrap();
        assert!(
            replace_corpus_locations(&pool, at, "CORPUSExtract.json.gz", &[])
                .await
                .is_err()
        );
        let (locations,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM corpus_locations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(locations, 1);
    }

    /// The delivered file's size and SHA-256 land in `corpus_deliveries`;
    /// the provenance-less wrapper leaves them NULL.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn a_load_records_the_delivered_file_provenance(pool: PgPool) {
        let at = Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap();
        let sha = "0f".repeat(32);
        let rows = [location("559500", Some("CLPHMJN"), Some("CLJ"))];
        replace_corpus_locations_with_provenance(
            &pool,
            at,
            "CORPUSExtract.json.gz",
            &DeliveredFileProvenance {
                bytes: Some(295_957),
                sha256: Some(&sha),
            },
            &rows,
        )
        .await
        .unwrap();
        let read = || async {
            let row: (Option<i64>, Option<String>) =
                sqlx::query_as("SELECT source_bytes, sha256 FROM corpus_deliveries")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            row
        };
        assert_eq!(read().await, (Some(295_957), Some(sha.clone())));

        replace_corpus_locations(&pool, at, "CORPUSExtract.json.gz", &rows)
            .await
            .unwrap();
        assert_eq!(read().await, (None, None));
    }
}
