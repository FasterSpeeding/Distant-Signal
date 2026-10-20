import { describe, it, expect } from 'vitest';
import { splitJourneyTrains } from './journeyTrains';
import type { JourneyListItem, TrackedTrainListItem } from './types';

function train(id: number, journeyIds?: number[]): TrackedTrainListItem {
  return {
    id,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: null,
    resolutionStatus: 'resolved',
    trainUid: null,
    status: null,
    delayMinutes: null,
    trackedAt: '2026-09-21T12:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    ...(journeyIds === undefined ? {} : { journeyIds }),
  };
}

function journey(id: number, trainSubscriptionId: number | null = null): JourneyListItem {
  return {
    id,
    customName: null,
    createdAt: '2026-09-21T12:00:00Z',
    legId: id * 10,
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    serviceDate: '2026-09-22',
    originName: null,
    destinationName: null,
    matchMode: 'manual',
    trainSubscriptionId,
    resolutionStatus: null,
    status: null,
    delayMinutes: null,
  };
}

describe('splitJourneyTrains', () => {
  it("folds every leg's train into its journey, whichever leg is current", () => {
    const split = splitJourneyTrains(
      [train(1, [100]), train(2, [100]), train(3, [100]), train(4, [])],
      [journey(100, 1)],
    );
    expect(split.standalone.map((t) => t.id)).toEqual([4]);
    expect(split.trainIdsByJourney.get(100)).toEqual([1, 2, 3]);
  });

  it('puts a train in two journeys under one of them only, the first listed in journeyIds', () => {
    const split = splitJourneyTrains([train(1, [100, 200])], [journey(200), journey(100)]);
    expect(split.standalone).toEqual([]);
    expect(split.trainIdsByJourney.get(100)).toEqual([1]);
    expect(split.trainIdsByJourney.has(200)).toBe(false);
  });

  it('keeps a train standalone when its journey is not listed', () => {
    const split = splitJourneyTrains([train(1, [300])], [journey(100)]);
    expect(split.standalone.map((t) => t.id)).toEqual([1]);
  });

  it('folds nothing when the journeys fetch failed', () => {
    const split = splitJourneyTrains([train(1, [100])], null);
    expect(split.standalone.map((t) => t.id)).toEqual([1]);
    expect(split.trainIdsByJourney.size).toBe(0);
  });

  it("falls back to each journey's current leg for an older backend without journeyIds", () => {
    const split = splitJourneyTrains([train(1), train(2)], [journey(100, 1)]);
    expect(split.standalone.map((t) => t.id)).toEqual([2]);
    expect(split.trainIdsByJourney.get(100)).toEqual([1]);
  });
});
