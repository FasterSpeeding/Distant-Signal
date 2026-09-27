//! `compare_full_coverage --windows`: the shadow-mode evidence for the
//! windowed full-coverage stats
//! (docs/superpowers/specs/2026-09-27-full-coverage-windowed-stats-design.md
//! section 8.3, and its "Decisions (2026-09-27)" section). Every function
//! here is pure over rows loaded by [`load`]; the binary only formats.
//!
//! What it answers, per line and window:
//!
//! 1. **Health** -- how many 15-minute `recent` buckets arrived, and why
//!    the ineligible ones were ineligible.
//! 2. **Would-escalate log** -- every eligible `recent` bucket judged by the
//!    same `common::full_coverage_window::classify_full_coverage_window`
//!    the aggregator uses, against the severity the line was actually
//!    showing at that moment (`line_status_history`), split into the
//!    enforced tier (at or above `--min-rank`, default Severe) and the
//!    lower tiers that would only be recorded (Minor Delays / Reduced
//!    Service). Flapping lines, and lines escalated in more than 20% of
//!    their daytime buckets, are flagged.
//! 3. **Against LDBWS** -- each `recent` bucket beside the LDBWS
//!    half-hourly rows covering the same due range: late and cancel rates,
//!    their correlation, and a severity confusion matrix.
//! 4. **Against the closed day** -- the closed-day audit row against the
//!    last `day_to_date` bucket before close, and against LDBWS's daily
//!    rates. Version-1 (whole-population) rows are reported apart, never
//!    mixed with version 2.
//! 5. **The aggregator's own record** (`full_coverage_window_verdicts`):
//!    what it enforced and what it only would have.
//! 6. **Volume per line** -- to pick the pilot lines: daytime trains per
//!    window, eligibility, escalations, with a suggested set of five lines
//!    of different volumes that have clean health.
//!
//! Note: full coverage counts a train delayed at 3 minutes
//! (`Defaults::full_coverage_delay_threshold_minutes`), LDBWS at 5
//! (`delay_threshold_minutes`), so full coverage's late rates run higher by
//! construction.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use common::full_coverage_window::{
    IneligibleReason, WindowVerdict, classify_full_coverage_window, escalation_decision,
};
use common::{
    Defaults, FullCoverageWindowCounts, FullCoverageWindowKind, FullCoverageWindowStatsRow,
    Severity, severity_rank,
};
use sqlx::{PgPool, Row};

use crate::data::full_coverage_window::{
    BUCKET, StoredVerdict, StoredWindow, closed_day_rows_for_range, verdicts_for_range,
    windows_for_range,
};

/// A line counts as "flapping" with this many escalate/clear transitions
/// inside [`FLAP_WINDOW`].
pub const FLAP_TRANSITIONS: usize = 3;
pub const FLAP_WINDOW: chrono::Duration = chrono::Duration::hours(2);
/// A line escalated in more than this share of its daytime buckets is
/// flagged as possibly biased.
pub const DAYTIME_ESCALATION_SHARE: f64 = 0.20;
/// LDBWS half-hours are classified only with at least this many services.
pub const LDBWS_MIN_TOTAL: i64 = 6;

/// Everything the report reads, loaded once.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub windows: Vec<StoredWindow>,
    pub verdicts: Vec<StoredVerdict>,
    /// line_id -> (computed_at, worst severity), oldest first.
    pub history: HashMap<String, Vec<(DateTime<Utc>, Severity)>>,
    pub ldbws_half_hours: Vec<LdbwsHalfHour>,
    pub ldbws_days: Vec<LdbwsDay>,
    pub closed_days: Vec<common::FullCoverageLineStatsRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LdbwsHalfHour {
    pub line_id: String,
    pub half_hour_start: DateTime<Utc>,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LdbwsDay {
    pub line_id: String,
    pub day: NaiveDate,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
}

/// The worst severity among a `line_status_history.statuses` JSON array.
pub fn worst_severity_of(statuses: &serde_json::Value) -> Severity {
    statuses
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.get("severity").cloned())
        .filter_map(|v| serde_json::from_value::<Severity>(v).ok())
        .max_by_key(|s| severity_rank(*s))
        .unwrap_or(Severity::GoodService)
}

