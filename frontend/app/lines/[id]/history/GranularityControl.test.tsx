import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GranularityControl } from './GranularityControl';

const push = vi.fn();
vi.mock('next/navigation', () => ({ useRouter: () => ({ push }) }));

describe('GranularityControl', () => {
  it('renders all four options when all four are available', () => {
    renderWithMantine(
      <GranularityControl
        basePath="/lines/northern/history"
        preset="7d"
        from="2026-08-14T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    for (const label of ['30 min', 'Hourly', '6-hourly', 'Daily']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.queryByText(/are not shown for this range/)).not.toBeInTheDocument();
  });

  it('omits unavailable tiers and names them in the dimmed note', () => {
    renderWithMantine(
      <GranularityControl
        basePath="/lines/northern/history"
        preset={null}
        from="2026-07-01T00:00:00Z"
        to="2026-08-10T00:00:00Z"
        granularity="day"
        available={['sixHour', 'day']}
      />,
    );
    expect(screen.queryByText('30 min')).not.toBeInTheDocument();
    expect(screen.queryByText('Hourly')).not.toBeInTheDocument();
    expect(screen.getByText('6-hourly')).toBeInTheDocument();
    expect(screen.getByText(/30 min, Hourly are not shown for this range/)).toBeInTheDocument();
  });

  it('navigates with the preset and the new granularity when a preset range is active', () => {
    renderWithMantine(
      <GranularityControl
        basePath="/lines/northern/history"
        preset="30d"
        from="2026-07-22T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    fireEvent.click(screen.getByText('Hourly'));
    expect(push).toHaveBeenCalledWith('/lines/northern/history?range=30d&granularity=hour');
  });

  it('navigates with the raw from/to when a custom range is active (no preset)', () => {
    renderWithMantine(
      <GranularityControl
        basePath="/lines/northern/history"
        preset={null}
        from="2026-07-22T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    fireEvent.click(screen.getByText('30 min'));
    expect(push).toHaveBeenCalledWith(
      '/lines/northern/history?from=2026-07-22T00%3A00%3A00Z&to=2026-08-21T00%3A00%3A00Z&granularity=halfHour',
    );
  });
  // 2026-09-22 UX review, I9/P4: this control shipped with no
  // `aria-label`, no `aria-labelledby` and no visible label, directly
  // beneath a "Period" control that has all three.
  it('names its radiogroup, the same way the sibling Period control does', () => {
    renderWithMantine(
      <GranularityControl
        basePath="/lines/northern/history"
        preset="7d"
        from="2026-08-14T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    expect(screen.getByRole('radiogroup', { name: 'Granularity' })).toBeInTheDocument();
    // Visible, not label-only: a sighted user's only previous hint was
    // dimmed helper text BELOW the control naming the ABSENT options.
    expect(screen.getByText('Granularity')).toBeInTheDocument();
  });
});
