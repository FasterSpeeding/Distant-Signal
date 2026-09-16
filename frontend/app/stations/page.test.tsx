import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import StationSearchPage, { metadata } from './page';

// The page mounts StationSearchForm, a client component that calls
// useRouter() at the top of its body -- same stub app/trains/page.test.tsx
// uses for TrainSearchForm, and the same one StationSearchForm's own test
// file installs.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
}));

// StationSearchForm's useSuggestions hook fires a real fetch on mount for
// any non-empty query; it starts empty here, but an inert 200 keeps this
// file independent of network behaviour either way.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('StationSearchPage', () => {
  it('renders the heading and the search form', () => {
    renderWithMantine(StationSearchPage());
    expect(screen.getByRole('heading', { name: 'Station Disruption Lookup' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Look up' })).toBeInTheDocument();
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    // The <h1>, not the shorter nav label ("Station Lookup") -- so the tab
    // title and the heading a visitor lands on agree.
    expect(metadata.title).toBe('Station Disruption Lookup — Distant Signal');
  });

  it('describes station lookup rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Look up any UK station by name or CRS code for the disruptions affecting lines through it, its scheduled departures, per-operator delay and cancellation stats and its accessibility & facilities.',
    );
  });

  it("doesn't call the timetable rows live, which the page they describe explicitly disclaims", () => {
    // StationTimetable heads its section "Scheduled departures" and says
    // outright that its rows are "from the scheduled timetable, not live
    // running information, and may be up to 30 minutes out of date" --
    // metadata promising "live departures" would contradict the very page
    // it is a preview of.
    expect(metadata.description).toMatch(/scheduled departures/);
    expect(metadata.description).not.toMatch(/live departures/);
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // See the equivalent case in app/incidents/page.test.tsx for why the
    // mirror is asserted against literals rather than against
    // `metadata.title`/`.description`.
    expect(metadata.openGraph).toMatchObject({
      title: 'Station Disruption Lookup — Distant Signal',
      description:
        'Look up any UK station by name or CRS code for the disruptions affecting lines through it, its scheduled departures, per-operator delay and cancellation stats and its accessibility & facilities.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Station Disruption Lookup — Distant Signal',
      description:
        'Look up any UK station by name or CRS code for the disruptions affecting lines through it, its scheduled departures, per-operator delay and cancellation stats and its accessibility & facilities.',
    });
  });
});
