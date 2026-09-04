//! `schedule-reference`: a sibling container in the `schedulefeed` Pod.
//! Once `schedule-ingest` has extracted a verified-stable delivery into
//! `storage_dir/<timestamp>/` (see
//! `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`),
//! reads that delivery's `RJTTF*MCA.txt` (`TI` records) and `RJTTF*MSN.txt`
//! (`A` records) directly off the already-local, read-only-mounted PVC,
//! resolves a STANOX->CRS table, and POSTs it to `api`'s
//! `/private/stanox-crs`. See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md.

mod config;
mod discovery;
mod parser;

use std::time::Duration;

use clap::Parser;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut last_processed_delivery: Option<String> = None;

    loop {
        interval.tick().await;
        let cycle_start = std::time::Instant::now();
        let result = poll_once(
            &client,
            &config,
            &mut last_processed_delivery,
            &internal_oauth,
        )
        .await;
        metrics::histogram!(common::metrics::metric_name(
            "schedule_reference_cycle_duration_seconds"
        ))
        .record(cycle_start.elapsed().as_secs_f64());
        if let Err(err) = result {
            tracing::error!(error = ?err, "schedule-reference cycle failed; will retry next interval");
        }
    }
}

/// Streams `path` line-by-line, keeping only lines starting with `prefix`
/// -- so the real 707MB `RJTTF<n>MCA.txt` is never held in memory whole,
/// only its ~12,085 `TI` lines (the `RJTTF<n>MSN.txt` file, at ~340KB
/// total, is small enough that this matters far less for it, but the same
/// function is reused for both for one consistent code path).
fn read_prefixed_lines(path: &std::path::Path, prefix: &str) -> anyhow::Result<String> {
    read_prefixed_lines_multi(path, &[prefix])
}

