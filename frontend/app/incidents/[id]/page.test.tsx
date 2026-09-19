import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentDetailPage, { generateMetadata } from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import { notFound } from 'next/navigation';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentDetail, Suggestion } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getIncident: vi.fn(), getAllTocs: vi.fn(), getStationName: vi.fn() };
});

// Neutral defaults matching this page's own fail-soft posture (both
// `getAllTocs`/`getStationName` calls are wrapped in `.catch(...)` in
// `page.tsx`, degrading to "no name resolved" rather than failing the
// page) -- every test gets these unless it overrides them, so a test that
// doesn't care about operator/station names still exercises the exact
// "reference data unavailable" branch this page falls back to in
// production whenever `API_BASE_URL` genuinely has nothing to say.
beforeEach(() => {
  vi.mocked(api.getAllTocs).mockResolvedValue([]);
  vi.mocked(api.getStationName).mockResolvedValue(null);
});
// No throwing mock, no useRouter -- this page renders no client component
// that needs useRouter (unlike `/lines/[id]/page.test.tsx`'s
// DeleteLineButton case). notFound() is a plain no-op here; the page's own
// unconditional `throw err;` after calling it is what makes the promise
// actually reject in this mocked environment (real Next.js relies on
// notFound() throwing its own internal error instead).
vi.mock('next/navigation', () => ({ notFound: vi.fn() }));

function detail(overrides: Partial<IncidentDetail> = {}): IncidentDetail {
  return {
    incidentId: '12345',
    summary: 'Signal failure at Woking',
    description: '<p>Delays expected</p>',
    operators: ['VT'],
    affectedStations: ['WOK', 'WAT'],
    priority: 3,
    validityPeriods: [{ fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true }],
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-08-30T09:00:00Z',
    fetchedAt: '2026-08-31T10:15:00Z',
    currentlyAffectsLines: [{ id: 'south-western', name: 'South Western Main Line' }],
    history: [
      {
        summary: 'Signal failure at Woking',
        description: '<p>Delays expected</p>',
        operators: ['VT'],
        affectedStations: ['WOK', 'WAT'],
        priority: 3,
        validityPeriods: [{ fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true }],
        isPlanned: false,
        isCleared: false,
        recordedAt: '2026-08-30T09:00:00Z',
      },
    ],
    ...overrides,
  };
}

