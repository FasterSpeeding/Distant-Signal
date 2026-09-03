import type { LineStatus, SampleAvailability, SampleStats } from './types';

/** Structural supertype of anything `sampleUnavailableReason`/
 * `formatSampleSummary` can render a reason for -- the existing per-line
 * `LineStatus` callers and the new per-(station, operator)
 * `StationOperatorSampleStats` rows both satisfy this without a cast
 * (`StationOperatorSampleStats` simply has no `dataQuality`/
 * `fullCoverageStats` field, which TypeScript treats as `undefined`).
 * Widened, not renamed -- see
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9
 * for why the eventual source-agnostic rename flagged by
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * stays a separate, later step. `fullCoverageStats` widened onto this same
 * carrier for Decision 1: only `LineStatus` ever carries it in practice
 * (`StationOverriderSampleStats` never does, per that design doc's own
 * "line-level scope only" statement), but the type stays structural rather
 * than a union, matching this type's existing shape. */
type SampleStatsCarrier = {
  sampleStats?: SampleStats;
  sampleAvailability: SampleAvailability;
  dataQuality?: LineStatus['dataQuality'];
  fullCoverageStats?: SampleStats;
};

/** The aggregator attaches the same sample-derived stats to every status on
 * a line's report, so the first one found is representative of all of them
 * — the rationale `RepresentativeInfo` already documents, extracted here
 * because four call sites had independently reimplemented it. */
export function firstSampleStats(statuses: LineStatus[]): SampleStats | undefined {
  return statuses.find((status) => status.sampleStats)?.sampleStats;
}

/** The first status carrying real full-coverage stats if any does, else the
 * first status carrying real sample stats if any does, else the first
 * status overall — so a caller always has a `dataQuality`/
 * `sampleAvailability` to build a reason from, even when nothing on the
 * line has stats. Returns `undefined` only for an empty array.
 *
 * The `fullCoverageStats` precedence step is Decision 3's extension
 * (docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md):
 * a summary row should prefer the more complete number when one status on
 * the line has it, mirroring `escalate_from_sample_stats`'s existing
 * "prefer worse-but-more-informed" posture on the backend. This does not
 * change today's behavior for any line (nothing carries `fullCoverageStats`
 * yet) -- it's forward-looking scaffolding for once something does. */
export function representativeStatus(statuses: LineStatus[]): LineStatus | undefined {
  return statuses.find((s) => s.fullCoverageStats) ?? statuses.find((s) => s.sampleStats) ?? statuses[0];
}

/** `null` rather than 0 for an empty sample: "0% cancelled" out of nothing
 * is a claim the data doesn't support. */
export function cancelledPercent(stats: SampleStats | undefined): number | null {
  if (!stats || stats.total === 0) return null;
  return Math.round((stats.cancelled / stats.total) * 100);
}

/** The human-readable reason sample stats aren't shown, or `null` when real
 * stats are available and the caller should render numbers instead.
 * Precedence, in order: full-coverage available (new, most confident, see
 * Decision 2) -> sample available -> TfL (structural) -> sample
 * available/absent hedges. MUST check `dataQuality` before
 * `sampleAvailability` — a TfL-quality status's `sampleAvailability` is
 * `'no-coverage'` by construction (it never went through the aggregator or
 * DLR pilot), not a meaningful live-pipeline-gap signal. See this app's
 * plan/spec docs for
 * docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md's
 * Decision 1/4 and
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * Decision 2. */
export function sampleUnavailableReason(status: SampleStatsCarrier): string | null {
  if (status.fullCoverageStats) return null;
  if (status.sampleStats) return null;
  if (status.dataQuality === 'tfl') {
    return "Not measured by this app — status is TfL's own.";
  }
  if (status.sampleAvailability.state === 'no-coverage') {
    return 'No live departure data received for this line yet.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}

/** Renders whichever real numbers exist -- `fullCoverageStats` preferred
 * over `sampleStats` when both are present on the same status (Decision 1:
 * "prefer status.fullCoverageStats over status.sampleStats when both exist
 * on the representative status — same numeric rendering, no new column").
 * Falls through to `sampleUnavailableReason`'s hedge copy when neither is
 * present. */
export function formatSampleSummary(status: SampleStatsCarrier | undefined): string {
  if (!status) return 'No sample data'; // defensive; should not occur in practice
  const reason = sampleUnavailableReason(status);
  if (reason) return reason;
  const stats = (status.fullCoverageStats ?? status.sampleStats)!;
  const cancelled = cancelledPercent(stats);
  const delay = `Avg delay ${stats.avgDelayMinutes.toFixed(1)} min`;
  return cancelled === null ? delay : `${delay} · ${cancelled}% cancelled`;
}

/** NEW (Decision 2): a short, confident provenance line shown alongside
 * real numbers once full coverage exists -- deliberately does not replace
 * the numeric rendering itself (`formatSampleSummary` already routes
 * `fullCoverageStats` through the same numeric formatter `sampleStats`
 * used); this is purely the trust-building sentence next to it. Always
 * `null` in practice today, since nothing produces `fullCoverageStats` yet
 * -- forward-looking scaffolding for once a full-coverage producer
 * exists. */
export function coverageProvenanceNote(status: SampleStatsCarrier): string | null {
  if (status.fullCoverageStats) {
    return 'Based on real train-movement data for every scheduled service on this line — not a live-departure sample.';
  }
  return null;
}

/** NEW (Decision 2): the "upgrading, not yet upgraded" fourth copy state --
 * distinct from both existing sample-absence hedges -- for a line that has
 * been opted into full coverage (`full_coverage_enabled`) but whose signal
 * hasn't resolved yet this cycle. Needs the full `LineStatus` type, not the
 * narrower `SampleStatsCarrier`, since `StationOperatorSampleStats` has no
 * `fullCoverageAvailability` field at all (full coverage is scoped to the
 * line level only, per the design doc's own scoping). Always `null` in
 * practice today, since no line has `full_coverage_enabled` set yet. */
export function pendingCoverageNote(status: LineStatus): string | null {
  if (status.fullCoverageAvailability.state === 'pending') {
    return 'Full train-movement data is being resolved for this line — showing the live sample in the meantime.';
  }
  return null;
}