/// As `read_prefixed_lines`, but matching any of `prefixes` -- added for
/// Task 7's CIF `SCHEDULE` read (`BS`/`BX`/`LO`/`LI`/`CR`/`LT`), which
/// needs several record types kept, not just one. `read_prefixed_lines`
/// itself (the `TI`/`A` single-prefix reads) stays untouched as a thin
/// wrapper over this, so neither existing call site changes shape.
fn read_prefixed_lines_multi(path: &std::path::Path, prefixes: &[&str]) -> anyhow::Result<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = String::new();
    for line in reader.lines() {
        let line = line?;
        if prefixes.iter().any(|prefix| line.starts_with(prefix)) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Scans for the most recent complete delivery, skips if unchanged since
/// `last_processed_delivery`, else reads+parses+POSTs it and only advances
/// `last_processed_delivery` on a successful POST.
async fn poll_once(
    client: &Client,
    config: &Config,
    last_processed_delivery: &mut Option<String>,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<()> {
    let Some(delivery) = discovery::latest_complete_delivery(&config.storage_dir)? else {
        tracing::debug!("no complete MCA+MSN delivery directory found yet");
        return Ok(());
    };
    if Some(&delivery.dir_name) == last_processed_delivery.as_ref() {
        tracing::debug!(
            delivery = %delivery.dir_name,
            "no new delivery since last successful parse; nothing to do"
        );
        return Ok(());
    }

    let ti_text = read_prefixed_lines(&delivery.mca_path, "TI")?;
    let a_text = read_prefixed_lines(&delivery.msn_path, "A")?;

    let ti_records = parser::parse_ti_lines(&ti_text);
    let msn_crs = parser::parse_msn_a_lines(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs);

    tracing::info!(
        delivery = %delivery.dir_name,
        ti_records = ti_records.len(),
        resolved = rows.len(),
        "parsed stanox/crs table from delivery"
    );

    // `common::StanoxCrsRecord::source_sequence` predates this crate's own
    // zip/mtime-delivery rework and is shared with `crates/trust-consumer`
    // -- out of this fix's scope to retype. Best-effort only: the embedded
    // number in the MCA filename (e.g. the `942` in `RJTTF942MCA.txt`) is
    // NOT relied on to decide which delivery is newest (see `discovery.rs`
    // and this repo's 2026-09-03 correction note) -- it's used here purely
    // as informational provenance for this one downstream table, falling
    // back to `0` if the filename doesn't carry a parseable number.
    let source_sequence = embedded_sequence_number(&delivery.mca_path).unwrap_or(0);

    let records: Vec<common::StanoxCrsRecord> = rows
        .into_iter()
        .map(|row| common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence,
        })
        .collect();

    common::ingest::post_batch(
        client,
        &config.api_ingest_url,
        internal_oauth,
        &records,
        "stanox/crs rows",
    )
    .await?;

    // Only advance on a successful POST -- a failed POST just means the
    // already-computed table is discarded and rebuilt from the same
    // still-local, unchanged files next cycle (cheap), matching the
    // spec's Error handling: "a failed POST just means the already-
    // computed in-memory table is discarded and rebuilt... next cycle".
    *last_processed_delivery = Some(delivery.dir_name.clone());

    publish_cif_derived_products(client, config, &delivery.mca_path, internal_oauth, &records)
        .await;

    Ok(())
}

/// Task 3's (whole-network-trip-search plan) shared wrapper: builds the
/// whole-network `ScheduleIndex` ONCE from this delivery's `BS`/`BX`/`LO`/
/// `LI`/`CR`/`LT` records, then runs BOTH CIF-derived publishes off that
/// one index/`today` pair -- the per-line publish this crate already had
/// (Task 7 of the option-b-live-consumer plan, UNCHANGED below beyond its
/// own signature: same per-line loop, same individual-object POST, same
/// line-filtering predicate) and the new per-station whole-network publish
/// this plan adds. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1.
async fn publish_cif_derived_products(
    client: &Client,
    config: &Config,
    mca_path: &std::path::Path,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    stanox_crs_records: &[common::StanoxCrsRecord],
) {
    let mca_schedule_text = match read_prefixed_lines_multi(
        mca_path,
        &["BS", "BX", "LO", "LI", "CR", "LT"],
    ) {
        Ok(text) => text,
        Err(err) => {
            tracing::error!(error = ?err, "failed to read CIF SCHEDULE records from delivery; skipping this cycle's CIF-derived publishes");
            return;
        }
    };

    let index = schedule_query::ScheduleIndex::from_text(&mca_schedule_text);
    // schedule-reference has no rail-day concept of its own yet --
    // publishing against the plain calendar date is deliberate and
    // sufficient here, UNCHANGED from before this restructuring:
    // `schedules_touching`/`departures_by_crs` both resolve STP overlays
    // per calendar date already, and `full-coverage-consumer`'s OWN
    // rail-day gating is what decides Pending/Available for the line
    // population, not this publish step.
    let today = chrono::Utc::now().date_naive();

    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
}

/// UNCHANGED per-line publish logic (per-line loop, per-line individual
/// POST) -- only its own signature changed: `index`/`today` are now
/// shared, caller-supplied inputs (built once by
/// `publish_cif_derived_products`) rather than rebuilt here on every call.
/// Behavior-preserving: the JSON body shape (`line_id`/`service_date`/
/// `population`), the per-line filtering predicate (`lines_to_publish`,
/// untouched), and the individual-object POST (`post_schedule_line_population`,
/// untouched) are all byte-for-byte the same as before this plan's Task 3.
/// The lint suppression below is a direct, unavoidable consequence of that
/// byte-for-byte constraint: `index` is now `&ScheduleIndex` (caller-
/// supplied) rather than an owned `ScheduleIndex` built locally, so the
/// loop body's unchanged `&index` (see Task 3 Step 5's own diff check)
/// trips `clippy::needless_borrow` -- fixing the lint would mean editing
/// the loop body, which the plan's Step 5 forbids.
#[allow(clippy::needless_borrow)]
async fn publish_schedule_line_population(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    for line in lines_to_publish(&config.lines) {
        let tiplocs: Vec<&str> = line
            .stations
            .iter()
            .filter_map(|s| s.tiploc.as_deref())
            .collect();
        let resolved = schedule_query::schedules_touching(&index, &tiplocs, today);
        let population: Vec<schedule_query::LinePopulationEntry> =
            resolved.into_iter().map(Into::into).collect();
        let body = serde_json::json!({
            "line_id": line.id,
            "service_date": today,
            "population": population,
        });
        if let Err(err) = post_schedule_line_population(
            client,
            &config.schedule_line_population_url,
            internal_oauth,
            &body,
        )
        .await
        {
            tracing::error!(error = ?err, line_id = %line.id, "failed to publish schedule line population; will retry next cycle");
        }
    }
}

/// The whole-network trip-search design doc's Decision 1: every
/// non-cancelled schedule's departure-bearing calling points, bucketed by
/// CRS via this cycle's already-resolved `stanox_crs_records`, capped to
/// the earliest `MAX_DEPARTURES_PER_STATION` per station, published as ONE
/// batch-array POST (not one POST per CRS, unlike the per-line publish
/// above -- see the design doc's Decision 1 for why: this route has one
/// reader, `api` itself, storing every row from one cycle in one
/// transaction).
const MAX_DEPARTURES_PER_STATION: usize = 10; // mirrors poller-ldbws's own
// num_rows=10 default,
// crates/poller-ldbws/src/config.rs:45-46

async fn publish_schedule_network_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = stanox_crs_records
        .iter()
        .map(|r| {
            (
                schedule_query::normalize_tiploc(&r.tiploc).to_string(),
                r.crs.clone(),
            )
        })
        .collect();
    let now = london_local_time_now();

    let by_crs = schedule_query::departures_by_crs(index, today, now, &tiploc_to_crs);
    let rows = schedule_network_departures_rows(by_crs, today);

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_network_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived network departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish schedule-derived network departures; will retry next cycle");
    }
}

