//! `GET /public/trains/resolve` (see `get_trains_resolve`) lives here too:
//! it maps a live departure-board row to a CIF `(uid, service_date)`.
//!
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
//! TRUE first calling point) and `stops_at` (one other calling point, at
//! most) are both optional, independent filters, along with the `from`/`to`
//! time range. `from`/`to` bound `station`'s own `scheduled` time;
//! `arrival_from`/`arrival_to` are a SEPARATE, independent `"HH:MM"` bound
//! pair on when the train ARRIVES at `stops_at`'s named calling point, and
//! require `stops_at` to be set at all (a `400` otherwise -- see
//! `TrainSearchParams::arrival_from`'s own doc comment and
//! docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md).
//! `operator` (2026-10-22) is a comma-separated list of ATOC codes, "any
//! of": a row carries its schedule's operator when it has one (a
//! schedule's `BX` record's ATOC code, decoded into `operator_atoc` -- see
//! `schedule_query::records::BasicSchedule::operator_atoc` -- and rendered
//! as `render::calling_point_departure_json`'s `"operator"` key). It
//! follows `GET /Journeys/{journeyId}/legs/{legId}/candidates`'
//! (`routes::journeys::get_leg_candidates`) filter, plus a `400` for a code
//! that is not a known operator.
//!
//! **A search covers the whole train day of `date`** (2026-10-22): every
//! service of that service date, its departures after midnight
//! (`dayOffset: 1`) included, and never the previous date's late trains
//! unless `across_midnight=true` asks for them (see below).
//! Times compare in train-day minutes (minutes past `date`'s midnight,
//! over 1440 after it) on the public basis the rows show; `*_day_offset`
//! puts a bound after midnight, and a `to` earlier than an explicit `from`
//! runs past it. Results run in train-day order, so the after-midnight
//! departures come last. See `resolve_window` and
//! `queries::search_schedule_calling_point_departures`.
//!
//! **`across_midnight=true`** (2026-10-22) also returns the PREVIOUS
//! service date's trains still running after its midnight (its
//! `dayOffset >= 1` rows) whose departure falls inside the window on
//! `date`'s clock, merged into train-day order -- `/Trips/plan`'s
//! `acrossMidnight`, for the small hours. Every row carries its own
//! `serviceDate` (the key of its `GET /Train/by-uid/{uid}/{date}`), and its
//! day offsets count from that. `/trains` sends it when the date defaults
//! to today; without it a search stays strictly `date`'s own services. See
//! `queries::search_schedule_calling_point_departures`.
//!
//! **`stops_at` replaced the earlier single-valued `destination` filter**
//! (the schedule's TRUE final calling point) -- see
//! docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md for
//! the full reasoning. This is a genuine, deliberate behavior change, not
//! a rename: `destination` meant "this IS the schedule's true final stop";
//! `stops_at` means "the schedule calls here at some point in its route",
//! true destination or not. `stops_at` is deliberately scoped to a SINGLE
//! station (not a list) -- a deliberate simplification of an earlier
//! multi-station ("ALL-of-N") shape that shipped and was then scoped back
//! down; see that design doc for the reasoning. A caller who genuinely
//! needs "true destination equals X" (as opposed to "calls at X") has no
//! equivalent filter any more -- see that design doc's own open question.
//!
//! **`stops_at` always means "later in the journey than `station`", for
//! ANY named station, not only when it repeats `station`.** A row is
//! trivially a member of its own calling-point list, so
//! `station=WAT&stops_at=WAT` used to return every train out of Waterloo
//! -- identical to supplying no `stops_at` at all. A call at the station
//! `station` itself named counts only if it comes LATER in the journey,
//! which is the question a caller typing one station into both fields
//! actually means: does this working come BACK, as a loop/circular service
//! does. **(2026-09-22, superseding the original loop-service fix's other
//! half)** a call at a DIFFERENT station now applies the exact same
//! ordering test: `stops_at=X` only matches a call at X that falls later
//! than `station`, so a train that already passed through X before ever
//! reaching `station` is excluded -- "stops at X" means you can actually
//! get there from where you searched, full stop, with no special-cased
//! exception for the non-loop case. See
//! docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md's
//! "Addendum (2026-09-22)".
//!
//! **The schedule's TRUE terminus now satisfies `stops_at` too**, having
//! never done so before: it is arrival-only, so it has no row of its own
//! in `schedule_destination_departures` and was previously unreachable.
//! That is what makes the Kingston-Loop shape -- out of Waterloo, round,
//! terminating back at Waterloo -- findable at all, but it is NOT confined
//! to the loop case: every `stops_at` search naming any schedule's true
//! destination now returns trains it did not return before. Deliberate,
//! and it partially (only partially -- the matches are still mixed in with
//! intermediate-stop ones) answers the "true destination equals X"
//! question the paragraph above says has no filter. See
//! `queries::search_schedule_calling_point_departures`'s own doc comment.
//! `date` (`"YYYY-MM-DD"`, optional) selects which `service_date` this
//! search runs against, defaulting to today. It is accepted within the
//! static window around today (`SEARCH_WINDOW_BACKWARD_DAYS`/
//! `SEARCH_WINDOW_FORWARD_DAYS` below) or anywhere inside the published
//! timetable's own `service_date` range (2026-10-08, see
//! `searchable_range`); anything else is a `400` naming both ranges.
//! `GET /public/trains/search/dates` reports the same range. See
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
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::CallingPointDepartureCursor;
use crate::data::train_resolve;
use crate::render::calling_point_departure_json;

/// Page size when the caller does not ask for one.
pub(crate) const DEFAULT_SEARCH_LIMIT: i64 = 50;

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
pub(crate) const MAX_SEARCH_LIMIT: i64 = 200;

/// Forward static search window, in days: a future `date` this route
/// always accepts, published or not (a later one is accepted only when the
/// timetable holds rows that far out -- see `searchable_range`). This is
/// the floor of `schedule-reference`'s forward-publish window
/// (`SCHEDULE_FORWARD_PUBLISH_DAYS`, `crates/schedule-reference/src/config.rs`,
/// 7-60, default 28), not a copy of it: the published range read by
/// `searchable_range` carries the rest, so raising that setting needs no
/// change here. See
/// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1.2.
const SEARCH_WINDOW_FORWARD_DAYS: i64 = 7;

/// Backward static search window, in days (an earlier date is accepted
/// only while the timetable still holds it). Must not exceed
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
/// `arrival_from`/`arrival_to`-without-`stops_at` 400 in
/// `get_trains_search`): a 400 naming the field is the honest answer, not
/// a silently-narrower-than-requested search. This extends that same
/// posture to malformed (unrecognized) parameter NAMES. See
/// `trains_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it`
/// below for the regression coverage.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrainSearchParams {
    /// Required. A 3-letter station code; the search is keyed on ANY station a
    /// train calls at -- boarding or alighting, including where it starts
    /// or ends -- not just where it departs from or terminates.
    station: String,
    /// Optional, `"YYYY-MM-DD"`. Selects which `service_date` this search
    /// runs against; defaults to today (London-local, computed from the
    /// same single `Utc::now()` read this handler already uses for
    /// everything else -- see this file's own module doc comment on
    /// timezone handling and git history `baa4e75`/`8250a9a`). Must be
    /// within `SEARCH_WINDOW_BACKWARD_DAYS` days ago and
    /// `SEARCH_WINDOW_FORWARD_DAYS` days from today, inclusive, or within
    /// the published timetable's `service_date` range (see
    /// `searchable_range`), or this 400s -- a date outside both is a
    /// request this deployment cannot answer, not a "nothing found" case,
    /// so it is NOT a 404.
    date: Option<String>,
    /// Optional. Filters to schedules whose TRUE origin (their first
    /// calling point) is this CRS -- NOT "any calling point along the
    /// route", which is what `station` above already answers. See
    /// `schedule_query::DestinationDeparture`'s own doc comment for the
    /// `origin_crs`-vs-`true_origin_crs` distinction this filters on.
    origin: Option<String>,
    /// Optional. Filters to schedules that call at this station somewhere
    /// along their route (boarding or alighting), the schedule's true
    /// terminus included -- except that a call at the station `station`
    /// itself named counts only if it falls LATER in the journey, so
    /// naming the same CRS in both fields finds loop/circular workings
    /// rather than matching every train out of that station by
    /// construction. Otherwise independent of
    /// `station`/`origin` above. Replaces the earlier single-valued
    /// `destination` (TRUE final calling point) filter -- see this
    /// module's own doc comment and
    /// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md
    /// for why that is a genuine behavior change, not a rename.
    ///
    /// Deliberately a single value, not a list: an earlier version of this
    /// filter accepted zero or more repeated `stops_at` keys and matched
    /// ALL-of-N (relational division). That shipped and was then
    /// deliberately scoped back down to exactly one station -- plain
    /// `axum::extract::Query` (`serde_urlencoded`) is sufficient for a
    /// single optional `String` field; the `axum_extra`/`serde_html_form`
    /// dependency the multi-valued shape needed no longer applies here.
    stops_at: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on the departure from
    /// `station` -- its public time, else its working one, as each row's
    /// `scheduled` shows it (2026-10-22: the working time until then), in
    /// train-day minutes with `from_day_offset`. Always honored exactly as given -- including a value already in the
    /// past relative to `now` today -- because an explicit bound is a
    /// deliberate request, not something to silently re-floor. The
    /// `now`-forward default (see this module's own doc comment) only
    /// fills in when `from` is omitted entirely AND the resolved `date` is
    /// today; see `default_from` and `resolve_window` in `get_trains_search`.
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound, as `from`; see
    /// `to_day_offset` for one after midnight.
    to: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on the time the train
    /// ARRIVES at `stops_at`'s named calling point (its public arrival,
    /// else its working one, in train-day minutes with
    /// `arrival_from_day_offset`, 2026-10-22) -- a SEPARATE filter
    /// from `from`/`to` above, which stay scoped to `station`. Requires
    /// `stops_at` to be set; see this route's own validation below for why
    /// an arrival-time filter with no calling point to arrive at 400s
    /// instead of being silently ignored, mirroring `MAX_SEARCH_LIMIT`'s
    /// own doc comment's reasoning for the other filters. Where `stops_at`
    /// matched the schedule's true terminus (which has no calling-point
    /// row of its own), this bounds that terminus's own arrival instead.
    /// Either way a schedule whose named calling point has no booked
    /// arrival at all is dropped, not kept -- see
    /// `queries::search_schedule_calling_point_departures`. See
    /// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
    arrival_from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound. Same `stops_at`-set
    /// requirement as `arrival_from`.
    arrival_to: Option<String>,
    /// Optional, `0` or `1` (2026-10-22): the day `from` is on, counted
    /// from `date` -- `1` is after `date`'s midnight, still on `date`'s
    /// trains (`date=FRI&from=00:30&from_day_offset=1` is "from 00:30 on
    /// Saturday morning"), the same convention as `/Trips/plan`'s
    /// `dayOffset`. Absent is `0`. Needs `from`. See `resolve_window`.
    from_day_offset: Option<String>,
    /// Optional, `0` or `1` (2026-10-22): the day `to` is on, as
    /// `from_day_offset`. Absent, it is inferred: `1` when `to` is earlier
    /// in the day than an explicit `from` (`from=22:00&to=01:30` runs past
    /// midnight), else `0`. Needs `to`.
    to_day_offset: Option<String>,
    /// Optional, `0` or `1` (2026-10-22): the day `arrival_from` is on, as
    /// `from_day_offset`. Absent is `0`. Needs `arrival_from`.
    arrival_from_day_offset: Option<String>,
    /// Optional, `0` or `1` (2026-10-22): the day `arrival_to` is on.
    /// Absent, it is inferred like `to_day_offset`'s, against
    /// `arrival_from` when given, else an explicit `from`. Needs
    /// `arrival_to`.
    arrival_to_day_offset: Option<String>,
    /// Optional (2026-10-22). Comma-separated ATOC operator codes, e.g.
    /// `operator=SW,GW`: only trains run by one of them (the schedule's
    /// CIF `BX` operator, each row's `operator`). Codes are trimmed and
    /// uppercased, empty entries dropped; a code that is not two letters or
    /// digits, or not a known operator (`tocs`), is a `400`. Same
    /// multi-value convention as `/incidents`' `operator` and the leg
    /// candidates' `operator`. Known means in `tocs`, or running a train in
    /// the published timetable on a date this search covers (2026-10-22;
    /// see `check_known_operators`).
    operator: Option<String>,
    /// Optional, `true` or `false` (2026-10-22): also the previous service
    /// date's trains still running after its midnight, inside the window
    /// on `date`'s clock (see this module's doc comment). Absent is
    /// `false`: `date`'s own services only.
    across_midnight: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new()
        .route("/trains/search", axum::routing::get(get_trains_search))
        .route(
            "/trains/search/dates",
            axum::routing::get(get_trains_search_dates),
        )
        .route(
            "/trains/search/operators",
            axum::routing::get(get_trains_search_operators),
        )
        .route("/trains/resolve", axum::routing::get(get_trains_resolve))
}

/// Query parameters of `GET /public/trains/resolve`. Unknown names are a
/// `400` for the same reason as [`TrainSearchParams`]: a misspelled
/// `destination` would otherwise silently widen the match.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrainResolveParams {
    /// Required. The board's own station, a 3-letter station code.
    station: String,
    /// Required, `"YYYY-MM-DD"`: the London-local calendar date of `time`
    /// at `station` -- NOT necessarily the train's CIF service date (a
    /// train that started before midnight carries the previous one; the
    /// response's `serviceDate` says which). Same window as
    /// `/public/trains/search`'s `date`.
    date: String,
    /// Required, `"HH:MM"`: the board's scheduled (public) time at `station`.
    time: String,
    /// Optional: the board's Retail Service ID (LDBWS `rsid`), 6-8 letters
    /// and digits.
    rsid: Option<String>,
    /// Optional: the board's destination CRS.
    destination: Option<String>,
    /// Optional: the board's operator, a 2-character ATOC code.
    operator: Option<String>,
    /// Optional: `departure` (default) or `arrival`.
    kind: Option<String>,
}

