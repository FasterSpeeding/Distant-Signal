//! `GET /public/lines/{id}/trains?view=summary` -- the slim, windowed view
//! of a line's trains the line page renders (2026-10-06,
//! docs/superpowers/specs/2026-10-06-line-page-trains-design.md).
//!
//! The default `/trains` response ships every train of the day with its
//! whole stopping pattern -- about 20 MB for the South West Main Line, of
//! which the page used one calling point per train. That response stays
//! exactly as it is for MCP and other consumers; this view is opt-in.
//!
//! What it does differently:
//! - **Membership.** `scope` defaults to `line,shared` (the line's own
//!   trains, and trains running a stretch of it), filtered in SQL.
//! - **Line time.** Each train's time is its first public call on the line
//!   (`lineDue`, published by `schedule-reference`), not its origin's
//!   working time; trains are sorted by it.
//! - **A window.** `from`/`to` (`HH:MM`, hours up to 47 for the next
//!   morning) keep trains whose `lineDue` falls in `[from, to)`; `at` adds a
//!   separate `running` list of trains between their first and last
//!   on-line call at that moment. Only those trains' calling points leave
//!   Postgres, and only their live state is looked up.
//! - **Small rows.** uid, operator, service mode, scope, direction, origin,
//!   destination (never empty when the train calls on the line), the
//!   on-line public calls, and a compact live status.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::http::StatusCode;
use chrono::{NaiveTime, Timelike};
use serde::Serialize;

use crate::app::App;
use crate::data::schedule_services::{ServiceMode, ServiceModeFields};
use crate::data::{queries, trains};

/// Most trains one summary response lists (`limit`'s default).
pub(crate) const DEFAULT_LIMIT: usize = 500;
/// `limit`'s ceiling.
pub(crate) const MAX_LIMIT: usize = 2000;
/// How far before `at` a train may have reached the line and still be
/// running on it: only entries due on the line within this many minutes
/// before `at` are considered for `running`. Six hours covers every line's
/// end-to-end run except the longest cross-country trains (Edinburgh to
/// Plymouth),
/// which drop out of `running` after six hours on the line.
pub(crate) const RUNNING_LOOKBACK_MINUTES: i32 = 6 * 60;
/// How late past its last on-line call a train may be and still count as
/// a running candidate (its live delay decides).
const RUNNING_DELAY_GRACE_MINUTES: i32 = 180;
/// Minutes in a day.
const DAY: i32 = 24 * 60;
/// Latest minute `from`/`to`/`at` may name: 47:59, the next morning of a
/// service date (CIF day offsets past one do not occur on a line).
const MAX_MINUTE: i32 = 2 * DAY - 1;

/// Parsed and validated summary parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SummaryParams {
    /// `[from, to)` in minutes after the service date's midnight.
    pub window: Option<(i32, i32)>,
    pub directions: Option<Vec<String>>,
    pub at: Option<i32>,
    pub limit: usize,
}

/// `"HH:MM"` (hours 0--47) as minutes after the service date's midnight.
pub(crate) fn parse_minute(name: &str, raw: &str) -> Result<i32, String> {
    let bad = || format!("invalid {name} {raw:?}: use HH:MM (00:00 to 47:59)");
    let (h, m) = raw.trim().split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let h: i32 = h.parse().map_err(|_| bad())?;
    let m: i32 = m.parse().map_err(|_| bad())?;
    if !(0..48).contains(&h) || !(0..60).contains(&m) {
        return Err(bad());
    }
    Ok(h * 60 + m)
}

/// `from`/`to` as a half-open minute range. `to` at or before `from`, both
/// on the service date itself, wraps past midnight (`23:00`--`01:00` is
/// two hours). A missing bound is open.
pub(crate) fn parse_window(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Option<(i32, i32)>, String> {
    let from = from
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_minute("from", s))
        .transpose()?;
    let to = to
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_minute("to", s))
        .transpose()?;
    Ok(match (from, to) {
        (None, None) => None,
        (Some(from), None) => Some((from, MAX_MINUTE + 1)),
        (None, Some(to)) => Some((0, to)),
        (Some(from), Some(to)) if to <= from && to < DAY => Some((from, to + DAY)),
        (Some(from), Some(to)) if to <= from => {
            return Err(format!(
                "invalid window: to ({}) must be after from ({})",
                format_minute(to),
                format_minute(from)
            ));
        }
        (Some(from), Some(to)) => Some((from, to)),
    })
}

