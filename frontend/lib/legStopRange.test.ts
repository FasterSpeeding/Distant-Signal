import { describe, expect, it } from 'vitest';
import { legProgressIndex, legStopRange } from './legStopRange';
import type { JourneyStop } from './types';

function stop(crs: string | null, overrides: Partial<JourneyStop> = {}): JourneyStop {
  return {
    crs,
    name: crs,
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

const route = (...codes: (string | null)[]) => codes.map((code) => stop(code));

describe('legStopRange', () => {
  const ecml = route('KGX', 'PBO', 'YRK', 'NCL', 'EDB');

  it('spans the leg from its boarding stop to its alighting stop, inclusive', () => {
    expect(legStopRange(ecml, 'PBO', 'NCL')).toEqual({ start: 1, end: 3 });
  });

  it('matches station codes case-insensitively', () => {
    expect(legStopRange(ecml, 'pbo', 'ncl')).toEqual({ start: 1, end: 3 });
  });

  it('keeps the start at the origin when the leg boards there', () => {
    expect(legStopRange(ecml, 'KGX', 'YRK')).toEqual({ start: 0, end: 2 });
  });

  it('keeps the end at the terminus when the leg alights there', () => {
    expect(legStopRange(ecml, 'YRK', 'EDB')).toEqual({ start: 2, end: 4 });
  });

  it('is null when the leg covers the whole train (nothing to clip)', () => {
    expect(legStopRange(ecml, 'KGX', 'EDB')).toBeNull();
  });

  it('is null with no board or alight stop (a plain train view)', () => {
    expect(legStopRange(ecml, null, null)).toBeNull();
    expect(legStopRange(ecml, undefined, undefined)).toBeNull();
  });

  it('is null with no stops at all', () => {
    expect(legStopRange([], 'PBO', 'NCL')).toBeNull();
  });

  it('falls back to the origin when the boarding stop is missing from the calling pattern', () => {
    expect(legStopRange(ecml, 'ZZZ', 'YRK')).toEqual({ start: 0, end: 2 });
    expect(legStopRange(ecml, null, 'YRK')).toEqual({ start: 0, end: 2 });
  });

  it('falls back to the terminus when the alighting stop is missing (a diversion)', () => {
    expect(legStopRange(ecml, 'PBO', 'ZZZ')).toEqual({ start: 1, end: 4 });
    expect(legStopRange(ecml, 'PBO', null)).toEqual({ start: 1, end: 4 });
  });

  it('shows everything when neither stop is in the calling pattern', () => {
    expect(legStopRange(ecml, 'ZZZ', 'YYY')).toBeNull();
  });

  it('runs to the terminus when the alighting stop only comes before the boarding stop', () => {
    expect(legStopRange(ecml, 'NCL', 'PBO')).toEqual({ start: 3, end: 4 });
  });

  it('runs to the terminus when the leg boards and alights at a stop the train calls at once', () => {
    expect(legStopRange(ecml, 'YRK', 'YRK')).toEqual({ start: 2, end: 4 });
  });

  describe('a train that calls at a station twice', () => {
    // A loop: Liverpool Lime Street out round the loop and back.
    const loop = route('LIV', 'MRF', 'JAM', 'MRF', 'SOP');

    it('boards at the last call before the alighting stop', () => {
      expect(legStopRange(loop, 'MRF', 'SOP')).toEqual({ start: 3, end: 4 });
    });

    it('alights at the first call after the boarding stop', () => {
      expect(legStopRange(loop, 'LIV', 'MRF')).toEqual({ start: 0, end: 1 });
    });

    it('covers the whole circle when boarding and alighting at its own origin', () => {
      const circle = route('LIV', 'MRF', 'JAM', 'LIV');
      expect(legStopRange(circle, 'LIV', 'LIV')).toBeNull();
      const longer = route('LIV', 'MRF', 'JAM', 'LIV', 'SOP');
      expect(legStopRange(longer, 'LIV', 'LIV')).toEqual({ start: 0, end: 3 });
    });
  });

  it('keeps a cancelled stop at either end inside the range', () => {
    const stops = [
      stop('KGX'),
      stop('PBO', { cancelled: true }),
      stop('YRK'),
      stop('NCL', { cancelled: true }),
      stop('EDB'),
    ];
    expect(legStopRange(stops, 'PBO', 'NCL')).toEqual({ start: 1, end: 3 });
  });

  it('ignores stops with no station code', () => {
    expect(legStopRange(route('KGX', null, 'YRK', 'EDB'), 'YRK', 'EDB')).toEqual({ start: 2, end: 3 });
  });
});

describe('legProgressIndex', () => {
  const range = { start: 2, end: 5 };

  it('has not started before the boarding stop', () => {
    expect(legProgressIndex(-1, range)).toEqual({ index: -1, phase: 'before' });
    expect(legProgressIndex(1, range)).toEqual({ index: -1, phase: 'before' });
  });

  it('renumbers a stop on the leg from the boarding stop', () => {
    expect(legProgressIndex(2, range)).toEqual({ index: 0, phase: 'on' });
    expect(legProgressIndex(4, range)).toEqual({ index: 2, phase: 'on' });
  });

  it('is complete at or past the alighting stop', () => {
    expect(legProgressIndex(5, range)).toEqual({ index: 3, phase: 'after' });
    expect(legProgressIndex(8, range)).toEqual({ index: 3, phase: 'after' });
  });
});
