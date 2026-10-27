import { describe, it, expect, beforeAll, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { gapSpans, TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

interface MockSeries {
  name: string;
  color?: string;
}
interface MockChartProps {
  data: unknown[];
  series: MockSeries[];
  curveType?: string;
  valueFormatter?: (value: number) => string;
  xAxisProps?: { tickFormatter?: (value: string) => string };
  tooltipProps?: { labelFormatter?: (label: unknown) => string };
}

vi.mock('@mantine/charts', () => ({
  LineChart: (props: MockChartProps) => (
    <div
      data-testid="line-chart"
      data-has-tick-formatter={String(typeof props.xAxisProps?.tickFormatter === 'function')}
      data-formatted-first-point={
        props.xAxisProps?.tickFormatter ? props.xAxisProps.tickFormatter('2026-08-01T12:00:00Z') : ''
      }
      data-tooltip-label={props.tooltipProps?.labelFormatter?.('2026-08-01T12:00:00Z') ?? ''}
      data-curve={props.curveType ?? ''}
      data-colors={props.series.map((s) => s.color).join(',')}
      data-formatted-value={props.valueFormatter?.(0.25) ?? ''}
    />
  ),
  BarChart: (props: MockChartProps) => (
    <div
      data-testid="bar-chart"
      data-series={props.series.map((s) => s.name).join(',')}
      data-points={JSON.stringify(props.data)}
      data-colors={props.series.map((s) => s.color).join(',')}
      data-formatted-value={props.valueFormatter?.(80000) ?? ''}
    />
  ),
}));

function point(bucketKey: string, delayRate: number | null) {
  return { bucketKey, delayRate };
}

// TrendsCharts loads its Recharts half on demand (next/dynamic); load it once
// up front so the first chart assertion isn't timed against a cold import.
beforeAll(async () => {
  await import('@/app/lines/[id]/history/TrendsChartParts');
});

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
      gapSpans([
        point('2026-08-01', 0.1),
        point('2026-08-02', null),
        point('2026-08-03', null),
        point('2026-08-04', 0.2),
      ]),
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
    const points = [halfHourPoint('2026-08-30T14:00:00Z', 0.1), halfHourPoint('2026-08-31T14:00:00Z', null)];
    const spans = gapSpans(points);
    expect(spans).toEqual([{ startKey: '2026-08-31T14:00:00Z', endKey: '2026-08-31T14:00:00Z' }]);
    expect(spans[0]!.startKey).not.toBe(spans[0]!.endKey === points[0]!.bucketKey ? points[0]!.bucketKey : undefined);
  });
});

describe('TrendsCharts granularity prop', () => {
  const points: ChartPoint[] = [
    {
      bucketKey: '2026-08-01T12:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0,
      skipRate: 0,
      avgDelayMinutes: 1,
      total: 42,
      sampleCycles: 50,
    },
  ];

  it.each(['halfHour', 'hour', 'sixHour'] as const)(
    'gives the x-axis a time-of-day tickFormatter for the %s granularity',
    async (granularity) => {
      renderWithMantine(<TrendsCharts points={points} granularity={granularity} order={2} />);
      const chart = (await screen.findAllByTestId('line-chart'))[0];
      expect(chart).toHaveAttribute('data-has-tick-formatter', 'true');
      expect(chart).toHaveAttribute('data-formatted-first-point', '13:00');
    },
  );

  // Review §3.3/I10: the day axis used to print the raw bucket key
  // ("2026-08-01") verbatim -- every other date in the app reads "15 Sep"
  // style. Now formatted, not omitted.
  it('gives the x-axis a human short-date tickFormatter for the day granularity', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    const chart = (await screen.findAllByTestId('line-chart'))[0];
    expect(chart).toHaveAttribute('data-has-tick-formatter', 'true');
    expect(chart).toHaveAttribute('data-formatted-first-point', '1 Aug');
  });
});

describe('TrendsCharts showVolume prop', () => {
  const points: ChartPoint[] = [
    {
      bucketKey: '2026-08-01T12:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0,
      skipRate: 0,
      avgDelayMinutes: 1,
      total: 42,
      sampleCycles: 50,
    },
  ];

  it('does not render the bar chart by default', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    expect(screen.queryByTestId('bar-chart')).not.toBeInTheDocument();
  });

  it('renders the bar chart, reading total, when showVolume is true', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = await screen.findByTestId('bar-chart');
    expect(barChart).toHaveAttribute('data-series', 'total');
    const barPoints = JSON.parse(barChart.dataset.points as string);
    expect(barPoints[0].total).toBe(42);
  });

  it('renders the bar chart above (before) the rate line chart in document order', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = await screen.findByTestId('bar-chart');
    const lineCharts = await screen.findAllByTestId('line-chart');
    expect(barChart.compareDocumentPosition(lineCharts[0]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('still renders the two rate/delay line charts unchanged when showVolume is true', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    expect(await screen.findAllByTestId('line-chart')).toHaveLength(2);
  });
});

describe('TrendsCharts theming', () => {
  const points: ChartPoint[] = [
    {
      bucketKey: '2026-08-01T12:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0,
      skipRate: 0,
      avgDelayMinutes: 1,
      total: 80000,
      sampleCycles: 50,
    },
  ];

  it('draws straight lines in the theme colours, with the site units', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="halfHour" order={2} />);
    const [rates, delay] = await screen.findAllByTestId('line-chart');
    expect(rates).toHaveAttribute('data-curve', 'linear');
    expect(delay).toHaveAttribute('data-curve', 'linear');
    expect(rates).toHaveAttribute(
      'data-colors',
      'var(--ds-chart-delay),var(--ds-chart-cancellation),var(--ds-chart-skip)',
    );
    expect(rates).toHaveAttribute('data-formatted-value', '25%');
    expect(delay).toHaveAttribute('data-formatted-value', '0.3 min');
    expect(rates).toHaveAttribute('data-tooltip-label', '1 Aug, 13:00');
  });

  it('puts thousands separators on the trains counted chart', async () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const bars = await screen.findByTestId('bar-chart');
    expect(bars).toHaveAttribute('data-formatted-value', '80,000');
    expect(bars).toHaveAttribute('data-colors', 'var(--ds-chart-count)');
  });
});
