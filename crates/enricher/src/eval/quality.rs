//! Quality evaluation: how well a model extracts what the enricher stores,
//! scored against the dataset's gold labels. Independent of environment
//! performance: requests use the target's generous `quality_timeout_secs`,
//! a transport failure is counted apart from a bad answer, and scoring
//! works purely from [`PipelineRecord`]s -- live, or replayed offline from
//! a saved `*.records.jsonl` without the model.
//!
//! What is scored (definitions in `docs/enricher-model-eval.md`):
//!
//! - **Completion / valid output**: did the pipeline produce something the
//!   service would write (parseable, schema-valid, non-empty, aligned)?
//! - **Periods**: predicted periods are matched to gold periods greedily by
//!   field agreement (ties broken by position); unmatched predictions are
//!   hallucinated periods, unmatched gold periods are missed ones.
//! - **Fields** (per matched pair): each scored field gets a [`Verdict`].
//!   Accuracy counts both correct verdicts; precision/recall treat a
//!   non-null value as a positive, so `hallucinated` (non-null where gold
//!   is null) costs precision and `missed` (null where gold is not) costs
//!   recall. `wrong` (non-null, wrong value) costs both.
//! - **Category** (free text in the schema) against `category_any_of`.
//! - **Consistency** (repetitions > 1): whether repeats of the same case
//!   agree, via `churn::compare` (`scope_description` wording excluded).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use chrono::{DateTime, Datelike, TimeDelta, Utc};
use serde::Serialize;

use crate::churn::{self, ChurnField};
use crate::eval::dataset::{Case, Expected, Gold, GoldPeriod, normalize_label};
use crate::eval::pipeline::{Extraction, Failure, FailureKind, PipelineRecord};
use crate::eval::report::{self, TargetInfo, pct, ratio};
use crate::llm::{ExtractionPeriod, ScheduleWindow};

/// The scored per-period fields, in report order.
pub(crate) const FIELDS: [&str; 7] = [
    "date_range",
    "from_date",
    "to_date",
    "schedule_window",
    "impact_type",
    "resolution_status",
    "apparent_severity",
];

/// Enum fields that get a confusion matrix.
const CONFUSION_FIELDS: [&str; 3] = ["resolution_status", "apparent_severity", "impact_type"];

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScoreOptions {
    /// Predicted and gold instants this close count as equal (default 0).
    pub date_tolerance: TimeDelta,
}

/// One scored field of one matched period (or the case's category).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    /// Non-null and accepted.
    Correct,
    /// Null, and null is accepted.
    CorrectNull,
    /// Non-null, but not an accepted value (gold is non-null).
    Wrong,
    /// Non-null where only null is accepted.
    Hallucinated,
    /// Null where a non-null value is expected.
    Missed,
}

impl Verdict {
    fn is_correct(self) -> bool {
        matches!(self, Verdict::Correct | Verdict::CorrectNull)
    }

    fn label(self) -> &'static str {
        match self {
            Verdict::Correct => "correct",
            Verdict::CorrectNull => "correct_null",
            Verdict::Wrong => "wrong",
            Verdict::Hallucinated => "hallucinated",
            Verdict::Missed => "missed",
        }
    }
}

/// Judges `predicted` against the accepted values with `eq` for non-null
/// values.
fn judge<T>(predicted: Option<&T>, accepted: &[Option<T>], eq: impl Fn(&T, &T) -> bool) -> Verdict {
    let matched = accepted
        .iter()
        .any(|want| match (want.as_ref(), predicted) {
            (None, None) => true,
            (Some(want), Some(got)) => eq(got, want),
            _ => false,
        });
    match (matched, predicted) {
        (true, Some(_)) => Verdict::Correct,
        (true, None) => Verdict::CorrectNull,
        (false, None) => Verdict::Missed,
        (false, Some(_)) if accepted.iter().all(Option::is_none) => Verdict::Hallucinated,
        (false, Some(_)) => Verdict::Wrong,
    }
}

/// Verdict counts for one field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct FieldTally {
    pub correct: usize,
    pub correct_null: usize,
    pub wrong: usize,
    pub hallucinated: usize,
    pub missed: usize,
}

impl FieldTally {
    fn add(&mut self, verdict: Verdict) {
        match verdict {
            Verdict::Correct => self.correct += 1,
            Verdict::CorrectNull => self.correct_null += 1,
            Verdict::Wrong => self.wrong += 1,
            Verdict::Hallucinated => self.hallucinated += 1,
            Verdict::Missed => self.missed += 1,
        }
    }

    fn merge(&mut self, other: &FieldTally) {
        self.correct += other.correct;
        self.correct_null += other.correct_null;
        self.wrong += other.wrong;
        self.hallucinated += other.hallucinated;
        self.missed += other.missed;
    }

    pub(crate) fn scored(&self) -> usize {
        self.correct + self.correct_null + self.wrong + self.hallucinated + self.missed
    }

    pub(crate) fn summary(&self) -> FieldSummary {
        FieldSummary {
            tally: *self,
            scored: self.scored(),
            accuracy: ratio(self.correct + self.correct_null, self.scored()),
            precision: ratio(self.correct, self.correct + self.wrong + self.hallucinated),
            recall: ratio(self.correct, self.correct + self.wrong + self.missed),
        }
    }
}

/// A [`FieldTally`] plus its rates (`None` when the denominator is 0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct FieldSummary {
    #[serde(flatten)]
    pub tally: FieldTally,
    pub scored: usize,
    pub accuracy: Option<f64>,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

/// One field judgement, with display values for the report.
struct Judgement {
    field: &'static str,
    verdict: Verdict,
    want: String,
    got: String,
    /// For a wrong `from_date`/`to_date`: [`date_error_kind`].
    date_error: Option<&'static str>,
}

/// `(from_date, to_date)`.
type Range = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

fn show_date(date: Option<&DateTime<Utc>>) -> String {
    date.map_or_else(|| "null".to_string(), DateTime::to_rfc3339)
}

fn show_window(window: Option<&ScheduleWindow>) -> String {
    window.map_or_else(
        || "null".to_string(),
        |w| {
            let days: Vec<String> = w.days_of_week.iter().map(u8::to_string).collect();
            format!("[{}] {}-{}", days.join(","), w.start_time, w.end_time)
        },
    )
}

fn show_label(label: Option<&String>) -> String {
    label.map_or_else(|| "null".to_string(), Clone::clone)
}

fn show_any<T>(accepted: &[T], show: impl Fn(&T) -> String) -> String {
    accepted.iter().map(show).collect::<Vec<_>>().join("|")
}

