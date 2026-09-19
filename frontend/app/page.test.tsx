import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import DashboardPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type {
  LineStatusReport,
  SharedGroupCustomLine,
  SharedGroupTrain,
  TrackedTrainListItem,
} from '@/lib/types';

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
    sharedGroupCount: 0,
    ...overrides,
    customName: overrides.customName ?? null,
  };
}

// One row of `GET /public/groups/shared-trains` -- a train another member
// shared into a group this caller belongs to. Distinct origin/destination
// from `item()` above so a shared row and an own row are never confusable
// in an assertion.
function sharedTrain(overrides: Partial<SharedGroupTrain> = {}): SharedGroupTrain {
  return {
    groupId: 'group-1',
    groupName: 'Family',
    trainSubscriptionId: 50,
    pinOriginCrs: 'PAD',
    pinDestinationCrs: 'RDG',
    pinOriginName: null,
    pinDestinationName: null,
    pinScheduledDeparture: '2026-08-31T07:15:00Z',
    serviceDate: '2026-08-31',
    resolutionStatus: 'resolved',
    trainUid: 'S99999',
    status: 'en_route',
    delayMinutes: null,
    customName: null,
    addedBy: 'user-2',
    addedByName: 'Sam',
    addedByTag: null,
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
  // Every test that predates group sharing says nothing about it -- default
  // the fourth fetch to "nothing shared with you" so each still describes
  // exactly the scenario it was written for. Also load-bearing mechanically:
  // the page calls `.catch()` on this promise, and an automocked `undefined`
  // return has no `.catch`.
  vi.mocked(api.getSharedGroupTrains).mockResolvedValue([]);
  vi.mocked(api.getStationName).mockResolvedValue(null);
  vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
  vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue(null);
});

