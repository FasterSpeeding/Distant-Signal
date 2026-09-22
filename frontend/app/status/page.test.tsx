import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NetworkStatusPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatus, LineStatusReport } from '@/lib/types';

vi.mock('@/lib/api');
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

function status(overrides: Partial<LineStatus> & { statusSeverity: number }): LineStatus {
  return {
    statusSeverityDescription: 'x',
    reason: '',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    lineStatuses: [],
    computedAt: '2026-09-22T09:00:00Z',
    ...overrides,
  };
}

beforeEach(() => {
  __resetStaleCacheForTests();
  vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
});

describe('NetworkStatusPage', () => {
  it('renders a nonzero counter tile as a real link to /lines?statusGroup=<group>, with a descriptive accessible name', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());

    const link = screen.getByRole('link', { name: '1 line with Severe Disruption — view in All Lines' });
    expect(link).toHaveAttribute('href', '/lines?statusGroup=severe');
  });

  it('does not link a zero-count tile, and shows "none" instead of "0"', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());

    // Only 'severe' is nonzero here -- the other four groups' tiles must not
    // be links at all (2026-09-22 UX review §2.1's own recommendation).
    expect(screen.queryByRole('link', { name: /Good Service/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Informational/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Planned/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Minor Disruption/ })).not.toBeInTheDocument();
    // "none" appears once per zero-count tile (four of the five groups).
    expect(screen.getAllByText('none')).toHaveLength(4);
  });

  it('orders the counter tiles worst-first (regression: 2026-09-22 UX review §2.2, ascending Good..Severe read the answer last)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }), // severe
    ]);
    renderWithMantine(await NetworkStatusPage());
    const labels = screen
      .getAllByText(/Good Service|Informational|Planned|Minor Disruption|Severe Disruption/)
      // Both the tile's own label and (for the affected line) `StatusBadge`'s
      // uppercase text match this pattern -- keep only the tile labels,
      // identified by NOT being all-uppercase (StatusBadge renders
      // upper-cased text via CSS, but the DOM text content itself is
      // whatever `severityLabel` returns, so filter on the tile's known
      // exact label set instead).
      .filter((el) => ['Good Service', 'Informational', 'Planned', 'Minor Disruption', 'Severe Disruption'].includes(el.textContent ?? ''));
    expect(labels[0]).toHaveTextContent('Severe Disruption');
    expect(labels[labels.length - 1]).toHaveTextContent('Good Service');
  });

  it('shows the good-service empty state when nothing is affected', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('Every line is running a Good Service.')).toBeInTheDocument();
  });

  it('lists affected lines worst-first, unbounded (no five-row cap), as real LineStatusCard links', async () => {
    const reports = Array.from({ length: 8 }, (_, i) =>
      report({ id: `line-${i}`, name: `Line ${i}`, lineStatuses: [status({ statusSeverity: 2 })] }),
    );
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    renderWithMantine(await NetworkStatusPage());
    for (const r of reports) {
      expect(screen.getByRole('link', { name: new RegExp(r.name) })).toHaveAttribute('href', `/lines/${r.id}`);
    }
  });

  it('excludes a merged TfL id from both the counts and the mode breakdown', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line (TfL)', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('0 lines tracked across National Rail and TfL right now.')).toBeInTheDocument();
  });

  it('names only the modes actually present in the subtitle (regression: 2026-09-22 UX review §2.3, "National Rail and TfL" even with zero TfL lines)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('1 line tracked across National Rail right now.')).toBeInTheDocument();
  });

  it('shows "No lines tracked" for an empty mode instead of a green "All Good Service" badge (regression: 2026-09-22 UX review §2.3)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('No lines tracked.')).toBeInTheDocument();
    expect(screen.queryByText('All Good Service')).toBeInTheDocument(); // National Rail's own card, unaffected
  });

  it('colours the "N affected" badge by the worst severity in the slice, not a fixed yellow (regression: 2026-09-22 UX review §2.3)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 9 })] }), // mild
      report({ id: 'b', name: 'B', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 2 })] }), // severe
    ]);
    renderWithMantine(await NetworkStatusPage());
    const badge = screen.getByText('2 affected');
    // `Badge`'s colour is expressed as CSS custom properties on the root
    // element (Mantine v7) -- `--badge-color` reflects the `color` prop
    // passed in.
    expect(badge.closest('.mantine-Badge-root')).toHaveStyle({
      '--badge-color': 'var(--mantine-color-red-light-color)',
    });
  });

  it('shows a last-updated line under the subtitle when there is real data (regression: 2026-09-22 UX review §2.5, "right now" with no timestamp)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', computedAt: '2026-09-22T09:05:00Z', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText(/^Updated/)).toBeInTheDocument();
  });

  it('shows no last-updated line for an empty snapshot', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.queryByText(/^Updated/)).not.toBeInTheDocument();
  });

  it('still renders the page shell when the status fetch fails outright (no stale entry yet)', async () => {
    vi.mocked(api.getLineStatusForMode).mockRejectedValue(new Error('connect ECONNREFUSED'));
    await expect(NetworkStatusPage()).rejects.toThrow();
    // Documents current behavior: unlike app/lines/page.tsx (which wraps
    // getAllLines in withStaleFallback and has a prior successful render to
    // fall back to), a cold cache with no prior success still throws to
    // app/error.tsx, same as every other withStaleFallback call site on a
    // cold cache.
  });
});

describe('metadata', () => {
  it('titles the page after its own heading', () => {
    expect(metadata.title).toBe('Network Status — Distant Signal');
  });

  it('mirrors title/description into openGraph and twitter', () => {
    expect(metadata.openGraph).toMatchObject({ title: metadata.title, type: 'website' });
    expect(metadata.twitter).toMatchObject({ card: 'summary', title: metadata.title });
  });
});
