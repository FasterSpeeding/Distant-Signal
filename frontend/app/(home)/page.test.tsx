import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import DashboardPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type {
  JourneyDetail,
  JourneyListItem,
  LineStatusReport,
  OperatorSummary,
  SharedGroupCustomLine,
} from '@/lib/types';
import { MINOR_DELAYS, NETWORK as TRAM_NETWORK } from '@/test/transitFixtures';
import {
  journeyDetail,
  journeyListItem,
  loneTrain,
  stop,
  trainLeg,
  trainState,
  untrackedLeg,
} from '@/test/journeyFixtures';

vi.mock('@/lib/api');
// `withStaleFallback` (lib/liveDataCache.ts) reads the session cookie via
// `next/headers` to scope its cache per visitor, and there is no Next
// request context in a unit test. Same stub shape lib/api.test.ts uses,
// plus the `.get()` the cache needs.
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

// The anonymous branch's login nudge is now LoginControl (Task 1), which calls
// usePathname()/useSearchParams() -- same stub AuthStatus.test.tsx and
// TicketPanel.test.tsx use for the same reason. `useRouter` is stubbed too,
// as of the "Your Operators" section (Task 13): `OperatorStatusCard`
// renders a `PinToggle` per card, which calls `useRouter()` -- same
// workaround `app/operators/page.test.tsx` uses for the same reason (that
// hook throws outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

