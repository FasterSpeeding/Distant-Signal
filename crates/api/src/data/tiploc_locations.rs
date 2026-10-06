//! `tiploc_locations`: every TIPLOC's display name, location type and (for
//! a bus stop or ferry terminal) parent station, as `schedule-reference`
//! publishes them (migration `20261007100000_tiploc_locations.sql`). Read
//! by the journey stop renderers, the station-board destinations, the
//! planner's bus-stop and ferry end points and the location search.
//!
//! A TIPLOC with no row (a `TI` record newer than the last publish, or
//! before the first) falls back to its CORPUS description, named and
//! classified by the same [`common::location_naming`] rules
//! `schedule-reference` uses. See
//! docs/superpowers/specs/2026-10-06-tiploc-locations-design.md.

use std::collections::HashMap;

use anyhow::Result;
use common::location_naming::{self, LocationType};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::queries::normalize_code;

/// Replaces the whole table with `records` in one transaction: upsert by
/// TIPLOC (rows that did not change keep their `updated_at`), then delete
/// every TIPLOC `records` does not list. An empty `records` is refused by
/// the route before this is reached; here it is a no-op, never a wipe.
pub async fn replace_tiploc_locations(
    pool: &PgPool,
    records: &[common::TiplocLocationRecord],
) -> Result<u64> {
    if records.is_empty() {
        return Ok(0);
    }
    let batch = crate::data::queries::last_per_key(records, |r| normalize_code(&r.tiploc));
    let col = |f: fn(&common::TiplocLocationRecord) -> Option<String>| -> Vec<Option<String>> {
        batch.iter().map(|r| f(r)).collect()
    };
    let int = |f: fn(&common::TiplocLocationRecord) -> Option<i32>| -> Vec<Option<i32>> {
        batch.iter().map(|r| f(r)).collect()
    };
    let tiploc: Vec<String> = batch.iter().map(|r| normalize_code(&r.tiploc)).collect();
    let location_type: Vec<&str> = batch.iter().map(|r| r.location_type.as_str()).collect();
    let name: Vec<&str> = batch.iter().map(|r| r.name.as_str()).collect();
    let display_name: Vec<&str> = batch.iter().map(|r| r.display_name.as_str()).collect();
    let parent_crs: Vec<Option<String>> = batch
        .iter()
        .map(|r| r.parent_crs.as_deref().map(normalize_code))
        .collect();
    let parent_source: Vec<Option<&str>> = batch
        .iter()
        .map(|r| r.parent_source.map(common::ParentSource::as_str))
        .collect();
    let counts = |f: fn(&common::TiplocLocationRecord) -> i32| -> Vec<i32> {
        batch.iter().map(|r| f(r)).collect()
    };

    let mut tx = pool.begin().await?;
    sqlx::query(
        r"
        INSERT INTO tiploc_locations (
            tiploc, location_type, name, display_name, ti_name, ti_crs, stanox,
            msn_name, msn_code, msn_easting, msn_northing, msn_interchange,
            parent_crs, parent_source, parent_distance_m,
            rail_calls, rail_passes, bus_calls, ship_calls, source_sequence, updated_at)
        SELECT tiploc, location_type, name, display_name, ti_name, ti_crs, stanox,
               msn_name, msn_code, msn_easting, msn_northing, msn_interchange,
               parent_crs, parent_source, parent_distance_m,
               rail_calls, rail_passes, bus_calls, ship_calls, source_sequence, NOW()
        FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[],
                    $7::text[], $8::text[], $9::text[], $10::int4[], $11::int4[], $12::int4[],
                    $13::text[], $14::text[], $15::int4[], $16::int4[], $17::int4[],
                    $18::int4[], $19::int4[], $20::int4[])
            AS i(tiploc, location_type, name, display_name, ti_name, ti_crs, stanox,
                 msn_name, msn_code, msn_easting, msn_northing, msn_interchange,
                 parent_crs, parent_source, parent_distance_m,
                 rail_calls, rail_passes, bus_calls, ship_calls, source_sequence)
        ON CONFLICT (tiploc) DO UPDATE SET
            location_type = EXCLUDED.location_type,
            name = EXCLUDED.name,
            display_name = EXCLUDED.display_name,
            ti_name = EXCLUDED.ti_name,
            ti_crs = EXCLUDED.ti_crs,
            stanox = EXCLUDED.stanox,
            msn_name = EXCLUDED.msn_name,
            msn_code = EXCLUDED.msn_code,
            msn_easting = EXCLUDED.msn_easting,
            msn_northing = EXCLUDED.msn_northing,
            msn_interchange = EXCLUDED.msn_interchange,
            parent_crs = EXCLUDED.parent_crs,
            parent_source = EXCLUDED.parent_source,
            parent_distance_m = EXCLUDED.parent_distance_m,
            rail_calls = EXCLUDED.rail_calls,
            rail_passes = EXCLUDED.rail_passes,
            bus_calls = EXCLUDED.bus_calls,
            ship_calls = EXCLUDED.ship_calls,
            source_sequence = EXCLUDED.source_sequence,
            updated_at = NOW()
        WHERE (tiploc_locations.location_type, tiploc_locations.name,
               tiploc_locations.display_name, tiploc_locations.ti_name,
               tiploc_locations.ti_crs, tiploc_locations.stanox, tiploc_locations.msn_name,
               tiploc_locations.msn_code, tiploc_locations.msn_easting,
               tiploc_locations.msn_northing, tiploc_locations.msn_interchange,
               tiploc_locations.parent_crs, tiploc_locations.parent_source,
               tiploc_locations.parent_distance_m, tiploc_locations.rail_calls,
               tiploc_locations.rail_passes, tiploc_locations.bus_calls,
               tiploc_locations.ship_calls, tiploc_locations.source_sequence)
              IS DISTINCT FROM
              (EXCLUDED.location_type, EXCLUDED.name, EXCLUDED.display_name,
               EXCLUDED.ti_name, EXCLUDED.ti_crs, EXCLUDED.stanox, EXCLUDED.msn_name,
               EXCLUDED.msn_code, EXCLUDED.msn_easting, EXCLUDED.msn_northing,
               EXCLUDED.msn_interchange, EXCLUDED.parent_crs, EXCLUDED.parent_source,
               EXCLUDED.parent_distance_m, EXCLUDED.rail_calls, EXCLUDED.rail_passes,
               EXCLUDED.bus_calls, EXCLUDED.ship_calls, EXCLUDED.source_sequence)
        ",
    )
    .bind(&tiploc)
    .bind(&location_type)
    .bind(&name)
    .bind(&display_name)
    .bind(col(|r| r.ti_name.clone()))
    .bind(col(|r| r.ti_crs.clone()))
    .bind(col(|r| r.stanox.clone()))
    .bind(col(|r| r.msn_name.clone()))
    .bind(col(|r| r.msn_code.clone()))
    .bind(int(|r| r.msn_easting))
    .bind(int(|r| r.msn_northing))
    .bind(int(|r| r.msn_interchange))
    .bind(&parent_crs)
    .bind(&parent_source)
    .bind(int(|r| r.parent_distance_m))
    .bind(counts(|r| r.rail_calls))
    .bind(counts(|r| r.rail_passes))
    .bind(counts(|r| r.bus_calls))
    .bind(counts(|r| r.ship_calls))
    .bind(counts(|r| r.source_sequence))
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM tiploc_locations WHERE NOT (tiploc = ANY($1))")
        .bind(&tiploc)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(batch.len() as u64)
}

