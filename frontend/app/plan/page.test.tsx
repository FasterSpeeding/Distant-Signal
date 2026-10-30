import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { byVisibleText } from '@/test/routeText';
import { KINGS_CROSS } from '@/test/tflStopFixtures';
import { ApiNotFoundError, getTflStop, getTrainSearchDates } from '@/lib/api';
import type { TripPlanResponse } from '@/lib/types';
import PlanPage, { metadata } from './page';

// `LoginPromptModal` -> `useLoginHref` reads the path and query, so the
// stub carries what a real visitor on `/plan` would have. `TrackTrainForm`
// (under "I know my route") calls `useRouter()` at the top of its body.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/plan',
  useSearchParams: () => new URLSearchParams('origin=EUS'),
}));

// The date picker's range is read on the server; it fails by default (the
// picker falls back to a week ahead). `getTflStop` is set per test; every
// other export stays real.
vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTrainSearchDates: vi.fn(async () => {
      throw new Error('not stubbed');
    }),
    getTflStop: vi.fn(),
  };
});

// Same reasoning as `PlanTripFlow.test.tsx`: the station autocompletes'
// debounced lookups must not eat this file's `fetch` mocks.
// `getStationNames` knows the stations a test lists here, so the From and
// To fields can show a restored station by name (`withNames`).
const KNOWN_NAMES = new Map<string, string>();
function withNames(names: Record<string, string>) {
  for (const [code, name] of Object.entries(names)) KNOWN_NAMES.set(code, name);
}
vi.mock('@/lib/suggestions', () => ({
  searchStations: async () => [],
  searchTocs: async () => [],
  searchPlannerLocations: async () => [],
  getStationNames: async (codes: string[]) =>
    new Map(codes.flatMap((code) => (KNOWN_NAMES.has(code) ? [[code, KNOWN_NAMES.get(code)!] as const] : []))),
}));
// From and To also look up tram stations (`GET /api/transit/networks`).
vi.mock('@/lib/plannerEnds', () => ({ searchPlannerEnds: async () => [] }));

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

// By aria-label, not role: the dropdown is still mid-transition (not yet
// "visible" to a role query) when its day buttons are rendered.
function dayButton(label: string): Promise<HTMLElement> {
  return waitFor(() => {
    const day = document.querySelector<HTMLElement>(`[aria-label="${label}"]`);
    if (!day) throw new Error(`no day ${label}`);
    return day;
  });
}