/// `direction=up|down|loop` (comma list).
pub(crate) fn parse_directions(raw: Option<&str>) -> Result<Option<Vec<String>>, String> {
    const DIRECTIONS: [&str; 3] = ["up", "down", "loop"];
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim) {
        let Some(name) = DIRECTIONS.iter().find(|d| part.eq_ignore_ascii_case(d)) else {
            return Err(format!(
                "invalid direction {part:?}: use up, down or loop (comma-separated)"
            ));
        };
        if !out.iter().any(|o: &String| o == name) {
            out.push((*name).to_string());
        }
    }
    Ok(Some(out))
}

/// `limit` (1..=[`MAX_LIMIT`], default [`DEFAULT_LIMIT`]).
pub(crate) fn parse_limit(raw: Option<&str>) -> Result<usize, String> {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(DEFAULT_LIMIT);
    };
    match raw.trim().parse::<usize>() {
        Ok(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
        _ => Err(format!("invalid limit {raw:?}: use 1 to {MAX_LIMIT}")),
    }
}

/// Minutes after the service date's midnight as `"HH:MM"` (hours may
/// exceed 23 for the next morning).
pub(crate) fn format_minute(minute: i32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// A time with its day offset as minutes after the service date's
/// midnight.
fn minute_of(time: NaiveTime, day_offset: u8) -> i32 {
    // Both terms are small: hour < 24, minute < 60, day_offset a few days.
    i32::from(day_offset) * DAY + i32::try_from(time.hour() * 60 + time.minute()).unwrap_or(0)
}

/// The SQL calling-point range for a request: the window, widened to take
/// in running candidates around `at`. `None` (every entry's calling
/// points) only when neither is given.
pub(crate) fn calling_point_range(
    window: Option<(i32, i32)>,
    at: Option<i32>,
) -> Option<(i32, i32)> {
    let running = at.map(|at| (at - RUNNING_LOOKBACK_MINUTES, at + 1));
    match (window, running) {
        (None, None) => None,
        (Some(w), None) => Some(w),
        (None, Some(r)) => Some(r),
        (Some(w), Some(r)) => Some((w.0.min(r.0), w.1.max(r.1))),
    }
}

/// `serviceMode` for an entry, exactly as the default `/trains` view works
/// it out: the `schedule_services` row first (it knows the Train Category,
/// so a permanent bus and a rail-replacement bus differ), else the
/// population's own CIF Train Status (`5` a replacement bus, `B` a bus,
/// `S`/`4` a ferry; without the category a status-`B` replacement bus reads
/// as `bus`).
pub(crate) fn service_mode(
    uid: &str,
    train_status: Option<&str>,
    modes: &HashMap<String, ServiceMode>,
) -> ServiceMode {
    modes.get(uid).copied().unwrap_or_else(|| {
        ServiceMode::from_train_status(train_status.and_then(|status| status.chars().next()))
    })
}

/// One public call at one of the line's stations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OnLineStop {
    pub crs: String,
    /// Public departure, else public arrival (minutes after midnight of
    /// the service date).
    pub minute: i32,
    /// Public arrival, else public departure -- when the train is AT the
    /// station; the run ends at its last stop's arrival.
    pub arrival_minute: i32,
}

/// The line's stations as membership sees them: catalogue CRS plus the
/// timetable's alias CRS.
pub(crate) struct LineStations {
    pub(crate) stations: HashSet<String>,
    pub(crate) aliases: HashMap<String, String>,
}

impl LineStations {
    pub(crate) fn from_definition(line: &common::LineDefinition) -> Self {
        Self {
            stations: line.stations.iter().map(|s| s.crs.to_uppercase()).collect(),
            aliases: line
                .crs_aliases
                .iter()
                .map(|(from, to)| (from.to_uppercase(), to.to_uppercase()))
                .collect(),
        }
    }

    /// The catalogue CRS a timetable CRS counts as, if it is on the line.
    fn station(&self, crs: &str) -> Option<String> {
        if self.stations.contains(crs) {
            return Some(crs.to_string());
        }
        self.aliases
            .get(crs)
            .filter(|to| self.stations.contains(to.as_str()))
            .cloned()
    }
}

fn tiploc_crs<'a>(tiploc_to_crs: &'a HashMap<String, String>, tiploc: &str) -> Option<&'a str> {
    tiploc_to_crs
        .get(&tiploc.trim().to_uppercase())
        .map(String::as_str)
}

