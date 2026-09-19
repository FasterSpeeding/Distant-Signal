import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineDetailPage, { generateMetadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import { ApiNotFoundError } from '@/lib/api';
import type {
  LineStatusReport,
  LineSummary,
  CustomLineDetail,
  LineHalfHourlyStats,
  LineHalfHourlyCoverageStats,
} from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getLineStatus: vi.fn(),
    getCustomLine: vi.fn(),
    getLineDefinition: vi.fn(),
    getAllLines: vi.fn(),
    // Defaulted here (rather than in every describe's own `beforeEach`,
    // the way `getAllLines` etc. are) since most tests in this file don't
    // care about operator-name resolution at all -- an empty list keeps
    // `operatorLabel` on its bare-code fallback, matching this page's
    // pre-existing "Operators: SW" behaviour. The one test that DOES care
    // (`categoryLabel`/`operatorLabel` below) overrides it locally.
    getAllTocs: vi.fn().mockResolvedValue([]),
    getLineHalfHourlyStats: vi.fn(),
    getLineHalfHourlyCoverageStats: vi.fn(),
  };
});
// `withStaleFallback` (lib/liveDataCache.ts) reads the session cookie via
// `next/headers` to scope its cache per visitor, and there is no Next
// request context in a unit test. Same stub shape lib/api.test.ts uses,
// plus the `.get()` the cache needs.
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

// DeleteLineButton (rendered whenever Edit/Delete render) calls useRouter()
// from next/navigation, which throws outside a real Next.js App Router
// tree -- same workaround PinToggle.test.tsx/TicketPanel.test.tsx use.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  notFound: vi.fn(),
}));
// Same rationale as TrendsResults.test.tsx's own mock: this repo's
// convention is not to assert on Recharts' SVG output, so the embedded
// trends section below is exercised through the data-driven props actually
// handed to `LineChart`, not real chart rendering.
vi.mock('@mantine/charts', () => ({
  LineChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="line-chart" data-series={props.series.map((series) => series.name).join(',')} />
  ),
  BarChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="bar-chart" data-series={props.series.map((series) => series.name).join(',')} />
  ),
}));

function report(id: string, name: string): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SW'],
    computedAt: '2026-08-31T09:00:00Z',
    lineStatuses: [],
  };
}

const lines: LineSummary[] = [
  { id: 'custom-my-commute', name: 'My Commute', category: 'custom', operators: ['SW'], source: 'custom' },
];

function customLine(overrides: Partial<CustomLineDetail> = {}): CustomLineDetail {
  return {
    id: 'custom-my-commute',
    name: 'My Commute',
    operators: ['SW'],
    stations: ['WOK', 'CLJ'],
    headcodePrefixes: [],
    destinationCrsFilter: [],
    isOwner: true,
    sharedWithGroups: [],
    ...overrides,
  };
}

function halfHourlyStatsRow(overrides: Partial<LineHalfHourlyStats> = {}): LineHalfHourlyStats {
  return {
    halfHourStart: '2026-08-30T14:00:00Z',
    sampleCycles: 500,
    total: 100,
    delayed: 10,
    cancelled: 2,
    skipped: 1,
    avgDelayMinutes: 3.5,
    delayRate: 0.1,
    cancellationRate: 0.02,
    skipRate: 0.01,
    ...overrides,
  };
}

function halfHourlyCoverageStatsRow(
  overrides: Partial<LineHalfHourlyCoverageStats> = {},
): LineHalfHourlyCoverageStats {
  return {
    halfHourStart: '2026-08-30T14:00:00Z',
    resolvedWindows: 25,
    total: 100,
    delayed: 10,
    cancelled: 2,
    skipped: 1,
    avgDelayMinutes: 3.5,
    delayRate: 0.1,
    cancellationRate: 0.02,
    skipRate: 0.01,
    ...overrides,
  };
}

async function renderPage(id = 'custom-my-commute') {
  const element = await LineDetailPage({ params: Promise.resolve({ id }) });
  return renderWithMantine(element);
}

