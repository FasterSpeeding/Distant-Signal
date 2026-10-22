import { describe, expect, it } from 'vitest';
import { apiQuery, OVERNIGHT_CUTOFF, timingMode, timingSummary } from './tripPlanTiming';
import { buildTripPlanQuery, type TripPlanQuery } from './tripPlan';

const base: TripPlanQuery = {
  originCrs: 'EUS',
  destinationCrs: 'MKC',
  waypointCrs: [],
  date: '2026-10-24',
  results: 'fastest',
};

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

describe('timingSummary', () => {
  it('reads as the search the visitor made', () => {
    expect(timingSummary({ date: '2026-10-24', arriveBy: '09:00' })).toBe('Arrive by 09:00, 24 Oct 2026');
    expect(timingSummary({ date: '2026-10-24', departAfter: '08:30' })).toBe('Depart at 08:30, 24 Oct 2026');
    expect(timingSummary({ date: '2026-10-24' })).toBe('Depart at 00:00, 24 Oct 2026');
  });
});
