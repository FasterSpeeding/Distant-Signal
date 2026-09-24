import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackJourneyAgainButton } from './TrackJourneyAgainButton';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

function journey(legs: JourneyLegDetail[], isOwner = true): JourneyDetail {
  return { id: 167, customName: null, createdAt: '2026-09-22T00:00:00Z', legs, isOwner, shareLink: null };
}

describe('TrackJourneyAgainButton', () => {
  beforeEach(() => pushMock.mockClear());

  it('navigates to a pick-mode /track URL for a leg with no window', () => {
    renderWithMantine(<TrackJourneyAgainButton journey={journey([leg()])} />);
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey again' }));
    expect(pushMock).toHaveBeenCalledWith('/track?mode=pick&origin=KGX&destination=YRK');
  });

  it('navigates to a window-mode /track URL for a leg with window bounds', () => {
    renderWithMantine(
      <TrackJourneyAgainButton
        journey={journey([leg({ departAfter: '08:00:00', matchMode: 'unmatched' })])}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey again' }));
    expect(pushMock).toHaveBeenCalledWith('/track?mode=window&origin=KGX&destination=YRK&departAfter=08%3A00');
  });

  it('renders for a non-owner (shared-group viewer) too -- see Judgment Call 7', () => {
    renderWithMantine(<TrackJourneyAgainButton journey={journey([leg()], false)} />);
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });

  it('renders nothing when there is no origin to reproduce', () => {
    renderWithMantine(
      <TrackJourneyAgainButton
        journey={journey([leg({ originCrs: null, destinationCrs: null })])}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Track this journey again' })).not.toBeInTheDocument();
  });
});
