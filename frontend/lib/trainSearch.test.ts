import { describe, expect, it } from 'vitest';
import {
  clockMinutes,
  nextDateLabel,
  parseNextDayParam,
  parseOperatorParam,
  splitAtMidnight,
  upperBoundNextDay,
} from './trainSearch';

describe('parseOperatorParam', () => {
  it('trims, uppercases and drops repeats and anything that is not a code', () => {
    expect(parseOperatorParam(' sw,,GW, sw ,toolong,X')).toEqual(['SW', 'GW']);
    expect(parseOperatorParam(undefined)).toEqual([]);
    expect(parseOperatorParam('')).toEqual([]);
  });

  it('drops codes that are not known operators, given the list', () => {
    expect(parseOperatorParam('SW,ZZ,GW', new Set(['SW', 'GW']))).toEqual(['SW', 'GW']);
    // An empty list (it failed to load) filters nothing out.
    expect(parseOperatorParam('SW,ZZ', new Set())).toEqual(['SW', 'ZZ']);
  });
});

describe('parseNextDayParam', () => {
  it('is only on for 1', () => {
    expect(parseNextDayParam('1')).toBe(true);
    expect(parseNextDayParam('0')).toBe(false);
    expect(parseNextDayParam('yes')).toBe(false);
    expect(parseNextDayParam(null)).toBe(false);
  });
});

describe('clockMinutes', () => {
  it('reads HH:MM and nothing else', () => {
    expect(clockMinutes('01:30')).toBe(90);
    expect(clockMinutes('23:59')).toBe(1439);
    expect(clockMinutes('24:00')).toBeNull();
    expect(clockMinutes('')).toBeNull();
  });
});

describe('upperBoundNextDay', () => {
  it('is after midnight by necessity when earlier than the lower bound', () => {
    expect(upperBoundNextDay('01:30', false, '22:00', false)).toEqual({ nextDay: true, implied: true });
  });

  it('is the visitor’s choice otherwise', () => {
    expect(upperBoundNextDay('23:00', false, '22:00', false)).toEqual({ nextDay: false, implied: false });
    expect(upperBoundNextDay('23:00', true, '22:00', false)).toEqual({ nextDay: true, implied: false });
    expect(upperBoundNextDay('01:30', false, '', false)).toEqual({ nextDay: false, implied: false });
  });

  it('follows a lower bound that is itself after midnight', () => {
    expect(upperBoundNextDay('02:00', false, '00:30', true)).toEqual({ nextDay: true, implied: true });
  });
});

describe('splitAtMidnight', () => {
  it('splits at the first row after midnight', () => {
    const rows = [{ dayOffset: 0 }, {}, { dayOffset: 1 }, { dayOffset: 1 }];
    expect(splitAtMidnight(rows)).toEqual({ sameDay: rows.slice(0, 2), afterMidnight: rows.slice(2) });
    expect(splitAtMidnight([{ dayOffset: 0 }])).toEqual({ sameDay: [{ dayOffset: 0 }], afterMidnight: [] });
    expect(splitAtMidnight([{ dayOffset: 1 }])).toEqual({ sameDay: [], afterMidnight: [{ dayOffset: 1 }] });
  });
});

describe('nextDateLabel', () => {
  it('names the day after the searched date, across a month and a clock change', () => {
    expect(nextDateLabel('2026-10-23')).toBe('Sat 24 Oct');
    expect(nextDateLabel('2026-10-31')).toBe('Sun 1 Nov');
    expect(nextDateLabel('2026-10-24')).toBe('Sun 25 Oct');
  });
});
