//! Resolves a live departure-board row (station, time, retail service ID,
//! destination, operator) to the CIF `(train_uid, service_date)` key
//! `GET /Train/by-uid/{uid}/{date}` takes. Backs
//! `GET /public/trains/resolve` (`routes::trains::get_trains_resolve`).
//!
//! An LDBWS board row carries no CIF `uid` and no Darwin `rid`; the only
//! identifier it shares with the CIF timetable is the Retail Service ID
//! (`rsid`), which `schedule-reference` publishes onto every
//! `schedule_destination_departures` row (see
//! `schedule_query::records::BasicSchedule::rsid`). An RSID is not unique
//! on its own -- measured on the real `RJTTF971MCA` extract (2026-09-26),
//! 5-18 RSIDs per day are shared by several UIDs (Heathrow Express uses one
//! RSID for a whole group of services), but no two UIDs shared an RSID at
//! the same calling point and working time, nor within +-5 minutes of each
//! other there. So this always keys on (station, local time window) first
//! and uses the RSID to pick within it.
//!
//! Split into one read ([`resolve_candidates`]) and one pure decision
//! ([`resolve`]) so the matching rules are unit-testable without a
//! database.

use chrono::{NaiveDate, NaiveDateTime};
use sqlx::PgPool;

/// Half-width of the window an RSID match may sit in around the board's
/// time. Wider than [`TIMETABLE_WINDOW_MINUTES`] because an RSID already
/// identifies the service; the window only has to absorb the gap between
/// the board's PUBLIC time and the stored WORKING time (normally <= 1
/// minute) with margin, while still separating the repeats of a shared
/// RSID (the tightest real repeat measured was 15 minutes apart).
pub const RSID_WINDOW_MINUTES: i64 = 5;

/// Half-width of the time-only (no RSID) timetable heuristic's window --
/// the `std +- 2 minutes` documented on
/// `routes::departures::get_station_departures`.
pub const TIMETABLE_WINDOW_MINUTES: i64 = 2;

/// Whether the board row is a departure from, or an arrival at, the station.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveKind {
    Departure,
    Arrival,
}

/// How a resolved train was identified. Rendered as `matchedOn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedOn {
    /// The full 8-character RSID matched.
    Rsid,
    /// Only the first 6 characters (ATOC prefix + service number, i.e.
    /// without the 2-digit portion suffix) matched.
    RsidPrefix,
    /// No RSID was usable; time plus destination/operator decided.
    Timetable,
}

impl MatchedOn {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchedOn::Rsid => "rsid",
            MatchedOn::RsidPrefix => "rsidPrefix",
            MatchedOn::Timetable => "timetable",
        }
    }
}

/// One stored calling at the requested station inside the widest window,
/// with its event (departure or arrival) as a London-local date-time.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ResolveCandidate {
    pub train_uid: String,
    pub service_date: NaiveDate,
    pub rsid: Option<String>,
    pub destination_crs: String,
    pub operator_atoc: Option<String>,
    pub at: NaiveDateTime,
}

/// The caller's board row, already validated and normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    /// London-local date-time of the board's scheduled time at the station.
    pub target: NaiveDateTime,
    /// Uppercased; 6-8 ASCII alphanumerics.
    pub rsid: Option<String>,
    /// Uppercased CRS of the train's destination.
    pub destination: Option<String>,
    /// Uppercased 2-character ATOC code.
    pub operator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Found {
        train_uid: String,
        service_date: NaiveDate,
        matched_on: MatchedOn,
    },
    NotFound,
    /// More than one distinct `(train_uid, service_date)`, sorted.
    Ambiguous(Vec<(String, NaiveDate)>),
}

fn within(c: &ResolveCandidate, target: NaiveDateTime, minutes: i64) -> bool {
    (c.at - target).num_seconds().abs() <= minutes * 60
}

fn distinct_keys<'a>(
    candidates: impl Iterator<Item = &'a ResolveCandidate>,
) -> Vec<(String, NaiveDate)> {
    let mut keys: Vec<(String, NaiveDate)> = candidates
        .map(|c| (c.train_uid.clone(), c.service_date))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

fn matches_filters(c: &ResolveCandidate, request: &ResolveRequest) -> bool {
    request
        .destination
        .as_deref()
        .is_none_or(|d| c.destination_crs == d)
        && request
            .operator
            .as_deref()
            .is_none_or(|o| c.operator_atoc.as_deref() == Some(o))
}

#[expect(
    clippy::expect_used,
    reason = "the invariant is established just above; the expect message names it"
)]
fn single_or_ambiguous(keys: Vec<(String, NaiveDate)>, matched_on: MatchedOn) -> ResolveOutcome {
    match keys.len() {
        0 => ResolveOutcome::NotFound,
        1 => {
            let (train_uid, service_date) = keys.into_iter().next().expect("len checked");
            ResolveOutcome::Found {
                train_uid,
                service_date,
                matched_on,
            }
        }
        _ => ResolveOutcome::Ambiguous(keys),
    }
}