describe('LineDetailPage Edit/Delete visibility', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
  });

  it('a catalogue line (getCustomLine 404s) never shows Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    await renderPage();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    // Flushes the embedded trends Suspense boundary before the test ends,
    // rather than leaving it to resolve after -- same reason the two tests
    // below explicitly wait for the trends section too.
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  it('a custom line the caller OWNS (isOwner: true) shows Edit/Delete', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: true }));
    await renderPage();
    expect(screen.getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/lines/custom-my-commute/edit');
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  // A `200` from `getCustomLine` used to prove ownership by itself. Custom-
  // line group sharing ended that: a member of a group the line was shared
  // into gets the same full detail with `isOwner: false`, and must be shown
  // no mutation control -- both because the backend would 404 the attempt
  // and because offering them reads as "this line is yours".
  it('a custom line SHARED with the caller (isOwner: false) shows neither Edit nor Delete', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: false }));
    await renderPage();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    expect(
      screen.getByText('Shared with you through a group. Only its owner can edit it.'),
    ).toBeInTheDocument();
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  it('a shared line never names the groups its owner shared it into', async () => {
    // `sharedWithGroups` is owner-only on the wire; this pins that the page
    // would not render it even if a future backend change leaked one.
    vi.mocked(api.getCustomLine).mockResolvedValue(
      customLine({ isOwner: false, sharedWithGroups: [{ id: 'grp-secret', name: 'Secret Group' }] }),
    );
    await renderPage();
    expect(screen.queryByText(/Secret Group/)).not.toBeInTheDocument();
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  // The unchanged-behaviour half of the no-status-row fix below: when the
  // status endpoint does answer, the page renders the computed severity
  // and none of the "not yet computed" copy.
  it('renders the computed status, not the no-status state, when a status row exists', async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: true }));
    await renderPage();
    expect(screen.getByText('Good Service')).toBeInTheDocument();
    expect(screen.queryByText('No status yet')).not.toBeInTheDocument();
    expect(screen.queryByText(/No status has been computed for this line yet/)).not.toBeInTheDocument();
    await screen.findByText('Not enough sampled data yet for this line.');
  });

  it("shows the owner their line's own 'Shared with' group list, linked", async () => {
    vi.mocked(api.getCustomLine).mockResolvedValue(
      customLine({ isOwner: true, sharedWithGroups: [{ id: 'grp-1', name: 'Family' }] }),
    );
    await renderPage();
    expect(screen.getByRole('link', { name: 'Family' })).toHaveAttribute('href', '/groups/grp-1');
    await screen.findByText('Not enough sampled data yet for this line.');
  });
});

// Item 3: the detail page used to only link out to `/lines/[id]/history`
// for the trend charts -- this embeds a rolling-24h half-hourly preview of
// them directly via `HalfHourlyTrendsResults` (formerly `HourlyTrendsResults`,
// before the bucket size was halved to 30 minutes), sharing `TrendsCharts`
// (the actual chart-rendering leaf) with the history page's daily Trends
// tab, per docs/superpowers/plans/2026-09-02-trend-chart-granularity.md
// Task 13.
describe('LineDetailPage embedded trends', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine());
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
  });

  it('renders the trend charts when the line already has recent half-hourly stats', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([
      halfHourlyStatsRow({ halfHourStart: '2026-08-30T13:30:00Z' }),
      halfHourlyStatsRow({ halfHourStart: '2026-08-30T14:00:00Z' }),
    ]);
    await renderPage();

    expect(await screen.findByRole('heading', { name: 'Recent trends (last 24 hours)' })).toBeInTheDocument();
    const charts = await screen.findAllByTestId('line-chart');
    expect(charts).toHaveLength(2);
    expect(screen.queryByText('Not enough sampled data yet for this line.')).not.toBeInTheDocument();
    // The full range picker/Timeline/longer ranges stay one click away
    // rather than being duplicated inline.
    expect(screen.getByRole('link', { name: 'View history' })).toHaveAttribute(
      'href',
      '/lines/custom-my-commute/history',
    );
  });

  // The sane-fallback case that matters most for Task 1: a line that was
  // just created has no `line_status_daily_stats` rows yet (the aggregator
  // hasn't run a cycle for it), so this must degrade to the same honest
  // empty state `TrendsResults` already shows on the full history page --
  // not an error, not an indefinite loading state.
  it('shows the sane no-data-yet fallback for a line with no half-hourly stats -- e.g. one just created', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    await renderPage();

    expect(await screen.findByRole('heading', { name: 'Recent trends (last 24 hours)' })).toBeInTheDocument();
    expect(await screen.findByText('Not enough sampled data yet for this line.')).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
  });

  // Review §2.11: both "Recent trends" Suspense boundaries used to fall
  // back to a bare, unlabelled `Skeleton` -- no accessible name, nothing
  // for a screen reader to announce while the fetch was still in flight.
  // A `getLineHalfHourlyStats`/`getLineHalfHourlyCoverageStats` call that
  // never resolves keeps both boundaries suspended for the life of the
  // test, so their fallback content can be asserted on directly.
  it('shows a labelled, announced loading state while the half-hourly fetches are pending', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockReturnValue(new Promise(() => {}));
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockReturnValue(new Promise(() => {}));
    await renderPage();

    const fallbacks = await screen.findAllByText('Loading trends…');
    expect(fallbacks).toHaveLength(2);
    for (const fallback of fallbacks) {
      const region = fallback.closest('[role="status"]');
      expect(region).not.toBeNull();
      expect(region).toHaveAttribute('aria-busy', 'true');
    }
  });

  // Regression guard for Task 2 of
  // docs/superpowers/plans/2026-09-03-half-hourly-coverage-trends-plan.md --
  // asserts the wiring actually landed on this page, not just that
  // HalfHourlyCoverageTrendsResults works in isolation
  // (HalfHourlyCoverageTrendsResults.test.tsx already covers that).
  it('renders the "Full coverage" section underneath the sample-derived trend charts', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([
      halfHourlyCoverageStatsRow({ halfHourStart: '2026-08-30T13:30:00Z' }),
      halfHourlyCoverageStatsRow({ halfHourStart: '2026-08-30T14:00:00Z' }),
    ]);
    await renderPage();

    expect(await screen.findByRole('heading', { name: 'Full coverage' })).toBeInTheDocument();
    // Two charts from the "Full coverage" section on top of the sample
    // series' own empty state (no chart).
    expect(await screen.findAllByTestId('line-chart')).toHaveLength(2);
  });

  it('shows the "Full coverage" empty-state fallback for a line with no full-coverage data yet -- e.g. every line today, since no producer exists', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
    await renderPage();

    expect(await screen.findByText('Not enough full-coverage data yet for this line.')).toBeInTheDocument();
  });
});

