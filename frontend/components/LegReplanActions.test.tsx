import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LegReplanActions, type LegReplanContext } from './LegReplanActions';
import { trainLeg } from '@/test/journeyFixtures';
import { breakResponse, suggestionSet } from '@/test/replanFixtures';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

const context: LegReplanContext = {
  missable: false,
  openBreakId: null,
  isBreakTrigger: false,
  suggestionsMuted: false,
};

function leg(state: Parameters<typeof trainLeg>[5] = {}) {
  return trainLeg(7, ['YRK', 'York'], ['EDB', 'Edinburgh'], '2026-09-17T08:52:00Z', '2026-09-17T11:19:00Z', {
    scheduleDestinationName: 'Edinburgh',
    ...state,
  });
}

function stubFetch(body: unknown) {
  const fetchMock = vi.fn(async () => new Response(JSON.stringify(body), { status: 200 }));
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('LegReplanActions', () => {
  it('renders nothing for an ordinary leg', () => {
    renderWithMantine(<LegReplanActions journeyId={42} leg={leg()} context={context} />);
    expect(screen.queryAllByRole('button')).toHaveLength(0);
  });

  it('offers "I missed this train" on the missable leg, and plans from it', async () => {
    const fetchMock = stubFetch(breakResponse({ cause: 'user_missed', fault: 'user' }));
    renderWithMantine(<LegReplanActions journeyId={42} leg={leg()} context={{ ...context, missable: true }} />);
    expect(screen.queryByRole('button', { name: 'Find another way' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'I missed this train' }));
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/legs/7/missed', { method: 'POST' });
  });

  it('never offers "I missed this train" on a cancelled train', () => {
    renderWithMantine(
      <LegReplanActions journeyId={42} leg={leg({ status: 'cancelled' })} context={{ ...context, missable: true }} />,
    );
    expect(screen.queryByRole('button', { name: 'I missed this train' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Find another way' })).toBeInTheDocument();
  });

  it('finds other trains for a cancelled leg with no break open', async () => {
    const fetchMock = stubFetch(suggestionSet());
    renderWithMantine(<LegReplanActions journeyId={42} leg={leg({ status: 'cancelled' })} context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'Find another way' }));
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/legs/7/alternatives?cause=cancelled', { method: 'GET' });
  });

  it("opens the open break's suggestions on its trigger leg", async () => {
    const fetchMock = stubFetch(breakResponse());
    renderWithMantine(
      <LegReplanActions journeyId={42} leg={leg()} context={{ ...context, openBreakId: 5, isBreakTrigger: true }} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Find another way' }));
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/breaks/current', { method: 'GET' });
  });
});
