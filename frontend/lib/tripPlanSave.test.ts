import { describe, expect, it } from 'vitest';
import { itineraryLegsForSaving, transferAsUntracked } from './tripPlanSave';
import type { TripPlanItinerary, TripPlanLeg } from './types';

const NAMES = new Map([
  ['EUS', 'London Euston'],
  ['KGX', 'London Kings Cross'],
]);
const nameFor = (code: string) => NAMES.get(code);

const train = (
  trainUid: string,
  originCrs: string,
  destinationCrs: string,
  departure: string,
  arrival: string,
  arrivalDayOffset = 0,
): TripPlanLeg => ({
  kind: 'train',
  trainUid,
  serviceDate: '2026-10-21',
  originCrs,
  destinationCrs,
  scheduledDeparture: departure,
  scheduledArrival: arrival,
  arrivalDayOffset,
});
const transfer = (
  mode: string,
  originCrs: string | null,
  destinationCrs: string | null,
  minutes: number,
): TripPlanLeg => ({
  kind: 'transfer',
  mode,
  originCrs,
  destinationCrs,
  minutes,
});
const itinerary = (legs: TripPlanLeg[], continuesPreviousTrain = false): TripPlanItinerary => ({
  legs,
  changeCount: 0,
  totalDurationMinutes: 0,
  continuesPreviousTrain,
});

/** Spec 2026-10-20 §2, follow-up: the planner's save keeps its transfers
 * as untracked legs instead of dropping them. */
describe('itineraryLegsForSaving', () => {
  it('saves a cross-London Tube transfer between two trains as an untracked leg', () => {
    const legs = itineraryLegsForSaving(
      [
        itinerary([
          // Arrives Euston 09:50 BST (08:50Z), Tube 20 min, train leaves KGX.
          train('T1', 'MKC', 'EUS', '09:00:00', '09:50:00'),
          transfer('TUBE', 'EUS', 'KGX', 20),
          train('T2', 'KGX', 'YRK', '10:30:00', '12:30:00'),
        ]),
      ],
      nameFor,
    );
    expect(legs).toEqual([
      { trainUid: 'T1', serviceDate: '2026-10-21', originCrs: 'MKC', destinationCrs: 'EUS' },
      {
        kind: 'untracked',
        transitMode: 'tube',
        originName: 'London Euston',
        destinationName: 'London Kings Cross',
        plannedDeparture: '2026-10-21T08:50:00.000Z',
        plannedArrival: '2026-10-21T09:10:00.000Z',
      },
      { trainUid: 'T2', serviceDate: '2026-10-21', originCrs: 'KGX', destinationCrs: 'YRK' },
    ]);
  });

  it('times a transfer before the first train back from that train, and after midnight from its day offset', () => {
    const legs = itineraryLegsForSaving(
      [
        itinerary([
          transfer('WALK', 'EUS', 'KGX', 15),
          train('T1', 'KGX', 'PBO', '23:30:00', '00:20:00', 1),
          transfer('BUS', 'PBO', 'XYZ', 10),
        ]),
      ],
      nameFor,
    );
    expect(legs[0]).toMatchObject({
      transitMode: 'walk',
      plannedDeparture: '2026-10-21T22:15:00.000Z',
      plannedArrival: '2026-10-21T22:30:00.000Z',
    });
    // The bus starts when the train arrives at 00:20 on the 22nd (BST).
    expect(legs[2]).toMatchObject({
      transitMode: 'bus',
      originName: 'PBO',
      destinationName: 'XYZ',
      plannedDeparture: '2026-10-21T23:20:00.000Z',
      plannedArrival: '2026-10-21T23:30:00.000Z',
    });
  });

  it('keeps one leg for a train ridden through a waypoint, and leaves out a change within a station', () => {
    const legs = itineraryLegsForSaving(
      [
        itinerary([train('T1', 'EUS', 'MKC', '08:00:00', '08:40:00')]),
        itinerary([train('T1', 'MKC', 'MAN', '08:45:00', '10:00:00'), transfer('TRANSFER', 'MAN', 'MAN', 5)], true),
      ],
      nameFor,
    );
    expect(legs).toEqual([{ trainUid: 'T1', serviceDate: '2026-10-21', originCrs: 'EUS', destinationCrs: 'MAN' }]);
  });
});

describe('transferAsUntracked', () => {
  it('maps each fixed-link mode and drops a TIPLOC end', () => {
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'METRO', originCrs: 'tiploc:NCLX', destinationCrs: 'KGX', minutes: 5 },
        nameFor,
      ),
    ).toEqual({
      transitMode: 'metro',
      destinationName: 'London Kings Cross',
    });
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'FERRY', originCrs: null, destinationCrs: null, minutes: 5 },
        nameFor,
      ),
    ).toEqual({
      transitMode: 'ferry',
    });
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'TRANSFER', originCrs: 'EUS', destinationCrs: 'EUS', minutes: 5 },
        nameFor,
      ),
    ).toBeNull();
  });
});