#[expect(
    clippy::ref_option,
    reason = "callers hold the Option by reference in a struct field"
)]
fn non_empty(raw: &Option<String>) -> Option<&str> {
    raw.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Validates and uppercases a caller-supplied Retail Service ID.
fn normalize_rsid(raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if !(6..=8).contains(&trimmed.len()) || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "rsid must be a 6-8 character retail service ID (letters and digits)".to_string(),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Validates and uppercases a 2-character ATOC operator code.
fn normalize_operator(raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "operator must be a 2-character ATOC code".to_string(),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// `GET /public/trains/resolve?station=&date=&time=[&rsid=][&destination=][&operator=][&kind=]`
/// -- resolves one live departure-board row to the CIF key
/// `GET /Train/by-uid/{uid}/{date}` takes. Built for a client (the MCP
/// server, a board UI) that holds an LDBWS row -- which carries no `uid`
/// and no `rid` -- and wants the train page for it.
///
/// Matching lives in `crate::data::train_resolve` (see `resolve` there for
/// the exact rules): an `rsid` is matched exactly, then on its first 6
/// characters, within +-5 minutes of the WORKING time; with no usable
/// `rsid`, the +-2 minute timetable heuristic plus destination/operator
/// that `routes::departures::get_station_departures` documents. Both the
/// given date (`day_offset = 0`) and, after midnight, the previous service
/// date (`day_offset = 1`) are searched.
///
/// * `200 {"trainUid","serviceDate","matchedOn","href"}` -- `matchedOn` is
///   `"rsid"`, `"rsidPrefix"` or `"timetable"`, so a caller can tell an
///   exact join from a heuristic one.
/// * `400` (plain text naming the field) for a missing/malformed parameter
///   or a date outside the window.
/// * `404` (plain text) when nothing matches.
/// * `409` (plain text listing every `uid/serviceDate`) when more than one
///   train still matches -- this route never guesses.
async fn get_trains_resolve(
    State(app): State<App>,
    Query(params): Query<TrainResolveParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let station = normalize_crs("station", &params.station)?;
    let time = normalize_time("time", params.time.trim())?;
    let rsid = non_empty(&params.rsid).map(normalize_rsid).transpose()?;
    let destination = non_empty(&params.destination)
        .map(|s| normalize_crs("destination", s))
        .transpose()?;
    let operator = non_empty(&params.operator)
        .map(normalize_operator)
        .transpose()?;
    let kind = match non_empty(&params.kind) {
        None | Some("departure") => train_resolve::ResolveKind::Departure,
        Some("arrival") => train_resolve::ResolveKind::Arrival,
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "kind must be departure or arrival".to_string(),
            ));
        }
    };
    // Same single London-local `today` read and window as `get_trains_search`.
    let today = super::london_today();
    let date = normalize_date(&params.date, today)?;

    let request = train_resolve::ResolveRequest {
        target: date.and_time(time),
        rsid,
        destination,
        operator,
    };
    let candidates =
        train_resolve::resolve_candidates(&app.database, &station, request.target, kind)
            .await
            .map_err(internal_error)?;

    match train_resolve::resolve(&candidates, &request) {
        train_resolve::ResolveOutcome::Found {
            train_uid,
            service_date,
            matched_on,
        } => Ok(Json(json!({
            "href": format!("/Train/by-uid/{train_uid}/{service_date}"),
            "trainUid": train_uid,
            "serviceDate": service_date,
            "matchedOn": matched_on.as_str(),
        }))),
        train_resolve::ResolveOutcome::NotFound => Err((
            StatusCode::NOT_FOUND,
            format!("no scheduled train matches that board row at {station}"),
        )),
        train_resolve::ResolveOutcome::Ambiguous(keys) => Err((
            StatusCode::CONFLICT,
            format!(
                "more than one scheduled train matches that board row: {}",
                keys.iter()
                    .map(|(uid, date)| format!("{uid}/{date}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
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
    let parsed = parse_date(raw)?;
    if !static_window(today).contains(&parsed) {
        return Err((StatusCode::BAD_REQUEST, static_window_message()));
    }
    Ok(parsed)
}

/// Parses a caller-supplied `"YYYY-MM-DD"`, with no window check.
fn parse_date(raw: &str) -> Result<chrono::NaiveDate, (StatusCode, String)> {
    chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "date must be YYYY-MM-DD".to_string(),
        )
    })
}

/// `today - SEARCH_WINDOW_BACKWARD_DAYS ..= today + SEARCH_WINDOW_FORWARD_DAYS`.
fn static_window(today: chrono::NaiveDate) -> std::ops::RangeInclusive<chrono::NaiveDate> {
    (today - chrono::Duration::days(SEARCH_WINDOW_BACKWARD_DAYS))
        ..=(today + chrono::Duration::days(SEARCH_WINDOW_FORWARD_DAYS))
}

fn static_window_message() -> String {
    format!(
        "date must be within {SEARCH_WINDOW_BACKWARD_DAYS} days ago and \
         {SEARCH_WINDOW_FORWARD_DAYS} days from today"
    )
}

/// The dates `GET /public/trains/search` accepts: the static window
/// around `today` (unchanged since it shipped, so a date inside it still
/// answers `200` or `404` exactly as before) widened to cover every
/// `service_date` the timetable actually holds (`published`, from
/// `queries::schedule_destination_departures_date_range`). The second part
/// is what lets a client reach a date as far ahead as `schedule-reference`
/// publishes, or as far back as the aggregator keeps, without this crate
/// copying either setting.
fn searchable_range(
    today: chrono::NaiveDate,
    published: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
) -> (chrono::NaiveDate, chrono::NaiveDate) {
    let window = static_window(today);
    match published {
        Some((earliest, latest)) => ((*window.start()).min(earliest), (*window.end()).max(latest)),
        None => (*window.start(), *window.end()),
    }
}

/// Window-bounds a parsed search `date`. Inside the static window it is
/// accepted with no query; outside it, only when it falls within the
/// published timetable (see `searchable_range`). The `400` names the
/// searchable range and the published one, so a client can correct the
/// date without a second call.
async fn check_search_date(
    app: &App,
    date: chrono::NaiveDate,
    today: chrono::NaiveDate,
) -> Result<(), (StatusCode, String)> {
    if static_window(today).contains(&date) {
        return Ok(());
    }
    let published = queries::schedule_destination_departures_date_range(&app.database)
        .await
        .map_err(internal_error)?;
    let (from, to) = searchable_range(today, published);
    if (from..=to).contains(&date) {
        return Ok(());
    }
    Err((
        StatusCode::BAD_REQUEST,
        out_of_range_message(from, to, published),
    ))
}

fn out_of_range_message(
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    published: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
) -> String {
    match published {
        Some((earliest, latest)) => format!(
            "date must be between {from} and {to}: schedule data is published for \
             {earliest} to {latest}"
        ),
        None => format!("{}: no schedule data is published", static_window_message()),
    }
}

/// `GET /public/trains/search/dates` -- the `date` range
/// `GET /public/trains/search` accepts, for a client to check before it
/// searches.
///
/// `200 {"from", "to", "publishedFrom", "publishedTo", "provisionalFrom"}`:
/// `from`/`to` (`"YYYY-MM-DD"`, inclusive) bound what the search accepts (a
/// date outside is a `400`); `publishedFrom`/`publishedTo` bound the dates
/// the timetable holds rows for (`null` when it holds none). A date inside
/// `from..=to` with no rows of its own is a `404` from the search.
/// `provisionalFrom` (2026-10-08) is the first date whose timetable is
/// provisional (see `routes::provisional`); it may lie past `to`.
async fn get_trains_search_dates(
    State(app): State<App>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let today = super::london_today();
    let published = queries::schedule_destination_departures_date_range(&app.database)
        .await
        .map_err(internal_error)?;
    let (from, to) = searchable_range(today, published);
    Ok(Json(json!({
        "from": from,
        "to": to,
        "publishedFrom": published.map(|(earliest, _)| earliest),
        "publishedTo": published.map(|(_, latest)| latest),
        "provisionalFrom": crate::routes::provisional::provisional_from(today),
    })))
}

/// `GET /public/trains/search/operators` (2026-10-22) -- the operators
/// `GET /public/trains/search`'s `operator` can name, for a client's
/// operator picker.
///
/// `200 {"operators": [{"code", "name"}]}`, by code: every `tocs` operator
/// with its name, then joined by each operator code the published
/// timetable carries that `tocs` lacks, whose `name` is `null` (a client
/// shows the code). The search itself checks a code against the dates it
/// covers (see `check_known_operators`), so a code listed here can still
/// match no train on a given date.
async fn get_trains_search_operators(
    State(app): State<App>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let operators = crate::data::reference::train_search_operators(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(json!({
        "operators": operators
            .into_iter()
            .map(|(code, name)| json!({"code": code, "name": name}))
            .collect::<Vec<_>>(),
    })))
}

/// Validates and uppercases a CRS code.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter station code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// The four `"HH:MM"` bounds of a search, as given.
#[derive(Debug, Clone, Copy, Default)]
struct WindowTimes {
    from: Option<chrono::NaiveTime>,
    to: Option<chrono::NaiveTime>,
    arrival_from: Option<chrono::NaiveTime>,
    arrival_to: Option<chrono::NaiveTime>,
}

/// The four `*_day_offset` parameters, as given (`None` when absent).
#[derive(Debug, Clone, Copy, Default)]
struct WindowDayOffsets {
    from: Option<u32>,
    to: Option<u32>,
    arrival_from: Option<u32>,
    arrival_to: Option<u32>,
}

/// One end of the search window: a clock time on day `day_offset` of the
/// train day (0 the service date, 1 the morning after).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowBound {
    time: chrono::NaiveTime,
    day_offset: u32,
}

impl WindowBound {
    /// Train-day minutes (`queries::train_day_minutes`).
    fn minutes(self) -> i32 {
        queries::train_day_minutes(self.time, self.day_offset)
    }

    /// `{"time": "HH:MM", "dayOffset": n}`, `/Trips/plan`'s echo shape.
    fn to_json(self) -> Value {
        json!({
            "time": self.time.format("%H:%M").to_string(),
            "dayOffset": self.day_offset,
        })
    }
}

/// The search window, every bound resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    from: WindowBound,
    to: Option<WindowBound>,
    arrival_from: Option<WindowBound>,
    arrival_to: Option<WindowBound>,
}

impl Window {
    /// The response's `window`: `from`/`to` bound the departure from
    /// `station`, `arrivalFrom`/`arrivalTo` the arrival at `stops_at`;
    /// `null` where unbounded.
    fn to_json(self) -> Value {
        json!({
            "from": self.from.to_json(),
            "to": self.to.map(WindowBound::to_json),
            "arrivalFrom": self.arrival_from.map(WindowBound::to_json),
            "arrivalTo": self.arrival_to.map(WindowBound::to_json),
        })
    }
}

/// Resolves the search window. `from` is the explicit one, else
/// `default_from` (the `now` default, or midnight), on day `0` unless
/// `from_day_offset` says otherwise. An upper bound with no day offset of
/// its own runs past midnight when it is earlier in the day than its lower
/// bound -- `to` against an explicit `from`, `arrival_to` against
/// `arrival_from`, else an explicit `from` -- because read on the same day
/// it would match nothing: "from 22:00 to 01:30" means 01:30 the next
/// morning. Never against the `now` default: at 15:00, `to=09:00` stays an
/// already-passed window rather than turning into tomorrow morning. An
/// explicit offset always wins, even one that makes the window empty.
fn resolve_window(
    times: WindowTimes,
    offsets: WindowDayOffsets,
    default_from: chrono::NaiveTime,
) -> Window {
    let bound = |time, day_offset: Option<u32>| WindowBound {
        time,
        day_offset: day_offset.unwrap_or(0),
    };
    // An upper bound's day: its own, else the day that puts it at or after
    // `lower` (train-day minutes), when there is one to compare with.
    let upper = |time: chrono::NaiveTime, day_offset: Option<u32>, lower: Option<i32>| {
        let day_offset = day_offset.unwrap_or_else(|| {
            u32::from(lower.is_some_and(|lower| queries::train_day_minutes(time, 0) < lower))
        });
        WindowBound { time, day_offset }
    };
    let explicit_from = times.from.map(|time| bound(time, offsets.from));
    let from = explicit_from.unwrap_or_else(|| bound(default_from, None));
    let explicit_from_minutes = explicit_from.map(WindowBound::minutes);
    let arrival_from = times
        .arrival_from
        .map(|time| bound(time, offsets.arrival_from));
    Window {
        from,
        to: times
            .to
            .map(|time| upper(time, offsets.to, explicit_from_minutes)),
        arrival_from,
        arrival_to: times.arrival_to.map(|time| {
            upper(
                time,
                offsets.arrival_to,
                arrival_from
                    .map(WindowBound::minutes)
                    .or(explicit_from_minutes),
            )
        }),
    }
}

/// Parses one `*_day_offset`: absent or empty is `None`, `0` and `1` are
/// themselves, anything else is a `400`; so is an offset for a time that
/// was not given (`time_label` names it), as `/Trips/plan`'s `dayOffset`.
#[expect(
    clippy::ref_option,
    reason = "callers hold the Option by reference in a struct field"
)]
fn parse_day_offset(
    label: &str,
    raw: &Option<String>,
    time_label: &str,
    time: Option<chrono::NaiveTime>,
) -> Result<Option<u32>, (StatusCode, String)> {
    let Some(raw) = non_empty(raw) else {
        return Ok(None);
    };
    let day_offset = match raw {
        "0" => 0,
        "1" => 1,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{label} must be 0 or 1, not '{raw}'"),
            ));
        }
    };
    if time.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} needs {time_label}"),
        ));
    }
    Ok(Some(day_offset))
}

