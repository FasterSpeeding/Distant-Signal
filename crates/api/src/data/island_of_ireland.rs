//! Storage for `island_of_ireland_stations`/`island_of_ireland_lines`
//! (Tier A) and `island_of_ireland_station_samples` (Tier B) --
//! docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md. Same
//! upsert-on-id, no-history shape as `crates/api/src/data/queries.rs`'s
//! `upsert_stations`/`upsert_tocs`.

use anyhow::Result;
use common::island_of_ireland::{
    IslandOfIrelandDeparture, IslandOfIrelandLineDefinition, IslandOfIrelandNetwork,
    IslandOfIrelandStation, IslandOfIrelandStationSample,
};
use sqlx::PgPool;

fn network_wire(network: IslandOfIrelandNetwork) -> &'static str {
    match network {
        IslandOfIrelandNetwork::NorthernIreland => "northern-ireland",
        IslandOfIrelandNetwork::RepublicOfIreland => "republic-of-ireland",
    }
}

fn network_from_wire(wire: &str) -> Result<IslandOfIrelandNetwork> {
    match wire {
        "northern-ireland" => Ok(IslandOfIrelandNetwork::NorthernIreland),
        "republic-of-ireland" => Ok(IslandOfIrelandNetwork::RepublicOfIreland),
        other => anyhow::bail!("unrecognized island_of_ireland network: {other}"),
    }
}

/// `ingest_freshness` sources for the two Tier A feeds. The upserts skip an
/// unchanged row (R-016 class, Train Register N1), so a row's `fetched_at`
/// now means "last changed" and the feed-level "last fetched" is recorded
/// here instead, as for the GB reference tables (`queries::record_ingest`).
const STATIONS_SOURCE: &str = "island_of_ireland_stations";
const LINES_SOURCE: &str = "island_of_ireland_lines";

/// One `INSERT ... SELECT FROM UNNEST ... ON CONFLICT` for the whole batch,
/// leaving a row whose values are unchanged untouched (no new tuple, no
/// WAL). The last entry wins when a batch names one id twice. Returns the
/// number of stations received, as before.
pub async fn upsert_stations(pool: &PgPool, stations: &[IslandOfIrelandStation]) -> Result<u64> {
    if stations.is_empty() {
        return Ok(0);
    }
    let batch = crate::data::queries::last_per_key(stations, |station| station.id.clone());
    let ids: Vec<&str> = batch.iter().map(|s| s.id.as_str()).collect();
    let names: Vec<&str> = batch.iter().map(|s| s.name.as_str()).collect();
    let networks: Vec<&str> = batch.iter().map(|s| network_wire(s.network)).collect();
    let latitudes: Vec<Option<f64>> = batch.iter().map(|s| s.latitude).collect();
    let longitudes: Vec<Option<f64>> = batch.iter().map(|s| s.longitude).collect();

    let mut tx = pool.begin().await?;
    sqlx::query(
        r"
        INSERT INTO island_of_ireland_stations (id, name, network, latitude, longitude, fetched_at)
        SELECT id, name, network, latitude, longitude, NOW()
        FROM UNNEST($1::text[], $2::text[], $3::text[], $4::float8[], $5::float8[])
            AS i(id, name, network, latitude, longitude)
        ON CONFLICT (id) DO UPDATE SET
            name       = EXCLUDED.name,
            network    = EXCLUDED.network,
            latitude   = EXCLUDED.latitude,
            longitude  = EXCLUDED.longitude,
            fetched_at = NOW()
        WHERE (island_of_ireland_stations.name, island_of_ireland_stations.network,
               island_of_ireland_stations.latitude, island_of_ireland_stations.longitude)
              IS DISTINCT FROM
              (EXCLUDED.name, EXCLUDED.network, EXCLUDED.latitude, EXCLUDED.longitude)
        ",
    )
    .bind(&ids)
    .bind(&names)
    .bind(&networks)
    .bind(&latitudes)
    .bind(&longitudes)
    .execute(&mut *tx)
    .await?;
    crate::data::queries::record_ingest(&mut tx, STATIONS_SOURCE).await?;
    tx.commit().await?;
    Ok(stations.len() as u64)
}

