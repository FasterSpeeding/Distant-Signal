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

/** A Tube ride on the TfL network (the TfL transit design §11): its
 * stations in TfL's stop-area spelling. Invented times. */
export const TUBE_LEG: TripPlanTransitLeg = {
  ...TRAM_LEG,
  network: 'tfl',
  serviceMode: 'tube',
  line: { id: 'tfl:victoria', name: 'Victoria', colour: '#1f97d4' },
  headsign: null,
  originStop: { id: '940GZZLUKSX', name: "King's Cross St. Pancras", platform: null },
  destinationStop: { id: '940GZZLUBXN', name: 'Brixton', platform: null },
};

/** A walk from a National Rail station to its TfL station. */
export const WALK_TO_TUBE: TransferLeg = {
  ...WALK_TO_TRAM,
  originCrs: 'KGX',
  transitWalk: {
    network: 'tfl',
    stop: { id: '940GZZLUKSX', name: "King's Cross St. Pancras" },
    towardsStop: true,
    walkMinutes: 4,
    estimated: true,
  },
};

/** Wimbledon to Milton Keynes Central by the District line: production's
 * `GET /Trips/plan?origin=WIM&destination=MKC&results=options&transit=true`
 * for 2026-11-02, its second option, trimmed. A walk into the TfL station,
 * a timetabled District ride, a walk out to West Brompton, then two trains.
 * Each walk names the TfL stop at its Tube end. */
export const WIM_MKC_BY_DISTRICT: TripPlanItinerary = {
  legs: [
    {
      kind: 'transfer',
      mode: 'WALK',
      originCrs: 'WIM',
      destinationCrs: null,
      minutes: 4,
      transitWalk: {
        network: 'tfl',
        stop: { id: '940GZZLUWIM', name: 'Wimbledon' },
        towardsStop: true,
        walkMinutes: 2,
        estimated: true,
      },
    },
    {
      ...TUBE_LEG,
      serviceDate: '2026-11-02',
      scheduledDeparture: '09:09:00',
      scheduledArrival: '09:25:00',
      line: { id: 'tfl:district', name: 'District', colour: '#13843f' },
      originStop: { id: '940GZZLUWIM', name: 'Wimbledon', platform: null },
      destinationStop: { id: '940GZZLUWBN', name: 'West Brompton', platform: null },
      headwayMinutes: 4,
    },
    {
      kind: 'transfer',
      mode: 'WALK',
      originCrs: null,
      destinationCrs: 'WBP',
      minutes: 4,
      transitWalk: {
        network: 'tfl',
        stop: { id: '940GZZLUWBN', name: 'West Brompton' },
        towardsStop: false,
        walkMinutes: 2,
        estimated: true,
      },
    },
    {
      kind: 'train',
      trainUid: 'W29235',
      serviceDate: '2026-11-02',
      originCrs: 'WBP',
      destinationCrs: 'WFJ',
      scheduledDeparture: '09:47:00',
      scheduledArrival: '10:24:00',
      publicDeparture: '09:47:00',
      publicArrival: '10:24:00',
      arrivalDayOffset: 0,
    },
    {
      kind: 'train',
      trainUid: 'W33964',
      serviceDate: '2026-11-02',
      originCrs: 'WFJ',
      destinationCrs: 'MKC',
      scheduledDeparture: '10:30:00',
      scheduledArrival: '10:50:00',
      publicDeparture: '10:30:00',
      publicArrival: '10:50:00',
      arrivalDayOffset: 0,
    },
  ],
  changeCount: 2,
  totalDurationMinutes: 110,
  scheduledOnly: false,
};
