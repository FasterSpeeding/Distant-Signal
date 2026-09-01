import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentDetailPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import { notFound } from 'next/navigation';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentDetail } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getIncident: vi.fn() };
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
});