describe('IncidentDetailPage', () => {
  it('calls notFound() when getIncident throws ApiNotFoundError', async () => {
    vi.mocked(api.getIncident).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(IncidentDetailPage({ params: Promise.resolve({ id: 'does-not-exist' }) })).rejects.toThrow();
    // The above only proves the promise rejects, which the page's own
    // unconditional `throw err;` after calling notFound() would cause even
    // if notFound() were never invoked (it's a mocked no-op here). Assert
    // it was actually called so this test can't pass for the wrong reason.
    expect(vi.mocked(notFound)).toHaveBeenCalled();
  });

  it('renders the summary, description, and affected stations', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('Signal failure at Woking')).toBeInTheDocument();
    expect(screen.getByText('Delays expected')).toBeInTheDocument();
    expect(screen.getByText('WOK')).toBeInTheDocument();
    // Review §2.9: bare CRS pills with nothing on the page to say what
    // they are -- this heading is what makes them read as station
    // identifiers rather than three floating, unexplained letters.
    expect(screen.getByText('Affected stations')).toBeInTheDocument();
  });

  it('renders no "Affected stations" heading when the incident has none', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ affectedStations: [] }));
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.queryByText('Affected stations')).not.toBeInTheDocument();
  });

  // Review §2.9: "fetched" is the poller's own word, not a passenger's.
  it('labels the fetch timestamp in reader-facing terms, not the poller\'s own word', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText(`Last updated from National Rail: ${formatDateTime('2026-08-31T10:15:00Z')}`)).toBeInTheDocument();
  });

  it('renders a link to each currently-affected line', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByRole('link', { name: 'South Western Main Line' })).toHaveAttribute('href', '/lines/south-western');
  });

  it('renders the "not currently reported anywhere" empty state', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ currentlyAffectsLines: [] }));
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('Not currently reported on any tracked line.')).toBeInTheDocument();
  });

  it('renders the history timeline with at least the first-seen entry', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('First seen')).toBeInTheDocument();
  });

  // The single-entry fixtures above never exercise `formatValidityPeriod`'s
  // date-range branch (both a `fromDate` and a non-null `toDate`) or
  // `describeChanges`'s actual diff branch (two entries with genuinely
  // different field values) -- both were previously only verified by code
  // inspection. This covers both with two validity periods and two history
  // entries that differ in `priority`.
  it('renders a date-range validity period and a diffed history entry', async () => {
    const olderEntry = {
      summary: 'Signal failure at Woking',
      description: '<p>Delays expected</p>',
      operators: ['VT'],
      affectedStations: ['WOK', 'WAT'],
      priority: 3,
      validityPeriods: [{ fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true }],
      isPlanned: false,
      isCleared: false,
      recordedAt: '2026-08-30T09:00:00Z',
    };
    const newerEntry = { ...olderEntry, priority: 5, recordedAt: '2026-08-31T10:00:00Z' };

    const rangedPeriod = { fromDate: '2026-08-20T08:00:00Z', toDate: '2026-08-20T10:00:00Z', isNow: false };
    const ongoingPeriod = { fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true };

    vi.mocked(api.getIncident).mockResolvedValue(
      detail({
        validityPeriods: [rangedPeriod, ongoingPeriod],
        history: [newerEntry, olderEntry],
      }),
    );
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));

    expect(
      screen.getByText(`${formatDateTime(rangedPeriod.fromDate)} – ${formatDateTime(rangedPeriod.toDate)}`),
    ).toBeInTheDocument();
    expect(screen.getByText(`${formatDateTime(ongoingPeriod.fromDate)} – ongoing`)).toBeInTheDocument();

    expect(screen.getByText('priority changed from 3 to 5')).toBeInTheDocument();
    expect(screen.getByText('First seen')).toBeInTheDocument();
  });

  // Review §2.12: the timezone note appears once for the whole "First
  // seen"/"Last updated from National Rail" section, not once per
  // timestamp, even though the page also shows several other timestamps
  // (validity periods, history entries) that are not this note's target
  // section.
  it('states the UK-local-time note exactly once, next to First seen/Last updated', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getAllByText('Times in UK local time')).toHaveLength(1);
  });

  // Review §3.3: the detail page was a dead end -- no back link or
  // breadcrumb, and this is the page most likely to be reached from a
  // shared URL with no browser history to go back to.
  it('renders a back link to the incident archive', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByRole('link', { name: /Incident Archive/ })).toHaveAttribute('href', '/incidents');
  });

  // Review §3.3's "at-a-glance strip": the page previously rendered neither
  // `incident.operators` nor `incident.isCleared` at all, even though both
  // are already public and already shown on the archive rows.
  describe('the at-a-glance strip (review §3.3)', () => {
    it('shows an Active badge for a live incident and a Cleared one for a cleared incident', async () => {
      vi.mocked(api.getIncident).mockResolvedValue(detail({ isCleared: false }));
      renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(screen.queryByText('Cleared')).not.toBeInTheDocument();
    });

    it('shows a Cleared badge, and replaces "Currently affects" with a cleared notice, once isCleared is true', async () => {
      vi.mocked(api.getIncident).mockResolvedValue(
        detail({ isCleared: true, currentlyAffectsLines: [{ id: 'south-western', name: 'South Western Main Line' }] }),
      );
      renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
      expect(screen.getByText('Cleared')).toBeInTheDocument();
      expect(screen.queryByText('Active')).not.toBeInTheDocument();
      expect(screen.getByText('This incident has been cleared.')).toBeInTheDocument();
      // The line the incident used to affect must not still be listed --
      // a cleared incident with an "ongoing" validity period would
      // otherwise read as a live, unresolved contradiction.
      expect(screen.queryByText('Currently affects')).not.toBeInTheDocument();
      expect(screen.queryByRole('link', { name: 'South Western Main Line' })).not.toBeInTheDocument();
    });

    it('resolves an operator code to its name via the TOC reference lookup, falling back to the bare code', async () => {
      const tocs: Suggestion[] = [{ code: 'VT', name: 'Avanti West Coast' }];
      vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
      vi.mocked(api.getIncident).mockResolvedValue(detail({ operators: ['VT', 'ZZ'] }));
      renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
      expect(screen.getByText('Operators: Avanti West Coast (VT), ZZ')).toBeInTheDocument();
    });

    it('renders no Operators line for an incident with no attributed operator', async () => {
      vi.mocked(api.getIncident).mockResolvedValue(detail({ operators: [] }));
      renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
      expect(screen.queryByText(/^Operators:/)).not.toBeInTheDocument();
    });

    // Review §2.9: bare CRS pills ("KGX", "EUS") floating in the page with
    // nothing to say what they are. Now labelled "Name (CRS)" -- the same
    // fallback contract `stationLabel` uses everywhere else in this app --
    // rather than just a hover-only `title` tooltip.
    it('labels an affected-station badge with its resolved name and code', async () => {
      vi.mocked(api.getStationName).mockImplementation(async (crs) =>
        crs === 'WOK' ? 'Woking' : null,
      );
      vi.mocked(api.getIncident).mockResolvedValue(detail({ affectedStations: ['WOK', 'WAT'] }));
      renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
      expect(screen.getByText('Woking (WOK)')).toBeInTheDocument();
      // WAT never resolved a name -- falls back to the bare code.
      expect(screen.getByText('WAT')).toBeInTheDocument();
    });
  });

  // Review §3.3: "History" used to render as a heading with nothing under
  // it whenever `incident.history` was empty -- the detail-page spec
  // assumed at least a first-seen snapshot always exists, but a freshly
  // ingested incident can be caught between ingest and its first change.
  it('renders a fallback message when history is empty, instead of a bare heading', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ history: [] }));
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('No changes recorded since this incident was first seen.')).toBeInTheDocument();
  });

  // Review §3.3 (a11y): "Validity", "Currently affects", "History" and
  // "Affected stations" used to be `Text fw={500}`, not real headings --
  // the document outline was a lone `<h1>` and nothing else.
  it('renders the section captions as real headings, not styled text', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByRole('heading', { level: 2, name: 'Affected stations' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Validity' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Currently affects' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'History' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Signal failure at Woking' })).toBeInTheDocument();
  });
});

