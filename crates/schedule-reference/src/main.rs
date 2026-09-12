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
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut last_processed_delivery: Option<String> =
        seed_last_processed_delivery(&client, &config, &internal_oauth).await;
    match &last_processed_delivery {
        Some(delivery) => tracing::info!(
            delivery = %delivery,
            "seeded last_processed_delivery from api's persisted schedule-feed-ingests record; will not redundantly republish this delivery after a restart"
        ),
        None => tracing::info!(
            "no prior schedule-feed delivery recorded by api yet; will process the next delivery poll_once finds (first-run behavior)"
        ),
    }

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

/// Renders `delivered_at` in the exact directory-name shape
/// `schedule-ingest::delivery::delivery_dir_name` uses for its
/// timestamp-named delivery directories under `storage_dir`:
/// `YYYYMMDDTHHMMSSZ`. Kept as a small local copy rather than a cross-crate
/// import -- that function is private to `schedule-ingest`, and the two
/// crates already communicate a delivery's identity purely by string shape
/// (see `discovery::CompleteDelivery::dir_name`'s own doc comment), not a
/// shared Rust type, the same posture `schedule-ingest::main`'s own
/// `ScheduleFeedIngestRequest` documents for its mirrored struct. Both this
/// function and the real `delivery_dir_name` derive their output from the
/// SAME underlying timestamp (the delivery zip's own mtime, recorded as
/// `schedule_feed_ingests.delivered_at`), via the same `chrono` format
/// string, so the two are guaranteed to agree.
fn dir_name_from_delivered_at(delivered_at: chrono::DateTime<chrono::Utc>) -> String {
    delivered_at.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Seeds `last_processed_delivery` from `api`'s own persisted record of the
/// most recently successfully-ingested CIF delivery (`GET
/// /private/schedule-feed-ingests` -- the same route `schedule-ingest`
/// POSTs to, and the same GET-a-freshness-marker-at-startup pattern
/// `common::ingest::time_until_next_poll` already establishes for every
/// other poller in this workspace), rather than always starting at `None`
/// on a process restart.
///
/// Without this, restarting this container (the `reference` sibling in the
/// `schedulefeed` Pod) always re-triggers a full, redundant republish of
/// `schedule_destination_departures` for the whole 7-day forward window --
/// ~1.7-2 million rows torn down and rebuilt in Postgres -- even when the
/// underlying delivery was already fully processed hours earlier, because
/// `last_processed_delivery` lived only in this process's memory. Confirmed
/// directly against production: a delivery reprocessed at a real restart
/// had already been ingested 8.5 hours earlier per `schedule_feed_ingests`.
///
/// Returns `None` -- this service's pre-existing, still-correct
/// first-run/fallback behavior (`poll_once` processes the next delivery it
/// finds) -- in two distinct cases:
/// * `api` has never recorded a delivery at all (`fetched_at: None`) -- a
///   genuine, valid, once-ever case (a fresh deployment's
///   `schedule_feed_ingests` table starts empty), not an error.
/// * The GET itself fails (network error, `api` not yet reachable, a bad
///   response) -- logged at `warn`, but never propagated as a hard startup
///   failure: this service must still be able to start and make forward
///   progress even if this one optimization can't be applied yet.
async fn seed_last_processed_delivery(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> Option<String> {
    let response: common::ingest::LastFetchedResponse = match common::ingest::get_json(
        client,
        &config.schedule_feed_ingests_url,
        internal_oauth,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(
                error = ?err,
                "could not fetch last schedule-feed delivery from api on startup; falling back to first-run behavior for this process lifetime"
            );
            return None;
        }
    };
    response.fetched_at.map(dir_name_from_delivered_at)
}

/// Forward publish window, in days, for `schedule_destination_departures`:
/// how many days beyond today this service also computes and publishes on
/// every cycle. See
/// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1.2.
/// The route-side search window
/// (`crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`) must be
/// kept in sync with this value by hand -- there is no shared constant
/// across the `api`/`schedule-reference` crate boundary, matching this
/// codebase's existing per-crate-constant convention (e.g.
/// `MAX_DEPARTURES_PER_STATION` here vs. `MAX_SEARCH_LIMIT` in `api`).
const DESTINATION_DEPARTURES_FORWARD_DAYS: i64 = 7;

/// `today..=today+forward_days`, inclusive, today first. Pure and
/// unit-testable without a mock HTTP server or a `ScheduleIndex`, same
/// convention as `lines_to_publish` just below it in this file.
fn forward_publish_dates(today: chrono::NaiveDate, forward_days: i64) -> Vec<chrono::NaiveDate> {
    (0..=forward_days)
        .map(|offset| today + chrono::Duration::days(offset))
        .collect()
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

    publish_schedule_line_population(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    // Third CIF-derived product off the SAME one-per-cycle ScheduleIndex --
    // the design doc's Approach B is explicit that this must not trigger a
    // second parse or a resident index. Unlike the two products above,
    // this one publishes a WINDOW of dates, not just `today`: see
    // docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md
    // §1/§2. `publish_schedule_destination_departures` itself is
    // unmodified -- it already accepts an arbitrary date; only the number
    // of times it's called per cycle changes.
    for date in forward_publish_dates(today, DESTINATION_DEPARTURES_FORWARD_DAYS) {
        publish_schedule_destination_departures(
            client,
            config,
            &index,
            date,
            stanox_crs_records,
            internal_oauth,
        )
        .await;
    }
}

/// Per-line publish logic (per-line loop, per-line individual POST):
/// `index`/`today` are shared, caller-supplied inputs (built once by
/// `publish_cif_derived_products`) rather than rebuilt here on every call.
/// The JSON body shape (`line_id`/`service_date`/`population`) and the
/// individual-object POST (`post_schedule_line_population`) are unchanged
/// from before this plan's Task 3.
///
/// As of the 2026-09-09 tiploc-schedule-matching-gap fix, this now also
/// takes `stanox_crs_records` (the same real, CIF-derived data
/// `publish_schedule_network_departures`/`publish_schedule_destination_departures`
/// already invert into a `tiploc_to_crs` map, just below) and resolves each
/// line's TIPLOC filter list -- and `lines_to_publish`'s own inclusion
/// predicate -- from it, via `crs_to_tiploc_map`/`line_tiplocs`, rather
/// than from the TOML `tiploc` field. See `lines_to_publish`'s doc comment
/// for why: the TOML field is documentation/display metadata only and was
/// never a reliable proxy for "does this station appear in real CIF data."
///
/// The lint suppression below predates this fix (Task 3 Step 5's own
/// byte-for-byte constraint on this loop, since relaxed by this change):
/// `index` is `&ScheduleIndex` (caller-supplied) rather than an owned
/// `ScheduleIndex` built locally, so the loop body's `&index` trips
/// `clippy::needless_borrow`.
#[allow(clippy::needless_borrow)]
async fn publish_schedule_line_population(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let crs_to_tiploc = crs_to_tiploc_map(stanox_crs_records);
    for line in lines_to_publish(&config.lines, &crs_to_tiploc) {
        let tiplocs = line_tiplocs(line, &crs_to_tiploc);
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

/// Real, CIF-derived CRS -> TIPLOC(s) map, inverted from
/// `stanox_crs_records` -- the mirror image of the `tiploc_to_crs` map
/// `publish_schedule_network_departures`/`publish_schedule_destination_departures`
/// already build from the same data, just keyed the other way round. A CRS
/// can resolve to more than one TIPLOC in practice (multiple STANOX rows
/// can share a CRS, e.g. different platforms/areas of one physical
/// location -- see `queries::list_stanox_crs_for_crs`'s own doc in
/// `crates/api`), so this is `Vec<String>`-valued, not a single TIPLOC.
fn crs_to_tiploc_map(
    stanox_crs_records: &[common::StanoxCrsRecord],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for record in stanox_crs_records {
        map.entry(record.crs.to_uppercase())
            .or_default()
            .push(record.tiploc.clone());
    }
    map
}

/// One line's real TIPLOC filter list for `schedule_query::schedules_touching`,
/// resolved per-station from the real, CIF-derived `crs_to_tiploc` map --
/// NOT from the TOML `tiploc` field (see `lines_to_publish`'s doc comment
/// for why that field is no longer used for this).
fn line_tiplocs<'a>(
    line: &common::LineDefinition,
    crs_to_tiploc: &'a std::collections::HashMap<String, Vec<String>>,
) -> Vec<&'a str> {
    line.stations
        .iter()
        .filter_map(|s| crs_to_tiploc.get(&s.crs.to_uppercase()))
        .flatten()
        .map(String::as_str)
        .collect()
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

/// Pure JSON-shaping logic, split out of
/// `publish_schedule_destination_departures` purely so it is unit-testable
/// without a mock HTTP server -- same convention as
/// `schedule_network_departures_rows` directly above.
///
/// **A flatten, not a grouping.** Its sibling above emits one row per CRS
/// key with a capped, sorted `departures` array inside it; this one emits
/// one row per DEPARTURE, each carrying its own `destination_crs`, and
/// there is no array, no sort and no cap anywhere in it. The three
/// differences all have the same cause:
///
/// * **No cap**, because no cap value is defensible. London Waterloo
///   buckets ~9,634 departure-bearing calling points for a single day and
///   the next several busiest destinations are within the same order of
///   magnitude, so any cap truncates precisely the destinations a
///   whole-network destination search exists to serve. Worse, this publish
///   fires once per CIF DELIVERY (roughly daily), not once per 30-minute
///   cycle, so an earliest-first cap freezes at delivery time and is
///   entirely in the past by the evening. See
///   docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
///   §1 and §3.
/// * **No sort**, because ordering is the read side's job now:
///   `queries::search_schedule_calling_point_departures`'s `ORDER BY
///   scheduled, train_uid` rides
///   `schedule_destination_departures_calling_point_idx`. Sorting ~377,000
///   rows here would be wasted work.
/// * **One row per departure**, because the destination is no longer a
///   bucket key -- it is a column, and a filter predicate, on a flat table.
///
/// `service_date` is emitted on every row, unlike the four-key sketch in
/// the addendum's §3, because the ingest handler's first statement is a
/// `DELETE ... WHERE service_date = $1` and `common::ingest::post_batch`
/// posts a bare array with nowhere else to carry the day. Budget ~100 bytes
/// per entry when sizing the POST (including the ~10-byte `true_origin_crs`
/// field and the ~10-byte `destination_arrival` field added alongside it,
/// see
/// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md),
/// not ~55 or ~80.
fn schedule_destination_departures_rows(
    mut by_destination: std::collections::HashMap<
        String,
        Vec<schedule_query::DestinationDeparture>,
    >,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_destination
        .drain()
        .flat_map(|(destination_crs, departures)| {
            departures.into_iter().map(move |d| {
                serde_json::json!({
                    "service_date": today,
                    "destination_crs": destination_crs,
                    "scheduled": d.scheduled,
                    "day_offset": d.day_offset,
                    "train_uid": d.uid,
                    "origin_crs": d.origin_crs,
                    "true_origin_crs": d.true_origin_crs,
                    "calling_point_arrival": d.calling_point_arrival,
                    "destination_arrival": d.destination_arrival,
                    "destination_arrival_day_offset": d.destination_arrival_day_offset,
                })
            })
        })
        .collect()
}

/// The destination-keyed sibling of `publish_schedule_network_departures`
/// directly above: same one-batch-array POST shape, same `tiploc_to_crs`
/// map built from this cycle's already-resolved `stanox_crs_records`, same
/// log-and-continue error posture (a failed POST just means this delivery's
/// grouping is discarded and rebuilt when the next one lands). See
/// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B, as revised by
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md,
/// Approach C.
///
/// **Two deliberate differences from the sibling, both easy to "fix" back
/// by mistake:**
///
/// 1. `now` is `chrono::NaiveTime::MIN`, NOT `london_local_time_now()`.
///    That is not an oversight and it is not a placeholder -- it publishes
///    the WHOLE rail day, on purpose. The sibling's publish-time
///    `now`-forward filter is evaluated exactly once per CIF delivery
///    (roughly daily -- `poll_once` returns early unless the delivery
///    directory changed, `main.rs:101-107`), so whatever the clock happened
///    to read when the delivery landed becomes the boundary for the rest of
///    the day. For a next-10-per-station board that is a tolerable
///    staleness; for a destination search it silently empties the busiest
///    destinations by evening. So this product publishes everything and
///    `GET /public/trains/search` applies `scheduled >= now` at REQUEST
///    time, where the clock is actually correct. If you change this back to
///    `london_local_time_now()`, you reintroduce that bug. See the
///    addendum's §1.3.
/// 2. The rows are flat and uncapped (see
///    `schedule_destination_departures_rows`), so this is a much larger
///    body than the sibling's: ~377,000 objects, ~30MB. That is inside
///    `DefaultBodyLimit::max(100 * 1024 * 1024)`
///    (`crates/api/src/routes/mod.rs:86`) with ~3.3x headroom. If a future
///    measurement pushes it past ~60MB, chunk it with
///    `for chunk in rows.chunks(50_000)` and teach the ingest handler
///    "the first chunk clears the day" -- addendum §3's documented
///    fallback, and Task 1 Step 3 of this plan.
async fn publish_schedule_destination_departures(
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
    // Midnight, i.e. no publish-time `now`-forward filter at all -- see this
    // function's own doc comment, point 1. Deliberate; do not "fix".
    let now = chrono::NaiveTime::MIN;

    let by_destination =
        schedule_query::departures_by_destination_crs(index, today, now, &tiploc_to_crs);
    let rows = schedule_destination_departures_rows(by_destination, today);

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_destination_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived destination departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish schedule-derived destination departures; will retry next cycle");
    }
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

/// Every catalogued line with at least one station resolvable to a real,
/// CIF-derived TIPLOC via `crs_to_tiploc` (built from this cycle's own
/// `stanox_crs_records` by `crs_to_tiploc_map`) -- a line with zero
/// resolvable TIPLOCs trivially produces an empty `schedules_touching`
/// result, harmless (if pointless) to publish, so this doesn't bother
/// filtering it out for correctness, only to avoid a wasted POST.
///
/// As of the 2026-09-09 tiploc-schedule-matching-gap fix, this predicate no
/// longer looks at the TOML `tiploc` field at all (previously: "a line with
/// at least one `tiploc`-bearing station"). That field is hand-curated,
/// optional, and largely absent -- 39 of 109 `lines/*.toml` files have it
/// set on precisely zero stations (all of ScotRail, Southeastern,
/// Merseyrail, London Overground, Heathrow Express, and others) -- so
/// gating a whole line's publish on it silently dropped
/// `schedule_line_population` for those lines entirely, even though the
/// real `stanox_crs` table (this function's new `crs_to_tiploc` input) had
/// everything needed to resolve them. The TOML `tiploc` field is now purely
/// documentation/display metadata; see `lines/SCHEMA.md`.
fn lines_to_publish<'a>(
    lines: &'a [common::LineDefinition],
    crs_to_tiploc: &std::collections::HashMap<String, Vec<String>>,
) -> impl Iterator<Item = &'a common::LineDefinition> {
    lines.iter().filter(move |l| {
        l.stations
            .iter()
            .any(|s| crs_to_tiploc.contains_key(&s.crs.to_uppercase()))
    })
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
    common::ingest::post_json(client, url, tokens, body)
        .await
        .map_err(|err| anyhow::anyhow!("schedule-line-population POST failed: {err}"))
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

    /// Builds the real, CIF-derived `crs_to_tiploc` map `lines_to_publish`/
    /// `line_tiplocs` now consult, straight from `(crs, tiploc)` pairs --
    /// deliberately NOT built via `crs_to_tiploc_map` itself in most of
    /// these tests, so the fixture doesn't depend on the function under
    /// test.
    fn fixture_crs_to_tiploc(
        pairs: &[(&str, &str)],
    ) -> std::collections::HashMap<String, Vec<String>> {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (crs, tiploc) in pairs {
            map.entry(crs.to_uppercase())
                .or_default()
                .push(tiploc.to_string());
        }
        map
    }

    #[test]
    fn forward_publish_dates_returns_today_through_today_plus_n_inclusive() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        let dates = forward_publish_dates(today, 3);
        assert_eq!(
            dates,
            vec![
                chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 11).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 12).unwrap(),
            ],
            "today plus 0..=3 days, in order, today first"
        );
    }

    #[test]
    fn forward_publish_dates_with_zero_forward_days_is_just_today() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        assert_eq!(forward_publish_dates(today, 0), vec![today]);
    }

    #[test]
    fn lines_to_publish_includes_a_line_with_at_least_one_real_cif_tiploc_bearing_station() {
        let lines = vec![fixture_line(
            "zzz-with-tiploc",
            vec![
                fixture_station("ZZA", None),
                fixture_station("ZZB", Some("ZZBTPL")),
            ],
        )];
        let crs_to_tiploc = fixture_crs_to_tiploc(&[("ZZB", "ZZBTPL")]);
        let published: Vec<&str> = lines_to_publish(&lines, &crs_to_tiploc)
            .map(|l| l.id.as_str())
            .collect();
        assert_eq!(published, vec!["zzz-with-tiploc"]);
    }

    #[test]
    fn lines_to_publish_excludes_a_line_with_no_real_cif_tiploc_bearing_station_at_all() {
        let lines = vec![fixture_line(
            "zzz-no-tiploc",
            vec![fixture_station("ZZA", None), fixture_station("ZZB", None)],
        )];
        let crs_to_tiploc = fixture_crs_to_tiploc(&[]);
        let published: Vec<&str> = lines_to_publish(&lines, &crs_to_tiploc)
            .map(|l| l.id.as_str())
            .collect();
        assert!(published.is_empty());
    }

    /// The actual regression test for the tiploc-schedule-matching-gap bug
    /// (2026-09-09): a station whose TOML entry carries no `tiploc` at all
    /// -- exactly the 39-of-109 `lines/*.toml` files case the live-
    /// production investigation found -- must still be published, because
    /// its real TIPLOC comes from the CIF-derived `crs_to_tiploc` map, not
    /// from this TOML field. Before this fix, `lines_to_publish` looked
    /// only at `s.tiploc.is_some()`, so this exact line (no station has a
    /// TOML `tiploc`) would have been silently dropped from
    /// `schedule_line_population` entirely, even with a matching real
    /// `stanox_crs` record for ZZA.
    #[test]
    fn lines_to_publish_includes_a_line_whose_toml_has_no_tiploc_but_has_a_real_cif_tiploc_record()
    {
        let lines = vec![fixture_line(
            "zzz-toml-tiploc-less-but-real",
            vec![fixture_station("ZZA", None)],
        )];
        let crs_to_tiploc = fixture_crs_to_tiploc(&[("ZZA", "ZZATPL")]);
        let published: Vec<&str> = lines_to_publish(&lines, &crs_to_tiploc)
            .map(|l| l.id.as_str())
            .collect();
        assert_eq!(published, vec!["zzz-toml-tiploc-less-but-real"]);
    }

    #[test]
    fn line_tiplocs_resolves_from_the_real_crs_to_tiploc_map_not_the_toml_field() {
        let line = fixture_line(
            "zzz-mixed",
            vec![
                fixture_station("ZZA", None), // no TOML tiploc, but a real CIF record
                fixture_station("ZZB", Some("ZZB-TOML-TPL")), // TOML tiploc is ignored now
            ],
        );
        let crs_to_tiploc =
            fixture_crs_to_tiploc(&[("ZZA", "ZZA-REAL-TPL"), ("ZZB", "ZZB-REAL-TPL")]);
        let mut tiplocs = line_tiplocs(&line, &crs_to_tiploc);
        tiplocs.sort_unstable();
        assert_eq!(tiplocs, vec!["ZZA-REAL-TPL", "ZZB-REAL-TPL"]);
    }

    #[test]
    fn line_tiplocs_can_resolve_multiple_real_tiplocs_for_one_crs() {
        // A CRS can map to more than one real STANOX/TIPLOC (e.g. different
        // platforms/areas of one physical station) -- see
        // `crs_to_tiploc_map`'s own doc comment.
        let line = fixture_line("zzz-multi", vec![fixture_station("ZZA", None)]);
        let crs_to_tiploc = fixture_crs_to_tiploc(&[("ZZA", "ZZA-ONE"), ("ZZA", "ZZA-TWO")]);
        let mut tiplocs = line_tiplocs(&line, &crs_to_tiploc);
        tiplocs.sort_unstable();
        assert_eq!(tiplocs, vec!["ZZA-ONE", "ZZA-TWO"]);
    }

    #[test]
    fn crs_to_tiploc_map_inverts_stanox_crs_records_uppercasing_the_crs_key() {
        let records = vec![
            common::StanoxCrsRecord {
                stanox: "S1".to_string(),
                crs: "znt".to_string(),
                tiploc: "ZNOTIPLOC".to_string(),
                station_name: "TEST STATION".to_string(),
                source_sequence: 1,
            },
            common::StanoxCrsRecord {
                stanox: "S2".to_string(),
                crs: "ZNT".to_string(),
                tiploc: "ZNOTIPLOC2".to_string(),
                station_name: "TEST STATION".to_string(),
                source_sequence: 1,
            },
        ];
        let map = crs_to_tiploc_map(&records);
        let mut tiplocs = map.get("ZNT").cloned().unwrap_or_default();
        tiplocs.sort_unstable();
        assert_eq!(
            tiplocs,
            vec!["ZNOTIPLOC".to_string(), "ZNOTIPLOC2".to_string()]
        );
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
                day_offset: 0,
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
                day_offset: 0,
                destination_crs: Some("CRE".to_string()),
            }],
        );
        by_crs.insert(
            "WAT".to_string(),
            vec![schedule_query::ScheduleDeparture {
                uid: "U2".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                day_offset: 0,
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

    #[test]
    fn schedule_network_departures_rows_carries_day_offset_onto_each_published_departure() {
        // `ScheduleDeparture` derives `Serialize` -- this proves that
        // derive actually surfaces `day_offset` on the wire rather than
        // dropping it, since `schedule_network_departures_rows` never lists
        // fields by hand (it serializes the whole struct via `json!`'s
        // `Vec<ScheduleDeparture>` field). Backs
        // `GET /public/stations/{crs}/schedule-departures`, the exact route
        // `TrackTrainForm.tsx::pickCifDeparture` reads its picker rows from.
        let mut by_crs = std::collections::HashMap::new();
        by_crs.insert(
            "BKG".to_string(),
            vec![schedule_query::ScheduleDeparture {
                uid: "F49687".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(0, 7, 0).unwrap(),
                day_offset: 1,
                destination_crs: Some("SNF".to_string()),
            }],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let rows = schedule_network_departures_rows(by_crs, today);

        assert_eq!(rows.len(), 1);
        let departures = rows[0]["departures"].as_array().unwrap();
        assert_eq!(
            departures[0]["day_offset"], 1,
            "Barking 00:07's day_offset must reach the published row, not be dropped"
        );
    }

    #[test]
    fn schedule_destination_departures_rows_produces_one_flat_row_per_departure_carrying_its_destination()
     {
        // The load-bearing shape assertion: this function FLATTENS. Two
        // destinations holding three departures between them produce THREE
        // rows, not two, and each row names its own destination rather than
        // inheriting it from a bucket key it no longer has.
        let mut by_destination = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "U1".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
                schedule_query::DestinationDeparture {
                    uid: "U1".to_string(),
                    origin_crs: "CRE".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(10, 5, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        );
        by_destination.insert(
            "EDB".to_string(),
            vec![schedule_query::DestinationDeparture {
                uid: "U2".to_string(),
                origin_crs: "KGX".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                day_offset: 0,
                true_origin_crs: None,
                calling_point_arrival: None,
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let mut rows = schedule_destination_departures_rows(by_destination, today);
        // HashMap iteration order is unspecified; sort for a stable assert.
        rows.sort_by_key(|r| {
            (
                r["destination_crs"].as_str().unwrap().to_string(),
                r["scheduled"].as_str().unwrap().to_string(),
            )
        });

        assert_eq!(rows.len(), 3, "one row per DEPARTURE, not per destination");

        assert_eq!(
            rows[0],
            serde_json::json!({
                "service_date": "2026-09-07",
                "destination_crs": "EDB",
                "scheduled": "09:00:00",
                "day_offset": 0,
                "train_uid": "U2",
                "origin_crs": "KGX",
                "true_origin_crs": null,
                "calling_point_arrival": null,
                "destination_arrival": null,
                "destination_arrival_day_offset": 0,
            }),
            "exactly ten keys, named exactly as the table's columns are"
        );

        // The same UID appears twice under MAN, once per departure-bearing
        // calling point -- that is the whole point of the grouping, and the
        // table's PK (which includes origin_crs) admits both.
        assert_eq!(rows[1]["destination_crs"], "MAN");
        assert_eq!(rows[1]["train_uid"], "U1");
        assert_eq!(rows[1]["origin_crs"], "EUS");
        assert_eq!(rows[1]["scheduled"], "08:22:00");
        assert_eq!(rows[2]["destination_crs"], "MAN");
        assert_eq!(rows[2]["train_uid"], "U1");
        assert_eq!(rows[2]["origin_crs"], "CRE");
        assert_eq!(rows[2]["scheduled"], "10:05:00");

        for row in &rows {
            assert!(
                row.get("departures").is_none(),
                "there is no nested departures array any more -- the shape is flat"
            );
            assert!(
                row.get("uid").is_none(),
                "the JSON key is train_uid (the column name), not DestinationDeparture::uid"
            );
        }
    }

    #[test]
    fn schedule_destination_departures_rows_includes_the_true_origin_crs_field() {
        let mut by_destination: std::collections::HashMap<
            String,
            Vec<schedule_query::DestinationDeparture>,
        > = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "C11052".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: Some("EUS".to_string()),
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
                schedule_query::DestinationDeparture {
                    uid: "C11052".to_string(),
                    origin_crs: "CRE".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(10, 5, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        );
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();

        let rows = schedule_destination_departures_rows(by_destination, today);

        let eus_row = rows
            .iter()
            .find(|r| r["origin_crs"] == "EUS")
            .expect("EUS row present");
        assert_eq!(eus_row["true_origin_crs"], "EUS");

        let cre_row = rows
            .iter()
            .find(|r| r["origin_crs"] == "CRE")
            .expect("CRE row present");
        assert!(
            cre_row["true_origin_crs"].is_null(),
            "a None true_origin_crs must serialize as JSON null, not be omitted"
        );
    }

    #[test]
    fn schedule_destination_departures_rows_includes_the_destination_arrival_field() {
        let mut by_destination: std::collections::HashMap<
            String,
            Vec<schedule_query::DestinationDeparture>,
        > = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "C11052".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: Some("EUS".to_string()),
                    calling_point_arrival: None,
                    destination_arrival: Some(chrono::NaiveTime::from_hms_opt(11, 30, 0).unwrap()),
                    destination_arrival_day_offset: 0,
                },
                schedule_query::DestinationDeparture {
                    uid: "C99999".to_string(),
                    origin_crs: "CRE".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        );
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();

        let rows = schedule_destination_departures_rows(by_destination, today);

        let c11052_row = rows
            .iter()
            .find(|r| r["train_uid"] == "C11052")
            .expect("C11052 row present");
        assert_eq!(c11052_row["destination_arrival"], "11:30:00");

        let c99999_row = rows
            .iter()
            .find(|r| r["train_uid"] == "C99999")
            .expect("C99999 row present");
        assert!(
            c99999_row["destination_arrival"].is_null(),
            "a None destination_arrival must serialize as JSON null, not be omitted"
        );
    }

    #[test]
    fn schedule_destination_departures_rows_is_uncapped_and_keeps_every_entry_of_a_huge_bucket() {
        // Regression guard against a reintroduced cap. The real busiest
        // destination holds ~9,634 entries for one day
        // (docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
        // §1.1), so 9,634 is used here deliberately rather than a round
        // number: if anyone ever reintroduces a truncate, this fails.
        let mut by_destination = std::collections::HashMap::new();
        let departures: Vec<schedule_query::DestinationDeparture> = (0..9_634u32)
            .map(|i| schedule_query::DestinationDeparture {
                uid: format!("U{i:05}"),
                origin_crs: if i % 2 == 0 { "EUS" } else { "CRE" }.to_string(),
                // Seconds since midnight, wrapped into a real 24h clock.
                scheduled: chrono::NaiveTime::from_num_seconds_from_midnight_opt(i % 86_400, 0)
                    .unwrap(),
                day_offset: 0,
                true_origin_crs: None,
                calling_point_arrival: None,
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            })
            .collect();
        by_destination.insert("WAT".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(
            rows.len(),
            9_634,
            "every entry must survive -- there is no cap, by design"
        );
    }

    #[test]
    fn schedule_destination_departures_rows_does_not_sort_and_does_not_need_to() {
        // Explicitly records that ordering is NOT this function's job any
        // more. The read route's `ORDER BY scheduled, train_uid` rides
        // `schedule_destination_departures_calling_point_idx`
        // (queries::search_schedule_calling_point_departures), so a
        // publish-side sort would be pure wasted work over ~377,000 rows.
        // This test asserts the function is a faithful, order-preserving
        // flatten of each bucket rather than asserting a sort it must not do.
        let mut by_destination = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "LATE".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
                schedule_query::DestinationDeparture {
                    uid: "EARLY".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
                    day_offset: 0,
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]["train_uid"], "LATE",
            "input order within a bucket is preserved verbatim; no sort happens here"
        );
        assert_eq!(rows[1]["train_uid"], "EARLY");
    }

    /// Every field but `schedule_feed_ingests_url` is an inert placeholder
    /// -- these tests exercise only `seed_last_processed_delivery`, which
    /// touches that one field plus the `client`/`internal_oauth` arguments
    /// passed in alongside it. Same "config fixture with one caller-supplied
    /// knob" convention as this crate's sibling crates' own `test_config`
    /// helpers (e.g. `schedule-ingest::main::tests::test_config`).
    fn test_config(schedule_feed_ingests_url: &str) -> Config {
        Config {
            storage_dir: std::path::PathBuf::from("/tmp/schedule-reference-test-does-not-exist"),
            poll_interval_secs: 1800,
            api_ingest_url: "http://127.0.0.1:1/stanox-crs".to_string(),
            schedule_line_population_url: "http://127.0.0.1:1/schedule-line-population"
                .to_string(),
            schedule_network_departures_url: "http://127.0.0.1:1/schedule-network-departures"
                .to_string(),
            schedule_destination_departures_url:
                "http://127.0.0.1:1/schedule-destination-departures".to_string(),
            schedule_feed_ingests_url: schedule_feed_ingests_url.to_string(),
            lines: common::config::LineCatalogue(vec![]),
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: "placeholder-set-per-test-below".to_string(),
                internal_oauth_client_id: "test-client".to_string(),
                internal_oauth_scope: "groups".to_string(),
                internal_oauth_username: "test-user".to_string(),
                internal_oauth_password: "test-password".to_string(),
            },
            metrics_port: 0,
            metrics: common::service_args::MetricsArgs {
                metrics_enabled: false,
            },
        }
    }

    /// Mounts a token-issuing mock onto `server` and returns a token cache
    /// pointed at it -- mirrors `common::poller_loop::tests::token_cache`
    /// and `common::ingest::tests`' own mock-Authentik setup exactly (same
    /// `/token/` path, same fake-JWT response shape), so
    /// `seed_last_processed_delivery`'s real `common::ingest::get_json`
    /// call succeeds its bearer-token fetch before hitting whichever
    /// `/schedule-feed-ingests` mock each test below mounts separately.
    async fn mock_token_cache(server: &wiremock::MockServer) -> common::oauth_client::OAuthTokenCache {
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/token/"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake-jwt",
                "expires_in": 300,
            })))
            .mount(server)
            .await;
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: format!("{}/token/", server.uri()),
            client_id: "test-client".to_string(),
            scope: "groups".to_string(),
            username: "test-user".to_string(),
            password: "test-password".to_string(),
        })
    }

    #[test]
    fn dir_name_from_delivered_at_matches_schedule_ingests_own_directory_name_format() {
        // Mirrors schedule-ingest::delivery::delivery_dir_name's own fixture
        // (`delivery_dir_name_matches_the_expected_compact_sortable_format`)
        // byte-for-byte -- the whole point of this function is that the two
        // crates agree on this exact string for the exact same instant, so
        // a seeded `last_processed_delivery` actually matches the real
        // `discovery::CompleteDelivery::dir_name` a fresh disk scan finds.
        let delivered_at: chrono::DateTime<chrono::Utc> = "2026-09-03T17:28:30Z".parse().unwrap();
        assert_eq!(dir_name_from_delivered_at(delivered_at), "20260903T172830Z");
    }

    /// The actual regression test for this fix (2026-09-12): after a
    /// restart, this service must seed its dedup state from `api`'s real,
    /// persisted delivery record instead of unconditionally starting at
    /// `None`, so it doesn't redundantly republish `schedule_destination_departures`'
    /// whole 7-day forward window for a delivery already processed hours
    /// earlier.
    #[tokio::test]
    async fn seed_last_processed_delivery_seeds_from_a_real_prior_record_when_one_exists() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/schedule-feed-ingests"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "fetchedAt": "2026-09-03T17:28:30Z"
                })),
            )
            .mount(&server)
            .await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config(&format!("{}/schedule-feed-ingests", server.uri()));

        let seeded = seed_last_processed_delivery(&client, &config, &tokens).await;

        assert_eq!(
            seeded,
            Some("20260903T172830Z".to_string()),
            "must seed the exact dir_name a real discovery::latest_complete_delivery scan would find for this delivered_at"
        );
    }

    /// The other half of this fix's binding contract: a genuinely fresh
    /// deployment (an empty `schedule_feed_ingests` table, `fetchedAt:
    /// null`) is a real, valid, once-ever case -- not an error -- and must
    /// still fall back to this service's pre-existing first-run behavior.
    #[tokio::test]
    async fn seed_last_processed_delivery_falls_back_to_none_when_no_prior_record_exists() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/schedule-feed-ingests"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "fetchedAt": null
                })),
            )
            .mount(&server)
            .await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config(&format!("{}/schedule-feed-ingests", server.uri()));

        let seeded = seed_last_processed_delivery(&client, &config, &tokens).await;

        assert_eq!(
            seeded, None,
            "an empty schedule_feed_ingests table must fall back to None/first-run behavior, not error or panic"
        );
    }

    /// A failed GET (here: nothing mounted at all, so it 404s) must be as
    /// harmless as a genuinely fresh deployment -- this optimization must
    /// never become a hard startup failure. Same fallback posture as
    /// `common::ingest::time_until_next_poll`'s own "poll now" fallback on
    /// a failed freshness check.
    #[tokio::test]
    async fn seed_last_processed_delivery_falls_back_to_none_when_the_get_itself_fails() {
        let server = wiremock::MockServer::start().await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config(&format!("{}/schedule-feed-ingests", server.uri()));

        let seeded = seed_last_processed_delivery(&client, &config, &tokens).await;

        assert_eq!(seeded, None);
    }
}