/// Pure sort/cap/JSON-shaping logic, split out of
/// `publish_schedule_network_departures` purely so it's unit-testable
/// without a mock HTTP server -- same "pure logic separated from I/O"
/// convention `lines_to_publish`/`read_prefixed_lines_multi` already
/// establish in this file.
fn schedule_network_departures_rows(
    mut by_crs: std::collections::HashMap<String, Vec<schedule_query::ScheduleDeparture>>,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_crs
        .drain()
        .map(|(crs, mut departures)| {
            departures.sort_by_key(|d| d.scheduled);
            departures.truncate(MAX_DEPARTURES_PER_STATION);
            serde_json::json!({ "crs": crs, "service_date": today, "departures": departures })
        })
        .collect()
}

/// The CIF `booked_departure`/`booked_arrival` fields
/// (`schedule_query::records::CallingPoint`'s own doc) are Europe/London
/// LOCAL civil time, not UTC -- comparing them against a naive
/// `chrono::Utc::now().time()` would be wrong by an hour for the ~7 months
/// of British Summer Time. Resolves the design doc's Open Question 1:
/// unlike `crates/api/src/data/eta_blend.rs::london_to_utc` (which resolves
/// a NAIVE local datetime to UTC, and therefore has to handle the
/// ambiguous-hour/nonexistent-hour DST edge cases via `LocalResult`), this
/// goes the other way -- FROM a known UTC instant TO its local
/// Europe/London clock time via `DateTime::with_timezone`, which is always
/// exactly one unambiguous answer.
fn london_local_time_at(instant: chrono::DateTime<chrono::Utc>) -> chrono::NaiveTime {
    instant.with_timezone(&chrono_tz::Europe::London).time()
}

fn london_local_time_now() -> chrono::NaiveTime {
    london_local_time_at(chrono::Utc::now())
}

/// Every catalogued line with at least one `tiploc`-bearing station -- a
/// line with zero TIPLOCs trivially produces an empty `schedules_touching`
/// result, harmless (if pointless) to publish, so this doesn't bother
/// filtering it out for correctness, only to avoid a wasted POST.
fn lines_to_publish(
    lines: &[common::LineDefinition],
) -> impl Iterator<Item = &common::LineDefinition> {
    lines
        .iter()
        .filter(|l| l.stations.iter().any(|s| s.tiploc.is_some()))
}

/// A single-object POST (not a batch array) -- `common::ingest::post_batch`
/// serializes a slice as a JSON array, which doesn't fit this route's body
/// shape, so this is a small bespoke sibling rather than a forced reuse.
async fn post_schedule_line_population(
    client: &Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("schedule-line-population POST failed: {status} {text}");
    }
}

