import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { ScheduleRow, type ScheduleRowData } from './ScheduleRow';

function row(overrides: Partial<ScheduleRowData> = {}): ScheduleRowData {
  return {
    key: 'svc-1',
    scheduled: '10:00',
    destinationCrs: 'RDG',
    destinationName: null,
    operator: 'GW',
    isCancelled: false,
    delayMinutes: 0,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...overrides,
  };
}

describe('ScheduleRow', () => {
  it('renders the scheduled time, destination and operator, falling back to the bare code when no name resolved', () => {
    renderWithMantine(<ScheduleRow row={row({ scheduled: '14:40', destinationCrs: 'BSK', operator: 'SW' })} />);
    expect(screen.getByText('14:40 · BSK · SW')).toBeInTheDocument();
  });

  // 2026-09-22 UX review follow-up (item 5): the live picker used to show
  // only the raw CRS code with no name at all -- `destinationName` is now
  // resolved server-side and rendered via the shared `stationLabel`
  // "Name (CODE)" convention.
  it('renders a resolved destination name alongside its code', () => {
    renderWithMantine(
      <ScheduleRow row={row({ scheduled: '14:40', destinationCrs: 'BSK', destinationName: 'Basingstoke', operator: 'SW' })} />,
    );
    expect(screen.getByText('14:40 · Basingstoke (BSK) · SW')).toBeInTheDocument();
  });

  it('shows a green "On time" badge with both colour and text for an on-time service', () => {
    renderWithMantine(<ScheduleRow row={row({ isCancelled: false, delayMinutes: 0 })} />);
    expect(screen.getByText('On time')).toBeInTheDocument();
  });

  it('shows an orange delay badge naming the minutes late, not colour alone', () => {
    renderWithMantine(<ScheduleRow row={row({ delayMinutes: 12 })} />);
    expect(screen.getByText('+12 min')).toBeInTheDocument();
  });

  it('shows a red "Cancelled" badge, not colour alone', () => {
    renderWithMantine(<ScheduleRow row={row({ isCancelled: true })} />);
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
  });

  it('shows the platform badge when a platform is known', () => {
    renderWithMantine(<ScheduleRow row={row({ platform: '6', plannedPlatform: '6', platformChanged: false })} />);
    expect(screen.getByText('Platform 6')).toBeInTheDocument();
  });

  it('shows a changed-platform badge naming both the current and planned platform in text', () => {
    renderWithMantine(<ScheduleRow row={row({ platform: '9', plannedPlatform: '6', platformChanged: true })} />);
    expect(screen.getByText('Platform 9 (changed from 6)')).toBeInTheDocument();
  });

  it('shows no platform badge at all when the platform is unknown', () => {
    renderWithMantine(<ScheduleRow row={row({ platform: null })} />);
    expect(screen.queryByText(/Platform/)).not.toBeInTheDocument();
  });

  it('calls onSelect when a clickable row is clicked', () => {
    const onSelect = vi.fn();
    renderWithMantine(<ScheduleRow row={row()} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole('button'));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('calls onSelect on Enter/Space when a clickable row is focused', () => {
    const onSelect = vi.fn();
    renderWithMantine(<ScheduleRow row={row()} onSelect={onSelect} />);
    fireEvent.keyDown(screen.getByRole('button'), { key: 'Enter' });
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('is not interactive at all when the row is cancelled, even with onSelect provided', () => {
    const onSelect = vi.fn();
    renderWithMantine(<ScheduleRow row={row({ isCancelled: true })} onSelect={onSelect} />);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('is not interactive when no onSelect is given at all (a plain display row)', () => {
    renderWithMantine(<ScheduleRow row={row()} />);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('omits the operator segment when none is given', () => {
    renderWithMantine(<ScheduleRow row={row({ operator: undefined, scheduled: '09:00', destinationCrs: 'WAT' })} />);
    expect(screen.getByText('09:00 · WAT')).toBeInTheDocument();
  });
});
