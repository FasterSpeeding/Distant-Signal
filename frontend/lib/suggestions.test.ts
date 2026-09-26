import { describe, it, expect, vi, afterEach } from 'vitest';
import { searchNearbyStations, getStationNames } from './suggestions';

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

  it('throws on a non-2xx response, so a real failure is never mistaken for "nothing found"', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('bad request', { status: 400 })));

    await expect(searchNearbyStations(0, 0)).rejects.toThrow('400');
  });
});

describe('getStationNames', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('resolves each distinct code to its exact-match name via /api/stations?q=', async () => {
    const fetchMock = vi.fn(async (input: string) => {
      const url = new URL(input, 'http://localhost');
      const q = url.searchParams.get('q');
      if (q === 'EUS') {
        return new Response(JSON.stringify([{ code: 'EUS', name: 'London Euston' }]), { status: 200 });
      }
      if (q === 'MKC') {
        return new Response(JSON.stringify([{ code: 'MKC', name: 'Milton Keynes Central' }]), { status: 200 });
      }
      return new Response(JSON.stringify([]), { status: 200 });
    });
    vi.stubGlobal('fetch', fetchMock);

    const names = await getStationNames(['EUS', 'MKC']);

    expect(names).toEqual(new Map([['EUS', 'London Euston'], ['MKC', 'Milton Keynes Central']]));
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('fetches each distinct code only once, even when it appears multiple times', async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify([{ code: 'EUS', name: 'London Euston' }]), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);

    await getStationNames(['EUS', 'EUS', 'EUS']);

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('omits a code with no exact-match row (substring search returned other stations only)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([{ code: 'EUSTON SQUARE', name: 'Not a real CRS' }]), { status: 200 })),
    );

    const names = await getStationNames(['ZZZ']);

    expect(names.size).toBe(0);
  });

  it('omits a code whose lookup fails, without losing names for the other codes', async () => {
    const fetchMock = vi.fn(async (input: string) => {
      const url = new URL(input, 'http://localhost');
      if (url.searchParams.get('q') === 'EUS') {
        return new Response(JSON.stringify([{ code: 'EUS', name: 'London Euston' }]), { status: 200 });
      }
      throw new TypeError('Failed to fetch');
    });
    vi.stubGlobal('fetch', fetchMock);

    const names = await getStationNames(['EUS', 'MKC']);

    expect(names).toEqual(new Map([['EUS', 'London Euston']]));
  });
});
