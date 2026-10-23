import { describe, expect, it } from 'vitest';
import { followedFromLabel } from './followedFrom';
import type { FollowedFrom } from './types';

const group: FollowedFrom = {
  via: 'group',
  ownerName: 'Olive',
  ownerTag: null,
  groupId: 'grp-1',
  groupName: 'Family',
  followedAt: '2026-10-22T08:00:00Z',
};

describe('followedFromLabel', () => {
  it("names the owner of a journey followed through a group, as the group's own credit does", () => {
    expect(followedFromLabel(group)).toBe('Shared by Olive');
    expect(followedFromLabel({ ...group, ownerName: null, ownerTag: 'a1b2c3' })).toBe('Shared by a member (#a1b2c3)');
  });

  it('names no one for a share link, which shows nothing about its owner', () => {
    expect(
      followedFromLabel({ ...group, via: 'link', ownerName: null, ownerTag: null, groupId: null, groupName: null }),
    ).toBe('Shared with you by link');
  });
});