describe('PlanPage (/plan, Plan a journey)', () => {
  beforeEach(() => {
    // An inert 200 for anything a form fetches on its own.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('[]', { status: 200 })),
    );
    window.history.replaceState(null, '', '/plan');
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    KNOWN_NAMES.clear();
  });

  it('opens on the planner, with every other way to start beside it', async () => {
    await renderPage();
    expect(screen.getByRole('heading', { name: 'Plan a journey', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'I know my route' })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).not.toBeChecked();
    expect(screen.getByRole('combobox', { name: 'From' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'To' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Find routes' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Track this train' })).not.toBeInTheDocument();
  });

  it('links to the ways in that have a page of their own: a train search, a ticket and templates', async () => {
    await renderPage();
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
    expect(screen.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute('href', '/track/mine/add-ticket');
    expect(screen.getByRole('link', { name: 'Journey templates' })).toHaveAttribute('href', '/journeys/templates');
  });

  it('says planning needs no account, and that saving asks you to log in', async () => {
    await renderPage();
    expect(screen.getByText(/You don.t need an account to plan/)).toBeInTheDocument();
    expect(screen.getByText(/asked to log in when you save it/)).toBeInTheDocument();
    // The two pages' cross-links are gone: everything is on this one.
    expect(screen.queryByRole('link', { name: 'Track it directly' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Plan a journey' })).not.toBeInTheDocument();
    expect(screen.queryByText(/New journey/)).not.toBeInTheDocument();
  });

  it('writes the chosen option into the address bar, and drops it for the planner', async () => {
    await renderPage();
    fireEvent.click(screen.getByText('I know my route'));
    expect(window.location.search).toBe('?mode=known');
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
    fireEvent.click(screen.getByText('Plan a route for me'));
    expect(window.location.search).toBe('');
  });

  describe('?mode= (also what /journeys/new links carry, redirected here)', () => {
    it('opens on "I know my route" for ?mode=known', async () => {
      await renderPage({ mode: 'known' });
      expect(screen.getByRole('radio', { name: 'I know my route' })).toBeChecked();
      expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
    });

    it('opens on the time window search for ?mode=window, as /track does', async () => {
      await renderPage({ mode: 'window' });
      expect(screen.getByRole('radio', { name: 'I know my route' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
      expect(screen.getByRole('button', { name: 'Search for a train' })).toBeInTheDocument();
    });

    it('opens on "I know the train" for ?mode=pick', async () => {
      await renderPage({ mode: 'pick' });
      expect(screen.getByRole('radio', { name: 'I know the train' })).toBeChecked();
    });

    it('opens on "Tube, bus or walk" for ?mode=untracked', async () => {
      await renderPage({ mode: 'untracked' });
      expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
      expect(screen.getByRole('button', { name: 'Start journey' })).toBeInTheDocument();
    });

    it('opens on the planner for ?mode=plan, or a mode it does not know', async () => {
      await renderPage({ mode: 'plan' });
      expect(screen.getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
      document.body.innerHTML = '';
      await renderPage({ mode: 'teleport' });
      expect(screen.getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
    });
  });

  it('pre-fills From from ?origin=, the same parameter /track takes', async () => {
    withNames({ EUS: 'London Euston' });
    await renderPage({ origin: 'eus' });
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('London Euston (EUS)'));
  });

  it('ignores an ?origin= that is not a station code', async () => {
    await renderPage({ origin: '<script>' });
    expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('');
  });

  it('restores a shared search, advanced options included, from the query string', async () => {
    withNames({ PRE: 'Preston' });
    await renderPage({
      origin: 'EUS',
      destination: 'pre',
      results: 'options',
      via: 'STA,CRE',
      avoidChange: 'WVH',
      maxChanges: '1',
    });
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'To' })).toHaveValue('Preston (PRE)'));
    expect(screen.getByRole('radio', { name: 'Compare options' })).toBeChecked();
    const advanced = screen.getByRole('button', { name: 'Advanced options (3 set)' });
    expect(advanced).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText("Pass through STA, CRE · Don't change at WVH · Max 1 change")).toBeInTheDocument();
  });

  it('takes the first of a repeated ?origin=', async () => {
    withNames({ MKC: 'Milton Keynes Central' });
    await renderPage({ origin: ['MKC', 'EUS'] });
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'From' })).toHaveValue('Milton Keynes Central (MKC)'),
    );
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
    await screen.findByText(byVisibleText('08:00 EUS to MKC 08:50'));
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0]![0])).toMatch(/^\/api\/Trips\/plan\?/);
    expect(screen.queryByText('Log in required')).not.toBeInTheDocument();
    // The search is in the address bar, to share or bookmark.
    expect(window.location.search).toContain('origin=EUS');
    expect(window.location.search).toContain('destination=MKC');

    const radios = screen.getAllByRole('radio');
    fireEvent.click(radios[radios.length - 1]!);
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey' }));

    // Saving is what needs the session: the 401 opens the login prompt,
    // whose link brings the visitor back to this page.
    const dialog = await screen.findByRole('dialog', { name: 'Log in required' });
    expect(within(dialog).getByText('Log in to track this journey.')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  // The planner's picker ends where the timetable search does, or a week
  // ahead when that can't be read.
  describe("the planner's date picker", () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      vi.setSystemTime(new Date('2026-09-05T12:00:00.000Z'));
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it("ends at the search range's last day", async () => {
      vi.mocked(getTrainSearchDates).mockResolvedValueOnce({
        from: '2026-08-29',
        to: '2026-09-20',
        publishedFrom: '2026-09-04',
        publishedTo: '2026-09-20',
      });
      await renderPage();
      fireEvent.focus(screen.getByLabelText('Date'));
      expect(await dayButton('20 September 2026')).not.toBeDisabled();
      expect(await dayButton('21 September 2026')).toBeDisabled();
    });

    it('falls back to a week ahead when the range could not be read', async () => {
      await renderPage();
      fireEvent.focus(screen.getByLabelText('Date'));
      expect(await dayButton('12 September 2026')).not.toBeDisabled();
      expect(await dayButton('13 September 2026')).toBeDisabled();
    });
  });

  // A TfL stop page's "Add as a journey leg".
  describe('from a TfL stop', () => {
    it('opens the Tube, bus or walk form starting at that stop', async () => {
      vi.mocked(getTflStop).mockResolvedValue(KINGS_CROSS);
      await renderPage({ fromTflStop: '940GZZLUKSX' });
      expect(getTflStop).toHaveBeenCalledWith('940GZZLUKSX');
      expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
      expect(screen.getByLabelText('How', { selector: 'input' })).toHaveValue('Tube');
      expect(screen.getByRole('combobox', { name: 'From (optional)' })).toHaveValue("King's Cross St. Pancras");
    });

    it('opens on that stop even when ?mode= names another option', async () => {
      vi.mocked(getTflStop).mockResolvedValue(KINGS_CROSS);
      await renderPage({ fromTflStop: '940GZZLUKSX', mode: 'known' });
      expect(screen.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
    });

    it('opens as usual for an unknown or malformed stop', async () => {
      vi.mocked(getTflStop).mockReset().mockRejectedValue(new ApiNotFoundError('404'));
      await renderPage({ fromTflStop: '940GZZLUNOPE' });
      expect(screen.getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();

      vi.mocked(getTflStop).mockClear();
      document.body.innerHTML = '';
      await renderPage({ fromTflStop: 'not a stop' });
      expect(getTflStop).not.toHaveBeenCalled();
    });
  });

  it('exports metadata matching its heading, canonical at /plan, without any per-visitor value', () => {
    expect(metadata.title).toBe('Plan a journey');
    expect(String(metadata.description)).toMatch(/track it live/);
    expect(metadata.alternates?.canonical).toMatch(/\/plan$/);
  });
});
