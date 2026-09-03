import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CoverageTrendsResults, toCoverageChartPoints } from './CoverageTrendsResults';
import * as api from '@/lib/api';
import type { LineDailyCoverageStats } from '@/lib/types';

// Mirrors the component's own (intentionally unexported) floor -- see
// CoverageTrendsResults.tsx's SPARSE_DATA_FLOOR_WINDOWS comment for why
// this is its own, separately-calibrated placeholder, not shared with
// TrendsResults.tsx's SPARSE_DATA_FLOOR_CYCLES.
const SPARSE_DATA_FLOOR_WINDOWS = 20;

vi.mock('@/lib/api');

// Same rationale/shape as TrendsResults.test.tsx's own LineChart mock --
// see that file's comment for the full reasoning.
type MockLineChartProps = {
  data: unknown[];
  series: { name: string; strokeDasharray?: string | number }[];
  connectNulls?: boolean;
  withLegend?: boolean;
  valueFormatter?: (value: number) => string;
  xAxisProps?: unknown;
};

const lineChartMock = vi.fn((props: MockLineChartProps) => (
  <div
    data-testid="line-chart"
    data-series={props.series.map((series) => series.name).join(',')}
    data-connect-nulls={String(props.connectNulls)}
    data-points={JSON.stringify(props.data)}
  />
));

vi.mock('@mantine/charts', () => ({ LineChart: (props: MockLineChartProps) => lineChartMock(props) }));

function row(overrides: Partial<LineDailyCoverageStats> = {}): LineDailyCoverageStats {
  return {
    day: '2026-08-01',
    resolvedWindows: 500,
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

describe('toCoverageChartPoints', () => {
  it('preserves a day at or above the sparse-data floor', () => {
    const stats = [row({ resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS })];
    const [point] = toCoverageChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-01',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      sampleCycles: SPARSE_DATA_FLOOR_WINDOWS,
    });
  });

  it('turns a day below the sparse-data floor into a gap, preserving the resolvedWindows count', () => {
    const stats = [row({ resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS - 1 })];
    const [point] = toCoverageChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_WINDOWS - 1);
    expect(point.bucketKey).toBe('2026-08-01');
  });

  it('returns an empty array for an empty input, not a synthetic point', () => {
    expect(toCoverageChartPoints([])).toEqual([]);
  });
});

describe('CoverageTrendsResults', () => {
  it('renders the empty state (distinct wording from the sample-series one) when there are no rows', async () => {
    vi.mocked(api.getLineDailyCoverageStats).mockResolvedValue([]);
    renderWithMantine(
      await CoverageTrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );
    const text = screen.getByText('Not enough full-coverage data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('a normal multi-day range renders without throwing and shows the full-coverage honesty copy verbatim', async () => {
    vi.mocked(api.getLineDailyCoverageStats).mockResolvedValue([
      row({ day: '2026-08-01' }),
      row({ day: '2026-08-02' }),
      row({ day: '2026-08-03' }),
    ]);
    renderWithMantine(
      await CoverageTrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    expect(
      screen.getByText(
        /Rates shown cover every scheduled service on this line, cross-referenced against real train-movement data — not a sample of live departures at a handful of stations\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });

  it('a sparse day does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineDailyCoverageStats).mockResolvedValue([
      row({ day: '2026-08-01', resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS - 1 }),
      row({ day: '2026-08-02', resolvedWindows: 500 }),
    ]);
    renderWithMantine(
      await CoverageTrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    expect(rateChart).toBeDefined();
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseDay = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-01');
    expect(sparseDay.delayRate).toBeNull();
    expect(sparseDay.delayRate).not.toBe(0);
  });

  it('renders the section title and reuses TrendsCharts unmodified (two chart instances, gaps not interpolated)', async () => {
    vi.mocked(api.getLineDailyCoverageStats).mockResolvedValue([row({ day: '2026-08-01' })]);
    renderWithMantine(
      await CoverageTrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    expect(screen.getByRole('heading', { name: 'Full coverage' })).toBeInTheDocument();
    const charts = screen.getAllByTestId('line-chart');
    expect(charts).toHaveLength(2);
    for (const chart of charts) {
      expect(chart.dataset.connectNulls).toBe('false');
    }
  });
});
