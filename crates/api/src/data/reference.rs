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

/// A station returned by [`nearest_stations`]: the same `code`/`name` shape
/// as [`Suggestion`] plus the great-circle distance from the caller's point,
/// in kilometres (this app has no other established distance unit anywhere
/// in its schema or docs -- see `docs/superpowers/specs/2026-08-31-other-uk-
/// transit-networks-research.md` and `2026-09-05-ireland-vs-northern-
/// ireland-friction-research.md`, both of which quote UK/Ireland network
/// distances in km -- so km is the default for a UK-facing app rather than
/// miles).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NearbyStation {
    pub code: String,
    pub name: String,
    pub distance_km: f64,
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

/// Returns the `limit` stations nearest to (`lat`, `lon`), nearest first,
/// using the standard Haversine great-circle formula evaluated directly in
/// SQL -- no PostGIS/geo extension is installed anywhere in this codebase,
/// and at ~2,500 UK stations a full-table scan with `ORDER BY distance
/// LIMIT n` is well within budget without a bounding box or spatial index
/// (see docs/superpowers/specs/2026-09-21-near-me-station-lookup-design.md).
///
/// Stations with a `NULL` `latitude` or `longitude` (not every RDM/NRE
/// reference row carries coordinates) are excluded via the `WHERE` clause
/// rather than surfaced with a nonsense distance.
///
/// `lat`/`lon` are the caller's position in degrees; validating they're
/// finite and within the plausible [-90, 90]/[-180, 180] ranges is the
/// route layer's job (`routes::reference::normalize_coordinate`), not
/// this function's -- this function trusts its callers, matching
/// `search_stations`'s trust of its own already-sanitized `q`.
pub async fn nearest_stations(
    pool: &PgPool,
    lat: f64,
    lon: f64,
    limit: i64,
) -> Result<Vec<NearbyStation>> {
    // Haversine distance in km, Earth radius 6371 km. `LEAST` clamps the
    // `asin` argument to <= 1: without this, floating-point rounding on a
    // point very close to (or exactly at) a station's own coordinates can
    // push the intermediate value fractionally past 1, and `asin` of an
    // out-of-domain input is a Postgres runtime error, not a
    // merely-inaccurate result. No lower-bound clamp is needed: the
    // argument is `sqrt(sin(..)^2 + cos(..)*cos(..)*sin(..)^2)`, a sum of
    // squares under a square root, which is always >= 0.
    let rows: Vec<NearbyStation> = sqlx::query_as(
        "SELECT crs AS code, name, \
           2 * 6371 * asin(LEAST(1.0, sqrt( \
             sin(radians(($1::double precision - latitude) / 2)) ^ 2 + \
             cos(radians(latitude)) * cos(radians($1::double precision)) * \
             sin(radians(($2::double precision - longitude) / 2)) ^ 2 \
           ))) AS distance_km \
         FROM stations \
         WHERE latitude IS NOT NULL AND longitude IS NOT NULL \
         ORDER BY distance_km ASC \
         LIMIT $3",
    )
    .bind(lat)
    .bind(lon)
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

/// Top-level `stations.accessibility` keys considered "accessibility or
/// amenities" data for the station page's facilities section -- see
/// docs/superpowers/specs/2026-09-12-station-accessibility-design.md
/// Decision 1 for why this list and not the full RDM `Station` object.
/// Each value is forwarded completely unexamined by
/// [`filter_accessibility_fields`]: this only filters by key name, it does
/// not decompose or validate what's inside (Global Constraint 7,
/// docs/superpowers/plans/01-poller-microservices.md:40-42).
pub const ACCESSIBILITY_KEYS: &[&str] = &[
    "stationAccessibility",
    "staffAssistance",
    "toiletsAndChanging",
    "lifts",
    "transportLinks",
    "cycling",
    "carParks",
    "dropOffPickUp",
    "platformFacilities",
    "stationFacilities",
    "helpAndSupport",
    "loungesAndWaiting",
];

/// The in-memory filter step, pulled out as its own pure function so it can
/// be unit-tested without a database (see `accessibility_filter_tests`
/// below). Drops every key not in [`ACCESSIBILITY_KEYS`] and every
/// allowlisted key whose value is JSON `null`. `full` is not required to be
/// a JSON object -- a non-object input (which should never happen for this
/// column, but is not asserted against) produces an empty object, the same
/// as an object with no allowlisted keys present.
pub(crate) fn filter_accessibility_fields(full: &serde_json::Value) -> serde_json::Value {
    let mut filtered = serde_json::Map::new();
    if let Some(obj) = full.as_object() {
        for key in ACCESSIBILITY_KEYS {
            if let Some(value) = obj.get(*key).filter(|value| !value.is_null()) {
                filtered.insert((*key).to_string(), value.clone());
            }
        }
    }
    serde_json::Value::Object(filtered)
}

/// Returns `None` when `stations` has no row for `crs` at all (this app has
/// never captured reference data for it) -- distinct from `Some` of an
/// empty object, which means the row exists but none of
/// [`ACCESSIBILITY_KEYS`] were present (or all were null) in its
/// `accessibility` JSONB. Those are two genuinely different, separately
/// representable database states (the column is `NOT NULL DEFAULT '{}'`),
/// and the route above them keeps them apart as `404` vs `200 {}`.
///
/// Exact `crs = $1` match, no case normalization -- same convention
/// `latest_station_sample` (`crates/api/src/data/queries.rs`) already uses
/// for a single-CRS lookup; this function does not introduce or fix
/// case-sensitivity handling either way (see the design spec's Open
/// questions/risks).
pub async fn station_accessibility(pool: &PgPool, crs: &str) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query("SELECT accessibility FROM stations WHERE crs = $1")
        .bind(crs)
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let full: serde_json::Value = row.try_get("accessibility")?;
    Ok(Some(filter_accessibility_fields(&full)))
}

