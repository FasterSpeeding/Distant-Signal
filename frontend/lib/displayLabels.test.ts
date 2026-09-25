import { describe, it, expect } from 'vitest';
import { categoryLabel, tocNameLookup, operatorLabel } from './displayLabels';

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
