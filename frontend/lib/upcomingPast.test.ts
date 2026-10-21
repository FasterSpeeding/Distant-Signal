import { describe, expect, it } from 'vitest';
import { splitByStatus, splitTravelOrdered } from './upcomingPast';

const TODAY = '2026-10-21';

/** A row as the cut-off reads it: its start day and whether it says it is over. */
interface Row {
  name: string;
  startDay: string;
  over: boolean;
}

function row(name: string, startDay: string, over = false): Row {
  return { name, startDay, over };
}

function split(rows: Row[]) {
  const { upcoming, past } = splitTravelOrdered(rows, (r) => r, TODAY);
  return { upcoming: upcoming.map((r) => r.name), past: past.map((r) => r.name) };
}

describe('splitTravelOrdered', () => {
  it('splits at the first row that is over, keeping the order', () => {
    expect(
      split([row('a', TODAY), row('b', '2026-10-22'), row('c', TODAY, true), row('d', '2026-10-19', true)]),
    ).toEqual({ upcoming: ['a', 'b'], past: ['c', 'd'] });
  });

  it('puts every row after a past one in Past, whatever it says itself', () => {
    expect(split([row('a', TODAY), row('b', '2026-10-20', true), row('c', '2026-10-20')])).toEqual({
      upcoming: ['a'],
      past: ['b', 'c'],
    });
  });

  it('cuts where a row starts on an earlier day than the one before it', () => {
    expect(split([row('a', TODAY), row('b', '2026-10-25'), row('c', TODAY), row('d', '2026-10-18')])).toEqual({
      upcoming: ['a', 'b'],
      past: ['c', 'd'],
    });
  });

  it('keeps every row up to the last one starting after today upcoming', () => {
    expect(split([row('a', '2026-10-19', true), row('b', '2026-10-30'), row('c', '2026-10-01', true)])).toEqual({
      upcoming: ['a', 'b'],
      past: ['c'],
    });
  });

  it('handles all upcoming, all past and no rows', () => {
    expect(split([row('a', TODAY), row('b', '2026-10-22')])).toEqual({ upcoming: ['a', 'b'], past: [] });
    expect(split([row('a', '2026-10-20', true), row('b', '2026-10-01', true)])).toEqual({
      upcoming: [],
      past: ['a', 'b'],
    });
    expect(split([])).toEqual({ upcoming: [], past: [] });
  });
});

describe('splitByStatus', () => {
  it('places each row by its own status, keeping the order', () => {
    const rows = [row('a', TODAY, true), row('b', TODAY), row('c', '2026-10-01', true), row('d', '2026-10-01')];
    const { upcoming, past } = splitByStatus(rows, (r) => r.over);
    expect(upcoming.map((r) => r.name)).toEqual(['b', 'd']);
    expect(past.map((r) => r.name)).toEqual(['a', 'c']);
  });
});
