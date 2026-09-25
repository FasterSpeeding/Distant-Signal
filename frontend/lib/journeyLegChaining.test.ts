import { describe, it, expect } from 'vitest';
import { journeyCanAddLeg, journeyPriorDestinationCrs } from './journeyLegChaining';
import type { JourneyDetail, JourneyLegDetail, TrackedTrainState } from './types';

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'WAT',
    originName: null,
    destinationCrs: 'CLJ',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

function trackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'CLJ',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P12345',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'CLJ',
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function journey(legs: JourneyLegDetail[]): JourneyDetail {
  return { id: 1, customName: null, createdAt: '2026-09-22T00:00:00Z', legs, isOwner: true, shareLink: null };
}

describe('journeyPriorDestinationCrs', () => {
  it('returns null for a journey with no legs', () => {
    expect(journeyPriorDestinationCrs(journey([]))).toBeNull();
  });

  it("prefers the last leg's own destinationCrs", () => {
    expect(journeyPriorDestinationCrs(journey([leg({ destinationCrs: 'RDG' })]))).toBe('RDG');
  });

  it("falls back to the matched train's scheduleDestinationCrs when the leg's own destinationCrs is null", () => {
    const j = journey([
      leg({ destinationCrs: null, trackedTrainState: trackedState({ scheduleDestinationCrs: 'EDB' }) }),
    ]);
    expect(journeyPriorDestinationCrs(j)).toBe('EDB');
  });

  it('reads off the LAST leg, not the first, for a multi-leg journey', () => {
    const j = journey([leg({ id: 1, destinationCrs: 'YRK' }), leg({ id: 2, destinationCrs: 'NCL' })]);
    expect(journeyPriorDestinationCrs(j)).toBe('NCL');
  });

  it('returns null when neither the leg nor its matched train has a destination', () => {
    const j = journey([leg({ destinationCrs: null, trackedTrainState: null })]);
    expect(journeyPriorDestinationCrs(j)).toBeNull();
  });
});

describe('journeyCanAddLeg', () => {
  it('is false for a journey with no legs', () => {
    expect(journeyCanAddLeg(journey([]))).toBe(false);
  });

  it('is false while the last leg is unmatched (no trackedTrainState)', () => {
    expect(journeyCanAddLeg(journey([leg({ trackedTrainState: null })]))).toBe(false);
  });

  it('is true once the last leg is matched', () => {
    expect(journeyCanAddLeg(journey([leg({ trackedTrainState: trackedState() })]))).toBe(true);
  });

  it('checks the LAST leg, not an earlier one, in a multi-leg journey', () => {
    const j = journey([
      leg({ id: 1, trackedTrainState: trackedState() }),
      leg({ id: 2, trackedTrainState: null }),
    ]);
    expect(journeyCanAddLeg(j)).toBe(false);
  });
});
