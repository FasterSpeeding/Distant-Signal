//! Queries for user preferences: pinned lines and pinned stations. See
//! `docs/superpowers/specs/2026-07-09-frontend-personalization-design.md`.

use anyhow::Result;
use sqlx::{PgPool, Row};

pub async fn list_pinned_line_ids(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT line_id FROM pinned_lines ORDER BY pinned_at")
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("line_id")?)).collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT crs FROM pinned_stations ORDER BY pinned_at")
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

/// Replaces the entire pinned-lines set with `ids`, in one transaction
/// (delete-all then insert-all) so a PUT is atomic — concurrent readers
/// never see a partially-updated list.
pub async fn replace_pinned_lines(pool: &PgPool, ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines").execute(&mut *tx).await?;
    for id in ids {
        sqlx::query("INSERT INTO pinned_lines (line_id, pinned_at) VALUES ($1, NOW())")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`, for stations.
pub async fn replace_pinned_stations(pool: &PgPool, crs_codes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations").execute(&mut *tx).await?;
    for crs in crs_codes {
        sqlx::query("INSERT INTO pinned_stations (crs, pinned_at) VALUES ($1, NOW())")
            .bind(crs)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
