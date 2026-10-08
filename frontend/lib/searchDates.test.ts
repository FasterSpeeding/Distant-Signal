import { describe, expect, it } from 'vitest';
import { FALLBACK_SEARCH_WINDOW_DAYS, searchDateBounds, searchDateDescription } from './searchDates';

const TODAY = '2026-10-08';

describe('searchDateBounds', () => {
  it("uses the API's accepted range when there is one", () => {
    expect(searchDateBounds({ from: '2026-09-30', to: '2026-11-05' }, TODAY)).toEqual({
      minDate: '2026-09-30',
      maxDate: '2026-11-05',
    });
  });

  it('falls back to a week either side of today without a range', () => {
    const fallback = { minDate: '2026-10-01', maxDate: '2026-10-15' };
    expect(FALLBACK_SEARCH_WINDOW_DAYS).toBe(7);
    expect(searchDateBounds(null, TODAY)).toEqual(fallback);
    expect(searchDateBounds(undefined, TODAY)).toEqual(fallback);
  });

  it('falls back on a malformed or inverted range', () => {
    const fallback = { minDate: '2026-10-01', maxDate: '2026-10-15' };
    expect(searchDateBounds({ from: '1 Oct', to: '2026-11-05' }, TODAY)).toEqual(fallback);
    expect(searchDateBounds({ from: '2026-11-05', to: '2026-10-01' }, TODAY)).toEqual(fallback);
  });

  it('crosses a month and a year boundary in the fallback', () => {
    expect(searchDateBounds(null, '2026-12-28')).toEqual({ minDate: '2026-12-21', maxDate: '2027-01-04' });
  });
});

describe('searchDateDescription', () => {
  it('keeps the week wording for the fallback window', () => {
    expect(searchDateDescription(searchDateBounds(null, TODAY), TODAY)).toBe(
      'Search a different day, up to a week either side of today.',
    );
  });

  it('names how far back and ahead a wider range reaches', () => {
    expect(searchDateDescription({ minDate: '2026-10-01', maxDate: '2026-11-05' }, TODAY)).toBe(
      'Search a different day, up to 7 days back and 28 days ahead.',
    );
  });

  it('uses the singular for one day and never goes negative', () => {
    expect(searchDateDescription({ minDate: '2026-10-09', maxDate: '2026-10-09' }, TODAY)).toBe(
      'Search a different day, up to 0 days back and 1 day ahead.',
    );
  });
});
