//! The `/trains` boards' advanced stop filters' parameters (2026-10-30):
//! `calls_at`, `via`, `avoid` and `avoid_stop`, on `GET
//! /public/trains/search` and `GET /public/trains/arrivals`. What they
//! mean, and how a train is checked, is `data::stop_filters`'s doc.
//!
//! Each is a comma-separated list, validated like `/Trips/plan`'s own
//! (`routes::trips`): `calls_at` and `via` are ordered lists of at most
//! [`MAX_ORDERED`] entries, each of which may be an OR choice (`KGX|EUS`,
//! `group:LON`); `avoid` and `avoid_stop` take at most eight
//! stations or groups each, and no `|`. Every code must be a known station
//! (or a known `tiploc:` location), and the lists must not contradict
//! each other or the stations searched. Each problem is a `400` before
//! the board is read.
//!
//! `previously_calls_at` and `previously_stops_at` (2026-10-30, DS-213)
//! are ordered lists like `calls_at`, over the part of the run before the
//! station searched.

use std::collections::HashSet;

use axum::http::StatusCode;
use sqlx::PgPool;

use crate::data::station_groups::StationChoice;
use crate::data::stop_filters::{SearchStopFilter, StopFilters, tiplocs_for_codes};
use crate::routes::trips::{MAX_VIAS, parse_ordered_choices, parse_station_list};

/// Cap on `calls_at` and on `via`: the planner's via cap. Each entry is one
/// more ordered match over a train's calls, so the cost is in reading the
/// trains, not in the number; three is what a "via X, Y and Z" question
/// needs, and keeps the form's lists short.
pub(super) const MAX_ORDERED: usize = MAX_VIAS;

/// The raw parameters.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StopFilterParams<'a> {
    pub calls_at: Option<&'a str>,
    pub via: Option<&'a str>,
    pub avoid: Option<&'a str>,
    pub avoid_stop: Option<&'a str>,
    pub previously_calls_at: Option<&'a str>,
    pub previously_stops_at: Option<&'a str>,
}

/// The lists as parsed, before their codes are looked up.
#[derive(Debug, Clone)]
pub(super) struct ParsedStopFilters {
    calls_at: Vec<StationChoice>,
    via: Vec<StationChoice>,
    avoid: Vec<StationChoice>,
    avoid_stop: Vec<StationChoice>,
    previously_calls_at: Vec<StationChoice>,
    previously_stops_at: Vec<StationChoice>,
}

/// Which part of the run the `previously_*` lists share with the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Board {
    /// The search (departures, or between two stations): the others look
    /// after the station, the `previously_*` lists before it.
    Search,
    /// The arrivals board: every list looks before the station.
    Arrivals,
}

/// Parses the lists; `None` when all are absent or empty.
pub(super) fn parse(
    params: StopFilterParams<'_>,
) -> Result<Option<ParsedStopFilters>, (StatusCode, String)> {
    let parsed = ParsedStopFilters {
        calls_at: parse_ordered_choices("calls_at", params.calls_at, MAX_ORDERED)?,
        via: parse_ordered_choices("via", params.via, MAX_ORDERED)?,
        avoid: parse_station_list("avoid", params.avoid)?,
        avoid_stop: parse_station_list("avoid_stop", params.avoid_stop)?,
        previously_calls_at: parse_ordered_choices(
            "previously_calls_at",
            params.previously_calls_at,
            MAX_ORDERED,
        )?,
        previously_stops_at: parse_ordered_choices(
            "previously_stops_at",
            params.previously_stops_at,
            MAX_ORDERED,
        )?,
    };
    let empty = parsed.calls_at.is_empty()
        && parsed.via.is_empty()
        && parsed.avoid.is_empty()
        && parsed.avoid_stop.is_empty()
        && parsed.previously_calls_at.is_empty()
        && parsed.previously_stops_at.is_empty();
    Ok((!empty).then_some(parsed))
}