/// Day-order and whitespace don't matter; times compare as written.
fn same_window(a: &ScheduleWindow, b: &ScheduleWindow) -> bool {
    let days = |w: &ScheduleWindow| w.days_of_week.iter().copied().collect::<BTreeSet<u8>>();
    days(a) == days(b)
        && a.start_time.trim() == b.start_time.trim()
        && a.end_time.trim() == b.end_time.trim()
}

/// A predicted range with both sides null says nothing: same as no range.
fn predicted_range(period: &ExtractionPeriod) -> Option<Range> {
    period
        .date_range
        .as_ref()
        .map(|r| (r.from_date, r.to_date))
        .filter(|(from, to)| from.is_some() || to.is_some())
}

/// Judges every scored field of one predicted period against one gold
/// period.
fn judge_period(
    predicted: &ExtractionPeriod,
    gold: &GoldPeriod,
    opts: ScoreOptions,
) -> Vec<Judgement> {
    let mut out = judge_dates(predicted, gold, opts);
    if let Gold::Expect(want) = &gold.schedule_window {
        let got = predicted.schedule_window.as_ref();
        out.push(Judgement {
            field: "schedule_window",
            verdict: judge(got, std::slice::from_ref(want), same_window),
            want: show_window(want.as_ref()),
            got: show_window(got),
            date_error: None,
        });
    }
    if let Some(accepted) = &gold.impact_type {
        let got = predicted.impact_type.as_ref();
        out.push(Judgement {
            field: "impact_type",
            verdict: judge(got, accepted.accepted(), String::eq),
            want: show_any(accepted.accepted(), |v| show_label(v.as_ref())),
            got: show_label(got),
            date_error: None,
        });
    }
    for (field, accepted, got) in [
        (
            "resolution_status",
            &gold.resolution_status,
            &predicted.resolution_status,
        ),
        (
            "apparent_severity",
            &gold.apparent_severity,
            &predicted.apparent_severity,
        ),
    ] {
        if let Some(accepted) = accepted {
            let wanted: Vec<Option<String>> =
                accepted.accepted().iter().cloned().map(Some).collect();
            out.push(Judgement {
                field,
                verdict: judge(Some(got), &wanted, String::eq),
                want: show_any(accepted.accepted(), String::clone),
                got: got.clone(),
                date_error: None,
            });
        }
    }
    out
}

/// The `date_range`, `from_date` and `to_date` judgements.
fn judge_dates(
    predicted: &ExtractionPeriod,
    gold: &GoldPeriod,
    opts: ScoreOptions,
) -> Vec<Judgement> {
    let mut out = Vec::new();
    let date_eq = |a: &DateTime<Utc>, b: &DateTime<Utc>| (*a - *b).abs() <= opts.date_tolerance;
    let opt_date_eq = |a: &Option<DateTime<Utc>>, b: &Option<DateTime<Utc>>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => date_eq(a, b),
        _ => false,
    };
    if let Gold::Expect(gold_range) = &gold.date_range {
        let want = gold_range
            .as_ref()
            .map(|r| (r.from_date, r.to_date))
            .filter(|(from, to)| from.is_some() || to.is_some());
        let got = predicted_range(predicted);
        let show = |r: Option<&Range>| {
            r.map_or_else(
                || "null".to_string(),
                |(from, to)| format!("{} .. {}", show_date(from.as_ref()), show_date(to.as_ref())),
            )
        };
        out.push(Judgement {
            field: "date_range",
            verdict: judge(got.as_ref(), &[want], |g, w| {
                opt_date_eq(&g.0, &w.0) && opt_date_eq(&g.1, &w.1)
            }),
            want: show(want.as_ref()),
            got: show(got.as_ref()),
            date_error: None,
        });
    }
    let range = predicted.date_range.as_ref();
    for (field, want, got) in [
        (
            "from_date",
            gold.gold_from_date(),
            range.and_then(|r| r.from_date),
        ),
        (
            "to_date",
            gold.gold_to_date(),
            range.and_then(|r| r.to_date),
        ),
    ] {
        if let Gold::Expect(want) = want {
            let verdict = judge(got.as_ref(), &[want], date_eq);
            out.push(Judgement {
                field,
                verdict,
                want: show_date(want.as_ref()),
                got: show_date(got.as_ref()),
                date_error: match (verdict, got, want) {
                    (Verdict::Wrong, Some(got), Some(want)) => Some(date_error_kind(got, want)),
                    _ => None,
                },
            });
        }
    }
    out
}

/// Pairs predicted with gold periods: highest field agreement first, ties
/// by smaller position difference, then by position. Returns
/// `(predicted_index, gold_index)` sorted by gold index.
pub(crate) fn match_periods(
    predicted: &[ExtractionPeriod],
    gold: &[GoldPeriod],
    opts: ScoreOptions,
) -> Vec<(usize, usize)> {
    let mut candidates = Vec::new();
    for (p, predicted_period) in predicted.iter().enumerate() {
        for (g, gold_period) in gold.iter().enumerate() {
            let agreement = judge_period(predicted_period, gold_period, opts)
                .iter()
                .filter(|j| j.verdict.is_correct())
                .count();
            candidates.push((std::cmp::Reverse(agreement), p.abs_diff(g), g, p));
        }
    }
    candidates.sort_unstable();
    let (mut used_p, mut used_g) = (BTreeSet::new(), BTreeSet::new());
    let mut pairs = Vec::new();
    for (_, _, g, p) in candidates {
        if !used_p.contains(&p) && !used_g.contains(&g) {
            used_p.insert(p);
            used_g.insert(g);
            pairs.push((p, g));
        }
    }
    pairs.sort_by_key(|&(_, g)| g);
    pairs
}

