import type { JourneyLegDetail } from './types';

/** A journey leg's own status classification, independent of
 * `severity.ts`'s `SeverityGroup` -- a leg's status space (has this leg
 * even been matched to a real train yet? is it cancelled? delayed? just
 * waiting for its first movement report?) is not TfL/National Rail's
 * `statusSeverity` integer code and has no case in `SEVERITY_TABLE` to
 * reuse. This is the same rank-and-reduce IDIOM `severity.ts`'s
 * `worstStatus`/`GROUP_RANK` already establishes, ported to a new domain
 * rather than importing that file's (module-private) machinery directly. */
export type LegStatusGroup = 'good' | 'awaiting' | 'unmatched' | 'delayed' | 'severe';

/** Higher rank = worse. Mirrors `severity.ts`'s `GROUP_RANK` shape and
 * ordering convention exactly (good=0 ... severe=4), but over this
 * module's own `LegStatusGroup`, not `SeverityGroup`. Per spec §3: an
 * unmatched leg ("needs a train picked") outranks a merely-delayed leg,
 * and a cancelled leg outranks both. */
const LEG_STATUS_RANK: Record<LegStatusGroup, number> = {
  good: 0,
  awaiting: 1,
  delayed: 2,
  unmatched: 3,
  severe: 4,
};

/** Classifies one leg. Delay threshold is `delayMinutes > 0`, matching
 * `TrainJourney.tsx`'s own existing per-leg delay badge exactly (not the
 * notifier's separate 15-minute escalation threshold) -- see this plan's
 * Judgment Call 4. */
export function legStatusGroup(leg: JourneyLegDetail): LegStatusGroup {
  if (leg.trackedTrainState === null || leg.trackedTrainState === undefined) {
    return 'unmatched';
  }
  const state = leg.trackedTrainState;
  if (state.status === 'cancelled') return 'severe';
  if (state.status === 'awaiting_activation' || state.status === null) return 'awaiting';
  if (state.delayMinutes !== null && state.delayMinutes > 0) return 'delayed';
  return 'good'; // 'en_route' with no reported delay, or 'completed'.
}

/** Higher rank = worse, same convention as `severity.ts`'s `severityRank`. */
export function legStatusRank(leg: JourneyLegDetail): number {
  return LEG_STATUS_RANK[legStatusGroup(leg)];
}

/** Picks the single worst-status leg across a journey's legs, by
 * `legStatusRank`. Returns `null` for a journey with no legs at all
 * (should not occur in practice, but avoids a runtime crash on `reduce`
 * over an empty array if it ever does). */
export function worstLegStatus(legs: JourneyLegDetail[]): LegStatusGroup | null {
  if (legs.length === 0) return null;
  return legs.reduce(
    (worst, leg) => (legStatusRank(leg) > LEG_STATUS_RANK[worst] ? legStatusGroup(leg) : worst),
    legStatusGroup(legs[0]),
  );
}
