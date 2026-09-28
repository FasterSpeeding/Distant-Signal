//! Per-stop LDBWS board data for a train's journey stops: served as
//! `journeyStops[].board` on `GET /Train/by-uid/{uid}/{date}`
//! (`trains::PublicTrainState`) and on `TrackedTrainState`.
//!
//! **What it is.** For a stop with a booked departure, the row on that
//! station's current LDBWS departure board (`station_samples`, polled by
//! `poller-ldbws`) that is this train, if exactly one row is:
//! `{delayReason, cancelReason, isCancelled, delayMinutes, estimated,
//! observedAt}`. `board` is `null` whenever there is no such row. Null means
//! "not known", never "on time".
//!
//! Nested rather than flat because `JourneyStop.delayMinutes` already means
//! TRUST's minutes late at the stop; this one is Darwin's (MCP team naming
//! decision, 2026-09-27). Scope and measurements:
//! ds-review/ldbws-delay-reason-on-train-detail-scope.md.
//!
//! **Matching (train -> board, per stop).** Candidates are the rows of a
//! fresh board for the stop's station whose `std` is within
//! [`RSID_WINDOW_MINUTES`] of the stop's booked departure (dated against the
//! board's own `polled_at`, as the platform overlay does), and whose
//! serviceID TIPLOC (`common::service_id_tiploc`) is the stop's TIPLOC. A
//! serviceID that doesn't parse is no veto, except on a board found only
//! through its `tiplocs` (below), where the TIPLOC is the only reason the
//! board is relevant. Then, sharing `train_resolve`'s rules:
//!
//! 1. The train has a CIF RSID and some candidate carries one: the
//!    candidates with exactly that RSID; if none, those sharing its first 6
//!    characters (the service number without the portion suffix). Exactly
//!    one row matches; zero or several is `null`. A candidate set that
//!    carries RSIDs but not this one is `null`, not a fallback guess.
//! 2. Otherwise (no RSID on the train, or on no candidate -- rows stored
//!    before the poller decoded `rsid`): candidates within
//!    [`TIMETABLE_WINDOW_MINUTES`] whose destination is the train's
//!    terminus CRS and whose operator is the train's ATOC code. Exactly one
//!    row, or `null`.
//!
//! Never nearest-wins: a reason attached to the wrong train is worse than
//! none. Cancelled rows are candidates (they carry `cancelReason`).
//!
//! **When it is null regardless.** The board is older than
//! [`BOARD_FRESHNESS`] (a station no longer sampled keeps its last row
//! forever); the stop has no booked departure (terminating or set-down
//! only -- boards are departures only); the train has already left the
//! stop (TRUST reported a departure there or any movement at a later stop;
//! phase 1 keeps no "last known reason" once the row leaves the board).
//!
//! **Sub-CRS stations.** A board is published under the main CRS, but CIF
//! gives some platforms their own TIPLOC and CRS (PADTLL -> PDX). A stop is
//! therefore matched against its own CRS's board AND any board whose
//! `station_samples.tiplocs` contains its TIPLOC.
//!
//! **Not changed:** `journey::apply_station_sample_platforms` still uses its
//! own destination-and-nearest-time match for platforms. Moving it onto
//! this matcher would change which platforms are shown (fewer, but never
//! another train's), which has not been approved.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::journey::JourneyStop;
use crate::data::queries::BoardSampleRow;
use crate::data::train_resolve::{RSID_WINDOW_MINUTES, TIMETABLE_WINDOW_MINUTES};

/// A board older than this is ignored (about six poll rotations; the
/// aggregator uses 15 minutes for its per-line stats).
pub const BOARD_FRESHNESS: Duration = Duration::minutes(10);

/// Characters of an RSID that identify the service without its portion
/// suffix -- the same prefix rule as `train_resolve::resolve`.
const RSID_PREFIX_LEN: usize = 6;