/// A TIPLOC's name and kind, as the stop renderers and the planner serve
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub tiploc: String,
    /// Place name without the kind of stop (`Heathrow Terminal 3`).
    pub name: String,
    /// Passenger-facing name (`Heathrow Terminal 3 (bus stop)`).
    pub display_name: String,
    pub location_type: LocationType,
    /// The parent station's CRS, only when `stations` has it.
    pub parent_crs: Option<String>,
    /// That station's name.
    pub parent_name: Option<String>,
}

impl LocationInfo {
    /// The planner and search identifier: `tiploc:SANWBUS`.
    pub fn code(&self) -> String {
        location_naming::tiploc_code(&self.tiploc)
    }

    /// `Keswick (bus)` / `Brodick (ferry)`, the location search's label.
    pub fn search_label(&self) -> String {
        location_naming::search_label(&self.name, self.location_type)
    }
}

#[derive(sqlx::FromRow)]
struct LocationRow {
    tiploc: String,
    name: String,
    display_name: String,
    location_type: String,
    parent_crs: Option<String>,
    parent_name: Option<String>,
}

impl From<LocationRow> for LocationInfo {
    fn from(row: LocationRow) -> Self {
        LocationInfo {
            tiploc: row.tiploc,
            name: row.name,
            display_name: row.display_name,
            location_type: LocationType::parse(&row.location_type).unwrap_or(LocationType::Other),
            parent_name: row.parent_crs.as_ref().and(row.parent_name),
            parent_crs: row.parent_crs,
        }
    }
}

