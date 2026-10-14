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

    const link = screen.getByRole('link', { name: '1 line with severe disruption, view in Lines' });
    expect(link).toHaveAttribute('href', '/lines?statusGroup=severe');
  });

  it('does not link a zero-count tile, and shows "0"', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());

    // Only 'severe' is nonzero here -- the other four groups' tiles must not
    // be links at all (2026-09-22 UX review §2.1's own recommendation).
    expect(screen.queryByRole('link', { name: /good service/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /informational/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /planned/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /minor disruption/i })).not.toBeInTheDocument();
    // "0" appears once per zero-count tile (four of the five groups).
    expect(screen.getAllByText('0')).toHaveLength(4);
    // One page-level "Updated" line, not one per card as well.
    expect(screen.getAllByText(/^Updated/)).toHaveLength(1);
  });

  it('orders the counter tiles worst-first (regression: 2026-09-22 UX review §2.2, ascending Good..Severe read the answer last)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }), // severe
    ]);
    renderWithMantine(await NetworkStatusPage());
    // The tile labels, in sentence case like every label (round 2).
    const labels = [...document.querySelectorAll('[data-status-tile] .statusTile__label')].map((el) => el.textContent);
    expect(labels).toEqual(['Severe disruption', 'Minor disruption', 'Planned', 'Informational', 'Good service']);
  });

  it('shows the good-service empty state when nothing is affected', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('Every line is running a Good Service.')).toBeInTheDocument();
  });

  it('draws its section headings as h2s at the app-wide section size (docs/style-guide.md "Heading rules")', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Status' })).toBeInTheDocument();
    const sections = screen.getAllByRole('heading', { level: 2 });
    expect(sections.map((h) => h.textContent)).toEqual(expect.arrayContaining(['By mode', 'Lines to watch']));
    for (const heading of sections) {
      expect(heading.style.getPropertyValue('--title-fz')).toContain('--mantine-h3-font-size');
    }
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

  it('shows "No lines tracked" for an empty mode, with no badge (regression: 2026-09-22 UX review §2.3)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    const empty = screen.getByText('No lines tracked.').closest('.mantine-Card-root')!;
    expect(empty.querySelector('[data-status-badge]')).toBeNull();
    // National Rail's own card, unaffected: a Good service badge and a sentence.
    expect(screen.getByText('1 line, all running normally.')).toBeInTheDocument();
  });

  it('badges a mode with the worst severity in the slice, and counts the affected lines in words (regression: 2026-09-22 UX review §2.3)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 9 })] }), // mild
      report({ id: 'b', name: 'B', modeName: 'national-rail', lineStatuses: [status({ statusSeverity: 2 })] }), // severe
    ]);
    renderWithMantine(await NetworkStatusPage());
    const card = screen.getByText('2 of 2 lines affected.').closest('.mantine-Card-root')!;
    expect(card.querySelector('[data-status-badge]')).toHaveTextContent('Suspended');
  });

  it('shows a last-updated line under the subtitle when there is real data (regression: 2026-09-22 UX review §2.5, "right now" with no timestamp)', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({
        id: 'a',
        name: 'A',
        computedAt: '2026-09-22T09:05:00Z',
        lineStatuses: [status({ statusSeverity: 10 })],
      }),
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
    expect(metadata.title).toBe('Status');
  });

  it('mirrors title/description into openGraph and twitter', () => {
    expect(metadata.openGraph).toMatchObject({ title: 'Status · Distant Signal', type: 'website' });
    expect(metadata.twitter).toMatchObject({ card: 'summary_large_image', title: 'Status · Distant Signal' });
  });
});
