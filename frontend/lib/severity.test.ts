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
