import { afterEach, describe, it, expect, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { byVisibleText } from '@/test/routeText';
import type { TripPlanResponse } from '@/lib/types';
import PlanPage, { metadata } from './page';

// `LoginPromptModal` -> `useLoginHref` reads the path and query, so the
// stub carries what a real visitor on `/plan` would have.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/plan',
  useSearchParams: () => new URLSearchParams('origin=EUS'),
}));

// The date picker's range is read on the server; it fails by default (the
// picker falls back to a week ahead), and every other export stays real.
vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTrainSearchDates: vi.fn(async () => {
      throw new Error('not stubbed');
    }),
  };
});

// Same reasoning as `PlanTripFlow.test.tsx`: the station autocompletes'
// debounced lookups must not eat this file's `fetch` mocks.
vi.mock('@/lib/suggestions', () => ({
  searchStations: async () => [],
  searchTocs: async () => [],
  searchPlannerLocations: async () => [],
  getStationNames: async () => new Map<string, string>(),
}));

const plan: TripPlanResponse = {
  results: 'fastest',
  segments: [
    {
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      cappedByMaxChanges: false,
      itineraries: [
        {
          legs: [
            {
              kind: 'train',
              trainUid: 'C11052',
              serviceDate: '2026-10-06',
              originCrs: 'EUS',
              destinationCrs: 'MKC',
              scheduledDeparture: '08:00:00',
              scheduledArrival: '08:50:00',
              arrivalDayOffset: 0,
            },
          ],
          changeCount: 0,
          totalDurationMinutes: 50,
        },
      ],
    },
  ],
};

async function renderPage(searchParams: Record<string, string | string[]> = {}) {
  renderWithMantine(await PlanPage({ searchParams: Promise.resolve(searchParams) }));
}

describe('PlanPage (/plan)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens straight on the planner, with no "I know my route" toggle', async () => {
    await renderPage();
    expect(screen.getByRole('heading', { name: 'Plan a journey', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'From' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'To' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Find routes' })).toBeInTheDocument();
    expect(screen.queryByText('I know my route')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Track this train' })).not.toBeInTheDocument();
  });

  it('says planning needs no account, and links to tracking for a known train', async () => {
    await renderPage();
    expect(screen.getByText(/You don.t need an account to plan/)).toBeInTheDocument();
    expect(screen.queryByText(/needs a Distant Signal account/)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track it directly' })).toHaveAttribute('href', '/journeys/new');
  });

  it('pre-fills From from ?origin=, the same parameter /track takes', async () => {
    await renderPage({ origin: 'eus' });
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('EUS');
  });

  it('ignores an ?origin= that is not a station code', async () => {
    await renderPage({ origin: '<script>' });
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('');
  });

  it('restores a shared search, advanced options included, from the query string', async () => {
    await renderPage({
      origin: 'EUS',
      destination: 'pre',
      results: 'options',
      via: 'STA,CRE',
      avoidChange: 'WVH',
      maxChanges: '1',
    });
    expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('PRE');
    expect(screen.getByRole('radio', { name: 'Compare options' })).toBeChecked();
    const advanced = screen.getByRole('button', { name: 'Advanced options (3 set)' });
    expect(advanced).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText("Pass through STA, CRE · Don't change at WVH · Max 1 change")).toBeInTheDocument();
  });

  it('takes the first of a repeated ?origin=', async () => {
    await renderPage({ origin: ['MKC', 'EUS'] });
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('MKC');
  });

  it('lets a signed-out visitor plan and see results, asking them to log in only on "Track this journey"', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(plan) })
      .mockResolvedValueOnce(new Response('no session', { status: 401 }));
    vi.stubGlobal('fetch', fetchMock);

    await renderPage({ origin: 'EUS' });
    fireEvent.change(screen.getByRole('combobox', { name: 'To' }), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByRole('button', { name: 'Find routes' }));

    // The plan request went out with no login step in front of it, and
    // its results render for a visitor with no session.
    await screen.findByText(byVisibleText('08:00 EUS → MKC 08:50'));
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0]![0])).toMatch(/^\/api\/Trips\/plan\?/);
    expect(screen.queryByText('Log in required')).not.toBeInTheDocument();

    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey' }));

    // Saving is what needs the session: the 401 opens the login prompt,
    // whose link brings the visitor back to this page.
    const dialog = await screen.findByRole('dialog', { name: 'Log in required' });
    expect(within(dialog).getByText('Log in to track this journey.')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('exports metadata matching its heading, without any per-visitor value', () => {
    expect(metadata.title).toBe('Plan a journey');
    expect(String(metadata.description)).toContain('track the one you pick');
  });
});
