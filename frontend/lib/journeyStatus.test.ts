import { describe, it, expect } from 'vitest';
import { journeyListItemStatusGroup, legStatusGroup, legStatusRank, worstLegStatus } from './journeyStatus';
import type { JourneyLegDetail, JourneyListItem, TrackedTrainState } from './types';

function baseTrackedTrainState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-08-28',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    sharedGroupCount: 0,
    customName: null,
    ...overrides,
  };
}

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'WAT',
    originName: null,
    destinationCrs: 'WOK',
    destinationName: null,
    serviceDate: '2026-08-28',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'auto',
    trackedTrainState: baseTrackedTrainState(),
    // Integration (2026-09-22): journey Phase 3 made `legSkip` a REQUIRED
    // field on `JourneyLegDetail` -- the API always emits it, `null` for a
    // leg with no matched train yet and an object once one is bound. These
    // Phase 2 factories predate that field; `null` is the right default
    // here because nothing in these suites exercises skip detection.
    legSkip: null,
    ...overrides,
  };
}

describe('legStatusGroup', () => {
  it('classifies a leg with no tracked train state as unmatched', () => {
    const leg = baseLeg({ trackedTrainState: null, matchMode: 'unmatched' });
    expect(legStatusGroup(leg)).toBe('unmatched');
  });

  it('classifies a cancelled tracked train as severe', () => {
    const leg = baseLeg({ trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) });
    expect(legStatusGroup(leg)).toBe('severe');
  });

  it('classifies an awaiting_activation tracked train as awaiting', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'awaiting_activation' }),
    });
    expect(legStatusGroup(leg)).toBe('awaiting');
  });

  it('classifies a tracked train with a null status as awaiting (not yet resolved)', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: null, resolutionStatus: 'pending' }),
    });
    expect(legStatusGroup(leg)).toBe('awaiting');
  });

  it('classifies an en_route train with a positive delay as delayed', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 5 }),
    });
    expect(legStatusGroup(leg)).toBe('delayed');
  });

  it('classifies an en_route train with no reported delay as good', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }),
    });
    expect(legStatusGroup(leg)).toBe('good');
  });

  it('classifies an en_route train with a zero delay as good (threshold is > 0, not >= 0)', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 0 }),
    });
    expect(legStatusGroup(leg)).toBe('good');
  });

  it('classifies a completed train with no reported delay as good', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'completed', delayMinutes: null }),
    });
    expect(legStatusGroup(leg)).toBe('good');
  });

  it('classifies a completed train with a stale positive delay figure as delayed (the function does not special-case status when delayMinutes is positive)', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'completed', delayMinutes: 12 }),
    });
    expect(legStatusGroup(leg)).toBe('delayed');
  });

  // 2026-09-22 UX review, I13: `legStatusGroup` read only `status` and
  // `delayMinutes`, so a punctual train that had stopped calling at the
  // traveller's own station rolled the whole journey up as "On track".
  it('classifies an on-time en_route train whose leg DESTINATION is skipped as skipped, not good', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 0 }),
      legSkip: { originSkipped: false, destinationSkipped: true },
    });
    expect(legStatusGroup(leg)).toBe('skipped');
  });

  it('classifies an on-time en_route train whose leg ORIGIN is skipped as skipped', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }),
      legSkip: { originSkipped: true, destinationSkipped: false },
    });
    expect(legStatusGroup(leg)).toBe('skipped');
  });

  it('lets a skip outrank a delay on the same leg', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 22 }),
      legSkip: { originSkipped: false, destinationSkipped: true },
    });
    expect(legStatusGroup(leg)).toBe('skipped');
  });

  it('still reports a CANCELLED train as severe even when a skip is also flagged', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }),
      legSkip: { originSkipped: true, destinationSkipped: true },
    });
    expect(legStatusGroup(leg)).toBe('severe');
  });

  it('treats a legSkip with both flags false as no signal at all', () => {
    const leg = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }),
      legSkip: { originSkipped: false, destinationSkipped: false },
    });
    expect(legStatusGroup(leg)).toBe('good');
  });
});

