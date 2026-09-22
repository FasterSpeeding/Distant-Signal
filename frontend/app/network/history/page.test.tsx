import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NetworkHistoryPage from './page';
import * as api from '@/lib/api';
import type { HistoryRetention } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getHistoryRetention: vi.fn(),
    getNetworkDailyStats: vi.fn(),
    getNetworkHalfHourlyStats: vi.fn(),
    getNetworkHourlyStats: vi.fn(),
    getNetworkSixHourlyStats: vi.fn(),
  };
});
vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));
vi.mock('@mantine/charts', () => ({
  LineChart: () => <div data-testid="line-chart" />,
  BarChart: () => <div data-testid="bar-chart" />,
}));

const retention: HistoryRetention = {
  historyRetentionDays: 7,
  dailyStatsRetentionDays: 300,
  halfHourlyStatsRetentionHours: 840,
};

async function renderPage(searchParams: { from?: string; to?: string; range?: string; granularity?: string } = {}) {
  const element = await NetworkHistoryPage({ searchParams: Promise.resolve(searchParams) });
  return renderWithMantine(element);
}

describe('NetworkHistoryPage', () => {
  // Review M11/§3.5: matches the "History: {name}" pattern both
  // `/lines/[id]/history` and `/operators/[code]/history` already use,
  // rather than the one-off "Network history" this page shipped with.
  it('titles the page "History: Network", not "Network history" (review M11)', async () => {
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getNetworkDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByRole('heading', { name: 'History: Network', level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Network history' })).not.toBeInTheDocument();
  });

  // Review [OH] §3.4/I11: same "say the scope" line the operator history
  // page carries under its own title.
  it('states the scope under the title', async () => {
    vi.mocked(api.getHistoryRetention).mockResolvedValue(retention);
    vi.mocked(api.getNetworkDailyStats).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('Every National Rail line this app tracks (TfL not included)')).toBeInTheDocument();
  });
});
