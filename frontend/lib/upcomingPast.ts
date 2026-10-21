import { journeyRowStatus, trackedTrainRowStatus, type TrackedRowStatus } from './trackedRowStatus';
import type { JourneyListItem, TrackedTrainListItem } from './types';

/** The Journeys page's cut-off between Upcoming and Past (DS-115).
 *
 * The rule is the one the home page and the api already share for "is this
 * journey over": its last leg has arrived (a reported arrival, else the
 * timetabled arrival plus any running delay, else plus
 * `TRAVEL_ARRIVAL_GRACE_MINUTES`) or every leg is finished. The api applies
 * it in SQL (`travel_order::train_arrived_sql`) to order `GET
 * /Journeys/mine` and `GET /Train/mine`: everything still to come or under
 * way, soonest first, THEN everything travelled, most recent first. The
 * home page applies it to legs read in full (`lib/homeJourneys.ts`,
 * `legIsOver`).
 *
 * A list row carries no times, so this does not recompute the rule. It
 * finds where the api's order crosses over, from what each row does say:
 *
 * - a row that starts after today (London) has not been travelled, so it
 *   and every row before it are Upcoming;
 * - a row that is over is Past, and so is every row after it. "Over" is
 *   the row's own status (`TrackedRowStatus.finished`, DS-114): every leg
 *   finished by the summary counts (`finishedLegCount`, which the api
 *   counts by the same arrival rule), or its day has passed;
 * - a row that starts on an earlier day than the row before it is Past,
 *   with every row after it: still-to-come rows only ever get later.
 *
 * Upcoming includes a journey under way now: it is not over yet. */
export interface UpcomingPastSplit<T> {
  upcoming: T[];
  past: T[];
}

/** What the cut-off reads from one row. */
interface TravelRow {
  /** The London day the row's travel starts ("2026-10-21"). */
  startDay: string;
  /** The row says it is over (`TrackedRowStatus.finished`). */
  over: boolean;
}

/** Splits rows that come in the api's travel order (see above) at the
 * first row that is Past. Each half keeps the api's order. */
export function splitTravelOrdered<T>(
  rows: readonly T[],
  read: (row: T) => TravelRow,
  today: string,
): UpcomingPastSplit<T> {
  const travel = rows.map(read);
  // The last row known to be still to come: nothing before it is Past.
  let lastUpcoming = -1;
  travel.forEach((row, index) => {
    if (row.startDay > today) lastUpcoming = index;
  });
  const found = travel.findIndex(
    (row, index) =>
      index > lastUpcoming && (row.over || (index > 0 && row.startDay < (travel[index - 1]?.startDay ?? ''))),
  );
  const cut = found === -1 ? rows.length : found;
  return { upcoming: rows.slice(0, cut), past: rows.slice(cut) };
}

/** Splits rows whose order says nothing about travel (trains shared into a
 * group come in the group endpoint's own order), by each row's own status. */
export function splitByStatus<T>(rows: readonly T[], over: (row: T) => boolean): UpcomingPastSplit<T> {
  return { upcoming: rows.filter((row) => !over(row)), past: rows.filter((row) => over(row)) };
}

/** A row with its badge status, which the split reads too. */
export interface WithStatus {
  status: TrackedRowStatus;
}

/** `GET /Journeys/mine`'s rows, in its travel order, split. `today` is the
 * London day (`londonDayKey`). */
export function splitJourneyRows(
  journeys: readonly JourneyListItem[],
  today: string,
): UpcomingPastSplit<{ journey: JourneyListItem } & WithStatus> {
  return splitTravelOrdered(
    journeys.map((journey) => ({ journey, status: journeyRowStatus(journey, today) })),
    ({ journey, status }) => ({ startDay: journey.firstServiceDate ?? journey.serviceDate, over: status.finished }),
    today,
  );
}

/** `GET /Train/mine`'s rows (the ones not folded into a journey), in its
 * travel order, split. A lone train is a one-leg journey. */
export function splitTrainRows(
  trains: readonly TrackedTrainListItem[],
  today: string,
): UpcomingPastSplit<{ train: TrackedTrainListItem } & WithStatus> {
  return splitTravelOrdered(
    trains.map((train) => ({ train, status: trackedTrainRowStatus(train, today) })),
    ({ train, status }) => ({ startDay: train.serviceDate, over: status.finished }),
    today,
  );
}
