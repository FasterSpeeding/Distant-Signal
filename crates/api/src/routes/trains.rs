//! `GET /public/trains/search` -- calling-point-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page.
//! Generalizes the earlier destination-first search
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md) into a
//! calling-point-first one -- see
//! docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly
//! (`queries::search_schedule_calling_point_departures`) as a bounded index
//! range scan (over `schedule_destination_departures_calling_point_idx`)
//! with a keyset cursor. This is a publish-then-poll read of a table
//! `schedule-reference` writes when a CIF delivery lands -- never a
//! synchronous call into that service.
//!
//! **v1 filter set, and why it stops here.** `station` (any calling
//! point -- boarding or alighting) is required; `origin` (the schedule's
//! TRUE first calling point) and `stops_at` (any calling point or points,
//! zero or more) are both optional, independent filters, along with the
//! `from`/`to` time range. `from`/`to` bound `station`'s own `scheduled`
//! time; `arrival_from`/`arrival_to` are a SEPARATE, independent `"HH:MM"`
//! bound pair on when the train ARRIVES at `stops_at`'s single named
//! calling point, and require `stops_at` to name EXACTLY ONE station (a
//! `400` otherwise -- see `TrainSearchParams::arrival_from`'s own doc
//! comment and
//! docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md).
//! There is deliberately NO operator filter: the
//! CIF SCHEDULE feed's operator field is parsed-but-undecoded everywhere in
//! this codebase, so a CIF-derived row has no operator to filter on at all.
//!
//! **`stops_at` replaced the earlier single-valued `destination` filter**
//! (the schedule's TRUE final calling point) -- see
//! docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md for
//! the full reasoning. This is a genuine, deliberate behavior change, not
//! a rename: `destination` meant "this IS the schedule's true final stop";
//! `stops_at` means "the schedule calls here at some point in its route",
//! true destination or not, and requires ALL named stations to match
//! (relational division), not just one. A caller who genuinely needs
//! "true destination equals X" (as opposed to "calls at X") has no
//! equivalent filter any more -- see that design doc's own open question.
//! `date` (`"YYYY-MM-DD"`, optional) selects which `service_date` this
//! search runs against, defaulting to today -- but only within a bounded
//! window (`SEARCH_WINDOW_BACKWARD_DAYS`/`SEARCH_WINDOW_FORWARD_DAYS`
//! below), not the whole published timetable: a date outside that window
//! is a `400`. See
//! docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md.
//!
//! **This route owns the `now`-forward DEFAULT**, which is the whole point
//! of the storage shape behind it. The publish stores the entire rail day
//! uncapped, because it fires once per CIF delivery -- roughly daily -- so
//! a publish-time filter would freeze at whatever the clock read when the
//! delivery landed. Evaluating `now` here means a bare search at 18:00
//! defaults to "what's coming up next" rather than the whole day. **This
//! default only applies when `date` resolves to today, and only when the
//! caller supplies no explicit `from` at all** -- browsing any other day in
//! the window has no "now" to be forward of, and an explicit `from` is
//! always honored exactly as given, even one that names a time already in
//! the past relative to `now`: the caller asked for that specific window on
//! purpose, so it is never silently re-floored to `now`.
//!
//! **Pagination is a keyset cursor, not an offset.** `limit` bounds one
//! page; `after` carries the last row of the previous page.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum_extra::extract::Query;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::CallingPointDepartureCursor;
use crate::render::calling_point_departure_json;

/// Page size when the caller does not ask for one.
const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected.
///
/// This is an unauthenticated, unmetered public route over a table with
/// ~377,000 rows per day. Without a ceiling, `?limit=1000000` is a free
/// full-table scan and a large response for any anonymous caller. 200 is
/// four default pages -- generous for any real client.
///
/// An over-large `limit` is clamped, not rejected -- the honest answer is
/// "here are 200, with a cursor for the rest." A `limit` that is zero,
/// negative or unparseable IS malformed and does 400.
///
/// That's an asymmetry with `from`, `to`, `stops_at`, `origin` and
/// `station`, which all 400 on a bad value instead of silently ignoring it:
/// clamping `limit` can only ever return FEWER rows than the caller asked
/// for, and it's still a safe, honest answer because there's a cursor for
/// the rest. Silently dropping a malformed filter like `stops_at` would
/// do the opposite -- it would return MORE rows than the caller asked for,
/// under a filter the caller thinks is still applied. That reads as a
/// broken search, not a rejected input, so those fields 400 instead of
/// clamping or ignoring.
const MAX_SEARCH_LIMIT: i64 = 200;

