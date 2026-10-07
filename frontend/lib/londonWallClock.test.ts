import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  addCalendarDays,
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

  // R-085: `.startOf('day')`/`.endOf('day')` on a zoned dayjs went through
  // the host zone, so on the clock-change days (and the days either side)
  // an America/* browser got bounds an hour off.
  describe.each(['America/Los_Angeles', 'America/New_York', 'Asia/Tokyo', 'Europe/London', 'UTC'])(
    'clock-change days with the host zone %s',
    (tz) => {
      it.each([
        // [day, start, end]
        ['2026-03-28', '2026-03-28T00:00:00.000Z', '2026-03-28T23:59:59.999Z'],
        ['2026-03-29', '2026-03-29T00:00:00.000Z', '2026-03-29T22:59:59.999Z'], // 23 hours
        ['2026-03-30', '2026-03-29T23:00:00.000Z', '2026-03-30T22:59:59.999Z'],
        ['2026-10-24', '2026-10-23T23:00:00.000Z', '2026-10-24T22:59:59.999Z'],
        ['2026-10-25', '2026-10-24T23:00:00.000Z', '2026-10-25T23:59:59.999Z'], // 25 hours
        ['2026-10-26', '2026-10-26T00:00:00.000Z', '2026-10-26T23:59:59.999Z'],
      ])('bounds %s', (day, start, end) => {
        process.env.TZ = tz;
        expect(londonDayStartIso(day)).toBe(start);
        expect(londonDayEndIso(day)).toBe(end);
        // And back: each bound falls on its own London day.
        expect(londonCalendarDay(start)).toBe(day);
        expect(londonCalendarDay(end)).toBe(day);
      });

      it('maps instants either side of the London midnights to the right day', () => {
        process.env.TZ = tz;
        expect(londonCalendarDay('2026-03-28T23:59:59.999Z')).toBe('2026-03-28');
        expect(londonCalendarDay('2026-03-29T00:00:00.000Z')).toBe('2026-03-29');
        expect(londonCalendarDay('2026-03-29T22:59:59.999Z')).toBe('2026-03-29');
        expect(londonCalendarDay('2026-03-29T23:00:00.000Z')).toBe('2026-03-30');
        expect(londonCalendarDay('2026-10-24T22:59:59.999Z')).toBe('2026-10-24');
        expect(londonCalendarDay('2026-10-24T23:00:00.000Z')).toBe('2026-10-25');
        expect(londonCalendarDay('2026-10-25T23:59:59.999Z')).toBe('2026-10-25');
        expect(londonCalendarDay('2026-10-26T00:00:00.000Z')).toBe('2026-10-26');
      });
    },
  );

  it('maps an instant to its London calendar day, round-tripping the bounds', () => {
    expect(londonCalendarDay('2026-05-09T23:00:00.000Z')).toBe('2026-05-10');
    expect(londonCalendarDay(londonDayEndIso('2026-05-10'))).toBe('2026-05-10');
    expect(londonCalendarDay('garbage-value-x')).toBe('garbage-va');
  });
});

describe.each(['America/Los_Angeles', 'Pacific/Auckland', 'Europe/London', 'UTC'])(
  'addCalendarDays with the host zone %s',
  (tz) => {
    it('moves by whole calendar days across clock changes and month/year ends', () => {
      process.env.TZ = tz;
      expect(addCalendarDays('2026-10-24', 1)).toBe('2026-10-25');
      expect(addCalendarDays('2026-10-25', 1)).toBe('2026-10-26');
      expect(addCalendarDays('2026-03-29', 1)).toBe('2026-03-30');
      expect(addCalendarDays('2026-09-30', 1)).toBe('2026-10-01');
      expect(addCalendarDays('2026-09-05', 0)).toBe('2026-09-05');
      expect(addCalendarDays('2027-01-01', -1)).toBe('2026-12-31');
    });
  },
);