/// `journeyStops[].board`. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopBoard {
    /// Darwin's passenger delay text, verbatim.
    pub delay_reason: Option<String>,
    /// Darwin's passenger cancellation text, verbatim.
    pub cancel_reason: Option<String>,
    /// The whole service is cancelled at this station.
    pub is_cancelled: bool,
    /// `etd - std` in minutes (0 for early), when `etd` is a time or
    /// "On time"; `null` when it is a status word ("Delayed",
    /// "Cancelled"), so an unknown delay never reads as on time.
    pub delay_minutes: Option<i32>,
    /// LDBWS `etd`, verbatim: `"HH:MM"` (London local), `"On time"`,
    /// `"Delayed"` or `"Cancelled"`.
    pub estimated: String,
    /// When the board was polled (`station_samples.polled_at`).
    pub observed_at: DateTime<Utc>,
}

/// What the matcher needs to know about the train itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrainBoardKeys {
    /// CIF `BX` Retail Service ID, when the schedule has exactly one.
    pub rsid: Option<String>,
    /// CIF `BX` ATOC code, when the schedule has exactly one.
    pub operator: Option<String>,
}

/// The boards a batch of journeys can match against, indexed by board CRS
/// and by covered TIPLOC.
#[derive(Debug, Default)]
pub struct BoardIndex<'a> {
    by_crs: HashMap<String, &'a BoardSampleRow>,
    by_tiploc: HashMap<String, Vec<&'a BoardSampleRow>>,
}

impl<'a> BoardIndex<'a> {
    pub fn new(rows: &'a [BoardSampleRow]) -> Self {
        let mut index = BoardIndex::default();
        for row in rows {
            index
                .by_crs
                .insert(row.sample.crs.trim().to_ascii_uppercase(), row);
            for tiploc in &row.tiplocs {
                index
                    .by_tiploc
                    .entry(tiploc.to_ascii_uppercase())
                    .or_default()
                    .push(row);
            }
        }
        index
    }

    pub fn is_empty(&self) -> bool {
        self.by_crs.is_empty()
    }