describe('DashboardPage', () => {
  // Review §3.1.7: the nav bar already says "Distant Signal" -- this
  // page's own <h1> used to repeat it, reading as duplicated branding
  // above the fold on mobile. It keeps the <h1> (for the outline, and for
  // a link-unfurler bot reading past a nav it never sees) but no longer
  // duplicates the brand name in its text.
  it('titles the anonymous h1 something other than the brand name the nav already carries', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Live UK rail status', level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Distant Signal' })).not.toBeInTheDocument();
  });

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
  // already degrades to a disabled control here, same as it would in any
  // browser lacking Push API support (review §2.11 -- the slot is always
  // reserved, never absent). See NotificationsToggle.test.tsx for the
  // component's own behavior under a stubbed-supported browser.
  it('renders a disabled "Enable notifications" control under jsdom (no Push API support)', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(await screen.findByRole('button', { name: /enable notifications/i })).toBeDisabled();
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
    // Both pinned sections are empty here, so review §3.1.4 puts "Right
    // now" first: h2 "Right now" -> h1 "Your Lines" -> h2 "Your Stations"
    // -> ... . Its own level-2 heading never skips to h3 regardless of
    // where in the page it renders.
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
  });

  // Review §3.1.4: the authenticated dashboard used to double a "Browse
  // all lines"/"Look up a station" link beside its section heading with an
  // identical link ~40px below it, inside the empty-state sentence.
  describe('empty-state link deduplication (review §3.1.4)', () => {
    it('keeps only the inline "Browse all lines" link when Your Lines is empty, not a second one beside the heading', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['WAT'] });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your Lines', level: 1 });
      // Scope to everything from the heading's own row up to (but not
      // including) the next section, so this can't accidentally pass by
      // matching the "Your Stations" section's own link instead.
      const section = heading.closest('div')?.parentElement as HTMLElement;
      expect(within(section).getAllByRole('link', { name: 'Browse all lines' })).toHaveLength(1);
    });

    it('keeps only the inline "Look up a station" link when Your Stations is empty, not a second one beside the heading', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your Stations', level: 2 });
      const section = heading.closest('div')?.parentElement as HTMLElement;
      expect(within(section).getAllByRole('link', { name: 'Look up a station' })).toHaveLength(1);
    });

    it('still shows the heading-level "Browse all lines" link once Your Lines has pinned rows', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your Lines', level: 1 });
      expect(within(heading.parentElement as HTMLElement).getByRole('link', { name: 'Browse all lines' })).toHaveAttribute(
        'href',
        '/lines',
      );
    });

    it('still shows the heading-level "Look up a station" link once Your Stations has pinned rows', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['WAT'] });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      renderWithMantine(await DashboardPage());
      const heading = screen.getByRole('heading', { name: 'Your Stations', level: 2 });
      expect(
        within(heading.parentElement as HTMLElement).getByRole('link', { name: 'Look up a station' }),
      ).toHaveAttribute('href', '/stations');
    });
  });

  // Review §3.1.4: "order 'Right now' first when both pinned sections are
  // empty" -- otherwise the module's usual position (after Your Stations,
  // only when Lines specifically is empty) is unchanged.
  describe('"Right now" ordering when both pinned sections are empty (review §3.1.4)', () => {
    function headingNames() {
      return screen.getAllByRole('heading').map((h) => h.textContent);
    }

    it('puts "Right now" before "Your Lines" when both sections are empty', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
      renderWithMantine(await DashboardPage());
      const names = headingNames();
      expect(names.indexOf('Right now')).toBeLessThan(names.indexOf('Your Lines'));
      // Rendered exactly once, not doubled up at both its old and new spot.
      expect(names.filter((n) => n === 'Right now')).toHaveLength(1);
    });

    it('keeps "Right now" after "Your Stations" (its ordinary spot) when only Lines is empty', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: ['WAT'] });
      vi.mocked(api.getStationName).mockResolvedValue('Waterloo');
      vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
      renderWithMantine(await DashboardPage());
      const names = headingNames();
      expect(names.indexOf('Your Stations')).toBeLessThan(names.indexOf('Right now'));
      expect(names.filter((n) => n === 'Right now')).toHaveLength(1);
    });
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
            { statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never,
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
      expect(
        screen.getByText(/Showing the first 5 — 7 more lines are not at Good Service\./),
      ).toBeInTheDocument();
      expect(overflowLink(/7 more lines are not at Good Service/)).toHaveAttribute('href', '/lines');
    });

    it('says "line is", not "lines are", when exactly one is hidden', async () => {
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(6));
      renderWithMantine(await DashboardPage());
      expect(
        screen.getByText(/Showing the first 5 — 1 more line is not at Good Service\./),
      ).toBeInTheDocument();
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
            { statusSeverity: 9, statusSeverityDescription: 'Minor Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never,
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
      expect(
        screen.getByText(/Showing the first 5 — 1 more line is not at Good Service\./),
      ).toBeInTheDocument();
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
            { statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '', sampleAvailability: { state: 'no-coverage' } } as never,
          ],
        }),
      ]);
      renderWithMantine(await DashboardPage());
      expect(screen.getByText(/6 lines not at Good Service right now/)).toBeInTheDocument();
      expect(
        screen.getByText(/Showing the first 5 — 1 more line is not at Good Service\./),
      ).toBeInTheDocument();
    });

    it('renders the overflow line in the authenticated zero-pinned-lines branch too', async () => {
      vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
      vi.mocked(api.getLineStatusForMode).mockResolvedValue(disruptedReports(7));
      renderWithMantine(await DashboardPage());
      expect(screen.getByRole('heading', { name: 'Right now', level: 2 })).toBeInTheDocument();
      expect(
        screen.getByText(/Showing the first 5 — 2 more lines are not at Good Service\./),
      ).toBeInTheDocument();
      // "Browse all lines" beside the "Your Lines" heading points at /lines
      // too, so this scopes to the overflow row's own copy of it.
      expect(overflowLink(/2 more lines are not at Good Service/)).toHaveAttribute('href', '/lines');
    });
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

  // Task 1.5 (WCAG 2.5.3): the tracked-train row pairs a route title with
  // this delay badge in a `Group wrap="nowrap"` (`StatusRow`) -- without a
  // shrink guard on the badge, a long enough route/station name can crush
  // it instead of the title truncating. `expectNoUnguardedNowrapBadges`
  // sweeps every `data-wrap="nowrap"` row this page renders, not just this
  // one train's.
  it('gives every wrap="nowrap" row\'s badge a shrink guard, even with a long route name', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({
        delayMinutes: 12,
        pinOriginName: 'A Station With An Implausibly Long Name For Testing',
        pinDestinationName: 'Another Equally Verbose Destination Station Name',
      }),
    ]);
    const { container } = renderWithMantine(await DashboardPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
    expectNoUnguardedNowrapBadges(container);
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
    expect(screen.getByRole('heading', { name: 'Live UK rail status', level: 1 })).toBeInTheDocument();
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

  const loggedIn = { authenticated: true as const, id: 'u1', email: 'a@b.com', name: 'A' };

  it('is absent entirely when nothing has been shared with the caller', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
  });

  it('is absent for an anonymous visitor even if the endpoint somehow returned rows', async () => {
    // The anonymous branch returns before this section is ever rendered --
    // a shared custom line is only ever visible to a signed-in member.
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
    expect(screen.queryByText('My Commute')).not.toBeInTheDocument();
  });

  it('renders a shared line with its group tag, its sharer, and a link to the line', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Lines shared with you' })).toBeInTheDocument();
    expect(screen.getByText('My Commute')).toBeInTheDocument();
    expect(screen.getByText('from Family')).toBeInTheDocument();
    expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /My Commute/ })).toHaveAttribute(
      'href',
      '/lines/custom-my-commute',
    );
  });

  it("tags a line shared into two of the caller's groups with both, on one row", async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([
      sharedLine({ groupId: 'grp-1', groupName: 'Family' }),
      sharedLine({ groupId: 'grp-2', groupName: 'Commute Buddies' }),
    ]);
    renderWithMantine(await DashboardPage());

    expect(screen.getAllByText('My Commute')).toHaveLength(1);
    expect(screen.getByText('from Family')).toBeInTheDocument();
    expect(screen.getByText('from Commute Buddies')).toBeInTheDocument();
  });

  it('falls back to "a member" rather than a raw user id when the sharer has no display name', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine({ grantedByName: null })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('Shared by a member')).toBeInTheDocument();
    expect(screen.queryByText(/user-2/)).not.toBeInTheDocument();
  });

  it('renders no owner controls of any kind on a shared line -- it is view-only', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
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
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['custom-my-commute'],
      pinnedStations: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'custom-my-commute', name: 'My Commute' }),
    ]);
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
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getPreferences).mockResolvedValue({
      pinnedLines: ['custom-my-commute'],
      pinnedStations: [],
    });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue([sharedLine()]);
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Lines shared with you' })).toBeInTheDocument();
    expect(screen.getAllByText('My Commute')).toHaveLength(1);
  });

  it('survives the shared-lines fetch failing, rather than blanking the dashboard', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
    vi.mocked(api.getSharedGroupCustomLines).mockRejectedValue(new Error('boom'));
    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Lines shared with you' })).not.toBeInTheDocument();
  });

  // Task 1.5 (WCAG 2.5.3): `SharedCustomLineSummaryRow` pairs its title
  // with a status badge in a `Group wrap="nowrap"` (`StatusRow`) -- the
  // badge must not be crushable, even with a very long line name.
  it('gives the status badge a shrink guard, even with a very long line name', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn);
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
    expect(within(row).getByText('Severe Delays')).toBeInTheDocument();
    expectNoUnguardedNowrapBadges(container);
  });
});

