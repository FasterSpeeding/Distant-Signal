import { describe, it, expect } from 'vitest';
import { collapseHistory, groupSpansByDay, resolveRange } from './history';
import type { LineStatusHistoryEntry } from './types';

function entry(computedAt: string, statuses: Array<[number, string]>): LineStatusHistoryEntry {
  return {
    $type: 'NRStatus.LineStatusReport',
    id: 'northern',
    name: 'Northern',
    modeName: 'national-rail',
    operators: ['NT'],
    computedAt,
    lineStatuses: statuses.map(([statusSeverity, reason]) => ({
      statusSeverity,
      statusSeverityDescription: 'x',
      reason,
      dataQuality: 'ldbws-inferred' as const,
      validityPeriods: [],
    })),
  };
}

describe('collapseHistory', () => {
  it('collapses consecutive recomputes carrying identical statuses into one span', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:10:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:20:00Z', [[9, 'Minor delays']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].samples).toBe(3);
    expect(spans[0].from).toBe('2026-08-19T18:00:00Z');
    expect(spans[0].to).toBe('2026-08-19T18:20:00Z');
  });

  it('starts a new span when the status set changes', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[10, 'Good Service']]),
      entry('2026-08-19T18:10:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:20:00Z', [[10, 'Good Service']]),
    ]);
    expect(spans).toHaveLength(3);
  });

  it('ignores the order statuses happen to arrive in within one entry', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[9, 'A'], [4, 'B']]),
      entry('2026-08-19T18:10:00Z', [[4, 'B'], [9, 'A']]),
    ]);
    expect(spans).toHaveLength(1);
  });

  it('sorts oldest-first before collapsing, so a span is always a real contiguous run', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:20:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].from).toBe('2026-08-19T18:00:00Z');
  });

  it('reports the worst severity in the span, by true rank', () => {
    // 4 (Planned Closure) is numerically lower but less severe than 6.
    const spans = collapseHistory([entry('2026-08-19T18:00:00Z', [[4, 'A'], [6, 'B']])]);
    expect(spans[0].severity).toBe(6);
  });

  it('returns nothing for no entries', () => {
    expect(collapseHistory([])).toEqual([]);
  });
});

describe('groupSpansByDay', () => {
  it('groups by London day, newest day first and newest span first within a day', () => {
    const spans = collapseHistory([
      entry('2026-08-19T10:00:00Z', [[10, 'Good Service']]),
      entry('2026-08-19T11:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-20T10:00:00Z', [[10, 'Good Service']]),
    ]);
    const days = groupSpansByDay(spans);
    expect(days.map((d) => d.day)).toEqual(['2026-08-20', '2026-08-19']);
    expect(days[1].spans[0].from).toBe('2026-08-19T11:00:00Z');
  });
});

describe('resolveRange', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');

  it('defaults to the last 7 days when nothing is in the URL', () => {
    const range = resolveRange({}, NOW);
    expect(range.preset).toBe('7d');
    expect(Date.parse(range.to) - Date.parse(range.from)).toBe(7 * 86400000);
  });

  it('honours the 30-day preset', () => {
    expect(resolveRange({ range: '30d' }, NOW).preset).toBe('30d');
  });

  it('honours an explicit custom range and reports no preset', () => {
    const range = resolveRange(
      { from: '2026-08-01T00:00:00Z', to: '2026-08-05T00:00:00Z' },
      NOW,
    );
    expect(range.preset).toBeNull();
    expect(range.from).toBe('2026-08-01T00:00:00Z');
  });

  it('falls back to the default rather than erroring on junk', () => {
    expect(resolveRange({ from: 'nonsense', to: 'also nonsense' }, NOW).preset).toBe('7d');
    expect(resolveRange({ range: 'forever' }, NOW).preset).toBe('7d');
  });

  it('ignores a half-specified custom range', () => {
    expect(resolveRange({ from: '2026-08-01T00:00:00Z' }, NOW).preset).toBe('7d');
  });
});