/// Loads every input for `[from, to)` (UTC instants), for one line or all.
pub async fn load(
    pool: &PgPool,
    line_id: Option<&str>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Inputs> {
    // `day_to_date` buckets of the last day run on to its close (02:00
    // London the next morning); `recent` ones are kept to `[from, to)`.
    let windows = windows_for_range(pool, line_id, from, to + chrono::Duration::hours(3))
        .await?
        .into_iter()
        .filter(|w| w.row.window_kind == FullCoverageWindowKind::DayToDate || w.bucket_start < to)
        .collect();
    // The verdict table only exists once api's 20260927120200 migration ran.
    let verdicts = verdicts_for_range(pool, line_id, from, to)
        .await
        .unwrap_or_else(|err| {
            tracing::warn!(error = ?err, "could not read full_coverage_window_verdicts");
            Vec::new()
        });

    let mut history: HashMap<String, Vec<(DateTime<Utc>, Severity)>> = HashMap::new();
    let rows = sqlx::query(
        "SELECT line_id, computed_at, statuses FROM line_status_history
          WHERE ($1::text IS NULL OR line_id = $1)
            AND computed_at >= $2 - interval '7 days' AND computed_at < $3
          ORDER BY line_id, computed_at",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    for row in rows {
        let statuses: serde_json::Value = row.try_get("statuses")?;
        history
            .entry(row.try_get("line_id")?)
            .or_default()
            .push((row.try_get("computed_at")?, worst_severity_of(&statuses)));
    }

    let ldbws_half_hours = sqlx::query(
        "SELECT line_id, half_hour_start, total, delayed, cancelled
           FROM line_status_half_hourly_stats
          WHERE ($1::text IS NULL OR line_id = $1)
            AND half_hour_start >= $2 - interval '2 hours' AND half_hour_start < $3",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(LdbwsHalfHour {
            line_id: row.try_get("line_id")?,
            half_hour_start: row.try_get("half_hour_start")?,
            total: row.try_get("total")?,
            delayed: row.try_get("delayed")?,
            cancelled: row.try_get("cancelled")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let (from_day, to_day) = (
        from.date_naive() - chrono::Duration::days(1),
        to.date_naive(),
    );
    let ldbws_days = sqlx::query(
        "SELECT line_id, day, total, delayed, cancelled FROM line_status_daily_stats
          WHERE ($1::text IS NULL OR line_id = $1) AND day BETWEEN $2 AND $3",
    )
    .bind(line_id)
    .bind(from_day)
    .bind(to_day)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(LdbwsDay {
            line_id: row.try_get("line_id")?,
            day: row.try_get("day")?,
            total: row.try_get("total")?,
            delayed: row.try_get("delayed")?,
            cancelled: row.try_get("cancelled")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let closed_days = closed_day_rows_for_range(pool, line_id, from_day, to_day).await?;
    Ok(Inputs {
        windows,
        verdicts,
        history,
        ldbws_half_hours,
        ldbws_days,
        closed_days,
    })
}

fn recent(windows: &[StoredWindow]) -> impl Iterator<Item = &StoredWindow> {
    windows
        .iter()
        .filter(|w| w.row.window_kind == FullCoverageWindowKind::Recent)
}

/// A bucket's verdict, judged at its own `computed_at` (so a stored row is
/// never "stale"; a missing bucket is what shows a stopped consumer).
pub fn verdict_of(row: &FullCoverageWindowStatsRow, thresholds: &Defaults) -> WindowVerdict {
    classify_full_coverage_window(row, thresholds, row.computed_at)
}

/// London hour of an instant (07-19 is "daytime").
fn london_hour(at: DateTime<Utc>) -> u32 {
    at.with_timezone(&chrono_tz::Europe::London).hour()
}

fn is_daytime(at: DateTime<Utc>) -> bool {
    (7..19).contains(&london_hour(at))
}

fn thresholds<'a>(
    per_line: &'a HashMap<String, Defaults>,
    line_id: &str,
    fallback: &'a Defaults,
) -> &'a Defaults {
    per_line.get(line_id).unwrap_or(fallback)
}

// --- 1. Health --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineHealth {
    pub line_id: String,
    pub buckets_present: usize,
    pub buckets_expected: usize,
    pub eligible: usize,
    pub ineligible: BTreeMap<&'static str, usize>,
}

impl LineHealth {
    pub fn presence(&self) -> f64 {
        ratio(self.buckets_present, self.buckets_expected)
    }
}

fn ratio(n: usize, d: usize) -> f64 {
    if d == 0 { 0.0 } else { n as f64 / d as f64 }
}

pub fn health(
    inputs: &Inputs,
    per_line: &HashMap<String, Defaults>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<LineHealth> {
    let expected = ((to - from).num_minutes() / BUCKET.num_minutes()).max(0) as usize;
    let fallback = Defaults::default();
    let mut by_line: BTreeMap<String, LineHealth> = BTreeMap::new();
    for w in recent(&inputs.windows) {
        let h = by_line
            .entry(w.row.line_id.clone())
            .or_insert_with(|| LineHealth {
                line_id: w.row.line_id.clone(),
                buckets_expected: expected,
                ..LineHealth::default()
            });
        h.buckets_present += 1;
        match verdict_of(&w.row, thresholds(per_line, &w.row.line_id, &fallback)) {
            WindowVerdict::Ineligible(reason) => {
                *h.ineligible.entry(reason.as_str()).or_default() += 1
            }
            _ => h.eligible += 1,
        }
    }
    by_line.into_values().collect()
}

// --- 2. Would-escalate log ----------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Escalation {
    pub line_id: String,
    pub at: DateTime<Utc>,
    pub from: Severity,
    pub to: Severity,
    /// `to` is below the enforced gate: it would only be recorded
    /// (`would_escalate_to` with `below_min_rank`), never shown.
    pub below_min_rank: bool,
    pub reason: String,
    pub counts: FullCoverageWindowCounts,
}

/// The severity `line_id` was showing at `at`: the latest history row at
/// or before it (Good Service when there is none).
pub fn severity_at(history: &[(DateTime<Utc>, Severity)], at: DateTime<Utc>) -> Severity {
    let idx = history.partition_point(|(t, _)| *t <= at);
    idx.checked_sub(1)
        .map(|i| history[i].1)
        .unwrap_or(Severity::GoodService)
}

pub fn escalations(
    inputs: &Inputs,
    per_line: &HashMap<String, Defaults>,
    min_rank: u8,
) -> Vec<Escalation> {
    let fallback = Defaults::default();
    let empty = Vec::new();
    let mut out = Vec::new();
    for w in recent(&inputs.windows) {
        let line = &w.row.line_id;
        let verdict = verdict_of(&w.row, thresholds(per_line, line, &fallback));
        let current = severity_at(
            inputs.history.get(line).unwrap_or(&empty),
            w.row.computed_at,
        );
        let decision = escalation_decision(&verdict, current, min_rank);
        let (Some(to), WindowVerdict::Escalate { reason, .. }) =
            (decision.would_escalate_to, &verdict)
        else {
            continue;
        };
        out.push(Escalation {
            line_id: line.clone(),
            at: w.row.computed_at,
            from: current,
            to,
            below_min_rank: decision.below_min_rank,
            reason: reason.clone(),
            counts: w.row.counts.clone(),
        });
    }
    out
}

/// Totals by (severity, enforced tier or not) and by London hour.
pub fn escalation_totals(
    escalations: &[Escalation],
) -> (BTreeMap<(String, bool), usize>, BTreeMap<u32, usize>) {
    let mut by_severity = BTreeMap::new();
    let mut by_hour = BTreeMap::new();
    for e in escalations {
        *by_severity
            .entry((e.to.description().to_string(), !e.below_min_rank))
            .or_default() += 1;
        *by_hour.entry(london_hour(e.at)).or_default() += 1;
    }
    (by_severity, by_hour)
}

/// Lines with at least [`FLAP_TRANSITIONS`] escalate/clear transitions
/// inside any [`FLAP_WINDOW`], considering every eligible bucket in order.
pub fn flapping_lines(
    inputs: &Inputs,
    per_line: &HashMap<String, Defaults>,
    min_rank: u8,
) -> Vec<String> {
    let fallback = Defaults::default();
    let empty = Vec::new();
    let mut states: BTreeMap<&str, Vec<(DateTime<Utc>, bool)>> = BTreeMap::new();
    for w in recent(&inputs.windows) {
        let line = w.row.line_id.as_str();
        let verdict = verdict_of(&w.row, thresholds(per_line, line, &fallback));
        if matches!(verdict, WindowVerdict::Ineligible(_)) {
            continue;
        }
        let current = severity_at(
            inputs.history.get(line).unwrap_or(&empty),
            w.row.computed_at,
        );
        let escalating = escalation_decision(&verdict, current, min_rank)
            .would_escalate_to
            .is_some();
        states
            .entry(line)
            .or_default()
            .push((w.row.computed_at, escalating));
    }
    let mut flapping = Vec::new();
    for (line, mut seq) in states {
        seq.sort_by_key(|(t, _)| *t);
        let transitions: Vec<DateTime<Utc>> = seq
            .windows(2)
            .filter(|pair| pair[0].1 != pair[1].1)
            .map(|pair| pair[1].0)
            .collect();
        let flaps = transitions
            .windows(FLAP_TRANSITIONS)
            .any(|t| t[FLAP_TRANSITIONS - 1] - t[0] <= FLAP_WINDOW);
        if flaps {
            flapping.push(line.to_string());
        }
    }
    flapping
}

/// Lines escalated (at any tier) in more than [`DAYTIME_ESCALATION_SHARE`]
/// of their eligible daytime buckets: `(line, share)`.
pub fn often_escalated_lines(
    inputs: &Inputs,
    per_line: &HashMap<String, Defaults>,
    escalations: &[Escalation],
) -> Vec<(String, f64)> {
    let fallback = Defaults::default();
    let mut eligible: BTreeMap<&str, usize> = BTreeMap::new();
    for w in recent(&inputs.windows).filter(|w| is_daytime(w.row.computed_at)) {
        let line = w.row.line_id.as_str();
        if !matches!(
            verdict_of(&w.row, thresholds(per_line, line, &fallback)),
            WindowVerdict::Ineligible(_)
        ) {
            *eligible.entry(line).or_default() += 1;
        }
    }
    let mut escalated: HashMap<&str, usize> = HashMap::new();
    for e in escalations.iter().filter(|e| is_daytime(e.at)) {
        *escalated.entry(e.line_id.as_str()).or_default() += 1;
    }
    eligible
        .into_iter()
        .filter_map(|(line, n)| {
            let share = ratio(escalated.get(line).copied().unwrap_or(0), n);
            (share > DAYTIME_ESCALATION_SHARE).then(|| (line.to_string(), share))
        })
        .collect()
}

// --- 3. Against LDBWS ------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct LdbwsPair {
    pub line_id: String,
    pub bucket_start: DateTime<Utc>,
    pub fc: FullCoverageWindowCounts,
    pub fc_eligible: bool,
    pub fc_severity: Option<Severity>,
    pub ldbws_total: i64,
    pub ldbws_delayed: i64,
    pub ldbws_cancelled: i64,
    /// `None` when LDBWS had fewer than [`LDBWS_MIN_TOTAL`] services.
    pub ldbws_severity: Option<Severity>,
}

