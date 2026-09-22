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
  it('renders one counter tile per severity group, each linking to /lines?statusGroup=<group>', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());

    // A plain count-only regex (e.g. /0/) matches four of the five tiles at
    // once here, since only 'severe' has a nonzero count -- so each lookup
    // pairs the count with its bucket's own label (both rendered inside the
    // same tile) to uniquely identify one link, per the brief's own
    // fallback note for this test.
    expect(screen.getByRole('link', { name: /0.*Good Service/ })).toHaveAttribute('href', '/lines?statusGroup=good');
    expect(screen.getByRole('link', { name: /0.*Informational/ })).toHaveAttribute(
      'href',
      '/lines?statusGroup=informational',
    );
    expect(screen.getByRole('link', { name: /0.*Planned/ })).toHaveAttribute('href', '/lines?statusGroup=planned');
    expect(screen.getByRole('link', { name: /0.*Minor Disruption/ })).toHaveAttribute(
      'href',
      '/lines?statusGroup=mild',
    );
    expect(screen.getByRole('link', { name: /1.*Severe Disruption/ })).toHaveAttribute(
      'href',
      '/lines?statusGroup=severe',
    );
  });

  it('shows the good-service empty state when nothing is affected', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('Every line is running a Good Service.')).toBeInTheDocument();
  });

  it('lists affected lines worst-first, unbounded (no five-row cap)', async () => {
    const reports = Array.from({ length: 8 }, (_, i) =>
      report({ id: `line-${i}`, name: `Line ${i}`, lineStatuses: [status({ statusSeverity: 2 })] }),
    );
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    renderWithMantine(await NetworkStatusPage());
    for (const r of reports) {
      expect(screen.getByRole('link', { name: new RegExp(r.name) })).toBeInTheDocument();
    }
  });

  it('excludes a merged TfL id from both the counts and the mode breakdown', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line (TfL)', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('0 lines tracked across National Rail and TfL right now.')).toBeInTheDocument();
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
