import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HalfHourlyTrendsResults, toHalfHourlyChartPoints } from './HalfHourlyTrendsResults';
import * as api from '@/lib/api';
import type { LineHalfHourlyStats } from '@/lib/types';

// Mirrors the component's own (intentionally unexported) floor -- see
// HalfHourlyTrendsResults.tsx's SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY comment
// for why the exact value (10) is a re-derived, still-unvalidated placeholder.
const SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY = 10;

vi.mock('@/lib/api');

// Same rationale/conventions as TrendsResults.test.tsx's own mock of
// `@mantine/charts`' LineChart -- see that file's comment for the full
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

function halfHourlyRow(overrides: Partial<LineHalfHourlyStats> = {}): LineHalfHourlyStats {
  return {
    halfHourStart: '2026-08-31T14:00:00Z',
    sampleCycles: 25,
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

describe('toHalfHourlyChartPoints', () => {
  it('preserves a half hour at or above the sparse-data floor', () => {
    const stats = [halfHourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY })];
    const [point] = toHalfHourlyChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-31T14:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      total: 100,
      sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY,
    });
  });

  it('turns a half hour below the sparse-data floor into a gap, preserving sampleCycles', () => {
    const stats = [halfHourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1 })];
    const [point] = toHalfHourlyChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.total).toBe(100); // never nulled, even when sparse
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1);
    expect(point.bucketKey).toBe('2026-08-31T14:00:00Z');
  });
});

describe('HalfHourlyTrendsResults', () => {
  it('renders the empty state when there are no rows, inside a bounded container', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    const text = screen.getByText('Not enough sampled data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('a sparse half hour does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([
      halfHourlyRow({ halfHourStart: '2026-08-31T13:30:00Z', sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1 }),
      halfHourlyRow({ halfHourStart: '2026-08-31T14:00:00Z', sampleCycles: 25 }),
    ]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
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

  it('a normal multi-bucket range renders without throwing and shows the "that half hour" honesty copy verbatim', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([
      halfHourlyRow({ halfHourStart: '2026-08-31T12:00:00Z' }),
      halfHourlyRow({ halfHourStart: '2026-08-31T12:30:00Z' }),
      halfHourlyRow({ halfHourStart: '2026-08-31T13:00:00Z' }),
    ]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    expect(
      screen.getByText(
        /Rates shown count each distinct train once per half hour, based on its status the first time it was seen that half hour -- not a share of poll cycles\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });

  it('passes granularity="halfHour" through to TrendsCharts, giving the x-axis a formatTime tickFormatter', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([halfHourlyRow({ halfHourStart: '2026-08-31T14:00:00Z' })]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    for (const [props] of lineChartMock.mock.calls) {
      expect(typeof props.xAxisProps?.tickFormatter).toBe('function');
      // A short "HH:mm"-shaped label, not the raw RFC3339 bucketKey.
      const formatted = props.xAxisProps!.tickFormatter!('2026-08-31T14:00:00Z');
      expect(formatted).not.toBe('2026-08-31T14:00:00Z');
      expect(formatted).toMatch(/^\d{2}:\d{2}$/);
    }
  });

  it('renders both chart titles at h3, one level below /lines/[id]\'s h2 "Recent trends (last 24 hours)"', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([halfHourlyRow({ halfHourStart: '2026-08-31T14:00:00Z' })]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    expect(screen.getByRole('heading', { name: 'Delay / cancellation / skip rate', level: 3 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Average delay (minutes)', level: 3 })).toBeInTheDocument();
  });
});
