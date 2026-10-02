//! `full_coverage_line_window_stats`: the windowed full-coverage history
//! `full-coverage-consumer` posts (`POST /private/full-coverage-window-stats`),
//! and the read-only loaders `compare_full_coverage --windows` uses.
//! See docs/superpowers/specs/2026-09-27-full-coverage-windowed-stats-design.md
//! section 7.

use anyhow::Result;
use chrono::{DateTime, DurationRound, NaiveDate, Utc};
use common::{FullCoverageWindowCounts, FullCoverageWindowKind, FullCoverageWindowStatsRow};
use sqlx::{PgPool, Row};

/// Width of one history bucket: each line keeps one row per window kind per
/// 15 minutes (the latest write in the bucket wins).
pub const BUCKET: chrono::Duration = chrono::Duration::minutes(15);

/// `computed_at` truncated to its 15-minute bucket. Computed here, never
/// taken from the wire.
#[expect(
    clippy::expect_used,
    reason = "a constant or range-checked time is always valid"
)]
pub fn bucket_start(computed_at: DateTime<Utc>) -> DateTime<Utc> {
    computed_at
        .duration_trunc(BUCKET)
        .expect("a 15-minute truncation of a real timestamp cannot overflow")
}

/// Why a posted row was refused -- a 400, not a 500, since retrying the
/// same body can never succeed.
#[derive(Debug)]
pub struct InvalidWindowRow {
    pub line_id: String,
    pub problem: String,
}

impl std::fmt::Display for InvalidWindowRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid full-coverage window row for {}: {}",
            self.line_id, self.problem
        )
    }
}

pub fn validate(rows: &[FullCoverageWindowStatsRow]) -> Result<(), InvalidWindowRow> {
    for row in rows {
        let problem = if row.line_id.trim().is_empty() {
            Some("empty line_id".to_string())
        } else if !matches!(row.relevance.as_str(), "full" | "stops_only") {
            Some(format!("unknown relevance {:?}", row.relevance))
        } else if row.window_end < row.window_start {
            Some("window_end before window_start".to_string())
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(InvalidWindowRow {
                line_id: row.line_id.clone(),
                problem,
            });
        }
    }
    Ok(())
}

