import { describe, expect, it } from 'vitest';
import { changedFromPlatform, compactPlatformLabel, isPlatformChange, platformLabel } from './platformLabel';

const unchanged = { platform: '4', plannedPlatform: '4', platformChanged: false };
const changed = { platform: '9', plannedPlatform: '6', platformChanged: true };
const changedUnknownPlan = { platform: '9', plannedPlatform: null, platformChanged: true };

describe('platformLabel', () => {
  it('names the platform, and the planned one when it changed', () => {
    expect(platformLabel(unchanged)).toBe('Platform 4');
    expect(platformLabel(changed)).toBe('Platform 9 (changed from 6)');
    expect(platformLabel({ ...unchanged, platform: '4A' })).toBe('Platform 4A');
  });

  it('reads as an ordinary platform when the planned one is unknown', () => {
    expect(isPlatformChange(changedUnknownPlan)).toBe(false);
    expect(platformLabel(changedUnknownPlan)).toBe('Platform 9');
  });
});

describe('compactPlatformLabel', () => {
  it('drops the word "Platform" but keeps a change in words', () => {
    expect(compactPlatformLabel(unchanged)).toBe('4');
    expect(compactPlatformLabel({ ...unchanged, platform: '10B' })).toBe('10B');
    expect(compactPlatformLabel(changed)).toBe('9 (was 6)');
    expect(compactPlatformLabel(changedUnknownPlan)).toBe('9');
  });
});

describe('changedFromPlatform', () => {
  it('names the booked platform once the api compares against it', () => {
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: '6', platformBasis: 'board' })).toBe('6');
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: null, platformBasis: 'board' })).toBe('9');
  });

  it('keeps the first platform seen for an older response', () => {
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: '6' })).toBe('9');
  });
});
