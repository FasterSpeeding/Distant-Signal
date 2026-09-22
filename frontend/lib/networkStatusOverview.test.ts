import { describe, it, expect } from 'vitest';
import { buildNetworkStatusOverview } from './networkStatusOverview';
import type { LineStatus, LineStatusReport } from './types';

function status(overrides: Partial<LineStatus> & { statusSeverity: number }): LineStatus {
  return {
    statusSeverityDescription: 'x',
    reason: '',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    lineStatuses: [],
    computedAt: '2026-09-22T09:00:00Z',
    ...overrides,
  };
}

describe('buildNetworkStatusOverview', () => {
  it('buckets each line by its worst status, into the five real SeverityGroup values', () => {
    const reports = [
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }), // good
      report({ id: 'b', name: 'B', lineStatuses: [status({ statusSeverity: 9 })] }),  // mild
      report({ id: 'c', name: 'C', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'd', name: 'D', lineStatuses: [status({ statusSeverity: 4 })] }),  // planned
      report({ id: 'e', name: 'E', lineStatuses: [status({ statusSeverity: 0 })] }),  // informational
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.counts).toEqual({ good: 1, mild: 1, severe: 1, planned: 1, informational: 1 });
    expect(overview.totalLines).toBe(5);
  });

  it('excludes MERGED_TFL_LINE_IDS so a merged line is never counted twice', () => {
    const reports = [
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line (TfL)', lineStatuses: [status({ statusSeverity: 23 })] }),
      report({ id: 'elizabeth-line', name: 'Elizabeth line', modeName: 'elizabeth-line', lineStatuses: [status({ statusSeverity: 10 })] }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.totalLines).toBe(1);
    expect(overview.counts.good).toBe(1);
    expect(overview.counts.severe).toBe(0);
  });

  it('sorts worstFirst by severity rank descending, then alphabetically, and excludes good-service lines', () => {
    const reports = [
      report({ id: 'wcml', name: 'West Coast Main Line', lineStatuses: [status({ statusSeverity: 9 })] }), // mild
      report({ id: 'gwr', name: 'Great Western Railway', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'ecml', name: 'East Coast Main Line', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'good', name: 'Good Line', lineStatuses: [status({ statusSeverity: 10 })] }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.worstFirst.map((r) => r.id)).toEqual(['ecml', 'gwr', 'wcml']);
  });

  it('splits National Rail from every TfL-published mode', () => {
    const reports = [
      report({ id: 'nr', name: 'NR', modeName: 'national-rail' }),
      report({ id: 'tube', name: 'Tube', modeName: 'tube' }),
      report({ id: 'dlr', name: 'DLR', modeName: 'dlr' }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.byMode.nationalRail.map((r) => r.id)).toEqual(['nr']);
    expect(overview.byMode.tfl.map((r) => r.id).sort()).toEqual(['dlr', 'tube']);
  });

  it('groups by country via countryForReport, keying only countries actually present', () => {
    const reports = [report({ id: 'nr', name: 'NR', modeName: 'national-rail' })];
    const overview = buildNetworkStatusOverview(reports);
    expect(Object.keys(overview.byCountry)).toEqual(['Gb']);
  });

  it('returns all-zero counts and empty lists for no reports', () => {
    const overview = buildNetworkStatusOverview([]);
    expect(overview.counts).toEqual({ good: 0, informational: 0, planned: 0, mild: 0, severe: 0 });
    expect(overview.worstFirst).toEqual([]);
    expect(overview.totalLines).toBe(0);
  });

  it('lastUpdated is null for no reports', () => {
    expect(buildNetworkStatusOverview([]).lastUpdated).toBeNull();
  });

  it('lastUpdated is the most recent computedAt across every report, not response order', () => {
    const reports = [
      report({ id: 'a', name: 'A', computedAt: '2026-09-22T09:00:00Z' }),
      report({ id: 'b', name: 'B', computedAt: '2026-09-22T09:05:00Z' }),
      report({ id: 'c', name: 'C', computedAt: '2026-09-22T08:55:00Z' }),
    ];
    expect(buildNetworkStatusOverview(reports).lastUpdated).toBe('2026-09-22T09:05:00Z');
  });
});
