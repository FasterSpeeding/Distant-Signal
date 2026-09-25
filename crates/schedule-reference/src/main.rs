//! `schedule-reference`: a sibling container in the `schedulefeed` Pod.
//! Once `schedule-ingest` has extracted a verified-stable delivery into
//! `storage_dir/<timestamp>/` (see
//! `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`),
//! reads that delivery's `RJTTF*MCA.txt` (`TI` records) and `RJTTF*MSN.txt`
//! (`A` records) directly off the already-local, read-only-mounted PVC,
//! resolves a STANOX->CRS table, and POSTs it to `api`'s
//! `/private/stanox-crs`. See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md.

mod alf;
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
            "seeded last_processed_delivery from this service's OWN persisted publish-completion \
             marker; will not redundantly republish this delivery after a restart"
        ),
        None => tracing::info!(
            "no completed schedule-reference publish cycle recorded by api yet; will process the next delivery poll_once finds (first-run behavior)"
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

/// Every product one delivery's publish cycle is responsible for, and what
/// happened to it -- the bookkeeping that decides whether
/// `last_processed_delivery` may advance.
///
/// **The bug this type exists to fix.** `last_processed_delivery` used to be
/// advanced immediately after the FIRST of seven publishes (`stanox_crs`),
/// before any of the other six ran. Each of those six logged
/// `"...; will retry next cycle"` on failure -- and none of them ever did:
/// the very next cycle saw `last_processed_delivery == delivery.dir_name`,
/// short-circuited on "no new delivery since last successful parse", and did
/// not try again until a genuinely NEW delivery directory appeared, roughly
/// 24 hours later. An `api` pod restarting mid-cycle, the client's 30s
/// `REQUEST_TIMEOUT` expiring on a large body, one Postgres deadlock or one
/// 413 was enough to lose a whole day of one (or several) products, with a
/// single `error!` line and a log message that actively asserted the
/// opposite.
///
/// So failures are now classified, because "retry the whole delivery next
/// cycle" is only the right answer for some of them:
///
/// * `retryable` -- a later attempt at the SAME delivery could plausibly
///   succeed: every HTTP publish failure (`api` unreachable or restarting, a
///   timeout, a 413, a deadlock), and a failure to read the delivery's own
///   files (a transient PVC/NFS blip). These hold `last_processed_delivery`
///   where it was, so the next cycle reprocesses the whole delivery from
///   scratch. That is safe because every one of the seven publishes is
///   idempotent at the `api` end -- verified, not assumed: `upsert_stanox_crs`
///   and `upsert_tiploc_crs` are per-key `INSERT ... ON CONFLICT DO UPDATE`;
///   `upsert_fixed_links` is a whole-table `DELETE` + re-`INSERT` in one
///   transaction; `upsert_schedule_line_population` is a per-`(line_id,
///   service_date)` upsert; `upsert_schedule_network_departures` is a
///   per-`(crs, service_date)` upsert; and
///   `upsert_schedule_destination_departures`/
///   `upsert_schedule_calling_points_full` each replace their own
///   `service_date` in one transaction. None of them appends, none of them
///   increments, none of them has a side effect outside its own table -- so
///   re-publishing an already-successful product costs only the work, never
///   correctness.
/// * `permanent` -- a later attempt at the SAME delivery would fail
///   identically, because the input itself is the problem (this delivery's
///   ALF member parses to zero fixed links). Retrying that every 30 minutes
///   for a day would re-parse a 700MB+ file and rebuild ~1.7M+ rows over and
///   over to reach the same conclusion, so these are logged loudly and do
///   NOT hold the marker back.
///
/// A delivery with no ALF file at all is neither: it is a legitimate,
/// recorded state of that delivery (see `poll_once`'s own `warn!`), not a
/// failure of this cycle.
#[derive(Debug, Default)]
struct CycleOutcome {
    retryable_failures: Vec<String>,
    permanent_failures: Vec<String>,
}

impl CycleOutcome {
    /// Records a failure a later attempt at the same delivery could fix --
    /// the classification that holds `last_processed_delivery` back.
    fn retryable(&mut self, product: impl Into<String>) {
        self.retryable_failures.push(product.into());
    }

    /// Records a failure that is deterministic in this delivery's own input
    /// -- logged, but never worth reprocessing the delivery for.
    fn permanent(&mut self, product: impl Into<String>) {
        self.permanent_failures.push(product.into());
    }

    /// Whether this delivery may be marked processed. Deliberately keyed on
    /// `retryable_failures` alone -- see this type's own doc comment.
    fn may_advance_marker(&self) -> bool {
        self.retryable_failures.is_empty()
    }

    /// Whether every single product published, with nothing logged against
    /// it -- the only state that justifies writing the DURABLE completion
    /// marker (`POST /private/schedule-reference-publishes`), as opposed to
    /// merely advancing this process's in-memory marker.
    ///
    /// The two are not the same bar, on purpose. A permanent failure means
    /// this process should stop re-attempting the delivery (so the in-memory
    /// marker advances), but it must NOT claim in `api`'s durable record that
    /// the delivery published completely -- that record is the one piece of
    /// after-the-fact evidence anyone has, and a restart genuinely should
    /// re-attempt a delivery whose last cycle did not fully publish.
    fn fully_published(&self) -> bool {
        self.retryable_failures.is_empty() && self.permanent_failures.is_empty()
    }
}

/// Scans for the most recent complete delivery, skips if unchanged since
/// `last_processed_delivery`, else reads+parses+POSTs every product it
/// derives from that delivery, and only then advances
/// `last_processed_delivery` -- see [`CycleOutcome`] for why "only then" is
/// load-bearing and which failures hold it back.
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
    let msn_change_time = parser::parse_msn_change_time_by_tiploc(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs, &msn_change_time);
    let tiploc_rows = parser::resolve_tiploc_crs(&ti_records, &msn_crs, &msn_change_time);

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
            change_time_minutes: row.change_time_minutes,
        })
        .collect();

    let mut outcome = CycleOutcome::default();

    // No longer `?`. A failed `stanox_crs` POST used to abort the whole
    // cycle, which meant one transient failure on this ONE route also
    // withheld the other six products that had nothing wrong with them. It
    // is now recorded like every other publish: the remaining six still get
    // their chance this cycle, and the unadvanced marker is what guarantees
    // this one is retried on the next.
    if let Err(err) = common::ingest::post_batch(
        client,
        &config.api_ingest_url,
        internal_oauth,
        &records,
        "stanox/crs rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish stanox/crs rows; this delivery will be retried next cycle");
        outcome.retryable("stanox_crs");
    }

    // `tiploc_crs` (Task 4 of
    // docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) is a
    // strict superset of `stanox_crs` used for defense-in-depth. Like every
    // other publish in this cycle it is log-and-continue (a failure here
    // must not stop the publishes below it from being attempted), but --
    // unlike before this fix -- "continue" no longer means "and never try
    // again": the failure is recorded on `outcome`, which is what actually
    // makes the next cycle retry it.
    let tiploc_crs_records: Vec<common::TiplocCrsRecord> = tiploc_rows
        .into_iter()
        .map(|row| common::TiplocCrsRecord {
            tiploc: row.tiploc,
            crs: row.crs,
            station_name: row.station_name,
            stanox: row.stanox,
            source_sequence,
            change_time_minutes: row.change_time_minutes,
        })
        .collect();

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.tiploc_crs_url,
        internal_oauth,
        &tiploc_crs_records,
        "tiploc/crs rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish tiploc/crs rows; this delivery will be retried next cycle");
        outcome.retryable("tiploc_crs");
    }

    if let Some(alf_path) = &delivery.alf_path {
        publish_fixed_links(
            client,
            config,
            alf_path,
            internal_oauth,
            source_sequence,
            &mut outcome,
        )
        .await;
    } else {
        tracing::warn!(
            delivery = %delivery.dir_name,
            "this delivery has no ALF file; fixed-links data was not refreshed this cycle \
             (previous cycle's rows, if any, remain in place)"
        );
    }

    publish_cif_derived_products(
        client,
        config,
        &delivery.mca_path,
        internal_oauth,
        &records,
        &tiploc_crs_records,
        &mut outcome,
    )
    .await;

    if !outcome.may_advance_marker() {
        // The marker stays exactly where it was, so the NEXT cycle sees this
        // delivery as unprocessed and reruns the whole thing -- which is what
        // the log line every one of these publishes prints has always
        // claimed, and until this fix never did. Re-publishing the products
        // that DID succeed is idempotent and cheap; see `CycleOutcome`.
        tracing::error!(
            delivery = %delivery.dir_name,
            failed_products = ?outcome.retryable_failures,
            permanently_failed_products = ?outcome.permanent_failures,
            "not marking this delivery as processed: at least one product failed to publish this \
             cycle, so the whole delivery will be reprocessed and republished on the next cycle"
        );
        return Ok(());
    }

    if !outcome.permanent_failures.is_empty() {
        tracing::error!(
            delivery = %delivery.dir_name,
            permanently_failed_products = ?outcome.permanent_failures,
            "marking this delivery as processed despite a product failing in a way that \
             reprocessing the SAME delivery cannot fix -- that product's previous rows, if any, \
             remain in place until the next delivery lands; the durable completion marker is \
             deliberately NOT written, so a restart still re-attempts this delivery"
        );
    }

    *last_processed_delivery = Some(delivery.dir_name.clone());

    if outcome.fully_published() {
        record_completed_publish(client, config, internal_oauth, &delivery.dir_name).await;
    }

    Ok(())
}

/// Writes this service's OWN durable "delivery fully published" marker, the
/// one `seed_last_processed_delivery` reads back on the next start.
///
/// Best-effort, and deliberately so: if this POST fails, the in-memory
/// `last_processed_delivery` has already advanced (this process knows what it
/// published), so nothing is republished now; the only consequence is that a
/// restart before the next delivery falls back to first-run behavior and
/// republishes a delivery that was already complete. Wasteful for one cycle,
/// never data loss -- the exact same safe direction
/// `seed_last_processed_delivery`'s own failure path already takes. Failing
/// the cycle over it would be strictly worse: it would turn a bookkeeping
/// blip into a retry of a publish that already succeeded.
async fn record_completed_publish(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    delivery: &str,
) {
    let body = common::ingest::ScheduleReferencePublishRequest {
        delivery: delivery.to_string(),
    };
    match common::ingest::post_json(
        client,
        &config.schedule_reference_publishes_url,
        internal_oauth,
        &body,
    )
    .await
    {
        Ok(()) => tracing::info!(
            delivery = %delivery,
            "recorded this delivery's completed publish cycle with api; a restart will not \
             redundantly republish it"
        ),
        Err(err) => tracing::warn!(
            error = ?err,
            delivery = %delivery,
            "could not record this delivery's completed publish cycle with api; this process will \
             still not republish it, but a restart before the next delivery will (wasteful, not \
             incorrect)"
        ),
    }
}

