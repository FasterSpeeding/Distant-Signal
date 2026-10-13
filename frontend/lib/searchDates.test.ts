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

  it('uses the singular for one day', () => {
    expect(searchDateDescription({ minDate: '2026-10-07', maxDate: '2026-10-09' }, TODAY)).toBe(
      'Search a different day, up to 1 day back and 1 day ahead.',
    );
  });

  it('leaves out a direction the range does not reach, never "0 days"', () => {
    expect(searchDateDescription({ minDate: TODAY, maxDate: '2026-10-15' }, TODAY)).toBe(
      'Search a later day, up to 7 days ahead.',
    );
    expect(searchDateDescription({ minDate: '2026-10-01', maxDate: TODAY }, TODAY)).toBe(
      'Search an earlier day, up to 7 days back.',
    );
    expect(searchDateDescription({ minDate: TODAY, maxDate: TODAY }, TODAY)).toBe('Only today can be searched.');
  });

  it('names the dates when today is outside the range (the browser and server clocks disagree)', () => {
    expect(searchDateDescription({ minDate: '2026-10-09', maxDate: '2026-10-09' }, TODAY)).toBe(
      'Only 9 Oct 2026 can be searched.',
    );
    expect(searchDateDescription({ minDate: '2026-10-06', maxDate: '2026-10-20' }, '2026-09-17')).toBe(
      'Search a day from 6 Oct 2026 to 20 Oct 2026.',
    );
    expect(searchDateDescription({ minDate: '2026-09-01', maxDate: '2026-09-07' }, TODAY)).toBe(
      'Search a day from 1 Sep 2026 to 7 Sep 2026.',
    );
  });
});
