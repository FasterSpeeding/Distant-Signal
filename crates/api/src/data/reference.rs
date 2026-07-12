//! Read-only type-ahead search over the `stations`/`tocs` reference
//! tables. See
//! docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md.
//!
//! Uses runtime-checked `sqlx::query_as` rather than the `query_as!`
//! macro family, matching `queries.rs`'s established rationale: the
//! macros need a live DB or a checked-in `.sqlx` cache at compile time,
//! which this workspace deliberately doesn't carry.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Suggestion {
    pub code: String,
    pub name: String,
}

/// Matches `q` as a case-insensitive substring of either the CRS code or
/// the station name. `q` must already be trimmed and non-empty (callers
/// go through `routes::reference::sanitize_query` first).
pub async fn search_stations(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let pattern = format!("%{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT crs AS code, name FROM stations \
         WHERE crs ILIKE $1 OR name ILIKE $1 \
         ORDER BY name LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Matches `q` as a case-insensitive substring of either the ATOC code or
/// the operator name. Same trimmed/non-empty contract as
/// [`search_stations`].
pub async fn search_tocs(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let pattern = format!("%{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT atoc_code AS code, name FROM tocs \
         WHERE atoc_code ILIKE $1 OR name ILIKE $1 \
         ORDER BY name LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
