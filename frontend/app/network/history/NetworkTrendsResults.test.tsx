import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NetworkTrendsResults } from './NetworkTrendsResults';
import * as api from '@/lib/api';
import type { LineDailyStats } from '@/lib/types';

vi.mock('@/lib/api');
vi.mock('@mantine/charts', () => ({
  LineChart: () => <div data-testid="line-chart" />,
  BarChart: () => <div data-testid="bar-chart" />,
}));

function dailyRow(overrides: Partial<LineDailyStats> = {}): LineDailyStats {
  return {
    day: '2026-08-01',
    sampleCycles: 2000,
    total: 2000,
    delayed: 100,
    cancelled: 20,
    skipped: 5,
    avgDelayMinutes: 3.5,
    delayRate: 0.05,
    cancellationRate: 0.01,
    skipRate: 0.0025,
    ...overrides,
  };
}

describe('NetworkTrendsResults', () => {
  // Review [OH] §3.3/I10: "network-history-desktop.png shows 'simultaneously
  // running.Rates shown' with no space" -- the finding this specific test
  // guards, verified here rather than only against the (already
  // space-containing) source text, by rendering the real component with a
  // real template literal instead of an expression-adjacent text node.
  it('puts a real space between the honesty copy and the per-page scope sentence', async () => {
    vi.mocked(api.getNetworkDailyStats).mockResolvedValue([dailyRow()]);
    renderWithMantine(await NetworkTrendsResults({ from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    expect(screen.getByText(/flat line\. Rates shown are summed/)).toBeInTheDocument();
    expect(screen.queryByText(/flat line\.Rates/)).not.toBeInTheDocument();
  });

  it('does not use "--" for a dash, and avoids the word "catalogue", in the scope sentence', async () => {
    vi.mocked(api.getNetworkDailyStats).mockResolvedValue([dailyRow()]);
    const { container } = renderWithMantine(
      await NetworkTrendsResults({ from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    const paragraphs = container.querySelectorAll('p');
    for (const p of paragraphs) {
      expect(p.textContent).not.toContain('--');
      expect(p.textContent?.toLowerCase()).not.toContain('catalogue');
    }
  });

  it('collapses the fuller explanation behind a "How these rates are calculated" disclosure', async () => {
    vi.mocked(api.getNetworkDailyStats).mockResolvedValue([dailyRow()]);
    const { container } = renderWithMantine(
      await NetworkTrendsResults({ from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    expect(container.querySelector('details summary')?.textContent).toBe('How these rates are calculated');
  });
});
