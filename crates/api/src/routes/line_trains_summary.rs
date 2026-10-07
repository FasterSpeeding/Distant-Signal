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
//!   on-line call at that moment. Only those trains' stops are read, and
//!   only their live state is looked up.
//! - **Small rows.** uid, operator, service mode, scope, direction, origin,
//!   destination (never empty when the train calls on the line), the
//!   on-line public calls, and a compact live status.
//!
//! **Source (phase 3).** The rows come from `line_train_summaries` when it
//! holds current rows for the line and date (see
//! `data::line_train_summaries`), else from the population JSONB. Both
//! paths produce the same [`SummaryRow`]s through
//! `line_train_summaries::derive_row`, so the response is the same.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::http::StatusCode;
use serde::Serialize;

use crate::app::App;
use crate::data::line_train_summaries::{self as lts, RUNNING_DELAY_GRACE_MINUTES, ShipRule};
use crate::data::schedule_services::{ServiceMode, ServiceModeFields};
use crate::data::{queries, trains};

pub(crate) use crate::data::line_train_summaries::{DAY, LineStations, SummaryRow};

/// Most trains one summary response lists (`limit`'s default).
pub(crate) const DEFAULT_LIMIT: usize = 500;
/// `limit`'s ceiling.
pub(crate) const MAX_LIMIT: usize = 2000;
/// Latest minute `from`/`to`/`at` may name: 47:59, the next morning of a
/// service date (CIF day offsets past one do not occur on a line).
pub(crate) const MAX_MINUTE: i32 = 2 * DAY - 1;

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

/// `limit` (1..=`max`, default `default`).
pub(crate) fn parse_limit_within(
    raw: Option<&str>,
    default: usize,
    max: usize,
) -> Result<usize, String> {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(default);
    };
    match raw.trim().parse::<usize>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n),
        _ => Err(format!("invalid limit {raw:?}: use 1 to {max}")),
    }
}

/// `limit` (1..=[`MAX_LIMIT`], default [`DEFAULT_LIMIT`]).
pub(crate) fn parse_limit(raw: Option<&str>) -> Result<usize, String> {
    parse_limit_within(raw, DEFAULT_LIMIT, MAX_LIMIT)
}

