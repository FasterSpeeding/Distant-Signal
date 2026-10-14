import { notFound } from 'next/navigation';
import { ApiNotFoundError, getTransitNetwork } from './api';
import type { TransitLine, TransitLineStatus } from './types';

/** A network or line path segment (`metrolink`, `blue`): the api's grammar,
 * checked before asking it so a malformed URL 404s at once. */
export const TRANSIT_ID = /^[a-z0-9-]{1,32}$/;

/** A station's ATCO code (`9400ZZMAPIC`). */
export const TRANSIT_STOP_ID = /^[A-Z0-9]{1,16}$/;

/** The caption every scheduled board and timetable carries: the times are
 * the published timetable, not live tram times (transit spec §2). */
export const SCHEDULED_CAPTION = 'Scheduled times, not live';

/** `value`, or `notFound()` when the api answered 404 (an unknown network,
 * line or stop, or a network the api does not serve). */
export async function orNotFound<T>(value: Promise<T>): Promise<T> {
  try {
    return await value;
  } catch (err) {
    if (err instanceof ApiNotFoundError) notFound();
    throw err;
  }
}

/** `HH:MM` of an RFC 3339 time already in UK local time. */
export function departureClock(rfc3339: string): string {
  return rfc3339.slice(11, 16);
}

/** A line's display colour for a swatch, or a neutral grey. */
export function lineColour(line: Pick<TransitLine, 'colour'> | null | undefined): string {
  return line?.colour ?? 'var(--mantine-color-gray-5)';
}

/** `metrolink:blue` -> `blue`. */
export function localLineId(publicId: string): string {
  return publicId.slice(publicId.indexOf(':') + 1);
}

/** A tram line's public id (`metrolink:blue`). */
export const TRANSIT_LINE_ID = /^[a-z0-9-]{1,32}:[a-z0-9-]{1,32}$/;

/** A pinned tram line, as the home page's "Your lines" shows it. */
export interface PinnedTramLine {
  network: string;
  networkName: string;
  line: TransitLine & { status?: TransitLineStatus };
}

/** The pinned tram lines among `pinnedIds`, in pin order: each network's
 * lines (with their status) fetched once. A line whose network isn't
 * served, or no longer has it, is left out, as is every line of a network
 * whose fetch fails. */
export async function loadPinnedTramLines(pinnedIds: string[]): Promise<PinnedTramLine[]> {
  const ids = pinnedIds.filter((id) => TRANSIT_LINE_ID.test(id));
  if (ids.length === 0) return [];
  const networks = [...new Set(ids.map((id) => id.slice(0, id.indexOf(':'))))];
  const details = await Promise.all(
    networks.map((network) =>
      Promise.resolve()
        .then(() => getTransitNetwork(network))
        .catch(() => null),
    ),
  );
  const byNetwork = new Map(networks.map((network, i) => [network, details[i] ?? null]));
  return ids.flatMap((id) => {
    const network = id.slice(0, id.indexOf(':'));
    const detail = byNetwork.get(network);
    const line = detail?.lines.find((l) => l.id === id);
    return detail && line ? [{ network, networkName: detail.name, line }] : [];
  });
}
