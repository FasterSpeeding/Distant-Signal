//! Per-query build of a service date's [`schedule_query::Connection`] array
//! and [`schedule_query::InterchangeData`] -- built fresh from Postgres on
//! every trip-planning query and discarded after, never held resident.
//! See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md's
//! Judgment Call 1 for why this shape (an already-ingested, indexed
//! Postgres read, not a raw CIF re-parse) was chosen over both of the
//! design spec's own named hosting options.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use schedule_query::{
    CallingPointForConnections, Connection, FixedLink, InterchangeData, build_connections,
};
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
struct CallingPointRow {
    uid: String,
    // Selected only so the query text documents what `ORDER BY uid, seq`
    // orders by; the ordering itself is done in SQL, not by reading this
    // field back in Rust.
    #[allow(dead_code)]
    seq: i16,
    tiploc: String,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    day_offset: i16,
}

/// Reads every `schedule_calling_points_full` row for `date`, grouped by
/// `uid` and already ordered by `seq` (the `ORDER BY` below, not an
/// in-memory re-sort) -- exactly the shape [`schedule_query::build_connections`]
/// needs. `None` if no rows exist for `date` at all (no CIF delivery has
/// published this far ahead yet) -- the caller (Phase 5's route handler)
/// maps this to a 404, same "no CIF-derived schedule data has been
/// published for this leg's service date" convention
/// `search_journey_leg_candidates` already establishes.
pub async fn fetch_calling_points_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<HashMap<String, Vec<CallingPointForConnections>>>> {
    let rows: Vec<CallingPointRow> = sqlx::query_as(
        "SELECT uid, seq, tiploc, booked_arrival, booked_departure, day_offset \
         FROM schedule_calling_points_full WHERE service_date = $1 ORDER BY uid, seq",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut by_uid: HashMap<String, Vec<CallingPointForConnections>> = HashMap::new();
    for row in rows {
        by_uid
            .entry(row.uid)
            .or_default()
            .push(CallingPointForConnections {
                tiploc: row.tiploc,
                booked_arrival: row.booked_arrival,
                booked_departure: row.booked_departure,
                day_offset: row.day_offset.max(0) as u8,
            });
    }
    Ok(Some(by_uid))
}

/// [`fetch_calling_points_for_date`] plus [`schedule_query::build_connections`]
/// in one call -- the single function Phase 5's route handler calls.
pub async fn build_connections_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<Vec<Connection>>> {
    let Some(by_uid) = fetch_calling_points_for_date(pool, date).await? else {
        return Ok(None);
    };
    let schedules: Vec<(&str, &[CallingPointForConnections])> = by_uid
        .iter()
        .map(|(uid, points)| (uid.as_str(), points.as_slice()))
        .collect();
    Ok(Some(build_connections(schedules)))
}

#[derive(Debug, sqlx::FromRow)]
struct FixedLinkRow {
    from_crs: String,
    to_crs: String,
    mode: String,
    minutes: i32,
    valid_from: String,
    valid_to: String,
    days_mask: String,
}

/// Builds [`InterchangeData`] from the whole current `stanox_crs`,
/// `tiploc_crs`, and `fixed_links` tables (Phase 1's `stanox_crs`/
/// `fixed_links` reads, plus `tiploc_crs` added by Task 3 of
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) -- all
/// small tables, so a full-table read on every trip-planning query is the
/// same cost class as the existing per-request reference-data reads this
/// app already does elsewhere (e.g. `queries::list_stanox_crs`), not a new
/// performance concern.
///
/// `tiploc_crs` is read in a SECOND pass, after the `stanox_crs` pass
/// below, so a `tiploc_crs` row naturally overrides/adds to whatever the
/// `stanox_crs` pass already populated for that TIPLOC on `insert`
/// (`change_time_by_tiploc`, `tiploc_to_crs`) -- same union-read posture as
/// `queries::crs_for_tiploc`/`crs_for_tiplocs_batch`/
/// `list_stanox_crs_for_crs`, which prefer a `tiploc_crs` row when a
/// TIPLOC exists in both tables. `crs_to_tiplocs`'s existing
/// dedup-by-`contains` guard already prevents duplicate entries regardless
/// of which pass runs first. A TIPLOC that exists ONLY in `tiploc_crs` --
/// e.g. Vauxhall's/Clapham Junction's previously-dropped sibling TIPLOC --
/// now also populates every one of these maps, which it could not before
/// this plan (see `journey.rs`'s `tiploc_key` doc comment).
pub async fn fetch_interchange_data(pool: &PgPool) -> Result<InterchangeData> {
    let stanox_rows = crate::data::queries::list_stanox_crs(pool).await?;
    let mut change_time_by_tiploc = HashMap::new();
    let mut tiploc_to_crs = HashMap::new();
    let mut crs_to_tiplocs: HashMap<String, Vec<String>> = HashMap::new();
    for row in &stanox_rows {
        if let Some(minutes) = row.change_time_minutes {
            change_time_by_tiploc.insert(row.tiploc.clone(), minutes);
        }
        tiploc_to_crs.insert(row.tiploc.clone(), row.crs.clone());
        // `stanox_crs.stanox` is the primary key, not `tiploc` -- multiple
        // STANOX rows (different platforms/areas of one physical station,
        // see `queries::crs_for_tiploc`'s own doc comment) can share one
        // TIPLOC, so guard against pushing the same TIPLOC into the same
        // CRS's list twice (harmless but wasteful: `sibling_tiplocs` would
        // otherwise return the same sibling more than once). This table is
        // small (~3,100 rows total), so an O(n) `contains` check per push
        // is fine.
        let siblings = crs_to_tiplocs.entry(row.crs.clone()).or_default();
        if !siblings.contains(&row.tiploc) {
            siblings.push(row.tiploc.clone());
        }
    }

    let tiploc_crs_rows = crate::data::queries::list_tiploc_crs(pool).await?;
    for row in &tiploc_crs_rows {
        if let Some(minutes) = row.change_time_minutes {
            change_time_by_tiploc.insert(row.tiploc.clone(), minutes);
        }
        tiploc_to_crs.insert(row.tiploc.clone(), row.crs.clone());
        let siblings = crs_to_tiplocs.entry(row.crs.clone()).or_default();
        if !siblings.contains(&row.tiploc) {
            siblings.push(row.tiploc.clone());
        }
    }

    let fixed_link_rows: Vec<FixedLinkRow> = sqlx::query_as(
        "SELECT from_crs, to_crs, mode, minutes, valid_from, valid_to, days_mask FROM fixed_links",
    )
    .fetch_all(pool)
    .await?;
    let mut fixed_links_from_crs: HashMap<String, Vec<FixedLink>> = HashMap::new();
    for row in fixed_link_rows {
        fixed_links_from_crs
            .entry(row.from_crs)
            .or_default()
            .push(FixedLink {
                mode: row.mode,
                to_crs: row.to_crs,
                minutes: row.minutes,
                valid_from: row.valid_from,
                valid_to: row.valid_to,
                days_mask: row.days_mask,
            });
    }

    Ok(InterchangeData {
        change_time_by_tiploc,
        tiploc_to_crs,
        crs_to_tiplocs,
        fixed_links_from_crs,
    })
}

