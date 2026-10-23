import { describe, expect, it } from 'vitest';
import { DEFAULT_JOURNEY_ENTRY_MODE, parseJourneyEntryMode } from './journeyEntryMode';

describe('parseJourneyEntryMode', () => {
  it('defaults to the planner', () => {
    expect(DEFAULT_JOURNEY_ENTRY_MODE).toBe('plan');
    expect(parseJourneyEntryMode(undefined)).toEqual({ entryMode: 'plan' });
  });

  it.each(['known', 'plan', 'untracked'] as const)('honours ?mode=%s', (mode) => {
    expect(parseJourneyEntryMode(mode)).toEqual({ entryMode: mode });
  });

  it.each(['pick', 'window'] as const)("takes /track's ?mode=%s as I know my route, on that half", (mode) => {
    expect(parseJourneyEntryMode(mode)).toEqual({ entryMode: 'known', knownRouteMode: mode });
  });

  it('reads the first of a repeated value', () => {
    expect(parseJourneyEntryMode(['window', 'untracked'])).toEqual({ entryMode: 'known', knownRouteMode: 'window' });
  });

  it.each(['', 'KNOWN', 'tube', 'pin'])('treats ?mode=%j as absent', (mode) => {
    expect(parseJourneyEntryMode(mode)).toEqual({ entryMode: 'plan' });
  });
});
