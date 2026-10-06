import { describe, it, expect } from 'vitest';
import { endedDescription, incidentIdFromSource, incidentState } from './incidents';

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
});

describe('endedDescription', () => {
  it('names when the source last listed it, in UK time', () => {
    expect(endedDescription('2026-10-05T22:55:00+00:00')).toBe(
      'No longer listed by the source since 5 Oct 2026, 23:55',
    );
  });
});