describe('LineDetailPage -- outage behaviour', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    // A plain Error, NOT ApiNotFoundError: getCustomLine maps only 401/404
    // to that class, so a rejected fetch or a 5xx -- what an outage
    // actually produces -- arrives as a generic Error. Asserting with
    // ApiNotFoundError here passed while the page still blanked in the
    // real failure mode.
    vi.mocked(api.getCustomLine).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getLineDefinition).mockRejectedValue(new Error('connect ECONNREFUSED'));
    // Same: this is awaited inside a <Suspense> on the page, and Suspense
    // does not catch errors -- a resolved [] never exercised that path.
    vi.mocked(api.getLineHalfHourlyStats).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
  });

  it('keeps rendering the last-known status when the status fetch fails', async () => {
    await renderPage();
    cleanup();

    vi.mocked(api.getLineStatus).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
  });

  // withStaleFallback rethrows ApiNotFoundError unconditionally, so the
  // notFound() branch must keep working even with a warm cache entry.
  //
  // Every source is denied here, not just the status one: a 404 from
  // `/Line/{id}/Status` alone now means "no status row yet", which is not
  // grounds for a 404 on its own (see the no-status-row describe below).
  // A line that no source can even name is the real deleted/unknown case,
  // and that is what this asserts -- with a warm stale entry sitting in
  // the cache for the very same key, which must not be served.
  it('still 404s for an unknown line rather than serving a stale entry', async () => {
    await renderPage();
    cleanup();

    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getAllLines).mockResolvedValue([]);

    // `notFound` is mocked as a no-op here (the real one throws), so the
    // page renders on past it -- what matters is that the 404 branch was
    // taken, and that the stale entry was NOT used to render the line.
    await renderPage();
    expect(notFound).toHaveBeenCalled();
    expect(screen.queryByRole('heading', { name: 'My Commute', level: 1 })).not.toBeInTheDocument();
  });
});