/// Reads and parses `alf_path`'s already-local, read-only-mounted ALF
/// member, and POSTs the result as one full-replace batch (see
/// `queries::upsert_fixed_links`'s own doc comment). Best-effort: a read or
/// parse failure here logs and returns, exactly like every other
/// `publish_*` function's own log-and-continue posture (`main.rs`'s
/// existing convention throughout) -- it never propagates as a hard
/// `poll_once` failure, since fixed-links data degrading for one cycle
/// must never take down the STANOX/CRS or schedule-population publishes
/// that already succeeded this same cycle.
///
/// Also guards a THIRD failure shape, distinct from an unreadable file
/// (handled above) or an absent ALF file entirely (guarded by this
/// function's caller in `poll_once`, which simply never calls this function
/// when `delivery.alf_path` is `None`): a file that reads fine but parses to
/// zero links (`alf::parse_alf_lines` returns an empty `Vec` -- a zero-byte,
/// truncated, or format-changed ALF member). Without this guard, an empty
/// batch would flow straight through to `post_batch` and `api`'s
/// `upsert_fixed_links` (a full `DELETE` + zero `INSERT`s) would wipe the
/// table. So a zero-parsed batch is logged at `error` and this cycle's
/// publish is skipped entirely, leaving the previous cycle's rows in place --
/// the same "leave the old rows rather than delete them with nothing to
/// replace them" posture the absent-file case above already gets, just
/// extended to cover this distinct, file-present-but-empty shape too. It is
/// recorded as a [`CycleOutcome::permanent`] failure, not a retryable one:
/// re-reading the same unchanged file on the same unchanged delivery would
/// parse to zero links again, so holding the whole delivery back for it would
/// only re-run a 700MB+ parse every 30 minutes to reach the same answer.
///
/// An ALF *read* failure, by contrast, is recorded as
/// [`CycleOutcome::retryable`] -- a PVC/NFS read can fail transiently and
/// succeed on the next attempt, and this product going stale for a whole day
/// over one such blip is exactly the failure class this file's 2026-09-25
/// rework exists to close.
async fn publish_fixed_links(
    client: &Client,
    config: &Config,
    alf_path: &std::path::Path,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    source_sequence: i32,
    outcome: &mut CycleOutcome,
) {
    let text = match std::fs::read_to_string(alf_path) {
        Ok(text) => text,
        Err(err) => {
            tracing::error!(error = ?err, path = ?alf_path, "failed to read ALF file; skipping fixed-links publish this cycle");
            outcome.retryable("fixed_links (ALF read failed)");
            return;
        }
    };

    let records = alf::parse_alf_lines(&text);

    if records.is_empty() {
        tracing::error!(path = ?alf_path, "ALF file parsed to zero fixed links; \
            skipping publish rather than wiping the table");
        outcome.permanent("fixed_links (ALF parsed to zero links)");
        return;
    }

    tracing::info!(count = records.len(), "parsed ALF fixed links");

    let records: Vec<common::FixedLinkRecord> = records
        .into_iter()
        .map(|link| common::FixedLinkRecord {
            mode: link.mode,
            from_crs: link.from_crs,
            to_crs: link.to_crs,
            minutes: link.minutes,
            valid_from: link.valid_from,
            valid_to: link.valid_to,
            days_mask: link.days_mask,
            source_sequence,
        })
        .collect();

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.fixed_links_url,
        internal_oauth,
        &records,
        "fixed-link rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish fixed links; this delivery will be retried next cycle");
        outcome.retryable("fixed_links");
    }
}

