import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyBreakBanner } from './JourneyBreakBanner';
import { trainLeg } from '@/test/journeyFixtures';
import { breakResponse, openBreakSummary } from '@/test/replanFixtures';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

const legs = [
  trainLeg(7, ['YRK', 'York'], ['EDB', 'Edinburgh'], '2026-09-17T08:52:00Z', '2026-09-17T11:19:00Z', {
    scheduleDestinationName: 'Edinburgh',
  }),
];

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('JourneyBreakBanner', () => {
  it('says what broke and how many other ways there are', () => {
    renderWithMantine(<JourneyBreakBanner journeyId={42} openBreak={openBreakSummary()} legs={legs} isOwner />);
    const banner = screen.getByRole('region', { name: 'Connection at York missed' });
    expect(banner).toHaveTextContent('3 other ways to Edinburgh. First arrives 11:12.');
    expect(screen.getByRole('button', { name: 'See options' })).toBeInTheDocument();
  });

  it('words a cancelled train from its trigger leg', () => {
    renderWithMantine(
      <JourneyBreakBanner
        journeyId={42}
        openBreak={openBreakSummary({ cause: 'cancelled', suggestionCount: 0, firstArrival: null })}
        legs={legs}
        isOwner
      />,
    );
    expect(screen.getByText('09:52 to Edinburgh cancelled')).toBeInTheDocument();
    expect(screen.getByText('No more trains to Edinburgh today.')).toBeInTheDocument();
  });

  it('says it is still looking while suggestions are not ready', () => {
    renderWithMantine(
      <JourneyBreakBanner
        journeyId={42}
        openBreak={openBreakSummary({ suggestionsReady: false })}
        legs={legs}
        isOwner
      />,
    );
    expect(screen.getByText('Looking for other trains.')).toBeInTheDocument();
  });

  it('is only text for someone who is not the owner', () => {
    renderWithMantine(
      <JourneyBreakBanner journeyId={42} openBreak={openBreakSummary()} legs={legs} isOwner={false} initialOpen />,
    );
    expect(screen.getByText('Connection at York missed')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'See options' })).not.toBeInTheDocument();
    expect(screen.queryByText('Loading trains')).not.toBeInTheDocument();
  });

  it('opens the suggestions on "See options"', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(breakResponse()), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneyBreakBanner journeyId={42} openBreak={openBreakSummary()} legs={legs} isOwner />);
    fireEvent.click(screen.getByRole('button', { name: 'See options' }));
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/breaks/current', { method: 'GET' });
  });
});
