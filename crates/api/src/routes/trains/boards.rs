//! The `/trains` page's two schedule views that are not the calling-point
//! search (2026-10-30):
//!
//! * `GET /public/trains/arrivals` -- a station's scheduled arrivals for a
//!   date and time window: every train that sets down there, its
//!   terminating trains included. The calling-point search
//!   (`get_trains_search`) only sees departures, since a terminus has no
//!   departure row.
//! * `GET /public/trains/by-headcode` -- the trains running under one
//!   headcode on a date, each as a row at its origin, for finding a
//!   specific train when you know its headcode but not where it runs.
//!
//! Both read the published timetable only (`schedule_destination_departures`,
//! and `schedule_services` for the headcode's uids), like the search, and
//! render rows through `render::calling_point_departure_json`, so a client
//! shows them with the search's own row component.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use super::{
    WindowBound, WindowDayOffsets, WindowTimes, check_known_operators, check_search_date,
    internal_error, non_empty, normalize_crs, normalize_limit, normalize_time, parse_date,
    parse_day_offset, parse_operators, resolve_window,
};
use crate::app::App;
use crate::data::queries;
use crate::render::calling_point_departure_json;

/// The most trains `by-headcode` returns. A headcode is reused across the
/// network (a `2C10` in Kent and another in Scotland), but rarely more than
/// a handful of times a day.
const MAX_HEADCODE_MATCHES: i64 = 50;

/// Query parameters of `GET /public/trains/arrivals`. Unknown names are a
/// `400`, as for the search.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArrivalsParams {
    /// Required, a 3-letter station code.
    station: String,
    /// Optional `"YYYY-MM-DD"`, the service date; today by default, and the
    /// same accepted range as the search's `date`.
    date: Option<String>,
    /// Optional `"HH:MM"` bounds on the arrival, inclusive, with
    /// `from_day_offset`/`to_day_offset` as the search has them. With no
    /// `from`, today's search starts at `now` and any other date's at
    /// midnight.
    from: Option<String>,
    to: Option<String>,
    from_day_offset: Option<String>,
    to_day_offset: Option<String>,
    /// Optional, a 3-letter station code: only trains that start there
    /// (the search's `origin`).
    origin: Option<String>,
    /// Optional comma-separated ATOC codes: only trains run by one of them
    /// (the search's `operator`, validated the same way).
    operator: Option<String>,
    /// Page size, as the search's.
    limit: Option<String>,
    /// A previous page's `nextCursor`.
    after: Option<String>,
}

/// Query parameters of `GET /public/trains/by-headcode`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeadcodeParams {
    /// Required, a 4-character headcode: a digit, a letter, two digits
    /// (`1S00`). Case-insensitive.
    headcode: String,
    /// Optional `"YYYY-MM-DD"`, as the search's `date`.
    date: Option<String>,
}

/// Validates and uppercases a headcode (the CIF Train Identity).
fn normalize_headcode(raw: &str) -> Result<String, (StatusCode, String)> {
    let code = raw.trim().to_ascii_uppercase();
    let bytes = code.as_bytes();
    let valid = bytes.len() == 4
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_alphabetic()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit();
    if valid {
        Ok(code)
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            "headcode must be a digit, a letter and two digits, like 1S00".to_string(),
        ))
    }
}

/// The service date a request names: `date`, window-checked as the
/// search's, else today.
async fn requested_date(
    app: &App,
    raw: Option<&str>,
    today: chrono::NaiveDate,
) -> Result<chrono::NaiveDate, (StatusCode, String)> {
    match raw.filter(|s| !s.trim().is_empty()) {
        Some(raw) => {
            let date = parse_date(raw)?;
            check_search_date(app, date, today).await?;
            Ok(date)
        }
        None => Ok(today),
    }
}

/// The `404` the search gives for a date with no timetable at all.
fn unpublished(service_date: chrono::NaiveDate, today: chrono::NaiveDate) -> (StatusCode, String) {
    let day = if service_date == today {
        "today".to_string()
    } else {
        service_date.format("%Y-%m-%d").to_string()
    };
    (
        StatusCode::NOT_FOUND,
        format!("no CIF-derived schedule data has been published for {day}"),
    )
}

