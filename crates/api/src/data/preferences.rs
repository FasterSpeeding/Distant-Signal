//! Queries for user preferences: pinned lines, pinned stations, and pinned
//! operators. See
//! `docs/superpowers/specs/2026-07-09-frontend-personalization-design.md`.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::{PgPool, Row};

pub async fn list_pinned_line_ids(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT line_id FROM pinned_lines WHERE user_id = $1 ORDER BY pinned_at, line_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("line_id")?))
        .collect()
}

/// How many users have pinned each line, across all users. Only lines with
/// at least one pin appear. Used by `GET /private/sample-stations`'s
/// optional pinned-lines filter and cap priority (LEG-18); never exposed
/// per user.
pub async fn count_pins_per_line(pool: &PgPool) -> Result<HashMap<String, i64>> {
    let rows = sqlx::query("SELECT line_id, COUNT(*) AS pins FROM pinned_lines GROUP BY line_id")
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| Ok((row.try_get("line_id")?, row.try_get("pins")?)))
        .collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows =
        sqlx::query("SELECT crs FROM pinned_stations WHERE user_id = $1 ORDER BY pinned_at, crs")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("crs")?))
        .collect()
}

pub async fn list_pinned_operator_codes(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT operator_code FROM pinned_operators WHERE user_id = $1 ORDER BY pinned_at, operator_code",
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

/// Drops later duplicates from `items`, keeping the first occurrence of
/// each (and therefore the caller's own ordering for whichever copy
/// survives) -- shared by all three `replace_pinned_*` functions below.
///
/// Load-bearing, not cosmetic: each of those functions inserts one row per
/// element inside a single transaction with no `ON CONFLICT` clause, so a
/// caller-supplied duplicate (the same id/code twice in one PUT body) would
/// hit the `(user_id, <key>)` primary key on the second insert and fail the
/// whole request with a 500 -- a client bug (double-submitted pin) turning
/// into a server error instead of a clean, idempotent write. Route-layer
/// length capping (see `crates/api/src/routes/preferences.rs`) handles the
/// sibling "unbounded array" concern; this handles "unbounded array with
/// unbounded duplicates in it" being cheap to send even under that cap.
fn dedupe_preserving_order(items: &[String]) -> Vec<&String> {
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    items
        .iter()
        .filter(|item| seen.insert(item.as_str()))
        .collect()
}

/// One `INSERT ... SELECT FROM unnest(..) WITH ORDINALITY` for a whole
/// pinned set (DB2-16). It used to be one INSERT per item, all stamped with
/// the transaction-constant `NOW()`, so every row of a PUT had the same
/// `pinned_at` and the `ORDER BY pinned_at` reads came back in heap order.
/// Each row's `pinned_at` is now `NOW()` plus its position in microseconds,
/// so the caller's order survives without a schema change. `table` and
/// `column` are compile-time literals from this module, never input.
async fn insert_pinned_in_order(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &'static str,
    column: &'static str,
    user_id: &str,
    items: &[&String],
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let items: Vec<&str> = items.iter().map(|item| item.as_str()).collect();
    let sql = format!(
        "INSERT INTO {table} (user_id, {column}, pinned_at) \
         SELECT $1, item, NOW() + ord * INTERVAL '1 microsecond' \
         FROM unnest($2::text[]) WITH ORDINALITY AS t(item, ord)"
    );
    sqlx::query(&sql)
        .bind(user_id)
        .bind(&items)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Replaces `user_id`'s entire pinned-lines set with `ids`, in one
/// transaction (delete-all then insert-all) so a PUT is atomic — concurrent
/// readers never see a partially-updated list. Scoped to `user_id` now, not
/// the whole table -- the pre-ownership version's `DELETE FROM pinned_lines`
/// (no predicate) would wipe every other user's pins too.
///
/// `ids` is deduplicated (see [`dedupe_preserving_order`]) before the
/// insert loop -- a duplicate id in the caller's array must not turn into a
/// primary-key-violation 500.
pub async fn replace_pinned_lines(pool: &PgPool, user_id: &str, ids: &[String]) -> Result<()> {
    let ids = dedupe_preserving_order(ids);
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    insert_pinned_in_order(&mut tx, "pinned_lines", "line_id", user_id, &ids).await?;
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`, for stations
/// -- including the same de-duplication of `crs_codes` before the insert
/// loop, for the same reason (see [`dedupe_preserving_order`]).
pub async fn replace_pinned_stations(
    pool: &PgPool,
    user_id: &str,
    crs_codes: &[String],
) -> Result<()> {
    let crs_codes = dedupe_preserving_order(crs_codes);
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    insert_pinned_in_order(&mut tx, "pinned_stations", "crs", user_id, &crs_codes).await?;
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`/
/// `replace_pinned_stations` -- delete-all-then-insert-all in one
/// transaction, scoped to `user_id`, including the same de-duplication of
/// `codes` before the insert loop (see [`dedupe_preserving_order`]).
pub async fn replace_pinned_operators(
    pool: &PgPool,
    user_id: &str,
    codes: &[String],
) -> Result<()> {
    let codes = dedupe_preserving_order(codes);
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_operators WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    insert_pinned_in_order(
        &mut tx,
        "pinned_operators",
        "operator_code",
        user_id,
        &codes,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_preserving_order_drops_later_duplicates_keeping_first_occurrence_order() {
        let items = vec![
            "northern".to_string(),
            "victoria".to_string(),
            "northern".to_string(),
            "central".to_string(),
            "victoria".to_string(),
        ];
        let deduped: Vec<&str> = dedupe_preserving_order(&items)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(deduped, vec!["northern", "victoria", "central"]);
    }

    #[test]
    fn dedupe_preserving_order_of_an_already_unique_list_is_unchanged() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let deduped: Vec<&str> = dedupe_preserving_order(&items)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(deduped, vec!["a", "b", "c"]);
    }
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

    /// DB2-16: a PUT's order is stored, not left to heap order. Each row
    /// gets a distinct `pinned_at` in input order, and every list reads
    /// back exactly as written, including a reversed and a shuffled set.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                replace_pinned_sets_store_the_callers_order -- --ignored`"]
    async fn replace_pinned_sets_store_the_callers_order() {
        let pool = connect().await;
        let user_id = "TEST-PINNED-ORDER-USER";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, 'order@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let lines = owned(&[
            "zzz-test-line",
            "aaa-test-line",
            "mmm-test-line",
            "bbb-test-line",
        ]);
        let stations = owned(&["ZZZ", "AAA", "MMM", "BBB"]);
        let operators = owned(&["ZZ", "AA", "MM", "BB"]);
        replace_pinned_lines(&pool, user_id, &lines)
            .await
            .expect("pin lines");
        replace_pinned_stations(&pool, user_id, &stations)
            .await
            .expect("pin stations");
        replace_pinned_operators(&pool, user_id, &operators)
            .await
            .expect("pin operators");

        assert_eq!(list_pinned_line_ids(&pool, user_id).await.unwrap(), lines);
        assert_eq!(
            list_pinned_station_crs(&pool, user_id).await.unwrap(),
            stations
        );
        assert_eq!(
            list_pinned_operator_codes(&pool, user_id).await.unwrap(),
            operators
        );

        let stamps: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT operator_code, pinned_at FROM pinned_operators WHERE user_id = $1 \
             ORDER BY pinned_at",
        )
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            stamps.windows(2).all(|w| w[0].1 < w[1].1),
            "every row of one PUT must have its own pinned_at: {stamps:?}"
        );

        // Reversing the set is honoured too.
        let reversed: Vec<String> = operators.iter().rev().cloned().collect();
        replace_pinned_operators(&pool, user_id, &reversed)
            .await
            .unwrap();
        assert_eq!(
            list_pinned_operator_codes(&pool, user_id).await.unwrap(),
            reversed
        );

        for sql in [
            "DELETE FROM pinned_lines WHERE user_id = $1",
            "DELETE FROM pinned_stations WHERE user_id = $1",
            "DELETE FROM pinned_operators WHERE user_id = $1",
            "DELETE FROM users WHERE id = $1",
        ] {
            sqlx::query(sql)
                .bind(user_id)
                .execute(&pool)
                .await
                .expect("cleanup");
        }
    }

    /// The real-failure-shape regression for the review finding: before
    /// de-duplication, a duplicate entry in the PUT body reached the
    /// per-row insert loop unchanged, and the second insert of the same
    /// `(user_id, operator_code)` hit the primary key and failed the whole
    /// request with a 500 instead of a clean, idempotent write. `replace_pinned_operators`
    /// is exercised directly here (the same call
    /// `crates/api/src/routes/preferences.rs::put_pinned_operators` makes)
    /// with a caller-supplied duplicate, asserting it now succeeds and the
    /// duplicate collapses to one stored row.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                replace_pinned_operators_with_a_duplicate_code_does_not_500_and_collapses_to_one_row \
                -- --ignored`"]
    async fn replace_pinned_operators_with_a_duplicate_code_does_not_500_and_collapses_to_one_row()
    {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-PINNED-DUP-USER', 'test@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        // "SW" appears twice -- before de-duplication this would hit the
        // (user_id, operator_code) primary key on the second insert and
        // fail the whole transaction.
        replace_pinned_operators(
            &pool,
            "TEST-PINNED-DUP-USER",
            &["SW".to_string(), "VT".to_string(), "SW".to_string()],
        )
        .await
        .expect("a duplicate entry in the input must not fail the whole PUT with a 500");

        let mut codes = list_pinned_operator_codes(&pool, "TEST-PINNED-DUP-USER")
            .await
            .expect("list pinned operator codes");
        codes.sort();
        assert_eq!(
            codes,
            vec!["SW".to_string(), "VT".to_string()],
            "the duplicate must collapse to a single stored row, not be stored twice or dropped entirely"
        );

        sqlx::query("DELETE FROM pinned_operators WHERE user_id = 'TEST-PINNED-DUP-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PINNED-DUP-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }

    /// LEG-18: pins are counted per line across users. Asserts only on this
    /// test's own synthetic line ids, so pins left by other fixtures in a
    /// shared database cannot affect it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                count_pins_per_line_counts_each_line_across_users -- --ignored`"]
    async fn count_pins_per_line_counts_each_line_across_users() {
        let pool = connect().await;
        let users = ["TEST-PIN-COUNT-USER-A", "TEST-PIN-COUNT-USER-B"];
        for user in users {
            sqlx::query(
                "INSERT INTO users (id, email, name) VALUES ($1, 'pins@example.com', 'Test Rider') \
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(user)
            .execute(&pool)
            .await
            .expect("seed fixture user");
        }
        replace_pinned_lines(
            &pool,
            users[0],
            &[
                "test-pin-count-both".to_string(),
                "test-pin-count-one".to_string(),
            ],
        )
        .await
        .expect("pin lines for user A");
        replace_pinned_lines(&pool, users[1], &["test-pin-count-both".to_string()])
            .await
            .expect("pin lines for user B");

        let counts = count_pins_per_line(&pool).await.expect("count pins");

        for user in users {
            sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1")
                .bind(user)
                .execute(&pool)
                .await
                .expect("cleanup fixture pins");
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user)
                .execute(&pool)
                .await
                .expect("cleanup fixture user");
        }

        assert_eq!(counts.get("test-pin-count-both"), Some(&2));
        assert_eq!(counts.get("test-pin-count-one"), Some(&1));
        assert_eq!(counts.get("test-pin-count-never"), None);
    }
}
