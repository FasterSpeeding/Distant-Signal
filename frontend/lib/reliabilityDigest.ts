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
 * cancelled rows, deliberately -- see below) -- there is no separate
 * eligibility population for it (spec Decision 2) -- but IS further
 * narrowed to `delayMinutes > 0`: a "most delayed" list has no honest
 * entry for a journey that was on time or early (`delayMinutes <= 0`),
 * and without this filter a user with only a handful of on-time/early
 * eligible journeys would see nonsensical rows like "0 minutes late" or
 * "-3 minutes late" under that heading. A cancelled row with a genuinely
 * positive leftover `delayMinutes` can still appear here even though it
 * never contributes to `avgDelayMinutes`/`onTimePct` -- it is still a
 * real recorded delay figure on a real tracked train, and excluding it
 * from both would silently drop a legitimate "this journey was a mess"
 * data point the user tracked; this is a deliberate, reasoned choice, not
 * an oversight (a follow-up could mark such rows as cancelled in the UI,
 * but that is not this fix's concern). */
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

  const worstJourneys: WorstJourney[] = eligible
    .filter((t) => (t.delayMinutes as number) > 0)
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

export interface DelayRepayRollup {
  attachedTicketsWithOperator: number;
  eligibleCount: number;
  bandCounts: Record<string, number>;
}

/** The Delay Repay half's whole aggregation. Population is "attached
 * tickets that have a non-null operator" -- NOT "all tracked trains"
 * (operator data only reliably exists via an attached ticket; the
 * NR-primary "track this train" flow never captures one, and
 * tracked-train read models carry no operator field at all -- spec
 * Correction 2). A standalone ticket (`trackedTrainId: null`) is excluded
 * from both numerator and denominator: there is no journey yet to have
 * been delayed on. An attached ticket with `operator: null` is excluded
 * from the denominator too, NOT counted as "0% eligible" -- this app has
 * no idea whether that operator would have paid out at all, and folding
 * "we don't know" into the same bucket as "we know and it's zero" would
 * misstate the denominator's own meaning (spec Decision 3).
 *
 * Both fields count TICKETS, not distinct journeys/tracked trains: one
 * tracked train can legitimately have more than one ticket attached (see
 * `app/track/mine/page.test.tsx`'s "multiple tickets on one train" case),
 * and each is counted separately here. Calling code must describe these
 * numbers as ticket counts, not journey counts, to avoid overstating how
 * many distinct journeys were involved.
 *
 * Consumes `TicketListItem.estimate` -- the already-serialized output of
 * `estimate_delay_repay`, computed once server-side by
 * `build_ticket_list_item` -- never calls `estimate_delay_repay` itself
 * (spec Correction 5). Deliberately computes NO average/blended
 * percentage and NO currency total: `percentage` is a percentage of an
 * unknown fare (this app never stores ticket prices), so averaging two
 * different tickets' percentages produces a number with no unit anyone
 * can act on (spec Correction 1, Decision 3). `bandCounts`' keys are
 * `${scheme}-${bandMinutes}` (e.g. "DR15-30") -- a plain count per band
 * actually observed, nothing more. */
export function computeDelayRepayRollup(tickets: TicketListItem[]): DelayRepayRollup {
  const attached = tickets.filter((t) => t.trackedTrainId !== null && t.operator !== null);
  const eligible = attached.filter((t) => t.estimate !== null);

  const bandCounts: Record<string, number> = {};
  for (const t of eligible) {
    const estimate = t.estimate!;
    const key = `${estimate.scheme}-${estimate.bandMinutes}`;
    bandCounts[key] = (bandCounts[key] ?? 0) + 1;
  }

  return {
    attachedTicketsWithOperator: attached.length,
    eligibleCount: eligible.length,
    bandCounts,
  };
}
