import { describe, it, expect } from 'vitest';
import { bucketFor, governingPeriod, periodIsActive, periodIsUpcoming } from './validity';
import type { LineStatus, ValidityPeriod } from './types';

const NOW = Date.parse('2026-08-21T12:00:00Z');
const HOUR = 3_600_000;

function period(overrides: Partial<ValidityPeriod> = {}): ValidityPeriod {
  return { fromDate: new Date(NOW - HOUR).toISOString(), toDate: null, isNow: false, ...overrides };
}

function status(periods: ValidityPeriod[]): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Engineering works',
    dataQuality: 'planned',
    validityPeriods: periods,
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
  };
}

describe('periodIsActive', () => {
  it('trusts isNow for a period that has already started', () => {
    expect(periodIsActive(period({ fromDate: new Date(NOW - HOUR).toISOString(), isNow: true }), NOW)).toBe(true);
  });

  // Regression: `isNow` is built by the backend as `end_time.is_none()` --
  // "open-ended", NOT "covers now" (see `validity.ts`'s own doc comment). A
  // planned future engineering work therefore arrives as
  // `{ fromDate: <future>, toDate: null, isNow: true }`, and this function
  // used to short-circuit on the flag and call it Active today -- wrong tab,
  // and the Upcoming count on every line/station page short by one.
  it('does NOT treat a future-dated open-ended period as active, even with isNow set', () => {
    const futureOpenEnded = period({
      fromDate: new Date(NOW + 72 * HOUR).toISOString(),
      toDate: null,
      isNow: true,
    });
    expect(periodIsActive(futureOpenEnded, NOW)).toBe(false);
    // ...and it lands in Upcoming rather than falling out of every bucket.
    expect(periodIsUpcoming(futureOpenEnded, NOW)).toBe(true);
    expect(bucketFor(status([futureOpenEnded]), NOW)).toBe('upcoming');
  });

  it('still trusts isNow when fromDate is unparseable (no start to compare against)', () => {
    expect(periodIsActive(period({ fromDate: 'not a date', isNow: true }), NOW)).toBe(true);
    expect(periodIsActive(period({ fromDate: 'not a date', isNow: false }), NOW)).toBe(false);
  });

  it('treats a period that started in the past and has not ended as active, despite isNow being false', () => {
    // The exact shape the backend produces for in-progress planned works:
    // `is_now` is derived from "has no end time", so a dated window that
    // spans right now still arrives false.
    const spanning = period({
      fromDate: new Date(NOW - 5 * HOUR).toISOString(),
      toDate: new Date(NOW + 5 * HOUR).toISOString(),
      isNow: false,
    });
    expect(periodIsActive(spanning, NOW)).toBe(true);
  });

  it('treats an open-ended period that started in the past as active', () => {
    expect(periodIsActive(period({ toDate: null }), NOW)).toBe(true);
  });

  it('does not treat a finished period as active', () => {
    const ended = period({
      fromDate: new Date(NOW - 5 * HOUR).toISOString(),
      toDate: new Date(NOW - HOUR).toISOString(),
    });
    expect(periodIsActive(ended, NOW)).toBe(false);
  });

  it('does not treat a future period as active', () => {
    expect(periodIsActive(period({ fromDate: new Date(NOW + HOUR).toISOString() }), NOW)).toBe(false);
  });

  it('falls back to active rather than silently dropping an unparseable toDate', () => {
    expect(periodIsActive(period({ toDate: 'not a date' }), NOW)).toBe(true);
  });
});

describe('periodIsUpcoming', () => {
  it('is true only for a period that has not started', () => {
    expect(periodIsUpcoming(period({ fromDate: new Date(NOW + HOUR).toISOString() }), NOW)).toBe(true);
    expect(periodIsUpcoming(period(), NOW)).toBe(false);
  });
});

describe('bucketFor', () => {
  it('buckets an in-progress planned work as active', () => {
    const spanning = period({
      fromDate: new Date(NOW - HOUR).toISOString(),
      toDate: new Date(NOW + HOUR).toISOString(),
    });
    expect(bucketFor(status([spanning]), NOW)).toBe('active');
  });

  it('buckets a wholly future window as upcoming', () => {
    const future = period({
      fromDate: new Date(NOW + HOUR).toISOString(),
      toDate: new Date(NOW + 2 * HOUR).toISOString(),
    });
    expect(bucketFor(status([future]), NOW)).toBe('upcoming');
  });

  it('buckets a wholly past window as ended', () => {
    const past = period({
      fromDate: new Date(NOW - 2 * HOUR).toISOString(),
      toDate: new Date(NOW - HOUR).toISOString(),
    });
    expect(bucketFor(status([past]), NOW)).toBe('ended');
  });

  it('checks every period, not just the first', () => {
    const past = period({
      fromDate: new Date(NOW - 3 * HOUR).toISOString(),
      toDate: new Date(NOW - 2 * HOUR).toISOString(),
    });
    const future = period({ fromDate: new Date(NOW + HOUR).toISOString() });
    expect(bucketFor(status([past, future]), NOW)).toBe('upcoming');
  });

  it('treats a status carrying no validity information at all as active', () => {
    // Matches the aggregator's own fallback for an incident with no
    // validity period (`from_date: now, to_date: None, is_now: true`) —
    // "we do not know when" must not read as "it is over".
    expect(bucketFor(status([]), NOW)).toBe('active');
  });
});

describe('governingPeriod', () => {
  it('prefers the period covering now', () => {
    const past = period({
      fromDate: new Date(NOW - 3 * HOUR).toISOString(),
      toDate: new Date(NOW - 2 * HOUR).toISOString(),
    });
    const current = period({ fromDate: new Date(NOW - HOUR).toISOString() });
    expect(governingPeriod(status([past, current]), NOW)).toBe(current);
  });

  it('falls back to the soonest future period', () => {
    const later = period({ fromDate: new Date(NOW + 5 * HOUR).toISOString() });
    const sooner = period({ fromDate: new Date(NOW + HOUR).toISOString() });
    expect(governingPeriod(status([later, sooner]), NOW)).toBe(sooner);
  });

  it('returns undefined when there is nothing to describe', () => {
    expect(governingPeriod(status([]), NOW)).toBeUndefined();
  });
});
