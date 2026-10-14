//! The timetable loop (transit spec §6.1-6.3): download the network's
//! GTFS to the work dir, parse it on the blocking pool, and (plan task
//! 2.3) apply `NaPTAN` names and the rail links, check it against the last
//! commit and produce it.

use chrono::Utc;

use crate::Context;
use crate::adapter::gtfs::{self, GtfsOptions};
use crate::adapter::{FeedError, download_to_file};
use crate::model::TransitModel;

/// `transit_feed_refused_total{network, reason}`.
pub(crate) const FEED_REFUSED: &str = "transit_feed_refused_total";

/// Downloads and parses the network's GTFS.
pub(crate) async fn fetch_model(ctx: &Context) -> Result<TransitModel, FeedError> {
    let path = ctx.config.work_dir.join("feed.zip");
    let bytes = download_to_file(
        &ctx.client,
        &ctx.config.gtfs_url,
        &path,
        ctx.config.gtfs_max_bytes,
    )
    .await?;
    let today = common::transit::service_date(Utc::now());
    let options = GtfsOptions {
        today,
        horizon_days: ctx.config.horizon_days,
        max_entry_bytes: gtfs::MAX_ENTRY_BYTES,
    };
    let network = ctx.network.clone();
    let parse_path = path.clone();
    let started = std::time::Instant::now();
    let parsed = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&parse_path)
            .map_err(|err| FeedError::Io(format!("opening {}: {err}", parse_path.display())))?;
        gtfs::parse(std::io::BufReader::new(file), &network, &options)
    })
    .await
    .map_err(|err| FeedError::Io(format!("the parse task failed: {err}")))?;
    if let Err(err) = tokio::fs::remove_file(&path).await {
        tracing::warn!(error = %err, path = %path.display(), "could not delete the downloaded feed");
    }
    let model = parsed?;
    tracing::info!(
        network = %ctx.network.id,
        bytes,
        seconds = started.elapsed().as_secs_f64(),
        lines = model.lines.len(),
        stops = model.stops.len(),
        patterns = model.patterns.len(),
        trips = model.trips.len(),
        service_days = model.service_day_count(),
        "parsed the timetable"
    );
    Ok(model)
}

/// One timetable cycle. Plan task 2.3 adds the enrichment, the feed guard
/// and the produce; until then a cycle parses and reports.
pub(crate) async fn poll_once(ctx: &Context) -> anyhow::Result<()> {
    match fetch_model(ctx).await {
        Ok(_) => anyhow::bail!(
            "network {}: the timetable parsed, but producing it lands in transit plan task 2.3",
            ctx.network.id
        ),
        Err(err) => {
            metrics::counter!(
                common::metrics::metric_name(FEED_REFUSED),
                "network" => ctx.network.id.clone(),
                "reason" => err.reason()
            )
            .increment(1);
            Err(err.into())
        }
    }
}
