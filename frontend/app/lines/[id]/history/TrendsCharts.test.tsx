import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { gapSpans, TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

vi.mock('@mantine/charts', () => ({
  LineChart: (props: { xAxisProps?: { tickFormatter?: (value: string) => string } }) => (
    <div
      data-testid="line-chart"
      data-has-tick-formatter={String(typeof props.xAxisProps?.tickFormatter === 'function')}
    />
  ),
  BarChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="bar-chart" data-series={props.series.map((s) => s.name).join(',')} data-points={JSON.stringify(props.data)} />
  ),
}));

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

describe('gapSpans (half-hourly buckets)', () => {
  function halfHourPoint(halfHourStart: string, delayRate: number | null) {
    return { bucketKey: halfHourStart, delayRate };
  }

  it('returns a single-bucket span for one isolated sparse half hour', () => {
    expect(
      gapSpans([
        halfHourPoint('2026-08-31T12:00:00Z', 0.1),
        halfHourPoint('2026-08-31T12:30:00Z', null),
        halfHourPoint('2026-08-31T13:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-08-31T12:30:00Z', endKey: '2026-08-31T12:30:00Z' }]);
  });

  it('merges a multi-bucket gap into one span, including one that crosses a day boundary', () => {
    expect(
      gapSpans([
        halfHourPoint('2026-08-31T23:30:00Z', 0.1),
        halfHourPoint('2026-09-01T00:00:00Z', null),
        halfHourPoint('2026-09-01T00:30:00Z', null),
        halfHourPoint('2026-09-01T01:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-09-01T00:00:00Z', endKey: '2026-09-01T00:30:00Z' }]);
  });

  it('does not collide two buckets that share the same wall-clock time-of-day label on different days', () => {
    // Regression guard for the finding in this plan's Status note: the raw
    // RFC3339 instant, not a formatted "HH:mm" label, must be what
    // gapSpans/referenceAreaBounds treat as the bucket identity.
    const points = [
      halfHourPoint('2026-08-30T14:00:00Z', 0.1),
      halfHourPoint('2026-08-31T14:00:00Z', null),
    ];
    const spans = gapSpans(points);
    expect(spans).toEqual([{ startKey: '2026-08-31T14:00:00Z', endKey: '2026-08-31T14:00:00Z' }]);
    expect(spans[0].startKey).not.toBe(spans[0].endKey === points[0].bucketKey ? points[0].bucketKey : undefined);
  });
});

describe('TrendsCharts granularity prop', () => {
  const points: ChartPoint[] = [
    { bucketKey: '2026-08-01T12:00:00Z', delayRate: 0.1, cancellationRate: 0, skipRate: 0, avgDelayMinutes: 1, total: 42, sampleCycles: 50 },
  ];

  it.each(['halfHour', 'hour', 'sixHour'] as const)(
    'gives the x-axis a tickFormatter for the %s granularity',
    (granularity) => {
      renderWithMantine(<TrendsCharts points={points} granularity={granularity} order={2} />);
      expect(screen.getAllByTestId('line-chart')[0]).toHaveAttribute('data-has-tick-formatter', 'true');
    },
  );

  it('gives the x-axis no tickFormatter for the day granularity', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    expect(screen.getAllByTestId('line-chart')[0]).toHaveAttribute('data-has-tick-formatter', 'false');
  });
});

describe('TrendsCharts showVolume prop', () => {
  const points: ChartPoint[] = [
    { bucketKey: '2026-08-01T12:00:00Z', delayRate: 0.1, cancellationRate: 0, skipRate: 0, avgDelayMinutes: 1, total: 42, sampleCycles: 50 },
  ];

  it('does not render the bar chart by default', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    expect(screen.queryByTestId('bar-chart')).not.toBeInTheDocument();
  });

  it('renders the bar chart, reading total, when showVolume is true', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = screen.getByTestId('bar-chart');
    expect(barChart).toHaveAttribute('data-series', 'total');
    const barPoints = JSON.parse(barChart.dataset.points as string);
    expect(barPoints[0].total).toBe(42);
  });

  it('renders the bar chart above (before) the rate line chart in document order', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = screen.getByTestId('bar-chart');
    const lineCharts = screen.getAllByTestId('line-chart');
    expect(barChart.compareDocumentPosition(lineCharts[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('still renders the two rate/delay line charts unchanged when showVolume is true', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });
});
