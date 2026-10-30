import type { PlatformBasis, PlatformStatus } from './types';

/** A calling point's platform facts, as the api sends them on a
 * `JourneyStop` (a departure-board row carries only the first three). */
export interface PlatformFacts {
  platform: string | null;
  plannedPlatform: string | null;
  platformChanged: boolean;
  platformStatus?: PlatformStatus | null | undefined;
  bookedPlatform?: string | null | undefined;
  platformBasis?: PlatformBasis | null | undefined;
}

/** What `PlatformBadge` shows, one state per look:
 *
 * - `booked`: only the timetable's platform is known, so it is provisional.
 * - `live`: a live board shows it, unchanged; `confirmed` once the api
 *   says the platform is confirmed (`platformBasis: 'confirmed'`).
 * - `changed`: it differs from the platform the stop was meant to use,
 *   which is named (`previous`).
 * - `cancelled`: the train no longer calls there. */
export type PlatformDisplay =
  | { state: 'booked'; platform: string }
  | { state: 'live'; platform: string; confirmed: boolean }
  | { state: 'changed'; platform: string; previous: string; confirmed: boolean }
  | { state: 'cancelled'; platform: string };

export type PlatformState = PlatformDisplay['state'];

/** The platform a changed platform is "changed from" (DS-127): the
 * timetable's booked platform once the api compares against it (it sends
 * `platformBasis`), else the first platform seen, as before. */
export function changedFromPlatform(stop: {
  plannedPlatform: string | null;
  bookedPlatform?: string | null | undefined;
  platformBasis?: string | null | undefined;
}): string | null {
  return stop.platformBasis !== undefined ? (stop.bookedPlatform ?? stop.plannedPlatform) : stop.plannedPlatform;
}

/** Which state a stop's platform is in, or `null` when no platform is
 * known (never a fabricated one). A change with no earlier platform to
 * name reads as an ordinary platform; any `platformStatus` but
 * `cancelled`, and any `platformBasis` but `booked` and `confirmed`,
 * reads as a live board's. */
export function platformDisplay(facts: PlatformFacts): PlatformDisplay | null {
  const { platform, platformBasis = null } = facts;
  if (platform === null) {
    return platformBasis === 'booked' && facts.bookedPlatform
      ? { state: 'booked', platform: facts.bookedPlatform }
      : null;
  }
  if (facts.platformStatus === 'cancelled') return { state: 'cancelled', platform };
  const confirmed = platformBasis === 'confirmed';
  const previous = changedFromPlatform(facts);
  if (facts.platformChanged && previous !== null && previous !== platform) {
    return { state: 'changed', platform, previous, confirmed };
  }
  return { state: 'live', platform, confirmed };
}

/** What a screen reader hears: every fact the badge's look carries, in
 * words. "Platform 4 (booked, not confirmed)", "Platform 4",
 * "Platform 4 (confirmed)", "Platform 9 (changed from 6)",
 * "Platform 9 (confirmed, changed from 6)", "Platform 4 (cancelled)". */
export function spokenPlatformLabel(display: PlatformDisplay): string {
  const name = `Platform ${display.platform}`;
  switch (display.state) {
    case 'booked':
      return `${name} (booked, not confirmed)`;
    case 'cancelled':
      return `${name} (cancelled)`;
    case 'changed':
      return `${name} (${display.confirmed ? 'confirmed, ' : ''}changed from ${display.previous})`;
    case 'live':
      return display.confirmed ? `${name} (confirmed)` : name;
  }
}

/** The booked badge's tooltip (DS-127). */
export const BOOKED_PLATFORM_TOOLTIP = 'Booked platform, not yet shown on station boards';
/** The cancelled badge's tooltip. */
export const CANCELLED_PLATFORM_TOOLTIP = 'Cancelled: the train no longer calls at this platform';

/** The badge's hover tooltip (`title`), a mouse user's extra: everything
 * in it is also on screen or in the spoken label. None for a plain live
 * platform, which needs no explaining. */
export function platformTooltip(display: PlatformDisplay): string | undefined {
  switch (display.state) {
    case 'booked':
      return BOOKED_PLATFORM_TOOLTIP;
    case 'cancelled':
      return CANCELLED_PLATFORM_TOOLTIP;
    case 'changed':
      return `Platform ${display.platform}, changed from ${display.previous} (${display.confirmed ? 'confirmed' : 'expected, not yet confirmed'})`;
    case 'live':
      return display.confirmed ? 'Confirmed platform' : undefined;
  }
}
