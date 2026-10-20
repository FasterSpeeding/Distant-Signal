import { describe, expect, it } from 'vitest';
import { apiQuery, earlierQuery, laterQuery, OVERNIGHT_CUTOFF, timingMode, timingSummary } from './tripPlanTiming';
import { buildTripPlanQuery, type TripPlanQuery } from './tripPlan';
import type { TripPlanJourney, TripPlanResponse } from './types';

const base: TripPlanQuery = {
  originCrs: 'EUS',
  destinationCrs: 'MKC',
  waypointCrs: [],
  date: '2026-10-24',
  results: 'fastest',
};

function journey(departure: [string, number], arrival: [string, number]): TripPlanJourney {
  return {
    changeCount: 0,
    departure: { time: departure[0], dayOffset: departure[1] },
    arrival: { time: arrival[0], dayOffset: arrival[1] },
    totalDurationMinutes: 0,
  };
}

function plan(...journeys: TripPlanJourney[]): TripPlanResponse {
  return { results: 'fastest', segments: [], journeys };
}

describe('timingMode', () => {
  it('is arrive-by only when an arrival time is set', () => {
    expect(timingMode({ arriveBy: '09:00' })).toBe('arriveBy');
    expect(timingMode({ arriveBy: undefined })).toBe('departAfter');
    expect(timingMode({})).toBe('departAfter');
  });
});

describe('apiQuery (overnight mapping)', () => {
  it('sends a time before the cutoff, in either mode, as the previous day searched across midnight', () => {
    expect(OVERNIGHT_CUTOFF).toBe('03:00');
    for (const [query, field] of [
      [{ ...base, arriveBy: '00:30' }, 'arriveBy'],
      [{ ...base, departAfter: '00:30' }, 'departAfter'],
    ] as const) {
      const sent = apiQuery(query);
      expect(sent).toMatchObject({ date: '2026-10-23', [field]: '00:30', dayOffset: 1, acrossMidnight: true });
      const params = new URLSearchParams(buildTripPlanQuery(sent));
      expect(params.get('date')).toBe('2026-10-23');
      expect(params.get(field)).toBe('00:30:00');
      expect(params.get('dayOffset')).toBe('1');
      expect(params.get('acrossMidnight')).toBe('true');
    }
  });

  it('crosses a month and a year boundary', () => {
    expect(apiQuery({ ...base, date: '2027-01-01', arriveBy: '02:59' }).date).toBe('2026-12-31');
  });

  it('leaves the cutoff itself, later times and no time at all alone', () => {
    for (const query of [
      { ...base, arriveBy: '03:00' },
      { ...base, arriveBy: '09:00' },
      { ...base, departAfter: '03:00' },
      { ...base },
    ]) {
      const sent = apiQuery(query);
      expect(sent.date).toBe('2026-10-24');
      expect(sent.dayOffset).toBeUndefined();
      const params = new URLSearchParams(buildTripPlanQuery(sent));
      expect(params.has('dayOffset')).toBe(false);
      expect(params.has('acrossMidnight')).toBe(false);
    }
  });

  it('never carries a stale dayOffset over', () => {
    const sent = apiQuery({ ...base, departAfter: '09:00', dayOffset: 1, acrossMidnight: true });
    expect(sent.dayOffset).toBeUndefined();
    expect(sent.acrossMidnight).toBeUndefined();
  });
});

describe('laterQuery / earlierQuery', () => {
  it('pages later by departure and earlier by arrival, from the journeys shown', () => {
    const shown = plan(journey(['08:10:00', 0], ['09:00:00', 0]), journey(['08:40:00', 0], ['09:20:00', 0]));
    expect(laterQuery({ ...base, arriveBy: '09:30' }, '2026-10-24', shown)).toEqual({
      ...base,
      departAfter: '08:11',
    });
    expect(earlierQuery({ ...base, departAfter: '08:00' }, '2026-10-24', shown)).toEqual({
      ...base,
      arriveBy: '09:19',
    });
  });

  it('keeps the rest of the search', () => {
    const query = { ...base, viaCrs: ['STA'], maxChanges: 1, results: 'options' as const, departAfter: '07:00' };
    const later = laterQuery(query, '2026-10-24', plan(journey(['08:00:00', 0], ['09:00:00', 0])));
    expect(later).toMatchObject({ viaCrs: ['STA'], maxChanges: 1, results: 'options', departAfter: '08:01' });
    expect(later).not.toHaveProperty('arriveBy');
  });

  it('turns a time after midnight into the next calendar date', () => {
    // An overnight arrive-by search: service date 23 Oct, arriving 00:20 (+1).
    const shown = plan(journey(['23:30:00', 0], ['00:20:00', 1]));
    const earlier = earlierQuery({ ...base, arriveBy: '00:30' }, '2026-10-23', shown);
    expect(earlier).toMatchObject({ date: '2026-10-24', arriveBy: '00:19' });
    // ...which the overnight mapping sends back to the 23rd's trains.
    expect(apiQuery(earlier!)).toMatchObject({
      date: '2026-10-23',
      arriveBy: '00:19',
      dayOffset: 1,
      acrossMidnight: true,
    });
    // Later from 23:59 rolls over to 00:00 the next day.
    const late = plan(journey(['23:59:00', 0], ['00:30:00', 1]));
    const later = laterQuery(base, '2026-10-24', late);
    expect(later).toMatchObject({ date: '2026-10-25', departAfter: '00:00' });
    // ...which searches both dates: the 24th's trains after midnight and
    // the 25th's first ones.
    expect(apiQuery(later!)).toMatchObject({
      date: '2026-10-24',
      departAfter: '00:00',
      dayOffset: 1,
      acrossMidnight: true,
    });
  });

  it('rolls back a day from an arrival at midnight', () => {
    const shown = plan(journey(['23:00:00', 0], ['00:00:00', 0]));
    expect(earlierQuery(base, '2026-10-24', shown)).toMatchObject({ date: '2026-10-23', arriveBy: '23:59' });
  });

  it('has nothing to page from without a journey', () => {
    expect(laterQuery(base, '2026-10-24', plan())).toBeNull();
    expect(earlierQuery(base, '2026-10-24', { results: 'fastest', segments: [] })).toBeNull();
  });
});

describe('timingSummary', () => {
  it('reads as the search the visitor made', () => {
    expect(timingSummary({ date: '2026-10-24', arriveBy: '09:00' })).toBe('Arrive by 09:00, 24 Oct 2026');
    expect(timingSummary({ date: '2026-10-24', departAfter: '08:30' })).toBe('Depart at 08:30, 24 Oct 2026');
    expect(timingSummary({ date: '2026-10-24' })).toBe('Depart at 00:00, 24 Oct 2026');
  });
});
