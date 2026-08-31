import { describe, it, expect } from 'vitest';
import { DISPLAYED_MODES, DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from './modes';

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

describe('MERGED_TFL_LINE_IDS', () => {
  it('covers Elizabeth line and all six London Overground lines', () => {
    // Mirrors TFL_TO_NR_LINE_ID in crates/common/src/lib.rs -- see
    // docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
    // Areas 1 and 2. A pinned line missing from this list would double-count
    // on the home dashboard (frontend/app/page.tsx) once both its NR and TfL
    // rows exist.
    expect(MERGED_TFL_LINE_IDS).toEqual([
      'tfl-elizabeth',
      'tfl-liberty',
      'tfl-lioness',
      'tfl-mildmay',
      'tfl-suffragette',
      'tfl-weaver',
      'tfl-windrush',
    ]);
  });
});
