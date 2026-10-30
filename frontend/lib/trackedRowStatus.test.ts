import { describe, it, expect } from 'vitest';
import {
  groupJourneyRowStatus,
  journeyListItemCancellation,
  journeyRowStatus,
  trackedTrainRowStatus,
  type TrackedTrainRowInput,
  viewerCanPickTrain,
} from './trackedRowStatus';
import type { GroupJourney, JourneyListItem, ListCancellation } from './types';

const TODAY = '2026-10-21';
const PAST = '2026-10-20';

function train(overrides: Partial<TrackedTrainRowInput> = {}): TrackedTrainRowInput {
  return {
    serviceDate: TODAY,
    resolutionStatus: 'resolved',
    status: 'en_route',
    delayMinutes: 0,
    trackingPhase: 'live',
    ...overrides,
  };
}

/** A journey whose current leg is the same train as `train()`. */
function journey(overrides: Partial<JourneyListItem> = {}): JourneyListItem {
  return {
    id: 1,
    customName: null,
    createdAt: '2026-10-01T09:00:00Z',
    legId: 10,
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    serviceDate: TODAY,
    originName: null,
    destinationName: null,
    legCount: 1,
    matchMode: 'auto',
    trainSubscriptionId: 5,
    resolutionStatus: 'resolved',
    status: 'en_route',
    delayMinutes: 0,
    trackingPhase: 'live',
    ...overrides,
  };
}

/** The same state as a lone train, a one-leg journey and a three-leg one. */
function allThree(fields: Partial<TrackedTrainRowInput & JourneyListItem>) {
  const { kind: _kind, ...trainFields } = fields;
  return {
    lone: trackedTrainRowStatus(train(trainFields), TODAY),
    oneLeg: journeyRowStatus(journey(fields), TODAY),
    multiLeg: journeyRowStatus(journey({ ...fields, legCount: 3 }), TODAY),
  };
}