function report(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'x',
    id: 'bakerloo',
    name: 'Bakerloo',
    modeName: 'tube',
    operators: [],
    lineStatuses: [
      {
        statusSeverity: 10,
        statusSeverityDescription: 'Good Service',
        reason: '',
        sampleAvailability: { state: 'no-coverage' },
      } as never,
    ],
    computedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function operator(overrides: Partial<OperatorSummary> = {}): OperatorSummary {
  return {
    code: 'VT',
    name: 'Avanti West Coast',
    lineIds: ['wcml'],
    worstSeverity: 1,
    reason: 'Operational issues',
    computedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

// `vi.mock('@/lib/api')` automocks every export to a vi.fn() returning
// undefined. The page now calls `.catch()` on several of these promises, so
// each needs at least a resolved default; individual tests override what
// they care about. The stale cache is real module state, so it is reset too.
beforeEach(() => {
  __resetStaleCacheForTests();
  vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
    authenticated: false,
    id: null,
    username: null,
    name: null,
  });
  vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
  vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
  // Every test that predates group sharing says nothing about it -- default
  // the fourth fetch to "nothing shared with you" so each still describes
  // exactly the scenario it was written for. Also load-bearing mechanically:
  // the page calls `.catch()` on this promise, and an automocked `undefined`
  // return has no `.catch`.
  vi.mocked(api.getStationName).mockResolvedValue(null);
  vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
  vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue(null);
  // Every test that predates operator pinning (Task 13) says nothing about
  // it -- default the new sixth fetch to "no operators in the catalogue"
  // so each still describes exactly the scenario it was written for.
  // Load-bearing mechanically too: `allOperators.filter(...)` below is
  // unguarded, and an automocked `undefined` return has no `.filter`.
  vi.mocked(api.getAllOperators).mockResolvedValue([]);
  // Every test that predates the journey sections says nothing about
  // journeys: no journeys, and no detail to read.
  vi.mocked(api.getMyJourneys).mockResolvedValue([]);
  vi.mocked(api.getJourney).mockRejectedValue(new Error('no journey in this test'));
  vi.mocked(api.getTrackedTrainById).mockRejectedValue(new Error('no train in this test'));
});

describe('DashboardPage', () => {
  // Review §3.1.7: the nav bar already says "Distant Signal" -- this
  // page's own <h1> used to repeat it, reading as duplicated branding
  // above the fold on mobile. It keeps the <h1> (for the outline, and for
  // a link-unfurler bot reading past a nav it never sees) but no longer
  // duplicates the brand name in its text.
  it('titles the anonymous h1 something other than the brand name the nav already carries', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Live UK rail status', level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Distant Signal' })).not.toBeInTheDocument();
  });

  // Signed in, the planner is in the nav bar at every width; the anonymous
  // page keeps it first among its quick links, as the one journey feature
  // that needs no account.
  it('offers the planner, which needs no account, to a signed-out visitor', async () => {
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: 'Plan a journey' })).toHaveAttribute('href', '/plan');
  });

  it('anonymous, all lines good: shows the no-disruption message, not a raw empty state', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in to pin your lines, stations and operators' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2F',
    );
  });

  // Notifications moved to /account/preferences (round 2): the home page
  // no longer carries the control, logged in or out.
  it.each([false, true])('has no notifications control (authenticated: %s)', async (authenticated) => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(
      authenticated
        ? { authenticated: true, id: 'u1', username: null, name: 'Ada' }
        : { authenticated: false, id: null, username: null, name: null },
    );
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('button', { name: /enable notifications/i })).toBeNull();
  });

  it('anonymous, a line disrupted: lists it, worst-first', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'central',
        name: 'Central',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: '',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
      report(),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('anonymous: merged TfL counterpart ids are excluded from the widget', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'tfl-elizabeth',
        name: 'Elizabeth line',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: '',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
  });

  it('logged in: renders the existing pinned-lines/pinned-stations behavior, not the anonymous branch', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['central'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your lines' })).toBeInTheDocument();
    // Load-bearing specifically for the PINNED case (Task 7): this user has
    // pinned a line, so "Right now" must stay absent even though the
    // authenticated branch can now render it for a zero-pinned-lines user.
    expect(screen.queryByText(/Right now/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: 'Log in to pin your lines, stations and operators' }),
    ).not.toBeInTheDocument();
  });

  it('shows a pinned tram line with its status in "Your lines", and drops it while its network is dark', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['metrolink:blue'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getTransitNetwork).mockResolvedValue({
      ...TRAM_NETWORK,
      lines: [{ ...TRAM_NETWORK.lines[0]!, status: MINOR_DELAYS }],
    });
    renderWithMantine(await DashboardPage());
    const card = screen.getByRole('link', { name: /Blue Line/ });
    expect(card).toHaveAttribute('href', '/transit/metrolink/lines/blue');
    expect(card.textContent).toContain('Manchester Metrolink');
    expect(card.textContent).toContain('Minor delays');
    expect(screen.queryByText(/You haven't pinned any lines yet/)).not.toBeInTheDocument();
    cleanup();
    vi.mocked(api.getTransitNetwork).mockRejectedValue(new Error('404'));
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('link', { name: /Blue Line/ })).not.toBeInTheDocument();
    expect(screen.getByText(/You haven't pinned any lines yet/)).toBeInTheDocument();
  });

  it('shows the live "Right now" module to a logged-in user with no pinned lines', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'central',
        name: 'Central',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: '',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now' })).toBeInTheDocument();
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('still shows it when they have pinned stations but no pinned lines', async () => {
    // Gated on pinned LINES only: a user with pinned stations but no
    // pinned lines still has a line-shaped hole on the dashboard.
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['WAT'], pinnedOperators: [] });
    vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now' })).toBeInTheDocument();
  });

  it('hides it once they pin a line', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['central'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Right now' })).not.toBeInTheDocument();
  });

  it('renders the module at heading level 2 in the authenticated branch, no skip', async () => {
    // All three pinned sections are empty here, so review §3.1.4 puts
    // "Right now" first: h1 "Home" (visually hidden) -> h2 "Right now" ->
    // h2 "Your lines" -> h2 "Your stations" -> ... . Its own level-2
    // heading never skips to h3 regardless of where in the page it renders.
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeInTheDocument();
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
    expect(screen.getByRole('heading', { name: 'Your lines', level: 2 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
  });

  it('draws every section heading as an h2 at the app-wide section size, never the unsized 26px h2', async () => {
    // docs/style-guide.md "Heading rules": `SectionTitle` (h2 at h3 size).
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    const sections = screen.getAllByRole('heading', { level: 2 });
    expect(sections.map((h) => h.textContent)).toEqual(
      expect.arrayContaining(['Right now', 'Your stations', 'Your operators']),
    );
    for (const heading of sections) {
      expect(heading.style.getPropertyValue('--title-fz')).toContain('--mantine-h3-font-size');
    }
  });

  // Review §3.1.4: the authenticated dashboard used to double a "Browse
  // all lines"/"Look up a station" link beside its section heading with an
  // identical link ~40px below it, inside the empty-state sentence.
  describe('empty-state link deduplication (review §3.1.4)', () => {
    it('keeps only the inline "Browse all lines" link when Your Lines is empty, not a second one beside the heading', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: [],
        pinnedStations: ['WAT'],
        pinnedOperators: [],
      });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your lines', level: 2 });
      // Scope to everything from the heading's own row up to (but not
      // including) the next section, so this can't accidentally pass by
      // matching the "Your Stations" section's own link instead.
      const section = heading.closest('div')?.parentElement as HTMLElement;
      expect(within(section).getAllByRole('link', { name: 'Browse all lines' })).toHaveLength(1);
    });

    it('keeps only the inline "Look up a station" link when Your Stations is empty, not a second one beside the heading', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: ['central'],
        pinnedStations: [],
        pinnedOperators: [],
      });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your stations', level: 2 });
      const section = heading.closest('div')?.parentElement as HTMLElement;
      expect(within(section).getAllByRole('link', { name: 'Look up a station' })).toHaveLength(1);
    });

    it('still shows the heading-level "Browse all lines" link once Your Lines has pinned rows', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: ['central'],
        pinnedStations: [],
        pinnedOperators: [],
      });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your lines', level: 2 });
      expect(
        within(heading.parentElement as HTMLElement).getByRole('link', { name: 'Browse all lines' }),
      ).toHaveAttribute('href', '/lines');
    });

    it('still shows the heading-level "Look up a station" link once Your Stations has pinned rows', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: [],
        pinnedStations: ['WAT'],
        pinnedOperators: [],
      });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your stations', level: 2 });
      expect(
        within(heading.parentElement as HTMLElement).getByRole('link', { name: 'Look up a station' }),
      ).toHaveAttribute('href', '/stations');
    });
  });

  // Review §3.1.4: "order 'Right now' first when both pinned sections are
  // empty" -- generalized by Task 13 to all THREE pinned sections, now that
  // "Your Operators" exists (`allPinnedSectionsEmpty`, renamed from
  // `bothPinnedSectionsEmpty`). Otherwise the module's usual position
  // (after Your Operators, only when Lines specifically is empty) is
  // unchanged.
  describe('"Right now" ordering when all pinned sections are empty (review §3.1.4)', () => {
    function headingNames() {
      return screen.getAllByRole('heading').map((h) => h.textContent);
    }

    it('puts "Right now" before "Your Lines" when all three sections are empty', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
      renderWithMantine(await DashboardPage());
      const names = headingNames();
      expect(names.indexOf('Right now')).toBeLessThan(names.indexOf('Your lines'));
      // Rendered exactly once, not doubled up at both its old and new spot.
      expect(names.filter((n) => n === 'Right now')).toHaveLength(1);
    });

    it('keeps "Right now" after "Your Stations" (its ordinary spot) when only Lines is empty', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: [],
        pinnedStations: ['WAT'],
        pinnedOperators: [],
      });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
      renderWithMantine(await DashboardPage());
      const names = headingNames();
      expect(names.indexOf('Your stations')).toBeLessThan(names.indexOf('Right now'));
      expect(names.filter((n) => n === 'Right now')).toHaveLength(1);
    });

    // Direct regression check for the `bothPinnedSectionsEmpty` ->
    // `allPinnedSectionsEmpty` rename (Task 13): Lines and Stations are
    // both empty here -- the exact input the OLD two-way flag would have
    // called "both empty" and put "Right now" at the very top for -- but
    // Operators has a pin, so the new three-way `&&` must keep it in its
    // ordinary spot instead. A regressed rename (one that dropped the
    // Operators leg, or used `||`) would put "Right now" first here.
    it('keeps "Right now" in its ordinary spot, not at the top, when only Operators has something pinned', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({
        pinnedLines: [],
        pinnedStations: [],
        pinnedOperators: ['VT'],
      });
      vi.mocked(api.getAllOperators).mockResolvedValue([operator({ code: 'VT' })]);
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
      renderWithMantine(await DashboardPage());
      const names = headingNames();
      expect(names.indexOf('Your operators')).toBeLessThan(names.indexOf('Right now'));
      expect(names.filter((n) => n === 'Right now')).toHaveLength(1);
    });
  });

  it('anonymous branch still renders "Right now" identically after the RightNowModule extraction', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'central',
        name: 'Central',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: '',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  // The "Right now" list is capped at 5 cards while its heading states the
  // true total, so anything past the fifth affected line used to be counted
  // and then silently dropped. These cover the overflow line that now says
  // how many are missing and links out to the full list.
  describe('"Right now" overflow beyond the 5 rendered cards', () => {
    // Zero-padded names so the module's alphabetical tiebreak (every line
    // here shares one severity) is the numeric order the assertions read in.
    function disruptedReports(n: number): LineStatusReport[] {
      return Array.from({ length: n }, (_, i) => {
        const label = String(i + 1).padStart(2, '0');
        return report({
          id: `line-${label}`,
          name: `Line ${label}`,
          lineStatuses: [
            {
              statusSeverity: 6,
              statusSeverityDescription: 'Severe Delays',
              reason: '',
              sampleAvailability: { state: 'no-coverage' },
            } as never,
          ],
        });
      });
    }

    /** The overflow affordance's own link, not the identically-labelled
     * "Browse all lines" this page already renders beside its intro/"Your
     * Lines" heading -- found by scoping to the row the overflow sentence
     * sits in, so the assertion can't accidentally pass on the other one. */
    function overflowLink(sentence: RegExp) {
      const note = screen.getByText(sentence);
      return within(note.parentElement as HTMLElement).getByRole('link', { name: 'Browse all lines' });
    }

    it('counts the lines it is not showing and links to the full list', async () => {
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(12));
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/12 lines not at Good Service right now/)).toBeInTheDocument();
      // Five cards, then the overflow line accounting for the other seven.
      expect(screen.getByRole('link', { name: /Line 05/ })).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /Line 06/ })).not.toBeInTheDocument();
      expect(screen.getByText(/Showing the first 5\. 7 more lines are not at Good Service\./)).toBeInTheDocument();
      expect(overflowLink(/7 more lines are not at Good Service/)).toHaveAttribute('href', '/lines');
    });

    it('says "line is", not "lines are", when exactly one is hidden', async () => {
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(6));
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/Showing the first 5\. 1 more line is not at Good Service\./)).toBeInTheDocument();
    });

    it('hides the least severe lines, not an arbitrary five', async () => {
      // Guards the "first" in the copy actually meaning worst-first: four
      // severe lines plus two mild ones, and it must be the mild pair that
      // ends up behind the overflow line. Named so alphabetical order alone
      // would put the mild ones FIRST, so this fails if the severity sort is
      // dropped or reversed.
      const mild = (id: string, name: string) =>
        report({
          id,
          name,
          lineStatuses: [
            {
              statusSeverity: 9,
              statusSeverityDescription: 'Minor Delays',
              reason: '',
              sampleAvailability: { state: 'no-coverage' },
            } as never,
          ],
        });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([
        mild('aardvark', 'Aardvark'),
        mild('abacus', 'Abacus'),
        ...disruptedReports(4),
      ]);
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/6 lines not at Good Service right now/)).toBeInTheDocument();
      // Four severe lines fill four of the five slots; the fifth goes to the
      // alphabetically-first mild line, and only "Abacus" is hidden.
      expect(screen.getByRole('link', { name: /Line 04/ })).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /Aardvark/ })).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /Abacus/ })).not.toBeInTheDocument();
      expect(screen.getByText(/Showing the first 5\. 1 more line is not at Good Service\./)).toBeInTheDocument();
    });

    it('shows no overflow line when the list is exactly full', async () => {
      // The boundary the count is most likely to get wrong: 5 affected, 5
      // rendered, nothing hidden.
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(5));
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/5 lines not at Good Service right now/)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /Line 05/ })).toBeInTheDocument();
      expect(screen.queryByText(/Showing the first/)).not.toBeInTheDocument();
      // The anonymous branch's own "Browse all lines" is still there; what
      // must be absent is a second one, from the overflow row.
      expect(screen.getAllByRole('link', { name: 'Browse all lines' })).toHaveLength(1);
    });

    it('shows no overflow line for a short list', async () => {
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(2));
      renderWithMantine(await DashboardPage());
      expect(screen.queryByText(/Showing the first/)).not.toBeInTheDocument();
    });

    it('counts only lines the module itself lists -- good-service and merged TfL rows inflate neither half', async () => {
      // The hidden figure is `count - rendered`, and both halves must come
      // from the same filtered set: a Good Service line and a merged TfL id
      // are excluded from the total as well as from the cards, so 6 affected
      // among 8 reports still hides exactly 1.
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([
        ...disruptedReports(6),
        report({ id: 'bakerloo', name: 'Bakerloo' }),
        report({
          id: 'tfl-elizabeth',
          name: 'Elizabeth line',
          lineStatuses: [
            {
              statusSeverity: 6,
              statusSeverityDescription: 'Severe Delays',
              reason: '',
              sampleAvailability: { state: 'no-coverage' },
            } as never,
          ],
        }),
      ]);
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/6 lines not at Good Service right now/)).toBeInTheDocument();
      expect(screen.getByText(/Showing the first 5\. 1 more line is not at Good Service\./)).toBeInTheDocument();
    });

    it('renders the overflow line in the authenticated zero-pinned-lines branch too', async () => {
      vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
        authenticated: true,
        id: 'u1',
        username: 'a',
        name: 'A',
      });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(7));
      renderWithMantine(await DashboardPage());
      expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
      expect(screen.getByText(/Showing the first 5\. 2 more lines are not at Good Service\./)).toBeInTheDocument();
      // "Browse all lines" beside the "Your Lines" heading points at /lines
      // too, so this scopes to the overflow row's own copy of it.
      expect(overflowLink(/2 more lines are not at Good Service/)).toHaveAttribute('href', '/lines');
    });
  });

  // `getSession()` itself never resolves to this shape on a real auth
  // glitch any more -- a network error, timeout or 5xx now surfaces as a
  // rejection that `getSessionOrLoggedOut()` (`lib/api.ts`) catches, logs,
  // and turns into exactly this logged-out `SessionInfo`, rather than the
  // page seeing a raw rejection at all. That catching-and-logging behaviour
  // is what's actually under test at the `lib/api.test.ts` level now (see
  // "getSessionOrLoggedOut" there); this page only needs to keep trusting
  // whatever `getSessionOrLoggedOut()` resolves to, which this pins.
  it('degrades to the anonymous branch when getSessionOrLoggedOut() resolves logged-out (its own failure-fallback shape), not a crash', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: 'Log in to pin your lines, stations and operators' })).toBeInTheDocument();
  });
});