// The bug this describe exists for: a custom line whose own detail page
// 404'd for its owner. `/Line/{id}/Status` 404s until the aggregator has
// written a `line_status` row, which a just-created custom line does not
// have yet, and the page treated that 404 as "no such line" -- even though
// `GET /public/lines/{id}` was returning the line's full definition
// happily. The definition is what the bulk of this page renders, so a
// missing status row must degrade only the status section.
describe('LineDetailPage -- a line with no status row yet', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('no matching line(s)'));
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine());
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
  });

  it("renders the owner's brand-new custom line instead of 404ing it", async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();

    await renderPage();

    expect(notFound).not.toHaveBeenCalled();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
    // The definition-derived parts all still render. `getAllTocs()`
    // defaults to `[]` for this whole file, so "SW" has no name to
    // resolve to and stays bare -- a separate test below covers the
    // resolved-name case.
    expect(screen.getByText('Operators: SW')).toBeInTheDocument();
    // Review §2.9: the raw TOML-enum token ("custom") must not reach the
    // page -- `categoryLabel()` maps it to a human label.
    expect(screen.getByText('Category: Custom line')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/lines/custom-my-commute/edit');
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View history' })).toHaveAttribute(
      'href',
      '/lines/custom-my-commute/history',
    );
    // The embedded trend sections are still wired up -- they read
    // `line_status_half_hourly_stats`, not `line_status`, so a missing
    // status row is none of their business. Only the heading is asserted
    // (it renders outside the Suspense boundaries): with no `IssueList`
    // on this branch, nothing client-side schedules the re-render this
    // test environment needs to retry a resolved async-component promise,
    // so the boundaries' own contents never flush here. That is a
    // limitation of rendering async Server Components under jsdom, not of
    // the page -- `HalfHourlyTrendsResults.test.tsx` covers their real
    // behaviour directly.
    expect(screen.getByRole('heading', { name: 'Recent trends (last 24 hours)' })).toBeInTheDocument();
  });

  // Review §2.9: an ATOC code like "SW" is meaningless to a passenger who
  // knows "South Western Railway" -- this resolves it the same way
  // `app/lines/page.tsx` and `app/stations/[crs]/page.tsx` already do.
  it('resolves an operator code to its name via getAllTocs()', async () => {
    vi.mocked(api.getAllTocs).mockResolvedValueOnce([{ code: 'SW', name: 'South Western Railway' }]);

    await renderPage();

    expect(screen.getByText('Operators: South Western Railway (SW)')).toBeInTheDocument();
  });

  it('says so honestly rather than claiming Good Service', async () => {
    await renderPage();

    expect(
      screen.getByText(
        'No status has been computed for this line yet. It appears here once the aggregator has run a cycle covering it.',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('No status yet')).toBeInTheDocument();
    // `worstStatus` would have synthesised exactly this for an empty
    // report -- true of a line the aggregator has assessed, a lie about
    // one it has never seen.
    expect(screen.queryByText('Good Service')).not.toBeInTheDocument();
  });

  it('renders for a group member the line is shared with, minus the owner controls', async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: false }));
    // A granted non-owner is not shown this line by `GET /public/lines`
    // (that list is "mine to edit"), so its name can only come from
    // `getCustomLine` here.
    vi.mocked(api.getAllLines).mockResolvedValue([]);

    await renderPage();

    expect(notFound).not.toHaveBeenCalled();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
    expect(
      screen.getByText('Shared with you through a group. Only its owner can edit it.'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });

  // Not just a custom-line case: a catalogue line is listed from config
  // regardless of whether `line_status` has a row for it, so the same
  // degraded-but-rendered page is the right answer there too.
  it('renders a catalogue line that has no status row', async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getAllLines).mockResolvedValue([
      { id: 'sw-main', name: 'South Western Main Line', category: 'main-line', operators: ['SW'], source: 'catalogue' },
    ]);

    await renderPage('sw-main');

    expect(notFound).not.toHaveBeenCalled();
    expect(screen.getByRole('heading', { name: 'South Western Main Line', level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
  });

  // The other half of the fix: a genuinely nonexistent id, or someone
  // else's private custom line (both `getCustomLine` 404 + absent from
  // `getAllLines`), must still 404 the whole page.
  it('404s when no source can name the line -- an unknown id, or a line that is not the viewer\'s to see', async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getLineDefinition).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getAllLines).mockResolvedValue(lines);

    await renderPage('custom-someone-elses');

    expect(notFound).toHaveBeenCalled();
    // `notFound` is a no-op in these tests, so the page renders on past it
    // -- with nothing in the heading. Asserting the heading is empty (not
    // merely that it doesn't say "My Commute", which was never a candidate
    // for this id) is what would catch a future fallback that resolved a
    // name from a source this viewer isn't entitled to.
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('');
  });

  // The backend 404s rather than returning an empty array today, so this
  // is a guard on the defensive branch in `fetchLineStatusResult`, not on
  // reachable behaviour: the pre-fix code read `.name` straight off
  // `reports[0]` and would have died on a TypeError.
  it('treats an empty status array the same as a 404, rather than crashing', async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getLineStatus).mockResolvedValue([]);

    await renderPage();

    expect(notFound).not.toHaveBeenCalled();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
    expect(screen.getByText('No status yet')).toBeInTheDocument();
  });

  // Only a 404 means "no status computed yet". A 5xx or a dropped
  // connection is an outage, and (with no stale entry to fall back on)
  // must still reach app/error.tsx's retrying state rather than be
  // rendered as a confident "this line has no status".
  it('does not dress a backend outage up as a missing status row', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new Error('connect ECONNREFUSED'));
    await expect(renderPage()).rejects.toThrow('connect ECONNREFUSED');
  });

  // Failing closed: an unreachable backend must not be reported as a
  // nonexistent line, but it also leaves nothing to render, so `notFound()`
  // is still the outcome -- what matters is that ownership/existence are
  // never guessed at from a connectivity failure.
  it('404s rather than inventing a page when every source is unreachable', async () => {
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    vi.mocked(api.getCustomLine).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getLineDefinition).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getAllLines).mockResolvedValue([]);

    await renderPage();

    expect(notFound).toHaveBeenCalled();
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
  });
});

