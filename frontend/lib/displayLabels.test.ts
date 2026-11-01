import { describe, it, expect } from 'vitest';
import {
  categoryLabel,
  changesBadge,
  isCrossStationTransfer,
  tocNameLookup,
  operatorLabel,
  transferLabel,
  transferLegLabel,
  transferLegMode,
} from './displayLabels';
import type { TripPlanItinerary, TripPlanLeg } from './types';

describe('categoryLabel', () => {
  it('maps a known TOML-enum category to a human label', () => {
    expect(categoryLabel('main-line')).toBe('Main line');
  });

  it('maps a TfL mode-name category to a human label', () => {
    expect(categoryLabel('elizabeth-line')).toBe('Elizabeth line');
  });

  it('maps the custom-line category to a human label', () => {
    expect(categoryLabel('custom')).toBe('Custom line');
  });

  it('falls back to the raw token for an unlisted category', () => {
    expect(categoryLabel('some-future-category')).toBe('some-future-category');
  });

  // Signal Box Audit, flib Low finding: "prototype-key lookups can render a
  // function as a label". `category` is an open-ended string (a TOML
  // category, a TfL mode_name, or "custom") -- a category literally named
  // "constructor" (or another Object.prototype key) used to make
  // `CATEGORY_LABELS[category]` resolve to `Object.prototype.constructor`
  // (a function), which `?? category` would not catch.
  it('falls back to the raw token for a category matching an Object.prototype key, not a function', () => {
    expect(categoryLabel('constructor')).toBe('constructor');
    expect(typeof categoryLabel('constructor')).toBe('string');
    expect(categoryLabel('toString')).toBe('toString');
    expect(categoryLabel('hasOwnProperty')).toBe('hasOwnProperty');
  });
});

describe('operatorLabel', () => {
  it('renders "Name (CODE)" when the code resolves', () => {
    const lookup = tocNameLookup([{ code: 'GR', name: 'London North Eastern Railway' }]);
    expect(operatorLabel('GR', lookup)).toBe('London North Eastern Railway (GR)');
  });

  it('falls back to the bare code when the lookup has no match', () => {
    const lookup = tocNameLookup([{ code: 'GR', name: 'London North Eastern Railway' }]);
    expect(operatorLabel('SW', lookup)).toBe('SW');
  });

  it('falls back to the bare code for every operator when the TOC list is empty', () => {
    const lookup = tocNameLookup([]);
    expect(operatorLabel('SW', lookup)).toBe('SW');
  });
});

describe('transferLabel', () => {
  it('adds the line where known, saying "line" once for the Tube', () => {
    expect(transferLabel('TUBE', 'Victoria')).toBe('Tube (Victoria line)');
    expect(transferLabel('tube', 'Elizabeth line')).toBe('Tube (Elizabeth line)');
    expect(transferLabel('METRO', 'DLR')).toBe('Metro (DLR)');
  });

  it('is the mode alone without a line', () => {
    expect(transferLabel('TUBE')).toBe('Tube');
    expect(transferLabel('WALK', '  ')).toBe('Walk');
    expect(transferLabel('TUBE', null)).toBe('Tube');
  });
});

describe('transfer legs (#95)', () => {
  const stop = (id: string) => ({ id, name: id, modes: ['tube'], lines: [] });
  const leg = {
    kind: 'transfer' as const,
    mode: 'TRANSFER',
    originCrs: 'EUS',
    destinationCrs: 'VIC',
    minutes: 19,
  };

  it('tells a TRANSFER between stations from a change within one', () => {
    expect(isCrossStationTransfer(leg)).toBe(true);
    expect(isCrossStationTransfer({ ...leg, destinationCrs: 'eus' })).toBe(false);
    expect(isCrossStationTransfer({ ...leg, originCrs: null, destinationCrs: null })).toBe(false);
    expect(isCrossStationTransfer({ ...leg, mode: 'TUBE' })).toBe(false);
  });

  it('reads a known Tube hop as the Tube: run by LT, or a Tube stop at both ends', () => {
    expect(transferLegMode(leg)).toBe('TRANSFER');
    expect(transferLegMode({ ...leg, operator: 'LT' })).toBe('TUBE');
    expect(transferLegMode({ ...leg, originStop: stop('A'), destinationStop: stop('B') })).toBe('TUBE');
    expect(transferLegMode({ ...leg, originStop: stop('A') })).toBe('TRANSFER');
    expect(transferLegMode({ ...leg, operator: 'ds:DLR' })).toBe('TRANSFER');
    // A change within one station stays one, whoever runs the pair's link.
    expect(transferLegMode({ ...leg, destinationCrs: 'EUS', operator: 'LT' })).toBe('TRANSFER');
    expect(transferLegMode({ ...leg, mode: 'WALK', operator: 'LT' })).toBe('WALK');
  });

  it('labels each the way it saves', () => {
    expect(transferLegLabel(leg)).toBe('Transfer');
    expect(transferLegLabel({ ...leg, operator: 'LT', line: 'Victoria' })).toBe('Tube (Victoria line)');
    expect(transferLegLabel({ ...leg, destinationCrs: 'EUS' })).toBe('Change');
    expect(transferLegLabel({ ...leg, mode: 'TUBE', line: 'Northern' })).toBe('Tube (Northern line)');
  });
});

describe('changesBadge', () => {
  const actonTown = { id: '940GZZLUACT', name: 'Acton Town', modes: ['tube'], lines: [] };
  const ride = (mode: string, changes: boolean): TripPlanLeg => ({
    kind: 'transfer',
    mode,
    originCrs: null,
    destinationCrs: null,
    minutes: 60,
    access: { end: 'both', basis: 'line', distanceMetres: 33_000, ...(changes && { changeAt: actonTown }) },
  });
  const itinerary = (legs: TripPlanLeg[], changeCount: number, accessChangeCount?: number): TripPlanItinerary => ({
    legs,
    changeCount,
    ...(accessChangeCount !== undefined && { accessChangeCount }),
    totalDurationMinutes: 60,
  });

  it("counts National Rail's changes alone as before", () => {
    expect(changesBadge(itinerary([], 0))).toEqual({ total: 0, label: '0 changes' });
    expect(changesBadge(itinerary([], 1, 0))).toEqual({ total: 1, label: '1 change' });
  });

  it("adds a TfL ride's change and says what it is", () => {
    expect(changesBadge(itinerary([ride('TUBE', true)], 0, 1))).toEqual({
      total: 1,
      label: '1 change',
      breakdown: '1 Tube change',
    });
    expect(changesBadge(itinerary([ride('TUBE', true), ride('METRO', true)], 2, 2))).toEqual({
      total: 4,
      label: '4 changes',
      breakdown: '2 train changes, 1 Tube change, 1 Metro change',
    });
  });

  it('falls back to TfL when the legs do not name every change', () => {
    expect(changesBadge(itinerary([ride('TUBE', false)], 0, 1)).breakdown).toBe('1 TfL change');
  });
});