/// Classifies a wrong non-null date: the error shapes the prompt's three
/// date conventions predict.
pub(crate) fn date_error_kind(got: DateTime<Utc>, want: DateTime<Utc>) -> &'static str {
    let diff = (got - want).abs();
    if diff == TimeDelta::hours(1) {
        "off_by_1h"
    } else if diff == TimeDelta::days(1) {
        "off_by_1d"
    } else if got.year() != want.year() && got.with_year(want.year()) == Some(want) {
        "wrong_year"
    } else {
        "other"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaseStatus {
    Completed,
    TransportFailure,
    InvalidOutput,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(crate) struct LowConfidence {
    pub periods: usize,
    pub resolution: usize,
    pub severity: usize,
}

/// One scored (case, repetition).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CaseScore {
    pub case_id: String,
    pub repetition: u32,
    pub tags: Vec<String>,
    pub status: CaseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicted_category: Option<String>,
    pub gold_periods: Option<usize>,
    pub predicted_periods: Option<usize>,
    pub matched_periods: usize,
    pub extra_periods: usize,
    pub missed_periods: usize,
    pub fields: BTreeMap<&'static str, FieldTally>,
    pub date_errors: BTreeMap<&'static str, usize>,
    pub low_confidence: LowConfidence,
    /// Periods the service's `MAX_PERIODS` cap dropped.
    pub truncated_periods: usize,
    /// Human-readable list of everything that didn't match.
    pub mismatches: Vec<String>,
    /// The case's gold labels score something (category and/or periods).
    /// An observational case doesn't, and stays out of the exact-match rate.
    pub scored: bool,
    /// Completed, something scored, every scored field correct and the
    /// period count right.
    pub exact: bool,
    #[serde(skip)]
    confusion: Vec<(&'static str, String, String)>,
}

fn score_failure(case: &Case, record: &PipelineRecord, failure: Failure) -> CaseScore {
    let status = match failure.kind {
        FailureKind::Transport => CaseStatus::TransportFailure,
        FailureKind::InvalidOutput => CaseStatus::InvalidOutput,
    };
    let gold_periods = case
        .expected
        .as_ref()
        .and_then(|e| e.periods.as_ref())
        .map(Vec::len);
    CaseScore {
        case_id: case.id.clone(),
        repetition: record.repetition,
        tags: case.tags.clone(),
        status,
        mismatches: vec![format!(
            "{} failed ({}): {}",
            failure.stage.label(),
            failure.outcome,
            failure.message
        )],
        failure: Some(failure),
        category: None,
        predicted_category: None,
        gold_periods,
        predicted_periods: None,
        matched_periods: 0,
        extra_periods: 0,
        missed_periods: 0,
        fields: BTreeMap::new(),
        date_errors: BTreeMap::new(),
        low_confidence: LowConfidence::default(),
        truncated_periods: 0,
        scored: case
            .expected
            .as_ref()
            .is_some_and(Expected::scores_anything),
        exact: false,
        confusion: Vec::new(),
    }
}

/// Scores one completed extraction against a labelled case.
fn score_extraction(
    case: &Case,
    repetition: u32,
    extraction: &Extraction,
    opts: ScoreOptions,
) -> CaseScore {
    let expected = case.expected.clone().unwrap_or_default();
    let mut mismatches = Vec::new();
    let category = expected.category_any_of.as_ref().map(|accepted| {
        let got = normalize_label(&extraction.category);
        let ok = accepted.iter().any(|a| normalize_label(a) == got);
        if !ok {
            mismatches.push(format!(
                "category: got {:?}, want one of {accepted:?}",
                extraction.category
            ));
        }
        if ok { Verdict::Correct } else { Verdict::Wrong }
    });

    let predicted = &extraction.periods;
    let mut fields: BTreeMap<&'static str, FieldTally> = BTreeMap::new();
    let mut date_errors: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut confusion = Vec::new();
    let (mut matched, mut extra, mut missed) = (0, 0, 0);
    if let Some(gold) = &expected.periods {
        let pairs = match_periods(predicted, gold, opts);
        matched = pairs.len();
        extra = predicted.len() - matched;
        missed = gold.len() - matched;
        if predicted.len() != gold.len() {
            mismatches.push(format!(
                "period count: got {}, want {}",
                predicted.len(),
                gold.len()
            ));
        }
        for &(p, g) in &pairs {
            for j in judge_period(&predicted[p], &gold[g], opts) {
                fields.entry(j.field).or_default().add(j.verdict);
                if CONFUSION_FIELDS.contains(&j.field) {
                    confusion.push((j.field, j.want.clone(), j.got.clone()));
                }
                if j.verdict.is_correct() {
                    continue;
                }
                if let Some(kind) = j.date_error {
                    *date_errors.entry(kind).or_default() += 1;
                }
                let hint = gold[g]
                    .scope_hint
                    .as_deref()
                    .map(|h| format!(" ({h})"))
                    .unwrap_or_default();
                mismatches.push(format!(
                    "period {}{hint} {} {}: got {}, want {}",
                    g + 1,
                    j.field,
                    j.verdict.label(),
                    j.got,
                    j.want
                ));
            }
        }
    }
    let low_confidence = LowConfidence {
        periods: predicted.len(),
        resolution: predicted
            .iter()
            .filter(|p| p.resolution_status_confidence == "low")
            .count(),
        severity: predicted
            .iter()
            .filter(|p| p.severity_confidence == "low")
            .count(),
    };
    let scored = expected.scores_anything();
    let exact = scored && mismatches.is_empty();
    CaseScore {
        case_id: case.id.clone(),
        repetition,
        tags: case.tags.clone(),
        status: CaseStatus::Completed,
        failure: None,
        category,
        predicted_category: Some(extraction.category.clone()),
        gold_periods: expected.periods.as_ref().map(Vec::len),
        predicted_periods: Some(predicted.len()),
        matched_periods: matched,
        extra_periods: extra,
        missed_periods: missed,
        fields,
        date_errors,
        low_confidence,
        truncated_periods: extraction.dropped_period_count,
        mismatches,
        scored,
        exact,
        confusion,
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(crate) struct PeriodSummary {
    pub predicted: usize,
    pub gold: usize,
    pub matched: usize,
    /// Predicted periods with no gold counterpart (hallucinated periods).
    pub extra: usize,
    /// Gold periods with no predicted counterpart.
    pub missed: usize,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    /// Outputs (with gold periods) whose period count was exactly right.
    pub count_exact: usize,
    pub count_scored: usize,
    pub count_accuracy: Option<f64>,
    pub count_mean_abs_error: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct Consistency {
    /// Cases with at least two completed repetitions.
    pub cases_compared: usize,
    /// ...whose every repetition matched the first.
    pub stable_cases: usize,
    pub stable_rate: Option<f64>,
    /// How many cases each `churn` field differed in (`scope_description`
    /// is reported but doesn't make a case unstable: it's free text).
    pub unstable_fields: BTreeMap<&'static str, usize>,
}

/// Completion and exact-match counts for the cases carrying one tag.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct TagSummary {
    pub attempts: usize,
    pub completed: usize,
    pub exact: usize,
    pub exact_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct QualitySummary {
    /// Labelled (case, repetition) pairs scored.
    pub attempts: usize,
    /// Records of cases without gold labels (consistency only).
    pub unlabelled_attempts: usize,
    /// Records naming a case id the dataset doesn't have (skipped).
    pub unknown_case_records: usize,
    pub completed: usize,
    pub transport_failures: usize,
    pub invalid_outputs: usize,
    pub failures_by_stage: BTreeMap<&'static str, usize>,
    pub completed_rate: Option<f64>,
    /// Completed / (attempts that got an answer at all).
    pub valid_output_rate: Option<f64>,
    /// Exact outputs / attempts whose case scores anything (a failure is
    /// never exact; observational cases are left out).
    pub exact_match_rate: Option<f64>,
    pub category: Option<FieldSummary>,
    pub periods: PeriodSummary,
    pub fields: BTreeMap<&'static str, FieldSummary>,
    pub date_errors: BTreeMap<&'static str, usize>,
    /// field -> gold value(s) -> predicted value -> count.
    pub confusion: BTreeMap<&'static str, BTreeMap<String, BTreeMap<String, usize>>>,
    pub low_confidence_resolution_rate: Option<f64>,
    pub low_confidence_severity_rate: Option<f64>,
    pub truncated_outputs: usize,
    pub consistency: Option<Consistency>,
    pub by_tag: BTreeMap<String, TagSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QualityReport {
    pub kind: &'static str,
    pub generated_at: String,
    pub target: TargetInfo,
    pub dataset: String,
    pub repetitions: u32,
    pub date_tolerance_mins: i64,
    pub summary: QualitySummary,
    pub cases: Vec<CaseScore>,
}

/// Scores records against the dataset.
pub(crate) fn score(
    cases: &[Case],
    records: &[PipelineRecord],
    opts: ScoreOptions,
) -> (QualitySummary, Vec<CaseScore>) {
    let by_id: BTreeMap<&str, &Case> = cases.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut summary = QualitySummary::default();
    let mut scores = Vec::new();
    let mut outputs: BTreeMap<&str, Vec<Extraction>> = BTreeMap::new();
    for record in records {
        let Some(case) = by_id.get(record.case_id.as_str()) else {
            summary.unknown_case_records += 1;
            continue;
        };
        let outcome = record.outcome();
        if let Ok(extraction) = &outcome {
            outputs
                .entry(case.id.as_str())
                .or_default()
                .push(extraction.clone());
        }
        if case.expected.is_none() {
            summary.unlabelled_attempts += 1;
            continue;
        }
        scores.push(match outcome {
            Ok(extraction) => score_extraction(case, record.repetition, &extraction, opts),
            Err(failure) => score_failure(case, record, failure),
        });
    }
    aggregate(&mut summary, &scores);
    summary.consistency = consistency(&outputs);
    (summary, scores)
}

fn aggregate(summary: &mut QualitySummary, scores: &[CaseScore]) {
    let mut category = FieldTally::default();
    let mut fields: BTreeMap<&'static str, FieldTally> = BTreeMap::new();
    let mut low = LowConfidence::default();
    let mut abs_error = 0;
    let (mut exact, mut scored) = (0, 0);
    let p = &mut summary.periods;
    for score in scores {
        summary.attempts += 1;
        scored += usize::from(score.scored);
        for tag in &score.tags {
            let t = summary.by_tag.entry(tag.clone()).or_default();
            t.attempts += 1;
            t.completed += usize::from(score.status == CaseStatus::Completed);
            t.exact += usize::from(score.exact);
        }
        match score.status {
            CaseStatus::Completed => summary.completed += 1,
            CaseStatus::TransportFailure => summary.transport_failures += 1,
            CaseStatus::InvalidOutput => summary.invalid_outputs += 1,
        }
        if let Some(failure) = &score.failure {
            *summary
                .failures_by_stage
                .entry(failure.stage.label())
                .or_default() += 1;
            continue;
        }
        if score.exact {
            exact += 1;
        }
        if let Some(verdict) = score.category {
            category.add(verdict);
        }
        for (field, tally) in &score.fields {
            fields.entry(field).or_default().merge(tally);
        }
        for (kind, n) in &score.date_errors {
            *summary.date_errors.entry(kind).or_default() += n;
        }
        for (field, want, got) in &score.confusion {
            *summary
                .confusion
                .entry(field)
                .or_default()
                .entry(want.clone())
                .or_default()
                .entry(got.clone())
                .or_default() += 1;
        }
        low.periods += score.low_confidence.periods;
        low.resolution += score.low_confidence.resolution;
        low.severity += score.low_confidence.severity;
        if score.truncated_periods > 0 {
            summary.truncated_outputs += 1;
        }
        if let (Some(gold), Some(predicted)) = (score.gold_periods, score.predicted_periods) {
            p.gold += gold;
            p.predicted += predicted;
            p.matched += score.matched_periods;
            p.extra += score.extra_periods;
            p.missed += score.missed_periods;
            p.count_scored += 1;
            if gold == predicted {
                p.count_exact += 1;
            }
            abs_error += gold.abs_diff(predicted);
        }
    }
    summary.exact_match_rate = ratio(exact, scored);
    for t in summary.by_tag.values_mut() {
        t.exact_rate = ratio(t.exact, t.attempts);
    }
    summary.completed_rate = ratio(summary.completed, summary.attempts);
    summary.valid_output_rate = ratio(
        summary.completed,
        summary.attempts - summary.transport_failures,
    );
    summary.category = (category.scored() > 0).then(|| category.summary());
    summary.fields = fields.iter().map(|(f, t)| (*f, t.summary())).collect();
    summary.low_confidence_resolution_rate = ratio(low.resolution, low.periods);
    summary.low_confidence_severity_rate = ratio(low.severity, low.periods);
    p.precision = ratio(p.matched, p.predicted);
    p.recall = ratio(p.matched, p.gold);
    p.count_accuracy = ratio(p.count_exact, p.count_scored);
    p.count_mean_abs_error = ratio(abs_error, p.count_scored);
}

fn consistency(outputs: &BTreeMap<&str, Vec<Extraction>>) -> Option<Consistency> {
    let mut result = Consistency::default();
    for runs in outputs.values() {
        let Some((first, rest)) = runs.split_first() else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        result.cases_compared += 1;
        let mut changed: BTreeSet<ChurnField> = BTreeSet::new();
        for other in rest {
            let report = churn::compare(
                Some(&first.category),
                &first.periods,
                &other.category,
                &other.periods,
            );
            changed.extend(report.changed);
        }
        for field in &changed {
            *result.unstable_fields.entry(field.label()).or_default() += 1;
        }
        if changed.iter().all(|f| *f == ChurnField::ScopeDescription) {
            result.stable_cases += 1;
        }
    }
    result.stable_rate = ratio(result.stable_cases, result.cases_compared);
    (result.cases_compared > 0).then_some(result)
}

/// The human-readable report.
pub(crate) fn render_markdown(report: &QualityReport) -> String {
    let s = &report.summary;
    let mut md = String::new();
    let _ = writeln!(md, "# Enricher quality eval: {}\n", report.target.name);
    report::target_lines(&mut md, &report.target);
    let _ = writeln!(
        md,
        "- Dataset: `{}` ({} labelled attempt(s), {} repetition(s), date tolerance {} min)\n- Generated: {}\n",
        report.dataset,
        s.attempts,
        report.repetitions,
        report.date_tolerance_mins,
        report.generated_at
    );
    if s.unknown_case_records > 0 {
        let _ = writeln!(
            md,
            "> {} record(s) named case ids missing from the dataset and were skipped.\n",
            s.unknown_case_records
        );
    }
    summary_section(&mut md, s);
    fields_section(&mut md, s);
    diagnostics_section(&mut md, s);
    cases_section(&mut md, &report.cases);
    md
}

fn summary_section(md: &mut String, s: &QualitySummary) {
    md.push_str("## Summary\n\n");
    let period = &s.periods;
    let rows = vec![
        vec![
            "Completed (service would write)".into(),
            format!("{}/{} ({})", s.completed, s.attempts, pct(s.completed_rate)),
        ],
        vec![
            "Valid output rate (excludes transport failures)".into(),
            pct(s.valid_output_rate),
        ],
        vec![
            "Transport failures (perf issue, not quality)".into(),
            s.transport_failures.to_string(),
        ],
        vec![
            "Invalid outputs (bad JSON / empty / misaligned)".into(),
            s.invalid_outputs.to_string(),
        ],
        vec!["Exact case match".into(), pct(s.exact_match_rate)],
        vec![
            "Category accuracy".into(),
            pct(s.category.and_then(|c| c.accuracy)),
        ],
        vec!["Period count accuracy".into(), pct(period.count_accuracy)],
        vec![
            "Period count mean abs error".into(),
            report::num(period.count_mean_abs_error),
        ],
        vec![
            "Period precision / recall".into(),
            format!("{} / {}", pct(period.precision), pct(period.recall)),
        ],
        vec![
            "Hallucinated / missed periods".into(),
            format!("{} / {}", period.extra, period.missed),
        ],
        vec![
            "Low-confidence resolution / severity".into(),
            format!(
                "{} / {}",
                pct(s.low_confidence_resolution_rate),
                pct(s.low_confidence_severity_rate)
            ),
        ],
        vec![
            "Outputs truncated by the period cap".into(),
            s.truncated_outputs.to_string(),
        ],
    ];
    md.push_str(&report::table(&["Metric", "Value"], &rows));
}

fn fields_section(md: &mut String, s: &QualitySummary) {
    md.push_str("\n## Fields (matched periods)\n\n");
    let rows: Vec<Vec<String>> = FIELDS
        .iter()
        .filter_map(|field| s.fields.get(field).map(|f| (field, f)))
        .map(|(field, f)| {
            vec![
                format!("`{field}`"),
                f.scored.to_string(),
                pct(f.accuracy),
                pct(f.precision),
                pct(f.recall),
                f.tally.wrong.to_string(),
                f.tally.hallucinated.to_string(),
                f.tally.missed.to_string(),
            ]
        })
        .collect();
    md.push_str(&report::table(
        &[
            "Field",
            "Scored",
            "Accuracy",
            "Precision",
            "Recall",
            "Wrong",
            "Hallucinated",
            "Missed",
        ],
        &rows,
    ));
}

/// Date-error kinds, confusions, consistency and the per-tag breakdown.
fn diagnostics_section(md: &mut String, s: &QualitySummary) {
    if !s.date_errors.is_empty() {
        md.push_str("\n## Wrong dates by kind\n\n");
        let rows: Vec<Vec<String>> = s
            .date_errors
            .iter()
            .map(|(k, n)| vec![format!("`{k}`"), n.to_string()])
            .collect();
        md.push_str(&report::table(&["Kind", "Count"], &rows));
    }

    let confused: Vec<Vec<String>> = s
        .confusion
        .iter()
        .flat_map(|(field, wants)| {
            wants.iter().flat_map(move |(want, gots)| {
                gots.iter()
                    .filter(move |(got, _)| !want.split('|').any(|w| w == got.as_str()))
                    .map(move |(got, n)| {
                        vec![
                            format!("`{field}`"),
                            want.clone(),
                            got.clone(),
                            n.to_string(),
                        ]
                    })
            })
        })
        .collect();
    if !confused.is_empty() {
        md.push_str("\n## Confusions (gold -> predicted)\n\n");
        md.push_str(&report::table(
            &["Field", "Gold", "Predicted", "Count"],
            &confused,
        ));
    }

    if let Some(c) = &s.consistency {
        md.push_str("\n## Consistency across repetitions\n\n");
        let _ = writeln!(
            md,
            "{}/{} case(s) stable ({}). Fields that varied: {}.",
            c.stable_cases,
            c.cases_compared,
            pct(c.stable_rate),
            if c.unstable_fields.is_empty() {
                "none".to_string()
            } else {
                c.unstable_fields
                    .iter()
                    .map(|(f, n)| format!("`{f}` in {n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
    }

    if !s.by_tag.is_empty() {
        md.push_str("\n## By tag\n\n");
        let rows: Vec<Vec<String>> = s
            .by_tag
            .iter()
            .map(|(tag, t)| {
                vec![
                    format!("`{tag}`"),
                    t.attempts.to_string(),
                    t.completed.to_string(),
                    format!("{} ({})", t.exact, pct(t.exact_rate)),
                ]
            })
            .collect();
        md.push_str(&report::table(
            &["Tag", "Attempts", "Completed", "Exact"],
            &rows,
        ));
    }
}

fn cases_section(md: &mut String, cases: &[CaseScore]) {
    md.push_str("\n## Cases\n\n");
    let rows: Vec<Vec<String>> = cases
        .iter()
        .map(|c| {
            let periods = format!(
                "{}/{}",
                c.predicted_periods.map_or("-".into(), |n| n.to_string()),
                c.gold_periods.map_or("-".into(), |n| n.to_string())
            );
            let outcome = match c.status {
                CaseStatus::Completed if c.exact => "exact",
                CaseStatus::Completed if !c.scored => "completed (unscored)",
                CaseStatus::Completed => "completed",
                CaseStatus::TransportFailure => "transport failure",
                CaseStatus::InvalidOutput => "invalid output",
            };
            vec![
                format!("`{}`", c.case_id),
                c.repetition.to_string(),
                outcome.to_string(),
                periods,
                report::escape(&c.mismatches.join("; ")),
            ]
        })
        .collect();
    md.push_str(&report::table(
        &["Case", "Rep", "Outcome", "Periods (got/want)", "Mismatches"],
        &rows,
    ));
}

/// One row per target, for comparing models.
pub(crate) fn comparison_markdown(reports: &[QualityReport]) -> String {
    let mut md = String::from("# Enricher quality eval: comparison\n\n");
    let field = |r: &QualityReport, f: &str| pct(r.summary.fields.get(f).and_then(|s| s.accuracy));
    let rows: Vec<Vec<String>> = reports
        .iter()
        .map(|r| {
            let s = &r.summary;
            vec![
                r.target.name.clone(),
                format!("`{}`", r.target.model),
                pct(s.completed_rate),
                pct(s.valid_output_rate),
                pct(s.exact_match_rate),
                pct(s.category.and_then(|c| c.accuracy)),
                pct(s.periods.count_accuracy),
                format!("{} / {}", pct(s.periods.precision), pct(s.periods.recall)),
                field(r, "resolution_status"),
                field(r, "apparent_severity"),
                field(r, "impact_type"),
                format!("{} / {}", field(r, "from_date"), field(r, "to_date")),
                field(r, "schedule_window"),
                s.consistency
                    .as_ref()
                    .map_or("-".to_string(), |c| pct(c.stable_rate)),
            ]
        })
        .collect();
    md.push_str(&report::table(
        &[
            "Target",
            "Model",
            "Completed",
            "Valid output",
            "Exact",
            "Category",
            "Period count",
            "Period P / R",
            "Resolution",
            "Severity",
            "Impact type",
            "From / to date",
            "Schedule window",
            "Stable",
        ],
        &rows,
    ));
    md.push_str("\nField columns are accuracy over matched periods. See each target's own report for precision/recall, hallucinations and per-case mismatches.\n");
    md
}

fn options_from_env() -> ScoreOptions {
    ScoreOptions {
        date_tolerance: TimeDelta::minutes(crate::eval::env_parse("EVAL_DATE_TOLERANCE_MINS", 0)),
    }
}

/// Scores, writes `<stem>.json` and `<stem>.md` into `dir`, and returns the
/// report.
fn score_and_write(
    dir: &std::path::Path,
    stem: &str,
    target: TargetInfo,
    dataset_label: &str,
    cases: &[Case],
    records: &[PipelineRecord],
    opts: ScoreOptions,
) -> anyhow::Result<QualityReport> {
    let (summary, scores) = score(cases, records, opts);
    let repetitions = records.iter().map(|r| r.repetition + 1).max().unwrap_or(0);
    let report = QualityReport {
        kind: "quality",
        generated_at: Utc::now().to_rfc3339(),
        target,
        dataset: dataset_label.to_string(),
        repetitions,
        date_tolerance_mins: opts.date_tolerance.num_minutes(),
        summary,
        cases: scores,
    };
    crate::eval::write_json(&dir.join(format!("{stem}.json")), &report)?;
    crate::eval::write_text(&dir.join(format!("{stem}.md")), &render_markdown(&report))?;
    Ok(report)
}

async fn run_live() -> anyhow::Result<()> {
    let (dataset_path, cases) = crate::eval::load_cases()?;
    let cases: Vec<Case> = cases.into_iter().filter(|c| c.expected.is_some()).collect();
    if cases.is_empty() {
        anyhow::bail!("no labelled cases to evaluate");
    }
    let cases: std::sync::Arc<[Case]> = cases.into();
    let targets = crate::eval::target::load_targets()?;
    let repetitions: u32 = crate::eval::env_parse("EVAL_REPEATS", 1);
    let concurrency: usize = crate::eval::env_parse("EVAL_CONCURRENCY", 1);
    let opts = options_from_env();
    let dir = crate::eval::run_dir("quality", "")?;
    let mut reports = Vec::new();
    for target in &targets {
        eprintln!(
            "quality: {} ({}), {} case(s) x {repetitions}, timeout {}s",
            target.name,
            target.model,
            cases.len(),
            target.quality_timeout_secs
        );
        let client = std::sync::Arc::new(target.client(target.quality_timeout_secs)?);
        let records = crate::eval::pipeline::run_jobs(
            client,
            &target.label(),
            &cases,
            repetitions,
            concurrency,
        )
        .await;
        let stem = target.file_stem();
        crate::eval::write_records(&dir.join(format!("{stem}.records.jsonl")), &records)?;
        let report = score_and_write(
            &dir,
            &stem,
            TargetInfo::from_target(target),
            &dataset_path,
            &cases,
            &records,
            opts,
        )?;
        println!("{}", render_markdown(&report));
        reports.push(report);
    }
    crate::eval::write_text(&dir.join("comparison.md"), &comparison_markdown(&reports))?;
    println!("{}", comparison_markdown(&reports));
    eprintln!("quality: reports written to {}", dir.display());
    Ok(())
}

fn run_replay() -> anyhow::Result<()> {
    let paths = crate::eval::env_list("EVAL_RECORDS").ok_or_else(|| {
        anyhow::anyhow!("set EVAL_RECORDS to one or more *.records.jsonl files (comma-separated)")
    })?;
    let (dataset_path, cases) = crate::eval::load_cases()?;
    let opts = options_from_env();
    let mut by_target: BTreeMap<(String, String), Vec<PipelineRecord>> = BTreeMap::new();
    for path in &paths {
        for record in crate::eval::read_records(&crate::eval::resolve(path))? {
            by_target
                .entry((record.label.target.clone(), record.label.model.clone()))
                .or_default()
                .push(record);
        }
    }
    let dir = crate::eval::run_dir("quality", "-replay")?;
    let mut reports = Vec::new();
    for ((name, model), records) in &by_target {
        let target = TargetInfo {
            name: name.clone(),
            model: model.clone(),
            environment: None,
            base_url: None,
        };
        let stem = crate::eval::target::file_stem(name);
        let report = score_and_write(&dir, &stem, target, &dataset_path, &cases, records, opts)?;
        println!("{}", render_markdown(&report));
        reports.push(report);
    }
    crate::eval::write_text(&dir.join("comparison.md"), &comparison_markdown(&reports))?;
    println!("{}", comparison_markdown(&reports));
    eprintln!("quality (replay): reports written to {}", dir.display());
    Ok(())
}

/// Live quality eval. See `crate::eval`'s module doc for the command.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real LLM endpoint (EVAL_TARGETS or LLM_BASE_URL/LLM_MODEL); see eval module doc"]
async fn live_eval_quality() {
    if let Err(err) = run_live().await {
        panic!("quality eval failed: {err:#}");
    }
}

/// Offline re-scoring of saved records: no model needed.
#[test]
#[ignore = "needs EVAL_RECORDS (saved *.records.jsonl files); see eval module doc"]
fn replay_quality_score() {
    if let Err(err) = run_replay() {
        panic!("replay scoring failed: {err:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::pipeline::{FakeBackend, FakeReply, Pass, TargetLabel, run_jobs};
    use crate::llm::LlmCallError;

    fn opts() -> ScoreOptions {
        ScoreOptions {
            date_tolerance: TimeDelta::zero(),
        }
    }

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    #[test]
    fn verdicts_cover_every_null_and_non_null_combination() {
        let eq = |a: &&str, b: &&str| a == b;
        assert_eq!(judge(Some(&"a"), &[Some("a")], eq), Verdict::Correct);
        assert_eq!(judge(None, &[None::<&str>], eq), Verdict::CorrectNull);
        assert_eq!(judge(Some(&"b"), &[Some("a")], eq), Verdict::Wrong);
        assert_eq!(judge(Some(&"b"), &[None], eq), Verdict::Hallucinated);
        assert_eq!(judge(None, &[Some("a")], eq), Verdict::Missed);
        // Any-of, including null.
        assert_eq!(judge(None, &[Some("a"), None], eq), Verdict::CorrectNull);
        assert_eq!(judge(Some(&"a"), &[Some("a"), None], eq), Verdict::Correct);
        assert_eq!(judge(Some(&"b"), &[Some("a"), None], eq), Verdict::Wrong);
    }

    #[test]
    fn tally_rates() {
        let mut tally = FieldTally::default();
        for v in [
            Verdict::Correct,
            Verdict::Correct,
            Verdict::CorrectNull,
            Verdict::Wrong,
            Verdict::Hallucinated,
            Verdict::Missed,
        ] {
            tally.add(v);
        }
        let s = tally.summary();
        assert_eq!(s.scored, 6);
        assert!(close(s.accuracy, 3.0 / 6.0));
        assert!(close(s.precision, 2.0 / 4.0));
        assert!(close(s.recall, 2.0 / 4.0));
        assert_eq!(FieldTally::default().summary().accuracy, None);
    }

    #[test]
    fn date_errors_are_classified() {
        let want: DateTime<Utc> = "2026-05-10T23:00:00Z".parse().unwrap();
        let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            date_error_kind(at("2026-05-11T00:00:00Z"), want),
            "off_by_1h"
        );
        assert_eq!(
            date_error_kind(at("2026-05-11T23:00:00Z"), want),
            "off_by_1d"
        );
        assert_eq!(
            date_error_kind(at("2024-05-10T23:00:00Z"), want),
            "wrong_year"
        );
        assert_eq!(date_error_kind(at("2026-06-01T00:00:00Z"), want), "other");
    }

    fn labelled_case() -> Case {
        serde_json::from_value(serde_json::json!({
            "id": "two",
            "tags": ["multi_period"],
            "summary": "s",
            "description": "d",
            "reference_date": "2026-04-01T00:00:00Z",
            "expected": {
                "category_any_of": ["engineering_works"],
                "periods": [
                    {
                        "date_range": {"from_date": "2026-05-10T23:00:00Z", "to_date": "2026-07-26T23:00:00Z"},
                        "schedule_window": null,
                        "resolution_status": "ongoing",
                        "apparent_severity": ["moderate_disruption", "severe_disruption"],
                        "impact_type": "rail_replacement_bus"
                    },
                    {
                        "scope_hint": "second",
                        "date_range": {"from_date": "2026-07-26T23:00:00Z", "to_date": "2026-10-11T23:00:00Z"},
                        "schedule_window": {"days_of_week": [1, 2, 3, 4], "start_time": "11:00", "end_time": "14:00"},
                        "resolution_status": "ongoing",
                        "impact_type": null
                    }
                ]
            }
        }))
        .unwrap()
    }

    fn period(from: &str, to: &str, severity: &str, impact: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "scope_description": format!("{from} {severity}"),
            "date_range": {"from_date": from, "to_date": to},
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": severity,
            "impact_type": impact,
        })
    }

    /// The predicted periods come back in the wrong order, the second one
    /// is an hour off on `from_date`, has no schedule window, and has a
    /// hallucinated impact type; a third period is invented.
    fn flawed_primary() -> serde_json::Value {
        serde_json::json!({
            "category": "Engineering Works",
            "periods": [
                period("2026-07-27T00:00:00Z", "2026-10-11T23:00:00Z", "normal", Some("diversion")),
                period("2026-05-10T23:00:00Z", "2026-07-26T23:00:00Z", "severe_disruption", Some("rail_replacement_bus")),
                period("2026-12-01T00:00:00Z", "2026-12-02T00:00:00Z", "normal", None),
            ]
        })
    }

    #[tokio::test]
    async fn scoring_a_flawed_output_counts_each_kind_of_error() {
        let backend = FakeBackend::default().agreeing("two", &flawed_primary());
        let label = TargetLabel {
            target: "fake".into(),
            model: "m".into(),
        };
        let cases: std::sync::Arc<[Case]> = vec![labelled_case()].into();
        let records = run_jobs(std::sync::Arc::new(backend), &label, &cases, 1, 1).await;
        let (summary, scores) = score(&cases, &records, opts());

        assert_eq!(summary.attempts, 1);
        assert_eq!(summary.completed, 1);
        assert!(
            close(summary.category.unwrap().accuracy, 1.0),
            "normalized category"
        );
        let score = &scores[0];
        assert!(!score.exact);
        assert_eq!(
            (
                score.matched_periods,
                score.extra_periods,
                score.missed_periods
            ),
            (2, 1, 0)
        );
        assert!(close(summary.periods.precision, 2.0 / 3.0));
        assert!(close(summary.periods.recall, 1.0));
        assert_eq!(summary.periods.count_exact, 0);

        let f = |name: &str| summary.fields[name].tally;
        // Matched despite the order: gold 1 <-> predicted 2 is fully right.
        assert_eq!(f("date_range").correct, 1);
        assert_eq!(f("date_range").wrong, 1);
        assert_eq!(f("from_date").wrong, 1);
        assert_eq!(f("to_date").correct, 2);
        assert_eq!(f("schedule_window").missed, 1);
        assert_eq!(f("impact_type").correct, 1);
        assert_eq!(f("impact_type").hallucinated, 1);
        assert_eq!(f("apparent_severity").correct, 1, "any-of accepted");
        assert_eq!(f("apparent_severity").scored(), 1, "unscored on period 2");
        assert_eq!(summary.date_errors["off_by_1h"], 1);
        assert!(
            score
                .mismatches
                .iter()
                .any(|m| m.contains("(second)") && m.contains("schedule_window")),
            "{:?}",
            score.mismatches
        );
        assert_eq!(
            summary.confusion["impact_type"]["null"]["diversion"], 1,
            "{:?}",
            summary.confusion
        );
    }

    #[tokio::test]
    async fn failures_are_split_by_kind_and_excluded_from_field_scores() {
        let backend = FakeBackend::default().with(
            "two",
            Pass::Primary,
            FakeReply::Error {
                error: || LlmCallError::GatewayUnavailable { status: 504 },
                retries: 1,
            },
        );
        let label = TargetLabel {
            target: "fake".into(),
            model: "m".into(),
        };
        let mut invalid = labelled_case();
        invalid.id = "bad".into();
        let backend = backend.with("bad", Pass::Primary, FakeReply::Content("[]".into()));
        let cases: std::sync::Arc<[Case]> = vec![labelled_case(), invalid].into();
        let records = run_jobs(std::sync::Arc::new(backend), &label, &cases, 1, 1).await;
        let (summary, _) = score(&cases, &records, opts());
        assert_eq!(summary.attempts, 2);
        assert_eq!(summary.transport_failures, 1);
        assert_eq!(summary.invalid_outputs, 1);
        assert!(close(summary.completed_rate, 0.0));
        assert!(close(summary.valid_output_rate, 0.0));
        assert!(close(summary.exact_match_rate, 0.0));
        assert!(summary.fields.is_empty());
        assert_eq!(summary.failures_by_stage["primary"], 2);
    }

    #[tokio::test]
    async fn replayed_records_score_identically_and_repeats_measure_consistency() {
        let good = serde_json::json!({
            "category": "engineering_works",
            "periods": [
                period("2026-05-10T23:00:00Z", "2026-07-26T23:00:00Z", "severe_disruption", Some("rail_replacement_bus")),
                {
                    "scope_description": "second",
                    "date_range": {"from_date": "2026-07-26T23:00:00Z", "to_date": "2026-10-11T23:00:00Z"},
                    "schedule_window": {"days_of_week": [4, 3, 2, 1], "start_time": "11:00", "end_time": "14:00"},
                    "resolution_status": "ongoing",
                    "apparent_severity": "moderate_disruption",
                    "impact_type": null
                }
            ]
        });
        let backend = FakeBackend::default().agreeing("two", &good);
        let label = TargetLabel {
            target: "fake".into(),
            model: "m".into(),
        };
        let cases: std::sync::Arc<[Case]> = vec![labelled_case()].into();
        let records = run_jobs(std::sync::Arc::new(backend), &label, &cases, 2, 1).await;
        let (live, scores) = score(&cases, &records, opts());
        assert!(scores.iter().all(|s| s.exact), "{:?}", scores[0].mismatches);
        assert!(close(live.exact_match_rate, 1.0));
        let tag = &live.by_tag["multi_period"];
        assert_eq!((tag.attempts, tag.exact), (2, 2));
        let consistency = live.consistency.as_ref().unwrap();
        assert_eq!(
            (consistency.cases_compared, consistency.stable_cases),
            (1, 1)
        );

        // Through JSONL and back (what replay_quality_score reads).
        let jsonl: String = records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap() + "\n")
            .collect();
        let replayed = crate::eval::parse_records(&jsonl).unwrap();
        let (offline, _) = score(&cases, &replayed, opts());
        assert_eq!(
            serde_json::to_value(&offline).unwrap(),
            serde_json::to_value(&live).unwrap()
        );
    }

    #[tokio::test]
    async fn observational_cases_stay_out_of_the_exact_match_rate() {
        let mut observational = labelled_case();
        observational.id = "obs".into();
        observational.expected = Some(Expected::default());
        let backend = FakeBackend::default().agreeing("obs", &flawed_primary());
        let label = TargetLabel {
            target: "fake".into(),
            model: "m".into(),
        };
        let cases: std::sync::Arc<[Case]> = vec![observational].into();
        let records = run_jobs(std::sync::Arc::new(backend), &label, &cases, 1, 1).await;
        let (summary, scores) = score(&cases, &records, opts());
        assert_eq!((summary.attempts, summary.completed), (1, 1));
        assert!(!scores[0].scored && !scores[0].exact);
        assert_eq!(summary.exact_match_rate, None);
        assert!(
            render_markdown(&QualityReport {
                kind: "quality",
                generated_at: "now".into(),
                target: TargetInfo {
                    name: "t".into(),
                    model: "m".into(),
                    environment: None,
                    base_url: None,
                },
                dataset: "d".into(),
                repetitions: 1,
                date_tolerance_mins: 0,
                summary,
                cases: scores,
            })
            .contains("completed (unscored)")
        );
    }

    #[test]
    fn records_for_unknown_or_unlabelled_cases_are_not_scored() {
        let record = PipelineRecord {
            label: TargetLabel {
                target: "t".into(),
                model: "m".into(),
            },
            case_id: "gone".into(),
            repetition: 0,
            elapsed_ms: 0,
            calls: Vec::new(),
        };
        let mut unlabelled = labelled_case();
        unlabelled.id = "u".into();
        unlabelled.expected = None;
        let mut for_unlabelled = record.clone();
        for_unlabelled.case_id = "u".into();
        let (summary, scores) = score(&[unlabelled], &[record, for_unlabelled], opts());
        assert_eq!(summary.unknown_case_records, 1);
        assert_eq!(summary.unlabelled_attempts, 1);
        assert!(scores.is_empty());
    }

    #[test]
    fn date_tolerance_accepts_near_misses() {
        let case = labelled_case();
        let gold = &case.expected.as_ref().unwrap().periods.as_ref().unwrap()[1];
        let predicted: ExtractionPeriod = serde_json::from_value(period(
            "2026-07-27T00:00:00Z",
            "2026-10-11T23:00:00Z",
            "moderate_disruption",
            None,
        ))
        .unwrap();
        let strict = judge_period(&predicted, gold, opts());
        let lenient = judge_period(
            &predicted,
            gold,
            ScoreOptions {
                date_tolerance: TimeDelta::hours(1),
            },
        );
        let verdict =
            |js: &[Judgement], field: &str| js.iter().find(|j| j.field == field).unwrap().verdict;
        assert_eq!(verdict(&strict, "from_date"), Verdict::Wrong);
        assert_eq!(verdict(&lenient, "from_date"), Verdict::Correct);
        assert_eq!(verdict(&lenient, "date_range"), Verdict::Correct);
    }

    #[test]
    fn markdown_renders_every_section() {
        let report = QualityReport {
            kind: "quality",
            generated_at: "now".into(),
            target: TargetInfo {
                name: "t".into(),
                model: "m".into(),
                environment: Some("laptop".into()),
                base_url: None,
            },
            dataset: "d.jsonl".into(),
            repetitions: 1,
            date_tolerance_mins: 0,
            summary: QualitySummary::default(),
            cases: Vec::new(),
        };
        let md = render_markdown(&report);
        assert!(md.contains("# Enricher quality eval: t"));
        assert!(md.contains("laptop"));
        let comparison = comparison_markdown(&[report]);
        assert!(comparison.contains("| t |"));
    }
}
