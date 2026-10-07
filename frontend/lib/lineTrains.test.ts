import { describe, expect, it } from 'vitest';
import {
  directionLabel,
  directionTabs,
  formatApiMinute,
  formatClock,
  frequencySummary,
  groupPatterns,
  groupShared,
  hubStations,
  lineHref,
  londonMinuteOfDay,
  paramsForHref,
  parseLinePageParams,
  parseMinute,
  resolveWindow,
  sortByLineTime,
  splitRunning,
  stopStrip,
} from './lineTrains';
import { STATIONS, train } from '@/test/lineTrainsFixtures';

describe('times', () => {
  it('parses HH:MM up to 47:59 and rejects the rest', () => {
    expect(parseMinute('00:00')).toBe(0);
    expect(parseMinute('13:05')).toBe(785);
    expect(parseMinute('25:30')).toBe(1530);
    for (const bad of ['48:00', '7:00', '07:60', '', undefined, null, 'aa:bb']) {
      expect(parseMinute(bad)).toBeNull();
    }
  });

  it('formats API minutes past midnight and clock times within a day', () => {
    expect(formatApiMinute(1530)).toBe('25:30');
    expect(formatApiMinute(-5)).toBe('00:00');
    expect(formatClock(1530)).toBe('01:30');
    expect(formatClock(785)).toBe('13:05');
  });

  it('reads the London wall clock, not UTC', () => {
    // 12:00Z in July is 13:00 BST.
    expect(londonMinuteOfDay(new Date('2026-07-01T12:00:00Z'))).toBe(13 * 60);
    expect(londonMinuteOfDay(new Date('2026-12-01T12:00:00Z'))).toBe(12 * 60);
  });
});

describe('resolveWindow', () => {
  it('is now-30 to now+2h on a desktop and now+1h on a phone, with Earlier/Later steps', () => {
    const w = resolveWindow(14 * 60);
    expect([w.from, w.to, w.phoneTo].map(formatApiMinute)).toEqual(['13:30', '16:00', '15:00']);
    expect([w.earlierDesktop, w.laterDesktop].map(formatApiMinute)).toEqual(['12:00', '16:00']);
    expect([w.earlierPhone, w.laterPhone].map(formatApiMinute)).toEqual(['13:00', '15:00']);
  });

  it('clamps at the start of the day and runs into the next morning', () => {
    const early = resolveWindow(10);
    expect(early.from).toBe(0);
    expect(early.earlierDesktop).toBe(0);
    const late = resolveWindow(23 * 60 + 30);
    expect(formatApiMinute(late.to)).toBe('25:30');
    expect(formatApiMinute(late.laterDesktop)).toBe('25:30');
  });
});

describe('URL params', () => {
  it('keeps only well-formed values', () => {
    expect(parseLinePageParams({ dir: 'up', at: '17:00', from: 'wok', to: 'WAT', view: 'routes' })).toEqual({
      dir: 'up',
      at: 1020,
      from: 'WOK',
      to: 'WAT',
      view: 'routes',
    });
    expect(parseLinePageParams({ dir: 'north', at: '7pm', from: 'WOKING', to: ['WAT', 'X'], view: 'x' })).toEqual({
      dir: null,
      at: null,
      from: null,
      to: 'WAT',
      view: null,
    });
  });

  it('builds links that keep the other parameters and land on the section', () => {
    const params = parseLinePageParams({ dir: 'up', at: '17:00' });
    expect(lineHref('swr-south-west-main', { ...paramsForHref(params), at: '19:00' })).toBe(
      '/lines/swr-south-west-main?dir=up&at=19%3A00#trains',
    );
    expect(lineHref('swr-south-west-main', { ...paramsForHref(params), dir: null, at: null })).toBe(
      '/lines/swr-south-west-main#trains',
    );
  });
});

describe('sorting and running', () => {
  it('sorts by time on the line, a next-morning train last', () => {
    const sorted = sortByLineTime([
      train({ uid: 'C', lineDue: { time: '00:20', dayOffset: 1 } }),
      train({ uid: 'A', lineDue: { time: '23:50', dayOffset: 0 } }),
      train({ uid: 'B', lineDue: null }),
      train({ uid: 'D', lineDue: { time: '06:00', dayOffset: 0 } }),
    ]);
    expect(sorted.map((t) => t.uid)).toEqual(['D', 'A', 'C', 'B']);
  });

  it('lists a running train once, in Running now, and never a shared one there', () => {
    const a = train({ uid: 'A' });
    const b = train({ uid: 'B' });
    const s = train({ uid: 'S', scope: 'shared' });
    const split = splitRunning([a, b], [a, s]);
    expect(split.running.map((t) => t.uid)).toEqual(['A']);
    expect(split.upcoming.map((t) => t.uid)).toEqual(['B']);
    expect(splitRunning([a], null).upcoming).toEqual([a]);
  });
});

describe('direction tabs', () => {
  it('labels up and down by terminus and adds Loop only when loop trains run', () => {
    expect(directionLabel('up', STATIONS)).toBe('Towards London Waterloo');
    expect(directionLabel('down', STATIONS)).toBe('Towards Weymouth');
    const tabs = directionTabs({ line: { up: 3, down: 4 }, shared: { up: 9 } }, STATIONS, true);
    expect(tabs.map((t) => [t.dir, t.label, t.count])).toEqual([
      [null, 'All', 7],
      ['up', 'Towards London Waterloo', 3],
      ['down', 'Towards Weymouth', 4],
    ]);
    const loop = directionTabs({ line: { loop: 2 } }, STATIONS, true);
    expect(loop.map((t) => t.label)).toContain('Loop');
    expect(directionTabs({ unknown: { none: 5 } }, STATIONS, false)).toEqual([]);
  });
});

