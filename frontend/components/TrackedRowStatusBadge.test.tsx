import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyStatusGroupBadge, TrackedRowStatusBadge } from './TrackedRowStatusBadge';
import type { TrackedRowStatus, TrackedRowStatusKind } from '@/lib/trackedRowStatus';
import type { ListCancellation } from '@/lib/types';

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
    ['partCancelled', 'Part cancelled', 'critical'],
    ['connectionAtRisk', 'Tight connection', 'caution'],
    ['connectionMissed', 'Connection missed', 'late'],
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

  it('part cancelled sits beside the final arrival delay', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('partCancelled', { delayMinutes: 0 })} />);
    expect(toneOf('Part cancelled')).toBe('critical');
    expect(screen.getByText('On time')).toBeInTheDocument();
  });

  it('a bus says what it is and that it is timetabled only', () => {
    renderWithMantine(<TrackedRowStatusBadge status={status('timetableOnly', { serviceMode: 'bus' })} />);
    expect(screen.getByText('Bus service')).toBeInTheDocument();
    expect(toneOf('Timetabled only')).toBe('neutral');
  });
});

describe('TrackedRowStatusBadge: a train cancelled in whole or part (DS-181)', () => {
  const york = { crs: 'YRK', name: 'York' };
  const reason = { code: 'JP', text: 'Failure to maintain vegetation', source: 'trust' } as const;
  function cancellation(overrides: Partial<ListCancellation> = {}): ListCancellation {
    return { kind: 'partial', startsFrom: null, terminatesAt: null, reason: null, ...overrides };
  }

  /** The visible badge's text and tone, the dimmed reason line, and what a
   * screen reader hears for the group. */
  function rendered(rowStatus: TrackedRowStatus) {
    const { container } = renderWithMantine(<TrackedRowStatusBadge status={rowStatus} />);
    const badge = container.querySelector('[data-state-badge="critical"]');
    const group = container.querySelector('[data-row-cancellation]');
    return {
      badge: badge?.textContent ?? null,
      badgeHidden: badge?.getAttribute('aria-hidden') ?? null,
      reason: container.querySelector('[data-row-cancellation-reason]')?.textContent ?? null,
      kind: group?.getAttribute('data-row-cancellation') ?? null,
    };
  }

  it('says where a part-cancelled train terminates, with the reason under it', () => {
    const view = rendered(
      status('partCancelled', { cancellation: cancellation({ terminatesAt: york, reason }), delayMinutes: 0 }),
    );
    expect(view).toEqual({
      badge: 'Terminates at York',
      badgeHidden: 'true',
      reason: 'Failure to maintain vegetation',
      kind: 'partial',
    });
    expect(
      screen.getByText('This train terminates at York. Reason: Failure to maintain vegetation.'),
    ).toBeInTheDocument();
    // The final delay still sits beside it, and stays readable.
    expect(screen.getByText('On time')).toBeInTheDocument();
    expect(screen.queryByText('Part cancelled')).not.toBeInTheDocument();
  });

  it('says where a part-cancelled train starts from, with no reason line when none is given', () => {
    const view = rendered(status('partCancelled', { cancellation: cancellation({ startsFrom: york }) }));
    expect(view).toMatchObject({ badge: 'Starts from York', reason: null });
    expect(screen.getByText('This train starts from York.')).toBeInTheDocument();
  });

  it('a cancelled middle section reads "Part cancelled"', () => {
    const view = rendered(status('partCancelled', { cancellation: cancellation({ reason }) }));
    expect(view).toMatchObject({ badge: 'Part cancelled', reason: 'Failure to maintain vegetation' });
    expect(
      screen.getByText('This train is part cancelled. Reason: Failure to maintain vegetation.'),
    ).toBeInTheDocument();
  });

  it('a wholly cancelled train keeps "Cancelled" and adds the reason', () => {
    const view = rendered(status('severe', { cancellation: cancellation({ kind: 'full', reason }) }));
    expect(view).toEqual({
      badge: 'Cancelled',
      badgeHidden: 'true',
      reason: 'Failure to maintain vegetation',
      kind: 'full',
    });
    expect(screen.getByText('This train is cancelled. Reason: Failure to maintain vegetation.')).toBeInTheDocument();
  });

  it('without a cancellation, "Cancelled" and "Part cancelled" render as before', () => {
    for (const kind of ['severe', 'partCancelled'] as const) {
      const { container, unmount } = renderWithMantine(
        <TrackedRowStatusBadge status={status(kind, { cancellation: null })} />,
      );
      expect(container.querySelector('[data-row-cancellation]')).toBeNull();
      expect(container.querySelector('[aria-hidden="true"]')).toBeNull();
      unmount();
    }
  });

  it('a cancellation never rewords a row that is not cancelled', () => {
    renderWithMantine(
      <TrackedRowStatusBadge status={status('good', { cancellation: cancellation(), delayMinutes: 0 })} />,
    );
    expect(screen.getByText('On time')).toBeInTheDocument();
    expect(screen.queryByText('Part cancelled')).not.toBeInTheDocument();
  });

  it('the home page group badge words a cancellation the same way', () => {
    renderWithMantine(
      <JourneyStatusGroupBadge group="partCancelled" cancellation={cancellation({ terminatesAt: york })} />,
    );
    expect(screen.getByText('Terminates at York')).toBeInTheDocument();
    expect(screen.getByText('This train terminates at York.')).toBeInTheDocument();
  });
});
