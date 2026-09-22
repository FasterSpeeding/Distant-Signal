import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JourneysNewPage, { metadata } from './page';

// The page mounts JourneyCreationFlow, a client component that renders
// TrackTrainForm as its leg-1 step -- that calls useRouter() at the top of
// its body, same stub `app/track/page.test.tsx` installs for the same
// reason.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/journeys/new',
  useSearchParams: () => new URLSearchParams(''),
}));

// TrackTrainForm's departures picker fires a real fetch as soon as its
// origin field holds a valid CRS, and its useSuggestions hooks fetch for
// any non-empty query -- nothing here is pre-filled, but an inert 200 keeps
// this file independent of network behaviour either way.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('JourneysNewPage', () => {
  it('renders the heading, an account hint, and the leg-1 tracking form', () => {
    renderWithMantine(<JourneysNewPage />);

    expect(screen.getByRole('heading', { name: 'Track a Journey', level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/needs a Distant Signal account/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  it('exports metadata matching its own heading', () => {
    expect(metadata.title).toContain('Track a Journey');
  });
});
