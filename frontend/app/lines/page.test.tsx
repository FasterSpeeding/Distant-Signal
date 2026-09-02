import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AllLinesPage from './page';
import * as api from '@/lib/api';
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
const preferences: Preferences = { pinnedLines: [], pinnedStations: [] };
const reports: LineStatusReport[] = [];
const tocs: Suggestion[] = [{ code: 'VT', name: 'Avanti West Coast' }];

async function renderPage() {
  return renderWithMantine(await AllLinesPage());
}

describe('AllLinesPage', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getPreferences).mockResolvedValue(preferences);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
  });

  it('renders a "New custom line" link pointing at /lines/new, sharing a row with the page title', async () => {
    await renderPage();

    const link = screen.getByRole('link', { name: 'New custom line' });
    expect(link).toHaveAttribute('href', '/lines/new');
    const heading = screen.getByRole('heading', { name: 'All Lines', level: 1 });
    // Same "shared parent row" assertion style CustomLineForm.test.tsx
    // already uses for its Cancel/submit pairing.
    expect(link.parentElement).toBe(heading.parentElement);
  });

  it('no longer renders CustomLineForm inline on this page', async () => {
    await renderPage();

    expect(screen.queryByLabelText('Name')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create line' })).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'New Custom Line' })).not.toBeInTheDocument();
  });
});
