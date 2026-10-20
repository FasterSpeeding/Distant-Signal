import { describe, it, expect } from 'vitest';
import { bucketFor, governingPeriod, periodIsActive, periodIsUpcoming, validityPhase } from './validity';
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

describe('validityPhase', () => {
  const at = (hours: number) => new Date(NOW + hours * HOUR).toISOString();

  it('is upcoming when every window starts later', () => {
    expect(validityPhase([period({ fromDate: at(100), toDate: at(124) })], NOW)).toBe('upcoming');
  });

  it('is active while a window covers now, its end exclusive', () => {
    expect(validityPhase([period({ fromDate: at(-1), toDate: at(1) })], NOW)).toBe('active');
    expect(validityPhase([period({ fromDate: at(0), toDate: at(1) })], NOW)).toBe('active');
    expect(validityPhase([period({ fromDate: at(-1), toDate: at(0) })], NOW)).toBe('ended');
  });

  it('is ended when every window has finished', () => {
    expect(validityPhase([period({ fromDate: at(-48), toDate: at(-24) })], NOW)).toBe('ended');
  });

  it('reads several windows straddling now', () => {
    const past = period({ fromDate: at(-48), toDate: at(-24) });
    const current = period({ fromDate: at(-1), toDate: at(1) });
    const future = period({ fromDate: at(24), toDate: at(48) });
    expect(validityPhase([past, current, future], NOW)).toBe('active');
    expect(validityPhase([past, future], NOW)).toBe('upcoming');
  });

  it('treats an open end as never finishing, and isNow as "open-ended", not "now"', () => {
    expect(validityPhase([period({ fromDate: at(-24), toDate: null, isNow: true })], NOW)).toBe('active');
    expect(validityPhase([period({ fromDate: at(24), toDate: null, isNow: true })], NOW)).toBe('upcoming');
  });

  it('is active with no windows at all', () => {
    expect(validityPhase([], NOW)).toBe('active');
  });

  it('compares instants across the October clock change', () => {
    // 00:00 BST to 01:45 BST on Sunday 25 October 2026; 01:30 GMT (the
    // second 01:30 that night, 01:30Z) is after it although it reads earlier.
    const short = [period({ fromDate: '2026-10-24T23:00:00Z', toDate: '2026-10-25T00:45:00Z' })];
    expect(validityPhase(short, Date.parse('2026-10-25T00:30:00Z'))).toBe('active');
    expect(validityPhase(short, Date.parse('2026-10-25T01:30:00Z'))).toBe('ended');
  });
});
