import { listPhrase, livePreviewImage, previewDigest, previewSpan, previewWhen, tidy } from './livePreview';
import type { PreviewBandTone, StatusPreviewCard } from './livePreview';
import type { PreviewImage } from './pageMetadata';
import { severityBadgeLabel, severityGroup } from './severity';
import { transitIncidentHref } from './transitStops';
import type { TransitSituationDetail } from './types';

/** What a transit disruption's link preview says
 * (`/transit/[network]/incidents/[id]/card.png`, drawn by
 * `renderStatusPreviewImage` like a National Rail incident's card,
 * `lib/incidentPreview.ts`), and the evergreen `og:description` beside it.
 * Pure: everything comes from the disruption the page reads, the network's
 * name, the stop names it looked up, and `now`.
 *
 * Unlike a Knowledgebase incident, a transit disruption has a severity
 * (its worst status on the lines it affects), so a live one's band is
 * that status, in the status pill's colours and words. */

/** Lines beyond this many become "and N more". */
const MAX_LIST_ITEMS = 3;

export interface TransitIncidentPreviewLookups {
  /** The network's name ("Manchester Metrolink"). */
  networkName: string;
  /** Station id to name, for the stops it names. */
  stopNames?: ReadonlyMap<string, string>;
  now: number;
}

function band(situation: TransitSituationDetail): { tone: PreviewBandTone; label: string } {
  if (situation.state === 'ended') return { tone: 'informational', label: 'Ended' };
  if (situation.state === 'upcoming') {
    return { tone: 'planned', label: situation.planned ? 'Upcoming planned work' : 'Upcoming' };
  }
  if (situation.affects.length === 0) return { tone: 'informational', label: 'Notice in effect' };
  if (situation.planned) return { tone: 'planned', label: 'Planned work in progress' };
  return { tone: severityGroup(situation.severity), label: severityBadgeLabel(situation.severity) };
}

function valid(value: string | null | undefined): value is string {
  return !!value && !Number.isNaN(Date.parse(value));
}

/** When it runs, as far as the card can say. */
export function transitIncidentTiming(situation: TransitSituationDetail, now: number): string[] {
  if (situation.state === 'active') {
    const current = situation.validity.find(
      (p) => Date.parse(p.start) <= now && (!valid(p.end) || Date.parse(p.end) > now),
    );
    if (!current) return [];
    const parts = valid(current.start) ? [`Since ${previewWhen(current.start)}`] : [];
    parts.push(valid(current.end) ? `Expected to end ${previewWhen(current.end)}` : 'No end time given');
    return parts;
  }
  if (situation.state === 'upcoming') {
    if (!valid(situation.starts)) return [];
    const period = situation.validity.find((p) => p.start === situation.starts);
    return valid(period?.end)
      ? [`Runs ${previewSpan(situation.starts, period.end)}`]
      : [`Starts ${previewWhen(situation.starts)}`];
  }
  if (valid(situation.endedAt)) return [`Last listed ${previewWhen(situation.endedAt)}`];
  return valid(situation.validityEndedAt) ? [`Ended ${previewWhen(situation.validityEndedAt)}`] : [];
}

function stopPhrase(situation: TransitSituationDetail, names: ReadonlyMap<string, string> | undefined): string | null {
  const stops = situation.stops.map((id) => names?.get(id)).filter((name): name is string => !!name);
  return stops.length > 0 ? `At ${listPhrase(stops)}` : null;
}

/** The card as the shared renderer draws it: where, the network, then when. */
export function transitIncidentPreviewCard(
  situation: TransitSituationDetail,
  lookups: TransitIncidentPreviewLookups,
): StatusPreviewCard {
  const lines = situation.affects.map((line) => tidy(line.name)).filter((name) => name !== '');
  const timing = transitIncidentTiming(situation, lookups.now);
  const rows = [lookups.networkName, timing.length > 0 ? `${timing.join(' · ')} (UK time)` : null];
  return {
    band: { ...band(situation), aside: `Updated ${previewWhen(situation.updatedAt)}` },
    title: tidy(situation.summary),
    lead: stopPhrase(situation, lookups.stopNames),
    chips: { names: lines.slice(0, MAX_LIST_ITEMS), more: Math.max(0, lines.length - MAX_LIST_ITEMS) },
    rows: rows.filter((row): row is string => row !== null && row !== ''),
  };
}

/** The `og:description`: what and where, not how it stands now, so a
 * cached preview stays true ("Disruption on the Blue Line and Green Line,
 * Manchester Metrolink."). The image carries the state. */
export function transitIncidentPreviewDescription(situation: TransitSituationDetail, networkName: string): string {
  const lead = situation.planned ? 'Planned work' : situation.affects.length === 0 ? 'Notice' : 'Disruption';
  const lines = situation.affects.map((line) => tidy(line.name)).filter((name) => name !== '');
  if (lines.length === 0) return `${lead}, ${networkName}: ${tidy(situation.summary).replace(/\.+$/, '')}.`;
  return `${lead} on ${listPhrase(lines)}, ${networkName}.`;
}

/** The preview image's alt text: the disruption, then its state. */
export function transitIncidentPreviewAlt(card: StatusPreviewCard, siteName: string): string {
  return `${card.title} (${card.band.label.toLowerCase()}) on ${siteName}`;
}

/** A digest of what the card is drawn from, for the image URL's `?v=`.
 * `updatedAt` is left out: it moves with every snapshot. */
export function transitIncidentPreviewVersion(situation: TransitSituationDetail): string {
  return previewDigest([
    situation.summary,
    situation.state,
    situation.planned,
    situation.severity,
    situation.starts,
    situation.endedAt,
    situation.validityEndedAt,
    situation.validity,
    situation.stops,
    situation.affects.map((line) => [line.name, line.severity]),
  ]);
}

/** `/transit/{network}/incidents/{id}/card.png?v=...`, the page's preview image. */
export function transitIncidentPreviewImage(
  network: string,
  situation: TransitSituationDetail,
  alt: string,
): PreviewImage {
  return livePreviewImage(
    `${transitIncidentHref(network, situation.id)}/card.png`,
    transitIncidentPreviewVersion(situation),
    alt,
  );
}
