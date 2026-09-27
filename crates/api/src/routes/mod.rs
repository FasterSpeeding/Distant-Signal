use axum::extract::DefaultBodyLimit;
use axum::middleware;

use crate::app::{App, Router};
use crate::auth::require_internal_oauth;

pub mod account;
pub mod admin;
pub mod auth;
pub mod chatbot;
pub mod departures;
pub mod freshness;
pub mod groups;
pub mod health;
pub mod history_retention;
pub mod incidents;
pub mod ingest;
pub mod island_of_ireland;
pub mod journey_templates;
pub mod journeys;
pub mod line_status;
pub mod lines;
pub mod notifications;
pub mod operator_history;
pub mod operators;
pub mod preferences;
pub mod reference;
pub mod samples;
pub mod stanox_crs;
pub mod station_stats;
pub mod train;
pub mod trains;
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
pub(crate) fn london_now() -> chrono::DateTime<chrono_tz::Tz> {
    chrono::Utc::now().with_timezone(&chrono_tz::Europe::London)
}

/// London-local "today" -- see [`london_now`].
pub(crate) fn london_today() -> chrono::NaiveDate {
    london_now().date_naive()
}

/// The one shared bound for short free-text fields on authenticated writes
/// (API-7). Without it a logged-in user could park multi-megabyte strings
/// in TEXT columns (the default body limit is 2MB, 8MB on the train
/// router). Counts characters, not bytes, like `validate_custom_name`, and
/// rejects control characters, which no short display field needs.
///
/// `label` is user-facing copy (these 400 bodies are rendered verbatim by
/// the frontend), so it is plain words such as "The operator", never a
/// field name.
pub(crate) fn validate_short_text(
    label: &str,
    value: &str,
    max_chars: usize,
) -> Result<(), String> {
    if value.chars().count() > max_chars {
        return Err(format!(
            "{label} is too long. Keep it to {max_chars} characters or fewer."
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} contains characters that aren't allowed."));
    }
    Ok(())
}

/// [`validate_short_text`] for a list field: at most `max_items` entries,
/// and every entry must pass `is_valid`. `describe` is the user-facing
/// shape, for example "three-letter station codes".
pub(crate) fn validate_code_list(
    label: &str,
    values: &[String],
    max_items: usize,
    describe: &str,
    is_valid: impl Fn(&str) -> bool,
) -> Result<(), String> {
    if values.len() > max_items {
        return Err(format!("{label} can have at most {max_items} entries."));
    }
    if let Some(bad) = values.iter().find(|v| !is_valid(v)) {
        // Bounded before echoing, so an oversized entry never comes back
        // whole in the error body.
        let shown: String = bad.chars().take(16).filter(|c| !c.is_control()).collect();
        return Err(format!("{label} must be {describe}; '{shown}' isn't one."));
    }
    Ok(())
}

/// `len` ASCII letters or digits, case-insensitive. The shape of an ATOC
/// operator code (2) and of a CRS code when `letters_only`.
pub(crate) fn is_ascii_code(
    value: &str,
    min_len: usize,
    max_len: usize,
    letters_only: bool,
) -> bool {
    let n = value.len();
    (min_len..=max_len).contains(&n)
        && value.bytes().all(|b| {
            if letters_only {
                b.is_ascii_alphabetic()
            } else {
                b.is_ascii_alphanumeric()
            }
        })
}

/// A three-letter CRS code (untrimmed; callers trim first if they accept
/// padding).
pub(crate) fn is_crs_code(value: &str) -> bool {
    is_ascii_code(value, 3, 3, true)
}

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
    // unauthenticated (no `require_internal_oauth` layer applied), just
    // not routed through this particular function.
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
        .merge(groups::router())
        .merge(chatbot::router())
        .merge(station_stats::router())
        .merge(departures::router())
        .merge(stanox_crs::router())
        .merge(trains::router())
}

/// Takes the app state directly (rather than picking it up later via
/// `Router::with_state`) because the internal-auth layer needs a concrete
/// token value at the point it's constructed: `axum::middleware::from_fn`
/// fixes its handler's state to `()`, so a stateful check has to go through
/// `from_fn_with_state`, which takes the state by value up front.
pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .merge(samples::router())
        .layer(middleware::from_fn_with_state(app, require_internal_oauth))
        // Axum's `Json` extractor enforces an implicit 2MB body-read limit
        // unless overridden. `StationReference::accessibility` (see
        // crates/common) is a `#[serde(flatten)]` passthrough that carries
        // *every* unmodeled per-station field from the RDM feed verbatim
        // (carParks, ticketBuying, lifts, transportLinks, address, ...),
        // not just accessibility data — so the full ~2,600-station feed
        // measures ~55MB raw, which is what actually surfaced as a 413 on
        // poller-stations' ingest POST (a prior fix here that assumed
        // ~20MB was itself too low; verified directly against the live RDM
        // feed rather than guessed). 100MB leaves ~2x headroom over
        // today's measured size for feed growth.
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_short_text_counts_characters_and_rejects_control_characters() {
        assert!(validate_short_text("The name", "abc", 3).is_ok());
        assert!(validate_short_text("The name", "äöü", 3).is_ok());
        assert!(
            validate_short_text("The name", "abcd", 3)
                .unwrap_err()
                .contains("3 characters")
        );
        assert!(validate_short_text("The name", "a\nb", 10).is_err());
    }

    #[test]
    fn validate_code_list_bounds_count_and_shape_without_echoing_the_whole_value() {
        let ok = vec!["WAT".to_string(), "wok".to_string()];
        assert!(validate_code_list("Stops", &ok, 2, "codes", is_crs_code).is_ok());
        assert!(validate_code_list("Stops", &ok, 1, "codes", is_crs_code).is_err());
        let bad = vec!["W".repeat(10_000)];
        let err = validate_code_list("Stops", &bad, 5, "codes", is_crs_code).unwrap_err();
        assert!(err.len() < 100, "{err}");
    }

    #[test]
    fn is_ascii_code_checks_length_and_charset() {
        assert!(is_ascii_code("SW", 2, 2, false));
        assert!(is_ascii_code("1P", 1, 4, false));
        assert!(!is_ascii_code("1P", 1, 4, true));
        assert!(!is_ascii_code("S W", 2, 3, false));
        assert!(!is_ascii_code("ÄB", 2, 3, false));
        assert!(is_crs_code("wat"));
        assert!(!is_crs_code("WA1"));
    }
}