impl LdbwsPair {
    pub fn fc_late_rate(&self) -> f64 {
        ratio(self.fc.delayed as usize, self.fc.total as usize)
    }
    pub fn fc_cancel_rate(&self) -> f64 {
        ratio(self.fc.cancelled() as usize, self.fc.total as usize)
    }
    pub fn ldbws_late_rate(&self) -> f64 {
        ratio(self.ldbws_delayed as usize, self.ldbws_total as usize)
    }
    pub fn ldbws_cancel_rate(&self) -> f64 {
        ratio(self.ldbws_cancelled as usize, self.ldbws_total as usize)
    }
}

/// LDBWS counts judged by the same mapping (explicit cancellations only,
/// same thresholds), `None` below [`LDBWS_MIN_TOTAL`].
fn ldbws_severity(
    template: &FullCoverageWindowStatsRow,
    total: i64,
    delayed: i64,
    cancelled: i64,
    t: &Defaults,
) -> Option<Severity> {
    if total < LDBWS_MIN_TOTAL {
        return None;
    }
    let as_u32 = |v: i64| u32::try_from(v.max(0)).unwrap_or(u32::MAX);
    let mut row = template.clone();
    row.partial = false;
    row.feed_stale = false;
    row.counts = FullCoverageWindowCounts {
        total: as_u32(total),
        delayed: as_u32(delayed),
        cancelled_explicit: as_u32(cancelled),
        ..FullCoverageWindowCounts::default()
    };
    let mut relaxed = t.clone();
    relaxed.full_coverage_min_sample_size = LDBWS_MIN_TOTAL;
    Some(match verdict_of(&row, &relaxed) {
        WindowVerdict::Escalate { severity, .. } => severity,
        _ => Severity::GoodService,
    })
}

