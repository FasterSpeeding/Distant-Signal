//! `GET /public/lines/{id}/timetable` -- a line's full-day timetable,
//! cursor-paged (2026-10-07,
//! docs/superpowers/specs/2026-10-06-line-page-trains-design.md §5).
//!
//! Reads `line_train_summaries` (`data::line_train_summaries::timetable_page`):
//! filtered, ordered and cut in SQL by a keyset cursor over
//! `(time, uid)`, where the time is the departure from `from` when given,
//! else the train's time on the line (`lineDue`). When the table holds no
//! current rows for the line and date (not yet published since the table
//! existed, or a catalogue change), the same page is worked out from the
//! population JSONB in memory -- slower, same answer.
//!
//! Live state and service modes are read for the page's trains only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::line_trains_summary::{
    self as summary, DueJson, LineStationJson, LiveDecor, Renderer, TrainJson,
};
use crate::app::App;
use crate::data::line_train_summaries::{self as lts, ShipRule, TimetableFilter};

/// Trains per page by default.
pub(crate) const DEFAULT_LIMIT: usize = 50;
/// `limit`'s ceiling.
pub(crate) const MAX_LIMIT: usize = 200;

/// `GET /public/lines/{id}/timetable`'s query (all optional).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct TimetableQuery {
    pub date: Option<chrono::NaiveDate>,
    /// As `/trains`' `scope`; default `line,shared`.
    pub scope: Option<String>,
    /// `up`, `down`, `loop` (comma list). `direction` is accepted too, as
    /// on `/trains?view=summary`.
    pub dir: Option<String>,
    pub direction: Option<String>,
    /// Station CRS: trains with a public call there, timed by it.
    pub from: Option<String>,
    /// Station CRS: trains with a public call there (after `from`).
    pub to: Option<String>,
    /// `HH:MM` (hours to 47): trains from this time on.
    pub at: Option<String>,
    /// The previous page's `nextCursor`.
    pub after: Option<String>,
    pub limit: Option<String>,
}

/// A station parameter: a three-letter CRS, upper-cased.
pub(crate) fn parse_crs(name: &str, raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if raw.len() == 3 && raw.chars().all(|c| c.is_ascii_alphabetic()) {
        Ok(Some(raw.to_ascii_uppercase()))
    } else {
        Err(format!("invalid {name} {raw:?}: use a station's three-letter CRS code"))
    }
}

/// The cursor as served: `<minute>.<uid>` (a CIF uid is alphanumeric;
/// `-` is let through for fixture uids).
pub(crate) fn format_cursor((minute, uid): &(i32, String)) -> String {
    format!("{minute}.{uid}")
}

/// Parses [`format_cursor`]'s output.
pub(crate) fn parse_cursor(raw: Option<&str>) -> Result<Option<(i32, String)>, String> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let bad = || format!("invalid after {raw:?}: pass a nextCursor back unchanged");
    let (minute, uid) = raw.split_once('.').ok_or_else(bad)?;
    let minute: i32 = minute.parse().map_err(|_| bad())?;
    if !(0..=summary::MAX_MINUTE).contains(&minute)
        || uid.is_empty()
        || uid.len() > 16
        || !uid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(bad());
    }
    Ok(Some((minute, uid.to_string())))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimetableTrainJson {
    #[serde(flatten)]
    train: TrainJson,
    /// When the train is listed: its departure from `from`, else its time
    /// on the line (`lineDue`).
    time: DueJson,
    /// Its arrival at `to`, when `to` was given.
    arrival: Option<DueJson>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimetableJson {
    line_id: String,
    date: chrono::NaiveDate,
    scope_applied: bool,
    scopes: Vec<String>,
    directions: Option<Vec<String>>,
    from: Option<String>,
    to: Option<String>,
    at: Option<String>,
    stations: Vec<LineStationJson>,
    /// The day's trains under `scope`, `from` and `to`, per scope and
    /// direction (`none` without one) -- before `dir`, `at` and the
    /// cursor: the direction tabs' counts.
    counts: BTreeMap<String, BTreeMap<String, i64>>,
    trains: Vec<TimetableTrainJson>,
    /// Pass back as `after` for the next page; `null` on the last.
    next_cursor: Option<String>,
}

/// Validated timetable parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimetableParams {
    pub scopes: Vec<String>,
    pub filter: TimetableFilter,
}

/// Validates the query (a `400` message on error). `scopes` comes from
/// the shared `scope` parser; `None` is the default `line,shared`.
pub(crate) fn parse_params(
    query: &TimetableQuery,
    scopes: Option<Vec<String>>,
) -> Result<TimetableParams, String> {
    let scopes = scopes.unwrap_or_else(|| vec!["line".to_string(), "shared".to_string()]);
    let directions = summary::parse_directions(query.dir.as_deref().or(query.direction.as_deref()))?;
    let from = parse_crs("from", query.from.as_deref())?;
    let to = parse_crs("to", query.to.as_deref())?;
    if from.is_some() && from == to {
        return Err("invalid to: must differ from from".to_string());
    }
    let at = query
        .at
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| summary::parse_minute("at", s))
        .transpose()?;
    Ok(TimetableParams {
        filter: TimetableFilter {
            scopes: Some(scopes.clone()),
            directions,
            from,
            to,
            at,
            after: parse_cursor(query.after.as_deref())?,
            limit: summary::parse_limit_within(query.limit.as_deref(), DEFAULT_LIMIT, MAX_LIMIT)?,
        },
        scopes,
    })
}

