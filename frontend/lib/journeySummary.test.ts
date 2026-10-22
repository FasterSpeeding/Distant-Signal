import { describe, it, expect } from 'vitest';
import { journeyDetailSummary, journeyListItemSummary } from './journeyStatus';
import type { JourneyLegDetail, JourneyListItem, JourneyStop, TrackedTrainState } from './types';

function state(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-10-21',
    pinOriginCrs: null,
    pinDestinationCrs: null,
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
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
    trackingPhase: 'live',
    ...overrides,
  };
}

function leg(id: number, from: string, to: string, overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id,
    originCrs: from,
    originName: null,
    destinationCrs: to,
    destinationName: null,
    serviceDate: '2026-10-21',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: state(),
    legSkip: null,
    ...overrides,
  };
}

function stop(crs: string, overrides: Partial<JourneyStop> = {}): JourneyStop {
  return {
    crs,
    name: null,
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Unknown',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...overrides,
  };
}

/** KGX to YRK arriving `arrival` (expected), then YRK to EDB leaving 10:30
 * and not yet started. */
function twoLegs(arrival: string, firstDelay: number, secondDelay: number | null = 0): JourneyLegDetail[] {
  return [
    leg(1, 'KGX', 'YRK', {
      trackedTrainState: state({
        delayMinutes: firstDelay,
        journeyStops: [
          stop('KGX', { kind: 'Origin', scheduledDeparture: '2026-10-21T08:00:00Z' }),
          stop('YRK', { scheduledArrival: '2026-10-21T10:00:00Z', estimatedArrival: arrival }),
        ],
      }),
    }),
    leg(2, 'YRK', 'EDB', {
      trackedTrainState: state({
        status: 'awaiting_activation',
        trackingPhase: 'awaitingReport',
        delayMinutes: secondDelay,
        journeyStops: [stop('YRK', { kind: 'Origin', scheduledDeparture: '2026-10-21T10:30:00Z' })],
      }),
    }),
  ];
}

describe('journeyDetailSummary: the journey is judged at its final destination', () => {
  it("the user's example: first leg late, connection made, last leg on time, so the journey is on time", () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'completed', delayMinutes: 12 }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'en_route', delayMinutes: 0 }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({
      group: 'good',
      phase: 'inProgress',
      finalArrivalDelayMinutes: 0,
    });
  });

  it('the last leg running late makes the journey late', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'completed', delayMinutes: 0 }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'en_route', delayMinutes: 9 }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({ group: 'delayed', finalArrivalDelayMinutes: 9 });
  });

  it('a late first leg with time to spare before the next is still on time', () => {
    // Expected into YRK 10:12, the next leaves 10:30.
    // The next train not reporting yet doesn't head it: the current leg does.
    expect(journeyDetailSummary(twoLegs('2026-10-21T10:12:00Z', 12))).toMatchObject({ group: 'good' });
  });

  it('a late first leg that leaves under five minutes to change is a tight connection', () => {
    expect(journeyDetailSummary(twoLegs('2026-10-21T10:27:00Z', 27))).toMatchObject({ group: 'connectionAtRisk' });
  });

  it('a first leg due in after the next one leaves is a missed connection', () => {
    expect(journeyDetailSummary(twoLegs('2026-10-21T10:40:00Z', 40))).toMatchObject({ group: 'connectionMissed' });
  });

  it("the api's connection, with the station's minimum change time, wins over the page's own gap", () => {
    const withConnection = (legs: JourneyLegDetail[], connection: NonNullable<JourneyLegDetail['connection']>) =>
      legs.map((l, i) => (i === 1 ? { ...l, connection } : l));
    // 18 minutes looks fine without a minimum; the api says missed.
    expect(
      journeyDetailSummary(
        withConnection(twoLegs('2026-10-21T10:12:00Z', 12), {
          gapMinutes: 3,
          minimumChangeMinutes: 5,
          state: 'missed',
        }),
      ),
    ).toMatchObject({ group: 'connectionMissed' });
    // The page's gap says missed, but the next train is late too.
    expect(
      journeyDetailSummary(
        withConnection(twoLegs('2026-10-21T10:40:00Z', 40), { gapMinutes: 8, minimumChangeMinutes: 5, state: 'ok' }),
      ),
    ).toMatchObject({ group: 'good' });
    expect(
      journeyDetailSummary(
        withConnection(twoLegs('2026-10-21T10:12:00Z', 12), {
          gapMinutes: 6,
          minimumChangeMinutes: 5,
          state: 'atRisk',
        }),
      ),
    ).toMatchObject({ group: 'connectionAtRisk' });
    // `unknown` is no signal, and doesn't fall back to the page's gap.
    expect(
      journeyDetailSummary(
        withConnection(twoLegs('2026-10-21T10:40:00Z', 40), {
          gapMinutes: null,
          minimumChangeMinutes: null,
          state: 'unknown',
        }),
      ),
    ).toMatchObject({ group: 'good' });
  });

  it('one cancelled leg of two is part cancelled, beside the final arrival delay', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'completed', delayMinutes: 0 }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'cancelled', delayMinutes: null }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({
      group: 'partCancelled',
      cancelledLegCount: 1,
      // A cancelled last train has no arrival.
      finalArrivalDelayMinutes: null,
    });
  });

  it('a cancelled first leg with the rest on time is part cancelled, and says the arrival was on time', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'cancelled' }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'completed', delayMinutes: 0 }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({
      group: 'partCancelled',
      phase: 'completed',
      finalArrivalDelayMinutes: 0,
    });
  });

  it('every train leg cancelled is cancelled, and stays active until its day is over', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'cancelled' }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'cancelled' }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({ group: 'severe', phase: 'upcoming' });
  });

  it('every leg arrived is completed, with the last leg delay', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'completed', delayMinutes: 20 }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'completed', delayMinutes: 3 }) }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({
      group: 'delayed',
      phase: 'completed',
      finalArrivalDelayMinutes: 3,
    });
  });

  it('nothing started yet is upcoming', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', {
        trackedTrainState: state({ status: null, trackingPhase: 'planned', delayMinutes: null }),
      }),
      leg(2, 'YRK', 'EDB', {
        trackedTrainState: state({ status: null, trackingPhase: 'planned', delayMinutes: null }),
      }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({ group: 'planned', phase: 'upcoming' });
  });

  it('a final walk and a leg with no timetable do not blank the summary', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'completed', delayMinutes: 0 }) }),
      leg(2, 'YRK', 'EDB', { trackedTrainState: state({ status: 'completed', delayMinutes: 4 }) }),
      leg(3, 'EDB', 'EDB', {
        kind: 'untracked',
        trackedTrainState: null,
        untrackedStatus: 'done',
        plannedArrival: '2026-10-21T13:00:00Z',
      }),
    ];
    // The walk is done: the journey is completed, 4 min late from the last train.
    expect(journeyDetailSummary(legs)).toMatchObject({
      group: 'delayed',
      phase: 'completed',
      finalArrivalDelayMinutes: 4,
    });
  });

  it('a later leg not in the timetable still flags the journey', () => {
    const legs: JourneyLegDetail[] = [
      leg(1, 'KGX', 'YRK', { trackedTrainState: state({ status: 'en_route', delayMinutes: 0 }) }),
      leg(2, 'YRK', 'EDB', {
        trackedTrainState: state({ status: null, trackingPhase: 'noTimetable', delayMinutes: null }),
      }),
    ];
    expect(journeyDetailSummary(legs)).toMatchObject({ group: 'noTimetable', phase: 'inProgress' });
  });

  it('has no summary without legs', () => {
    expect(journeyDetailSummary([])).toBeNull();
  });
});

