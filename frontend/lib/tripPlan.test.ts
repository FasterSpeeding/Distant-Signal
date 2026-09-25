import { describe, expect, it, vi, afterEach } from 'vitest';
import { buildTripPlanQuery, fetchTripPlan, TripPlanError } from './tripPlan';

describe('buildTripPlanQuery', () => {
  it('builds a minimal query with no waypoints or departAfter', () => {
    const query = buildTripPlanQuery({
      originCrs: 'eus',
      destinationCrs: 'mkc',
      waypointCrs: [],
      date: '2026-09-23',
      results: 'fastest',
    });
    const params = new URLSearchParams(query);
    expect(params.get('origin')).toBe('EUS');
    expect(params.get('destination')).toBe('MKC');
    expect(params.get('results')).toBe('fastest');
    expect(params.has('waypoints')).toBe(false);
    expect(params.has('departAfter')).toBe(false);
  });

  it('joins ordered waypoints with a comma, uppercased', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'EDB',
      waypointCrs: ['york', 'ncl'],
      date: '2026-09-23',
      results: 'options',
    });
    expect(new URLSearchParams(query).get('waypoints')).toBe('YORK,NCL');
  });

  it('filters out blank waypoint entries', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: ['', '  ', 'YRK'],
      date: '2026-09-23',
      results: 'fastest',
    });
    expect(new URLSearchParams(query).get('waypoints')).toBe('YRK');
  });

  it('appends :00 to a departAfter HH:MM value', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-09-23',
      departAfter: '08:30',
      results: 'fastest',
    });
    expect(new URLSearchParams(query).get('departAfter')).toBe('08:30:00');
  });
});

describe('fetchTripPlan', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the parsed response on success', async () => {
    const body = { results: 'fastest', segments: [] };
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(body) } as Response)
    );
    const result = await fetchTripPlan({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-09-23',
      results: 'fastest',
    });
    expect(result).toEqual(body);
  });

  it('throws TripPlanError with the backend message and status on failure', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue({ ok: false, status: 404, text: () => Promise.resolve('no schedule data published') } as Response)
    );
    await expect(
      fetchTripPlan({ originCrs: 'EUS', destinationCrs: 'MKC', waypointCrs: [], date: '2099-01-01', results: 'fastest' })
    ).rejects.toMatchObject(new TripPlanError('no schedule data published', 404));
  });

  it('still surfaces the backend message verbatim on a 400 (bad CRS/results value)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: false, status: 400, text: () => Promise.resolve('unknown CRS code') } as Response)
    );
    await expect(
      fetchTripPlan({ originCrs: 'ZZZ', destinationCrs: 'MKC', waypointCrs: [], date: '2026-09-23', results: 'fastest' })
    ).rejects.toMatchObject(new TripPlanError('unknown CRS code', 400));
  });

  // The bug: an unreachable/misbehaving backend's raw response body (which
  // can be Next's own proxy error text, an HTML error page, or a stack
  // trace fragment) used to become the message shown verbatim in
  // PlanTripFlow's alert for ANY non-ok status, not just the two
  // backend-authored ones (400/404). A 500 must now get a generic,
  // honest message instead -- while the real body is still logged to the
  // console so the failure stays debuggable server-side.
  it('replaces a non-400/404 error body with a generic message, logging the raw body instead', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue({ ok: false, status: 500, text: () => Promise.resolve('<html>Internal Server Error</html>') } as Response)
    );
    const promise = fetchTripPlan({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-09-23',
      results: 'fastest',
    });
    await expect(promise).rejects.toBeInstanceOf(TripPlanError);
    await expect(promise).rejects.toMatchObject({
      status: 500,
      message: 'Something went wrong planning this trip. Please try again.',
    });
    await expect(promise).rejects.not.toMatchObject({ message: expect.stringContaining('<html>') });
    expect(consoleError).toHaveBeenCalledWith(expect.stringContaining('500'), '<html>Internal Server Error</html>');
  });
});
