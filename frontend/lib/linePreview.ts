import {
  livePreviewImage,
  listPhrase,
  plainText,
  previewDigest,
  previewWhen,
  type StatusPreviewCard,
} from './livePreview';
import type { PreviewImage } from './pageMetadata';
import { severityBadgeLabel, severityGroup, severityRank, worstStatus } from './severity';
import type { LineStatus, LineStatusReport } from './types';

/** A line's live link preview (`/lines/[id]/card.png`; `lib/livePreview.ts`
 * for what live cards share): its worst status as the band, the line's
 * name, what is wrong, how many disruptions there are, its operators and
 * the sampled trains' figures, all as of the status's own `computedAt`.
 * Pure: the report the page already reads and the operator names.
 *
 * Line colours are not in the data a line page reads (the catalogue has
 * none), so the card has none. */

/** No `line_status` row yet: never "Good service", which would be a claim
 * about a line nothing has assessed (the page shows "No status yet"). */
export const NO_LINE_STATUS_LABEL = 'No status yet';

/** The last row of every live card that has a time: it is a picture of
 * one moment, and the page has the rest. */
export const SNAPSHOT_NOTE = 'A snapshot in UK time: open the page for live status';

/** The statuses that are something wrong: not Good Service or No Issues. */
function disruptions(report: LineStatusReport): LineStatus[] {
  return report.lineStatuses.filter((status) => severityGroup(status.statusSeverity) !== 'good');
}

/** "5 of 42 sampled trains delayed, avg 3.2 min late", the page's own
 * figures (`RepresentativeInfo`), or null with nothing sampled. */
export function sampleRow(statuses: readonly LineStatus[]): string | null {
  const carrier = statuses.find((status) => status.fullCoverageStats) ?? statuses.find((status) => status.sampleStats);
  const stats = carrier?.fullCoverageStats ?? carrier?.sampleStats;
  if (!stats || stats.total <= 0) return null;
  return `${stats.delayed} of ${stats.total} sampled trains delayed, avg ${stats.avgDelayMinutes.toFixed(1)} min late`;
}

/** The card. `report` is null for a line with no status row yet. */
export function linePreviewCard(
  name: string,
  report: LineStatusReport | null,
  operatorCodes: readonly string[],
  operatorNames: ReadonlyMap<string, string>,
): StatusPreviewCard {
  const operators = operatorCodes.map((code) => operatorNames.get(code) ?? code);
  const operatorRow = operators.length > 0 ? listPhrase(operators) : null;
  if (report === null) {
    return {
      band: { tone: 'informational', label: NO_LINE_STATUS_LABEL, aside: '' },
      title: name,
      lead: null,
      chips: { names: [], more: 0 },
      rows: operatorRow ? [operatorRow] : [],
    };
  }
  const worst = worstStatus(report);
  const wrong = disruptions(report);
  const reason = wrong.length > 0 && 'reason' in worst ? plainText(worst.reason) : '';
  const rows = [
    wrong.length > 1 ? `${wrong.length} disruptions on this line` : null,
    operatorRow,
    sampleRow(report.lineStatuses),
    SNAPSHOT_NOTE,
  ];
  return {
    band: {
      tone: severityGroup(worst.statusSeverity),
      label: severityBadgeLabel(worst.statusSeverity),
      aside: `As of ${previewWhen(report.computedAt)}`,
    },
    title: name,
    lead: reason === '' ? null : reason,
    chips: { names: [], more: 0 },
    rows: rows.filter((row): row is string => row !== null),
  };
}

/** A digest of the line's state for `?v=`: every status's severity and
 * reason, worst first. Not `computedAt` or the sampled figures, which move
 * every cycle: the URL changes when the status does, and the image shows
 * the latest figures whenever it is fetched. */
export function linePreviewVersion(name: string, report: LineStatusReport | null): string {
  const statuses = report
    ? [...report.lineStatuses]
        .sort((a, b) => severityRank(b.statusSeverity) - severityRank(a.statusSeverity))
        .map((status) => [status.statusSeverity, status.reason])
    : null;
  return previewDigest([name, statuses]);
}

/** `/lines/{id}/card.png?v=...`. */
export function linePreviewImage(id: string, name: string, report: LineStatusReport | null, alt: string): PreviewImage {
  return livePreviewImage(`/lines/${encodeURIComponent(id)}/card.png`, linePreviewVersion(name, report), alt);
}