fn int(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Upserts one row per `(line_id, window_kind, bucket_start)`. A row whose
/// `computed_at` is OLDER than the one already stored for its bucket is
/// ignored, so a replayed or late POST can never move a bucket backwards.
/// Returns the number of rows written.
#[expect(
    clippy::cast_possible_wrap,
    reason = "stats_version is a small constant"
)]
pub async fn upsert_full_coverage_window_stats(
    pool: &PgPool,
    rows: &[FullCoverageWindowStatsRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    for row in rows {
        let c = &row.counts;
        let result = sqlx::query(
            r"
            INSERT INTO full_coverage_line_window_stats
                (line_id, window_kind, bucket_start, service_date, window_start, window_end,
                 computed_at, total, on_time, delayed, cancelled_explicit, cancelled_presumed,
                 skipped, pending, unobserved, avg_delay_minutes, relevance, presumed_enabled,
                 partial, feed_stale, stats_version, cancelled_in_advance, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, now())
            ON CONFLICT (line_id, window_kind, bucket_start) DO UPDATE SET
                service_date       = EXCLUDED.service_date,
                window_start       = EXCLUDED.window_start,
                window_end         = EXCLUDED.window_end,
                computed_at        = EXCLUDED.computed_at,
                total              = EXCLUDED.total,
                on_time            = EXCLUDED.on_time,
                delayed            = EXCLUDED.delayed,
                cancelled_explicit = EXCLUDED.cancelled_explicit,
                cancelled_presumed = EXCLUDED.cancelled_presumed,
                skipped            = EXCLUDED.skipped,
                pending            = EXCLUDED.pending,
                unobserved         = EXCLUDED.unobserved,
                avg_delay_minutes  = EXCLUDED.avg_delay_minutes,
                relevance          = EXCLUDED.relevance,
                presumed_enabled   = EXCLUDED.presumed_enabled,
                partial            = EXCLUDED.partial,
                feed_stale         = EXCLUDED.feed_stale,
                stats_version      = EXCLUDED.stats_version,
                cancelled_in_advance = EXCLUDED.cancelled_in_advance,
                updated_at         = EXCLUDED.updated_at
            WHERE EXCLUDED.computed_at >= full_coverage_line_window_stats.computed_at
            ",
        )
        .bind(&row.line_id)
        .bind(row.window_kind.as_str())
        .bind(bucket_start(row.computed_at))
        .bind(row.service_date)
        .bind(row.window_start)
        .bind(row.window_end)
        .bind(row.computed_at)
        .bind(int(c.total))
        .bind(int(c.on_time))
        .bind(int(c.delayed))
        .bind(int(c.cancelled_explicit))
        .bind(int(c.cancelled_presumed))
        .bind(int(c.skipped))
        .bind(int(c.pending))
        .bind(int(c.unobserved))
        .bind(c.avg_delay_minutes)
        .bind(&row.relevance)
        .bind(row.presumed_enabled)
        .bind(row.partial)
        .bind(row.feed_stale)
        .bind(row.stats_version as i16)
        .bind(int(c.cancelled_in_advance))
        .execute(&mut *tx)
        .await?;
        count += result.rows_affected();
    }
    tx.commit().await?;
    Ok(count)
}

/// The most recent `updated_at` in the table -- the freshness GET.
pub async fn last_full_coverage_window_stats_fetch(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let (fetched_at,): (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM full_coverage_line_window_stats")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

/// One stored window bucket, as the comparison reads it back.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredWindow {
    pub bucket_start: DateTime<Utc>,
    pub row: FullCoverageWindowStatsRow,
}

const WINDOW_COLUMNS: &str = "line_id, window_kind, bucket_start, service_date, window_start, \
    window_end, computed_at, total, on_time, delayed, cancelled_explicit, cancelled_presumed, \
    skipped, pending, unobserved, avg_delay_minutes, relevance, presumed_enabled, partial, \
    feed_stale, stats_version, cancelled_in_advance";

#[expect(
    clippy::cast_sign_loss,
    reason = "clamped to >= 0 first; the columns hold small counts and versions"
)]
fn stored_window(row: &sqlx::postgres::PgRow) -> Result<StoredWindow> {
    let uint = |name: &str| -> Result<u32> { Ok(row.try_get::<i32, _>(name)?.max(0) as u32) };
    let kind: String = row.try_get("window_kind")?;
    Ok(StoredWindow {
        bucket_start: row.try_get("bucket_start")?,
        row: FullCoverageWindowStatsRow {
            line_id: row.try_get("line_id")?,
            window_kind: FullCoverageWindowKind::parse(&kind)
                .ok_or_else(|| anyhow::anyhow!("unknown window_kind {kind:?}"))?,
            service_date: row.try_get("service_date")?,
            window_start: row.try_get("window_start")?,
            window_end: row.try_get("window_end")?,
            computed_at: row.try_get("computed_at")?,
            counts: FullCoverageWindowCounts {
                total: uint("total")?,
                on_time: uint("on_time")?,
                delayed: uint("delayed")?,
                cancelled_explicit: uint("cancelled_explicit")?,
                cancelled_presumed: uint("cancelled_presumed")?,
                skipped: uint("skipped")?,
                pending: uint("pending")?,
                unobserved: uint("unobserved")?,
                avg_delay_minutes: row.try_get("avg_delay_minutes")?,
                cancelled_in_advance: uint("cancelled_in_advance")?,
            },
            relevance: row.try_get("relevance")?,
            presumed_enabled: row.try_get("presumed_enabled")?,
            partial: row.try_get("partial")?,
            feed_stale: row.try_get("feed_stale")?,
            stats_version: row.try_get::<i16, _>("stats_version")?.max(0) as u16,
        },
    })
}