describe('tracked row status: a lone train and a journey read the same', () => {
  it('active, on time: the delay badge, not finished', () => {
    for (const status of Object.values(allThree({ status: 'en_route', delayMinutes: 0 }))) {
      expect(status).toMatchObject({ kind: 'good', finished: false, delayMinutes: 0 });
    }
  });

  it('active, late', () => {
    for (const status of Object.values(allThree({ status: 'en_route', delayMinutes: 12 }))) {
      expect(status).toMatchObject({ kind: 'delayed', finished: false, delayMinutes: 12 });
    }
  });

  it('upcoming: planned, then awaiting its first report', () => {
    for (const status of Object.values(allThree({ status: null, delayMinutes: null, trackingPhase: 'planned' }))) {
      expect(status).toMatchObject({ kind: 'planned', finished: false });
    }
    for (const status of Object.values(
      allThree({ status: 'awaiting_activation', delayMinutes: null, trackingPhase: 'awaitingReport' }),
    )) {
      expect(status).toMatchObject({ kind: 'awaiting', finished: false });
    }
  });

  it('completed: finished, keeping its final delay', () => {
    for (const status of Object.values(allThree({ status: 'completed', delayMinutes: 7 }))) {
      expect(status).toMatchObject({ kind: 'completed', finished: true, delayMinutes: 7 });
    }
  });

  it('cancelled: red while active and still red once its day is over', () => {
    const today = allThree({ status: 'cancelled' });
    expect(today.lone).toMatchObject({ kind: 'severe', finished: false });
    expect(today.oneLeg).toMatchObject({ kind: 'severe', finished: false });
    expect(today.multiLeg).toMatchObject({ kind: 'partCancelled', finished: false });

    expect(trackedTrainRowStatus(train({ status: 'cancelled', serviceDate: PAST }), TODAY)).toMatchObject({
      kind: 'severe',
      finished: true,
    });
    expect(journeyRowStatus(journey({ status: 'cancelled', serviceDate: PAST }), TODAY)).toMatchObject({
      kind: 'severe',
      finished: true,
    });
  });

  it('part-cancelled and arrived: finished, still red, with the final arrival delay', () => {
    const arrived = journey({
      status: 'completed',
      legCount: 2,
      trainLegCount: 2,
      cancelledLegCount: 1,
      finishedLegCount: 1,
      startedLegCount: 1,
      finalArrivalDelayMinutes: 0,
    });
    expect(journeyRowStatus(arrived, TODAY)).toMatchObject({ kind: 'partCancelled', finished: true, delayMinutes: 0 });
  });

  it('part-cancelled: a multi-leg journey with a cancelled leg says so, finished or not', () => {
    expect(journeyRowStatus(journey({ status: 'cancelled', legCount: 2 }), TODAY)).toMatchObject({
      kind: 'partCancelled',
      finished: false,
    });
    expect(journeyRowStatus(journey({ status: 'cancelled', legCount: 2, serviceDate: PAST }), TODAY)).toMatchObject({
      kind: 'partCancelled',
      finished: true,
    });
  });

  it('no timetable: flagged today, and still flagged once past', () => {
    for (const status of Object.values(allThree({ status: null, delayMinutes: null, trackingPhase: 'noTimetable' }))) {
      expect(status).toMatchObject({ kind: 'noTimetable', finished: false });
    }
    expect(
      trackedTrainRowStatus(train({ status: null, trackingPhase: 'noTimetable', serviceDate: PAST }), TODAY),
    ).toMatchObject({ kind: 'noTimetable', finished: true });
    expect(
      journeyRowStatus(journey({ status: null, trackingPhase: 'noTimetable', serviceDate: PAST }), TODAY),
    ).toMatchObject({ kind: 'noTimetable', finished: true });
  });

  it('a past day that never reported reads "no report", for both', () => {
    const past = { status: null, delayMinutes: null, trackingPhase: 'awaitingReport' as const, serviceDate: PAST };
    expect(trackedTrainRowStatus(train(past), TODAY)).toMatchObject({ kind: 'noReport', finished: true });
    expect(journeyRowStatus(journey(past), TODAY)).toMatchObject({ kind: 'noReport', finished: true });
  });

  it('a past day whose reports stopped short of arriving reads "no report", without its stale forecast', () => {
    const stuck = { status: 'en_route', delayMinutes: 4, delayProvisional: true, serviceDate: PAST };
    const expected = { kind: 'noReport', finished: true, delayMinutes: null, delayProvisional: false };
    expect(trackedTrainRowStatus(train(stuck), TODAY)).toMatchObject(expected);
    expect(journeyRowStatus(journey(stuck), TODAY)).toMatchObject(expected);
  });
});

describe('groupJourneyRowStatus: a group row says what the owner sees', () => {
  function shared(overrides: Partial<GroupJourney> = {}): GroupJourney {
    return {
      journeyId: 1,
      customName: null,
      legCount: 2,
      pinOriginCrs: 'KGX',
      pinDestinationCrs: 'EDB',
      pinOriginName: null,
      pinDestinationName: null,
      pinScheduledDeparture: null,
      serviceDate: TODAY,
      resolutionStatus: 'resolved',
      trainUid: 'C1',
      status: 'en_route',
      delayMinutes: 6,
      addedBy: 'user-2',
      addedByName: null,
      addedByTag: null,
      ...overrides,
    };
  }

  it('a running train reads like a lone train', () => {
    expect(groupJourneyRowStatus(shared(), TODAY)).toEqual(
      trackedTrainRowStatus(train({ status: 'en_route', delayMinutes: 6, trackingPhase: null }), TODAY),
    );
  });

  it('a leg with no train picked asks for one', () => {
    expect(groupJourneyRowStatus(shared({ resolutionStatus: null, status: null }), TODAY)).toMatchObject({
      kind: 'unmatched',
    });
  });

  it('an untracked-only journey shows its clock state, and completed once done', () => {
    expect(
      groupJourneyRowStatus(
        shared({ kind: 'untracked', untrackedStatus: 'inProgress', resolutionStatus: null }),
        TODAY,
      ),
    ).toMatchObject({ kind: 'untrackedInProgress', finished: false });
    expect(
      groupJourneyRowStatus(shared({ kind: 'untracked', untrackedStatus: 'done', resolutionStatus: null }), TODAY),
    ).toMatchObject({ kind: 'completed', finished: true });
  });
});

