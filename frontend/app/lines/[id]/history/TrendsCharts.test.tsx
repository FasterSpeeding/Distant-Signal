import { describe, it, expect } from 'vitest';
import { gapSpans } from './TrendsCharts';

function point(bucketKey: string, delayRate: number | null) {
  return { bucketKey, delayRate };
}

describe('gapSpans', () => {
  it('returns no spans when there are no gap days', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', 0.2)])).toEqual([]);
  });

  it('returns a single-day span for one isolated gap day', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null), point('2026-08-03', 0.2)])).toEqual([
      { startKey: '2026-08-02', endKey: '2026-08-02' },
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
    ).toEqual([{ startKey: '2026-08-02', endKey: '2026-08-03' }]);
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
      { startKey: '2026-08-01', endKey: '2026-08-01' },
      { startKey: '2026-08-03', endKey: '2026-08-04' },
    ]);
  });

  it('returns one full-width span when every day is a gap', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', null)])).toEqual([
      { startKey: '2026-08-01', endKey: '2026-08-02' },
    ]);
  });

  it('handles a leading gap flush against the start of the range', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', 0.1)])).toEqual([
      { startKey: '2026-08-01', endKey: '2026-08-01' },
    ]);
  });

  it('handles a trailing gap flush against the end of the range', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null)])).toEqual([
      { startKey: '2026-08-02', endKey: '2026-08-02' },
    ]);
  });

  it('returns no spans for an empty points array', () => {
    expect(gapSpans([])).toEqual([]);
  });
});

describe('gapSpans (hourly buckets)', () => {
  function hourPoint(hourStart: string, delayRate: number | null) {
    return { bucketKey: hourStart, delayRate };
  }

  it('returns a single-bucket span for one isolated sparse hour', () => {
    expect(
      gapSpans([
        hourPoint('2026-08-31T12:00:00Z', 0.1),
        hourPoint('2026-08-31T13:00:00Z', null),
        hourPoint('2026-08-31T14:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-08-31T13:00:00Z', endKey: '2026-08-31T13:00:00Z' }]);
  });

  it('merges a multi-hour gap into one span, including one that crosses a day boundary', () => {
    expect(
      gapSpans([
        hourPoint('2026-08-31T23:00:00Z', 0.1),
        hourPoint('2026-09-01T00:00:00Z', null),
        hourPoint('2026-09-01T01:00:00Z', null),
        hourPoint('2026-09-01T02:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-09-01T00:00:00Z', endKey: '2026-09-01T01:00:00Z' }]);
  });

  it('does not collide two buckets that share the same wall-clock hour label on different days', () => {
    // Regression guard for the finding in this plan's Status note: the raw
    // RFC3339 instant, not a formatted "HH:mm" label, must be what
    // gapSpans/referenceAreaBounds treat as the bucket identity.
    const points = [
      hourPoint('2026-08-30T14:00:00Z', 0.1),
      hourPoint('2026-08-31T14:00:00Z', null),
    ];
    const spans = gapSpans(points);
    expect(spans).toEqual([{ startKey: '2026-08-31T14:00:00Z', endKey: '2026-08-31T14:00:00Z' }]);
    expect(spans[0].startKey).not.toBe(spans[0].endKey === points[0].bucketKey ? points[0].bucketKey : undefined);
  });
});