describe('DashboardPage -- outage behaviour', () => {
  // Outage behaviour (design spec Decision 5 / plan Task 5).
  it('keeps rendering the last-known line status when the status fetch fails', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    cleanup();

    vi.mocked(api.getLineStatusForMode).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await DashboardPage());

    // Still the real page, not a throw up to app/error.tsx.
    expect(screen.getByRole('heading', { name: 'Live UK rail status', level: 1 })).toBeInTheDocument();
  });

  it('renders with nothing pinned rather than throwing when getPreferences fails', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your lines', level: 2 })).toBeInTheDocument();
    expect(screen.getByText(/haven't pinned any lines yet/)).toBeInTheDocument();
  });

  it('renders rather than throwing when getMyTrackedTrains fails', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    vi.mocked(api.getMyTrackedTrains).mockRejectedValue(new Error('500'));

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your lines', level: 2 })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Upcoming journeys' })).toBeInTheDocument();
  });

  it("keeps the dashboard up when a pinned station's disruption fetch fails", async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['KGX'], pinnedOperators: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your stations', level: 2 })).toBeInTheDocument();
    expect(screen.getByText('KGX')).toBeInTheDocument();
  });
});

describe('DashboardPage -- pinned station line-coverage distinction', () => {
  // The regression this task exists for, on the dashboard's own pinned-
  // station card: before this fix, `.catch(() => [])` silently swallowed
  // the backend's new "no line coverage" 404 the exact same way it already
  // swallowed a real connectivity failure, so a pinned but uncovered
  // station rendered `worstSeverityAcrossReports([])` -- a Good Service
  // badge, indistinguishable from a genuinely fine, fully-covered pinned
  // station. See crates/api/src/routes/line_status.rs's
  // get_stop_point_disruption and app/stations/[crs]/page.tsx's
  // fetchStationDisruptions for the same distinction made on the station
  // detail page.
  beforeEach(() => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: true,
      id: 'u1',
      username: 'a',
      name: 'A',
    });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['RAY'], pinnedOperators: [] });
    vi.mocked(api.getStationName).mockResolvedValue('Raynes Park');
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  });

  it('shows a "Not tracked" badge, not "Good Service", for a pinned station the backend 404s as uncovered', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(
      new api.ApiNotFoundError('no line coverage for stop point: RAY'),
    );

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Not tracked')).toBeInTheDocument();
    expect(screen.getByText('Not covered by our line-status tracking yet.')).toBeInTheDocument();
    expect(screen.queryByText('Good Service')).not.toBeInTheDocument();
  });

  it('still shows the real "Good Service" badge for a genuinely covered, currently-fine pinned station', async () => {
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Good service')).toBeInTheDocument();
    expect(screen.queryByText('Not tracked')).not.toBeInTheDocument();
  });

  it('prefers fullCoverageStats over sampleStats in the pinned-station card subtitle (Decision 3)', async () => {
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([
      {
        $type: 'x',
        id: 'swr-alton',
        name: 'Alton',
        modeName: 'national-rail',
        operators: [],
        computedAt: '2026-09-03T00:00:00Z',
        lineStatuses: [
          {
            statusSeverity: 10,
            statusSeverityDescription: 'Good Service',
            reason: '',
            dataQuality: 'trust-inferred',
            validityPeriods: [],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'available' },
            sampleStats: { total: 20, delayed: 5, cancelled: 1, skipped: 0, avgDelayMinutes: 4.0 },
            fullCoverageStats: { total: 500, delayed: 10, cancelled: 5, skipped: 0, avgDelayMinutes: 2.0 },
          },
        ],
      },
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText(/Avg delay 2\.0 min/)).toBeInTheDocument();
    expect(screen.queryByText(/Avg delay 4\.0 min/)).not.toBeInTheDocument();
  });
});

