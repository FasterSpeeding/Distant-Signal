import type { LineStatus, SampleStats } from './types';

/** The aggregator attaches the same sample-derived stats to every status on
 * a line's report, so the first one found is representative of all of them
 * — the rationale `RepresentativeInfo` already documents, extracted here
 * because four call sites had independently reimplemented it. */
export function firstSampleStats(statuses: LineStatus[]): SampleStats | undefined {
  return statuses.find((status) => status.sampleStats)?.sampleStats;
}

/** The first status carrying real stats if any does, else the first status
 * overall — so a caller always has a `dataQuality`/`sampleAvailability` to
 * build a reason from, even when nothing on the line has stats. Returns
 * `undefined` only for an empty array. */
export function representativeStatus(statuses: LineStatus[]): LineStatus | undefined {
  return statuses.find((s) => s.sampleStats) ?? statuses[0];
}

/** `null` rather than 0 for an empty sample: "0% cancelled" out of nothing
 * is a claim the data doesn't support. */
export function cancelledPercent(stats: SampleStats | undefined): number | null {
  if (!stats || stats.total === 0) return null;
  return Math.round((stats.cancelled / stats.total) * 100);
}

/** The human-readable reason sample stats aren't shown, or `null` when real
 * stats are available and the caller should render numbers instead. MUST
 * check `dataQuality` before `sampleAvailability` — a TfL-quality status's
 * `sampleAvailability` is `'no-coverage'` by construction (it never went
 * through the aggregator or DLR pilot), not a meaningful live-pipeline-gap
 * signal. See this app's plan/spec docs for
 * docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md's
 * Decision 1/4. */
export function sampleUnavailableReason(status: LineStatus): string | null {
  if (status.sampleStats) return null;
  if (status.dataQuality === 'tfl') {
    return "Not measured by this app — status is TfL's own.";
  }
  if (status.sampleAvailability.state === 'no-coverage') {
    return 'No live departure data received for this line yet.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}

export function formatSampleSummary(status: LineStatus | undefined): string {
  if (!status) return 'No sample data'; // defensive; should not occur in practice
  const reason = sampleUnavailableReason(status);
  if (reason) return reason;
  const stats = status.sampleStats!;
  const cancelled = cancelledPercent(stats);
  const delay = `Avg delay ${stats.avgDelayMinutes.toFixed(1)} min`;
  return cancelled === null ? delay : `${delay} · ${cancelled}% cancelled`;
}