/// Each `recent` bucket paired with the LDBWS half-hours whose start lies
/// in the window's due range.
pub fn ldbws_pairs(inputs: &Inputs, per_line: &HashMap<String, Defaults>) -> Vec<LdbwsPair> {
    let fallback = Defaults::default();
    let mut halves: HashMap<&str, Vec<&LdbwsHalfHour>> = HashMap::new();
    for h in &inputs.ldbws_half_hours {
        halves.entry(h.line_id.as_str()).or_default().push(h);
    }
    let mut out = Vec::new();
    for w in recent(&inputs.windows) {
        let line = w.row.line_id.as_str();
        let t = thresholds(per_line, line, &fallback);
        let (mut total, mut delayed, mut cancelled) = (0, 0, 0);
        for h in halves.get(line).into_iter().flatten() {
            if h.half_hour_start >= w.row.window_start && h.half_hour_start < w.row.window_end {
                total += h.total;
                delayed += h.delayed;
                cancelled += h.cancelled;
            }
        }
        let verdict = verdict_of(&w.row, t);
        out.push(LdbwsPair {
            line_id: line.to_string(),
            bucket_start: w.bucket_start,
            fc: w.row.counts.clone(),
            fc_eligible: !matches!(verdict, WindowVerdict::Ineligible(_)),
            fc_severity: match verdict {
                WindowVerdict::Escalate { severity, .. } => Some(severity),
                WindowVerdict::Good => Some(Severity::GoodService),
                WindowVerdict::Ineligible(_) => None,
            },
            ldbws_total: total,
            ldbws_delayed: delayed,
            ldbws_cancelled: cancelled,
            ldbws_severity: ldbws_severity(&w.row, total, delayed, cancelled, t),
        });
    }
    out
}

