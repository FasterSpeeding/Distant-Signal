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

/** An `ldbws-inferred` status, as `infer_from_samples`/`good_service()`
 * (`crates/aggregator/src/aggregation.rs`) produce and the backend's
 * carry-forward fix (docs/superpowers/specs/
 * 2026-08-30-inferred-time-ranges-design.md) now stamps with a stable
 * `fromDate` across cycles for the same underlying disruption, rather than
 * a fresh timestamp every poll. */
function ldbwsInferredStatus(reason: string, fromDate: string): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason,
    dataQuality: 'ldbws-inferred',
    validityPeriods: [{ fromDate, toDate: null, isNow: true }],
  };
}

function report(id: string, name: string, statuses: LineStatus[]): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
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

  // Regression coverage for the dedup bug identified in docs/superpowers/
  // specs/2026-08-30-inferred-time-ranges-design.md: `infer_from_samples`
  // used to stamp `fromDate` with a fresh `Utc::now()` on every aggregation
  // cycle, independently per line, so two lines sharing one genuine
  // operator-wide LDBWS-detected disruption never had matching `fromDate`s
  // and could never dedupe here. The backend fix (carrying `fromDate`
  // forward across cycles when the underlying status is unchanged) closes
  // this for free -- no change to `statusKey`/`dedupeStationIssues`
  // themselves. These tests lock in that the fix is what closes the gap,
  // not just assert it in prose.
  it('merges LDBWS-inferred statuses across lines when fromDate matches (the carry-forward fix)', () => {
    const items = dedupeStationIssues([
      report('south-west-main', 'South West Main Line', [
        ldbwsInferredStatus('Points failure at Woking', '2026-08-30T06:00:00Z'),
      ]),
      report('portsmouth-direct', 'Portsmouth Direct Line', [
        ldbwsInferredStatus('Points failure at Woking', '2026-08-30T06:00:00Z'),
      ]),
    ]);
    expect(items).toHaveLength(1);
    expect(items[0].lines!.map((l) => l.name)).toEqual(['South West Main Line', 'Portsmouth Direct Line']);
  });

  it('does not merge LDBWS-inferred statuses across lines when fromDate differs (a genuinely different disruption)', () => {
    const items = dedupeStationIssues([
      report('south-west-main', 'South West Main Line', [
        ldbwsInferredStatus('Points failure at Woking', '2026-08-30T06:00:00Z'),
      ]),
      report('portsmouth-direct', 'Portsmouth Direct Line', [
        ldbwsInferredStatus('Points failure at Woking', '2026-08-30T09:17:42.123Z'),
      ]),
    ]);
    expect(items).toHaveLength(2);
  });
});
