//! "Recently cleared": Knowledgebase incidents RDM has cleared that its
//! feed still lists, for an info-only section on the line, station and
//! status pages (user decision, 2026-10-12). Nothing here touches a line's
//! severity: the aggregator ignores cleared incidents, as before.
//!
//! # The window
//!
//! An incident is listed while all of:
//!
//! - it is cleared (`is_cleared`) and not "Ended (no longer listed)";
//! - the feed still lists it: its `fetched_at` is within
//!   [`LISTED_WITHIN_SECS`] of the latest snapshot
//!   (`ingest_freshness.fetched_at` for `incidents`). Both snapshot writers
//!   bump every listed cleared row's `fetched_at` in both row-heartbeat
//!   modes (`ds_store::incidents`' module docs), and nothing else records
//!   whether a cleared row is still listed: `source_missing_polls` never
//!   counts a cleared row's misses. A snapshot's chunks all land within a
//!   few seconds of each other, while the previous snapshot is a whole poll
//!   interval (300 s) earlier, so 120 s separates "in the latest snapshot"
//!   from "last seen one poll ago" with room either side (the same bound as
//!   `removal::MIN_INFERENCE_GAP_SECS`). RDM purges cleared incidents
//!   nightly at about 23:00 UTC, which ends most items' listing;
//! - it was cleared in the current rail day (since 02:00 Europe/London),
//!   so an incident the nightly purge somehow misses still drops off.
//!
//! # When it was cleared, and what was expected
//!
//! `clearedAt` is the first `incident_history` row of the current cleared
//! run: the first cleared row after the last uncleared one (a reopen starts
//! a new run). No `cleared_at` column: `apply_snapshot` has written a
//! history row on every `is_cleared` flip since 2026-10-06, the history is
//! never pruned, and the same lateral read has to fetch the last uncleared
//! row anyway for its text, so a column would add a migration, an upsert
//! branch and a backfill without saving a query. An incident with no
//! cleared history row (only possible for a clear before 2026-10-06) is
//! left out rather than given an invented time.
//!
//! The last uncleared text is searched for its stated end ([`expected_end`]),
//! "Disruption is expected until 22:30" or "expected until the end of the
//! day", with a regex over RDM's phrasing, never the LLM. Validated on
//! 2026-10-12 against 883 production clears (39 days): 822 of the 855 that
//! had pre-clear text match (96.1%; 656 clock times, 166 end of day), and a
//! manual review of every match not phrased "expected until" found no false
//! positive. The misses are mostly deliberate: an end on a later day
//! ("expected until 04:00 on Tuesday 8 September") and ticket acceptance
//! ("(until the end of the day)").

use std::sync::LazyLock;

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveTime, Utc};
use regex::Regex;
use sqlx::PgPool;

/// How close to the latest snapshot a cleared row's `fetched_at` must be
/// for the feed to still list it (module docs).
pub const LISTED_WITHIN_SECS: i32 = 120;

/// The start of `now`'s rail day: 02:00 Europe/London today, or yesterday
/// before 02:00.
#[expect(clippy::expect_used, reason = "a constant time is always valid")]
pub fn rail_day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    common::rail_day::london_to_utc(
        common::rail_day::current_rail_day(now),
        NaiveTime::from_hms_opt(2, 0, 0).expect("02:00 is a valid time"),
    )
}

/// The end the last uncleared text stated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedEnd {
    /// "expected until 22:30", as an instant.
    At(DateTime<Utc>),
    /// "expected until the end of the day" (or "of service").
    EndOfDay,
}

