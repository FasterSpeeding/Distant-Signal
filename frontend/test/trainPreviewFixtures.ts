import type { JourneyStop, PublicTrainState } from '@/lib/types';

/** Made-up trains for the train preview card (`lib/trainPreview.ts`): the
 * stations are real, the UIDs, headcodes, times and reasons invented.
 * `TRAIN_PREVIEW_NOW` is 14:00 UK time on Friday 30 October 2026, the day
 * every live one runs. */
export const TRAIN_PREVIEW_NOW = Date.parse('2026-10-30T14:00:00Z');

export function previewStop(name: string, crs: string, overrides: Partial<JourneyStop> = {}): JourneyStop {
  return {
    crs,
    name,
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

export function previewTrain(overrides: Partial<PublicTrainState> = {}): PublicTrainState {
  return {
    trainsId: 1,
    trainUid: 'Z90001',
    serviceDate: '2026-10-30',
    originCrs: 'WOK',
    originName: 'Woking',
    destinationCrs: 'WAT',
    destinationName: 'London Waterloo',
    scheduledDeparture: '2026-10-30T13:42:00Z',
    callingPoints: null,
    trainId: null,
    headcode: '2Z42',
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    journeyStops: null,
    mayHaveArrived: false,
    operatorCode: 'SW',
    operatorName: 'South Western Railway',
    ...overrides,
  };
}

/** On time, between Surbiton and Wimbledon. */
export const ON_TIME_TRAIN = previewTrain({
  trainId: '402Z42MO30',
  lastReportedLocation: 'SUR',
  lastEventType: 'DEPARTURE',
  journeyStops: [
    previewStop('Woking', 'WOK', {
      kind: 'Origin',
      scheduledDeparture: '2026-10-30T13:42:00Z',
      actualDeparture: '2026-10-30T13:42:00Z',
      platform: '2',
      displayPlatform: '2',
    }),
    previewStop('Surbiton', 'SUR', {
      scheduledArrival: '2026-10-30T13:55:00Z',
      scheduledDeparture: '2026-10-30T13:56:00Z',
      actualArrival: '2026-10-30T13:55:00Z',
      actualDeparture: '2026-10-30T13:56:00Z',
    }),
    previewStop('Wimbledon', 'WIM', {
      scheduledArrival: '2026-10-30T14:03:00Z',
      estimatedArrival: '2026-10-30T14:03:00Z',
    }),
    previewStop('London Waterloo', 'WAT', { kind: 'Terminate', scheduledArrival: '2026-10-30T14:16:00Z' }),
  ],
});

/** 12 minutes late, past Haywards Heath. */
export const DELAYED_TRAIN = previewTrain({
  trainUid: 'Z90002',
  originCrs: 'BTN',
  originName: 'Brighton',
  destinationCrs: 'VIC',
  destinationName: 'London Victoria',
  scheduledDeparture: '2026-10-30T13:05:00Z',
  headcode: '1Z05',
  trainId: '871Z05MO30',
  delayMinutes: 12,
  operatorCode: 'SN',
  operatorName: 'Southern',
  lastReportedLocation: 'HHE',
  lastEventType: 'DEPARTURE',
  journeyStops: [
    previewStop('Brighton', 'BTN', {
      kind: 'Origin',
      scheduledDeparture: '2026-10-30T13:05:00Z',
      actualDeparture: '2026-10-30T13:09:00Z',
    }),
    previewStop('Haywards Heath', 'HHE', {
      scheduledArrival: '2026-10-30T13:34:00Z',
      scheduledDeparture: '2026-10-30T13:35:00Z',
      actualArrival: '2026-10-30T13:46:00Z',
      actualDeparture: '2026-10-30T13:47:00Z',
    }),
    previewStop('East Croydon', 'ECR', {
      scheduledArrival: '2026-10-30T13:56:00Z',
      estimatedArrival: '2026-10-30T14:08:00Z',
    }),
    previewStop('London Victoria', 'VIC', {
      kind: 'Terminate',
      scheduledArrival: '2026-10-30T14:12:00Z',
      estimatedArrival: '2026-10-30T14:24:00Z',
    }),
  ],
});

/** Cancelled outright, with a reason. */
export const CANCELLED_TRAIN = previewTrain({
  trainUid: 'Z90003',
  originCrs: 'LWS',
  originName: 'Lewes',
  destinationCrs: 'SEF',
  destinationName: 'Seaford',
  scheduledDeparture: '2026-10-30T14:20:00Z',
  headcode: '2Z20',
  status: 'cancelled',
  delayMinutes: null,
  operatorCode: 'SN',
  operatorName: 'Southern',
  cancelled: true,
  cancellation: {
    kind: 'full',
    startsFrom: null,
    terminatesAt: null,
    from: null,
    to: null,
    sections: [],
    reason: { code: 'IB', text: 'Points failure', source: 'trust' },
    at: '2026-10-30T13:31:00Z',
  },
  journeyStops: [
    previewStop('Lewes', 'LWS', { kind: 'Origin', scheduledDeparture: '2026-10-30T14:20:00Z' }),
    previewStop('Newhaven Town', 'NVN', { scheduledArrival: '2026-10-30T14:29:00Z' }),
    previewStop('Seaford', 'SEF', { kind: 'Terminate', scheduledArrival: '2026-10-30T14:36:00Z' }),
  ],
});

/** Tomorrow's train, not yet running: its timetable and booked platform. */
export const PLANNED_TRAIN = previewTrain({
  trainUid: 'Z90004',
  serviceDate: '2026-10-31',
  originCrs: 'RDG',
  originName: 'Reading',
  destinationCrs: 'PAD',
  destinationName: 'London Paddington',
  scheduledDeparture: '2026-10-31T07:15:00Z',
  headcode: '1Z15',
  status: null,
  delayMinutes: null,
  operatorCode: 'GW',
  operatorName: 'Great Western Railway',
  trackingPhase: 'planned',
  journeyStops: [
    previewStop('Reading', 'RDG', {
      kind: 'Origin',
      scheduledDeparture: '2026-10-31T07:15:00Z',
      bookedPlatform: '4',
      displayPlatform: '4',
      platformBasis: 'booked',
    }),
    previewStop('London Paddington', 'PAD', { kind: 'Terminate', scheduledArrival: '2026-10-31T07:42:00Z' }),
  ],
});