/// Seeds `last_processed_delivery` from THIS SERVICE'S OWN persisted record
/// of the most recent delivery whose publish cycle actually completed (`GET
/// /private/schedule-reference-publishes`), rather than always starting at
/// `None` on a process restart.
///
/// Without any seeding at all, restarting this container (the `reference`
/// sibling in the `schedulefeed` Pod) always re-triggers a full, redundant
/// republish of `schedule_destination_departures` for the whole forward
/// window -- ~1.7-2 million rows torn down and rebuilt in Postgres -- even
/// when the underlying delivery was already fully processed hours earlier,
/// because `last_processed_delivery` lives only in this process's memory.
/// Confirmed directly against production: a delivery reprocessed at a real
/// restart had already been ingested 8.5 hours earlier.
///
/// **This reads a marker `schedule-reference` writes for itself, NOT
/// `schedule-ingest`'s `/schedule-feed-ingests` record, and that change is
/// the fix for a real, latent day-of-data-loss bug.** This function used to
/// read `MAX(delivered_at) FROM schedule_feed_ingests` -- which the SIBLING
/// container writes the instant it has EXTRACTED and verified a delivery zip,
/// before this service has read a single byte of it. The two are different
/// facts, and treating "extracted" as "published" meant that a restart of
/// this container between those two moments (an OOM kill during the
/// in-memory CIF parse of a 700MB+ file, a rolling deploy, any crash) seeded
/// `last_processed_delivery` to a delivery this service had never published.
/// `poll_once` then short-circuited on "no new delivery since last successful
/// parse", and every product for that delivery -- both CRS crosswalks, fixed
/// links, per-line population, network departures, and up to 8 days each of
/// destination departures and full calling points -- silently never
/// published until the next delivery landed, roughly 24 hours later. The
/// restart-dedup optimization was, in that window, a
/// lose-a-whole-day-of-data mechanism. See `poll_once`/[`CycleOutcome`] for
/// the other half of the same fix (what "completed" now has to mean before
/// the marker is written at all).
///
/// Returns `None` -- this service's pre-existing, still-correct
/// first-run/fallback behavior (`poll_once` processes the next delivery it
/// finds) -- in two distinct cases:
/// * `api` has never recorded a completed publish cycle (`delivery: None`)
///   -- a genuine, valid case (a fresh deployment's
///   `schedule_reference_publishes` table starts empty, and so does an
///   existing deployment's on the release that introduces it), not an error.
/// * The GET itself fails (network error, `api` not yet reachable, a bad
///   response) -- logged at `warn`, but never propagated as a hard startup
///   failure: this service must still be able to start and make forward
///   progress even if this one optimization can't be applied yet.
async fn seed_last_processed_delivery(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> Option<String> {
    let response: common::ingest::LastCompletedPublishResponse = match common::ingest::get_json(
        client,
        &config.schedule_reference_publishes_url,
        internal_oauth,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(
                error = ?err,
                "could not fetch this service's own last completed publish cycle from api on startup; falling back to first-run behavior for this process lifetime"
            );
            return None;
        }
    };
    response.delivery
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

/// Forward publish window for `schedule_calling_points_full` -- same value
/// as DESTINATION_DEPARTURES_FORWARD_DAYS (both are whole-network,
/// full-day products published on the same cycle for the same reason: a
/// trip-planning query needs the query date, which may be up to a week
/// ahead, immediately queryable without waiting for a same-day publish).
const TRIP_PLANNING_FORWARD_DAYS: i64 = 7;

/// Enforced at COMPILE time, not merely asserted at runtime (a
/// `debug_assert_eq!` would compile to nothing in a release build, giving
/// no real guarantee): `publish_cif_derived_products`'s per-date loop
/// deliberately reuses ONE `forward_publish_dates` call, bounded by
/// `DESTINATION_DEPARTURES_FORWARD_DAYS`, for both
/// `publish_schedule_destination_departures` and
/// `publish_schedule_calling_points_full` -- see that loop's own comment.
/// The two constants above are kept separate and independently documented
/// (they answer different design questions and could legitimately diverge
/// later), so this is what actually keeps the reused bound honest: if
/// either constant ever changes without the other, this fails the BUILD,
/// not just a debug-mode assertion, forcing whoever changes one to either
/// change both back into sync or split the loop into two.
const _: () = assert!(TRIP_PLANNING_FORWARD_DAYS == DESTINATION_DEPARTURES_FORWARD_DAYS);

/// `today..=today+forward_days`, inclusive, today first. Pure and
/// unit-testable without a mock HTTP server or a `ScheduleIndex`, same
/// convention as `lines_to_publish` just below it in this file.
fn forward_publish_dates(today: chrono::NaiveDate, forward_days: i64) -> Vec<chrono::NaiveDate> {
    (0..=forward_days)
        .map(|offset| today + chrono::Duration::days(offset))
        .collect()
}

/// This process's own "log this TIPLOC at least once" dedup set for
/// [`log_new_unresolved_booked_tiplocs`] -- see
/// `common::log_once::LogOnceSet`'s own doc comment for why a plain,
/// in-memory, process-lifetime set (not a DB table) is the right posture
/// here: `schedule-reference` restarts rarely and a real CIF full-timetable
/// delivery lands roughly once a day, so this publish step runs at most a
/// handful of times between restarts -- nowhere near often enough for even
/// "one extra log line per already-known gap after a restart" to matter.
static UNRESOLVED_TIPLOC_LOG: std::sync::LazyLock<common::log_once::LogOnceSet> =
    std::sync::LazyLock::new(common::log_once::LogOnceSet::new);

/// The parse-time half of this codebase's "a CIF calling point that looks
/// like it should be a real station never silently vanishes from
/// production logs" observability story -- added for the 2026-09-23
/// production incident (train `Y80908`) where a real, booked station
/// (Northampton, TIPLOC `NMPTN`) had no `stanox_crs` row at all and nothing
/// alerted anyone until a live forensic investigation was needed. See
/// `crates/api/src/data/journey.rs`'s `log_if_unresolved_booked_stop` for
/// the query-time half (the same class of gap, caught again wherever a
/// single train's journey is actually rendered, in case a gap appears in
/// scheduling data that this whole-network publish step somehow didn't
/// catch -- belt and braces, not redundant, since the two run in different
/// processes against slightly different views of the same underlying
/// data).
///
/// Delegates the actual "which TIPLOCs" decision entirely to
/// `schedule_query::unresolved_booked_tiplocs` (this crate has `tracing`;
/// that one deliberately does not -- see that function's own doc comment),
/// deduplicates via `UNRESOLVED_TIPLOC_LOG`, and logs each newly-seen one at
/// `warn`: a real, actionable data-quality signal worth a human eventually
/// reading and fixing the reference data for, but not urgent enough to page
/// anyone at 3am, and this publish step runs on every new CIF delivery for
/// as long as this process stays up -- without the dedup, a station whose
/// gap is already known and simply not yet fixed would re-log every single
/// delivery, forever.
///
/// As of this plan's Task 4, this takes `tiploc_crs_records` (the richer,
/// TIPLOC-primary crosswalk, see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) rather
/// than `stanox_crs_records`: either source is a valid choice here (both
/// are supersets of what this function actually needs, a plain
/// TIPLOC->CRS map), but `tiploc_crs_records` is the more consistent
/// choice since `publish_schedule_network_departures`/
/// `publish_schedule_destination_departures` (this same file) already
/// switched to it for their own `tiploc_to_crs` maps -- this function now
/// sees the SAME superset those two do, rather than the narrower,
/// STANOX-truncated one.
fn log_new_unresolved_booked_tiplocs(
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    tiploc_crs_records: &[common::TiplocCrsRecord],
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = tiploc_crs_records
        .iter()
        .map(|r| {
            (
                schedule_query::normalize_tiploc(&r.tiploc).to_string(),
                r.crs.clone(),
            )
        })
        .collect();

    for tiploc in schedule_query::unresolved_booked_tiplocs(index, today, &tiploc_to_crs) {
        if !UNRESOLVED_TIPLOC_LOG.should_log(&tiploc) {
            continue;
        }
        tracing::warn!(
            tiploc = %tiploc,
            service_date = %today,
            "CIF schedule calling point has a booked time but no tiploc_crs row at all (not \
             even an X-prefixed Network Rail pseudo-CRS) -- looks like it could be a real, \
             unmapped station rather than a legitimate non-station junction/timing point; \
             logged once per process, see reference-data/stanox-crs.md and this delivery's own \
             TI/A records for this TIPLOC to investigate"
        );
    }
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
///
/// As of this plan's Task 4, this also takes `tiploc_crs_records` (the
/// richer, TIPLOC-primary crosswalk `parser::resolve_tiploc_crs` produces,
/// see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) --
/// `stanox_crs_records` stays as a parameter too (unchanged) rather than
/// being removed. Both are now passed through to
/// `publish_schedule_line_population`, which unions them via
/// `crs_to_tiploc_map` (see that function's own doc comment for the
/// `AFK`/`EBD`/`SFA`/`POO`-class gap this closes).
async fn publish_cif_derived_products(
    client: &Client,
    config: &Config,
    mca_path: &std::path::Path,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    stanox_crs_records: &[common::StanoxCrsRecord],
    tiploc_crs_records: &[common::TiplocCrsRecord],
    outcome: &mut CycleOutcome,
) {
    let mca_schedule_text = match read_prefixed_lines_multi(
        mca_path,
        &["BS", "BX", "LO", "LI", "CR", "LT"],
    ) {
        Ok(text) => text,
        Err(err) => {
            tracing::error!(error = ?err, "failed to read CIF SCHEDULE records from delivery; skipping this cycle's CIF-derived publishes");
            // Retryable, not permanent: a read of the read-only-mounted PVC
            // can fail transiently, and this branch means FIVE of this
            // service's seven products published nothing at all this cycle.
            outcome.retryable("all CIF-derived products (MCA SCHEDULE read failed)");
            return;
        }
    };

    let index = schedule_query::ScheduleIndex::from_text(&mca_schedule_text);
    // schedule-reference has no rail-day concept of its own yet --
    // publishing against the plain calendar date is deliberate and
    // sufficient here:
    // `schedules_touching`/`departures_by_crs` both resolve STP overlays
    // per calendar date already, and `full-coverage-consumer`'s OWN
    // rail-day gating is what decides Pending/Available for the line
    // population, not this publish step.
    //
    // LONDON-local, not UTC. This was `chrono::Utc::now().date_naive()`
    // until 2026-09-25, which meant that during the 00:00-01:00 BST window
    // this container and its `schedule-ingest` sibling -- which has always
    // used London-local time for the equivalent decision, see
    // `schedule-ingest::main`'s own `Utc::now().with_timezone(&London)` --
    // disagreed about which rail day a delivery belonged to, and every
    // product published in that hour landed under YESTERDAY's date: stale
    // for the day it was published for, and overwriting a date whose own
    // data was already complete. The CIF times these products carry are
    // Europe/London civil time throughout (see `london_local_time_at`'s
    // own doc comment), so London-local is also the only date that makes
    // those times mean what they say.
    let today = london_local_date_now();

    log_new_unresolved_booked_tiplocs(&index, today, tiploc_crs_records);

    publish_schedule_line_population(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        tiploc_crs_records,
        internal_oauth,
        outcome,
    )
    .await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        tiploc_crs_records,
        internal_oauth,
        outcome,
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
    // `TRIP_PLANNING_FORWARD_DAYS` is a separate, independently-documented
    // constant from `DESTINATION_DEPARTURES_FORWARD_DAYS` -- the two answer
    // different design questions and could legitimately diverge later --
    // but this loop reuses the SAME `forward_publish_dates` call for both
    // per-date publishes below (this file's own "one pass, multiple
    // outputs" precedent, Task 1 Step 4). The compile-time `const _: ()`
    // assertion next to both constants' declarations, above, is what keeps
    // this reused bound honest if either constant ever changes.
    for date in forward_publish_dates(today, DESTINATION_DEPARTURES_FORWARD_DAYS) {
        publish_schedule_destination_departures(
            client,
            config,
            &index,
            date,
            tiploc_crs_records,
            internal_oauth,
            outcome,
        )
        .await;
        // Fourth CIF-derived product off the SAME one-per-cycle
        // ScheduleIndex and the SAME per-date loop as the sibling call
        // directly above -- one pass, multiple outputs, this file's own
        // established precedent.
        publish_schedule_calling_points_full(client, config, &index, date, internal_oauth, outcome)
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
/// As of the 2026-09-24 tiploc-crs-crosswalk gap fix, this also takes
/// `tiploc_crs_records` and passes both crosswalks through to
/// `crs_to_tiploc_map`, which now unions them -- see that function's own
/// doc comment for why `stanox_crs_records` alone permanently excluded a
/// real, checked-in set of CRS codes (`AFK`/`ASI`, `EBD`/`EBF`, `SDI`/`SFA`,
/// `POO`/`PFT`) that `tiploc_crs_records` resolves unambiguously.
///
/// The lint suppression below predates this fix (Task 3 Step 5's own
/// byte-for-byte constraint on this loop, since relaxed by this change):
/// `index` is `&ScheduleIndex` (caller-supplied) rather than an owned
/// `ScheduleIndex` built locally, so the loop body's `&index` trips
/// `clippy::needless_borrow`.
///
/// `too_many_arguments` is allowed for the same reason every sibling
/// `publish_*` in this file takes its inputs individually: they are all
/// caller-supplied, built once per cycle by `publish_cif_derived_products`,
/// and bundling them into a struct purely to satisfy an argument count would
/// hide which of them each publish actually reads. The eighth argument is
/// `outcome`, the per-cycle failure ledger the 2026-09-25 retry fix threads
/// through every publish -- see [`CycleOutcome`].
#[allow(clippy::needless_borrow, clippy::too_many_arguments)]
async fn publish_schedule_line_population(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    tiploc_crs_records: &[common::TiplocCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    outcome: &mut CycleOutcome,
) {
    let crs_to_tiploc = crs_to_tiploc_map(stanox_crs_records, tiploc_crs_records);
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
            tracing::error!(error = ?err, line_id = %line.id, "failed to publish schedule line population; this delivery will be retried next cycle");
            outcome.retryable(format!("schedule_line_population (line {})", line.id));
        }
    }
}

/// Real, CIF-derived CRS -> TIPLOC(s) map, inverted from the UNION of
/// `stanox_crs_records` and `tiploc_crs_records` -- the mirror image of the
/// `tiploc_to_crs` map `publish_schedule_network_departures`/
/// `publish_schedule_destination_departures` already build from the same
/// data, just keyed the other way round. A CRS can resolve to more than one
/// TIPLOC in practice (multiple STANOX rows can share a CRS, e.g. different
/// platforms/areas of one physical location -- see
/// `queries::list_stanox_crs_for_crs`'s own doc in `crates/api`), so this
/// is `Vec<String>`-valued, not a single TIPLOC.
///
/// As of the 2026-09-24 tiploc-crs-crosswalk gap fix, `tiploc_crs_records`
/// is unioned in alongside `stanox_crs_records` rather than this map being
/// built from `stanox_crs_records` alone. `stanox_crs` permanently
/// EXCLUDES 5 STANOX values (`89428`, `52215`, `89530`, `86935`, `87981`
/// -- see reference-data/stanox-crs.md) that each cover two TIPLOCs with
/// two genuinely different, non-`X`-prefixed real CRS: an ambiguity that is
/// unresolvable at the STANOX level, so `stanox_crs` drops both rows rather
/// than guess. That silently meant `crs_to_tiploc_map` had ZERO entries for
/// `AFK`/`ASI` (Ashford (Kent)/Ashford International), `EBD`/`EBF`
/// (Ebbsfleet International domestic/international), `SDI`/`SFA` (Stratford
/// International/its domestic platforms) and `POO`/`PFT` (Poole/Poole Ferry
/// Terminal) -- real stations several checked-in `lines/*.toml` files list
/// (e.g. `lines/southeastern-highspeed.toml`'s `AFK`/`EBD`/`SFA`,
/// `lines/swr-south-west-main.toml`'s `POO`), so any real schedule touching
/// ONLY one of these stations was silently excluded from
/// `schedule_line_population`.
///
/// `tiploc_crs` (`PRIMARY KEY (tiploc)`, see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) carries
/// every one of these TIPLOCs as its own independently-resolvable row --
/// unlike `stanox_crs`, there is nothing to disambiguate when the grouping
/// key IS the TIPLOC already, so inverting `tiploc_crs` (this direction)
/// is structurally unambiguous, unlike inverting `stanox_crs` (which is
/// exactly the STANOX-keyed direction that motivated excluding those 5
/// STANOX values in the first place). This function merges TIPLOC->CRS
/// from both sources FIRST (preferring `tiploc_crs_records` on any TIPLOC
/// present in both, the same deterministic convention
/// `queries::crs_for_tiploc`/`queries::list_stanox_crs_for_crs` in
/// `crates/api` already use for the same union), then inverts the merged
/// map once -- so a TIPLOC that resolves via `stanox_crs_records` alone
/// still resolves exactly as before, and one that resolves via
/// `tiploc_crs_records` only (or with a different CRS in each source) now
/// also resolves, deterministically.
fn crs_to_tiploc_map(
    stanox_crs_records: &[common::StanoxCrsRecord],
    tiploc_crs_records: &[common::TiplocCrsRecord],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut tiploc_to_crs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for record in stanox_crs_records {
        tiploc_to_crs.insert(record.tiploc.clone(), record.crs.clone());
    }
    // Inserted second so it overwrites any `stanox_crs_records`-derived
    // entry for the same TIPLOC key -- `tiploc_crs_records` wins on
    // conflict, matching `crates/api/src/data/queries.rs`'s own
    // `tiploc_crs`-preferred union convention.
    for record in tiploc_crs_records {
        tiploc_to_crs.insert(record.tiploc.clone(), record.crs.clone());
    }

    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (tiploc, crs) in tiploc_to_crs {
        map.entry(crs.to_uppercase()).or_default().push(tiploc);
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

/// As of this plan's Task 4, `tiploc_to_crs` (below) is built from
/// `tiploc_crs_records` (the richer, TIPLOC-primary crosswalk
/// `parser::resolve_tiploc_crs` produces) rather than
/// `stanox_crs_records` -- see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md. This
/// only changes which real, same-STANOX calling points can now resolve
/// simultaneously (e.g. Vauxhall's `VAUXHLM`/`VAUXHLW`); the batching,
/// capping and POST shape described just above are unchanged.
async fn publish_schedule_network_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    tiploc_crs_records: &[common::TiplocCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    outcome: &mut CycleOutcome,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = tiploc_crs_records
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
        tracing::error!(error = ?err, "failed to publish schedule-derived network departures; this delivery will be retried next cycle");
        outcome.retryable("schedule_network_departures");
    }
}

/// Pure sort/cap/JSON-shaping logic, split out of
/// `publish_schedule_network_departures` purely so it's unit-testable
/// without a mock HTTP server -- same "pure logic separated from I/O"
/// convention `lines_to_publish`/`read_prefixed_lines_multi` already
/// establish in this file.
///
/// Sorts by `(day_offset, scheduled)`, NOT bare `scheduled` -- a bucket
/// here holds every departure-bearing calling point at one CRS across
/// EVERY train for the day, so it routinely mixes an ordinary same-day
/// departure (`day_offset: 0`, e.g. `23:50`) with a genuine overnight
/// service's post-midnight calling point at the same station (`day_offset:
/// 1`, e.g. `00:07`) -- the exact live-confirmed c2c UID F49687 Barking
/// case `schedule_query::resolve`'s own `f49687_raw` fixture documents. A
/// bare `NaiveTime` sort put the `00:07` entry BEFORE the `23:50` one,
/// inverting true chronological order, and then `truncate` could drop a
/// genuinely-earlier same-day departure to make room for it -- this is the
/// exact bug class `CallingPoint::day_offset`/`assign_day_offsets` exist to
/// prevent everywhere else in this codebase (see
/// `crates/schedule-query/src/resolve.rs`'s own doc comment), just missed
/// here at publish time. Backs `GET /public/stations/{crs}/schedule-departures`,
/// the CIF fallback picker `TrackTrainForm.tsx::pickCifDeparture` reads.
fn schedule_network_departures_rows(
    mut by_crs: std::collections::HashMap<String, Vec<schedule_query::ScheduleDeparture>>,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_crs
        .drain()
        .map(|(crs, mut departures)| {
            departures.sort_by_key(|d| (d.day_offset, d.scheduled));
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
/// not ~55 or ~80. The later `operator_atoc` field is a nullable 2-char
/// string and does not move that estimate.
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
                    "operator_atoc": d.operator_atoc,
                })
            })
        })
        .collect()
}

