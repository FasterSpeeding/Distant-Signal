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
        lineId="northern"
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
        lineId="northern"
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
        lineId="northern"
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
        lineId="northern"
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
});