/// Forward search window, in days: the furthest future `date` this route
/// will accept. Must be kept in sync by hand with `schedule-reference`'s
/// own forward-publish loop
/// (`crates/schedule-reference/src/main.rs::DESTINATION_DEPARTURES_FORWARD_DAYS`)
/// -- there is no shared constant across the crate boundary, matching this
/// codebase's existing per-crate-constant convention. See
/// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1.2.
const SEARCH_WINDOW_FORWARD_DAYS: i64 = 7;

/// Backward search window, in days. Must not exceed
/// `Config::schedule_destination_departures_retention_days`
/// (`crates/aggregator/src/config.rs`, currently 8, one more than this
/// value) or a date this route claims to support could 404 anyway because
/// its rows have already been pruned.
const SEARCH_WINDOW_BACKWARD_DAYS: i64 = 7;

/// `#[serde(deny_unknown_fields)]` is load-bearing here, not decorative:
/// without it, axum's `Query` extractor silently drops any query parameter
/// whose name doesn't match a field below (e.g. a caller sending
/// `arrivalFrom`/`arrivalTo` instead of the actual `arrival_from`/
/// `arrival_to`), producing a `200` whose filters simply never applied --
/// a request that LOOKS accepted but has zero effect. That is exactly the
/// failure mode this route's own doc comment already rejects for malformed
/// filter VALUES (see `MAX_SEARCH_LIMIT`'s doc comment above, and the
/// `arrival_from`/`arrival_to`-without-exactly-one-`stops_at` 400 in
/// `get_trains_search`): a 400 naming the field is the honest answer, not
/// a silently-narrower-than-requested search. This extends that same
/// posture to malformed (unrecognized) parameter NAMES. See
/// `trains_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it`
/// below for the regression coverage.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on ANY station a
    /// train calls at -- boarding or alighting, including where it starts
    /// or ends -- not just where it departs from or terminates.
    station: String,
    /// Optional, `"YYYY-MM-DD"`. Selects which `service_date` this search
    /// runs against; defaults to today (London-local, computed from the
    /// same single `Utc::now()` read this handler already uses for
    /// everything else -- see this file's own module doc comment on
    /// timezone handling and git history `baa4e75`/`8250a9a`). Must be
    /// within `SEARCH_WINDOW_BACKWARD_DAYS` days ago and
    /// `SEARCH_WINDOW_FORWARD_DAYS` days from today, inclusive, or this
    /// 400s -- a date outside the supported window is a request this
    /// deployment has already decided it can never answer, not a "nothing
    /// found" case, so it is NOT a 404.
    date: Option<String>,
    /// Optional. Filters to schedules whose TRUE origin (their first
    /// calling point) is this CRS -- NOT "any calling point along the
    /// route", which is what `station` above already answers. See
    /// `schedule_query::DestinationDeparture`'s own doc comment for the
    /// `origin_crs`-vs-`true_origin_crs` distinction this filters on.
    origin: Option<String>,
    /// Optional, zero or more, repeated query key
    /// (`?stops_at=RDG&stops_at=OXF`). Filters to schedules that call at
    /// EVERY named station somewhere along their route (boarding or
    /// alighting) -- ALL-of-N, not ANY-of-N -- independent of
    /// `station`/`origin` above. Replaces the earlier single-valued
    /// `destination` (TRUE final calling point) filter -- see this
    /// module's own doc comment and
    /// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md
    /// for why that is a genuine behavior change, not a rename.
    ///
    /// **Extracted via `axum_extra::extract::Query`, not plain
    /// `axum::extract::Query`, and that is load-bearing, not a style
    /// choice.** Confirmed empirically (not assumed, per this feature's own
    /// design note): axum's own `Query` runs on `serde_urlencoded`, whose
    /// `Deserializer` is a bare `serde::de::value::MapDeserializer` over
    /// EVERY raw `(key, value)` pair with no grouping at all -- a bare
    /// `Vec<String>` field fails outright even for `?stops_at=RDG&stops_at=OXF`
    /// ("invalid type: string ..., expected a sequence" on the first pair).
    /// `axum_extra::extract::Query` runs on `serde_html_form` instead, which
    /// is specifically built to group repeated keys into one sequence
    /// before struct-field deserialization ever sees them -- covering ZERO
    /// (`#[serde(default)]`, an empty `Vec`), ONE and 2+ occurrences all
    /// correctly with a plain `Vec<String>` field, no custom
    /// `deserialize_with` needed. `axum_extra::extract::Query`'s rejection
    /// still 400s the same way plain `Query`'s does, so `deny_unknown_fields`
    /// below and every other field's behavior are unaffected by this swap.
    #[serde(default)]
    stops_at: Vec<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    /// Always honored exactly as given -- including a value already in the
    /// past relative to `now` today -- because an explicit bound is a
    /// deliberate request, not something to silently re-floor. The
    /// `now`-forward default (see this module's own doc comment) only
    /// fills in when `from` is omitted entirely AND the resolved `date` is
    /// today; see `scheduled_from`'s computation in `get_trains_search`.
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on the time the train
    /// ARRIVES at `stops_at`'s single named calling point -- a SEPARATE
    /// filter from `from`/`to` above, which stay scoped to `station`.
    /// Requires `stops_at` to name EXACTLY ONE station; see this route's
    /// own validation below for why an arrival-time filter with an
    /// ambiguous (zero, or two-or-more) calling point to arrive at 400s
    /// instead of being silently ignored, mirroring `MAX_SEARCH_LIMIT`'s
    /// own doc comment's reasoning for the other filters. See
    /// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
    arrival_from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound. Same single-`stops_at`
    /// requirement as `arrival_from`.
    arrival_to: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Parses a caller-supplied `"HH:MM"` into a real `NaiveTime`.
