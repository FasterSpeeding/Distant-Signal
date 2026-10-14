import { notFound } from 'next/navigation';
import { ApiNotFoundError } from './api';
import type { TransitLine } from './types';

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
