import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HourlyTrendsResults, toHourlyChartPoints } from './HourlyTrendsResults';
import * as api from '@/lib/api';
import type { LineHourlyStats } from '@/lib/types';

// Mirrors the component's own (intentionally unexported) floor -- see
// HourlyTrendsResults.tsx's SPARSE_DATA_FLOOR_CYCLES_HOURLY comment for why
// the exact value (20) is a re-derived, still-unvalidated placeholder.
const SPARSE_DATA_FLOOR_CYCLES_HOURLY = 20;

vi.mock('@/lib/api');

// Same rationale/conventions as TrendsResults.test.tsx's own mock of
// `@mantine/charts`' LineChart -- see that file's comment for the full
// reasoning. `xAxisProps` is captured too so the `granularity="hour"`
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

function hourlyRow(overrides: Partial<LineHourlyStats> = {}): LineHourlyStats {
  return {
    hourStart: '2026-08-31T14:00:00Z',
    sampleCycles: 50,
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

describe('toHourlyChartPoints', () => {
  it('preserves an hour at or above the sparse-data floor', () => {
    const stats = [hourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HOURLY })];
    const [point] = toHourlyChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-31T14:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HOURLY,
    });
  });

  it('turns an hour below the sparse-data floor into a gap, preserving sampleCycles', () => {
    const stats = [hourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HOURLY - 1 })];
    const [point] = toHourlyChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_CYCLES_HOURLY - 1);
    expect(point.bucketKey).toBe('2026-08-31T14:00:00Z');
  });
});

describe('HourlyTrendsResults', () => {
  it('renders the empty state when there are no rows, inside a bounded container', async () => {
    vi.mocked(api.getLineHourlyStats).mockResolvedValue([]);
    renderWithMantine(
      await HourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    const text = screen.getByText('Not enough sampled data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('a sparse hour does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineHourlyStats).mockResolvedValue([
      hourlyRow({ hourStart: '2026-08-31T13:00:00Z', sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HOURLY - 1 }),
      hourlyRow({ hourStart: '2026-08-31T14:00:00Z', sampleCycles: 50 }),
    ]);
    renderWithMantine(
      await HourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    expect(rateChart).toBeDefined();
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseHour = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-31T13:00:00Z');
    expect(sparseHour.delayRate).toBeNull();
    expect(sparseHour.cancellationRate).toBeNull();
    expect(sparseHour.skipRate).toBeNull();
    expect(sparseHour.delayRate).not.toBe(0);
  });

  it('a normal multi-hour range renders without throwing and shows the "that hour" honesty copy verbatim', async () => {
    vi.mocked(api.getLineHourlyStats).mockResolvedValue([
      hourlyRow({ hourStart: '2026-08-31T12:00:00Z' }),
      hourlyRow({ hourStart: '2026-08-31T13:00:00Z' }),
      hourlyRow({ hourStart: '2026-08-31T14:00:00Z' }),
    ]);
    renderWithMantine(
      await HourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    expect(
      screen.getByText(
        /Rates shown count each distinct train once per hour, based on its status the first time it was seen that hour -- not a share of poll cycles\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });

  it('passes granularity="hour" through to TrendsCharts, giving the x-axis a formatTime tickFormatter', async () => {
    vi.mocked(api.getLineHourlyStats).mockResolvedValue([hourlyRow({ hourStart: '2026-08-31T14:00:00Z' })]);
    renderWithMantine(
      await HourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );

    for (const [props] of lineChartMock.mock.calls) {
      expect(typeof props.xAxisProps?.tickFormatter).toBe('function');
      // A short "HH:mm"-shaped label, not the raw RFC3339 bucketKey.
      const formatted = props.xAxisProps!.tickFormatter!('2026-08-31T14:00:00Z');
      expect(formatted).not.toBe('2026-08-31T14:00:00Z');
      expect(formatted).toMatch(/^\d{2}:\d{2}$/);
    }
  });
});