/// A train's public calls at the line's stations, in order, consecutive
/// calls at one station (several TIPLOCs, e.g. platform groups) merged.
pub(crate) fn on_line_stops(
    calling_points: &[schedule_query::CallingPoint],
    tiploc_to_crs: &HashMap<String, String>,
    line: &LineStations,
) -> Vec<OnLineStop> {
    let mut stops: Vec<OnLineStop> = Vec::new();
    for cp in calling_points {
        let departure = cp
            .public_departure
            .map(|t| minute_of(t, cp.departure_day_offset()));
        let arrival = cp.public_arrival.map(|t| minute_of(t, cp.day_offset));
        let Some(minute) = departure.or(arrival) else {
            continue;
        };
        let Some(crs) = tiploc_crs(tiploc_to_crs, &cp.tiploc).and_then(|crs| line.station(crs))
        else {
            continue;
        };
        if let Some(last) = stops.last_mut().filter(|s| s.crs == crs) {
            last.minute = minute;
            continue;
        }
        stops.push(OnLineStop {
            crs,
            minute,
            arrival_minute: arrival.unwrap_or(minute),
        });
    }
    stops
}

/// The schedule's first or last bookable station: the nearest calling
/// point (from the given end) whose TIPLOC resolves to a real station CRS.
/// Walks past depots, junctions and pseudo-CRS (`X..`) ends, which is what
/// left 379 South West Main Line rows without a destination.
pub(crate) fn endpoint_crs<'a>(
    mut calling_points: impl Iterator<Item = &'a schedule_query::CallingPoint>,
    tiploc_to_crs: &HashMap<String, String>,
) -> Option<String> {
    calling_points.find_map(|cp| {
        let calls = cp.public_arrival.is_some()
            || cp.public_departure.is_some()
            || cp.booked_arrival.is_some()
            || cp.booked_departure.is_some();
        tiploc_crs(tiploc_to_crs, &cp.tiploc)
            .filter(|crs| calls && queries::is_bookable_crs(crs))
            .map(str::to_string)
    })
}

/// Is a train with this on-line span running on the line at `at`?
/// Between its first on-line call and its last on-line arrival (pushed back
/// by a known delay), not cancelled, and not reported as finished.
pub(crate) fn is_running(
    due_minute: i32,
    end_minute: i32,
    at: i32,
    live: Option<&LiveSummary>,
) -> bool {
    if live.is_some_and(|l| l.cancelled || l.status.as_deref() == Some("completed")) {
        return false;
    }
    let delay = live.and_then(|l| l.delay_minutes).unwrap_or(0).max(0);
    due_minute <= at && at <= end_minute + delay
}

/// The compact live status the summary carries.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveSummary {
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub delay_provisional: bool,
    pub cancelled: bool,
    pub last_reported_location: Option<String>,
}

