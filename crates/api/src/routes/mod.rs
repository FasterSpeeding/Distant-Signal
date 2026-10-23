use crate::app::Router;

pub mod account;
pub mod admin;
pub mod auth;
pub mod connected_apps;
#[cfg(test)]
#[expect(
    clippy::too_many_lines,
    reason = "test code: scenario tests read top to bottom"
)]
mod delegated_db_tests;
pub mod departures;
pub mod freshness;
pub mod groups;
pub mod health;
pub mod history_retention;
pub mod incidents;
pub mod island_of_ireland;
#[cfg(test)]
#[expect(
    clippy::too_many_lines,
    reason = "test code: scenario tests read top to bottom"
)]
mod journey_follows_db_tests;
pub mod journey_itinerary;
pub mod journey_replan;
pub mod journey_templates;
pub mod journeys;
#[cfg(test)]
mod leg_validation_db_tests;
pub mod line_status;
pub mod line_timetable;
pub mod line_trains_summary;
pub mod lines;
pub mod notifications;
pub mod operator_history;
pub mod operators;
pub mod preferences;
pub mod provisional;
pub mod reference;
pub mod schedule_rows;
pub mod stanox_crs;
pub mod station_stats;
pub mod tfl_stop_page;
pub mod tfl_stops;
pub mod train;
pub mod trains;
pub mod transit;
pub mod trips;

/// The current instant on the Europe/London wall clock.
///
/// Every rail-day-keyed table this API reads (`schedule_network_departures`,
/// `schedule_line_population`, `schedule_destination_departures`, ...) is
/// keyed by London service date, never UTC: for an hour every night (all of
/// 23:00-00:00 UTC during BST) `Utc::now().date_naive()` is still London's
/// yesterday. Routes -- and the db tests that seed "today" for them -- must
/// all derive "today"/"now" from this one function so they can never
/// disagree about which day it is.
///
/// Test builds can pin it per thread with [`pin_london_now_for_tests`], so a
/// db test no longer depends on the wall clock (DB review 2026-09-27 B4).
pub(crate) fn london_now() -> chrono::DateTime<chrono_tz::Tz> {
    #[cfg(test)]
    if let Some(pinned) = PINNED_NOW.with(std::cell::Cell::get) {
        return pinned.with_timezone(&chrono_tz::Europe::London);
    }
    chrono::Utc::now().with_timezone(&chrono_tz::Europe::London)
}

#[cfg(test)]
thread_local! {
    static PINNED_NOW: std::cell::Cell<Option<chrono::DateTime<chrono::Utc>>> =
        const { std::cell::Cell::new(None) };
}

/// Pins [`london_now`] (and so [`london_today`]) to `instant` on the current
/// thread until the returned guard is dropped. A `#[tokio::test]` uses the
/// current-thread runtime, so the handler under test (driven through
/// `oneshot`) runs on the same thread and sees the pinned value; a
/// `multi_thread` test would not.
#[cfg(test)]
#[must_use = "the pin is lifted as soon as the guard is dropped"]
pub(crate) fn pin_london_now_for_tests(instant: chrono::DateTime<chrono::Utc>) -> PinnedLondonNow {
    PINNED_NOW.with(|cell| cell.set(Some(instant)));
    PinnedLondonNow(())
}

/// Guard returned by [`pin_london_now_for_tests`].
#[cfg(test)]
pub(crate) struct PinnedLondonNow(());

#[cfg(test)]
impl Drop for PinnedLondonNow {
    fn drop(&mut self) {
        PINNED_NOW.with(|cell| cell.set(None));
    }
}

/// London-local "today" -- see [`london_now`].
pub(crate) fn london_today() -> chrono::NaiveDate {
    london_now().date_naive()
}

// Moved to `ds_store::validate` (ingest architecture plan 1A.2): the
// ingest half of `train_tracking` needs them too.
pub(crate) use ds_store::validate::{
    is_ascii_code, is_crs_code, validate_code_list, validate_short_text,
};

pub fn public_router() -> Router {
    // `health::router()` already declares its own `/health` route, so this
    // must `merge` (mount directly under `/public`) rather than `nest`
    // another `/health` prefix on top of it — nesting here previously
    // produced `/public/health/health` instead of the intended
    // `/public/health`, discovered while wiring up the docker-compose
    // healthcheck in Task 6's end-to-end verification.
    //
    // `line_status::router()` is deliberately NOT merged in here. This
    // function's output is always nested under `/public` in `main.rs`
    // (load-bearing: `docker-compose.yml`'s healthcheck hits
    // `/public/health` and `crates/api/Dockerfile`'s HEALTHCHECK comment
    // says the same), but the four line-status endpoints must be
    // reachable at the unprefixed paths DESIGN.md specifies
    // (`GET /Line/Mode/national-rail/Status`, `GET /StopPoint/{crs}/Disruption`,
    // etc.) so that clients already built against TfL's own API work
    // unchanged — that's the entire point of mimicking TfL's response
    // shape. Nesting them under `/public` like `health` would silently
    // break that compatibility. `main.rs` merges `line_status::router()`
    // directly onto the top-level router instead; it's still
    // unauthenticated, just not routed through this particular function.
    Router::new()
        .merge(health::router())
        .merge(freshness::router())
        .merge(history_retention::router())
        .merge(incidents::router())
        .merge(lines::router())
        .merge(notifications::router())
        .merge(operator_history::router())
        .merge(operators::router())
        .merge(preferences::router())
        .merge(reference::router())
        .merge(island_of_ireland::router())
        .merge(auth::router())
        .merge(admin::router())
        .merge(account::router())
        .merge(connected_apps::router())
        .merge(groups::router())
        .merge(station_stats::router())
        .merge(departures::router())
        .merge(stanox_crs::router())
        .merge(trains::router())
        .merge(transit::router())
        .merge(tfl_stops::router())
        .merge(tfl_stop_page::router())
}
