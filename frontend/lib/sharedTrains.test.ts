import { describe, it, expect } from 'vitest';
import { mergeSharedTrains } from './sharedTrains';
import type { SharedGroupTrain } from './types';

function sharedTrain(overrides: Partial<SharedGroupTrain> = {}): SharedGroupTrain {
  return {
    groupId: 'group-1',
    groupName: 'Family',
    trainSubscriptionId: 1,
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    serviceDate: '2026-08-31',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: null,
    customName: null,
    addedBy: 'user-2',
    addedByName: 'Sam',
    ...overrides,
  };
}

describe('mergeSharedTrains', () => {
  it('keeps one row per train, in the order the API returned them', () => {
    const merged = mergeSharedTrains([
      sharedTrain({ trainSubscriptionId: 7 }),
      sharedTrain({ trainSubscriptionId: 3 }),
    ]);
    expect(merged.map((row) => row.train.trainSubscriptionId)).toEqual([7, 3]);
    expect(merged.map((row) => row.groupNames)).toEqual([['Family'], ['Family']]);
  });

  it('a train shared into two of the caller’s groups collapses into one row carrying both group names', () => {
    const merged = mergeSharedTrains([
      sharedTrain({ trainSubscriptionId: 7, groupId: 'g1', groupName: 'Family' }),
      sharedTrain({ trainSubscriptionId: 7, groupId: 'g2', groupName: 'Commuters' }),
    ]);
    expect(merged).toHaveLength(1);
    expect(merged[0].groupNames).toEqual(['Family', 'Commuters']);
  });

  it('two different groups that happen to share a name are tagged once, not twice', () => {
    const merged = mergeSharedTrains([
      sharedTrain({ trainSubscriptionId: 7, groupId: 'g1', groupName: 'Family' }),
      sharedTrain({ trainSubscriptionId: 7, groupId: 'g2', groupName: 'Family' }),
    ]);
    expect(merged[0].groupNames).toEqual(['Family']);
  });

  it('drops any row for a train the caller already tracks themselves (belt-and-braces against the backend filter)', () => {
    const merged = mergeSharedTrains(
      [sharedTrain({ trainSubscriptionId: 7 }), sharedTrain({ trainSubscriptionId: 8 })],
      new Set([7]),
    );
    expect(merged.map((row) => row.train.trainSubscriptionId)).toEqual([8]);
  });

  it('no shared trains at all is an empty list, not a throw', () => {
    expect(mergeSharedTrains([])).toEqual([]);
  });

  it('keeps the first row’s train fields verbatim (they describe the same subscription)', () => {
    const merged = mergeSharedTrains([
      sharedTrain({ trainSubscriptionId: 7, customName: 'School run', groupName: 'Family' }),
      sharedTrain({ trainSubscriptionId: 7, customName: 'School run', groupName: 'Commuters' }),
    ]);
    expect(merged[0].train.customName).toBe('School run');
    expect(merged[0].train.addedByName).toBe('Sam');
  });
});
