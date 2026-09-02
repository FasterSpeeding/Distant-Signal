import { describe, it, expect } from 'vitest';
import { groupHistoryByDay, resolveRange, resolveHourlyRange, retentionShortfallDays } from './history';
import type { LineStatusHistoryEntry } from './types';

function entry(computedAt: string, statuses: Array<[number, string]>): LineStatusHistoryEntry {
  return {
    $type: 'DistantSignal.LineStatusReport',
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
      sampleAvailability: { state: 'no-coverage' as const },
    })),
  };
}

function spansFor(entries: LineStatusHistoryEntry[]) {
  return groupHistoryByDay(entries).flatMap((day) => day.spans);
}

describe('groupHistoryByDay', () => {
  it('collapses consecutive recomputes carrying identical statuses into one span', () => {
    const spans = spansFor([
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:10:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:20:00Z', [[9, 'Minor delays']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].samples).toBe(3);
    expect(spans[0].from).toBe('2026-08-19T18:00:00Z');
    expect(spans[0].to).toBe('2026-08-19T18:20:00Z');
    expect(spans[0].flips).toHaveLength(1);
  });

  it('groups a flapping incident into one span spanning its full first-to-last-seen window', () => {
    // Same underlying incident, severity repeatedly crossing an escalation
    // threshold and dropping back — this is the "same Wrexham incident
    // scattered across a dozen alternating rows" case from production.
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [[9, 'Works in the area']]),
      entry('2026-08-19T12:00:00Z', [[6, 'Works in the area']]),
      entry('2026-08-19T13:00:00Z', [[9, 'Works in the area']]),
      entry('2026-08-19T14:00:00Z', [[6, 'Works in the area']]),
      entry('2026-08-19T15:00:00Z', [[9, 'Works in the area']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].from).toBe('2026-08-19T11:00:00Z');
    expect(spans[0].to).toBe('2026-08-19T15:00:00Z');
    expect(spans[0].samples).toBe(5);
    // 6 (Severe Delays) outranks 9 (Minor Delays).
    expect(spans[0].severity).toBe(6);
    expect(spans[0].flips).toHaveLength(5);
  });

  it('keeps two genuinely different, simultaneously ongoing incidents as separate spans', () => {
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [
        [9, 'Signal failure near Chester'],
        [6, 'Points failure near Crewe'],
      ]),
      entry('2026-08-19T11:10:00Z', [
        [9, 'Signal failure near Chester'],
        [6, 'Points failure near Crewe'],
      ]),
    ]);
    expect(spans).toHaveLength(2);
    expect(spans.map((s) => s.reason).sort()).toEqual(['Points failure near Crewe', 'Signal failure near Chester']);
  });

  it('does not let a live-sample-count annotation churn defeat grouping', () => {
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [[6, 'Works in the area (live samples show: 5 of 9 sampled services delayed.)']]),
      entry('2026-08-19T11:10:00Z', [[6, 'Works in the area (live samples show: 7 of 14 sampled services delayed.)']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].reason).toBe('Works in the area');
    expect(spans[0].samples).toBe(2);
  });

  it('starts a new span when the reason genuinely changes', () => {
    const spans = spansFor([
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:10:00Z', [[9, 'A different incident']]),
    ]);
    expect(spans).toHaveLength(2);
  });

  it('groups "no active status" recomputes into their own span', () => {
    const spans = spansFor([
      entry('2026-08-19T18:00:00Z', []),
      entry('2026-08-19T18:10:00Z', []),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].samples).toBe(2);
  });

  it('reports the worst severity in the span, by true rank', () => {
    // 4 (Planned Closure) is numerically lower but less severe than 6.
    const spans = spansFor([entry('2026-08-19T18:00:00Z', [[4, 'A'], [6, 'B']])]);
    const worst = spans.find((s) => s.reason === 'B');
    expect(worst?.severity).toBe(6);
  });

  it('returns nothing for no entries', () => {
    expect(groupHistoryByDay([])).toEqual([]);
  });

  it('groups by London day, newest day first and newest span first within a day', () => {
    const days = groupHistoryByDay([
      entry('2026-08-19T10:00:00Z', [[10, 'Good Service']]),
      entry('2026-08-19T11:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-20T10:00:00Z', [[10, 'Good Service']]),
    ]);
    expect(days.map((d) => d.day)).toEqual(['2026-08-20', '2026-08-19']);
    expect(days[1].spans[0].reason).toBe('Minor delays');
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

describe('resolveHourlyRange', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');

  it('resolves exactly a 24-hour window ending at now', () => {
    const range = resolveHourlyRange(NOW);
    expect(range.to).toBe(new Date(NOW).toISOString());
    expect(Date.parse(range.to) - Date.parse(range.from)).toBe(24 * 60 * 60 * 1000);
  });
});

describe('retentionShortfallDays', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');

  it('is null when retentionDays is unknown (fetch failed)', () => {
    const range = resolveRange({ range: '30d' }, NOW);
    expect(retentionShortfallDays(range, null, NOW)).toBeNull();
  });

  it('is null when the requested range fits entirely within retention', () => {
    const range = resolveRange({ range: '7d' }, NOW);
    expect(retentionShortfallDays(range, 7, NOW)).toBeNull();
    // A generous retention window covers the wider preset too.
    const wide = resolveRange({ range: '30d' }, NOW);
    expect(retentionShortfallDays(wide, 30, NOW)).toBeNull();
  });

  it('reports the exact shortfall for the 30-day preset against 7-day retention', () => {
    const range = resolveRange({ range: '30d' }, NOW);
    // 30 requested - 7 retained = 23 days that can never come back, matching
    // the "23 of the 30 requested days" framing this fix exists for.
    expect(retentionShortfallDays(range, 7, NOW)).toBe(23);
  });

  it('reports zero-shortfall (null) right at the retention boundary', () => {
    const range = resolveRange({ from: new Date(NOW - 7 * 86400000).toISOString(), to: new Date(NOW).toISOString() }, NOW);
    expect(retentionShortfallDays(range, 7, NOW)).toBeNull();
  });

  it('handles a custom range that starts before retention allows', () => {
    const range = resolveRange(
      { from: new Date(NOW - 14 * 86400000).toISOString(), to: new Date(NOW).toISOString() },
      NOW,
    );
    expect(retentionShortfallDays(range, 7, NOW)).toBe(7);
  });

  it('is null for an unparseable from value rather than throwing', () => {
    expect(retentionShortfallDays({ from: 'nonsense' }, 7, NOW)).toBeNull();
  });
});
