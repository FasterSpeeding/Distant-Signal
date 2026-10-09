import { formatTime } from './dateFormat';

/** How a commuter names a train: "08:42 Woking to London Waterloo" (the
 * departure time from its origin, then the route). Without a known origin
 * and destination there is nothing a commuter would recognise, so it is
 * null and the caller falls back. Times are London time. */
export function trainName({
  departure,
  origin,
  destination,
}: {
  departure: string | null | undefined;
  origin: string | null | undefined;
  destination: string | null | undefined;
}): string | null {
  if (!origin || !destination) return null;
  const route = `${origin} to ${destination}`;
  return departure ? `${formatTime(departure)} ${route}` : route;
}

/** The enthusiast identifiers, labelled, for secondary text under a train's
 * name: "Headcode 1S00 · UID W12345". Null when neither is known. */
export function trainIdentifiers({
  headcode,
  uid,
}: {
  headcode: string | null | undefined;
  uid: string | null | undefined;
}): string | null {
  const parts = [headcode ? `Headcode ${headcode}` : null, uid ? `UID ${uid}` : null].filter(
    (part): part is string => part !== null,
  );
  return parts.length > 0 ? parts.join(' · ') : null;
}
