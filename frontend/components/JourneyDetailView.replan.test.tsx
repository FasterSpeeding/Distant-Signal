import { describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyDetailView } from './JourneyDetailView';
import { journeyDetail, trainLeg } from '@/test/journeyFixtures';
import { openBreakSummary } from '@/test/replanFixtures';
import type { JourneyDetail } from '@/lib/types';

/* The journey page's mid-journey changes (spec 2026-10-22 §8.2): which
 * leg offers "I missed this train" and "Find another way", the open
 * break's banner, and "Earlier plans". */

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
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
