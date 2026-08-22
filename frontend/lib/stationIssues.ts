import type { LineStatus, LineStatusReport } from './types';

export interface IssueLineRef {
  id: string;
  name: string;
}

/** An issue plus the lines it was reported on. `lines` is optional so the
 * line detail page — where every issue belongs to the one line named in the
 * heading — doesn't have to say so on every row. */
export interface IssueItem {
  status: LineStatus;
  lines?: IssueLineRef[];
}

/** Identity of an issue across reports. Reason alone is not enough (the
 * same words at two severities are two different situations) and the
 * validity window has to be in the key too, or a recurring closure would
 * merge with next month's. */
export function statusKey(status: LineStatus): string {
  return [
    status.statusSeverity,
    status.dataQuality,
    status.reason,
    status.validityPeriods.map((p) => `${p.fromDate}/${p.toDate ?? ''}/${p.isNow}`).join(';'),
  ].join(' ');
}

/** `/StopPoint/{crs}/Disruption` returns one report per line through the
 * station, and an operator-wide incident lands identically on all of them —
 * Woking rendered the same three disruptions, each behind its own filter
 * block and tab bar, three times over. Collapse identical statuses into one
 * item carrying every line it was reported on, so attribution survives
 * without the repetition. First-seen order is preserved; `IssueList` sorts
 * by urgency afterwards. */
export function dedupeStationIssues(reports: LineStatusReport[]): IssueItem[] {
  const byKey = new Map<string, IssueItem & { lines: IssueLineRef[] }>();

  for (const report of reports) {
    for (const status of report.lineStatuses) {
      const key = statusKey(status);
      const existing = byKey.get(key);
      if (!existing) {
        byKey.set(key, { status, lines: [{ id: report.id, name: report.name }] });
        continue;
      }
      if (!existing.lines.some((line) => line.id === report.id)) {
        existing.lines.push({ id: report.id, name: report.name });
      }
    }
  }

  return Array.from(byKey.values());
}