/// Best-effort extraction of the digits embedded in a real delivery's own
/// MCA filename (e.g. `942` from `RJTTF942MCA.txt`) -- see this function's
/// one call site for why this is informational only, never used to decide
/// delivery identity/recency.
fn embedded_sequence_number(mca_path: &std::path::Path) -> Option<i32> {
    let name = mca_path.file_name()?.to_str()?;
    let digits = name.strip_prefix("RJTTF")?.strip_suffix("MCA.txt")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod poll_once_tests {
    use super::*;

    #[test]
    fn read_prefixed_lines_extracts_only_matching_lines_from_a_mixed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.txt");
        std::fs::write(
            &path,
            "HDsomething\nTIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           \nBSsomeschedule\n",
        )
        .unwrap();

        let ti_text = read_prefixed_lines(&path, "TI").unwrap();
        assert_eq!(ti_text.lines().count(), 1);
        assert!(ti_text.starts_with("TIEUSTON"));

        let records = parser::parse_ti_lines(&ti_text);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tiploc, "EUSTON");
    }

    #[test]
    fn embedded_sequence_number_parses_the_real_filename_shape() {
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("RJTTF942MCA.txt")),
            Some(942)
        );
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("/some/dir/RJTTF1MCA.txt")),
            Some(1)
        );
    }

    #[test]
    fn embedded_sequence_number_is_none_for_a_non_matching_shape() {
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("not-a-real-name.txt")),
            None
        );
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("RJTTFabcMCA.txt")),
            None
        );
    }

    fn fixture_line(id: &str, stations: Vec<common::Station>) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "rail".to_string(),
            category: "national-rail".to_string(),
            operators: vec![],
            stations,
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    fn fixture_station(crs: &str, tiploc: Option<&str>) -> common::Station {
        common::Station {
            crs: crs.to_string(),
            tiploc: tiploc.map(str::to_string),
            role: "minor".to_string(),
            segment: None,
        }
    }

    #[test]
    fn lines_to_publish_includes_a_line_with_at_least_one_tiploc_bearing_station() {
        let lines = vec![fixture_line(
            "zzz-with-tiploc",
            vec![
                fixture_station("ZZA", None),
                fixture_station("ZZB", Some("ZZBTPL")),
            ],
        )];
        let published: Vec<&str> = lines_to_publish(&lines).map(|l| l.id.as_str()).collect();
        assert_eq!(published, vec!["zzz-with-tiploc"]);
    }

    #[test]
    fn lines_to_publish_excludes_a_line_with_no_tiploc_bearing_station_at_all() {
        let lines = vec![fixture_line(
            "zzz-no-tiploc",
            vec![fixture_station("ZZA", None), fixture_station("ZZB", None)],
        )];
        let published: Vec<&str> = lines_to_publish(&lines).map(|l| l.id.as_str()).collect();
        assert!(published.is_empty());
    }

    #[test]
    fn london_local_time_at_a_summer_instant_is_one_hour_ahead_of_utc() {
        // 2026-07-15 13:00:00 UTC is 14:00:00 BST (July is daylight saving).
        let instant: chrono::DateTime<chrono::Utc> = "2026-07-15T13:00:00Z".parse().unwrap();
        assert_eq!(
            london_local_time_at(instant),
            chrono::NaiveTime::from_hms_opt(14, 0, 0).unwrap()
        );
    }

    #[test]
    fn london_local_time_at_a_winter_instant_matches_utc() {
        // 2026-01-15 13:00:00 UTC is 13:00:00 GMT (January is not daylight saving).
        let instant: chrono::DateTime<chrono::Utc> = "2026-01-15T13:00:00Z".parse().unwrap();
        assert_eq!(
            london_local_time_at(instant),
            chrono::NaiveTime::from_hms_opt(13, 0, 0).unwrap()
        );
    }

    #[test]
    fn schedule_network_departures_rows_sorts_earliest_first_and_caps_at_ten() {
        let mut by_crs = std::collections::HashMap::new();
        let departures: Vec<schedule_query::ScheduleDeparture> = (0..12)
            .rev() // deliberately out of order
            .map(|hour| schedule_query::ScheduleDeparture {
                uid: format!("U{hour:05}"),
                scheduled: chrono::NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
                destination_crs: None,
            })
            .collect();
        by_crs.insert("EUS".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let rows = schedule_network_departures_rows(by_crs, today);

        assert_eq!(rows.len(), 1);
        let row_departures = rows[0]["departures"].as_array().unwrap();
        assert_eq!(row_departures.len(), MAX_DEPARTURES_PER_STATION);
        assert_eq!(
            row_departures[0]["uid"], "U00000",
            "earliest-first after sort"
        );
        assert_eq!(
            row_departures[9]["uid"], "U00009",
            "capped at 10, entries 10 and 11 dropped"
        );
    }

    #[test]
    fn schedule_network_departures_rows_produces_one_row_per_crs_key() {
        let mut by_crs = std::collections::HashMap::new();
        by_crs.insert(
            "EUS".to_string(),
            vec![schedule_query::ScheduleDeparture {
                uid: "U1".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                destination_crs: Some("CRE".to_string()),
            }],
        );
        by_crs.insert(
            "WAT".to_string(),
            vec![schedule_query::ScheduleDeparture {
                uid: "U2".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                destination_crs: None,
            }],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let rows = schedule_network_departures_rows(by_crs, today);

        assert_eq!(rows.len(), 2);
        let crs_values: Vec<&str> = rows.iter().map(|r| r["crs"].as_str().unwrap()).collect();
        assert!(crs_values.contains(&"EUS"));
        assert!(crs_values.contains(&"WAT"));
        for row in &rows {
            assert_eq!(row["service_date"], "2026-09-04");
        }
    }
}
