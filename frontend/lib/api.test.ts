import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
  getPreferences,
  getAllLines,
  getCustomLine,
  getLineDefinition,
  getDataFreshness,
  ApiNotFoundError,
} from './api';

const sampleReport = {
  $type: 'NRStatus.LineStatusReport',
  id: 'wcml',
  name: 'West Coast Main Line',
  modeName: 'national-rail',
  operators: ['AW'],
  lineStatuses: [],
};

describe('api client', () => {
  beforeEach(() => {
    vi.stubEnv('API_BASE_URL', 'http://test-api:8080');
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([sampleReport]), { status: 200 })),
    );
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it('getLineStatusForMode fetches the correct URL', async () => {
    await getLineStatusForMode('national-rail');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/Mode/national-rail/Status',
      expect.objectContaining({ next: { revalidate: 30 } }),
    );
  });

  it('getLineStatus joins multiple ids with commas and passes detail=true', async () => {
    await getLineStatus(['wcml', 'swr-alton'], true);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml,swr-alton/Status?detail=true',
      expect.objectContaining({ next: { revalidate: 30 } }),
    );
  });

  it('getLineStatus omits the detail query param when false', async () => {
    await getLineStatus(['wcml'], false);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Status',
      expect.objectContaining({ next: { revalidate: 30 } }),
    );
  });

  it('getStopPointDisruption fetches the correct URL with no caching', async () => {
    await getStopPointDisruption('WOK');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/StopPoint/WOK/Disruption',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineStatusHistory builds the correct range URL', async () => {
    await getLineStatusHistory('wcml', '2026-07-01T00:00:00Z', '2026-07-07T00:00:00Z');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Status/2026-07-01T00:00:00Z/to/2026-07-07T00:00:00Z',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getPreferences fetches the correct URL with no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ pinnedLines: ['wcml'], pinnedStations: ['WOK'] }), { status: 200 })),
    );
    await getPreferences();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/preferences',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getAllLines fetches the correct URL with no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getAllLines();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getCustomLine fetches the correct URL with no caching', async () => {
    const sampleLine = {
      id: 'custom-my-commute',
      name: 'My Commute',
      operators: ['SW'],
      stations: ['WOK', 'WAT'],
      headcodePrefixes: [],
      destinationCrsFilter: [],
    };
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify(sampleLine), { status: 200 })),
    );
    await getCustomLine('custom-my-commute');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/custom-my-commute',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineDefinition fetches the correct URL with no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ stations: ['WOK', 'WAT'], operators: ['SW'] }), { status: 200 })),
    );
    await getLineDefinition('swr-alton');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/swr-alton/definition',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getDataFreshness fetches the correct URL', async () => {
    await getDataFreshness();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/freshness',
      expect.objectContaining({ next: { revalidate: 30 } }),
    );
  });

  it('throws an ApiNotFoundError on a 404 response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('not found', { status: 404 })),
    );
    // Load-bearing for app/lines/[id]/page.tsx, which relies on this
    // specific subtype to distinguish "genuinely not found" (-> notFound())
    // from other failures (-> rethrown, surfaced via error.tsx) — so it's
    // not enough to just match the message, the type must be pinned too.
    await expect(getLineStatus(['not-a-line'], false)).rejects.toThrow(/404/);
    await expect(getLineStatus(['not-a-line'], false)).rejects.toBeInstanceOf(ApiNotFoundError);
  });

  it('throws a plain Error (not an ApiNotFoundError) on a non-404 non-2xx response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('server error', { status: 500 })),
    );
    await expect(getLineStatus(['wcml'], false)).rejects.toThrow(/500/);
    await expect(getLineStatus(['wcml'], false)).rejects.not.toBeInstanceOf(ApiNotFoundError);
  });
});
