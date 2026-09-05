import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrendsResults, toChartPoints } from './TrendsResults';
import * as api from '@/lib/api';
import type { LineDailyStats, LineHalfHourlyStats, LineHourlyStats, LineSixHourlyStats } from '@/lib/types';

vi.mock('@/lib/api');

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

function dailyRow(overrides: Partial<LineDailyStats> = {}): LineDailyStats {
  return {
    day: '2026-08-01',
    sampleCycles: 500,
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

function halfHourlyRow(overrides: Partial<LineHalfHourlyStats> = {}): LineHalfHourlyStats {
  return { ...dailyRow(), halfHourStart: '2026-08-31T14:00:00Z', ...overrides } as LineHalfHourlyStats;
}

function hourlyRow(overrides: Partial<LineHourlyStats> = {}): LineHourlyStats {
  return { ...dailyRow(), bucketStart: '2026-08-31T14:00:00Z', ...overrides } as LineHourlyStats;
}

function sixHourlyRow(overrides: Partial<LineSixHourlyStats> = {}): LineSixHourlyStats {
  return { ...dailyRow(), bucketStart: '2026-08-31T12:00:00Z', ...overrides } as LineSixHourlyStats;
}

describe('toChartPoints (generic)', () => {
  it('preserves a bucket at or above the given floor', () => {
    const stats = [dailyRow({ sampleCycles: 20 })];
    const [point] = toChartPoints(stats, (row) => row.day, 20);
    expect(point).toEqual({
      bucketKey: '2026-08-01',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      sampleCycles: 20,
    });
  });

  it('turns a bucket below the given floor into a gap, preserving sampleCycles', () => {
    const stats = [dailyRow({ sampleCycles: 19 })];
    const [point] = toChartPoints(stats, (row) => row.day, 20);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.sampleCycles).toBe(19);
  });
});

describe('TrendsResults', () => {
  it('defaults to the day granularity when none is passed, unchanged from before this feature', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyRow({ day: '2026-08-01' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(api.getLineDailyStats).toHaveBeenCalledWith('wcml', '2026-08-01', '2026-08-08');
    expect(screen.getByText(/Rates shown count each distinct train once per day/)).toBeInTheDocument();
  });

  it('renders the empty state when there are no rows, inside a bounded container', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    const text = screen.getByText('Not enough sampled data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('degrades gracefully, not to app/error.tsx, when the backend fetch throws', async () => {
    vi.mocked(api.getLineDailyStats).mockRejectedValue(new Error('boom'));
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByText("Trend data isn't available right now.")).toBeInTheDocument();
  });

  it('a sparse day does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([
      dailyRow({ day: '2026-08-01', sampleCycles: 19 }),
      dailyRow({ day: '2026-08-02', sampleCycles: 500 }),
    ]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseDay = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-01');
    expect(sparseDay.delayRate).toBeNull();
    expect(sparseDay.delayRate).not.toBe(0);
  });

  it('renders both chart titles at h2, one level below this page\'s only h1 ("History: {name}")', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyRow({ day: '2026-08-01' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByRole('heading', { name: 'Delay / cancellation / skip rate', level: 2 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Average delay (minutes)', level: 2 })).toBeInTheDocument();
  });

  it.each([
    ['halfHour', 'getLineHalfHourlyStats', halfHourlyRow, 10, 'per half hour'] as const,
    ['hour', 'getLineHourlyStats', hourlyRow, 20, 'per hour'] as const,
    ['sixHour', 'getLineSixHourlyStats', sixHourlyRow, 120, 'per six-hour period'] as const,
  ])('dispatches to the right fetch, floor, and honesty copy for the %s granularity', async (granularity, fnName, rowFactory, floor, copyFragment) => {
    const mockFn = vi.mocked(api[fnName as keyof typeof api]) as unknown as ReturnType<typeof vi.fn>;
    mockFn.mockResolvedValue([rowFactory({ sampleCycles: floor })]);
    renderWithMantine(
      await TrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z', granularity }),
    );
    expect(mockFn).toHaveBeenCalledWith('wcml', '2026-08-31T00:00:00Z', '2026-09-01T00:00:00Z');
    expect(screen.getByText(new RegExp(`Rates shown count each distinct train once ${copyFragment}`))).toBeInTheDocument();

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    expect(points[0].delayRate).not.toBeNull(); // exactly at the floor -- not sparse
  });

  it.each([
    ['halfHour', 'getLineHalfHourlyStats', halfHourlyRow, 10] as const,
    ['hour', 'getLineHourlyStats', hourlyRow, 20] as const,
    ['sixHour', 'getLineSixHourlyStats', sixHourlyRow, 120] as const,
  ])('treats a %s bucket one below its floor as a gap', async (granularity, fnName, rowFactory, floor) => {
    const mockFn = vi.mocked(api[fnName as keyof typeof api]) as unknown as ReturnType<typeof vi.fn>;
    mockFn.mockResolvedValue([rowFactory({ sampleCycles: floor - 1 })]);
    renderWithMantine(
      await TrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z', granularity }),
    );
    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    expect(points[0].delayRate).toBeNull();
  });
});
