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
/// (including catalogue-line ids, which are never rows in this table). The
/// second element of the tuple is the row's `user_id`. It's still typed
/// `Option<String>` at the Rust level, but since migration
/// 20260901120000_custom_lines_owner_not_null.sql the database itself
/// guarantees it's always `Some` -- the transient NULL-owner window opened
/// by `20260828100000_add_ownership.sql` is closed. `get_line` (the only
/// caller that needs it) uses this to gate ownership; `get_line_definition`
/// ignores it.
pub async fn get_custom_line(
    pool: &PgPool,
    id: &str,
) -> Result<Option<(CustomLine, Option<String>)>> {
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

/// How many custom lines one user may own.
///
/// This is not a storage limit -- a `custom_lines` row is tiny. It is a cap
/// on recurring COMPUTE: `aggregator`'s `run_cycle` reloads every custom line
/// from this table on every cycle (60s by default) and merges it into the
/// global catalogue, so each one is re-evaluated against every active
/// incident by the matcher, gets its segments rebuilt in the
/// `SegmentRegistry`, and takes its own `line_status`/daily-stats writes --
/// every cycle, forever, whether or not anyone ever looks at it. Creation was
/// previously bounded only by "a non-empty name and at least 2 stations", so
/// one user scripting a few thousand creates would degrade the aggregation
/// cycle for every user of the system.
///
/// 50 is far above any plausible real use (the feature exists so someone can
/// describe their own commute -- a handful of routes), and far below the
/// thousands it takes to matter for cycle time.
pub const MAX_CUSTOM_LINES_PER_USER: i64 = 50;

/// How many custom lines `user_id` currently owns, for
/// [`MAX_CUSTOM_LINES_PER_USER`] enforcement.
pub async fn count_custom_lines_for_user(pool: &PgPool, user_id: &str) -> Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM custom_lines WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
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
pub async fn insert_custom_line(
    pool: &PgPool,
    new: NewCustomLine,
    user_id: &str,
) -> Result<CustomLine> {
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

/// Every custom-line id in `ids` that `user_id` may READ: either they own
/// it outright, or its owner has granted it (see
/// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
/// §2.3-§2.7) into at least one group `user_id` is CURRENTLY a member of.
///
/// This is the single gate every custom-line read path funnels through --
/// `routes::lines::get_line`/`get_line_definition` and
/// `routes::line_status::filter_private_custom_rows`/
/// `get_line_status_history` -- replacing what used to be a bare
/// `owners_for_ids` comparison at each of them. Each of those checks became
/// strictly WIDER by exactly one disjunct ("or granted into one of my
/// groups"); none of them lost a condition.
///
/// One query, not one grant-lookup per id, via a LEFT JOIN through
/// `custom_line_group_grants` + `group_members`, because `get_mode_status`
/// calls this with every custom-line row on the instance and must not gain
/// an N+1.
///
/// Access is resolved LIVE on every call: deleting the grant row makes the
/// very next request from a now-ungranted member miss this set, with
/// nothing cached or capability-shaped left over to revoke separately
/// (design §2.5).
///
/// Deliberately returns a `HashSet`, not a `HashMap<String, Option<String>>`
/// like [`owners_for_ids`]: callers here only ever need "can this caller
/// read this id," never "who owns it," so there is no reason for this
/// function's shape to hand an owner's user id to a caller that has no
/// business with it.
pub async fn readable_custom_line_ids(
    pool: &PgPool,
    ids: &[String],
    user_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT cl.id \
         FROM custom_lines cl \
         LEFT JOIN custom_line_group_grants g ON g.line_id = cl.id \
         LEFT JOIN group_members gm ON gm.group_id = g.group_id AND gm.user_id = $2 \
         WHERE cl.id = ANY($1) AND (cl.user_id = $2 OR gm.user_id IS NOT NULL)",
    )
    .bind(ids)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// The id prefix [`slugify`] stamps onto every custom line, and therefore
/// the only thing that distinguishes a private, user-owned row from a
/// public catalogue/TfL one anywhere a table keyed by line id (most
/// notably `line_status`) is read.
pub const CUSTOM_LINE_ID_PREFIX: &str = "custom-";

/// Drops every row whose line id names a private custom line the caller
/// may not read -- neither owned by them, nor granted (see
/// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
/// §3.2) into a group they are currently a member of. Catalogue/TfL rows
/// (no `custom-` prefix) are always kept untouched, and a request that
/// touched no custom row at all costs no extra query.
///
/// `caller_user_id` is `None` for an anonymous caller -- every custom-line
/// row is dropped for them: an anonymous caller owns nothing and is a
/// member of nothing, so there is no user id worth binding and the grant
/// lookup is skipped entirely. Anonymous callers therefore see FEWER rows,
/// never an error; every route using this gate stays usable logged out.
///
/// Generic over the row type, with `id_of` naming the field that carries
/// the line id, because the two tables' row structs disagree on that
/// field's name -- `queries::LineStatusRow::id` vs
/// `queries::IncidentLineRefRow::line_id` -- and that cosmetic difference
/// is not a reason to have two copies of a privacy gate. It lives here,
/// next to [`readable_custom_line_ids`], rather than in whichever route
/// module happened to need it first: `GET /public/incidents/{id}` shipped
/// a live disclosure of other users' custom-line ids and names precisely
/// because the gate was a private helper inside `routes::line_status` that
/// a second reader of `line_status` never found (see
/// docs/superpowers/specs/2026-09-16-custom-lines-in-incident-archive-filter-research.md
/// §5c). ANY new reader of a line-id-keyed table must funnel through this.
pub async fn retain_readable_custom_rows<T>(
    pool: &PgPool,
    rows: Vec<T>,
    caller_user_id: Option<&str>,
    id_of: impl Fn(&T) -> &str,
) -> Result<Vec<T>> {
    let custom_ids: Vec<String> = rows
        .iter()
        .map(&id_of)
        .filter(|id| id.starts_with(CUSTOM_LINE_ID_PREFIX))
        .map(str::to_string)
        .collect();
    if custom_ids.is_empty() {
        return Ok(rows);
    }
    let Some(user_id) = caller_user_id else {
        return Ok(rows
            .into_iter()
            .filter(|row| !id_of(row).starts_with(CUSTOM_LINE_ID_PREFIX))
            .collect());
    };
    let readable = readable_custom_line_ids(pool, &custom_ids, user_id).await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let id = id_of(row);
            !id.starts_with(CUSTOM_LINE_ID_PREFIX) || readable.contains(id)
        })
        .collect())
}

