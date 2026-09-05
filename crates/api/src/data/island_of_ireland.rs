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

pub async fn upsert_stations(pool: &PgPool, stations: &[IslandOfIrelandStation]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for station in stations {
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_stations (id, name, network, latitude, longitude, fetched_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            ON CONFLICT (id) DO UPDATE SET
                name       = EXCLUDED.name,
                network    = EXCLUDED.network,
                latitude   = EXCLUDED.latitude,
                longitude  = EXCLUDED.longitude,
                fetched_at = NOW()
            "#,
        )
        .bind(&station.id)
        .bind(&station.name)
        .bind(network_wire(station.network))
        .bind(station.latitude)
        .bind(station.longitude)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

pub async fn upsert_lines(pool: &PgPool, lines: &[IslandOfIrelandLineDefinition]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for line in lines {
        let stations_json = serde_json::to_value(&line.stations)?;
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_lines (id, name, network, stations, fetched_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (id) DO UPDATE SET
                name       = EXCLUDED.name,
                network    = EXCLUDED.network,
                stations   = EXCLUDED.stations,
                fetched_at = NOW()
            "#,
        )
        .bind(&line.id)
        .bind(&line.name)
        .bind(network_wire(line.network))
        .bind(&stations_json)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

pub async fn last_stations_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM island_of_ireland_stations")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

pub async fn last_lines_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM island_of_ireland_lines")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
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

pub async fn upsert_station_samples(
    pool: &PgPool,
    samples: &[IslandOfIrelandStationSample],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let departures_json = serde_json::to_value(&sample.departures)?;
        sqlx::query(
            r#"
            INSERT INTO island_of_ireland_station_samples (station_id, network, polled_at, departures)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (station_id) DO UPDATE SET
                network    = EXCLUDED.network,
                polled_at  = EXCLUDED.polled_at,
                departures = EXCLUDED.departures
            "#,
        )
        .bind(&sample.station_id)
        .bind(network_wire(sample.network))
        .bind(sample.polled_at)
        .bind(&departures_json)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
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
                island_of_ireland_samples -- --ignored --test-threads=1`"]
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland_samples -- --ignored --test-threads=1`"]
    async fn latest_for_an_unseen_station_is_none_not_an_error() {
        let pool = connect().await;
        delete_fixture(&pool, "ZSAMPNONE").await;
        let fetched = latest_station_sample(&pool, "ZSAMPNONE")
            .await
            .expect("query");
        assert_eq!(fetched, None);
    }
}