describe('journey-only and train-only states', () => {
  it('a journey still needing a train asks for one today, and stops asking once past', () => {
    const open = { matchMode: 'unmatched' as const, trainSubscriptionId: null, status: null, resolutionStatus: null };
    expect(journeyRowStatus(journey(open), TODAY)).toMatchObject({ kind: 'unmatched', finished: false });
    expect(journeyRowStatus(journey({ ...open, serviceDate: PAST }), TODAY)).toMatchObject({
      kind: 'noTrainPicked',
      finished: true,
    });
  });

  // DS-199: picking a train is the owner's, or a follower's, to do.
  it('asks only the owner or a follower to pick a train; anyone else reads "No train picked yet"', () => {
    const open = { matchMode: 'unmatched' as const, trainSubscriptionId: null, status: null, resolutionStatus: null };
    const sharedVia = [
      { groupId: 'g1', groupName: 'Family', addedByName: 'Sam', addedByTag: null, addedAt: '2026-10-01T00:00:00Z' },
    ];
    const followedFrom = {
      via: 'group' as const,
      ownerName: 'Sam',
      ownerTag: null,
      groupId: 'g1',
      groupName: 'Family',
      followedAt: '2026-10-01T00:00:00Z',
    };
    expect(viewerCanPickTrain(journey(open))).toBe(true);
    expect(viewerCanPickTrain(journey({ ...open, sharedVia }))).toBe(false);
    expect(viewerCanPickTrain(journey({ ...open, sharedVia, followedFrom }))).toBe(true);
    expect(journeyRowStatus(journey(open), TODAY).kind).toBe('unmatched');
    expect(journeyRowStatus(journey({ ...open, followedFrom }), TODAY).kind).toBe('unmatched');
    expect(journeyRowStatus(journey({ ...open, sharedVia }), TODAY)).toMatchObject({
      kind: 'noTrainPickedYet',
      finished: false,
    });
    // Past, it says what anyone would: none was picked.
    expect(journeyRowStatus(journey({ ...open, sharedVia, serviceDate: PAST }), TODAY).kind).toBe('noTrainPicked');
    // A shared journey whose owner picked a train reads by that train.
    expect(journeyRowStatus(journey({ sharedVia, trainSubscriptionId: null }), TODAY).kind).toBe('good');
  });

  it('a journey of untracked legs only is finished once they are done', () => {
    const untracked = {
      kind: 'untracked' as const,
      trainSubscriptionId: null,
      status: null,
      matchMode: 'manual' as const,
    };
    expect(journeyRowStatus(journey({ ...untracked, untrackedStatus: 'inProgress' }), TODAY)).toMatchObject({
      kind: 'untrackedInProgress',
      finished: false,
    });
    expect(journeyRowStatus(journey({ ...untracked, untrackedStatus: 'done' }), TODAY)).toMatchObject({
      kind: 'completed',
      finished: true,
    });
  });

  it('a bus or ferry is timetabled only, and finished by its date', () => {
    const bus = { serviceMode: 'bus' as const, status: null, resolutionStatus: 'schedule_matched' };
    expect(trackedTrainRowStatus(train(bus), TODAY)).toMatchObject({ kind: 'timetableOnly', finished: false });
    expect(trackedTrainRowStatus(train({ ...bus, serviceDate: PAST }), TODAY)).toMatchObject({
      kind: 'timetableOnly',
      finished: true,
      serviceMode: 'bus',
    });
  });

  it('a train still pending its match waits like a journey leg does', () => {
    expect(
      trackedTrainRowStatus(train({ resolutionStatus: 'pending', status: null, trackingPhase: null }), TODAY),
    ).toMatchObject({ kind: 'awaiting', finished: false });
  });
});

