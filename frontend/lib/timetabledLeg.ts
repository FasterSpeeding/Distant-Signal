import { formatTime } from './dateFormat';
import { transitIncidentHref, transitNetworkName } from './transitStops';
import type { LegTimetable, TransitMode, TripPlanTransitNotice } from './types';

// A timetabled leg (DS-237): a Tube, DLR or tram ride saved from a plan,
// found in its network's timetable on every read. Its times are the
// timetable's, never live: nothing here says on time, late or expected.

/** "train" or "tram", for the sentences below. */
export function vehicleNoun(mode: TransitMode | null | undefined): 'tram' | 'train' {
  return mode === 'tram' ? 'tram' : 'train';
}

/** The caption under a timetabled leg: whose timetable the times are
 * from, that they are not live, and how often the line runs. */
export function timetableCaption(timetable: LegTimetable, mode: TransitMode | null | undefined): string {
  const network = transitNetworkName(timetable.network, '');
  const source = `Times from ${network ? `${network}'s` : 'the'} timetable, not live.`;
  if (timetable.headwayMinutes === undefined) return source;
  const vehicles = vehicleNoun(mode) === 'tram' ? 'Trams' : 'Trains';
  const minutes = timetable.headwayMinutes === 1 ? 'minute' : `${timetable.headwayMinutes} minutes`;
  return `${source} ${vehicles} usually run every ${minutes}.`;
}

/** The note when the timetable no longer has the ride as planned, or
 * `null` when it does (or when there is nothing worth saying). `done`
 * legs say nothing about a missing timetable: it has moved on. */
export function timetableNote(
  timetable: LegTimetable,
  mode: TransitMode | null | undefined,
  done: boolean,
): { text: string; tone: 'neutral' | 'caution' } | null {
  const vehicle = vehicleNoun(mode);
  const line = timetable.line?.name ?? 'line';
  switch (timetable.state) {
    case 'matched':
      return null;
    case 'retimed':
      if (!timetable.scheduledDeparture || !timetable.scheduledArrival) return null;
      return {
        text: `The timetable now has this ${vehicle} at ${formatTime(timetable.scheduledDeparture)} to ${formatTime(timetable.scheduledArrival)}.`,
        tone: 'neutral',
      };
    case 'notInTimetable':
      if (done) return null;
      return {
        text: `This ${vehicle} isn't in the timetable any more. The ${line} still runs between these stops that day, so check the times before you travel.`,
        tone: 'caution',
      };
    case 'noService':
      if (done) return null;
      return {
        text: `The timetable has no ${line} ${vehicle}s between these stops that day. It may be closed for planned work, so check before you travel.`,
        tone: 'caution',
      };
    case 'noTimetable':
      if (done) return null;
      return { text: "We don't have the timetable for that day yet.", tone: 'neutral' };
    default:
      return null;
  }
}

/** Where a line's disruption is told in full: a situation's own page, or
 * for TfL's lines their status page (`/lines/tfl-victoria`). */
export function noticeHref(network: string, notice: TripPlanTransitNotice): string | null {
  if (notice.situationId) return transitIncidentHref(network, notice.situationId);
  if (network === 'tfl' && notice.lineId?.startsWith('tfl:')) {
    return `/lines/tfl-${encodeURIComponent(notice.lineId.slice('tfl:'.length))}`;
  }
  return null;
}

/** A timetabled line's page (`/transit/tfl/lines/victoria`). */
export function timetabledLineHref(timetable: LegTimetable): string | null {
  const id = timetable.line?.id;
  if (!id?.startsWith(`${timetable.network}:`)) return null;
  const local = id.slice(timetable.network.length + 1);
  return `/transit/${encodeURIComponent(timetable.network)}/lines/${encodeURIComponent(local)}`;
}
