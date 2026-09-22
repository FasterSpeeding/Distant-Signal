import { describe, expect, it } from 'vitest';
import { legEndpointName, legRouteAndTime, legDestinationArrivalLabel } from './journeyLegLabel';
import type { JourneyStop } from './types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: null,
    name: null,
    tiploc: null,
    kind: null,
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

const noPin = { pinOriginCrs: null, pinOriginName: null, pinDestinationCrs: null, pinDestinationName: null };

describe('legEndpointName', () => {
  it('resolves the name from the matching journey stop', () => {
    const stops = [stop({ crs: 'YRK', name: 'York' })];
    expect(legEndpointName('YRK', stops, noPin, 'origin')).toBe('York');
  });

  it('matches CRS case-insensitively', () => {
    const stops = [stop({ crs: 'YRK', name: 'York' })];
    expect(legEndpointName('yrk', stops, noPin, 'origin')).toBe('York');
  });

  it('falls back to the pin name when the pin CRS matches and no stop resolved a name', () => {
    const pin = { pinOriginCrs: 'KGX', pinOriginName: 'London Kings Cross', pinDestinationCrs: null, pinDestinationName: null };
    expect(legEndpointName('KGX', null, pin, 'origin')).toBe('London Kings Cross');
  });

  it('does not use the pin name when the pin CRS is for the other end', () => {
    const pin = { pinOriginCrs: 'KGX', pinOriginName: 'London Kings Cross', pinDestinationCrs: null, pinDestinationName: null };
    expect(legEndpointName('YRK', null, pin, 'origin')).toBeNull();
  });

  it('returns null when nothing resolves', () => {
    expect(legEndpointName('YRK', null, null, 'origin')).toBeNull();
    expect(legEndpointName(null, null, null, 'origin')).toBeNull();
  });
});

describe('legRouteAndTime', () => {
  it('renders the route with names and the leg origin departure time', () => {
    const stops = [
      stop({ crs: 'KGX', name: 'London Kings Cross', scheduledDeparture: '2026-09-22T16:00:00Z' }),
      stop({ crs: 'YRK', name: 'York' }),
    ];
    expect(legRouteAndTime('KGX', 'YRK', stops, noPin)).toBe('London Kings Cross (KGX) → York (YRK) · 17:00');
  });

  it('omits the time when the leg origin stop is not found', () => {
    expect(legRouteAndTime('KGX', 'YRK', null, noPin)).toBe('KGX → YRK');
  });

  it('falls back to bare CRS codes when no names resolve', () => {
    const stops = [stop({ crs: 'KGX', name: null }), stop({ crs: 'YRK', name: null })];
    expect(legRouteAndTime('KGX', 'YRK', stops, noPin)).toBe('KGX → YRK');
  });

  it('uses the leg destination stop, not the train terminus, for a mid-route leg', () => {
    const stops = [
      stop({ crs: 'KGX', name: 'London Kings Cross', kind: 'Origin', scheduledDeparture: '2026-09-22T16:00:00Z' }),
      stop({ crs: 'YRK', name: 'York', kind: 'Intermediate' }),
      stop({ crs: 'EDB', name: 'Edinburgh', kind: 'Terminate' }),
    ];
    // Leg is KGX -> YRK even though the train continues to Edinburgh.
    expect(legRouteAndTime('KGX', 'YRK', stops, noPin)).toBe('London Kings Cross (KGX) → York (YRK) · 17:00');
  });
});

describe('legDestinationArrivalLabel', () => {
  it('labels a confirmed actual arrival without hedging', () => {
    const stops = [
      stop({
        crs: 'YRK',
        actualArrival: '2026-09-22T18:22:00Z',
        estimatedArrival: '2026-09-22T18:30:00Z',
        scheduledArrival: '2026-09-22T18:00:00Z',
      }),
    ];
    expect(legDestinationArrivalLabel('YRK', stops)).toBe('arrive 19:22');
  });

  it('hedges with "est." when only an estimate is known', () => {
    const stops = [stop({ crs: 'YRK', estimatedArrival: '2026-09-22T18:30:00Z', scheduledArrival: '2026-09-22T18:00:00Z' })];
    expect(legDestinationArrivalLabel('YRK', stops)).toBe('arrive est. 19:30');
  });

  it('hedges with "est." when only the bare schedule is known', () => {
    const stops = [stop({ crs: 'YRK', scheduledArrival: '2026-09-22T18:00:00Z' })];
    expect(legDestinationArrivalLabel('YRK', stops)).toBe('arrive est. 19:00');
  });

  it('returns null when the destination stop cannot be found', () => {
    expect(legDestinationArrivalLabel('YRK', null)).toBeNull();
    expect(legDestinationArrivalLabel('YRK', [stop({ crs: 'NCL' })])).toBeNull();
  });
});
