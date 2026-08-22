//! Parses TfL's `GET /Line/dlr/Timetable/{stopPointId}` response for one
//! fixed pilot station (Poplar — see the plan's Global Constraints for why
//! this pilot doesn't cover the whole network). `knownJourneys[]` gives
//! each scheduled departure as an `hour`/`minute` pair with no date; this
//! module combines each with the `service_date` the caller is asking
//! about (`chrono::Utc::now()`'s date, threaded in the same way
//! `poller-tfl/src/schema.rs::parse_line_status` threads `now` — never
//! read directly, so parsing stays deterministic under test).

use anyhow::Result;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc, Weekday};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TimetableResponse {
    timetable: Timetable,
}

#[derive(Debug, Deserialize)]
struct Timetable {
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Route {
    #[serde(default)]
    schedules: Vec<Schedule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    name: String,
    #[serde(default)]
    known_journeys: Vec<KnownJourney>,
}

#[derive(Debug, Deserialize)]
struct KnownJourney {
    hour: String,
    minute: String,
    // A Task 2 live capture found this is a JSON integer in practice (e.g.
    // `0`, `1`), not the string the plan originally assumed — see
    // `crates/poller-tfl/tests/fixtures/README.md` finding 3. Deserializing
    // it into a `String` field fails against real data, so it's typed
    // numeric here instead.
    #[serde(default)]
    #[serde(rename = "intervalId")]
    interval_id: Option<i64>,
}

/// One scheduled DLR departure from the pilot station, resolved to a real
/// timestamp for `service_date`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledTrip {
    pub scheduled_departure: DateTime<Utc>,
    /// TfL's Timetable response does not carry a destination per journey
    /// the way Arrivals does — only a route-level `intervalId` grouping.
    /// Matching (Task 5) does not use this field yet; kept for a future
    /// iteration that resolves `intervalId` to a real destination via
    /// `timetable.routes[].stationIntervals[]`, which this pilot does not
    /// parse.
    pub interval_id: Option<i64>,
}

/// The `schedules[].name` this pilot selects for a given `service_date`,
/// per the day-of-week rule below.
fn expected_schedule_name(service_date: NaiveDate) -> &'static str {
    // A Task 2 live capture found each route's `schedules[]` holds one
    // entry per day-type (observed names: "Monday - Friday", "Sunday",
    // "Saturdays and Public Holidays"), each carrying its own complete
    // `knownJourneys[]` list for that day-type only — not one flat pool of
    // departures shared across every day, as the plan originally assumed.
    // See `crates/poller-tfl/tests/fixtures/README.md` finding 4. Parsing
    // must therefore pick the one schedule matching `service_date`'s day
    // of week; flattening every schedule together would combine weekday
    // and weekend departures into a single inflated (~3x) trip count, and
    // Task 6 promotes unmatched scheduled trips to "cancelled" — so that
    // bug would manufacture large numbers of phantom cancellations. This
    // is a pilot-tier approximation: it does not special-case real public
    // holidays, so a Bank Holiday Monday will incorrectly use the weekday
    // schedule. That's accepted as out of scope for this pilot.
    match service_date.weekday() {
        Weekday::Sat => "Saturdays and Public Holidays",
        Weekday::Sun => "Sunday",
        _ => "Monday - Friday",
    }
}

