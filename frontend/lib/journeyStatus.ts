import { connectionGapMinutes } from './journeyLegChaining';
import { isUntrackedLeg } from './untrackedLeg';
import type { JourneyLegDetail, JourneyListItem, TrackingPhaseFields } from './types';

/** A journey leg's own status classification, independent of
 * `severity.ts`'s `SeverityGroup` -- a leg's status space (has this leg
 * even been matched to a real train yet? is it cancelled? delayed? just
 * waiting for its first movement report?) is not TfL/National Rail's
 * `statusSeverity` integer code and has no case in `SEVERITY_TABLE` to
 * reuse. This is the same rank-and-reduce IDIOM `severity.ts`'s
 * `worstStatus`/`GROUP_RANK` already establishes, ported to a new domain
 * rather than importing that file's (module-private) machinery directly. */
export type LegStatusGroup =
  | 'untrackedDone'
  | 'untrackedPlanned'
  | 'untrackedInProgress'
  | 'good'
  | 'planned'
  | 'awaiting'
  | 'unmatched'
  | 'noTimetable'
  | 'delayed'
  | 'skipped'
  | 'severe';

/** Higher rank = worse. Mirrors `severity.ts`'s `GROUP_RANK` shape and
 * ordering convention exactly (good=0 ... worst last), but over this
 * module's own `LegStatusGroup`, not `SeverityGroup`. Per spec §3: an
 * unmatched leg ("needs a train picked") outranks a merely-delayed leg, a
 * leg whose own origin or destination is being SKIPPED outranks both (the
 * traveller cannot use this train at all, and unlike an unmatched leg
 * nothing on screen is even asking them to act), and a cancelled leg
 * outranks everything. */
const LEG_STATUS_RANK: Record<LegStatusGroup, number> = {
  // Spec 2026-10-20 §2.3: an untracked leg (Tube, bus, walk) ranks below
  // every train state, so a journey with any train leg is summarised by
  // its trains; one of untracked legs only, by the worst of those.
  untrackedDone: -3,
  untrackedPlanned: -2,
  untrackedInProgress: -1,
  good: 0,
  // A timetabled train whose first Network Rail report isn't due yet
  // (spec §4): nothing is wrong, but it isn't confirmed running either.
  planned: 1,
  awaiting: 2,
  delayed: 3,
  // Not in the timetable (spec §3): the traveller should check the UID or
  // date, a step short of a leg that still needs a train picked.
  noTimetable: 4,
  unmatched: 5,
  skipped: 6,
  severe: 7,
};

/** Whether this leg's matched train has stopped calling at one of the two
 * stations the leg actually needs -- its own origin or its own destination
 * (`crates/api/src/data/station_skip.rs`'s `LegSkipStatus`, surfaced as
 * `JourneyLegDetail.legSkip`). `null` means "nothing to report", NOT
 * "checked and clean" -- see `LegSkipStatus`'s own doc comment -- so it is
 * deliberately treated as no signal rather than as a negative one. */
function legIsSkipped(leg: JourneyLegDetail): boolean {
  return leg.legSkip?.originSkipped === true || leg.legSkip?.destinationSkipped === true;
}

/** Classifies one leg. Delay threshold is `delayMinutes > 0`, matching
 * `TrainJourney.tsx`'s own existing per-leg delay badge exactly (not the
 * notifier's separate 15-minute escalation threshold) -- see this plan's
 * Judgment Call 4.
 *
 * The `legIsSkipped` arm sits directly under the cancellation arm and
 * ABOVE the delay/awaiting/good arms, so its `LEG_STATUS_RANK` position
 * and its position in this branch chain agree: a running, punctual train
 * that is no longer calling at the traveller's own station used to fall
 * straight through to `'good'` and roll the whole journey up as "On
 * track" (2026-09-22 UX review, I13). A CANCELLED train still wins --
 * "this train isn't running" subsumes "this train isn't stopping". */
