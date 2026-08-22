import { describe, it, expect } from 'vitest';
import { cancelledPercent, firstSampleStats, formatSampleSummary } from './sampleStats';
import type { LineStatus, SampleStats } from './types';

const stats: SampleStats = { total: 160, delayed: 142, cancelled: 8, skipped: 1, avgDelayMinutes: 12.44 };

function status(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    ...overrides,
  };
}

describe('firstSampleStats', () => {
  it('returns undefined when nothing carries stats', () => {
    expect(firstSampleStats([status()])).toBeUndefined();
  });

  it('returns the first status that carries stats', () => {
    expect(firstSampleStats([status(), status({ sampleStats: stats })])).toBe(stats);
  });
});

describe('cancelledPercent', () => {
  it('rounds to a whole percentage', () => {
    expect(cancelledPercent(stats)).toBe(5);
  });

  it('returns null rather than dividing by zero on an empty sample', () => {
    expect(cancelledPercent({ ...stats, total: 0 })).toBeNull();
  });

  it('returns null for missing stats', () => {
    expect(cancelledPercent(undefined)).toBeNull();
  });
});

describe('formatSampleSummary', () => {
  it('renders the one-line summary used across cards, rows and tables', () => {
    expect(formatSampleSummary(stats)).toBe('Avg delay 12.4 min · 5% cancelled');
  });

  it('says so when there is no sample data', () => {
    expect(formatSampleSummary(undefined)).toBe('No sample data');
  });
});
