import { describe, expect, it } from 'vitest';
import {
  apiSearchQuery,
  clockMinutes,
  legacyTrainsParams,
  nextDateLabel,
  operatorChoices,
  parseBoardParam,
  parseNextDayParam,
  parseOperatorParam,
  serviceDateLabel,
  splitAtMidnight,
  splitSearchRows,
  trainsViewFor,
  uidTrainHref,
  upperBoundNextDay,
} from './trainSearch';

describe('legacyTrainsParams', () => {
  const legacy = (query: string) => legacyTrainsParams(new URLSearchParams(query))?.toString() ?? null;

  it('leaves a current query string alone', () => {
    expect(legacy('')).toBeNull();
    expect(legacy('station=RDG&time=09%3A00&until=10%3A00&date=2026-10-30')).toBeNull();
    expect(legacy('from=RDG&to=PAD')).toBeNull();
    expect(legacy('headcode=1S00')).toBeNull();
  });

  it('renames the time window and its day offsets', () => {
    expect(legacy('station=ZFX&date=2099-01-05&from=22:00&to=01:30&operator=CC')).toBe(
      'station=ZFX&date=2099-01-05&time=22%3A00&until=01%3A30&operator=CC',
    );
    expect(legacy('station=MAN&from=00:30&from_day_offset=1&to_day_offset=1&to=02:00')).toBe(
      'station=MAN&time=00%3A30&time_day_offset=1&until_day_offset=1&until=02%3A00',
    );
  });

  it('turns station and stops_at into the between-stations view, keeping everything else', () => {
    expect(legacy('station=man&origin=EUS&stops_at=wat&arrival_from=10:00&ticketId=7')).toBe(
      'origin=EUS&arrival_from=10%3A00&ticketId=7&from=man&to=wat',
    );
    expect(legacy('stops_at=WAT')).toBe('to=WAT');
  });
});

describe('trainsViewFor', () => {
  it('reads the view from the params', () => {
    expect(trainsViewFor(new URLSearchParams(''))).toBe('station');
    expect(trainsViewFor(new URLSearchParams('station=RDG&board=arrivals'))).toBe('station');
    expect(trainsViewFor(new URLSearchParams('from=RDG&to=PAD'))).toBe('between');
    expect(trainsViewFor(new URLSearchParams('to=PAD'))).toBe('between');
    expect(trainsViewFor(new URLSearchParams('headcode=1S00'))).toBe('service');
    expect(trainsViewFor(new URLSearchParams('uid=W12345&date=2026-10-30'))).toBe('service');
  });
});

describe('parseBoardParam', () => {
  it('is departures unless it says arrivals', () => {
    expect(parseBoardParam('arrivals')).toBe('arrivals');
    expect(parseBoardParam(' Arrivals ')).toBe('arrivals');
    expect(parseBoardParam('departures')).toBe('departures');
    expect(parseBoardParam(null)).toBe('departures');
    expect(parseBoardParam('junk')).toBe('departures');
  });
});

describe('uidTrainHref', () => {
  it('links a valid UID to its train page, on the date given or today', () => {
    expect(uidTrainHref(new URLSearchParams('uid=w12345&date=2026-10-31'), '2026-10-30')).toBe(
      '/train/W12345/2026-10-31',
    );
    expect(uidTrainHref(new URLSearchParams('uid=W12345'), '2026-10-30')).toBe('/train/W12345/2026-10-30');
    expect(uidTrainHref(new URLSearchParams('uid=W12345&date=tomorrow'), '2026-10-30')).toBe(
      '/train/W12345/2026-10-30',
    );
    expect(uidTrainHref(new URLSearchParams('uid=1S00'), '2026-10-30')).toBeNull();
    expect(uidTrainHref(new URLSearchParams(''), '2026-10-30')).toBeNull();
  });
});

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

describe('apiSearchQuery', () => {
  it("asks for yesterday's late trains only when no date is picked", () => {
    expect(apiSearchQuery('station=MAN&from=00%3A15')).toBe('station=MAN&from=00%3A15&across_midnight=true');
    expect(apiSearchQuery('station=MAN&date=')).toBe('station=MAN&date=&across_midnight=true');
    expect(apiSearchQuery('station=MAN&date=2026-10-23')).toBe('station=MAN&date=2026-10-23');
  });
});

describe('splitSearchRows', () => {
  it("puts the previous date's rows first, then splits the searched date's at its midnight", () => {
    const rows = [
      { uid: 'A', dayOffset: 1, serviceDate: '2026-10-23' },
      { uid: 'B', dayOffset: 0, serviceDate: '2026-10-24' },
      { uid: 'C', dayOffset: 1, serviceDate: '2026-10-23' },
      { uid: 'D', dayOffset: 1, serviceDate: '2026-10-24' },
      // An older backend sends no `serviceDate`: the searched date's.
      { uid: 'E', dayOffset: 1 },
    ];
    const split = splitSearchRows(rows, '2026-10-24');
    expect(split.previousDay.map((row) => row.uid)).toEqual(['A', 'C']);
    expect(split.sameDay.map((row) => row.uid)).toEqual(['B']);
    expect(split.afterMidnight.map((row) => row.uid)).toEqual(['D', 'E']);
  });
});

describe('serviceDateLabel', () => {
  it('names the date itself', () => {
    expect(serviceDateLabel('2026-10-23')).toBe('Fri 23 Oct');
    expect(serviceDateLabel('2026-10-25')).toBe('Sun 25 Oct');
  });
});

describe('operatorChoices', () => {
  it('lists named operators by name, then unnamed codes by code', () => {
    expect(
      operatorChoices({ SW: 'South Western Railway', GW: 'Great Western Railway' }, ['TW', 'QC', 'SW', 'TW']),
    ).toEqual([
      { value: 'GW', label: 'GW · Great Western Railway' },
      { value: 'SW', label: 'SW · South Western Railway' },
      { value: 'QC', label: 'QC' },
      { value: 'TW', label: 'TW' },
    ]);
  });
});
