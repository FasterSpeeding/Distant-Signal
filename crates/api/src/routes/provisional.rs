//! Whether a service date's published timetable is still provisional
//! (2026-10-08).
//!
//! `schedule-reference` publishes the per-date timetable products
//! `SCHEDULE_FORWARD_PUBLISH_DAYS` ahead (28 by default, 7 before). A far
//! date's rows are the CIF as it stands today, and the short-term plan for
//! that date is not finished: STP overlays and cancellations for
//! engineering works and other late changes keep arriving in the daily CIF
//! updates until a few weeks before the day, so a train listed four weeks
//! out may yet be retimed, replaced by a bus or removed.
//!
//! **The rule**: a service date is provisional when it is more than
//! `SCHEDULE_PROVISIONAL_AFTER_DAYS` (default 7, the publish horizon these
//! surfaces had before the window grew) days after today, London time:
//! `service_date > today + N`, so `provisionalFrom = today + N + 1`.
//!
//! **Why a date rule and not a per-row signal.** `schedule_services.stp` is
//! the obvious candidate, and it does not answer the question. It records
//! which CIF record WON for a date (`P` permanent, `O` overlay, `N` new),
//! not whether the short-term plan for that date is complete: a `P` winner
//! four weeks out is exactly the row a late overlay would still replace,
//! and its being `P` today says nothing about whether one is coming; an
//! `O`/`N` winner can itself be superseded or cancelled by a later STP
//! record. Production (2026-10-08) shows 10-13% non-`P` winners on
//! weekdays and 38-56% at the weekend across the whole seven-day window,
//! i.e. the indicator tracks engineering-works weekends, not distance from
//! today. CIF carries no "plan finalised" marker per date, so the distance
//! from today is the only signal the data has. It is applied per response:
//! every response that carries it is for exactly one service date.
//!
//! The flag is advisory only: nothing is filtered on it.

use std::sync::LazyLock;

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{Map, Value};

/// Days after today a service date's timetable is still treated as firm.
/// Seven: the forward publish window before it was widened to 28 days,
/// the horizon every surface showed until then.
pub(crate) const DEFAULT_PROVISIONAL_AFTER_DAYS: i64 = 7;

/// The highest accepted setting: `schedule-reference`'s own ceiling on
/// `SCHEDULE_FORWARD_PUBLISH_DAYS`, so 60 marks nothing provisional.
pub(crate) const MAX_PROVISIONAL_AFTER_DAYS: i64 = 60;

const PROVISIONAL_AFTER_DAYS_ENV: &str = "SCHEDULE_PROVISIONAL_AFTER_DAYS";

/// `SCHEDULE_PROVISIONAL_AFTER_DAYS`, read once.
static PROVISIONAL_AFTER_DAYS: LazyLock<i64> =
    LazyLock::new(|| parse_after_days(std::env::var(PROVISIONAL_AFTER_DAYS_ENV).ok().as_deref()));

/// Parses the setting: an integer in `0..=MAX_PROVISIONAL_AFTER_DAYS`;
/// unset is the default, anything else is the default with a warning.
fn parse_after_days(raw: Option<&str>) -> i64 {
    let Some(raw) = raw else {
        return DEFAULT_PROVISIONAL_AFTER_DAYS;
    };
    match raw.trim().parse::<i64>() {
        Ok(days) if (0..=MAX_PROVISIONAL_AFTER_DAYS).contains(&days) => days,
        _ => {
            tracing::warn!(
                name = PROVISIONAL_AFTER_DAYS_ENV,
                raw,
                default = DEFAULT_PROVISIONAL_AFTER_DAYS,
                "invalid value (want 0-{MAX_PROVISIONAL_AFTER_DAYS}); using the default"
            );
            DEFAULT_PROVISIONAL_AFTER_DAYS
        }
    }
}

/// The first provisional service date: `today + N + 1`.
pub(crate) fn provisional_from(today: NaiveDate) -> NaiveDate {
    provisional_from_with(today, *PROVISIONAL_AFTER_DAYS)
}

fn provisional_from_with(today: NaiveDate, after_days: i64) -> NaiveDate {
    today + chrono::Duration::days(after_days + 1)
}

/// The two response-level fields: `provisional` (this response's service
/// date is on or after `provisionalFrom`) and `provisionalFrom` (the first
/// provisional date, `"YYYY-MM-DD"`, so a client can tell where the line
/// falls without its own copy of the setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimetableCertainty {
    pub(crate) provisional: bool,
    pub(crate) provisional_from: NaiveDate,
}

impl TimetableCertainty {
    /// The fields for `service_date`, against London `today`.
    pub(crate) fn for_date(service_date: NaiveDate, today: NaiveDate) -> Self {
        Self::from_threshold(service_date, provisional_from(today))
    }

    fn from_threshold(service_date: NaiveDate, provisional_from: NaiveDate) -> Self {
        Self {
            provisional: service_date >= provisional_from,
            provisional_from,
        }
    }

    /// Adds `provisional` and `provisionalFrom` to a JSON response object.
    pub(crate) fn insert_into(self, object: &mut Map<String, Value>) {
        object.insert("provisional".to_string(), Value::Bool(self.provisional));
        object.insert(
            "provisionalFrom".to_string(),
            Value::String(self.provisional_from.format("%Y-%m-%d").to_string()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parse_after_days_defaults_and_bounds() {
        assert_eq!(parse_after_days(None), 7);
        assert_eq!(parse_after_days(Some(" 14 ")), 14);
        assert_eq!(parse_after_days(Some("0")), 0);
        assert_eq!(parse_after_days(Some("60")), 60);
        assert_eq!(parse_after_days(Some("61")), 7);
        assert_eq!(parse_after_days(Some("-1")), 7);
        assert_eq!(parse_after_days(Some("week")), 7);
    }

    #[test]
    fn provisional_starts_the_day_after_today_plus_n() {
        let today = d(2026, 10, 8);
        let threshold = provisional_from_with(today, 7);
        assert_eq!(threshold, d(2026, 10, 16));
        let firm = TimetableCertainty::from_threshold(d(2026, 10, 15), threshold);
        assert!(!firm.provisional, "today + 7 is still firm");
        let far = TimetableCertainty::from_threshold(d(2026, 10, 16), threshold);
        assert!(far.provisional, "today + 8 is provisional");
        assert!(!TimetableCertainty::from_threshold(today, threshold).provisional);
        assert!(!TimetableCertainty::from_threshold(d(2026, 10, 1), threshold).provisional);
    }

    #[test]
    fn zero_days_marks_tomorrow_provisional() {
        let today = d(2026, 10, 8);
        let threshold = provisional_from_with(today, 0);
        assert!(!TimetableCertainty::from_threshold(today, threshold).provisional);
        assert!(TimetableCertainty::from_threshold(d(2026, 10, 9), threshold).provisional);
    }

    #[test]
    fn serializes_and_inserts_camel_case_fields() {
        let certainty = TimetableCertainty::from_threshold(d(2026, 11, 1), d(2026, 10, 16));
        assert_eq!(
            serde_json::to_value(certainty).unwrap(),
            serde_json::json!({"provisional": true, "provisionalFrom": "2026-10-16"})
        );
        let mut object = Map::new();
        certainty.insert_into(&mut object);
        assert_eq!(
            Value::Object(object),
            serde_json::json!({"provisional": true, "provisionalFrom": "2026-10-16"})
        );
    }
}