describe('legStatusRank', () => {
  it('orders the six groups good < awaiting < delayed < unmatched < skipped < severe', () => {
    const good = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }),
    });
    const awaiting = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'awaiting_activation' }),
    });
    const delayed = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 10 }),
    });
    const unmatched = baseLeg({ trackedTrainState: null, matchMode: 'unmatched' });
    const skipped = baseLeg({
      trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }),
      legSkip: { originSkipped: false, destinationSkipped: true },
    });
    const severe = baseLeg({ trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) });

    expect(legStatusRank(good)).toBeLessThan(legStatusRank(awaiting));
    expect(legStatusRank(awaiting)).toBeLessThan(legStatusRank(delayed));
    expect(legStatusRank(delayed)).toBeLessThan(legStatusRank(unmatched));
    expect(legStatusRank(unmatched)).toBeLessThan(legStatusRank(skipped));
    expect(legStatusRank(skipped)).toBeLessThan(legStatusRank(severe));
  });
});

describe('worstLegStatus', () => {
  it('returns null for a journey with no legs', () => {
    expect(worstLegStatus([])).toBeNull();
  });

  it('rolls up to good when every leg is good', () => {
    const legs = [
      baseLeg({ id: 1, trackedTrainState: baseTrackedTrainState({ status: 'en_route' }) }),
      baseLeg({ id: 2, trackedTrainState: baseTrackedTrainState({ status: 'completed' }) }),
    ];
    expect(worstLegStatus(legs)).toBe('good');
  });

  it('an unmatched leg outranks a merely-delayed leg (spec §3: "needs a train picked" is worse than a known-late train)', () => {
    const legs = [
      baseLeg({
        id: 1,
        trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 20 }),
      }),
      baseLeg({ id: 2, trackedTrainState: null, matchMode: 'unmatched' }),
    ];
    expect(worstLegStatus(legs)).toBe('unmatched');
  });

  it('a cancelled leg outranks both an unmatched leg and a delayed leg', () => {
    const legs = [
      baseLeg({
        id: 1,
        trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 20 }),
      }),
      baseLeg({ id: 2, trackedTrainState: null, matchMode: 'unmatched' }),
      baseLeg({ id: 3, trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) }),
    ];
    expect(worstLegStatus(legs)).toBe('severe');
  });

  it('picks the single worst leg regardless of its position in the array', () => {
    const legs = [
      baseLeg({ id: 1, trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) }),
      baseLeg({ id: 2, trackedTrainState: baseTrackedTrainState({ status: 'en_route' }) }),
    ];
    expect(worstLegStatus(legs)).toBe('severe');
  });
});

/** Spec 2026-10-20: untracked legs and the planned / not-in-the-timetable
 * phases in the journey rollup. */
