import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HistoryRangePicker } from './HistoryRangePicker';

vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));

describe('HistoryRangePicker', () => {
  it('shows the active preset as filled and the inactive one as light', () => {
    renderWithMantine(
      <HistoryRangePicker
        lineId="northern"
        preset="30d"
        from="2026-07-22T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByRole('button', { name: 'Last 30 days' })).toHaveAttribute('data-variant', 'filled');
    expect(screen.getByRole('button', { name: 'Last 7 days' })).toHaveAttribute('data-variant', 'light');
  });

  it('marks the active preset for assistive technology too', () => {
    renderWithMantine(
      <HistoryRangePicker lineId="northern" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.getByRole('button', { name: 'Last 7 days' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('does not nag about picking dates once a range is already showing', () => {
    renderWithMantine(
      <HistoryRangePicker lineId="northern" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.queryByText(/Pick both a start and end date/)).not.toBeInTheDocument();
  });
});