    /// The boards relevant to one stop, each with whether it was found by
    /// the stop's own CRS (`true`) or only through its TIPLOC (`false`).
    fn boards_for(
        &self,
        crs: Option<&str>,
        tiploc: Option<&str>,
    ) -> Vec<(&'a BoardSampleRow, bool)> {
        let mut out: Vec<(&'a BoardSampleRow, bool)> = Vec::new();
        if let Some(row) = crs.and_then(|crs| self.by_crs.get(&crs.trim().to_ascii_uppercase())) {
            out.push((row, true));
        }
        if let Some(rows) = tiploc.and_then(|t| self.by_tiploc.get(&t.to_ascii_uppercase())) {
            for row in rows {
                if !out.iter().any(|(seen, _)| std::ptr::eq(*seen, *row)) {
                    out.push((row, false));
                }
            }
        }
        out
    }
}

/// `etd - std` in whole minutes, `None` unless `etd` is a time or
/// "On time". Early running clamps to 0 and a midnight wrap is handled the
/// same way as `poller-ldbws`'s `compute_delay_minutes` (only a diff below
/// -12 h is a wrap), but a status word is `None` here, not 0.
pub fn board_delay_minutes(std: &str, etd: &str) -> Option<i32> {
    let scheduled = NaiveTime::parse_from_str(std, "%H:%M").ok()?;
    if etd.eq_ignore_ascii_case("On time") {
        return Some(0);
    }
    let estimated = NaiveTime::parse_from_str(etd, "%H:%M").ok()?;
    let diff = (estimated - scheduled).num_minutes();
    Some(if diff < -720 {
        (diff + 1440) as i32
    } else {
        diff.max(0) as i32
    })
}

fn rsid_prefix(rsid: &str) -> Option<&str> {
    rsid.get(..RSID_PREFIX_LEN)
}

/// Exactly one element, or `None`.
fn only<T>(mut items: Vec<T>) -> Option<T> {
    if items.len() == 1 { items.pop() } else { None }
}

/// The one board row that is this train at `stop`, with its board's poll
/// time. See the module doc for every rule.
fn match_stop<'a>(
    stop: &JourneyStop,
    has_left: bool,
    boards: &BoardIndex<'a>,
    keys: &TrainBoardKeys,
    terminus_crs: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(&'a common::StationDeparture, DateTime<Utc>)> {
    let booked = stop.scheduled_departure?;
    if has_left {
        return None;
    }
    let stop_tiploc = stop.tiploc.as_deref();

    struct Candidate<'a> {
        row: &'a common::StationDeparture,
        polled_at: DateTime<Utc>,
        delta: Duration,
    }
    let mut candidates: Vec<Candidate<'a>> = Vec::new();
    for (board, by_crs) in boards.boards_for(stop.crs.as_deref(), stop_tiploc) {
        let polled_at = board.sample.polled_at;
        if now - polled_at > BOARD_FRESHNESS {
            continue;
        }
        for row in &board.sample.departures {
            let hint = common::service_id_tiploc(&row.service_id);
            let tiploc_ok = match (hint, stop_tiploc) {
                (Some(hint), Some(stop_tiploc)) => hint.eq_ignore_ascii_case(stop_tiploc),
                // No parseable hint: no veto on the stop's own board, but a
                // board reached only by TIPLOC needs the TIPLOC to agree.
                (None, _) => by_crs,
                (Some(_), None) => by_crs,
            };
            if !tiploc_ok {
                continue;
            }
            let Ok(std) = NaiveTime::parse_from_str(&row.scheduled, "%H:%M") else {
                continue;
            };
            let Some(at) = crate::data::eta_blend::resolve_london_time_near(polled_at, std) else {
                continue;
            };
            let delta = (at - booked).abs();
            if delta > Duration::minutes(RSID_WINDOW_MINUTES) {
                continue;
            }
            candidates.push(Candidate {
                row,
                polled_at,
                delta,
            });
        }
    }

    let picked = match keys.rsid.as_deref() {
        Some(rsid) if candidates.iter().any(|c| c.row.rsid.is_some()) => {
            let exact: Vec<&Candidate<'a>> = candidates
                .iter()
                .filter(|c| {
                    c.row
                        .rsid
                        .as_deref()
                        .is_some_and(|r| r.eq_ignore_ascii_case(rsid))
                })
                .collect();
            if exact.is_empty() {
                let prefix = rsid_prefix(rsid)?;
                only(
                    candidates
                        .iter()
                        .filter(|c| {
                            c.row
                                .rsid
                                .as_deref()
                                .and_then(rsid_prefix)
                                .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
                        })
                        .collect(),
                )?
            } else {
                only(exact)?
            }
        }
        _ => {
            let terminus = terminus_crs?;
            let operator = keys.operator.as_deref()?;
            only(
                candidates
                    .iter()
                    .filter(|c| c.delta <= Duration::minutes(TIMETABLE_WINDOW_MINUTES))
                    .filter(|c| c.row.destination_crs.eq_ignore_ascii_case(terminus))
                    .filter(|c| c.row.operator.eq_ignore_ascii_case(operator))
                    .collect(),
            )?
        }
    };
    Some((picked.row, picked.polled_at))
}

