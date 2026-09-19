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
