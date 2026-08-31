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

/// Fetches one custom line by id, or `None` if no custom line has that id
/// (including catalogue-line ids, which are never rows in this table).
/// The second element of the tuple is the row's `user_id` (`None` for a
/// legacy pre-ownership-retrofit row -- see
/// `crates/api/migrations/20260828100000_add_ownership.sql` -- not just
/// "anonymous author," since custom lines have never had an anonymous
/// *write* path). `get_line` (the only caller that needs it) uses this to
/// compute `CustomLineDetail.isOwner`; `get_line_definition` ignores it.
pub async fn get_custom_line(pool: &PgPool, id: &str) -> Result<Option<(CustomLine, Option<String>)>> {
    let row = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter, user_id \
         FROM custom_lines WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some((
        CustomLine {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            operators: row.try_get("operators")?,
            stations: row.try_get("stations")?,
            headcode_prefixes: row.try_get("headcode_prefixes")?,
            destination_crs_filter: row.try_get("destination_crs_filter")?,
        },
        row.try_get("user_id")?,
    )))
}

/// Inserts a new custom line, deriving its id from `new.name` via
/// [`slugify`]. On a slug collision (another custom line already has that
/// id — e.g. two lines both named "My Commute"), retries with `-2`, `-3`,
/// ... appended until an unused id is found. The existence check is atomic
/// with the insert (`ON CONFLICT ... DO NOTHING RETURNING id`) so two
/// concurrent requests racing on the same id can't both pass a check and
/// then have one fail on the `PRIMARY KEY` constraint.
///
/// Also pins the newly created line, in the same transaction as the
/// insert — mirrors [`delete_custom_line`]'s existing "custom_lines row +
/// pinned_lines row together" pattern. A custom line only exists because
/// this instance's user made it, so the alternative (created but not
/// pinned, invisible on the home page until the user remembers to pin it
/// themselves) serves no one. The pin insert tolerates a conflict
/// (`ON CONFLICT DO NOTHING`): `pinned_lines` has no FK to `custom_lines`
/// by design (ids are free-form, client-supplied strings via
/// `PUT /preferences/pinned-lines`, never validated against any line
/// catalogue — see the preferences migration; that endpoint does require
/// an authenticated user now, but it still accepts any id the client
/// sends), so a stale row for this exact id can already exist from an
/// earlier pin of an id that didn't correspond to any line yet. Without this, creating a
/// line whose slug collides with such a stale pin would roll back an
/// otherwise-valid `custom_lines` insert and surface as a 500.
pub async fn insert_custom_line(pool: &PgPool, new: NewCustomLine, user_id: &str) -> Result<CustomLine> {
    let base_id = slugify(&new.name);
    let mut id = base_id.clone();
    let mut suffix = 2;
    loop {
        let mut tx = pool.begin().await?;
        let inserted: Option<String> = sqlx::query_scalar(
            r#"
            INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, user_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT (id) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&id)
        .bind(&new.name)
        .bind(&new.operators)
        .bind(&new.stations)
        .bind(&new.headcode_prefixes)
        .bind(&new.destination_crs_filter)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;

        if inserted.is_some() {
            sqlx::query(
                "INSERT INTO pinned_lines (user_id, line_id, pinned_at) VALUES ($1, $2, NOW()) \
                 ON CONFLICT (user_id, line_id) DO NOTHING",
            )
            .bind(user_id)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            break;
        }
        id = format!("{base_id}-{suffix}");
        suffix += 1;
    }

    Ok(CustomLine {
        id,
        name: new.name,
        operators: new.operators,
        stations: new.stations,
        headcode_prefixes: new.headcode_prefixes,
        destination_crs_filter: new.destination_crs_filter,
    })
}

/// Updates an existing custom line's editable fields in place. The `id`
/// itself is never changed — it was derived once at creation time and
/// pinned-line references / bookmarked URLs depend on it staying stable,
/// even if the line is later renamed. Returns `None` if no custom line
/// has that id (mirrors [`delete_custom_line`]'s `bool` — `Option` here
/// instead since the caller needs the updated row back on success).
pub async fn update_custom_line(
    pool: &PgPool,
    id: &str,
    new: NewCustomLine,
    user_id: &str,
) -> Result<Option<CustomLine>> {
    let result = sqlx::query(
        r#"
        UPDATE custom_lines
        SET name = $2, operators = $3, stations = $4, headcode_prefixes = $5, destination_crs_filter = $6
        WHERE id = $1 AND user_id = $7
        "#,
    )
    .bind(id)
    .bind(&new.name)
    .bind(&new.operators)
    .bind(&new.stations)
    .bind(&new.headcode_prefixes)
    .bind(&new.destination_crs_filter)
    .bind(user_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(Some(CustomLine {
        id: id.to_string(),
        name: new.name,
        operators: new.operators,
        stations: new.stations,
        headcode_prefixes: new.headcode_prefixes,
        destination_crs_filter: new.destination_crs_filter,
    }))
}

/// Deletes a custom line by id, and any `pinned_lines` row referencing it,
/// in one transaction — without this, unpinning would be impossible for a
/// line that no longer exists, and the stale pin would sit forever (no FK
/// exists to catch it, since `pinned_lines` intentionally has none — see
/// the preferences migration). Returns `true` if a custom line was
/// deleted, `false` if no custom line had that id (a no-op either way for
/// `pinned_lines`, since a non-custom-line id was never insertable there
/// through normal use, but the DELETE is harmless if it somehow was).
pub async fn delete_custom_line(pool: &PgPool, id: &str, user_id: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query("DELETE FROM custom_lines WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let deleted = result.rows_affected() > 0;
    if deleted {
        sqlx::query("DELETE FROM pinned_lines WHERE line_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(deleted)
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

#[cfg(test)]
mod db_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row \
                -- --ignored`"]
    async fn get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-CUSTOM-LINE-OWNER', 'owner@example.com', 'Owner') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        // The real write path: identical to what `create_line` does.
        let owned = insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Owned Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-CUSTOM-LINE-OWNER",
        )
        .await
        .expect("insert owned line");

        let (_, owner) = get_custom_line(&pool, &owned.id)
            .await
            .expect("get custom line")
            .expect("line should exist");
        assert_eq!(owner, Some("TEST-CUSTOM-LINE-OWNER".to_string()));

        // A legacy row that predates the ownership retrofit (see
        // `crates/api/migrations/20260828100000_add_ownership.sql`) has a
        // NULL `user_id` -- inserted directly, since `insert_custom_line`
        // always attributes an owner now and has no way to produce this
        // shape itself.
        sqlx::query(
            "INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, user_id, created_at) \
             VALUES ('custom-test-legacy-line', 'Test Legacy Line', '{}', '{WOK,CLJ}', '{}', '{}', NULL, NOW())",
        )
        .execute(&pool)
        .await
        .expect("seed legacy fixture line");

        let (_, legacy_owner) = get_custom_line(&pool, "custom-test-legacy-line")
            .await
            .expect("get custom line")
            .expect("line should exist");
        assert_eq!(legacy_owner, None);

        sqlx::query("DELETE FROM custom_lines WHERE id = $1")
            .bind(&owned.id)
            .execute(&pool)
            .await
            .expect("cleanup owned fixture line");
        sqlx::query("DELETE FROM custom_lines WHERE id = 'custom-test-legacy-line'")
            .execute(&pool)
            .await
            .expect("cleanup legacy fixture line");
        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-CUSTOM-LINE-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-CUSTOM-LINE-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }
}
