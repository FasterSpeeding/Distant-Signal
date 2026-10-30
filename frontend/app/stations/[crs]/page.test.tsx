import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import StationDisruptionPage, { generateMetadata } from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatusReport, StationOperatorSampleStats } from '@/lib/types';
import { visibleText } from '@/test/routeText';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getStopPointDisruption: vi.fn(),
    getPreviewStopPointDisruption: vi.fn(),
    getPreferences: vi.fn(),
    getStationName: vi.fn(),
    getStationSampleStats: vi.fn(),
    getStationAccessibility: vi.fn(),
    getAllTocs: vi.fn(),
    getTransitRailLinks: vi.fn(),
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
// `notFound()` actually throws in real Next.js -- mocked to do the same
// (rather than a bare `vi.fn()` no-op) so a test can tell "notFound()
// halted execution" from "notFound() is a no-op and execution silently
// fell through to a wrong-but-non-erroring result," same pattern as
// app/train/[uid]/[date]/page.test.tsx's notFoundMock.
const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
const permanentRedirectMock = vi.fn((url: string) => {
  throw new Error(`NEXT_REDIRECT ${url}`);
});
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/stations/KGX',
  useSearchParams: () => new URLSearchParams(''),
  notFound: () => notFoundMock(),
  permanentRedirect: (url: string) => permanentRedirectMock(url),
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
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([report('ecml', 'East Coast Main Line')]);
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getStationAccessibility).mockResolvedValue({});
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);
  });

  it('offers to plan a journey from this station, pre-filling it as the origin', async () => {
    await renderPage();
    expect(screen.getByRole('link', { name: 'Plan a journey from here' })).toHaveAttribute('href', '/plan?origin=KGX');
  });

  it('links the tram stops to change to, and nothing while there are none', async () => {
    await renderPage();
    expect(screen.queryByText(/Change for trams/)).toBeNull();
    cleanup();
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([
      {
        network: 'metrolink',
        networkName: 'Manchester Metrolink',
        stopId: '9400ZZMAPIC',
        stopName: 'Piccadilly',
        kind: 'interchange',
        note: null,
      },
    ]);
    await renderPage();
    expect(screen.getByText(/Change for trams/).textContent).toBe(
      'Change for trams: Piccadilly (Manchester Metrolink)',
    );
    expect(screen.getByRole('link', { name: 'Piccadilly' })).toHaveAttribute(
      'href',
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
  });

  it("renders the station's disruptions normally", async () => {
    await renderPage();
    expect(screen.getByRole('heading', { name: 'London Kings Cross (KGX)', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'East Coast Main Line' })).toBeInTheDocument();
  });

  // Design spec Decision 5 / plan Task 5: a backend outage keeps the
  // last-known content on screen instead of blanking the page.
  it('keeps rendering the last-known disruptions when the fetch fails', async () => {
    await renderPage();
    cleanup();

    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'London Kings Cross (KGX)', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'East Coast Main Line' })).toBeInTheDocument();
  });

  it('throws when the fetch fails with nothing cached, leaving app/error.tsx to handle it', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await expect(renderPage()).rejects.toThrow('connect ECONNREFUSED');
  });

  it('renders with the station unpinned rather than throwing when getPreferences fails', async () => {
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'London Kings Cross (KGX)', level: 1 })).toBeInTheDocument();
  });

  it('shows no departure board, and sends both Departures links to the train search for this station', async () => {
    vi.mocked(api.getStationName).mockResolvedValue('London Euston');
    await renderPage('EUS');

    expect(screen.queryByRole('button', { name: 'Scheduled departures' })).toBeNull();
    expect(screen.queryByRole('list', { name: 'Scheduled departures' })).toBeNull();
    const onThisPage = screen.getByRole('navigation', { name: 'On this page' });
    expect(within(onThisPage).getByRole('link', { name: 'Departures' })).toHaveAttribute('href', '/trains?station=EUS');
    expect(screen.getByRole('link', { name: 'See departures from London Euston' })).toHaveAttribute(
      'href',
      '/trains?station=EUS',
    );
    // The board fetched its rows client-side; nothing does now.
    expect(vi.mocked(fetch)).not.toHaveBeenCalled();
  });

  it('names the station by its code in the departures link when the name lookup fails', async () => {
    vi.mocked(api.getStationName).mockRejectedValue(new Error('500'));
    await renderPage('KGX');

    expect(screen.getByRole('link', { name: 'See departures from KGX' })).toHaveAttribute(
      'href',
      '/trains?station=KGX',
    );
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
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getStationAccessibility).mockResolvedValue({});
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);
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
    expect(screen.queryByText("This station isn't covered by our line-status tracking yet.")).not.toBeInTheDocument();
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
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
    vi.mocked(api.getStationAccessibility).mockResolvedValue({});
  });

  it('renders the "not part of our live departure sampling" copy when the route 404s', async () => {
    vi.mocked(api.getStationSampleStats).mockRejectedValue(
      new ApiNotFoundError('no sample data collected for station: KGX'),
    );
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText("This station isn't part of our live departure sampling.")).toBeInTheDocument();
    // LEG-23: no National Rail data shown, so no credit under it.
    expect(document.querySelector('[data-nre-credit]')).toBeNull();
  });

  it('renders the "no live departures currently recorded" copy for a covered-but-quiet board', async () => {
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('No live departures currently recorded at this station.')).toBeInTheDocument();
  });

  // LEG-23: the Schedule 1 credit sits directly under the LDBWS-derived block.
  it('credits National Rail directly under the live-sampled stats', async () => {
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);

    await renderPage();

    const credit = document.querySelector('#stats [data-nre-credit]');
    expect(credit).not.toBeNull();
    expect(visibleText(credit as Element)).toContain('powered by NationalRail (Train Information Services Ltd)');
    expect(credit?.querySelector('a')).toHaveAttribute('href', 'https://www.nationalrail.co.uk');
  });

  it('renders one row per operator in the order returned, resolving names via tocs with a bare-code fallback', async () => {
    const operatorStats: StationOperatorSampleStats[] = [
      {
        operator: 'GR',
        sampleAvailability: { state: 'available' },
        sampleStats: { total: 10, delayed: 2, cancelled: 0, skipped: 0, avgDelayMinutes: 3.5 },
        fullCoverageAvailability: { state: 'not-enabled' },
      },
      {
        operator: 'SR',
        sampleAvailability: { state: 'below-threshold', observed: 1, required: 3 },
        fullCoverageAvailability: { state: 'not-enabled' },
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

  it('prefers fullCoverageStats over sampleStats for a row that carries both, end to end through the real component tree', async () => {
    const operatorStats: StationOperatorSampleStats[] = [
      {
        operator: 'GR',
        sampleAvailability: { state: 'available' },
        sampleStats: { total: 10, delayed: 2, cancelled: 0, skipped: 0, avgDelayMinutes: 3.5 },
        fullCoverageStats: { total: 52, delayed: 6, cancelled: 1, skipped: 0, avgDelayMinutes: 2.1 },
        fullCoverageAvailability: { state: 'available' },
      },
      {
        operator: 'SR',
        sampleAvailability: { state: 'below-threshold', observed: 1, required: 3 },
        fullCoverageAvailability: { state: 'not-enabled' },
      },
    ];
    vi.mocked(api.getStationSampleStats).mockResolvedValue(operatorStats);
    vi.mocked(api.getAllTocs).mockResolvedValue([{ code: 'GR', name: 'LNER' }]);

    await renderPage();

    // 1/52 = 2% cancelled, avg 2.1 -- distinct from sampleStats' 0%/3.5,
    // so this proves the full-coverage numbers actually rendered, not the
    // sample ones, via formatSampleSummary's existing precedence chain.
    expect(screen.getByText('Avg delay 2.1 min · 2% cancelled')).toBeInTheDocument();
    expect(screen.queryByText('Avg delay 3.5 min · 0% cancelled')).not.toBeInTheDocument();
  });
});

describe('StationDisruptionPage -- accessibility & facilities', () => {
  // A fourth, independent coverage question again: this station has
  // ordinary line coverage and ordinary sampling throughout, so only
  // `getStationAccessibility` varies per test -- design spec Decision 9's
  // three honest states.
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);
  });

  it('renders the "not yet captured" copy when the route 404s', async () => {
    vi.mocked(api.getStationAccessibility).mockRejectedValue(
      new ApiNotFoundError('no station reference data for: KGX'),
    );

    await renderPage();

    expect(screen.getByText("We don't have station reference data for this station yet.")).toBeInTheDocument();
    expect(
      screen.queryByText('No accessibility or facilities details have been published for this station.'),
    ).not.toBeInTheDocument();
  });

  it('renders the "nothing published" copy for a row with no allowlisted keys', async () => {
    vi.mocked(api.getStationAccessibility).mockResolvedValue({});

    await renderPage();

    expect(
      screen.getByText('No accessibility or facilities details have been published for this station.'),
    ).toBeInTheDocument();
    expect(screen.queryByText("We don't have station reference data for this station yet.")).not.toBeInTheDocument();
  });

  it('renders grouped headings and at least one rendered value for a populated response, end to end through the real component tree', async () => {
    vi.mocked(api.getStationAccessibility).mockResolvedValue({
      stationAccessibility: { stepFree: true },
      carParks: [{ spaces: 120 }],
    });

    await renderPage();

    expect(screen.getByText('Step-free access & assistance')).toBeInTheDocument();
    expect(screen.getByText('Getting here')).toBeInTheDocument();
    expect(screen.getByText('Step free:')).toBeInTheDocument();
    expect(screen.getByText('Yes')).toBeInTheDocument();
    expect(screen.getByText('Car parks')).toBeInTheDocument();
    // review §3.5.2, re-verified end to end: the page's own h1 -> the
    // section's h2 -> its now-promoted h3 group titles, with no level
    // skipped in between.
    expect(screen.getByRole('heading', { level: 1, name: 'London Kings Cross (KGX)' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Accessibility & facilities' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 3, name: 'Step-free access & assistance' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 3, name: 'Getting here' })).toBeInTheDocument();
  });

  it('renders a deeply nested shape as labelled rows, no longer as a JSON dump', async () => {
    // This exact value used to produce a collapsed "Raw data" disclosure:
    // the old renderer bailed the whole object to `JSON.stringify` the
    // moment one own value was a nested object, which measured at 96.2% of
    // real key-renders
    // (docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md
    // §2.1). Shape dispatch recurses it instead.
    vi.mocked(api.getStationAccessibility).mockResolvedValue({
      lifts: { level1: { level2: { level3: 'too deep' } } },
    });

    await expect(renderPage()).resolves.toBeDefined();
    expect(screen.queryByRole('button', { name: /Raw data/ })).not.toBeInTheDocument();
    expect(screen.getByText('Level3:')).toBeInTheDocument();
    expect(screen.getByText('too deep')).toBeInTheDocument();
  });

  it('renders the raw-JSON last resort without throwing for a shape no pattern describes', async () => {
    // The wire type is twelve `unknown`s and `renderAccessibilityValue` is
    // a total function over `unknown`, so the terminal branch stays
    // reachable even though no real payload reaches it any more (§4.9).
    vi.mocked(api.getStationAccessibility).mockResolvedValue({
      lifts: new Date('2026-09-16'),
    });

    await expect(renderPage()).resolves.toBeDefined();
    // "Lifts: " prefix: the disclosure control's accessible name is
    // qualified by the field it belongs to, so that several "Raw data"
    // disclosures on one page stay distinguishable as landmarks -- see
    // `components/StationAccessibilitySection.tsx`'s `Disclosure`.
    expect(screen.getByRole('button', { name: 'Lifts: Raw data' })).toBeInTheDocument();
  });

  // Same stale-serving posture as every other section on this page: a
  // non-404 failure with nothing cached is not swallowed into a coverage
  // state, it propagates to app/error.tsx.
  it('throws (not "unavailable") for a non-404 failure with nothing cached', async () => {
    vi.mocked(api.getStationAccessibility).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await expect(renderPage()).rejects.toThrow('connect ECONNREFUSED');
  });

  it('keeps rendering the last-known facilities when a later fetch fails', async () => {
    vi.mocked(api.getStationAccessibility).mockResolvedValue({ staffAssistance: 'Available all day' });
    await renderPage();
    cleanup();

    vi.mocked(api.getStationAccessibility).mockRejectedValue(new Error('connect ECONNREFUSED'));

    await renderPage();
    expect(screen.getByText('Available all day')).toBeInTheDocument();
  });
});

describe('generateMetadata', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
    vi.mocked(api.getPreviewStopPointDisruption).mockResolvedValue([]);
  });

  it('titles the page with the station name and gives an evergreen description and canonical', async () => {
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([report('ecml', 'East Coast Main Line')]);
    const metadata = await generateMetadata({ params: Promise.resolve({ crs: 'KGX' }) });
    expect(metadata.title).toBe('London Kings Cross (KGX)');
    expect(metadata.description).toBe('Disruption, accessibility and departures for London Kings Cross (KGX).');
    expect(metadata.alternates).toEqual({ canonical: '/stations/KGX' });
    expect(metadata.openGraph).toMatchObject({
      title: 'London Kings Cross (KGX) · Distant Signal',
      url: '/stations/KGX',
    });
    expect(metadata.twitter).toMatchObject({ card: 'summary_large_image' });
  });

  it('lists its own preview image, with the same alt text, versioned by the disruptions', async () => {
    vi.mocked(api.getStopPointDisruption).mockClear();
    vi.mocked(api.getPreviewStopPointDisruption).mockResolvedValue([]);
    const calm = await generateMetadata({ params: Promise.resolve({ crs: 'KGX' }) });
    const [image] = calm.openGraph?.images as { url: string; alt: string }[];
    expect(image).toEqual({
      url: expect.stringMatching(/^\/stations\/KGX\/card\.png\?v=[0-9a-f]{8}$/) as unknown,
      alt: 'London Kings Cross (KGX) on Distant Signal',
      width: 1200,
      height: 630,
      type: 'image/png',
    });

    vi.mocked(api.getPreviewStopPointDisruption).mockResolvedValue([report('ecml', 'East Coast Main Line')]);
    const disrupted = await generateMetadata({ params: Promise.resolve({ crs: 'KGX' }) });
    expect((disrupted.openGraph?.images as { url: string }[])[0]!.url).not.toBe(image!.url);
    expect(disrupted.description).toBe(calm.description);
    // The page's own per-visitor read is left to the page.
    expect(api.getStopPointDisruption).not.toHaveBeenCalled();
  });

  it('still lists the image when the disruptions are unreachable', async () => {
    vi.mocked(api.getPreviewStopPointDisruption).mockRejectedValue(new Error('down'));
    const metadata = await generateMetadata({ params: Promise.resolve({ crs: 'KGX' }) });
    expect((metadata.openGraph?.images as { url: string }[])[0]!.url).toMatch(/^\/stations\/KGX\/card\.png\?v=/);
  });

  it('never quotes the live status, which would go stale in a search snippet', async () => {
    vi.mocked(api.getStopPointDisruption).mockRejectedValue(new ApiNotFoundError('no line coverage'));
    const metadata = await generateMetadata({ params: Promise.resolve({ crs: 'KGX' }) });
    expect(metadata.description).toBe('Disruption, accessibility and departures for London Kings Cross (KGX).');
  });

  it('redirects a lower- or mixed-case code to the upper-case page', async () => {
    for (const crs of ['kgx', 'Kgx']) {
      permanentRedirectMock.mockClear();
      await expect(generateMetadata({ params: Promise.resolve({ crs }) })).rejects.toThrow('NEXT_REDIRECT');
      expect(permanentRedirectMock).toHaveBeenCalledWith('/stations/KGX');
      await expect(StationDisruptionPage({ params: Promise.resolve({ crs }) })).rejects.toThrow('NEXT_REDIRECT');
    }
  });

  it('calls notFound() for an unknown station, matching the page component', async () => {
    vi.mocked(api.getStationName).mockResolvedValue(null);
    notFoundMock.mockClear();
    await expect(generateMetadata({ params: Promise.resolve({ crs: 'ZZZ' }) })).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });
});

describe('structured data', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [], pinnedOperators: [] });
    vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
    vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
    vi.mocked(api.getStationAccessibility).mockResolvedValue({});
    vi.mocked(api.getAllTocs).mockResolvedValue([]);
    vi.mocked(api.getTransitRailLinks).mockResolvedValue([]);
    process.env.NEXT_PUBLIC_SITE_URL = 'https://ds.example';
  });

  afterEach(() => {
    delete process.env.NEXT_PUBLIC_SITE_URL;
  });

  it('renders a TrainStation and its breadcrumbs as JSON-LD', async () => {
    const { container } = await renderPage();
    const scripts = container.querySelectorAll('script[type="application/ld+json"]');
    expect(scripts).toHaveLength(1);
    const data = JSON.parse(scripts[0]?.textContent ?? '') as { '@type': string }[];
    expect(data.map((item) => item['@type'])).toEqual(['TrainStation', 'BreadcrumbList']);
    expect(data[0]).toMatchObject({
      name: 'London Kings Cross',
      identifier: 'KGX',
      url: 'https://ds.example/stations/KGX',
    });
  });
});