fn normalize_time(label: &str, raw: &str) -> Result<chrono::NaiveTime, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("{label} must be a time of day in HH:MM form"),
        )
    })
}

/// Parses and window-bounds a caller-supplied `"YYYY-MM-DD"` `date`
/// against `today`. `today` is always the caller's single
/// `Utc::now().with_timezone(&Europe::London)`-derived value -- see this
/// function's own call site for why a second `Utc::now()` read must never
/// be introduced here.
fn normalize_date(
    raw: &str,
    today: chrono::NaiveDate,
) -> Result<chrono::NaiveDate, (StatusCode, String)> {
    let parsed = chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "date must be YYYY-MM-DD".to_string(),
        )
    })?;
    let earliest = today - chrono::Duration::days(SEARCH_WINDOW_BACKWARD_DAYS);
    let latest = today + chrono::Duration::days(SEARCH_WINDOW_FORWARD_DAYS);
    if parsed < earliest || parsed > latest {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "date must be within {SEARCH_WINDOW_BACKWARD_DAYS} days ago and \
                 {SEARCH_WINDOW_FORWARD_DAYS} days from today"
            ),
        ));
    }
    Ok(parsed)
}

/// Validates and uppercases a CRS code.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter CRS code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Parses and bounds the page size.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let parsed: i64 = raw.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        )
    })?;
    if parsed < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        ));
    }
    Ok(parsed.min(MAX_SEARCH_LIMIT))
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"HH:MM:SS|train_uid"`. Two parts, not three: `origin_crs` (the station
/// being searched) is now the fixed equality filter for the whole query,
/// not a value that varies within one page, so it carries no ordering
/// information and doesn't belong in the cursor.
///
/// Base64 makes the value visibly OPAQUE, matching this crate's established
/// posture for other cursors in this codebase. Not signed: the cursor names
/// a public timetable row on an unauthenticated route.
fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently ignored -- ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row.
fn decode_cursor(raw: &str) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [scheduled, train_uid] = parts.as_slice() else {
        return Err(invalid());
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(CallingPointDepartureCursor {
        scheduled,
        train_uid: (*train_uid).to_string(),
    })
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let station = normalize_crs("station", &params.station)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    // Deduped after normalization: `queries::search_schedule_calling_point_departures`'s
    // ALL-of-N match compares `COUNT(DISTINCT origin_crs)` against
    // `array_length($stops_at, 1)` -- a raw element count, not a distinct
    // one. A duplicate entry (e.g. `?stops_at=RDG&stops_at=RDG`, or the
    // same station typed twice with different casing before
    // `normalize_crs` uppercases it) would otherwise inflate that length
    // past what any real schedule's `COUNT(DISTINCT ...)` could ever
    // reach, silently zeroing out every match -- including ones that
    // genuinely stop at every named station. A duplicate is redundant
    // input, not ambiguous input like an empty/two-station `stops_at`
    // paired with an arrival bound (see the 400 below), so it's
    // normalized away here rather than rejected.
    let stops_at = params
        .stops_at
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("stops_at", s))
        .collect::<Result<std::collections::BTreeSet<String>, _>>()?
        .into_iter()
        .collect::<Vec<String>>();
    let from_time = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("to", s))
        .transpose()?;
    let arrival_from_time = params
        .arrival_from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("arrival_from", s))
        .transpose()?;
    let arrival_to_time = params
        .arrival_to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("arrival_to", s))
        .transpose()?;
    // Same "malformed input 400s, it is never silently ignored" posture
    // this file's own doc comment already argues for `from`/`to`/
    // `stops_at`/`origin`/`station` (lines 68-76): an arrival filter with
    // an ambiguous (zero, or two-or-more) calling point to arrive AT is
    // ambiguous input, not a wider search, so this 400s rather than
    // quietly acting as though neither bound was set.
    if (arrival_from_time.is_some() || arrival_to_time.is_some()) && stops_at.len() != 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "arrival_from and arrival_to require stops_at to name exactly one station".to_string(),
        ));
    }
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // `today` and `now` are deliberately read from ONE
    // `Utc::now().with_timezone(...)` call rather than two independent
    // `Utc::now()` calls -- see this same reasoning's original writeup in
    // this route's git history (baa4e75) for why two independent reads can
    // disagree about which calendar day it is around the UTC/London
    // midnight boundary during British Summer Time. `date` (if supplied)
    // is validated against THIS SAME `today`, never a second, independently
    // computed one -- see
    // docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §6.
    let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
    let today = london_now.date_naive();
    let now = london_now.time();

    let service_date = match params.date.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => normalize_date(raw, today)?,
        None => today,
    };

    // The `now`-forward floor is a DEFAULT, not a filter: it only ever
    // fills in for a lower bound the caller didn't supply at all. An
    // explicit `from` is always honored exactly as given -- including one
    // that names a time already in the past relative to `now` -- because
    // the caller asked for that specific window on purpose; silently
    // re-flooring it to `now` via `max(now, from)` produced zero results
    // for a perfectly legitimate request (e.g. searching 09:00-12:00 at
    // 15:00). The `now`-forward default itself only makes sense when
    // searching TODAY -- for any other date, forward or backward, there is
    // no "now" to be forward of, and applying today's clock time to a
    // different date's rows would silently and incorrectly filter them by
    // the wrong day's clock. See the design doc's §5.
    let scheduled_from = match from_time {
        Some(from) => from,
        None if service_date == today => now,
        None => chrono::NaiveTime::MIN,
    };

    let Some(page) = queries::search_schedule_calling_point_departures(
        &app.database,
        &station,
        service_date,
        scheduled_from,
        origin.as_deref(),
        &stops_at,
        to_time,
        arrival_from_time,
        arrival_to_time,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        let day_description = if service_date == today {
            "today".to_string()
        } else {
            service_date.format("%Y-%m-%d").to_string()
        };
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule data has been published for {day_description}"),
        ));
    };

    Ok(Json(json!({
        "results": page
            .departures
            .iter()
            .map(|row| calling_point_departure_json(row, &station))
            .collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_today(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(chrono::Utc::now().date_naive())
            .execute(pool)
            .await
            .expect("cleanup today's schedule_destination_departures rows");
    }

    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        use chrono::Timelike;

        let now = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .time();
        let now = chrono::NaiveTime::from_hms_opt(now.hour(), now.minute(), 0)
            .expect("valid time from valid hour/minute");
        let past = chrono::NaiveTime::MIN;
        let (soon, soon_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(30));
        let (later, later_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(60));
        assert!(
            soon_wrapped == 0 && later_wrapped == 0 && now > past,
            "these tests need at least an hour before midnight and a moment after it; \
             re-run outside 23:00-00:01 Europe/London"
        );
        (past, soon, later)
    }

    /// Seeds today with rows all sharing ONE `origin_crs` (`station_crs`,
    /// the new required search key) but varying `destination_crs` and
    /// `true_origin_crs`, so the two optional filters can be exercised
    /// independently of the fixed station. One already-departed row (which
    /// the route must hide), and two future rows.
    async fn seed_today(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (past, soon, later) = relative_times();
        for (scheduled, train_uid, destination_crs, true_origin_crs) in [
            (past, "C10000", "WAT", Some("PAD")),
            (soon, "C10001", "WAT", Some("PAD")),
            (later, "C10002", "BRI", Some("SWA")),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind(station_crs)
            .bind(true_origin_crs)
            .execute(pool)
            .await
            .expect("seed fixture row");
        }
    }

    async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn results(body: &str) -> Vec<Value> {
        let json: Value = serde_json::from_str(body).unwrap();
        assert!(
            json.is_object() && json.get("results").is_some() && json.get("nextCursor").is_some(),
            "the body is an envelope with exactly `results` and `nextCursor`: {json}"
        );
        json["results"].as_array().cloned().unwrap()
    }

    fn next_cursor(body: &str) -> Option<String> {
        let json: Value = serde_json::from_str(body).unwrap();
        json["nextCursor"].as_str().map(str::to_string)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_missing_station_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "station is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_station_is_a_400_not_a_404() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("station"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("from"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_arrival_from_without_any_stops_at_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&arrival_from=09:00").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("stops_at"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_arrival_to_without_any_stops_at_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search?station=ZRB&arrival_to=09:00").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_arrival_from_with_two_stops_at_entries_is_a_400() {
        // The 2+-entries companion to the zero-entries case above: once
        // `stops_at` names more than one station there is no single
        // well-defined calling point left to scope "arrival" to either.
        let pool = connect().await;
        let (status, body) = get(
            &pool,
            "/trains/search?station=ZRB&stops_at=WAT&stops_at=BRI&arrival_from=09:00",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("stops_at"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_arrival_from_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(
            &pool,
            "/trains/search?station=ZRB&stops_at=WAT&arrival_from=teatime",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("arrival_from"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("after"),
            "400 body should name the field: {body}"
        );

        let (status, _) = get(&pool, "/trains/search?station=ZRB&after=bm9uc2Vuc2U").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, _) = get(&pool, "/trains/search?station=ZRB&limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("limit"),
            "400 body should name the field: {body}"
        );

        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_SEARCH_LIMIT, never rejected"
        );
        assert_eq!(
            results(&body).len(),
            2,
            "the fixture only has two future rows"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_nothing_published_for_today_is_a_404() {
        let pool = connect().await;
        delete_today(&pool).await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("today"),
            "the 404 is about today's publish, not about the station: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_unknown_station_on_a_published_day_is_200_and_empty() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRF").await;
        assert_eq!(status, StatusCode::OK);
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&origin=ZZZ").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "published-but-unmatched is a 200 with an empty results array, never a 404"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_station_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=zrb").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let (_, soon, _) = relative_times();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(
            rows[0]["scheduled"],
            soon.format("%H:%M").to_string(),
            "seconds trimmed"
        );
        assert_eq!(
            rows[0]["stationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert_eq!(rows[0]["originCrs"], "PAD");
        assert_eq!(rows[0]["destinationCrs"], "WAT");
        assert!(
            rows[0].get("origin_crs").is_none(),
            "no stray snake_case field"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        assert!(
            !uids.contains(&"C10000"),
            "an already-departed row must not be returned: {uids:?}"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_honors_an_explicit_from_to_window_fully_in_the_past_on_today() {
        // Regression for the reported bug: a caller who explicitly asks for
        // a past `from`/`to` window on TODAY's date (e.g. searching
        // 09:00-12:00 at 3pm) must get the real matching rows, not an
        // empty result -- an explicit lower bound must never be silently
        // re-floored to `now` via `max(now, from)`. `seed_today` plants
        // `C10000` at `NaiveTime::MIN` (00:00:00), already-departed by any
        // time this test runs (guarded by `relative_times`'s own
        // near-midnight assertion), so an explicit `from=00:00&to=00:00`
        // window can only return it if the floor is gone.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&from=00:00&to=00:00").await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            uids,
            vec!["C10000".to_string()],
            "an explicit past from/to window on today must return its real matching rows, \
             not be silently floored to now: {uids:?}"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_with_no_explicit_from_still_defaults_to_the_now_floor_on_today() {
        // Companion to the test above: proves the DEFAULT (no explicit
        // `from` at all) still floors to `now` on today's date, so a bare
        // search keeps showing "what's coming up next" rather than the
        // whole day including already-departed trains. `seed_today` plants
        // one already-departed row (`C10000`) and two future ones
        // (`C10001`, `C10002`); with no `from` supplied, only the future
        // two must come back.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let mut uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        uids.sort();
        assert_eq!(
            uids,
            vec!["C10001".to_string(), "C10002".to_string()],
            "with no explicit `from`, today's search must still default to the now-forward \
             floor: {uids:?}"
        );
        delete_today(&pool).await;
    }

    /// Three schedules sharing the required station `ZRB` but with varying
    /// calling points beyond it, built to discriminate `stops_at`'s
    /// ALL-of-N membership check from a plain single-station filter:
    ///
    /// * `T51001` calls `ZRB`, `AAA`, `BBB` AND `CCC`.
    /// * `T51002` calls `ZRB`, `AAA` and `BBB`, but NOT `CCC`.
    /// * `T51003` calls `ZRB` and `AAA` only, from a DIFFERENT true origin
    ///   (`PAD`, not `SWA`) -- lets a `stops_at` test double as an
    ///   `origin`-independence check without a second fixture.
    ///
    /// Every row's `destination_crs` is `EEE` -- none of `AAA`/`BBB`/`CCC`
    /// is any of these schedules' TRUE destination, which is the load-
    /// bearing fact `trains_search_stops_at_single_entry_matches_regardless_of_true_destination`
    /// exists to exploit: the deleted `destination` filter could never have
    /// matched any of these trains on `AAA`, but `stops_at` does.
    async fn seed_stops_at(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (_, soon, later) = relative_times();
        let five = chrono::Duration::minutes(5);
        for (train_uid, true_origin_crs, calling_points) in [
            (
                "T51001",
                "SWA",
                vec![
                    (station_crs, soon),
                    ("AAA", soon + five),
                    ("BBB", soon + five * 2),
                    ("CCC", soon + five * 3),
                ],
            ),
            (
                "T51002",
                "SWA",
                vec![
                    (station_crs, soon + five * 4),
                    ("AAA", soon + five * 5),
                    ("BBB", soon + five * 6),
                ],
            ),
            (
                "T51003",
                "PAD",
                vec![(station_crs, later), ("AAA", later + five)],
            ),
        ] {
            for (origin_crs, scheduled) in calling_points {
                sqlx::query(
                    "INSERT INTO schedule_destination_departures \
                        (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(today)
                .bind("EEE")
                .bind(scheduled)
                .bind(train_uid)
                .bind(origin_crs)
                .bind(true_origin_crs)
                .execute(pool)
                .await
                .expect("seed stops_at fixture row");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stops_at_requires_all_listed_calling_points_all_of_n() {
        let pool = connect().await;
        seed_stops_at(&pool, "ZRB").await;

        let (status, body) = get(
            &pool,
            "/trains/search?station=ZRB&stops_at=AAA&stops_at=CCC",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(
            rows.len(),
            1,
            "T51002 calls AAA but not CCC, and must be excluded even though it partially matches: {rows:?}"
        );
        assert_eq!(rows[0]["uid"], "T51001");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stops_at_a_duplicate_entry_does_not_zero_out_matches() {
        // Regression: `search_schedule_calling_point_departures`'s ALL-of-N
        // check compares `COUNT(DISTINCT origin_crs)` against
        // `array_length($stops_at, 1)` -- a raw element count, not a
        // distinct one. Sending the same station twice used to inflate
        // that length past what any real schedule's distinct-calling-point
        // count could ever reach, silently matching nothing at all --
        // including T51001/T51002/T51003, which genuinely all call at AAA.
        // `get_trains_search` now dedupes `stops_at` after normalization
        // specifically to prevent this.
        let pool = connect().await;
        seed_stops_at(&pool, "ZRB").await;

        let (status, body) = get(
            &pool,
            "/trains/search?station=ZRB&stops_at=AAA&stops_at=AAA",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert!(
            !rows.is_empty(),
            "a duplicate stops_at entry must not silently zero out every match: {rows:?}"
        );
        let uids: std::collections::BTreeSet<&str> = rows
            .iter()
            .map(|row| row["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            std::collections::BTreeSet::from(["T51001", "T51002", "T51003"]),
            "?stops_at=AAA&stops_at=AAA must match exactly the same trains as a single \
             ?stops_at=AAA: {rows:?}"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stops_at_single_entry_matches_regardless_of_true_destination() {
        let pool = connect().await;
        seed_stops_at(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&stops_at=AAA").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: std::collections::BTreeSet<&str> = rows
            .iter()
            .map(|row| row["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            std::collections::BTreeSet::from(["T51001", "T51002", "T51003"]),
            "every schedule calling at AAA must match, even though NONE of them terminates \
             there (their true destination is EEE): {rows:?}"
        );
        for row in &rows {
            assert_eq!(
                row["destinationCrs"], "EEE",
                "stops_at=AAA matched a schedule whose true destination is EEE, not AAA -- \
                 proving this is not a disguised true-destination filter"
            );
        }

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_combines_origin_stops_at_and_time_filters_together() {
        let pool = connect().await;
        seed_stops_at(&pool, "ZRB").await;
        let (_, soon, later) = relative_times();
        let five = chrono::Duration::minutes(5);
        // T51001 (SWA, scheduled `soon`) and T51002 (SWA, scheduled
        // `soon + 4*5m`) both call AAA and share true_origin SWA; the
        // `from` bound below excludes T51001 by time, and T51003's
        // different true_origin (PAD) is what origin=SWA excludes it on.
        let uri = format!(
            "/trains/search?station=ZRB&origin=SWA&stops_at=AAA&from={}&to={}",
            (soon + five * 4).format("%H:%M"),
            later.format("%H:%M"),
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "T51002");
        delete_today(&pool).await;
    }

    /// Two schedules sharing the required station `ZRB`, both also calling
    /// at the INTERMEDIATE point `OXF` (neither's true destination -- both
    /// terminate at `BHM`) at the SAME `scheduled` (station) time, but with
    /// DIFFERENT arrivals at `OXF` itself -- so only `arrival_from`/
    /// `arrival_to` scoped to `OXF`'s own `calling_point_arrival`, never
    /// `scheduled`/`to` (station-scoped) nor the schedule-level
    /// `destination_arrival` (BHM-scoped), can tell them apart.
    async fn seed_stops_at_arrival(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (_, soon, later) = relative_times();
        for (train_uid, oxf_arrival) in [("T52001", soon), ("T52002", later)] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, calling_point_arrival) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(today)
            .bind("BHM")
            .bind(soon)
            .bind(train_uid)
            .bind(station_crs)
            .bind(Option::<&str>::None)
            .bind(Option::<chrono::NaiveTime>::None)
            .execute(pool)
            .await
            .expect("seed the station row");
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, calling_point_arrival) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(today)
            .bind("BHM")
            .bind(soon)
            .bind(train_uid)
            .bind("OXF")
            .bind(Option::<&str>::None)
            .bind(oxf_arrival)
            .execute(pool)
            .await
            .expect("seed the OXF row");
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stop_arrival_filters_by_the_named_intermediate_calling_points_own_arrival()
     {
        let pool = connect().await;
        seed_stops_at_arrival(&pool, "ZRB").await;
        let (_, _, later) = relative_times();

        let uri = format!(
            "/trains/search?station=ZRB&stops_at=OXF&arrival_from={}&arrival_to={}",
            later.format("%H:%M"),
            later.format("%H:%M"),
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "T52002");
        assert_eq!(
            rows[0]["destinationCrs"], "BHM",
            "OXF is NOT this schedule's true destination -- the arrival filter matched OXF's \
             OWN arrival, not BHM's destination_arrival"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&body).len(), 2);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, first) = get(&pool, "/trains/search?station=ZRB&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let first_rows = results(&first);
        assert_eq!(first_rows.len(), 1);
        assert_eq!(first_rows[0]["uid"], "C10001");
        let cursor = next_cursor(&first).expect("a second page exists, so a cursor is returned");

        let (status, second) = get(
            &pool,
            &format!("/trains/search?station=ZRB&limit=1&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_eq!(
            second_rows[0]["uid"], "C10002",
            "`after` must continue from the cursor, not restart at page 1"
        );
        assert!(
            next_cursor(&second).is_none(),
            "the last page must not hand back a cursor"
        );

        delete_today(&pool).await;
    }

    fn london_is_currently_ahead_of_utc() -> bool {
        use chrono::Offset;

        chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .offset()
            .fix()
            .local_minus_utc()
            != 0
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_inside_the_utc_vs_london_gap() {
        let pool = connect().await;
        delete_today(&pool).await;

        let is_bst = london_is_currently_ahead_of_utc();
        let today = chrono::Utc::now().date_naive();
        let london_time = {
            use chrono::Timelike;

            let t = chrono::Utc::now()
                .with_timezone(&chrono_tz::Europe::London)
                .time();
            chrono::NaiveTime::from_hms_opt(t.hour(), t.minute(), 0)
                .expect("valid time from valid hour/minute")
        };
        let (gap_time, wrapped) = london_time.overflowing_sub_signed(chrono::Duration::minutes(20));
        assert!(
            wrapped == 0 && london_time >= chrono::NaiveTime::from_hms_opt(0, 20, 0).unwrap(),
            "this test needs at least 20 minutes since London midnight; re-run outside \
             00:00-00:20 Europe/London"
        );

        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(today)
        .bind("WAT")
        .bind(gap_time)
        .bind("C10099")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed the gap-row fixture");

        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();

        if is_bst {
            assert!(
                !uids.contains(&"C10099".to_string()),
                "during BST, a row scheduled 20 minutes before the correct London-local `now` \
                 has already departed and must be excluded; if this fails, `now` has regressed \
                 to bare UTC time: {uids:?}"
            );
        } else {
            // Not currently observing BST: `Europe::London` and UTC agree
            // outside BST, so there is no gap between the two clocks to pin
            // this fixture inside -- reverting the route to bare UTC would
            // compute the exact same `now` this test just ran against, and
            // the assertion above would pass either way. There is nothing
            // this test COULD discriminate in that window, so skipping the
            // core assertion here is a true no-op, not a flaky pass/fail or
            // a silently-lost coverage gap.
        }

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_date_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&date=not-a-date").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("date"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_date_outside_the_supported_window() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let too_far_future = today + chrono::Duration::days(8);
        let too_far_past = today - chrono::Duration::days(8);

        let (status, body) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}",
                too_far_future.format("%Y-%m-%d")
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("date"),
            "400 body should name the field: {body}"
        );

        let (status, body) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}",
                too_far_past.format("%Y-%m-%d")
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("date"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_accepts_a_date_exactly_at_the_edge_of_the_window() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let edge_future = today + chrono::Duration::days(7);
        let edge_past = today - chrono::Duration::days(7);

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date IN ($1, $2)")
            .bind(edge_future)
            .bind(edge_past)
            .execute(&pool)
            .await
            .expect("cleanup edge-date fixtures");

        for (date, uid) in [(edge_future, "C30001"), (edge_past, "C30002")] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(date)
            .bind("WAT")
            .bind(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap())
            .bind(uid)
            .bind("ZRB")
            .bind(Option::<&str>::None)
            .execute(&pool)
            .await
            .expect("seed edge-date fixture row");

            let (status, body) = get(
                &pool,
                &format!(
                    "/trains/search?station=ZRB&date={}",
                    date.format("%Y-%m-%d")
                ),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "exactly 7 days out must be inside the window: {body}"
            );
            let rows = results(&body);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["uid"], uid);
        }

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date IN ($1, $2)")
            .bind(edge_future)
            .bind(edge_past)
            .execute(&pool)
            .await
            .expect("cleanup edge-date fixtures");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_now_forward_only_when_date_is_today() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");

        // A row scheduled at the very start of tomorrow -- long "in the
        // past" relative to today's current clock time, which is exactly
        // the case that must NOT be now-forward-filtered once `date` picks
        // a day other than today.
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tomorrow)
        .bind("WAT")
        .bind(chrono::NaiveTime::from_hms_opt(0, 5, 0).unwrap())
        .bind("C30003")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed tomorrow's early-morning fixture row");

        let (status, body) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}",
                tomorrow.format("%Y-%m-%d")
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        assert!(
            uids.contains(&"C30003".to_string()),
            "a 00:05 row on a FUTURE date must not be hidden by today's now-forward filter: {uids:?}"
        );

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_uses_from_as_a_plain_bound_with_no_now_floor_on_a_non_today_date() {
        // Coverage for `scheduled_from`'s `Some(from) => from` arm on a
        // non-today date specifically --
        // `trains_search_applies_now_forward_only_when_date_is_today` above
        // only exercises the `None` arm (no `from` supplied at all) for a
        // non-today date. This test supplies `from` explicitly alongside a
        // non-today `date` and proves it is used as a PLAIN inclusive lower
        // bound with NO `now` floor applied at all, per this route's own
        // doc comment on `scheduled_from` in `get_trains_search` -- today's
        // clock is irrelevant to every OTHER date, explicit `from` or not.
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");

        // A row scheduled at the very start of tomorrow. If `from` were
        // wrongly combined with TODAY's `now` via `max(now, from)`, this row
        // would be excluded any time after 00:05 today -- which is true for
        // nearly the entire day, so this fixture reliably discriminates the
        // bug this test is guarding against.
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tomorrow)
        .bind("WAT")
        .bind(chrono::NaiveTime::from_hms_opt(0, 5, 0).unwrap())
        .bind("C30004")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed tomorrow's early-morning fixture row");

        let (status, body) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}&from=00:00",
                tomorrow.format("%Y-%m-%d")
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        assert!(
            uids.contains(&"C30004".to_string()),
            "an explicit from=00:00 on a FUTURE date must be a plain lower bound, with no \
             now-based floor leaking in from today's clock: {uids:?}"
        );

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_omitting_date_still_defaults_to_today() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            results(&body).len(),
            2,
            "identical to the existing today-only behavior when `date` is absent"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it()
    {
        // Root-cause regression for a bug reported as "arrivalFrom/
        // arrivalTo are accepted but never filter": those names don't exist
        // on `TrainSearchParams` at all -- the real fields are snake_case
        // `arrival_from`/`arrival_to`, exactly like every other query
        // parameter this route accepts (`from`, `to`, `origin`,
        // `stops_at`...). Before `#[serde(deny_unknown_fields)]` was added
        // to `TrainSearchParams`, axum's `Query` extractor silently dropped
        // any parameter name it didn't recognize -- so a caller who typos or
        // guesses the wrong casing for ANY filter (not just this one) gets a
        // 200 with an unfiltered result set instead of any indication their
        // filter never applied. That is precisely the "silently accepted,
        // zero effect" failure mode this file's own doc comment already
        // rejects for malformed VALUES (see `MAX_SEARCH_LIMIT`'s doc comment
        // and the `arrival_from`/`arrival_to`-without-exactly-one-`stops_at`
        // 400 above) -- this test extends the same posture to malformed
        // (unrecognized) parameter NAMES.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (_, _, later) = relative_times();

        let uri = format!(
            "/trains/search?station=ZRB&arrivalFrom={}&arrivalTo={}",
            later.format("%H:%M"),
            later.format("%H:%M"),
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "an unrecognized query parameter name must 400, not silently no-op: {body}"
        );
        assert!(
            body.contains("arrivalFrom"),
            "the 400 body should name the offending parameter: {body}"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_404_for_an_unpublished_in_window_date_names_that_date() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let target = today + chrono::Duration::days(3);
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(target)
            .execute(&pool)
            .await
            .expect("ensure target date has no rows");

        let (status, body) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}",
                target.format("%Y-%m-%d")
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains(&target.format("%Y-%m-%d").to_string()),
            "the 404 should name the actually-requested date, not always say 'today': {body}"
        );
    }
}
