import { describe, it, expect, vi, afterEach } from 'vitest';
import { searchNearbyStations } from './suggestions';

describe('searchNearbyStations', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches through the same-origin proxy with lat/lon query params', async () => {
    const fetchMock = vi.fn(
      async (_input: string, _init?: RequestInit) =>
        new Response(JSON.stringify([{ code: 'WOK', name: 'Woking', distanceKm: 0.4 }]), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);

    const result = await searchNearbyStations(51.3191, -0.561);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe('/api/stations/nearby?lat=51.3191&lon=-0.561');
    expect(init).toEqual({ signal: undefined });
    expect(result).toEqual([{ code: 'WOK', name: 'Woking', distanceKm: 0.4 }]);
  });

  it('forwards an AbortSignal when provided', async () => {
    const controller = new AbortController();
    const fetchMock = vi.fn(
      async (_input: string, _init?: RequestInit) => new Response(JSON.stringify([]), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);

    await searchNearbyStations(0, 0, controller.signal);

    expect(fetchMock.mock.calls[0][1]).toEqual({ signal: controller.signal });
  });

  it('resolves to an empty array on a non-2xx response, rather than throwing', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('bad request', { status: 400 })));

    expect(await searchNearbyStations(0, 0)).toEqual([]);
  });
});
