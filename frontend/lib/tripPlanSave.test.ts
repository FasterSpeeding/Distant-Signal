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
  it('saves a cross-London Tube transfer between two trains as an untracked leg (working times, no public ones)', () => {
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

  it('falls back to working times: back from the first train, and after midnight by its day offset', () => {
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

  it('times transfers from public times when the legs have them, as the planner plans', () => {
    const withPublic = (leg: TripPlanLeg, publicTimes: Record<string, string | number>): TripPlanLeg => ({
      ...leg,
      ...publicTimes,
    });
    const legs = itineraryLegsForSaving(
      [
        itinerary([
          // A walk before the first train: ends at its PUBLIC departure
          // 08:31 (BST, so 07:31Z), not the working 08:30.
          transfer('WALK', 'KGX', 'EUS', 10),
          withPublic(train('T1', 'EUS', 'MKC', '08:30:00', '09:20:30'), {
            publicDeparture: '08:31:00',
            publicDepartureDayOffset: 0,
            // Public arrival 09:19 (rounded down), not the working 09:20:30.
            publicArrival: '09:19:00',
            publicArrivalDayOffset: 0,
          }),
          transfer('TUBE', 'MKC', 'KGX', 20),
          // Public arrival just after midnight: its own day offset counts.
          withPublic(train('T2', 'KGX', 'YRK', '23:30:00', '23:59:30'), {
            publicDeparture: '23:30:00',
            publicDepartureDayOffset: 0,
            publicArrival: '00:01:00',
            publicArrivalDayOffset: 1,
          }),
          transfer('BUS', 'YRK', 'XYZ', 5),
        ]),
      ],
      nameFor,
    );
    expect(legs[0]).toMatchObject({
      transitMode: 'walk',
      plannedDeparture: '2026-10-21T07:21:00.000Z',
      plannedArrival: '2026-10-21T07:31:00.000Z',
    });
    expect(legs[2]).toMatchObject({
      transitMode: 'tube',
      plannedDeparture: '2026-10-21T08:19:00.000Z',
      plannedArrival: '2026-10-21T08:39:00.000Z',
    });
    expect(legs[4]).toMatchObject({
      transitMode: 'bus',
      plannedDeparture: '2026-10-21T23:01:00.000Z',
      plannedArrival: '2026-10-21T23:06:00.000Z',
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

  it("takes a Tube transfer's TfL stops, names and ids, where the plan has them", () => {
    const euston = { id: '940GZZTESTE', name: 'Euston', modes: ['tube'], lines: [] };
    const kingsCross = { id: '940GZZTESTK', name: "King's Cross St. Pancras", modes: ['tube'], lines: [] };
    const tube: TripPlanLeg = {
      kind: 'transfer',
      mode: 'TUBE',
      originCrs: 'EUS',
      destinationCrs: 'KGX',
      minutes: 12,
      originStop: euston,
      destinationStop: kingsCross,
    };
    expect(transferAsUntracked(tube, nameFor)).toEqual({
      transitMode: 'tube',
      originName: 'Euston',
      originStopId: '940GZZTESTE',
      destinationName: "King's Cross St. Pancras",
      destinationStopId: '940GZZTESTK',
    });
    // One end unknown: that end keeps the station name and sends no id.
    const { destinationStop: _gone, ...half } = tube;
    expect(transferAsUntracked(half, nameFor)).toEqual({
      transitMode: 'tube',
      originName: 'Euston',
      originStopId: '940GZZTESTE',
      destinationName: 'London Kings Cross',
    });
    // And the saved itinerary carries them on the untracked leg.
    const legs = itineraryLegsForSaving(
      [itinerary([train('A1', 'MKC', 'EUS', '08:00', '08:40'), tube, train('B2', 'KGX', 'YRK', '09:30', '11:30')])],
      nameFor,
    );
    expect(legs[1]).toMatchObject({
      kind: 'untracked',
      transitMode: 'tube',
      originStopId: '940GZZTESTE',
      destinationStopId: '940GZZTESTK',
    });
  });

  it("carries a transfer's line as the untracked leg's line name", () => {
    const tube: TripPlanLeg = {
      kind: 'transfer',
      mode: 'TUBE',
      originCrs: 'VXH',
      destinationCrs: 'VIC',
      minutes: 11,
      line: 'Victoria',
    };
    expect(transferAsUntracked(tube, nameFor)).toMatchObject({ transitMode: 'tube', lineName: 'Victoria line' });
    const { line: _none, ...unnamed } = tube;
    expect(transferAsUntracked(unnamed, nameFor)).not.toHaveProperty('lineName');
  });
});