/// An RSID hit that names exactly one train wins outright -- the RSID is
/// the stronger key, so a destination/operator that disagrees (a board
/// showing a portion's own destination, say) does not veto it. Several
/// trains are narrowed by destination/operator; if that leaves exactly one,
/// it wins; otherwise the caller gets every candidate the narrowing kept
/// (or, if it kept none, every RSID hit) as a 409, never a guess.
fn narrow_rsid_hits(
    hits: &[&ResolveCandidate],
    request: &ResolveRequest,
    matched_on: MatchedOn,
) -> ResolveOutcome {
    let all = distinct_keys(hits.iter().copied());
    if all.len() <= 1 {
        return single_or_ambiguous(all, matched_on);
    }
    let narrowed = distinct_keys(hits.iter().copied().filter(|c| matches_filters(c, request)));
    if narrowed.is_empty() {
        ResolveOutcome::Ambiguous(all)
    } else {
        single_or_ambiguous(narrowed, matched_on)
    }
}

/// The matching rules, in order:
///
/// 1. With `rsid`: calls within [`RSID_WINDOW_MINUTES`] carrying exactly that
///    RSID; if none, those whose RSID shares its first 6 characters (the
///    service number without the portion suffix). Either set is narrowed
///    per [`narrow_rsid_hits`].
/// 2. If neither matched, and some call in that window DOES carry an RSID,
///    the board's train is not in the published timetable under that ID:
///    `NotFound`, not a timetable guess. Only when no call in the window
///    carries any RSID at all (rows published before the RSID was, or a
///    schedule whose `BX` record left it blank) does it fall through to 3.
/// 3. Without a usable `rsid`: the timetable heuristic -- calls within
///    [`TIMETABLE_WINDOW_MINUTES`] that match every supplied
///    destination/operator. Exactly one train, or nothing.
pub fn resolve(candidates: &[ResolveCandidate], request: &ResolveRequest) -> ResolveOutcome {
    if let Some(rsid) = request.rsid.as_deref() {
        let in_window: Vec<&ResolveCandidate> = candidates
            .iter()
            .filter(|c| within(c, request.target, RSID_WINDOW_MINUTES))
            .collect();
        let exact: Vec<&ResolveCandidate> = in_window
            .iter()
            .copied()
            .filter(|c| c.rsid.as_deref() == Some(rsid))
            .collect();
        if !exact.is_empty() {
            return narrow_rsid_hits(&exact, request, MatchedOn::Rsid);
        }
        let prefix = &rsid[..rsid.len().min(6)];
        let prefixed: Vec<&ResolveCandidate> = in_window
            .iter()
            .copied()
            .filter(|c| c.rsid.as_deref().and_then(|r| r.get(..6)) == Some(prefix))
            .collect();
        if !prefixed.is_empty() {
            return narrow_rsid_hits(&prefixed, request, MatchedOn::RsidPrefix);
        }
        if in_window.iter().any(|c| c.rsid.is_some()) {
            return ResolveOutcome::NotFound;
        }
    }

    single_or_ambiguous(
        distinct_keys(
            candidates
                .iter()
                .filter(|c| within(c, request.target, TIMETABLE_WINDOW_MINUTES))
                .filter(|c| matches_filters(c, request)),
        ),
        MatchedOn::Timetable,
    )
}

/// Every service date from `first` to `last` inclusive (3-4 in practice).
/// Bound as `service_date = ANY($dates)` rather than `BETWEEN`: every index
/// on `schedule_destination_departures` leads with `service_date`, and on
/// Postgres 16 (no skip scan) a range on the leading column scans every
/// entry for those dates, filtering on the CRS, whereas equality probes
/// can seek on `(service_date, origin_crs)` / `(service_date,
/// destination_crs)` too (DB2-10; prod EXPLAIN 2026-09-27 for EUS:
/// 4,576 buffers / 34.6 ms -> 586 buffers / 0.9 ms).
fn candidate_service_dates(first: NaiveDate, last: NaiveDate) -> Vec<NaiveDate> {
    first.iter_days().take_while(|d| *d <= last).collect()
}

