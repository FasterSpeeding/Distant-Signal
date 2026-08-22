import { describe, it, expect } from 'vitest';
import { DISPLAYED_MODES, DISPLAYED_MODES_PARAM } from './modes';

describe('DISPLAYED_MODES', () => {
  it('covers National Rail and the five TfL modes this app ingests', () => {
    expect(DISPLAYED_MODES).toEqual([
      'national-rail',
      'tube',
      'dlr',
      'overground',
      'elizabeth-line',
      'tram',
    ]);
  });

  it('renders as the comma-separated path segment the API expects', () => {
    // Matches SUPPORTED_MODES in crates/api/src/routes/line_status.rs; a
    // mode missing from that list is a 400, not an empty result.
    expect(DISPLAYED_MODES_PARAM).toBe('national-rail,tube,dlr,overground,elizabeth-line,tram');
  });
});
