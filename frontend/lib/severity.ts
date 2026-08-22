import type { LineStatusReport } from './types';

type SeverityGroup = 'good' | 'informational' | 'planned' | 'mild' | 'severe';

const SEVERITY_TABLE: Record<number, { label: string; group: SeverityGroup }> = {
  0: { label: 'Special Service', group: 'informational' },
  1: { label: 'Closed', group: 'severe' },
  2: { label: 'Suspended', group: 'severe' },
  3: { label: 'Part Suspended', group: 'severe' },
  4: { label: 'Planned Closure', group: 'planned' },
  5: { label: 'Part Closure', group: 'planned' },
  6: { label: 'Severe Delays', group: 'severe' },
  7: { label: 'Reduced Service', group: 'mild' },
  8: { label: 'Rail Replacement', group: 'severe' },
  9: { label: 'Minor Delays', group: 'mild' },
  10: { label: 'Good Service', group: 'good' },
  11: { label: 'Part Closed', group: 'severe' },
  12: { label: 'Exit Only', group: 'informational' },
  13: { label: 'No Step Free Access', group: 'informational' },
  14: { label: 'Change of Frequency', group: 'mild' },
  20: { label: 'Recovering', group: 'mild' },
  21: { label: 'Diverted', group: 'severe' },
  // TfL-only codes. Their numbers are this app's own discriminants (see
  // crates/common/src/lib.rs), not TfL's raw statusSeverity: TfL's 20 is
  // "Service Closed" but 20 was already taken by the NR "Recovering"
  // extension, so the poller remaps them on the way in.
  22: { label: 'Service Closed', group: 'informational' },
  23: { label: 'Not Running', group: 'severe' },
  24: { label: 'Issues Reported', group: 'mild' },
  25: { label: 'No Issues', group: 'good' },
  26: { label: 'Information', group: 'informational' },
};

const GROUP_COLOR: Record<SeverityGroup, string> = {
  good: 'green',
  informational: 'gray',
  planned: 'blue',
  mild: 'yellow',
  severe: 'red',
};

// TfL's `statusSeverity` codes are NOT monotonic with actual severity (e.g.
// 10 GoodService sits in the middle of the numeric range, while 21 Diverted
// and 11 PartClosed are severe but numerically high). This rank reflects
// true severity ordering — severe > mild > planned > informational > good —
// and should be used instead of the raw `statusSeverity` number whenever
// statuses need to be compared/ranked (e.g. picking the "worst" status).
const GROUP_RANK: Record<SeverityGroup, number> = {
  good: 0,
  informational: 1,
  planned: 2,
  mild: 3,
  severe: 4,
};

export function severityColor(severity: number): string {
  const entry = SEVERITY_TABLE[severity];
  return entry ? GROUP_COLOR[entry.group] : 'gray';
}

export function severityLabel(severity: number): string {
  return SEVERITY_TABLE[severity]?.label ?? 'Unknown';
}

/** Higher rank = more severe. Unknown severities rank alongside `informational`. */
export function severityRank(severity: number): number {
  const entry = SEVERITY_TABLE[severity];
  return GROUP_RANK[entry?.group ?? 'informational'];
}

/** Picks the most severe status on a report by true severity rank (see
 * `severityRank`), not by the raw `statusSeverity` number. Returns a
 * synthetic Good-Service-shaped object when the report has no statuses at
 * all. */
export function worstStatus(report: LineStatusReport) {
  if (report.lineStatuses.length === 0) {
    return { statusSeverity: 10, reason: '' };
  }
  return report.lineStatuses.reduce((worst, current) =>
    severityRank(current.statusSeverity) > severityRank(worst.statusSeverity) ? current : worst,
  );
}