/// [`parse`], then [`ParsedStopFilters::check_conflicts`] against
/// `anchors` on `board`.
pub(super) fn parse_checked(
    params: StopFilterParams<'_>,
    anchors: &[Anchor<'_>],
    board: Board,
) -> Result<Option<ParsedStopFilters>, (StatusCode, String)> {
    let parsed = parse(params)?;
    if let Some(filters) = &parsed {
        filters.check_conflicts(anchors, board)?;
    }
    Ok(parsed)
}

/// The station a search is anchored at, and what it is called in a `400`.
pub(super) struct Anchor<'a> {
    pub crs: &'a str,
    pub role: &'static str,
}

impl ParsedStopFilters {
    /// A `400` for lists that contradict each other or the stations
    /// searched (`anchors`), mirroring `/Trips/plan`'s
    /// `check_avoid_conflicts` and `check_via_conflicts`:
    ///
    /// * an avoided station that is searched (a train cannot avoid where
    ///   you board or alight);
    /// * a single-station `calls_at` or `via` entry that is searched (it
    ///   says nothing: the filters look past it);
    /// * a `via` entry every station of which is in `avoid`, or a
    ///   `calls_at` entry every station of which is in `avoid` or
    ///   `avoid_stop` (it can never be met). A `via` in `avoid_stop` is
    ///   allowed: pass through without stopping.
    ///
    /// The `previously_*` lists get the same checks as `calls_at`: a
    /// single-station entry that is searched is a `400`, and so, on the
    /// arrivals board (`board`), where they look at the same rows as the
    /// avoid lists, is an entry wholly in `avoid` or `avoid_stop`. On the
    /// search they look before the station and the avoid lists after it,
    /// so the two never clash.
    pub(super) fn check_conflicts(
        &self,
        anchors: &[Anchor<'_>],
        board: Board,
    ) -> Result<(), (StatusCode, String)> {
        let bad = |message: String| Err((StatusCode::BAD_REQUEST, message));
        let role_of = |code: &str| anchors.iter().find(|a| a.crs == code).map(|a| a.role);
        for (name, choices) in [("avoid", &self.avoid), ("avoid_stop", &self.avoid_stop)] {
            for choice in choices {
                for code in &choice.codes {
                    let Some(role) = role_of(code) else { continue };
                    return bad(if choice.is_single_station() {
                        format!("{name}: '{code}' is {role}; a train cannot avoid it")
                    } else {
                        format!(
                            "{name}: '{}' includes '{code}', which is {role}; a train cannot \
                             avoid it",
                            choice.label
                        )
                    });
                }
            }
        }
        let codes = |choices: &[StationChoice]| -> HashSet<String> {
            choices
                .iter()
                .flat_map(|c| c.codes.iter().cloned())
                .collect()
        };
        let avoided = codes(&self.avoid);
        let not_stopped: HashSet<String> =
            avoided.union(&codes(&self.avoid_stop)).cloned().collect();
        let earlier_blocked = match board {
            Board::Search => HashSet::new(),
            Board::Arrivals => not_stopped.clone(),
        };
        for (name, choices, blocked, blocked_by) in [
            (
                "calls_at",
                &self.calls_at,
                &not_stopped,
                "avoid or avoid_stop",
            ),
            ("via", &self.via, &avoided, "avoid"),
            (
                "previously_calls_at",
                &self.previously_calls_at,
                &earlier_blocked,
                "avoid or avoid_stop",
            ),
            (
                "previously_stops_at",
                &self.previously_stops_at,
                &earlier_blocked,
                "avoid or avoid_stop",
            ),
        ] {
            for choice in choices {
                let label = &choice.label;
                if choice.is_single_station()
                    && let Some(role) = role_of(label)
                {
                    return bad(format!(
                        "{name}: '{label}' is {role}; name a station elsewhere on the train's \
                         route"
                    ));
                }
                if choice.codes.iter().all(|code| blocked.contains(code)) {
                    return bad(if choice.is_single_station() {
                        format!("{name}: '{label}' is also in {blocked_by}; a train cannot do both")
                    } else {
                        format!(
                            "{name}: every station of '{label}' is also in {blocked_by}; a train \
                             cannot do both"
                        )
                    });
                }
            }
        }
        Ok(())
    }

    /// The lists as TIPLOC sets. A code with no TIPLOC is a `400`, except
    /// a group member, which is skipped (a group is one entry; it is a
    /// `400` only when none of its stations is known).
    pub(super) async fn resolve(&self, pool: &PgPool) -> Result<StopFilters, (StatusCode, String)> {
        let all: Vec<String> = [
            &self.calls_at,
            &self.via,
            &self.avoid,
            &self.avoid_stop,
            &self.previously_calls_at,
            &self.previously_stops_at,
        ]
        .into_iter()
        .flatten()
        .flat_map(|choice| choice.codes.iter().cloned())
        .collect::<HashSet<String>>()
        .into_iter()
        .collect();
        let found = tiplocs_for_codes(pool, &all)
            .await
            .map_err(super::internal_error)?;
        let tiplocs = |name: &str,
                       choice: &StationChoice|
         -> Result<HashSet<String>, (StatusCode, String)> {
            let mut set = HashSet::new();
            for code in &choice.codes {
                match found.get(code) {
                    Some(tiplocs) => set.extend(tiplocs.iter().cloned()),
                    // A group's member with no TIPLOC is just never met.
                    None if !choice.groups.is_empty() && !is_named_alternative(choice, code) => {}
                    None => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            format!("{name}: '{code}' is not a recognised station code"),
                        ));
                    }
                }
            }
            if set.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("{name}: '{}' names no recognised station", choice.label),
                ));
            }
            Ok(set)
        };
        let ordered = |name: &str, choices: &[StationChoice]| {
            choices
                .iter()
                .map(|choice| tiplocs(name, choice))
                .collect::<Result<Vec<_>, _>>()
        };
        let union = |name: &str,
                     choices: &[StationChoice]|
         -> Result<HashSet<String>, (StatusCode, String)> {
            Ok(ordered(name, choices)?.into_iter().flatten().collect())
        };
        Ok(StopFilters {
            calls_at: ordered("calls_at", &self.calls_at)?,
            via: ordered("via", &self.via)?,
            avoid: union("avoid", &self.avoid)?,
            avoid_stop: union("avoid_stop", &self.avoid_stop)?,
            previously_calls_at: ordered("previously_calls_at", &self.previously_calls_at)?,
            previously_stops_at: ordered("previously_stops_at", &self.previously_stops_at)?,
        })
    }

    /// Which lists are set, by wire name.
    #[cfg(test)]
    fn names(&self) -> Vec<&'static str> {
        [
            ("calls_at", self.calls_at.len()),
            ("via", self.via.len()),
            ("avoid", self.avoid.len()),
            ("avoid_stop", self.avoid_stop.len()),
            ("previously_calls_at", self.previously_calls_at.len()),
            ("previously_stops_at", self.previously_stops_at.len()),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(name, _)| name)
        .collect()
    }
}