export function legStatusGroup(leg: JourneyLegDetail): LegStatusGroup {
  if (isUntrackedLeg(leg)) {
    if (leg.untrackedStatus === 'inProgress') return 'untrackedInProgress';
    if (leg.untrackedStatus === 'done') return 'untrackedDone';
    return 'untrackedPlanned';
  }
  if (!leg.trackedTrainState) {
    return 'unmatched';
  }
  const state = leg.trackedTrainState;
  if (state.status === 'cancelled') return 'severe';
  if (legIsSkipped(leg)) return 'skipped';
  // `trackingPhase` (spec 2026-10-20 §3, §4) before the waiting arm: a
  // train with no timetable and no report, or one whose first report
  // isn't due yet, used to read "Awaiting first report" too.
  if (state.trackingPhase === 'noTimetable') return 'noTimetable';
  if (state.trackingPhase === 'planned') return 'planned';
  if (state.status === 'awaiting_activation' || state.status === null) return 'awaiting';
  if (state.delayMinutes !== null && state.delayMinutes > 0) return 'delayed';
  return 'good'; // 'en_route' with no reported delay, or 'completed'.
}

/** The same classification applied to one row of `GET /Journeys/mine`
 * (`JourneyListItem`), which flattens a single "current leg"'s fields
 * instead of nesting a `TrackedTrainState` -- see
 * `crates/api/src/data/journeys.rs::JourneyListItem`. Deliberately reuses
 * `legStatusGroup` by adapting the flat row onto the shape it already
 * classifies, rather than restating the branch chain: two copies of this
 * decision would be free to disagree about what "On track" means on a
 * list page versus a detail page.
 *
 * ONE honest gap, and it is the wire format's, not this function's: the
 * list row carries no `legSkip`, so a skipped leg reads here as whatever
 * its delay/status says (typically "On track") and only shows its real
 * status once opened. Widening `GET /Journeys/mine` to run
 * `station_skip::leg_skip_status` per row means one or two extra Darwin
 * sample reads per journey on a list endpoint -- deferred deliberately,
 * not overlooked. */
export function journeyListItemStatusGroup(item: JourneyListItem): LegStatusGroup {
  return legStatusGroup({
    id: item.legId,
    originCrs: item.originCrs,
    // `JourneyListItem` resolves the same two station names the detail
    // leg does, so the adapter passes them straight through rather than
    // nulling them -- they play no part in the classification below, but
    // an adapter that silently drops a field it HAS is a trap for the
    // next person who reads one of them in `legStatusGroup`.
    originName: item.originName,
    destinationCrs: item.destinationCrs,
    destinationName: item.destinationName,
    serviceDate: item.serviceDate,
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: item.matchMode,
    // `JourneyListItem` carries no windowSearched field (same flattened-row
    // gap as `legSkip` below) -- legStatusGroup never reads it, so the
    // adapter just needs a valid placeholder, not the real value.
    windowSearched: false,
    kind: item.kind ?? 'train',
    untrackedStatus: item.untrackedStatus ?? null,
    trackedTrainState:
      item.trainSubscriptionId === null
        ? null
        : ({
            status: item.status,
            delayMinutes: item.delayMinutes,
            resolutionStatus: item.resolutionStatus,
            trackingPhase: item.trackingPhase ?? null,
          } as JourneyLegDetail['trackedTrainState']),
    legSkip: null,
  });
}

/** The same classification for a train tracked on its own (`GET
 * /Train/mine`'s `TrackedTrainListItem`, or a group-shared train), so a
 * lone train and a journey whose current leg is that train read the same
 * on `/track/mine`. Adapts the row onto `journeyListItemStatusGroup`'s own
 * input rather than restating the branch chain. A tracked train always
 * has a subscription, so it never reads as "Needs a train picked". */