/// The destination-keyed sibling of `publish_schedule_network_departures`
/// directly above: same one-batch-array POST shape, same `tiploc_to_crs`
/// map built from this cycle's already-resolved `tiploc_crs_records` (as
/// of this plan's Task 4 -- see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md, and
/// `publish_schedule_network_departures`'s own doc comment for the same
/// switch), same log-and-continue error posture (a failed POST just means
/// this delivery's grouping is discarded and rebuilt when the next one
/// lands). See
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
///    `for chunk in rows.chunks(50_000)`, calling
///    `common::ingest::post_batch` once per chunk with
///    `?first_chunk=true` on the URL for the first chunk and
///    `?first_chunk=false` for every chunk after it (see the single call
///    below, which always sends `?first_chunk=true` today since this
///    function still sends exactly one call per date) -- addendum §3's
///    documented fallback, and Task 1 Step 3 of this plan. The ingest side
///    of "the first chunk clears the day" already exists
///    (`queries::upsert_schedule_destination_departures_chunk`,
///    `crates/api/src/routes/ingest.rs::post_schedule_destination_departures`'s
///    `?first_chunk=` query parameter) -- turning this fallback on is then
///    just this loop change, no further API-side work.
async fn publish_schedule_destination_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    tiploc_crs_records: &[common::TiplocCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    outcome: &mut CycleOutcome,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = tiploc_crs_records
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

    if let Err(err) = post_date_scoped_rows_in_chunks(
        client,
        &config.schedule_destination_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived destination departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, %today, "failed to publish schedule-derived destination departures; this delivery will be retried next cycle");
        outcome.retryable("schedule_destination_departures");
    }
}

/// Pure row-building logic split out of `publish_schedule_calling_points_full`,
/// for the same "unit-testable without a mock HTTP server" reason
/// `schedule_network_departures_rows`/`schedule_destination_departures_rows`
/// exist as their own functions.
///
/// **Filters to genuinely public calling points (2026-09-25 fix).** Before
/// this change, every `LO`/`LI`/`LT` calling point of every non-cancelled
/// schedule was published here regardless of its CIF Activity code -- this
/// is the feed `crates/trip-planner`'s connection-graph builders read (via
/// `crates/api`'s `schedule_calling_points_full` table), so a request-stop,
/// set-down-only, pickup-only or genuinely not-advertised-to-the-public stop
/// was offered to the graph exactly like a real, boardable one, with no
/// signal distinguishing them. Reuses [`schedule_query::records::CallingPoint::is_public_pickup`]
/// -- the same Activity-code decode this crate's High/Medium pass already
/// built and proved for the departure-board/search paths
/// (`schedules_touching`, `departures_by_crs`, `departures_by_destination_crs`)
/// -- rather than re-deriving a second copy of the same activity-code table.
///
/// **`Terminate` calling points are always kept, regardless of
/// `is_public_pickup()`.** This is a deliberate asymmetry, not an oversight:
/// `is_public_pickup` answers "can a passenger BOARD here," and a schedule's
/// own terminating stop is where a passenger currently ON the train
/// ALIGHTS, not somewhere anyone boards to continue on this same service --
/// its real CIF Activity code is almost always `TF` ("train finishes"),
/// which is not one of [`schedule_query::records::CallingPoint`]'s pickup
/// codes (`T`/`TB`/`U`/`R`), so `is_public_pickup()` returns `false` for
/// essentially every real `Terminate` calling point that exists. Filtering
/// `Terminate` rows by boardability would therefore delete every schedule's
/// own destination from this feed -- breaking
/// `queries::list_schedule_calling_points_full_for_train`'s own documented
/// contract that "the schedule's own terminating calling point is already
/// one of these rows" (that function backs `journey::build_journey_stops`'s
/// fallback source), which is a strictly worse regression than the gap this
/// fix closes. A `Terminate` row genuinely marked not-advertised-to-the-public
/// (CIF's `N` code) is a real, currently-unhandled edge case this fix does
/// NOT attempt to solve -- this crate has no real-fixture evidence of one
/// occurring (every real `LT` fixture in this codebase's own tests decodes
/// `TF`), and inventing handling for an unverified shape would violate this
/// crate's own "no invented API details" convention; it is left as a known,
/// narrower gap rather than blessed as correct.
///
/// `seq` is re-numbered contiguously from 0 over the FILTERED sequence, not
/// the original one -- its only documented job is to preserve true stopping
/// order for `ORDER BY seq` (see `queries::ScheduleCallingPointsFullRow::seq`'s
/// own doc comment: "NOT a real CIF field, assigned at publish time"), which
/// a contiguous renumbering does exactly as well as a gappy one, with a
/// simpler on-the-wire shape.
fn schedule_calling_points_full_rows(
    index: &schedule_query::ScheduleIndex,
    date: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        let public_calling_points = resolved.calling_points.iter().filter(|cp| {
            cp.is_public_pickup() || cp.kind == schedule_query::CallingPointKind::Terminate
        });
        for (seq, cp) in public_calling_points.enumerate() {
            let kind = match cp.kind {
                schedule_query::CallingPointKind::Origin => "origin",
                schedule_query::CallingPointKind::Intermediate => "intermediate",
                schedule_query::CallingPointKind::Terminate => "terminate",
            };
            rows.push(serde_json::json!({
                "service_date": date,
                "uid": resolved.uid,
                "seq": seq as i32,
                "tiploc": schedule_query::normalize_tiploc(&cp.tiploc).to_string(),
                "kind": kind,
                "booked_arrival": cp.booked_arrival,
                "booked_departure": cp.booked_departure,
                "day_offset": cp.day_offset,
            }));
        }
    }
    rows
}

