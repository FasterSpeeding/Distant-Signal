import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineDetailPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineStatusReport, LineSummary, CustomLineDetail, LineHalfHourlyStats } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getLineStatus: vi.fn(),
    getCustomLine: vi.fn(),
    getLineDefinition: vi.fn(),
    getAllLines: vi.fn(),
    getLineHalfHourlyStats: vi.fn(),
  };
});
// DeleteLineButton (rendered whenever Edit/Delete render) calls useRouter()
// from next/navigation, which throws outside a real Next.js App Router
// tree -- same workaround PinToggle.test.tsx/TicketPanel.test.tsx use.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  notFound: vi.fn(),
}));
// Same rationale as TrendsResults.test.tsx's own mock: this repo's
// convention is not to assert on Recharts' SVG output, so the embedded
// trends section below is exercised through the data-driven props actually
// handed to `LineChart`, not real chart rendering.
vi.mock('@mantine/charts', () => ({
  LineChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="line-chart" data-series={props.series.map((series) => series.name).join(',')} />
  ),
}));

function report(id: string, name: string): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SW'],
    computedAt: '2026-08-31T09:00:00Z',
    lineStatuses: [],
  };
}

const lines: LineSummary[] = [
  { id: 'custom-my-commute', name: 'My Commute', category: 'custom', operators: ['SW'], source: 'custom' },
];

function customLine(overrides: Partial<CustomLineDetail> = {}): CustomLineDetail {
  return {
    id: 'custom-my-commute',
    name: 'My Commute',
    operators: ['SW'],
    stations: ['WOK', 'CLJ'],
    headcodePrefixes: [],
    destinationCrsFilter: [],
    ...overrides,
  };
}

function halfHourlyStatsRow(overrides: Partial<LineHalfHourlyStats> = {}): LineHalfHourlyStats {
  return {
    halfHourStart: '2026-08-30T14:00:00Z',
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

async function renderPage(id = 'custom-my-commute') {
  const element = await LineDetailPage({ params: Promise.resolve({ id }) });
  return renderWithMantine(element);
}

describe('LineDetailPage Edit/Delete visibility', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
  });

  it('a catalogue line (getCustomLine 404s) never shows Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    await renderPage();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    // Flushes the embedded trends Suspense boundary before the test ends,
    // rather than leaving it to resolve after -- same reason the two tests
    // below explicitly wait for the trends section too.
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  it('a custom line (getCustomLine resolves) shows Edit/Delete -- ownership is already enforced by getCustomLine 404ing for anyone else', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine());
    await renderPage();
    expect(screen.getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/lines/custom-my-commute/edit');
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    await screen.findByText('Not enough sampled data yet for this line.');
  });
});

// Item 3: the detail page used to only link out to `/lines/[id]/history`
// for the trend charts -- this embeds a rolling-24h half-hourly preview of
// them directly via `HalfHourlyTrendsResults` (formerly `HourlyTrendsResults`,
// before the bucket size was halved to 30 minutes), sharing `TrendsCharts`
// (the actual chart-rendering leaf) with the history page's daily Trends
// tab, per docs/superpowers/plans/2026-09-02-trend-chart-granularity.md
// Task 13.
describe('LineDetailPage embedded trends', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine());
  });

  it('renders the trend charts when the line already has recent half-hourly stats', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([
      halfHourlyStatsRow({ halfHourStart: '2026-08-30T13:30:00Z' }),
      halfHourlyStatsRow({ halfHourStart: '2026-08-30T14:00:00Z' }),
    ]);
    await renderPage();

    expect(await screen.findByRole('heading', { name: 'Recent trends (last 24 hours)' })).toBeInTheDocument();
    const charts = await screen.findAllByTestId('line-chart');
    expect(charts).toHaveLength(2);
    expect(screen.queryByText('Not enough sampled data yet for this line.')).not.toBeInTheDocument();
    // The full range picker/Timeline/longer ranges stay one click away
    // rather than being duplicated inline.
    expect(screen.getByRole('link', { name: 'View history' })).toHaveAttribute(
      'href',
      '/lines/custom-my-commute/history',
    );
  });

  // The sane-fallback case that matters most for Task 1: a line that was
  // just created has no `line_status_daily_stats` rows yet (the aggregator
  // hasn't run a cycle for it), so this must degrade to the same honest
  // empty state `TrendsResults` already shows on the full history page --
  // not an error, not an indefinite loading state.
  it('shows the sane no-data-yet fallback for a line with no half-hourly stats -- e.g. one just created', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    await renderPage();

    expect(await screen.findByRole('heading', { name: 'Recent trends (last 24 hours)' })).toBeInTheDocument();
    expect(await screen.findByText('Not enough sampled data yet for this line.')).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
  });
});
