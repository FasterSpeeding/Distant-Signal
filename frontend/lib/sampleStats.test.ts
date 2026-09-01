import { describe, it, expect } from 'vitest';
import { cancelledPercent, firstSampleStats, formatSampleSummary, representativeStatus, sampleUnavailableReason } from './sampleStats';
import type { LineStatus, SampleStats } from './types';

const stats: SampleStats = { total: 160, delayed: 142, cancelled: 8, skipped: 1, avgDelayMinutes: 12.44 };

function status(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
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

describe('representativeStatus', () => {
  it('returns the first status carrying real stats when any exists', () => {
    const withStats = status({ sampleStats: stats });
    expect(representativeStatus([status(), withStats])).toBe(withStats);
  });

  it('falls back to the first status overall when none carries stats', () => {
    const first = status({ reason: 'first' });
    expect(representativeStatus([first, status({ reason: 'second' })])).toBe(first);
  });

  it('returns undefined only for an empty array', () => {
    expect(representativeStatus([])).toBeUndefined();
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

describe('sampleUnavailableReason', () => {
  it('returns null when sampleStats is present', () => {
    expect(sampleUnavailableReason(status({ sampleStats: stats }))).toBeNull();
  });

  it('returns the TfL copy when dataQuality is tfl, regardless of sampleAvailability', () => {
    const tflStatus = status({ dataQuality: 'tfl', sampleAvailability: { state: 'below-threshold', observed: 0, required: 1 } });
    expect(sampleUnavailableReason(tflStatus)).toBe("Not measured by this app — status is TfL's own.");
  });

  it('returns the no-coverage copy', () => {
    expect(sampleUnavailableReason(status({ sampleAvailability: { state: 'no-coverage' } }))).toBe(
      'No live departure data received for this line yet.',
    );
  });

  it('returns the below-threshold copy', () => {
    expect(
      sampleUnavailableReason(status({ sampleAvailability: { state: 'below-threshold', observed: 2, required: 3 } })),
    ).toBe('Too few live departures sampled to report a rate right now.');
  });
});

describe('formatSampleSummary', () => {
  it('renders the one-line summary used across cards, rows and tables', () => {
    expect(formatSampleSummary(status({ sampleStats: stats }))).toBe('Avg delay 12.4 min · 5% cancelled');
  });

  it('says so when there is no status at all', () => {
    expect(formatSampleSummary(undefined)).toBe('No sample data');
  });

  it('renders the no-coverage reason when there is a status but no stats', () => {
    expect(formatSampleSummary(status())).toBe('No live departure data received for this line yet.');
  });
});
