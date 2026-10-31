import type { ReactNode } from 'react';
import { LineStatusRow, LineStatusRowDetail, LineStatusRowItem, LineStatusRowList } from '@/components/LineStatusRow';
import { lineDisruptions, TransitLineDisruption } from '@/components/TransitDisruptionSummary';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { isGoodSeverity } from '@/lib/severity';
import type { TransitLine, TransitLineStatus } from '@/lib/types';

type ListedLine = TransitLine & { status?: TransitLineStatus };

/** A network's lines as `LineStatusRow`s, the page's main way in: each
 * line's colour, name and status badge, then its disruption summary, in
 * the rows' 1/2/3-column grid, so seven Metrolink lines or eleven Tube
 * lines read at a glance.
 *
 * `summary` fills each row's second line: the network page passes the
 * line's disruption summary there. By default a line with published
 * disruptions shows its worst one's headline, linking its page, and the
 * rest's count (`TransitLineDisruption`, no second chip); any other line
 * whose status isn't good says why, in a line (`TransitLineStatusSummary`). */
export function TransitLineList({
  network,
  lines,
  label,
  summary = (line) =>
    lineDisruptions(line) ? (
      <TransitLineDisruption network={network} line={line} chip={false} />
    ) : (
      <TransitLineStatusSummary status={line.status} lineName={line.name} />
    ),
}: {
  network: string;
  lines: ListedLine[];
  /** The list's accessible name ("Underground"). */
  label: string;
  summary?: (line: ListedLine) => ReactNode;
}) {
  return (
    <LineStatusRowList label={label}>
      {lines.map((line) => (
        <LineStatusRowItem key={line.id}>
          <LineStatusRow
            href={`/transit/${network}/lines/${line.lineId}`}
            name={line.name}
            colour={line}
            badge={<TransitStatusBadge status={line.status} />}
          >
            {summary(line)}
          </LineStatusRow>
        </LineStatusRowItem>
      ))}
    </LineStatusRowList>
  );
}

/** A line's status in a line, under its badge: the first status's reason
 * that isn't good service ("Minor delays between Bank and Morden"), and how
 * many more it has. Nothing while the line runs well, or has no feed.
 * For a line whose status names no disruption page (TfL's own status);
 * one that does gets `TransitLineDisruption`, its headline linking that
 * page. TfL starts its reasons with the line's name
 * ("PICCADILLY LINE: ..."), which the card already shows, so a leading
 * "<line name>:" is dropped. */
export function TransitLineStatusSummary({
  status,
  lineName,
}: {
  status?: TransitLineStatus | null | undefined;
  lineName?: string | undefined;
}) {
  if (status?.state !== 'current') return null;
  const problems = status.statuses.filter((entry) => !isGoodSeverity(entry.severity));
  const first = problems[0];
  const raw = first?.reason.trim() || first?.description.trim();
  if (!raw) return null;
  const text = lineName ? withoutLinePrefix(raw, lineName) : raw;
  const more = problems.length - 1;
  return (
    <LineStatusRowDetail data-line-summary>
      {text}
      {more > 0 && ` (and ${more} more)`}
    </LineStatusRowDetail>
  );
}

/** `text` without a leading "<line name>[ line]:" ("Bakerloo Line: ..."),
 * matched ignoring case; unchanged when it has none or nothing follows. */
export function withoutLinePrefix(text: string, lineName: string): string {
  const colon = text.indexOf(':');
  if (colon <= 0) return text;
  const prefix = text.slice(0, colon).trim().toLowerCase();
  const name = lineName.trim().toLowerCase();
  if (prefix !== name && prefix !== `${name} line`) return text;
  const rest = text.slice(colon + 1).trim();
  return rest || text;
}
