//! Queries for user preferences: pinned lines and pinned stations. See
//! `docs/superpowers/specs/2026-07-09-frontend-personalization-design.md`.

use anyhow::Result;
use sqlx::{PgPool, Row};

pub async fn list_pinned_line_ids(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT line_id FROM pinned_lines WHERE user_id = $1 ORDER BY pinned_at")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("line_id")?)).collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT crs FROM pinned_stations WHERE user_id = $1 ORDER BY pinned_at")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("crs")?)).collect()
}

/// Filters `candidates` down to only those that exist in `stations` —
/// used to drop stale pinned-station ids on read.
pub async fn filter_existing_station_crs(pool: &PgPool, candidates: &[String]) -> Result<Vec<String>> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query("SELECT crs FROM stations WHERE crs = ANY($1)")
        .bind(candidates)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("crs")?)).collect()
}

/// Replaces `user_id`'s entire pinned-lines set with `ids`, in one
/// transaction (delete-all then insert-all) so a PUT is atomic — concurrent
/// readers never see a partially-updated list. Scoped to `user_id` now, not
/// the whole table -- the pre-ownership version's `DELETE FROM pinned_lines`
/// (no predicate) would wipe every other user's pins too.
pub async fn replace_pinned_lines(pool: &PgPool, user_id: &str, ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    for id in ids {
        sqlx::query("INSERT INTO pinned_lines (user_id, line_id, pinned_at) VALUES ($1, $2, NOW())")
            .bind(user_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`, for stations.
pub async fn replace_pinned_stations(pool: &PgPool, user_id: &str, crs_codes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
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
