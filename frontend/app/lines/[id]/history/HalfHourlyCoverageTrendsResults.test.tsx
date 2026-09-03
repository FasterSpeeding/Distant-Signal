import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HalfHourlyCoverageTrendsResults, toHalfHourlyCoverageChartPoints } from './HalfHourlyCoverageTrendsResults';
import * as api from '@/lib/api';
import type { LineHalfHourlyCoverageStats } from '@/lib/types';

// Mirrors the component's own (intentionally unexported) floor -- see
// HalfHourlyCoverageTrendsResults.tsx's SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY
// comment for why the exact value (10) is a doubly-unvalidated placeholder.
const SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY = 10;

vi.mock('@/lib/api');

// Same rationale/conventions as HalfHourlyTrendsResults.test.tsx's own mock
// of `@mantine/charts`' LineChart -- see that file's comment for the full
// reasoning. `xAxisProps` is captured too so the `granularity="halfHour"`
// tickFormatter can be asserted on directly.
type MockLineChartProps = {
  data: unknown[];
  series: { name: string; strokeDasharray?: string | number }[];
  connectNulls?: boolean;
  withLegend?: boolean;
  valueFormatter?: (value: number) => string;
  xAxisProps?: { padding?: unknown; tickFormatter?: (value: string) => string };
};

const lineChartMock = vi.fn((props: MockLineChartProps) => (
  <div
    data-testid="line-chart"
    data-series={props.series.map((series) => series.name).join(',')}
    data-connect-nulls={String(props.connectNulls)}
    data-points={JSON.stringify(props.data)}
    data-with-legend={String(props.withLegend)}
    data-dash-patterns={props.series.map((series) => series.strokeDasharray ?? '').join(',')}
  />
));

vi.mock('@mantine/charts', () => ({ LineChart: (props: MockLineChartProps) => lineChartMock(props) }));

function halfHourlyCoverageRow(overrides: Partial<LineHalfHourlyCoverageStats> = {}): LineHalfHourlyCoverageStats {
  return {
    halfHourStart: '2026-08-31T14:00:00Z',
    resolvedWindows: 25,
    total: 100,
    delayed: 10,
    cancelled: 2,
    skipped: 1,
    avgDelayMinutes: 3.5,
    delayRate: 0.1,
    cancellationRate: 0.02,
    skipRate: 0.01,
    ...overrides,
  };
}

describe('toHalfHourlyCoverageChartPoints', () => {
  it('preserves a half hour at or above the sparse-data floor', () => {
    const stats = [halfHourlyCoverageRow({ resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY })];
    const [point] = toHalfHourlyCoverageChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-31T14:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      sampleCycles: SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY,
    });
  });

  it('turns a half hour below the sparse-data floor into a gap, preserving sampleCycles', () => {
    const stats = [halfHourlyCoverageRow({ resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY - 1 })];
    const [point] = toHalfHourlyCoverageChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY - 1);
    expect(point.bucketKey).toBe('2026-08-31T14:00:00Z');
  });
});

describe('HalfHourlyCoverageTrendsResults', () => {
  it('renders the empty state when there are no rows, inside a bounded container', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    const text = screen.getByText('Not enough full-coverage data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('renders the unreachable-backend fallback when the fetch rejects', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    expect(await screen.findByText("Coverage trend data isn't available right now.")).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
  });

  it('a sparse half hour does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([
      halfHourlyCoverageRow({
        halfHourStart: '2026-08-31T13:30:00Z',
        resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY - 1,
      }),
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T14:00:00Z', resolvedWindows: 25 }),
    ]);
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    expect(rateChart).toBeDefined();
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseHalfHour = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-31T13:30:00Z');
    expect(sparseHalfHour.delayRate).toBeNull();
    expect(sparseHalfHour.cancellationRate).toBeNull();
    expect(sparseHalfHour.skipRate).toBeNull();
    expect(sparseHalfHour.delayRate).not.toBe(0);
  });

  it('a normal multi-bucket range renders without throwing and shows the full-coverage honesty copy verbatim', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T12:00:00Z' }),
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T12:30:00Z' }),
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T13:00:00Z' }),
    ]);
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    // Verbatim match of CoverageTrendsResults.tsx's own honesty sentence --
    // not reworded for granularity (design doc Decision 3): it states a
    // population fact, not a per-bucket attribution rule.
    expect(
      screen.getByText(
        'Rates shown cover every scheduled service on this line, cross-referenced against real train-movement data — not a sample of live departures at a handful of stations.',
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });

  it('passes granularity="halfHour" through to TrendsCharts, giving the x-axis a formatTime tickFormatter', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T14:00:00Z' }),
    ]);
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    for (const [props] of lineChartMock.mock.calls) {
      expect(typeof props.xAxisProps?.tickFormatter).toBe('function');
      // A short "HH:mm"-shaped label, not the raw RFC3339 bucketKey.
      const formatted = props.xAxisProps!.tickFormatter!('2026-08-31T14:00:00Z');
      expect(formatted).not.toBe('2026-08-31T14:00:00Z');
      expect(formatted).toMatch(/^\d{2}:\d{2}$/);
    }
  });

  it('renders "Full coverage" at h3 and both chart titles at h4, one level below', async () => {
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([
      halfHourlyCoverageRow({ halfHourStart: '2026-08-31T14:00:00Z' }),
    ]);
    renderWithMantine(
      await HalfHourlyCoverageTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    expect(screen.getByRole('heading', { name: 'Full coverage', level: 3 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Delay / cancellation / skip rate', level: 4 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Average delay (minutes)', level: 4 })).toBeInTheDocument();
  });
});