/// `filters` resolved, with the TIPLOCs of the searched `station` and of
/// `stops_at`, for the calling-point search.
pub(super) async fn search_filter(
    pool: &PgPool,
    filters: &ParsedStopFilters,
    station: &str,
    stops_at: Option<&str>,
) -> Result<SearchStopFilter, (StatusCode, String)> {
    let resolved = filters.resolve(pool).await?;
    let codes: Vec<&str> = std::iter::once(station).chain(stops_at).collect();
    let mut anchors = station_tiplocs(pool, &codes).await?.into_iter();
    Ok(SearchStopFilter {
        filters: resolved,
        station_tiplocs: anchors.next().unwrap_or_default(),
        stops_at_tiplocs: anchors.next(),
    })
}

/// The TIPLOCs of each of `codes`, in order (empty for an unknown one: a
/// searched station is placed on its train by time then, see
/// `data::stop_filters::find_departure`).
pub(super) async fn station_tiplocs(
    pool: &PgPool,
    codes: &[&str],
) -> Result<Vec<HashSet<String>>, (StatusCode, String)> {
    let owned: Vec<String> = codes.iter().map(ToString::to_string).collect();
    let mut found = tiplocs_for_codes(pool, &owned)
        .await
        .map_err(super::internal_error)?;
    Ok(owned
        .iter()
        .map(|code| found.remove(code).unwrap_or_default())
        .collect())
}