/// Every stored call at `station` whose event falls within
/// [`RSID_WINDOW_MINUTES`] of `target` (both London-local), on any service
/// date that could reach it -- the same date with `day_offset = 0` and, just
/// after midnight, the previous date with `day_offset = 1` (the event
/// date-time is `(service_date + day_offset) + time`, so a window spanning
/// midnight works too).
///
/// * `Departure`: rows whose own calling point is `station`, at `scheduled`
///   (the WORKING departure). Only boardable calls have rows at all -- see
///   `schedule_query::resolve::departures_by_destination_crs`.
/// * `Arrival`: those same rows at their `calling_point_arrival` (the
///   arrival's day is one earlier than the row's `day_offset` when the train
///   dwells across midnight), plus the terminus, which has no row of its
///   own: rows with `destination_crs = station`, at `destination_arrival` /
///   `destination_arrival_day_offset`. A set-down-only intermediate stop has
///   no row either, so an arrival there cannot be resolved.
pub async fn resolve_candidates(
    pool: &PgPool,
    station: &str,
    target: NaiveDateTime,
    kind: ResolveKind,
) -> anyhow::Result<Vec<ResolveCandidate>> {
    let window = chrono::Duration::minutes(RSID_WINDOW_MINUTES);
    let earliest = target - window;
    let latest = target + window;
    // A row's service_date is at most its day_offset (0..=2 in practice)
    // before the event's own date.
    let service_dates =
        candidate_service_dates(earliest.date() - chrono::Duration::days(2), latest.date());

    let sql = match kind {
        ResolveKind::Departure => {
            "SELECT train_uid, service_date, rsid, destination_crs, operator_atoc, at FROM ( \
                SELECT train_uid, service_date, rsid, destination_crs, operator_atoc, \
                       (service_date + day_offset::int) + scheduled AS at \
                  FROM schedule_destination_departures \
                 WHERE origin_crs = $1 AND service_date = ANY($2) \
             ) d WHERE at BETWEEN $3 AND $4"
        }
        ResolveKind::Arrival => {
            "SELECT DISTINCT train_uid, service_date, rsid, destination_crs, operator_atoc, at FROM ( \
                SELECT train_uid, service_date, rsid, destination_crs, operator_atoc, \
                       (service_date + day_offset::int) + calling_point_arrival \
                         - CASE WHEN calling_point_arrival > scheduled \
                                THEN interval '1 day' ELSE interval '0' END AS at \
                  FROM schedule_destination_departures \
                 WHERE origin_crs = $1 AND service_date = ANY($2) \
                   AND calling_point_arrival IS NOT NULL \
                UNION ALL \
                SELECT train_uid, service_date, rsid, destination_crs, operator_atoc, \
                       (service_date + destination_arrival_day_offset::int) + destination_arrival AS at \
                  FROM schedule_destination_departures \
                 WHERE destination_crs = $1 AND service_date = ANY($2) \
                   AND destination_arrival IS NOT NULL \
             ) a WHERE at BETWEEN $3 AND $4"
        }
    };

    Ok(sqlx::query_as::<_, ResolveCandidate>(sql)
        .bind(station)
        .bind(&service_dates)
        .bind(earliest)
        .bind(latest)
        .fetch_all(pool)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_service_dates_is_every_date_in_the_inclusive_range() {
        let d = |s: &str| s.parse::<NaiveDate>().unwrap();
        assert_eq!(
            candidate_service_dates(d("2026-09-25"), d("2026-09-27")),
            vec![d("2026-09-25"), d("2026-09-26"), d("2026-09-27")]
        );
        assert_eq!(
            candidate_service_dates(d("2026-09-27"), d("2026-09-27")),
            vec![d("2026-09-27")]
        );
        assert!(candidate_service_dates(d("2026-09-28"), d("2026-09-27")).is_empty());
    }

    fn at(date: &str, time: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M").unwrap()
    }

    fn cand(
        uid: &str,
        rsid: Option<&str>,
        dest: &str,
        op: Option<&str>,
        when: NaiveDateTime,
    ) -> ResolveCandidate {
        ResolveCandidate {
            train_uid: uid.to_string(),
            service_date: when.date(),
            rsid: rsid.map(str::to_string),
            destination_crs: dest.to_string(),
            operator_atoc: op.map(str::to_string),
            at: when,
        }
    }

    fn req(
        target: NaiveDateTime,
        rsid: Option<&str>,
        dest: Option<&str>,
        op: Option<&str>,
    ) -> ResolveRequest {
        ResolveRequest {
            target,
            rsid: rsid.map(str::to_string),
            destination: dest.map(str::to_string),
            operator: op.map(str::to_string),
        }
    }

    fn found(uid: &str, matched_on: MatchedOn) -> impl Fn(&ResolveOutcome) -> bool + '_ {
        move |o| matches!(o, ResolveOutcome::Found { train_uid, matched_on: m, .. } if train_uid == uid && *m == matched_on)
    }

    #[test]
    fn an_exact_rsid_beats_a_same_time_train_and_ignores_a_disagreeing_destination() {
        let t = at("2026-09-27", "10:00");
        let c = [
            cand(
                "A",
                Some("SR408800"),
                "GLC",
                Some("SR"),
                at("2026-09-27", "10:01"),
            ),
            cand("B", Some("SR999900"), "EDB", Some("SR"), t),
        ];
        let o = resolve(&c, &req(t, Some("SR408800"), Some("EDB"), None));
        assert!(found("A", MatchedOn::Rsid)(&o), "{o:?}");
    }

    #[test]
    fn a_shared_rsid_outside_the_window_is_not_a_candidate() {
        // Heathrow-Express shape: one RSID, departures 15 minutes apart.
        let t = at("2026-09-27", "10:00");
        let c = [
            cand("A", Some("HX010100"), "HXX", Some("HX"), t),
            cand(
                "B",
                Some("HX010100"),
                "HXX",
                Some("HX"),
                at("2026-09-27", "10:15"),
            ),
        ];
        assert!(found("A", MatchedOn::Rsid)(&resolve(
            &c,
            &req(t, Some("HX010100"), None, None)
        )));
    }

    #[test]
    fn prefix_fallback_then_destination_narrowing_then_409() {
        let t = at("2026-09-27", "10:00");
        let c = [
            cand("A", Some("SE123401"), "RAM", Some("SE"), t),
            cand("B", Some("SE123402"), "DVP", Some("SE"), t),
        ];
        assert_eq!(
            resolve(&c, &req(t, Some("SE123400"), None, None)),
            ResolveOutcome::Ambiguous(vec![("A".into(), t.date()), ("B".into(), t.date())])
        );
        assert!(found("B", MatchedOn::RsidPrefix)(&resolve(
            &c,
            &req(t, Some("SE123400"), Some("DVP"), None)
        )));
        // A destination neither portion has does not fabricate a winner.
        assert!(matches!(
            resolve(&c, &req(t, Some("SE123400"), Some("CBW"), None)),
            ResolveOutcome::Ambiguous(k) if k.len() == 2
        ));
    }

    #[test]
    fn an_unmatched_rsid_among_rsid_bearing_rows_is_not_found_not_a_timetable_guess() {
        let t = at("2026-09-27", "10:00");
        let c = [cand("A", Some("GW100000"), "PAD", Some("GW"), t)];
        assert_eq!(
            resolve(&c, &req(t, Some("XC555500"), Some("PAD"), Some("GW"))),
            ResolveOutcome::NotFound
        );
    }

    #[test]
    fn rows_without_any_rsid_fall_back_to_the_two_minute_timetable_match() {
        let t = at("2026-09-27", "10:00");
        let c = [
            cand("A", None, "PAD", Some("GW"), at("2026-09-27", "10:01")),
            cand("B", None, "BRI", Some("GW"), t),
            cand("C", None, "PAD", Some("GW"), at("2026-09-27", "10:04")),
        ];
        assert!(found("A", MatchedOn::Timetable)(&resolve(
            &c,
            &req(t, Some("GW100000"), Some("PAD"), None)
        )));
        assert!(found("A", MatchedOn::Timetable)(&resolve(
            &c,
            &req(t, None, Some("PAD"), Some("GW"))
        )));
        assert!(
            matches!(resolve(&c, &req(t, None, None, None)), ResolveOutcome::Ambiguous(k) if k.len() == 2)
        );
        assert_eq!(
            resolve(&c, &req(t, None, Some("PAD"), Some("XC"))),
            ResolveOutcome::NotFound
        );
    }
}
