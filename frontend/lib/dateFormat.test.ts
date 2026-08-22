import { describe, it, expect } from 'vitest';
import { formatDate, formatDateTime, formatTime, londonDayKey } from './dateFormat';

describe('formatDate', () => {
  it('renders an unambiguous UK date, never M/D/YYYY', () => {
    expect(formatDate('2026-05-10T00:00:00Z')).toBe('10 May 2026');
    expect(formatDate('2026-10-11T00:00:00Z')).toBe('11 Oct 2026');
  });

  it('is independent of the runtime locale and timezone', () => {
    // Node in CI/containers resolves to en-US/UTC; a UK browser does not.
    // A bare `toLocaleDateString()` therefore produced different markup on
    // the server and the client — a hydration mismatch as well as a
    // correctness bug.
    expect(Intl.DateTimeFormat().resolvedOptions().locale).not.toBe('en-GB');
    expect(formatDate('2026-05-10T00:00:00Z')).toBe('10 May 2026');
  });
});

describe('formatDateTime', () => {
  it('renders a 24-hour UK date-time with no seconds', () => {
    expect(formatDateTime('2026-08-19T18:56:01Z')).toBe('19 Aug 2026, 19:56');
  });
});

describe('formatTime', () => {
  it('renders a 24-hour London wall-clock time', () => {
    expect(formatTime('2026-08-19T18:56:01Z')).toBe('19:56');
  });
});

describe('londonDayKey', () => {
  it('keys by the London calendar day, not the UTC one', () => {
    // 23:30 UTC on 19 Aug is 00:30 on 20 Aug in British Summer Time.
    expect(londonDayKey('2026-08-19T23:30:00Z')).toBe('2026-08-20');
    expect(londonDayKey('2026-08-19T12:00:00Z')).toBe('2026-08-19');
  });
});
