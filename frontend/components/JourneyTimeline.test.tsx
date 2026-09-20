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

  // Task 3.6.2: a stop whose server-side TIPLOC->CRS->name join didn't
  // resolve gets a by-index placeholder, never the old "Unknown location"
  // string.
  it('falls back to a by-index placeholder ("Stop N") when neither name nor CRS is known', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Terminate' }),
        ]}
      />,
    );
    expect(screen.getByText('Stop 2')).toBeInTheDocument();
    expect(screen.queryByText('Unknown location')).not.toBeInTheDocument();
  });

  // The first/last row is special-cased: the tracked pin's own
  // origin/destination is always known, even when this particular stop's
  // own name/CRS didn't resolve, so it seeds the label instead of falling
  // through to "Stop 1"/"Stop N".
  it('seeds the first/last row from the tracked pin origin/destination when the stop itself has no name or CRS', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: null, name: null, kind: 'Origin' }),
          stop({ crs: 'RDG', name: 'Reading', kind: 'Intermediate' }),
          stop({ crs: null, name: null, kind: 'Terminate' }),
        ]}
        endpointNames={{ originName: 'London Waterloo', destinationName: 'Woking' }}
      />,
    );
    expect(screen.getByText('London Waterloo')).toBeInTheDocument();
    expect(screen.getByText('Woking')).toBeInTheDocument();
    expect(screen.queryByText('Stop 1')).not.toBeInTheDocument();
    expect(screen.queryByText('Stop 3')).not.toBeInTheDocument();
  });

  // Genuinely degenerate case: nothing at all resolved, not even the pin's
  // own origin/destination -- N rows of "Stop 1"/"Stop 2"/... would look
  // like a real (if terse) timetable, so this collapses to one honest,
  // dimmed line instead and renders no table at all.
  it('collapses to a single dimmed line when every stop is unnamed, with no table rendered', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: null, name: null, kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
          stop({ crs: null, name: null, kind: 'Terminate' }),
        ]}
      />,
    );
    expect(screen.getByText('3 stops — station names unavailable')).toBeInTheDocument();
    expect(screen.queryByRole('table')).not.toBeInTheDocument();
    expect(screen.queryByText(/Stop \d/)).not.toBeInTheDocument();
  });

  it('does not collapse when only some stops are unnamed', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
        ]}
      />,
    );
    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.queryByText(/station names unavailable/)).not.toBeInTheDocument();
  });
});
