import { describe, expect, it } from 'vitest';
import { itineraryLegsForSaving, transferAsUntracked } from './tripPlanSave';
import { trainLegsForTracking } from './tripPlan';
import type { ItineraryLegBody } from './tripPlanSave';
import type { TripPlanItinerary, TripPlanLeg } from './types';
import {
  RAIL_TRAM_RAIL,
  REPLACEMENT_BUS_LEG,
  TRAM_LEG,
  TRAM_ONLY,
  TUBE_LEG,
  WALK_FROM_TRAM,
  WALK_TO_TRAM,
  WALK_TO_TUBE,
  WIM_MKC_BY_DISTRICT,
} from '@/test/transitPlanFixtures';

const NAMES = new Map([
  ['EUS', 'London Euston'],
  ['KGX', 'London Kings Cross'],
  ['VIC', 'London Victoria'],
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
  it('saves the access and egress legs of a TfL stop From and To with their stops (DS-135)', () => {
    const uxbridge = { id: '940GZZLUUXB', name: 'Uxbridge', modes: ['tube'], lines: [] };
    const harrow = { id: '940GZZLUHOH', name: 'Harrow-on-the-Hill', modes: ['tube'], lines: [] };
    const legs = itineraryLegsForSaving(
      [
        itinerary([
          {
            kind: 'transfer',
            mode: 'TUBE',
            originCrs: null,
            destinationCrs: 'HOH',
            minutes: 30,
            line: 'Metropolitan',
            originStop: uxbridge,
            destinationStop: harrow,
            access: { end: 'origin', basis: 'line', distanceMetres: 11_000 },
          },
          // Leaves Harrow 09:00 BST (08:00Z).
          train('T1', 'HOH', 'EUS', '09:00:00', '09:30:00'),
          {
            kind: 'transfer',
            mode: 'WALK',
            originCrs: 'EUS',
            destinationCrs: null,
            minutes: 5,
            destinationStop: { id: '940GZZLUEUS', name: 'Euston', modes: ['tube'], lines: [] },
            access: { end: 'destination', basis: 'hub', distanceMetres: 0 },
          },
        ]),
      ],
      nameFor,
    );
    expect(legs).toEqual([
      {
        kind: 'untracked',
        transitMode: 'tube',
        originName: 'Uxbridge',
        destinationName: 'Harrow-on-the-Hill',
        lineName: 'Metropolitan line',
        originStopId: '940GZZLUUXB',
        destinationStopId: '940GZZLUHOH',
        plannedDeparture: '2026-10-21T07:30:00.000Z',
        plannedArrival: '2026-10-21T08:00:00.000Z',
      },
      { trainUid: 'T1', serviceDate: '2026-10-21', originCrs: 'HOH', destinationCrs: 'EUS' },
      {
        kind: 'untracked',
        transitMode: 'walk',
        originName: 'London Euston',
        destinationName: 'Euston',
        // The walk to the stop is named after it; only a ride can have its id.
        plannedDeparture: '2026-10-21T08:30:00.000Z',
        plannedArrival: '2026-10-21T08:35:00.000Z',
      },
    ]);
  });

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

  it('saves a merged Tube ride as one leg, and times transfers in a row one after another', () => {
    const victoria = (originCrs: string, destinationCrs: string, minutes: number, line: string): TripPlanLeg => ({
      kind: 'transfer',
      mode: 'TUBE',
      originCrs,
      destinationCrs,
      minutes,
      line,
      originStop: { id: `940GZZLU${originCrs}`, name: originCrs, modes: ['tube'], lines: [] },
      destinationStop: { id: `940GZZLU${destinationCrs}`, name: destinationCrs, modes: ['tube'], lines: [] },
    });
    // Euston to Vauxhall, one Victoria line ride the planner served as one
    // leg (DS-139): one untracked leg with both stops.
    const merged = itineraryLegsForSaving(
      [itinerary([train('T1', 'MKC', 'EUS', '09:00:00', '09:50:00'), victoria('EUS', 'VXH', 22, 'Victoria')])],
      nameFor,
    );
    expect(merged[1]).toEqual({
      kind: 'untracked',
      transitMode: 'tube',
      originName: 'EUS',
      destinationName: 'VXH',
      lineName: 'Victoria line',
      originStopId: '940GZZLUEUS',
      destinationStopId: '940GZZLUVXH',
      plannedDeparture: '2026-10-21T08:50:00.000Z',
      plannedArrival: '2026-10-21T09:12:00.000Z',
    });
    // The Northern line to Euston, then the Victoria line: two legs, the
    // second starting when the first ends.
    const twoLines = itineraryLegsForSaving(
      [
        itinerary([
          train('T1', 'BTN', 'CHX', '09:00:00', '09:50:00'),
          victoria('CHX', 'EUS', 18, 'Northern'),
          victoria('EUS', 'VXH', 22, 'Victoria'),
        ]),
      ],
      nameFor,
    );
    expect(twoLines.slice(1)).toMatchObject([
      {
        lineName: 'Northern line',
        plannedDeparture: '2026-10-21T08:50:00.000Z',
        plannedArrival: '2026-10-21T09:08:00.000Z',
      },
      {
        lineName: 'Victoria line',
        plannedDeparture: '2026-10-21T09:08:00.000Z',
        plannedArrival: '2026-10-21T09:30:00.000Z',
      },
    ]);
    // With no train before them, they end when the next train leaves.
    const beforeTrain = itineraryLegsForSaving(
      [
        itinerary([
          victoria('CHX', 'EUS', 18, 'Northern'),
          victoria('EUS', 'VXH', 22, 'Victoria'),
          train('T2', 'VXH', 'WOK', '10:00:00', '10:30:00'),
        ]),
      ],
      nameFor,
    );
    expect(beforeTrain.slice(0, 2)).toMatchObject([
      { plannedDeparture: '2026-10-21T08:20:00.000Z', plannedArrival: '2026-10-21T08:38:00.000Z' },
      { plannedDeparture: '2026-10-21T08:38:00.000Z', plannedArrival: '2026-10-21T09:00:00.000Z' },
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

  it('saves a TRANSFER link between two different stations as an untracked transfer leg on its times', () => {
    // The timetable's TRANSFER links are often real hops between stations:
    // Euston to Victoria is a 19-minute one. Arrives Euston 09:50 BST.
    const legs = itineraryLegsForSaving(
      [
        itinerary([
          train('T1', 'MKC', 'EUS', '09:00:00', '09:50:00'),
          transfer('TRANSFER', 'EUS', 'VIC', 19),
          train('T2', 'VIC', 'BTN', '10:30:00', '11:30:00'),
        ]),
      ],
      nameFor,
    );
    expect(legs).toEqual([
      { trainUid: 'T1', serviceDate: '2026-10-21', originCrs: 'MKC', destinationCrs: 'EUS' },
      {
        kind: 'untracked',
        transitMode: 'transfer',
        originName: 'London Euston',
        destinationName: 'London Victoria',
        plannedDeparture: '2026-10-21T08:50:00.000Z',
        plannedArrival: '2026-10-21T09:09:00.000Z',
      },
      { trainUid: 'T2', serviceDate: '2026-10-21', originCrs: 'VIC', destinationCrs: 'BTN' },
    ]);
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
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'TRANSFER', originCrs: 'eus', destinationCrs: 'EUS', minutes: 5 },
        nameFor,
      ),
    ).toBeNull();
    // A TRANSFER between two stations is a journey of its own (Euston to
    // Victoria, 19 minutes), as is one with a TIPLOC end, named at its CRS.
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'TRANSFER', originCrs: 'EUS', destinationCrs: 'VIC', minutes: 19 },
        nameFor,
      ),
    ).toEqual({ transitMode: 'transfer', originName: 'London Euston', destinationName: 'London Victoria' });
    expect(
      transferAsUntracked(
        { kind: 'transfer', mode: 'TRANSFER', originCrs: 'tiploc:STRATFD', destinationCrs: 'KGX', minutes: 12 },
        nameFor,
      ),
    ).toEqual({ transitMode: 'transfer', destinationName: 'London Kings Cross' });
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

describe('itineraryLegsForSaving with a tram leg', () => {
  const NAMES_MCR = new Map([
    ['MAN', 'Manchester Piccadilly'],
    ['MCV', 'Manchester Victoria'],
  ]);
  const mcrName = (code: string) => NAMES_MCR.get(code);

  it('saves a TfL Tube ride as an untracked Tube leg between its TfL stops', () => {
    const legs = itineraryLegsForSaving([{ ...TRAM_ONLY, legs: [TUBE_LEG] }], mcrName);
    expect(legs).toEqual([
      {
        kind: 'untracked',
        transitMode: 'tube',
        originName: "King's Cross St. Pancras",
        destinationName: 'Brixton',
        lineName: 'Victoria',
        originStopId: '940GZZLUKSX',
        destinationStopId: '940GZZLUBXN',
        plannedDeparture: '2026-11-04T08:14:00.000Z',
        plannedArrival: '2026-11-04T08:22:00.000Z',
        transitNetwork: 'tfl',
        transitLineId: 'victoria',
      },
    ]);
  });

  it('saves the tram as an untracked tram leg on its own times, its stations by id', () => {
    const legs = itineraryLegsForSaving([RAIL_TRAM_RAIL], mcrName);
    expect(legs).toEqual([
      { trainUid: 'C10001', serviceDate: '2026-11-04', originCrs: 'SPT', destinationCrs: 'MAN' },
      {
        kind: 'untracked',
        transitMode: 'walk',
        originName: 'Manchester Piccadilly',
        destinationName: 'Piccadilly',
        // A walk has no stop id (the api takes one only on a ride); the
        // stop's name is its place.
        plannedDeparture: '2026-11-04T08:00:00.000Z',
        plannedArrival: '2026-11-04T08:06:00.000Z',
      },
      {
        kind: 'untracked',
        transitMode: 'tram',
        originName: 'Piccadilly',
        destinationName: 'Victoria',
        lineName: 'Blue Line',
        originStopId: '9400ZZMAPIC',
        destinationStopId: '9400ZZMAVIC',
        // The tram's own times, not borrowed from the trains either side.
        plannedDeparture: '2026-11-04T08:14:00.000Z',
        plannedArrival: '2026-11-04T08:22:00.000Z',
        // Its network ride, to find it in the timetable again (DS-237).
        transitNetwork: 'metrolink',
        transitLineId: 'blue',
        headsign: 'Bury',
      },
      {
        kind: 'untracked',
        transitMode: 'walk',
        originName: 'Victoria',
        destinationName: 'Manchester Victoria',
        // The walk follows the tram it leaves, not the train before it.
        plannedDeparture: '2026-11-04T08:22:00.000Z',
        plannedArrival: '2026-11-04T08:27:00.000Z',
      },
      { trainUid: 'C10002', serviceDate: '2026-11-04', originCrs: 'MCV', destinationCrs: 'LDS' },
    ]);
  });

  it('saves a replacement bus as a bus, with no line', () => {
    const [leg] = itineraryLegsForSaving(
      [{ legs: [REPLACEMENT_BUS_LEG], changeCount: 0, totalDurationMinutes: 16 }],
      mcrName,
    );
    expect(leg).toEqual({
      kind: 'untracked',
      transitMode: 'bus',
      originName: 'Piccadilly',
      destinationName: 'Victoria',
      originStopId: '9400ZZMAPIC',
      destinationStopId: '9400ZZMAVIC',
      plannedDeparture: '2026-11-04T08:15:00.000Z',
      plannedArrival: '2026-11-04T08:31:00.000Z',
    });
  });

  it('saves a Tube ride with its network and line, and no headsign when it has none', () => {
    const [leg] = itineraryLegsForSaving([{ legs: [TUBE_LEG], changeCount: 0, totalDurationMinutes: 8 }], mcrName);
    expect(leg).toMatchObject({
      kind: 'untracked',
      transitMode: 'tube',
      originStopId: '940GZZLUKSX',
      destinationStopId: '940GZZLUBXN',
      transitNetwork: 'tfl',
      transitLineId: 'victoria',
    });
    expect(leg).not.toHaveProperty('headsign');
  });

  it('a tram-only plan has no train to track', () => {
    expect(trainLegsForTracking([TRAM_ONLY])).toEqual([]);
  });
});

/** `POST /Journeys/itinerary` refuses a stop id on a leg whose mode can't
 * have one (`journeys::attach_untracked_stops`: a TfL stop only on a tube,
 * dlr, tram or metro leg; a trip-planner tram station only on a tram or
 * bus leg), failing the whole save: "Only a tube, dlr, tram, metro leg can
 * have a stop id." */
describe('itineraryLegsForSaving sends stop ids only where the api takes them', () => {
  const TFL_STOP_MODES: string[] = ['tube', 'dlr', 'tram', 'metro'];
  const TRANSIT_STOP_MODES: string[] = ['tram', 'bus'];
  const accepted = (leg: ItineraryLegBody) => {
    if (!('kind' in leg)) return true;
    return [leg.originStopId, leg.destinationStopId].every(
      (id) =>
        id === undefined || (id.startsWith('9400') ? TRANSIT_STOP_MODES : TFL_STOP_MODES).includes(leg.transitMode),
    );
  };
  const wimName = (code: string) =>
    new Map([
      ['WIM', 'Wimbledon'],
      ['WBP', 'West Brompton'],
    ]).get(code);

  it('saves Wimbledon to Milton Keynes by the District line: walks by name, the Tube ride with its stops', () => {
    const legs = itineraryLegsForSaving([WIM_MKC_BY_DISTRICT], wimName);
    expect(legs).toEqual([
      {
        kind: 'untracked',
        transitMode: 'walk',
        originName: 'Wimbledon',
        destinationName: 'Wimbledon',
        plannedDeparture: '2026-11-02T09:05:00.000Z',
        plannedArrival: '2026-11-02T09:09:00.000Z',
      },
      {
        kind: 'untracked',
        transitMode: 'tube',
        originName: 'Wimbledon',
        destinationName: 'West Brompton',
        lineName: 'District',
        originStopId: '940GZZLUWIM',
        destinationStopId: '940GZZLUWBN',
        plannedDeparture: '2026-11-02T09:09:00.000Z',
        plannedArrival: '2026-11-02T09:25:00.000Z',
        transitNetwork: 'tfl',
        transitLineId: 'district',
      },
      {
        kind: 'untracked',
        transitMode: 'walk',
        originName: 'West Brompton',
        destinationName: 'West Brompton',
        plannedDeparture: '2026-11-02T09:25:00.000Z',
        plannedArrival: '2026-11-02T09:29:00.000Z',
      },
      { trainUid: 'W29235', serviceDate: '2026-11-02', originCrs: 'WBP', destinationCrs: 'WFJ' },
      { trainUid: 'W33964', serviceDate: '2026-11-02', originCrs: 'WFJ', destinationCrs: 'MKC' },
    ]);
    expect(legs.every(accepted)).toBe(true);
  });

  it('every leg type: walks to a TfL or tram stop, transfers by every mode, every transit ride', () => {
    const euston = { id: '940GZZLUEUS', name: 'Euston', modes: ['tube'], lines: [] };
    const victoria = { id: '940GZZLUVIC', name: 'Victoria', modes: ['tube'], lines: [] };
    const transfers: TripPlanLeg[] = ['WALK', 'TUBE', 'METRO', 'BUS', 'FERRY', 'TRANSFER'].map((mode) => ({
      kind: 'transfer',
      mode,
      originCrs: 'EUS',
      destinationCrs: 'VIC',
      minutes: 10,
      originStop: euston,
      destinationStop: victoria,
    }));
    const rides: TripPlanLeg[] = [
      TUBE_LEG,
      { ...TUBE_LEG, serviceMode: 'dlr' },
      { ...TUBE_LEG, serviceMode: 'tram' },
      // A replacement bus on the TfL network would carry TfL stops.
      { ...TUBE_LEG, serviceMode: 'replacementBus', line: null },
      TRAM_LEG,
      REPLACEMENT_BUS_LEG,
    ];
    const first = train('A1', 'MKC', 'EUS', '07:00', '07:40');
    const legs = itineraryLegsForSaving(
      [itinerary([first, WALK_TO_TUBE, WALK_TO_TRAM, WALK_FROM_TRAM, ...transfers, ...rides])],
      nameFor,
    );
    const untracked = legs.filter((leg) => 'kind' in leg);
    expect(untracked).toHaveLength(15);
    for (const leg of untracked) expect(accepted(leg), JSON.stringify(leg)).toBe(true);
    // The legs that can have their stops keep them.
    expect(untracked.filter((leg) => leg.originStopId !== undefined).map((leg) => leg.transitMode)).toEqual([
      'tube',
      'metro',
      'tube',
      'dlr',
      'tram',
      'tram',
      'bus',
    ]);
  });
});