/// RDM's ways of stating when disruption ends. Anchored on the words that
/// introduce an end ("expected", "continue", "disruption", "in place",
/// "service") or on "until approximately", so a ticket-acceptance window
/// or "until around 09:00" in a timetable note does not match. Group 1 and
/// 2 are a clock time's hour and minute; neither set means end of day.
#[expect(clippy::expect_used, reason = "a constant pattern")]
static EXPECTED_UNTIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:(?:expected|continue|disruption|in\s+place|service)\s+until\s+(?:approximately\s+|about\s+|around\s+)?|until\s+(?:approximately|about)\s+)(?:(\d{1,2})[:.](\d{2})\b|(?:the\s+)?end\s+of\s+(?:the\s+)?(?:day|service)\b)",
    )
    .expect("valid regex")
});

/// An end followed by "on Sunday 13 December" or "tomorrow" is a later
/// day's, not this one's.
#[expect(clippy::expect_used, reason = "a constant pattern")]
static LATER_DAY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(?:on|tomorrow)\b").expect("valid regex"));

/// RDM descriptions are HTML: tags become spaces, `&nbsp;` a space.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
}

/// What a text states as its end, before it is placed in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stated {
    /// A London wall-clock hour and minute.
    Clock(u32, u32),
    EndOfDay,
}

/// `text`'s first stated end, if any.
fn stated_end(text: &str) -> Option<Stated> {
    for captures in EXPECTED_UNTIL.captures_iter(text) {
        let whole = captures.get(0)?;
        if LATER_DAY.is_match(&text[whole.end()..]) {
            continue;
        }
        match (captures.get(1), captures.get(2)) {
            (Some(hour), Some(minute)) => {
                let (Ok(hour), Ok(minute)) =
                    (hour.as_str().parse::<u32>(), minute.as_str().parse::<u32>())
                else {
                    continue;
                };
                if hour > 23 || minute > 59 {
                    continue;
                }
                return Some(Stated::Clock(hour, minute));
            }
            _ => return Some(Stated::EndOfDay),
        }
    }
    None
}

/// The end the pre-clear text stated, published at `published_at` (when
/// that text was first recorded), for an incident cleared at `cleared_at`.
/// The summary is searched first (it is the headline), then the
/// description.
///
/// A clock time is the occurrence nearest `published_at` (within 12 hours
/// either side): "until 00:30" published at 23:50 is the next day's 00:30,
/// "until 23:30" published at 00:30 the previous evening's. End of day
/// counts only when the text was published in the rail day it was cleared
/// in; yesterday's "end of the day" is not today's.
pub fn expected_end(
    summary: &str,
    description: &str,
    published_at: DateTime<Utc>,
    cleared_at: DateTime<Utc>,
) -> Option<ExpectedEnd> {
    let stated = stated_end(summary).or_else(|| stated_end(&plain_text(description)))?;
    let Stated::Clock(hour, minute) = stated else {
        let same_rail_day = common::rail_day::current_rail_day(published_at)
            == common::rail_day::current_rail_day(cleared_at);
        return same_rail_day.then_some(ExpectedEnd::EndOfDay);
    };
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let local_date = published_at
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive();
    let mut at = common::rail_day::london_to_utc(local_date, time);
    if at - published_at > Duration::hours(12) {
        at = common::rail_day::london_to_utc(local_date - Duration::days(1), time);
    } else if published_at - at > Duration::hours(12) {
        at = common::rail_day::london_to_utc(local_date + Duration::days(1), time);
    }
    Some(ExpectedEnd::At(at))
}

/// Where a caller wants items from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Every recently cleared incident (the status page).
    Everywhere,
    /// Incidents whose stored `affected_lines` overlap `line_ids`, or whose
    /// `affected_stations` include `station` (a line page: its own id; a
    /// station page: every catalogue line through it, plus its code).
    Matching {
        line_ids: Vec<String>,
        station: Option<String>,
    },
}

/// One recently cleared incident, with its last uncleared text.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecentlyClearedRow {
    pub incident_id: String,
    /// The clearing text, as published.
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_lines: Vec<String>,
    pub cleared_at: DateTime<Utc>,
    /// When the feed last listed it.
    pub fetched_at: DateTime<Utc>,
    pub prior_summary: Option<String>,
    pub prior_description: Option<String>,
    /// When the last uncleared text was first recorded.
    pub prior_recorded_at: Option<DateTime<Utc>>,
}

