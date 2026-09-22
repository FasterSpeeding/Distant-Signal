import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyLegCard } from './JourneyLegCard';
import type { JourneyLegDetail, TrackedTrainState } from '@/lib/types';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

function trackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P9E010',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 22,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EDB',
    scheduleDestinationName: 'Edinburgh',
    scheduleCallingPoints: null,
    journeyStops: [
      {
        crs: 'KGX',
        name: 'London Kings Cross',
        tiploc: null,
        kind: 'Origin',
        scheduledArrival: null,
        scheduledDeparture: '2026-09-22T16:00:00Z',
        actualArrival: null,
        actualDeparture: '2026-09-22T16:00:00Z',
        estimatedArrival: null,
        estimatedDeparture: null,
        lastEventType: 'DEPARTURE',
        variationStatus: null,
        delayMinutes: 0,
        stopStatus: 'Called',
        skipSource: null,
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
      {
        crs: 'YRK',
        name: 'York',
        tiploc: null,
        kind: 'Intermediate',
        scheduledArrival: '2026-09-22T18:00:00Z',
        scheduledDeparture: null,
        actualArrival: null,
        actualDeparture: null,
        estimatedArrival: '2026-09-22T18:22:00Z',
        estimatedDeparture: null,
        lastEventType: null,
        variationStatus: null,
        delayMinutes: null,
        stopStatus: 'Scheduled',
        skipSource: null,
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
      {
        crs: 'EDB',
        name: 'Edinburgh',
        tiploc: null,
        kind: 'Terminate',
        scheduledArrival: '2026-09-22T20:00:00Z',
        scheduledDeparture: null,
        actualArrival: null,
        actualDeparture: null,
        estimatedArrival: '2026-09-22T20:22:00Z',
        estimatedDeparture: null,
        lastEventType: null,
        variationStatus: null,
        delayMinutes: null,
        stopStatus: 'Scheduled',
        skipSource: null,
        platform: null,
        plannedPlatform: null,
        platformChanged: false,
      },
    ],
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: trackedState(),
    legSkip: null,
    ...overrides,
  };
}

describe('JourneyLegCard', () => {
  beforeEach(() => {
    // `JourneyLegCandidates` fetches its own candidate list on mount
    // whenever it's rendered (the open-leg branch always renders it for
    // an owner; the matched branch renders it once "Change train" is
    // clicked) -- default every test to a real resolved response so that
    // effect never throws calling `.then()` on `undefined`. Individual
    // tests can still override this with `vi.mocked(fetch).mockResolvedValue(...)`.
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 })),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // 2026-09-22 UX review finding I16/2.7: the leg's own route+time, not
  // the train's headcode, is the card's title.
  it('titles a matched leg by its own route and departure time, not the train headcode', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('London Kings Cross (KGX) → York (YRK) · 17:00')).toBeInTheDocument();
  });

  it('still shows the headcode, dimmed, as secondary information', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('Train P9E010')).toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I14/2.4: "Change train" moved to the top
  // of the card, alongside the title, for a windowed leg.
  it('shows "Change train" top-of-card for a matched, windowed, owner-viewed leg', () => {
    renderWithMantine(
      <JourneyLegCard journeyId={167} leg={baseLeg({ departAfter: '18:00:00' })} isOwner isOnlyLeg={false} />,
    );
    expect(screen.getByRole('button', { name: 'Change train' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove leg' })).not.toBeInTheDocument();
  });

  // The headline fix: a no-window leg (a direct pin/known-train pick) had
  // NO recovery action at all before this. It now gets "Remove leg".
  it('shows "Remove leg" instead of "Change train" for a matched, no-window, owner-viewed leg', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByRole('button', { name: 'Remove leg' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Change train' })).not.toBeInTheDocument();
  });

  it('offers neither action to a non-owner (shared-group viewer)', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner={false} isOnlyLeg={false} />);
    expect(screen.queryByRole('button', { name: 'Remove leg' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Change train' })).not.toBeInTheDocument();
  });

  it('removing the leg calls DELETE and refreshes when a sibling leg remains', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg({ id: 5 })} isOwner isOnlyLeg={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove leg' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove leg' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove leg' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/167/legs/5', { method: 'DELETE' }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('clicking "Change train" toggles the candidate picker open', () => {
    renderWithMantine(
      <JourneyLegCard journeyId={167} leg={baseLeg({ departAfter: '18:00:00' })} isOwner isOnlyLeg={false} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Change train' }));
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  // The traveller's own alighting station, distinct from the underlying
  // train's terminus.
  it('marks the leg destination row "You get off here" in the timeline', () => {
    renderWithMantine(<JourneyLegCard journeyId={167} leg={baseLeg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByText('You get off here')).toBeInTheDocument();
  });

  it('titles an open leg by its own route, in the same date format the matched card uses', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ trackedTrainState: null, originCrs: 'YRK', destinationCrs: 'NCL' })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    // 2026-09-22 UX review finding 2.9: "YRK → NCL, 2026-09-22" (a raw ISO
    // date) is gone -- the open leg now uses the same `formatDate` the
    // matched card's own title does ("22 Sept 2026", not "2026-09-22").
    expect(screen.getByText('Pick a train — YRK → NCL, 22 Sept 2026')).toBeInTheDocument();
    expect(screen.queryByText(/2026-09-22/)).not.toBeInTheDocument();
    expect(screen.getByText('Searching for a train to track — pick one below.')).toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I18: "the window the user just typed is
  // never shown back to them" -- an open leg's own departAfter/etc. used
  // to be silently dropped from the card entirely.
  it('shows the leg\'s own search window on an open leg', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({
          trackedTrainState: null,
          originCrs: 'YRK',
          destinationCrs: 'NCL',
          departAfter: '18:00:00',
          arriveBefore: '21:30:00',
        })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(
      screen.getByText('Departing at or after 18:00 · arriving at or before 21:30'),
    ).toBeInTheDocument();
  });

  it('shows nothing extra for an open leg with no window at all', () => {
    renderWithMantine(
      <JourneyLegCard
        journeyId={167}
        leg={baseLeg({ trackedTrainState: null, originCrs: 'YRK', destinationCrs: 'NCL' })}
        isOwner
        isOnlyLeg={false}
      />,
    );
    expect(screen.queryByText(/Departing|Arriving/)).not.toBeInTheDocument();
  });
});
