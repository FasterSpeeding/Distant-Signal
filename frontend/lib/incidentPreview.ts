import { incidentState, type IncidentState } from './incidents';
import {
  listPhrase,
  livePreviewImage,
  previewDigest,
  previewSpan,
  previewWhen,
  type StatusPreviewCard,
  tidy,
  TITLE_FITS_LENGTH,
} from './livePreview';
import type { PreviewImage } from './pageMetadata';
import type { IncidentDetail } from './types';
import { periodIsActive, periodIsUpcoming } from './validity';

/** What an incident's link preview says (`/incidents/[id]/card.png`, drawn
 * by `renderStatusPreviewImage`; see `lib/livePreview.ts` for what live
 * cards share), and the evergreen `og:description` beside it. Pure:
 * everything comes from the incident the page already reads, the operator
 * names it already looks up, and `now`.
 *
 * National Rail gives incidents no severity (`priority` is an undocumented
 * integer, `crates/common`), no reason category and no line colours, so
 * the card shows none of those. Its coloured band is the incident's state,
 * in the severity pill's palette (`--ds-status-badge-*`), and always
 * spelled out in words. */

/** The `StatusBadge` fill a state's band takes. A live disruption of
 * unknown severity reads as caution (yellow, "mild"), never red: nothing
 * says it is severe. Planned work, live or upcoming, is blue; a cleared
 * incident is green (service restored); one the feed simply stopped
 * listing is gray, outcome not stated (docs/style-guide.md, "Semantic
 * roles"). */
export type IncidentPreviewTone = 'mild' | 'planned' | 'good' | 'informational';

export interface IncidentPreviewCard {
  /** The coloured band: its words carry the state, the colour only repeats it. */
  status: { tone: IncidentPreviewTone; label: string };
  /** The summary, tidied: whitespace collapsed, and a leading "CLEARED:"
   * dropped because the band already says so. */
  title: string;
  /** Up to `MAX_LIST_ITEMS` line names, and how many more there are. */
  lines: { names: string[]; more: number };
  /** "Avanti West Coast and Transport for Wales", or null. */
  operators: string | null;
  /** "Between Chester and Llandudno Junction" (from a summary too long to
   * be sure it shows whole), else the affected stations, else null. */
  route: string | null;
  /** "Since Thu 30 Oct, 13:50", "Expected to end 15:00": one or two parts. */
  timing: string[];
  /** "Updated Thu 30 Oct, 14:03": the last recorded change. */
  updated: string;
}

export interface IncidentPreviewLookups {
  /** ATOC code to operator name (`tocNameLookup`); an unknown code shows as is. */
  operatorNames: ReadonlyMap<string, string>;
  /** CRS to station name, for the codes the caller looked up. */
  stationNames?: ReadonlyMap<string, string | null>;
  now: number;
}

/** Lines and operators beyond this many become "and N more". */
export const MAX_LIST_ITEMS = 3;
/** The affected stations the image route looks up (one lookup each): the
 * ones the card can name before "and N more". */
export const MAX_PREVIEW_STATIONS = MAX_LIST_ITEMS;

const STATUS: Record<IncidentState, (planned: boolean) => IncidentPreviewCard['status']> = {
  active: (planned) =>
    planned
      ? { tone: 'planned', label: 'Planned work in progress' }
      : { tone: 'mild', label: 'Disruption in progress' },
  upcoming: (planned) => ({ tone: 'planned', label: planned ? 'Upcoming planned work' : 'Upcoming' }),
  cleared: () => ({ tone: 'good', label: 'Cleared' }),
  ended: () => ({ tone: 'informational', label: 'Ended' }),
};

/** The summary without a leading "CLEARED:" (the band says it). */
export function previewTitle(summary: string): string {
  const title = tidy(summary);
  const stripped = title.replace(/^cleared\s*:\s*/i, '');
  return stripped === '' ? title : stripped;
}

/** The place an incident's summary names, as Knowledgebase summaries put
 * it: "between Chester and Llandudno Junction", "through Rochester", "at
 * Lewes". Stops before the time ("expected until 15:00"), the dates ("from
 * Monday 25 ...") and the cause. Null when the summary names no place this
 * way. */
export function routePhrase(summary: string): string | null {
  const [, word, rest] =
    /\b(between|through|at|near)\s+(.+?)(?=\s+(?:expected|until|from|on|due|following|after|while|is|are|has|have|will)\b|[.,;]|$)/i.exec(
      tidy(summary),
    ) ?? [];
  const place = rest?.trim();
  if (!word || !place || /^\d/.test(place)) return null;
  return `${word.toLowerCase()} ${place}`;
}

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function valid(value: string | null | undefined): value is string {
  return !!value && !Number.isNaN(Date.parse(value));
}