/// The recently cleared incidents (module docs) at `now`, newest clear
/// first. One query; both laterals use `incident_history_id_time`, and the
/// outer filter leaves a few dozen candidate rows (`incidents` itself is a
/// few thousand rows, so it needs no new index).
pub async fn recently_cleared(
    pool: &PgPool,
    now: DateTime<Utc>,
    placement: &Placement,
) -> Result<Vec<RecentlyClearedRow>> {
    let (everywhere, line_ids, stations): (bool, &[String], Vec<String>) = match placement {
        Placement::Everywhere => (true, &[], Vec::new()),
        Placement::Matching { line_ids, station } => (
            false,
            line_ids.as_slice(),
            station.iter().cloned().collect(),
        ),
    };
    let rows = sqlx::query_as::<_, RecentlyClearedRow>(
        "SELECT i.incident_id, i.summary, i.description, i.operators, i.affected_lines, \
                cleared.recorded_at AS cleared_at, i.fetched_at, \
                prior.summary AS prior_summary, prior.description AS prior_description, \
                prior.recorded_at AS prior_recorded_at \
           FROM incidents i \
           CROSS JOIN (SELECT fetched_at FROM ingest_freshness WHERE source = 'incidents') snap \
           LEFT JOIN LATERAL ( \
                SELECT h.summary, h.description, h.recorded_at FROM incident_history h \
                 WHERE h.incident_id = i.incident_id AND NOT h.is_cleared \
                 ORDER BY h.recorded_at DESC LIMIT 1) prior ON true \
           JOIN LATERAL ( \
                SELECT h.recorded_at FROM incident_history h \
                 WHERE h.incident_id = i.incident_id AND h.is_cleared \
                   AND h.recorded_at > COALESCE(prior.recorded_at, '-infinity'::timestamptz) \
                 ORDER BY h.recorded_at LIMIT 1) cleared ON true \
          WHERE i.is_cleared AND i.source_removed_at IS NULL \
            AND i.fetched_at >= snap.fetched_at - make_interval(secs => $1) \
            AND cleared.recorded_at >= $2 \
            AND ($3 OR i.affected_lines && $4::text[] OR i.affected_stations && $5::text[]) \
          ORDER BY cleared.recorded_at DESC, i.incident_id",
    )
    .bind(f64::from(LISTED_WITHIN_SECS))
    .bind(rail_day_start(now))
    .bind(everywhere)
    .bind(line_ids)
    .bind(&stations)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    /// Real pre-clear phrasings from the 2026-10-12 production sample.
    #[test]
    fn the_rdm_phrasings_are_read() {
        for (text, expected) in [
            (
                "Residual disruption between Brockenhurst and Bournemouth expected until 22:30",
                Some(Stated::Clock(22, 30)),
            ),
            (
                "Disruption is expected until 22:30.",
                Some(Stated::Clock(22, 30)),
            ),
            (
                "Lines reopened: disruption between Redhill and Guildford expected until the end of the day",
                Some(Stated::EndOfDay),
            ),
            (
                "Disruption between Leeds and York expected until the end of service",
                Some(Stated::EndOfDay),
            ),
            (
                "Disruption is expected until approximately 15:00.",
                Some(Stated::Clock(15, 0)),
            ),
            (
                "This is expected to continue until 18:45.",
                Some(Stated::Clock(18, 45)),
            ),
            (
                "This may cause disruption until 19.30.",
                Some(Stated::Clock(19, 30)),
            ),
            (
                "No trains between Liskeard and Looe until approximately 21:30",
                Some(Stated::Clock(21, 30)),
            ),
            (
                "These alterations are expected to be in place until the end of the day.",
                Some(Stated::EndOfDay),
            ),
            (
                "Reduced Mildmay line service until the end of the day",
                Some(Stated::EndOfDay),
            ),
            ("DISRUPTION EXPECTED UNTIL 07:00", Some(Stated::Clock(7, 0))),
        ] {
            assert_eq!(stated_end(text), expected, "{text}");
        }
    }

    #[test]
    fn other_until_phrases_are_not_an_end() {
        for text in [
            "Trains will be cancelled until further notice.",
            "Disruption between Hitchin and Peterborough expected until 04:00 on Tuesday 8 September",
            "Delays expected until the end of the day on Sunday 13 December",
            "Tickets are valid at no extra cost (until the end of the day).",
            "The usual half-hourly service will operate in the morning peak, until around 09:00.",
            "Please wait until the train has stopped.",
            "Disruption is expected until 25:00.",
            "Disruption has now ended.",
        ] {
            assert_eq!(stated_end(text), None, "{text}");
        }
    }

    #[test]
    fn a_later_day_end_is_skipped_for_a_same_day_one() {
        assert_eq!(
            stated_end("Expected until 04:00 on Tuesday. Disruption is expected until 23:00."),
            Some(Stated::Clock(23, 0))
        );
    }

    #[test]
    fn the_description_is_read_as_text_when_the_summary_says_nothing() {
        let published = utc(2026, 10, 2, 20, 0);
        assert_eq!(
            expected_end(
                "Residual disruption between Brockenhurst and Bournemouth",
                "<p>As service recovers, trains may be delayed.</p><p>Disruption is expected&nbsp;until 22:30.</p>",
                published,
                utc(2026, 10, 2, 20, 26),
            ),
            // 22:30 BST
            Some(ExpectedEnd::At(utc(2026, 10, 2, 21, 30)))
        );
    }

    #[test]
    fn the_summary_wins_over_the_description() {
        assert_eq!(
            expected_end(
                "Disruption expected until 19:00",
                "<p>Disruption is expected until 18:00.</p>",
                utc(2026, 10, 2, 15, 0),
                utc(2026, 10, 2, 16, 0),
            ),
            Some(ExpectedEnd::At(utc(2026, 10, 2, 18, 0)))
        );
    }

    #[test]
    fn a_clock_time_is_the_occurrence_nearest_publication() {
        // Published 23:50 BST (22:50 UTC): "until 00:30" is the next day's.
        assert_eq!(
            expected_end(
                "Disruption expected until 00:30",
                "",
                utc(2026, 10, 2, 22, 50),
                utc(2026, 10, 2, 23, 0),
            ),
            Some(ExpectedEnd::At(utc(2026, 10, 2, 23, 30)))
        );
        // Published 00:30 BST: "until 23:30" was the previous evening's.
        assert_eq!(
            expected_end(
                "Disruption expected until 23:30",
                "",
                utc(2026, 10, 2, 23, 30),
                utc(2026, 10, 2, 23, 40),
            ),
            Some(ExpectedEnd::At(utc(2026, 10, 2, 22, 30)))
        );
        // Winter (GMT): 22:30 local is 22:30 UTC.
        assert_eq!(
            expected_end(
                "Disruption expected until 22:30",
                "",
                utc(2026, 12, 2, 20, 0),
                utc(2026, 12, 2, 21, 0),
            ),
            Some(ExpectedEnd::At(utc(2026, 12, 2, 22, 30)))
        );
    }

    #[test]
    fn end_of_day_counts_only_within_the_rail_day_of_the_clear() {
        let text = "Disruption expected until the end of the day";
        assert_eq!(
            expected_end(text, "", utc(2026, 10, 2, 9, 0), utc(2026, 10, 2, 15, 0)),
            Some(ExpectedEnd::EndOfDay)
        );
        // Published 00:30 BST on the 3rd is still the 2nd's rail day.
        assert_eq!(
            expected_end(text, "", utc(2026, 10, 2, 9, 0), utc(2026, 10, 2, 23, 30)),
            Some(ExpectedEnd::EndOfDay)
        );
        assert_eq!(
            expected_end(text, "", utc(2026, 10, 1, 9, 0), utc(2026, 10, 2, 9, 0)),
            None,
            "yesterday's end of day"
        );
    }

    #[test]
    fn the_rail_day_starts_at_two_in_the_morning_london_time() {
        // 01:30 BST on the 3rd: still the 2nd's rail day, from 02:00 BST.
        assert_eq!(
            rail_day_start(utc(2026, 10, 3, 0, 30)),
            utc(2026, 10, 2, 1, 0)
        );
        assert_eq!(
            rail_day_start(utc(2026, 10, 3, 1, 30)),
            utc(2026, 10, 3, 1, 0)
        );
        // GMT.
        assert_eq!(
            rail_day_start(utc(2026, 12, 3, 12, 0)),
            utc(2026, 12, 3, 2, 0)
        );
    }
}

