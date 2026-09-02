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
/// the station name, ranked in three tiers: exact code match, then
/// name-prefix match, then any other substring match, alphabetical within
/// each tier.
///
/// The ranking exists because plain `ORDER BY name` demonstrably buries
/// the answer to the single most likely query on this dataset: "York"
/// is a substring of ~40 Yorkshire station names, so the unranked query
/// returned Bentley (South Yorkshire), Bramley (West Yorkshire),
/// Chapeltown (South Yorkshire) and Clapham (North Yorkshire) above the
/// 20-row cap while York itself was never visible
/// (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F1).
///
/// This overrides the autocomplete spec's Non-goal that "plain substring
/// `ILIKE` on code or name is enough for a list this size"
/// (docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md:23-24).
/// The substring matching *is* still enough -- the WHERE clause is
/// unchanged and nothing that matched before stops matching -- but the
/// ordering was not. Still no `pg_trgm`, still no index: the CASE is
/// evaluated on rows the existing sequential scan already produced.
///
/// Two callers depend on this ordering for correctness, not just display:
/// `StationSearchForm`'s "Look up" button navigates to `suggestions[0]`
/// when the typed text isn't an exact match (frontend/app/stations/
/// StationSearchForm.tsx:27), and `getStationName` filters this response
/// for an exact code match (frontend/lib/api.ts:115-121) -- which the
/// 20-row cap could truncate out of the window for a code whose letters
/// are a common name substring (WAT also matches Blackwater, Bridgwater,
/// Waterbeach, Watford Junction...). Exact-code-first makes that row
/// always row 1, so it can never be capped away.
///
/// `q` must already be trimmed and non-empty (callers go through
/// `routes::reference::sanitize_query` first).
pub async fn search_stations(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let contains = format!("%{q}%");
    let prefix = format!("{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT crs AS code, name FROM stations \
         WHERE crs ILIKE $1 OR name ILIKE $1 \
         ORDER BY \
           CASE \
             WHEN crs ILIKE $2 THEN 0 \
             WHEN name ILIKE $3 THEN 1 \
             ELSE 2 \
           END, \
           name \
         LIMIT $4",
    )
    .bind(&contains)
    .bind(q)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Matches `q` as a case-insensitive substring of either the ATOC code or
/// the operator name, ranked with the same three-tier `CASE` as
/// [`search_stations`] (exact code, then name prefix, then substring):
/// the two functions are the same query shape over the same kind of
/// table, and leaving this one unranked would make the operator field
/// rank e.g. "SW" below whatever sorts first alphabetically among the
/// ~30 operator names, and would make the two functions diverge for no
/// reason. Same trimmed/non-empty contract as [`search_stations`].
pub async fn search_tocs(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let contains = format!("%{q}%");
    let prefix = format!("{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT atoc_code AS code, name FROM tocs \
         WHERE atoc_code ILIKE $1 OR name ILIKE $1 \
         ORDER BY \
           CASE \
             WHEN atoc_code ILIKE $2 THEN 0 \
             WHEN name ILIKE $3 THEN 1 \
             ELSE 2 \
           END, \
           name \
         LIMIT $4",
    )
    .bind(&contains)
    .bind(q)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Returns every TOC, ordered by name -- used where the full reference
/// set is needed up front (e.g. resolving every operator code present
/// in a table) rather than type-ahead search.
pub async fn get_all_tocs(pool: &PgPool) -> Result<Vec<Suggestion>> {
    let rows: Vec<Suggestion> =
        sqlx::query_as("SELECT atoc_code AS code, name FROM tocs ORDER BY name")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

// These tests seed and delete their own rows in a reserved `Z…` CRS/ATOC
// namespace with invented names, rather than reusing real reference data:
// the CI database (`.github/workflows/ci.yml:216`) is freshly migrated and
// empty, so a ranking test has to bring its own fixtures anyway, and a
// developer's local database may hold real reference data that a test
// seeded under a real code could corrupt or that could perturb the
// assertions. Each fixture below stands in for a real-world case named in
// its comment.
#[cfg(test)]
mod db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                search_stations_ranks_exact_code_then_name_prefix_then_substring \
                -- --ignored`"]
    async fn search_stations_ranks_exact_code_then_name_prefix_then_substring() {
        let pool = connect().await;

        // Stands in for "York": ZOR is the exact-code match (tier 0) even
        // though its own name doesn't contain "zor". ZBU/ZAA/ZRK are
        // name-prefix matches (tier 1) standing in for names that start
        // with the query; they land in ZBU, ZAA, ZRK order because
        // alphabetical-within-tier puts the shortest prefix match first
        // ("Zorbury" < "Zork" < "Zorkton Parkway"), which is why no
        // separate exact-name tier is needed (Decision 1). ZZR/ZBY/ZBL are
        // substring-only matches (tier 2) standing in for Bentley (South
        // Yorkshire) / Bramley (West Yorkshire) style buried results.
        let fixtures: [(&str, &str); 7] = [
            ("ZOR", "Somewhere Else"),
            ("ZBU", "Zorbury"),
            ("ZAA", "Zork"),
            ("ZRK", "Zorkton Parkway"),
            ("ZZR", "Ashby-de-la-Zork"),
            ("ZBY", "Bentley (South Zorkshire)"),
            ("ZBL", "Bramley (West Zorkshire)"),
        ];
        for (crs, name) in fixtures {
            sqlx::query(
                "INSERT INTO stations (crs, name) VALUES ($1, $2) \
                 ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name",
            )
            .bind(crs)
            .bind(name)
            .execute(&pool)
            .await
            .expect("seed fixture station");
        }

        // Lowercase query: exercises case-insensitivity and all three
        // tiers in a single call.
        let results = search_stations(&pool, "zor", 20).await.expect("search");
        let codes: Vec<&str> = results.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["ZOR", "ZBU", "ZAA", "ZRK", "ZZR", "ZBY", "ZBL"],
            "full sequence, not just membership -- the defect being fixed is ordering"
        );

        for (crs, _) in fixtures {
            sqlx::query("DELETE FROM stations WHERE crs = $1")
                .bind(crs)
                .execute(&pool)
                .await
                .expect("cleanup fixture station");
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                search_stations_does_not_truncate_the_exact_code_match_out_of_the_limit \
                -- --ignored`"]
    async fn search_stations_does_not_truncate_the_exact_code_match_out_of_the_limit() {
        let pool = connect().await;

        // Under the old `ORDER BY name`, all 22 fillers below sort ahead
        // of "Somewhere Else" alphabetically, so the exact-code row falls
        // outside `LIMIT 20` entirely and `getStationName`
        // (frontend/lib/api.ts:115-121), which filters this response for
        // an exact code match, silently returns `null` -- making the UI
        // fall back to a bare code, which is the very thing Tasks 8-9
        // exist to remove. Under the tiered ordering the exact-code row is
        // always row 1 and can never be capped away.
        sqlx::query(
            "INSERT INTO stations (crs, name) VALUES ('ZOR', 'Somewhere Else') \
             ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name",
        )
        .execute(&pool)
        .await
        .expect("seed exact-match station");

        let mut filler_codes = Vec::with_capacity(22);
        for i in 1..=22 {
            let code = format!("Y{i:02}");
            let name = format!("A-Zor Filler {i:02}");
            sqlx::query(
                "INSERT INTO stations (crs, name) VALUES ($1, $2) \
                 ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name",
            )
            .bind(&code)
            .bind(&name)
            .execute(&pool)
            .await
            .expect("seed filler station");
            filler_codes.push(code);
        }

        let results = search_stations(&pool, "zor", 20).await.expect("search");
        assert_eq!(
            results.first().map(|r| r.code.as_str()),
            Some("ZOR"),
            "the exact-code match must be row 1, not capped out by 22 alphabetically-earlier \
             substring matches"
        );

        sqlx::query("DELETE FROM stations WHERE crs = 'ZOR'")
            .execute(&pool)
            .await
            .expect("cleanup exact-match station");
        for code in filler_codes {
            sqlx::query("DELETE FROM stations WHERE crs = $1")
                .bind(&code)
                .execute(&pool)
                .await
                .expect("cleanup filler station");
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                search_tocs_ranks_exact_code_then_name_prefix_then_substring \
                -- --ignored`"]
    async fn search_tocs_ranks_exact_code_then_name_prefix_then_substring() {
        let pool = connect().await;

        // `atoc_code` is CHAR(2), so the exact-code tier needs a 2-char
        // query: "zz" against ZZ (tier 0, code is an exact case-insensitive
        // match), a name starting with "Zz" (tier 1, name-prefix match),
        // and a name that only contains "zz" as a substring (tier 2).
        // `legal_name` is `NOT NULL`, so it is supplied even though
        // `search_tocs` never selects it.
        let fixtures: [(&str, &str, &str); 3] = [
            ("ZZ", "Somewhere Else Rail", "Somewhere Else Rail Ltd"),
            ("ZY", "Zzebra Trains", "Zzebra Trains Ltd"),
            (
                "ZA",
                "Amalgamated Zzebra Holdings",
                "Amalgamated Zzebra Holdings Ltd",
            ),
        ];
        for (code, name, legal_name) in fixtures {
            sqlx::query(
                "INSERT INTO tocs (atoc_code, name, legal_name) VALUES ($1, $2, $3) \
                 ON CONFLICT (atoc_code) DO UPDATE SET name = EXCLUDED.name, \
                 legal_name = EXCLUDED.legal_name",
            )
            .bind(code)
            .bind(name)
            .bind(legal_name)
            .execute(&pool)
            .await
            .expect("seed fixture toc");
        }

        let results = search_tocs(&pool, "zz", 20).await.expect("search");
        // ZZ's code is an exact case-insensitive match for "zz" (tier 0);
        // ZY's name "Zzebra Trains" starts with "Zz" (tier 1); ZA's name
        // "Amalgamated Zzebra Holdings" only contains "zz" as a substring
        // (tier 2).
        let codes: Vec<&str> = results.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(codes, vec!["ZZ", "ZY", "ZA"]);

        for (code, _, _) in fixtures {
            sqlx::query("DELETE FROM tocs WHERE atoc_code = $1")
                .bind(code)
                .execute(&pool)
                .await
                .expect("cleanup fixture toc");
        }
    }
}
