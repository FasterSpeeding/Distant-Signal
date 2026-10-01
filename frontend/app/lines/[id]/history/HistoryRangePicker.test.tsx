import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
import { HistoryRangePicker, londonTodayDayProps } from './HistoryRangePicker';

const pushMock = vi.hoisted(() => vi.fn());
vi.mock('next/navigation', () => ({ useRouter: () => ({ push: pushMock }) }));

describe('HistoryRangePicker', () => {
  it('labels the period control and gives it an accessible name', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset="30d"
        from="2026-07-22T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByText('Period')).toBeInTheDocument();
    expect(screen.getByRole('radiogroup', { name: 'Period' })).toBeInTheDocument();
  });

  it('marks the active preset as the checked segment', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset="30d"
        from="2026-07-22T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
    expect(screen.getByRole('radio', { name: '7 days' })).not.toBeChecked();
  });

  it('does not show the date picker or nag about picking dates while a preset is active', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset="7d"
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.queryByText('Pick a date range')).not.toBeInTheDocument();
    expect(screen.queryByText(/Pick both a start and end date/)).not.toBeInTheDocument();
  });

  it('shows the date picker and "Show history" once Custom… is selected', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset="7d"
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    fireEvent.click(screen.getByRole('radio', { name: 'Custom…' }));
    expect(screen.getByText('Pick a date range')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show history' })).toBeInTheDocument();
  });

  it('shows Custom… as selected (not an undefined state) when the resolved range has no preset', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-01T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByRole('radio', { name: 'Custom…' })).toBeChecked();
    expect(screen.getByText('Pick a date range')).toBeInTheDocument();
  });

  it('resyncs the displayed range when from/to/preset props change on an already-rendered instance', () => {
    // A client-side navigation (e.g. clicking a preset) re-renders this
    // component with fresh props rather than remounting it, so the fix has
    // to be verified with `rerender`, not a second fresh `render` — a
    // `useState` initializer alone would pass a test that only checked a
    // fresh mount's initial value while still going stale in the app.
    const { rerender } = renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByDisplayValue('2026-08-14 – 2026-08-21')).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <HistoryRangePicker
          basePath="/lines/northern/history"
          preset={null}
          from="2026-07-22T12:00:00Z"
          to="2026-08-21T12:00:00Z"
        />
      </MantineProvider>,
    );

    expect(screen.getByDisplayValue('2026-07-22 – 2026-08-21')).toBeInTheDocument();
  });

  it('switches back to a preset segment, hiding the picker again, when a preset prop change follows', () => {
    // Simulates the effect of clicking "7 days" while Custom was showing:
    // `handlePreset` navigates, and the page re-renders this instance with
    // a fresh `preset` prop rather than remounting it.
    const { rerender } = renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByText('Pick a date range')).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <HistoryRangePicker
          basePath="/lines/northern/history"
          preset="7d"
          from="2026-08-14T12:00:00Z"
          to="2026-08-21T12:00:00Z"
        />
      </MantineProvider>,
    );

    expect(screen.getByRole('radio', { name: '7 days' })).toBeChecked();
    expect(screen.queryByText('Pick a date range')).not.toBeInTheDocument();
  });

  // FE-5: the picker shows and submits London calendar days.
  it('displays the London day of a London-evening bound, not the UTC day', () => {
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-13T23:30:00Z"
        to="2026-08-20T23:30:00Z"
      />,
    );
    expect(screen.getByDisplayValue('2026-08-14 – 2026-08-21')).toBeInTheDocument();
  });

  it('submits London-day bounds covering the whole of the end day', () => {
    pushMock.mockClear();
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Show history' }));
    expect(pushMock).toHaveBeenCalledWith(
      '/lines/northern/history?from=2026-08-13T23:00:00.000Z&to=2026-08-21T22:59:59.999Z',
    );
  });
});

// Mantine's `highlightToday` marks the browser's local day; this page groups
// history by London day, so the marker must sit on London's today.
describe("HistoryRangePicker's today marker", () => {
  const originalTz = process.env.TZ;
  afterEach(() => {
    vi.useRealTimers();
    if (originalTz === undefined) {
      delete process.env.TZ;
    } else {
      process.env.TZ = originalTz;
    }
  });

  it('londonTodayDayProps marks only the given day', () => {
    expect(londonTodayDayProps('2026-08-21', '2026-08-21')).toEqual({
      'data-today': true,
      'data-highlight-today': true,
    });
    expect(londonTodayDayProps('2026-08-22', '2026-08-21')).toEqual({});
  });

  it("highlights London's today, not the day a visitor ahead of UK time is already on", async () => {
    process.env.TZ = 'Asia/Tokyo'; // UTC+9
    vi.useFakeTimers({ shouldAdvanceTime: true });
    // 16:30Z: 17:30 on 21 Aug in London, already 01:30 on 22 Aug in Tokyo.
    vi.setSystemTime(new Date('2026-08-21T16:30:00.000Z'));
    renderWithMantine(
      <HistoryRangePicker
        basePath="/lines/northern/history"
        preset={null}
        from="2026-08-14T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Pick a date range' }));
    // By aria-label, not role: the dropdown is still mid-transition when its
    // day buttons are rendered.
    const day = (label: string) =>
      waitFor(() => {
        const el = document.querySelector<HTMLElement>(`[aria-label="${label}"]`);
        if (!el) throw new Error(`no day ${label}`);
        return el;
      });
    // Mantine's today style needs both attributes.
    const londonToday = await day('21 August 2026');
    expect(londonToday).toHaveAttribute('data-today', 'true');
    expect(londonToday).toHaveAttribute('data-highlight-today', 'true');
    // Mantine still tags the browser's day `data-today`, but unhighlighted.
    expect(await day('22 August 2026')).not.toHaveAttribute('data-highlight-today');
  });
});