/// One `schedule_destination_departures` row, as both views read it.
#[derive(Debug, sqlx::FromRow)]
struct BoardRow {
    train_uid: String,
    origin_crs: String,
    destination_crs: String,
    true_origin_crs: Option<String>,
    scheduled: chrono::NaiveTime,
    day_offset: i16,
    public_departure: Option<chrono::NaiveTime>,
    calling_point_arrival: Option<chrono::NaiveTime>,
    public_calling_point_arrival: Option<chrono::NaiveTime>,
    destination_arrival: Option<chrono::NaiveTime>,
    destination_arrival_day_offset: i16,
    public_destination_arrival: Option<chrono::NaiveTime>,
    operator_atoc: Option<String>,
    headcode: Option<String>,
}

const BOARD_COLUMNS: &str = "train_uid, origin_crs, destination_crs, true_origin_crs, scheduled, \
     day_offset, public_departure, calling_point_arrival, public_calling_point_arrival, \
     destination_arrival, destination_arrival_day_offset, public_destination_arrival, \
     operator_atoc, headcode";

impl BoardRow {
    /// The search's raw row shape, for `calling_point_departure_json`.
    fn raw_json(&self, service_date: chrono::NaiveDate) -> Value {
        let hms = |t: chrono::NaiveTime| t.format("%H:%M:%S").to_string();
        json!({
            "uid": self.train_uid,
            "service_date": service_date,
            "day_offset": self.day_offset,
            "destination_crs": self.destination_crs,
            "true_origin_crs": self.true_origin_crs,
            "scheduled": hms(self.scheduled),
            "destination_arrival": self.destination_arrival.map(hms),
            "destination_arrival_day_offset": self.destination_arrival_day_offset,
            "operator_atoc": self.operator_atoc,
            "headcode": self.headcode,
            "public_departure": self.public_departure.map(hms),
            "public_destination_arrival": self.public_destination_arrival.map(hms),
        })
    }

    /// This call's departure as shown (public, else working) and its day
    /// offset.
    fn shown_departure(&self) -> (chrono::NaiveTime, i64) {
        let working_offset = i64::from(self.day_offset);
        match self.public_departure {
            Some(public) => (
                public,
                nearest_day_offset(self.scheduled, working_offset, public),
            ),
            None => (self.scheduled, working_offset),
        }
    }

    /// The arrival at this row's own calling point (an intermediate call),
    /// as shown, with its day offset: the working arrival dated from the
    /// call's departure, then the public one from that. `None` when the
    /// call has no arrival (the train starts here).
    fn call_arrival(&self) -> Option<(chrono::NaiveTime, i64)> {
        let working = self.calling_point_arrival.map(|t| {
            (
                t,
                nearest_day_offset(self.scheduled, i64::from(self.day_offset), t),
            )
        });
        let reference = working.unwrap_or((self.scheduled, i64::from(self.day_offset)));
        match self.public_calling_point_arrival {
            Some(public) => Some((public, nearest_day_offset(reference.0, reference.1, public))),
            None => working,
        }
    }

    /// The arrival at the train's terminus, as shown, with its day offset.
    fn terminus_arrival(&self) -> Option<(chrono::NaiveTime, i64)> {
        let working = self
            .destination_arrival
            .map(|t| (t, i64::from(self.destination_arrival_day_offset)));
        match (self.public_destination_arrival, working) {
            (Some(public), Some((time, offset))) => {
                Some((public, nearest_day_offset(time, offset, public)))
            }
            (Some(public), None) => Some((public, i64::from(self.destination_arrival_day_offset))),
            (None, working) => working,
        }
    }
}

/// `time`'s day offset, taken the short way round midnight from
/// `reference` on day `reference_offset` (`queries::nearest_day_offset`'s
/// rule).
fn nearest_day_offset(
    reference: chrono::NaiveTime,
    reference_offset: i64,
    time: chrono::NaiveTime,
) -> i64 {
    const HALF_DAY_SECONDS: i64 = 12 * 60 * 60;
    let delta = (time - reference).num_seconds();
    if delta > HALF_DAY_SECONDS {
        (reference_offset - 1).max(0)
    } else if delta < -HALF_DAY_SECONDS {
        reference_offset + 1
    } else {
        reference_offset
    }
}

