import type { LineStatusReport } from './types';

export type SeverityGroup = 'good' | 'informational' | 'planned' | 'mild' | 'severe';

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

/** The same severity-group -> colour mapping `severityColor` already uses
 * for a raw severity number, exported directly for a caller that only has
 * the `SeverityGroup` bucket, not a specific status (e.g. the network
 * dashboard's counter tiles, keyed by `SEVERITY_GROUPS_BY_RANK` itself --
 * see `app/status/page.tsx`). Single-sourced so a tile's colour cue can
 * never drift from `StatusBadge`'s own colour for the same bucket. */
export const SEVERITY_GROUP_COLORS: Record<SeverityGroup, string> = GROUP_COLOR;

// TfL's `statusSeverity` codes are NOT monotonic with actual severity (e.g.
// 10 GoodService sits in the middle of the numeric range, while 21 Diverted
// and 11 PartClosed are severe but numerically high). This rank reflects
// true severity ordering — severe > mild > planned > informational > good —
// and should be used instead of the raw `statusSeverity` number whenever
// statuses need to be compared/ranked (e.g. picking the "worst" status).
export const GROUP_RANK: Record<SeverityGroup, number> = {
  good: 0,
  informational: 1,
  planned: 2,
  mild: 3,
  severe: 4,
};

/** Display copy for each `SeverityGroup`, single-sourced so the network
 * dashboard's counter tiles (`app/status/page.tsx`) and `AllLinesTable`'s
 * status-group filter chips can never say something different for the same
 * bucket. */
export const SEVERITY_GROUP_LABELS: Record<SeverityGroup, string> = {
  good: 'Good Service',
  informational: 'Informational',
  planned: 'Planned',
  mild: 'Minor Disruption',
  severe: 'Severe Disruption',
};

/** Every `SeverityGroup`, ordered best-to-worst by `GROUP_RANK` -- the
 * iteration order for the dashboard's five counter tiles and
 * `AllLinesTable`'s filter chips, so both render in one deliberate order
 * rather than relying on object-key iteration order. */
export const SEVERITY_GROUPS_BY_RANK: readonly SeverityGroup[] = (
  Object.keys(GROUP_RANK) as SeverityGroup[]
).sort((a, b) => GROUP_RANK[a] - GROUP_RANK[b]);

/** Narrows an untyped query-string value (e.g. `/lines?statusGroup=severe`)
 * to a real `SeverityGroup`, or `false` for anything else -- an unknown/
 * missing/malformed value must fall back to "no filter" rather than being
 * silently treated as some specific bucket. Used by `app/lines/page.tsx` to
 * validate `searchParams.statusGroup` before handing it to `AllLinesTable`
 * as `initialStatusGroup`. */
export function isSeverityGroup(value: string | undefined): value is SeverityGroup {
  return value !== undefined && (SEVERITY_GROUPS_BY_RANK as readonly string[]).includes(value);
}

export function severityColor(severity: number): string {
  const entry = SEVERITY_TABLE[severity];
  return entry ? GROUP_COLOR[entry.group] : 'gray';
}

/** True for both National Rail's Good Service (10) and TfL's No Issues
 * (25) — the two severities `SEVERITY_TABLE` classifies as `'good'`. Use
 * this instead of comparing `statusSeverity === 10` directly whenever the
 * question is "is this line/status actually fine", so TfL statuses aren't
 * silently excluded. */
export function isGoodSeverity(severity: number): boolean {
  return SEVERITY_TABLE[severity]?.group === 'good';
}

export function severityLabel(severity: number): string {
  return SEVERITY_TABLE[severity]?.label ?? 'Unknown';
}

/** Higher rank = more severe. Unknown severities rank alongside `informational`. */
export function severityRank(severity: number): number {
  const entry = SEVERITY_TABLE[severity];
  return GROUP_RANK[entry?.group ?? 'informational'];
}

/** The `SeverityGroup` a raw severity number belongs to (see
 * `SEVERITY_TABLE` above) -- the bucket-membership counterpart of
 * `severityRank`'s numeric ordering. Added for the network-wide dashboard's
 * five-bucket breakdown (`lib/networkStatusOverview.ts`) and
 * `AllLinesTable`'s own status-group filter, both of which need to know
 * WHICH group a status falls into, not just how it ranks against another
 * one. Same unknown-severity fallback as `severityRank`: an unrecognized
 * number is treated as `'informational'`. */
export function severityGroup(severity: number): SeverityGroup {
  return SEVERITY_TABLE[severity]?.group ?? 'informational';
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