/// Publishes this cycle's whole-network resolved calling points for `date`
/// -- the literal, un-bucketed persistence Phase 2 of the dynamic
/// trip-planning plan adds (see that plan's Task 1). Unlike
/// `publish_schedule_destination_departures`'s own bucketed/flattened
/// shape, this emits every calling point of every non-cancelled schedule,
/// in order, with no `now`-forward filter at all (a trip-planning query
/// needs the WHOLE day, including departures already in the past relative
/// to publish time, since the traveller picks their own date/time at query
/// time, not at publish time -- same reasoning as
/// `publish_schedule_destination_departures`'s own `NaiveTime::MIN`, see
/// that function's doc comment, point 1).
///
/// **Chunked as of 2026-09-25**, via [`post_date_scoped_rows_in_chunks`] --
/// the `rows.chunks(50_000)` fallback this doc comment previously described as
/// a follow-up "just this loop change" away is now what actually runs, on every
/// publish, rather than waiting for a measurement to force it. This is the
/// largest product this service publishes (every `LO`/`LI`/`LT` calling point
/// of every non-cancelled schedule, including the passing points and junction
/// TIPLOCs its departure-bearing sibling excludes, so realistically 2-3x that
/// sibling's ~377,000 rows per date) and it publishes eight dates per cycle,
/// which put a single un-chunked POST plausibly at or over `api`'s
/// `DefaultBodyLimit::max(100 * 1024 * 1024)` (`crates/api/src/routes/mod.rs`)
/// and/or this crate's 30s `REQUEST_TIMEOUT`. See [`PUBLISH_CHUNK_ROWS`] for
/// the sizing and [`post_date_scoped_rows_in_chunks`] for the `?first_chunk=`
/// contract -- already supported on the ingest side
/// (`queries::upsert_schedule_calling_points_full_chunk`) -- that keeps
/// chunking from turning into per-chunk data loss.
///
/// **Non-public calling points are filtered out before publish (2026-09-25;
/// separate from and in addition to the 2026-09-25 chunking change above).**
/// See [`schedule_calling_points_full_rows`], the pure row-building function
/// this now delegates to, for the full reasoning -- this product feeds the
/// dynamic trip-planning connections graph (`crates/trip-planner`), and
/// before this fix it published every stop CIF marks non-public (a
/// set-down-only, pickup-only, operational or not-advertised stop)
/// alongside every genuinely boardable one, with nothing distinguishing
/// them on the wire. A connections graph built directly off that feed could
/// offer to route a passenger through a stop CIF says they can never
/// actually board or alight at.
async fn publish_schedule_calling_points_full(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    date: chrono::NaiveDate,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    outcome: &mut CycleOutcome,
) {
    let rows = schedule_calling_points_full_rows(index, date);

    if let Err(err) = post_date_scoped_rows_in_chunks(
        client,
        &config.schedule_calling_points_full_url,
        internal_oauth,
        &rows,
        "schedule-derived full calling-point rows",
    )
    .await
    {
        tracing::error!(error = ?err, %date, "failed to publish schedule calling points; this delivery will be retried next cycle");
        outcome.retryable("schedule_calling_points_full");
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

/// The London-local CALENDAR DATE at `instant` -- the date half of
/// [`london_local_time_at`] directly above, and the date every CIF-derived
/// product is published under (`publish_cif_derived_products`'s `today`).
///
/// `publish_cif_derived_products` used `chrono::Utc::now().date_naive()` until
/// 2026-09-25. For the ~7 months of British Summer Time that is the WRONG
/// DATE for a full hour every night: between 00:00 and 01:00 BST the UTC date
/// is still yesterday's. The sibling `schedule-ingest` container has always
/// decided its own equivalent day question in London-local time
/// (`Utc::now().with_timezone(&London)`), so for that hour the two containers
/// in the same Pod disagreed about which rail day a delivery belonged to, and
/// a cycle that ran inside it published today's timetable under yesterday's
/// `service_date` -- overwriting a complete day with a day of data that does
/// not belong to it, and leaving the real current day unpublished. Not
/// theoretical for the products in question: `schedule-ingest`'s deliveries
/// genuinely land in the evening/overnight window.
///
/// Same `DateTime::with_timezone` direction as [`london_local_time_at`], so
/// the same "always exactly one unambiguous answer" reasoning applies -- no
/// `LocalResult` handling is needed here either.
fn london_local_date_at(instant: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDate {
    instant
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive()
}

fn london_local_date_now() -> chrono::NaiveDate {
    london_local_date_at(chrono::Utc::now())
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

/// How many rows go in one POST to a date-scoped, wholesale-replace ingest
/// route (`/schedule-destination-departures`,
/// `/schedule-calling-points-full`).
///
/// 50,000 is the figure both those products' own doc comments have carried as
/// the documented-but-unimplemented fallback since they were written
/// (`for chunk in rows.chunks(50_000)`), now implemented. At the ~100 bytes
/// per destination-departures row that product's sizing design measured, one
/// chunk is roughly 5MB of JSON -- about 5% of `api`'s
/// `DefaultBodyLimit::max(100 * 1024 * 1024)` (`crates/api/src/routes/mod.rs`)
/// and small enough that a single chunk comfortably fits inside this crate's
/// 30-second [`REQUEST_TIMEOUT`], which is the limit that actually binds
/// first.
///
/// **Why this was closed proactively rather than after a failure.**
/// `schedule_calling_points_full` emits every `LO`/`LI`/`LT` calling point of
/// every non-cancelled schedule -- including passing points and junction
/// TIPLOCs that its sibling `schedule_destination_departures` (departure-
/// bearing calling points only) excludes -- so it is realistically 2-3x that
/// product's ~377,000 rows per date, and it publishes EIGHT dates per cycle.
/// A single un-chunked POST of that was plausibly at or over the 100MB body
/// limit and/or the 30s client timeout, and the failure mode was invisible:
/// one `error!` line, then (before this same commit's `CycleOutcome` fix) no
/// retry until the next delivery.
const PUBLISH_CHUNK_ROWS: usize = 50_000;

/// POSTs `rows` to a date-scoped, wholesale-replace ingest route in
/// [`PUBLISH_CHUNK_ROWS`]-sized chunks, telling the route explicitly which
/// chunk is allowed to clear the date.
///
/// **The contract, and the data-loss bug it exists to avoid.** Both target
/// routes replace a service date by `DELETE ... WHERE service_date =
/// ANY(...)` followed by an `INSERT ... UNNEST(...)`, in one transaction. If
/// every chunk of one date ran that unchanged, chunk 2 would delete
/// everything chunk 1 had just inserted and the date would end up holding
/// only the LAST chunk -- a far worse bug than the oversized body this
/// chunking exists to prevent. So `?first_chunk=true` is sent on the FIRST
/// chunk of a date only (clear the date, then insert), and
/// `?first_chunk=false` on every chunk after it (insert only).
///
/// `first_chunk` is `api`'s OWN already-landed parameter name for exactly this
/// (`routes::ingest::ScheduleChunkParams`,
/// `queries::upsert_schedule_calling_points_full_chunk`) -- the ingest side
/// added it ahead of a publisher that would use it, and this is that
/// publisher. Getting the name wrong here would be silent and catastrophic:
/// `api` would ignore the unknown parameter, every chunk would take the
/// `DELETE` branch, and each date would keep only its last chunk.
///
/// **Partial-date exposure, and why it is the right trade.** A failure part
/// way through a date (chunk 5 of 12 times out) leaves that date holding
/// chunks 1-4 instead of either the old data or the complete new data -- the
/// one thing the previous single-transaction shape could not do. That is
/// bounded and self-healing: the failure is recorded on [`CycleOutcome`], the
/// delivery marker does not advance, and the next cycle (≤ `poll_interval_secs`
/// later) republishes the whole date starting from a `first_chunk=true` chunk.
/// The alternative -- a body that never lands at all, for a whole day, with
/// one log line -- is not better, it is just less visible.
async fn post_date_scoped_rows_in_chunks(
    client: &Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    rows: &[serde_json::Value],
    noun: &str,
) -> anyhow::Result<()> {
    // An empty publish is still POSTed, exactly once, rather than skipped:
    // both receiving routes treat an empty batch as a deliberate no-op that
    // must NOT delete the date (see `queries::upsert_schedule_calling_points_full`'s
    // own "an empty `rows` is a no-op, and that is load-bearing"), and
    // sending it keeps this cycle's `posted 0 <noun>` log line -- the only
    // evidence that the publish ran at all and genuinely had nothing to say.
    if rows.is_empty() {
        return common::ingest::post_batch(client, &first_chunk_url(url, true), tokens, rows, noun)
            .await;
    }

    let chunk_count = rows.len().div_ceil(PUBLISH_CHUNK_ROWS);
    for (index, chunk) in rows.chunks(PUBLISH_CHUNK_ROWS).enumerate() {
        let first_chunk = index == 0;
        common::ingest::post_batch(
            client,
            &first_chunk_url(url, first_chunk),
            tokens,
            chunk,
            noun,
        )
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "chunk {}/{chunk_count} ({} rows, first_chunk={first_chunk}) failed: {err}",
                index + 1,
                chunk.len(),
            )
        })?;
    }
    Ok(())
}

/// Appends the `first_chunk` query parameter
/// [`post_date_scoped_rows_in_chunks`] uses to tell a date-scoped ingest route
/// whether this chunk is the one that clears the date -- `api`'s own parameter
/// name for it, see that function's doc comment. Handles a URL that already
/// carries a query string, since these URLs come from configuration and
/// nothing stops an operator setting one.
fn first_chunk_url(url: &str, first_chunk: bool) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}first_chunk={first_chunk}")
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

    /// A minimal, real-shaped `TiplocCrsRecord` for `crs_to_tiploc_map`
    /// tests -- `stanox`/`station_name` are filled with placeholder values
    /// since those two fields don't participate in `crs_to_tiploc_map`'s
    /// own logic at all.
    fn fixture_tiploc_crs_record(tiploc: &str, crs: &str) -> common::TiplocCrsRecord {
        common::TiplocCrsRecord {
            tiploc: tiploc.to_string(),
            crs: crs.to_string(),
            station_name: format!("{crs} TEST STATION"),
            stanox: "00000".to_string(),
            source_sequence: 1,
            change_time_minutes: None,
        }
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
                change_time_minutes: None,
            },
            common::StanoxCrsRecord {
                stanox: "S2".to_string(),
                crs: "ZNT".to_string(),
                tiploc: "ZNOTIPLOC2".to_string(),
                station_name: "TEST STATION".to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            },
        ];
        let map = crs_to_tiploc_map(&records, &[]);
        let mut tiplocs = map.get("ZNT").cloned().unwrap_or_default();
        tiplocs.sort_unstable();
        assert_eq!(
            tiplocs,
            vec!["ZNOTIPLOC".to_string(), "ZNOTIPLOC2".to_string()]
        );
    }

    /// The real regression test for the 2026-09-24 tiploc-crs-crosswalk gap
    /// this fix closes: before it, `crs_to_tiploc_map` inverted
    /// `stanox_crs_records` alone, which -- per
    /// reference-data/stanox-crs.md's own documented exclusion policy --
    /// permanently has ZERO rows for `AFK`/`ASI` (STANOX `89428`),
    /// `EBD`/`EBF` (STANOX `89530`), `SDI`/`SFA` (STANOX `52215`) and
    /// `POO`/`PFT` (STANOX `86935`): each of these STANOX covers two
    /// TIPLOCs with two genuinely different, non-`X`-prefixed real CRS, an
    /// ambiguity `stanox_crs` leaves unresolved by excluding both rows
    /// entirely rather than guessing. Real TIPLOC codes, from
    /// reference-data/crs-tiploc.csv: `ASHFKY`/`ASHFKI`, `EBSFDOM`/
    /// `EBSFLTI`, `STFODOM`/`STFORDI`, `POOLE`/`POLEFT`.
    #[test]
    fn crs_to_tiploc_map_resolves_the_four_real_stanox_excluded_crs_pairs_via_tiploc_crs() {
        // stanox_crs_records deliberately empty -- exactly like the real,
        // permanent exclusion of these STANOX values from stanox_crs.
        let stanox_crs_records: Vec<common::StanoxCrsRecord> = vec![];
        let tiploc_crs_records = vec![
            fixture_tiploc_crs_record("ASHFKY", "AFK"),
            fixture_tiploc_crs_record("ASHFKI", "ASI"),
            fixture_tiploc_crs_record("EBSFDOM", "EBD"),
            fixture_tiploc_crs_record("EBSFLTI", "EBF"),
            fixture_tiploc_crs_record("STFODOM", "SFA"),
            fixture_tiploc_crs_record("STFORDI", "SDI"),
            fixture_tiploc_crs_record("POOLE", "POO"),
            fixture_tiploc_crs_record("POLEFT", "PFT"),
        ];

        let map = crs_to_tiploc_map(&stanox_crs_records, &tiploc_crs_records);

        assert_eq!(map.get("AFK"), Some(&vec!["ASHFKY".to_string()]));
        assert_eq!(map.get("ASI"), Some(&vec!["ASHFKI".to_string()]));
        assert_eq!(map.get("EBD"), Some(&vec!["EBSFDOM".to_string()]));
        assert_eq!(map.get("EBF"), Some(&vec!["EBSFLTI".to_string()]));
        assert_eq!(map.get("SFA"), Some(&vec!["STFODOM".to_string()]));
        assert_eq!(map.get("SDI"), Some(&vec!["STFORDI".to_string()]));
        assert_eq!(map.get("POO"), Some(&vec!["POOLE".to_string()]));
        assert_eq!(map.get("PFT"), Some(&vec!["POLEFT".to_string()]));
    }

    /// Non-regression half of the fix above: a CRS resolvable via
    /// `stanox_crs_records` alone (no `tiploc_crs_records` row at all) must
    /// keep resolving exactly as before this fix.
    #[test]
    fn crs_to_tiploc_map_still_resolves_a_crs_that_only_stanox_crs_carries() {
        let stanox_crs_records = vec![common::StanoxCrsRecord {
            stanox: "72410".to_string(),
            crs: "EUS".to_string(),
            tiploc: "EUSTON".to_string(),
            station_name: "LONDON EUSTON".to_string(),
            source_sequence: 1,
            change_time_minutes: None,
        }];
        let map = crs_to_tiploc_map(&stanox_crs_records, &[]);
        assert_eq!(map.get("EUS"), Some(&vec!["EUSTON".to_string()]));
    }

    /// Deterministic preference convention: when the SAME TIPLOC appears in
    /// both sources with a DIFFERENT CRS, `tiploc_crs_records` wins --
    /// mirroring `crates/api/src/data/queries.rs`'s own `tiploc_crs`-
    /// preferred union for the TIPLOC->CRS direction (`crs_for_tiploc`/
    /// `list_stanox_crs_for_crs`).
    #[test]
    fn crs_to_tiploc_map_prefers_tiploc_crs_over_stanox_crs_on_a_conflicting_same_tiploc() {
        let stanox_crs_records = vec![common::StanoxCrsRecord {
            stanox: "S1".to_string(),
            crs: "OLD".to_string(),
            tiploc: "ZZCONFLICT".to_string(),
            station_name: "TEST STATION".to_string(),
            source_sequence: 1,
            change_time_minutes: None,
        }];
        let tiploc_crs_records = vec![fixture_tiploc_crs_record("ZZCONFLICT", "NEW")];

        let map = crs_to_tiploc_map(&stanox_crs_records, &tiploc_crs_records);

        assert_eq!(map.get("NEW"), Some(&vec!["ZZCONFLICT".to_string()]));
        assert!(
            !map.contains_key("OLD"),
            "the stale stanox_crs CRS for this TIPLOC must not also appear"
        );
    }

    /// The end-to-end regression test this gap fix was reviewed against: a
    /// line whose ONLY real CIF-resolvable station is one of these
    /// previously-unresolvable CRS codes must now be published by
    /// `lines_to_publish`, and `line_tiplocs`'s filter list for it must
    /// include the real TIPLOC -- where before this fix (`crs_to_tiploc_map`
    /// built from `stanox_crs_records` alone) it would have been silently
    /// dropped, exactly like `lines/southeastern-highspeed.toml`'s real
    /// `AFK`/`EBD`/`SFA` stations and `lines/swr-south-west-main.toml`'s
    /// real `POO`.
    #[test]
    fn lines_to_publish_includes_a_line_whose_only_station_is_afk_ebd_sfa_or_poo_class_via_tiploc_crs()
     {
        let stanox_crs_records: Vec<common::StanoxCrsRecord> = vec![]; // permanently excluded, real-world
        let tiploc_crs_records = vec![
            fixture_tiploc_crs_record("ASHFKY", "AFK"),
            fixture_tiploc_crs_record("EBSFDOM", "EBD"),
            fixture_tiploc_crs_record("STFODOM", "SFA"),
            fixture_tiploc_crs_record("POOLE", "POO"),
        ];
        let crs_to_tiploc = crs_to_tiploc_map(&stanox_crs_records, &tiploc_crs_records);

        let lines = vec![
            fixture_line(
                "southeastern-highspeed-like",
                vec![fixture_station("AFK", None)],
            ),
            fixture_line(
                "southeastern-ebbsfleet-like",
                vec![fixture_station("EBD", None)],
            ),
            fixture_line(
                "southeastern-stratford-like",
                vec![fixture_station("SFA", None)],
            ),
            fixture_line(
                "swr-south-west-main-like",
                vec![fixture_station("POO", None)],
            ),
        ];

        let published: Vec<&str> = lines_to_publish(&lines, &crs_to_tiploc)
            .map(|l| l.id.as_str())
            .collect();
        assert_eq!(
            published,
            vec![
                "southeastern-highspeed-like",
                "southeastern-ebbsfleet-like",
                "southeastern-stratford-like",
                "swr-south-west-main-like",
            ]
        );

        // Before this fix, crs_to_tiploc_map built from stanox_crs_records
        // alone would have had zero entries for any of these 4 CRS codes,
        // so this exact lines_to_publish call would have returned nothing.
        let crs_to_tiploc_before_fix = crs_to_tiploc_map(&stanox_crs_records, &[]);
        let published_before_fix: Vec<&str> = lines_to_publish(&lines, &crs_to_tiploc_before_fix)
            .map(|l| l.id.as_str())
            .collect();
        assert!(
            published_before_fix.is_empty(),
            "sanity check: without tiploc_crs_records, none of these AFK/EBD/SFA/POO-only lines \
             resolve, confirming the fixture actually exercises this fix"
        );

        let afk_tiplocs = line_tiplocs(&lines[0], &crs_to_tiploc);
        assert_eq!(afk_tiplocs, vec!["ASHFKY"]);
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
    fn schedule_network_departures_rows_sorts_by_day_offset_before_scheduled_time() {
        // The real live-confirmed regression this fix targets (see
        // `schedule_network_departures_rows`'s own doc comment and
        // `schedule_query::resolve`'s `f49687_raw` fixture): a bucket at one
        // CRS mixes an ordinary same-day departure with a genuine overnight
        // service's post-midnight calling point at the SAME station. A bare
        // `scheduled`-only sort would put F49687's `00:07` (day_offset 1)
        // BEFORE the other train's `23:50` (day_offset 0), even though
        // `00:07` is really the NEXT calendar day and so chronologically
        // LATER.
        let mut by_crs = std::collections::HashMap::new();
        by_crs.insert(
            "BKG".to_string(),
            vec![
                schedule_query::ScheduleDeparture {
                    uid: "F49687".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(0, 7, 0).unwrap(),
                    day_offset: 1,
                    destination_crs: Some("SNF".to_string()),
                },
                schedule_query::ScheduleDeparture {
                    uid: "C11052".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(23, 50, 0).unwrap(),
                    day_offset: 0,
                    destination_crs: Some("CRE".to_string()),
                },
            ],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let rows = schedule_network_departures_rows(by_crs, today);

        assert_eq!(rows.len(), 1);
        let departures = rows[0]["departures"].as_array().unwrap();
        assert_eq!(
            departures[0]["uid"], "C11052",
            "23:50 on day_offset 0 is chronologically FIRST, even though its bare clock time is \
             larger than the other entry's"
        );
        assert_eq!(
            departures[1]["uid"], "F49687",
            "00:07 on day_offset 1 is really the next calendar day, so it must sort LAST here"
        );
    }

    #[test]
    fn schedule_network_departures_rows_truncation_never_drops_an_earlier_same_day_departure_for_an_overnight_one()
     {
        // The truncation half of the same bug, at the REAL
        // `MAX_DEPARTURES_PER_STATION` cap (10): ten ordinary same-day
        // departures plus one genuine overnight (day_offset 1) calling
        // point at the same station, one entry over the cap. Under the old
        // bare-`scheduled` sort, the day_offset-1 entry's small clock value
        // (`00:07`) sorted FIRST, so `truncate(10)` kept it and dropped the
        // truly-latest same-day departure (`08:09`) instead of the entry
        // that is genuinely latest in real chronological order.
        let mut departures: Vec<schedule_query::ScheduleDeparture> = (0..10)
            .map(|hour| schedule_query::ScheduleDeparture {
                uid: format!("SAME-DAY-{hour:02}"),
                scheduled: chrono::NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
                day_offset: 0,
                destination_crs: None,
            })
            .collect();
        departures.push(schedule_query::ScheduleDeparture {
            uid: "OVERNIGHT".to_string(),
            scheduled: chrono::NaiveTime::from_hms_opt(0, 7, 0).unwrap(),
            day_offset: 1,
            destination_crs: None,
        });
        let mut by_crs = std::collections::HashMap::new();
        by_crs.insert("BKG".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let rows = schedule_network_departures_rows(by_crs, today);
        let kept: Vec<&str> = rows[0]["departures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();

        assert_eq!(kept.len(), MAX_DEPARTURES_PER_STATION);
        assert_eq!(
            kept,
            vec![
                "SAME-DAY-00",
                "SAME-DAY-01",
                "SAME-DAY-02",
                "SAME-DAY-03",
                "SAME-DAY-04",
                "SAME-DAY-05",
                "SAME-DAY-06",
                "SAME-DAY-07",
                "SAME-DAY-08",
                "SAME-DAY-09",
            ],
            "the ten same-day departures, in true chronological order, must all survive the cap \
             -- the day_offset-1 OVERNIGHT entry (really the day AFTER every one of them) is the \
             one that is genuinely latest and correctly the one truncated away"
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
                    operator_atoc: None,
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
                    operator_atoc: None,
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
                operator_atoc: Some("SR".to_string()),
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
                "operator_atoc": "SR",
            }),
            "exactly eleven keys, named exactly as the table's columns are, \
             with a Some(\"SR\") operator_atoc round-tripping to the JSON string \"SR\""
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
                    operator_atoc: None,
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
                    operator_atoc: None,
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
                    operator_atoc: None,
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
                    operator_atoc: None,
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
                operator_atoc: None,
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
                    operator_atoc: None,
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
                    operator_atoc: None,
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

    /// Regression tests for the 2026-09-25 non-public-stop filter on
    /// `schedule_calling_points_full_rows` -- see that function's own doc
    /// comment for the full reasoning (this is the feed the dynamic
    /// trip-planning connections graph reads, and before this fix it
    /// included every CIF calling point regardless of public/non-public
    /// Activity code).
    mod schedule_calling_points_full_rows_tests {
        use super::*;

        // Real `BS`/`LO`/`LT` lines, byte-verbatim, already quoted and
        // verified in `schedule_query::parse`'s own tests.
        const BS_C00573_PERMANENT: &str =
            "BSNC005732605172612060000001 PXX1S003101121194800 DMU    125      S A T        P";
        const LO_EUSTON: &str = "LOEUSTON  0822 08227  C      TB";
        const LT_EUSTON: &str = "LTEUSTON  0804 08079     TF";
        // Real `LI` fixture (`LICARLILE ...`, Activity `T`) with ONLY its
        // Activity field overwritten to `D` (set-down only) -- same
        // byte-verbatim-except-one-field technique
        // `schedule_query::parse::activity_tests::with_activity` already
        // established, applied here directly since that helper is private
        // to its own crate.
        const LI_CARLILE_SET_DOWN_ONLY: &str = "LICARLILE 1202 1213      120212131        D";
        const LI_CARLILE_PUBLIC: &str = "LICARLILE 1202 1213      120212131        T";

        fn service_date() -> chrono::NaiveDate {
            chrono::NaiveDate::from_ymd_opt(2026, 5, 17).unwrap()
        }

        #[test]
        fn a_non_public_intermediate_stop_is_excluded_while_a_public_one_is_retained() {
            let text = format!(
                "{BS_C00573_PERMANENT}\n{LO_EUSTON}\n{LI_CARLILE_SET_DOWN_ONLY}\n{LT_EUSTON}"
            );
            let index = schedule_query::ScheduleIndex::from_text(&text);

            let rows = schedule_calling_points_full_rows(&index, service_date());

            let tiplocs: Vec<&str> = rows.iter().map(|r| r["tiploc"].as_str().unwrap()).collect();
            assert!(
                !tiplocs.contains(&"CARLILE"),
                "the set-down-only (non-public-pickup) intermediate stop must be excluded: {tiplocs:?}"
            );
            assert!(
                tiplocs.contains(&"EUSTON"),
                "the real, public Origin stop must still be published: {tiplocs:?}"
            );

            // Only Origin (EUSTON) and Terminate (EUSTON, the schedule loops
            // back in this synthetic fixture -- irrelevant to this test)
            // survive; the dropped LI leaves a real gap in `seq`.
            assert_eq!(
                rows.len(),
                2,
                "the non-public LI must not appear at all: {rows:?}"
            );
        }

        #[test]
        fn a_public_intermediate_stop_is_retained() {
            let text =
                format!("{BS_C00573_PERMANENT}\n{LO_EUSTON}\n{LI_CARLILE_PUBLIC}\n{LT_EUSTON}");
            let index = schedule_query::ScheduleIndex::from_text(&text);

            let rows = schedule_calling_points_full_rows(&index, service_date());

            let tiplocs: Vec<&str> = rows.iter().map(|r| r["tiploc"].as_str().unwrap()).collect();
            assert!(
                tiplocs.contains(&"CARLILE"),
                "a genuinely public (Activity `T`) intermediate stop must be published: {tiplocs:?}"
            );
            assert_eq!(
                rows.len(),
                3,
                "Origin, the public LI, and Terminate all survive"
            );
        }

        #[test]
        fn the_terminate_stop_is_always_kept_even_though_tf_is_not_a_pickup_code() {
            // `TF` ("train finishes") is real `LT_EUSTON`'s own Activity
            // code and is NOT one of `is_public_pickup`'s pickup codes
            // (`T`/`TB`/`U`/`R`) -- this is the exact case the Terminate
            // carve-out in `schedule_calling_points_full_rows`'s own doc
            // comment exists for. If this regressed, every schedule's own
            // destination would silently vanish from this feed.
            let text = format!("{BS_C00573_PERMANENT}\n{LO_EUSTON}\n{LT_EUSTON}");
            let index = schedule_query::ScheduleIndex::from_text(&text);

            let rows = schedule_calling_points_full_rows(&index, service_date());

            let kinds: Vec<&str> = rows.iter().map(|r| r["kind"].as_str().unwrap()).collect();
            assert_eq!(
                kinds,
                vec!["origin", "terminate"],
                "the Terminate row must survive despite TF not being a pickup code"
            );
        }

        #[test]
        fn seq_is_renumbered_contiguously_over_the_filtered_sequence() {
            let text = format!(
                "{BS_C00573_PERMANENT}\n{LO_EUSTON}\n{LI_CARLILE_SET_DOWN_ONLY}\n{LT_EUSTON}"
            );
            let index = schedule_query::ScheduleIndex::from_text(&text);

            let rows = schedule_calling_points_full_rows(&index, service_date());

            let seqs: Vec<i64> = rows.iter().map(|r| r["seq"].as_i64().unwrap()).collect();
            assert_eq!(
                seqs,
                vec![0, 1],
                "seq must stay contiguous over the surviving rows, not carry a gap for the \
                 dropped LI"
            );
        }
    }

    /// Points EVERY one of this `Config`'s `*_URL` fields at `base`, on the
    /// real route paths, so a `poll_once` test can mount per-route mocks and
    /// see exactly which products published and which did not. `lines` is
    /// empty on purpose: `publish_schedule_line_population` then has nothing
    /// to publish, keeping these tests about the six whole-network products.
    pub(super) fn test_config_for_server(base: &str) -> Config {
        Config {
            storage_dir: std::path::PathBuf::from("/tmp/schedule-reference-test-does-not-exist"),
            poll_interval_secs: 1800,
            api_ingest_url: format!("{base}/private/stanox-crs"),
            schedule_line_population_url: format!("{base}/private/schedule-line-population"),
            schedule_network_departures_url: format!("{base}/private/schedule-network-departures"),
            schedule_destination_departures_url: format!(
                "{base}/private/schedule-destination-departures"
            ),
            fixed_links_url: format!("{base}/private/fixed-links"),
            schedule_calling_points_full_url: format!(
                "{base}/private/schedule-calling-points-full"
            ),
            tiploc_crs_url: format!("{base}/private/tiploc-crs"),
            schedule_reference_publishes_url: format!(
                "{base}/private/schedule-reference-publishes"
            ),
            lines: common::config::LineCatalogue(vec![]),
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: format!("{base}/token/"),
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
    /// `/token/` path, same fake-JWT response shape), so the real
    /// `common::ingest::get_json`/`post_batch` calls under test succeed their
    /// bearer-token fetch before hitting whichever per-route mock each test
    /// mounts separately.
    pub(super) async fn mock_token_cache(
        server: &wiremock::MockServer,
    ) -> common::oauth_client::OAuthTokenCache {
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/token/"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "fake-jwt",
                    "expires_in": 300,
                })),
            )
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

    /// The seeding half of the 2026-09-25 restart-dedup fix: after a restart,
    /// this service must seed its dedup state from ITS OWN completion marker,
    /// so it neither redundantly republishes a delivery it already finished
    /// nor skips one it never finished.
    #[tokio::test]
    async fn seed_last_processed_delivery_seeds_from_its_own_completion_marker() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/private/schedule-reference-publishes",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "delivery": "20260903T172830Z"
                })),
            )
            .mount(&server)
            .await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config_for_server(&server.uri());

        let seeded = seed_last_processed_delivery(&client, &config, &tokens).await;

        assert_eq!(
            seeded,
            Some("20260903T172830Z".to_string()),
            "must seed the exact dir_name a real discovery::latest_complete_delivery scan finds"
        );
    }

    /// **The regression test for finding #1 (High).** The dedup marker must
    /// come from `schedule-reference`'s own completion record, NOT from
    /// `schedule-ingest`'s extraction record.
    ///
    /// The shape reproduced here is the exact production one: `schedule-ingest`
    /// HAS recorded a delivery (so `/private/schedule-feed-ingests` would
    /// happily answer with it, and is mounted here to prove it is not read),
    /// but `schedule-reference` never finished publishing it -- it was
    /// restarted mid-cycle. Seeding from the ingest record made `poll_once`
    /// short-circuit on "no new delivery" and silently skip every product for
    /// that delivery until the next one landed ~24 hours later. Seeding from
    /// its own (absent) completion marker must instead fall back to first-run
    /// behavior, i.e. process the delivery.
    #[tokio::test]
    async fn seed_last_processed_delivery_does_not_seed_from_schedule_ingests_extraction_record() {
        let server = wiremock::MockServer::start().await;
        // schedule-ingest DID extract and record a delivery.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/private/schedule-feed-ingests"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "fetchedAt": "2026-09-03T17:28:30Z"
                })),
            )
            .mount(&server)
            .await;
        // schedule-reference never completed a publish cycle for it.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/private/schedule-reference-publishes",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "delivery": null })),
            )
            .mount(&server)
            .await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config_for_server(&server.uri());

        let seeded = seed_last_processed_delivery(&client, &config, &tokens).await;

        assert_eq!(
            seeded, None,
            "a delivery schedule-ingest extracted but schedule-reference never published must NOT \
             be treated as already processed -- that is the bug that silently loses a whole day of \
             every published product"
        );
        let ingest_reads = server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .into_iter()
            .filter(|req| req.url.path() == "/private/schedule-feed-ingests")
            .count();
        assert_eq!(
            ingest_reads, 0,
            "schedule-reference must not read schedule-ingest's extraction record at all any more"
        );
    }

    /// A genuinely fresh deployment (an empty `schedule_reference_publishes`
    /// table, `delivery: null`) is a real, valid case -- not an error -- and
    /// must fall back to this service's pre-existing first-run behavior.
    #[tokio::test]
    async fn seed_last_processed_delivery_falls_back_to_none_when_no_prior_record_exists() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/private/schedule-reference-publishes",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "delivery": null })),
            )
            .mount(&server)
            .await;
        let tokens = mock_token_cache(&server).await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let config = test_config_for_server(&server.uri());

        assert_eq!(
            seed_last_processed_delivery(&client, &config, &tokens).await,
            None,
            "an empty schedule_reference_publishes table must fall back to None/first-run behavior"
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
        let config = test_config_for_server(&server.uri());

        assert_eq!(
            seed_last_processed_delivery(&client, &config, &tokens).await,
            None
        );
    }
}

