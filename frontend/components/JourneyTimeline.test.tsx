import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyTimeline } from './JourneyTimeline';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'RDG',
    name: 'Reading',
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
    ...overrides,
  };
}

describe('JourneyTimeline', () => {
  it('renders a station name for every stop, in order', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Terminate' }),
        ]}
      />,
    );
    const names = screen.getAllByText(/Reading|London Waterloo/);
    expect(names[0]).toHaveTextContent('Reading');
    expect(names[1]).toHaveTextContent('London Waterloo');
  });

  it('falls back to the CRS code when no station name is known', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ crs: 'ZZZ', name: null })]} />);
    expect(screen.getByText('ZZZ')).toBeInTheDocument();
  });

  it('shows only the scheduled time for a stop with no actual time yet', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ scheduledDeparture: '2026-09-08T08:00:00Z', actualDeparture: null })]}
      />,
    );
    expect(screen.queryByText(/late|early|on time/i)).not.toBeInTheDocument();
  });

  it('shows a late badge for a positive delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:04:00Z',
            delayMinutes: 4,
          }),
        ]}
      />,
    );
    expect(screen.getByText('4m late')).toBeInTheDocument();
  });

  it('shows an early badge for a negative delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: '2026-09-08T07:59:00Z',
            delayMinutes: -1,
          }),
        ]}
      />,
    );
    expect(screen.getByText('1m early')).toBeInTheDocument();
  });

  it('shows an on-time badge for a zero delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:00:00Z',
            delayMinutes: 0,
          }),
        ]}
      />,
    );
    expect(screen.getByText('On time')).toBeInTheDocument();
  });

  it('renders as a table with a column for each fact shown', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({})]} />);
    const table = screen.getByRole('table', { name: 'Journey timeline' });
    expect(table).toBeInTheDocument();
    expect(screen.getByText('Station')).toBeInTheDocument();
    expect(screen.getByText('Scheduled')).toBeInTheDocument();
    expect(screen.getByText('Actual / est.')).toBeInTheDocument();
    expect(screen.getByText('Delay')).toBeInTheDocument();
  });

  it('shows an estimated time, prefixed and visually distinguished, for a stop with no actual time yet', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: null,
            estimatedArrival: '2026-09-08T08:05:00Z',
          }),
        ]}
      />,
    );
    const estimate = screen.getByText(/est\. 09:05/);
    expect(estimate).toBeInTheDocument();
    // Italicised/muted -- visually distinct from a confirmed actual time,
    // not just distinguishable by its "est." prefix.
    expect(estimate).toHaveStyle({ fontStyle: 'italic' });
  });

  it('never shows an estimated time alongside a confirmed actual time', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: '2026-09-08T08:04:00Z',
            estimatedArrival: '2026-09-08T08:05:00Z',
          }),
        ]}
      />,
    );
    expect(screen.queryByText(/^est\./)).not.toBeInTheDocument();
    expect(screen.getByText('09:04')).toBeInTheDocument();
  });
});