/// Minutes after the service date's midnight as `"HH:MM"` (hours may
/// exceed 23 for the next morning).
pub(crate) fn format_minute(minute: i32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
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

/// How far before `at` a train may have reached the line and still be
/// running on it.
pub(crate) const RUNNING_LOOKBACK_MINUTES: i32 = 6 * 60;

/// Could a train be running at `at`, before its live state is known? Due
/// on the line in the six hours before `at`, and its last on-line arrival
/// at most the delay grace before it.
pub(crate) fn is_running_candidate(row: &SummaryRow, at: i32) -> bool {
    match (row.due_minute, row.end_minute()) {
        (Some(due), Some(end)) => {
            due <= at
                && due >= at - RUNNING_LOOKBACK_MINUTES
                && at <= end + RUNNING_DELAY_GRACE_MINUTES
        }
        _ => false,
    }
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
pub(crate) struct StationRef {
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

/// A time with its day offset, as the API writes `lineDue`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DueJson {
    time: String,
    day_offset: i32,
}

impl DueJson {
    pub(crate) fn from_minute(minute: i32) -> Self {
        Self {
            time: format_minute(minute.rem_euclid(DAY)),
            day_offset: minute.div_euclid(DAY),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrainJson {
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
pub(crate) struct LineStationJson {
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

/// Does `row` match the `direction` filter? A row without a direction
/// matches only no filter.
pub(crate) fn direction_matches(row: &SummaryRow, directions: Option<&[String]>) -> bool {
    directions.is_none_or(|d| {
        row.direction
            .as_deref()
            .is_some_and(|own| d.iter().any(|x| x == own))
    })
}

/// The catalogue line `id` names, if `api`'s catalogue has it.
pub(crate) fn catalogue_line<'a>(app: &'a App, id: &str) -> Option<&'a common::LineDefinition> {
    app.config.lines.iter().find(|l| l.id == id)
}

/// Where a read came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowSource {
    /// `line_train_summaries`.
    Table,
    /// The population JSONB (no current table rows).
    Population,
}

/// One line's rows for one date, narrowed to `scopes`, with stops and
/// ends for the rows `ship` names: from `line_train_summaries` when it
/// holds current rows, else worked out from the population. `None` when
/// there is no population (the route's 404).
pub(crate) async fn load_rows(
    app: &App,
    id: &str,
    service_date: chrono::NaiveDate,
    scopes: Option<&[String]>,
    ship: ShipRule,
) -> anyhow::Result<Option<(Vec<SummaryRow>, bool, RowSource)>> {
    let stations = LineStations::for_line(catalogue_line(app, id));
    let fingerprint = lts::derivation_fingerprint(&stations);
    if let Some((rows, has_scope)) =
        lts::list_summary_rows(&app.database, id, service_date, &fingerprint, scopes, ship).await?
    {
        return Ok(Some((rows, has_scope, RowSource::Table)));
    }
    let Some(queries::LineTrainSummaryRows { rows, has_scope }) =
        queries::list_line_train_summary_rows(&app.database, id, service_date, scopes, ship)
            .await?
    else {
        return Ok(None);
    };
    // Decode the calling points that came back, then resolve every TIPLOC
    // they name in one query.
    let decoded: Vec<(lts::EntryFields, Vec<schedule_query::CallingPoint>)> = rows
        .into_iter()
        .filter_map(|row| {
            let cps = row
                .calling_points_json
                .as_deref()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_default();
            Some((
                lts::EntryFields {
                    uid: row.uid?,
                    operator_atoc: row.operator_atoc,
                    train_status: row.train_status,
                    scope: row.scope,
                    direction: row.direction,
                    line_due_time: row.line_due_time,
                    line_due_day_offset: row.line_due_day_offset,
                },
                cps,
            ))
        })
        .collect();
    let tiplocs: Vec<String> = decoded
        .iter()
        .flat_map(|(_, cps)| cps.iter().map(|cp| cp.tiploc.trim().to_uppercase()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let tiploc_to_crs = queries::crs_for_tiplocs_batch(&app.database, &tiplocs).await?;
    let rows = decoded
        .into_iter()
        .map(|(fields, cps)| lts::derive_row(fields, &cps, has_scope, &tiploc_to_crs, &stations))
        .collect();
    Ok(Some((rows, has_scope, RowSource::Population)))
}

/// Live state and service modes for the trains a response names.
pub(crate) struct LiveDecor {
    live: HashMap<String, LiveSummary>,
    modes: HashMap<String, ServiceMode>,
}

impl LiveDecor {
    pub(crate) async fn load(
        app: &App,
        service_date: chrono::NaiveDate,
        uids: Vec<String>,
    ) -> anyhow::Result<Self> {
        let mut live_states =
            trains::get_public_train_states_for_line(&app.database, &uids, service_date).await?;
        crate::data::train_reasons::attach_to_public_states(&app.database, &mut live_states).await;
        // Service modes for the same trains, from `schedule_services` (a
        // read failure falls back to the Train Status, as the default view
        // does).
        let modes =
            crate::data::schedule_services::modes_for_or_trains(&app.database, service_date, &uids)
                .await;
        Ok(Self {
            live: live_states
                .iter()
                .map(|s| (s.train_uid.clone(), LiveSummary::from(s)))
                .collect(),
            modes,
        })
    }

    pub(crate) fn live(&self, uid: &str) -> Option<&LiveSummary> {
        self.live.get(uid)
    }
}

/// Renders rows: live state, modes and station names.
pub(crate) struct Renderer<'a> {
    pub(crate) decor: &'a LiveDecor,
    pub(crate) names: HashMap<String, String>,
}

impl Renderer<'_> {
    /// The station names a response naming `rows` needs: the line's own
    /// stations and every row's ends and on-line stops.
    pub(crate) async fn load_names<'r>(
        app: &App,
        line: Option<&common::LineDefinition>,
        rows: impl Iterator<Item = &'r SummaryRow>,
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut name_crs: HashSet<String> = line
            .map(|l| l.stations.iter().map(|s| s.crs.to_uppercase()).collect())
            .unwrap_or_default();
        for c in rows {
            name_crs.extend(c.origin_crs.iter().cloned());
            name_crs.extend(c.destination_crs.iter().cloned());
            name_crs.extend(c.stops.iter().map(|s| s.crs.clone()));
        }
        let name_crs: Vec<String> = name_crs.into_iter().collect();
        queries::station_names_for_crs_batch(&app.database, &name_crs).await
    }

    pub(crate) fn station_ref(&self, crs: &str) -> StationRef {
        StationRef {
            crs: crs.to_string(),
            name: self.names.get(crs).cloned(),
        }
    }

    pub(crate) fn train_json(&self, c: &SummaryRow) -> TrainJson {
        // Never without a destination when the train calls on the line:
        // the last on-line call stands in for an unresolvable schedule end.
        let destination = c
            .destination_crs
            .as_deref()
            .or_else(|| c.stops.last().map(|s| s.crs.as_str()))
            .map(|crs| self.station_ref(crs));
        let origin = c
            .origin_crs
            .as_deref()
            .or_else(|| c.stops.first().map(|s| s.crs.as_str()))
            .map(|crs| self.station_ref(crs));
        TrainJson {
            uid: c.uid.clone(),
            operator: c.operator_atoc.clone(),
            service_mode: ServiceModeFields(service_mode(
                &c.uid,
                c.train_status.as_deref(),
                &self.decor.modes,
            )),
            scope: c.scope.clone(),
            direction: c.direction.clone(),
            line_due: c.due_minute.map(DueJson::from_minute),
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
            live: self.decor.live(&c.uid).cloned(),
        }
    }

    /// The line's catalogue stations, in order, with names and roles.
    pub(crate) fn line_stations(
        &self,
        line: Option<&common::LineDefinition>,
    ) -> Vec<LineStationJson> {
        line.map(|l| {
            l.stations
                .iter()
                .map(|s| LineStationJson {
                    crs: s.crs.to_uppercase(),
                    name: self.names.get(&s.crs.to_uppercase()).cloned(),
                    role: s.role.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
    }
}

/// Builds the summary response for `/public/lines/{id}/trains?view=summary`.
/// `None` when the line has no population for the date (the route's 404).
pub(crate) async fn build(
    app: &App,
    id: &str,
    service_date: chrono::NaiveDate,
    scopes: Vec<String>,
    params: &SummaryParams,
) -> anyhow::Result<Option<axum::response::Response>> {
    let ship = ShipRule {
        window: params.window,
        at: params.at,
    };
    let Some((candidates, has_scope, _source)) =
        load_rows(app, id, service_date, Some(&scopes), ship).await?
    else {
        return Ok(None);
    };
    let catalogue_line = catalogue_line(app, id);

    let in_window = |c: &SummaryRow| match (params.window, c.due_minute) {
        (None, _) => true,
        (Some((from, to)), Some(due)) => from <= due && due < to,
        (Some(_), None) => false,
    };
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for c in candidates.iter().filter(|c| in_window(c)) {
        *counts
            .entry(c.scope.clone().unwrap_or_else(|| "unknown".to_string()))
            .or_default()
            .entry(c.direction.clone().unwrap_or_else(|| "none".to_string()))
            .or_default() += 1;
    }

    let directions = params.directions.as_deref();
    let sort_key = |c: &SummaryRow| (c.due_minute.unwrap_or(i32::MAX), c.uid.clone());
    let mut listed: Vec<&SummaryRow> = candidates
        .iter()
        .filter(|c| in_window(c) && direction_matches(c, directions))
        .collect();
    listed.sort_by_key(|c| sort_key(c));
    let truncated = listed.len() > params.limit;
    listed.truncate(params.limit);

    let mut running_candidates: Vec<&SummaryRow> = match params.at {
        None => Vec::new(),
        Some(at) => candidates
            .iter()
            .filter(|c| direction_matches(c, directions) && is_running_candidate(c, at))
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
    let decor = LiveDecor::load(app, service_date, live_uids).await?;

    let running: Option<Vec<&SummaryRow>> = params.at.map(|at| {
        running_candidates
            .into_iter()
            .filter(|c| match (c.due_minute, c.end_minute()) {
                (Some(due), Some(end)) => is_running(due, end, at, decor.live(&c.uid)),
                _ => false,
            })
            .collect()
    });

    let names = Renderer::load_names(
        app,
        catalogue_line,
        listed.iter().chain(running.iter().flatten()).copied(),
    )
    .await?;
    let renderer = Renderer {
        decor: &decor,
        names,
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
        stations: renderer.line_stations(catalogue_line),
        counts,
        truncated,
        trains: listed.iter().map(|c| renderer.train_json(c)).collect(),
        running: running.map(|r| r.iter().map(|c| renderer.train_json(c)).collect()),
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
    use crate::data::line_train_summaries::{endpoint_crs, on_line_stops};

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