describe('LineDetailPage -- embedded fetches must not blank the page', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getCustomLine).mockResolvedValue({} as never);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
  });

  // getCustomLine maps only 401/404 to ApiNotFoundError; a rejected fetch
  // or 5xx is a plain Error, which used to be rethrown straight to
  // app/error.tsx.
  it('renders when the custom-line ownership probe fails with a connectivity error', async () => {
    vi.mocked(api.getCustomLine).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
    // Failed closed: ownership could not be confirmed, so no owner controls.
    expect(screen.queryByRole('link', { name: 'Edit' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
  });

  // Rendered inside <Suspense>, which catches suspension but not errors.
  it('renders when the embedded trends fetch fails', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'My Commute', level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("Trend data isn't available right now.")).toBeInTheDocument();
  });
});

describe('generateMetadata', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
  });

  it('titles the page with the line name and describes its worst current status', async () => {
    vi.mocked(api.getLineStatus).mockResolvedValue([
      {
        ...report('custom-my-commute', 'My Commute'),
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: 'Signal failure at Woking',
            dataQuality: 'trust-inferred',
            validityPeriods: [],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'no-coverage' },
          } as never,
        ],
      },
    ]);
    const metadata = await generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) });
    expect(metadata.title).toBe('My Commute — Distant Signal');
    expect(metadata.description).toBe('My Commute: Severe Delays — Signal failure at Woking');
    expect(metadata.openGraph?.title).toBe('My Commute — Distant Signal');
    expect(metadata.twitter).toMatchObject({ card: 'summary' });
  });

  it('describes a line with no reason text (Good Service)', async () => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    const metadata = await generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) });
    expect(metadata.description).toBe('My Commute: Good Service');
  });

  // `generateMetadata` runs independently of the page component and its
  // own `notFound()` 404s the route by itself -- so it has to draw the
  // same "no status row" vs. "no such line" distinction the page does, or
  // it would silently undo the fix for every status-less line.
  it('calls notFound() when no source can name the line, matching the page component', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(api.getAllLines).mockResolvedValue([]);
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();
    await generateMetadata({ params: Promise.resolve({ id: 'unknown' }) });
    expect(notFound).toHaveBeenCalled();
  });

  it('titles a line that has no status row yet, rather than 404ing the route', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('no matching line(s)'));
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine());
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();

    const metadata = await generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) });

    expect(notFound).not.toHaveBeenCalled();
    expect(metadata.title).toBe('My Commute — Distant Signal');
    expect(metadata.description).toBe('My Commute: no status computed yet');
    expect(metadata.openGraph?.title).toBe('My Commute — Distant Signal');
  });

  // Same failing-closed rule as the page's: a `getCustomLine` that blew up
  // on connectivity is not evidence the line is gone, but it leaves no
  // name either, so the route still 404s rather than titling a page
  // "undefined".
  it('falls back to the all-lines list when the custom-line probe is unreachable', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('no matching line(s)'));
    vi.mocked(api.getCustomLine).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();

    const metadata = await generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) });

    expect(notFound).not.toHaveBeenCalled();
    expect(metadata.title).toBe('My Commute — Distant Signal');
  });

  // The page component lets this same fetch throw, and the two halves of
  // one route must agree: with the line list unreachable we cannot know
  // whether the id is a catalogue line, and `notFound()` would be a
  // confident answer we don't have.
  it('propagates an unreachable line list instead of 404ing the route', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new ApiNotFoundError('no matching line(s)'));
    vi.mocked(api.getCustomLine).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('connect ECONNREFUSED'));
    const { notFound } = await import('next/navigation');
    vi.mocked(notFound).mockClear();

    await expect(
      generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) }),
    ).rejects.toThrow('connect ECONNREFUSED');
    expect(notFound).not.toHaveBeenCalled();
  });

  it('propagates a non-404 status failure instead of reporting no status', async () => {
    vi.mocked(api.getLineStatus).mockRejectedValue(new Error('connect ECONNREFUSED'));
    await expect(
      generateMetadata({ params: Promise.resolve({ id: 'custom-my-commute' }) }),
    ).rejects.toThrow('connect ECONNREFUSED');
  });
});