impl From<&trains::PublicTrainState> for LiveSummary {
    fn from(s: &trains::PublicTrainState) -> Self {
        Self {
            status: s.status.clone(),
            delay_minutes: s.delay_minutes,
            delay_provisional: s.delay_provisional,
            cancelled: s.cancelled,
            last_reported_location: s.last_reported_location.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StationRef {
    crs: String,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StopJson {
    crs: String,
    time: String,
    day_offset: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DueJson {
    time: String,
    day_offset: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrainJson {
    uid: String,
    operator: Option<String>,
    /// `serviceMode` and `liveTracking`, as on every schedule surface.
    #[serde(flatten)]
    service_mode: ServiceModeFields,
    scope: Option<String>,
    direction: Option<String>,
    line_due: Option<DueJson>,
    origin: Option<StationRef>,
    destination: Option<StationRef>,
    on_line_stops: Vec<StopJson>,
    live: Option<LiveSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LineStationJson {
    crs: String,
    name: Option<String>,
    role: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowJson {
    from: String,
    to: String,
}

/// The whole summary body.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryJson {
    line_id: String,
    date: chrono::NaiveDate,
    scope_applied: bool,
    scopes: Vec<String>,
    window: Option<WindowJson>,
    at: Option<String>,
    directions: Option<Vec<String>>,
    stations: Vec<LineStationJson>,
    /// Trains in the window, per scope and direction (`none` for a train
    /// without one), before the `direction` filter -- for tab counts.
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    truncated: bool,
    trains: Vec<TrainJson>,
    /// Only with `at`.
    running: Option<Vec<TrainJson>>,
}

/// One population entry, worked out.
struct Candidate {
    row: queries::LineTrainSummaryRow,
    uid: String,
    due: Option<i32>,
    stops: Vec<OnLineStop>,
    origin: Option<String>,
    destination: Option<String>,
}

impl Candidate {
    fn end_minute(&self) -> Option<i32> {
        self.stops.last().map(|s| s.arrival_minute)
    }

    fn direction_matches(&self, directions: Option<&[String]>) -> bool {
        directions.is_none_or(|d| {
            self.row
                .direction
                .as_deref()
                .is_some_and(|own| d.iter().any(|x| x == own))
        })
    }
}

fn parse_due(row: &queries::LineTrainSummaryRow) -> Option<i32> {
    let time = NaiveTime::parse_from_str(row.line_due_time.as_deref()?, "%H:%M:%S").ok()?;
    let offset = u8::try_from(row.line_due_day_offset.unwrap_or(0)).ok()?;
    Some(minute_of(time, offset))
}

/// Builds the summary response for `/public/lines/{id}/trains?view=summary`.
/// `None` when the line has no population for the date (the route's 404).
#[expect(
    clippy::too_many_lines,
    reason = "one linear pipeline: rows, crosswalk, window, live, names, body"
)]
pub(crate) async fn build(
    app: &App,
    id: &str,
    service_date: chrono::NaiveDate,
    scopes: Vec<String>,
    params: &SummaryParams,
) -> anyhow::Result<Option<axum::response::Response>> {
    let Some(queries::LineTrainSummaryRows { rows, has_scope }) =
        queries::list_line_train_summary_rows(
            &app.database,
            id,
            service_date,
            Some(&scopes),
            calling_point_range(params.window, params.at),
        )
        .await?
    else {
        return Ok(None);
    };

    let catalogue_line = app.config.lines.iter().find(|l| l.id == id);
    let line_stations = catalogue_line.map_or_else(
        || LineStations {
            stations: HashSet::new(),
            aliases: HashMap::new(),
        },
        LineStations::from_definition,
    );

    // Decode the calling points that came back, then resolve every TIPLOC
    // they name in one query.
    let decoded: Vec<(
        queries::LineTrainSummaryRow,
        Vec<schedule_query::CallingPoint>,
    )> = rows
        .into_iter()
        .filter(|row| row.uid.is_some())
        .map(|mut row| {
            let cps = row
                .calling_points_json
                .take()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            (row, cps)
        })
        .collect();
    let tiplocs: Vec<String> = decoded
        .iter()
        .flat_map(|(_, cps)| cps.iter().map(|cp| cp.tiploc.trim().to_uppercase()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let tiploc_to_crs = queries::crs_for_tiplocs_batch(&app.database, &tiplocs).await?;

    let candidates: Vec<Candidate> = decoded
        .into_iter()
        .map(|(row, cps)| {
            let stops = on_line_stops(&cps, &tiploc_to_crs, &line_stations);
            // A population from before `line_due` existed: the first
            // on-line public call is the same thing.
            let due = parse_due(&row).or_else(|| {
                (!has_scope)
                    .then(|| stops.first().map(|s| s.minute))
                    .flatten()
            });
            Candidate {
                uid: row.uid.clone().unwrap_or_default(),
                origin: endpoint_crs(cps.iter(), &tiploc_to_crs),
                destination: endpoint_crs(cps.iter().rev(), &tiploc_to_crs),
                row,
                due,
                stops,
            }
        })
        .collect();

    let in_window = |c: &Candidate| match (params.window, c.due) {
        (None, _) => true,
        (Some((from, to)), Some(due)) => from <= due && due < to,
        (Some(_), None) => false,
    };
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for c in candidates.iter().filter(|c| in_window(c)) {
        *counts
            .entry(c.row.scope.clone().unwrap_or_else(|| "unknown".to_string()))
            .or_default()
            .entry(
                c.row
                    .direction
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            )
            .or_default() += 1;
    }

    let directions = params.directions.as_deref();
    let sort_key = |c: &Candidate| (c.due.unwrap_or(i32::MAX), c.uid.clone());
    let mut listed: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| in_window(c) && c.direction_matches(directions))
        .collect();
    listed.sort_by_key(|c| sort_key(c));
    let truncated = listed.len() > params.limit;
    listed.truncate(params.limit);

    let mut running_candidates: Vec<&Candidate> = match params.at {
        None => Vec::new(),
        Some(at) => candidates
            .iter()
            .filter(|c| c.direction_matches(directions))
            .filter(|c| match (c.due, c.end_minute()) {
                (Some(due), Some(end)) => due <= at && at <= end + RUNNING_DELAY_GRACE_MINUTES,
                _ => false,
            })
            .collect(),
    };
    running_candidates.sort_by_key(|c| sort_key(c));

    // Live state for exactly the trains this response names.
    let live_uids: Vec<String> = listed
        .iter()
        .chain(running_candidates.iter())
        .map(|c| c.uid.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut live_states =
        trains::get_public_train_states_for_line(&app.database, &live_uids, service_date).await?;
    crate::data::train_reasons::attach_to_public_states(&app.database, &mut live_states).await;
    // Service modes for the same trains, from `schedule_services` (a read
    // failure falls back to the Train Status, as the default view does).
    let service_modes = crate::data::schedule_services::modes_for_or_trains(
        &app.database,
        service_date,
        &live_uids,
    )
    .await;
    let live_by_uid: HashMap<String, LiveSummary> = live_states
        .iter()
        .map(|s| (s.train_uid.clone(), LiveSummary::from(s)))
        .collect();

    let running: Option<Vec<&Candidate>> = params.at.map(|at| {
        running_candidates
            .into_iter()
            .filter(|c| match (c.due, c.end_minute()) {
                (Some(due), Some(end)) => is_running(due, end, at, live_by_uid.get(&c.uid)),
                _ => false,
            })
            .collect()
    });

    // Station names for everything the body names.
    let mut name_crs: HashSet<String> = catalogue_line
        .map(|l| l.stations.iter().map(|s| s.crs.to_uppercase()).collect())
        .unwrap_or_default();
    for c in listed.iter().chain(running.iter().flatten()) {
        name_crs.extend(c.origin.iter().cloned());
        name_crs.extend(c.destination.iter().cloned());
        name_crs.extend(c.stops.iter().map(|s| s.crs.clone()));
    }
    let name_crs: Vec<String> = name_crs.into_iter().collect();
    let names = queries::station_names_for_crs_batch(&app.database, &name_crs).await?;
    let station_ref = |crs: &str| StationRef {
        crs: crs.to_string(),
        name: names.get(crs).cloned(),
    };

    let train_json = |c: &Candidate| -> TrainJson {
        // Never without a destination when the train calls on the line:
        // the last on-line call stands in for an unresolvable schedule end.
        let destination = c
            .destination
            .as_deref()
            .or_else(|| c.stops.last().map(|s| s.crs.as_str()))
            .map(station_ref);
        let origin = c
            .origin
            .as_deref()
            .or_else(|| c.stops.first().map(|s| s.crs.as_str()))
            .map(station_ref);
        TrainJson {
            uid: c.uid.clone(),
            operator: c.row.operator_atoc.clone(),
            service_mode: ServiceModeFields(service_mode(
                &c.uid,
                c.row.train_status.as_deref(),
                &service_modes,
            )),
            scope: c.row.scope.clone(),
            direction: c.row.direction.clone(),
            line_due: c.due.map(|due| DueJson {
                time: format_minute(due % DAY),
                day_offset: due / DAY,
            }),
            origin,
            destination,
            on_line_stops: c
                .stops
                .iter()
                .map(|s| StopJson {
                    crs: s.crs.clone(),
                    time: format_minute(s.minute % DAY),
                    day_offset: s.minute / DAY,
                })
                .collect(),
            live: live_by_uid.get(&c.uid).cloned(),
        }
    };

    let body = SummaryJson {
        line_id: id.to_string(),
        date: service_date,
        scope_applied: has_scope,
        scopes: if has_scope { scopes } else { Vec::new() },
        window: params.window.map(|(from, to)| WindowJson {
            from: format_minute(from),
            to: format_minute(to.min(MAX_MINUTE + 1)),
        }),
        at: params.at.map(format_minute),
        directions: params.directions.clone(),
        stations: catalogue_line
            .map(|l| {
                l.stations
                    .iter()
                    .map(|s| LineStationJson {
                        crs: s.crs.to_uppercase(),
                        name: names.get(&s.crs.to_uppercase()).cloned(),
                        role: s.role.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        counts,
        truncated,
        trains: listed.iter().map(|c| train_json(c)).collect(),
        running: running.map(|r| r.iter().map(|c| train_json(c)).collect()),
    };
    Ok(Some(super::lines::with_scope_applied(
        axum::Json(body),
        Some(has_scope),
    )))
}

/// Is `view` the summary? `None`/`full` is the default view; anything
/// else is a `400`.
pub(crate) fn is_summary_view(view: Option<&str>) -> Result<bool, (StatusCode, String)> {
    match view.map(str::trim) {
        None | Some("" | "full") => Ok(false),
        Some("summary") => Ok(true),
        Some(other) => Err(bad_request(format!(
            "invalid view {other:?}: use summary or full"
        ))),
    }
}

/// Maps a parameter error to the route's `400`.
pub(crate) fn bad_request(message: String) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(
        tiploc: &str,
        arr: Option<&str>,
        dep: Option<&str>,
        day: u8,
    ) -> schedule_query::CallingPoint {
        serde_json::from_value(serde_json::json!({
            "tiploc": tiploc,
            "kind": "Intermediate",
            "booked_arrival": arr.map(|t| format!("{t}:00")),
            "booked_departure": dep.map(|t| format!("{t}:00")),
            "public_arrival": arr.map(|t| format!("{t}:00")),
            "public_departure": dep.map(|t| format!("{t}:00")),
            "is_half_minute_arrival": false,
            "is_half_minute_departure": false,
            "day_offset": day,
        }))
        .expect("calling point")
    }

    fn passing(tiploc: &str) -> schedule_query::CallingPoint {
        serde_json::from_value(serde_json::json!({
            "tiploc": tiploc, "kind": "Intermediate", "booked_arrival": null,
            "booked_departure": null, "is_half_minute_arrival": false,
            "is_half_minute_departure": false,
        }))
        .expect("calling point")
    }

    fn crosswalk() -> HashMap<String, String> {
        [
            ("WATRLMN", "WAT"),
            ("CLPHMJM", "CLJ"),
            ("CLPHMJC", "CLJ"),
            ("WOKING", "WOK"),
            ("BSNGSTK", "BSK"),
            ("WEYMTH", "WEY"),
            ("WEYMDEP", "XWD"),
            ("PDXTLL", "PDX"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
    }

    fn swml() -> LineStations {
        LineStations {
            stations: ["WAT", "CLJ", "WOK", "BSK", "WEY", "PAD"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            aliases: [("PDX".to_string(), "PAD".to_string())]
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn parse_minute_accepts_next_morning_hours_and_rejects_the_rest() {
        assert_eq!(parse_minute("from", "00:00"), Ok(0));
        assert_eq!(parse_minute("from", "13:05"), Ok(13 * 60 + 5));
        assert_eq!(parse_minute("from", "25:30"), Ok(25 * 60 + 30));
        assert_eq!(parse_minute("from", "47:59"), Ok(MAX_MINUTE));
        for bad in ["48:00", "7:00", "07:60", "0700", "", "aa:bb", "-1:00"] {
            assert!(parse_minute("from", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parse_window_wraps_past_midnight_and_leaves_missing_bounds_open() {
        assert_eq!(parse_window(None, None), Ok(None));
        assert_eq!(
            parse_window(Some("13:30"), Some("16:00")),
            Ok(Some((810, 960)))
        );
        assert_eq!(
            parse_window(Some("23:00"), Some("01:00")),
            Ok(Some((1380, 1500)))
        );
        assert_eq!(
            parse_window(Some("23:00"), Some("25:00")),
            Ok(Some((1380, 1500)))
        );
        assert_eq!(
            parse_window(Some("10:00"), None),
            Ok(Some((600, MAX_MINUTE + 1)))
        );
        assert_eq!(parse_window(None, Some("10:00")), Ok(Some((0, 600))));
        assert!(parse_window(Some("26:00"), Some("25:00")).is_err());
        assert!(parse_window(Some("10:00"), Some("bad")).is_err());
    }

    #[test]
    fn parse_directions_and_limit() {
        assert_eq!(parse_directions(None), Ok(None));
        assert_eq!(
            parse_directions(Some("Up, down,up")),
            Ok(Some(vec!["up".into(), "down".into()]))
        );
        assert!(parse_directions(Some("north")).is_err());
        assert_eq!(parse_limit(None), Ok(DEFAULT_LIMIT));
        assert_eq!(parse_limit(Some("20")), Ok(20));
        assert!(parse_limit(Some("0")).is_err());
        assert!(parse_limit(Some("2001")).is_err());
    }

    #[test]
    fn calling_point_range_takes_in_running_candidates() {
        assert_eq!(calling_point_range(None, None), None);
        assert_eq!(
            calling_point_range(Some((600, 750)), None),
            Some((600, 750))
        );
        assert_eq!(
            calling_point_range(Some((600, 750)), Some(630)),
            Some((630 - RUNNING_LOOKBACK_MINUTES, 750))
        );
        assert_eq!(
            calling_point_range(None, Some(630)),
            Some((630 - RUNNING_LOOKBACK_MINUTES, 631))
        );
    }

    #[test]
    fn on_line_stops_keep_public_calls_at_line_stations_in_order() {
        let cps = vec![
            cp("WEYMDEP", None, Some("09:50"), 0),
            cp("WATRLMN", None, Some("10:00"), 0),
            cp("CLPHMJM", Some("10:07"), Some("10:08"), 0),
            cp("CLPHMJC", None, Some("10:09"), 0),
            passing("WOKING"),
            cp("BSNGSTK", Some("10:45"), Some("10:46"), 0),
            cp("PDXTLL", Some("23:59"), Some("00:01"), 0),
        ];
        let stops = on_line_stops(&cps, &crosswalk(), &swml());
        let got: Vec<(&str, i32, i32)> = stops
            .iter()
            .map(|s| (s.crs.as_str(), s.minute, s.arrival_minute))
            .collect();
        assert_eq!(
            got,
            [
                ("WAT", 600, 600),
                // Two TIPLOCs, one station: merged, the later departure.
                ("CLJ", 609, 607),
                ("BSK", 646, 645),
                // An alias CRS counts as its catalogue station; a departure
                // before its arrival is the next day.
                ("PAD", DAY + 1, 1439),
            ]
        );
    }

    #[test]
    fn endpoints_skip_depots_and_pseudo_crs() {
        let cps = [
            cp("WEYMDEP", None, Some("09:50"), 0),
            cp("WATRLMN", None, Some("10:00"), 0),
            cp("WEYMTH", Some("12:00"), None, 0),
            passing("WEYMDEP"),
        ];
        let cw = crosswalk();
        assert_eq!(endpoint_crs(cps.iter(), &cw).as_deref(), Some("WAT"));
        assert_eq!(endpoint_crs(cps.iter().rev(), &cw).as_deref(), Some("WEY"));
        assert_eq!(endpoint_crs([passing("NOWHERE")].iter(), &cw), None);
    }

    #[test]
    fn running_respects_cancellation_completion_and_delay() {
        let live = |status: &str, delay: Option<i32>, cancelled: bool| LiveSummary {
            status: Some(status.to_string()),
            delay_minutes: delay,
            delay_provisional: false,
            cancelled,
            last_reported_location: None,
        };
        assert!(is_running(600, 700, 650, None));
        assert!(is_running(600, 700, 600, None));
        assert!(!is_running(600, 700, 599, None));
        assert!(!is_running(600, 700, 701, None));
        assert!(is_running(
            600,
            700,
            710,
            Some(&live("en_route", Some(12), false))
        ));
        assert!(!is_running(
            600,
            700,
            650,
            Some(&live("en_route", None, true))
        ));
        assert!(!is_running(
            600,
            700,
            650,
            Some(&live("completed", None, false))
        ));
        // An early train is not running past its booked end.
        assert!(!is_running(
            600,
            700,
            701,
            Some(&live("en_route", Some(-3), false))
        ));
    }

    #[test]
    fn service_mode_prefers_schedule_services_then_the_train_status() {
        let modes = HashMap::from([("C1".to_owned(), ServiceMode::ReplacementBus)]);
        // A schedule_services row wins over the population's status.
        assert_eq!(
            service_mode("C1", Some("B"), &modes),
            ServiceMode::ReplacementBus
        );
        let none = HashMap::new();
        for (status, want) in [
            (Some("5"), ServiceMode::ReplacementBus),
            (Some("B"), ServiceMode::Bus),
            (Some("S"), ServiceMode::Ferry),
            (Some("4"), ServiceMode::Ferry),
            (Some("P"), ServiceMode::Train),
            (None, ServiceMode::Train),
        ] {
            assert_eq!(service_mode("C2", status, &none), want, "{status:?}");
        }
    }
}