export function trackedTrainListStatusGroup(
  train: {
    serviceDate: string;
    status: string | null;
    delayMinutes: number | null;
    resolutionStatus: string;
  } & TrackingPhaseFields,
): LegStatusGroup {
  return legStatusGroup({
    id: 0,
    originCrs: null,
    originName: null,
    destinationCrs: null,
    destinationName: null,
    serviceDate: train.serviceDate,
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    windowSearched: false,
    kind: 'train',
    untrackedStatus: null,
    trackedTrainState: {
      status: train.status,
      delayMinutes: train.delayMinutes,
      resolutionStatus: train.resolutionStatus,
      trackingPhase: train.trackingPhase ?? null,
    } as JourneyLegDetail['trackedTrainState'],
    legSkip: null,
  });
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
  const [firstLeg, ...laterLegs] = legs;
  if (firstLeg === undefined) return null;
  return laterLegs.reduce(
    (worst, leg) => (legStatusRank(leg) > LEG_STATUS_RANK[worst] ? legStatusGroup(leg) : worst),
    legStatusGroup(firstLeg),
  );
}

/** A journey's headline status: a leg's group, or one of three that only a
 * whole journey has. `partCancelled`: some train legs are cancelled, not
 * all. `connectionAtRisk`/`connectionMissed`: a late leg leaves little or
 * no time to make the next one (journey page only: the list row has no
 * stop times). */
export type JourneySummaryGroup = LegStatusGroup | 'partCancelled' | 'connectionAtRisk' | 'connectionMissed';

/** Where the journey is as a whole: no leg started yet, under way, or
 * every leg arrived (a completed train, an untracked leg past its planned
 * arrival) or cancelled. */
export type JourneyPhase = 'upcoming' | 'inProgress' | 'completed';

/** `LEG_STATUS_RANK` with the three journey-only groups slotted in: a tight
 * connection beside a delay, a missed one beside "Needs a train picked"
 * (both stop the journey running as planned), part cancelled just below
 * cancelled. */
const SUMMARY_RANK: Record<JourneySummaryGroup, number> = {
  ...LEG_STATUS_RANK,
  connectionAtRisk: 3.5,
  connectionMissed: 5.5,
  partCancelled: 6.5,
};

/** A late arrival with less than this many minutes to change is "at risk".
 * No station's own minimum interchange time is known here, so this is a
 * plain margin, applied only when the arriving leg is running late. */
export const CONNECTION_AT_RISK_MINUTES = 5;

export interface JourneySummary {
  group: JourneySummaryGroup;
  phase: JourneyPhase;
  cancelledLegCount: number;
  /** The delay at the final destination (the last train leg's arrival),
   * which is what decides whether the journey is on time. */
  finalArrivalDelayMinutes: number | null;
  finalArrivalDelayProvisional: boolean;
}

/** What `summariseJourney` reads, the same for the journey page (built from
 * every leg) and a `GET /Journeys/mine` row (built from its counts). */
export interface JourneySummaryInput {
  /** The current leg's group: the earliest leg not yet completed (or done),
   * else the last, as `GET /Journeys/mine` picks it. */
  currentLegGroup: LegStatusGroup;
  /** The other legs' groups. Only problems head the journey from here: a
   * leg not in the timetable, not stopping at the traveller's station, or
   * still needing a train. */
  otherLegGroups: readonly LegStatusGroup[];
  legCount: number;
  trainLegCount: number;
  cancelledLegCount: number;
  unmatchedLegCount: number;
  finishedLegCount: number;
  startedLegCount: number;
  finalArrivalDelayMinutes: number | null;
  finalArrivalDelayProvisional: boolean;
  connection: 'atRisk' | 'missed' | null;
}

const OTHER_LEG_FLAGS: ReadonlySet<LegStatusGroup> = new Set(['noTimetable', 'skipped', 'unmatched']);