/// End-to-end `poll_once` coverage for finding #2 (High): six of this
/// service's seven publishes logged `"will retry next cycle"` and never did,
/// because `last_processed_delivery` advanced after the FIRST one.
///
/// These drive the real `poll_once` against a real delivery directory on disk
/// and a mock `api`, because that is the only place the bug was observable:
/// every individual publish function already "worked", and what was broken was
/// the sequencing between them.
#[cfg(test)]
mod poll_once_retry_tests {
    use super::*;

    /// One real, minimal delivery directory of the exact shape
    /// `schedule-ingest` produces and `discovery::latest_complete_delivery`
    /// accepts: a timestamp-named directory holding an `RJTTF<n>MCA.txt` and
    /// an `RJTTF<n>MSN.txt`.
    ///
    /// The MCA carries a byte-verbatim real `TI` record (so the STANOX/CRS and
    /// TIPLOC/CRS publishes have something real to resolve) plus one
    /// `BS`/`LO`/`LT` schedule block whose date range is generated around
    /// `today` so the CIF-derived products resolve it for the dates this
    /// service actually publishes. The `BS` line is the byte-verbatim real
    /// `C00573` record from `schedule_query::parse`'s own fixtures with only
    /// its UID, date range and days-run field replaced.
    fn write_fixture_delivery(root: &std::path::Path, dir_name: &str) {
        const TI_EUSTON: &str =
            "TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           ";
        const BS_REAL: &str =
            "BSNC005732605172612060000001 PXX1S003101121194800 DMU    125      S A T        P";
        const LO_EUSTON: &str = "LOEUSTON  0822 08227  C      TB";
        const LT_EUSTON: &str = "LTEUSTON  0904 09079     TF";
        const A_WATRLMN: &str = "A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815";

        let today = london_local_date_now();
        let from = (today - chrono::Duration::days(1))
            .format("%y%m%d")
            .to_string();
        // Wider than the publish window (`DESTINATION_DEPARTURES_FORWARD_DAYS`)
        // so every date this cycle publishes resolves the schedule, not just
        // the first.
        let to = (today + chrono::Duration::days(30))
            .format("%y%m%d")
            .to_string();
        let bs = format!("BSNT00001{from}{to}1111111{}", &BS_REAL[28..]);

        let dir = root.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("RJTTF942MCA.txt"),
            format!("{TI_EUSTON}\n{bs}\n{LO_EUSTON}\n{LT_EUSTON}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("RJTTF942MSN.txt"), format!("{A_WATRLMN}\n")).unwrap();
    }

    /// Mounts a 200-answering POST mock for every publish route, then lets the
    /// caller override individual routes by mounting a mock FIRST (wiremock
    /// matches in mount order, so anything mounted before this call wins).
    async fn mount_all_publishes_ok(server: &wiremock::MockServer) {
        for path in [
            "/private/stanox-crs",
            "/private/tiploc-crs",
            "/private/schedule-line-population",
            "/private/schedule-network-departures",
            "/private/schedule-destination-departures",
            "/private/schedule-calling-points-full",
            "/private/schedule-reference-publishes",
        ] {
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path(path))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "upserted": 1 })),
                )
                .mount(server)
                .await;
        }
    }

    async fn posts_to(server: &wiremock::MockServer, path: &str) -> usize {
        server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .into_iter()
            .filter(|req| req.url.path() == path)
            .count()
    }

    /// **The regression test for finding #2.** The LAST of the seven publishes
    /// fails; the delivery must NOT be recorded as processed, so the next
    /// cycle retries it -- which is exactly what every one of those publishes'
    /// own log lines has always claimed and, before this fix, never did.
    #[tokio::test]
    async fn a_failing_late_publish_leaves_the_delivery_unprocessed_so_the_next_cycle_retries_it() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        // Mounted BEFORE the catch-all 200s, so this route's 500 wins.
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(
                "/private/schedule-calling-points-full",
            ))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;
        mount_all_publishes_ok(&server).await;

        let storage = tempfile::tempdir().unwrap();
        write_fixture_delivery(storage.path(), "20260925T180000Z");
        let mut config = super::poll_once_tests::test_config_for_server(&server.uri());
        config.storage_dir = storage.path().to_path_buf();
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();

        let mut last_processed_delivery: Option<String> = None;
        poll_once(&client, &config, &mut last_processed_delivery, &tokens)
            .await
            .expect("a failed publish is logged and accumulated, never a hard cycle error");

        assert_eq!(
            last_processed_delivery, None,
            "one failed publish must leave the dedup marker untouched, so the NEXT cycle \
             reprocesses and republishes the whole delivery"
        );
        assert_eq!(
            posts_to(&server, "/private/schedule-reference-publishes").await,
            0,
            "a cycle that did not fully publish must not write the durable completion marker"
        );
        // The products AFTER the failing one in the cycle must still have been
        // attempted this cycle -- a failure is log-and-continue, not abort.
        assert!(
            posts_to(&server, "/private/schedule-destination-departures").await > 0,
            "the sibling product published in the same per-date loop must still be attempted"
        );
    }

    /// The mirror image: every publish succeeds, so the delivery IS recorded
    /// as processed both in memory and durably, and a second `poll_once` with
    /// no new delivery does nothing.
    #[tokio::test]
    async fn a_fully_successful_cycle_advances_the_marker_and_records_the_durable_completion() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        mount_all_publishes_ok(&server).await;

        let storage = tempfile::tempdir().unwrap();
        write_fixture_delivery(storage.path(), "20260925T180000Z");
        let mut config = super::poll_once_tests::test_config_for_server(&server.uri());
        config.storage_dir = storage.path().to_path_buf();
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();

        let mut last_processed_delivery: Option<String> = None;
        poll_once(&client, &config, &mut last_processed_delivery, &tokens)
            .await
            .expect("cycle");

        assert_eq!(
            last_processed_delivery,
            Some("20260925T180000Z".to_string()),
            "a fully successful cycle must advance the dedup marker"
        );
        assert_eq!(
            posts_to(&server, "/private/schedule-reference-publishes").await,
            1,
            "a fully successful cycle must write exactly one durable completion marker"
        );

        let stanox_posts_after_first_cycle = posts_to(&server, "/private/stanox-crs").await;
        poll_once(&client, &config, &mut last_processed_delivery, &tokens)
            .await
            .expect("second cycle");
        assert_eq!(
            posts_to(&server, "/private/stanox-crs").await,
            stanox_posts_after_first_cycle,
            "a second cycle with no new delivery must publish nothing at all"
        );
    }

    /// A publish failure that reprocessing the SAME delivery cannot fix (this
    /// delivery's ALF member parses to zero fixed links) must NOT hold the
    /// delivery back -- otherwise this service would re-parse a 700MB+ file
    /// and rebuild millions of rows every `poll_interval_secs`, forever, to
    /// reach the same conclusion. It must also not write the durable
    /// completion marker, since the cycle genuinely did not fully publish.
    #[tokio::test]
    async fn a_permanently_failing_product_does_not_trap_the_delivery_in_a_retry_loop() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        mount_all_publishes_ok(&server).await;

        let storage = tempfile::tempdir().unwrap();
        write_fixture_delivery(storage.path(), "20260925T180000Z");
        // A present-but-unparseable ALF member: reads fine, parses to zero
        // links. `publish_fixed_links` refuses to publish an empty batch
        // (it would wipe the table) and records a PERMANENT failure.
        std::fs::write(
            storage.path().join("20260925T180000Z/RJTTF942ALF.txt"),
            "not an ALF record at all\n",
        )
        .unwrap();
        let mut config = super::poll_once_tests::test_config_for_server(&server.uri());
        config.storage_dir = storage.path().to_path_buf();
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();

        let mut last_processed_delivery: Option<String> = None;
        poll_once(&client, &config, &mut last_processed_delivery, &tokens)
            .await
            .expect("cycle");

        assert_eq!(
            last_processed_delivery,
            Some("20260925T180000Z".to_string()),
            "a failure reprocessing cannot fix must not hold the delivery back"
        );
        assert_eq!(
            posts_to(&server, "/private/fixed-links").await,
            0,
            "an ALF that parses to zero links must not be published (it would wipe the table)"
        );
        assert_eq!(
            posts_to(&server, "/private/schedule-reference-publishes").await,
            0,
            "a cycle with any failed product must not claim a complete publish in api's durable \
             record -- a restart should still re-attempt this delivery"
        );
    }
}