/// The columns every read selects; `parent_crs` is blanked when `stations`
/// has no such row.
const SELECT_LOCATION: &str = "SELECT l.tiploc, l.name, l.display_name, l.location_type, \
            s.crs::text AS parent_crs, s.name AS parent_name \
     FROM tiploc_locations l LEFT JOIN stations s ON s.crs = UPPER(l.parent_crs)";

/// The CORPUS fallback for a TIPLOC with no `tiploc_locations` row: its
/// CORPUS description, classified by name alone (a TIPLOC CORPUS gives a
/// CRS for is a station), never with a parent.
fn corpus_fallback(tiploc: &str, description: &str, crs: Option<&str>) -> LocationInfo {
    let location_type = if crs.is_some_and(crate::data::queries::is_bookable_crs) {
        LocationType::Station
    } else {
        location_naming::classify_name(description).unwrap_or(LocationType::Other)
    };
    let names = location_naming::location_name(description, location_type);
    LocationInfo {
        tiploc: tiploc.to_string(),
        name: names.name,
        display_name: names.display_name,
        location_type,
        parent_crs: None,
        parent_name: None,
    }
}

/// `tiploc_locations` for every TIPLOC in `tiplocs` (keys normalised as
/// `journey::tiploc_key` builds them: trimmed, upper case), CORPUS for any
/// it lacks; a TIPLOC in neither is absent.
pub async fn locations_for_tiplocs(
    pool: &PgPool,
    tiplocs: &[String],
) -> Result<HashMap<String, LocationInfo>> {
    if tiplocs.is_empty() {
        return Ok(HashMap::new());
    }
    let keys: Vec<String> = tiplocs.iter().map(|t| normalize_code(t)).collect();
    let rows: Vec<LocationRow> =
        sqlx::query_as(&format!("{SELECT_LOCATION} WHERE l.tiploc = ANY($1)"))
            .bind(&keys)
            .fetch_all(pool)
            .await?;
    let mut out: HashMap<String, LocationInfo> = rows
        .into_iter()
        .map(|row| (row.tiploc.clone(), LocationInfo::from(row)))
        .collect();
    let missing: Vec<&String> = keys.iter().filter(|key| !out.contains_key(*key)).collect();
    if !missing.is_empty() {
        let corpus: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT DISTINCT ON (tiploc) tiploc, nlc_desc, crs FROM corpus_locations \
             WHERE tiploc = ANY($1) AND nlc_desc IS NOT NULL AND nlc_desc <> '' \
             ORDER BY tiploc, crs NULLS LAST, nlc",
        )
        .bind(&missing)
        .fetch_all(pool)
        .await?;
        for (tiploc, description, crs) in corpus {
            let info = corpus_fallback(&tiploc, &description, crs.as_deref());
            out.insert(tiploc, info);
        }
    }
    Ok(out)
}

/// Every bus stop and ferry terminal, by TIPLOC: the planner's extra end
/// points.
pub async fn road_or_water_locations(pool: &PgPool) -> Result<Vec<LocationInfo>> {
    let rows: Vec<LocationRow> = sqlx::query_as(&format!(
        "{SELECT_LOCATION} WHERE l.location_type IN ('bus_stop', 'ferry_terminal') ORDER BY l.tiploc"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(LocationInfo::from).collect())
}

/// One bus stop or ferry terminal, by `tiploc:` code; `None` for anything
/// else.
pub async fn road_or_water_location(pool: &PgPool, code: &str) -> Result<Option<LocationInfo>> {
    let Some(tiploc) = location_naming::tiploc_from_code(code) else {
        return Ok(None);
    };
    let row: Option<LocationRow> = sqlx::query_as(&format!(
        "{SELECT_LOCATION} WHERE l.tiploc = $1 AND l.location_type IN ('bus_stop', 'ferry_terminal')"
    ))
    .bind(&tiploc)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(LocationInfo::from))
}

