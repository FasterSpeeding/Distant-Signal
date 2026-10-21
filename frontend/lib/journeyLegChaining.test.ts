import { describe, it, expect } from 'vitest';
import {
  connectionGapMinutes,
  journeyCanAddLeg,
  journeyFirstLegStart,
  journeyPriorArrival,
  journeyPriorDestinationCrs,
  journeyPriorDestinationName,
  legDepartureInstant,
  legExpectedDepartureInstant,
} from './journeyLegChaining';
import type { JourneyDetail, JourneyLegDetail, JourneyStop, TrackedTrainState } from './types';

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
    const j = journey([leg({ id: 1, trackedTrainState: trackedState() }), leg({ id: 2, trackedTrainState: null })]);
    expect(journeyCanAddLeg(j)).toBe(false);
  });
});

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

/** A matched WAT to CLJ leg on a train with the given stops. */
function matchedJourney(journeyStops: JourneyStop[] | null): JourneyDetail {
  return journey([leg({ trackedTrainState: trackedState({ journeyStops }) })]);
}

describe('journeyPriorArrival', () => {
  it("reads the public arrival at the last leg's destination, in London time", () => {
    const j = matchedJourney([
      stop('WAT', { kind: 'Origin', publicDeparture: '2026-09-22T09:00:00Z' }),
      stop('CLJ', {
        // BST: 09:07Z is 10:07 in London. The working and estimated times
        // are ignored.
        publicArrival: '2026-09-22T09:07:00Z',
        scheduledArrival: '2026-09-22T09:06:00Z',
        estimatedArrival: '2026-09-22T09:20:00Z',
      }),
      stop('WOK', { kind: 'Terminate', publicArrival: '2026-09-22T09:30:00Z' }),
    ]);
    expect(journeyPriorArrival(j)).toEqual({ date: '2026-09-22', time: '10:07' });
  });

  it('moves to the next day for an arrival after London midnight', () => {
    const j = matchedJourney([
      stop('WAT', { kind: 'Origin', publicDeparture: '2026-09-22T22:40:00Z' }),
      // 23:15Z on 22 September is 00:15 on 23 September in London (BST).
      stop('CLJ', { kind: 'Terminate', publicArrival: '2026-09-22T23:15:00Z' }),
    ]);
    expect(journeyPriorArrival(j)).toEqual({ date: '2026-09-23', time: '00:15' });
  });

  it('uses the working arrival only when the API sent no public times', () => {
    // GMT in December: London time is UTC.
    const j = matchedJourney([stop('WAT'), stop('CLJ', { scheduledArrival: '2026-12-01T18:04:00Z' })]);
    expect(journeyPriorArrival(j)).toEqual({ date: '2026-12-01', time: '18:04' });
  });

  it('picks the call after the origin when the route visits the destination twice', () => {
    const j = journey([
      leg({
        originCrs: 'WAT',
        destinationCrs: 'CLJ',
        trackedTrainState: trackedState({
          journeyStops: [
            stop('CLJ', { kind: 'Origin', publicDeparture: '2026-09-22T08:00:00Z' }),
            stop('WAT', { publicArrival: '2026-09-22T08:10:00Z', publicDeparture: '2026-09-22T08:15:00Z' }),
            stop('CLJ', { kind: 'Terminate', publicArrival: '2026-09-22T08:25:00Z' }),
          ],
        }),
      }),
    ]);
    expect(journeyPriorArrival(j)).toEqual({ date: '2026-09-22', time: '09:25' });
  });

  it('is null for a journey with no legs', () => {
    expect(journeyPriorArrival(journey([]))).toBeNull();
  });

  it('is null for an unmatched last leg', () => {
    expect(journeyPriorArrival(journey([leg({ trackedTrainState: null })]))).toBeNull();
  });

  it('is null when the train has no stop list', () => {
    expect(journeyPriorArrival(matchedJourney(null))).toBeNull();
  });

  it('is null when the destination has no known arrival', () => {
    expect(journeyPriorArrival(matchedJourney([stop('WAT'), stop('CLJ')]))).toBeNull();
  });

  it('is null when no stop is the destination', () => {
    const j = matchedJourney([stop('WAT'), stop('WOK', { publicArrival: '2026-09-22T09:30:00Z' })]);
    expect(journeyPriorArrival(j)).toBeNull();
  });
});