function timing(incident: IncidentDetail, state: IncidentState, now: number): string[] {
  const periods = incident.validityPeriods;
  if (state === 'active') {
    const current = periods.find((period) => periodIsActive(period, now));
    if (!current) return [];
    const parts = valid(current.fromDate) ? [`Since ${previewWhen(current.fromDate)}`] : [];
    parts.push(valid(current.toDate) ? `Expected to end ${previewWhen(current.toDate)}` : 'No end time given');
    return parts;
  }
  if (state === 'upcoming') {
    const next = periods.find((period) => periodIsUpcoming(period, now));
    const starts = valid(incident.startsAt) ? incident.startsAt : next?.fromDate;
    if (!valid(starts)) return [];
    const ends = next?.toDate;
    return valid(ends) ? [`Runs ${previewSpan(starts, ends)}`] : [`Starts ${previewWhen(starts)}`];
  }
  if (state === 'ended' && valid(incident.sourceRemovedAt)) {
    return [`Last listed ${previewWhen(incident.sourceRemovedAt)}`];
  }
  // Cleared, or every window finished: when it ran, as far as is known.
  const first = periods.map((period) => period.fromDate).find(valid);
  const end = valid(incident.validityEndedAt) ? incident.validityEndedAt : undefined;
  if (first && end) return [`Ran ${previewSpan(first, end)}`];
  if (end) return [`Ended ${previewWhen(end)}`];
  return first ? [`Started ${previewWhen(first)}`] : [];
}

/** The newest recorded change (history is newest first), else first seen. */
function lastChange(incident: IncidentDetail): string {
  const recorded = incident.history[0]?.recordedAt;
  return valid(recorded) ? recorded : incident.firstSeenAt;
}

function operatorNames(incident: IncidentDetail, names: ReadonlyMap<string, string>): string[] {
  return incident.operators.map((code) => names.get(code) ?? code);
}

/** The card's contents. `state` is the api's when it sends one
 * (`incidentState`), so the card and the page's badge always agree. */
export function incidentPreviewCard(incident: IncidentDetail, lookups: IncidentPreviewLookups): IncidentPreviewCard {
  const state = incidentState(incident, lookups.now);
  const lineNames = incident.currentlyAffectsLines.map((line) => tidy(line.name)).filter((name) => name !== '');
  const operators = operatorNames(incident, lookups.operatorNames);
  const stations = incident.affectedStations.map((crs) => lookups.stationNames?.get(crs) ?? crs);
  const title = previewTitle(incident.summary);
  // The place is in the summary already: repeat it only where the title
  // may be cut short.
  const place = title.length > TITLE_FITS_LENGTH ? routePhrase(incident.summary) : null;
  return {
    status: STATUS[state](incident.isPlanned),
    title,
    lines: { names: lineNames.slice(0, MAX_LIST_ITEMS), more: Math.max(0, lineNames.length - MAX_LIST_ITEMS) },
    operators: operators.length > 0 ? listPhrase(operators) : null,
    route: place ? capitalise(place) : stations.length > 0 ? `At ${listPhrase(stations)}` : null,
    timing: timing(incident, state, lookups.now),
    updated: `Updated ${previewWhen(lastChange(incident))}`,
  };
}

/** The card as the shared renderer draws it: where, who, then when. */
export function incidentStatusCard(card: IncidentPreviewCard): StatusPreviewCard {
  const rows = [card.operators, card.timing.length > 0 ? `${card.timing.join(' · ')} (UK time)` : null];
  return {
    band: { ...card.status, aside: card.updated },
    title: card.title,
    lead: card.route,
    chips: card.lines,
    rows: rows.filter((row): row is string => row !== null),
  };
}

/** The `og:description`: what and where, not how it stands now, so a
 * preview cached by a chat app stays true after the status changes
 * ("Disruption between Chester and Llandudno Junction affecting Avanti
 * West Coast and Transport for Wales."). The image carries the state. */
export function incidentPreviewDescription(incident: IncidentDetail, names: ReadonlyMap<string, string>): string {
  const lead = incident.isPlanned ? 'Planned work' : 'Disruption';
  const place = routePhrase(incident.summary);
  const lines = incident.currentlyAffectsLines.map((line) => tidy(line.name)).filter((name) => name !== '');
  const where = place ?? (lines.length > 0 ? `on ${listPhrase(lines)}` : null);
  const operators = operatorNames(incident, names);
  const who = operators.length > 0 ? `affecting ${listPhrase(operators)}` : null;
  if (!where && !who) return `${lead}: ${previewTitle(incident.summary).replace(/\.+$/, '')}.`;
  return `${[lead, where, who].filter(Boolean).join(' ')}.`;
}

/** The preview image's alt text: the incident, then its state. */
export function incidentPreviewAlt(card: IncidentPreviewCard, siteName: string): string {
  return `${card.title} (${card.status.label.toLowerCase()}) on ${siteName}`;
}

/** A digest of everything the card is drawn from, for the image URL's
 * `?v=` (`lib/livePreview.ts`): a new state, time or line list gets a new
 * URL, an unchanged incident keeps its cached one. `fetchedAt` is left
 * out: it moves on every poll. */
export function incidentPreviewVersion(incident: IncidentDetail): string {
  return previewDigest([
    incident.summary,
    incident.state ?? null,
    incident.isPlanned,
    incident.isCleared,
    incident.sourceRemovedAt ?? null,
    incident.startsAt ?? null,
    incident.validityEndedAt ?? null,
    incident.validityPeriods,
    incident.operators,
    incident.affectedStations,
    incident.currentlyAffectsLines.map((line) => line.name),
    incident.history[0]?.recordedAt ?? null,
  ]);
}

/** `/incidents/{id}/card.png?v=...`, the page's preview image. */
export function incidentPreviewImage(incident: IncidentDetail, alt: string): PreviewImage {
  return livePreviewImage(
    `/incidents/${encodeURIComponent(incident.incidentId)}/card.png`,
    incidentPreviewVersion(incident),
    alt,
  );
}
