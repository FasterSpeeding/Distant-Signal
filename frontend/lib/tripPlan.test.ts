import { describe, expect, it, vi, afterEach } from 'vitest';
import { buildTripPlanQuery, collectTripPlanStationCodes, fetchTripPlan, TripPlanError } from './tripPlan';
import type { TripPlanResponse } from './types';

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

describe('collectTripPlanStationCodes', () => {
  it('collects segment endpoints plus every leg endpoint, deduped, across segments and itineraries', () => {
    const response: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'BHM',
          destinationCrs: 'GLC',
          cappedByMaxChanges: false,
          itineraries: [
            {
              // Boards/alights mid-route -- CRE/PRE differ from the
              // segment's own BHM/GLC (see this function's own doc
              // comment).
              legs: [
                {
                  kind: 'train',
                  trainUid: 'X1',
                  serviceDate: '2026-09-23',
                  originCrs: 'CRE',
                  destinationCrs: 'PRE',
                  scheduledDeparture: '10:00:00',
                  scheduledArrival: '11:15:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 0,
              totalDurationMinutes: 75,
            },
            {
              legs: [
                { kind: 'transfer', mode: 'WALK', originCrs: 'BHM', destinationCrs: 'BHM', minutes: 5 },
                {
                  kind: 'train',
                  trainUid: 'X2',
                  serviceDate: '2026-09-23',
                  originCrs: 'BHM',
                  destinationCrs: 'GLC',
                  scheduledDeparture: '10:30:00',
                  scheduledArrival: '11:45:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 1,
              totalDurationMinutes: 75,
            },
          ],
        },
        { originCrs: 'GLC', destinationCrs: 'EDB', cappedByMaxChanges: false, itineraries: [] },
      ],
    };
    const codes = collectTripPlanStationCodes(response);
    expect(new Set(codes)).toEqual(new Set(['BHM', 'GLC', 'CRE', 'PRE', 'EDB']));
    // Deduped -- 'BHM' and 'GLC' each appear multiple times above.
    expect(codes.length).toBe(new Set(codes).size);
  });

  it('omits a leg end that is null (no stanox_crs match for that TIPLOC)', () => {
    const response: TripPlanResponse = {
      results: 'fastest',
      segments: [
        {
          originCrs: 'EUS',
          destinationCrs: 'MKC',
          cappedByMaxChanges: false,
          itineraries: [
            {
              legs: [
                {
                  kind: 'train',
                  trainUid: 'C1',
                  serviceDate: '2026-09-23',
                  originCrs: null,
                  destinationCrs: null,
                  scheduledDeparture: '08:00:00',
                  scheduledArrival: '08:50:00',
                  arrivalDayOffset: 0,
                },
              ],
              changeCount: 0,
              totalDurationMinutes: 50,
            },
          ],
        },
      ],
    };
    expect(new Set(collectTripPlanStationCodes(response))).toEqual(new Set(['EUS', 'MKC']));
  });

  it('returns an empty array for a plan with no segments', () => {
    expect(collectTripPlanStationCodes({ results: 'fastest', segments: [] })).toEqual([]);
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
