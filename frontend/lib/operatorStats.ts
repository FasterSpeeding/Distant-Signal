import type { OperatorSummary } from './types';

/** Renders an operator rollup's aggregate delay/cancellation figures, or a
 * hedge sentence when none are available. Deliberately NOT
 * `lib/sampleStats.ts`'s `formatSampleSummary` -- that function requires a
 * `SampleStatsCarrier` shape (`sampleAvailability`, optional `dataQuality`)
 * that has no single coherent value once several lines' independently-
 * varying availability/quality are merged into one rollup. See
 * docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 3.
 *
 * The `"TfL"` special case matches `lib/sampleStats.ts`'s
 * `sampleUnavailableReason`'s own TfL wording exactly, for the same
 * underlying reason: TfL statuses never carry sample stats (their
 * `dataQuality` is `'tfl'`, not sample-derived), so a `"TfL"` rollup's
 * `sampleStats` is always absent, and that absence means the same thing
 * here as it does per-line. */
export function formatOperatorSampleSummary(operator: Pick<OperatorSummary, 'code' | 'sampleStats'>): string {
  if (!operator.sampleStats) {
    return operator.code === 'TfL'
      ? "Not measured by this app — status is TfL's own."
      : 'No delay/cancellation data available for this operator.';
  }
  const { total, cancelled, avgDelayMinutes } = operator.sampleStats;
  const cancelledPct = total > 0 ? Math.round((cancelled / total) * 100) : null;
  const delay = `Avg delay ${avgDelayMinutes.toFixed(1)} min`;
  return cancelledPct === null ? delay : `${delay} · ${cancelledPct}% cancelled`;
}