// The home page's own copy of the /track/mine fix: a train another member
// shared into a group the caller belongs to is one of "their" trains for
// the purposes of this section, and before this it was invisible here --
// the only surfaces that showed it at all were /groups/{id} and
// /track/mine. Mirrors app/track/mine/page.test.tsx's own
// `group-shared trains` block, scoped to this page's condensed section.
describe('DashboardPage -- group-shared trains in Your Tracked Trains', () => {
  beforeEach(() => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  });

  it('renders a group-shared train alongside the caller’s own, tagged with its group and sharer', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
    expect(screen.getByText('PAD → RDG')).toBeInTheDocument();
    expect(screen.getByText('from Family')).toBeInTheDocument();
    expect(screen.getByText('Shared by Sam')).toBeInTheDocument();
  });

  it('a caller who tracks nothing themselves still gets the section for trains shared with them', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByText('PAD → RDG')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/track/mine');
  });

  // Task 1.5 (WCAG 2.5.3): `SharedTrainSummaryRow` pairs its title with
  // `TrackedTrainStatusBadge` in a `Group wrap="nowrap"` (`StatusRow`) --
  // the badge(s) must not be crushable, even with very long station names.
  it('gives the shared row’s status badges a shrink guard, even with very long station names', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({
        delayMinutes: 9,
        pinOriginName: 'A Station With An Implausibly Long Name For Testing',
        pinDestinationName: 'Another Equally Verbose Destination Station Name',
      }),
    ]);

    const { container } = renderWithMantine(await DashboardPage());

    expect(screen.getByText('9m late')).toBeInTheDocument();
    expectNoUnguardedNowrapBadges(container);
  });

  it('nothing own and nothing shared: the section stays hidden entirely (Decision 4)', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([]);

    renderWithMantine(await DashboardPage());

    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
  });

  it('carries both tags inside the shared row itself, not stranded elsewhere in the section', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    const sharedRow = screen.getByText('PAD → RDG').closest('.mantine-Card-root');
    expect(sharedRow).not.toBeNull();
    expect(within(sharedRow as HTMLElement).getByText('from Family')).toBeInTheDocument();
    expect(within(sharedRow as HTMLElement).getByText('Shared by Sam')).toBeInTheDocument();

    // ...and the caller's own row carries neither -- an unattributed row
    // must keep reading as "one I tracked myself".
    const ownRow = screen.getByText('WAT → WOK').closest('.mantine-Card-root');
    expect(within(ownRow as HTMLElement).queryByText(/^from /)).not.toBeInTheDocument();
    expect(within(ownRow as HTMLElement).queryByText(/^Shared by /)).not.toBeInTheDocument();
  });

  it('a train shared into two of the caller’s groups renders once, tagged with both', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ groupId: 'g1', groupName: 'Family' }),
      sharedTrain({ groupId: 'g2', groupName: 'Commuters' }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getAllByText('PAD → RDG')).toHaveLength(1);
    expect(screen.getByText('from Family')).toBeInTheDocument();
    expect(screen.getByText('from Commuters')).toBeInTheDocument();
  });

  it('a shared train with a uid links to the public /train/{uid}/{date} page, even before it resolves', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ resolutionStatus: 'schedule_matched', trainUid: 'S99999', status: null }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('link', { name: /PAD → RDG/ })).toHaveAttribute('href', '/train/S99999/2026-08-31');
  });

  it('a shared train with no uid is not linked at all — the by-id route is owner-scoped', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ resolutionStatus: 'pending', trainUid: null, status: null }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('PAD → RDG')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /PAD → RDG/ })).not.toBeInTheDocument();
    // Specifically never the owner-scoped by-id route, which 404s for
    // anyone but the train's owner.
    for (const link of screen.getAllByRole('link')) {
      expect(link.getAttribute('href') ?? '').not.toMatch(/\/train\/by-id\//);
    }
  });

  it('renders the shared train’s live status and delay badges, same as an own row', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ delayMinutes: 9 })]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('En route')).toBeInTheDocument();
    expect(screen.getByText('9m late')).toBeInTheDocument();
  });

  it('falls back to "a member" when the sharer has no name or username, never a raw user id', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ addedBy: 'sso-subject-1234', addedByName: null }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Shared by a member')).toBeInTheDocument();
    expect(screen.queryByText(/sso-subject-1234/)).not.toBeInTheDocument();
  });

  /** A BLANK name, not a null one -- what an identity provider with no name
   * on file for the sharer actually sends. `??` treats `''` as a usable
   * label, which would render "Shared by " with nothing after it. The
   * backend normalizes blanks away now; this guards the rows written before
   * it did, exactly as /track/mine's and /groups/{id}'s own rows do. */
  it('a sharer whose name is blank rather than null is still credited as "a member"', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ addedBy: 'sso-subject-1234', addedByName: '   ' }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Shared by a member')).toBeInTheDocument();
  });

  /** Same distinguishing suffix /track/mine and /groups/{id} render, on the
   * home page's copy of the shared-train row: two sharers this app cannot
   * name are two different credits, not one repeated "a member". */
  it('credits two unnameable sharers distinguishably', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ trainSubscriptionId: 50, addedBy: 'sso-1', addedByName: null, addedByTag: 'a1b2c3' }),
      sharedTrain({
        trainSubscriptionId: 51,
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        addedBy: 'sso-2',
        addedByName: null,
        addedByTag: 'd4e5f6',
      }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Shared by a member (#a1b2c3)')).toBeInTheDocument();
    expect(screen.getByText('Shared by a member (#d4e5f6)')).toBeInTheDocument();
  });

  it('offers no edit/delete/ticket control on someone else’s shared train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    expect(screen.queryByRole('button', { name: /rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /delete/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /ticket/i })).not.toBeInTheDocument();
  });

  it('puts the caller’s own rows before the shared ones, each half in its endpoint’s order', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ id: 1, pinOriginCrs: 'WAT' })]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ trainSubscriptionId: 50, pinOriginCrs: 'PAD' }),
      sharedTrain({ trainSubscriptionId: 51, pinOriginCrs: 'KGX' }),
    ]);

    renderWithMantine(await DashboardPage());

    const rendered = screen.getAllByText(/→ (WOK|RDG)$/).map((el) => el.textContent ?? '');
    expect(rendered).toEqual(['WAT → WOK', 'PAD → RDG', 'KGX → RDG']);
  });

  /** The home page labels a shared row by ROUTE, deliberately, where
   * /track/mine labels it with the sharer's `customName`: this page's own
   * rows label by route too, and the two halves of one list disagreeing
   * about what a heading even is would be worse than differing from a
   * sibling page. Pinned so a later "unify these two rows" refactor has to
   * make that choice consciously rather than silently. */
  it('labels a shared row by route, not by the sharer’s custom name', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ customName: 'Morning commute' })]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('PAD → RDG')).toBeInTheDocument();
    expect(screen.queryByText('Morning commute')).not.toBeInTheDocument();
  });

  it('a shared train with no pin data degrades to a date-only label, never "Invalid Date"', async () => {
    // The pre-match case: an NR-primary subscription whose train has no
    // schedule data yet has no departure time, and a pin created from a
    // bare origin has no destination.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({
        pinOriginCrs: null,
        pinDestinationCrs: null,
        pinScheduledDeparture: null,
        resolutionStatus: 'pending',
        trainUid: null,
        status: null,
      }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('Unknown station')).toBeInTheDocument();
    expect(screen.queryByText(/Invalid Date/)).not.toBeInTheDocument();
    expect(screen.queryByText(/null/i)).not.toBeInTheDocument();
    // Still attributed -- the tags are what stop an unlabelled row reading
    // as one the caller tracked themselves.
    expect(screen.getByText('from Family')).toBeInTheDocument();
  });

  it('caps the whole section at 5 rows across both halves, not 5 of each', async () => {
    // The cap is Decision 1's, and it is about this supplementary section
    // not out-competing the line-status overview above it -- two five-row
    // halves would be ten rows.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(
      Array.from({ length: 4 }, (_, i) => item({ id: i + 1, pinOriginCrs: `T${i + 1}` })),
    );
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([
      sharedTrain({ trainSubscriptionId: 50, pinOriginCrs: 'S1' }),
      sharedTrain({ trainSubscriptionId: 51, pinOriginCrs: 'S2' }),
    ]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('T4 → WOK')).toBeInTheDocument();
    expect(screen.getByText('S1 → RDG')).toBeInTheDocument();
    expect(screen.queryByText('S2 → RDG')).not.toBeInTheDocument();
  });

  it('a caller with five of their own trains sees no shared rows here — "View all" is the way to them', async () => {
    // The documented, deliberate consequence of capping the SECTION rather
    // than each half. It's the case most likely to surprise someone later,
    // so it is pinned rather than left implied by the partial-truncation
    // test above.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(
      Array.from({ length: 5 }, (_, i) => item({ id: i + 1, pinOriginCrs: `T${i + 1}` })),
    );
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('T5 → WOK')).toBeInTheDocument();
    expect(screen.queryByText('PAD → RDG')).not.toBeInTheDocument();
    expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/track/mine');
  });

  it('never renders the same train twice when a shared row collides with one of the caller’s own', async () => {
    // `trainSubscriptionId` and `TrackedTrainListItem.id` are the same id
    // space (`train_subscriptions.id`). The backend already excludes the
    // caller's own subscriptions (`ts.user_id <> $1`); `mergeSharedTrains`
    // filters again so a regression there can't produce a duplicated row
    // that ALSO loses its own row's affordances.
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ id: 50 })]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain({ trainSubscriptionId: 50 })]);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
    expect(screen.queryByText('PAD → RDG')).not.toBeInTheDocument();
    expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
  });

  it('a null (401) shared-trains response degrades to the caller’s own list, not a crash', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue(null);

    renderWithMantine(await DashboardPage());

    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
    expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
  });

  it('a failing shared-trains fetch still renders the caller’s own trains and the rest of the page', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    vi.mocked(api.getSharedGroupTrains).mockRejectedValue(new Error('API request failed: 500'));

    renderWithMantine(await DashboardPage());

    expect(screen.getByRole('heading', { name: 'Your Lines', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
    expect(screen.queryByText(/^from /)).not.toBeInTheDocument();
  });

  it('an anonymous visitor never sees the section, whatever the shared-trains call returns', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    // Defensive: the anonymous branch returns before any of this is read,
    // so even an (impossible) populated response can't leak a shared train
    // onto a logged-out home page.
    vi.mocked(api.getSharedGroupTrains).mockResolvedValue([sharedTrain()]);

    renderWithMantine(await DashboardPage());

    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
    expect(screen.queryByText('PAD → RDG')).not.toBeInTheDocument();
    expect(screen.queryByText('from Family')).not.toBeInTheDocument();
  });
});