/** Spec 2026-10-20 §2.3: untracked legs chain like train legs, from their
 * planned times. */
describe('chaining across untracked legs', () => {
  const tube = leg({
    id: 2,
    kind: 'untracked',
    originCrs: null,
    destinationCrs: null,
    originName: 'Brixton',
    destinationName: 'Euston',
    matchMode: 'manual',
    transitMode: 'tube',
    // 23:35 to 23:58 UTC on 22 Sep is 00:35 to 00:58 BST on the 23rd.
    plannedDeparture: '2026-09-22T23:35:00Z',
    plannedArrival: '2026-09-22T23:58:00Z',
    untrackedStatus: 'planned',
  });

  it('a next leg can follow an untracked leg, starting from its planned arrival and place', () => {
    const j = journey([tube]);
    expect(journeyCanAddLeg(j)).toBe(true);
    expect(journeyPriorArrival(j)).toEqual({ date: '2026-09-23', time: '00:58' });
    expect(journeyPriorDestinationCrs(j)).toBeNull();
    expect(journeyPriorDestinationName(j)).toBe('Euston');
  });

  it('an open train leg still cannot be chained onto', () => {
    expect(journeyCanAddLeg(journey([tube, leg({ id: 3, trackedTrainState: null })]))).toBe(false);
  });

  it('works out the gap between a train arrival and a Tube departure, and back', () => {
    const train = leg({
      id: 1,
      originCrs: 'WAT',
      destinationCrs: 'CLJ',
      trackedTrainState: trackedState({
        journeyStops: [
          stop('WAT', { publicDeparture: '2026-09-22T23:00:00Z' }),
          stop('CLJ', { publicArrival: '2026-09-22T23:20:00Z' }),
        ],
      }),
    });
    expect(legDepartureInstant(train)).toBe('2026-09-22T23:00:00Z');
    expect(legDepartureInstant(tube)).toBe('2026-09-22T23:35:00Z');
    expect(connectionGapMinutes(train, tube)).toBe(15);
    expect(connectionGapMinutes(tube, train)).toBe(-58);
    expect(connectionGapMinutes(tube, leg({ id: 4, trackedTrainState: null }))).toBeNull();
  });
});

describe('journeyFirstLegStart', () => {
  it('names where and when the first leg starts, for a leg inserted before it', () => {
    const train = leg({
      id: 4,
      originCrs: 'WAT',
      originName: 'London Waterloo',
      trackedTrainState: trackedState({ journeyStops: [stop('WAT', { publicDeparture: '2026-09-22T08:00:00Z' })] }),
    });
    expect(journeyFirstLegStart(journey([train]))).toEqual({
      legId: 4,
      place: 'London Waterloo',
      crs: 'WAT',
      departure: { date: '2026-09-22', time: '09:00' },
    });
    expect(journeyFirstLegStart(journey([]))).toBeNull();
  });
});

describe('legExpectedDepartureInstant', () => {
  it("prefers the origin's reported departure, then its estimate, then the timetable", () => {
    const withStop = (overrides: Partial<JourneyStop>) =>
      leg({
        trackedTrainState: trackedState({
          journeyStops: [stop('WAT', { publicDeparture: '2026-09-22T08:00:00Z', ...overrides })],
        }),
      });
    expect(legExpectedDepartureInstant(withStop({}))).toBe('2026-09-22T08:00:00Z');
    expect(legExpectedDepartureInstant(withStop({ estimatedDeparture: '2026-09-22T08:07:00Z' }))).toBe(
      '2026-09-22T08:07:00Z',
    );
    expect(
      legExpectedDepartureInstant(
        withStop({ estimatedDeparture: '2026-09-22T08:07:00Z', actualDeparture: '2026-09-22T08:09:00Z' }),
      ),
    ).toBe('2026-09-22T08:09:00Z');
  });

  it("is an untracked leg's planned departure, and null with no stop list", () => {
    expect(legExpectedDepartureInstant(leg({ kind: 'untracked', plannedDeparture: '2026-09-22T07:40:00Z' }))).toBe(
      '2026-09-22T07:40:00Z',
    );
    expect(legExpectedDepartureInstant(leg({ trackedTrainState: trackedState({ journeyStops: null }) }))).toBeNull();
  });
});
