import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  londonCalendarDay,
  londonDayEndIso,
  londonDayStartIso,
  londonToday,
  londonWallClockToUtc,
  nowInLondon,
} from './londonWallClock';

// 2026-09-26 "Repeater Signal" review, finding M8. Each case that depends on
// the process zone flips `process.env.TZ` (same technique, and same
// delete-not-assign-undefined restore, as `lib/dateFormat.test.ts`).
const originalTz = process.env.TZ;
afterEach(() => {
  if (originalTz === undefined) {
    delete process.env.TZ;
  } else {
    process.env.TZ = originalTz;
  }
  vi.useRealTimers();
});

describe('londonWallClockToUtc', () => {
  it('resolves a BST wall-clock time to UTC+1', () => {
    expect(londonWallClockToUtc('2026-09-05 10:00:00').toISOString()).toBe('2026-09-05T09:00:00.000Z');
  });

  it('resolves a GMT wall-clock time to UTC+0', () => {
    expect(londonWallClockToUtc('2026-01-15 10:00:00').toISOString()).toBe('2026-01-15T10:00:00.000Z');
  });

  it('accepts the T-separated form too', () => {
    expect(londonWallClockToUtc('2026-09-05T10:00:00').toISOString()).toBe('2026-09-05T09:00:00.000Z');
  });

  it('is independent of the host zone -- a bare `new Date(...)` is not', () => {
    process.env.TZ = 'Etc/GMT-2'; // UTC+2
    expect(new Date('2026-09-05T10:00:00').toISOString()).toBe('2026-09-05T08:00:00.000Z');
    expect(londonWallClockToUtc('2026-09-05 10:00:00').toISOString()).toBe('2026-09-05T09:00:00.000Z');
  });
});

describe('nowInLondon', () => {
  it("reads the London calendar day and time, not the host zone's", () => {
    process.env.TZ = 'Asia/Tokyo'; // UTC+9
    vi.useFakeTimers();
    // 16:30Z: 17:30 on 5 Sep in London (BST), 01:30 on 6 Sep in Tokyo.
    vi.setSystemTime(new Date('2026-09-05T16:30:00.000Z'));
    expect(nowInLondon().format('YYYY-MM-DD HH:mm:ss')).toBe('2026-09-05 17:30:00');
    expect(nowInLondon().add(1, 'day').format('YYYY-MM-DD')).toBe('2026-09-06');
  });

  it('crosses into the next London day at London midnight, not UTC midnight', () => {
    vi.useFakeTimers();
    // 23:30Z on 5 Sep is 00:30 on 6 Sep in BST.
    vi.setSystemTime(new Date('2026-09-05T23:30:00.000Z'));
    expect(nowInLondon().format('YYYY-MM-DD')).toBe('2026-09-06');
  });
});

describe('londonToday', () => {
  it("is London's calendar day when the host zone has already rolled over", () => {
    process.env.TZ = 'Asia/Tokyo'; // UTC+9
    vi.useFakeTimers();
    // 16:30Z: 5 Sep in London, already 6 Sep in Tokyo.
    vi.setSystemTime(new Date('2026-09-05T16:30:00.000Z'));
    expect(londonToday()).toBe('2026-09-05');
  });

  it("is London's calendar day when the host zone is still on the previous day", () => {
    process.env.TZ = 'America/New_York'; // UTC-4 in September
    vi.useFakeTimers();
    // 23:30Z: 00:30 on 6 Sep in London, 19:30 on 5 Sep in New York.
    vi.setSystemTime(new Date('2026-09-05T23:30:00.000Z'));
    expect(londonToday()).toBe('2026-09-06');
  });
});

// FE-5: date-only filters are London days, not UTC days.
describe('London day bounds', () => {
  it('starts and ends a BST day at London midnight', () => {
    expect(londonDayStartIso('2026-05-10')).toBe('2026-05-09T23:00:00.000Z');
    expect(londonDayEndIso('2026-05-10')).toBe('2026-05-10T22:59:59.999Z');
  });

  it('uses UTC+0 in GMT', () => {
    expect(londonDayStartIso('2026-01-10')).toBe('2026-01-10T00:00:00.000Z');
    expect(londonDayEndIso('2026-01-10')).toBe('2026-01-10T23:59:59.999Z');
  });

  it('handles the 23-hour and 25-hour clock-change days', () => {
    expect(londonDayStartIso('2026-03-29')).toBe('2026-03-29T00:00:00.000Z');
    expect(londonDayEndIso('2026-03-29')).toBe('2026-03-29T22:59:59.999Z');
    expect(londonDayStartIso('2026-10-25')).toBe('2026-10-24T23:00:00.000Z');
    expect(londonDayEndIso('2026-10-25')).toBe('2026-10-25T23:59:59.999Z');
  });

  it('is independent of the host zone', () => {
    process.env.TZ = 'America/Los_Angeles';
    expect(londonDayStartIso('2026-05-10')).toBe('2026-05-09T23:00:00.000Z');
    expect(londonCalendarDay('2026-05-09T23:30:00Z')).toBe('2026-05-10');
  });

  it('maps an instant to its London calendar day, round-tripping the bounds', () => {
    expect(londonCalendarDay('2026-05-09T23:00:00.000Z')).toBe('2026-05-10');
    expect(londonCalendarDay(londonDayEndIso('2026-05-10'))).toBe('2026-05-10');
    expect(londonCalendarDay('garbage-value-x')).toBe('garbage-va');
  });
});
