import { describe, it, expect } from 'vitest';
import { incidentIdFromSource } from './incidents';

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
