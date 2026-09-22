//! Station-skip detection for a journey leg's own origin/destination,
//! notifier-side -- §5.2 of docs/superpowers/specs/2026-09-22-journey-tracking-design.md.
//! Cannot import `crates/api/src/data/station_skip.rs` (this crate does
//! not depend on `crates/api` -- see this plan's Architecture section for
//! why), so this async wrapper is written fresh against this crate's own
//! `queries.rs`; the actual matching/skip-membership logic is still shared
//! via `common::match_darwin_departure`/`departure_skips_station` so the
//! two independent implementations (this one, and
//! `crates/api/src/data/station_skip.rs::find_leg_skip`) can never define
//! "skipped" two different ways.

use common::{departure_skips_station, match_darwin_departure};
use sqlx::PgPool;

use crate::queries::{self, CommittedLeg};

/// Whether either end of `leg`'s own travel intent is among today's
/// skipped calling points, per the live Darwin sample(s) currently on
/// file. `Ok(false)` (never an error) when there's simply no sample yet to
/// check against -- a genuine "don't know" degrades to "assume not
/// skipped," matching `station_skip::leg_skip_status`'s own best-effort
/// posture on the API side. A real DB connectivity failure still
/// propagates via `?`, same as every other query in this crate -- the
/// caller (`run_skip_check_cycle`) lets that fail the whole cycle, retried
/// next interval, same as `run_cycle`/`run_forward_queue_cycle` already do
/// for their own DB errors.
pub async fn leg_is_skipped(pool: &PgPool, leg: &CommittedLeg) -> anyhow::Result<bool> {
    let match_target = leg
        .pin_destination_crs
        .as_deref()
        .or(leg.next_calling_point.as_deref());

    let Some(origin_sample) = queries::station_sample_for_crs(pool, &leg.origin_crs).await? else {
        return Ok(false);
    };
    let destination_skipped = match_darwin_departure(&origin_sample.departures, match_target)
        .is_some_and(|matched| departure_skips_station(matched, &leg.destination_crs));

    let origin_skipped = match leg.train_origin_crs.as_deref() {
        Some(train_origin) if !train_origin.eq_ignore_ascii_case(&leg.origin_crs) => {
            match queries::station_sample_for_crs(pool, train_origin).await? {
                Some(sample) => match_darwin_departure(&sample.departures, match_target)
                    .is_some_and(|matched| departure_skips_station(matched, &leg.origin_crs)),
                None => false,
            }
        }
        _ => false,
    };

    Ok(destination_skipped || origin_skipped)
}