describe('generateMetadata', () => {
  it('titles the page with the incident summary and describes the affected lines', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    const metadata = await generateMetadata({ params: Promise.resolve({ id: '12345' }) });
    expect(metadata.title).toBe('Signal failure at Woking — Distant Signal');
    expect(metadata.description).toBe('Real-Time incident affecting South Western Main Line.');
    expect(metadata.openGraph?.title).toBe('Signal failure at Woking — Distant Signal');
    expect(metadata.twitter).toMatchObject({ card: 'summary' });
  });

  it('labels a planned-work incident distinctly from a real-time one', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ isPlanned: true }));
    const metadata = await generateMetadata({ params: Promise.resolve({ id: '12345' }) });
    expect(metadata.description).toBe('Planned Work incident affecting South Western Main Line.');
  });

  it('falls back to the summary when no line currently reports the incident', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ currentlyAffectsLines: [] }));
    const metadata = await generateMetadata({ params: Promise.resolve({ id: '12345' }) });
    expect(metadata.description).toBe('Real-Time incident: Signal failure at Woking.');
  });

  it('calls notFound() when getIncident throws ApiNotFoundError, matching the page component', async () => {
    vi.mocked(api.getIncident).mockRejectedValue(new ApiNotFoundError('not found'));
    vi.mocked(notFound).mockClear();
    await expect(generateMetadata({ params: Promise.resolve({ id: 'does-not-exist' }) })).rejects.toThrow();
    expect(vi.mocked(notFound)).toHaveBeenCalled();
  });
});
