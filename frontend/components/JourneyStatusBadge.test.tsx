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
    destinationCrs: 'WOK',
    serviceDate: '2026-08-28',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'auto',
    trackedTrainState: baseTrackedTrainState(),
    ...overrides,
  };
}

describe('JourneyStatusBadge', () => {
  it('shows "Needs a train picked" when any leg is unmatched', () => {
    renderWithMantine(
      <JourneyStatusBadge
        legs={[
          baseLeg({ id: 1, trackedTrainState: baseTrackedTrainState({ status: 'en_route', delayMinutes: null }) }),
          baseLeg({ id: 2, matchMode: 'unmatched', trackedTrainState: null }),
        ]}
      />,
    );
    expect(screen.getByText('Needs a train picked')).toBeInTheDocument();
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
});
