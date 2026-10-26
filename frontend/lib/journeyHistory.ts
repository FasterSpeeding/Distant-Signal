import { londonDayStartIso } from './londonWallClock';
import type { MergedSharedTrain } from './sharedTrains';
import { trackedTrainRowStatus } from './trackedRowStatus';
import type { JourneyListItem, TrackedTrainListItem } from './types';
import {
  splitByStatus,
  splitJourneyRows,
  splitTrainRows,
  type UpcomingPastSplit,
  type WithStatus,
} from './upcomingPast';

/** The Journeys page's one ordering rule (DS-178), for both of its lists:
 *
 * - Upcoming (still to come or under way): soonest departure first.
 * - Past (travelled): newest departure first.
 *
 * "Departure" is when the travel happens, not when it was saved or shared:
 * a journey's first leg's departure (the api's `departsAt`: the timetabled
 * departure for a train leg, else its pinned departure, else the London
 * start of its date; an untracked leg's planned departure), a lone train's
 * departure from the traveller's stop, else the start of its date. A tie
 * goes to the newest journey id; between kinds, a journey first.
 *
 * The caller's own journeys and every journey shared with them (followed,
 * or shared into one of their groups) are ONE list from the api, already
 * merged and ordered under one cap (`GET /Journeys/mine?include=followed,
 * shared`), so they are never re-sorted here. Only the rarer trains with
 * no journey (tracked before every train became a one-leg journey, or
 * shared into a group as a bare train) come from other lists; this merges
 * them in by the same key, so nothing sits in a block of its own after the
 * journeys. */
export type JourneysPageEntry =
  | ({ kind: 'journey'; key: string; journey: JourneyListItem } & WithStatus)
  | ({ kind: 'train'; key: string; train: TrackedTrainListItem } & WithStatus)
  | ({ kind: 'sharedTrain'; key: string; row: MergedSharedTrain } & WithStatus);

/** When a journey sets off, in ms: the api's `departsAt`, else (an older
 * api) the London start of its first day. */
export function journeyDepartsAt(journey: JourneyListItem): number {
  return Date.parse(journey.departsAt ?? londonDayStartIso(journey.firstServiceDate ?? journey.serviceDate));
}

/** When a train sets off, in ms: the api's `departsAt` for one of the
 * caller's own, else its pinned departure, else the London start of its
 * service date. */
export function trainDepartsAt(
  train: Pick<TrackedTrainListItem, 'pinScheduledDeparture' | 'serviceDate'> & {
    departsAt?: string;
  },
): number {
  return Date.parse(train.departsAt ?? train.pinScheduledDeparture ?? londonDayStartIso(train.serviceDate));
}

/** Merges lists that are each already in `order`'s order into one, never
 * reordering within a list: at each step the head that comes first by
 * `departs` wins, an earlier list on a tie. */
export function mergeTravelOrdered<T>(
  lists: readonly (readonly T[])[],
  departs: (row: T) => number,
  order: 'soonest' | 'newest',
): T[] {
  const heads = lists.map(() => 0);
  const merged: T[] = [];
  for (;;) {
    let best = -1;
    let bestAt = 0;
    lists.forEach((list, index) => {
      const head = list[heads[index] ?? 0];
      if (head === undefined) return;
      const at = departs(head);
      const better = best === -1 || (order === 'soonest' ? at < bestAt : at > bestAt);
      if (better) {
        best = index;
        bestAt = at;
      }
    });
    if (best === -1) return merged;
    const list = lists[best] ?? [];
    const position = heads[best] ?? 0;
    const row = list[position];
    if (row !== undefined) merged.push(row);
    heads[best] = position + 1;
  }
}

function entryDepartsAt(entry: JourneysPageEntry): number {
  switch (entry.kind) {
    case 'journey':
      return journeyDepartsAt(entry.journey);
    case 'train':
      return trainDepartsAt(entry.train);
    case 'sharedTrain':
      return trainDepartsAt(entry.row.train);
  }
}

/** Shared bare trains come newest share first; this puts them in the
 * section's order first, newest subscription on a tie. */
function sortSharedTrains(entries: JourneysPageEntry[], order: 'soonest' | 'newest'): JourneysPageEntry[] {
  return [...entries].sort((a, b) => {
    const diff = entryDepartsAt(a) - entryDepartsAt(b);
    if (diff !== 0) return order === 'soonest' ? diff : -diff;
    const aId = a.kind === 'sharedTrain' ? a.row.train.trainSubscriptionId : 0;
    const bId = b.kind === 'sharedTrain' ? b.row.train.trainSubscriptionId : 0;
    return bId - aId;
  });
}

/** The Journeys page's two lists, each one merged list in the order above.
 * `journeys` is `GET /Journeys/mine`'s rows (own, followed and shared, in
 * the api's order), `trains` the caller's own trains in no listed journey
 * (`GET /Train/mine`'s order), `sharedTrains` the bare trains shared with
 * them. `today` is the London day (`londonDayKey`). */
export function journeysPageSections(
  {
    journeys,
    trains,
    sharedTrains,
  }: {
    journeys: readonly JourneyListItem[];
    trains: readonly TrackedTrainListItem[];
    sharedTrains: readonly MergedSharedTrain[];
  },
  today: string,
): UpcomingPastSplit<JourneysPageEntry> {
  const journeyItems = splitJourneyRows(journeys, today);
  const trainItems = splitTrainRows(trains, today);
  const sharedItems = splitByStatus(
    sharedTrains.map((row) => ({ row, status: trackedTrainRowStatus(row.train, today) })),
    (item) => item.status.finished,
  );
  const section = (half: 'upcoming' | 'past'): JourneysPageEntry[] => {
    const order = half === 'upcoming' ? 'soonest' : 'newest';
    const lists: JourneysPageEntry[][] = [
      journeyItems[half].map(({ journey, status }) => ({
        kind: 'journey' as const,
        key: `journey-${String(journey.id)}`,
        journey,
        status,
      })),
      trainItems[half].map(({ train, status }) => ({
        kind: 'train' as const,
        key: `train-${String(train.id)}`,
        train,
        status,
      })),
      sortSharedTrains(
        sharedItems[half].map(({ row, status }) => ({
          kind: 'sharedTrain' as const,
          key: `shared-train-${String(row.train.trainSubscriptionId)}`,
          row,
          status,
        })),
        order,
      ),
    ];
    return mergeTravelOrdered(lists, entryDepartsAt, order);
  };
  return { upcoming: section('upcoming'), past: section('past') };
}
