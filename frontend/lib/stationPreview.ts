import { SNAPSHOT_NOTE } from './linePreview';
import { livePreviewImage, plainText, previewDigest, previewWhen, type StatusPreviewCard } from './livePreview';
import type { PreviewImage } from './pageMetadata';
import { severityBadgeLabel, severityGroup, severityRank, worstStatus } from './severity';
import type { LineStatusReport } from './types';

/** A station's live link preview (`/stations/[crs]/card.png`;
 * `lib/livePreview.ts` for what live cards share): the worst status on the
 * lines through it as the band, its name and code, what is wrong, and the
 * disrupted lines as chips. Pure: the station disruption reports the page
 * already reads.
 *
 * No departures: they change by the minute and a chat app may show the
 * card for hours, so a list of trains would mislead long after it was
 * true, where a disruption usually lasts. No accessibility either: the
 * page shows the source's own free-text facilities, which have no single
 * "step-free" answer to put on a card without inventing one. */

/** `/StopPoint/{crs}/Disruption`'s answer, as the page reads it: the
 * disrupted lines through the station (`[]` when every one is fine), or
 * `'none'` when no tracked line serves it. */
export type StationDisruptionState = { coverage: 'covered'; reports: LineStatusReport[] } | { coverage: 'none' };

/** The worst status across every report, with its line. */
function worstAcross(reports: readonly LineStatusReport[]) {
  return reports
    .map((report) => ({ report, status: worstStatus(report) }))
    .reduce<{ report: LineStatusReport; status: ReturnType<typeof worstStatus> } | null>(
      (worst, current) =>
        worst === null || severityRank(current.status.statusSeverity) > severityRank(worst.status.statusSeverity)
          ? current
          : worst,
      null,
    );
}

/** The card. `heading` is the page's own ("Woking (WOK)"); `now` stands in
 * for the time when no report carries one (nothing disrupted). */
export function stationPreviewCard(heading: string, state: StationDisruptionState, now: number): StatusPreviewCard {
  if (state.coverage === 'none') {
    return {
      band: { tone: 'informational', label: 'No line status tracked', aside: '' },
      title: heading,
      lead: null,
      chips: { names: [], more: 0 },
      rows: [],
    };
  }
  const disrupted = state.reports.filter((report) => severityGroup(worstStatus(report).statusSeverity) !== 'good');
  const worst = worstAcross(disrupted);
  const computed = state.reports.map((report) => Date.parse(report.computedAt)).filter((t) => !Number.isNaN(t));
  const asOf = `As of ${previewWhen(new Date(computed.length > 0 ? Math.max(...computed) : now).toISOString())}`;
  if (worst === null) {
    return {
      band: { tone: 'good', label: 'No disruptions', aside: asOf },
      title: heading,
      lead: null,
      chips: { names: [], more: 0 },
      rows: [SNAPSHOT_NOTE],
    };
  }
  const names = [...disrupted]
    .sort((a, b) => severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity))
    .map((report) => report.name);
  const reason = 'reason' in worst.status ? plainText(worst.status.reason) : '';
  return {
    band: {
      tone: severityGroup(worst.status.statusSeverity),
      label: severityBadgeLabel(worst.status.statusSeverity),
      aside: asOf,
    },
    title: heading,
    lead: reason === '' ? null : reason,
    chips: { names, more: 0 },
    rows: [SNAPSHOT_NOTE],
  };
}

/** A digest of the station's state for `?v=`: each disrupted line's
 * statuses (severity and reason), not their times. */
export function stationPreviewVersion(heading: string, state: StationDisruptionState | null): string {
  if (state === null || state.coverage === 'none') return previewDigest([heading, state?.coverage ?? null]);
  const lines = state.reports
    .map((report) => [report.id, report.lineStatuses.map((status) => [status.statusSeverity, status.reason])] as const)
    .sort((a, b) => a[0].localeCompare(b[0]));
  return previewDigest([heading, lines]);
}

/** `/stations/{crs}/card.png?v=...`. `state` is null when the page
 * couldn't read it: the URL then changes once it can. */
export function stationPreviewImage(
  crs: string,
  heading: string,
  state: StationDisruptionState | null,
  alt: string,
): PreviewImage {
  return livePreviewImage(`/stations/${encodeURIComponent(crs)}/card.png`, stationPreviewVersion(heading, state), alt);
}
