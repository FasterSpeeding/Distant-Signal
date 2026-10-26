import { describe, expect, it } from 'vitest';
import { journeyDepartsAt, journeysPageSections, mergeTravelOrdered, trainDepartsAt } from './journeyHistory';
import type { MergedSharedTrain } from './sharedTrains';
import type { JourneyListItem, SharedGroupTrain, TrackedTrainListItem } from './types';

const TODAY = '2026-08-31';

function journey(id: number, departsAt: string, travelled: boolean, extra: Partial<JourneyListItem> = {}) {
  return {
    id,
    customName: null,
    createdAt: '2026-08-01T00:00:00Z',
    legId: id,
    originCrs: 'WAT',
    destinationCrs: 'RDG',
    serviceDate: departsAt.slice(0, 10),
    originName: null,
    destinationName: null,
    matchMode: 'manual',
    trainSubscriptionId: null,
    resolutionStatus: null,
    status: null,
    delayMinutes: null,
    travelled,
    departsAt,
    ...extra,
  } satisfies JourneyListItem;
}

function train(id: number, departsAt: string, travelled: boolean): TrackedTrainListItem {
  return {
    id,
    serviceDate: departsAt.slice(0, 10),
    pinOriginCrs: 'GLD',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: departsAt,
    resolutionStatus: 'resolved',
    trainUid: 'C1',
    status: travelled ? 'completed' : null,
    delayMinutes: null,
    trackedAt: '2026-08-01T00:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    travelled,
    departsAt,
  };
}

function sharedTrain(id: number, pinScheduledDeparture: string, serviceDate: string): MergedSharedTrain {
  const row: SharedGroupTrain = {
    groupId: 'g',
    groupName: 'Family',
    trainSubscriptionId: id,
    pinOriginCrs: 'PAD',
    pinDestinationCrs: 'RDG',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture,
    serviceDate,
    resolutionStatus: 'resolved',
    trainUid: 'S1',
    status: 'completed',
    delayMinutes: null,
    customName: null,
    addedBy: 'u',
    addedByName: 'Sam',
    addedByTag: null,
  };
  return { train: row, groupNames: ['Family'] };
}

describe('mergeTravelOrdered', () => {
  const at = (value: number) => value;

  it('interleaves sorted lists without reordering any of them', () => {
    expect(
      mergeTravelOrdered(
        [
          [9, 7, 3],
          [8, 4],
          [6, 1],
        ],
        at,
        'newest',
      ),
    ).toEqual([9, 8, 7, 6, 4, 3, 1]);
    expect(
      mergeTravelOrdered(
        [
          [1, 5],
          [2, 3, 9],
        ],
        at,
        'soonest',
      ),
    ).toEqual([1, 2, 3, 5, 9]);
  });

  it('keeps a list in its own order even where it disagrees with the key', () => {
    // The api's order is the truth for its list: never re-sorted here.
    expect(mergeTravelOrdered([[5, 9, 1], [7]], at, 'newest')).toEqual([7, 5, 9, 1]);
  });

  it('breaks a tie in favour of the earlier list', () => {
    const rows = mergeTravelOrdered([[{ id: 'journey', at: 5 }], [{ id: 'train', at: 5 }]], (row) => row.at, 'newest');
    expect(rows.map((row) => row.id)).toEqual(['journey', 'train']);
  });

  it('handles empty lists', () => {
    expect(mergeTravelOrdered<number>([[], []], at, 'newest')).toEqual([]);
    expect(mergeTravelOrdered([[], [2, 1]], at, 'newest')).toEqual([2, 1]);
  });
});

describe('departure keys', () => {
  it("reads a journey's departsAt, else the London start of its first day", () => {
    expect(journeyDepartsAt(journey(1, '2026-08-31T09:00:00Z', false))).toBe(Date.parse('2026-08-31T09:00:00Z'));
    const { departsAt: _dropped, ...older } = journey(1, '2026-08-31T09:00:00Z', false, {
      firstServiceDate: '2026-07-01',
    });
    // British Summer Time: London midnight is 23:00 UTC the day before.
    expect(journeyDepartsAt(older)).toBe(Date.parse('2026-06-30T23:00:00Z'));
  });

  it("reads a train's departsAt, else its pin, else the start of its date", () => {
    expect(trainDepartsAt({ departsAt: '2026-08-31T10:00:00Z', pinScheduledDeparture: null, serviceDate: TODAY })).toBe(
      Date.parse('2026-08-31T10:00:00Z'),
    );
    expect(trainDepartsAt({ pinScheduledDeparture: '2026-08-31T11:00:00Z', serviceDate: TODAY })).toBe(
      Date.parse('2026-08-31T11:00:00Z'),
    );
    expect(trainDepartsAt({ pinScheduledDeparture: null, serviceDate: '2026-12-01' })).toBe(
      Date.parse('2026-12-01T00:00:00Z'),
    );
  });
});

describe('journeysPageSections', () => {
  it('lists Past newest first, journeys own and shared as the api sent them, trains merged in', () => {
    const journeys = [
      journey(10, '2026-09-02T08:00:00Z', false),
      journey(11, '2026-09-05T08:00:00Z', false, { sharedVia: [] }),
      journey(12, '2026-08-30T08:00:00Z', true),
      journey(13, '2026-08-20T08:00:00Z', true, {
        sharedVia: [{ groupId: 'g', groupName: 'Family', addedByName: 'Sam', addedByTag: null, addedAt: '' }],
      }),
      journey(14, '2026-08-10T08:00:00Z', true),
    ];
    const trains = [train(20, '2026-09-03T08:00:00Z', false), train(21, '2026-08-25T08:00:00Z', true)];
    // Newest share first from the group endpoint, so out of travel order.
    const shared = [
      sharedTrain(31, '2026-08-05T08:00:00Z', '2026-08-05'),
      sharedTrain(30, '2026-08-29T08:00:00Z', '2026-08-29'),
    ];

    const sections = journeysPageSections({ journeys, trains, sharedTrains: shared }, TODAY);

    expect(sections.upcoming.map((entry) => entry.key)).toEqual(['journey-10', 'train-20', 'journey-11']);
    expect(sections.past.map((entry) => entry.key)).toEqual([
      'journey-12',
      'shared-train-30',
      'train-21',
      'journey-13',
      'journey-14',
      'shared-train-31',
    ]);
  });

  it('puts a journey before a train leaving at the same moment, and a newer shared train first on a tie', () => {
    const sections = journeysPageSections(
      {
        journeys: [journey(1, '2026-08-30T08:00:00Z', true)],
        trains: [train(2, '2026-08-30T08:00:00Z', true)],
        sharedTrains: [
          sharedTrain(40, '2026-08-30T08:00:00Z', '2026-08-30'),
          sharedTrain(41, '2026-08-30T08:00:00Z', '2026-08-30'),
        ],
      },
      TODAY,
    );
    expect(sections.past.map((entry) => entry.key)).toEqual([
      'journey-1',
      'train-2',
      'shared-train-41',
      'shared-train-40',
    ]);
  });
});