function row(overrides: Partial<JourneyListItem> = {}): JourneyListItem {
  return {
    id: 1,
    customName: null,
    createdAt: '2026-10-01T09:00:00Z',
    legId: 10,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    serviceDate: '2026-10-21',
    originName: null,
    destinationName: null,
    legCount: 2,
    matchMode: 'manual',
    trainSubscriptionId: 5,
    resolutionStatus: 'resolved',
    status: 'en_route',
    delayMinutes: 12,
    trackingPhase: 'live',
    trainLegCount: 2,
    cancelledLegCount: 0,
    unmatchedLegCount: 0,
    finishedLegCount: 0,
    startedLegCount: 1,
    finalArrivalDelayMinutes: 0,
    finalArrivalDelayProvisional: true,
    ...overrides,
  };
}

describe('journeyListItemSummary: the same rule from a list row', () => {
  it('a late current leg with the final arrival on time reads on time', () => {
    expect(journeyListItemSummary(row())).toMatchObject({
      group: 'good',
      phase: 'inProgress',
      finalArrivalDelayMinutes: 0,
      finalArrivalDelayProvisional: true,
    });
  });

  it('a cancelled leg among running ones is part cancelled', () => {
    expect(journeyListItemSummary(row({ cancelledLegCount: 1 }))).toMatchObject({ group: 'partCancelled' });
  });

  it('every train leg cancelled is cancelled', () => {
    expect(journeyListItemSummary(row({ status: 'cancelled', cancelledLegCount: 2 }))).toMatchObject({
      group: 'severe',
    });
  });

  it('every leg arrived is completed', () => {
    expect(
      journeyListItemSummary(
        row({ status: 'completed', finishedLegCount: 2, startedLegCount: 2, finalArrivalDelayMinutes: 6 }),
      ),
    ).toMatchObject({ group: 'delayed', phase: 'completed', finalArrivalDelayMinutes: 6 });
  });

  it('a later leg still needing a train flags the journey', () => {
    expect(journeyListItemSummary(row({ unmatchedLegCount: 1 }))).toMatchObject({ group: 'unmatched' });
  });

  it('without the counts (an older backend) it reads the current leg as before', () => {
    const {
      trainLegCount: _trainLegCount,
      cancelledLegCount: _cancelledLegCount,
      unmatchedLegCount: _unmatchedLegCount,
      finishedLegCount: _finishedLegCount,
      startedLegCount: _startedLegCount,
      finalArrivalDelayMinutes: _finalArrivalDelayMinutes,
      finalArrivalDelayProvisional: _finalArrivalDelayProvisional,
      ...old
    } = row();
    expect(journeyListItemSummary(old)).toMatchObject({ group: 'delayed', finalArrivalDelayMinutes: 12 });
    expect(journeyListItemSummary({ ...old, status: 'cancelled' })).toMatchObject({ group: 'partCancelled' });
    expect(journeyListItemSummary({ ...old, status: 'cancelled', legCount: 1 })).toMatchObject({ group: 'severe' });
  });
});
