import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
  getLineDailyStats,
  getPreferences,
  getAllLines,
  getAllTocs,
  getCustomLine,
  getLineDefinition,
  getDataFreshness,
  getHistoryRetention,
  getStationName,
  getTrackedTrainById,
  getTrackedTrainByUidAndDate,
  getTicketsForTrackedTrain,
  getMyTrackedTrains,
  getDelayRepayEstimate,
  getMyTickets,
  getIncident,
  ApiNotFoundError,
} from './api';

// `getPreferences` reads the incoming request's cookies through
// `next/headers` so it can forward them to the backend (a Server
// Component's own fetch carries none of the browser's cookies by itself).
// There is no Next.js request context in a unit test, so `cookies()` is
// stubbed here; `incomingCookies` is what each test dials in.
const incomingCookies = { header: '' };
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => incomingCookies.header }),
}));

const sampleReport = {
  $type: 'DistantSignal.LineStatusReport',
  id: 'wcml',
  name: 'West Coast Main Line',
  modeName: 'national-rail',
  operators: ['VT'],
  lineStatuses: [],
};

describe('api client', () => {
  beforeEach(() => {
    incomingCookies.header = '';
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

  it('getLineStatusForMode fetches the correct URL with no caching', async () => {
    await getLineStatusForMode('national-rail');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/Mode/national-rail/Status',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineStatusForMode passes a comma-separated mode list through unescaped', async () => {
    await getLineStatusForMode('national-rail,tube,tram');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/Mode/national-rail,tube,tram/Status',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineStatus joins multiple ids with commas and passes detail=true', async () => {
    await getLineStatus(['wcml', 'swr-alton'], true);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml,swr-alton/Status?detail=true',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineStatus omits the detail query param when false', async () => {
    await getLineStatus(['wcml'], false);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Status',
      expect.objectContaining({ cache: 'no-store' }),
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

  it('getLineDailyStats builds the correct range URL', async () => {
    const sampleStats = [
      {
        day: '2026-07-01',
        sampleCycles: 100,
        total: 150,
        delayed: 30,
        cancelled: 5,
        skipped: 2,
        avgDelayMinutes: 12.5,
        delayRate: 0.2,
        cancellationRate: 0.033,
        skipRate: 0.013,
      },
    ];
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify(sampleStats), { status: 200 })),
    );
    await getLineDailyStats('wcml', '2026-07-01', '2026-07-07');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Stats/2026-07-01/to/2026-07-07',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineDailyStats resolves a 200 with an array as-is', async () => {
    const sampleStats = [
      {
        day: '2026-07-01',
        sampleCycles: 100,
        total: 150,
        delayed: 30,
        cancelled: 5,
        skipped: 2,
        avgDelayMinutes: 12.5,
        delayRate: 0.2,
        cancellationRate: 0.033,
        skipRate: 0.013,
      },
    ];
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify(sampleStats), { status: 200 })),
    );
    await expect(getLineDailyStats('wcml', '2026-07-01', '2026-07-07')).resolves.toEqual(sampleStats);
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

  // `/public/preferences` requires an authenticated user. A Server
  // Component's own fetch does NOT inherit the browser's cookies, so
  // without this forwarding a genuinely logged-in visitor's session would
  // be invisible to the backend and every one of these pages would render
  // unpersonalized (or, before the 401 tolerance below, not at all).
  it('getPreferences forwards the incoming request cookies to the backend', async () => {
    incomingCookies.header = 'distant_signal_session=abc123; theme=dark';
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ pinnedLines: ['wcml'], pinnedStations: [] }), { status: 200 })),
    );
    await expect(getPreferences()).resolves.toEqual({ pinnedLines: ['wcml'], pinnedStations: [] });
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/preferences',
      expect.objectContaining({ headers: { Cookie: 'distant_signal_session=abc123; theme=dark' } }),
    );
  });

  it('getPreferences sends no Cookie header when the visitor has no cookies at all', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ pinnedLines: [], pinnedStations: [] }), { status: 200 })),
    );
    await getPreferences();
    const init = vi.mocked(fetch).mock.calls[0][1] as RequestInit;
    expect(init.headers).toBeUndefined();
  });

  // Load-bearing for app/page.tsx, app/lines/page.tsx and
  // app/stations/[crs]/page.tsx: all three await getPreferences()
  // unguarded, so a thrown 401 would take the whole page down for every
  // anonymous visitor. "Not signed in" must degrade to "nothing pinned".
  it('getPreferences treats a 401 as no preferences rather than throwing', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('no session', { status: 401 })),
    );
    await expect(getPreferences()).resolves.toEqual({ pinnedLines: [], pinnedStations: [] });
  });

  // The 401 tolerance above is deliberately narrow: a backend that is down
  // or broken must still surface as an error, not masquerade as an
  // anonymous visitor with an empty dashboard.
  it('getPreferences still throws on a non-401 failure', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('server error', { status: 500 })),
    );
    await expect(getPreferences()).rejects.toThrow(/500/);
  });

  // The 401 tolerance is scoped to /public/preferences alone — every other
  // endpoint routed through `fetchJson` must keep throwing on a 401.
  it('a 401 from any other endpoint still throws', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('unauthorized', { status: 401 })),
    );
    await expect(getAllLines()).rejects.toThrow(/401/);
    await expect(getAllLines()).rejects.not.toBeInstanceOf(ApiNotFoundError);
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

  it('getAllTocs fetches the correct URL, cached for an hour', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getAllTocs();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/tocs/all',
      expect.objectContaining({ next: { revalidate: 3600 } }),
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

  it('getDataFreshness fetches the correct URL with no caching', async () => {
    await getDataFreshness();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/freshness',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getHistoryRetention fetches the correct URL with no caching', async () => {
    await getHistoryRetention();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/history-retention',
      expect.objectContaining({ cache: 'no-store' }),
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

  it('getStationName caches the CRS lookup rather than refetching per render', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([{ code: 'WOK', name: 'Woking' }]), { status: 200 })),
    );
    // CRS -> name is reference data that changes on the order of years, so
    // unlike the live disruption feeds this must not be `no-store`.
    await expect(getStationName('WOK')).resolves.toBe('Woking');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/stations?q=WOK',
      expect.objectContaining({ next: { revalidate: 3600 } }),
    );
  });

  it('getStationName returns null when the search has no exact match', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([{ code: 'WOF', name: 'Wolverton' }]), { status: 200 })),
    );
    await expect(getStationName('WOK')).resolves.toBeNull();
  });

  it('getTrackedTrainById fetches the correct URL with no caching', async () => {
    await getTrackedTrainById(42);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/42',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getTrackedTrainByUidAndDate fetches the correct URL with no caching', async () => {
    await getTrackedTrainByUidAndDate('C21373', '2026-08-28');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/by-uid/C21373/2026-08-28',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getTrackedTrainById throws ApiNotFoundError on a 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getTrackedTrainById(999)).rejects.toBeInstanceOf(ApiNotFoundError);
  });

  it('getTicketsForTrackedTrain fetches the correct URL, forwarding cookies, with no caching', async () => {
    incomingCookies.header = 'distant_signal_session=abc123';
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await getTicketsForTrackedTrain(1);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/1/tickets',
      expect.objectContaining({
        cache: 'no-store',
        headers: { Cookie: 'distant_signal_session=abc123' },
      }),
    );
  });

  it('getTicketsForTrackedTrain returns null on a 401 (not logged in)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toBeNull();
  });

  it('getTicketsForTrackedTrain returns null on a 404 (logged in, not the owner)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toBeNull();
  });

  it('getTicketsForTrackedTrain resolves an empty array as owner-with-no-tickets, not null', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await expect(getTicketsForTrackedTrain(1)).resolves.toEqual([]);
  });

  it('getTicketsForTrackedTrain still throws on a non-401/404 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getTicketsForTrackedTrain(1)).rejects.toThrow(/500/);
  });

  it('getMyTrackedTrains fetches the correct URL, forwarding cookies, with no caching', async () => {
    incomingCookies.header = 'distant_signal_session=abc123';
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await getMyTrackedTrains();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/mine',
      expect.objectContaining({
        cache: 'no-store',
        headers: { Cookie: 'distant_signal_session=abc123' },
      }),
    );
  });

  it('getMyTrackedTrains returns null on a 401 (not logged in)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getMyTrackedTrains()).resolves.toBeNull();
  });

  it('getMyTrackedTrains resolves an empty array as logged-in-with-nothing-tracked, not null', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await expect(getMyTrackedTrains()).resolves.toEqual([]);
  });

  it('getMyTrackedTrains still throws on a non-401 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getMyTrackedTrains()).rejects.toThrow(/500/);
  });

  it('getDelayRepayEstimate fetches the correct URL with no caching', async () => {
    const sample = { delayMinutes: 45, estimate: null, claimUrl: 'https://example.com', disclaimer: 'x' };
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(sample), { status: 200 })));
    await getDelayRepayEstimate(1, 7);
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/1/tickets/7/delay-repay',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getDelayRepayEstimate returns null on a 401 or 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getDelayRepayEstimate(1, 7)).resolves.toBeNull();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getDelayRepayEstimate(1, 7)).resolves.toBeNull();
  });

  it('getMyTickets fetches the correct URL, forwarding cookies, with no caching', async () => {
    incomingCookies.header = 'distant_signal_session=abc123';
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await getMyTickets();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Train/tickets/mine',
      expect.objectContaining({
        cache: 'no-store',
        headers: { Cookie: 'distant_signal_session=abc123' },
      }),
    );
  });

  it('getMyTickets returns null on a 401 (not logged in)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
    await expect(getMyTickets()).resolves.toBeNull();
  });

  it('getMyTickets resolves an empty array as logged-in-with-no-tickets, not null', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
    await expect(getMyTickets()).resolves.toEqual([]);
  });

  it('getMyTickets still throws on a non-401 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getMyTickets()).rejects.toThrow(/500/);
  });

  it('getIncident fetches the correct URL with no caching', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ incidentId: '123' }), { status: 200 })));
    await getIncident('123');
    expect(fetch).toHaveBeenCalledWith('http://test-api:8080/public/incidents/123', expect.objectContaining({ cache: 'no-store' }));
  });

  it('getIncident throws ApiNotFoundError on a 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getIncident('does-not-exist')).rejects.toThrow(ApiNotFoundError);
  });

  it('getIncident still throws on a non-404 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getIncident('123')).rejects.toThrow(/500/);
  });

  // `incidentId` ultimately originates from external feed data (see
  // `incidentIdFromSource`), so it must be percent-encoded before being
  // interpolated into the URL -- otherwise a path-like value (e.g.
  // containing `/` or `..`) could resolve to an unrelated route.
  it('getIncident percent-encodes the incident id in the URL', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ incidentId: '123/../lines' }), { status: 200 })));
    await getIncident('123/../lines');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/incidents/123%2F..%2Flines',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
});
