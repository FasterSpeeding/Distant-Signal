import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyDetailView } from './JourneyDetailView';
import { journeyDetail, trainLeg } from '@/test/journeyFixtures';
import { openBreakSummary, suggestionSet } from '@/test/replanFixtures';
import type { JourneyDetail } from '@/lib/types';

/* The journey page's mid-journey changes (spec 2026-10-22 §8.2): which
 * leg offers "I missed this train" and "Find another way", the open
 * break's banner, and "Earlier plans". */

const { refreshMock } = vi.hoisted(() => ({ refreshMock: vi.fn() }));

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: refreshMock }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

const KGX: [string, string] = ['KGX', 'London Kings Cross'];
const YRK: [string, string] = ['YRK', 'York'];
const NCL: [string, string] = ['NCL', 'Newcastle'];
const EDB: [string, string] = ['EDB', 'Edinburgh'];

/** Three trains on 17 Sep 2026: the first arrived, the second still to
 * go, the third cancelled. */
function journey(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
  return {
    ...journeyDetail(42, [
      trainLeg(1, KGX, YRK, '2026-09-17T07:00:00Z', '2026-09-17T09:00:00Z', { status: 'completed' }),
      trainLeg(2, YRK, NCL, '2026-09-17T10:00:00Z', '2026-09-17T11:00:00Z'),
      trainLeg(3, NCL, EDB, '2026-09-17T11:30:00Z', '2026-09-17T13:00:00Z', { status: 'cancelled' }),
    ]),
    ...overrides,
  };
}

function renderView(detail: JourneyDetail) {
  return renderWithMantine(
    <JourneyDetailView journey={detail} fetchedAt="2026-09-17T09:30:00Z" origin="https://example.test" />,
  );
}

function legCard(id: number): HTMLElement {
  const card = document.getElementById(`leg-${id}`);
  if (card === null) throw new Error(`no leg ${id}`);
  return card;
}

describe('JourneyDetailView mid-journey changes', () => {
  it('offers "I missed this train" on the next train only, and "Find another way" on the cancelled one', () => {
    renderView(journey());
    expect(screen.getAllByRole('button', { name: 'I missed this train' })).toHaveLength(1);
    expect(within(legCard(2)).getByRole('button', { name: 'I missed this train' })).toBeInTheDocument();
    expect(within(legCard(1)).queryByRole('button', { name: 'I missed this train' })).not.toBeInTheDocument();
    expect(within(legCard(3)).getByRole('button', { name: 'Find another way' })).toBeInTheDocument();
    // Phase 0's "Change train" stays on every train leg.
    expect(screen.getAllByRole('button', { name: 'Change train' })).toHaveLength(3);
  });

  it('offers none of them to someone who is not the owner', () => {
    renderView(journey({ isOwner: false, revisionCount: 3 }));
    expect(screen.queryByRole('button', { name: 'I missed this train' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Find another way' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Earlier plans/ })).not.toBeInTheDocument();
  });

  it('shows the open break above the legs and "Find another way" on its trigger', () => {
    renderView(journey({ openBreak: openBreakSummary({ triggerLegId: 2 }) }));
    const banner = screen.getByRole('region', { name: 'Connection at York missed' });
    expect(banner.compareDocumentPosition(legCard(1)) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(legCard(2)).getByRole('button', { name: 'Find another way' })).toBeInTheDocument();
  });

  it('lists earlier plans when the journey has had more than one', () => {
    renderView(journey({ revisionCount: 3 }));
    expect(screen.getByRole('button', { name: 'Earlier plans (2)' })).toBeInTheDocument();
  });

  it('has no earlier plans with only the booked one', () => {
    renderView(journey({ revisionCount: 1 }));
    expect(screen.queryByRole('button', { name: /Earlier plans/ })).not.toBeInTheDocument();
  });
});

/* The prod report (2026-10-22): "You missed the ..." on the first leg of
 * a journey whose train left on time. A departed first leg is never
 * missed on the page's own say-so, and looking at what "I missed this
 * train" offers records nothing. */
describe('JourneyDetailView missed notices', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    refreshMock.mockReset();
  });

  /** One train, KGX 09:00Z to YRK 11:00Z, on its way and on time; the
   * page fetched at 09:30Z, after it left. */
  function underway(overrides: Partial<JourneyDetail> = {}): JourneyDetail {
    return {
      ...journeyDetail(42, [
        trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
          status: 'en_route',
          delayMinutes: 0,
        }),
      ]),
      ...overrides,
    };
  }

  it('shows no missed notice for an on-time first leg after it departed', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(suggestionSet()), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    renderView(underway());
    expect(screen.queryByText(/You missed/)).not.toBeInTheDocument();
    expect(document.querySelector('[data-journey-break]')).toBeNull();

    // Opening "I missed this train" and closing it again changes nothing.
    fireEvent.click(within(legCard(1)).getByRole('button', { name: 'I missed this train' }));
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    // The open sheet itself says nothing was missed either.
    expect(screen.getByRole('heading', { name: 'Other ways to Edinburgh' })).toBeInTheDocument();
    expect(screen.queryByText(/You missed/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByText(/You missed/)).not.toBeInTheDocument());
    expect(document.querySelector('[data-journey-break]')).toBeNull();
    expect(refreshMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/legs/1/alternatives?cause=missed', { method: 'GET' });
  });

  it('says "You missed" once the traveller has marked the first leg missed', () => {
    renderView(
      underway({
        openBreak: openBreakSummary({ cause: 'user_missed', fault: 'user', triggerLegId: 1, stationName: null }),
      }),
    );
    expect(screen.getByRole('region', { name: /^You missed the \d\d:\d\d to York$/ })).toBeInTheDocument();
  });
});