/// Train-day minutes of a time on day `day_offset`.
fn minutes(time: chrono::NaiveTime, day_offset: i64) -> i32 {
    queries::train_day_minutes(time, u32::try_from(day_offset.clamp(0, 7)).unwrap_or(0))
}

/// One arrival on the board, before rendering.
struct Arrival {
    row: BoardRow,
    time: chrono::NaiveTime,
    day_offset: i64,
    terminates: bool,
}

impl Arrival {
    fn key(&self) -> (i32, &str) {
        (
            minutes(self.time, self.day_offset),
            self.row.train_uid.as_str(),
        )
    }

    /// The raw row, the station and the arrival fields, for `render_rows`.
    fn render_parts(
        self,
        service_date: chrono::NaiveDate,
        station: &str,
    ) -> (Value, String, serde_json::Map<String, Value>) {
        let mut extra = serde_json::Map::new();
        extra.insert(
            "arrival".to_string(),
            json!(self.time.format("%H:%M").to_string()),
        );
        extra.insert("arrivalDayOffset".to_string(), json!(self.day_offset));
        extra.insert("terminates".to_string(), json!(self.terminates));
        if self.terminates {
            // The row read is the train's first call, not a departure from
            // `station`: there is none, so no `dayOffset` either.
            extra.insert("scheduled".to_string(), Value::Null);
            extra.insert("publicDeparture".to_string(), Value::Null);
            extra.insert("workingDeparture".to_string(), Value::Null);
        } else {
            let (_, day_offset) = self.row.shown_departure();
            extra.insert("dayOffset".to_string(), json!(day_offset));
        }
        (self.row.raw_json(service_date), station.to_string(), extra)
    }
}

/// What `GET /public/trains/arrivals` keeps of a station's arrivals.
struct ArrivalsFilter<'a> {
    /// The window, train-day minutes, inclusive.
    from_minutes: i32,
    to_minutes: Option<i32>,
    /// Only trains starting here.
    origin: Option<&'a str>,
    /// Only trains run by one of these.
    operators: Option<&'a [String]>,
    /// Only rows after this `(minute, uid)` (a cursor).
    after: Option<(i32, &'a str)>,
}

impl ArrivalsFilter<'_> {
    fn keeps(&self, arrival: &Arrival) -> bool {
        let key = arrival.key();
        key.0 >= self.from_minutes
            && self.to_minutes.is_none_or(|to| key.0 <= to)
            && self
                .origin
                .is_none_or(|origin| arrival.row.true_origin_crs.as_deref() == Some(origin))
            && self.operators.is_none_or(|codes| {
                arrival
                    .row
                    .operator_atoc
                    .as_ref()
                    .is_some_and(|code| codes.contains(code))
            })
            && self.after.is_none_or(|after| key > after)
    }

    /// The first `limit` arrivals kept, in order, and the cursor after the
    /// last of them when more follow.
    fn page(&self, arrivals: Vec<Arrival>, limit: usize) -> (Vec<Arrival>, Option<String>) {
        let mut page: Vec<Arrival> = arrivals
            .into_iter()
            .filter(|arrival| self.keeps(arrival))
            .take(limit.saturating_add(1))
            .collect();
        let has_more = page.len() > limit;
        page.truncate(limit);
        let next_cursor = has_more.then(|| page.last()).flatten().map(|arrival| {
            let (minute, uid) = arrival.key();
            encode_arrivals_cursor(minute, uid)
        });
        (page, next_cursor)
    }
}

