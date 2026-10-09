import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AllLinesPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatusReport, LineSummary, Suggestion, Preferences } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getAllLines: vi.fn(),
    getPreferences: vi.fn(),
    getLineStatusForMode: vi.fn(),
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

// AllLinesTable renders a PinToggle per row, which calls useRouter() from
// next/navigation -- same workaround AllLinesTable.test.tsx itself uses
// (that hook throws outside a real Next.js App Router tree). PinToggle also
// unconditionally renders LoginPromptModal, which calls useLoginHref() --
// and therefore usePathname()/useSearchParams() -- on every render
// regardless of whether the modal is open (see LoginPromptModal's own doc
// comment), so both stubs are needed here too even though this file's own
// tests never exercise the login-prompt path directly.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/lines',
  useSearchParams: () => new URLSearchParams(''),
}));

const lines: LineSummary[] = [
  { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
];
// `Preferences` requires both `pinnedLines` and `pinnedStations` -- see
// every other fixture of this type across the test suite (e.g.
// `app/page.test.tsx`, `components/PinToggle.test.tsx`).
const preferences: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
const reports: LineStatusReport[] = [];
const tocs: Suggestion[] = [{ code: 'VT', name: 'Avanti West Coast' }];

async function renderPage(searchParams: Record<string, string | string[]> = {}) {
  return renderWithMantine(await AllLinesPage({ searchParams: Promise.resolve(searchParams) }));
}

describe('AllLinesPage', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getPreferences).mockResolvedValue(preferences);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
  });

  it('renders "Incident Archive" and "New custom line" links, sharing a row with the page title', async () => {
    await renderPage();

    const newLineLink = screen.getByRole('link', { name: 'New custom line' });
    expect(newLineLink).toHaveAttribute('href', '/lines/new');
    const incidentsLink = screen.getByRole('link', { name: 'Incidents' });
    expect(incidentsLink).toHaveAttribute('href', '/incidents');
    const heading = screen.getByRole('heading', { name: 'Lines', level: 1 });
    // The two links share an inner Group with each other, and that inner
    // Group is itself a sibling of the heading in the same outer row --
    // same "shared parent row" assertion style CustomLineForm.test.tsx uses
    // for its Cancel/submit pairing, extended one level for the nested
    // Group these two links now share (plan Task 5, Step 6).
    expect(newLineLink.parentElement).toBe(incidentsLink.parentElement);
    expect(newLineLink.parentElement?.parentElement).toBe(heading.parentElement);
  });

  it('no longer renders CustomLineForm inline on this page', async () => {
    await renderPage();

    expect(screen.queryByLabelText('Name')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create line' })).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'New Custom Line' })).not.toBeInTheDocument();
  });

  // Outage behaviour (design spec Decision 5 / plan Task 5).
  it('keeps rendering the last-known line status when the status fetch fails', async () => {
    // One good render populates the stale cache...
    await renderPage();
    cleanup();
    vi.mocked(api.getLineStatusForMode).mockRejectedValue(new Error('connect ECONNREFUSED'));
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('connect ECONNREFUSED'));

    // ...so the next one survives the outage instead of throwing to
    // app/error.tsx.
    await renderPage();
    expect(screen.getByRole('heading', { name: 'Lines', level: 1 })).toBeInTheDocument();
  });

  it('still renders, with nothing pinned, when getPreferences fails', async () => {
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'Lines', level: 1 })).toBeInTheDocument();
  });

  it('still renders when the TOC reference lookup fails', async () => {
    vi.mocked(api.getAllTocs).mockRejectedValue(new Error('500'));

    await renderPage();
    expect(screen.getByRole('heading', { name: 'Lines', level: 1 })).toBeInTheDocument();
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Lines');
  });

  it('describes the whole-network line table rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Every National Rail and TfL line, with its current status, delays and cancellations.',
    );
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // See the equivalent case in app/incidents/page.test.tsx for why the
    // mirror is asserted against literals rather than against
    // `metadata.title`/`.description`.
    expect(metadata.openGraph).toMatchObject({
      title: 'Lines · Distant Signal',
      description: 'Every National Rail and TfL line, with its current status, delays and cancellations.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary_large_image',
      title: 'Lines · Distant Signal',
      description: 'Every National Rail and TfL line, with its current status, delays and cancellations.',
    });
  });
});

describe('statusGroup deep link', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    // Fixture with multiple lines: wcml (mild) and gwr (severe)
    const statusGroupTestLines: LineSummary[] = [
      { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
      { id: 'gwr', name: 'Great Western Railway', category: 'Long Distance', operators: ['GW'], source: 'catalogue' },
    ];
    const statusGroupTestReports: LineStatusReport[] = [
      {
        $type: 'DistantSignal.LineStatusReport',
        id: 'wcml',
        name: 'West Coast Main Line',
        modeName: 'national-rail',
        operators: [],
        computedAt: '2026-07-15T09:00:00Z',
        lineStatuses: [
          {
            statusSeverity: 9,
            statusSeverityDescription: 'Minor Delays',
            reason: '',
            dataQuality: 'knowledgebase',
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
            validityPeriods: [],
            sampleStats: { total: 10, delayed: 2, cancelled: 1, skipped: 0, avgDelayMinutes: 5 },
          },
        ],
      },
      {
        $type: 'DistantSignal.LineStatusReport',
        id: 'gwr',
        name: 'Great Western Railway',
        modeName: 'national-rail',
        operators: [],
        computedAt: '2026-07-15T09:00:00Z',
        lineStatuses: [
          {
            statusSeverity: 2,
            statusSeverityDescription: 'Suspended',
            reason: '',
            dataQuality: 'knowledgebase',
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
            validityPeriods: [],
            sampleStats: { total: 10, delayed: 5, cancelled: 3, skipped: 0, avgDelayMinutes: 20 },
          },
        ],
      },
    ];
    vi.mocked(api.getAllLines).mockResolvedValue(statusGroupTestLines);
    vi.mocked(api.getPreferences).mockResolvedValue(preferences);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(statusGroupTestReports);
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
  });

  it('pre-selects the AllLinesTable status filter from a valid ?statusGroup= value', async () => {
    // wcml has statusSeverity 9 ("Minor Delays") = 'mild'; gwr has
    // statusSeverity 2 ("Suspended") = 'severe'. Rendering with
    // statusGroup=severe should pre-filter to show only gwr.
    await renderPage({ statusGroup: 'severe' });
    expect(screen.getByRole('link', { name: 'Great Western Railway' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'West Coast Main Line' })).not.toBeInTheDocument();
  });

  it('ignores an unrecognized ?statusGroup= value rather than erroring', async () => {
    await renderPage({ statusGroup: 'not-a-real-group' });
    expect(screen.getByRole('heading', { name: 'Lines', level: 1 })).toBeInTheDocument();
  });
});
