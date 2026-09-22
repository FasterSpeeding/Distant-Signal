import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { theme } from '@/lib/theme';
import { HistoryRangePicker } from './HistoryRangePicker';

vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));

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
      <HistoryRangePicker basePath="/lines/northern/history" preset="30d" from="2026-07-22T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
    expect(screen.getByRole('radio', { name: '7 days' })).not.toBeChecked();
  });

  it('does not show the date picker or nag about picking dates while a preset is active', () => {
    renderWithMantine(
      <HistoryRangePicker basePath="/lines/northern/history" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.queryByText('Pick a date range')).not.toBeInTheDocument();
    expect(screen.queryByText(/Pick both a start and end date/)).not.toBeInTheDocument();
  });

  it('shows the date picker and "Show history" once Custom… is selected', () => {
    renderWithMantine(
      <HistoryRangePicker basePath="/lines/northern/history" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    fireEvent.click(screen.getByRole('radio', { name: 'Custom…' }));
    expect(screen.getByText('Pick a date range')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show history' })).toBeInTheDocument();
  });

  it('shows Custom… as selected (not an undefined state) when the resolved range has no preset', () => {
    renderWithMantine(
      <HistoryRangePicker basePath="/lines/northern/history" preset={null} from="2026-08-01T12:00:00Z" to="2026-08-21T12:00:00Z" />,
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
      <HistoryRangePicker basePath="/lines/northern/history" preset={null} from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.getByDisplayValue('2026-08-14 – 2026-08-21')).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <HistoryRangePicker basePath="/lines/northern/history" preset={null} from="2026-07-22T12:00:00Z" to="2026-08-21T12:00:00Z" />
      </MantineProvider>,
    );

    expect(screen.getByDisplayValue('2026-07-22 – 2026-08-21')).toBeInTheDocument();
  });

  it('switches back to a preset segment, hiding the picker again, when a preset prop change follows', () => {
    // Simulates the effect of clicking "7 days" while Custom was showing:
    // `handlePreset` navigates, and the page re-renders this instance with
    // a fresh `preset` prop rather than remounting it.
    const { rerender } = renderWithMantine(
      <HistoryRangePicker basePath="/lines/northern/history" preset={null} from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.getByText('Pick a date range')).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <HistoryRangePicker basePath="/lines/northern/history" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />
      </MantineProvider>,
    );

    expect(screen.getByRole('radio', { name: '7 days' })).toBeChecked();
    expect(screen.queryByText('Pick a date range')).not.toBeInTheDocument();
  });
});