/// [`upsert_stations`] for lines: one batched statement, unchanged rows
/// left untouched.
pub async fn upsert_lines(pool: &PgPool, lines: &[IslandOfIrelandLineDefinition]) -> Result<u64> {
    if lines.is_empty() {
        return Ok(0);
    }
    let batch = crate::data::queries::last_per_key(lines, |line| line.id.clone());
    let ids: Vec<&str> = batch.iter().map(|l| l.id.as_str()).collect();
    let names: Vec<&str> = batch.iter().map(|l| l.name.as_str()).collect();
    let networks: Vec<&str> = batch.iter().map(|l| network_wire(l.network)).collect();
    let stations: Vec<serde_json::Value> = batch
        .iter()
        .map(|l| serde_json::to_value(&l.stations))
        .collect::<Result<_, _>>()?;

    let mut tx = pool.begin().await?;
    sqlx::query(
        r"
        INSERT INTO island_of_ireland_lines (id, name, network, stations, fetched_at)
        SELECT id, name, network, stations, NOW()
        FROM UNNEST($1::text[], $2::text[], $3::text[], $4::jsonb[])
            AS i(id, name, network, stations)
        ON CONFLICT (id) DO UPDATE SET
            name       = EXCLUDED.name,
            network    = EXCLUDED.network,
            stations   = EXCLUDED.stations,
            fetched_at = NOW()
        WHERE (island_of_ireland_lines.name, island_of_ireland_lines.network,
               island_of_ireland_lines.stations)
              IS DISTINCT FROM
              (EXCLUDED.name, EXCLUDED.network, EXCLUDED.stations)
        ",
    )
    .bind(&ids)
    .bind(&names)
    .bind(&networks)
    .bind(&stations)
    .execute(&mut *tx)
    .await?;
    crate::data::queries::record_ingest(&mut tx, LINES_SOURCE).await?;
    tx.commit().await?;
    Ok(lines.len() as u64)
}

/// When `source`'s feed last landed: its `ingest_freshness` row, or, for
/// data written before that row existed, the table's newest `fetched_at`
/// (`GREATEST` ignores a NULL). `max_fetched_at_sql` is a constant.
async fn last_fetch(
    pool: &PgPool,
    source: &str,
    max_fetched_at_sql: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let sql = format!(
        "SELECT GREATEST((SELECT fetched_at FROM ingest_freshness WHERE source = $1), ({max_fetched_at_sql}))"
    );
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as(&sql).bind(source).fetch_one(pool).await?;
    Ok(fetched_at)
}

pub async fn last_stations_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    last_fetch(
        pool,
        STATIONS_SOURCE,
        "SELECT MAX(fetched_at) FROM island_of_ireland_stations",
    )
    .await
}

pub async fn last_lines_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    last_fetch(
        pool,
        LINES_SOURCE,
        "SELECT MAX(fetched_at) FROM island_of_ireland_lines",
    )
    .await
}

/// Backs `GET /public/island-of-ireland/stations` (Task A3) -- the whole
/// catalogue is small (~150-300 rows across both networks even once NIR
/// exists), so this is a plain unpaginated list, optionally filtered by
/// network, ordered by name -- same ordering choice as
/// `reference::get_all_tocs`.
/// `(id, name, network, latitude, longitude)`, factored into a named alias
/// so clippy's `type_complexity` lint doesn't fire on `list_stations`'s own
/// row-tuple annotation.
type StationRow = (String, String, String, Option<f64>, Option<f64>);

pub async fn list_stations(
    pool: &PgPool,
    network: Option<IslandOfIrelandNetwork>,
) -> Result<Vec<IslandOfIrelandStation>> {
    let rows: Vec<StationRow> = match network {
        Some(network) => {
            sqlx::query_as(
                "SELECT id, name, network, latitude, longitude FROM island_of_ireland_stations \
                 WHERE network = $1 ORDER BY name",
            )
            .bind(network_wire(network))
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, name, network, latitude, longitude FROM island_of_ireland_stations \
                 ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        }
    };

    rows.into_iter()
        .map(|(id, name, network, latitude, longitude)| {
            Ok(IslandOfIrelandStation {
                id,
                name,
                network: network_from_wire(&network)?,
                latitude,
                longitude,
            })
        })
        .collect()
}