/// [`recently_cleared`] against a real database, through the real write
/// path (`queries::upsert_incident_snapshot`, then the history and
/// `fetched_at` backdated by hand). Rewrites `ingest_freshness`'s
/// `incidents` row and resets `incident_feed_state`, so needs
/// `--test-threads=1`.
#[cfg(test)]
mod db_tests {
    use common::IncidentMessage;
    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::data::queries::RowHeartbeat;

    const PREFIX: &str = "TEST-RECENTLY-CLEARED-";

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn reset(pool: &PgPool) {
        for sql in [
            "DELETE FROM incident_history WHERE incident_id LIKE 'TEST-RECENTLY-CLEARED-%'",
            "DELETE FROM incidents WHERE incident_id LIKE 'TEST-RECENTLY-CLEARED-%'",
            "DELETE FROM incident_feed_state",
        ] {
            sqlx::query(sql).execute(pool).await.expect(sql);
        }
    }

    fn incident(suffix: &str, summary: &str, cleared: bool) -> IncidentMessage {
        IncidentMessage {
            incident_id: format!("{PREFIX}{suffix}"),
            summary: summary.to_string(),
            description: format!("<p>{summary}.</p>"),
            operators: vec!["ZZ".to_string()],
            affected_stations: vec![],
            priority: 2,
            validity: vec![],
            is_planned: false,
            is_cleared: cleared,
        }
    }