/// Parses `operator=`: comma-separated ATOC codes, trimmed, uppercased and
/// deduplicated, empty entries dropped. `None` when nothing is left (no
/// filter). A malformed code is a `400`.
fn parse_operators(raw: Option<&str>) -> Result<Option<Vec<String>>, (StatusCode, String)> {
    let mut codes: Vec<String> = Vec::new();
    for entry in raw.unwrap_or_default().split(',') {
        if entry.trim().is_empty() {
            continue;
        }
        let code = normalize_operator(entry)?;
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    Ok((!codes.is_empty()).then_some(codes))
}

/// A `400` naming every code in `codes` that is not a known operator: an
/// unknown code would otherwise just match nothing, which reads as "no
/// trains" rather than as the typo it is. Known is in `tocs` OR running a
/// train in the published timetable on one of `service_dates` (2026-10-22):
/// `tocs` lacks some codes the timetable carries (`TW`, `QC`, `LF` on
/// 2026-10-22), and their trains must stay filterable.
async fn check_known_operators(
    app: &App,
    codes: &[String],
    service_dates: &[chrono::NaiveDate],
) -> Result<(), (StatusCode, String)> {
    let known = crate::data::reference::known_operator_codes(&app.database, codes, service_dates)
        .await
        .map_err(internal_error)?;
    let unknown: Vec<&str> = codes
        .iter()
        .filter(|code| !known.contains(*code))
        .map(String::as_str)
        .collect();
    match unknown.as_slice() {
        [] => Ok(()),
        [code] => Err((
            StatusCode::BAD_REQUEST,
            format!("operator {code} is not a known train operator code"),
        )),
        codes => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "operators {} are not known train operator codes",
                codes.join(", ")
            ),
        )),
    }
}

/// Parses and bounds the page size.
pub(crate) fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
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
/// `"HH:MM:SS|train_uid"`, plus `"|day_offset"` for a row after midnight
/// (2026-10-22), plus `"|p"` for a row of the previous service date (an
/// `across_midnight` search's, whose day offset is always 1 or more). No `origin_crs`: the station
/// being searched is the fixed equality filter for the whole query,
/// not a value that varies within one page, so it carries no ordering
/// information and doesn't belong in the cursor.
///
/// Base64 makes the value visibly OPAQUE, matching this crate's established
/// posture for other cursors in this codebase. Not signed: the cursor names
/// a public timetable row on an unauthenticated route.
pub(crate) fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
    let base = format!(
        "{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid
    );
    // The working day offset, as a third part, only when it is not 0
    // (2026-10-22): a same-day cursor reads exactly as before.
    if cursor.previous_service_date {
        URL_SAFE_NO_PAD.encode(format!("{base}|{}|p", cursor.day_offset))
    } else if cursor.day_offset == 0 {
        URL_SAFE_NO_PAD.encode(base)
    } else {
        URL_SAFE_NO_PAD.encode(format!("{base}|{}", cursor.day_offset))
    }
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently ignored -- ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row.
/// A previous-service-date cursor is one too: only the train search
/// writes them (see `decode_search_cursor`).
pub(crate) fn decode_cursor(
    raw: &str,
) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
    decode_search_cursor(raw, false)
}

/// [`decode_cursor`], also reading a previous-service-date cursor when
/// `across_midnight` (the search that wrote it asked for those rows).
fn decode_search_cursor(
    raw: &str,
    across_midnight: bool,
) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let mut parts: Vec<&str> = decoded.split('|').collect();
    let previous_service_date = parts.len() == 4 && parts.last() == Some(&"p");
    if previous_service_date {
        if !across_midnight {
            return Err(invalid());
        }
        parts.pop();
    }
    let (scheduled, train_uid, day_offset) = match parts.as_slice() {
        [scheduled, train_uid] => (*scheduled, *train_uid, 0),
        [scheduled, train_uid, day_offset] => (
            *scheduled,
            *train_uid,
            day_offset
                .parse::<i16>()
                .ok()
                .filter(|offset| (1..=7).contains(offset))
                .ok_or_else(invalid)?,
        ),
        _ => return Err(invalid()),
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(CallingPointDepartureCursor {
        day_offset,
        scheduled,
        train_uid: (*train_uid).to_string(),
        previous_service_date,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
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
    let stops_at = params
        .stops_at
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("stops_at", s))
        .transpose()?;
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
    // no calling point to arrive AT is ambiguous input, not a wider search,
    // so this 400s rather than quietly acting as though neither bound was
    // set.
    if (arrival_from_time.is_some() || arrival_to_time.is_some()) && stops_at.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "arrival_from and arrival_to require stops_at to be set".to_string(),
        ));
    }
    let day_offsets = WindowDayOffsets {
        from: parse_day_offset(
            "from_day_offset",
            &params.from_day_offset,
            "from",
            from_time,
        )?,
        to: parse_day_offset("to_day_offset", &params.to_day_offset, "to", to_time)?,
        arrival_from: parse_day_offset(
            "arrival_from_day_offset",
            &params.arrival_from_day_offset,
            "arrival_from",
            arrival_from_time,
        )?,
        arrival_to: parse_day_offset(
            "arrival_to_day_offset",
            &params.arrival_to_day_offset,
            "arrival_to",
            arrival_to_time,
        )?,
    };
    let operators = parse_operators(params.operator.as_deref())?;
    let across_midnight = parse_across_midnight(params.across_midnight.as_deref())?;
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|raw| decode_search_cursor(raw, across_midnight))
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
    let london_now = super::london_now();
    let today = london_now.date_naive();
    let now = london_now.time();

    let service_date = match params.date.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => {
            let date = parse_date(raw)?;
            check_search_date(&app, date, today).await?;
            date
        }
        None => today,
    };
    // The service dates whose trains this search returns: `date`, and with
    // `across_midnight` the day before.
    let previous_date = service_date.pred_opt().filter(|_| across_midnight);
    let service_dates: Vec<chrono::NaiveDate> =
        previous_date.into_iter().chain([service_date]).collect();
    if let Some(codes) = &operators {
        check_known_operators(&app, codes, &service_dates).await?;
    }

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
    let default_from = if service_date == today {
        now
    } else {
        chrono::NaiveTime::MIN
    };
    let window = resolve_window(
        WindowTimes {
            from: from_time,
            to: to_time,
            arrival_from: arrival_from_time,
            arrival_to: arrival_to_time,
        },
        day_offsets,
        default_from,
    );

    let search = queries::CallingPointSearch {
        station_crs: &station,
        service_date,
        depart_from: window.from.minutes(),
        depart_to: window.to.map(WindowBound::minutes),
        true_origin_crs: origin.as_deref(),
        stops_at: stops_at.as_deref(),
        arrive_from: window.arrival_from.map(WindowBound::minutes),
        arrive_to: window.arrival_to.map(WindowBound::minutes),
        operators: operators.as_deref(),
        across_midnight,
        after: after.as_ref(),
        limit,
    };
    let Some(page) = queries::search_schedule_calling_point_departures(&app.database, &search)
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

    // Batched destination-name enrichment, closing the gap
    // `render::calling_point_departure_json`'s own doc comment now
    // documents: `TrainSearchForm.tsx` used to render a bare
    // `destinationCrs` code with no name, unlike every other departure
    // board in this crate (`station_departure_json`/`schedule_departure_json`
    // both already resolve one). Same batched-lookup shape
    // `routes::departures::get_station_departures` already established:
    // collect every row's `destination_crs`, resolve them all in one query,
    // hand the map to the renderer.
    // The same one lookup also names each row's true origin (`originName`,
    // 2026-10-07): the origin codes simply join the destination codes.
    let destination_crs: Vec<String> = page
        .departures
        .iter()
        .flat_map(|d| {
            ["destination_crs", "true_origin_crs"]
                .into_iter()
                .filter_map(|key| d.get(key).and_then(Value::as_str))
        })
        .map(str::to_string)
        .collect();
    let destination_names = queries::station_names_for_crs_batch(&app.database, &destination_crs)
        .await
        .map_err(internal_error)?;

    let mut results: Vec<Value> = page
        .departures
        .iter()
        .map(|row| {
            let mut rendered = calling_point_departure_json(row, &station, &destination_names);
            // `dayOffset`: days after the row's service date the departure
            // from `station` falls on, the same convention as
            // `/schedule-departures`' own `dayOffset` -- it moves the
            // departure's calendar day, never the service date, so the
            // row's `GET /Train/by-uid/{uid}/{date}` stays its
            // `serviceDate` (the searched date, or with `across_midnight`
            // the day before). It is `scheduled`'s (the public departure,
            // else the working one); `workingDayOffset` is
            // `workingDeparture`'s.
            if let Some(object) = rendered.as_object_mut() {
                object.insert(
                    "serviceDate".to_string(),
                    row.get("service_date")
                        .cloned()
                        .unwrap_or_else(|| json!(service_date)),
                );
                let working_day_offset = row.get("day_offset").and_then(Value::as_u64).unwrap_or(0);
                let (_, day_offset) = crate::render::public_else_working(
                    row,
                    "public_departure",
                    "scheduled",
                    working_day_offset,
                );
                object.insert("dayOffset".to_string(), Value::from(day_offset));
                object.insert(
                    "workingDayOffset".to_string(),
                    Value::from(working_day_offset),
                );
                if stops_at.is_some() {
                    insert_stops_at_arrival(object, row);
                }
            }
            rendered
        })
        .collect();
    crate::routes::schedule_rows::attach_origin_names(&mut results, &destination_names);
    let results = annotate_by_service_date(&app, results, service_date, previous_date).await;
    let mut body = json!({
        "results": results,
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
        // Additive (2026-10-22): the train day searched, and the time
        // window as applied -- `from` filled in with the `now` default,
        // every day offset resolved.
        "serviceDate": service_date,
        "window": window.to_json(),
        // Additive (2026-10-22): whether the previous date's late trains
        // were asked for, and every service date the rows can be of.
        "acrossMidnight": across_midnight,
        "serviceDates": service_dates,
    });
    // Additive (2026-10-08): whether `service_date`'s timetable is still
    // provisional -- once per response, since every row shares the date.
    if let Some(object) = body.as_object_mut() {
        crate::routes::provisional::TimetableCertainty::for_date(service_date, today)
            .insert_into(object);
    }
    Ok(Json(body))
}

/// `?across_midnight=`: absent, empty or `false` is off, `true` on (any
/// case); anything else is a `400`, as `/Trips/plan`'s `acrossMidnight`.
fn parse_across_midnight(raw: Option<&str>) -> Result<bool, (StatusCode, String)> {
    match raw
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "false" => Ok(false),
        "true" => Ok(true),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("across_midnight must be true or false, not '{other}'"),
        )),
    }
}

/// Attaches each row's live state and service mode, both read per service
/// date: the rows of `previous_date` (an `across_midnight` search's, their
/// `serviceDate` the day before) against that date, the rest against
/// `service_date`. Row order is kept.
async fn annotate_by_service_date(
    app: &App,
    results: Vec<Value>,
    service_date: chrono::NaiveDate,
    previous_date: Option<chrono::NaiveDate>,
) -> Vec<Value> {
    let previous_key = previous_date.map(|date| json!(date));
    let is_previous = |row: &Value| {
        previous_key
            .as_ref()
            .is_some_and(|key| row.get("serviceDate") == Some(key))
    };
    let flags: Vec<bool> = results.iter().map(is_previous).collect();
    let (mut previous_rows, mut own_rows): (Vec<Value>, Vec<Value>) =
        results.into_iter().partition(is_previous);
    for (date, rows) in [
        (Some(service_date), &mut own_rows),
        (previous_date, &mut previous_rows),
    ] {
        let Some(date) = date else { continue };
        if rows.is_empty() {
            continue;
        }
        crate::routes::schedule_rows::attach_live(&app.database, date, rows).await;
        crate::data::schedule_services::annotate_uid_rows(&app.database, date, rows).await;
    }
    let mut own = own_rows.into_iter();
    let mut previous = previous_rows.into_iter();
    flags
        .into_iter()
        .filter_map(|flag| if flag { previous.next() } else { own.next() })
        .collect()
}