/// Whether `code` is named in `choice`'s label itself (`KGX|group:LON`
/// names KGX), not only through one of its groups.
fn is_named_alternative(choice: &StationChoice, code: &str) -> bool {
    choice.label.split('|').any(|part| part == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(calls_at: &str, via: &str, avoid: &str, avoid_stop: &str) -> ParsedStopFilters {
        parse(StopFilterParams {
            calls_at: Some(calls_at),
            via: Some(via),
            avoid: Some(avoid),
            avoid_stop: Some(avoid_stop),
            ..StopFilterParams::default()
        })
        .expect("valid")
        .expect("some list set")
    }

    fn error(result: Result<impl std::fmt::Debug, (StatusCode, String)>) -> String {
        let (status, message) = result.expect_err("a 400");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        message
    }

    const SEARCHED: [Anchor<'static>; 2] = [
        Anchor {
            crs: "RDG",
            role: "the station searched",
        },
        Anchor {
            crs: "PAD",
            role: "the stops_at station",
        },
    ];

    #[test]
    fn empty_lists_are_no_filter() {
        assert!(parse(StopFilterParams::default()).unwrap().is_none());
        assert!(
            parse(StopFilterParams {
                calls_at: Some(" , "),
                avoid: Some(""),
                ..StopFilterParams::default()
            })
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn the_lists_parse_like_the_planners() {
        let filters = parsed("swi|group:lon", "did", "bhm, group:LON", "oxf");
        assert_eq!(filters.names(), ["calls_at", "via", "avoid", "avoid_stop"]);
        assert_eq!(filters.calls_at[0].label, "SWI|group:LON");
        assert_eq!(filters.avoid.len(), 2);
    }

    #[test]
    fn caps_and_bad_shapes_are_400s() {
        let message = error(parse(StopFilterParams {
            calls_at: Some("AAA,BBB,CCC,DDD"),
            ..StopFilterParams::default()
        }));
        assert_eq!(
            message,
            "too many calls_at stations: 4 given, at most 3 allowed"
        );
        let message = error(parse(StopFilterParams {
            via: Some("AAA,BBB,CCC,DDD"),
            ..StopFilterParams::default()
        }));
        assert_eq!(message, "too many via stations: 4 given, at most 3 allowed");
        let nine = "AAA,BBB,CCC,DDD,EEE,FFF,GGG,HHH,III";
        let message = error(parse(StopFilterParams {
            avoid_stop: Some(nine),
            ..StopFilterParams::default()
        }));
        assert_eq!(
            message,
            "too many avoid_stop stations: 9 given, at most 8 allowed"
        );
        let message = error(parse(StopFilterParams {
            avoid: Some("KGX|EUS"),
            ..StopFilterParams::default()
        }));
        assert!(
            message.starts_with("avoid: 'KGX|EUS' uses '|'"),
            "{message}"
        );
        let message = error(parse(StopFilterParams {
            calls_at: Some("SWI,swi"),
            ..StopFilterParams::default()
        }));
        assert_eq!(
            message,
            "calls_at: 'SWI' is given twice in a row; name it once"
        );
        let message = error(parse(StopFilterParams {
            via: Some("group:NOPE"),
            ..StopFilterParams::default()
        }));
        assert!(message.contains("not a known station group"), "{message}");
    }

    #[test]
    fn an_avoided_searched_station_is_a_400() {
        let message = error(parsed("", "", "RDG", "").check_conflicts(&SEARCHED, Board::Search));
        assert_eq!(
            message,
            "avoid: 'RDG' is the station searched; a train cannot avoid it"
        );
        let message =
            error(parsed("", "", "", "group:LON").check_conflicts(&SEARCHED, Board::Search));
        assert_eq!(
            message,
            "avoid_stop: 'group:LON' includes 'PAD', which is the stops_at station; a train \
             cannot avoid it"
        );
    }

    #[test]
    fn a_searched_station_as_calls_at_or_via_is_a_400_but_a_group_holding_it_is_not() {
        let message = error(parsed("PAD", "", "", "").check_conflicts(&SEARCHED, Board::Search));
        assert_eq!(
            message,
            "calls_at: 'PAD' is the stops_at station; name a station elsewhere on the train's \
             route"
        );
        assert!(
            error(parsed("", "RDG", "", "").check_conflicts(&SEARCHED, Board::Search))
                .starts_with("via: 'RDG'")
        );
        assert!(
            parsed("group:LON", "", "", "")
                .check_conflicts(&SEARCHED, Board::Search)
                .is_ok()
        );
    }

    #[test]
    fn contradictory_lists_are_400s() {
        let message = error(parsed("SWI", "", "", "SWI").check_conflicts(&[], Board::Search));
        assert_eq!(
            message,
            "calls_at: 'SWI' is also in avoid or avoid_stop; a train cannot do both"
        );
        let message = error(parsed("", "DID", "DID", "").check_conflicts(&[], Board::Search));
        assert_eq!(
            message,
            "via: 'DID' is also in avoid; a train cannot do both"
        );
        // Passing through without stopping is a real question.
        assert!(
            parsed("", "DID", "", "DID")
                .check_conflicts(&[], Board::Search)
                .is_ok()
        );
        // An OR choice is a 400 only when every station of it is blocked.
        assert!(
            parsed("SWI|DID", "", "SWI", "")
                .check_conflicts(&[], Board::Search)
                .is_ok()
        );
        let message =
            error(parsed("SWI|DID", "", "SWI", "DID").check_conflicts(&[], Board::Search));
        assert!(
            message.starts_with("calls_at: every station of 'SWI|DID'"),
            "{message}"
        );
    }

    fn previously(calls_at: &str, stops_at: &str, avoid: &str) -> ParsedStopFilters {
        parse(StopFilterParams {
            previously_calls_at: Some(calls_at),
            previously_stops_at: Some(stops_at),
            avoid: Some(avoid),
            ..StopFilterParams::default()
        })
        .expect("valid")
        .expect("some list set")
    }

    #[test]
    fn the_previously_lists_parse_like_calls_at() {
        let filters = previously("swi|group:lon,did", "oxf", "");
        assert_eq!(
            filters.names(),
            ["previously_calls_at", "previously_stops_at"]
        );
        assert_eq!(filters.previously_calls_at[0].label, "SWI|group:LON");
        let message = error(parse(StopFilterParams {
            previously_stops_at: Some("AAA,BBB,CCC,DDD"),
            ..StopFilterParams::default()
        }));
        assert_eq!(
            message,
            "too many previously_stops_at stations: 4 given, at most 3 allowed"
        );
        let message = error(parse(StopFilterParams {
            previously_calls_at: Some("SWI,SWI"),
            ..StopFilterParams::default()
        }));
        assert_eq!(
            message,
            "previously_calls_at: 'SWI' is given twice in a row; name it once"
        );
    }

    #[test]
    fn a_searched_station_as_a_previously_entry_is_a_400() {
        let message = error(previously("RDG", "", "").check_conflicts(&SEARCHED, Board::Search));
        assert_eq!(
            message,
            "previously_calls_at: 'RDG' is the station searched; name a station elsewhere on \
             the train's route"
        );
        assert!(
            error(previously("", "PAD", "").check_conflicts(&SEARCHED, Board::Search))
                .starts_with("previously_stops_at: 'PAD' is the stops_at station")
        );
    }

    #[test]
    fn a_previously_entry_clashes_with_avoid_only_on_the_arrivals_board() {
        // On the search, avoid looks after the station, previously_* before.
        assert!(
            previously("SWI", "", "SWI")
                .check_conflicts(&[], Board::Search)
                .is_ok()
        );
        let message = error(previously("SWI", "", "SWI").check_conflicts(&[], Board::Arrivals));
        assert_eq!(
            message,
            "previously_calls_at: 'SWI' is also in avoid or avoid_stop; a train cannot do both"
        );
    }

    #[test]
    fn a_named_alternative_is_told_apart_from_a_group_member() {
        let choice = parsed("KGX|group:LON", "", "", "").calls_at[0].clone();
        assert!(is_named_alternative(&choice, "KGX"));
        assert!(!is_named_alternative(&choice, "PAD"));
    }
}
