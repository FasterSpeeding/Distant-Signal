//! Queries for user preferences: pinned lines and pinned stations. See
//! `docs/superpowers/specs/2026-07-09-frontend-personalization-design.md`.

use anyhow::Result;
use sqlx::{PgPool, Row};

pub async fn list_pinned_line_ids(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows =
        sqlx::query("SELECT line_id FROM pinned_lines WHERE user_id = $1 ORDER BY pinned_at")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("line_id")?))
        .collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT crs FROM pinned_stations WHERE user_id = $1 ORDER BY pinned_at")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("crs")?))
        .collect()
}

pub async fn list_pinned_operator_codes(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT operator_code FROM pinned_operators WHERE user_id = $1 ORDER BY pinned_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("operator_code")?))
        .collect()
}

/// Filters `candidates` down to only those that exist in `stations` —
/// used to drop stale pinned-station ids on read.
pub async fn filter_existing_station_crs(
    pool: &PgPool,
    candidates: &[String],
) -> Result<Vec<String>> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query("SELECT crs FROM stations WHERE crs = ANY($1)")
        .bind(candidates)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("crs")?))
        .collect()
}

/// Replaces `user_id`'s entire pinned-lines set with `ids`, in one
/// transaction (delete-all then insert-all) so a PUT is atomic — concurrent
/// readers never see a partially-updated list. Scoped to `user_id` now, not
/// the whole table -- the pre-ownership version's `DELETE FROM pinned_lines`
/// (no predicate) would wipe every other user's pins too.
pub async fn replace_pinned_lines(pool: &PgPool, user_id: &str, ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for id in ids {
        sqlx::query(
            "INSERT INTO pinned_lines (user_id, line_id, pinned_at) VALUES ($1, $2, NOW())",
        )
        .bind(user_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`, for stations.
pub async fn replace_pinned_stations(
    pool: &PgPool,
    user_id: &str,
    crs_codes: &[String],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for crs in crs_codes {
        sqlx::query("INSERT INTO pinned_stations (user_id, crs, pinned_at) VALUES ($1, $2, NOW())")
            .bind(user_id)
            .bind(crs)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`/
/// `replace_pinned_stations` -- delete-all-then-insert-all in one
/// transaction, scoped to `user_id`.
pub async fn replace_pinned_operators(
    pool: &PgPool,
    user_id: &str,
    codes: &[String],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_operators WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for code in codes {
        sqlx::query(
            "INSERT INTO pinned_operators (user_id, operator_code, pinned_at) VALUES ($1, $2, NOW())",
        )
        .bind(user_id)
        .bind(code)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                replace_pinned_operators_then_list_pinned_operator_codes_round_trips \
                -- --ignored`"]
    async fn replace_pinned_operators_then_list_pinned_operator_codes_round_trips() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-PINNED-OPERATORS-USER', 'test@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        replace_pinned_operators(
            &pool,
            "TEST-PINNED-OPERATORS-USER",
            &["SW".to_string(), "TfL".to_string()],
        )
        .await
        .expect("pin operators");
        let codes = list_pinned_operator_codes(&pool, "TEST-PINNED-OPERATORS-USER")
            .await
            .expect("list pinned operator codes");
        assert_eq!(codes, vec!["SW".to_string(), "TfL".to_string()]);

        // A second replace fully supersedes the first set (delete-then-
        // insert, not merge).
        replace_pinned_operators(&pool, "TEST-PINNED-OPERATORS-USER", &["VT".to_string()])
            .await
            .expect("replace pinned operators");
        let codes = list_pinned_operator_codes(&pool, "TEST-PINNED-OPERATORS-USER")
            .await
            .expect("list pinned operator codes after replace");
        assert_eq!(codes, vec!["VT".to_string()]);

        sqlx::query("DELETE FROM pinned_operators WHERE user_id = 'TEST-PINNED-OPERATORS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PINNED-OPERATORS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }
}