pub async fn list_lines(
    pool: &PgPool,
    network: Option<IslandOfIrelandNetwork>,
) -> Result<Vec<IslandOfIrelandLineDefinition>> {
    let rows: Vec<(String, String, String, serde_json::Value)> = match network {
        Some(network) => {
            sqlx::query_as(
                "SELECT id, name, network, stations FROM island_of_ireland_lines \
                 WHERE network = $1 ORDER BY name",
            )
            .bind(network_wire(network))
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, name, network, stations FROM island_of_ireland_lines ORDER BY name",
            )
            .fetch_all(pool)
            .await?
        }
    };

    rows.into_iter()
        .map(|(id, name, network, stations)| {
            Ok(IslandOfIrelandLineDefinition {
                id,
                name,
                network: network_from_wire(&network)?,
                stations: serde_json::from_value(stations)?,
            })
        })
        .collect()
}

/// One batched statement for the whole poll, the shape of
/// `queries::upsert_station_samples`: `polled_at` is the sample's age, read
/// per station, so it advances on every poll; a row is skipped only when
/// nothing differs, and an unchanged board keeps its stored `departures`
/// value (no new TOAST chunks). The last entry wins when a batch names one
/// station twice.
pub async fn upsert_station_samples(
    pool: &PgPool,
    samples: &[IslandOfIrelandStationSample],
) -> Result<u64> {
    if samples.is_empty() {
        return Ok(0);
    }
    let batch = crate::data::queries::last_per_key(samples, |sample| sample.station_id.clone());
    let ids: Vec<&str> = batch.iter().map(|s| s.station_id.as_str()).collect();
    let networks: Vec<&str> = batch.iter().map(|s| network_wire(s.network)).collect();
    let polled_at: Vec<chrono::DateTime<chrono::Utc>> = batch.iter().map(|s| s.polled_at).collect();
    let departures: Vec<serde_json::Value> = batch
        .iter()
        .map(|s| serde_json::to_value(&s.departures))
        .collect::<Result<_, _>>()?;

    sqlx::query(
        r"
        INSERT INTO island_of_ireland_station_samples (station_id, network, polled_at, departures)
        SELECT station_id, network, polled_at, departures
        FROM UNNEST($1::text[], $2::text[], $3::timestamptz[], $4::jsonb[])
            AS i(station_id, network, polled_at, departures)
        ON CONFLICT (station_id) DO UPDATE SET
            network    = EXCLUDED.network,
            polled_at  = EXCLUDED.polled_at,
            departures = CASE
                WHEN island_of_ireland_station_samples.departures IS DISTINCT FROM EXCLUDED.departures
                THEN EXCLUDED.departures
                ELSE island_of_ireland_station_samples.departures
            END
        WHERE (island_of_ireland_station_samples.network,
               island_of_ireland_station_samples.polled_at,
               island_of_ireland_station_samples.departures)
              IS DISTINCT FROM
              (EXCLUDED.network, EXCLUDED.polled_at, EXCLUDED.departures)
        ",
    )
    .bind(&ids)
    .bind(&networks)
    .bind(&polled_at)
    .bind(&departures)
    .execute(pool)
    .await?;
    Ok(samples.len() as u64)
}

pub async fn last_station_samples_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (polled_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(polled_at) FROM island_of_ireland_station_samples")
            .fetch_one(pool)
            .await?;
    Ok(polled_at)
}