describe('legStatusGroup: untracked legs and tracking phases', () => {
  const untracked = (untrackedStatus: 'planned' | 'inProgress' | 'done', id = 9) =>
    baseLeg({
      id,
      kind: 'untracked',
      originCrs: null,
      destinationCrs: null,
      trackedTrainState: null,
      transitMode: 'tube',
      untrackedStatus,
    });

  it('an untracked leg is never "needs a train picked"', () => {
    expect(legStatusGroup(untracked('planned'))).toBe('untrackedPlanned');
    expect(legStatusGroup(untracked('inProgress'))).toBe('untrackedInProgress');
    expect(legStatusGroup(untracked('done'))).toBe('untrackedDone');
  });

  it('a train leg reads its trackingPhase before the waiting arm', () => {
    const phase = (trackingPhase: 'planned' | 'noTimetable', status: TrackedTrainState['status'] = null) =>
      legStatusGroup(baseLeg({ trackedTrainState: baseTrackedTrainState({ status, trackingPhase }) }));
    expect(phase('planned')).toBe('planned');
    expect(phase('planned', 'awaiting_activation')).toBe('planned');
    expect(phase('noTimetable')).toBe('noTimetable');
    expect(
      legStatusGroup(
        baseLeg({ trackedTrainState: baseTrackedTrainState({ status: null, trackingPhase: 'awaitingReport' }) }),
      ),
    ).toBe('awaiting');
    expect(
      legStatusGroup(
        baseLeg({ trackedTrainState: baseTrackedTrainState({ status: 'cancelled', trackingPhase: 'live' }) }),
      ),
    ).toBe('severe');
  });

  it('ranks untracked below every train state, then planned between good and awaiting', () => {
    const train = (overrides: Partial<TrackedTrainState>) =>
      baseLeg({ trackedTrainState: baseTrackedTrainState(overrides) });
    const order = [
      untracked('done'),
      untracked('planned'),
      untracked('inProgress'),
      train({ status: 'en_route', delayMinutes: 0 }),
      train({ status: null, trackingPhase: 'planned' }),
      train({ status: null }),
      train({ status: 'en_route', delayMinutes: 5 }),
      train({ status: null, trackingPhase: 'noTimetable' }),
      baseLeg({ trackedTrainState: null }),
    ];
    const ranks = order.map(legStatusRank);
    expect([...ranks].sort((a, b) => a - b)).toEqual(ranks);
    expect(new Set(ranks).size).toBe(ranks.length);
  });

  it('a journey with a train leg is summarised by its trains, one of untracked legs by those', () => {
    const onTime = baseLeg({ trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: 0 }) });
    expect(worstLegStatus([untracked('inProgress'), onTime])).toBe('good');
    expect(worstLegStatus([untracked('done'), untracked('inProgress', 10)])).toBe('untrackedInProgress');
  });

  it('a /Journeys/mine row adapts its kind and phase', () => {
    const row = (overrides: Partial<JourneyListItem>): JourneyListItem => ({
      id: 1,
      customName: null,
      createdAt: '2026-10-20T00:00:00Z',
      legId: 1,
      originCrs: null,
      destinationCrs: null,
      serviceDate: '2026-10-21',
      originName: null,
      destinationName: null,
      matchMode: 'manual',
      trainSubscriptionId: null,
      resolutionStatus: null,
      status: null,
      delayMinutes: null,
      ...overrides,
    });
    expect(journeyListItemStatusGroup(row({ kind: 'untracked', untrackedStatus: 'inProgress' }))).toBe(
      'untrackedInProgress',
    );
    expect(
      journeyListItemStatusGroup(
        row({ kind: 'train', trainSubscriptionId: 4, resolutionStatus: 'pending', trackingPhase: 'planned' }),
      ),
    ).toBe('planned');
    expect(
      journeyListItemStatusGroup(
        row({ kind: 'train', trainSubscriptionId: 4, resolutionStatus: 'pending', trackingPhase: 'noTimetable' }),
      ),
    ).toBe('noTimetable');
    expect(journeyListItemStatusGroup(row({ kind: 'train' }))).toBe('unmatched');
  });

  // DS-199: a shared or followed row has the owner's subscription id
  // blanked, but still carries the train the owner picked.
  it('a shared row with no subscription id but a picked train reads by that train', () => {
    const row: JourneyListItem = {
      id: 1,
      customName: null,
      createdAt: '2026-10-20T00:00:00Z',
      legId: 1,
      originCrs: 'KGX',
      destinationCrs: 'EDB',
      serviceDate: '2026-10-21',
      originName: null,
      destinationName: null,
      matchMode: 'manual',
      trainSubscriptionId: null,
      resolutionStatus: 'resolved',
      status: 'en_route',
      delayMinutes: 7,
      kind: 'train',
      sharedVia: [],
    };
    expect(journeyListItemStatusGroup(row)).toBe('delayed');
    expect(journeyListItemStatusGroup({ ...row, resolutionStatus: null, status: null })).toBe('unmatched');
  });
});
