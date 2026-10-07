/**
 * `GET /public/trains/search` rows (and the journey-leg candidates built on
 * them) as `ServiceRow` input: the adapter the line page's station-pair
 * search first used (`pairRow`), shared by the station board, `/trains` and
 * the journey-leg candidates.
 *
 * Live state, the station's day offset and the origin's name are optional
 * on the wire (a backend that predates them sends none): without them a
 * row reads "Scheduled", with no next-day marker, and names its origin by
 * code.
 */
import { operatorLabel } from './displayLabels';
import type { LineTrainSummary, TrainSearchResult } from './types';

/** A search row as a line-summary row. `known` is the same train from a
 * line summary, whose live state, scope and direction fill in what the
 * search row lacks. */
export function searchRowSummary(row: TrainSearchResult, known?: LineTrainSummary): LineTrainSummary {
  const live = row.live ? { ...row.live, lastReportedLocation: null } : (known?.live ?? null);
  return {
    uid: row.uid,
    operator: row.operator ?? null,
    serviceMode: row.serviceMode ?? known?.serviceMode ?? 'train',
    liveTracking: row.liveTracking ?? known?.liveTracking ?? null,
    scope: known?.scope ?? null,
    direction: known?.direction ?? null,
    lineDue: null,
    origin: row.originCrs ? { crs: row.originCrs, name: row.originName ?? null } : null,
    destination: row.destinationCrs ? { crs: row.destinationCrs, name: row.destinationName ?? null } : null,
    onLineStops: [],
    live,
  };
}

/** The row's time at the searched station, `HH:MM`: the public departure,
 * else the working one. */
export function searchRowTime(row: Pick<TrainSearchResult, 'publicDeparture' | 'scheduled'>): string {
  return (row.publicDeparture ?? row.scheduled ?? '--:--').slice(0, 5);
}

/** The row's dimmed second line: where the train comes from ("Starts
 * here" when that is the searched station) and, given an ATOC code to
 * name lookup, its operator. `undefined` when there is neither. */
export function searchRowDetails(
  row: Pick<TrainSearchResult, 'originCrs' | 'originName' | 'stationCrs' | 'operator'>,
  operatorNames?: ReadonlyMap<string, string>,
): string | undefined {
  const parts: string[] = [];
  if (row.originCrs) {
    parts.push(row.originCrs === row.stationCrs ? 'Starts here' : `From ${row.originName ?? row.originCrs}`);
  }
  if (row.operator && operatorNames) parts.push(operatorLabel(row.operator, operatorNames));
  return parts.length > 0 ? parts.join(' · ') : undefined;
}