#[cfg(test)]
mod db_tests {
    use super::*;

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for db_tests");
        PgPool::connect(&url)
            .await
            .expect("connect to test database")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_returns_none_when_nothing_is_published() {
        let pool = connect().await;
        let far_future = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let result = build_connections_for_date(&pool, far_future)
            .await
            .expect("query succeeds");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_builds_a_real_connection_from_seeded_rows() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTUID1', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTUID1', 1, 'MKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");

        let connections = build_connections_for_date(&pool, date)
            .await
            .expect("query succeeds")
            .expect("rows exist for this date");
        assert!(
            connections
                .iter()
                .any(|c| c.uid == "TESTUID1" && c.from_tiploc == "EUSTON")
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTUID1'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                fetch_interchange_data -- --ignored --test-threads=1`"]
    async fn fetch_interchange_data_reads_real_stanox_crs_and_fixed_links_rows() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes) \
             VALUES ('TEST-IC-STANOX', 'ZZZ', 'ZZZTPL', 'TEST STATION', 1, 7) \
             ON CONFLICT (stanox) DO UPDATE SET change_time_minutes = EXCLUDED.change_time_minutes",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence) \
             VALUES ('WALK', 'ZZZ', 'YYY', 8, '0000', '2359', '1111111', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed fixed_links");