    async fn write(pool: &PgPool, batch: &[IncidentMessage]) {
        let matcher = common::matcher::LineMatcher::new(&[]);
        ds_store::incidents::apply_snapshot(pool, &matcher, batch, RowHeartbeat::On)
            .await
            .expect("apply snapshot");
    }

    async fn set_lines(pool: &PgPool, suffix: &str, lines: &[&str], stations: &[&str]) {
        sqlx::query(
            "UPDATE incidents SET affected_lines = $2, affected_stations = $3 \
              WHERE incident_id = $1",
        )
        .bind(format!("{PREFIX}{suffix}"))
        .bind(lines)
        .bind(stations)
        .execute(pool)
        .await
        .expect("set lines");
    }

    /// Moves every history row and `fetched_at` of one incident back.
    async fn backdate(pool: &PgPool, suffix: &str, by: Duration) {
        for sql in [
            "UPDATE incident_history SET recorded_at = recorded_at - $2 WHERE incident_id = $1",
            "UPDATE incidents SET fetched_at = fetched_at - $2 WHERE incident_id = $1",
        ] {
            sqlx::query(sql)
                .bind(format!("{PREFIX}{suffix}"))
                .bind(by)
                .execute(pool)
                .await
                .expect(sql);
        }
    }

    fn ids(rows: &[RecentlyClearedRow]) -> Vec<String> {
        rows.iter()
            .filter(|row| row.incident_id.starts_with(PREFIX))
            .map(|row| row.incident_id.trim_start_matches(PREFIX).to_string())
            .collect()
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                recently_cleared::db_tests -- --ignored --test-threads=1`"]
    async fn lists_what_the_feed_still_lists_cleared_this_rail_day() {
        let pool = test_pool().await;
        reset(&pool).await;
        let live_text = "Disruption between Woking and Guildford expected until 23:59";

        // A: live, then cleared, still listed.
        write(&pool, &[incident("A", live_text, false)]).await;
        write(
            &pool,
            &[incident("A", "CLEARED: Disruption has now ended", true)],
        )
        .await;
        // B: born cleared, then the feed stops listing it.
        write(&pool, &[incident("B", "CLEARED: B ended", true)]).await;
        backdate(&pool, "B", Duration::minutes(5)).await;
        // C: still live.
        write(&pool, &[incident("C", live_text, false)]).await;
        // D: reopened, then cleared again: its clear is the second one.
        write(&pool, &[incident("D", live_text, false)]).await;
        write(&pool, &[incident("D", "CLEARED: D ended", true)]).await;
        backdate(&pool, "D", Duration::minutes(30)).await;
        write(&pool, &[incident("D", "Disruption again", false)]).await;
        write(&pool, &[incident("D", "CLEARED: D ended again", true)]).await;
        // The latest snapshot lists A, C and D (B's fetched_at is 5 min old).
        write(
            &pool,
            &[
                incident("A", "CLEARED: Disruption has now ended", true),
                incident("C", live_text, false),
                incident("D", "CLEARED: D ended again", true),
            ],
        )
        .await;
        set_lines(&pool, "A", &["line-a"], &[]).await;
        set_lines(&pool, "D", &["line-d"], &["WOK"]).await;

        let now = Utc::now();
        let rows = recently_cleared(&pool, now, &Placement::Everywhere)
            .await
            .expect("query");
        let mut listed = ids(&rows);
        listed.sort();
        assert_eq!(listed, vec!["A", "D"], "B unlisted, C live");

        let a = rows
            .iter()
            .find(|row| row.incident_id == format!("{PREFIX}A"))
            .expect("A");
        assert_eq!(a.summary, "CLEARED: Disruption has now ended");
        assert_eq!(a.prior_summary.as_deref(), Some(live_text));
        let prior_at = a.prior_recorded_at.expect("A has a live history row");
        assert!(a.cleared_at > prior_at);
        assert!(matches!(
            expected_end(
                a.prior_summary.as_deref().unwrap_or_default(),
                a.prior_description.as_deref().unwrap_or_default(),
                prior_at,
                a.cleared_at,
            ),
            Some(ExpectedEnd::At(_))
        ));

        let d = rows
            .iter()
            .find(|row| row.incident_id == format!("{PREFIX}D"))
            .expect("D");
        assert_eq!(d.prior_summary.as_deref(), Some("Disruption again"));
        assert!(
            now - d.cleared_at < Duration::minutes(5),
            "the current run's clear, not the first one 30 min ago"
        );

        // Placement: by line, by station (through a line or its own code).
        let by_line = |line_ids: &[&str], station: Option<&str>| Placement::Matching {
            line_ids: line_ids.iter().map(ToString::to_string).collect(),
            station: station.map(str::to_string),
        };
        for (placement, expected) in [
            (by_line(&["line-a"], None), vec!["A"]),
            (by_line(&["line-d", "line-x"], None), vec!["D"]),
            (by_line(&[], Some("WOK")), vec!["D"]),
            (by_line(&["line-x"], Some("GLD")), vec![]),
        ] {
            let rows = recently_cleared(&pool, now, &placement)
                .await
                .expect("query");
            assert_eq!(ids(&rows), expected, "{placement:?}");
        }

        // A clear before the current rail day's 02:00 is no longer listed.
        let tomorrow = now + Duration::days(1);
        let rows = recently_cleared(&pool, tomorrow, &Placement::Everywhere)
            .await
            .expect("query");
        assert_eq!(ids(&rows), Vec::<String>::new());
        reset(&pool).await;
    }
}
