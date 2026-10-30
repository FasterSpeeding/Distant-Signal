import { tflStopHref } from './tflStops';
import type { TransitLine } from './types';

// Client-safe transit helpers: no server imports (`lib/transit.ts` has
// `./api`), so the planner's Client Components can use them.

/** The caption every scheduled board and timetable carries, and every
 * planned route with a tram leg: the times are the published timetable,
 * not live tram times (transit spec §2). */
export const SCHEDULED_CAPTION = 'Scheduled times, not live';

/** A line's display colour for a swatch, or a neutral grey. */
export function lineColour(line: Pick<TransitLine, 'colour'> | null | undefined): string {
  return line?.colour ?? 'var(--mantine-color-gray-5)';
}

/** A timetabled transit network's station as the trip planner names it
 * (transit trip planning §5.2): the NaPTAN access-node spelling,
 * `9400ZZMAPIC` for Metrolink's Piccadilly. Never a CRS, so never a
 * station page or board. A London tram stop keeps its TfL spelling
 * (`940GZZCRWMB`, `isTflStopCode`). */
const TRANSIT_STOP_CODE = /^9400[0-9A-Z]{4,16}$/i;

export function isTransitStopCode(code: string | null | undefined): boolean {
  return !!code && TRANSIT_STOP_CODE.test(code.trim());
}

/** Which network's pages a stop id belongs to, by its NaPTAN prefix. Only
 * networks whose stations the planner and untracked legs name by their
 * own id are listed; a TfL stop (`940G`) has its own page. */
const TRANSIT_STOP_PREFIXES: readonly { prefix: string; network: string; name: string }[] = [
  { prefix: '9400ZZMA', network: 'metrolink', name: 'Metrolink' },
];

export function transitNetworkOf(id: string): { network: string; name: string } | undefined {
  const upper = id.trim().toUpperCase();
  return TRANSIT_STOP_PREFIXES.find((entry) => upper.startsWith(entry.prefix));
}

/** A planner transit station as From, To or a segment end shows it:
 * `Piccadilly (Metrolink)`, as a TfL stop says `Uxbridge (Tube)`. */
export function transitStopPlaceName(name: string, networkName: string): string {
  return `${name} (${networkName})`;
}

/** The short name of a network, by id (`metrolink`), for
 * `transitStopPlaceName`: "Metrolink", not the network row's full
 * "Manchester Metrolink". A network this bundle doesn't know is
 * `fallback`. */
export function transitNetworkName(network: string, fallback = 'tram'): string {
  return TRANSIT_STOP_PREFIXES.find((entry) => entry.network === network)?.name ?? fallback;
}

/** A timetabled network's stop page: `/transit/metrolink/stops/9400ZZMAPIC`.
 * `stop` is the station's id (never a platform's), as the stop lists, the
 * line pages, the timetable and a disruption's `stops` all give it. */
export function transitStopHref(network: string, stop: string): string {
  return `/transit/${encodeURIComponent(network)}/stops/${encodeURIComponent(stop)}`;
}

/** The page for a stop an untracked leg was saved with: a Metrolink
 * station's transit stop page, a TfL stop's page (`940G`, London trams
 * included), or `null` for anything else. */
export function stopPageHref(id: string | null | undefined): string | null {
  if (!id?.trim()) return null;
  const transit = transitNetworkOf(id);
  if (transit) return transitStopHref(transit.network, id.trim().toUpperCase());
  if (/^940G[0-9A-Z]{4,16}$/i.test(id.trim())) return tflStopHref(id.trim().toUpperCase());
  return null;
}
