//! CRUD queries for user-defined custom lines (`custom_lines` table). See
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`.

use anyhow::Result;
use common::CustomLine;
use sqlx::{PgPool, Row};

pub struct NewCustomLine {
    pub name: String,
    pub operators: Vec<String>,
    pub stations: Vec<String>,
    pub headcode_prefixes: Vec<String>,
    pub destination_crs_filter: Vec<String>,
}

/// Turns a line name into a stable, URL-safe id: lowercase, non-alphanumeric
/// runs collapsed to a single `-`, leading/trailing `-` trimmed, prefixed
/// `custom-` so it can never collide with a static `lines/*.toml` id (none
/// of which start with `custom-`).
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = true; // suppresses a leading dash
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    format!("custom-{slug}")
}

pub async fn list_custom_lines(pool: &PgPool) -> Result<Vec<CustomLine>> {
    let rows = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CustomLine {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                operators: row.try_get("operators")?,
                stations: row.try_get("stations")?,
                headcode_prefixes: row.try_get("headcode_prefixes")?,
                destination_crs_filter: row.try_get("destination_crs_filter")?,
            })
        })
        .collect()
}

/// Inserts a new custom line, deriving its id from `new.name` via
/// [`slugify`]. On a slug collision (another custom line already has that
/// id — e.g. two lines both named "My Commute"), retries with `-2`, `-3`,
/// ... appended until an unused id is found.
pub async fn insert_custom_line(pool: &PgPool, new: NewCustomLine) -> Result<CustomLine> {
    let base_id = slugify(&new.name);
    let mut id = base_id.clone();
    let mut suffix = 2;
    loop {
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM custom_lines WHERE id = $1")
            .bind(&id)
            .fetch_optional(pool)
            .await?;
        if existing.is_none() {
            break;
        }
        id = format!("{base_id}-{suffix}");
        suffix += 1;
    }

    sqlx::query(
        r#"
        INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        "#,
    )
    .bind(&id)
    .bind(&new.name)
    .bind(&new.operators)
    .bind(&new.stations)
    .bind(&new.headcode_prefixes)
    .bind(&new.destination_crs_filter)
    .execute(pool)
    .await?;

    Ok(CustomLine {
        id,
        name: new.name,
        operators: new.operators,
        stations: new.stations,
        headcode_prefixes: new.headcode_prefixes,
        destination_crs_filter: new.destination_crs_filter,
    })
}

/// Deletes a custom line by id. Returns `true` if a row was deleted,
/// `false` if no custom line had that id.
pub async fn delete_custom_line(pool: &PgPool, id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM custom_lines WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes_punctuation() {
        assert_eq!(slugify("My Commute"), "custom-my-commute");
    }

    #[test]
    fn slugify_collapses_runs_of_punctuation() {
        assert_eq!(slugify("Woking -> Alton!!"), "custom-woking-alton");
    }

    #[test]
    fn slugify_trims_trailing_punctuation() {
        assert_eq!(slugify("Trailing---"), "custom-trailing");
    }
}