/// Sets `board` on every stop of one train (see the module doc). Run after
/// the movement overlay, which it reads to tell whether the train has left
/// a stop.
pub fn apply_station_sample_board(
    stops: &mut [JourneyStop],
    boards: &BoardIndex<'_>,
    keys: &TrainBoardKeys,
    now: DateTime<Utc>,
) {
    for stop in stops.iter_mut() {
        stop.board = None;
    }
    if boards.is_empty() {
        return;
    }
    let terminus_crs = stops
        .iter()
        .rev()
        .find(|stop| stop.kind == Some(schedule_query::CallingPointKind::Terminate))
        .and_then(|stop| stop.crs.clone());
    // Every stop before the last one TRUST reported any movement at has
    // been left, even if its own departure report is missing.
    let last_reported = stops
        .iter()
        .rposition(|stop| stop.actual_arrival.is_some() || stop.actual_departure.is_some());
    for index in 0..stops.len() {
        let has_left = stops[index].actual_departure.is_some()
            || last_reported.is_some_and(|last| index < last);
        let board = match_stop(
            &stops[index],
            has_left,
            boards,
            keys,
            terminus_crs.as_deref(),
            now,
        )
        .map(|(row, polled_at)| StopBoard {
            delay_reason: row.delay_reason.clone(),
            cancel_reason: row.cancel_reason.clone(),
            is_cancelled: row.is_cancelled,
            delay_minutes: if row.is_cancelled {
                None
            } else {
                board_delay_minutes(&row.scheduled, &row.estimated)
            },
            estimated: row.estimated.clone(),
            observed_at: polled_at,
        });
        stops[index].board = board;
    }
}

