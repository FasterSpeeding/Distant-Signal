//! Fixture for scripts/tests/test_diff_api_surface.py: the api before a move.
//! A comment's SQL is not a literal: SELECT nothing FROM comments.

/* A block comment /* nested */ "DELETE FROM not_a_literal" */

pub const FETCH_METRIC: &str = "api_fetch_total";

pub async fn record_ingest<'a>(pool: &'a Pool, source: &str) -> Result<()> {
    let quote = '"';
    let escaped = '\'';
    metrics::counter!(common::metrics::metric_name(FETCH_METRIC), "source" => source).increment(1);
    sqlx::query(
        r#"INSERT INTO ingest_log (source, "at")
           VALUES ($1, NOW())"#,
    )
    .bind(source)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn last_fetch(pool: &Pool) -> Result<Option<i64>> {
    sqlx::query_scalar("SELECT max(at) \
                        FROM ingest_log")
        .fetch_one(pool)
        .await
}

pub fn public_reader() -> &'static str {
    metrics::gauge!("api_reader_up").set(1.0);
    "SELECT id FROM stations WHERE crs = $1"
}

#[cfg(test)]
mod tests {
    #[test]
    fn seeds() {
        let _ = "INSERT INTO stations (crs) VALUES ('KGX')";
        let _ = include_str!("../../migrations/0001_init.sql");
        metrics::counter!("api_test_only_total").increment(1);
    }
}