/// One timetable page, from the table or (no current rows) the
/// population. `None` when there is neither (the route's 404).
pub(crate) async fn load_page(
    app: &App,
    id: &str,
    service_date: chrono::NaiveDate,
    params: &TimetableParams,
) -> anyhow::Result<Option<(lts::TimetablePage, summary::RowSource)>> {
    let stations = lts::LineStations::for_line(summary::catalogue_line(app, id));
    let fingerprint = lts::derivation_fingerprint(&stations);
    if let Some(page) =
        lts::timetable_page(&app.database, id, service_date, &fingerprint, &params.filter).await?
    {
        return Ok(Some((page, summary::RowSource::Table)));
    }
    let Some((rows, has_scope, _)) = summary::load_rows(
        app,
        id,
        service_date,
        Some(&params.scopes),
        ShipRule::default(),
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some((
        lts::timetable_page_in_memory(rows, has_scope, &params.filter),
        summary::RowSource::Population,
    )))
}

/// Builds the `/timetable` body. `None` when the line has no population
/// for the date.
pub(crate) async fn build(
    app: &App,
    id: &str,
    service_date: chrono::NaiveDate,
    params: TimetableParams,
) -> anyhow::Result<Option<axum::response::Response>> {
    let Some((page, _source)) = load_page(app, id, service_date, &params).await? else {
        return Ok(None);
    };
    let line = summary::catalogue_line(app, id);
    let uids: Vec<String> = page.entries.iter().map(|e| e.row.uid.clone()).collect();
    let decor = LiveDecor::load(app, service_date, uids).await?;
    let names = Renderer::load_names(app, line, page.entries.iter().map(|e| &e.row)).await?;
    let renderer = Renderer {
        decor: &decor,
        names,
    };
    let mut counts: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    for (scope, direction, n) in &page.counts {
        *counts
            .entry(scope.clone().unwrap_or_else(|| "unknown".to_string()))
            .or_default()
            .entry(direction.clone().unwrap_or_else(|| "none".to_string()))
            .or_default() += n;
    }
    let has_scope = page.has_scope;
    let TimetableParams { scopes, filter } = params;
    let body = TimetableJson {
        line_id: id.to_string(),
        date: service_date,
        scope_applied: has_scope,
        scopes: if has_scope { scopes } else { Vec::new() },
        directions: filter.directions,
        from: filter.from,
        to: filter.to,
        at: filter.at.map(summary::format_minute),
        stations: renderer.line_stations(line),
        counts,
        trains: page
            .entries
            .iter()
            .map(|e| TimetableTrainJson {
                train: renderer.train_json(&e.row),
                time: DueJson::from_minute(e.minute),
                arrival: e.to_arrival.map(DueJson::from_minute),
            })
            .collect(),
        next_cursor: page.next.as_ref().map(format_cursor),
    };
    Ok(Some(super::lines::with_scope_applied(
        axum::Json(body),
        Some(has_scope),
    )))
}

#[cfg(test)]
mod db_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crs_and_cursor_parse_and_round_trip() {
        assert_eq!(parse_crs("from", None), Ok(None));
        assert_eq!(parse_crs("from", Some(" wok ")), Ok(Some("WOK".into())));
        assert!(parse_crs("from", Some("WOKI")).is_err());
        assert!(parse_crs("from", Some("W1K")).is_err());
        let cursor = (1445, "C12345".to_string());
        assert_eq!(
            parse_cursor(Some(&format_cursor(&cursor))),
            Ok(Some(cursor))
        );
        assert_eq!(parse_cursor(Some("")), Ok(None));
        for bad in ["600", "x.C1", "600.", "600.C 1", "-5.C1", "3000.C1", "600.C1;--"] {
            assert!(parse_cursor(Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn params_default_scope_accept_both_direction_names_and_reject_a_loop() {
        let query = TimetableQuery {
            direction: Some("up".into()),
            from: Some("wat".into()),
            at: Some("25:10".into()),
            ..TimetableQuery::default()
        };
        let params = parse_params(&query, None).expect("valid");
        assert_eq!(params.scopes, ["line", "shared"]);
        assert_eq!(params.filter.directions, Some(vec!["up".to_string()]));
        assert_eq!(params.filter.from.as_deref(), Some("WAT"));
        assert_eq!(params.filter.at, Some(25 * 60 + 10));
        assert_eq!(params.filter.limit, DEFAULT_LIMIT);
        let query = TimetableQuery {
            dir: Some("down".into()),
            direction: Some("up".into()),
            ..TimetableQuery::default()
        };
        assert_eq!(
            parse_params(&query, None).expect("valid").filter.directions,
            Some(vec!["down".to_string()]),
            "dir wins"
        );
        let query = TimetableQuery {
            from: Some("WAT".into()),
            to: Some("wat".into()),
            ..TimetableQuery::default()
        };
        assert!(parse_params(&query, None).is_err());
        let query = TimetableQuery {
            limit: Some("201".into()),
            ..TimetableQuery::default()
        };
        assert!(parse_params(&query, None).is_err());
    }
}
