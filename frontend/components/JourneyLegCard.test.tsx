import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyLegCard } from './JourneyLegCard';
import type { JourneyLegDetail } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/1',
  useSearchParams: () => new URLSearchParams(''),
}));

function baseLeg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'EDB',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'unmatched',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

/** Every owner-visible open-leg card mounts `JourneyLegCandidates`, which
 * fires a real fetch on mount -- awaiting its zero-result state lets that
 * effect settle inside `act()` before a test's own assertions run. */
async function settleCandidates() {
  await screen.findByText(/No scheduled trains match this window\./);
}

describe('JourneyLegCard (open leg)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders resolved station names in the header, not raw CRS codes, when names resolve', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(
      <JourneyLegCard
        journeyId={1}
        isOwner
        leg={baseLeg({ originName: 'London Kings Cross', destinationName: 'Edinburgh' })}
      />,
    );
    await settleCandidates();

    expect(screen.getByText('London Kings Cross (KGX) → Edinburgh (EDB), 22 Sept 2026')).toBeInTheDocument();
    expect(screen.queryByText(/KGX → EDB/)).not.toBeInTheDocument();
  });

  it('falls back to bare CRS codes when no station name resolved, still using the shared formatter', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner leg={baseLeg()} />);
    await settleCandidates();

    expect(screen.getByText('KGX → EDB, 22 Sept 2026')).toBeInTheDocument();
  });

  it('shows the persisted search window back to the user, and an Edit search link for the owner', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(
      <JourneyLegCard
        journeyId={1}
        isOwner
        leg={baseLeg({ departAfter: '08:00:00', arriveBefore: '12:30:00' })}
      />,
    );
    await settleCandidates();

    expect(
      screen.getByText('Looking for trains departing KGX after 08:00, arriving EDB before 12:30.'),
    ).toBeInTheDocument();
    const editLink = screen.getByRole('link', { name: 'Edit search' });
    expect(editLink).toHaveAttribute('href', '/track?mode=window&origin=KGX');
  });

  it('does not offer Edit search to a non-owning group member', () => {
    // A non-owner never renders `JourneyLegCandidates` at all (the API
    // would 404 a pick attempt anyway) -- no fetch to await here.
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner={false} leg={baseLeg({ departAfter: '08:00:00' })} />);

    expect(screen.queryByRole('link', { name: 'Edit search' })).not.toBeInTheDocument();
    // Still shows the criteria, just without the edit affordance.
    expect(screen.getByText(/Looking for trains departing KGX after 08:00\./)).toBeInTheDocument();
  });

  it('renders no window-criteria line when the leg has no persisted bound', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner leg={baseLeg()} />);
    await settleCandidates();

    expect(screen.queryByText(/Looking for trains/)).not.toBeInTheDocument();
  });

  it('no longer shows the old unconditional "Searching…" copy — the candidate list owns its own state text', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    renderWithMantine(<JourneyLegCard journeyId={1} isOwner leg={baseLeg()} />);
    await settleCandidates();

    expect(screen.queryByText('Searching for a train to track — pick one below.')).not.toBeInTheDocument();
  });

  // Review §2.3/I15: the open-leg card is the one that needs the user to
  // do something -- it used to be visually indistinguishable from a plain
  // white box. A left accent border now flags it, on both the owner and
  // non-owner branches (only the OWNER can act, but both are states that
  // need a train picked).
  it('gives the open-leg card a left accent border, unlike a plain card', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"results":[],"nextCursor":null}', { status: 200 })));
    const { container } = renderWithMantine(<JourneyLegCard journeyId={1} isOwner leg={baseLeg()} />);
    await settleCandidates();

    const card = container.querySelector('.mantine-Card-root');
    expect(card).toHaveStyle({ borderLeftWidth: '4px' });
  });

  it('accents the non-owner "waiting for the owner" card the same way', () => {
    const { container } = renderWithMantine(<JourneyLegCard journeyId={1} isOwner={false} leg={baseLeg()} />);

    const card = container.querySelector('.mantine-Card-root');
    expect(card).toHaveStyle({ borderLeftWidth: '4px' });
    expect(screen.getByText('Waiting for the owner to pick a train.')).toBeInTheDocument();
  });
});
