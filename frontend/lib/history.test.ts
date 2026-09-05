import { describe, it, expect } from 'vitest';
import {
  groupHistoryByDay,
  resolveRange,
  resolveHalfHourlyRange,
  retentionShortfallDays,
  availableGranularities,
  resolveGranularity,
  granularityShortfallDays,
} from './history';
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
      fullCoverageAvailability: { state: 'not-enabled' },
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

  it('does not let embedded live sample counts defeat grouping of an ongoing sample-inferred situation', () => {
    // The write-side/read-side regression this fix exists for: pure count
    // wobble on `classify()`'s baked-in "N of M sampled services ..."
    // text, same most-cited cause, must collapse into one span.
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [[9, '5 of 9 sampled services delayed. (most cited: Signal failure)']]),
      entry('2026-08-19T11:10:00Z', [[9, '7 of 14 sampled services delayed. (most cited: Signal failure)']]),
      entry('2026-08-19T11:20:00Z', [[9, '2 of 3 sampled services delayed. (most cited: Signal failure)']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].reason).toBe('N of M sampled services delayed. (most cited: Signal failure)');
    expect(spans[0].samples).toBe(3);
  });

  it('starts a new span when the most-cited cause genuinely changes, even though counts also fluctuate', () => {
    // Design decision: the count fluctuating minute-to-minute is noise, but
    // a genuine change in the most-cited reported cause (e.g. Signal
    // failure -> Engineering works) is real information worth a new entry,
    // so it must NOT collapse together with the prior cause's span.
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [[9, '5 of 9 sampled services delayed. (most cited: Signal failure)']]),
      entry('2026-08-19T11:10:00Z', [[9, '7 of 14 sampled services delayed. (most cited: Engineering works)']]),
    ]);
    expect(spans).toHaveLength(2);
    expect(spans.map((s) => s.reason).sort()).toEqual([
      'N of M sampled services delayed. (most cited: Engineering works)',
      'N of M sampled services delayed. (most cited: Signal failure)',
    ]);
  });

  it('normalizes both clauses of a combined delay+skip sample-inferred reason', () => {
    const spans = spansFor([
      entry('2026-08-19T11:00:00Z', [
        [6, '5 of 9 sampled services delayed, 3 of 9 sampled services skipping a scheduled stop.'],
      ]),
      entry('2026-08-19T11:10:00Z', [
        [6, '7 of 14 sampled services delayed, 4 of 14 sampled services skipping a scheduled stop.'],
      ]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].reason).toBe(
      'N of M sampled services delayed, N of M sampled services skipping a scheduled stop.',
    );
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

describe('resolveHalfHourlyRange', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');

  it('resolves exactly a 24-hour window ending at now', () => {
    const range = resolveHalfHourlyRange(NOW);
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

describe('availableGranularities', () => {
  const GENEROUS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };

  it('offers all four tiers for a narrow (12-hour) range', () => {
    expect(availableGranularities(12 * 3_600_000, GENEROUS)).toEqual(['halfHour', 'hour', 'sixHour', 'day']);
  });

  it('excludes half-hourly and hourly (point budget) but keeps six-hourly and daily for a 10-day range', () => {
    // 10 days: halfHour -> 480 points, hour -> 240 points (both over 200);
    // sixHour -> 40 points (under 200); all three are still within the
    // 840-hour (35-day) retention ceiling, so only the point budget excludes them.
    const tenDaysMs = 10 * 86_400_000;
    expect(availableGranularities(tenDaysMs, GENEROUS)).toEqual(['sixHour', 'day']);
  });

  it('excludes every sub-daily tier (retention) for a range wider than the shared 35-day ceiling, leaving only day', () => {
    const fortyDaysMs = 40 * 86_400_000;
    expect(availableGranularities(fortyDaysMs, GENEROUS)).toEqual(['day']);
  });

  it('never returns an empty array, even with zero ceilings', () => {
    expect(
      availableGranularities(365 * 86_400_000, { dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 }),
    ).toEqual(['day']);
  });
});

describe('resolveGranularity', () => {
  const GENEROUS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };
  const ONE_DAY_MS = 86_400_000;

  it('defaults to day when unset', () => {
    expect(resolveGranularity({}, ONE_DAY_MS, GENEROUS)).toBe('day');
  });

  it('falls back to day for junk input', () => {
    expect(resolveGranularity({ granularity: 'fortnightly' }, ONE_DAY_MS, GENEROUS)).toBe('day');
  });

  it('honours a requested tier that is available', () => {
    expect(resolveGranularity({ granularity: 'hour' }, ONE_DAY_MS, GENEROUS)).toBe('hour');
  });

  it('falls back to the next coarser available tier when the requested one is not available', () => {
    // 10 days: hour is unavailable (point budget), sixHour is the next coarser available tier.
    const tenDaysMs = 10 * 86_400_000;
    expect(resolveGranularity({ granularity: 'hour' }, tenDaysMs, GENEROUS)).toBe('sixHour');
  });

  it('falls all the way back to day when nothing finer is available', () => {
    const fortyDaysMs = 40 * 86_400_000;
    expect(resolveGranularity({ granularity: 'halfHour' }, fortyDaysMs, GENEROUS)).toBe('day');
  });
});

describe('granularityShortfallDays', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');
  const CEILINGS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };

  it('is null for the day tier when the range fits within dailyStatsRetentionDays', () => {
    const range = { from: new Date(NOW - 30 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'day', CEILINGS, NOW)).toBeNull();
  });

  it('reports the shortfall for the day tier when the range exceeds dailyStatsRetentionDays', () => {
    const range = { from: new Date(NOW - 310 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'day', CEILINGS, NOW)).toBe(10);
  });

  it('is null for a sub-daily tier when the range fits within the hours-derived ceiling', () => {
    const range = { from: new Date(NOW - 30 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'hour', CEILINGS, NOW)).toBeNull();
  });

  it('reports the shortfall for a sub-daily tier converted from hours to days', () => {
    // 840 hours = 35 days; a 40-day-old range is 5 days beyond that.
    const range = { from: new Date(NOW - 40 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'halfHour', CEILINGS, NOW)).toBe(5);
  });
});