describe('DashboardPage -- Lines shared with you section', () => {
  function sharedLine(overrides: Partial<SharedGroupCustomLine> = {}): SharedGroupCustomLine {
    return {
      groupId: 'grp-1',
      groupName: 'Family',
      lineId: 'custom-my-commute',
      lineName: 'My Commute',
      grantedBy: 'user-2',
      grantedByName: 'Sam',
      grantedByTag: null,
      ...overrides,
    };
  }

  const loggedIn = { authenticated: true as const, id: 'u1', username: 'a', name: 'A' };

  it('is absent entirely when nothing has been shared with the caller', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
  });

  it('is absent for an anonymous visitor even if the endpoint somehow returned rows', async () => {
    // The anonymous branch returns before this section is ever rendered --
    // a shared custom line is only ever visible to a signed-in member.
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
    expect(screen.queryByText('My Commute')).not.toBeInTheDocument();
  });

  it('renders a shared line with its group tag, its sharer, and a link to the line', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Lines shared with you' })).toBeInTheDocument();
    expect(screen.getByText('My Commute')).toBeInTheDocument();
    expect(screen.getByText('From Family')).toBeInTheDocument();
    expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /My Commute/ })).toHaveAttribute('href', '/lines/custom-my-commute');
  });

  it("tags a line shared into two of the caller's groups with both, on one row", async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([
      sharedLine({ groupId: 'grp-1', groupName: 'Family' }),
      sharedLine({ groupId: 'grp-2', groupName: 'Commute Buddies' }),
    ]);
    renderWithMantine(await DashboardPage());

    expect(screen.getAllByText('My Commute')).toHaveLength(1);
    expect(screen.getByText('From Family')).toBeInTheDocument();
    expect(screen.getByText('From Commute Buddies')).toBeInTheDocument();
  });

  it('falls back to "a member" rather than a raw user id when the sharer has no display name', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine({ grantedByName: null })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('Shared by a member')).toBeInTheDocument();
    expect(screen.queryByText(/user-2/)).not.toBeInTheDocument();
  });

  it('renders no owner controls of any kind on a shared line -- it is view-only', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.queryByRole('button', { name: /edit/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /delete/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /stop sharing/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /pin/i })).not.toBeInTheDocument();
  });

  it('does not render a shared line twice when the caller has also pinned it', async () => {
    // A granted member can pin a shared line like any other, and "Your
    // Lines" renders it from allReports -- so it must not also appear
    // under "Lines shared with you".
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['custom-my-commute'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'custom-my-commute', name: 'My Commute' })]);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.getAllByText('My Commute')).toHaveLength(1);
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
  });

  it('still shows a pinned shared line that has no status row yet, rather than dropping it', async () => {
    // The dedupe above keys off what "Your Lines" will ACTUALLY render
    // (`pinnedLineReports`), not off `preferences.pinnedLines`. The two
    // differ for a line the aggregator hasn't computed a status for yet:
    // it's pinned, but absent from `allReports`, so "Your Lines" skips it.
    // Excluding it here too would drop it from the page entirely.
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['custom-my-commute'],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Lines shared with you' })).toBeInTheDocument();
    expect(screen.getAllByText('My Commute')).toHaveLength(1);
  });

  it('survives the shared-lines fetch failing, rather than blanking the dashboard', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockRejectedValue(new Error('boom'));
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your lines' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
  });

  // Task 1.5 (WCAG 2.5.3): `SharedCustomLineSummaryRow` pairs its title
  // with a status badge in a `Group wrap="nowrap"` (`StatusRow`) -- the
  // badge must not be crushable, even with a very long line name.
  it('gives the status badge a shrink guard, even with a very long line name', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'custom-my-commute',
        name: 'My Commute',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: 'signalling',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
    ]);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([
      sharedLine({ lineName: 'An Implausibly Long Custom Line Name Chosen To Threaten This Row’s Layout' }),
    ]);
    const { container } = renderWithMantine(await DashboardPage());

    const row = screen
      .getByText('An Implausibly Long Custom Line Name Chosen To Threaten This Row’s Layout')
      .closest('.mantine-Card-root') as HTMLElement;
    expect(within(row).getByText('Severe delays')).toBeInTheDocument();
    expectNoUnguardedNowrapBadges(container);
  });
});

