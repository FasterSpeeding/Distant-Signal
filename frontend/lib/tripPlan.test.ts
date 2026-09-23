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
});