/// Single-id counterpart of [`retain_readable_custom_rows`], for the routes
/// whose `{id}` path segment IS the whole query rather than a filter over a
/// set of rows: `/Line/{id}/Status/...`, the six `/Line/{id}/Stats/...`, and
/// `/public/lines/{id}/schedule`/`/trains`. A catalogue/TfL id is always
/// readable and costs no query; a `custom-` id is readable only by its owner
/// or a current member of a group it is granted into; an anonymous caller
/// short-circuits to `false` with no query at all.
///
/// A route that refuses on this should answer exactly as it would for a
/// genuinely unknown line id (an empty array, or its usual 404) -- never a
/// distinct `403`, which would itself confirm the id exists.
pub async fn caller_may_read_line_id(
    pool: &PgPool,
    id: &str,
    caller_user_id: Option<&str>,
) -> Result<bool> {
    if !id.starts_with(CUSTOM_LINE_ID_PREFIX) {
        return Ok(true);
    }
    let Some(user_id) = caller_user_id else {
        return Ok(false);
    };
    let ids = [id.to_string()];
    Ok(readable_custom_line_ids(pool, &ids, user_id)
        .await?
        .contains(id))
}

/// Owners for every custom-prefixed id in `ids`, for filtering a bulk
/// status response by ownership without an N+1 query per row (see
/// `crate::routes::line_status`'s three affected handlers). Catalogue/TfL
/// ids in `ids` simply won't match anything here -- callers should look
/// them up unconditionally in the returned map and treat "no entry" as
/// "not a custom line, leave it alone," never as "unowned."
///
/// NOTE for new read gates: this is the narrower, owner-only primitive and
/// is no longer what the custom-line read paths use. Group sharing means
/// "readable by this caller" is strictly wider than "owned by this caller"
/// -- use [`readable_custom_line_ids`] instead, or a shared custom line
/// will be invisible to the very members its owner granted it to.
pub async fn owners_for_ids(
    pool: &PgPool,
    ids: &[String],
) -> Result<std::collections::HashMap<String, Option<String>>> {
    let rows = sqlx::query("SELECT id, user_id FROM custom_lines WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("id")?,
                row.try_get::<Option<String>, _>("user_id")?,
            ))
        })
        .collect()
}

