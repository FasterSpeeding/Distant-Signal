//! A journey window leg's search filters (DS-215): the Between stations
//! search's `operator`, `origin` (here `startsAt`), `terminates_at` and
//! stop filters, for "Add a leg" and "Edit search" on a journey
//! (`POST /Journeys/{id}/legs`, `POST .../legs/{legId}/train`) and the
//! leg's candidates (`GET .../legs/{legId}/candidates`). A leg runs between
//! two stations, so its filters are checked exactly as
//! `GET /public/trains/search` checks them with `station` the leg's origin
//! and `stops_at` its destination: the same parsers, caps and `400`s, the
//! messages naming the `/trains` parameter.

use axum::http::StatusCode;
use chrono::NaiveDate;

use super::stop_filters::{self, Anchor, Board, StopFilterParams};
use super::{check_known_operators, normalize_crs, parse_operators};
use crate::app::App;
use crate::data::journeys::LegSearchFilters;
use crate::data::stop_filters::SearchStopFilter;

/// A leg's filters, checked, as the candidates search applies them.
#[derive(Debug, Default)]
pub(crate) struct LegFilters {
    /// ATOC codes, any of which (`operator`).
    pub operators: Option<Vec<String>>,
    /// The train's true origin, exactly (`origin`).
    pub starts_at: Option<String>,
    /// The train's true terminus, exactly (`terminates_at`).
    pub terminates_at: Option<String>,
    /// The stop filters over the part of the train ridden between the
    /// leg's two stations, and the `previously_*` ones before its origin.
    pub stop_filter: Option<SearchStopFilter>,
}

/// `filters` resolved for a leg from `origin` to `destination` (CRS codes,
/// upper case): a `400` for anything `/public/trains/search` refuses
/// between those two stations, except that an operator code is only
/// checked for its form here (see [`check`]).
pub(crate) async fn resolve(
    app: &App,
    filters: &LegSearchFilters,
    origin: &str,
    destination: &str,
) -> Result<LegFilters, (StatusCode, String)> {
    let filters = filters.trimmed();
    let operators = parse_operators(filters.operator.as_deref())?;
    let starts_at = filters
        .starts_at
        .as_deref()
        .map(|code| normalize_crs("startsAt", code))
        .transpose()?;
    let terminates_at = filters
        .terminates_at
        .as_deref()
        .map(|code| normalize_crs("terminatesAt", code))
        .transpose()?;
    let anchors = [
        Anchor {
            crs: origin,
            role: "the leg's origin",
        },
        Anchor {
            crs: destination,
            role: "the leg's destination",
        },
    ];
    let parsed = stop_filters::parse_checked(
        StopFilterParams {
            calls_at: filters.calls_at.as_deref(),
            via: filters.via.as_deref(),
            avoid: filters.avoid.as_deref(),
            avoid_stop: filters.avoid_stop.as_deref(),
            previously_calls_at: filters.previously_calls_at.as_deref(),
            previously_stops_at: filters.previously_stops_at.as_deref(),
        },
        &anchors,
        Board::Search,
    )?;
    let stop_filter = match &parsed {
        Some(parsed) => Some(
            stop_filters::search_filter(&app.database, parsed, origin, Some(destination)).await?,
        ),
        None => None,
    };
    Ok(LegFilters {
        operators,
        starts_at,
        terminates_at,
        stop_filter,
    })
}

/// [`resolve`], and each operator code checked against the timetable
/// for `service_date` as the search checks it, for saving with a leg:
/// the filters to store, normalized (codes upper case, values trimmed),
/// or `None` when none is set.
pub(crate) async fn check(
    app: &App,
    filters: &LegSearchFilters,
    origin: &str,
    destination: &str,
    service_date: NaiveDate,
) -> Result<Option<LegSearchFilters>, (StatusCode, String)> {
    let resolved = resolve(app, filters, origin, destination).await?;
    if let Some(codes) = &resolved.operators {
        check_known_operators(app, codes, &[service_date]).await?;
    }
    let trimmed = filters.trimmed();
    let normalized = LegSearchFilters {
        operator: resolved.operators.map(|codes| codes.join(",")),
        starts_at: resolved.starts_at,
        terminates_at: resolved.terminates_at,
        ..trimmed
    };
    Ok((!normalized.is_empty()).then_some(normalized))
}

#[cfg(test)]
mod tests {
    use crate::data::journeys::LegSearchFilters;

    #[test]
    fn the_wire_shape_is_camel_case_and_refuses_an_unknown_key() {
        let filters: LegSearchFilters = serde_json::from_value(serde_json::json!({
            "operator": "SW",
            "startsAt": "WAT",
            "terminatesAt": "POO",
            "previouslyCallsAt": "CLJ",
            "previouslyStopsAt": "VXH",
            "callsAt": "BSK",
            "via": "WOK",
            "avoid": "GLD",
            "avoidStop": "FNB",
        }))
        .expect("every filter deserializes");
        assert_eq!(filters.starts_at.as_deref(), Some("WAT"));
        assert_eq!(filters.avoid_stop.as_deref(), Some("FNB"));
        let unknown = serde_json::from_value::<LegSearchFilters>(serde_json::json!({
            "origin": "WAT",
        }));
        assert!(
            unknown.is_err(),
            "the /trains name `origin` is `startsAt` here"
        );
    }

    #[test]
    fn trimming_drops_blank_values_and_an_all_blank_set_is_empty() {
        let filters = LegSearchFilters {
            operator: Some("  ".to_string()),
            calls_at: Some(" RDG ".to_string()),
            ..LegSearchFilters::default()
        }
        .trimmed();
        assert_eq!(filters.operator, None);
        assert_eq!(filters.calls_at.as_deref(), Some("RDG"));
        assert!(!filters.is_empty());
        assert!(
            LegSearchFilters {
                via: Some(String::new()),
                ..LegSearchFilters::default()
            }
            .trimmed()
            .is_empty()
        );
    }

    #[test]
    fn serializing_skips_unset_filters() {
        let filters = LegSearchFilters {
            operator: Some("GW".to_string()),
            ..LegSearchFilters::default()
        };
        assert_eq!(
            serde_json::to_value(&filters).expect("serializes"),
            serde_json::json!({"operator": "GW"})
        );
    }
}
