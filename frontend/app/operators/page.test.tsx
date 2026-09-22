import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import OperatorsPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { OperatorSummary, Preferences, SessionInfo } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getAllOperators: vi.fn(),
    getPreferences: vi.fn(),
    getSession: vi.fn(),
  };
});
// `withStaleFallback` (lib/liveDataCache.ts) reads the session cookie via
// `next/headers` to scope its cache per visitor, and there is no Next
// request context in a unit test. Same stub shape lib/api.test.ts uses,
// plus the `.get()` the cache needs.
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

// OperatorStatusCard renders a PinToggle per card, which calls useRouter()
// from next/navigation -- same workaround that LineStatusCard tests use
// (that hook throws outside a real Next.js App Router tree). PinToggle also
// unconditionally renders LoginPromptModal, which calls useLoginHref() --
// and therefore usePathname()/useSearchParams() -- on every render
// regardless of whether the modal is open (see LoginPromptModal's own doc
// comment), so both stubs are needed here too even though this file's own
// tests never exercise the login-prompt path directly.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/operators',
  useSearchParams: () => new URLSearchParams(''),
}));

const operators: OperatorSummary[] = [
  {
    code: 'VT',
    name: 'Avanti West Coast',
    lineIds: ['wcml'],
    worstSeverity: 1,
    reason: 'Operational issues',
    sampleStats: { total: 100, delayed: 10, cancelled: 2, skipped: 0, avgDelayMinutes: 5 },
    computedAt: '2026-09-22T00:00:00Z',
  },
  {
    code: 'TW',
    name: 'Arriva Trains Wales',
    lineIds: ['cardiff-main'],
    worstSeverity: 0,
    reason: 'Good Service',
    sampleStats: { total: 100, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 2 },
    computedAt: '2026-09-22T00:00:00Z',
  },
];
// `Preferences` requires `pinnedLines`, `pinnedStations`, and `pinnedOperators` --
// this page defines its own NO_PREFERENCES constant already in this shape.
const preferences: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
const sessionInfo: SessionInfo = { authenticated: false, id: null, email: null, name: null };

async function renderPage() {
  return renderWithMantine(await OperatorsPage());
}

describe('OperatorsPage', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getAllOperators).mockResolvedValue(operators);
    vi.mocked(api.getPreferences).mockResolvedValue(preferences);
    vi.mocked(api.getSession).mockResolvedValue(sessionInfo);
  });

  it('renders one OperatorStatusCard per operator', async () => {
    await renderPage();

    const heading = screen.getByRole('heading', { name: 'Operators', level: 1 });
    expect(heading).toBeInTheDocument();

    // Two operators should render two cards
    for (const operator of operators) {
      expect(screen.getByText(operator.name)).toBeInTheDocument();
    }
  });

  it('renders the empty-state text when getAllOperators resolves to an empty array', async () => {
    vi.mocked(api.getAllOperators).mockResolvedValue([]);

    await renderPage();

    expect(screen.getByText('No operator status data available right now.')).toBeInTheDocument();
  });

  // Review M7/§2.6: alphabetical was fine at nine operators, but the
  // homepage source describes a full catalogue of 25-40 -- worst-first
  // matches every other status surface in the app (the dashboard's "Lines
  // to watch", the homepage's own pinned sections).
  it('orders cards worst-first, not alphabetically (review M7)', async () => {
    await renderPage();

    const headings = screen.getAllByText(/Avanti West Coast|Arriva Trains Wales/);
    // Avanti (worstSeverity 1 = "Closed" -> severe group) must render
    // before Arriva (worstSeverity 0 = "Special Service" -> informational
    // group) even though "Arriva" sorts first alphabetically.
    const names = headings.map((el) => el.textContent);
    expect(names.indexOf('Avanti West Coast')).toBeLessThan(names.indexOf('Arriva Trains Wales'));
  });

  it('renders an intro sentence under the heading (review M7)', async () => {
    await renderPage();

    expect(screen.getByText(/Every operator this app tracks/)).toBeInTheDocument();
  });

  it('still renders with nothing pinned when getPreferences fails', async () => {
    vi.mocked(api.getPreferences).mockRejectedValue(new Error('500'));

    await renderPage();

    const heading = screen.getByRole('heading', { name: 'Operators', level: 1 });
    expect(heading).toBeInTheDocument();

    // Cards should still render with the operators even though preferences fetch failed
    for (const operator of operators) {
      expect(screen.getByText(operator.name)).toBeInTheDocument();
    }
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Operators — Distant Signal');
  });

  it('describes the operator list rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Every train operator this app tracks — National Rail TOCs and TfL — with its current worst status and aggregate delay/cancellation figures at a glance.',
    );
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    expect(metadata.openGraph).toMatchObject({
      title: 'Operators — Distant Signal',
      description:
        'Every train operator this app tracks — National Rail TOCs and TfL — with its current worst status and aggregate delay/cancellation figures at a glance.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Operators — Distant Signal',
      description:
        'Every train operator this app tracks — National Rail TOCs and TfL — with its current worst status and aggregate delay/cancellation figures at a glance.',
    });
  });
});
