import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackedRowStatusBadge } from './TrackedRowStatusBadge';
import type { TrackedRowStatus, TrackedRowStatusKind } from '@/lib/trackedRowStatus';

function status(kind: TrackedRowStatusKind, overrides: Partial<TrackedRowStatus> = {}): TrackedRowStatus {
  return { kind, finished: false, delayMinutes: null, delayProvisional: false, serviceMode: null, ...overrides };
}

function toneOf(text: string): string | null {
  return screen.getByText(text).closest('[data-state-badge]')?.getAttribute('data-state-badge') ?? null;
}

describe('TrackedRowStatusBadge', () => {
  it.each([
    ['planned', 'Planned', 'neutral'],
    ['awaiting', 'Awaiting first report', 'neutral'],
    ['noTimetable', 'Not in the timetable', 'action'],
    ['unmatched', 'Needs a train picked', 'action'],
    ['severe', 'Cancelled', 'critical'],
    ['legCancelled', 'Leg cancelled', 'critical'],
    ['noReport', 'No report received', 'neutral'],
    ['noTrainPicked', 'No train picked', 'neutral'],
    ['good', 'On time', 'good'],
    ['delayed', 'Delayed', 'caution'],
  ] as const)('%s reads "%s" (%s)', (kind, label, tone) => {
    renderWithMantine(<TrackedRowStatusBadge status={status(kind)} />);
    expect(toneOf(label)).toBe(tone);
  });

  it('shows the delay badge in place of "Delayed" once the delay is known', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('delayed', { delayMinutes: 12 })} />);
    expect(screen.getByText('12 min late')).toBeInTheDocument();
    expect(screen.queryByText('Delayed')).not.toBeInTheDocument();
  });

  it('a completed row keeps its final delay', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('completed', { finished: true, delayMinutes: 0 })} />);
    expect(toneOf('Completed')).toBe('neutral');
    expect(screen.getByText('On time')).toBeInTheDocument();
  });

  it('a cancelled row stays red once finished', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('severe', { finished: true, delayMinutes: 5 })} />);
    expect(toneOf('Cancelled')).toBe('critical');
    expect(screen.queryByText('5 min late')).not.toBeInTheDocument();
  });

  it('a bus says what it is and that it is timetabled only', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('timetableOnly', { serviceMode: 'bus' })} />);
    expect(screen.getByText('Bus service')).toBeInTheDocument();
    expect(toneOf('Timetabled only')).toBe('neutral');
  });
});
