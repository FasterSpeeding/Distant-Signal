import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import DashboardPage from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatusReport, TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api');
// `withStaleFallback` (lib/liveDataCache.ts) reads the session cookie via
// `next/headers` to scope its cache per visitor, and there is no Next
// request context in a unit test. Same stub shape lib/api.test.ts uses,
// plus the `.get()` the cache needs.
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

// The anonymous branch's login nudge is now LoginLink (Task 1), which calls
// usePathname()/useSearchParams() -- same stub AuthStatus.test.tsx and
// TicketPanel.test.tsx use for the same reason.
vi.mock('next/navigation', () => ({
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

function report(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'x', id: 'bakerloo', name: 'Bakerloo', modeName: 'tube', operators: [],
    lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '', sampleAvailability: { state: 'no-coverage' } } as never],
    computedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function item(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    // Defaults to null (bare-code rendering) rather than a real name so
    // every pre-existing assertion in this file that checks for the code
    // itself keeps working unchanged -- the name-rendering path gets its
    // own dedicated test below.
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
  };
}

// `vi.mock('@/lib/api')` automocks every export to a vi.fn() returning
// undefined. The page now calls `.catch()` on several of these promises, so
// each needs at least a resolved default; individual tests override what
// they care about. The stale cache is real module state, so it is reset too.
beforeEach(() => {
  __resetStaleCacheForTests();
  vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
  vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
  vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
  vi.mocked(api.getStationName).mockResolvedValue(null);
  vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
});

describe('DashboardPage', () => {
  it('anonymous, all lines good: shows the no-disruption message, not a raw empty state', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toHaveAttribute(
      'href', '/api/auth/login?return_to=%2F',
    );
  });

  // NotificationsToggle (Decision 6's single global toggle) renders itself
  // unconditionally on this page -- both branches below -- but gates on
  // browser capability (`'serviceWorker' in navigator && 'PushManager' in
  // window`), which jsdom has neither of by default. No component mock is
  // needed for this page's own tests as a result: the real component
  // already degrades to rendering nothing here, same as it would in any
  // browser lacking Push API support. See NotificationsToggle.test.tsx for
  // the component's own behavior under a stubbed-supported browser.
  it('renders no "Enable notifications" control under jsdom (no Push API support)', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('button', { name: /enable notifications/i })).not.toBeInTheDocument();
  });

  it('anonymous, a line disrupted: lists it, worst-first', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never] }),
      report(),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('anonymous: merged TfL counterpart ids are excluded from the widget', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never] }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
  });

  it('logged in: renders the existing pinned-lines/pinned-stations behavior, not the anonymous branch', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    // Load-bearing specifically for the PINNED case (Task 7): this user has
    // pinned a line, so "Right now" must stay absent even though the
    // authenticated branch can now render it for a zero-pinned-lines user.
    expect(screen.queryByText(/Right now/)).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in to pin your lines and stations' })).not.toBeInTheDocument();
  });

  it('shows the live "Right now" module to a logged-in user with no pinned lines', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never] }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now' })).toBeInTheDocument();
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('still shows it when they have pinned stations but no pinned lines', async () => {
    // Gated on pinned LINES only: a user with pinned stations but no
    // pinned lines still has a line-shaped hole on the dashboard.
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['WAT'] });
    vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now' })).toBeInTheDocument();
  });

  it('hides it once they pin a line', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Right now' })).not.toBeInTheDocument();
  });

  it('renders the module at heading level 2 in the authenticated branch, no skip', async () => {
    // h1 "Your Lines" -> h2 "Your Stations" -> h2 "Right now" -> h2 "Your
    // Tracked Trains".
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
  });

  it('anonymous branch still renders "Right now" identically after the RightNowModule extraction', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never] }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('logged in, an auth glitch (getSession rejects): degrades to the anonymous branch, not a crash', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('boom'));
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toBeInTheDocument();
  });
});

// Minimal, shared stubs for the two pre-existing fetches this page also
// makes -- not under test here, just enough for the page to render without
// throwing. Scoped to this describe block only (not global): the
// `DashboardPage` describe block above sets its own explicit mocks per
// test and doesn't need these defaults.
//
// `getSession` is explicitly re-mocked to a logged-in user here too --
// without this, the last test in the `DashboardPage` describe block above
// leaves `getSession` mocked to a *rejected* promise (its own
// auth-glitch-degrades-to-anonymous test), and since Vitest doesn't reset
// mocks between tests by default, that rejection would otherwise leak into
// every test below and force the anonymous branch, which never renders the
// Your Tracked Trains section this whole describe block exists to test.
describe('DashboardPage -- Your Tracked Trains section', () => {
  beforeEach(() => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  });

  it('getMyTrackedTrains() returns null (logged out): section absent, other two sections unchanged', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Stations' })).toBeInTheDocument();
  });

  it('getMyTrackedTrains() returns [] (logged in, nothing tracked): section absent', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
  });

  it('populated list: section present with a "View all" link to /track/mine', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/track/mine');
  });

  it('more than 5 tracked trains: only the first 5 (as returned) are rendered', async () => {
    const trains = Array.from({ length: 7 }, (_, i) => item({ id: i + 1, pinOriginCrs: `T${i + 1}` }));
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(trains);
    renderWithMantine(await DashboardPage());
    for (let i = 1; i <= 5; i++) {
      expect(screen.getByText(new RegExp(`^T${i}`))).toBeInTheDocument();
    }
    expect(screen.queryByText(/^T6/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^T7/)).not.toBeInTheDocument();
  });

  it('rows render in the order getMyTrackedTrains returned them (no client-side re-sort)', async () => {
    const first = item({ id: 1, pinOriginCrs: 'WAT' });
    const second = item({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    renderWithMantine(await DashboardPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ delayMinutes: 12 })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
  });

  it('renders station names when the backend resolved them, not just bare codes', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ pinOriginName: 'London Waterloo', pinDestinationName: 'Woking' }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('London Waterloo (WAT) → Woking (WOK)')).toBeInTheDocument();
  });

  it('falls back to the bare code, not "null" or an empty label, when a name did not resolve', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ pinOriginName: null, pinDestinationName: null })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });
});

describe('DashboardPage -- outage behaviour', () => {
  // Outage behaviour (design spec Decision 5 / plan Task 5).
  it('keeps rendering the last-known line status when the status fetch fails', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central' }),
    ]);
    renderWithMantine(await DashboardPage());
    cleanup();

    vi.mocked(api.getLineStatusForMode).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await DashboardPage());

    // Still the real page, not a throw up to app/error.tsx.
    expect(screen.getByRole('heading', { name: 'Distant Signal', level: 1 })).toBeInTheDocument();
  });

  it('renders with nothing pinned rather than throwing when getPreferences fails', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.c', name: 'A' });
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines', level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/haven't pinned any lines yet/)).toBeInTheDocument();
  });

  it('renders rather than throwing when getMyTrackedTrains fails', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.c', name: 'A' });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    vi.mocked(api.getMyTrackedTrains).mockRejectedValue(new Error('500'));

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines', level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
  });

  it('keeps the dashboard up when a pinned station\'s disruption fetch fails', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.c', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['KGX'] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Stations', level: 2 })).toBeInTheDocument();
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
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.c', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['RAY'] });
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

    expect(screen.getByText('Good Service')).toBeInTheDocument();
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
