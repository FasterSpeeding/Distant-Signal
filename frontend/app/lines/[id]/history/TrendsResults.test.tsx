import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrendsResults, toChartPoints } from './TrendsResults';
import * as api from '@/lib/api';
import type { LineDailyStats } from '@/lib/types';

// Mirrors the component's own (intentionally unexported) floor -- see
// TrendsResults.tsx's SPARSE_DATA_FLOOR_CYCLES comment for why the exact
// value (20) is still a placeholder, not validated.
const SPARSE_DATA_FLOOR_CYCLES = 20;

vi.mock('@/lib/api');

// This repo's stated convention is not to assert on Recharts' own SVG pixel
// output (see the design spec's Testing section and TicketPanel.test.tsx's
// precedent for async Server Component tests). A light mock of
// `@mantine/charts`' `LineChart` lets these tests assert on the data-driven
// props actually passed to it -- `series`/`data`/`connectNulls` -- and on
// how many separate chart instances got rendered, without depending on
// Recharts' internal SVG rendering in jsdom.
vi.mock('@mantine/charts', () => ({
  LineChart: (props: { data: unknown[]; series: { name: string }[]; connectNulls?: boolean }) => (
    <div
      data-testid="line-chart"
      data-series={props.series.map((series) => series.name).join(',')}
      data-connect-nulls={String(props.connectNulls)}
      data-points={JSON.stringify(props.data)}
    />
  ),
}));

function row(overrides: Partial<LineDailyStats> = {}): LineDailyStats {
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

describe('toChartPoints', () => {
  it('preserves a day at or above the sparse-data floor', () => {
    const stats = [row({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES })];
    const [point] = toChartPoints(stats);
    expect(point).toEqual({
      day: '2026-08-01',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      sampleCycles: SPARSE_DATA_FLOOR_CYCLES,
    });
  });

  it('turns a day below the sparse-data floor into a gap, preserving sampleCycles', () => {
    const stats = [row({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES - 1 })];
    const [point] = toChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_CYCLES - 1);
    expect(point.day).toBe('2026-08-01');
  });
});

describe('TrendsResults', () => {
  it('renders the empty state when there are no rows', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByText('Not enough sampled data yet for this line.')).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
  });

  it('a sparse day does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([
      row({ day: '2026-08-01', sampleCycles: SPARSE_DATA_FLOOR_CYCLES - 1 }),
      row({ day: '2026-08-02', sampleCycles: 500 }),
    ]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    expect(rateChart).toBeDefined();
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseDay = points.find((point: { day: string }) => point.day === '2026-08-01');
    expect(sparseDay.delayRate).toBeNull();
    expect(sparseDay.cancellationRate).toBeNull();
    expect(sparseDay.skipRate).toBeNull();
    // Not a zero -- a genuine gap, per Decision 3.
    expect(sparseDay.delayRate).not.toBe(0);
  });

  it('a normal multi-day range renders without throwing and shows the honesty copy verbatim', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([
      row({ day: '2026-08-01' }),
      row({ day: '2026-08-02' }),
      row({ day: '2026-08-03' }),
    ]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    expect(
      screen.getByText(
        /Rates shown count each distinct train once per day, based on its status the first time it was seen that day -- not a share of poll cycles\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });

  it('renders the average-delay chart as a separate LineChart instance from the three-rate chart', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([row({ day: '2026-08-01' }), row({ day: '2026-08-02' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const charts = screen.getAllByTestId('line-chart');
    expect(charts).toHaveLength(2);
    const seriesSets = charts.map((chart) => chart.dataset.series);
    expect(seriesSets).toContain('delayRate,cancellationRate,skipRate');
    expect(seriesSets).toContain('avgDelayMinutes');
    // Never combined into one four-series chart.
    expect(seriesSets).not.toContain('delayRate,cancellationRate,skipRate,avgDelayMinutes');
  });

  it('passes connectNulls={false} to both charts so gaps render instead of interpolating', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([row({ day: '2026-08-01' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const charts = screen.getAllByTestId('line-chart');
    for (const chart of charts) {
      expect(chart.dataset.connectNulls).toBe('false');
    }
  });
});