/// The four `stopsAt*` fields of a search row, present only when the
/// search set `stops_at`: the arrival at the call `stops_at` matched on
/// (the earliest one the filter accepts -- see
/// `queries::search_schedule_calling_point_departures`).
///
/// * `stopsAtArrival`: the public (GBTT) arrival, `"HH:MM"`.
/// * `stopsAtArrivalDayOffset`: days after the service date (the searched
///   `date`) that `stopsAtArrival` falls on.
/// * `stopsAtWorkingArrival`/`stopsAtWorkingArrivalDayOffset`: the same for
///   the working (WTT) arrival. `arrival_from`/`arrival_to` compare the
///   public arrival, else this one (2026-10-22).
///
/// A time and its offset are `null` together when the call has no such
/// time stored (a public time before the schedule's next publish).
fn insert_stops_at_arrival(object: &mut serde_json::Map<String, Value>, row: &Value) {
    let hh_mm = |key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .map_or(Value::Null, |s| Value::String(s.chars().take(5).collect()))
    };
    let offset = |key: &str| row.get(key).cloned().unwrap_or(Value::Null);
    object.insert("stopsAtArrival".to_string(), hh_mm("stops_at_arrival"));
    object.insert(
        "stopsAtArrivalDayOffset".to_string(),
        offset("stops_at_arrival_day_offset"),
    );
    object.insert(
        "stopsAtWorkingArrival".to_string(),
        hh_mm("stops_at_working_arrival"),
    );
    object.insert(
        "stopsAtWorkingArrivalDayOffset".to_string(),
        offset("stops_at_working_arrival_day_offset"),
    );
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(&err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn at(h: u32, m: u32, day_offset: u32) -> WindowBound {
        WindowBound {
            time: hm(h, m),
            day_offset,
        }
    }

    #[test]
    fn a_to_earlier_than_an_explicit_from_runs_past_midnight() {
        let window = resolve_window(
            WindowTimes {
                from: Some(hm(22, 0)),
                to: Some(hm(1, 30)),
                ..WindowTimes::default()
            },
            WindowDayOffsets::default(),
            hm(0, 0),
        );
        assert_eq!(window.from, at(22, 0, 0));
        assert_eq!(window.to, Some(at(1, 30, 1)));
        assert_eq!(window.to.unwrap().minutes(), 1530);
    }

    #[test]
    fn the_now_default_never_moves_to_onto_the_next_day() {
        let window = resolve_window(
            WindowTimes {
                to: Some(hm(9, 0)),
                ..WindowTimes::default()
            },
            WindowDayOffsets::default(),
            hm(15, 0),
        );
        assert_eq!(window.from, at(15, 0, 0));
        assert_eq!(window.to, Some(at(9, 0, 0)));
    }

    #[test]
    fn explicit_day_offsets_win() {
        let window = resolve_window(
            WindowTimes {
                from: Some(hm(0, 15)),
                to: Some(hm(0, 10)),
                ..WindowTimes::default()
            },
            WindowDayOffsets {
                from: Some(1),
                to: Some(0),
                ..WindowDayOffsets::default()
            },
            hm(0, 0),
        );
        assert_eq!(window.from, at(0, 15, 1));
        assert_eq!(window.to, Some(at(0, 10, 0)));
    }

    #[test]
    fn arrival_to_wraps_against_arrival_from_else_an_explicit_from() {
        let times = WindowTimes {
            from: Some(hm(23, 0)),
            arrival_to: Some(hm(0, 45)),
            ..WindowTimes::default()
        };
        let window = resolve_window(times, WindowDayOffsets::default(), hm(0, 0));
        assert_eq!(window.arrival_from, None);
        assert_eq!(window.arrival_to, Some(at(0, 45, 1)));

        let window = resolve_window(
            WindowTimes {
                arrival_from: Some(hm(0, 30)),
                ..times
            },
            WindowDayOffsets::default(),
            hm(0, 0),
        );
        assert_eq!(window.arrival_from, Some(at(0, 30, 0)));
        assert_eq!(window.arrival_to, Some(at(0, 45, 0)));
    }

    #[test]
    fn operators_are_trimmed_uppercased_and_deduplicated() {
        assert_eq!(parse_operators(None), Ok(None));
        assert_eq!(parse_operators(Some(" , ,")), Ok(None));
        assert_eq!(
            parse_operators(Some("sw, GW,,Sw")),
            Ok(Some(vec!["SW".to_string(), "GW".to_string()]))
        );
        assert!(parse_operators(Some("SW,SWR")).is_err());
    }

    #[test]
    fn a_cursor_keeps_its_day_offset_and_an_old_one_reads_as_day_zero() {
        let cursor = CallingPointDepartureCursor {
            day_offset: 1,
            scheduled: hm(0, 30),
            train_uid: "C12345".to_string(),
            previous_service_date: false,
        };
        assert_eq!(decode_cursor(&encode_cursor(&cursor)), Ok(cursor));

        let same_day = CallingPointDepartureCursor {
            day_offset: 0,
            scheduled: hm(9, 0),
            train_uid: "C12345".to_string(),
            previous_service_date: false,
        };
        let encoded = encode_cursor(&same_day);
        assert_eq!(encoded, URL_SAFE_NO_PAD.encode("09:00:00|C12345"));
        assert_eq!(decode_cursor(&encoded), Ok(same_day));

        assert!(decode_cursor(&URL_SAFE_NO_PAD.encode("00:30:00|C12345|x")).is_err());
        assert!(decode_cursor(&URL_SAFE_NO_PAD.encode("00:30:00|C12345|0")).is_err());
    }

    #[test]
    fn a_previous_service_date_cursor_reads_back_only_across_midnight() {
        let cursor = CallingPointDepartureCursor {
            day_offset: 1,
            scheduled: hm(0, 40),
            train_uid: "C12345".to_string(),
            previous_service_date: true,
        };
        let encoded = encode_cursor(&cursor);
        assert_eq!(encoded, URL_SAFE_NO_PAD.encode("00:40:00|C12345|1|p"));
        assert_eq!(decode_search_cursor(&encoded, true), Ok(cursor));
        // A search without `across_midnight`, and the leg candidates,
        // never wrote one.
        assert!(decode_search_cursor(&encoded, false).is_err());
        assert!(decode_cursor(&encoded).is_err());
        // Its day offset is still 1 or more.
        assert!(
            decode_search_cursor(&URL_SAFE_NO_PAD.encode("00:40:00|C12345|0|p"), true).is_err()
        );
        assert!(
            decode_search_cursor(&URL_SAFE_NO_PAD.encode("00:40:00|C12345|1|q"), true).is_err()
        );
    }

    #[test]
    fn across_midnight_is_true_or_false() {
        assert_eq!(parse_across_midnight(None), Ok(false));
        assert_eq!(parse_across_midnight(Some(" ")), Ok(false));
        assert_eq!(parse_across_midnight(Some("False")), Ok(false));
        assert_eq!(parse_across_midnight(Some("TRUE")), Ok(true));
        assert!(parse_across_midnight(Some("1")).is_err());
    }
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
            migration_database_url: None,
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            transit_networks: Vec::new(),
            admin_group: String::new(),
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
            metrics_port: 9091,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            background_loops: true,
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
        };

        std::sync::Arc::new(AppState {
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
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
            delegated: std::sync::Arc::default(),
            tfl_arrivals: std::sync::Arc::default(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    /// The instant every test in this module runs at: noon on a BST day in
    /// 2099, via [`crate::routes::pin_london_now_for_tests`] (DB review
    /// 2026-09-27 B3/B4). Pinning it means
    ///
    /// * no test depends on the wall clock any more -- the old
    ///   `relative_times` had to skip every test between 22:30 and 00:01
    ///   London, and the UTC-vs-London gap test could only assert during
    ///   BST;
    /// * "today" and every date in the route's +-7 day window are far-future,
    ///   synthetic days this module owns outright, so the day-scoped deletes
    ///   below (the route's "is anything published for this date?" check
    ///   means a test must control the whole day) can never touch real data.
    ///
    /// No other test module uses August 2099.
    fn pinned_now() -> chrono::DateTime<chrono::Utc> {
        "2099-08-14T11:00:00Z"
            .parse()
            .expect("valid pinned instant (12:00 BST)")
    }

    /// Held for the whole test: the clock pin, plus a cleanup of this
    /// module's fixture window and `RSLV*` rows that runs before the test
    /// seeds anything and again on drop, pass or fail.
    struct TestGuards {
        _clock: crate::routes::PinnedLondonNow,
        _cleanup: crate::test_support::FixtureCleanup,
    }

    async fn connect() -> (PgPool, TestGuards) {
        let clock = crate::routes::pin_london_now_for_tests(pinned_now());
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");
        let today = crate::routes::london_today();
        // Out to today+40: the published-range tests seed a date beyond
        // the static window (`FAR_DAYS`).
        let (first, last) = (
            today - chrono::Duration::days(8),
            today + chrono::Duration::days(40),
        );
        crate::test_support::assert_synthetic_date(first);
        let cleanup = crate::test_support::FixtureCleanup::new(
            &pool,
            [
                format!(
                    "DELETE FROM schedule_destination_departures \
                     WHERE service_date BETWEEN '{first}' AND '{last}'"
                ),
                "DELETE FROM schedule_destination_departures WHERE train_uid LIKE 'RSLV%'"
                    .to_string(),
                "DELETE FROM trains WHERE train_uid LIKE 'RSLV%'".to_string(),
            ],
        )
        .await;
        (
            pool,
            TestGuards {
                _clock: clock,
                _cleanup: cleanup,
            },
        )
    }

    /// Deletes every `schedule_destination_departures` row on each of
    /// `dates` -- only ever a synthetic (2050+) fixture day, asserted.
    async fn delete_days(pool: &PgPool, dates: &[chrono::NaiveDate]) {
        for &date in dates {
            crate::test_support::assert_synthetic_date(date);
            sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
                .bind(date)
                .execute(pool)
                .await
                .expect("cleanup fixture-day schedule_destination_departures rows");
        }
    }

    async fn delete_today(pool: &PgPool) {
        delete_days(pool, &[crate::routes::london_today()]).await;
    }

    /// `(past, soon, later)` relative to the pinned London `now` (12:00):
    /// 00:00, 12:30 and 13:00, with plenty of room after `later` for the
    /// callers that build stops a little past it.
    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        let now = crate::routes::london_now().time();
        let past = chrono::NaiveTime::MIN;
        let soon = now + chrono::Duration::minutes(30);
        let later = now + chrono::Duration::minutes(60);
        assert!(
            past < now && now < soon && soon < later,
            "the pinned clock must leave room either side of now: {now}"
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
        let today = crate::routes::london_today();
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
        let router: axum::Router = Router::new()
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
        let (status, _) = get(&pool, "/trains/search?station=ZRB&arrival_to=09:00").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_arrival_from_is_a_400() {
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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

    /// Each row carries `serviceMode`/`liveTracking` from
    /// `schedule_services`; a uid with no row there is a train.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rows_carry_the_service_mode() {
        let (pool, _guards) = connect().await;
        seed_today(&pool, "ZRB").await;
        let today = crate::routes::london_today();
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, train_status, \
                train_category, stp) VALUES ($1, 'C10002', 'ferry', 'S', NULL, 'P') \
             ON CONFLICT (service_date, uid) DO UPDATE SET mode = EXCLUDED.mode",
        )
        .bind(today)
        .execute(&pool)
        .await
        .expect("seed schedule_services");

        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(rows[0]["serviceMode"], "train");
        assert_eq!(rows[0]["liveTracking"], true);
        assert_eq!(rows[1]["uid"], "C10002");
        assert_eq!(rows[1]["serviceMode"], "ferry");
        assert_eq!(rows[1]["liveTracking"], false);

        sqlx::query("DELETE FROM schedule_services WHERE service_date = $1 AND uid = 'C10002'")
            .bind(today)
            .execute(&pool)
            .await
            .ok();
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        let (pool, _guards) = connect().await;
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
        // `C10000` at `NaiveTime::MIN` (00:00:00), already-departed at the
        // pinned noon clock, so an explicit `from=00:00&to=00:00`
        // window can only return it if the floor is gone.
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
    /// membership check from a plain equality on `station`:
    ///
    /// * `T51001` calls `ZRB`, `AAA`, `BBB` AND `CCC`.
    /// * `T51002` calls `ZRB`, `AAA` and `BBB`, but NOT `CCC`.
    /// * `T51003` calls `ZRB` and `AAA` only, from a DIFFERENT true origin
    ///   (`PAD`, not `SWA`) -- lets a `stops_at` test double as an
    ///   `origin`-independence check without a second fixture.
    ///
    /// Every row's `destination_crs` is `EEE` -- none of `AAA`/`BBB`/`CCC`
    /// is any of these schedules' TRUE destination, which is the load-
    /// bearing fact `trains_search_stops_at_matches_regardless_of_true_destination`
    /// exists to exploit: the deleted `destination` filter could never have
    /// matched any of these trains on `AAA`, but `stops_at` does.
    async fn seed_stops_at(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = crate::routes::london_today();
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
    async fn trains_search_stops_at_matches_regardless_of_true_destination() {
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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

    /// Three schedules out of the same station, one circular, one not, and
    /// one that passes back through and carries on. Times are `soon`
    /// (= now + 30m) plus a multiple of five minutes.
    ///
    /// * `T53001` -- the loop. Departs `station_crs` at +0, calls `KNG`,
    ///   and TERMINATES back at `station_crs` (an arrival-only calling
    ///   point, which is why it gets no row of its own and lives solely in
    ///   `destination_crs`). The shape of a Kingston Loop working.
    /// * `T53002` -- the control. Departs `station_crs` at +5, calls
    ///   `KNG`, and terminates at `EEE`; it never comes back.
    /// * `T53003` -- the through-loop. Departs `station_crs` at +20, calls
    ///   `KNG`, departs `station_crs` AGAIN at +30 and terminates at
    ///   `EEE`. Only the FIRST of its two departures comes back.
    async fn seed_loop(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = crate::routes::london_today();
        let (_, soon, _) = relative_times();
        let five = chrono::Duration::minutes(5);
        for (train_uid, destination_crs, calls) in [
            ("T53001", station_crs, vec![(station_crs, 0), ("KNG", 2)]),
            ("T53002", "EEE", vec![(station_crs, 1), ("KNG", 3)]),
            (
                "T53003",
                "EEE",
                vec![(station_crs, 4), ("KNG", 5), (station_crs, 6)],
            ),
        ] {
            for (origin_crs, offset) in calls {
                sqlx::query(
                    "INSERT INTO schedule_destination_departures \
                        (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(today)
                .bind(destination_crs)
                .bind(soon + five * offset)
                .bind(train_uid)
                .bind(origin_crs)
                .bind(station_crs)
                .execute(pool)
                .await
                .expect("seed loop fixture row");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_station_and_stops_at_naming_one_station_finds_loop_services() {
        // End-to-end over the wire for the loop fix: the same CRS in both
        // fields used to be a tautology (every train out of that station
        // matched, because a row is a member of its own calling-point
        // list) and now asks "does this working come back here".
        let (pool, _guards) = connect().await;
        seed_loop(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&stops_at=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: Vec<&str> = rows
            .iter()
            .map(|row| row["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            vec!["T53001", "T53003"],
            "the circular working, and the through-loop's FIRST departure only; T53002 departs \
             ZRB but never returns, and T53003's second departure does not either: {rows:?}"
        );
        assert_eq!(
            rows[0]["destinationCrs"], "ZRB",
            "the loop's return call IS its terminus -- matched through destination_crs, which \
             is the only place an arrival-only calling point exists in this table"
        );
        assert_eq!(
            rows[1]["destinationCrs"], "EEE",
            "the through-loop matched on a later ZRB DEPARTURE instead, and still terminates \
             somewhere else entirely"
        );

        // The same ordering rule now applies to a DIFFERENT stops_at
        // station too (2026-09-22): all three trains call at KNG, but
        // T53003's SECOND ZRB departure (offset 6) calls KNG EARLIER
        // (offset 5) -- it already passed through KNG before this
        // departure, so it is no longer reachable from it and must be
        // excluded, leaving three matches instead of four.
        let (status, body) = get(&pool, "/trains/search?station=ZRB&stops_at=KNG").await;
        assert_eq!(status, StatusCode::OK);
        let mut uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        uids.sort();
        assert_eq!(
            uids,
            vec![
                "T53001".to_string(),
                "T53002".to_string(),
                "T53003".to_string(),
            ],
            "T53003's SECOND ZRB departure is excluded: its only KNG call is BEFORE it, so \
             the later-than rule now applies to every stops_at station, not just the \
             same-station loop case"
        );

        delete_today(&pool).await;
    }

    /// Two schedules built to isolate the 2026-09-22 reversal on its own,
    /// away from `seed_loop`'s same-station loop case: `T54001` calls `FOO`
    /// BEFORE `station_crs`, so it already passed through `FOO` before
    /// ever reaching the search origin; `T54002` calls `station_crs` first
    /// and `FOO` after, so it remains reachable from there. Neither
    /// train's true destination is `FOO`.
    async fn seed_stops_at_ordering(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = crate::routes::london_today();
        let (_, soon, _) = relative_times();
        let five = chrono::Duration::minutes(5);
        for (train_uid, calling_points) in [
            ("T54001", vec![("FOO", 0), (station_crs, 1)]),
            ("T54002", vec![(station_crs, 0), ("FOO", 1)]),
        ] {
            for (origin_crs, offset) in calling_points {
                sqlx::query(
                    "INSERT INTO schedule_destination_departures \
                        (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(today)
                .bind("EEE")
                .bind(soon + five * offset)
                .bind(train_uid)
                .bind(origin_crs)
                .bind(station_crs)
                .execute(pool)
                .await
                .expect("seed stops_at ordering fixture row");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stops_at_a_different_station_excludes_a_call_before_the_search_origin() {
        // The 2026-09-22 reversal, isolated from the same-station loop case
        // `trains_search_station_and_stops_at_naming_one_station_finds_loop_services`
        // already covers: `stops_at` naming a DIFFERENT station from
        // `station` now also requires that station's call to come LATER --
        // "stops at X" means you can actually get there from where you
        // searched, not that the train passed through X at some point.
        let (pool, _guards) = connect().await;
        seed_stops_at_ordering(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&stops_at=FOO").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: Vec<&str> = rows
            .iter()
            .map(|row| row["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            vec!["T54002"],
            "T54001 called FOO BEFORE ever reaching ZRB and is now excluded; T54002 calls FOO \
             AFTER ZRB and still matches: {rows:?}"
        );

        delete_today(&pool).await;
    }

    /// Two schedules sharing the required station `ZRB`, both also calling
    /// at the INTERMEDIATE point `OXF` (neither's true destination -- both
    /// terminate at `BHM`) with DIFFERENT arrivals at `OXF` itself -- so
    /// only `arrival_from`/`arrival_to` scoped to `OXF`'s own
    /// `calling_point_arrival`, never `scheduled`/`to` (station-scoped) nor
    /// the schedule-level `destination_arrival` (BHM-scoped), can tell them
    /// apart. `OXF`'s own `scheduled` (its departure, a two-minute dwell
    /// after its arrival -- not the arrival itself) is always later than
    /// `ZRB`'s: two DIFFERENT calling points booked at the literal same
    /// instant never happens in real CIF timetables, and the `stops_at`
    /// ordering rule (`(day_offset, scheduled) >`) requires OXF's row to
    /// genuinely follow ZRB's to be reachable from it at all.
    async fn seed_stops_at_arrival(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = crate::routes::london_today();
        let (_, soon, later) = relative_times();
        let dwell = chrono::Duration::minutes(2);
        for (train_uid, oxf_arrival) in [("T52001", soon), ("T52002", later)] {
            let oxf_scheduled = oxf_arrival + dwell;
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
            .bind(oxf_scheduled)
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
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

        crate::routes::london_now().offset().fix().local_minus_utc() != 0
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_inside_the_utc_vs_london_gap() {
        let (pool, _guards) = connect().await;
        delete_today(&pool).await;

        // The pinned clock is in BST, so London is an hour ahead of UTC and
        // there is a real gap to put the fixture in -- on every run, not
        // only during British Summer Time as it was on the wall clock.
        assert!(
            london_is_currently_ahead_of_utc(),
            "pinned_now() must be a BST instant for this test to discriminate anything"
        );
        // ONE London clock read for both the seeded date and the seeded
        // time, and the same `london_now` the route itself reads.
        let london_now = crate::routes::london_now();
        let today = london_now.date_naive();
        let london_time = {
            use chrono::Timelike;

            let t = london_now.time();
            chrono::NaiveTime::from_hms_opt(t.hour(), t.minute(), 0)
                .expect("valid time from valid hour/minute")
        };
        // 20 minutes before London `now` (so already departed), but 40
        // minutes AFTER UTC `now` -- a route that regressed to bare UTC time
        // would still list it.
        let gap_time = london_time - chrono::Duration::minutes(20);

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

        assert!(
            !uids.contains(&"C10099".to_string()),
            "a row scheduled 20 minutes before the correct London-local `now` has already \
             departed and must be excluded; if this fails, `now` has regressed to bare UTC \
             time: {uids:?}"
        );

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_date_is_a_400() {
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
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
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let edge_future = today + chrono::Duration::days(7);
        let edge_past = today - chrono::Duration::days(7);

        delete_days(&pool, &[edge_future, edge_past]).await;

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

        delete_days(&pool, &[edge_future, edge_past]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_now_forward_only_when_date_is_today() {
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let tomorrow = today + chrono::Duration::days(1);

        delete_days(&pool, &[tomorrow]).await;

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

        delete_days(&pool, &[tomorrow]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_uses_from_as_a_plain_bound_with_no_now_floor_on_a_non_today_date() {
        // Coverage for an explicit `from` (`resolve_window`) on a
        // non-today date specifically --
        // `trains_search_applies_now_forward_only_when_date_is_today` above
        // only exercises the `None` arm (no `from` supplied at all) for a
        // non-today date. This test supplies `from` explicitly alongside a
        // non-today `date` and proves it is used as a PLAIN inclusive lower
        // bound with NO `now` floor applied at all, per this route's own
        // doc comment on `default_from` in `get_trains_search` -- today's
        // clock is irrelevant to every OTHER date, explicit `from` or not.
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let tomorrow = today + chrono::Duration::days(1);

        delete_days(&pool, &[tomorrow]).await;

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

        delete_days(&pool, &[tomorrow]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_omitting_date_still_defaults_to_today() {
        let (pool, _guards) = connect().await;
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
        // and the `arrival_from`/`arrival_to`-without-`stops_at`
        // 400 above) -- this test extends the same posture to malformed
        // (unrecognized) parameter NAMES.
        let (pool, _guards) = connect().await;
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
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let target = today + chrono::Duration::days(3);
        delete_days(&pool, &[target]).await;

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

    // --- dates beyond the static window ---------------------------------------

    /// How far past today the published-range tests seed: well beyond
    /// `SEARCH_WINDOW_FORWARD_DAYS`, inside `connect`'s cleanup range.
    const FAR_DAYS: i64 = 30;

    async fn seed_one_departure(pool: &PgPool, date: chrono::NaiveDate, uid: &str) {
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs) \
             VALUES ($1, 'WAT', '09:00', $2, 'ZRB')",
        )
        .bind(date)
        .bind(uid)
        .execute(pool)
        .await
        .expect("seed a far-date fixture row");
    }

    /// The published range as the route sees it, so the expected text does
    /// not depend on what else the shared database holds.
    async fn published_range(pool: &PgPool) -> (chrono::NaiveDate, chrono::NaiveDate) {
        queries::schedule_destination_departures_date_range(pool)
            .await
            .expect("date range")
            .expect("a published range")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_accepts_a_published_date_beyond_the_static_window() {
        let (pool, _guards) = connect().await;
        let far = crate::routes::london_today() + chrono::Duration::days(FAR_DAYS);
        delete_days(&pool, &[far]).await;
        seed_one_departure(&pool, far, "C60001").await;

        let (status, body) = get(&pool, &format!("/trains/search?station=ZRB&date={far}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C60001");

        // The discovery route reports the same range.
        let (status, body) = get(&pool, "/trains/search/dates").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json: Value = serde_json::from_str(&body).unwrap();
        let (earliest, latest) = published_range(&pool).await;
        assert_eq!(latest, far);
        assert_eq!(json["to"], far.to_string());
        assert_eq!(json["publishedTo"], far.to_string());
        assert_eq!(json["publishedFrom"], earliest.to_string());

        delete_days(&pool, &[far]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_date_outside_the_published_range_is_a_400_naming_it() {
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let far = today + chrono::Duration::days(FAR_DAYS);
        delete_days(&pool, &[far]).await;
        seed_one_departure(&pool, far, "C60002").await;
        let (earliest, latest) = published_range(&pool).await;
        assert_eq!(latest, far);
        let from = earliest.min(today - chrono::Duration::days(SEARCH_WINDOW_BACKWARD_DAYS));

        let beyond = far + chrono::Duration::days(1);
        let (status, body) = get(&pool, &format!("/trains/search?station=ZRB&date={beyond}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            format!(
                "date must be between {from} and {far}: schedule data is published for \
                 {earliest} to {far}"
            )
        );

        // A date inside the static window is still accepted (and 404s when
        // it has no rows), whatever the published range.
        let (status, _) = get(
            &pool,
            &format!(
                "/trains/search?station=ZRB&date={}",
                today + chrono::Duration::days(SEARCH_WINDOW_FORWARD_DAYS)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        delete_days(&pool, &[far]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_marks_a_date_past_the_provisional_horizon_provisional() {
        let (pool, _guards) = connect().await;
        let today = crate::routes::london_today();
        let horizon = crate::routes::provisional::DEFAULT_PROVISIONAL_AFTER_DAYS;
        let firm = today + chrono::Duration::days(horizon);
        let provisional = firm + chrono::Duration::days(1);
        let far = today + chrono::Duration::days(FAR_DAYS);
        delete_days(&pool, &[firm, provisional, far]).await;
        seed_one_departure(&pool, firm, "C60011").await;
        seed_one_departure(&pool, provisional, "C60012").await;
        seed_one_departure(&pool, far, "C60013").await;

        for (date, expected) in [(firm, false), (provisional, true), (far, true)] {
            let (status, body) =
                get(&pool, &format!("/trains/search?station=ZRB&date={date}")).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            let json: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(json["provisional"], expected, "{date}: {json}");
            assert_eq!(json["provisionalFrom"], provisional.to_string(), "{json}");
            assert_eq!(json["results"].as_array().map(Vec::len), Some(1), "{json}");
        }

        let (status, body) = get(&pool, "/trains/search/dates").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["provisionalFrom"], provisional.to_string(), "{json}");

        delete_days(&pool, &[firm, provisional, far]).await;
    }

    #[test]
    fn searchable_range_widens_the_static_window_to_the_published_one() {
        let today = chrono::NaiveDate::from_ymd_opt(2099, 8, 14).unwrap();
        let day = |n: i64| today + chrono::Duration::days(n);
        assert_eq!(searchable_range(today, None), (day(-7), day(7)));
        assert_eq!(
            searchable_range(today, Some((day(-8), day(60)))),
            (day(-8), day(60))
        );
        // A published range inside the window never narrows it.
        assert_eq!(
            searchable_range(today, Some((day(-1), day(3)))),
            (day(-7), day(7))
        );
        assert_eq!(
            out_of_range_message(day(-7), day(7), None),
            "date must be within 7 days ago and 7 days from today: no schedule data is published"
        );
    }

    // --- stopsAt* arrival fields ----------------------------------------------

    /// One `schedule_destination_departures` row with every column the
    /// `stopsAt*` fields read.
    struct Call {
        date: chrono::NaiveDate,
        uid: &'static str,
        crs: &'static str,
        scheduled: chrono::NaiveTime,
        day_offset: i16,
        arrival: Option<chrono::NaiveTime>,
        public_arrival: Option<chrono::NaiveTime>,
        destination_crs: &'static str,
        destination_arrival: chrono::NaiveTime,
        destination_arrival_day_offset: i16,
        public_destination_arrival: chrono::NaiveTime,
    }

    async fn insert_call(pool: &PgPool, call: &Call) {
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                 day_offset, calling_point_arrival, public_calling_point_arrival, \
                 destination_arrival, destination_arrival_day_offset, public_destination_arrival) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(call.date)
        .bind(call.destination_crs)
        .bind(call.scheduled)
        .bind(call.uid)
        .bind(call.crs)
        .bind(call.day_offset)
        .bind(call.arrival)
        .bind(call.public_arrival)
        .bind(call.destination_arrival)
        .bind(call.destination_arrival_day_offset)
        .bind(call.public_destination_arrival)
        .execute(pool)
        .await
        .expect("seed a stopsAt fixture call");
    }

    fn t(h: u32, m: u32, s: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, s).unwrap()
    }

    /// A fixture call: `(crs, departure, day offset, working arrival,
    /// public arrival)`.
    type StopsAtSeedStop = (
        &'static str,
        chrono::NaiveTime,
        i16,
        Option<chrono::NaiveTime>,
        Option<chrono::NaiveTime>,
    );
    /// A fixture train: `(uid, destination, destination arrival, its day
    /// offset, public destination arrival, calls)`.
    type StopsAtSeedTrain = (
        &'static str,
        &'static str,
        chrono::NaiveTime,
        i16,
        chrono::NaiveTime,
        Vec<StopsAtSeedStop>,
    );

    /// Seeds tomorrow (no `now` floor) with four trains out of `ZQA`, each
    /// call given as `(crs, departure, day offset, working arrival, public
    /// arrival)`:
    ///
    /// * `T54001`: `ZQB` intermediate, then terminates at `ZQD`.
    /// * `T54002`: leaves `ZQA` at 23:50 and reaches `ZQB` after midnight.
    /// * `T54003`: arrives `ZQC` before midnight (23:58 working, 23:59
    ///   public) and departs after it; arrives `ZQE` at 23:59:30 working,
    ///   rounded to a 00:00 public arrival the next day.
    /// * `T54004`: calls at `ZQB` twice, at 10:10 and 10:40.
    #[expect(
        clippy::too_many_lines,
        reason = "four literal fixture trains, one call per line"
    )]
    async fn seed_stops_at_arrivals(pool: &PgPool) -> chrono::NaiveDate {
        let date = crate::routes::london_today() + chrono::Duration::days(1);
        delete_days(pool, &[date]).await;
        let trains: [StopsAtSeedTrain; 4] = [
            (
                "T54001",
                "ZQD",
                t(10, 30, 0),
                0,
                t(10, 31, 0),
                vec![
                    ("ZQA", t(10, 0, 0), 0, None, None),
                    (
                        "ZQB",
                        t(10, 12, 0),
                        0,
                        Some(t(10, 10, 30)),
                        Some(t(10, 11, 0)),
                    ),
                ],
            ),
            (
                "T54002",
                "ZQD",
                t(0, 50, 0),
                1,
                t(0, 50, 0),
                vec![
                    ("ZQA", t(23, 50, 0), 0, None, None),
                    ("ZQB", t(0, 20, 0), 1, Some(t(0, 18, 0)), Some(t(0, 19, 0))),
                ],
            ),
            (
                "T54003",
                "ZQD",
                t(1, 0, 0),
                1,
                t(1, 0, 0),
                vec![
                    ("ZQA", t(23, 40, 0), 0, None, None),
                    ("ZQC", t(0, 1, 0), 1, Some(t(23, 58, 0)), Some(t(23, 59, 0))),
                    ("ZQE", t(0, 2, 0), 1, Some(t(23, 59, 30)), Some(t(0, 0, 0))),
                ],
            ),
            (
                "T54004",
                "ZQD",
                t(11, 0, 0),
                0,
                t(11, 0, 0),
                vec![
                    ("ZQA", t(9, 50, 0), 0, None, None),
                    (
                        "ZQB",
                        t(10, 11, 0),
                        0,
                        Some(t(10, 10, 0)),
                        Some(t(10, 10, 0)),
                    ),
                    (
                        "ZQX",
                        t(10, 25, 0),
                        0,
                        Some(t(10, 24, 0)),
                        Some(t(10, 24, 0)),
                    ),
                    (
                        "ZQB",
                        t(10, 41, 0),
                        0,
                        Some(t(10, 40, 0)),
                        Some(t(10, 40, 0)),
                    ),
                ],
            ),
        ];
        for (uid, destination_crs, destination_arrival, destination_offset, public_dest, stops) in
            trains
        {
            for (crs, scheduled, day_offset, arrival, public_arrival) in stops {
                insert_call(
                    pool,
                    &Call {
                        date,
                        uid,
                        crs,
                        scheduled,
                        day_offset,
                        arrival,
                        public_arrival,
                        destination_crs,
                        destination_arrival,
                        destination_arrival_day_offset: destination_offset,
                        public_destination_arrival: public_dest,
                    },
                )
                .await;
            }
        }
        date
    }

    async fn stops_at_rows(pool: &PgPool, query: &str) -> Vec<Value> {
        let (status, body) = get(pool, &format!("/trains/search?station=ZQA&{query}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        results(&body)
    }

    fn row<'a>(rows: &'a [Value], uid: &str) -> &'a Value {
        rows.iter()
            .find(|r| r["uid"] == uid)
            .unwrap_or_else(|| panic!("no {uid} row in {rows:?}"))
    }

    fn stops_at_fields(row: &Value) -> Value {
        serde_json::json!({
            "stopsAtArrival": row["stopsAtArrival"],
            "stopsAtArrivalDayOffset": row["stopsAtArrivalDayOffset"],
            "stopsAtWorkingArrival": row["stopsAtWorkingArrival"],
            "stopsAtWorkingArrivalDayOffset": row["stopsAtWorkingArrivalDayOffset"],
        })
    }

    fn arrival(public: &str, public_offset: i64, working: &str, working_offset: i64) -> Value {
        serde_json::json!({
            "stopsAtArrival": public,
            "stopsAtArrivalDayOffset": public_offset,
            "stopsAtWorkingArrival": working,
            "stopsAtWorkingArrivalDayOffset": working_offset,
        })
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_stops_at_rows_carry_the_arrival_at_that_stop() {
        let (pool, _guards) = connect().await;
        let date = seed_stops_at_arrivals(&pool).await;

        // An intermediate stop, not the destination: its own arrival, not
        // `destinationArrival`'s.
        let rows = stops_at_rows(&pool, &format!("date={date}&stops_at=ZQB")).await;
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        assert_eq!(uids, ["T54004", "T54001", "T54002"]);
        let t54001 = row(&rows, "T54001");
        assert_eq!(stops_at_fields(t54001), arrival("10:11", 0, "10:10", 0));
        // The destination's public arrival (2026-10-20), its working one
        // alongside.
        assert_eq!(t54001["destinationArrival"], "10:31");
        assert_eq!(t54001["workingDestinationArrival"], "10:30");

        // After midnight: the next day.
        assert_eq!(
            stops_at_fields(row(&rows, "T54002")),
            arrival("00:19", 1, "00:18", 1)
        );

        // Calls twice: the earliest call the filter accepts...
        assert_eq!(
            stops_at_fields(row(&rows, "T54004")),
            arrival("10:10", 0, "10:10", 0)
        );
        // ...which an arrival bound moves to the later call.
        let rows = stops_at_rows(
            &pool,
            &format!("date={date}&stops_at=ZQB&arrival_from=10:30"),
        )
        .await;
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        // T54002's 00:19 arrival is the next morning, so after 10:30 in
        // train-day minutes (2026-10-22; a wall-clock bound dropped it).
        assert_eq!(uids, ["T54004", "T54002"]);
        assert_eq!(stops_at_fields(&rows[0]), arrival("10:40", 0, "10:40", 0));

        // The true terminus: its destination arrival.
        let rows = stops_at_rows(&pool, &format!("date={date}&stops_at=ZQD")).await;
        assert_eq!(rows.len(), 4);
        assert_eq!(
            stops_at_fields(row(&rows, "T54001")),
            arrival("10:31", 0, "10:30", 0)
        );
        assert_eq!(
            stops_at_fields(row(&rows, "T54002")),
            arrival("00:50", 1, "00:50", 1)
        );

        // Arrived before midnight, departed after it: the arrival is dated
        // the day before its departure.
        let rows = stops_at_rows(&pool, &format!("date={date}&stops_at=ZQC")).await;
        assert_eq!(
            stops_at_fields(row(&rows, "T54003")),
            arrival("23:59", 0, "23:58", 0)
        );
        // A 23:59H working arrival rounded to a 00:00 public one: the public
        // time is the next day.
        let rows = stops_at_rows(&pool, &format!("date={date}&stops_at=ZQE")).await;
        assert_eq!(
            stops_at_fields(row(&rows, "T54003")),
            arrival("00:00", 1, "23:59", 0)
        );

        delete_days(&pool, &[date]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_without_stops_at_has_no_stops_at_fields() {
        let (pool, _guards) = connect().await;
        let date = seed_stops_at_arrivals(&pool).await;

        let rows = stops_at_rows(&pool, &format!("date={date}")).await;
        assert_eq!(rows.len(), 4);
        for row in &rows {
            let object = row.as_object().unwrap();
            assert!(
                object.keys().all(|key| !key.starts_with("stopsAt")),
                "{row}"
            );
        }

        delete_days(&pool, &[date]).await;
    }

    // --- Operator filter and the train day (2026-10-22) ------------------------

    /// One departure-bearing row for the operator and train-day tests:
    /// `(uid, working departure, its day offset, public departure,
    /// operator)` at `station` on `date`.
    async fn insert_departure(
        pool: &PgPool,
        date: chrono::NaiveDate,
        station: &str,
        row: (
            &str,
            chrono::NaiveTime,
            i16,
            Option<chrono::NaiveTime>,
            Option<&str>,
        ),
    ) {
        let (uid, scheduled, day_offset, public_departure, operator) = row;
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                 day_offset, public_departure, operator_atoc) \
             VALUES ($1, 'WAT', $2, $3, $4, $5, $6, $7)",
        )
        .bind(date)
        .bind(scheduled)
        .bind(uid)
        .bind(station)
        .bind(day_offset)
        .bind(public_departure)
        .bind(operator)
        .execute(pool)
        .await
        .expect("seed a departure");
    }

    fn uids(body: &str) -> Vec<String> {
        results(body)
            .iter()
            .map(|r| r["uid"].as_str().unwrap().to_string())
            .collect()
    }

    /// The fixture operators: two known codes, and two that are not.
    const FIXTURE_TOCS: [(&str, &str); 2] = [("Q3", "Fixture Rail"), ("Q4", "Fixture Trains")];
    const UNKNOWN_TOCS: [&str; 2] = ["Q5", "Q6"];

    async fn set_fixture_tocs(pool: &PgPool, present: bool) {
        for code in FIXTURE_TOCS
            .iter()
            .map(|(code, _)| *code)
            .chain(UNKNOWN_TOCS)
        {
            sqlx::query("DELETE FROM tocs WHERE atoc_code = $1")
                .bind(code)
                .execute(pool)
                .await
                .expect("clear a fixture toc");
        }
        if present {
            for (code, name) in FIXTURE_TOCS {
                sqlx::query("INSERT INTO tocs (atoc_code, name, legal_name) VALUES ($1, $2, $2)")
                    .bind(code)
                    .bind(name)
                    .execute(pool)
                    .await
                    .expect("seed a fixture toc");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_operator_filter_keeps_the_named_operators_and_400s_an_unknown_one() {
        let (pool, _guards) = connect().await;
        set_fixture_tocs(&pool, true).await;
        let date = crate::routes::london_today() + chrono::Duration::days(2);
        delete_days(&pool, &[date]).await;
        for row in [
            ("OPQ301", t(9, 0, 0), 0, None, Some("Q3")),
            ("OPQ401", t(10, 0, 0), 0, None, Some("Q4")),
            ("OPNONE", t(11, 0, 0), 0, None, None),
            ("OPQ302", t(12, 0, 0), 0, None, Some("Q3")),
        ] {
            insert_departure(&pool, date, "ZOF", row).await;
        }
        let search = |query: &str| format!("/trains/search?station=ZOF&date={date}&{query}");

        // One operator.
        let (status, body) = get(&pool, &search("operator=Q3")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["OPQ301", "OPQ302"]);

        // Several, any of them: trimmed, uppercased, repeats and empty
        // entries ignored.
        let (status, body) = get(&pool, &search("operator=q3,%20Q4,,Q3")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["OPQ301", "OPQ401", "OPQ302"]);

        // Nothing but separators is no filter at all.
        let (status, body) = get(&pool, &search("operator=,%20,")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body).len(), 4);

        // An unknown code is a 400 naming it, not an empty result.
        let (status, body) = get(&pool, &search("operator=Q3,Q5")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "operator Q5 is not a known train operator code");
        let (status, body) = get(&pool, &search("operator=Q5,q6")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "operators Q5, Q6 are not known train operator codes");

        // A malformed one too.
        let (status, body) = get(&pool, &search("operator=Q33")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("operator"), "{body}");

        // The plural is a typo, not a second filter.
        let (status, _) = get(&pool, &search("operators=Q3")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        set_fixture_tocs(&pool, false).await;
        delete_days(&pool, &[date]).await;
    }

    /// Two consecutive fixture service dates at `ZMN`: `date`'s evening and
    /// after-midnight departures, and `date + 1`'s own early ones.
    async fn seed_train_day(pool: &PgPool) -> (chrono::NaiveDate, chrono::NaiveDate) {
        let date = crate::routes::london_today() + chrono::Duration::days(2);
        let next = date + chrono::Duration::days(1);
        delete_days(pool, &[date, next]).await;
        for row in [
            // Public 22:00, working 21:59H: shown, and searched, at 22:00.
            ("MNPB01", t(21, 59, 30), 0, Some(t(22, 0, 0)), None),
            ("MNEV01", t(22, 30, 0), 0, Some(t(22, 30, 0)), None),
            // `date`'s own trains, departing after its midnight.
            ("MNPM01", t(0, 30, 0), 1, Some(t(0, 30, 0)), None),
            ("MNLT01", t(1, 45, 0), 1, None, None),
        ] {
            insert_departure(pool, date, "ZMN", row).await;
        }
        for row in [
            ("MNEA01", t(0, 20, 0), 0, None, None),
            ("MNMO01", t(6, 0, 0), 0, None, None),
        ] {
            insert_departure(pool, next, "ZMN", row).await;
        }
        (date, next)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_covers_the_whole_train_day_past_midnight() {
        let (pool, _guards) = connect().await;
        let (date, next) = seed_train_day(&pool).await;
        let search = |query: &str| format!("/trains/search?station=ZMN&date={date}&{query}");

        // The whole train day, in train-day order: after-midnight last.
        let (status, body) = get(&pool, &search("from=00:00")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["MNPB01", "MNEV01", "MNPM01", "MNLT01"]);
        let rows = results(&body);
        assert_eq!(rows[2]["scheduled"], "00:30");
        assert_eq!(rows[2]["dayOffset"], 1);
        assert_eq!(rows[1]["dayOffset"], 0);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["serviceDate"], date.to_string());

        // A window across midnight: `to` earlier than `from` is the next
        // morning, and the echo says so.
        let (status, body) = get(&pool, &search("from=22:00&to=01:30")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["MNPB01", "MNEV01", "MNPM01"]);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["window"],
            serde_json::json!({
                "from": {"time": "22:00", "dayOffset": 0},
                "to": {"time": "01:30", "dayOffset": 1},
                "arrivalFrom": null,
                "arrivalTo": null,
            })
        );

        // The same window, spelled out.
        let (_, explicit) = get(&pool, &search("from=22:00&to=01:30&to_day_offset=1")).await;
        assert_eq!(uids(&explicit), uids(&body));

        // An explicit offset wins, even one that leaves nothing.
        let (status, body) = get(&pool, &search("from=22:00&to=01:30&to_day_offset=0")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(uids(&body).is_empty());

        // A window that starts after midnight.
        let (status, body) = get(&pool, &search("from=00:15&from_day_offset=1")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["MNPM01", "MNLT01"]);

        // Compared on the public time: 21:59H working is 22:00 public.
        let (_, body) = get(&pool, &search("from=22:00&to=22:00")).await;
        assert_eq!(uids(&body), ["MNPB01"]);
        let (_, body) = get(&pool, &search("from=21:00&to=21:59")).await;
        assert!(uids(&body).is_empty());

        // Pages run across midnight, one row at a time.
        let mut seen = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..6 {
            let query = match &after {
                Some(cursor) => search(&format!("from=21:00&limit=1&after={cursor}")),
                None => search("from=21:00&limit=1"),
            };
            let (status, body) = get(&pool, &query).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            seen.extend(uids(&body));
            after = next_cursor(&body);
            if after.is_none() {
                break;
            }
        }
        assert_eq!(seen, ["MNPB01", "MNEV01", "MNPM01", "MNLT01"]);

        // The next date's search is ITS train day: its own 00:20 service,
        // never `date`'s 00:30 one.
        let (status, body) = get(
            &pool,
            &format!("/trains/search?station=ZMN&date={next}&from=00:00"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["MNEA01", "MNMO01"]);

        delete_days(&pool, &[date, next]).await;
    }

    /// One `schedule_services` row: `uid` on `date`, run by `operator`.
    async fn insert_service(pool: &PgPool, date: chrono::NaiveDate, uid: &str, operator: &str) {
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
             VALUES ($1, $2, 'train', 'P', $3)",
        )
        .bind(date)
        .bind(uid)
        .bind(operator)
        .execute(pool)
        .await
        .expect("seed schedule_services");
    }

    async fn delete_services(pool: &PgPool, dates: &[chrono::NaiveDate]) {
        for &date in dates {
            crate::test_support::assert_synthetic_date(date);
            sqlx::query("DELETE FROM schedule_services WHERE service_date = $1")
                .bind(date)
                .execute(pool)
                .await
                .expect("cleanup fixture-day schedule_services rows");
        }
    }

    /// A code `tocs` lacks is still filterable when the timetable runs a
    /// train under it on the searched date (2026-10-22): from
    /// `schedule_services`, or, for a date it holds nothing for, from the
    /// departures themselves.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_operator_filter_accepts_a_timetable_only_code() {
        let (pool, _guards) = connect().await;
        set_fixture_tocs(&pool, true).await;
        let date = crate::routes::london_today() + chrono::Duration::days(2);
        let other = date + chrono::Duration::days(1);
        delete_days(&pool, &[date, other]).await;
        delete_services(&pool, &[date, other]).await;
        for row in [
            ("OPQ301", t(9, 0, 0), 0, None, Some("Q3")),
            ("OPQ701", t(10, 0, 0), 0, None, Some("Q7")),
            ("OPQ801", t(11, 0, 0), 0, None, Some("Q8")),
        ] {
            insert_departure(&pool, date, "ZOF", row).await;
        }
        insert_departure(
            &pool,
            other,
            "ZOF",
            ("OPQ302", t(9, 0, 0), 0, None, Some("Q3")),
        )
        .await;
        let search = |on: chrono::NaiveDate, query: &str| {
            format!("/trains/search?station=ZOF&date={on}&{query}")
        };

        // No `schedule_services` rows for `date`: the departures say Q7 runs.
        let (status, body) = get(&pool, &search(date, "operator=Q7")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["OPQ701"]);

        // With them, they do; a `tocs` code mixes in.
        insert_service(&pool, date, "OPQ701", "Q7").await;
        insert_service(&pool, date, "OPQ801", "Q8").await;
        let (status, body) = get(&pool, &search(date, "operator=q8,Q3,Q7")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["OPQ301", "OPQ701", "OPQ801"]);

        // A code neither knows is still a 400.
        let (status, body) = get(&pool, &search(date, "operator=Q7,Q5")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "operator Q5 is not a known train operator code");

        // Known is per searched date: no Q7 train runs on `other`.
        let (status, body) = get(&pool, &search(other, "operator=Q7")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "operator Q7 is not a known train operator code");

        // The operator list: `tocs` named, the timetable's extras by code.
        let (status, body) = get(&pool, "/trains/search/operators").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json: Value = serde_json::from_str(&body).unwrap();
        let operators = json["operators"].as_array().unwrap();
        for expected in [
            serde_json::json!({"code": "Q3", "name": "Fixture Rail"}),
            serde_json::json!({"code": "Q7", "name": null}),
            serde_json::json!({"code": "Q8", "name": null}),
        ] {
            assert!(operators.contains(&expected), "{expected} in {body}");
        }
        let codes: Vec<&str> = operators
            .iter()
            .map(|o| o["code"].as_str().unwrap())
            .collect();
        assert!(codes.windows(2).all(|pair| pair[0] < pair[1]), "{codes:?}");

        delete_services(&pool, &[date, other]).await;
        set_fixture_tocs(&pool, false).await;
        delete_days(&pool, &[date, other]).await;
    }

    /// `across_midnight` (2026-10-22): at 00:30 London, a search with the
    /// date left to default also finds yesterday's 00:40 train, labelled
    /// with its own service date; an explicit date without the param stays
    /// that date's own trains.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_across_midnight_adds_yesterdays_late_trains() {
        let (pool, _guards) = connect().await;
        // 00:30 BST on the day after the pinned date.
        let _clock = crate::routes::pin_london_now_for_tests(
            "2099-08-14T23:30:00Z".parse().expect("valid instant"),
        );
        let today = crate::routes::london_today();
        let yesterday = today.pred_opt().unwrap();
        assert_eq!(crate::routes::london_now().time(), t(0, 30, 0));
        delete_days(&pool, &[yesterday, today]).await;
        for row in [
            ("AMPV23", t(23, 0, 0), 0, Some(t(23, 0, 0)), None),
            // Left before now.
            ("AMPV20", t(0, 20, 0), 1, Some(t(0, 20, 0)), None),
            ("AMPV40", t(0, 40, 0), 1, Some(t(0, 40, 0)), None),
            ("AMPV55", t(0, 55, 0), 1, None, None),
        ] {
            insert_departure(&pool, yesterday, "ZAM", row).await;
        }
        for row in [
            ("AMTD45", t(0, 45, 0), 0, None, None),
            ("AMTD06", t(6, 0, 0), 0, None, None),
        ] {
            insert_departure(&pool, today, "ZAM", row).await;
        }

        // The date defaults to today; `/trains` asks for yesterday's late
        // trains too. Merged in time order, each row with its own date.
        let (status, body) = get(&pool, "/trains/search?station=ZAM&across_midnight=true").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["AMPV40", "AMTD45", "AMPV55", "AMTD06"]);
        let rows = results(&body);
        assert_eq!(rows[0]["serviceDate"], yesterday.to_string());
        assert_eq!(rows[0]["scheduled"], "00:40");
        assert_eq!(rows[0]["dayOffset"], 1);
        assert_eq!(rows[1]["serviceDate"], today.to_string());
        assert_eq!(rows[1]["dayOffset"], 0);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["serviceDate"], today.to_string());
        assert_eq!(json["acrossMidnight"], true);
        assert_eq!(
            json["serviceDates"],
            serde_json::json!([yesterday.to_string(), today.to_string()])
        );

        // Page by page, the cursor crossing between the two dates.
        let mut seen = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..6 {
            let query = match &after {
                Some(cursor) => {
                    format!(
                        "/trains/search?station=ZAM&across_midnight=true&limit=1&after={cursor}"
                    )
                }
                None => "/trains/search?station=ZAM&across_midnight=true&limit=1".to_string(),
            };
            let (status, body) = get(&pool, &query).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            seen.extend(uids(&body));
            after = next_cursor(&body);
            if after.is_none() {
                break;
            }
        }
        assert_eq!(seen, ["AMPV40", "AMTD45", "AMPV55", "AMTD06"]);

        // An explicit date is that date's own trains only.
        let (status, body) = get(&pool, &format!("/trains/search?station=ZAM&date={today}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(uids(&body), ["AMTD45", "AMTD06"]);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["acrossMidnight"], false);
        assert_eq!(json["serviceDates"], serde_json::json!([today.to_string()]));
        assert!(
            results(&body)
                .iter()
                .all(|row| row["serviceDate"] == today.to_string())
        );

        // As is a defaulted date without the param (the API's default).
        let (_, body) = get(&pool, "/trains/search?station=ZAM").await;
        assert_eq!(uids(&body), ["AMTD45", "AMTD06"]);

        // An explicit date that asks for more gets it.
        let (_, body) = get(
            &pool,
            &format!("/trains/search?station=ZAM&date={today}&across_midnight=true&to=00:50"),
        )
        .await;
        assert_eq!(uids(&body), ["AMPV40", "AMTD45"]);

        // A previous-date cursor means nothing to a search without it.
        let cursor = URL_SAFE_NO_PAD.encode("00:40:00|AMPV40|1|p");
        let (status, _) = get(&pool, &format!("/trains/search?station=ZAM&after={cursor}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?station=ZAM&across_midnight=yes").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "across_midnight must be true or false, not 'yes'");

        delete_days(&pool, &[yesterday, today]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_malformed_or_unanchored_day_offset() {
        let (pool, _guards) = connect().await;
        for (query, expected) in [
            (
                "from=00:30&from_day_offset=2",
                "from_day_offset must be 0 or 1, not '2'",
            ),
            ("to_day_offset=1", "to_day_offset needs to"),
            (
                "stops_at=WAT&arrival_to_day_offset=1",
                "arrival_to_day_offset needs arrival_to",
            ),
            (
                "stops_at=WAT&arrival_from=01:00&arrival_from_day_offset=yes",
                "arrival_from_day_offset must be 0 or 1, not 'yes'",
            ),
        ] {
            let (status, body) = get(&pool, &format!("/trains/search?station=ZRB&{query}")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
            assert_eq!(body, expected, "{query}");
        }
    }

    // --- GET /trains/resolve --------------------------------------------------
    //
    // Fixture rows use made-up `ZR*` station codes and `RSLV*` uids, on
    // today+4 (and today+3 for the overnight case) of the pinned clock.

    fn resolve_day() -> chrono::NaiveDate {
        crate::routes::london_today() + chrono::Duration::days(4)
    }

    fn hm(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    async fn clear_resolve_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid LIKE 'RSLV%'")
            .execute(pool)
            .await
            .expect("cleanup resolve fixture rows");
        sqlx::query("DELETE FROM trains WHERE train_uid LIKE 'RSLV%'")
            .execute(pool)
            .await
            .expect("cleanup resolve fixture trains");
    }

    /// One departure row. `arrivals` is `(calling_point_arrival,
    /// destination_arrival)`.
    #[expect(
        clippy::too_many_arguments,
        reason = "test fixture: one positional argument per column it seeds"
    )]
    async fn seed_resolve_row(
        pool: &PgPool,
        service_date: chrono::NaiveDate,
        uid: &str,
        station: &str,
        destination: &str,
        scheduled: chrono::NaiveTime,
        day_offset: i16,
        rsid: Option<&str>,
        operator: Option<&str>,
        arrivals: (Option<chrono::NaiveTime>, Option<chrono::NaiveTime>),
    ) {
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, day_offset, train_uid, origin_crs, \
                 calling_point_arrival, destination_arrival, operator_atoc, rsid) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(service_date)
        .bind(destination)
        .bind(scheduled)
        .bind(day_offset)
        .bind(uid)
        .bind(station)
        .bind(arrivals.0)
        .bind(arrivals.1)
        .bind(operator)
        .bind(rsid)
        .execute(pool)
        .await
        .expect("seed resolve fixture row");
    }

    async fn resolve_json(pool: &PgPool, query: &str) -> (StatusCode, String) {
        get(pool, &format!("/trains/resolve?{query}")).await
    }

    fn resolved(body: &str) -> Value {
        serde_json::from_str(body).unwrap_or_else(|_| panic!("not JSON: {body}"))
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_exact_rsid_picks_its_train_over_a_same_time_one() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        let none = (None, None);
        seed_resolve_row(
            &pool,
            d,
            "RSLVA1",
            "ZRA",
            "ZRB",
            hm(10, 0),
            0,
            Some("SR408800"),
            Some("SR"),
            none,
        )
        .await;
        seed_resolve_row(
            &pool,
            d,
            "RSLVB1",
            "ZRA",
            "ZRB",
            hm(10, 0),
            0,
            Some("SR999900"),
            Some("SR"),
            none,
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=zra&date={d}&time=10:01&rsid=sr408800"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            resolved(&body),
            json!({
                "trainUid": "RSLVA1",
                "serviceDate": d.to_string(),
                "matchedOn": "rsid",
                "href": format!("/Train/by-uid/RSLVA1/{d}"),
            })
        );
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_falls_back_to_the_six_character_rsid_prefix() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        seed_resolve_row(
            &pool,
            d,
            "RSLVP1",
            "ZRA",
            "ZRB",
            hm(11, 0),
            0,
            Some("SE123401"),
            Some("SE"),
            (None, None),
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=11:00&rsid=SE123400"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json = resolved(&body);
        assert_eq!(json["trainUid"], "RSLVP1");
        assert_eq!(json["matchedOn"], "rsidPrefix");
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_split_portions_are_a_409_until_destination_breaks_the_tie() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        let none = (None, None);
        seed_resolve_row(
            &pool,
            d,
            "RSLVS1",
            "ZRA",
            "ZRC",
            hm(12, 0),
            0,
            Some("SE777701"),
            Some("SE"),
            none,
        )
        .await;
        seed_resolve_row(
            &pool,
            d,
            "RSLVS2",
            "ZRA",
            "ZRD",
            hm(12, 0),
            0,
            Some("SE777702"),
            Some("SE"),
            none,
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=12:00&rsid=SE777700"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body.contains(&format!("RSLVS1/{d}")) && body.contains(&format!("RSLVS2/{d}")),
            "the 409 lists every candidate: {body}"
        );

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=12:00&rsid=SE777700&destination=zrd"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(resolved(&body)["trainUid"], "RSLVS2");
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_after_midnight_finds_the_previous_service_date() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        let previous = d - chrono::Duration::days(1);
        seed_resolve_row(
            &pool,
            previous,
            "RSLVN1",
            "ZRA",
            "ZRB",
            hm(0, 20),
            1,
            Some("XC000100"),
            Some("XC"),
            (None, None),
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=00:20&rsid=XC000100"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json = resolved(&body);
        assert_eq!(json["trainUid"], "RSLVN1");
        assert_eq!(json["serviceDate"], previous.to_string());
        assert_eq!(json["href"], format!("/Train/by-uid/RSLVN1/{previous}"));

        // The same row is NOT a match for 00:20 on its own service date.
        let (status, _) = resolve_json(
            &pool,
            &format!("station=ZRA&date={previous}&time=00:20&rsid=XC000100"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_without_an_rsid_uses_the_timetable_heuristic() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        let none = (None, None);
        seed_resolve_row(
            &pool,
            d,
            "RSLVT1",
            "ZRA",
            "ZRB",
            hm(13, 0),
            0,
            None,
            Some("GW"),
            none,
        )
        .await;
        seed_resolve_row(
            &pool,
            d,
            "RSLVT2",
            "ZRA",
            "ZRE",
            hm(13, 1),
            0,
            None,
            Some("GW"),
            none,
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=13:00&destination=ZRB&operator=gw"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json = resolved(&body);
        assert_eq!(json["trainUid"], "RSLVT1");
        assert_eq!(json["matchedOn"], "timetable");

        // Rows published before rsid existed: a board rsid still resolves
        // through the timetable fallback.
        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=13:00&rsid=GW100000&destination=ZRE"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(resolved(&body)["trainUid"], "RSLVT2");

        let (status, body) = resolve_json(&pool, &format!("station=ZRA&date={d}&time=13:00")).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "time alone is ambiguous here: {body}"
        );
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_nothing_matching_is_a_404() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        seed_resolve_row(
            &pool,
            d,
            "RSLVZ1",
            "ZRA",
            "ZRB",
            hm(14, 0),
            0,
            Some("GW200000"),
            Some("GW"),
            (None, None),
        )
        .await;

        for query in [
            format!("station=ZRA&date={d}&time=15:00"),
            format!("station=ZRA&date={d}&time=14:00&rsid=VT999900"),
            format!("station=ZRQ&date={d}&time=14:00&rsid=GW200000"),
        ] {
            let (status, body) = resolve_json(&pool, &query).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{query}: {body}");
        }
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_rejects_malformed_input_naming_the_field() {
        let (pool, _guards) = connect().await;
        let d = resolve_day();
        let far = d + chrono::Duration::days(30);
        for (query, field) in [
            (format!("date={d}&time=10:00"), "station"),
            ("station=ZRA&time=10:00".to_string(), "date"),
            (format!("station=ZRA&date={d}"), "time"),
            (format!("station=ZR1&date={d}&time=10:00"), "station"),
            ("station=ZRA&date=tomorrow&time=10:00".to_string(), "date"),
            (format!("station=ZRA&date={far}&time=10:00"), "date"),
            (format!("station=ZRA&date={d}&time=25:00"), "time"),
            (format!("station=ZRA&date={d}&time=10:00&rsid=SR4"), "rsid"),
            (
                format!("station=ZRA&date={d}&time=10:00&rsid=SR40-800"),
                "rsid",
            ),
            (
                format!("station=ZRA&date={d}&time=10:00&destination=Z"),
                "destination",
            ),
            (
                format!("station=ZRA&date={d}&time=10:00&operator=SRX"),
                "operator",
            ),
            (format!("station=ZRA&date={d}&time=10:00&kind=pass"), "kind"),
            (format!("station=ZRA&date={d}&time=10:00&uid=C00001"), "uid"),
        ] {
            let (status, body) = resolve_json(&pool, &query).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
            assert!(
                body.contains(field),
                "{query}: the 400 names `{field}`: {body}"
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_arrival_matches_the_terminus_and_intermediate_arrivals() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        // RSLVR1 calls at ZRA (arr 17:58, dep 18:00) and terminates at ZRT
        // at 18:30; ZRT has no row of its own.
        seed_resolve_row(
            &pool,
            d,
            "RSLVR1",
            "ZRA",
            "ZRT",
            hm(18, 0),
            0,
            Some("GW555500"),
            Some("GW"),
            (Some(hm(17, 58)), Some(hm(18, 30))),
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRT&date={d}&time=18:30&rsid=GW555500&kind=arrival"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(resolved(&body)["trainUid"], "RSLVR1");

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=17:58&kind=arrival&destination=ZRT"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(resolved(&body)["matchedOn"], "timetable");

        // A terminus has no departure to resolve.
        let (status, _) = resolve_json(
            &pool,
            &format!("station=ZRT&date={d}&time=18:30&rsid=GW555500"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        clear_resolve_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_resolve -- --ignored --test-threads=1`"]
    async fn trains_resolve_href_is_accepted_by_train_by_uid() {
        let (pool, _guards) = connect().await;
        clear_resolve_fixtures(&pool).await;
        let d = resolve_day();
        seed_resolve_row(
            &pool,
            d,
            "RSLVH1",
            "ZRA",
            "ZRB",
            hm(19, 0),
            0,
            Some("LM300000"),
            Some("LM"),
            (None, None),
        )
        .await;

        let (status, body) = resolve_json(
            &pool,
            &format!("station=ZRA&date={d}&time=19:00&rsid=LM300000"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let href = resolved(&body)["href"].as_str().unwrap().to_string();

        let router: axum::Router = Router::new()
            .merge(crate::routes::train::router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(Request::builder().uri(&href).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status, StatusCode::OK, "{href}: {body}");
        assert_eq!(body["trainUid"], "RSLVH1");
        assert_eq!(body["serviceDate"], d.to_string());
        clear_resolve_fixtures(&pool).await;
    }

    /// Removes the `SRLV*` live-state fixtures and their origin station.
    /// `train_current_state` first: its `trains_id` is `ON DELETE SET
    /// NULL`, so deleting the train alone would orphan the state row.
    async fn clear_live_fixtures(pool: &PgPool) {
        for sql in [
            "DELETE FROM train_current_state WHERE trains_id IN \
                (SELECT id FROM trains WHERE train_uid LIKE 'SRLV%')",
            "DELETE FROM trains WHERE train_uid LIKE 'SRLV%'",
            "DELETE FROM schedule_destination_departures WHERE train_uid LIKE 'SRLV%'",
            "DELETE FROM stations WHERE crs = 'ZSO'",
        ] {
            sqlx::query(sql)
                .execute(pool)
                .await
                .expect("clear live fixtures");
        }
    }

    /// A `trains` row with live state `status`/`delay` for `uid` on `date`.
    async fn seed_live_state(
        pool: &PgPool,
        uid: &str,
        date: chrono::NaiveDate,
        status: &str,
        delay: Option<i32>,
    ) {
        let (trains_id,): (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date, train_id) VALUES ($1, $2, $1) \
             RETURNING id",
        )
        .bind(uid)
        .bind(date)
        .fetch_one(pool)
        .await
        .expect("seed trains row");
        sqlx::query(
            "INSERT INTO train_current_state (trains_id, status, last_reported_location, \
                 delay_minutes, updated_at) VALUES ($1, $2, 'Reading', $3, NOW())",
        )
        .bind(trains_id)
        .bind(status)
        .bind(delay)
        .execute(pool)
        .await
        .expect("seed train_current_state row");
    }

    /// The 2026-10-07 additive fields: `live` (with state, cancelled, and
    /// none), the station-level `dayOffset` (a post-midnight call) and
    /// `originName`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rows_carry_live_status_day_offset_and_origin_name() {
        let (pool, _guards) = connect().await;
        clear_live_fixtures(&pool).await;
        delete_today(&pool).await;
        let today = crate::routes::london_today();
        sqlx::query("INSERT INTO stations (crs, name) VALUES ('ZSO', 'Zed Origin')")
            .execute(&pool)
            .await
            .expect("seed origin station");
        let hm = |h, m| chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap();
        // (uid, scheduled at ZSL, day_offset there, true origin)
        for (uid, scheduled, day_offset, origin) in [
            ("SRLVNX", hm(0, 30), 1_i16, Some("ZSO")),
            ("SRLVLV", hm(12, 30), 0, Some("ZSO")),
            ("SRLVCX", hm(13, 0), 0, None),
            ("SRLVNO", hm(13, 30), 0, Some("ZSX")),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                     true_origin_crs, day_offset) \
                 VALUES ($1, 'WAT', $2, $3, 'ZSL', $4, $5)",
            )
            .bind(today)
            .bind(scheduled)
            .bind(uid)
            .bind(origin)
            .bind(day_offset)
            .execute(&pool)
            .await
            .expect("seed search row");
        }
        seed_live_state(&pool, "SRLVLV", today, "en_route", Some(4)).await;
        seed_live_state(&pool, "SRLVCX", today, "cancelled", None).await;
        // A live row on ANOTHER service date must not leak onto today's.
        seed_live_state(
            &pool,
            "SRLVNO",
            today - chrono::Duration::days(1),
            "en_route",
            Some(9),
        )
        .await;

        let (status, body) = get(&pool, "/trains/search?station=ZSL&from=00:00").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let rows = results(&body);
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        // Train-day order: the post-midnight call comes last.
        assert_eq!(uids, ["SRLVLV", "SRLVCX", "SRLVNO", "SRLVNX"]);

        // Post-midnight: the call is the next calendar day.
        assert_eq!(rows[3]["dayOffset"], 1, "{}", rows[3]);
        assert_eq!(rows[0]["dayOffset"], 0, "{}", rows[0]);

        // Origin name, from the true origin.
        assert_eq!(rows[3]["originCrs"], "ZSO");
        assert_eq!(rows[3]["originName"], "Zed Origin");
        assert!(rows[1]["originName"].is_null(), "no origin: {}", rows[1]);
        assert!(
            rows[2]["originName"].is_null(),
            "unknown station: {}",
            rows[2]
        );

        // Live state, the line summary's object.
        assert_eq!(
            rows[0]["live"],
            serde_json::json!({
                "status": "en_route",
                "delayMinutes": 4,
                "delayProvisional": false,
                "cancelled": false,
                "lastReportedLocation": "Reading",
            }),
        );
        assert_eq!(rows[1]["live"]["cancelled"], true, "{}", rows[1]);
        assert_eq!(rows[1]["live"]["status"], "cancelled");
        for row in [&rows[2], &rows[3]] {
            let object = row.as_object().unwrap();
            assert!(
                object.contains_key("live") && row["live"].is_null(),
                "{row}"
            );
        }

        // Paged: the fields ride the same keyset pages, into the
        // post-midnight band.
        let (_, first) = get(&pool, "/trains/search?station=ZSL&from=00:00&limit=2").await;
        assert_eq!(results(&first).len(), 2);
        let cursor = next_cursor(&first).expect("a second page");
        let (status, second) = get(
            &pool,
            &format!("/trains/search?station=ZSL&from=00:00&limit=2&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{second}");
        let second = results(&second);
        assert_eq!(second[0]["uid"], "SRLVNO");
        assert!(second[0]["live"].is_null());
        assert_eq!(second[1]["uid"], "SRLVNX");
        assert_eq!(second[1]["dayOffset"], 1);

        clear_live_fixtures(&pool).await;
    }
}