/// Pure, no-database coverage of the [`ACCESSIBILITY_KEYS`] filter --
/// deliberately not in `db_tests` below, so the allowlist's behaviour is
/// asserted on every ordinary `cargo test -p api` run rather than only
/// under `--ignored` with a live Postgres.
#[cfg(test)]
mod accessibility_filter_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn forwards_an_allowlisted_key_that_is_present() {
        let full = json!({ "lifts": { "count": 2 }, "somethingElse": "ignored" });
        let filtered = filter_accessibility_fields(&full);
        assert_eq!(filtered, json!({ "lifts": { "count": 2 } }));
    }

    #[test]
    fn drops_a_non_allowlisted_key_even_though_it_is_present() {
        let full = json!({ "ticketBuying": { "open": true } });
        let filtered = filter_accessibility_fields(&full);
        assert_eq!(filtered, json!({}));
    }

    #[test]
    fn drops_a_null_valued_allowlisted_key() {
        let full = json!({ "carParks": null, "lifts": { "count": 1 } });
        let filtered = filter_accessibility_fields(&full);
        assert_eq!(filtered, json!({ "lifts": { "count": 1 } }));
    }

    #[test]
    fn an_empty_accessibility_object_produces_an_empty_object() {
        let full = json!({});
        let filtered = filter_accessibility_fields(&full);
        assert_eq!(filtered, json!({}));
    }

    /// The column is `NOT NULL DEFAULT '{}'` so this should be unreachable,
    /// but the filter is documented as total over any `Value` -- a
    /// non-object must degrade to `{}`, never panic.
    #[test]
    fn a_non_object_accessibility_value_produces_an_empty_object() {
        assert_eq!(filter_accessibility_fields(&json!(null)), json!({}));
        assert_eq!(filter_accessibility_fields(&json!("nonsense")), json!({}));
        assert_eq!(filter_accessibility_fields(&json!([1, 2, 3])), json!({}));
    }

    #[test]
    fn forwards_every_allowlisted_key_present_and_drops_everything_else_in_one_pass() {
        let full = json!({
            "stationAccessibility": { "stepFree": true },
            "staffAssistance": "Available 06:00-23:00",
            "toiletsAndChanging": null,
            "lifts": [{ "location": "Platform 1" }],
            "transportLinks": ["Bus", "Underground"],
            "cycling": {},
            "carParks": [{ "spaces": 120 }],
            "dropOffPickUp": null,
            "platformFacilities": { "seating": true },
            "stationFacilities": { "wifi": true },
            "helpAndSupport": "0800 123 4567",
            "loungesAndWaiting": { "firstClass": false },
            "ticketBuying": { "open": true },
            "staffingLevel": "Full",
            "informationServices": {},
            "address": { "line1": "1 Station Rd" },
            "stationMap": "https://example.invalid/map.png",
            "stationAlerts": [],
            "slug": "example-station",
            "sixteenCharacterName": "EXAMPLE STN",
            "nationalLocationCode": "1234",
            "minimumConnectionTime": 5,
            "changeHistory": { "changedBy": "AAP2" }
        });
        let filtered = filter_accessibility_fields(&full);
        assert_eq!(
            filtered,
            json!({
                "stationAccessibility": { "stepFree": true },
                "staffAssistance": "Available 06:00-23:00",
                "lifts": [{ "location": "Platform 1" }],
                "transportLinks": ["Bus", "Underground"],
                "cycling": {},
                "carParks": [{ "spaces": 120 }],
                "platformFacilities": { "seating": true },
                "stationFacilities": { "wifi": true },
                "helpAndSupport": "0800 123 4567",
                "loungesAndWaiting": { "firstClass": false }
            }),
            "every non-allowlisted key dropped, every null-valued allowlisted key dropped, \
             every present-and-non-null allowlisted key forwarded verbatim"
        );
    }

    /// Guards the one thing the spec fixes by name (Decision 1): the
    /// allowlist is exactly these twelve keys. A thirteenth key silently
    /// added here would start shipping unvetted RDM data to every visitor.
    #[test]
    fn the_allowlist_is_exactly_the_twelve_spec_named_keys() {
        assert_eq!(
            ACCESSIBILITY_KEYS,
            &[
                "stationAccessibility",
                "staffAssistance",
                "toiletsAndChanging",
                "lifts",
                "transportLinks",
                "cycling",
                "carParks",
                "dropOffPickUp",
                "platformFacilities",
                "stationFacilities",
                "helpAndSupport",
                "loungesAndWaiting",
            ]
        );
    }
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
                nearest_stations_orders_nearest_first_and_excludes_null_coordinates \
                -- --ignored`"]
    async fn nearest_stations_orders_nearest_first_and_excludes_null_coordinates() {
        let pool = connect().await;

        // Woking-ish reference point. ZNA is a few hundred metres away,
        // ZNB a few km away, ZNC much further, and ZND has no coordinates
        // at all -- standing in for the real-world case where an RDM
        // reference row simply never got a lat/lon populated.
        // crs, name, (latitude, longitude).
        type StationFixture = (&'static str, &'static str, Option<(f64, f64)>);
        let fixtures: [StationFixture; 4] = [
            ("ZNA", "Near Fixture Station", Some((51.3200, -0.5600))),
            ("ZNB", "Middling Fixture Station", Some((51.3600, -0.5200))),
            ("ZNC", "Far Fixture Station", Some((52.4800, -1.9000))),
            ("ZND", "No Coordinates Fixture Station", None),
        ];
        for (crs, name, coords) in fixtures {
            let (lat, lon) = coords.map_or((None, None), |(lat, lon)| (Some(lat), Some(lon)));
            sqlx::query(
                "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name, \
                 latitude = EXCLUDED.latitude, longitude = EXCLUDED.longitude",
            )
            .bind(crs)
            .bind(name)
            .bind(lat)
            .bind(lon)
            .execute(&pool)
            .await
            .expect("seed fixture station");
        }

        let results = nearest_stations(&pool, 51.3191, -0.5610, 20)
            .await
            .expect("nearest_stations query");
        let codes: Vec<&str> = results
            .iter()
            .filter(|r| {
                r.code.starts_with('Z') && r.code.len() == 3 && r.code.as_bytes()[1] == b'N'
            })
            .map(|r| r.code.as_str())
            .collect();
        assert_eq!(
            codes,
            vec!["ZNA", "ZNB", "ZNC"],
            "nearest first, and ZND (null coordinates) must never appear"
        );
        assert!(
            results.iter().all(|r| r.code != "ZND"),
            "a station with null lat/lon must be excluded entirely, not returned with a bogus \
             distance"
        );
        let distances: Vec<f64> = results
            .iter()
            .filter(|r| ["ZNA", "ZNB", "ZNC"].contains(&r.code.as_str()))
            .map(|r| r.distance_km)
            .collect();
        assert!(
            distances.windows(2).all(|w| w[0] <= w[1]),
            "distances must be non-decreasing: {distances:?}"
        );
        assert!(
            distances[0] < 1.0,
            "ZNA is a few hundred metres from the reference point: {distances:?}"
        );

        for (crs, _, _) in fixtures {
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
                nearest_stations_respects_the_limit \
                -- --ignored`"]
    async fn nearest_stations_respects_the_limit() {
        let pool = connect().await;

        let fixtures: [(&str, &str, f64, f64); 3] = [
            ("ZLA", "Limit Fixture A", 51.30, -0.50),
            ("ZLB", "Limit Fixture B", 51.31, -0.51),
            ("ZLC", "Limit Fixture C", 51.32, -0.52),
        ];
        for (crs, name, lat, lon) in fixtures {
            sqlx::query(
                "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name, \
                 latitude = EXCLUDED.latitude, longitude = EXCLUDED.longitude",
            )
            .bind(crs)
            .bind(name)
            .bind(lat)
            .bind(lon)
            .execute(&pool)
            .await
            .expect("seed fixture station");
        }

        let results = nearest_stations(&pool, 51.30, -0.50, 2)
            .await
            .expect("nearest_stations query");
        assert_eq!(results.len(), 2, "limit=2 must return at most 2 rows");

        for (crs, _, _, _) in fixtures {
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                station_accessibility_returns_the_filtered_set_for_an_existing_row \
                -- --ignored`"]
    async fn station_accessibility_returns_the_filtered_set_for_an_existing_row() {
        let pool = connect().await;

        sqlx::query(
            "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
             ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
        )
        .bind("ZFA")
        .bind("Fixture Facilities Station")
        .bind(serde_json::json!({
            "lifts": { "count": 2 },
            "carParks": null,
            "ticketBuying": { "open": true }
        }))
        .execute(&pool)
        .await
        .expect("seed fixture station");

        let result = station_accessibility(&pool, "ZFA").await.expect("query");
        assert_eq!(
            result,
            Some(serde_json::json!({ "lifts": { "count": 2 } })),
            "carParks (null) and ticketBuying (non-allowlisted) must both be absent"
        );

        sqlx::query("DELETE FROM stations WHERE crs = 'ZFA'")
            .execute(&pool)
            .await
            .expect("cleanup fixture station");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                station_accessibility_returns_some_empty_object_for_a_row_with_no_allowlisted_keys \
                -- --ignored`"]
    async fn station_accessibility_returns_some_empty_object_for_a_row_with_no_allowlisted_keys() {
        let pool = connect().await;

        sqlx::query(
            "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
             ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
        )
        .bind("ZFF")
        .bind("Fixture Quiet Reference Station")
        .bind(serde_json::json!({ "ticketBuying": { "open": true } }))
        .execute(&pool)
        .await
        .expect("seed fixture station");

        let result = station_accessibility(&pool, "ZFF").await.expect("query");
        assert_eq!(
            result,
            Some(serde_json::json!({})),
            "a row that exists but has no allowlisted keys is Some({{}}), never None -- the \
             404-vs-200-{{}} split above this depends on the two staying distinct"
        );

        sqlx::query("DELETE FROM stations WHERE crs = 'ZFF'")
            .execute(&pool)
            .await
            .expect("cleanup fixture station");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                station_accessibility_returns_none_when_no_row_exists_for_the_crs \
                -- --ignored`"]
    async fn station_accessibility_returns_none_when_no_row_exists_for_the_crs() {
        let pool = connect().await;
        sqlx::query("DELETE FROM stations WHERE crs = 'ZFB'")
            .execute(&pool)
            .await
            .expect("ensure no fixture row present");

        let result = station_accessibility(&pool, "ZFB").await.expect("query");
        assert_eq!(
            result, None,
            "no stations row at all must be None, not Some({{}})"
        );
    }
}
