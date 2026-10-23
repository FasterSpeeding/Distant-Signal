import { afterEach, describe, expect, it, vi } from 'vitest';
import { searchPlannerEnds } from './plannerEnds';
import { codeRouteLabel, codeStationLabel, isTflStopCode, tflStopPlaceName } from './stationLabel';

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status });
}

const UXBRIDGE = {
  id: '940GZZLUUXB',
  name: 'Uxbridge',
  modes: ['tube'],
  lines: [
    { id: 'metropolitan', name: 'Metropolitan' },
    { id: 'piccadilly', name: 'Piccadilly' },
  ],
};

describe('searchPlannerEnds', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('returns nothing for a blank query without fetching', async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    expect(await searchPlannerEnds(' ')).toEqual([]);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('lists TfL stops by their stop id after the stations, best match first', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.startsWith('/api/stations')) return json([{ code: 'UXX', name: 'Uxbridge Road' }]);
      if (url.startsWith('/api/tfl/stops')) return json([UXBRIDGE]);
      return json([], 404);
    });
    vi.stubGlobal('fetch', fetchMock);
    const results = await searchPlannerEnds('uxbridge');
    expect(fetchMock).toHaveBeenCalledWith('/api/stations?q=uxbridge&stops=true', {});
    expect(fetchMock).toHaveBeenCalledWith('/api/tfl/stops?q=uxbridge&modes=tube%2Cdlr%2Ctram', {});
    expect(results).toEqual([
      { code: 'UXX', name: 'Uxbridge Road' },
      { code: '940GZZLUUXB', name: 'Uxbridge (Tube)', detail: 'Tube: Metropolitan, Piccadilly' },
    ]);
  });

  it('keeps the stations when the stop lookup fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.startsWith('/api/stations')) return json([{ code: 'EUS', name: 'London Euston' }]);
        throw new TypeError('network down');
      }),
    );
    expect(await searchPlannerEnds('euston')).toEqual([{ code: 'EUS', name: 'London Euston' }]);
  });
});

describe('TfL stop codes', () => {
  it('recognises a stop id, never a CRS, a tiploc: code or a group', () => {
    expect(isTflStopCode('940GZZLUUXB')).toBe(true);
    expect(isTflStopCode('940gzzdlbec')).toBe(true);
    for (const code of ['UXB', 'tiploc:UXBRIDG', 'group:LON', '940G', null]) expect(isTflStopCode(code)).toBe(false);
  });

  it('names a stop by its name and modes, never its id', () => {
    expect(tflStopPlaceName(UXBRIDGE)).toBe('Uxbridge (Tube)');
    expect(tflStopPlaceName({ name: 'Bank', modes: ['tube', 'dlr'] })).toBe('Bank (Tube, DLR)');
    expect(codeStationLabel('940GZZLUUXB', 'Uxbridge (Tube)')).toBe('Uxbridge (Tube)');
    expect(codeRouteLabel('940GZZLUUXB', 'Uxbridge (Tube)', 'HOH', undefined)).toBe('Uxbridge (Tube) to HOH');
  });
});