describe('shared group and hubs', () => {
  it('groups shared trains by operator and route, in time order', () => {
    const groups = groupShared(
      [
        train({
          uid: 'X2',
          operator: 'XC',
          lineDue: { time: '09:12', dayOffset: 0 },
          origin: { crs: 'MAN', name: 'Manchester Piccadilly' },
          destination: { crs: 'BMH', name: 'Bournemouth' },
        }),
        train({
          uid: 'G1',
          operator: 'GW',
          lineDue: { time: '08:40', dayOffset: 0 },
          origin: { crs: 'CDF', name: 'Cardiff Central' },
          destination: { crs: 'POR', name: 'Portsmouth Harbour' },
        }),
        train({
          uid: 'X1',
          operator: 'XC',
          lineDue: { time: '08:12', dayOffset: 0 },
          origin: { crs: 'MAN', name: 'Manchester Piccadilly' },
          destination: { crs: 'BMH', name: 'Bournemouth' },
        }),
      ],
      (code) => ({ XC: 'CrossCountry', GW: 'Great Western Railway' })[code] ?? code,
    );
    expect(groups.map((g) => [g.operator, g.route, g.trains.map((t) => t.uid)])).toEqual([
      ['CrossCountry', 'Manchester Piccadilly → Bournemouth', ['X1', 'X2']],
      ['Great Western Railway', 'Cardiff Central → Portsmouth Harbour', ['G1']],
    ]);
  });

  it('takes termini and junctions as hubs', () => {
    expect(hubStations(STATIONS).map((s) => s.crs)).toEqual(['WAT', 'CLJ', 'WOK', 'BSK', 'WEY']);
    expect(hubStations([{ crs: 'AAA', name: 'A', role: 'minor' }]).map((s) => s.crs)).toEqual(['AAA']);
  });
});

describe('stop strip', () => {
  it('shows key stations after the first, counts the rest, and names them all in text', () => {
    const strip = stopStrip(
      train({
        uid: 'A',
        onLineStops: ['WAT', 'CLJ', 'WOK', 'WIN', 'SOA', 'SOU', 'WEY'].map((crs) => ({
          crs,
          time: '08:00',
          dayOffset: 0,
        })),
      }),
      STATIONS,
    );
    expect(strip.shown).toEqual(['Winchester', 'Southampton Central', 'Weymouth']);
    expect(strip.hidden).toBe(3);
    expect(strip.text).toBe(
      'Then calls at Clapham Junction, Woking, Winchester, Southampton Airport Parkway, Southampton Central and Weymouth',
    );
  });

  it('falls back to the last stop and says when there is nothing further', () => {
    const strip = stopStrip(
      train({ uid: 'A', onLineStops: ['WAT', 'CLJ', 'WOK'].map((crs) => ({ crs, time: '08:00', dayOffset: 0 })) }),
      STATIONS,
    );
    expect(strip.shown).toEqual(['Woking']);
    expect(strip.hidden).toBe(1);
    expect(stopStrip(train({ uid: 'B', onLineStops: [] }), STATIONS).text).toBe('No further stops on this line');
  });
});

describe('pattern grouping', () => {
  it('summarises a regular interval with its minutes past the hour', () => {
    expect(frequencySummary([485, 515, 545, 575])).toBe('every 30 min · xx:05, xx:35');
    expect(frequencySummary([480, 495, 510])).toBe('every 15 min · xx:00, xx:15, xx:30');
    // A minute's drift reads as one slot.
    expect(frequencySummary([993, 1022, 1053, 1082])).toBe('every 30 min · xx:02, xx:33');
    expect(frequencySummary([480, 520, 560])).toBe('every 40 min');
    expect(frequencySummary([480, 500, 560])).toBe('3 trains');
    expect(frequencySummary([480, 510])).toBe('2 trains');
    expect(frequencySummary([480])).toBe('1 train');
  });

  it('groups by stopping pattern and tells fast from stopping on the same route', () => {
    const stops = (crs: string[]) => crs.map((c) => ({ crs: c, time: '08:00', dayOffset: 0 }));
    const fast = (uid: string, time: string) =>
      train({ uid, lineDue: { time, dayOffset: 0 }, onLineStops: stops(['WAT', 'WOK', 'WEY']) });
    const slow = (uid: string, time: string) =>
      train({ uid, lineDue: { time, dayOffset: 0 }, onLineStops: stops(['WAT', 'CLJ', 'WOK', 'BSK', 'WEY']) });
    const groups = groupPatterns([
      fast('F1', '08:05'),
      slow('S1', '08:20'),
      fast('F2', '08:35'),
      fast('F3', '09:05'),
      train({
        uid: 'O',
        destination: { crs: 'SOU', name: 'Southampton Central' },
        lineDue: { time: '08:50', dayOffset: 0 },
      }),
    ]);
    expect(groups.map((g) => [g.route, g.kind, g.frequency, g.trains.length])).toEqual([
      ['London Waterloo → Weymouth', 'fast', 'every 30 min · xx:05, xx:35', 3],
      ['London Waterloo → Weymouth', 'stopping', '1 train', 1],
      ['London Waterloo → Southampton Central', null, '1 train', 1],
    ]);
  });
});
