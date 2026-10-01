import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { WorkingTimetable } from './WorkingTimetable';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: null,
    name: null,
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Unknown',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...overrides,
  };
}

const STOPS = [
  stop({
    crs: 'EUS',
    name: 'London Euston',
    kind: 'Origin',
    scheduledDeparture: '2026-10-01T10:40:00Z',
    workingDeparture: '2026-10-01T10:40:00Z',
  }),
  // A passing point: no stop times, only a pass time.
  stop({ crs: 'WFJ', name: 'Watford Junction', tiploc: 'WATFDJ', workingPass: '2026-10-01T10:52:30Z' }),
  stop({
    crs: 'MTH',
    name: 'Motherwell',
    scheduledArrival: '2026-10-01T16:00:00Z',
    workingArrival: '2026-10-01T16:00:30Z',
    workingDeparture: '2026-10-01T16:02:00Z',
    canBoard: false,
    canAlight: true,
  }),
];

describe('WorkingTimetable', () => {
  it('is collapsed behind a disclosure button by default', () => {
    renderWithMantine(<WorkingTimetable stops={STOPS} />);
    const toggle = screen.getByRole('button', { name: 'Detailed (working timetable)' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('table', { name: 'Working timetable' })).not.toBeInTheDocument();
  });

  it('shows working times with half-minutes, passing points and direction once opened', async () => {
    renderWithMantine(<WorkingTimetable stops={STOPS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Detailed (working timetable)' }));
    expect(await screen.findByRole('table', { name: 'Working timetable' })).toBeInTheDocument();
    expect(screen.getByText('11:52½')).toBeInTheDocument();
    expect(screen.getByText('11:52 and a half')).toBeInTheDocument();
    expect(screen.getByText('Passing point')).toBeInTheDocument();
    expect(screen.getByText('17:00½')).toBeInTheDocument();
    expect(screen.getByText('Set down only')).toBeInTheDocument();
  });

  it('renders nothing when no stop has a working time (an older schedule)', () => {
    renderWithMantine(<WorkingTimetable stops={[stop({ crs: 'EUS', scheduledDeparture: '2026-10-01T10:40:00Z' })]} />);
    expect(screen.queryByRole('button', { name: 'Detailed (working timetable)' })).not.toBeInTheDocument();
  });
});
