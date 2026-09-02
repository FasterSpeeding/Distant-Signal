import { describe, it, expect } from 'vitest';
import { incidentSourceLabel } from './incidentSource';

describe('incidentSourceLabel', () => {
  it('labels a real-shaped 32-hex knowledgebase incident id', () => {
    expect(incidentSourceLabel('knowledgebase-incident-EC354602568440DB82B2835903B7A5FE')).toBe(
      'National Rail Knowledgebase',
    );
  });

  it('labels the LDBWS sampling literal', () => {
    expect(incidentSourceLabel('ldbws-sampling')).toBe('Live departure boards');
  });

  it('labels a TfL line-status source', () => {
    expect(incidentSourceLabel('tfl-line-status-northern')).toBe('Transport for London');
  });

  it('returns null for a null source', () => {
    expect(incidentSourceLabel(null)).toBeNull();
  });

  it('returns null for an undefined source', () => {
    expect(incidentSourceLabel(undefined)).toBeNull();
  });

  it('returns null for an unrecognised source, rather than the raw string', () => {
    expect(incidentSourceLabel('some-future-pipeline-source')).toBeNull();
  });
});