describe('metadata', () => {
  it("keeps the bare site name as the home page's title, with no redundant suffix", () => {
    // Every other page is "X — Distant Signal"; the front door is the one
    // page whose own name IS the site name, and "Distant Signal — Distant
    // Signal" is not an improvement.
    expect(metadata.title).toBe('Distant Signal');
  });

  it('carries its own description rather than only inheriting the site-wide one', () => {
    expect(metadata.description).toBe(
      "Live UK rail line status at a glance: which lines aren't running a Good Service right now — then pin the lines and stations you care about, and track your trains, once you're logged in.",
    );
  });

  it('hedges the pinned and tracked sections as logged-in-only, which is all an unfurler bot can ever see', () => {
    // A link-unfurler carries no session cookie, so it renders the
    // ANONYMOUS branch -- which has no "Your Lines"/"Your Stations"/"Your
    // Tracked Trains" sections at all. An unhedged "plus the lines you've
    // pinned" would promise a logged-out visitor something the page they
    // were just linked to does not contain.
    expect(metadata.description).toMatch(/once you're logged in/);
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
        "Live UK rail line status at a glance: which lines aren't running a Good Service right now — then pin the lines and stations you care about, and track your trains, once you're logged in.",
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Distant Signal',
      description:
        "Live UK rail line status at a glance: which lines aren't running a Good Service right now — then pin the lines and stations you care about, and track your trains, once you're logged in.",
    });
  });
});
