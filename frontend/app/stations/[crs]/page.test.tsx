import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import StationDisruptionPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatusReport, StationOperatorSampleStats } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getStopPointDisruption: vi.fn(),
    getPreferences: vi.fn(),
    getStationName: vi.fn(),
    getStationSampleStats: vi.fn(),
    getAllTocs: vi.fn(),
  };
});

// `withStaleFallback` (lib/liveDataCache.ts) reads the session cookie via
// `next/headers` to scope its cache per visitor, and there is no Next
// request context in a unit test. Same stub shape lib/api.test.ts uses,
// plus the `.get()` the cache needs.
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

// PinToggle calls useRouter(), and unconditionally renders LoginPromptModal
// which calls usePathname()/useSearchParams() -- the same stub set
// app/lines/page.test.tsx documents for the same reason.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/stations/KGX',
  useSearchParams: () => new URLSearchParams(''),
  notFound: vi.fn(),
}));

function report(id: string, name: string): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['GR'],
    lineStatuses: [
      {
        statusSeverity: 6,
        statusSeverityDescription: 'Severe Delays',
        reason: 'Signalling failure',
        sampleAvailability: { state: 'no-coverage' },
        validityPeriods: [],
      } as never,
    ],
    computedAt: '2026-09-02T00:00:00Z',
  };
}

async function renderPage(crs = 'KGX') {
  const element = await StationDisruptionPage({ params: Promise.resolve({ crs }) });
  return renderWithMantine(element);
}

describe('StationDisruptionPage -- outage behaviour', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([report('ecml', 'East Coast Main Line')]);
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
  });

  it('renders the station\'s disruptions normally', async () => {
    await renderPage();
    expect(
      screen.getByRole('heading', { name: 'Disruptions at London Kings Cross (KGX)', level: 1 }),
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'East Coast Main Line' })).toBeInTheDocument();
  });

  // Design spec Decision 5 / plan Task 5: a backend outage keeps the
  // last-known content on screen instead of blanking the page.
  it('keeps rendering the last-known disruptions when the fetch fails', async () => {
    await renderPage();
    cleanup();

    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(
      screen.getByRole('heading', { name: 'Disruptions at London Kings Cross (KGX)', level: 1 }),
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'East Coast Main Line' })).toBeInTheDocument();
  });

  it('throws when the fetch fails with nothing cached, leaving app/error.tsx to handle it', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await expect(renderPage()).rejects.toThrow('connect ECONNREFUSED');
  });

  it('renders with the station unpinned rather than throwing when getPreferences fails', async () => {
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));

    await renderPage();
    expect(
      screen.getByRole('heading', { name: 'Disruptions at London Kings Cross (KGX)', level: 1 }),
    ).toBeInTheDocument();
  });
});

describe('StationDisruptionPage -- line-coverage distinction', () => {
  // The regression this task exists for: a station with zero line coverage
  // must render honestly, not as though every covering line were confirmed
  // fine -- see crates/api/src/routes/line_status.rs's
  // get_stop_point_disruption, which now 404s (ApiNotFoundError) for this
  // exact case instead of a `200 []` indistinguishable from good service.
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getStationName).mockResolvedValue('Raynes Park');
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
  });

  it('renders the "not covered" copy, not "no disruptions", when the backend 404s for zero line coverage', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(
      new ApiNotFoundError('no line coverage for stop point: RAY'),
    );

    await renderPage('RAY');

    expect(screen.getByText("This station isn't covered by our line-status tracking yet.")).toBeInTheDocument();
    expect(screen.queryByText('No disruptions affecting this station.')).not.toBeInTheDocument();
  });

  it('still renders "no disruptions" (not the coverage copy) for a genuinely covered, currently-fine station', async () => {
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);

    await renderPage('RAY');

    expect(screen.getByText('No disruptions affecting this station.')).toBeInTheDocument();
    expect(
      screen.queryByText("This station isn't covered by our line-status tracking yet."),
    ).not.toBeInTheDocument();
  });

  it('still throws (and is not swallowed as "no coverage") for a non-404 failure with nothing cached', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await expect(renderPage('RAY')).rejects.toThrow('connect ECONNREFUSED');
  });
});

describe('StationDisruptionPage -- sample stats by operator', () => {
  // Independent of the disruption-coverage tests above: this station has
  // ordinary line coverage throughout, so only `getStationSampleStats`
  // varies per test -- design spec Decision 9's three honest states.
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
  });

  it('renders the "not part of our live departure sampling" copy when the route 404s', async () => {
    vi.mocked(api.getStationSampleStats).mockRejectedValue(
      new ApiNotFoundError('no sample data collected for station: KGX'),
    );
    vi.mocked(api.getAllTocs).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText("This station isn't part of our live departure sampling.")).toBeInTheDocument();
  });

  it('renders the "no live departures currently recorded" copy for a covered-but-quiet board', async () => {
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('No live departures currently recorded at this station.')).toBeInTheDocument();
  });

  it('renders one row per operator in the order returned, resolving names via tocs with a bare-code fallback', async () => {
    const operatorStats: StationOperatorSampleStats[] = [
      {
        operator: 'GR',
        sampleAvailability: { state: 'available' },
        sampleStats: { total: 10, delayed: 2, cancelled: 0, skipped: 0, avgDelayMinutes: 3.5 },
      },
      {
        operator: 'SR',
        sampleAvailability: { state: 'below-threshold', observed: 1, required: 3 },
      },
    ];
    vi.mocked(api.getStationSampleStats).mockResolvedValue(operatorStats);
    // Only GR is named -- SR should fall back to the bare code.
    vi.mocked(api.getAllTocs).mockResolvedValue([{ code: 'GR', name: 'LNER' }]);

    await renderPage();

    const rows = screen.getAllByText(/LNER|^SR$/);
    expect(rows.map((el) => el.textContent)).toEqual(['LNER', 'SR']);
    expect(screen.getByText('Avg delay 3.5 min · 0% cancelled')).toBeInTheDocument();
    expect(screen.getByText('Too few live departures sampled to report a rate right now.')).toBeInTheDocument();
  });
});
