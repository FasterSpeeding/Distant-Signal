import { describe, it, expect } from 'vitest';
import { severityColor, severityLabel, worstStatus } from './severity';
import type { LineStatusReport } from './types';

describe('severityColor', () => {
  it('maps GoodService (10) to green', () => {
    expect(severityColor(10)).toBe('green');
  });

  it('maps informational values to gray', () => {
    expect(severityColor(0)).toBe('gray');  // SpecialService
    expect(severityColor(12)).toBe('gray'); // ExitOnly
    expect(severityColor(13)).toBe('gray'); // NoStepFree
  });

  it('maps planned values to blue', () => {
    expect(severityColor(4)).toBe('blue'); // PlannedClosure
    expect(severityColor(5)).toBe('blue'); // PartClosure
  });

  it('maps mild disruption values to yellow', () => {
    expect(severityColor(9)).toBe('yellow');  // MinorDelays
    expect(severityColor(7)).toBe('yellow');  // ReducedService
    expect(severityColor(14)).toBe('yellow'); // ChangeOfFrequency
    expect(severityColor(20)).toBe('yellow'); // Recovering
  });

  it('maps severe disruption values to red', () => {
    expect(severityColor(6)).toBe('red');  // SevereDelays
    expect(severityColor(2)).toBe('red');  // Suspended
    expect(severityColor(3)).toBe('red');  // PartSuspended
    expect(severityColor(1)).toBe('red');  // Closed
    expect(severityColor(11)).toBe('red'); // PartClosed
    expect(severityColor(8)).toBe('red');  // BusService
    expect(severityColor(21)).toBe('red'); // Diverted
  });

  it('falls back to gray for an unrecognized value', () => {
    expect(severityColor(999)).toBe('gray');
  });
});

describe('severityLabel', () => {
  it('returns a human label for a known severity', () => {
    expect(severityLabel(10)).toBe('Good Service');
    expect(severityLabel(2)).toBe('Suspended');
  });

  it('returns a fallback label for an unrecognized value', () => {
    expect(severityLabel(999)).toBe('Unknown');
  });
});

describe('worstStatus', () => {
  const baseReport: LineStatusReport = {
    $type: 'NRStatus.LineStatusReport',
    id: 'wcml',
    name: 'West Coast Main Line',
    modeName: 'national-rail',
    operators: ['AW'],
    lineStatuses: [],
    computedAt: '2026-07-15T09:00:00Z',
  };

  it('returns Good Service when there are no statuses', () => {
    const worst = worstStatus(baseReport);
    expect(worst.statusSeverity).toBe(10);
  });

  it('picks the most severe status by rank, not the lowest statusSeverity number', () => {
    const report: LineStatusReport = {
      ...baseReport,
      lineStatuses: [
        { statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '', dataQuality: 'knowledgebase', validityPeriods: [] },
        { statusSeverity: 21, statusSeverityDescription: 'Diverted', reason: 'Diverted', dataQuality: 'knowledgebase', validityPeriods: [] },
      ],
    };
    const worst = worstStatus(report);
    expect(worst.statusSeverity).toBe(21);
    expect(worst.reason).toBe('Diverted');
  });
});

describe('TfL severity codes', () => {
  // The Rust half of this table lives in crates/common/src/lib.rs
  // (`severity_from_tfl_code` + `severity_rank`), and
  // `rank_matches_the_frontends_group_table` there asserts the two agree.
  it('labels the five TfL-only codes', () => {
    expect(severityLabel(22)).toBe('Service Closed');
    expect(severityLabel(23)).toBe('Not Running');
    expect(severityLabel(24)).toBe('Issues Reported');
    expect(severityLabel(25)).toBe('No Issues');
    expect(severityLabel(26)).toBe('Information');
  });

  it('greys out an overnight closure rather than painting it red', () => {
    // Service Closed is the ordinary state of the Underground at 02:00 —
    // 13 of 20 lines were reporting it when this was written. A red
    // network every night is a false alarm, not information.
    expect(severityColor(22)).toBe('gray');
    expect(severityColor(26)).toBe('gray');
  });

  it('keeps an unexpectedly absent service severe, and "no issues" good', () => {
    expect(severityColor(23)).toBe('red');
    expect(severityColor(24)).toBe('yellow');
    expect(severityColor(25)).toBe('green');
  });

  it('does not confuse TfL Service Closed with the NR Recovering extension', () => {
    expect(severityLabel(20)).toBe('Recovering');
    expect(severityLabel(22)).toBe('Service Closed');
  });
});
