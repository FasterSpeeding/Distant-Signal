//! Checks a hand-entered train leg against the timetable: the train must
//! call at the leg's origin and then at its destination on the service
//! date, letting passengers board at the origin and get off at the
//! destination.
//!
//! The leg picker (`queries::search_journey_leg_candidates`) and the trip
//! planner never offer anything else; this is for a leg typed by hand (a
//! `knownTrain` leg with an origin or destination override, a UID bound to
//! a window leg, or a delegated itinerary from ds-mcp).
//!
//! **Only when the schedule is known.** A train with no
//! `schedule_calling_points_full` rows for its UID and date (a late STP or
//! VSTP service, a date not yet published, a cancelled schedule) passes
//! unchecked, exactly as before: tracking it may still resolve later, and
//! the leg's resolution status already says it hasn't. Saved legs are never
//! re-checked.
//!
//! Stations are matched as the journey timeline matches them: each calling
//! point's TIPLOC goes through `queries::crs_for_tiplocs_batch` (the union
//! of `tiploc_crs` and `stanox_crs`, the same reference tables the planner's
//! interchange data and the published `schedule_destination_departures`
//! CRS codes come from). A NULL `can_board`/`can_alight` (a row published
//! before migration `20261001160000`) means allowed, as in the planner. A
//! passing point (no booked arrival or departure) is not a call.

use std::collections::HashMap;

use chrono::NaiveDate;
use ds_store::trains::types::tiploc_key;
use sqlx::PgPool;

use crate::data::queries;

/// Why a leg was refused. Each has a stable wire code ([`Self::code`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegCallError {
    /// No call at the origin at all (it may pass through).
    DoesNotCallAtOrigin,
    /// No call at the destination at all.
    DoesNotCallAtDestination,
    /// Both are called at, but never the destination after a call where
    /// boarding is allowed at the origin.
    DestinationBeforeOrigin,
    /// The origin is called at, but only to set down (or it is the
    /// terminus).
    BoardingNotAllowed,
    /// A later call at the destination exists, but only to pick up (a `U`
    /// stop).
    AlightingNotAllowed,
}

impl LegCallError {
    /// The stable code ds-mcp and the web app may key on.
    pub fn code(self) -> &'static str {
        match self {
            LegCallError::DoesNotCallAtOrigin => "train_does_not_call_at_origin",
            LegCallError::DoesNotCallAtDestination => "train_does_not_call_at_destination",
            LegCallError::DestinationBeforeOrigin => "destination_before_origin",
            LegCallError::BoardingNotAllowed => "boarding_not_allowed",
            LegCallError::AlightingNotAllowed => "alighting_not_allowed",
        }
    }
}

/// One timetabled calling point, reduced to what the check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The calling point's CRS, `None` when its TIPLOC has none.
    pub crs: Option<String>,
    /// `false` for a passing point.
    pub calls: bool,
    pub can_board: bool,
    pub can_alight: bool,
}

/// The pure check over `calls` (in schedule order). An absent `origin`
/// means the train's own origin and an absent `destination` its own
/// terminus, which are always fine, so only the given end is checked.
pub fn check_calls(
    calls: &[Call],
    origin: Option<&str>,
    destination: Option<&str>,
) -> Result<(), LegCallError> {
    let at = |crs: &str| -> Vec<usize> {
        calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.calls && call.crs.as_deref() == Some(crs))
            .map(|(index, _)| index)
            .collect()
    };
    let origin_calls = origin.map(at);
    let destination_calls = destination.map(at);
    if origin_calls.as_ref().is_some_and(Vec::is_empty) {
        return Err(LegCallError::DoesNotCallAtOrigin);
    }
    if destination_calls.as_ref().is_some_and(Vec::is_empty) {
        return Err(LegCallError::DoesNotCallAtDestination);
    }
    // The first call at the origin where boarding is allowed: any pairing
    // with a later alightable call works from there, so it is the only
    // one that matters. With no origin given, the train's own start.
    let board_from = match &origin_calls {
        Some(indices) => match indices.iter().copied().find(|&i| calls[i].can_board) {
            Some(index) => Some(index),
            None => return Err(LegCallError::BoardingNotAllowed),
        },
        None => None,
    };
    let Some(destination_calls) = destination_calls else {
        // No destination given: the train runs on to its terminus, which
        // lets everyone off, and a boardable call is never the terminus.
        return Ok(());
    };
    let later: Vec<usize> = destination_calls
        .into_iter()
        .filter(|&j| board_from.is_none_or(|i| j > i))
        .collect();
    if later.is_empty() {
        return Err(LegCallError::DestinationBeforeOrigin);
    }
    if later.iter().any(|&j| calls[j].can_alight) {
        Ok(())
    } else {
        Err(LegCallError::AlightingNotAllowed)
    }
}

