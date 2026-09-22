import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineHistoryPage, { HistoryResults } from './page';
import * as api from '@/lib/api';
import type { LineStatusReport, LineDailyStats } from '@/lib/types';
import { formatDate } from '@/lib/dateFormat';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getLineStatus: vi.fn(),
    getLineStatusHistory: vi.fn(),
    getHistoryRetention: vi.fn(),
    getLineDailyStats: vi.fn(),
    getLineDailyCoverageStats: vi.fn(),
  };
});
vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));
// Same rationale as TrendsResults.test.tsx's own mock -- this repo's
// convention is not to assert on Recharts' SVG output.
vi.mock('@mantine/charts', () => ({
  LineChart: () => <div data-testid="line-chart" />,
  BarChart: () => <div data-testid="bar-chart" />,
}));

function report(id: string, name: string): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['CC'],
    computedAt: '2026-08-31T09:00:00Z',
    lineStatuses: [],
  };
}

function dailyStatsRow(overrides: Partial<LineDailyStats> = {}): LineDailyStats {
  return {
    day: '2026-08-30',
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

async function renderPage(searchParams: { from?: string; to?: string; range?: string } = {}) {
  const element = await LineHistoryPage({
    params: Promise.resolve({ id: 'c2c' }),
    searchParams: Promise.resolve(searchParams),
  });
  return renderWithMantine(element);
}

// Regression coverage for the actual "/lines/[id]/history currently does
// not work" bug: this page is an async Server Component, and Mantine's
// `Tabs` carries a `"use client"` directive -- reaching its `List`/`Tab`/
// `Panel` sub-components via the `Tabs.List`/`Tabs.Tab`/`Tabs.Panel`
// dot-notation compound API (rather than the flat `TabsList`/`TabsTab`/
// `TabsPanel` named exports `page.tsx` now uses) resolved to `undefined`
// once Next actually compiled the Server/Client boundary, 500ing the whole
// route with "Element type is invalid ... got: undefined". This was
// confirmed live against a running dev server (`next dev`/`next build`),
// not caught by this file alone -- jsdom + `@testing-library/react`
// render everything as one ordinary client tree and never enforce that
// boundary, so these tests would pass identically against either form of
// the JSX. They're kept anyway as basic regression coverage for the page's
// rendering logic (which was previously completely untested), not as a
// substitute for the live check.
describe('LineHistoryPage', () => {
  beforeEach(() => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('c2c', 'c2c (London, Tilbury & Southend line)')]);
    vi.mocked(api.getHistoryRetention).mockResolvedValue({
      historyRetentionDays: 7,
      dailyStatsRetentionDays: 300,
      halfHourlyStatsRetentionHours: 840,
    });
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([]);
    vi.mocked(api.getLineDailyStats).mockResolvedValue([]);
    vi.mocked(api.getLineDailyCoverageStats).mockResolvedValue([]);
  });

  it('renders both tabs, defaulting to Timeline, with no crash', async () => {
    await renderPage();
    expect(screen.getByRole('tab', { name: 'Timeline', selected: true })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Trends' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to line' })).toHaveAttribute('href', '/lines/c2c');
  });

  // Review §2.11: the Timeline panel's Suspense fallback used to be a
  // bare, unlabelled `Skeleton` -- no accessible name, nothing for a
  // screen reader to announce while `getLineStatusHistory` was still in
  // flight. A call that never resolves keeps the boundary suspended for
  // the life of the test, so the fallback content can be asserted on
  // directly.
  it('shows a labelled, announced loading state while the Timeline fetch is pending', async () => {
    vi.mocked(api.getLineStatusHistory).mockReturnValue(new Promise(() => {}));
    await renderPage();

    const fallback = await screen.findByText('Loading history…');
    expect(fallback).toHaveAttribute('role', 'status');
    expect(fallback).toHaveAttribute('aria-busy', 'true');
  });

  it('switching to the Trends tab renders the daily-stats charts without crashing', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyStatsRow({ day: '2026-08-30' }), dailyStatsRow({ day: '2026-08-31' })]);
    await renderPage();

    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));

    expect(await screen.findAllByTestId('line-chart')).toHaveLength(2);
  });

  // NB deviating from the plan's own literal fixture here: the plan's Task
  // 10 test asserted this against `renderPage()`'s bare DEFAULT range (the
  // "Last 7 days" preset, no query params), but that's inconsistent with
  // Task 6's own already-committed, already-tested `availableGranularities`
  // point-budget math (`frontend/lib/history.ts`) -- a 7-day range is
  // 7*48=336 half-hour buckets, over the MAX_CHART_POINTS=200 ceiling, so
  // `halfHour` is correctly EXCLUDED for the default preset (confirmed by
  // `history.test.ts`'s own "excludes half-hourly and hourly ... for a
  // 10-day range" case, same formula, smaller range). Using a narrow
  // 12-hour custom range instead -- the same width `history.test.ts`'s own
  // "offers all four tiers for a narrow (12-hour) range" case uses -- to
  // exercise a range where all four really are available.
  it('the Trends tab shows a granularity control offering all four tiers for a narrow range', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyStatsRow({ day: '2026-08-30' })]);
    await renderPage({ from: '2026-08-31T00:00:00Z', to: '2026-08-31T12:00:00Z' });
    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    for (const label of ['30 min', 'Hourly', '6-hourly', 'Daily']) {
      expect(await screen.findByText(label)).toBeInTheDocument();
    }
  });

  it('a very wide custom range hides the sub-daily tiers and still renders the daily chart', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyStatsRow({ day: '2026-01-01' })]);
    await renderPage({ from: '2025-01-01T00:00:00Z', to: '2026-08-21T00:00:00Z' });
    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    expect(await screen.findByText('Daily')).toBeInTheDocument();
    expect(screen.queryByText('30 min')).not.toBeInTheDocument();
    expect(screen.getByText(/are not shown for this range/)).toBeInTheDocument();
  });

  it('switching to the Trends tab with no daily stats yet shows the sane fallback, not a crash', async () => {
    await renderPage();

    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));

    expect(await screen.findByText('Not enough sampled data yet for this line.')).toBeInTheDocument();
  });

  it('renders a Timeline per-day header at h2, one level below this page\'s only h1 ("History: {name}")', async () => {
    // Awaits HistoryResults directly rather than rendering LineHistoryPage
    // and waiting on its Suspense boundary -- see HistoryResults' own doc
    // comment in page.tsx for why (this harness has no RSC runtime, so that
    // Suspense boundary never settles here). Reuses report()'s own default
    // computedAt ('2026-08-31T09:00:00Z') rather than inventing a new date
    // string, per this task's plan.
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([report('c2c', 'c2c (London, Tilbury & Southend line)')]);
    renderWithMantine(
      await HistoryResults({
        id: 'c2c',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/c2c/history',
      }),
    );

    expect(screen.getByRole('heading', { name: formatDate('2026-08-31T09:00:00Z'), level: 2 })).toBeInTheDocument();
  });

  // Review §2.12: the timezone note appears once for the whole Timeline
  // section, not once per row -- exercised here with entries spanning two
  // different days (two separate day groups, each with its own rows) so a
  // per-row or per-day regression would be caught.
  it('states the UK-local-time note exactly once across multiple days of rows', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([
      report('c2c', 'c2c (London, Tilbury & Southend line)'),
      { ...report('c2c', 'c2c (London, Tilbury & Southend line)'), computedAt: '2026-08-30T09:00:00Z' },
    ]);
    renderWithMantine(
      await HistoryResults({
        id: 'c2c',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/c2c/history',
      }),
    );

    expect(screen.getAllByText('Times in UK local time')).toHaveLength(1);
  });

  // Review §2.9: "recompute" is the aggregator's own internal word for
  // what happened, not a sentence a passenger would recognise.
  it('describes entries as status changes, not internal "recompute"s', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([
      report('c2c', 'c2c (London, Tilbury & Southend line)'),
      { ...report('c2c', 'c2c (London, Tilbury & Southend line)'), computedAt: '2026-08-30T09:00:00Z' },
    ]);
    renderWithMantine(
      await HistoryResults({
        id: 'c2c',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/c2c/history',
      }),
    );

    expect(screen.getByText(/2 status changes across/)).toBeInTheDocument();
    expect(screen.queryByText(/recompute/)).not.toBeInTheDocument();
  });

  // Task 3.4.9: a status whose own `reason` is literally its severity
  // label reads as redundant right next to the row's StatusBadge, which
  // already shows that same label -- "GOOD SERVICE Good Service 21:34".
  it('does not echo the severity label back as the reason text', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([
      {
        ...report('c2c', 'c2c (London, Tilbury & Southend line)'),
        lineStatuses: [
          {
            statusSeverity: 10,
            statusSeverityDescription: 'Good Service',
            reason: 'Good Service',
            dataQuality: 'knowledgebase',
            validityPeriods: [],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
          },
        ],
      },
    ]);
    renderWithMantine(
      await HistoryResults({
        id: 'c2c',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/c2c/history',
      }),
    );

    expect(screen.getByText('No incidents reported')).toBeInTheDocument();
    expect(screen.queryByText('Good Service', { selector: '.issueRow__reason' })).not.toBeInTheDocument();
  });

  // A genuinely empty reason is a different fact (none was supplied at
  // all) and must keep its own, distinct copy rather than collapsing into
  // the redundancy-only "No incidents reported" text above.
  it('still says "No reason given" for a genuinely empty reason', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([
      report('c2c', 'c2c (London, Tilbury & Southend line)'),
    ]);
    renderWithMantine(
      await HistoryResults({
        id: 'c2c',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/c2c/history',
      }),
    );

    expect(screen.getByText('No reason given')).toBeInTheDocument();
  });

  // 2026-09-22 UX review §5.1: this Suspense boundary catches suspension,
  // not errors -- an unguarded throw here used to propagate all the way to
  // the route's global error.tsx, blanking the page title, "Back to line"
  // link, Period control and both tabs. This is a regression test for the
  // fix, not for the specific (unconfirmed) LNER trigger the review's
  // screenshot showed -- any fetch failure now resolves to real markup.
  it('resolves to a Paper instead of throwing when the history fetch fails', async () => {
    vi.mocked(api.getLineStatusHistory).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(
      await HistoryResults({
        id: 'lner-ecml',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/lner-ecml/history',
      }),
    );

    const text = screen.getByText("Couldn't load this line's history right now.");
    expect(text).toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  // 2026-09-22 UX review §5.2: "No history entries in that range." used to
  // be honest but terminal -- no way out, and no link to the one control
  // (the Period picker) that would actually change the answer.
  it('offers a "Try 30 days" way out of an empty range, when not already viewing 30 days', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([]);
    renderWithMantine(
      await HistoryResults({
        id: 'cross-country',
        from: '2026-08-26T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '7d',
        basePath: '/lines/cross-country/history',
      }),
    );

    expect(screen.getByText('No history entries in that range.')).toBeInTheDocument();
    const link = screen.getByRole('link', { name: '30 days' });
    expect(link).toHaveAttribute('href', '/lines/cross-country/history?range=30d');
  });

  it('withholds the "Try 30 days" suggestion once 30 days is already the active preset', async () => {
    vi.mocked(api.getLineStatusHistory).mockResolvedValue([]);
    renderWithMantine(
      await HistoryResults({
        id: 'cross-country',
        from: '2026-08-03T00:00:00Z',
        to: '2026-09-02T00:00:00Z',
        preset: '30d',
        basePath: '/lines/cross-country/history',
      }),
    );

    expect(screen.getByText('No history entries in that range.')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: '30 days' })).not.toBeInTheDocument();
  });
});