pub fn parse_timetable(json: &str, service_date: NaiveDate) -> Result<Vec<ScheduledTrip>> {
    let response: TimetableResponse = serde_json::from_str(json)?;
    let expected_name = expected_schedule_name(service_date);
    let mut trips = Vec::new();
    for route in &response.timetable.routes {
        // If a route has no schedule matching the expected day-type name
        // (e.g. a spelling quirk on that particular route), skip its
        // contribution silently rather than erroring — a partial Poplar
        // timetable degrades better than one route's naming quirk
        // crashing the whole parse.
        let Some(schedule) = route.schedules.iter().find(|s| s.name == expected_name) else {
            continue;
        };
        for journey in &schedule.known_journeys {
            let hour: u32 = journey.hour.parse()?;
            let minute: u32 = journey.minute.parse()?;
            let naive = service_date.and_hms_opt(hour, minute, 0).ok_or_else(|| {
                anyhow::anyhow!("invalid knownJourney time {}:{}", journey.hour, journey.minute)
            })?;
            trips.push(ScheduledTrip {
                scheduled_departure: Utc.from_utc_datetime(&naive),
                interval_id: journey.interval_id,
            });
        }
    }
    Ok(trips)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Adapted from crates/poller-tfl/tests/fixtures/dlr_timetable_poplar.json,
    // trimmed to two routes each carrying "Monday - Friday" and "Saturdays
    // and Public Holidays" schedules, so the day-of-week selection logic
    // has something to distinguish. intervalId is a JSON integer, matching
    // the real capture (see fixtures/README.md finding 3).
    const DLR_TIMETABLE_JSON: &str = r#"{
      "lineId": "dlr",
      "lineName": "DLR",
      "direction": "outbound",
      "timetable": {
        "departureStopId": "940GZZDLPOP",
        "routes": [
          {
            "stationIntervals": [],
            "schedules": [
              {
                "name": "Monday - Friday",
                "knownJourneys": [
                  { "hour": "10", "minute": "02", "intervalId": 1 },
                  { "hour": "10", "minute": "06", "intervalId": 2 }
                ]
              },
              {
                "name": "Sunday",
                "knownJourneys": [
                  { "hour": "11", "minute": "15", "intervalId": 0 }
                ]
              },
              {
                "name": "Saturdays and Public Holidays",
                "knownJourneys": [
                  { "hour": "09", "minute": "45", "intervalId": 0 }
                ]
              }
            ]
          },
          {
            "stationIntervals": [],
            "schedules": [
              {
                "name": "Monday - Friday",
                "knownJourneys": [
                  { "hour": "10", "minute": "04", "intervalId": 1 }
                ]
              },
              {
                "name": "Saturdays and Public Holidays",
                "knownJourneys": [
                  { "hour": "09", "minute": "50", "intervalId": 1 }
                ]
              }
            ]
          }
        ]
      }
    }"#;

    fn weekday_service_date() -> NaiveDate {
        // 2026-08-22 is a Saturday; use the following Monday for weekday tests.
        NaiveDate::from_ymd_opt(2026, 8, 24).unwrap()
    }

    fn saturday_service_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 22).unwrap()
    }

    #[test]
    fn parses_known_journeys_into_scheduled_trips_on_the_given_date() {
        let trips = parse_timetable(DLR_TIMETABLE_JSON, weekday_service_date()).expect("should parse");
        // 2 journeys from route 0's "Monday - Friday" + 1 from route 1's "Monday - Friday" = 3.
        assert_eq!(trips.len(), 3);
        assert_eq!(
            trips[0].scheduled_departure,
            "2026-08-24T10:02:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(trips[0].interval_id, Some(1));
        assert_eq!(
            trips[1].scheduled_departure,
            "2026-08-24T10:06:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            trips[2].scheduled_departure,
            "2026-08-24T10:04:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn a_saturday_service_date_only_pulls_from_the_saturday_schedule() {
        let trips = parse_timetable(DLR_TIMETABLE_JSON, saturday_service_date()).expect("should parse");
        // 1 journey from route 0's "Saturdays and Public Holidays" + 1 from route 1's = 2.
        assert_eq!(trips.len(), 2);
        assert_eq!(
            trips[0].scheduled_departure,
            "2026-08-22T09:45:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            trips[1].scheduled_departure,
            "2026-08-22T09:50:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn a_route_missing_the_expected_schedule_name_is_skipped_not_errored() {
        let json = r#"{
          "lineId": "dlr",
          "lineName": "DLR",
          "direction": "outbound",
          "timetable": {
            "departureStopId": "940GZZDLPOP",
            "routes": [
              {
                "schedules": [
                  { "name": "Sunday", "knownJourneys": [{ "hour": "11", "minute": "00", "intervalId": 0 }] }
                ]
              }
            ]
          }
        }"#;
        let trips = parse_timetable(json, weekday_service_date()).expect("should parse");
        assert!(trips.is_empty());
    }

    #[test]
    fn a_response_with_no_journeys_parses_to_an_empty_list() {
        let json = r#"{"lineId":"dlr","lineName":"DLR","direction":"outbound","timetable":{"departureStopId":"940GZZDLPOP","routes":[]}}"#;
        assert!(parse_timetable(json, weekday_service_date()).expect("should parse").is_empty());
    }
}
