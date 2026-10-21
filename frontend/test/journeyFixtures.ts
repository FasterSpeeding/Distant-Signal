import type {
  JourneyDetail,
  JourneyLegDetail,
  JourneyListItem,
  JourneyStop,
  TrackedTrainListItem,
  TrackedTrainState,
} from '@/lib/types';

/* Journey builders for the home page's journey tests (`lib/homeJourneys`,
 * `app/(home)/HomeJourneys`, `app/(home)/page`). Defaults describe one
 * London Kings Cross to York train leg on 17 Sep 2026, departing 10:00 and
 * arriving 12:00 UK time (09:00Z and 11:00Z). */

export function stop(crs: string, overrides: Partial<JourneyStop> = {}): JourneyStop {
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

/** A train's state, with stops at `origin` (departing `departure`) and
 * `destination` (arriving `arrival`). */
export function trainState(
  origin: string,
  destination: string,
  departure: string,
  arrival: string,
  overrides: Partial<TrackedTrainState> = {},
): TrackedTrainState {
  return {
    id: 1,
    serviceDate: departure.slice(0, 10),
    pinOriginCrs: origin,
    pinDestinationCrs: destination,
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P12345',
    trainId: null,
    status: 'awaiting_activation',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: destination,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: [
      stop(origin, { kind: 'Origin', scheduledDeparture: departure }),
      stop(destination, { kind: 'Terminate', scheduledArrival: arrival }),
    ],
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

export function trainLeg(
  id: number,
  origin: [crs: string, name: string],
  destination: [crs: string, name: string],
  departure: string,
  arrival: string,
  state: Partial<TrackedTrainState> = {},
): JourneyLegDetail {
  return {
    id,
    originCrs: origin[0],
    originName: origin[1],
    destinationCrs: destination[0],
    destinationName: destination[1],
    serviceDate: departure.slice(0, 10),
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    kind: 'train',
    trackedTrainState: trainState(origin[0], destination[0], departure, arrival, state),
    legSkip: null,
  };
}

export function untrackedLeg(
  id: number,
  from: string,
  to: string,
  departure: string,
  arrival: string,
  overrides: Partial<JourneyLegDetail> = {},
): JourneyLegDetail {
  return {
    id,
    originCrs: null,
    originName: from,
    destinationCrs: null,
    destinationName: to,
    serviceDate: departure.slice(0, 10),
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    kind: 'untracked',
    transitMode: 'tube',
    lineName: 'Victoria line',
    plannedDeparture: departure,
    plannedArrival: arrival,
    untrackedStatus: 'planned',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

export function journeyDetail(id: number, legs: JourneyLegDetail[], customName: string | null = null): JourneyDetail {
  return { id, customName, createdAt: '2026-09-01T00:00:00Z', legs, isOwner: true, shareLink: null };
}

/** The `GET /Journeys/mine` row for a journey, its current leg being the
 * first unless `current` says otherwise. */
export function journeyListItem(
  detail: JourneyDetail,
  current: JourneyLegDetail | undefined = detail.legs[0],
  overrides: Partial<JourneyListItem> = {},
): JourneyListItem {
  const first = detail.legs[0];
  const last = detail.legs.at(-1);
  const state = current?.trackedTrainState ?? null;
  return {
    id: detail.id,
    customName: detail.customName,
    createdAt: detail.createdAt,
    legId: current?.id ?? 0,
    originCrs: current?.originCrs ?? null,
    destinationCrs: current?.destinationCrs ?? null,
    serviceDate: current?.serviceDate ?? '2026-09-17',
    originName: current?.originName ?? null,
    destinationName: current?.destinationName ?? null,
    firstOriginCrs: first?.originCrs ?? null,
    firstOriginName: first?.originName ?? null,
    lastDestinationCrs: last?.destinationCrs ?? null,
    lastDestinationName: last?.destinationName ?? null,
    firstServiceDate: first?.serviceDate ?? '2026-09-17',
    legCount: detail.legs.length,
    matchMode: current?.matchMode ?? 'manual',
    trainSubscriptionId: state === null ? null : state.id,
    resolutionStatus: state?.resolutionStatus ?? null,
    status: state?.status ?? null,
    delayMinutes: state?.delayMinutes ?? null,
    trackingPhase: state?.trackingPhase ?? null,
    kind: current?.kind ?? 'train',
    untrackedStatus: current?.untrackedStatus ?? null,
    ...overrides,
  };
}

/** A `GET /Train/mine` row for a train tracked on its own (no journey):
 * Kings Cross to York, departing 10:00 UK time (09:00Z) on 17 Sep 2026,
 * timetabled and not yet travelled. */
export function loneTrain(id: number, overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id,
    serviceDate: '2026-09-17',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: 'London Kings Cross',
    pinDestinationName: 'York',
    pinScheduledDeparture: '2026-09-17T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: `W${id}`,
    status: null,
    delayMinutes: null,
    trackedAt: '2026-09-01T00:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    journeyIds: [],
    travelled: false,
    trackingPhase: 'planned',
    ...overrides,
  };
}