/// Every stored bucket with `bucket_start` in `[from, to)`, for one line or
/// (`line_id: None`) every line, ordered by line, kind and bucket.
pub async fn windows_for_range(
    pool: &PgPool,
    line_id: Option<&str>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<StoredWindow>> {
    let rows = sqlx::query(&format!(
        "SELECT {WINDOW_COLUMNS} FROM full_coverage_line_window_stats
         WHERE ($1::text IS NULL OR line_id = $1) AND bucket_start >= $2 AND bucket_start < $3
         ORDER BY line_id, window_kind, bucket_start"
    ))
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    rows.iter().map(stored_window).collect()
}

/// One `full_coverage_window_verdicts` row: what aggregator decided about a
/// line's `recent` window (shadow or enforce mode).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredVerdict {
    pub line_id: String,
    pub bucket_start: DateTime<Utc>,
    pub evaluated_at: DateTime<Utc>,
    pub mode: String,
    pub verdict: String,
    pub ineligible_reason: Option<String>,
    pub verdict_severity: Option<common::Severity>,
    pub current_severity: Option<common::Severity>,
    pub would_escalate_to: Option<common::Severity>,
    pub below_min_rank: bool,
    pub in_allowlist: bool,
    pub enforced: bool,
    pub reason: Option<String>,
    /// The rule behind an `escalate` verdict; `None` for other verdicts,
    /// and for rows written before the column existed (all rate-based).
    pub basis: Option<common::full_coverage_window::EscalationBasis>,
}

fn severity_from_db(value: Option<i16>) -> Option<common::Severity> {
    let value = u8::try_from(value?).ok()?;
    serde_json::from_value(serde_json::Value::from(value)).ok()
}

/// Every stored aggregator verdict with `bucket_start` in `[from, to)`.
pub async fn verdicts_for_range(
    pool: &PgPool,
    line_id: Option<&str>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<StoredVerdict>> {
    let rows = sqlx::query(
        "SELECT line_id, bucket_start, evaluated_at, mode, verdict, ineligible_reason,
                verdict_severity, current_severity, would_escalate_to, below_min_rank,
                in_allowlist, enforced, reason, basis
           FROM full_coverage_window_verdicts
          WHERE ($1::text IS NULL OR line_id = $1) AND bucket_start >= $2 AND bucket_start < $3
          ORDER BY line_id, bucket_start",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredVerdict {
                line_id: row.try_get("line_id")?,
                bucket_start: row.try_get("bucket_start")?,
                evaluated_at: row.try_get("evaluated_at")?,
                mode: row.try_get("mode")?,
                verdict: row.try_get("verdict")?,
                ineligible_reason: row.try_get("ineligible_reason")?,
                verdict_severity: severity_from_db(row.try_get("verdict_severity")?),
                current_severity: severity_from_db(row.try_get("current_severity")?),
                would_escalate_to: severity_from_db(row.try_get("would_escalate_to")?),
                below_min_rank: row.try_get("below_min_rank")?,
                in_allowlist: row.try_get("in_allowlist")?,
                enforced: row.try_get("enforced")?,
                reason: row.try_get("reason")?,
                basis: row
                    .try_get::<Option<String>, _>("basis")?
                    .as_deref()
                    .and_then(common::full_coverage_window::EscalationBasis::parse),
            })
        })
        .collect()
}