/// Pearson correlation; `None` with fewer than 3 points or no variance.
pub fn correlation(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), (a, b)| (x + a / n, y + b / n));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in points {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// (full-coverage severity, LDBWS severity) -> count, over pairs where both
/// are classified.
pub fn confusion(pairs: &[LdbwsPair]) -> BTreeMap<(String, String), usize> {
    let mut out = BTreeMap::new();
    for p in pairs {
        if let (Some(fc), Some(ld)) = (p.fc_severity, p.ldbws_severity) {
            *out.entry((fc.description().to_string(), ld.description().to_string()))
                .or_default() += 1;
        }
    }
    out
}

/// When LDBWS shows at least Minor Delays, how often the eligible window
/// does too (design section 8.4, criterion 5): `(agreeing, of)`.
pub fn agreement_when_ldbws_sees_trouble(pairs: &[LdbwsPair]) -> (usize, usize) {
    let trouble = |s: Severity| severity_rank(s) >= severity_rank(Severity::MinorDelays);
    let relevant: Vec<&LdbwsPair> = pairs
        .iter()
        .filter(|p| p.fc_eligible && p.ldbws_severity.is_some_and(trouble))
        .collect();
    let agreeing = relevant
        .iter()
        .filter(|p| p.fc_severity.is_some_and(trouble))
        .count();
    (agreeing, relevant.len())
}

// --- 4. Against the closed day --------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ClosedDayCheck {
    pub line_id: String,
    pub service_date: NaiveDate,
    pub stats_version: u16,
    pub availability: String,
    pub partial: bool,
    pub closed_total: usize,
    /// The last `day_to_date` bucket's total before close, if stored.
    pub last_day_to_date_total: Option<u32>,
    pub closed_late_rate: f64,
    pub closed_cancel_rate: f64,
    pub ldbws_late_rate: Option<f64>,
    pub ldbws_cancel_rate: Option<f64>,
}

impl ClosedDayCheck {
    /// v2 only: the closed row and the last day-to-date bucket agree
    /// within 2% (the last hour's trains resolve in between).
    pub fn totals_agree(&self) -> Option<bool> {
        let dtd = self.last_day_to_date_total?;
        if self.stats_version < 2 {
            return None;
        }
        let closed = self.closed_total as f64;
        Some((closed - f64::from(dtd)).abs() <= 0.02 * closed.max(1.0))
    }
}

pub fn closed_day_checks(inputs: &Inputs) -> Vec<ClosedDayCheck> {
    let mut last_dtd: HashMap<(&str, NaiveDate), &StoredWindow> = HashMap::new();
    for w in inputs
        .windows
        .iter()
        .filter(|w| w.row.window_kind == FullCoverageWindowKind::DayToDate)
    {
        let key = (w.row.line_id.as_str(), w.row.service_date);
        if last_dtd
            .get(&key)
            .is_none_or(|prev| prev.row.computed_at < w.row.computed_at)
        {
            last_dtd.insert(key, w);
        }
    }
    let ldbws: HashMap<(&str, NaiveDate), &LdbwsDay> = inputs
        .ldbws_days
        .iter()
        .map(|d| ((d.line_id.as_str(), d.day), d))
        .collect();
    inputs
        .closed_days
        .iter()
        .map(|row| {
            let key = (row.line_id.as_str(), row.service_date);
            let ld = ldbws.get(&key);
            ClosedDayCheck {
                line_id: row.line_id.clone(),
                service_date: row.service_date,
                stats_version: row.stats_version.unwrap_or(1),
                availability: row.availability.clone(),
                partial: row.partial,
                closed_total: row.stats.total,
                last_day_to_date_total: last_dtd.get(&key).map(|w| w.row.counts.total),
                closed_late_rate: ratio(row.stats.delayed, row.stats.total),
                closed_cancel_rate: ratio(row.stats.cancelled, row.stats.total),
                ldbws_late_rate: ld.map(|d| ratio(d.delayed as usize, d.total as usize)),
                ldbws_cancel_rate: ld.map(|d| ratio(d.cancelled as usize, d.total as usize)),
            }
        })
        .collect()
}

// --- 5. The aggregator's record -------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VerdictSummary {
    pub evaluated: usize,
    pub by_verdict: BTreeMap<String, usize>,
    /// (severity, "enforced" | "would-escalate" | "below gate") -> count.
    pub escalations: BTreeMap<(String, &'static str), usize>,
}

pub fn verdict_summary(verdicts: &[StoredVerdict]) -> VerdictSummary {
    let mut summary = VerdictSummary::default();
    for v in verdicts {
        summary.evaluated += 1;
        let label = match (&v.verdict[..], &v.ineligible_reason) {
            ("ineligible", Some(reason)) => format!("ineligible: {reason}"),
            (verdict, _) => verdict.to_string(),
        };
        *summary.by_verdict.entry(label).or_default() += 1;
        if let Some(to) = v.would_escalate_to {
            let kind = if v.enforced {
                "enforced"
            } else if v.below_min_rank {
                "below gate"
            } else {
                "would-escalate"
            };
            *summary
                .escalations
                .entry((to.description().to_string(), kind))
                .or_default() += 1;
        }
    }
    summary
}

