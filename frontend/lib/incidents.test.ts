import { describe, it, expect } from 'vitest';
import {
  endedDescription,
  incidentIdFromSource,
  incidentState,
  upcomingDescription,
  validityEndedDescription,
} from './incidents';

describe('incidentIdFromSource', () => {
  it('strips the known prefix and returns the raw incident id', () => {
    expect(incidentIdFromSource('knowledgebase-incident-12345')).toBe('12345');
  });

  it('returns null for null', () => {
    expect(incidentIdFromSource(null)).toBeNull();
  });

  it('returns null for undefined', () => {
    expect(incidentIdFromSource(undefined)).toBeNull();
  });

  it('returns null for the shared LDBWS-inferred literal constant', () => {
    expect(incidentIdFromSource('ldbws-sampling')).toBeNull();
  });

  it('returns null for a TfL line-keyed source, even though it superficially looks id-shaped', () => {
    expect(incidentIdFromSource('tfl-line-status-northern')).toBeNull();
  });

  it('returns null for an empty string', () => {
    expect(incidentIdFromSource('')).toBeNull();
  });
});

describe('incidentState', () => {
  it('is cleared when RDM cleared it, whatever else is set', () => {
    expect(incidentState({ isCleared: true, sourceRemovedAt: null })).toBe('cleared');
    expect(incidentState({ isCleared: true, sourceRemovedAt: '2026-10-05T22:55:00+00:00' })).toBe('cleared');
  });

  it('is ended when the feed stopped listing it without clearing it', () => {
    expect(incidentState({ isCleared: false, sourceRemovedAt: '2026-10-05T22:55:00+00:00' })).toBe('ended');
  });

  it('is active otherwise, including against an api that predates sourceRemovedAt', () => {
    expect(incidentState({ isCleared: false, sourceRemovedAt: null })).toBe('active');
    expect(incidentState({ isCleared: false })).toBe('active');
  });

  it("uses the api's state whenever it is there", () => {
    for (const state of ['active', 'upcoming', 'ended', 'cleared'] as const) {
      expect(incidentState({ isCleared: false, sourceRemovedAt: null, state })).toBe(state);
    }
    // An unknown value (a newer api) falls back rather than being trusted.
    expect(incidentState({ isCleared: true, state: 'paused' })).toBe('cleared');
  });

  it('places an uncleared, listed incident by its windows when the api has no state', () => {
    const now = Date.parse('2026-10-20T12:00:00Z');
    const sunday = { fromDate: '2026-10-24T23:00:00+00:00', toDate: '2026-10-25T23:59:00+00:00', isNow: false };
    expect(incidentState({ isCleared: false, validityPeriods: [sunday] }, now)).toBe('upcoming');
    expect(incidentState({ isCleared: false, validityPeriods: [sunday] }, Date.parse('2026-10-25T01:30:00Z'))).toBe(
      'active',
    );
    expect(incidentState({ isCleared: false, validityPeriods: [sunday] }, Date.parse('2026-10-26T00:00:00Z'))).toBe(
      'ended',
    );
    // Cleared and no-longer-listed still win over the windows.
    expect(incidentState({ isCleared: true, validityPeriods: [sunday] }, now)).toBe('cleared');
    expect(
      incidentState({ isCleared: false, sourceRemovedAt: '2026-10-19T22:55:00Z', validityPeriods: [sunday] }, now),
    ).toBe('ended');
  });
});

describe('upcomingDescription and validityEndedDescription', () => {
  it('name the time in UK time, across the clock change', () => {
    // 23:00 UTC on Saturday 24 October is midnight BST on the Sunday.
    expect(upcomingDescription('2026-10-24T23:00:00+00:00')).toBe('Not in effect yet: starts 25 Oct 2026, 00:00');
    // 23:59 UTC on Sunday 25 October is 23:59 GMT, after the clocks went back.
    expect(validityEndedDescription('2026-10-25T23:59:00+00:00')).toBe('Its validity period ended 25 Oct 2026, 23:59');
  });
});

describe('endedDescription', () => {
  it('names when the source last listed it, in UK time', () => {
    expect(endedDescription('2026-10-05T22:55:00+00:00')).toBe(
      'No longer listed by the source since 5 Oct 2026, 23:55',
    );
  });
});