/** The one journey-level rule (2026-10-21), for the Track list, the journey
 * page header and the home page alike.
 *
 *  - On time or late is judged at the final destination: the last train
 *    leg's arrival delay. An earlier leg's delay counts only through a
 *    missed or tight connection; the journey page's per-leg line still
 *    shows it. A leg's own "Delayed" never heads the journey.
 *  - Any cancelled train leg flags the journey: "Cancelled" when every
 *    train leg is, "Part cancelled" otherwise, even if the traveller still
 *    got there on time (the badge then sits beside the final delay).
 *  - Otherwise the worst of: the current leg's own state, any leg still
 *    needing a train, problems on other legs, the connection and the final
 *    delay, by `SUMMARY_RANK`.
 *  - A leg with no timetable or an untracked leg never blanks the summary:
 *    the phase counts it like any other leg, and the final delay comes
 *    from the last train leg. */
export function summariseJourney(input: JourneySummaryInput): JourneySummary {
  const current = input.currentLegGroup;
  const candidates: JourneySummaryGroup[] = [
    current === 'delayed' || current === 'severe' ? 'good' : current,
    ...input.otherLegGroups.filter((group) => OTHER_LEG_FLAGS.has(group)),
  ];
  if (input.unmatchedLegCount > 0) candidates.push('unmatched');
  if (input.finalArrivalDelayMinutes !== null && input.finalArrivalDelayMinutes > 0) candidates.push('delayed');
  if (input.connection === 'missed') candidates.push('connectionMissed');
  if (input.connection === 'atRisk') candidates.push('connectionAtRisk');
  if (input.cancelledLegCount > 0) {
    candidates.push(input.cancelledLegCount >= input.trainLegCount ? 'severe' : 'partCancelled');
  }
  const group = candidates.reduce((worst, next) => (SUMMARY_RANK[next] > SUMMARY_RANK[worst] ? next : worst));

  // Completed once every leg has arrived or been cancelled, and at least
  // one arrived: a wholly cancelled journey stays active (and red) until
  // its day is over, a part-cancelled one that got there is finished.
  let phase: JourneyPhase = 'inProgress';
  if (input.finishedLegCount > 0 && input.finishedLegCount + input.cancelledLegCount >= input.legCount) {
    phase = 'completed';
  } else if (input.startedLegCount === 0) {
    phase = 'upcoming';
  }

  return {
    group,
    phase,
    cancelledLegCount: input.cancelledLegCount,
    finalArrivalDelayMinutes: input.finalArrivalDelayMinutes,
    finalArrivalDelayProvisional: input.finalArrivalDelayProvisional,
  };
}

function trainStatus(leg: JourneyLegDetail): string | null {
  return isUntrackedLeg(leg) ? null : (leg.trackedTrainState?.status ?? null);
}

/** Arrived: a completed train, or an untracked leg that is done. */
function legFinished(leg: JourneyLegDetail): boolean {
  if (isUntrackedLeg(leg)) return leg.untrackedStatus === 'done';
  return trainStatus(leg) === 'completed';
}

function legStarted(leg: JourneyLegDetail): boolean {
  if (isUntrackedLeg(leg)) return leg.untrackedStatus === 'inProgress' || leg.untrackedStatus === 'done';
  const status = trainStatus(leg);
  return status === 'en_route' || status === 'completed';
}

/** The connection the journey is most at risk on: between a leg that is
 * running (not cancelled) and a next leg that hasn't started, so a
 * connection already made, or one no longer on offer, isn't judged. Uses
 * the journey page's own gap (`connectionGapMinutes`). */
function worstConnection(legs: readonly JourneyLegDetail[]): 'atRisk' | 'missed' | null {
  let worst: 'atRisk' | null = null;
  for (const [index, leg] of legs.entries()) {
    const next = legs[index + 1];
    if (next === undefined) break;
    if (trainStatus(leg) === 'cancelled' || trainStatus(next) === 'cancelled' || legStarted(next)) continue;
    const gap = connectionGapMinutes(leg, next);
    if (gap === null) continue;
    if (gap < 0) return 'missed';
    const late = (leg.trackedTrainState?.delayMinutes ?? 0) > 0;
    if (late && gap < CONNECTION_AT_RISK_MINUTES) worst = 'atRisk';
  }
  return worst;
}