/// Every arrival at `station` on `service_date`: each call there with an
/// arrival (a train that can set down), then each train that terminates
/// there. One read each: the calls over the calling-point index, the
/// termini over the primary key's `(service_date, destination_crs)`
/// prefix, one row per train (its first).
async fn station_arrivals(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    station: &str,
) -> anyhow::Result<Vec<Arrival>> {
    let calls: Vec<BoardRow> = sqlx::query_as(&format!(
        "SELECT {BOARD_COLUMNS} FROM schedule_destination_departures \
         WHERE service_date = $1 AND origin_crs = $2 \
           AND can_alight IS NOT FALSE \
           AND (calling_point_arrival IS NOT NULL OR public_calling_point_arrival IS NOT NULL)"
    ))
    .bind(service_date)
    .bind(station)
    .fetch_all(pool)
    .await?;
    let termini: Vec<BoardRow> = sqlx::query_as(&format!(
        "SELECT DISTINCT ON (train_uid) {BOARD_COLUMNS} FROM schedule_destination_departures \
         WHERE service_date = $1 AND destination_crs = $2 \
           AND (destination_arrival IS NOT NULL OR public_destination_arrival IS NOT NULL) \
         ORDER BY train_uid, day_offset, scheduled"
    ))
    .bind(service_date)
    .bind(station)
    .fetch_all(pool)
    .await?;
    let mut arrivals: Vec<Arrival> = Vec::with_capacity(calls.len() + termini.len());
    for row in calls {
        if let Some((time, day_offset)) = row.call_arrival() {
            arrivals.push(Arrival {
                row,
                time,
                day_offset,
                terminates: false,
            });
        }
    }
    for row in termini {
        if let Some((time, day_offset)) = row.terminus_arrival() {
            arrivals.push(Arrival {
                row,
                time,
                day_offset,
                terminates: true,
            });
        }
    }
    arrivals.sort_by(|a, b| a.key().cmp(&b.key()));
    Ok(arrivals)
}

/// The arrivals cursor: base64url of `"minutes|uid"`, the last row's
/// train-day minute and uid.
fn encode_arrivals_cursor(minute: i32, uid: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{minute}|{uid}"))
}

fn decode_arrivals_cursor(raw: &str) -> Result<(i32, String), (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous request".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw.trim()).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let (minute, uid) = decoded.split_once('|').ok_or_else(invalid)?;
    let minute: i32 = minute.parse().map_err(|_| invalid())?;
    if uid.is_empty() || !(0..=8 * 1440).contains(&minute) {
        return Err(invalid());
    }
    Ok((minute, uid.to_string()))
}