describe("tracked row status: the train's own cancellation (DS-181)", () => {
  const york = { crs: 'YRK', name: 'York' };
  const reason = { code: 'JP', text: 'Failure to maintain vegetation', source: 'trust' } as const;
  const terminates: ListCancellation = { kind: 'partial', startsFrom: null, terminatesAt: york, reason };
  const startsFrom: ListCancellation = { kind: 'partial', startsFrom: york, terminatesAt: null, reason: null };
  const middle: ListCancellation = { kind: 'partial', startsFrom: null, terminatesAt: null, reason: null };
  const full: ListCancellation = { kind: 'full', startsFrom: null, terminatesAt: null, reason };

  it('a train TRUST marked cancelled en route that terminates short is part cancelled, not cancelled', () => {
    const { lone, oneLeg } = allThree({ status: 'cancelled', delayMinutes: null, cancellation: terminates });
    for (const status of [lone, oneLeg]) {
      expect(status).toMatchObject({ kind: 'partCancelled', cancellation: terminates });
    }
  });

  it('a change of origin flags a train whose status is still running', () => {
    const { lone, oneLeg } = allThree({ status: 'en_route', delayMinutes: 3, cancellation: startsFrom });
    for (const status of [lone, oneLeg]) {
      expect(status).toMatchObject({ kind: 'partCancelled', delayMinutes: 3, cancellation: startsFrom });
    }
  });

  it('a cancelled middle section is part cancelled too', () => {
    const { lone, oneLeg } = allThree({ status: 'en_route', delayMinutes: 0, cancellation: middle });
    for (const status of [lone, oneLeg]) {
      expect(status).toMatchObject({ kind: 'partCancelled', cancellation: middle });
    }
  });

  it('a wholly cancelled train stays cancelled and carries its reason', () => {
    const { lone, oneLeg } = allThree({ status: 'cancelled', delayMinutes: null, cancellation: full });
    for (const status of [lone, oneLeg]) {
      expect(status).toMatchObject({ kind: 'severe', cancellation: full });
    }
  });

  it('a part-cancelled train that arrived stays flagged once finished', () => {
    const { lone } = allThree({ status: 'completed', delayMinutes: 0, cancellation: terminates });
    expect(lone).toMatchObject({ kind: 'partCancelled', finished: true, cancellation: terminates });
  });

  it('no cancellation reads as before', () => {
    for (const status of Object.values(allThree({ status: 'cancelled', delayMinutes: null, cancellation: null }))) {
      expect(status.kind === 'severe' || status.kind === 'partCancelled').toBe(true);
      expect(status.cancellation ?? null).toBeNull();
    }
    expect(allThree({ status: 'en_route', delayMinutes: 0, cancellation: null }).lone).toMatchObject({
      kind: 'good',
      cancellation: null,
    });
  });

  it("keeps a multi-leg journey's rollup without one train's detail", () => {
    const row = journey({
      legCount: 2,
      trainLegCount: 2,
      cancelledLegCount: 1,
      finishedLegCount: 0,
      startedLegCount: 0,
      unmatchedLegCount: 0,
      status: 'cancelled',
      cancellation: full,
    });
    expect(journeyListItemCancellation(row)).toBeNull();
    expect(journeyRowStatus(row, TODAY)).toMatchObject({ kind: 'partCancelled', cancellation: null });
  });

  it("a multi-leg journey whose current train is part cancelled says so, without that train's detail", () => {
    const row = journey({
      legCount: 2,
      trainLegCount: 2,
      cancelledLegCount: 0,
      finishedLegCount: 0,
      startedLegCount: 1,
      unmatchedLegCount: 0,
      cancellation: terminates,
    });
    expect(journeyRowStatus(row, TODAY)).toMatchObject({ kind: 'partCancelled', cancellation: null });
  });

  it('a journey with one train leg and a walk carries the train detail', () => {
    const row = journey({
      legCount: 2,
      trainLegCount: 1,
      cancelledLegCount: 1,
      finishedLegCount: 0,
      startedLegCount: 0,
      unmatchedLegCount: 0,
      status: 'cancelled',
      cancellation: full,
    });
    expect(journeyListItemCancellation(row)).toBe(full);
    expect(journeyRowStatus(row, TODAY)).toMatchObject({ kind: 'severe', cancellation: full });
  });
});