// --- 6. Volume per line ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineVolume {
    pub line_id: String,
    /// Median evaluable trains in a daytime (07-19 London) `recent` window.
    pub daytime_median: u32,
    pub daytime_p90: u32,
    /// Largest trains-due count of any `day_to_date` bucket (a day's
    /// relevant trains, near its close).
    pub max_daily: u32,
    pub eligible_share: f64,
    pub bucket_presence: f64,
    pub escalations_enforced_tier: usize,
    pub escalations_below_gate: usize,
    pub flapping: bool,
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn line_volumes(
    inputs: &Inputs,
    health: &[LineHealth],
    escalations: &[Escalation],
    flapping: &[String],
) -> Vec<LineVolume> {
    let mut daytime: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    let mut max_daily: HashMap<&str, u32> = HashMap::new();
    for w in &inputs.windows {
        let line = w.row.line_id.as_str();
        match w.row.window_kind {
            FullCoverageWindowKind::Recent => {
                let v = daytime.entry(line).or_default();
                if is_daytime(w.row.computed_at) {
                    v.push(w.row.counts.total);
                }
            }
            FullCoverageWindowKind::DayToDate => {
                let c = &w.row.counts;
                let due = c.total + c.pending + c.unobserved;
                let m = max_daily.entry(line).or_default();
                *m = (*m).max(due);
            }
        }
    }
    let health: HashMap<&str, &LineHealth> =
        health.iter().map(|h| (h.line_id.as_str(), h)).collect();
    daytime
        .into_iter()
        .map(|(line, mut totals)| {
            totals.sort_unstable();
            let h = health.get(line);
            LineVolume {
                line_id: line.to_string(),
                daytime_median: percentile(&totals, 50.0),
                daytime_p90: percentile(&totals, 90.0),
                max_daily: max_daily.get(line).copied().unwrap_or(0),
                eligible_share: h.map_or(0.0, |h| ratio(h.eligible, h.buckets_present)),
                bucket_presence: h.map_or(0.0, |h| h.presence()),
                escalations_enforced_tier: escalations
                    .iter()
                    .filter(|e| e.line_id == line && !e.below_min_rank)
                    .count(),
                escalations_below_gate: escalations
                    .iter()
                    .filter(|e| e.line_id == line && e.below_min_rank)
                    .count(),
                flapping: flapping.iter().any(|f| f == line),
            }
        })
        .collect()
}

/// The design's pilot guidance (open question 2): five lines with daytime
/// medians near 6, 10, 20, 30 and 45 trains per window, each with clean
/// health (at least 95% of buckets present, not flapping), nearest first.
pub const PILOT_TARGETS: [u32; 5] = [6, 10, 20, 30, 45];

pub fn suggest_pilots(volumes: &[LineVolume]) -> Vec<(u32, Option<String>)> {
    let mut used: Vec<&str> = Vec::new();
    PILOT_TARGETS
        .iter()
        .map(|&target| {
            let pick = volumes
                .iter()
                .filter(|v| v.bucket_presence >= 0.95 && !v.flapping && v.daytime_median > 0)
                .filter(|v| !used.contains(&v.line_id.as_str()))
                .min_by_key(|v| v.daytime_median.abs_diff(target));
            if let Some(v) = pick {
                used.push(&v.line_id);
            }
            (target, pick.map(|v| v.line_id.clone()))
        })
        .collect()
}

