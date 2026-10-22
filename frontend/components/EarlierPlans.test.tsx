import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EarlierPlans } from './EarlierPlans';
import type { Revision, RevisionLeg } from '@/lib/types';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: refreshMock }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

function revisionLeg(overrides: Partial<RevisionLeg> = {}): RevisionLeg {
  return {
    position: 0,
    sourceLegId: 7,
    kind: 'train',
    trainUid: 'L1',
    serviceDate: '2026-09-17',
    atocCode: 'GR',
    originCrs: 'YRK',
    originName: 'York',
    destinationCrs: 'EDB',
    destinationName: 'Edinburgh',
    bookedDeparture: '2026-09-17T08:52:00Z',
    bookedArrival: '2026-09-17T11:19:00Z',
    transitMode: null,
    originPlace: null,
    destinationPlace: null,
    lineName: null,
    plannedDeparture: null,
    plannedArrival: null,
    ticketIds: [],
    fate: 'planned',
    ...overrides,
  };
}

function revision(overrides: Partial<Revision> = {}): Revision {
  return {
    revisionNo: 1,
    createdAt: '2026-09-16T18:00:00Z',
    reason: 'booked',
    cause: null,
    fault: null,
    breakId: null,
    actor: 'user',
    isCompensationBaseline: true,
    canRevert: false,
    legs: [revisionLeg()],
    ...overrides,
  };
}

const revisions: Revision[] = [
  revision({ revisionNo: 3, reason: 'replace', legs: [revisionLeg({ bookedDeparture: '2026-09-17T09:04:00Z' })] }),
  revision({
    revisionNo: 2,
    reason: 'replace',
    canRevert: true,
    legs: [revisionLeg({ bookedDeparture: '2026-09-17T08:58:00Z', fate: 'not_used' })],
  }),
  revision({ revisionNo: 1, legs: [revisionLeg({ fate: 'cancelled' }), revisionLeg({ position: 1, fate: 'missed' })] }),
];

let fetchMock: Mock<typeof fetch>;

beforeEach(() => {
  refreshMock.mockReset();
  fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === '/api/Journeys/42/revisions') return new Response(JSON.stringify({ revisions }), { status: 200 });
    if (url === '/api/Journeys/42/revisions/2/revert' && init?.method === 'POST') {
      return new Response(JSON.stringify({ journeyId: 42, revisionNo: 4, newLegIds: [9], undoRevisionNo: 3 }));
    }
    throw new Error(`unexpected fetch ${url}`);
  });
  vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('EarlierPlans', () => {
  it('renders nothing with only the booked plan', () => {
    renderWithMantine(<EarlierPlans journeyId={42} revisionCount={1} />);
    expect(screen.queryByText(/Earlier plans/)).not.toBeInTheDocument();
  });

  it('loads the earlier plans only when opened, with their fates', async () => {
    renderWithMantine(<EarlierPlans journeyId={42} revisionCount={3} />);
    const toggle = screen.getByRole('button', { name: 'Earlier plans (2)' });
    expect(fetchMock).not.toHaveBeenCalled();
    fireEvent.click(toggle);

    const booked = (await screen.findByText('Booked plan')).closest('[data-revision]') as HTMLElement;
    expect(within(booked).getByText('Booked')).toBeInTheDocument();
    expect(within(booked).getByText('Cancelled')).toBeInTheDocument();
    expect(within(booked).getByText('Missed')).toBeInTheDocument();
    expect(within(booked).queryByRole('button', { name: 'Go back to this plan' })).not.toBeInTheDocument();

    // The current plan (the newest) is the page itself, not an earlier one.
    expect(screen.queryByText('10:04 York to Edinburgh')).not.toBeInTheDocument();
    const changed = screen.getByText('09:58 York to Edinburgh').closest('[data-revision]') as HTMLElement;
    expect(within(changed).getByText('Not used')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('goes back to an earlier plan', async () => {
    renderWithMantine(<EarlierPlans journeyId={42} revisionCount={3} />);
    fireEvent.click(screen.getByRole('button', { name: 'Earlier plans (2)' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Go back to this plan' }));
    await vi.waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/revisions/2/revert', { method: 'POST' });
  });

  it('says when it could not go back', async () => {
    fetchMock.mockImplementation(async (input: RequestInfo | URL) => {
      if (String(input).endsWith('/revisions')) return new Response(JSON.stringify({ revisions }), { status: 200 });
      return new Response('revision_gone', { status: 409 });
    });
    renderWithMantine(<EarlierPlans journeyId={42} revisionCount={3} />);
    fireEvent.click(screen.getByRole('button', { name: 'Earlier plans (2)' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Go back to this plan' }));
    expect(
      await screen.findByText("Couldn't go back to this plan. It changed in the meantime. Reload and try again."),
    ).toBeInTheDocument();
  });

  it('says when it could not load them', async () => {
    fetchMock.mockImplementation(async () => new Response('', { status: 500 }));
    renderWithMantine(<EarlierPlans journeyId={42} revisionCount={2} />);
    fireEvent.click(screen.getByRole('button', { name: 'Earlier plans (1)' }));
    expect(await screen.findByText("Couldn't load earlier plans. Try again.")).toBeInTheDocument();
  });
});