/// Coverage for finding #3 (High): the largest published product
/// (`schedule_calling_points_full`) was sent as one POST per date, plausibly
/// at or over `api`'s 100MB body limit and this crate's 30s
/// `REQUEST_TIMEOUT`, eight times per cycle -- and the `rows.chunks(50_000)`
/// fallback its own doc comment described was never implemented.
///
/// The tests that matter here are the ones about the CONTRACT, not the split:
/// chunking a wholesale-replace publish is only safe if exactly one chunk per
/// date is allowed to clear the date.
#[cfg(test)]
mod chunked_publish_tests {
    use super::*;

    fn rows(count: usize) -> Vec<serde_json::Value> {
        (0..count)
            .map(|i| serde_json::json!({ "service_date": "2026-09-25", "seq": i }))
            .collect()
    }

    async fn capture_posts(server: &wiremock::MockServer, path: &str) -> Vec<(usize, String)> {
        server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .into_iter()
            .filter(|req| req.url.path() == path)
            .map(|req| {
                let query = req.url.query().unwrap_or_default().to_string();
                let body: Vec<serde_json::Value> =
                    serde_json::from_slice(&req.body).expect("body is a JSON array");
                (body.len(), query)
            })
            .collect()
    }

    #[test]
    fn first_chunk_url_marks_only_the_chunk_that_may_clear_the_date() {
        assert_eq!(
            first_chunk_url("http://api:8080/private/schedule-calling-points-full", true),
            "http://api:8080/private/schedule-calling-points-full?first_chunk=true"
        );
        assert_eq!(
            first_chunk_url(
                "http://api:8080/private/schedule-calling-points-full",
                false
            ),
            "http://api:8080/private/schedule-calling-points-full?first_chunk=false"
        );
    }