/// A refused leg: the code and a sentence for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegRejection {
    pub error: LegCallError,
    pub message: String,
}

impl LegRejection {
    pub fn code(&self) -> &'static str {
        self.error.code()
    }
}

/// The sentence for `error`, naming the station (`name (CRS)`, or the bare
/// code when it has no name), the train and the date.
fn message(
    error: LegCallError,
    train_uid: &str,
    service_date: NaiveDate,
    origin: &str,
    destination: &str,
) -> String {
    let date = service_date.format("%-d %b %Y");
    match error {
        LegCallError::DoesNotCallAtOrigin => {
            format!("Train {train_uid} doesn't stop at {origin} on {date}.")
        }
        LegCallError::DoesNotCallAtDestination => {
            format!("Train {train_uid} doesn't stop at {destination} on {date}.")
        }
        LegCallError::DestinationBeforeOrigin => format!(
            "Train {train_uid} reaches {destination} before {origin} on {date}, so it can't take \
             you from {origin} to {destination}."
        ),
        LegCallError::BoardingNotAllowed => format!(
            "You can't board train {train_uid} at {origin} on {date}: it only lets passengers off \
             there."
        ),
        LegCallError::AlightingNotAllowed => format!(
            "You can't get off train {train_uid} at {destination} on {date}: it only picks \
             passengers up there."
        ),
    }
}