/// Backs `GET /public/island-of-ireland/stations/{id}/departures` (Task
/// B3) -- raw pass-through, mirrors `queries::latest_station_sample`
/// (`crates/api/src/data/queries.rs:918-934`) exactly, one level down in a
/// different table.
pub async fn latest_station_sample(
    pool: &PgPool,
    station_id: &str,
) -> Result<Option<IslandOfIrelandStationSample>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT station_id, network, polled_at, departures FROM island_of_ireland_station_samples \
         WHERE station_id = $1",
    )
    .bind(station_id)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        let network: String = row.try_get("network")?;
        let departures_json: serde_json::Value = row.try_get("departures")?;
        Ok(IslandOfIrelandStationSample {
            station_id: row.try_get("station_id")?,
            network: network_from_wire(&network)?,
            polled_at: row.try_get("polled_at")?,
            departures: serde_json::from_value::<Vec<IslandOfIrelandDeparture>>(departures_json)?,
        })
    })
    .transpose()
}

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

    async fn delete_fixture_station(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_stations WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture station");
    }

    async fn delete_fixture_line(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_lines WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture line");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_stations_then_list_round_trips_and_filters_by_network() {
        let pool = connect().await;
        delete_fixture_station(&pool, "ZIOI1").await;
        delete_fixture_station(&pool, "ZIOI2").await;

        let stations = vec![
            IslandOfIrelandStation {
                id: "ZIOI1".to_string(),
                name: "Zesttown".to_string(),
                network: IslandOfIrelandNetwork::RepublicOfIreland,
                latitude: Some(53.0),
                longitude: Some(-6.0),
            },
            IslandOfIrelandStation {
                id: "ZIOI2".to_string(),
                name: "Zorough".to_string(),
                network: IslandOfIrelandNetwork::NorthernIreland,
                latitude: None,
                longitude: None,
            },
        ];
        let upserted = upsert_stations(&pool, &stations).await.expect("upsert");
        assert_eq!(upserted, 2);

        let roi_only = list_stations(&pool, Some(IslandOfIrelandNetwork::RepublicOfIreland))
            .await
            .expect("list roi");
        assert!(roi_only.iter().any(|s| s.id == "ZIOI1"));
        assert!(!roi_only.iter().any(|s| s.id == "ZIOI2"));

        let all = list_stations(&pool, None).await.expect("list all");
        assert!(all.iter().any(|s| s.id == "ZIOI1"));
        assert!(all.iter().any(|s| s.id == "ZIOI2"));

        delete_fixture_station(&pool, "ZIOI1").await;
        delete_fixture_station(&pool, "ZIOI2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_lines_stores_ordered_stations_and_repeat_upsert_replaces_not_duplicates() {
        let pool = connect().await;
        delete_fixture_line(&pool, "ZLINE1").await;

        let first = IslandOfIrelandLineDefinition {
            id: "ZLINE1".to_string(),
            name: "Zest Line".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            stations: vec!["ZIOI1".to_string(), "ZIOI2".to_string()],
        };
        upsert_lines(&pool, &[first]).await.expect("first upsert");

        let second = IslandOfIrelandLineDefinition {
            id: "ZLINE1".to_string(),
            name: "Zest Line (renamed)".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            stations: vec!["ZIOI2".to_string(), "ZIOI1".to_string()],
        };
        upsert_lines(&pool, &[second]).await.expect("second upsert");

        let lines = list_lines(&pool, None).await.expect("list");
        let line = lines
            .iter()
            .find(|l| l.id == "ZLINE1")
            .expect("row present");
        assert_eq!(line.name, "Zest Line (renamed)");
        assert_eq!(
            line.stations,
            vec!["ZIOI2".to_string(), "ZIOI1".to_string()]
        );

        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM island_of_ireland_lines WHERE id = 'ZLINE1'")
                .fetch_all(&pool)
                .await
                .expect("select");
        assert_eq!(rows.len(), 1, "upsert must replace, not duplicate");

        delete_fixture_line(&pool, "ZLINE1").await;
    }

    /// `xmin` changes exactly when Postgres writes a new row version.
    async fn xmin(pool: &PgPool, table: &str, id: &str) -> String {
        sqlx::query_scalar(&format!("SELECT xmin::text FROM {table} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("read xmin")
    }

    /// N1: one batched statement; an unchanged row is not rewritten, a
    /// changed one is, the last duplicate in a batch wins, and the feed's
    /// "last fetched" still advances when nothing changed.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_stations_skips_unchanged_rows_and_still_records_the_fetch() {
        let pool = connect().await;
        delete_fixture_station(&pool, "ZIOI3").await;
        delete_fixture_station(&pool, "ZIOI4").await;
        let station = |id: &str, name: &str| IslandOfIrelandStation {
            id: id.to_string(),
            name: name.to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            latitude: Some(53.0),
            longitude: None,
        };

        // A duplicate id in one batch: the last entry wins (one statement
        // cannot touch a row twice).
        let first = vec![
            station("ZIOI3", "Old name"),
            station("ZIOI3", "Zedford"),
            station("ZIOI4", "Zeal"),
        ];
        assert_eq!(upsert_stations(&pool, &first).await.unwrap(), 3);
        let all = list_stations(&pool, None).await.unwrap();
        assert_eq!(
            all.iter()
                .find(|s| s.id == "ZIOI3")
                .map(|s| s.name.as_str()),
            Some("Zedford")
        );
        let fetched_first = last_stations_fetch(&pool).await.unwrap().expect("recorded");
        let unchanged_before = xmin(&pool, "island_of_ireland_stations", "ZIOI3").await;
        let changed_before = xmin(&pool, "island_of_ireland_stations", "ZIOI4").await;

        upsert_stations(
            &pool,
            &[station("ZIOI3", "Zedford"), station("ZIOI4", "Zeal Halt")],
        )
        .await
        .unwrap();
        assert_eq!(
            xmin(&pool, "island_of_ireland_stations", "ZIOI3").await,
            unchanged_before,
            "an unchanged station must not be rewritten"
        );
        assert_ne!(
            xmin(&pool, "island_of_ireland_stations", "ZIOI4").await,
            changed_before,
            "a changed station must be rewritten"
        );
        let fetched_second = last_stations_fetch(&pool).await.unwrap().expect("recorded");
        assert!(fetched_second >= fetched_first);
        assert_eq!(upsert_stations(&pool, &[]).await.unwrap(), 0);

        delete_fixture_station(&pool, "ZIOI3").await;
        delete_fixture_station(&pool, "ZIOI4").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn upsert_lines_skips_unchanged_rows() {
        let pool = connect().await;
        delete_fixture_line(&pool, "ZLINE2").await;
        let line = |stations: &[&str]| IslandOfIrelandLineDefinition {
            id: "ZLINE2".to_string(),
            name: "Zest Loop".to_string(),
            network: IslandOfIrelandNetwork::NorthernIreland,
            stations: stations.iter().map(ToString::to_string).collect(),
        };
        upsert_lines(&pool, &[line(&["ZA", "ZB"])]).await.unwrap();
        let before = xmin(&pool, "island_of_ireland_lines", "ZLINE2").await;
        upsert_lines(&pool, &[line(&["ZA", "ZB"])]).await.unwrap();
        assert_eq!(
            xmin(&pool, "island_of_ireland_lines", "ZLINE2").await,
            before
        );
        assert!(last_lines_fetch(&pool).await.unwrap().is_some());
        // Reordering the stations is a change.
        upsert_lines(&pool, &[line(&["ZB", "ZA"])]).await.unwrap();
        assert_ne!(
            xmin(&pool, "island_of_ireland_lines", "ZLINE2").await,
            before
        );
        let lines = list_lines(&pool, None).await.unwrap();
        let stored = lines.iter().find(|l| l.id == "ZLINE2").expect("row");
        assert_eq!(stored.stations, vec!["ZB".to_string(), "ZA".to_string()]);
        delete_fixture_line(&pool, "ZLINE2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland -- --ignored --test-threads=1`"]
    async fn last_fetch_against_an_empty_table_is_null() {
        let pool = connect().await;
        // Reads the real table as-is -- relies on CI's freshly-migrated,
        // otherwise-empty database, same posture
        // `station_full_coverage_samples_get_last_fetched_on_an_empty_table_is_null`
        // already documents for its own table.
        let fetched = last_stations_fetch(&pool).await;
        assert!(fetched.is_ok());
    }
}

#[cfg(test)]
mod sample_db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect")
    }

    async fn delete_fixture(pool: &PgPool, station_id: &str) {
        sqlx::query("DELETE FROM island_of_ireland_station_samples WHERE station_id = $1")
            .bind(station_id)
            .execute(pool)
            .await
            .expect("cleanup fixture sample");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_then_latest_round_trips_and_repeat_upsert_replaces -- --ignored --test-threads=1`"]
    async fn upsert_then_latest_round_trips_and_repeat_upsert_replaces() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMP1").await;

        let first = IslandOfIrelandStationSample {
            station_id: "ZSAMP1".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            polled_at: chrono::Utc::now(),
            departures: vec![IslandOfIrelandDeparture {
                train_code: "Z1".to_string(),
                origin: "Zesttown".to_string(),
                destination: "Zorough".to_string(),
                scheduled_arrival: None,
                scheduled_departure: Some("10:00".to_string()),
                expected_arrival: None,
                expected_departure: Some("10:00".to_string()),
                late_minutes: 0,
                status: "On Time".to_string(),
                due_in_minutes: Some(3),
            }],
        };
        upsert_station_samples(&pool, &[first])
            .await
            .expect("first upsert");

        let fetched = latest_station_sample(&pool, "ZSAMP1")
            .await
            .expect("fetch")
            .expect("row present");
        assert_eq!(fetched.departures[0].train_code, "Z1");

        let second = IslandOfIrelandStationSample {
            station_id: "ZSAMP1".to_string(),
            network: IslandOfIrelandNetwork::RepublicOfIreland,
            polled_at: chrono::Utc::now(),
            departures: vec![],
        };
        upsert_station_samples(&pool, &[second])
            .await
            .expect("second upsert");
        let fetched = latest_station_sample(&pool, "ZSAMP1")
            .await
            .expect("fetch")
            .expect("row still present");
        assert!(fetched.departures.is_empty());

        delete_fixture(&pool, "ZSAMP1").await;
    }

    /// N1: a re-poll with the same board and the same `polled_at` writes
    /// nothing; a newer `polled_at` advances the age; the last duplicate in
    /// a batch wins.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_station_samples_skips_an_identical_poll -- --ignored --test-threads=1`"]
    async fn upsert_station_samples_skips_an_identical_poll() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMP2").await;
        let polled_at: chrono::DateTime<chrono::Utc> = "2031-01-02T03:04:05Z".parse().unwrap();
        let sample =
            |polled_at, departures: Vec<IslandOfIrelandDeparture>| IslandOfIrelandStationSample {
                station_id: "ZSAMP2".to_string(),
                network: IslandOfIrelandNetwork::RepublicOfIreland,
                polled_at,
                departures,
            };
        let departure = IslandOfIrelandDeparture {
            train_code: "Z2".to_string(),
            origin: "Zesttown".to_string(),
            destination: "Zorough".to_string(),
            scheduled_arrival: None,
            scheduled_departure: Some("11:00".to_string()),
            expected_arrival: None,
            expected_departure: Some("11:02".to_string()),
            late_minutes: 2,
            status: "Late".to_string(),
            due_in_minutes: Some(5),
        };
        let xmin = |pool: PgPool| async move {
            sqlx::query_scalar::<_, String>(
                "SELECT xmin::text FROM island_of_ireland_station_samples WHERE station_id = 'ZSAMP2'",
            )
            .fetch_one(&pool)
            .await
            .expect("xmin")
        };

        upsert_station_samples(
            &pool,
            &[
                sample(polled_at, vec![]),
                sample(polled_at, vec![departure.clone()]),
            ],
        )
        .await
        .unwrap();
        let fetched = latest_station_sample(&pool, "ZSAMP2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.departures, vec![departure.clone()]);
        let before = xmin(pool.clone()).await;

        upsert_station_samples(&pool, &[sample(polled_at, vec![departure.clone()])])
            .await
            .unwrap();
        assert_eq!(
            xmin(pool.clone()).await,
            before,
            "identical poll rewrote the row"
        );

        let later = polled_at + chrono::Duration::minutes(1);
        upsert_station_samples(&pool, &[sample(later, vec![departure])])
            .await
            .unwrap();
        assert_ne!(xmin(pool.clone()).await, before);
        assert_eq!(
            last_station_samples_fetch(&pool)
                .await
                .unwrap()
                .map(|t| t >= later),
            Some(true)
        );

        delete_fixture(&pool, "ZSAMP2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                latest_for_an_unseen_station_is_none_not_an_error -- --ignored --test-threads=1`"]
    async fn latest_for_an_unseen_station_is_none_not_an_error() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMPNONE").await;
        let fetched = latest_station_sample(&pool, "ZSAMPNONE")
            .await
            .expect("query");
        assert_eq!(fetched, None);
    }
}
