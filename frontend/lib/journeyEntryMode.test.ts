import { describe, expect, it } from 'vitest';
import { DEFAULT_JOURNEY_ENTRY_MODE, entryModeSearch, parseJourneyEntryMode } from './journeyEntryMode';

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

describe('entryModeSearch', () => {
  it("names a non-default option in ?mode=, keeping the planner's search", () => {
    expect(entryModeSearch('?origin=EUS&destination=MKC', 'known')).toBe('?origin=EUS&destination=MKC&mode=known');
    expect(entryModeSearch('?mode=window', 'untracked')).toBe('?mode=untracked');
  });

  it('drops ?mode= for the planner, the default', () => {
    expect(entryModeSearch('?origin=EUS&mode=known', 'plan')).toBe('?origin=EUS');
    expect(entryModeSearch('?mode=known', 'plan')).toBe('');
    expect(entryModeSearch('', 'plan')).toBe('');
  });

  it('keeps a TfL stop only on Tube, bus or walk', () => {
    expect(entryModeSearch('?fromTflStop=940GZZLUKSX', 'untracked')).toBe('?fromTflStop=940GZZLUKSX&mode=untracked');
    expect(entryModeSearch('?fromTflStop=940GZZLUKSX&mode=untracked', 'known')).toBe('?mode=known');
  });
});
