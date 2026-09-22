import { describe, it, expect } from 'vitest';
import { trackAgainPrefill, trackAgainHref } from './trackAgainPrefill';
import type { JourneyDetail, JourneyLegDetail, TrackedTrainState } from './types';

function trackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P9E010',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EDB',
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: trackedState(),
    legSkip: null,
    ...overrides,
  };
}

function journey(legs: JourneyLegDetail[]): JourneyDetail {
  return { id: 1, customName: null, createdAt: '2026-09-22T00:00:00Z', legs, isOwner: true };
}

describe('trackAgainPrefill', () => {
  it('reads pick mode for a leg with no window bounds', () => {
    const result = trackAgainPrefill(journey([leg()]));
    expect(result).toEqual({
      mode: 'pick',
      origin: 'KGX',
      destination: 'YRK',
      departAfter: null,
      departBefore: null,
      arriveAfter: null,
      arriveBefore: null,
    });
  });

  it('reads window mode when any window bound is set, even on an unmatched leg', () => {
    const result = trackAgainPrefill(
      journey([
        leg({
          departAfter: '08:00:00',
          departBefore: '10:00:00',
          arriveAfter: null,
          arriveBefore: null,
          matchMode: 'unmatched',
          trackedTrainState: null,
        }),
      ]),
    );
    expect(result).toEqual({
      mode: 'window',
      origin: 'KGX',
      destination: 'YRK',
      departAfter: '08:00',
      departBefore: '10:00',
      arriveAfter: null,
      arriveBefore: null,
    });
  });

  it('reads window mode for a MATCHED window leg too (match_mode does not decide this)', () => {
    // A window-mode leg keeps its window fields set forever, even after
    // being matched -- matchMode alone can't distinguish this from a
    // pin/knownTrain leg. See Judgment Call 3.
    const result = trackAgainPrefill(
      journey([leg({ arriveAfter: '17:00:00', arriveBefore: '19:00:00' })]),
    );
    expect(result?.mode).toBe('window');
    expect(result?.arriveAfter).toBe('17:00');
    expect(result?.arriveBefore).toBe('19:00');
  });

  it('falls back to the matched train pin CRS when the leg row itself has no origin/destination', () => {
    const result = trackAgainPrefill(
      journey([
        leg({
          originCrs: null,
          destinationCrs: null,
          trackedTrainState: trackedState({ pinOriginCrs: 'PAD', pinDestinationCrs: 'RDG' }),
        }),
      ]),
    );
    expect(result?.origin).toBe('PAD');
    expect(result?.destination).toBe('RDG');
  });

  it('uses only the FIRST leg of a multi-leg journey', () => {
    const result = trackAgainPrefill(
      journey([leg({ originCrs: 'KGX', destinationCrs: 'YRK' }), leg({ id: 2, originCrs: 'YRK', destinationCrs: 'EDB' })]),
    );
    expect(result?.origin).toBe('KGX');
    expect(result?.destination).toBe('YRK');
  });

  it('returns null for a journey with no legs at all', () => {
    expect(trackAgainPrefill(journey([]))).toBeNull();
  });
});

describe('trackAgainHref', () => {
  it('builds a pick-mode URL with origin and destination only', () => {
    expect(trackAgainHref(journey([leg()]))).toBe('/track?mode=pick&origin=KGX&destination=YRK');
  });

  it('builds a window-mode URL with only the bounds that were actually set', () => {
    const href = trackAgainHref(
      journey([leg({ departAfter: '08:00:00', arriveBefore: null, matchMode: 'unmatched', trackedTrainState: null })]),
    );
    expect(href).toBe('/track?mode=window&origin=KGX&destination=YRK&departAfter=08%3A00');
  });

  it('returns null when there is no origin to reproduce', () => {
    const href = trackAgainHref(
      journey([leg({ originCrs: null, destinationCrs: null, trackedTrainState: null, matchMode: 'unmatched' })]),
    );
    expect(href).toBeNull();
  });
});
