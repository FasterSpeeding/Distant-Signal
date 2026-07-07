import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
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

  it('throws a descriptive error on a non-2xx response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('not found', { status: 404 })),
    );
    await expect(getLineStatus(['not-a-line'], false)).rejects.toThrow(/404/);
  });
});