/// Checks `train_uid` on `service_date` from `origin_crs` to
/// `destination_crs` (normalised: trimmed, uppercase; either may be absent,
/// see [`check_calls`]). `Ok(None)` when the leg is fine or the train has no
/// published schedule for that date (see the module doc).
pub async fn validate_train_leg(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
) -> anyhow::Result<Option<LegRejection>> {
    if origin_crs.is_none() && destination_crs.is_none() {
        return Ok(None);
    }
    let rows =
        queries::list_schedule_calling_points_full_for_train(pool, train_uid, service_date).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut tiplocs: Vec<String> = rows.iter().map(|row| tiploc_key(&row.tiploc)).collect();
    tiplocs.sort();
    tiplocs.dedup();
    let tiploc_to_crs = queries::crs_for_tiplocs_batch(pool, &tiplocs).await?;
    let calls: Vec<Call> = rows
        .iter()
        .map(|row| Call {
            crs: tiploc_to_crs
                .get(&tiploc_key(&row.tiploc))
                .map(|crs| crs.trim().to_ascii_uppercase()),
            calls: row.booked_arrival.is_some() || row.booked_departure.is_some(),
            can_board: row.can_board.unwrap_or(true),
            can_alight: row.can_alight.unwrap_or(true),
        })
        .collect();
    let Err(error) = check_calls(&calls, origin_crs, destination_crs) else {
        return Ok(None);
    };

    let codes: Vec<String> = [origin_crs, destination_crs]
        .into_iter()
        .flatten()
        .map(str::to_string)
        .collect();
    let names: HashMap<String, String> = queries::station_names_for_crs_batch(pool, &codes)
        .await
        .unwrap_or_default();
    let label = |crs: Option<&str>| -> String {
        match crs {
            Some(crs) => match names.get(crs) {
                Some(name) => format!("{name} ({crs})"),
                None => crs.to_string(),
            },
            None => String::new(),
        }
    };
    Ok(Some(LegRejection {
        error,
        message: message(
            error,
            train_uid,
            service_date,
            &label(origin_crs),
            &label(destination_crs),
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(crs: &str, can_board: bool, can_alight: bool) -> Call {
        Call {
            crs: Some(crs.to_string()),
            calls: true,
            can_board,
            can_alight,
        }
    }

    fn pass(crs: &str) -> Call {
        Call {
            crs: Some(crs.to_string()),
            calls: false,
            can_board: false,
            can_alight: false,
        }
    }

    /// Euston -> Woking-ish line: origin, an ordinary stop, a pass, a
    /// set-down-only stop, a pick-up-only stop, a request stop, terminus.
    fn route() -> Vec<Call> {
        vec![
            call("EUS", true, false),
            call("WFJ", true, true),
            pass("BLY"),
            call("SDO", false, true),
            call("UPO", true, false),
            call("REQ", true, true),
            call("END", false, true),
        ]
    }

    #[test]
    fn an_ordinary_leg_and_open_ended_legs_pass() {
        let calls = route();
        assert_eq!(check_calls(&calls, Some("EUS"), Some("END")), Ok(()));
        assert_eq!(check_calls(&calls, Some("WFJ"), Some("SDO")), Ok(()));
        assert_eq!(check_calls(&calls, Some("UPO"), None), Ok(()));
        assert_eq!(check_calls(&calls, None, Some("SDO")), Ok(()));
        assert_eq!(check_calls(&calls, None, None), Ok(()));
    }

    #[test]
    fn a_request_stop_allows_both_boarding_and_alighting() {
        let calls = route();
        assert_eq!(check_calls(&calls, Some("REQ"), Some("END")), Ok(()));
        assert_eq!(check_calls(&calls, Some("WFJ"), Some("REQ")), Ok(()));
    }

    #[test]
    fn each_refusal_has_its_own_code() {
        let calls = route();
        let refuse = |o: Option<&str>, d: Option<&str>| check_calls(&calls, o, d).unwrap_err();
        assert_eq!(
            refuse(Some("XXX"), Some("END")),
            LegCallError::DoesNotCallAtOrigin
        );
        // A passing point is not a call.
        assert_eq!(
            refuse(Some("BLY"), Some("END")),
            LegCallError::DoesNotCallAtOrigin
        );
        assert_eq!(
            refuse(Some("EUS"), Some("BLY")),
            LegCallError::DoesNotCallAtDestination
        );
        assert_eq!(
            refuse(Some("REQ"), Some("WFJ")),
            LegCallError::DestinationBeforeOrigin
        );
        assert_eq!(
            refuse(Some("SDO"), Some("END")),
            LegCallError::BoardingNotAllowed
        );
        assert_eq!(refuse(Some("END"), None), LegCallError::BoardingNotAllowed);
        assert_eq!(
            refuse(Some("EUS"), Some("UPO")),
            LegCallError::AlightingNotAllowed
        );
        assert_eq!(refuse(None, Some("EUS")), LegCallError::AlightingNotAllowed);
        assert_eq!(
            LegCallError::DestinationBeforeOrigin.code(),
            "destination_before_origin"
        );
    }

    #[test]
    fn a_train_calling_twice_is_fine_on_any_boardable_then_alightable_pair() {
        // A loop: AAA, BBB, AAA again, CCC.
        let calls = vec![
            call("AAA", true, false),
            call("BBB", true, true),
            call("AAA", true, true),
            call("CCC", false, true),
        ];
        assert_eq!(check_calls(&calls, Some("BBB"), Some("AAA")), Ok(()));
        assert_eq!(check_calls(&calls, Some("AAA"), Some("BBB")), Ok(()));
    }

    #[test]
    fn messages_name_the_train_station_and_date_without_codes() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let text = message(
            LegCallError::BoardingNotAllowed,
            "C12345",
            date,
            "Woking (WOK)",
            "Reading (RDG)",
        );
        assert_eq!(
            text,
            "You can't board train C12345 at Woking (WOK) on 20 Oct 2026: it only lets \
             passengers off there."
        );
        for error in [
            LegCallError::DoesNotCallAtOrigin,
            LegCallError::DoesNotCallAtDestination,
            LegCallError::DestinationBeforeOrigin,
            LegCallError::BoardingNotAllowed,
            LegCallError::AlightingNotAllowed,
        ] {
            let text = message(error, "C12345", date, "Woking (WOK)", "Reading (RDG)");
            // The frontend shows an API sentence only when it has no
            // underscore, brace or angle bracket and is at most 240 chars
            // (`frontend/lib/failure.ts`).
            assert!(text.len() <= 240, "{text}");
            assert!(
                !text.contains(['_', '{', '}', '<', '>', '`', '\n']),
                "{text}"
            );
        }
    }
}
