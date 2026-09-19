import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentsPage, { metadata } from './page';
import * as api from '@/lib/api';
import type { LineSummary, Suggestion } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getAllLines: vi.fn(), getAllTocs: vi.fn() };
});

// `IncidentSearchForm` (review §3.3) now runs its own search on mount, so
// rendering this page always fires a `fetch` -- stubbed here the same way
// `IncidentSearchForm.test.tsx` stubs it, rather than letting the real
// global `fetch` reject on a relative URL every time this page renders.
const fetchMock = vi.fn();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockReset();
  fetchMock.mockResolvedValue({
    ok: true,
    status: 200,
    json: () => Promise.resolve({ results: [], nextCursor: null }),
  } as Response);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const TEST_LINES: LineSummary[] = [
  { id: 'south-western', name: 'South Western Main Line', category: 'main', operators: ['SW'], source: 'catalogue' },
];
const TEST_TOCS: Suggestion[] = [{ code: 'SW', name: 'South Western Railway' }];

describe('IncidentsPage', () => {
  it('renders the heading and the search form with fetched reference data', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
    // Waits out `IncidentSearchForm`'s own mount-triggered auto-search
    // (review §3.3) so its state update lands before this test (and RTL's
    // unmount) finishes, rather than racing cleanup.
    expect(await screen.findByRole('button', { name: 'Search' })).toBeInTheDocument();
  });

  it('degrades to empty reference-data lists if either fetch fails, rather than crashing the page', async () => {
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('network error'));
    vi.mocked(api.getAllTocs).mockRejectedValue(new Error('network error'));

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
    await screen.findByRole('button', { name: 'Search' });
  });

  it('passes searchParams through as initial filter values', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(
      await IncidentsPage({
        searchParams: Promise.resolve({ operator: 'SW,VT', line: 'south-western' }),
      }),
    );

    // The MultiSelect renders its selected values as removable pills with
    // this exact label text -- confirms initialOperator was parsed and
    // passed through rather than dropped. Scoped to the pill label class:
    // Mantine also keeps its (closed, `display: none`) options list mounted
    // in the DOM with the same "CODE — Name" text, so an unscoped
    // `getByText` matches both.
    expect(
      screen.getByText('SW — South Western Railway', { selector: '.mantine-Pill-label' }),
    ).toBeInTheDocument();
    await screen.findByRole('button', { name: 'Search' });
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Incident Archive — Distant Signal');
  });

  it('describes the cross-network archive search rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Search National Rail incident messages across the whole network, filtered by operator, line and date range — the last 30 days by default, or everything this app has ever ingested.',
    );
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // Next merges page metadata into the root layout's PER FIELD, and
    // `app/layout.tsx` has no `openGraph`/`twitter` at all -- so a page
    // that set only `title`/`description` would unfurl with no og:title
    // whatsoever. Asserting the mirror (rather than just "openGraph
    // exists") is what stops the three copies drifting apart. Spelled as
    // literals rather than as `metadata.title`/`.description`: those read
    // the same two consts the subject does, so a self-comparison would be
    // structurally incapable of failing.
    expect(metadata.openGraph).toMatchObject({
      title: 'Incident Archive — Distant Signal',
      description:
        'Search National Rail incident messages across the whole network, filtered by operator, line and date range — the last 30 days by default, or everything this app has ever ingested.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Incident Archive — Distant Signal',
      description:
        'Search National Rail incident messages across the whole network, filtered by operator, line and date range — the last 30 days by default, or everything this app has ever ingested.',
    });
  });
});
