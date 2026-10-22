import type {
  BreakResponse,
  OpenBreakSummary,
  ReplanSuggestion,
  ReplanSuggestionLeg,
  SuggestionSet,
} from '@/lib/types';

/* Mid-journey changes builders (`lib/journeyBreak`, `AlternativesSheet`,
 * `JourneyBreakBanner`). Defaults describe a journey to Edinburgh whose
 * 09:52 from York (08:52Z on 17 Sep 2026) broke; the suggestions leave
 * York at 10:04, 09:58 and 10:20 UK time. */

type TrainSuggestionLeg = Extract<ReplanSuggestionLeg, { kind: 'train' }>;

export function trainSuggestionLeg(overrides: Partial<TrainSuggestionLeg> = {}): TrainSuggestionLeg {
  return {
    kind: 'train',
    trainUid: 'L12345',
    serviceDate: '2026-09-17',
    originCrs: 'YRK',
    originName: 'York',
    destinationCrs: 'EDB',
    destinationName: 'Edinburgh',
    departure: '2026-09-17T09:04:00Z',
    arrival: '2026-09-17T11:31:00Z',
    scheduledDeparture: '2026-09-17T09:04:00Z',
    scheduledArrival: '2026-09-17T11:31:00Z',
    operator: 'GR',
    operatorName: 'LNER',
    platform: null,
    cancelled: false,
    ...overrides,
  };
}

export function replanSuggestion(overrides: Partial<ReplanSuggestion> = {}): ReplanSuggestion {
  return {
    id: 's1',
    departure: '2026-09-17T09:04:00Z',
    arrival: '2026-09-17T11:31:00Z',
    changeCount: 0,
    delayMinutes: 12,
    throughLegId: 7,
    rejoinsLegId: null,
    legs: [trainSuggestionLeg()],
    cost: 10,
    fit: 'covered',
    fitLabel: 'Usually covered by your ticket',
    fitReason: 'Your ticket is valid on any train on this route.',
    picks: ['recommended', 'sameOperator'],
    pickLabels: ['Recommended', 'Same operator'],
    card: true,
    ...overrides,
  };
}

export function suggestionSet(overrides: Partial<SuggestionSet> = {}): SuggestionSet {
  return {
    generatedAt: '2026-09-17T08:55:00Z',
    fingerprint: 'fp-1',
    fromLegId: 7,
    originCrs: 'YRK',
    originName: 'York',
    destinationCrs: 'EDB',
    destinationName: 'Edinburgh',
    departAfter: '2026-09-17T08:55:00Z',
    bookedArrival: '2026-09-17T11:19:00Z',
    hint: null,
    trigger: {
      legId: 7,
      departure: '2026-09-17T08:52:00Z',
      originName: 'York',
      destinationName: 'Edinburgh',
      trainDestinationName: 'Edinburgh',
    },
    suggestions: [
      replanSuggestion(),
      replanSuggestion({
        id: 's2',
        departure: '2026-09-17T08:58:00Z',
        arrival: '2026-09-17T11:10:00Z',
        delayMinutes: -9,
        legs: [trainSuggestionLeg({ departure: '2026-09-17T08:58:00Z', arrival: '2026-09-17T11:10:00Z' })],
        fit: 'likelyNewTicket',
        fitLabel: 'May need a new ticket',
        fitReason: 'Your Advance ticket is only valid on LNER.',
        picks: ['fastest'],
        pickLabels: ['Fastest'],
        cost: 12,
      }),
      replanSuggestion({
        id: 's3',
        departure: '2026-09-17T09:20:00Z',
        arrival: '2026-09-17T11:58:00Z',
        delayMinutes: 39,
        legs: [trainSuggestionLeg({ departure: '2026-09-17T09:20:00Z', arrival: '2026-09-17T11:58:00Z' })],
        fit: null,
        fitLabel: null,
        fitReason: null,
        picks: [],
        pickLabels: [],
        card: false,
        cost: 20,
      }),
    ],
    stale: false,
    ...overrides,
  };
}

export function breakResponse(overrides: Partial<BreakResponse> = {}): BreakResponse {
  return {
    id: 5,
    journeyId: 42,
    cause: 'connection_missed',
    fault: 'operator',
    severity: 3,
    status: 'open',
    triggerLegId: 7,
    detectedAt: '2026-09-17T08:50:00Z',
    openedVia: 'in_app',
    faultNote: null,
    suggestions: suggestionSet(),
    stale: false,
    ...overrides,
  };
}

export function openBreakSummary(overrides: Partial<OpenBreakSummary> = {}): OpenBreakSummary {
  return {
    id: 5,
    cause: 'connection_missed',
    fault: 'operator',
    severity: 3,
    triggerLegId: 7,
    detectedAt: '2026-09-17T08:50:00Z',
    suggestionsReady: true,
    stale: false,
    stationName: 'York',
    destinationName: 'Edinburgh',
    suggestionCount: 3,
    firstArrival: '2026-09-17T10:12:00Z',
    ...overrides,
  };
}
