import { describe, it, expect } from 'vitest';
import {
  DISPLAYED_MODES,
  DISPLAYED_MODES_PARAM,
  MERGED_TFL_LINE_IDS,
  MODE_TO_COUNTRY,
  countryForMode,
  countryForReport,
  type Country,
} from './modes';

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

describe('MODE_TO_COUNTRY / countryForMode / countryForReport', () => {
  it('MODE_TO_COUNTRY has no real entries yet -- Ireland Tier C has not shipped', () => {
    // Deliberately asserts emptiness, not just "doesn't throw" -- a
    // regression here would mean someone guessed at an unconfirmed
    // modeName value (see this table's own doc comment for why that must
    // not happen before a real poller exists).
    expect(MODE_TO_COUNTRY).toEqual({});
  });

  it('defaults every currently-displayed mode to Gb', () => {
    for (const mode of DISPLAYED_MODES) {
      expect(countryForMode(mode)).toBe('Gb');
    }
  });

  it('defaults an unrecognised modeName to Gb', () => {
    expect(countryForMode('some-mode-nobody-has-invented-yet')).toBe('Gb');
  });

  it('maps a modeName present in an injected table to its country', () => {
    // No real modeName maps to a non-Gb country today -- this exercises
    // the derivation logic itself against a synthetic table so it isn't
    // left untested until Ireland Tier C ships (see
    // docs/superpowers/plans/2026-09-05-country-filtering-plan.md
    // Judgment Call 2). The key is illustrative only, matching
    // docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196's
    // own "island-of-ireland-*" framing -- not a claim about a real value.
    const syntheticTable: Record<string, Country> = { 'island-of-ireland-nir': 'NorthernIreland' };
    expect(countryForMode('island-of-ireland-nir', syntheticTable)).toBe('NorthernIreland');
    expect(countryForMode('national-rail', syntheticTable)).toBe('Gb');
  });

  it("countryForReport derives from a report's modeName the same way", () => {
    const syntheticTable: Record<string, Country> = { 'island-of-ireland-roi': 'RepublicOfIreland' };
    expect(countryForReport({ modeName: 'island-of-ireland-roi' }, syntheticTable)).toBe('RepublicOfIreland');
    expect(countryForReport({ modeName: 'national-rail' }, syntheticTable)).toBe('Gb');
  });
});
