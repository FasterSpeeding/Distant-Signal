import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HistoryRangePicker } from './HistoryRangePicker';

const DAY_MS = 24 * 60 * 60 * 1000;

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
}));

// Finds a day cell in the open calendar by its day-of-month label, excluding
// the greyed-out days that spill over from the adjacent month (Mantine marks
// those with `data-outside`) so a plain day number stays unambiguous. Mantine's
// popover mounts its dropdown content on a later tick (its open transition),
// so this waits for the day buttons to appear rather than querying synchronously.
async function clickDay(dayOfMonth: string) {
  const candidates = await screen.findAllByText(dayOfMonth, { selector: 'button' });
  const day = candidates.find((el) => !el.hasAttribute('data-outside'));
  if (!day) throw new Error(`No in-month day button found for "${dayOfMonth}"`);
  fireEvent.click(day);
}

function pushedUrl(): URL {
  expect(pushMock).toHaveBeenCalledTimes(1);
  return new URL(pushMock.mock.calls[0][0], 'http://localhost');
}

describe('HistoryRangePicker', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('renders Last 7 days and Last 30 days presets', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    expect(screen.getByRole('button', { name: 'Last 7 days' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Last 30 days' })).toBeInTheDocument();
  });

  it('navigates straight to a 7-day range ending now when "Last 7 days" is clicked', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Last 7 days' }));

    const url = pushedUrl();
    expect(url.pathname).toBe('/lines/wcml/history');
    const to = new Date(url.searchParams.get('to')!).getTime();
    const from = new Date(url.searchParams.get('from')!).getTime();
    expect(to).toBeGreaterThan(Date.now() - 5000); // "now", not stale
    expect(to - from).toBe(7 * DAY_MS);
  });

  it('navigates straight to a 30-day range ending now when "Last 30 days" is clicked', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Last 30 days' }));

    const url = pushedUrl();
    const to = new Date(url.searchParams.get('to')!).getTime();
    const from = new Date(url.searchParams.get('from')!).getTime();
    expect(to).toBeGreaterThan(Date.now() - 5000);
    expect(to - from).toBe(30 * DAY_MS);
  });

  it('disables "Show history" and explains that both ends are required before any date is picked', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    expect(screen.getByRole('button', { name: 'Show history' })).toBeDisabled();
    expect(screen.getByText('Pick both a start and end date to continue.')).toBeInTheDocument();
  });

  it('keeps "Show history" disabled with only one end of the range picked', async () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Pick a date range' }));
    await clickDay('10');

    expect(screen.getByRole('button', { name: 'Show history' })).toBeDisabled();
    expect(screen.getByText('Pick both a start and end date to continue.')).toBeInTheDocument();
  });

  it('enables "Show history" once both ends are picked and clears the reminder', async () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Pick a date range' }));
    await clickDay('10');
    await clickDay('15');

    expect(screen.getByRole('button', { name: 'Show history' })).toBeEnabled();
    expect(screen.queryByText('Pick both a start and end date to continue.')).not.toBeInTheDocument();
  });

  it('navigates using the from/to search-param contract when a custom range is confirmed', async () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Pick a date range' }));
    await clickDay('10');
    await clickDay('15');
    fireEvent.click(screen.getByRole('button', { name: 'Show history' }));

    const url = pushedUrl();
    expect(url.pathname).toBe('/lines/wcml/history');
    // The calendar defaults to the current month, so build the expected
    // dates off "now" rather than hardcoding a month/year. Mantine's picker
    // tracks calendar days as UTC midnight regardless of local timezone, so
    // the expected values are built the same way rather than via the local
    // `Date(y, m, d)` constructor.
    const now = new Date();
    const expectedFrom = new Date(Date.UTC(now.getFullYear(), now.getMonth(), 10)).toISOString();
    const expectedTo = new Date(Date.UTC(now.getFullYear(), now.getMonth(), 15)).toISOString();
    expect(url.searchParams.get('from')).toBe(expectedFrom);
    expect(url.searchParams.get('to')).toBe(expectedTo);
  });

  it('fills the picker and drops the reminder when a preset is used', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Last 7 days' }));

    // The bug: presets navigated without touching `value`, so the picker
    // sat empty on its placeholder and the "pick both ends" reminder stayed
    // on screen directly above a populated history list.
    expect(screen.queryByText('Pick dates range')).not.toBeInTheDocument();
    expect(screen.queryByText('Pick both a start and end date to continue.')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show history' })).toBeEnabled();
  });

  it('shows the preset range as calendar days in the picker', () => {
    renderWithMantine(<HistoryRangePicker lineId="wcml" />);
    fireEvent.click(screen.getByRole('button', { name: 'Last 30 days' }));

    const url = pushedUrl();
    const expected = [url.searchParams.get('from')!, url.searchParams.get('to')!]
      .map((iso) => new Date(iso).getUTCDate().toString());
    const picker = screen.getByRole('button', { name: /Pick a date range/ });
    for (const day of expected) {
      expect(picker.textContent).toContain(day);
    }
  });
});