// Task 13: the third pinned-collection section, alongside "Your Lines" and
// "Your Stations" above -- same worst-first-then-alphabetical sort, same
// "browse the full list"/"pin something" empty-state shape, filtered from
// the same public, unauthenticated `getAllOperators()` catalogue fetched
// unconditionally above (see the fetch's own comment in app/page.tsx).
describe('DashboardPage -- Your Operators section', () => {
  const loggedIn = { authenticated: true as const, id: 'u1', username: 'a', name: 'A' };

  it('renders a pinned operator with a matching getAllOperators entry as a card under "Your Operators"', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: ['VT'],
    });
    vi.mocked(api.getAllOperators).mockResolvedValue([operator({ code: 'VT', name: 'Avanti West Coast' })]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your operators' })).toBeInTheDocument();
    expect(screen.getByText('Avanti West Coast')).toBeInTheDocument();
  });

  it("filters allOperators down to the caller's own pins, not the whole catalogue", async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: ['VT'],
    });
    vi.mocked(api.getAllOperators).mockResolvedValue([
      operator({ code: 'VT', name: 'Avanti West Coast' }),
      operator({ code: 'GW', name: 'Great Western Railway' }),
    ]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Avanti West Coast')).toBeInTheDocument();
    expect(screen.queryByText('Great Western Railway')).not.toBeInTheDocument();
  });

  it('shows the empty-state sentence, linking to /operators, when nothing is pinned', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: [],
    });
    renderWithMantine(await DashboardPage());

    expect(screen.getByText(/haven't pinned any operators yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Browse all operators' })).toHaveAttribute('href', '/operators');
  });

  it('still shows the heading-level "Browse all operators" link once Your Operators has pinned rows', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: ['VT'],
    });
    vi.mocked(api.getAllOperators).mockResolvedValue([operator({ code: 'VT' })]);
    renderWithMantine(await DashboardPage());

    const heading = screen.getByRole('heading', { name: 'Your operators' });
    expect(
      within(heading.parentElement as HTMLElement).getByRole('link', { name: 'Browse all operators' }),
    ).toHaveAttribute('href', '/operators');
  });

  // Final whole-branch review finding: the operators fetch used to be
  // unconditional, even though the anonymous branch never renders anything
  // pinned-operator-shaped -- pure wasted cost for that visitor.
  it('never fetches the operators catalogue for an anonymous visitor', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({
      authenticated: false,
      id: null,
      username: null,
      name: null,
    });
    // This mock's call count accumulates across every earlier test in this
    // file (nothing in this suite resets mocks between tests) -- cleared
    // here so this assertion is about THIS render, not the file's history.
    vi.mocked(api.getAllOperators).mockClear();
    renderWithMantine(await DashboardPage());

    expect(api.getAllOperators).not.toHaveBeenCalled();
  });

  // Final whole-branch review finding: a failed catalogue fetch used to be
  // swallowed as `[]`, which read identically to "the catalogue is empty"
  // once filtered against the caller's pins -- so a caller who HAD pinned
  // operators was told, falsely, that they hadn't pinned any at all.
  it('shows a "couldn\'t load" message, not the false empty-state sentence, when the operators fetch fails and the caller has pinned operators', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: ['VT'],
    });
    vi.mocked(api.getAllOperators).mockRejectedValue(new Error('network error'));
    renderWithMantine(await DashboardPage());

    expect(screen.getByText("Couldn't load operator status right now.")).toBeInTheDocument();
    expect(screen.queryByText(/haven't pinned any operators yet/)).not.toBeInTheDocument();
  });

  // Counterpart of the test above: when the caller genuinely has nothing
  // pinned, a failed fetch has nothing to have lost, so the ordinary
  // empty-state sentence (not the "couldn't load" one) is still the right
  // message.
  it('still shows the ordinary empty-state sentence when the operators fetch fails but the caller has nothing pinned', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: [],
    });
    vi.mocked(api.getAllOperators).mockRejectedValue(new Error('network error'));
    renderWithMantine(await DashboardPage());

    expect(screen.getByText(/haven't pinned any operators yet/)).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load operator status right now.")).not.toBeInTheDocument();
  });

  // Review M9: "Your Lines" (`LineStatusCard`) has no pin control of its
  // own, so "Your Operators" cards drop theirs on this page too, rather
  // than the dashboard having one card type pinnable and the other not for
  // no visible reason. Unpinning stays reachable from `/operators` itself.
  it('does not render a pin star on an operator card here (review M9)', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: [],
      pinnedStations: [],
      pinnedOperators: ['VT'],
    });
    vi.mocked(api.getAllOperators).mockResolvedValue([operator({ code: 'VT', name: 'Avanti West Coast' })]);
    const { container } = renderWithMantine(await DashboardPage());

    expect(container.querySelector('[aria-label*="Pin"]')).not.toBeInTheDocument();
  });

  // Review M10: the LNER signalling failure used to print in full on both
  // the "Your Lines" card and the "Your Operators" card for a caller who
  // pinned both the line and its operator -- collapse the operator card's
  // copy of it into a cross-reference once the driving line is already
  // pinned and shown above.
  it('collapses the reason into a cross-reference when the worst line is already pinned in Your Lines (review M10)', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['ecml'],
      pinnedStations: [],
      pinnedOperators: ['LN'],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'ecml',
        name: 'LNER East Coast Main Line',
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: 'Signalling failure near Peterborough',
            sampleAvailability: { state: 'no-coverage' },
          } as never,
        ],
      }),
    ]);
    vi.mocked(api.getAllOperators).mockResolvedValue([
      operator({
        code: 'LN',
        name: 'London North Eastern Railway',
        lineIds: ['ecml'],
        worstSeverity: 6,
        reason: 'Signalling failure near Peterborough',
        worstLineId: 'ecml',
        worstLineName: 'LNER East Coast Main Line',
      }),
    ]);
    renderWithMantine(await DashboardPage());

    // The full sentence appears once, on the "Your Lines" card -- the
    // operator card underneath cross-references it instead of repeating it.
    expect(screen.getAllByText('Signalling failure near Peterborough')).toHaveLength(1);
    expect(screen.getByText(/See/)).toBeInTheDocument();
  });
});
describe('DashboardPage -- journey sections', () => {
  const KGX: [string, string] = ['KGX', 'London Kings Cross'];
  const YRK: [string, string] = ['YRK', 'York'];
  const LOGGED_IN = { authenticated: true as const, id: 'u1', username: 'a', name: 'A' };

  // Midday on 17 Sep 2026, UK time. Only `Date` is faked: the page awaits
  // real promises.
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-09-17T11:00:00Z'));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /** Under way now: 11:30 to 13:30 UK time, its train 12 min late. */
  const underWay = journeyDetail(
    10,
    [
      trainLeg(1, KGX, YRK, '2026-09-17T10:30:00Z', '2026-09-17T12:30:00Z', {
        id: 101,
        status: 'en_route',
        delayMinutes: 12,
      }),
    ],
    'Work trip',
  );
  /** Later today: a Tube ride at 18:00. */
  const laterToday = journeyDetail(
    11,
    [untrackedLeg(1, 'Euston', 'Victoria', '2026-09-17T17:00:00Z', '2026-09-17T17:20:00Z')],
    'Evening out',
  );
  /** Next week, not read in full. */
  const nextWeek = journeyDetail(12, [
    trainLeg(1, YRK, KGX, '2026-09-24T08:00:00Z', '2026-09-24T10:00:00Z', { id: 102 }),
  ]);
  /** This morning, done. */
  const thisMorning = journeyDetail(
    13,
    [untrackedLeg(1, 'Home', 'Euston', '2026-09-17T06:00:00Z', '2026-09-17T06:30:00Z')],
    'Early walk',
  );

  function mockJourneys(details: JourneyDetail[], unreadItems: JourneyListItem[] = []) {
    vi.mocked(api.getMyJourneys).mockResolvedValue([
      ...details.map((detail) => journeyListItem(detail)),
      ...unreadItems,
    ]);
    vi.mocked(api.getJourney).mockImplementation(async (id) => {
      const detail = details.find((candidate) => candidate.id === id);
      if (!detail) throw new Error(`no journey ${id}`);
      return detail;
    });
  }

  it('logged out: no real journey sections, and the journeys are never fetched', async () => {
    vi.mocked(api.getMyJourneys).mockClear();
    vi.mocked(api.getJourney).mockClear();
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    expect(screen.queryByText('No upcoming journeys.')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'New journey' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'View all' })).not.toBeInTheDocument();
    expect(api.getMyJourneys).not.toHaveBeenCalled();
    expect(api.getJourney).not.toHaveBeenCalled();
  });

  // The teaser: the signed-in "Upcoming journeys" frame, saying what
  // tracking a journey gives you, with the one login flow (it also signs
  // new visitors up) and the planner, which needs no account.
  it('logged out: an Upcoming journeys teaser with a log in or sign up button and the planner', async () => {
    renderWithMantine(await DashboardPage());
    const headings = screen.getAllByRole('heading').map((heading) => heading.textContent);
    expect(headings.slice(0, 3)).toEqual(['Live UK rail status', 'Upcoming journeys', 'Right now']);
    expect(screen.getByRole('heading', { name: 'Upcoming journeys', level: 2 })).toBeInTheDocument();

    const teaser = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(teaser).getByText('Track a journey to see it here')).toBeInTheDocument();
    expect(within(teaser).getByText(/platform changes for every leg/)).toHaveTextContent(
      /connection is at risk.*Delay Repay/,
    );
    expect(within(teaser).getByRole('link', { name: 'Log in or sign up' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2F',
    );
    expect(within(teaser).getByRole('link', { name: 'Plan a journey' })).toHaveAttribute('href', '/plan');
    // The planner moved here from the quick links: one link, not two.
    expect(screen.getAllByRole('link', { name: 'Plan a journey' })).toHaveLength(1);
  });

  it('logged in: no teaser, and no log in or sign up button', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([thisMorning]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByText('Track a journey to see it here')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in or sign up' })).not.toBeInTheDocument();
    expect(screen.getAllByRole('region', { name: 'Upcoming journeys' })).toHaveLength(1);
  });

  it('logged in with a journey under way and others to come: current first, then upcoming, then the pins', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    vi.mocked(api.getJourney).mockClear();
    mockJourneys(
      [underWay, laterToday, thisMorning],
      [journeyListItem(nextWeek, undefined, { firstServiceDate: '2026-09-24', serviceDate: '2026-09-24' })],
    );
    renderWithMantine(await DashboardPage());

    const headings = screen.getAllByRole('heading').map((heading) => heading.textContent);
    // Nothing is pinned, so "Right now" leads the pins (review §3.1.4), after
    // the journeys.
    expect(headings.slice(0, 5)).toEqual(['Home', 'Current journey', 'Upcoming journeys', 'Right now', 'Your lines']);

    const current = screen.getByRole('region', { name: 'Current journey' });
    const card = within(current).getByRole('link', { name: /Work trip/ });
    expect(card).toHaveAttribute('href', '/journeys/10');
    expect(within(card).getByText('Delayed')).toBeInTheDocument();
    expect(within(card).getByText('12 min late')).toBeInTheDocument();

    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    const rows = within(upcoming)
      .getAllByRole('link')
      .map((link) => link.getAttribute('href'))
      .filter((href) => href?.startsWith('/journeys/') && href !== '/journeys/new');
    // Soonest first; this morning's finished journey is on neither list.
    expect(rows).toEqual(['/journeys/11', '/journeys/12']);
    expect(within(upcoming).getByText(/Departs 18:00/)).toBeInTheDocument();
    expect(screen.queryByText('Early walk')).not.toBeInTheDocument();
    // Only today's journeys were read in full.
    expect(
      vi
        .mocked(api.getJourney)
        .mock.calls.map(([id]) => id)
        .sort(),
    ).toEqual([10, 11, 13]);
  });

  it('logged in with upcoming journeys only: no current section', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([laterToday]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Upcoming journeys' })).toBeInTheDocument();
  });

  // DS-115: "New journey" lives in this section, so a logged-in visitor
  // with nothing coming up still gets it, where the section would be.
  it('logged in with no current or upcoming journeys: Upcoming journeys says so, and offers New journey', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([thisMorning]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    const headings = screen.getAllByRole('heading').map((heading) => heading.textContent);
    expect(headings.slice(0, 2)).toEqual(['Home', 'Upcoming journeys']);
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByText('No upcoming journeys.')).toBeInTheDocument();
    expect(within(upcoming).getByRole('link', { name: 'New journey' })).toHaveAttribute('href', '/journeys/new');
    expect(within(upcoming).queryByRole('link', { name: 'View all' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your lines', level: 2 })).toBeInTheDocument();
  });

  it('logged in with no journeys at all: still offers New journey', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    vi.mocked(api.getMyJourneys).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByRole('link', { name: 'New journey' })).toHaveAttribute('href', '/journeys/new');
  });

  // The home page has no "Tracked trains" list: a train tracked on its own
  // is a one-leg journey in the two journey sections.
  it('has no Tracked trains section, whatever is tracked', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([underWay]);
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      loneTrain(101, { journeyIds: [10], status: 'en_route' }),
      loneTrain(5, {
        pinOriginCrs: 'WAT',
        pinOriginName: 'London Waterloo',
        pinDestinationCrs: 'WOK',
        pinDestinationName: 'Woking',
        pinScheduledDeparture: '2026-09-17T15:00:00Z',
      }),
      loneTrain(6, { serviceDate: '2026-09-10', travelled: true }),
    ]);
    renderWithMantine(await DashboardPage());
    const headings = screen.getAllByRole('heading').map((heading) => heading.textContent);
    expect(headings).not.toContain('Tracked trains');
    // The journey's own train is shown through the journey, not again on
    // its own; the travelled train is left to the Journeys page.
    expect(screen.queryByRole('link', { name: /^London Kings Cross \(KGX\) to York \(YRK\)/ })).toBeNull();
    expect(screen.getAllByRole('link', { name: /^London Waterloo \(WAT\) to Woking \(WOK\)/ })).toHaveLength(1);
    expect(screen.queryByRole('link', { name: /10 Sep 2026/ })).not.toBeInTheDocument();
  });

  it('makes a lone train under way the current journey by its list row when its read fails', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    vi.mocked(api.getTrackedTrainById).mockClear();
    mockJourneys([]);
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      loneTrain(5, { status: 'en_route', delayMinutes: 12, delayProvisional: true }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(api.getTrackedTrainById).toHaveBeenCalledWith(5);
    const current = screen.getByRole('region', { name: 'Current journey' });
    const card = within(current).getByRole('link', {
      name: /^London Kings Cross \(KGX\) to York \(YRK\), 17 Sep 2026 · 10:00/,
    });
    expect(card).toHaveAttribute('href', '/train/W5/2026-09-17');
    expect(within(card).getByText(/12 min late/)).toBeInTheDocument();
    expect(within(card).getByText('Destination')).toBeInTheDocument();
    // The list row has no arrival time.
    expect(within(card).getByText('York (YRK)')).toBeInTheDocument();
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByText('No upcoming journeys.')).toBeInTheDocument();
  });

  it("reads a lone train in full and shows its arrival on the current card, as a journey's", async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([]);
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([loneTrain(5, { status: 'en_route', delayMinutes: 7 })]);
    vi.mocked(api.getTrackedTrainById).mockResolvedValue(
      trainState('KGX', 'YRK', '2026-09-17T10:30:00Z', '2026-09-17T12:30:00Z', {
        id: 5,
        status: 'en_route',
        delayMinutes: 7,
        journeyStops: [
          stop('KGX', {
            kind: 'Origin',
            scheduledDeparture: '2026-09-17T10:30:00Z',
            actualDeparture: '2026-09-17T10:37:00Z',
          }),
          stop('YRK', {
            kind: 'Terminate',
            scheduledArrival: '2026-09-17T12:30:00Z',
            estimatedArrival: '2026-09-17T12:37:00Z',
          }),
        ],
      }),
    );
    renderWithMantine(await DashboardPage());
    const card = within(screen.getByRole('region', { name: 'Current journey' })).getByRole('link');
    expect(within(card).getByText('York (YRK), arrive est. 13:37')).toBeInTheDocument();
  });

  it('keeps a lone train late leaving out of Current journey until it goes', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([]);
    // Timetabled 11:50 UK time, now 12:00, expected away 12:15.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      loneTrain(5, { pinScheduledDeparture: '2026-09-17T10:50:00Z' }),
    ]);
    vi.mocked(api.getTrackedTrainById).mockResolvedValue(
      trainState('KGX', 'YRK', '2026-09-17T10:50:00Z', '2026-09-17T12:50:00Z', {
        id: 5,
        journeyStops: [
          stop('KGX', {
            kind: 'Origin',
            scheduledDeparture: '2026-09-17T10:50:00Z',
            estimatedDeparture: '2026-09-17T11:15:00Z',
          }),
          stop('YRK', { kind: 'Terminate', scheduledArrival: '2026-09-17T12:50:00Z' }),
        ],
      }),
    );
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByRole('link', { name: /to York/ })).toHaveAttribute('href', '/train/W5/2026-09-17');
  });

  it('lists a lone train still to come in Upcoming journeys, ordered in with the journeys by departure', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys(
      [laterToday],
      [journeyListItem(nextWeek, undefined, { firstServiceDate: '2026-09-24', serviceDate: '2026-09-24' })],
    );
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      loneTrain(5, { pinScheduledDeparture: '2026-09-17T13:00:00Z', customName: 'Lunch train' }),
      loneTrain(6, { serviceDate: '2026-09-20', pinScheduledDeparture: '2026-09-20T07:00:00Z' }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    const rows = within(upcoming)
      .getAllByRole('link')
      .map((link) => link.getAttribute('href'))
      .filter((href) => href !== '/journeys/new' && href !== '/journeys');
    expect(rows).toEqual(['/train/W5/2026-09-17', '/journeys/11', '/train/W6/2026-09-20', '/journeys/12']);
    const lunch = within(upcoming).getByRole('link', { name: /^Lunch train/ });
    expect(within(lunch).getByText('London Kings Cross (KGX) to York (YRK), 17 Sep 2026 · 14:00')).toBeInTheDocument();
    expect(within(lunch).getByText('Planned')).toBeInTheDocument();
    expect(within(upcoming).getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/journeys');
    expect(within(upcoming).queryByText('No upcoming journeys.')).not.toBeInTheDocument();
  });

  it('leaves trains shared into a group to the Journeys page', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    mockJourneys([]);
    vi.mocked(api.getSharedGroupTrains).mockClear();
    renderWithMantine(await DashboardPage());
    expect(api.getSharedGroupTrains).not.toHaveBeenCalled();
    expect(screen.queryByText(/^From /)).not.toBeInTheDocument();
  });

  it('a failing journeys fetch drops only the journey sections, and folds no trains away', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    vi.mocked(api.getMyJourneys).mockRejectedValue(new Error('down'));
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      loneTrain(101, { journeyIds: [10], pinScheduledDeparture: '2026-09-17T13:00:00Z' }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    // "New journey" stays, and the section doesn't claim there are none.
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByRole('link', { name: /KGX\) to York/ })).toBeInTheDocument();
    expect(within(upcoming).getByText("Couldn't load your journeys. Try again.")).toBeInTheDocument();
    expect(within(upcoming).queryByText('No upcoming journeys.')).not.toBeInTheDocument();
    expect(within(upcoming).getByRole('link', { name: 'New journey' })).toHaveAttribute('href', '/journeys/new');
  });

  it('a failing detail read leaves a journey starting today in the upcoming list', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(LOGGED_IN);
    vi.mocked(api.getMyJourneys).mockResolvedValue([journeyListItem(underWay)]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Current journey' })).not.toBeInTheDocument();
    const upcoming = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(upcoming).getByRole('link', { name: /Work trip/ })).toBeInTheDocument();
  });
});

describe('metadata', () => {
  it("keeps the bare site name as the home page's title, with no redundant suffix", () => {
    // Every other page is "X · Distant Signal" through the root template;
    // the front door's own name IS the site name, so it opts out.
    expect(metadata.title).toEqual({ absolute: 'Distant Signal' });
  });

  it('carries its own description rather than only inheriting the site-wide one', () => {
    expect(metadata.description).toBe(
      'Live UK rail status: which lines have problems right now. Log in to pin lines and stations and track trains.',
    );
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // The root layout has no `openGraph`/`twitter` at all and Next merges
    // metadata per-field, so without these the site's own front page
    // unfurls with no og:title anywhere -- which is the whole point of
    // this export. Asserted against the literals, not against
    // `metadata.title`/`.description`: those read the same consts the
    // subject does, so a self-comparison could not fail.
    expect(metadata.openGraph).toMatchObject({
      title: 'Distant Signal',
      description:
        'Live UK rail status: which lines have problems right now. Log in to pin lines and stations and track trains.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary_large_image',
      title: 'Distant Signal',
      description:
        'Live UK rail status: which lines have problems right now. Log in to pin lines and stations and track trains.',
    });
  });
});