/// [`TrainBoardKeys`] for every `(train_uid, service_date)`, from
/// `schedule_destination_departures` (index
/// `(train_uid, service_date, scheduled)`). A value is `None` unless the
/// train's rows agree on exactly one, the same rule as
/// `train_operator::operators_for_trains`. A train with no rows is absent.
pub async fn board_keys_for_trains(
    pool: &PgPool,
    trains: &[(String, NaiveDate)],
) -> anyhow::Result<HashMap<(String, NaiveDate), TrainBoardKeys>> {
    if trains.is_empty() {
        return Ok(HashMap::new());
    }
    let uids: Vec<&str> = trains.iter().map(|(uid, _)| uid.as_str()).collect();
    let dates: Vec<NaiveDate> = trains.iter().map(|(_, date)| *date).collect();
    let rows: Vec<(String, NaiveDate, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT d.train_uid, d.service_date, \
                CASE WHEN COUNT(DISTINCT d.rsid) = 1 THEN MIN(d.rsid) END, \
                CASE WHEN COUNT(DISTINCT d.operator_atoc) = 1 THEN MIN(d.operator_atoc) END \
         FROM UNNEST($1::text[], $2::date[]) AS k(train_uid, service_date) \
         JOIN schedule_destination_departures d \
           ON d.train_uid = k.train_uid AND d.service_date = k.service_date \
         GROUP BY d.train_uid, d.service_date",
    )
    .bind(&uids)
    .bind(&dates)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(uid, date, rsid, operator)| {
            (
                (uid, date),
                TrainBoardKeys {
                    rsid: rsid
                        .map(|r| r.trim().to_ascii_uppercase())
                        .filter(|r| !r.is_empty()),
                    operator,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journey::test_support::stop;
    use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};

    const NOW: &str = "2026-09-28T16:00:00Z";

    fn now() -> DateTime<Utc> {
        NOW.parse().unwrap()
    }

    fn row(
        service_id: &str,
        std: &str,
        etd: &str,
        destination: &str,
        rsid: Option<&str>,
    ) -> common::StationDeparture {
        common::StationDeparture {
            service_id: service_id.to_string(),
            operator: "SW".to_string(),
            destination_crs: destination.to_string(),
            scheduled: std.to_string(),
            estimated: etd.to_string(),
            is_cancelled: false,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: vec![],
            platform: None,
            planned_platform: None,
            rsid: rsid.map(str::to_string),
            calling_points: vec![],
        }
    }

    fn sample_board(
        crs: &str,
        polled_at: &str,
        tiplocs: &[&str],
        departures: Vec<common::StationDeparture>,
    ) -> BoardSampleRow {
        BoardSampleRow {
            sample: common::StationSample {
                crs: crs.to_string(),
                polled_at: polled_at.parse().unwrap(),
                departures,
            },
            tiplocs: tiplocs.iter().map(|t| t.to_string()).collect(),
        }
    }

    /// WAT 17:00 BST -> CLJ 17:07 -> WOK (16:00Z / 16:07Z UTC).
    fn journey() -> Vec<JourneyStop> {
        vec![
            stop("WAT", "WATRLMN", Origin, Some("2026-09-28T16:00:00Z")),
            stop("CLJ", "CLPHMJM", Intermediate, Some("2026-09-28T16:07:00Z")),
            stop("WOK", "WOKING", Terminate, None),
        ]
    }

    fn keys(rsid: Option<&str>) -> TrainBoardKeys {
        TrainBoardKeys {
            rsid: rsid.map(str::to_string),
            operator: Some("SW".to_string()),
        }
    }

    fn apply(stops: &mut [JourneyStop], rows: &[BoardSampleRow], keys: &TrainBoardKeys) {
        apply_station_sample_board(stops, &BoardIndex::new(rows), keys, now());
    }

    #[test]
    fn an_rsid_match_wins_over_a_same_time_same_destination_row() {
        let mut stops = journey();
        let mut ours = row("9100001WATRLMN_", "17:00", "17:06", "WOK", Some("SW123400"));
        ours.delay_reason = Some("a signalling fault".to_string());
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &["WATRLMN"],
            vec![
                row(
                    "9100002WATRLMN_",
                    "17:00",
                    "On time",
                    "WOK",
                    Some("SW999900"),
                ),
                ours,
            ],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(
            stops[0].board,
            Some(StopBoard {
                delay_reason: Some("a signalling fault".to_string()),
                cancel_reason: None,
                is_cancelled: false,
                delay_minutes: Some(6),
                estimated: "17:06".to_string(),
                observed_at: "2026-09-28T15:58:00Z".parse().unwrap(),
            })
        );
        assert_eq!(stops[1].board, None, "CLJ has no board");
        assert_eq!(
            stops[2].board, None,
            "a terminating stop is never on a board"
        );
    }

    #[test]
    fn an_rsid_prefix_matches_a_portion_suffix_but_a_foreign_rsid_is_null_not_a_guess() {
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![row(
                "9100001WATRLMN_",
                "17:01",
                "On time",
                "WOK",
                Some("SW123401"),
            )],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board.as_ref().unwrap().delay_minutes, Some(0));

        // Same destination, operator and time, but it names another train.
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![row(
                "9100001WATRLMN_",
                "17:00",
                "On time",
                "WOK",
                Some("SW555500"),
            )],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board, None);
    }

    #[test]
    fn two_rows_with_the_same_rsid_prefix_are_a_tie_and_null() {
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![
                row(
                    "9100001WATRLMN_",
                    "17:00",
                    "On time",
                    "WOK",
                    Some("SW123401"),
                ),
                row(
                    "9100002WATRLMN_",
                    "17:00",
                    "On time",
                    "GLD",
                    Some("SW123402"),
                ),
            ],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board, None);
    }

    #[test]
    fn without_rsids_a_unique_destination_operator_time_row_matches_and_a_tie_is_null() {
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![
                row("9100001WATRLMN_", "17:01", "Delayed", "WOK", None),
                // Different destination at the same time: not a candidate.
                row("9100002WATRLMN_", "17:00", "On time", "GLD", None),
                // Right destination, 4 minutes out: outside the 2-minute window.
                row("9100003WATRLMN_", "17:04", "On time", "WOK", None),
            ],
        )];
        apply(&mut stops, &rows, &keys(None));
        let board = stops[0].board.as_ref().expect("unique match");
        assert_eq!(board.estimated, "Delayed");
        assert_eq!(board.delay_minutes, None, "Delayed is unknown, not on time");

        // Board rows carry no RSID yet, the train has one: same fallback.
        let mut stops = journey();
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert!(stops[0].board.is_some());

        // Two same-destination rows within 2 minutes: null, not nearest.
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![
                row("9100001WATRLMN_", "17:00", "On time", "WOK", None),
                row("9100002WATRLMN_", "17:02", "On time", "WOK", None),
            ],
        )];
        apply(&mut stops, &rows, &keys(None));
        assert_eq!(stops[0].board, None);

        // No known operator: the fallback cannot run.
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:58:00Z",
            &[],
            vec![row("9100001WATRLMN_", "17:00", "On time", "WOK", None)],
        )];
        apply(&mut stops, &rows, &TrainBoardKeys::default());
        assert_eq!(stops[0].board, None);
    }

    #[test]
    fn a_cancelled_row_matches_and_carries_its_reason_with_no_delay_minutes() {
        let mut stops = journey();
        let mut cancelled = row(
            "9100001CLPHMJM_",
            "17:07",
            "Cancelled",
            "WOK",
            Some("SW123400"),
        );
        cancelled.is_cancelled = true;
        cancelled.cancel_reason = Some("a shortage of train crew".to_string());
        let rows = [sample_board(
            "CLJ",
            "2026-09-28T15:59:30Z",
            &[],
            vec![cancelled],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        let board = stops[1]
            .board
            .as_ref()
            .expect("cancelled rows are candidates");
        assert!(board.is_cancelled);
        assert_eq!(
            board.cancel_reason.as_deref(),
            Some("a shortage of train crew")
        );
        assert_eq!(board.delay_minutes, None);
    }

    #[test]
    fn a_stale_board_is_ignored() {
        let mut stops = journey();
        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:49:59Z",
            &[],
            vec![row(
                "9100001WATRLMN_",
                "17:00",
                "On time",
                "WOK",
                Some("SW123400"),
            )],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board, None);

        let rows = [sample_board(
            "WAT",
            "2026-09-28T15:50:00Z",
            &[],
            vec![row(
                "9100001WATRLMN_",
                "17:00",
                "On time",
                "WOK",
                Some("SW123400"),
            )],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert!(
            stops[0].board.is_some(),
            "exactly 10 minutes old is still fresh"
        );
    }

    #[test]
    fn a_stop_the_train_has_left_is_null_even_while_its_row_is_still_on_the_board() {
        let rows = [
            sample_board(
                "WAT",
                "2026-09-28T15:59:00Z",
                &[],
                vec![row(
                    "9100001WATRLMN_",
                    "17:00",
                    "17:03",
                    "WOK",
                    Some("SW123400"),
                )],
            ),
            sample_board(
                "CLJ",
                "2026-09-28T15:59:00Z",
                &[],
                vec![row(
                    "9100001CLPHMJM_",
                    "17:07",
                    "17:10",
                    "WOK",
                    Some("SW123400"),
                )],
            ),
        ];
        // TRUST reported the WAT departure.
        let mut stops = journey();
        stops[0].actual_departure = Some("2026-09-28T16:03:00Z".parse().unwrap());
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board, None);
        assert!(stops[1].board.is_some());

        // No WAT departure report, but an arrival at CLJ: WAT was left.
        let mut stops = journey();
        stops[1].actual_arrival = Some("2026-09-28T16:09:00Z".parse().unwrap());
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[0].board, None);
        assert!(stops[1].board.is_some(), "arrived but not yet departed");
    }

    #[test]
    fn a_service_id_for_another_tiploc_is_excluded_and_an_opaque_one_is_no_veto() {
        let mut stops = journey();
        let rows = [sample_board(
            "CLJ",
            "2026-09-28T15:59:00Z",
            &[],
            // Windsor-lines TIPLOC at the same station: a different call.
            vec![row(
                "9100001CLPHMJW_",
                "17:07",
                "On time",
                "WOK",
                Some("SW123400"),
            )],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert_eq!(stops[1].board, None);

        let mut stops = journey();
        let rows = [sample_board(
            "CLJ",
            "2026-09-28T15:59:00Z",
            &[],
            vec![row("opaque==", "17:07", "On time", "WOK", Some("SW123400"))],
        )];
        apply(&mut stops, &rows, &keys(Some("SW123400")));
        assert!(stops[1].board.is_some());
    }

    #[test]
    fn a_sub_crs_stop_matches_the_main_stations_board_through_its_tiplocs() {
        // Elizabeth line at Paddington: the stop's CRS is PDX, which has no
        // board; PAD's board covers PADTLL.
        let mut stops = vec![
            stop("PDX", "PADTLL", Origin, Some("2026-09-28T16:00:00Z")),
            stop("ABW", "ABWDXR", Terminate, None),
        ];
        let rows = [sample_board(
            "PAD",
            "2026-09-28T15:59:00Z",
            &["PADTON", "PADTLL"],
            vec![
                row(
                    "9100001PADTON_1",
                    "17:00",
                    "On time",
                    "ABW",
                    Some("XR111100"),
                ),
                row("9100002PADTLL_1", "17:00", "17:02", "ABW", Some("XR123400")),
                // Opaque serviceID on a board found only by TIPLOC: vetoed.
                row("opaque==", "17:00", "On time", "ABW", Some("XR123400")),
            ],
        )];
        let keys = TrainBoardKeys {
            rsid: Some("XR123400".to_string()),
            operator: Some("XR".to_string()),
        };
        apply(&mut stops, &rows, &keys);
        assert_eq!(stops[0].board.as_ref().unwrap().estimated, "17:02");
    }

    #[test]
    fn a_post_midnight_std_is_dated_to_the_next_day() {
        // 00:05 BST on the 29th = 23:05Z on the 28th, board polled 23:00Z.
        let mut stops = vec![
            stop("WAT", "WATRLMN", Origin, Some("2026-09-28T23:05:00Z")),
            stop("WOK", "WOKING", Terminate, None),
        ];
        let rows = [sample_board(
            "WAT",
            "2026-09-28T23:00:00Z",
            &[],
            vec![row(
                "9100001WATRLMN_",
                "00:05",
                "00:07",
                "WOK",
                Some("SW123400"),
            )],
        )];
        apply_station_sample_board(
            &mut stops,
            &BoardIndex::new(&rows),
            &keys(Some("SW123400")),
            "2026-09-28T23:01:00Z".parse().unwrap(),
        );
        assert_eq!(stops[0].board.as_ref().unwrap().delay_minutes, Some(2));
    }

    #[test]
    fn board_delay_minutes_is_null_for_a_status_word_and_handles_wrap_and_early() {
        assert_eq!(board_delay_minutes("10:00", "10:07"), Some(7));
        assert_eq!(board_delay_minutes("10:00", "On time"), Some(0));
        assert_eq!(board_delay_minutes("10:00", "09:58"), Some(0));
        assert_eq!(board_delay_minutes("23:58", "00:03"), Some(5));
        assert_eq!(board_delay_minutes("10:00", "Delayed"), None);
        assert_eq!(board_delay_minutes("10:00", "Cancelled"), None);
        assert_eq!(board_delay_minutes("bad", "10:00"), None);
    }

    #[test]
    fn board_serializes_camel_case_and_an_unmatched_stop_serializes_null() {
        let board = StopBoard {
            delay_reason: Some("a fault".to_string()),
            cancel_reason: None,
            is_cancelled: false,
            delay_minutes: None,
            estimated: "Delayed".to_string(),
            observed_at: "2026-09-28T15:58:00Z".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_value(&board).unwrap(),
            serde_json::json!({
                "delayReason": "a fault",
                "cancelReason": null,
                "isCancelled": false,
                "delayMinutes": null,
                "estimated": "Delayed",
                "observedAt": "2026-09-28T15:58:00Z",
            })
        );
        let unmatched = serde_json::to_value(stop("WAT", "WATRLMN", Origin, None)).unwrap();
        assert!(unmatched.get("board").is_some(), "key must be present");
        assert!(unmatched["board"].is_null());
    }
}