/// Every `IneligibleReason`, for stable report columns.
pub const INELIGIBLE_REASONS: [IneligibleReason; 4] = [
    IneligibleReason::BelowThreshold,
    IneligibleReason::Partial,
    IneligibleReason::FeedStale,
    IneligibleReason::StaleRow,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn window(
        line: &str,
        computed: &str,
        total: u32,
        delayed: u32,
        cancelled: u32,
    ) -> StoredWindow {
        let computed_at = at(computed);
        StoredWindow {
            bucket_start: crate::data::full_coverage_window::bucket_start(computed_at),
            row: FullCoverageWindowStatsRow {
                line_id: line.to_string(),
                window_kind: FullCoverageWindowKind::Recent,
                service_date: computed_at.date_naive(),
                window_start: computed_at - chrono::Duration::minutes(70),
                window_end: computed_at - chrono::Duration::minutes(10),
                computed_at,
                counts: FullCoverageWindowCounts {
                    total,
                    on_time: total - delayed - cancelled,
                    delayed,
                    cancelled_explicit: cancelled,
                    ..Default::default()
                },
                relevance: "full".to_string(),
                presumed_enabled: true,
                partial: false,
                feed_stale: false,
                stats_version: 2,
            },
        }
    }

    fn no_overrides() -> HashMap<String, Defaults> {
        HashMap::new()
    }

    /// Would-escalate against a synthetic history: escalation only above
    /// what the line was showing, split by the enforced gate.
    #[test]
    fn would_escalate_is_judged_against_the_severity_shown_at_the_time() {
        let mut inputs = Inputs {
            windows: vec![
                window("a", "2026-09-27T09:00:00Z", 12, 7, 0), // Severe over Good
                window("a", "2026-09-27T10:00:00Z", 12, 7, 0), // Severe, already Severe
                window("a", "2026-09-27T11:00:00Z", 12, 3, 0), // Minor over Good: below gate
                window("a", "2026-09-27T12:00:00Z", 4, 4, 0),  // below threshold
            ],
            ..Inputs::default()
        };
        inputs.history.insert(
            "a".to_string(),
            vec![
                (at("2026-09-27T05:00:00Z"), Severity::GoodService),
                (at("2026-09-27T09:30:00Z"), Severity::SevereDelays),
                (at("2026-09-27T10:30:00Z"), Severity::GoodService),
            ],
        );
        let log = escalations(&inputs, &no_overrides(), 4);
        assert_eq!(log.len(), 2);
        assert_eq!(
            (log[0].from, log[0].to),
            (Severity::GoodService, Severity::SevereDelays)
        );
        assert!(!log[0].below_min_rank);
        assert_eq!(log[1].to, Severity::MinorDelays);
        assert!(log[1].below_min_rank);
        let (by_severity, _) = escalation_totals(&log);
        assert_eq!(by_severity[&("Minor Delays".to_string(), false)], 1);
        assert_eq!(by_severity[&("Severe Delays".to_string(), true)], 1);

        let h = health(
            &inputs,
            &no_overrides(),
            at("2026-09-27T09:00:00Z"),
            at("2026-09-27T13:00:00Z"),
        );
        assert_eq!(h[0].buckets_expected, 16);
        assert_eq!(h[0].buckets_present, 4);
        assert_eq!(h[0].ineligible["below_threshold"], 1);
    }

    #[test]
    fn severity_at_takes_the_latest_row_at_or_before() {
        let history = vec![
            (at("2026-09-27T05:00:00Z"), Severity::MinorDelays),
            (at("2026-09-27T09:00:00Z"), Severity::GoodService),
        ];
        assert_eq!(
            severity_at(&history, at("2026-09-27T04:00:00Z")),
            Severity::GoodService
        );
        assert_eq!(
            severity_at(&history, at("2026-09-27T05:00:00Z")),
            Severity::MinorDelays
        );
        assert_eq!(
            severity_at(&history, at("2026-09-27T08:59:00Z")),
            Severity::MinorDelays
        );
        assert_eq!(
            severity_at(&history, at("2026-09-27T12:00:00Z")),
            Severity::GoodService
        );
    }

    #[test]
    fn three_transitions_inside_two_hours_is_flapping() {
        let flappy = Inputs {
            windows: vec![
                window("f", "2026-09-27T09:00:00Z", 12, 7, 0),
                window("f", "2026-09-27T09:15:00Z", 12, 0, 0),
                window("f", "2026-09-27T09:30:00Z", 12, 7, 0),
                window("f", "2026-09-27T09:45:00Z", 12, 0, 0),
                window("s", "2026-09-27T09:00:00Z", 12, 7, 0),
                window("s", "2026-09-27T09:15:00Z", 12, 0, 0),
                window("s", "2026-09-27T13:30:00Z", 12, 7, 0),
            ],
            ..Inputs::default()
        };
        assert_eq!(
            flapping_lines(&flappy, &no_overrides(), 4),
            vec!["f".to_string()]
        );
    }

    /// Each recent bucket is paired with the half-hours starting inside its
    /// due range; LDBWS below 6 services is not classified.
    #[test]
    fn ldbws_half_hours_are_paired_by_due_range() {
        let w = window("a", "2026-09-27T12:00:00Z", 12, 6, 0); // due 10:50-11:50
        let half = |start: &str, total: i64, delayed: i64| LdbwsHalfHour {
            line_id: "a".to_string(),
            half_hour_start: at(start),
            total,
            delayed,
            cancelled: 0,
        };
        let inputs = Inputs {
            windows: vec![w],
            ldbws_half_hours: vec![
                half("2026-09-27T10:30:00Z", 100, 100), // starts before the range
                half("2026-09-27T11:00:00Z", 4, 2),
                half("2026-09-27T11:30:00Z", 4, 2),
                half("2026-09-27T12:00:00Z", 100, 100), // after
            ],
            ..Inputs::default()
        };
        let pairs = ldbws_pairs(&inputs, &no_overrides());
        assert_eq!(pairs.len(), 1);
        let p = &pairs[0];
        assert_eq!((p.ldbws_total, p.ldbws_delayed), (8, 4));
        assert_eq!(p.fc_severity, Some(Severity::SevereDelays));
        assert_eq!(p.ldbws_severity, Some(Severity::SevereDelays));
        assert_eq!(agreement_when_ldbws_sees_trouble(&pairs), (1, 1));
        assert_eq!(confusion(&pairs).len(), 1);
    }

    #[test]
    fn correlation_of_perfectly_related_rates_is_one() {
        let c = correlation(&[(0.1, 0.2), (0.2, 0.4), (0.3, 0.6)]).unwrap();
        assert!((c - 1.0).abs() < 1e-9);
        assert_eq!(correlation(&[(0.1, 0.2), (0.2, 0.4)]), None);
    }

    /// A v1 (whole-population) closed row is never compared with the
    /// windows: no totals check, and it keeps its version.
    #[test]
    fn closed_day_checks_separate_v1_from_v2() {
        let mut dtd = window("a", "2026-09-28T00:55:00Z", 100, 5, 1);
        dtd.row.window_kind = FullCoverageWindowKind::DayToDate;
        dtd.row.service_date = "2026-09-27".parse().unwrap();
        let v2 = common::FullCoverageLineStatsRow {
            line_id: "a".to_string(),
            service_date: "2026-09-27".parse().unwrap(),
            availability: "available".to_string(),
            stats: common::SampleStats {
                total: 101,
                delayed: 5,
                cancelled: 1,
                skipped: 0,
                avg_delay_minutes: 1.0,
            },
            partial: false,
            breakdown: None,
            stats_version: Some(2),
        };
        let mut v1 = v2.clone();
        v1.line_id = "b".to_string();
        v1.stats_version = Some(1);
        let inputs = Inputs {
            windows: vec![dtd],
            closed_days: vec![v2, v1],
            ldbws_days: vec![LdbwsDay {
                line_id: "a".to_string(),
                day: "2026-09-27".parse().unwrap(),
                total: 50,
                delayed: 1,
                cancelled: 0,
            }],
            ..Inputs::default()
        };
        let checks = closed_day_checks(&inputs);
        assert_eq!(
            checks[0].totals_agree(),
            Some(true),
            "100 vs 101 is within 2%"
        );
        assert_eq!(checks[0].ldbws_late_rate, Some(0.02));
        assert_eq!(checks[1].stats_version, 1);
        assert_eq!(checks[1].totals_agree(), None);
    }

    #[test]
    fn verdict_summary_splits_enforced_would_and_below_gate() {
        let v = |verdict: &str, to: Option<Severity>, below: bool, enforced: bool| StoredVerdict {
            line_id: "a".to_string(),
            bucket_start: at("2026-09-27T12:00:00Z"),
            evaluated_at: at("2026-09-27T12:01:00Z"),
            mode: "shadow".to_string(),
            verdict: verdict.to_string(),
            ineligible_reason: None,
            verdict_severity: to,
            current_severity: Some(Severity::GoodService),
            would_escalate_to: to,
            below_min_rank: below,
            in_allowlist: enforced,
            enforced,
            reason: None,
        };
        let summary = verdict_summary(&[
            v("escalate", Some(Severity::SevereDelays), false, true),
            v("escalate", Some(Severity::SevereDelays), false, false),
            v("escalate", Some(Severity::MinorDelays), true, false),
            v("good", None, false, false),
        ]);
        assert_eq!(summary.evaluated, 4);
        assert_eq!(
            summary.escalations[&("Severe Delays".to_string(), "enforced")],
            1
        );
        assert_eq!(
            summary.escalations[&("Severe Delays".to_string(), "would-escalate")],
            1
        );
        assert_eq!(
            summary.escalations[&("Minor Delays".to_string(), "below gate")],
            1
        );
    }

    #[test]
    fn pilot_suggestions_pick_distinct_clean_lines_nearest_each_target() {
        let vol = |line: &str, median: u32, presence: f64, flapping: bool| LineVolume {
            line_id: line.to_string(),
            daytime_median: median,
            bucket_presence: presence,
            flapping,
            ..LineVolume::default()
        };
        let volumes = vec![
            vol("six", 6, 1.0, false),
            vol("seven", 7, 1.0, false),
            vol("gappy-ten", 10, 0.5, false),
            vol("flappy-20", 20, 1.0, true),
            vol("twentyfive", 25, 1.0, false),
            vol("fortyfour", 44, 1.0, false),
        ];
        let picks = suggest_pilots(&volumes);
        assert_eq!(picks[0], (6, Some("six".to_string())));
        assert_eq!(picks[1], (10, Some("seven".to_string())));
        assert_eq!(picks[2], (20, Some("twentyfive".to_string())));
        assert_eq!(picks[3], (30, Some("fortyfour".to_string())));
        assert_eq!(picks[4], (45, None), "no clean line left");
    }

    #[test]
    fn worst_severity_of_history_json() {
        let statuses = serde_json::json!([{"severity": 10}, {"severity": 6}]);
        assert_eq!(worst_severity_of(&statuses), Severity::SevereDelays);
        assert_eq!(
            worst_severity_of(&serde_json::json!([])),
            Severity::GoodService
        );
    }
}
