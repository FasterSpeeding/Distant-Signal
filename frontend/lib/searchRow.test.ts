import { describe, expect, it } from 'vitest';
import { searchRowDetails, searchRowSummary, searchRowTime } from './searchRow';
import { train } from '@/test/lineTrainsFixtures';
import type { TrainSearchResult } from './types';

function row(overrides: Partial<TrainSearchResult> = {}): TrainSearchResult {
  return {
    uid: 'C1',
    scheduled: '08:22',
    stationCrs: 'RDG',
    originCrs: 'PAD',
    destinationCrs: 'BRI',
    destinationName: 'Bristol Temple Meads',
    destinationArrival: '09:40',
    destinationArrivalDayOffset: 0,
    operator: 'GW',
    serviceMode: 'train',
    ...overrides,
  };
}

const live = {
  status: 'en_route',
  delayMinutes: 4,
  delayProvisional: false,
  cancelled: false,
  lastReportedLocation: null,
};

describe('searchRowSummary', () => {
  it('maps the route, operator and mode, with no live state from an older backend', () => {
    expect(searchRowSummary(row())).toEqual({
      uid: 'C1',
      operator: 'GW',
      serviceMode: 'train',
      liveTracking: null,
      scope: null,
      direction: null,
      lineDue: null,
      origin: { crs: 'PAD', name: null },
      destination: { crs: 'BRI', name: 'Bristol Temple Meads' },
      onLineStops: [],
      live: null,
    });
  });

  it("uses the row's own live state and origin name when the backend sends them", () => {
    const summary = searchRowSummary(row({ live, originName: 'London Paddington' }));
    expect(summary.live).toEqual(live);
    expect(summary.origin).toEqual({ crs: 'PAD', name: 'London Paddington' });
  });

  it("falls back to a line summary's live state, scope and direction for the same train", () => {
    const known = train({ uid: 'C1', live: { ...live, lastReportedLocation: 'Didcot' } });
    // A row the backend sent with `live: null` (no live state) still
    // borrows the line summary's.
    expect(searchRowSummary(row({ live: null }), known).live?.lastReportedLocation).toBe('Didcot');
    const summary = searchRowSummary(row(), known);
    expect(summary.live?.delayMinutes).toBe(4);
    expect(summary.scope).toBe('line');
    expect(summary.direction).toBe('down');
    // The row's own live state wins.
    expect(searchRowSummary(row({ live: { ...live, delayMinutes: 9 } }), known).live?.delayMinutes).toBe(9);
  });
});

describe('searchRowTime', () => {
  it('prefers the public departure and trims to HH:MM', () => {
    expect(searchRowTime(row({ publicDeparture: '08:20' }))).toBe('08:20');
    expect(searchRowTime(row({ publicDeparture: null, scheduled: '08:22:30' }))).toBe('08:22');
    expect(searchRowTime(row({ scheduled: null }))).toBe('--:--');
  });
});

describe('searchRowDetails', () => {
  const names = new Map([['GW', 'Great Western Railway']]);

  it('names the origin and the operator', () => {
    expect(searchRowDetails(row({ originName: 'London Paddington' }), names)).toBe(
      'From London Paddington · Great Western Railway (GW)',
    );
  });

  it('falls back to the origin code until the backend sends its name', () => {
    expect(searchRowDetails(row(), names)).toBe('From PAD · Great Western Railway (GW)');
  });

  it('says "Starts here" for a train starting at the searched station', () => {
    expect(searchRowDetails(row({ originCrs: 'RDG' }))).toBe('Starts here');
  });

  it('leaves the operator out without a lookup, and says nothing when nothing is known', () => {
    expect(searchRowDetails(row())).toBe('From PAD');
    expect(searchRowDetails(row({ originCrs: null, operator: null }), names)).toBeUndefined();
  });
});
