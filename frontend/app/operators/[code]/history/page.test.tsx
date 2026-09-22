import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import OperatorHistoryPage from './page';
import * as api from '@/lib/api';
import type { HistoryRetention, OperatorSummary, Suggestion } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getAllTocs: vi.fn(),
    getHistoryRetention: vi.fn(),
    getOperator: vi.fn(),
    getOperatorDailyStats: vi.fn(),
    getOperatorHalfHourlyStats: vi.fn(),
    getOperatorHourlyStats: vi.fn(),
    getOperatorSixHourlyStats: vi.fn(),
  };
});
vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));
// This repo's convention (see TrendsResults.test.tsx): don't assert on
// Recharts' own SVG output, just that the wrapping components render.
vi.mock('@mantine/charts', () => ({
  LineChart: () => <div data-testid="line-chart" />,
  BarChart: () => <div data-testid="bar-chart" />,
}));

const tocs: Suggestion[] = [{ code: 'GR', name: 'London North Eastern Railway' }];

function operator(overrides: Partial<OperatorSummary> = {}): OperatorSummary {
  return {
    code: 'GR',
    name: 'London North Eastern Railway',
    lineIds: ['ecml', 'grand-central'],
    worstSeverity: 6,
    reason: 'Signalling failure',
    computedAt: '2026-09-22T00:00:00Z',
    ...overrides,
  };
}

const retention: HistoryRetention = {
  historyRetentionDays: 7,
  dailyStatsRetentionDays: 300,
  halfHourlyStatsRetentionHours: 840,
};

async function renderPage(searchParams: { from?: string; to?: string; range?: string; granularity?: string } = {}) {
  const element = await OperatorHistoryPage({
    params: Promise.resolve({ code: 'GR' }),
    searchParams: Promise.resolve(searchParams),
  });
  return renderWithMantine(element);
}

describe('OperatorHistoryPage', () => {
  it('titles the page "History: {operator name}"', async () => {
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getOperator).mockResolvedValue(operator());
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(
      screen.getByRole('heading', { name: 'History: London North Eastern Railway', level: 1 }),
    ).toBeInTheDocument();
  });

  // Review [OH] §3.4/I11: "History: London North Eastern Railway" used to
  // give no sense of scope until the last sentence of the methodology
  // paragraph -- this line under the title states it up front.
  it('shows how many lines the operator rollup covers, under the title', async () => {
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getOperator).mockResolvedValue(operator({ lineIds: ['ecml', 'grand-central'] }));
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('2 lines')).toBeInTheDocument();
  });

  it('uses singular "line" for a single-line operator', async () => {
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getOperator).mockResolvedValue(operator({ lineIds: ['ecml'] }));
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('1 line')).toBeInTheDocument();
  });

  it('omits the line-count line rather than showing a wrong one when the operator rollup fetch fails', async () => {
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getOperator).mockRejectedValue(new Error('network error'));
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(screen.queryByText(/^\d+ lines?$/)).not.toBeInTheDocument();
    // The page itself must still render -- a failed line-count fetch is
    // not fatal to the page.
    expect(
      screen.getByRole('heading', { name: 'History: London North Eastern Railway', level: 1 }),
    ).toBeInTheDocument();
  });
});
