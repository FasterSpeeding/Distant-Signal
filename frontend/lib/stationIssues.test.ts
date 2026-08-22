import { describe, it, expect } from 'vitest';
import { dedupeStationIssues } from './stationIssues';
import type { LineStatus, LineStatusReport } from './types';

function status(reason: string, severity = 9): LineStatus {
  return {
    statusSeverity: severity,
    statusSeverityDescription: 'Minor Delays',
    reason,
    dataQuality: 'planned',
    validityPeriods: [{ fromDate: '2026-05-10T00:00:00Z', toDate: '2026-10-11T00:00:00Z', isNow: false }],
  };
}

function report(id: string, name: string, statuses: LineStatus[]): LineStatusReport {
  return {
    $type: 'NRStatus.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SW'],
    computedAt: '2026-08-21T12:00:00Z',
    lineStatuses: statuses,
  };
}

describe('dedupeStationIssues', () => {
  it('collapses an operator-wide issue reported on three lines into one item', () => {
    const shared = status('Berrylands Station Upgrade');
    const items = dedupeStationIssues([
      report('portsmouth-direct', 'Portsmouth Direct Line', [shared]),
      report('south-west-main', 'South West Main Line', [{ ...shared }]),
      report('alton', 'Alton Line', [{ ...shared }]),
    ]);
    expect(items).toHaveLength(1);
    // dedupeStationIssues always populates `lines`; the `!` is asserting
    // that guarantee, not sidestepping the (correctly) optional field on
    // the shared `IssueItem` type.
    expect(items[0].lines!.map((l) => l.name)).toEqual([
      'Portsmouth Direct Line',
      'South West Main Line',
      'Alton Line',
    ]);
  });

  it('keeps genuinely different issues apart', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('Signal failure')]),
      report('b', 'B', [status('Engineering works')]),
    ]);
    expect(items).toHaveLength(2);
  });

  it('does not merge issues that differ only in severity', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('Same words', 9)]),
      report('b', 'B', [status('Same words', 6)]),
    ]);
    expect(items).toHaveLength(2);
  });

  it('preserves first-seen order', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('First'), status('Second')]),
      report('b', 'B', [status('Second')]),
    ]);
    expect(items.map((i) => i.status.reason)).toEqual(['First', 'Second']);
  });

  it('does not list the same line twice for an issue reported twice on it', () => {
    const items = dedupeStationIssues([report('a', 'A', [status('Dup'), status('Dup')])]);
    expect(items).toHaveLength(1);
    expect(items[0].lines).toHaveLength(1);
  });

  it('returns nothing for no reports', () => {
    expect(dedupeStationIssues([])).toEqual([]);
  });
});
