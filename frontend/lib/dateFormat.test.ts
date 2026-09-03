import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  formatDate,
  formatDateTime,
  formatLocalDateTime,
  formatTime,
  londonDayKey,
} from './dateFormat';

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

describe('formatLocalDateTime', () => {
  // The one formatter in the module with no `timeZone`, so it is the one
  // test block whose result depends on the process's ambient zone. Node
  // re-reads `process.env.TZ` for *subsequently* constructed `Intl`
  // objects, and `formatLocalDateTime` builds its formatter per call
  // precisely so that is observable (dateFormat.ts, that function's
  // comment) -- verified on this repo's Node 24. Saved and restored around
  // each case because the surrounding blocks in this file assert against
  // the ambient zone/locale being non-UK.
  //
  // Restored by `delete`, not by assignment, when `TZ` was unset to begin
  // with (it is, both in this container and in CI -- `.github/workflows/
  // ci.yml` sets no `TZ`): `process.env` coerces its values to strings, so
  // `process.env.TZ = undefined` writes the literal string "undefined",
  // which Node then resolves to UTC rather than back to the host zone.
  const originalTz = process.env.TZ;
  const restoreTz = () => {
    if (originalTz === undefined) {
      delete process.env.TZ;
    } else {
      process.env.TZ = originalTz;
    }
  };

  beforeEach(restoreTz);
  afterEach(restoreTz);

  it('matches formatDateTime when the host zone is London', () => {
    process.env.TZ = 'Europe/London';
    expect(formatLocalDateTime('2026-08-19T18:56:01Z')).toBe('19 Aug 2026, 19:56');
    expect(formatLocalDateTime('2026-08-19T18:56:01Z')).toBe(formatDateTime('2026-08-19T18:56:01Z'));
  });

  it('differs from formatDateTime when the host zone is not London', () => {
    // 18:56 UTC on 19 Aug is 03:56 the *next* day in Tokyo but 19:56 the
    // same evening in London -- a different time and a different date, so
    // this catches a regression that silently reverted to the pinned zone.
    process.env.TZ = 'Asia/Tokyo';
    expect(formatLocalDateTime('2026-08-19T18:56:01Z')).toBe('20 Aug 2026, 03:56');
    expect(formatDateTime('2026-08-19T18:56:01Z')).toBe('19 Aug 2026, 19:56');
  });

  it('keeps the en-GB medium-date/short-time shape, never M/D/YYYY or seconds', () => {
    // The locale stays explicit even though the timezone does not: this is
    // a timezone change only, and dropping `en-GB` would reinstate the
    // "5/10/2026 for 10 May" bug the module header describes.
    process.env.TZ = 'America/New_York';
    expect(formatLocalDateTime('2026-05-10T16:00:00Z')).toBe('10 May 2026, 12:00');
  });
});

describe('londonDayKey', () => {
  it('keys by the London calendar day, not the UTC one', () => {
    // 23:30 UTC on 19 Aug is 00:30 on 20 Aug in British Summer Time.
    expect(londonDayKey('2026-08-19T23:30:00Z')).toBe('2026-08-20');
    expect(londonDayKey('2026-08-19T12:00:00Z')).toBe('2026-08-19');
  });
});