/// Bus stops and ferry terminals whose name, display name, TIPLOC or
/// `tiploc:` code contains `q` (already trimmed, non-empty), served by at
/// least one bus or ship -- a stop no service uses is no planner end point.
/// Name-prefix matches first, then alphabetical.
pub async fn search_road_or_water(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<LocationInfo>> {
    let escaped = crate::data::reference::escape_ilike_pattern(q);
    let contains = format!("%{escaped}%");
    let prefix = format!("{escaped}%");
    let rows: Vec<LocationRow> = sqlx::query_as(&format!(
        "{SELECT_LOCATION} \
         WHERE l.location_type IN ('bus_stop', 'ferry_terminal') \
           AND (l.bus_calls > 0 OR l.ship_calls > 0) \
           AND (l.name ILIKE $1 ESCAPE '\\' OR l.display_name ILIKE $1 ESCAPE '\\' \
                OR l.tiploc ILIKE $1 ESCAPE '\\' OR ('tiploc:' || l.tiploc) ILIKE $1 ESCAPE '\\') \
         ORDER BY CASE WHEN ('tiploc:' || l.tiploc) ILIKE $2 ESCAPE '\\' THEN 0 \
                       WHEN l.name ILIKE $3 ESCAPE '\\' THEN 1 ELSE 2 END, \
                  l.name, l.tiploc \
         LIMIT $4"
    ))
    .bind(&contains)
    .bind(&escaped)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(LocationInfo::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_fallback_names_and_classifies_by_description() {
        let stop = corpus_fallback("DUNSRSTP", "DUNSTER STEEP BUS STOP", None);
        assert_eq!(stop.location_type, LocationType::BusStop);
        assert_eq!(stop.display_name, "Dunster Steep (bus stop)");
        assert_eq!(stop.parent_crs, None);
        let signal = corpus_fallback("MARY10", "MARYLEBONE 10 SIGNAL", None);
        assert_eq!(signal.location_type, LocationType::PassingPoint);
        assert_eq!(signal.display_name, "Marylebone 10 Signal");
        let station = corpus_fallback("LEUCHRS", "LEUCHARS", Some("LEU"));
        assert_eq!(station.location_type, LocationType::Station);
        let pseudo = corpus_fallback("VICTRCR", "VICTORIA CARRIAGE ROAD", Some("XVR"));
        assert_eq!(pseudo.location_type, LocationType::Other);
    }

    #[test]
    fn destination_keys_name_their_tiploc() {
        use crate::data::queries::tiploc_of_destination_key;
        assert_eq!(
            tiploc_of_destination_key("tiploc:HTRBUS3").as_deref(),
            Some("HTRBUS3")
        );
        assert_eq!(
            tiploc_of_destination_key("~HTRBUS3").as_deref(),
            Some("HTRBUS3")
        );
        assert_eq!(tiploc_of_destination_key("KGX"), None);
        assert_eq!(tiploc_of_destination_key("~"), None);
    }

    #[test]
    fn a_location_has_a_tiploc_code_and_a_search_label() {
        let info = LocationInfo {
            tiploc: "KESWICK".to_string(),
            name: "Keswick".to_string(),
            display_name: "Keswick (bus station)".to_string(),
            location_type: LocationType::BusStop,
            parent_crs: None,
            parent_name: None,
        };
        assert_eq!(info.code(), "tiploc:KESWICK");
        assert_eq!(info.search_label(), "Keswick (bus)");
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use common::{ParentSource, TiplocLocationRecord};

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for db_tests");
        PgPool::connect(&url)
            .await
            .expect("connect to test database")
    }

    fn record(tiploc: &str, kind: LocationType, display: &str) -> TiplocLocationRecord {
        let names = location_naming::location_name(display, kind);
        TiplocLocationRecord {
            tiploc: tiploc.to_string(),
            location_type: kind,
            name: names.name,
            display_name: names.display_name,
            ti_name: Some(display.to_string()),
            ti_crs: None,
            stanox: None,
            msn_name: None,
            msn_code: None,
            msn_easting: None,
            msn_northing: None,
            msn_interchange: None,
            parent_crs: None,
            parent_source: None,
            parent_distance_m: None,
            rail_calls: 0,
            rail_passes: 0,
            bus_calls: i32::from(kind == LocationType::BusStop),
            ship_calls: i32::from(kind == LocationType::FerryTerminal),
            source_sequence: 980,
        }
    }

    /// Seeds two stations and three locations (one parented on a real
    /// `stations` row, one on a CRS `stations` lacks) under a `ZT` prefix.
    pub(crate) async fn seed(pool: &PgPool) {
        sqlx::query(
            "INSERT INTO stations (crs, name) VALUES ('ZTR', 'Zed Town Reading') \
             ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name",
        )
        .execute(pool)
        .await
        .expect("seed station");
        let mut bus = record("ZTRBUS", LocationType::BusStop, "ZED TOWN BUS");
        bus.parent_crs = Some("ZTR".to_string());
        bus.parent_source = Some(ParentSource::Nearest);
        bus.parent_distance_m = Some(141);
        let mut orphan = record("ZTWBUS", LocationType::BusStop, "ZED WATCHET BUS");
        orphan.parent_crs = Some("ZZQ".to_string());
        orphan.parent_source = Some(ParentSource::SameTiploc);
        let ferry = record("ZTFERRY", LocationType::FerryTerminal, "ZED ISLAND");
        replace_tiploc_locations(pool, &[bus, orphan, ferry])
            .await
            .expect("replace");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api tiploc_locations -- --ignored --test-threads=1`"]
    async fn tiploc_locations_replace_upserts_prunes_and_reads_back() {
        let pool = connect().await;
        seed(&pool).await;

        let found = locations_for_tiplocs(
            &pool,
            &[
                "ztrbus".to_string(),
                "ZTWBUS".to_string(),
                "ZTNOPE".to_string(),
            ],
        )
        .await
        .expect("lookup");
        let bus = &found["ZTRBUS"];
        assert_eq!(bus.display_name, "Zed Town (bus stop)");
        assert_eq!(bus.parent_crs.as_deref(), Some("ZTR"));
        assert_eq!(bus.parent_name.as_deref(), Some("Zed Town Reading"));
        // A parent `stations` lacks is not served.
        assert_eq!(found["ZTWBUS"].parent_crs, None);
        assert!(!found.contains_key("ZTNOPE"));

        let search = search_road_or_water(&pool, "zed", 20)
            .await
            .expect("search");
        let labels: Vec<String> = search.iter().map(LocationInfo::search_label).collect();
        assert!(
            labels.contains(&"Zed Island (ferry)".to_string()),
            "{labels:?}"
        );
        assert!(labels.contains(&"Zed Town (bus)".to_string()), "{labels:?}");
        let by_code = search_road_or_water(&pool, "tiploc:ZTFERRY", 20)
            .await
            .expect("search");
        assert_eq!(by_code[0].tiploc, "ZTFERRY");
        assert_eq!(
            road_or_water_location(&pool, "tiploc:ztrbus")
                .await
                .expect("one")
                .map(|l| l.tiploc),
            Some("ZTRBUS".to_string())
        );

        // A later delivery without ZTWBUS and ZTFERRY prunes them.
        replace_tiploc_locations(
            &pool,
            &[record("ZTRBUS", LocationType::BusStop, "ZED TOWN BUS")],
        )
        .await
        .expect("replace again");
        let left: Vec<String> = sqlx::query_scalar(
            "SELECT tiploc FROM tiploc_locations WHERE tiploc LIKE 'ZT%' ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .expect("read");
        assert_eq!(left, ["ZTRBUS"]);
        // An empty batch never wipes the table.
        assert_eq!(replace_tiploc_locations(&pool, &[]).await.expect("noop"), 0);

        sqlx::query("DELETE FROM tiploc_locations WHERE tiploc LIKE 'ZT%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stations WHERE crs = 'ZTR'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Station-board and search destinations keyed by a terminus TIPLOC
    /// (`tiploc:` now, `~` before 2026-10-07) get the location's name.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api tiploc_locations -- --ignored --test-threads=1`"]
    async fn tiploc_locations_name_tiploc_keyed_destinations() {
        let pool = connect().await;
        seed(&pool).await;
        let names = crate::data::queries::station_names_for_crs_batch(
            &pool,
            &[
                "ZTR".to_string(),
                "tiploc:ZTRBUS".to_string(),
                "~ZTFERRY".to_string(),
                "tiploc:ZTNOPE".to_string(),
            ],
        )
        .await
        .expect("names");
        assert_eq!(names["ZTR"], "Zed Town Reading");
        assert_eq!(names["tiploc:ZTRBUS"], "Zed Town (bus stop)");
        assert_eq!(names["TIPLOC:ZTRBUS"], "Zed Town (bus stop)");
        assert_eq!(names["~ZTFERRY"], "Zed Island (ferry terminal)");
        assert!(!names.contains_key("tiploc:ZTNOPE"));
        sqlx::query("DELETE FROM tiploc_locations WHERE tiploc LIKE 'ZT%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stations WHERE crs = 'ZTR'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api tiploc_locations -- --ignored --test-threads=1`"]
    async fn tiploc_locations_fall_back_to_corpus() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO corpus_locations (nlc, tiploc, nlc_desc, delivered_at, source_file) \
             VALUES ('999901', 'ZTCORP', 'ZED STEEP BUS STOP', NOW(), 'test')",
        )
        .execute(&pool)
        .await
        .expect("seed corpus");
        let found = locations_for_tiplocs(&pool, &["ZTCORP".to_string()])
            .await
            .expect("lookup");
        assert_eq!(found["ZTCORP"].display_name, "Zed Steep (bus stop)");
        assert_eq!(found["ZTCORP"].location_type, LocationType::BusStop);
        sqlx::query("DELETE FROM corpus_locations WHERE nlc = '999901'")
            .execute(&pool)
            .await
            .ok();
    }
}
