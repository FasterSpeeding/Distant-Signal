import type { TripPlanItinerary, TripPlanLeg, TripPlanTransitLeg } from '@/lib/types';

/** `GET /Trips/plan` legs for a rail, tram, rail route across Manchester
 * (transit trip planning §11.1), shared by the planner's tests. */

type TrainLeg = Extract<TripPlanLeg, { kind: 'train' }>;
type TransferLeg = Extract<TripPlanLeg, { kind: 'transfer' }>;

export const TRAIN_INTO_MAN: TrainLeg = {
  kind: 'train',
  trainUid: 'C10001',
  serviceDate: '2026-11-04',
  originCrs: 'SPT',
  destinationCrs: 'MAN',
  scheduledDeparture: '07:45:00',
  scheduledArrival: '08:00:00',
  publicDeparture: '07:45:00',
  publicArrival: '08:00:00',
  arrivalDayOffset: 0,
};

export const WALK_TO_TRAM: TransferLeg = {
  kind: 'transfer',
  mode: 'WALK',
  originCrs: 'MAN',
  destinationCrs: null,
  minutes: 6,
  transitWalk: {
    network: 'metrolink',
    stop: { id: '9400ZZMAPIC', name: 'Piccadilly' },
    towardsStop: true,
    walkMinutes: 4,
    estimated: true,
  },
};

export const TRAM_LEG: TripPlanTransitLeg = {
  kind: 'transit',
  network: 'metrolink',
  serviceMode: 'tram',
  liveTracking: false,
  realtime: false,
  serviceDate: '2026-11-04',
  scheduledDeparture: '08:14:00',
  scheduledArrival: '08:22:00',
  departureDayOffset: 0,
  arrivalDayOffset: 0,
  line: { id: 'metrolink:blue', name: 'Blue Line', colour: '#1f6fb5' },
  headsign: 'Bury',
  originCrs: null,
  destinationCrs: null,
  originStop: { id: '9400ZZMAPIC', name: 'Piccadilly', platform: 'To Bury' },
  destinationStop: { id: '9400ZZMAVIC', name: 'Victoria', platform: null },
  headwayMinutes: 6,
  notices: [],
};

export const REPLACEMENT_BUS_LEG: TripPlanTransitLeg = {
  ...TRAM_LEG,
  serviceMode: 'replacementBus',
  line: null,
  headsign: 'Victoria',
  scheduledDeparture: '08:15:00',
  scheduledArrival: '08:31:00',
};

export const WALK_FROM_TRAM: TransferLeg = {
  kind: 'transfer',
  mode: 'WALK',
  originCrs: null,
  destinationCrs: 'MCV',
  minutes: 5,
  transitWalk: {
    network: 'metrolink',
    stop: { id: '9400ZZMAVIC', name: 'Victoria' },
    towardsStop: false,
    walkMinutes: 3,
    estimated: true,
  },
};

export const TRAIN_FROM_MCV: TrainLeg = {
  kind: 'train',
  trainUid: 'C10002',
  serviceDate: '2026-11-04',
  originCrs: 'MCV',
  destinationCrs: 'LDS',
  scheduledDeparture: '08:40:00',
  scheduledArrival: '09:30:00',
  publicDeparture: '08:40:00',
  publicArrival: '09:30:00',
  arrivalDayOffset: 0,
};

export const RAIL_TRAM_RAIL: TripPlanItinerary = {
  legs: [TRAIN_INTO_MAN, WALK_TO_TRAM, TRAM_LEG, WALK_FROM_TRAM, TRAIN_FROM_MCV],
  changeCount: 2,
  totalDurationMinutes: 105,
  scheduledOnly: true,
};

export const TRAM_ONLY: TripPlanItinerary = {
  legs: [TRAM_LEG],
  changeCount: 0,
  totalDurationMinutes: 8,
  scheduledOnly: true,
};