/// Every `full_coverage_line_stats` row with `service_date` in
/// `[from, to]`, for one line or every line.
pub async fn closed_day_rows_for_range(
    pool: &PgPool,
    line_id: Option<&str>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<common::FullCoverageLineStatsRow>> {
    if let Some(line_id) = line_id {
        crate::data::queries::full_coverage_line_stats_for_range(pool, line_id, from, to).await
    } else {
        let lines: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT line_id FROM full_coverage_line_stats
             WHERE service_date BETWEEN $1 AND $2 ORDER BY line_id",
        )
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?;
        let mut rows = Vec::new();
        for line in lines {
            rows.extend(
                crate::data::queries::full_coverage_line_stats_for_range(pool, &line, from, to)
                    .await?,
            );
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(line_id: &str, computed_at: &str, total: u32) -> FullCoverageWindowStatsRow {
        FullCoverageWindowStatsRow {
            line_id: line_id.to_string(),
            window_kind: FullCoverageWindowKind::Recent,
            service_date: "2026-09-27".parse().unwrap(),
            window_start: "2026-09-27T10:50:00Z".parse().unwrap(),
            window_end: "2026-09-27T11:50:00Z".parse().unwrap(),
            computed_at: computed_at.parse().unwrap(),
            counts: FullCoverageWindowCounts {
                total,
                on_time: total,
                ..Default::default()
            },
            relevance: "full".to_string(),
            presumed_enabled: true,
            partial: false,
            feed_stale: false,
            stats_version: 2,
        }
    }

    #[test]
    fn buckets_are_15_minutes_and_computed_from_computed_at() {
        let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            bucket_start(at("2026-09-27T12:14:59Z")),
            at("2026-09-27T12:00:00Z")
        );
        assert_eq!(
            bucket_start(at("2026-09-27T12:15:00Z")),
            at("2026-09-27T12:15:00Z")
        );
    }

    #[test]
    fn validation_rejects_an_unknown_relevance() {
        assert!(validate(&[row("line-a", "2026-09-27T12:00:00Z", 1)]).is_ok());
        let mut bad = row("line-a", "2026-09-27T12:00:00Z", 1);
        bad.relevance = "everything".to_string();
        assert!(validate(&[bad]).is_err());
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    const FIXTURE_LINE_ID: &str = "ZTEST-WINDOW";

    async fn cleanup(pool: &PgPool) {
        sqlx::query("DELETE FROM full_coverage_line_window_stats WHERE line_id = $1")
            .bind(FIXTURE_LINE_ID)
            .execute(pool)
            .await
            .unwrap();
    }

    /// Upsert into the computed bucket; a later write in the same bucket
    /// replaces it; an OLDER `computed_at` (a replayed or late POST) does
    /// not; the next bucket is a new row.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_window -- --ignored --test-threads=1`"]
    async fn upserts_by_bucket_and_never_moves_a_bucket_backwards() {
        let pool = connect().await;
        cleanup(&pool).await;

        let first = row(FIXTURE_LINE_ID, "2026-09-27T12:01:00Z", 10);
        let mut later = row(FIXTURE_LINE_ID, "2026-09-27T12:02:00Z", 11);
        later.counts.on_time = 9;
        later.counts.cancelled_explicit = 2;
        later.counts.cancelled_in_advance = 1;
        let stale = row(FIXTURE_LINE_ID, "2026-09-27T12:01:30Z", 99);
        let next_bucket = row(FIXTURE_LINE_ID, "2026-09-27T12:16:00Z", 12);

        assert_eq!(
            upsert_full_coverage_window_stats(&pool, &[first])
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            upsert_full_coverage_window_stats(&pool, std::slice::from_ref(&later))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            upsert_full_coverage_window_stats(&pool, &[stale])
                .await
                .unwrap(),
            0,
            "an older computed_at does not overwrite"
        );
        upsert_full_coverage_window_stats(&pool, &[next_bucket])
            .await
            .unwrap();

        let stored = windows_for_range(
            &pool,
            Some(FIXTURE_LINE_ID),
            "2026-09-27T00:00:00Z".parse().unwrap(),
            "2026-09-28T00:00:00Z".parse().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(
            stored[0].bucket_start,
            "2026-09-27T12:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(stored[0].row, later, "round-trips exactly");
        assert_eq!(stored[1].row.counts.total, 12);
        assert!(
            last_full_coverage_window_stats_fetch(&pool)
                .await
                .unwrap()
                .is_some()
        );

        cleanup(&pool).await;
    }
}