/** The summary from a journey's full legs (`GET /Journeys/{id}`). `null`
 * for a journey with no legs. */
export function journeyDetailSummary(legs: readonly JourneyLegDetail[]): JourneySummary | null {
  const notCompleted = legs.findIndex(
    (leg) => !(trainStatus(leg) === 'completed' || (isUntrackedLeg(leg) && leg.untrackedStatus === 'done')),
  );
  const currentIndex = notCompleted === -1 ? legs.length - 1 : notCompleted;
  const currentLeg = legs[currentIndex];
  if (currentLeg === undefined) return null;
  const trainLegs = legs.filter((leg) => !isUntrackedLeg(leg));
  const finalState = trainLegs[trainLegs.length - 1]?.trackedTrainState ?? null;
  const finalArrives = finalState !== null && finalState.status !== 'cancelled';
  return summariseJourney({
    currentLegGroup: legStatusGroup(currentLeg),
    otherLegGroups: legs.filter((_, index) => index !== currentIndex).map(legStatusGroup),
    legCount: legs.length,
    trainLegCount: trainLegs.length,
    cancelledLegCount: trainLegs.filter((leg) => trainStatus(leg) === 'cancelled').length,
    unmatchedLegCount: trainLegs.filter((leg) => leg.trackedTrainState === null).length,
    finishedLegCount: legs.filter(legFinished).length,
    startedLegCount: legs.filter(legStarted).length,
    finalArrivalDelayMinutes: finalArrives ? finalState.delayMinutes : null,
    finalArrivalDelayProvisional: finalArrives ? (finalState.delayProvisional ?? false) : false,
    connection: worstConnection(legs),
  });
}

/** The summary from one `GET /Journeys/mine` row. With the journey-wide
 * counts the row carries, this is the journey page's rule, less the
 * connection (no stop times on the list) and problems on legs other than
 * the current one. A backend that predates the counts gets the current
 * leg alone, as the list showed before. */
export function journeyListItemSummary(item: JourneyListItem): JourneySummary {
  const current = journeyListItemStatusGroup(item);
  const legCount = item.legCount ?? 1;
  const currentFinished = item.status === 'completed' || current === 'untrackedDone';
  const currentStarted = currentFinished || item.status === 'en_route' || current === 'untrackedInProgress';
  const hasFinalDelay = item.finalArrivalDelayMinutes !== undefined;
  return summariseJourney({
    currentLegGroup: current,
    otherLegGroups: [],
    legCount: item.finishedLegCount === undefined ? 1 : legCount,
    // Without the counts, a cancelled current leg of a multi-leg journey
    // can't be told from a wholly cancelled journey; it reads "Part
    // cancelled", which is true either way that some of it is.
    trainLegCount: item.trainLegCount ?? (current === 'severe' && legCount > 1 ? legCount : 1),
    cancelledLegCount: item.cancelledLegCount ?? (current === 'severe' ? 1 : 0),
    unmatchedLegCount: item.unmatchedLegCount ?? (current === 'unmatched' ? 1 : 0),
    finishedLegCount: item.finishedLegCount ?? (currentFinished ? 1 : 0),
    startedLegCount: item.startedLegCount ?? (currentStarted ? 1 : 0),
    finalArrivalDelayMinutes: hasFinalDelay ? (item.finalArrivalDelayMinutes ?? null) : item.delayMinutes,
    finalArrivalDelayProvisional: hasFinalDelay
      ? (item.finalArrivalDelayProvisional ?? false)
      : (item.delayProvisional ?? false),
    connection: null,
  });
}
