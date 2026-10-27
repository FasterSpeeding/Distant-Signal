import { notFound } from 'next/navigation';
import { ApiNotFoundError, getTransitNetwork } from './api';
import type { TransitLine, TransitLineStatus } from './types';

// In their own module so a Client Component (`lib/stationSearch.ts`) can
// check ids without pulling in this module's server-only `./api`.
export { TRANSIT_ID, TRANSIT_STOP_ID } from './transitIds';
export { lineColour, SCHEDULED_CAPTION } from './transitStops';

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
