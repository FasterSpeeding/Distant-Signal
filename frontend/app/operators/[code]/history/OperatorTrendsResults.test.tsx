import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { OperatorTrendsResults } from './OperatorTrendsResults';
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

describe('OperatorTrendsResults', () => {
  // Review [OH] §3.3/I10: this is the operator-page half of the
  // "running.Rates" missing-space finding -- rendered via a single template
  // literal now (see the component's own comment), which cannot suffer the
  // expr-adjacent-text whitespace collapse `GranularityControl.tsx`
  // documents for the same bug class.
  it('puts a real space between the honesty copy and the per-page scope sentence', async () => {
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([dailyRow()]);
    renderWithMantine(await OperatorTrendsResults({ code: 'GR', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    expect(screen.getByText(/flat line\. Rates shown are summed/)).toBeInTheDocument();
    expect(screen.queryByText(/flat line\.Rates/)).not.toBeInTheDocument();
  });

  it('does not use "--" for a dash, and avoids the word "rollup", in the scope sentence', async () => {
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([dailyRow()]);
    const { container } = renderWithMantine(
      await OperatorTrendsResults({ code: 'GR', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    const paragraphs = container.querySelectorAll('p');
    for (const p of paragraphs) {
      expect(p.textContent).not.toContain('--');
      expect(p.textContent?.toLowerCase()).not.toContain('rollup');
    }
  });

  it('collapses the fuller explanation behind a "How these rates are calculated" disclosure', async () => {
    vi.mocked(api.getOperatorDailyStats).mockResolvedValue([dailyRow()]);
    const { container } = renderWithMantine(
      await OperatorTrendsResults({ code: 'GR', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }),
    );

    expect(container.querySelector('details summary')?.textContent).toBe('How these rates are calculated');
  });
});
