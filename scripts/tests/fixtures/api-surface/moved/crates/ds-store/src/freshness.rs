//! Fixture: the ingest functions after the move into ds-store, reformatted.

pub const FETCH_METRIC: &str = "api_fetch_total";

pub async fn record_ingest<'a>(pool: &'a Pool, source: &str) -> Result<()> {
    let quote = '"';
    let escaped = '\'';
    metrics::counter!(
        common::metrics::metric_name(FETCH_METRIC),
        "source" => source
    )
    .increment(1);
    sqlx::query(r#"INSERT INTO ingest_log (source, "at") VALUES ($1, NOW())"#)
        .bind(source)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn last_fetch(pool: &Pool) -> Result<Option<i64>> {
    sqlx::query_scalar("SELECT max(at) FROM ingest_log")
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn seeds() {
        let _ = "INSERT INTO stations (crs) VALUES ('KGX')";
        let _ = include_str!("../../api/migrations/0001_init.sql");
        metrics::counter!("api_test_only_total").increment(1);
    }
}
