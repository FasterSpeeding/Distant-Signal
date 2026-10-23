/** Shared bits of the TfL stop page (`/tfl/stops/[id]`, DS-136). No server
 * imports, so the station search (a Client Component) can link here too. */

/** A NaPTAN stop ID as the api stores them (`common::tfl_stops::is_stop_id`):
 * upper-case letters and digits. */
export const TFL_STOP_ID = /^[A-Z0-9]{4,20}$/;

/** The stop's page. */
export function tflStopHref(id: string): string {
  return `/tfl/stops/${encodeURIComponent(id)}`;
}

/** What each mode is called on a badge. */
export const TFL_MODE_BADGES: Record<string, string> = { tube: 'Underground', dlr: 'DLR', tram: 'Tram' };

export function tflModeBadge(mode: string): string {
  return TFL_MODE_BADGES[mode] ?? mode;
}

/** A TfL line ID that `/lines/tfl-{id}` can name (`/lines/[id]`'s own ID
 * shape). */
export const TFL_LINE_ID = /^[a-z0-9-]+$/;

/** `/lines/tfl-{id}`: the line's status page. */
export function tflLineHref(id: string): string {
  return `/lines/tfl-${id}`;
}

/** How long until a train arrives, as a board says it: "Due" inside a
 * minute, else whole minutes ("1 min", "12 min"). */
export function arrivalDue(timeToStationSecs: number): string {
  const minutes = Math.floor(Math.max(0, timeToStationSecs) / 60);
  return minutes === 0 ? 'Due' : `${minutes} min`;
}

/** The same in words for a screen reader: "due now", "in 1 minute". */
export function arrivalDueSpoken(timeToStationSecs: number): string {
  const minutes = Math.floor(Math.max(0, timeToStationSecs) / 60);
  if (minutes === 0) return 'due now';
  return minutes === 1 ? 'in 1 minute' : `in ${minutes} minutes`;
}

/** The query `/journeys/new` reads to start an untracked leg from a TfL
 * stop ("Add as a journey leg"). */
export const JOURNEY_FROM_TFL_STOP_PARAM = 'fromTflStop';

/** `/journeys/new`, set up for a Tube, DLR or tram leg from `stopId`. */
export function journeyFromTflStopHref(stopId: string): string {
  return `/journeys/new?${new URLSearchParams({ [JOURNEY_FROM_TFL_STOP_PARAM]: stopId }).toString()}`;
}