/// Caller-scoped variant of [`list_custom_lines`] -- used by `list_lines`
/// (`GET /public/lines`) once custom lines become private, so an
/// authenticated caller sees only their own custom lines in the bulk list,
/// never anyone else's. Deliberately a separate function rather than an
/// `Option<&str>` parameter on `list_custom_lines` itself: the anonymous
/// case (Decision 8) skips the custom-line query entirely rather than
/// calling this with some sentinel, so the two call shapes never need to
/// share a signature.
pub async fn list_custom_lines_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<CustomLine>> {
    let rows = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
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

    /// Shared fixtures for the group-grant tests below (the older tests in
    /// this module predate them and still connect/seed inline -- left as
    /// they are rather than churned).
    mod fixtures {
        use sqlx::PgPool;
        use sqlx::postgres::PgPoolOptions;

        pub async fn connect() -> PgPool {
            let database_url =
                std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
            PgPoolOptions::new()
                .connect(&database_url)
                .await
                .expect("connect to postgres")
        }

        pub async fn seed_user(pool: &PgPool, id: &str) {
            sqlx::query(
                "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) \
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(id)
            .bind(format!("{id}@example.com"))
            .bind(id)
            .execute(pool)
            .await
            .expect("seed fixture user");
        }

        pub async fn seed_group(pool: &PgPool, name: &str, owner_id: &str) -> String {
            crate::data::groups::create_group(pool, name, owner_id)
                .await
                .expect("seed fixture group")
        }

        pub async fn seed_membership(pool: &PgPool, group_id: &str, user_id: &str) {
            sqlx::query(
                "INSERT INTO group_members (group_id, user_id, role, joined_at) \
                 VALUES ($1, $2, 'member', NOW()) ON CONFLICT DO NOTHING",
            )
            .bind(group_id)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed fixture membership");
        }

        pub async fn grant(pool: &PgPool, group_id: &str, line_id: &str, granted_by: &str) {
            sqlx::query(
                "INSERT INTO custom_line_group_grants (group_id, line_id, granted_by) \
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(group_id)
            .bind(line_id)
            .bind(granted_by)
            .execute(pool)
            .await
            .expect("seed fixture grant");
        }

        /// Deletes groups first, then custom lines and pins, then users --
        /// `groups.created_by` and `custom_line_group_grants.granted_by`
        /// both reference `users(id)` with no cascade, so users must go
        /// last.
        pub async fn cleanup(pool: &PgPool, group_ids: &[&str], user_ids: &[&str]) {
            for id in group_ids {
                sqlx::query("DELETE FROM groups WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .ok();
            }
            for id in user_ids {
                sqlx::query("DELETE FROM custom_lines WHERE user_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .ok();
                sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .ok();
                sqlx::query("DELETE FROM users WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .ok();
            }
        }

        pub fn new_line(name: &str) -> super::NewCustomLine {
            super::NewCustomLine {
                name: name.to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                readable_custom_line_ids_includes_the_owners_own_line_with_no_grant \
                -- --ignored --test-threads=1`"]
    async fn readable_custom_line_ids_includes_the_owners_own_line_with_no_grant() {
        // Baseline: this function must never be a strict widening that
        // accidentally requires a grant even for the line's own owner.
        let pool = fixtures::connect().await;
        fixtures::seed_user(&pool, "TEST-GRANT-READ-OWNER-1").await;
        let line = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Read Test 1"),
            "TEST-GRANT-READ-OWNER-1",
        )
        .await
        .expect("insert line");

        let readable = readable_custom_line_ids(
            &pool,
            std::slice::from_ref(&line.id),
            "TEST-GRANT-READ-OWNER-1",
        )
        .await
        .expect("query");
        assert!(readable.contains(&line.id));

        fixtures::cleanup(&pool, &[], &["TEST-GRANT-READ-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                readable_custom_line_ids_includes_a_granted_line_for_a_fellow_member \
                -- --ignored --test-threads=1`"]
    async fn readable_custom_line_ids_includes_a_granted_line_for_a_fellow_member() {
        let pool = fixtures::connect().await;
        fixtures::seed_user(&pool, "TEST-GRANT-READ-OWNER-2").await;
        fixtures::seed_user(&pool, "TEST-GRANT-READ-MEMBER-2").await;
        let group_id = fixtures::seed_group(&pool, "Grant Read 2", "TEST-GRANT-READ-OWNER-2").await;
        fixtures::seed_membership(&pool, &group_id, "TEST-GRANT-READ-MEMBER-2").await;
        let line = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Read Test 2"),
            "TEST-GRANT-READ-OWNER-2",
        )
        .await
        .expect("insert line");
        fixtures::grant(&pool, &group_id, &line.id, "TEST-GRANT-READ-OWNER-2").await;

        let readable = readable_custom_line_ids(
            &pool,
            std::slice::from_ref(&line.id),
            "TEST-GRANT-READ-MEMBER-2",
        )
        .await
        .expect("query");
        assert!(
            readable.contains(&line.id),
            "a fellow group member must be able to read a line granted into that group"
        );

        fixtures::cleanup(
            &pool,
            &[&group_id],
            &["TEST-GRANT-READ-OWNER-2", "TEST-GRANT-READ-MEMBER-2"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                readable_custom_line_ids_excludes_a_granted_line_for_a_non_member \
                -- --ignored --test-threads=1`"]
    async fn readable_custom_line_ids_excludes_a_granted_line_for_a_non_member() {
        // The core privacy case. The stranger is deliberately a member of a
        // DIFFERENT group that has grants of its own, so a query that
        // forgot to correlate `group_members.group_id` with the grant's
        // would pass every other test in this file and fail only this one.
        let pool = fixtures::connect().await;
        fixtures::seed_user(&pool, "TEST-GRANT-READ-OWNER-3").await;
        fixtures::seed_user(&pool, "TEST-GRANT-READ-STRANGER-3").await;
        let group_a = fixtures::seed_group(&pool, "Grant Read 3A", "TEST-GRANT-READ-OWNER-3").await;
        let group_b =
            fixtures::seed_group(&pool, "Grant Read 3B", "TEST-GRANT-READ-STRANGER-3").await;
        let line_a = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Read Test 3A"),
            "TEST-GRANT-READ-OWNER-3",
        )
        .await
        .expect("insert line A");
        let line_b = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Read Test 3B"),
            "TEST-GRANT-READ-STRANGER-3",
        )
        .await
        .expect("insert line B");
        fixtures::grant(&pool, &group_a, &line_a.id, "TEST-GRANT-READ-OWNER-3").await;
        fixtures::grant(&pool, &group_b, &line_b.id, "TEST-GRANT-READ-STRANGER-3").await;

        let readable = readable_custom_line_ids(
            &pool,
            &[line_a.id.clone(), line_b.id.clone()],
            "TEST-GRANT-READ-STRANGER-3",
        )
        .await
        .expect("query");
        assert!(
            !readable.contains(&line_a.id),
            "a line granted into a group the caller is NOT in must stay invisible"
        );
        assert!(
            readable.contains(&line_b.id),
            "positive control: the caller's own line is still readable"
        );

        fixtures::cleanup(
            &pool,
            &[&group_a, &group_b],
            &["TEST-GRANT-READ-OWNER-3", "TEST-GRANT-READ-STRANGER-3"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                readable_custom_line_ids_excludes_a_line_after_its_grant_is_revoked \
                -- --ignored --test-threads=1`"]
    async fn readable_custom_line_ids_excludes_a_line_after_its_grant_is_revoked() {
        // Design §2.5: revocation is immediate and complete, because access
        // is resolved live on every call and there is nothing cached to
        // separately invalidate. Read, revoke, read again.
        let pool = fixtures::connect().await;
        fixtures::seed_user(&pool, "TEST-GRANT-REVOKE-OWNER").await;
        fixtures::seed_user(&pool, "TEST-GRANT-REVOKE-MEMBER").await;
        let group_id =
            fixtures::seed_group(&pool, "Grant Revoke Read", "TEST-GRANT-REVOKE-OWNER").await;
        fixtures::seed_membership(&pool, &group_id, "TEST-GRANT-REVOKE-MEMBER").await;
        let line = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Revoke Read Test"),
            "TEST-GRANT-REVOKE-OWNER",
        )
        .await
        .expect("insert line");
        fixtures::grant(&pool, &group_id, &line.id, "TEST-GRANT-REVOKE-OWNER").await;

        assert!(
            readable_custom_line_ids(
                &pool,
                std::slice::from_ref(&line.id),
                "TEST-GRANT-REVOKE-MEMBER",
            )
            .await
            .expect("query before")
            .contains(&line.id)
        );

        let removed = crate::data::groups::remove_custom_line_grant(
            &pool,
            &group_id,
            &line.id,
            "TEST-GRANT-REVOKE-OWNER",
            false,
        )
        .await
        .expect("revoke");
        assert!(removed);

        assert!(
            !readable_custom_line_ids(
                &pool,
                std::slice::from_ref(&line.id),
                "TEST-GRANT-REVOKE-MEMBER",
            )
            .await
            .expect("query after")
            .contains(&line.id),
            "revocation must cut off access on the very next read"
        );

        fixtures::cleanup(
            &pool,
            &[&group_id],
            &["TEST-GRANT-REVOKE-OWNER", "TEST-GRANT-REVOKE-MEMBER"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_custom_line_cascades_its_group_grants -- --ignored --test-threads=1`"]
    async fn delete_custom_line_cascades_its_group_grants() {
        // Design §2.2: the FK does this, not application code --
        // `delete_custom_line` itself is completely unaware grants exist.
        // Also re-asserts the existing `pinned_lines` cleanup alongside it,
        // since the two live in the same transaction and a future edit
        // could plausibly break either.
        let pool = fixtures::connect().await;
        fixtures::seed_user(&pool, "TEST-GRANT-CASCADE-OWNER").await;
        let group_a =
            fixtures::seed_group(&pool, "Grant Cascade A", "TEST-GRANT-CASCADE-OWNER").await;
        let group_b =
            fixtures::seed_group(&pool, "Grant Cascade B", "TEST-GRANT-CASCADE-OWNER").await;
        let line = insert_custom_line(
            &pool,
            fixtures::new_line("Grant Cascade Test"),
            "TEST-GRANT-CASCADE-OWNER",
        )
        .await
        .expect("insert line");
        fixtures::grant(&pool, &group_a, &line.id, "TEST-GRANT-CASCADE-OWNER").await;
        fixtures::grant(&pool, &group_b, &line.id, "TEST-GRANT-CASCADE-OWNER").await;

        let deleted = delete_custom_line(&pool, &line.id, "TEST-GRANT-CASCADE-OWNER")
            .await
            .expect("delete line");
        assert!(deleted);

        let remaining: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE line_id = $1")
                .bind(&line.id)
                .fetch_one(&pool)
                .await
                .expect("count grants");
        assert_eq!(
            remaining.0, 0,
            "every grant into every group should have cascaded away with the line"
        );
        let pins: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pinned_lines WHERE line_id = $1")
            .bind(&line.id)
            .fetch_one(&pool)
            .await
            .expect("count pins");
        assert_eq!(pins.0, 0, "the existing pinned_lines cleanup still runs");

        fixtures::cleanup(&pool, &[&group_a, &group_b], &["TEST-GRANT-CASCADE-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_custom_line_reports_the_owning_user_id \
                -- --ignored`"]
    async fn get_custom_line_reports_the_owning_user_id() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

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

        sqlx::query("DELETE FROM custom_lines WHERE id = $1")
            .bind(&owned.id)
            .execute(&pool)
            .await
            .expect("cleanup owned fixture line");
        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-CUSTOM-LINE-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-CUSTOM-LINE-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                custom_lines_user_id_column_rejects_null -- --ignored`"]
    async fn custom_lines_user_id_column_rejects_null() {
        // Migration 20260901120000_custom_lines_owner_not_null.sql deleted
        // every surviving NULL-owner row (the repo owner's explicit choice,
        // a deviation from the plan's own reassign-to-placeholder default --
        // see that migration's header comment) and added a NOT NULL
        // constraint to `custom_lines.user_id`. A legacy NULL-owner row can
        // therefore no longer exist: this asserts the constraint is real at
        // the database level, not just assumed, by attempting the exact
        // insert shape the old fixture used to seed a "legacy row" and
        // confirming Postgres now rejects it.
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let result = sqlx::query(
            "INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, user_id, created_at) \
             VALUES ('custom-test-null-owner-rejected', 'Test Null Owner Rejected', '{}', '{WOK,CLJ}', '{}', '{}', NULL, NOW())",
        )
        .execute(&pool)
        .await;

        assert!(
            result.is_err(),
            "inserting a custom_lines row with an explicit NULL user_id should fail after the NOT NULL migration"
        );

        // Defensive cleanup in case the assertion above is ever run against
        // a database that predates the migration and the insert actually
        // succeeded -- don't leave a stray row behind.
        sqlx::query("DELETE FROM custom_lines WHERE id = 'custom-test-null-owner-rejected'")
            .execute(&pool)
            .await
            .expect("cleanup fixture row if the insert unexpectedly succeeded");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                owners_for_ids -- --ignored`"]
    async fn owners_for_ids_returns_real_owner_and_omits_missing() {
        // Prior to migration 20260901120000_custom_lines_owner_not_null.sql
        // this also seeded a legacy NULL-owner row and asserted
        // `owners_for_ids` reported `Some(None)` for it. That migration
        // deleted every surviving NULL-owner row and made the column
        // NOT NULL (the repo owner's explicit choice -- see that
        // migration's header comment), so a legacy row can no longer exist
        // to seed; `custom_lines_user_id_column_rejects_null` above covers
        // the constraint itself instead.
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-OWNERS-FOR-IDS-OWNER', 'owner@example.com', 'Owner') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        // The real write path: insert an owned line
        let owned = insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Owned Line for Owners".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-OWNERS-FOR-IDS-OWNER",
        )
        .await
        .expect("insert owned line");

        let ids = vec![
            owned.id.clone(),
            "catalogue-line-not-in-custom-table".to_string(), // A catalogue/TfL id not in the table
        ];

        let owners_map = owners_for_ids(&pool, &ids)
            .await
            .expect("call owners_for_ids");

        // The owned line should have its real owner
        assert_eq!(
            owners_map.get(&owned.id),
            Some(&Some("TEST-OWNERS-FOR-IDS-OWNER".to_string())),
            "owned line should have real owner"
        );

        // The catalogue id should not be in the map at all (no entry, not Some(None))
        assert!(
            !owners_map.contains_key("catalogue-line-not-in-custom-table"),
            "catalogue/TfL id not in custom_lines should be completely absent from map"
        );

        // Cleanup
        sqlx::query("DELETE FROM custom_lines WHERE id = $1")
            .bind(&owned.id)
            .execute(&pool)
            .await
            .expect("cleanup owned fixture line");
        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-OWNERS-FOR-IDS-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-OWNERS-FOR-IDS-OWNER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_custom_lines_for_user -- --ignored`"]
    async fn list_custom_lines_for_user_returns_only_calling_users_rows() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        // Create two test users
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-LIST-USER-1', 'user1@example.com', 'User 1') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user 1");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-LIST-USER-2', 'user2@example.com', 'User 2') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user 2");

        // User 1 creates a custom line
        let user1_line = insert_custom_line(
            &pool,
            NewCustomLine {
                name: "User 1 Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-USER-1",
        )
        .await
        .expect("insert user 1 line");

        // User 2 creates a different custom line
        let user2_line = insert_custom_line(
            &pool,
            NewCustomLine {
                name: "User 2 Line".to_string(),
                operators: vec!["TW".to_string()],
                stations: vec!["CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-USER-2",
        )
        .await
        .expect("insert user 2 line");

        // Query for user 1's lines -- should only get user 1's line
        let user1_lines = list_custom_lines_for_user(&pool, "TEST-LIST-USER-1")
            .await
            .expect("list user 1 lines");

        assert_eq!(user1_lines.len(), 1, "user 1 should have exactly 1 line");
        assert_eq!(
            user1_lines[0].id, user1_line.id,
            "user 1's line should be their created line"
        );
        assert_eq!(
            user1_lines[0].name, "User 1 Line",
            "user 1's line should have correct name"
        );

        // Query for user 2's lines -- should only get user 2's line
        let user2_lines = list_custom_lines_for_user(&pool, "TEST-LIST-USER-2")
            .await
            .expect("list user 2 lines");

        assert_eq!(user2_lines.len(), 1, "user 2 should have exactly 1 line");
        assert_eq!(
            user2_lines[0].id, user2_line.id,
            "user 2's line should be their created line"
        );
        assert_eq!(
            user2_lines[0].name, "User 2 Line",
            "user 2's line should have correct name"
        );

        // Query for a user with no lines
        let empty_lines = list_custom_lines_for_user(&pool, "TEST-LIST-USER-NONEXISTENT")
            .await
            .expect("list nonexistent user lines");

        assert_eq!(
            empty_lines.len(),
            0,
            "nonexistent user should have no lines"
        );

        // Cleanup
        sqlx::query("DELETE FROM custom_lines WHERE id = $1")
            .bind(&user1_line.id)
            .execute(&pool)
            .await
            .expect("cleanup user 1 line");
        sqlx::query("DELETE FROM custom_lines WHERE id = $1")
            .bind(&user2_line.id)
            .execute(&pool)
            .await
            .expect("cleanup user 2 line");
        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-LIST-USER-1'")
            .execute(&pool)
            .await
            .expect("cleanup user 1 pins");
        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-LIST-USER-2'")
            .execute(&pool)
            .await
            .expect("cleanup user 2 pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-LIST-USER-1'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user 1");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-LIST-USER-2'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user 2");
    }
}
