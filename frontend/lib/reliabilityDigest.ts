import type { TrackedTrainListItem, TicketListItem } from './types';

/** The punctuality half's population: a train whose real identity was
 * resolved, that has a real recorded delay figure, and whose service date
 * has fully elapsed. `serviceDate < today` (not a `status === 'completed'`
 * check) is the honest proxy for "this journey has actually happened" --
 * `train_current_state.status` never reliably reaches `'completed'` in
 * practice (a pre-existing, already-documented gap; see
 * docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md Finding
 * 1). `today` is a "YYYY-MM-DD" string the caller computes once per
 * request via `londonDayKey(new Date())` (`lib/dateFormat.ts`), not
 * recomputed per row. See
 * docs/superpowers/specs/2026-09-12-reliability-digest-design.md Decision
 * 1. */
export function isEligibleForPunctuality(train: TrackedTrainListItem, today: string): boolean {
  return train.resolutionStatus === 'resolved' && train.delayMinutes !== null && train.serviceDate < today;
}

export interface WorstJourney {
  trainId: number;
  trainUid: string | null;
  serviceDate: string;
  delayMinutes: number;
}

export interface PunctualitySummary {
  eligibleCount: number;
  onTimePct: number | null;
  avgDelayMinutes: number | null;
  cancelledCount: number;
  worstJourneys: WorstJourney[];
}

/** The punctuality half's whole aggregation. Reuses
 * `isEligibleForPunctuality`'s population for every figure -- there is no
 * separate population for `worstJourneys` (spec Decision 2). Cancelled
 * journeys are excluded from the delay/on-time arithmetic entirely and
 * counted separately (spec Decision 1's own extension of DESIGN.md
 * §5.5/§6's "delay rate and cancellation rate are two independent axes"
 * posture) -- a cancelled row's leftover `delayMinutes` never contributes
 * to `avgDelayMinutes` or `onTimePct`, but can still surface in
 * `worstJourneys` (it is still a real recorded delay figure on a real
 * tracked train; excluding it from *both* would silently drop a legitimate
 * "this journey was a mess" data point the user tracked). "On time" is
 * `delayMinutes <= 0`, matching `RowStatusBadge`'s existing convention on
 * this exact page, not `common::Defaults::delay_threshold_minutes` (spec
 * Correction 6). Returns an explicit no-data shape (`null`, never `NaN`
 * dressed up as `0`) when nothing is eligible.
 *
 * `eligibleCount` reports the non-cancelled arithmetic population --
 * exactly the denominator `onTimePct`/`avgDelayMinutes` were computed
 * over -- not the raw `isEligibleForPunctuality` population. Cancelled
 * journeys are "reported as a separate count, never blended into" the
 * headline figures (spec Global Constraints); folding them into
 * `eligibleCount` while excluding them from the arithmetic would silently
 * make "N of your last {eligibleCount} were on time" describe a
 * denominator the arithmetic never actually used. `worstJourneys` still
 * draws from the full `isEligibleForPunctuality` population (including
 * cancelled rows) -- there is no separate population for it (spec
 * Decision 2). */
export function computePunctualitySummary(trains: TrackedTrainListItem[], today: string): PunctualitySummary {
  const eligible = trains.filter((t) => isEligibleForPunctuality(t, today));
  const cancelledCount = eligible.filter((t) => t.status === 'cancelled').length;
  const forArithmetic = eligible.filter((t) => t.status !== 'cancelled');

  const onTimePct =
    forArithmetic.length === 0
      ? null
      : Math.round((forArithmetic.filter((t) => (t.delayMinutes as number) <= 0).length / forArithmetic.length) * 100);

  const avgDelayMinutes =
    forArithmetic.length === 0
      ? null
      : forArithmetic.reduce((sum, t) => sum + (t.delayMinutes as number), 0) / forArithmetic.length;

  const worstJourneys: WorstJourney[] = [...eligible]
    .sort((a, b) => {
      const delayDiff = (b.delayMinutes as number) - (a.delayMinutes as number);
      if (delayDiff !== 0) return delayDiff;
      return b.serviceDate.localeCompare(a.serviceDate);
    })
    .slice(0, 5)
    .map((t) => ({
      trainId: t.id,
      trainUid: t.trainUid,
      serviceDate: t.serviceDate,
      delayMinutes: t.delayMinutes as number,
    }));

  return {
    eligibleCount: forArithmetic.length,
    onTimePct,
    avgDelayMinutes,
    cancelledCount,
    worstJourneys,
  };
}
