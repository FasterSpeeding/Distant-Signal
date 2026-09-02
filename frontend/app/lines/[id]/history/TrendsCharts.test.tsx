import { describe, it, expect } from 'vitest';
import { gapSpans } from './TrendsCharts';

function point(day: string, delayRate: number | null) {
  return { day, delayRate };
}

describe('gapSpans', () => {
  it('returns no spans when there are no gap days', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', 0.2)])).toEqual([]);
  });

  it('returns a single-day span for one isolated gap day', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null), point('2026-08-03', 0.2)])).toEqual([
      { startDay: '2026-08-02', endDay: '2026-08-02' },
    ]);
  });

  it('merges a multi-day gap into one span', () => {
    // NB: the plan's own literal test fixture for this case (`docs/superpowers/plans/2026-09-02-line-history-chart-fixes.md`,
    // Task 4 Step 4) asserts `endDay: '2026-08-04'`, but day04 here has a
    // non-null value (0.2) -- that's inconsistent with both gapSpans'
    // prescribed algorithm (Step 1) and every other case in this same
    // describe block (e.g. "multiple separate spans" below correctly ends
    // a gap at the last *null* day, not the following valid day). Treating
    // that as a typo in the plan, not a spec to match: propagating it here
    // would mean the shaded gap band spills into a valid data day, exactly
    // the spillover risk Task 4 Step 3 itself calls out as a live-render
    // risk to check for.
    expect(
      gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null), point('2026-08-03', null), point('2026-08-04', 0.2)]),
    ).toEqual([{ startDay: '2026-08-02', endDay: '2026-08-03' }]);
  });

  it('returns multiple separate spans for multiple separate gap runs', () => {
    expect(
      gapSpans([
        point('2026-08-01', null),
        point('2026-08-02', 0.1),
        point('2026-08-03', null),
        point('2026-08-04', null),
        point('2026-08-05', 0.2),
      ]),
    ).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-01' },
      { startDay: '2026-08-03', endDay: '2026-08-04' },
    ]);
  });

  it('returns one full-width span when every day is a gap', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', null)])).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-02' },
    ]);
  });

  it('handles a leading gap flush against the start of the range', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', 0.1)])).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-01' },
    ]);
  });

  it('handles a trailing gap flush against the end of the range', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null)])).toEqual([
      { startDay: '2026-08-02', endDay: '2026-08-02' },
    ]);
  });

  it('returns no spans for an empty points array', () => {
    expect(gapSpans([])).toEqual([]);
  });
});