        let data = fetch_interchange_data(&pool).await.expect("query succeeds");
        assert_eq!(data.change_time_by_tiploc.get("ZZZTPL"), Some(&7));
        assert!(
            data.fixed_links_from_crs
                .get("ZZZ")
                .is_some_and(|links| links.iter().any(|l| l.to_crs == "YYY"))
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-IC-STANOX'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM fixed_links WHERE from_crs = 'ZZZ'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the final-review finding this Phase 2 fix round
    /// exists for: `schedule_calling_points_full.tiploc` is now written
    /// normalized (bare) at publish time
    /// (`schedule-reference::publish_schedule_calling_points_full` now
    /// calls `schedule_query::normalize_tiploc`, matching its two sibling
    /// publish functions in the same file), which matches
    /// `stanox_crs.tiploc`'s own bare storage form -- see
    /// `crate::data::queries::crs_for_tiploc`'s own doc comment for the
    /// real, already-hit "roughly a third of all real station TIPLOCs" /
    /// 2026-09-16 "Unknown location" incident this exact mismatch caused
    /// before.
    ///
    /// This test deliberately seeds a genuinely padded TIPLOC value
    /// directly into `schedule_calling_points_full` (bypassing the fixed
    /// publisher entirely, simulating any future regression that
    /// reintroduces an unnormalized write), then walks the full three-hop
    /// path -- `fetch_calling_points_for_date` ->
    /// `fetch_interchange_data` -> `schedule_query::minimum_change_time`
    /// -- to prove `minimum_change_time`'s own defense-in-depth
    /// normalization (not just the publisher fix) makes the padded value
    /// still resolve against the bare-keyed `stanox_crs` row, rather than
    /// silently missing and falling back to the 5-minute default. None of
    /// the three tasks' own individual tests spanned all three hops
    /// together, which is exactly how the original bug went unnoticed.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                a_padded_tiploc_from_calling_points_full_still_matches_bare_stanox_crs_change_time \
                -- --ignored --test-threads=1`"]
    async fn a_padded_tiploc_from_calling_points_full_still_matches_bare_stanox_crs_change_time() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();

        // A padded TIPLOC, exactly as a real CIF schedule-body TIPLOC field
        // carries it (see `schedule_query::tiploc`'s own doc comment) -- 7
        // characters, space-padded, shorter than 7 chars when trimmed.
        let padded_tiploc = "EUSTON ";
        assert_eq!(
            padded_tiploc.len(),
            7,
            "must be genuinely padded, matching real CIF shape"
        );

        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTUID-PAD', 0, $2, 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTUID-PAD', 1, 'MKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .bind(padded_tiploc)
        .execute(&pool)
        .await
        .expect("seed calling points");

        // The matching interchange row is keyed on the BARE form -- real
        // `stanox_crs` storage, per `crs_for_tiploc`'s own doc comment.
        // `change_time_minutes = 9`, distinct from the 5-minute default, so
        // the test can tell a real lookup apart from a silent miss.
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes) \
             VALUES ('TEST-PAD-STANOX', 'EUS', 'EUSTON', 'TEST EUSTON', 1, 9) \
             ON CONFLICT (stanox) DO UPDATE SET change_time_minutes = EXCLUDED.change_time_minutes",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let by_uid = fetch_calling_points_for_date(&pool, date)
            .await
            .expect("query succeeds")
            .expect("rows exist for this date");
        let calling_points = by_uid.get("TESTUID-PAD").expect("seeded schedule present");
        let the_tiploc_from_the_calling_point = calling_points
            .iter()
            .find(|cp| cp.tiploc.trim() == "EUSTON")
            .expect("the seeded, still-padded EUSTON calling point is present")
            .tiploc
            .clone();
        assert_eq!(
            the_tiploc_from_the_calling_point, padded_tiploc,
            "sanity check: the row read back must still be padded -- fetch_calling_points_for_date \
             does no normalization of its own, by design"
        );

        let interchange_data = fetch_interchange_data(&pool).await.expect("query succeeds");

        assert_eq!(
            schedule_query::minimum_change_time(
                &interchange_data,
                &the_tiploc_from_the_calling_point
            ),
            schedule_query::ChangeTime::Finite(9),
            "a padded TIPLOC read back from schedule_calling_points_full must still match its \
             bare-keyed stanox_crs change-time row via minimum_change_time's own normalization, \
             not silently miss and fall back to the 5-minute default"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTUID-PAD'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-PAD-STANOX'")
            .execute(&pool)
            .await
            .ok();
    }
}