/// Renders rows through the search's renderer, names their origins and
/// destinations, and attaches live state and service mode.
async fn render_rows(
    app: &App,
    service_date: chrono::NaiveDate,
    rows: Vec<(Value, String, serde_json::Map<String, Value>)>,
) -> Result<Vec<Value>, (StatusCode, String)> {
    let codes: Vec<String> = rows
        .iter()
        .flat_map(|(raw, _, _)| {
            ["destination_crs", "true_origin_crs"]
                .into_iter()
                .filter_map(|key| raw.get(key).and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    let names: HashMap<String, String> =
        queries::station_names_for_crs_batch(&app.database, &codes)
            .await
            .map_err(internal_error)?;
    let mut results: Vec<Value> = rows
        .into_iter()
        .map(|(raw, station, extra)| {
            let mut rendered = calling_point_departure_json(&raw, &station, &names);
            if let Some(object) = rendered.as_object_mut() {
                object.insert("serviceDate".to_string(), json!(service_date));
                object.extend(extra);
            }
            rendered
        })
        .collect();
    crate::routes::schedule_rows::attach_origin_names(&mut results, &names);
    crate::routes::schedule_rows::attach_live(&app.database, service_date, &mut results).await;
    crate::data::schedule_services::annotate_uid_rows(&app.database, service_date, &mut results)
        .await;
    Ok(results)
}

/// `GET /public/trains/arrivals?station=[&date=][&from=][&to=][&from_day_offset=][&to_day_offset=][&origin=][&operator=][&limit=][&after=]`
/// -- a station's scheduled arrivals (2026-10-30).
///
/// One row per arrival, in train-day order of the arrival as shown (the
/// public time, else the working one): every call at `station` a passenger
/// can get off at, and every train terminating there. A row is the search's
/// row (`uid`, `originCrs`/`originName`, `destinationCrs`/`destinationName`,
/// `operator`, `identity`, `serviceMode`, `live`, `serviceDate`) plus
/// `arrival` (`"HH:MM"`), `arrivalDayOffset` (days after `serviceDate`),
/// `terminates` (whether the train ends here) and `scheduled`/`dayOffset`,
/// the departure onward from `station` (`null`/absent for a terminating
/// train). Only `date`'s own trains: the previous date's trains arriving
/// after midnight are not included.
///
/// `200 {"results", "nextCursor", "serviceDate", "window"}` plus the
/// timetable-certainty fields; `404` when nothing is published for the
/// date; `400` for a malformed parameter.
pub(super) async fn get_trains_arrivals(
    State(app): State<App>,
    Query(params): Query<ArrivalsParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let station = normalize_crs("station", &params.station)?;
    let origin = non_empty(&params.origin)
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    let operators = parse_operators(params.operator.as_deref())?;
    let from_time = non_empty(&params.from)
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = non_empty(&params.to)
        .map(|s| normalize_time("to", s))
        .transpose()?;
    let offsets = WindowDayOffsets {
        from: parse_day_offset(
            "from_day_offset",
            &params.from_day_offset,
            "from",
            from_time,
        )?,
        to: parse_day_offset("to_day_offset", &params.to_day_offset, "to", to_time)?,
        arrival_from: None,
        arrival_to: None,
    };
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = non_empty(&params.after)
        .map(decode_arrivals_cursor)
        .transpose()?;

    let london_now = crate::routes::london_now();
    let today = london_now.date_naive();
    let service_date = requested_date(&app, params.date.as_deref(), today).await?;
    if let Some(codes) = &operators {
        check_known_operators(&app, codes, &[service_date]).await?;
    }
    let default_from = if service_date == today {
        london_now.time()
    } else {
        chrono::NaiveTime::MIN
    };
    let window = resolve_window(
        WindowTimes {
            from: from_time,
            to: to_time,
            arrival_from: None,
            arrival_to: None,
        },
        offsets,
        default_from,
    );

    let arrivals = station_arrivals(&app.database, service_date, &station)
        .await
        .map_err(internal_error)?;
    if arrivals.is_empty()
        && !queries::schedule_destination_departures_published_for(&app.database, service_date)
            .await
            .map_err(internal_error)?
    {
        return Err(unpublished(service_date, today));
    }
    let filter = ArrivalsFilter {
        from_minutes: window.from.minutes(),
        to_minutes: window.to.map(WindowBound::minutes),
        origin: origin.as_deref(),
        operators: operators.as_deref(),
        after: after.as_ref().map(|(minute, uid)| (*minute, uid.as_str())),
    };
    let (page, next_cursor) = filter.page(arrivals, usize::try_from(limit).unwrap_or(usize::MAX));
    let rows = page
        .into_iter()
        .map(|arrival| arrival.render_parts(service_date, &station))
        .collect();
    let results = render_rows(&app, service_date, rows).await?;
    let mut body = json!({
        "results": results,
        "nextCursor": next_cursor,
        "serviceDate": service_date,
        "window": {
            "from": window.from.to_json(),
            "to": window.to.map(WindowBound::to_json),
        },
    });
    if let Some(object) = body.as_object_mut() {
        crate::routes::provisional::TimetableCertainty::for_date(service_date, today)
            .insert_into(object);
    }
    Ok(Json(body))
}

/// `GET /public/trains/by-headcode?headcode=[&date=]` -- the trains
/// running under one headcode on a date (2026-10-30), each as a row at its
/// origin: the search's row shape with `stationCrs` the train's first
/// calling point and `scheduled` its departure from there, in departure
/// order. At most 50.
///
/// The headcode's trains are read from `schedule_services` (one row per
/// train and date); a date it has not been published for yet answers an
/// empty list. `200 {"results", "serviceDate"}` plus the
/// timetable-certainty fields; `404` when nothing at all is published for
/// the date; `400` for a malformed headcode or date.
pub(super) async fn get_trains_by_headcode(
    State(app): State<App>,
    Query(params): Query<HeadcodeParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let headcode = normalize_headcode(&params.headcode)?;
    let today = crate::routes::london_today();
    let service_date = requested_date(&app, params.date.as_deref(), today).await?;
    // Each train's first call that day: its origin departure.
    let mut rows: Vec<BoardRow> = sqlx::query_as(&format!(
        "SELECT DISTINCT ON (train_uid) {BOARD_COLUMNS} FROM schedule_destination_departures \
         WHERE service_date = $1 \
           AND train_uid IN ( \
               SELECT uid FROM schedule_services WHERE service_date = $1 AND headcode = $2 \
               LIMIT $3) \
         ORDER BY train_uid, day_offset, scheduled"
    ))
    .bind(service_date)
    .bind(&headcode)
    .bind(MAX_HEADCODE_MATCHES)
    .fetch_all(&app.database)
    .await
    .map_err(|err| internal_error(err.into()))?;
    if rows.is_empty()
        && !queries::schedule_destination_departures_published_for(&app.database, service_date)
            .await
            .map_err(internal_error)?
    {
        return Err(unpublished(service_date, today));
    }
    rows.sort_by_key(|row| {
        let (time, day_offset) = row.shown_departure();
        (minutes(time, day_offset), row.train_uid.clone())
    });
    let rendered = rows
        .into_iter()
        .map(|row| {
            let mut extra = serde_json::Map::new();
            let (_, day_offset) = row.shown_departure();
            extra.insert("dayOffset".to_string(), json!(day_offset));
            let origin = row.origin_crs.clone();
            (row.raw_json(service_date), origin, extra)
        })
        .collect();
    let results = render_rows(&app, service_date, rendered).await?;
    let mut body = json!({
        "results": results,
        "serviceDate": service_date,
    });
    if let Some(object) = body.as_object_mut() {
        crate::routes::provisional::TimetableCertainty::for_date(service_date, today)
            .insert_into(object);
    }
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headcodes_are_a_digit_a_letter_and_two_digits() {
        assert_eq!(normalize_headcode(" 1s00 ").unwrap(), "1S00");
        for bad in ["", "1S0", "1S000", "S100", "1S0A", "W12345"] {
            assert!(normalize_headcode(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn arrivals_cursor_round_trips_and_rejects_junk() {
        let cursor = encode_arrivals_cursor(1530, "C12345");
        assert_eq!(
            decode_arrivals_cursor(&cursor).unwrap(),
            (1530, "C12345".to_string())
        );
        for bad in [
            "",
            "!!",
            &URL_SAFE_NO_PAD.encode("x|C1"),
            &URL_SAFE_NO_PAD.encode("12|"),
        ] {
            assert!(
                decode_arrivals_cursor(bad).is_err(),
                "{bad} must be rejected"
            );
        }
    }

    fn row(scheduled: &str, day_offset: i16) -> BoardRow {
        let time = |t: &str| chrono::NaiveTime::parse_from_str(t, "%H:%M").unwrap();
        BoardRow {
            train_uid: "C1".to_string(),
            origin_crs: "RDG".to_string(),
            destination_crs: "PAD".to_string(),
            true_origin_crs: Some("BRI".to_string()),
            scheduled: time(scheduled),
            day_offset,
            public_departure: None,
            calling_point_arrival: None,
            public_calling_point_arrival: None,
            destination_arrival: None,
            destination_arrival_day_offset: 0,
            public_destination_arrival: None,
            operator_atoc: None,
            headcode: None,
        }
    }

    #[test]
    fn a_call_arrival_before_midnight_is_dated_the_day_before_its_departure() {
        let time = |t: &str| chrono::NaiveTime::parse_from_str(t, "%H:%M").unwrap();
        let mut call = row("00:02", 1);
        call.calling_point_arrival = Some(time("23:58"));
        assert_eq!(call.call_arrival(), Some((time("23:58"), 0)));
        call.public_calling_point_arrival = Some(time("00:00"));
        assert_eq!(call.call_arrival(), Some((time("00:00"), 1)));
        assert_eq!(row("09:00", 0).call_arrival(), None);
    }

    #[test]
    fn a_terminus_arrival_prefers_the_public_time() {
        let time = |t: &str| chrono::NaiveTime::parse_from_str(t, "%H:%M").unwrap();
        let mut terminus = row("22:00", 0);
        terminus.destination_arrival = Some(time("23:59"));
        assert_eq!(terminus.terminus_arrival(), Some((time("23:59"), 0)));
        terminus.public_destination_arrival = Some(time("00:01"));
        assert_eq!(terminus.terminus_arrival(), Some((time("00:01"), 1)));
    }
}
