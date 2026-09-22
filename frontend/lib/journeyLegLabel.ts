import { formatTime } from './dateFormat';
import { routeLabel } from './stationLabel';
import type { JourneyStop } from './types';

/** The subset of `TrackedTrainState`/`TrainJourneyState` this module reads
 * -- a structural type, not an import of either, so a caller with only a
 * bare pin shape (or none at all) can still call these functions. */
export interface JourneyLegPinNames {
  pinOriginCrs: string | null;
  pinOriginName: string | null;
  pinDestinationCrs: string | null;
  pinDestinationName: string | null;
}

function crsMatch(a: string | null | undefined, b: string | null | undefined): boolean {
  return !!a && !!b && a.toUpperCase() === b.toUpperCase();
}

function findStop(stops: JourneyStop[] | null | undefined, crs: string | null): JourneyStop | null {
  if (!stops || !crs) return null;
  return stops.find((stop) => crsMatch(stop.crs, crs)) ?? null;
}

/** Resolves a display name for one leg endpoint's CRS -- prefers the name
 * `JourneyTimeline`'s own row for that stop already resolved (so a leg's
 * title/label never disagrees with the calling-point table beneath it),
 * falling back to the tracked pin's own origin/destination name when the
 * pin's CRS happens to match this leg's (common case: a leg created
 * directly from a pin/known-train pick carries the same origin the pin
 * did), and finally `null` -- `routeLabel`'s own bare-CRS-code fallback,
 * never a fabricated name. 2026-09-22 UX review findings I16/2.7 (the
 * matched leg card's title) and M18 (the page's own default `<h1>`) both
 * need this same resolution, hence factored out rather than duplicated. */
export function legEndpointName(
  crs: string | null,
  stops: JourneyStop[] | null | undefined,
  pin: JourneyLegPinNames | null,
  end: 'origin' | 'destination',
): string | null {
  const stop = findStop(stops, crs);
  if (stop?.name) return stop.name;
  if (!pin) return null;
  const pinCrs = end === 'origin' ? pin.pinOriginCrs : pin.pinDestinationCrs;
  const pinName = end === 'origin' ? pin.pinOriginName : pin.pinDestinationName;
  return crsMatch(pinCrs, crs) ? pinName : null;
}

/** `"London Kings Cross (KGX) → York (YRK) · 16:00"`-shaped label for one
 * leg (2026-09-22 UX review finding I16/2.7: "the card is about the
 * train, not the leg" -- this is the leg's own identity, meant to replace
 * a headcode as a card's primary title). The departure time comes from
 * THIS LEG's own origin stop specifically -- not the underlying train's
 * overall journey, which can start earlier on a leg that boards partway
 * through a longer service -- and is omitted (bare route only) whenever
 * it isn't known yet (an open leg, or a matched leg with no
 * `journeyStops`). */
export function legRouteAndTime(
  originCrs: string | null,
  destinationCrs: string | null,
  stops: JourneyStop[] | null | undefined,
  pin: JourneyLegPinNames | null,
): string {
  const originName = legEndpointName(originCrs, stops, pin, 'origin');
  const destinationName = legEndpointName(destinationCrs, stops, pin, 'destination');
  const route = routeLabel(originCrs, originName, destinationCrs, destinationName);
  const originStop = findStop(stops, originCrs);
  const departure = originStop ? (originStop.scheduledDeparture ?? originStop.scheduledArrival) : null;
  return departure ? `${route} · ${formatTime(departure)}` : route;
}

/** "arrive 18:22" (a confirmed actual arrival) or "arrive est. 18:22" (an
 * estimate or the bare schedule -- never asserted as confirmed) for the
 * leg's own destination row -- `null` when nothing is known yet. Backs
 * the inter-leg connector (2026-09-22 UX review finding I12/2.6: "Change
 * at York — arrive est. 18:22") -- reading the LEG's own destination stop
 * rather than the train's terminus, for the same reason `legRouteAndTime`
 * above does. */
export function legDestinationArrivalLabel(
  destinationCrs: string | null,
  stops: JourneyStop[] | null | undefined,
): string | null {
  const stop = findStop(stops, destinationCrs);
  if (!stop) return null;
  if (stop.actualArrival) return `arrive ${formatTime(stop.actualArrival)}`;
  const estimate = stop.estimatedArrival ?? stop.scheduledArrival;
  return estimate ? `arrive est. ${formatTime(estimate)}` : null;
}
