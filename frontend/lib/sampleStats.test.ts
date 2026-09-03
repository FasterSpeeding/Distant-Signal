import { describe, it, expect } from 'vitest';
import {
  cancelledPercent,
  coverageProvenanceNote,
  firstSampleStats,
  formatSampleSummary,
  pendingCoverageNote,
  representativeStatus,
  sampleUnavailableReason,
} from './sampleStats';
import type { LineStatus, SampleStats } from './types';

const stats: SampleStats = { total: 160, delayed: 142, cancelled: 8, skipped: 1, avgDelayMinutes: 12.44 };
const coverageStats: SampleStats = { total: 500, delayed: 20, cancelled: 5, skipped: 2, avgDelayMinutes: 3.1 };

function status(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
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

  it('prefers a status carrying fullCoverageStats over one carrying only sampleStats (Decision 3)', () => {
    const sampleOnly = status({ sampleStats: stats, reason: 'sample-only' });
    const fullCoverage = status({ fullCoverageStats: coverageStats, reason: 'full-coverage' });
    expect(representativeStatus([sampleOnly, fullCoverage])).toBe(fullCoverage);
    // Order-independent: full coverage still wins even listed first.
    expect(representativeStatus([fullCoverage, sampleOnly])).toBe(fullCoverage);
  });

  it('falls back to sampleStats when nothing carries fullCoverageStats', () => {
    const first = status({ reason: 'plain' });
    const sampleOnly = status({ sampleStats: stats, reason: 'sample-only' });
    expect(representativeStatus([first, sampleOnly])).toBe(sampleOnly);
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

  it('returns null when only fullCoverageStats is present (Decision 1/2 -- most confident, checked first)', () => {
    expect(sampleUnavailableReason(status({ fullCoverageStats: coverageStats }))).toBeNull();
  });
});

describe('sampleUnavailableReason with a dataQuality-less carrier (StationOperatorSampleStats-shaped)', () => {
  // Widened signature (Decision 9): a per-operator station row has no
  // `dataQuality` field at all -- TypeScript sees it as `undefined`, so the
  // `'tfl'` branch can never fire for it in practice. `'no-coverage'` is
  // documented-unreachable through the real /sample-stats route (Decision 7),
  // but TypeScript can't enforce that invariant, so this proves the type
  // still accepts it structurally and renders *some* sensible string rather
  // than crashing.

  it('never takes the tfl branch when dataQuality is absent', () => {
    const carrier = { sampleAvailability: { state: 'below-threshold' as const, observed: 1, required: 3 } };
    expect(sampleUnavailableReason(carrier)).toBe('Too few live departures sampled to report a rate right now.');
  });

  it('renders real stats when present, with no dataQuality field at all', () => {
    const carrier = { sampleStats: stats, sampleAvailability: { state: 'available' as const } };
    expect(sampleUnavailableReason(carrier)).toBeNull();
    expect(formatSampleSummary(carrier)).toBe('Avg delay 12.4 min · 5% cancelled');
  });

  it('still renders a sensible string for the type-accepted-but-documented-unreachable no-coverage state', () => {
    const carrier = { sampleAvailability: { state: 'no-coverage' as const } };
    expect(sampleUnavailableReason(carrier)).toBe('No live departure data received for this line yet.');
  });

  it('below-threshold carrier formats through formatSampleSummary the same as a LineStatus would', () => {
    const carrier = { sampleAvailability: { state: 'below-threshold' as const, observed: 0, required: 3 } };
    expect(formatSampleSummary(carrier)).toBe('Too few live departures sampled to report a rate right now.');
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

  it('prefers fullCoverageStats over sampleStats when both are present on the same status (Decision 1)', () => {
    const both = status({ sampleStats: stats, fullCoverageStats: coverageStats });
    // coverageStats: 5/500 = 1% cancelled, avg 3.1 -- distinct from stats'
    // 5% cancelled/12.4 avg, so this proves which one actually rendered.
    expect(formatSampleSummary(both)).toBe('Avg delay 3.1 min · 1% cancelled');
  });

  it('renders real numbers from fullCoverageStats alone, with no sampleStats present', () => {
    expect(formatSampleSummary(status({ fullCoverageStats: coverageStats }))).toBe(
      'Avg delay 3.1 min · 1% cancelled',
    );
  });
});

describe('coverageProvenanceNote', () => {
  it('returns the confident provenance sentence when fullCoverageStats is present', () => {
    expect(coverageProvenanceNote(status({ fullCoverageStats: coverageStats }))).toBe(
      'Based on real train-movement data for every scheduled service on this line — not a live-departure sample.',
    );
  });

  it('returns null when fullCoverageStats is absent, even with real sampleStats', () => {
    expect(coverageProvenanceNote(status({ sampleStats: stats }))).toBeNull();
  });

  it('returns null for a bare carrier with neither field', () => {
    expect(coverageProvenanceNote({ sampleAvailability: { state: 'no-coverage' } })).toBeNull();
  });
});

describe('pendingCoverageNote', () => {
  it('returns the "still resolving" sentence only when fullCoverageAvailability is pending', () => {
    expect(pendingCoverageNote(status({ fullCoverageAvailability: { state: 'pending' } }))).toBe(
      'Full train-movement data is being resolved for this line — showing the live sample in the meantime.',
    );
  });

  it('returns null for not-enabled (the default, overwhelming majority case)', () => {
    expect(pendingCoverageNote(status())).toBeNull();
  });

  it('returns null for available -- that case gets coverageProvenanceNote instead, not this', () => {
    expect(
      pendingCoverageNote(status({ fullCoverageAvailability: { state: 'available' } })),
    ).toBeNull();
  });
});
