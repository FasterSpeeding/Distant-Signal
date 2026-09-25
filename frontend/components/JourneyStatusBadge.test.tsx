import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyStatusBadge } from './JourneyStatusBadge';
import type { JourneyLegDetail, TrackedTrainState } from '@/lib/types';

// Same fixture shape as `lib/journeyStatus.test.ts` -- kept in sync
// deliberately so both test files agree on what a "real" leg/tracked-train
// state looks like.
function baseTrackedTrainState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-08-28',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    sharedGroupCount: 0,
    customName: null,
    ...overrides,
  };
}

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'WAT',
    originName: null,
    destinationCrs: 'WOK',
    destinationName: null,
    serviceDate: '2026-08-28',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'auto',
    trackedTrainState: baseTrackedTrainState(),
    // Integration (2026-09-22): journey Phase 3 made `legSkip` a REQUIRED
    // field on `JourneyLegDetail` -- the API always emits it, `null` for a
    // leg with no matched train yet and an object once one is bound. These
    // Phase 2 factories predate that field; `null` is the right default
    // here because nothing in these suites exercises skip detection.
    legSkip: null,
    ...overrides,
  };
}

describe('JourneyStatusBadge', () => {
  // 2026-09-22 UX review finding I15/2.3: the badge now says HOW MANY
  // legs are unmatched, and is a same-page anchor to the FIRST one.
  it('shows "1 leg needs a train" and links to that leg when exactly one is unmatched', () => {
    renderWithMantine(
      <JourneyStatusBadge
        legs={[
          baseLeg({ id: 1, trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }) }),
          baseLeg({ id: 2, matchMode: 'unmatched', trackedTrainState: null }),
        ]}
      />,
    );
    const badge = screen.getByText('1 leg needs a train');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('a')).toHaveAttribute('href', '#leg-2');
  });

  it('shows "N legs need a train" (plural) and links to the FIRST unmatched leg when several are', () => {
    renderWithMantine(
      <JourneyStatusBadge
        legs={[
          baseLeg({ id: 1, matchMode: 'unmatched', trackedTrainState: null }),
          baseLeg({ id: 2, matchMode: 'unmatched', trackedTrainState: null }),
        ]}
      />,
    );
    const badge = screen.getByText('2 legs need a train');
    expect(badge.closest('a')).toHaveAttribute('href', '#leg-1');
  });

  it('shows "Cancelled" when the worst leg is cancelled even if another is merely unmatched', () => {
    renderWithMantine(
      <JourneyStatusBadge
        legs={[
          baseLeg({ id: 1, matchMode: 'unmatched', trackedTrainState: null }),
          baseLeg({ id: 2, trackedTrainState: baseTrackedTrainState({ status: 'cancelled' }) }),
        ]}
      />,
    );
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
    expect(screen.queryByText('Needs a train picked')).not.toBeInTheDocument();
  });

  it('renders nothing for an empty leg list', () => {
    // Not `container.textContent`/`toBeEmptyDOMElement()` -- MantineProvider
    // injects `<style>` tags into the render tree (see
    // `TrackedTrainStatusBadge.test.tsx`'s own comment on the same issue),
    // so the container is never literally empty. No `.mantine-Badge-root`
    // at all is the actual claim this test makes.
    const { container } = renderWithMantine(<JourneyStatusBadge legs={[]} />);
    expect(container.querySelectorAll('.mantine-Badge-root')).toHaveLength(0);
  });

  // Review §2.5/M17: no `Tooltip` wrapper repeating the badge's own visible
  // text -- it added nothing for a sighted user and was unreachable by
  // keyboard (a bare `Badge` isn't a focusable tooltip trigger). Mantine's
  // `Tooltip` only mounts its floating content on hover/focus, so the
  // meaningful assertion is structural: the badge renders as a direct
  // child, not wrapped in whatever trigger element `Tooltip` adds.
  it('renders the badge directly, with no wrapping tooltip trigger', () => {
    const { container } = renderWithMantine(
      <JourneyStatusBadge legs={[baseLeg({ matchMode: 'unmatched', trackedTrainState: null })]} />,
    );
    const badge = container.querySelector('.mantine-Badge-root');
    expect(badge?.parentElement).toBe(container);
  });
});