    #[test]
    fn first_chunk_url_appends_to_a_url_that_already_has_a_query_string() {
        assert_eq!(
            first_chunk_url("http://api:8080/private/x?trace=1", true),
            "http://api:8080/private/x?trace=1&first_chunk=true"
        );
    }

    /// **The core contract test.** Several chunks per date, and exactly ONE of
    /// them -- the first -- carries `first_chunk=true`. If every chunk carried it,
    /// each chunk's `DELETE ... WHERE service_date = ANY(...)` would delete
    /// the chunks before it and the date would end up holding only the last
    /// chunk: a far worse bug than the oversized body this chunking prevents.
    #[tokio::test]
    async fn only_the_first_chunk_of_a_date_is_allowed_to_clear_that_date() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chunked"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let url = format!("{}/private/chunked", server.uri());

        post_date_scoped_rows_in_chunks(
            &client,
            &url,
            &tokens,
            &rows(PUBLISH_CHUNK_ROWS * 2 + 1),
            "test rows",
        )
        .await
        .expect("all chunks accepted");

        let posts = capture_posts(&server, "/private/chunked").await;
        assert_eq!(posts.len(), 3, "2*chunk+1 rows must be split into 3 POSTs");
        assert_eq!(
            posts[0],
            (PUBLISH_CHUNK_ROWS, "first_chunk=true".to_string())
        );
        assert_eq!(
            posts[1],
            (PUBLISH_CHUNK_ROWS, "first_chunk=false".to_string())
        );
        assert_eq!(posts[2], (1, "first_chunk=false".to_string()));
    }

    /// A publish small enough to fit in one chunk must be byte-for-byte the
    /// same single wholesale-replace POST it always was -- chunking must not
    /// change the common case.
    #[tokio::test]
    async fn a_publish_that_fits_in_one_chunk_is_still_exactly_one_replacing_post() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chunked"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let url = format!("{}/private/chunked", server.uri());

        post_date_scoped_rows_in_chunks(&client, &url, &tokens, &rows(10), "test rows")
            .await
            .expect("accepted");

        assert_eq!(
            capture_posts(&server, "/private/chunked").await,
            vec![(10, "first_chunk=true".to_string())]
        );
    }

    /// An empty publish is still exactly one POST -- the receiving route
    /// treats an empty batch as a deliberate no-op that must NOT delete the
    /// date, and sending it keeps the `posted 0 <noun>` log line that is the
    /// only evidence the publish ran and had nothing to say.
    #[tokio::test]
    async fn an_empty_publish_is_one_post_and_never_silently_skipped() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chunked"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let url = format!("{}/private/chunked", server.uri());

        post_date_scoped_rows_in_chunks(&client, &url, &tokens, &[], "test rows")
            .await
            .expect("accepted");

        assert_eq!(
            capture_posts(&server, "/private/chunked").await,
            vec![(0, "first_chunk=true".to_string())]
        );
    }

    /// A failing chunk must surface as an error naming WHICH chunk failed, so
    /// the caller records a retryable failure and the next cycle republishes
    /// the whole date from a `first_chunk=true` chunk.
    #[tokio::test]
    async fn a_failing_chunk_is_an_error_that_names_the_chunk() {
        let server = wiremock::MockServer::start().await;
        let tokens = super::poll_once_tests::mock_token_cache(&server).await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/private/chunked"))
            .respond_with(wiremock::ResponseTemplate::new(413))
            .mount(&server)
            .await;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let url = format!("{}/private/chunked", server.uri());

        let err = post_date_scoped_rows_in_chunks(&client, &url, &tokens, &rows(5), "test rows")
            .await
            .expect_err("a 413 must not be swallowed");
        let message = format!("{err}");
        assert!(
            message.contains("chunk 1/1") && message.contains("first_chunk=true"),
            "error must identify the chunk and its first_chunk flag; got: {message}"
        );
    }
}

/// Coverage for finding #4 (Medium): `publish_cif_derived_products` computed
/// "today" in UTC while its `schedule-ingest` sibling has always used
/// London-local time for the equivalent decision, so during the 00:00-01:00
/// BST window the two containers disagreed about which rail day a delivery
/// belonged to and published data landed under the wrong date.
#[cfg(test)]
mod london_local_date_tests {
    use super::*;

    #[test]
    fn a_summer_instant_just_after_london_midnight_is_already_the_next_london_date() {
        // 23:30 UTC on 1 July is 00:30 BST on 2 July -- the exact hour the bug
        // lived in. UTC says the 1st; London (and `schedule-ingest`) say the
        // 2nd, and the 2nd is the rail day the data belongs to.
        let instant: chrono::DateTime<chrono::Utc> = "2026-07-01T23:30:00Z".parse().unwrap();
        assert_eq!(
            london_local_date_at(instant),
            chrono::NaiveDate::from_ymd_opt(2026, 7, 2).unwrap()
        );
        assert_ne!(
            london_local_date_at(instant),
            instant.date_naive(),
            "this is the hour the old UTC computation got wrong; if these are equal the fixture \
             no longer exercises the bug"
        );
    }

    #[test]
    fn a_winter_instant_matches_utc_because_london_is_utc_then() {
        let instant: chrono::DateTime<chrono::Utc> = "2026-01-01T23:30:00Z".parse().unwrap();
        assert_eq!(
            london_local_date_at(instant),
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
        assert_eq!(london_local_date_at(instant), instant.date_naive());
    }

    #[test]
    fn a_summer_instant_in_the_middle_of_the_day_is_unaffected() {
        let instant: chrono::DateTime<chrono::Utc> = "2026-07-01T12:00:00Z".parse().unwrap();
        assert_eq!(
            london_local_date_at(instant),
            chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
        );
    }
}
